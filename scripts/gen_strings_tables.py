#!/usr/bin/env python3
"""Gera `crates/je-base/src/strings_tables.rs` a partir de `vs/base/common/strings.ts`.

Extrai do original, sem digitar nada à mão: os dados de grapheme break (`getGraphemeBreakRawData`),
os caracteres ambíguos (`AmbiguousCharacters`), os invisíveis (`InvisibleCharacters`), as faixas da
classe RTL de `makeContainsRtl` e as faixas de `isEmojiImprecise`.

Uso: `python3 -I scripts/gen_strings_tables.py`
"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / 'ref' / 'vscode' / 'src' / 'vs' / 'base' / 'common' / 'strings.ts'
OUT = ROOT / 'crates' / 'je-base' / 'src' / 'strings_tables.rs'


def json_literals(text):
    """Os literais de `JSON.parse('...')` do arquivo, na ordem, já com as escapes do TS resolvidas."""
    found = []
    for match in re.finditer(r"JSON\.parse\(\s*'((?:[^'\\]|\\.)*)'\s*\)", text):
        raw = match.group(1)
        unknown = set(re.findall(r'\\(.)', raw)) - {'"'}
        if unknown:
            sys.exit(f'escape inesperada no literal JSON: {unknown}')
        found.append(json.loads(raw.replace('\\"', '"')))
    return found


def function_body(text, header):
    start = text.index(header)
    depth = 0
    for index in range(text.index('{', start), len(text)):
        if text[index] == '{':
            depth += 1
        elif text[index] == '}':
            depth -= 1
            if depth == 0:
                return text[start:index + 1]
    sys.exit(f'corpo de {header} não termina')


def unit(token):
    return int(token[2:], 16)


def class_ranges(body):
    """Itens `\\uXXXX` ou `\\uXXXX-\\uXXXX` de uma classe de caracteres, como (início, fim)."""
    ranges = []
    for first, last in re.findall(r'(\\u[0-9A-Fa-f]{4})(?:-(\\u[0-9A-Fa-f]{4}))?', body):
        ranges.append((unit(first), unit(last or first)))
    return ranges


def rtl_ranges(text):
    """Faixas de code point da regex de `makeContainsRtl`: a classe BMP e os pares de surrogates."""
    body = function_body(text, 'function makeContainsRtl')
    source = re.search(r'return /\(\?:(.*)\)/;', body).group(1)
    ranges = []
    for alt in source.split(']|'):
        alt = alt.rstrip(']')
        if alt.startswith('['):
            ranges += class_ranges(alt[1:])
        else:
            high_token, low_class = alt.split('[', 1)
            high = unit(high_token)
            for low_first, low_last in class_ranges(low_class):
                base = 0x10000 + ((high - 0xD800) << 10) - 0xDC00
                ranges.append((base + low_first, base + low_last))
    return ranges


def emoji_ranges(text):
    """Faixas de `isEmojiImprecise`: cada termo é `x >= A && x <= B` ou `x === N`."""
    body = function_body(text, 'export function isEmojiImprecise')
    ranges = []
    for term in re.findall(r'\(([^()]*)\)', body.split('return', 1)[1]):
        match = re.fullmatch(r'x >= (\w+) && x <= (\w+)', term.strip())
        if match:
            ranges.append((int(match.group(1), 0), int(match.group(2), 0)))
            continue
        match = re.fullmatch(r'x === (\w+)', term.strip())
        if match:
            ranges.append((int(match.group(1), 0),) * 2)
    return ranges


def rust_numbers(values):
    return ', '.join(str(v) for v in values)


def main():
    text = SOURCE.read_text(encoding='utf-8')
    grapheme, ambiguous, invisible = json_literals(text)
    if len(grapheme) % 3 or not all(isinstance(v, int) for v in grapheme):
        sys.exit('dados de grapheme break fora do formato esperado')
    version = json.loads((ROOT / 'ref' / 'vscode' / 'package.json').read_text(encoding='utf-8'))['version']
    lines = [
        '// Arquivo gerado por `scripts/gen_strings_tables.py` a partir de `src/vs/base/common/strings.ts`',
        f'// (VS Code {version}). Não editar: rode `python3 -I scripts/gen_strings_tables.py`.',
        '',
        '/// Árvore binária de intervalos do grapheme break, em ternos (início, fim, tipo), como em',
        '/// `getGraphemeBreakRawData`. O terno 0 é descartado pela busca, que começa no nó 1.',
        f'pub static GRAPHEME_BREAK_RAW_DATA: [u32; {len(grapheme)}] = [{rust_numbers(grapheme)}];',
        '',
        '/// Pares (código do ponto, ASCII equivalente) achatados, por locale, na ordem do original.',
        f'pub static AMBIGUOUS_CHARACTER_DATA: [(&str, &[u32]); {len(ambiguous)}] = [',
    ]
    lines += [f'    ({json.dumps(key)}, &[{rust_numbers(values)}]),' for key, values in ambiguous.items()]
    lines += ['];', '', '/// Code points invisíveis, por locale, na ordem do original.',
              f'pub static INVISIBLE_CHARACTER_DATA: [(&str, &[u32]); {len(invisible)}] = [']
    lines += [f'    ({json.dumps(key)}, &[{rust_numbers(values)}]),' for key, values in invisible.items()]
    rtl = rtl_ranges(text)
    emoji = emoji_ranges(text)
    lines += ['];', '', '/// Faixas de code point classificadas como "R" ou "AL" (a regex de `makeContainsRtl`).',
              f'pub static RTL_RANGES: [(u32, u32); {len(rtl)}] = [' + ', '.join(f'({a}, {b})' for a, b in rtl) + '];',
              '', '/// Faixas de `isEmojiImprecise`.',
              f'pub static EMOJI_IMPRECISE_RANGES: [(u32, u32); {len(emoji)}] = [' + ', '.join(f'({a}, {b})' for a, b in emoji) + '];',
              '']
    OUT.write_text('\n'.join(lines), encoding='utf-8')
    print(f'{OUT.relative_to(ROOT)}: {len(grapheme) // 3} nós, {len(ambiguous)} locales ambíguos, '
          f'{len(invisible)} invisíveis, {len(rtl)} faixas RTL, {len(emoji)} faixas de emoji')


if __name__ == '__main__':
    main()
