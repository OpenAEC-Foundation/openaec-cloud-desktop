//! Testes de integração do núcleo local.

use std::fs;
use std::path::Path;

use openaec_local::base::Base;
use openaec_local::bloqueio::Trancas;
use openaec_local::estrutura::{Papel, Projecto, Raiz};
use openaec_local::historico::Historico;
use openaec_local::motor::{self, Accao};
use openaec_local::utilizadores::{iniciar_administrador, ListaUtilizadores, Utilizador};
use openaec_local::versoes::Versoes;

fn raiz(temp: &tempfile::TempDir) -> Raiz {
    Raiz::iniciar(temp.path()).expect("iniciar raiz")
}

fn projecto(raiz: &Raiz, nome: &str) -> std::path::PathBuf {
    raiz
        .criar_projecto(&Projecto {
            nome: nome.into(),
            descricao: "teste".into(),
            responsavel: "admin".into(),
            etiquetas: vec![],
            estado: "activo".into(),
            criado_em: openaec_local::agora_ms(),
            alterado_em: openaec_local::agora_ms(),
        })
        .expect("criar projecto")
}

#[test]
fn estrutura_da_raiz() {
    let temp = tempfile::tempdir().unwrap();
    let raiz = raiz(&temp);
    for pasta in openaec_local::RAIZ_PASTAS {
        assert!(temp.path().join(pasta).is_dir(), "falta {pasta}");
    }
    assert!(raiz.configuracao().is_ok());
    let outra = Raiz::abrir(temp.path()).expect("reabrir raiz");
    assert_eq!(outra.caminho(), raiz.caminho());
}

#[test]
fn projectos_sao_autonomos() {
    let temp = tempfile::tempdir().unwrap();
    let raiz = raiz(&temp);
    let pasta = projecto(&raiz, "Edificio X");
    for sub in ["Documentos", "Modelos", "IFC", "DWG", "PDF", "Quantidades"] {
        assert!(pasta.join(sub).is_dir(), "falta {sub}");
    }
    assert!(pasta.join("projeto.json").is_file());
    let lista = raiz.listar_projectos().unwrap();
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0].nome, "Edificio X");
}

#[test]
fn utilizadores_e_permissoes() {
    let temp = tempfile::tempdir().unwrap();
    let raiz = raiz(&temp);
    iniciar_administrador(&raiz, "segredo").unwrap();
    let lista = ListaUtilizadores::carregar(&raiz).unwrap();
    let admin = lista.autenticar("admin", "segredo").expect("login falhou");
    assert!(admin.papel.administra());
    assert!(lista.autenticar("admin", "errada").is_none());

    let mut lista = lista;
    let ana = Utilizador::novo("ana", Papel::GestorDeProjecto, "passe1").unwrap();
    lista.gravar_utilizador(&raiz, ana).unwrap();
    let consulta = Utilizador::novo("rui", Papel::Consulta, "passe2").unwrap();
    lista.gravar_utilizador(&raiz, consulta).unwrap();
    let lista = ListaUtilizadores::carregar(&raiz).unwrap();
    assert!(lista.procurar("ana").unwrap().papel.gere());
    assert!(!lista.procurar("rui").unwrap().papel.edita());
}

#[test]
fn trancas_sao_exclusivas() {
    let temp = tempfile::tempdir().unwrap();
    let raiz = raiz(&temp);
    let pasta = projecto(&raiz, "Moradia Y");
    let trancas = Trancas::do_projecto(&pasta);

    trancas.trancar("IFC/modelo.ifc", "ana", "pc-ana").unwrap();
    assert!(trancas.trancar("IFC/modelo.ifc", "rui", "pc-rui").is_err(), "duas trancas no mesmo ficheiro");
    let actual = trancas.actual("IFC/modelo.ifc").unwrap().unwrap();
    assert_eq!(actual.utilizador, "ana");

    trancas.destrancar("IFC/modelo.ifc", "ana").unwrap();
    trancas.trancar("IFC/modelo.ifc", "rui", "pc-rui").unwrap();
    assert!(trancas.todas().unwrap().len() == 1);
}

