//! Porte de `src/vs/workbench/api/test/browser/mainThreadEditorTabs.test.ts` (suíte `MainThreadEditorTabs`), caso a caso e na mesma ordem.
//!
//! Casos de fora:
//! - `multi diff tab whose resources changed does not become the active tab`: exige o `MultiDiffEditorInput` (com o
//!   `IMultiDiffSourceResolverService`, o `MultiDiffEditorItem`, o observável `resources` e o `createCodeEditorServices`) e o mapa
//!   `_multiDiffEditorInputListeners` do `MainThreadEditorTabs`, que ainda não têm porte (ver o cabeçalho de
//!   `api/browser/main_thread_editor_tabs.rs`). Não vira `#[test]` até a classe existir.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (`disposables` do original é o guarda).
//! - `new class extends mock<T>() { ... }` vira os dublês locais abaixo: o membro que o original não sobrescreve falha com `unreachable!`
//!   (o `TypeError` do `mock`), e os eventos não sobrescritos (`Event.None`) são `Event::none()` (o padrão do trait).
//! - `override readonly groups` com leitura contada (`groupsReadCount++`) é um `Cell` lido por `groups()`.
//! - `SingleProxyRPCProtocol({})` é o contexto de [`UnusedExtHost`] (o proxy do host que falha ao ser chamado, como o objeto vazio); o
//!   `SingleProxyRPCProtocol(extHostEditorTabs)` é o `ExtHostEditorTabs` real servindo de proxy.
//! - `class NamedEditorInput extends TestEditorInput` é o [`NamedEditorInput`], que repete o `TestEditorInput` (os campos dele são
//!   privados) com `getName` e `isDirty` lidos de células. `editor === activeEditor` é a identidade do `Rc`.
//! - `await Promise.resolve()` é `Promise::resolved(()).await`, dentro de `run_with_faked_timers` (as microtarefas só andam ali).
//! - `extHostEditorTabs.tabGroups.activeTabGroup.activeTab?.label` é `active_tab().map(|tab| tab.label())`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/test_rpc_protocol.rs"]
mod test_rpc_protocol;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, ModalEditorPartOptions};
use je_workbench::api::browser::main_thread_editor_tabs::MainThreadEditorTabs;
use je_workbench::api::common::ext_host_editor_tabs::ExtHostEditorTabs;
use je_workbench::api::common::ext_host_protocol::{
    EditorGroupColumn, EditorTabGroupDto, ExtHostEditorTabsShape, IMainContext, MainThreadEditorTabsShape, TabOperation,
};
use je_workbench::common::editor::editor_group_model::EditorOrIndex;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::{
    EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, GroupIdentifier, GroupModelChangeKind, IEditorPane, UntypedEditorInput,
};
use je_workbench::common::editor::editor_group_model::GroupModelChangeEvent;
use je_workbench::services::editor::editor_groups_service::{
    FindGroupScope, GroupDirection, GroupRef, GroupsOrder, IEditorGroup, IEditorGroupsService, IEditorPart, IModalEditorPart,
};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, EditorsChangeEvent, FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::layout::layout_service::WindowId;
use test_rpc_protocol::single_proxy_rpc_protocol;

// ---- dublês

/// `SingleProxyRPCProtocol({})`: o proxy do host de extensões sem nenhum método.
struct UnusedExtHost;

impl ExtHostEditorTabsShape for UnusedExtHost {
    fn rpc_accept_editor_tab_model(&self, _tab_groups: Vec<EditorTabGroupDto>) {
        unreachable!("$acceptEditorTabModel is not a function")
    }

    fn rpc_accept_tab_group_update(&self, _group_dto: EditorTabGroupDto) {
        unreachable!("$acceptTabGroupUpdate is not a function")
    }

    fn rpc_accept_tab_operation(&self, _operation: TabOperation) {
        unreachable!("$acceptTabOperation is not a function")
    }
}

