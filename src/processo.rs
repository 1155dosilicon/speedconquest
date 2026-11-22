//! Achar o processo do jogo (Wine) e ler a memória dele por fora.
//!
//! No Wine o endereço do Windows é o mesmo que o Linux vê, então basta
//! `/proc/<pid>/mem` (precisa de `ptrace_scope` = 0 ou ser pai do processo).

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::config::{Endereco, Jogo, Tipo, interpretar_endereco};

pub struct Processo {
    pub pid: u32,
    mem: File,
    /// Módulos carregados: nome em maiúsculas → endereço base.
    modulos: HashMap<String, u64>,
    bits: u8,
}

/// Valor lido de uma variável.
#[derive(Debug, Clone, PartialEq)]
pub enum Valor {
    Num(i64),
    Texto(String),
    Vec3([f32; 3]),
    /// `quantos` floats espaçados de `passo` bytes (ex.: z das 4 rodas).
    Floats(Vec<f32>),
    /// Listas de inteiros (`u8s`/`u32s`/`i32s`), grupos em sequência.
    Inteiros(Vec<i64>),
}

/// PIDs cujo `comm` bate com o nome do exe (o Wine põe o nome do .exe no comm,
/// cortado em 15 caracteres).
pub fn procurar(nome_exe: &str) -> Vec<u32> {
    let alvo: String = nome_exe.to_ascii_uppercase().chars().take(15).collect();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .is_ok_and(|c| c.trim_end().to_ascii_uppercase() == alvo)
        })
        .collect()
}

/// Lê `/proc/<pid>/maps` e devolve (nome do arquivo em maiúsculas → base, caminho).
/// Base = primeiro mapeamento do arquivo com deslocamento 0.
fn modulos(pid: u32) -> Result<HashMap<String, (u64, PathBuf)>> {
    let maps = std::fs::read_to_string(format!("/proc/{pid}/maps"))?;
    let mut out = HashMap::new();
    for linha in maps.lines() {
        let mut campos = linha.split_whitespace();
        let (Some(faixa), Some(_perm), Some(desloc), Some(_dev), Some(_inode)) = (
            campos.next(),
            campos.next(),
            campos.next(),
            campos.next(),
            campos.next(),
        ) else {
            continue;
        };
        // O caminho pode ter espaços: é o resto da linha.
        let caminho = campos.collect::<Vec<_>>().join(" ");
        if !caminho.starts_with('/') || u64::from_str_radix(desloc, 16) != Ok(0) {
            continue;
        }
        let Some(inicio) = faixa
            .split('-')
            .next()
            .and_then(|s| u64::from_str_radix(s, 16).ok())
        else {
            continue;
        };
        let p = PathBuf::from(&caminho);
        let Some(nome) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        out.entry(nome.to_ascii_uppercase()).or_insert((inicio, p));
    }
    Ok(out)
}

