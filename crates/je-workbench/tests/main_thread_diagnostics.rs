//! Porte de `src/vs/workbench/api/test/browser/mainThreadDiagnostics.test.ts` (suíte `MainThreadDiagnostics`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup` e `teardown` (criar e descartar o `MarkerService`) são [`new_marker_service`] e o `marker_service.dispose()` no fim de cada caso;
//!   `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `new class implements IExtHostContext { ... getProxy() { return { $acceptMarkersChange } } }` é o [`ProxyContext`], que devolve o mesmo
//!   proxy para qualquer identificador e guarda o que `$acceptMarkersChange` recebeu; o `{ $acceptMarkersChange() { } }` dos dois primeiros
//!   casos é o mesmo contexto com o registro ignorado.
//! - `new class extends mock<IUriIdentityService>() { override asCanonicalUri(uri) { return uri; } }` é o [`IdentityUriService`]; o `ext_uri`
//!   que o original deixa sem definição falha com `unreachable!`, como o `TypeError` do `mock`.
//! - `severity: 1` é `MarkerSeverity::HINT`; `code: '666'` é `MarkerCode::Text`. O `IMarkerChange` `[resource, markers, true]` é o struct de três campos.
//! - `await timeout(0)` com os timers virtuais (`runWithFakedTimers`) deixa o `MicrotaskEmitter` do `MarkerService` entregar o evento.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::IDisposable;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::markers::{IMarkerService, MarkerCode, MarkerData, MarkerSeverity};
use je_platform::resources::uri_file;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::api::browser::main_thread_diagnostics::MainThreadDiagnostics;
use je_workbench::api::common::ext_host_protocol::{ExtHostDiagnosticsShape, IMainContext, IMarkerChange, MainThreadDiagnosticsShape};

type ChangedData = Rc<RefCell<Vec<Vec<(URI, Vec<MarkerData>)>>>>;

/// O objeto de `getProxy()`: guarda cada `$acceptMarkersChange`.
struct RecordingProxy {
    changed_data: ChangedData,
}

impl ExtHostDiagnosticsShape for RecordingProxy {
    fn rpc_accept_markers_change(&self, data: Vec<(URI, Vec<MarkerData>)>) {
        self.changed_data.borrow_mut().push(data);
    }
}

/// O `IExtHostContext` dos casos: qualquer identificador devolve o mesmo proxy.
struct ProxyContext {
    proxy: Rc<dyn ExtHostDiagnosticsShape>,
}

impl ProxyContext {
    fn new(changed_data: &ChangedData) -> Rc<dyn IMainContext> {
        Rc::new(Self { proxy: Rc::new(RecordingProxy { changed_data: changed_data.clone() }) })
    }
}

impl IMainContext for ProxyContext {
    fn get_proxy_erased(&self, _sid: &'static str) -> Rc<dyn Any> {
        Rc::new(self.proxy.clone())
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// `new class extends mock<IUriIdentityService>() { override asCanonicalUri(uri) { return uri; } }`.
struct IdentityUriService;

impl IUriIdentityService for IdentityUriService {
    fn ext_uri(&self) -> ExtUri {
        unreachable!("mock<IUriIdentityService>: ext_uri não é definido")
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// `markerService = new MarkerService()` do `setup`.
fn new_marker_service() -> Rc<MarkerService> {
    MarkerService::new()
}

fn new_main_thread_diagnostics(changed_data: &ChangedData, marker_service: &Rc<MarkerService>) -> Rc<MainThreadDiagnostics> {
    let marker_service: Rc<dyn IMarkerService> = marker_service.clone();
    MainThreadDiagnostics::new(&*ProxyContext::new(changed_data), marker_service, Rc::new(IdentityUriService))
}

/// `{ code: '666', startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 1, severity: 1, source: 'me', message }`.
fn marker_data(message: &str) -> MarkerData {
    let mut data = MarkerData::new(MarkerSeverity::HINT, message, 1, 1, 1, 1);
    data.code = Some(MarkerCode::Text("666".to_owned()));
    data.source = Some("me".to_owned());
    data
}

#[test]
fn clear_markers_on_dispose() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let marker_service = new_marker_service();

    let changed_data: ChangedData = Rc::default();
    let diag = new_main_thread_diagnostics(&changed_data, &marker_service);

    let resource = uri_file("a");
    diag.rpc_change_many("foo", vec![IMarkerChange { resource: resource.clone(), markers: Some(vec![marker_data("fffff")]), is_truncated: Some(true) }]);

    assert_eq!((marker_service.read(None).len(), marker_service.is_source_truncated(&resource)), (1, true));
    diag.dispose();
    assert_eq!((marker_service.read(None).len(), marker_service.is_source_truncated(&resource)), (0, false));

    marker_service.dispose();
}

#[test]
fn on_did_change_diagnostics_triggers_twice_on_same_diagnostics_136434() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let marker_service = new_marker_service();

    run_with_faked_timers(async move {
        let changed_data: ChangedData = Rc::default();

        let diag = new_main_thread_diagnostics(&changed_data, &marker_service);

        let target = uri_file("a");
        diag.rpc_change_many("foo", vec![IMarkerChange { resource: target.clone(), markers: Some(vec![marker_data("same_owner")]), is_truncated: None }]);
        marker_service.change_one("bar", &target, vec![marker_data("forgein_owner")], None);

        // added one marker via the API and one via the ext host. the latter must not
        // trigger an event to the extension host

        timeout(0).await;
        assert_eq!(marker_service.read(None).len(), 2);
        let changed_data = changed_data.borrow();
        assert_eq!(changed_data.len(), 1);
        assert_eq!(changed_data[0].len(), 1);
        assert_eq!(changed_data[0][0].1[0].message, "forgein_owner");

        diag.dispose();
        marker_service.dispose();
    });
}

#[test]
fn on_did_change_diagnostics_different_behavior_when_extension_kind_ui_running_on_remote_workspace_136955() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let marker_service = new_marker_service();

    run_with_faked_timers(async move {
        let target = uri_file("a");
        marker_service.change_one("bar", &target, vec![marker_data("message")], None);

        let changed_data: ChangedData = Rc::default();

        let diag = new_main_thread_diagnostics(&changed_data, &marker_service);

        diag.rpc_clear("bar");
        timeout(0).await;
        assert_eq!(marker_service.read(None).len(), 0);
        assert_eq!(changed_data.borrow().len(), 1);

        diag.dispose();
        marker_service.dispose();
    });
}
