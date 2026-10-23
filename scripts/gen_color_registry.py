#!/usr/bin/env python3
"""Gera `crates/je-theme/src/registered_colors.rs` a partir das chamadas `registerColor(...)` do VS Code.

Fontes (relativas a `ref/vscode/src/vs`): `platform/theme/common/colors/*.ts` (na ordem de
`colorRegistry.ts`) e `workbench/common/theme.ts`.

Uso (a partir da raiz do repositório):

    python3 -I scripts/gen_color_registry.py            # escreve o arquivo gerado
    python3 -I scripts/gen_color_registry.py --check    # só confere se o arquivo versionado está em dia

O gerador não chuta: tudo que ele não consegue traduzir com certeza (expressão fora do subconjunto
conhecido, referência sem definição, `registerColor` fora de uma declaração `const`, cor com hexadecimal
inválido, identificador duplicado, referência a cor não registrada) vira uma linha na lista de erros,
e o programa sai com código 1 sem escrever nada.
"""

import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VSCODE = os.path.join(ROOT, "ref", "vscode")
VS = os.path.join(VSCODE, "src", "vs")
OUT = os.path.join(ROOT, "crates", "je-theme", "src", "registered_colors.rs")

COLORS_DIR = "platform/theme/common/colors"
# Ordem das exportações de `colorRegistry.ts`, e por fim `workbench/common/theme.ts`.
SOURCES = [
    f"{COLORS_DIR}/baseColors.ts",
    f"{COLORS_DIR}/chartsColors.ts",
    f"{COLORS_DIR}/editorColors.ts",
    f"{COLORS_DIR}/inputColors.ts",
    f"{COLORS_DIR}/listColors.ts",
    f"{COLORS_DIR}/menuColors.ts",
    f"{COLORS_DIR}/minimapColors.ts",
    f"{COLORS_DIR}/miscColors.ts",
    f"{COLORS_DIR}/quickpickColors.ts",
    f"{COLORS_DIR}/searchColors.ts",
    "workbench/common/theme.ts",
]
THEME_KEYS = ("light", "dark", "hcDark", "hcLight")
NAMED_COLORS = {
    "white": (255, 255, 255, 1),
    "black": (0, 0, 0, 1),
    "red": (255, 0, 0, 1),
    "blue": (0, 0, 255, 1),
    "green": (0, 255, 0, 1),
    "cyan": (0, 255, 255, 1),
    "lightgrey": (211, 211, 211, 1),
    "transparent": (0, 0, 0, 0),
}


class GenError(Exception):
    pass


# ---------------------------------------------------------------- tokenizer

TOKEN_RE = re.compile(
    r"""
    (?P<ws>\s+)
  | (?P<lcomment>//[^\n]*)
  | (?P<bcomment>/\*.*?\*/)
  | (?P<id>[A-Za-z_$][A-Za-z0-9_$]*)
  | (?P<num>(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)
  | (?P<str>'(?:[^'\\\n]|\\.)*'|"(?:[^"\\\n]|\\.)*")
  | (?P<tpl>`)
  | (?P<punct>=>|===|!==|==|!=|&&|\|\||[(){}\[\],:;.+\-*/=<>?!&|])
    """,
    re.VERBOSE | re.DOTALL,
)

ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "'": "'", '"': '"', "\\": "\\"}


def unescape(raw):
    body = raw[1:-1]
    out = []
    i = 0
    while i < len(body):
        c = body[i]
        if c == "\\":
            nxt = body[i + 1]
            if nxt not in ESCAPES:
                raise GenError(f"escape não suportado em string: \\{nxt}")
            out.append(ESCAPES[nxt])
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def tokenize(src):
    toks = []
    pos = 0
    line = 1
    while pos < len(src):
        m = TOKEN_RE.match(src, pos)
        if not m:
            raise GenError(f"caractere inesperado na linha {line}: {src[pos]!r}")
        kind = m.lastgroup
        text = m.group()
        if kind == "tpl":
            raise GenError(f"template literal na linha {line} não é suportado")
        if kind not in ("ws", "lcomment", "bcomment"):
            if kind == "str":
                toks.append(("str", unescape(text), line))
            elif kind == "num":
                toks.append(("num", float(text), line))
            else:
                toks.append((kind, text, line))
        line += text.count("\n")
        pos = m.end()
    return toks


