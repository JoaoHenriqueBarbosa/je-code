//! Como o bun 1.4.2 decide se o arquivo principal (`bun main.js`, sem `package.json`) roda como módulo ES ou como
//! CommonJS. A regra foi medida arquivo por arquivo (a mesma que `moduleMode` em `scripts/golden-prelude.js` aplica com o
//! `bun build` de juiz):
//!
//! - ESM é o padrão: um arquivo sem nenhum marcador de CJS é módulo ES (estrito, `this` do topo `undefined`, `var` do
//!   topo fora do global) mesmo sem `import` nem `export`.
//! - CJS (embrulhado em `function(exports, require, module, __filename, __dirname)`) quando o arquivo tem qualquer um
//!   destes marcadores: referência livre a `module`, `exports`, `__dirname`, `__filename` ou `require` (exceto a instrução
//!   `typeof require;` sozinha, código morto que o transpilador elimina); `this` no topo, inclusive em arrow function do
//!   topo; a instrução `with`. Declarar o nome (variável, função, classe, parâmetro) desfaz o marcador.
//! - A diretiva `"use strict"` como primeira instrução faz o arquivo CJS estrito, a menos que ele tenha sintaxe de
//!   módulo (`import`, `export`, `import.meta`, `await` no topo) e nenhum marcador de CJS: aí o ESM vence.
//!
//! Não marcam: `arguments`, `new.target`, `return` no topo, `o.require`, `{ require: 1 }`, a palavra numa string,
//! `globalThis.module`, `import()` dinâmico.

use crate::wtf::unicode::{is_id_continue, is_id_start};

/// O modo em que o bun roda o arquivo principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainModuleKind {
    Esm,
    Cjs,
    /// CJS cuja função de wrapper é estrita pela diretiva do topo.
    CjsStrict,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Identifier(String),
    Punctuator(&'static str),
    String(String),
    Template,
    Number,
    Regex,
}

struct Lexed {
    token: Token,
    newline_before: bool,
}

const PUNCTUATORS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.",
    "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "**", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+",
    "-", "*", "/", "%", "&", "|", "^", "!", "~", "?", ":", "=", ".", "@", "#",
];

/// Palavras depois das quais uma `/` começa uma expressão regular, não uma divisão.
const REGEX_AFTER_WORD: &[&str] =
    &["return", "typeof", "instanceof", "in", "of", "new", "delete", "void", "throw", "case", "do", "else", "yield", "await"];

fn is_identifier_start(c: char) -> bool {
    c == '$' || c == '_' || c == '\\' || is_id_start(c as u32)
}