pub fn sha256_arquivo(caminho: &Path) -> Result<String> {
    let mut f = File::open(caminho).with_context(|| format!("abrir {}", caminho.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub enum Anexo {
    Ok(Processo),
    /// Exe com sha256 desconhecido (outra versão do jogo).
    VersaoDesconhecida {
        caminho: PathBuf,
        sha256: String,
    },
}

impl Processo {
    /// Anexa ao processo e confere o sha256 do exe.
    pub fn anexar(pid: u32, jogo: &Jogo) -> Result<Anexo> {
        let mods = modulos(pid)?;
        let nome = jogo.exe.nome.to_ascii_uppercase();
        let (_, caminho) = mods
            .get(&nome)
            .with_context(|| format!("{nome} não está no maps do pid {pid}"))?;
        let sha = sha256_arquivo(caminho)?;
        if !jogo.exe.sha256.iter().any(|s| s.eq_ignore_ascii_case(&sha)) {
            return Ok(Anexo::VersaoDesconhecida {
                caminho: caminho.clone(),
                sha256: sha,
            });
        }
        let mem = File::open(format!("/proc/{pid}/mem"))
            .with_context(|| format!("abrir /proc/{pid}/mem (ptrace_scope?)"))?;
        Ok(Anexo::Ok(Processo {
            pid,
            mem,
            modulos: mods.into_iter().map(|(k, (b, _))| (k, b)).collect(),
            bits: jogo.exe.bits,
        }))
    }

    pub fn vivo(&self) -> bool {
        Path::new(&format!("/proc/{}", self.pid)).exists()
    }

    pub fn ler(&self, endereco: u64, buf: &mut [u8]) -> Result<()> {
        self.mem
            .read_exact_at(buf, endereco)
            .with_context(|| format!("ler {} bytes em {endereco:#x}", buf.len()))
    }

    fn ler_ponteiro(&self, endereco: u64) -> Result<u64> {
        if self.bits == 64 {
            let mut b = [0u8; 8];
            self.ler(endereco, &mut b)?;
            Ok(u64::from_le_bytes(b))
        } else {
            let mut b = [0u8; 4];
            self.ler(endereco, &mut b)?;
            Ok(u64::from(u32::from_le_bytes(b)))
        }
    }

    fn resolver(&self, e: &Endereco, ponteiros: &[u64]) -> Result<u64> {
        let base = match &e.modulo {
            Some(m) => *self
                .modulos
                .get(&m.to_ascii_uppercase())
                .with_context(|| format!("módulo {m} não carregado"))?,
            None => 0,
        };
        let mut a = base.wrapping_add(e.desloc);
        for off in ponteiros {
            let p = self.ler_ponteiro(a)?;
            if p == 0 {
                anyhow::bail!("ponteiro nulo em {a:#x}");
            }
            a = p.wrapping_add(*off);
        }
        Ok(a)
    }

    /// Lista de inteiros: um bloco contíguo por grupo (uma leitura só), e os itens
    /// tirados dele. `None` se qualquer parte falhar.
    fn ler_lista(&self, a: u64, v: &crate::config::Var) -> Option<Vec<i64>> {
        let largura: usize = if v.tipo == Tipo::U8s { 1 } else { 4 };
        let passo = usize::try_from(v.passo).ok()?;
        let vao = passo
            .checked_mul(v.quantos.checked_sub(1)?)?
            .checked_add(largura)?;
        let mut bloco = vec![0u8; vao];
        let mut out = Vec::with_capacity(v.quantos * v.grupos.max(1));
        for g in 0..v.grupos.max(1) {
            let ag = a.checked_add(v.passo_grupo.checked_mul(g as u64)?)?;
            self.ler(ag, &mut bloco).ok()?;
            out.extend(
                bloco
                    .chunks(passo)
                    .take(v.quantos)
                    .map(|c| inteiro_le(v.tipo, c)),
            );
        }
        Some(out)
    }

    /// Lê todas as variáveis do jogo. Variável que falhou fica fora do mapa.
    pub fn ler_vars(&self, jogo: &Jogo) -> HashMap<String, Valor> {
        let mut out = HashMap::new();
        for (nome, v) in &jogo.var {
            // Endereços já foram validados ao carregar a config.
            let Ok(e) = interpretar_endereco(&v.endereco) else {
                continue;
            };
            let Ok(a) = self.resolver(&e, &v.ponteiros) else {
                continue;
            };
            let valor = match v.tipo {
                Tipo::I32 => {
                    let mut b = [0u8; 4];
                    self.ler(a, &mut b)
                        .ok()
                        .map(|()| Valor::Num(i64::from(i32::from_le_bytes(b))))
                }
                Tipo::U32 => {
                    let mut b = [0u8; 4];
                    self.ler(a, &mut b)
                        .ok()
                        .map(|()| Valor::Num(i64::from(u32::from_le_bytes(b))))
                }
                Tipo::F32 => {
                    let mut b = [0u8; 4];
                    self.ler(a, &mut b)
                        .ok()
                        .and_then(|()| f32_para_num(f32::from_le_bytes(b)))
                        .map(Valor::Num)
                }
                Tipo::Texto => {
                    let mut b = vec![0u8; v.tamanho];
                    self.ler(a, &mut b).ok().map(|()| Valor::Texto(texto_c(&b)))
                }
                Tipo::Vec3 => {
                    let mut b = [0u8; 12];
                    self.ler(a, &mut b).ok().map(|()| Valor::Vec3(vec3_le(&b)))
                }
                Tipo::Floats => (0..v.quantos)
                    .map(|i| {
                        let mut b = [0u8; 4];
                        let ai = a.checked_add(v.passo.checked_mul(i as u64)?)?;
                        self.ler(ai, &mut b).ok()?;
                        Some(f32::from_le_bytes(b))
                    })
                    .collect::<Option<Vec<f32>>>()
                    .map(Valor::Floats),
                Tipo::U8s | Tipo::U32s | Tipo::I32s => self.ler_lista(a, v).map(Valor::Inteiros),
            };
            if let Some(valor) = valor {
                out.insert(nome.clone(), valor);
            }
        }
        out
    }
}

/// Texto terminado em NUL; bytes fora de ASCII imprimível viram '?'.
/// Float lido → inteiro. NaN/infinito ou fora de ±2^53 = leitura inválida.
fn f32_para_num(x: f32) -> Option<i64> {
    let x = f64::from(x).round();
    // Faixa conferida antes: a conversão é exata.
    #[allow(clippy::cast_possible_truncation)] // ver acima
    (x.is_finite() && x.abs() < 9.0e15).then_some(x as i64)
}

/// Item de uma lista: `c` começa no item (o último pedaço pode ser só o item).
fn inteiro_le(tipo: Tipo, c: &[u8]) -> i64 {
    match (tipo, c) {
        (Tipo::U8s, [b, ..]) => i64::from(*b),
        (Tipo::U32s, [a, b, c, d, ..]) => i64::from(u32::from_le_bytes([*a, *b, *c, *d])),
        (Tipo::I32s, [a, b, c, d, ..]) => i64::from(i32::from_le_bytes([*a, *b, *c, *d])),
        // `ler_lista` garante pedaços com a largura do tipo.
        _ => 0,
    }
}

fn vec3_le(b: &[u8; 12]) -> [f32; 3] {
    std::array::from_fn(|i| {
        f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
    })
}

fn texto_c(b: &[u8]) -> String {
    b.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| {
            if (0x20..0x7f).contains(&c) {
                c as char
            } else {
                '?'
            }
        })
        .collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn float_vira_inteiro() {
        assert_eq!(f32_para_num(127_244.4), Some(127_244));
        assert_eq!(f32_para_num(-0.6), Some(-1));
        assert_eq!(f32_para_num(f32::NAN), None);
        assert_eq!(f32_para_num(f32::MAX), None);
    }

    #[test]
    fn texto_corta_no_nul() {
        assert_eq!(texto_c(b"UNIX\0lixo"), "UNIX");
        assert_eq!(texto_c(b"\0UNIX"), "");
        assert_eq!(texto_c(b"A\xffB"), "A?B");
    }

    #[test]
    fn le_lista_em_grupos() {
        // Duas "etapas" de 16 bytes; em cada uma, 3 itens u32 a cada 4 bytes
        // a partir do byte 4, e um u8 no byte 3.
        let mut dados = [0u8; 32];
        for (g, base) in [0usize, 16].into_iter().enumerate() {
            for i in 0..3 {
                let n = u32::try_from(100 * g + i).unwrap();
                dados[base + 4 + 4 * i..base + 8 + 4 * i].copy_from_slice(&n.to_le_bytes());
            }
            dados[base + 3] = 7 + u8::try_from(g).unwrap();
        }
        dados[4..8].copy_from_slice(&0xDD60_E402u32.to_le_bytes());
        let p = Processo {
            pid: std::process::id(),
            mem: File::open("/proc/self/mem").unwrap(),
            modulos: HashMap::new(),
            bits: 64,
        };
        let base = std::ptr::addr_of!(dados) as u64;
        let var = |tipo: Tipo, quantos: usize, passo: u64| crate::config::Var {
            tipo,
            tamanho: 0,
            quantos,
            passo,
            grupos: 2,
            passo_grupo: 16,
            endereco: String::new(),
            ponteiros: vec![],
            registrar: false,
        };
        let v = p.ler_lista(base + 4, &var(Tipo::U32s, 3, 4)).unwrap();
        assert_eq!(v, vec![0xDD60_E402, 1, 2, 100, 101, 102]);
        let v = p.ler_lista(base + 3, &var(Tipo::U8s, 1, 1)).unwrap();
        assert_eq!(v, vec![7, 8]);
        let v = p.ler_lista(base + 4, &var(Tipo::I32s, 1, 4)).unwrap();
        assert_eq!(
            v,
            vec![
                i64::from(i32::from_le_bytes(0xDD60_E402u32.to_le_bytes())),
                100
            ]
        );
    }

    #[test]
    fn le_a_propria_memoria() {
        let segredo: u32 = 0xC0FF_EE42;
        let p = Processo {
            pid: std::process::id(),
            mem: File::open("/proc/self/mem").unwrap(),
            modulos: HashMap::new(),
            bits: 64,
        };
        let mut b = [0u8; 4];
        p.ler(std::ptr::addr_of!(segredo) as u64, &mut b).unwrap();
        assert_eq!(u32::from_le_bytes(b), segredo);
    }
}
