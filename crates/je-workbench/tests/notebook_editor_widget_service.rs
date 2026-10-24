//! Porte de `src/vs/workbench/contrib/notebook/test/browser/NotebookEditorWidgetService.test.ts` (suíte
//! `NotebookEditorWidgetService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, e o `disposables` do `setup`/`teardown` é o
//!   store dele (o `teardown(() => disposables.dispose())` é o descarte do guarda). Os casos rodam no relógio real, como os
//!   `async function` do mocha sem `runWithFakedTimers`. O corpo do original é síncrono, e o mocha deixa as microtarefas
//!   (a do `whenReady.then(...)` do serviço) esvaziarem antes do `teardown`: o [`run`] faz o mesmo com `run_microtasks()` no
//!   fim do corpo.
//! - O widget de notebook é o dublê [`StubWidget`] (o `mock<NotebookEditorWidget>` com `onWillHide`, `getDomNode` e `dispose`
//!   vazios); o `createWidget` protegido que o `TestNotebookEditorWidgetService` sobrescreve é a [`TestWidgetFactory`], que
//!   o serviço portado recebe como [`NotebookWidgetFactory`]. `createInstance(TestNotebookEditorWidgetService)` é
//!   `NotebookEditorWidgetService::new` com os quatro serviços do construtor.
//! - `setupInstantiationService(disposables)` do `testNotebookEditor.ts` registra dezenas de serviços; o caso lê só o
//!   `IContextKeyService` (o `MockContextKeyService`) e o `IEditorProgressService` (o [`StubProgressService`], que nenhum caso
//!   usa) no `accessor` de `retrieveWidget`, e são os dois que entram no [`InstantiationService`].
//! - `mock<NotebookEditorInput>` com `resource` e `typeId` é o `TestEditorInput` do workbench (recurso e tipo); o `retrieveWidget`
//!   recebe a identidade `{ resource, typeId }` como [`NotebookInputIdentity`]. As entradas e os `Emitter`s do `setup` entram
//!   no store do guarda, porque no Rust eles são descartáveis rastreados.
//! - `IEditorGroup`s com `id`, `onDidCloseEditor` e `onWillMoveEditor` são o [`StubGroup`] (`Event.None` é `Event::none()`);
//!   o `IEditorGroupsService` com `onDidRemoveGroup`, `onDidAddGroup`, `whenReady`, `groups` e `getPart` é o
//!   [`StubGroupsService`]; o `{ windowId: 0 } as IEditorPart` é o [`StubPart`]. O `IEditorService` com
//!   `onDidEditorsChange = Event.None` é o `TestEditorService` do workbench. O que o `mock<...>` deixa sem corpo lança o
//!   `TypeError` do JavaScript ([`absent`]).
//! - `widget.value` é `NotebookBorrowValue::value()` (o acessor vivo do original); `assert.strictEqual(a, b)` entre widgets é
//!   `Rc::ptr_eq` ([`same`]) e `undefined` é `None`. `await new Promise(resolve => setTimeout(resolve, 0))` é [`sleep`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{Promise, Thenable};
use je_base::runtime::run_microtasks;
use je_base::uri::URI;
use je_editor::code_action::platform::{IEditorProgressService, IProgressIndicator, IProgressRunner, ProgressTotal};
use je_editor::core_2d::dimension::Dimension;
use je_editor::range::Range;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::contextkey::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, ModalEditorPartOptions};
use je_workbench::common::editor::{EditorCloseEvent, EditorInputOrUntyped, EditorInputRef, EditorWillMoveEvent, GroupIdentifier};
use je_workbench::contrib::notebook::browser::contrib::find::find_match_decoration_model::FindMatchDecorationModel;
use je_workbench::contrib::notebook::browser::notebook_browser::{CellFindMatchWithIndex, ICellViewModel, INotebookFindOptions};
use je_workbench::contrib::notebook::browser::notebook_editor_widget::{NotebookEditorWidget, NotebookWidgetDomNode};
use je_workbench::contrib::notebook::browser::services::notebook_editor_service_impl::{
    EditorGroupProgressService, I_EDITOR_PROGRESS_SERVICE, NOTEBOOK_EDITOR_INPUT_ID, NotebookBorrowValue, NotebookEditorWidgetService, NotebookEditorWidgetServiceDeps,
    NotebookInputIdentity, NotebookWidgetFactory,
};
use je_workbench::contrib::notebook::common::notebook_common::INotebookTextModel;
use je_workbench::services::editor::editor_groups_service::{
    FindGroupScope, GroupDirection, GroupRef, GroupsOrder, IEditorGroup, IEditorGroupsService, IEditorPart, IModalEditorPart,
};
use je_workbench::services::layout::layout_service::WindowId;
use je_workbench::test::browser::workbench_test_services::{TestEditorInput, TestEditorService};

