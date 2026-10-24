//! Porte de `src/vs/workbench/contrib/files/browser/explorerService.ts`: o `ExplorerService`.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são o `Deps` do [`Injectable`], na ordem do original; o
//!   `IDecorationsService` não entra (o serviço de decorações e o `ExplorerDecorationsProvider` que o `registerView`
//!   registra ainda não foram portados), então o `registerView` só guarda a vista. Por isso o serviço também ainda não
//!   é registrado como singleton (`registerSingleton(IExplorerService, ExplorerService, ...)` é de
//!   `explorer.contribution.ts`).
//! - **`IBulkEditService.apply`.** O contrato do porte (`je_editor::code_action::platform`) aplica um `WorkspaceEdit`, então o
//!   `applyBulkEdit` converte os `ResourceFileEdit` em `WorkspaceEdit` e passa as opções do original (`undoRedoSource`,
//!   `label`, `code`, `progress`, `token` e `confirmBeforeUndo`); o cancelamento segue como no original (o token é cancelado
//!   pelo `onDidCancel`).
//! - **`IProgressService`.** É o de `je_platform::progress::progress`; o `ProgressLocation` de `ExplorerBulkEditOptions`
//!   (o do `collaborators`) é convertido para ele.
//! - **Eventos e promessas.** Cada ouvinte captura um `Weak` do serviço (`Rc::new_cyclic`). O corpo `async` de cada
//!   método é um [`async_fn`], que roda de forma síncrona até o primeiro `await`, como no JavaScript; o
//!   `Promises.settled` é [`promises::settled`].
//! - **`Map` e `Set` de itens.** O `Map<ExplorerItem, boolean>` e os `Set<ExplorerItem>` dos lotes comparam por
//!   identidade do objeto: são os [`ItemMap`] (ordem de inserção, chave é o ponteiro do `Rc`); o `Map<number, batch>` e
//!   o `Set<batch>` de lotes usam o ponteiro do `Rc<MoveBatch>`.
//! - **Configuração.** `this.config` é o [`Dynamic`] do `getValue('explorer')`, o mesmo objeto que o
//!   `TestConfigurationService` devolve, então uma mudança feita no objeto depois é vista aqui, como no original.
//! - **`Event.any` de dois eventos de tipos diferentes.** O registro de provedor e a mudança de capacidades trazem só o
//!   `scheme` que o ouvinte lê; cada um vira um `Event<String>` por `Event.map` (cujo emissor pertence ao `disposables`
//!   do serviço) antes do `Event.any`.
//! - **`roots`.** O array é o do modelo; [`ExplorerService::roots_mut`] é o `service.roots.push(...)` dos testes.

use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::rc::{Rc, Weak};

use je_base::async_::{RunOnceScheduler, async_fn, promises};
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error, on_unexpected_error};
use je_base::event::any;
use je_base::glob::Sibling;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::map::{OrderedMap, ResourceMap, ResourceMapKeyFn};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_editor::code_action::languages::{IWorkspaceFileEdit, WorkspaceEdit, WorkspaceEditItem};
use je_editor::code_action::platform::{BulkEditOptions, IBulkEditService};
use je_editor::services::clipboard_service::IClipboardService;
use je_editor::undo_redo::UndoRedoSource;
use je_instantiation::{Injectable, service_impl};
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileChangeType, FileChangesEvent, FileOperation, FileOperationEvent, ResolveFileOptions};
use je_platform::files::i_file_service::IFileService;
use je_platform::notification::notification_message::NotificationPart;
use je_platform::progress::progress::{IProgressService, ProgressCancellable, ProgressLocation, ProgressOptions};
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;

use super::browser_files::{EditableData, EditableItem, ExplorerBulkEditOptions, ExplorerReveal, IExplorerService, IExplorerView};
use super::common_files::{LexicographicOptions, SESSIONS_FILES_VIEW_ID, SortOrder, SortOrderConfiguration, VIEW_ID};
use super::explorer_model::{ExplorerItem, ExplorerModel};
use crate::common::editor::EditorInputOrUntyped;
use crate::common::editor::editor_input::EditorInput;
use crate::common::resources::ResourceGlobMatcher;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use crate::services::host::host::IHostService;
use crate::services::textfile::collaborators::{self, IWorkingCopyFileService, WorkingCopyFileEvent};

thread_local! {
    /// `UNDO_REDO_SOURCE`.
    pub static UNDO_REDO_SOURCE: Rc<UndoRedoSource> = Rc::new(UndoRedoSource::new());
}

/// O ponteiro do `Rc`, a identidade do objeto que o `Map` e o `Set` do JavaScript usam como chave.
fn identity<T>(value: &Rc<T>) -> usize {
    Rc::as_ptr(value) as usize
}

/// `Map<ExplorerItem, V>` (e, com `V = ()`, o `Set<ExplorerItem>`) por identidade, na ordem de inserção.
struct ItemMap<V> {
    entries: OrderedMap<usize, (Rc<ExplorerItem>, V)>,
}

impl<V: Clone> ItemMap<V> {
    fn new() -> Self {
        Self { entries: OrderedMap::new() }
    }

    fn set(&mut self, item: &Rc<ExplorerItem>, value: V) {
        self.entries.insert(identity(item), (item.clone(), value));
    }

    fn get(&self, item: &Rc<ExplorerItem>) -> Option<V> {
        self.entries.get(&identity(item)).map(|(_, value)| value.clone())
    }

    fn has(&self, item: &Rc<ExplorerItem>) -> bool {
        self.entries.contains_key(&identity(item))
    }

    fn delete(&mut self, item: &Rc<ExplorerItem>) {
        self.entries.remove(&identity(item));
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    /// `Array.from(map)`: os pares na ordem de inserção.
    fn to_vec(&self) -> Vec<(Rc<ExplorerItem>, V)> {
        self.entries.values().cloned().collect()
    }
}

/// `IExplorerMoveBatch`.
struct MoveBatch {
    targets: RefCell<ResourceMap<URI>>,
    operations: RefCell<Vec<Promise<()>>>,
    refreshes: RefCell<ItemMap<bool>>,
    source_parents: RefCell<ItemMap<()>>,
    target_parents: RefCell<ItemMap<()>>,
    moved_items: RefCell<ItemMap<()>>,
}

/// O `role?: 'source' | 'target'` de `refreshMove`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveRole {
    Source,
    Target,
}

