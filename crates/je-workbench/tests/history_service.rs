//! Porte de `src/vs/workbench/services/history/test/browser/historyService.test.ts` (suíte `HistoryService`), caso a caso e na mesma ordem.
//!
//! Os 23 casos estão portados; nenhum ficou de fora. Os casos rodam sobre o `EditorService` real, a [`EditorPart`] lógica e o
//! [`HistoryService`] real, montados pelo [`EditorServiceAccessor`] (as divergências do acessor estão no cabeçalho de
//! `support/editor_service_support.rs`).
//!
//! Notas de tradução:
//! - **`createServices`.** [`create_services`] cria o acessor, espera a parte (`await part.whenReady`), grava
//!   `workbench.editor.navigationScope` e `search.exclude` no `TestConfigurationService` do acessor (o original cria um novo e o
//!   registra com `instantiationService.stub(IConfigurationService, ...)` antes de criar o serviço) e registra os serviços que o
//!   `HistoryService` injeta e a coleção do acessor ainda não tinha (`IWorkspacesService`, `ILifecycleService` e
//!   `IContextKeyService`; o serviço de layout não existe: ver o cabeçalho de `history_service.rs`). O
//!   `instantiationService.createInstance(HistoryService)` é o `create_instance` do serviço de instanciação.
//! - **`setup` e `teardown`.** O `setup` registra o painel `MyTestEditorForEditorHistory` para a `TestFileEditorInput`, o painel
//!   lado a lado e o `TestTextFileEditor` ([`setup`]). O `teardown` é o `workbenchTeardown` do acessor (nos casos que o chamam)
//!   seguido do `disposables.clear()` ([`finish`]); os casos que não chamam o `workbenchTeardown` só limpam o `disposables`
//!   ([`finish_without_teardown`]). Os casos `async` rodam em `run_with_faked_timers` ([`run_async`]).
//! - **`TestTextFileEditor`.** `setSelection(selection, reason)` é `EditorPane::set_selection` do painel registrado por
//!   `register_test_file_editor` (ver o cabeçalho de `editor_panes.rs`); `pane.options` lido por `assertTextSelection` é
//!   `IEditorPane::text_options`. O `Selection` do original é o de `je_editor::selection`.
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título do caso (a suíte mais o
//!   título do caso) é o argumento de [`to_resource`].
//! - **`TestFileEditorInputWithUntyped`.** A subclasse que sobrescreve `toUntyped()` é a própria `TestFileEditorInput` com
//!   `set_untyped_override` ([`new_input_with_untyped`]).
//! - **`(historyService as HistoryService).onDidChangeEditorNavigationStack`.** É `HistoryService::on_did_change_editor_navigation_stack`.
//! - **`editorHistoryListeners.size`.** É `HistoryService::editor_history_listeners_size_for_testing`.
//! - **`Event.toPromise(...)` e `DeferredPromise`.** São `Event::to_promise(None)` (que dispara uma vez só); o `DeferredPromise`
//!   completado por `onDidActiveEditorChange` dos casos `go to last edit location` e `reopen closed editor` é a mesma promessa.
//! - **`transient editors suspends editor change tracking`.** A segunda `editorChangePromise` do original (encadeada com `.then` e
//!   nunca aguardada) não tem asserção e não é criada.
//! - **`assert.strictEqual(a, b)` entre painéis, grupos e entradas** é a identidade do ponteiro.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::TypeId;
use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable, id_of};
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::selection::Selection;
use je_editor::text_model::TextSource;
use je_platform::contextkey::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::files::files::{FileChange, FileChangeType, FileChangesEvent, FileOperation, FileOperationEvent, FileStat, FileStatWithMetadata};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::opener::editor_types::EditorOptions;
use je_platform::resources::{host_environment, uri_file};
use je_platform::workspaces::workspaces::{
    EnterWorkspaceResultCell, I_WORKSPACES_SERVICE, IWorkspacesService, Recent, RecentlyOpened, RecentlyOpenedCell, WorkspaceFolderCreationData, WorkspaceIdentifierCell,
};
use je_platform::workspace::WorkspaceIdentifier;
use je_platform::backup::BackupInfo;
use je_workbench::browser::parts::editor::editor_panes::EditorPane;
use je_workbench::browser::parts::editor::editor_part::EditorPart;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{
    EditorInputOrUntyped, EditorInputRef, EditorPaneSelectionChangeEvent, EditorPaneSelectionChangeReason, IEditorPane, TextEditorOptions, UntypedEditorInput,
};
use je_workbench::services::editor::editor_groups_service::{GroupDirection, GroupRef, IEditorGroup, IEditorPart, PreferredGroup};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::services::history::history::{GoFilter, GoScope, I_HISTORY_SERVICE, IHistoryService};
use je_workbench::services::history::history_service::{EditorNavigationStack, HistoryService, NavigationRemoval};
use je_workbench::services::lifecycle::lifecycle::{ILifecycleService, I_LIFECYCLE_SERVICE};
use je_workbench::services::textfile::textfiles::ITextFileEditorModel;
use je_workbench::test::browser::workbench_test_services::{TestFileEditorInput, register_test_editor};
use support::editor_service_support::{EditorServiceAccessor, register_test_file_editor, register_test_side_by_side_editor};

const SUITE_TITLE: &str = "HistoryService";
const TEST_EDITOR_ID: &str = "MyTestEditorForEditorHistory";
const TEST_EDITOR_INPUT_ID: &str = "testEditorInputForHistoyService";

type Group = Rc<dyn IEditorGroup>;
type Pane = Rc<dyn IEditorPane>;

//#region Infraestrutura da suíte

/// Os serviços de `createServices()`: a parte, o serviço de histórico, o `EditorService` e o acessor.
struct Fixture {
    accessor: EditorServiceAccessor,
    part: Rc<EditorPart>,
    history: Rc<HistoryService>,
    service: Rc<dyn IEditorService>,
}

