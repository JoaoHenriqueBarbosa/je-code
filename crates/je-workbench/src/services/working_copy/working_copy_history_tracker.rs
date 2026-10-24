//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyHistoryTracker.ts`: o `WorkingCopyHistoryTracker`, que
//! grava uma entrada no histórico local quando uma working copy de arquivo armazenado é salva (respeitando
//! `workbench.localHistory.*`) e move as entradas quando o arquivo é movido ou renomeado.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados são argumentos do construtor, na ordem do original. `@IUndoRedoService` é o
//!   [`UndoRedoService`] concreto de `je-editor` (o porte não tem o trait) e `@IFileService` é o contrato
//!   [`IFileService`] do porte.
//! - **`IWorkbenchContribution`.** O registro `registerWorkbenchContribution(WorkingCopyHistoryTracker, Restored)` fica para a
//!   montagem do workbench; a struct só guarda o `Disposable`.
//! - **`isStoredFileWorkingCopySaveEvent`.** O `stat` do evento é o [`WorkingCopySaveEvent::stat`], preenchido pelo
//!   `WorkingCopyService` só para as working copies de arquivo armazenado (ver `IWorkingCopy::stored_file_save_event`); sem ele,
//!   o evento é ignorado, que é o `false` do original.
//! - **`this` nos ouvintes.** Os ouvintes capturam um `Weak` do rastreador (`Rc::new_cyclic`).
//! - **`ResourceMap` e `Map<URI, number>`.** O `ResourceMap` do porte não devolve referência mutável: cada leitura clona o valor e
//!   cada atualização regrava a entrada.
//! - **`undefined`.** `historyEntryContentVersion.get(resource) === contentVersion` é a comparação de `Option<u64>` com `Some`.
//! - **Funções `async` sem espera.** `onDidRunFileOperation` (um ouvinte `async`) roda como [`async_fn`] cuja promessa é
//!   descartada, como o JavaScript descarta a promessa do ouvinte.
//! - **`localize`.** A chave e a mensagem são as do original; o `localize` do porte devolve a mensagem em inglês.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{GlobalIdleValue, Limiter, async_fn};
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::map::{ResourceMap, ResourceMapKeyFn};
use je_base::network::schemas;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_editor::undo_redo_service::{IUndoRedoService, UndoRedoService};
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileOperation, FileOperationEvent, FileStatWithMetadata};
use je_platform::files::i_file_service::IFileService;
use je_platform::path::path_service::IPathService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;

use super::working_copy_history::{IWorkingCopyHistoryService, MAX_PARALLEL_HISTORY_IO_OPS, WorkingCopyHistoryEntryDescriptor};
use super::working_copy_history_service::{configuration_number, register_save_source};
use super::working_copy_service::{IWorkingCopyService, WorkingCopySaveEvent};
use crate::common::editor::SaveSource;
use crate::common::resources::{GetExpression, ResourceGlobMatcher, ShouldUpdate};

const ENABLED_SETTING: &str = "workbench.localHistory.enabled";
const SIZE_LIMIT_SETTING: &str = "workbench.localHistory.maxFileSize";
const EXCLUDES_SETTING: &str = "workbench.localHistory.exclude";

/// `WorkingCopyHistoryTracker.UNDO_REDO_SAVE_SOURCE`.
fn undo_redo_save_source() -> SaveSource {
    register_save_source("undoRedo.source", "Undo / Redo")
}

/// `resource => this.uriIdentityService.extUri.getComparisonKey(resource)`.
fn comparison_key_fn(ext_uri: &ExtUri) -> ResourceMapKeyFn {
    let ext_uri = ext_uri.clone();

    Rc::new(move |resource: &URI| ext_uri.get_comparison_key(resource, false))
}

struct TrackerState {
    pending_add_history_entry_operations: ResourceMap<CancellationTokenSource>,
    working_copy_content_version: ResourceMap<u64>,
    history_entry_content_version: ResourceMap<u64>,
}

/// `WorkingCopyHistoryTracker`.
pub struct WorkingCopyHistoryTracker {
    base: Rc<Disposable>,
    limiter: Limiter<()>,
    resource_exclude_matcher: Rc<GlobalIdleValue<Rc<ResourceGlobMatcher>>>,
    state: RefCell<TrackerState>,
    working_copy_history_service: Rc<dyn IWorkingCopyHistoryService>,
    path_service: Rc<dyn IPathService>,
    configuration_service: Rc<dyn IConfigurationService>,
    undo_redo_service: Rc<UndoRedoService>,
}