#[test]
fn versoes_e_recuperacao() {
    let temp = tempfile::tempdir().unwrap();
    let raiz = raiz(&temp);
    let pasta = projecto(&raiz, "Escola Z");
    let ficheiro = "Documentos/nota.txt";
    fs::write(pasta.join(ficheiro), "versão 1").unwrap();

    let versoes = Versoes::do_projecto(&pasta);
    versoes
        .guardar_antes_de_escrever(&pasta, ficheiro, "ana", "pc-ana", "antes de gravar v2")
        .unwrap();
    fs::write(pasta.join(ficheiro), "versão 2").unwrap();

    let historico = versoes.historico(ficheiro).unwrap();
    assert_eq!(historico.len(), 1);
    assert_eq!(historico[0].utilizador, "ana");

    let v1 = &historico[0];
    versoes.recuperar(&pasta, v1, "rui").unwrap();
    assert_eq!(fs::read_to_string(pasta.join(ficheiro)).unwrap(), "versão 1");
    let historico = versoes.historico(ficheiro).unwrap();
    assert_eq!(historico.len(), 2, "a recuperação também guarda a actual");
}

#[test]
fn motor_detecta_mudancas_e_conflitos() {
    let temp = tempfile::tempdir().unwrap();
    let raiz = raiz(&temp);
    let pasta = projecto(&raiz, "Edificio X");
    fs::write(pasta.join("IFC/a.ifc"), "aaa").unwrap();

    let plano = motor::ronda(raiz.caminho(), &pasta, "pc-1", &[]).unwrap();
    assert_eq!(plano.contar(&Accao::Novo), 1);
    motor::aplicar(raiz.caminho(), &pasta, "pc-1", "ana", &plano).unwrap();

    let plano = motor::ronda(raiz.caminho(), &pasta, "pc-1", &[]).unwrap();
    assert!(plano.accoes.is_empty(), "sem alterações, plano vazio");

    fs::write(pasta.join("IFC/a.ifc"), "bbb").unwrap();
    fs::write(pasta.join("DWG/b.dwg"), "novo").unwrap();
    let plano = motor::ronda(raiz.caminho(), &pasta, "pc-1", &[]).unwrap();
    assert_eq!(plano.contar(&Accao::Alterado), 1);
    assert_eq!(plano.contar(&Accao::Novo), 1);
    motor::aplicar(raiz.caminho(), &pasta, "pc-1", "ana", &plano).unwrap();

    // A estação pc-2 nunca viu nada: tudo é novo para ela.
    let plano = motor::ronda(raiz.caminho(), &pasta, "pc-2", &[]).unwrap();
    assert_eq!(plano.contar(&Accao::Novo), 2);

    // Conflito: o ficheiro mudou na pasta e localmente ao mesmo tempo.
    fs::write(pasta.join("IFC/a.ifc"), "ccc").unwrap();
    let plano = motor::ronda(raiz.caminho(), &pasta, "pc-1", &["IFC/a.ifc".to_string()]).unwrap();
    assert_eq!(plano.contar(&Accao::Conflito), 1);
    motor::resolver_conflito(&pasta, "IFC/a.ifc", "ana").unwrap();
    assert!(pasta.join("IFC/a (conflito ana).ifc").is_file());

    fs::remove_file(pasta.join("DWG/b.dwg")).unwrap();
    let plano = motor::ronda(raiz.caminho(), &pasta, "pc-1", &[]).unwrap();
    assert_eq!(plano.contar(&Accao::Removido), 1);
}

#[test]
fn base_sqlite_na_pasta_raiz() {
    let temp = tempfile::tempdir().unwrap();
    let raiz = raiz(&temp);
    let base = Base::abrir(raiz.caminho()).unwrap();
    assert!(Path::new(raiz.oculta().to_str().unwrap())
        .join("openaec.db")
        .is_file());
    base.registar_ficheiro("IFC/a.ifc", "Edificio X", "abc", 3, 1, "ana").unwrap();
    let estado = base.estado_ficheiro("IFC/a.ifc").unwrap().unwrap();
    assert_eq!(estado.0, "abc");
    base.esquecer_ficheiro("IFC/a.ifc").unwrap();
    assert!(base.estado_ficheiro("IFC/a.ifc").unwrap().is_none());
}

#[test]
fn historico_regista_e_le() {
    let temp = tempfile::tempdir().unwrap();
    let raiz = raiz(&temp);
    let historico = Historico::da_raiz(&raiz);
    historico.registar("ana", "pc-ana", "gravar", "Edificio X", "IFC/a.ifc", "").unwrap();
    historico.registar("rui", "pc-rui", "trancar", "Moradia Y", "DWG/b.dwg", "").unwrap();
    let tudo = historico.ler(None, 10).unwrap();
    assert_eq!(tudo.len(), 2);
    assert_eq!(tudo[0].accao, "trancar", "mais recente primeiro");
    let so = historico.ler(Some("Edificio X"), 10).unwrap();
    assert_eq!(so.len(), 1);
    assert_eq!(so[0].utilizador, "ana");
}
