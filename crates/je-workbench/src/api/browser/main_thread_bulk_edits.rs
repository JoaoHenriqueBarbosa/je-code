//! Porte de `src/vs/workbench/api/browser/mainThreadBulkEdits.ts`: o `MainThreadBulkEdits`, que aplica as edições de
//! workspace que o host de extensões envia (`$tryApplyWorkspaceEdit`), e o `reviveWorkspaceEditDto`.
//!
//! Divergências:
//! - **`@extHostNamedCustomer(MainContext.MainThreadBulkEdits)` e o `IExtHostContext` do construtor:** o registro de
//!   clientes não existe ainda; o construtor recebe o [`IMainContext`], que (como no original) não é usado.
//! - **`revive<WorkspaceEdit>(data)`.** É a identidade: o DTO do porte já guarda as `URI` revividas. A edição de
//!   célula de notebook, que o `WorkspaceEdit` do editor não representa, viaja como a [`ResourceNotebookCellEdit`] (um
//!   `ICustomEdit` que o `BulkEditService` reconhece e aplica pelo `BulkCellEdits`), e o `outputs.items.valueBytes` das células
//!   não é reescrito.
//! - **`reviveWorkspaceEditDto` com dois tipos de retorno.** As duas sobrecargas (`IWorkspaceEditDto` e `IWorkspaceEditDto |
//!   undefined`) são uma função só; o `data` nulo (que devolve o próprio `undefined`) é o `Option` de quem chama.
//!   `Dto`s sem `edits` não existem (o campo é um `Vec`).
//! - **Erros.** O `throw` de `Could not revive data transfer file` é o `Err` do `Result`.
//! - **`$tryApplyWorkspaceEdit(dto, undoRedoGroupId?, isRefactoring?)`.** O `undoRedoGroupId` chega como `f64` (o
//!   `number` do protocolo) e vira o `id` (`u64`) do grupo.

use std::rc::Rc;

use je_base::buffer::{VSBuffer, decode_base64};
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::code_action::languages::{IWorkspaceFileEdit, WorkspaceEdit, WorkspaceEditItem, WorkspaceFileEditOptions};
use je_editor::code_action::platform::{BulkEditOptions, IBulkEditService};
use je_platform::log::{LogService, Logger};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::common::ext_host_protocol::{
    IMainContext, IWorkspaceEditDto, MainThreadBulkEditsShape, SerializableObjectWithBuffers, WorkspaceEditEntryDto, WorkspaceFileEditContentsDto,
    WorkspaceFileEditOptionsDto,
};
use crate::contrib::bulk_edit::browser::bulk_cell_edits::ResourceNotebookCellEdit;

/// `MainThreadBulkEdits`.
pub struct MainThreadBulkEdits {
    bulk_edit_service: Rc<dyn IBulkEditService>,
    log_service: Rc<dyn LogService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl MainThreadBulkEdits {
    pub fn new(
        _ext_host_context: &Rc<dyn IMainContext>,
        bulk_edit_service: Rc<dyn IBulkEditService>,
        log_service: Rc<dyn LogService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        Rc::new(Self { bulk_edit_service, log_service, uri_identity_service })
    }
}

impl IDisposable for MainThreadBulkEdits {
    fn dispose(&self) {}
}

impl MainThreadBulkEditsShape for MainThreadBulkEdits {
    fn rpc_try_apply_workspace_edit(
        &self,
        workspace_edit_dto: SerializableObjectWithBuffers<IWorkspaceEditDto>,
        undo_redo_group_id: Option<f64>,
        is_refactoring: Option<bool>,
    ) -> Promise<bool> {
        let edits = match revive_workspace_edit_dto(&workspace_edit_dto.value, &self.uri_identity_service, None) {
            Ok(edits) => edits,
            Err(error) => return Promise::rejected(error),
        };
        let log_service = self.log_service.clone();
        self.bulk_edit_service
            .apply(
                &edits,
                BulkEditOptions { undo_redo_group_id: undo_redo_group_id.map(|id| id as u64), respect_auto_save_config: is_refactoring, ..BulkEditOptions::default() },
            )
            .then_result(move |result| match result {
                Ok(result) => Ok(result.is_applied),
                Err(error) => {
                    log_service.warn(&format!("IGNORING workspace edit: {error}"));
                    Ok(false)
                }
            })
    }
}

/// `resolveDataTransferFile?: (id: string) => Promise<VSBuffer>` de `reviveWorkspaceEditDto`.
pub type ResolveDataTransferFile = Rc<dyn Fn(&str) -> Promise<VSBuffer>>;

fn revive_file_edit_options(options: &WorkspaceFileEditOptionsDto, resolve_data_transfer_file: Option<&ResolveDataTransferFile>) -> Result<WorkspaceFileEditOptions, Error> {
    let contents = match &options.contents {
        Some(WorkspaceFileEditContentsDto::Base64(value)) => Some(Promise::resolved(decode_base64(value))),
        Some(WorkspaceFileEditContentsDto::DataTransferItem(id)) => match resolve_data_transfer_file {
            Some(resolve) => Some(resolve(id)),
            None => return Err(Error::new("Could not revive data transfer file")),
        },
        None => None,
    };
    Ok(WorkspaceFileEditOptions {
        overwrite: options.overwrite,
        ignore_if_not_exists: options.ignore_if_not_exists,
        ignore_if_exists: options.ignore_if_exists,
        recursive: options.recursive,
        copy: options.copy,
        folder: options.folder,
        skip_trash_bin: options.skip_trash_bin,
        max_size: options.max_size,
        contents,
    })
}

/// `reviveWorkspaceEditDto(data, uriIdentityService, resolveDataTransferFile?)`.
pub fn revive_workspace_edit_dto(
    data: &IWorkspaceEditDto,
    uri_identity_service: &Rc<dyn IUriIdentityService>,
    resolve_data_transfer_file: Option<&ResolveDataTransferFile>,
) -> Result<WorkspaceEdit, Error> {
    let mut edits = Vec::with_capacity(data.edits.len());
    for edit in &data.edits {
        edits.push(match edit {
            WorkspaceEditEntryDto::Text(text_edit) => {
                let mut text_edit = text_edit.clone();
                text_edit.resource = uri_identity_service.as_canonical_uri(&text_edit.resource);
                WorkspaceEditItem::Text(text_edit)
            }
            WorkspaceEditEntryDto::File(file_edit) => {
                let options = match &file_edit.options {
                    Some(options) => Some(revive_file_edit_options(options, resolve_data_transfer_file)?),
                    None => None,
                };
                WorkspaceEditItem::File(IWorkspaceFileEdit {
                    old_resource: file_edit.old_resource.as_ref().map(|resource| uri_identity_service.as_canonical_uri(resource)),
                    new_resource: file_edit.new_resource.as_ref().map(|resource| uri_identity_service.as_canonical_uri(resource)),
                    options,
                    metadata: file_edit.metadata.clone(),
                })
            }
            WorkspaceEditEntryDto::Cell(cell_edit) => {
                let mut cell_edit = cell_edit.clone();
                cell_edit.resource = uri_identity_service.as_canonical_uri(&cell_edit.resource);
                WorkspaceEditItem::Custom(Rc::new(ResourceNotebookCellEdit::lift(&cell_edit)))
            }
        });
    }
    Ok(WorkspaceEdit { edits })
}
