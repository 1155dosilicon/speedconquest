//! speedconquest — conquistas offline para jogos de PC rodando no Wine.
//!
//! Uso:
//!   speedconquest                vigia: espera um jogo suportado e avalia as conquistas
//!   speedconquest janela         janela com a lista de conquistas (egui)
//!   speedconquest lista          mostra as conquistas de cada perfil
//!   speedconquest relatorio      resumo do histórico de dinheiro (para calibrar metas)
//!   speedconquest popup NIVEL TITULO LINHA2 [PCT]   mostra um popup (uso interno/teste)
//!   speedconquest png SAIDA.png  desenha popups de exemplo num PNG (sem X)
//!
//! Banco: $SPEEDCONQUEST_DB ou ~/.local/share/speedconquest/conquistas.db

mod avaliador;
mod banco;
mod config;
mod janela;
mod popup;
mod processo;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::avaliador::{Avaliador, Evento, porcentagem};
use crate::banco::Banco;
use crate::config::{Jogo, Nivel};
use crate::popup::Aviso;
use crate::processo::{Anexo, Processo};

/// Leituras por segundo.
const HZ: u64 = 30;
/// Leituras (1 s) após trocar de perfil que só servem de base.
const LEITURAS_BASE: u32 = 30;
/// Intervalo mínimo entre gravações das estatísticas (o odômetro muda a cada leitura).
const GRAVAR_A_CADA: Duration = Duration::from_secs(10);
/// Intervalo entre gravações dos maiores valores (o que a janela mostra).
const MAIORES_A_CADA: Duration = Duration::from_secs(2);

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("vigiar") => vigiar(),
        Some("janela") => janela::abrir(abrir_banco()?, config::jogos()?),
        Some("lista") => lista(),
        Some("relatorio") => relatorio(),
        Some("popup") => {
            let [_, nivel, titulo, linha2, resto @ ..] = args.as_slice() else {
                bail!("uso: popup NIVEL TITULO LINHA2 [PCT]");
            };
            let nivel =
                Nivel::de_texto(nivel).with_context(|| format!("nível inválido: {nivel}"))?;
            let progresso = resto.first().map(|p| p.parse::<u8>()).transpose()?;
            popup::exibir(&Aviso {
                nivel,
                titulo: titulo.clone(),
                linha2: linha2.clone(),
                progresso,
            })
        }
        Some("png") => {
            let saida = args.get(1).context("uso: png SAIDA.png")?;
            exemplo_png(saida)
        }
        Some(outro) => bail!("comando desconhecido: {outro} (veja o começo de src/main.rs)"),
    }
}

fn abrir_banco() -> Result<Banco> {
    let caminho = match std::env::var_os("SPEEDCONQUEST_DB") {
        Some(p) => PathBuf::from(p),
        None => banco::caminho_padrao()?,
    };
    Banco::abrir(&caminho)
}

/// Desenha o popup de cada conquista (e um aviso de progresso) em SAIDA-N.png.
fn exemplo_png(saida: &str) -> Result<()> {
    let base = std::path::Path::new(saida);
    let mut avisos = vec![];
    for jogo in config::jogos()? {
        for c in &jogo.conquistas {
            avisos.push(Aviso {
                nivel: c.nivel,
                titulo: c.nome.clone(),
                linha2: c.descricao.clone(),
                progresso: None,
            });
            if c.marcos && avisos.len() < 2 {
                avisos.push(Aviso {
                    nivel: c.nivel,
                    titulo: c.nome.clone(),
                    linha2: format!("50% · {}", c.descricao),
                    progresso: Some(50),
                });
            }
        }
    }
    for (i, av) in avisos.iter().enumerate() {
        let p = base.with_file_name(format!(
            "{}-{i:02}.png",
            base.file_stem().and_then(|s| s.to_str()).unwrap_or("popup")
        ));
        popup::salvar_png(av, 1.0, &p)?;
        println!("{}", p.display());
    }
    Ok(())
}

fn lista() -> Result<()> {
    let banco = abrir_banco()?;
    for jogo in config::jogos()? {
        let perfis = banco.perfis(&jogo.id)?;
        println!("{} ({} conquistas)", jogo.nome, jogo.conquistas.len());
        if perfis.is_empty() {
            println!("  nenhum perfil com progresso ainda");
        }
        for perfil in perfis {
            let feitas = jogo
                .conquistas
                .iter()
                .filter(|c| {
                    banco
                        .desbloqueada(&jogo.id, &perfil, &c.id)
                        .unwrap_or(false)
                })
                .count();
            println!("  perfil {perfil}: {feitas}/{}", jogo.conquistas.len());
            for c in &jogo.conquistas {
                let estado = if banco.desbloqueada(&jogo.id, &perfil, &c.id)? {
                    "✓".to_string()
                } else {
                    format!("{}%", banco.maior_marco(&jogo.id, &perfil, &c.id)?)
                };
                println!(
                    "    [{estado:>4}] {:<7} {} — {}",
                    c.nivel.como_texto(),
                    c.nome,
                    c.descricao
                );
            }
        }
    }
    Ok(())
}