# ---------------------------------------------------------------- parser (subconjunto de TypeScript)


class Parser:
    def __init__(self, toks, pos):
        self.toks = toks
        self.pos = pos

    def peek(self, k=0):
        i = self.pos + k
        return self.toks[i] if i < len(self.toks) else ("eof", None, 0)

    def next(self):
        t = self.peek()
        self.pos += 1
        return t

    def is_p(self, value, k=0):
        t = self.peek(k)
        return t[0] == "punct" and t[1] == value

    def expect_p(self, value):
        t = self.next()
        if not (t[0] == "punct" and t[1] == value):
            raise GenError(f"linha {t[2]}: esperado {value!r}, achei {t[1]!r}")

    def expression(self):
        left = self.postfix()
        # `+` entre strings (descrições concatenadas)
        while self.is_p("+"):
            self.next()
            right = self.postfix()
            if left[0] == "str" and right[0] == "str":
                left = ("str", left[1] + right[1])
            elif left[0] == "num" and right[0] == "num":
                left = ("num", left[1] + right[1])
            else:
                raise GenError(f"linha {self.peek()[2]}: '+' só é suportado entre strings ou entre números")
        return left

    def postfix(self):
        node = self.primary()
        while True:
            if self.is_p("."):
                self.next()
                t = self.next()
                if t[0] != "id":
                    raise GenError(f"linha {t[2]}: esperado nome depois de '.'")
                node = ("member", node, t[1])
            elif self.is_p("("):
                node = ("call", node, self.arguments())
            else:
                return node

    def arguments(self):
        self.expect_p("(")
        args = []
        while not self.is_p(")"):
            args.append(self.expression())
            if self.is_p(","):
                self.next()
            elif not self.is_p(")"):
                raise GenError(f"linha {self.peek()[2]}: esperado ',' ou ')' nos argumentos")
        self.expect_p(")")
        return args

    def primary(self):
        t = self.next()
        kind, value, line = t
        if kind == "str":
            return ("str", value)
        if kind == "num":
            return ("num", value)
        if kind == "punct" and value == "-":
            inner = self.postfix()
            if inner[0] != "num":
                raise GenError(f"linha {line}: '-' só é suportado antes de número")
            return ("num", -inner[1])
        if kind == "punct" and value == "(":
            inner = self.expression()
            self.expect_p(")")
            return inner
        if kind == "punct" and value == "{":
            return self.object_literal()
        if kind == "id":
            if value == "null":
                return ("null",)
            if value == "undefined":
                return ("undefined",)
            if value in ("true", "false"):
                return ("bool", value == "true")
            if value == "new":
                callee = self.next()
                if callee[0] != "id":
                    raise GenError(f"linha {line}: esperado nome depois de 'new'")
                return ("new", callee[1], self.arguments())
            return ("id", value)
        raise GenError(f"linha {line}: expressão não suportada começando em {value!r}")

    def object_literal(self):
        entries = []
        while not self.is_p("}"):
            key = self.next()
            if key[0] not in ("id", "str"):
                raise GenError(f"linha {key[2]}: chave de objeto não suportada: {key[1]!r}")
            self.expect_p(":")
            entries.append((key[1], self.expression()))
            if self.is_p(","):
                self.next()
            elif not self.is_p("}"):
                raise GenError(f"linha {self.peek()[2]}: esperado ',' ou '}}' no objeto")
        self.expect_p("}")
        return ("obj", entries)


def statement_end(toks, pos):
    """Índice do `;` (ou do `}` que fecha um bloco) que termina a instrução que começa em `pos`."""
    depth = 0
    i = pos
    while i < len(toks):
        kind, value, _ = toks[i]
        if kind == "punct":
            if value in "({[":
                depth += 1
            elif value in ")}]":
                depth -= 1
                if depth == 0 and value == "}" and not (i + 1 < len(toks) and toks[i + 1][1] in (";", ",")):
                    return i
            elif value == ";" and depth == 0:
                return i
        i += 1
    return len(toks) - 1


# ---------------------------------------------------------------- coleta

