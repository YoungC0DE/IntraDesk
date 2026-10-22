# IntraDesk — Guia para agentes

IntraDesk é um fork do RustDesk para **acesso remoto dentro da rede local (LAN)**,
**somente Windows 10/11 (x64)**, distribuído como **executável portátil** (sem
instalação). Não depende de internet nem de servidor rendezvous/relay: os
computadores se encontram por broadcast na LAN e conectam direto por IP.

Este arquivo é a fonte de verdade do projeto. O plano de trabalho está em
`todo.md`.

Repositório: https://github.com/YoungC0DE/IntraDesk (fork de
https://github.com/rustdesk/rustdesk; o histórico e os créditos do RustDesk
são preservados — não remova avisos de copyright nem a licença AGPL-3.0)

## Escopo do produto

### Mantém
- Descoberta automática dos computadores da LAN e lista na tela inicial.
- Conexão direta por IP (com senha).
- Na sessão remota: tela cheia, área de transferência (texto), ativar/desativar
  áudio, escala da imagem, qualidade da imagem, resolução.
- Controle de mouse e teclado (base do acesso remoto).

### Remove
- Gravação de tela, chat, teclado virtual, chamada de voz.
- Tudo que depende de internet/servidor: ID server, relay, login de conta,
  catálogo de endereços (address book), grupos, atualização automática, 2FA ligado a conta.
- Suporte a Linux, macOS, Android, iOS e Web (código, scripts, CI e pastas
  `flutter/{android,ios,linux,macos}`, `flatpak`, `appimage`, `fastlane`).
- Instalador / serviço instalado (o app roda como portátil).
- Itens ainda em aberto (transferência de arquivos, múltiplos monitores,
  Ctrl+Alt+Del etc.) estão em **Decisões pendentes** no `todo.md`. Não remova
  nada dessa lista sem a decisão registrada.

### Rede (padrão)
| Uso | Protocolo | Porta |
|---|---|---|
| Conexão direta (sessão) | TCP | 21118 |
| Descoberta LAN (broadcast) | UDP | 21119 |

As portas vêm de `RENDEZVOUS_PORT` (21116) + offset em `hbb_common`. O código
de descoberta fica em `src/lan.rs`; o acesso direto é a opção
`direct-server` / `direct-access-port` (`libs/base/src/config/keys.rs`).

## Identidade visual

Visual inspirado no AnyDesk: painel "Este computador" (nome, IP, senha) à
esquerda/topo, campo "Conectar a" e grade de computadores descobertos. Cores
em laranja escuro, definidas **só** em `MyTheme` (`flutter/lib/common.dart`).
Nenhuma cor hardcoded em widgets.

| Token | Claro | Escuro |
|---|---|---|
| `accent` (primária) | `#C2410C` | `#EA580C` |
| `accentHover` | `#9A3412` | `#F97316` |
| `accentSoft` (fundos/seleção) | `#FFEDD5` | `#431407` |
| texto sobre `accent` | `#FFFFFF` | `#FFFFFF` |

Nome exibido: **IntraDesk**. Ícones em `res/` (`icon.ico`, `tray-icon.ico`,
`logo.svg` etc.) e `flutter/windows/runner/resources/`.

## Estrutura relevante

- `src/` — app Rust (núcleo de sessão, servidor, cliente)
  - `src/lan.rs` — descoberta na LAN (broadcast UDP)
  - `src/client.rs` — conexão de saída; `secure_connection` faz o handshake
  - `src/server/` — captura de vídeo/áudio, clipboard, input, conexões de entrada
  - `src/rendezvous_mediator.rs` — fala com o servidor rendezvous (a desativar)
  - `src/platform/windows.rs` — código Windows
  - `src/lang/` — traduções (o produto usa **pt-BR** e **en**)
- `flutter/lib/desktop/` — UI desktop (a única usada)
  - `pages/desktop_home_page.dart`, `pages/connection_page.dart` — tela inicial
  - `widgets/remote_toolbar.dart` — barra da sessão remota (menus a enxugar)
