//! Porte de `src/vs/workbench/api/test/browser/extHostDiagnostics.test.ts` (suíte `ExtHostDiagnostics`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (`store` é o guarda).
//! - `class DiagnosticsShape extends mock<MainThreadDiagnosticsShape>()` é o [`DiagnosticsShape`], com os fechos opcionais que cada caso
//!   sobrescreve; o `mock<T>()` cru (que não tem método nenhum) é [`DiagnosticsShape::empty`], que falha com `panic!` ao ser chamado, e o
//!   `{ $clear }` do caso `#54359` é [`DiagnosticsShape::clear_only`]. `new Emitter()` é `Emitter::new()` e `emitter.event` é `emitter.event()`.
//! - `assert.throws(() => ...)` é `errors::catch(...).is_err()`. Os `undefined!` de `set` são `None`; `[uri, undefined!]` é `(uri, None)`; o
//!   array esparso (`new Array(3)`) é `Vec<Option<Diagnostic>>`. `Event.toPromise(...).then(...)` é `to_promise(None)` com o `await` e as
//!   conferências depois (o resultado é o mesmo, já que o `await p` do original espera as conferências).
//! - `immutable read`: o `Object.freeze` não tem equivalente (o `get` devolve uma cópia, e o tipo impede escrever na coleção); o caso confere que
//!   mexer na cópia (`clear`, `pop`, troca de elemento) não muda o que a coleção guarda, que é a garantia que o original testa.
//! - `diagnostics with related information` (`done`): o `$changeMany` do dublê confere o que recebeu e marca um flag, conferido depois do `set`.
//! - `new ExtHostDiagnostics(new class implements IMainContext ...)` é o `TestMainContext` com o `MainThreadDiagnostics` registrado, e
//!   `new class extends mock<IExtHostDocumentsAndEditors>() { getDocument() { return undefined } }` é um `ExtHostDocumentsAndEditors` real e vazio
//!   (que devolve `None` para qualquer documento). `ExtUri(uri => uri.scheme === 'insensitive')` é o `ExtHostFileSystemInfo` com o esquema
//!   `insensitive` registrado sem `PATH_CASE_SENSITIVE`, que o torna insensível a caixa.
//! - `runWithFakedTimers` é o `run_with_faked_timers` (o `DebounceEmitter` de 50 ms anda no tempo virtual).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::errors;
use je_base::event::Emitter;
use je_base::lifecycle::IDisposable;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_platform::files::files::FileSystemProviderCapabilities;
use je_platform::log::NullLogService;
use je_platform::markers::markers::{MarkerData, MarkerSeverity};
use je_platform::resources::ext_uri;
use je_workbench::api::common::ext_host_diagnostics::{DiagnosticCollection, ExtHostDiagnostics, UriChangeEmitter};
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_file_system_info::ExtHostFileSystemInfo;
use je_workbench::api::common::ext_host_protocol::{ExtHostDiagnosticsShape, ExtHostFileSystemInfoShape, IMainContext, IMarkerChange, MainContext, MainThreadDiagnosticsShape};
use je_workbench::api::common::ext_host_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Range};
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_workspace_support::TestMainContext;

type ChangeManyFn = Box<dyn Fn(&str, &[IMarkerChange])>;

/// `class DiagnosticsShape extends mock<MainThreadDiagnosticsShape>()`: `$changeMany` e `$clear` não fazem nada, a menos que o caso
/// os sobrescreva.
struct DiagnosticsShape {
    change_many: Option<ChangeManyFn>,
    clear: Option<Box<dyn Fn(&str)>>,
    /// O `mock` cru: um método sem fecho falha em vez de não fazer nada.
    missing_methods_fail: bool,
}

impl Default for DiagnosticsShape {
    fn default() -> Self {
        DiagnosticsShape { change_many: None, clear: None, missing_methods_fail: false }
    }
}

