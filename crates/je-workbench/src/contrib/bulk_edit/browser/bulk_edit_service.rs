//! Porte de `src/vs/workbench/contrib/bulkEdit/browser/bulkEditService.ts`: o `BulkEditService` (a implementação do
//! `IBulkEditService` do workbench) e o `BulkEdit` que ele cria para cada chamada, mais o registro da configuração
//! `files.refactoring.autoSave`.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados dos construtores são o `Deps` do [`Injectable`], na ordem do original; o
//!   `BulkEdit` é criado por `createInstance` com os [`BulkEditArgs`].
//! - **`IBulkEditService.apply(edit: ResourceEdit[] | WorkspaceEdit, options?)`.** O contrato do porte (o trait de
//!   `je_editor::code_action::platform`) recebe o `WorkspaceEdit`; a forma `ResourceEdit[]` é
//!   [`BulkEditService::apply_resource_edits`], e o `apply` do trait apenas converte os itens e a chama.
//! - **`liftEdits`.** `ResourceTextEdit.is/lift`, `ResourceFileEdit.is/lift`, `ResourceNotebookCellEdit.is/lift` e
//!   `ResourceAttachmentEdit.is/lift` viram o `match` sobre o item do `WorkspaceEdit`; um [`WorkspaceEditItem::Custom`] é
//!   a [`ResourceEdit::Custom`], e a [`ResourceNotebookCellEdit`] (a edição de célula de notebook) se reconhece dentre elas pelo
//!   [`ICustomEdit::as_any`]: o `BulkEdit` a aplica pelo [`BulkCellEdits`], e as demais pelo `OpaqueEdits`. O `throw new
//!   Error('Unsupported edit')` não tem caso aqui.
//! - **`Object.getPrototypeOf(a) === Object.getPrototypeOf(b)`.** É a igualdade do discriminante de [`ResourceEdit`], com a
//!   [`ResourceNotebookCellEdit`] separada das demais edições customizadas ([`same_prototype`]).
//! - **`this._logEdits`.** O `JSON.stringify(edits)` é um resumo em JSON com os recursos de cada edição (as edições do
//!   porte não têm serialização completa).
//! - **`options.undoRedoGroupId`.** É o `id` (`u64`) do [`UndoRedoGroup`]; `undoRedoSource` é um `Rc`.
//! - **Estado compartilhado.** O serviço guarda um `Weak` de si (`Rc::new_cyclic`), porque o corpo `async` do `apply`
//!   precisa de um dono `'static`; o `_previewHandler` e a lista de grupos ativos moram em `Rc`.
//! - **`ILifecycleService.onBeforeShutdown`.** O `e.veto(promessa, id)` recebe a promessa do `_shouldVeto`.

use std::cell::RefCell;
use std::mem::discriminant;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::linked_list::LinkedList;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::map::{ResourceMap, ResourceSet};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::bulk_edit_service::{BulkEditPreviewHandler, ResourceEdit, ResourceFileEdit, ResourceTextEdit};
use je_editor::code_action::languages::{ICustomEdit, WorkspaceEdit, WorkspaceEditItem};
use je_editor::code_action::platform::{BulkEditOptions, BulkEditResult, I_BULK_EDIT_SERVICE, IBulkEditService};
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use je_editor::config::editor_options::registry::READ_ONLY;
use je_editor::text_model_edit_source::TextModelEditSource;
use je_editor::undo_redo::{UndoRedoGroup, UndoRedoSource};
use je_instantiation::{InstantiationType, Injectable, InstantiationService, register_singleton, service_impl};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::dialogs::{BaseDialogOptions, Confirmation, DialogDetail, IDialogService};
use je_platform::log::{LogArg, LogLevel, LogMessage, LogService, Logger, can_log};
use je_platform::progress::progress::{IProgress, Progress, ProgressStep};

use super::bulk_cell_edits::{BulkCellEdits, BulkCellEditsArgs, ResourceNotebookCellEdit};
use super::bulk_file_edits::{BulkFileEdits, BulkFileEditsArgs};
use super::bulk_text_edits::{BulkTextEdits, BulkTextEditsArgs};
use super::opaque_edits::{OpaqueEdits, OpaqueEditsArgs};
use crate::services::editor::editor_service::{ActiveTextEditorControl, IEditorService};
use crate::services::lifecycle::lifecycle::{ILifecycleService, ShutdownReason};
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `autoSaveSetting`.
const AUTO_SAVE_SETTING: &str = "files.refactoring.autoSave";

