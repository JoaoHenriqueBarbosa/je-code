#!/usr/bin/env python3
"""Gera `crates/je-theme/src/registered_colors.rs` a partir de TODAS as cores que o VS Code registra.

Fontes (relativas a `ref/vscode/src/vs`): toda chamada `registerColor(...)` de qualquer `.ts` fora de pastas
de teste (os nomes importados são resolvidos arquivo a arquivo, seguindo `import`, `export * from` e
`export { } from`), o laço `registerColors()` do terminal (`terminal.ansi*`, expandido de `ansiColorMap`)
e os `contributes.colors` dos `package.json` das extensões embutidas (`ref/vscode/extensions/*`), que o
VS Code registra na partida. As cores de `platform/theme/common/colors/*.ts` e `workbench/common/theme.ts`
vêm primeiro, na ordem do `colorRegistry.ts`; as demais seguem por caminho de arquivo.

Uso (a partir da raiz do repositório):

    python3 -I scripts/gen_color_registry.py            # escreve o arquivo gerado
    python3 -I scripts/gen_color_registry.py --check    # só confere se o arquivo versionado está em dia

O gerador não chuta: tudo que ele não consegue traduzir com certeza (expressão fora do subconjunto
conhecido, referência sem definição, chamada a `registerColor` que não virou cor, cor com hexadecimal
inválido, identificador duplicado, referência a cor não registrada) vira uma linha na lista de erros,
e o programa sai com código 1 sem escrever nada.
"""

