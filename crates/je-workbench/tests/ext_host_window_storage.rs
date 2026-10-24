//! Teste de unidade de `ExtHostWindow`, `ExtHostStorage`, `ExtHostSecretState` e `ExtHostApiDeprecationService` (`extHostWindow.ts`,
//! `extHostStorage.ts`, `extHostSecretState.ts` e `extHostApiDeprecationService.ts`; o oráculo não tem teste deles, então não é porte)
//! contra dublês dos proxies `MainThread*` que registram as chamadas.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_api_deprecation_service::{ExtHostApiDeprecationService, IExtHostApiDeprecationService};
use je_workbench::api::common::ext_host_protocol::{
    ExtHostSecretStateShape, ExtHostStorageShape, ExtHostWindowShape, IExtensionIdWithVersion, IMainContext, IOpenUriOptions, MainContext, MainThreadSecretStateShape,
    MainThreadStorageShape, MainThreadTelemetryShape, MainThreadWindowShape, WindowInitialState,
};
use je_workbench::api::common::ext_host_storage::{ExtHostSecretState, ExtHostStorage};
use je_workbench::api::common::ext_host_window::{ExtHostWindow, UriOrString, WindowState};

#[derive(Default)]
struct TestRpcProtocol {
    objects: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
}

impl IMainContext for TestRpcProtocol {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        self.objects.borrow().get(sid).cloned().unwrap_or_else(|| panic!("o proxy '{sid}' não foi registrado"))
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        self.objects.borrow_mut().insert(sid, value);
    }
}

fn context() -> Rc<dyn IMainContext> {
    Rc::new(TestRpcProtocol::default())
}

fn pump() {
    for _ in 0..16 {
        je_base::runtime::turn();
    }
}

// ---- janela -----------------------------------------------------------------------------------------------------------

struct RecordingWindow {
    calls: RefCell<Vec<String>>,
}

impl IDisposable for RecordingWindow {
    fn dispose(&self) {}
}

impl MainThreadWindowShape for RecordingWindow {
    fn rpc_get_initial_state(&self) -> Promise<WindowInitialState> {
        Promise::resolved(WindowInitialState { is_focused: false, is_active: true })
    }

    fn rpc_open_uri(&self, uri: URI, uri_string: Option<String>, options: IOpenUriOptions) -> Promise<bool> {
        self.calls.borrow_mut().push(format!("open {} text={uri_string:?} tunneling={:?}", uri.to_string(false), options.allow_tunneling));
        Promise::resolved(true)
    }

    fn rpc_as_external_uri(&self, uri: URI, _options: IOpenUriOptions) -> Promise<URI> {
        Promise::resolved(uri.with(&je_base::uri::UriChange { authority: "externo.exemplo".into(), ..Default::default() }))
    }
}

fn window() -> (Rc<ExtHostWindow>, Rc<RecordingWindow>) {
    let rpc = context();
    let proxy = Rc::new(RecordingWindow { calls: RefCell::new(Vec::new()) });
    let shape: Rc<dyn MainThreadWindowShape> = proxy.clone();
    rpc.set(&MainContext::MAIN_THREAD_WINDOW, shape);
    (ExtHostWindow::new(&rpc, Some("AQID")), proxy)
}

#[test]
fn window_state_starts_focused_and_active_then_takes_the_initial_state_and_fires_on_changes() {
    let rpc = context();
    let proxy = Rc::new(RecordingWindow { calls: RefCell::new(Vec::new()) });
    let shape: Rc<dyn MainThreadWindowShape> = proxy.clone();
    rpc.set(&MainContext::MAIN_THREAD_WINDOW, shape);
    let window = ExtHostWindow::new(&rpc, None);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _subscription = window.on_did_change_window_state().listen(move |state| sink.borrow_mut().push(*state));
    assert_eq!(window.get_state(), WindowState { focused: true, active: true });
    pump();
    assert_eq!(window.get_state(), WindowState { focused: false, active: true }, "o estado inicial do workbench vence o padrão");
    assert_eq!(*seen.borrow(), vec![WindowState { focused: false, active: true }], "só o que mudou dispara");

    window.rpc_on_did_change_window_active(true);
    assert_eq!(seen.borrow().len(), 1, "o mesmo valor não dispara");
    window.rpc_on_did_change_window_focus(true);
    assert_eq!(seen.borrow().len(), 2);
}

#[test]
fn native_handle_is_decoded_from_base64_and_replaced_by_the_workbench() {
    let (window, _proxy) = window();
    assert_eq!(window.native_handle(), Some(vec![1, 2, 3]));
    window.rpc_on_did_change_active_native_window_handle(Some("BAU=".to_owned()));
    assert_eq!(window.native_handle(), Some(vec![4, 5]));
    window.rpc_on_did_change_active_native_window_handle(None);
    assert_eq!(window.native_handle(), None);
}

