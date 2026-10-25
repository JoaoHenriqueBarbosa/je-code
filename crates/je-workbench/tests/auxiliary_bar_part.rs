//! Porte de `src/vs/workbench/test/browser/parts/auxiliaryBarPart.test.ts` (suíte `Auxiliary Bar - Responsive title actions`) e de
//! `auxiliaryBarTestUtils.ts` (`createTestAuxiliaryBarPart` e `TestAuxiliaryBarPart`).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo do caso; o `store.add(...)` do original é
//!   `guard.add_rc` (`guard.add` para os registros de menu).
//! - **DOM vira árvore de widgets.** A raiz `.monaco-workbench` e o `.part.auxiliarybar` são nós do `Ui`, e o contêiner passado ao
//!   `create` é o [`ViewElement`] nativo do nó. A vista é a [`NativeCompositePartView`]: o título (`h2`), a barra de ferramentas
//!   do título e a barra global são nós reais, e as caixas (`getBoundingClientRect`) são `absolute_bounds` do nó. O
//!   `container.style.width` é a largura do nó do contêiner. `querySelector('h2')` é o nó do título da vista; `title.clientWidth >=
//!   title.scrollWidth` são as medidas do `ViewElement` do `h2` (a largura do nó e a do texto inteiro); `title.textContent` é o texto
//!   guardado pela vista (sem a caixa alta do CSS).
//! - **As três apresentações** (`''`, `'modern-ui modern-ui-tabs'` e `'modern-ui modern-ui-tabs modern-ui-compact'`) são as classes da
//!   raiz. As folhas `part.css`, `padding.css`, `fontRamp.css` e `tabs.css` que o original importa são as regras da vista nativa
//!   (a vista usa a altura de título do `PartLayout`, que só muda com o `isFloatingPanelsEnabled` do serviço de layout, que o caso
//!   não liga): as três apresentações medem o mesmo.
//! - **`globalActionsVisible`**: os seletores `.codicon-screen-full` e `.codicon-close` dentro de `.global-actions` são os dois itens
//!   da barra global (`getItemsLength() == 2`) e a largura positiva da barra global e do nó `.global-actions`.
//! - **`TestAuxiliaryBarMenuService extends MenuService`** é o [`RemappingMenuService`] (só o `createMenu` troca o menu);
//!   **`TestAuxiliaryBarPart extends AuxiliaryBarPart`** é o [`TestAuxiliaryBarBehavior`], que repassa tudo ao
//!   [`AuxiliaryBarBehavior`] e troca o `getToolbarResponsiveBehavior` (com o `getAvailableWidth`) e o `shouldShowCompositeBar`
//!   (por [`AuxiliaryBarPart::extend`]). O `getActiveComposite()` sobrescrito é o [`TestActiveComposite`] (`test.chat`, `Chat`), pelo
//!   gancho `get_active_composite` do comportamento: sem ele a parte se vê vazia e o `layoutEmptyMessage` zera o título. O composite
//!   não tem `getActionViewItem`, e a vista nativa não tem provedor de item de ação (a `ToolBar` desenha cada ação), então não há o
//!   que sobrescrever além disso.
//! - **`ViewDescriptorService`** é o [`TestViewDescriptorService`] (sem contêineres), o que o serviço real também devolve sem
//!   contribuições carregadas.
//! - **`IToolBarResponsiveBehaviorOptions.getAvailableWidth`** lê `toolbarAvailableWidth ?? toolBar.getElement().getBoundingClientRect().width ?? 0`.
//! - **`deepStrictEqual` de objetos literais** é `assert_eq!` de estruturas com os mesmos campos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[path = "support/pane_composite_part_support.rs"]
mod pane_composite_part_support;
#[path = "support/part_doubles.rs"]
mod part_doubles;
#[path = "../../je-editor/tests/support/test_color_theme_service.rs"]
mod test_color_theme_service;
#[path = "../../je-editor/tests/support/test_notification_service.rs"]
mod test_notification_service;

