//! Porte de `src/vs/platform/telemetry/test/browser/telemetryService.test.ts` (suíte `TelemetryService`), caso a
//! caso e na mesma ordem.
//!
//! Divergências:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; o
//!   `TestConfigurationService` tem o emissor entregue ao guarda, como o `githubEnterprise.test.ts` faz.
//! - `sinonTestFn` (relógio falso do sinon, `this.clock.tick(ErrorTelemetry.ERROR_FLUSH_TIMEOUT)`) é
//!   `runtime::with_faked_timers` e `runtime::advance_fake_time`.
//! - `mainWindow.onerror` é o [`WindowErrorHandler`] de cada caso (o hospedeiro o aciona com `report`): cada caso
//!   tem o seu, então o `setup`/`teardown` que salva e restaura `mainWindow.onerror` não tem par. O mesmo vale para
//!   `Errors.setUnexpectedErrorHandler(() => {})` e a restauração do `finally`: o tratador é por thread e cada caso
//!   roda na sua, então só o silêncio é instalado. `sinon.stub()` é um tratador que registra as chamadas.
//! - `new Error(msg)` com `error.stack = ...` é `Error::new(msg).with_stack(...)`; a pilha em array
//!   (`settings.stack`) é a mesma lista unida por `\n` (a gambiarra de `Array.isArray(err.stack)` não existe no
//!   `Error` do porte). Onde o JavaScript tem uma pilha real (`new Error('test')`), o `Error` não tem pilha, e o
//!   caso que precisa de uma (`Error events`) a define, pelo mesmo ramo que o original reserva ao Phantom.
//! - `publicLog(name, data)` com literal é `public_log` com [`ITelemetryData`] (os campos livres em `extra`).
//! - `TestProductService` é o [`ProductService`] padrão (o `product.json` do VS Code estável).
//! - O `getValue` sobrescrito de `Telemetry Service checks with config service` (devolve o nível para toda chave) é o
//!   `TestConfigurationService` com a chave `telemetry.telemetryLevel` definida: as outras duas chaves lidas
//!   (`enableCrashReporter` e `enableTelemetry`) ficam indefinidas, e o resultado é o mesmo. O `emitter.fire({
//!   affectsConfiguration: () => true })` é a mudança da chave `telemetry.telemetryLevel`.
//! - `Math.random() % 2 === 0` do getter `bar` é um booleano fixo: o caso só confere o tipo.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::*;
use je_base::actions::ITelemetryData;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::product::{IProductService, ProductService};
use je_platform::telemetry::error_telemetry::{ErrorTelemetry, OnErrorHandler, WindowErrorHandler};
use je_platform::telemetry::{
    CommonPropertyValue, ITelemetryAppender, ITelemetryService, NullAppender, TelemetryConfiguration, TelemetryLevel, TelemetryService, TelemetryServiceConfig,
};

/// `TestTelemetryAppender`.
struct TestTelemetryAppender {
    events: RefCell<Vec<(String, Dynamic)>>,
    is_disposed: Cell<bool>,
}

impl TestTelemetryAppender {
    fn new() -> Rc<Self> {
        Rc::new(Self { events: RefCell::new(Vec::new()), is_disposed: Cell::new(false) })
    }

    fn get_events_count(&self) -> usize {
        self.events.borrow().len()
    }

    fn event_names(&self) -> Vec<String> {
        self.events.borrow().iter().map(|(name, _)| name.clone()).collect()
    }

    /// `events[index].data`.
    fn data(&self, index: usize) -> Dynamic {
        self.events.borrow()[index].1.clone()
    }

    /// `events[index].data[field]`, que é um texto.
    fn text(&self, index: usize, field: &str) -> String {
        text_of(&self.data(index).get_str(field))
    }
}

impl ITelemetryAppender for TestTelemetryAppender {
    fn log(&self, event_name: &str, data: &Dynamic) {
        self.events.borrow_mut().push((event_name.to_owned(), data.clone()));
    }

    fn flush(&self) -> Promise<()> {
        self.is_disposed.set(true);
        Promise::resolved(())
    }
}

fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("o valor não é texto: {other:?}"),
    }
}

fn appenders(appender: &Rc<TestTelemetryAppender>) -> Vec<Rc<dyn ITelemetryAppender>> {
    let appender: Rc<dyn ITelemetryAppender> = appender.clone();
    vec![appender]
}

/// `TestProductService`.
fn product() -> Rc<dyn IProductService> {
    Rc::new(ProductService::default())
}

/// `new TestConfigurationService()`, com o emissor entregue ao guarda.
fn configuration(guard: &LeakGuard) -> Rc<TestConfigurationService> {
    let service = TestConfigurationService::new(None);
    guard.add_rc(service.on_did_change_configuration_emitter.clone());
    service
}

/// `new TelemetryService(config, new TestConfigurationService(), TestProductService)`.
fn telemetry_service(guard: &LeakGuard, config: TelemetryServiceConfig) -> Rc<TelemetryService> {
    let configuration: Rc<dyn IConfigurationService> = self::configuration(guard);
    TelemetryService::new(config, Some(configuration), product()).expect("há serviço de configuração")
}

/// `TestErrorTelemetryService`: o serviço com `sendErrorTelemetry` ligado.
fn error_telemetry_service(guard: &LeakGuard, config: TelemetryServiceConfig) -> Rc<TelemetryService> {
    telemetry_service(guard, TelemetryServiceConfig { send_error_telemetry: true, ..config })
}

/// Um serviço só com o appender dado.
fn service_with(guard: &LeakGuard, appender: &Rc<TestTelemetryAppender>) -> Rc<TelemetryService> {
    telemetry_service(guard, TelemetryServiceConfig { appenders: appenders(appender), ..TelemetryServiceConfig::default() })
}

/// O literal `{ chave: valor, ... }` de dados de um evento.
fn event_data(entries: Vec<(&str, Dynamic)>) -> ITelemetryData {
    ITelemetryData { extra: entries.into_iter().map(|(name, value)| (name.to_owned(), value)).collect(), ..ITelemetryData::default() }
}

/// `emitter.fire({ affectsConfiguration: ... })` da chave dada.
fn fire_config_change(config: &TestConfigurationService, key: &str) {
    let event = ConfigurationChangeEvent::new(change(&[key], &[]), None, Rc::new(empty_test_configuration()), None, null_log());
    event.source.set(Some(ConfigurationTarget::User));
    config.on_did_change_configuration_emitter.fire(Rc::new(event));
}

