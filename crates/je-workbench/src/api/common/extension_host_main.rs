//! Porte de `src/vs/workbench/api/common/extensionHostMain.ts`: o `ErrorHandler` do host de extensões (o tratador de erros
//! não capturados, antecipado e o que conhece as extensões), mais o `ExtensionPaths` de `extHostExtensionService.ts` que ele
//! consulta.
//!
//! NÃO PORTADOS (e por quê):
//! - `ExtensionHostMain` (a classe), `IExitFn` e `IConsolePatchFn`: o construtor monta o `RPCProtocol`, a coleção de serviços
//!   do host (`IExtHostInitDataService`, `IHostUtils`, `IURITransformerService`) e transforma o `IExtensionHostInitData`, e
//!   nada disso existe no porte (o transporte do RPC entra na Fase 5). `asBrowserUri` e `terminate` dependem do mesmo
//!   `RPCProtocol` e do `IExtHostExtensionService` inteiro. O teste (`extensionHostMain.test.ts`) só usa o `ErrorHandler`.
//!
//! Divergências:
//! - **O motor JS.** O `ErrorHandler` do original age sobre três coisas do V8: `Error.stackTraceLimit`, a propriedade
//!   `Error.prepareStackTrace` (redefinida como acessora) e o `error.stack`, que o V8 monta uma vez, de forma preguiçosa, no
//!   primeiro acesso, chamando o `prepareStackTrace` vigente com os quadros capturados na criação do erro. Em Rust isso é o
//!   modelo [`ErrorConstructor`] (o construtor global `Error`), [`JsError`] (um erro com os quadros capturados e a pilha
//!   preguiçosa e em cache) e [`PrepareStackTrace`] (a função, com a marca `_wasWrapped`). A ponte com o zjsc, quando existir,
//!   liga o `Error` do motor a esse modelo. Quem cria o erro entrega os quadros ([`V8CallSite`]).
//! - **Identidade do erro.** O `WeakMap<Error, ...>` do original é indexado pelo identificador que o [`JsError`] carrega; o
//!   erro que circula pelo `onUnexpectedError` do `je-base` (o [`Error`] do porte, sem identidade) leva esse identificador na
//!   propriedade [`ENGINE_ERROR_ID_PROP`] ([`JsError::to_error`]), e o tratador volta ao [`JsError`] por ela. O `err.stack`
//!   que o `transformErrorForSerialization` lê é a pilha preguiçosa do [`JsError`] (o tratador a materializa no mesmo ponto em
//!   que o original a lê). O mapa guarda a entrada até o fim do tratador (não é fraco).
//! - **`err instanceof ExtensionError`** é o nome `ExtensionError` com a propriedade `extension` (o identificador); o
//!   `PendingMigrationError.is` é a conferência de nome, como o próprio original faz no segundo ramo.
//! - **Serviços**: `ServicesAccessor` vira os parâmetros explícitos de [`ErrorHandler::install_early_handler`] e a struct
//!   [`ErrorHandlerServices`] de [`ErrorHandler::install_full_handler`]. `IExtHostRpcService` é o [`IMainContext`];
//!   `IExtHostExtensionService` é o trait reduzido [`IExtHostExtensionService`] (só `getExtensionRegistry` e
//!   `getExtensionPathIndex`, o que o `ErrorHandler` lê; o serviço inteiro é de `extHostExtensionService.ts`);
//!   `IExtHostTelemetry` é o trait [`ExtensionErrorReporter`] (só `onExtensionError`), que o [`ExtHostTelemetry`] implementa;
//!   `getExtensionRegistry()` devolve o [`IReadOnlyExtensionDescriptionRegistry`] (quem sobrescreve um método, como um teste,
//!   implementa o trait). `static async` é uma [`Promise`] de função `async` (roda de forma síncrona até o primeiro `await`).
//! - `V8CallSite` (de `base/common/errors.ts`) não traz `getThis` e `getFunction` (devolvem `unknown` e `Function`, sem
//!   representação), e `URI.file(fileName)` usa a plataforma do processo.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};

