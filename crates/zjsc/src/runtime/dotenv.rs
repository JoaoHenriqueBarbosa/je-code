//! O leitor de `.env` do bun (`Parser` de `bun_dotenv::env_loader`), só o que `util.parseEnv` usa: valores novos
//! sobrescrevem os antigos (`OVERWRITE`), sem expansão de `$VAR` (`EXPAND` falso) e fora do modo de processo
//! (`IS_PROCESS` falso, então o valor entre aspas sai sem elas).

/// Espaço em branco do `.env`. O texto é UTF-8: o conjunto é só ASCII, porque `0xA0` é byte de continuação (o NBSP é
/// `C2 A0`) e aparar byte a byte corromperia a sequência.
const WHITESPACE: &[u8] = b"\t\x0B\x0C \n\r";

fn is_whitespace(byte: u8) -> bool {
    WHITESPACE.contains(&byte)
}

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&byte| !is_whitespace(byte)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|&byte| !is_whitespace(byte)).map_or(start, |at| at + 1);
    &bytes[start..end]
}

struct Parser<'a> {
    pos: usize,
    src: &'a [u8],
    /// O valor entre aspas já sem as barras de escape, reaproveitado a cada linha.
    value: Vec<u8>,
}

impl<'a> Parser<'a> {
    fn skip_line(&mut self) {
        match self.src[self.pos..].iter().position(|&byte| byte == b'\n' || byte == b'\r') {
            Some(at) => self.pos += at + 1,
            None => self.pos = self.src.len(),
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.src.len() && is_whitespace(self.src[self.pos]) {
            self.pos += 1;
        }
    }

    /// A chave de `CHAVE=`, `CHAVE:` (seguido de espaço) ou `export CHAVE=`; `None` deixa a posição onde estava.
    fn parse_key(&mut self, check_export: bool) -> Option<&'a [u8]> {
        if check_export {
            self.skip_whitespace();
        }
        let start = self.pos;
        let mut end = start;
        while end < self.src.len() && matches!(self.src[end], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | b'.') {
            end += 1;
        }
        if end < self.src.len() && start < end {
            self.pos = end;
            self.skip_whitespace();
            if self.pos < self.src.len() {
                if check_export && end < self.pos && &self.src[start..end] == b"export" {
                    if let Some(key) = self.parse_key(false) {
                        return Some(key);
                    }
                }
                match self.src[self.pos] {
                    b'=' => {
                        self.pos += 1;
                        return Some(&self.src[start..end]);
                    }
                    b':' => {
                        let next = self.pos + 1;
                        if next < self.src.len() && is_whitespace(self.src[next]) {
                            self.pos += 2;
                            return Some(&self.src[start..end]);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.pos = start;
        None
    }

    /// O valor entre aspas (o `quote` está em `self.pos`) com o conteúdo cru em `self.value`: o primeiro `quote` sem
    /// escape fecha o valor e o resto da linha é descartado. Só as aspas duplas interpretam `\n` e `\r`; `\r` solto vira
    /// `\n`. Sem a aspa de fechamento, `false` e a posição fica.
    fn parse_quoted(&mut self, quote: u8) -> bool {
        let start = self.pos;
        self.value.clear();
        let mut end = start + 1;
        while end < self.src.len() {
            match self.src[end] {
                b'\\' => end += 1,
                byte if byte == quote => {
                    end += 1;
                    self.pos = end;
                    self.skip_line();
                    let mut at = start + 1;
                    while at < end - 1 {
                        match self.src[at] {
                            b'\\' if quote == b'"' => {
                                match self.src[at + 1] {
                                    b'n' => self.value.push(b'\n'),
                                    b'r' => self.value.push(b'\r'),
                                    _ => self.value.extend_from_slice(&self.src[at..at + 2]),
                                }
                                at += 2;
                            }
                            b'\\' => {
                                self.value.push(b'\\');
                                at += 1;
                            }
                            b'\r' => {
                                at += 1;
                                if at >= end - 1 || self.src[at] != b'\n' {
                                    self.value.push(b'\n');
                                }
                            }
                            byte => {
                                self.value.push(byte);
                                at += 1;
                            }
                        }
                    }
                    return true;
                }
                _ => {}
            }
            end += 1;
        }
        false
    }

    fn parse_value(&mut self) -> Vec<u8> {
        let start = self.pos;
        self.skip_whitespace();
        if self.pos >= self.src.len() {
            return Vec::new();
        }
        if matches!(self.src[self.pos], b'`' | b'"' | b'\'') && self.parse_quoted(self.src[self.pos]) {
            return std::mem::take(&mut self.value);
        }
        let end = self.src[start..].iter().position(|&byte| matches!(byte, b'#' | b'\r' | b'\n')).map_or(self.src.len(), |at| start + at);
        self.pos = end;
        trim(&self.src[start..end]).to_vec()
    }
}

/// Os pares `(chave, valor)` do texto, na ordem em que cada chave apareceu pela primeira vez; a chave repetida mantém o
/// lugar e fica com o último valor.
pub(crate) fn parse(source: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let source = source.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(source);
    let mut parser = Parser { pos: 0, src: source, value: Vec::new() };
    let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    while parser.pos < parser.src.len() {
        let Some(key) = parser.parse_key(true) else {
            parser.skip_line();
            continue;
        };
        let value = parser.parse_value();
        match entries.iter_mut().find(|(known, _)| known == key) {
            Some(entry) => entry.1 = value,
            None => entries.push((key.to_vec(), value)),
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn pairs(text: &str) -> Vec<(String, String)> {
        parse(text.as_bytes()).into_iter().map(|(key, value)| (String::from_utf8(key).unwrap(), String::from_utf8(value).unwrap())).collect()
    }

    fn pair(key: &str, value: &str) -> (String, String) {
        (key.to_owned(), value.to_owned())
    }

    /// Cada caso foi medido com `util.parseEnv` no bun 1.4.2.
    #[test]
    fn follows_bun_parse_env() {
        assert_eq!(pairs("a=1\nb=2\na=3"), vec![pair("a", "3"), pair("b", "2")]);
        assert_eq!(
            pairs("export A=1\nB: 2\nC='x y'\nD=\"l1\\nl2\"\nE=`bt`\nF=a#b\nG = 7 # c\n\u{FEFF}H=bom"),
            vec![pair("A", "1"), pair("B", "2"), pair("C", "x y"), pair("D", "l1\nl2"), pair("E", "bt"), pair("F", "a"), pair("G", "7")]
        );
        assert_eq!(pairs("A=\"unterminated\nB=2"), vec![pair("A", "\"unterminated"), pair("B", "2")]);
        assert_eq!(pairs("\u{FEFF}first=1"), vec![pair("first", "1")]);
        assert_eq!(pairs("ok=é\u{1F600}"), vec![pair("ok", "é\u{1F600}")]);
    }
}
