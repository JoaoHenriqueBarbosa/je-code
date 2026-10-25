//! Porte de `src/vs/workbench/contrib/modernUI/test/electron-browser/frostedGlass.contribution.test.ts` (suíte
//! `FrostedGlassContribution`), caso a caso e na mesma ordem. Os casos em laço do original (`for (const ... of [...])`)
//! viram um `#[test]` por elemento.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso, e `store.add(...)` é
//!   `guard.add_rc(...)`.
//! - O DOM do original (`document.createElement('div')`, `classList.contains`, `style.getPropertyValue`) é o `TestHost`
//!   (o trait `FrostedGlassHost`): cada contêiner é um [`LayoutContainerId`] com as classes e as variáveis de estilo que a
//!   contribuição escreve. `layout.containers.push(x)` é `push_container`, e `addedContainer.fire(...)` é `add_container`.
//! - `TestAccessibilityService`, `TestThemeService` (com `TestColorTheme({}, scheme)`) e o `INativeHostService` do caso são
//!   `TestAccessibilityService`, `TestThemeService` e `TestNativeHost`, as implementações dos recortes que a contribuição
//!   lê. `ColorScheme.HIGH_CONTRAST_DARK`, `HIGH_CONTRAST_LIGHT` e `LIGHT` são os `ThemeType` de mesmo nome.
//! - `NullLogService` com `warn` sobrescrito é o `TestLogService`, que guarda o texto de cada `warn`.
//! - `TestConfigurationService.setUserConfiguration` seguido de `onDidChangeConfigurationEmitter.fire({ affectsConfiguration:
//!   setting => setting === key, ... })` é o `set_user_configuration` seguido do `ConfigurationChangeEvent` da chave.
//! - `await Promise.resolve()`, `await pending.complete(...)` e `await services.configure(...)` esvaziam a fila de
//!   microtarefas (`run_microtasks`), que contém as reações das promessas.
//! - `properties` (o `{ ...getConfigurationProperties() }` do começo da suíte, depois de o `import` de
//!   `workbench.contribution.js` registrar as configurações) é o registro depois de `register_frosted_glass_configuration(true)`
//!   (`isNative` é verdadeiro no `electron-browser`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime::run_microtasks;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationModel};
use je_platform::configuration::registry::{PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::ConfigurationChange;
use je_platform::log::{LogArg, LogLevel, LogMessage, Logger, NullLogService};
use je_theme::ThemeType;
use je_workbench::contrib::modern_ui::frosted_glass_configuration::register_frosted_glass_configuration;
use je_workbench::contrib::modern_ui::frosted_glass_contribution::{
    FROSTED_GLASS_CLASS, FROSTED_GLASS_OPACITY_PROPERTY, FrostedGlassContribution, FrostedGlassHost, FrostedGlassThemeService, GpuCompositingHost,
};
use je_workbench::services::layout::layout_service::{AddedContainer, LayoutContainerId, LayoutSettings, ModernUiFrostedGlassOpacity};
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};

/// O `mainContainer` do `layout`.
const MAIN_CONTAINER: LayoutContainerId = LayoutContainerId(1);

/// O `HTMLElement` do original: as classes e as variáveis de estilo de cada contêiner.
struct TestHost {
    containers: RefCell<Vec<LayoutContainerId>>,
    classes: RefCell<HashMap<LayoutContainerId, BTreeSet<String>>>,
    properties: RefCell<HashMap<(LayoutContainerId, String), String>>,
    added_container: Rc<Emitter<AddedContainer>>,
}

impl TestHost {
    fn new(added_container: Rc<Emitter<AddedContainer>>) -> Rc<Self> {
        Rc::new(Self {
            containers: RefCell::new(vec![MAIN_CONTAINER]),
            classes: RefCell::new(HashMap::new()),
            properties: RefCell::new(HashMap::new()),
            added_container,
        })
    }