/// `ExplorerService`.
pub struct ExplorerService {
    this: Weak<ExplorerService>,
    file_service: Rc<dyn IFileService>,
    configuration_service: Rc<dyn IConfigurationService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    clipboard_service: Rc<dyn IClipboardService>,
    editor_service: Rc<dyn IEditorService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    bulk_edit_service: Rc<dyn IBulkEditService>,
    progress_service: Rc<dyn IProgressService>,
    files_configuration_service: Rc<dyn IFilesConfigurationService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    working_copy_file_service: Rc<dyn IWorkingCopyFileService>,

    disposables: Rc<DisposableStore>,
    editable: RefCell<Option<EditableItem>>,
    should_refresh_after_editing: Cell<bool>,
    config: RefCell<Dynamic>,
    cut_items: RefCell<Option<Vec<Rc<ExplorerItem>>>>,
    view: RefCell<Option<Rc<dyn IExplorerView>>>,
    model: Rc<ExplorerModel>,
    on_file_changes_scheduler: Rc<RunOnceScheduler>,
    file_change_events: RefCell<Vec<Rc<FileChangesEvent>>>,
    reveal_exclude_matcher: Rc<ResourceGlobMatcher>,
    move_batches: RefCell<OrderedMap<usize, Rc<MoveBatch>>>,
    pending_move_source_batches: RefCell<OrderedMap<usize, Rc<MoveBatch>>>,
}

impl ExplorerService {
    /// `EXPLORER_FILE_CHANGES_REACT_DELAY`: delay in ms to react to file changes to give our internal events a chance to
    /// react first.
    const EXPLORER_FILE_CHANGES_REACT_DELAY: u64 = 500;

    /// A própria instância (o `this` do original).
    fn this(&self) -> Rc<ExplorerService> {
        self.this.upgrade().expect("o ExplorerService já foi liberado")
    }

    /// `this.view`.
    fn view(&self) -> Option<Rc<dyn IExplorerView>> {
        self.view.borrow().clone()
    }

    /// `this.config.sortOrder === SortOrder.Modified`.
    fn sort_order_is_modified(&self) -> bool {
        matches!(self.config.borrow().get_str("sortOrder"), Dynamic::String(sort_order) if sort_order == SortOrder::Modified.as_str())
    }

    /// `this.config.fileNesting.enabled`.
    fn file_nesting_enabled(&self) -> bool {
        self.config.borrow().get_str("fileNesting").get_str("enabled").is_truthy()
    }