/// `new class extends mock<MainThreadEditorTabsShape>() { }`.
struct UnusedMainThreadEditorTabs;

impl IDisposable for UnusedMainThreadEditorTabs {
    fn dispose(&self) {}
}

impl MainThreadEditorTabsShape for UnusedMainThreadEditorTabs {
    fn rpc_move_tab(&self, _tab_id: &str, _index: usize, _view_column: EditorGroupColumn, _preserve_focus: Option<bool>) {
        unreachable!("$moveTab is not a function")
    }

    fn rpc_close_tab(&self, _tab_ids: Vec<String>, _preserve_focus: Option<bool>) -> Promise<bool> {
        unreachable!("$closeTab is not a function")
    }

    fn rpc_close_group(&self, _group_ids: Vec<u32>, _preserve_focus: Option<bool>) -> Promise<bool> {
        unreachable!("$closeGroup is not a function")
    }
}

/// `new class extends mock<IEditorGroup>() { override readonly id = 2; }`.
struct ModalGroup {
    id: GroupIdentifier,
}

impl IEditorGroup for ModalGroup {
    fn id(&self) -> GroupIdentifier {
        self.id
    }

    fn window_id(&self) -> WindowId {
        unreachable!("mock<IEditorGroup>: windowId não é definido")
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        unreachable!("mock<IEditorGroup>: activeEditor não é definido")
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        unreachable!("mock<IEditorGroup>: editors não é definido")
    }

    fn is_locked(&self) -> bool {
        unreachable!("mock<IEditorGroup>: isLocked não é definido")
    }

    fn is_active(&self, _editor: &EditorInputOrUntyped) -> bool {
        unreachable!("mock<IEditorGroup>: isActive não é definido")
    }
}

/// O grupo dos casos que montam abas: `id`, `editors`, `isSticky`, `isPinned` e `isActive` respondem, o resto falha.
struct StateGroup {
    id: GroupIdentifier,
    editors: Vec<EditorInputRef>,
    sticky: Rc<Cell<bool>>,
    pinned: Rc<Cell<bool>>,
    active_editor: Rc<RefCell<EditorInputRef>>,
}

impl IEditorGroup for StateGroup {
    fn id(&self) -> GroupIdentifier {
        self.id
    }

    fn window_id(&self) -> WindowId {
        unreachable!("mock<IEditorGroup>: windowId não é definido")
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        unreachable!("mock<IEditorGroup>: activeEditor não é definido")
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        self.editors.clone()
    }

    fn is_locked(&self) -> bool {
        unreachable!("mock<IEditorGroup>: isLocked não é definido")
    }

    fn is_active(&self, editor: &EditorInputOrUntyped) -> bool {
        match editor {
            EditorInputOrUntyped::Typed(editor) => std::ptr::addr_eq(Rc::as_ptr(editor), Rc::as_ptr(&*self.active_editor.borrow())),
            EditorInputOrUntyped::Untyped(_) => false,
        }
    }

    fn is_sticky(&self, _editor_or_index: &EditorOrIndex) -> bool {
        self.sticky.get()
    }

    fn is_pinned(&self, _editor_or_index: &EditorOrIndex) -> bool {
        self.pinned.get()
    }
}

/// `new class extends mock<IModalEditorPart>() { override readonly groups = [modalGroup]; }`.
struct ModalEditorPart {
    groups: Vec<Rc<dyn IEditorGroup>>,
}

impl IEditorPart for ModalEditorPart {
    fn window_id(&self) -> WindowId {
        unreachable!("mock<IModalEditorPart>: windowId não é definido")
    }

    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        unreachable!("mock<IModalEditorPart>: activeGroup não é definido")
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        self.groups.clone()
    }
}

impl IModalEditorPart for ModalEditorPart {
    fn close(&self) -> bool {
        unreachable!("mock<IModalEditorPart>: close não é definido")
    }
}

