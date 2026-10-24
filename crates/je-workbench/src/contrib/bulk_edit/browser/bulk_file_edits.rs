//! Porte de `src/vs/workbench/contrib/bulkEdit/browser/bulkFileEdits.ts`: o `BulkFileEdits` e as operações que ele
//! encadeia (`RenameOperation`, `CopyOperation`, `CreateOperation`, `DeleteOperation`), cada uma com a operação inversa que
//! o `FileUndoRedoElement` (um `IWorkspaceUndoRedoElement`) executa ao desfazer e ao refazer.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são o `Deps` do [`Injectable`], na ordem do original, e os
//!   argumentos estáticos são o `Args`; as operações são criadas por `createInstance` do [`InstantiationService`], como no
//!   original (o `IInstantiationService` é o `Rc<InstantiationService>`).
//! - **`IFileOperation`.** É o trait [`IFileOperation`]; `Noop` e as quatro operações o implementam, e `perform` devolve a
//!   operação inversa como `Rc<dyn IFileOperation>`. Como o corpo `async` precisa ser `'static`, cada `perform` copia o que
//!   usa das suas edições e dos serviços antes de começar.
//! - **Edições.** O discriminante `type` (`'rename' | 'copy' | 'delete' | 'create'`) é o enum [`FileEdit`], que agrupa
//!   edições vizinhas do mesmo tipo.
//! - **`fileService.resolve(uri, { resolveMetadata: true })`** é o [`IFileService::resolve_with_metadata`].
//! - **Opções espalhadas.** `{ recursive: true, folder: ..., ...edit.options }` do `CopyOperation` é a opção do edit quando
//!   ela existe e o valor do literal quando não existe (`None` é a chave ausente).
//! - **`edit.options.contents`.** É a `Promise<VSBuffer>` das opções; o `await` dela está no `apply`, antes do agrupamento.
//! - **`useTrash`.** O `&&` encadeado do original é o `bool` (o `undefined` do `getValue` ausente é `false`).
//! - **IDs do grupo.** O `undoRedoGroupId` do `IFileOperationUndoRedoInfo` é o `id` do [`UndoRedoGroup`] (`u64`) como
//!   `usize`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::network::schemas;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_base::dynamic::Truthy;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_editor::code_action::languages::WorkspaceFileEditOptions;
use je_editor::undo_redo::{IUndoRedoService, UndoRedoElementRef, UndoRedoGroup, UndoRedoSource, WorkspaceUndoRedoElement};
use je_instantiation::{Injectable, InstantiationService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileContent, FileSystemProviderCapabilities};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogMessage, LogService, Logger};
use je_platform::progress::progress::IProgress;

use crate::services::textfile::collaborators::{
    CopyOperation as ICopyOperation, CreateFileOperation as ICreateFileOperation, CreateOperation as ICreateOperation, DeleteOperation as IDeleteOperation,
    FileOperationUndoRedoInfo, IWorkingCopyFileService, MoveOperation as IMoveOperation, RequiredSourceTargetPair,
};
use crate::services::textfile::textfiles::{EncodedContent, ITextFileService};

/// `IFileOperation`.
pub trait IFileOperation {
    fn uris(&self) -> Vec<URI>;
    fn perform(&self, token: CancellationToken) -> Promise<Rc<dyn IFileOperation>>;
    /// `toString()`.
    fn describe(&self) -> String;
}

/// `new Noop()`.
fn noop() -> Rc<dyn IFileOperation> {
    Rc::new(Noop)
}

struct Noop;

impl IFileOperation for Noop {
    fn uris(&self) -> Vec<URI> {
        Vec::new()
    }

    fn perform(&self, _token: CancellationToken) -> Promise<Rc<dyn IFileOperation>> {
        Promise::resolved(noop())
    }

    fn describe(&self) -> String {
        "(noop)".to_string()
    }
}

