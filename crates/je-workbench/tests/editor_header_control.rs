//! Porte de `src/vs/workbench/test/browser/parts/editor/editorHeaderControl.test.ts` (suíte `EditorHeaderControl`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **O laço do original.** O `for (const tabHeight of ['default', 'compact'] as const)` vira um `#[test]` por altura, com o título do
//!   mocha no nome (`reserves header height across tab and delayed menu changes (compact)` é
//!   `reserves_header_height_across_tab_and_delayed_menu_changes_compact`), sobre o mesmo corpo parametrizado. São 6 casos gerados
//!   pelos 5 blocos `test(...)` do arquivo.
//! - **O `createHeader`.** O `workbenchInstantiationService` é o [`Header::new`]: os serviços reais que o porte tem
//!   (`ContextKeyService`, `MenuService` com o `MenuRegistry` e o `InMemoryStorageService`, `TestConfigurationService` com
//!   `{ breadcrumbs: { enabled: false } }`, `NativeContextViewService`) e os dublês `mock<T>()` do resto, montados direto no
//!   [`EditorHeaderControlServices`]; os das migalhas vêm de `breadcrumbs_control_services`, que o teste do `BreadcrumbsControl`
//!   também usa. O `groupView` é o [`HeaderGroup`] (`activeEditor`, `activeEditorPane` indefinido, `groupsView`,
//!   `onDidActiveEditorChange` e `relayout`, que grava `control.height`), o `groupsView` é o [`HeaderGroupsView`] com
//!   `{ ...DEFAULT_EDITOR_PART_OPTIONS, tabHeight }`, e `$('.title')` é um nó solto com a classe.
//! - **`container.querySelector(...)?.style.display`.** É a busca em pré-ordem do primeiro nó com a classe, com `display` igual a `''`
//!   (visível) ou `'none'` (o `visible` falso do nó), e `undefined` quando não há nó ([`display_of`]).
//! - **`editor === first`.** É a identidade da entrada (`Rc`), como a igualdade de objetos do JavaScript.
//! - **`await timeout(100)`.** Os casos com `runWithFakedTimers({ useFakeTimers: true }, ...)` rodam em `run_with_faked_timers`; a
//!   promessa de `timeout` é esperada com `.await.unwrap()`.
//! - **`header.heights` e `new Set(...)`.** São as alturas gravadas pelo `relayout`, sem repetição e na ordem em que apareceram.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
mod editor_input_services;
mod support;
#[path = "support/breadcrumbs_control_support.rs"]
mod breadcrumbs_control_support;
#[path = "support/main_thread_support.rs"]
mod main_thread_support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[allow(dead_code)]
mod platform_actions_doubles;
#[path = "../../je-platform/tests/common/keybinding.rs"]
mod keybinding;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use breadcrumbs_control_support::{RecordingEditorService, breadcrumbs_control_services};
use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_platform::action::CommandAction;
use je_platform::actions::{AnyMenuItem, IMenuService, MenuId, MenuItem, MenuService, menu_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::ContextKeyExpr;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::storage::InMemoryStorageService;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::create_detached_el;
use je_workbench::browser::parts::editor::breadcrumbs::BreadcrumbsService;
use je_workbench::browser::parts::editor::editor::{IEditorGroupMenuIds, IEditorGroupView, IEditorGroupsView, ReserveHeaderSpace, default_editor_part_options};
use je_workbench::browser::parts::editor::editor_header_control::{EditorHeaderControl, EditorHeaderControlServices};
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::{ActiveEditorChangeEvent, EditorInputOrUntyped, EditorInputRef, GroupIdentifier};
use je_workbench::services::contextview::native_context_view_service::NativeContextViewService;
use je_workbench::services::editor::editor_groups_service::{EditorPartOptions, EditorPartOptionsChangeEvent, IEditorGroup, TabHeight};
use je_workbench::services::layout::layout_service::WindowId;
use je_workbench::test::browser::workbench_test_services::TestQuickInputService;
use je_base::uri::URI;
use keybinding::MockKeybindingService;
use notification_doubles::NoCommandService;
use platform_actions_doubles::TestTelemetryService;

fn not_defined(member: &str) -> ! {
    panic!("{member} is not a function")
}

//#region Dublês

/// `class TestHeaderEditorInput extends EditorInput { typeId = 'test.headerEditor'; resource = undefined }`.
struct TestHeaderEditorInput {
    base: EditorInputBase,
}

impl TestHeaderEditorInput {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()) })
    }
}

impl IDisposable for TestHeaderEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for TestHeaderEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        "test.headerEditor".to_owned()
    }

    fn resource(&self) -> Option<URI> {
        None
    }
}