impl DiagnosticsShape {
    fn with_change_many(change_many: impl Fn(&str, &[IMarkerChange]) + 'static) -> DiagnosticsShape {
        DiagnosticsShape { change_many: Some(Box::new(change_many)), ..DiagnosticsShape::default() }
    }

    /// O `mock<MainThreadDiagnosticsShape>()` sem nada.
    fn empty() -> DiagnosticsShape {
        DiagnosticsShape { missing_methods_fail: true, ..DiagnosticsShape::default() }
    }

    /// `{ $clear(owner) { ... } }`.
    fn clear_only(clear: impl Fn(&str) + 'static) -> DiagnosticsShape {
        DiagnosticsShape { clear: Some(Box::new(clear)), missing_methods_fail: true, ..DiagnosticsShape::default() }
    }

    fn shared(self) -> Rc<dyn MainThreadDiagnosticsShape> {
        Rc::new(self)
    }
}

impl IDisposable for DiagnosticsShape {
    fn dispose(&self) {}
}

impl MainThreadDiagnosticsShape for DiagnosticsShape {
    fn rpc_change_many(&self, owner: &str, entries: Vec<IMarkerChange>) {
        match &self.change_many {
            Some(change_many) => change_many(owner, &entries),
            None if self.missing_methods_fail => panic!("$changeMany is not a function"),
            None => {}
        }
    }

    fn rpc_clear(&self, owner: &str) {
        match &self.clear {
            Some(clear) => clear(owner),
            None if self.missing_methods_fail => panic!("$clear is not a function"),
            None => {}
        }
    }
}

fn uri(text: &str) -> URI {
    URI::parse(text, false)
}

/// `URI.from({ scheme, path })`.
fn uri_from(scheme: &str, path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(scheme.into()), path: Some(path.into()), ..UriComponents::default() }, false)
}

fn range(start_line: f64, start_character: f64, end_line: f64, end_character: f64) -> Range {
    Range::from_coordinates(start_line, start_character, end_line, end_character).expect("intervalo válido")
}

fn diagnostic(range: Range, message: &str) -> Diagnostic {
    Diagnostic::new(range, message, None).expect("mensagem válida")
}

fn diagnostic_with_severity(range: Range, message: &str, severity: DiagnosticSeverity) -> Diagnostic {
    Diagnostic::new(range, message, Some(severity)).expect("mensagem válida")
}

fn version_provider(_uri: &URI) -> Option<i64> {
    None
}

fn emitter() -> Rc<Emitter<Vec<URI>>> {
    Emitter::new()
}

fn collection_with(
    max_total: usize,
    max_per_file: usize,
    version_provider: impl Fn(&URI) -> Option<i64> + 'static,
    proxy: DiagnosticsShape,
    emitter: Rc<dyn UriChangeEmitter>,
) -> DiagnosticCollection {
    DiagnosticCollection::new("test", "test", max_total, max_per_file, version_provider, ext_uri(), Some(proxy.shared()), emitter)
}

/// `new DiagnosticCollection('test', 'test', 100, 100, versionProvider, extUri, new DiagnosticsShape(), new Emitter())`.
fn default_collection() -> DiagnosticCollection {
    collection_with(100, 100, version_provider, DiagnosticsShape::default(), emitter())
}

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn file_system_info() -> Rc<ExtHostFileSystemInfo> {
    Rc::new(ExtHostFileSystemInfo::new())
}

/// O `new class implements IMainContext` dos casos que criam um `ExtHostDiagnostics`.
fn main_context(shape: DiagnosticsShape) -> Rc<TestMainContext> {
    let context = Rc::new(TestMainContext::new());
    context.set(&MainContext::MAIN_THREAD_DIAGNOSTICS, shape.shared());
    context
}

/// `new ExtHostDiagnostics(context, new NullLogService(), fileSystemInfoService, <documents que não têm documento>)`.
fn ext_host_diagnostics(context: &Rc<TestMainContext>, file_system_info: Rc<ExtHostFileSystemInfo>) -> ExtHostDiagnostics {
    let documents = Rc::new(ExtHostDocumentsAndEditors::new(context.clone() as Rc<dyn IMainContext>, NullLogService::shared()));
    ExtHostDiagnostics::new(&**context, NullLogService::shared(), file_system_info, documents)
}