use super::ext_host_language_features_api::IExtHostApiDeprecationService;
use super::ext_host_protocol::{IMainContext, MainContext};
use super::ext_host_telemetry::ExtHostTelemetry;
use crate::services::extensions::extension_description_registry::IReadOnlyExtensionDescriptionRegistry;

/// A propriedade do [`Error`] do porte que guarda o identificador do [`JsError`] de origem.
pub const ENGINE_ERROR_ID_PROP: &str = "$$jsErrorId";

/// O nome do `PendingMigrationError`.
const PENDING_MIGRATION_ERROR_NAME: &str = "PendingMigrationError";

/// O nome do `ExtensionError`.
const EXTENSION_ERROR_NAME: &str = "ExtensionError";

/// A propriedade do [`Error`] do porte que guarda o `extension` de um `ExtensionError`.
pub const EXTENSION_ERROR_EXTENSION_PROP: &str = "extension";

/// `V8CallSite`, ver https://github.com/v8/v8/wiki/Stack%20Trace%20API#basic-stack-traces
pub trait V8CallSite {
    fn get_type_name(&self) -> Option<String>;
    fn get_function_name(&self) -> Option<String>;
    fn get_method_name(&self) -> Option<String>;
    fn get_file_name(&self) -> Option<String>;
    fn get_line_number(&self) -> Option<f64>;
    fn get_column_number(&self) -> Option<f64>;
    fn get_eval_origin(&self) -> Option<String>;
    fn is_toplevel(&self) -> bool;
    fn is_eval(&self) -> bool;
    fn is_native(&self) -> bool;
    fn is_constructor(&self) -> bool;
    /// `toString()`.
    fn to_string(&self) -> String;
}

/// A função de `Error.prepareStackTrace`: recebe o erro e os quadros e devolve a pilha (qualquer valor).
pub type PrepareStackTraceFn = Rc<dyn Fn(&JsError, &[Rc<dyn V8CallSite>]) -> Dynamic>;

/// Um valor de `Error.prepareStackTrace`: a função e a marca `_wasWrapped` que o `ErrorHandler` pendura nas funções que
/// ele próprio embrulhou.
#[derive(Clone)]
pub struct PrepareStackTrace {
    function: PrepareStackTraceFn,
    was_wrapped: bool,
}

impl PrepareStackTrace {
    pub fn new(function: impl Fn(&JsError, &[Rc<dyn V8CallSite>]) -> Dynamic + 'static) -> Self {
        Self { function: Rc::new(function), was_wrapped: false }
    }

    /// `v.call(Error, error, stackTrace)`.
    pub fn call(&self, error: &JsError, stack_trace: &[Rc<dyn V8CallSite>]) -> Dynamic {
        (self.function)(error, stack_trace)
    }

    /// `a === b`: a mesma função.
    pub fn is_same_function(&self, other: &PrepareStackTrace) -> bool {
        Rc::ptr_eq(&self.function, &other.function)
    }

    /// `v[_wasWrapped]`.
    pub fn was_wrapped(&self) -> bool {
        self.was_wrapped
    }
}

/// O par `get`/`set` de uma propriedade acessora `prepareStackTrace` (o `Object.defineProperty` do original).
pub struct PrepareStackTraceAccessor {
    pub get: Box<dyn Fn() -> Option<PrepareStackTrace>>,
    pub set: Box<dyn Fn(Option<PrepareStackTrace>)>,
}

enum PrepareStackTraceSlot {
    /// A propriedade de dados padrão (`undefined` até alguém definir).
    Data(Option<PrepareStackTrace>),
    Accessor(Rc<PrepareStackTraceAccessor>),
}

/// O construtor global `Error` do motor: `stackTraceLimit` e a propriedade `prepareStackTrace`.
pub struct ErrorConstructor {
    stack_trace_limit: Cell<usize>,
    prepare_stack_trace: RefCell<PrepareStackTraceSlot>,
    errors: RefCell<HashMap<u64, Weak<JsError>>>,
    next_error_id: Cell<u64>,
}