/// `{ isUndoing: true }`.
fn undoing() -> FileOperationUndoRedoInfo {
    FileOperationUndoRedoInfo { undo_redo_group_id: None, is_undoing: Some(true) }
}

/// `options.overwrite === undefined && options.ignoreIfExists && await fileService.exists(newUri)`: a edição que não
/// sobrescreve, ignora o que já existe e tem o destino existente é pulada.
async fn is_skipped(file_service: &Rc<dyn IFileService>, new_uri: &URI, options: &WorkspaceFileEditOptions) -> Result<bool, Error> {
    Ok(options.overwrite.is_none() && options.ignore_if_exists.unwrap_or(false) && file_service.exists(new_uri).await?)
}

fn join_uris(uris: impl Iterator<Item = String>) -> String {
    uris.collect::<Vec<_>>().join(", ")
}

// #region RenameOperation

/// `RenameEdit`.
#[derive(Clone)]
pub struct RenameEdit {
    pub new_uri: URI,
    pub old_uri: URI,
    pub options: WorkspaceFileEditOptions,
}

/// `RenameOperation`.
pub struct RenameOperation {
    edits: Vec<RenameEdit>,
    undo_redo_info: FileOperationUndoRedoInfo,
    working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
    file_service: Rc<dyn IFileService>,
}

impl Injectable for RenameOperation {
    type Args = (Vec<RenameEdit>, FileOperationUndoRedoInfo);
    type Deps = (Rc<dyn IWorkingCopyFileService>, Rc<dyn IFileService>);

    fn create((edits, undo_redo_info): Self::Args, (working_copy_file_service, file_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { edits, undo_redo_info, working_copy_file_service, file_service })
    }
}

impl IFileOperation for RenameOperation {
    fn uris(&self) -> Vec<URI> {
        self.edits.iter().flat_map(|edit| [edit.new_uri.clone(), edit.old_uri.clone()]).collect()
    }

    fn perform(&self, token: CancellationToken) -> Promise<Rc<dyn IFileOperation>> {
        let (edits, undo_redo_info) = (self.edits.clone(), self.undo_redo_info);
        let (working_copy_file_service, file_service) = (self.working_copy_file_service.clone(), self.file_service.clone());
        async_fn(async move {
            let mut moves = Vec::new();
            let mut undoes = Vec::new();
            for edit in &edits {
                // check: not overwriting, but ignoring, and the target file exists
                if !is_skipped(&file_service, &edit.new_uri, &edit.options).await? {
                    moves.push(IMoveOperation {
                        file: RequiredSourceTargetPair { source: edit.old_uri.clone(), target: edit.new_uri.clone() },
                        overwrite: edit.options.overwrite,
                    });

                    // reverse edit
                    undoes.push(RenameEdit { new_uri: edit.old_uri.clone(), old_uri: edit.new_uri.clone(), options: edit.options.clone() });
                }
            }

            if moves.is_empty() {
                return Ok(noop());
            }

            working_copy_file_service.move_(moves, token, Some(undo_redo_info)).await?;
            let undo: Rc<dyn IFileOperation> = RenameOperation::create((undoes, undoing()), (working_copy_file_service, file_service));
            Ok(undo)
        })
    }

    fn describe(&self) -> String {
        format!("(rename {})", join_uris(self.edits.iter().map(|edit| format!("{} to {}", edit.old_uri.to_string(false), edit.new_uri.to_string(false)))))
    }
}

// #endregion

// #region CopyOperation

/// `CopyEdit`.
#[derive(Clone)]
pub struct CopyEdit {
    pub new_uri: URI,
    pub old_uri: URI,
    pub options: WorkspaceFileEditOptions,
}

/// `CopyOperation`.
pub struct CopyOperation {
    edits: Vec<CopyEdit>,
    undo_redo_info: FileOperationUndoRedoInfo,
    working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
    file_service: Rc<dyn IFileService>,
    instantiation_service: Rc<InstantiationService>,
}

