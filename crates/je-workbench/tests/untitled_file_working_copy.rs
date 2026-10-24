//! Porte de `src/vs/workbench/services/workingCopy/test/browser/untitledFileWorkingCopy.test.ts` (suíte `UntitledFileWorkingCopy`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada
//!   caso (`LeakGuard`, o `disposables` do original é o store dele, `store`) e o `setup` ([`setup_working_copy`]) logo depois.
//!   Os casos `async` rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`TestUntitledFileWorkingCopyModel`, a fábrica, o `createWorkingCopy` e os utilitários.** Estão em
//!   `tests/support/untitled_file_working_copy_support.rs`; o `TestServiceAccessor` é o de
//!   `tests/support/stored_file_working_copy_support.rs`.
//! - **`workingCopy = createWorkingCopy(...)`** reatribui a variável `working_copy` do caso (um `let` novo, que sombreia o
//!   anterior).
//! - **`workingCopy.model?.x`.** `workingCopy.model` é [`UntitledFileWorkingCopy::model`] (o modelo tipado).
//! - **`isReadableStream`, `consumeStream` e `consumeReadable`.** O conteúdo do backup é lido por [`backup_contents`].
//! - **Contadores.** `counter++` nos ouvintes é [`count_events`], que devolve o contador compartilhado.
//! - **`workingCopyBackupService.backup(workingCopy, ...)` e `hasBackupSync(workingCopy)`.** Uma working copy, que no original
//!   é um `IWorkingCopyIdentifier` por tipagem estrutural, vira o identificador com [`WorkingCopyIdentifier::of`].
//! - **`assert.fail('Missing untitled backup')`.** É o `assert_eq!` contra o texto esperado: um backup sem conteúdo reprova.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_workbench::services::working_copy::file_working_copy::IFileWorkingCopy;
use je_workbench::services::working_copy::untitled_file_working_copy::IUntitledFileWorkingCopy;
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_backup::IWorkingCopyBackupService;
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use support::stored_file_working_copy_support::TestServiceAccessor;
use support::untitled_file_working_copy_support::{
    TestUntitledFileWorkingCopy, backup_contents, count_events, create_working_copy, initial, setup_working_copy, untitled_resource,
};

/// `createWorkingCopy(uri, hasAssociatedFilePath, initialValue)` com o recurso padrão da suíte.
fn create(store: &Rc<DisposableStore>, accessor: &TestServiceAccessor, has_associated_file_path: bool, initial_value: &str) -> Rc<TestUntitledFileWorkingCopy> {
    create_working_copy(store, accessor, &untitled_resource(), has_associated_file_path, initial_value, false)
}

#[test]
fn registers_with_working_copy_service() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (accessor, working_copy) = setup_working_copy(&store, false);

        assert_eq!(accessor.working_copy_service.working_copies().len(), 1);

        working_copy.dispose();

        assert_eq!(accessor.working_copy_service.working_copies().len(), 0);
    });
}

#[test]
fn dirty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (_accessor, working_copy) = setup_working_copy(&store, false);

        assert!(!working_copy.is_dirty());

        let change_dirty_counter = count_events(&store, working_copy.on_did_change_dirty());
        let content_change_counter = count_events(&store, working_copy.on_did_change_content());

        working_copy.resolve().await.unwrap();
        assert!(working_copy.is_resolved());

        // Dirty from: Model content change
        working_copy.model().unwrap().update_contents("hello dirty");
        assert_eq!(content_change_counter.get(), 1);

        assert!(working_copy.is_dirty());
        assert_eq!(change_dirty_counter.get(), 1);

        working_copy.save(None).await.unwrap();

        assert!(!working_copy.is_dirty());
        assert_eq!(change_dirty_counter.get(), 2);
    });
}

#[test]
fn dirty_cleared_when_content_event_signals_is_empty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (_accessor, working_copy) = setup_working_copy(&store, false);

        assert!(!working_copy.is_dirty());

        working_copy.resolve().await.unwrap();

        working_copy.model().unwrap().update_contents("hello dirty");
        assert!(working_copy.is_dirty());

        working_copy.model().unwrap().fire_content_change_event(initial(true));

        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn dirty_not_cleared_when_content_event_signals_is_empty_when_associated_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (accessor, working_copy) = setup_working_copy(&store, false);

        working_copy.dispose();
        let working_copy = create(&store, &accessor, true, "");

        working_copy.resolve().await.unwrap();

        working_copy.model().unwrap().update_contents("hello dirty");
        assert!(working_copy.is_dirty());

        working_copy.model().unwrap().fire_content_change_event(initial(true));

        assert!(working_copy.is_dirty());
    });
}