/// `new class extends mock<IEditorGroupsService>() { ... }`: cada membro que o caso sobrescreve é um campo, o resto falha.
#[derive(Default)]
struct EditorGroupsServiceStub {
    active_modal_editor_part: Option<Rc<dyn IModalEditorPart>>,
    /// `get groups()`.
    groups: Option<Box<dyn Fn() -> Vec<Rc<dyn IEditorGroup>>>>,
    /// `getGroups()`.
    get_groups: Option<Box<dyn Fn() -> Vec<Rc<dyn IEditorGroup>>>>,
    /// `get activeGroup()`.
    active_group: Option<Box<dyn Fn() -> Rc<dyn IEditorGroup>>>,
    /// `getGroup()`.
    get_group: Option<Box<dyn Fn(GroupIdentifier) -> Option<Rc<dyn IEditorGroup>>>>,
}

impl IEditorGroupsService for EditorGroupsServiceStub {
    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        (self.active_group.as_ref().expect("mock<IEditorGroupsService>: activeGroup não é definido"))()
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        (self.groups.as_ref().expect("mock<IEditorGroupsService>: groups não é definido"))()
    }

    fn get_groups(&self, _order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        (self.get_groups.as_ref().expect("mock<IEditorGroupsService>: getGroups não é definido"))()
    }

    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        (self.get_group.as_ref().expect("mock<IEditorGroupsService>: getGroup não é definido"))(identifier)
    }

    fn find_group(&self, _scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        unreachable!("mock<IEditorGroupsService>: findGroup não é definido")
    }

    fn add_group(&self, _location: Option<GroupRef>, _direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        unreachable!("mock<IEditorGroupsService>: addGroup não é definido")
    }

    fn main_part(&self) -> Rc<dyn IEditorPart> {
        unreachable!("mock<IEditorGroupsService>: mainPart não é definido")
    }

    fn create_auxiliary_editor_part(&self, _options: Option<AuxiliaryEditorOptions>) -> Promise<Rc<dyn IEditorPart>> {
        unreachable!("mock<IEditorGroupsService>: createAuxiliaryEditorPart não é definido")
    }

    fn create_modal_editor_part(&self, _options: Option<ModalEditorPartOptions>) -> Promise<Rc<dyn IModalEditorPart>> {
        unreachable!("mock<IEditorGroupsService>: createModalEditorPart não é definido")
    }

    fn active_modal_editor_part(&self) -> Option<Rc<dyn IModalEditorPart>> {
        self.active_modal_editor_part.clone()
    }

    // `override readonly onDidAddGroup = Event.None;` e `onDidRemoveGroup`: o padrão do trait.

    fn when_ready(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `new class extends mock<IEditorService>() { override readonly onDidEditorsChange = editorChanges.event; }`.
struct EditorServiceStub {
    on_did_editors_change: Event<EditorsChangeEvent>,
}

impl IEditorService for EditorServiceStub {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        unreachable!("mock<IEditorService>: visibleEditorPanes não é definido")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        unreachable!("mock<IEditorService>: activeEditor não é definido")
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        unreachable!("mock<IEditorService>: activeTextEditorControl não é definido")
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        unreachable!("mock<IEditorService>: activeTextEditorLanguageId não é definido")
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        unreachable!("mock<IEditorService>: openEditor não é definido")
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        unreachable!("mock<IEditorService>: findEditors não é definido")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        unreachable!("mock<IEditorService>: save não é definido")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        unreachable!("mock<IEditorService>: saveAll não é definido")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        unreachable!("mock<IEditorService>: getEditors não é definido")
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        unreachable!("mock<IEditorService>: openEditors não é definido")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        unreachable!("mock<IEditorService>: isVisible não é definido")
    }

    fn on_did_editors_change(&self) -> Event<EditorsChangeEvent> {
        self.on_did_editors_change.clone()
    }
}

