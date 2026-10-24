//! Porte de `src/vs/workbench/api/test/common/extensionHostMain.test.ts` (suíte `ExtensionHostMain#ErrorHandler - Wrapping
//! prepareStackTrace can cause slowdown and eventual stack overflow #184926`, com a sub-suíte do gist), caso a caso e na mesma
//! ordem.
//!
//! PORTADOS (7 de 7 `test(`): todos, sobre o modelo do motor JS de `extension_host_main.rs` (ver a nota de lá: o V8 monta o
//! `error.stack` uma vez, no primeiro acesso, com o `Error.prepareStackTrace` vigente; aqui isso é o `ErrorConstructor` e o
//! `JsError`). O que os casos conferem (o `findSubstr` chamado uma vez por erro, o embrulho que não se repete em 2500, 10000 e
//! 12500 atribuições, a restauração) é a lógica do `ErrorHandler`, portada inteira; o V8 em si não é exercido.
//!
//! Notas de tradução:
//! - `suiteSetup` (instalar o tratador completo) roda no começo de cada caso, porque o tratador de erros é do thread e cada
//!   caso é um thread; o `setup` (`findSubstrCount = 0`) é a criação do contador. O serviço de extensões e o registro (criados
//!   dentro do `suiteSetup` no original, antes de o rastreador de vazamentos do `setup` ligar) são montados antes do guarda
//!   [`ensure_no_disposables_are_leaked_in_test_suite`], para não contarem como vazamento. O `teardown`
//!   (`Error.prepareStackTrace = originalPrepareStackTrace`, que era `undefined`) é o `Drop` do [`Fixture`].
//! - `new InstantiationService(collection, false)` e o `ServiceCollection` viram a struct [`ErrorHandlerServices`] (ver a nota
//!   de `extension_host_main.rs`). `mock<MainThreadExtensionServiceShape>() implements MainThreadErrorsShape` é a struct
//!   [`MainThreadMock`], que implementa as duas formas; os métodos que o `mock` deixaria indefinidos lançam, como a chamada de
//!   uma função inexistente. `getProxy` devolve o mesmo objeto para os dois identificadores.
//! - `class extends ExtensionPaths { override findSubstr }` é [`CountingExtensionPaths`] (o `ExtensionPaths` real mais o
//!   `findSubstr` sobrescrito, que conta e devolve a `nullExtensionDescription`), e `class extends ExtensionDescriptionRegistry
//!   { override getExtensionDescription }` é [`NullDescriptionRegistry`] (o registro real mais a sobrescrita).
//!   `NullApiDeprecationService` (de `extHostApiDeprecationService.ts`, sem porte) é [`NullApiDeprecationService`].
//! - `new Error()` é [`Fixture::new_error`], com dois quadros de pilha fixos (o primeiro com nome de arquivo, que dispara o
//!   `findSubstr`); `error.stack` é `JsError::stack`; `onUnexpectedError(err)` recebe `JsError::to_error`. `assert.ok(stack)` é a
//!   veracidade do `Dynamic`. O `(_, stack) => stack` do último caso devolve a lista de quadros em texto (um valor verdadeiro).
//! - `Error.prepareStackTrace = value` e `Error.prepareStackTrace` são `set_prepare_stack_trace` e `prepare_stack_trace`; o
//!   `do_something_else(original?.(...args))` do caso de embrulho passa `undefined` quando não há `original`.
//! - O `if (isFirefox || isSafari) return` do início da suíte não se aplica (nem Firefox nem Safari).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error, SerializedError};
use je_base::lifecycle::IDisposable;
use je_base::performance::PerformanceMark;
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_language_features_api::IExtHostApiDeprecationService;
use je_workbench::api::common::ext_host_protocol::{IMainContext, MainThreadErrorsShape, MainThreadExtensionServiceShape};
use je_workbench::api::common::extension_host_main::{
    ErrorConstructor, ErrorHandler, ErrorHandlerServices, ExtensionErrorReporter, ExtensionPathIndex, ExtensionPaths, IExtHostExtensionService, JsError,
    PrepareStackTrace, V8CallSite,
};
use je_workbench::services::extensions::extension_activation::{ExtensionActivationReason, MissingExtensionDependency};
use je_workbench::services::extensions::extension_description_registry::{
    ExtensionDescriptionRegistry, IActivationEventsReader, IReadOnlyExtensionDescriptionRegistry, SharedExtensionDescription,
};
use je_workbench::services::extensions::extensions::null_extension_description;

