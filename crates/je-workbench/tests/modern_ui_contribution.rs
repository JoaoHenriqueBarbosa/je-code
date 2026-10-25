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
//! Portados com as margens do editor, as camadas de borda dos cartões compactos (`compact_card_border_layers`) e o estilo
//! das abas conectadas (`tab_variable`, `connected_tab_fill_background`, `connected_edge_border_top_color`,
//! `tab_border_container`, com o participante `collect_modern_tab_color_customizations`): de `compact density keeps the
//! panel cluster flush and square` até `uses explicitly customized selected borders only when the active top border is
//! absent`. O `generateColorThemeCSS` desses casos é o estilo sem `with_explicit_colors_only`, que já aplica os padrões do
//! registro e os participantes; `createUnloadedTheme` é `ColorTheme::unloaded` e `setCustomColors` é `set_custom_colors`.
//! Portados com a fileira, a máscara e o apoio do botão de nova aba (`connected_tab_strip_background`, `add_tab_background`),
//! as máscaras das ações (`tab_actions_background`) e a moldura do grupo em alto contraste (`connected_group_frame`): de
//! `preserves hover action customization ...` até `retains the empty root connected editor frame in high contrast`.
//! Portados com a geometria nativa das abas (`ModernUiStyle::tab_box_context`, que monta o `TabBoxContext` de
//! `multi_editor_tabs_control.rs` com as classes da raiz e da aba e os tamanhos do tema; `tab_fill_rect` mede o
//! `getBoundingClientRect()` do `.tab-fill`), o `pointer-events` e o `z-index` das abas (`tab_actions_pointer_events`,
//! `tab_z_index`) e a borda do editor (`editor_border_width`, `editor_border_color`): de `preserves upper-row pill gutters
//! ...` até `reserves the connected terminal shoulder ...`. Em `preserves a single HC frame ...` a linha de base da
//! geometria do cartão é por densidade, porque o DOM do original deixa os `--vscode-spacing-*` indefinidos (margens nulas) e
//! a margem nativa usa o espaçamento do tema.
//! Portados com o contorno do `.tab-fill` (`tab_fill_outline_style`), a largura das bordas dos indicadores
//! (`tab_border_container_border_widths`), as bordas e o conteúdo dos ombros (`connected_edge_border_color`,
//! `fill_shoulder_content`, `fill_shoulder_box`, `tab_fill_border_color`) e as medidas da fileira
//! (`tabs_and_actions_padding_top`, `tabs_container_padding_bottom`): de `HC active multi-selection keeps one cap outline ...`
//! até `keeps the right shoulder curved and the left edge straight at row boundaries`.

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
    ActionTabState, ActivityBarMetrics, ActivityItemState, BoxOffsets, ClassList, CornerRadii, EditorTabState, FillShoulder, GradientKind, ListRowHost, MENUBAR_MENU_HOLDER_POSITION, MenubarButtonState, ModernUiStyle,
    NotificationInsets, NotificationsPlacement, PaneHost, PaneState, PartMargins, SashPlacement, TAB_CONNECTED_EDGE_PSEUDO_CONTENT, TAB_FILL_BACKGROUND_CLIP, TabBorderIndicator, TabElement, TabRowPlacement,
    EDITOR_TABS_SCROLLBAR_Z_INDEX, VISIBLE_SCROLLBAR_Z_INDEX, css_rgb, editor_group_child_display, tab_fill_bottom_left_radius, tab_fill_top_right_radius,
};
use je_workbench::browser::parts::editor::multi_editor_tabs_control::{BoxRect, StrokeSide, TabBoxContext, TabCssClasses, TabSizingParams, tab_box_style, tab_fill_rect};
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
    theme_with_colors_of_type(ThemeType::Dark, colors)
}

/// `theme_with_colors` para qualquer um dos quatro tipos de tema.
fn theme_with_colors_of_type(theme_type: ThemeType, colors: &[(&str, &str)]) -> ColorTheme {
    let mut theme = unloaded_theme(theme_type);
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

/// Os quatro `margin-*` computados, na ordem `top`, `right`, `bottom`, `left`.
fn margins_css_list(margins: PartMargins) -> [String; 4] {
    [margins.top, margins.right, margins.bottom, margins.left].map(px)
}

/// O `margin` computado: o shorthand na forma mais curta (um, dois, três ou quatro valores).
fn margin_css(margins: PartMargins) -> String {
    let [top, right, bottom, left] = margins_css_list(margins);
    if top == right && right == bottom && bottom == left {
        top
    } else if top == bottom && right == left {
        format!("{top} {right}")
    } else if right == left {
        format!("{top} {right} {bottom}")
    } else {
        format!("{top} {right} {bottom} {left}")
    }
}

#[test]
fn compact_density_keeps_the_panel_cluster_flush_and_square() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // `--vscode-surface-border` e `--vscode-editor-border` são customizações do tema; `--vscode-cornerRadius-large: 8px`,
    // `--vscode-spacing-size40: 4px`, `--vscode-spacing-sizeNone: 0px` e `--vscode-strokeThickness: 1px` são os valores do tema.
    let theme = theme_with_colors(&[("surface.border", "#123456"), ("editor.border", "#654321")]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-compact floating-panels");
    let activity_bar = ClassList::new("part activitybar left");
    let side_bar = ClassList::new("part sidebar floating-part-outer-left floating-part-outer-top floating-part-outer-bottom");
    let panel = ClassList::new("part panel");
    let auxiliary_bar = ClassList::new("part auxiliarybar floating-part-outer-right floating-part-outer-top floating-part-outer-bottom");
    let editor = ClassList::new("part editor floating-editor-outer-left floating-editor-outer-right floating-editor-outer-top floating-editor-outer-bottom");
    let webview_overlay_content =
        ClassList::new("webview-overlay-content webview-overlay-outer-left webview-overlay-outer-right webview-overlay-outer-top webview-overlay-outer-bottom");
    let modal_webview_overlay_content = ClassList::new("webview-overlay-content webview-overlay-modal");
    let corners = |part: &ClassList| radii_css_list(style.part_corner_radii(part).expect("o raio da parte"));
    let margin = |part: &ClassList| margins_css_list(style.floating_part_margins(part).expect("a margem da parte"));
    let floating_border_color = |part: &ClassList| style.floating_part_border_colors(part).expect("a borda da parte").to_css();
    // `backgroundImage.split('radial-gradient').length - 1` e `backgroundOrigin.split(', ').every(origin => origin === 'border-box')`.
    let corner_border_count = |part: &ClassList| style.compact_card_border_layers(part).iter().filter(|layer| layer.kind == GradientKind::Radial).count();
    let all_border_backgrounds_use_border_box = |part: &ClassList| {
        let layers = style.compact_card_border_layers(part);
        !layers.is_empty() && layers.iter().all(|layer| layer.origin_border_box)
    };
    let square = || ["0px"; 4].map(str::to_owned);
    let transparent = "rgba(0, 0, 0, 0)".to_owned();

    assert_eq!(
        (
            (corners(&activity_bar), corner_border_count(&activity_bar)),
            (
                margin(&side_bar),
                corners(&side_bar),
                floating_border_color(&side_bar),
                all_border_backgrounds_use_border_box(&side_bar),
                corner_border_count(&side_bar),
            ),
            (corners(&panel), floating_border_color(&panel), all_border_backgrounds_use_border_box(&panel), corner_border_count(&panel)),
            (corners(&auxiliary_bar), corner_border_count(&auxiliary_bar)),
            (
                margin(&editor),
                corners(&editor),
                style.editor_border_color(&editor).map(|color| css_rgb(&color)),
                all_border_backgrounds_use_border_box(&editor),
                corner_border_count(&editor),
            ),
            radii_css(style.editor_content_corner_radii(&editor).expect("o raio do conteúdo do editor")),
            radii_css_list(style.webview_overlay_corner_radii(&webview_overlay_content).expect("o raio do webview")),
            radii_css(style.webview_overlay_corner_radii(&modal_webview_overlay_content).expect("o raio do webview modal")),
        ),
        (
            (square(), 0),
            (square(), square(), transparent.clone(), true, 0),
            (square(), transparent.clone(), true, 0),
            (square(), 0),
            (square(), square(), Some(transparent), true, 0),
            "0px".to_owned(),
            square(),
            "8px".to_owned(),
        )
    );
}

#[test]
fn compact_perimeter_stays_flush_with_hidden_window_chrome_and_restores_the_default_density() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui floating-panels");
    let parts: Vec<ClassList> = [
        "activitybar left",
        "activitybar right",
        "sidebar floating-part-outer-left floating-part-outer-top floating-part-outer-bottom",
        "auxiliarybar floating-part-outer-right floating-part-outer-top floating-part-outer-bottom",
        "panel top floating-part-outer-left floating-part-outer-right floating-part-outer-top floating-part-outer-bottom",
        "panel bottom floating-part-outer-left floating-part-outer-right floating-part-outer-top floating-part-outer-bottom",
        "editor floating-editor-outer-left floating-editor-outer-right floating-editor-outer-top floating-editor-outer-bottom",
    ]
    .iter()
    .map(|class_name| ClassList::new(&format!("part {class_name}")))
    .collect();
    // `{ margin, radius }` de cada parte.
    let read_geometry = |style: &ModernUiStyle| -> Vec<(String, String)> {
        parts
            .iter()
            .map(|part| (margin_css(style.floating_part_margins(part).expect("a margem da parte")), radii_css(style.part_corner_radii(part).expect("o raio da parte"))))
            .collect()
    };
    let default_geometry = read_geometry(&style);

    for chrome_classes in ["", "top-window-edge", "nostatusbar", "top-window-edge nostatusbar"] {
        style.set_root_class_name(&format!("monaco-workbench modern-ui floating-panels modern-ui-compact {chrome_classes}"));
        let flush: Vec<(String, String)> = parts.iter().map(|_| ("0px".to_owned(), "0px".to_owned())).collect();
        assert_eq!(read_geometry(&style), flush, "{chrome_classes}");
    }

    style.set_root_class_name("monaco-workbench modern-ui floating-panels");
    assert_eq!(read_geometry(&style), default_geometry);
}

