//! Porte de `src/vs/workbench/contrib/modernUI/test/browser/modernUI.contribution.test.ts` (suíte
//! `ModernUIContribution`), caso a caso e na mesma ordem, nos casos que decidem o estado da contribuição (o submenu e
//! as ações de densidade, as classes dos contêineres, o relayout, o tamanho do cabeçalho dos panes e as abas conectadas).
//!
//! Notas de tradução:
//! - O DOM do original não recebe o CSS gerado pelo tema, só as variáveis `--vscode-*` que o caso define; o caso dos
//!   separadores dos pane headers usa `with_explicit_colors_only` para não herdar o padrão registrado de `panel.border`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso, e `store.add(...)` é
//!   `guard.add_rc(...)`.
//! - O `import` do módulo registra o submenu e as ações; aqui o começo de cada caso chama
//!   `register_layout_density_menu` e `register_layout_density_actions` (os registros são por thread).
//! - `ModernUITestLayoutService` e `ModernUITestAuxiliaryWindowService` são o `TestHost` (o trait `ModernUiHost` reúne
//!   o layout, as classes dos contêineres e o layout da janela auxiliar): `containers`, `layoutCount`,
//!   `auxiliaryWindow.layoutCount` e `container.classList.contains` são os campos e `has_class`. `mainContainer` é o
//!   primeiro contêiner, e `addContainer` empilha e dispara `onDidAddContainer`.
//! - `new ModernUITestPane()` (`Pane` com `minimumBodySize: 0` e `maximumBodySize: 0`) é o `Pane` de `je-widgets`, e
//!   `pane.minimumSize` é o `minimum_size()` dele (o cabeçalho global mais o corpo).
//! - `TestConfigurationService.setUserConfiguration` seguido de `onDidChangeConfigurationEmitter.fire({ affectsConfiguration:
//!   key => key === X, ... })` é o `set_user_configuration` seguido do `ConfigurationChangeEvent` da chave `X`.
//! - O `TestConfigurationService` com `updateValue` pendente é o `PendingUpdateConfigurationService`, que registra as
//!   chamadas e devolve a `Promise` que o teste completa (`DeferredPromise`).
//! - `getToggledExpression(item.command.toggled)` é o `Toggled::Expression` (ou a condição do `Toggled::Info`), e o
//!   `context(...)` com `getValue` é o `TestContext` (`IContext`).
//! - `TestInstantiationService` com `stub(IConfigurationService, ...)` e `stub(IWorkbenchEnvironmentService, { isSessionsWindow:
//!   false })` é o `InstantiationService` com os dois serviços, e `command.handler(accessor)` roda em `invoke_function`.
//! - `Action2.run` do porte não devolve valor (ver `layout_density_actions.rs`): o caso confere as chamadas de
//!   `updateValue` pelo handler do comando registrado e a espera pelo `updateValue` em `set_layout_density`, que é o
//!   corpo do `run`, num segundo serviço de configuração.
//!
//! O `getComputedStyle` do original é o `ModernUiStyle` (`resolved_style.rs`): o estilo que as regras de `floatingPanels.css`
//! e de `modernUI/browser/media/*.css` resolvem a partir das classes da parte e das cores do tema, o mesmo que os widgets
//! nativos pintam. Cada `root.style.setProperty('--vscode-X', ...)` é uma customização de cor do tema (`X` com ponto), e as
//! classes do `appendElement` são as da parte. Os casos de borda, de fundo do shell e das cores das abas conferem esse
//! estilo resolvido, como o original.
//!
//! Recortes: os demais casos da suíte leem o estilo calculado de um DOM com os `*.css` do VS Code (`getComputedStyle`,
//! `getBoundingClientRect`, `generateColorThemeCSS`); a geometria (`getBoundingClientRect`, margens, larguras) ainda não
//! tem resolvedor, e o `.css` é a especificação lida ao portar cada widget. Voltam com o desenho nativo dos widgets e com o participante de tema (`generateColorThemeCSS`): `supports isolated notification and dialog presentation`, `uses part-specific pane colors ...`
//! até `rounds activity bar corners ...` e todos os que seguem (barra de atividade, barra de status, menu compacto,
//! sashes, cantos do macOS, abas conectadas por tema) exceto os portados abaixo, e `uses the editor background for the
//! connected surface` até `applies editor tab foregrounds ...`, `uses readable theme foregrounds ...` até `fades tab
//! actions only when ...`, e de `uses legacy color customizations ...` até `keeps panel global actions above ...`.
//! Portados com a geometria do resolvedor (`pane_composite_action_box`, `activity_rail_geometry`, `floating_part_margins`,
//! `activity_bar_corner_radii`): de `pane composite actions fill ...` até `keeps a gutter between horizontal panels ...`.
//! Portados com o resolvedor das alças dos sashes (`sash_grip_content`, `sash_grip`, `vertical_sash_highlight_insets`) e
//! dos raios das partes (`part_corner_radii` e afins, com o zoom e a moldura nativa do macOS): de `keeps collapsed primary
//! side bar grips ...` até `keeps macOS corner radii in sync ...`. No caso dos sashes do editor modal, o cursor, o
//! `pointer-events` e o destaque (`::before`) vêm do `Sash` nativo de `je-widgets`, que pinta o que o `sash.css` define.
//! Nos casos parciais ficou só a parte que não é CSS: no terceiro caso `paneHeaderLineHeight` e
//! `paneHeaderInlineLineHeight`; em `toggles uppercase view headers without relayout` o `text-transform` dos títulos;
//! em `Explorer title casing ...` o `text-transform` (fica a decisão `shouldPreserveWorkspaceNameCase`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableStore;
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::action::Title;
use je_platform::actions::actions::{AnyMenuItem, menu_registry};
use je_platform::actions::menu_ids;
use je_platform::commands::commands::commands_registry;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationKeys, ConfigurationModel};
use je_platform::configuration::service::{IConfigurationService, I_CONFIGURATION_SERVICE};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{
    ConfigurationChange, ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_platform::contextkey::contextkey::{ContextKeyExpression, IContext};
use je_platform::environment::environment::EnvironmentService;
use je_platform::log::NullLogService;
use je_platform::resources::{join_path, uri_file};
use je_platform::workspace::{IWorkspace, WorkbenchState, WorkspaceFolder};
use je_base::uri::URI;
use je_theme::ColorTheme;
use je_theme::ThemeType;
use je_theme::color::Color;
use je_theme::registry::ColorValue::Id;
use je_theme::registry::{ColorDefaults, ColorTransform, ColorValue, Defaults, color_registry};
use je_theme::to_rgba8;
use je_workbench::contrib::files::browser::views::explorer_view::should_preserve_workspace_name_case;
use je_base::actions::{Action, ActionRef, Separator};
use je_base::key_codes::KeyCode;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Cursor, Event as UiEvent, EventKind, KeyData, Modifiers, NodeId, PointerEvents, Role, Ui};
use je_widgets::action_bar::{ActionBar, ActionsOrientation, IActionBarOptions, IActionOptions};
use je_widgets::menu::{HorizontalDirection, IMenuDirection, IMenuOptions, Menu, VerticalDirection, unthemed_menu_styles};
use je_widgets::menu_bar::{IMenuBarOptions, MenuBar, MenuBarMenu};
use je_widgets::modifier_key_emitter::ModifierKeyEmitter;
use je_widgets::paneview::{Pane, PaneContent, PaneOptions};
use je_widgets::sash::{Sash, SashOptions, set_global_sash_hover_border};
use je_widgets::splitview::SplitViewView;
use je_workbench::browser::actions::layout_density_actions::{LayoutDensityMenu, register_layout_density_actions, set_layout_density};
use je_workbench::contrib::modern_ui::modern_ui_contribution::{ModernUiContribution, ModernUiHost, register_layout_density_menu};
use je_workbench::contrib::modern_ui::resolved_style::{
    ActionTabState, ActivityBarMetrics, ActivityItemState, BoxOffsets, ClassList, CornerRadii, EditorTabState, ListRowHost, MENUBAR_MENU_HOLDER_POSITION, MenubarButtonState, ModernUiStyle, NotificationInsets,
    NotificationsPlacement, PaneHost, PaneState, SashPlacement, VISIBLE_SCROLLBAR_Z_INDEX, css_rgb,
};
use je_workbench::services::environment::environment_service::I_WORKBENCH_ENVIRONMENT_SERVICE;
use je_workbench::services::layout::layout_service::{
    AddedContainer, LayoutContainerId, LayoutSettings, ModernUiDensity, ModernUiEditorTabStyle,
};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;

/// `ModernUITestPane`.
struct ModernUiTestPane;

impl PaneContent for ModernUiTestPane {
    fn render_header(&self, _container: NodeId) {}
    fn render_body(&self, _container: NodeId) {}
    fn layout_body(&self, _height: f64, _width: f64) {}
}

/// `ModernUITestLayoutService` e `ModernUITestAuxiliaryWindowService`.
struct TestHost {
    containers: RefCell<Vec<LayoutContainerId>>,
    classes: RefCell<HashMap<LayoutContainerId, BTreeSet<String>>>,
    layout_count: Cell<usize>,
    auxiliary_layout_count: Cell<usize>,
    on_did_add_container_emitter: Rc<Emitter<AddedContainer>>,
}

impl TestHost {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            containers: RefCell::new(vec![LayoutContainerId(1)]),
            classes: RefCell::new(HashMap::new()),
            layout_count: Cell::new(0),
            auxiliary_layout_count: Cell::new(0),
            on_did_add_container_emitter: Emitter::new(),
        })
    }

    fn main_container() -> LayoutContainerId {
        LayoutContainerId(1)
    }

    /// `addContainer(container, disposables)`.
    fn add_container(&self, container: LayoutContainerId, disposables: Rc<DisposableStore>) {
        self.containers.borrow_mut().push(container);
        self.on_did_add_container_emitter.fire(AddedContainer { container, disposables });
    }

    fn has_class(&self, container: LayoutContainerId, class_name: &str) -> bool {
        self.classes.borrow().get(&container).is_some_and(|classes| classes.contains(class_name))
    }
}

impl ModernUiHost for TestHost {
    fn on_did_add_container(&self) -> Event<AddedContainer> {
        self.on_did_add_container_emitter.event()
    }

    fn containers(&self) -> Vec<LayoutContainerId> {
        self.containers.borrow().clone()
    }

    fn main_container(&self) -> LayoutContainerId {
        Self::main_container()
    }

    fn layout(&self) {
        self.layout_count.set(self.layout_count.get() + 1);
    }

    fn toggle_class(&self, container: LayoutContainerId, class_name: &str, force: bool) {
        let mut classes = self.classes.borrow_mut();
        let set = classes.entry(container).or_default();
        if force {
            set.insert(class_name.to_owned());
        } else {
            set.remove(class_name);
        }
    }

    fn layout_auxiliary_window(&self, _container: LayoutContainerId) {
        self.auxiliary_layout_count.set(self.auxiliary_layout_count.get() + 1);
    }
}

/// O `context(modernUI, density)` do primeiro caso.
struct TestContext {
    modern_ui: bool,
    density: ModernUiDensity,
}

impl IContext for TestContext {
    fn get_value(&self, key: &str) -> Dynamic {
        if key == format!("config.{}", LayoutSettings::ModernUi.as_str()) {
            Dynamic::Bool(self.modern_ui)
        } else if key == format!("config.{}", LayoutSettings::ModernUiDensity.as_str()) {
            Dynamic::string(self.density.as_str())
        } else {
            Dynamic::Undefined
        }
    }
}

fn title_text(title: &Title) -> String {
    match title {
        Title::Plain(text) => text.clone(),
        Title::Localized(title) => title.value.clone(),
    }
}

