//! Avaliador: transforma leituras sucessivas em eventos.
//!
//! Regra central: só **transições** contam. Uma conquista dispara quando o valor
//! passa de abaixo da meta para >= meta entre duas leituras do mesmo perfil.
//! Ao anexar ou trocar de perfil, as primeiras leituras só servem de base
//! (progresso que já existia no save não desbloqueia nada).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::config::{Conquista, CorridaCfg};
use crate::processo::Valor;

/// Marcos de progresso que geram aviso (em %).
pub const MARCOS: [u8; 3] = [25, 50, 75];

/// Acima disto (m/s, ~540 km/h) um deslocamento entre leituras é teletransporte
/// (reiniciar corrida, ir para a garagem, câmera de replay), não movimento.
const VEL_MAX_MPS: f64 = 150.0;
/// Folga fixa do teste de teletransporte (m), para leituras muito próximas.
const FOLGA_TELEPORTE_M: f64 = 2.0;
/// Passos menores que isto (m) não contam no odômetro: tremidinha do carro parado.
const PASSO_MIN_M: f64 = 0.01;
/// Janela da média de velocidade (s): a leitura a 30 Hz não acompanha os quadros
/// do jogo, então a velocidade entre duas leituras seguidas oscila muito.
const JANELA_S: f64 = 0.5;
/// Velocidade só sai com pelo menos isto de histórico (s).
const JANELA_MIN_S: f64 = 0.25;
/// Cambalhota: com o z do vetor "pra cima" acima disto o carro está de pé; abaixo
/// do negativo, de cabeça para baixo. Entre os dois vale o último lado (histerese:
/// tremer de lado no chão não conta meia-volta).
const DE_PE: f32 = 0.5;
/// Giro maior que isto entre duas leituras (graus) é o jogo reposicionando o carro
/// (capotou e ficou preso, reiniciou), não cambalhota: a 30 Hz isso seria 2.700°/s.
const GIRO_MAX_GRAUS: f32 = 90.0;
/// De pé por este tempo (s) encerra a cambalhota (o giro volta a 0).
const FIM_GIRO_S: f64 = 1.0;
/// `TempoAcima`: quedas abaixo do limite (ou leituras falhas) mais curtas que
/// isto (s) não zeram a contagem. A velocidade calculada treme alguns km/h.
const TOLERANCIA_ABAIXO_S: f64 = 0.5;
/// A tabela de resultados só conta depois de ficar parada este tempo (s): o jogo
/// pode escrevê-la em mais de um quadro.
const RESULTADO_PARADO_S: f64 = 1.0;

/// Mudança de uma variável marcada com `registrar` (vai para o histórico).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mudanca {
    pub var: String,
    pub antes: i64,
    pub depois: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoDerivada {
    /// Soma de todos os aumentos de `fonte` (ex.: ganhos totais = soma de cada vez
    /// que o dinheiro subiu; gastar não desconta). Guardada por perfil.
    SomaAumentos,
    /// Metros percorridos pela posição `fonte` (odômetro). Guardada por perfil.
    Distancia,
    /// km/h da posição `fonte`, média da janela. Só ao vivo.
    Velocidade,
    /// Graus (múltiplos de 180) girados de ponta ou de lado na cambalhota atual,
    /// pelo vetor "pra cima" `fonte`. Só ao vivo.
    Giro,
    /// Voltas completas em cambalhotas, somadas. Guardada por perfil.
    Voltas,
    /// Segundos seguidos com `fonte` >= `limite`. Só ao vivo.
    TempoAcima { limite: i64 },
    /// Metros na horizontal do salto em andamento (posição `fonte`); 0 no chão.
    /// Só ao vivo.
    Salto,
    /// Metros de todos os saltos, somados no pouso. Guardada por perfil.
    SaltosSomados,
    /// Itens de uma lista que passam no `Filtro` (ex.: corridas URL vencidas,
    /// etapas concluídas). Vem do save, então é ao vivo, mas **nunca desce** enquanto
    /// o perfil não troca: se o jogo zerar a estrutura por um instante (recarregar
    /// a carreira) e voltar, a volta não parece progresso novo.
    Conta,
    /// Soma dos itens de uma lista (ex.: outruns vencidos em cada etapa). Ao vivo,
    /// nunca desce (como `Conta`).
    Soma,
    /// Corridas terminadas cujo resultado passa no `FiltroCorrida`. A tabela de
    /// resultados só muda quando uma corrida termina: cada mudança (depois da
    /// base) é uma corrida nova. Guardada por perfil.
    Corridas,
}

/// Quais itens a `Conta` conta.
#[derive(Debug, Clone, Default)]
pub struct Filtro {
    /// Só itens com este valor (`None` = diferentes de 0).
    pub igual: Option<i64>,
    /// Lista paralela de chaves: conta só os itens cuja chave está em `em`.
    pub chaves: Option<String>,
    pub em: HashSet<i64>,
    /// Só para `Corridas`.
    pub corrida: Option<FiltroCorrida>,
}

/// Quais corridas a `Corridas` conta.
#[derive(Debug, Clone, Default)]
pub struct FiltroCorrida {
    /// `pista / 100` tem de estar aqui (vazio = qualquer pista).
    pub pistas: Vec<i64>,
    pub vencer: bool,
    /// Segundos de vantagem sobre o 2º colocado (implica vencer).
    pub vantagem_min_s: Option<i64>,
    /// Todas as trocas de marcha perfeitas, e pelo menos uma.
    pub trocas_perfeitas: bool,
}

/// O que interessa do resultado do jogador numa corrida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resultado {
    pub pista: i64,
    pub posicao: i64,
    pub tempo: i64,
    pub venceu: bool,
    /// Segundos (em ticks) à frente do 2º colocado, se venceu e o 2º terminou.
    pub vantagem_ticks: Option<i64>,
    pub ticks_por_s: i64,
    pub trocas: i64,
    pub trocas_perfeitas: i64,
}

impl Resultado {
    /// Linha do jogador na tabela (`linhas` válidas de `c.largura` campos).
    fn da_tabela(c: &CorridaCfg, tabela: &[i64], linhas: usize, pista: i64) -> Option<Self> {
        let f = &c.campo;
        let mut validas = tabela.chunks_exact(c.largura).take(linhas);
        let j = validas
            .clone()
            .find(|l| l[f.id] == c.id_jogador && l[f.posicao] >= 1)?;
        let venceu = j[f.posicao] == 1 && j[f.status] == c.terminou;
        let vantagem_ticks = if venceu {
            validas
                .find(|l| l[f.posicao] == 2 && l[f.status] == c.terminou && l[f.tempo] > j[f.tempo])
                .map(|l| l[f.tempo] - j[f.tempo])
        } else {
            None
        };
        Some(Resultado {
            pista,
            posicao: j[f.posicao],
            tempo: j[f.tempo],
            venceu,
            vantagem_ticks,
            ticks_por_s: c.ticks_por_s,
            trocas: j[f.trocas],
            trocas_perfeitas: j[f.trocas_perfeitas],
        })
    }

    /// Mesmo resultado do jogador (a pista não entra: ela muda ao sair para o menu,
    /// e o jogo mexe na tabela nessa hora sem ser corrida nova).
    fn mesma_corrida(&self, o: &Resultado) -> bool {
        (
            self.posicao,
            self.tempo,
            self.venceu,
            self.vantagem_ticks,
            self.trocas,
            self.trocas_perfeitas,
        ) == (
            o.posicao,
            o.tempo,
            o.venceu,
            o.vantagem_ticks,
            o.trocas,
            o.trocas_perfeitas,
        )
    }

    fn passa(&self, f: &FiltroCorrida) -> bool {
        (f.pistas.is_empty() || f.pistas.contains(&self.pista.div_euclid(100)))
            && (!f.vencer || self.venceu)
            && f.vantagem_min_s.is_none_or(|s| {
                self.vantagem_ticks
                    .is_some_and(|v| v >= s.saturating_mul(self.ticks_por_s))
            })
            && (!f.trocas_perfeitas || (self.trocas > 0 && self.trocas_perfeitas == self.trocas))
    }
}

/// Estatística calculada por nós.
#[derive(Debug, Clone)]
pub struct Derivada {
    pub nome: String,
    pub fonte: String,
    pub tipo: TipoDerivada,
    /// Saltos: variável `floats` com a altura das rodas.
    pub rodas: Option<String>,
    /// Saltos: variável `vec3` com a velocidade do carro.
    pub velocidade_vetor: Option<String>,
    /// Só para `Conta`.
    pub filtro: Filtro,
}

impl Derivada {
    fn guardada(&self) -> bool {
        matches!(
            self.tipo,
            TipoDerivada::SomaAumentos
                | TipoDerivada::Distancia
                | TipoDerivada::Voltas
                | TipoDerivada::SaltosSomados
                | TipoDerivada::Corridas
        )
    }

    fn usa_voo(&self) -> bool {
        matches!(self.tipo, TipoDerivada::Salto | TipoDerivada::SaltosSomados)
    }

    fn usa_rodopio(&self) -> bool {
        matches!(self.tipo, TipoDerivada::Giro | TipoDerivada::Voltas)
    }

