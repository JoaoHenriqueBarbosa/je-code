//! Porte de `WTF/wtf/URL.cpp`: o que ficou fora de `url_parser.rs` (que tem a `class URL` com o
//! `URLParser`): o construtor `URL(String&&)`, `strippedForUseAsReport`, `fileSystemPath`,
//! `fileURLWithFileSystemPath` e `operator==`. O tipo é um só, `url_parser::URL`.

use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::String as WtfString;
pub use crate::wtf::url_parser::URL;
use crate::wtf::url_parser::URLParser;
use crate::wtf::url_query::QueryEncoding;

/// `decodeEscapeSequence` (URL.cpp): `%XX` em `index`, ou nada.
fn decode_escape_sequence(input: &WtfString, index: u32, length: u32) -> Option<u8> {
    if index + 3 > length || input.code_unit_at(index) != '%' as u16 {
        return None;
    }
    let digit = |i: u32| char::from_u32(input.code_unit_at(i) as u32).and_then(|c| c.to_digit(16));
    let (high, low) = (digit(index + 1)?, digit(index + 2)?);
    Some((high * 16 + low) as u8)
}

/// `decodeEscapeSequencesFromParsedURL` (URL.cpp): os `%XX` desfeitos e o resultado lido como UTF-8.
fn decode_escape_sequences_from_parsed_url(input: &WtfString) -> WtfString {
    let length = input.length();
    let has_percent = (0..length).any(|i| input.code_unit_at(i) == '%' as u16);
    if length < 3 || !has_percent {
        return input.clone();
    }
    let mut percent_decoded: Vec<u8> = Vec::with_capacity(length as usize);
    let mut i = 0;
    while i < length {
        if let Some(decoded) = decode_escape_sequence(input, i, length) {
            percent_decoded.push(decoded);
            i += 3;
        } else {
            percent_decoded.push(input.code_unit_at(i) as u8);
            i += 1;
        }
    }
    WtfString::from_utf8(&percent_decoded)
}

/// `escapeFilePathWithoutCopying` (URL.cpp): escapa os bytes UTF-8 do caminho. A tabela de escape mora em
/// `ul_common::file_url`, que o transpilador do bun também usa.
fn escape_file_path(path: &WtfString) -> WtfString {
    WtfString::from_latin1(ul_common::file_url::escape_path(&path.utf8(ConversionMode::LenientConversion)).as_bytes())
}

impl URL {
    /// `URL(String&& absoluteURL, const URLTextEncoding* = nullptr)`.
    pub fn from_string(absolute_url: &WtfString) -> URL {
        URLParser::parse_url(absolute_url, &URL::default(), QueryEncoding::None)
    }

    /// `URL::user()`: o usuário com os `%XX` desfeitos.
    pub fn user(&self) -> WtfString {
        decode_escape_sequences_from_parsed_url(&self.encoded_user())
    }

    /// `URL::password()`: a senha com os `%XX` desfeitos.
    pub fn password(&self) -> WtfString {
        decode_escape_sequences_from_parsed_url(&self.encoded_password())
    }

    /// `URL::hasOpaquePath()`.
    pub fn has_opaque_path(&self) -> bool {
        self.has_opaque_path
    }

    /// `URL::strippedForUseAsReport()`: tira as credenciais, a consulta e o fragmento.
    pub fn stripped_for_use_as_report(&self) -> WtfString {
        if !self.is_valid {
            return self.string.clone();
        }

        let end = self.credentials_end();
        if self.user_start == end && self.path_end == self.string.length() {
            return self.string.clone();
        }

        let mut result: Vec<u16> = Vec::new();
        for i in 0..self.user_start {
            result.push(self.string.code_unit_at(i));
        }
        for i in end..self.path_end {
            result.push(self.string.code_unit_at(i));
        }
        WtfString::from_utf16(&result)
    }

    /// `URL::fileSystemPath()` (ramo POSIX).
    pub fn file_system_path(&self) -> WtfString {
        if !self.protocol_is_file() {
            return WtfString::default();
        }
        decode_escape_sequences_from_parsed_url(&self.path())
    }

