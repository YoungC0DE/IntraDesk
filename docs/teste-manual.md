# Roteiro de teste manual — IntraDesk

Teste de aceitação entre dois PCs Windows na mesma rede local, **sem internet**.
Use o `IntraDesk-<commit>.exe` do artefato `IntraDesk-windows-x64` do CI (ou o
`dist\IntraDesk-dev.exe` do `scripts\build.ps1`).

## Preparação

- **PC A** (acessado) e **PC B** (quem acessa). Idealmente um com Windows 10 e
  outro com Windows 11; repita os itens marcados com 🔁 trocando os papéis.
- Os dois na mesma sub-rede (ex.: mesmo switch/Wi-Fi), **sem internet**: desligue
  o cabo/rota do roteador para fora ou use um switch isolado. Confirme com
  `ping 8.8.8.8` (deve falhar) e `ping <IP do outro PC>` (deve responder).
- PC A com **2 monitores** e, em uma rodada, escala de exibição em **150%**.
- Pelo menos um dos PCs **sem o Visual C++ Redistributable** instalado
  (Configurações → Aplicativos: nenhum "Microsoft Visual C++ 20xx Redistributable").
- Copie o .exe para a Área de Trabalho de cada PC. Não instale nada.

Registre cada item como ✅ ok, ❌ falhou (descreva) ou ➖ não testado. Para
falhas, anexe o log: `%APPDATA%\IntraDesk\log\` (pasta do processo principal e
`server`/`cm`).

## 1. Primeira execução

| # | Passo | Esperado | Resultado |
|---|---|---|---|
| 1.1 | Abrir o .exe no PC sem VC++ Redistributable | Abre sem erro de DLL faltando (`VCRUNTIME140.dll`, `MSVCP140.dll`) | |
| 1.2 | Abrir de novo (2ª vez) | Abre mais rápido (usa o cache em `%LOCALAPPDATA%\intradesk`) | |
| 1.3 | Abrir o .exe com o IntraDesk já aberto | Não abre outra janela; traz a existente para frente | |
| 1.4 | Ver a tela inicial | Painel "Este computador" com nome, IP e senha; nome **IntraDesk** e ícone laranja na janela, barra de tarefas e bandeja | |
| 1.5 | Firewall do Windows ligado, regra ainda não liberada | Aviso "Firewall do Windows bloqueando conexões" com "Liberar no firewall"; após aceitar o UAC, o aviso some | |

## 2. Descoberta e conexão 🔁

| # | Passo | Esperado | Resultado |
|---|---|---|---|
| 2.1 | Aba "Computadores na rede" no PC B | PC A aparece sozinho em até ~10 s, com nome, usuário e IP | |
| 2.2 | Busca pelo nome, usuário e IP do PC A | Filtra o card | |
| 2.3 | Duplo clique no card do PC A | Pede senha | |
| 2.4 | Senha errada | "Senha incorreta"; no PC A aparece a tentativa | |
| 2.5 | Senha errada 7 vezes em 1 minuto | "Tente novamente daqui a 1 minuto" | |
| 2.6 | Senha temporária certa | Sessão abre com a imagem do PC A | |
| 2.7 | "Conectar a" com o IP do PC A | Conecta (com senha) | |
| 2.8 | "Conectar a" com o nome do PC A | Conecta (com senha) | |
| 2.9 | Definir senha permanente no PC A (Configurações → Segurança) e conectar com ela | Conecta; a senha temporária também continua valendo | |
| 2.10 | Aba "Sessões recentes" e "Favoritos" (adicionar o PC A) | Mostram o PC A | |
| 2.11 | Configurações → Rede → "Negar descoberta da LAN" no PC A | PC A some da lista do PC B; conexão por IP continua funcionando | |

## 3. Segurança

| # | Passo | Esperado | Resultado |
|---|---|---|---|
| 3.1 | Durante a sessão, olhar o PC A | Janela de aviso "sendo acessado" visível, sempre no topo, com "Desconectar" | |
| 3.2 | "Desconectar" no PC A | Sessão fecha no PC B | |
| 3.3 | Conectar sem digitar senha | Só pede senha; não há botão de aceite no PC A | |
| 3.4 | Chave alterada: no PC A, fechar o IntraDesk, renomear `%APPDATA%\IntraDesk` para `IntraDesk.bak`, abrir de novo; no PC B, conectar | PC B mostra o aviso de **chave de identidade diferente** e só segue se confirmar | |
| 3.5 | Desfazer o 3.4 (voltar a pasta) | — | |

## 4. Sessão remota 🔁

| # | Passo | Esperado | Resultado |
|---|---|---|---|
| 4.1 | Mouse e teclado (digitar num Bloco de Notas no PC A) | Funcionam, acentos inclusive | |
| 4.2 | Tela cheia (entrar e sair) | Ok | |
| 4.3 | Escala: original, ajustar à janela, personalizada | Ok | |
| 4.4 | Qualidade: ótima, balanceada, velocidade, personalizada | Imagem muda de acordo | |
| 4.5 | Resolução do PC A pela barra | Muda e volta ao desconectar | |
| 4.6 | Trocar de monitor (PC A com 2 monitores) | Mostra cada monitor | |
| 4.7 | PC A com escala 150% | Imagem nítida, cliques no lugar certo | |
| 4.8 | Áudio: tocar um vídeo no PC A; desligar e ligar áudio na barra | Som chega no PC B; some e volta | |
| 4.9 | Área de transferência: texto, imagem e arquivos, PC A → B e B → A | Copia nos dois sentidos; "desativar área de transferência" bloqueia | |
| 4.10 | Transferência de arquivos (menu da barra e botão "Transferir arquivo" da tela inicial) | Envia e recebe arquivos e pastas | |
| 4.11 | Ctrl+Alt+Del, bloquear, reiniciar o PC A | Fazem o que dizem (reiniciar: confirmar e reconectar depois) | |
| 4.12 | Duas sessões (PC B conecta em A e num terceiro PC, ou em A duas vezes) | Abrem como **abas** na mesma janela | |
| 4.13 | "Atualizar" na barra | Imagem é redesenhada | |
| 4.14 | Desconectar pela barra | Sessão fecha; aviso some no PC A | |
| 4.15 | Arrastar a barra pela alça (⋮⋮), recolher e expandir; fechar e reconectar | Desliza só na horizontal, no topo; volta na mesma posição ao reconectar | |

## 5. Rede e recuperação

| # | Passo | Esperado | Resultado |
|---|---|---|---|
| 5.1 | Durante a sessão, tirar o cabo/Wi-Fi do PC A por ~10 s e voltar | PC B mostra a queda e reconecta (ou permite reconectar) sem reabrir o app | |
| 5.2 | Firewall do PC A bloqueando (remover a regra do IntraDesk) | PC B não conecta; PC A mostra o aviso de firewall | |
| 5.3 | Imagem congelada (T4.5): no PC A com pouca RAM livre, abrir um CSV grande no Excel durante a sessão, por alguns minutos | A imagem não congela; se congelar, procure `switch due to encoding fails` no log do `server` do PC A e anote se a imagem voltou sozinha | |

## 6. Bandeja e administrador

| # | Passo | Esperado | Resultado |
|---|---|---|---|
| 6.1 | Ícone na bandeja com o app aberto | Aparece; dica "IntraDesk - Pronto" (ou nº de sessões durante uma sessão) | |
| 6.2 | Bandeja → "Abrir" (com a janela minimizada) | Janela volta | |
| 6.3 | Bandeja → "Copiar senha" e colar no Bloco de Notas | Cola a senha de uso único atual | |
| 6.4 | Bandeja → "Sair" | App fecha, ícone some da bandeja, PC deixa de aceitar conexões | |
| 6.5 | Fechar a janela pelo X | App fecha e o ícone some da bandeja (D16) | |
| 6.6 | "Executar como administrador" no PC A, aceitar o UAC | App reabre elevado, com a bandeja; o botão some | |
| 6.7 | Com o PC A elevado, abrir algo que peça UAC no PC A durante a sessão | PC B vê e responde a tela do UAC | |
| 6.8 | "Executar como administrador" e **recusar** o UAC | Nada muda; o app continua aberto | |

## 7. Configurações e idioma

| # | Passo | Esperado | Resultado |
|---|---|---|---|
| 7.1 | Abas: Geral, Segurança, Rede, Exibição, Áudio, Sobre | Só essas; nenhuma menção a ID server, relay, conta ou atualização | |
| 7.2 | Idioma pt-BR e inglês; tema claro e escuro | Textos e cores corretos (laranja) | |
| 7.3 | Sobre | Versão, licença AGPL-3.0, link do código-fonte, crédito ao RustDesk | |

## Resultado

| Data | PCs (Windows) | Build (commit) | Quem testou | Falhas |
|---|---|---|---|---|
| | | | | |