/// `class NamedEditorInput extends TestEditorInput`: `getName` e `isDirty` lidos de células.
struct NamedEditorInput {
    base: EditorInputBase,
    resource: URI,
    type_id: String,
    dirty: Cell<bool>,
    name: RefCell<String>,
}

impl NamedEditorInput {
    fn new(resource: URI, type_id: &str, name: &str) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            base: EditorInputBase::new(this.clone()),
            resource,
            type_id: type_id.to_owned(),
            dirty: Cell::new(false),
            name: RefCell::new(name.to_owned()),
        })
    }

    fn set_name(&self, name: &str) {
        *self.name.borrow_mut() = name.to_owned();
    }

    fn set_dirty(&self, dirty: bool) {
        self.dirty.set(dirty);
    }
}

impl IDisposable for NamedEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for NamedEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        self.type_id.clone()
    }

    fn editor_id(&self) -> Option<String> {
        Some(self.type_id.clone())
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        Promise::resolved(None)
    }

    fn get_name(&self) -> String {
        self.name.borrow().clone()
    }

    fn is_dirty(&self) -> bool {
        self.dirty.get()
    }
}

/// `{ groupId, event: { kind, editor, editorIndex } }`.
fn editors_change(group_id: GroupIdentifier, kind: GroupModelChangeKind, editor: &EditorInputRef, editor_index: i64) -> EditorsChangeEvent {
    EditorsChangeEvent {
        group_id,
        event: GroupModelChangeEvent { kind, editor: Some(editor.clone()), editor_index: Some(editor_index), old_editor_index: None, context: None, sticky: None },
    }
}

fn single_proxy<T: ?Sized + 'static>(thing: Rc<T>) -> Rc<dyn IMainContext> {
    single_proxy_rpc_protocol(thing)
}

#[test]
fn ignores_only_missing_modal_editor_label_changes() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let modal_group: Rc<dyn IEditorGroup> = Rc::new(ModalGroup { id: 2 });
        let modal_editor_part: Rc<dyn IModalEditorPart> = Rc::new(ModalEditorPart { groups: vec![modal_group.clone()] });
        let groups_read_count = Rc::new(Cell::new(0_usize));
        let counter = groups_read_count.clone();
        let editor_groups_service = EditorGroupsServiceStub {
            active_modal_editor_part: Some(modal_editor_part),
            get_group: Some(Box::new(|_| None)),
            groups: Some(Box::new(move || {
                counter.set(counter.get() + 1);
                Vec::new()
            })),
            ..EditorGroupsServiceStub::default()
        };
        let editor_changes: Rc<Emitter<EditorsChangeEvent>> = disposables.add_rc(Emitter::new());
        let editor_service = EditorServiceStub { on_did_editors_change: editor_changes.event() };
        let input: Rc<dyn EditorInput> = disposables.add_rc(je_workbench::test::browser::workbench_test_services::TestEditorInput::new(URI::parse("test:modal", false), "testEditor"));
        disposables.add_rc(MainThreadEditorTabs::new(
            &*single_proxy::<dyn ExtHostEditorTabsShape>(Rc::new(UnusedExtHost)),
            Rc::new(editor_groups_service),
            TestConfigurationService::new(None),
            NullLogService::shared(),
            Rc::new(editor_service),
        ));
        Promise::resolved(()).await.expect("a promessa resolvida");
        groups_read_count.set(0);

        editor_changes.fire(editors_change(modal_group.id(), GroupModelChangeKind::EditorLabel, &input, 0));
        let rebuilds_after_label_change = groups_read_count.get();
        editor_changes.fire(editors_change(modal_group.id(), GroupModelChangeKind::EditorOpen, &input, 0));

        assert_eq!((rebuilds_after_label_change, groups_read_count.get()), (0, 1));
    });
}

