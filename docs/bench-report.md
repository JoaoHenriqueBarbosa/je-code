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
| H05 | O Bevy serve de substrato: mesma cena a 60 fps na GPU, alvo web, sem C na árvore; e NÃO tem caminho de CPU próprio | `testbench/scene-bevy` + `scripts/pure-rust-check.sh` | a medir |
| H06 | O iced serve de substrato: wgpu na GPU, `tiny-skia` na CPU, alvo web; medir fps dos dois caminhos | `testbench/scene-iced` | a medir |
| H07 | Uma pilha própria sobre Linebender (vello, vello_cpu, parley, accesskit, winit) custa menos que adaptar um toolkit pronto às exigências do workbench (layout do VS Code, temas, acessibilidade) | comparação dos protótipos H04 a H06 na mesma cena | a medir |
| H08 | A mesma cena no alvo `wasm32-unknown-unknown` roda a 60 fps no Chromium com WebGPU e cai para WebGL sem perder paridade | `testbench/scene-web` | a medir |

## Texto e fontes

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H09 | `parley` + `fontique` + `skrifa` acham as fontes do sistema que o VS Code usa (Segoe UI e Consolas no Windows, system-ui e Droid Sans Mono no Linux, SF no macOS), fazem fallback e desenham emoji colorido | `testbench/font-probe` | a medir |
| H10 | O layout de texto do parley reproduz as métricas que o VS Code mede no Chromium (largura de caractere, lineHeight, posição de caret) dentro de 1 px, com e sem ligaduras | comparar com `editor.fontInfo` extraído do oráculo | a medir |
| H11 | O `codicon.ttf` e o `seti.woff` do oráculo desenham idênticos (perceptualmente) ao Chromium nos tamanhos usados pelo workbench | render dos glifos e comparação com screenshot do oráculo | a medir |

## Persistência e formatos

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H12 | Um SQLite em Rust puro (`turso`) lê e escreve `state.vscdb` e os bancos de `workspaceStorage` do oráculo sem corromper nada que o `@vscode/sqlite3` leia depois | `testbench/sqlite-probe` + `oracle-harness/sqlite_dump.js` (Node do Electron do oráculo com o `@vscode/sqlite3` dele), sobre cópia do `state.vscdb` global real | **Confirmada no comportamento, parcial na pureza** (eu, 2026-10-23): despejo das 186 linhas de `ItemTable` byte a byte idêntico ao do oráculo; upsert nosso relido pelo oráculo com `integrity_check: ok`; upsert do oráculo relido por nós. Mas a crate `turso` puxa `turso_core` com features padrão, e a feature `autovacuum` liga o `simsimd`, que compila C (`rustc-link-lib=static=simsimd`). `turso_core` com `default-features = false` e só `pure-rust-crypto`, `fs` e `json` compila sem nenhum C (varredura de link-lib vazia). Decisão: a camada de storage usa `turso_core` direto (API de `Statement::step`), não a crate `turso` |
| H13 | `settings.json`, `keybindings.json`, snippets, perfis, `extensions.json`, temas JSON com comentários e vírgula final são lidos pelo mesmo parser tolerante do VS Code (`jsonc`) com as mesmas mensagens de erro | porte do `json.ts` do `vs/base` e os 100+ casos de teste dele | a medir |

## Linguagem e extensões

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H14 | Um motor de regex compatível com o Oniguruma em Rust puro (tradução para `regex-automata`, com `fancy-regex` para backreference e lookaround) tokeniza as 83 gramáticas TextMate embutidas igual ao `vscode-textmate` + `vscode-oniguruma` | corpus de arquivos tokenizado pelo oráculo (Node do Electron) e pelo nosso | a medir |
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
| H23 | Do Linux saem binários de Windows (`x86_64-pc-windows-msvc` com `rust-lld` autônomo) e de macOS (`aarch64-apple-darwin` com `rust-lld` e stubs `.tbd`) sem toolchain fora do Rust | build de `win-probe` para os três alvos | a medir |
| H24 | O binário Linux roda dentro do WSLg com a mesma GPU que fora | H01 e H02 já medem isso aqui | **Confirmada com ressalva** (eu, 2026-10-23): a GPU chega pelo Mesa d3d12 só com `MESA_LOADER_DRIVER_OVERRIDE=d3d12`; ver H27 |
| H27 | Detectar WSL na partida (`/dev/dxg` presente e `d3d12_dri.so` instalado) e relançar o próprio executável com `GALLIUM_DRIVER=d3d12` e o backend GL entrega a GPU sem ação do user, sem `set_var` (que é `unsafe` na edição 2024) | `testbench/win-probe` com relançamento | a medir |

## Fidelidade e método

| Id | Hipótese | Experimento | Veredito |
| --- | --- | --- | --- |
| H25 | A suíte mocha do VS Code (cerca de 50 mil `test(` em `src/vs`, eu, 2026-10-23, tag 1.143.1) porta caso a caso para Rust e serve de oráculo de comportamento das camadas `base`, `platform`, `editor` e `workbench` | portar os 2 306 de `vs/base` primeiro e medir a taxa de casos que dependem de DOM | a medir |
| H26 | Para a UI, screenshot do oráculo (xvfb) contra o nosso, comparados por diferença perceptual, é um oráculo suficiente para layout e tema | montar o harness e medir o ruído entre duas capturas do próprio oráculo | a medir |

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