impl WorkingCopyHistoryTracker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        working_copy_service: Rc<dyn IWorkingCopyService>,
        working_copy_history_service: Rc<dyn IWorkingCopyHistoryService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        path_service: Rc<dyn IPathService>,
        configuration_service: Rc<dyn IConfigurationService>,
        undo_redo_service: Rc<UndoRedoService>,
        context_service: Rc<dyn IWorkspaceContextService>,
        file_service: Rc<dyn IFileService>,
    ) -> Rc<Self> {
        let base = Disposable::new();

        let limiter = Limiter::new(MAX_PARALLEL_HISTORY_IO_OPS);
        base.register(Rc::new(limiter.clone()));

        let resource_exclude_matcher = Self::create_resource_exclude_matcher(&base, &configuration_service, &context_service);

        let ext_uri = uri_identity_service.ext_uri();
        let state = TrackerState {
            pending_add_history_entry_operations: ResourceMap::with_key_fn(comparison_key_fn(&ext_uri)),
            working_copy_content_version: ResourceMap::with_key_fn(comparison_key_fn(&ext_uri)),
            history_entry_content_version: ResourceMap::with_key_fn(comparison_key_fn(&ext_uri)),
        };

        undo_redo_save_source();

        let this = Rc::new(Self {
            base,
            limiter,
            resource_exclude_matcher,
            state: RefCell::new(state),
            working_copy_history_service,
            path_service,
            configuration_service,
            undo_redo_service,
        });
        this.register_listeners(&working_copy_service, &file_service);

        this
    }

    /// `this._register(new GlobalIdleValue(() => this._register(new ResourceGlobMatcher(...))))`.
    fn create_resource_exclude_matcher(
        base: &Rc<Disposable>,
        configuration_service: &Rc<dyn IConfigurationService>,
        context_service: &Rc<dyn IWorkspaceContextService>,
    ) -> Rc<GlobalIdleValue<Rc<ResourceGlobMatcher>>> {
        let (idle_base, idle_configuration, idle_context) = (base.clone(), configuration_service.clone(), context_service.clone());
        let idle_value = Rc::new(GlobalIdleValue::new(move || {
            let expression_configuration = idle_configuration.clone();
            let get_expression: GetExpression = Rc::new(move |root: Option<&URI>| {
                let overrides = ConfigurationOverrides { resource: root.cloned(), ..ConfigurationOverrides::default() };
                let expression = expression_configuration.get_value(Some(EXCLUDES_SETTING), Some(&overrides));
                expression.is_truthy().then_some(expression)
            });
            let should_update: ShouldUpdate = Rc::new(move |event| event.affects_configuration(EXCLUDES_SETTING, None));

            let matcher = ResourceGlobMatcher::new(get_expression, should_update, idle_context, idle_configuration);
            idle_base.register(matcher.clone());

            matcher
        }));

        let registered = idle_value.clone();
        base.register(to_disposable(move || registered.dispose()));

        idle_value
    }

    fn register_listeners(self: &Rc<Self>, working_copy_service: &Rc<dyn IWorkingCopyService>, file_service: &Rc<dyn IFileService>) {
        let weak: Weak<Self> = Rc::downgrade(self);

        // File Events
        let this = weak.clone();
        self.base.register(file_service.on_did_run_operation().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_did_run_file_operation(e);
            }
        }));

        // Working Copy Events
        let this = weak.clone();
        self.base.register(working_copy_service.on_did_change_content().listen(move |working_copy| {
            if let Some(this) = this.upgrade() {
                this.on_did_change_content(&working_copy.resource());
            }
        }));
        self.base.register(working_copy_service.on_did_save().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_did_save(e);
            }
        }));
    }

    fn on_did_run_file_operation(self: &Rc<Self>, e: &FileOperationEvent) {
        let Some(target) = self.target_to_track(e) else {
            return; // return early for working copies we are not interested in
        };

        let source = e.resource.clone();
        let target = target.resource.clone();
        let this = self.clone();

        let _ = async_fn(async move {
            // Move working copy history entries for this file move event
            let resources = this.working_copy_history_service.move_entries(&source, &target).await?;

            // Make sure to track the content version of each entry that
            // was moved in our map. This ensures that a subsequent save
            // without a content change does not add a redundant entry
            // (https://github.com/microsoft/vscode/issues/145881)
            for resource in resources {
                let content_version = this.get_content_version(&resource);
                this.state.borrow_mut().history_entry_content_version.set(resource, content_version);
            }

            Ok(())
        });
    }

    fn on_did_change_content(&self, resource: &URI) {
        // Increment content version ID for resource
        let content_version_id = self.get_content_version(resource);
        self.state.borrow_mut().working_copy_content_version.set(resource.clone(), content_version_id + 1);
    }

    fn get_content_version(&self, resource: &URI) -> u64 {
        self.state.borrow().working_copy_content_version.get(resource).copied().unwrap_or(0)
    }

    fn on_did_save(self: &Rc<Self>, e: &WorkingCopySaveEvent) {
        let resource = e.working_copy.resource();
        let Some(stat) = e.stat.as_ref().filter(|stat| self.should_track_history(&resource, stat)) else {
            return; // return early for working copies we are not interested in
        };

        let content_version = self.get_content_version(&resource);
        if self.state.borrow().history_entry_content_version.get(&resource).copied() == Some(content_version) {
            return; // return early when content version already has associated history entry
        }

        // Cancel any previous operation for this resource
        let previous = self.state.borrow().pending_add_history_entry_operations.get(&resource).cloned();
        if let Some(previous) = previous {
            previous.dispose_with(true);
        }

        // Create new cancellation token support and remember
        let cts = CancellationTokenSource::default();
        self.state.borrow_mut().pending_add_history_entry_operations.set(resource.clone(), cts.clone());

        // Queue new operation to add to history
        let (this, e, mtime) = (self.clone(), e.clone(), stat.mtime());
        let _ = self.limiter.queue(move || {
            async_fn(async move {
                if cts.token().is_cancellation_requested() {
                    return Ok(());
                }

                let resource = e.working_copy.resource();
                let content_version = this.get_content_version(&resource);

                // Figure out source of save operation if not provided already
                let mut source = e.event.source.clone().filter(|source| !source.is_empty());
                if source.is_none() {
                    source = this.resolve_source_from_undo_redo(&resource);
                }

                // Add entry
                this.working_copy_history_service
                    .add_entry(WorkingCopyHistoryEntryDescriptor { resource: resource.clone(), timestamp: Some(mtime), source }, cts.token())
                    .await?;

                // Remember content version as being added to history
                this.state.borrow_mut().history_entry_content_version.set(resource.clone(), content_version);

                if cts.token().is_cancellation_requested() {
                    return Ok(());
                }

                // Finally remove from pending operations
                this.state.borrow_mut().pending_add_history_entry_operations.delete(&resource);

                Ok(())
            })
        });
    }

    fn resolve_source_from_undo_redo(&self, resource: &URI) -> Option<SaveSource> {
        if let Some(last_stack_element) = self.undo_redo_service.get_last_element(resource) {
            if last_stack_element.code() == "undoredo.textBufferEdit" {
                return None; // ignore any unspecific stack element that resulted just from typing
            }

            return Some(last_stack_element.label());
        }

        let all_stack_elements = self.undo_redo_service.get_elements(resource);
        if !all_stack_elements.future.is_empty() || !all_stack_elements.past.is_empty() {
            return Some(undo_redo_save_source());
        }

        None
    }

    /// `shouldTrackHistoryFromFileOperationEvent`: o `stat` do destino quando o evento é um movimento a rastrear.
    fn target_to_track<'a>(&self, e: &'a FileOperationEvent) -> Option<&'a FileStatWithMetadata> {
        if !e.is_operation(FileOperation::Move) {
            return None; // only interested in move operations
        }

        e.target.as_ref().filter(|target| self.should_track_history(&target.resource, target))
    }

    fn should_track_history(&self, resource: &URI, stat: &FileStatWithMetadata) -> bool {
        let scheme = resource.scheme().to_string_lossy();
        if scheme != self.path_service.default_uri_scheme() // track history for all workspace resources
            && scheme != schemas::VSCODE_USER_DATA // track history for all settings
            && scheme != schemas::IN_MEMORY
        // track history for tests that use in-memory
        {
            return false; // do not support unknown resources
        }

        let configured_max_file_size_in_bytes = 1024.0 * configuration_number(self.configuration_service.as_ref(), SIZE_LIMIT_SETTING, Some(resource));
        if stat.size() as f64 > configured_max_file_size_in_bytes {
            return false; // only track files that are not too large
        }

        let overrides = ConfigurationOverrides { resource: Some(resource.clone()), ..ConfigurationOverrides::default() };
        if matches!(self.configuration_service.get_value(Some(ENABLED_SETTING), Some(&overrides)), Dynamic::Bool(false)) {
            return false; // do not track when history is disabled
        }

        // Finally check for exclude setting
        !self.resource_exclude_matcher.value().matches(resource, None)
    }
}

impl IDisposable for WorkingCopyHistoryTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}