impl Injectable for CopyOperation {
    type Args = (Vec<CopyEdit>, FileOperationUndoRedoInfo);
    type Deps = (Rc<dyn IWorkingCopyFileService>, Rc<dyn IFileService>, Rc<InstantiationService>);

    fn create((edits, undo_redo_info): Self::Args, (working_copy_file_service, file_service, instantiation_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { edits, undo_redo_info, working_copy_file_service, file_service, instantiation_service })
    }
}

impl IFileOperation for CopyOperation {
    fn uris(&self) -> Vec<URI> {
        self.edits.iter().flat_map(|edit| [edit.new_uri.clone(), edit.old_uri.clone()]).collect()
    }

    fn perform(&self, token: CancellationToken) -> Promise<Rc<dyn IFileOperation>> {
        let (edits, undo_redo_info) = (self.edits.clone(), self.undo_redo_info);
        let (working_copy_file_service, file_service, instantiation_service) =
            (self.working_copy_file_service.clone(), self.file_service.clone(), self.instantiation_service.clone());
        async_fn(async move {
            // (1) create copy operations, remove noops
            let mut copies: Vec<ICopyOperation> = Vec::new();
            for edit in &edits {
                // check: not overwriting, but ignoring, and the target file exists
                if !is_skipped(&file_service, &edit.new_uri, &edit.options).await? {
                    copies.push(ICopyOperation {
                        file: RequiredSourceTargetPair { source: edit.old_uri.clone(), target: edit.new_uri.clone() },
                        overwrite: edit.options.overwrite,
                    });
                }
            }

            if copies.is_empty() {
                return Ok(noop());
            }

            // (2) perform the actual copy and use the return stats to build undo edits
            let stats = working_copy_file_service.copy(copies, token, Some(undo_redo_info)).await?;
            let mut undoes = Vec::new();

            for (stat, edit) in stats.iter().zip(&edits) {
                let mut options = edit.options.clone();
                options.recursive = options.recursive.or(Some(true));
                options.folder = options.folder.or(Some(stat.is_directory));
                undoes.push(DeleteEdit { old_uri: stat.resource.clone(), options, undoes_create: false });
            }

            let undo: Rc<dyn IFileOperation> = instantiation_service.create_instance::<DeleteOperation>((undoes, undoing()));
            Ok(undo)
        })
    }

    fn describe(&self) -> String {
        format!("(copy {})", join_uris(self.edits.iter().map(|edit| format!("{} to {}", edit.old_uri.to_string(false), edit.new_uri.to_string(false)))))
    }
}

// #endregion

// #region CreateOperation

/// `CreateEdit`.
#[derive(Clone)]
pub struct CreateEdit {
    pub new_uri: URI,
    pub options: WorkspaceFileEditOptions,
    pub contents: Option<VSBuffer>,
}

/// `CreateOperation`.
pub struct CreateOperation {
    edits: Vec<CreateEdit>,
    undo_redo_info: FileOperationUndoRedoInfo,
    file_service: Rc<dyn IFileService>,
    working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
    instantiation_service: Rc<InstantiationService>,
    text_file_service: Rc<dyn ITextFileService>,
}

impl Injectable for CreateOperation {
    type Args = (Vec<CreateEdit>, FileOperationUndoRedoInfo);
    type Deps = (Rc<dyn IFileService>, Rc<dyn IWorkingCopyFileService>, Rc<InstantiationService>, Rc<dyn ITextFileService>);

    fn create((edits, undo_redo_info): Self::Args, (file_service, working_copy_file_service, instantiation_service, text_file_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { edits, undo_redo_info, file_service, working_copy_file_service, instantiation_service, text_file_service })
    }
}

impl IFileOperation for CreateOperation {
    fn uris(&self) -> Vec<URI> {
        self.edits.iter().map(|edit| edit.new_uri.clone()).collect()
    }