/// O `MenuRegistry.getMenuItems(MenuId.GlobalActivity).filter(isISubmenuItem).find(... 'Layout Density')`.
fn layout_density_parent() -> Option<je_platform::actions::actions::SubmenuItem> {
    menu_registry().get_menu_items(&menu_ids::global_activity()).into_iter().find_map(|item| match item {
        AnyMenuItem::Submenu(submenu) if title_text(&submenu.title) == "Layout Density" => Some(submenu),
        _ => None,
    })
}

fn setup_registrations(guard: &common::LeakGuard) {
    guard.add(register_layout_density_menu());
    guard.add(register_layout_density_actions());
}

/// `getToggledExpression`.
fn toggled_expression(toggled: &Option<je_platform::action::Toggled>) -> Option<ContextKeyExpression> {
    match toggled {
        Some(je_platform::action::Toggled::Expression(expression)) => Some(expression.clone()),
        Some(je_platform::action::Toggled::Info(info)) => Some(info.condition.clone()),
        None => None,
    }
}

#[test]
fn shows_layout_density_options_in_the_settings_menu_only_when_modern_ui_is_enabled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    setup_registrations(&guard);
    let parent = layout_density_parent();
    let options: Vec<_> = if parent.is_some() {
        menu_registry()
            .get_menu_items(&LayoutDensityMenu())
            .into_iter()
            .filter_map(|item| match item {
                AnyMenuItem::Menu(menu_item) => Some(menu_item),
                AnyMenuItem::Submenu(_) => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let context = |modern_ui: bool, density: ModernUiDensity| TestContext { modern_ui, density };

    let actual_parent = parent.as_ref().map(|parent| {
        (
            parent.group.clone(),
            parent.order,
            parent.when.as_ref().map(|when| when.evaluate(&context(true, ModernUiDensity::Default))),
            parent.when.as_ref().map(|when| when.evaluate(&context(false, ModernUiDensity::Default))),
        )
    });
    let actual_options: Vec<_> = options
        .iter()
        .map(|item| {
            let toggled = toggled_expression(&item.command.toggled);
            (
                title_text(&item.command.title),
                toggled.as_ref().map(|toggled| toggled.evaluate(&context(true, ModernUiDensity::Default))),
                toggled.as_ref().map(|toggled| toggled.evaluate(&context(true, ModernUiDensity::Compact))),
            )
        })
        .collect();

    assert_eq!(
        (actual_parent, actual_options),
        (
            Some((Some("2_configuration".to_owned()), Some(8.0), Some(true), Some(false))),
            vec![
                ("Default".to_owned(), Some(true), Some(false)),
                ("Compact".to_owned(), Some(false), Some(true)),
            ],
        )
    );
}

/// O `new class extends TestConfigurationService { override updateValue(...) { updates.push(...); return updateComplete.p; } }`.
struct PendingUpdateConfigurationService {
    inner: Rc<TestConfigurationService>,
    update_complete: Promise<()>,
    updates: RefCell<Vec<(String, Option<String>, Option<ConfigurationTarget>)>>,
}

impl IConfigurationService for PendingUpdateConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.inner.on_did_change_configuration()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        self.inner.get_configuration_data()
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        self.inner.get_value(section, overrides)
    }

    fn update_value(&self, key: &str, value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        let text = match &value {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        };
        self.updates.borrow_mut().push((key.to_owned(), text, target));
        self.update_complete.clone()
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        self.inner.inspect(key, overrides)
    }

    fn reload_configuration(&self) -> Promise<()> {
        self.inner.reload_configuration()
    }

    fn keys(&self) -> ConfigurationKeys {
        self.inner.keys()
    }
}

#[test]
fn updates_the_layout_density_from_the_settings_menu() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    setup_registrations(&guard);
    let (update_complete, resolver) = Promise::<()>::new();
    let configuration_service =
        Rc::new(PendingUpdateConfigurationService { inner: TestConfigurationService::new(None), update_complete: update_complete.clone(), updates: RefCell::new(Vec::new()) });
    let environment_service: Rc<dyn je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService> = TestEnvironmentService::new();
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let instantiation_service = guard.add_rc(InstantiationService::new(
        ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration).with(I_WORKBENCH_ENVIRONMENT_SERVICE, environment_service.clone()),
    ));
    let parent = layout_density_parent().expect("o submenu de densidade está registrado");
    let compact_option = menu_registry()
        .get_menu_items(&LayoutDensityMenu())
        .into_iter()
        .find_map(|item| match item {
            AnyMenuItem::Menu(menu_item) if menu_item.command.id == "workbench.action.setLayoutDensity.compact" => Some(menu_item),
            _ => None,
        })
        .expect("a opção compacta está registrada");
    assert_eq!(parent.submenu, LayoutDensityMenu());
    let command = commands_registry().get_command(&compact_option.command.id).expect("o comando está registrado");

    // O handler do comando registrado grava a densidade no serviço de configuração.
    instantiation_service.invoke_function(|accessor| (command.handler)(accessor, &[]));
    // O corpo do `run` devolve a promessa do `updateValue`: o comando só termina quando a atualização termina.
    let second = Rc::new(PendingUpdateConfigurationService { inner: TestConfigurationService::new(None), update_complete, updates: RefCell::new(Vec::new()) });
    let completion = set_layout_density(second.as_ref(), environment_service.as_ref(), ModernUiDensity::Compact);
    let command_completed_before_update = completion.is_settled();
    resolver.resolve(());
    let command_completed = completion.is_settled();

    assert_eq!(
        (configuration_service.updates.borrow().clone(), second.updates.borrow().clone(), command_completed_before_update, command_completed),
        (
            vec![(LayoutSettings::ModernUiDensity.as_str().to_owned(), Some(ModernUiDensity::Compact.as_str().to_owned()), None)],
            vec![(LayoutSettings::ModernUiDensity.as_str().to_owned(), Some(ModernUiDensity::Compact.as_str().to_owned()), None)],
            false,
            true,
        )
    );
}

fn empty_model() -> ConfigurationModel {
    ConfigurationModel::create_empty_model(NullLogService::shared())
}

fn empty_configuration() -> Configuration {
    Configuration::new(
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        ResourceMap::new(),
        empty_model(),
        ResourceMap::new(),
        NullLogService::shared(),
    )
}

/// `await configurationService.setUserConfiguration(key, value)` e o `onDidChangeConfigurationEmitter.fire({ ... })`.
fn update_configuration(configuration_service: &TestConfigurationService, key: &str, value: Dynamic) {
    let _ = configuration_service.set_user_configuration(key, value, None);
    let change = ConfigurationChange { keys: vec![JsString::from(key)], overrides: Vec::new() };
    let event = ConfigurationChangeEvent::new(change, None, Rc::new(empty_configuration()), None, NullLogService::shared());
    configuration_service.on_did_change_configuration_emitter.fire(Rc::new(event));
}

#[test]
fn applies_startup_density_and_relayouts_when_density_or_enablement_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
        (LayoutSettings::ModernUi.as_str(), Dynamic::Bool(true)),
        (LayoutSettings::ModernUiDensity.as_str(), Dynamic::string(ModernUiDensity::Compact.as_str())),
        (LayoutSettings::ModernUiUppercaseViewHeaders.as_str(), Dynamic::Bool(true)),
    ])));
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let host = TestHost::new();
    guard.add_rc(host.on_did_add_container_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let host_dyn: Rc<dyn ModernUiHost> = host.clone();
    guard.add_rc(ModernUiContribution::new(configuration, host_dyn));
    let ui = Ui::new();
    let pane = guard.add_rc(Pane::new(
        &ui,
        PaneOptions { title: "Test".to_owned(), minimum_body_size: Some(0.0), maximum_body_size: Some(0.0), ..PaneOptions::default() },
        Rc::new(ModernUiTestPane),
    ));
    pane.render();

    let main = TestHost::main_container();
    let auxiliary = LayoutContainerId(2);
    host.add_container(auxiliary, guard.add_rc(DisposableStore::new()));

    let startup_state = (
        [main, auxiliary].map(|container| {
            [
                host.has_class(container, "modern-ui"),
                host.has_class(container, "modern-ui-compact"),
                host.has_class(container, "modern-ui-tabs"),
                host.has_class(container, "modern-ui-notifications-dialogs"),
                host.has_class(container, "modern-ui-uppercase-view-headers"),
            ]
        }),
        pane.minimum_size(),
        host.layout_count.get(),
        host.auxiliary_layout_count.get(),
    );

    update_configuration(&configuration_service, LayoutSettings::ModernUiDensity.as_str(), Dynamic::string(ModernUiDensity::Default.as_str()));
    let default_density_state = (
        host.has_class(main, "modern-ui-compact"),
        host.has_class(auxiliary, "modern-ui-compact"),
        pane.minimum_size(),
        host.layout_count.get(),
        host.auxiliary_layout_count.get(),
    );

    update_configuration(&configuration_service, LayoutSettings::ModernUi.as_str(), Dynamic::Bool(false));
    let after_toggle = (
        [main, auxiliary].map(|container| {
            [
                host.has_class(container, "modern-ui"),
                host.has_class(container, "modern-ui-compact"),
                host.has_class(container, "modern-ui-tabs"),
                host.has_class(container, "modern-ui-notifications-dialogs"),
                host.has_class(container, "modern-ui-uppercase-view-headers"),
            ]
        }),
        pane.minimum_size(),
        host.layout_count.get(),
        host.auxiliary_layout_count.get(),
    );

    assert_eq!(startup_state, ([[true; 5], [true; 5]], 28.0, 0, 0));
    assert_eq!(default_density_state, (false, false, 28.0, 1, 1));
    assert_eq!(after_toggle, ([[false; 5], [false; 5]], 22.0, 2, 2));
}

#[test]
fn switches_connected_editor_tabs_across_windows_and_cleans_up_on_disable_and_disposal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
        (LayoutSettings::ModernUi.as_str(), Dynamic::Bool(true)),
        (LayoutSettings::ModernUiEditorTabStyle.as_str(), Dynamic::string(ModernUiEditorTabStyle::Connected.as_str())),
    ])));
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let host = TestHost::new();
    guard.add_rc(host.on_did_add_container_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let host_dyn: Rc<dyn ModernUiHost> = host.clone();
    let contribution = guard.add_rc(ModernUiContribution::new(configuration, host_dyn));
    let auxiliary = LayoutContainerId(2);
    let get_state = || {
        let containers = host.containers.borrow().clone();
        (
            containers.iter().map(|container| host.has_class(*container, "modern-ui-connected-editor-tabs")).collect::<Vec<_>>(),
            containers.iter().map(|container| host.has_class(*container, "modern-ui-tabs")).collect::<Vec<_>>(),
            host.layout_count.get(),
            host.auxiliary_layout_count.get(),
        )
    };
    let update = |key: &str, value: &str| update_configuration(&configuration_service, key, Dynamic::string(value));
    let update_enabled = |value: bool| update_configuration(&configuration_service, LayoutSettings::ModernUi.as_str(), Dynamic::Bool(value));
    let tab_style = LayoutSettings::ModernUiEditorTabStyle.as_str();

    let startup = get_state();
    host.add_container(auxiliary, guard.add_rc(DisposableStore::new()));
    let auxiliary_startup = get_state();
    update(tab_style, ModernUiEditorTabStyle::Pill.as_str());
    let pill = get_state();
    update(tab_style, ModernUiEditorTabStyle::Connected.as_str());
    let connected = get_state();
    update_enabled(false);
    let disabled = get_state();
    update(tab_style, ModernUiEditorTabStyle::Pill.as_str());
    let changed_while_disabled = get_state();
    update_enabled(true);
    let reenabled = get_state();
    update(tab_style, ModernUiEditorTabStyle::Connected.as_str());
    je_base::lifecycle::IDisposable::dispose(contribution.as_ref());
    let disposed = get_state();

    assert_eq!(startup, (vec![true], vec![true], 0, 0));
    assert_eq!(auxiliary_startup, (vec![true, true], vec![true, true], 0, 0));
    assert_eq!(pill, (vec![false, false], vec![true, true], 1, 1));
    assert_eq!(connected, (vec![true, true], vec![true, true], 2, 2));
    assert_eq!(disabled, (vec![false, false], vec![false, false], 3, 3));
    assert_eq!(changed_while_disabled, (vec![false, false], vec![false, false], 3, 3));
    assert_eq!(reenabled, (vec![false, false], vec![true, true], 4, 4));
    assert_eq!(disposed, (vec![false, false], vec![false, false], 5, 5));
}