    /// O array `roots` do modelo, para quem o altera de fora (`service.roots.push(...)`).
    pub fn roots_mut(&self) -> std::cell::RefMut<'_, Vec<Rc<ExplorerItem>>> {
        self.model.roots_mut()
    }

    /// `refresh(reveal = true)`.
    pub fn refresh(&self, reveal: bool) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            // Do not refresh the tree when it is showing temporary nodes (phantom elements)
            if this.view().is_some_and(|view| view.has_phantom_elements()) {
                return Ok(());
            }

            for root in this.model.roots() {
                root.forget_children();
            }
            if let Some(view) = this.view() {
                view.refresh(true, None, None).await?;
                let resource = match this.editor_service.active_editor() {
                    Some(EditorInputOrUntyped::Typed(editor)) => editor.resource(),
                    _ => None,
                };
                let auto_reveal = this.configuration_service.get_value(None, None).get_str("explorer").get_str("autoReveal");

                if let (true, Some(resource), true) = (reveal, resource, auto_reveal.is_truthy()) {
                    let reveal_mode = match &auto_reveal {
                        Dynamic::String(mode) => ExplorerReveal::Mode(mode.to_string_lossy()),
                        other => ExplorerReveal::Flag(other.is_truthy()),
                    };
                    // We did a top level refresh, reveal the active file #67118
                    drop(this.select(&resource, Some(reveal_mode)));
                }
            }
            Ok(())
        })
    }

    /// `new RunOnceScheduler(async () => { ... }, EXPLORER_FILE_CHANGES_REACT_DELAY)`: o corpo do agendador.
    fn react_to_file_changes(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let events = std::mem::take(&mut *this.file_change_events.borrow_mut());

            // Filter to the ones we care
            let mut types = vec![FileChangeType::Deleted];
            if this.sort_order_is_modified() {
                types.push(FileChangeType::Updated);
            }

            let mut should_refresh = false;
            // For DELETED and UPDATED events go through the explorer model and check if any of the items got affected
            for root in this.model.roots() {
                if let Some(view) = this.view()
                    && !should_refresh
                {
                    should_refresh = does_file_event_affect(&root, &view, &events, &types);
                }
            }
            // For ADDED events we need to go through all the events and check if the explorer is already aware of some of them
            // Or if they affect not yet resolved parts of the explorer. If that is the case we will not refresh.
            let extension = ext_uri();
            for event in &events {
                if should_refresh {
                    continue;
                }
                for resource in &event.raw_added {
                    // A root folder that was previously deleted got restored, its parent is not part of the model #134144
                    let identity_ext_uri = this.uri_identity_service.ext_uri();
                    if this.model.roots().iter().any(|root| identity_ext_uri.is_equal(Some(&root.resource()), Some(resource), false)) {
                        should_refresh = true;
                        break;
                    }

                    let parent = this.model.find_closest(&extension.dirname(resource));
                    // Parent of the added resource is resolved and the explorer model is not aware of the added resource - we need to refresh
                    if let Some(parent) = parent
                        && parent.get_child(&extension.basename(resource, None)).is_none()
                    {
                        should_refresh = true;
                        break;
                    }
                }
            }

            if should_refresh {
                this.refresh(false).await?;
            }
            Ok(())
        })
    }

    // File events

    /// `finishMoveBatch(correlationId)`.
    fn finish_move_batch(&self, correlation_id: usize) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let batch = this.move_batches.borrow().get(&correlation_id).cloned();
            let Some(batch) = batch else {
                return Ok(());
            };
            this.move_batches.borrow_mut().remove(&correlation_id);
            let result: Result<(), Error> = async {
                // File service events are synchronous, but their model/view handlers can still be running.
                let operations = batch.operations.borrow().clone();
                promises::settled(operations).await?;
                // Reconcile every source before destinations, including parents that serve both roles.
                let sources = {
                    let (this, batch) = (this.clone(), batch.clone());
                    let sources = this.refresh_move_sources(&batch);
                    sources.finally(move || {
                        this.pending_move_source_batches.borrow_mut().remove(&identity(&batch));
                    })
                };
                let refresh_targets = {
                    let (this, batch) = (this.clone(), batch.clone());
                    move |_| this.refresh_move_targets(&batch)
                };
                promises::settled(vec![sources.clone(), sources.then_result_promise(refresh_targets)]).await?;
                Ok(())
            }
            .await;
            this.pending_move_source_batches.borrow_mut().remove(&identity(&batch));
            this.clear_move_batch(&batch);
            result
        })
    }

    /// O `refreshTargets` de `finishMoveBatch`: `Promises.settled(Array.from(batch.refreshes).filter(([item]) =>
    /// batch.targetParents.has(item)).map(([item, recursive]) => this.refreshMoveParent(item, recursive)))`.
    fn refresh_move_targets(&self, batch: &Rc<MoveBatch>) -> Promise<()> {
        let this = self.this();
        let batch = batch.clone();
        async_fn(async move {
            let targets: Vec<(Rc<ExplorerItem>, bool)> =
                batch.refreshes.borrow().to_vec().into_iter().filter(|(item, _)| batch.target_parents.borrow().has(item)).collect();
            promises::settled(targets.into_iter().map(|(item, recursive)| this.refresh_move_parent(item, recursive)).collect()).await?;
            Ok(())
        })
    }

    /// `refreshMoveParent(item, recursive)`.
    fn refresh_move_parent(&self, item: Rc<ExplorerItem>, recursive: bool) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if !this.disposables.is_disposed()
                && let Some(view) = this.view()
            {
                view.refresh(recursive, Some(&item), None).await?;
            }
            Ok(())
        })
    }

    /// `refreshMoveSources(batch)`.
    fn refresh_move_sources(&self, batch: &Rc<MoveBatch>) -> Promise<()> {
        let this = self.this();
        let batch = batch.clone();
        async_fn(async move {
            let sources: Vec<(Rc<ExplorerItem>, bool)> = batch
                .refreshes
                .borrow()
                .to_vec()
                .into_iter()
                .filter(|(item, _)| batch.source_parents.borrow().has(item) || !batch.target_parents.borrow().has(item))
                .collect();
            for (item, _) in &sources {
                batch.source_parents.borrow_mut().delete(item);
                if !batch.target_parents.borrow().has(item) {
                    batch.refreshes.borrow_mut().delete(item);
                }
            }
            promises::settled(sources.into_iter().map(|(item, recursive)| this.refresh_move_parent(item, recursive)).collect()).await?;
            Ok(())
        })
    }

    /// `refreshInterleavedMoveSources(items, batches)`.
    fn refresh_interleaved_move_sources(&self, items: &[Rc<ExplorerItem>], batches: &[Rc<MoveBatch>]) -> Option<Promise<()>> {
        let affected: Vec<Rc<MoveBatch>> = self
            .pending_move_source_batches
            .borrow()
            .values()
            .filter(|batch| {
                !batches.iter().any(|candidate| Rc::ptr_eq(candidate, batch))
                    && batch.moved_items.borrow().to_vec().iter().any(|(moved, _)| items.iter().any(|item| is_equal_or_ancestor_item(item, moved)))
            })
            .cloned()
            .collect();
        if affected.is_empty() {
            return None;
        }
        // A moved item may now be owned by another parent. Remove its stale source rows before that parent is refreshed.
        let refreshes = affected.iter().map(|batch| self.refresh_move_sources(batch)).collect();
        Some(promises::settled(refreshes).then_result(|result| {
            if let Err(error) = result {
                on_unexpected_error(&error);
            }
            Ok(())
        }))
    }

    /// `clearMoveBatch(batch)`.
    fn clear_move_batch(&self, batch: &Rc<MoveBatch>) {
        batch.targets.borrow_mut().clear();
        batch.operations.borrow_mut().clear();
        batch.refreshes.borrow_mut().clear();
        batch.source_parents.borrow_mut().clear();
        batch.target_parents.borrow_mut().clear();
        batch.moved_items.borrow_mut().clear();
    }

    /// `refreshMove(recursive, item, batches, role?)`.
    fn refresh_move(&self, recursive: bool, item: Option<Rc<ExplorerItem>>, batches: Vec<Rc<MoveBatch>>, role: Option<MoveRole>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if this.disposables.is_disposed() {
                return Ok(());
            }
            match item {
                Some(item) if !batches.is_empty() => {
                    for batch in &batches {
                        let previous = batch.refreshes.borrow().get(&item);
                        batch.refreshes.borrow_mut().set(&item, recursive || previous == Some(true));
                        match role {
                            Some(MoveRole::Source) => batch.source_parents.borrow_mut().set(&item, ()),
                            Some(MoveRole::Target) => batch.target_parents.borrow_mut().set(&item, ()),
                            None => {}
                        }
                    }
                }
                item => {
                    if let Some(view) = this.view() {
                        view.refresh(recursive, item.as_ref(), None).await?;
                    }
                }
            }
            Ok(())
        })
    }

    /// `onDidRunOperation(e, batches)`.
    fn on_did_run_operation(&self, e: FileOperationEvent, batches: Vec<Rc<MoveBatch>>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            // When nesting, changes to one file in a folder may impact the rendered structure
            // of all the folder's immediate children, thus a recursive refresh is needed.
            // Ideally the tree would be able to recusively refresh just one level but that does not yet exist.
            let should_deep_refresh = this.file_nesting_enabled();
            let extension = ext_uri();

            // Add
            if e.is_operation(FileOperation::Create) || e.is_operation(FileOperation::Copy) {
                let added_element = event_target(&e);
                let parent_resource = extension.dirname(&added_element.resource);
                let parents = this.model.find_all(&parent_resource);

                if !parents.is_empty() {
                    // Add the new file to its parent (Model)
                    let additions = parents
                        .into_iter()
                        .map(|parent| {
                            let (this, added_element) = (this.clone(), added_element.clone());
                            async_fn(async move {
                                // We have to check if the parent is resolved #29177
                                let resolve_metadata = this.sort_order_is_modified();
                                if !parent.is_directory_resolved() {
                                    let options = ResolveFileOptions { resolve_to: Vec::new(), resolve_single_child_descendants: false, resolve_metadata };
                                    let stat = this.file_service.resolve(&parent.resource(), Some(options)).await?;
                                    let model_stat = ExplorerItem::create(
                                        &this.file_service,
                                        &this.configuration_service,
                                        &this.files_configuration_service,
                                        &stat,
                                        parent.parent().as_ref(),
                                        None,
                                    );
                                    ExplorerItem::merge_local_with_disk(&model_stat, &parent);
                                }

                                let child_element = ExplorerItem::create(
                                    &this.file_service,
                                    &this.configuration_service,
                                    &this.files_configuration_service,
                                    &added_element,
                                    parent.parent().as_ref(),
                                    None,
                                );
                                // Make sure to remove any previous version of the file if any
                                parent.remove_child(&child_element);
                                parent.add_child(&child_element);
                                // Refresh the Parent (View)
                                if let Some(view) = this.view() {
                                    view.refresh(should_deep_refresh, Some(&parent), None).await?;
                                }
                                Ok(())
                            })
                        })
                        .collect();
                    Promise::all(additions).await?;
                }
            }
            // Move (including Rename)
            else if e.is_operation(FileOperation::Move) {
                let old_resource = e.resource.clone();
                let new_element = event_target(&e);
                let old_parent_resource = extension.dirname(&old_resource);
                let new_parent_resource = extension.dirname(&new_element.resource);
                let model_elements = this.model.find_all(&old_resource);
                let same_parent_move = model_elements.iter().all(|element| element.nested_parent().is_none())
                    && this.uri_identity_service.ext_uri().is_equal(Some(&old_parent_resource), Some(&new_parent_resource), false);
                let interleaved_sources = this.refresh_interleaved_move_sources(&model_elements, &batches);
                let track_moved_items = || {
                    for batch in &batches {
                        for model_element in &model_elements {
                            batch.moved_items.borrow_mut().set(model_element, ());
                        }
                    }
                };

                // Handle Rename
                if same_parent_move {
                    for model_element in &model_elements {
                        model_element.rename(&new_element.name, Some(new_element.mtime() as f64));
                    }
                    track_moved_items();
                    if let Some(interleaved_sources) = interleaved_sources {
                        interleaved_sources.await?;
                    }
                    let renames = model_elements.iter().map(|element| this.refresh_move(should_deep_refresh, element.parent(), batches.clone(), None)).collect();
                    Promise::all(renames).await?;
                }
                // Handle Move
                else {
                    let new_parents = this.model.find_all(&new_parent_resource);
                    if !new_parents.is_empty() && !model_elements.is_empty() {
                        // Move in Model
                        let moves: Vec<(Option<Rc<ExplorerItem>>, Option<Rc<ExplorerItem>>, Rc<ExplorerItem>)> = model_elements
                            .iter()
                            .enumerate()
                            .map(|(index, model_element)| {
                                let old_parent = model_element.parent();
                                let old_nested_parent = model_element.nested_parent();
                                model_element.move_to(&new_parents[index]);
                                (old_parent, old_nested_parent, new_parents[index].clone())
                            })
                            .collect();
                        track_moved_items();
                        if let Some(interleaved_sources) = interleaved_sources {
                            interleaved_sources.await?;
                        }
                        let refreshes = moves
                            .into_iter()
                            .map(|(old_parent, old_nested_parent, new_parent)| {
                                let (this, batches) = (this.clone(), batches.clone());
                                async_fn(async move {
                                    if let Some(old_nested_parent) = old_nested_parent {
                                        this.refresh_move(false, Some(old_nested_parent), batches.clone(), Some(MoveRole::Source)).await?;
                                    }
                                    this.refresh_move(false, old_parent, batches.clone(), Some(MoveRole::Source)).await?;
                                    this.refresh_move(should_deep_refresh, Some(new_parent), batches, Some(MoveRole::Target)).await?;
                                    Ok(())
                                })
                            })
                            .collect();
                        Promise::all(refreshes).await?;
                    } else if let Some(interleaved_sources) = interleaved_sources {
                        interleaved_sources.await?;
                    }
                }
            }
            // Delete
            else if e.is_operation(FileOperation::Delete) {
                let model_elements = this.model.find_all(&e.resource);
                let deletions = model_elements
                    .into_iter()
                    .map(|model_element| {
                        let this = this.clone();
                        async_fn(async move {
                            if let Some(parent) = model_element.parent() {
                                // Remove Element from Parent (Model)
                                parent.remove_child(&model_element);
                                if let Some(view) = this.view() {
                                    view.focus_next();
                                }

                                if let Some(old_nested_parent) = model_element.nested_parent() {
                                    old_nested_parent.remove_child(&model_element);
                                    if let Some(view) = this.view() {
                                        view.refresh(false, Some(&old_nested_parent), None).await?;
                                    }
                                }
                                // Refresh Parent (View)
                                if let Some(view) = this.view() {
                                    view.refresh(should_deep_refresh, Some(&parent), None).await?;
                                }

                                if let Some(view) = this.view()
                                    && view.get_focus().is_empty()
                                {
                                    view.focus_last();
                                }
                            }
                            Ok(())
                        })
                    })
                    .collect();
                Promise::all(deletions).await?;
            }
            Ok(())
        })
    }

    /// Check if an item matches a explorer.autoRevealExclude pattern.
    fn should_auto_reveal_item(&self, item: Option<&Rc<ExplorerItem>>, ignore: bool) -> bool {
        let Some(item) = item else {
            return true;
        };
        if ignore {
            return true;
        }
        let has_sibling: &dyn Fn(&str) -> Sibling = &|name| Sibling::Ready(item.parent().is_some_and(|parent| parent.get_child(name).is_some()));
        if self.reveal_exclude_matcher.matches(&item.resource(), Some(has_sibling)) {
            return false;
        }
        let root = item.root();
        let mut current_item = item.parent();
        loop {
            match current_item {
                Some(current) if Rc::ptr_eq(&current, &root) => return true,
                None => return true,
                Some(current) => {
                    if self.reveal_exclude_matcher.matches(&current.resource(), None) {
                        return false;
                    }
                    current_item = current.parent();
                }
            }
        }
    }

    /// `onConfigurationUpdated(event)`.
    fn on_configuration_updated(&self, event: &ConfigurationChangeEvent) -> Promise<()> {
        let this = self.this();
        if !event.affects_configuration("explorer", None) {
            return Promise::resolved(());
        }

        let mut should_refresh = false;

        if event.affects_configuration("explorer.fileNesting", None) {
            should_refresh = true;
        }

        let configuration = self.configuration_service.get_value(None, None);
        let explorer = optional_property(&configuration, "explorer");

        let config_sort_order = truthy_or(optional_property(&explorer, "sortOrder"), Dynamic::string(SortOrder::Default.as_str()));
        let current = self.config.borrow().clone();
        if !current.get_str("sortOrder").strict_equals(&config_sort_order) {
            should_refresh = !current.get_str("sortOrder").is_undefined();
        }

        let config_lexicographic_options =
            truthy_or(optional_property(&explorer, "sortOrderLexicographicOptions"), Dynamic::string(LexicographicOptions::Default.as_str()));
        if !current.get_str("sortOrderLexicographicOptions").strict_equals(&config_lexicographic_options) {
            should_refresh = should_refresh || !current.get_str("sortOrderLexicographicOptions").is_undefined();
        }
        let sort_order_reverse = truthy_or(optional_property(&explorer, "sortOrderReverse"), Dynamic::Bool(false));

        if !current.get_str("sortOrderReverse").strict_equals(&sort_order_reverse) {
            should_refresh = should_refresh || !current.get_str("sortOrderReverse").is_undefined();
        }

        *self.config.borrow_mut() = configuration.get_str("explorer");

        if should_refresh {
            return this.refresh(true);
        }
        Promise::resolved(())
    }

    /// Os ouvintes do construtor.
    fn register_listeners(&self) {
        let weak = self.this.clone();
        let disposables = &self.disposables;

        disposables.add(self.working_copy_file_service.on_will_run_working_copy_file_operation().listen({
            let weak = weak.clone();
            move |e: &WorkingCopyFileEvent| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if e.operation == FileOperation::Move && e.files.len() > 1 {
                    let uri_identity_service = this.uri_identity_service.clone();
                    let key_fn: ResourceMapKeyFn = Rc::new(move |resource: &URI| uri_identity_service.ext_uri().get_comparison_key(resource, false));
                    let mut targets = ResourceMap::with_key_fn(key_fn);
                    for file in &e.files {
                        if let Some(source) = &file.source {
                            targets.set(source.clone(), file.target.clone());
                        }
                    }
                    let batch = Rc::new(MoveBatch {
                        targets: RefCell::new(targets),
                        operations: RefCell::new(Vec::new()),
                        refreshes: RefCell::new(ItemMap::new()),
                        source_parents: RefCell::new(ItemMap::new()),
                        target_parents: RefCell::new(ItemMap::new()),
                        moved_items: RefCell::new(ItemMap::new()),
                    });
                    this.move_batches.borrow_mut().insert(e.correlation_id, batch.clone());
                    this.pending_move_source_batches.borrow_mut().insert(identity(&batch), batch);
                }
            }
        }));
        disposables.add(
            any(vec![
                self.working_copy_file_service.on_did_run_working_copy_file_operation(),
                self.working_copy_file_service.on_did_fail_working_copy_file_operation(),
            ])
            .listen({
                let weak = weak.clone();
                move |e: &WorkingCopyFileEvent| {
                    if let Some(this) = weak.upgrade() {
                        drop(this.finish_move_batch(e.correlation_id).catch(|error| on_unexpected_error(&error)));
                    }
                }
            }),
        );
        disposables.add(self.file_service.on_did_run_operation().listen({
            let weak = weak.clone();
            move |e: &FileOperationEvent| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let mut batches: Vec<Rc<MoveBatch>> = Vec::new();
                if e.is_operation(FileOperation::Move) {
                    let extension = this.uri_identity_service.ext_uri();
                    let target = event_target(e);
                    for batch in this.move_batches.borrow().values() {
                        if extension.is_equal(batch.targets.borrow().get(&e.resource), Some(&target.resource), false) {
                            batches.push(batch.clone());
                        }
                    }
                }
                let operation = this.on_did_run_operation(e.clone(), batches.clone()).catch(|error| on_unexpected_error(&error));
                for batch in &batches {
                    batch.operations.borrow_mut().push(operation.clone());
                }
            }
        }));

        disposables.add(self.file_service.on_did_files_change().listen({
            let weak = weak.clone();
            move |e: &Rc<FileChangesEvent>| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                this.file_change_events.borrow_mut().push(e.clone());
                // Don't mess with the file tree while in the process of editing. #112293
                if this.editable.borrow().is_some() {
                    return;
                }
                if !this.on_file_changes_scheduler.is_scheduled() {
                    this.on_file_changes_scheduler.schedule(None);
                }
            }
        }));
        disposables.add(self.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |e: &Rc<ConfigurationChangeEvent>| {
                if let Some(this) = weak.upgrade() {
                    drop(this.on_configuration_updated(e));
                }
            }
        }));
        disposables.add(
            any(vec![
                self.file_service.on_did_change_file_system_provider_registrations().map(|e| e.scheme.clone(), Some(self.disposables.as_ref())),
                self.file_service.on_did_change_file_system_provider_capabilities().map(|e| e.scheme.clone(), Some(self.disposables.as_ref())),
            ])
            .listen({
                let weak = weak.clone();
                move |scheme: &String| {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    let scheme = scheme.clone();
                    drop(async_fn(async move {
                        let mut affected = false;
                        for root in this.model.roots() {
                            if root.resource().scheme() == scheme.as_str() {
                                affected = true;
                                root.forget_children();
                            }
                        }
                        if affected && let Some(view) = this.view() {
                            view.set_tree_input().await?;
                        }
                        Ok(())
                    }));
                }
            }),
        );
        disposables.add(self.model.on_did_change_roots().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade()
                    && let Some(view) = this.view()
                {
                    drop(view.set_tree_input());
                }
            }
        }));
    }
}

