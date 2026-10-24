//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookKernelService.test.ts` (suíte `NotebookKernelService`), caso a
//! caso e na mesma ordem. Os 4 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`_store`); o `setup` (o
//!   `DisposableStore`, o `onDidAddNotebookDocument` e os stubs de `INotebookService` e `IMenuService`) é [`Suite::new`], e o
//!   `teardown` (`disposables.dispose()`) é o `Drop` de [`Suite`].
//! - O `INotebookService` mockado, o `IMenuService` mockado e o `TestNotebookKernel` são os de
//!   `support/notebook_services_support.rs`. O `id` aleatório do `TestNotebookKernel` (`Math.random() + 'kernel'`) é um
//!   contador. `info.all[0] === k2` é a identidade do objeto ([`same`]).
//! - `kernelService.getMatchingKernel({ uri, notebookType })` e `selectKernelForNotebook(kernel, { uri, viewType, notebookType })`
//!   recebem o [`NotebookTextModelLike`] (o `viewType` do literal não é lido).
//! - `await Event.toPromise(kernelService.onDidChangeSelectedNotebooks)` é um ouvinte de um disparo que guarda o evento: o
//!   `onDidAddNotebookDocument.fire` chama o ouvinte na hora, e o `await` do original só espera essa entrega.
//! - `instantiationService.createInstance(NotebookTextModel, viewType, uri, [], {}, transientOptions)` é
//!   `NotebookTextModel::new(...)` com os serviços de `setup_notebook_services`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::uri::URI;
use je_workbench::contrib::notebook::browser::services::notebook_kernel_service_impl::NotebookKernelService;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::TransientOptions;
use je_workbench::contrib::notebook::common::notebook_kernel_service::{
    INotebookKernel, INotebookKernelMatchResult, INotebookKernelService, ISelectedNotebooksChangeEvent, NotebookTextModelLike,
};
use je_workbench::contrib::notebook::common::notebook_service::INotebookService;
use support::notebook_services_support::{NotebookServiceMock, TestNotebookKernel, TestNotebookKernelOptions, create_kernel_service};
use support::test_notebook_editor::{empty_metadata, setup_notebook_services};

thread_local! {
    static KERNEL_COUNTER: Cell<u32> = const { Cell::new(0) };
}

/// `Math.random() + 'kernel'`.
fn next_kernel_id() -> String {
    KERNEL_COUNTER.with(|counter| {
        counter.set(counter.get() + 1);
        format!("{}kernel", counter.get())
    })
}

/// `new TestNotebookKernel(opts)` com o `id` aleatório do original.
fn kernel(label: Option<&'static str>, view_type: Option<&'static str>) -> Rc<dyn INotebookKernel> {
    TestNotebookKernel::new(TestNotebookKernelOptions { id: Some(next_kernel_id()), label, view_type, ..Default::default() })
}

fn same(a: &Rc<dyn INotebookKernel>, b: &Rc<dyn INotebookKernel>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

fn like(uri: &URI, notebook_type: &str) -> NotebookTextModelLike {
    NotebookTextModelLike { uri: uri.clone(), notebook_type: notebook_type.to_string() }
}

fn matching(kernel_service: &Rc<NotebookKernelService>, uri: &URI, notebook_type: &str) -> INotebookKernelMatchResult {
    kernel_service.get_matching_kernel(&like(uri, notebook_type))
}

/// O `setup` da suíte.
struct Suite {
    disposables: Rc<DisposableStore>,
    on_did_add_notebook_document: Rc<Emitter<Rc<NotebookTextModel>>>,
    kernel_service: Rc<NotebookKernelService>,
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
        Self { disposables, on_did_add_notebook_document, kernel_service, _store: store }
    }
}

