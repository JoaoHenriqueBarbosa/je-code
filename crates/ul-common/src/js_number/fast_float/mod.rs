//! Porte de `WTF/wtf/fast_float/fast_float.h` (fast_float de Lemire, usado por `wtf/FastFloat.cpp`).
//! Um submódulo por seção do amálgama.
pub mod float_common;
pub mod fast_float;
pub mod ascii_number;
pub mod fast_table;
pub mod decimal_to_binary;
pub mod bigint;
pub mod digit_comparison;
pub mod parse_number;
pub use parse_number::{parse_double, parse_fixed_double};
