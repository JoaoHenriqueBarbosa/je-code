//! Porte de `src/vs/workbench/api/test/browser/extHostDocumentsAndEditors.test.ts` (suíte
//! `ExtHostDocumentsAndEditors`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso. O caso `async`
//!   roda em `run_with_faked_timers`.
//! - `new TestRPCProtocol()` é o [`TestRpcProtocol`]: o `getProxy` do original devolve um proxy que repassa a
//!   chamada ao objeto registrado por `set`, depois de um `setTimeout(0)` e de serializar os argumentos; aqui o
//!   proxy é o próprio objeto registrado por `set`, sem fio (nenhum método do proxy é chamado neste caso).
//!   Registra-se o `MainThreadDocuments`, o único que o caso pede: um dublê que falha se for chamado.
//! - O `uri: URI.parse('foo:bar')` do `UriComponents` é o `URI` revivido, e o `EOL: '\n'` é `Eol::Lf`.
//! - `new Promise((resolve, reject) => { ... })` é o par de `Promise::new`, e o `try/catch/finally` do ouvinte é
//!   `errors::catch` seguido do `dispose` da inscrição (`d.dispose()`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::errors;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model::Eol;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CreateDocumentOptions, DocumentsAndEditorsDelta, ExtHostDocumentsAndEditorsShape, IMainContext, MainContext, MainThreadDocumentsShape, ModelAddedData,
    OpenDocumentOptions,
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

/// O `setup`: `new ExtHostDocumentsAndEditors(new TestRPCProtocol(), new NullLogService())`.
fn setup() -> ExtHostDocumentsAndEditors {
    let rpc: Rc<dyn IMainContext> = Rc::new(TestRpcProtocol::default());
    let proxy: Rc<dyn MainThreadDocumentsShape> = Rc::new(UnusedMainThreadDocuments);
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, proxy);
    ExtHostDocumentsAndEditors::new(rpc, NullLogService::shared())
}

#[test]
fn the_value_of_text_document_is_closed_is_incorrect_when_a_text_document_is_closed_27949() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let editors = setup();

    editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
        added_documents: Some(vec![ModelAddedData {
            eol: Eol::Lf,
            is_dirty: true,
            language_id: "fooLang".to_owned(),
            uri: URI::parse("foo:bar", false),
            version_id: 1,
            lines: vec![JsString::from("first"), JsString::from("second")],
            encoding: "utf8".to_owned(),
        }]),
        ..DocumentsAndEditorsDelta::default()
    });

    run_with_faked_timers(async move {
        let (promise, resolver) = Promise::<()>::new();
        let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::default();

        let d = editors.on_did_remove_documents().listen({
            let subscription = subscription.clone();
            move |documents| {
                let outcome = errors::catch(|| {
                    for data in documents {
                        assert!(data.document().is_closed());
                    }
                });
                match outcome {
                    Ok(()) => resolver.resolve(()),
                    Err(error) => resolver.reject(error),
                }
                if let Some(d) = subscription.borrow_mut().take() {
                    d.dispose();
                }
            }
        });
        *subscription.borrow_mut() = Some(d);

        editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
            removed_documents: Some(vec![URI::parse("foo:bar", false)]),
            ..DocumentsAndEditorsDelta::default()
        });

        promise.await.unwrap();
    });
}
