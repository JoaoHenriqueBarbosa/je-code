//! Porte de `src/vs/platform/terminal/common/capabilities/promptTypeDetectionCapability.ts`: a capacidade que
//! guarda o tipo de prompt em uso (por exemplo p10k, posh-git), informado pela integração com o shell.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use super::capabilities::{IPromptTypeDetectionCapability, TerminalCapability};

/// `PromptTypeDetectionCapability`.
pub struct PromptTypeDetectionCapability {
    base: Rc<Disposable>,
    prompt_type: RefCell<Option<String>>,
    on_prompt_type_changed: Rc<Emitter<Option<String>>>,
}

impl PromptTypeDetectionCapability {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_prompt_type_changed: Rc<Emitter<Option<String>>> = Emitter::new();
        base.register(on_prompt_type_changed.clone());
        Rc::new(Self { base, prompt_type: RefCell::new(None), on_prompt_type_changed })
    }
}

impl IDisposable for PromptTypeDetectionCapability {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IPromptTypeDetectionCapability for PromptTypeDetectionCapability {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::PromptTypeDetection
    }

    fn prompt_type(&self) -> Option<String> {
        self.prompt_type.borrow().clone()
    }

    fn on_prompt_type_changed(&self) -> Event<Option<String>> {
        self.on_prompt_type_changed.event()
    }

    fn set_prompt_type(&self, value: &str) {
        *self.prompt_type.borrow_mut() = Some(value.to_owned());
        self.on_prompt_type_changed.fire(Some(value.to_owned()));
    }
}