/// `ColorThemeData.createUnloadedTheme('vs-dark', colors)`: um tema escuro só com o `colorMap` dado (cores do tema, não
/// customizações do usuário).
fn unloaded_theme_with_file_colors(colors: &[(&str, &str)]) -> ColorTheme {
    unloaded_theme_of_type_with_file_colors(ThemeType::Dark, colors)
}

/// `ColorThemeData.createUnloadedTheme(themeId, colors)` para qualquer um dos quatro tipos de tema.
fn unloaded_theme_of_type_with_file_colors(theme_type: ThemeType, colors: &[(&str, &str)]) -> ColorTheme {
    ColorTheme::unloaded("", theme_type, colors.iter().map(|(id, hex)| ((*id).to_owned(), Color::from_hex(hex))).collect())
}

/// O tipo do tema que `createUnloadedTheme` deriva do id (`vs-dark`, `vs`, `hc-black` e `hc-light`).
fn theme_type_of(theme_id: &str) -> ThemeType {
    match theme_id {
        "vs-dark" => ThemeType::Dark,
        "vs" => ThemeType::Light,
        "hc-black" => ThemeType::HighContrastDark,
        "hc-light" => ThemeType::HighContrastLight,
        other => panic!("tema desconhecido: {other}"),
    }
}

/// O `background-color` computado de um elemento: o elemento sem fundo declarado é transparente.
fn computed_background(color: Option<Color>) -> String {
    css_rgb(&color.unwrap_or_else(Color::transparent_color))
}

/// Os textos de uma lista de cores, no formato do `getComputedStyle`.
fn some_strings<const N: usize>(texts: [&str; N]) -> [Option<String>; N] {
    texts.map(|text| Some(text.to_owned()))
}

#[test]
fn uses_the_editor_background_for_the_connected_surface() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let backgrounds = [("#1f1f1f", "#ffffff17"), ("#ffffff", "#2468ac"), ("#1f1f1f", "#00000000")].map(|(editor, tab)| {
        let theme = unloaded_theme_with_file_colors(&[("editor.background", editor), ("modernEditorTab.activeBackground", tab)]);
        // `--modern-ui-connected-tab-surface` do `registerThemingParticipant` de `connectedEditorTabs.ts`.
        let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui-tabs modern-ui-connected-editor-tabs");
        style.connected_tab_surface().map(|color| color.to_string())
    });

    assert_eq!(backgrounds, some_strings(["#1f1f1f", "#ffffff", "#1f1f1f"]));
}

#[test]
fn uses_legacy_background_customizations_for_connected_tabs() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme = unloaded_theme_with_file_colors(&[("editor.background", "#000000"), ("editorGroupHeader.connectedTabsBackground", "#111111")]);
    theme.set_custom_colors([
        ("tab.activeBackground", "#123456"),
        ("tab.unfocusedActiveBackground", "#234567"),
        ("tab.inactiveBackground", "#345678"),
        ("tab.unfocusedInactiveBackground", "#456789"),
        ("tab.hoverBackground", "#56789A"),
        ("tab.unfocusedHoverBackground", "#6789AB"),
    ]);
    // O CSS gerado pelo tema (`generateColorThemeCSS` com os participantes) é o estilo sem `with_explicit_colors_only`.
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs");
    let colors = |group_active: bool| {
        let tab = |class_name: &str| TabElement::new(class_name, group_active);
        // O `.title.tabs` do grupo não é uma aba: as sondas `hover-probe` e `active-hover-probe` herdam dele.
        let title = tab("");
        // As sondas `upper-*` estão dentro da aba `connected-tab-upper-row`.
        let upper_tab = tab("tab connected-tab-upper-row");
        let hover_name = if group_active { "--modern-ui-editor-tab-hover-background" } else { "--modern-ui-editor-tab-unfocused-hover-background" };
        [
            style.connected_tab_fill_background(&tab("tab active")),
            style.connected_tab_fill_background(&tab("tab")),
            style.tab_variable("--modern-ui-editor-tab-hover-background", &upper_tab),
            style.tab_variable("--modern-ui-editor-tab-unfocused-hover-background", &upper_tab),
            style.tab_variable("--modern-ui-editor-tab-action-unfocused-hover-background", &upper_tab),
            style.tab_variable(hover_name, &title),
            style.tab_variable("--modern-ui-connected-tab-active-hover-background", &title),
        ]
        .map(rgb_of)
    };

    assert_eq!(
        (colors(true), colors(false)),
        (
            some_strings(["rgb(18, 52, 86)", "rgb(52, 86, 120)", "rgb(86, 120, 154)", "rgb(103, 137, 171)", "rgb(103, 137, 171)", "rgb(86, 120, 154)", "rgb(86, 120, 154)"]),
            some_strings(["rgb(35, 69, 103)", "rgb(69, 103, 137)", "rgb(86, 120, 154)", "rgb(103, 137, 171)", "rgb(103, 137, 171)", "rgb(103, 137, 171)", "rgb(103, 137, 171)"]),
        )
    );
}

