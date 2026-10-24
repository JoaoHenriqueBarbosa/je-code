//! Porte da parte lógica de `src/vs/workbench/contrib/preferences/browser/settingsEditor2.ts`: `isSettingsSearchUpToDate`
//! (o `SettingsEditor2` em si é um `EditorPane` de DOM e não entra).

/// `isSettingsSearchUpToDate(searchPending, renderedSearchQuery, currentSearchValue)`: whether Settings search results are
/// safe to focus from the search input. `search_pending` must be the boolean from `Delayer::is_triggered`.
pub fn is_settings_search_up_to_date(search_pending: bool, rendered_search_query: Option<&str>, current_search_value: &str) -> bool {
    !search_pending && rendered_search_query == Some(js_trim(current_search_value))
}

/// `String.prototype.trim`: tira o espaço em branco do JavaScript (`WhiteSpace` e `LineTerminator`, o que o
/// `char::is_whitespace` cobre, mais o BOM `U+FEFF`).
fn js_trim(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}')
}
