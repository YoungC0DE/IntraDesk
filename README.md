# IntraDesk

Acesso remoto para computadores Windows **na mesma rede local**, sem internet,
sem servidor e sem instalação.

> **Status:** em desenvolvimento. O plano de trabalho está em [`todo.md`](todo.md).

## Como funciona

1. Abra o `IntraDesk.exe` em cada computador. Não é preciso instalar nada.
2. A tela inicial lista automaticamente os computadores da rede que estão com o
   IntraDesk aberto.
3. Clique em um computador (ou digite o IP dele), informe a senha exibida na
   máquina remota e conecte.

A descoberta usa broadcast UDP na LAN e a sessão é uma conexão TCP direta entre
os dois computadores. Nenhum dado sai da rede local.

## Requisitos

- Windows 10 ou 11 (x64)
- Computadores na mesma rede/sub-rede (o broadcast não atravessa roteadores)
- Firewall do Windows liberando as portas abaixo. Na primeira execução, o
  Windows pergunta se quer permitir; escolha **Redes privadas**.

| Uso | Protocolo | Porta |
|---|---|---|
| Sessão remota | TCP | 21118 |
| Descoberta de computadores | UDP | 21119 |

## Recursos da sessão remota

- Tela cheia
- Área de transferência compartilhada
- Ativar/desativar áudio
- Escala da imagem
- Qualidade da imagem
- Resolução da tela remota

## Compilando (Windows)

### Sem instalar nada: GitHub Actions

Cada push na `master` (ou em branches `t*`) roda o workflow
`.github/workflows/build-windows.yml`. Ele compila, roda os testes e publica na
aba **Actions** o artefato `IntraDesk-windows-x64` com o `.exe` portátil.

### Na sua máquina

0. Clone o repositório:

   ```powershell
   git clone https://github.com/YoungC0DE/IntraDesk.git
   ```

1. Prepare o ambiente uma vez (instala Git, Python, VS Build Tools, LLVM 15,
   Rust 1.75, Flutter 3.24.5, vcpkg e as bibliotecas C/C++). A primeira
   execução leva mais de 1 hora por causa do ffmpeg:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\scripts\setup-dev.ps1
   ```

2. Abra um **novo** terminal e compile. O executável sai em `dist\IntraDesk-dev.exe`:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1
   ```

Testes:

```powershell
cargo test --lib --features flutter
cd flutter; flutter test
```

## Estrutura

| Caminho | Conteúdo |
|---|---|
| `src/` | Núcleo em Rust: cliente, servidor, descoberta na LAN (`src/lan.rs`) |
| `src/server/` | Captura de vídeo/áudio, área de transferência, entrada de mouse/teclado |
| `flutter/lib/desktop/` | Interface (tela inicial e janela da sessão remota) |
| `libs/scrap/` | Captura de tela |
| `libs/enigo/` | Simulação de mouse e teclado |
| `libs/clipboard/` | Área de transferência |
| `libs/hbb_common/` | Rede, configuração e protocolo (base do RustDesk) |
| `libs/portable/` | Empacotador do executável único |

Para agentes de IA e contribuidores: veja [`CLAUDE.md`](CLAUDE.md).

## Licença e origem

O IntraDesk é um fork do [RustDesk](https://github.com/rustdesk/rustdesk),
© Purslane Ltd. e contribuidores, e é distribuído sob a mesma licença
**GNU AGPL-3.0** (ver [`LICENCE`](LICENCE)). Quem distribuir versões modificadas
deve disponibilizar o código-fonte correspondente nos termos da licença.

"RustDesk" é marca dos seus respectivos donos. O IntraDesk não é afiliado ao
projeto RustDesk.

> **Uso responsável:** use o IntraDesk apenas em computadores que você tem
> autorização para acessar.