/// `liftEdits(edits)`.
fn lift_edits(edits: &[WorkspaceEditItem]) -> Vec<ResourceEdit> {
    edits
        .iter()
        .map(|edit| match edit {
            WorkspaceEditItem::Text(edit) => ResourceEdit::Text(ResourceTextEdit::lift(edit.clone())),
            WorkspaceEditItem::File(edit) => ResourceEdit::File(ResourceFileEdit::lift(edit.clone())),
            WorkspaceEditItem::Custom(edit) => ResourceEdit::Custom(edit.clone()),
        })
        .collect()
}

/// `Object.getPrototypeOf(a) === Object.getPrototypeOf(b)`: as edições de texto e de arquivo têm o discriminante da variante, e as
/// [`ResourceEdit::Custom`] se dividem entre a [`ResourceNotebookCellEdit`] e as demais (`ResourceAttachmentEdit`).
fn same_prototype(a: &ResourceEdit, b: &ResourceEdit) -> bool {
    match (a, b) {
        (ResourceEdit::Custom(a), ResourceEdit::Custom(b)) => ResourceNotebookCellEdit::is(&**a) == ResourceNotebookCellEdit::is(&**b),
        _ => discriminant(a) == discriminant(b),
    }
}

/// O resumo em JSON de uma edição, para o `_logEdits`.
fn describe_edit(edit: &ResourceEdit) -> String {
    let uri = |uri: &Option<URI>| uri.as_ref().map_or("null".to_string(), |uri| format!("\"{}\"", uri.to_string(false)));
    match edit {
        ResourceEdit::Text(edit) => format!("{{\"resource\":\"{}\",\"versionId\":{}}}", edit.resource.to_string(false), edit.version_id.map_or("null".to_string(), |id| id.to_string())),
        ResourceEdit::File(edit) => format!("{{\"oldResource\":{},\"newResource\":{}}}", uri(&edit.old_resource), uri(&edit.new_resource)),
        ResourceEdit::Custom(edit) => format!("{{\"resource\":\"{}\"}}", edit.resource().to_string(false)),
    }
}

/// `{ report: _ => this._progress.report({ increment: 100 / this._edits.length }) }`.
struct IncrementProgress {
    progress: Rc<dyn IProgress<ProgressStep>>,
    increment: f64,
}

impl IProgress<()> for IncrementProgress {
    fn report(&self, _item: ()) {
        self.progress.report(ProgressStep { message: None, increment: Some(self.increment), total: None });
    }
}

/// Os argumentos estáticos do construtor de `BulkEdit`, na ordem do original.
pub struct BulkEditArgs {
    pub label: Option<String>,
    pub code: Option<String>,
    pub editor: Option<Rc<CodeEditorWidget>>,
    pub progress: Rc<dyn IProgress<ProgressStep>>,
    pub token: CancellationToken,
    pub edits: Vec<ResourceEdit>,
    pub undo_redo_group: UndoRedoGroup,
    pub undo_redo_source: Option<Rc<UndoRedoSource>>,
    pub confirm_before_undo: bool,
}

/// `BulkEdit`.
pub struct BulkEdit {
    args: BulkEditArgs,
    instantiation_service: Rc<InstantiationService>,
    log_service: Rc<dyn LogService>,
}

impl Injectable for BulkEdit {
    type Args = BulkEditArgs;
    type Deps = (Rc<InstantiationService>, Rc<dyn LogService>);

    fn create(args: Self::Args, (instantiation_service, log_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { args, instantiation_service, log_service })
    }
}

