//! Banco SQLite: conquistas e marcos de progresso por (jogo, perfil).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

pub struct Banco {
    c: Connection,
}

pub fn caminho_padrao() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(std::env::var_os("HOME").context("HOME não definido")?)
            .join(".local/share"),
    };
    Ok(base.join("speedconquest/conquistas.db"))
}

fn agora_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn agora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Banco {
    pub fn abrir(caminho: &Path) -> Result<Self> {
        if let Some(d) = caminho.parent() {
            std::fs::create_dir_all(d).with_context(|| format!("criar {}", d.display()))?;
        }
        let c =
            Connection::open(caminho).with_context(|| format!("abrir {}", caminho.display()))?;
        // O vigia grava e a janela lê ao mesmo tempo: espera o outro em vez de falhar.
        c.busy_timeout(std::time::Duration::from_secs(2))?;
        Self::iniciar(c)
    }

    #[cfg(test)]
    pub fn em_memoria() -> Result<Self> {
        Self::iniciar(Connection::open_in_memory()?)
    }

    fn iniciar(c: Connection) -> Result<Self> {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS desbloqueio(
                 jogo TEXT NOT NULL, perfil TEXT NOT NULL, conquista TEXT NOT NULL,
                 quando INTEGER NOT NULL,
                 PRIMARY KEY (jogo, perfil, conquista));
             CREATE TABLE IF NOT EXISTS marco(
                 jogo TEXT NOT NULL, perfil TEXT NOT NULL, conquista TEXT NOT NULL,
                 pct INTEGER NOT NULL, quando INTEGER NOT NULL,
                 PRIMARY KEY (jogo, perfil, conquista, pct));
             CREATE TABLE IF NOT EXISTS estatistica(
                 jogo TEXT NOT NULL, perfil TEXT NOT NULL, nome TEXT NOT NULL,
                 valor INTEGER NOT NULL,
                 PRIMARY KEY (jogo, perfil, nome));
             CREATE TABLE IF NOT EXISTS historico(
                 jogo TEXT NOT NULL, perfil TEXT NOT NULL, var TEXT NOT NULL,
                 quando_ms INTEGER NOT NULL, antes INTEGER NOT NULL, depois INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS historico_perfil ON historico(jogo, perfil, var, quando_ms);
             CREATE TABLE IF NOT EXISTS maior(
                 jogo TEXT NOT NULL, perfil TEXT NOT NULL, var TEXT NOT NULL,
                 valor INTEGER NOT NULL, quando INTEGER NOT NULL,
                 PRIMARY KEY (jogo, perfil, var));",
        )?;
        Ok(Banco { c })
    }

    pub fn desbloqueada(&self, jogo: &str, perfil: &str, conquista: &str) -> Result<bool> {
        Ok(self
            .c
            .query_row(
                "SELECT 1 FROM desbloqueio WHERE jogo=?1 AND perfil=?2 AND conquista=?3",
                params![jogo, perfil, conquista],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Grava o desbloqueio. `false` se já estava gravado.
    pub fn desbloquear(&self, jogo: &str, perfil: &str, conquista: &str) -> Result<bool> {
        let n = self.c.execute(
            "INSERT OR IGNORE INTO desbloqueio(jogo, perfil, conquista, quando) VALUES (?1, ?2, ?3, ?4)",
            params![jogo, perfil, conquista, agora()],
        )?;
        Ok(n == 1)
    }

    /// Grava o marco de progresso. `false` se já estava gravado (ou se a conquista
    /// já foi desbloqueada: aí o marco não interessa).
    pub fn marcar(&self, jogo: &str, perfil: &str, conquista: &str, pct: u8) -> Result<bool> {
        if self.desbloqueada(jogo, perfil, conquista)? {
            return Ok(false);
        }
        let n = self.c.execute(
            "INSERT OR IGNORE INTO marco(jogo, perfil, conquista, pct, quando) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![jogo, perfil, conquista, pct, agora()],
        )?;
        Ok(n == 1)
    }

    /// Perfis com algo gravado para o jogo.
    pub fn perfis(&self, jogo: &str) -> Result<Vec<String>> {
        let mut st = self.c.prepare(
            "SELECT perfil FROM desbloqueio WHERE jogo=?1 UNION SELECT perfil FROM marco WHERE jogo=?1 UNION SELECT perfil FROM estatistica WHERE jogo=?1 UNION SELECT perfil FROM historico WHERE jogo=?1 UNION SELECT perfil FROM maior WHERE jogo=?1 ORDER BY 1",
        )?;
        let v = st
            .query_map(params![jogo], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(v)
    }

    pub fn estatisticas(&self, jogo: &str, perfil: &str) -> Result<HashMap<String, i64>> {
        let mut st = self
            .c
            .prepare("SELECT nome, valor FROM estatistica WHERE jogo=?1 AND perfil=?2")?;
        let v = st
            .query_map(params![jogo, perfil], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<HashMap<String, i64>>>()?;
        Ok(v)
    }

    pub fn gravar_estatisticas(
        &self,
        jogo: &str,
        perfil: &str,
        valores: &HashMap<String, i64>,
    ) -> Result<()> {
        for (nome, valor) in valores {
            self.c.execute(
                "INSERT INTO estatistica(jogo, perfil, nome, valor) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(jogo, perfil, nome) DO UPDATE SET valor=excluded.valor",
                params![jogo, perfil, nome, valor],
            )?;
        }
        Ok(())
    }

    pub fn registrar(
        &self,
        jogo: &str,
        perfil: &str,
        var: &str,
        antes: i64,
        depois: i64,
    ) -> Result<()> {
        self.c.execute(
            "INSERT INTO historico(jogo, perfil, var, quando_ms, antes, depois) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![jogo, perfil, var, agora_ms(), antes, depois],
        )?;
        Ok(())
    }

    /// Histórico de uma variável: (quando_ms, antes, depois), do mais antigo ao mais novo.
    pub fn historico(&self, jogo: &str, perfil: &str, var: &str) -> Result<Vec<(i64, i64, i64)>> {
        let mut st = self.c.prepare(
            "SELECT quando_ms, antes, depois FROM historico WHERE jogo=?1 AND perfil=?2 AND var=?3 ORDER BY quando_ms",
        )?;
        let v = st
            .query_map(params![jogo, perfil, var], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Maior valor já visto de cada variável de progresso (para a janela).
    pub fn maiores(&self, jogo: &str, perfil: &str) -> Result<HashMap<String, i64>> {
        let mut st = self
            .c
            .prepare("SELECT var, valor FROM maior WHERE jogo=?1 AND perfil=?2")?;
        let v = st
            .query_map(params![jogo, perfil], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<HashMap<String, i64>>>()?;
        Ok(v)
    }

    /// Grava os valores que forem maiores que os já gravados.
    pub fn gravar_maiores(
        &self,
        jogo: &str,
        perfil: &str,
        valores: &HashMap<String, i64>,
    ) -> Result<()> {
        for (var, valor) in valores {
            self.c.execute(
                "INSERT INTO maior(jogo, perfil, var, valor, quando) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(jogo, perfil, var) DO UPDATE SET
                     quando = CASE WHEN excluded.valor > valor THEN excluded.quando ELSE quando END,
                     valor = MAX(valor, excluded.valor)",
                params![jogo, perfil, var, valor, agora()],
            )?;
        }
        Ok(())
    }

    /// Perfil que avançou em algo por último (o que a janela abre primeiro).
    pub fn perfil_recente(&self, jogo: &str) -> Result<Option<String>> {
        Ok(self
            .c
            .query_row(
                "SELECT perfil FROM (
                     SELECT perfil, quando FROM desbloqueio WHERE jogo=?1
                     UNION ALL SELECT perfil, quando FROM marco WHERE jogo=?1
                     UNION ALL SELECT perfil, quando FROM maior WHERE jogo=?1)
                 ORDER BY quando DESC LIMIT 1",
                params![jogo],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Conquistas desbloqueadas do perfil → data e hora locais ("21/11/2026 20:00").
    pub fn desbloqueios(&self, jogo: &str, perfil: &str) -> Result<HashMap<String, String>> {
        let mut st = self.c.prepare(
            "SELECT conquista, strftime('%d/%m/%Y %H:%M', quando, 'unixepoch', 'localtime')
             FROM desbloqueio WHERE jogo=?1 AND perfil=?2",
        )?;
        let v = st
            .query_map(params![jogo, perfil], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<HashMap<String, String>>>()?;
        Ok(v)
    }

    /// Maior marco de cada conquista do perfil.
    pub fn marcos(&self, jogo: &str, perfil: &str) -> Result<HashMap<String, u8>> {
        let mut st = self.c.prepare(
            "SELECT conquista, MAX(pct) FROM marco WHERE jogo=?1 AND perfil=?2 GROUP BY conquista",
        )?;
        let v = st
            .query_map(params![jogo, perfil], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<HashMap<String, u8>>>()?;
        Ok(v)
    }

    /// Maior marco gravado (0 se nenhum).
    pub fn maior_marco(&self, jogo: &str, perfil: &str, conquista: &str) -> Result<u8> {
        let v: Option<u8> = self.c.query_row(
            "SELECT MAX(pct) FROM marco WHERE jogo=?1 AND perfil=?2 AND conquista=?3",
            params![jogo, perfil, conquista],
            |r| r.get(0),
        )?;
        Ok(v.unwrap_or(0))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn por_perfil() {
        let b = Banco::em_memoria().unwrap();
        assert!(b.desbloquear("nfsu2", "X", "a").unwrap());
        assert!(!b.desbloquear("nfsu2", "X", "a").unwrap());
        assert!(b.desbloqueada("nfsu2", "X", "a").unwrap());
        assert!(!b.desbloqueada("nfsu2", "Y", "a").unwrap());
        assert!(b.desbloquear("nfsu2", "Y", "a").unwrap());
    }

    #[test]
    fn marcos() {
        let b = Banco::em_memoria().unwrap();
        assert!(b.marcar("j", "X", "a", 25).unwrap());
        assert!(!b.marcar("j", "X", "a", 25).unwrap());
        assert!(b.marcar("j", "X", "a", 50).unwrap());
        assert_eq!(b.maior_marco("j", "X", "a").unwrap(), 50);
        b.desbloquear("j", "X", "a").unwrap();
        assert!(!b.marcar("j", "X", "a", 75).unwrap());
        assert_eq!(b.perfis("j").unwrap(), vec!["X".to_string()]);
    }

    #[test]
    fn estatisticas_e_historico() {
        let b = Banco::em_memoria().unwrap();
        b.gravar_estatisticas("j", "X", &HashMap::from([("ganhos".into(), 10)]))
            .unwrap();
        b.gravar_estatisticas("j", "X", &HashMap::from([("ganhos".into(), 25)]))
            .unwrap();
        assert_eq!(b.estatisticas("j", "X").unwrap()["ganhos"], 25);
        assert!(b.estatisticas("j", "Y").unwrap().is_empty());
        b.registrar("j", "Y", "dinheiro", 0, 500).unwrap();
        let h = b.historico("j", "Y", "dinheiro").unwrap();
        assert_eq!((h[0].1, h[0].2), (0, 500));
        assert_eq!(
            b.perfis("j").unwrap(),
            vec!["X".to_string(), "Y".to_string()]
        );
    }

    #[test]
    fn maiores_so_sobem() {
        let b = Banco::em_memoria().unwrap();
        b.gravar_maiores("j", "Z", &HashMap::from([("vel".into(), 210)]))
            .unwrap();
        b.gravar_maiores("j", "Z", &HashMap::from([("vel".into(), 150)]))
            .unwrap();
        assert_eq!(b.maiores("j", "Z").unwrap()["vel"], 210);
        b.gravar_maiores("j", "Z", &HashMap::from([("vel".into(), 260)]))
            .unwrap();
        assert_eq!(b.maiores("j", "Z").unwrap()["vel"], 260);
        assert_eq!(b.perfis("j").unwrap(), vec!["Z".to_string()]);
        assert_eq!(b.perfil_recente("j").unwrap().as_deref(), Some("Z"));
        assert_eq!(b.perfil_recente("outro").unwrap(), None);
    }

    #[test]
    fn desbloqueios_e_marcos_do_perfil() {
        let b = Banco::em_memoria().unwrap();
        b.desbloquear("j", "X", "a").unwrap();
        b.marcar("j", "X", "b", 25).unwrap();
        b.marcar("j", "X", "b", 50).unwrap();
        let d = b.desbloqueios("j", "X").unwrap();
        assert_eq!(d.len(), 1);
        // dd/mm/aaaa hh:mm
        assert_eq!(d["a"].len(), 16);
        assert_eq!(b.marcos("j", "X").unwrap()["b"], 50);
        assert!(b.desbloqueios("j", "Y").unwrap().is_empty());
    }
}
