//! Ferramenta de administração do OpenAEC Local Workspace.
//!
//! Exemplos:
//!   openaec-local init /OpenAEC --admin-passe segredo
//!   openaec-local utilizador criar ana gestor --raiz /OpenAEC
//!   openaec-local projeto criar "Edifício X" --raiz /OpenAEC
//!   openaec-local trancar Edificio_X IFC/modelo.ifc --raiz /OpenAEC --utilizador ana
//!   openaec-local historico Edificio_X --raiz /OpenAEC

use std::path::PathBuf;
use std::process::exit;

use openaec_local::estrutura::{Papel, Projecto, Raiz};
use openaec_local::historico::Historico;
use openaec_local::motor;
use openaec_local::utilizadores::{iniciar_administrador, ListaUtilizadores, Utilizador};
use openaec_local::versoes::Versoes;
use openaec_local::bloqueio::Trancas;

fn maquina() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "estacao".into())
}

fn papel_de(texto: &str) -> Option<Papel> {
    match texto.to_lowercase().as_str() {
        "administrador" | "admin" => Some(Papel::Administrador),
        "gestor" | "gestordeprojecto" => Some(Papel::GestorDeProjecto),
        "tecnico" => Some(Papel::Tecnico),
        "consulta" => Some(Papel::Consulta),
        _ => None,
    }
}