impl ErrorConstructor {
    /// O `Error` de um motor recém-criado: `stackTraceLimit` 10 (o padrão do V8) e `prepareStackTrace` indefinido.
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            stack_trace_limit: Cell::new(10),
            prepare_stack_trace: RefCell::new(PrepareStackTraceSlot::Data(None)),
            errors: RefCell::new(HashMap::new()),
            next_error_id: Cell::new(0),
        })
    }

    /// `Error.stackTraceLimit`.
    pub fn stack_trace_limit(&self) -> usize {
        self.stack_trace_limit.get()
    }

    /// `Error.stackTraceLimit = limit`.
    pub fn set_stack_trace_limit(&self, limit: usize) {
        self.stack_trace_limit.set(limit);
    }

    /// `Error.prepareStackTrace` (o `get`).
    pub fn prepare_stack_trace(&self) -> Option<PrepareStackTrace> {
        let accessor = match &*self.prepare_stack_trace.borrow() {
            PrepareStackTraceSlot::Data(value) => return value.clone(),
            PrepareStackTraceSlot::Accessor(accessor) => accessor.clone(),
        };
        (accessor.get)()
    }

    /// `Error.prepareStackTrace = value` (o `set`).
    pub fn set_prepare_stack_trace(&self, value: Option<PrepareStackTrace>) {
        let accessor = match &mut *self.prepare_stack_trace.borrow_mut() {
            PrepareStackTraceSlot::Data(slot) => {
                *slot = value;
                return;
            }
            PrepareStackTraceSlot::Accessor(accessor) => accessor.clone(),
        };
        (accessor.set)(value);
    }

    /// `Object.defineProperty(Error, 'prepareStackTrace', { configurable: false, get, set })`: a propriedade acessora não
    /// pode ser redefinida (o `TypeError` do motor é o `Err`).
    pub fn define_prepare_stack_trace_accessor(&self, accessor: PrepareStackTraceAccessor) -> Result<(), Error> {
        let mut slot = self.prepare_stack_trace.borrow_mut();
        if matches!(&*slot, PrepareStackTraceSlot::Accessor(_)) {
            return Err(Error::type_error("Cannot redefine property: prepareStackTrace"));
        }
        *slot = PrepareStackTraceSlot::Accessor(Rc::new(accessor));
        Ok(())
    }

    /// `new Error(message)` com `name` e os quadros capturados (limitados por `stackTraceLimit`, como o V8).
    pub fn new_error(self: &Rc<Self>, name: &str, message: &str, call_sites: Vec<Rc<dyn V8CallSite>>) -> Rc<JsError> {
        let id = self.next_error_id.get() + 1;
        self.next_error_id.set(id);
        let call_sites: Vec<Rc<dyn V8CallSite>> = call_sites.into_iter().take(self.stack_trace_limit.get()).collect();
        let error = Rc::new(JsError {
            id,
            name: name.to_owned(),
            message: message.to_owned(),
            call_sites,
            constructor: self.clone(),
            stack: RefCell::new(None),
        });
        self.errors.borrow_mut().insert(id, Rc::downgrade(&error));
        error
    }

    /// O erro do motor com o identificador `id` (o inverso de [`JsError::to_error`]).
    pub fn error_by_id(&self, id: u64) -> Option<Rc<JsError>> {
        self.errors.borrow().get(&id).and_then(Weak::upgrade)
    }

    /// O `err.stack` do tratador: um erro do motor traz a pilha preguiçosa (montada agora, se ainda não foi); qualquer outro
    /// erro traz a que já tem.
    fn with_current_stack(&self, err: &Error) -> Error {
        let mut result = err.clone();
        if let Some(js_error) = engine_error_of(self, err)
            && let Dynamic::String(stack) = js_error.stack()
        {
            result.stack = Some(stack.to_string_lossy());
        }
        result
    }
}

/// O [`JsError`] de origem de um [`Error`] do porte, quando ele veio do motor.
fn engine_error_of(constructor: &ErrorConstructor, err: &Error) -> Option<Rc<JsError>> {
    let id = err.prop(ENGINE_ERROR_ID_PROP)?.parse::<u64>().ok()?;
    constructor.error_by_id(id)
}