/// A chamada de uma função que o `mock` deixou indefinida.
fn not_implemented(name: &str) -> ! {
    errors::throw(Error::type_error(format!("{name} is not a function")))
}

/// `mock<MainThreadExtensionServiceShape>() implements MainThreadErrorsShape` com `$onExtensionRuntimeError` e
/// `$onUnexpectedError` vazios.
struct MainThreadMock;

impl IDisposable for MainThreadMock {
    fn dispose(&self) {}
}

impl MainThreadErrorsShape for MainThreadMock {
    fn rpc_on_unexpected_error(&self, _err: SerializedError) {}
}

impl MainThreadExtensionServiceShape for MainThreadMock {
    fn rpc_get_extension(&self, _extension_id: &str) -> Promise<Option<ExtensionDescription>> {
        not_implemented("$getExtension")
    }

    fn rpc_activate_extension(&self, _extension_id: &ExtensionIdentifier, _reason: &ExtensionActivationReason) -> Promise<()> {
        not_implemented("$activateExtension")
    }

    fn rpc_on_will_activate_extension(&self, _extension_id: &ExtensionIdentifier) -> Promise<()> {
        not_implemented("$onWillActivateExtension")
    }

    fn rpc_on_did_activate_extension(
        &self,
        _extension_id: &ExtensionIdentifier,
        _code_loading_time: f64,
        _activate_call_time: f64,
        _activate_resolved_time: f64,
        _activation_reason: &ExtensionActivationReason,
    ) {
        not_implemented("$onDidActivateExtension")
    }

    fn rpc_on_extension_activation_error(
        &self,
        _extension_id: &ExtensionIdentifier,
        _error: SerializedError,
        _missing_extension_dependency: Option<MissingExtensionDependency>,
    ) -> Promise<()> {
        not_implemented("$onExtensionActivationError")
    }

    fn rpc_on_extension_runtime_error(&self, _extension_id: &ExtensionIdentifier, _error: SerializedError) {}

    fn rpc_set_performance_marks(&self, _marks: Vec<PerformanceMark>) -> Promise<()> {
        not_implemented("$setPerformanceMarks")
    }

    fn rpc_as_browser_uri(&self, _uri: &URI) -> Promise<URI> {
        not_implemented("$asBrowserUri")
    }
}

/// `IExtHostRpcService` com o `getProxy` que devolve o mesmo objeto para qualquer identificador.
struct TestRpcService {
    proxy: Rc<MainThreadMock>,
}

impl IMainContext for TestRpcService {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "MainThreadErrors" => Rc::new(self.proxy.clone() as Rc<dyn MainThreadErrorsShape>),
            "MainThreadExtensionService" => Rc::new(self.proxy.clone() as Rc<dyn MainThreadExtensionServiceShape>),
            _ => not_implemented("getProxy"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {
        not_implemented("set")
    }
}

/// `IExtHostTelemetry` com `onExtensionError` que devolve `true`.
struct TestTelemetry;

impl ExtensionErrorReporter for TestTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        true
    }
}

/// `NullApiDeprecationService`.
struct NullApiDeprecationService;

impl IExtHostApiDeprecationService for NullApiDeprecationService {
    fn report(&self, _api_id: &str, _extension: &ExtensionDescription, _migration_suggestion: &str) {}
}

/// `new class extends ExtensionPaths { override findSubstr(key) { findSubstrCount++; return nullExtensionDescription; } }`.
struct CountingExtensionPaths {
    _paths: ExtensionPaths,
    find_substr_count: Rc<Cell<usize>>,
}

