//! Porte de `src/vs/platform/terminal/common/autoApprove/sedFileWriteParser.ts`.
//!
//! Divergências:
//! - O texto do comando é `&str` e não [`je_base::JsString`]: o original só olha para caracteres ASCII
//!   (aspas, `-`, `i`, `e`, `f`) e para o `\s`, e nenhum deles é substituto UTF-16, então percorrer `char`
//!   dá o mesmo resultado que percorrer unidades. O que mede em unidades (`unquoted.length <= 10`) conta
//!   com `encode_utf16`. Os índices em bytes (`flags.indexOf('i') < flags.length - 1`) só comparam "há algo
//!   depois do caractere ASCII", que é o mesmo em bytes e em unidades.
//! - O `\s` do original (e o `/\s/.test(char)`) é [`is_js_whitespace`] (que difere de `char::is_whitespace`).
//! - A regex de `canHandle` roda no mesmo motor de expressões regulares do JavaScript (`JsRegExp`), com o
//!   padrão do original, para preservar o retrocesso.
//! - `readonly commandName = 'sed'` é o campo público `command_name`.
//! - `x.slice(1, -1)` de um texto entre aspas é [`strip_quotes`], que devolve `None` quando o texto não
//!   começa e termina com a mesma aspa (um texto de uma aspa só dá `""`, como o `slice` do JavaScript).

use je_base::js_regexp::JsRegExp;
use je_base::js_string::is_js_whitespace;

/// `inPlaceRegex` de `canHandle`.
const IN_PLACE_PATTERN: &str =
    r#"(?:^|\s)(-[a-zA-Z]*[iI][a-zA-Z]*\S*|--in-place(?:=\S*)?|(-i|-I)\s*'[^']*'|(-i|-I)\s*"[^"]*")(?:\s|$)"#;

thread_local! {
    static IN_PLACE_REGEX: JsRegExp = JsRegExp::from_str(IN_PLACE_PATTERN, "").expect("o padrão fixo do sed -i é válido");
}

/// `/\s/.test(char)` para um caractere.
fn is_space(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(is_js_whitespace)
}

/// O conteúdo de um texto que começa e termina com a mesma aspa (`'` ou `"`), sem elas: `slice(1, -1)`.
fn strip_quotes(text: &str) -> Option<&str> {
    ['\'', '"'].into_iter().find(|&quote| text.starts_with(quote) && text.ends_with(quote)).map(|_| {
        if text.len() < 2 { "" } else { &text[1..text.len() - 1] }
    })
}

/// Parser for detecting file writes from `sed` commands using in-place editing.
///
/// Handles:
/// - `sed -i 's/foo/bar/' file.txt` (GNU)
/// - `sed -i.bak 's/foo/bar/' file.txt` (GNU with backup suffix)
/// - `sed -i '' 's/foo/bar/' file.txt` (macOS/BSD with empty backup suffix)
/// - `sed --in-place 's/foo/bar/' file.txt` (GNU long form)
/// - `sed --in-place=.bak 's/foo/bar/' file.txt` (GNU long form with backup)
/// - `sed -I 's/foo/bar/' file.txt` (BSD case-insensitive variant)
#[derive(Clone, Debug)]
pub struct SedFileWriteParser {
    pub command_name: &'static str,
}

impl Default for SedFileWriteParser {
    fn default() -> Self {
        Self { command_name: "sed" }
    }
}

impl SedFileWriteParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn can_handle(&self, command_text: &str) -> bool {
        // Check if this is a sed command
        if !command_text.strip_prefix("sed").and_then(|rest| rest.chars().next()).is_some_and(is_space) {
            return false;
        }

        // Check for -i, -I, or --in-place flag
        let units: Vec<u16> = command_text.encode_utf16().collect();
        IN_PLACE_REGEX.with(|regex| regex.exec(&units).is_some())
    }

    pub fn extract_file_writes(&self, command_text: &str) -> Vec<String> {
        let tokens = tokenize_command(command_text);
        extract_file_targets(&tokens)
    }
}

