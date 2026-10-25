#!/usr/bin/env python3
"""Placar de conformidade: testes mocha do oráculo contra testes portados.

Para cada `*.test.ts` de `ref/vscode/src/vs/<camada>` conta os `test(` (casos do oráculo) e procura o
arquivo portado correspondente em `crates/*/tests/<nome>.rs`, contando os `#[test]` dele. Um arquivo
portado declara o arquivo de origem na primeira linha de doc (`Porte de \\`src/vs/.../x.test.ts\\``).
A contagem vem do binário de teste compilado (`--list` menos `--ignored`), que inclui os casos gerados por
macro; sem binário atualizado, vale a contagem dos `#[test]` do texto. Escreve `docs/conformance.md`. Casos
portados só contam se `cargo test` passa: o placar não roda os testes, ele lê a contagem; rode `cargo test`
(com o mesmo `CARGO_TARGET_DIR`) antes.
"""
import os
import pathlib
import re
import subprocess
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
# Num arquivo que declara um auxiliar local `function test(...)` (o `keyCodes.test.ts`), as chamadas desse auxiliar não são casos
# do mocha: ali só conta a chamada `test(` com o nome do caso em texto literal.
LOCAL_TEST_HELPER_RE = re.compile(r'\bfunction\s+test\s*\(')
NAMED_TEST_RE = re.compile(r'^\s*test\(\s*[\'"`]', re.M)


def oracle_cases(text):
    """Os casos do mocha de um arquivo de teste do oráculo."""
    if LOCAL_TEST_HELPER_RE.search(text):
        return len(NAMED_TEST_RE.findall(text))
    return len(TEST_RE.findall(text))
PORT_RE = re.compile(r'Porte de `((?:src/vs|extensions)/[^`]+\.test\.ts)`')


SUITE_MACRO_RE = re.compile(r'text_file_service_io_cases!\(\s*[^;)]+\)')
SUITE_MACRO_FILE = ROOT / 'crates/je-workbench/tests/support/text_file_service_io_suite.rs'


def macro_cases(text):
    """Casos que um teste gera por macro de suíte: a lista de casos da macro, a fonte única (não são `#[test]` literais no arquivo)."""
    if not SUITE_MACRO_RE.search(text) or not SUITE_MACRO_FILE.exists():
        return 0
    body = SUITE_MACRO_FILE.read_text(encoding='utf-8')
    start = body.index('(@each $accessor_factory;', body.index('macro_rules! text_file_service_io_cases'))
    block = body[start:body.index(');', start)]
    return len(re.findall(r'^\s+([a-z_0-9]+)\s*$', block, re.M))


def listed_cases(path):
    """Os casos ativos que o binário de teste compilado lista (`--list` menos os `--ignored`), que conta também os casos que
    uma macro gera. `None` quando não há binário: aí vale a contagem do texto. O diretório dos binários vem de
    `CARGO_TARGET_DIR` (o mesmo do `cargo test` que rodou antes) ou do `target/` do repositório."""
    target = pathlib.Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')) / 'debug' / 'deps'
    if not target.is_dir():
        return None
    stem = path.stem
    # O binário se chama `<nome>-<hash>`: com o mesmo nome de teste em dois crates não há como saber qual é qual
    if sum(1 for _ in ROOT.glob(f'crates/*/tests/{stem}.rs')) > 1:
        return None
    binaries = [b for b in target.glob(f'{stem}-*') if b.is_file() and os.access(b, os.X_OK) and b.suffix == '']
    if not binaries:
        return None
    binary = max(binaries, key=lambda b: b.stat().st_mtime)
    if binary.stat().st_mtime < path.stat().st_mtime:
        return None  # binário mais velho que o teste: não reflete o arquivo atual
    def count(*args):
        out = subprocess.run([str(binary), '--list', '--format', 'terse', *args], capture_output=True, text=True, timeout=60)
        return sum(1 for line in out.stdout.splitlines() if line.endswith(': test'))
    try:
        return count() - count('--ignored')
    except (OSError, subprocess.TimeoutExpired):
        return None


def ported():
    found = {}
    for path in ROOT.glob('crates/*/tests/*.rs'):
        text = path.read_text(encoding='utf-8')
        match = PORT_RE.search(text)
        if match:
            # `#[test]` seguido de `#[ignore...]` é caso pendente (falha conhecida registrada): não conta como portado
            active = listed_cases(path)
            if active is None:
                active = len(re.findall(r'#\[test\]\n(?!\s*#\[ignore)', text)) + macro_cases(text)
            key = match.group(1)
            # Dois arquivos podem portar o mesmo arquivo de origem (a suíte `createSuite` de i/o roda em dois serviços): vale o maior.
            if key not in found or active > found[key][1]:
                found[key] = (path.relative_to(ROOT), active)
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
            n = oracle_cases(f.read_text(encoding='utf-8', errors='replace'))
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