/// Um `Error` do motor: nome, mensagem, os quadros capturados na criação e a pilha, que o V8 monta no primeiro acesso e
/// guarda.
pub struct JsError {
    id: u64,
    name: String,
    message: String,
    call_sites: Vec<Rc<dyn V8CallSite>>,
    constructor: Rc<ErrorConstructor>,
    stack: RefCell<Option<Dynamic>>,
}

impl JsError {
    /// O identificador do erro (a chave do `WeakMap`).
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// `error.stack`: no primeiro acesso chama o `Error.prepareStackTrace` vigente (ou, sem ele, monta o texto padrão do V8) e
    /// guarda o resultado; os acessos seguintes devolvem o mesmo valor.
    pub fn stack(&self) -> Dynamic {
        let cached = self.stack.borrow().clone();
        if let Some(cached) = cached {
            return cached;
        }
        let value = match self.constructor.prepare_stack_trace() {
            Some(prepare) => prepare.call(self, &self.call_sites),
            None => {
                let frames: String = self.call_sites.iter().map(|call| format!("\n    at {}", call.to_string())).collect();
                Dynamic::string(&format!("{}: {}{}", self.name, self.message, frames))
            }
        };
        *self.stack.borrow_mut() = Some(value.clone());
        value
    }

    /// O [`Error`] do porte que representa este erro nos tratadores do `je-base`, com o identificador em
    /// [`ENGINE_ERROR_ID_PROP`].
    pub fn to_error(&self) -> Error {
        Error::named(self.name.clone(), self.message.clone()).with_prop(ENGINE_ERROR_ID_PROP, self.id)
    }
}

/// O índice de caminhos das extensões que o `ErrorHandler` consulta (o `findSubstr` da classe `ExtensionPaths`, que um
/// teste sobrescreve).
pub trait ExtensionPathIndex {
    /// `findSubstr(key)`.
    fn find_substr(&self, key: &URI) -> Option<Rc<ExtensionDescription>>;
}

/// `ExtensionPaths`.
pub struct ExtensionPaths {
    search_tree: RefCell<TernarySearchTree<URI, Rc<ExtensionDescription>>>,
}

impl ExtensionPaths {
    pub fn new(search_tree: TernarySearchTree<URI, Rc<ExtensionDescription>>) -> Self {
        Self { search_tree: RefCell::new(search_tree) }
    }

    /// `setSearchTree(searchTree)`.
    pub fn set_search_tree(&self, search_tree: TernarySearchTree<URI, Rc<ExtensionDescription>>) {
        *self.search_tree.borrow_mut() = search_tree;
    }

    /// `forEach(callback)`: o callback recebe o valor e a chave.
    pub fn for_each(&self, callback: impl FnMut(&Rc<ExtensionDescription>, &URI)) {
        self.search_tree.borrow().for_each(callback);
    }
}

impl ExtensionPathIndex for ExtensionPaths {
    fn find_substr(&self, key: &URI) -> Option<Rc<ExtensionDescription>> {
        self.search_tree.borrow().find_substr(key).cloned()
    }
}

/// O que o `ErrorHandler` lê de `IExtHostExtensionService` (ver a nota do módulo).
pub trait IExtHostExtensionService {
    /// `getExtensionRegistry()`.
    fn get_extension_registry(&self) -> Promise<Rc<dyn IReadOnlyExtensionDescriptionRegistry>>;

    /// `getExtensionPathIndex()`.
    fn get_extension_path_index(&self) -> Promise<Rc<dyn ExtensionPathIndex>>;
}

/// O que o `ErrorHandler` lê de `IExtHostTelemetry`.
pub trait ExtensionErrorReporter {
    /// `onExtensionError(extension, error)`: se o erro foi encaminhado.
    fn on_extension_error(&self, extension: &ExtensionIdentifier, error: &Error) -> bool;
}

impl ExtensionErrorReporter for ExtHostTelemetry {
    fn on_extension_error(&self, extension: &ExtensionIdentifier, error: &Error) -> bool {
        ExtHostTelemetry::on_extension_error(self, extension, error)
    }
}