impl BulkEdit {
    /// `ariaMessage()`.
    pub fn aria_message(&self) -> String {
        let mut other_resources: ResourceMap<bool> = ResourceMap::new();
        let mut text_edit_resources: ResourceMap<bool> = ResourceMap::new();
        let mut text_edit_count = 0;
        for edit in &self.args.edits {
            match edit {
                ResourceEdit::Text(edit) => {
                    text_edit_count += 1;
                    text_edit_resources.set(edit.resource.clone(), true);
                }
                ResourceEdit::File(edit) => {
                    if let Some(resource) = edit.old_resource.as_ref().or(edit.new_resource.as_ref()) {
                        other_resources.set(resource.clone(), true);
                    }
                }
                ResourceEdit::Custom(_) => {}
            }
        }
        if self.args.edits.is_empty() {
            localize("summary.0", "Made no edits", &[])
        } else if other_resources.size() == 0 {
            if text_edit_count > 1 && text_edit_resources.size() > 1 {
                localize("summary.nm", "Made {0} text edits in {1} files", &[&text_edit_count.to_string(), &text_edit_resources.size().to_string()])
            } else {
                localize("summary.n0", "Made {0} text edits in one file", &[&text_edit_count.to_string()])
            }
        } else {
            localize(
                "summary.textFiles",
                "Made {0} text edits in {1} files, also created or deleted {2} files",
                &[&text_edit_count.to_string(), &text_edit_resources.size().to_string(), &other_resources.size().to_string()],
            )
        }
    }

    /// `this._label || localize('workspaceEdit', "Workspace Edit")`.
    fn label(&self) -> String {
        self.args.label.clone().filter(|label| !label.is_empty()).unwrap_or_else(|| localize("workspaceEdit", "Workspace Edit", &[]))
    }

    /// `this._code || 'undoredo.workspaceEdit'`.
    fn code(&self) -> String {
        self.args.code.clone().filter(|code| !code.is_empty()).unwrap_or_else(|| "undoredo.workspaceEdit".to_string())
    }

    /// `perform(reason?)`: `this` é o `Rc` do objeto criado por `createInstance`.
    pub fn perform(self: &Rc<Self>, reason: Option<TextModelEditSource>) -> Promise<Vec<URI>> {
        let this = self.clone();
        async_fn(async move { this.run(reason).await })
    }

    async fn run(&self, reason: Option<TextModelEditSource>) -> Result<Vec<URI>, Error> {
        let edits = &self.args.edits;
        if edits.is_empty() {
            return Ok(Vec::new());
        }

        let mut ranges: Vec<usize> = vec![1];
        for i in 1..edits.len() {
            if same_prototype(&edits[i - 1], &edits[i]) {
                if let Some(last) = ranges.last_mut() {
                    *last += 1;
                }
            } else {
                ranges.push(1);
            }
        }

        // Show infinte progress when there is only 1 item since we do not know how long it takes
        let increment = if edits.len() > 1 { Some(0.0) } else { None };
        self.args.progress.report(ProgressStep { message: None, increment, total: Some(100.0) });
        // Increment by percentage points since progress API expects that
        let progress: Rc<dyn IProgress<()>> = Rc::new(IncrementProgress { progress: self.args.progress.clone(), increment: 100.0 / edits.len() as f64 });

        let mut resources: Vec<Vec<URI>> = Vec::new();
        let mut index = 0;
        for range in ranges {
            if self.args.token.is_cancellation_requested() {
                break;
            }
            let group = &edits[index..index + range];
            match &group[0] {
                ResourceEdit::File(_) => {
                    self.log_edits("_performFileEdits", group);
                    let edits = group.iter().filter_map(|edit| if let ResourceEdit::File(edit) = edit { Some(edit.clone()) } else { None }).collect();
                    resources.push(self.perform_file_edits(edits, progress.clone()).await?);
                }
                ResourceEdit::Text(_) => {
                    self.log_edits("_performTextEdits", group);
                    let edits = group.iter().filter_map(|edit| if let ResourceEdit::Text(edit) = edit { Some(edit.clone()) } else { None }).collect();
                    resources.push(self.perform_text_edits(edits, progress.clone(), reason.clone()).await?);
                }
                ResourceEdit::Custom(custom) if ResourceNotebookCellEdit::is(&**custom) => {
                    self.log_edits("_performCellEdits", group);
                    let edits = group.iter().filter_map(|edit| if let ResourceEdit::Custom(edit) = edit { ResourceNotebookCellEdit::downcast(&**edit).cloned() } else { None }).collect();
                    resources.push(self.perform_cell_edits(edits, progress.clone()).await?);
                }
                ResourceEdit::Custom(_) => {
                    self.log_edits("_performOpaqueEdits", group);
                    let edits = group.iter().filter_map(|edit| if let ResourceEdit::Custom(edit) = edit { Some(edit.clone()) } else { None }).collect();
                    resources.push(self.perform_opaque_edits(edits, progress.clone()).await?);
                }
            }
            index += range;
        }

        Ok(resources.into_iter().flatten().collect())
    }

