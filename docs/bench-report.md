# Bancada: hipóteses, experimentos e vereditos

Toda premissa de arquitetura do je-code vira uma hipótese aqui antes de virar código. Cada linha
leva o experimento (em `testbench/`), o veredito e a fonte do número (**eu** = medido nesta máquina,
**fork** = reportado por agente, **vigente** = herdado de arquivo versionado, com data). Hipótese sem
experimento rodado fica como **a medir**.

Máquina de medição: WSL2 (kernel 6.18) sobre Windows com RTX 5060 Ti via `/dev/dxg`, 16 núcleos,
25 GB de RAM, WSLg (Wayland e X11), Mesa 25.0 com GL sobre d3d12 e sem ICD Vulkan instalado.

## Substrato gráfico e de janela

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H01 | O wgpu enxerga uma GPU sob o WSLg, e com qual backend | `testbench/gpu-probe`: enumera adapters de todos os backends | **Parcial** (eu, 2026-10-23): sem ajuda, só `Gl \| llvmpipe` (software); Vulkan só `lvp` (software) porque o `mesa-vulkan-drivers` do Debian 13 não traz o `dzn`. A RTX 5060 Ti aparece como `Gl \| D3D12 (NVIDIA GeForce RTX 5060 Ti)` quando o Mesa é forçado com `GALLIUM_DRIVER=d3d12` (o `d3d12_dri.so` existe; o Mesa desta distro não o escolhe sozinho). Caminho de GPU no WSLg = GL sobre D3D12 |
| H02 | Uma janela winit no WSLg apresenta pelo wgpu com vsync a 60 fps | `testbench/win-probe`: 5 s de clear com `AutoVsync`, conta frames | **Confirmada** (eu, 2026-10-23): Wayland, `Gl \| D3D12 (RTX 5060 Ti)`, 1280x800, só `Fifo`: 295 frames em 5,01 s = 58,9 fps. X11 (XWayland) com o mesmo adapter: 194 fps (o compositor RDP não impõe vsync no X11). Sem o override: llvmpipe GL a 59,1 fps no Wayland e llvmpipe Vulkan a 420 fps no X11. O caminho X11 exigiu instalar `libxkbcommon-x11-0` (lib de sistema que o winit abre por dlopen) |
| H03 | O caminho de CPU (`vello_cpu` sobre `softbuffer`) pinta uma cena de workbench em 1080p a 30 fps ou mais | `testbench/scene-bench` (`headless-cpu`, `window-cpu`): partes do layout em retângulos, o `pieceTreeBase.ts` real (1 882 linhas) com cor por token, rolagem de 1 px por frame; `SCENE_DENSE=4` repete cada linha 4 vezes (linhas cheias até a borda) | **Confirmada** (eu, 2026-10-23; `vello_cpu` 0.3 com 8 threads e AVX2): sem janela, 10,7 ms por frame (94 fps) em texto normal e 23,8 ms (42 fps, p99 29,1 ms) em texto denso. Com janela no WSLg, de ponta a ponta (pintura, conversão para o formato do softbuffer e apresentação): Wayland 81,6 fps e X11 77,3 fps em texto normal; Wayland 37,5 fps e X11 36,6 fps em texto denso. Margem estreita no pior caso: o `vello_cpu` usou 8 das 16 threads por padrão |
| H04 | O caminho de GPU (`vello` sobre wgpu) pinta a mesma cena a 60 fps ou mais | `testbench/scene-bench` (`headless-gpu`, `window-gpu`), `GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl` para o Mesa usar a RTX; 30 frames de aquecimento (compilação de shader) fora da medida; a imagem da swapchain é adquirida antes de pintar | **Confirmada no X11, parcial no Wayland** (eu, 2026-10-23, RTX 5060 Ti via GL sobre D3D12): sem janela, 7,1 ms por frame (141 fps) em texto normal e 9,7 ms (103 fps) em texto denso. Em janela, com vsync (só `Fifo` disponível): X11 60,4 fps (p99 18,7 ms) e 59,2 fps em texto denso; Wayland 52,9 fps e 55,1 fps. A diferença do Wayland está na apresentação pelo compositor do WSLg, não na pintura (a pintura cabe em menos da metade do quadro). Atenção: `MESA_LOADER_DRIVER_OVERRIDE=d3d12` sozinho NÃO entrega a GPU; quem entrega é `GALLIUM_DRIVER=d3d12`. Com o pintor pintando antes de adquirir a imagem, o Wayland ficou em 55,6 fps e o X11 em 54,0 fps (medida anterior, mesma máquina) |
| H05 | O Bevy serve de substrato: mesma cena a 60 fps na GPU, alvo web, sem C na árvore; e NÃO tem caminho de CPU próprio | `testbench/scene-bevy`: a mesma cena com `bevy_ui` + `bevy_text`, linhas do editor e do minimap virtualizadas como no VS Code, `PresentMode::AutoNoVsync`, média de 240 frames do `FrameTimeDiagnosticsPlugin` | **Refutada** (eu, 2026-10-23, Bevy 0.20 com wgpu 30): (1) sem rasterizador de CPU; o único caminho sem GPU é um adapter de software, e no Vulkan por software (lavapipe, X11) fez 34,0 fps em texto normal e 27,1 fps em texto denso, abaixo dos 30 exigidos no pior caso, onde o `vello_cpu` em janela fez 81,6 e 37,5; (2) no WSLg não chega à RTX: o Bevy não reexporta o backend GL do wgpu (precisa de `wgpu/gles` à mão) e, ligado, a surface volta com zero modos de apresentação (`Options: []`) e o renderer entra em `unreachable!`, porque a instância GL do wgpu 30 precisa do display handle na criação e o Bevy não passa; (3) o `blake3` do `bevy_asset` compila C e assembly sem a feature `pure`; (4) o caminho Wayland do `accesskit_winit` liga a `libwayland-client` em tempo de build (exige `libwayland-dev` e `pkg-config`), onde o winit sozinho a abre por dlopen |
| H06 | O iced serve de substrato: wgpu na GPU, `tiny-skia` na CPU, alvo web; medir fps dos dois caminhos | `testbench/scene-iced` | **NÃO MEDIDO**: dispensado pela decisão da H07 (ver lá o motivo); reabre se a pilha Linebender tropeçar |
| H07 | Uma pilha própria sobre Linebender (vello, vello_cpu, parley, accesskit, winit) custa menos que adaptar um toolkit pronto às exigências do workbench (layout do VS Code, temas, acessibilidade) | comparação dos protótipos H03 a H05 na mesma cena | **Confirmada** (eu, 2026-10-23): a mesma `Scene` vai para a GPU (`vello`) e para a CPU (`vello_cpu`) pelo mesmo trait de pintura, o caminho de CPU passa dos 30 fps no pior caso medido, o de GPU chega a 60 no X11 do WSLg, e nada compila C. O Bevy foi refutado (H05). O iced (H06) não foi medido: a decisão não depende dele, porque o caminho de CPU dele é `tiny-skia`, inferior ao `vello_cpu` em paridade de pintura com a GPU, e o modelo Elm dele não casa com o modelo de serviços e eventos do workbench; fica registrado como não medido. Os widgets do VS Code (lista, árvore, splitview, grid, sash, menus) são próprios no original também, então o trabalho de widget é o mesmo em qualquer substrato |
| H08 | A mesma cena no alvo `wasm32-unknown-unknown` roda a 60 fps no Chromium com WebGPU e cai para WebGL sem perder paridade | `testbench/scene-web` | a medir |

