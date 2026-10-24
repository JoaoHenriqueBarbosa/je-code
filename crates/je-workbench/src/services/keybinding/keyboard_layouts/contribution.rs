//! Porte de `src/vs/workbench/services/keybinding/browser/keyboardLayouts/_.contribution.ts`: o registro dos layouts de
//! teclado que os arquivos de layout alimentam e o serviço lê.
//!
//! O `static readonly INSTANCE` (um singleton do módulo) é um `thread_local`, acessado por
//! [`KeyboardLayoutContribution::with_instance`]: o porte não tem estado global compartilhado entre threads, e cada teste
//! roda na sua própria thread, como o módulo ES carregado de novo a cada processo de teste.

use std::cell::RefCell;

use crate::services::keybinding::keymap_info::KeymapInfoSource;

pub struct KeyboardLayoutContribution {
    layout_infos: RefCell<Vec<KeymapInfoSource>>,
}

thread_local! {
    static INSTANCE: KeyboardLayoutContribution = const { KeyboardLayoutContribution { layout_infos: RefCell::new(Vec::new()) } };
}

impl KeyboardLayoutContribution {
    /// `KeyboardLayoutContribution.INSTANCE`.
    pub fn with_instance<R>(f: impl FnOnce(&KeyboardLayoutContribution) -> R) -> R {
        INSTANCE.with(f)
    }

    /// `get layoutInfos()`: uma cópia da lista registrada até agora.
    pub fn layout_infos(&self) -> Vec<KeymapInfoSource> {
        self.layout_infos.borrow().clone()
    }

    pub fn register_keyboard_layout(&self, layout: KeymapInfoSource) {
        self.layout_infos.borrow_mut().push(layout);
    }
}
