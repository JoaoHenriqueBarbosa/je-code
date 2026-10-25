//! Porte de `src/vs/workbench/test/browser/actions/menuMotion.test.ts`.
//!
//! Notas de tradução:
//! - `$('.anchor')` é o [`ViewElement`] com a classe `anchor`; o `deepStrictEqual` sobre o `HTMLElement` compara a identidade
//!   do elemento, que é o `Rc::ptr_eq` da âncora devolvida.
//! - `options.getMenuClassName?.()` e `options.closeAnimation` são os métodos de [`WorkbenchMenuMotionContextMenuOptions`].
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::contextview::context_view::AnchorTarget;
use je_platform::contextview::view_element::ViewElement;
use je_workbench::browser::actions::menu_motion::{WORKBENCH_MENU_MOTION_CLASS, get_workbench_menu_motion_context_menu_options, workbench_menu_close_animation};

#[test]
fn provides_element_anchoring_and_motion_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let anchor = ViewElement::with_classes(&["anchor"]);
    let options = get_workbench_menu_motion_context_menu_options(anchor.clone());

    let AnchorTarget::Element(actual_anchor) = options.get_anchor() else { panic!("a âncora devia ser o elemento") };
    assert!(Rc::ptr_eq(&actual_anchor, &anchor));
    assert_eq!(options.get_menu_class_name(), Some(WORKBENCH_MENU_MOTION_CLASS.to_owned()));
    assert_eq!(options.close_animation(), Some(workbench_menu_close_animation()));
}