## Texto e fontes

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H09 | `parley` + `fontique` + `skrifa` acham as fontes do sistema que o VS Code usa (Segoe UI e Consolas no Windows, system-ui e Droid Sans Mono no Linux, SF no macOS), fazem fallback e desenham emoji colorido | `testbench/font-probe system` (fontique com fontconfig por dlopen, como o Chromium usa no Linux) | **Parcial** (eu, 2026-10-23): as listas do VS Code resolvem como no oráculo para texto latino (`system-ui, "Ubuntu", "Droid Sans", sans-serif` dá DejaVu Sans; `"Droid Sans Mono", "monospace", monospace` dá DejaVu Sans Mono, o mesmo que a H10 viu no editor do oráculo) e CJK cai na WenQuanYi Zen Hei. Divergências prováveis com o Chromium, ainda não conferidas: emoji em texto proporcional fica na DejaVu Sans (glifo monocromático; o Chromium prefere a Noto Color Emoji para apresentação emoji) e, no monoespaçado, emoji cai na Unifont Sample e hebraico na FreeMono. Próximo passo: comparar fonte por trecho com o `CSS.getPlatformFontsForNode` do CDP no oráculo |
| H10 | O layout de texto do parley reproduz as métricas que o VS Code mede no Chromium (largura de caractere, lineHeight, posição de caret) dentro de 1 px, com e sem ligaduras | `oracle-harness/cdp_eval.js` lê do editor do oráculo a largura de cada `.view-line` de `wip/scratch/fixtures/metrics.txt`; `testbench/font-probe` mede as mesmas linhas no parley | **Confirmada** (eu, 2026-10-23): o oráculo pede `"Droid Sans Mono", monospace` a 14 px, a Droid Sans Mono não está instalada e o Chromium cai no `monospace` do fontconfig (DejaVu Sans Mono). Larguras: oráculo 84,296875 / 354,015625 / 168,578125 px; parley 84,287109 / 354,005859 / 168,574219 px. A diferença inteira é o arredondamento do LayoutNG para cima em 1/64 px (`LayoutUnit::FromFloatCeil`): `ceil(w * 64) / 64` do valor do parley dá exatamente o do oráculo nas três linhas, inclusive com tab e acento. Line height de 19 px é o `editor.lineHeight` calculado pelo VS Code (1,35 x 14 arredondado), não métrica da fonte. Ligaduras e posição de caret: a medir na fase 3 |
| H11 | O `codicon.ttf` e o `seti.woff` do oráculo desenham idênticos (perceptualmente) ao Chromium nos tamanhos usados pelo workbench | render dos glifos e comparação com screenshot do oráculo | a medir |

