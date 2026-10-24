//! Porte de `src/vs/workbench/contrib/files/test/browser/explorerService.test.ts` (suíte `Files - ExplorerService`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `store`.** O `LeakGuard` no começo de cada caso; o `store.add(x)` do
//!   original é o `guard.add_rc(x)`.
//! - **`workbenchInstantiationService({ fileService, configurationService }, store)`.** O [`TestInstantiationService`] com os
//!   serviços que o `ExplorerService` lê, nas versões de teste do porte: `TestFileService`, o `TestConfigurationService` do
//!   caso, o `TestContextService` (a pasta `testWorkspace`), o `UriIdentityService` real sobre o arquivo de teste,
//!   `TestEditorService`, o `TestClipboardService`, o `TestEnvironmentService`, o `NullFilesConfigurationService` e
//!   [`TestProgressService`]. Os `instantiationService.stub(X, { ... })` com objeto parcial são os dublês que lançam
//!   `TypeError: <membro> is not a function` nos membros que o objeto do original não define ([`not_defined`]).
//! - **`new class extends mock<IExplorerView>() { override ... }`.** É o [`ViewMock`]: cada membro sobrescrito é um fecho, e
//!   os outros lançam `TypeError` como o `mock` do original.
//! - **`async function fire/finish/settleViewWork`.** Funções que devolvem [`Promise`] por `async_fn` (o corpo roda até o
//!   primeiro `await` na hora, como no JavaScript); o corpo dos casos `async` roda em `run_with_faked_timers`.
//! - **`assert.deepStrictEqual({ a, b }, { ... })`.** São `assert_eq!` sobre os campos (ou a tupla deles).
//! - **`setUnexpectedErrorHandler` e o `finally` que o restaura.** O tratador do caso guarda os erros; ao fim, o
//!   `record_unexpected_errors()` reinstala o do guarda, que é o `originalErrorHandler`.
//!
//! Casos que ficaram de fora (dependem de infraestrutura que o porte ainda não tem, e estão no `explorerService.test.ts`):
//! - `preserves real tree lookup and selection after multi-source ... moves (...)` (seis) e `keeps moved items reachable when
//!   another move touches them during a batch (...)` (três): criam a `WorkbenchCompressibleAsyncDataTree` com o
//!   `IListService`, os contextos de chave e o DOM (`append(document.body, $('.explorer-move-test'))`).
//! - `updates Explorer after a bulk edit stops on cancellation` e `... rejects a partial failure`: usam o `BulkFileEdits`
//!   (`contrib/bulkEdit/browser/bulkFileEdits.ts`) e o `WorkingCopyFileService.move/copy`, que não foram portados.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_clipboard_service.rs"]
mod test_clipboard_service;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, record_unexpected_errors, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::promise::{DeferredPromise, Promise};
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange};
use je_editor::code_action::languages::WorkspaceEdit;
use je_editor::code_action::platform::{BulkEditOptions, BulkEditResult, I_BULK_EDIT_SERVICE, IBulkEditService};
use je_editor::services::clipboard_service::I_CLIPBOARD_SERVICE;
use je_instantiation::Injectable;
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::files::{FileChange, FileChangeType, FileChangesEvent, FileOperation, FileOperationEvent};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::progress::progress::{I_PROGRESS_SERVICE, IProgress, IProgressService, Progress, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::resources::{host_environment, join_path};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::workspace::I_WORKSPACE_CONTEXT_SERVICE;
use je_workbench::contrib::files::browser_files::{EditableData, ExplorerReveal, IExplorerService, IExplorerView};
use je_workbench::contrib::files::common_files::{SESSIONS_FILES_VIEW_ID, VIEW_ID};
use je_workbench::contrib::files::explorer_model::{ExplorerItem, NewExplorerItem};
use je_workbench::contrib::files::explorer_service::ExplorerService;
use je_workbench::services::editor::editor_service::I_EDITOR_SERVICE;
use je_workbench::services::environment::environment_service::I_WORKBENCH_ENVIRONMENT_SERVICE;
use je_workbench::services::files_configuration::files_configuration_service::{I_FILES_CONFIGURATION_SERVICE, IFilesConfigurationService};
use je_workbench::services::host::host::{I_HOST_SERVICE, IHostService, ReloadOptions};
use je_workbench::services::textfile::collaborators::{I_WORKING_COPY_FILE_SERVICE, IWorkingCopyFileService, WorkingCopyFileEvent, WorkingCopyFileEventFile};
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEditorService, TestEnvironmentService};
use je_workbench::test::common::workbench_test_services::{FileStatOptions, NullFilesConfigurationService, TestContextService, TestFileService, create_file_stat};
use test_clipboard_service::TestClipboardService;
use test_instantiation_service::TestInstantiationService;

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_defined(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `TestProgressService` do `workbenchInstantiationService`: roda a tarefa na hora, com `Progress.None`.
struct TestProgressService;

impl IProgressService for TestProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let progress: Rc<dyn IProgress<ProgressStep>> = Progress::<ProgressStep>::none();
        task(progress)
    }
}

/// `instantiationService.stub(IBulkEditService, {})`.
struct StubBulkEditService;

impl IBulkEditService for StubBulkEditService {
    fn apply(&self, _edit: &WorkspaceEdit, _options: BulkEditOptions) -> Promise<BulkEditResult> {
        not_defined("apply")
    }
}

/// `instantiationService.stub(IHostService, { onDidChangeFocus: focusChanged.event })`.
struct StubHostService {
    on_did_change_focus: Event<bool>,
}