/// `Errors.setUnexpectedErrorHandler(() => { })`.
fn silence_unexpected_errors() {
    errors::set_unexpected_error_handler(|_| {});
}

/// `this.clock.tick(ErrorTelemetry.ERROR_FLUSH_TIMEOUT)`.
fn tick() {
    runtime::advance_fake_time(ErrorTelemetry::ERROR_FLUSH_TIMEOUT);
}

/// Uma chamada ao tratador de `mainWindow.onerror`.
type OnErrorCall = (String, Option<String>, Option<u32>, Option<u32>, Option<Error>);

/// `sinon.stub()` de `mainWindow.onerror`: o tratador e a lista das chamadas que recebeu.
fn error_stub() -> (OnErrorHandler, Rc<RefCell<Vec<OnErrorCall>>>) {
    let calls: Rc<RefCell<Vec<OnErrorCall>>> = Rc::new(RefCell::new(Vec::new()));
    let recorded = calls.clone();
    let handler: OnErrorHandler = Rc::new(move |message: &str, file: Option<&str>, line: Option<u32>, column: Option<u32>, error: Option<&Error>| {
        recorded.borrow_mut().push((message.to_owned(), file.map(str::to_owned), line, column, error.cloned()));
    });
    (handler, calls)
}

/// O serviço de erros, o coletor e a janela de um caso de erro.
struct ErrorHarness {
    appender: Rc<TestTelemetryAppender>,
    service: Rc<TelemetryService>,
    window: Rc<WindowErrorHandler>,
    error_telemetry: ErrorTelemetry,
}

impl ErrorHarness {
    /// `mainWindow.onerror = stub` (se há), `new TestErrorTelemetryService({ appenders, piiPaths })` e
    /// `new ErrorTelemetry(service)`.
    fn new(guard: &LeakGuard, pii_paths: &[&str], stub: Option<OnErrorHandler>) -> Self {
        let window = WindowErrorHandler::new();
        window.set_on_error(stub);
        let appender = TestTelemetryAppender::new();
        let config = TelemetryServiceConfig { appenders: appenders(&appender), pii_paths: pii_paths.iter().map(|path| (*path).to_owned()).collect(), ..TelemetryServiceConfig::default() };
        let service = error_telemetry_service(guard, config);
        let error_telemetry = ErrorTelemetry::new(service.clone(), &window);
        Self { appender, service, window, error_telemetry }
    }

    /// `errorTelemetry.dispose(); service.dispose();`.
    fn dispose(&self) {
        self.error_telemetry.dispose();
        self.service.dispose();
    }
}

/// `ErrorTestingSettings`.
struct ErrorTestingSettings {
    personal_info: String,
    important_info: String,
    file_prefix: String,
    dangerous_path_without_important_info: String,
    dangerous_path_with_important_info: String,
    missing_model_prefix: String,
    missing_model_message: String,
    no_such_file_prefix: String,
    no_such_file_message: String,
    stack: Vec<String>,
    random_user_file: String,
    anonymized_random_user_file: String,
    anonymized_node_module_path: String,
    anonymized_node_module_asar_path: String,
    anonymized_full_node_module_path: String,
    anonymized_full_node_module_asar_path: String,
    extension_path_to_retain: String,
    anonymized_extension_path: String,
    server_insiders_extension_path_to_retain: String,
    anonymized_server_insiders_extension_path: String,
    builtin_extension_path_to_retain: String,
    anonymized_builtin_extension_path: String,
}

impl ErrorTestingSettings {
    fn new() -> Self {
        let random_user_file = "a/path/that/doe_snt/con-tain/code/names.js";
        let node_module_path_to_retain = "node_modules/path/that/shouldbe/retained/names.js:14:15854";
        let node_module_asar_path_to_retain = "node_modules.asar/path/that/shouldbe/retained/names.js:14:12354";
        let full_node_module_path = "/Users/username/projects/vscode/node_modules/@xterm/xterm/lib/xterm.js:1:243732";
        let full_node_module_asar_path = "/Users/username/projects/vscode/node_modules.asar/@xterm/xterm/lib/xterm.js:1:376066";
        let extension_path_to_retain = ".vscode/extensions/ms-python.python-2024.0.1/out/extension.js:144:145516";
        let full_extension_path = "/Users/username/.vscode/extensions/ms-python.python-2024.0.1/out/extension.js:144:145516";
        let server_insiders_extension_path_to_retain = ".vscode-server-insiders/extensions/ms-vscode.remote-server-2024.1.0/out/server.js:99:8888";
        let full_server_insiders_extension_path = "/home/user/.vscode-server-insiders/extensions/ms-vscode.remote-server-2024.1.0/out/server.js:99:8888";
        let builtin_extension_path_to_retain = "Resources/app/extensions/git/out/git.js:42:1234";
        let full_builtin_extension_path = "/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/git/out/git.js:42:1234";

        let personal_info = "DANGEROUS/PATH";
        let important_info = "important/information";
        let file_prefix = "file:///";
        let dangerous_path_with_important_info = format!("{file_prefix}{personal_info}/resources/app/{important_info}");
        let dangerous_path_without_important_info = format!("{file_prefix}{personal_info}");

        let missing_model_prefix = "Received model events for missing model ";
        let missing_model_message = format!("{missing_model_prefix} {dangerous_path_without_important_info}");

        let no_such_file_prefix = "ENOENT: no such file or directory";
        let no_such_file_message = format!("{no_such_file_prefix} '{personal_info}'");

        let stack = vec![
            format!("at e._modelEvents ({random_user_file}:11:7309)"),
            format!("    at t.AllWorkers ({random_user_file}:6:8844)"),
            format!("    at e.(anonymous function) [as _modelEvents] ({random_user_file}:5:29552)"),
            format!("    at Function.<anonymous> ({random_user_file}:6:8272)"),
            format!("    at e.dispatch ({random_user_file}:5:26931)"),
            format!("    at e.request (/{node_module_asar_path_to_retain})"),
            format!("    at t._handleMessage ({node_module_asar_path_to_retain})"),
            format!("    at t._onmessage (/{node_module_path_to_retain})"),
            format!("    at t.onmessage ({node_module_path_to_retain})"),
            format!("    at get dimensions ({full_node_module_path})"),
            format!("    at _._refreshCanvasDimensions ({full_node_module_asar_path})"),
            format!("    at uv.provideCodeActions ({full_extension_path})"),
            format!("    at remote.handleConnection ({full_server_insiders_extension_path})"),
            format!("    at git.getRepositoryState ({full_builtin_extension_path})"),
            "    at DedicatedWorkerGlobalScope.self.onmessage".to_owned(),
            dangerous_path_with_important_info.clone(),
            dangerous_path_without_important_info.clone(),
            missing_model_message.clone(),
            no_such_file_message.clone(),
        ];

        Self {
            personal_info: personal_info.to_owned(),
            important_info: important_info.to_owned(),
            file_prefix: file_prefix.to_owned(),
            dangerous_path_without_important_info,
            dangerous_path_with_important_info,
            missing_model_prefix: missing_model_prefix.to_owned(),
            missing_model_message,
            no_such_file_prefix: no_such_file_prefix.to_owned(),
            no_such_file_message,
            stack,
            random_user_file: random_user_file.to_owned(),
            anonymized_random_user_file: "<REDACTED: user-file-path>".to_owned(),
            anonymized_node_module_path: "<REDACTED: user-file-path>/node_modules/path/that/shouldbe/retained/names.js:14:15854".to_owned(),
            anonymized_node_module_asar_path: "<REDACTED: user-file-path>/node_modules.asar/path/that/shouldbe/retained/names.js:14:12354".to_owned(),
            anonymized_full_node_module_path: "<REDACTED: user-file-path>/node_modules/@xterm/xterm/lib/xterm.js:1:243732".to_owned(),
            anonymized_full_node_module_asar_path: "<REDACTED: user-file-path>/node_modules.asar/@xterm/xterm/lib/xterm.js:1:376066".to_owned(),
            extension_path_to_retain: extension_path_to_retain.to_owned(),
            anonymized_extension_path: "<REDACTED: user-file-path>/.vscode/extensions/ms-python.python-2024.0.1/out/extension.js:144:145516".to_owned(),
            server_insiders_extension_path_to_retain: server_insiders_extension_path_to_retain.to_owned(),
            anonymized_server_insiders_extension_path: "<REDACTED: user-file-path>/.vscode-server-insiders/extensions/ms-vscode.remote-server-2024.1.0/out/server.js:99:8888".to_owned(),
            builtin_extension_path_to_retain: builtin_extension_path_to_retain.to_owned(),
            anonymized_builtin_extension_path: "<REDACTED: user-file-path>/Resources/app/extensions/git/out/git.js:42:1234".to_owned(),
        }
    }