## Persistência e formatos

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H12 | Um SQLite em Rust puro (`turso`) lê e escreve `state.vscdb` e os bancos de `workspaceStorage` do oráculo sem corromper nada que o `@vscode/sqlite3` leia depois | `testbench/sqlite-probe` + `oracle-harness/sqlite_dump.js` (Node do Electron do oráculo com o `@vscode/sqlite3` dele), sobre cópia do `state.vscdb` global real | **Confirmada no comportamento, parcial na pureza** (eu, 2026-10-23): despejo das 186 linhas de `ItemTable` byte a byte idêntico ao do oráculo; upsert nosso relido pelo oráculo com `integrity_check: ok`; upsert do oráculo relido por nós. Mas a crate `turso` puxa `turso_core` com features padrão, e a feature `autovacuum` liga o `simsimd`, que compila C (`rustc-link-lib=static=simsimd`). `turso_core` com `default-features = false` e só `pure-rust-crypto`, `fs` e `json` compila sem nenhum C (varredura de link-lib vazia). Decisão: a camada de storage usa `turso_core` direto (API de `Statement::step`), não a crate `turso` |
| H13 | `settings.json`, `keybindings.json`, snippets, perfis, `extensions.json`, temas JSON com comentários e vírgula final são lidos pelo mesmo parser tolerante do VS Code (`jsonc`) com as mesmas mensagens de erro | porte do `json.ts` do `vs/base` e os 100+ casos de teste dele | a medir |

