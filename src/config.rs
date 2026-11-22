//! Definição de jogos (`jogos/<id>/jogo.toml`) e conquistas (`conquistas.toml`).
//! Os arquivos vão embutidos no binário.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Jogo {
    pub id: String,
    pub nome: String,
    pub exe: Exe,
    /// Nome da variável que identifica o save ativo.
    pub perfil: String,
    /// Textos que o jogo põe no lugar do nome quando não há save carregado.
    #[serde(default)]
    pub sem_perfil: Vec<String>,
    pub var: BTreeMap<String, Var>,
    /// Estatísticas calculadas por nós (guardadas por perfil).
    #[serde(default)]
    pub derivada: BTreeMap<String, DerivadaCfg>,
    /// Formato da tabela de resultados de corrida (para `corridas_de`).
    pub corrida: Option<CorridaCfg>,
    /// Como a janela mostra cada variável de progresso (sem entrada = contagem).
    #[serde(default)]
    pub unidade: BTreeMap<String, Unidade>,
    #[serde(skip)]
    pub conquistas: Vec<Conquista>,
    /// Listas nomeadas de números (hashes de corridas etc.), de `listas.toml`.
    #[serde(skip)]
    pub listas: BTreeMap<String, Vec<i64>>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Unidade {
    pub texto: String,
    /// Antes do número (US$) em vez de depois (km/h).
    #[serde(default)]
    pub antes: bool,
    /// Valor guardado ÷ divisor = valor mostrado (metros → km: 1000). Padrão 1.
    #[serde(default)]
    pub divisor: i64,
}

impl Unidade {
    /// Valor formatado em pt-BR: "US$ 12.500", "19,9 km", "287 km/h".
    pub fn formatar(&self, v: i64) -> String {
        let n = if self.divisor > 1 {
            let inteiro = milhares(v / self.divisor);
            let decimo = (v % self.divisor).abs() * 10 / self.divisor;
            format!("{inteiro},{decimo}")
        } else {
            milhares(v)
        };
        if self.antes {
            format!("{} {n}", self.texto)
        } else if self.texto == "°" {
            format!("{n}°")
        } else {
            format!("{n} {}", self.texto)
        }
    }
}

/// Inteiro com separador de milhar em pt-BR (12.500).
pub fn milhares(v: i64) -> String {
    let s = v.unsigned_abs().to_string();
    let mut out = String::new();
    if v < 0 {
        out.push('-');
    }
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

#[derive(Debug, Deserialize)]
pub struct Exe {
    pub nome: String,
    pub bits: u8,
    pub sha256: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tipo {
    I32,
    U32,
    /// Float, arredondado para inteiro (pontos de drift etc.).
    F32,
    Texto,
    /// Três f32 seguidos (x, y, z), ex.: posição do carro em metros.
    Vec3,
    /// `quantos` f32 a cada `passo` bytes, ex.: altura das 4 rodas.
    Floats,
    /// Listas de inteiros (`quantos` a cada `passo` bytes, repetidos em `grupos`
    /// a cada `passo_grupo`), ex.: um campo de cada item de uma tabela do jogo.
    U8s,
    U32s,
    I32s,
}

impl Tipo {
    /// Número (escalar) que pode ser meta de conquista ou fonte de soma.
    pub fn numerico(self) -> bool {
        matches!(self, Tipo::I32 | Tipo::U32 | Tipo::F32)
    }

    /// Lista de inteiros.
    pub fn lista(self) -> bool {
        matches!(self, Tipo::U8s | Tipo::U32s | Tipo::I32s)
    }
}

#[derive(Debug, Deserialize)]
pub struct Var {
    pub tipo: Tipo,
    /// Só para `texto`: bytes lidos (corta no primeiro NUL).
    #[serde(default)]
    pub tamanho: usize,
    /// Só para `floats` e listas: quantos e a distância entre eles em bytes.
    #[serde(default)]
    pub quantos: usize,
    #[serde(default)]
    pub passo: u64,
    /// Só para listas: repete a leitura `grupos` vezes, a cada `passo_grupo`
    /// bytes (ex.: o mesmo campo em cada etapa da carreira). Padrão 1.
    #[serde(default)]
    pub grupos: usize,
    #[serde(default)]
    pub passo_grupo: u64,
    /// `MODULO+0xDESLOC` ou `0xABSOLUTO`.
    pub endereco: String,
    /// Cadeia de ponteiros estilo Cheat Engine: para cada item, lê um ponteiro no
    /// endereço atual e soma o deslocamento.
    #[serde(default)]
    pub ponteiros: Vec<u64>,
    /// Gravar cada mudança no histórico (para calibrar metas). Só para variáveis
    /// que mudam pouco (dinheiro sim, velocidade não).
    #[serde(default)]
    pub registrar: bool,
}

/// Tabela de resultados que o jogo preenche no fim de cada corrida: uma linha
/// (grupo da var `tabela`) por corredor, campos de 4 em 4 bytes.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CorridaCfg {
    /// Var `i32s`/`u32s` com `grupos` = linhas e `quantos` = campos por linha.
    pub tabela: String,
    /// Var lista: a soma é o número de linhas válidas (corredores da corrida).
    pub corredores: String,
    /// Var numérica com o número da pista (os filtros usam `pista / 100`).
    pub pista: String,
    /// Valor do campo `id` na linha do jogador.
    pub id_jogador: i64,
    /// Valor do campo `status` de quem terminou a corrida.
    pub terminou: i64,
    /// Unidades do campo `tempo` por segundo.
    pub ticks_por_s: i64,
    pub campo: CamposCorrida,
    /// Campos por linha (`quantos` da var `tabela`), preenchido ao carregar.
    #[serde(skip)]
    pub largura: usize,
}

/// Índice (em i32, não em bytes) de cada campo dentro da linha.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CamposCorrida {
    /// Colocação final (1 = venceu; a tabela vem ordenada por ela).
    pub posicao: usize,
    pub id: usize,
    pub status: usize,
    pub tempo: usize,
    /// Trocas de marcha (drag) e quantas foram perfeitas.
    pub trocas: usize,
    pub trocas_perfeitas: usize,
}