class Collected:
    def __init__(self):
        self.consts = {}  # nome -> (ast, arquivo, linha) de const que não é registerColor
        self.registered = {}  # nome -> ast do registerColor
        self.order = []  # nomes registrados na ordem de leitura
        self.errors = []
        self.opaque = set()  # consts que não consegui parsear (referência a elas falha)


def collect_file(rel, src, out):
    toks = tokenize(src)
    total_calls = sum(1 for i, t in enumerate(toks) if t[0] == "id" and t[1] == "registerColor" and i + 1 < len(toks) and toks[i + 1][1] == "(")
    parsed_calls = 0
    i = 0
    while i < len(toks):
        t = toks[i]
        if t[0] == "id" and t[1] == "export":
            i += 1
            continue
        if t[0] == "id" and t[1] == "const" and toks[i + 1][0] == "id" and toks[i + 2][1] == "=":
            name = toks[i + 1][1]
            end = statement_end(toks, i)
            needs = any(x[0] == "id" and x[1] == "registerColor" and toks[j + 1][1] == "(" for j, x in enumerate(toks[i:end + 1], start=i) if j + 1 < len(toks))
            try:
                p = Parser(toks, i + 3)
                ast = p.expression()
                if p.pos != end:
                    raise GenError(f"linha {toks[p.pos][2]}: sobrou texto depois da expressão ({toks[p.pos][1]!r})")
            except GenError as e:
                if needs:
                    out.errors.append(f"{rel}: const {name}: {e}")
                    parsed_calls += sum(1 for j in range(i, end + 1) if toks[j][0] == "id" and toks[j][1] == "registerColor" and toks[j + 1][1] == "(")
                else:
                    out.opaque.add(name)
                i = end + 1
                continue
            if ast[0] == "call" and ast[1] == ("id", "registerColor"):
                if name in out.registered or name in out.consts:
                    out.errors.append(f"{rel}: const {name} definida duas vezes")
                out.registered[name] = (ast, rel, t[2])
                out.order.append(name)
                parsed_calls += 1
            else:
                if needs:
                    out.errors.append(f"{rel}: const {name}: registerColor dentro de expressão que não é a chamada direta")
                    parsed_calls += 1
                if name in out.registered or name in out.consts:
                    out.errors.append(f"{rel}: const {name} definida duas vezes")
                out.consts[name] = (ast, rel, t[2])
            i = end + 1
            continue
        if t[0] == "id" and t[1] == "registerColor" and toks[i + 1][1] == "(":
            end = statement_end(toks, i)
            try:
                p = Parser(toks, i)
                ast = p.expression()
                if p.pos != end:
                    raise GenError(f"linha {toks[p.pos][2]}: sobrou texto depois da expressão ({toks[p.pos][1]!r})")
                name = f"<chamada solta na linha {t[2]}>"
                out.registered[name] = (ast, rel, t[2])
                out.order.append(name)
            except GenError as e:
                out.errors.append(f"{rel}:{t[2]}: chamada solta a registerColor: {e}")
            parsed_calls += 1
            i = end + 1
            continue
        i = statement_end(toks, i) + 1
    if parsed_calls != total_calls:
        out.errors.append(f"{rel}: {total_calls} chamadas a registerColor, mas só {parsed_calls} foram traduzidas")


# ---------------------------------------------------------------- avaliação


