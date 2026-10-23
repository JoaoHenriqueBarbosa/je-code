//! Leitor de JSON com comentários e vírgula sobrando, como o `jsonc-parser` que o VS Code usa nos
//! `package.json` das extensões e nos arquivos de tema.

use serde_json::Value;

/// Remove comentários `//` e `/* */` e vírgulas antes de `}` ou `]`, fora de strings.
fn strip(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                i += 1;
                if chars[i - 1] == '"' {
                    break;
                }
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    drop_trailing_commas(&out)
}

fn drop_trailing_commas(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut in_string = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_string {
            out.push(c);
            if c == '"' && !is_escaped(&chars, i) {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn is_escaped(chars: &[char], index: usize) -> bool {
    chars[..index].iter().rev().take_while(|&&c| c == '\\').count() % 2 == 1
}

pub fn parse(source: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(&strip(source.trim_start_matches('\u{feff}')))
}
