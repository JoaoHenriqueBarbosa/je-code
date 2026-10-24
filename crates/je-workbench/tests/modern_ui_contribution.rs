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
//! Recortes: os demais casos da suíte (cerca de 110) leem o estilo calculado de um DOM com os `*.css` do VS Code
//! (`getComputedStyle`, `getBoundingClientRect`, `generateColorThemeCSS`) ou os registros de cor e de tema do
//! `common/theme.ts`; não há DOM, e o `.css` é a especificação lida ao portar cada widget. Voltam com o desenho nativo
//! dos widgets e com o registro de cores: `insets multi diff file rows ...`, `supports isolated notification and dialog
//! presentation`, os de cor de painel, aba e barra de atividade, `toggles uppercase view headers without relayout`,
//! `Explorer title casing ...`, as abas conectadas por tema e todos os que seguem até `keeps panel global actions
//! above overflowing title actions`. No terceiro caso, `paneHeaderLineHeight` e `paneHeaderInlineLineHeight` (estilo
//! calculado do cabeçalho) ficam de fora.

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
use je_platform::log::NullLogService;
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