/// `new class extends mock<IEditorGroupsView>() { partOptions = { ...DEFAULT_EDITOR_PART_OPTIONS, tabHeight }; onDidChangeEditorPartOptions = Event.None }`.
struct HeaderGroupsView {
    tab_height: TabHeight,
}

impl IEditorGroupsView for HeaderGroupsView {
    fn activate_group(&self, _group: &Rc<dyn IEditorGroupView>) {
        not_defined("activateGroup")
    }

    fn part_options(&self) -> EditorPartOptions {
        EditorPartOptions { tab_height: self.tab_height, ..default_editor_part_options() }
    }

    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent> {
        Event::none()
    }
}

/// `new class extends mock<IEditorGroupView>() { activeEditor; activeEditorPane = undefined; groupsView; onDidActiveEditorChange; relayout() }`.
struct HeaderGroup {
    active_editor: RefCell<Option<EditorInputRef>>,
    groups_view: Rc<dyn IEditorGroupsView>,
    on_did_active_editor_change: Rc<Emitter<ActiveEditorChangeEvent>>,
    /// `heights.push(control.height)`.
    on_relayout: Rc<dyn Fn()>,
}

impl IEditorGroup for HeaderGroup {
    fn id(&self) -> GroupIdentifier {
        not_defined("id")
    }

    fn window_id(&self) -> WindowId {
        not_defined("windowId")
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        self.active_editor.borrow().clone()
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        not_defined("editors")
    }

    fn is_locked(&self) -> bool {
        not_defined("isLocked")
    }

    fn is_active(&self, _editor: &EditorInputOrUntyped) -> bool {
        not_defined("isActive")
    }

    fn on_did_active_editor_change(&self) -> Event<ActiveEditorChangeEvent> {
        self.on_did_active_editor_change.event()
    }

    fn active_editor_pane(&self) -> Option<Rc<dyn je_workbench::common::editor::IEditorPane>> {
        None
    }
}

impl IEditorGroupView for HeaderGroup {
    fn groups_view(&self) -> Rc<dyn IEditorGroupsView> {
        self.groups_view.clone()
    }

    fn relayout(&self) {
        (self.on_relayout)();
    }
}

//#endregion

/// `state()`: o que o caso lê do cabeçalho.
#[derive(Debug, PartialEq)]
struct State {
    height: f64,
    display: Option<&'static str>,
    actions_display: Option<&'static str>,
}

/// `container.querySelector<HTMLElement>(selector)?.style.display`: `''`, `'none'` ou `undefined` sem o nó.
fn display_of(ui: &Ui, container: NodeId, class: &str) -> Option<&'static str> {
    let tree = ui.tree();
    let node = tree.subtree(container).into_iter().find(|node| tree.has_class(*node, class))?;
    Some(if tree.style(node).visible { "" } else { "none" })
}

/// O que `createHeader` devolve. O guarda de vazamento é o do caso (há um só por teste, que o rastreador é global).
struct Header {
    ui: Ui,
    container: NodeId,
    control: Rc<EditorHeaderControl>,
    group: Rc<HeaderGroup>,
    heights: Rc<RefCell<Vec<f64>>>,
    has_actions: Rc<dyn IContextKey>,
}

