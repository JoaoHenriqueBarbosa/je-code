//! Porte de `src/vs/workbench/contrib/debug/test/browser/baseDebugView.test.ts` (suíte `Debug - Base Debug View`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - **DOM.** O `$('.')` e o `$('.container')` de `dom.ts` são o `Element::create` do contrato lógico de `link_detector.rs`
//!   (`className`, `textContent`, `childElementCount`, `firstChild` e `querySelector('a')` são `class_name`, `text_content`,
//!   `child_element_count`, `child_nodes()[0]` e `query_selector_tag("a")`). `dom.isHTMLDivElement` é a conferência da
//!   marca `DIV`. O `HighlightedLabel` é o `LogicalHighlightedLabel` de `base_debug_view.rs` (`label.element.textContent` é
//!   `label.element().text_content()`); `label.dispose()` não faz nada porque o rótulo lógico não guarda descartáveis.
//! - **`setup`.** `workbenchInstantiationService(undefined, disposables)`, o `TestConfigurationService` e
//!   `IHoverService => NullHoverService` viram os serviços de teste montados antes do guarda de vazamentos de cada caso (como o
//!   `workbenchInstantiationService` do original, que vive fora do rastreio do caso), passados ao
//!   `DebugExpressionRenderer` por `create_expression_renderer`.
//! - `configurationService.setUserConfiguration('debug.showVariableTypes', ...)` é `set_user_configuration` com `Dynamic::Bool`.
//! - `new Variable(session, 1, scope, 2, name, evaluateName, value, 0, 0, undefined, hint, type)`, o `Thread`, o `StackFrame` e o
//!   `Scope` do `setup` de cada caso são `create_variable` e `create_local_scope` (`tests/support/debug_view_support.rs`).
//! - `variable.value = '...'` (o setter do `ExpressionContainer`) é `variable.base.set_value(...)`; `expression.type = ...` é
//!   `expression.base.expr_type`; `expression.available = true` é `expression.available.set(true)`.
//! - `isWindows` é a plataforma do hospedeiro (`Platform::detect().is_windows`).
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` do caso; a `store` de cada caso é registrada no guarda.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_base::platform::Platform;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_widgets::highlighted_label::IHighlight;
use je_workbench::contrib::debug::base_debug_view::{self, IHighlightedLabel, IVariableTemplateData, LogicalHighlightedLabel};
use je_workbench::contrib::debug::debug::IDebugSession;
use je_workbench::contrib::debug::debug_expression_renderer::{DebugExpressionRenderer, ExpressionOrValue, IRenderValueOptions, IRenderVariableOptions};
use je_workbench::contrib::debug::debug_model::{Expression, Scope};
use je_workbench::contrib::debug::debug_session::DebugSessionOptions;
use je_workbench::contrib::debug::debug_types::{IDebugSessionFull, State};
use je_workbench::contrib::debug::link_detector::{Element, LinkNode};
use je_workbench::contrib::debug::statusbar_color_provider::is_statusbar_in_debug_mode;
use serde_json::json;
use support::debug_view_support::{create_expression_renderer, create_local_scope, create_variable};
use support::mock_debug::{MockSession, create_mock_debug_model, create_test_session};

/// O `setup`: a configuração e o renderizador sobre ela.
struct Setup {
    configuration_service: Rc<TestConfigurationService>,
    renderer: Rc<DebugExpressionRenderer>,
}

fn setup() -> Setup {
    let configuration_service = TestConfigurationService::new(None);
    let renderer = create_expression_renderer(&configuration_service);
    Setup { configuration_service, renderer }
}

/// Os quatro elementos novos de `expression = $('.'); name = $('.'); type = $('.'); value = $('.')`.
struct Elements {
    expression: Rc<Element>,
    name: Rc<Element>,
    r#type: Rc<Element>,
    value: Rc<Element>,
}

impl Elements {
    fn new() -> Self {
        Self { expression: Element::create("."), name: Element::create("."), r#type: Element::create("."), value: Element::create(".") }
    }
}

/// `renderer.renderVariable({ expression, name, type, value, label, lazyButton }, variable, { showChanged: false })` na `store`.
fn render_variable_into(
    renderer: &DebugExpressionRenderer,
    store: &DisposableStore,
    elements: &Elements,
    label: &Rc<LogicalHighlightedLabel>,
    lazy_button: &Rc<Element>,
    variable: &Rc<je_workbench::contrib::debug::debug_model::Variable>,
) {
    let label: Rc<dyn IHighlightedLabel> = label.clone();
    store.add(renderer.render_variable(
        &IVariableTemplateData {
            expression: elements.expression.clone(),
            name: elements.name.clone(),
            r#type: elements.r#type.clone(),
            value: elements.value.clone(),
            label,
            lazy_button: lazy_button.clone(),
        },
        variable,
        &IRenderVariableOptions { show_changed: false, highlights: Vec::<IHighlight>::new() },
    ));
}

fn assert_variable(setup: &Setup, session: &Rc<MockSession>, scope: &Rc<Scope>, store: &DisposableStore, display_type: bool) {
    let renderer = &*setup.renderer;
    let variable = create_variable(session, scope, "foo", "bar.foo", None, json!({}), Some("string"));
    let mut elements = Elements::new();
    let label = LogicalHighlightedLabel::new(&elements.name);
    let lazy_button = Element::create(".");
    render_variable_into(renderer, store,&elements, &label, &lazy_button, &variable);

    assert_eq!(label.element().text_content(), "foo");
    assert_eq!(elements.value.text_content(), "");

    variable.base.set_value("hey");
    elements = Elements::new();
    render_variable_into(renderer, store,&elements, &label, &lazy_button, &variable);
    assert_eq!(elements.value.text_content(), "hey");
    assert_eq!(label.element().text_content(), if display_type { "foo: " } else { "foo =" });
    assert_eq!(elements.r#type.text_content(), if display_type { "string =" } else { "" });

    let path = if Platform::detect().is_windows { "C:\\foo.js:5" } else { "/foo.js:5" };
    variable.base.set_value(path);
    elements = Elements::new();
    render_variable_into(renderer, store,&elements, &label, &lazy_button, &variable);
    let link = elements.value.query_selector_tag("a").expect("o valor tem um link");
    assert_eq!(link.text_content(), path);

    let variable = create_variable(session, scope, "console", "console", Some("5"), json!({ "kind": "virtual" }), None);
    elements = Elements::new();
    render_variable_into(renderer, store,&elements, &label, &lazy_button, &variable);
    assert_eq!(elements.name.class_name(), "virtual");
    assert_eq!(label.element().text_content(), "console =");
    assert_eq!(elements.value.class_name(), "value number");

    let variable = create_variable(session, scope, "xpto", "xpto.xpto", None, json!({}), Some("custom-type"));
    render_variable_into(renderer, store,&elements, &label, &lazy_button, &variable);
    assert_eq!(label.element().text_content(), "xpto");
    assert_eq!(elements.value.text_content(), "");
    variable.base.set_value("2");
    elements = Elements::new();
    render_variable_into(renderer, store,&elements, &label, &lazy_button, &variable);
    assert_eq!(elements.value.text_content(), "2");
    assert_eq!(label.element().text_content(), if display_type { "xpto: " } else { "xpto =" });
    assert_eq!(elements.r#type.text_content(), if display_type { "custom-type =" } else { "" });
}

#[test]
fn render_view_tree() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let container = Element::create(".container");
    let tree_container = base_debug_view::render_view_tree(&container);

    assert_eq!(tree_container.class_name(), "debug-view-content file-icon-themable-tree");
    assert_eq!(container.child_element_count(), 1);
    let first_child = container.child_nodes().into_iter().next();
    assert!(matches!(first_child, Some(LinkNode::Element(first)) if Rc::ptr_eq(&first, &tree_container)));
    assert_eq!(tree_container.tag_name(), "DIV");
}

#[test]
fn render_expression_value() {
    let setup = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let renderer = &*setup.renderer;
    let mut container = Element::create(".container");
    let store = guard.add_rc(DisposableStore::new());
    store.add(renderer.render_value(&container, ExpressionOrValue::Text("render \n me"), &IRenderValueOptions::default()));
    assert_eq!(container.class_name(), "container value");
    assert_eq!(container.text_content(), "render \n me");

    let expression = Expression::new("console", None);
    expression.base.set_value("Object");
    container = Element::create(".container");
    let colorize = IRenderValueOptions { colorize: true, ..IRenderValueOptions::default() };
    store.add(renderer.render_value(&container, ExpressionOrValue::Container(&*expression), &colorize));
    assert_eq!(container.class_name(), "container value unavailable error");

    expression.available.set(true);
    expression.base.set_value("\"string value\"");
    container = Element::create(".container");
    store.add(renderer.render_value(&container, ExpressionOrValue::Container(&*expression), &colorize));
    assert_eq!(container.class_name(), "container value string");
    assert_eq!(container.text_content(), "\"string value\"");

    *expression.base.expr_type.borrow_mut() = Some("boolean".to_owned());
    container = Element::create(".container");
    store.add(renderer.render_value(&container, ExpressionOrValue::Container(&*expression), &colorize));
    assert_eq!(container.class_name(), "container value boolean");
    assert_eq!(container.text_content(), expression.base.value());

    expression.base.set_value("this is a long string");
    container = Element::create(".container");
    store.add(renderer.render_value(
        &container,
        ExpressionOrValue::Container(&*expression),
        &IRenderValueOptions { colorize: true, max_value_length: Some(4), ..IRenderValueOptions::default() },
    ));
    assert_eq!(container.text_content(), "this...");

    let path = if Platform::detect().is_windows { "C:\\foo.js:5" } else { "/foo.js:5" };
    expression.base.set_value(path);
    container = Element::create(".container");
    store.add(renderer.render_value(&container, ExpressionOrValue::Container(&*expression), &colorize));
    let link = container.query_selector_tag("a").expect("o contêiner tem um link");
    assert_eq!(link.text_content(), expression.base.value());
}

#[test]
fn render_variable() {
    let setup = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let session = MockSession::new();
    let scope = create_local_scope(&session);

    drop(setup.configuration_service.set_user_configuration("debug.showVariableTypes", Dynamic::Bool(false), None));
    let store = guard.add_rc(DisposableStore::new());
    assert_variable(&setup, &session, &scope, &store, false);
}

#[test]
fn render_variable_with_display_type_setting() {
    let setup = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let session = MockSession::new();
    let scope = create_local_scope(&session);

    drop(setup.configuration_service.set_user_configuration("debug.showVariableTypes", Dynamic::Bool(true), None));
    let store = guard.add_rc(DisposableStore::new());
    assert_variable(&setup, &session, &scope, &store, true);
}

#[test]
fn statusbar_in_debug_mode() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = guard.add_rc(create_test_session(&model, "mockSession", None, &guard.store()));
    let session2 = guard.add_rc(create_test_session(
        &model,
        "mockSession",
        Some(DebugSessionOptions { suppress_debug_statusbar: Some(true), ..DebugSessionOptions::default() }),
        &guard.store(),
    ));
    let first: Rc<dyn IDebugSessionFull> = session.clone();
    let second: Rc<dyn IDebugSessionFull> = session2.clone();
    assert!(!is_statusbar_in_debug_mode(State::Inactive, &[]));
    assert!(!is_statusbar_in_debug_mode(State::Initializing, &[first.clone()]));
    assert!(is_statusbar_in_debug_mode(State::Running, &[first.clone()]));
    assert!(is_statusbar_in_debug_mode(State::Stopped, &[first.clone()]));

    assert!(!is_statusbar_in_debug_mode(State::Running, &[second.clone()]));
    assert!(is_statusbar_in_debug_mode(State::Running, &[first.clone(), second.clone()]));

    let mut configuration = session.configuration();
    configuration["noDebug"] = serde_json::json!(true);
    session.set_configuration(configuration, None);
    assert!(!is_statusbar_in_debug_mode(State::Running, &[first.clone()]));
    assert!(!is_statusbar_in_debug_mode(State::Running, &[first, second]));
}