#[test]
fn toggles_uppercase_view_headers_without_relayout() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
        (LayoutSettings::ModernUi.as_str(), Dynamic::Bool(true)),
        (LayoutSettings::ModernUiUppercaseViewHeaders.as_str(), Dynamic::Bool(false)),
    ])));
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let host = TestHost::new();
    guard.add_rc(host.on_did_add_container_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let host_dyn: Rc<dyn ModernUiHost> = host.clone();
    guard.add_rc(ModernUiContribution::new(configuration, host_dyn));
    let main = TestHost::main_container();

    let before_toggle = (host.has_class(main, "modern-ui-uppercase-view-headers"), host.layout_count.get());
    update_configuration(&configuration_service, LayoutSettings::ModernUiUppercaseViewHeaders.as_str(), Dynamic::Bool(true));
    let after_toggle = (host.has_class(main, "modern-ui-uppercase-view-headers"), host.layout_count.get());

    assert_eq!(before_toggle, (false, 0));
    assert_eq!(after_toggle, (true, 0));
}

#[test]
fn insets_multi_diff_file_rows_like_explorer_rows_in_both_layout_densities() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench");
    // `read(row)`: o `left` e o `right` calculados são o mesmo recuo (a linha vai de borda a borda do recuo).
    let mut state = |classes: &str| {
        style.set_root_class_name(&format!("monaco-workbench {classes}"));
        let inset = |host| style.list_row_horizontal_inset(host);
        (inset(ListRowHost::PaneBody { chat_view: false }), inset(ListRowHost::MultiDiffFileList))
    };

    let standard = state("");
    let modern = state("modern-ui");
    let compact = state("modern-ui modern-ui-compact");

    assert_eq!((standard, modern, compact), ((0.0, 0.0), (4.0, 4.0), (2.0, 2.0)));
}

/// O valor em px que o `getComputedStyle` escreve.
fn px(value: f64) -> String {
    format!("{value}px")
}

/// Os quatro deslocamentos de uma caixa posicionada, na ordem `top`, `right`, `bottom`, `left`, como o computado (`auto` é
/// o deslocamento que nenhuma regra define).
fn offsets_css(offsets: BoxOffsets) -> [String; 4] {
    [offsets.top, offsets.right, offsets.bottom, offsets.left].map(|offset| offset.map_or_else(|| "auto".to_owned(), px))
}

/// O `border-radius` computado: o shorthand de um valor quando os cantos são iguais, senão os quatro.
fn radii_css(radii: CornerRadii) -> String {
    let CornerRadii(top_left, top_right, bottom_right, bottom_left) = radii;
    if top_left == top_right && top_right == bottom_right && bottom_right == bottom_left {
        px(top_left)
    } else {
        format!("{} {} {} {}", px(top_left), px(top_right), px(bottom_right), px(bottom_left))
    }
}

#[test]
fn supports_isolated_notification_and_dialog_presentation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Os `--vscode-spacing-*` e `--vscode-cornerRadius-large` que o caso escreve na raiz são os valores do tema; só as duas
    // variáveis `--modern-ui-notifications-*` são do shell.
    let theme = unloaded_theme(ThemeType::Dark);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-notifications-dialogs");
    style.set_notification_insets(NotificationInsets { inline: Some(12.0), block_start: Some(24.0), block_end: None });

    let center = style.notifications_center_offsets(NotificationsPlacement::BottomRight).map(offsets_css);
    let top_center = style.notifications_center_offsets(NotificationsPlacement::TopRight).map(offsets_css);
    let toasts = style.notifications_toasts_offsets(NotificationsPlacement::BottomRight).map(offsets_css);
    let top_toasts = style.notifications_toasts_offsets(NotificationsPlacement::TopRight).map(offsets_css);
    let padding = style.notification_item_padding().map(|padding| format!("{} {}", px(padding.vertical), px(padding.horizontal)));

    assert_eq!(
        (
            padding,
            center.as_ref().map(|offsets| (offsets[1].clone(), offsets[2].clone())),
            style.notifications_center_radius().map(px),
            style.notifications_center_row_radius(true).map(radii_css),
            top_center.as_ref().map(|offsets| offsets[0].clone()),
            toasts.as_ref().map(|offsets| (offsets[1].clone(), offsets[2].clone())),
            style.notifications_toast_radius().map(radii_css),
            top_toasts.as_ref().map(|offsets| offsets[0].clone()),
            (px(style.notification_toast_margin_bottom(false)), px(style.notification_toast_margin_bottom(true))),
            style.dialog_padding().map(px),
            style.dialog_min_width().map(px),
        ),
        (
            Some("6px 2px".to_owned()),
            Some(("12px".to_owned(), "36px".to_owned())),
            Some("8px".to_owned()),
            Some("0px 0px 8px 8px".to_owned()),
            Some("24px".to_owned()),
            Some(("8px".to_owned(), "32px".to_owned())),
            Some("8px".to_owned()),
            Some("20px".to_owned()),
            ("0px".to_owned(), "4px".to_owned()),
            Some("4px".to_owned()),
            Some("440px".to_owned()),
        )
    );
}

/// O `ModernUITestPane` dentro de `.part.<host>`, com a pilha e a orientação do caso.
fn pane_state(host: PaneHost, first_in_stack: bool, horizontal: bool) -> PaneState {
    PaneState { host, first_in_stack, horizontal, header_hovered: false }
}

#[test]
fn uses_part_specific_pane_colors_and_only_draws_panel_header_separators_in_vertical_layouts() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[
        ("sideBar.background", "#FF8888"),
        ("sideBarSectionHeader.border", "#FF0000"),
        ("panel.background", "#8888FF"),
        ("panelSectionHeader.border", "#0000FF"),
    ]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui");
    let rgb = |color: Option<Color>| color.map(|color| css_rgb(&color));

    // `followingSideBarPane` e `followingVerticalPane`: o segundo pane de uma pilha vertical.
    let following_side_bar = pane_state(PaneHost::SideBar, false, false);
    let first_vertical = pane_state(PaneHost::Panel, true, false);
    let following_vertical = pane_state(PaneHost::Panel, false, false);
    let first_horizontal = pane_state(PaneHost::Panel, true, true);
    let following_horizontal = pane_state(PaneHost::Panel, false, true);
    // `leftBorder: '#FF0000'` só vale no pane horizontal; a borda inline é `1px solid` dessa cor.
    let left_border = Color::from_hex("#FF0000");

    assert_eq!(
        (
            rgb(style.pane_background(following_side_bar.host)),
            rgb(style.pane_header_background(following_side_bar)),
            rgb(style.pane_header_separator_color(following_side_bar.host)),
            rgb(style.pane_background(following_vertical.host)),
            rgb(style.pane_header_background(following_vertical)),
            rgb(style.pane_header_separator_color(following_vertical.host)),
            style.pane_header_separator_visible(first_vertical),
            style.pane_header_separator_visible(following_vertical),
            style.pane_header_border_top_width().map(px),
            style.pane_header_separator_visible(first_horizontal),
            style.pane_header_separator_visible(following_horizontal),
            (following_horizontal.horizontal.then(|| px(1.0)), following_horizontal.horizontal.then(|| css_rgb(&left_border))),
        ),
        (
            Some("rgb(255, 136, 136)".to_owned()),
            Some("rgb(255, 136, 136)".to_owned()),
            Some("rgb(255, 0, 0)".to_owned()),
            Some("rgb(136, 136, 255)".to_owned()),
            Some("rgb(136, 136, 255)".to_owned()),
            Some("rgb(0, 0, 255)".to_owned()),
            false,
            true,
            Some("0px".to_owned()),
            false,
            false,
            (Some("1px".to_owned()), Some("rgb(255, 0, 0)".to_owned())),
        )
    );
}

#[test]
fn falls_back_to_the_surface_border_for_pane_header_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("surface.border", "#123456")]);
    // O DOM do original só tem `--vscode-surface-border`: sem `--vscode-panel-border`, a cadeia do painel cai na superfície.
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui").with_explicit_colors_only();

    let colors = [PaneHost::SideBar, PaneHost::Panel].map(|host| style.pane_header_separator_color(host).map(|color| css_rgb(&color)));

    assert_eq!(colors, [Some("rgb(18, 52, 86)".to_owned()), Some("rgb(18, 52, 86)".to_owned())]);
}

#[test]
fn provides_opaque_default_colors_for_high_contrast_pane_dividers() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let divider_state = |theme: ColorTheme| {
        let colors = ["sideBarSectionHeader.border", "panelSectionHeader.border", "panelSection.border"].map(|id| theme.resolve_color(id, true));
        (colors.clone().map(|color| color.is_some()), colors.map(|color| color.map(|color| color.is_opaque())))
    };

    let hc_dark = divider_state(unloaded_theme(ThemeType::HighContrastDark));
    let hc_light = divider_state(unloaded_theme(ThemeType::HighContrastLight));

    let expected = ([true, true, true], [Some(true), Some(true), Some(true)]);
    assert_eq!((hc_dark, hc_light), (expected, expected));
}

#[test]
fn respects_customized_high_contrast_pane_dividers() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let colors = [
        ("contrastBorder", "#6FC3DF"),
        ("sideBarSectionHeader.border", "#6F7783"),
        ("panelSectionHeader.border", "#6F7783"),
        ("panelSection.border", "#6F7783"),
    ];
    // `root.classList.replace('hc-black', 'hc-light')` troca o tipo do tema e a classe da raiz.
    let get_divider_colors = |theme_type: ThemeType, root_class_name: &str| {
        let mut theme = unloaded_theme(theme_type);
        theme.set_custom_colors(colors.iter().copied());
        let style = ModernUiStyle::new(&theme, root_class_name);
        let rgb = |color: Option<Color>| color.map(|color| css_rgb(&color));
        (
            rgb(style.pane_header_separator_color(PaneHost::SideBar)),
            rgb(style.pane_header_separator_color(PaneHost::Panel)),
            // `leftBorder: 'var(--vscode-panelSection-border)'`.
            rgb(style.variable("--vscode-panelSection-border")),
        )
    };

    let dark = get_divider_colors(ThemeType::HighContrastDark, "monaco-workbench modern-ui hc-black");
    let light = get_divider_colors(ThemeType::HighContrastLight, "monaco-workbench modern-ui hc-light");

    let expected = (Some("rgb(111, 119, 131)".to_owned()), Some("rgb(111, 119, 131)".to_owned()), Some("rgb(111, 119, 131)".to_owned()));
    assert_eq!((dark, light), (expected.clone(), expected));
}

/// O `IWorkspace` literal `{ id: 'test', folders: [], configuration }` do caso.
struct TestWorkspace {
    configuration: Option<URI>,
}

impl IWorkspace for TestWorkspace {
    fn id(&self) -> String {
        "test".to_string()
    }

    fn folders(&self) -> Vec<WorkspaceFolder> {
        Vec::new()
    }

    fn transient(&self) -> bool {
        false
    }

    fn configuration(&self) -> Option<URI> {
        self.configuration.clone()
    }

    fn name(&self) -> Option<String> {
        None
    }
}

