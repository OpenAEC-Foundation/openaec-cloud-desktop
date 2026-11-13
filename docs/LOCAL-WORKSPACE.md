# OpenAEC Local Workspace

Variante do OpenAEC Desktop App para **trabalho totalmente local, sem cloud**: colaboração
BIM/CAD entre vários computadores de uma rede interna (LAN/Wi-Fi) apenas através de uma
**pasta partilhada** (SMB no Windows, NFS/SMB no Linux). Mantém a filosofia do projecto
original (Tauri 2 + Rust + SolidJS, trancas, versões, histórico); substitui a camada cloud
por ficheiros locais e SQLite na própria pasta raiz.

- Núcleo local: `crates/openaec-local` (biblioteca Rust + CLI `openaec-local`)
- Testes: `cargo test` dentro de `crates/openaec-local` (8 testes de integração)

---

## 1. Análise da arquitectura actual

| Componente | Papel | Dependência cloud |
|---|---|---|
| `crates/openaec-sync` (`api.rs`, `sessie.rs`, `oauth.rs`) | cliente `openaec_storage.api.*`, OAuth/bearer | sim |
| `crates/openaec-sync` (`opslag.rs`, `motor.rs`, `blokken.rs`) | motor de sincronização delta contra o servidor | sim |
| `crates/openaec-sync` (`overdracht.rs`) | enviar ficheiros com link de download | sim |
| `src-tauri` (`cloudfilter.rs`, `serverplek.rs`, `opaanvraag.rs`, `updates.rs`) | Cloud Filter API, ambiente ligado, pedidos assinados, actualizações | sim |
| `src-tauri` (`sloten.rs`, `delen.rs`, `bewaking.rs`, `logboek.rs`) | trancas, deellinks, vigilância de ficheiros, registo | via servidor |
| `src` (SolidJS) | UI: Overzicht, Ruimtes, Delen, Tools, Instellingen | via servidor |

## 2. Componentes cloud encontrados

1. API `openaec_storage.api.*` (árvore, upload/download em blocos, quota).
2. Autenticação OAuth2 com renovação de token (`sessie.rs`, `oauth.rs`, chaveiro do SO).
3. Trancas, versões e deellinks guardados no servidor.
4. `openaec_storage.overdracht.*` (envio de ficheiros com link).
5. Cloud Filter API (placeholders do Windows) sobre pastas do servidor.
6. Actualizações automáticas e catálogo de ferramentas por HTTP.

## 3. Estratégia de migração

- **Manter**: Tauri 2, SolidJS, vigilância local (`notify`), trancas, versões, histórico,
  a experiência de interface (janelas, barra de estado, título 32 px).
- **Substituir**: todo o transporte HTTP por leitura/escrita directa na pasta partilhada;
  autenticação OAuth por ficheiro local de utilizadores (ou Active Directory opcional);
  índice do servidor por **SQLite (WAL)** na raiz.
- **Acrescentar**: estado por estação (`.openaec/estado-<maquina>.json`), presenças,
  expiração de trancas abandonadas, resolução de conflitos que conserva ambas as versões.
- **Desligar**: Cloud Filter, deellinks públicos, transferências, actualizador online
  (passam a instalação local/por rede).

## 4. Nova arquitectura

```
┌────────────┐   ┌────────────┐   ┌────────────┐
│ Estação A  │   │ Estação B  │   │ Estação C  │   (Windows 10/11, Linux)
│ OpenAEC UI │   │ OpenAEC UI │   │ OpenAEC UI │
│ motor+cache│   │ motor+cache│   │ motor+cache│
└─────┬──────┘   └─────┬──────┘   └─────┬──────┘
      │   SMB / NFS (pasta partilhada, só de leitura-escrita)
      └───────────────┴────────────────┘
                      ▼
            /Servidor/OpenAEC  (pasta raiz)
            ├── Projetos/  Utilizadores/  Configuracoes/
            ├── Biblioteca/  Backups/  Logs/
            └── .openaec/  (openaec.db SQLite + estado por estação)
```

Cada estação: (1) observa a pasta (vigilância + ronda periódica, 30 s); (2) compara com o
seu estado guardado; (3) aplica o plano (novos/alterados/removidos/conflitos); (4) escreve
trancas, versões e histórico como ficheiros que todas as estações leem directamente.

## 5. Estrutura de pastas

```
/OpenAEC
├── Projetos/
│   └── Edificio_X/
│       ├── projeto.json          (metadados, responsável, estado)
│       ├── Documentos/ Modelos/ IFC/ DWG/ PDF/ Imagens/ Quantidades/
│       └── .openaec/
│           ├── versoes/          (versões anteriores por ficheiro)
│           └── trancas/          (trancas activas)
├── Utilizadores/
│   ├── utilizadores.json         (contas locais; origem local|activedirectory)
│   └── presenca-<maquina>.json   (quem está online)
├── Configuracoes/configuracao.json
├── Biblioteca/                   (componentes partilhados)
├── Backups/
├── Logs/historico.jsonl          (histórico de alterações)
└── .openaec/
    ├── openaec.db                (SQLite WAL: índice de ficheiros, versões, trancas)
    └── estado-<maquina>.json     (estado conhecido por estação)
```