    /// `layout.containers.push(container)`.
    fn push_container(&self, container: LayoutContainerId) {
        self.containers.borrow_mut().push(container);
    }

    /// `layout.containers.push(added)` seguido de `addedContainer.fire({ container: added, disposables })`.
    fn add_container(&self, container: LayoutContainerId, disposables: Rc<DisposableStore>) {
        self.push_container(container);
        self.added_container.fire(AddedContainer { container, disposables });
    }

    /// `container.classList.contains('modern-ui-frosted-glass')`.
    fn has_glass_class(&self, container: LayoutContainerId) -> bool {
        self.classes.borrow().get(&container).is_some_and(|classes| classes.contains(FROSTED_GLASS_CLASS))
    }

    /// `container.style.getPropertyValue('--modern-ui-glass-opacity')`.
    fn opacity_property(&self, container: LayoutContainerId) -> String {
        self.properties.borrow().get(&(container, FROSTED_GLASS_OPACITY_PROPERTY.to_owned())).cloned().unwrap_or_default()
    }
}

impl FrostedGlassHost for TestHost {
    fn on_did_add_container(&self) -> Event<AddedContainer> {
        self.added_container.event()
    }

    fn containers(&self) -> Vec<LayoutContainerId> {
        self.containers.borrow().clone()
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

    fn set_style_property(&self, container: LayoutContainerId, name: &str, value: &str) {
        self.properties.borrow_mut().insert((container, name.to_owned()), value.to_owned());
    }

    fn remove_style_property(&self, container: LayoutContainerId, name: &str) {
        self.properties.borrow_mut().remove(&(container, name.to_owned()));
    }
}

/// `new class extends TestAccessibilityService { reduced = false; ... }`.
struct TestAccessibilityService {
    reduced: Cell<bool>,
    transparency_changed: Rc<Emitter<()>>,
}

impl IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        self.transparency_changed.event()
    }
    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
    fn is_screen_reader_optimized(&self) -> bool {
        false
    }
    fn is_motion_reduced(&self) -> bool {
        false
    }
    fn is_transparency_reduced(&self) -> bool {
        self.reduced.get()
    }
    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }
    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

/// `TestThemeService`: o tema de teste (`TestColorTheme({})` é escuro) e `setTheme`, que dispara a mudança.
struct TestThemeService {
    theme_type: Cell<ThemeType>,
    theme_changed: Rc<Emitter<()>>,
}

impl TestThemeService {
    /// `theme.setTheme(new TestColorTheme({}, scheme))`.
    fn set_theme(&self, theme_type: ThemeType) {
        self.theme_type.set(theme_type);
        self.theme_changed.fire(());
    }
}

impl FrostedGlassThemeService for TestThemeService {
    fn color_theme_type(&self) -> ThemeType {
        self.theme_type.get()
    }

    fn on_did_color_theme_change(&self) -> Event<()> {
        self.theme_changed.event()
    }
}

/// O `INativeHostService` do caso: `checks` conta as leituras e `result` é a promessa que cada uma devolve.
struct TestNativeHost {
    checks: Cell<usize>,
    result: RefCell<Promise<bool>>,
    gpu_changed: Rc<Emitter<bool>>,
}

impl GpuCompositingHost for TestNativeHost {
    fn is_gpu_compositing_enabled(&self) -> Promise<bool> {
        self.checks.set(self.checks.get() + 1);
        self.result.borrow().clone()
    }

    fn on_did_change_gpu_compositing(&self) -> Event<bool> {
        self.gpu_changed.event()
    }
}

/// `new class extends NullLogService { override warn(message) { warnings.push(message); } }`.
struct TestLogService {
    warnings: RefCell<Vec<String>>,
}

