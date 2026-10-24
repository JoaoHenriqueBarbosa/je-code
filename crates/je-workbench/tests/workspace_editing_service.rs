//! Porte de `src/vs/workbench/services/workspaces/test/browser/workspaceEditingService.test.ts` (suíte
//! `WorkspaceEditingService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada caso.
//! - **`UNKNOWN_EMPTY_WINDOW_WORKSPACE`** é o identificador de janela vazia com o id `empty-window`.
//! - **`strictEqual` entre objetos** (identidade no original) vira igualdade estrutural dos identificadores.
//! - **`let executed = false; (async () => { executed = true; })()`** é um `Promise` criado por `async_fn`, que roda o
//!   corpo na hora, e a flag é um `Cell`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::async_fn;
use je_base::promise::Promise;
use je_platform::resources::uri_file;
use je_platform::workspace::{
    AnyWorkspaceIdentifier, EmptyWorkspaceIdentifier, SingleFolderWorkspaceIdentifier, UNKNOWN_EMPTY_WINDOW_WORKSPACE_ID,
    WorkspaceIdentifier,
};
use je_workbench::services::workspaces::browser::abstract_workspace_editing_service::DidEnterWorkspaceEvent;

/// `UNKNOWN_EMPTY_WINDOW_WORKSPACE`.
fn unknown_empty_window_workspace() -> AnyWorkspaceIdentifier {
    AnyWorkspaceIdentifier::Empty(EmptyWorkspaceIdentifier { id: UNKNOWN_EMPTY_WINDOW_WORKSPACE_ID.to_string() })
}

/// `{ id: 'new-workspace', configPath: URI.file('/test/workspace.code-workspace') }`.
fn new_workspace() -> AnyWorkspaceIdentifier {
    AnyWorkspaceIdentifier::Workspace(WorkspaceIdentifier {
        id: "new-workspace".to_string(),
        config_path: uri_file("/test/workspace.code-workspace"),
    })
}

// suite('DidEnterWorkspaceEvent')

#[test]
fn event_captures_old_workspace_and_new_workspace_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let old_workspace = AnyWorkspaceIdentifier::SingleFolder(SingleFolderWorkspaceIdentifier {
        id: "old-folder".to_string(),
        uri: uri_file("/old/folder"),
    });
    let new_workspace = new_workspace();
    let event = DidEnterWorkspaceEvent::new(old_workspace.clone(), new_workspace.clone());

    assert_eq!(event.old_workspace, old_workspace);
    assert_eq!(event.new_workspace, new_workspace);
}

#[test]
fn join_collects_promises() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let event = DidEnterWorkspaceEvent::new(unknown_empty_window_workspace(), new_workspace());

        let executed1 = Rc::new(Cell::new(false));
        let executed2 = Rc::new(Cell::new(false));

        let flag = executed1.clone();
        event.join(async_fn(async move {
            flag.set(true);
            Ok(())
        }));

        let flag = executed2.clone();
        event.join(async_fn(async move {
            flag.set(true);
            Ok(())
        }));

        event.wait().await.unwrap();

        assert!(executed1.get(), "First promise should have executed");
        assert!(executed2.get(), "Second promise should have executed");
    });
}

#[test]
fn wait_resolves_when_all_promises_complete() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let event = Rc::new(DidEnterWorkspaceEvent::new(unknown_empty_window_workspace(), new_workspace()));

        let (promise1, resolve1) = Promise::<()>::new();
        let (promise2, resolve2) = Promise::<()>::new();

        event.join(promise1);
        event.join(promise2);

        let wait_completed = Rc::new(Cell::new(false));
        let flag = wait_completed.clone();
        let waiting = event.clone();
        let wait_promise = async_fn(async move { waiting.wait().await }).then(move |_| flag.set(true));

        // Should not be completed yet
        Promise::resolved(()).await.unwrap();
        assert!(!wait_completed.get());

        // Resolve first promise
        resolve1.resolve(());
        Promise::resolved(()).await.unwrap();
        assert!(!wait_completed.get());

        // Resolve second promise
        resolve2.resolve(());
        wait_promise.await.unwrap();
        assert!(wait_completed.get());
    });
}

#[test]
fn wait_resolves_immediately_when_no_promises_are_joined() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let event = DidEnterWorkspaceEvent::new(unknown_empty_window_workspace(), new_workspace());

        event.wait().await.unwrap();
        // Should complete without error
    });
}
