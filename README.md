# IntraDesk

Acesso remoto para computadores Windows **na mesma rede local**, sem internet,
sem servidor e sem instalação.

[![Release](https://img.shields.io/github/v/release/YoungC0DE/IntraDesk)](https://github.com/YoungC0DE/IntraDesk/releases/latest)
[![Licença](https://img.shields.io/github/license/YoungC0DE/IntraDesk)](LICENCE)

## Download

[**⬇ Baixar IntraDesk.exe (última versão)**](https://github.com/YoungC0DE/IntraDesk/releases/latest/download/IntraDesk.exe)

É um executável único e portátil: baixe e abra, sem instalar. Versões
anteriores ficam em [Releases](https://github.com/YoungC0DE/IntraDesk/releases).

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

- Controle de mouse e teclado
- Tela cheia
- Área de transferência compartilhada
- Transferência de arquivos
- Ativar/desativar áudio
- Escala, qualidade e resolução da imagem
- Troca de monitor
- Ctrl+Alt+Del, bloquear e reiniciar o computador remoto

## Segurança

- Toda conexão exige senha. A senha temporária muda a cada execução, e é
  possível definir uma senha fixa.
- A sessão é sempre criptografada. Na primeira conexão, o IntraDesk guarda a
  chave do computador remoto e avisa se ela mudar depois.
- Muitas senhas erradas bloqueiam novas tentativas temporariamente.
- Enquanto alguém está conectado, a janela "sendo acessado" fica sempre
  visível, com o botão para desconectar.
- O app não acessa a internet: sem telemetria, atualização automática ou
  servidores externos.

## Compilando (Windows)

### Sem instalar nada: GitHub Actions

Cada push na `master` (ou em branches `t*`) roda o workflow
`.github/workflows/build-windows.yml`. Ele compila, roda os testes e publica na
aba **Actions** o artefato `IntraDesk-windows-x64` com o `.exe` portátil.

### Publicando uma versão

A versão do produto fica em `flutter/pubspec.yaml` (`version: 1.0.0+1`). A do
`Cargo.toml` é a versão de protocolo herdada do RustDesk, usada entre os
computadores para saber quais recursos o outro lado tem: não a altere.

Para publicar, atualize a versão no `pubspec.yaml`, faça o commit e crie a tag
correspondente:

```powershell
git tag v1.0.0
git push origin v1.0.0
```

O CI compila, roda os testes e cria a release com o `IntraDesk.exe` e o seu
SHA-256. É ela que o link de [Download](#download) entrega. Se a tag não bater
com a versão do `pubspec.yaml`, a publicação falha.

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
