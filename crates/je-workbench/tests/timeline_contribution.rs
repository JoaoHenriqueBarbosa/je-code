//! Porte de `src/vs/workbench/contrib/timeline/test/browser/timeline.contribution.test.ts` (suíte `Open Timeline command`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo do caso. O `disposables` de `runCommand`
//!   é um `DisposableStore` próprio, descartado depois da chamada (o `finally` do original).
//! - **`import '../../browser/timeline.contribution.js'`.** O efeito colateral do `import` é a chamada de
//!   [`register_timeline_contribution`], antes do guarda de vazamentos.
//! - **`upcastPartial<I...>({ ... })`.** Cada dublê é uma struct que define só os membros do objeto do original; os outros
//!   lançam `TypeError: <membro> is not a function` ([`not_a_function`]), como o objeto parcial faz ao ser chamado.
//!   `lastFocusedList: undefined` é o [`IListService`] sem lista; `activeEditor` é o [`TestEditorInput`] (ou `undefined`).
//! - **`openView: async id => { calls.push(...); return null; }`.** A `open_view` do porte devolve `Promise<()>`
//!   (`IViewsService` só tem esse membro), então o `null` do retorno não aparece.
//! - **`await instantiationService.invokeFunction(handler, argument)`.** O handler devolve a promessa do corpo
//!   (`promise_from_dynamic`); o `await` esvazia as microtarefas (`runtime::run_microtasks`) e confere que ela resolveu.
//! - **`uri?.path`.** É o caminho do `URI` ou o texto `undefined`, como na interpolação do original.
//! - **`assert.deepStrictEqual({ ... }, { ... })`.** São três `assert_eq!` sobre as listas de chamadas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_platform::commands::commands::commands_registry;
use je_platform::list::list_service::{I_LIST_SERVICE, IListService, IListWidget};
use je_platform::resources::uri_file;
use je_workbench::api::browser::main_thread_commands::promise_from_dynamic;
use je_workbench::api::common::ext_host_values::ApiValue;
use je_workbench::common::editor::{
    EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput,
};
use je_workbench::contrib::files::browser_files::EditableData;
use je_workbench::contrib::timeline::browser::timeline_contribution::{OPEN_TIMELINE_ACTION_ID, register_timeline_contribution};
use je_workbench::contrib::timeline::common::timeline::{
    I_TIMELINE_SERVICE, ITimelineService, TIMELINE_PANE_ID, TimelineChangeEvent, TimelineEditable, TimelineItemIdentifier, TimelineOptions,
    TimelineProvider, TimelineProvidersChangeEvent, TimelineRequest, TimelineSource, WorkspaceTimelineProvider,
};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, I_EDITOR_SERVICE, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::views::views_service::{I_VIEWS_SERVICE, IViewsService};
use je_workbench::test::browser::workbench_test_services::TestEditorInput;
use test_instantiation_service::TestInstantiationService;

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_a_function(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `upcastPartial<IListService>({ lastFocusedList: undefined })`.
struct NoFocusedListService;

impl IListService for NoFocusedListService {
    fn last_focused_list(&self) -> Option<Rc<dyn IListWidget>> {
        None
    }
}

/// `upcastPartial<IEditorService>({ activeEditor })`.
struct ActiveEditorService {
    active_editor: Option<EditorInputOrUntyped>,
}

impl IEditorService for ActiveEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        not_a_function("visibleEditorPanes")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        self.active_editor.clone()
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        not_a_function("activeTextEditorControl")
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        not_a_function("activeTextEditorLanguageId")
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        not_a_function("openEditor")
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        not_a_function("findEditors")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        not_a_function("save")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        not_a_function("saveAll")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        not_a_function("getEditors")
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        not_a_function("openEditors")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        not_a_function("isVisible")
    }
}

/// `upcastPartial<IViewsService>({ openView })`.
struct RecordingViewsService {
    calls: Rc<RefCell<Vec<String>>>,
}

impl IViewsService for RecordingViewsService {
    fn open_view(&self, id: &str, _focus: bool) -> Promise<()> {
        self.calls.borrow_mut().push(format!("openView {id}"));
        Promise::resolved(())
    }
}

/// `upcastPartial<ITimelineService>({ setUri })`.
struct RecordingTimelineService {
    calls: Rc<RefCell<Vec<String>>>,
}

impl ITimelineService for RecordingTimelineService {
    fn on_did_change_providers(&self) -> Event<TimelineProvidersChangeEvent> {
        not_a_function("onDidChangeProviders")
    }