    /// `Conta`/`Soma` desta leitura. `None` se a lista (ou as chaves) falhou.
    fn de_lista(&self, vars: &HashMap<String, Valor>) -> Option<i64> {
        let Some(Valor::Inteiros(itens)) = vars.get(&self.fonte) else {
            return None;
        };
        if self.tipo == TipoDerivada::Soma {
            return Some(itens.iter().fold(0i64, |a, &b| a.saturating_add(b)));
        }
        let chaves = match &self.filtro.chaves {
            Some(c) => match vars.get(c) {
                Some(Valor::Inteiros(k)) if k.len() == itens.len() => Some(k),
                _ => return None,
            },
            None => None,
        };
        let n = itens
            .iter()
            .enumerate()
            .filter(|&(_, &v)| match self.filtro.igual {
                Some(x) => v == x,
                None => v != 0,
            })
            .filter(|&(i, _)| chaves.is_none_or(|k| self.filtro.em.contains(&k[i])))
            .count();
        i64::try_from(n).ok()
    }
}

/// Histórico recente de uma posição.
#[derive(Debug, Default)]
struct Rastro {
    janela: VecDeque<(f64, [f32; 3])>,
}

fn distancia(a: [f32; 3], b: [f32; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (f64::from(*x) - f64::from(y)).powi(2))
        .sum::<f64>()
        .sqrt()
}

impl Rastro {
    /// Acrescenta uma leitura; devolve o deslocamento desde a anterior, ou `None`
    /// se não há anterior ou se foi teletransporte (aí o histórico recomeça).
    fn acrescentar(&mut self, t: f64, p: [f32; 3]) -> Option<f64> {
        if !p.iter().all(|x| x.is_finite()) {
            self.janela.clear();
            return None;
        }
        let passo = self.janela.back().and_then(|&(t0, p0)| {
            let d = distancia(p0, p);
            let limite = VEL_MAX_MPS * (t - t0).max(0.0) + FOLGA_TELEPORTE_M;
            (d <= limite).then_some(d)
        });
        if passo.is_none() {
            self.janela.clear();
        }
        self.janela.push_back((t, p));
        // Mantém uma leitura com idade >= JANELA_S para a média cobrir a janela toda.
        while self.janela.len() > 2
            && self
                .janela
                .get(1)
                .is_some_and(|&(t1, _)| t - t1 >= JANELA_S)
        {
            self.janela.pop_front();
        }
        passo
    }

    fn velocidade_kmh(&self) -> Option<i64> {
        let (&(t0, p0), &(t1, p1)) = (self.janela.front()?, self.janela.back()?);
        let dt = t1 - t0;
        if dt < JANELA_MIN_S {
            return None;
        }
        // Limitado por VEL_MAX_MPS (teletransporte zera a janela): cabe em i64.
        #[allow(clippy::cast_possible_truncation)] // ver acima
        Some((distancia(p0, p1) / dt * 3.6).round() as i64)
    }
}

/// Cambalhotas de um vetor "pra cima": conta meias-voltas (de pé ↔ de cabeça
/// para baixo). Limitação aceita: virar de teto e voltar pelo mesmo lado também
/// conta 360° (sem o eixo de giro não dá para distinguir; é raro).
#[derive(Debug, Default)]
struct Rodopio {
    anterior: Option<[f32; 3]>,
    /// +1 de pé, -1 de cabeça para baixo, 0 = ainda não sabe.
    lado: i8,
    /// Meias-voltas na cambalhota atual.
    meias: u32,
    de_pe_desde: Option<f64>,
}

impl Rodopio {
    /// Acrescenta uma leitura; devolve quantas voltas completas terminaram nela.
    fn acrescentar(&mut self, t: f64, cima: [f32; 3]) -> u32 {
        let norma = cima.iter().map(|x| x * x).sum::<f32>().sqrt();
        if !norma.is_finite() || norma < 0.5 {
            *self = Rodopio::default();
            return 0;
        }
        let u = cima.map(|x| x / norma);
        let pulou = self.anterior.is_some_and(|a| {
            let cos: f32 = a.iter().zip(u).map(|(x, y)| x * y).sum();
            cos.clamp(-1.0, 1.0).acos().to_degrees() > GIRO_MAX_GRAUS
        });
        if pulou {
            *self = Rodopio::default();
        }
        self.anterior = Some(u);
        let lado = if u[2] > DE_PE {
            1
        } else if u[2] < -DE_PE {
            -1
        } else {
            self.lado
        };
        let mut voltas = 0;
        if self.lado != 0 && lado != self.lado {
            self.meias += 1;
            if lado == 1 && self.meias.is_multiple_of(2) {
                voltas = 1;
            }
        }
        self.lado = lado;
        if u[2] > DE_PE {
            let desde = *self.de_pe_desde.get_or_insert(t);
            if t - desde >= FIM_GIRO_S {
                self.meias = 0;
            }
        } else {
            self.de_pe_desde = None;
        }
        voltas
    }

    fn graus(&self) -> i64 {
        i64::from(self.meias) * 180
    }
}

/// Saltos: leituras seguidas "no ar" para decolar e "no chão" para pousar.
const LEITURAS_DECOLAR: u32 = 3;
const LEITURAS_POUSAR: u32 = 2;
/// No ar: rodas paradas (fim de curso) e velocidade vertical caindo mais que
/// isto (m/s) entre leituras. Na câmera lenta dos saltos grandes a 30 Hz cai ~0,2.
const QUEDA_VZ_MIN: f32 = 0.05;
const RODA_PARADA: f32 = 1e-5;

fn horizontal(a: [f32; 3], b: [f32; 3]) -> f64 {
    (f64::from(a[0]) - f64::from(b[0])).hypot(f64::from(a[1]) - f64::from(b[1]))
}

/// Detector de saltos (conferido com saltos de verdade em 2026-11-21, ver NOTAS
/// do NFSU2): no ar, as rodas ficam paradas no fim de curso e a velocidade
/// vertical cai sem parar. Não depende do curso da suspensão de cada carro.
#[derive(Debug, Default)]
struct Voo {
    anterior: Option<(f64, [f32; 3], f32, Vec<f32>)>,
    seguidos: u32,
    candidato: Option<[f32; 3]>,
    decolagem: Option<[f32; 3]>,
    fora: u32,
    pouso: Option<[f32; 3]>,
    /// Metros do salto em andamento.
    metros: f64,
}

impl Voo {
    /// Acrescenta uma leitura; devolve os metros do salto que acabou de pousar.
    fn acrescentar(&mut self, t: f64, pos: [f32; 3], vz: f32, rodas: &[f32]) -> Option<f64> {
        let validos = pos.iter().chain(rodas).all(|x| x.is_finite()) && vz.is_finite();
        let Some((t0, p0, vz0, r0)) = self.anterior.take().filter(|_| validos) else {
            *self = Voo::default();
            if validos {
                self.anterior = Some((t, pos, vz, rodas.to_vec()));
            }
            return None;
        };
        if pos == p0 {
            // Mesmo quadro do jogo (ou pausado): nada a fazer.
            self.anterior = Some((t0, p0, vz0, r0));
            return None;
        }
        if horizontal(p0, pos) > VEL_MAX_MPS * (t - t0).max(0.0) + FOLGA_TELEPORTE_M
            || r0.len() != rodas.len()
        {
            *self = Voo::default();
            self.anterior = Some((t, pos, vz, rodas.to_vec()));
            return None;
        }
        let no_ar = rodas
            .iter()
            .zip(&r0)
            .all(|(a, b)| (a - b).abs() < RODA_PARADA)
            && vz - vz0 < -QUEDA_VZ_MIN;
        self.anterior = Some((t, pos, vz, rodas.to_vec()));
        let Some(inicio) = self.decolagem else {
            self.seguidos = if no_ar { self.seguidos + 1 } else { 0 };
            if self.seguidos == 1 {
                self.candidato = Some(p0);
            }
            if self.seguidos >= LEITURAS_DECOLAR {
                self.decolagem = self.candidato;
                self.metros = self.decolagem.map_or(0.0, |d| horizontal(d, pos));
            }
            return None;
        };
        if no_ar {
            self.fora = 0;
            self.pouso = None;
            self.metros = horizontal(inicio, pos);
            return None;
        }
        self.fora += 1;
        let pouso = *self.pouso.get_or_insert(pos);
        self.metros = horizontal(inicio, pouso);
        if self.fora < LEITURAS_POUSAR {
            return None;
        }
        let m = self.metros;
        self.decolagem = None;
        self.seguidos = 0;
        self.fora = 0;
        self.pouso = None;
        self.metros = 0.0;
        Some(m)
    }

    fn metros(&self) -> i64 {
        // Limitado pelo teste de teletransporte a cada leitura: pequeno.
        #[allow(clippy::cast_possible_truncation)] // ver acima
        let m = self.metros.floor() as i64;
        m
    }
}