fn falha(erro: openaec_local::Erro) -> ! {
    eprintln!("erro: {erro}");
    exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        ajuda();
        exit(2);
    }
    let raiz = valor(&args, "--raiz").map(PathBuf::from).unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    });
    match args[0].as_str() {
        "init" => {
            let raiz_caminho = args.get(1).cloned().unwrap_or_else(|| raiz.to_string_lossy().into_owned());
            let r = match Raiz::iniciar(&raiz_caminho) {
                Ok(r) => r,
                Err(e) => falha(e),
            };
            if let Some(passe) = valor(&args, "--admin-passe") {
                if let Err(e) = iniciar_administrador(&r, &passe) {
                    eprintln!("aviso: {e}");
                }
            }
            println!("ambiente criado em {}", r.caminho().display());
        }
        "utilizador" => {
            let r = match Raiz::abrir(&raiz) {
                Ok(r) => r,
                Err(e) => falha(e),
            };
            let mut lista = match ListaUtilizadores::carregar(&r) {
                Ok(l) => l,
                Err(e) => falha(e),
            };
            match args.get(1).map(String::as_str) {
                Some("criar") => {
                    let nome = args.get(2).expect("falta o nome");
                    let papel = papel_de(args.get(3).map(|v| v.as_str()).unwrap_or("tecnico")).expect("papel: administrador | gestor | tecnico | consulta");
                    let passe = valor(&args, "--passe").unwrap_or_else(|| "mudar-já".into());
                    let u = match Utilizador::novo(nome, papel, &passe) {
                        Ok(u) => u,
                        Err(e) => falha(e),
                    };
                    if let Err(e) = lista.gravar_utilizador(&r, u) {
                        falha(e);
                    }
                    println!("utilizador {nome} criado com papel {papel:?}");
                }
                Some("listar") | _ => {
                    for u in &lista.utilizadores {
                        println!("{:<20} {:<18} {:?}", u.nome, u.maquina, u.papel);
                    }
                }
            }
        }
        "projeto" => {
            let r = match Raiz::abrir(&raiz) {
                Ok(r) => r,
                Err(e) => falha(e),
            };
            match args.get(1).map(String::as_str) {
                Some("criar") => {
                    let nome = args.get(2).expect("falta o nome do projecto");
                    let p = Projecto {
                        nome: nome.into(),
                        descricao: valor(&args, "--descricao").unwrap_or_default(),
                        responsavel: valor(&args, "--responsavel").unwrap_or_default(),
                        etiquetas: Vec::new(),
                        estado: "activo".into(),
                        criado_em: openaec_local::agora_ms(),
                        alterado_em: openaec_local::agora_ms(),
                    };
                    match r.criar_projecto(&p) {
                        Ok(pasta) => println!("projecto criado em {}", pasta.display()),
                        Err(e) => falha(e),
                    }
                }
                Some("listar") | _ => match r.listar_projectos() {
                    Ok(lista) => {
                        for p in lista {
                            println!("{}  {}", p.nome, p.estado);
                        }
                    }
                    Err(e) => falha(e),
                },
            }
        }
        "trancar" | "destrancar" => {
            let r = match Raiz::abrir(&raiz) {
                Ok(r) => r,
                Err(e) => falha(e),
            };
            let projeto = args.get(1).expect("falta o projecto");
            let ficheiro = args.get(2).expect("falta o ficheiro");
            let utilizador = valor(&args, "--utilizador").unwrap_or_else(|| "admin".into());
            let pasta = r.projetos().join(projeto);
            let trancas = Trancas::do_projecto(&pasta);
            let historico = Historico::da_raiz(&r);
            let resultado = if args[0] == "trancar" {
                trancas.trancar(ficheiro, &utilizador, &maquina()).map(|_| "tranca criada")
            } else {
                trancas.destrancar(ficheiro, &utilizador).map(|_| "tranca libertada")
            };
            match resultado {
                Ok(msg) => {
                    let _ = historico.registar(&utilizador, &maquina(), &args[0], projeto, ficheiro, "");
                    println!("{msg}");
                }
                Err(e) => falha(e),
            }
        }
        "historico" => {
            let r = match Raiz::abrir(&raiz) {
                Ok(r) => r,
                Err(e) => falha(e),
            };
            let historico = Historico::da_raiz(&r);
            let filtro = args.get(1).map(String::as_str);
            match historico.ler(filtro, 50) {
                Ok(registos) => {
                    for reg in registos {
                        println!(
                            "{} {:<12} {:<10} {} {}",
                            reg.momento, reg.utilizador, reg.accao, reg.projecto, reg.ficheiro
                        );
                    }
                }
                Err(e) => falha(e),
            }
        }
        "versoes" => {
            let r = match Raiz::abrir(&raiz) {
                Ok(r) => r,
                Err(e) => falha(e),
            };
            let projeto = args.get(1).expect("falta o projecto");
            let ficheiro = args.get(2).expect("falta o ficheiro");
            let pasta = r.projetos().join(projeto);
            match Versoes::do_projecto(&pasta).historico(ficheiro) {
                Ok(versoes) => {
                    for v in versoes {
                        println!("{} {:<12} {} bytes  {}", v.momento, v.utilizador, v.bytes, v.comentario);
                    }
                }
                Err(e) => falha(e),
            }
        }
        "sincronizar" => {
            let r = match Raiz::abrir(&raiz) {
                Ok(r) => r,
                Err(e) => falha(e),
            };
            let projeto = args.get(1).expect("falta o projecto");
            let pasta = r.projetos().join(projeto);
            let m = maquina();
            let plano = match motor::ronda(r.caminho(), &pasta, &m, &[]) {
                Ok(p) => p,
                Err(e) => falha(e),
            };
            println!(
                "novos {} alterados {} removidos {} conflitos {} trancados {}",
                plano.contar(&motor::Accao::Novo),
                plano.contar(&motor::Accao::Alterado),
                plano.contar(&motor::Accao::Removido),
                plano.contar(&motor::Accao::Conflito),
                plano.trancados.len()
            );
            if let Err(e) = motor::aplicar(r.caminho(), &pasta, &m, "admin", &plano) {
                falha(e);
            }
        }
        _ => ajuda(),
    }
}

fn valor(args: &[String], chave: &str) -> Option<String> {
    args.iter()
        .position(|a| a == chave)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn ajuda() {
    println!(
        "OpenAEC Local Workspace — administração\n\
         \n\
         uso: openaec-local <comando> [argumentos] [--raiz <pasta>]\n\
         \n\
         comandos:\n\
           init <pasta> [--admin-passe <passe>]         cria a estrutura do ambiente\n\
           utilizador criar <nome> <papel> [--passe p]  administrador|gestor|tecnico|consulta\n\
           utilizador listar                           lista os utilizadores\n\
           projeto criar <nome> [--descricao d]        cria um projecto\n\
           projeto listar                             lista os projectos\n\
           trancar <projeto> <ficheiro>                bloqueia um ficheiro para edição\n\
           destrancar <projeto> <ficheiro>              liberta o ficheiro\n\
           versoes <projeto> <ficheiro>                histórico de versões\n\
           sincronizar <projeto>                       uma ronda de sincronização\n\
           historico [projeto]                          últimas alterações"
    );
}