#[test]
fn dispose_check() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let collection = default_collection();

    collection.dispose();
    collection.dispose(); // that's OK
    assert!(errors::catch(|| collection.name().to_owned()).is_err());
    assert!(errors::catch(|| collection.clear()).is_err());
    assert!(errors::catch(|| collection.delete(&uri("aa:bb"))).is_err());
    assert!(errors::catch(|| collection.for_each(|_, _, _| {})).is_err());
    assert!(errors::catch(|| collection.get(&uri("aa:bb"))).is_err());
    assert!(errors::catch(|| collection.has(&uri("aa:bb"))).is_err());
    assert!(errors::catch(|| collection.set(&uri("aa:bb"), Some(Vec::<Diagnostic>::new()))).is_err());
    assert!(errors::catch(|| collection.set(&uri("aa:bb"), None::<Vec<Diagnostic>>)).is_err());
}

#[test]
fn diagnostic_collection_for_each_clear_has() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let mut collection = default_collection();
    assert_eq!(collection.name(), "test");
    collection.dispose();
    assert!(errors::catch(|| collection.name().to_owned()).is_err());

    let mut c = 0;
    collection = default_collection();
    collection.for_each(|_, _, _| c += 1);
    assert_eq!(c, 0);

    collection.set(&uri("foo:bar"), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "message-1"), diagnostic(range(0.0, 0.0, 1.0, 1.0), "message-2")]));
    collection.for_each(|_, _, _| c += 1);
    assert_eq!(c, 1);

    c = 0;
    collection.clear();
    collection.for_each(|_, _, _| c += 1);
    assert_eq!(c, 0);

    collection.set(&uri("foo:bar1"), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "message-1"), diagnostic(range(0.0, 0.0, 1.0, 1.0), "message-2")]));
    collection.set(&uri("foo:bar2"), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "message-1"), diagnostic(range(0.0, 0.0, 1.0, 1.0), "message-2")]));
    collection.for_each(|_, _, _| c += 1);
    assert_eq!(c, 2);

    assert!(collection.has(&uri("foo:bar1")));
    assert!(collection.has(&uri("foo:bar2")));
    assert!(!collection.has(&uri("foo:bar3")));
    collection.delete(&uri("foo:bar1"));
    assert!(!collection.has(&uri("foo:bar1")));

    collection.dispose();
}

#[test]
fn diagnostic_collection_immutable_read() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let collection = default_collection();
    collection.set(&uri("foo:bar"), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "message-1"), diagnostic(range(0.0, 0.0, 1.0, 1.0), "message-2")]));

    // O que `get` devolve é uma cópia: mexer nela não muda o que a coleção guarda.
    let mut array = collection.get(&uri("foo:bar"));
    array.clear();
    assert_eq!(collection.get(&uri("foo:bar")).len(), 2);
    let mut array = collection.get(&uri("foo:bar"));
    array.pop();
    assert_eq!(collection.get(&uri("foo:bar")).len(), 2);
    let mut array = collection.get(&uri("foo:bar"));
    array[0] = diagnostic(range(0.0, 0.0, 0.0, 0.0), "evil");
    assert_eq!(collection.get(&uri("foo:bar"))[0].message, "message-1");

    collection.for_each(|_uri, array, _collection| {
        // `array` é `&[Diagnostic]`: o tipo não deixa escrever. Uma cópia mexida também não chega na coleção.
        let mut copy = array.to_vec();
        copy.clear();
        copy.pop();
        assert_eq!(array.len(), 2);
    });

    let array = collection.get(&uri("foo:bar"));
    assert_eq!(array.len(), 2);

    collection.dispose();
}