#[test]
fn uses_legacy_border_customizations_for_connected_tabs() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // `--vscode-tab-selectedBorderTop` é a customização `tab.selectedBorderTop`; o DOM só tem as variáveis que o caso escreve,
    // sem o CSS gerado pelo tema.
    let theme = theme_with_colors(&[("tab.selectedBorderTop", "#56789a")]);
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs").with_explicit_colors_only();
    style.set_root_variable("--modern-ui-editor-tab-custom-active-border-top", Color::from_hex("#123456"));
    style.set_root_variable("--modern-ui-editor-tab-custom-unfocused-active-border-top", Color::from_hex("#234567"));
    let border_styles = |group_active: bool| {
        // `--tab-border-top-color` e `--tab-border-bottom-color` que o caso escreve em cada aba.
        let (active_top, active_bottom) = if group_active { ("#123456", "#345678") } else { ("#234567", "#456789") };
        let active_tab = TabElement::new("tab active connected-tab-top-row tab-border-top tab-border-bottom", group_active)
            .with_border_colors(Some(Color::from_hex(active_top)), Some(Color::from_hex(active_bottom)));
        let selected_tab = TabElement::new("tab selected tab-border-top", group_active).with_border_colors(Some(Color::from_hex("#56789a")), None);
        let active_top_border = style.tab_border_container(&active_tab, TabBorderIndicator::Top);
        let active_bottom_border = style.tab_border_container(&active_tab, TabBorderIndicator::Bottom);
        let selected_top_border = style.tab_border_container(&selected_tab, TabBorderIndicator::Top);
        (
            (active_top_border.display, rgb_of(style.connected_tab_fill_border_top_color(&active_tab)), rgb_of(style.connected_edge_border_top_color(&active_tab))),
            (active_bottom_border.display, css_rgb(&active_bottom_border.background)),
            (selected_top_border.display, rgb_of(style.connected_tab_fill_border_top_color(&selected_tab))),
        )
    };

    let expected = |edge: &str, bottom: &str| {
        (
            ("none", Some("rgba(0, 0, 0, 0)".to_owned()), Some(edge.to_owned())),
            ("block", bottom.to_owned()),
            ("none", Some("rgb(86, 120, 154)".to_owned())),
        )
    };
    assert_eq!(
        (border_styles(true), border_styles(false)),
        (expected("rgb(18, 52, 86)", "rgb(52, 86, 120)"), expected("rgb(35, 69, 103)", "rgb(69, 103, 137)"))
    );
}

#[test]
fn uses_explicitly_customized_selected_borders_only_when_the_active_top_border_is_absent() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    type Colors<'a> = &'a [(&'a str, &'a str)];
    let cases: [(Colors, Colors, [&str; 2]); 5] = [
        (&[], &[], ["rgb(37, 37, 38)", "rgb(37, 37, 38)"]),
        (&[], &[("tab.selectedBorderTop", "#a3e635")], ["rgb(163, 230, 53)", "rgb(163, 230, 53)"]),
        (&[("tab.activeBorderTop", "#3994bc")], &[("tab.selectedBorderTop", "#a3e635")], ["rgb(37, 37, 38)", "rgb(37, 37, 38)"]),
        (
            &[],
            &[("tab.selectedBorderTop", "#a3e635"), ("tab.activeBorderTop", "#22d3ee"), ("tab.unfocusedActiveBorderTop", "#c084fc")],
            ["rgb(34, 211, 238)", "rgb(192, 132, 252)"],
        ),
        (&[], &[("tab.selectedBorderTop", "#a3e635"), ("tab.activeBorderTop", "#00000000")], ["rgba(0, 0, 0, 0)", "rgba(0, 0, 0, 0)"]),
    ];

    let actual: Vec<[String; 2]> = cases
        .iter()
        .map(|(colors, customizations, _)| {
            let mut theme = unloaded_theme_with_file_colors(colors);
            theme.set_custom_colors(customizations.iter().copied());
            // O CSS gerado pelo tema (`generateColorThemeCSS`) é o estilo sem `with_explicit_colors_only`.
            let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui-tabs modern-ui-connected-editor-tabs");
            // O grupo ativo e o sem foco, cada um com a aba `active selected tab-border-top` e o seu `tab-connected-edge`.
            [true, false].map(|group_active| {
                let tab = TabElement::new("tab active selected tab-border-top", group_active);
                css_rgb(&style.connected_edge_border_top_color(&tab).expect("a aresta conectada da aba ativa"))
            })
        })
        .collect();
    let expected: Vec<[String; 2]> = cases.iter().map(|(_, _, expected)| (*expected).map(str::to_owned)).collect();

    assert_eq!(actual, expected);
}

#[test]
fn preserves_hover_action_customization_on_connected_wrapped_and_separate_pinned_rows() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = [false, true].map(|customize_action| {
        let mut theme = unloaded_theme(ThemeType::Dark);
        let mut customizations = vec![("modernEditorTab.hoverBackground", "#7c2d12")];
        if customize_action {
            customizations.push(("modernEditorTab.hoverActionBackground", "#5a1f0c"));
        }
        theme.set_custom_colors(customizations);
        let style = ModernUiStyle::new(&theme, "upper-action-customization monaco-workbench modern-ui-tabs modern-ui-connected-editor-tabs");
        // Os grupos ativo e sem foco, cada um com a aba da fileira única (`single`), a da fileira com quebra (`wrapped`) e a da
        // fileira das fixadas (`pinned`). As duas últimas são `connected-tab-upper-row`; `wrapping` e `two-tab-bars` são do
        // contêiner e não mudam a variável da aba.
        [true, false]
            .into_iter()
            .flat_map(|group_active| {
                ["single", "wrapped", "pinned"].map(|layout| {
                    let tab = TabElement::new(if layout == "single" { "tab" } else { "tab connected-tab-upper-row" }, group_active);
                    let probe = if group_active { "--modern-ui-editor-tab-action-hover-background" } else { "--modern-ui-editor-tab-action-unfocused-hover-background" };
                    rgb_of(style.tab_variable(probe, &tab))
                })
            })
            .collect::<Vec<_>>()
    });

    assert_eq!(actual, [vec![Some("rgb(124, 45, 18)".to_owned()); 6], vec![Some("rgb(90, 31, 12)".to_owned()); 6]]);
}

#[test]
fn uses_separate_connected_strip_and_border_colors_without_changing_the_editor_group_header() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let results = ["#2468ac", "#ff000080", "#00000000"].map(|color| {
        let mut theme = unloaded_theme_of_type_with_file_colors(
            ThemeType::Light,
            &[
                ("editor.background", "#ffffff"),
                ("editorGroup.border", "#123456"),
                ("editorGroupHeader.tabsBorder", "#445566"),
                ("editorGroupHeader.tabsBackground", "#eeeeee"),
                ("editorGroupHeader.connectedTabsBackground", "#ddeeff"),
                ("modernEditorTab.activeBackground", "#abcdef"),
            ],
        );
        theme.set_custom_colors([("modernEditorTab.activeBackground", color), ("tab.activeBackground", "#654321")]);
        let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs");
        let tab = TabElement::new("tab active", true);
        let separator = rgb_of(style.connected_tab_separator_color(&tab));
        (
            // O `.tab-fill`, o `.tab-connected-edge`, o `::before` e o `::after` do fill, o `::after` da fileira e o `.tab-actions`.
            [
                rgb_of(style.connected_tab_fill_background(&tab)),
                rgb_of(style.connected_edge_border_top_color(&tab)),
                separator.clone(),
                separator.clone(),
                separator,
                Some(css_rgb(&style.tab_actions_background(&tab, true))),
            ],
            rgb_of(style.variable("--vscode-editorGroupHeader-tabsBackground")),
            rgb_of(style.connected_tab_strip_background()),
            theme.resolve_color("editorGroup.border", true).map(|border| border.to_string()),
        )
    });

    let surface = |fill: &str| -> [Option<String>; 6] {
        let border = "rgb(68, 85, 102)";
        [fill, border, border, border, border, "rgba(0, 0, 0, 0)"].map(|color| Some(color.to_owned()))
    };
    let expected_for = |fill: &str| (surface(fill), Some("rgb(238, 238, 238)".to_owned()), Some("rgb(221, 238, 255)".to_owned()), Some("#123456".to_owned()));
    assert_eq!(results, [expected_for("rgb(36, 104, 172)"), expected_for("rgba(255, 0, 0, 0.5)"), expected_for("rgba(0, 0, 0, 0)")]);
}

