//! Porte de `src/vs/base/common/envfile.ts`: lê um arquivo `.env`/`.envrc` padrão e devolve as
//! variáveis de ambiente que ele define.
//!
//! O `Map` do original guarda a ordem de inserção, e atribuir a uma chave já presente mantém a posição
//! dela. [`EnvVars`] faz o mesmo.

/// O `Map<string, string>` que `parseEnvFile` devolve.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvVars(Vec<(String, String)>);

impl EnvVars {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Os pares na ordem de inserção.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    fn set(&mut self, key: String, value: String) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key, value)),
        }
    }
}

/// `String.prototype.trim`: o conjunto de espaços do JavaScript, não o do Rust.
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.len_utf16() == 1 && crate::js_string::is_js_whitespace(c as u16))
}

/// `parseEnvFile(src)`.
pub fn parse_env_file(src: &str) -> EnvVars {
    let mut result = EnvVars::default();

    // Normalize line breaks: `\r\n?` vira `\n`.
    let normalized = src.replace("\r\n", "\n").replace('\r', "\n");

    for line in normalized.split('\n') {
        // Skip empty lines and comments
        let line = js_trim(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse the line into key and value
        if let Some((key, value)) = parse_line(line)
            && !key.is_empty()
        {
            result.set(key, value);
        }
    }

    result
}

fn parse_line(line: &str) -> Option<(String, String)> {
    // Handle export prefix
    let line = match line.strip_prefix("export ") {
        Some(rest) => js_trim(rest),
        None => line,
    };

    // Find the key-value separator
    let separator_index = find_index_outside_quotes(line, |c| c == '=' || c == ':')?;

    let key = js_trim(&line[..separator_index]).to_string();
    let mut value = js_trim(&line[separator_index + 1..]);

    // Handle comments and remove them
    if let Some(comment_index) = find_index_outside_quotes(value, |c| c == '#') {
        value = js_trim(&value[..comment_index]);
    }

    // Process quoted values
    let mut value = value.to_string();
    let chars: Vec<char> = value.chars().collect();
    if let (Some(&first), Some(&last)) = (chars.first(), chars.last())
        && chars.len() >= 2
        && first == last
        && matches!(first, '"' | '\'' | '`')
    {
        // Remove surrounding quotes
        value = chars[1..chars.len() - 1].iter().collect();

        // Handle escaped characters in double quotes
        if first == '"' {
            value = value.replace("\\n", "\n").replace("\\r", "\r");
        }
    }

    Some((key, value))
}

/// O índice (em bytes) do primeiro caractere fora de aspas que satisfaz `predicate`.
fn find_index_outside_quotes(text: &str, predicate: impl Fn(char) -> bool) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut previous: Option<char> = None;

    for (i, c) in text.char_indices() {
        match quote {
            Some(open) => {
                if c == open && previous != Some('\\') {
                    quote = None;
                }
            }
            None if matches!(c, '"' | '\'' | '`') => quote = Some(c),
            None if predicate(c) => return Some(i),
            None => {}
        }
        previous = Some(c);
    }

    None
}
