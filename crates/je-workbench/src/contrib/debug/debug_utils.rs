//! Porte parcial de `src/vs/workbench/contrib/debug/common/debugUtils.ts`: só o `isUriString`, que `debugSource` usa.

/// `isUriString`: heuristics: a valid uri starts with a scheme and the scheme has at least 2 characters so that it
/// doesn't look like a drive letter (`/^[a-zA-Z][a-zA-Z0-9\+\-\.]+:/`). O `undefined` do original é a ausência do texto,
/// que o chamador trata antes (o texto vazio não casa).
pub fn is_uri_string(s: &str) -> bool {
    let mut chars = s.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let mut scheme_rest = 0;
    for c in chars {
        if c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.') {
            scheme_rest += 1;
        } else {
            return c == ':' && scheme_rest >= 1;
        }
    }
    false
}
