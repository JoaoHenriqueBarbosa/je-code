//! Porte de `src/vs/platform/accessibility/test/browser/accessibilityService.test.ts` (suíte `AccessibilityService`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`TestInstantiationService`.** Os serviços entram direto no construtor: `TestConfigurationService` com os quatro
//!   valores padrão do `createService` e `config`, o `MockContextKeyService` e um `ILayoutService` com o `container`
//!   como contêiner principal, ativo e único, e o `onDidAddContainer` ligado a um emissor do teste.
//! - **DOM.** `document.createElement('div')` é `ViewElement::new()` e `classList.contains` é `class_list_contains`.
//! - **`matchMedia`.** O navegador de teste (jsdom) responde que nenhuma consulta casa e nunca emite `change`: é o
//!   [`TestHost`].
//! - **Evento de configuração.** O literal `{ affectsConfiguration(id) { return id === ... } }` é o
//!   `ConfigurationChangeEvent` real com a chave alterada, cuja `affectsConfiguration` responde o mesmo.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o [`LeakGuard`] criado no começo de cada caso.

mod common;

use std::rc::Rc;

use common::{LeakGuard, change, empty_test_configuration, ensure_no_disposables_are_leaked_in_test_suite, null_log};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableStore;
use je_platform::accessibility::IAccessibilityService;
use je_platform::accessibility::accessibility_service::{AccessibilityService, IAccessibilityHost, IMediaQueryList};
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::layout::layout_service::{ILayoutService, LayoutAddContainerEvent, LayoutContainerEvent, Viewport};

/// O `ILayoutService` do `createService`: `mainContainer`, `activeContainer`, `containers`, `getContainer`,
/// `onDidAddContainer` e `onDidLayoutContainer: Event.None`.
struct TestLayoutService {
    container: Rc<ViewElement>,
    on_did_add_container: Rc<Emitter<LayoutAddContainerEvent>>,
}

impl ILayoutService for TestLayoutService {
    fn main_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }
    fn active_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }
    fn get_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }
    fn on_did_layout_container(&self) -> Event<LayoutContainerEvent> {
        Event::none()
    }
    fn window_viewport(&self) -> Viewport {
        Viewport { inner_width: 1280.0, inner_height: 800.0, page_x_offset: 0.0, page_y_offset: 0.0 }
    }
    fn containers(&self) -> Vec<Rc<ViewElement>> {
        vec![self.container.clone()]
    }
    fn on_did_add_container(&self) -> Event<LayoutAddContainerEvent> {
        self.on_did_add_container.event()
    }
}

/// `matchMedia(...)` sem correspondência e sem mudança.
struct NoMatch;

impl IMediaQueryList for NoMatch {
    fn matches(&self) -> bool {
        false
    }
    fn on_change(&self) -> Event<()> {
        Event::none()
    }
}

struct TestHost;

impl IAccessibilityHost for TestHost {
    fn match_media(&self, _query: &str) -> Rc<dyn IMediaQueryList> {
        Rc::new(NoMatch)
    }
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

struct Env {
    service: Rc<AccessibilityService>,
    configuration_service: Rc<TestConfigurationService>,
    container: Rc<ViewElement>,
    on_did_add_container: Rc<Emitter<LayoutAddContainerEvent>>,
}

/// `createService(config)`.
fn create_service(guard: &LeakGuard, config: &[(&str, Dynamic)]) -> Env {
    let mut values = vec![
        ("editor.accessibilitySupport", Dynamic::string("off")),
        ("workbench.reduceMotion", Dynamic::string("off")),
        ("workbench.reduceTransparency", Dynamic::string("off")),
        ("accessibility.underlineLinks", Dynamic::Bool(false)),
    ];
    for (key, value) in config {
        match values.iter_mut().find(|entry| entry.0 == *key) {
            Some(slot) => slot.1 = value.clone(),
            None => values.push((*key, value.clone())),
        }
    }
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object(values)));
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());

    let container = ViewElement::new();
    let on_did_add_container = Emitter::new();
    guard.add_rc(on_did_add_container.clone());
    let layout_service: Rc<dyn ILayoutService> = Rc::new(TestLayoutService { container: container.clone(), on_did_add_container: on_did_add_container.clone() });

    let service = guard.add_rc(AccessibilityService::new(MockContextKeyService::new(), layout_service, configuration_service.clone(), Rc::new(TestHost)));
    Env { service, configuration_service, container, on_did_add_container }
}