#[test]
fn explorer_title_casing_follows_the_workspace_name_decision() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let environment = TestEnvironmentService::new();
    let untitled_workspaces_home = environment.untitled_workspaces_home();
    // O original lê o `text-transform` calculado dos títulos (`capitalize` sem a classe `preserve-*`, `none` com ela, e
    // `uppercase` com `modern-ui-uppercase-view-headers`); sem DOM o caso confere a decisão que liga a classe.
    let preserve_case = |state: WorkbenchState, configuration: Option<URI>| {
        should_preserve_workspace_name_case(state, &TestWorkspace { configuration }, &*environment)
    };

    let untitled = preserve_case(WorkbenchState::Workspace, Some(join_path(&untitled_workspaces_home, &["1234", "workspace.json"])));
    let named = preserve_case(WorkbenchState::Workspace, Some(uri_file("/some/path/myWorkspace.code-workspace")));
    let folder = preserve_case(WorkbenchState::Folder, None);

    assert_eq!((untitled, named, folder), (false, true, true));
}

/// `ColorThemeData.createUnloadedTheme(id)`: o tema sem arquivo e sem cores, só com os padrões do registro.
fn unloaded_theme(theme_type: ThemeType) -> ColorTheme {
    ColorTheme::unloaded("", theme_type, HashMap::new())
}

/// `colorRegistry.getColors().find(color => color.id === id)?.defaults`.
fn registered_defaults(id: &str) -> Option<Defaults> {
    color_registry().get(id).and_then(|color| color.defaults)
}

fn is_registered(id: &str) -> bool {
    color_registry().get(id).is_some()
}

const SURFACE_BORDER: Option<Defaults> = Some(Defaults::Single(ColorValue::Id("surface.border")));

/// O `<div class="monaco-workbench ...">` com as variáveis `--vscode-X` do `style.setProperty`: um tema de cores `X`.
fn theme_with_colors(colors: &[(&str, &str)]) -> ColorTheme {
    let mut theme = unloaded_theme(ThemeType::Dark);
    theme.set_custom_colors(colors.iter().copied());
    theme
}

#[test]
fn uses_the_editor_surface_border_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("editor.border", "#123456"), ("surface.border", "#654321")]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui floating-panels");
    let editor = ClassList::new("part editor");

    assert_eq!(
        (registered_defaults("editor.border"), style.editor_border_color(&editor).map(|color| css_rgb(&color))),
        (SURFACE_BORDER, Some("rgb(18, 52, 86)".to_owned()))
    );
}

#[test]
fn uses_the_modern_activity_bar_border_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("modernActivityBar.border", "#123456"), ("surface.border", "#654321")]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui floating-panels");
    let activity_bar = style.floating_part_border_colors(&ClassList::new("part activitybar left"));
    // Os outros cartões mantêm a borda de superfície compartilhada, menos a aresta que cedem à barra como emenda.
    let side_bar = style.floating_part_border_colors(&ClassList::new("part sidebar left"));

    assert_eq!(
        (registered_defaults("modernActivityBar.border"), activity_bar.map(|colors| colors.to_css()), side_bar.map(|colors| colors.to_css())),
        (
            SURFACE_BORDER,
            Some("rgb(18, 52, 86)".to_owned()),
            Some("rgb(101, 67, 33) rgb(101, 67, 33) rgb(101, 67, 33) rgba(0, 0, 0, 0)".to_owned()),
        )
    );
}

#[test]
fn uses_the_modern_panel_border_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("modernPanel.border", "#123456"), ("surface.border", "#654321")]);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui floating-panels");
    let panel = ClassList::new("part panel bottom");
    let panel_border = style.floating_part_border_colors(&panel).map(|colors| colors.to_css());

    style.set_root_class_name("monaco-workbench modern-ui floating-panels modern-ui-compact");

    assert_eq!(
        (registered_defaults("modernPanel.border"), panel_border, style.floating_card_stroke_color(&panel).map(|color| color.to_string())),
        (SURFACE_BORDER, Some("rgb(18, 52, 86)".to_owned()), Some("#123456".to_owned()))
    );
}

#[test]
fn uses_the_modern_ui_shell_background_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("modernUI.shellBackground", "#123456"), ("titleBar.activeBackground", "#654321")]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui floating-panels");
    // A raiz do workbench e a grade (`monaco-grid-view`) pintam o mesmo fundo do shell.
    let shell = style.shell_background().map(|color| css_rgb(&color));

    assert_eq!(
        (
            registered_defaults("modernUI.shellBackground"),
            registered_defaults("modernUI.inactiveShellBackground"),
            shell.clone(),
            shell,
        ),
        (
            Some(Defaults::Single(ColorValue::Id("titleBar.activeBackground"))),
            Some(Defaults::Single(ColorValue::Id("titleBar.inactiveBackground"))),
            Some("rgb(18, 52, 86)".to_owned()),
            Some("rgb(18, 52, 86)".to_owned()),
        )
    );
}

#[test]
fn uses_opaque_surface_border_defaults() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let dark_theme = unloaded_theme(ThemeType::Dark);
    let light_theme = unloaded_theme(ThemeType::Light);
    let dark_surface_border = dark_theme.resolve_color("surface.border", true);
    let dark_editor_border = dark_theme.resolve_color("editor.border", true);
    let light_surface_border = light_theme.resolve_color("surface.border", true);
    let light_editor_border = light_theme.resolve_color("editor.border", true);
    let equals = |a: &Option<Color>, b: &Option<Color>| a.as_ref().zip(b.as_ref()).map(|(a, b)| a.equals(b));
    let is_opaque = |color: &Option<Color>| color.as_ref().map(Color::is_opaque);

    assert_eq!(
        (
            is_opaque(&dark_surface_border),
            is_opaque(&dark_editor_border),
            equals(&dark_editor_border, &dark_surface_border),
            is_opaque(&light_surface_border),
            is_opaque(&light_editor_border),
            equals(&light_editor_border, &light_surface_border),
        ),
        (Some(true), Some(true), Some(true), Some(true), Some(true), Some(true))
    );
}

#[test]
fn inherits_the_customized_activity_bar_background_when_inactive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme = unloaded_theme(ThemeType::Dark);
    theme.set_custom_colors([("modernActivityBar.background", "#123456")]);

    assert_eq!(
        (
            theme.resolve_color("modernActivityBar.background", true).map(|color| color.to_string()),
            theme.resolve_color("modernActivityBar.inactiveBackground", true).map(|color| color.to_string()),
        ),
        (Some("#123456".to_owned()), Some("#123456".to_owned()))
    );
}

#[test]
fn supports_deprecated_modern_activity_bar_item_colors() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme = unloaded_theme(ThemeType::Dark);
    let deprecated_color_ids =
        ["modernActivityBar.activeBackground", "modernActivityBar.activeForeground", "modernActivityBar.hoverBackground", "modernActivityBar.hoverForeground"];
    theme.set_custom_colors([
        (deprecated_color_ids[0], "#112233"),
        (deprecated_color_ids[1], "#223344"),
        (deprecated_color_ids[2], "#334455"),
        (deprecated_color_ids[3], "#445566"),
    ]);

    let item_color_ids = [
        "modernActivityBarItem.activeBackground",
        "modernActivityBarItem.activeForeground",
        "modernActivityBarItem.hoverBackground",
        "modernActivityBarItem.hoverForeground",
    ];
    assert_eq!(
        (
            item_color_ids.map(|id| theme.resolve_color(id, true).map(|color| color.to_string())),
            deprecated_color_ids.map(|id| color_registry().get(id).is_some_and(|color| color.deprecation_message.is_some())),
        ),
        (
            [Some("#112233".to_owned()), Some("#223344".to_owned()), Some("#334455".to_owned()), Some("#445566".to_owned())],
            [true, true, true, true],
        )
    );
}

const fn single(value: ColorValue) -> Option<Defaults> {
    Some(Defaults::Single(value))
}

#[test]
fn uses_the_registered_modern_tab_colors() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // As cores das variáveis `--vscode-modernTab-*` e `--vscode-modernEditorTab-*` do `style.setProperty` são as
    // customizações do tema; o estilo resolvido é o que o `getComputedStyle` lê no original.
    let theme = theme_with_colors(&[
        ("modernTab.activeBackground", "#123456"),
        ("modernTab.activeForeground", "#abcdef"),
        ("modernTab.hoverBackground", "#654321"),
        ("modernTab.hoverForeground", "#fedcba"),
        ("modernEditorTab.activeActionBackground", "#102030"),
        ("modernEditorTab.activeBackground", "#234567"),
        ("modernEditorTab.activeForeground", "#bcdef0"),
        ("modernEditorTab.activeHoverActionBackground", "#203040"),
        ("modernEditorTab.activeHoverBackground", "#456789"),
        ("modernEditorTab.inactiveBackground", "#345678"),
        ("modernEditorTab.hoverActionBackground", "#302010"),
        ("modernEditorTab.hoverBackground", "#765432"),
        ("modernEditorTab.hoverForeground", "#edcba9"),
        ("modernEditorTab.selectedActionBackground", "#554433"),
    ]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui-tabs");
    let rgb = |color: Option<Color>| color.map(|color| css_rgb(&color));
    // O grupo do original não leva `active`, as abas `active` e `selected` são as do caso.
    let group = EditorTabState::default();
    let active_tab = EditorTabState { active: true, ..group };
    let selected_tab = EditorTabState { selected: true, actions_focus_within: true, ..group };
    let checked = ActionTabState { checked: true, ..ActionTabState::default() };
    let modern_tab_ids = ["modernTab.activeBackground", "modernTab.activeForeground", "modernTab.hoverBackground", "modernTab.hoverForeground"];
    let editor_ids = [
        "modernEditorTab.activeActionBackground",
        "modernEditorTab.activeBackground",
        "modernEditorTab.activeForeground",
        "modernEditorTab.activeHoverActionBackground",
        "modernEditorTab.activeHoverBackground",
        "editorGroupHeader.connectedTabsBackground",
        "modernEditorTab.inactiveBackground",
        "modernEditorTab.hoverActionBackground",
        "modernEditorTab.hoverBackground",
        "modernEditorTab.hoverForeground",
        "modernEditorTab.selectedActionBackground",
    ];

    assert_eq!(
        (
            modern_tab_ids.map(is_registered),
            modern_tab_ids.map(registered_defaults),
            editor_ids.map(is_registered),
            editor_ids.map(registered_defaults),
            [
                rgb(style.pane_tab_indicator_background(checked)),
                rgb(style.action_tab_foreground(checked)),
                rgb(style.editor_tab_fill_background(active_tab)),
                rgb(style.editor_tab_foreground(active_tab)),
                rgb(style.editor_tab_fill_background(group)),
                rgb(style.variable("--modern-ui-tab-hover-background")),
                rgb(style.variable("--vscode-modernTab-hoverForeground")),
                rgb(style.variable("--modern-ui-editor-tab-hover-background")),
                rgb(style.variable("--modern-ui-editor-tab-hover-foreground")),
                rgb(style.variable("--modern-ui-editor-tab-active-hover-background")),
                rgb(style.variable("--modern-ui-editor-tab-action-active-background")),
                rgb(style.variable("--modern-ui-editor-tab-action-active-hover-background")),
                rgb(style.variable("--modern-ui-editor-tab-action-hover-background")),
                rgb(style.editor_tab_actions_background(selected_tab)),
                rgb(style.settings_tab_background(checked)),
                rgb(style.action_tab_foreground(checked)),
            ],
        ),
        (
            [true; 4],
            [
                single(Id("list.inactiveSelectionBackground")),
                single(ColorValue::Transform(&ColorTransform::OneOf(&[Id("list.inactiveSelectionForeground"), Id("foreground")]))),
                single(Id("list.hoverBackground")),
                single(ColorValue::Transform(&ColorTransform::OneOf(&[Id("list.hoverForeground"), Id("foreground")]))),
            ],
            [true; 11],
            [
                single(ColorValue::Transform(&ColorTransform::Opaque { value: Id("modernEditorTab.activeBackground"), background: Id("editor.background") })),
                single(Id("modernTab.activeBackground")),
                single(Id("modernTab.activeForeground")),
                single(ColorValue::Transform(&ColorTransform::Opaque { value: Id("modernEditorTab.activeHoverBackground"), background: Id("editor.background") })),
                single(Id("modernEditorTab.hoverBackground")),
                single(Id("editorGroupHeader.tabsBackground")),
                single(ColorValue::Rgba(0, 0, 0, 0.0)),
                single(ColorValue::Transform(&ColorTransform::Opaque { value: Id("modernEditorTab.hoverBackground"), background: Id("editor.background") })),
                single(Id("modernTab.hoverBackground")),
                single(Id("modernTab.hoverForeground")),
                single(ColorValue::Transform(&ColorTransform::Opaque { value: Id("tab.selectedBackground"), background: Id("editor.background") })),
            ],
            [
                "rgb(18, 52, 86)",
                "rgb(171, 205, 239)",
                "rgb(35, 69, 103)",
                "rgb(188, 222, 240)",
                "rgb(52, 86, 120)",
                "rgb(101, 67, 33)",
                "rgb(254, 220, 186)",
                "rgb(118, 84, 50)",
                "rgb(237, 203, 169)",
                "rgb(69, 103, 137)",
                "rgb(16, 32, 48)",
                "rgb(32, 48, 64)",
                "rgb(48, 32, 16)",
                "rgb(85, 68, 51)",
                "rgb(18, 52, 86)",
                "rgb(171, 205, 239)",
            ]
            .map(|expected| Some(expected.to_owned())),
        )
    );
}