impl IHostService for StubHostService {
    fn reload(&self, _options: Option<ReloadOptions>) -> Promise<()> {
        not_defined("reload")
    }

    fn restart(&self) -> Promise<()> {
        not_defined("restart")
    }

    fn on_did_change_focus(&self) -> Event<bool> {
        self.on_did_change_focus.clone()
    }
}

/// `instantiationService.stub(IUriIdentityService, { extUri: extUriIgnorePathCase })`.
struct StubUriIdentityService {
    ext_uri: ExtUri,
}

impl IUriIdentityService for StubUriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        self.ext_uri.clone()
    }

    fn as_canonical_uri(&self, _uri: &URI) -> URI {
        not_defined("asCanonicalUri")
    }
}

/// `instantiationService.stub(IWorkingCopyFileService, { onWillRun..., onDidRun..., onDidFail... })`.
struct TestWorkingCopyFileService {
    will_run: Rc<Emitter<WorkingCopyFileEvent>>,
    did_run: Rc<Emitter<WorkingCopyFileEvent>>,
    did_fail: Rc<Emitter<WorkingCopyFileEvent>>,
}

impl IWorkingCopyFileService for TestWorkingCopyFileService {
    fn on_will_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.will_run.event()
    }

    fn on_did_fail_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.did_fail.event()
    }

    fn on_did_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.did_run.event()
    }
}

type RefreshFn = Box<dyn Fn(bool, Option<&Rc<ExplorerItem>>) -> Promise<()>>;

/// `new class extends mock<IExplorerView>() { override ... }`: só os membros sobrescritos existem.
#[derive(Default)]
struct ViewMock {
    id: Option<String>,
    set_editable: Option<Box<dyn Fn(&Rc<ExplorerItem>, bool) -> Promise<()>>>,
    refresh: Option<RefreshFn>,
    has_phantom_elements: Option<Box<dyn Fn() -> bool>>,
    is_item_visible: Option<Box<dyn Fn() -> bool>>,
    focus_next: Option<Box<dyn Fn()>>,
    get_focus: Option<Box<dyn Fn() -> Vec<Rc<ExplorerItem>>>>,
}

impl IExplorerView for ViewMock {
    fn id(&self) -> String {
        self.id.clone().unwrap_or_else(|| not_defined("id"))
    }

    fn auto_reveal(&self) -> ExplorerReveal {
        not_defined("autoReveal")
    }

    fn set_auto_reveal(&self, _value: ExplorerReveal) {
        not_defined("autoReveal")
    }

    fn get_context(&self, _respect_multi_selection: bool) -> Vec<Rc<ExplorerItem>> {
        not_defined("getContext")
    }

    fn refresh(&self, recursive: bool, item: Option<&Rc<ExplorerItem>>, _cancel_editing: Option<bool>) -> Promise<()> {
        match &self.refresh {
            Some(refresh) => refresh(recursive, item),
            None => not_defined("refresh"),
        }
    }

    fn select_resource(&self, _resource: Option<&URI>, _reveal: Option<ExplorerReveal>, _retry: Option<usize>) -> Promise<()> {
        not_defined("selectResource")
    }

    fn set_tree_input(&self) -> Promise<()> {
        not_defined("setTreeInput")
    }

    fn items_copied(&self, _stats: &[Rc<ExplorerItem>], _cut: bool, _previous_cut: Option<&[Rc<ExplorerItem>]>) {
        not_defined("itemsCopied")
    }

    fn set_editable(&self, stat: &Rc<ExplorerItem>, is_editing: bool) -> Promise<()> {
        match &self.set_editable {
            Some(set_editable) => set_editable(stat, is_editing),
            None => not_defined("setEditable"),
        }
    }

    fn is_item_visible(&self, _item: &Rc<ExplorerItem>) -> bool {
        match &self.is_item_visible {
            Some(is_item_visible) => is_item_visible(),
            None => not_defined("isItemVisible"),
        }
    }

    fn is_item_collapsed(&self, _item: &Rc<ExplorerItem>) -> Result<bool, Error> {
        not_defined("isItemCollapsed")
    }

    fn has_focus(&self) -> bool {
        not_defined("hasFocus")
    }

    fn get_focus(&self) -> Vec<Rc<ExplorerItem>> {
        match &self.get_focus {
            Some(get_focus) => get_focus(),
            None => not_defined("getFocus"),
        }
    }

    fn focus_next(&self) {
        match &self.focus_next {
            Some(focus_next) => focus_next(),
            None => not_defined("focusNext"),
        }
    }

    fn focus_last(&self) {
        not_defined("focusLast")
    }

    fn has_phantom_elements(&self) -> bool {
        match &self.has_phantom_elements {
            Some(has_phantom_elements) => has_phantom_elements(),
            None => not_defined("hasPhantomElements"),
        }
    }
}

/// `new TestConfigurationService({ explorer: { ... } })`.
fn configuration_with(explorer: Vec<(&str, Dynamic)>) -> Rc<TestConfigurationService> {
    TestConfigurationService::new(Some(Dynamic::object([("explorer", Dynamic::object(explorer))])))
}