## Linguagem e extensões

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H14 | Um motor compatível com o Oniguruma em Rust puro, sob um porte do `vscode-textmate`, tokeniza as gramáticas embutidas igual ao `vscode-textmate` + `vscode-oniguruma` do oráculo | `testbench/textmate-probe`: `ferriki-textmate` 1.1.2 (porte do `vscode-textmate`) sobre o `ferroni` 2.5.2 (Oniguruma em Rust puro), as 109 fixtures de `extensions/vscode-colorize-tests` contra as goldens `colorize-results` (texto, escopos e cor resolvida nos 10 temas padrão) | **Confirmada** (fork mediu 108 de 109, 2026-10-23; eu, depois da correção, 2026-10-23): 109 de 109 fixtures idênticas, 19 584 de 19 584 tokens com `c`, `t` e `r` iguais. A única divergência era do ferroni: `\p` sem chave rejeitado onde o Oniguruma do VS Code lê `p` literal (gramática `log`); corrigido em `vendor/ferroni` (`[patch.crates-io]`, mudança em `JE-CODE.md`). A tradução para `regex-automata` + `fancy-regex` da hipótese original fica refutada pelas 243 chamadas `\g<..>`. Controle negativo do fork: sem injeções, `md-math.md` cai a 28%. Desempenho em arquivo grande: NÃO MEDIDO |
| H15 | O zjsc (98,6% do test262, vigente 2026-10-12 no pseudo-linus) mais a compat Node do zbun/ul-bun rodam o extension host: ativa as 106 extensões embutidas e as instaladas do user | `testbench/exthost-probe` | a medir |
| H16 | O desempenho do zjsc (294 a 803 vezes mais lento que o bun em código de chamadas, vigente 2026-10-12) basta para o `tsserver` responder completions abaixo de 200 ms num projeto médio; se refutada, a saída é um JIT de linha de base em Rust safe | medir `tsserver` no zjsc contra o Node do oráculo | a medir |
| H17 | O `vscode.d.ts` (21 241 linhas, 185 arquivos de proposta) tem a superfície inteira implementável sobre o RPC do extension host sem passo de JS intermediário | inventário por namespace | a medir |

## Serviços do host

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H18 | Terminal: pty em Rust puro (`portable-pty`, ConPTY no Windows via bindings sem C) + parser VT (`vte`) reproduz o xterm.js num corpus de saídas de programas reais | corpus gravado no oráculo | a medir |
| H19 | Busca: as crates do ripgrep como biblioteca (`grep-*`, `ignore`) devolvem os mesmos resultados que o `@vscode/ripgrep` invocado pelo oráculo | corpus de buscas | a medir |
| H20 | Watcher: `notify` cobre a semântica do `@parcel/watcher` (recursivo, exclusões, coalescência) nas quatro plataformas | corpus de eventos | a medir |
| H21 | Rede: `rustls` com provider RustCrypto (sem `ring`/`aws-lc`) + `ureq`/`tungstenite` atendem marketplace, update, settings sync e tunnels | `scripts/pure-rust-check.sh` + chamadas reais | a medir |
| H22 | Teclado: winit + xkb no Linux, bindings no Windows e macOS, reproduzem a detecção de layout do `native-keymap` | comparar `keyboardLayout` com o oráculo | a medir |

## Compilação cruzada

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H23 | Do Linux saem binários de Windows e de macOS sem toolchain fora do Rust no produto | Windows: `scripts/docker/windows-cross.Dockerfile` (Rust 1.99.1 + llvm-mingw 20250528, alvo `x86_64-pc-windows-gnullvm`, CRT e libunwind estáticos), executado no Windows real pela interop do WSL | **Confirmada para Windows, aberta para macOS** (eu, 2026-10-23): o `rust-lld` sozinho não basta (faltam as import libs do Windows e o libunwind); com o llvm-mingw num container sai um `.exe` que só importa DLLs do sistema. Sem `-static`, o `.exe` morre com 0xC0000135 (falta `libunwind.dll`). No Windows nativo, mesma cena da H03/H04: GPU (Vulkan na RTX, `Immediate`) 702,8 fps em texto normal (p99 2,5 ms) e 165,6 fps em texto denso; CPU (`vello_cpu`, softbuffer GDI) 153,0 fps e 70,8 fps; `win-probe` com vsync 60,3 fps. macOS: o SDK da Apple não é redistribuível, então o caminho previsto é um runner macOS de CI; NÃO MEDIDO |
| H24 | O binário Linux roda dentro do WSLg com a mesma GPU que fora | H01 e H02 já medem isso aqui | **Confirmada com ressalva** (eu, 2026-10-23): a GPU chega pelo Mesa d3d12 só com `MESA_LOADER_DRIVER_OVERRIDE=d3d12`; ver H27 |
| H27 | Detectar WSL na partida (`/dev/dxg` presente e `d3d12_dri.so` instalado) e relançar o próprio executável com `GALLIUM_DRIVER=d3d12` e o backend GL entrega a GPU sem ação do user, sem `set_var` (que é `unsafe` na edição 2024) | `testbench/win-probe` com relançamento | **Confirmada** (eu, 2026-10-23): `crates/je-render/src/wsl.rs` detecta `/dev/dxg` e `d3d12_dri.so`, e o `je-code` se relança por `CommandExt::exec` (seguro) com `GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl`; resultado: `pintando com Gpu { adapter: "D3D12 (NVIDIA GeForce RTX 5060 Ti)", api: "Gl" }`. Com `JE_NO_WSL_GPU=1` o adapter é o llvmpipe, que o presenter recusa como GPU, e a pintura vai para o `vello_cpu` (`Cpu { threads: 8 }`), como manda a H03 |