impl ExtensionPathIndex for CountingExtensionPaths {
    fn find_substr(&self, _key: &URI) -> Option<SharedExtensionDescription> {
        self.find_substr_count.set(self.find_substr_count.get() + 1);
        Some(Rc::new(null_extension_description()))
    }
}

/// `basicActivationEventsReader`: sempre vazio.
struct BasicActivationEventsReader;

impl IActivationEventsReader for BasicActivationEventsReader {
    fn read_activation_events(&self, _extension_description: &ExtensionDescription) -> Vec<String> {
        Vec::new()
    }
}

/// `new class extends ExtensionDescriptionRegistry { override getExtensionDescription(id) { return nullExtensionDescription; } }`.
struct NullDescriptionRegistry {
    inner: Rc<ExtensionDescriptionRegistry>,
}

impl IReadOnlyExtensionDescriptionRegistry for NullDescriptionRegistry {
    fn contains_activation_event(&self, activation_event: &str) -> bool {
        self.inner.contains_activation_event(activation_event)
    }

    fn contains_extension(&self, extension_id: &str) -> bool {
        self.inner.contains_extension(extension_id)
    }

    fn get_extension_descriptions_for_activation_event(&self, activation_event: &str) -> Vec<SharedExtensionDescription> {
        self.inner.get_extension_descriptions_for_activation_event(activation_event)
    }

    fn get_all_extension_descriptions(&self) -> Vec<SharedExtensionDescription> {
        self.inner.get_all_extension_descriptions()
    }

    fn get_extension_description(&self, _extension_id: &str) -> Option<SharedExtensionDescription> {
        Some(Rc::new(null_extension_description()))
    }

    fn get_extension_description_by_uuid(&self, uuid: &str) -> Option<SharedExtensionDescription> {
        self.inner.get_extension_description_by_uuid(uuid)
    }
}

/// `IExtHostExtensionService` do teste: `getExtensionPathIndex` e `getExtensionRegistry`.
struct TestExtensionService {
    registry: Rc<ExtensionDescriptionRegistry>,
    find_substr_count: Rc<Cell<usize>>,
}

impl IExtHostExtensionService for TestExtensionService {
    fn get_extension_registry(&self) -> Promise<Rc<dyn IReadOnlyExtensionDescriptionRegistry>> {
        Promise::resolved(Rc::new(NullDescriptionRegistry { inner: self.registry.clone() }) as Rc<dyn IReadOnlyExtensionDescriptionRegistry>)
    }

    fn get_extension_path_index(&self) -> Promise<Rc<dyn ExtensionPathIndex>> {
        Promise::resolved(
            Rc::new(CountingExtensionPaths { _paths: ExtensionPaths::new(extensions_index()), find_substr_count: self.find_substr_count.clone() })
                as Rc<dyn ExtensionPathIndex>,
        )
    }
}

/// `TernarySearchTree.forUris<IExtensionDescription>()`.
fn extensions_index() -> TernarySearchTree<URI, SharedExtensionDescription> {
    TernarySearchTree::for_uris(Rc::new(|_: &URI| false), Rc::new(|_: &URI| false))
}

/// Um quadro de pilha do V8.
struct TestCallSite {
    file_name: Option<String>,
    text: String,
}

impl V8CallSite for TestCallSite {
    fn get_type_name(&self) -> Option<String> {
        None
    }

    fn get_function_name(&self) -> Option<String> {
        None
    }

    fn get_method_name(&self) -> Option<String> {
        None
    }

    fn get_file_name(&self) -> Option<String> {
        self.file_name.clone()
    }

    fn get_line_number(&self) -> Option<f64> {
        None
    }

    fn get_column_number(&self) -> Option<f64> {
        None
    }

    fn get_eval_origin(&self) -> Option<String> {
        None
    }

    fn is_toplevel(&self) -> bool {
        false
    }

    fn is_eval(&self) -> bool {
        false
    }

    fn is_native(&self) -> bool {
        false
    }

    fn is_constructor(&self) -> bool {
        false
    }

