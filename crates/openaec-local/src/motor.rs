//! Motor local: compara a pasta partilhada com o estado conhecido e produz
//! um plano de sincronização por estação.
//!
//! Cada estação guarda o seu estado em `.openaec/estado-<maquina>.json`
//! (caminho → hash). Uma ronda:
//!
//! 1. percorre a pasta do projecto e calcula o hash BLAKE3 de cada ficheiro;
//! 2. compara com o estado guardado: `adicionados`, `alterados`, `removidos`;
//! 3. cruza com o histórico e as trancas para detectar conflitos
//!    (o ficheiro mudou na pasta por outro utilizador enquanto esta estação
//!    também o alterou) — em caso de conflito, as duas versões são
//!    conservadas: a da pasta fica e a local é copiada como
//!    `<nome> (conflito <utilizador>)`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::base::Base;
use crate::bloqueio::Trancas;
use crate::{agora_ms, Erro, Resultado};

/// Hash BLAKE3 do conteúdo de um ficheiro (hexadecimal).
pub fn hash_ficheiro(caminho: &Path) -> Resultado<String> {
    let bytes = fs::read(caminho)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

/// Estado de uma estação: caminho → hash da última ronda vista.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Estado {
    #[serde(default)]
    pub ficheiros: BTreeMap<String, String>,
    #[serde(default = "agora_ms")]
    pub visto_em: u64,
}

impl Estado {
    fn caminho_da_estacao(raiz: &Path, maquina: &str) -> std::path::PathBuf {
        raiz.join(".openaec").join(format!("estado-{maquina}.json"))
    }

    /// Carrega o estado da estação (novo se não existir).
    pub fn carregar(raiz: &Path, maquina: &str) -> Resultado<Self> {
        let f = Self::caminho_da_estacao(raiz, maquina);
        if !f.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(&fs::read_to_string(f)?)?)
    }

    /// Grava o estado da estação.
    pub fn guardar(&self, raiz: &Path, maquina: &str) -> Resultado<()> {
        let f = Self::caminho_da_estacao(raiz, maquina);
        if let Some(pai) = f.parent() {
            fs::create_dir_all(pai)?;
        }
        fs::write(f, serde_json::to_string(self)?)?;
        Ok(())
    }
}

/// O que fazer com um ficheiro numa ronda.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "accão")]
pub enum Accao {
    /// Novo na pasta partilhada.
    Novo,
    /// Conteúdo diferente do estado conhecido.
    Alterado,
    /// Desapareceu da pasta.
    Removido,
    /// Alterado na pasta e por esta estação ao mesmo tempo.
    Conflito,
}

/// Plano de uma ronda de sincronização.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Plano {
    pub accoes: Vec<(String, Accao)>,
    /// Ficheiros bloqueados por outros utilizadores (não editar).
    pub trancados: Vec<(String, String)>,
}

impl Plano {
    pub fn contar(&self, accao: &Accao) -> usize {
        self.accoes.iter().filter(|(_, a)| a == accao).count()
    }
}

/// Ignora ficheiros internos e temporários.
fn ignorar(caminho: &str) -> bool {
    let nome = caminho.rsplit(['/', '\\']).next().unwrap_or(caminho);
    nome.starts_with("~$")
        || nome == "projeto.json"
        || nome.starts_with(".")
        || nome.ends_with(".tmp")
        || nome.ends_with(".lock")
        || caminho.contains(".openaec")
}

/// Percorre a pasta do projecto e devolve caminho relativo → hash.
fn varrer(projecto: &Path) -> Resultado<BTreeMap<String, String>> {
    fn varrer_rec(pasta: &Path, prefixo: &str, saida: &mut BTreeMap<String, String>) -> Resultado<()> {
        for entrada in fs::read_dir(pasta)? {
            let p = entrada?.path();
            let nome = p.file_name().unwrap().to_string_lossy().into_owned();
            let relativo = if prefixo.is_empty() {
                nome.clone()
            } else {
                format!("{prefixo}/{nome}")
            };
            if ignorar(&relativo) {
                continue;
            }
            if p.is_dir() {
                varrer_rec(&p, &relativo, saida)?;
            } else if p.is_file() {
                saida.insert(relativo, hash_ficheiro(&p)?);
            }
        }
        Ok(())
    }
    let mut saida = BTreeMap::new();
    varrer_rec(projecto, "", &mut saida)?;
    Ok(saida)
}