#[test]
fn keeps_the_shared_connected_strip_on_the_standard_header_background_outside_modern_ui() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme_of_type_with_file_colors(
        ThemeType::Light,
        &[
            ("editor.background", "#ffffff"),
            ("editorGroupHeader.tabsBackground", "#f8f8f8"),
            ("editorGroupHeader.connectedTabsBackground", "#e5e5e5"),
            ("modernEditorTab.hoverBackground", "#00000080"),
        ],
    );
    let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui-tabs modern-ui-connected-editor-tabs");
    // As sondas estão na raiz, que não é uma aba nem um grupo.
    let root = TabElement::new("", true);
    let colors = |style: &ModernUiStyle| {
        (
            rgb_of(style.tab_variable("--modern-ui-connected-tab-strip-background", &root)),
            rgb_of(style.tab_variable("--modern-ui-connected-tab-upper-hover-background", &root)),
        )
    };

    let shared = colors(&style);
    style.set_root_class_name("monaco-workbench modern-ui-tabs modern-ui-connected-editor-tabs modern-ui");
    let editor = colors(&style);

    assert_eq!(
        (shared, editor),
        (
            (Some("rgb(248, 248, 248)".to_owned()), Some("rgb(123, 123, 123)".to_owned())),
            (Some("rgb(229, 229, 229)".to_owned()), Some("rgb(114, 114, 114)".to_owned())),
        )
    );
}

