//! Citação de nomes como o quotearg do gnulib faz, escrita a partir da documentação dos estilos
//! (`--quoting-style`) e do comportamento observado: o estilo padrão do tar é `escape` (barra invertida,
//! escapes do C pra controles, octal de três dígitos pro resto que não é imprimível; UTF-8 imprimível
//! passa como está); as mensagens de erro usam o mesmo estilo com `:` também escapado.
//!
//! O motor é um só. O que cada programa faz de diferente (conjunto de caracteres seguros no estilo
//! `shell`, aspas duplas quando o nome tem `'`, bytes altos crus) é parâmetro: o [`Rules`].

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Literal,
    Shell,
    ShellAlways,
    ShellEscape,
    ShellEscapeAlways,
    C,
    CMaybe,
    Escape,
    Locale,
    CLocale,
}

impl Style {
    pub fn parse(s: &[u8]) -> Option<Style> {
        Some(match s {
            b"literal" => Style::Literal,
            b"shell" => Style::Shell,
            b"shell-always" => Style::ShellAlways,
            b"shell-escape" => Style::ShellEscape,
            b"shell-escape-always" => Style::ShellEscapeAlways,
            b"c" => Style::C,
            b"c-maybe" => Style::CMaybe,
            b"escape" => Style::Escape,
            b"locale" => Style::Locale,
            b"clocale" => Style::CLocale,
            _ => return None,
        })
    }

    pub const NAMES: &'static [&'static str] =
        &["literal", "shell", "shell-always", "shell-escape", "shell-escape-always", "c", "c-maybe", "escape", "locale", "clocale"];
}

/// Configuração de citação (estilo e caracteres extras).
#[derive(Clone, Debug)]
pub struct Quoting {
    pub style: Style,
    /// Caracteres ASCII adicionais a citar (`--quote-chars`).
    pub extra: Vec<u8>,
    /// Caracteres a não citar (`--no-quote-chars`).
    pub except: Vec<u8>,
}

impl Default for Quoting {
    fn default() -> Self {
        Quoting { style: Style::Escape, extra: Vec::new(), except: Vec::new() }
    }
}

/// As variações de comportamento que cada programa tem em cima do motor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Nome com `'` e sem `$`, crase, `"`, `\` e `!` sai entre aspas duplas em vez de `'\''`.
    pub prefer_double_quotes: bool,
    /// Nos estilos de barra invertida, bytes >= 0x80 passam crus (sem olhar UTF-8 nem octal).
    pub raw_high_bytes: bool,
}

impl Rules {
    /// O quotearg do gnulib, como o tar, o xargs e o diff usam.
    pub const GNULIB: Rules = Rules { prefer_double_quotes: false, raw_high_bytes: false };
    /// GNU patch.
    pub const PATCH: Rules = Rules { prefer_double_quotes: true, raw_high_bytes: true };
}

/// Decodifica o próximo caractere UTF-8 válido em `s`; `None` se o byte não começa um caractere válido.
fn next_char(s: &[u8]) -> Option<(char, usize)> {
    let n = match s.first()? {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let chunk = s.get(..n)?;
    let st = std::str::from_utf8(chunk).ok()?;
    st.chars().next().map(|c| (c, n))
}

/// Imprimível no sentido do `iswprint` do glibc em C.UTF-8 (aproximação: não é controle nem um dos
/// separadores invisíveis de formatação).
fn printable(c: char) -> bool {
    if c.is_control() {
        return false;
    }
    !matches!(c as u32, 0x2028 | 0x2029 | 0xfff9..=0xfffb | 0xe0000..=0xe007f) && !(0xd800..=0xdfff).contains(&(c as u32))
}

fn c_escape(b: u8) -> Option<&'static [u8]> {
    Some(match b {
        0x07 => b"\\a",
        0x08 => b"\\b",
        0x0c => b"\\f",
        b'\n' => b"\\n",
        b'\r' => b"\\r",
        b'\t' => b"\\t",
        0x0b => b"\\v",
        _ => return None,
    })
}

fn push_octal(out: &mut Vec<u8>, b: u8) {
    out.push(b'\\');
    out.push(b'0' + (b >> 6));
    out.push(b'0' + ((b >> 3) & 7));
    out.push(b'0' + (b & 7));
}