/// Resultado de uma leitura.
#[derive(Debug, Default)]
pub struct Passo {
    pub eventos: Vec<Evento>,
    pub mudancas: Vec<Mudanca>,
    /// As estatísticas guardadas mudaram (quem chama grava no banco).
    pub derivadas_mudaram: bool,
    /// O perfil trocou: estatísticas finais do perfil anterior, para gravar.
    pub perfil_anterior: Option<(String, HashMap<String, i64>)>,
    /// Uma corrida acabou de terminar (resultado novo na tabela).
    pub corrida: Option<Resultado>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evento {
    Desbloqueio { conquista: String },
    Marco { conquista: String, pct: u8 },
}

pub struct Avaliador {
    conquistas: Vec<Conquista>,
    var_perfil: String,
    sem_perfil: Vec<String>,
    perfil: Option<String>,
    /// Leituras seguidas com o mesmo perfil. Enquanto < `leituras_base`, só atualiza a base.
    leituras_estaveis: u32,
    leituras_base: u32,
    anterior: HashMap<String, i64>,
    derivadas: Vec<Derivada>,
    /// Valores das derivadas guardadas do perfil atual (carregados do banco por quem chama).
    acumulados: HashMap<String, i64>,
    /// Frações de metro do odômetro ainda não somadas em `acumulados`.
    resto_m: HashMap<String, f64>,
    rastros: HashMap<String, Rastro>,
    rodopios: HashMap<String, Rodopio>,
    /// Detector de saltos por posição.
    voos: HashMap<String, Voo>,
    /// Cada `TempoAcima` em andamento: (início, desde quando está abaixo).
    acima_desde: HashMap<String, (f64, Option<f64>)>,
    /// Maior valor visto de cada `Conta`/`Soma` neste perfil (elas nunca descem).
    maximos: HashMap<String, i64>,
    registrar: Vec<String>,
    corrida: Option<CorridaCfg>,
    /// Última tabela de resultados já avaliada (ou de base).
    tabela_vista: Option<Vec<i64>>,
    /// Tabela nova esperando ficar parada: (tabela, desde quando).
    tabela_nova: Option<(Vec<i64>, f64)>,
    /// Resultado do jogador na `tabela_vista`.
    resultado_visto: Option<Resultado>,
}

impl Avaliador {
    /// `leituras_base`: quantas leituras após trocar de perfil servem só de base
    /// (o jogo pode levar alguns quadros para preencher tudo ao carregar um save).
    pub fn new(
        conquistas: Vec<Conquista>,
        var_perfil: String,
        sem_perfil: Vec<String>,
        derivadas: Vec<Derivada>,
        registrar: Vec<String>,
        leituras_base: u32,
    ) -> Self {
        Avaliador {
            conquistas,
            var_perfil,
            sem_perfil,
            perfil: None,
            leituras_estaveis: 0,
            leituras_base,
            anterior: HashMap::new(),
            derivadas,
            acumulados: HashMap::new(),
            resto_m: HashMap::new(),
            rastros: HashMap::new(),
            rodopios: HashMap::new(),
            voos: HashMap::new(),
            acima_desde: HashMap::new(),
            maximos: HashMap::new(),
            registrar,
            corrida: None,
            tabela_vista: None,
            tabela_nova: None,
            resultado_visto: None,
        }
    }

    /// Liga a contagem de corridas (`TipoDerivada::Corridas`) com este formato de tabela.
    pub fn com_corrida(mut self, corrida: Option<CorridaCfg>) -> Self {
        self.corrida = corrida;
        self
    }

    /// Valores salvos das derivadas do perfil atual. Chamar logo depois que o
    /// perfil mudou (durante as leituras de base).
    pub fn carregar_derivadas(&mut self, valores: HashMap<String, i64>) {
        // A base também passa a ter os valores salvos: senão a próxima leitura
        // pareceria um salto de 0 até eles.
        for d in self.derivadas.iter().filter(|d| d.guardada()) {
            if self.anterior.contains_key(&d.nome) {
                self.anterior
                    .insert(d.nome.clone(), valores.get(&d.nome).copied().unwrap_or(0));
            }
        }
        self.acumulados = valores;
    }

    pub fn derivadas(&self) -> &HashMap<String, i64> {
        &self.acumulados
    }

    pub fn perfil(&self) -> Option<&str> {
        self.perfil.as_deref()
    }

    /// A última leitura já valeu (perfil carregado e fora das leituras de base).
    pub fn estavel(&self) -> bool {
        self.perfil.is_some() && self.leituras_estaveis > self.leituras_base
    }

    /// Valores numéricos da última leitura, incluindo as derivadas.
    pub fn valores(&self) -> &HashMap<String, i64> {
        &self.anterior
    }