impl Injectable for ExplorerService {
    type Args = ();
    type Deps = (
        Rc<dyn IFileService>,
        Rc<dyn IConfigurationService>,
        Rc<dyn IWorkspaceContextService>,
        Rc<dyn IClipboardService>,
        Rc<dyn IEditorService>,
        Rc<dyn IUriIdentityService>,
        Rc<dyn IBulkEditService>,
        Rc<dyn IProgressService>,
        Rc<dyn IHostService>,
        Rc<dyn IFilesConfigurationService>,
        Rc<dyn IWorkbenchEnvironmentService>,
        Rc<dyn IWorkingCopyFileService>,
    );

    fn create(
        _: (),
        (
            file_service,
            configuration_service,
            context_service,
            clipboard_service,
            editor_service,
            uri_identity_service,
            bulk_edit_service,
            progress_service,
            host_service,
            files_configuration_service,
            environment_service,
            working_copy_file_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let config = configuration_service.get_value(Some("explorer"), None);

        let disposables = DisposableStore::new();
        let model = ExplorerModel::new(
            context_service.clone(),
            uri_identity_service.clone(),
            file_service.clone(),
            configuration_service.clone(),
            files_configuration_service.clone(),
        );
        disposables.add_rc(model.clone());

        let reveal_exclude_matcher = ResourceGlobMatcher::new(
            {
                let configuration_service = configuration_service.clone();
                Rc::new(move |uri: Option<&URI>| {
                    let overrides = ConfigurationOverrides { override_identifier: None, resource: uri.cloned() };
                    Some(get_reveal_excludes(&configuration_service.get_value(None, Some(&overrides))))
                })
            },
            Rc::new(|event: &ConfigurationChangeEvent| event.affects_configuration("explorer.autoRevealExclude", None)),
            context_service.clone(),
            configuration_service.clone(),
        );

        let this = Rc::new_cyclic(|weak: &Weak<ExplorerService>| {
            let on_file_changes_scheduler = RunOnceScheduler::new(
                {
                    let weak = weak.clone();
                    move || {
                        if let Some(this) = weak.upgrade() {
                            drop(this.react_to_file_changes());
                        }
                    }
                },
                Self::EXPLORER_FILE_CHANGES_REACT_DELAY,
            );
            Self {
                this: weak.clone(),
                file_service,
                configuration_service,
                context_service,
                clipboard_service,
                editor_service,
                uri_identity_service,
                bulk_edit_service,
                progress_service,
                files_configuration_service,
                environment_service,
                working_copy_file_service,
                disposables,
                editable: RefCell::new(None),
                should_refresh_after_editing: Cell::new(false),
                config: RefCell::new(config),
                cut_items: RefCell::new(None),
                view: RefCell::new(None),
                model,
                on_file_changes_scheduler,
                file_change_events: RefCell::new(Vec::new()),
                reveal_exclude_matcher,
                move_batches: RefCell::new(OrderedMap::new()),
                pending_move_source_batches: RefCell::new(OrderedMap::new()),
            }
        });

        this.disposables.add_rc(this.on_file_changes_scheduler.clone());
        this.disposables.add_rc(this.reveal_exclude_matcher.clone());
        this.register_listeners();

        // Refresh explorer when window gets focus to compensate for missing file events #126817
        let weak = this.this.clone();
        this.disposables.add(host_service.on_did_change_focus().listen(move |has_focus: &bool| {
            if !*has_focus {
                return;
            }
            let Some(this) = weak.upgrade() else {
                return;
            };
            if this.editable.borrow().is_some() {
                this.should_refresh_after_editing.set(true);
            } else {
                drop(this.refresh(false));
            }
        }));

        this
    }
}

service_impl!(ExplorerService => dyn IExplorerService, disposable);

impl IExplorerService for ExplorerService {
    fn roots(&self) -> Vec<Rc<ExplorerItem>> {
        self.model.roots()
    }

