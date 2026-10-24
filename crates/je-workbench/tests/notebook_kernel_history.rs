//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookKernelHistory.test.ts` (suíte `NotebookKernelHistoryService`), caso a caso e na mesma ordem. Os 2 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso; o `setup` (o `DisposableStore`, o `INotebookService` e o
//!   `IMenuService` mockados e o `NotebookKernelService`) é [`Suite::new`] e o `teardown` é o `Drop`. O `INotebookService`, o `IMenuService`
//!   e o `TestNotebookKernel` são os de `support/notebook_services_support.rs`, e o `NotebookKernelService` vem de `create_kernel_service`.
//! - O `IStorageService` mockado, cujo `get('notebook.kernelHistory')` devolve um JSON fixo e o resto não faz nada, é o `InMemoryStorageService`
//!   com essa chave gravada antes de criar o serviço (a leitura do serviço é a mesma). O `INotebookLoggingService` mockado
//!   (`info`/`debug` vazios) é o `NullNotebookLoggingService`.
//! - O `id` aleatório do `TestNotebookKernel` (`Math.random() + 'kernel'`) é um contador. `assert.deepStrictEqual(info.all, [k3, k2])`
//!   compara os ids dos kernels (a identidade do objeto é a do `Rc`).
//! - `{ uri: u1, notebookType: 'foo' }` é o [`NotebookTextModelLike`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::uri::URI;
use je_platform::storage::{InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_workbench::contrib::notebook::browser::services::notebook_kernel_history_service_impl::NotebookKernelHistoryService;
use je_workbench::contrib::notebook::browser::services::notebook_kernel_service_impl::NotebookKernelService;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_kernel_service::{
    INotebookKernel, INotebookKernelHistoryService, INotebookKernelService, NotebookTextModelLike,
};
use je_workbench::contrib::notebook::common::notebook_service::INotebookService;
use support::notebook_services_support::{NotebookServiceMock, TestNotebookKernel, TestNotebookKernelOptions, create_kernel_service};
use support::test_notebook_editor::NullNotebookLoggingService;

thread_local! {
    static KERNEL_COUNTER: Cell<u32> = const { Cell::new(0) };
}

/// `new TestNotebookKernel({ label, notebookType })`, com o `id` aleatório do original.
fn kernel(label: &'static str, notebook_type: &'static str) -> Rc<TestNotebookKernel> {
    let id = KERNEL_COUNTER.with(|counter| {
        counter.set(counter.get() + 1);
        format!("{}kernel", counter.get())
    });
    TestNotebookKernel::new(TestNotebookKernelOptions { id: Some(id), label: Some(label), view_type: Some(notebook_type), ..Default::default() })
}

fn like(uri: &URI, notebook_type: &str) -> NotebookTextModelLike {
    NotebookTextModelLike { uri: uri.clone(), notebook_type: notebook_type.to_string() }
}

fn ids(kernels: &[Rc<dyn INotebookKernel>]) -> Vec<String> {
    kernels.iter().map(|kernel| kernel.id()).collect()
}

/// O `setup` da suíte.
struct Suite {
    disposables: Rc<DisposableStore>,
    kernel_service: Rc<NotebookKernelService>,
    _on_did_add_notebook_document: Rc<Emitter<Rc<NotebookTextModel>>>,
    _store: LeakGuard,
}

impl Suite {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let disposables = DisposableStore::new();
        let on_did_add_notebook_document = Emitter::<Rc<NotebookTextModel>>::new();
        disposables.add(on_did_add_notebook_document.clone());

        let notebook_service: Rc<dyn INotebookService> = Rc::new(NotebookServiceMock {
            on_did_add_notebook_document: on_did_add_notebook_document.event(),
            on_will_remove_notebook_document: Event::none(),
            get_notebook_text_model: None,
        });
        let kernel_service = create_kernel_service(&disposables, notebook_service);
        Self { disposables, kernel_service, _on_did_add_notebook_document: on_did_add_notebook_document, _store: store }
    }

    /// O `IStorageService` do caso, com o JSON de `notebook.kernelHistory`, e o `NotebookKernelHistoryService` sobre ele.
    fn create_history_service(&self, stored_history: &str) -> Rc<NotebookKernelHistoryService> {
        let storage = self.disposables.add_rc(InMemoryStorageService::new());
        storage.store("notebook.kernelHistory", StorageValue::String(stored_history.to_string()), StorageScope::Workspace, StorageTarget::User);
        let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
        let kernel_service: Rc<dyn INotebookKernelService> = self.kernel_service.clone();
        self.disposables.add_rc(NotebookKernelHistoryService::new(storage_service, kernel_service, Rc::new(NullNotebookLoggingService)))
    }
}

impl Drop for Suite {
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

#[test]
fn notebook_kernel_empty_history() {
    let suite = Suite::new();
    let u1 = URI::parse("foo:///one", false);

    let k1 = kernel("z", "foo");
    let k2 = kernel("a", "foo");

    suite.disposables.add(suite.kernel_service.register_kernel(k1.clone()));
    suite.disposables.add(suite.kernel_service.register_kernel(k2.clone()));

    let kernel_history_service = suite.create_history_service(r#"{"foo":{"entries":[]}}"#);

    let mut info = kernel_history_service.get_kernels(&like(&u1, "foo"));
    assert_eq!(info.all.len(), 0);
    assert!(info.selected.is_none());

    // update priorities for u1 notebook
    let k2_dyn: Rc<dyn INotebookKernel> = k2.clone();
    suite.kernel_service.update_kernel_notebook_affinity(&k2_dyn, &u1, Some(2));

    info = kernel_history_service.get_kernels(&like(&u1, "foo"));
    assert_eq!(info.all.len(), 0);
    // MRU only auto selects kernel if there is only one
    assert!(info.selected.is_none());
}

#[test]
fn notebook_kernel_history_restore() {
    let suite = Suite::new();
    let u1 = URI::parse("foo:///one", false);

    let k1 = kernel("z", "foo");
    let k2 = kernel("a", "foo");
    let k3 = kernel("b", "foo");

    suite.disposables.add(suite.kernel_service.register_kernel(k1.clone()));
    suite.disposables.add(suite.kernel_service.register_kernel(k2.clone()));
    suite.disposables.add(suite.kernel_service.register_kernel(k3.clone()));

    let stored = format!(r#"{{"foo":{{"entries":["{}"]}}}}"#, k2.id());
    let kernel_history_service = suite.create_history_service(&stored);
    let mut info = kernel_history_service.get_kernels(&like(&u1, "foo"));
    assert_eq!(info.all.len(), 1);
    assert!(info.selected.is_none());

    let k3_dyn: Rc<dyn INotebookKernel> = k3.clone();
    kernel_history_service.add_most_recent_kernel(&k3_dyn);
    info = kernel_history_service.get_kernels(&like(&u1, "foo"));
    assert_eq!(ids(&info.all), vec![k3.id(), k2.id()]);
}