class Evaluator:
    def __init__(self, c):
        self.c = c
        self.stack = []

    def number(self, ast):
        if ast[0] == "num":
            return ast[1]
        if ast[0] == "id":
            return self.const_number(ast[1])
        raise GenError(f"número esperado, achei {ast[0]}")

    def const_number(self, name):
        if name in self.c.consts:
            ast = self.c.consts[name][0]
            if ast[0] == "num":
                return ast[1]
        raise GenError(f"const numérica {name} não definida")

    def check_hex(self, h):
        if not re.fullmatch(r"#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})", h):
            raise GenError(f"hexadecimal inválido {h!r}")
        return h

    def color(self, ast):
        """Traduz uma expressão de ColorValue para a representação intermediária."""
        kind = ast[0]
        if kind == "str":
            if ast[1].startswith("#"):
                return ("hex", self.check_hex(ast[1]))
            return ("id", ast[1])
        if kind == "id":
            name = ast[1]
            if name in self.c.registered:
                return ("id", self.registered_id(name))
            if name in self.c.consts:
                if name in self.stack:
                    raise GenError(f"referência circular em {name}")
                self.stack.append(name)
                try:
                    return self.color(self.c.consts[name][0])
                finally:
                    self.stack.pop()
            if name in self.c.opaque:
                raise GenError(f"{name} é uma const que o gerador não traduz")
            raise GenError(f"identificador {name} sem definição conhecida")
        if kind == "obj":
            entries = dict(ast[1])
            op = entries.get("op")
            if op == ("member", ("id", "ColorTransformType"), "Mix") and set(entries) <= {"op", "color", "with", "ratio"} and {"color", "with"} <= set(entries):
                ratio = self.number(entries["ratio"]) if "ratio" in entries else None
                return ("mix", self.color(entries["color"]), self.color(entries["with"]), ratio)
            raise GenError(f"objeto de transformação não suportado: {sorted(entries)}")
        if kind == "member" and ast[1] == ("id", "Color"):
            if ast[2] in NAMED_COLORS:
                return ("rgba",) + NAMED_COLORS[ast[2]]
            raise GenError(f"Color.{ast[2]} não suportado")
        if kind == "new" and ast[1] == "Color":
            if len(ast[2]) == 1 and ast[2][0][0] == "new" and ast[2][0][1] == "RGBA":
                args = [self.number(a) for a in ast[2][0][2]]
                if len(args) == 3:
                    args.append(1.0)
                if len(args) != 4:
                    raise GenError("new RGBA precisa de 3 ou 4 argumentos")
                r, g, b, a = args
                if any(v != int(v) or not 0 <= v <= 255 for v in (r, g, b)):
                    raise GenError(f"canal RGBA fora de [0, 255] ou fracionário: {args}")
                if round(a, 3) != a or not 0 <= a <= 1:
                    raise GenError(f"alfa RGBA com mais de três casas ou fora de [0, 1]: {a}")
                return ("rgba", int(r), int(g), int(b), a)
            raise GenError("new Color só é suportado com new RGBA")
        if kind == "call":
            callee, args = ast[1], ast[2]
            if callee[0] == "id":
                fn = callee[1]
                if fn in ("transparent", "lighten", "darken") and len(args) == 2:
                    return (fn, self.color(args[0]), self.number(args[1]))
                if fn == "opaque" and len(args) == 2:
                    return ("opaque", self.color(args[0]), self.color(args[1]))
                if fn == "oneOf" and args:
                    return ("oneOf", [self.color(a) for a in args])
                if fn == "ifDefinedThenElse" and len(args) == 3:
                    cond = self.color(args[0])
                    if cond[0] != "id":
                        raise GenError("o primeiro argumento de ifDefinedThenElse deve ser um identificador de cor")
                    return ("ifDefinedThenElse", cond[1], self.color(args[1]), self.color(args[2]))
                if fn == "lessProminent" and len(args) == 4:
                    return ("lessProminent", self.color(args[0]), self.color(args[1]), self.number(args[2]), self.number(args[3]))
                raise GenError(f"função {fn}/{len(args)} não suportada")
            if callee[0] == "member":
                base, method = callee[1], callee[2]
                if base == ("id", "Color") and method == "fromHex" and len(args) == 1 and args[0][0] == "str":
                    return ("hex", self.check_hex(args[0][1]))
                if method in ("transparent", "lighten", "darken") and len(args) == 1:
                    return (method, self.color(base), self.number(args[0]))
                raise GenError(f"método {method} não suportado")
        raise GenError(f"expressão de cor não suportada ({kind})")

    def registered_id(self, name):
        ast = self.c.registered[name][0]
        arg0 = ast[2][0]
        if arg0[0] != "str":
            raise GenError(f"{name}: o id do registerColor não é uma string literal")
        return arg0[1]

    def description(self, ast):
        if ast[0] == "str":
            return ast[1]
        if ast[0] == "call" and ast[1] in (("id", "localize"), ("member", ("id", "nls"), "localize")):
            args = ast[2]
            if len(args) == 2 and args[0][0] == "str" and args[1][0] == "str":
                return args[1][1]
            raise GenError("localize com argumentos além de (chave, mensagem) ou com chave não literal")
        raise GenError("descrição não suportada")

    def defaults(self, ast):
        if ast[0] == "null":
            return None
        if ast[0] == "obj" and any(k == "op" for k, _ in ast[1]):
            return ("single", self.color(ast))
        if ast[0] == "obj":
            keys = [k for k, _ in ast[1]]
            if sorted(keys) != sorted(THEME_KEYS):
                raise GenError(f"objeto de padrões com chaves {keys}, esperado {list(THEME_KEYS)}")
            per = {}
            for k, v in ast[1]:
                per[k] = None if v[0] == "null" else self.color(v)
            return ("per_theme", per)
        return ("single", self.color(ast))

    def contribution(self, name):
        ast, rel, line = self.c.registered[name]
        args = ast[2]
        if not 3 <= len(args) <= 5:
            raise GenError(f"registerColor com {len(args)} argumentos")
        if args[0][0] != "str":
            raise GenError("id não literal")
        needs_transparency = False
        deprecation = None
        if len(args) >= 4:
            if args[3][0] == "bool":
                needs_transparency = args[3][1]
            elif args[3][0] != "undefined":
                raise GenError("needsTransparency não literal")
        if len(args) == 5:
            deprecation = self.description(args[4])
        return {
            "id": args[0][1],
            "description": self.description(args[2]),
            "defaults": self.defaults(args[1]),
            "needs_transparency": needs_transparency,
            "deprecation": deprecation,
            "source": rel,
        }