    /// `error.stack = settings.stack`.
    fn error(&self, message: &str) -> Error {
        Error::new(message).with_stack(self.stack.join("\n"))
    }

    /// `settings.stack[4].replace(settings.randomUserFile, settings.anonymizedRandomUserFile)`.
    fn anonymized_stack_line(&self) -> String {
        self.stack[4].replacen(&self.random_user_file, &self.anonymized_random_user_file, 1)
    }

    /// As conferências de que nenhum dado pessoal sobrou no texto (`indexOf(personalInfo)` e `indexOf(filePrefix)`).
    fn assert_no_personal_info(&self, text: &str) {
        assert!(!text.contains(&self.personal_info));
        assert!(!text.contains(&self.file_prefix));
    }

    /// As conferências da pilha: a linha anonimizada fica, e o número de linhas é o da pilha original.
    fn assert_callstack_shape(&self, callstack: &str) {
        assert!(callstack.contains(&self.anonymized_stack_line()));
        assert_eq!(callstack.split('\n').count(), self.stack.len());
    }
}

#[test]
fn disposing() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let test_appender = TestTelemetryAppender::new();
        let service = service_with(&guard, &test_appender);

        service.public_log("testPrivateEvent", None);
        assert_eq!(test_appender.get_events_count(), 1);

        service.dispose();
        assert!(!test_appender.is_disposed.get());
    });
}

// event reporting
#[test]
fn simple_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let test_appender = TestTelemetryAppender::new();
        let service = service_with(&guard, &test_appender);

        service.public_log("testEvent", None);
        assert_eq!(test_appender.get_events_count(), 1);
        assert_eq!(test_appender.event_names(), vec!["testEvent"]);
        assert!(!matches!(test_appender.data(0), Dynamic::Null));

        service.dispose();
    });
}

#[test]
fn fixed_telemetry_level_does_not_require_a_configuration_service() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let test_appender = TestTelemetryAppender::new();
        let service = TelemetryService::create_with_level(
            TelemetryServiceConfig { appenders: appenders(&test_appender), send_error_telemetry: true, ..TelemetryServiceConfig::default() },
            TelemetryLevel::Error,
            product(),
        );

        service.public_log("usageEvent", None);
        service.public_log_error("errorEvent", None);

        assert_eq!(test_appender.event_names(), vec!["errorEvent"]);
        service.dispose();
    });
}

#[test]
fn event_with_data() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let test_appender = TestTelemetryAppender::new();
        let service = service_with(&guard, &test_appender);

        service.public_log(
            "testEvent",
            Some(event_data(vec![
                ("stringProp", s("property")),
                ("numberProp", n(1.0)),
                ("booleanProp", Dynamic::Bool(true)),
                ("complexProp", obj(vec![("value", n(0.0))])),
            ])),
        );

        assert_eq!(test_appender.get_events_count(), 1);
        assert_eq!(test_appender.event_names(), vec!["testEvent"]);
        let data = test_appender.data(0);
        assert!(!matches!(data, Dynamic::Null));
        assert_deep_strict_equal(&data.get_str("stringProp"), &s("property"));
        assert_deep_strict_equal(&data.get_str("numberProp"), &n(1.0));
        assert_deep_strict_equal(&data.get_str("booleanProp"), &Dynamic::Bool(true));
        assert_deep_strict_equal(&data.get_str("complexProp").get_str("value"), &n(0.0));

        service.dispose();
    });
}