impl Logger for TestLogService {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        Event::none()
    }
    fn get_level(&self) -> LogLevel {
        LogLevel::Info
    }
    fn set_level(&self, _level: LogLevel) {}
    fn trace_args(&self, _message: &str, _args: &[LogArg]) {}
    fn debug_args(&self, _message: &str, _args: &[LogArg]) {}
    fn info_args(&self, _message: &str, _args: &[LogArg]) {}
    fn warn_args(&self, message: &LogMessage, _args: &[LogArg]) {
        if let LogMessage::Text(text) = message {
            self.warnings.borrow_mut().push(text.clone());
        }
    }
    fn error_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn flush(&self) {}
}

impl IDisposable for TestLogService {
    fn dispose(&self) {}
}

/// O que `createServices(enabled, isSessionsWindow)` devolve.
struct Services {
    configuration: Rc<TestConfigurationService>,
    layout: Rc<TestHost>,
    accessibility: Rc<TestAccessibilityService>,
    transparency_changed: Rc<Emitter<()>>,
    theme: Rc<TestThemeService>,
    native_host: Rc<TestNativeHost>,
    gpu_changed: Rc<Emitter<bool>>,
    log: Rc<TestLogService>,
    environment: Rc<TestEnvironmentService>,
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

/// `await configuration.setUserConfiguration(key, value)` e o `onDidChangeConfigurationEmitter.fire({ ... })`.
fn update_configuration(configuration_service: &TestConfigurationService, key: &str, value: Dynamic) {
    let _ = configuration_service.set_user_configuration(key, value, None);
    let change = ConfigurationChange { keys: vec![JsString::from(key)], overrides: Vec::new() };
    let event = ConfigurationChangeEvent::new(change, None, Rc::new(empty_configuration()), None, NullLogService::shared());
    configuration_service.on_did_change_configuration_emitter.fire(Rc::new(event));
}

/// O estado de um contêiner que `styleState` lê: a classe e a variável de opacidade.
type ContainerStyle = (bool, String);

fn glass(opacity: &str) -> ContainerStyle {
    (true, opacity.to_owned())
}

fn solid() -> ContainerStyle {
    (false, String::new())
}

fn default_opacity() -> String {
    format!("{}%", ModernUiFrostedGlassOpacity::Default as i32)
}

impl Services {
    fn new(guard: &LeakGuard, enabled: bool, is_sessions_window: bool) -> Self {
        let configuration = TestConfigurationService::new(Some(Dynamic::object([
            (LayoutSettings::ModernUi.as_str(), Dynamic::Bool(true)),
            (LayoutSettings::ModernUiFrostedGlass.as_str(), Dynamic::Bool(enabled)),
        ])));
        guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
        let added_container = guard.add_rc(Emitter::<AddedContainer>::new());
        let layout = TestHost::new(added_container.clone());
        let transparency_changed = guard.add_rc(Emitter::<()>::new());
        let accessibility = Rc::new(TestAccessibilityService { reduced: Cell::new(false), transparency_changed: transparency_changed.clone() });
        let theme_changed = guard.add_rc(Emitter::<()>::new());
        let theme = Rc::new(TestThemeService { theme_type: Cell::new(ThemeType::Dark), theme_changed });
        let gpu_changed = guard.add_rc(Emitter::<bool>::new());
        let native_host =
            Rc::new(TestNativeHost { checks: Cell::new(0), result: RefCell::new(Promise::resolved(true)), gpu_changed: gpu_changed.clone() });
        let log = guard.add_rc(Rc::new(TestLogService { warnings: RefCell::new(Vec::new()) }));
        let environment = TestEnvironmentService::with_overrides(EnvironmentOverrides { is_sessions_window, ..EnvironmentOverrides::default() });
        Self { configuration, layout, accessibility, transparency_changed, theme, native_host, gpu_changed, log, environment }
    }

    /// `createContribution()`.
    fn create_contribution(&self, guard: &LeakGuard) -> Rc<FrostedGlassContribution> {
        let configuration: Rc<dyn IConfigurationService> = self.configuration.clone();
        guard.add_rc(FrostedGlassContribution::new(
            configuration,
            self.layout.clone(),
            self.accessibility.clone(),
            self.theme.clone(),
            self.native_host.clone(),
            self.log.clone(),
            self.environment.clone(),
        ))
    }