use je_workbench::browser::pane_composite::{extensions, pane_composite_registry};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::actions::{Action, ActionRef, Separator};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::themables::ThemeIcon;
use je_editor::services::theme_service::IThemeService;
use je_platform::action::{CommandAction, Icon, Title};
use je_platform::actions::actions::{AnyMenuItem, IMenu, IMenuService, MenuActionGroup, MenuActionOptions, MenuCreateOptions, MenuId, MenuItem, MenuItemAction, SplitButton, SubmenuItem, SubmenuItemAction, menu_registry};
use je_platform::actions::menu_ids;
use je_platform::actions::menu_service::MenuService;
use je_platform::commands::ICommandService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::telemetry::null_telemetry_service;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Ui};
use je_widgets::toolbar::{IToolBarResponsiveBehaviorOptions, ToggleMenuAction};
use je_workbench::browser::composite::{IComposite, ICompositeControl};
use je_workbench::browser::parts::auxiliarybar::auxiliary_bar_part::{AuxiliaryBarBehavior, AuxiliaryBarPart, AuxiliaryBarPartServices};
use je_workbench::browser::parts::composite_part::{ICompositeToolBar, IToolBarItems};
use je_workbench::browser::parts::native_composite_part_view::{NativeCompositePartView, NativeCompositePartViewServices};
use je_workbench::browser::parts::pane_composite_bar::PaneCompositeBarOptions;
use je_workbench::browser::parts::pane_composite_part::{AbstractPaneCompositePart, CompositeBarPosition, PaneCompositePartBehavior};
use je_workbench::services::contextview::native_context_view_service::node_element;
use je_workbench::services::layout::layout_service::Parts;
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use notification_doubles::{NoCommandService, NoKeybindingService};
use pane_composite_part_support::{PaneTestDeps, PaneTestLayoutService, TestViewDescriptorService, empty_instantiation_service, pane_part_services, test_storage_service};
use part_doubles::TestContextMenuService;
use test_color_theme_service::TestThemeService;
use test_notification_service::TestNotificationService;

/// `MenuId.for('test.auxiliaryBar.newChat')`.
fn new_chat_menu() -> MenuId {
    MenuId::for_id("test.auxiliaryBar.newChat")
}

/// `MenuId.for('test.auxiliaryBar.title')`.
fn global_actions_menu() -> MenuId {
    MenuId::for_id("test.auxiliaryBar.title")
}

/// `TestAuxiliaryBarMenuService extends MenuService`: `createMenu` troca `MenuId.AuxiliaryBarTitle` por `globalActionsMenu`.
struct RemappingMenuService {
    inner: Rc<dyn IMenuService>,
}

impl IMenuService for RemappingMenuService {
    fn create_menu(&self, id: &MenuId, context_key_service: Rc<dyn IContextKeyService>, options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        let id = if *id == menu_ids::auxiliary_bar_title() { global_actions_menu() } else { id.clone() };
        self.inner.create_menu(&id, context_key_service, options)
    }

    fn get_menu_actions(&self, id: &MenuId, context_key_service: &dyn IContextKeyService, options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        self.inner.get_menu_actions(id, context_key_service, options)
    }

    fn get_menu_contexts(&self, id: &MenuId) -> HashSet<String> {
        self.inner.get_menu_contexts(id)
    }

    fn reset_hidden_states(&self, menu_ids: Option<&[MenuId]>) {
        self.inner.reset_hidden_states(menu_ids);
    }
}

/// O composite que o `getActiveComposite()` sobrescrito do original devolve (`getId: () => 'test.chat'`, `getTitle: () => 'Chat'`).
struct TestActiveComposite;

impl IComposite for TestActiveComposite {
    fn on_did_focus(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_blur(&self) -> Event<()> {
        Event::none()
    }

    fn has_focus(&self) -> bool {
        false
    }

    fn get_id(&self) -> String {
        "test.chat".to_owned()
    }

    fn get_title(&self) -> Option<String> {
        Some("Chat".to_owned())
    }

    fn get_control(&self) -> Option<Rc<dyn ICompositeControl>> {
        None
    }

    fn focus(&self) {}
}

/// `class TestAuxiliaryBarPart extends AuxiliaryBarPart`.
struct TestAuxiliaryBarBehavior {
    inner: Rc<AuxiliaryBarBehavior>,
    toolbar_available_width: Rc<Cell<Option<f64>>>,
    part: Rc<RefCell<Weak<AbstractPaneCompositePart>>>,
    ui: Ui,
}

impl PaneCompositePartBehavior for TestAuxiliaryBarBehavior {
    fn minimum_width(&self) -> f64 {
        self.inner.minimum_width()
    }