/// `workbenchInstantiationService({ fileService, configurationService }, store)` com os dublês que o `ExplorerService` lê.
fn workbench_instantiation_service(
    guard: &LeakGuard,
    file_service: &Rc<TestFileService>,
    configuration: &Rc<TestConfigurationService>,
    host: Rc<dyn IHostService>,
    uri_identity: Option<Rc<dyn IUriIdentityService>>,
    environment: Rc<TestEnvironmentService>,
    working_copy: Rc<dyn IWorkingCopyFileService>,
) -> Rc<TestInstantiationService> {
    let instantiation = TestInstantiationService::new();
    let file_service_contract: Rc<dyn IFileService> = file_service.clone();
    let configuration_contract: Rc<dyn IConfigurationService> = configuration.clone();
    let uri_identity = uri_identity.unwrap_or_else(|| {
        let concrete = guard.add_rc(UriIdentityService::create((), (file_service_contract.clone(),)));
        let service: Rc<dyn IUriIdentityService> = concrete;
        service
    });
    let files_configuration: Rc<dyn IFilesConfigurationService> = Rc::new(NullFilesConfigurationService);
    instantiation.stub(I_FILE_SERVICE, file_service_contract);
    instantiation.stub(I_CONFIGURATION_SERVICE, configuration_contract);
    instantiation.stub(I_WORKSPACE_CONTEXT_SERVICE, TestContextService::new(None, None));
    instantiation.stub(I_CLIPBOARD_SERVICE, Rc::new(TestClipboardService::new()));
    instantiation.stub(I_EDITOR_SERVICE, Rc::new(TestEditorService));
    instantiation.stub(I_URI_IDENTITY_SERVICE, uri_identity);
    instantiation.stub(I_BULK_EDIT_SERVICE, Rc::new(StubBulkEditService));
    instantiation.stub(I_PROGRESS_SERVICE, Rc::new(TestProgressService));
    instantiation.stub(I_HOST_SERVICE, host);
    instantiation.stub(I_FILES_CONFIGURATION_SERVICE, files_configuration);
    instantiation.stub(I_WORKBENCH_ENVIRONMENT_SERVICE, environment);
    instantiation.stub(I_WORKING_COPY_FILE_SERVICE, working_copy);
    instantiation
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

fn files_configuration() -> Rc<dyn IFilesConfigurationService> {
    Rc::new(NullFilesConfigurationService)
}

/// `new ExplorerItem(resource, fileService, configuration, NullFilesConfigurationService, parent, directory)` com o
/// `_isDirectoryResolved = directory` e o `parent.addChild(item)` do `add` da suíte.
fn add_item(file_service: &Rc<TestFileService>, configuration: &Rc<TestConfigurationService>, parent: &Rc<ExplorerItem>, name: &str, directory: bool) -> Rc<ExplorerItem> {
    let file_service: Rc<dyn IFileService> = file_service.clone();
    let configuration: Rc<dyn IConfigurationService> = configuration.clone();
    let item = ExplorerItem::new(
        join_path(&parent.resource(), &[name]),
        file_service,
        configuration,
        files_configuration(),
        Some(parent),
        Some(directory),
        None,
        None,
        None,
        None,
        None,
        false,
    );
    item.set_directory_resolved(directory);
    parent.add_child(&item);
    item
}

// Files - ExplorerService

/// Os membros do `suite('Files - ExplorerService')` que os casos de fora da `suite('move batches')` usam.
struct Outer {
    explorer_service: Rc<ExplorerService>,
    focus_changed: Rc<Emitter<bool>>,
    file_service: Rc<TestFileService>,
    calls: Rc<RefCell<Vec<String>>>,
    focus_during_render: Rc<Cell<bool>>,
    item: Rc<ExplorerItem>,
}

impl Outer {
    /// O `setup` da suíte externa.
    fn new(guard: &LeakGuard) -> Self {
        let calls: Rc<RefCell<Vec<String>>> = Rc::default();
        let focus_during_render = Rc::new(Cell::new(false));
        let focus_changed: Rc<Emitter<bool>> = guard.add_rc(Emitter::new());
        let file_service = guard.add_rc(TestFileService::new());
        let configuration = configuration_with(vec![("autoReveal", Dynamic::Bool(false))]);
        let host: Rc<dyn IHostService> = Rc::new(StubHostService { on_did_change_focus: focus_changed.event() });
        let working_copy: Rc<dyn IWorkingCopyFileService> =
            Rc::new(TestWorkingCopyFileService { will_run: Emitter::new(), did_run: Emitter::new(), did_fail: Emitter::new() });
        let instantiation = workbench_instantiation_service(guard, &file_service, &configuration, host, None, TestEnvironmentService::new(), working_copy);
        let explorer_service = guard.add_rc(instantiation.create_instance::<ExplorerService>(()));
        let view = ViewMock {
            set_editable: Some(Box::new({
                let (calls, focus_during_render, focus_changed) = (calls.clone(), focus_during_render.clone(), focus_changed.clone());
                move |_stat, is_editing| {
                    calls.borrow_mut().push(if is_editing { "start editing" } else { "finish editing" }.to_string());
                    if is_editing && focus_during_render.get() {
                        focus_changed.fire(true);
                    }
                    Promise::resolved(())
                }
            })),
            refresh: Some(Box::new({
                let calls = calls.clone();
                move |_recursive, _item| {
                    calls.borrow_mut().push("refresh".to_string());
                    Promise::resolved(())
                }
            })),
            has_phantom_elements: Some(Box::new(|| false)),
            is_item_visible: Some(Box::new(|| true)),
            ..ViewMock::default()
        };
        explorer_service.register_view(Rc::new(view));
        let root = explorer_service.roots()[0].clone();
        let configuration_contract: Rc<dyn IConfigurationService> = configuration;
        let file_service_contract: Rc<dyn IFileService> = file_service.clone();
        let item = NewExplorerItem::new(file_service_contract, configuration_contract, files_configuration(), &root, false);
        root.add_child(&item);
        Self { explorer_service, focus_changed, file_service, calls, focus_during_render, item }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn root_resource(&self) -> URI {
        self.explorer_service.roots()[0].resource()
    }
}

/// O `editableData` da suíte.
fn editable_data() -> EditableData {
    EditableData { validation_message: Rc::new(|_| None), placeholder: None, starting_value: None, on_finish: Rc::new(|_, _| Promise::resolved(())) }
}

/// `new FileChangesEvent([{ resource, type: ADDED }], false)`.
fn added_event(resource: URI) -> Rc<FileChangesEvent> {
    Rc::new(FileChangesEvent::new(&[FileChange::new(FileChangeType::Added, resource)], false))
}

/// Um `{ name, recursive }` das chamadas de `refresh` da vista.
type Refresh = (Option<String>, bool);

fn refresh_of(name: &str, recursive: bool) -> Refresh {
    (Some(name.to_string()), recursive)
}

/// O `[{ name: 'source', recursive: false }, { name: 'target', recursive: false }]` que quase todos os casos esperam.
fn source_then_target() -> Vec<Refresh> {
    vec![refresh_of("source", false), refresh_of("target", false)]
}

/// Um `IWaitUntilData<WorkingCopyFileEvent>` do `batch(events)`.
#[derive(Clone)]
struct Operation {
    correlation_id: usize,
    files: Vec<WorkingCopyFileEventFile>,
}

/// `fire(emitter, event)`.
fn fire(emitter: &Rc<Emitter<WorkingCopyFileEvent>>, operation: &Operation) -> Promise<()> {
    let (emitter, operation) = (emitter.clone(), operation.clone());
    async_fn(async move {
        let event = WorkingCopyFileEvent::new(operation.correlation_id, FileOperation::Move, operation.files.clone());
        emitter.fire(event.clone());
        Promise::all(event.take_joiners()).await?;
        Ok(())
    })
}

/// `settleViewWork()`.
fn settle_view_work(view_work: &Rc<RefCell<Vec<Promise<()>>>>) -> Promise<()> {
    let view_work = view_work.clone();
    async_fn(async move {
        loop {
            let count = view_work.borrow().len();
            let work = view_work.borrow().clone();
            Promise::all_settled(work).await?;
            timeout(0).await?;
            if count == view_work.borrow().len() {
                return Ok(());
            }
        }
    })
}

// Files - ExplorerService move batches

/// Os membros do `suite('move batches')` e o seu `setup`.
struct MoveBatches {
    service: Rc<ExplorerService>,
    file_service: Rc<TestFileService>,
    configuration: Rc<TestConfigurationService>,
    will_run: Rc<Emitter<WorkingCopyFileEvent>>,
    did_run: Rc<Emitter<WorkingCopyFileEvent>>,
    did_fail: Rc<Emitter<WorkingCopyFileEvent>>,
    source: Rc<ExplorerItem>,
    target: Rc<ExplorerItem>,
    refreshes: Rc<RefCell<Vec<Refresh>>>,
    refresh: Rc<RefCell<Option<Rc<dyn Fn() -> Promise<()>>>>>,
    view_work: Rc<RefCell<Vec<Promise<()>>>>,
    correlation_id: Cell<usize>,
}

impl MoveBatches {
    fn new(guard: &LeakGuard) -> Self {
        let refreshes: Rc<RefCell<Vec<Refresh>>> = Rc::default();
        let refresh: Rc<RefCell<Option<Rc<dyn Fn() -> Promise<()>>>>> = Rc::default();
        let view_work: Rc<RefCell<Vec<Promise<()>>>> = Rc::default();
        let file_service = guard.add_rc(TestFileService::new());
        let configuration = configuration_with(vec![
            ("autoReveal", Dynamic::Bool(false)),
            ("fileNesting", Dynamic::object([("enabled", Dynamic::Bool(false))])),
        ]);
        let will_run: Rc<Emitter<WorkingCopyFileEvent>> = guard.add_rc(Emitter::new());
        let did_run: Rc<Emitter<WorkingCopyFileEvent>> = guard.add_rc(Emitter::new());
        let did_fail: Rc<Emitter<WorkingCopyFileEvent>> = guard.add_rc(Emitter::new());
        let host: Rc<dyn IHostService> = Rc::new(StubHostService { on_did_change_focus: Event::none() });
        let working_copy: Rc<dyn IWorkingCopyFileService> =
            Rc::new(TestWorkingCopyFileService { will_run: will_run.clone(), did_run: did_run.clone(), did_fail: did_fail.clone() });
        let uri_identity: Rc<dyn IUriIdentityService> = Rc::new(StubUriIdentityService { ext_uri: ExtUri::ignore_path_case(host_environment()) });
        let instantiation = workbench_instantiation_service(guard, &file_service, &configuration, host, Some(uri_identity), TestEnvironmentService::new(), working_copy);
        let service = guard.add_rc(instantiation.create_instance::<ExplorerService>(()));
        let root = service.roots()[0].clone();
        let source = add_item(&file_service, &configuration, &root, "source", true);
        let target = add_item(&file_service, &configuration, &root, "target", true);
        let view = ViewMock {
            refresh: Some(Box::new({
                let (refreshes, refresh, view_work) = (refreshes.clone(), refresh.clone(), view_work.clone());
                move |recursive, item| {
                    refreshes.borrow_mut().push((item.map(|item| item.name()), recursive));
                    let refresh = refresh.clone();
                    let work = async_fn(async move {
                        let hook = refresh.borrow().clone();
                        if let Some(hook) = hook {
                            hook().await?;
                        }
                        Ok(())
                    });
                    view_work.borrow_mut().push(work.clone());
                    work
                }
            })),
            has_phantom_elements: Some(Box::new(|| false)),
            is_item_visible: Some(Box::new(|| true)),
            focus_next: Some(Box::new(|| {})),
            get_focus: Some(Box::new({
                let source = source.clone();
                move || vec![source.clone()]
            })),
            ..ViewMock::default()
        };
        service.register_view(Rc::new(view));
        Self { service, file_service, configuration, will_run, did_run, did_fail, source, target, refreshes, refresh, view_work, correlation_id: Cell::new(0) }
    }

    /// `add(parent, name, directory = false)`.
    fn add(&self, parent: &Rc<ExplorerItem>, name: &str, directory: bool) -> Rc<ExplorerItem> {
        add_item(&self.file_service, &self.configuration, parent, name, directory)
    }

    /// `moveEvent(item, parent = target, name = item.name)`.
    fn move_event(&self, item: &Rc<ExplorerItem>, parent: Option<&Rc<ExplorerItem>>, name: Option<&str>) -> FileOperationEvent {
        let parent = parent.unwrap_or(&self.target);
        let name = name.map_or_else(|| item.name(), ToString::to_string);
        let options = FileStatOptions { is_file: Some(!item.is_directory()), is_directory: Some(item.is_directory()), ..FileStatOptions::default() };
        let stat = create_file_stat(&join_path(&parent.resource(), &[name.as_str()]), &options);
        FileOperationEvent::new(item.resource(), FileOperation::Move, Some(stat))
    }

    /// `batch(events)`.
    fn batch(&self, events: &[FileOperationEvent]) -> Operation {
        let correlation_id = self.correlation_id.get();
        self.correlation_id.set(correlation_id + 1);
        let files = events
            .iter()
            .map(|event| WorkingCopyFileEventFile { source: Some(event.resource.clone()), target: event.target.as_ref().expect("o evento de movimento tem destino").resource.clone() })
            .collect();
        Operation { correlation_id, files }
    }

    /// `fileService.fireAfterOperation(event)`.
    fn fire_after(&self, event: &FileOperationEvent) {
        self.file_service.fire_after_operation(event.clone());
    }

    /// `finish(emitter, event)`.
    fn finish(&self, emitter: &Rc<Emitter<WorkingCopyFileEvent>>, operation: &Operation) -> Promise<()> {
        let (fired, view_work) = (fire(emitter, operation), self.view_work.clone());
        async_fn(async move {
            fired.await?;
            settle_view_work(&view_work).await?;
            Ok(())
        })
    }

    /// `[...refreshes]`.
    fn refreshes(&self) -> Vec<Refresh> {
        self.refreshes.borrow().clone()
    }

    fn settle(&self) -> Promise<()> {
        settle_view_work(&self.view_work)
    }
}

#[test]
fn move_batches_updates_every_model_entry_immediately_and_refreshes_each_parent_once_at_completion() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let events: Vec<FileOperationEvent> = (0..32).map(|index| h.move_event(&h.add(&h.source, &format!("file{index}.ts"), false), None, None)).collect();
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        for event in &events {
            h.fire_after(event);
        }
        let during = (h.source.children_len(), h.target.children_len(), h.refreshes());
        h.finish(&h.did_run, &operation).await.unwrap();

        assert_eq!(during, (0, 32, vec![]));
        assert_eq!(h.refreshes(), source_then_target());
    });
}