#[test]
fn tab_divider_inherits_tab_border_unless_customized() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme = unloaded_theme(ThemeType::Dark);
    theme.set_custom_colors([("tab.border", "#123456")]);
    let inherited = theme.resolve_color("tab.divider", true).map(|color| color.to_string());
    theme.set_custom_colors([("tab.border", "#123456"), ("tab.divider", "#654321")]);
    let customized = theme.resolve_color("tab.divider", true).map(|color| color.to_string());

    assert_eq!((inherited, customized), (Some("#123456".to_owned()), Some("#654321".to_owned())));
}

/// O `rgb(...)` do computado de uma cor opcional.
fn rgb_of(color: Option<Color>) -> Option<String> {
    color.map(|color| css_rgb(&color))
}

#[test]
fn pane_composite_actions_fill_regular_and_agents_headers() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    // O contêiner do título é mais alto (40) que os 32px fixos do Modern UI, para provar a altura fixa e não o 100%.
    let regular = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs");
    let agents = ModernUiStyle::new(&theme, "monaco-workbench modern-ui-tabs");
    let regular_box = regular.pane_composite_action_box(40.0).unwrap();
    let agents_box = agents.pane_composite_action_box(35.0).unwrap();
    let agents_icon_box = agents.pane_composite_action_box(35.0).unwrap();
    let (regular_badge_top, regular_badge_right) = regular.pane_composite_badge_content_offsets().unwrap();
    let (agents_badge_top, agents_badge_right) = agents.pane_composite_badge_content_offsets().unwrap();

    assert_eq!(
        [
            px(regular_box.item_height),
            px(regular_box.indicator_height),
            px(regular_badge_top),
            px(regular_badge_right),
            px(agents_box.item_height),
            px(agents_box.indicator_height),
            px(agents_icon_box.item_height),
            px(agents_icon_box.indicator_height),
            px(agents_badge_top),
            px(agents_badge_right),
        ],
        ["32px", "24px", "13px", "2px", "35px", "24px", "35px", "24px", "13px", "2px"].map(str::to_owned)
    );
    assert_eq!((agents_icon_box.indicator_top_inset, agents_icon_box.indicator_bottom_inset), (5.5, 5.5));
}

#[test]
fn panel_title_tabs_drop_the_classic_1px_title_border_so_the_32px_pills_center_in_the_32px_bar() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs");
    let title_border_top = style.bottom_panel_title_border_top_width(&ClassList::new("part pane-composite-part panel basepanel bottom"));
    // `border-top: none`: largura 0 e estilo `none`.
    let (width, border_style) = title_border_top.map_or((1.0, "solid"), |width| (width, "none"));

    assert_eq!(
        (px(width), border_style, px(style.pane_composite_action_box(32.0).unwrap().item_border_block)),
        ("0px".to_owned(), "none", "4px".to_owned())
    );
}

#[test]
fn pane_composite_actions_use_regular_label_weight() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let regular = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs");
    let agents = ModernUiStyle::new(&theme, "monaco-workbench modern-ui-tabs");

    // O item auxiliar (`.auxiliarybar`) usa a mesma regra do regular: as ações não são de ícone.
    assert_eq!(
        (regular.pane_composite_label_font_weight(false), regular.pane_composite_label_font_weight(false), agents.pane_composite_label_font_weight(false)),
        (Some(400.0), Some(400.0), Some(400.0))
    );
}

#[test]
fn pane_composite_overflow_uses_the_icon_foreground() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("icon.foreground", "#123456")]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs");

    // O `color` inline `rgba(231, 231, 231, 0.6)` perde para o `!important` da regra.
    assert_eq!(rgb_of(style.pane_composite_overflow_foreground()), Some("rgb(18, 52, 86)".to_owned()));
}

#[test]
fn preserves_modern_ui_activity_indicators_badges_and_horizontal_pane_dividers() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[
        ("modernActivityBarItem.activeBackground", "#123456"),
        ("modernActivityBarItem.activeForeground", "#abcdef"),
        ("modernTab.activeBackground", "#654321"),
        ("modernTab.activeForeground", "#fedcba"),
    ]);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs");
    style.set_activity_bar_metrics(ActivityBarMetrics { width: Some(36.0), action_height: Some(36.0) });
    let agents = ModernUiStyle::new(&theme, "monaco-workbench modern-ui-tabs");
    let activity_bar = ClassList::new("activitybar");
    let part = ClassList::new("part pane-composite-part");
    let checked = ActivityItemState { checked: true, hovered: false };
    let horizontal_checked = ActionTabState { checked: true, icon: true, hovered: false };
    let (indicator, radius) = style.activity_bar_indicator_box(&activity_bar).unwrap();
    let badge = style.activity_bar_badge_content(&activity_bar, true).unwrap();
    let activity_colors = [
        "modernActivityBar.background",
        "modernActivityBar.inactiveBackground",
        "modernActivityBarItem.activeBackground",
        "modernActivityBarItem.activeForeground",
        "modernActivityBarItem.hoverBackground",
        "modernActivityBarItem.hoverForeground",
    ];

    assert_eq!(
        (
            activity_colors.map(is_registered),
            rgb_of(style.activity_bar_indicator_background(checked)),
            rgb_of(style.activity_bar_label_foreground(checked)),
            rgb_of(agents.pane_tab_indicator_background(horizontal_checked)),
            rgb_of(agents.action_tab_foreground(horizontal_checked)),
            (px(indicator.width), px(indicator.height), px(radius)),
            (px(badge.top), badge.width.map(px), px(badge.height)),
            (
                style.pane_composite_header_footer_border_width(&part).map(px),
                style.pane_composite_header_overflow_visible(&part),
                style.pane_composite_header_footer_border_width(&part).map(px),
            ),
        ),
        (
            [true; 6],
            Some("rgb(18, 52, 86)".to_owned()),
            Some("rgb(171, 205, 239)".to_owned()),
            Some("rgb(101, 67, 33)".to_owned()),
            Some("rgb(254, 220, 186)".to_owned()),
            ("32px".to_owned(), "32px".to_owned(), "4px".to_owned()),
            ("18px".to_owned(), Some("16px".to_owned()), "16px".to_owned()),
            (Some("0px".to_owned()), true, Some("0px".to_owned())),
        )
    );
}

/// Os quatro `border-*-radius` computados, na ordem `top-left`, `top-right`, `bottom-right`, `bottom-left`.
fn radii_css_list(radii: CornerRadii) -> [String; 4] {
    let CornerRadii(top_left, top_right, bottom_right, bottom_left) = radii;
    [top_left, top_right, bottom_right, bottom_left].map(px)
}

/// `ModernUiStyle` de uma raiz flutuante com as variáveis `--activity-bar-*` dos casos da barra de atividade.
fn floating_style<'a>(theme: &'a ColorTheme, root_class_name: &str) -> ModernUiStyle<'a> {
    let mut style = ModernUiStyle::new(theme, root_class_name);
    style.set_activity_bar_metrics(ActivityBarMetrics { width: Some(36.0), action_height: Some(36.0) });
    style
}

/// A geometria que o caso mede nas duas trilhas: a largura e o centro do item, a margem da janela, o vão até a barra
/// lateral e a distância do indicador até ela, na ordem do objeto do original.
fn rail_measures(style: &ModernUiStyle, rail: &str, side_bar: &str, rail_on_left: bool) -> [f64; 5] {
    let rail_class = ClassList::new(rail);
    let geometry = style.activity_rail_geometry(&rail_class).unwrap();
    let rail_margins = style.floating_part_margins(&rail_class).unwrap();
    let side_bar_margins = style.floating_part_margins(&ClassList::new(side_bar)).unwrap();
    if rail_on_left {
        let gap = side_bar_margins.left + rail_margins.right;
        [geometry.action_width, geometry.action_center_offset, rail_margins.left, gap, gap + geometry.width - geometry.indicator_right]
    } else {
        let gap = rail_margins.left + side_bar_margins.right;
        [geometry.action_width, geometry.action_center_offset, rail_margins.right, gap, gap + geometry.indicator_left]
    }
}

#[test]
fn centers_activity_bar_items_within_floating_navigation_rails() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = floating_style(&theme, "monaco-workbench modern-ui modern-ui-compact floating-panels");

    assert_eq!(
        (
            rail_measures(&style, "part activitybar left", "part sidebar", true),
            rail_measures(&style, "part activitybar right", "part sidebar", false),
        ),
        ([36.0, 0.0, 0.0, 0.0, 4.0], [36.0, 0.0, 0.0, 0.0, 4.0])
    );
}

#[test]
fn centers_default_density_activity_bar_items_and_meets_the_side_bar_flush() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = floating_style(&theme, "monaco-workbench modern-ui floating-panels");
    let left = rail_measures(&style, "part activitybar left", "part sidebar left", true);
    let right = rail_measures(&style, "part activitybar right", "part sidebar right", false);
    let rail_widths = ["part activitybar left", "part activitybar right"].map(|rail| style.activity_rail_geometry(&ClassList::new(rail)).unwrap().width);

    // A trilha leva o vão de 4px da janela de um lado, encontra a barra lateral sem vão do outro e centra a coluna de
    // ícones na faixa própria de 8px, independente do vão: o cartão fica com 44px.
    assert_eq!(
        ((left[0], left[1], left[2], left[3]), (right[0], right[1], right[2], right[3]), rail_widths),
        ((36.0, 0.0, 4.0, 0.0), (36.0, 0.0, 4.0, 0.0), [44.0, 44.0])
    );
}