#[test]
fn revert() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (_accessor, working_copy) = setup_working_copy(&store, false);

        let revert_counter = count_events(&store, working_copy.on_did_revert());
        let dispose_counter = count_events(&store, working_copy.on_will_dispose());

        working_copy.resolve().await.unwrap();

        working_copy.model().unwrap().update_contents("hello dirty");
        assert!(working_copy.is_dirty());

        working_copy.revert(None).await.unwrap();

        assert_eq!(revert_counter.get(), 1);
        assert_eq!(dispose_counter.get(), 1);
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn dispose() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (_accessor, working_copy) = setup_working_copy(&store, false);

        let dispose_counter = count_events(&store, working_copy.on_will_dispose());

        working_copy.resolve().await.unwrap();
        working_copy.dispose();

        assert_eq!(dispose_counter.get(), 1);
    });
}

#[test]
fn backup() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (_accessor, working_copy) = setup_working_copy(&store, false);

        assert!(working_copy.backup(&CancellationToken::None).await.unwrap().content.is_none());

        working_copy.resolve().await.unwrap();

        working_copy.model().unwrap().update_contents("Hello Backup");
        let backup = working_copy.backup(&CancellationToken::None).await.unwrap();

        let contents = backup_contents(backup.content).await.unwrap();

        assert_eq!(contents.as_deref(), Some("Hello Backup"));
    });
}

#[test]
fn resolve_without_contents() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (_accessor, working_copy) = setup_working_copy(&store, false);

        assert!(!working_copy.is_resolved());
        assert!(!working_copy.has_associated_file_path());
        assert!(working_copy.model().is_none());

        working_copy.resolve().await.unwrap();

        assert!(working_copy.is_resolved());
        assert!(working_copy.model().is_some());
    });
}

#[test]
fn resolve_with_initial_contents() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (accessor, working_copy) = setup_working_copy(&store, false);

        working_copy.dispose();

        let working_copy = create(&store, &accessor, false, "Hello Initial");

        let content_change_counter = count_events(&store, working_copy.on_did_change_content());

        assert!(working_copy.is_dirty());

        working_copy.resolve().await.unwrap();

        assert!(working_copy.is_dirty());
        assert_eq!(working_copy.model().unwrap().contents(), "Hello Initial");
        assert_eq!(content_change_counter.get(), 1);

        working_copy.model().unwrap().update_contents("Changed contents");

        working_copy.resolve().await.unwrap(); // second resolve should be ignored
        assert_eq!(working_copy.model().unwrap().contents(), "Changed contents");
    });
}

#[test]
fn backup_with_initial_contents_uses_those_even_if_unresolved() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (accessor, working_copy) = setup_working_copy(&store, false);

        working_copy.dispose();

        let working_copy = create(&store, &accessor, false, "Hello Initial");

        assert!(working_copy.is_dirty());

        let backup = working_copy.backup(&CancellationToken::None).await.unwrap().content;
        let value = backup_contents(backup).await.unwrap();

        assert_eq!(value.as_deref(), Some("Hello Initial"));
    });
}

#[test]
fn resolve_with_associated_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (accessor, working_copy) = setup_working_copy(&store, false);

        working_copy.dispose();
        let working_copy = create(&store, &accessor, true, "");

        working_copy.resolve().await.unwrap();

        assert!(working_copy.is_dirty());
        assert!(working_copy.has_associated_file_path());
    });
}

#[test]
fn resolve_with_backup() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let store = disposables.store();
        let (accessor, working_copy) = setup_working_copy(&store, false);

        working_copy.resolve().await.unwrap();
        working_copy.model().unwrap().update_contents("Hello Backup");

        let backup = working_copy.backup(&CancellationToken::None).await.unwrap();
        accessor.working_copy_backup_service.backup(&WorkingCopyIdentifier::of(&*working_copy), backup.content, None, backup.meta, None).await.unwrap();

        assert!(accessor.working_copy_backup_service.has_backup_sync(&WorkingCopyIdentifier::of(&*working_copy), None, None));

        working_copy.dispose();

        let working_copy = create(&store, &accessor, false, "");

        let content_change_counter = count_events(&store, working_copy.on_did_change_content());

        working_copy.resolve().await.unwrap();

        assert!(working_copy.is_dirty());
        assert_eq!(working_copy.model().unwrap().contents(), "Hello Backup");
        assert_eq!(content_change_counter.get(), 1);
    });
}