    fn to_string(&self) -> String {
        self.text.clone()
    }
}

/// O `suiteSetup`, o `setup` e o `teardown` da suíte.
struct Fixture {
    error_constructor: Rc<ErrorConstructor>,
    find_substr_count: Rc<Cell<usize>>,
    _guard: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let error_constructor = ErrorConstructor::new();
        let find_substr_count = Rc::new(Cell::new(0));

        // O serviço de extensões e o registro nascem antes do guarda, como no `suiteSetup` do original.
        let services = ErrorHandlerServices {
            log_service: NullLogService::shared(),
            rpc_service: Rc::new(TestRpcService { proxy: Rc::new(MainThreadMock) }),
            extension_service: Rc::new(TestExtensionService {
                registry: ExtensionDescriptionRegistry::new(Rc::new(BasicActivationEventsReader), Vec::new()),
                find_substr_count: find_substr_count.clone(),
            }),
            extension_telemetry: Rc::new(TestTelemetry),
            api_deprecation_service: Rc::new(NullApiDeprecationService),
            error_constructor: error_constructor.clone(),
        };

        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_real_timers(async move {
            ErrorHandler::install_full_handler(services).await.unwrap();
        });

        Self { error_constructor, find_substr_count, _guard: guard }
    }

    /// `new Error(message)`.
    fn new_error(&self, message: &str) -> Rc<JsError> {
        let call_sites: Vec<Rc<dyn V8CallSite>> = vec![
            Rc::new(TestCallSite { file_name: Some("/ext/test/extension.js".to_owned()), text: "activate (/ext/test/extension.js:10:5)".to_owned() }),
            Rc::new(TestCallSite { file_name: None, text: "<anonymous>".to_owned() }),
        ];
        self.error_constructor.new_error("Error", message, call_sites)
    }

    fn find_substr_count(&self) -> usize {
        self.find_substr_count.get()
    }

    /// `for (let i = 0; i < n; ++i) { Error.prepareStackTrace = Error.prepareStackTrace; }`.
    fn reassign_prepare_stack_trace(&self, times: usize) {
        for _ in 0..times {
            self.error_constructor.set_prepare_stack_trace(self.error_constructor.prepare_stack_trace());
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // teardown: Error.prepareStackTrace = originalPrepareStackTrace (undefined)
        self.error_constructor.set_prepare_stack_trace(None);
    }
}

#[test]
fn basics() {
    let fx = Fixture::new();

    let err = fx.new_error("test1");

    errors::on_unexpected_error(&err.to_error());

    assert_eq!(fx.find_substr_count(), 1);
}

#[test]
fn set_reset_prepare_stack_trace_callback() {
    let fx = Fixture::new();

    let original = fx.error_constructor.prepare_stack_trace();
    fx.error_constructor.set_prepare_stack_trace(Some(PrepareStackTrace::new(|_error, _stack| Dynamic::string("stack"))));
    let probe_err = fx.new_error("");
    let stack = probe_err.stack();
    assert!(stack.is_truthy());
    fx.error_constructor.set_prepare_stack_trace(original);
    assert_eq!(fx.find_substr_count(), 1);

    // already checked
    errors::on_unexpected_error(&probe_err.to_error());
    assert_eq!(fx.find_substr_count(), 1);

    // one more error
    let err = fx.new_error("test2");
    errors::on_unexpected_error(&err.to_error());

    assert_eq!(fx.find_substr_count(), 2);
}

#[test]
fn wrap_prepare_stack_trace_callback() {
    let fx = Fixture::new();

    fn do_something_else(params: Dynamic) -> Dynamic {
        params
    }

    let original = fx.error_constructor.prepare_stack_trace();
    fx.error_constructor.set_prepare_stack_trace(Some(PrepareStackTrace::new(move |error, stack| {
        do_something_else(original.as_ref().map_or(Dynamic::Undefined, |original| original.call(error, stack)))
    })));
    let probe_err = fx.new_error("");
    let stack = probe_err.stack();
    assert!(stack.is_truthy());

    errors::on_unexpected_error(&probe_err.to_error());
    assert_eq!(fx.find_substr_count(), 1);
}

