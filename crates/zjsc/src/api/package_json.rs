//! Leitor de `package.json` do Bun, o único do crate: aceita BOM, comentários (`//` e `/* */`) e vírgula
//! final; JSON inválido vira `None` (o Bun trata como "sem campo", sem erro). Guarda só o que a
//! resolução de módulos precisa: strings, `null`, objetos (na ordem do arquivo) e vetores.

pub enum Json {
  Str(String),
  Null,
  Object(Vec<(String, Json)>),
  Array(Vec<Json>),
  /// Número, `true`, `false`: o resolvedor só precisa saber que não é string nem objeto.
  Other,
}

impl Json {
  /// Primeiro membro com a chave `key` (chave repetida: vale a primeira), se `self` for objeto.
  pub fn get(&self, key: &str) -> Option<&Json> {
    match self {
      Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
      _ => None,
    }
  }
}

/// Lê o documento inteiro. Lixo depois do primeiro valor é ignorado.
pub fn parse(text: &str) -> Option<Json> {
  Reader { s: text.strip_prefix('\u{feff}').unwrap_or(text).as_bytes(), i: 0 }.value()
}

struct Reader<'a> {
  s: &'a [u8],
  i: usize,
}

impl Reader<'_> {
  fn peek(&self) -> Option<u8> {
    self.s.get(self.i).copied()
  }

  fn eat(&mut self, c: u8) -> Option<()> {
    self.skip_ws();
    (self.peek() == Some(c)).then(|| self.i += 1)
  }

  fn skip_ws(&mut self) {
    loop {
      match self.peek() {
        Some(b' ' | b'\t' | b'\n' | b'\r') => self.i += 1,
        Some(b'/') if self.s.get(self.i + 1) == Some(&b'/') => {
          while self.peek().is_some_and(|c| c != b'\n') {
            self.i += 1;
          }
        }
        Some(b'/') if self.s.get(self.i + 1) == Some(&b'*') => {
          self.i += 2;
          while self.i < self.s.len() && !self.s[self.i..].starts_with(b"*/") {
            self.i += 1;
          }
          self.i = (self.i + 2).min(self.s.len());
        }
        _ => return,
      }
    }
  }

  fn string(&mut self) -> Option<String> {
    self.eat(b'"')?;
    let mut out: Vec<u8> = Vec::new();
    loop {
      let c = self.peek()?;
      self.i += 1;
      match c {
        b'"' => return String::from_utf8(out).ok(),
        b'\\' => {
          let e = self.peek()?;
          self.i += 1;
          match e {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'u' => {
              let hex = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
              self.i += 4;
              let ch = char::from_u32(u32::from_str_radix(hex, 16).ok()?).unwrap_or('\u{fffd}');
              out.extend_from_slice(ch.to_string().as_bytes());
            }
            other => out.push(other),
          }
        }
        _ => out.push(c),
      }
    }
  }

  /// Itens separados por vírgula (com vírgula final) até `close`; `item` lê um item.
  fn list<T>(&mut self, close: u8, mut item: impl FnMut(&mut Self) -> Option<T>) -> Option<Vec<T>> {
    let mut items = Vec::new();
    loop {
      self.skip_ws();
      if self.peek() == Some(close) {
        self.i += 1;
        return Some(items);
      }
      items.push(item(self)?);
      self.skip_ws();
      match self.peek()? {
        b',' => self.i += 1,
        c if c == close => {}
        _ => return None,
      }
    }
  }

  fn value(&mut self) -> Option<Json> {
    self.skip_ws();
    match self.peek()? {
      b'"' => self.string().map(Json::Str),
      b'{' => {
        self.i += 1;
        let members = self.list(b'}', |r| {
          let key = r.string()?;
          r.eat(b':')?;
          Some((key, r.value()?))
        })?;
        Some(Json::Object(members))
      }
      b'[' => {
        self.i += 1;
        self.list(b']', Self::value).map(Json::Array)
      }
      _ => {
        let start = self.i;
        while self.peek().is_some_and(|c| !matches!(c, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')) {
          self.i += 1;
        }
        (self.i > start).then(|| if &self.s[start..self.i] == b"null" { Json::Null } else { Json::Other })
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lenient_syntax_and_first_duplicate_wins() {
    let doc = parse("\u{feff}// c\n{/* x */\"a\":\"1\",\"a\":\"2\",\"b\":[1,null,],}").unwrap();
    assert!(matches!(doc.get("a"), Some(Json::Str(s)) if s == "1"));
    assert!(matches!(doc.get("b"), Some(Json::Array(v)) if v.len() == 2));
    assert!(parse("{\"a\":1 \"b\":2}").is_none());
    assert!(parse("{\"a\":").is_none());
  }
}