    /// Processa uma leitura feita no instante `t` (segundos, relógio monotônico).
    /// Os eventos são candidatos: quem chama confere no banco se já tinham
    /// acontecido para este perfil.
    pub fn passo(&mut self, vars: &HashMap<String, Valor>, t: f64) -> Passo {
        let perfil = match vars.get(&self.var_perfil) {
            Some(Valor::Texto(t)) if !t.is_empty() && !self.sem_perfil.contains(t) => {
                Some(t.clone())
            }
            _ => None,
        };
        let mut out = Passo::default();
        if perfil != self.perfil {
            if let Some(antigo) = self.perfil.take()
                && !self.acumulados.is_empty()
            {
                out.perfil_anterior = Some((antigo, std::mem::take(&mut self.acumulados)));
            }
            self.perfil = perfil;
            self.leituras_estaveis = 0;
            self.anterior.clear();
            self.acumulados.clear();
            self.resto_m.clear();
            self.rastros.clear();
            self.rodopios.clear();
            self.voos.clear();
            self.acima_desde.clear();
            self.maximos.clear();
            self.tabela_vista = None;
            self.tabela_nova = None;
            self.resultado_visto = None;
        }
        if self.perfil.is_none() {
            return out;
        }

        let mut numeros: HashMap<String, i64> = vars
            .iter()
            .filter_map(|(k, v)| match v {
                Valor::Num(n) => Some((k.clone(), *n)),
                Valor::Texto(_) | Valor::Vec3(_) | Valor::Floats(_) | Valor::Inteiros(_) => None,
            })
            .collect();
        let estavel = self.leituras_estaveis >= self.leituras_base;
        out.corrida = self.corrida_nova(vars, &numeros, t, estavel);

        // Posições: um rastro por variável vec3 (várias derivadas podem usar a mesma).
        let mut passos_m: HashMap<&str, f64> = HashMap::new();
        for (k, v) in vars {
            match v {
                Valor::Vec3(p) => {
                    if let Some(d) = self
                        .rastros
                        .entry(k.clone())
                        .or_default()
                        .acrescentar(t, *p)
                    {
                        passos_m.insert(k.as_str(), d);
                    }
                }
                _ => {
                    self.rastros.remove(k);
                }
            }
        }
        for k in self.rastros.keys().cloned().collect::<Vec<_>>() {
            if !vars.contains_key(&k) {
                self.rastros.remove(&k);
            }
        }
        // Saltos: um detector por posição.
        let mut pousos: HashMap<&str, f64> = HashMap::new();
        let mut vistos: Vec<&str> = vec![];
        for d in self.derivadas.iter().filter(|d| d.usa_voo()) {
            if vistos.contains(&d.fonte.as_str()) {
                continue;
            }
            vistos.push(d.fonte.as_str());
            let leitura = (
                vars.get(&d.fonte),
                d.velocidade_vetor.as_ref().and_then(|v| vars.get(v)),
                d.rodas.as_ref().and_then(|r| vars.get(r)),
            );
            if let (Some(Valor::Vec3(p)), Some(Valor::Vec3(v)), Some(Valor::Floats(r))) = leitura {
                let voo = self.voos.entry(d.fonte.clone()).or_default();
                if let Some(m) = voo.acrescentar(t, *p, v[2], r) {
                    pousos.insert(d.fonte.as_str(), m);
                }
            } else {
                self.voos.remove(&d.fonte);
            }
        }
        // Cambalhotas: um rodopio por vetor usado por Giro/Voltas.
        let mut voltas_novas: HashMap<&str, u32> = HashMap::new();
        for d in self.derivadas.iter().filter(|d| d.usa_rodopio()) {
            if voltas_novas.contains_key(d.fonte.as_str()) {
                continue;
            }
            match vars.get(&d.fonte) {
                Some(Valor::Vec3(c)) => {
                    let n = self
                        .rodopios
                        .entry(d.fonte.clone())
                        .or_default()
                        .acrescentar(t, *c);
                    voltas_novas.insert(d.fonte.as_str(), n);
                }
                _ => {
                    self.rodopios.remove(&d.fonte);
                }
            }
        }

        if estavel {
            for d in self
                .derivadas
                .iter()
                .filter(|d| d.tipo == TipoDerivada::Distancia)
            {
                let Some(&m) = passos_m.get(d.fonte.as_str()) else {
                    continue;
                };
                if m < PASSO_MIN_M {
                    continue;
                }
                let resto = self.resto_m.entry(d.nome.clone()).or_insert(0.0);
                *resto += m;
                let inteiros = resto.floor();
                if inteiros >= 1.0 {
                    *resto -= inteiros;
                    let v = self.acumulados.entry(d.nome.clone()).or_insert(0);
                    // `inteiros` <= VEL_MAX_MPS * dt + folga + 1: pequeno.
                    #[allow(clippy::cast_possible_truncation)] // ver acima
                    {
                        *v = v.saturating_add(inteiros as i64);
                    }
                    out.derivadas_mudaram = true;
                }
            }
            for d in self
                .derivadas
                .iter()
                .filter(|d| d.tipo == TipoDerivada::SomaAumentos)
            {
                if let (Some(&antes), Some(&agora)) =
                    (self.anterior.get(&d.fonte), numeros.get(&d.fonte))
                    && agora > antes
                {
                    let v = self.acumulados.entry(d.nome.clone()).or_insert(0);
                    *v = v.saturating_add(agora - antes);
                    out.derivadas_mudaram = true;
                }
            }
            for d in self
                .derivadas
                .iter()
                .filter(|d| d.tipo == TipoDerivada::Voltas)
            {
                if let Some(&n) = voltas_novas.get(d.fonte.as_str())
                    && n > 0
                {
                    let v = self.acumulados.entry(d.nome.clone()).or_insert(0);
                    *v = v.saturating_add(i64::from(n));
                    out.derivadas_mudaram = true;
                }
            }
            for d in self
                .derivadas
                .iter()
                .filter(|d| d.tipo == TipoDerivada::SaltosSomados)
            {
                if let Some(&m) = pousos.get(d.fonte.as_str()) {
                    let v = self.acumulados.entry(d.nome.clone()).or_insert(0);
                    // Metros de um salto: pequeno (teste de teletransporte).
                    #[allow(clippy::cast_possible_truncation)] // ver acima
                    {
                        *v = v.saturating_add(m.floor() as i64);
                    }
                    out.derivadas_mudaram = true;
                }
            }
            if let Some(r) = &out.corrida {
                for d in self
                    .derivadas
                    .iter()
                    .filter(|d| d.filtro.corrida.as_ref().is_some_and(|f| r.passa(f)))
                {
                    let v = self.acumulados.entry(d.nome.clone()).or_insert(0);
                    *v = v.saturating_add(1);
                    out.derivadas_mudaram = true;
                }
            }
            for var in &self.registrar {
                if let (Some(&antes), Some(&depois)) = (self.anterior.get(var), numeros.get(var))
                    && antes != depois
                {
                    out.mudancas.push(Mudanca {
                        var: var.clone(),
                        antes,
                        depois,
                    });
                }
            }
        }
        for d in &self.derivadas {
            let v = match d.tipo {
                _ if d.guardada() => Some(self.acumulados.get(&d.nome).copied().unwrap_or(0)),
                TipoDerivada::Velocidade => {
                    self.rastros.get(&d.fonte).and_then(Rastro::velocidade_kmh)
                }
                TipoDerivada::Giro => self.rodopios.get(&d.fonte).map(Rodopio::graus),
                TipoDerivada::Salto => self.voos.get(&d.fonte).map(Voo::metros),
                TipoDerivada::Conta | TipoDerivada::Soma => d.de_lista(vars).map(|v| {
                    let m = self.maximos.entry(d.nome.clone()).or_insert(v);
                    *m = (*m).max(v);
                    *m
                }),
                // Depois das outras: a fonte pode ser uma derivada ao vivo.
                _ => continue,
            };
            if let Some(v) = v {
                numeros.insert(d.nome.clone(), v);
            }
        }
        for d in &self.derivadas {
            let TipoDerivada::TempoAcima { limite } = d.tipo else {
                continue;
            };
            let acima = numeros.get(&d.fonte).is_some_and(|&v| v >= limite);
            let Some(estado) = (match self.acima_desde.get_mut(&d.nome) {
                Some(e) => Some(e),
                None if acima => Some(self.acima_desde.entry(d.nome.clone()).or_insert((t, None))),
                None => None,
            }) else {
                if numeros.contains_key(&d.fonte) {
                    numeros.insert(d.nome.clone(), 0);
                }
                continue;
            };
            if acima {
                estado.1 = None;
            } else if t - *estado.1.get_or_insert(t) > TOLERANCIA_ABAIXO_S {
                self.acima_desde.remove(&d.nome);
                numeros.insert(d.nome.clone(), 0);
                continue;
            }
            // Segundos inteiros; `t - início` >= 0 e pequeno.
            #[allow(clippy::cast_possible_truncation)] // ver acima
            let s = (t - estado.0).max(0.0).floor() as i64;
            numeros.insert(d.nome.clone(), s);
        }
        if estavel {
            for c in &self.conquistas {
                let Some(prog) = &c.progresso else {
                    continue; // platina: quem chama decide
                };
                let (Some(&antes), Some(&agora)) =
                    (self.anterior.get(&prog.var), numeros.get(&prog.var))
                else {
                    continue;
                };
                if let Some(e) = transicao(c, antes, agora) {
                    out.eventos.push(e);
                }
            }
        }
        self.leituras_estaveis = self.leituras_estaveis.saturating_add(1);
        // Variável que falhou nesta leitura sai da base: quando voltar, volta como base.
        self.anterior = numeros;
        out
    }
}

impl Avaliador {
    /// Resultado da corrida que acabou de terminar, se a tabela mudou desde a
    /// última vez e ficou parada `RESULTADO_PARADO_S`. A tabela inteira é comparada
    /// (não só as linhas válidas): o número de corredores muda na largada da
    /// corrida seguinte, e isso não é resultado novo.
    fn corrida_nova(
        &mut self,
        vars: &HashMap<String, Valor>,
        numeros: &HashMap<String, i64>,
        t: f64,
        estavel: bool,
    ) -> Option<Resultado> {
        let c = self.corrida.as_ref()?;
        // Leitura falhou: espera a próxima, sem mexer no estado.
        let Some(Valor::Inteiros(tabela)) = vars.get(&c.tabela) else {
            return None;
        };
        let linhas = match vars.get(&c.corredores) {
            Some(Valor::Inteiros(n)) => n.iter().fold(0i64, |a, &b| a.saturating_add(b.max(0))),
            _ => return None,
        };
        let linhas = usize::try_from(linhas).ok()?;
        let pista = *numeros.get(&c.pista)?;
        if !estavel || self.tabela_vista.is_none() {
            self.tabela_vista = Some(tabela.clone());
            self.tabela_nova = None;
            self.resultado_visto = Resultado::da_tabela(c, tabela, linhas, pista);
            return None;
        }
        if self.tabela_vista.as_ref() == Some(tabela) {
            self.tabela_nova = None;
            return None;
        }
        match &self.tabela_nova {
            Some((nova, desde)) if nova == tabela => {
                if t - desde < RESULTADO_PARADO_S {
                    return None;
                }
            }
            _ => {
                self.tabela_nova = Some((tabela.clone(), t));
                return None;
            }
        }
        self.tabela_nova = None;
        self.tabela_vista = Some(tabela.clone());
        let r = Resultado::da_tabela(c, tabela, linhas, pista)?;
        if self
            .resultado_visto
            .as_ref()
            .is_some_and(|v| v.mesma_corrida(&r))
        {
            return None;
        }
        self.resultado_visto = Some(r.clone());
        Some(r)
    }
}

/// Maior marco (desbloqueio ou %) cruzado de `antes` para `agora`.
fn transicao(c: &Conquista, antes: i64, agora: i64) -> Option<Evento> {
    let meta = c.progresso.as_ref()?.meta;
    if antes < meta && agora >= meta {
        return Some(Evento::Desbloqueio {
            conquista: c.id.clone(),
        });
    }
    if !c.marcos {
        return None;
    }
    MARCOS
        .iter()
        .rev()
        .find(|&&pct| {
            // Para cima: com meta 34, 8 vitórias (23,5%) ainda não são 25%.
            let limite = meta.saturating_mul(i64::from(pct)).saturating_add(99) / 100;
            antes < limite && agora >= limite
        })
        .map(|&pct| Evento::Marco {
            conquista: c.id.clone(),
            pct,
        })
}

/// Porcentagem (0..=100) de `valor` em relação à meta.
pub fn porcentagem(valor: i64, meta: i64) -> u8 {
    if meta <= 0 {
        return 100;
    }
    let p = valor.clamp(0, meta).saturating_mul(100) / meta;
    // `p` está em 0..=100 pelo clamp acima.
    u8::try_from(p).unwrap_or(100)
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::config::{Nivel, Progresso};

    fn c(id: &str, meta: i64) -> Conquista {
        Conquista {
            id: id.into(),
            nivel: Nivel::Bronze,
            nome: id.into(),
            descricao: String::new(),
            progresso: Some(Progresso {
                var: "dinheiro".into(),
                meta,
            }),
            marcos: true,
        }
    }

    fn leitura(perfil: &str, dinheiro: i64) -> HashMap<String, Valor> {
        HashMap::from([
            ("perfil".into(), Valor::Texto(perfil.into())),
            ("dinheiro".into(), Valor::Num(dinheiro)),
        ])
    }

    fn av() -> Avaliador {
        Avaliador::new(
            vec![c("10k", 10_000), c("100k", 100_000)],
            "perfil".into(),
            vec!["No Profile".into()],
            vec![Derivada {
                nome: "ganhos".into(),
                fonte: "dinheiro".into(),
                tipo: TipoDerivada::SomaAumentos,
                rodas: None,
                velocidade_vetor: None,
                filtro: Filtro::default(),
            }],
            vec!["dinheiro".into()],
            2,
        )
    }

    #[test]
    fn ganhos_somam_so_aumentos_depois_da_base() {
        let mut a = av();
        a.passo(&leitura("X", 1_000), 0.0);
        a.carregar_derivadas(HashMap::from([("ganhos".into(), 500)]));
        a.passo(&leitura("X", 1_000), 0.0);
        let p = a.passo(&leitura("X", 3_000), 0.0);
        assert!(p.derivadas_mudaram);
        assert_eq!(a.derivadas()["ganhos"], 2_500);
        // Gastar não desconta.
        let p = a.passo(&leitura("X", 100), 0.0);
        assert!(!p.derivadas_mudaram);
        a.passo(&leitura("X", 1_100), 0.0);
        assert_eq!(a.derivadas()["ganhos"], 3_500);
        // Trocar de perfil zera até quem chama carregar os do outro perfil.
        a.passo(&leitura("Y", 50_000), 0.0);
        assert!(a.derivadas().is_empty());
    }

    #[test]
    fn conquista_por_ganhos_totais() {
        let c100 = cv("ganhos_100k", "ganhos", 100_000, true);
        let mut a = Avaliador::new(
            vec![c100],
            "perfil".into(),
            vec![],
            vec![Derivada {
                nome: "ganhos".into(),
                fonte: "dinheiro".into(),
                tipo: TipoDerivada::SomaAumentos,
                rodas: None,
                velocidade_vetor: None,
                filtro: Filtro::default(),
            }],
            vec![],
            1,
        );
        a.passo(&leitura("X", 0), 0.0);
        a.carregar_derivadas(HashMap::from([("ganhos".into(), 95_000)]));
        // Gastar e ganhar de novo: o saldo nunca passa de 100k, os ganhos sim.
        assert!(a.passo(&leitura("X", 4_000), 0.0).eventos.is_empty());
        assert!(a.passo(&leitura("X", 0), 0.0).eventos.is_empty());
        assert_eq!(
            a.passo(&leitura("X", 2_000), 0.0).eventos,
            vec![Evento::Desbloqueio {
                conquista: "ganhos_100k".into()
            }]
        );
    }

    #[test]
    fn mudancas_registradas_so_depois_da_base() {
        let mut a = av();
        assert!(a.passo(&leitura("X", 0), 0.0).mudancas.is_empty());
        assert!(a.passo(&leitura("X", 9), 0.0).mudancas.is_empty());
        assert_eq!(
            a.passo(&leitura("X", 700), 0.0).mudancas,
            vec![Mudanca {
                var: "dinheiro".into(),
                antes: 9,
                depois: 700
            }]
        );
        assert!(a.passo(&leitura("X", 700), 0.0).mudancas.is_empty());
    }

    #[test]
    fn texto_de_sem_perfil_nao_e_perfil() {
        let mut a = av();
        for _ in 0..3 {
            assert!(a.passo(&leitura("No Profile", 0), 0.0).eventos.is_empty());
        }
        assert!(
            a.passo(&leitura("No Profile", 20_000), 0.0)
                .eventos
                .is_empty()
        );
        assert_eq!(a.perfil(), None);
    }

    #[test]
    fn save_que_ja_tem_dinheiro_nao_desbloqueia() {
        let mut a = av();
        assert!(a.passo(&leitura("X", 500_000), 0.0).eventos.is_empty());
        assert!(a.passo(&leitura("X", 500_000), 0.0).eventos.is_empty());
        assert!(a.passo(&leitura("X", 501_000), 0.0).eventos.is_empty());
    }

    #[test]
    fn cruzar_a_meta_desbloqueia() {
        let mut a = av();
        a.passo(&leitura("X", 9_000), 0.0);
        a.passo(&leitura("X", 9_000), 0.0);
        assert_eq!(
            a.passo(&leitura("X", 10_500), 0.0).eventos,
            vec![Evento::Desbloqueio {
                conquista: "10k".into()
            }]
        );
    }

    #[test]
    fn leituras_de_base_ignoram_saltos() {
        let mut a = av();
        a.passo(&leitura("X", 0), 0.0);
        // Ainda dentro das 2 leituras de base: o salto é o save sendo carregado.
        assert!(a.passo(&leitura("X", 50_000), 0.0).eventos.is_empty());
        assert!(a.passo(&leitura("X", 50_000), 0.0).eventos.is_empty());
    }

    #[test]
    fn trocar_de_perfil_refaz_a_base() {
        let mut a = av();
        for _ in 0..3 {
            a.passo(&leitura("X", 100), 0.0);
        }
        // Perfil Y com mais dinheiro: troca, não conquista.
        assert!(a.passo(&leitura("Y", 200_000), 0.0).eventos.is_empty());
        assert!(a.passo(&leitura("Y", 200_000), 0.0).eventos.is_empty());
        assert_eq!(a.perfil(), Some("Y"));
    }

    #[test]
    fn sem_perfil_nao_avalia() {
        let mut a = av();
        for _ in 0..3 {
            assert!(a.passo(&leitura("", 0), 0.0).eventos.is_empty());
        }
        assert!(a.passo(&leitura("", 20_000), 0.0).eventos.is_empty());
        assert_eq!(a.perfil(), None);
    }

    #[test]
    fn marcos_pegam_so_o_maior() {
        let mut a = av();
        a.passo(&leitura("X", 0), 0.0);
        a.passo(&leitura("X", 0), 0.0);
        // 0 → 60.000: 10k desbloqueia; 100k cruza 25% e 50% → só 50%.
        let ev = a.passo(&leitura("X", 60_000), 0.0).eventos;
        assert_eq!(
            ev,
            vec![
                Evento::Desbloqueio {
                    conquista: "10k".into()
                },
                Evento::Marco {
                    conquista: "100k".into(),
                    pct: 50
                },
            ]
        );
    }

    #[test]
    fn variavel_que_falhou_volta_como_base() {
        let mut a = av();
        a.passo(&leitura("X", 0), 0.0);
        a.passo(&leitura("X", 0), 0.0);
        let mut sem_dinheiro = leitura("X", 0);
        sem_dinheiro.remove("dinheiro");
        a.passo(&sem_dinheiro, 0.0);
        // Voltou já acima: não há leitura anterior para comparar.
        assert!(a.passo(&leitura("X", 20_000), 0.0).eventos.is_empty());
    }

    fn av_pos(conquistas: Vec<Conquista>) -> Avaliador {
        Avaliador::new(
            conquistas,
            "perfil".into(),
            vec![],
            vec![
                Derivada {
                    nome: "odometro".into(),
                    fonte: "pos".into(),
                    tipo: TipoDerivada::Distancia,
                    rodas: None,
                    velocidade_vetor: None,
                    filtro: Filtro::default(),
                },
                Derivada {
                    nome: "kmh".into(),
                    fonte: "pos".into(),
                    tipo: TipoDerivada::Velocidade,
                    rodas: None,
                    velocidade_vetor: None,
                    filtro: Filtro::default(),
                },
            ],
            vec![],
            1,
        )
    }

    fn pos(perfil: &str, x: f32) -> HashMap<String, Valor> {
        HashMap::from([
            ("perfil".into(), Valor::Texto(perfil.into())),
            ("pos".into(), Valor::Vec3([x, -594.0, 25.0])),
        ])
    }

    fn cv(id: &str, var: &str, meta: i64, marcos: bool) -> Conquista {
        let mut c = c(id, meta);
        if let Some(p) = &mut c.progresso {
            p.var = var.into();
        }
        c.marcos = marcos;
        c
    }

    #[test]
    fn odometro_soma_metros_e_ignora_teleporte() {
        let mut a = av_pos(vec![]);
        // 30 Hz a 20 m/s = 72 km/h.
        let dt = 1.0 / 30.0;
        let mut x = 1000.0_f32;
        for i in 0..91 {
            a.passo(&pos("X", x), f64::from(i) * dt);
            x += 20.0 / 30.0;
        }
        // 90 passos depois da 1ª leitura (base) e da 2ª (estável): ~89 × 0,667 m.
        let m = a.derivadas()["odometro"];
        assert!((58..=60).contains(&m), "{m}");
        assert_eq!(a.valores()["kmh"], 72);
        // Teletransporte de 5 km: não conta e zera a janela da velocidade.
        let p = a.passo(&pos("X", x + 5000.0), 91.0 * dt);
        assert_eq!(a.derivadas()["odometro"], m);
        assert!(!p.derivadas_mudaram);
        assert!(!a.valores().contains_key("kmh"));
    }

    #[test]
    fn parado_tremendo_nao_anda() {
        let mut a = av_pos(vec![]);
        for i in 0..300 {
            let x = if i % 2 == 0 { 10.0 } else { 10.004 };
            a.passo(&pos("X", x), f64::from(i) / 30.0);
        }
        assert!(!a.derivadas().contains_key("odometro"));
        assert_eq!(a.valores()["kmh"], 0);
    }

    #[test]
    fn recorde_de_velocidade_sem_marcos() {
        let mut a = av_pos(vec![cv("200kmh", "kmh", 200, false)]);
        let mut x = 0.0_f32;
        let mut eventos = vec![];
        // Acelera de 0 a ~250 km/h em 10 s.
        for i in 0..300 {
            let t = f64::from(i) / 30.0;
            x += (70.0 * t / 10.0) as f32 / 30.0;
            eventos.extend(a.passo(&pos("X", x), t).eventos);
        }
        assert_eq!(
            eventos,
            vec![Evento::Desbloqueio {
                conquista: "200kmh".into()
            }]
        );
    }

    #[test]
    fn trocar_de_perfil_devolve_as_estatisticas_do_anterior() {
        let mut a = av_pos(vec![]);
        for i in 0..10 {
            a.passo(&pos("X", i as f32 * 0.5), f64::from(i) / 30.0);
        }
        let p = a.passo(&pos("Y", 0.0), 10.0 / 30.0);
        let (perfil, est) = p.perfil_anterior.unwrap();
        assert_eq!(perfil, "X");
        assert_eq!(est["odometro"], 4);
        assert!(a.derivadas().is_empty());
    }

    fn av_giro(conquistas: Vec<Conquista>) -> Avaliador {
        Avaliador::new(
            conquistas,
            "perfil".into(),
            vec![],
            vec![
                Derivada {
                    nome: "giro".into(),
                    fonte: "cima".into(),
                    tipo: TipoDerivada::Giro,
                    rodas: None,
                    velocidade_vetor: None,
                    filtro: Filtro::default(),
                },
                Derivada {
                    nome: "voltas".into(),
                    fonte: "cima".into(),
                    tipo: TipoDerivada::Voltas,
                    rodas: None,
                    velocidade_vetor: None,
                    filtro: Filtro::default(),
                },
            ],
            vec![],
            1,
        )
    }

    fn cima(graus: f64) -> HashMap<String, Valor> {
        let r = graus.to_radians();
        HashMap::from([
            ("perfil".into(), Valor::Texto("X".into())),
            (
                "cima".into(),
                Valor::Vec3([0.0, r.sin() as f32, r.cos() as f32]),
            ),
        ])
    }

    /// Gira de `de` até `ate` graus a 720°/s (24° por leitura a 30 Hz).
    fn girar(a: &mut Avaliador, t: &mut f64, de: f64, ate: f64) -> Vec<Evento> {
        let mut ev = vec![];
        let passo = 24.0 * (ate - de).signum();
        let mut g = de;
        while (ate - g).abs() > 1e-9 {
            g = if (ate - g).abs() < passo.abs() {
                ate
            } else {
                g + passo
            };
            *t += 1.0 / 30.0;
            ev.extend(a.passo(&cima(g), *t).eventos);
        }
        ev
    }

    fn parado(a: &mut Avaliador, t: &mut f64, graus: f64, s: f64) -> Vec<Evento> {
        let mut ev = vec![];
        for _ in 0..(s * 30.0) as u32 {
            *t += 1.0 / 30.0;
            ev.extend(a.passo(&cima(graus), *t).eventos);
        }
        ev
    }

    fn desbloqueios(ev: &[Evento]) -> Vec<&str> {
        ev.iter()
            .filter_map(|e| match e {
                Evento::Desbloqueio { conquista } => Some(conquista.as_str()),
                Evento::Marco { .. } => None,
            })
            .collect()
    }

    #[test]
    fn cambalhota_completa_e_dupla() {
        let mut a = av_giro(vec![
            cv("360", "giro", 360, false),
            cv("720", "giro", 720, false),
        ]);
        let mut t = 0.0;
        assert!(parado(&mut a, &mut t, 0.0, 1.0).is_empty());
        let ev = girar(&mut a, &mut t, 0.0, 360.0);
        assert_eq!(desbloqueios(&ev), ["360"]);
        assert_eq!(a.valores()["giro"], 360);
        assert_eq!(a.derivadas()["voltas"], 1);
        // De pé 1 s: a cambalhota acaba e o giro zera.
        parado(&mut a, &mut t, 360.0, 1.5);
        assert_eq!(a.valores()["giro"], 0);
        // Candidatos: o 360 de novo (o banco descarta, já estava feito) e o 720.
        let ev = girar(&mut a, &mut t, 0.0, 720.0);
        assert_eq!(desbloqueios(&ev), ["360", "720"]);
        assert_eq!(a.derivadas()["voltas"], 3);
    }

    #[test]
    fn tremer_de_lado_nao_conta() {
        let mut a = av_giro(vec![cv("360", "giro", 360, false)]);
        let mut t = 0.0;
        parado(&mut a, &mut t, 0.0, 0.5);
        girar(&mut a, &mut t, 0.0, 90.0);
        // Deitado de lado, balançando ±25° por 10 s (hidráulica, preso na parede).
        let mut ev = vec![];
        for i in 0..300 {
            t += 1.0 / 30.0;
            let g = 90.0 + if i % 2 == 0 { 25.0 } else { -25.0 };
            ev.extend(a.passo(&cima(g), t).eventos);
        }
        assert!(ev.is_empty());
        assert!(a.valores()["giro"] <= 180, "{}", a.valores()["giro"]);
        assert!(!a.derivadas().contains_key("voltas"));
    }

    #[test]
    fn reposicionar_o_carro_nao_e_cambalhota() {
        let mut a = av_giro(vec![cv("360", "giro", 360, false)]);
        let mut t = 0.0;
        parado(&mut a, &mut t, 0.0, 0.5);
        girar(&mut a, &mut t, 0.0, 180.0);
        parado(&mut a, &mut t, 180.0, 3.0);
        // O jogo desvira o carro de uma vez: 180° numa leitura.
        let ev = parado(&mut a, &mut t, 0.0, 2.0);
        assert!(ev.is_empty());
        assert!(!a.derivadas().contains_key("voltas"));
    }

    #[test]
    fn voltas_somadas_com_marcos() {
        let mut a = av_giro(vec![cv("dubl", "voltas", 4, true)]);
        let mut t = 0.0;
        parado(&mut a, &mut t, 0.0, 0.5);
        let mut ev = vec![];
        for _ in 0..4 {
            ev.extend(girar(&mut a, &mut t, 0.0, 360.0));
            ev.extend(parado(&mut a, &mut t, 0.0, 1.5));
        }
        assert_eq!(
            ev,
            vec![
                Evento::Marco {
                    conquista: "dubl".into(),
                    pct: 25
                },
                Evento::Marco {
                    conquista: "dubl".into(),
                    pct: 50
                },
                Evento::Marco {
                    conquista: "dubl".into(),
                    pct: 75
                },
                Evento::Desbloqueio {
                    conquista: "dubl".into()
                },
            ]
        );
    }

    #[test]
    fn tempo_seguido_acima_do_limite() {
        let mut a = Avaliador::new(
            vec![cv("30s", "t250", 30, false)],
            "perfil".into(),
            vec![],
            vec![Derivada {
                nome: "t250".into(),
                fonte: "kmh".into(),
                tipo: TipoDerivada::TempoAcima { limite: 250 },
                rodas: None,
                velocidade_vetor: None,
                filtro: Filtro::default(),
            }],
            vec![],
            1,
        );
        let mut t = 0.0;
        let mut ev = vec![];
        // `None` = leitura falhou.
        let mut andar = |kmh: Option<i64>, s: f64, ev: &mut Vec<Evento>| {
            for _ in 0..(s * 30.0).round() as u32 {
                t += 1.0 / 30.0;
                let mut v = HashMap::from([("perfil".into(), Valor::Texto("X".into()))]);
                if let Some(k) = kmh {
                    v.insert("kmh".into(), Valor::Num(k));
                }
                ev.extend(a.passo(&v, t).eventos);
            }
        };
        // 20 s acima, 1 s abaixo (zera), depois 26 s com quedas e falhas curtas.
        andar(Some(270), 20.0, &mut ev);
        andar(Some(200), 1.0, &mut ev);
        for _ in 0..5 {
            andar(Some(270), 5.0, &mut ev);
            andar(Some(245), 0.3, &mut ev);
        }
        andar(Some(270), 1.0, &mut ev);
        andar(None, 0.3, &mut ev);
        assert!(ev.is_empty(), "27,8 s: ainda não");
        andar(Some(270), 3.0, &mut ev);
        assert_eq!(desbloqueios(&ev), ["30s"]);
    }

    fn av_salto(conquistas: Vec<Conquista>) -> Avaliador {
        let d = |nome: &str, tipo| Derivada {
            nome: nome.into(),
            fonte: "pos".into(),
            tipo,
            rodas: Some("rodas".into()),
            velocidade_vetor: Some("vel".into()),
            filtro: Filtro::default(),
        };
        Avaliador::new(
            conquistas,
            "perfil".into(),
            vec![],
            vec![
                d("salto", TipoDerivada::Salto),
                d("no_ar", TipoDerivada::SaltosSomados),
            ],
            vec![],
            1,
        )
    }

    /// Simula `s` segundos a 30 Hz andando a `vx` m/s. No chão as rodas tremem;
    /// no ar ficam paradas em -0,028 e vz cai 16 m/s² (gravidade do NFSU2).
    struct Pista {
        t: f64,
        x: f32,
        z: f32,
        vz: f32,
        i: u32,
    }

    impl Pista {
        fn andar(&mut self, a: &mut Avaliador, vx: f32, s: f64, no_ar: bool) -> Vec<Evento> {
            let mut ev = vec![];
            for _ in 0..(s * 30.0).round() as u32 {
                let dt = 1.0 / 30.0;
                self.t += dt;
                self.i += 1;
                self.x += vx * dt as f32;
                let rodas = if no_ar {
                    self.vz -= 16.0 * dt as f32;
                    self.z += self.vz * dt as f32;
                    vec![-0.028; 4]
                } else {
                    self.vz = 0.0;
                    let r = 0.15 + 0.01 * (self.i % 3) as f32;
                    vec![r, r + 0.002, r - 0.003, r]
                };
                let v = HashMap::from([
                    ("perfil".into(), Valor::Texto("X".into())),
                    ("pos".into(), Valor::Vec3([self.x, 0.0, self.z])),
                    ("vel".into(), Valor::Vec3([vx, 0.0, self.vz])),
                    ("rodas".into(), Valor::Floats(rodas)),
                ]);
                ev.extend(a.passo(&v, self.t).eventos);
            }
            ev
        }
    }

    fn pista() -> Pista {
        Pista {
            t: 0.0,
            x: 0.0,
            z: 10.0,
            vz: 0.0,
            i: 0,
        }
    }

    #[test]
    fn salto_mede_metros_e_soma_no_pouso() {
        let mut a = av_salto(vec![
            cv("30m", "salto", 30, false),
            cv("50m", "salto", 50, false),
            cv("km", "no_ar", 100, true),
        ]);
        let mut p = pista();
        assert!(p.andar(&mut a, 35.0, 1.0, false).is_empty());
        p.vz = 10.0;
        // 1,2 s no ar a 35 m/s = 42 m.
        let ev = p.andar(&mut a, 35.0, 1.2, true);
        assert_eq!(desbloqueios(&ev), ["30m"]);
        assert!(!a.derivadas().contains_key("no_ar"), "soma só no pouso");
        let ev = p.andar(&mut a, 35.0, 1.0, false);
        let m = a.derivadas()["no_ar"];
        assert!((40..=44).contains(&m), "{m}");
        assert_eq!(a.valores()["salto"], 0);
        assert!(ev.contains(&Evento::Marco {
            conquista: "km".into(),
            pct: 25
        }));
    }

    #[test]
    fn solavanco_curto_e_carro_parado_nao_sao_salto() {
        let mut a = av_salto(vec![cv("1m", "salto", 1, false)]);
        let mut p = pista();
        p.andar(&mut a, 30.0, 1.0, false);
        // 2 leituras no ar (menos que LEITURAS_DECOLAR): não decola.
        p.vz = 2.0;
        assert!(p.andar(&mut a, 30.0, 2.0 / 30.0, true).is_empty());
        p.andar(&mut a, 30.0, 1.0, false);
        // Parado: rodas e vz constantes (nem caindo).
        let mut ev = vec![];
        for i in 0..100 {
            let v = HashMap::from([
                ("perfil".into(), Valor::Texto("X".into())),
                (
                    "pos".into(),
                    Valor::Vec3([p.x + (i % 2) as f32 * 0.001, 0.0, 10.0]),
                ),
                ("vel".into(), Valor::Vec3([0.0, 0.0, 0.0])),
                ("rodas".into(), Valor::Floats(vec![0.2; 4])),
            ]);
            p.t += 1.0 / 30.0;
            ev.extend(a.passo(&v, p.t).eventos);
        }
        assert!(ev.is_empty());
        assert!(!a.derivadas().contains_key("no_ar"));
    }

    #[test]
    fn teletransporte_no_ar_cancela_o_salto() {
        let mut a = av_salto(vec![cv("30m", "salto", 30, false)]);
        let mut p = pista();
        p.andar(&mut a, 35.0, 1.0, false);
        p.vz = 10.0;
        p.andar(&mut a, 35.0, 0.3, true);
        // O jogo reposiciona o carro 2 km adiante (ex.: caiu do mapa).
        p.x += 2000.0;
        let ev = p.andar(&mut a, 35.0, 0.6, true);
        let ev2 = p.andar(&mut a, 35.0, 1.0, false);
        assert!(ev.is_empty() && ev2.is_empty(), "{ev:?} {ev2:?}");
        assert!(a.derivadas().get("no_ar").is_none_or(|&m| m < 30));
    }

    /// Roda o detector numa gravação CSV (t, pos xyz, vel xyz, 4 rodas) e lista
    /// os saltos. Para calibrar: `SPEEDCONQUEST_GRAVACAO=arq.csv cargo nextest
    /// run --run-ignored only saltos_da_gravacao --no-capture`.
    #[test]
    #[ignore = "precisa de SPEEDCONQUEST_GRAVACAO"]
    fn saltos_da_gravacao() {
        let Some(arq) = std::env::var_os("SPEEDCONQUEST_GRAVACAO") else {
            return;
        };
        let mut a = av_salto(vec![]);
        let texto = std::fs::read_to_string(arq).unwrap();
        let mut ultimo = 0;
        for linha in texto.lines() {
            let c: Vec<f32> = linha.split(',').map(|x| x.parse().unwrap()).collect();
            let v = HashMap::from([
                ("perfil".into(), Valor::Texto("X".into())),
                ("pos".into(), Valor::Vec3([c[1], c[2], c[3]])),
                ("vel".into(), Valor::Vec3([c[4], c[5], c[6]])),
                ("rodas".into(), Valor::Floats(c[7..11].to_vec())),
            ]);
            a.passo(&v, f64::from(c[0]));
            let total = a.derivadas().get("no_ar").copied().unwrap_or(0);
            if total != ultimo {
                eprintln!("t={:.1} salto de {} m", c[0], total - ultimo);
                ultimo = total;
            }
        }
    }

    /// Avaliador com a contagem de vitórias URL (hashes 10 e 11) e a soma de outruns.
    fn av_carreira() -> Avaliador {
        let d = |nome: &str, fonte: &str, tipo, filtro| Derivada {
            nome: nome.into(),
            fonte: fonte.into(),
            tipo,
            rodas: None,
            velocidade_vetor: None,
            filtro,
        };
        Avaliador::new(
            vec![cv("url", "url", 2, true), cv("outruns", "outruns", 4, true)],
            "perfil".into(),
            vec![],
            vec![
                d(
                    "url",
                    "feitos",
                    TipoDerivada::Conta,
                    Filtro {
                        igual: Some(1),
                        chaves: Some("hashes".into()),
                        em: HashSet::from([10, 11]),
                        corrida: None,
                    },
                ),
                d(
                    "outruns",
                    "etapa_outruns",
                    TipoDerivada::Soma,
                    Filtro::default(),
                ),
            ],
            vec![],
            2,
        )
    }

    /// `feitos[i]` com hashes 10, 11, 12 (12 não é URL); `outruns` por etapa.
    fn carreira(perfil: &str, feitos: [i64; 3], outruns: [i64; 2]) -> HashMap<String, Valor> {
        HashMap::from([
            ("perfil".into(), Valor::Texto(perfil.into())),
            ("feitos".into(), Valor::Inteiros(feitos.to_vec())),
            ("hashes".into(), Valor::Inteiros(vec![10, 11, 12])),
            ("etapa_outruns".into(), Valor::Inteiros(outruns.to_vec())),
        ])
    }

    #[test]
    fn conta_filtra_por_valor_e_chave() {
        let mut a = av_carreira();
        let mut t = 0.0;
        let mut ev = vec![];
        for leitura in [
            carreira("X", [0, 0, 0], [0, 0]),
            carreira("X", [0, 0, 0], [0, 0]),
            // Hash 12 não é URL, valor 5 não é "vencida".
            carreira("X", [5, 0, 1], [0, 0]),
            carreira("X", [1, 0, 1], [1, 0]),
            carreira("X", [1, 1, 1], [1, 3]),
        ] {
            t += 0.1;
            ev.extend(a.passo(&leitura, t).eventos);
        }
        assert_eq!(a.valores()["url"], 2);
        assert_eq!(a.valores()["outruns"], 4);
        assert_eq!(
            ev,
            vec![
                Evento::Marco {
                    conquista: "url".into(),
                    pct: 50
                },
                Evento::Marco {
                    conquista: "outruns".into(),
                    pct: 25
                },
                Evento::Desbloqueio {
                    conquista: "url".into()
                },
                Evento::Desbloqueio {
                    conquista: "outruns".into()
                },
            ]
        );
    }

    #[test]
    fn conta_nao_desce_nem_desbloqueia_ao_voltar() {
        let mut a = av_carreira();
        let mut t = 0.0;
        let mut ev = vec![];
        // Save que já tem tudo: base. Depois o jogo zera a carreira por um
        // instante (mesmo perfil) e recarrega: nada dispara.
        for leitura in [
            carreira("X", [1, 1, 0], [2, 2]),
            carreira("X", [1, 1, 0], [2, 2]),
            carreira("X", [0, 0, 0], [0, 0]),
            carreira("X", [1, 1, 0], [2, 2]),
        ] {
            t += 0.1;
            ev.extend(a.passo(&leitura, t).eventos);
        }
        assert!(ev.is_empty(), "{ev:?}");
        // Outro perfil zera o máximo: a base é refeita com o save dele.
        for leitura in [
            carreira("Y", [0, 0, 0], [0, 0]),
            carreira("Y", [0, 0, 0], [0, 0]),
            carreira("Y", [1, 0, 0], [0, 0]),
        ] {
            t += 0.1;
            ev.extend(a.passo(&leitura, t).eventos);
        }
        assert_eq!(
            ev,
            vec![Evento::Marco {
                conquista: "url".into(),
                pct: 50
            }]
        );
    }

    #[test]
    fn conta_some_se_as_chaves_nao_batem() {
        let mut a = av_carreira();
        let mut v = carreira("X", [1, 1, 1], [0, 0]);
        v.insert("hashes".into(), Valor::Inteiros(vec![10]));
        a.passo(&v, 0.0);
        assert!(!a.valores().contains_key("url"));
        assert_eq!(a.valores()["outruns"], 0);
    }

    #[test]
    fn marco_arredonda_para_cima() {
        let k = c("drag", 34);
        // 25% de 34 = 8,5: só a 9ª vitória cruza.
        assert_eq!(transicao(&k, 7, 8), None);
        assert_eq!(
            transicao(&k, 8, 9),
            Some(Evento::Marco {
                conquista: "drag".into(),
                pct: 25
            })
        );
        assert_eq!(
            transicao(&c("dois", 2), 0, 1).map(|e| format!("{e:?}")),
            Some("Marco { conquista: \"dois\", pct: 50 }".into())
        );
    }

    /// Tabela de 4 linhas de 6 campos: posição, id, status, tempo, trocas, perfeitas.
    fn av_corrida() -> Avaliador {
        let d = |nome: &str, filtro: FiltroCorrida| Derivada {
            nome: nome.into(),
            fonte: "tab".into(),
            tipo: TipoDerivada::Corridas,
            rodas: None,
            velocidade_vetor: None,
            filtro: Filtro {
                corrida: Some(filtro),
                ..Filtro::default()
            },
        };
        Avaliador::new(
            vec![
                cv("passeio", "passeios", 1, false),
                cv("relojoeiro", "drags", 1, false),
            ],
            "perfil".into(),
            vec![],
            vec![
                d(
                    "passeios",
                    FiltroCorrida {
                        pistas: vec![40],
                        vencer: true,
                        vantagem_min_s: Some(30),
                        trocas_perfeitas: false,
                    },
                ),
                d(
                    "drags",
                    FiltroCorrida {
                        pistas: vec![42, 44],
                        vencer: true,
                        vantagem_min_s: None,
                        trocas_perfeitas: true,
                    },
                ),
            ],
            vec![],
            2,
        )
        .com_corrida(Some(CorridaCfg {
            tabela: "tab".into(),
            corredores: "n".into(),
            pista: "pista".into(),
            id_jogador: 0,
            terminou: 1,
            ticks_por_s: 4000,
            campo: crate::config::CamposCorrida {
                posicao: 0,
                id: 1,
                status: 2,
                tempo: 3,
                trocas: 4,
                trocas_perfeitas: 5,
            },
            largura: 6,
        }))
    }

    /// `linhas`: (id, status, tempo em s, trocas, perfeitas), já na ordem de chegada.
    /// Completa até 4 linhas com lixo de corrida antiga (não deve ser lido).
    fn tab(pista: i64, linhas: &[(i64, i64, f64, i64, i64)]) -> HashMap<String, Valor> {
        let mut t = vec![];
        for (i, &(id, st, s, tr, pf)) in linhas.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)] // teste: tempos pequenos
            let ticks = (s * 4000.0) as i64;
            t.extend([i as i64 + 1, id, st, ticks, tr, pf]);
        }
        for i in linhas.len()..4 {
            t.extend([i as i64 + 1, 0, 1, 1, 9, 9]);
        }
        HashMap::from([
            ("perfil".into(), Valor::Texto("EU".into())),
            ("tab".into(), Valor::Inteiros(t)),
            (
                "n".into(),
                Valor::Inteiros(vec![1, linhas.len() as i64 - 1, 0, 0]),
            ),
            ("pista".into(), Valor::Num(pista)),
        ])
    }

    /// Lê `v` por `s` segundos a 30 Hz a partir de `*t`.
    fn ler_por(a: &mut Avaliador, t: &mut f64, v: &HashMap<String, Valor>, s: f64) -> Vec<Evento> {
        let mut ev = vec![];
        let fim = *t + s;
        while *t < fim {
            ev.extend(a.passo(v, *t).eventos);
            *t += 1.0 / 30.0;
        }
        ev
    }

    #[test]
    fn circuito_com_30s_de_vantagem() {
        let mut a = av_corrida();
        let mut t = 0.0;
        // Base: a última corrida (vencida com folga) já estava na tabela.
        let velha = tab(4021, &[(0, 1, 100.0, 0, 0), (100, 1, 200.0, 0, 0)]);
        assert!(ler_por(&mut a, &mut t, &velha, 2.0).is_empty());
        // 29 s não basta; URL (47xx) não é circuito comum; 2º sem tempo não conta.
        for v in [
            tab(4021, &[(0, 1, 100.0, 0, 0), (100, 1, 129.0, 0, 0)]),
            tab(4701, &[(0, 1, 100.0, 0, 0), (100, 1, 140.0, 0, 0)]),
            tab(4021, &[(0, 1, 100.0, 0, 0), (100, 9, 0.0, 0, 0)]),
            tab(4021, &[(0, 1, 100.0, 0, 0)]),
            tab(4021, &[(100, 1, 60.0, 0, 0), (0, 1, 100.0, 0, 0)]),
        ] {
            assert!(ler_por(&mut a, &mut t, &v, 2.0).is_empty(), "{v:?}");
        }
        assert_eq!(a.derivadas().get("passeios").copied().unwrap_or(0), 0);
        let ev = ler_por(
            &mut a,
            &mut t,
            &tab(4021, &[(0, 1, 100.0, 0, 0), (101, 1, 130.5, 0, 0)]),
            2.0,
        );
        assert_eq!(desbloqueios(&ev), ["passeio"]);
        assert_eq!(a.derivadas()["passeios"], 1);
    }

    #[test]
    fn tabela_nova_so_conta_parada_e_uma_vez() {
        let mut a = av_corrida();
        let mut t = 0.0;
        let vazia = tab(4021, &[]);
        ler_por(&mut a, &mut t, &vazia, 1.0);
        // Escrita em dois quadros: a meia tabela some antes de 1 s.
        let meia = tab(4021, &[(0, 1, 100.0, 0, 0), (100, 0, 0.0, 0, 0)]);
        let cheia = tab(4021, &[(0, 1, 100.0, 0, 0), (100, 1, 140.0, 0, 0)]);
        assert!(ler_por(&mut a, &mut t, &meia, 0.5).is_empty());
        let ev = ler_por(&mut a, &mut t, &cheia, 3.0);
        assert_eq!(desbloqueios(&ev), ["passeio"]);
        assert_eq!(a.derivadas()["passeios"], 1);
        // Sair para o menu: o jogo mexe em outra linha e a pista vira a do menu.
        let mut menu = cheia.clone();
        if let Some(Valor::Inteiros(t)) = menu.get_mut("tab") {
            t[23] = 7;
        }
        menu.insert("pista".into(), Valor::Num(2000));
        assert!(ler_por(&mut a, &mut t, &menu, 3.0).is_empty());
        assert_eq!(a.derivadas()["passeios"], 1);
        // Largada da corrida seguinte: muda o número de corredores, a tabela não.
        let mut largada = cheia.clone();
        largada.insert("n".into(), Valor::Inteiros(vec![1, 3, 0, 0]));
        assert!(ler_por(&mut a, &mut t, &largada, 3.0).is_empty());
        assert_eq!(a.derivadas()["passeios"], 1);
    }

    #[test]
    fn drag_so_com_trocas_perfeitas() {
        let mut a = av_corrida();
        let mut t = 0.0;
        ler_por(&mut a, &mut t, &tab(4201, &[]), 1.0);
        for v in [
            tab(4201, &[(0, 1, 13.0, 5, 4), (100, 1, 14.0, 5, 5)]),
            tab(4201, &[(0, 1, 13.5, 0, 0), (100, 1, 14.0, 5, 5)]),
            tab(4201, &[(100, 1, 12.0, 5, 5), (0, 1, 13.0, 5, 5)]),
            tab(4021, &[(0, 1, 13.0, 5, 5), (100, 1, 14.0, 5, 5)]),
        ] {
            assert!(ler_por(&mut a, &mut t, &v, 2.0).is_empty(), "{v:?}");
        }
        let ev = ler_por(
            &mut a,
            &mut t,
            &tab(4404, &[(0, 1, 13.0, 5, 5), (100, 1, 13.1, 6, 1)]),
            2.0,
        );
        assert_eq!(desbloqueios(&ev), ["relojoeiro"]);
    }

    #[test]
    fn corrida_trocar_de_perfil_refaz_a_base() {
        let mut a = av_corrida();
        let mut t = 0.0;
        let v = tab(4021, &[(0, 1, 100.0, 0, 0), (100, 1, 140.0, 0, 0)]);
        ler_por(&mut a, &mut t, &v, 1.0);
        let mut outro = v.clone();
        outro.insert("perfil".into(), Valor::Texto("OUTRO".into()));
        // Mesmo resultado lido de novo no outro perfil: é base, não corrida nova.
        assert!(ler_por(&mut a, &mut t, &outro, 2.0).is_empty());
        assert!(ler_por(&mut a, &mut t, &v, 2.0).is_empty());
    }

    #[test]
    fn porcentagens() {
        assert_eq!(porcentagem(2_500, 10_000), 25);
        assert_eq!(porcentagem(-5, 10_000), 0);
        assert_eq!(porcentagem(99_999_999, 10_000), 100);
    }
}