#[test]
fn rounds_activity_bar_corners_exposed_by_horizontal_panels() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let corners = |layout_classes: &str| {
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui floating-panels {layout_classes}"));
        let css = |rail: &str| radii_css_list(style.activity_bar_corner_radii(&ClassList::new(rail)).unwrap());
        (css("part activitybar left"), css("part activitybar right"))
    };
    let expected = |left: [&str; 4], right: [&str; 4]| (left.map(str::to_owned), right.map(str::to_owned));

    assert_eq!(
        [
            corners("panel-position-bottom panel-alignment-left"),
            corners("panel-position-bottom panel-alignment-right"),
            corners("panel-position-bottom panel-alignment-justify"),
            corners("panel-position-bottom panel-alignment-center"),
            corners("panel-position-top panel-alignment-left"),
            corners("panel-position-top panel-alignment-right"),
            corners("panel-position-top panel-alignment-justify"),
            corners("panel-position-bottom panel-alignment-justify nopanel"),
            corners("modern-ui-compact panel-position-bottom panel-alignment-justify"),
        ],
        [
            expected(["8px", "0px", "8px", "8px"], ["0px", "8px", "8px", "0px"]),
            expected(["8px", "0px", "0px", "8px"], ["0px", "8px", "8px", "8px"]),
            expected(["8px", "0px", "8px", "8px"], ["0px", "8px", "8px", "8px"]),
            expected(["8px", "0px", "0px", "8px"], ["0px", "8px", "8px", "0px"]),
            expected(["8px", "8px", "0px", "8px"], ["0px", "8px", "8px", "0px"]),
            expected(["8px", "0px", "0px", "8px"], ["8px", "8px", "8px", "0px"]),
            expected(["8px", "8px", "0px", "8px"], ["8px", "8px", "8px", "0px"]),
            expected(["8px", "0px", "0px", "8px"], ["0px", "8px", "8px", "0px"]),
            expected(["0px", "0px", "0px", "0px"], ["0px", "0px", "0px", "0px"]),
        ]
    );
}

#[test]
fn keeps_a_gutter_between_horizontal_panels_and_the_right_activity_bar() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let gap = |layout_classes: &str| {
        let style = floating_style(&theme, &format!("monaco-workbench modern-ui floating-panels panel-position-bottom {layout_classes}"));
        let panel = style.floating_part_margins(&ClassList::new("part panel bottom")).unwrap();
        let activity_bar = style.floating_part_margins(&ClassList::new("part activitybar right")).unwrap();
        (activity_bar.left + panel.right, px(panel.right), px(activity_bar.left))
    };
    let expected = |gap: f64, panel_margin_right: &str, activity_bar_margin_left: &str| (gap, panel_margin_right.to_owned(), activity_bar_margin_left.to_owned());

    assert_eq!(
        [gap("panel-alignment-justify"), gap("panel-alignment-right"), gap("panel-alignment-justify nosidebar"), gap("panel-alignment-justify modern-ui-compact")],
        [expected(4.0, "4px", "0px"), expected(4.0, "4px", "0px"), expected(4.0, "0px", "4px"), expected(0.0, "0px", "0px")]
    );
}

#[test]
fn centers_status_bar_items_within_the_status_bar_background() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let measure = |class_name: &str, status_bar_height: f64| {
        let style = ModernUiStyle::new(&theme, class_name);
        let padding = style.status_bar_padding();
        // O item é um `display: inline-block` de `height: 100%`; o rótulo, um flex de `height: 100%` dentro dele. O
        // contêiner de itens preenche a caixa de conteúdo, então o item começa depois do `padding-top` da barra.
        let item_height = style.status_bar_item_height(status_bar_height);
        let label_height = item_height;
        let center_offset = padding.top + item_height / 2.0 - status_bar_height / 2.0;
        (px(padding.top), px(padding.bottom), item_height, label_height, center_offset)
    };

    assert_eq!(
        [
            measure("monaco-workbench modern-ui floating-panels", 28.0),
            measure("monaco-workbench modern-ui modern-ui-compact floating-panels", 26.0),
            measure("monaco-workbench", 22.0),
        ],
        [
            ("2px".to_owned(), "2px".to_owned(), 24.0, 24.0, 0.0),
            ("2px".to_owned(), "2px".to_owned(), 22.0, 22.0, 0.0),
            ("0px".to_owned(), "0px".to_owned(), 22.0, 22.0, 0.0),
        ]
    );
}

#[test]
fn preserves_status_bar_item_margin_exceptions_across_layout_densities() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let scenarios = [
        "",
        "compact-left",
        "compact-left compact-right",
        "compact-right",
        "has-background-color",
        "has-background-color compact-left",
        "has-background-color compact-right",
    ];
    let measure_density = |compact: bool| {
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui floating-panels{}", if compact { " modern-ui-compact" } else { "" }));
        scenarios.map(|classes| {
            let margin = style.status_bar_label_margin(&ClassList::new(&format!("statusbar-item {classes}")));
            [px(margin.left), px(margin.right)]
        })
    };
    let expected = |margins: [[&str; 2]; 7]| margins.map(|pair| pair.map(str::to_owned));

    assert_eq!(
        (measure_density(false), measure_density(true)),
        (
            expected([["3px", "3px"], ["0px", "5px"], ["0px", "0px"], ["5px", "0px"], ["0px", "0px"], ["0px", "0px"], ["0px", "0px"]]),
            expected([["2px", "2px"], ["0px", "5px"], ["0px", "0px"], ["5px", "0px"], ["0px", "0px"], ["0px", "0px"], ["0px", "0px"]]),
        )
    );
}

#[test]
fn keeps_colored_edge_item_hover_surfaces_flush_without_changing_their_content_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let measure = |compact: bool, alignment: &str, colored: bool, joined_class: Option<&str>| {
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui floating-panels{}", if compact { " modern-ui-compact" } else { "" }));
        let edge_class = if alignment == "left" { "first-visible-item" } else { "last-visible-item" };
        let item = ClassList::new(&format!(
            "statusbar-item {alignment} {edge_class}{}{}",
            if colored { " has-background-color" } else { "" },
            joined_class.map_or(String::new(), |class| format!(" {class}"))
        ));
        let (item_padding, label_padding) = (style.status_bar_item_padding(&item), style.status_bar_label_padding(&item));
        [px(item_padding.left), px(item_padding.right), px(label_padding.left), px(label_padding.right)]
    };
    let expected = |values: [&str; 4]| values.map(str::to_owned);
    let standard_left = measure(false, "left", false, None);

    assert_eq!(
        (
            [standard_left[0].clone(), standard_left[2].clone()],
            [
                measure(false, "left", true, None),
                measure(false, "left", true, Some("compact-right")),
                measure(false, "right", true, None),
                measure(false, "right", true, Some("compact-left")),
            ],
            [
                measure(true, "left", true, None),
                measure(true, "left", true, Some("compact-right")),
                measure(true, "right", true, None),
                measure(true, "right", true, Some("compact-left")),
            ],
        ),
        (
            ["4px".to_owned(), "4px".to_owned()],
            [expected(["0px", "0px", "8px", "6px"]), expected(["0px", "0px", "8px", "6px"]), expected(["0px", "0px", "6px", "8px"]), expected(["0px", "0px", "6px", "8px"])],
            [expected(["0px", "0px", "4px", "4px"]), expected(["0px", "0px", "4px", "4px"]), expected(["0px", "0px", "4px", "6px"]), expected(["0px", "0px", "4px", "6px"])],
        )
    );
}

#[test]
fn compact_status_bar_keeps_its_horizontal_padding_independently_of_the_panel_perimeter() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let padding = |activity_bar_classes: &str| {
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui modern-ui-compact floating-panels {activity_bar_classes}"));
        let padding = style.status_bar_padding();
        [px(padding.left), px(padding.right)]
    };
    let expected = ["4px".to_owned(), "4px".to_owned()];

    assert_eq!(
        [padding(""), padding("activitybar-compact"), padding("noactivitybar"), padding("activitybar-compact noactivitybar")],
        [expected.clone(), expected.clone(), expected.clone(), expected]
    );
}

#[test]
fn keeps_the_connected_editor_tab_color_audit_exhaustive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ids: Vec<&str> = color_registry().get_colors().iter().map(|color| color.id).collect();
    let sorted_with_prefix = |prefix: &str| {
        let mut matching: Vec<&str> = ids.iter().copied().filter(|id| id.starts_with(prefix)).collect();
        matching.sort_unstable();
        matching
    };
    let editor_group_header_colors: Vec<&str> =
        ["editorGroupHeader.connectedTabsBackground", "editorGroupHeader.tabsBackground", "editorGroupHeader.tabsBorder"].into_iter().filter(|id| ids.contains(id)).collect();

    assert_eq!(
        sorted_with_prefix("tab."),
        vec![
            "tab.activeBackground",
            "tab.activeBorder",
            "tab.activeBorderTop",
            "tab.activeForeground",
            "tab.activeModifiedBorder",
            "tab.border",
            "tab.divider",
            "tab.dragAndDropBorder",
            "tab.hoverBackground",
            "tab.hoverBorder",
            "tab.hoverForeground",
            "tab.inactiveBackground",
            "tab.inactiveForeground",
            "tab.inactiveModifiedBorder",
            "tab.lastPinnedBorder",
            "tab.selectedBackground",
            "tab.selectedBorderTop",
            "tab.selectedForeground",
            "tab.unfocusedActiveBackground",
            "tab.unfocusedActiveBorder",
            "tab.unfocusedActiveBorderTop",
            "tab.unfocusedActiveForeground",
            "tab.unfocusedActiveModifiedBorder",
            "tab.unfocusedHoverBackground",
            "tab.unfocusedHoverBorder",
            "tab.unfocusedHoverForeground",
            "tab.unfocusedInactiveBackground",
            "tab.unfocusedInactiveForeground",
            "tab.unfocusedInactiveModifiedBorder",
        ]
    );
    assert_eq!(
        sorted_with_prefix("modernTab."),
        vec!["modernTab.activeBackground", "modernTab.activeForeground", "modernTab.hoverBackground", "modernTab.hoverForeground"]
    );
    assert_eq!(
        sorted_with_prefix("modernEditorTab."),
        vec![
            "modernEditorTab.activeActionBackground",
            "modernEditorTab.activeBackground",
            "modernEditorTab.activeForeground",
            "modernEditorTab.activeHoverActionBackground",
            "modernEditorTab.activeHoverBackground",
            "modernEditorTab.hoverActionBackground",
            "modernEditorTab.hoverBackground",
            "modernEditorTab.hoverForeground",
            "modernEditorTab.inactiveBackground",
            "modernEditorTab.selectedActionBackground",
        ]
    );
    assert_eq!(
        editor_group_header_colors,
        vec!["editorGroupHeader.connectedTabsBackground", "editorGroupHeader.tabsBackground", "editorGroupHeader.tabsBorder"]
    );
}

/// As caixas `left` e `top` de um elemento posicionado: `position: fixed` ancora na janela, qualquer outro valor desloca
/// pela origem do bloco contêiner.
fn positioned_origin(position: &str, declared: (f64, f64), container_origin: (f64, f64)) -> (f64, f64) {
    if position == "fixed" { declared } else { (declared.0 + container_origin.0, declared.1 + container_origin.1) }
}

#[test]
fn keeps_floating_rail_overlays_anchored_to_the_viewport() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui floating-panels");
    style.set_activity_bar_metrics(ActivityBarMetrics { width: Some(36.0), action_height: Some(36.0) });
    let activity_bar = ClassList::new("part activitybar left");
    let menubar = style.compact_menubar_box(&activity_bar).expect("o menubar compacto da barra de atividade");
    let rail = style.activity_rail_geometry(&activity_bar).expect("a trilha flutuante");
    let rail_margins = style.floating_part_margins(&activity_bar).expect("a margem da trilha");
    let rail_right = rail_margins.left + rail.width;
    // `top: 120px; left: 240px; width: 200px; height: 160px` no menu do botão aberto.
    let (top, left) = positioned_origin(MENUBAR_MENU_HOLDER_POSITION, (120.0, 240.0), (rail_margins.left, 0.0));

    assert_eq!(
        (MENUBAR_MENU_HOLDER_POSITION, px(menubar.height), px(menubar.height), menubar.display, px(menubar.padding_top), top, left, 200.0, left > rail_right),
        ("fixed", "39px".to_owned(), "39px".to_owned(), "flex", "3px".to_owned(), 120.0, 240.0, 200.0, true)
    );
    assert_eq!(menubar.height, 39.0);
}