    fn perform(&self, token: CancellationToken) -> Promise<Rc<dyn IFileOperation>> {
        let (edits, undo_redo_info) = (self.edits.clone(), self.undo_redo_info);
        let (file_service, working_copy_file_service) = (self.file_service.clone(), self.working_copy_file_service.clone());
        let (instantiation_service, text_file_service) = (self.instantiation_service.clone(), self.text_file_service.clone());
        async_fn(async move {
            let mut folder_creates: Vec<ICreateOperation> = Vec::new();
            let mut file_creates: Vec<ICreateFileOperation> = Vec::new();
            let mut undoes = Vec::new();

            for edit in &edits {
                if *edit.new_uri.scheme() == schemas::UNTITLED {
                    continue; // ignore, will be handled by a later edit
                }
                if is_skipped(&file_service, &edit.new_uri, &edit.options).await? {
                    continue; // not overwriting, but ignoring, and the target file exists
                }
                let is_folder = edit.options.folder.unwrap_or(false);
                if is_folder {
                    folder_creates.push(ICreateOperation { resource: edit.new_uri.clone(), overwrite: None });
                } else {
                    // If the contents are part of the edit they include the encoding, thus use them. Otherwise get the encoding for a new empty file.
                    let encoded_readable = match &edit.contents {
                        Some(contents) => Some(EncodedContent::Buffer(contents.clone())),
                        None => text_file_service.get_encoded_readable(Some(&edit.new_uri), None, None).await?,
                    };
                    file_creates.push(ICreateFileOperation { resource: edit.new_uri.clone(), contents: encoded_readable, overwrite: edit.options.overwrite });
                }
                undoes.push(DeleteEdit { old_uri: edit.new_uri.clone(), options: edit.options.clone(), undoes_create: !is_folder && edit.contents.is_none() });
            }

            if folder_creates.is_empty() && file_creates.is_empty() {
                return Ok(noop());
            }

            working_copy_file_service.create_folder(folder_creates, token.clone(), Some(undo_redo_info)).await?;
            working_copy_file_service.create(file_creates, token, Some(undo_redo_info)).await?;

            let undo: Rc<dyn IFileOperation> = instantiation_service.create_instance::<DeleteOperation>((undoes, undoing()));
            Ok(undo)
        })
    }

    fn describe(&self) -> String {
        format!(
            "(create {})",
            join_uris(self.edits.iter().map(|edit| {
                if edit.options.folder.unwrap_or(false) {
                    format!("folder {}", edit.new_uri.to_string(false))
                } else {
                    format!("file {} with {} bytes", edit.new_uri.to_string(false), edit.contents.as_ref().map_or(0, VSBuffer::byte_length))
                }
            }))
        )
    }
}

// #endregion

// #region DeleteOperation

/// `DeleteEdit`.
#[derive(Clone)]
pub struct DeleteEdit {
    pub old_uri: URI,
    pub options: WorkspaceFileEditOptions,
    pub undoes_create: bool,
}

/// `DeleteOperation`.
pub struct DeleteOperation {
    edits: Vec<DeleteEdit>,
    undo_redo_info: FileOperationUndoRedoInfo,
    working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
    file_service: Rc<dyn IFileService>,
    configuration_service: Rc<dyn IConfigurationService>,
    instantiation_service: Rc<InstantiationService>,
    log_service: Rc<dyn LogService>,
}

impl Injectable for DeleteOperation {
    type Args = (Vec<DeleteEdit>, FileOperationUndoRedoInfo);
    type Deps = (Rc<dyn IWorkingCopyFileService>, Rc<dyn IFileService>, Rc<dyn IConfigurationService>, Rc<InstantiationService>, Rc<dyn LogService>);