#[test]
fn event_data_redacts_file_paths_beneath_user_content_paths() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let test_appender = TestTelemetryAppender::new();
        let user_home = "/home/someone";
        let app_root = "/home/someone/.local/share/code/resources/app";
        let extensions_path = "/home/someone/.vscode/extensions";
        let service = telemetry_service(
            &guard,
            TelemetryServiceConfig {
                appenders: appenders(&test_appender),
                pii_paths: vec![app_root.to_owned(), extensions_path.to_owned(), user_home.to_owned()],
                user_content_paths: vec![user_home.to_owned()],
                ..TelemetryServiceConfig::default()
            },
        );

        let data = vec![
            ("userFile", format!("Failed to read {user_home}/projects/sample/config.py")),
            ("userNodeModulesFile", format!("at load ({user_home}/projects/sample/node_modules/@scope/pkg/index.js:1:2)")),
            ("userExtensionsFile", format!("at activate ({user_home}/projects/sample/.vscode/extensions/sample.extension/out/main.js:1:2)")),
            ("appNodeModulesFile", format!("at load ({app_root}/node_modules/pkg/index.js:1:2)")),
            ("extensionFile", format!("at activate ({extensions_path}/publisher.extension-1.0.0/out/extension.js:1:2)")),
        ];
        let keys: Vec<&str> = data.iter().map(|(key, _)| *key).collect();
        service.public_log("testEvent", Some(event_data(data.iter().map(|(key, value)| (*key, s(value))).collect())));

        let sent: Vec<(&str, String)> = keys.iter().map(|key| (*key, test_appender.text(0, key))).collect();
        assert_eq!(
            sent,
            vec![
                ("userFile", "Failed to read <REDACTED: user-file-path>".to_owned()),
                ("userNodeModulesFile", "at load (<REDACTED: user-file-path>:1:2)".to_owned()),
                ("userExtensionsFile", "at activate (<REDACTED: user-file-path>:1:2)".to_owned()),
                ("appNodeModulesFile", "at load (/node_modules/pkg/index.js:1:2)".to_owned()),
                ("extensionFile", "at activate (/publisher.extension-1.0.0/out/extension.js:1:2)".to_owned()),
            ]
        );

        service.dispose();
    });
}

#[test]
fn common_properties_added_to_all_events_simple_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_appender = TestTelemetryAppender::new();
    let service = telemetry_service(
        &guard,
        TelemetryServiceConfig {
            appenders: appenders(&test_appender),
            common_properties: Some(obj(vec![("foo", s("JA!")), ("bar", Dynamic::Bool(false))])),
            ..TelemetryServiceConfig::default()
        },
    );

    service.public_log("testEvent", None);
    let first = test_appender.data(0);

    assert_eq!(first.own_keys().len(), 2);
    assert!(matches!(first.get_str("foo"), Dynamic::String(_)));
    assert!(matches!(first.get_str("bar"), Dynamic::Bool(_)));

    service.dispose();
}

#[test]
fn common_properties_added_to_all_events_event_with_data() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_appender = TestTelemetryAppender::new();
    let service = telemetry_service(
        &guard,
        TelemetryServiceConfig {
            appenders: appenders(&test_appender),
            common_properties: Some(obj(vec![("foo", s("JA!")), ("bar", Dynamic::Bool(false))])),
            ..TelemetryServiceConfig::default()
        },
    );

    service.public_log("testEvent", Some(event_data(vec![("hightower", s("xl")), ("price", n(8000.0))])));
    let first = test_appender.data(0);

    assert_eq!(first.own_keys().len(), 4);
    assert!(matches!(first.get_str("foo"), Dynamic::String(_)));
    assert!(matches!(first.get_str("bar"), Dynamic::Bool(_)));
    assert!(matches!(first.get_str("hightower"), Dynamic::String(_)));
    assert!(matches!(first.get_str("price"), Dynamic::Number(_)));

    service.dispose();
}

#[test]
fn telemetry_info_comes_from_properties() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let null_appender: Rc<dyn ITelemetryAppender> = Rc::new(NullAppender);
    let service = telemetry_service(
        &guard,
        TelemetryServiceConfig {
            appenders: vec![null_appender],
            common_properties: Some(obj(vec![("sessionID", s("one")), ("common.machineId", s("three"))])),
            ..TelemetryServiceConfig::default()
        },
    );

    assert_eq!(service.session_id(), "one");
    assert_eq!(service.machine_id(), "three");

    service.dispose();
}

#[test]
fn set_common_property_adds_and_removes_property_from_subsequent_events() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_appender = TestTelemetryAppender::new();
    let service = service_with(&guard, &test_appender);

    service.public_log("eventBeforeSet", None);
    service.set_common_property("common.copilotTrackingId", Some(CommonPropertyValue::Text("test-tracking-id".to_owned())));
    service.public_log("eventAfterSet", None);
    service.set_common_property("common.copilotTrackingId", None);
    service.public_log("eventAfterClear", None);

    let tracking_ids: Vec<Option<String>> = (0..3)
        .map(|index| match test_appender.data(index).get_str("common.copilotTrackingId") {
            Dynamic::Undefined => None,
            other => Some(text_of(&other)),
        })
        .collect();
    assert_eq!(tracking_ids, vec![None, Some("test-tracking-id".to_owned()), None]);

    service.dispose();
}

#[test]
fn msft_internal_reflects_common_properties_set_after_construction() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let null_appender: Rc<dyn ITelemetryAppender> = Rc::new(NullAppender);
    let service = telemetry_service(&guard, TelemetryServiceConfig { appenders: vec![null_appender], ..TelemetryServiceConfig::default() });

    let before_set = service.msft_internal();
    service.set_common_property("common.msftInternal", Some(CommonPropertyValue::Bool(true)));

    assert_eq!((before_set, service.msft_internal()), (None, Some(true)));

    service.dispose();
}

#[test]
fn telemetry_on_by_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_appender = TestTelemetryAppender::new();
    let service = service_with(&guard, &test_appender);

    service.public_log("testEvent", None);
    assert_eq!(test_appender.get_events_count(), 1);
    assert_eq!(test_appender.event_names(), vec!["testEvent"]);

    service.dispose();
}

#[test]
fn error_events() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();

        let harness = ErrorHarness::new(&guard, &[], None);

        // for Phantom
        let e = Error::new("This is a test.").with_stack("blah");
        errors::on_unexpected_error(&e);
        tick();

        assert_eq!(harness.appender.get_events_count(), 1);
        assert_eq!(harness.appender.event_names(), vec!["UnhandledError"]);
        assert_eq!(harness.appender.text(0, "msg"), "This is a test.");

        harness.dispose();
    });
}

