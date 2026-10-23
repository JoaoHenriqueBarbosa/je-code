# PRD: je-code, o VS Code em Rust

## Visão e tese

O VS Code é o editor que o mundo usa, e é um site rodando num browser embutido. O je-code é o mesmo
editor, com a mesma superfície (comandos, settings, keybindings, temas, extensões, user data), escrito
em Rust seguro e desenhado nativamente: wgpu na GPU, rasterização em software na CPU. Nenhuma outra
linguagem no produto, nenhum `unsafe` nosso, nenhum DOM.

A tese é a mesma dos portes anteriores (cobogo, fiscal-rs, Plinus): fidelidade é placar contra um
oráculo, e cada premissa de arquitetura é hipótese medida antes de virar código.

## Objetivos e métricas de sucesso

- **Paridade completa e retrocompatível.** Tudo que o VS Code 1.143.1 faz, o je-code faz; o user
  data existente (`settings.json`, `keybindings.json`, `state.vscdb`, perfis, extensões instaladas,
  `workspaceStorage`) é lido e escrito no mesmo formato, e as extensões publicadas no marketplace
  rodam sem recompilar.
- **Quatro alvos.** Windows, Linux (inclusive dentro do WSLg), macOS e web, do mesmo código.
- **Desempenho.** 60 fps na GPU quando houver; no mínimo 30 fps no caminho de CPU.
- **Pureza.** `unsafe_code = "forbid"` no workspace e `scripts/pure-rust-check.sh` verde em todos
  os alvos.

## Oráculo

O VS Code real: o binário estável 1.143.1 (`ref/oracle/`, commit `e6468682`), a fonte na mesma tag
(`ref/vscode`) e a suíte dela. Comportamento se mede pelos testes mocha portados caso a caso; layout e
tema se medem por screenshot do oráculo sob xvfb contra o nosso, por diferença perceptual.

## Tamanho do que se porta (eu, 2026-10-23, tag 1.143.1, sem testes)

| Camada | Linhas de TS | Testes mocha |
| --- | --- | --- |
| `vs/base` | 105 038 | 2 306 |
| `vs/platform` | 502 289 | 17 045 |
| `vs/editor` | 202 920 | 3 148 |
| `vs/workbench/services` | 128 023 | 2 543 |
| `vs/workbench/api` (extension host) | 87 291 | 1 008 |
| `vs/workbench/browser` (shell) | 70 316 | 0 |
| `vs/workbench/contrib` (102 contribuições) | 862 544 | 17 175 |
| `vs/sessions` | 195 436 | 7 058 |
| `vs/server` + `vs/code` | 12 005 | 90 |
| CSS (especificação de layout e tema) | 97 718 em 508 arquivos | |
| Extensões embutidas (106, TS) | 1 040 483 | |
| `vscode.d.ts` (API de extensão) | 21 241 | |

## Fora de escopo

- **Agents view e sidebar do Copilot/chat com BYOK** (decisão do user, 2026-10-23). Sai: `vs/sessions`
  (181 195 linhas, 6 208 testes), `workbench/contrib/chat` (359 163 linhas, 10 802 testes),
  `contrib/welcomeAgentSessions`, `contrib/remoteCodingAgents`, `contrib/agentsVoice` e a extensão embutida
  `copilot` (eu, tag 1.143.1). A confirmar com o user, sem portar até lá: `contrib/inlineChat` (4 182 linhas)
  e `contrib/mcp` (22 148 linhas), que dependem do chat. Os contadores de paridade das fases 6 e 8
  descontam essas pastas.
- Reproduzir o Electron, o Chromium ou o DOM. Os `.css` são lidos como especificação.
- Telemetria para a Microsoft e serviços que exigem conta deles (Copilot, Settings Sync com a conta
  deles) são implementados como protocolo, não como obrigação: funcionam se o user configurar.

## Fase 0: bancada e fundações

