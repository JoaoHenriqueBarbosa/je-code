//! Porte de `src/vs/workbench/contrib/files/test/browser/fileCommands.test.ts` (suíte `Reveal in Explorer command`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo do caso, e o `TestInstantiationService` do
//!   `createServices` é adicionado a ele (o `store.add`).
//! - **`import '../../browser/fileCommands.js'`.** O efeito colateral do `import` é a chamada de [`register_file_commands`], antes do
//!   guarda de vazamentos.
//! - **`upcastPartial<I...>({ ... })`.** Cada dublê é uma struct que define só os membros do objeto do original; os outros
//!   lançam `TypeError: <membro> is not a function` ([`not_a_function`]) ou, nos traits do porte que já têm esse corpo
//!   padrão, o próprio corpo padrão. `isInsideWorkspace: () => true` é o [`InsideWorkspaceService`].
//! - **`calls: object[]`.** É um `Vec<Call>` com um variante por forma de objeto empurrado (`{ view, focus }`, `{ expanded }`,
//!   `{ select, reveal, autoReveal }`, `{ focus: true }`), e o `assert.deepStrictEqual` é um `assert_eq!`.
//! - **`openView`.** A `open_view` do porte devolve `Promise<()>`; a que devolve a view é `open_explorer_view`, que o dublê
//!   implementa com a mesma gravação.
//! - **`for (const viewId of [VIEW_ID, SESSIONS_FILES_VIEW_ID])`.** São dois testes sobre o mesmo corpo.
//! - **`await instantiation.invokeFunction(handler, resource)` e `assert.rejects`.** O handler devolve a promessa do corpo
//!   (`promise_from_dynamic`); o `await` esvazia as microtarefas (`runtime::run_microtasks`) e lê o resultado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_platform::commands::commands::commands_registry;
use je_platform::list::list_service::{I_LIST_SERVICE, IListService, IListWidget};
use je_platform::resources::uri_file;
use je_platform::workspace::{
    CurrentWorkspaceQuery, I_WORKSPACE_CONTEXT_SERVICE, IWorkspaceContextService, WorkbenchState, Workspace, WorkspaceFolder, WorkspaceFoldersChangeEvent,
    WorkspaceFoldersWillChangeEvent,
};
use je_workbench::api::browser::main_thread_commands::promise_from_dynamic;
use je_workbench::api::common::ext_host_values::ApiValue;
use je_workbench::common::editor::{
    EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput,
};
use je_workbench::contrib::files::browser::file_commands::{REVEAL_IN_EXPLORER_COMMAND_ID, register_file_commands};
use je_workbench::contrib::files::browser_files::{
    EditableItem, ExplorerBulkEditOptions, ExplorerReveal, I_EXPLORER_SERVICE, IExplorerService,
};
use je_workbench::contrib::files::common_files::{SESSIONS_FILES_VIEW_ID, VIEW_ID};
use je_workbench::contrib::files::explorer_model::ExplorerItem;
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, I_EDITOR_SERVICE, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::views::views_service::{I_VIEWS_SERVICE, IExplorerViewPane, IViewPane, IViewsService};
use test_instantiation_service::TestInstantiationService;

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_a_function(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// Um elemento de `calls: object[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    /// `{ view: id, focus }`.
    View { view: String, focus: bool },
    /// `{ expanded }`.
    Expanded(bool),
    /// `{ select: uri.toString(), reveal, autoReveal: view.autoReveal }`.
    Select { select: String, reveal: Option<ExplorerReveal>, auto_reveal: ExplorerReveal },
    /// `{ focus: true }`.
    Focus,
}

type Calls = Rc<RefCell<Vec<Call>>>;

/// `upcastPartial<ExplorerView>({ autoReveal: true, setExpanded, focus })`.
struct TestExplorerView {
    calls: Calls,
    auto_reveal: RefCell<ExplorerReveal>,
}

impl IViewPane for TestExplorerView {
    fn set_expanded(&self, expanded: bool) -> bool {
        self.calls.borrow_mut().push(Call::Expanded(expanded));
        true
    }

    fn focus(&self) {
        self.calls.borrow_mut().push(Call::Focus);
    }
}

impl IExplorerViewPane for TestExplorerView {
    fn auto_reveal(&self) -> ExplorerReveal {
        self.auto_reveal.borrow().clone()
    }