- `flutter/lib/common/widgets/peer_*.dart` — cards e abas de peers
- `libs/portable/` — empacotador que gera **um único .exe** autoextraível
- `libs/hbb_common/` — rede, sockets, `Config`, protos de rendezvous.
  **Submódulo git** de https://github.com/rustdesk/hbb_common, no commit fixado
  pelo upstream. Depois de clonar: `git submodule update --init --recursive`.
  Para alterá-lo, veja T0.6 no `todo.md` (fork próprio do `hbb_common`)
- `libs/base/` — chaves de opção, proto de mensagens, código de plataforma do
  cliente. `libs/base/src/config/keys.rs` é o único caminho de import para opções
- `libs/scrap/` captura de tela · `libs/enigo/` entrada · `libs/clipboard/` área de transferência
- `build.py` — build; para o IntraDesk: `python build.py --flutter --portable`

## Regras para agentes

### Coordenação (vários agentes em paralelo)
- O plano está em `todo.md`. Antes de começar, **marque a tarefa** como
  `[~]` e coloque seu identificador em `@dono`. Ao terminar, marque `[x]`.
- Pegue só tarefas cujas dependências (`dep:`) estão `[x]`.
- Cada tarefa lista a **área** (arquivos/pastas). Não edite arquivos da área de
  outra tarefa em andamento; se precisar, anote no `todo.md` e combine.
- Arquivos "quentes" com alto risco de conflito: `flutter/lib/common.dart`,
  `src/flutter_ffi.rs`, `src/client.rs`, `src/server/connection.rs`,
  `flutter/lib/desktop/widgets/remote_toolbar.dart`, `Cargo.toml`. Mudanças
  nesses arquivos devem ser pequenas e focadas.
- Descoberta nova (bug, decisão necessária, tarefa faltando) vai para o
  `todo.md`, não só para a conversa.

### Git
- Branch por tarefa: `t<fase>.<n>-descricao-curta` (ex.: `t1.5-remove-recording`).
- Commits pequenos, em inglês, citando a tarefa (`T1.5: remove screen recording`).
- Branch principal: `master` (padrão do fork). Não faça push nem merge nela sem pedido explícito.

### Limpeza e remoção de features
- **Remover código é esperado.** Nas tarefas de limpeza, apague o código morto
  por completo (não deixe atrás de `#[cfg]` nem flags desligadas).
- Ao remover uma feature, remova todas as camadas juntas: UI Flutter, FFI
  (`src/flutter_ffi.rs` + wrapper Dart), lógica Rust e chaves de tradução não
  usadas. Se um item do protocolo (`message.proto`) deixar de ser usado,
  mantenha o campo no proto e remova só o tratamento.
- Não use `#[cfg(target_os = ...)]` para manter outras plataformas: o alvo é só
  Windows. Ao tocar em código com cfg de plataforma, simplifique para o caminho
  Windows apenas quando isso fizer parte da tarefa.
- Fora das tarefas de limpeza, mude só o necessário: sem refatorações,
  reformatações ou correções não relacionadas à tarefa (anote-as no `todo.md`).

### Idioma
- Código, símbolos e mensagens de commit em inglês.
- Documentação e textos de UI em pt-BR (com fallback em inglês).

### Máquina de desenvolvimento
- O notebook de desenvolvimento é acessado remotamente por um **RustDesk
  portátil** (`%LOCALAPPDATA%\rustdesk`). Nunca encerre processos `rustdesk.exe`
  nem altere `%APPDATA%\RustDesk` ou `%LOCALAPPDATA%\rustdesk`: isso derruba o
  acesso do usuário.
- Até T2.7 estar concluída, **não execute o build do IntraDesk nesta máquina**:
  ele ainda usa o mesmo nome, pasta de extração, configuração, pipe IPC e
  portas do RustDesk e entraria em conflito com a sessão remota. Teste em
  outros PCs.

### Segurança (inegociável)
- Toda conexão exige senha. Não crie modo "sem senha".
- Sem servidor rendezvous, o handshake atual cai em **texto puro**
  (`src/client.rs`, `secure_connection`, ramo `sign_pk == None`). A tarefa de
  criptografia (T3.x no `todo.md`) é pré-requisito para qualquer release.
