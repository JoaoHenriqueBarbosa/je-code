# Estado medido do roadmap

Conferido e atualizado a cada integração. Cada número leva fonte: **eu**, **fork** ou **vigente** (data).
Critério nunca medido aparece como NÃO MEDIDO.

Fase corrente: **0 (bancada e fundações)**.

## Fase 0

| Critério | Estado | Fonte |
| --- | --- | --- |
| H01 a H14, H23 a H27 com veredito | 13 de 19 (H02, H03, H07, H10, H23 no Windows, H24, H26 no lado do oráculo, H27 confirmadas; H05 refutada; H01, H04, H12 parciais; H06 dispensada) | eu, 2026-10-23 |
| Substrato gráfico e de texto escolhido pela medida | **Escolhido**: winit + vello (GPU) + vello_cpu (CPU) + parley + accesskit; Bevy refutado (H05) | eu, 2026-10-23 |
| Harness do oráculo com um teste mocha portado e um screenshot comparado | Testes mocha: 72+ casos portados passando. Screenshot: captura do oráculo por CDP pronta e determinística (0 pixels de ruído); falta o lado do je-code | eu, 2026-10-23 |
| `scripts/pure-rust-check.sh` verde nos 4 alvos | NÃO MEDIDO | |

## Fase 1 (adiantada em paralelo à Fase 0)

| Critério | Estado | Fonte |
| --- | --- | --- |
| Testes de `vs/base` portados (oráculo: 2 278 casos em 150 arquivos) | 72 casos em 3 arquivos (`json` 17/17, `path` 9/9, `strings` 46/48; os 2 restantes dependem de RegExp ECMAScript) | eu, `cargo test -p je-base`, 2026-10-23 |
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