fn is_identifier_part(c: char) -> bool {
    c == '$' || c == '_' || c == '\\' || c == '\u{200c}' || c == '\u{200d}' || is_id_continue(c as u32)
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Uma `/` nesta posição começa expressão regular quando o token anterior não termina uma expressão.
fn regex_allowed(previous: Option<&Token>) -> bool {
    match previous {
        None => true,
        Some(Token::Identifier(word)) => REGEX_AFTER_WORD.contains(&word.as_str()),
        Some(Token::Punctuator(p)) => !matches!(*p, ")" | "]" | "}" | "++" | "--"),
        Some(_) => false,
    }
}

/// Os tokens do fonte, com comentários e espaços descartados. O miolo de `${...}` de template literal entra como tokens
/// comuns; o texto do template vira um `Token::Template`. Fonte malformado termina a lista onde o léxico se perde.
fn tokenize(source: &str) -> Vec<Lexed> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens: Vec<Lexed> = Vec::new();
    // Cada `{` aberto: verdadeiro quando é o `${` de um template (o `}` que o fecha retoma o texto do template).
    let mut braces: Vec<bool> = Vec::new();
    let mut index = 0;
    let mut newline_before = false;
    let push = |tokens: &mut Vec<Lexed>, token: Token, newline_before: &mut bool| {
        tokens.push(Lexed { token, newline_before: *newline_before });
        *newline_before = false;
    };
    // Lê o texto de um template a partir de `index` (depois do `` ` `` ou do `}`); devolve onde parou e se parou num `${`.
    let scan_template = |mut index: usize| -> (usize, bool) {
        while index < chars.len() {
            match chars[index] {
                '\\' => index += 2,
                '`' => return (index + 1, false),
                '$' if chars.get(index + 1) == Some(&'{') => return (index + 2, true),
                _ => index += 1,
            }
        }
        (index, false)
    };
    while index < chars.len() {
        let c = chars[index];
        if is_line_terminator(c) {
            newline_before = true;
            index += 1;
        } else if c.is_whitespace() || c == '\u{feff}' {
            index += 1;
        } else if c == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && !is_line_terminator(chars[index]) {
                index += 1;
            }
        } else if c == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index < chars.len() && !(chars[index] == '*' && chars.get(index + 1) == Some(&'/')) {
                newline_before |= is_line_terminator(chars[index]);
                index += 1;
            }
            index += 2;
        } else if c == '#' && index == 0 && chars.get(1) == Some(&'!') {
            while index < chars.len() && !is_line_terminator(chars[index]) {
                index += 1;
            }
        } else if c == '\'' || c == '"' {
            let start = index + 1;
            index += 1;
            while index < chars.len() && chars[index] != c && !is_line_terminator(chars[index]) {
                index += if chars[index] == '\\' { 2 } else { 1 };
            }
            let text: String = chars[start..index.min(chars.len())].iter().collect();
            index += 1;
            push(&mut tokens, Token::String(text), &mut newline_before);
        } else if c == '`' {
            let (end, opened) = scan_template(index + 1);
            index = end;
            if opened {
                braces.push(true);
            }
            push(&mut tokens, Token::Template, &mut newline_before);
        } else if c == '}' && braces.last() == Some(&true) {
            braces.pop();
            let (end, opened) = scan_template(index + 1);
            index = end;
            if opened {
                braces.push(true);
            }
        } else if c.is_ascii_digit() || (c == '.' && chars.get(index + 1).is_some_and(char::is_ascii_digit)) {
            let radix_prefix = c == '0' && chars.get(index + 1).is_some_and(|next| next.is_ascii_alphabetic());
            while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_' || chars[index] == '.') {
                // O expoente com sinal (`1e-5`); em `0x1e` o `e` é dígito.
                if !radix_prefix && matches!(chars[index], 'e' | 'E') && matches!(chars.get(index + 1), Some('+' | '-')) {
                    index += 1;
                }
                index += 1;
            }
            push(&mut tokens, Token::Number, &mut newline_before);
        } else if is_identifier_start(c) {
            let start = index;
            while index < chars.len() && is_identifier_part(chars[index]) {
                index += if chars[index] == '\\' { 6 } else { 1 };
            }
            let word: String = chars[start..index.min(chars.len())].iter().collect();
            push(&mut tokens, Token::Identifier(word), &mut newline_before);
        } else if c == '/' && regex_allowed(tokens.last().map(|lexed| &lexed.token)) {
            index += 1;
            let mut in_class = false;
            while index < chars.len() && !is_line_terminator(chars[index]) {
                match chars[index] {
                    '\\' => index += 1,
                    '[' => in_class = true,
                    ']' => in_class = false,
                    '/' if !in_class => break,
                    _ => {}
                }
                index += 1;
            }
            index += 1;
            while index < chars.len() && is_identifier_part(chars[index]) {
                index += 1;
            }
            push(&mut tokens, Token::Regex, &mut newline_before);
        } else {
            let rest: String = chars[index..(index + 4).min(chars.len())].iter().collect();
            let Some(punctuator) = PUNCTUATORS.iter().find(|p| rest.starts_with(**p)) else {
                break;
            };
            match *punctuator {
                "{" => braces.push(false),
                "}" => {
                    braces.pop();
                }
                _ => {}
            }
            index += punctuator.chars().count();
            push(&mut tokens, Token::Punctuator(punctuator), &mut newline_before);
        }
    }
    tokens
}

/// O que um `{` abre, para saber se `this` e `await` ainda estão no topo dentro dele.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Brace {
    Block,
    /// Corpo de função comum ou método: `this` e `await` dentro dele não são os do topo.
    Function,
    /// Corpo de arrow function: `this` é o do topo, `await` não.
    Arrow,
    /// Corpo de classe: métodos e inicializadores de campo têm o próprio `this`.
    Class,
}

/// As palavras de controle cujo `(...)` precede um bloco comum.
const CONTROL_WORDS: &[&str] = &["if", "for", "while", "switch", "catch", "with"];
const MARKERS: &[&str] = &["require", "module", "exports", "__dirname", "__filename"];

