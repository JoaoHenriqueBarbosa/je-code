# Estado medido do roadmap

Conferido e atualizado a cada integração. Cada número leva fonte: **eu**, **fork** ou **vigente** (data).
Critério nunca medido aparece como NÃO MEDIDO.

Fase corrente: **0 (bancada e fundações)**.

## Fase 0

| Critério | Estado | Fonte |
| --- | --- | --- |
| H01 a H14, H23 a H27 com veredito | 19 de 19 (H08 parcial: WebGPU a 60 fps e CPU com simd128 a 56 a 60 fps no Chrome do Windows; sem WebGL por construção; confirmadas: H02, H03, H07, H10, H11 (codicon), H14, H23 no Windows, H24, H26, H27; refutada: H05; parciais: H01, H04, H09, H12, H13 (só o JSON); dispensada: H06). Em medição: nenhuma. NÃO MEDIDO: H25 além de `vs/base` | eu, 2026-10-23 |
| Substrato gráfico e de texto escolhido pela medida | **Escolhido**: winit + vello (GPU) + vello_cpu (CPU) + parley + accesskit; Bevy refutado (H05) | eu, 2026-10-23 |
| Harness do oráculo com um teste mocha portado e um screenshot comparado | Testes mocha: 72+ casos portados passando. Screenshot de ponta a ponta: moldura do je-code contra o oráculo, fundos idênticos, 5,285% de pixels diferentes só no que falta portar (H26) | eu, 2026-10-23 |
| `scripts/pure-rust-check.sh` verde nos 4 alvos | **Verde** para o produto (`crates/*`) nos 4 alvos; ligações só a libs do sistema (`libdl` no Linux, `libobjc` no macOS) | eu, 2026-10-23 |

## Fase 1 (adiantada em paralelo à Fase 0)

| Critério | Estado | Fonte |
| --- | --- | --- |
| Testes de `vs/base` portados (oráculo: 2 278 casos em 150 arquivos) | 929 casos em 37 arquivos (40,78%); 961 testes passando no `je-base` | eu, `scripts/conformance.py` + `cargo test -p je-base`, 2026-10-23 |
| Widgets de base com screenshot comparado ao oráculo | NÃO MEDIDO | |

## Fases 2 a 8

Nenhum critério medido ainda; os tamanhos de cada camada estão no `roadmap.md` (eu, 2026-10-23).

## Registro

- 2026-10-23: workspace criado; oráculo 1.143.1 baixado e fonte na mesma tag; sondas H01 e H02 escritas.
- 2026-10-23: H02 confirmada (58,9 fps com vsync no Wayland do WSLg sobre a RTX via GL/D3D12, 194 fps no X11); H01 parcial (GPU só com `MESA_LOADER_DRIVER_OVERRIDE=d3d12`); H27 criada.
- 2026-10-23: H03 confirmada (CPU: 81,6 fps normal e 37,5 fps denso em janela no WSLg); H04 parcial (GPU: 60,4 fps no X11, 52,9 a 55,1 no Wayland do WSLg).
- 2026-10-23: H12 parcial: turso lê e escreve o `state.vscdb` real igual ao oráculo; a crate `turso` compila C (simsimd), `turso_core` mínimo não.
- 2026-10-23: H14 dimensionada (243 chamadas de subexpressão nas gramáticas: motor Oniguruma próprio); H25 dimensionada (2 209 testes em `vs/base`, 1 533 `common`).
- 2026-10-23: escopo reduzido pelo user: agents view e sidebar do Copilot/chat (BYOK) saem; `inlineChat` e `mcp` a confirmar.
- 2026-10-23: H05 refutada (Bevy: sem CPU, 34/27 fps em software, sem RTX no WSLg); H07 confirmada: substrato = winit + vello + vello_cpu + parley + accesskit.
- 2026-10-23: H23 confirmada para Windows (cross no container llvm-mingw; no Windows nativo a cena faz 703 fps na GPU e 153 na CPU); macOS aberto (SDK não redistribuível, runner de CI).
- 2026-10-23: H27 confirmada (o `je-code` se relança com a GPU no WSL); `vs/base`: json, path e strings portados (72 casos).
- 2026-10-23: H10 confirmada (parley + teto em 1/64 px = largura do Chromium do oráculo); zyarr integrado (RegExp ECMAScript do pseudo-linus, 450 testes).
- 2026-10-23: H26 confirmada de ponta a ponta (je-render offscreen + je-theme contra a captura do oráculo).
- 2026-10-23: je-codicons (785 ids, fonte codicon.ttf); H09 parcial (fontique resolve as listas do VS Code como o oráculo no latino; emoji e scripts sem conferência); no WSLg a janela vai pelo X11 (60 fps).
- 2026-10-23: H14 confirmada: ferriki-textmate + ferroni corrigido = 109 de 109 goldens de tokenização do VS Code, 100% dos tokens.
- 2026-10-23: vs/base em 25,55% dos casos do oráculo (582); je-theme com as 450 cores registradas iguais ao oráculo.
- 2026-10-23: vs/base em 40,78% (async 145/145, glob, filters, fuzzyScorer, comparers com ICU, keyCodes); je-theme com 998 cores iguais ao oráculo; je-storage com ida e volta no SQLite do oráculo; H14 confirmada (TextMate 109/109).
- 2026-10-23: primeira tela de editor de verdade: `je-editor-view` (rolagem, linhas visíveis, números, tokens coloridos pelo tema, guias, realce, seleção com cantos arredondados, cursor piscando, colchetes colorizados e casando, entrada de mouse e teclado, desfazer e refazer, clipboard) ligado ao `je-code` (abre o arquivo, linguagem pela extensão, tokenização em fatias, Ctrl+S, `Ln x, Col y`). Contra o editor do oráculo: 2,614% dos pixels diferentes (H26). Arquivo de 1 milhão de linhas: CPU 130 fps rolando e 100 fps digitando, GPU 76 e 65 fps (H28), eu.