/// Tokenizes a command into individual arguments, handling quotes and escapes.
fn tokenize_command(command_text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;

    for c in command_text.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }

        if c == '\\' && !in_single_quote {
            escaped = true;
            current.push(c);
            continue;
        }

        if c == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            current.push(c);
            continue;
        }

        if c == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            current.push(c);
            continue;
        }

        if is_space(c) && !in_single_quote && !in_double_quote {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }

        current.push(c);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Extracts file targets from tokenized sed command arguments.
/// Files are generally the last non-option, non-script arguments.
fn extract_file_targets(tokens: &[String]) -> Vec<String> {
    if tokens.first().map(String::as_str) != Some("sed") {
        return Vec::new();
    }

    let mut files: Vec<String> = Vec::new();
    let mut i = 1; // Skip 'sed'
    let mut found_script = false;

    while i < tokens.len() {
        let token = tokens[i].as_str();

        // Long options
        if token.starts_with("--") {
            if token == "--in-place" || token.starts_with("--in-place=") {
                // In-place flag (already verified we have one)
                i += 1;
                continue;
            }
            if token == "--expression" || token == "--file" {
                // Skip the option and its argument
                i += 2;
                found_script = true;
                continue;
            }
            if token.starts_with("--expression=") || token.starts_with("--file=") {
                i += 1;
                found_script = true;
                continue;
            }
            // Other long options like --sandbox, --debug, etc.
            i += 1;
            continue;
        }

        // Short options
        if token.starts_with('-') && token.chars().nth(1).is_some_and(|c| c != '-') {
            // Could be combined flags like -ni or -i.bak
            let flags = &token[1..];

            // Check if this is -i with backup suffix attached (e.g., -i.bak)
            let in_place_index = flags.find('i').or_else(|| flags.find('I'));

            if in_place_index.is_some_and(|index| index + 1 < flags.len()) {
                // -i.bak style - backup suffix is attached
                i += 1;
                continue;
            }

            // Check if -i or -I is the last flag and next token could be backup suffix
            if (flags.ends_with('i') || flags.ends_with('I')) && i + 1 < tokens.len() {
                let next_token = tokens[i + 1].as_str();
                // macOS/BSD style: -i '' or -i "" (empty string backup suffix)
                // Only treat it as a backup suffix if it's empty or looks like a backup
                // extension (starts with '.' and is short). Don't match sed scripts like 's/foo/bar/'.
                if next_token == "''" || next_token == "\"\"" {
                    i += 2;
                    continue;
                }
                // Check for quoted backup suffixes like '.bak' or ".backup"
                if let Some(unquoted) = strip_quotes(next_token) {
                    // Backup suffixes typically start with '.' and are short extensions
                    if unquoted.starts_with('.') && unquoted.encode_utf16().count() <= 10 && !unquoted.contains('/') {
                        i += 2;
                        continue;
                    }
                }
            }

            // Check for -e or -f which take arguments
            if flags.contains('e') || flags.contains('f') {
                let option_index = flags.find('e').or_else(|| flags.find('f'));

                // If -e or -f is not the last character, the rest of the token is the argument
                if option_index.is_some_and(|index| index + 1 < flags.len()) {
                    found_script = true;
                    i += 1;
                    continue;
                }

                // Otherwise, the next token is the argument
                found_script = true;
                i += 2;
                continue;
            }

            i += 1;
            continue;
        }

        // Non-option argument
        if !found_script {
            // First non-option is the script (unless -e/-f was used)
            found_script = true;
            i += 1;
            continue;
        }

        // Subsequent non-option arguments are files
        // Strip surrounding quotes from file path
        files.push(strip_quotes(token).unwrap_or(token).to_owned());
        i += 1;
    }

    files
}
