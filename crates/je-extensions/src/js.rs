//! Ponte entre `&str` e as funções de `je_base::strings`, que trabalham em UTF-16 como o JavaScript.

use std::cmp::Ordering;

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `strings.equalsIgnoreCase`.
pub fn equals_ignore_case(a: &str, b: &str) -> bool {
    je_base::strings::equals_ignore_case(&utf16(a), &utf16(b))
}

/// `strings.compareIgnoreCase`.
pub fn compare_ignore_case(a: &str, b: &str) -> Ordering {
    je_base::strings::compare_ignore_case(&utf16(a), &utf16(b)).cmp(&0)
}