#[test]
fn diagnostics_collection_set_with_duplicated_tuples() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let collection = default_collection();
    let uri_ = uri("sc:hightower");
    collection.set_many(vec![
        (uri_.clone(), Some(vec![diagnostic(range(0.0, 0.0, 0.0, 1.0), "message-1")])),
        (uri("some:thing"), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "something")])),
        (uri_.clone(), Some(vec![diagnostic(range(0.0, 0.0, 0.0, 1.0), "message-2")])),
    ]);

    let array = collection.get(&uri_);
    assert_eq!(array.len(), 2);
    assert_eq!(array[0].message, "message-1");
    assert_eq!(array[1].message, "message-2");

    // clear
    collection.delete(&uri_);
    assert!(!collection.has(&uri_));

    // bad tuple clears 1/2
    collection.set_many(vec![
        (uri_.clone(), Some(vec![diagnostic(range(0.0, 0.0, 0.0, 1.0), "message-1")])),
        (uri("some:thing"), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "something")])),
        (uri_.clone(), None),
    ]);
    assert!(!collection.has(&uri_));

    // clear
    collection.delete(&uri_);
    assert!(!collection.has(&uri_));

    // bad tuple clears 2/2
    collection.set_many(vec![
        (uri_.clone(), Some(vec![diagnostic(range(0.0, 0.0, 0.0, 1.0), "message-1")])),
        (uri("some:thing"), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "something")])),
        (uri_.clone(), None),
        (uri_.clone(), Some(vec![diagnostic(range(0.0, 0.0, 0.0, 1.0), "message-2")])),
        (uri_.clone(), Some(vec![diagnostic(range(0.0, 0.0, 0.0, 1.0), "message-3")])),
    ]);

    let array = collection.get(&uri_);
    assert_eq!(array.len(), 2);
    assert_eq!(array[0].message, "message-2");
    assert_eq!(array[1].message, "message-3");

    collection.dispose();
}

#[test]
fn diagnostics_collection_set_tuple_overrides_11547() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let last_entries: Rc<RefCell<Option<Vec<IMarkerChange>>>> = Rc::default();
    let captured = last_entries.clone();
    let collection = collection_with(
        100,
        100,
        version_provider,
        DiagnosticsShape::with_change_many(move |_owner, entries| *captured.borrow_mut() = Some(entries.to_vec())),
        emitter(),
    );
    let uri_ = uri("sc:hightower");

    collection.set_many(vec![(uri_.clone(), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "error")]))]);
    assert_eq!(collection.get(&uri_).len(), 1);
    assert_eq!(collection.get(&uri_)[0].message, "error");
    {
        let entries = last_entries.borrow();
        let entries = entries.as_ref().expect("recebeu a mudança");
        assert_eq!(entries.len(), 1);
        let data1 = entries[0].markers.as_ref().expect("marcadores");
        assert_eq!(data1.len(), 1);
        assert_eq!(data1[0].message, "error");
    }
    *last_entries.borrow_mut() = None;

    collection.set_many(vec![(uri_.clone(), Some(vec![diagnostic(range(0.0, 0.0, 1.0, 1.0), "warning")]))]);
    assert_eq!(collection.get(&uri_).len(), 1);
    assert_eq!(collection.get(&uri_)[0].message, "warning");
    {
        let entries = last_entries.borrow();
        let entries = entries.as_ref().expect("recebeu a mudança");
        assert_eq!(entries.len(), 1);
        let data2 = entries[0].markers.as_ref().expect("marcadores");
        assert_eq!(data2.len(), 1);
        assert_eq!(data2[0].message, "warning");
    }
    *last_entries.borrow_mut() = None;
}

#[test]
fn do_send_message_when_not_making_a_change() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let change_count = Rc::new(Cell::new(0));
    let event_count = Rc::new(Cell::new(0));

    let emitter = emitter();
    let events = event_count.clone();
    store.add(emitter.event().listen(move |_| events.set(events.get() + 1)));
    let changes = change_count.clone();
    let collection = collection_with(100, 100, version_provider, DiagnosticsShape::with_change_many(move |_, _| changes.set(changes.get() + 1)), emitter.clone());

    let uri_ = uri("sc:hightower");
    let diag = diagnostic(range(0.0, 0.0, 0.0, 1.0), "ffff");

    collection.set(&uri_, Some(vec![diag.clone()]));
    assert_eq!(change_count.get(), 1);
    assert_eq!(event_count.get(), 1);

    collection.set(&uri_, Some(vec![diag]));
    assert_eq!(change_count.get(), 2);
    assert_eq!(event_count.get(), 2);
}

