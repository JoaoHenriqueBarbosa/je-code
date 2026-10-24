//! Porte de `src/vs/platform/telemetry/common/assignmentContext.ts`: a validação do contexto de atribuição de um experimento
//! antes de ele ser confiado a eventos de telemetria.
//!
//! Divergências: a expressão regular `ASSIGNMENT_CONTEXT_ENTRY_PATTERN` (`/^[^:;\s\x00-\x1F\x7F]+:[^;\x00-\x1F\x7F]+$/`) é a
//! conferência manual de [`is_valid_entry`], com o `\s` do JavaScript ([`is_js_whitespace`], que difere de
//! `char::is_whitespace` em U+0085 e U+FEFF); `value.length` conta unidades UTF-16.

/// `MAX_ASSIGNMENT_CONTEXT_LENGTH`.
const MAX_ASSIGNMENT_CONTEXT_LENGTH: usize = 8 * 1024;

/// O `\s` de uma expressão regular do JavaScript.
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `\x00-\x1F\x7F`.
fn is_control(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}')
}

/// `ASSIGNMENT_CONTEXT_ENTRY_PATTERN.test(entry)`.
fn is_valid_entry(entry: &str) -> bool {
    // A primeira classe exclui `:`, então o primeiro `:` é o separador.
    let Some((key, value)) = entry.split_once(':') else {
        return false;
    };
    !key.is_empty()
        && key.chars().all(|c| c != ';' && !is_js_whitespace(c) && !is_control(c))
        && !value.is_empty()
        && value.chars().all(|c| c != ';' && !is_control(c))
}

/// `isValidAssignmentContext(value)`: validates an experiment assignment context before it is trusted onto telemetry events.
pub fn is_valid_assignment_context(value: &str) -> bool {
    if value.is_empty() || value.encode_utf16().count() > MAX_ASSIGNMENT_CONTEXT_LENGTH {
        return false;
    }

    let entries: Vec<&str> = match value.strip_suffix(';') {
        Some(without_separator) => without_separator.split(';').collect(),
        None => value.split(';').collect(),
    };
    !entries.is_empty() && entries.into_iter().all(is_valid_entry)
}