Workspace com `forbid(unsafe_code)`, verificação de Rust puro, sondas de GPU e janela no WSLg,
cena de editor nos candidatos a substrato (pilha própria Linebender, Bevy, iced), fontes, SQLite puro,
regex Oniguruma, harness de oráculo (testes mocha e screenshot).

Critério de saída: H01 a H14 e H23 a H26 com veredito; substrato gráfico e de texto escolhido pela
medida; harness do oráculo rodando dois casos de ponta a ponta (um teste mocha portado, um
screenshot comparado).

## Fase 1: `vs/base`

Eventos, lifecycle, async, cancelamento, URI, paths, strings, glob, JSON tolerante, diff, árvore de
intervalos, buffer, codificações, e os widgets de base (lista, árvore, splitview, grid, sash, menu,
botões, input, scrollbar) desenhados nativamente.

Critério de saída: os 2 306 testes portados passando, ou declarados como dependentes de DOM com a
contagem; cada widget de base com screenshot comparado ao oráculo.

## Fase 2: `vs/platform`

Configuração (defaults, user, workspace, perfis, políticas), keybindings e resolução de teclado,
arquivos (provider de disco, watcher), storage (`state.vscdb`), log, workspace e trust, temas (cores,
ícones de arquivo e de produto), extensões (manifesto, VSIX, marketplace, instalação), diálogos,
clipboard, notificações nativas, update, telemetria como protocolo.

Critério de saída: 17 045 testes portados; user data real do oráculo lido e reescrito sem diferença.

## Fase 3: `vs/editor`

Piece tree, tokenização TextMate e semântica, view model, layout de linhas, cursor múltiplo,
comandos de edição, find e replace, folding, suggest, hover, code lens, inlay hints, minimap, diff
editor, acessibilidade.

Critério de saída: 3 148 testes portados; editor abrindo arquivo de 100 MB com scroll a 60 fps na
GPU e 30 na CPU; tokens idênticos ao oráculo nas 83 gramáticas (H14).

## Fase 4: shell do workbench

Layout em partes (barra de título, atividade, sidebar, painel, grupos de editor, status bar),
viewlets e views, command palette e quick input, notificações, diálogos, menus de contexto, barra de
menus nativa, tema aplicado a tudo, dispatch de keybindings, janelas auxiliares.

Critério de saída: o workbench vazio abre com screenshot perceptualmente igual ao oráculo nos temas
Dark Modern e Light Modern; 2 543 testes de `workbench/services` portados.

## Fase 5: extension host

Protocolo RPC do extension host, `vscode.d.ts` inteiro, ativação por eventos, extension host local
sobre o zjsc com compat Node, web extension host, as 106 extensões embutidas rodando, instalação do
marketplace, e o `tsserver` com latência aceitável (H16).

Critério de saída: as 106 embutidas ativam sem erro; as extensões instaladas na máquina do user
ativam e os comandos delas respondem; 1 008 testes de `workbench/api` portados.

## Fase 6: contribuições

As 102 pastas de `workbench/contrib`: explorador, busca, SCM e git, debug (DAP), terminal (pty, VT,
render), tasks, notebooks, testes, chat, remoto, settings e keybindings editors, markdown, webviews,
timeline, snippets, emmet, e tudo o mais. Ordem por uso medido no oráculo e por dependência.

Critério de saída: 17 175 testes portados; cada contribuição com cenário de ponta a ponta contra o
oráculo.

## Fase 7: plataformas e desempenho

Windows (ConPTY, registro, mutex, associação de arquivos), macOS (menu nativo, Metal via wgpu),
web (wasm, WebGPU e WebGL, extension host em worker), WSLg, empacotamento e update.

Critério de saída: os quatro alvos compilando do Linux (H23); 60 fps na GPU e 30 na CPU nos cenários
de referência em cada plataforma; binários estáticos publicados.

## Fase 8: retrocompatibilidade

Migração do user data real, corpus de extensões populares do marketplace, `vs/sessions` e o que
restar da superfície.

Critério de saída: o user abre o je-code no lugar do VS Code com o mesmo perfil e não nota diferença
de função; corpus de 100 extensões mais instaladas ativando.
