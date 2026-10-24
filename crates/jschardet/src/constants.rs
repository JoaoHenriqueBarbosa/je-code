//! Porte de `src/constants.js`.

/// Estado de um prober: ainda sem conclusão (`detecting`).
pub const DETECTING: u8 = 0;
/// Estado de um prober: encontrou a codificação (`foundIt`).
pub const FOUND_IT: u8 = 1;
/// Estado de um prober: não é esta codificação (`notMe`).
pub const NOT_ME: u8 = 2;

/// Estado de uma máquina de estados de codificação: início de caractere (`start`).
pub const START: u8 = 0;
/// Estado de uma máquina de estados de codificação: sequência inválida (`error`).
pub const ERROR: u8 = 1;
/// Estado de uma máquina de estados de codificação: só esta codificação admite a sequência (`itsMe`).
pub const ITS_ME: u8 = 2;

pub const SHORTCUT_THRESHOLD: f64 = 0.95;
