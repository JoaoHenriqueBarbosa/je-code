//! Porte de `src/vs/workbench/contrib/files/browser/views/explorerViewer.ts`, fatia 2b (parte do arrastar e soltar): o
//! restante do `FileDragAndDrop` (`onDragOver`, `getDragURI`, `getDragLabel`, `onDragStart`, `drop`, `handleExplorerDrop`,
//! `doHandleRootDrop`, `doHandleExplorerDropOnCopy`, `doHandleExplorerDropOnMove` e os auxiliares de pilhas comprimidas).
//! A decisão pura de `handleDragOver` está em [`super::explorer_viewer::handle_drag_over`].
//!
//! Divergências:
//! - **DOM.** O original acha o nome comprimido sob o ponteiro com `elementFromPoint` e os atributos
//!   `data-icon-label-count`/`-index`; aqui o widget de rótulo entrega o resultado como [`IconLabelNameHit`] e o
//!   destaque `drop-target` do nome comprimido volta em [`DragOverOutcome::compressed_drop_target`].
//! - **`ExternalFileImport`/`BrowserFileUpload`.** São de `fileImportExport.ts` e dependem do `DataTransfer` nativo da
//!   janela; o drop de arquivos de fora chega como o [`ExternalFileDropHandler`] injetado. O ramo `isWeb` não existe: o
//!   porte é nativo.
//! - **`openEditors`.** Os editores copiados abrem em sequência com `openEditor({ resource, options: { pinned: true } })`.
//! - **JSON de `CodeDataTransfers.FILES`.** Montado à mão (lista de textos), sem depender de um serializador.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::error_message::to_error_message_of;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::resources::distinct_parents;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_editor::code_action::languages::WorkspaceFileEditOptions;
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::dialogs::{Checkbox, Confirmation, BaseDialogOptions, DialogDetail, FileNameOrResource, IDialogService, get_file_names_message};
use je_platform::files::files::{FileOperationError, FileOperationResult};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{fs_path, join_path};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use je_platform::workspaces::workspaces::WorkspaceFolderCreationData;

use super::explorer_viewer::{DragOverData, DragOverInput, DragOverReaction, TargetSector, get_file_or_folder_label_suffix, handle_drag_over};
use crate::browser::parts::views::tree_view::{CODE_DATA_TRANSFERS_FILES, IDragDataTransfer};
use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
use crate::contrib::files::browser::file_import_export::get_multiple_files_overwrite_confirm;
use crate::contrib::files::browser_files::{ExplorerBulkEditOptions, IExplorerService};
use crate::contrib::files::common_files::{IncrementalNamingSetting, UndoConfirmLevel};
use crate::contrib::files::explorer_model::ExplorerItem;
use crate::contrib::files::file_actions::{FileToPaste, PasteResource, find_valid_paste_file_target};
use crate::services::editor::editor_service::IEditorService;
use crate::services::workspaces::workspace_editing::IWorkspaceEditingService;

/// `FileDragAndDrop.CONFIRM_DND_SETTING_KEY`.
pub const CONFIRM_DND_SETTING_KEY: &str = "explorer.confirmDragAndDrop";

/// `explorer.enableDragAndDrop`.
const ENABLE_DND_SETTING_KEY: &str = "explorer.enableDragAndDrop";

/// O `ExternalFileImport.import(target, event)` (ou o upload do navegador) sobre o `DataTransfer` do evento.
pub type ExternalFileDropHandler = Rc<dyn Fn(&Rc<ExplorerItem>) -> Promise<()>>;

/// O resultado de `getIconLabelNameFromHTMLElement` sob o ponteiro: o nome comprimido (`count` partes, a de `index`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconLabelNameHit {
    pub count: usize,
    pub index: usize,
}

/// O dado solto: `NativeDragAndDropData` ou `ElementsDragAndDropData` (já resolvido por [`stats_from_drag_data`]).
#[derive(Clone)]
pub enum DropData {
    Native,
    Explorer(Vec<Rc<ExplorerItem>>),
}