import json
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
  | (?P<tpl>`(?:[^`\\$]|\\.|\$(?!\{))*`)
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
            toks.append(("str", unescape(text), line))
            line += text.count("\n")
            pos = m.end()
            continue
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


# ---------------------------------------------------------------- leitura dos arquivos

IDENT = r"[A-Za-z_$][A-Za-z0-9_$]*"
CONST_RE = re.compile(r"\bconst\s+(" + IDENT + r")\s*(?::[^=]*?)?=(?![=>])")
CALL_RE = re.compile(r"(?<![.\w$])registerColor\s*\(")
IMPORT_RE = re.compile(r"\bimport\s+(?:type\s+)?(?:(\*\s+as\s+" + IDENT + r")|\{([^}]*)\}|(" + IDENT + r"))\s*from\s*'([^']+)'")
EXPORT_FROM_RE = re.compile(r"\bexport\s+(?:type\s+)?(?:(\*)(?:\s+as\s+(" + IDENT + r"))?|\{([^}]*)\})\s*from\s*'([^']+)'")
BINDING_RE = re.compile(r"\bconst\s+(" + IDENT + r")\s*(?::[^=;]*?)?=\s*$")


def mask_source(src):
    """Devolve (sem_comentarios, mascarado): o primeiro troca comentários por espaços, o segundo troca
    também o conteúdo de strings e templates, mantendo os deslocamentos e as quebras de linha."""
    nc = list(src)
    mk = list(src)
    n = len(src)

    def blank(arr, a, b):
        for k in range(a, b):
            if arr[k] != "\n":
                arr[k] = " "

    def scan_string(i, quote):
        j = i + 1
        while j < n:
            ch = src[j]
            if ch == "\\":
                j += 2
                continue
            if ch == quote:
                return j
            if quote == "`" and ch == "$" and src[j + 1:j + 2] == "{":
                j = scan_code(j + 2, closing="}")
                continue
            if quote != "`" and ch == "\n":
                return j
            j += 1
        return n

    def scan_code(i, closing=None):
        depth = 0
        while i < n:
            ch = src[i]
            nxt = src[i + 1:i + 2]
            if ch == "/" and nxt == "/":
                e = src.find("\n", i)
                e = n if e < 0 else e
                blank(nc, i, e)
                blank(mk, i, e)
                i = e
            elif ch == "/" and nxt == "*":
                e = src.find("*/", i + 2)
                e = n if e < 0 else e + 2
                blank(nc, i, e)
                blank(mk, i, e)
                i = e
            elif ch in "'\"`":
                e = scan_string(i, ch)
                blank(mk, i + 1, e)
                i = e + 1
            elif closing and ch == "{":
                depth += 1
                i += 1
            elif closing and ch == "}":
                if depth == 0:
                    return i + 1
                depth -= 1
                i += 1
            else:
                i += 1
        return i

    scan_code(0)
    return "".join(nc), "".join(mk)


def match_close(masked, open_pos):
    """Posição do fecho que casa com o abre em `open_pos`, no texto mascarado."""
    depth = 0
    for i in range(open_pos, len(masked)):
        ch = masked[i]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth == 0:
                return i
    raise GenError("parêntese sem fecho")


def expression_end(masked, start):
    """Fim da expressão que começa em `start`: `;` ou fecho sem abre, ambos em profundidade zero."""
    depth = 0
    for i in range(start, len(masked)):
        ch = masked[i]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                return i
            depth -= 1
        elif ch == ";" and depth == 0:
            return i
    return len(masked)


def line_of(src, pos):
    return src.count("\n", 0, pos) + 1


def parse_slice(src, a, b, base_line):
    toks = tokenize(src[a:b])
    toks = [(k, v, ln + base_line - 1) for k, v, ln in toks]
    p = Parser(toks, 0)
    ast = p.expression()
    if p.pos != len(toks):
        raise GenError(f"linha {toks[p.pos][2]}: sobrou texto depois da expressão ({toks[p.pos][1]!r})")
    return ast


class SourceFile:
    """Um arquivo .ts: constantes, chamadas a `registerColor`, imports e reexportações."""

    def __init__(self, rel, src):
        self.rel = rel
        self.src = src
        self.nc, self.masked = mask_source(src)
        self.consts = {}  # nome -> [posição depois do `=`]
        for m in CONST_RE.finditer(self.masked):
            self.consts.setdefault(m.group(1), []).append(m.end())
        self.imports = {}  # nome local -> (módulo, nome exportado) ou (módulo, None) para `* as`
        for m in IMPORT_RE.finditer(self.nc):
            module = m.group(4)
            if m.group(1):
                self.imports[m.group(1).split()[-1]] = (module, None)
            elif m.group(3):
                self.imports[m.group(3)] = (module, "default")
            else:
                for part in m.group(2).split(","):
                    part = re.sub(r"^\s*type\s+", "", part).strip()
                    if not part:
                        continue
                    pieces = part.split(" as ")
                    self.imports[pieces[-1].strip()] = (module, pieces[0].strip())
        self.star_exports = []
        self.named_exports = {}  # nome exportado -> (módulo, nome original)
        for m in EXPORT_FROM_RE.finditer(self.nc):
            module = m.group(4)
            if m.group(1) and not m.group(2):
                self.star_exports.append(module)
            elif m.group(3):
                for part in m.group(3).split(","):
                    part = re.sub(r"^\s*type\s+", "", part).strip()
                    if part:
                        pieces = part.split(" as ")
                        self.named_exports[pieces[-1].strip()] = (module, pieces[0].strip())


class Project:
    def __init__(self):
        self.files = {}
        self.errors = []

    def load(self, rel):
        if rel not in self.files:
            with open(os.path.join(VS, rel), encoding="utf-8") as f:
                self.files[rel] = SourceFile(rel, f.read())
        return self.files[rel]

    def module_path(self, rel, spec):
        if not spec.startswith("."):
            return None
        base = os.path.normpath(os.path.join(os.path.dirname(rel), spec))
        if base.endswith(".js"):
            base = base[:-3]
        for cand in (base + ".ts", os.path.join(base, "index.ts")):
            if os.path.isfile(os.path.join(VS, cand)):
                return cand
        return None

    def definition(self, rel, name, seen=None):
        """Acha a definição do nome `name` visível no arquivo `rel`: ('const', arquivo, nome) ou None."""
        seen = seen or set()
        if (rel, name) in seen:
            return None
        seen.add((rel, name))
        f = self.load(rel)
        if name in f.consts:
            return (f.rel, name)
        if name in f.imports:
            module, orig = f.imports[name]
            target = self.module_path(rel, module)
            if target is None or orig is None:
                return None
            return self.exported(target, orig, seen)
        return None

    def exported(self, rel, name, seen):
        f = self.load(rel)
        if name in f.consts:
            return (f.rel, name)
        if name in f.named_exports:
            module, orig = f.named_exports[name]
            target = self.module_path(rel, module)
            return self.exported(target, orig, seen) if target else None
        for module in f.star_exports:
            target = self.module_path(rel, module)
            if target and (target, name) not in seen:
                seen.add((target, name))
                found = self.exported(target, name, seen)
                if found:
                    return found
        return None

    def const_expr(self, rel, name):
        """Devolve ('reg', chamada, linha) se o const é `registerColor(...)`, senão ('expr', ast, linha)."""
        f = self.load(rel)
        starts = f.consts[name]
        texts = set()
        for start in starts:
            end = expression_end(f.masked, start)
            texts.add(re.sub(r"\s+", " ", f.nc[start:end]).strip())
        if len(texts) > 1:
            raise GenError(f"{rel}: const {name} definida mais de uma vez com expressões diferentes")
        start = starts[0]
        end = expression_end(f.masked, start)
        line = line_of(f.src, start)
        ast = parse_slice(f.nc, start, end, line)
        if ast[0] == "call" and ast[1] == ("id", "registerColor"):
            return ("reg", ast, line)
        return ("expr", ast, line)


def find_calls(project, rel):
    """Todas as chamadas `registerColor(...)` do arquivo: lista de (nome do const ou None, ast, linha)."""
    f = project.load(rel)
    calls = []
    for m in CALL_RE.finditer(f.masked):
        before = f.masked[max(0, m.start() - 300):m.start()]
        if re.search(r"\bfunction\s+$", before):
            continue
        open_pos = m.end() - 1
        close = match_close(f.masked, open_pos)
        line = line_of(f.src, m.start())
        binding = BINDING_RE.search(before)
        name = binding.group(1) if binding else None
        try:
            ast = parse_slice(f.nc, m.start(), close + 1, line)
        except GenError as e:
            project.errors.append(f"{rel}:{line}: {e}")
            continue
        calls.append((name, ast, line))
    return calls


def list_sources():
    out = []
    for dirpath, _, names in os.walk(VS):
        rel_dir = os.path.relpath(dirpath, VS)
        parts = rel_dir.split(os.sep)
        if "test" in parts or "node_modules" in parts:
            continue
        for n in names:
            if n.endswith(".ts") and not n.endswith(".test.ts") and not n.endswith(".d.ts"):
                rel = os.path.normpath(os.path.join(rel_dir, n))
                with open(os.path.join(VS, rel), encoding="utf-8") as f:
                    if "registerColor" in f.read():
                        out.append(rel)
    return sorted(out)


# ---------------------------------------------------------------- avaliação


class Evaluator:
    def __init__(self, project):
        self.p = project
        self.stack = []

    def lookup(self, rel, name):
        d = self.p.definition(rel, name)
        if d is None:
            raise GenError(f"identificador {name} sem definição conhecida")
        return d

    def number(self, ast, rel):
        if ast[0] == "num":
            return ast[1]
        if ast[0] == "id":
            drel, dname = self.lookup(rel, ast[1])
            kind, node, _ = self.p.const_expr(drel, dname)
            if kind == "expr" and node[0] == "num":
                return node[1]
            raise GenError(f"const numérica {ast[1]} não é um número literal")
        raise GenError(f"número esperado, achei {ast[0]}")

    def check_hex(self, h):
        if not re.fullmatch(r"#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})", h):
            raise GenError(f"hexadecimal inválido {h!r}")
        return h

    def color(self, ast, rel):
        """Traduz uma expressão de ColorValue (no escopo do arquivo `rel`) para a representação intermediária."""
        kind = ast[0]
        if kind == "str":
            if ast[1].startswith("#"):
                return ("hex", self.check_hex(ast[1]))
            return ("id", ast[1])
        if kind == "id":
            name = ast[1]
            drel, dname = self.lookup(rel, name)
            if (drel, dname) in self.stack:
                raise GenError(f"referência circular em {name}")
            self.stack.append((drel, dname))
            try:
                ckind, node, _ = self.p.const_expr(drel, dname)
                if ckind == "reg":
                    return ("id", self.registered_id(node))
                return self.color(node, drel)
            finally:
                self.stack.pop()
        if kind == "obj":
            entries = dict(ast[1])
            op = entries.get("op")
            if op == ("member", ("id", "ColorTransformType"), "Mix") and set(entries) <= {"op", "color", "with", "ratio"} and {"color", "with"} <= set(entries):
                ratio = self.number(entries["ratio"], rel) if "ratio" in entries else None
                return ("mix", self.color(entries["color"], rel), self.color(entries["with"], rel), ratio)
            raise GenError(f"objeto de transformação não suportado: {sorted(entries)}")
        if kind == "member" and ast[1] == ("id", "Color"):
            if ast[2] in NAMED_COLORS:
                return ("rgba",) + NAMED_COLORS[ast[2]]
            raise GenError(f"Color.{ast[2]} não suportado")
        if kind == "new" and ast[1] == "Color":
            if len(ast[2]) == 1 and ast[2][0][0] == "new" and ast[2][0][1] == "RGBA":
                args = [self.number(a, rel) for a in ast[2][0][2]]
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
                    return (fn, self.color(args[0], rel), self.number(args[1], rel))
                if fn == "opaque" and len(args) == 2:
                    return ("opaque", self.color(args[0], rel), self.color(args[1], rel))
                if fn == "oneOf" and args:
                    return ("oneOf", [self.color(a, rel) for a in args])
                if fn == "ifDefinedThenElse" and len(args) == 3:
                    cond = self.color(args[0], rel)
                    if cond[0] != "id":
                        raise GenError("o primeiro argumento de ifDefinedThenElse deve ser um identificador de cor")
                    return ("ifDefinedThenElse", cond[1], self.color(args[1], rel), self.color(args[2], rel))
                if fn == "lessProminent" and len(args) == 4:
                    return ("lessProminent", self.color(args[0], rel), self.color(args[1], rel), self.number(args[2], rel), self.number(args[3], rel))
                raise GenError(f"função {fn}/{len(args)} não suportada")
            if callee[0] == "member":
                base, method = callee[1], callee[2]
                if base == ("id", "Color") and method == "fromHex" and len(args) == 1 and args[0][0] == "str":
                    return ("hex", self.check_hex(args[0][1]))
                if method in ("transparent", "lighten", "darken") and len(args) == 1:
                    return (method, self.color(base, rel), self.number(args[0], rel))
                raise GenError(f"método {method} não suportado")
        raise GenError(f"expressão de cor não suportada ({kind})")

    def registered_id(self, call):
        arg0 = call[2][0]
        if arg0[0] != "str":
            raise GenError("o id do registerColor não é uma string literal")
        return arg0[1]

    def description(self, ast):
        if ast[0] == "str":
            return ast[1]
        if ast[0] == "call" and ast[1] in (("id", "localize"), ("member", ("id", "nls"), "localize")):
            args = ast[2]
            if len(args) >= 2 and all(a[0] == "str" for a in args):
                message = args[1][1]
                for i, extra in enumerate(args[2:]):
                    message = message.replace("{%d}" % i, extra[1])
                return message
            raise GenError("localize com argumentos que não são strings literais")
        raise GenError("descrição não suportada")

    def defaults(self, ast, rel):
        if ast[0] == "null":
            return None
        if ast[0] == "obj" and any(k == "op" for k, _ in ast[1]):
            return ("single", self.color(ast, rel))
        if ast[0] == "obj":
            keys = [k for k, _ in ast[1]]
            if sorted(keys) != sorted(THEME_KEYS):
                raise GenError(f"objeto de padrões com chaves {keys}, esperado {list(THEME_KEYS)}")
            per = {}
            for k, v in ast[1]:
                per[k] = None if v[0] == "null" else self.color(v, rel)
            return ("per_theme", per)
        return ("single", self.color(ast, rel))

    def contribution(self, call, rel, line):
        args = call[2]
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
            "defaults": self.defaults(args[1], rel),
            "needs_transparency": needs_transparency,
            "deprecation": deprecation,
            "source": rel,
            "line": line,
        }

    def terminal_ansi(self, rel, call, line):
        """`registerColors()` do terminal: um laço sobre `ansiColorMap` que registra `terminal.ansi*`."""
        expected_args = [("id", "id"), ("member", ("id", "entry"), "defaults")]
        args = call[2]
        if len(args) != 3 or args[:2] != expected_args or args[2][0] != "call":
            raise GenError("laço de registerColor com forma desconhecida")
        loc = args[2][2]
        if len(loc) < 2 or loc[1][0] != "str" or "{0}" not in loc[1][1]:
            raise GenError("descrição do laço sem {0}")
        template = loc[1][1]
        kind, node, _ = self.p.const_expr(rel, "ansiColorMap")
        if kind != "expr" or node[0] != "obj":
            raise GenError("ansiColorMap não é um objeto literal")
        out = []
        for key, entry in node[1]:
            fields = dict(entry[1])
            if set(fields) != {"index", "defaults"}:
                raise GenError(f"entrada {key} de ansiColorMap com campos {sorted(fields)}")
            out.append({
                "id": key,
                "description": template.replace("{0}", key[13:]),
                "defaults": self.defaults(fields["defaults"], rel),
                "needs_transparency": False,
                "deprecation": None,
                "source": rel,
                "line": line,
            })
        return out


