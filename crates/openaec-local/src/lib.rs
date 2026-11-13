//! OpenAEC Local Workspace — núcleo local de colaboração BIM/CAD.
//!
//! Substitui a camada cloud (`openaec-sync` → `openaec_storage.api.*`) por uma
//! pasta raiz partilhada em rede (SMB/NFS). Tudo o que a aplicação precisa —
//! projectos, utilizadores, trancas, versões e histórico — vive em ficheiros
//! dentro dessa pasta; a base SQLite (`openaec.db`) também é guardada lá.
//!
//! Estrutura da raiz:
//!
//! ```text
//! /OpenAEC
//! ├── Projetos/        uma pasta por projecto
//! ├── Utilizadores/    utilizadores.json (local ou Active Directory)
//! ├── Configuracoes/   configuracao.json
//! ├── Biblioteca/      componentes partilhados
//! ├── Backups/         cópias de segurança
//! ├── Logs/            historico.jsonl
//! └── .openaec/         openaec.db (SQLite) + estado por estação
//! ```

pub mod base;
pub mod bloqueio;
pub mod estrutura;
pub mod historico;
pub mod motor;
pub mod utilizadores;
pub mod versoes;

pub use estrutura::{Papel, Projecto, Raiz, RAIZ_PASTAS};
pub use utilizadores::{OrigemUtilizador, Utilizador};

/// Erro do núcleo local.
#[derive(Debug, thiserror::Error)]
pub enum Erro {
    #[error("caminho inválido: {0}")]
    Caminho(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Outro(String),
}

pub type Resultado<T> = std::result::Result<T, Erro>;

/// Momento actual em milissegundos desde a época UNIX.
pub fn agora_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