    /// `_logEdits(operation, edits)`: avoid serializing (potentially very many) edits when debug logging is off.
    fn log_edits(&self, operation: &str, edits: &[ResourceEdit]) {
        if can_log(self.log_service.get_level(), LogLevel::Debug) {
            let described = format!("[{}]", edits.iter().map(describe_edit).collect::<Vec<_>>().join(","));
            self.log_service.debug_args(operation, &[LogArg::from(described.as_str())]);
        }
    }

    /// `_performFileEdits(edits, undoRedoGroup, undoRedoSource, confirmBeforeUndo, progress)`.
    async fn perform_file_edits(&self, edits: Vec<ResourceFileEdit>, progress: Rc<dyn IProgress<()>>) -> Result<Vec<URI>, Error> {
        let model = self.instantiation_service.create_instance::<BulkFileEdits>(BulkFileEditsArgs {
            label: self.label(),
            code: self.code(),
            undo_redo_group: self.args.undo_redo_group.clone(),
            undo_redo_source: self.args.undo_redo_source.clone(),
            confirm_before_undo: self.args.confirm_before_undo,
            progress,
            token: self.args.token.clone(),
            edits,
        });
        model.apply().await
    }

    /// `_performTextEdits(edits, undoRedoGroup, undoRedoSource, progress, reason)`.
    async fn perform_text_edits(&self, edits: Vec<ResourceTextEdit>, progress: Rc<dyn IProgress<()>>, reason: Option<TextModelEditSource>) -> Result<Vec<URI>, Error> {
        let model = self.instantiation_service.create_instance::<BulkTextEdits>(BulkTextEditsArgs {
            label: self.label(),
            code: self.code(),
            editor: self.args.editor.clone(),
            undo_redo_group: self.args.undo_redo_group.clone(),
            undo_redo_source: self.args.undo_redo_source.clone(),
            progress,
            token: self.args.token.clone(),
            edits,
        });
        model.apply(reason).await
    }

    /// `_performCellEdits(edits, undoRedoGroup, undoRedoSource, progress)`.
    async fn perform_cell_edits(&self, edits: Vec<ResourceNotebookCellEdit>, progress: Rc<dyn IProgress<()>>) -> Result<Vec<URI>, Error> {
        let model = self.instantiation_service.create_instance::<BulkCellEdits>(BulkCellEditsArgs {
            undo_redo_group: self.args.undo_redo_group.clone(),
            undo_redo_source: self.args.undo_redo_source.clone(),
            progress,
            token: self.args.token.clone(),
            edits,
        });
        model.apply().await
    }

    /// `_performOpaqueEdits(edits, undoRedoGroup, undoRedoSource, progress)`.
    async fn perform_opaque_edits(&self, edits: Vec<Rc<dyn ICustomEdit>>, progress: Rc<dyn IProgress<()>>) -> Result<Vec<URI>, Error> {
        let model = self.instantiation_service.create_instance::<OpaqueEdits>(OpaqueEditsArgs {
            undo_redo_group: self.args.undo_redo_group.clone(),
            undo_redo_source: self.args.undo_redo_source.clone(),
            progress,
            token: self.args.token.clone(),
            edits,
        });
        model.apply().await
    }
}

/// `BulkEditService`.
pub struct BulkEditService {
    this: Weak<BulkEditService>,
    instantiation_service: Rc<InstantiationService>,
    log_service: Rc<dyn LogService>,
    editor_service: Rc<dyn IEditorService>,
    lifecycle_service: Rc<dyn ILifecycleService>,
    dialog_service: Rc<dyn IDialogService>,
    working_copy_service: Rc<dyn IWorkingCopyService>,
    config_service: Rc<dyn IConfigurationService>,
    active_undo_redo_groups: LinkedList<UndoRedoGroup>,
    preview_handler: Rc<RefCell<Option<BulkEditPreviewHandler>>>,
}

impl Injectable for BulkEditService {
    type Args = ();
    type Deps = (
        Rc<InstantiationService>,
        Rc<dyn LogService>,
        Rc<dyn IEditorService>,
        Rc<dyn ILifecycleService>,
        Rc<dyn IDialogService>,
        Rc<dyn IWorkingCopyService>,
        Rc<dyn IConfigurationService>,
    );

