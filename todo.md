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

## Decisões pendentes (dono do produto)

Não remova nem implemente esses itens antes da decisão. Registre aqui a resposta.

- [ ] **D1** Transferência de arquivos: manter ou remover? *(sugestão: remover na v1)*
- [ ] **D2** Múltiplos monitores (trocar de monitor na sessão): manter? *(sugestão: manter, é necessário quando o PC remoto tem 2 telas)*
- [ ] **D3** Ações de controle: Ctrl+Alt+Del, bloquear, reiniciar o PC remoto: manter? *(sugestão: manter só Ctrl+Alt+Del)*
- [ ] **D4** Modo privacidade (tela preta no remoto) e bloquear entrada do usuário remoto: manter? *(sugestão: remover)*
- [ ] **D5** Aceite manual: o usuário remoto precisa clicar "Aceitar", ou basta a senha? *(sugestão: senha + janela de aviso "sendo acessado" sempre visível no remoto)*
- [ ] **D6** Senha: só temporária (muda a cada execução) ou permitir senha fixa nas configurações? *(sugestão: as duas, como no AnyDesk)*
- [ ] **D7** Elevação: pedir privilégio de administrador ao abrir? Sem admin, a sessão não controla janelas elevadas (UAC) nem a tela de login. *(sugestão: botão "Executar como administrador" opcional, não forçado)*
- [ ] **D8** Idiomas da UI: só pt-BR, ou pt-BR + inglês? *(sugestão: pt-BR + en; remover os demais arquivos de `src/lang`)*
- [ ] **D9** Onde salvar a configuração: `%APPDATA%\IntraDesk` (padrão) ou ao lado do .exe (100% portátil, ex.: pendrive)?

---

## Fase 0 — Preparação (bloqueia todo o resto)

- [x] **T0.1** `libs/hbb_common` como submódulo de https://github.com/rustdesk/hbb_common, no commit fixado pelo upstream (`5d52530`).
  Área: `libs/hbb_common`, `.gitmodules` · @dono: claude