fn identifier(token: Option<&Lexed>) -> Option<&str> {
    match token.map(|lexed| &lexed.token) {
        Some(Token::Identifier(word)) => Some(word.as_str()),
        _ => None,
    }
}

fn punctuator(token: Option<&Lexed>) -> Option<&'static str> {
    match token.map(|lexed| &lexed.token) {
        Some(Token::Punctuator(p)) => Some(p),
        _ => None,
    }
}

/// Começo de instrução: nada antes, ou depois de `;`, `{`, `}` ou de uma quebra de linha.
fn at_statement_start(tokens: &[Lexed], index: usize) -> bool {
    index == 0 || tokens[index].newline_before || matches!(punctuator(tokens.get(index - 1)), Some(";" | "{" | "}"))
}

/// O índice do `(` que casa com o `)` em `close`.
fn matching_open_paren(tokens: &[Lexed], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for index in (0..=close).rev() {
        match punctuator(tokens.get(index)) {
            Some(")") => depth += 1,
            Some("(") => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Os nomes que o arquivo declara em algum lugar (variável, função, classe, parâmetro, `catch`): como no
/// `hasFreeMarker` do gerador, uma declaração em qualquer escopo desfaz o marcador do mesmo nome.
fn declared_names(tokens: &[Lexed]) -> Vec<&str> {
    let mut names = Vec::new();
    for (index, lexed) in tokens.iter().enumerate() {
        match &lexed.token {
            Token::Identifier(word) if matches!(word.as_str(), "var" | "let" | "const" | "function" | "class") => {
                if let Some(name) = identifier(tokens.get(index + 1)) {
                    names.push(name);
                }
            }
            // `x => ...`: parâmetro único de arrow.
            Token::Identifier(word) if punctuator(tokens.get(index + 1)) == Some("=>") => names.push(word.as_str()),
            Token::Punctuator(")") => {
                let Some(open) = matching_open_paren(tokens, index) else { continue };
                let before = identifier(open.checked_sub(1).and_then(|before| tokens.get(before)));
                let follows = punctuator(tokens.get(index + 1));
                let parameters = follows == Some("=>")
                    || before == Some("catch")
                    || (follows == Some("{") && !before.is_some_and(|word| CONTROL_WORDS.contains(&word)));
                if parameters {
                    names.extend(tokens[open + 1..index].iter().filter_map(|lexed| identifier(Some(lexed))));
                }
            }
            _ => {}
        }
    }
    names
}

/// O que o arquivo tem de CommonJS e de módulo ES.
struct Markers {
    commonjs: bool,
    module_syntax: bool,
}

fn scan_markers(tokens: &[Lexed]) -> Markers {
    let declared = declared_names(tokens);
    let mut braces: Vec<Brace> = Vec::new();
    // O próximo `{` abre o corpo de uma classe (depois de `class Nome extends Base`).
    let mut class_pending = false;
    let mut markers = Markers { commonjs: false, module_syntax: false };
    for (index, lexed) in tokens.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|before| tokens.get(before));
        let next = tokens.get(index + 1);
        let in_function = braces.iter().any(|brace| matches!(brace, Brace::Function | Brace::Class));
        match &lexed.token {
            Token::Punctuator("{") => {
                let kind = match punctuator(previous) {
                    _ if class_pending => Brace::Class,
                    Some("=>") => Brace::Arrow,
                    Some(")") => {
                        let open = matching_open_paren(tokens, index - 1);
                        let before = open.and_then(|open| open.checked_sub(1)).and_then(|before| tokens.get(before));
                        match &before.map(|lexed| &lexed.token) {
                            Some(Token::Identifier(word)) if CONTROL_WORDS.contains(&word.as_str()) => Brace::Block,
                            Some(Token::Identifier(_)) | Some(Token::Punctuator("]" | "*")) => Brace::Function,
                            _ => Brace::Block,
                        }
                    }
                    _ => Brace::Block,
                };
                class_pending = false;
                braces.push(kind);
            }
            Token::Punctuator("}") => {
                braces.pop();
            }
            Token::Identifier(word) => {
                let after_dot = matches!(punctuator(previous), Some("." | "?."));
                match word.as_str() {
                    "class" if !after_dot => class_pending = true,
                    "this" if !after_dot && !in_function => markers.commonjs = true,
                    "with" if !after_dot && punctuator(next) == Some("(") => markers.commonjs = true,
                    "import" if !after_dot => {
                        let meta = punctuator(next) == Some(".") && identifier(tokens.get(index + 2)) == Some("meta");
                        let declaration = at_statement_start(tokens, index) && !matches!(punctuator(next), Some("(" | "."));
                        markers.module_syntax |= meta || declaration;
                    }
                    "export" if !after_dot && at_statement_start(tokens, index) => markers.module_syntax = true,
                    "await" if !after_dot && !braces.iter().any(|brace| *brace != Brace::Block) => markers.module_syntax = true,
                    name if MARKERS.contains(&name) && !after_dot && punctuator(next) != Some(":") && !declared.contains(&name) => {
                        // `typeof require;` sozinho é código morto que o transpilador descarta.
                        let dead_typeof = name == "require"
                            && identifier(previous) == Some("typeof")
                            && at_statement_start(tokens, index - 1)
                            && (next.is_none_or(|next| next.newline_before) || punctuator(next) == Some(";"));
                        markers.commonjs |= !dead_typeof;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    markers
}

/// A diretiva `"use strict"` como primeira instrução do arquivo (depois de comentários, como no bun).
fn leading_use_strict(tokens: &[Lexed]) -> bool {
    matches!(tokens.first().map(|lexed| &lexed.token), Some(Token::String(text)) if text == "use strict")
        && tokens.get(1).is_none_or(|next| next.newline_before || matches!(next.token, Token::Punctuator(";")))
}

/// O modo em que o bun roda `source` como arquivo principal `.js`.
pub fn main_module_kind(source: &str) -> MainModuleKind {
    let tokens = tokenize(source);
    let markers = scan_markers(&tokens);
    if leading_use_strict(&tokens) {
        return if markers.module_syntax && !markers.commonjs { MainModuleKind::Esm } else { MainModuleKind::CjsStrict };
    }
    if markers.commonjs { MainModuleKind::Cjs } else { MainModuleKind::Esm }
}

#[cfg(test)]
mod tests {
    use super::{main_module_kind, MainModuleKind::*};

    /// Os casos de fronteira medidos no bun 1.4.2 (arquivo `.js` sem `package.json`).
    #[test]
    fn module_kind_matches_bun() {
        let cases = [
            ("console.log(1);", Esm),
            ("var x = 1;", Esm),
            ("import fs from 'fs';", Esm),
            ("export const a = 1;", Esm),
            ("await 1;", Esm),
            ("console.log(typeof arguments);", Esm),
            ("console.log(new.target);", Esm),
            ("var p = import('fs');", Esm),
            ("var o = { require: 1 }; o.require;", Esm),
            ("var s = 'require';", Esm),
            ("globalThis.module;", Esm),
            ("var require = 1; require;", Esm),
            ("function f(module) { return module; }", Esm),
            ("var f = (exports) => exports;", Esm),
            ("var f = exports => exports;", Esm),
            ("function f() { return this; }", Esm),
            ("var o = { m() { return this; } };", Esm),
            ("class A { x = this; m() { return this; } }", Esm),
            ("typeof require;", Esm),
            ("var r = require('fs');", Cjs),
            ("var r = typeof require;", Cjs),
            ("module.exports = 1;", Cjs),
            ("exports.a = 1;", Cjs),
            ("console.log(__dirname);", Cjs),
            ("console.log(__filename);", Cjs),
            ("function f() { return typeof module; }", Cjs),
            ("console.log(this);", Cjs),
            ("var f = () => this;", Cjs),
            ("with ({}) {}", Cjs),
            ("'use strict';\nconsole.log(1);", CjsStrict),
            ("// c\n\"use strict\"\nvar a;", CjsStrict),
            ("'use strict'; import fs from 'fs';", Esm),
            ("'use strict'; export var a = 1;", Esm),
            ("var a; 'use strict';", Esm),
            ("var t = `${require('fs')}`;", Cjs),
            ("var re = /this/; var t = `this`;", Esm),
        ];
        for (source, expected) in cases {
            assert_eq!(main_module_kind(source), expected, "{source}");
        }
    }
}