    fn create(
        (edits, undo_redo_info): Self::Args,
        (working_copy_file_service, file_service, configuration_service, instantiation_service, log_service): Self::Deps,
    ) -> Rc<Self> {
        Rc::new(Self { edits, undo_redo_info, working_copy_file_service, file_service, configuration_service, instantiation_service, log_service })
    }
}

impl IFileOperation for DeleteOperation {
    fn uris(&self) -> Vec<URI> {
        self.edits.iter().map(|edit| edit.old_uri.clone()).collect()
    }

    fn perform(&self, token: CancellationToken) -> Promise<Rc<dyn IFileOperation>> {
        let (edits, undo_redo_info) = (self.edits.clone(), self.undo_redo_info);
        let (working_copy_file_service, file_service) = (self.working_copy_file_service.clone(), self.file_service.clone());
        let (configuration_service, instantiation_service, log_service) =
            (self.configuration_service.clone(), self.instantiation_service.clone(), self.log_service.clone());
        async_fn(async move {
            // delete file

            let mut deletes: Vec<IDeleteOperation> = Vec::new();
            let mut undoes: Vec<CreateEdit> = Vec::new();

            for edit in &edits {
                let file_stat = match file_service.resolve_with_metadata(&edit.old_uri, None).await {
                    Ok(file_stat) => file_stat,
                    Err(_) => {
                        if !edit.options.ignore_if_not_exists.unwrap_or(false) {
                            return Err(Error::new(format!("{} does not exist and can not be deleted", edit.old_uri.to_string(false))));
                        }
                        continue;
                    }
                };

                deletes.push(IDeleteOperation {
                    resource: edit.old_uri.clone(),
                    recursive: edit.options.recursive,
                    use_trash: Some(
                        !edit.options.skip_trash_bin.unwrap_or(false)
                            && file_service.has_capability(&edit.old_uri, FileSystemProviderCapabilities::TRASH)
                            && configuration_service.get_value(Some("files.enableTrash"), None).is_truthy(),
                    ),
                });

                if !file_stat.is_file && !file_stat.is_directory {
                    continue;
                }

                // read file contents for undo operation. when a file is too large it won't be restored
                let mut file_content: Option<FileContent> = None;
                let mut file_content_exceeds_max_size = false;
                if !edit.undoes_create && !edit.options.folder.unwrap_or(false) {
                    file_content_exceeds_max_size = edit.options.max_size.is_some_and(|max_size| file_stat.size() as f64 > max_size);
                    if !file_content_exceeds_max_size {
                        match file_service.read_file(&edit.old_uri, None, None).await {
                            Ok(content) => file_content = Some(content),
                            Err(err) => log_service.error_args(&LogMessage::Error(err), &[]),
                        }
                    }
                }
                if !file_content_exceeds_max_size {
                    undoes.push(CreateEdit { new_uri: edit.old_uri.clone(), options: edit.options.clone(), contents: file_content.map(|content| content.value) });
                }
            }

            if deletes.is_empty() {
                return Ok(noop());
            }

            working_copy_file_service.delete(deletes, token, Some(undo_redo_info)).await?;

            if undoes.is_empty() {
                return Ok(noop());
            }
            let undo: Rc<dyn IFileOperation> = instantiation_service.create_instance::<CreateOperation>((undoes, undoing()));
            Ok(undo)
        })
    }

    fn describe(&self) -> String {
        format!("(delete {})", join_uris(self.edits.iter().map(|edit| edit.old_uri.to_string(false).to_string())))
    }
}

// #endregion

// #region FileUndoRedoElement

/// `FileUndoRedoElement`.
struct FileUndoRedoElement {
    label: String,
    code: String,
    operations: Rc<RefCell<Vec<Rc<dyn IFileOperation>>>>,
    confirm_before_undo: bool,
    resources: Vec<URI>,
}

impl FileUndoRedoElement {
    fn new(label: String, code: String, operations: Vec<Rc<dyn IFileOperation>>, confirm_before_undo: bool) -> Self {
        let resources = operations.iter().flat_map(|op| op.uris()).collect();
        Self { label, code, operations: Rc::new(RefCell::new(operations)), confirm_before_undo, resources }
    }