def extension_colors(project):
    """`contributes.colors` dos package.json das extensões embutidas (o VS Code os registra na partida)."""
    out = []
    ext_dir = os.path.join(VSCODE, "extensions")
    for name in sorted(os.listdir(ext_dir)):
        pkg = os.path.join(ext_dir, name, "package.json")
        if not os.path.isfile(pkg):
            continue
        with open(pkg, encoding="utf-8") as f:
            data = json.load(f)
        colors = data.get("contributes", {}).get("colors")
        if not colors:
            continue
        nls_path = os.path.join(ext_dir, name, "package.nls.json")
        nls = {}
        if os.path.isfile(nls_path):
            with open(nls_path, encoding="utf-8") as f:
                nls = json.load(f)
        rel = f"extensions/{name}/package.json"
        for c in colors:
            try:
                desc = c["description"]
                m = re.fullmatch(r"%(.+)%", desc)
                if m:
                    entry = nls[m.group(1)]
                    desc = entry["message"] if isinstance(entry, dict) else entry
                d = c["defaults"]
                if not re.fullmatch(r"[A-Za-z0-9]+(\.[A-Za-z0-9]+)*", c["id"]):
                    raise GenError("id com formato inválido")

                def val(s):
                    if not s:
                        raise GenError("valor de cor vazio")
                    return ("hex", Evaluator(project).check_hex(s)) if s.startswith("#") else ("id", s)

                per = {
                    "light": val(d["light"]),
                    "dark": val(d["dark"]),
                    "hcDark": val(d.get("highContrast") or d["dark"]),
                    "hcLight": val(d.get("highContrastLight") or d["light"]),
                }
                out.append({
                    "id": c["id"],
                    "description": desc,
                    "defaults": ("per_theme", per),
                    "needs_transparency": False,
                    "deprecation": None,
                    "source": rel,
                    "line": 0,
                })
            except (GenError, KeyError) as e:
                project.errors.append(f"{rel}: cor {c.get('id')}: {e!r}")
    return out


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


