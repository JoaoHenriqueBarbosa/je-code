//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyEditorService.ts`: os contratos
//! `IWorkingCopyEditorHandler` (quem sabe abrir, reconhecer e criar o editor de uma working copy) e
//! `IWorkingCopyEditorService` (o registro desses handlers) e a classe [`WorkingCopyEditorService`].
//!
//! Divergências:
//! - **Valores `T | Promise<T>`.** `handles` (`boolean | Promise<boolean>`) e `createEditor` (`EditorInput |
//!   Promise<EditorInput>`) devolvem [`MaybePromise`].
//! - **Parâmetros `IWorkingCopyIdentifier`.** São a struct [`WorkingCopyIdentifier`] (ver `working_copy.rs`); uma working
//!   copy passada a `isOpen` vira o identificador dela por [`WorkingCopyIdentifier::of`] (no original ela própria é o
//!   identificador, por tipagem estrutural).
//! - **Injeção.** O parâmetro decorado `@IEditorService` é a tupla `Deps` do [`Injectable`]; `registerSingleton(...,
//!   InstantiationType.Delayed)` vira [`register`], e o proxy da instanciação atrasada é o
//!   [`WorkingCopyEditorServiceProxy`].
//! - **`Set<IWorkingCopyEditorHandler>`.** É um `Vec` em ordem de inserção sem repetir a mesma instância (a identidade do
//!   ponteiro, como o `Set` do JavaScript); o `delete` do descarte remove essa instância.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::promise::MaybePromise;
use je_instantiation::{Injectable, InstantiationType, delayed_proxy, register_singleton, service_id, service_impl};

use crate::common::editor::{EditorIdentifier, EditorInputRef, EditorsOrder};
use crate::services::editor::editor_service::IEditorService;
use crate::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyIdentifier};

/// `IWorkingCopyEditorHandler`.
pub trait IWorkingCopyEditorHandler {
    /// Whether the handler is capable of opening the specific backup in an editor.
    fn handles(&self, working_copy: &WorkingCopyIdentifier) -> MaybePromise<bool>;

    /// Whether the provided working copy is opened in the provided editor.
    fn is_open(&self, working_copy: &WorkingCopyIdentifier, editor: &EditorInputRef) -> bool;

    /// Create an editor that is suitable of opening the provided working copy.
    fn create_editor(&self, working_copy: &WorkingCopyIdentifier) -> MaybePromise<EditorInputRef>;
}

/// `IWorkingCopyEditorService`.
pub trait IWorkingCopyEditorService {
    /// An event fired whenever a handler is registered.
    fn on_did_register_handler(&self) -> Event<Rc<dyn IWorkingCopyEditorHandler>>;

    /// Register a handler to the working copy editor service.
    fn register_handler(&self, handler: Rc<dyn IWorkingCopyEditorHandler>) -> DisposableRef;

    /// Finds the first editor that can handle the provided working copy.
    fn find_editor(&self, working_copy: &dyn IWorkingCopy) -> Option<EditorIdentifier>;
}

delayed_proxy!(pub WorkingCopyEditorServiceProxy for dyn IWorkingCopyEditorService {
    methods {
        fn register_handler(&self, handler: Rc<dyn IWorkingCopyEditorHandler>) -> DisposableRef;
        fn find_editor(&self, working_copy: &dyn IWorkingCopy) -> Option<EditorIdentifier>;
    }
    events {
        fn on_did_register_handler(&self) -> Rc<dyn IWorkingCopyEditorHandler>;
    }
});

service_id!(pub I_WORKING_COPY_EDITOR_SERVICE: dyn IWorkingCopyEditorService = "workingCopyEditorService", proxy WorkingCopyEditorServiceProxy);

/// `WorkingCopyEditorService`.
pub struct WorkingCopyEditorService {
    base: Rc<Disposable>,
    on_did_register_handler: Rc<Emitter<Rc<dyn IWorkingCopyEditorHandler>>>,
    handlers: Rc<RefCell<Vec<Rc<dyn IWorkingCopyEditorHandler>>>>,
    editor_service: Rc<dyn IEditorService>,
}

impl WorkingCopyEditorService {
    /// `constructor(@IEditorService editorService)`.
    pub fn new(editor_service: Rc<dyn IEditorService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_register_handler = Emitter::new();
        base.register(on_did_register_handler.clone());
        Rc::new(Self { base, on_did_register_handler, handlers: Rc::default(), editor_service })
    }

    fn is_open(&self, working_copy: &dyn IWorkingCopy, editor: &EditorInputRef) -> bool {
        let identifier = WorkingCopyIdentifier::of(working_copy);
        // O laço do original não segura um empréstimo: um handler pode registrar outro durante `isOpen`.
        let handlers = self.handlers.borrow().clone();
        handlers.iter().any(|handler| handler.is_open(&identifier, editor))
    }
}

impl Injectable for WorkingCopyEditorService {
    type Args = ();
    type Deps = (Rc<dyn IEditorService>,);

    fn create(_: (), (editor_service,): Self::Deps) -> Rc<Self> {
        Self::new(editor_service)
    }
}

service_impl!(WorkingCopyEditorService => dyn IWorkingCopyEditorService, disposable);

/// `registerSingleton(IWorkingCopyEditorService, WorkingCopyEditorService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<WorkingCopyEditorService>(I_WORKING_COPY_EDITOR_SERVICE, InstantiationType::Delayed);
}

impl IWorkingCopyEditorService for WorkingCopyEditorService {
    fn on_did_register_handler(&self) -> Event<Rc<dyn IWorkingCopyEditorHandler>> {
        self.on_did_register_handler.event()
    }

    fn register_handler(&self, handler: Rc<dyn IWorkingCopyEditorHandler>) -> DisposableRef {
        // Add to registry and emit as event
        {
            let mut handlers = self.handlers.borrow_mut();
            if !handlers.iter().any(|candidate| std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&handler))) {
                handlers.push(handler.clone());
            }
        }
        self.on_did_register_handler.fire(handler.clone());

        let handlers = self.handlers.clone();
        to_disposable(move || handlers.borrow_mut().retain(|candidate| !std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&handler))))
    }

    fn find_editor(&self, working_copy: &dyn IWorkingCopy) -> Option<EditorIdentifier> {
        self.editor_service.get_editors(EditorsOrder::MostRecentlyActive).into_iter().find(|editor_identifier| self.is_open(working_copy, &editor_identifier.editor))
    }
}

impl IDisposable for WorkingCopyEditorService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