#[test]
fn handle_global_errors() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let test_error = Error::new("test");
        harness.window.report("Error Message", Some("file.js"), Some(2), Some(42), Some(&test_error));
        tick();

        let expected_call: OnErrorCall = ("Error Message".to_owned(), Some("file.js".to_owned()), Some(2), Some(42), Some(test_error.clone()));
        assert!(calls.borrow().iter().all(|call| *call == expected_call));
        assert_eq!(calls.borrow().len(), 1);

        assert_eq!(harness.appender.get_events_count(), 1);
        assert_eq!(harness.appender.event_names(), vec!["UnhandledError"]);
        assert_eq!(harness.appender.text(0, "msg"), "Error Message");
        assert_eq!(harness.appender.text(0, "file"), "file.js");
        assert_deep_strict_equal(&harness.appender.data(0).get_str("line"), &n(2.0));
        assert_deep_strict_equal(&harness.appender.data(0).get_str("column"), &n(42.0));
        assert_eq!(harness.appender.text(0, "uncaught_error_msg"), "test");

        harness.dispose();
    });
}

#[test]
fn error_telemetry_removes_pii_from_filename_with_spaces() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let person_info_with_spaces = format!("{} {}", &settings.personal_info[..2], &settings.personal_info[2..]);
        let dangerous_filename_error = settings.error("dangerousFilename");
        let dangerous_file = format!("{}/test.js", settings.dangerous_path_with_important_info.replacen(&settings.personal_info, &person_info_with_spaces, 1));
        harness.window.report("dangerousFilename", Some(&dangerous_file), Some(2), Some(42), Some(&dangerous_filename_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        let file = harness.appender.text(0, "file");
        assert!(!file.contains(&settings.dangerous_path_with_important_info.replacen(&settings.personal_info, &person_info_with_spaces, 1)));
        assert_eq!(file, format!("{}/test.js", settings.important_info));

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_pii_from_filename() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));
        let dangerous_file = format!("{}/test.js", settings.dangerous_path_with_important_info);

        let dangerous_filename_error = settings.error("dangerousFilename");
        harness.window.report("dangerousFilename", Some(&dangerous_file), Some(2), Some(42), Some(&dangerous_filename_error));
        tick();
        assert_eq!(calls.borrow().len(), 1);
        assert!(!harness.appender.text(0, "file").contains(&settings.dangerous_path_with_important_info));

        let dangerous_filename_error = settings.error("dangerousFilename");
        harness.window.report("dangerousFilename", Some(&dangerous_file), Some(2), Some(42), Some(&dangerous_filename_error));
        tick();
        assert_eq!(calls.borrow().len(), 2);
        assert!(!harness.appender.text(0, "file").contains(&settings.dangerous_path_with_important_info));
        assert_eq!(harness.appender.text(0, "file"), format!("{}/test.js", settings.important_info));

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_pii() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], None);

        let dangerous_path_without_important_info_error = settings.error(&settings.dangerous_path_without_important_info);
        errors::on_unexpected_error(&dangerous_path_without_important_info_error);
        tick();

        settings.assert_no_personal_info(&harness.appender.text(0, "msg"));
        let callstack = harness.appender.text(0, "callstack");
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_redacts_only_offending_frames_and_preserves_the_rest_of_the_callstack() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let harness = ErrorHarness::new(&guard, &[], None);

        // A frame whose function name matches the broad `Generic Secret` heuristic
        // (`getStorageKey` contains `key(`) previously caused the entire callstack
        // to be redacted. See https://github.com/microsoft/vscode/issues/301200.
        let stack = [
            "Error: Something failed",
            "    at StorageService.getStorageKey (out/vs/platform/storage/storage.js:1:200)",
            "    at Foo.run (out/vs/workbench/foo.js:3:40)",
            "    at Bar.baz (out/vs/workbench/bar.js:5:60)",
        ];

        let error = Error::new("Something failed").with_stack(stack.join("\n"));
        errors::on_unexpected_error(&error);
        tick();

        assert_eq!(harness.appender.get_events_count(), 1);
        let cs = harness.appender.text(0, "callstack");
        // The whole stack must not collapse into a single redaction marker.
        assert_ne!(cs, "<REDACTED: Generic Secret>", "Entire callstack should not be redacted");
        assert_eq!(cs.split('\n').count(), stack.len(), "All frames should be preserved");
        // Only the offending frame is redacted, the others remain intact.
        assert!(cs.contains("Foo.run"), "Non-offending frames should be preserved");
        assert!(cs.contains("Bar.baz"), "Non-offending frames should be preserved");
        assert!(!cs.contains("getStorageKey"), "Offending frame should be redacted");

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_still_redacts_a_frame_whose_trailing_token_relies_on_the_newline_delimiter() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let harness = ErrorHarness::new(&guard, &[], None);

        // `getApiKey` ends the line, so the `Generic Secret` heuristic only
        // matches because of the following newline. Per-line redaction must
        // re-append that delimiter so this frame is still redacted, matching
        // the previous whole-string behavior.
        let stack = ["Error: boom", "    at Service.getApiKey", "    at Foo.run (out/vs/workbench/foo.js:3:40)"];

        let error = Error::new("boom").with_stack(stack.join("\n"));
        errors::on_unexpected_error(&error);
        tick();

        assert_eq!(harness.appender.get_events_count(), 1);
        let cs = harness.appender.text(0, "callstack");
        assert!(!cs.contains("getApiKey"), "Trailing-token frame should still be redacted");
        assert!(cs.contains("Foo.run"), "Other frames should be preserved");
        assert_eq!(cs.split('\n').count(), stack.len(), "All frames should be preserved");

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_pii() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let dangerous_path_without_important_info_error = settings.error("dangerousPathWithoutImportantInfo");
        harness.window.report(&settings.dangerous_path_without_important_info, Some("test.js"), Some(2), Some(42), Some(&dangerous_path_without_important_info_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        // Test that no file information remains, esp. personal info
        settings.assert_no_personal_info(&harness.appender.text(0, "msg"));
        let callstack = harness.appender.text(0, "callstack");
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_pii_but_preserves_code_file_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], None);

        let dangerous_path_with_important_info_error = settings.error(&settings.dangerous_path_with_important_info);

        // Test that important information remains but personal info does not
        errors::on_unexpected_error(&dangerous_path_with_important_info_error);
        tick();

        let msg = harness.appender.text(0, "msg");
        assert!(msg.contains(&settings.important_info));
        settings.assert_no_personal_info(&msg);
        let callstack = harness.appender.text(0, "callstack");
        assert!(callstack.contains(&settings.important_info));
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_pii_but_preserves_code_file_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let dangerous_path_with_important_info_error = settings.error("dangerousPathWithImportantInfo");
        harness.window.report(&settings.dangerous_path_with_important_info, Some("test.js"), Some(2), Some(42), Some(&dangerous_path_with_important_info_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        // Test that important information remains but personal info does not
        let callstack = harness.appender.text(0, "callstack");
        assert!(callstack.contains(&settings.anonymized_node_module_asar_path), "bare node_modules.asar path");
        assert!(callstack.contains(&settings.anonymized_node_module_path), "bare node_modules path");
        let msg = harness.appender.text(0, "msg");
        assert!(msg.contains(&settings.important_info));
        settings.assert_no_personal_info(&msg);
        assert!(callstack.contains(&settings.important_info));
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_pii_but_preserves_code_file_path_with_node_modules() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], None);

        let dangerous_path_with_important_info_error = settings.error(&settings.dangerous_path_with_important_info);

        errors::on_unexpected_error(&dangerous_path_with_important_info_error);
        tick();

        // All node_modules paths (bare and full) should preserve the node_modules/... suffix after redaction
        let cs = harness.appender.text(0, "callstack");
        assert!(cs.contains(&settings.anonymized_node_module_asar_path), "bare node_modules.asar path");
        assert!(cs.contains(&settings.anonymized_node_module_path), "bare node_modules path");
        assert!(cs.contains(&settings.anonymized_full_node_module_path), "full node_modules path");
        assert!(cs.contains(&settings.anonymized_full_node_module_asar_path), "full node_modules.asar path");

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_pii_but_preserves_extension_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], None);

        let dangerous_path_with_important_info_error = settings.error(&settings.dangerous_path_with_important_info);

        errors::on_unexpected_error(&dangerous_path_with_important_info_error);
        tick();

        let cs = harness.appender.text(0, "callstack");
        // Verify user extension path is preserved but parent folder is redacted
        assert!(cs.contains(&settings.extension_path_to_retain), "User extension path should be retained");
        assert!(cs.contains(&settings.anonymized_extension_path), "User extension path should be anonymized with preserved extension name");
        // Verify the username is removed
        assert!(!cs.contains("/Users/username/"), "Username should be redacted from extension path");

        // Verify server-insiders extension path is preserved (multi-segment suffix like .vscode-server-insiders)
        assert!(cs.contains(&settings.server_insiders_extension_path_to_retain), "Server-insiders extension path should be retained");
        assert!(cs.contains(&settings.anonymized_server_insiders_extension_path), "Server-insiders extension path should be anonymized with preserved extension name");
        // Verify the home directory is removed
        assert!(!cs.contains("/home/user/"), "Home directory should be redacted from server-insiders extension path");

        // Verify built-in extension path is preserved but app folder is redacted
        assert!(cs.contains(&settings.builtin_extension_path_to_retain), "Built-in extension path should be retained");
        assert!(cs.contains(&settings.anonymized_builtin_extension_path), "Built-in extension path should be anonymized with preserved extension name");
        // Verify the app path is removed
        assert!(!cs.contains("/Applications/Visual Studio Code.app"), "App path should be redacted from built-in extension path");

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_pii_but_preserves_code_file_path_when_pii_path_is_configured() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let settings = ErrorTestingSettings::new();
        let pii_path = format!("{}/resources/app/", settings.personal_info);
        let harness = ErrorHarness::new(&guard, &[pii_path.as_str()], None);

        let dangerous_path_with_important_info_error = settings.error(&settings.dangerous_path_with_important_info);

        // Test that important information remains but personal info does not
        errors::on_unexpected_error(&dangerous_path_with_important_info_error);
        tick();

        let msg = harness.appender.text(0, "msg");
        assert!(msg.contains(&settings.important_info));
        settings.assert_no_personal_info(&msg);
        let callstack = harness.appender.text(0, "callstack");
        assert!(callstack.contains(&settings.important_info));
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_pii_but_preserves_code_file_path_when_pii_path_is_configured() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let settings = ErrorTestingSettings::new();
        let pii_path = format!("{}/resources/app/", settings.personal_info);
        let harness = ErrorHarness::new(&guard, &[pii_path.as_str()], Some(stub));

        let dangerous_path_with_important_info_error = settings.error("dangerousPathWithImportantInfo");
        harness.window.report(&settings.dangerous_path_with_important_info, Some("test.js"), Some(2), Some(42), Some(&dangerous_path_with_important_info_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        // Test that important information remains but personal info does not
        let msg = harness.appender.text(0, "msg");
        assert!(msg.contains(&settings.important_info));
        settings.assert_no_personal_info(&msg);
        let callstack = harness.appender.text(0, "callstack");
        assert!(callstack.contains(&settings.important_info));
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_pii_but_preserves_missing_model_error_message() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], None);

        let missing_model_error = settings.error(&settings.missing_model_message);

        // Test that no file information remains, but this particular
        // error message does (Received model events for missing model)
        errors::on_unexpected_error(&missing_model_error);
        tick();

        let msg = harness.appender.text(0, "msg");
        assert!(msg.contains(&settings.missing_model_prefix));
        settings.assert_no_personal_info(&msg);
        let callstack = harness.appender.text(0, "callstack");
        assert!(callstack.contains(&settings.missing_model_prefix));
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_pii_but_preserves_missing_model_error_message() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let missing_model_error = settings.error("missingModelMessage");
        harness.window.report(&settings.missing_model_message, Some("test.js"), Some(2), Some(42), Some(&missing_model_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        // Test that no file information remains, but this particular
        // error message does (Received model events for missing model)
        let msg = harness.appender.text(0, "msg");
        assert!(msg.contains(&settings.missing_model_prefix));
        settings.assert_no_personal_info(&msg);
        let callstack = harness.appender.text(0, "callstack");
        assert!(callstack.contains(&settings.missing_model_prefix));
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_pii_but_preserves_no_such_file_error_message() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], None);

        let no_such_file_error = settings.error(&settings.no_such_file_message);

        // Test that no file information remains, but this particular
        // error message does (ENOENT: no such file or directory)
        errors::on_unexpected_error(&no_such_file_error);
        tick();

        let msg = harness.appender.text(0, "msg");
        assert!(msg.contains(&settings.no_such_file_prefix));
        settings.assert_no_personal_info(&msg);
        let callstack = harness.appender.text(0, "callstack");
        assert!(callstack.contains(&settings.no_such_file_prefix));
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_pii_but_preserves_no_such_file_error_message() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let (stub, calls) = error_stub();
        let settings = ErrorTestingSettings::new();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let no_such_file_error = settings.error("noSuchFileMessage");
        harness.window.report(&settings.no_such_file_message, Some("test.js"), Some(2), Some(42), Some(&no_such_file_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        // Test that no file information remains, but this particular
        // error message does (ENOENT: no such file or directory)
        errors::on_unexpected_error(&no_such_file_error);
        let msg = harness.appender.text(0, "msg");
        assert!(msg.contains(&settings.no_such_file_prefix));
        settings.assert_no_personal_info(&msg);
        let callstack = harness.appender.text(0, "callstack");
        assert!(callstack.contains(&settings.no_such_file_prefix));
        settings.assert_no_personal_info(&callstack);
        settings.assert_callstack_shape(&callstack);

        harness.dispose();
    });
}

