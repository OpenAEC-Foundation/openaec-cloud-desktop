//! Controlo de versões por ficheiro, com recuperação.
//!
//! Antes de cada gravação, a versão anterior é copiada para
//! `<projecto>/.openaec/versoes/<ficheiro>/<momento>_<utilizador>`. As versões
//! são baratas (cópia de ficheiros na pasta partilhada) e qualquer estação as
//! consegue consultar sem servidor.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{agora_ms, Erro, Resultado};

/// Registo de uma versão guardada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versao {
    pub ficheiro: String,
    pub utilizador: String,
    pub maquina: String,
    #[serde(default = "agora_ms")]
    pub momento: u64,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub comentario: String,
}

/// Versões de um projecto.
pub struct Versoes {
    pasta: PathBuf,
}

impl Versoes {
    /// Versões guardadas em `<projecto>/.openaec/versoes`.
    pub fn do_projecto(projecto: &Path) -> Self {
        Self {
            pasta: projecto.join(".openaec").join("versoes"),
        }
    }

    /// Guarda a versão actual do ficheiro antes de uma nova gravação.
    /// Devolve `None` se o ficheiro ainda não existia.
    pub fn guardar_antes_de_escrever(
        &self,
        projecto: &Path,
        ficheiro: &str,
        utilizador: &str,
        maquina: &str,
        comentario: &str,
    ) -> Resultado<Option<Versao>> {
        let origem = projecto.join(ficheiro);
        if !origem.is_file() {
            return Ok(None);
        }
        let momento = agora_ms();
        let destino = self
            .pasta
            .join(sanear_caminho(ficheiro))
            .join(format!("{momento}_{}", utilizador.replace(['/', '\\'], "_")));
        if let Some(pai) = destino.parent() {
            fs::create_dir_all(pai)?;
        }
        fs::copy(&origem, &destino)?;
        let bytes = fs::metadata(&destino)?.len();
        let versao = Versao {
            ficheiro: ficheiro.into(),
            utilizador: utilizador.into(),
            maquina: maquina.into(),
            momento,
            bytes,
            comentario: comentario.into(),
        };
        self.registar(&versao)?;
        Ok(Some(versao))
    }

    fn registar(&self, versao: &Versao) -> Resultado<()> {
        let f = self.pasta.join("versoes.jsonl");
        use std::io::Write;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(f)?;
        writeln!(f, "{}", serde_json::to_string(versao)?)?;
        Ok(())
    }

    /// Histórico de um ficheiro, do mais recente para o mais antigo.
    pub fn historico(&self, ficheiro: &str) -> Resultado<Vec<Versao>> {
        let f = self.pasta.join("versoes.jsonl");
        let Ok(texto) = fs::read_to_string(&f) else {
            return Ok(Vec::new());
        };
        let mut lista: Vec<Versao> = texto
            .lines()
            .filter_map(|l| serde_json::from_str::<Versao>(l).ok())
            .filter(|v: &Versao| v.ficheiro == ficheiro)
            .collect();
        lista.sort_by(|a, b| b.momento.cmp(&a.momento));
        Ok(lista)
    }

    /// Caminho do conteúdo de uma versão (para abrir ou copiar de volta).
    pub fn caminho_da_versao(&self, versao: &Versao) -> PathBuf {
        self.pasta.join(sanear_caminho(&versao.ficheiro)).join(format!(
            "{}_{}",
            versao.momento,
            versao.utilizador.replace(['/', '\\'], "_")
        ))
    }

    /// Recupera uma versão anterior: guarda a actual e repõe a escolhida.
    pub fn recuperar(&self, projecto: &Path, versao: &Versao, por: &str) -> Resultado<()> {
        let origem = self.caminho_da_versao(versao);
        if !origem.is_file() {
            return Err(Erro::Caminho(format!(
                "conteúdo da versão não encontrado: {}",
                origem.display()
            )));
        }
        self.guardar_antes_de_escrever(projecto, &versao.ficheiro, por, "", "antes de recuperar")?;
        fs::copy(&origem, projecto.join(&versao.ficheiro))?;
        Ok(())
    }
}

fn sanear_caminho(ficheiro: &str) -> String {
    ficheiro.replace(['/', '\\'], "_")
}
