//! Os dublês e as montagens compartilhados pelos testes de `baseDebugView.test.ts`, `watchExpressionView.test.ts` e
//! `variablesView.test.ts` (e pelo `handleANSIOutput`): o `LinkDetector` sobre os serviços de teste, o
//! `DebugExpressionRenderer`, o `IDebugService` cujo `getViewModel` o teste troca e o escopo, o quadro e a variável do
//! `setup` de cada caso.
//!
//! Divergências:
//! - **`MockDebugService`** (de `mockDebug.ts`) só entra aqui pelo recorte `IExpressionsDebugService` com o `getViewModel` que
//!   os testes sobrescrevem (`{ focusedStackFrame: undefined, getSelectedExpression: () => undefined }`); os demais membros
//!   do `MockDebugService` nenhum caso chama.
//! - **`range` do `StackFrame`.** O original passa `{ startLineNumber: 1, startColumn: 1, endLineNumber: undefined!,
//!   endColumn: undefined! }`, que nenhum caso lê; aqui o quadro não tem `range`.
//! - **`source: null!`** do `StackFrame` é um `Source` sem raw (`mock`), como o `createMockDebugModel` faz.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::DisposableStore;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::opener::opener::{
    IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_workbench::contrib::debug::base_debug_view::{
    IExpressionTemplateData, IExpressionTreeNode, IExpressionsDebugService, IExpressionsViewModel, IHighlightedLabel, LogicalHighlightedLabel, NullHoverService,
};
use je_workbench::contrib::debug::debug_expression_renderer::{DebugExpressionRenderer, DebugExpressionRendererServices};
use je_workbench::contrib::debug::debug_model::{Scope, StackFrame, Thread, Variable, VariableInit};
use je_workbench::contrib::debug::debug_source::Source;
use je_workbench::contrib::debug::debug_types::{IDebugSessionFull, IExpression, IExpressionContainer};
use je_workbench::contrib::debug::debug_view_model::SelectedExpression;
use je_workbench::contrib::debug::link_detector::{Element, IDebugSourceOpener, ITunnelService, LinkDetector};
use je_workbench::test::browser::workbench_test_services::{TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::TestFileService;
use je_base::lifecycle::DisposableRef;
use serde_json::Value;

use super::mock_command_service::MockCommandService;
use super::mock_debug::{MockSession, mock_uri_identity_service};

/// `{ canTunnel: () => false }`.
pub struct NoTunnelService;

impl ITunnelService for NoTunnelService {
    fn can_tunnel(&self, _uri: &URI) -> bool {
        false
    }
}

/// O `IOpenerService` do `workbenchInstantiationService`: nenhum caso abre um link.
pub struct TestOpenerService;

impl IOpenerService for TestOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        unreachable!("o linkDetector não registra opener")
    }

    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        unreachable!("o linkDetector não registra validador")
    }

    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        unreachable!("o linkDetector não registra resolvedor")
    }

    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        unreachable!("o linkDetector não troca o opener externo")
    }

    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        unreachable!("o linkDetector não registra opener externo")
    }

    fn open(&self, _resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        unreachable!("o linkDetector não resolve URI externo")
    }
}

/// O `createInstance(LinkDetector)` sobre os serviços de teste do workbench.
pub fn create_test_link_detector(configuration_service: Rc<dyn IConfigurationService>) -> LinkDetector {
    LinkDetector::new(
        Rc::new(TestEditorService),
        TestFileService::new(),
        Rc::new(TestOpenerService),
        TestPathService::new(None, None),
        Rc::new(NoTunnelService),
        TestEnvironmentService::new(),
        configuration_service,
    )
}

/// O `createInstance(DebugExpressionRenderer)` do `setup`: o hover é o `NullHoverService`; nenhum caso abre um destino de
/// `locationReference`.
pub fn create_expression_renderer(configuration_service: &Rc<TestConfigurationService>) -> Rc<DebugExpressionRenderer> {
    let command_service: Rc<dyn ICommandService> = Rc::new(MockCommandService::default());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    DebugExpressionRenderer::new(DebugExpressionRendererServices {
        command_service,
        configuration_service: configuration.clone(),
        hover_service: Rc::new(NullHoverService),
        link_detector: Rc::new(create_test_link_detector(configuration)),
        source_opener: Rc::new(|_source: Rc<Source>| -> Rc<dyn IDebugSourceOpener> { unreachable!("nenhum caso abre o destino de um locationReference") }),
    })
}

/// `{ focusedStackFrame: undefined, getSelectedExpression: () => undefined }`.
pub struct NoSelectionViewModel;

impl IExpressionsViewModel for NoSelectionViewModel {
    fn focused_stack_frame(&self) -> Option<Rc<StackFrame>> {
        None
    }

    fn get_selected_expression(&self) -> Option<SelectedExpression> {
        None
    }

    fn set_selected_expression(&self, _expression: Option<Rc<dyn IExpression>>, _setting_watch: bool) {
        unreachable!("setSelectedExpression não é usado")
    }