/// O que um `mock<...>` deixa sem corpo: chamar é um `TypeError`.
fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `new Promise(resolve => setTimeout(resolve, 0))`.
async fn sleep(ms: u64) {
    timeout(ms).await.expect("timeout");
}

/// O widget de teste: `new class extends mock<NotebookEditorWidget>() { onWillHide, getDomNode, dispose }`.
struct StubWidget;

/// `{ remove: () => { } } as HTMLElement`.
struct StubDomNode;

impl NotebookWidgetDomNode for StubDomNode {
    fn remove(&self) {}
}

impl NotebookEditorWidget for StubWidget {
    fn get_id(&self) -> String {
        absent("getId")
    }

    fn text_model(&self) -> Option<Rc<dyn INotebookTextModel>> {
        absent("textModel")
    }

    fn has_model(&self) -> bool {
        absent("hasModel")
    }

    fn on_will_change_model(&self) -> Event<Option<Rc<dyn INotebookTextModel>>> {
        absent("onWillChangeModel")
    }

    fn on_did_attach_view_model(&self) -> Event<()> {
        absent("onDidAttachViewModel")
    }

    fn get_cell_index(&self, _cell: &Rc<dyn ICellViewModel>) -> Option<usize> {
        absent("getCellIndex")
    }

    fn reveal_cell_offset_in_center(&self, _cell: &Rc<dyn ICellViewModel>, _offset: f64) {
        absent("revealCellOffsetInCenter")
    }

    fn set_cell_editor_selection(&self, _cell: &Rc<dyn ICellViewModel>, _range: &Range) {
        absent("setCellEditorSelection")
    }

    fn reveal_range_in_center_if_outside_viewport_async(&self, _cell: &Rc<dyn ICellViewModel>, _range: &Range) -> Promise<()> {
        absent("revealRangeInCenterIfOutsideViewportAsync")
    }

    fn find(
        &self,
        _query: &str,
        _options: &INotebookFindOptions,
        _token: CancellationToken,
        _skip_wrapping: bool,
        _skip_reveal: bool,
        _search_instance_id: &str,
    ) -> Promise<Vec<CellFindMatchWithIndex>> {
        absent("find")
    }

    fn create_find_match_decoration_model(&self, _search_instance_id: &str) -> Rc<dyn FindMatchDecorationModel> {
        absent("createFindMatchDecorationModel")
    }

    fn on_will_hide(&self) {}

    fn get_dom_node(&self) -> Rc<dyn NotebookWidgetDomNode> {
        Rc::new(StubDomNode)
    }

    fn dispose_widget(&self) {}
}

/// `protected override createWidget(): NotebookEditorWidget { return new class extends mock<NotebookEditorWidget>() { ... }; }`.
struct TestWidgetFactory;

impl NotebookWidgetFactory for TestWidgetFactory {
    fn create_widget(
        &self,
        _instantiation_service: &InstantiationService,
        _editor_group_context_key_service: Rc<dyn IContextKeyService>,
        _widget_dispose_store: &DisposableStore,
        _editor_group_editor_progress_service: Rc<dyn EditorGroupProgressService>,
        _initial_dimension: Option<Dimension>,
    ) -> Rc<dyn NotebookEditorWidget> {
        Rc::new(StubWidget)
    }
}

/// O `IEditorProgressService` que o `accessor` entrega; nenhum caso o usa.
struct StubProgressService;

impl IProgressIndicator for StubProgressService {
    fn show(&self, _total: ProgressTotal, _delay: Option<u64>) -> Rc<dyn IProgressRunner> {
        absent("show")
    }

    fn show_while(&self, _promise: &dyn Thenable, _delay: Option<u64>) -> Promise<()> {
        absent("showWhile")
    }
}

impl IEditorProgressService for StubProgressService {}

/// `new class extends mock<IEditorGroup>() { id; onDidCloseEditor; onWillMoveEditor }`.
struct StubGroup {
    id: GroupIdentifier,
    on_did_close_editor: Event<EditorCloseEvent>,
    on_will_move_editor: Event<EditorWillMoveEvent>,
}

impl IEditorGroup for StubGroup {
    fn id(&self) -> GroupIdentifier {
        self.id
    }

    fn window_id(&self) -> WindowId {
        absent("windowId")
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        absent("activeEditor")
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        absent("editors")
    }

    fn is_locked(&self) -> bool {
        absent("isLocked")
    }

