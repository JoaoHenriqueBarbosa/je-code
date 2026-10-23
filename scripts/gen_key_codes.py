#!/usr/bin/env python3
"""Gera crates/je-base/src/key_codes_tables.rs a partir de keyCodes.ts (VS Code 1.143.1).

Extrai do original os enums `KeyCode` e `ScanCode` (com os valores que o TypeScript atribui por
contagem) e a tabela `mappings` (uma linha por tecla, nove colunas). Qualquer linha dessas regiões
que o gerador não entender faz ele falhar, em vez de seguir com uma tabela incompleta.

Uso: python3 -I scripts/gen_key_codes.py
"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / 'ref' / 'vscode' / 'src' / 'vs' / 'base' / 'common' / 'keyCodes.ts'
OUTPUT = ROOT / 'crates' / 'je-base' / 'src' / 'key_codes_tables.rs'

STR = r"'(?:[^'\\]|\\.)*'"
MEMBER = re.compile(r'^(\w+)(?:\s*=\s*(-?\d+))?,?$')
ENTRY = re.compile(
    r'^\[(0|1), ScanCode\.(\w+), (' + STR + r'|empty), KeyCode\.(\w+), (' + STR + r'|empty), (\d+), '
    r'(' + STR + r'|empty), (' + STR + r'|empty), (' + STR + r'|empty)\],?$'
)


class Unsupported(Exception):
    pass


def rust_name(name):
    """`OEM_8` vira `Oem8`; nomes já em CamelCase (`KeyA`, `F1`) ficam como estão."""
    if '_' not in name:
        return name
    return ''.join(part.capitalize() for part in name.split('_'))


def strip_comments(text):
    text = re.sub(r'/\*.*?\*/', '', text, flags=re.DOTALL)
    return re.sub(r'//.*$', '', text, flags=re.MULTILINE)


def parse_enum(source, name):
    head = f'export const enum {name} {{'
    start = source.find(head)
    if start < 0:
        raise Unsupported(f'enum {name} não encontrado')
    start += len(head)
    end = source.index('\n}', start)
    members = []
    value = 0
    for raw in strip_comments(source[start:end]).splitlines():
        line = raw.strip()
        if not line:
            continue
        m = MEMBER.match(line)
        if not m:
            raise Unsupported(f'enum {name}: linha não reconhecida: {line}')
        if m.group(2) is not None:
            value = int(m.group(2))
        members.append((m.group(1), value))
        value += 1
    names = [n for n, _ in members]
    if len(set(names)) != len(names):
        raise Unsupported(f'enum {name}: membro repetido')
    return members


def unquote(token):
    if token == 'empty':
        return ''
    body = token[1:-1]
    out = []
    i = 0
    while i < len(body):
        if body[i] == '\\':
            if body[i + 1] not in "\\'":
                raise Unsupported(f'escape não suportado em {token}')
            out.append(body[i + 1])
            i += 2
        else:
            out.append(body[i])
            i += 1
    return ''.join(out)


def parse_mappings(source):
    head = 'const mappings: IMappingEntry[] = ['
    start = source.find(head)
    if start < 0:
        raise Unsupported('tabela mappings não encontrada')
    start += len(head)
    end = source.index('\n\t];', start)
    entries = []
    for raw in source[start:end].splitlines():
        line = re.sub(r'\s*//.*$', '', raw).strip()
        if not line:
            continue
        m = ENTRY.match(line)
        if not m:
            raise Unsupported(f'mappings: linha não reconhecida: {line}')
        immutable, scan, scan_str, key, key_str, event, vkey, us, general = m.groups()
        entries.append((int(immutable), scan, unquote(scan_str), key, unquote(key_str), int(event),
                        unquote(vkey), unquote(us), unquote(general)))
    return entries


def emit_enum(out, name, members):
    out.append('#[allow(non_upper_case_globals)]')
    out.append(f'impl {name} {{')
    for member, value in members:
        out.append(f'    pub const {rust_name(member)}: {name} = {name}({value});')
    out.append('}')
    out.append('')


def main():
    source = SOURCE.read_text(encoding='utf-8')
    key_codes = parse_enum(source, 'KeyCode')
    scan_codes = parse_enum(source, 'ScanCode')
    entries = parse_mappings(source)
    key_names = {n for n, _ in key_codes}
    scan_names = {n for n, _ in scan_codes}
    for entry in entries:
        if entry[1] not in scan_names or entry[3] not in key_names:
            raise Unsupported(f'mappings referencia código inexistente: {entry}')

    out = [
        '// Arquivo gerado por `scripts/gen_key_codes.py` a partir de `src/vs/base/common/keyCodes.ts`',
        '// (VS Code 1.143.1). Não editar: rode `python3 -I scripts/gen_key_codes.py`.',
        '',
        'use crate::key_codes::{KeyCode, KeyCodeMapping, ScanCode};',
        '',
    ]
    emit_enum(out, 'KeyCode', key_codes)
    emit_enum(out, 'ScanCode', scan_codes)
    out.append('/// A tabela `mappings` do original, na ordem dela.')
    out.append(f'pub(crate) static MAPPINGS: [KeyCodeMapping; {len(entries)}] = [')
    for immutable, scan, scan_str, key, key_str, event, vkey, us, general in entries:
        out.append(
            f'    KeyCodeMapping {{ immutable: {"true" if immutable else "false"}, '
            f'scan_code: ScanCode::{rust_name(scan)}, scan_code_str: {json.dumps(scan_str)}, '
            f'key_code: KeyCode::{rust_name(key)}, key_code_str: {json.dumps(key_str)}, '
            f'event_key_code: {event}, vkey: {json.dumps(vkey)}, '
            f'us_user_settings_label: {json.dumps(us)}, general_user_settings_label: {json.dumps(general)} }},'
        )
    out.append('];')
    out.append('')
    OUTPUT.write_text('\n'.join(out), encoding='utf-8')
    print(f'{OUTPUT.relative_to(ROOT)}: {len(key_codes)} KeyCode, {len(scan_codes)} ScanCode, {len(entries)} mapeamentos')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Unsupported as error:
        print(f'gen_key_codes: {error}', file=sys.stderr)
        sys.exit(1)