#[test]
fn move_batches_does_not_flush_a_move_batch_for_unrelated_interleaved_moves() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let root = h.service.roots()[0].clone();
        let other = h.add(&root, "other", true);
        let events = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        h.fire_after(&events[0]);
        let unrelated = h.move_event(&h.add(&other, "unrelated", false), None, None);
        h.fire_after(&unrelated);
        let immediate = h.refreshes();
        h.fire_after(&events[1]);
        h.finish(&h.did_run, &operation).await.unwrap();

        assert_eq!(immediate, vec![refresh_of("other", false)]);
        assert_eq!(
            h.refreshes(),
            vec![refresh_of("other", false), refresh_of("target", false), refresh_of("source", false), refresh_of("target", false)]
        );
    });
}

#[test]
fn move_batches_does_not_delay_file_operation_completion_while_explorer_refreshes_moved_parents() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let nested_parent = h.add(&h.source, "nest.ts", false);
        let child = h.add(&h.source, "child.ts", false);
        child.set_nested_parent(Some(&nested_parent));
        let events = vec![h.move_event(&child, None, None), h.move_event(&h.add(&h.source, "other.ts", false), None, None)];
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        let rendered: DeferredPromise<()> = DeferredPromise::new();
        let rendered_promise = rendered.promise.clone();
        *h.refresh.borrow_mut() = Some(Rc::new(move || rendered_promise.clone()));
        for event in &events {
            h.fire_after(event);
        }
        let finished = Rc::new(Cell::new(false));
        let completion = fire(&h.did_run, &operation).then({
            let finished = finished.clone();
            move |_| finished.set(true)
        });
        timeout(0).await.unwrap();
        let before_render = (finished.get(), h.refreshes());
        rendered.complete(()).await.unwrap();
        completion.await.unwrap();
        h.settle().await.unwrap();

        assert_eq!(before_render, (true, vec![refresh_of("nest.ts", false), refresh_of("source", false)]));
        assert_eq!(h.refreshes(), vec![refresh_of("nest.ts", false), refresh_of("source", false), refresh_of("target", false)]);
    });
}