    /// `state()`.
    fn state(&self) -> Vec<bool> {
        self.layout.containers().into_iter().map(|container| self.layout.has_glass_class(container)).collect()
    }

    /// `styleState()`.
    fn style_state(&self) -> Vec<ContainerStyle> {
        self.layout.containers().into_iter().map(|container| (self.layout.has_glass_class(container), self.layout.opacity_property(container))).collect()
    }

    /// `configure(key, value)`.
    fn configure(&self, key: &str, value: Dynamic) {
        update_configuration(&self.configuration, key, value);
        run_microtasks();
    }

    fn checks(&self) -> usize {
        self.native_host.checks.get()
    }

    /// `services.nativeHost.result = pending.p`.
    fn set_native_result(&self, result: Promise<bool>) {
        *self.native_host.result.borrow_mut() = result;
    }

    fn warnings(&self) -> Vec<String> {
        self.log.warnings.borrow().clone()
    }
}

fn opacity_key() -> &'static str {
    LayoutSettings::ModernUiFrostedGlassOpacity.as_str()
}

fn property(key: LayoutSettings) -> PropertySchema {
    register_frosted_glass_configuration(true);
    configuration_registry().get_configuration_properties().get(key.as_str()).expect("a configuração está registrada")
}

fn has_experimental_tag(property: &PropertySchema) -> bool {
    match property.get("tags") {
        Dynamic::Array(tags) => tags.to_vec().iter().any(|tag| tag.strict_equals(&Dynamic::string("experimental"))),
        _ => false,
    }
}

#[test]
fn shares_enabled_and_opacity_defaults_with_the_agents_window() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let enabled = property(LayoutSettings::ModernUiFrostedGlass);
    let opacity = property(LayoutSettings::ModernUiFrostedGlassOpacity);

    assert_deep_strict_equal(
        &Dynamic::object([
            ("enabled", enabled.get("default")),
            ("agentsWindow", enabled.get("agentsWindow")),
            ("opacity", opacity.get("default")),
            ("agentsOpacity", opacity.get("agentsWindow")),
        ]),
        &Dynamic::object([
            ("enabled", Dynamic::Bool(true)),
            ("agentsWindow", Dynamic::Undefined),
            ("opacity", Dynamic::Number(80.0)),
            ("agentsOpacity", Dynamic::Undefined),
        ]),
    );
}

#[test]
fn registers_both_settings_without_experimental_names_or_tags() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let keys = [LayoutSettings::ModernUiFrostedGlass, LayoutSettings::ModernUiFrostedGlassOpacity];

    assert_eq!(
        (keys.map(|key| key.as_str().to_owned()), keys.map(|key| has_experimental_tag(&property(key)))),
        (["workbench.modernUIFrostedGlass".to_owned(), "workbench.modernUIFrostedGlassOpacity".to_owned()], [false, false])
    );
}

#[test]
fn reads_the_shared_gpu_state_once_without_changing_surfaces_when_explicitly_disabled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, false, false);
    services.create_contribution(&guard);
    services.configure(opacity_key(), Dynamic::Number(75.0));

    assert_eq!((services.style_state(), services.checks()), (vec![solid()], 1));
}

/// O corpo do laço `normalizes ${name} opacity and reports invalid values`.
fn normalizes_opacity_and_reports_invalid_values(value: Dynamic, opacity: &str, warns: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let _ = services.configuration.set_user_configuration("workbench", Dynamic::object([("modernUIFrostedGlassOpacity", value)]), None);
    services.create_contribution(&guard);
    run_microtasks();

    let warnings: Vec<bool> = services.warnings().iter().map(|warning| warning.contains(opacity_key())).collect();
    assert_eq!((services.style_state(), warnings), (vec![glass(&format!("{opacity}%"))], if warns { vec![true] } else { Vec::new() }));
}