#[test]
fn open_uri_validates_the_scheme_and_forwards_the_original_text() {
    let (window, proxy) = window();
    let opened = window.open_uri(UriOrString::Text("https://exemplo.com/a".to_owned()), IOpenUriOptions { allow_tunneling: Some(true), ..IOpenUriOptions::default() });
    pump();
    assert_eq!(opened.result().expect("assentou").expect("sem erro"), true);
    assert_eq!(proxy.calls.borrow()[0], "open https://exemplo.com/a text=Some(\"https://exemplo.com/a\") tunneling=Some(true)");

    let command = window.open_uri(UriOrString::Uri(URI::parse("command:x.y", false)), IOpenUriOptions::default());
    assert_eq!(command.result().expect("assentou").expect_err("rejeita").message, "Invalid scheme 'command'");
    let empty = window.open_uri(UriOrString::Uri(URI::revive(&je_base::uri::UriState::default(), je_platform::resources::host_platform())), IOpenUriOptions::default());
    assert_eq!(empty.result().expect("assentou").expect_err("rejeita").message, "Invalid scheme - cannot be empty");
}

#[test]
fn as_external_uri_forwards_to_the_workbench() {
    let (window, _proxy) = window();
    let result = window.as_external_uri(&URI::parse("http://localhost:3000/", false), IOpenUriOptions::default());
    pump();
    assert_eq!(result.result().expect("assentou").expect("sem erro").authority().to_string_lossy(), "externo.exemplo");
}

// ---- armazenamento e segredos ------------------------------------------------------------------------------------------

struct RecordingStorage {
    stored: RefCell<Option<String>>,
    calls: RefCell<Vec<String>>,
}

impl IDisposable for RecordingStorage {
    fn dispose(&self) {}
}

impl MainThreadStorageShape for RecordingStorage {
    fn rpc_initialize_extension_storage(&self, shared: bool, extension_id: &str) -> Promise<Option<String>> {
        self.calls.borrow_mut().push(format!("init {shared} {extension_id}"));
        Promise::resolved(self.stored.borrow().clone())
    }

    fn rpc_set_value(&self, shared: bool, extension_id: &str, value: Dynamic) -> Promise<()> {
        self.calls.borrow_mut().push(format!("set {shared} {extension_id} {}", value.get_str("n") == Dynamic::Number(1.0)));
        Promise::resolved(())
    }

    fn rpc_register_extension_storage_keys_to_sync(&self, extension: IExtensionIdWithVersion, keys: Vec<String>) {
        self.calls.borrow_mut().push(format!("sync {}@{} {keys:?}", extension.id, extension.version));
    }
}

fn storage(stored: Option<&str>) -> (Rc<ExtHostStorage>, Rc<RecordingStorage>) {
    let rpc = context();
    let proxy = Rc::new(RecordingStorage { stored: RefCell::new(stored.map(str::to_owned)), calls: RefCell::new(Vec::new()) });
    let shape: Rc<dyn MainThreadStorageShape> = proxy.clone();
    rpc.set(&MainContext::MAIN_THREAD_STORAGE, shape);
    (ExtHostStorage::new(&rpc, NullLogService::shared()), proxy)
}

