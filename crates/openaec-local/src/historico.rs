//! Histórico de alterações do ambiente (`Logs/historico.jsonl`).
//!
//! Um registo por linha, apenas para acrescentar — seguro com várias estações
//! a escrever ao mesmo tempo na pasta partilhada.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::estrutura::Raiz;
use crate::{agora_ms, Resultado};

/// Um acontecimento no ambiente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registo {
    #[serde(default = "agora_ms")]
    pub momento: u64,
    pub utilizador: String,
    pub maquina: String,
    pub accao: String,
    pub projecto: String,
    #[serde(default)]
    pub ficheiro: String,
    #[serde(default)]
    pub detalhe: String,
}

/// Histórico do ambiente, guardado em `Logs/historico.jsonl`.
pub struct Historico {
    ficheiro: PathBuf,
}

impl Historico {
    pub fn da_raiz(raiz: &Raiz) -> Self {
        Self {
            ficheiro: raiz.logs().join("historico.jsonl"),
        }
    }

    /// Acrescenta um registo ao histórico.
    pub fn registar(
        &self,
        utilizador: &str,
        maquina: &str,
        accao: &str,
        projecto: &str,
        ficheiro: &str,
        detalhe: &str,
    ) -> Resultado<()> {
        let registo = Registo {
            momento: agora_ms(),
            utilizador: utilizador.into(),
            maquina: maquina.into(),
            accao: accao.into(),
            projecto: projecto.into(),
            ficheiro: ficheiro.into(),
            detalhe: detalhe.into(),
        };
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.ficheiro)?;
        writeln!(f, "{}", serde_json::to_string(&registo)?)?;
        Ok(())
    }

    /// Lê o histórico, do mais recente para o mais antigo.
    /// Com `filtro` mostra apenas o projecto dado.
    pub fn ler(&self, filtro: Option<&str>, limite: usize) -> Resultado<Vec<Registo>> {
        let Ok(texto) = fs::read_to_string(&self.ficheiro) else {
            return Ok(Vec::new());
        };
        let mut lista: Vec<Registo> = texto
            .lines()
            .filter_map(|l| serde_json::from_str::<Registo>(l).ok())
            .filter(|r: &Registo| filtro.map_or(true, |p| r.projecto == p))
            .collect();
        lista.reverse();
        lista.truncate(limite);
        Ok(lista)
    }
}