impl CamposCorrida {
    fn maior(&self) -> usize {
        [
            self.posicao,
            self.id,
            self.status,
            self.tempo,
            self.trocas,
            self.trocas_perfeitas,
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }
}

/// Estatística calculada por nós. Exatamente um dos campos.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivadaCfg {
    /// Soma de todos os aumentos desta variável numérica (gastar não desconta).
    /// Guardada por perfil.
    pub soma_aumentos_de: Option<String>,
    /// Distância percorrida em metros por esta variável `vec3` (odômetro).
    /// Guardada por perfil.
    pub distancia_de: Option<String>,
    /// Velocidade instantânea em km/h desta variável `vec3` (média de ~0,5 s).
    /// Só ao vivo, não é guardada.
    pub velocidade_de: Option<String>,
    /// Graus girados de ponta ou de lado na cambalhota atual, por esta variável
    /// `vec3` (vetor "pra cima" do carro). Só ao vivo.
    pub giro_de: Option<String>,
    /// Voltas completas (360°) em cambalhotas, somadas. Guardada por perfil.
    pub voltas_de: Option<String>,
    /// Segundos seguidos com esta variável numérica (ou derivada ao vivo)
    /// em `limite` ou mais. Só ao vivo.
    pub tempo_acima_de: Option<String>,
    /// Só para `tempo_acima_de`.
    pub limite: Option<i64>,
    /// Metros (na horizontal) do salto em andamento, pela posição `vec3` do
    /// carro. Só ao vivo; 0 no chão. Precisa de `rodas` e `velocidade_vetor`.
    pub salto_de: Option<String>,
    /// Metros de todos os saltos somados (soma no pouso). Guardada por perfil.
    /// Precisa de `rodas` e `velocidade_vetor`.
    pub saltos_somados_de: Option<String>,
    /// Para os saltos: variável `floats` com a altura de cada roda.
    pub rodas: Option<String>,
    /// Para os saltos: variável `vec3` com a velocidade do carro (m/s).
    pub velocidade_vetor: Option<String>,
    /// Quantos itens desta lista são diferentes de 0 (ou iguais a `igual`).
    /// Ao vivo, mas nunca desce (ver `TipoDerivada::Conta`).
    pub conta_de: Option<String>,
    /// Só para `conta_de`: conta só os itens com este valor.
    pub igual: Option<i64>,
    /// Só para `conta_de`: lista paralela de chaves (hashes); conta só os itens
    /// cuja chave está em alguma das listas `em` (de `listas.toml`).
    pub chaves: Option<String>,
    #[serde(default)]
    pub em: Vec<String>,
    /// Soma dos itens desta lista. Ao vivo, nunca desce.
    pub soma_de: Option<String>,
    /// Corridas terminadas que passam nos filtros abaixo, contadas pela tabela
    /// de resultados (a var `tabela` de `[corrida]`). Guardada por perfil.
    pub corridas_de: Option<String>,
    /// Só para `corridas_de`: `pista / 100` tem de estar aqui (vazio = qualquer).
    #[serde(default)]
    pub pistas: Vec<i64>,
    /// Só para `corridas_de`: o jogador terminou em 1º.
    #[serde(default)]
    pub vencer: bool,
    /// Só para `corridas_de`: venceu com pelo menos isto de segundos sobre o 2º.
    pub vantagem_min_s: Option<i64>,
    /// Só para `corridas_de`: todas as trocas de marcha perfeitas (e ao menos uma).
    #[serde(default)]
    pub trocas_perfeitas: bool,
}