    fn is_active(&self, _editor: &EditorInputOrUntyped) -> bool {
        absent("isActive")
    }

    fn on_did_close_editor(&self) -> Event<EditorCloseEvent> {
        self.on_did_close_editor.clone()
    }

    fn on_will_move_editor(&self) -> Event<EditorWillMoveEvent> {
        self.on_will_move_editor.clone()
    }
}

/// `{ windowId: 0 } as IEditorPart`.
struct StubPart;

impl IEditorPart for StubPart {
    fn window_id(&self) -> WindowId {
        WindowId(0)
    }

    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        absent("activeGroup")
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        absent("groups")
    }
}

/// `new class extends mock<IEditorGroupsService>() { onDidRemoveGroup; onDidAddGroup; whenReady; groups; getPart }`.
struct StubGroupsService {
    on_did_remove_group: Event<Rc<dyn IEditorGroup>>,
    groups: Vec<Rc<dyn IEditorGroup>>,
}

impl IEditorGroupsService for StubGroupsService {
    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        absent("activeGroup")
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        self.groups.clone()
    }

    fn get_groups(&self, _order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        absent("getGroups")
    }

    fn get_group(&self, _identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        absent("getGroup")
    }

    fn find_group(&self, _scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        absent("findGroup")
    }

    fn add_group(&self, _location: Option<GroupRef>, _direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        absent("addGroup")
    }

    fn main_part(&self) -> Rc<dyn IEditorPart> {
        absent("mainPart")
    }

    fn create_auxiliary_editor_part(&self, _options: Option<AuxiliaryEditorOptions>) -> Promise<Rc<dyn IEditorPart>> {
        absent("createAuxiliaryEditorPart")
    }

    fn create_modal_editor_part(&self, _options: Option<ModalEditorPartOptions>) -> Promise<Rc<dyn IModalEditorPart>> {
        absent("createModalEditorPart")
    }

    fn active_modal_editor_part(&self) -> Option<Rc<dyn IModalEditorPart>> {
        absent("activeModalEditorPart")
    }

    fn on_did_remove_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.on_did_remove_group.clone()
    }

    fn when_ready(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_part(&self, _group: GroupRef) -> Rc<dyn IEditorPart> {
        Rc::new(StubPart)
    }
}

/// `createNotebookInput(path, editorType)`: a entrada com `resource` e `typeId`, registrada no store do guarda.
fn create_notebook_input(store: &DisposableStore, path: &str, editor_type: &str) -> Rc<TestEditorInput> {
    store.add_rc(TestEditorInput::new(URI::parse(path, false), editor_type))
}

fn identity(input: &Rc<TestEditorInput>) -> NotebookInputIdentity {
    let editor: EditorInputRef = input.clone();
    NotebookInputIdentity::of(&editor).expect("a entrada tem recurso")
}

/// O `setup()` da suíte mais o `createInstance(TestNotebookEditorWidgetService)`: o serviço, o serviço de instanciação (o
/// `accessor` de `retrieveWidget`) e os eventos que os casos disparam.
struct Fixture {
    service: Rc<NotebookEditorWidgetService>,
    instantiation_service: Rc<InstantiationService>,
    on_did_remove_group: Rc<Emitter<Rc<dyn IEditorGroup>>>,
    on_will_move_editor: Rc<Emitter<EditorWillMoveEvent>>,
    editor_group1: Rc<dyn IEditorGroup>,
}

impl Fixture {
    /// `notebookEditorService.retrieveWidget(instantiationService, groupId, input)`.
    fn retrieve_widget(&self, group_id: GroupIdentifier, input: &NotebookInputIdentity) -> NotebookBorrowValue {
        self.instantiation_service.invoke_function(|accessor| self.service.retrieve_widget(accessor, group_id, input, None))
    }
}

