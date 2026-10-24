//! Porte de `src/vs/platform/undoRedo/common/undoRedo.ts`: os tipos e o contrato do serviço de desfazer e refazer.
//!
//! Divergências:
//! - `IUndoRedoElement = IResourceUndoRedoElement | IWorkspaceUndoRedoElement` é o [`UndoRedoElementRef`]; o `type`
//!   (`UndoRedoElementType`) é a variante. `IResourceUndoRedoElement` é o trait [`UndoRedoElement`] e
//!   `IWorkspaceUndoRedoElement` é o trait [`WorkspaceUndoRedoElement`].
//! - `undo(): Promise<void> | void` do elemento de um recurso: [`UndoRedoElement::undo`] e [`UndoRedoElement::redo`]
//!   são síncronos (o caso comum, e o que os implementadores existentes têm); quem precisa de operação assíncrona
//!   sobrescreve [`UndoRedoElement::undo_maybe`] e [`UndoRedoElement::redo_maybe`], que são os que o serviço chama.
//!   No elemento de workspace `undo` e `redo` já devolvem `MaybePromise<()>`.
//! - `split?()` e `prepareUndoRedo?()` são métodos opcionais no original; aqui `split` devolve `Option` e
//!   [`WorkspaceUndoRedoElement::can_split`] diz se o método existe (o `typeof element.split === 'function'`).
//!   `prepareUndoRedo` devolve `Option<MaybePromise<DisposableRef>>` (`None` é `void`).
//! - `IUndoRedoService.undo(resource: URI | UndoRedoSource)` é o [`UndoRedoTarget`].
//! - `UndoRedoGroup.None` e `UndoRedoSource.None` são `UndoRedoGroup::none()` e `UndoRedoSource::none()`.
// TODO(integração): mover para o crate je-platform (platform/undoRedo/common).

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_base::promise::MaybePromise;
use je_base::uri::URI;

/// `UndoRedoElementType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoRedoElementType {
    Resource,
    Workspace,
}

/// `IResourceUndoRedoElement`: uma operação de desfazer sobre um recurso.
pub trait UndoRedoElement: Any {
    /// The resource impacted by this element.
    fn resource(&self) -> URI;
    /// A user presentable label. May be localized.
    fn label(&self) -> String;
    /// A code describing the operation. Will not be localized.
    fn code(&self) -> String;
    /// `confirmBeforeUndo`: show a message to the user confirming when trying to undo this element.
    fn confirm_before_undo(&self) -> bool {
        false
    }
    fn undo(&self);
    fn redo(&self);
    /// O `undo(): Promise<void> | void` do original. O padrão roda [`UndoRedoElement::undo`].
    fn undo_maybe(&self) -> MaybePromise<()> {
        self.undo();
        MaybePromise::Value(())
    }
    /// O `redo(): Promise<void> | void` do original. O padrão roda [`UndoRedoElement::redo`].
    fn redo_maybe(&self) -> MaybePromise<()> {
        self.redo();
        MaybePromise::Value(())
    }
    fn as_any(&self) -> &dyn Any;
}

/// `IWorkspaceUndoRedoElement`: uma operação de desfazer sobre vários recursos.
pub trait WorkspaceUndoRedoElement: Any {
    /// The resources impacted by this element.
    fn resources(&self) -> Vec<URI>;
    /// A user presentable label. May be localized.
    fn label(&self) -> String;
    /// A code describing the operation. Will not be localized.
    fn code(&self) -> String;
    /// `confirmBeforeUndo`: show a message to the user confirming when trying to undo this element.
    fn confirm_before_undo(&self) -> bool {
        false
    }
    fn undo(&self) -> MaybePromise<()>;
    fn redo(&self) -> MaybePromise<()>;

    /// `typeof split === 'function'`: indica que este elemento pode ser dividido em elementos por recurso
    /// ([`WorkspaceUndoRedoElement::split`]).
    fn can_split(&self) -> bool {
        false
    }

    /// `split?()`: if implemented, indicates that this undo/redo element can be split into multiple per resource
    /// elements. `None` quando o método não existe.
    fn split(&self) -> Option<Vec<Rc<dyn UndoRedoElement>>> {
        None
    }

    /// `prepareUndoRedo?()`: if implemented, will be invoked before calling `undo()` or `redo()`. This is a good place
    /// to prepare everything such that the calls to `undo()` or `redo()` are synchronous. If a disposable is returned,
    /// it will be invoked to clean things up. `None` é `void` (ou o método ausente).
    fn prepare_undo_redo(&self) -> Option<MaybePromise<DisposableRef>> {
        None
    }

    fn as_any(&self) -> &dyn Any;
}