#[test]
fn diagnostics_collection_tuples_and_undefined_small_array_15585() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let collection = default_collection();
    let uri_ = uri("sc:hightower");
    let uri2 = uri("sc:nomad");
    let diag = diagnostic(range(0.0, 0.0, 0.0, 1.0), "ffff");

    collection.set_many(vec![
        (uri_.clone(), Some(vec![diag.clone(), diag.clone(), diag.clone()])),
        (uri_.clone(), None),
        (uri_.clone(), Some(vec![diag.clone()])),
        (uri2.clone(), Some(vec![diag.clone(), diag.clone()])),
        (uri2.clone(), None),
        (uri2.clone(), Some(vec![diag.clone()])),
    ]);

    assert_eq!(collection.get(&uri_).len(), 1);
    assert_eq!(collection.get(&uri2).len(), 1);
}

#[test]
fn diagnostics_collection_tuples_and_undefined_large_array_15585() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let collection = default_collection();
    let mut tuples: Vec<(URI, Option<Vec<Diagnostic>>)> = Vec::new();

    for i in 0..500 {
        let uri_ = uri(&format!("sc:hightower#{i}"));
        let diag = diagnostic(range(0.0, 0.0, 0.0, 1.0), &i.to_string());

        tuples.push((uri_.clone(), Some(vec![diag.clone(), diag.clone(), diag.clone()])));
        tuples.push((uri_.clone(), None));
        tuples.push((uri_, Some(vec![diag])));
    }

    collection.set_many(tuples);

    for i in 0..500 {
        let uri_ = uri(&format!("sc:hightower#{i}"));
        assert!(collection.has(&uri_));
        assert_eq!(collection.get(&uri_).len(), 1);
    }
}

#[test]
fn diagnostic_capping_max_per_file() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let last_entries: Rc<RefCell<Option<Vec<IMarkerChange>>>> = Rc::default();
    let captured = last_entries.clone();
    let collection = collection_with(
        100,
        250,
        version_provider,
        DiagnosticsShape::with_change_many(move |_owner, entries| *captured.borrow_mut() = Some(entries.to_vec())),
        emitter(),
    );
    let uri_ = uri("aa:bb");

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for i in 0..500 {
        let i = f64::from(i);
        diagnostics.push(diagnostic_with_severity(
            range(i, 0.0, i + 1.0, 0.0),
            &format!("error#{i}"),
            if i < 300.0 { DiagnosticSeverity::Warning } else { DiagnosticSeverity::Error },
        ));
    }

    collection.set(&uri_, Some(diagnostics));
    assert_eq!(collection.get(&uri_).len(), 500);
    let entries = last_entries.borrow();
    let entries = entries.as_ref().expect("recebeu a mudança");
    assert_eq!(entries.len(), 1);
    let markers = entries[0].markers.as_ref().expect("marcadores");
    assert_eq!(markers.len(), 251);
    assert_eq!(markers[0].severity, MarkerSeverity::ERROR);
    assert_eq!(markers[200].severity, MarkerSeverity::WARNING);
    assert_eq!(markers[250].severity, MarkerSeverity::INFO);
    assert_eq!(entries[0].is_truncated, Some(true));
}