/// Mudanças com menos que isto entre si contam como a mesma sessão de jogo.
const PAUSA_MS: i64 = 10 * 60 * 1000;

fn moeda(v: i64) -> String {
    format!(
        "{}US$ {}",
        if v < 0 { "-" } else { "" },
        config::milhares(v.saturating_abs())
    )
}

fn relatorio() -> Result<()> {
    let banco = abrir_banco()?;
    for jogo in config::jogos()? {
        println!("{}", jogo.nome);
        let vars = jogo.registrar();
        let perfis = banco.perfis(&jogo.id)?;
        if perfis.is_empty() {
            println!("  nada registrado ainda");
        }
        for perfil in perfis {
            println!("  perfil {perfil}");
            for (nome, valor) in banco.estatisticas(&jogo.id, &perfil)? {
                let distancia = jogo
                    .derivada
                    .get(&nome)
                    .is_some_and(|d| d.distancia_de.is_some());
                if distancia {
                    // Metros → km com uma casa; odômetro cabe folgado num f64.
                    #[allow(clippy::cast_precision_loss)] // ver acima
                    let km = valor as f64 / 1000.0;
                    println!("    {nome}: {km:.1} km");
                } else {
                    println!("    {nome}: {}", moeda(valor));
                }
            }
            for var in &vars {
                let h = banco.historico(&jogo.id, &perfil, var)?;
                if h.is_empty() {
                    continue;
                }
                let mut aumentos: Vec<i64> =
                    h.iter().map(|(_, a, d)| d - a).filter(|x| *x > 0).collect();
                let gasto: i64 = h.iter().map(|(_, a, d)| d - a).filter(|x| *x < 0).sum();
                let ativo_ms: i64 = h
                    .windows(2)
                    .map(|w| w[1].0 - w[0].0)
                    .filter(|g| *g < PAUSA_MS)
                    .sum();
                aumentos.sort_unstable();
                let soma: i64 = aumentos.iter().sum();
                println!("    {var}: {} mudanças", h.len());
                if let (Some(min), Some(max)) = (aumentos.first(), aumentos.last()) {
                    let n = i64::try_from(aumentos.len()).unwrap_or(i64::MAX);
                    println!(
                        "      aumentos: {n} (soma {}, média {}, mediana {}, menor {}, maior {})",
                        moeda(soma),
                        moeda(soma / n),
                        moeda(aumentos[aumentos.len() / 2]),
                        moeda(*min),
                        moeda(*max)
                    );
                }
                println!("      gastos: {}", moeda(-gasto));
                if ativo_ms > 60_000 {
                    let horas = ativo_ms as f64 / 3_600_000.0;
                    println!(
                        "      tempo ativo ~{:.1} h → {} por hora",
                        horas,
                        moeda((soma as f64 / horas) as i64)
                    );
                }
            }
        }
    }
    Ok(())
}