#[test]
fn telemetry_service_sends_events_when_telemetry_is_on() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let test_appender = TestTelemetryAppender::new();
        let service = service_with(&guard, &test_appender);
        service.public_log("testEvent", None);
        assert_eq!(test_appender.get_events_count(), 1);
        service.dispose();
    });
}

#[test]
fn telemetry_service_checks_with_config_service() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let config = configuration(&guard);
    let telemetry_level = |value: &str| {
        let _ = config.set_user_configuration("telemetry.telemetryLevel", s(value), None);
    };
    telemetry_level(TelemetryConfiguration::Off.as_str());

    let test_appender = TestTelemetryAppender::new();
    let configuration_service: Rc<dyn IConfigurationService> = config.clone();
    let service = TelemetryService::new(
        TelemetryServiceConfig { appenders: appenders(&test_appender), send_error_telemetry: true, ..TelemetryServiceConfig::default() },
        Some(configuration_service),
        product(),
    )
    .expect("há serviço de configuração");

    assert_eq!(service.telemetry_level(), TelemetryLevel::None);

    telemetry_level(TelemetryConfiguration::On.as_str());
    fire_config_change(&config, "telemetry.telemetryLevel");
    assert_eq!(service.telemetry_level(), TelemetryLevel::Usage);

    telemetry_level(TelemetryConfiguration::Error.as_str());
    fire_config_change(&config, "telemetry.telemetryLevel");
    assert_eq!(service.telemetry_level(), TelemetryLevel::Error);

    telemetry_level("invalid");
    fire_config_change(&config, "telemetry.telemetryLevel");
    service.public_log("invalidTelemetryLevel", None);
    service.public_log_error("invalidTelemetryLevelError", None);
    assert_eq!((service.telemetry_level(), test_appender.get_events_count()), (TelemetryLevel::None, 0));

    service.dispose();
}