/// Os serviços que `ErrorHandler.installFullHandler` pega do `accessor`, mais o construtor `Error` do motor.
pub struct ErrorHandlerServices {
    pub log_service: LogServiceRef,
    pub rpc_service: Rc<dyn IMainContext>,
    pub extension_service: Rc<dyn IExtHostExtensionService>,
    pub extension_telemetry: Rc<dyn ExtensionErrorReporter>,
    pub api_deprecation_service: Rc<dyn IExtHostApiDeprecationService>,
    pub error_constructor: Rc<ErrorConstructor>,
}

/// O que o `ErrorHandler` guarda por erro: a extensão dona (se achou) e a pilha formatada.
struct ExtensionErrorData {
    extension_identifier: Option<ExtensionIdentifier>,
    stack: String,
}

/// `PendingMigrationError.is(error)`.
fn is_pending_migration_error(error: &Error) -> bool {
    error.name == PENDING_MIGRATION_ERROR_NAME
}

/// A plataforma do processo (o `isWindows` do `URI.file`).
fn host_platform() -> Platform {
    if cfg!(windows) { Platform::Win32 } else { Platform::Posix }
}

/// `ErrorHandler`.
pub struct ErrorHandler;

impl ErrorHandler {
    /// `installEarlyHandler(accessor)`.
    pub fn install_early_handler(error_constructor: &Rc<ErrorConstructor>, log_service: LogServiceRef, rpc_service: &Rc<dyn IMainContext>) -> Promise<()> {
        // increase number of stack frames (from 10, https://github.com/v8/v8/wiki/Stack-Trace-API)
        error_constructor.set_stack_trace_limit(100);

        // does NOT dependent of extension information, can be installed immediately, and simply forwards
        // to the log service and main thread errors
        let main_thread_errors = rpc_service.get_proxy(&MainContext::MAIN_THREAD_ERRORS);
        let error_constructor = error_constructor.clone();

        errors::set_unexpected_error_handler(move |err| {
            log_service.error_args(&LogMessage::from(err), &[]);
            let data = errors::transform_error_for_serialization(&error_constructor.with_current_stack(err));
            main_thread_errors.rpc_on_unexpected_error(data);
        });
        Promise::resolved(())
    }

