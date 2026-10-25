//! Porte de `src/vs/editor/contrib/stickyScroll/browser/stickyScrollContribution.ts`: o registro do
//! [`StickyScrollController`] como contribuição do editor e das seis ações do sticky scroll.
//!
//! Representação (o mesmo molde de `register_folding`): as chamadas `registerEditorContribution(...)` e `registerAction2(...)`
//! que o original faz ao carregar o módulo são [`register_sticky_scroll`], que o dono do registro chama uma vez. A instanciação
//! do controlador no original é `createInstance` com os serviços injetados; aqui é o `services` recebido, que entrega os
//! [`StickyScrollControllerServices`] de cada editor. A função é idempotente: o registro acontece uma vez por thread.

use std::cell::Cell;
use std::rc::Rc;

use je_platform::actions::register_action2;

use super::sticky_scroll_actions::{FocusStickyScroll, StickyScrollFocusedAction, StickyScrollFocusedActionKind, ToggleStickyScroll};
use super::sticky_scroll_controller::{StickyScrollController, StickyScrollControllerServices};
use crate::code_editor_widget::{CodeEditorWidget, EditorContributionInstantiation, IEditorContribution};
use crate::editor_extensions::{EditorAction2Adapter, register_editor_contribution};

thread_local! {
    /// O registro da contribuição e das ações acontece uma vez por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `registerEditorContribution(StickyScrollController.ID, StickyScrollController, EditorContributionInstantiation.AfterFirstRender)`
/// e os seis `registerAction2(...)`.
pub fn register_sticky_scroll(services: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> StickyScrollControllerServices>) {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_contribution(
        StickyScrollController::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> { StickyScrollController::new(editor, services(editor)) }),
        EditorContributionInstantiation::AfterFirstRender,
    );
    register_action2(Rc::new(EditorAction2Adapter(ToggleStickyScroll)));
    register_action2(Rc::new(EditorAction2Adapter(FocusStickyScroll)));
    for kind in StickyScrollFocusedActionKind::REGISTERED {
        register_action2(Rc::new(EditorAction2Adapter(StickyScrollFocusedAction { kind })));
    }
}
