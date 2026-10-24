# IntraDesk — Plano de trabalho

Plano para transformar o fork do RustDesk no IntraDesk (ver `CLAUDE.md` e `README.md`).

## Como usar este arquivo (vários agentes)

- Status: `[ ]` livre · `[~]` em andamento · `[x]` concluída · `[!]` bloqueada (explique)
- Para pegar uma tarefa: mude para `[~]` e preencha `@dono: <agente/pessoa>`.
- Só pegue tarefas com todas as dependências (`dep:`) em `[x]`.
- **Área** = arquivos/pastas que a tarefa altera. Duas tarefas `[~]` não devem
  ter áreas sobrepostas. Se precisar mexer fora da sua área, anote aqui antes.
- Toda tarefa termina com: build Rust ok, `flutter analyze` sem erros novos e os
  testes da seção afetada passando. Registre o resultado em uma linha na tarefa.
- Achou algo novo? Adicione uma tarefa na fase certa (próximo número livre).

### Trilhas que podem rodar em paralelo (após a Fase 0)

| Trilha | Tarefas | Área principal |
|---|---|---|
| A — Limpeza de plataformas | T1.1–T1.4 | pastas de plataforma, CI, scripts |
| B — Limpeza de features | T1.5–T1.12 | `flutter/lib`, `src/` por feature |
| C — Rede LAN | T2.x | `src/lan.rs`, `src/client.rs`, `src/rendezvous_mediator.rs` |
| D — Segurança | T3.x | `src/client.rs` (handshake), `src/server/connection.rs` |
| E — UI e marca | T5.x | `flutter/lib/desktop/pages`, `flutter/lib/common.dart`, `res/` |

C e D tocam `src/client.rs`: combinem a ordem (C primeiro em `connect`, D em `secure_connection`).

---

## Decisões (dono do produto)

Decisões de escopo já tomadas. Nova dúvida de escopo: adicione aqui como D10, D11... e espere a resposta.

- [x] **D1** Transferência de arquivos → **manter** (decidido em 22/10/2026).
- [x] **D2** Múltiplos monitores (trocar de monitor na sessão) → **manter** (decidido em 22/10/2026).
- [x] **D3** Ações de controle → **manter todas**: Ctrl+Alt+Del, bloquear e reiniciar o PC remoto (decidido em 22/10/2026).
- [x] **D4** Modo privacidade (tela preta no remoto) e bloquear entrada do usuário remoto → **remover** (decidido em 22/10/2026).
- [x] **D5** Aceite → **basta a senha**, com janela de aviso "sendo acessado" sempre visível no PC remoto (decidido em 22/10/2026).
- [x] **D6** Senha → **as duas**: temporária (muda a cada execução) e fixa opcional nas configurações (decidido em 22/10/2026).
- [x] **D7** Elevação → **não forçar**: botão opcional "Executar como administrador" (sem admin, a sessão não controla janelas elevadas/UAC nem a tela de login) (decidido em 22/10/2026).
- [x] **D8** Idiomas → **pt-BR + inglês**; os demais arquivos de `src/lang` saem (decidido em 22/10/2026).
- [x] **D9** Onde salvar a configuração → **`%APPDATA%\IntraDesk`** (decidido em 22/10/2026).

---

## Fase 0 — Preparação (bloqueia todo o resto)

- [x] **T0.1** `libs/hbb_common` obtido de https://github.com/rustdesk/hbb_common, no commit fixado pelo upstream (`5d52530`) (hoje via subtree, ver T0.6).
  Área: `libs/hbb_common`, `.gitmodules` · @dono: claude
