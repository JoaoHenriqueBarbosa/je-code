//! Porte de `src/vs/editor/browser/config/tabFocus.ts`: o que a tecla Tab faz (`false`: o editor trata Tab e
//! Shift-Tab; `true`: Tab e Shift-Tab movem o foco da interface), com o evento de mudança.
//!
//! O original exporta uma instância única (`export const TabFocus = new TabFocusImpl()`). Aqui ela vive num
//! `thread_local!`, como o `EditorZoom` e o `InputMode`, e o acesso é pelas funções [`get_tab_focus_mode`],
//! [`set_tab_focus_mode`] e [`on_did_change_tab_focus`]. O valor inicial é `false`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};

/// `TabFocusImpl`.
pub struct TabFocusImpl {
    tab_focus: Cell<bool>,
    on_did_change_tab_focus: Rc<Emitter<bool>>,
}

impl TabFocusImpl {
    pub fn new() -> Self {
        Self { tab_focus: Cell::new(false), on_did_change_tab_focus: Emitter::new() }
    }

    /// `onDidChangeTabFocus`.
    pub fn on_did_change_tab_focus(&self) -> Event<bool> {
        self.on_did_change_tab_focus.event()
    }

    pub fn get_tab_focus_mode(&self) -> bool {
        self.tab_focus.get()
    }

    /// Dispara sempre, mesmo quando o valor não mudou, como o original.
    pub fn set_tab_focus_mode(&self, tab_focus_mode: bool) {
        self.tab_focus.set(tab_focus_mode);
        self.on_did_change_tab_focus.fire(tab_focus_mode);
    }
}

impl Default for TabFocusImpl {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static TAB_FOCUS: TabFocusImpl = TabFocusImpl::new();
}

/// `TabFocus.getTabFocusMode()`.
pub fn get_tab_focus_mode() -> bool {
    TAB_FOCUS.with(TabFocusImpl::get_tab_focus_mode)
}

/// `TabFocus.setTabFocusMode(tabFocusMode)`.
pub fn set_tab_focus_mode(tab_focus_mode: bool) {
    TAB_FOCUS.with(|singleton| singleton.set_tab_focus_mode(tab_focus_mode));
}

/// `TabFocus.onDidChangeTabFocus`.
pub fn on_did_change_tab_focus() -> Event<bool> {
    TAB_FOCUS.with(TabFocusImpl::on_did_change_tab_focus)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn starts_false_and_fires_on_every_set() {
        assert!(!get_tab_focus_mode());
        let seen: Rc<RefCell<Vec<bool>>> = Rc::default();
        let subscription = on_did_change_tab_focus().listen({
            let seen = seen.clone();
            move |mode| seen.borrow_mut().push(*mode)
        });
        set_tab_focus_mode(true);
        set_tab_focus_mode(true);
        set_tab_focus_mode(false);
        assert_eq!(*seen.borrow(), vec![true, true, false]);
        assert!(!get_tab_focus_mode());
        subscription.dispose();
    }
}