    /// `_reverse()`.
    fn reverse(&self) -> Promise<()> {
        let operations = self.operations.clone();
        async_fn(async move {
            let count = operations.borrow().len();
            for i in 0..count {
                let op = operations.borrow()[i].clone();
                let undo = op.perform(CancellationToken::None).await?;
                operations.borrow_mut()[i] = undo;
            }
            Ok(())
        })
    }
}

impl fmt::Display for FileUndoRedoElement {
    /// `toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&join_uris(self.operations.borrow().iter().map(|op| op.describe())))
    }
}

impl WorkspaceUndoRedoElement for FileUndoRedoElement {
    fn resources(&self) -> Vec<URI> {
        self.resources.clone()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn code(&self) -> String {
        self.code.clone()
    }

    fn confirm_before_undo(&self) -> bool {
        self.confirm_before_undo
    }

    fn undo(&self) -> MaybePromise<()> {
        MaybePromise::Promise(self.reverse())
    }

    fn redo(&self) -> MaybePromise<()> {
        MaybePromise::Promise(self.reverse())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// #endregion

/// Uma edição de arquivo, com o `type` do original como variante.
#[derive(Clone)]
enum FileEdit {
    Rename(RenameEdit),
    Copy(CopyEdit),
    Delete(DeleteEdit),
    Create(CreateEdit),
}

/// As edições de um mesmo tipo, vizinhas na lista original.
enum FileEditGroup {
    Rename(Vec<RenameEdit>),
    Copy(Vec<CopyEdit>),
    Delete(Vec<DeleteEdit>),
    Create(Vec<CreateEdit>),
}

impl FileEditGroup {
    fn of(edit: FileEdit) -> Self {
        match edit {
            FileEdit::Rename(edit) => Self::Rename(vec![edit]),
            FileEdit::Copy(edit) => Self::Copy(vec![edit]),
            FileEdit::Delete(edit) => Self::Delete(vec![edit]),
            FileEdit::Create(edit) => Self::Create(vec![edit]),
        }
    }

    /// `lastGroup[0].type === edit.type`: acrescenta a edição ao grupo quando o tipo é o mesmo; devolve a edição quando não é.
    fn push(&mut self, edit: FileEdit) -> Option<FileEdit> {
        match (self, edit) {
            (Self::Rename(group), FileEdit::Rename(edit)) => group.push(edit),
            (Self::Copy(group), FileEdit::Copy(edit)) => group.push(edit),
            (Self::Delete(group), FileEdit::Delete(edit)) => group.push(edit),
            (Self::Create(group), FileEdit::Create(edit)) => group.push(edit),
            (_, edit) => return Some(edit),
        }
        None
    }
}

/// `BulkFileEdits`.
pub struct BulkFileEdits {
    label: String,
    code: String,
    undo_redo_group: UndoRedoGroup,
    undo_redo_source: Option<Rc<UndoRedoSource>>,
    confirm_before_undo: bool,
    progress: Rc<dyn IProgress<()>>,
    token: CancellationToken,
    edits: Vec<ResourceFileEdit>,
    instantiation_service: Rc<InstantiationService>,
    undo_redo_service: Rc<dyn IUndoRedoService>,
}

/// Os argumentos estáticos do construtor de [`BulkFileEdits`], na ordem do original.
pub struct BulkFileEditsArgs {
    pub label: String,
    pub code: String,
    pub undo_redo_group: UndoRedoGroup,
    pub undo_redo_source: Option<Rc<UndoRedoSource>>,
    pub confirm_before_undo: bool,
    pub progress: Rc<dyn IProgress<()>>,
    pub token: CancellationToken,
    pub edits: Vec<ResourceFileEdit>,
}

impl Injectable for BulkFileEdits {
    type Args = BulkFileEditsArgs;
    type Deps = (Rc<InstantiationService>, Rc<dyn IUndoRedoService>);

    fn create(args: Self::Args, (instantiation_service, undo_redo_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self {
            label: args.label,
            code: args.code,
            undo_redo_group: args.undo_redo_group,
            undo_redo_source: args.undo_redo_source,
            confirm_before_undo: args.confirm_before_undo,
            progress: args.progress,
            token: args.token,
            edits: args.edits,
            instantiation_service,
            undo_redo_service,
        })
    }
}

impl BulkFileEdits {
    /// `apply()`: `this` é o `Rc` do objeto criado por `createInstance`.
    pub fn apply(self: &Rc<Self>) -> Promise<Vec<URI>> {
        let this = self.clone();
        async_fn(async move { this.run().await })
    }

    async fn run(&self) -> Result<Vec<URI>, Error> {
        let mut undo_operations: Vec<Rc<dyn IFileOperation>> = Vec::new();
        let undo_redo_info = FileOperationUndoRedoInfo { undo_redo_group_id: Some(self.undo_redo_group.id as usize), is_undoing: None };

        let mut edits: Vec<FileEdit> = Vec::new();
        for edit in &self.edits {
            let options = edit.options.clone();
            match (&edit.new_resource, &edit.old_resource) {
                (Some(new_resource), Some(old_resource)) if !options.copy.unwrap_or(false) => {
                    edits.push(FileEdit::Rename(RenameEdit { new_uri: new_resource.clone(), old_uri: old_resource.clone(), options }));
                }
                (Some(new_resource), Some(old_resource)) => {
                    edits.push(FileEdit::Copy(CopyEdit { new_uri: new_resource.clone(), old_uri: old_resource.clone(), options }));
                }
                (None, Some(old_resource)) => {
                    edits.push(FileEdit::Delete(DeleteEdit { old_uri: old_resource.clone(), options, undoes_create: false }));
                }
                (Some(new_resource), None) => {
                    let contents = match options.contents.clone() {
                        Some(contents) => Some(contents.await?),
                        None => None,
                    };
                    edits.push(FileEdit::Create(CreateEdit { new_uri: new_resource.clone(), options, contents }));
                }
                (None, None) => {}
            }
        }

        if edits.is_empty() {
            return Ok(Vec::new());
        }

        let mut groups: Vec<FileEditGroup> = Vec::new();
        for edit in edits {
            let rejected = match groups.last_mut() {
                Some(last_group) => last_group.push(edit),
                None => Some(edit),
            };
            if let Some(edit) = rejected {
                groups.push(FileEditGroup::of(edit));
            }
        }

        for group in groups {
            if self.token.is_cancellation_requested() {
                break;
            }

            let op: Rc<dyn IFileOperation> = match group {
                FileEditGroup::Rename(group) => self.instantiation_service.create_instance::<RenameOperation>((group, undo_redo_info)),
                FileEditGroup::Copy(group) => self.instantiation_service.create_instance::<CopyOperation>((group, undo_redo_info)),
                FileEditGroup::Delete(group) => self.instantiation_service.create_instance::<DeleteOperation>((group, undo_redo_info)),
                FileEditGroup::Create(group) => self.instantiation_service.create_instance::<CreateOperation>((group, undo_redo_info)),
            };

            let undo_op = op.perform(self.token.clone()).await?;
            undo_operations.push(undo_op);
            self.progress.report(());
        }

        let undo_redo_element = Rc::new(FileUndoRedoElement::new(self.label.clone(), self.code.clone(), undo_operations, self.confirm_before_undo));
        let resources = undo_redo_element.resources.clone();
        self.undo_redo_service.push_element(UndoRedoElementRef::Workspace(undo_redo_element), Some(&self.undo_redo_group), self.undo_redo_source.as_deref());
        Ok(resources)
    }
}