fn keeps_overlapping_batches_independent(finish_nested_first: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let root = h.service.roots()[0].clone();
        let third = h.add(&root, "third", true);
        let first = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let second =
            vec![h.move_event(&h.add(&h.source, "c", false), Some(&third), None), h.move_event(&h.add(&h.source, "d", false), Some(&third), None)];
        let operations = [h.batch(&first), h.batch(&second)];
        fire(&h.will_run, &operations[0]).await.unwrap();
        fire(&h.will_run, &operations[1]).await.unwrap();
        for event in &first {
            h.fire_after(event);
        }
        for event in &second {
            h.fire_after(event);
        }
        let order = if finish_nested_first { [1, 0] } else { [0, 1] };
        h.finish(&h.did_run, &operations[order[0]]).await.unwrap();
        let intermediate = h.refreshes();
        h.finish(&h.did_run, &operations[order[1]]).await.unwrap();

        let name_of = |index: usize| if index == 1 { "third" } else { "target" };
        assert_eq!(intermediate, vec![refresh_of("source", false), refresh_of(name_of(order[0]), false)]);
        let expected: Vec<Refresh> = order.iter().flat_map(|index| [refresh_of("source", false), refresh_of(name_of(*index), false)]).collect();
        assert_eq!(h.refreshes(), expected);
        assert_eq!((h.source.children_len(), h.target.children_len(), third.children_len()), (0, 2, 2));
    });
}