/// Estilos de barra invertida (`escape`, `c`, `c-maybe`, `locale`, `clocale`): corpo citado.
fn backslash_body(s: &[u8], q: &Quoting, rules: &Rules, quote_char: Option<u8>, colon: bool) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(s.len());
    let mut changed = false;
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        if b >= 0x80 {
            if rules.raw_high_bytes {
                out.push(b);
                i += 1;
                continue;
            }
            match next_char(&s[i..]) {
                Some((c, n)) if printable(c) => {
                    out.extend_from_slice(&s[i..i + n]);
                    i += n;
                }
                Some((_, n)) => {
                    for &x in &s[i..i + n] {
                        push_octal(&mut out, x);
                    }
                    changed = true;
                    i += n;
                }
                None => {
                    push_octal(&mut out, b);
                    changed = true;
                    i += 1;
                }
            }
            continue;
        }
        let forced = (q.extra.contains(&b) || (colon && b == b':')) && !q.except.contains(&b);
        if b == b'\\' {
            out.extend_from_slice(b"\\\\");
            changed = true;
        } else if let Some(e) = c_escape(b) {
            out.extend_from_slice(e);
            changed = true;
        } else if !(0x20..0x7f).contains(&b) {
            push_octal(&mut out, b);
            changed = true;
        } else if Some(b) == quote_char || forced {
            out.push(b'\\');
            out.push(b);
            changed = true;
        } else {
            out.push(b);
        }
        i += 1;
    }
    (out, changed)
}

/// Precisa de aspas no estilo `shell`: a pontuação segura é a do quotearg do gnulib, e `#` e `~` só
/// pedem aspas na primeira posição (conferido no tar, xargs, diff e patch do Debian 13).
fn shell_needs_quotes(s: &[u8]) -> bool {
    s.is_empty()
        || s.iter().enumerate().any(|(i, &b)| {
            !(b.is_ascii_alphanumeric() || b"%+,-./:@]_".contains(&b) || b >= 0x80 || (i > 0 && matches!(b, b'#' | b'~')))
        })
}