- [x] **T0.2** Repositório = fork do RustDesk (https://github.com/YoungC0DE/IntraDesk), branch `master`, histórico do upstream preservado. `.gitignore` deixou de ignorar `*png`, `*svg`, `*jpg` (escondia os ícones do projeto).
  dep: T0.1 · Área: raiz · @dono: claude
- [x] **T0.3** Ambiente de build: `scripts/setup-dev.ps1` + `scripts/build.ps1`. Build local completo validado em 21/10/2026 (`dist\IntraDesk-dev.exe`, ~25 MB). Ajustes que foram necessários: fechar o Visual Studio aberto antes do Build Tools; `rustup --no-self-update`; recarregar o PATH; compilar dentro do ambiente do VS 2022 (o VS 2026 sem C++ confundia o bindgen) preservando `VCPKG_ROOT`; `CPATH` com os headers do clang na geração do bridge (senão o ffigen gera `typedef bool`). Pendente: conferir o primeiro run do CI após habilitar o Actions no fork.
  dep: T0.2 · Área: `scripts/`, `README.md` · @dono: claude
- [x] **T0.4** Linha de base dos testes (21/10/2026, notebook de dev, features `hwcodec,vram,flutter`, release):
  - `cargo test --lib`: 217 ok, **1 falha pré-existente**: `platform::windows::installer_handoff::directory::tests::protected_directory_enforces_creator_permissions` (`fs::remove_dir` → "Acesso negado" ao apagar o diretório protegido em `%TEMP%`; depende das permissões da máquina, sem admin). Código do instalador, que sai em T1.10.
  - `cargo test -p clipboard`: 2 ok.
  - `flutter test`: 127 ok, 1 pulado.
  - `cargo test -p hbb_common` (rodado na T2.7): 104 unitários ok; **1 falha pré-existente** no teste de integração `proxy_transport::https_reconnects_for_tls_fallback` (falha igual sem nenhuma mudança nossa).
  dep: T0.3 · Área: este arquivo · @dono: claude
- [x] **T0.5** Teste manual de base: dois PCs Windows na mesma LAN, sem internet, usando o acesso direto por IP do RustDesk original (habilitar "Permitir acesso direto por IP"). Registrar o que funciona e o que falha.
  Resultado (24/10/2026, feito pelo usuário): acesso direto por IP funcionando sem problemas.
  dep: T0.3 · Área: este arquivo · @dono: usuário

- [x] **T0.6** `libs/hbb_common` convertido de submódulo para `git subtree` (base `5d52530`, histórico preservado, `.gitmodules` removido). Pode ser alterado direto neste repositório; não há fork separado.
  dep: T0.2 · Área: `.gitmodules`, `libs/hbb_common` · @dono:

## Fase 1 — Limpeza do projeto

### Plataformas (trilha A)
- [x] **T1.1** Alvos não Windows removidos do Flutter: `flutter/{android,ios,linux,macos}`, `flutter/lib/mobile`, `flutter/lib/web`, `web_model.dart`, `web_video_frame_queue.dart`, scripts `*.sh`; caminhos mobile/web que os usavam (entrada mobile, rotas mobile em `connect()`, prompt de deploy Android, keep-screen-on/diálogo de aceite Android no `server_model`, upload web no gerenciador de arquivos, imports condicionais web); item "Servidor ID/Relay" das configurações de rede (o diálogo ficava em `mobile/widgets/dialog.dart`). Pacotes só mobile saíram do `pubspec.yaml`; `pubspec.lock` regenerado com o Flutter 3.24.5 fixado (o lock do upstream vinha de um Flutter mais novo, por isso alguns pacotes "voltam" de versão). Testes de widgets mobile apagados (2).
  Testes: `flutter analyze` sem erros; `flutter test` 100 ok, 1 pulado.
  Obs.: depois de apagar as pastas, apague também sobras locais não versionadas (`flutter/{android,ios,linux,macos}` recriadas pelo `pub get` se a pasta existir).
  dep: T0.4 · Área: `flutter/` (exceto `flutter/lib/desktop`, `flutter/lib/common`) · @dono: claude
- [x] **T1.16** Ramos de plataforma do Dart dobrados para Windows: as flags (`isWeb`, `isAndroid`, `isIOS`, `isMobile`, `isMacOS`, `isLinux`, `isDesktop`, `isWebDesktop`...) viraram constantes e uma ferramenta descartável (pacote `analyzer`, fora do repositório) simplificou `if`/`else`, `if` em listas de widgets, `?:` e `&&`/`||`/`!` que as usavam (inclusive `final x = true/false` locais derivados delas); depois, guiado pelo `flutter analyze`, saíram o código morto após `return`, imports, campos e variáveis que ficaram sem uso (ex.: canais macOS de mouse relativo/botões laterais, área de trabalho Wayland/X11, seletor de documentos Android). `lib/native/common.dart` removido; só `isWindows` continua definido (há usos e variáveis locais homônimas que falam do SO do peer — a ferramenta não mexe em nomes declarados localmente no arquivo). ~45 arquivos, cerca de −2,3 mil linhas.
  Ficou: funções que viraram stubs sem efeito (ex.: `releaseMacOSInputForTabTransfer`, `RelativeMouseModel.initHostChannel`, `_updateLinuxWorkAreaCache(isWayland: false, isX11: false)`), parâmetros que só recebem constantes (`isDesktop: true`, `isMobile: false`) e helpers usados só pelo mobile (`AndroidPermissionManager`, `draggablePositions`...). Dá para limpar por arquivo, à medida que as telas forem reescritas (T4.x/T5.x).
  Testes: `flutter analyze` sem erros nem avisos; `flutter test` 97 ok, 1 pulado.
  dep: T1.1 · Área: `flutter/lib` · @dono: claude
- [x] **T1.2** Empacotamento e docs de outras plataformas removidos: `flatpak/`, `appimage/`, `fastlane/`, `Dockerfile`, `entrypoint.sh`, `res/{DEBIAN,PKGBUILD,rpm*.spec,*.desktop,rustdesk.service,osx-dist.sh,fdroid,mac-*,pacman_install,bump.sh}`, `[package.metadata.bundle]` (cargo-bundle Linux/macOS) e as traduções de `docs/` (README, CODE_OF_CONDUCT, CONTRIBUTING, SECURITY). Também saíram `res/update/` (metadados de atualização, sobra da T1.10) e os scripts Python da API do RustDesk Server Pro em `res/` (`ab.py`, `devices.py`, `audits.py`... sobra da T1.9). `build.py` reescrito só para Windows (1164 → ~150 linhas; mantém `--flutter --portable --hwcodec --vram --skip-portable-pack --print-features`).
  Ficou para T1.4: `res/vcpkg-triplets` (triplets Android, referenciados no `vcpkg.json`).
  Checagem: `build.py --print-features` e `cargo metadata` ok.
  dep: T0.4 · Área: essas pastas/arquivos, `build.py` · @dono: claude
- [x] **T1.3** CI enxuto: só `.github/workflows/build-windows.yml` (Windows x64: build, .exe portátil como artefato, `cargo test`, `flutter test`) + `bridge.yml` (um job, Flutter 3.22.3, sem caminhos de macOS/iOS). Removidos os demais workflows, `FUNDING.yml`, templates de issue, patches do Flutter 3.44 e script de assinatura macOS. Fora do build por ora: MSI, assinatura, ARM64, driver de impressora, display virtual (usbmmidd) e `RustDeskTempTopMostWindow` (modo privacidade).
  dep: T0.4 · Área: `.github/` · @dono: claude
- [x] **T1.4** Código Rust de outras plataformas removido (arquivos, módulos, features e dependências):
  - `src/platform/{linux,macos,gtk_sudo,mobile_wheel}.rs`, `macos.mm`, `macos/`, `privileges_scripts/`, `android_ifaddrs.c`; `src/server/{wayland,drm_capturer,drm_cursor_theme,uinput,rdp_input,dbus}.rs` + `uinput/`; `src/ipc/{drm,fs}.rs`; `build.rs` só Windows.
  - `libs/libxdo-sys-stub`; backends Linux/macOS de `libs/enigo` (e seu `build.rs`), `libs/clipboard` (`platform/unix`, feature `unix-file-copy-paste`), `libs/scrap` (`android`, `quartz`, `wayland`, `x11`, DRM, mediacodec; cfgs `dxgi/quartz/x11` do lib.rs/common), `libs/base` (`platform/{linux,macos}`, feature `wayland_probe`).
  - Features `drm`, `drm-wake`, `unix-file-copy-paste`, `screencapturekit`, `linux-pkg-config`, `mediacodec`, `scrap/wayland`; dependências só de Linux/macOS/Android nos `Cargo.toml` (raiz e libs), patch `libxdo-sys`, `[build-dependencies] os-version`; `res/vcpkg-triplets` (Android) e o `overlay-triplets` do `vcpkg.json`. `Cargo.lock` −1500 linhas.
  - Os trechos `#[cfg(target_os = "linux"/"macos"/"android"/"ios")]` e `#[cfg(feature = "drm"/...)]` espalhados pelo código **ficaram** (não compilam no Windows, mas são código morto): ver T1.18.
  Checagem: `cargo check --workspace --features flutter,hwcodec,vram` ok (sem avisos novos).
  dep: T1.1 · Área: `src/platform`, `Cargo.toml`, `libs/scrap` (backends não Windows), `libs/enigo`, `libs/clipboard` · @dono: claude
- [x] **T1.18** `cfg` de outras plataformas limpos em `src/` e `libs/` com uma ferramenta descartável (baseada em `syn`, fora do repositório) que avalia cada `#[cfg]`/`#[cfg_attr]` para Windows (`windows`/`dxgi` verdadeiros, demais SO falsos, features removidas falsas, `flutter` verdadeira, o resto desconhecido): itens/statements/campos/braços sempre falsos apagados, atributos sempre verdadeiros removidos, os parciais reescritos com o que sobra. 61 arquivos, −7,8 mil linhas; compilou sem ajustes. Também: `let mut` desnecessários em `main_set_options` e no `tray.rs`.
  Ficou para revisão manual (~40 pontos): `cfg!(...)` usados em expressões (ex.: `shortcuts.rs`, `video_service.rs`, `connection.rs`) e `#[cfg]` dentro de corpos de macro (`tokio::select!`, `cfg_if!`, `lazy_static!`) ou em parâmetros de função; `libs/scrap/build.rs` (ramos `linux-pkg-config`/macOS/Android); expressões de plataforma no `vcpkg.json`; README/exemplos do `libs/enigo` e `libs/clipboard/docs`. Listar com `grep -rn 'target_os = "\(linux\|macos\|android\|ios\)"' src libs`.
  Checagem: `cargo check --workspace --tests --examples` ok.
  dep: T1.4 · Área: `src/`, `libs/` · @dono: claude
- [x] **T1.5** Gravação de tela removida (UI, configurações, permissão "gravar sessão", FFI, `libs/scrap/.../record.rs`, dependência `webm`, upload de gravação, opções e atalho `toggle_recording`). Proto mantido.
  dep: T0.4 · Área: gravação · @dono: claude
- [x] **T1.6** Chat e chamada de voz removidos (menus, `chat_page.dart`, `chat_model.dart`, painéis no gerenciador de conexão, FFI, tratamento no cliente/servidor, atalhos `toggle_chat`/`toggle_voice_call`, pacotes `dash_chat_2` e `draggable_float_widget`, ícones). Áudio normal da sessão e "Dispositivo de entrada de áudio" mantidos (só saiu o override da chamada de voz). Proto mantido.
  Testes: Rust 216 ok + falha conhecida (saiu 1 teste do diretório de gravação); Flutter 123 ok, 1 pulado (saíram 4 testes de gravação/chat).
  dep: T0.4 · Área: chat/voz · @dono: claude
- [x] **T1.7** Ações móveis removidas da barra (menu de ações Android: voltar/início/apps/volume/energia e overlay flutuante). Menu de teclado (modos Map/Translate/Legacy, Ctrl+Alt+Del, bloquear, reiniciar) mantido. Helpers usados por `lib/mobile` ficam para T1.1.
  dep: T0.4 · Área: `remote_toolbar.dart`, `remote_page.dart` · @dono: claude
- [x] **T1.8** Removidos encaminhamento de portas/RDP, terminal (incl. login com usuário do SO do terminal), câmera, quadro branco e impressora remota (`libs/remote_printer`, código XPS, passos de instalador). Pacotes Dart `xterm`/`google_fonts` e crate `nokhwa` saíram. O servidor agora **recusa** pedidos de câmera/terminal/port-forward com "Connection not allowed" (antes cairiam como sessão de controle remoto). Contagem de falhas de senha/2FA revisada: equivalente à anterior.
  Testes: Rust 185 ok + falha conhecida (−31, todos de port-forward/mux/terminal/login do terminal); Flutter 101 ok, 1 pulado (−22, terminal).
  dep: T0.4 · Área: os recursos citados · @dono: claude
- [x] **T1.9** Conta e integrações com servidor removidas:
  - Rust: `src/hbbs_http*` (login/OIDC, sync com o servidor e "fechar pelo console web", cliente HTTP), `src/custom_server.rs` + bin `naming` (servidor/chave/API embutidos no nome do .exe, `--config`, licença no "Sobre"), `--assign`/`--deploy` (CLI que registrava o dispositivo no servidor), `deploy_device`, requisições HTTP genéricas e o proxy TCP de requisições HTTP via rendezvous (`common.rs`), **auditoria** do servidor (conexão/arquivo/alarmes em `server/connection.rs`, nota de fim de sessão em `ui_session_interface.rs`), **bot do Telegram do 2FA** (mandava o código por `api.telegram.org`; o TOTP local continua), senha vinda do catálogo pessoal (`client.rs`), `is_pro`.
  - Flutter: telas e modelos de login, catálogo (address book), grupos, OIDC, aviso de API server, cliente HTTP (`utils/http_service.dart`, `common/hbbs/hbbs.dart`), aba "Conta" e opção "nota ao fim da conexão" nas configurações, item "Nota" na barra da sessão, diálogos de nota/auditoria, import/export de configuração de servidor (incl. link `rustdesk://config`), bot do Telegram nas configurações de segurança, abas "Catálogo"/"Dispositivos acessíveis" da tela inicial (ficam Recentes, Favoritos, Descobertos), tags/notas/senha compartilhada nos cards, fontes de ícone AddressBook/DeviceGroup.
  - FFIs correspondentes removidas (bridge regenerado).
  Testes: `cargo test --lib` 163 ok, 0 falhas (−8, todos de HTTP/API server e `hbbs_http`); `flutter analyze` sem erros; `flutter test` 97 ok, 1 pulado (−3: `api_server_warning_test`; `autocomplete_peer_merge_test` adaptado para Descobertos + Recentes).
  dep: T0.4 · Área: `flutter/lib/common/widgets/{login,address_book,my_group,oidc_auth_status}.dart`, `src/hbbs_http*` · @dono: claude
- [x] **T1.17** Sobras de conta/servidor removidas: `Ab`/`Group` (e tipos, armazenamento e desserializadores), `is_disable_ab`/`is_disable_account` e `EXE_RENDEZVOUS_SERVER` no `hbb_common`; senha compartilhada do catálogo (`shared_password` em `client.rs`, parâmetro `is_shared_password` de `session_add`/`session_add_sync` e `isSharedPassword` em todo o Dart de abertura de sessão/janelas); opções sem efeito `allow-scope-violation-alarm`, `enable-flutter-http-on-rust`, `allow-https-21114`, `disable-group-panel`; parâmetro `onSubmit` de `msgBox`.
  Ficou: `PasswordSource::SharedAb` (ainda usado pela senha padrão `default-connect-password`); `access_token` passado ao `io_loop`/rendezvous e `secure_tcp_impl(.., log_on_success)` (código do rendezvous, sai com T2.1/T2.3); itens de rede (proxy, WebSocket, TLS, UDP) para T2.1/T5.3; `hide-help-cards` continua (ainda esconde cartões de ajuda).
  Checagem: `cargo check --workspace --tests`, `flutter analyze` ok; `flutter test` 97 ok.
  dep: T1.9 · Área: os arquivos citados · @dono: claude
- [x] **T1.10** Atualização automática e instalação removidas: `src/updater*`, `src/manual_update.rs`, `src/update_*`, `src/hbbs_http/downloader.rs`, `res/msi`, código de instalador/MSI/atualização verificada em `src/platform/windows*` (`installer_*`, `msi_registry`, `verified_update`), argumentos `--install`/`--update`/`--silent-install`/`--uninstall`/`--after-install`/`--before-uninstall`/`--install-service`/`--uninstall-service`/`--uninstall-cert`/`--noinstall`, "Parar serviço" na bandeja, `rustdesk_is_disable_installation` (runner) e opção `disable-installation`; no `libs/portable`, o modo "clique para instalar" (`*install.exe`) e a limpeza de update verificado. Flutter: `install_page.dart`, `update_progress.dart`, cartões "Instalar"/"Nova versão"/"Atualizar", opções "Verificar atualização ao iniciar", "Atualização automática" (chaves `enable-check-update`/`allow-auto-update`) e "Compartilhar sessão RDP"; FFIs correspondentes. IPC `ControllingSessionCount`/`HasNoActiveConns` (só o updater usava) saiu.
  Corrigido de passagem: `flutter/test/cm_demo.dart` não compilava desde a T1.8 (construtor de `Client`).
  Testes: `cargo test --lib` 171 ok, **0 falhas** (−14, todos de updater/instalador, incluindo a falha conhecida da linha de base); `flutter analyze` sem erros.
  dep: T0.4 · Área: esses arquivos · @dono: claude
- [x] **T1.15** Sobras de instalação/serviço removidas: `is_installed()` (sempre falso no portátil) e os ramos que dependiam dele; registro de instalação (`get_install_info`, `get_reg`, `is_cur_exe_the_installed`, `share_rdp` do registro — sem instalação ele valia "ligado", mantido assim); serviço do Windows (`start_os_service`/`run_service`/`launch_server`, bin `service` do macOS, dependência `windows-service`); argumentos `--service` e `--server` (só o serviço lançava o `--server`) e os comandos que exigiam instalação (`--password`, `--set-unlock-pin`, `--set-id`, `--option`); início automático da bandeja do serviço; seleção de sessão RDP (só servidor instalado oferecia); config de hwcodec vinda do serviço; `PortableState.is_installed`; `bind.mainIsInstalled()` no Flutter (cartão do daemon macOS, opções "só com janela aberta"/PIN de desbloqueio; o cadeado das configurações ficou `locked = false` até a T5.3 reescrever a página).
  **Mudança de comportamento:** a escuta da descoberta LAN (`lan::start_listening`) agora sempre inicia (antes só se instalado; o portátil não respondia aos broadcasts).
  Ficou: `--tray`/`tray.rs` (base para a T6.2); `is_server()` (sempre falso agora, ramos mortos); `--portable-service`/elevação (D7, T6.3); tela virtual (ver T1.19).
  Checagem: `cargo check --workspace --tests` e `flutter analyze` ok.
  dep: T1.10 · Área: os arquivos citados · @dono: claude
- [x] **T1.19** Tela virtual (driver IDD/Amyuni) removida: `src/virtual_display_manager.rs`, `libs/virtual_display` (+ `dylib`, build e cópia da DLL no `build.py`), `--install-idd`/`--uninstall-amyuni-idd`, tratamento de `ToggleVirtualDisplay` no servidor (o proto fica; a mensagem passa a ser ignorada), resolução 0x0 de tela virtual, envio automático de "ligar tela virtual" pelo cliente, FFI `session_toggle_virtual_display`, menu "Tela virtual" e resolução personalizada (só existia para tela virtual) na barra da sessão, atalho "Desconectar todas", `is_self_service_running`. Também: +19 chaves de tradução que ficaram sem uso desde a T1.13.
  Checagem: `cargo check --workspace --tests`, `flutter analyze` ok; `flutter test` 97 ok.
  dep: T1.15 · Área: os arquivos citados · @dono: claude
- [x] **T1.11** UI Sciter removida: `src/ui/`, `src/ui.rs`, `src/platform/delegate.rs` (macOS/Sciter), `res/inline-sciter.py`, dependência `sciter-rs`, feature `inline` e os 73 trechos `cfg(not(feature = "flutter"))`. `flutter` virou feature padrão (`cargo build` sem flags já compila a UI certa). ~11,5 mil linhas a menos. Testes iguais à linha de base (217 ok + a falha conhecida); `build.ps1` ok.
  dep: T0.4 · Área: `src/ui*` · @dono: claude
- [x] **T1.14** Sobras do Sciter, do chat e da T1.8 removidas: `CHILDREN`/`Children` e o `Config(("id", None))` periódico (`ui_interface.rs`); `ClientClipboardContext` e o parâmetro de `try_start_clipboard`; `#[allow(unused_mut)]` do `portable_service.rs`; build-deps `winres`/`winapi` e `[package.metadata.winres]` (o .exe usa o `Runner.rc`); flag `--flutter` do `build.py` (e do `build.ps1`, CI e `CLAUDE.md`); `Data::ConfirmDeleteFiles`/`SetNoConfirm` (ninguém enviava) com `RemoveJob.no_confirm` e `RemoveJobMeta`; métodos de UI do Sciter vazios no Flutter (`set_display`, `close_success`, `clear_all_jobs`, `update_transfer_list`, `confirm_delete_files`, `adapt_size`); comentários que citavam o Sciter no nosso código (ficaram os que falam de peers antigos remotos); nomes herdados do chat no Flutter (`unreadFileTransferLogCount`, `kConnectionManagerWindowSize{Closed,Open}SidePage`, `realSidePageWidth`); import `bail` do `ui_cm_interface.rs`; checagens sempre verdadeiras de `VideoSource::is_monitor()` (e o campo `source` do `StaticRefresh`); campos `terminal_persistent`/`port_forwards`/`privacy_mode` do `PeerConfig`.
  Corrigido de passagem: `cargo test` não compilava desde a T1.4 (`mobile_wheel.rs` era incluído por `#[path]` em `ui_session_interface.rs`).
  Mantidos: `JobType::Printer`/`DataSource::MemoryCursor`/`get_buf_data` (o tipo vem do proto e o servidor precisa continuar recusando pedidos de impressora; dá para trocar por uma recusa direta no proto, junto com T1.17) e o enum `VideoSource` de variante única (~60 pontos de encanamento sem efeito; refatoração sem ganho funcional).
  Checagem: `cargo check --workspace --tests` e `flutter analyze` ok.
  dep: T1.11 · Área: os arquivos citados · @dono: claude
- [x] **T1.12** Remover o modo privacidade (`src/privacy_mode*`, `RuntimeBroker_intradesk.exe`/`WindowInjection.dll`, opção na barra e nas configurações) e o "bloquear entrada do usuário" (D4).
  dep: D4 · Área: `src/privacy_mode*`, `src/server/connection.rs` (handlers), `remote_toolbar.dart`, `libs/portable` (cópia do RuntimeBroker) · @dono: claude
- [x] **T1.13** Traduções: só `en.rs`, `pt_BR.rs` e `template.rs` em `src/lang` (51 idiomas removidos); `LANGS` com en/pt-BR; qualquer idioma/locale português → pt-BR, o resto → inglês (idiomas salvos por versões antigas caem no inglês); `supportedLocales` do Flutter = en-US/pt-BR; lógica de CJK/pt-PT removida. 261 chaves sem uso apagadas (814 → 553): busca conservadora do texto da chave em `src/`, `libs/` e `flutter/lib` (chaves com `{}` como padrão, inclusive interpolação Dart). Teste novo `test_resolve_lang_maps_portuguese_to_pt_br_and_rest_to_english`.
  Checagem: `cargo check` e `flutter analyze` ok.
  dep: D8, T1.5–T1.12 · Área: `src/lang/`, `src/lang.rs` · @dono: claude

**Fim da Fase 1 (24/10/2026):** `cargo test --lib --features flutter,hwcodec,vram` 160 ok, 0 falhas; `flutter test` 97 ok, 1 pulado. (O `cargo test` precisa do ambiente do VS 2022 carregado, como no `scripts/build.ps1`; fora dele o bindgen do `kcp` não acha `stddef.h`.)

## Fase 2 — Funcionamento só na LAN (trilha C)

- [x] **T2.1** Servidor rendezvous/relay desligado no lado servidor: `src/rendezvous_mediator.rs` apagado (registro de ID/chave, hole punching UDP/TCP/IPv6, relay, WebRTC answerer, "intranet"); o servidor agora só sobe a escuta direta (`direct_server`, movida para `src/server.rs`), a descoberta LAN e o teste de AV1. Sem servidores padrão (`RENDEZVOUS_SERVERS` saiu; `get_rendezvous_servers` vazio). Saíram também os testes que contatavam servidores: tipo de NAT (`test_nat_type`, `CheckTestNatType`), latência dos servidores (`test_rendezvous_server`/`refresh_rendezvous_server`, IPC `TestRendezvousServer`/`Deployed`), "Alterar ID" (registrava o ID no servidor: FFI `main_change_id`/`main_get_async_status`, diálogo e cartão nas configurações, opção `disable-change-id`, `is_valid_custom_id`, 9 chaves de tradução), `version_check_request` (`api.rustdesk.com`, sem uso), bin `src/main.rs` (só testava rendezvous/NAT; o build usa `--lib`). Mudar opções não reinicia mais o mediador (`CheckIfRestart` só trata cache TLS e entrada de áudio).
  Ficou para a T2.3 (lado cliente): conexão por ID via rendezvous, KCP/hole punching (`kcp_stream.rs`; `KcpStream::accept` fica sem uso fora dos testes até lá), teste IPv6, `get_nat_type`, `PUBLIC_SERVER`, getters de servidor no `Config`. Para a T2.9: proxy/WebSocket/TLS/UDP. `ConnectionMeta` agora é sempre vazio (vinha do rendezvous); dá para remover junto com a T3.x em `connection.rs`.
  Aceite com Wireshark: fica para a T7.4 (depende também da T2.3).
  Checagem: `cargo check --workspace --lib --tests`, `flutter analyze` ok.
  dep: T0.5, T0.6 · Área: `src/rendezvous_mediator.rs`, `src/server.rs`, `libs/hbb_common/src/config.rs` · @dono: claude
- [x] **T2.2** Acesso direto por IP sempre ligado (porta TCP 21118, configurável em `direct-access-port`), sem opção para desligar: saiu a opção `direct-server` (Rust, `option2bool`/`bool2option`, chave de tradução "Enable direct IP access"); nas configurações fica só o campo da porta. Só "parar serviço" (`stop-service`) fecha a escuta. Saiu também o diálogo `changeDirectAccessPort` (sem uso desde a T1.1).
  Checagem: `cargo check --workspace --lib --tests`, `flutter analyze` ok.
  dep: T2.1, T0.6 · Área: `libs/hbb_common/src/config.rs`, `src/server.rs` · @dono: claude
- [x] **T2.3** Conexão de saída sempre direta. `Client::start` agora só abre TCP: se o destino é IP ou `host:porta`, disca nele (porta 21118 por padrão); se é um ID, disca nos IPs que a descoberta LAN registrou para ele (`LanPeers`). Timeout de 3 s por endereço; sem endereço que responda → "Computer not found on the network" / "Computador não encontrado na rede" (chave nova). Saíram: rendezvous do lado cliente, hole punching UDP/TCP/IPv6, KCP (`src/kcp_stream.rs`, deps `kcp-sys` e `stunclient`), corrida/oferta WebRTC e ICE, relay, health-check do rendezvous (`hc_connection`), detecção de queda por KCP/WebRTC no `io_loop`, parâmetros `key`/`token` (access token) do `io_loop`, tipo de NAT (`get_nat_type`, IPC `NatType`, `Config2.nat_type`), `secure_tcp*` (troca de chaves com o rendezvous, e seus testes), `using_public_server` (FFI `main_is_using_public_server`; limites de qualidade/FPS "fora do direto", link "configurar servidor" da tela inicial e opções de rede que só apareciam com servidor próprio), 5 chaves de tradução. Testes novos `peer_addrs_tests`.
  `secure_connection` (metade cliente do handshake) ficou, sem uso, para a T3.1.
  Checagem: `cargo check --workspace --lib --tests`, `flutter analyze` ok; `cargo test --lib client::` 20 ok.
  Teste manual (24/10/2026, feito pelo usuário com o build da T2.3): conexão entre dois PCs na LAN funcionando.
  dep: T2.1 · Área: `src/client.rs` (`connect`, `_start`) · @dono: claude
- [x] **T2.4** Descoberta automática: a tela inicial liga uma busca periódica na LAN (`lan::start_discovery`, uma vez por processo): uma rodada ao abrir e a cada 10 s; "atualizar"/abrir a aba pede uma rodada na hora (no máximo uma a cada 3 s). Cada peer guarda `last_seen` (campo novo em `DiscoveryPeer`): fica "offline" depois de 25 s sem responder (duas rodadas, para não piscar com um broadcast perdido) e sai da lista depois de 7 dias. O listener já estava sempre ativo desde a T1.15.
  Status online de Recentes/Favoritos/autocompletar agora vem da descoberta: o FFI `query_onlines` responde na hora a partir do `LanPeers` (casa pelo ID ou pelo IP/`ip:porta` digitado com os IPs de onde os peers responderam). Saíram a consulta ao rendezvous (`client::peer_online`), o runner `flutter::async_tasks`, `get_rendezvous_server` (`common.rs` e `ipc.rs`, e a chave IPC `rendezvous_server`) e `get_next_nonkeyexchange_msg`. Teste novo `lan::tests::split_by_online_matches_ids_and_addresses_of_online_peers`.
  Checagem: `cargo check --workspace --lib --tests`, `flutter analyze` ok; `cargo test --lib lan::` 1 ok.
  dep: T2.1 · Área: `src/lan.rs`, `src/flutter_ffi.rs` (se precisar expor algo), `src/client.rs` (`peer_online`), `src/flutter.rs`, `flutter/lib/common/widgets/{peers_view,autocomplete}.dart` · @dono: claude
- [x] **T2.5** Pacote de descoberta identificado: `PeerDiscovery` ganhou `app` (= `IntraDesk`) e `version` (proto, campos 8 e 9). O listener só responde a pings com `app = IntraDesk` e a busca ignora pongs sem ele (RustDesk original e builds do IntraDesk anteriores a esta tarefa ficam invisíveis). A resposta leva nome do computador, usuário logado e versão; o IP é o de origem do pacote. `DiscoveryPeer` guarda `version`, e `get_lan_peers` passa a enviar `ip` e `version` ao Flutter (o modelo `Peer` do Dart ainda não lê esses campos: fica para a T5.2, que desenha os cards). Teste novo `lan::tests::discovery_packets_from_other_apps_are_ignored`.
  Checagem: `cargo test --lib lan::` 2 ok.
  dep: T2.4, T0.6 · Área: `src/lan.rs`, `libs/hbb_common/protos` · @dono: claude
- [ ] **T2.6** Remover o indicador "Pronto / Conectando à rede" da tela inicial (depende do rendezvous) e trocar por "Visível na rede: IP x.x.x.x".
  dep: T2.1 · Área: `desktop_home_page.dart` · @dono:
- [x] **T2.7** App renomeado no núcleo: `APP_NAME` = `IntraDesk` (config em `%APPDATA%\IntraDesk`, logs, pipe `\\.\pipe\IntraDesk\query`, memória compartilhada e alvo dos `kill` passam a usar esse nome), executável `IntraDesk.exe` (extração do portátil em `%LOCALAPPDATA%\intradesk`) e `RuntimeBroker_intradesk.exe`. Efeitos de "cliente customizado" (nome ≠ RustDesk): sem checagem de atualização do RustDesk, textos da UI trocam RustDesk → IntraDesk, aparece "Powered by RustDesk".
  Aceite validado em 22/10/2026 no notebook de dev com o RustDesk portátil aberto e em uso: abrir e fechar o IntraDesk não alterou processos, config (hash dos 4 arquivos), pasta nem pipes do RustDesk; a sessão remota seguiu funcionando.
  Limite conhecido: portas 21118/21119 continuam iguais às do RustDesk; se os dois escutarem ao mesmo tempo, só um consegue (o RustDesk do notebook hoje não escuta nelas).
  Build local: depois de renomear o executável, apague `flutter\build\windows` (o cache do CMake guarda o alvo antigo).
  dep: T0.4, T0.6, D9 · Área: `libs/hbb_common/src/config.rs`, `libs/portable`, `flutter/windows/CMakeLists.txt`, `build.py`, `scripts/build.ps1`, CI · @dono: claude
- [ ] **T2.8** Firewall: detectar se as portas estão bloqueadas e mostrar aviso com botão "Liberar no firewall" (pede elevação e cria a regra com `netsh` só para redes privadas).
  dep: T2.2 · Área: `src/platform/windows.rs` (função nova), tela inicial · @dono:
- [ ] **T2.9** Remover opções de rede que só servem para sair da LAN: proxy (socks/http), WebSocket, fallback TLS inseguro e "desativar UDP" (`libs/hbb_common/src/{socket_client,proxy,websocket,tls}.rs`, `src/ipc.rs` `Socks`/`SocksWs`, configurações de rede no Flutter). Também: o transporte WebRTC no `hbb_common` (feature `webrtc`, `webrtc.rs`, variante em `stream.rs`, patches `webrtc*` no `Cargo.toml`), já sem uso desde a T2.3, e a dependência `reqwest` da raiz (sem uso desde a T1.9).
  dep: T2.3 · Área: os arquivos citados · @dono:
- [ ] **T2.10** Remover de ponta a ponta "forçar relay" e IDs de outro servidor, sem efeito desde a T2.3: `forceRelay` na abertura de sessão/janelas no Dart e no FFI (`session_add`, `session_reconnect`), opção `force-always-relay` (menu do card), diálogo "tentar via relay" (`relay-hint`, `relay_hint_tip`), sufixo `/r` (`handle_relay_id`), `id@servidor?key=` (`other_server`, `PUBLIC_SERVER`, `other-server-key`), `peer_relay`/`policy_relay`/`force_relay` e `is_force_relay`/`is_policy_relay` no `LoginConfigHandler`/`Interface`, `direct_failures` (`set_direct_failure`), `Config::get_rendezvous_server(s)`/`PROD_RENDEZVOUS_SERVER`/`Config2.rendezvous_server`/opção `custom-rendezvous-server`. O indicador de conexão "direta/relay" da sessão vira constante.
  dep: T2.3 · Área: `src/client.rs` (`LoginConfigHandler`), `src/flutter_ffi.rs`, `src/flutter.rs`, `flutter/lib/common.dart`, `flutter/lib/models/model.dart`, páginas de sessão · @dono:

## Fase 3 — Segurança (trilha D) — obrigatória antes de qualquer release

- [ ] **T3.1** Criptografia da sessão sem servidor. Hoje, sem a chave do rendezvous, `secure_connection` (`src/client.rs`, ramo `sign_pk == None`) segue em **texto puro**. Implementar troca de chaves direta entre os peers (o servidor já tem par de chaves próprio) com fixação da chave no primeiro contato (TOFU) e aviso se ela mudar.
  dep: T2.3 · Área: `src/client.rs` (`secure_connection`), `src/server/connection.rs` (lado servidor do handshake) · @dono:
- [ ] **T3.2** Senha obrigatória: temporária gerada a cada execução e exibida na tela inicial; senha fixa opcional (D6). Remover qualquer caminho que conecte sem senha.
  dep: D6, T2.2 · Área: `src/server/connection.rs`, `src/ui_interface.rs`, tela inicial · @dono:
- [ ] **T3.3** Proteção contra força bruta: bloqueio temporário por IP após N senhas erradas (verificar e reaproveitar o que já existe em `connection.rs`).
  dep: T3.2 · Área: `src/server/connection.rs` · @dono:
- [ ] **T3.4** Aviso visível no computador acessado enquanto houver sessão ativa (janela do gerenciador de conexão com botão "Desconectar") e comportamento de aceite conforme D5.
  dep: D5 · Área: `flutter/lib/desktop/pages/server_page.dart` · @dono:
- [ ] **T3.5** Aceitar conexões só de IPs privados (RFC 1918, link-local); recusar IP público mesmo com a porta exposta.
  dep: T2.2 · Área: `src/server.rs` / `src/server/connection.rs` · @dono:

## Fase 4 — Sessão remota

- [ ] **T4.1** Barra da sessão só com: tela cheia, área de transferência (ligar/desligar), áudio (ligar/desligar), escala (original / ajustar / personalizada), qualidade (ótima / balanceada / velocidade / personalizada), resolução, transferência de arquivos (D1), troca de monitor (D2), Ctrl+Alt+Del / bloquear / reiniciar o PC remoto (D3) e desconectar. Remover também o botão/atalho/opção "Mostrar meu cursor": ficou sem efeito depois que o quadro branco (que desenhava o cursor) saiu na T1.8.
  dep: T1.5–T1.8 · Área: `flutter/lib/desktop/widgets/remote_toolbar.dart` · @dono:
- [ ] **T4.2** Validar cada item da barra entre dois PCs Windows (ver T7.3) e corrigir o que quebrou com a limpeza.
  dep: T4.1, T3.1 · Área: conforme bug · @dono:
- [ ] **T4.3** Área de transferência: garantir texto, imagem e arquivos (D1) nos dois sentidos.
  dep: T4.1 · Área: `src/clipboard.rs`, `libs/clipboard` · @dono:
- [ ] **T4.4** Sessões em abas: manter (como AnyDesk) ou limitar a uma sessão por janela, sem os menus de abas que dependiam de features removidas.
  dep: T4.1 · Área: `remote_tab_page.dart`, `tabbar_widget.dart` · @dono:
- [ ] **T4.5** Bug relatado: depois de muito tempo de sessão, ao abrir um CSV grande no PC acessado, **a imagem congela** mas o áudio continua e o mouse mexe.
  Evidências (logs do RustDesk 1.5.1 portátil no notebook, out/2026):
  - Encoder em uso: **Intel Quick Sync H.265 por hardware** (`hevc_qsv`, caminho VRAM), tela 1366x768, ~1 GB de RAM livre.
  - Em todas as sessões com o problema: `[FFMPEG_VRAM_ENC] avcodec_send_frame failed ... Invalid FrameType:0` → `encode fail: no valid frame, times: 1`, e **nenhum log de vídeo depois disso** até o usuário fechar a sessão 10–30 s depois (ex.: 08/10 19:20:50 → 19:21:19; 13/10 11:56:20 → 11:56:29).
  - O contador de falhas nunca chega a 3, então o fallback existente (`video_service.rs`, `bail!("SWITCH")` após 3 falhas) não dispara: o loop de vídeo para de entregar quadros (provável travamento na captura DXGI/encoder QSV sob pressão de memória), enquanto áudio e cursor seguem em serviços separados (o cursor é desenhado no cliente).
  - Não é criptografia nem a rota pela internet: o áudio continua passando pela mesma conexão criptografada. Rodar só na LAN **não resolve sozinho**.
  Plano: (1) reproduzir no IntraDesk entre dois PCs (CSV grande no Excel, RAM baixa); (2) watchdog no serviço de vídeo: se a sessão está ativa e nenhum quadro é enviado por N segundos, registrar e reiniciar o serviço com encoder por software (VP9); (3) avaliar VP9 por software como padrão no IntraDesk, já que na LAN a banda sobra e evita bugs de driver de GPU.
  Teste rápido hoje, no RustDesk atual: no notebook, desativar "Habilitar codificação por hardware" (opção `enable-hwcodec`) ou escolher VP9 no menu de exibição da sessão; se o congelamento sumir, a causa está confirmada.
  dep: T0.3 · Área: `src/server/video_service.rs`, `libs/scrap/src/common/{codec,hwcodec,vram}.rs` · @dono:

## Fase 5 — Interface e marca (trilha E)

- [ ] **T5.1** Paleta laranja escuro em `MyTheme` (`flutter/lib/common.dart`) com os tokens do `CLAUDE.md`, temas claro e escuro. Trocar todas as cores azuis hardcoded (`0xFF0071FF`, `Colors.blue` etc.) pelos tokens.
  dep: T0.4 · Área: `flutter/lib/common.dart` + buscas por cores em `flutter/lib` · @dono:
- [ ] **T5.2** Nova tela inicial no estilo AnyDesk:
  - painel "Este computador": nome, IP, senha temporária (copiar / gerar nova);
  - campo "Conectar a" (IP ou nome) com botão Conectar;
  - grade de "Computadores na rede" (cards com nome, usuário, IP, status) com busca;
  - abas "Recentes" e "Favoritos"; remover abas de address book/grupos.
  dep: T2.4, T5.1 · Área: `desktop_home_page.dart`, `connection_page.dart`, `flutter/lib/common/widgets/peer_*.dart`, `peers_view.dart` · @dono:
- [ ] **T5.3** Configurações enxutas: Geral (idioma, tema), Segurança (senha fixa, aceite), Rede (portas, firewall), Exibição (qualidade/escala padrão), Áudio, Sobre (licença AGPL e link do código-fonte). Remover as outras abas.
  dep: T1.9, T1.10 · Área: `desktop_setting_page.dart` · @dono:
- [ ] **T5.4** Marca: nome "IntraDesk" em títulos, bandeja, janela, "Sobre"; novos ícones (`res/icon.ico`, `res/tray-icon.ico`, `res/logo*.svg`, `flutter/windows/runner/resources/app_icon.ico`, `flutter/assets`); metadados do .exe (`flutter/windows/runner/Runner.rc`, `Cargo.toml` `[package.metadata.winres]`).
  dep: T0.4 · Área: `res/`, `flutter/windows/runner`, `flutter/assets` · @dono:
- [ ] **T5.5** Textos da UI revisados em pt-BR (sem menção a ID, servidor ou relay).
  dep: T5.2, T5.3, T1.13 · Área: `src/lang/pt_BR.rs`, `src/lang/en.rs` · @dono:

## Fase 6 — Executável portátil

- [ ] **T6.1** Ajustar `build.py`/`libs/portable` para gerar `IntraDesk.exe` único (autoextraível) sem passos de instalador; extrair em pasta temporária do usuário e limpar ao sair.
  dep: T1.10 · Área: `build.py`, `libs/portable` · @dono:
- [ ] **T6.2** Instância única por usuário; ícone na bandeja com "Abrir", "Copiar senha", "Sair".
  dep: T2.7 · Área: `src/tray.rs`, `src/core_main.rs` · @dono:
- [ ] **T6.3** Executar como administrador (conforme D7): opção na UI que reabre o .exe elevado.
  dep: D7 · Área: `src/platform/windows.rs` (função nova), tela inicial · @dono:
- [ ] **T6.4** Verificar dependências de runtime: o .exe roda num Windows 10 limpo (sem VC++ Redistributable instalado, sem Flutter). Embutir DLLs necessárias.
  dep: T6.1 · Área: `build.py`, `libs/portable` · @dono:
- [ ] **T6.5** Manifesto do Windows: `supportedOS` só Windows 10+, DPI awareness per-monitor v2, `asInvoker` por padrão.
  dep: T6.1 · Área: `res/manifest.xml`, `flutter/windows/runner/runner.exe.manifest` · @dono:

## Fase 7 — Testes

- [ ] **T7.1** Testes unitários Rust: parse/serialização do pacote de descoberta (T2.5), filtro de IP privado (T3.5), TOFU de chaves (T3.1: chave nova aceita, chave igual aceita, chave diferente rejeitada).
  dep: T2.5, T3.1, T3.5 · Área: módulos `#[cfg(test)]` ao lado do código · @dono:
- [ ] **T7.2** Testes Flutter (`flutter/test/`): tela inicial lista os peers recebidos; barra da sessão mostra só os itens permitidos.
  dep: T4.1, T5.2 · Área: `flutter/test/` · @dono:
- [ ] **T7.3** Roteiro de teste manual (`docs/teste-manual.md`) entre dois PCs Windows 10 e 11, **sem internet**:
  descoberta automática, conexão por clique e por IP, senha errada/certa, tela cheia, clipboard nos dois sentidos, áudio liga/desliga, escala, qualidade, resolução, desconectar, PC remoto com 2 monitores, PC remoto com escala de 150%, firewall bloqueado, reconexão após rede cair.
  dep: T4.2 · Área: `docs/` · @dono:
- [ ] **T7.4** Verificação de rede: capturar tráfego (Wireshark) durante uma sessão e confirmar (a) nenhum pacote para fora da LAN, (b) conteúdo da sessão criptografado.
  dep: T2.1, T3.1 · Área: registro neste arquivo · @dono:
- [ ] **T7.5** Desempenho: CPU/banda em 1080p e 4K nas qualidades padrão; comparar com o RustDesk original (T0.5).
  dep: T4.2 · Área: registro neste arquivo · @dono:
- [ ] **T7.6** CI verde no workflow único (T1.3) com todos os testes.
  dep: T1.3, T7.1, T7.2 · Área: `.github/` · @dono:

## Limitações conhecidas (não são tarefas)

- Gerenciador de conexão: com um cliente de transferência de arquivos e outro de controle remoto ao mesmo tempo, abrir o painel lateral e trocar para a aba do controle remoto mostra a área vazia (antes mostrava o chat).

- Descoberta funciona só na mesma sub-rede (broadcast não passa por roteador/VLAN). Em redes segmentadas, conectar digitando o IP.
- Sem instalação/serviço: não é possível acessar a tela de login do Windows nem controlar janelas elevadas sem rodar como administrador (ver D7).