#[test]
fn diagnostic_capping_max_files() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let last_entries: Rc<RefCell<Option<Vec<IMarkerChange>>>> = Rc::default();
    let captured = last_entries.clone();
    let collection = collection_with(
        2,
        1,
        version_provider,
        DiagnosticsShape::with_change_many(move |_owner, entries| *captured.borrow_mut() = Some(entries.to_vec())),
        emitter(),
    );

    let diag = diagnostic(range(0.0, 0.0, 1.0, 1.0), "Hello");

    collection.set_many(vec![
        (uri("aa:bb1"), Some(vec![diag.clone()])),
        (uri("aa:bb2"), Some(vec![diag.clone()])),
        (uri("aa:bb3"), Some(vec![diag.clone()])),
        (uri("aa:bb4"), Some(vec![diag.clone()])),
    ]);
    assert_eq!(last_entries.borrow().as_ref().expect("recebeu a mudança").len(), 3); // goes above the limit and then stops
}

#[test]
fn diagnostic_eventing() {
    run_with_faked_timers(async {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();

        let emitter = emitter();
        let collection = DiagnosticCollection::new("ddd", "test", 100, 100, version_provider, ext_uri(), Some(DiagnosticsShape::default().shared()), emitter.clone());

        let diag1 = diagnostic(range(1.0, 1.0, 2.0, 3.0), "diag1");
        let diag2 = diagnostic(range(1.0, 1.0, 2.0, 3.0), "diag2");
        let diag3 = diagnostic(range(1.0, 1.0, 2.0, 3.0), "diag3");

        let p = emitter.event().to_promise(None);
        collection.set(&uri("aa:bb"), Some(Vec::<Diagnostic>::new()));
        let a = p.promise.clone().await.unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(text(&a[0]), "aa:bb");

        let p = emitter.event().to_promise(None);
        collection.set_many(vec![(uri("aa:bb"), Some(vec![diag1])), (uri("aa:cc"), Some(vec![diag2, diag3]))]);
        let e = p.promise.clone().await.unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(text(&e[0]), "aa:bb");
        assert_eq!(text(&e[1]), "aa:cc");

        let p = emitter.event().to_promise(None);
        collection.clear();
        let e = p.promise.clone().await.unwrap();
        assert_eq!(e.len(), 2);
    });
}

#[test]
fn languages_on_did_change_diagnostics_does_not_provide_document_uri_49582() {
    run_with_faked_timers(async {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();

        let emitter = emitter();
        let collection = DiagnosticCollection::new("ddd", "test", 100, 100, version_provider, ext_uri(), Some(DiagnosticsShape::default().shared()), emitter.clone());

        let diag1 = diagnostic(range(1.0, 1.0, 2.0, 3.0), "diag1");

        // delete
        collection.set(&uri("aa:bb"), Some(vec![diag1.clone()]));
        let p = emitter.event().to_promise(None);
        collection.delete(&uri("aa:bb"));
        let e = p.promise.clone().await.unwrap();
        assert_eq!(text(&e[0]), "aa:bb");

        // set->undefined (as delete)
        collection.set(&uri("aa:bb"), Some(vec![diag1]));
        let p = emitter.event().to_promise(None);
        collection.set(&uri("aa:bb"), None::<Vec<Diagnostic>>);
        let e = p.promise.clone().await.unwrap();
        assert_eq!(text(&e[0]), "aa:bb");
    });
}

#[test]
fn diagnostics_with_related_information() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    let collection = collection_with(
        100,
        100,
        version_provider,
        DiagnosticsShape::with_change_many(move |_owner, entries| {
            assert_eq!(entries.len(), 1);
            let data = entries[0].markers.as_ref().expect("marcadores");
            assert_eq!(data.len(), 1);

            let diag = &data[0];
            let related = diag.related_information.as_ref().expect("informação relacionada");
            assert_eq!(related.len(), 2);
            assert_eq!(related[0].message, "more1");
            assert_eq!(related[1].message, "more2");
            finished.set(true);
        }),
        emitter(),
    );

    let mut diag = diagnostic(range(0.0, 0.0, 1.0, 1.0), "Foo");
    diag.related_information = Some(vec![
        DiagnosticRelatedInformation::new(Location::new(Some(uri("cc:dd")), Some(range(0.0, 0.0, 0.0, 0.0).into())), "more1"),
        DiagnosticRelatedInformation::new(Location::new(Some(uri("cc:ee")), Some(range(0.0, 0.0, 0.0, 0.0).into())), "more2"),
    ]);

    collection.set(&uri("aa:bb"), Some(vec![diag]));
    assert!(done.get());
}