impl DerivadaCfg {
    /// (tipo, fonte), se exatamente um tipo foi preenchido.
    fn tipo(&self) -> Option<(crate::avaliador::TipoDerivada, &String)> {
        use crate::avaliador::TipoDerivada as T;
        let tipos = [
            (T::SomaAumentos, &self.soma_aumentos_de),
            (T::Distancia, &self.distancia_de),
            (T::Velocidade, &self.velocidade_de),
            (T::Giro, &self.giro_de),
            (T::Voltas, &self.voltas_de),
            (
                T::TempoAcima {
                    limite: self.limite.unwrap_or(0),
                },
                &self.tempo_acima_de,
            ),
            (T::Salto, &self.salto_de),
            (T::SaltosSomados, &self.saltos_somados_de),
            (T::Conta, &self.conta_de),
            (T::Soma, &self.soma_de),
            (T::Corridas, &self.corridas_de),
        ];
        let mut preenchidos = tipos
            .into_iter()
            .filter_map(|(t, f)| Some((t, f.as_ref()?)));
        let um = preenchidos.next()?;
        preenchidos.next().is_none().then_some(um)
    }
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Nivel {
    Bronze,
    Prata,
    Ouro,
    Platina,
}

impl Nivel {
    pub fn como_texto(self) -> &'static str {
        match self {
            Nivel::Bronze => "bronze",
            Nivel::Prata => "prata",
            Nivel::Ouro => "ouro",
            Nivel::Platina => "platina",
        }
    }

