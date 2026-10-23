# Estado medido do roadmap

Conferido e atualizado a cada integração. Cada número leva fonte: **eu**, **fork** ou **vigente** (data).
Critério nunca medido aparece como NÃO MEDIDO.

Fase corrente: **0 (bancada e fundações)**.

## Fase 0

| Critério | Estado | Fonte |
| --- | --- | --- |
| H01 a H14, H23 a H27 com veredito | 3 de 19 (H02 e H24 confirmadas, H01 parcial); H12 em execução | eu, 2026-10-23 |
| Substrato gráfico e de texto escolhido pela medida | NÃO MEDIDO | |
| Harness do oráculo com um teste mocha portado e um screenshot comparado | NÃO MEDIDO | |
| `scripts/pure-rust-check.sh` verde nos 4 alvos | NÃO MEDIDO | |

## Fases 1 a 8

Nenhum critério medido ainda; os tamanhos de cada camada estão no `roadmap.md` (eu, 2026-10-23).

## Registro

- 2026-10-23: workspace criado; oráculo 1.143.1 baixado e fonte na mesma tag; sondas H01 e H02 escritas.
- 2026-10-23: H02 confirmada (58,9 fps com vsync no Wayland do WSLg sobre a RTX via GL/D3D12, 194 fps no X11); H01 parcial (GPU só com `MESA_LOADER_DRIVER_OVERRIDE=d3d12`); H27 criada.
- 2026-10-23: H14 dimensionada (243 chamadas de subexpressão nas gramáticas: motor Oniguruma próprio); H25 dimensionada (2 209 testes em `vs/base`, 1 533 `common`).