/// O que `onDragOver` decide, mais o destaque do nome comprimido sob o ponteiro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragOverOutcome {
    pub reaction: DragOverReaction,
    /// O nome comprimido sob o ponteiro é o destino (a classe `drop-target`); o `feedback` da reação fica vazio.
    pub compressed_drop_target: bool,
}

/// `getCompressedStatFromDragEvent`: sobe do item para o pai tantas vezes quantas partes há depois da parte sob o ponteiro.
pub fn get_compressed_stat_from_drag_event(stat: &Rc<ExplorerItem>, hit: Option<IconLabelNameHit>) -> Rc<ExplorerItem> {
    let mut stat = stat.clone();
    if let Some(IconLabelNameHit { count, index }) = hit {
        let mut i = count as i64 - 1;
        while i > index as i64 {
            let Some(parent) = stat.parent() else { break };
            stat = parent;
            i -= 1;
        }
    }

    stat
}

/// `getStatsFromDragAndDropData`: as pilhas do `ElementsDragAndDropData`. Com um só elemento arrastado no início do
/// arrasto (`hit` presente), vale a pilha comprimida sob o ponteiro.
pub fn stats_from_drag_data(elements: &[Rc<ExplorerItem>], drag_start_hit: Option<Option<IconLabelNameHit>>) -> Vec<Rc<ExplorerItem>> {
    match drag_start_hit {
        Some(hit) if elements.len() == 1 => vec![get_compressed_stat_from_drag_event(&elements[0], hit)],
        _ => elements.to_vec(),
    }
}

/// Os serviços do `FileDragAndDrop`.
#[derive(Clone)]
pub struct FileDragAndDropServices {
    pub explorer_service: Rc<dyn IExplorerService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub file_service: Rc<dyn IFileService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub workspace_editing_service: Rc<dyn IWorkspaceEditingService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub external_file_drop: ExternalFileDropHandler,
}

/// `FileDragAndDrop`.
pub struct FileDragAndDrop {
    is_collapsed: Rc<dyn Fn(&Rc<ExplorerItem>) -> bool>,
    services: Rc<FileDragAndDropServices>,
    drop_enabled: Rc<Cell<bool>>,
    config_listener: DisposableRef,
}

fn drop_enabled_setting(configuration_service: &dyn IConfigurationService) -> bool {
    configuration_service.get_value(Some(ENABLE_DND_SETTING_KEY), None).is_truthy()
}

