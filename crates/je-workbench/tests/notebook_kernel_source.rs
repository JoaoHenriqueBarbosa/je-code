//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookKernelSource.test.ts` (suíte `Notebook kernel source menu lifetime`), caso a caso e na mesma ordem. Os 6 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; o `setup` (o `setupInstantiationService`,
//!   o stub de `INotebookService`, o `ICommandService` mockado, o `MenuService` real e o `NotebookKernelService`) é [`Suite::new`], e o
//!   `disposables` é o `DisposableStore` da suíte, descartado no `Drop`.
//! - `new class extends mock<ICommandService>() { override executeCommand() { return commandResult; } }` é o `MockCommandService` com o
//!   `execute_command` devolvendo a promessa corrente de `command_result` (a variável `commandResult` do original, trocada no caso do
//!   comando em voo).
//! - `new MenuService(commandService, new MockKeybindingService(), new InMemoryStorageService())`, o `MenuRegistry` e o `ContextKeyService`
//!   são os reais de `je-platform`; o `MockKeybindingService` é o de `je-platform/tests/common/keybinding.rs`.
//! - Os casos `async` rodam com `run_with_real_timers`: o `MenuService` reconstrói o menu por um `RunOnceScheduler`, e o `await refreshed` do
//!   quarto caso espera esse timer de verdade. `await` de `Promise` é o `.await` dela.
//! - `Event.toPromise(service.onDidChangeSourceActions)` é o `to_promise` do evento, criado antes do `appendMenuItem` como no original.
//! - `source.action.enabled` é `source.action().enabled()` e `action === other` é a identidade do `Rc`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-platform/tests/common/keybinding.rs"]
mod keybinding;

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::actions::IAction;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::URI;
use je_platform::action::CommandAction;
use je_platform::actions::actions::IMenuService;
use je_platform::actions::{AnyMenuItem, MenuItem, MenuService, menu_ids, menu_registry};
use je_platform::commands::commands::ICommandService;
use je_platform::contextkey::ContextKeyExpr;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_workbench::contrib::notebook::browser::services::notebook_kernel_service_impl::NotebookKernelService;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::TransientOptions;
use je_workbench::contrib::notebook::common::notebook_kernel_service::{INotebookKernelService, ISourceAction};
use je_workbench::contrib::notebook::common::notebook_service::INotebookService;
use keybinding::MockKeybindingService;
use support::mock_command_service::MockCommandService;
use support::notebook_services_support::NotebookServiceMock;
use support::test_notebook_editor::{NotebookTestServices, empty_metadata, setup_notebook_services};

/// O `setup` da suíte.
struct Suite {
    disposables: Rc<DisposableStore>,
    services: NotebookTestServices,
    on_will_remove_notebook: Rc<Emitter<Rc<NotebookTextModel>>>,
    context_key_service: Rc<dyn IContextKeyService>,
    command_result: Rc<RefCell<Promise<()>>>,
    service: Rc<NotebookKernelService>,
    _store: LeakGuard,
}

impl Suite {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let disposables = DisposableStore::new();
        let services = setup_notebook_services(&disposables);
        let on_will_remove_notebook = Emitter::<Rc<NotebookTextModel>>::new();
        disposables.add(on_will_remove_notebook.clone());

        let notebook_service: Rc<dyn INotebookService> = Rc::new(NotebookServiceMock {
            on_did_add_notebook_document: Event::none(),
            on_will_remove_notebook_document: on_will_remove_notebook.event(),
            get_notebook_text_model: None,
        });

        let command_result = Rc::new(RefCell::new(Promise::resolved(())));
        let current_result = command_result.clone();
        let command_service: Rc<dyn ICommandService> =
            Rc::new(MockCommandService { execute_command: Some(Box::new(move |_| current_result.borrow().then(|()| Dynamic::Undefined))) });

        let context_key_service_concrete = disposables.add_rc(ContextKeyService::new(services.configuration_service.clone()));
        let context_key_service: Rc<dyn IContextKeyService> = context_key_service_concrete;

        let in_memory_storage = disposables.add_rc(InMemoryStorageService::new());
        let menu_storage: Rc<dyn StorageService> = Rc::new(in_memory_storage.clone());
        let menu_service = disposables.add_rc(MenuService::new(command_service, Rc::new(MockKeybindingService), menu_storage));
        let menu_service: Rc<dyn IMenuService> = menu_service;

        let kernel_storage: Rc<dyn StorageService> = Rc::new(disposables.add_rc(InMemoryStorageService::new()));
        let service = disposables.add_rc(NotebookKernelService::new(notebook_service, kernel_storage, menu_service, context_key_service.clone()));
        disposables.add(menu_registry().append_menu_item(
            &menu_ids::notebook_kernel_source(),
            AnyMenuItem::Menu(MenuItem::new(CommandAction {
                precondition: Some(ContextKeyExpr::equals("testNotebookKernelEnabled", Dynamic::Bool(true))),
                ..CommandAction::new("test.notebookSource", "Test Kernel")
            })),
        ));