#[test]
fn keeps_selected_pill_surfaces_visible_beneath_transparent_tab_actions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for (theme_id, strip, hover, expected) in [
        ("vs-dark", "#202020", "#ffffff80", "rgb(143, 143, 143)"),
        ("vs", "#eeeeee", "#00000080", "rgb(118, 118, 118)"),
        ("vs", "#eeeeee", "#2468ac", "rgb(36, 104, 172)"),
        ("vs-dark", "#202020", "#ffffff00", "rgb(32, 32, 32)"),
        ("hc-black", "#000000", "#ffffff", "rgb(255, 255, 255)"),
        ("hc-light", "#ffffff", "#000000", "rgb(0, 0, 0)"),
    ] {
        let theme = unloaded_theme_of_type_with_file_colors(
            theme_type_of(theme_id),
            &[("editor.background", "#112233"), ("editorGroupHeader.tabsBackground", strip), ("modernEditorTab.hoverBackground", hover)],
        );
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs {theme_id}"));
        let high_contrast = theme_id.starts_with("hc-");
        // `title.compact-height` só muda a altura, não as cores.
        for compact in [false, true] {
            for focused_group in [false, true] {
                for actions_left in [false, true] {
                    for active in [false, true] {
                        let class_name = format!("tab connected-tab-upper-row{}{}", if actions_left { " tab-actions-left" } else { "" }, if active { " active" } else { "" });
                        let tab = TabElement::new(&class_name, focused_group);
                        let strip_color = computed_background(style.connected_tab_strip_background());
                        // A ação está com o foco: `action.focus()`.
                        assert_eq!(
                            (
                                computed_background(style.connected_tab_fill_background(&tab)),
                                rgb_of(style.tab_variable("--modern-ui-editor-tab-hover-background", &tab)),
                                css_rgb(&style.tab_actions_background(&tab, true)),
                            ),
                            (
                                if active {
                                    "rgb(17, 34, 51)".to_owned()
                                } else if high_contrast {
                                    "rgba(0, 0, 0, 0)".to_owned()
                                } else {
                                    strip_color
                                },
                                Some(expected.to_owned()),
                                "rgba(0, 0, 0, 0)".to_owned(),
                            ),
                            "{theme_id}, {hover}, compact: {compact}, focused group: {focused_group}, actions left: {actions_left}, active: {active}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn composites_translucent_connected_strip_colors_only_once() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: [(Option<&str>, &str); 4] =
        [(Some("#ffffff80"), "rgb(143, 143, 143)"), (Some("#ffffff00"), "rgb(32, 32, 32)"), (Some("#abcdef"), "rgb(171, 205, 239)"), (None, "rgb(32, 32, 32)")];
    for (strip, expected) in cases {
        let theme_ids: &[&str] = if strip.is_some() { &["vs-dark", "vs", "hc-black", "hc-light"] } else { &["hc-black", "hc-light"] };
        for theme_id in theme_ids {
            let mut colors = vec![("editor.background", "#202020")];
            colors.extend(strip.map(|strip| ("editorGroupHeader.tabsBackground", strip)));
            let theme = unloaded_theme_of_type_with_file_colors(theme_type_of(theme_id), &colors);
            let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs {theme_id}"));
            let high_contrast = theme_id.starts_with("hc-");
            // A aba inativa do grupo ativo, com o foco na ação (`action.focus()`).
            let tab = TabElement::new("tab", true);
            // `row.classList.toggle('wrapping', wrapped)`: a fileira com quebra pinta a mesma superfície.
            for wrapped in [false, true] {
                // A fileira, a máscara das abas fixas e o apoio do botão de nova aba leem a mesma variável da fileira.
                let strip_background = computed_background(style.connected_tab_strip_background());
                assert_eq!(
                    (
                        [strip_background.clone(), strip_background.clone(), css_rgb(&style.add_tab_background())],
                        strip_background,
                        computed_background(style.connected_tab_fill_background(&tab)),
                        high_contrast.then(|| css_rgb(&style.tab_actions_background(&tab, true))),
                    ),
                    (
                        [expected.to_owned(), expected.to_owned(), "rgba(0, 0, 0, 0)".to_owned()],
                        expected.to_owned(),
                        if high_contrast { "rgba(0, 0, 0, 0)".to_owned() } else { expected.to_owned() },
                        high_contrast.then(|| "rgba(0, 0, 0, 0)".to_owned()),
                    ),
                    "{theme_id}, {strip:?}, wrapped: {wrapped}"
                );
            }
        }
    }
}

#[test]
fn retains_the_empty_root_connected_editor_frame_in_high_contrast() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = ClassList::new("part editor editor-tabs-multiple");
    for theme_id in ["vs-dark", "vs", "hc-black", "hc-light"] {
        // `--vscode-focusBorder`, `--vscode-contrastBorder` e `--vscode-modernPanel-border` são customizações do tema.
        let mut theme = unloaded_theme(theme_type_of(theme_id));
        theme.set_custom_colors([("focusBorder", "#ffaa00"), ("contrastBorder", "#00aaff"), ("modernPanel.border", "#00ffff")]);
        let mut style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs");
        for floating in [false, true] {
            style.set_root_class_name(&format!(
                "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs {theme_id}{}",
                if floating { " floating-panels" } else { "" }
            ));
            for empty in [false, true, false] {
                let group = ClassList::new(if empty { "editor-group-container empty active" } else { "editor-group-container active" });
                let frame = style.connected_group_frame(&part, false);
                let has_frame = theme_id.starts_with("hc-");
                let actual = (
                    frame.as_ref().map_or("none", |_| "\"\""),
                    has_frame
                        .then(|| {
                            frame.as_ref().map(|frame| {
                                (px(frame.border_width), frame.color.as_ref().map(css_rgb), px(frame.radius), if frame.pointer_events_none { "none" } else { "auto" })
                            })
                        })
                        .flatten(),
                    empty.then(|| [editor_group_child_display(&group), editor_group_child_display(&group)]),
                );
                let expected = (
                    if has_frame { "\"\"" } else { "none" },
                    has_frame.then(|| ("1px".to_owned(), Some("rgb(0, 255, 255)".to_owned()), "8px".to_owned(), "none")),
                    empty.then_some(["none", "none"]),
                );
                assert_eq!(actual, expected, "{theme_id}, floating: {floating}, empty: {empty}");
            }
        }
    }
}

/// `[left, top, right, bottom]` de uma caixa da geometria nativa (`getBoundingClientRect()` do original).
fn box_array(rect: BoxRect) -> [f64; 4] {
    [rect.left, rect.top, rect.right, rect.bottom]
}

#[test]
fn preserves_upper_row_pill_gutters_for_wrapped_and_pinned_rows() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // `--vscode-spacing-size20`, `--vscode-cornerRadius-small` e `--vscode-strokeThickness` são os valores do tema.
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs");
    for pinned_row in [false, true] {
        // `title.two-tab-bars` é a fileira das fixadas, a barra superior; sem ela a fileira é a da quebra (`wrapping`).
        let placement = TabRowPlacement { wrapping: !pinned_row, upper_bar: pinned_row, two_tab_bars: pinned_row, first_tab_bar: pinned_row, tab_width: 120.0, ..TabRowPlacement::default() };
        for row_end in ["", "last-in-row", "connected-tab-right-edge"] {
            let tab = TabElement::new(&format!("tab active connected-tab-upper-row {row_end}"), true);
            // O `left` do `.tab-fill` e o `right`, a distância da borda direita da aba até a do preenchimento.
            let fill = tab_fill_rect(&style.tab_box_context(&tab, placement));
            assert_eq!(
                [px(fill.left), px(placement.tab_width - fill.right)],
                ["2px".to_owned(), "2px".to_owned()],
                "pinned row: {pinned_row}, row end: {row_end}"
            );
        }
    }
}

#[test]
fn hidden_connected_close_slots_do_not_intercept_pointer_input() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs");
    for state in ["", "active", "dirty", "sticky", "focus", ""] {
        // `action.blur()` e depois, no estado `focus`, `action.focus()`: o foco está na ação, então a aba tem `:focus-within`.
        let class_name = if state == "focus" { "" } else { state };
        let tab = TabElement { focus_within: state == "focus", ..TabElement::new(&format!("tab sizing-fit {class_name}"), true) };
        // O `.action-label` herda o `pointer-events` do `.tab-actions`, e nenhuma regra o troca.
        let pointer_events = style.tab_actions_pointer_events(&tab);
        assert_eq!([pointer_events, pointer_events], [if state.is_empty() { "none" } else { "auto" }; 2], "{}", if state.is_empty() { "hidden" } else { state });
    }
}

#[test]
fn raises_active_connected_sticky_shoulders_above_peers_but_below_scrollbars() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs");
    for sizing in ["fit", "shrink", "fixed"] {
        for sticky in ["compact", "shrink"] {
            for disabled in [false, true] {
                // `tabs.classList.toggle('disable-sticky-tabs', disabled)`: a classe é do `.tabs-container` das duas abas.
                let tab = |active: bool| TabElement {
                    disable_sticky_tabs: disabled,
                    ..TabElement::new(&format!("tab {}sticky sizing-{sizing} sticky-{sticky}", if active { "active " } else { "" }), true)
                };
                // `auto` é `None`; a barra de rolagem da fileira tem `z-index: 11` em qualquer estado.
                let actual = [style.tab_z_index(&tab(true)), style.tab_z_index(&tab(false)), Some(EDITOR_TABS_SCROLLBAR_Z_INDEX)];
                assert_eq!(actual, if disabled { [None, None, Some(11)] } else { [Some(9), Some(8), Some(11)] }, "{sizing}, {sticky}, disabled: {disabled}");
            }
        }
    }
}

#[test]
fn preserves_a_single_hc_frame_in_every_tab_mode_without_changing_the_outer_card_geometry() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // As variáveis `--vscode-*` da raiz são as cores do tema; sem o CSS gerado pelo tema, só elas existem no DOM.
    let themes: Vec<(&str, ColorTheme)> = ["vs-dark", "vs", "hc-black", "hc-light"]
        .into_iter()
        .map(|theme_id| {
            let mut theme = unloaded_theme(theme_type_of(theme_id));
            theme.set_custom_colors([("editor.border", "#123456"), ("editorGroup.border", "#123456"), ("modernPanel.border", "#00ffff"), ("focusBorder", "#ffaa00")]);
            (theme_id, theme)
        })
        .collect();
    let root_classes = |floating: bool, theme_id: &str, connected: bool, compact: bool| {
        let mut classes = String::from("monaco-workbench modern-ui modern-ui-tabs");
        for (enabled, class_name) in [(floating, "floating-panels"), (connected, "modern-ui-connected-editor-tabs"), (compact, "modern-ui-compact")] {
            if enabled {
                classes.push(' ');
                classes.push_str(class_name);
            }
        }
        format!("{classes} {theme_id}")
    };
    let editor_part = |multiple: bool| ClassList::new(&format!("part editor floating-editor-outer-left floating-editor-outer-top{}", if multiple { " editor-tabs-multiple" } else { "" }));
    // A geometria do cartão externo: as margens da parte (o lugar que ela ocupa na grade) e a borda que fica dentro dela
    // (onde começa o conteúdo). O `getBoundingClientRect()` do original lê o mesmo par.
    let geometry = |style: &ModernUiStyle, part: &ClassList| (style.floating_part_margins(part), px(style.editor_border_width(part)));
    // O original mede a linha de base uma vez por `floating`, com as variáveis de espaçamento da raiz indefinidas (margens
    // nulas em qualquer densidade). As margens nativas usam o espaçamento do tema e a densidade compacta as zera, então a
    // linha de base é por densidade: sem tema, sem abas conectadas e sem as abas em modo múltiplo.
    let baseline = |floating: bool, compact: bool| {
        let style = ModernUiStyle::new(&themes[0].1, &root_classes(floating, "", false, compact)).with_explicit_colors_only();
        geometry(&style, &editor_part(false))
    };

    for floating in [false, true] {
        for (theme_id, theme) in &themes {
            let high_contrast = theme_id.starts_with("hc-");
            for connected in [false, true] {
                for compact in [false, true] {
                    let style = ModernUiStyle::new(theme, &root_classes(floating, theme_id, connected, compact)).with_explicit_colors_only();
                    for show_tabs in ["multiple", "single", "none", "multiple"] {
                        let part = editor_part(show_tabs == "multiple");
                        let connected_frame = connected && show_tabs == "multiple";
                        let shared_card = connected_frame && high_contrast;
                        let border_width = style.editor_border_width(&part);
                        // `::after` do `.editor-group-container`: as bordas vêm da regra da moldura (linhas 39 a 48), e o
                        // `content: none` da linha 56 só a apaga no editor da grade do Modern UI.
                        let frame_rule = style.connected_group_frame(&part, false);
                        let group_frame = [
                            style.connected_group_frame(&part, true).map_or("none", |_| "\"\"").to_owned(),
                            px(frame_rule.as_ref().map_or(0.0, |frame| frame.border_width)),
                            px(frame_rule.as_ref().map_or(0.0, |frame| frame.border_width)),
                            px(frame_rule.as_ref().map_or(0.0, |frame| frame.radius)),
                        ];
                        let border = [px(border_width), px(border_width), css_rgb(&style.editor_border_color(&part).expect("a borda do editor"))];
                        let compact_stroke = (floating && compact).then(|| style.floating_card_stroke_color(&part).expect("o traço do cartão compacto").to_string());

                        let expected_border_color = if floating && compact {
                            "rgba(0, 0, 0, 0)"
                        } else if shared_card {
                            "rgb(0, 255, 255)"
                        } else {
                            "rgb(18, 52, 86)"
                        };
                        let expected_frame = if shared_card { ["none", "1px", "1px", "8px"] } else { ["none", "0px", "0px", "0px"] };
                        assert_eq!(
                            (geometry(&style, &part), border, compact_stroke, group_frame),
                            (
                                baseline(floating, compact),
                                [px(1.0), px(1.0), expected_border_color.to_owned()],
                                (floating && compact).then(|| if shared_card { "#00ffff" } else { "#123456" }.to_owned()),
                                expected_frame.map(str::to_owned),
                            ),
                            "{theme_id}, floating: {floating}, connected: {connected}, compact: {compact}, showTabs: {show_tabs}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn connects_hc_group_borders_without_moving_content_when_themes_or_active_groups_change() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = ClassList::new("part editor editor-tabs-multiple");
    let placement = TabRowPlacement { tab_width: 120.0, ..TabRowPlacement::default() };
    // A aba ativa (`connected-tab-top-row`) é a segunda da fileira, com 120px; a primeira abre a fileira.
    let first_placement = TabRowPlacement { starts_row: true, ..placement };
    let mut first_geometry = None;
    for theme_id in ["vs-dark", "vs", "hc-black", "hc-light"] {
        let theme = unloaded_theme_of_type_with_file_colors(
            theme_type_of(theme_id),
            &[
                ("editor.background", "#333333"),
                ("editorGroupHeader.tabsBorder", "#252526"),
                ("focusBorder", "#ffaa00"),
                ("contrastBorder", "#888888"),
                ("modernPanel.border", "#00ffff"),
            ],
        );
        // O CSS gerado pelo tema (`generateColorThemeCSS`) é o estilo sem `with_explicit_colors_only`.
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs {theme_id}"));
        // A geometria do cabeçalho conectado: a altura da fileira e as caixas do `.tab-fill` das duas abas (a altura e a
        // largura da aba, as linhas do breadcrumbs, do cabeçalho e do corpo não dependem do tema nem do grupo).
        let geometry = |group_active: bool| {
            let first = style.tab_box_context(&TabElement::new("tab first-in-row", group_active), first_placement);
            let active = style.tab_box_context(&TabElement::new("tab active connected-tab-top-row", group_active), placement);
            (first.connected_row_height(), box_array(tab_fill_rect(&first)), box_array(tab_fill_rect(&active)), (active.tab_width, active.tab_height))
        };
        let baseline = *first_geometry.get_or_insert_with(|| geometry(true));
        for group_active in [true, false] {
            let high_contrast = theme_id.starts_with("hc-");
            let border = if high_contrast {
                if group_active { "rgb(255, 170, 0)" } else { "rgb(136, 136, 136)" }
            } else {
                "rgb(37, 37, 38)"
            };
            let first_tab = TabElement::new("tab first-in-row", group_active);
            let tab = TabElement::new("tab active connected-tab-top-row", group_active);
            // `border` do `.tab-fill` pinta os quatro lados da mesma cor, então a tampa (topo e esquerda) lê uma só.
            let cap = rgb_of(style.connected_tab_fill_border_top_color(&tab));
            // `::before` (`border-right`, `border-bottom`) e `::after` (`border-left`, `border-bottom`) do `.tab-fill` e o
            // `::after` da fileira leem a mesma `--modern-ui-connected-tab-border`.
            let separator = rgb_of(style.connected_tab_separator_color(&tab));
            let frame = style.connected_group_frame(&part, false);
            let frame_width = px(frame.as_ref().map_or(0.0, |frame| frame.border_width));
            let actual = (
                geometry(group_active),
                style.tab_fill_border_widths(&first_tab).map(|widths| px(widths.left)),
                [cap.clone(), cap],
                [[separator.clone(), separator.clone()], [separator.clone(), separator.clone()]],
                separator,
                (
                    frame.as_ref().map_or("none", |_| "\"\"").to_owned(),
                    [frame_width.clone(), frame_width.clone(), frame_width],
                    high_contrast.then(|| frame.as_ref().and_then(|frame| frame.color.as_ref().map(css_rgb))).flatten(),
                    px(frame.as_ref().map_or(0.0, |frame| frame.radius)),
                    if frame.as_ref().is_some_and(|frame| frame.pointer_events_none) { "none" } else { "auto" },
                ),
                rgb_of(style.connected_tab_surface()),
            );
            let expected_separator = Some(border.to_owned());
            let expected = (
                baseline,
                Some("1px".to_owned()),
                [Some("rgba(0, 0, 0, 0)".to_owned()), Some("rgba(0, 0, 0, 0)".to_owned())],
                [[expected_separator.clone(), expected_separator.clone()], [expected_separator.clone(), expected_separator.clone()]],
                expected_separator,
                if high_contrast {
                    ("\"\"".to_owned(), ["1px".to_owned(), "1px".to_owned(), "1px".to_owned()], Some("rgb(0, 255, 255)".to_owned()), "8px".to_owned(), "none")
                } else {
                    ("none".to_owned(), ["0px".to_owned(), "0px".to_owned(), "0px".to_owned()], None, "0px".to_owned(), "auto")
                },
                Some("rgb(51, 51, 51)".to_owned()),
            );
            assert_eq!(actual, expected, "{theme_id}, active group: {group_active}");
        }
    }
}

#[test]
fn uses_the_panel_border_for_the_connected_hc_frame_in_a_multi_tab_modal_editor() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // A parte modal, filha do `.monaco-modal-editor-block`, fora da grade.
    let part = ClassList::new("part editor editor-tabs-multiple modal-editor-part");
    for theme_id in ["hc-black", "hc-light"] {
        // As variáveis `--vscode-*` da raiz são as cores do tema; sem o CSS gerado pelo tema, só elas existem no DOM.
        let mut theme = unloaded_theme(theme_type_of(theme_id));
        theme.set_custom_colors([("focusBorder", "#ffaa00"), ("contrastBorder", "#888888"), ("modernPanel.border", "#00ffff"), ("editorWidget.border", "#888888")]);
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs {theme_id}")).with_explicit_colors_only();
        for group_active in [true, false] {
            // Uma aba comum e a aba ativa (`connected-tab-top-row`), os dois filhos do `.tabs-container`.
            let tabs = [TabElement::new("tab", group_active), TabElement::new("tab active connected-tab-top-row", group_active)];
            let frame = style.connected_group_frame(&part, false);
            assert_eq!(
                (
                    tabs.len(),
                    rgb_of(style.connected_tab_fill_border_top_color(&tabs[1])),
                    frame.as_ref().map(|frame| (px(frame.border_width), frame.color.as_ref().map(css_rgb), px(frame.radius))),
                    frame.as_ref().map_or("none", |_| "\"\""),
                    px(style.editor_border_width(&part)),
                ),
                (2, Some("rgba(0, 0, 0, 0)".to_owned()), Some(("1px".to_owned(), Some("rgb(0, 255, 255)".to_owned()), "8px".to_owned())), "\"\"", "1px".to_owned()),
                "{theme_id}, active group: {group_active}"
            );
        }
    }
}

#[test]
fn reserves_the_connected_terminal_shoulder_without_moving_tab_content() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // O `left` do rótulo: o recuo da aba, que não depende de a aba estar ativa.
    let label_left = |context: &TabBoxContext| {
        tab_box_style(
            &TabCssClasses { last_tab: true, last_child: true, wrapping: context.wrapping, ..TabCssClasses::default() },
            &TabSizingParams { modern_ui_tabs: true, connected_editor_tabs: context.connected, ..TabSizingParams::default() },
        )
        .padding_left
    };
    for classes in ["", "modern-ui", "modern-ui modern-ui-connected-editor-tabs", "modern-ui-connected-editor-tabs"] {
        let connected = classes.contains("modern-ui-connected-editor-tabs");
        let shares_editor_frame = connected && classes.split(' ').any(|class_name| class_name == "modern-ui");
        for theme_id in ["vs-dark", "vs", "hc-black", "hc-light"] {
            let theme = unloaded_theme(theme_type_of(theme_id));
            let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui-tabs {classes} {theme_id}"));
            for active_group in [true, false] {
                for compact in [true, false] {
                    // A aba de 120px abre a fileira (`first-in-row`) e é a última (`connected-tab-last`); `compact-height` é do `.title`.
                    let placement = TabRowPlacement { compact_height: compact, starts_row: true, tab_width: 120.0, ..TabRowPlacement::default() };
                    let inactive = style.tab_box_context(&TabElement::new("tab first-in-row connected-tab-last", active_group), placement);
                    let active = style.tab_box_context(&TabElement::new("tab first-in-row connected-tab-last active", active_group), placement);
                    let (inactive_fill, active_fill) = (tab_fill_rect(&inactive), tab_fill_rect(&active));
                    // `--modern-ui-connected-tab-action-inset` e `--modern-ui-tab-action-padding` só existem nas abas conectadas;
                    // sem elas a sonda `var(..., 0px)` mede `0`.
                    let action_classes = TabCssClasses { last_tab: true, last_child: true, ..TabCssClasses::default() };
                    let (action_inset, action_padding) = if active.connected { (active.connected_action_inset(), active.action_padding(&action_classes)) } else { (0.0, 0.0) };

                    // A largura da pílula de 120px e a altura da fileira; o rótulo não anda quando a aba fica ativa.
                    let (expected_inset, expected_padding) = match (connected, compact, shares_editor_frame) {
                        (false, ..) => (0.0, 0.0),
                        (true, true, false) => (4.0, 28.0),
                        (true, true, true) => (4.5, 28.5),
                        (true, false, false) => (6.0, 30.0),
                        (true, false, true) => (6.5, 30.5),
                    };
                    assert_eq!(
                        (
                            (active.tab_width, active.tab_height),
                            label_left(&active),
                            [inactive_fill.top - active_fill.top, inactive_fill.left - active_fill.left, active_fill.right - inactive_fill.right],
                            (action_inset, action_padding),
                            // O ombro terminal de `7px` (`cornerRadius-medium` e o traço) mais o traço de `1px` da aba conectada.
                            px(active.connected_terminal_shoulder_margin(true)),
                            px(active.fill_top_left_radius()),
                        ),
                        (
                            (inactive.tab_width, inactive.tab_height),
                            label_left(&inactive),
                            if connected { [if shares_editor_frame { 4.5 } else { 3.5 }, 2.0, 2.0] } else { [0.0, 0.0, 0.0] },
                            (expected_inset, expected_padding),
                            if connected { "8px".to_owned() } else { "0px".to_owned() },
                            if connected { "0px".to_owned() } else { "4px".to_owned() },
                        ),
                        "{classes:?}, {theme_id}, active group: {active_group}, compact: {compact}"
                    );
                }
            }
        }
    }
}

#[test]
fn hc_active_multi_selection_keeps_one_cap_outline_without_an_inner_frame() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // A aba única do `.tabs-container` (a primeira, então abre a fileira) de 120px.
    let placement = TabRowPlacement { tab_width: 120.0, starts_row: true, ..TabRowPlacement::default() };
    for theme_id in ["hc-black", "hc-light"] {
        let mut theme = unloaded_theme(theme_type_of(theme_id));
        theme.set_custom_colors([("contrastActiveBorder", "#ff0000"), ("focusBorder", "#00ff00")]);
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs {theme_id}")).with_explicit_colors_only();
        let ordinary = TabElement::new("tab active", true);
        let selected = TabElement::new("tab active selected multi-selected", true);
        let bounds = box_array(tab_fill_rect(&style.tab_box_context(&ordinary, placement)));
        let ordinary_outline = style.tab_fill_outline_style(&ordinary);
        let indicator = style.tab_border_container_border_widths();
        assert_eq!(
            (
                ordinary_outline,
                style.tab_fill_outline_style(&selected),
                rgb_of(style.connected_edge_border_color(&selected, StrokeSide::Right, placement.starts_row)),
                [indicator.top, indicator.right, indicator.bottom, indicator.left].map(px),
                box_array(tab_fill_rect(&style.tab_box_context(&selected, placement))),
            ),
            ("none", "none", Some("rgb(255, 0, 0)".to_owned()), ["0px", "0px", "0px", "0px"].map(str::to_owned), bounds),
            "{theme_id}"
        );
    }
}

#[test]
fn matches_connected_cap_and_shoulder_radii_for_the_outside_stroke_and_omits_the_first_outer_shoulder() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = theme_with_colors(&[("focusBorder", "#ffaa00")]);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs").with_explicit_colors_only();
    let shoulders = |tab: &TabElement, starts_row: bool| [FillShoulder::Left, FillShoulder::Right].map(|shoulder| style.fill_shoulder_content(tab, starts_row, shoulder));

    // A primeira aba (`:first-child`, ativa) não tem o ombro externo da esquerda.
    let first_shoulders = shoulders(&TabElement::new("tab active first-in-row", true), true);
    // A aba do meio (a segunda, ativa quando a primeira deixa de ser).
    let middle = TabElement::new("tab active", true);
    let middle_context = style.tab_box_context(&middle, TabRowPlacement::default());
    let [left_shoulder, right_shoulder] = [FillShoulder::Left, FillShoulder::Right].map(|shoulder| style.fill_shoulder_box(&middle, false, shoulder).expect("o ombro da aba ativa"));

    assert_eq!(
        (
            first_shoulders,
            (
                shoulders(&middle, false),
                [px(middle_context.fill_top_left_radius()), px(tab_fill_top_right_radius(&middle_context))],
                TAB_FILL_BACKGROUND_CLIP,
                [px(left_shoulder.bottom_radius), px(right_shoulder.bottom_radius)],
                [[px(left_shoulder.width), px(left_shoulder.height)], [px(right_shoulder.width), px(right_shoulder.height)]],
            ),
        ),
        (
            ["none", "\"\""],
            (
                ["\"\"", "\"\""],
                ["7px".to_owned(), "7px".to_owned()],
                "border-box",
                ["7px".to_owned(), "7px".to_owned()],
                [["7px".to_owned(), "7px".to_owned()], ["7px".to_owned(), "7px".to_owned()]],
            ),
        )
    );
}

#[test]
fn aligns_single_row_connected_caps_while_retaining_wrapped_gutters() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs").with_explicit_colors_only();
    let single = TabRowPlacement::default();
    let wrapped = TabRowPlacement { wrapping: true, ..TabRowPlacement::default() };
    let row_start = |placement: TabRowPlacement| TabRowPlacement { starts_row: true, ..placement };

    // A fileira de uma linha: a aba inativa abre a fileira e a ativa é a segunda.
    let inactive_single = (TabElement::new("tab", true), row_start(single));
    let active_single = (TabElement::new("tab active", true), single);
    // A fileira quebrada: a pílula de cima abre a fileira, e as de baixo vêm depois dela.
    let upper = (TabElement::new("tab active connected-tab-upper-row connected-tab-top-row first-in-row", true), row_start(wrapped));
    let inactive_bottom = (TabElement::new("tab", true), wrapped);
    let active_bottom = (TabElement::new("tab active", true), wrapped);

    // A moldura de cima fora da quebra: o `left` do preenchimento e os raios da esquerda.
    let upper_unwrapped = style.tab_box_context(&upper.0, row_start(single));
    let non_wrapped_top_frame = (
        px(tab_fill_rect(&upper_unwrapped).left),
        px(upper_unwrapped.fill_top_left_radius()),
        px(tab_fill_bottom_left_radius(&upper_unwrapped)),
    );
    // `border-block` da aba, os recuos `top` e `bottom` do preenchimento e o `left` dele.
    let geometry = |(tab, placement): &(TabElement, TabRowPlacement)| {
        let context = style.tab_box_context(tab, *placement);
        let fill = tab_fill_rect(&context);
        let (border_top, border_bottom) = context.pill_border_block();
        ([px(border_top), px(border_bottom)], [px(fill.top), px(context.tab_height - fill.bottom)], px(fill.left))
    };
    let upper_context = style.tab_box_context(&upper.0, upper.1);
    let active_bottom_context = style.tab_box_context(&active_bottom.0, active_bottom.1);
    let top_widths = style.tab_fill_border_widths(&upper.0).expect("as larguras da borda do preenchimento");
    let transparent = Some("rgba(0, 0, 0, 0)".to_owned());

    assert_eq!(
        (
            non_wrapped_top_frame,
            px(style.tabs_and_actions_padding_top(wrapped)),
            // A reserva da ação (`padding-right`) é a mesma na pílula de cima e na aba de baixo.
            upper_context.action_padding(&TabCssClasses::default()) == active_bottom_context.action_padding(&TabCssClasses::default()),
            [px(style.tabs_container_padding_bottom(single)), px(style.tabs_container_padding_bottom(wrapped))],
            [px(style.tabs_row_separator_bottom()), px(style.tabs_row_separator_bottom())],
            (
                [style.tab_fill_border_color(&upper.0, StrokeSide::Top, upper.1), style.tab_fill_border_color(&upper.0, StrokeSide::Left, upper.1)],
                px(upper_context.fill_top_left_radius()),
                [px(top_widths.top), px(top_widths.left)],
            ),
            geometry(&upper),
            (geometry(&inactive_single), geometry(&inactive_bottom)),
            (geometry(&active_single), geometry(&active_bottom)),
        ),
        (
            ("2px".to_owned(), "4px".to_owned(), "4px".to_owned()),
            "2px".to_owned(),
            true,
            ["1px".to_owned(), "1px".to_owned()],
            ["0px".to_owned(), "0px".to_owned()],
            ([transparent.clone(), transparent], "4px".to_owned(), ["1px".to_owned(), "1px".to_owned()]),
            (["0px".to_owned(), "0px".to_owned()], ["2px".to_owned(), "2px".to_owned()], "2px".to_owned()),
            (
                (["0px".to_owned(), "0px".to_owned()], ["3.5px".to_owned(), "3.5px".to_owned()], "2px".to_owned()),
                (["0px".to_owned(), "0px".to_owned()], ["2px".to_owned(), "2px".to_owned()], "2px".to_owned()),
            ),
            (
                (["0px".to_owned(), "0px".to_owned()], ["-1px".to_owned(), "-1px".to_owned()], "0px".to_owned()),
                (["0px".to_owned(), "0px".to_owned()], ["2px".to_owned(), "-1px".to_owned()], "0px".to_owned()),
            ),
        )
    );
}

#[test]
fn shares_the_high_contrast_outer_frame_while_retaining_upper_row_outlines() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut outlines = Vec::new();
    for theme_id in ["hc-black", "hc-light"] {
        let theme = theme_with_colors_of_type(theme_type_of(theme_id), &[("focusBorder", "#ffaa00")]);
        let style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs {theme_id}")).with_explicit_colors_only();
        for upper_row in [false, true] {
            // `row.classList.toggle('wrapping', upperRow)` e `tab.classList.toggle('connected-tab-upper-row', upperRow)`.
            let tab = TabElement::new(if upper_row { "tab active connected-tab-top-row first-in-row connected-tab-upper-row" } else { "tab active connected-tab-top-row first-in-row" }, true);
            let placement = TabRowPlacement { wrapping: upper_row, starts_row: true, ..TabRowPlacement::default() };
            outlines.push((
                theme_id,
                upper_row,
                style.tab_fill_border_color(&tab, StrokeSide::Top, placement),
                style.tab_fill_border_color(&tab, StrokeSide::Left, placement),
            ));
        }
    }
    let expected = |theme_id: &'static str, upper_row: bool| {
        let color = Some(if upper_row { "rgb(255, 170, 0)" } else { "rgba(0, 0, 0, 0)" }.to_owned());
        (theme_id, upper_row, color.clone(), color)
    };
    assert_eq!(outlines, vec![expected("hc-black", false), expected("hc-black", true), expected("hc-light", false), expected("hc-light", true)]);
}