#[test]
fn unexpected_error_telemetry_removes_windows_pii_but_preserves_code_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let harness = ErrorHarness::new(&guard, &[], None);

        let windows_user_path = "c:/Users/bpasero/AppData/Local/Programs/Microsoft%20VS%20Code%20Insiders/resources/app/";
        let code_path = "out/vs/workbench/workbench.desktop.main.js";
        let stack = [
            format!("    at cTe.gc (vscode-file://vscode-app/{windows_user_path}{code_path}:2724:81492)"),
            format!("    at async cTe.setInput (vscode-file://vscode-app/{windows_user_path}{code_path}:2724:80650)"),
            format!("    at async qJe.S (vscode-file://vscode-app/{windows_user_path}{code_path}:698:58520)"),
            format!("    at async qJe.L (vscode-file://vscode-app/{windows_user_path}{code_path}:698:57080)"),
            format!("    at async qJe.openEditor (vscode-file://vscode-app/{windows_user_path}{code_path}:698:56162)"),
        ];

        let windows_error = Error::new("The editor could not be opened because the file was not found.").with_stack(stack.join("\n"));

        errors::on_unexpected_error(&windows_error);
        tick();

        assert_eq!(harness.appender.get_events_count(), 1);
        // Verify PII (username and path) is removed
        let callstack = harness.appender.text(0, "callstack");
        assert!(!callstack.contains("bpasero"));
        assert!(!callstack.contains("Users"));
        assert!(!callstack.contains("c:/Users"));
        // Verify important code path is preserved
        assert!(callstack.contains(code_path));
        assert!(callstack.contains("out/vs/workbench"));

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_windows_pii_but_preserves_code_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let windows_user_path = "c:/Users/bpasero/AppData/Local/Programs/Microsoft%20VS%20Code%20Insiders/resources/app/";
        let code_path = "out/vs/workbench/workbench.desktop.main.js";
        let stack = [
            format!("    at cTe.gc (vscode-file://vscode-app/{windows_user_path}{code_path}:2724:81492)"),
            format!("    at async cTe.setInput (vscode-file://vscode-app/{windows_user_path}{code_path}:2724:80650)"),
            format!("    at async qJe.S (vscode-file://vscode-app/{windows_user_path}{code_path}:698:58520)"),
        ];

        let windows_error = Error::new("The editor could not be opened because the file was not found.").with_stack(stack.join("\n"));

        harness.window.report("The editor could not be opened because the file was not found.", Some("test.js"), Some(2), Some(42), Some(&windows_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        // Verify PII (username and path) is removed
        let callstack = harness.appender.text(0, "callstack");
        assert!(!callstack.contains("bpasero"));
        assert!(!callstack.contains("Users"));
        assert!(!callstack.contains("c:/Users"));
        // Verify important code path is preserved
        assert!(callstack.contains(code_path));
        assert!(callstack.contains("out/vs/workbench"));

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_mac_os_pii_but_preserves_code_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let harness = ErrorHarness::new(&guard, &[], None);

        let mac_user_path = "Applications/Visual%20Studio%20Code%20-%20Insiders.app/Contents/Resources/app/";
        let code_path = "out/vs/workbench/workbench.desktop.main.js";
        let stack = [
            format!("    at uTe.gc (vscode-file://vscode-app/{mac_user_path}{code_path}:2720:81492)"),
            format!("    at async uTe.setInput (vscode-file://vscode-app/{mac_user_path}{code_path}:2720:80650)"),
            format!("    at async JJe.S (vscode-file://vscode-app/{mac_user_path}{code_path}:698:58520)"),
            format!("    at async JJe.L (vscode-file://vscode-app/{mac_user_path}{code_path}:698:57080)"),
            format!("    at async JJe.openEditor (vscode-file://vscode-app/{mac_user_path}{code_path}:698:56162)"),
        ];

        let mac_error = Error::new("The editor could not be opened because the file was not found.").with_stack(stack.join("\n"));

        errors::on_unexpected_error(&mac_error);
        tick();

        assert_eq!(harness.appender.get_events_count(), 1);
        // Verify PII (application path) is removed
        let callstack = harness.appender.text(0, "callstack");
        assert!(!callstack.contains("Applications/Visual"));
        assert!(!callstack.contains("Visual%20Studio%20Code"));
        // Verify important code path is preserved
        assert!(callstack.contains(code_path));
        assert!(callstack.contains("out/vs/workbench"));

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_mac_os_pii_but_preserves_code_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let mac_user_path = "Applications/Visual%20Studio%20Code%20-%20Insiders.app/Contents/Resources/app/";
        let code_path = "out/vs/workbench/workbench.desktop.main.js";
        let stack = [
            format!("    at uTe.gc (vscode-file://vscode-app/{mac_user_path}{code_path}:2720:81492)"),
            format!("    at async uTe.setInput (vscode-file://vscode-app/{mac_user_path}{code_path}:2720:80650)"),
            format!("    at async JJe.S (vscode-file://vscode-app/{mac_user_path}{code_path}:698:58520)"),
        ];

        let mac_error = Error::new("The editor could not be opened because the file was not found.").with_stack(stack.join("\n"));

        harness.window.report("The editor could not be opened because the file was not found.", Some("test.js"), Some(2), Some(42), Some(&mac_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        // Verify PII (application path) is removed
        let callstack = harness.appender.text(0, "callstack");
        assert!(!callstack.contains("Applications/Visual"));
        assert!(!callstack.contains("Visual%20Studio%20Code"));
        // Verify important code path is preserved
        assert!(callstack.contains(code_path));
        assert!(callstack.contains("out/vs/workbench"));

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_removes_linux_pii_but_preserves_code_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let harness = ErrorHarness::new(&guard, &[], None);

        let linux_user_path = "/home/parallels/GitDevelopment/vscode-node-sqlite3-perf/";
        let linux_system_path = "usr/share/code-insiders/resources/app/";
        let code_path = "out/vs/workbench/workbench.desktop.main.js";
        let stack = [
            format!("    at _kt.G (vscode-file://vscode-app/{linux_system_path}{code_path}:3825:65940)"),
            format!("    at _kt.F (vscode-file://vscode-app/{linux_system_path}{code_path}:3825:65765)"),
            format!("    at async axt.L (vscode-file://vscode-app/{linux_system_path}{code_path}:3830:9998)"),
            format!("    at async axt.readStream (vscode-file://vscode-app/{linux_system_path}{code_path}:3830:9773)"),
            format!("    at async mye.Eb (vscode-file://vscode-app/{linux_system_path}{code_path}:1313:12359)"),
        ];

        let linux_error = Error::new(format!("Invalid fake file 'git:{linux_user_path}index.js.git?{{\"path\":\"{linux_user_path}index.js\",\"ref\":\"\"}}' (Canceled: Canceled)"))
            .with_stack(stack.join("\n"));

        errors::on_unexpected_error(&linux_error);
        tick();

        assert_eq!(harness.appender.get_events_count(), 1);
        // Verify PII (username and home directory) is removed
        let msg = harness.appender.text(0, "msg");
        assert!(!msg.contains("parallels"));
        assert!(!msg.contains("/home/parallels"));
        assert!(!msg.contains("GitDevelopment"));
        let callstack = harness.appender.text(0, "callstack");
        assert!(!callstack.contains("parallels"));
        assert!(!callstack.contains("/home/parallels"));
        // Verify important code path is preserved
        assert!(callstack.contains(code_path));
        assert!(callstack.contains("out/vs/workbench"));

        harness.dispose();
    });
}

#[test]
fn uncaught_error_telemetry_removes_linux_pii_but_preserves_code_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let (stub, calls) = error_stub();
        let harness = ErrorHarness::new(&guard, &[], Some(stub));

        let linux_user_path = "/home/parallels/GitDevelopment/vscode-node-sqlite3-perf/";
        let linux_system_path = "usr/share/code-insiders/resources/app/";
        let code_path = "out/vs/workbench/workbench.desktop.main.js";
        let stack = [
            format!("    at _kt.G (vscode-file://vscode-app/{linux_system_path}{code_path}:3825:65940)"),
            format!("    at _kt.F (vscode-file://vscode-app/{linux_system_path}{code_path}:3825:65765)"),
            format!("    at async axt.L (vscode-file://vscode-app/{linux_system_path}{code_path}:3830:9998)"),
        ];

        let linux_error = Error::new(format!("Unable to read file 'git:{linux_user_path}index.js.git'")).with_stack(stack.join("\n"));

        harness.window.report(&format!("Unable to read file 'git:{linux_user_path}index.js.git'"), Some("test.js"), Some(2), Some(42), Some(&linux_error));
        tick();

        assert_eq!(calls.borrow().len(), 1);
        // Verify PII (username and home directory) is removed
        let msg = harness.appender.text(0, "msg");
        assert!(!msg.contains("parallels"));
        assert!(!msg.contains("/home/parallels"));
        assert!(!msg.contains("GitDevelopment"));
        let callstack = harness.appender.text(0, "callstack");
        assert!(!callstack.contains("parallels"));
        assert!(!callstack.contains("/home/parallels"));
        // Verify important code path is preserved
        assert!(callstack.contains(code_path));
        assert!(callstack.contains("out/vs/workbench"));

        harness.dispose();
    });
}