    fn create(
        _: (),
        (instantiation_service, log_service, editor_service, lifecycle_service, dialog_service, working_copy_service, config_service): Self::Deps,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            instantiation_service,
            log_service,
            editor_service,
            lifecycle_service,
            dialog_service,
            working_copy_service,
            config_service,
            active_undo_redo_groups: LinkedList::default(),
            preview_handler: Rc::default(),
        })
    }
}

service_impl!(BulkEditService => dyn IBulkEditService);

impl BulkEditService {
    /// `apply(editsIn: ResourceEdit[], options?)`.
    pub fn apply_resource_edits(&self, edits: Vec<ResourceEdit>, options: BulkEditOptions) -> Promise<BulkEditResult> {
        let this = self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda");
        async_fn(async move { this.apply_impl(edits, options).await })
    }

    async fn apply_impl(&self, edits: Vec<ResourceEdit>, options: BulkEditOptions) -> Result<BulkEditResult, Error> {
        let mut edits = edits;

        if edits.is_empty() {
            return Ok(BulkEditResult { aria_summary: localize("nothing", "Made no edits", &[]), is_applied: false });
        }

        let preview_handler = self.preview_handler.borrow().clone();
        if let Some(preview_handler) = preview_handler
            && (options.show_preview.unwrap_or(false) || edits.iter().any(|value| value.metadata().is_some_and(|metadata| metadata.needs_confirmation)))
        {
            edits = preview_handler(edits, Some(options.clone())).await?;
        }

        let mut code_editor = options.editor.clone();
        // try to find code editor
        if code_editor.is_none() {
            code_editor = match self.editor_service.active_text_editor_control() {
                Some(ActiveTextEditorControl::Code(candidate)) => candidate.code_editor_widget(),
                Some(ActiveTextEditorControl::Diff { modified, .. }) => modified.code_editor_widget(),
                None => None,
            };
        }

        if code_editor.as_ref().is_some_and(|editor| *editor.get_option(READ_ONLY)) {
            // If the code editor is readonly still allow bulk edits to be applied #68549
            code_editor = None;
        }

        // undo-redo-group: if a group id is passed then try to find it
        // in the list of active edits. otherwise (or when not found)
        // create a separate undo-redo-group
        let mut undo_redo_group: Option<UndoRedoGroup> = None;
        if let Some(undo_redo_group_id) = options.undo_redo_group_id {
            undo_redo_group = self.active_undo_redo_groups.iter().find(|candidate| candidate.id == undo_redo_group_id);
        }
        let mut undo_redo_group_remove = None;
        let undo_redo_group = match undo_redo_group {
            Some(group) => group,
            None => {
                let group = UndoRedoGroup::new();
                undo_redo_group_remove = Some(self.active_undo_redo_groups.push(group.clone()));
                group
            }
        };

        let label = options.quotable_label.clone().filter(|label| !label.is_empty()).or_else(|| options.label.clone());
        let edits_count = edits.len();
        let bulk_edit = self.instantiation_service.create_instance::<BulkEdit>(BulkEditArgs {
            label: label.clone(),
            code: options.code.clone(),
            editor: code_editor,
            progress: options.progress.clone().unwrap_or_else(Progress::<ProgressStep>::none),
            token: options.token.clone().unwrap_or(CancellationToken::None),
            edits,
            undo_redo_group,
            undo_redo_source: options.undo_redo_source.clone(),
            confirm_before_undo: options.confirm_before_undo.unwrap_or(false),
        });

        let listener: DisposableRef = self.lifecycle_service.on_before_shutdown().listen({
            let (dialog_service, label) = (self.dialog_service.clone(), label.clone());
            move |event| event.veto(MaybePromise::Promise(Self::should_veto(dialog_service.clone(), label.clone(), event.reason)), "veto.blukEditService")
        });
        let outcome = self.perform_and_save(&bulk_edit, &options, edits_count).await;
        if let Err(err) = &outcome {
            self.log_service.error_args(&LogMessage::Error(err.clone()), &[]);
        }
        listener.dispose();
        if let Some(remove) = undo_redo_group_remove {
            remove.remove();
        }
        outcome
    }