#[test]
fn upper_connected_pills_reserve_top_accents_for_customizations_and_retain_hc_outlines() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut measurements = Vec::new();
    let mut expected = Vec::new();
    for theme_id in ["vs", "vs-dark", "hc-black", "hc-light"] {
        let theme = theme_with_colors_of_type(theme_type_of(theme_id), &[("focusBorder", "#ffaa00"), ("contrastBorder", "#00ffff")]);
        let mut style = ModernUiStyle::new(&theme, &format!("monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs {theme_id}")).with_explicit_colors_only();
        style.set_root_variable("--modern-ui-editor-tab-custom-border", Color::from_hex("#445566"));
        // A pílula de cima de 160px, a primeira da fileira, em qualquer grupo (`group.classList.toggle('active', active)`).
        let placement = TabRowPlacement { tab_width: 160.0, starts_row: true, ..TabRowPlacement::default() };
        for active in [false, true] {
            let tab = TabElement::new("tab active connected-tab-upper-row tab-border-top", active);
            for customized in [false, true] {
                let before = box_array(tab_fill_rect(&style.tab_box_context(&tab, placement)));
                if customized {
                    style.set_root_variable("--modern-ui-editor-tab-custom-active-border-top", Color::from_hex("#112233"));
                    style.set_root_variable("--modern-ui-editor-tab-custom-unfocused-active-border-top", Color::from_hex("#445566"));
                } else {
                    style.remove_root_variable("--modern-ui-editor-tab-custom-active-border-top");
                    style.remove_root_variable("--modern-ui-editor-tab-custom-unfocused-active-border-top");
                }
                let borders = [StrokeSide::Top, StrokeSide::Right, StrokeSide::Bottom, StrokeSide::Left].map(|side| style.tab_fill_border_color(&tab, side, placement));
                let geometry_stable = before == box_array(tab_fill_rect(&style.tab_box_context(&tab, placement)));
                measurements.push((theme_id, active, customized, borders, geometry_stable));
                let transparent = "rgba(0, 0, 0, 0)".to_owned();
                let border = if theme_id.starts_with("hc-") { (if active { "rgb(255, 170, 0)" } else { "rgb(0, 255, 255)" }).to_owned() } else { transparent.clone() };
                let borders = if theme_id.starts_with("hc-") {
                    [border.clone(), border.clone(), border.clone(), border]
                } else {
                    let top = if customized { (if active { "rgb(17, 34, 51)" } else { "rgb(68, 85, 102)" }).to_owned() } else { transparent.clone() };
                    [top, transparent.clone(), transparent.clone(), transparent]
                };
                expected.push((theme_id, active, customized, borders.map(Some), true));
            }
        }
    }
    assert_eq!(measurements, expected);
}