/// `IWorkspacesService` de `TestWorkspacesService` (`workbenchTestServices.ts`): o que o `HistoryService` injeta.
struct TestWorkspacesService;

impl IWorkspacesService for TestWorkspacesService {
    fn enter_workspace(&self, _workspace_uri: &URI) -> Promise<Option<EnterWorkspaceResultCell>> {
        Promise::rejected(Error::new("Method not implemented."))
    }

    fn create_untitled_workspace(&self, _folders: Vec<WorkspaceFolderCreationData>, _remote_authority: Option<String>) -> Promise<WorkspaceIdentifierCell> {
        Promise::rejected(Error::new("Method not implemented."))
    }

    fn delete_untitled_workspace(&self, _workspace: &WorkspaceIdentifier) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_workspace_identifier(&self, _workspace_uri: &URI) -> Promise<WorkspaceIdentifierCell> {
        Promise::rejected(Error::new("Method not implemented."))
    }

    fn on_did_change_recently_opened(&self) -> Event<()> {
        Event::none()
    }

    fn add_recently_opened(&self, _recents: Vec<Recent>) -> Promise<()> {
        Promise::resolved(())
    }

    fn remove_recently_opened(&self, _workspaces: Vec<URI>) -> Promise<()> {
        Promise::resolved(())
    }

    fn clear_recently_opened(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_recently_opened(&self) -> Promise<RecentlyOpenedCell> {
        Promise::resolved(RecentlyOpened::default())
    }

    fn get_dirty_workspaces(&self) -> Promise<Vec<BackupInfo>> {
        Promise::resolved(Vec::new())
    }
}

/// O `setup` da suíte.
fn setup(disposables: &Rc<DisposableStore>) {
    disposables.add(register_test_editor(TEST_EDITOR_ID, vec![TypeId::of::<TestFileEditorInput>()], None));
    disposables.add(register_test_side_by_side_editor());
    disposables.add(register_test_file_editor());
}

/// `createServices(scope = GoScope.DEFAULT, configureSearchExclude = false)`.
async fn create_services(disposables: &Rc<DisposableStore>, scope: GoScope, configure_search_exclude: bool) -> Fixture {
    let accessor = EditorServiceAccessor::with_default_backup_service(disposables);
    accessor.part.when_ready().await.unwrap();

    match scope {
        GoScope::EditorGroup => accessor.test_configuration_service.set_user_configuration("workbench.editor.navigationScope", Dynamic::string("editorGroup"), None).await.unwrap(),
        GoScope::Editor => accessor.test_configuration_service.set_user_configuration("workbench.editor.navigationScope", Dynamic::string("editor"), None).await.unwrap(),
        GoScope::Default => {}
    }
    if configure_search_exclude {
        accessor
            .test_configuration_service
            .set_user_configuration("search", Dynamic::object([("exclude", Dynamic::object([("**/node_modules/**", Dynamic::Bool(true))]))]), None)
            .await
            .unwrap();
    }

    // Os serviços que o `HistoryService` injeta e a coleção do acessor ainda não tinha.
    accessor.collection.set(I_WORKSPACES_SERVICE, Rc::new(TestWorkspacesService) as Rc<dyn IWorkspacesService>);
    accessor.collection.set(I_LIFECYCLE_SERVICE, accessor.lifecycle_service.clone() as Rc<dyn ILifecycleService>);
    accessor.collection.set(I_CONTEXT_KEY_SERVICE, MockContextKeyService::new() as Rc<dyn IContextKeyService>);

    let history = disposables.add_rc(accessor.instantiation_service.create_instance::<HistoryService>(()));
    accessor.collection.set(I_HISTORY_SERVICE, history.clone() as Rc<dyn IHistoryService>);

    let part = accessor.part.clone();
    let service: Rc<dyn IEditorService> = accessor.editor_service.clone();

    Fixture { accessor, part, history, service }
}

/// O `teardown` com `workbenchTeardown(instantiationService)`: o `workbenchTeardown` do acessor e o `disposables.clear()`.
async fn finish(fixture: &Fixture, disposables: &Rc<DisposableStore>) {
    fixture.accessor.teardown().await.unwrap();
    disposables.clear();
}

/// O `teardown` dos casos que não chamam o `workbenchTeardown`: só o `disposables.clear()`.
fn finish_without_teardown(disposables: &Rc<DisposableStore>) {
    disposables.clear();
}

/// O caso assíncrono: o guarda de vazamentos, o `setup` e o corpo no relógio virtual.
fn run_async<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        setup(&disposables);
        body(disposables).await;
    });
}

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();

    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `URI.parse(value)`.
fn uri(value: &str) -> URI {
    URI::parse(value, false)
}

/// `resource.toString()`.
fn res(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `disposables.add(new TestFileEditorInput(URI.parse(resource), TEST_EDITOR_INPUT_ID))`.
fn new_input(disposables: &Rc<DisposableStore>, resource: &str) -> Rc<TestFileEditorInput> {
    new_input_of(disposables, uri(resource))
}

/// `disposables.add(new TestFileEditorInput(resource, TEST_EDITOR_INPUT_ID))`.
fn new_input_of(disposables: &Rc<DisposableStore>, resource: URI) -> Rc<TestFileEditorInput> {
    disposables.add_rc(TestFileEditorInput::new(resource, TEST_EDITOR_INPUT_ID))
}

/// `disposables.add(new TestFileEditorInputWithUntyped(resource, TEST_EDITOR_INPUT_ID))`: o `toUntyped()` devolve o recurso com
/// `options.override` `testOverride`.
fn new_input_with_untyped(disposables: &Rc<DisposableStore>, resource: URI) -> Rc<TestFileEditorInput> {
    let input = new_input_of(disposables, resource.clone());
    input.set_untyped_override(UntypedEditorInput { resource: Some(resource), options: Some(TextEditorOptions::with_override_id("testOverride")), ..UntypedEditorInput::default() });

    input
}

/// A entrada de teste como `EditorInput`.
fn editor_ref<T: EditorInput + 'static>(input: &Rc<T>) -> EditorInputRef {
    input.clone()
}

