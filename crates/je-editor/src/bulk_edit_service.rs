//! Porte parcial de `src/vs/editor/browser/services/bulkEditService.ts`: o `ResourceEdit` e o `ResourceFileEdit`.
//!
//! O `ResourceTextEdit` e o `ResourceEdit.convert` (que o usa) entram com o `BulkEditService`; o `IBulkEditService` e as
//! opções ficam em [`crate::code_action::platform`], reduzidos ao que o `codeAction` lê.
//!
//! Divergências:
//! - **Herança.** `ResourceFileEdit extends ResourceEdit implements IWorkspaceFileEdit` vira a struct com os campos de
//!   [`IWorkspaceFileEdit`] (`oldResource`, `newResource`, `options` e o `metadata` herdado de `ResourceEdit`), e a
//!   conversão para a interface é o [`From`] para [`IWorkspaceFileEdit`].
//! - **`ResourceFileEdit.is` e `lift`.** O original aceita um objeto qualquer (`unknown`); aqui o candidato é o
//!   [`IWorkspaceFileEdit`], porque a checagem estrutural (`newResource` ou `oldResource` presente) é a única que sobra.

use je_base::uri::URI;

use crate::code_action::languages::{IWorkspaceFileEdit, WorkspaceEditMetadata, WorkspaceFileEditOptions};

/// `ResourceFileEdit`.
#[derive(Clone, Default)]
pub struct ResourceFileEdit {
    pub old_resource: Option<URI>,
    pub new_resource: Option<URI>,
    pub options: WorkspaceFileEditOptions,
    /// `ResourceEdit.metadata`.
    pub metadata: Option<WorkspaceEditMetadata>,
}

impl ResourceFileEdit {
    /// `constructor(oldResource, newResource, options = {}, metadata?)`.
    pub fn new(old_resource: Option<URI>, new_resource: Option<URI>, options: Option<WorkspaceFileEditOptions>, metadata: Option<WorkspaceEditMetadata>) -> Self {
        Self { old_resource, new_resource, options: options.unwrap_or_default(), metadata }
    }

    /// `ResourceFileEdit.is(candidate)`: `Boolean(newResource) || Boolean(oldResource)`.
    pub fn is(candidate: &IWorkspaceFileEdit) -> bool {
        candidate.new_resource.is_some() || candidate.old_resource.is_some()
    }

    /// `ResourceFileEdit.lift(edit)`.
    pub fn lift(edit: IWorkspaceFileEdit) -> Self {
        Self::new(edit.old_resource, edit.new_resource, edit.options, edit.metadata)
    }
}

impl From<ResourceFileEdit> for IWorkspaceFileEdit {
    fn from(edit: ResourceFileEdit) -> Self {
        IWorkspaceFileEdit { old_resource: edit.old_resource, new_resource: edit.new_resource, options: Some(edit.options), metadata: edit.metadata }
    }
}