fn add_classes(tree: &mut je_ui::Tree, node: NodeId, class_name: &str) {
    for class in class_name.split_whitespace() {
        tree.add_class(node, class);
    }
}

fn key_up(ui: &Ui, target: NodeId, key_code: KeyCode) {
    ui.dispatch(&mut UiEvent::new(EventKind::KeyUp, target).with_key(KeyData { key_code, modifiers: Modifiers::default() }));
}

/// O primeiro descendente (a própria raiz fica de fora) com a classe, em ordem de documento.
fn first_with_class(ui: &Ui, root: NodeId, class_name: &str) -> Option<NodeId> {
    let tree = ui.tree();
    tree.subtree(root).into_iter().skip(1).find(|node| tree.has_class(*node, class_name))
}

/// A altura, o deslocamento do topo da primeira linha e o `margin-top` de cada linha de um `.monaco-menu`.
fn menu_geometry(ui: &Ui, style: &ModernUiStyle, menu: NodeId) -> Vec<(f64, f64, String)> {
    let container = first_with_class(ui, menu, "actions-container").expect("a lista de ações do menu");
    let tree = ui.tree();
    let rows = tree.children(container).to_vec();
    let first_top = tree.absolute_bounds(rows[0]).y0;
    rows.iter()
        .enumerate()
        .map(|(index, row)| (tree.bounds(*row).height(), tree.absolute_bounds(*row).y0 - first_top, px(style.activity_bar_action_margin_top(true, index > 0))))
        .collect()
}

fn keeps_compact_application_menu_spacing_independent_of_the_activity_rail(classes: &str, gap: f64, compact_activity_bar: bool, application_scrollbar_z_index: i32) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let mut style = ModernUiStyle::new(&theme, &format!("monaco-workbench {classes}"));
    style.set_activity_bar_metrics(ActivityBarMetrics { width: Some(36.0), action_height: Some(36.0) });
    style.set_activity_bar_action_gap(gap);

    let ui = Ui::new();
    let (menubar_node, composite_bar, reference_host) = {
        let root = ui.root();
        let mut tree = ui.tree_mut();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 1024.0, 768.0));
        let workbench = tree.create(root, Role::Generic);
        add_classes(&mut tree, workbench, &format!("monaco-workbench {classes}"));
        tree.set_bounds(workbench, Rect::new(0.0, 0.0, 1024.0, 400.0));
        let activity_bar = tree.create(workbench, Role::Generic);
        add_classes(&mut tree, activity_bar, &format!("part activitybar left{}", if compact_activity_bar { " compact" } else { "" }));
        tree.set_bounds(activity_bar, Rect::new(0.0, 0.0, 44.0, 400.0));
        let content = tree.create(activity_bar, Role::Generic);
        add_classes(&mut tree, content, "content");
        tree.set_bounds(content, Rect::new(0.0, 0.0, 44.0, 400.0));
        let menubar_node = tree.create(content, Role::Generic);
        add_classes(&mut tree, menubar_node, "menubar");
        tree.set_bounds(menubar_node, Rect::new(0.0, 0.0, 36.0, 39.0));
        let composite_bar = tree.create(content, Role::Generic);
        add_classes(&mut tree, composite_bar, "composite-bar");
        tree.set_bounds(composite_bar, Rect::new(0.0, 39.0, 36.0, 400.0));
        let reference_host = tree.create(workbench, Role::Generic);
        add_classes(&mut tree, reference_host, "reference-menu");
        tree.set_bounds(reference_host, Rect::new(600.0, 0.0, 600.0, 0.0));
        (menubar_node, composite_bar, reference_host)
    };

    let rail = guard.add_rc(ActionBar::new(&ui, composite_bar, IActionBarOptions { orientation: Some(ActionsOrientation::Vertical), ..Default::default() }));
    rail.push(
        &[guard.add_rc(Action::new("explorer", "Explorer", "", true, None)) as ActionRef, guard.add_rc(Action::new("search", "Search", "", true, None)) as ActionRef],
        IActionOptions::default(),
    );
    let actions: Vec<ActionRef> = vec![
        guard.add_rc(Action::new("undo", "Undo", "", true, None)) as ActionRef,
        guard.add_rc(Action::new("redo", "Redo", "", true, None)) as ActionRef,
        Separator::new() as ActionRef,
        guard.add_rc(Action::new("cut", "Cut", "", true, None)) as ActionRef,
        guard.add_rc(Action::new("copy", "Copy", "", true, None)) as ActionRef,
    ];
    ModifierKeyEmitter::get_instance(&ui).reset_key_status();
    let menu_bar = guard.add_rc(MenuBar::new(
        &ui,
        menubar_node,
        IMenuBarOptions {
            visibility: Some("compact".to_owned()),
            compact_mode: Some(IMenuDirection { horizontal: HorizontalDirection::Right, vertical: VerticalDirection::Below }),
            ..Default::default()
        },
        unthemed_menu_styles(),
    ));
    menu_bar.push(&[
        MenuBarMenu { label: "Edit".to_owned(), actions: actions.clone() },
        MenuBarMenu { label: "View".to_owned(), actions: Vec::new() },
        MenuBarMenu { label: "Help".to_owned(), actions: Vec::new() },
    ]);
    menu_bar.update(None);
    ui.run_animation_frame();
    menu_bar.toggle_focus();
    let menu_button = ui.tree().children(menubar_node)[0];
    key_up(&ui, menu_button, KeyCode::Enter);

    let holder = first_with_class(&ui, menubar_node, "menubar-menu-items-holder").expect("o menu do aplicativo está aberto");
    let application_menu = first_with_class(&ui, holder, "monaco-menu").expect("o menu do aplicativo");
    let first_item = first_with_class(&ui, application_menu, "action-menu-item").expect("o primeiro item do menu do aplicativo");
    key_up(&ui, first_item, KeyCode::RightArrow);
    let submenu_container = first_with_class(&ui, holder, "monaco-submenu").expect("o submenu abriu");
    let submenu = first_with_class(&ui, submenu_container, "monaco-menu").expect("o menu do submenu");

    let reference_menu = guard.add_rc(Menu::new(&ui, reference_host, &actions, IMenuOptions::default(), Rc::new(unthemed_menu_styles())));
    let reference_node = first_with_class(&ui, reference_host, "monaco-menu").expect("o menu de referência");
    let application_rows = menu_geometry(&ui, &style, application_menu);
    let rail_items: Vec<NodeId> = {
        let tree = ui.tree();
        tree.subtree(composite_bar).into_iter().filter(|node| tree.has_class(*node, "action-item")).collect()
    };
    let submenu_container_z_index = ui.tree().style(submenu_container).z_index;

    assert_eq!(
        (
            px(style.activity_bar_action_margin_top(false, rail_items.len() > 1)),
            application_rows.iter().map(|row| row.2.clone()).collect::<Vec<_>>(),
            application_rows[1].1,
            style.application_menu_scrollbar_z_index(),
            menu_geometry(&ui, &style, submenu),
            submenu_container_z_index,
            VISIBLE_SCROLLBAR_Z_INDEX,
        ),
        (
            px(gap),
            vec!["0px".to_owned(), "0px".to_owned(), "0px".to_owned()],
            24.0,
            application_scrollbar_z_index,
            menu_geometry(&ui, &style, reference_node),
            1,
            11,
        )
    );
    assert_eq!(rail_items.len(), 2);
    menu_bar.blur();
    drop(reference_menu);
    ModifierKeyEmitter::dispose_instance(&ui);
}

#[test]
fn keeps_compact_application_menu_spacing_independent_of_the_activity_rail_in_default_density() {
    keeps_compact_application_menu_spacing_independent_of_the_activity_rail("modern-ui floating-panels", 8.0, false, 0);
}

#[test]
fn keeps_compact_application_menu_spacing_independent_of_the_activity_rail_in_compact_density() {
    keeps_compact_application_menu_spacing_independent_of_the_activity_rail("modern-ui floating-panels modern-ui-compact", 4.0, false, 0);
}

#[test]
fn keeps_compact_application_menu_spacing_independent_of_the_activity_rail_in_compact_activity_bar() {
    keeps_compact_application_menu_spacing_independent_of_the_activity_rail("modern-ui floating-panels activitybar-compact", 0.0, true, 0);
}

#[test]
fn keeps_compact_application_menu_spacing_independent_of_the_activity_rail_in_classic_layout() {
    keeps_compact_application_menu_spacing_independent_of_the_activity_rail("", 0.0, false, 11);
}

#[test]
fn styles_focused_and_open_compact_application_menu_states() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let customized = |selection_border: &str| {
        theme_with_colors(&[
            ("menubar.selectionBorder", selection_border),
            ("modernActivityBarItem.hoverBackground", "#234567"),
            ("modernActivityBarItem.hoverForeground", "#345678"),
            ("modernActivityBarItem.activeBackground", "#456789"),
            ("modernActivityBarItem.activeForeground", "#56789a"),
        ])
    };
    // O executor sem cabeça do original não ativa `:focus`; `.open` exercita os seletores compartilhados de foco e de aberto.
    let target_styles = |theme: &ColorTheme, root_class_name: &str| {
        let style = ModernUiStyle::new(theme, root_class_name);
        let toggle = style.compact_menu_toggle_style(MenubarButtonState { open: true, ..MenubarButtonState::default() });
        let outline = toggle.outline.expect("o título aberto tem contorno");
        (
            css_rgb(&toggle.button_background.unwrap_or_else(Color::transparent_color)),
            css_rgb(&toggle.background.expect("o fundo do alvo")),
            css_rgb(&toggle.color.expect("a cor do alvo")),
            css_rgb(&outline.color.expect("a cor do contorno")),
            outline.style,
            px(outline.width),
            toggle.font_size.map(px),
        )
    };
    let focus_and_open = target_styles(&customized("#123456"), "monaco-workbench modern-ui floating-panels");
    let high_contrast_focus_and_open = target_styles(&customized("#abcdef"), "monaco-workbench modern-ui floating-panels hc-black");

    let expected = |outline_color: &str| {
        (
            "rgba(0, 0, 0, 0)".to_owned(),
            "rgb(69, 103, 137)".to_owned(),
            "rgb(86, 120, 154)".to_owned(),
            outline_color.to_owned(),
            "solid",
            "1px".to_owned(),
            Some("16px".to_owned()),
        )
    };
    assert_eq!((focus_and_open, high_contrast_focus_and_open), (expected("rgb(18, 52, 86)"), expected("rgb(171, 205, 239)")));
}

/// Os `classList` de um nó da árvore nativa, para o resolvedor.
fn node_classes(ui: &Ui, node: NodeId) -> ClassList {
    ClassList::new(&ui.tree().classes(node).join(" "))
}

/// Os seletores de descendente de `sashHandles.css`: o sash está dentro de um `.part` ou de um `.monaco-modal-editor-block`?
fn sash_placement(ui: &Ui, node: NodeId) -> SashPlacement {
    let tree = ui.tree();
    let mut placement = SashPlacement::default();
    let mut ancestor = tree.parent(node);
    while let Some(current) = ancestor {
        placement.inside_part |= tree.has_class(current, "part");
        placement.inside_modal_editor_block |= tree.has_class(current, "monaco-modal-editor-block");
        ancestor = tree.parent(current);
    }
    placement
}

/// O `background-color` computado de um nó pintado: `rgb(...)` opaco ou `rgba(...)`.
fn rgba_css(color: Option<Rgba>) -> Option<String> {
    color.map(|Rgba { r, g, b, a }| if a == 255 { format!("rgb({r}, {g}, {b})") } else { format!("rgba({r}, {g}, {b}, {})", f64::from(a) / 255.0) })
}

