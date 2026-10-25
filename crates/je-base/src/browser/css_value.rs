//! Porte de `asCssValueWithDefault` de `src/vs/base/browser/cssValue.ts`.

/// Acrescenta o valor padrão a uma referência `var(--nome)` que não o traz, recursivamente nos padrões que são
/// outras `var(...)`. Qualquer outro valor passa como veio; sem valor, vale o padrão.
pub fn as_css_value_with_default(css_property_value: Option<&str>, default: &str) -> String {
    let Some(value) = css_property_value else {
        return default.to_owned();
    };
    // `/^\s*var\((.+)\)$/`: o `.` não casa quebra de linha.
    let variable_arguments = value
        .trim_start()
        .strip_prefix("var(")
        .and_then(|rest| rest.strip_suffix(')'))
        .filter(|inner| !inner.is_empty() && !inner.contains(['\n', '\r', '\u{2028}', '\u{2029}']));
    let Some(inner) = variable_arguments else {
        return value.to_owned();
    };
    // `split(',', 2)`: no máximo as duas primeiras partes.
    let mut parts = inner.split(',');
    let name = parts.next().unwrap_or_default();
    let default = match parts.next() {
        Some(second) => as_css_value_with_default(Some(second.trim()), default),
        None => default.to_owned(),
    };
    format!("var({name}, {default})")
}
