//! Porte de `src/vs/base/test/browser/ui/dropdown/dropdown.test.ts` (suíte `DropdownMenu`), caso a caso e na mesma ordem.
//!
//! O `IContextMenuProvider` do original é um objeto que guarda os delegados; aqui é [`RecordingProvider`]. Os
//! `delegate.getMenuClassName?.()` e `delegate.closeAnimation` viram as chamadas do trait
//! `IContextMenuDelegate`, e o `deepStrictEqual` compara as duas tuplas lidas de cada delegado.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_ui::{NodeId, Ui};
use je_widgets::context_view_api::{CONTEXT_VIEW_MENU_MOTION_CLASS, IContextMenuDelegate, IContextMenuProvider, IContextViewCloseAnimation, context_view_menu_close_animation};
use je_widgets::dropdown::{DropdownMenu, IDropdown, IDropdownMenuOptions};
use support::ensure_no_disposables_are_leaked_in_test_suite;

/// `{ showContextMenu: delegate => delegates.push(delegate) }`.
#[derive(Default)]
struct RecordingProvider {
    delegates: RefCell<Vec<Rc<dyn IContextMenuDelegate>>>,
}

impl IContextMenuProvider for RecordingProvider {
    fn show_context_menu(&self, delegate: Rc<dyn IContextMenuDelegate>) {
        self.delegates.borrow_mut().push(delegate);
    }
}

fn container(ui: &Ui) -> NodeId {
    ui.tree_mut().create_detached(je_ui::Role::Generic)
}

#[test]
fn applies_menu_motion_by_default_and_preserves_overrides() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let provider = Rc::new(RecordingProvider::default());
    let custom_close_animation = IContextViewCloseAnimation { class_name: "custom-closing".to_owned(), duration: 42.0, required_ancestor_classes: Vec::new() };

    let default_container = container(&ui);
    let mut options = IDropdownMenuOptions::new(provider.clone());
    options.actions = Some(Vec::new());
    options.menu_class_name = Some("custom-menu".to_owned());
    store.add_rc(DropdownMenu::new(&ui, default_container, options)).show();

    let overridden_container = container(&ui);
    let mut options = IDropdownMenuOptions::new(provider.clone());
    options.actions = Some(Vec::new());
    options.menu_class_name = Some(CONTEXT_VIEW_MENU_MOTION_CLASS.to_owned());
    options.close_animation = Some(custom_close_animation.clone());
    store.add_rc(DropdownMenu::new(&ui, overridden_container, options)).show();

    let observed: Vec<(Option<String>, Option<IContextViewCloseAnimation>)> = provider.delegates.borrow().iter().map(|delegate| (delegate.get_menu_class_name(), delegate.close_animation())).collect();
    assert_eq!(
        observed,
        vec![
            (Some(format!("custom-menu {CONTEXT_VIEW_MENU_MOTION_CLASS}")), Some(context_view_menu_close_animation())),
            (Some(CONTEXT_VIEW_MENU_MOTION_CLASS.to_owned()), Some(custom_close_animation)),
        ]
    );

    // `toDisposable(() => container.remove())`
    store.add(je_base::lifecycle::to_disposable({
        let ui = ui.clone();
        move || {
            ui.remove(default_container);
            ui.remove(overridden_container);
        }
    }));
}