#[test]
fn normalizes_unset_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Undefined, "80", false);
}

#[test]
fn normalizes_minimum_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Number(50.0), "50", false);
}

#[test]
fn normalizes_fractional_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Number(75.5), "75.5", false);
}

#[test]
fn normalizes_maximum_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Number(100.0), "100", false);
}

#[test]
fn normalizes_below_minimum_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Number(25.0), "50", true);
}

#[test]
fn normalizes_above_maximum_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Number(125.0), "100", true);
}

#[test]
fn normalizes_nan_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Number(f64::NAN), "80", true);
}

#[test]
fn normalizes_positive_infinity_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Number(f64::INFINITY), "80", true);
}

#[test]
fn normalizes_negative_infinity_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Number(f64::NEG_INFINITY), "80", true);
}

#[test]
fn normalizes_string_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::string("75"), "80", true);
}

#[test]
fn normalizes_boolean_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Bool(true), "80", true);
}

#[test]
fn normalizes_null_opacity_and_reports_invalid_values() {
    normalizes_opacity_and_reports_invalid_values(Dynamic::Null, "80", true);
}

#[test]
fn requires_modern_ui_as_well_as_the_glass_setting() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    services.configure(LayoutSettings::ModernUi.as_str(), Dynamic::Bool(false));
    services.create_contribution(&guard);
    run_microtasks();

    assert_eq!((services.state(), services.checks()), (vec![false], 1));
}

#[test]
fn agents_overlays_do_not_depend_on_the_editor_window_modern_ui_setting() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, true);
    services.configure(LayoutSettings::ModernUi.as_str(), Dynamic::Bool(false));
    services.configure(opacity_key(), Dynamic::Number(75.0));
    services.create_contribution(&guard);
    run_microtasks();
    let enabled_state = services.style_state();
    services.configure(LayoutSettings::ModernUiFrostedGlass.as_str(), Dynamic::Bool(false));

    assert_eq!((enabled_state, services.style_state()), (vec![glass("75%")], vec![solid()]));
}

/// O corpo do laço `Agents overlays retain the ${fallback} fallback`.
fn agents_overlays_retain_the_fallback(fallback: &str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, fallback != "disabled", true);
    services.configure(LayoutSettings::ModernUi.as_str(), Dynamic::Bool(false));
    services.accessibility.reduced.set(fallback == "reduced transparency");
    if fallback == "high contrast" {
        services.theme.set_theme(ThemeType::HighContrastDark);
    }
    if fallback == "software compositing" {
        services.set_native_result(Promise::resolved(false));
    }
    services.create_contribution(&guard);
    run_microtasks();

    assert_eq!(services.style_state(), vec![solid()]);
}

#[test]
fn agents_overlays_retain_the_disabled_fallback() {
    agents_overlays_retain_the_fallback("disabled");
}

#[test]
fn agents_overlays_retain_the_reduced_transparency_fallback() {
    agents_overlays_retain_the_fallback("reduced transparency");
}

#[test]
fn agents_overlays_retain_the_high_contrast_fallback() {
    agents_overlays_retain_the_fallback("high contrast");
}

#[test]
fn agents_overlays_retain_the_software_compositing_fallback() {
    agents_overlays_retain_the_fallback("software compositing");
}

/// O corpo do laço `stays solid until compositing is known, then uses accelerated=${accelerated}`.
fn stays_solid_until_compositing_is_known(accelerated: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let pending = DeferredPromise::<bool>::new();
    services.set_native_result(pending.promise.clone());
    services.create_contribution(&guard);
    let pending_state = services.state();
    let _ = pending.complete(accelerated);
    run_microtasks();

    assert_eq!((pending_state, services.state()), (vec![false], vec![accelerated]));
}

#[test]
fn stays_solid_until_compositing_is_known_then_uses_accelerated_true() {
    stays_solid_until_compositing_is_known(true);
}