- [x] **T0.2** Repositório = fork do RustDesk (https://github.com/YoungC0DE/IntraDesk), branch `master`, histórico do upstream preservado. `.gitignore` deixou de ignorar `*png`, `*svg`, `*jpg` (escondia os ícones do projeto).
  dep: T0.1 · Área: raiz · @dono: claude
- [~] **T0.3** Ambiente de build. Criados `scripts/setup-dev.ps1` (instala Rust 1.75, Flutter 3.24.5 + engine RustDesk, LLVM 15.0.6, vcpkg, VS Build Tools, Python, ferramentas do bridge) e `scripts/build.ps1` (bridge + build + .exe portátil em `dist/`). Falta: rodar o setup nesta máquina, fazer o primeiro build local e registrar aqui o que precisou de ajuste. Conferir também se o primeiro run do CI (`build-windows.yml`) passou.
  dep: T0.2 · Área: `scripts/`, `README.md` · @dono: claude
- [ ] **T0.4** Rodar a suíte atual (`cargo test --lib`, `flutter test`) e anotar aqui as falhas que já existem, para não serem confundidas com regressões.
  dep: T0.3 · Área: este arquivo · @dono:
- [ ] **T0.5** Teste manual de base: dois PCs Windows na mesma LAN, sem internet, usando o acesso direto por IP do RustDesk original (habilitar "Permitir acesso direto por IP"). Registrar o que funciona e o que falha.
  dep: T0.3 · Área: este arquivo · @dono:

- [ ] **T0.6** Fork do `hbb_common`: antes da primeira tarefa que altera `libs/hbb_common` (T2.1, T2.2, T2.5, T2.7), criar o fork `YoungC0DE/hbb_common`, apontar `.gitmodules` para ele (branch `intradesk`) e atualizar o submódulo. Mudanças no `hbb_common` viram commits no fork + bump do submódulo aqui.
  dep: T0.2 · Área: `.gitmodules`, `libs/hbb_common` · @dono:

## Fase 1 — Limpeza do projeto

### Plataformas (trilha A)
- [ ] **T1.1** Remover alvos não Windows do Flutter: `flutter/{android,ios,linux,macos}`, `flutter/lib/mobile`, `flutter/lib/web`, scripts `flutter/*.sh`, `flutter/build_*.sh`, dependências do `pubspec.yaml` usadas só por eles.
  dep: T0.4 · Área: `flutter/` (exceto `flutter/lib/desktop`, `flutter/lib/common`) · @dono:
- [ ] **T1.2** Remover empacotamento e docs de outras plataformas: `flatpak/`, `appimage/`, `fastlane/`, `res/DEBIAN`, `res/PKGBUILD`, `res/rpm*.spec`, `res/*.desktop`, `res/*.service`, `res/osx-dist.sh`, `res/fdroid`, `res/mac-*`, `Dockerfile`, `entrypoint.sh`, `docs/README-*.md`, `docs/CODE_OF_CONDUCT-*.md`.
  dep: T0.4 · Área: essas pastas/arquivos · @dono:
- [x] **T1.3** CI enxuto: só `.github/workflows/build-windows.yml` (Windows x64: build, .exe portátil como artefato, `cargo test`, `flutter test`) + `bridge.yml` (um job, Flutter 3.22.3, sem caminhos de macOS/iOS). Removidos os demais workflows, `FUNDING.yml`, templates de issue, patches do Flutter 3.44 e script de assinatura macOS. Fora do build por ora: MSI, assinatura, ARM64, driver de impressora, display virtual (usbmmidd) e `RustDeskTempTopMostWindow` (modo privacidade).
  dep: T0.4 · Área: `.github/` · @dono: claude
- [ ] **T1.4** Remover código Rust de outras plataformas: `src/platform/{linux,macos,gtk_sudo,...}`, `libs/libxdo-sys-stub`, features `drm`, `unix-file-copy-paste`, `screencapturekit`, `linux-pkg-config`, `mediacodec`; dependências só de Linux/macOS/Android no `Cargo.toml`. Fazer em passos pequenos, compilando a cada passo.
  dep: T1.1 · Área: `src/platform`, `Cargo.toml`, `libs/scrap` (backends não Windows), `libs/enigo`, `libs/clipboard` · @dono:

### Features (trilha B) — cada tarefa remove UI + FFI + Rust + traduções da feature
- [ ] **T1.5** Remover gravação de tela (`_RecordMenu`, gravador em `libs/scrap/src/common/record.rs` e chamadas).
  dep: T0.4 · Área: `remote_toolbar.dart` (menu de gravação), módulo de gravação · @dono:
- [ ] **T1.6** Remover chat e chamada de voz (`_ChatMenu`, `_VoiceCallMenu`, `chat_page.dart`, `audio_input.dart` se só usado por voz, tratamento no servidor).
  dep: T0.4 · Área: `remote_toolbar.dart` (menus chat/voz), `flutter/lib/common/widgets/chat_page.dart`, `src/server/connection.rs` (handlers) · @dono:
- [ ] **T1.7** Remover teclado virtual / ações móveis (`_MobileActionMenu` e afins) e menus de teclado avançados que não fazem sentido (manter só o necessário para Windows→Windows).
  dep: T0.4 · Área: `remote_toolbar.dart`, `flutter/lib/common/widgets/keyboard_shortcuts` · @dono:
- [ ] **T1.8** Remover encaminhamento de portas, terminal, câmera, whiteboard, impressora remota: páginas `port_forward_*`, `terminal_*`, `view_camera_*`, `src/whiteboard`, `libs/remote_printer`, `src/port_forward*.rs`.
  dep: T0.4 · Área: esses arquivos + pontos de entrada em `desktop_tab_page.dart`/`main.dart` · @dono:
- [ ] **T1.9** Remover conta/servidor: login, OIDC, address book, grupos, `src/hbbs_http*`, `custom_server.rs`, tela de configurações de ID/relay server, `api_server_warning.dart`.
  dep: T0.4 · Área: `flutter/lib/common/widgets/{login,address_book,my_group,oidc_auth_status}.dart`, `src/hbbs_http*` · @dono:
- [ ] **T1.10** Remover atualização automática e instalação: `src/updater*`, `src/manual_update.rs`, `src/update_*`, `install_page.dart`, `update_progress.dart`, `res/msi`, opções "Instalar" na UI.
  dep: T0.4 · Área: esses arquivos · @dono:
- [ ] **T1.11** Remover a UI Sciter legada (`src/ui/`, `src/ui.rs`, `res/inline-sciter.py`, feature `inline`).
  dep: T0.4 · Área: `src/ui*` · @dono:
- [ ] **T1.12** Itens das decisões D1–D4 marcados como "remover".
  dep: D1–D4 · Área: conforme decisão · @dono:
- [ ] **T1.13** Traduções: manter só os idiomas de D8 em `src/lang` e apagar chaves sem uso (após T1.5–T1.12).
  dep: D8, T1.5–T1.12 · Área: `src/lang/`, `src/lang.rs` · @dono:

## Fase 2 — Funcionamento só na LAN (trilha C)

- [ ] **T2.1** Desligar o servidor rendezvous/relay: não iniciar `rendezvous_mediator`, remover os servidores padrão (`RENDEZVOUS_SERVERS`), nenhuma tentativa de conexão externa. Aceite: com Wireshark, o app não envia nada para fora da sub-rede.
  dep: T0.5, T0.6 · Área: `src/rendezvous_mediator.rs`, `src/server.rs`, `libs/hbb_common/src/config.rs` · @dono:
- [ ] **T2.2** Acesso direto por IP sempre ligado (porta TCP 21118), sem opção para desligar.
  dep: T2.1, T0.6 · Área: `libs/hbb_common/src/config.rs`, `src/server.rs` · @dono:
- [ ] **T2.3** Conexão de saída sempre direta: conectar por IP (digitado ou vindo da descoberta), sem consultar servidor nem tentar hole punching/relay. Timeout curto e mensagem clara ("Computador não encontrado na rede").
  dep: T2.1 · Área: `src/client.rs` (`connect`, `_start`) · @dono:
- [ ] **T2.4** Descoberta automática: o listener de `src/lan.rs` sempre ativo; busca automática ao abrir e a cada N segundos (sem precisar clicar em atualizar); computadores que não respondem somem da lista (ou ficam "offline") após um tempo.
  dep: T2.1 · Área: `src/lan.rs`, `src/flutter_ffi.rs` (se precisar expor algo) · @dono:
- [ ] **T2.5** Resposta da descoberta inclui: nome do computador, usuário logado, IP e versão do IntraDesk. Ignorar respostas de outros apps (RustDesk original) com um identificador próprio no pacote.
  dep: T2.4, T0.6 · Área: `src/lan.rs`, `libs/hbb_common/protos` · @dono:
- [ ] **T2.6** Remover o indicador "Pronto / Conectando à rede" da tela inicial (depende do rendezvous) e trocar por "Visível na rede: IP x.x.x.x".
  dep: T2.1 · Área: `desktop_home_page.dart` · @dono:
- [ ] **T2.7** Renomear o app no núcleo: `APP_NAME` = IntraDesk, pasta de config conforme D9, nome do pipe IPC e do mutex de instância única, nome do `rustdesk.exe` e a pasta de extração do portátil (`libs/portable`, hoje `%LOCALAPPDATA%\rustdesk`). Objetivo: o IntraDesk roda ao lado de um RustDesk aberto sem conflito. **Prioridade alta**: o notebook de desenvolvimento é acessado por RustDesk portátil e não pode rodar o build antes disso.
  Aceite: com o RustDesk portátil aberto, abrir e fechar o `IntraDesk.exe` não afeta o RustDesk (sessão, ID, senha, config).
  dep: T0.4, T0.6, D9 · Área: `libs/hbb_common/src/config.rs`, `src/ipc.rs`, `libs/portable`, `flutter/windows/runner`, `build.py` · @dono:
- [ ] **T2.8** Firewall: detectar se as portas estão bloqueadas e mostrar aviso com botão "Liberar no firewall" (pede elevação e cria a regra com `netsh` só para redes privadas).
  dep: T2.2 · Área: `src/platform/windows.rs` (função nova), tela inicial · @dono:

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

- [ ] **T4.1** Barra da sessão só com: tela cheia, área de transferência (ligar/desligar), áudio (ligar/desligar), escala (original / ajustar / personalizada), qualidade (ótima / balanceada / velocidade / personalizada), resolução, desconectar, além do que for decidido em D2/D3.
  dep: T1.5–T1.8 · Área: `flutter/lib/desktop/widgets/remote_toolbar.dart` · @dono:
- [ ] **T4.2** Validar cada item da barra entre dois PCs Windows (ver T7.3) e corrigir o que quebrou com a limpeza.
  dep: T4.1, T3.1 · Área: conforme bug · @dono:
- [ ] **T4.3** Área de transferência: garantir texto (e imagem, se já funcionar) nos dois sentidos; arquivos só se D1 = manter.
  dep: T4.1, D1 · Área: `src/clipboard.rs`, `libs/clipboard` · @dono:
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

- Descoberta funciona só na mesma sub-rede (broadcast não passa por roteador/VLAN). Em redes segmentadas, conectar digitando o IP.
- Sem instalação/serviço: não é possível acessar a tela de login do Windows nem controlar janelas elevadas sem rodar como administrador (ver D7).