    fn maximum_width(&self) -> f64 {
        self.inner.maximum_width()
    }

    fn minimum_height(&self) -> f64 {
        self.inner.minimum_height()
    }

    fn maximum_height(&self) -> f64 {
        self.inner.maximum_height()
    }

    fn to_json(&self) -> Dynamic {
        self.inner.to_json()
    }

    fn get_toolbar_responsive_behavior(&self) -> Option<IToolBarResponsiveBehaviorOptions> {
        let (width, part, ui) = (self.toolbar_available_width.clone(), self.part.clone(), self.ui.clone());
        Some(IToolBarResponsiveBehaviorOptions {
            get_available_width: Some(Rc::new(move || {
                width.get().unwrap_or_else(|| {
                    let tool_bar = part.borrow().upgrade().and_then(|part| part.tool_bar());
                    tool_bar.map_or(0.0, |tool_bar| ui.tree().bounds(tool_bar.get_element()).width())
                })
            })),
            ..self.inner.get_toolbar_responsive_behavior().unwrap_or_default()
        })
    }

    fn update_styles(&self, part: &AbstractPaneCompositePart) {
        self.inner.update_styles(part);
    }

    fn get_composite_bar_options(&self, part: &AbstractPaneCompositePart) -> PaneCompositeBarOptions {
        self.inner.get_composite_bar_options(part)
    }

    fn get_active_composite(&self, _part: &AbstractPaneCompositePart) -> Option<Rc<dyn IComposite>> {
        Some(Rc::new(TestActiveComposite))
    }

    fn should_show_composite_bar(&self, _part: &AbstractPaneCompositePart) -> bool {
        false
    }

    fn get_composite_bar_position(&self, part: &AbstractPaneCompositePart) -> CompositeBarPosition {
        self.inner.get_composite_bar_position(part)
    }
}

/// O que `TestAuxiliaryBarPart` expõe ao caso.
struct TestAuxiliaryBarPart {
    part: Rc<AuxiliaryBarPart>,
    view: Rc<NativeCompositePartView>,
    toolbar_available_width: Rc<Cell<Option<f64>>>,
}

impl TestAuxiliaryBarPart {
    /// `setToolbarAvailableWidth(width)`.
    fn set_toolbar_available_width(&self, width: f64) {
        self.toolbar_available_width.set(Some(width));
    }