/// `{ pinned: true }`.
fn pinned() -> Option<EditorOptions> {
    Some(EditorOptions { pinned: Some(true), ..EditorOptions::default() })
}

/// `{ resource }` ou `{ resource, options: { pinned: true } }` como `IResourceEditorInput`.
fn untyped_resource(resource: &URI, pin: bool) -> UntypedEditorInput {
    UntypedEditorInput { resource: Some(resource.clone()), options: pin.then(|| TextEditorOptions::from(EditorOptions { pinned: Some(true), ..EditorOptions::default() })), ..UntypedEditorInput::default() }
}

/// `editorService.openEditor({ resource, options: { pinned } }, group)`.
async fn open_resource(service: &dyn IEditorService, resource: &URI, pin: bool, group: Option<PreferredGroup>) -> Option<Pane> {
    service.open_editor_in(untyped_resource(resource, pin), group).await.unwrap()
}

/// `group.openEditor(input, { pinned: true })`.
async fn open_pinned<T: EditorInput + 'static>(group: &Group, input: &Rc<T>) -> Option<Pane> {
    group.open_editor(&editor_ref(input), pinned()).await.unwrap()
}

/// `assert.strictEqual(a, b)` entre painéis.
fn same_pane(a: &Pane, b: &Pane) -> bool {
    id_of(a) == id_of(b)
}

/// `assert.strictEqual(a, b)` entre grupos.
fn same_group(a: &Group, b: &Group) -> bool {
    Rc::ptr_eq(a, b)
}

/// `assert.strictEqual(group.activeEditor, input)`.
fn is_active<U: ?Sized>(group: &Group, expected: &Rc<U>) -> bool {
    group.active_editor().is_some_and(|active| id_of(&active) == id_of(expected))
}

/// `pane.group`.
fn pane_group(part: &Rc<EditorPart>, pane: &Pane) -> Group {
    part.get_group(pane.group_id()).expect("o grupo do painel existe")
}

/// `assert.ok(pane)`.
fn expect_pane(pane: Option<Pane>) -> Pane {
    pane.expect("o editor abriu")
}

fn gref(group: &Group) -> GroupRef {
    GroupRef::Group(group.clone())
}

/// `group.activeEditor?.resource?.toString()`.
fn active_resource(group: &Group) -> Option<String> {
    group.active_editor().and_then(|editor| editor.resource()).map(|resource| res(&resource))
}

/// `editorService.activeEditor?.resource?.toString()`.
fn service_active_resource(service: &dyn IEditorService) -> Option<String> {
    match service.active_editor() {
        Some(EditorInputOrUntyped::Typed(editor)) => editor.resource().map(|resource| res(&resource)),
        Some(EditorInputOrUntyped::Untyped(editor)) => editor.resource.map(|resource| res(&resource)),
        None => None,
    }
}

/// `{ reason }` de `IEditorPaneSelectionChangeEvent`.
fn event_of(reason: EditorPaneSelectionChangeReason) -> Option<EditorPaneSelectionChangeEvent> {
    Some(EditorPaneSelectionChangeEvent { reason })
}

/// `setTextSelection(historyService, pane, selection, reason)`.
async fn set_text_selection(history: &Rc<HistoryService>, pane: &Pane, selection: Selection, reason: EditorPaneSelectionChangeReason) {
    let promise = history.on_did_change_editor_navigation_stack().to_promise(None).promise;

    // `pane.setSelection(selection, reason)`
    pane.as_any().downcast_ref::<EditorPane>().expect("o painel é o TestTextFileEditor").set_selection(Some(selection), reason);

    promise.await.unwrap();
}

/// `assertTextSelection(expected, pane)`.
fn assert_text_selection(expected: &Selection, pane: &Pane) {
    let options = pane.text_options().expect("EditorPane has no selection");
    let selection = options.selection;

    assert_eq!(selection.map(|selection| selection.start_line_number), Some(f64::from(expected.start_line_number)));
    assert_eq!(selection.map(|selection| selection.start_column), Some(f64::from(expected.start_column)));
    assert_eq!(selection.and_then(|selection| selection.end_line_number), Some(f64::from(expected.end_line_number)));
    assert_eq!(selection.and_then(|selection| selection.end_column), Some(f64::from(expected.end_column)));
}

//#endregion

#[test]
fn back_forward_basics() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history) = (&fixture.part, &fixture.history);

        let input1 = new_input(&disposables, "foo://bar1");
        open_pinned(&part.active_group(), &input1).await;
        assert!(is_active(&part.active_group(), &input1));

        let input2 = new_input(&disposables, "foo://bar2");
        open_pinned(&part.active_group(), &input2).await;
        assert!(is_active(&part.active_group(), &input2));

        history.go_back(None).await.unwrap();
        assert!(is_active(&part.active_group(), &input1));

        history.go_forward(None).await.unwrap();
        assert!(is_active(&part.active_group(), &input2));

        finish_without_teardown(&disposables);
    });
}