    fn sort_order_configuration(&self) -> SortOrderConfiguration {
        let config = self.config.borrow();
        let text = |name: &str| match config.get_str(name) {
            Dynamic::String(value) => Some(value.to_string_lossy()),
            _ => None,
        };
        SortOrderConfiguration {
            sort_order: SortOrder::from_setting(text("sortOrder").as_deref()),
            lexicographic_options: LexicographicOptions::from_setting(text("sortOrderLexicographicOptions").as_deref()),
            reverse: config.get_str("sortOrderReverse").is_truthy(),
        }
    }

    fn register_view(&self, context_provider: Rc<dyn IExplorerView>) {
        *self.view.borrow_mut() = Some(context_provider);
    }

    fn get_view_id(&self) -> String {
        (if self.environment_service.is_sessions_window() { SESSIONS_FILES_VIEW_ID } else { VIEW_ID }).to_string()
    }

    fn get_context(&self, respect_multi_selection: bool, ignore_nested_children: Option<bool>) -> Vec<Rc<ExplorerItem>> {
        let Some(view) = self.view() else {
            return Vec::new();
        };
        let ignore_nested_children = ignore_nested_children.unwrap_or(false);

        let mut items: Vec<Rc<ExplorerItem>> = Vec::new();
        for item in view.get_context(respect_multi_selection) {
            add_unique(&mut items, item);
        }
        // `Set#forEach` also visits the entries added while it runs.
        let mut index = 0;
        while index < items.len() {
            let item = items[index].clone();
            index += 1;
            // We will error out trying to resolve collapsed nodes that have not yet been resolved.
            // So we catch and ignore them in the multiSelect context
            if respect_multi_selection
                && !ignore_nested_children
                && let Ok(true) = view.is_item_collapsed(&item)
                && let Some(nested_children) = item.nested_children()
            {
                for child in nested_children {
                    add_unique(&mut items, child);
                }
            }
        }

        items
    }

