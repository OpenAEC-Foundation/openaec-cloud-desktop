//! Utilizadores e permissões, sem servidor de autenticação.
//!
//! Os utilizadores vivem em `Utilizadores/utilizadores.json` na pasta raiz.
//! Cada estação confirma a palavra-passe localmente (hash BLAKE3 do sal +
//! palavra-passe); com Active Directory opcional, o ficheiro apenas lista os
//! utilizadores e a verificação é delegada ao domínio.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::estrutura::{Papel, Raiz};
use crate::{agora_ms, Erro, Resultado};

/// De onde vem a autenticação do utilizador.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrigemUtilizador {
    /// Ficheiro local de utilizadores.
    #[default]
    Local,
    /// Active Directory (verificação delegada ao domínio).
    ActiveDirectory,
}

/// Um utilizador do ambiente local.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utilizador {
    pub nome: String,
    pub papel: Papel,
    #[serde(default = "agora_ms")]
    pub criado_em: u64,
    #[serde(default)]
    pub origem: OrigemUtilizador,
    /// Salt + hash BLAKE3 em base64 (`sal:hash`); vazio com Active Directory.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub segredo: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub maquina: String,
}

impl Utilizador {
    /// Cria um utilizador local com palavra-passe guardada como hash.
    pub fn novo(nome: &str, papel: Papel, palavra_passe: &str) -> Resultado<Self> {
        if nome.trim().is_empty() {
            return Err(Erro::Outro("o nome de utilizador não pode estar vazio".into()));
        }
        let sal = crate::bloqueio::sal_aleatorio(16);
        let hash = hash_palavra_passe(&sal, palavra_passe);
        Ok(Self {
            nome: nome.trim().into(),
            papel,
            criado_em: agora_ms(),
            origem: OrigemUtilizador::Local,
            segredo: format!("{sal}:{hash}"),
            maquina: String::new(),
        })
    }

    /// Confere a palavra-passe (apenas origem local).
    pub fn conferir_palavra_passe(&self, palavra_passe: &str) -> bool {
        match self.origem {
            OrigemUtilizador::Local => match self.segredo.split_once(':') {
                Some((sal, hash)) => hash_palavra_passe(sal, palavra_passe) == hash,
                None => false,
            },
            OrigemUtilizador::ActiveDirectory => false,
        }
    }
}

fn hash_palavra_passe(sal: &str, palavra_passe: &str) -> String {
    blake3::hash(format!("{sal}:{palavra_passe}").as_bytes()).to_hex().to_string()
}

/// Lista de utilizadores do ambiente (`Utilizadores/utilizadores.json`).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ListaUtilizadores {
    #[serde(default)]
    pub utilizadores: Vec<Utilizador>,
}

impl ListaUtilizadores {
    pub fn carregar(raiz: &Raiz) -> Resultado<Self> {
        let f = raiz
            .caminho()
            .join("Utilizadores")
            .join("utilizadores.json");
        if !f.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(&fs::read_to_string(f)?)?)
    }

    pub fn guardar(&self, raiz: &Raiz) -> Resultado<()> {
        let f = raiz
            .caminho()
            .join("Utilizadores")
            .join("utilizadores.json");
        fs::write(f, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn procurar(&self, nome: &str) -> Option<&Utilizador> {
        self.utilizadores
            .iter()
            .find(|u| u.nome.eq_ignore_ascii_case(nome))
    }

    /// Autentica por nome + palavra-passe (origem local).
    pub fn autenticar(&self, nome: &str, palavra_passe: &str) -> Option<&Utilizador> {
        self.procurar(nome)
            .filter(|u| u.conferir_palavra_passe(palavra_passe))
    }

    /// Adiciona ou substitui um utilizador e grava o ficheiro.
    pub fn gravar_utilizador(&mut self, raiz: &Raiz, utilizador: Utilizador) -> Resultado<()> {
        match self
            .utilizadores
            .iter_mut()
            .find(|u| u.nome.eq_ignore_ascii_case(&utilizador.nome))
        {
            Some(existing) => *existing = utilizador,
            None => self.utilizadores.push(utilizador),
        }
        self.guardar(raiz)
    }
}

/// Garante que existe pelo menos um administrador; cria o administrador
/// inicial `admin` com a palavra-passe dada se a lista estiver vazia.
pub fn iniciar_administrador(raiz: &Raiz, palavra_passe: &str) -> Resultado<Utilizador> {
    let mut lista = ListaUtilizadores::carregar(raiz)?;
    if lista.procurar("admin").is_some() {
        return Err(Erro::Outro("o administrador inicial já existe".into()));
    }
    let admin = Utilizador::novo("admin", Papel::Administrador, palavra_passe)?;
    lista.gravar_utilizador(raiz, admin.clone())?;
    Ok(admin)
}

/// Registo de actividade por estação (quem está online).
#[derive(Debug)]
pub struct Presencas {
    pasta: PathBuf,
}

impl Presencas {
    pub fn nova(raiz: &Raiz) -> Self {
        Self {
            pasta: raiz.caminho().join("Utilizadores"),
        }
    }

    /// Anuncia a estação (`Utilizadores/presenca-<maquina>.json`).
    pub fn anunciar(&self, nome: &str, maquina: &str) -> Resultado<()> {
        let f = self.pasta.join(format!("presenca-{maquina}.json"));
        let corpo = serde_json::json!({ "utilizador": nome, "maquina": maquina, "visto_em": agora_ms() });
        fs::write(f, serde_json::to_string(&corpo)?)?;
        Ok(())
    }
}