impl Drop for Suite {
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

#[test]
fn notebook_priorities() {
    let suite = Suite::new();
    let kernel_service = &suite.kernel_service;

    let u1 = URI::parse("foo:///one", false);
    let u2 = URI::parse("foo:///two", false);

    let k1 = kernel(Some("z"), None);
    let k2 = kernel(Some("a"), None);

    suite.disposables.add(kernel_service.register_kernel(k1.clone()));
    suite.disposables.add(kernel_service.register_kernel(k2.clone()));

    // equal priorities -> sort by name
    let mut info = matching(kernel_service, &u1, "foo");
    assert!(same(&info.all[0], &k2));
    assert!(same(&info.all[1], &k1));

    // update priorities for u1 notebook
    kernel_service.update_kernel_notebook_affinity(&k2, &u1, Some(2));
    kernel_service.update_kernel_notebook_affinity(&k2, &u2, Some(1));

    // updated
    info = matching(kernel_service, &u1, "foo");
    assert!(same(&info.all[0], &k2));
    assert!(same(&info.all[1], &k1));

    // NOT updated
    info = matching(kernel_service, &u2, "foo");
    assert!(same(&info.all[0], &k2));
    assert!(same(&info.all[1], &k1));

    // reset
    kernel_service.update_kernel_notebook_affinity(&k2, &u1, None);
    info = matching(kernel_service, &u1, "foo");
    assert!(same(&info.all[0], &k2));
    assert!(same(&info.all[1], &k1));
}

#[test]
fn new_kernel_with_higher_affinity_wins_issue_122028() {
    let suite = Suite::new();
    let kernel_service = &suite.kernel_service;
    let notebook = URI::parse("foo:///one", false);

    let kernel_a = kernel(None, None);
    suite.disposables.add(kernel_service.register_kernel(kernel_a.clone()));

    let mut info = matching(kernel_service, &notebook, "foo");
    assert_eq!(info.all.len(), 1);
    assert!(same(&info.all[0], &kernel_a));

    let better_kernel = kernel(None, None);
    suite.disposables.add(kernel_service.register_kernel(better_kernel.clone()));

    info = matching(kernel_service, &notebook, "foo");
    assert_eq!(info.all.len(), 2);

    kernel_service.update_kernel_notebook_affinity(&better_kernel, &notebook, Some(2));
    info = matching(kernel_service, &notebook, "foo");
    assert_eq!(info.all.len(), 2);
    assert!(same(&info.all[0], &better_kernel));
    assert!(same(&info.all[1], &kernel_a));
}

#[test]
fn on_did_change_selected_notebooks_not_fired_on_initial_notebook_open_121904() {
    let suite = Suite::new();
    let kernel_service = &suite.kernel_service;

    let uri = URI::parse("foo:///one", false);
    let jupyter = like(&uri, "jupyter");
    let dotnet = like(&uri, "dotnet");

    let jupyter_kernel = kernel(None, Some("jupyter"));
    let dotnet_kernel = kernel(None, Some("dotnet"));
    suite.disposables.add(kernel_service.register_kernel(jupyter_kernel.clone()));
    suite.disposables.add(kernel_service.register_kernel(dotnet_kernel.clone()));

    kernel_service.select_kernel_for_notebook(Some(&jupyter_kernel), &jupyter);
    kernel_service.select_kernel_for_notebook(Some(&dotnet_kernel), &dotnet);

    let mut info = kernel_service.get_matching_kernel(&dotnet);
    assert!(info.selected.as_ref().is_some_and(|selected| same(selected, &dotnet_kernel)));

    info = kernel_service.get_matching_kernel(&jupyter);
    assert!(info.selected.as_ref().is_some_and(|selected| same(selected, &jupyter_kernel)));
}

#[test]
fn on_did_change_selected_notebooks_not_fired_on_initial_notebook_open_121904_p2() {
    let suite = Suite::new();
    let kernel_service = &suite.kernel_service;

    let uri = URI::parse("foo:///one", false);
    let jupyter = like(&uri, "jupyter");
    let dotnet = like(&uri, "dotnet");

    let jupyter_kernel = kernel(None, Some("jupyter"));
    let dotnet_kernel = kernel(None, Some("dotnet"));
    suite.disposables.add(kernel_service.register_kernel(jupyter_kernel.clone()));
    suite.disposables.add(kernel_service.register_kernel(dotnet_kernel.clone()));

    kernel_service.select_kernel_for_notebook(Some(&jupyter_kernel), &jupyter);
    kernel_service.select_kernel_for_notebook(Some(&dotnet_kernel), &dotnet);

    let transient_options = TransientOptions::default();
    let services = setup_notebook_services(&suite.disposables);
    let create_notebook = |view_type: &str, uri: &URI| {
        suite.disposables.add_rc(NotebookTextModel::new(
            view_type,
            uri.clone(),
            Vec::new(),
            empty_metadata(),
            transient_options.clone(),
            services.undo_redo_service.clone(),
            services.model_service.clone(),
            services.language_service(),
            services.language_detection_service(),
            services.notebook_execution_state_service(),
            services.notebook_logging_service(),
        ))
    };
    let first_event = |kernel_service: &Rc<NotebookKernelService>| {
        let received: Rc<RefCell<Option<ISelectedNotebooksChangeEvent>>> = Rc::new(RefCell::new(None));
        let sink = received.clone();
        let listener = kernel_service.on_did_change_selected_notebooks().once().listen(move |event| *sink.borrow_mut() = Some(event.clone()));
        (received, listener)
    };

    {
        // open as jupyter -> bind event
        let (p1, listener) = first_event(kernel_service);
        let d1 = create_notebook(&jupyter.notebook_type, &jupyter.uri);
        suite.on_did_add_notebook_document.fire(d1);
        let event = p1.borrow().clone().expect("o evento de ligação dispara");
        assert_eq!(event.new_kernel, Some(jupyter_kernel.id()));
        listener.dispose();
    }
    {
        // RE-open as dotnet -> bind event
        let (p2, listener) = first_event(kernel_service);
        let d2 = create_notebook(&dotnet.notebook_type, &dotnet.uri);
        suite.on_did_add_notebook_document.fire(d2);
        let event2 = p2.borrow().clone().expect("o evento de ligação dispara");
        assert_eq!(event2.new_kernel, Some(dotnet_kernel.id()));
        listener.dispose();
    }
}