    fn apply_bulk_edit(&self, edit: Vec<ResourceFileEdit>, options: ExplorerBulkEditOptions) -> Promise<()> {
        let cancellation_token_source = CancellationTokenSource::new(None);
        let location = match options.progress_location.unwrap_or(collaborators::ProgressLocation::Window) {
            collaborators::ProgressLocation::Explorer => ProgressLocation::Explorer,
            collaborators::ProgressLocation::Scm => ProgressLocation::Scm,
            collaborators::ProgressLocation::Extensions => ProgressLocation::Extensions,
            collaborators::ProgressLocation::Window => ProgressLocation::Window,
            collaborators::ProgressLocation::Notification => ProgressLocation::Notification,
            collaborators::ProgressLocation::Dialog => ProgressLocation::Dialog,
        };
        let mut progress_options = ProgressOptions::new(location);
        progress_options.title = Some(NotificationPart::Text(options.progress_label.clone()));
        progress_options.cancellable = Some(ProgressCancellable::Flag(edit.len() > 1));
        if location != ProgressLocation::Window {
            progress_options.delay = Some(500.0);
        }

        let workspace_edit = WorkspaceEdit { edits: edit.into_iter().map(|edit| WorkspaceEditItem::File(IWorkspaceFileEdit::from(edit))).collect() };
        let bulk_edit_service = self.bulk_edit_service.clone();
        let undo_label = options.undo_label.clone();
        let confirm_before_undo = options.confirm_before_undo;
        let token = cancellation_token_source.token();
        let promise = self.progress_service.with_progress(
            progress_options,
            Box::new(move |progress| {
                async_fn(async move {
                    let bulk_options = BulkEditOptions {
                        undo_redo_source: Some(UNDO_REDO_SOURCE.with(Rc::clone)),
                        label: Some(undo_label),
                        code: Some("undoredo.explorerOperation".to_string()),
                        progress: Some(progress),
                        token: Some(token),
                        confirm_before_undo,
                        ..BulkEditOptions::default()
                    };
                    bulk_edit_service.apply(&workspace_edit, bulk_options).await?;
                    Ok(())
                })
            }),
            Some({
                let cancellation_token_source = cancellation_token_source.clone();
                Rc::new(move |_choice: Option<i32>| cancellation_token_source.cancel())
            }),
        );
        let mut explorer_options = ProgressOptions::new(ProgressLocation::Explorer);
        explorer_options.delay = Some(500.0);
        let outer = self.progress_service.with_progress(explorer_options, Box::new(move |_progress| promise), None);
        async_fn(async move {
            outer.await?;
            cancellation_token_source.dispose_with(false);
            Ok(())
        })
    }

