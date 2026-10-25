//! Porte de `src/vs/workbench/contrib/modernUI/test/browser/modernUI.contribution.test.ts` (suíte
//! `ModernUIContribution`), caso a caso e na mesma ordem, nos casos que decidem o estado da contribuição (o submenu e
//! as ações de densidade, as classes dos contêineres, o relayout, o tamanho do cabeçalho dos panes e as abas conectadas).
//!
//! Notas de tradução:
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
//! Recortes: os demais casos da suíte leem o estilo calculado de um DOM com os `*.css` do VS Code (`getComputedStyle`,
//! `getBoundingClientRect`, `generateColorThemeCSS`); não há DOM, e o `.css` é a especificação lida ao portar cada
//! widget. Voltam com o desenho nativo dos widgets e com o participante de tema (`generateColorThemeCSS`): `insets multi
//! diff file rows ...`, `supports isolated notification and dialog presentation`, `uses part-specific pane colors ...`
//! até `rounds activity bar corners ...` e todos os que seguem (barra de atividade, barra de status, menu compacto,
//! sashes, cantos do macOS, abas conectadas por tema) exceto os portados abaixo, e `uses the editor background for the
//! connected surface` até `applies editor tab foregrounds ...`, `uses readable theme foregrounds ...` até `fades tab
//! actions only when ...`, e de `uses legacy color customizations ...` até `keeps panel global actions above ...`.
//! Nos casos parciais ficou só a parte que não é CSS: no terceiro caso `paneHeaderLineHeight` e
//! `paneHeaderInlineLineHeight`; em `toggles uppercase view headers without relayout` o `text-transform` dos títulos;
//! em `Explorer title casing ...` o `text-transform` (fica a decisão `shouldPreserveWorkspaceNameCase`); nos casos de
//! borda e de fundo do shell o `borderColor` e o `backgroundColor` calculados (ficam os padrões registrados); em `uses
//! the registered modern tab colors` os valores calculados (ficam o registro e os padrões).

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
use je_theme::registry::{ColorTransform, ColorValue, Defaults, color_registry};
use je_workbench::contrib::files::browser::views::explorer_view::should_preserve_workspace_name_case;
use je_ui::{NodeId, Ui};
use je_widgets::paneview::{Pane, PaneContent, PaneOptions};
use je_widgets::splitview::SplitViewView;
use je_workbench::browser::actions::layout_density_actions::{LayoutDensityMenu, register_layout_density_actions, set_layout_density};
use je_workbench::contrib::modern_ui::modern_ui_contribution::{ModernUiContribution, ModernUiHost, register_layout_density_menu};
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

#[test]
fn uses_the_editor_surface_border_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // O `borderColor` calculado do `part editor` vem do `.css` (especificação lida ao portar o desenho da parte): fica o
    // padrão registrado.
    assert_eq!(registered_defaults("editor.border"), SURFACE_BORDER);
}

#[test]
fn uses_the_modern_activity_bar_border_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Os `borderColor` calculados da barra de atividade e da barra lateral (e o vão cedido à barra) vêm do `.css`.
    assert_eq!(registered_defaults("modernActivityBar.border"), SURFACE_BORDER);
}

#[test]
fn uses_the_modern_panel_border_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // O `borderColor` do painel e a `--modern-ui-floating-card-stroke-color` compacta vêm do `.css`.
    assert_eq!(registered_defaults("modernPanel.border"), SURFACE_BORDER);
}

#[test]
fn uses_the_modern_ui_shell_background_color() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // O fundo calculado do workbench e da grade vem do `.css`.
    assert_eq!(
        (registered_defaults("modernUI.shellBackground"), registered_defaults("modernUI.inactiveShellBackground")),
        (
            Some(Defaults::Single(ColorValue::Id("titleBar.activeBackground"))),
            Some(Defaults::Single(ColorValue::Id("titleBar.inactiveBackground"))),
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
    // Os valores calculados dos painéis, das abas, das sondas de `var(--modern-ui-...)` e da aba das configurações
    // (`getComputedStyle`) vêm dos `.css`; ficam o registro e os padrões de cada cor.
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