    /// `installFullHandler(accessor)`.
    pub fn install_full_handler(services: ErrorHandlerServices) -> Promise<()> {
        async_fn(async move {
            // uses extension knowledges to correlate errors with extensions
            let ErrorHandlerServices { log_service, rpc_service, extension_service, extension_telemetry, api_deprecation_service, error_constructor } =
                services;

            let main_thread_extensions = rpc_service.get_proxy(&MainContext::MAIN_THREAD_EXTENSION_SERVICE);
            let main_thread_errors = rpc_service.get_proxy(&MainContext::MAIN_THREAD_ERRORS);

            let extensions_registry = extension_service.get_extension_registry().await?;
            let extensions_map = extension_service.get_extension_path_index().await?;
            let extension_errors: Rc<RefCell<HashMap<u64, ExtensionErrorData>>> = Rc::default();

            // PART 1
            // set the prepareStackTrace-handle and use it as a side-effect to associate errors
            // with extensions - this works by looking up callsites in the extension path index
            let prepare_stack_trace_and_find_extension = {
                let extension_errors = extension_errors.clone();
                PrepareStackTrace::new(move |error: &JsError, stack_trace: &[Rc<dyn V8CallSite>]| {
                    if let Some(data) = extension_errors.borrow().get(&error.id()) {
                        return Dynamic::string(&data.stack);
                    }
                    let mut stack_trace_message = String::new();
                    let mut extension: Option<Rc<ExtensionDescription>> = None;
                    for call in stack_trace {
                        stack_trace_message.push_str(&format!("\n\tat {}", call.to_string()));
                        let file_name = call.get_file_name();
                        if extension.is_none()
                            && let Some(file_name) = file_name.filter(|file_name| !file_name.is_empty())
                        {
                            extension = extensions_map.find_substr(&URI::file(file_name.as_str(), host_platform()));
                        }
                    }
                    let name = if error.name().is_empty() { "Error" } else { error.name() };
                    let result = format!("{name}: {}{stack_trace_message}", error.message());
                    extension_errors
                        .borrow_mut()
                        .insert(error.id(), ExtensionErrorData { extension_identifier: extension.map(|ext| ext.identifier.clone()), stack: result.clone() });
                    Dynamic::string(&result)
                })
            };

            let current: Rc<RefCell<PrepareStackTrace>> = Rc::new(RefCell::new(prepare_stack_trace_and_find_extension.clone()));
            let accessor = PrepareStackTraceAccessor {
                get: Box::new({
                    let current = current.clone();
                    move || Some(current.borrow().clone())
                }),
                set: Box::new({
                    let current = current.clone();
                    let base = prepare_stack_trace_and_find_extension;
                    move |value: Option<PrepareStackTrace>| {
                        let Some(value) = value else {
                            *current.borrow_mut() = base.clone();
                            return;
                        };
                        if value.is_same_function(&base) || value.was_wrapped() {
                            *current.borrow_mut() = value;
                            return;
                        }

                        let wrapped_base = base.clone();
                        *current.borrow_mut() = PrepareStackTrace {
                            function: Rc::new(move |error: &JsError, stack_trace: &[Rc<dyn V8CallSite>]| {
                                wrapped_base.call(error, stack_trace);
                                value.call(error, stack_trace)
                            }),
                            was_wrapped: true,
                        };
                    }
                }),
            };
            error_constructor.define_prepare_stack_trace_accessor(accessor)?;

            // PART 2
            // set the unexpectedErrorHandler and check for extensions that have been identified as
            // having caused the error. Note that the runtime order is actually reversed, the code
            // below accesses the stack-property which triggers the code above
            {
                let log_service = log_service.clone();
                let error_constructor = error_constructor.clone();
                let main_thread_extensions = main_thread_extensions.clone();
                errors::set_unexpected_error_handler(move |err| {
                    if !is_pending_migration_error(err) {
                        log_service.error_args(&LogMessage::from(err), &[]);
                    }

                    let err_with_stack = error_constructor.with_current_stack(err);
                    let error_data = errors::transform_error_for_serialization(&err_with_stack);

                    let extension: Option<ExtensionIdentifier> = match err.prop(EXTENSION_ERROR_EXTENSION_PROP) {
                        Some(extension) if err.name == EXTENSION_ERROR_NAME => Some(ExtensionIdentifier::new(extension)),
                        _ => engine_error_of(&error_constructor, err)
                            .and_then(|js_error| extension_errors.borrow().get(&js_error.id()).and_then(|data| data.extension_identifier.clone())),
                    };

                    let Some(extension) = extension else {
                        return;
                    };

                    if is_pending_migration_error(err) {
                        // report pending migration via the API deprecation service which (1) informs the extensions author during
                        // dev-time and (2) collects telemetry so that we can reach out too
                        if let Some(extension_desc) = extensions_registry.get_extension_description(&extension.value) {
                            let stack = err_with_stack.stack.as_deref().unwrap_or("undefined");
                            api_deprecation_service.report(&err.name, &extension_desc, &format!("{}\n FROM: {}", err.message, stack));
                        }
                    } else {
                        main_thread_extensions.rpc_on_extension_runtime_error(&extension, error_data);
                        let reported = extension_telemetry.on_extension_error(&extension, err);
                        log_service.trace_args(
                            "forwarded error to extension?",
                            &[LogArg::Value(Dynamic::Bool(reported)), LogArg::Value(Dynamic::string(&extension.value))],
                        );
                    }
                });
            }

            let listener_constructor = error_constructor.clone();
            // O ouvinte nunca é removido no original: a função de remoção devolvida é descartada sem ser chamada.
            drop(errors::add_error_listener(move |err| {
                main_thread_errors.rpc_on_unexpected_error(errors::transform_error_for_serialization(&listener_constructor.with_current_stack(err)));
            }));
            Ok(())
        })
    }
}
