//! Porte de `src/vs/workbench/contrib/debug/test/browser/watchExpressionView.test.ts` (suíte `Debug - Watch Debug View`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - **DOM.** Os `$('.')` do `data` são `Element::create(".")` (o contrato lógico de `link_detector.rs`) e o `HighlightedLabel` é o
//!   `LogicalHighlightedLabel` de `base_debug_view.rs`, montados por `create_expression_template_data`
//!   (`tests/support/debug_view_support.rs`). `value.textContent`, `type.textContent` e `label.element.textContent` são
//!   `text_content()` dos elementos.
//! - **`setup`.** `workbenchInstantiationService(undefined, disposables)`, o `TestConfigurationService`,
//!   `IHoverService => NullHoverService` e o `MockDebugService` cujo `getViewModel` devolve
//!   `{ focusedStackFrame: undefined, getSelectedExpression: () => undefined }` viram os serviços de teste montados antes do guarda de
//!   vazamentos de cada caso (como o `workbenchInstantiationService` do original, que vive fora do rastreio do caso).
//! - `instantiationService.createInstance(WatchExpressionsRenderer, expressionRenderer)` é `WatchExpressionsRenderer::new` com os
//!   serviços explícitos; o `instantiationService.stub(IConfigurationService, configurationService)` repetido dentro do caso é o
//!   mesmo serviço, que já está no construtor.
//! - `configurationService.setUserConfiguration('debug', { showVariableTypes: true })` é `set_user_configuration` com um
//!   `Dynamic::object`.
//! - O `node` (`ITreeNode`) tem só o que o renderizador lê (`element` e `filterData`).
//! - `node.element.value = 'xpto'` é `variable.base.set_value("xpto")` (o `ExpressionContainer.value`).
//! - `disposables.add(new DisposableStore())` e `disposables.add(new HighlightedLabel(name))` são os `DisposableStore` registrados no
//!   `LeakGuard` do caso (o rótulo lógico não guarda descartáveis).
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` do caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::debug::base_debug_view::{AbstractExpressionsRenderer, LogicalInputBoxFactory, NullHoverService};
use je_workbench::contrib::debug::debug_expression_renderer::DebugExpressionRenderer;
use je_workbench::contrib::debug::watch_expressions_view::WatchExpressionsRenderer;
use support::debug_view_support::{
    MockViewModelDebugService, create_expression_renderer, create_expression_template_data, create_foo_variable_node, create_local_scope,
};
use support::mock_debug::MockSession;

fn assert_watch_variable(guard: &LeakGuard, watch_expressions_renderer: &WatchExpressionsRenderer, display_type: bool) {
    let session = MockSession::new();
    let scope = create_local_scope(&session);
    let (variable, node) = create_foo_variable_node(&session, &scope);
    let element_disposable = guard.add_rc(DisposableStore::new());
    let template_disposable = guard.add_rc(DisposableStore::new());
    let data = create_expression_template_data(element_disposable, template_disposable);
    watch_expressions_renderer.render_element(&node, 0, &data);
    assert_eq!(data.value.text_content(), "");
    assert_eq!(data.label.element().text_content(), if display_type { "foo: " } else { "foo =" });

    variable.base.set_value("xpto");
    watch_expressions_renderer.render_element(&node, 0, &data);
    assert_eq!(data.value.text_content(), "xpto");
    assert_eq!(data.r#type.text_content(), if display_type { "string =" } else { "" });
    assert_eq!(data.label.element().text_content(), if display_type { "foo: " } else { "foo =" });
}

/// O `setup`: a configuração, o renderizador de expressões e o serviço de depuração.
struct Setup {
    configuration_service: Rc<TestConfigurationService>,
    expression_renderer: Rc<DebugExpressionRenderer>,
    debug_service: Rc<MockViewModelDebugService>,
}

fn setup() -> Setup {
    let configuration_service = TestConfigurationService::new(None);
    let expression_renderer = create_expression_renderer(&configuration_service);
    Setup { configuration_service, expression_renderer, debug_service: MockViewModelDebugService::without_selection() }
}

impl Setup {
    /// `createInstance(WatchExpressionsRenderer, expressionRenderer)`.
    fn watch_expressions_renderer(&self) -> Rc<WatchExpressionsRenderer> {
        let configuration_service: Rc<dyn IConfigurationService> = self.configuration_service.clone();
        WatchExpressionsRenderer::new(
            self.expression_renderer.clone(),
            self.debug_service.clone(),
            Rc::new(LogicalInputBoxFactory),
            Rc::new(NullHoverService),
            configuration_service,
        )
    }
}

#[test]
fn watch_expressions_with_display_type() {
    let setup = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    drop(setup.configuration_service.set_user_configuration("debug", Dynamic::object([("showVariableTypes", Dynamic::Bool(true))]), None));
    let watch_expressions_renderer = setup.watch_expressions_renderer();
    assert_watch_variable(&guard, &watch_expressions_renderer, true);
}

#[test]
fn watch_expressions() {
    let setup = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    drop(setup.configuration_service.set_user_configuration("debug", Dynamic::object([("showVariableTypes", Dynamic::Bool(false))]), None));
    let watch_expressions_renderer = setup.watch_expressions_renderer();
    assert_watch_variable(&guard, &watch_expressions_renderer, false);
}