    /// O corpo do `try` de `apply`: executa a edição e salva os arquivos envolvidos quando a configuração pede.
    async fn perform_and_save(&self, bulk_edit: &Rc<BulkEdit>, options: &BulkEditOptions, edits_count: usize) -> Result<BulkEditResult, Error> {
        let resources = bulk_edit.perform(options.reason.clone()).await?;

        // when enabled (option AND setting) loop over all dirty working copies and trigger save
        // for those that were involved in this bulk edit operation.
        if options.respect_auto_save_config.unwrap_or(false)
            && matches!(self.config_service.get_value(Some(AUTO_SAVE_SETTING), None), Dynamic::Bool(true))
            && resources.len() > 1
        {
            self.save_all(resources).await?;
        }

        Ok(BulkEditResult { aria_summary: bulk_edit.aria_message(), is_applied: edits_count > 0 })
    }

    /// `_saveAll(resources)`.
    fn save_all(&self, resources: Vec<URI>) -> Promise<()> {
        let set = Rc::new(ResourceSet::from_entries(resources, None));
        let saves: Vec<Promise<()>> = self
            .working_copy_service
            .dirty_working_copies()
            .into_iter()
            .map(|copy| {
                let set = set.clone();
                async_fn(async move {
                    if set.has(&copy.resource()) {
                        copy.save(None).await?;
                    }
                    Ok(())
                })
            })
            .collect();

        let log_service = self.log_service.clone();
        async_fn(async move {
            for item in Promise::all_settled(saves).await? {
                if let Err(err) = item {
                    log_service.warn_args(&LogMessage::Error(err), &[]);
                }
            }
            Ok(())
        })
    }

    /// `_shouldVeto(label, reason)`.
    fn should_veto(dialog_service: Rc<dyn IDialogService>, label: Option<String>, reason: ShutdownReason) -> Promise<bool> {
        async_fn(async move {
            let message = match reason {
                ShutdownReason::Close => localize("closeTheWindow.message", "Are you sure you want to close the window?", &[]),
                ShutdownReason::Load => localize("changeWorkspace.message", "Are you sure you want to change the workspace?", &[]),
                ShutdownReason::Reload => localize("reloadTheWindow.message", "Are you sure you want to reload the window?", &[]),
                _ if Platform::detect().is_macintosh => localize("quitMessageMac", "Are you sure you want to quit?", &[]),
                _ => localize("quitMessage", "Are you sure you want to exit?", &[]),
            };

            let detail = label.filter(|label| !label.is_empty()).unwrap_or_else(|| localize("fileOperation", "File operation", &[]));
            let mut base = BaseDialogOptions::new(message);
            base.detail = Some(DialogDetail::Text(localize("areYouSureQuiteBulkEdit.detail", "'{0}' is in progress.", &[&detail])));
            let result = dialog_service.confirm(Confirmation { base, primary_button: None, cancel_button: None }).await?;

            Ok(!result.confirmed)
        })
    }
}

impl IBulkEditService for BulkEditService {
    fn apply(&self, edit: &WorkspaceEdit, options: BulkEditOptions) -> Promise<BulkEditResult> {
        self.apply_resource_edits(lift_edits(&edit.edits), options)
    }

    fn has_preview_handler(&self) -> bool {
        self.preview_handler.borrow().is_some()
    }

    fn set_preview_handler(&self, handler: BulkEditPreviewHandler) -> DisposableRef {
        *self.preview_handler.borrow_mut() = Some(handler.clone());
        let preview_handler = self.preview_handler.clone();
        to_disposable(move || {
            let is_current = preview_handler.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &handler));
            if is_current {
                *preview_handler.borrow_mut() = None;
            }
        })
    }
}

/// `registerSingleton(IBulkEditService, BulkEditService, InstantiationType.Delayed)` e o
/// `registerConfiguration` do fim do arquivo original (a configuração `files.refactoring.autoSave`).
pub fn register() {
    register_singleton::<BulkEditService>(I_BULK_EDIT_SERVICE, InstantiationType::Delayed);

    configuration_registry().register_configuration(ConfigurationNode::new(
        ConfigurationNodeData { id: Some("files".to_owned()), ..Default::default() }.with_properties([(
            AUTO_SAVE_SETTING,
            PropertySchema::from_entries([
                ("description", Dynamic::string(&localize("refactoring.autoSave", "Controls if files that were part of a refactoring are saved automatically", &[]))),
                ("default", Dynamic::Bool(true)),
                ("type", Dynamic::string("boolean")),
            ]),
        )]),
    ));
}
