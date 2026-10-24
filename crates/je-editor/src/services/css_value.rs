//! Porte de `src/vs/base/browser/cssValue.ts`, no que as regras de decoração usam: `asCSSUrl`, e o `CSS.escape`
//! do navegador que ele chama (a serialização de identificador do CSSOM).
//!
//! Diferença do original: `FileAccess.uriToBrowserUri(uri)` é o parâmetro `uri_to_browser_uri` (o
//! `FileAccess` do original é um singleton global que o porte não tem; o chamador passa o
//! `FileAccess::uri_to_browser_uri` da plataforma, ou a identidade quando o contexto não é nativo).

use je_base::uri::URI;

/// `CSS.escape(value)`: <https://drafts.csswg.org/cssom/#serialize-an-identifier>.
pub fn css_escape(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::with_capacity(value.len());
    for (index, &ch) in chars.iter().enumerate() {
        let code = ch as u32;
        if code == 0 {
            // If the character is NULL (U+0000), then the REPLACEMENT CHARACTER (U+FFFD).
            result.push('\u{FFFD}');
        } else if (0x01..=0x1F).contains(&code)
            || code == 0x7F
            || (index == 0 && ch.is_ascii_digit())
            || (index == 1 && ch.is_ascii_digit() && chars[0] == '-')
        {
            // A control character, or a digit in a position where an identifier cannot have one: escaped
            // as a code point.
            result.push_str(&format!("\\{code:x} "));
        } else if index == 0 && ch == '-' && chars.len() == 1 {
            result.push_str("\\-");
        } else if code >= 0x80 || ch == '-' || ch == '_' || ch.is_ascii_alphanumeric() {
            result.push(ch);
        } else {
            result.push('\\');
            result.push(ch);
        }
    }
    result
}

/// `sizeValue(value)`: só `[\w.%+-]` sobrevive (o `console.warn` do original não existe).
pub fn size_value(value: &str) -> String {
    value.chars().filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '%' | '+' | '-')).collect()
}

/// `hexColorValue(value)`: o original usa `/[^[0-9a-fA-F#]]/gi`, que é a classe negada `[^[0-9a-fA-F#]` seguida de um
/// `]` literal, então só remove os pares "caractere fora da classe + `]`" (um valor sem `]` passa intacto).
pub fn hex_color_value(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let in_class = |ch: char| ch.is_ascii_hexdigit() || matches!(ch, '[' | '#');
    let mut result = String::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        if !in_class(chars[index]) && chars.get(index + 1) == Some(&']') {
            index += 2;
        } else {
            result.push(chars[index]);
            index += 1;
        }
    }
    result
}

/// `identValue(value)`: só `[_\-a-z0-9]` sobrevive.
pub fn ident_value(value: &str) -> String {
    value.chars().filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')).collect()
}

/// `stringValue(value)`: `'...'` com a aspa simples escapada.
pub fn string_value(value: &str) -> String {
    format!("'{}'", value.replace('\'', "\\000027"))
}

/// `className(value, escapingExpected)`: o `CSS.escape` do valor.
pub fn class_name(value: &str) -> String {
    css_escape(value)
}

/// `asCSSUrl(uri)`: `url('...')`.
pub fn as_css_url(uri: Option<&URI>, uri_to_browser_uri: &dyn Fn(&URI) -> URI) -> String {
    let Some(uri) = uri else {
        return "url('')".to_string();
    };
    format!("url('{}')", css_escape(&uri_to_browser_uri(uri).to_string(true).to_string_lossy()))
}
