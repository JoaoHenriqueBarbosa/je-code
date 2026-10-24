//! Tradução de `tools/FunctionOverrides.h` e `FunctionOverrides.cpp`.
//!
//! Vale a opção `functionOverrides` (`Options::functionOverrides()`), que só existe com as opções
//! restritas ligadas.
//!
//! DIVERGÊNCIAS:
//!
//! - `FunctionOverridesAssertScope` (`RELEASE_ASSERT(g_jscConfig.restrictedOptionsEnabled)`) é a
//!   configuração congelada do processo e não existe aqui; a opção `functionOverrides` só vem preenchida
//!   por quem liga as opções restritas.
//! - O singleton (`LazyNeverDestroyed` com `std::call_once`) é um `thread_local` (`String` da WTF é `Rc`,
//!   não atravessa threads); o `Lock m_lock` some junto.
//! - `fgets`/`FILE*` são o `BufRead` do Rust com a mesma regra do `fgets` (para no `\n` ou em
//!   `BUFSIZ - 1` bytes); `strstr`/`strchr` são buscas de bytes. O texto é lido como Latin1, como o
//!   `String(span<const char>)` do C++.
//! - `dataLog` é o stderr do `Host`, `exitProcess(EXIT_FAILURE)` é `Host::exit(1)`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};

use crate::parser::source_code::SourceCode;
use crate::runtime::host::host;
use crate::parser::source_provider::{SourceProviderSourceType, StringSourceProvider};
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::options_list::Options;
use crate::runtime::source_origin::SourceOrigin;
use crate::wtf::text::string_common::NOT_FOUND;
use crate::wtf::text::text_position::TextPosition;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

/// `BUFSIZ` do glibc.
const BUFSIZ: usize = 8192;

/// `struct FunctionOverrideInfo`.
#[derive(Clone, Default)]
pub struct FunctionOverrideInfo {
    pub source_code: SourceCode,
    pub first_line: u32,
    pub line_count: u32,
    pub start_column: u32,
    pub end_column: u32,
    pub parameters_start_offset: u32,
    pub function_start: u32,
    pub function_end: u32,
}

/// `class FunctionOverrides`.
pub struct FunctionOverrides {
    entries: HashMap<WtfString, WtfString>,
}

thread_local! {
    static OVERRIDES: RefCell<Option<FunctionOverrides>> = const { RefCell::new(None) };
}

/*
  The overrides file defines function bodies that we will want to override with
  a replacement for debugging purposes. The overrides file may contain
  'override' and 'with' clauses like these:

     // Example 1: function foo1(a)
     override !@#$%{ print("In foo1"); }!@#$%
     with abc{
         print("I am overridden");
     }abc

     // Example 2: function foo2(a)
     override %%%{
         print("foo2's body has a string with }%% in it.");
         // Because }%% appears in the function body here, we cannot use
         // %% or % as the delimiter. %%% is ok though.
     }%%%
     with %%%{
         print("Overridden foo2");
     }%%%

  1. Comments are lines starting with //.  All comments will be ignored.

  2. An 'override' clause is used to specify the original function body we
     want to override. The with clause is used to specify the overriding
     function body.

     An 'override' clause must be followed immediately by a 'with' clause.

  3. An 'override' clause must be of the form:
         override <delimiter>{...function body...}<delimiter>

     The override keyword must be at the start of the line.

     <delimiter> may be any string of any ASCII characters (except for '{',
     '}', and whitespace characters) as long as the pattern of "}<delimiter>"
     does not appear in the function body e.g. the override clause of Example 2
     above illustrates this.

     The start and end <delimiter> must be identical.

     The space between the override keyword and the start <delimiter> is
     required.

     All characters between the pair of delimiters will be considered to
     be part of the function body string. This allows us to also work
     with script source that are multi-lined i.e. newlines are allowed.

  4. A 'with' clause is identical in form to an 'override' clause except that
     it uses the 'with' keyword instead of the 'override' keyword.
 */

/// `FAIL_WITH_ERROR(error, errorMessageInBrackets)`.
fn fail_with_error(error: &str, message: &str) -> ! {
    host().write_stderr(format!("functionOverrides {}: {}", error, message).as_bytes());
    host().exit(1)
}

const SYNTAX_ERROR: &str = "SYNTAX ERROR";
const IO_ERROR: &str = "IO ERROR";