    fn update_views(&self) {
        unreachable!("updateViews não é usado")
    }

    fn evaluate_lazy_expression(&self, _expression: Rc<dyn IExpression>) -> Promise<()> {
        unreachable!("evaluateLazyExpression não é usado")
    }

    fn get_visualized_expression(&self, _expression: &Rc<dyn IExpression>) -> Option<Result<Rc<dyn IExpression>, String>> {
        unreachable!("getVisualizedExpression não é usado")
    }

    fn set_visualized_expression(&self, _original: Rc<dyn IExpression>, _visualized: Option<(Rc<dyn IExpression>, String)>) {
        unreachable!("setVisualizedExpression não é usado")
    }
}

/// O `MockDebugService` com o `getViewModel` trocado pelo teste.
pub struct MockViewModelDebugService {
    view_model: Rc<dyn IExpressionsViewModel>,
}

impl MockViewModelDebugService {
    pub fn without_selection() -> Rc<Self> {
        Rc::new(Self { view_model: Rc::new(NoSelectionViewModel) })
    }
}

impl IExpressionsDebugService for MockViewModelDebugService {
    fn get_view_model(&self) -> Rc<dyn IExpressionsViewModel> {
        self.view_model.clone()
    }

    fn rename_watch_expression(&self, _id: &str, _new_name: &str) {
        unreachable!("renameWatchExpression não é usado")
    }

    fn remove_watch_expressions(&self, _id: Option<&str>) {
        unreachable!("removeWatchExpressions não é usado")
    }
}

/// `new Scope(stackFrame, 1, 'local', 1, false, 10, 10)` sobre `new StackFrame(thread, 1, null!, 'app.js', 'normal', range, 0, true)`
/// e `new Thread(session, 'mockthread', 1)`.
pub fn create_local_scope(session: &Rc<MockSession>) -> Rc<Scope> {
    let thread = Thread::new(session.clone(), "mockthread", 1);
    let source = Rc::new(Source::new(None, "mock", &*mock_uri_identity_service(), &NullLogService));
    let stack_frame = StackFrame::new(thread, 1, source, "app.js", Some("normal"), None, 0, true, None);
    Scope::new(stack_frame, 1, "local".to_owned(), 1, false, Some(10), Some(10), None)
}

/// `new Variable(session, 1, scope, 2, name, evaluateName, value, 0, 0, undefined, presentationHint, type)`.
pub fn create_variable(
    session: &Rc<MockSession>,
    scope: &Rc<Scope>,
    name: &str,
    evaluate_name: &str,
    value: Option<&str>,
    presentation_hint: Value,
    variable_type: Option<&str>,
) -> Rc<Variable> {
    let parent: Rc<dyn IExpressionContainer> = scope.clone();
    let session: Rc<dyn IDebugSessionFull> = session.clone();
    let mut init = VariableInit::new(parent, name.to_owned());
    init.session = Some(session);
    init.thread_id = Some(1);
    init.reference = Some(2);
    init.evaluate_name = Some(evaluate_name.to_owned());
    init.value = value.map(str::to_owned);
    init.named_variables = Some(0);
    init.indexed_variables = Some(0);
    init.presentation_hint = Some(presentation_hint);
    init.ty = variable_type.map(str::to_owned);
    Variable::new_variable(init)
}

/// O `data` de `renderElement` dos testes de `watchExpressionView` e `variablesView`: seis elementos `$('.')`, o
/// `HighlightedLabel` sobre o `name` e os dois `DisposableStore` do teste.
pub fn create_expression_template_data(element_disposable: Rc<DisposableStore>, template_disposable: Rc<DisposableStore>) -> Rc<IExpressionTemplateData> {
    let expression = Element::create(".");
    let name = Element::create(".");
    let r#type = Element::create(".");
    let value = Element::create(".");
    let label: Rc<dyn IHighlightedLabel> = LogicalHighlightedLabel::new(&name);
    Rc::new(IExpressionTemplateData {
        expression,
        name,
        r#type,
        value,
        input_box_container: Element::create("."),
        element_disposable,
        template_disposable,
        label,
        lazy_button: Element::create("."),
        current_element: RefCell::new(None),
    })
}

/// O `node` dos dois testes: `{ element: new Variable(session, 1, scope, 2, 'foo', 'bar.foo', undefined, 0, 0, undefined, {}, 'string'),
/// ..., filterData: undefined }`, com a variável à parte (o teste muda o valor dela).
pub fn create_foo_variable_node(session: &Rc<MockSession>, scope: &Rc<Scope>) -> (Rc<Variable>, IExpressionTreeNode) {
    let variable = create_variable(session, scope, "foo", "bar.foo", None, serde_json::json!({}), Some("string"));
    let element: Rc<dyn IExpression> = variable.clone();
    (variable, IExpressionTreeNode { element, filter_data: None })
}