fn vigiar() -> Result<()> {
    let jogos = config::jogos()?;
    let banco = abrir_banco()?;
    // pid → última mensagem de erro (para não repetir no log a cada volta).
    let mut problemas: HashMap<u32, String> = HashMap::new();
    eprintln!(
        "speedconquest: esperando um jogo ({})",
        jogos
            .iter()
            .map(|j| j.exe.nome.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    loop {
        for jogo in &jogos {
            for pid in processo::procurar(&jogo.exe.nome) {
                let msg = match Processo::anexar(pid, jogo) {
                    Ok(Anexo::Ok(p)) => {
                        eprintln!("speedconquest: {} (pid {pid}) anexado", jogo.nome);
                        if let Err(e) = vigiar_processo(&p, jogo, &banco) {
                            eprintln!("speedconquest: erro vigiando pid {pid}: {e:#}");
                        }
                        eprintln!("speedconquest: {} (pid {pid}) terminou", jogo.nome);
                        continue;
                    }
                    Ok(Anexo::VersaoDesconhecida { caminho, sha256 }) => format!(
                        "{}: versão do exe desconhecida, ignorando ({}, sha256 {sha256})",
                        jogo.nome,
                        caminho.display()
                    ),
                    Err(e) => format!("{}: não deu para anexar no pid {pid}: {e:#}", jogo.nome),
                };
                if problemas.get(&pid) != Some(&msg) {
                    eprintln!("speedconquest: {msg}");
                    problemas.insert(pid, msg);
                }
            }
        }
        problemas.retain(|pid, _| std::path::Path::new(&format!("/proc/{pid}")).exists());
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Variáveis de ambiente gráficas do processo do jogo (o popup vai para o mesmo
/// display, que no gamescope é o Xwayland dele).
fn ambiente_grafico(pid: u32) -> Vec<(String, String)> {
    let Ok(dados) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return Vec::new();
    };
    dados
        .split(|&b| b == 0)
        .filter_map(|kv| {
            let s = std::str::from_utf8(kv).ok()?;
            let (k, v) = s.split_once('=')?;
            matches!(k, "DISPLAY" | "XAUTHORITY").then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

struct Popups {
    env: Vec<(String, String)>,
    fila: VecDeque<Aviso>,
    atual: Option<Child>,
}

impl Popups {
    /// Mostra o próximo da fila quando o anterior acabou (um de cada vez).
    fn tick(&mut self) {
        if let Some(c) = &mut self.atual {
            match c.try_wait() {
                Ok(None) => return,
                Ok(Some(_)) | Err(_) => self.atual = None,
            }
        }
        let Some(av) = self.fila.pop_front() else {
            return;
        };
        if !self.env.iter().any(|(k, _)| k == "DISPLAY") {
            eprintln!(
                "speedconquest: jogo sem DISPLAY, popup não mostrado: {}",
                av.titulo
            );
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("speedconquest: current_exe: {e}");
                return;
            }
        };
        let mut cmd = Command::new(exe);
        cmd.arg("popup")
            .arg(av.nivel.como_texto())
            .arg(&av.titulo)
            .arg(&av.linha2);
        if let Some(p) = av.progresso {
            cmd.arg(p.to_string());
        }
        cmd.envs(self.env.iter().map(|(k, v)| (k, v)));
        match cmd.spawn() {
            Ok(c) => self.atual = Some(c),
            Err(e) => eprintln!("speedconquest: popup: {e}"),
        }
    }
}

fn vigiar_processo(p: &Processo, jogo: &Jogo, banco: &Banco) -> Result<()> {
    let mut av = Avaliador::new(
        jogo.conquistas.clone(),
        jogo.perfil.clone(),
        jogo.sem_perfil.clone(),
        jogo.derivadas(),
        jogo.registrar(),
        LEITURAS_BASE,
    )
    .com_corrida(jogo.corrida.clone());
    let mut popups = Popups {
        env: ambiente_grafico(p.pid),
        fila: VecDeque::new(),
        atual: None,
    };
    let mut perfil_log: Option<String> = None;
    // Maiores valores vistos das variáveis de progresso, do perfil atual (para a janela).
    let vars_progresso: Vec<&str> = jogo
        .conquistas
        .iter()
        .filter_map(|c| c.progresso.as_ref().map(|p| p.var.as_str()))
        .collect();
    let mut maiores: HashMap<String, i64> = HashMap::new();
    let mut maiores_novos: HashMap<String, i64> = HashMap::new();
    let mut maiores_gravados = Instant::now();
    let inicio = Instant::now();
    // Odômetro muda a cada leitura: grava no máximo a cada GRAVAR_A_CADA.
    let mut pendente = false;
    let mut gravado = Instant::now();
    // SPEEDCONQUEST_DEBUG=1: mostra os valores lidos uma vez por segundo.
    let debug = std::env::var_os("SPEEDCONQUEST_DEBUG").is_some();
    let mut leituras: u64 = 0;
    while p.vivo() {
        let vars = p.ler_vars(jogo);
        let passo = av.passo(&vars, inicio.elapsed().as_secs_f64());
        leituras += 1;
        if debug && leituras.is_multiple_of(HZ) {
            let mut v: Vec<_> = av.valores().iter().collect();
            v.sort();
            eprintln!("speedconquest: {:?} {v:?}", av.perfil());
        }
        if debug && let Some(r) = &passo.corrida {
            eprintln!("speedconquest: corrida terminou: {r:?}");
        }
        if let Some((antigo, est)) = &passo.perfil_anterior {
            banco.gravar_estatisticas(&jogo.id, antigo, est)?;
            pendente = false;
        }
        if av.perfil().map(str::to_string) != perfil_log {
            if let Some(antigo) = &perfil_log {
                banco.gravar_maiores(&jogo.id, antigo, &maiores_novos)?;
            }
            maiores_novos.clear();
            perfil_log = av.perfil().map(str::to_string);
            maiores = match &perfil_log {
                Some(nome) => banco.maiores(&jogo.id, nome)?,
                None => HashMap::new(),
            };
            match &perfil_log {
                Some(nome) => {
                    let est = banco.estatisticas(&jogo.id, nome)?;
                    eprintln!("speedconquest: perfil ativo: {nome} {est:?}");
                    av.carregar_derivadas(est);
                }
                None => eprintln!("speedconquest: nenhum perfil carregado"),
            }
        }
        if let Some(perfil) = av.perfil() {
            for m in &passo.mudancas {
                banco.registrar(&jogo.id, perfil, &m.var, m.antes, m.depois)?;
            }
            if av.estavel() {
                for var in &vars_progresso {
                    if let Some(&v) = av.valores().get(*var)
                        && maiores.get(*var).is_none_or(|&m| v > m)
                    {
                        maiores.insert((*var).to_string(), v);
                        maiores_novos.insert((*var).to_string(), v);
                    }
                }
            }
            if !maiores_novos.is_empty() && maiores_gravados.elapsed() >= MAIORES_A_CADA {
                banco.gravar_maiores(&jogo.id, perfil, &maiores_novos)?;
                maiores_novos.clear();
                maiores_gravados = Instant::now();
            }
            pendente |= passo.derivadas_mudaram;
            if pendente && gravado.elapsed() >= GRAVAR_A_CADA {
                banco.gravar_estatisticas(&jogo.id, perfil, av.derivadas())?;
                pendente = false;
                gravado = Instant::now();
            }
            for ev in &passo.eventos {
                if let Some(aviso) = registrar(ev, jogo, perfil, av.valores(), banco)? {
                    let desbloqueio = aviso.progresso.is_none();
                    popups.fila.push_back(aviso);
                    if desbloqueio && let Some(aviso) = platina(jogo, perfil, banco)? {
                        popups.fila.push_back(aviso);
                    }
                }
            }
        }
        popups.tick();
        std::thread::sleep(Duration::from_millis(1000 / HZ));
    }
    if let Some(perfil) = av.perfil() {
        if pendente {
            banco.gravar_estatisticas(&jogo.id, perfil, av.derivadas())?;
        }
        banco.gravar_maiores(&jogo.id, perfil, &maiores_novos)?;
    }
    Ok(())
}

/// Grava o evento no banco; devolve o aviso se for novidade para este perfil.
fn registrar(
    ev: &Evento,
    jogo: &Jogo,
    perfil: &str,
    valores: &HashMap<String, i64>,
    banco: &Banco,
) -> Result<Option<Aviso>> {
    let id = match ev {
        Evento::Desbloqueio { conquista } | Evento::Marco { conquista, .. } => conquista,
    };
    let c = jogo
        .conquistas
        .iter()
        .find(|c| &c.id == id)
        .context("conquista sumiu")?;
    Ok(match ev {
        Evento::Desbloqueio { .. } => banco
            .desbloquear(&jogo.id, perfil, id)?
            .then(|| aviso_desbloqueio(c, perfil)),
        Evento::Marco { pct, .. } => banco.marcar(&jogo.id, perfil, id, *pct)?.then(|| {
            let atual = c
                .progresso
                .as_ref()
                .and_then(|p| Some(porcentagem(*valores.get(&p.var)?, p.meta)))
                .unwrap_or(*pct);
            println!("… [{perfil}] {}: {atual}%", c.nome);
            Aviso {
                nivel: c.nivel,
                titulo: c.nome.clone(),
                linha2: format!("{atual}% · {}", c.descricao),
                progresso: Some(atual),
            }
        }),
    })
}

fn aviso_desbloqueio(c: &config::Conquista, perfil: &str) -> Aviso {
    println!(
        "🏆 [{perfil}] {} ({}) — {}",
        c.nome,
        c.nivel.como_texto(),
        c.descricao
    );
    Aviso {
        nivel: c.nivel,
        titulo: c.nome.clone(),
        linha2: c.descricao.clone(),
        progresso: None,
    }
}

/// Desbloqueia a platina se todas as outras estão feitas neste perfil.
fn platina(jogo: &Jogo, perfil: &str, banco: &Banco) -> Result<Option<Aviso>> {
    let Some(p) = jogo.platina() else {
        return Ok(None);
    };
    for c in jogo.conquistas.iter().filter(|c| c.id != p.id) {
        if !banco.desbloqueada(&jogo.id, perfil, &c.id)? {
            return Ok(None);
        }
    }
    Ok(banco
        .desbloquear(&jogo.id, perfil, &p.id)?
        .then(|| aviso_desbloqueio(p, perfil)))
}
