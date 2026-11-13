//! Base de dados SQLite do ambiente, guardada na própria pasta raiz
//! (`.openaec/openaec.db`). Serve de índice rápido: ficheiros, versões,
//! trancas e histórico. Sem servidor: cada estação abre o ficheiro em
//! modo WAL, que suporta leitores simultâneos com um escritor.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::Resultado;

/// Uma base SQLite aberta sobre a pasta partilhada.
pub struct Base {
    caminho: PathBuf,
    ligacao: Connection,
}

impl Base {
    /// Abre (ou cria) a base na raiz e garante o esquema.
    pub fn abrir(raiz: &Path) -> Resultado<Self> {
        let pasta = raiz.join(".openaec");
        std::fs::create_dir_all(&pasta)?;
        let caminho = pasta.join("openaec.db");
        let ligacao = Connection::open(&caminho)?;
        ligacao.pragma_update(None, "journal_mode", "WAL")?;
        ligacao.pragma_update(None, "busy_timeout", 5000)?;
        ligacao.execute_batch(
            "CREATE TABLE IF NOT EXISTS ficheiros (
                caminho TEXT PRIMARY KEY,
                projecto TEXT NOT NULL,
                hash TEXT NOT NULL,
                bytes INTEGER NOT NULL,
                momento INTEGER NOT NULL,
                utilizador TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_ficheiros_projecto ON ficheiros (projecto);
            CREATE TABLE IF NOT EXISTS versoes (
                ficheiro TEXT NOT NULL,
                momento INTEGER NOT NULL,
                utilizador TEXT NOT NULL,
                maquina TEXT NOT NULL,
                bytes INTEGER NOT NULL,
                comentario TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (ficheiro, momento)
            );
            CREATE TABLE IF NOT EXISTS trancas (
                ficheiro TEXT PRIMARY KEY,
                utilizador TEXT NOT NULL,
                maquina TEXT NOT NULL,
                renovada_em INTEGER NOT NULL
            );",
        )?;
        Ok(Self { caminho, ligacao })
    }

    pub fn caminho(&self) -> &Path {
        &self.caminho
    }

    /// Regista (ou actualiza) o estado conhecido de um ficheiro.
    pub fn registar_ficheiro(
        &self,
        caminho: &str,
        projecto: &str,
        hash: &str,
        bytes: u64,
        momento: u64,
        utilizador: &str,
    ) -> Resultado<()> {
        self.ligacao.execute(
            "INSERT INTO ficheiros (caminho, projecto, hash, bytes, momento, utilizador)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(caminho) DO UPDATE SET
                projecto = excluded.projecto,
                hash = excluded.hash,
                bytes = excluded.bytes,
                momento = excluded.momento,
                utilizador = excluded.utilizador",
            rusqlite::params![caminho, projecto, hash, bytes, momento, utilizador],
        )?;
        Ok(())
    }

    /// O estado conhecido de um ficheiro: (hash, bytes, momento, utilizador).
    pub fn estado_ficheiro(&self, caminho: &str) -> Resultado<Option<(String, u64, u64, String)>> {
        let mut stmt = self.ligacao.prepare(
            "SELECT hash, bytes, momento, utilizador FROM ficheiros WHERE caminho = ?1",
        )?;
        let mut linhas = stmt.query([caminho])?;
        Ok(match linhas.next()? {
            Some(l) => Some((
                l.get(0)?,
                l.get::<_, i64>(1)? as u64,
                l.get::<_, i64>(2)? as u64,
                l.get(3)?,
            )),
            None => None,
        })
    }

    /// Remove o registo de um ficheiro (foi apagado).
    pub fn esquecer_ficheiro(&self, caminho: &str) -> Resultado<()> {
        self.ligacao
            .execute("DELETE FROM ficheiros WHERE caminho = ?1", [caminho])?;
        Ok(())
    }

    /// Regista uma tranca na base (espelho da pasta `trancas`).
    pub fn registar_tranca(
        &self,
        ficheiro: &str,
        utilizador: &str,
        maquina: &str,
        renovada_em: u64,
    ) -> Resultado<()> {
        self.ligacao.execute(
            "INSERT INTO trancas (ficheiro, utilizador, maquina, renovada_em)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(ficheiro) DO UPDATE SET
                utilizador = excluded.utilizador,
                maquina = excluded.maquina,
                renovada_em = excluded.renovada_em",
            rusqlite::params![ficheiro, utilizador, maquina, renovada_em],
        )?;
        Ok(())
    }

    /// Trancas activas na base.
    pub fn trancas(&self) -> Resultado<Vec<(String, String, String, u64)>> {
        let mut stmt = self
            .ligacao
            .prepare("SELECT ficheiro, utilizador, maquina, renovada_em FROM trancas")?;
        let linhas = stmt.query_map([], |l| {
            Ok((
                l.get::<_, String>(0)?,
                l.get::<_, String>(1)?,
                l.get::<_, String>(2)?,
                l.get::<_, i64>(3)? as u64,
            ))
        })?;
        Ok(linhas.collect::<std::result::Result<_, _>>()?)
    }

    /// Compacta a base (VACUUM). Chamar em manutenção.
    pub fn compactar(&self) -> Resultado<()> {
        self.ligacao.execute("VACUUM", [])?;
        Ok(())
    }
}

impl std::fmt::Debug for Base {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Base({})", self.caminho.display())
    }
}

impl Drop for Base {
    fn drop(&mut self) {
        let _ = self.ligacao.pragma_update(None, "wal_checkpoint", "PASSIVE");
    }
}