#[test]
fn keeps_the_right_shoulder_curved_and_the_left_edge_straight_at_row_boundaries() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = unloaded_theme(ThemeType::Dark);
    let style = ModernUiStyle::new(&theme, "monaco-workbench modern-ui modern-ui-tabs modern-ui-connected-editor-tabs").with_explicit_colors_only();
    // Os ombros do `.tab-fill` e as máscaras do `.tab-connected-edge` (`::before` e `::after`).
    let edges = |tab: &TabElement, starts_row: bool| {
        (
            [FillShoulder::Left, FillShoulder::Right].map(|shoulder| style.fill_shoulder_content(tab, starts_row, shoulder)),
            [TAB_CONNECTED_EDGE_PSEUDO_CONTENT; 2],
        )
    };

    // O fim da fileira é a segunda aba (depois de uma aba comum); ela está ativa primeiro.
    let row_end = edges(&TabElement::new("tab active last-in-row", true), false);
    // O começo da fileira vem logo depois do `last-in-row` (`.tab.last-in-row + .tab.active`) e fica ativo depois.
    let row_start = edges(&TabElement::new("tab active first-in-row", true), true);

    assert_eq!(
        (row_end, row_start),
        ((["\"\"", "\"\""], ["none", "none"]), (["none", "\"\""], ["none", "none"]))
    );
}