    fn has_view_focus(&self) -> bool {
        self.view().is_some_and(|view| view.has_focus())
    }

    // IExplorerService methods

    fn find_closest(&self, resource: &URI) -> Option<Rc<ExplorerItem>> {
        self.model.find_closest(resource)
    }

    fn find_closest_root(&self, resource: &URI) -> Option<Rc<ExplorerItem>> {
        let extension = self.uri_identity_service.ext_uri();
        let mut parent_roots: Vec<Rc<ExplorerItem>> =
            self.model.roots().into_iter().filter(|root| extension.is_equal_or_parent(resource, &root.resource(), false, false)).collect();
        parent_roots.sort_by_key(|root| Reverse(root.resource().path().len()));
        parent_roots.into_iter().next()
    }

    fn set_editable(&self, stat: Rc<ExplorerItem>, data: Option<EditableData>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(view) = this.view() else {
                return Ok(());
            };

            match data {
                None => *this.editable.borrow_mut() = None,
                Some(data) => {
                    *this.editable.borrow_mut() = Some(EditableItem { stat: stat.clone(), data });
                    this.on_file_changes_scheduler.cancel();
                }
            }
            let is_editing = this.is_editable(Some(&stat));
            if view.set_editable(&stat, is_editing).await.is_err() {
                return Ok(());
            }

            if this.editable.borrow().is_none() {
                if this.should_refresh_after_editing.get() {
                    this.should_refresh_after_editing.set(false);
                    this.refresh(false).await?;
                }
                if !this.file_change_events.borrow().is_empty() && !this.on_file_changes_scheduler.is_scheduled() {
                    this.on_file_changes_scheduler.schedule(None);
                }
            }
            Ok(())
        })
    }

    fn set_to_copy(&self, items: Vec<Rc<ExplorerItem>>, cut: bool) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let previously_cut_items = this.cut_items.borrow().clone();
            *this.cut_items.borrow_mut() = if cut { Some(items.clone()) } else { None };
            this.clipboard_service.write_resources(items.iter().map(|item| item.resource()).collect()).await?;

            if let Some(view) = this.view() {
                view.items_copied(&items, cut, previously_cut_items.as_deref());
            }
            Ok(())
        })
    }

    fn is_cut(&self, item: &Rc<ExplorerItem>) -> bool {
        let extension = self.uri_identity_service.ext_uri();
        self.cut_items
            .borrow()
            .as_ref()
            .is_some_and(|cut_items| cut_items.iter().any(|cut| extension.is_equal(Some(&cut.resource()), Some(&item.resource()), false)))
    }

    fn get_editable(&self) -> Option<EditableItem> {
        self.editable.borrow().clone()
    }

    fn get_editable_data(&self, stat: &Rc<ExplorerItem>) -> Option<EditableData> {
        self.editable.borrow().as_ref().filter(|editable| Rc::ptr_eq(&editable.stat, stat)).map(|editable| editable.data.clone())
    }

    fn is_editable(&self, stat: Option<&Rc<ExplorerItem>>) -> bool {
        self.editable.borrow().as_ref().is_some_and(|editable| stat.is_none_or(|stat| Rc::ptr_eq(&editable.stat, stat)))
    }

    fn select(&self, resource: &URI, reveal: Option<ExplorerReveal>) -> Promise<()> {
        let this = self.this();
        let resource = resource.clone();
        async_fn(async move {
            let Some(view) = this.view() else {
                return Ok(());
            };

            // If file or parent matches exclude patterns, do not reveal unless reveal argument is 'force'
            let ignore_reveal_excludes = reveal == Some(ExplorerReveal::Mode("force".to_string()));

            let file_stat = this.find_closest(&resource);
            if let Some(file_stat) = file_stat {
                if !this.should_auto_reveal_item(Some(&file_stat), ignore_reveal_excludes) {
                    return Ok(());
                }
                view.select_resource(Some(&file_stat.resource()), reveal, None).await?;
                return Ok(());
            }

            // Stat needs to be resolved first and then revealed
            let options = ResolveFileOptions { resolve_to: vec![resource.clone()], resolve_single_child_descendants: false, resolve_metadata: this.sort_order_is_modified() };
            let Some(root) = this.find_closest_root(&resource) else {
                return Ok(());
            };

            let attempt: Result<(), Error> = async {
                let stat = this.file_service.resolve(&root.resource(), Some(options.clone())).await?;

                // Convert to model
                let model_stat = ExplorerItem::create(
                    &this.file_service,
                    &this.configuration_service,
                    &this.files_configuration_service,
                    &stat,
                    None,
                    Some(&options.resolve_to),
                );
                // Update Input with disk Stat
                ExplorerItem::merge_local_with_disk(&model_stat, &root);
                let item = root.find(&resource);
                view.refresh(true, Some(&root), None).await?;

                // Once item is resolved, check again if folder should be expanded
                if let Some(item) = &item
                    && !this.should_auto_reveal_item(Some(item), ignore_reveal_excludes)
                {
                    return Ok(());
                }
                let selected = item.as_ref().map(|item| item.resource());
                view.select_resource(selected.as_ref(), reveal.clone(), None).await?;
                Ok(())
            }
            .await;
            if let Err(error) = attempt {
                root.set_error(Some(error));
                view.refresh(false, Some(&root), None).await?;
            }
            Ok(())
        })
    }

    fn refresh(&self) -> Promise<()> {
        ExplorerService::refresh(self, true)
    }
}