#[test]
fn stays_solid_until_compositing_is_known_then_uses_accelerated_false() {
    stays_solid_until_compositing_is_known(false);
}

#[test]
fn uses_the_latest_opacity_when_a_pending_capability_check_completes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let pending = DeferredPromise::<bool>::new();
    services.set_native_result(pending.promise.clone());
    services.create_contribution(&guard);
    services.configure(opacity_key(), Dynamic::Number(75.0));
    let pending_state = services.style_state();
    let _ = pending.complete(true);
    run_microtasks();

    assert_eq!((pending_state, services.style_state(), services.checks()), (vec![solid()], vec![glass("75%")], 1));
}

#[test]
fn logs_a_failed_capability_check_and_retains_solid_backgrounds() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let pending = DeferredPromise::<bool>::new();
    services.set_native_result(pending.promise.clone());
    services.create_contribution(&guard);
    let _ = pending.error(Error::new("GPU status unavailable"));
    run_microtasks();

    assert_eq!(
        (services.state(), services.warnings()),
        (vec![false], vec!["Unable to read initial GPU compositing state for frosted glass.".to_owned()])
    );
}

#[test]
fn updates_existing_and_newly_added_auxiliary_windows_and_cleans_them_up() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    services.layout.push_container(LayoutContainerId(2));
    let contribution = services.create_contribution(&guard);
    run_microtasks();
    let initial_state = services.style_state();
    services.configure(opacity_key(), Dynamic::Number(75.0));
    let updated_state = services.style_state();
    let auxiliary_store = guard.add_rc(DisposableStore::new());
    services.layout.add_container(LayoutContainerId(3), auxiliary_store.clone());
    let enabled_state = services.style_state();
    auxiliary_store.dispose();
    let closed_state = services.style_state();
    contribution.dispose();

    assert_eq!(initial_state, vec![glass(&default_opacity()), glass(&default_opacity())]);
    assert_eq!(updated_state, vec![glass("75%"), glass("75%")]);
    assert_eq!(enabled_state, vec![glass("75%"), glass("75%"), glass("75%")]);
    assert_eq!(closed_state, vec![glass("75%"), glass("75%"), solid()]);
    assert_eq!(services.style_state(), vec![solid(), solid(), solid()]);
    assert_eq!(services.checks(), 1);
}

#[test]
fn turns_off_immediately_when_gpu_compositing_is_lost_and_can_recover() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    services.create_contribution(&guard);
    run_microtasks();
    let initial_state = services.style_state();
    services.gpu_changed.fire(false);
    services.configure(opacity_key(), Dynamic::Number(75.0));
    let lost_state = services.style_state();
    services.gpu_changed.fire(true);

    assert_eq!((initial_state, lost_state, services.style_state()), (vec![glass("80%")], vec![solid()], vec![glass("75%")]));
}

#[test]
fn a_gpu_loss_event_wins_over_a_stale_startup_response() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let pending = DeferredPromise::<bool>::new();
    services.set_native_result(pending.promise.clone());
    services.create_contribution(&guard);
    services.gpu_changed.fire(false);
    let _ = pending.complete(true);
    run_microtasks();

    assert_eq!(services.state(), vec![false]);
}

#[test]
fn a_gpu_recovery_event_wins_over_a_stale_startup_response() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let pending = DeferredPromise::<bool>::new();
    services.set_native_result(pending.promise.clone());
    services.create_contribution(&guard);
    services.gpu_changed.fire(false);
    services.gpu_changed.fire(true);
    let _ = pending.complete(false);
    run_microtasks();

    assert_eq!(services.state(), vec![true]);
}

#[test]
fn recovers_through_the_shared_gpu_event_after_the_initial_read_fails() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let pending = DeferredPromise::<bool>::new();
    services.set_native_result(pending.promise.clone());
    services.create_contribution(&guard);
    let _ = pending.error(Error::new("GPU state unavailable"));
    run_microtasks();
    let failed_state = services.state();
    services.gpu_changed.fire(true);

    assert_eq!((failed_state, services.state(), services.checks()), (vec![false], vec![true], 1));
}