#[test]
fn vscode_languages_get_diagnostics_appears_to_return_old_diagnostics_in_some_circumstances_54359() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let owner_history: Rc<RefCell<Vec<String>>> = Rc::default();
    let history = owner_history.clone();
    let context = main_context(DiagnosticsShape::clear_only(move |owner| history.borrow_mut().push(owner.to_owned())));
    let diags = ext_host_diagnostics(&context, file_system_info());

    let collection1 = diags.create_diagnostic_collection(&null_extension_description().identifier, Some("foo"));
    let collection2 = diags.create_diagnostic_collection(&null_extension_description().identifier, Some("foo")); // warns, uses a different owner

    collection1.clear();
    collection2.clear();

    let owner_history = owner_history.borrow();
    assert_eq!(owner_history.len(), 2);
    assert_eq!(owner_history[0], "foo");
    assert_eq!(owner_history[1], "foo0");
}

#[test]
fn error_updating_diagnostics_from_extension_60394() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let call_count = Rc::new(Cell::new(0));
    let calls = call_count.clone();
    let collection = collection_with(100, 100, version_provider, DiagnosticsShape::with_change_many(move |_, _| calls.set(calls.get() + 1)), emitter());

    let mut array: Vec<Diagnostic> = Vec::new();
    let diag1 = diagnostic(range(0.0, 0.0, 1.0, 1.0), "Foo");
    let diag2 = diagnostic(range(0.0, 0.0, 1.0, 1.0), "Bar");

    array.push(diag1);
    array.push(diag2.clone());

    collection.set(&uri("test:me"), Some(array.clone()));
    assert_eq!(call_count.get(), 1);

    collection.set(&uri("test:me"), Some(array.clone()));
    assert_eq!(call_count.get(), 2); // equal array

    array.push(diag2);
    collection.set(&uri("test:me"), Some(array));
    assert_eq!(call_count.get(), 3); // same but un-equal array
}

#[test]
fn get_diagnostics_does_not_tolerate_sparse_diagnostic_arrays() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let context = main_context(DiagnosticsShape::default());
    let diags = ext_host_diagnostics(&context, file_system_info());

    let collection = diags.create_diagnostic_collection(&null_extension_description().identifier, Some("sparse"));
    let uri_ = uri("sparse:uri");
    let diag = diagnostic(range(0.0, 0.0, 0.0, 0.0), "holey");
    let mut sparse_diagnostics: Vec<Option<Diagnostic>> = vec![None; 3];
    sparse_diagnostics[1] = Some(diag);

    collection.set(&uri_, Some(sparse_diagnostics));

    let result = diags.get_diagnostics(&uri_);
    assert_eq!(result.len(), 1);
    // `Vec<Diagnostic>` não tem buracos: o `coalesce` do `set` os tirou.
}

#[test]
fn version_id_is_set_whenever_possible() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let all: Rc<RefCell<Vec<IMarkerChange>>> = Rc::default();
    let captured = all.clone();
    let collection = collection_with(
        100,
        100,
        |_uri| Some(7),
        DiagnosticsShape::with_change_many(move |_owner, entries| captured.borrow_mut().extend_from_slice(entries)),
        emitter(),
    );

    let mut array: Vec<Diagnostic> = Vec::new();
    let diag1 = diagnostic(range(0.0, 0.0, 1.0, 1.0), "Foo");
    let diag2 = diagnostic(range(0.0, 0.0, 1.0, 1.0), "Bar");

    array.push(diag1.clone());
    array.push(diag2.clone());

    collection.set(&uri("test:one"), Some(array));
    collection.set(&uri("test:two"), Some(vec![diag1]));
    collection.set(&uri("test:three"), Some(vec![diag2]));

    let all_versions: Vec<Option<i64>> = all
        .borrow()
        .iter()
        .flat_map(|tuple| tuple.markers.as_ref().expect("marcadores").iter().map(|marker: &MarkerData| marker.model_version_id))
        .collect();
    assert_eq!(all_versions, vec![Some(7), Some(7), Some(7), Some(7)]);
}

