//! Porte parcial de `src/vs/workbench/services/workingCopy/common/workingCopyEditorService.ts`: os contratos
//! `IWorkingCopyEditorHandler` (quem sabe abrir, reconhecer e criar o editor de uma working copy) e
//! `IWorkingCopyEditorService` (o registro desses handlers).
//!
//! Divergências:
//! - **Valores `T | Promise<T>`.** `handles` (`boolean | Promise<boolean>`) e `createEditor` (`EditorInput |
//!   Promise<EditorInput>`) devolvem [`MaybePromise`].
//! - **Parâmetros `IWorkingCopyIdentifier`.** São a struct [`WorkingCopyIdentifier`] (ver `working_copy.rs`).
//! - **Classe.** A classe `WorkingCopyEditorService` lê `IEditorService.getEditors(EditorsOrder.MOST_RECENTLY_ACTIVE)`, que
//!   o `IEditorService` reduzido do porte ainda não tem; ela entra com o porte do `EditorService`. Este módulo é só o
//!   contrato que os handlers (como o do editor sem título) leem.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::MaybePromise;
use je_instantiation::service_id;

use crate::common::editor::{EditorIdentifier, EditorInputRef};
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

service_id!(pub I_WORKING_COPY_EDITOR_SERVICE: dyn IWorkingCopyEditorService = "workingCopyEditorService");