    pub fn de_texto(s: &str) -> Option<Self> {
        Some(match s {
            "bronze" => Nivel::Bronze,
            "prata" => Nivel::Prata,
            "ouro" => Nivel::Ouro,
            "platina" => Nivel::Platina,
            _ => return None,
        })
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Conquista {
    pub id: String,
    pub nivel: Nivel,
    pub nome: String,
    pub descricao: String,
    /// `None` só na platina: desbloqueia sozinha quando todas as outras estão feitas.
    pub progresso: Option<Progresso>,
    /// Avisos de 25/50/75%. Desligar em recordes ao vivo (velocidade, salto):
    /// 25% de 300 km/h não é progresso de nada.
    #[serde(default = "sim")]
    pub marcos: bool,
}

fn sim() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
pub struct Progresso {
    pub var: String,
    pub meta: i64,
}

#[derive(Deserialize)]
struct ArquivoConquistas {
    conquista: Vec<Conquista>,
}

/// Endereço já interpretado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endereco {
    /// `None` = endereço absoluto.
    pub modulo: Option<String>,
    pub desloc: u64,
}

pub fn interpretar_endereco(s: &str) -> Result<Endereco> {
    let s = s.trim();
    let (modulo, num) = match s.rsplit_once('+') {
        Some((m, n)) => (Some(m.trim().to_string()), n.trim()),
        None => (None, s),
    };
    let hex = num
        .strip_prefix("0x")
        .or_else(|| num.strip_prefix("0X"))
        .with_context(|| format!("endereço sem 0x: {s:?}"))?;
    let desloc =
        u64::from_str_radix(hex, 16).with_context(|| format!("endereço inválido: {s:?}"))?;
    Ok(Endereco { modulo, desloc })
}

/// (jogo.toml, conquistas.toml, listas.toml) de cada jogo.
const EMBUTIDOS: &[(&str, &str, &str)] = &[(
    include_str!("../jogos/nfsu2/jogo.toml"),
    include_str!("../jogos/nfsu2/conquistas.toml"),
    include_str!("../jogos/nfsu2/listas.toml"),
)];

fn carregar(jogo_toml: &str, conquistas_toml: &str, listas_toml: &str) -> Result<Jogo> {
    let mut jogo: Jogo = toml::from_str(jogo_toml).context("jogo.toml")?;
    let c: ArquivoConquistas = toml::from_str(conquistas_toml)
        .with_context(|| format!("conquistas.toml de {}", jogo.id))?;
    jogo.conquistas = c.conquista;
    jogo.listas =
        toml::from_str(listas_toml).with_context(|| format!("listas.toml de {}", jogo.id))?;
    if let Some(c) = &mut jogo.corrida {
        c.largura = jogo.var.get(&c.tabela).map_or(0, |v| v.quantos);
    }
    validar(&jogo)?;
    Ok(jogo)
}

fn validar(j: &Jogo) -> Result<()> {
    if j.exe.bits != 32 && j.exe.bits != 64 {
        bail!("{}: bits deve ser 32 ou 64", j.id);
    }
    match j.var.get(&j.perfil) {
        Some(v) if v.tipo == Tipo::Texto && v.tamanho > 0 => {}
        _ => bail!(
            "{}: variável de perfil {:?} deve ser texto com tamanho",
            j.id,
            j.perfil
        ),
    }
    for nome in j.unidade.keys() {
        if !j.var.contains_key(nome) && !j.derivada.contains_key(nome) {
            bail!("{}: [unidade] de variável que não existe: {nome}", j.id);
        }
    }
    for (nome, v) in &j.var {
        interpretar_endereco(&v.endereco).with_context(|| format!("{}: var {nome}", j.id))?;
        let em_lista = v.tipo == Tipo::Floats || v.tipo.lista();
        if em_lista != (v.quantos > 0 && v.passo > 0) {
            bail!(
                "{}: var {nome}: `quantos`/`passo` só (e sempre) em floats e listas",
                j.id
            );
        }
        if (v.grupos > 1) != (v.passo_grupo > 0) || (v.grupos > 1 && !v.tipo.lista()) {
            bail!(
                "{}: var {nome}: `grupos` (> 1) e `passo_grupo` juntos, só em listas",
                j.id
            );
        }
    }
    for (nome, d) in &j.derivada {
        if j.var.contains_key(nome) {
            bail!("{}: derivada {nome} tem o mesmo nome de uma var", j.id);
        }
        use crate::avaliador::TipoDerivada as T;
        let Some((tipo, fonte)) = d.tipo() else {
            bail!("{}: derivada {nome} precisa de exatamente um tipo", j.id);
        };
        if d.limite.is_some() != matches!(tipo, T::TempoAcima { .. }) {
            bail!(
                "{}: derivada {nome}: `limite` só (e sempre) com tempo_acima_de",
                j.id
            );
        }
        let salto = matches!(tipo, T::Salto | T::SaltosSomados);
        let tipo_de =
            |nome: &Option<String>| nome.as_ref().and_then(|n| j.var.get(n)).map(|v| v.tipo);
        if salto
            && (tipo_de(&d.rodas) != Some(Tipo::Floats)
                || tipo_de(&d.velocidade_vetor) != Some(Tipo::Vec3))
        {
            bail!(
                "{}: derivada {nome}: salto precisa de `rodas` (floats) e `velocidade_vetor` (vec3)",
                j.id
            );
        }
        if !salto && (d.rodas.is_some() || d.velocidade_vetor.is_some()) {
            bail!(
                "{}: derivada {nome}: `rodas`/`velocidade_vetor` só em saltos",
                j.id
            );
        }
        let var = j.var.get(fonte);
        if tipo != T::Conta && (d.igual.is_some() || d.chaves.is_some() || !d.em.is_empty()) {
            bail!(
                "{}: derivada {nome}: `igual`/`chaves`/`em` só em conta_de",
                j.id
            );
        }
        let filtro_corrida =
            !d.pistas.is_empty() || d.vencer || d.vantagem_min_s.is_some() || d.trocas_perfeitas;
        if tipo != T::Corridas && filtro_corrida {
            bail!(
                "{}: derivada {nome}: `pistas`/`vencer`/`vantagem_min_s`/`trocas_perfeitas` só em corridas_de",
                j.id
            );
        }
        if tipo == T::Corridas && j.corrida.as_ref().is_none_or(|c| &c.tabela != fonte) {
            bail!(
                "{}: derivada {nome}: corridas_de precisa ser a `tabela` de [corrida]",
                j.id
            );
        }
        if d.chaves.is_some() != !d.em.is_empty() {
            bail!("{}: derivada {nome}: `chaves` e `em` vão juntos", j.id);
        }
        if let Some(ch) = &d.chaves {
            let mesma_forma =
                |a: &Var, b: &Var| a.quantos == b.quantos && a.grupos.max(1) == b.grupos.max(1);
            match (j.var.get(ch), var) {
                (Some(c), Some(v)) if c.tipo.lista() && mesma_forma(c, v) => {}
                _ => bail!(
                    "{}: derivada {nome}: `chaves` deve ser lista do mesmo tamanho",
                    j.id
                ),
            }
            if let Some(l) = d.em.iter().find(|l| !j.listas.contains_key(*l)) {
                bail!(
                    "{}: derivada {nome}: lista {l:?} não existe em listas.toml",
                    j.id
                );
            }
        }
        let ok = match tipo {
            T::SomaAumentos => var.is_some_and(|v| v.tipo.numerico()),
            T::Conta | T::Soma => var.is_some_and(|v| v.tipo.lista()),
            // A tabela em si é conferida com a seção [corrida], abaixo.
            T::Corridas => var.is_some(),
            T::Distancia | T::Velocidade | T::Giro | T::Voltas | T::Salto | T::SaltosSomados => {
                var.is_some_and(|v| v.tipo == Tipo::Vec3)
            }
            // Fonte numérica: var ou outra derivada (ex.: velocidade).
            T::TempoAcima { .. } => {
                var.is_some_and(|v| v.tipo.numerico())
                    || (j.derivada.contains_key(fonte) && fonte != nome)
            }
        };
        if !ok {
            bail!(
                "{}: derivada {nome} usa variável inexistente ou do tipo errado",
                j.id
            );
        }
    }
    if let Some(c) = &j.corrida {
        let tabela_ok = j.var.get(&c.tabela).is_some_and(|v| {
            matches!(v.tipo, Tipo::I32s | Tipo::U32s) && v.passo == 4 && c.campo.maior() < v.quantos
        });
        if !tabela_ok {
            bail!(
                "{}: [corrida]: `tabela` deve ser i32s/u32s de passo 4 com todos os campos",
                j.id
            );
        }
        if !j.var.get(&c.corredores).is_some_and(|v| v.tipo.lista()) {
            bail!("{}: [corrida]: `corredores` deve ser lista", j.id);
        }
        if !j.var.get(&c.pista).is_some_and(|v| v.tipo.numerico()) {
            bail!("{}: [corrida]: `pista` deve ser numérica", j.id);
        }
        if c.ticks_por_s <= 0 {
            bail!("{}: [corrida]: `ticks_por_s` deve ser > 0", j.id);
        }
    }
    let mut ids = std::collections::HashSet::new();
    for c in &j.conquistas {
        if !ids.insert(&c.id) {
            bail!("{}: conquista repetida {}", j.id, c.id);
        }
        let Some(prog) = &c.progresso else {
            if c.nivel != Nivel::Platina {
                bail!("{}: conquista {} sem progresso (só a platina)", j.id, c.id);
            }
            continue;
        };
        let numerica = j.derivada.contains_key(&prog.var)
            || j.var.get(&prog.var).is_some_and(|v| v.tipo.numerico());
        if !numerica {
            bail!(
                "{}: conquista {} usa variável numérica inexistente {:?}",
                j.id,
                c.id,
                prog.var
            );
        }
        if prog.meta <= 0 {
            bail!("{}: conquista {} com meta <= 0", j.id, c.id);
        }
    }
    if j.conquistas
        .iter()
        .filter(|c| c.progresso.is_none())
        .count()
        > 1
    {
        bail!("{}: mais de uma platina", j.id);
    }
    Ok(())
}

pub fn jogos() -> Result<Vec<Jogo>> {
    EMBUTIDOS
        .iter()
        .map(|(j, c, l)| carregar(j, c, l))
        .collect()
}

impl Jogo {
    pub fn derivadas(&self) -> Vec<crate::avaliador::Derivada> {
        self.derivada
            .iter()
            .filter_map(|(nome, d)| {
                // `validar` garante exatamente um tipo preenchido.
                let (tipo, fonte) = d.tipo()?;
                Some(crate::avaliador::Derivada {
                    nome: nome.clone(),
                    fonte: fonte.clone(),
                    tipo,
                    rodas: d.rodas.clone(),
                    velocidade_vetor: d.velocidade_vetor.clone(),
                    filtro: crate::avaliador::Filtro {
                        igual: d.igual,
                        chaves: d.chaves.clone(),
                        em: d
                            .em
                            .iter()
                            .flat_map(|l| self.listas.get(l).into_iter().flatten().copied())
                            .collect(),
                        corrida: (tipo == crate::avaliador::TipoDerivada::Corridas).then(|| {
                            crate::avaliador::FiltroCorrida {
                                pistas: d.pistas.clone(),
                                vencer: d.vencer,
                                vantagem_min_s: d.vantagem_min_s,
                                trocas_perfeitas: d.trocas_perfeitas,
                            }
                        }),
                    },
                })
            })
            .collect()
    }