    fn set_auto_reveal(&self, value: ExplorerReveal) {
        *self.auto_reveal.borrow_mut() = value;
    }
}

/// `upcastPartial<IWorkspaceContextService>({ isInsideWorkspace: () => true })`.
struct InsideWorkspaceService;

impl IWorkspaceContextService for InsideWorkspaceService {
    fn on_did_change_workbench_state(&self) -> Event<WorkbenchState> {
        not_a_function("onDidChangeWorkbenchState")
    }

    fn on_did_change_workspace_name(&self) -> Event<()> {
        not_a_function("onDidChangeWorkspaceName")
    }

    fn on_will_change_workspace_folders(&self) -> Event<WorkspaceFoldersWillChangeEvent> {
        not_a_function("onWillChangeWorkspaceFolders")
    }

    fn on_did_change_workspace_folders(&self) -> Event<WorkspaceFoldersChangeEvent> {
        not_a_function("onDidChangeWorkspaceFolders")
    }

    fn get_complete_workspace(&self) -> Promise<Rc<Workspace>> {
        not_a_function("getCompleteWorkspace")
    }

    fn get_workspace(&self) -> Rc<Workspace> {
        not_a_function("getWorkspace")
    }

    fn get_workbench_state(&self) -> WorkbenchState {
        not_a_function("getWorkbenchState")
    }

    fn get_workspace_folder(&self, _resource: &URI) -> Option<WorkspaceFolder> {
        not_a_function("getWorkspaceFolder")
    }

    fn is_current_workspace(&self, _workspace_id_or_folder: &CurrentWorkspaceQuery) -> bool {
        not_a_function("isCurrentWorkspace")
    }

    fn is_inside_workspace(&self, _resource: &URI) -> bool {
        true
    }

    fn has_workspace_data(&self) -> bool {
        not_a_function("hasWorkspaceData")
    }
}

/// `upcastPartial<IEditorService>({})`.
struct EmptyEditorService;

impl IEditorService for EmptyEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        not_a_function("visibleEditorPanes")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        None
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

/// `upcastPartial<IListService>({})`.
struct EmptyListService;

impl IListService for EmptyListService {
    fn last_focused_list(&self) -> Option<Rc<dyn IListWidget>> {
        not_a_function("lastFocusedList")
    }
}

/// `upcastPartial<IViewsService>({ openView })`.
struct TestViewsService {
    calls: Calls,
    view: Rc<TestExplorerView>,
}

impl IViewsService for TestViewsService {
    fn open_view(&self, _id: &str, _focus: bool) -> Promise<()> {
        not_a_function("openView")
    }

    fn open_explorer_view(&self, id: &str, focus: bool) -> Promise<Option<Rc<dyn IExplorerViewPane>>> {
        self.calls.borrow_mut().push(Call::View { view: id.to_string(), focus });
        let view: Rc<dyn IExplorerViewPane> = self.view.clone();
        Promise::resolved(Some(view))
    }
}

/// `upcastPartial<IExplorerService>({ getViewId, select })`.
struct TestExplorerService {
    view_id: String,
    calls: Calls,
    view: Rc<TestExplorerView>,
    select_error: Option<Error>,
}

impl IExplorerService for TestExplorerService {
    fn roots(&self) -> Vec<Rc<ExplorerItem>> {
        not_a_function("roots")
    }

    fn get_context(&self, _respect_multi_selection: bool, _ignore_nested_children: Option<bool>) -> Vec<Rc<ExplorerItem>> {
        not_a_function("getContext")
    }

    fn get_editable(&self) -> Option<EditableItem> {
        not_a_function("getEditable")
    }

    fn find_closest(&self, _resource: &URI) -> Option<Rc<ExplorerItem>> {
        not_a_function("findClosest")
    }

    fn set_to_copy(&self, _stats: Vec<Rc<ExplorerItem>>, _cut: bool) -> Promise<()> {
        not_a_function("setToCopy")
    }

    fn apply_bulk_edit(&self, _edit: Vec<ResourceFileEdit>, _options: ExplorerBulkEditOptions) -> Promise<()> {
        not_a_function("applyBulkEdit")
    }

