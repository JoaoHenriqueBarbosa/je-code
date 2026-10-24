//! Porte de `src/vs/platform/undoRedo/common/undoRedo.ts`: o recorte do contrato `IUndoRedoService` que o editor
//! personalizado usa (`pushElement`, `removeElements`, `undo` e `redo`) e o elemento de recurso (`IResourceUndoRedoElement`).
//!
//! Divergências:
//! - O serviço de pilha (`UndoRedoService`) e o resto do contrato (`registerUriComparisonKeyComputer`, `getUriComparisonKey`,
//!   `getLastElement`, `getElements`, `setElementsValidFlag`, `createSnapshot`, `restoreSnapshot`, `canUndo`, `canRedo`) não
//!   foram portados: nada neste porte os chama. Os elementos de espaço de trabalho (`IWorkspaceUndoRedoElement`) e o
//!   `UndoRedoGroup` e o `UndoRedoSource` do `pushElement` também ficam de fora; só entra o elemento de recurso, que é o
//!   único que o `MainThreadCustomEditorModel` empurra.
//! - `undo(resource | source)` recebe só o `URI`. `undo()` e `redo()` do elemento (`Promise<void> | void`) devolvem
//!   sempre uma [`Promise`] (o `void` é a promessa já resolvida), e o `undo` do serviço também.

use std::rc::Rc;

use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `UndoRedoElementType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoRedoElementType {
    Resource,
    Workspace,
}

/// `IResourceUndoRedoElement`.
#[derive(Clone)]
pub struct ResourceUndoRedoElement {
    /// The resource impacted by this element.
    pub resource: URI,
    /// A user presentable label. May be localized.
    pub label: String,
    /// A code describing the operation. Will not be localized.
    pub code: String,
    /// Show a message to the user confirming when trying to undo this element.
    pub confirm_before_undo: Option<bool>,
    pub undo: Rc<dyn Fn() -> Promise<()>>,
    pub redo: Rc<dyn Fn() -> Promise<()>>,
}

impl ResourceUndoRedoElement {
    /// `type: UndoRedoElementType.Resource`.
    pub fn element_type(&self) -> UndoRedoElementType {
        UndoRedoElementType::Resource
    }
}

/// `IUndoRedoService` (ver a nota do módulo).
pub trait IUndoRedoService {
    /// Add a new element to the `undo` stack. This will destroy the `redo` stack.
    fn push_element(&self, element: ResourceUndoRedoElement);

    /// Remove elements that target `resource`.
    fn remove_elements(&self, resource: &URI);

    fn undo(&self, resource: &URI) -> Promise<()>;

    fn redo(&self, resource: &URI) -> Promise<()>;
}

service_id!(pub I_UNDO_REDO_SERVICE: dyn IUndoRedoService = "undoRedoService");