    /// `get toolbar()`.
    fn toolbar(&self) -> Rc<dyn ICompositeToolBar> {
        self.part.tool_bar().expect("Assertion Failed: argument is undefined or null")
    }
}

/// `titleLabel?.updateTitle(...)`, `toolBar?.setAriaLabel(...)` e `toolBar?.setActions(...)` de `setTitleActions`.
fn set_title_actions(part: &AuxiliaryBarPart, title: &str, primary: &[ActionRef], secondary: &[ActionRef]) {
    if let Some(title_label) = part.title_label() {
        title_label.update_title("test.chat", title, None);
    }
    if let Some(tool_bar) = part.tool_bar() {
        tool_bar.set_aria_label(&format!("{title} actions"));
        tool_bar.set_actions(primary, secondary, None);
    }
}

/// `createTestAuxiliaryBarPart(container, store)`.
fn create_test_auxiliary_bar_part(guard: &LeakGuard, ui: &Ui, container: NodeId) -> TestAuxiliaryBarPart {
    let theme_service = guard.add_rc(TestThemeService::new_default());
    let theme_service: Rc<dyn IThemeService> = theme_service;
    let storage_service = test_storage_service(guard);
    let keybinding_service: Rc<dyn IKeybindingService> = Rc::new(NoKeybindingService);
    let command_service: Rc<dyn ICommandService> = Rc::new(NoCommandService);
    let configuration_service = TestConfigurationService::new(None);
    let context_key_service = guard.add_rc(ContextKeyService::new(configuration_service.clone()));
    let context_key_service: Rc<dyn IContextKeyService> = context_key_service;
    let menu_service: Rc<dyn IMenuService> =
        Rc::new(RemappingMenuService { inner: { let menu_service = guard.add_rc(MenuService::new(command_service.clone(), keybinding_service.clone(), storage_service.clone())); menu_service } });
    let context_menu_service = Rc::new(TestContextMenuService);

    for (order, (id, title, icon)) in [("test.maximize", "Maximize Secondary Side Bar", "screen-full"), ("test.close", "Hide Secondary Side Bar", "close")].into_iter().enumerate() {
        guard.add(menu_registry().append_menu_item(
            &global_actions_menu(),
            AnyMenuItem::Menu(MenuItem {
                command: CommandAction { icon: Some(Icon::Theme(ThemeIcon::from_id(icon))), ..CommandAction::new(id, Title::Plain(title.to_owned())) },
                group: Some("navigation".to_owned()),
                order: Some(order as f64),
                ..MenuItem::new(CommandAction::new(id, Title::Plain(title.to_owned())))
            }),
        ));
    }

    let deps = PaneTestDeps {
        notification_service: Rc::new(TestNotificationService),
        theme_service,
        keybinding_service: keybinding_service.clone(),
        context_key_service: context_key_service.clone(),
        menu_service: menu_service.clone(),
        context_menu_service: context_menu_service.clone(),
        command_service: command_service.clone(),
        configuration_service: configuration_service.clone(),
        view_descriptor_service: Rc::new(TestViewDescriptorService { can_move_views: false }),
        extension_service: TestExtensionService::new(),
        storage_service: storage_service.clone(),
    };
    let layout_service = PaneTestLayoutService::new(&[Parts::AuxiliarybarPart, Parts::EditorPart, Parts::StatusbarPart]);

    let view = NativeCompositePartView::new(
        ui,
        NativeCompositePartViewServices {
            menu_service: menu_service.clone(),
            context_key_service: context_key_service.clone(),
            context_menu_service: context_menu_service.clone(),
            keybinding_service: keybinding_service.clone(),
            command_service: command_service.clone(),
            telemetry_service: null_telemetry_service(),
            container_node: {
                let root = ui.root();
                Rc::new(move |_| Some(root))
            },
            device_pixel_ratio: Rc::new(|| 1.0),
            foreground: Rc::new(|| Rgba::opaque(0xCC, 0xCC, 0xCC)),
        },
        true,
    );

    let toolbar_available_width: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let part_slot: Rc<RefCell<Weak<AbstractPaneCompositePart>>> = Rc::new(RefCell::new(Weak::new()));
    let (width, slot, behavior_ui) = (toolbar_available_width.clone(), part_slot.clone(), ui.clone());
    let part = guard.add_rc(AuxiliaryBarPart::extend(
        AuxiliaryBarPartServices { pane: pane_part_services(&deps, layout_service, view.clone(), empty_instantiation_service()), command_service: command_service.clone() },
        move |inner| Rc::new(TestAuxiliaryBarBehavior { inner, toolbar_available_width: width, part: slot, ui: behavior_ui }),
    ));
    *part_slot.borrow_mut() = part.weak();

    let container_element = node_element(container);
    part.create(&container_element);

    let new_chat: ActionRef = MenuItemAction::new(
        CommandAction { icon: Some(Icon::Theme(ThemeIcon::from_id("plus"))), ..CommandAction::new("new", Title::Plain("New Chat".to_owned())) },
        None,
        None,
        None,
        None,
        context_key_service.as_ref(),
        command_service.clone(),
    );
    let new_window: ActionRef =
        MenuItemAction::new(CommandAction::new("window", Title::Plain("New Chat in New Window".to_owned())), None, None, None, None, context_key_service.as_ref(), command_service.clone());
    let new_chat_submenu = SubmenuItemAction::new(
        SubmenuItem {
            title: Title::Plain("New Chat".to_owned()),
            submenu: new_chat_menu(),
            icon: Some(Icon::Theme(ThemeIcon::from_id("plus"))),
            when: None,
            group: None,
            order: None,
            is_selection: None,
            is_split_button: Some(SplitButton::Plain(true)),
        },
        None,
        vec![new_chat, new_window],
    )
    .as_action();
    let settings: ActionRef = MenuItemAction::new(
        CommandAction { icon: Some(Icon::Theme(ThemeIcon::from_id("gear"))), ..CommandAction::new("settings", Title::Plain("Open Customizations".to_owned())) },
        None,
        None,
        None,
        None,
        context_key_service.as_ref(),
        command_service.clone(),
    );
    let history = guard.add_rc(Action::new("history", "History", "", true, None));
    let history: ActionRef = history;
    set_title_actions(&part, "Chat", &[new_chat_submenu, settings], &[history]);

    TestAuxiliaryBarPart { part, view, toolbar_available_width }
}

/// O estado que o caso captura a cada largura.
#[derive(Debug, PartialEq)]
struct State {
    actions: Vec<String>,
    separator_width: f64,
    title_visible: bool,
    overlap: bool,
    global_actions_visible: bool,
}

#[test]
fn overflows_new_chat_before_settings_and_restores_them_in_reverse_order() {
    for presentation in ["", "modern-ui modern-ui-tabs", "modern-ui modern-ui-tabs modern-ui-compact"] {
        // Os registros de `Registry.as<PaneCompositeRegistry>(...)` nascem na importação do módulo no original, antes do
        // rastreio de vazamentos do caso.
        for id in [extensions::VIEWLETS, extensions::PANELS, extensions::AUXILIARY] {
            pane_composite_registry(id);
        }
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        // setup: `root = append(document.body, $('.monaco-workbench'))`
        let ui = Ui::new();
        let ui_root = ui.root();
        let root = ui.tree_mut().create(ui_root, Role::Generic);
        ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1000.0, 800.0));

