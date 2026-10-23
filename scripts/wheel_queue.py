#!/usr/bin/env python3
"""Fila medida da roda de sonnets: um item por `.test.ts` ainda não portado e dentro do escopo, com os
caminhos EXATOS que o agente precisa (o agente só lê e escreve, sem Grep nem Glob): o teste, os `.ts` de
produção que ele importa (resolvidos para caminho real, com linhas), os helpers de teste, o destino
(crate, arquivo de teste e módulo) e o tamanho. Escreve wip/notes/wheel-queue.json e imprime o resumo.
Uso: wheel_queue.py [prefixo de área, ex. editor/test]"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VS = ROOT / 'ref' / 'vscode' / 'src' / 'vs'
OUT = ['sessions', 'platform/agentHost', 'platform/github', 'platform/mcp', 'platform/agentPlugins',
       'platform/customizationMarketplace', 'workbench/contrib/chat', 'workbench/contrib/inlineChat',
       'workbench/contrib/mcp', 'workbench/contrib/welcomeAgentSessions',
       'workbench/contrib/remoteCodingAgents', 'workbench/contrib/agentsVoice']
CRATE = [('base/', 'je-base'), ('platform/', 'je-platform'), ('editor/', 'je-editor'),
         ('workbench/', 'je-workbench'), ('server/', 'je-server'), ('code/', 'je-code')]
IMPORT = re.compile(r"from '(\.[^']+)\.js'")


def snake(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).replace('.', '_').lower()


def done():
    found = set()
    for p in ROOT.glob('crates/*/tests/*.rs'):
        m = re.search(r'Porte de `(src/vs/[^`]+\.test\.ts)`', p.read_text(encoding='utf-8', errors='replace')[:400])
        if m:
            found.add(m.group(1))
    return found


def main():
    prefix = sys.argv[1] if len(sys.argv) > 1 else ''
    finished = done()
    queue = []
    for test in sorted(VS.rglob('*.test.ts')):
        rel = test.relative_to(VS).as_posix()
        if any(rel.startswith(o + '/') for o in OUT) or not rel.startswith(prefix):
            continue
        key = 'src/vs/' + rel
        text = test.read_text(encoding='utf-8', errors='replace')
        cases = len(re.findall(r'^\s*test\(', text, re.M))
        if key in finished or cases == 0:
            continue
        sources, helpers = [], []
        for imp in IMPORT.findall(text):
            target = (test.parent / imp).resolve()
            path = target.with_suffix('.ts')
            if not path.exists():
                continue
            entry = {'path': path.relative_to(ROOT).as_posix(), 'lines': sum(1 for _ in path.open(encoding='utf-8', errors='replace'))}
            (helpers if '/test/' in entry['path'] else sources).append(entry)
        crate = next((c for p, c in CRATE if rel.startswith(p)), 'je-platform')
        queue.append({
            'test': test.relative_to(ROOT).as_posix(), 'cases': cases,
            'test_lines': sum(1 for _ in text.splitlines()),
            'sources': sources, 'helpers': helpers,
            'dest_test': f'crates/{crate}/tests/{snake(test.name.replace(".test.ts", ""))}.rs',
            'dest_crate': crate,
            'source_lines': sum(s['lines'] for s in sources),
        })
    (ROOT / 'wip' / 'notes' / 'wheel-queue.json').write_text(json.dumps(queue, ensure_ascii=False, indent=1) + '\n', encoding='utf-8')
    print(f'{len(queue)} itens, {sum(q["cases"] for q in queue)} casos, {sum(q["source_lines"] for q in queue)} linhas de produção')


if __name__ == '__main__':
    main()