## Fidelidade e método

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H25 | A suíte mocha do VS Code (cerca de 50 mil `test(` em `src/vs`, eu, 2026-10-23, tag 1.143.1) porta caso a caso para Rust e serve de oráculo de comportamento das camadas `base`, `platform`, `editor` e `workbench` | portar os 2 306 de `vs/base` primeiro e medir a taxa de casos que dependem de DOM | a medir |
| H26 | Para a UI, screenshot do oráculo (xvfb) contra o nosso, comparados por diferença perceptual, é um oráculo suficiente para layout e tema | `oracle-harness/run-oracle.sh` + `cdp_screenshot.js` (VS Code 1.143.1 sob `xvfb-run`, `--disable-gpu`, CDP) contra `testbench/screenshot-probe` (moldura pintada pelo `je-render` no caminho de CPU, cores do `2026-dark.json` lidas pelo `je-theme`), comparados por `oracle-harness/imgdiff.py` | **Confirmada** (eu, 2026-10-23): duas capturas do oráculo diferem em 0 de 960 000 pixels (pintura determinística). A moldura do je-code contra o workbench vazio do oráculo (sem a sidebar do chat, tema 2026 Dark): 50 733 pixels diferentes (5,285%), todos em conteúdo ainda não portado (textos, codicons, marca d'água, keycaps) e no contorno de 1 px dos painéis flutuantes; os fundos das partes batem pixel a pixel. Achado: a borda do painel flutuante no oráculo é `#252526` na activity bar e `#2A2B2C` no editor, que não são as chaves óbvias do tema; sai do porte das regras de CSS do layout |

## Medições parciais (dimensionam hipóteses ainda sem veredito)

- **H14, sintaxe Oniguruma nas 83 gramáticas embutidas** (eu, 2026-10-23, tag 1.143.1, 17 271 padrões
  `match`/`begin`/`end`/`while`): lookahead 6 674, lookbehind 4 062, possessivos 1 669, quantificador
  `{n,m}` 1 567, `\G` 1 289, flags inline `(?x)`/`(?i)` 1 272, classes POSIX `[[:alpha:]]` 1 168,
  `\R \A \z \Z` 656, grupo atômico `(?>` 516, backreference numérica 403, escapes `\u`/`\x{` 323,
  **chamada de subexpressão `\g<..>` 243**, `\p{..}` 149, grupo nomeado 96, interseção de classes `&&` 71,
  `\k<..>` 36, `\h` 26, condicional `(?(` 3, operador ausente `(?~` 2, `\K` 1. Consequência: `fancy-regex`
  e `regress` não têm chamada de subexpressão, atômico com semântica Oniguruma, `(?~` nem `\K`; o caminho
  é um motor backtracking próprio compatível com o Oniguruma 6 (o mesmo desfecho da H23 do Plinus para o
  POSIX), medido contra o `vscode-oniguruma` do oráculo.
- **H25, testes de `vs/base`** (eu, 2026-10-23): 142 arquivos, 2 209 casos `test(`: `common` 1 533 (81
  arquivos), `browser` 596 (47), `node` 80 (14). 39 arquivos importam DOM. Os de `common` portam direto; os
  de `browser` testam widgets e portam contra o widget nativo correspondente.
- **H12, bancos reais disponíveis** (eu, 2026-10-23): `state.vscdb` global do user no Windows com 780 KiB e
  11 `workspaceStorage/*/state.vscdb`.