        Self { disposables, services, on_will_remove_notebook, context_key_service, command_result, service, _store: store }
    }

    /// `createNotebook(path)`.
    fn create_notebook(&self, path: &str) -> Rc<NotebookTextModel> {
        NotebookTextModel::new(
            "test-notebook",
            URI::parse(format!("test:///{path}").as_str(), false),
            Vec::new(),
            empty_metadata(),
            TransientOptions::default(),
            self.services.undo_redo_service.clone(),
            self.services.model_service.clone(),
            self.services.language_service(),
            self.services.language_detection_service(),
            self.services.notebook_execution_state_service(),
            self.services.notebook_logging_service(),
        )
    }

    /// `closeNotebook(notebook)`.
    fn close_notebook(&self, notebook: &Rc<NotebookTextModel>) {
        self.on_will_remove_notebook.fire(notebook.clone());
        notebook.dispose();
    }

    /// `service.getSourceActions(notebook, contextKeyService)`.
    fn source_actions(&self, notebook: &Rc<NotebookTextModel>) -> Vec<Rc<dyn ISourceAction>> {
        self.service.get_source_actions(&**notebook, Some(self.context_key_service.clone()))
    }
}

impl Drop for Suite {
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

fn same(a: &Rc<dyn ISourceAction>, b: &Rc<dyn ISourceAction>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

fn action_ids(actions: &[Rc<dyn ISourceAction>]) -> Vec<String> {
    actions.iter().map(|source| source.action().id()).collect()
}

#[test]
fn reopening_a_notebook_creates_source_actions_with_its_current_context() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        let enabled = suite.context_key_service.create_key("testNotebookKernelEnabled", Dynamic::Bool(false));
        let first = suite.create_notebook("closed");
        let initial = suite.source_actions(&first).into_iter().find(|source| source.action().id() == "test.notebookSource").unwrap().action().enabled();
        suite.close_notebook(&first);
        enabled.set(Dynamic::Bool(true));
        let reopened = suite.create_notebook("closed");
        let current = suite.source_actions(&reopened).into_iter().find(|source| source.action().id() == "test.notebookSource").unwrap().action().enabled();
        suite.close_notebook(&reopened);
        assert_eq!([initial, current], [false, true]);
    });
}

#[test]
fn callers_can_change_the_returned_action_list_without_changing_the_cache() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        let notebook = suite.create_notebook("closed");
        let mut actions = suite.source_actions(&notebook);
        actions.clear();
        let ids = action_ids(&suite.source_actions(&notebook));
        suite.close_notebook(&notebook);
        assert_eq!(ids, vec!["test.notebookSource".to_string()]);
    });
}

#[test]
fn closing_one_notebook_preserves_another_notebook_source_action() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        suite.context_key_service.create_key("testNotebookKernelEnabled", Dynamic::Bool(true));
        let closing = suite.create_notebook("closing");
        let remaining = suite.create_notebook("remaining");
        suite.source_actions(&closing);
        let action = suite.source_actions(&remaining).remove(0);
        suite.close_notebook(&closing);
        let updates = Rc::new(Cell::new(0));
        let counter = updates.clone();
        suite.disposables.add(action.on_did_change_state().listen(move |_| counter.set(counter.get() + 1)));
        action.run_action().await.unwrap();
        let is_same = same(&suite.source_actions(&remaining)[0], &action);
        suite.close_notebook(&remaining);
        assert_eq!((is_same, updates.get()), (true, 2));
    });
}

#[test]
fn menu_refresh_disposes_obsolete_source_action_events() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        suite.context_key_service.create_key("testNotebookKernelEnabled", Dynamic::Bool(true));
        let notebook = suite.create_notebook("closed");
        let old_action = suite.source_actions(&notebook).remove(0);
        let updates = Rc::new(Cell::new(0));
        let counter = updates.clone();
        suite.disposables.add(old_action.on_did_change_state().listen(move |_| counter.set(counter.get() + 1)));
        let refreshed = suite.service.on_did_change_source_actions().to_promise(None);
        suite.disposables.add(menu_registry().append_menu_item(
            &menu_ids::notebook_kernel_source(),
            AnyMenuItem::Menu(MenuItem::new(CommandAction::new("test.anotherNotebookSource", "Another Kernel"))),
        ));
        refreshed.await.unwrap();
        old_action.run_action().await.unwrap();
        let has_new_action = suite.source_actions(&notebook).iter().any(|source| source.action().id() == "test.anotherNotebookSource");
        suite.close_notebook(&notebook);
        assert_eq!((updates.get(), has_new_action), (0, true));
    });
}

#[test]
fn an_in_flight_source_command_can_complete_after_its_notebook_closes() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        suite.context_key_service.create_key("testNotebookKernelEnabled", Dynamic::Bool(true));
        let pending = DeferredPromise::<()>::new();
        *suite.command_result.borrow_mut() = pending.promise.clone();
        let notebook = suite.create_notebook("closed");
        let action = suite.source_actions(&notebook).remove(0);
        let execution = action.run_action();
        let running_before_close = suite.service.get_running_source_actions(&*notebook).len();
        suite.close_notebook(&notebook);
        let running_after_close = suite.service.get_running_source_actions(&*notebook).len();
        pending.complete(()).await.unwrap();
        execution.await.unwrap();
        assert_eq!((running_before_close, running_after_close, action.execution().is_none()), (1, 0, true));
    });
}

#[test]
fn service_shutdown_disposes_live_source_action_events() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        suite.context_key_service.create_key("testNotebookKernelEnabled", Dynamic::Bool(true));
        let notebook = suite.create_notebook("closed");
        let action = suite.source_actions(&notebook).remove(0);
        let updates = Rc::new(Cell::new(0));
        let counter = updates.clone();
        suite.disposables.add(action.on_did_change_state().listen(move |_| counter.set(counter.get() + 1)));
        suite.service.dispose();
        action.run_action().await.unwrap();
        suite.close_notebook(&notebook);
        assert_eq!(updates.get(), 0);
    });
}