fn setting_text(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `explorer.<name>` da configuração.
fn explorer_setting(configuration_service: &dyn IConfigurationService, name: &str) -> Dynamic {
    configuration_service.get_value(None, None).get_str("explorer").get_str(name)
}

/// O texto JSON de uma lista de textos (`JSON.stringify(string[])`).
fn json_string_array(values: &[String]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                ch => out.push(ch),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}

impl FileDragAndDrop {
    /// `constructor(isCollapsed, ...)`.
    pub fn new(is_collapsed: Rc<dyn Fn(&Rc<ExplorerItem>) -> bool>, services: FileDragAndDropServices) -> Self {
        let drop_enabled = Rc::new(Cell::new(drop_enabled_setting(services.configuration_service.as_ref())));
        let config_listener = services.configuration_service.on_did_change_configuration().listen({
            let (drop_enabled, configuration_service) = (drop_enabled.clone(), services.configuration_service.clone());
            move |event| {
                if event.affects_configuration(ENABLE_DND_SETTING_KEY, None) {
                    drop_enabled.set(drop_enabled_setting(configuration_service.as_ref()));
                }
            }
        });

        Self { is_collapsed, services: Rc::new(services), drop_enabled, config_listener }
    }

    /// `onDragOver`. `is_copy` é `ctrlKey && !isMacintosh || altKey && isMacintosh`; `contains_files_type` é o
    /// `containsDragType(FILES, CodeDataTransfers.FILES, RESOURCES)` do dado nativo.
    pub fn on_drag_over(
        &self,
        data: DragOverData,
        target: Option<Rc<ExplorerItem>>,
        target_sector: Option<TargetSector>,
        is_copy: bool,
        hit: Option<IconLabelNameHit>,
    ) -> DragOverOutcome {
        let reject = DragOverOutcome { reaction: DragOverReaction::Reject, compressed_drop_target: false };
        if !self.drop_enabled.get() {
            return reject;
        }

        // Compressed folders
        if let (Some(target), Some(hit)) = (&target, hit) {
            if hit.index + 1 < hit.count {
                let compressed_target = get_compressed_stat_from_drag_event(target, Some(hit));
                return match self.handle_drag_over(data, Some(compressed_target), target_sector, is_copy) {
                    DragOverReaction::Reject => reject,
                    accepted => DragOverOutcome { reaction: accepted, compressed_drop_target: true },
                };
            }
        }

        DragOverOutcome { reaction: self.handle_drag_over(data, target, target_sector, is_copy), compressed_drop_target: false }
    }

    fn handle_drag_over(&self, data: DragOverData, target: Option<Rc<ExplorerItem>>, target_sector: Option<TargetSector>, is_copy: bool) -> DragOverReaction {
        let ext_uri = self.services.uri_identity_service.ext_uri();
        handle_drag_over(&DragOverInput { data, target, target_sector, is_copy, ext_uri: &ext_uri, context_service: self.services.context_service.as_ref() })
    }

    /// `getDragURI`.
    pub fn get_drag_uri(&self, element: &Rc<ExplorerItem>) -> Option<String> {
        if self.services.explorer_service.is_editable(Some(element)) {
            return None;
        }

        Some(element.resource().to_string(false).to_string_lossy())
    }

    /// `getDragLabel`.
    pub fn get_drag_label(&self, elements: &[Rc<ExplorerItem>], hit: Option<IconLabelNameHit>) -> Option<String> {
        if elements.len() == 1 {
            return Some(get_compressed_stat_from_drag_event(&elements[0], hit).name());
        }

        Some(elements.len().to_string())
    }

    /// `onDragStart`.
    pub fn on_drag_start(&self, items: &[Rc<ExplorerItem>], data_transfer: Option<&dyn IDragDataTransfer>) {
        let Some(data_transfer) = data_transfer else { return };
        if items.is_empty() {
            return;
        }

        // Apply some datatransfer types to allow for dragging the element outside of the application
        let resources: Vec<URI> = items.iter().map(|item| item.resource()).collect();
        data_transfer.fill_editors_drag_data(&resources);

        // The only custom data transfer we set from the explorer is a file transfer
        // to be able to DND between multiple code file explorers across windows
        let file_resources: Vec<String> = resources.iter().filter(|resource| resource.scheme().to_string_lossy() == schemas::FILE).map(fs_path).collect();
        if !file_resources.is_empty() {
            data_transfer.set_data(CODE_DATA_TRANSFERS_FILES, &json_string_array(&file_resources));
        }
    }

    /// `drop`. O erro de qualquer passo vira o diálogo de erro, como no original.
    pub fn drop(&self, data: DropData, target: Option<Rc<ExplorerItem>>, target_sector: Option<TargetSector>, hit: Option<IconLabelNameHit>, is_copy: bool) -> Promise<()> {
        let services = self.services.clone();
        let is_collapsed = self.is_collapsed.clone();
        async_fn(async move {
            // Find compressed target
            let mut target = target.map(|target| get_compressed_stat_from_drag_event(&target, hit));
            let mut target_sector = target_sector;

            // Find parent to add to
            if target.is_none() {
                target = services.explorer_service.roots().last().cloned();
                target_sector = Some(TargetSector::Bottom);
            }
            let Some(mut target) = target else { return Ok(()) };
            if !target.is_directory() {
                if let Some(parent) = target.parent() {
                    target = parent;
                }
            }
            if target.is_readonly().is_readonly() {
                return Ok(());
            }

            let result = match data {
                // External file DND (Import/Upload file)
                DropData::Native => (services.external_file_drop)(&target).await,
                // In-Explorer DND (Move/Copy file)
                DropData::Explorer(elements) => handle_explorer_drop(services.clone(), is_collapsed, elements, target, target_sector, is_copy).await,
            };

            if let Err(error) = result {
                services.dialog_service.error(&to_error_message_of(&error, false), None).await?;
            }
            Ok(())
        })
    }

    /// `onDragEnd`: o destaque `drop-target` do nome comprimido é do widget (ver [`DragOverOutcome`]).
    pub fn on_drag_end(&self) {}
}

impl IDisposable for FileDragAndDrop {
    fn dispose(&self) {
        self.config_listener.dispose();
    }
}

/// `handleExplorerDrop`.
async fn handle_explorer_drop(
    services: Rc<FileDragAndDropServices>,
    is_collapsed: Rc<dyn Fn(&Rc<ExplorerItem>) -> bool>,
    elements: Vec<Rc<ExplorerItem>>,
    target: Rc<ExplorerItem>,
    target_sector: Option<TargetSector>,
    is_copy: bool,
) -> Result<(), Error> {
    // O `Map<ExplorerItem, boolean>` do original, na ordem de inserção.
    let mut distinct_items: Vec<(Rc<ExplorerItem>, bool)> = Vec::new();
    for element in elements {
        let collapsed = is_collapsed(&element);
        match distinct_items.iter_mut().find(|(item, _)| Rc::ptr_eq(item, &element)) {
            Some(entry) => entry.1 = collapsed,
            None => distinct_items.push((element, collapsed)),
        }
    }

    let mut index = 0;
    while index < distinct_items.len() {
        let (item, collapsed) = distinct_items[index].clone();
        index += 1;
        if !collapsed {
            continue;
        }

        if let Some(nested_children) = item.nested_children() {
            for child in nested_children {
                // if parent is collapsed, then the nested children is considered collapsed to operate as a group
                // and skip collapsed state check since they're not in the tree
                match distinct_items.iter_mut().find(|(item, _)| Rc::ptr_eq(item, &child)) {
                    Some(entry) => entry.1 = true,
                    None => distinct_items.push((child, true)),
                }
            }
        }
    }

    let all_items: Vec<Rc<ExplorerItem>> = distinct_items.into_iter().map(|(item, _)| item).collect();
    let ext_uri = services.uri_identity_service.ext_uri();
    let items: Vec<Rc<ExplorerItem>> = distinct_parents(&all_items, |s| s.resource(), &ext_uri).into_iter().cloned().collect();
    if items.is_empty() {
        return Ok(());
    }

    // Handle confirm setting
    let confirm_drag_and_drop = !is_copy && services.configuration_service.get_value(Some(CONFIRM_DND_SETTING_KEY), None).is_truthy();
    if confirm_drag_and_drop {
        let all_roots = items.iter().all(|s| s.is_root());
        let message = if items.len() > 1 && all_roots {
            localize("confirmRootsMove", "Are you sure you want to change the order of multiple root folders in your workspace?", &[])
        } else if items.len() > 1 {
            localize("confirmMultiMove", "Are you sure you want to move the following {0} files into '{1}'?", &[&items.len().to_string(), &target.name()])
        } else if items[0].is_root() {
            localize("confirmRootMove", "Are you sure you want to change the order of root folder '{0}' in your workspace?", &[&items[0].name()])
        } else {
            localize("confirmMove", "Are you sure you want to move '{0}' into '{1}'?", &[&items[0].name(), &target.name()])
        };
        let detail = if items.len() > 1 && !all_roots {
            let names: Vec<FileNameOrResource> = items.iter().map(|i| FileNameOrResource::Resource(i.resource())).collect();
            Some(DialogDetail::Text(get_file_names_message(&names)))
        } else {
            None
        };

        let confirmation = services
            .dialog_service
            .confirm(Confirmation {
                base: BaseDialogOptions {
                    detail,
                    checkbox: Some(Checkbox { label: localize("doNotAskAgain", "Do not ask me again", &[]), checked: None }),
                    ..BaseDialogOptions::new(message)
                },
                primary_button: Some(localize("moveButtonLabel", "&&Move", &[])),
                cancel_button: None,
            })
            .await?;

        if !confirmation.confirmed {
            return Ok(());
        }

        // Check for confirmation checkbox
        if confirmation.checkbox_checked == Some(true) {
            services.configuration_service.update_value(CONFIRM_DND_SETTING_KEY, Dynamic::Bool(false), None, None).await?;
        }
    }

    let roots: Vec<Rc<ExplorerItem>> = items.iter().filter(|s| s.is_root()).cloned().collect();
    do_handle_root_drop(&services, &roots, &target, target_sector).await?;

    let sources: Vec<Rc<ExplorerItem>> = items.iter().filter(|s| !s.is_root()).cloned().collect();
    if is_copy {
        return do_handle_explorer_drop_on_copy(&services, &sources, &target).await;
    }

    do_handle_explorer_drop_on_move(&services, &sources, &target).await
}

/// `doHandleRootDrop`.
async fn do_handle_root_drop(services: &FileDragAndDropServices, roots: &[Rc<ExplorerItem>], target: &Rc<ExplorerItem>, target_sector: Option<TargetSector>) -> Result<(), Error> {
    if roots.is_empty() {
        return Ok(());
    }

    let ext_uri = services.uri_identity_service.ext_uri();
    let folders = services.context_service.get_workspace().folders();
    let mut target_index: Option<usize> = None;
    let mut source_indices: Vec<usize> = Vec::new();
    let mut workspace_creation_data: Vec<WorkspaceFolderCreationData> = Vec::new();
    let mut roots_to_move: Vec<WorkspaceFolderCreationData> = Vec::new();

    for (index, folder) in folders.iter().enumerate() {
        let data = WorkspaceFolderCreationData { uri: folder.uri.clone(), name: Some(folder.name.clone()) };

        // Is current target
        if ext_uri.is_equal(Some(&folder.uri), Some(&target.resource()), false) {
            target_index = Some(index);
        }

        // Is current source
        if roots.iter().any(|root| ext_uri.is_equal(Some(&folder.uri), Some(&root.resource()), false)) {
            source_indices.push(index);
        }

        if roots.iter().all(|r| r.resource().to_string(false) != folder.uri.to_string(false)) {
            workspace_creation_data.push(data);
        } else {
            roots_to_move.push(data);
        }
    }

    let target_index = match target_index {
        None => workspace_creation_data.len(),
        Some(mut target_index) => {
            if matches!(target_sector, Some(TargetSector::Bottom | TargetSector::CenterBottom)) {
                target_index += 1;
            }
            // Adjust target index if source was located before target.
            // The move will cause the index to change
            for source_index in &source_indices {
                if *source_index < target_index {
                    target_index -= 1;
                }
            }
            target_index
        }
    };

    let at = target_index.min(workspace_creation_data.len());
    workspace_creation_data.splice(at..at, roots_to_move);

    let count = workspace_creation_data.len() as f64;
    services.workspace_editing_service.update_folders(0.0, Some(count), Some(workspace_creation_data), None).await
}

/// `doHandleExplorerDropOnCopy`.
async fn do_handle_explorer_drop_on_copy(services: &FileDragAndDropServices, sources: &[Rc<ExplorerItem>], target: &Rc<ExplorerItem>) -> Result<(), Error> {
    // Reuse duplicate action when user copies
    let incremental_naming = IncrementalNamingSetting::from_setting(setting_text(&explorer_setting(services.configuration_service.as_ref(), "incrementalNaming")).as_deref());
    let confirm_undo = setting_text(&explorer_setting(services.configuration_service.as_ref(), "confirmUndo"));
    let allow_overwrite = incremental_naming == IncrementalNamingSetting::Disabled;

    let mut resource_file_edits: Vec<ResourceFileEdit> = Vec::new();
    for source in sources {
        let resource = source.resource();
        let new_resource = find_valid_paste_file_target(
            services.explorer_service.clone(),
            services.file_service.clone(),
            services.dialog_service.clone(),
            target.clone(),
            FileToPaste { resource: PasteResource::Uri(resource.clone()), is_directory: Some(source.is_directory()), allow_overwrite },
            incremental_naming,
        )
        .await?;
        let Some(new_resource) = new_resource else { continue };

        resource_file_edits.push(ResourceFileEdit::new(
            Some(resource),
            Some(new_resource),
            Some(WorkspaceFileEditOptions { copy: Some(true), overwrite: Some(allow_overwrite), ..WorkspaceFileEditOptions::default() }),
            None,
        ));
    }

    let label_suffix = get_file_or_folder_label_suffix(sources);
    services
        .explorer_service
        .apply_bulk_edit(
            resource_file_edits.clone(),
            ExplorerBulkEditOptions {
                confirm_before_undo: Some(UndoConfirmLevel::Default.is(confirm_undo.as_deref()) || UndoConfirmLevel::Verbose.is(confirm_undo.as_deref())),
                undo_label: localize("copy", "Copy {0}", &[&label_suffix]),
                progress_label: localize("copying", "Copying {0}", &[&label_suffix]),
                progress_location: None,
            },
        )
        .await?;

    for edit in &resource_file_edits {
        let Some(new_resource) = &edit.new_resource else { continue };
        let is_file = services.explorer_service.find_closest(new_resource).is_some_and(|item| !item.is_directory());
        if !is_file {
            continue;
        }

        let mut options = TextEditorOptions::default();
        options.pinned = Some(true);
        services.editor_service.open_editor(UntypedEditorInput { resource: Some(new_resource.clone()), options: Some(options), ..UntypedEditorInput::default() }).await?;
    }

    Ok(())
}

/// `doHandleExplorerDropOnMove`.
async fn do_handle_explorer_drop_on_move(services: &FileDragAndDropServices, sources: &[Rc<ExplorerItem>], target: &Rc<ExplorerItem>) -> Result<(), Error> {
    // Do not allow moving readonly items
    let resource_file_edits: Vec<ResourceFileEdit> = sources
        .iter()
        .filter(|source| !source.is_readonly().is_readonly())
        .map(|source| ResourceFileEdit::new(Some(source.resource()), Some(join_path(&target.resource(), &[source.name().as_str()])), None, None))
        .collect();
    let label_suffix = get_file_or_folder_label_suffix(sources);
    let confirm_undo = setting_text(&explorer_setting(services.configuration_service.as_ref(), "confirmUndo"));
    let options = ExplorerBulkEditOptions {
        confirm_before_undo: Some(UndoConfirmLevel::Verbose.is(confirm_undo.as_deref())),
        undo_label: localize("move", "Move {0}", &[&label_suffix]),
        progress_label: localize("moving", "Moving {0}", &[&label_suffix]),
        progress_location: None,
    };

    let error = match services.explorer_service.apply_bulk_edit(resource_file_edits.clone(), options.clone()).await {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };

    // Any other error: bubble up
    let is_conflict = FileOperationError::from_error(&error).is_some_and(|e| e.file_operation_result == FileOperationResult::FileMoveConflict);
    if !is_conflict {
        return Err(error);
    }

    // Conflict
    let mut overwrites: Vec<URI> = Vec::new();
    for edit in &resource_file_edits {
        if let Some(new_resource) = &edit.new_resource {
            if services.file_service.exists(new_resource).await? {
                overwrites.push(new_resource.clone());
            }
        }
    }
    if overwrites.is_empty() {
        return Err(error);
    }

    // Move with overwrite if the user confirms
    let confirm = get_multiple_files_overwrite_confirm(&overwrites);
    let confirmed = services.dialog_service.confirm(confirm).await?.confirmed;
    if confirmed {
        let with_overwrite: Vec<ResourceFileEdit> = resource_file_edits
            .iter()
            .map(|edit| ResourceFileEdit::new(edit.old_resource.clone(), edit.new_resource.clone(), Some(WorkspaceFileEditOptions { overwrite: Some(true), ..WorkspaceFileEditOptions::default() }), None))
            .collect();
        services.explorer_service.apply_bulk_edit(with_overwrite, options).await?;
    }

    Ok(())
}
