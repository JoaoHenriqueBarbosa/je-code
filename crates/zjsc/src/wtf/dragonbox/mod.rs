//! Porte de `WTF/wtf/dragonbox` (Dragonbox de Junekey Jeon, com a camada de impressão da WebKit).
//!
//! O Dragonbox é um template com política para cada decisão (sinal, zeros à direita, arredondamento
//! decimal para binário e binário para decimal, cache). A WTF só instancia uma configuração, a que
//! `dragonbox::detail::to_chars_n` monta: `sign::ignore`, `trailing_zero::ignore`,
//! `decimal_to_binary_rounding::nearest_to_even`, `binary_to_decimal_rounding::to_even` e
//! `cache::full`. O porte traz exatamente essa instanciação, para `f32` (`ieee754_binary32`) e `f64`
//! (`ieee754_binary64`); as demais políticas (`left_closed_directed`, `right_closed_directed`,
//! `cache::compact`, `trailing_zero::remove` e `report`) nunca são instanciadas pela WebKit e não
//! têm comportamento observável, então não se portam.
pub mod detail;
pub mod dragonbox;
pub mod dragonbox_to_chars;
pub mod ieee754_format;
