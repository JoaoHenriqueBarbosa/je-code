//! Porte de `src/vs/workbench/contrib/bulkEdit/browser/opaqueEdits.ts`: o `ResourceAttachmentEdit` e o `OpaqueEdits`.
//!
//! Divergências:
//! - **`ResourceAttachmentEdit.is` e `lift`.** No original servem para reconhecer e converter um objeto qualquer com
//!   `undo` e `redo`. Aqui o candidato já é o [`ICustomEdit`] do editor, que tem os dois por contrato, então os dois
//!   são a identidade e não existem como funções: o [`OpaqueEdits`] recebe os `Rc<dyn ICustomEdit>` direto, e o
//!   [`ResourceAttachmentEdit`] é só uma implementação de [`ICustomEdit`] com fechos.
//! - **`undo: () => Promise<void> | void`.** Os fechos devolvem [`MaybePromise`]; o [`ICustomEdit`] sempre devolve
//!   [`Promise`], e o `void` vira a promessa já resolvida.
//! - **Elemento de desfazer.** O literal `{ type: UndoRedoElementType.Resource, ... }` é uma struct que implementa
//!   [`UndoRedoElement`]: o serviço chama `undo_maybe` e `redo_maybe`, que devolvem o que o fecho devolve; `undo` e
//!   `redo` (sem retorno) só existem para satisfazer o contrato e rodam o mesmo fecho sem esperar a promessa.
//! - **`edit.metadata?.label || 'Custom Edit'`.** O rótulo vazio é falso, como no JavaScript.
//! - **Injeção.** `@IUndoRedoService` é o `Deps` do [`Injectable`]; os argumentos estáticos são o [`OpaqueEditsArgs`].

use std::any::Any;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::code_action::languages::{ICustomEdit, WorkspaceEditMetadata};
use je_editor::undo_redo::{IUndoRedoService, UndoRedoElement, UndoRedoElementRef, UndoRedoGroup, UndoRedoSource};
use je_instantiation::Injectable;
use je_platform::progress::progress::IProgress;

/// `() => Promise<void> | void` do `undo` e do `redo` de uma edição.
pub type UndoRedoAction = Rc<dyn Fn() -> MaybePromise<()>>;

/// `MaybePromise<()>` como a [`Promise`] que o [`ICustomEdit`] devolve.
fn to_promise(value: MaybePromise<()>) -> Promise<()> {
    match value {
        MaybePromise::Value(()) => Promise::resolved(()),
        MaybePromise::Promise(promise) => promise,
    }
}

/// `ResourceAttachmentEdit`.
pub struct ResourceAttachmentEdit {
    pub resource: URI,
    pub undo: UndoRedoAction,
    pub redo: UndoRedoAction,
    /// `ResourceEdit.metadata`.
    pub metadata: Option<WorkspaceEditMetadata>,
}

impl ResourceAttachmentEdit {
    /// `constructor(resource, undo, redo, metadata?)`.
    pub fn new(resource: URI, undo: UndoRedoAction, redo: UndoRedoAction, metadata: Option<WorkspaceEditMetadata>) -> Self {
        Self { resource, undo, redo, metadata }
    }
}

impl ICustomEdit for ResourceAttachmentEdit {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn metadata(&self) -> Option<WorkspaceEditMetadata> {
        self.metadata.clone()
    }

    fn undo(&self) -> Promise<()> {
        to_promise((self.undo)())
    }

    fn redo(&self) -> Promise<()> {
        to_promise((self.redo)())
    }
}

/// O elemento `{ type: UndoRedoElementType.Resource, resource, label, code: 'paste', undo, redo }` de `apply()`.
struct OpaqueUndoRedoElement {
    resource: URI,
    label: String,
    undo: UndoRedoAction,
    redo: UndoRedoAction,
}

impl UndoRedoElement for OpaqueUndoRedoElement {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn code(&self) -> String {
        "paste".to_string()
    }

    fn undo(&self) {
        drop(self.undo_maybe());
    }

    fn redo(&self) {
        drop(self.redo_maybe());
    }

    fn undo_maybe(&self) -> MaybePromise<()> {
        (self.undo)()
    }

    fn redo_maybe(&self) -> MaybePromise<()> {
        (self.redo)()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Os argumentos estáticos do construtor de [`OpaqueEdits`], na ordem do original.
pub struct OpaqueEditsArgs {
    pub undo_redo_group: UndoRedoGroup,
    pub undo_redo_source: Option<Rc<UndoRedoSource>>,
    pub progress: Rc<dyn IProgress<()>>,
    pub token: CancellationToken,
    pub edits: Vec<Rc<dyn ICustomEdit>>,
}

/// `OpaqueEdits`.
pub struct OpaqueEdits {
    undo_redo_group: UndoRedoGroup,
    undo_redo_source: Option<Rc<UndoRedoSource>>,
    progress: Rc<dyn IProgress<()>>,
    token: CancellationToken,
    edits: Vec<Rc<dyn ICustomEdit>>,
    undo_redo_service: Rc<dyn IUndoRedoService>,
}

impl Injectable for OpaqueEdits {
    type Args = OpaqueEditsArgs;
    type Deps = (Rc<dyn IUndoRedoService>,);

    fn create(args: Self::Args, (undo_redo_service,): Self::Deps) -> Rc<Self> {
        Rc::new(Self {
            undo_redo_group: args.undo_redo_group,
            undo_redo_source: args.undo_redo_source,
            progress: args.progress,
            token: args.token,
            edits: args.edits,
            undo_redo_service,
        })
    }
}

impl OpaqueEdits {
    /// `apply()`: `this` é o `Rc` do objeto criado por `createInstance`.
    pub fn apply(self: &Rc<Self>) -> Promise<Vec<URI>> {
        let this = self.clone();
        async_fn(async move {
            let mut resources: Vec<URI> = Vec::new();

            for edit in &this.edits {
                if this.token.is_cancellation_requested() {
                    break;
                }

                edit.redo().await?;

                let (undo_edit, redo_edit) = (edit.clone(), edit.clone());
                let element = OpaqueUndoRedoElement {
                    resource: edit.resource(),
                    label: edit.metadata().map(|metadata| metadata.label).filter(|label| !label.is_empty()).unwrap_or_else(|| "Custom Edit".to_string()),
                    undo: Rc::new(move || MaybePromise::Promise(undo_edit.undo())),
                    redo: Rc::new(move || MaybePromise::Promise(redo_edit.redo())),
                };
                this.undo_redo_service.push_element(UndoRedoElementRef::Resource(Rc::new(element)), Some(&this.undo_redo_group), this.undo_redo_source.as_deref());

                this.progress.report(());
                resources.push(edit.resource());
            }

            Ok(resources)
        })
    }
}
