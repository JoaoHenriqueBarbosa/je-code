//! O URL `file://` de um caminho do sistema de arquivos: `WTF::URL::fileURLWithFileSystemPath(path)` do WebKit, no
//! ramo POSIX, sem o fragmento. É o que o `Bun.pathToFileURL`, o `import.meta.url` e o `url.pathToFileURL` do bun
//! entregam, e o que o transpilador (`wip/zbun`) inlina em `import.meta.url`.
//!
//! O WebKit faz duas coisas em sequência:
//!
//! 1. `escapeFilePathWithoutCopying`: percent-encode dos bytes UTF-8 do caminho que a tabela `filePathEscapeTable`
//!    marca (`escape_path`, também usada pelo zjsc, que passa a string pelo parser de URL dele);
//! 2. o `URLParser`, sobre `file://` + caminho: tira os controles do fim, percent-encode do conjunto de caminho e
//!    resolve os segmentos `.` e `..` (`file_url_from_path` reproduz só o que um caminho já escapado pode provocar).
//!
//! O zjsc confere as duas contra o parser de URL dele num corpus (`wtf::url::tests`), que é quem tem o parser
//! completo.

/// `isEscapeCharacter` do `escapeFilePathWithoutCopying`, sobre a `filePathEscapeTable` (POSIX): `\0`, `\t`, `\n`,
/// `\r`, espaço, `"`, `#`, `%`, `?`, `[ \ ] ^ | ~` e todo byte a partir de 0x80.
pub fn needs_escape(byte: u8) -> bool {
    const TABLE: [u64; 2] = [
        1 | (1 << 9) | (1 << 10) | (1 << 13) | (1 << 32) | (1 << 34) | (1 << 35) | (1 << 37) | (1 << 63),
        (1 << (b'[' - 64)) | (1 << (b'\\' - 64)) | (1 << (b']' - 64)) | (1 << (b'^' - 64)) | (1 << (b'|' - 64)) | (1 << (b'~' - 64)),
    ];
    byte >= 128 || (TABLE[(byte >> 6) as usize] >> (byte & 63)) & 1 != 0
}

/// `escapeFilePathWithoutCopying`: `%XX` maiúsculo para cada byte de `path` que `needs_escape` marca.
pub fn escape_path(path: &[u8]) -> String {
    let mut escaped = String::with_capacity(path.len());
    for &byte in path {
        if needs_escape(byte) {
            escaped.push_str(&format!("%{byte:02X}"));
        } else {
            escaped.push(byte as char);
        }
    }
    escaped
}

/// O conjunto de percent-encode de caminho do `URLParser` (C0, espaço, `"`, `#`, `<`, `>`, `?`, `` ` ``, `{`, `}`,
/// DEL e o que vem depois): o que o `escape_path` deixa passar e o parser ainda codifica.
fn parser_encodes_path_byte(byte: u8) -> bool {
    byte <= 0x20 || byte >= 0x7F || matches!(byte, b'"' | b'#' | b'<' | b'>' | b'?' | b'`' | b'{' | b'}')
}

/// `URL::fileURLWithFileSystemPath(path).stringWithoutFragmentIdentifier()` no ramo POSIX.
pub fn file_url_from_path(path: &[u8]) -> String {
    let mut input = escape_path(path).into_bytes();
    // O parser tira os controles (e o espaço) do fim da entrada inteira; o espaço já saiu como `%20`.
    while input.last().is_some_and(|&byte| byte <= 0x20) {
        input.pop();
    }
    // O caminho que o parser vê começa depois da `/` que abre o caminho (`file://` + `/` quando falta).
    let mut segments: Vec<Vec<u8>> = Vec::new();
    let tail: &[u8] = input.strip_prefix(b"/").unwrap_or(&input);
    let mut parts = tail.split(|&byte| byte == b'/').peekable();
    while let Some(part) = parts.next() {
        let last = parts.peek().is_none();
        match part {
            b".." => {
                // Uma letra de unidade normalizada (`C:`) sozinha no caminho não sai.
                if !(segments.len() == 1 && is_drive_letter(&segments[0])) {
                    segments.pop();
                }
                if last {
                    segments.push(Vec::new());
                }
            }
            b"." => {
                if last {
                    segments.push(Vec::new());
                }
            }
            _ => segments.push(part.iter().flat_map(|&byte| encode_for_path(byte)).collect()),
        }
    }
    let mut url = String::from("file://");
    for segment in &segments {
        url.push('/');
        url.extend(segment.iter().map(|&byte| byte as char));
    }
    if segments.is_empty() {
        url.push('/');
    }
    url
}

fn encode_for_path(byte: u8) -> Vec<u8> {
    if parser_encodes_path_byte(byte) { format!("%{byte:02X}").into_bytes() } else { vec![byte] }
}

/// `C:` com letra ASCII e dois-pontos (o `|` do URL de unidade antigo já foi escapado para `%7C`).
fn is_drive_letter(segment: &[u8]) -> bool {
    segment.len() == 2 && segment[0].is_ascii_alphabetic() && segment[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_what_the_webkit_table_marks() {
        assert_eq!(escape_path(b"/tmp/a b.js"), "/tmp/a%20b.js");
        assert_eq!(escape_path(b"/a#b?c%d"), "/a%23b%3Fc%25d");
        assert_eq!(escape_path("/\u{e9}".as_bytes()), "/%C3%A9");
        assert_eq!(escape_path(b"a[b]c^d|e~f\\g\"h"), "a%5Bb%5Dc%5Ed%7Ce%7Ef%5Cg%22h");
        assert_eq!(escape_path(b"a\0b\tc\nd\re"), "a%00b%09c%0Ad%0De");
    }

    #[test]
    fn file_url_follows_the_parser() {
        assert_eq!(file_url_from_path(b"/tmp/a b.js"), "file:///tmp/a%20b.js");
        assert_eq!(file_url_from_path(b"lib/a.js"), "file:///lib/a.js");
        assert_eq!(file_url_from_path(b"/a#b?c%d"), "file:///a%23b%3Fc%25d");
        assert_eq!(file_url_from_path(b""), "file:///");
        assert_eq!(file_url_from_path(b"/"), "file:///");
        assert_eq!(file_url_from_path(b"/a/./b/../c"), "file:///a/c");
        assert_eq!(file_url_from_path(b"/a/b/.."), "file:///a/");
        assert_eq!(file_url_from_path(b"/a/b/."), "file:///a/b/");
        assert_eq!(file_url_from_path(b"/../.."), "file:///");
        assert_eq!(file_url_from_path(b"//x//y"), "file:////x//y");
        assert_eq!(file_url_from_path(b"/a<b>c`d{e}"), "file:///a%3Cb%3Ec%60d%7Be%7D");
        assert_eq!(file_url_from_path(b"/a\x01"), "file:///a");
        assert_eq!(file_url_from_path(b"/a\x01b\x7f"), "file:///a%01b%7F");
        assert_eq!(file_url_from_path(b"/C:/.."), "file:///C:/");
    }
}
