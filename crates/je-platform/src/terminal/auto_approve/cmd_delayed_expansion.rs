//! Porte de `src/vs/platform/terminal/common/autoApprove/cmdDelayedExpansion.ts`.
//!
//! Divergências: o original é `/![^!\r\n]+!/.test(value)`. Aqui é uma varredura equivalente, sem o motor de
//! expressões regulares: a classe `[^!\r\n]` exclui o `!`, então o `+` nunca atravessa um `!` e basta contar,
//! a partir de cada `!` de abertura, os caracteres até o próximo `!` (um `!` que fecha também abre o
//! seguinte, como o casamento recomeçando nele). Nenhum desses caracteres é substituto UTF-16, então
//! percorrer `char` dá o mesmo resultado que percorrer unidades. O `TODO` do original (consolidar com os
//! helpers de `runInTerminalHelpers`) é do upstream.

/// Returns whether the text contains balanced CMD delayed environment-variable
/// expansion syntax, such as `!APPDATA!` or `!VAR:~0,1!`.
pub fn contains_cmd_delayed_expansion(value: &str) -> bool {
    // `Some(n)`: há um `!` aberto e `n` caracteres de `[^!\r\n]` depois dele.
    let mut open: Option<usize> = None;
    for c in value.chars() {
        match c {
            '!' => {
                if open.is_some_and(|count| count >= 1) {
                    return true;
                }
                open = Some(0);
            }
            '\r' | '\n' => open = None,
            _ => open = open.map(|count| count + 1),
        }
    }
    false
}