#[test]
fn keeps_collapsed_primary_side_bar_grips_only_while_the_activity_bar_can_anchor_them() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    // `getComputedStyle(sash, '::after').content` de cada sash, na ordem do objeto do original: o sash mínimo e o máximo da
    // barra lateral primária, o mínimo e o máximo da barra lateral secundária e o do painel.
    let grips = |root_class_name: &str| {
        let style = ModernUiStyle::new(&theme, root_class_name);
        [
            "monaco-sash vertical minimum primary-sidebar-sash",
            "monaco-sash vertical maximum primary-sidebar-sash",
            "monaco-sash vertical minimum",
            "monaco-sash vertical maximum",
            "monaco-sash horizontal maximum",
        ]
        .map(|sash| style.sash_grip_content(&ClassList::new(sash), SashPlacement::default()))
    };

    assert_eq!(
        (
            // A trilha fica sozinha como o próprio cartão, então o sash de revelar fica num vão de verdade.
            grips("monaco-workbench modern-ui nosidebar nopanel"),
            // Nada sobrou naquela borda para formar um vão.
            grips("monaco-workbench modern-ui nosidebar noactivitybar nopanel"),
        ),
        (["\"\"", "\"\"", "\"\"", "\"\"", "none"], ["none", "none", "\"\"", "\"\"", "none"])
    );
}

#[test]
fn modal_editor_sashes_hide_grips_without_changing_resize_affordances() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("sash.hoverBorder", "#123456")]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui");
    // `--vscode-sash-hoverBorder` é a cor do destaque do sash nativo.
    set_global_sash_hover_border(style.variable("--vscode-sash-hoverBorder").map(|color| to_rgba8(&color)));
    let ui = Ui::new();
    let (workbench, resizable) = {
        let root = ui.root();
        let mut tree = ui.tree_mut();
        let workbench = tree.create(root, Role::Generic);
        add_classes(&mut tree, workbench, "monaco-workbench modern-ui");
        let modal = tree.create(workbench, Role::Generic);
        add_classes(&mut tree, modal, "monaco-modal-editor-block");
        let resizable = tree.create(modal, Role::Generic);
        add_classes(&mut tree, resizable, "modal-editor-resizable");
        (workbench, resizable)
    };
    let workbench_sash = guard.add_rc(Sash::new_vertical(&ui, workbench, Rc::new(|_: &Sash| 0.0), SashOptions::default()));
    let vertical_sash = guard.add_rc(Sash::new_vertical(&ui, resizable, Rc::new(|_: &Sash| 0.0), SashOptions::default()));
    let horizontal_sash = guard.add_rc(Sash::new_horizontal(&ui, resizable, Rc::new(|_: &Sash| 0.0), SashOptions::default()));
    let sashes = [&vertical_sash, &horizontal_sash];
    let grip = |sash: &Rc<Sash>| style.sash_grip_content(&node_classes(&ui, sash.element()), sash_placement(&ui, sash.element()));
    let resting_grips = sashes.map(grip);

    guard.add(vertical_sash.add_class("hover"));
    guard.add(horizontal_sash.add_class("active"));
    sashes.iter().for_each(|sash| sash.refresh_style());

    assert_eq!(
        (
            grip(&workbench_sash),
            resting_grips,
            sashes.map(grip),
            sashes.map(|sash| ui.tree().style(sash.element()).pointer_events),
            sashes.map(|sash| ui.tree().style(sash.element()).cursor),
            // O `::before` do sash nativo é o primeiro filho.
            sashes.map(|sash| rgba_css({
                let tree = ui.tree();
                tree.style(tree.children(sash.element())[0]).background
            })),
        ),
        (
            "\"\"",
            ["none", "none"],
            ["none", "none"],
            [PointerEvents::Auto, PointerEvents::Auto],
            [Some(Cursor::EwResize), Some(Cursor::NsResize)],
            [Some("rgb(18, 52, 86)".to_owned()), Some("rgb(18, 52, 86)".to_owned())],
        )
    );
}

#[test]
fn compact_density_hides_sash_grips() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-compact");
    let grip = |sash: &str| style.sash_grip_content(&ClassList::new(sash), SashPlacement::default());

    assert_eq!((grip("monaco-sash vertical"), grip("monaco-sash horizontal")), ("none", "none"));
}

#[test]
fn sash_grips_use_the_modern_sash_grip_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("modernSash.gripForeground", "#123456")]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui");
    let grip = |sash: &str| style.sash_grip(&ClassList::new(sash), SashPlacement::default()).expect("a alça do sash");
    let (vertical, horizontal) = (grip("monaco-sash vertical"), grip("monaco-sash horizontal"));
    const FOREGROUND_AT_40_PERCENT: ColorValue = ColorValue::Transform(&ColorTransform::Transparent { value: Id("foreground"), factor: 0.4 });

    assert_eq!(
        (
            registered_defaults("modernSash.gripForeground"),
            css_rgb(&vertical.dot_color),
            // Os dois pontos de fora são desenhados com `box-shadow` e por isso leem `currentColor`.
            css_rgb(&horizontal.shadow_color),
            css_rgb(&horizontal.dot_color),
            vertical.opacity,
        ),
        (
            Some(Defaults::PerTheme(ColorDefaults {
                dark: Some(FOREGROUND_AT_40_PERCENT),
                light: Some(FOREGROUND_AT_40_PERCENT),
                hc_dark: Some(Id("foreground")),
                hc_light: Some(Id("foreground")),
            })),
            "rgb(18, 52, 86)".to_owned(),
            "rgb(18, 52, 86)".to_owned(),
            "rgb(18, 52, 86)".to_owned(),
            1.0,
        )
    );
}

#[test]
fn compact_vertical_sash_highlights_meet_both_panel_edges() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-compact floating-panels");
    let vertical_sash = ClassList::new("monaco-sash vertical");
    let insets = |style: &ModernUiStyle| style.vertical_sash_highlight_insets(&vertical_sash, SashPlacement::default()).expect("o destaque do sash da grade");
    let (attached_top, attached_bottom) = insets(&style);

    style.set_root_class_name("monaco-workbench modern-ui modern-ui-compact floating-panels top-window-edge");
    let (exposed_top, _) = insets(&style);

    assert_eq!(
        ([px(attached_top), px(attached_bottom)], px(exposed_top)),
        (["0px".to_owned(), "0px".to_owned()], "0px".to_owned())
    );
}

#[test]
fn default_density_top_panel_keeps_the_outer_bottom_gutter_when_maximized() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui floating-panels");
    let margin_bottom = |panel: &str| px(style.floating_part_margins(&ClassList::new(panel)).expect("a margem do painel").bottom);

    assert_eq!(
        (margin_bottom("part panel top"), margin_bottom("part panel top floating-part-outer-bottom")),
        ("0px".to_owned(), "4px".to_owned())
    );
}

#[test]
fn compact_auxiliary_window_editors_keep_their_outer_radius() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-compact");
    let editor = ClassList::new("part editor");

    assert_eq!(radii_css(style.part_corner_radii(&editor).expect("o raio do editor")), "8px");
}

#[test]
fn matches_exposed_macos_window_corners_without_changing_internal_card_corners() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui floating-panels mac macos-tahoe nostatusbar noactivitybar");
    let part = |classes: &str| radii_css_list(style.part_corner_radii(&ClassList::new(classes)).expect("o raio da parte"));
    let editor_content = |classes: &str| radii_css_list(style.editor_content_corner_radii(&ClassList::new(classes)).expect("o raio do conteúdo do editor"));
    let webview = |classes: &str| radii_css_list(style.webview_overlay_corner_radii(&ClassList::new(classes)).expect("o raio do webview"));
    let editor = "part editor floating-editor-outer-left floating-editor-outer-right floating-editor-outer-bottom";
    let corners = |radii: [&str; 4]| radii.map(str::to_owned);

    assert_eq!(
        vec![
            ("leftActivityBar", part("part activitybar left")),
            ("rightActivityBar", part("part activitybar right")),
            ("sidebar", part("part sidebar left floating-part-outer-left floating-part-outer-bottom")),
            ("auxiliaryBar", part("part auxiliarybar right floating-part-outer-right floating-part-outer-bottom")),
            ("panel", part("part panel bottom floating-part-outer-left floating-part-outer-right floating-part-outer-bottom")),
            ("editor", part(editor)),
            ("editorContent", editor_content(editor)),
            ("webview", webview("webview-overlay-content webview-overlay-outer-left webview-overlay-outer-right webview-overlay-outer-bottom")),
            ("modalWebview", webview("webview-overlay-content webview-overlay-modal")),
            ("internalPanel", part("part panel bottom floating-part-outer-bottom")),
            ("sidebarAbovePanel", part("part sidebar left floating-part-outer-left")),
        ],
        vec![
            ("leftActivityBar", corners(["8px", "0px", "0px", "12px"])),
            ("rightActivityBar", corners(["0px", "8px", "12px", "0px"])),
            ("sidebar", corners(["8px", "8px", "8px", "12px"])),
            ("auxiliaryBar", corners(["8px", "8px", "12px", "8px"])),
            ("panel", corners(["8px", "8px", "12px", "12px"])),
            ("editor", corners(["8px", "8px", "12px", "12px"])),
            ("editorContent", corners(["8px", "8px", "11px", "11px"])),
            ("webview", corners(["8px", "8px", "11px", "11px"])),
            ("modalWebview", corners(["8px", "8px", "8px", "8px"])),
            ("internalPanel", corners(["8px", "8px", "8px", "8px"])),
            ("sidebarAbovePanel", corners(["8px", "8px", "8px", "8px"])),
        ]
    );
}

#[test]
fn keeps_macos_corner_radii_in_sync_with_zoom_window_borders_and_chrome() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench");
    let panel = ClassList::new("part panel bottom floating-part-outer-right floating-part-outer-bottom");
    let mut radius = |classes: &str, zoom_factor: f64| {
        style.set_root_class_name(&format!("monaco-workbench modern-ui floating-panels {classes}"));
        style.set_window_zoom_factor(zoom_factor);
        // `border-bottom-right-radius`.
        px(style.part_corner_radii(&panel).expect("o raio do painel").2)
    };

    let actual = vec![
        ("tahoe", radius("mac macos-tahoe nostatusbar", 1.0)),
        ("olderMacOS", radius("mac nostatusbar", 1.0)),
        ("windowBorder", radius("mac macos-tahoe nostatusbar border hc-black", 1.0)),
        ("zoomedIn", radius("mac macos-tahoe nostatusbar", 2.0)),
        ("zoomedOut", radius("mac macos-tahoe nostatusbar", 0.5)),
        ("clamped", radius("mac macos-tahoe nostatusbar", 4.0)),
        ("statusBar", radius("mac macos-tahoe", 1.0)),
        ("fullscreen", radius("mac macos-tahoe nostatusbar fullscreen", 1.0)),
        ("compact", radius("mac macos-tahoe nostatusbar modern-ui-compact", 1.0)),
        ("web", radius("mac macos-tahoe nostatusbar web", 1.0)),
        ("windows", radius("windows nostatusbar", 1.0)),
        ("linux", radius("linux nostatusbar", 1.0)),
    ];

    assert_eq!(
        actual,
        [
            ("tahoe", "12px"),
            ("olderMacOS", "6px"),
            ("windowBorder", "11px"),
            ("zoomedIn", "4px"),
            ("zoomedOut", "28px"),
            ("clamped", "0px"),
            ("statusBar", "8px"),
            ("fullscreen", "8px"),
            ("compact", "0px"),
            ("web", "8px"),
            ("windows", "8px"),
            ("linux", "8px"),
        ]
        .map(|(name, value)| (name, value.to_owned()))
    );
}