# ---------------------------------------------------------------- emissão


def rust_str(s):
    out = ['"']
    for ch in s:
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif 0x20 <= ord(ch) < 0x7F:
            out.append(ch)
        else:
            out.append("\\u{%x}" % ord(ch))
    out.append('"')
    return "".join(out)


def rust_f64(x):
    r = repr(float(x))
    if "e" in r or "inf" in r or "nan" in r:
        raise GenError(f"número não suportado na emissão: {r}")
    return r


def emit_value(v):
    kind = v[0]
    if kind == "hex":
        return f"V::Hex({rust_str(v[1])})"
    if kind == "id":
        return f"V::Id({rust_str(v[1])})"
    if kind == "rgba":
        return f"V::Rgba({v[1]}, {v[2]}, {v[3]}, {rust_f64(v[4])})"
    if kind in ("transparent", "lighten", "darken"):
        variant = kind.capitalize()
        return f"V::Transform(&T::{variant} {{ value: {emit_value(v[1])}, factor: {rust_f64(v[2])} }})"
    if kind == "opaque":
        return f"V::Transform(&T::Opaque {{ value: {emit_value(v[1])}, background: {emit_value(v[2])} }})"
    if kind == "oneOf":
        return "V::Transform(&T::OneOf(&[" + ", ".join(emit_value(x) for x in v[1]) + "]))"
    if kind == "ifDefinedThenElse":
        return f"V::Transform(&T::IfDefinedThenElse {{ if_defined: {rust_str(v[1])}, then: {emit_value(v[2])}, otherwise: {emit_value(v[3])} }})"
    if kind == "mix":
        ratio = "None" if v[3] is None else f"Some({rust_f64(v[3])})"
        return f"V::Transform(&T::Mix {{ color: {emit_value(v[1])}, with: {emit_value(v[2])}, ratio: {ratio} }})"
    if kind == "lessProminent":
        return (
            f"V::Transform(&T::LessProminent {{ value: {emit_value(v[1])}, background: {emit_value(v[2])}, "
            f"factor: {rust_f64(v[3])}, transparency: {rust_f64(v[4])} }})"
        )
    raise GenError(f"valor intermediário desconhecido: {kind}")


def emit_opt(v):
    return "None" if v is None else f"Some({emit_value(v)})"


def emit_defaults(d):
    if d is None:
        return "None"
    if d[0] == "single":
        return f"Some(Defaults::Single({emit_value(d[1])}))"
    per = d[1]
    return (
        "Some(Defaults::PerTheme(ColorDefaults { "
        f"light: {emit_opt(per['light'])}, dark: {emit_opt(per['dark'])}, "
        f"hc_dark: {emit_opt(per['hcDark'])}, hc_light: {emit_opt(per['hcLight'])} }}))"
    )