impl Header {
    /// `createHeader(options, tabHeight)`: `options.showHeader === true` e `options.reserveHeaderSpace`.
    fn new(guard: &LeakGuard, show_header: bool, reserve_header_space: Option<ReserveHeaderSpace>, tab_height: TabHeight) -> Self {
        let ui = Ui::new();
        let root = ui.root();
        ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1024.0, 768.0));

        // `workbenchInstantiationService({ configurationService: () => new TestConfigurationService({ breadcrumbs: { enabled: false } }) })`
        let configuration = TestConfigurationService::new(Some(Dynamic::object([("breadcrumbs", Dynamic::object([("enabled", Dynamic::Bool(false))]))])));
        let configuration_service: Rc<dyn IConfigurationService> = configuration.clone();
        let workbench = crate::editor_input_services::WorkbenchServices::new(guard, &configuration);

        // `store.add(instantiationService.createInstance(ContextKeyService))` e `store.add(createInstance(MenuService))`
        let context_keys = guard.add_rc(ContextKeyService::new(configuration_service.clone()));
        let context_keys: Rc<dyn IContextKeyService> = context_keys;
        let storage = guard.add_rc(InMemoryStorageService::new());
        let menu_service = guard.add_rc(MenuService::new(Rc::new(NoCommandService), Rc::new(MockKeybindingService), Rc::new(storage)));
        let menu_service: Rc<dyn IMenuService> = menu_service;

        let has_actions = context_keys.create_key("test.editorHeader.hasActions", Dynamic::Bool(false));
        let menu_id = MenuId::for_id("test.editorHeader");
        guard.add(menu_registry().append_menu_item(
            &menu_id,
            AnyMenuItem::Menu(MenuItem {
                group: Some("navigation".to_owned()),
                when: Some(ContextKeyExpr::has("test.editorHeader.hasActions")),
                ..MenuItem::new(CommandAction::new("test.editorHeader.action", "Header Action"))
            }),
        ));

        let heights: Rc<RefCell<Vec<f64>>> = Rc::default();
        let control_for_heights: Rc<RefCell<Weak<EditorHeaderControl>>> = Rc::new(RefCell::new(Weak::new()));
        let groups_view: Rc<dyn IEditorGroupsView> = Rc::new(HeaderGroupsView { tab_height });
        let on_did_active_editor_change = guard.add_rc(Emitter::new());
        let group = Rc::new(HeaderGroup {
            active_editor: RefCell::new(None),
            groups_view: groups_view.clone(),
            on_did_active_editor_change,
            on_relayout: Rc::new({
                let (heights, control) = (heights.clone(), control_for_heights.clone());
                move || {
                    let control = control.borrow().upgrade();
                    if let Some(control) = control {
                        heights.borrow_mut().push(control.height());
                    }
                }
            }),
        });

        let context_view_service = guard.add_rc(NativeContextViewService::new(&ui, root));
        let quick_input = TestQuickInputService::new();
        let editor_service = Rc::new(RecordingEditorService::default());
        let breadcrumbs_service = Rc::new(BreadcrumbsService::new());
        let breadcrumbs = breadcrumbs_control_services(
            guard,
            &ui,
            configuration_service,
            &workbench,
            context_keys,
            context_view_service,
            quick_input,
            editor_service,
            breadcrumbs_service,
        );
        let services = EditorHeaderControlServices {
            breadcrumbs,
            menu_service,
            keybinding_service: Rc::new(MockKeybindingService),
            telemetry_service: Rc::new(TestTelemetryService::default()),
        };

        // `const container = $('.title')`
        let container = create_detached_el(&mut ui.tree_mut(), Role::Generic, "title");
        let control = guard.add_rc(EditorHeaderControl::new(
            container,
            group.clone(),
            groups_view,
            Some(IEditorGroupMenuIds { header_primary: Some(menu_id), ..IEditorGroupMenuIds::default() }),
            show_header,
            reserve_header_space,
            services,
        ));
        *control_for_heights.borrow_mut() = Rc::downgrade(&control);

        Self { ui, container, control, group, heights, has_actions }
    }

    /// `state()`.
    fn state(&self) -> State {
        State {
            height: self.control.height(),
            display: display_of(&self.ui, self.container, "editor-group-header"),
            actions_display: display_of(&self.ui, self.container, "editor-group-header-actions"),
        }
    }

    /// `activate(editor)`: `activeEditor = editor ?? null; onDidActiveEditorChange.fire({ editor })`.
    fn activate(&self, editor: Option<&EditorInputRef>) {
        *self.group.active_editor.borrow_mut() = editor.cloned();
        self.group.on_did_active_editor_change.fire(ActiveEditorChangeEvent { editor: editor.cloned(), is_explicit: None });
    }
}

/// `editor === target`: a identidade do objeto.
fn is_editor(editor: Option<&EditorInputRef>, target: &EditorInputRef) -> bool {
    editor.is_some_and(|editor| std::ptr::addr_eq(Rc::as_ptr(editor), Rc::as_ptr(target)))
}

fn state(height: f64, display: Option<&'static str>, actions_display: Option<&'static str>) -> State {
    State { height, display, actions_display }
}

/// `for (const tabHeight of ['default', 'compact'] as const)`.
fn reserves_header_height_across_tab_and_delayed_menu_changes(tab_height: TabHeight) {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let first = guard.add_rc(TestHeaderEditorInput::new());
        let first: EditorInputRef = first;
        let second = guard.add_rc(TestHeaderEditorInput::new());
        let second: EditorInputRef = second;
        let reserve: ReserveHeaderSpace = Rc::new({
            let (first, second) = (first.clone(), second.clone());
            move |editor: Option<&EditorInputRef>| is_editor(editor, &first) || is_editor(editor, &second)
        });
        let header = Header::new(&guard, true, Some(reserve), tab_height);
        let height = if tab_height == TabHeight::Compact { EditorHeaderControl::COMPACT_HEIGHT } else { EditorHeaderControl::DEFAULT_HEIGHT };

        header.activate(Some(&first));
        let mut states = vec![header.state()];
        header.has_actions.set(Dynamic::Bool(true));
        timeout(100).await.unwrap();
        states.push(header.state());

        header.has_actions.set(Dynamic::Bool(false));
        header.activate(Some(&second));
        states.push(header.state());
        timeout(100).await.unwrap();
        states.push(header.state());
        header.has_actions.set(Dynamic::Bool(true));
        timeout(100).await.unwrap();
        states.push(header.state());

        // `[...new Set(header.heights)]`
        let mut relayout_heights: Vec<f64> = Vec::new();
        for height in header.heights.borrow().iter() {
            if !relayout_heights.contains(height) {
                relayout_heights.push(*height);
            }
        }

        assert_eq!(
            (states, relayout_heights),
            (
                vec![
                    state(height, Some(""), Some("none")),
                    state(height, Some(""), Some("")),
                    state(height, Some(""), Some("none")),
                    state(height, Some(""), Some("none")),
                    state(height, Some(""), Some("")),
                ],
                vec![height],
            )
        );
    });
}