#[test]
fn move_batches_keeps_overlapping_batches_independent_nested_finishes_first_false() {
    keeps_overlapping_batches_independent(false);
}

#[test]
fn move_batches_keeps_overlapping_batches_independent_nested_finishes_first_true() {
    keeps_overlapping_batches_independent(true);
}

#[test]
fn move_batches_flushes_partial_failures_and_handles_later_events_without_a_batch() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let events = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        h.fire_after(&events[0]);
        h.finish(&h.did_fail, &operation).await.unwrap();
        let after_failure = h.refreshes();
        h.fire_after(&events[1]);
        h.settle().await.unwrap();

        let mut names = h.target.children_keys();
        names.sort();
        assert_eq!(after_failure, source_then_target());
        assert_eq!(h.refreshes(), [source_then_target(), source_then_target()].concat());
        assert_eq!(names, strings(&["a", "b"]));
    });
}

#[test]
fn move_batches_does_not_retain_an_empty_failed_batch_or_delay_single_file_moves() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let events = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let cancelled = h.batch(&events);
        fire(&h.will_run, &cancelled).await.unwrap();
        fire(&h.did_fail, &cancelled).await.unwrap();
        let single = h.batch(&events[..1]);
        fire(&h.will_run, &single).await.unwrap();
        h.fire_after(&events[0]);
        let immediate = h.refreshes();
        h.finish(&h.did_run, &single).await.unwrap();

        assert_eq!(immediate, vec![refresh_of("source", false)]);
        assert_eq!(h.refreshes(), source_then_target());
    });
}

#[test]
fn move_batches_matches_file_operation_resources_using_uri_identity() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let events = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let operation = h.batch(&events);
        let upper = |resource: &URI| resource.with(&UriChange { path: Field::Set(resource.path().to_upper_case()), ..UriChange::default() });
        let uppercase = Operation {
            files: operation
                .files
                .iter()
                .map(|file| WorkingCopyFileEventFile { source: file.source.as_ref().map(upper), target: upper(&file.target) })
                .collect(),
            ..operation.clone()
        };
        fire(&h.will_run, &uppercase).await.unwrap();
        for event in &events {
            h.fire_after(event);
        }
        let during = h.refreshes();
        h.finish(&h.did_run, &operation).await.unwrap();

        assert_eq!(during, vec![]);
        assert_eq!(h.refreshes(), source_then_target());
    });
}

#[test]
fn move_batches_joins_matching_events_to_every_overlapping_operation() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let events = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let first = h.batch(&events);
        let second = h.batch(&events);
        fire(&h.will_run, &first).await.unwrap();
        fire(&h.will_run, &second).await.unwrap();
        for event in &events {
            h.fire_after(event);
        }
        h.finish(&h.did_run, &second).await.unwrap();
        let first_completion = h.refreshes();
        h.finish(&h.did_fail, &first).await.unwrap();

        assert_eq!(first_completion, source_then_target());
        assert_eq!(h.refreshes(), [source_then_target(), source_then_target()].concat());
    });
}

#[test]
fn move_batches_refreshes_distinct_model_parents_even_when_they_represent_the_same_uri() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let file_service: Rc<dyn IFileService> = h.file_service.clone();
        let configuration: Rc<dyn IConfigurationService> = h.configuration.clone();
        let duplicate_root = ExplorerItem::new(
            h.service.roots()[0].resource(),
            file_service,
            configuration,
            files_configuration(),
            None,
            Some(true),
            None,
            None,
            None,
            None,
            None,
            false,
        );
        h.service.roots_mut().push(duplicate_root.clone());
        let duplicate_source = h.add(&duplicate_root, "source", true);
        let duplicate_target = h.add(&duplicate_root, "target", true);
        let events: Vec<FileOperationEvent> = ["a", "b"]
            .iter()
            .map(|name| {
                h.add(&duplicate_source, name, false);
                h.move_event(&h.add(&h.source, name, false), None, None)
            })
            .collect();
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        for event in &events {
            h.fire_after(event);
        }
        h.finish(&h.did_run, &operation).await.unwrap();

        assert_eq!(
            (h.source.children_len(), h.target.children_len(), duplicate_source.children_len(), duplicate_target.children_len()),
            (0, 2, 0, 2)
        );
        assert_eq!(
            h.refreshes(),
            vec![refresh_of("source", false), refresh_of("source", false), refresh_of("target", false), refresh_of("target", false)]
        );
    });
}