fn transparency(value: &str) -> (&'static str, Dynamic) {
    ("workbench.reduceTransparency", Dynamic::string(value))
}

fn motion(value: &str) -> (&'static str, Dynamic) {
    ("workbench.reduceMotion", Dynamic::string(value))
}

// suite('isTransparencyReduced')

#[test]
fn is_transparency_reduced_returns_false_when_config_is_off() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[transparency("off")]);
    assert!(!env.service.is_transparency_reduced());
}

#[test]
fn is_transparency_reduced_returns_true_when_config_is_on() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[transparency("on")]);
    assert!(env.service.is_transparency_reduced());
}

#[test]
fn is_transparency_reduced_adds_css_class_when_config_is_on() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[transparency("on")]);
    assert!(env.container.class_list_contains("monaco-reduce-transparency"));
}

#[test]
fn is_transparency_reduced_does_not_add_css_class_when_config_is_off() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[transparency("off")]);
    assert!(!env.container.class_list_contains("monaco-reduce-transparency"));
}

#[test]
fn is_transparency_reduced_fires_event_and_updates_class_on_config_change() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[transparency("off")]);
    assert!(!env.service.is_transparency_reduced());

    let fired = Rc::new(std::cell::Cell::new(false));
    let fired_in_listener = fired.clone();
    guard.add(env.service.on_did_change_reduced_transparency().listen(move |()| fired_in_listener.set(true)));

    // Simulate config change
    let _ = env.configuration_service.set_user_configuration("workbench.reduceTransparency", Dynamic::string("on"), None);
    let event = ConfigurationChangeEvent::new(change(&["workbench.reduceTransparency"], &[]), None, Rc::new(empty_test_configuration()), None, null_log());
    env.configuration_service.on_did_change_configuration_emitter.fire(Rc::new(event));

    assert!(fired.get());
    assert!(env.service.is_transparency_reduced());
    assert!(env.container.class_list_contains("monaco-reduce-transparency"));
}

// suite('isMotionReduced')

#[test]
fn is_motion_reduced_returns_false_when_config_is_off() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[motion("off")]);
    assert!(!env.service.is_motion_reduced());
}

#[test]
fn is_motion_reduced_returns_true_when_config_is_on() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[motion("on")]);
    assert!(env.service.is_motion_reduced());
}

#[test]
fn is_motion_reduced_adds_css_classes_when_config_is_on() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[motion("on")]);
    assert!(env.container.class_list_contains("monaco-reduce-motion"));
    assert!(!env.container.class_list_contains("monaco-enable-motion"));
}

#[test]
fn is_motion_reduced_adds_css_classes_when_config_is_off() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[motion("off")]);
    assert!(!env.container.class_list_contains("monaco-reduce-motion"));
    assert!(env.container.class_list_contains("monaco-enable-motion"));
}

#[test]
fn is_motion_reduced_adds_css_classes_to_new_containers() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = create_service(&guard, &[motion("off")]);
    let auxiliary_container = ViewElement::new();
    env.on_did_add_container.fire(LayoutAddContainerEvent { container: auxiliary_container.clone(), disposables: DisposableStore::new() });

    assert_eq!(
        (auxiliary_container.class_list_contains("monaco-reduce-motion"), auxiliary_container.class_list_contains("monaco-enable-motion")),
        (false, true)
    );
}
