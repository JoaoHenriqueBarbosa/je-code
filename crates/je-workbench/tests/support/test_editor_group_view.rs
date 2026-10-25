//! Porte de `TestEditorGroupView` de `src/vs/workbench/test/browser/workbenchTestServices.ts`: o grupo de editores de teste.
//!
//! Notas de tradução:
//! - Só o recorte que os testes leem existe: `id`, `windowId` (`mainWindow.vscodeWindowId`) e `editors`, que o teste atribui
//!   (campo com `RefCell`, atribuído por [`TestEditorGroupView::set_editors`]). Os demais membros do `IEditorGroupView` do
//!   original são os padrões de `IEditorGroup`, que lançam `Method not implemented.`, como o dublê.
//! - `activeEditor` é `undefined!` no original (nunca atribuído nestes testes): `None`.

use std::cell::RefCell;
use std::rc::Rc;

use je_workbench::common::editor::{EditorInputOrUntyped, EditorInputRef, GroupIdentifier};
use je_workbench::services::editor::editor_groups_service::IEditorGroup;
use je_workbench::services::layout::layout_service::{MAIN_WINDOW, WindowId};

/// `TestEditorGroupView`.
pub struct TestEditorGroupView {
    id: GroupIdentifier,
    editors: RefCell<Vec<EditorInputRef>>,
}

impl TestEditorGroupView {
    /// `constructor(public id: number)`.
    pub fn new(id: GroupIdentifier) -> Rc<Self> {
        Rc::new(Self { id, editors: RefCell::new(Vec::new()) })
    }

    /// `group.editors = [...]`.
    pub fn set_editors(&self, editors: Vec<EditorInputRef>) {
        *self.editors.borrow_mut() = editors;
    }
}

impl IEditorGroup for TestEditorGroupView {
    fn id(&self) -> GroupIdentifier {
        self.id
    }

    fn window_id(&self) -> WindowId {
        MAIN_WINDOW
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        None
    }

    /// `focus(): void { }`.
    fn focus(&self) {}

    fn editors(&self) -> Vec<EditorInputRef> {
        self.editors.borrow().clone()
    }

    fn is_locked(&self) -> bool {
        false
    }

    fn is_active(&self, _editor: &EditorInputOrUntyped) -> bool {
        false
    }
}