## 6. Plano de implementação por fases

| Fase | Conteúdo | Estado |
|---|---|---|
| 1 | Núcleo local `openaec-local`: estrutura, utilizadores/permissões, trancas, versões, histórico, SQLite, motor de rondas, conflitos | **concluída** (este PR) |
| 2 | Integração no `src-tauri`: comandos Tauri sobre o núcleo, vigilância `notify`, presenças | seguinte |
| 3 | UI SolidJS em português: ecrãs Projectos, Ficheiros, Utilizadores, Histórico (mantém a shell OpenAEC) | seguinte |
| 4 | Cache local por estação + modo offline (trabalhar local, sincronizar ao regressar a ligação) | seguinte |
| 5 | Active Directory opcional, cópias de segurança para `Backups/`, instalação NSIS/deb | seguinte |

## 7. Permissões

| Papel | Projectos | Ficheiros | Utilizadores |
|---|---|---|---|
| Administrador | criar/apagar | editar | gerir |
| Gestor de Projecto | criar | editar | — |
| Técnico | — | editar | — |
| Consulta | — | ler | — |

## 8. Instalação (por estação)

```bash
# requisitos: Rust 1.77+, Node 20+ (só para a UI)
cd crates/openaec-local
cargo build --release
# binário: target/release/openaec-local
```

Preparar o ambiente (uma vez, na pasta partilhada):

```bash
openaec-local init /Servidor/OpenAEC --admin-passe <palavra-passe-do-admin>
openaec-local utilizador criar ana gestor --raiz /Servidor/OpenAEC --passe passe1
openaec-local projeto criar "Edificio X" --raiz /Servidor/OpenAEC
```

## 9. Operar em rede LAN com vários utilizadores

1. **Servidor de ficheiros**: partilhe `/OpenAEC` por SMB (Windows) ou NFS/SMB (Linux).
   Monte-a em todas as estações com a mesma letra/ponto de montagem.
2. **Permissões da partilha**: leitura+escrita para o grupo de trabalho; sem exclusividade
   (várias estações escrevem trancas/histórico em simultâneo, sempre em *append*).
3. **Cada estação**: inicia sessão com a conta local (`utilizadores.json`); a aplicação
   anuncia-se em `Utilizadores/presenca-<maquina>.json` e faz rondas a cada 30 s.
4. **Edição**: antes de abrir um ficheiro para editar, a estação tranca-o
   (`trancar`); as outras estações vêem a tranca e avisam. Trancas sem renovação
   durante 10 minutos são consideradas abandonadas e libertadas.
5. **Conflitos**: se um ficheiro mudar na pasta e localmente ao mesmo tempo, o motor marca
   `Conflito` e conserva as duas versões (`<nome> (conflito <utilizador>).<ext>`).
6. **Versões**: cada gravação guarda a versão anterior em `.openaec/versoes/`;
   recuperar com `versoes` + `recuperar` (a versão actual também fica guardada).
7. **Histórico**: tudo o que acontece fica em `Logs/historico.jsonl`
   (`historico [projeto]`).
8. **Offline**: se a pasta partilhada ficar inacessível, a estação continua a abrir os
   ficheiros do cache local e sincroniza quando a ligação regressar (fase 4).
9. **Cópias de segurança**: copie a raiz inteira (incluindo `.openaec/openaec.db`) para
   `Backups/` ou outro destino; o SQLite em modo WAL copia-se de forma segura com a
   aplicação fechada ou usando o checkpoint automático.

### Comandos do dia-a-dia

```bash
openaec-local sincronizar "Edificio X" --raiz /OpenAEC      # ronda manual
openaec-local trancar "Edificio X" IFC/modelo.ifc --raiz /OpenAEC --utilizador ana
openaec-local destrancar "Edificio X" IFC/modelo.ifc --raiz /OpenAEC --utilizador ana
openaec-local versoes "Edificio X" IFC/modelo.ifc --raiz /OpenAEC
openaec-local historico "Edificio X" --raiz /OpenAEC
```

## 10. API do núcleo (resumo)

```rust
use openaec_local::estrutura::{Papel, Projecto, Raiz};

let raiz = Raiz::iniciar("/Servidor/OpenAEC")?;
let pasta = raiz.criar_projecto(&Projecto { nome: "Edificio X".into(), /* .. */ })?;

let trancas = openaec_local::bloqueio::Trancas::do_projecto(&pasta);
trancas.trancar("IFC/modelo.ifc", "ana", "pc-ana")?;   // exclusivo entre estações

let versoes = openaec_local::versoes::Versoes::do_projecto(&pasta);
versoes.guardar_antes_de_escrever(&pasta, "IFC/modelo.ifc", "ana", "pc-ana", "revisão")?;

let plano = openaec_local::motor::ronda(raiz.caminho(), &pasta, "pc-ana", &[])?;
openaec_local::motor::aplicar(raiz.caminho(), &pasta, "pc-ana", "ana", &plano)?;
```