#[test]
fn storage_initialization_parses_the_stored_json_and_falls_back_to_the_default() {
    let (extension_storage, proxy) = storage(Some(r#"{"n":1}"#));
    let value = extension_storage.initialize_extension_storage(true, "pub.ext", None);
    pump();
    assert_eq!(value.result().expect("assentou").expect("sem erro").expect("há valor").get_str("n"), Dynamic::Number(1.0));
    assert_eq!(proxy.calls.borrow()[0], "init true pub.ext");

    let (empty_storage, _) = storage(None);
    let defaulted = empty_storage.initialize_extension_storage(false, "pub.ext", Some(Dynamic::string("padrão")));
    pump();
    assert_eq!(defaulted.result().expect("assentou").expect("sem erro"), Some(Dynamic::string("padrão")));

    let (broken_storage, _) = storage(Some("{não é json"));
    let broken = broken_storage.initialize_extension_storage(false, "pub.ext", None);
    pump();
    assert_eq!(broken.result().expect("assentou").expect("o erro de parse não rejeita"), None);
}

#[test]
fn storage_changes_from_the_workbench_fire_only_for_valid_json() {
    let (extension_storage, proxy) = storage(None);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _subscription = extension_storage.on_did_change_storage().listen(move |event| sink.borrow_mut().push((event.shared, event.key.clone())));
    extension_storage.rpc_accept_value(true, "pub.ext", r#"{"n":2}"#);
    extension_storage.rpc_accept_value(true, "pub.ext", "isto não é json");
    assert_eq!(*seen.borrow(), vec![(true, "pub.ext".to_owned())]);

    let _ = extension_storage.set_value(false, "pub.ext", Dynamic::object([("n", Dynamic::Number(1.0))]));
    extension_storage.register_extension_storage_keys_to_sync(IExtensionIdWithVersion { id: "pub.ext".to_owned(), version: "1.0.0".to_owned() }, vec!["a".to_owned()]);
    assert_eq!(*proxy.calls.borrow(), vec!["set false pub.ext true".to_owned(), "sync pub.ext@1.0.0 [\"a\"]".to_owned()]);
}

struct RecordingSecrets {
    calls: RefCell<Vec<String>>,
}

impl IDisposable for RecordingSecrets {
    fn dispose(&self) {}
}

impl MainThreadSecretStateShape for RecordingSecrets {
    fn rpc_get_password(&self, extension_id: &str, key: &str) -> Promise<Option<String>> {
        self.calls.borrow_mut().push(format!("get {extension_id} {key}"));
        Promise::resolved(Some("segredo".to_owned()))
    }

    fn rpc_set_password(&self, extension_id: &str, key: &str, value: &str) -> Promise<()> {
        self.calls.borrow_mut().push(format!("set {extension_id} {key} {value}"));
        Promise::resolved(())
    }

    fn rpc_delete_password(&self, extension_id: &str, key: &str) -> Promise<()> {
        self.calls.borrow_mut().push(format!("delete {extension_id} {key}"));
        Promise::resolved(())
    }

    fn rpc_get_keys(&self, extension_id: &str) -> Promise<Vec<String>> {
        self.calls.borrow_mut().push(format!("keys {extension_id}"));
        Promise::resolved(vec!["a".to_owned(), "b".to_owned()])
    }
}

#[test]
fn secret_state_forwards_to_the_workbench_and_relays_password_changes() {
    let rpc = context();
    let proxy = Rc::new(RecordingSecrets { calls: RefCell::new(Vec::new()) });
    let shape: Rc<dyn MainThreadSecretStateShape> = proxy.clone();
    rpc.set(&MainContext::MAIN_THREAD_SECRET_STATE, shape);
    let secrets = ExtHostSecretState::new(&rpc);

    assert_eq!(secrets.get("pub.ext", "token").result().expect("assentou").expect("sem erro"), Some("segredo".to_owned()));
    let _ = secrets.store("pub.ext", "token", "novo");
    let _ = secrets.delete("pub.ext", "token");
    assert_eq!(secrets.keys("pub.ext").result().expect("assentou").expect("sem erro"), vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(*proxy.calls.borrow(), vec!["get pub.ext token", "set pub.ext token novo", "delete pub.ext token", "keys pub.ext"]);

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _subscription = secrets.on_did_change_password().listen(move |change| sink.borrow_mut().push(change.clone()));
    assert!(secrets.rpc_on_did_change_password("pub.ext", "token").result().expect("assentou").is_ok());
    assert_eq!(*seen.borrow(), vec![("pub.ext".to_owned(), "token".to_owned())]);
}

// ---- descontinuação ---------------------------------------------------------------------------------------------------

struct RecordingTelemetry {
    events: RefCell<Vec<(String, String)>>,
}

impl IDisposable for RecordingTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for RecordingTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<Dynamic>) {}

    fn rpc_public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        let data = data.expect("o evento tem dados");
        let usage = match data.get_str("usageId") {
            Dynamic::String(text) => text.to_string_lossy(),
            _ => String::new(),
        };
        self.events.borrow_mut().push((event_name.to_owned(), usage));
    }
}

fn extension() -> ExtensionDescription {
    ExtensionDescription {
        id: "pub.ext".to_owned(),
        identifier: ExtensionIdentifier::new("pub.ext"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: "file:///ext".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest { name: "ext".to_owned(), publisher: "pub".to_owned(), version: "1.0.0".to_owned(), ..ExtensionManifest::default() },
    }
}

#[test]
fn deprecated_api_usage_is_reported_once_per_api_extension_and_usage() {
    let rpc = context();
    let telemetry = Rc::new(RecordingTelemetry { events: RefCell::new(Vec::new()) });
    let shape: Rc<dyn MainThreadTelemetryShape> = telemetry.clone();
    rpc.set(&MainContext::MAIN_THREAD_TELEMETRY, shape);
    let service = ExtHostApiDeprecationService::new(&rpc, NullLogService::shared());

    service.report("api.x", &extension(), "use y", None);
    service.report("api.x", &extension(), "use y", None);
    service.report("api.x", &extension(), "use y", Some("uso-1"));
    service.report("api.z", &extension(), "use w", None);
    assert_eq!(
        *telemetry.events.borrow(),
        vec![
            ("extHostDeprecatedApiUsage".to_owned(), String::new()),
            ("extHostDeprecatedApiUsage".to_owned(), "uso-1".to_owned()),
            ("extHostDeprecatedApiUsage".to_owned(), String::new()),
        ]
    );
}