- 2026-10-24: retomada no jet-dev. `main` verificado (6 411 testes dos crates do produto passando). Roda de 5 sonnets: viewLayout, viewModelDecorations, glyphLanesModel, viewContext, editorTheme, suporte do viewModelImpl, gpu (atlas e buffers), sandbox, networkFilter, browserView, update, autoApprove, quickinput, remoteTunnel, terminalRecorder, terminal (environment, profiles, requestStore), otel, webContentExtractor, policy (freshness, fileManagedSettings), errors, nls, hash, id, executionGraph, jsonParse. Suíte: 6 762 passando, 0 falhas. Placar do oráculo: 5 370 de 16 410 (32,72%): base 81,65%, platform 53,56%, editor 51,27% (eu, `scripts/conformance.py`).

## Pendência antes do primeiro push

- O histórico local tem caminho da máquina em dois commits já corrigidos (`oracle-harness/zyarr-gen/gen.js` no c468a64 e `crates/je-platform/tests/settings_compat.rs` no 55c6335). Reescrever esses dois commits (ou fazer squash do histórico) antes de publicar o repositório.
- 2026-10-23: H08 parcial: WebGPU 60 fps (vsync) e vello_cpu com simd128 56 a 60 fps no Chrome; WebGPU indisponível no Chromium do WSL (Mesa sem dzn).
- 2026-10-23: tokenização no editor: `je-editor` com `TokenizationRegistry`, `LineTokens`, armazenamentos de tokens, `TextModel.tokenization()` (fatias em segundo plano com orçamento de tempo), serviço e registro de linguagens, configuração de linguagem; `je-languages` com a ponte TextMate (109 de 109 fixtures pelo `TextModel`, 10 temas) e o carregamento dos `language-configuration.json` (69 arquivos). Tokenização inicial de 50 814 linhas TypeScript em 901 ms (56 413 linhas/s), eu.
- 2026-10-23: `vs/platform` mostra 2,68% porque 12 108 dos 15 868 casos são do `agentHost` (agentes de IA, a confirmar com o user se cai no corte). Sem ele são 3 760 casos; o placar real da camada, descontado o agentHost, é 426 de 3 760 (11,3%).
- 2026-10-23: user confirmou o corte de IA e agentes (agentHost, github, mcp, chat, sessions...) com a porta aberta para o futuro. Placar sem essas pastas: 2 772 de 16 410 casos (16,89%); base 69,71%, platform 15,18%, editor 24,11%.
- 2026-10-23: `je-code.exe` (cross llvm-mingw) rodando no Windows nativo com a pasta `extensions` embutida ao lado do executável; o tema padrão passou a vir de `theme-defaults`, não da árvore de fontes. Relato do user, sem medição: desempenho igual ou superior ao do VS Code e paridade visual dentro do editor; fps no Windows com o workbench NÃO MEDIDO.
- 2026-10-23: je-editor ganhou config (infra de `editorOptions`, `editorZoom`, `fontInfo`), folding (`foldingModel`, `hiddenRangeModel`), `editKind`, `longDistanceWidgetPlacement`, `graph`, `computeGhostText`, `unicodeTextModelHighlighter` (eu, `cargo test -p je-editor`).
- 2026-10-23: placar do oráculo (escopo descontado) em 4 523 de 16 410 casos (27,56%): base 79,72%, platform 43,19%, editor 40,52%, workbench/services 2,01%, workbench/contrib 3,69% (eu, `scripts/conformance.py`). Novo crate `je-workbench`. Unificados no `je-base`: `JsRegExp`/`createRegExp`/`regExpLeadsToEndlessLoop` (saíram duas cópias do `je-editor`), `semver` (saiu do `je-extensions`) e `localeCompare` (duas aproximações trocadas pelo ICU). Opções do editor passaram a validar e descrever esquema sobre `Dynamic`, como o `unknown` e o `IConfigurationPropertySchema` do original.