#[test]
fn diagnostics_created_by_tasks_arent_accessible_to_extensions_47292() {
    run_with_faked_timers(async {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();

        // `getProxy` devolve `{}`: o espelho não tem proxy, então nada é chamado.
        let context = main_context(DiagnosticsShape::empty());
        let diags = ext_host_diagnostics(&context, file_system_info());

        //
        let uri_ = uri("foo:bar");
        let data = vec![MarkerData::new(MarkerSeverity::INFO, "message", 1, 1, 1, 1)];

        let p1 = diags.on_did_change_diagnostics.to_promise(None);
        diags.rpc_accept_markers_change(vec![(uri_.clone(), data)]);
        p1.promise.clone().await.unwrap();
        assert_eq!(diags.get_diagnostics(&uri_).len(), 1);

        let p2 = diags.on_did_change_diagnostics.to_promise(None);
        diags.rpc_accept_markers_change(vec![(uri_.clone(), Vec::new())]);
        p2.promise.clone().await.unwrap();
        assert_eq!(diags.get_diagnostics(&uri_).len(), 0);
    });
}

#[test]
fn languages_get_diagnostics_doesnt_handle_case_insensitivity_correctly_128198() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    // `new ExtUri(uri => uri.scheme === 'insensitive')`: o esquema registrado sem `PATH_CASE_SENSITIVE` ignora a caixa.
    let file_system_info = file_system_info();
    file_system_info.rpc_accept_provider_infos(&uri_from("insensitive", "/"), Some(FileSystemProviderCapabilities::NONE));

    let context = main_context(DiagnosticsShape::default());
    let diags = ext_host_diagnostics(&context, file_system_info);

    let col = diags.create_diagnostic_collection(&null_extension_description().identifier, None);

    let uri_sensitive = uri_from("foo", "/SOME/path");
    let uri_sensitive_case_b = uri_sensitive.with(&UriChange { path: Field::Set(uri_sensitive.path().to_upper_case()), ..UriChange::default() });

    let uri_insensitive = uri_from("insensitive", "/SOME/path");
    let uri_insensitive_upper = uri_insensitive.with(&UriChange { path: Field::Set(uri_insensitive.path().to_upper_case()), ..UriChange::default() });

    col.set(&uri_sensitive, Some(vec![diagnostic(range(0.0, 0.0, 0.0, 0.0), "sensitive")]));
    col.set(&uri_insensitive, Some(vec![diagnostic(range(0.0, 0.0, 0.0, 0.0), "insensitive")]));

    // collection itself honours casing
    assert_eq!(col.get(&uri_sensitive).len(), 1);
    assert_eq!(col.get(&uri_sensitive_case_b).len(), 0);

    assert_eq!(col.get(&uri_insensitive).len(), 1);
    assert_eq!(col.get(&uri_insensitive_upper).len(), 1);

    // languages.getDiagnostics honours casing
    assert_eq!(diags.get_diagnostics(&uri_sensitive).len(), 1);
    assert_eq!(diags.get_diagnostics(&uri_sensitive_case_b).len(), 0);

    assert_eq!(diags.get_diagnostics(&uri_insensitive).len(), 1);
    assert_eq!(diags.get_diagnostics(&uri_insensitive_upper).len(), 1);

    let mut from_for_each: Vec<URI> = Vec::new();
    col.for_each(|uri, _, _| from_for_each.push(uri.clone()));
    assert_eq!(from_for_each.len(), 2);
    assert_eq!(text(&from_for_each[0]), text(&uri_sensitive));
    assert_eq!(text(&from_for_each[1]), text(&uri_insensitive));
}
