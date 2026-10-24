//! Porte de `src/vs/workbench/api/test/browser/extHostDocumentContentProvider.test.ts` (suíte
//! `ExtHostDocumentContentProvider`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso, e o `runWithFakedTimers({}, ...)`
//!   é `run_with_faked_timers`.
//! - `SingleProxyRPCProtocol(mainThreadContentProvider)` devolve o mesmo objeto a qualquer proxy; o [`IMainContext`] do
//!   porte entrega o proxy pela forma pedida, então o [`TestRpcProtocol`] registra o dublê da thread principal em
//!   `MainThreadDocumentContentProviders` e, em `MainThreadDocuments`, o dublê que a criação do documento pede (falha se
//!   for chamado).
//! - O `MainThreadDocumentContentProvidersShape` do original (uma classe anônima) é o [`MainThreadContentProvider`]:
//!   `$onVirtualDocumentChange` espera `timeout(10)` e guarda o par `[uri, value]` em `changes` (compartilhado por `Rc`, e
//!   novo a cada `setup`, que no original zera o vetor); o `dispose` lança `Method not implemented.`. O `UriComponents`
//!   de `changes` é a [`URI`] e o `value` é [`JsString`].
//! - `provideTextDocumentContent` (função `async` do original, com `try/finally`) é o [`CountingProvider`]: o `finally`
//!   `stack--` roda depois do cálculo do valor, mesmo que a espera falhe; o `assert.strictEqual(stack, 0)` é `assert_eq!`
//!   dentro da função `async` (a falha rejeita a promessa, como no original).
//! - `new Emitter<URI>()` é `Emitter::new()`, e `emitter.event` é o `on_did_change` do provedor.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::model::Eol;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_document_content_providers::{ExtHostDocumentContentProvider, TextDocumentContentProvider};
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CreateDocumentOptions, DocumentsAndEditorsDelta, ExtHostDocumentsAndEditorsShape, IMainContext, MainContext, MainThreadDocumentContentProvidersShape,
    MainThreadDocumentsShape, ModelAddedData, OpenDocumentOptions,
};

/// `TestRPCProtocol`: os proxies são os objetos registrados por `set`.
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

/// O `MainThreadDocuments` que nenhum método do caso chama.
struct UnusedMainThreadDocuments;

impl IDisposable for UnusedMainThreadDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for UnusedMainThreadDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("o proxy não deveria ser chamado");
    }
}

impl MainThreadDocumentsShape for UnusedMainThreadDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }
}

type Changes = Rc<RefCell<Vec<(URI, JsString)>>>;

/// `new class implements MainThreadDocumentContentProvidersShape { ... }`.
struct MainThreadContentProvider {
    changes: Changes,
}

impl MainThreadDocumentContentProvidersShape for MainThreadContentProvider {
    fn rpc_register_text_content_provider(&self, _handle: u64, _scheme: &str) {}

    fn rpc_unregister_text_content_provider(&self, _handle: u64) {}

    fn rpc_on_virtual_document_change(&self, uri: &URI, value: &JsString) -> Promise<()> {
        let changes = self.changes.clone();
        let (uri, value) = (uri.clone(), value.clone());
        async_fn(async move {
            timeout(10).await?;
            changes.borrow_mut().push((uri, value));
            Ok(())
        })
    }
}

impl IDisposable for MainThreadContentProvider {
    fn dispose(&self) {
        errors::throw(Error::new("Method not implemented."));
    }
}

/// O `setup`: o documento `foo:bar` (versão 1, linhas `['foo']`) e o `ExtHostDocumentContentProvider` sobre ele.
fn setup() -> (Rc<ExtHostDocumentContentProvider>, Changes) {
    let changes: Changes = Rc::default();

    let rpc: Rc<dyn IMainContext> = Rc::new(TestRpcProtocol::default());
    let main_thread_content_provider: Rc<dyn MainThreadDocumentContentProvidersShape> = Rc::new(MainThreadContentProvider { changes: changes.clone() });
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENT_CONTENT_PROVIDERS, main_thread_content_provider);
    let documents: Rc<dyn MainThreadDocumentsShape> = Rc::new(UnusedMainThreadDocuments);
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, documents);

    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc.clone(), NullLogService::shared()));
    documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
        added_documents: Some(vec![ModelAddedData {
            is_dirty: false,
            language_id: "foo".to_owned(),
            uri: resource(),
            version_id: 1,
            lines: vec![JsString::from("foo")],
            eol: Eol::Lf,
            encoding: "utf8".to_owned(),
        }]),
        ..DocumentsAndEditorsDelta::default()
    });
    let document_content_provider = ExtHostDocumentContentProvider::new(&*rpc, documents_and_editors, NullLogService::shared());
    (document_content_provider, changes)
}

/// `URI.parse('foo:bar')`.
fn resource() -> URI {
    URI::parse("foo:bar", false)
}

/// O provedor do caso: lança o evento do `emitter`, devolve `X` e `Y` alternados e confere que nunca há duas chamadas
/// ao mesmo tempo (`stack`).
struct CountingProvider {
    emitter: Rc<Emitter<URI>>,
    contents: [&'static str; 2],
    counter: Rc<Cell<usize>>,
    stack: Rc<Cell<i32>>,
}

impl TextDocumentContentProvider for CountingProvider {
    fn on_did_change(&self) -> Option<Event<URI>> {
        Some(self.emitter.event())
    }

    fn provide_text_document_content(&self, _uri: &URI, _token: CancellationToken) -> MaybePromise<Option<JsString>> {
        let (contents, counter, stack) = (self.contents, self.counter.clone(), self.stack.clone());
        MaybePromise::Promise(async_fn(async move {
            assert_eq!(stack.get(), 0);
            stack.set(stack.get() + 1);
            let waited = timeout(0).await;
            let value = waited.map(|()| {
                let index = counter.get();
                counter.set(index + 1);
                contents[index % contents.len()]
            });
            stack.set(stack.get() - 1);
            Ok(Some(JsString::from(value?)))
        }))
    }
}

#[test]
fn text_document_content_provider_drops_on_did_change_events_when_they_happen_quickly_179711() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (document_content_provider, changes) = setup();

    run_with_faked_timers(async move {
        let emitter = Emitter::<URI>::new();
        let contents = ["X", "Y"];
        let counter = Rc::new(Cell::new(0));

        let stack = Rc::new(Cell::new(0));

        let d = document_content_provider
            .register_text_document_content_provider(
                &resource().scheme().to_string_lossy(),
                Rc::new(CountingProvider { emitter: emitter.clone(), contents, counter: counter.clone(), stack: stack.clone() }),
            )
            .unwrap();

        emitter.fire(resource());
        emitter.fire(resource());

        timeout(100).await.unwrap();

        assert_eq!(changes.borrow().len(), 2);
        assert_eq!(changes.borrow()[0].1, "X");
        assert_eq!(changes.borrow()[1].1, "Y");

        d.dispose();
    });
}
