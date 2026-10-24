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
OUT_OF_SCOPE = ['sessions', 'platform/agentHost', 'platform/github', 'platform/mcp', 'platform/agentPlugins',
                'platform/customizationMarketplace', 'workbench/contrib/chat', 'workbench/contrib/inlineChat',
                'workbench/contrib/mcp', 'workbench/contrib/welcomeAgentSessions',
                'workbench/contrib/remoteCodingAgents', 'workbench/contrib/agentsVoice']
# Cada camada é (nome, pasta sob `ref/vscode`, chave das origens). `workbench/test` e o resto de `vs/workbench` fora de
# `services`, `api` e `contrib` entram como `workbench (shell)`; as extensões embutidas (menos a `copilot`, fora de
# escopo) entram à parte porque rodam no extension host.
LAYERS = [(name, f'src/vs/{name}') for name in
          ['base', 'platform', 'editor', 'workbench/services', 'workbench/api', 'workbench/contrib', 'server', 'code']]
LAYERS.insert(6, ('workbench (shell)', 'src/vs/workbench'))
LAYERS.append(('extensions', 'extensions'))
SHELL_EXCLUDE = ['workbench/services', 'workbench/api', 'workbench/contrib']
EXTENSIONS_OUT_OF_SCOPE = ['extensions/copilot/']
TEST_RE = re.compile(r'^\s*test\(', re.M)
PORT_RE = re.compile(r'Porte de `((?:src/vs|extensions)/[^`]+\.test\.ts)`')


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
    source = VS.parent.parent
    for layer, folder in LAYERS:
        def in_scope(p):
            key = str(p.relative_to(source))
            rel = str(p.relative_to(VS)) if key.startswith('src/vs/') else ''
            if 'node_modules' in p.parts or any(key.startswith(o) for o in EXTENSIONS_OUT_OF_SCOPE):
                return False
            if layer == 'workbench (shell)' and any(rel.startswith(o + '/') for o in SHELL_EXCLUDE):
                return False
            return not any(rel.startswith(o + '/') for o in OUT_OF_SCOPE)
        files = sorted(p for p in (source / folder).rglob('*.test.ts') if in_scope(p))
        cases = ported_cases = ported_files = 0
        for f in files:
            n = len(TEST_RE.findall(f.read_text(encoding='utf-8', errors='replace')))
            cases += n
            key = str(f.relative_to(source))
            if key in done:
                # Um laço do original que vira vários `#[test]` não conta além dos casos do oráculo.
                counted = min(n, done[key][1])
                ported_files += 1
                ported_cases += counted
                detail.append(f'| `{key}` | {n} | `{done[key][0]}` | {counted} |')
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
