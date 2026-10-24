//! O `QuickAndDirtyJavaScriptSyntaxHighlighter` do bun (`src/bun_core/fmt.rs`), sem a parte de redação de segredos:
//! a coloração da linha de fonte que o relato de erro mostra quando as cores estão ligadas. Só texto ASCII de até
//! 2048 bytes é colorido; qualquer outro sai cru.

const RESET: &str = "\x1b[0m";
const MAX_HIGHLIGHT_BYTES: usize = 2048;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keyword {
    New,
    Import,
    Typed,
    Other,
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

/// A cor da palavra-chave (`Keyword::color_code`) e a classe que o laço usa para o que vem depois, ou `None` se a
/// palavra não é chave.
fn keyword(word: &[u8]) -> Option<(&'static str, Keyword)> {
    const MAGENTA: &str = "\x1b[35m";
    const BLUE: &str = "\x1b[34m";
    const ORANGE: &str = "\x1b[33m";
    const RED: &str = "\x1b[31m";
    let word = std::str::from_utf8(word).ok()?;
    Some(match word {
        "new" => (MAGENTA, Keyword::New),
        "import" => (MAGENTA, Keyword::Import),
        "abstract" | "namespace" | "declare" | "type" | "interface" => (BLUE, Keyword::Typed),
        "as" | "enum" | "implements" | "private" | "protected" | "public" | "string" | "number" | "boolean" | "symbol" | "any" | "object"
        | "unknown" | "never" | "readonly" => (BLUE, Keyword::Other),
        "delete" => (RED, Keyword::Other),
        "undefined" | "false" | "null" | "this" | "true" => (ORANGE, Keyword::Other),
        "async" | "await" | "case" | "catch" | "class" | "const" | "continue" | "debugger" | "default" | "do" | "else" | "break" | "export"
        | "extends" | "finally" | "for" | "function" | "if" | "in" | "instanceof" | "let" | "package" | "return" | "static" | "super"
        | "switch" | "throw" | "try" | "typeof" | "var" | "void" | "while" | "with" | "yield" => (MAGENTA, Keyword::Other),
        _ => return None,
    })
}

/// Acrescenta a `out` o `text` colorido como o bun faz com `enable_colors`; sem `colors`, ou com texto vazio, maior
/// que 2048 bytes ou não ASCII, o texto cru.
pub fn highlight_javascript(out: &mut Vec<u8>, text: &[u8], colors: bool) {
    if !colors || text.len() > MAX_HIGHLIGHT_BYTES || text.is_empty() || !text.is_ascii() {
        out.extend_from_slice(text);
        return;
    }
    highlight(out, text);
}

fn push(out: &mut Vec<u8>, parts: &[&[u8]]) {
    for part in parts {
        out.extend_from_slice(part);
    }
}

fn highlight(out: &mut Vec<u8>, mut text: &[u8]) {
    let reset = RESET.as_bytes();
    let mut previous: Option<Keyword> = None;
    'outer: while !text.is_empty() {
        if is_identifier_start(text[0]) {
            let mut end = 1;
            while end < text.len() && is_identifier_continue(text[end]) {
                end += 1;
            }
            let word = &text[..end];
            if let Some((color, class)) = keyword(word) {
                if word != b"as" {
                    previous = Some(class);
                }
                push(out, &[reset, color.as_bytes(), word, reset]);
            } else {
                match previous {
                    Some(Keyword::New) => {
                        previous = None;
                        if end < text.len() && text[end] == b'(' {
                            push(out, &[reset, b"\x1b[1m", word, reset]);
                            text = &text[end..];
                            continue;
                        }
                    }
                    Some(Keyword::Typed) => {
                        push(out, &[reset, b"\x1b[1m\x1b[34m", word, reset]);
                        previous = None;
                        text = &text[end..];
                        continue;
                    }
                    Some(Keyword::Import) if word == b"from" => {
                        push(out, &[reset, b"\x1b[35m", word, reset]);
                        previous = None;
                        text = &text[end..];
                        continue;
                    }
                    _ => {}
                }
                out.extend_from_slice(word);
            }
            text = &text[end..];
            continue;
        }
        match text[0] {
            b'0'..=b'9' => {
                previous = None;
                let mut end = 1;
                if text.len() > 1 && text[0] == b'0' && matches!(text[1], b'x' | b'X') {
                    end += 1;
                    while end < text.len() && (text[end].is_ascii_hexdigit() || text[end] == b'_') {
                        end += 1;
                    }
                } else {
                    while end < text.len() && matches!(text[end], b'0'..=b'9' | b'.' | b'e' | b'E' | b'x' | b'X' | b'b' | b'B' | b'o' | b'O' | b'_') {
                        end += 1;
                    }
                }
                if end < text.len() && text[end] == b'n' {
                    end += 1;
                }
                push(out, &[reset, b"\x1b[33m", &text[..end], reset]);
                text = &text[end..];
            }
            quote @ (b'`' | b'"' | b'\'') => {
                previous = None;
                let mut end = 1;
                while end < text.len() && text[end] != quote {
                    if quote == b'`' && text[end] == b'$' && end + 1 < text.len() && text[end + 1] == b'{' {
                        let curly_start = end;
                        end += 2;
                        while end < text.len() && text[end] != b'}' {
                            if end + 1 < text.len() && text[end] == b'\\' {
                                end += 1;
                            }
                            end += 1;
                        }
                        push(out, &[reset, b"\x1b[32m", &text[..curly_start], reset, b"${"]);
                        if curly_start + 2 < end {
                            highlight(out, &text[curly_start + 2..end]);
                        }
                        if end < text.len() && text[end] == b'}' {
                            out.push(b'}');
                            end += 1;
                        }
                        text = &text[end..];
                        end = 0;
                        if !text.is_empty() && text[0] == quote {
                            push(out, &[reset, b"\x1b[32m`", reset]);
                            text = &text[1..];
                            continue 'outer;
                        }
                        continue;
                    }
                    if end + 1 < text.len() && text[end] == b'\\' {
                        end += 1;
                    }
                    end += 1;
                }
                end += usize::from(end < text.len());
                push(out, &[reset, b"\x1b[32m", &text[..end], reset]);
                text = &text[end..];
            }
            b'/' => {
                previous = None;
                let mut end = 1;
                if end < text.len() && text[end] == b'/' {
                    while end < text.len() && text[end] != b'\n' {
                        end += 1;
                    }
                    let shown = &text[..end];
                    if end < text.len() && text[end] == b'\n' {
                        end += 1;
                    }
                    if end < text.len() && text[end] == b'\r' {
                        end += 1;
                    }
                    push(out, &[reset, b"\x1b[2m", shown, reset]);
                    text = &text[end..];
                    continue;
                }
                if end < text.len() && text[end] == b'*' {
                    end += 1;
                    while end + 2 < text.len() && &text[end..end + 2] != b"*/" {
                        end += 1;
                    }
                    if end + 2 < text.len() && &text[end..end + 2] == b"*/" {
                        end += 2;
                        push(out, &[reset, b"\x1b[2m", &text[..end], reset]);
                        text = &text[end..];
                        continue;
                    }
                    end = 1;
                }
                out.extend_from_slice(&text[..end]);
                text = &text[end..];
            }
            brace @ (b'}' | b'{') => {
                if previous != Some(Keyword::Import) {
                    previous = None;
                }
                out.push(brace);
                text = &text[1..];
            }
            bracket @ (b'[' | b']') => {
                previous = None;
                out.push(bracket);
                text = &text[1..];
            }
            b';' => {
                previous = None;
                push(out, &[reset, b"\x1b[2m;", reset]);
                text = &text[1..];
            }
            b'.' => {
                previous = None;
                if text.len() > 1 && (is_identifier_start(text[1]) || text[1] == b'#') {
                    let mut end = 2;
                    while end < text.len() && is_identifier_continue(text[end]) {
                        end += 1;
                    }
                    if end < text.len() && text[end] == b'(' {
                        push(out, &[reset, b"\x1b[3m\x1b[1m", &text[..end], reset]);
                        text = &text[end..];
                        continue;
                    }
                }
                out.push(b'.');
                text = &text[1..];
            }
            b'<' => {
                push(out, &[reset, b"<", reset]);
                text = &text[1..];
            }
            other => {
                out.push(other);
                text = &text[1..];
            }
        }
    }
}
