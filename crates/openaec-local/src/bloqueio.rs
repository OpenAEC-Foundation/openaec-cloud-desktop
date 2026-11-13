//! Bloqueio de ficheiros em edição (trancas cooperativas por ficheiro).
//!
//! Cada tranca é um ficheiro `trancas/<hash>.lock` dentro do `.openaec` do
//! projecto, com o utilizador, a máquina e o momento. Como tudo vive na pasta
//! partilhada, todas as estações vêem as mesmas trancas; não é preciso
//! nenhum servidor.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{agora_ms, Erro, Resultado};

/// Uma tranca sobre um ficheiro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tranca {
    pub ficheiro: String,
    pub utilizador: String,
    pub maquina: String,
    #[serde(default = "agora_ms")]
    pub criada_em: u64,
    #[serde(default = "renovada_padrao")]
    pub renovada_em: u64,
}

fn renovada_padrao() -> u64 {
    agora_ms()
}

/// Uma tranca é considerada abandonada após este tempo sem renovação.
pub const EXPIRACAO_MS: u64 = 10 * 60 * 1000;

/// Gera bytes aleatórios em hexadecimal (para sais e identificadores).
pub fn sal_aleatorio(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    // Sem dependências extra: mistura o tempo e o endereço de entropy do SO.
    let semente = agora_ms() ^ (bytes.as_ptr() as u64).rotate_left(17);
    let mut estado = semente | 1;
    for b in bytes.iter_mut() {
        estado = estado.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *b = (estado >> 33) as u8;
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn nome_tranca(ficheiro: &str) -> String {
    let h = blake3::hash(ficheiro.as_bytes());
    h.to_hex()[..16].to_string()
}

/// Trancas de um projecto.
pub struct Trancas {
    pasta: PathBuf,
}

impl Trancas {
    /// Trancas guardadas em `<projecto>/.openaec/trancas`.
    pub fn do_projecto(projecto: &Path) -> Self {
        Self {
            pasta: projecto.join(".openaec").join("trancas"),
        }
    }

    fn caminho(&self, ficheiro: &str) -> PathBuf {
        self.pasta.join(format!("{}.lock", nome_tranca(ficheiro)))
    }

    /// Tenta trancar um ficheiro para edição.
    ///
    /// Falha se outro utilizador (noutra máquina) tiver a tranca activa.
    /// Trancas abandonadas (sem renovação) são removidas automaticamente.
    pub fn trancar(&self, ficheiro: &str, utilizador: &str, maquina: &str) -> Resultado<Tranca> {
        if let Some(tranca) = self.actual(ficheiro)? {
            let mesma_pessoa = tranca.utilizador == utilizador && tranca.maquina == maquina;
            if !mesma_pessoa {
                return Err(Erro::Outro(format!(
                    "{} está a editar {} (desde {})",
                    tranca.utilizador,
                    ficheiro,
                    formatar_momento(tranca.criada_em)
                )));
            }
            return self.renovar(&tranca);
        }
        let tranca = Tranca {
            ficheiro: ficheiro.into(),
            utilizador: utilizador.into(),
            maquina: maquina.into(),
            criada_em: agora_ms(),
            renovada_em: agora_ms(),
        };
        fs::create_dir_all(&self.pasta)?;
        fs::write(self.caminho(ficheiro), serde_json::to_string(&tranca)?)?;
        Ok(tranca)
    }

    /// Liberta a tranca; ignora se já não existir.
    pub fn destrancar(&self, ficheiro: &str, utilizador: &str) -> Resultado<()> {
        let f = self.caminho(ficheiro);
        if let Ok(texto) = fs::read_to_string(&f) {
            if let Ok(tranca) = serde_json::from_str::<Tranca>(&texto) {
                if tranca.utilizador != utilizador {
                    return Err(Erro::Outro(format!(
                        "{} pertence a {}",
                        ficheiro, tranca.utilizador
                    )));
                }
            }
        }
        let _ = fs::remove_file(&f);
        Ok(())
    }

    /// Renova uma tranca própria (chamado periodicamente enquanto se edita).
    pub fn renovar(&self, tranca: &Tranca) -> Resultado<Tranca> {
        let mut nova = tranca.clone();
        nova.renovada_em = agora_ms();
        fs::write(self.caminho(&tranca.ficheiro), serde_json::to_string(&nova)?)?;
        Ok(nova)
    }

    /// A tranca activa de um ficheiro, se existir e não estiver expirada.
    pub fn actual(&self, ficheiro: &str) -> Resultado<Option<Tranca>> {
        let f = self.caminho(ficheiro);
        let Ok(texto) = fs::read_to_string(&f) else {
            return Ok(None);
        };
        let tranca: Tranca = serde_json::from_str(&texto)?;
        if agora_ms().saturating_sub(tranca.renovada_em) > EXPIRACAO_MS {
            let _ = fs::remove_file(&f);
            return Ok(None);
        }
        Ok(Some(tranca))
    }

    /// Todas as trancas activas do projecto.
    pub fn todas(&self) -> Resultado<Vec<Tranca>> {
        let mut lista = Vec::new();
        if !self.pasta.is_dir() {
            return Ok(lista);
        }
        for entrada in fs::read_dir(&self.pasta)? {
            let p = entrada?.path();
            if p.extension().and_then(|e| e.to_str()) != Some("lock") {
                continue;
            }
            if let Ok(texto) = fs::read_to_string(&p) {
                if let Ok(t) = serde_json::from_str::<Tranca>(&texto) {
                    if agora_ms().saturating_sub(t.renovada_em) <= EXPIRACAO_MS {
                        lista.push(t);
                    }
                }
            }
        }
        Ok(lista)
    }
}

fn formatar_momento(ms: u64) -> String {
    let segundos = ms / 1000;
    let dias = segundos / 86400;
    let resto = segundos % 86400;
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        1970 + dias / 365,
        (dias % 365) / 30 + 1,
        (dias % 365) % 30 + 1,
        resto / 3600,
        (resto % 3600) / 60
    )
}

/// Espera activa simples até a tranca ficar livre (para testes e CLI).
pub fn esperar_livre(trancas: &Trancas, ficheiro: &str, limite: Duration) -> Resultado<()> {
    let inicio = std::time::Instant::now();
    while inicio.elapsed() < limite {
        if trancas.actual(ficheiro)?.is_none() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(Erro::Outro("a tranca não ficou livre a tempo".into()))
}