def vscode_version():
    pkg = os.path.join(VSCODE, "package.json")
    with open(pkg, encoding="utf-8") as f:
        m = re.search(r'"version":\s*"([^"]+)"', f.read())
    return m.group(1) if m else "desconhecida"


def render(contributions):
    lines = [
        "// ARQUIVO GERADO por `scripts/gen_color_registry.py`. NÃO EDITE À MÃO: rode",
        "// `python3 -I scripts/gen_color_registry.py` para regerar.",
        "//",
        f"// Origem: as chamadas `registerColor(...)` do VS Code {vscode_version()} em",
        *[f"// - {s}" for s in SOURCES],
        "// (caminhos relativos a `ref/vscode/src/vs`), na ordem do `colorRegistry.ts` e por fim o `theme.ts`.",
        "",
        "use crate::registry::{ColorContribution, ColorDefaults, ColorTransform as T, ColorValue as V, Defaults};",
        "",
        f"/// As {len(contributions)} cores registradas, com id, padrões por tipo de tema e descrição.",
        "pub static REGISTERED_COLORS: &[ColorContribution] = &[",
    ]
    for c in contributions:
        lines.append("    ColorContribution {")
        lines.append(f"        id: {rust_str(c['id'])},")
        lines.append(f"        description: {rust_str(c['description'])},")
        lines.append(f"        defaults: {emit_defaults(c['defaults'])},")
        lines.append(f"        needs_transparency: {'true' if c['needs_transparency'] else 'false'},")
        dep = "None" if c["deprecation"] is None else f"Some({rust_str(c['deprecation'])})"
        lines.append(f"        deprecation_message: {dep},")
        lines.append("    },")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def referenced_ids(v, acc):
    if v is None:
        return
    kind = v[0]
    if kind == "id":
        acc.add(v[1])
    elif kind in ("transparent", "lighten", "darken"):
        referenced_ids(v[1], acc)
    elif kind in ("opaque", "mix"):
        referenced_ids(v[1], acc)
        referenced_ids(v[2], acc)
    elif kind == "oneOf":
        for x in v[1]:
            referenced_ids(x, acc)
    elif kind == "ifDefinedThenElse":
        acc.add(v[1])
        referenced_ids(v[2], acc)
        referenced_ids(v[3], acc)
    elif kind == "lessProminent":
        referenced_ids(v[1], acc)
        referenced_ids(v[2], acc)


def main(argv):
    check = "--check" in argv
    c = Collected()
    for rel in SOURCES:
        path = os.path.join(VS, rel)
        try:
            with open(path, encoding="utf-8") as f:
                src = f.read()
            collect_file(rel, src, c)
        except (GenError, OSError) as e:
            c.errors.append(f"{rel}: {e}")
    ev = Evaluator(c)
    contributions = []
    for name in c.order:
        try:
            contributions.append(ev.contribution(name))
        except GenError as e:
            _, rel, line = c.registered[name]
            c.errors.append(f"{rel}:{line}: {name}: {e}")
    seen = {}
    for contribution in contributions:
        if contribution["id"] in seen:
            c.errors.append(f"id duplicado {contribution['id']!r} em {seen[contribution['id']]} e {contribution['source']}")
        seen[contribution["id"]] = contribution["source"]
    refs = set()
    for contribution in contributions:
        d = contribution["defaults"]
        if d is None:
            continue
        for v in ([d[1]] if d[0] == "single" else d[1].values()):
            referenced_ids(v, refs)
    for ref in sorted(refs):
        if ref not in seen:
            c.errors.append(f"referência a cor não registrada: {ref!r}")
    if c.errors:
        print(f"{len(c.errors)} erro(s), nada foi escrito:", file=sys.stderr)
        for e in c.errors:
            print(f"  - {e}", file=sys.stderr)
        return 1
    text = render(contributions)
    if check:
        with open(OUT, encoding="utf-8") as f:
            current = f.read()
        if current != text:
            print("registered_colors.rs desatualizado, rode scripts/gen_color_registry.py", file=sys.stderr)
            return 1
        print(f"registered_colors.rs em dia ({len(contributions)} cores)")
        return 0
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"{len(contributions)} cores registradas, escritas em {os.path.relpath(OUT, ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