        // `root.className = 'monaco-workbench ' + presentation`
        ui.tree_mut().set_class_name(root, &format!("monaco-workbench {presentation}"));
        let container = ui.tree_mut().create(root, Role::Generic);
        ui.tree_mut().set_class_name(container, "part auxiliarybar");
        let part = create_test_auxiliary_bar_part(&guard, &ui, container);

        let mut states: Vec<State> = Vec::new();
        for (container_width, toolbar_width) in [(300.0, 120.0), (185.0, 80.0), (140.0, 50.0), (185.0, 80.0), (300.0, 120.0)] {
            ui.tree_mut().set_bounds(container, Rect::new(0.0, 0.0, container_width, 200.0));
            part.set_toolbar_available_width(toolbar_width);
            part.part.layout(container_width, 200.0, 0.0, 0.0);
            part.toolbar().relayout();

            let toolbar = part.toolbar();
            let length = toolbar.get_items_length();
            let title_node = part.view.title_node().expect("o título existe");
            let title_element = part.part.composite_part().title_label_element().expect("o título existe");
            let first_action = toolbar.get_item_element(0).expect("Assertion Failed: argument is undefined or null");
            let last_action = toolbar.get_item_element(length - 1).expect("Assertion Failed: argument is undefined or null");
            let global_actions = {
                let tree = ui.tree();
                let title_area = tree.parent(tree.parent(title_node).expect("o rótulo existe")).expect("o título existe");
                tree.children_with_class(title_area, "global-actions").first().copied().expect("Assertion Failed: argument is undefined or null")
            };
            let global_tool_bar = part.view.global_tool_bar().expect("a barra global existe");

            let tree = ui.tree();
            let title_right = tree.absolute_bounds(title_node).x1;
            states.push(State {
                actions: (0..length).map(|index| toolbar.get_item_action(index).map(|action| action.id()).unwrap_or_default()).collect(),
                separator_width: toolbar.get_item_width(length - 1),
                title_visible: part.view.title_text() == "Chat" && title_element.client_width() >= title_element.scroll_width(),
                overlap: title_right > tree.absolute_bounds(first_action).x0 + 0.5 || tree.absolute_bounds(last_action).x1 > tree.absolute_bounds(global_actions).x0 + 0.5,
                global_actions_visible: global_tool_bar.get_items_length() == 2
                    && global_tool_bar.get_items_width() > 0.0
                    && tree.absolute_bounds(global_actions).width() > 0.0,
            });
        }

        let separator_width = states[0].separator_width;
        let ids = |first: bool, second: bool| -> Vec<String> {
            let mut ids = Vec::new();
            if first {
                ids.push("submenuitem.test.auxiliaryBar.newChat".to_owned());
            }
            if second {
                ids.push("settings".to_owned());
            }
            ids.push(ToggleMenuAction::ID.to_owned());
            ids.push(Separator::ID.to_owned());
            ids
        };
        let expected = |actions: Vec<String>| State { actions, separator_width, title_visible: true, overlap: false, global_actions_visible: true };
        assert_eq!(
            states,
            vec![expected(ids(true, true)), expected(ids(false, true)), expected(ids(false, false)), expected(ids(false, true)), expected(ids(true, true))],
            "apresentação: {presentation:?}"
        );

        part.part.dispose();
    }
}