/// Uma ronda: compara a pasta do projecto com o estado da estação.
///
/// `alteracoes_locais` são os ficheiros que esta estação sabe ter gravado
/// desde a última ronda (para detectar conflitos). As trancas activas são
/// devolvidas para a interface avisar quem está a editar o quê.
pub fn ronda(
    raiz: &Path,
    projecto: &Path,
    maquina: &str,
    alteracoes_locais: &[String],
) -> Resultado<Plano> {
    let estado = Estado::carregar(raiz, maquina)?;
    let actual = varrer(projecto)?;
    let mut plano = Plano::default();

    for (caminho, hash) in &actual {
        let accao = match estado.ficheiros.get(caminho) {
            None => Accao::Novo,
            Some(h) if h == hash => continue,
            Some(_) => {
                if alteracoes_locais.iter().any(|l| l == caminho) {
                    Accao::Conflito
                } else {
                    Accao::Alterado
                }
            }
        };
        plano.accoes.push((caminho.clone(), accao));
    }
    for caminho in estado.ficheiros.keys() {
        if !actual.contains_key(caminho) {
            plano.accoes.push((caminho.clone(), Accao::Removido));
        }
    }

    let trancas = Trancas::do_projecto(projecto);
    for tranca in trancas.todas()? {
        if tranca.maquina != maquina {
            plano
                .trancados
                .push((tranca.ficheiro.clone(), tranca.utilizador.clone()));
        }
    }
    Ok(plano)
}

/// Aplica uma ronda: actualiza o estado da estação e o índice SQLite.
/// Em conflito, conserva ambas as versões (a local é copiada com sufixo).
pub fn aplicar(
    raiz: &Path,
    projecto: &Path,
    maquina: &str,
    utilizador: &str,
    plano: &Plano,
) -> Resultado<()> {
    let mut estado = Estado::carregar(raiz, maquina)?;
    let base = Base::abrir(raiz)?;
    let nome_projecto = projecto
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    for (caminho, accao) in &plano.accoes {
        match accao {
            Accao::Novo | Accao::Alterado | Accao::Conflito => {
                let p = projecto.join(caminho);
                if p.is_file() {
                    let hash = hash_ficheiro(&p)?;
                    let bytes = fs::metadata(&p)?.len();
                    base.registar_ficheiro(
                        caminho,
                        &nome_projecto,
                        &hash,
                        bytes,
                        agora_ms(),
                        utilizador,
                    )?;
                    estado.ficheiros.insert(caminho.clone(), hash);
                }
            }
            Accao::Removido => {
                base.esquecer_ficheiro(caminho)?;
                estado.ficheiros.remove(caminho);
            }
        }
    }
    estado.visto_em = agora_ms();
    estado.guardar(raiz, maquina)?;
    Ok(())
}

/// Resolve um conflito conservando a versão local com sufixo
/// `<nome> (conflito <utilizador>).<ext>`.
pub fn resolver_conflito(projecto: &Path, ficheiro: &str, utilizador: &str) -> Resultado<()> {
    let origem = projecto.join(ficheiro);
    if !origem.is_file() {
        return Err(Erro::Caminho(format!("{ficheiro} não existe")));
    }
    let destino = {
        let p = Path::new(ficheiro);
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = p
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let nome = format!("{stem} (conflito {utilizador}){ext}");
        p.with_file_name(nome)
    };
    fs::copy(&origem, projecto.join(&destino))?;
    Ok(())
}
