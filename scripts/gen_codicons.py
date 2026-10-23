#!/usr/bin/env python3
"""Gera crates/je-codicons/src/table.rs a partir de codiconsLibrary.ts e codicons.ts (VS Code 1.143.1).

`register(id, 0xNNNN)` vira (id, code point); `register(id, 'outro-id')` (derivado) herda o code point
do id citado. Qualquer linha `register(` que o gerador não entender faz ele falhar.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASE = ROOT / 'ref' / 'vscode' / 'src' / 'vs' / 'base' / 'common'
REG = re.compile(r"register\('([a-z0-9-]+)',\s*(0x[0-9a-fA-F]+|'([a-z0-9-]+)')\)")


def main():
    table = {}
    order = []
    for name in ('codiconsLibrary.ts', 'codicons.ts'):
        for line in (BASE / name).read_text(encoding='utf-8').splitlines():
            if 'register(' not in line or line.lstrip().startswith(('import', '//', 'export function')):
                continue
            m = REG.search(line)
            if not m:
                print(f'{name}: linha não reconhecida: {line.strip()}', file=sys.stderr)
                return 1
            ident, raw, ref = m.group(1), m.group(2), m.group(3)
            if ref is not None:
                if ref not in table:
                    print(f'{name}: {ident} referencia {ref}, que não existe', file=sys.stderr)
                    return 1
                code = table[ref]
            else:
                code = int(raw, 16)
            if ident not in table:
                order.append(ident)
            table[ident] = code
    out = ['// Gerado por scripts/gen_codicons.py a partir de codiconsLibrary.ts e codicons.ts', '// (VS Code 1.143.1). Não editar à mão.', '',
           f'/// (id, code point) de todos os codicons, na ordem de registro do original ({len(order)} ids).',
           'pub(crate) const CODICONS: &[(&str, u32)] = &[']
    out += [f'    ("{i}", 0x{table[i]:04x}),' for i in order]
    out.append('];')
    (ROOT / 'crates' / 'je-codicons' / 'src' / 'table.rs').write_text('\n'.join(out) + '\n', encoding='utf-8')
    print(f'{len(order)} codicons')
    return 0


if __name__ == '__main__':
    sys.exit(main())