fn setup(store: &Rc<DisposableStore>) -> Fixture {
    let on_did_remove_group = store.add_rc(Emitter::<Rc<dyn IEditorGroup>>::new());
    let on_did_close_editor = store.add_rc(Emitter::<EditorCloseEvent>::new());
    let on_will_move_editor = store.add_rc(Emitter::<EditorWillMoveEvent>::new());

    let editor_group1: Rc<dyn IEditorGroup> =
        Rc::new(StubGroup { id: 1, on_did_close_editor: on_did_close_editor.event(), on_will_move_editor: on_will_move_editor.event() });
    let editor_group2: Rc<dyn IEditorGroup> = Rc::new(StubGroup { id: 2, on_did_close_editor: Event::none(), on_will_move_editor: Event::none() });

    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    let progress_service: Rc<dyn EditorGroupProgressService> = Rc::new(StubProgressService);
    let instantiation_service =
        store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONTEXT_KEY_SERVICE, context_key_service.clone()).with(I_EDITOR_PROGRESS_SERVICE, progress_service)));

    let service = store.add_rc(NotebookEditorWidgetService::new(NotebookEditorWidgetServiceDeps {
        editor_group_service: Rc::new(StubGroupsService { on_did_remove_group: on_did_remove_group.event(), groups: vec![editor_group1.clone(), editor_group2] }),
        editor_service: Rc::new(TestEditorService),
        context_key_service,
        instantiation_service: instantiation_service.clone(),
        widget_factory: Rc::new(TestWidgetFactory),
        composite_editor_inputs: Rc::new(|_| None),
    }));
    Fixture { service, instantiation_service, on_did_remove_group, on_will_move_editor, editor_group1 }
}

/// O caso inteiro: o guarda de vazamentos, o `setup`, o corpo e, antes do `teardown`, as microtarefas pendentes.
fn run<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>, Fixture) -> F + 'static) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let store = guard.store();
        let fixture = setup(&store);
        body(store, fixture).await;
        run_microtasks();
    });
}

fn same(a: &Option<Rc<dyn NotebookEditorWidget>>, b: &Option<Rc<dyn NotebookEditorWidget>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

#[test]
fn retrieve_widget_within_group() {
    run(|store, fixture| async move {
        let input = identity(&create_notebook_input(&store, "/test.np", "type1"));
        let widget = fixture.retrieve_widget(1, &input);
        let value = widget.value();
        let widget2 = fixture.retrieve_widget(1, &input);

        assert!(widget2.value().is_some(), "should create a widget");
        assert!(same(&value, &widget2.value()), "should return the same widget");
        assert!(widget.value().is_none(), "initial borrow should no longer have widget");
    });
}

#[test]
fn retrieve_independent_widgets() {
    run(|store, fixture| async move {
        let input_type1 = identity(&create_notebook_input(&store, "/test.np", "type1"));
        let input_type2 = identity(&create_notebook_input(&store, "/test.np", "type2"));
        let widget = fixture.retrieve_widget(1, &input_type1);
        let widget_diff_group = fixture.retrieve_widget(2, &input_type1);
        let widget_diff_type = fixture.retrieve_widget(1, &input_type2);

        assert!(widget.value().is_some(), "should create a widget");
        assert!(widget_diff_group.value().is_some(), "should create a widget");
        assert!(widget_diff_type.value().is_some(), "should create a widget");
        assert!(!same(&widget.value(), &widget_diff_group.value()), "should return a different widget");
        assert!(!same(&widget.value(), &widget_diff_type.value()), "should return a different widget");
    });
}

#[test]
fn only_relevant_widgets_get_disposed() {
    run(|store, fixture| async move {
        let input_type1 = identity(&create_notebook_input(&store, "/test.np", "type1"));
        let input_type2 = identity(&create_notebook_input(&store, "/test.np", "type2"));
        let widget = fixture.retrieve_widget(1, &input_type1);
        let widget_diff_type = fixture.retrieve_widget(1, &input_type2);
        let widget_diff_group = fixture.retrieve_widget(2, &input_type1);

        fixture.on_did_remove_group.fire(fixture.editor_group1.clone());

        assert!(widget.value().is_none(), "widgets in group should get disposed");
        assert!(widget_diff_type.value().is_none(), "widgets in group should get disposed");
        assert!(widget_diff_group.value().is_some(), "other group should not be disposed");

        fixture.service.dispose();
    });
}

#[test]
fn widget_should_move_between_groups_when_editor_is_moved() {
    run(|store, fixture| async move {
        let input_type1 = create_notebook_input(&store, "/test.np", NOTEBOOK_EDITOR_INPUT_ID);
        let identity_type1 = identity(&input_type1);
        let initial_value = fixture.retrieve_widget(1, &identity_type1).value();

        sleep(0).await;

        let editor: EditorInputRef = input_type1.clone();
        fixture.on_will_move_editor.fire(EditorWillMoveEvent { editor, group_id: 1, target: 2 });

        let widget_diff_group = fixture.retrieve_widget(2, &identity_type1);
        let widget_first_group = fixture.retrieve_widget(1, &identity_type1);

        assert!(initial_value.is_some(), "valid widget");
        assert!(same(&widget_diff_group.value(), &initial_value), "widget should be reused in new group");
        assert!(!same(&widget_first_group.value(), &initial_value), "should create a new widget in the first group");
    });
}