/// `IUndoRedoElement`: `IResourceUndoRedoElement | IWorkspaceUndoRedoElement`.
#[derive(Clone)]
pub enum UndoRedoElementRef {
    Resource(Rc<dyn UndoRedoElement>),
    Workspace(Rc<dyn WorkspaceUndoRedoElement>),
}

impl From<Rc<dyn UndoRedoElement>> for UndoRedoElementRef {
    fn from(element: Rc<dyn UndoRedoElement>) -> Self {
        Self::Resource(element)
    }
}

impl From<Rc<dyn WorkspaceUndoRedoElement>> for UndoRedoElementRef {
    fn from(element: Rc<dyn WorkspaceUndoRedoElement>) -> Self {
        Self::Workspace(element)
    }
}

impl UndoRedoElementRef {
    /// `element.type`.
    pub fn element_type(&self) -> UndoRedoElementType {
        match self {
            Self::Resource(_) => UndoRedoElementType::Resource,
            Self::Workspace(_) => UndoRedoElementType::Workspace,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Resource(element) => element.label(),
            Self::Workspace(element) => element.label(),
        }
    }

    pub fn code(&self) -> String {
        match self {
            Self::Resource(element) => element.code(),
            Self::Workspace(element) => element.code(),
        }
    }

    pub fn confirm_before_undo(&self) -> bool {
        match self {
            Self::Resource(element) => element.confirm_before_undo(),
            Self::Workspace(element) => element.confirm_before_undo(),
        }
    }

    /// `undo()`.
    pub fn undo(&self) -> MaybePromise<()> {
        match self {
            Self::Resource(element) => element.undo_maybe(),
            Self::Workspace(element) => element.undo(),
        }
    }

    /// `redo()`.
    pub fn redo(&self) -> MaybePromise<()> {
        match self {
            Self::Resource(element) => element.redo_maybe(),
            Self::Workspace(element) => element.redo(),
        }
    }

    /// O objeto concreto do elemento (o `instanceof` do original).
    pub fn as_any(&self) -> &dyn Any {
        match self {
            Self::Resource(element) => element.as_any(),
            Self::Workspace(element) => element.as_any(),
        }
    }

    /// `element.type === UndoRedoElementType.Resource`.
    pub fn as_resource(&self) -> Option<&Rc<dyn UndoRedoElement>> {
        match self {
            Self::Resource(element) => Some(element),
            Self::Workspace(_) => None,
        }
    }

    /// `element.type === UndoRedoElementType.Workspace`.
    pub fn as_workspace(&self) -> Option<&Rc<dyn WorkspaceUndoRedoElement>> {
        match self {
            Self::Resource(_) => None,
            Self::Workspace(element) => Some(element),
        }
    }

    /// `a === b`: a identidade do objeto do elemento.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Resource(a), Self::Resource(b)) => std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b)),
            (Self::Workspace(a), Self::Workspace(b)) => std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b)),
            _ => false,
        }
    }
}

/// `IPastFutureElements`.
#[derive(Clone, Default)]
pub struct PastFutureElements {
    pub past: Vec<UndoRedoElementRef>,
    pub future: Vec<UndoRedoElementRef>,
}

/// `UriComparisonKeyComputer`.
pub trait UriComparisonKeyComputer {
    fn get_comparison_key(&self, uri: &URI) -> String;
}

/// `ResourceEditStackSnapshot`.
#[derive(Clone)]
pub struct ResourceEditStackSnapshot {
    pub resource: URI,
    pub elements: Vec<u64>,
}

thread_local! {
    static GROUP_ID: Cell<u64> = const { Cell::new(1) };
    static SOURCE_ID: Cell<u64> = const { Cell::new(1) };
}

/// `UndoRedoGroup`: elementos do mesmo grupo são desfeitos e refeitos juntos.
///
/// Divergência: o original passa a mesma referência de objeto adiante (o `NotebookTextModel` guarda o grupo num elemento
/// que só é empilhado depois). Aqui o contador de ordem é compartilhado (`Rc`) e o grupo é `Clone`: uma cópia avança
/// a mesma ordem que o original.
#[derive(Clone, Debug)]
pub struct UndoRedoGroup {
    pub id: u64,
    order: Rc<Cell<u64>>,
}

impl UndoRedoGroup {
    pub fn new() -> Self {
        Self {
            id: GROUP_ID.with(|id| {
                let current = id.get();
                id.set(current + 1);
                current
            }),
            order: Rc::new(Cell::new(1)),
        }
    }

    /// `UndoRedoGroup.None`: o grupo 0, que não agrupa nada.
    pub fn none() -> Self {
        Self {
            id: 0,
            order: Rc::new(Cell::new(1)),
        }
    }

    pub fn next_order(&self) -> u64 {
        if self.id == 0 {
            return 0;
        }
        let order = self.order.get();
        self.order.set(order + 1);
        order
    }
}