#[test]
fn back_forward_is_editor_group_aware() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);

        let resource = to_resource("back / forward: is editor group aware", "/path/index.txt");
        let other_resource = to_resource("back / forward: is editor group aware", "/path/other.html");

        let pane1 = expect_pane(open_resource(service, &resource, true, None).await);
        let pane2 = expect_pane(open_resource(service, &resource, true, Some(PreferredGroup::Side)).await);

        // [index.txt] | [>index.txt<]

        assert!(!same_pane(&pane1, &pane2));

        open_resource(service, &other_resource, true, Some(PreferredGroup::Group(pane_group(part, &pane2)))).await;

        // [index.txt] | [index.txt] [>other.html<]

        history.go_back(None).await.unwrap();

        // [index.txt] | [>index.txt<] [other.html]

        assert_eq!(part.active_group().id(), pane2.group_id());
        assert_eq!(active_resource(&part.active_group()), Some(res(&resource)));

        history.go_back(None).await.unwrap();

        // [>index.txt<] | [index.txt] [other.html]

        assert_eq!(part.active_group().id(), pane1.group_id());
        assert_eq!(active_resource(&part.active_group()), Some(res(&resource)));

        history.go_forward(None).await.unwrap();

        // [index.txt] | [>index.txt<] [other.html]

        assert_eq!(part.active_group().id(), pane2.group_id());
        assert_eq!(active_resource(&part.active_group()), Some(res(&resource)));

        history.go_forward(None).await.unwrap();

        // [index.txt] | [index.txt] [>other.html<]

        assert_eq!(part.active_group().id(), pane2.group_id());
        assert_eq!(active_resource(&part.active_group()), Some(res(&other_resource)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_in_editor_text_selection_changes_user() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (history, service) = (&fixture.history, &*fixture.service);

        let resource = to_resource("back / forward: in-editor text selection changes (user)", "/path/index.txt");

        let pane = expect_pane(open_resource(service, &resource, true, None).await);

        use EditorPaneSelectionChangeReason::User;
        set_text_selection(history, &pane, Selection::new(1, 2, 1, 2), User).await;
        set_text_selection(history, &pane, Selection::new(15, 1, 15, 1), User).await; // will be merged and dropped
        set_text_selection(history, &pane, Selection::new(16, 1, 16, 1), User).await; // will be merged and dropped
        set_text_selection(history, &pane, Selection::new(17, 1, 17, 1), User).await;
        set_text_selection(history, &pane, Selection::new(30, 5, 30, 8), User).await;
        set_text_selection(history, &pane, Selection::new(40, 1, 40, 1), User).await;

        history.go_back(Some(GoFilter::None)).await.unwrap();
        assert_text_selection(&Selection::new(30, 5, 30, 8), &pane);

        history.go_back(Some(GoFilter::None)).await.unwrap();
        assert_text_selection(&Selection::new(17, 1, 17, 1), &pane);

        history.go_back(Some(GoFilter::None)).await.unwrap();
        assert_text_selection(&Selection::new(1, 2, 1, 2), &pane);

        history.go_forward(Some(GoFilter::None)).await.unwrap();
        assert_text_selection(&Selection::new(17, 1, 17, 1), &pane);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_in_editor_text_selection_changes_navigation() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (history, service) = (&fixture.history, &*fixture.service);

        let resource = to_resource("back / forward: in-editor text selection changes (navigation)", "/path/index.txt");

        let pane = expect_pane(open_resource(service, &resource, true, None).await);

        use EditorPaneSelectionChangeReason::{Navigation, User};
        set_text_selection(history, &pane, Selection::new(2, 2, 2, 10), User).await; // this is our starting point
        set_text_selection(history, &pane, Selection::new(5, 3, 5, 20), Navigation).await; // this is our first target definition
        set_text_selection(history, &pane, Selection::new(120, 8, 120, 18), Navigation).await; // this is our second target definition
        set_text_selection(history, &pane, Selection::new(300, 3, 300, 20), User).await; // unrelated user navigation
        set_text_selection(history, &pane, Selection::new(500, 3, 500, 20), User).await; // unrelated user navigation
        set_text_selection(history, &pane, Selection::new(200, 3, 200, 20), User).await; // unrelated user navigation

        history.go_back(Some(GoFilter::Navigation)).await.unwrap(); // this should reveal the last navigation entry because we are not at it currently
        assert_text_selection(&Selection::new(120, 8, 120, 18), &pane);

        history.go_back(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        history.go_back(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        history.go_forward(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(120, 8, 120, 18), &pane);

        history.go_previous(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        history.go_previous(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(120, 8, 120, 18), &pane);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_in_editor_text_selection_changes_jump() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (history, service) = (&fixture.history, &*fixture.service);

        let resource = to_resource("back / forward: in-editor text selection changes (jump)", "/path/index.txt");

        let pane = expect_pane(open_resource(service, &resource, true, None).await);

        use EditorPaneSelectionChangeReason::{Jump, User};
        set_text_selection(history, &pane, Selection::new(2, 2, 2, 10), User).await;
        set_text_selection(history, &pane, Selection::new(5, 3, 5, 20), Jump).await;
        set_text_selection(history, &pane, Selection::new(120, 8, 120, 18), Jump).await;

        history.go_back(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        history.go_back(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(2, 2, 2, 10), &pane);

        history.go_forward(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        history.go_last(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(120, 8, 120, 18), &pane);

        history.go_previous(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        history.go_previous(Some(GoFilter::Navigation)).await.unwrap();
        assert_text_selection(&Selection::new(120, 8, 120, 18), &pane);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_selection_changes_with_jump_or_navigation_source_are_not_merged_143833() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (history, service) = (&fixture.history, &*fixture.service);

        let resource = to_resource("back / forward: selection changes with JUMP or NAVIGATION source are not merged (#143833)", "/path/index.txt");

        let pane = expect_pane(open_resource(service, &resource, true, None).await);

        use EditorPaneSelectionChangeReason::{Jump, Navigation, User};
        set_text_selection(history, &pane, Selection::new(2, 2, 2, 10), User).await;
        set_text_selection(history, &pane, Selection::new(5, 3, 5, 20), Jump).await;
        set_text_selection(history, &pane, Selection::new(6, 3, 6, 20), Navigation).await;

        history.go_back(Some(GoFilter::None)).await.unwrap();
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        history.go_back(Some(GoFilter::None)).await.unwrap();
        assert_text_selection(&Selection::new(2, 2, 2, 10), &pane);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_edit_selection_changes() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (history, service) = (&fixture.history, &*fixture.service);

        let resource = to_resource("back / forward: edit selection changes", "/path/index.txt");

        let pane = expect_pane(open_resource(service, &resource, true, None).await);

        use EditorPaneSelectionChangeReason::{Edit, User};
        set_text_selection(history, &pane, Selection::new(2, 2, 2, 10), User).await;
        set_text_selection(history, &pane, Selection::new(50, 3, 50, 20), Edit).await;
        set_text_selection(history, &pane, Selection::new(300, 3, 300, 20), User).await; // unrelated user navigation
        set_text_selection(history, &pane, Selection::new(500, 3, 500, 20), User).await; // unrelated user navigation
        set_text_selection(history, &pane, Selection::new(200, 3, 200, 20), User).await; // unrelated user navigation
        set_text_selection(history, &pane, Selection::new(5, 3, 5, 20), Edit).await;
        set_text_selection(history, &pane, Selection::new(200, 3, 200, 20), User).await; // unrelated user navigation

        history.go_back(Some(GoFilter::Edits)).await.unwrap(); // this should reveal the last navigation entry because we are not at it currently
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        history.go_back(Some(GoFilter::Edits)).await.unwrap();
        assert_text_selection(&Selection::new(50, 3, 50, 20), &pane);

        history.go_forward(Some(GoFilter::Edits)).await.unwrap();
        assert_text_selection(&Selection::new(5, 3, 5, 20), &pane);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_tracks_editor_moves_across_groups() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);

        let resource1 = to_resource("back / forward: tracks editor moves across groups", "/path/one.txt");
        let resource2 = to_resource("back / forward: tracks editor moves across groups", "/path/two.html");

        let pane1 = expect_pane(open_resource(service, &resource1, true, None).await);
        open_resource(service, &resource2, true, None).await;

        // [one.txt] [>two.html<]

        let side_group = part.add_group(Some(gref(&part.active_group())), GroupDirection::Right);

        // [one.txt] [>two.html<] | <empty>

        let editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        pane_group(part, &pane1).move_editor(&pane1.input().expect("o painel tem entrada"), &side_group);
        editor_change_promise.await.unwrap();

        // [one.txt] | [>two.html<]

        history.go_back(None).await.unwrap();

        // [>one.txt<] | [two.html]

        assert_eq!(part.active_group().id(), pane1.group_id());
        assert_eq!(active_resource(&part.active_group()), Some(res(&resource1)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_tracks_group_removals() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);

        let resource1 = to_resource("back / forward: tracks group removals", "/path/one.txt");
        let resource2 = to_resource("back / forward: tracks group removals", "/path/two.html");

        let pane1 = expect_pane(open_resource(service, &resource1, true, None).await);
        let pane2 = expect_pane(open_resource(service, &resource2, true, Some(PreferredGroup::Side)).await);

        // [one.txt] | [>two.html<]

        assert!(!same_pane(&pane1, &pane2));

        pane_group(part, &pane1).close_all_editors(None).await.unwrap();

        // [>two.html<]

        history.go_back(None).await.unwrap();

        // [>two.html<]

        assert_eq!(part.active_group().id(), pane2.group_id());
        assert_eq!(active_resource(&part.active_group()), Some(res(&resource2)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_editor_navigation_stack_navigation() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, service) = (&fixture.part, &*fixture.service);
        let _ = part;

        let stack = fixture.accessor.instantiation_service.create_instance::<EditorNavigationStack>((GoFilter::None, GoScope::Default));

        let resource = to_resource("back / forward: editor navigation stack - navigation", "/path/index.txt");
        let other_resource = to_resource("back / forward: editor navigation stack - navigation", "/path/index.html");
        let pane = open_resource(service, &resource, true, None).await;

        let changed = Rc::new(Cell::new(false));
        disposables.add({
            let changed = changed.clone();
            stack.on_did_change().listen(move |_| changed.set(true))
        });

        assert!(!stack.can_go_back());
        assert!(!stack.can_go_forward());
        assert!(!stack.can_go_last());

        // Opening our first editor emits change event
        stack.notify_navigation(pane.as_ref(), event_of(EditorPaneSelectionChangeReason::User));
        assert!(changed.get());
        changed.set(false);

        assert!(!stack.can_go_back());
        assert!(stack.can_go_last());

        // Opening same editor is not treated as new history stop
        stack.notify_navigation(pane.as_ref(), event_of(EditorPaneSelectionChangeReason::User));
        assert!(!stack.can_go_back());

        // Opening different editor allows to go back
        open_resource(service, &other_resource, true, None).await;

        stack.notify_navigation(pane.as_ref(), event_of(EditorPaneSelectionChangeReason::User));
        assert!(changed.get());
        changed.set(false);

        assert!(stack.can_go_back());

        stack.go_back().await.unwrap();
        assert!(!stack.can_go_back());
        assert!(stack.can_go_forward());
        assert!(stack.can_go_last());

        stack.go_forward().await.unwrap();
        assert!(stack.can_go_back());
        assert!(!stack.can_go_forward());

        stack.go_previous().await.unwrap();
        assert!(!stack.can_go_back());
        assert!(stack.can_go_forward());

        stack.go_previous().await.unwrap();
        assert!(stack.can_go_back());
        assert!(!stack.can_go_forward());

        stack.go_back().await.unwrap();
        stack.go_last().await.unwrap();
        assert!(stack.can_go_back());
        assert!(!stack.can_go_forward());

        stack.dispose();
        assert!(!stack.can_go_back());

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_editor_navigation_stack_mutations() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let service = &*fixture.service;
        let title = "back / forward: editor navigation stack - mutations";

        let stack = disposables.add_rc(fixture.accessor.instantiation_service.create_instance::<EditorNavigationStack>((GoFilter::None, GoScope::Default)));

        let resource = to_resource(title, "/path/index.txt");
        let other_resource = to_resource(title, "/path/index.html");
        let unrelated_resource = to_resource(title, "/path/unrelated.html");
        let pane = open_resource(service, &resource, true, None).await;
        let pane_input = || pane.as_ref().and_then(|pane| pane.input()).expect("o painel tem entrada");

        stack.notify_navigation(pane.as_ref(), None);

        open_resource(service, &other_resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);

        // Clear
        assert!(stack.can_go_back());
        stack.clear();
        assert!(!stack.can_go_back());

        open_resource(service, &resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);
        open_resource(service, &other_resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);

        // Remove unrelated resource does not cause any harm (via internal event)
        stack.go_back().await.unwrap();
        assert!(stack.can_go_forward());
        stack.remove(&NavigationRemoval::from(FileOperationEvent::new(unrelated_resource.clone(), FileOperation::Delete, None)));
        assert!(stack.can_go_forward());

        // Remove (via internal event)
        stack.go_forward().await.unwrap();
        assert!(stack.can_go_back());
        stack.remove(&NavigationRemoval::from(FileOperationEvent::new(resource.clone(), FileOperation::Delete, None)));
        assert!(!stack.can_go_back());
        stack.clear();

        open_resource(service, &resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);
        open_resource(service, &other_resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);

        // Remove (via external event)
        assert!(stack.can_go_back());
        stack.remove(&NavigationRemoval::from(FileChangesEvent::new(&[FileChange::new(FileChangeType::Deleted, resource.clone())], !Platform::detect().is_linux)));
        assert!(!stack.can_go_back());
        stack.clear();

        open_resource(service, &resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);
        open_resource(service, &other_resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);

        // Remove (via editor)
        assert!(stack.can_go_back());
        stack.remove(&NavigationRemoval::from(&pane_input()));
        assert!(!stack.can_go_back());
        stack.clear();

        open_resource(service, &resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);
        open_resource(service, &other_resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);

        // Remove (via group)
        assert!(stack.can_go_back());
        stack.remove(&NavigationRemoval::from(pane.as_ref().expect("o editor abriu").group_id()));
        assert!(!stack.can_go_back());
        stack.clear();

        open_resource(service, &resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);
        open_resource(service, &other_resource, true, None).await;
        stack.notify_navigation(pane.as_ref(), None);

        // Move
        let stat = FileStatWithMetadata::new(FileStat {
            resource: to_resource(title, "/path/other.txt"),
            name: "other.txt".to_string(),
            size: Some(0),
            mtime: Some(0),
            ctime: Some(0),
            etag: Some(String::new()),
            readonly: Some(false),
            locked: Some(false),
            executable: Some(false),
            is_file: true,
            is_directory: false,
            is_symbolic_link: false,
            children: None,
        });
        let moved_resource = stat.resource.clone();
        stack.move_(&FileOperationEvent::new(resource.clone(), FileOperation::Move, Some(stat)));
        stack.go_back().await.unwrap();
        assert_eq!(pane_input().resource().map(|resource| res(&resource)), Some(res(&moved_resource)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_editor_group_scope() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::EditorGroup, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);
        let title = "back / forward: editor group scope";

        let resource1 = to_resource(title, "/path/one.txt");
        let resource2 = to_resource(title, "/path/two.html");
        let resource3 = to_resource(title, "/path/three.html");

        let pane1 = expect_pane(open_resource(service, &resource1, true, None).await);
        open_resource(service, &resource2, true, None).await;
        open_resource(service, &resource3, true, None).await;

        // [one.txt] [two.html] [>three.html<]

        let side_group = part.add_group(Some(gref(&part.active_group())), GroupDirection::Right);

        // [one.txt] [two.html] [>three.html<] | <empty>

        let pane2 = expect_pane(open_resource(service, &resource1, true, Some(PreferredGroup::Group(side_group.clone()))).await);
        open_resource(service, &resource2, true, None).await;
        open_resource(service, &resource3, true, None).await;

        // [one.txt] [two.html] [>three.html<] | [one.txt] [two.html] [>three.html<]

        history.go_back(None).await.unwrap();
        history.go_back(None).await.unwrap();
        history.go_back(None).await.unwrap();

        assert_eq!(part.active_group().id(), pane2.group_id());
        assert_eq!(active_resource(&part.active_group()), Some(res(&resource1)));

        // [one.txt] [two.html] [>three.html<] | [>one.txt<] [two.html] [three.html]

        open_resource(service, &resource3, true, Some(PreferredGroup::Group(pane_group(part, &pane1)))).await;

        history.go_back(None).await.unwrap();
        history.go_back(None).await.unwrap();
        history.go_back(None).await.unwrap();

        assert_eq!(part.active_group().id(), pane1.group_id());
        assert_eq!(active_resource(&part.active_group()), Some(res(&resource1)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn back_forward_editor_scope() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Editor, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);
        let title = "back / forward: editor  scope";

        let resource1 = to_resource(title, "/path/one.txt");
        let resource2 = to_resource(title, "/path/two.html");

        let pane = expect_pane(open_resource(service, &resource1, true, None).await);

        use EditorPaneSelectionChangeReason::User;
        set_text_selection(history, &pane, Selection::new(2, 2, 2, 10), User).await;
        set_text_selection(history, &pane, Selection::new(50, 3, 50, 20), User).await;

        open_resource(service, &resource2, true, None).await;
        set_text_selection(history, &pane, Selection::new(12, 2, 12, 10), User).await;
        set_text_selection(history, &pane, Selection::new(150, 3, 150, 20), User).await;

        history.go_back(None).await.unwrap();
        assert_text_selection(&Selection::new(12, 2, 12, 10), &pane);

        history.go_back(None).await.unwrap();
        assert_text_selection(&Selection::new(12, 2, 12, 10), &pane); // no change

        assert_eq!(active_resource(&part.active_group()), Some(res(&resource2)));

        open_resource(service, &resource1, true, None).await;

        history.go_back(None).await.unwrap();
        assert_text_selection(&Selection::new(2, 2, 2, 10), &pane);

        history.go_back(None).await.unwrap();
        assert_text_selection(&Selection::new(2, 2, 2, 10), &pane); // no change

        assert_eq!(active_resource(&part.active_group()), Some(res(&resource1)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn go_to_last_edit_location() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (history, service) = (&fixture.history, &*fixture.service);
        let title = "go to last edit location";

        let resource = to_resource(title, "/path/index.txt");
        let other_resource = to_resource(title, "/path/index.html");
        open_resource(service, &resource, false, None).await;

        let model = fixture.accessor.files().resolve_model(&resource, None).await.unwrap();
        ITextFileEditorModel::text_editor_model(&*model).expect("o modelo de texto foi resolvido").set_value(TextSource::from("Hello World"));
        timeout(10).await.unwrap(); // history debounces change events

        open_resource(service, &other_resource, false, None).await;

        let on_did_active_editor_change = service.on_did_active_editor_change().to_promise(None).promise;

        drop(history.go_last(Some(GoFilter::Edits)));
        on_did_active_editor_change.await.unwrap();

        assert_eq!(service_active_resource(service), Some(res(&resource)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn reopen_closed_editor() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (history, service) = (&fixture.history, &*fixture.service);

        let resource = to_resource("reopen closed editor", "/path/index.txt");
        let pane = open_resource(service, &resource, false, None).await;

        pane_group(&fixture.part, &expect_pane(pane)).close_all_editors(None).await.unwrap();

        let on_did_active_editor_change = service.on_did_active_editor_change().to_promise(None).promise;

        drop(history.reopen_last_closed_editor());
        on_did_active_editor_change.await.unwrap();

        assert_eq!(service_active_resource(service), Some(res(&resource)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn reopen_closed_editors_as_a_batch_when_closed_together() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);
        let title = "reopen closed editors as a batch when closed together";

        let resource1 = to_resource(title, "/path/one.txt");
        let resource2 = to_resource(title, "/path/two.txt");
        let resource3 = to_resource(title, "/path/three.txt");

        open_resource(service, &resource1, true, None).await;
        open_resource(service, &resource2, true, None).await;
        open_resource(service, &resource3, true, None).await;

        assert_eq!(part.active_group().count(), 3);

        // Bulk close: all editors closed together should reopen together
        part.active_group().close_all_editors(None).await.unwrap();
        assert_eq!(part.active_group().count(), 0);

        history.reopen_last_closed_editor().await.unwrap();

        assert_eq!(part.active_group().count(), 3);
        let mut reopened: Vec<String> = part.active_group().editors().iter().filter_map(|editor| editor.resource()).map(|resource| res(&resource)).collect();
        reopened.sort();
        let mut expected: Vec<String> = [&resource1, &resource2, &resource3].iter().map(|resource| res(resource)).collect();
        expected.sort();
        assert_eq!(reopened, expected);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn get_history() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, true).await;
        let (part, history_service, service) = (&fixture.part, &fixture.history, fixture.service.clone());

        let mut history = history_service.get_history();
        assert_eq!(history.len(), 0);

        let input1 = new_input(&disposables, "foo://bar1/node_modules/test.txt");
        open_pinned(&part.active_group(), &input1).await;

        let input2 = new_input(&disposables, "foo://bar2");
        open_pinned(&part.active_group(), &input2).await;

        let input3 = new_input_with_untyped(&disposables, uri("foo://bar3"));
        open_pinned(&part.active_group(), &input3).await;

        let input4 = new_input_with_untyped(&disposables, uri_file("bar4"));
        open_pinned(&part.active_group(), &input4).await;

        history = history_service.get_history();
        assert_eq!(history.len(), 4);

        // first entry is untyped because it implements `toUntyped` and has a supported scheme
        assert!(matches!(&history[0], EditorInputOrUntyped::Untyped(entry) if entry.is_resource_editor_input()));
        assert!(matches!(&history[0], EditorInputOrUntyped::Untyped(entry) if entry.options.as_ref().and_then(|options| options.override_id()).as_deref() == Some("testOverride")));
        // second entry is not untyped even though it implements `toUntyped` but has unsupported scheme
        assert!(matches!(&history[1], EditorInputOrUntyped::Typed(_)));
        assert!(matches!(&history[2], EditorInputOrUntyped::Typed(_)));
        assert!(matches!(&history[3], EditorInputOrUntyped::Typed(_)));

        history_service.remove_from_history(&EditorInputOrUntyped::Typed(editor_ref(&input2)));
        history = history_service.get_history();
        assert_eq!(history.len(), 3);
        let first_resource = match &history[0] {
            EditorInputOrUntyped::Typed(entry) => entry.resource(),
            EditorInputOrUntyped::Untyped(entry) => entry.resource.clone(),
        };
        assert_eq!(first_resource.map(|resource| res(&resource)), Some(res(&input4.resource)));

        input1.dispose(); // disposing the editor will apply `search.exclude` rules
        history = history_service.get_history();
        assert_eq!(history.len(), 2);

        // side by side
        let input5 = new_input_with_untyped(&disposables, uri("file://bar5"));
        let input6 = new_input_with_untyped(&disposables, uri_file("file://bar1/node_modules/test.txt"));
        let input7 = SideBySideEditorInput::new(None, None, editor_ref(&input6), editor_ref(&input5), service.clone());
        open_pinned(&part.active_group(), &input7).await;

        history = history_service.get_history();
        assert_eq!(history.len(), 3);
        input7.dispose();

        history = history_service.get_history();
        assert_eq!(history.len(), 3); // only input5 survived, input6 is excluded via search.exclude

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn editor_dispose_clears_history_listener() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history) = (&fixture.part, &fixture.history);
        history.get_history();

        let input = TestFileEditorInput::new(uri_file("history-listener"), TEST_EDITOR_INPUT_ID);
        open_pinned(&part.active_group(), &input).await;

        assert_eq!(history.editor_history_listeners_size_for_testing(), 1);

        input.dispose();
        assert_eq!(history.editor_history_listeners_size_for_testing(), 0);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn get_last_active_file() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history) = (&fixture.part, &fixture.history);

        assert!(history.get_last_active_file("foo", None).is_none());

        let input1 = new_input(&disposables, "foo://bar1");
        open_pinned(&part.active_group(), &input1).await;

        let input2 = new_input(&disposables, "foo://bar2");
        open_pinned(&part.active_group(), &input2).await;

        assert_eq!(history.get_last_active_file("foo", None).map(|resource| res(&resource)), Some(res(&input2.resource)));
        assert_eq!(history.get_last_active_file("foo", Some("bar2")).map(|resource| res(&resource)), Some(res(&input2.resource)));
        assert_eq!(history.get_last_active_file("foo", Some("bar1")).map(|resource| res(&resource)), Some(res(&input1.resource)));

        finish_without_teardown(&disposables);
    });
}

#[test]
fn open_next_previous_recently_used_editor_single_group() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);

        let input1 = new_input(&disposables, "foo://bar1");
        let input2 = new_input(&disposables, "foo://bar2");

        open_pinned(&part.active_group(), &input1).await;
        assert!(is_active(&part.active_group(), &input1));

        open_pinned(&part.active_group(), &input2).await;
        assert!(is_active(&part.active_group(), &input2));

        let mut editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_previously_used_editor(None));
        editor_change_promise.await.unwrap();
        assert!(is_active(&part.active_group(), &input1));

        editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_next_recently_used_editor(None));
        editor_change_promise.await.unwrap();
        assert!(is_active(&part.active_group(), &input2));

        editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_previously_used_editor(Some(part.active_group().id())));
        editor_change_promise.await.unwrap();
        assert!(is_active(&part.active_group(), &input1));

        editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_next_recently_used_editor(Some(part.active_group().id())));
        editor_change_promise.await.unwrap();
        assert!(is_active(&part.active_group(), &input2));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_next_previous_recently_used_editor_multi_group() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);
        let root_group = part.active_group();

        let input1 = new_input(&disposables, "foo://bar1");
        let input2 = new_input(&disposables, "foo://bar2");

        let side_group = part.add_group(Some(gref(&root_group)), GroupDirection::Right);

        open_pinned(&root_group, &input1).await;
        open_pinned(&side_group, &input2).await;

        let mut editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_previously_used_editor(None));
        editor_change_promise.await.unwrap();
        assert!(same_group(&part.active_group(), &root_group));
        assert!(is_active(&root_group, &input1));

        editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_next_recently_used_editor(None));
        editor_change_promise.await.unwrap();
        assert!(same_group(&part.active_group(), &side_group));
        assert!(is_active(&side_group, &input2));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_next_previous_recently_is_reset_when_other_input_opens() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);

        let input1 = new_input(&disposables, "foo://bar1");
        let input2 = new_input(&disposables, "foo://bar2");
        let input3 = new_input(&disposables, "foo://bar3");
        let input4 = new_input(&disposables, "foo://bar4");

        open_pinned(&part.active_group(), &input1).await;
        open_pinned(&part.active_group(), &input2).await;
        open_pinned(&part.active_group(), &input3).await;

        let mut editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_previously_used_editor(None));
        editor_change_promise.await.unwrap();
        assert!(is_active(&part.active_group(), &input2));

        timeout(0).await.unwrap();
        open_pinned(&part.active_group(), &input4).await;

        editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_previously_used_editor(None));
        editor_change_promise.await.unwrap();
        assert!(is_active(&part.active_group(), &input2));

        editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        drop(history.open_next_recently_used_editor(None));
        editor_change_promise.await.unwrap();
        assert!(is_active(&part.active_group(), &input4));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn transient_editors_suspends_editor_change_tracking() {
    run_async(|disposables| async move {
        let fixture = create_services(&disposables, GoScope::Default, false).await;
        let (part, history, service) = (&fixture.part, &fixture.history, &*fixture.service);

        let input1 = new_input(&disposables, "foo://bar1");
        let input2 = new_input(&disposables, "foo://bar2");
        let input3 = new_input(&disposables, "foo://bar3");
        let input4 = new_input(&disposables, "foo://bar4");
        let input5 = new_input(&disposables, "foo://bar5");

        let editor_change_promise = service.on_did_active_editor_change().to_promise(None).promise;
        open_pinned(&part.active_group(), &input1).await;
        assert!(is_active(&part.active_group(), &input1));
        editor_change_promise.await.unwrap();

        let transient = Some(EditorOptions { transient: Some(true), ..EditorOptions::default() });
        part.active_group().open_editor(&editor_ref(&input2), transient.clone()).await.unwrap();
        assert!(is_active(&part.active_group(), &input2));
        part.active_group().open_editor(&editor_ref(&input3), transient).await.unwrap();
        assert!(is_active(&part.active_group(), &input3));

        open_pinned(&part.active_group(), &input4).await;
        assert!(is_active(&part.active_group(), &input4));
        open_pinned(&part.active_group(), &input5).await;
        assert!(is_active(&part.active_group(), &input5));

        // stack should be [input1, input4, input5]
        history.go_back(None).await.unwrap();
        assert!(is_active(&part.active_group(), &input4));
        history.go_back(None).await.unwrap();
        assert!(is_active(&part.active_group(), &input1));
        history.go_back(None).await.unwrap();
        assert!(is_active(&part.active_group(), &input1));

        history.go_forward(None).await.unwrap();
        assert!(is_active(&part.active_group(), &input4));
        history.go_forward(None).await.unwrap();
        assert!(is_active(&part.active_group(), &input5));

        finish(&fixture, &disposables).await;
    });
}