#[test]
fn reserves_header_height_across_tab_and_delayed_menu_changes_default() {
    reserves_header_height_across_tab_and_delayed_menu_changes(TabHeight::Default);
}

#[test]
fn reserves_header_height_across_tab_and_delayed_menu_changes_compact() {
    reserves_header_height_across_tab_and_delayed_menu_changes(TabHeight::Compact);
}

#[test]
fn does_not_reserve_an_empty_header_for_other_editors_or_an_empty_group() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let reserved_editor = guard.add_rc(TestHeaderEditorInput::new());
    let reserved_editor: EditorInputRef = reserved_editor;
    let other_editor = guard.add_rc(TestHeaderEditorInput::new());
    let other_editor: EditorInputRef = other_editor;
    let reserve: ReserveHeaderSpace = Rc::new({
        let reserved_editor = reserved_editor.clone();
        move |editor: Option<&EditorInputRef>| is_editor(editor, &reserved_editor)
    });
    let header = Header::new(&guard, true, Some(reserve), TabHeight::Default);

    header.activate(Some(&reserved_editor));
    let mut states = vec![header.state()];
    header.activate(Some(&other_editor));
    states.push(header.state());
    header.activate(None);
    states.push(header.state());

    assert_eq!(
        states,
        vec![
            state(EditorHeaderControl::DEFAULT_HEIGHT, Some(""), Some("none")),
            state(0.0, Some("none"), Some("none")),
            state(0.0, Some("none"), Some("none")),
        ]
    );
}

#[test]
fn layout_refreshes_header_reservation_when_editor_visibility_changes_without_switching_tabs() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_visible = Rc::new(Cell::new(false));
    let reserve: ReserveHeaderSpace = Rc::new({
        let editor_visible = editor_visible.clone();
        move |_: Option<&EditorInputRef>| editor_visible.get()
    });
    let header = Header::new(&guard, true, Some(reserve), TabHeight::Default);
    let editor = guard.add_rc(TestHeaderEditorInput::new());
    let editor: EditorInputRef = editor;
    header.activate(Some(&editor));
    let mut states = vec![header.state()];
    editor_visible.set(true);
    header.control.layout(400.0);
    states.push(header.state());
    editor_visible.set(false);
    header.control.layout(400.0);
    states.push(header.state());
    editor_visible.set(true);
    header.control.layout(400.0);
    states.push(header.state());

    assert_eq!(
        states,
        vec![
            state(0.0, Some("none"), Some("none")),
            state(EditorHeaderControl::DEFAULT_HEIGHT, Some(""), Some("none")),
            state(0.0, Some("none"), Some("none")),
            state(EditorHeaderControl::DEFAULT_HEIGHT, Some(""), Some("none")),
        ]
    );
}

#[test]
fn without_a_reservation_policy_header_visibility_follows_menu_contents() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let header = Header::new(&guard, true, None, TabHeight::Default);
        let editor = guard.add_rc(TestHeaderEditorInput::new());
        let editor: EditorInputRef = editor;
        header.activate(Some(&editor));
        let mut states = vec![header.state()];
        header.has_actions.set(Dynamic::Bool(true));
        timeout(100).await.unwrap();
        states.push(header.state());
        header.has_actions.set(Dynamic::Bool(false));
        timeout(100).await.unwrap();
        states.push(header.state());

        assert_eq!(
            states,
            vec![
                state(0.0, Some("none"), Some("none")),
                state(EditorHeaderControl::DEFAULT_HEIGHT, Some(""), Some("")),
                state(0.0, Some("none"), Some("none")),
            ]
        );
    });
}

#[test]
fn reservation_does_not_enable_a_group_header_when_show_header_is_false() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let reserve: ReserveHeaderSpace = Rc::new(|_: Option<&EditorInputRef>| true);
    let header = Header::new(&guard, false, Some(reserve), TabHeight::Default);
    let editor = guard.add_rc(TestHeaderEditorInput::new());
    let editor: EditorInputRef = editor;
    header.activate(Some(&editor));

    assert_eq!(header.state(), state(0.0, None, None));
}