#[test]
fn unexpected_error_telemetry_strips_web_origin_but_preserves_path_in_web_stack_traces_when_pii_paths_includes_origin() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        silence_unexpected_errors();
        let web_origin = "https://codespace-host.github.dev";
        let harness = ErrorHarness::new(&guard, &[web_origin], None);

        let bundle_path = "/static/build/bundle.js";
        let stack = [
            "Error: Something failed".to_owned(),
            format!("    at x3t._delegate ({web_origin}{bundle_path}:1:200953)"),
            format!("    at y4u.run ({web_origin}{bundle_path}:1:304822)"),
            "    at DedicatedWorkerGlobalScope.self.onmessage".to_owned(),
        ];

        let web_error = Error::new("Something failed").with_stack(stack.join("\n"));

        errors::on_unexpected_error(&web_error);
        tick();

        assert_eq!(harness.appender.get_events_count(), 1);
        let cs = harness.appender.text(0, "callstack");
        // Verify the web origin is stripped (not leaked as PII)
        assert!(!cs.contains(web_origin), "Web origin should be stripped");
        assert!(!cs.contains("https://"), "HTTPS scheme should be stripped");
        // Verify the bundle path is preserved for debugging
        assert!(cs.contains(bundle_path), "Bundle path should be preserved");

        harness.dispose();
    });
}