#[test]
fn move_batches_preserves_folder_descendants_renames_and_recursive_nesting_refreshes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        if let Dynamic::Object(nesting) = h.configuration.get_value(Some("explorer"), None).get_str("fileNesting") {
            nesting.set(&JsString::from("enabled"), Dynamic::Bool(true));
        }
        let folder = h.add(&h.source, "folder", true);
        let descendant = h.add(&folder, "child.ts", false);
        let old = h.add(&h.source, "old.ts", false);
        let events = vec![h.move_event(&old, Some(&h.source), Some("new.ts")), h.move_event(&folder, None, None)];
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        for event in &events {
            h.fire_after(event);
        }
        h.finish(&h.did_run, &operation).await.unwrap();

        assert_eq!(h.source.children_keys(), strings(&["new.ts"]));
        assert_eq!(h.target.children_keys(), strings(&["folder"]));
        assert_eq!(
            descendant.resource().path().to_string_lossy(),
            join_path(&h.target.resource(), &["folder", "child.ts"]).path().to_string_lossy()
        );
        assert_eq!(h.refreshes(), vec![refresh_of("source", true), refresh_of("target", true)]);
    });
}

#[test]
fn move_batches_keeps_copy_delete_and_external_file_changes_responsive_during_a_move_batch() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let events = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        let copy = h.move_event(&h.add(&h.source, "copy", false), None, None);
        let copy_target = copy.target.clone().expect("o evento de movimento tem destino");
        h.file_service.fire_after_operation(FileOperationEvent::new(copy.resource.clone(), FileOperation::Copy, Some(copy_target.clone())));
        timeout(0).await.unwrap();
        h.file_service.fire_after_operation(FileOperationEvent::new(copy_target.resource.clone(), FileOperation::Delete, None));
        timeout(0).await.unwrap();
        h.file_service.fire_file_changes(added_event(join_path(&h.source.resource(), &["external"])));
        timeout(500).await.unwrap();
        fire(&h.did_fail, &operation).await.unwrap();

        assert_eq!(h.refreshes(), vec![refresh_of("target", false), refresh_of("target", false), (None, true)]);
    });
}

#[test]
fn move_batches_attempts_every_parent_refresh_and_releases_failed_batch_state() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let events = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        for event in &events {
            h.fire_after(event);
        }
        let error = Error::new("refresh failed");
        let reported: Rc<RefCell<Vec<Error>>> = Rc::default();
        errors::set_unexpected_error_handler({
            let reported = reported.clone();
            move |candidate| reported.borrow_mut().push(candidate.clone())
        });
        *h.refresh.borrow_mut() = Some(Rc::new({
            let error = error.clone();
            move || Promise::rejected(error.clone())
        }));
        h.finish(&h.did_run, &operation).await.unwrap();
        record_unexpected_errors();
        *h.refresh.borrow_mut() = None;
        let after_failure = h.refreshes();
        let late = h.move_event(&h.add(&h.source, "late", false), None, None);
        h.fire_after(&late);
        h.settle().await.unwrap();

        assert_eq!(after_failure, source_then_target());
        assert_eq!(h.refreshes(), [source_then_target(), source_then_target()].concat());
        assert_eq!(*reported.borrow(), vec![error]);
    });
}

#[test]
fn move_batches_disposal_clears_pending_batches_and_prevents_asynchronous_refresh_continuations() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = MoveBatches::new(&guard);
    run_with_faked_timers(async move {
        let child = h.add(&h.source, "child", false);
        let nest = h.add(&h.source, "nest", false);
        child.set_nested_parent(Some(&nest));
        let events = vec![h.move_event(&child, None, None), h.move_event(&h.add(&h.source, "other", false), None, None)];
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        for event in &events {
            h.fire_after(event);
        }
        je_base::lifecycle::IDisposable::dispose(&*h.service);
        h.finish(&h.did_run, &operation).await.unwrap();
        let late = h.move_event(&h.add(&h.source, "late", false), None, None);
        h.fire_after(&late);
        h.settle().await.unwrap();

        assert_eq!(h.refreshes(), vec![]);
    });
}

#[test]
fn move_batches_disposal_during_a_flush_prevents_further_view_work() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let h = Rc::new(MoveBatches::new(&guard));
    let for_test = h.clone();
    run_with_faked_timers(async move {
        let h = for_test;
        let events = vec![h.move_event(&h.add(&h.source, "a", false), None, None), h.move_event(&h.add(&h.source, "b", false), None, None)];
        let operation = h.batch(&events);
        fire(&h.will_run, &operation).await.unwrap();
        for event in &events {
            h.fire_after(event);
        }
        let service = h.service.clone();
        *h.refresh.borrow_mut() = Some(Rc::new(move || {
            je_base::lifecycle::IDisposable::dispose(&*service);
            Promise::resolved(())
        }));
        h.finish(&h.did_run, &operation).await.unwrap();

        assert_eq!(h.refreshes(), vec![refresh_of("source", false)]);
    });
}