    fn select(&self, resource: &URI, reveal: Option<ExplorerReveal>) -> Promise<()> {
        self.calls.borrow_mut().push(Call::Select { select: resource.to_string(false), reveal, auto_reveal: self.view.auto_reveal() });
        match &self.select_error {
            Some(error) => Promise::rejected(error.clone()),
            None => Promise::resolved(()),
        }
    }

    fn get_view_id(&self) -> String {
        self.view_id.clone()
    }
}

struct Services {
    instantiation: Rc<TestInstantiationService>,
    calls: Calls,
    view: Rc<TestExplorerView>,
}

/// `createServices(viewId, selectError?)`.
fn create_services(guard: &LeakGuard, view_id: &str, select_error: Option<Error>) -> Services {
    let instantiation = guard.add_rc(TestInstantiationService::new());
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let view = Rc::new(TestExplorerView { calls: calls.clone(), auto_reveal: RefCell::new(ExplorerReveal::Flag(true)) });

    let workspace_service: Rc<dyn IWorkspaceContextService> = Rc::new(InsideWorkspaceService);
    instantiation.stub(I_WORKSPACE_CONTEXT_SERVICE, workspace_service);
    let editor_service: Rc<dyn IEditorService> = Rc::new(EmptyEditorService);
    instantiation.stub(I_EDITOR_SERVICE, editor_service);
    let list_service: Rc<dyn IListService> = Rc::new(EmptyListService);
    instantiation.stub(I_LIST_SERVICE, list_service);
    let views_service: Rc<dyn IViewsService> = Rc::new(TestViewsService { calls: calls.clone(), view: view.clone() });
    instantiation.stub(I_VIEWS_SERVICE, views_service);
    let explorer_service: Rc<dyn IExplorerService> =
        Rc::new(TestExplorerService { view_id: view_id.to_string(), calls: calls.clone(), view: view.clone(), select_error });
    instantiation.stub(I_EXPLORER_SERVICE, explorer_service);
    Services { instantiation, calls, view }
}

/// `instantiation.invokeFunction(CommandsRegistry.getCommand(REVEAL_IN_EXPLORER_COMMAND_ID)!.handler, resource)`, esperado.
fn run_reveal_command(instantiation: &TestInstantiationService, resource: &URI) -> Result<(), Error> {
    let handler = commands_registry().get_command(REVEAL_IN_EXPLORER_COMMAND_ID).expect("o comando revealInExplorer está registrado").handler;
    let args = [resource.to_dynamic()];
    let result = instantiation.invoke_function(|accessor| handler(accessor, &args));
    let promise = promise_from_dynamic(&result).expect("o handler devolve a promessa do corpo");
    runtime::run_microtasks();
    promise.result().expect("o comando assentou").map(|_| ())
}

fn reveals_in_the_applications_explorer(view_id: &str) {
    register_file_commands();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = uri_file("/workspace/folder");
    let Services { instantiation, calls, view } = create_services(&guard, view_id, None);

    run_reveal_command(&instantiation, &resource).expect("o comando resolveu");

    assert_eq!(
        *calls.borrow(),
        vec![
            Call::View { view: view_id.to_string(), focus: false },
            Call::Expanded(true),
            Call::Select { select: resource.to_string(false), reveal: Some(ExplorerReveal::Mode("force".to_string())), auto_reveal: ExplorerReveal::Flag(false) },
            Call::Focus,
        ]
    );
    assert_eq!(view.auto_reveal(), ExplorerReveal::Flag(true));
}

#[test]
fn reveals_in_the_applications_explorer_workbench_explorer_file_view() {
    reveals_in_the_applications_explorer(VIEW_ID);
}

#[test]
fn reveals_in_the_applications_explorer_sessions_files_explorer() {
    reveals_in_the_applications_explorer(SESSIONS_FILES_VIEW_ID);
}

#[test]
fn restores_auto_reveal_when_selection_fails() {
    register_file_commands();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = uri_file("/workspace/folder");
    let error = Error::new("Selection failed");
    let Services { instantiation, view, .. } = create_services(&guard, SESSIONS_FILES_VIEW_ID, Some(error.clone()));

    let rejection = run_reveal_command(&instantiation, &resource).expect_err("o comando rejeitou");

    assert_eq!(format!("{rejection:?}"), format!("{error:?}"));
    assert_eq!(view.auto_reveal(), ExplorerReveal::Flag(true));
}
