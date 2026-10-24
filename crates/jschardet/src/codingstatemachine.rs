//! Porte de `src/codingstatemachine.js`.

use crate::constants::START;

/// O objeto-modelo de `src/mbcssm/*.js` e `src/escsm.js`.
pub struct SmModel {
    pub class_table: &'static [u8],
    pub class_factor: usize,
    pub state_table: &'static [u8],
    pub char_len_table: &'static [u8],
    pub name: &'static str,
}

pub struct CodingStateMachine {
    model: &'static SmModel,
    current_char_len: usize,
    current_state: u8,
}

impl CodingStateMachine {
    pub fn new(model: &'static SmModel) -> Self {
        Self { model, current_char_len: 0, current_state: START }
    }

    pub fn reset(&mut self) {
        self.current_state = START;
    }

    pub fn next_state(&mut self, c: u8) -> u8 {
        // for each byte we get its class
        // if it is first byte, we also get byte length
        let byte_cls = usize::from(self.model.class_table[usize::from(c)]);
        if self.current_state == START {
            self.current_char_len = usize::from(self.model.char_len_table[byte_cls]);
        }
        // from byte's class and stateTable, we get its next state
        self.current_state = self.model.state_table[usize::from(self.current_state) * self.model.class_factor + byte_cls];
        self.current_state
    }

    pub fn get_current_char_len(&self) -> usize {
        self.current_char_len
    }

    pub fn get_coding_state_machine(&self) -> &'static str {
        self.model.name
    }
}