#[test]
fn prevent_rewrapping() {
    let fx = Fixture::new();

    let do_something_count = Rc::new(Cell::new(0));
    fx.error_constructor.set_prepare_stack_trace(Some(PrepareStackTrace::new({
        let do_something_count = do_something_count.clone();
        move |_result, _stack| {
            // do_something(stack)
            do_something_count.set(do_something_count.get() + 1);
            Dynamic::string("fakestack")
        }
    })));

    fx.reassign_prepare_stack_trace(2_500);

    let probe_err = fx.new_error("");
    let stack = probe_err.stack();
    assert_eq!(stack, Dynamic::string("fakestack"));

    errors::on_unexpected_error(&probe_err.to_error());
    assert_eq!(fx.find_substr_count(), 1);

    let probe_err2 = fx.new_error("");
    errors::on_unexpected_error(&probe_err2.to_error());
    assert_eq!(fx.find_substr_count(), 2);
    assert_eq!(do_something_count.get(), 2);
}

// https://gist.github.com/thecrypticace/f0f2e182082072efdaf0f8e1537d2cce

#[test]
fn gist_restored_separate_operations() {
    let fx = Fixture::new();

    // Actual Test
    // Operation 1
    let mut original = fx.error_constructor.prepare_stack_trace();
    fx.reassign_prepare_stack_trace(12_500);
    let err1 = fx.new_error("");
    assert!(err1.stack().is_truthy());
    assert_eq!(fx.find_substr_count(), 1);
    fx.error_constructor.set_prepare_stack_trace(original);

    // Operation 2
    original = fx.error_constructor.prepare_stack_trace();
    fx.reassign_prepare_stack_trace(12_500);
    assert!(fx.new_error("").stack().is_truthy());
    assert_eq!(fx.find_substr_count(), 2);
    fx.error_constructor.set_prepare_stack_trace(original);

    // Operation 3
    original = fx.error_constructor.prepare_stack_trace();
    fx.reassign_prepare_stack_trace(12_500);
    assert!(fx.new_error("").stack().is_truthy());
    assert_eq!(fx.find_substr_count(), 3);
    fx.error_constructor.set_prepare_stack_trace(original);

    // Operation 4
    original = fx.error_constructor.prepare_stack_trace();
    fx.reassign_prepare_stack_trace(12_500);
    assert!(fx.new_error("").stack().is_truthy());
    assert_eq!(fx.find_substr_count(), 4);
    fx.error_constructor.set_prepare_stack_trace(original);

    // Back to Operation 1
    assert!(err1.stack().is_truthy());
    assert_eq!(fx.find_substr_count(), 4);
}

#[test]
fn gist_never_restored_separate_operations() {
    let fx = Fixture::new();

    // Operation 1
    fx.reassign_prepare_stack_trace(12_500);
    assert!(fx.new_error("").stack().is_truthy());

    // Operation 2
    fx.reassign_prepare_stack_trace(12_500);
    assert!(fx.new_error("").stack().is_truthy());

    // Operation 3
    fx.reassign_prepare_stack_trace(12_500);
    assert!(fx.new_error("").stack().is_truthy());

    // Operation 4
    fx.reassign_prepare_stack_trace(12_500);
    assert!(fx.new_error("").stack().is_truthy());
}

#[test]
fn gist_restored_too_many_uses_before_restoration() {
    let fx = Fixture::new();

    let original = fx.error_constructor.prepare_stack_trace();
    fx.error_constructor.set_prepare_stack_trace(Some(PrepareStackTrace::new(|_error, stack| {
        Dynamic::array(stack.iter().map(|call| Dynamic::string(&call.to_string())))
    })));

    // Operation 1: more uses of `prepareStackTrace`
    fx.reassign_prepare_stack_trace(10_000);
    assert!(fx.new_error("").stack().is_truthy());

    fx.error_constructor.set_prepare_stack_trace(original);
}