#[test]
fn updating_a_background_tab_does_not_make_it_the_active_tab() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let input_a = disposables.add_rc(NamedEditorInput::new(URI::parse("test:a", false), "testEditor", "Panel A"));
        let input_b = disposables.add_rc(NamedEditorInput::new(URI::parse("test:b", false), "testEditor", "Panel B"));
        let editor_a: EditorInputRef = input_a.clone();
        let editor_b: EditorInputRef = input_b.clone();
        let active_editor = Rc::new(RefCell::new(editor_a.clone()));
        let sticky = Rc::new(Cell::new(false));
        let pinned = Rc::new(Cell::new(true));

        let group: Rc<dyn IEditorGroup> = Rc::new(StateGroup {
            id: 1,
            editors: vec![editor_a.clone(), editor_b.clone()],
            sticky: sticky.clone(),
            pinned: pinned.clone(),
            active_editor: active_editor.clone(),
        });
        let (groups, get_groups, active_group, get_group) = (group.clone(), group.clone(), group.clone(), group.clone());
        let editor_groups_service = EditorGroupsServiceStub {
            groups: Some(Box::new(move || vec![groups.clone()])),
            get_groups: Some(Box::new(move || vec![get_groups.clone()])),
            active_group: Some(Box::new(move || active_group.clone())),
            get_group: Some(Box::new(move |_| Some(get_group.clone()))),
            ..EditorGroupsServiceStub::default()
        };
        let editor_changes: Rc<Emitter<EditorsChangeEvent>> = disposables.add_rc(Emitter::new());
        let editor_service = EditorServiceStub { on_did_editors_change: editor_changes.event() };

        // Drive a real ext host so the assertions are made against the actual API surface
        let ext_host_editor_tabs = ExtHostEditorTabs::new(&*single_proxy::<dyn MainThreadEditorTabsShape>(Rc::new(UnusedMainThreadEditorTabs)));
        let ext_host_shape: Rc<dyn ExtHostEditorTabsShape> = ext_host_editor_tabs.clone();
        disposables.add_rc(MainThreadEditorTabs::new(
            &*single_proxy::<dyn ExtHostEditorTabsShape>(ext_host_shape),
            Rc::new(editor_groups_service),
            TestConfigurationService::new(None),
            NullLogService::shared(),
            Rc::new(editor_service),
        ));
        Promise::resolved(()).await.expect("a promessa resolvida");

        let active_tab_label = || ext_host_editor_tabs.tab_groups().active_tab_group().active_tab().map(|tab| tab.label());
        let initial = active_tab_label();

        // Tab B becomes the active tab
        *active_editor.borrow_mut() = editor_b.clone();
        editor_changes.fire(editors_change(group.id(), GroupModelChangeKind::EditorActive, &editor_b, 1));
        let after_activating_b = active_tab_label();

        // Any update to the background tab A must not steal the active tab
        input_a.set_name("Panel A (2)");
        editor_changes.fire(editors_change(group.id(), GroupModelChangeKind::EditorLabel, &editor_a, 0));
        let after_label_change = active_tab_label();

        input_a.set_dirty(true);
        editor_changes.fire(editors_change(group.id(), GroupModelChangeKind::EditorDirty, &editor_a, 0));
        let after_dirty_change = active_tab_label();

        sticky.set(true);
        editor_changes.fire(editors_change(group.id(), GroupModelChangeKind::EditorSticky, &editor_a, 0));
        let after_sticky_change = active_tab_label();

        pinned.set(false);
        editor_changes.fire(editors_change(group.id(), GroupModelChangeKind::EditorPin, &editor_a, 0));
        let after_preview_change = active_tab_label();

        let expected = |label: &str| Some(label.to_owned());
        assert_eq!(
            (initial, after_activating_b, after_label_change, after_dirty_change, after_sticky_change, after_preview_change),
            (expected("Panel A"), expected("Panel B"), expected("Panel B"), expected("Panel B"), expected("Panel B"), expected("Panel B"))
        );
    });
}