fn derives_the_view_id_from_the_application(is_sessions_window: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let file_service = guard.add_rc(TestFileService::new());
    let configuration = configuration_with(vec![("autoReveal", Dynamic::Bool(false))]);
    let host: Rc<dyn IHostService> = Rc::new(StubHostService { on_did_change_focus: Event::none() });
    let working_copy: Rc<dyn IWorkingCopyFileService> =
        Rc::new(TestWorkingCopyFileService { will_run: Emitter::new(), did_run: Emitter::new(), did_fail: Emitter::new() });
    let environment = TestEnvironmentService::with_overrides(EnvironmentOverrides { is_sessions_window, ..EnvironmentOverrides::default() });
    let instantiation = workbench_instantiation_service(&guard, &file_service, &configuration, host, None, environment, working_copy);
    let service = guard.add_rc(instantiation.create_instance::<ExplorerService>(()));
    let before = service.get_view_id();
    service.register_view(Rc::new(ViewMock { id: Some("registered.explorer".to_string()), ..ViewMock::default() }));
    let expected = if is_sessions_window { SESSIONS_FILES_VIEW_ID } else { VIEW_ID };
    assert_eq!((before, service.get_view_id()), (expected.to_string(), expected.to_string()));
}

#[test]
fn derives_the_view_id_from_the_application_before_and_after_view_creation_sessions_false() {
    derives_the_view_id_from_the_application(false);
}

#[test]
fn derives_the_view_id_from_the_application_before_and_after_view_creation_sessions_true() {
    derives_the_view_id_from_the_application(true);
}

#[test]
fn refreshes_on_window_focus_when_not_editing() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let outer = Outer::new(&guard);
    outer.focus_changed.fire(false);
    let after_blur = outer.calls();
    outer.focus_changed.fire(true);

    assert_eq!((after_blur, outer.calls()), (vec![], strings(&["refresh"])));
}

fn defers_focus_refresh(focus_before_render: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let outer = Outer::new(&guard);
    run_with_faked_timers(async move {
        outer.focus_during_render.set(focus_before_render);
        outer.explorer_service.set_editable(outer.item.clone(), Some(editable_data())).await.unwrap();
        if !focus_before_render {
            outer.focus_changed.fire(true);
        }
        let while_editing_calls = outer.calls();
        let while_editing_editable = outer.explorer_service.is_editable(Some(&outer.item));
        let children = outer.explorer_service.roots()[0].children();

        outer.explorer_service.set_editable(outer.item.clone(), None).await.unwrap();

        assert_eq!(while_editing_calls, strings(&["start editing"]));
        assert!(while_editing_editable);
        assert!(children.len() == 1 && Rc::ptr_eq(&children[0], &outer.item));
        assert_eq!(outer.calls(), strings(&["start editing", "finish editing", "refresh"]));
        assert!(!outer.explorer_service.is_editable(None));
    });
}

#[test]
fn defers_focus_refresh_before_rendering_the_input() {
    defers_focus_refresh(true);
}

#[test]
fn defers_focus_refresh_after_rendering_the_input() {
    defers_focus_refresh(false);
}

#[test]
fn coalesces_deferred_focus_refreshes_and_clears_them_after_editing() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let outer = Outer::new(&guard);
    run_with_faked_timers(async move {
        outer.explorer_service.set_editable(outer.item.clone(), Some(editable_data())).await.unwrap();
        outer.focus_changed.fire(true);
        outer.focus_changed.fire(false);
        outer.focus_changed.fire(true);
        outer.explorer_service.set_editable(outer.item.clone(), None).await.unwrap();
        outer.explorer_service.set_editable(outer.item.clone(), Some(editable_data())).await.unwrap();
        outer.focus_changed.fire(false);
        outer.explorer_service.set_editable(outer.item.clone(), None).await.unwrap();

        assert_eq!(outer.calls(), strings(&["start editing", "finish editing", "refresh", "start editing", "finish editing"]));
    });
}

#[test]
fn still_allows_an_explicit_refresh_while_editing() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let outer = Outer::new(&guard);
    run_with_faked_timers(async move {
        outer.explorer_service.set_editable(outer.item.clone(), Some(editable_data())).await.unwrap();
        outer.explorer_service.refresh(false).await.unwrap();

        assert_eq!(outer.calls(), strings(&["start editing", "refresh"]));
    });
}

#[test]
fn defers_file_changes_that_were_already_queued_when_editing_started() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let outer = Outer::new(&guard);
    run_with_faked_timers(async move {
        outer.file_service.fire_file_changes(added_event(join_path(&outer.root_resource(), &["external.txt"])));
        outer.explorer_service.set_editable(outer.item.clone(), Some(editable_data())).await.unwrap();
        timeout(500).await.unwrap();
        let while_editing = outer.calls();

        outer.explorer_service.set_editable(outer.item.clone(), None).await.unwrap();
        timeout(500).await.unwrap();

        assert_eq!(while_editing, strings(&["start editing"]));
        assert_eq!(outer.calls(), strings(&["start editing", "finish editing", "refresh"]));
    });
}

#[test]
fn refreshes_when_a_previously_deleted_root_folder_is_restored() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let outer = Outer::new(&guard);
    run_with_faked_timers(async move {
        let root = outer.root_resource();
        let outside = join_path(&je_platform::resources::ext_uri().dirname(&root), &["outside-workspace"]);
        outer.file_service.fire_file_changes(added_event(outside));
        timeout(500).await.unwrap();
        let after_unrelated_add = outer.calls();

        outer.file_service.fire_file_changes(added_event(root));
        timeout(500).await.unwrap();

        assert_eq!((after_unrelated_add, outer.calls()), (vec![], strings(&["refresh"])));
    });
}
