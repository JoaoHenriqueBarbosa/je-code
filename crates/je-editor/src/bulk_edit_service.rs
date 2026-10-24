//! Porte de `src/vs/editor/browser/services/bulkEditService.ts`: o `ResourceEdit` (com o `convert`), o `ResourceTextEdit`,
//! o `ResourceFileEdit` e o tipo do tratador de pré-visualização. O `IBulkEditService`, o `IBulkEditOptions` e o
//! `IBulkEditResult` ficam em [`crate::code_action::platform`], onde o `codeAction` já os lia.
//!
//! Divergências:
//! - **Herança.** `ResourceTextEdit extends ResourceEdit implements IWorkspaceTextEdit` e `ResourceFileEdit extends
//!   ResourceEdit implements IWorkspaceFileEdit` viram structs com os campos das interfaces (mais o `metadata` herdado de
//!   `ResourceEdit`); a conversão para a interface é o [`From`] para [`IWorkspaceTextEdit`] e [`IWorkspaceFileEdit`].
//!   A classe-base `ResourceEdit` é o enum [`ResourceEdit`], cujas variantes são as subclasses (o `instanceof` do
//!   original é o `match`). As subclasses de fora deste arquivo (`ResourceNotebookCellEdit` e `ResourceAttachmentEdit`)
//!   entram pela variante [`ResourceEdit::Custom`], o [`ICustomEdit`] do editor.
//! - **`ResourceTextEdit.is` e `ResourceFileEdit.is`.** O original aceita um objeto qualquer (`unknown`); aqui o candidato
//!   já é o [`IWorkspaceTextEdit`] ou o [`IWorkspaceFileEdit`], porque a checagem estrutural que sobra é só a de
//!   `ResourceFileEdit` (`newResource` ou `oldResource` presente); a de `ResourceTextEdit` (`resource` é uma URI e
//!   `textEdit` é um objeto) vale por construção do tipo.
//! - **`ResourceTextEdit.textEdit`.** É o [`TextEdit`] de `code_action::languages` (`TextEdit & { insertAsSnippet?,
//!   keepWhitespace? }`), cujo `range` nunca é ausente.
//! - **`ResourceEdit.convert`.** Devolve `Result`: o `throw new Error('Unsupported edit')` é o `Err`.

use std::rc::Rc;

use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::URI;

use crate::code_action::languages::{
    ICustomEdit, IWorkspaceFileEdit, IWorkspaceTextEdit, TextEdit, WorkspaceEdit, WorkspaceEditItem, WorkspaceEditMetadata, WorkspaceFileEditOptions,
};
use crate::code_action::platform::BulkEditOptions;

/// `ResourceEdit`: a classe-base das edições de recurso, uma variante por subclasse.
#[derive(Clone)]
pub enum ResourceEdit {
    /// `ResourceTextEdit`.
    Text(ResourceTextEdit),
    /// `ResourceFileEdit`.
    File(ResourceFileEdit),
    /// As subclasses definidas fora do editor (`ResourceNotebookCellEdit`, `ResourceAttachmentEdit`).
    Custom(Rc<dyn ICustomEdit>),
}

impl ResourceEdit {
    /// `metadata`.
    pub fn metadata(&self) -> Option<WorkspaceEditMetadata> {
        match self {
            Self::Text(edit) => edit.metadata.clone(),
            Self::File(edit) => edit.metadata.clone(),
            Self::Custom(edit) => edit.metadata(),
        }
    }

    /// `ResourceEdit.convert(edit)`.
    pub fn convert(edit: &WorkspaceEdit) -> Result<Vec<ResourceEdit>, Error> {
        edit.edits
            .iter()
            .map(|edit| match edit {
                WorkspaceEditItem::Text(edit) if ResourceTextEdit::is(edit) => Ok(Self::Text(ResourceTextEdit::lift(edit.clone()))),
                WorkspaceEditItem::File(edit) if ResourceFileEdit::is(edit) => Ok(Self::File(ResourceFileEdit::lift(edit.clone()))),
                _ => Err(Error::new("Unsupported edit")),
            })
            .collect()
    }
}

/// `IBulkEditPreviewHandler`: `(edits, options?) => Promise<ResourceEdit[]>`.
pub type BulkEditPreviewHandler = Rc<dyn Fn(Vec<ResourceEdit>, Option<BulkEditOptions>) -> Promise<Vec<ResourceEdit>>>;

/// `ResourceTextEdit`.
#[derive(Clone)]
pub struct ResourceTextEdit {
    pub resource: URI,
    pub text_edit: TextEdit,
    pub version_id: Option<i64>,
    /// `ResourceEdit.metadata`.
    pub metadata: Option<WorkspaceEditMetadata>,
}

impl ResourceTextEdit {
    /// `constructor(resource, textEdit, versionId = undefined, metadata?)`.
    pub fn new(resource: URI, text_edit: TextEdit, version_id: Option<i64>, metadata: Option<WorkspaceEditMetadata>) -> Self {
        Self { resource, text_edit, version_id, metadata }
    }

    /// `ResourceTextEdit.is(candidate)`: vale por construção do tipo (ver a nota do módulo).
    pub fn is(_candidate: &IWorkspaceTextEdit) -> bool {
        true
    }

    /// `ResourceTextEdit.lift(edit)`.
    pub fn lift(edit: IWorkspaceTextEdit) -> Self {
        Self::new(edit.resource, edit.text_edit, edit.version_id, edit.metadata)
    }
}

impl From<ResourceTextEdit> for IWorkspaceTextEdit {
    fn from(edit: ResourceTextEdit) -> Self {
        IWorkspaceTextEdit { resource: edit.resource, text_edit: edit.text_edit, version_id: edit.version_id, metadata: edit.metadata }
    }
}

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
