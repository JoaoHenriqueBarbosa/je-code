//! Porte de `src/vs/editor/common/inputMode.ts`: o modo de digitação do editor, inserção ou
//! sobrescrita (`'insert' | 'overtype'`), com o evento de mudança.
//!
//! O original exporta uma instância única (`export const InputMode = new InputModeImpl()`). Aqui ela vive
//! num `thread_local!`, como os demais singletons do crate (as opções e os emissores carregam `Rc`, que não
//! é `Sync`), e o acesso é pelas funções [`get_input_mode`], [`set_input_mode`] e
//! [`on_did_change_input_mode`]. O valor é o [`InputMode`] de `config/editor_options.rs`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};

use crate::config::editor_options::InputMode;

/// `InputModeImpl`.
pub struct InputModeImpl {
    input_mode: Cell<InputMode>,
    on_did_change_input_mode: Rc<Emitter<InputMode>>,
}

impl InputModeImpl {
    pub fn new() -> Self {
        Self {
            input_mode: Cell::new(InputMode::Insert),
            on_did_change_input_mode: Emitter::new(),
        }
    }

    /// `onDidChangeInputMode`.
    pub fn on_did_change_input_mode(&self) -> Event<InputMode> {
        self.on_did_change_input_mode.event()
    }

    pub fn get_input_mode(&self) -> InputMode {
        self.input_mode.get()
    }

    pub fn set_input_mode(&self, input_mode: InputMode) {
        self.input_mode.set(input_mode);
        self.on_did_change_input_mode.fire(input_mode);
    }
}

impl Default for InputModeImpl {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static INPUT_MODE: InputModeImpl = InputModeImpl::new();
}

/// `InputMode.getInputMode()`.
pub fn get_input_mode() -> InputMode {
    INPUT_MODE.with(InputModeImpl::get_input_mode)
}

/// `InputMode.setInputMode(inputMode)`.
pub fn set_input_mode(input_mode: InputMode) {
    INPUT_MODE.with(|singleton| singleton.set_input_mode(input_mode));
}

/// `InputMode.onDidChangeInputMode`.
pub fn on_did_change_input_mode() -> Event<InputMode> {
    INPUT_MODE.with(InputModeImpl::on_did_change_input_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_in_insert_and_follows_set() {
        assert_eq!(get_input_mode(), InputMode::Insert);
        set_input_mode(InputMode::Overtype);
        assert_eq!(get_input_mode(), InputMode::Overtype);
        set_input_mode(InputMode::Insert);
        assert_eq!(get_input_mode(), InputMode::Insert);
    }
}
