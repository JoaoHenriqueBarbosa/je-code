#!/usr/bin/env python3
"""Placar de conformidade: testes mocha do oráculo contra testes portados.

Para cada `*.test.ts` de `ref/vscode/src/vs/<camada>` conta os `test(` (casos do oráculo) e procura o
arquivo portado correspondente em `crates/*/tests/<nome>.rs`, contando os `#[test]` dele. Um arquivo
portado declara o arquivo de origem na primeira linha de doc (`Porte de \\`src/vs/.../x.test.ts\\``).
Escreve `docs/conformance.md`. Casos portados só contam se `cargo test` passa: o placar não roda os
testes, ele lê a contagem; a CI roda `cargo test` antes.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VS = ROOT / 'ref' / 'vscode' / 'src' / 'vs'
OUT_OF_SCOPE = ['sessions', 'workbench/contrib/chat', 'workbench/contrib/welcomeAgentSessions',
                'workbench/contrib/remoteCodingAgents', 'workbench/contrib/agentsVoice']
LAYERS = ['base', 'platform', 'editor', 'workbench/services', 'workbench/api', 'workbench/contrib', 'server', 'code']
TEST_RE = re.compile(r'^\s*test\(', re.M)
PORT_RE = re.compile(r'Porte de `(src/vs/[^`]+\.test\.ts)`')


def ported():
    found = {}
    for path in ROOT.glob('crates/*/tests/*.rs'):
        text = path.read_text(encoding='utf-8')
        match = PORT_RE.search(text)
        if match:
            found[match.group(1)] = (path.relative_to(ROOT), len(re.findall(r'#\[test\]', text)))
    return found


def main():
    done = ported()
    lines = ['# Placar de conformidade', '',
             'Gerado por `scripts/conformance.py`. Oráculo: testes mocha do VS Code 1.143.1; portado: `#[test]` '
             'nos arquivos que declaram a origem. Pastas fora de escopo (decisão do user) não entram.', '',
             '| Camada | Arquivos de teste | Casos do oráculo | Arquivos portados | Casos portados | % |',
             '| --- | --- | --- | --- | --- | --- |']
    total_cases = total_ported = 0
    detail = []
    for layer in LAYERS:
        files = sorted(p for p in (VS / layer).rglob('*.test.ts')
                       if not any(str(p.relative_to(VS)).startswith(o + '/') for o in OUT_OF_SCOPE))
        cases = ported_cases = ported_files = 0
        for f in files:
            n = len(TEST_RE.findall(f.read_text(encoding='utf-8', errors='replace')))
            cases += n
            key = 'src/vs/' + str(f.relative_to(VS))
            if key in done:
                ported_files += 1
                ported_cases += done[key][1]
                detail.append(f'| `{key}` | {n} | `{done[key][0]}` | {done[key][1]} |')
        total_cases += cases
        total_ported += ported_cases
        pct = 100.0 * ported_cases / cases if cases else 0.0
        lines.append(f'| `{layer}` | {len(files)} | {cases} | {ported_files} | {ported_cases} | {pct:.2f} |')
    lines.append(f'| **total** | | {total_cases} | | {total_ported} | {100.0 * total_ported / total_cases:.2f} |')
    lines += ['', '## Arquivos portados', '', '| Origem | Casos do oráculo | Porte | Casos portados |', '| --- | --- | --- | --- |']
    lines += detail
    (ROOT / 'docs' / 'conformance.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print('\n'.join(lines[4:4 + len(LAYERS) + 3]))


if __name__ == '__main__':
    sys.exit(main())