    /// `URL::fileURLWithFileSystemPath(StringView)` (ramo POSIX).
    pub fn file_url_with_file_system_path(path: &WtfString) -> URL {
        let mut text: Vec<u16> = "file://".encode_utf16().collect();
        if !path.starts_with_character('/' as u16) {
            text.push('/' as u16);
        }
        let escaped = escape_file_path(path);
        text.extend((0..escaped.length()).map(|i| escaped.code_unit_at(i)));
        URL::from_string(&WtfString::from_utf16(&text))
    }
}

impl PartialEq for URL {
    /// `operator==(const URL&, const URL&)`: igualdade das strings.
    fn eq(&self, other: &URL) -> bool {
        self.string == other.string
    }
}

impl Eq for URL {}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> URL {
        URL::from_string(&WtfString::from_utf8(text.as_bytes()))
    }

    #[test]
    fn host_and_report() {
        let u = url("https://user:pw@example.com:8080/a/b?q=1#f");
        assert_eq!(u.host(), WtfString::from_latin1(b"example.com"));
        assert_eq!(u.stripped_for_use_as_report(), WtfString::from_latin1(b"https://example.com:8080/a/b"));
    }

    #[test]
    fn relative_is_invalid() {
        let u = url("foo.js");
        assert!(!u.is_valid());
        assert!(u.host().is_null());
        assert_eq!(u.stripped_for_use_as_report(), WtfString::from_latin1(b"foo.js"));
    }

    #[test]
    fn file_path() {
        assert_eq!(url("file:///tmp/a%20b.js").file_system_path(), WtfString::from_latin1(b"/tmp/a b.js"));
        assert_eq!(url("file:///t/nonexist/%2e%2E/real.js").file_system_path(), WtfString::from_latin1(b"/t/real.js"));
        assert_eq!(url("https://x/a").file_system_path(), WtfString::default());
    }

    #[test]
    fn file_url_from_path() {
        let from = |path: &str| URL::file_url_with_file_system_path(&WtfString::from_utf8(path.as_bytes())).string().clone();
        assert_eq!(from("/tmp/a b.js"), WtfString::from_latin1(b"file:///tmp/a%20b.js"));
        assert_eq!(from("lib/a.js"), WtfString::from_latin1(b"file:///lib/a.js"));
        assert_eq!(from("/a#b?c%d"), WtfString::from_latin1(b"file:///a%23b%3Fc%25d"));
        assert_eq!(from("/\u{e9}"), WtfString::from_latin1(b"file:///%C3%A9"));
    }

    /// `ul_common::file_url::file_url_from_path` (o que o transpilador do bun usa) reproduz o parser completo daqui
    /// num corpus de caminhos com segmentos de ponto, controles, delimitadores e não ASCII.
    #[test]
    fn file_url_from_path_matches_the_full_parser() {
        let pieces: [&[u8]; 18] = [
            b"/", b"a", b"b c", b".", b"..", b"%2e", b"%", b"#", b"?", b"<>", b"`{}", b"\x01", b"\x7f", b"\0", b"C:", "\u{e9}".as_bytes(), b"\\", b"|~",
        ];
        let mut corpus: Vec<Vec<u8>> = vec![Vec::new()];
        for _ in 0..4 {
            let next: Vec<Vec<u8>> = corpus.iter().flat_map(|prefix| pieces.iter().map(move |piece| [prefix.as_slice(), *piece].concat())).collect();
            corpus.extend(next);
            corpus.truncate(40_000);
        }
        for path in &corpus {
            // O `#` sai escapado, então não há fragmento a tirar: o `stringWithoutFragmentIdentifier` é a string.
            let url = URL::file_url_with_file_system_path(&WtfString::from_utf8(path));
            let parsed = String::from_utf8_lossy(&url.string().utf8(ConversionMode::LenientConversion)).into_owned();
            assert_eq!(ul_common::file_url::file_url_from_path(path), parsed, "caminho {path:?}");
        }
    }
}
