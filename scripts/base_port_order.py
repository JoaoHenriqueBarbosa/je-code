#!/usr/bin/env python3
"""Ordem de porte de vs/base/common: grafo de imports entre os módulos, em camadas (cada camada só
depende das anteriores), com o estado de cada um (portado se existe crates/je-base/src/<snake>.rs).
Escreve wip/notes/base-port-order.md."""
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent
COMMON = ROOT / 'ref' / 'vscode' / 'src' / 'vs' / 'base' / 'common'
SRC = ROOT / 'crates' / 'je-base' / 'src'
IMPORT = re.compile(r"from '\./([A-Za-z0-9_]+)\.js'")


def snake(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()


mods = {p.stem: p for p in COMMON.glob('*.ts')}
deps = {m: sorted(set(IMPORT.findall(p.read_text(encoding='utf-8'))) & set(mods)) for m, p in mods.items()}
layers, placed = [], set()
remaining = set(mods)
while remaining:
    layer = sorted(m for m in remaining if set(deps[m]) <= placed)
    if not layer:  # ciclo: entra tudo o que sobrou numa camada só
        layer = sorted(remaining)
    layers.append(layer)
    placed |= set(layer)
    remaining -= set(layer)
lines = ['# Ordem de porte de `vs/base/common`', '', 'Gerado por `scripts/base_port_order.py`. [x] = portado em `crates/je-base`.', '']
for i, layer in enumerate(layers):
    lines.append(f'## Camada {i}')
    for m in layer:
        mark = 'x' if (SRC / f'{snake(m)}.rs').exists() else ' '
        n = sum(1 for _ in mods[m].open(encoding='utf-8'))
        lines.append(f'- [{mark}] `{m}` ({n} linhas){": " + ", ".join(deps[m]) if deps[m] else ""}')
    lines.append('')
(ROOT / 'wip' / 'notes' / 'base-port-order.md').write_text('\n'.join(lines), encoding='utf-8')
print(f'{len(mods)} módulos em {len(layers)} camadas; portados: {sum(1 for m in mods if (SRC / f"{snake(m)}.rs").exists())}')