- O app nunca deve abrir conexões para fora da LAN (sem telemetria, update ou
  servidores públicos).

### Build e testes (Windows)
- Ambiente: `scripts/setup-dev.ps1` (Rust 1.75, Flutter 3.24.5 com engine
  customizado do RustDesk, LLVM 15.0.6, vcpkg no commit fixado). As versões
  ficam nesse script **e** em `.github/workflows/build-windows.yml`; mude as duas juntas.
- Build local completo (bridge + Rust + Flutter + .exe único em `dist/`):
  `scripts/build.ps1`.
- CI: `.github/workflows/build-windows.yml` (usa `bridge.yml` para gerar o
  bridge). Roda em push na `master` e em branches `t*`; o .exe sai no artefato
  `IntraDesk-windows-x64`.
- Testes: `cargo test --lib --features flutter` e `cd flutter && flutter test`.

## Regras de Rust

- Evite `unwrap()` / `expect()` em código de produção. Exceções: testes e
  aquisição de lock (falha = envenenamento). No resto, `Result` + `?` ou
  tratamento explícito. Não ignore erros silenciosamente.
- Evite `.clone()` desnecessário; prefira empréstimo.
- Não adicione dependências sem necessidade.
- Imports: um `use` por crate, com bloco entre chaves
  (`use base::{fs, message_proto::*};`). Só separe quando um `#[cfg(...)]` vale
  para parte dos itens, ou para `pub use`.
- Comentários só para explicar motivo, restrição ou workaround não óbvio.

### Logging
- `debug` ou acima vai para o arquivo de log. Um log que pode disparar
  repetidamente (por pacote, frame, evento de entrada, iteração de loop ou em
  ritmo controlado pelo peer) não pode usar `debug`+ sem limite. Use
  `log::trace!` se for só para depuração, ou
  `hbb_common::throttled_log!(intervalo, nível, ...)` se indicar falha.

### Tokio
- Assuma que já existe um runtime. Nunca crie runtime aninhado nem chame
  `Runtime::block_on()` dentro de código async.
- Não segure lock através de `.await`.
- Use `.await`, `tokio::spawn`, canais; trabalho bloqueante em
  `spawn_blocking` ou thread dedicada. Nada de `std::thread::sleep()` em async.

## Flutter Rust Bridge

- **Não** rode `flutter_rust_bridge_codegen` (exige versão fixada difícil de
  instalar). Ao criar uma função FFI em `src/flutter_ffi.rs`, escreva à mão o
  wrapper Dart correspondente.
- `flutter/lib/generated_bridge.dart`, `src/bridge_generated.rs` e
  `src/bridge_generated.io.rs` são gerados (gitignored). Editar localmente para
  testar é ok, mas não vai para o commit.
- O bridge é gerado pelo `scripts/build.ps1` (local) e pelo `bridge.yml` (CI).

## Testes

- Correção de bug ganha 1 a 3 testes de regressão que falham sem a correção e
  passam com ela. Teste o que o usuário vê ou o que a API retorna, não estado
  privado.
- Sem infraestrutura de teste nova (harness, mocks, hooks só para teste em
  código de produção) sem pedido.
- Testes Dart em `flutter/test/*_test.dart`. Testes Rust em módulo
  `#[cfg(test)]` ao lado do código.

## Traduções (`src/lang/*.rs`)

- Cada arquivo é um `HashMap<chave, tradução>`. A chave é o texto em inglês.
- `template.rs` é a lista mestre de chaves; `en.rs` só tem as chaves cujo texto
  difere da chave. Os demais idiomas têm o conjunto completo; não traduzida =
  `("chave", "")`.
- Chave nova: frase em "sentence case" (`Use ID whitelisting`), adicionada no
  fim de `template.rs` e de cada idioma mantido. Preserve `{}`, `\n`, `\"`.
- Não traduza nomes técnicos (IntraDesk, TCP, UDP, UAC, IP...).
