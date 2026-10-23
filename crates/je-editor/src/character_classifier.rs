//! Porte de `src/vs/editor/common/core/characterClassifier.ts`.
//!
//! O original é genérico sobre um tipo numérico (`CharacterClassifier<T extends number>`) e guarda
//! `Uint8Array`, então o valor é sempre um byte: aqui o classificador guarda `u8` e quem o usa com um
//! `enum` (`WordCharacterClass`) converte na borda.

use std::collections::HashMap;

/// Classificador rápido: um mapa compacto para os caracteres abaixo de 256 e um mapa esparso para o resto.
#[derive(Clone, Debug)]
pub struct CharacterClassifier {
    ascii_map: [u8; 256],
    map: HashMap<i32, u8>,
    default_value: u8,
}

impl CharacterClassifier {
    pub fn new(default_value: u8) -> Self {
        Self {
            ascii_map: [default_value; 256],
            map: HashMap::new(),
            default_value,
        }
    }

    pub fn set(&mut self, char_code: i32, value: u8) {
        if (0..256).contains(&char_code) {
            self.ascii_map[char_code as usize] = value;
        } else {
            self.map.insert(char_code, value);
        }
    }

    pub fn get(&self, char_code: i32) -> u8 {
        if (0..256).contains(&char_code) {
            self.ascii_map[char_code as usize]
        } else {
            // `map.get(c) || default`: um zero guardado também cai no padrão, e dá no mesmo.
            match self.map.get(&char_code) {
                Some(&value) if value != 0 => value,
                _ => self.default_value,
            }
        }
    }

    pub fn clear(&mut self) {
        self.ascii_map = [self.default_value; 256];
        self.map.clear();
    }
}

/// `CharacterSet`.
#[derive(Clone, Debug)]
pub struct CharacterSet {
    actual: CharacterClassifier,
}

impl Default for CharacterSet {
    fn default() -> Self {
        Self::new()
    }
}

impl CharacterSet {
    pub fn new() -> Self {
        Self {
            actual: CharacterClassifier::new(0),
        }
    }

    pub fn add(&mut self, char_code: i32) {
        self.actual.set(char_code, 1);
    }

    pub fn has(&self, char_code: i32) -> bool {
        self.actual.get(char_code) == 1
    }

    pub fn clear(&mut self) {
        self.actual.clear();
    }
}