#[test]
fn disabling_during_the_capability_check_prevents_late_enablement() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let pending = DeferredPromise::<bool>::new();
    services.set_native_result(pending.promise.clone());
    services.create_contribution(&guard);
    services.configure(LayoutSettings::ModernUiFrostedGlass.as_str(), Dynamic::Bool(false));
    services.gpu_changed.fire(true);
    let _ = pending.complete(true);
    run_microtasks();

    assert_eq!(services.state(), vec![false]);
}

#[test]
fn disposing_during_the_capability_check_prevents_late_enablement() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    let pending = DeferredPromise::<bool>::new();
    services.set_native_result(pending.promise.clone());
    let contribution = services.create_contribution(&guard);
    contribution.dispose();
    let _ = pending.complete(true);
    run_microtasks();
    services.gpu_changed.fire(true);

    assert_eq!(services.state(), vec![false]);
}

#[test]
fn tracks_capabilities_while_disabled_without_issuing_another_state_read() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    services.create_contribution(&guard);
    run_microtasks();
    services.configure(LayoutSettings::ModernUiFrostedGlass.as_str(), Dynamic::Bool(false));
    let disabled_state = services.state();
    services.gpu_changed.fire(false);
    services.configure(LayoutSettings::ModernUiFrostedGlass.as_str(), Dynamic::Bool(true));
    run_microtasks();

    assert_eq!((disabled_state, services.state(), services.checks()), (vec![false], vec![false], 1));
}

#[test]
fn uses_gpu_updates_received_while_initially_disabled_when_enabling_glass() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, false, false);
    services.set_native_result(Promise::resolved(false));
    services.create_contribution(&guard);
    run_microtasks();
    services.gpu_changed.fire(true);
    let disabled_state = services.state();
    services.configure(LayoutSettings::ModernUiFrostedGlass.as_str(), Dynamic::Bool(true));

    assert_eq!((disabled_state, services.state(), services.checks()), (vec![false], vec![true], 1));
}

#[test]
fn responds_immediately_to_reduced_transparency_and_resumes_when_allowed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    services.create_contribution(&guard);
    run_microtasks();
    services.accessibility.reduced.set(true);
    services.transparency_changed.fire(());
    services.configure(opacity_key(), Dynamic::Number(50.0));
    services.gpu_changed.fire(true);
    let reduced_state = services.style_state();
    services.accessibility.reduced.set(false);
    services.transparency_changed.fire(());
    run_microtasks();

    assert_eq!((reduced_state, services.style_state()), (vec![solid()], vec![glass("50%")]));
}

/// O corpo do laço `retains solid surfaces in ${scheme} and resumes in a normal theme`.
fn retains_solid_surfaces_in_high_contrast_and_resumes_in_a_normal_theme(scheme: ThemeType) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let services = Services::new(&guard, true, false);
    services.create_contribution(&guard);
    run_microtasks();
    services.theme.set_theme(scheme);
    services.configure(opacity_key(), Dynamic::Number(50.0));
    let high_contrast_state = services.style_state();
    services.theme.set_theme(ThemeType::Light);
    run_microtasks();

    assert_eq!((high_contrast_state, services.style_state()), (vec![solid()], vec![glass("50%")]));
}

#[test]
fn retains_solid_surfaces_in_hc_black_and_resumes_in_a_normal_theme() {
    retains_solid_surfaces_in_high_contrast_and_resumes_in_a_normal_theme(ThemeType::HighContrastDark);
}

#[test]
fn retains_solid_surfaces_in_hc_light_and_resumes_in_a_normal_theme() {
    retains_solid_surfaces_in_high_contrast_and_resumes_in_a_normal_theme(ThemeType::HighContrastLight);
}