    /// A platina (conquista sem progresso), se houver.
    pub fn platina(&self) -> Option<&Conquista> {
        self.conquistas.iter().find(|c| c.progresso.is_none())
    }

    pub fn registrar(&self) -> Vec<String> {
        self.var
            .iter()
            .filter(|(_, v)| v.registrar)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn formatar_unidades() {
        let u = |t: &str, antes, divisor| Unidade {
            texto: t.into(),
            antes,
            divisor,
        };
        assert_eq!(u("US$", true, 0).formatar(547_950), "US$ 547.950");
        assert_eq!(u("km", false, 1000).formatar(19_917), "19,9 km");
        assert_eq!(u("km", false, 1000).formatar(5_000_000), "5.000,0 km");
        assert_eq!(u("km/h", false, 0).formatar(287), "287 km/h");
        assert_eq!(u("°", false, 0).formatar(720), "720°");
        assert_eq!(milhares(-1234), "-1.234");
        assert_eq!(milhares(999), "999");
    }

    #[test]
    fn embutidos_carregam() {
        let js = jogos().unwrap();
        assert_eq!(js[0].id, "nfsu2");
        assert_eq!(js[0].conquistas.len(), 42);
        assert_eq!(js[0].sem_perfil, vec!["No Profile".to_string()]);
    }

    #[test]
    fn platina_e_a_unica_sem_progresso() {
        let j = &jogos().unwrap()[0];
        assert_eq!(j.platina().unwrap().id, "platina");
        let jogo = include_str!("../jogos/nfsu2/jogo.toml");
        let l = include_str!("../jogos/nfsu2/listas.toml");
        let sem = "[[conquista]]\nid = \"a\"\nnivel = \"ouro\"\nnome = \"a\"\ndescricao = \"\"\n";
        assert!(carregar(jogo, sem, l).is_err());
        let duas = sem.replace("ouro", "platina")
            + &sem.replace("ouro", "platina").replace("\"a\"", "\"b\"");
        assert!(carregar(jogo, &duas, l).is_err());
        assert!(carregar(jogo, &sem.replace("ouro", "platina"), l).is_ok());
    }

    #[test]
    fn endereco() {
        assert_eq!(
            interpretar_endereco("SPEED2.EXE+0x461E74").unwrap(),
            Endereco {
                modulo: Some("SPEED2.EXE".into()),
                desloc: 0x461E74
            }
        );
        assert_eq!(
            interpretar_endereco("0x861e74").unwrap(),
            Endereco {
                modulo: None,
                desloc: 0x861E74
            }
        );
        assert!(interpretar_endereco("SPEED2.EXE+461E74").is_err());
    }
}
