//! Estrutura da pasta raiz partilhada e dos projectos.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{agora_ms, Erro, Resultado};

/// Pastas fixas criadas na raiz.
pub const RAIZ_PASTAS: [&str; 6] = [
    "Projetos",
    "Utilizadores",
    "Configuracoes",
    "Biblioteca",
    "Backups",
    "Logs",
];

/// Subpastas de cada projecto.
pub const PROJECTO_PASTAS: [&str; 6] = [
    "Documentos",
    "Modelos",
    "IFC",
    "DWG",
    "PDF",
    "Quantidades",
];

/// Papel de um utilizador dentro do ambiente local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Papel {
    Administrador,
    #[serde(rename = "gestor")]
    GestorDeProjecto,
    Tecnico,
    Consulta,
}

impl Papel {
    /// Pode criar/apagar projectos e gerir utilizadores.
    pub fn administra(&self) -> bool {
        matches!(self, Papel::Administrador)
    }
    /// Pode criar e editar projectos.
    pub fn gere(&self) -> bool {
        matches!(self, Papel::Administrador | Papel::GestorDeProjecto)
    }
    /// Pode editar ficheiros dentro de projectos.
    pub fn edita(&self) -> bool {
        !matches!(self, Papel::Consulta)
    }
}

/// Metadados de um projecto (`Projetos/<nome>/projeto.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projecto {
    pub nome: String,
    pub descricao: String,
    pub responsavel: String,
    #[serde(default)]
    pub etiquetas: Vec<String>,
    #[serde(default = "padrao_estado")]
    pub estado: String,
    #[serde(default = "agora_ms")]
    pub criado_em: u64,
    #[serde(default = "agora_ms")]
    pub alterado_em: u64,
}

fn padrao_estado() -> String {
    "activo".into()
}

/// A pasta raiz partilhada do ambiente local.
#[derive(Debug, Clone)]
pub struct Raiz {
    caminho: PathBuf,
}

impl Raiz {
    /// Abre uma raiz existente (valida a estrutura) sem escrever nada.
    pub fn abrir(caminho: impl AsRef<Path>) -> Resultado<Self> {
        let caminho = caminho.as_ref().to_path_buf();
        if !caminho.is_dir() {
            return Err(Erro::Caminho(format!("{} não é uma pasta", caminho.display())));
        }
        for pasta in RAIZ_PASTAS {
            if !caminho.join(pasta).is_dir() {
                return Err(Erro::Caminho(format!(
                    "falta a pasta {} em {} (execute `openaec-local init`)",
                    pasta,
                    caminho.display()
                )));
            }
        }
        Ok(Self { caminho })
    }

    /// Cria a estrutura completa na pasta dada. Idempotente.
    pub fn iniciar(caminho: impl AsRef<Path>) -> Resultado<Self> {
        let caminho = caminho.as_ref().to_path_buf();
        fs::create_dir_all(&caminho)?;
        for pasta in RAIZ_PASTAS {
            fs::create_dir_all(caminho.join(pasta))?;
        }
        fs::create_dir_all(caminho.join(".openaec"))?;
        let raiz = Self { caminho };
        raiz.escrever_configuracao_padrao()?;
        Ok(raiz)
    }

    pub fn caminho(&self) -> &Path {
        &self.caminho
    }

    pub fn projetos(&self) -> PathBuf {
        self.caminho.join("Projetos")
    }

    pub fn backups(&self) -> PathBuf {
        self.caminho.join("Backups")
    }

    pub fn logs(&self) -> PathBuf {
        self.caminho.join("Logs")
    }

    pub fn oculta(&self) -> PathBuf {
        self.caminho.join(".openaec")
    }

    /// Configuração do ambiente (`Configuracoes/configuracao.json`).
    pub fn configuracao(&self) -> Resultado<serde_json::Value> {
        let f = self.caminho.join("Configuracoes").join("configuracao.json");
        Ok(serde_json::from_str(&fs::read_to_string(f)?)?)
    }

    fn escrever_configuracao_padrao(&self) -> Resultado<()> {
        let f = self.caminho.join("Configuracoes").join("configuracao.json");
        if f.exists() {
            return Ok(());
        }
        let configuracao = serde_json::json!({
            "ambiente": "OpenAEC Local Workspace",
            "versao": env!("CARGO_PKG_VERSION"),
            "sincronizacao": { "intervalo_segundos": 30 },
            "cache_local": true,
            "autenticacao": { "tipo": "local", "active_directory": null }
        });
        fs::write(f, serde_json::to_string_pretty(&configuracao)?)?;
        Ok(())
    }

    /// Cria um projecto autónomo com as suas subpastas e metadados.
    pub fn criar_projecto(&self, projecto: &Projecto) -> Resultado<PathBuf> {
        if projecto.nome.trim().is_empty() {
            return Err(Erro::Outro("o nome do projecto não pode estar vazio".into()));
        }
        let pasta = self.projetos().join(sanear_nome(&projecto.nome));
        for sub in PROJECTO_PASTAS {
            fs::create_dir_all(pasta.join(sub))?;
        }
        fs::create_dir_all(pasta.join(".openaec").join("versoes"))?;
        fs::create_dir_all(pasta.join(".openaec").join("trancas"))?;
        let f = pasta.join("projeto.json");
        if !f.exists() {
            fs::write(f, serde_json::to_string_pretty(projecto)?)?;
        }
        Ok(pasta)
    }

    /// Lista os projectos existentes, com metadados quando disponíveis.
    pub fn listar_projectos(&self) -> Resultado<Vec<Projecto>> {
        let mut lista = Vec::new();
        for entrada in fs::read_dir(self.projetos())? {
            let pasta = entrada?.path();
            if !pasta.is_dir() {
                continue;
            }
            let f = pasta.join("projeto.json");
            if let Ok(texto) = fs::read_to_string(&f) {
                if let Ok(mut p) = serde_json::from_str::<Projecto>(&texto) {
                    if p.nome.is_empty() {
                        p.nome = pasta
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                    }
                    lista.push(p);
                    continue;
                }
            }
            let nome = pasta
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            lista.push(Projecto {
                nome,
                descricao: String::new(),
                responsavel: String::new(),
                etiquetas: Vec::new(),
                estado: padrao_estado(),
                criado_em: agora_ms(),
                alterado_em: agora_ms(),
            });
        }
        lista.sort_by(|a, b| a.nome.to_lowercase().cmp(&b.nome.to_lowercase()));
        Ok(lista)
    }
}

/// Remove caracteres problemáticos para nomes de pasta em Windows e Linux.
pub fn sanear_nome(nome: &str) -> String {
    let proibidos = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    nome.trim()
        .chars()
        .map(|c| if proibidos.contains(&c) { '_' } else { c })
        .collect::<String>()
        .trim_matches('.')
        .to_string()
}
