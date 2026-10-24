//! Porte de `src/vs/workbench/api/test/browser/extHostNotebookSerializer.test.ts` (suíte `ExtHostNotebookSerializer`), caso a caso e na mesma ordem.
//!
//! Os 4 casos estão portados.
//!
//! Notas de tradução:
//! - `new TestRPCProtocol()` é o [`TestMainContext`] montado por `support/ext_host_notebook_support.rs`; o `handles` do original é o
//!   `serializer_handles` do dublê de `MainThreadNotebook`. `ExtHostConsumerFileSystem` e `ExtHostSearch` não existem no porte (ver
//!   as notas do suporte).
//! - `assert.rejects(promessa, /NO serializer found/)` é a rejeição cuja mensagem contém o texto.
//! - `store.add(registration)` é `guard.add(...)`; o `store` do original, que vem de `ensureNoDisposablesAreLeakedInTestSuite()`, é o
//!   [`LeakGuard`], criado dentro do corpo de cada caso.
//! - `new Uint8Array([42])` é `vec![42]`; o `deserializeNotebook: () => result.p` é um serializador que devolve a promessa.

mod support;

use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::promise::{DeferredPromise, MaybePromise};
use je_workbench::api::common::ext_host_protocol::ExtHostNotebookShape;
use je_workbench::api::common::ext_host_notebook::NotebookSerializer;
use je_workbench::api::common::ext_host_types as types;
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_notebook_support::common::run_with_faked_timers;
use support::ext_host_notebook_support::{LeakGuard, NotebookHost, ensure_no_disposables_are_leaked_in_test_suite, new_notebook_host};

/// `{ deserializeNotebook: () => new NotebookData([]), serializeNotebook: () => new Uint8Array(bytes) }`.
struct FixedSerializer {
    bytes: Vec<u8>,
}

impl NotebookSerializer for FixedSerializer {
    fn deserialize_notebook(&self, _content: &[u8], _token: CancellationToken) -> MaybePromise<types::NotebookData> {
        MaybePromise::Value(types::NotebookData::new(Vec::new()))
    }

    fn serialize_notebook(&self, _data: &types::NotebookData, _token: CancellationToken) -> MaybePromise<Vec<u8>> {
        MaybePromise::Value(self.bytes.clone())
    }
}

/// `{ deserializeNotebook: () => result.p, serializeNotebook: () => new Uint8Array() }`.
struct DeferredSerializer {
    result: Rc<DeferredPromise<types::NotebookData>>,
}

impl NotebookSerializer for DeferredSerializer {
    fn deserialize_notebook(&self, _content: &[u8], _token: CancellationToken) -> MaybePromise<types::NotebookData> {
        MaybePromise::Promise(self.result.promise.clone())
    }

    fn serialize_notebook(&self, _data: &types::NotebookData, _token: CancellationToken) -> MaybePromise<Vec<u8>> {
        MaybePromise::Value(Vec::new())
    }
}

/// `createNotebooks()`: o `store` (o guarda) e o host, com o `ExtHostDocuments` registrado no `store`.
fn create_notebooks() -> (LeakGuard, NotebookHost) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let host = new_notebook_host();
    store.add_rc(host.documents.clone());
    (store, host)
}

fn handles(host: &NotebookHost) -> Vec<u32> {
    host.main_thread_notebook.serializer_handles.borrow().clone()
}

#[test]
fn disposed_serializers_cannot_deserialize_subsequent_requests() {
    run_with_faked_timers(async move {
        let (store, host) = create_notebooks();
        let registration = store.add(
            host.notebooks
                .register_notebook_serializer(&null_extension_description(), "test.notebook", Rc::new(FixedSerializer { bytes: Vec::new() }), None, None)
                .expect("registerNotebookSerializer"),
        );
        host.rpc.sync().await.expect("sync");
        let handles = handles(&host);
        let data = host.notebooks.rpc_data_to_notebook(handles[0], VSBuffer::alloc(0), CancellationToken::None).await.expect("$dataToNotebook");
        assert!(data.value.cells.is_empty());
        registration.dispose();
        let error = host.notebooks.rpc_data_to_notebook(handles[0], VSBuffer::alloc(0), CancellationToken::None).await.err().expect("deveria rejeitar");
        assert!(error.message.contains("NO serializer found"), "{}", error.message);
    });
}

#[test]
fn disposed_serializers_cannot_serialize_subsequent_requests() {
    run_with_faked_timers(async move {
        let (store, host) = create_notebooks();
        let registration = store.add(
            host.notebooks
                .register_notebook_serializer(&null_extension_description(), "test.notebook", Rc::new(FixedSerializer { bytes: vec![42] }), None, None)
                .expect("registerNotebookSerializer"),
        );
        host.rpc.sync().await.expect("sync");
        let handles = handles(&host);
        let data = host.notebooks.rpc_data_to_notebook(handles[0], VSBuffer::alloc(0), CancellationToken::None).await.expect("$dataToNotebook");
        let bytes = host.notebooks.rpc_notebook_to_data(handles[0], data.clone(), CancellationToken::None).await.expect("$notebookToData");
        assert_eq!(bytes.to_vec(), vec![42]);
        registration.dispose();
        let error = host.notebooks.rpc_notebook_to_data(handles[0], data, CancellationToken::None).await.err().expect("deveria rejeitar");
        assert!(error.message.contains("NO serializer found"), "{}", error.message);
    });
}

#[test]
fn disposing_one_serializer_leaves_another_registration_usable() {
    run_with_faked_timers(async move {
        let (store, host) = create_notebooks();
        let serializer: Rc<dyn NotebookSerializer> = Rc::new(FixedSerializer { bytes: vec![42] });
        let registration = store.add(
            host.notebooks.register_notebook_serializer(&null_extension_description(), "test.first", serializer.clone(), None, None).expect("registerNotebookSerializer"),
        );
        store.add(host.notebooks.register_notebook_serializer(&null_extension_description(), "test.second", serializer, None, None).expect("registerNotebookSerializer"));
        host.rpc.sync().await.expect("sync");
        let handles = handles(&host);
        registration.dispose();
        registration.dispose();
        let data = host.notebooks.rpc_data_to_notebook(handles[1], VSBuffer::alloc(0), CancellationToken::None).await.expect("$dataToNotebook");
        let bytes = host.notebooks.rpc_notebook_to_data(handles[1], data, CancellationToken::None).await.expect("$notebookToData");
        assert_eq!(bytes.to_vec(), vec![42]);
    });
}

#[test]
fn an_in_flight_deserialization_can_finish_without_reviving_a_disposed_serializer() {
    run_with_faked_timers(async move {
        let (store, host) = create_notebooks();
        let result = Rc::new(DeferredPromise::<types::NotebookData>::new());
        let registration = store.add(
            host.notebooks
                .register_notebook_serializer(&null_extension_description(), "test.notebook", Rc::new(DeferredSerializer { result: result.clone() }), None, None)
                .expect("registerNotebookSerializer"),
        );
        host.rpc.sync().await.expect("sync");
        let handles = handles(&host);
        let pending = host.notebooks.rpc_data_to_notebook(handles[0], VSBuffer::alloc(0), CancellationToken::None);
        registration.dispose();
        result.complete(types::NotebookData::new(Vec::new())).await.expect("complete");
        assert!(pending.await.expect("$dataToNotebook").value.cells.is_empty());
        let error = host.notebooks.rpc_data_to_notebook(handles[0], VSBuffer::alloc(0), CancellationToken::None).await.err().expect("deveria rejeitar");
        assert!(error.message.contains("NO serializer found"), "{}", error.message);
    });
}