    fn on_did_change_timeline(&self) -> Event<TimelineChangeEvent> {
        not_a_function("onDidChangeTimeline")
    }

    fn on_did_change_uri(&self) -> Event<Option<URI>> {
        not_a_function("onDidChangeUri")
    }

    fn on_did_change_editable(&self) -> Event<()> {
        not_a_function("onDidChangeEditable")
    }

    fn editable(&self) -> Option<TimelineEditable> {
        not_a_function("editable")
    }

    fn register_timeline_provider(&self, _provider: Rc<dyn TimelineProvider>) -> DisposableRef {
        not_a_function("registerTimelineProvider")
    }

    fn unregister_timeline_provider(&self, _id: &str) {
        not_a_function("unregisterTimelineProvider")
    }

    fn register_workspace_timeline_provider(&self, _provider: Rc<dyn WorkspaceTimelineProvider>) -> DisposableRef {
        not_a_function("registerWorkspaceTimelineProvider")
    }

    fn unregister_workspace_timeline_provider(&self, _id: &str) {
        not_a_function("unregisterWorkspaceTimelineProvider")
    }

    fn get_sources(&self) -> Vec<TimelineSource> {
        not_a_function("getSources")
    }

    fn has_workspace_providers(&self) -> bool {
        not_a_function("hasWorkspaceProviders")
    }

    fn get_timeline(&self, _id: &str, _uri: Option<&URI>, _options: &TimelineOptions, _token_source: CancellationTokenSource) -> Option<TimelineRequest> {
        not_a_function("getTimeline")
    }

    fn set_uri(&self, uri: Option<URI>) -> Promise<()> {
        let path = uri.as_ref().map_or_else(|| "undefined".to_string(), |uri| uri.path().to_string_lossy().to_string());
        self.calls.borrow_mut().push(format!("setUri {path}"));
        Promise::resolved(())
    }

    fn set_editable(&self, _item: TimelineItemIdentifier, _data: Option<EditableData>) {
        not_a_function("setEditable")
    }
}

/// `runCommand(options)`.
fn run_command(argument: Option<URI>, active_editor: Option<&str>) -> Vec<String> {
    let disposables = DisposableStore::new();
    let calls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let instantiation_service = disposables.add_rc(TestInstantiationService::new());

    let list_service: Rc<dyn IListService> = Rc::new(NoFocusedListService);
    instantiation_service.stub(I_LIST_SERVICE, list_service);

    let active_editor = active_editor.map(|path| {
        let input = disposables.add_rc(TestEditorInput::new(uri_file(path), "testEditor"));
        EditorInputOrUntyped::Typed(input as EditorInputRef)
    });
    let editor_service: Rc<dyn IEditorService> = Rc::new(ActiveEditorService { active_editor });
    instantiation_service.stub(I_EDITOR_SERVICE, editor_service);

    let views_service: Rc<dyn IViewsService> = Rc::new(RecordingViewsService { calls: calls.clone() });
    instantiation_service.stub(I_VIEWS_SERVICE, views_service);

    let timeline_service: Rc<dyn ITimelineService> = Rc::new(RecordingTimelineService { calls: calls.clone() });
    instantiation_service.stub(I_TIMELINE_SERVICE, timeline_service);

    let handler = commands_registry().get_command(OPEN_TIMELINE_ACTION_ID).expect("o comando files.openTimeline está registrado").handler;
    let args = [argument.map_or(Dynamic::Undefined, |argument| argument.to_dynamic())];
    let result = instantiation_service.invoke_function(|accessor| handler(accessor, &args));
    let promise = promise_from_dynamic(&result).expect("o handler devolve a promessa do corpo");
    runtime::run_microtasks();
    assert!(matches!(promise.result(), Some(Ok(_))), "o comando resolveu");

    disposables.dispose();
    let calls = calls.borrow().clone();
    calls
}

#[test]
fn resolves_the_resource_from_the_argument_or_the_active_editor() {
    register_timeline_contribution();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let argument = run_command(Some(uri_file("/workspace/argument.txt")), Some("/workspace/active.txt"));
    let active_editor = run_command(None, Some("/workspace/active.txt"));
    let no_resource = run_command(None, None);

    assert_eq!(argument, vec!["setUri /workspace/argument.txt".to_string()]);
    assert_eq!(active_editor, vec!["setUri /workspace/active.txt".to_string()]);
    assert_eq!(no_resource, vec![format!("openView {TIMELINE_PANE_ID}")]);
}
