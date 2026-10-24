//! Porte de `src/vs/workbench/contrib/debug/test/browser/variablesView.test.ts` (suíte `Debug - Variable Debug View`), caso a caso
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
//! - `instantiationService.createInstance(VariablesRenderer, expressionRenderer)` é `VariablesRenderer::new` com os serviços
//!   explícitos (o `IMenuService`, o `IContextKeyService`, o `IDebugVisualizerService` e o `IContextMenuService` só servem à
//!   barra de ações, que não é portada: ver o topo de `variables_view.rs`).
//! - `configurationService.setUserConfiguration('debug.showVariableTypes', true)` é `set_user_configuration` com `Dynamic::Bool`.
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
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::debug::base_debug_view::{AbstractExpressionsRenderer, LogicalInputBoxFactory, NullHoverService};
use je_workbench::contrib::debug::debug_expression_renderer::DebugExpressionRenderer;
use je_workbench::contrib::debug::variables_view::VariablesRenderer;
use support::debug_view_support::{
    MockViewModelDebugService, create_expression_renderer, create_expression_template_data, create_foo_variable_node, create_local_scope,
};
use support::mock_debug::MockSession;

fn assert_variable(guard: &LeakGuard, variables_renderer: &VariablesRenderer, display_type: bool) {
    let session = MockSession::new();
    let scope = create_local_scope(&session);
    let (variable, node) = create_foo_variable_node(&session, &scope);
    let element_disposable = guard.add_rc(DisposableStore::new());
    let template_disposable = guard.add_rc(DisposableStore::new());
    let data = create_expression_template_data(element_disposable, template_disposable);
    variables_renderer.render_element(&node, 0, &data);
    assert_eq!(data.value.text_content(), "");
    assert_eq!(data.label.element().text_content(), "foo");

    variable.base.set_value("xpto");
    variables_renderer.render_element(&node, 0, &data);
    assert_eq!(data.value.text_content(), "xpto");
    assert_eq!(data.r#type.text_content(), if display_type { "string =" } else { "" });
    assert_eq!(data.label.element().text_content(), if display_type { "foo: " } else { "foo =" });

    variables_renderer.dispose_template(&data);
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
    /// `createInstance(VariablesRenderer, expressionRenderer)`.
    fn variables_renderer(&self) -> Rc<VariablesRenderer> {
        VariablesRenderer::new(self.expression_renderer.clone(), self.debug_service.clone(), Rc::new(LogicalInputBoxFactory), Rc::new(NullHoverService))
    }
}

#[test]
fn variable_expressions_with_display_type() {
    let setup = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    drop(setup.configuration_service.set_user_configuration("debug.showVariableTypes", Dynamic::Bool(true), None));
    let variables_renderer = setup.variables_renderer();
    assert_variable(&guard, &variables_renderer, true);
}

#[test]
fn variable_expressions() {
    let setup = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    drop(setup.configuration_service.set_user_configuration("debug.showVariableTypes", Dynamic::Bool(false), None));
    let variables_renderer = setup.variables_renderer();
    assert_variable(&guard, &variables_renderer, false);
}