/// `fgets(buffer, bufferSize, file)`: lê até o `\n` (incluído) ou `bufferSize - 1` bytes.
fn fgets(file: &mut impl BufRead, buffer_size: usize) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        let available = match file.fill_buf() {
            Ok(available) => available,
            Err(_) => break,
        };
        if available.is_empty() {
            break;
        }
        let room = buffer_size - 1 - line.len();
        let limit = available.len().min(room);
        match available[..limit].iter().position(|&byte| byte == b'\n') {
            Some(newline) => {
                line.extend_from_slice(&available[..=newline]);
                file.consume(newline + 1);
                return Some(line);
            }
            None => {
                line.extend_from_slice(&available[..limit]);
                file.consume(limit);
                if line.len() == buffer_size - 1 {
                    return Some(line);
                }
            }
        }
    }
    if line.is_empty() { None } else { Some(line) }
}

/// `strstr(haystack, needle)`.
fn strstr(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// `hasDisallowedCharacters(const char*, size_t)`.
fn has_disallowed_characters(string: &[u8]) -> bool {
    // '{' is also disallowed, but we don't need to check for it because
    // parseClause() searches for '{' as the end of the start delimiter.
    // As a result, the parsed delimiter string will never include '{'.
    string.iter().any(|&c| c == b'}' || matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r'))
}

/// `parseClause(const char* keyword, size_t keywordLength, FILE*, const char* line, char* buffer, size_t bufferSize)`.
fn parse_clause(keyword: &str, file: &mut impl BufRead, first_line: &[u8]) -> WtfString {
    let keyword_bytes = keyword.as_bytes();
    let keyword_length = keyword_bytes.len();
    let line_text = || String::from_utf8_lossy(first_line).into_owned();

    let Some(keyword_pos) = strstr(first_line, keyword_bytes) else {
        fail_with_error(SYNTAX_ERROR, &format!("Expecting '{}' clause:\n{}\n", keyword, line_text()));
    };
    if keyword_pos != 0 {
        fail_with_error(SYNTAX_ERROR, &format!("Cannot have any characters before '{}':\n{}\n", keyword, line_text()));
    }
    if first_line.get(keyword_length) != Some(&b' ') {
        fail_with_error(SYNTAX_ERROR, &format!("'{}' must be followed by a ' ':\n{}\n", keyword, line_text()));
    }

    let delimiter_start = keyword_length + 1;
    let Some(delimiter_offset) = first_line[delimiter_start..].iter().position(|&c| c == b'{') else {
        fail_with_error(
            SYNTAX_ERROR,
            &format!("Missing {{ after '{}' clause start delimiter:\n{}\n", keyword, line_text()),
        );
    };
    let delimiter_end = delimiter_start + delimiter_offset;

    let delimiter = &first_line[delimiter_start..delimiter_end];
    let delimiter_text = String::from_utf8_lossy(delimiter).into_owned();

    if has_disallowed_characters(delimiter) {
        fail_with_error(
            SYNTAX_ERROR,
            &format!("Delimiter '{}' cannot have '{{', '}}', or whitespace:\n{}\n", delimiter_text, line_text()),
        );
    }

    let mut terminator = vec![b'}'];
    terminator.extend_from_slice(delimiter);

    // Start from the {.
    let mut line = first_line[delimiter_end..].to_vec();
    let mut builder: Vec<u8> = Vec::new();
    loop {
        if let Some(p) = strstr(&line, &terminator) {
            if line.get(p + terminator.len()) != Some(&b'\n') {
                fail_with_error(
                    SYNTAX_ERROR,
                    &format!(
                        "Unexpected characters after '{}' clause end delimiter '{}':\n{}\n",
                        keyword,
                        delimiter_text,
                        String::from_utf8_lossy(&line)
                    ),
                );
            }

            builder.extend_from_slice(&line[..=p]);
            return WtfString::from_latin1(&builder);
        }
        builder.extend_from_slice(&line);

        match fgets(file, BUFSIZ) {
            Some(next) => line = next,
            None => break,
        }
    }

    fail_with_error(
        SYNTAX_ERROR,
        &format!(
            "'{}' clause end delimiter '{}' not found:\n{}\n\nAre you missing a '}}' before the delimiter?\n",
            keyword,
            delimiter_text,
            String::from_utf8_lossy(&builder)
        ),
    );
}

impl FunctionOverrides {
    /// `FunctionOverrides(const char* functionOverridesFileName)`.
    fn new(overrides_file_name: Option<&str>) -> FunctionOverrides {
        let mut overrides = FunctionOverrides { entries: HashMap::new() };
        overrides.parse_overrides_in_file(overrides_file_name);
        overrides
    }

    /// `overrides()`: o singleton, construído na primeira chamada.
    fn with_overrides<R>(body: impl FnOnce(&mut FunctionOverrides) -> R) -> R {
        OVERRIDES.with(|slot| {
            let mut slot = slot.borrow_mut();
            let overrides = slot.get_or_insert_with(|| {
                let overrides_file_name = Options::function_overrides();
                FunctionOverrides::new(overrides_file_name.as_deref())
            });
            body(overrides)
        })
    }

    /// `reinstallOverrides()`.
    pub fn reinstall_overrides() {
        FunctionOverrides::with_overrides(|overrides| {
            let overrides_file_name = Options::function_overrides();
            overrides.entries.clear();
            overrides.parse_overrides_in_file(overrides_file_name.as_deref());
        });
    }

    /// `initializeOverrideFor(const SourceCode& origCode, OverrideInfo& result)`.
    pub fn initialize_override_for(orig_code: &SourceCode, result: &mut FunctionOverrideInfo) -> bool {
        assert!(Options::function_overrides().is_some());

        let source_string = orig_code.view();
        let source_body_start = source_string.find_character(u16::from(b'{'), 0);
        if source_body_start == NOT_FOUND {
            return false;
        }
        let source_body_string = source_string.substring(source_body_start as u32, source_string.length() - source_body_start as u32);

        let new_body = FunctionOverrides::with_overrides(|overrides| overrides.entries.get(&source_body_string).cloned());
        let Some(new_body) = new_body else {
            return false;
        };

        initialize_override_info(orig_code, &new_body, result);
        assert!(Options::function_overrides().is_some());
        true
    }

    /// `parseOverridesInFile(const char* fileName)`.
    fn parse_overrides_in_file(&mut self, file_name: Option<&str>) {
        let Some(file_name) = file_name else {
            return;
        };

        let file = match host().read_file(file_name) {
            Ok(file) => file,
            Err(_) => fail_with_error(
                IO_ERROR,
                &format!(
                    "Failed to open file {}. Did you add the file-read-data entitlement to WebProcess.sb?\n",
                    file_name
                ),
            ),
        };
        let mut file = BufReader::new(file.as_slice());

        while let Some(mut line) = fgets(&mut file, BUFSIZ) {
            if line.starts_with(b"//") {
                continue;
            }

            if line[0] == b'\n' || line[0] == 0 {
                continue;
            }

            let key_str = parse_clause("override", &mut file, &line);

            line = match fgets(&mut file, BUFSIZ) {
                Some(line) => line,
                None => fail_with_error(SYNTAX_ERROR, "Expecting 'with' clause:\n\n"),
            };

            let value_str = parse_clause("with", &mut file, &line);

            self.entries.entry(key_str).or_insert(value_str);
        }
    }
}

/// `initializeOverrideInfo(const SourceCode& origCode, const String& newBody, OverrideInfo& info)`.
fn initialize_override_info(orig_code: &SourceCode, new_body: &WtfString, info: &mut FunctionOverrideInfo) {
    let orig_provider_str = orig_code.provider().expect("SourceCode sem SourceProvider").source();
    let orig_start = orig_code.start_offset() as u32;
    let orig_function_start = orig_provider_str.reverse_find(&WtfString::from_latin1(b"function"), orig_start);
    let orig_brace_start = orig_provider_str.find_character(u16::from(b'{'), orig_start);
    let header_length = orig_brace_start.wrapping_sub(orig_function_start);
    let orig_header = orig_provider_str.substring(orig_function_start as u32, header_length as u32);

    let new_provider_string = crate::wtf::text::string_concatenate::make_string_dyn(&[&orig_header, new_body]);

    let overridden = WtfString::from_latin1(b"<overridden>");
    let url = URL::from_string(&overridden);
    let new_provider = StringSourceProvider::create(
        &new_provider_string,
        &SourceOrigin::new(url),
        overridden,
        SourceTaintedOrigin::Untainted,
        TextPosition::default(),
        SourceProviderSourceType::Program,
    );

    info.first_line = 1;
    info.line_count = 1; // Faking it. This doesn't really matter for now.
    info.start_column = 1;
    info.end_column = 1; // Faking it. This doesn't really matter for now.
    info.parameters_start_offset = new_provider_string.find_character(u16::from(b'('), 0) as u32;
    info.function_start = 0;
    info.function_end = new_provider_string.length() - 1;

    info.source_code = SourceCode::with_offsets(
        Some(new_provider),
        info.parameters_start_offset as i32,
        (info.function_end + 1) as i32,
        1,
        1,
    );
}