fn shell_quote(s: &[u8], always: bool, escape: bool, rules: &Rules) -> Vec<u8> {
    let has_unprintable = s.iter().any(|&b| b < 0x20 || b == 0x7f);
    if escape && has_unprintable {
        // $'...' com escapes do C.
        let mut out = b"'".to_vec();
        let mut in_dollar = false;
        for &b in s {
            let special = b < 0x20 || b == 0x7f;
            if special {
                if !in_dollar {
                    out.extend_from_slice(b"'$'");
                    in_dollar = true;
                }
                match c_escape(b) {
                    Some(e) => out.extend_from_slice(e),
                    None => push_octal(&mut out, b),
                }
            } else {
                if in_dollar {
                    out.extend_from_slice(b"''");
                    in_dollar = false;
                }
                if b == b'\'' {
                    out.extend_from_slice(b"'\\''");
                } else {
                    out.push(b);
                }
            }
        }
        out.push(b'\'');
        return out;
    }
    if !always && !shell_needs_quotes(s) {
        return s.to_vec();
    }
    if rules.prefer_double_quotes
        && s.contains(&b'\'')
        && !s.iter().any(|c| matches!(c, b'$' | b'`' | b'"' | b'\\' | b'!'))
    {
        let mut out = b"\"".to_vec();
        out.extend_from_slice(s);
        out.push(b'"');
        return out;
    }
    let mut out = b"'".to_vec();
    for &b in s {
        if b == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out.push(b'\'');
    out
}

/// Cita `s` no estilo configurado, com as regras de um programa. `colon` também escapa `:` (o
/// `quotearg_colon` das mensagens).
pub fn quote_rules(s: &[u8], q: &Quoting, rules: &Rules, colon: bool) -> Vec<u8> {
    match q.style {
        Style::Literal => s.to_vec(),
        Style::Escape => backslash_body(s, q, rules, None, colon).0,
        Style::C => {
            let (body, _) = backslash_body(s, q, rules, Some(b'"'), colon);
            let mut out = b"\"".to_vec();
            out.extend_from_slice(&body);
            out.push(b'"');
            out
        }
        Style::CMaybe => {
            let (body, changed) = backslash_body(s, q, rules, Some(b'"'), colon);
            if changed {
                let mut out = b"\"".to_vec();
                out.extend_from_slice(&body);
                out.push(b'"');
                out
            } else {
                body
            }
        }
        Style::Locale | Style::CLocale => {
            let (body, _) = backslash_body(s, q, rules, None, colon);
            let mut out = "\u{2018}".as_bytes().to_vec();
            out.extend_from_slice(&body);
            out.extend_from_slice("\u{2019}".as_bytes());
            out
        }
        Style::Shell => shell_quote(s, false, false, rules),
        Style::ShellAlways => shell_quote(s, true, false, rules),
        Style::ShellEscape => shell_quote(s, false, true, rules),
        Style::ShellEscapeAlways => shell_quote(s, true, true, rules),
    }
}

/// Cita `s` no estilo configurado, com as regras do GNU tar.
pub fn quote_with(s: &[u8], q: &Quoting, colon: bool) -> Vec<u8> {
    quote_rules(s, q, &Rules::GNULIB, colon)
}

/// Estilo padrão do tar (`escape`).
pub fn escape(s: &[u8]) -> Vec<u8> {
    quote_with(s, &Quoting::default(), false)
}

/// Estilo das mensagens de erro do tar: `escape` com `:` escapado.
pub fn colon(s: &[u8]) -> Vec<u8> {
    quote_with(s, &Quoting::default(), true)
}

/// Aspas do locale (‘x’), usadas em mensagens como "invalid argument ‘foo’ for ‘--sort’".
pub fn locale(s: &[u8]) -> Vec<u8> {
    quote_with(s, &Quoting { style: Style::Locale, ..Quoting::default() }, false)
}

/// Aspas do shell só quando precisam (estilo `shell`), com as regras de um programa.
pub fn shell(s: &[u8], rules: &Rules) -> Vec<u8> {
    shell_quote(s, false, false, rules)
}

/// Byte no estilo `cat -v`: `^A`, `^?`, `M-^@`, `M-a`.
pub fn cat_v(c: u8) -> String {
    let mut s = String::new();
    let mut c = c;
    if c >= 0x80 {
        s.push_str("M-");
        c -= 0x80;
    }
    if c < 0x20 {
        s.push('^');
        s.push((c + 0x40) as char);
    } else if c == 0x7f {
        s.push_str("^?");
    } else {
        s.push(c as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_like_gnu_tar() {
        assert_eq!(escape(b"d/bs\\x"), b"d/bs\\\\x");
        assert_eq!(escape(b"d/ctl\x01x"), b"d/ctl\\001x");
        assert_eq!(escape(b"d/hi\xffx"), b"d/hi\\377x");
        assert_eq!(escape(b"d/nl\nx"), b"d/nl\\nx");
        assert_eq!(escape(b"d/tab\tx"), b"d/tab\\tx");
        assert_eq!(escape("d/ünï ç".as_bytes()), "d/ünï ç".as_bytes());
        assert_eq!(escape(b"d/q\"x'y?*"), b"d/q\"x'y?*");
        assert_eq!(colon(b"x:y"), b"x\\:y");
    }

    #[test]
    fn c_style() {
        let q = Quoting { style: Style::C, ..Quoting::default() };
        assert_eq!(quote_with(b"d/q\"x", &q, false), b"\"d/q\\\"x\"");
        assert_eq!(quote_with(b"a", &q, false), b"\"a\"");
    }

    #[test]
    fn locale_style() {
        assert_eq!(locale(b"foo"), "\u{2018}foo\u{2019}".as_bytes());
    }

    #[test]
    fn shell_with_diff_rules() {
        assert_eq!(shell(b"-r", &Rules::GNULIB), b"-r");
        assert_eq!(shell(b"--unified=1", &Rules::GNULIB), b"'--unified=1'");
        assert_eq!(shell(b"a b", &Rules::GNULIB), b"'a b'");
        assert_eq!(shell(b"it's", &Rules::GNULIB), b"'it'\\''s'");
        assert_eq!(shell(b"", &Rules::GNULIB), b"''");
    }

    #[test]
    fn shell_like_debian_tar_and_xargs() {
        // `tar --quoting-style=shell -t` e `xargs -t` no Debian 13.
        assert_eq!(shell(b"a=b", &Rules::GNULIB), b"'a=b'");
        assert_eq!(shell(b"x^y", &Rules::GNULIB), b"'x^y'");
        assert_eq!(shell(b"#h", &Rules::GNULIB), b"'#h'");
        assert_eq!(shell(b"a#b", &Rules::GNULIB), b"a#b");
        assert_eq!(shell(b"~u", &Rules::GNULIB), b"'~u'");
        assert_eq!(shell(b"q]r", &Rules::GNULIB), b"q]r");
    }

    #[test]
    fn shell_with_patch_rules() {
        // `patching file ...` no Debian 13.
        assert_eq!(shell(b"q]r", &Rules::PATCH), b"q]r");
        assert_eq!(shell(b"a=b", &Rules::PATCH), b"'a=b'");
        assert_eq!(shell(b"a#b", &Rules::PATCH), b"a#b");
        assert_eq!(shell(b"a.txt", &Rules::PATCH), b"a.txt");
        assert_eq!(shell(b"a b.txt", &Rules::PATCH), b"'a b.txt'");
        assert_eq!(shell(b"it's.txt", &Rules::PATCH), b"\"it's.txt\"");
        assert_eq!(shell(b"it's$x", &Rules::PATCH), b"'it'\\''s$x'");
    }

    #[test]
    fn patch_backslash_styles() {
        let esc = Quoting { style: Style::Escape, extra: vec![b' '], except: Vec::new() };
        assert_eq!(quote_rules(b"a b\xc3\xa9\x01", &esc, &Rules::PATCH, false), b"a\\ b\xc3\xa9\\001");
        let c = Quoting { style: Style::C, ..Quoting::default() };
        assert_eq!(quote_rules(b"a \"b\"\xff", &c, &Rules::PATCH, false), b"\"a \\\"b\\\"\xff\"");
    }

    #[test]
    fn cat_v_notation() {
        assert_eq!(cat_v(1), "^A");
        assert_eq!(cat_v(0x7f), "^?");
        assert_eq!(cat_v(0x80), "M-^@");
        assert_eq!(cat_v(0xff), "M-^?");
        assert_eq!(cat_v(b'x'), "x");
    }
}