impl IDisposable for ExplorerService {
    fn dispose(&self) {
        self.disposables.dispose();
        let batches: Vec<Rc<MoveBatch>> = self.move_batches.borrow().values().cloned().collect();
        for batch in &batches {
            self.clear_move_batch(batch);
        }
        self.move_batches.borrow_mut().clear();
        self.pending_move_source_batches.borrow_mut().clear();
        *self.view.borrow_mut() = None;
    }
}

/// O `e.target` de um evento de operação que o original lê sem checar: `undefined` ali é um `TypeError`.
fn event_target(event: &FileOperationEvent) -> je_platform::files::files::FileStatWithMetadata {
    event
        .target
        .clone()
        .unwrap_or_else(|| errors::throw(Error::type_error("Cannot read properties of undefined (reading 'resource')")))
}

/// Acrescenta o item ao `Set<ExplorerItem>` se ainda não está nele.
fn add_unique(items: &mut Vec<Rc<ExplorerItem>>, item: Rc<ExplorerItem>) {
    if !items.iter().any(|existing| Rc::ptr_eq(existing, &item)) {
        items.push(item);
    }
}

/// `isEqualOrAncestorItem(item, candidate)`.
fn is_equal_or_ancestor_item(item: &Rc<ExplorerItem>, candidate: &Rc<ExplorerItem>) -> bool {
    let mut current = Some(candidate.clone());
    while let Some(candidate) = current {
        if Rc::ptr_eq(&candidate, item) {
            return true;
        }
        current = candidate.parent();
    }
    false
}

/// `doesFileEventAffect(item, view, events, types)`.
fn does_file_event_affect(item: &Rc<ExplorerItem>, view: &Rc<dyn IExplorerView>, events: &[Rc<FileChangesEvent>], types: &[FileChangeType]) -> bool {
    for child in item.children() {
        if view.is_item_visible(&child) {
            if events.iter().any(|event| event.contains(&child.resource(), types)) {
                return true;
            }
            if child.is_directory() && child.is_directory_resolved() && does_file_event_affect(&child, view, events, types) {
                return true;
            }
        }
    }

    false
}

/// `configuration?.explorer?.autoRevealExclude`, ou `{}` quando ausente (`getRevealExcludes`).
fn get_reveal_excludes(configuration: &Dynamic) -> Dynamic {
    let reveal_excludes = optional_property(&optional_property(configuration, "explorer"), "autoRevealExclude");

    if !reveal_excludes.is_truthy() {
        return Dynamic::object(Vec::<(&str, Dynamic)>::new());
    }

    reveal_excludes
}

/// `value?.key`: `undefined` quando o valor é `undefined` ou `null`.
fn optional_property(value: &Dynamic, key: &str) -> Dynamic {
    if value.is_nullish() { Dynamic::Undefined } else { value.get_str(key) }
}

/// `value || fallback`.
fn truthy_or(value: Dynamic, fallback: Dynamic) -> Dynamic {
    if value.is_truthy() { value } else { fallback }
}