# Pastas que o porte deixa fora de escopo (decisão do user), mas cujas cores o VS Code registra e os
# temas de terceiros definem: o registro é barato, então entram assim mesmo, com a origem marcada.
OUT_OF_SCOPE = (
    ("sessions/", "src/vs/sessions"),
    ("workbench/contrib/chat/", "chat"),
    ("workbench/common/agentsColors.ts", "agents view"),
    ("workbench/contrib/agentsVoice/", "agents view"),
)


def origin_note(rel):
    for prefix, label in OUT_OF_SCOPE:
        if rel.startswith(prefix):
            return f"{rel} (fora do escopo do porte: {label}; registrada mesmo assim)"
    return rel


def render(contributions):
    lines = [
        "// ARQUIVO GERADO por `scripts/gen_color_registry.py`. NÃO EDITE À MÃO: rode",
        "// `python3 -I scripts/gen_color_registry.py` para regerar.",
        "//",
        f"// Origem: todas as cores registradas pelo VS Code {vscode_version()}: as chamadas `registerColor(...)`",
        "// de `ref/vscode/src/vs` (caminhos relativos a `src/vs`), o laço de `terminal.ansi*` e os",
        "// `contributes.colors` das extensões embutidas. Cada grupo abaixo traz a sua origem num comentário.",
        "",
        "use crate::registry::{ColorContribution, ColorDefaults, ColorTransform as T, ColorValue as V, Defaults};",
        "",
        f"/// As {len(contributions)} cores registradas, com id, padrões por tipo de tema e descrição.",
        "pub static REGISTERED_COLORS: &[ColorContribution] = &[",
    ]
    last = None
    for c in contributions:
        if c["source"] != last:
            lines.append(f"    // origem: {origin_note(c['source'])}")
            last = c["source"]
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
    project = Project()
    ev = Evaluator(project)
    sources = list_sources()
    ordered = [r for r in SOURCES if r in sources] + [r for r in sources if r not in SOURCES]
    contributions = []
    for rel in ordered:
        if rel == "platform/theme/common/colorUtils.ts":
            continue  # define `registerColor`, não registra cor
        total = len(CALL_RE.findall(project.load(rel).masked)) - len(
            re.findall(r"\bfunction\s+registerColor\s*\(", project.load(rel).masked)
        )
        calls = find_calls(project, rel)
        if len(calls) != total:
            project.errors.append(f"{rel}: {total} chamadas a registerColor, mas só {len(calls)} foram lidas")
        for name, call, line in calls:
            try:
                if call[2] and call[2][0] == ("id", "id") and rel.endswith("terminal/common/terminalColorRegistry.ts"):
                    contributions.extend(ev.terminal_ansi(rel, call, line))
                else:
                    contributions.append(ev.contribution(call, rel, line))
            except GenError as e:
                project.errors.append(f"{rel}:{line}: {name or 'chamada sem nome'}: {e}")
    contributions.extend(extension_colors(project))
    seen = {}
    for contribution in contributions:
        if contribution["id"] in seen:
            project.errors.append(f"id duplicado {contribution['id']!r} em {seen[contribution['id']]} e {contribution['source']}")
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
            project.errors.append(f"referência a cor não registrada: {ref!r}")
    if project.errors:
        print(f"{len(project.errors)} erro(s), nada foi escrito:", file=sys.stderr)
        for e in project.errors:
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