impl Default for UndoRedoGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// `UndoRedoSource`: quem originou o elemento; desfazer por origem pede confirmação ao trocar.
#[derive(Debug)]
pub struct UndoRedoSource {
    pub id: u64,
    order: Cell<u64>,
}

impl UndoRedoSource {
    pub fn new() -> Self {
        Self {
            id: SOURCE_ID.with(|id| {
                let current = id.get();
                id.set(current + 1);
                current
            }),
            order: Cell::new(1),
        }
    }

    /// `UndoRedoSource.None`.
    pub fn none() -> Self {
        Self {
            id: 0,
            order: Cell::new(1),
        }
    }

    pub fn next_order(&self) -> u64 {
        if self.id == 0 {
            return 0;
        }
        let order = self.order.get();
        self.order.set(order + 1);
        order
    }
}

impl Default for UndoRedoSource {
    fn default() -> Self {
        Self::new()
    }
}

/// O que identifica quem desfaz: um recurso ou uma origem (`URI | UndoRedoSource`).
#[derive(Clone, Copy)]
pub enum UndoRedoTarget<'a> {
    Resource(&'a URI),
    Source(&'a UndoRedoSource),
}

/// `IUndoRedoService`.
pub trait IUndoRedoService {
    /// Register an URI -> string hasher. This is useful for making multiple URIs share the same undo-redo stack.
    fn register_uri_comparison_key_computer(&self, scheme: &str, uri_comparison_key_computer: Rc<dyn UriComparisonKeyComputer>) -> DisposableRef;

    /// Get the hash used internally for a certain URI. This uses any registered `UriComparisonKeyComputer`.
    fn get_uri_comparison_key(&self, resource: &URI) -> String;

    /// Add a new element to the `undo` stack. This will destroy the `redo` stack.
    fn push_element(&self, element: UndoRedoElementRef, group: Option<&UndoRedoGroup>, source: Option<&UndoRedoSource>);

    /// Get the last pushed element for a resource. If the last pushed element has been undone, returns `None`.
    fn get_last_element(&self, resource: &URI) -> Option<UndoRedoElementRef>;

    /// Get all the elements associated with a resource. This includes the past and the future.
    fn get_elements(&self, resource: &URI) -> PastFutureElements;

    /// Validate or invalidate stack elements associated with a resource.
    fn set_elements_valid_flag(&self, resource: &URI, is_valid: bool, filter: &dyn Fn(&UndoRedoElementRef) -> bool);

    /// Remove elements that target `resource`.
    fn remove_elements(&self, resource: &URI);

    /// Create a snapshot of the current elements on the undo-redo stack for a resource.
    fn create_snapshot(&self, resource: &URI) -> ResourceEditStackSnapshot;

    /// Attempt (as best as possible) to restore a certain snapshot previously created with `createSnapshot` for a resource.
    fn restore_snapshot(&self, snapshot: &ResourceEditStackSnapshot);

    fn can_undo(&self, target: UndoRedoTarget<'_>) -> bool;
    fn undo(&self, target: UndoRedoTarget<'_>) -> MaybePromise<()>;

    fn can_redo(&self, target: UndoRedoTarget<'_>) -> bool;
    fn redo(&self, target: UndoRedoTarget<'_>) -> MaybePromise<()>;
}

je_instantiation::delayed_proxy!(pub UndoRedoServiceProxy for dyn IUndoRedoService {
    methods {
        fn register_uri_comparison_key_computer(&self, scheme: &str, uri_comparison_key_computer: Rc<dyn UriComparisonKeyComputer>) -> DisposableRef;
        fn get_uri_comparison_key(&self, resource: &URI) -> String;
        fn push_element(&self, element: UndoRedoElementRef, group: Option<&UndoRedoGroup>, source: Option<&UndoRedoSource>);
        fn get_last_element(&self, resource: &URI) -> Option<UndoRedoElementRef>;
        fn get_elements(&self, resource: &URI) -> PastFutureElements;
        fn set_elements_valid_flag(&self, resource: &URI, is_valid: bool, filter: &dyn Fn(&UndoRedoElementRef) -> bool);
        fn remove_elements(&self, resource: &URI);
        fn create_snapshot(&self, resource: &URI) -> ResourceEditStackSnapshot;
        fn restore_snapshot(&self, snapshot: &ResourceEditStackSnapshot);
        fn can_undo(&self, target: UndoRedoTarget<'_>) -> bool;
        fn undo(&self, target: UndoRedoTarget<'_>) -> MaybePromise<()>;
        fn can_redo(&self, target: UndoRedoTarget<'_>) -> bool;
        fn redo(&self, target: UndoRedoTarget<'_>) -> MaybePromise<()>;
    }
    events {}
});

je_instantiation::service_id!(pub I_UNDO_REDO_SERVICE: dyn IUndoRedoService = "undoRedoService", proxy UndoRedoServiceProxy);
