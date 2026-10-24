//! Porte de `src/vs/workbench/services/workingCopy/test/browser/storedFileWorkingCopy.test.ts` (suítes `StoredFileWorkingCopy (with custom save)` e `StoredFileWorkingCopy`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`] e [`setup_custom_save`])
//!   logo depois. O `teardown` da suíte principal (`workingCopy.dispose()` e o laço sobre
//!   `accessor.workingCopyService.workingCopies`) é o `Drop` do [`Fixture`], que roda antes do guarda conferir os
//!   vazamentos (o guarda é declarado primeiro). Os casos `async` rodam em `run_with_faked_timers`; `await` é
//!   `.await.unwrap()` (uma rejeição reprova o caso). O caso sem `await` roda direto.
//! - **`workbenchInstantiationService`, `TestServiceAccessor` e os dublês.** Estão em `tests/support/stored_file_working_copy_support.rs`
//!   (o modelo e as fábricas do teste, o acessor, o `IWorkingCopyFileService` com participantes de salvar e os demais
//!   serviços que faltavam).
//! - **`createWorkingCopy(uri = resource)` e `workingCopy = createWorkingCopy()`.** [`Fixture::replace_working_copy`] cria
//!   a working copy pelo acessor (com o resolvedor externo `options => workingCopy.resolve(options)`) e a guarda como a
//!   atual. A primeira, do `setup`, é registrada no `disposables`; as demais não, como no original.
//! - **`workingCopy.save(options)`** é `save_stored` com o [`StoredFileWorkingCopySaveAsOptions`] montado por [`save_as`];
//!   `workingCopy.resolve(options)` é `resolve_stored`; `workingCopy.resolve()` é `resolve`. `workingCopy.model` é
//!   [`StoredFileWorkingCopy::model`] (o modelo tipado).
//! - **`isStoredFileWorkingCopySaveEvent(event)`** e `assert.ok(lastSaveEvent!.stat)` não existem: o tipo
//!   `StoredFileWorkingCopySaveEvent` já garante o `stat` (ver o porte da `StoredFileWorkingCopy`).
//! - **`isReadonly()`.** `assert.strictEqual(isReadonly(), false)` é `Readonly::Bool(false)` e `!!isReadonly()` é
//!   [`Readonly::is_readonly`].
//! - **Erros do serviço de arquivos.** `new FileOperationError(message, result)` é `FileOperationError::new(...).into_error()`
//!   ([`file_error`]) e `new NotModifiedSinceFileOperationError(message, stat)` é `FileOperationError::not_modified_since`;
//!   o `try/finally` que limpa `readShouldThrowError` e `writeShouldThrowError` é a limpeza logo depois do `await`, antes de
//!   o resultado ser verificado.
//! - **`backup.content instanceof VSBuffer`.** O conteúdo do backup é o enum `WorkingCopyBackupContent`, que só traz o leitor e
//!   o stream; o ramo do `VSBuffer` não existe.
//! - **`Event.toPromise`** é `Event::to_promise(None)`; `Promises.settled` é `promises::settled`; `timeout` é o de
//!   `je_base::async_`.
//! - **Participante que verifica o contexto.** O original captura o erro da asserção dentro do participante e o relança
//!   depois do `save`; aqui o participante guarda o contexto recebido e as asserções rodam depois do `save`.
//! - **`workingCopy === wc`** é a igualdade de endereço dos dois `Rc`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::{async_fn, promises, timeout};
use je_base::buffer::{VSBuffer, buffer_to_stream, readable_to_buffer, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::files::{FileChange, FileChangeType, FileChangesEvent, FileOperationError, FileOperationResult, FileStatWithMetadata, ResolveFileOptions};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::uri_file;
use je_workbench::common::editor::{RevertOptions, SaveOptions, SaveReason, with_save_source_registry};
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::textfile::collaborators::IWorkingCopyFileService;
use je_workbench::services::working_copy::file_working_copy::{IFileWorkingCopy, IFileWorkingCopyModel};
use je_workbench::services::working_copy::resource_working_copy::IResourceWorkingCopy;
use je_workbench::services::working_copy::stored_file_working_copy::{
    IStoredFileWorkingCopy, StoredFileWorkingCopyModelContentChangedEvent, StoredFileWorkingCopyResolveOptions, StoredFileWorkingCopySaveAsOptions,
    StoredFileWorkingCopySaveEvent, StoredFileWorkingCopyState,
};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackupContent, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_backup::IWorkingCopyBackupService;
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use je_workbench::test::common::workbench_test_services::TestFileService;
use support::stored_file_working_copy_support::{
    TestServiceAccessor, TestStoredFileWorkingCopy, TestStoredFileWorkingCopyModel, TestStoredFileWorkingCopyModelFactory, save_participant,
};

use StoredFileWorkingCopyState::{Conflict, Dirty, Error as ErrorState, Orphan, PendingSave, Saved};

//#region Utilitários dos casos

/// O que o `setup` da suíte principal monta: o acessor, a fábrica e a working copy atual.
struct Fixture {
    accessor: TestServiceAccessor,
    resource: URI,
    factory: Rc<TestStoredFileWorkingCopyModelFactory>,
    working_copy: RefCell<Rc<TestStoredFileWorkingCopy>>,
}

impl Drop for Fixture {
    /// O `teardown`: `workingCopy.dispose()` e o `dispose()` das working copies ainda registradas.
    fn drop(&mut self) {
        self.working_copy().dispose();

        self.accessor.dispose_registered_working_copies();
    }
}

impl Fixture {
    /// `workingCopy`.
    fn working_copy(&self) -> Rc<TestStoredFileWorkingCopy> {
        self.working_copy.borrow().clone()
    }

    /// `workingCopy = createWorkingCopy()`.
    fn replace_working_copy(&self) -> Rc<TestStoredFileWorkingCopy> {
        let working_copy = self.accessor.create_stored_file_working_copy(&self.resource, self.factory.clone());
        *self.working_copy.borrow_mut() = working_copy.clone();

        working_copy
    }
}

/// O `setup` da suíte `StoredFileWorkingCopy`.
fn setup(disposables: &LeakGuard) -> Fixture {
    let accessor = TestServiceAccessor::new(&disposables.store());
    let resource = uri_file("test/resource");
    let factory = TestStoredFileWorkingCopyModelFactory::new();
    let working_copy = disposables.add_rc(accessor.create_stored_file_working_copy(&resource, factory.clone()));

    Fixture { accessor, resource, factory, working_copy: RefCell::new(working_copy) }
}

/// O `setup` da suíte `StoredFileWorkingCopy (with custom save)`.
fn setup_custom_save(disposables: &LeakGuard) -> (TestServiceAccessor, Rc<TestStoredFileWorkingCopy>) {
    let accessor = TestServiceAccessor::new(&disposables.store());
    let working_copy = disposables.add_rc(accessor.create_stored_file_working_copy(&uri_file("test/resource"), TestStoredFileWorkingCopyModelFactory::with_custom_save()));

    (accessor, working_copy)
}

/// `workingCopy.model`.
fn model_of(working_copy: &TestStoredFileWorkingCopy) -> Rc<TestStoredFileWorkingCopyModel> {
    working_copy.model().expect("a working copy está resolvida")
}

/// `workingCopy.model?.contents`.
fn contents_of(working_copy: &TestStoredFileWorkingCopy) -> String {
    model_of(working_copy).contents()
}

/// `{ contents: bufferToStream(VSBuffer.fromString(text)) }`.
fn resolve_with_contents(text: &str) -> Option<StoredFileWorkingCopyResolveOptions> {
    Some(StoredFileWorkingCopyResolveOptions { contents: Some(buffer_to_stream(VSBuffer::from_string(text))), ..StoredFileWorkingCopyResolveOptions::default() })
}

/// `{ forceReadFromFile: true }`.
fn force_read_from_file() -> Option<StoredFileWorkingCopyResolveOptions> {
    Some(StoredFileWorkingCopyResolveOptions { force_read_from_file: Some(true), ..StoredFileWorkingCopyResolveOptions::default() })
}

/// As opções de salvar montadas por `configure` (o literal de objeto do original).
fn save_as(configure: impl FnOnce(&mut StoredFileWorkingCopySaveAsOptions)) -> Option<StoredFileWorkingCopySaveAsOptions> {
    let mut options = StoredFileWorkingCopySaveAsOptions::default();
    configure(&mut options);

    Some(options)
}

/// `{ force: true }`.
fn forced() -> Option<StoredFileWorkingCopySaveAsOptions> {
    save_as(|options| options.options.base.force = Some(true))
}

/// `{ reason }`.
fn with_reason(reason: SaveReason) -> Option<StoredFileWorkingCopySaveAsOptions> {
    save_as(|options| options.options.base.reason = Some(reason))
}

/// `{ soft: true }` das opções de reverter.
fn soft_revert() -> Option<RevertOptions> {
    Some(RevertOptions { soft: Some(true), ..RevertOptions::default() })
}

/// `{ force: true }` das opções de reverter.
fn forced_revert() -> Option<RevertOptions> {
    Some(RevertOptions { force: Some(true), ..RevertOptions::default() })
}

/// `new FileOperationError(message, result)`.
fn file_error(message: &str, result: FileOperationResult) -> Error {
    FileOperationError::new(message, result).into_error()
}

/// `{ ...stat, readonly }`.
fn stat_with_readonly(stat: &FileStatWithMetadata, readonly: bool) -> FileStatWithMetadata {
    let mut changed = stat.clone().into_stat();
    changed.readonly = Some(readonly);

    FileStatWithMetadata::new(changed)
}

/// Conta os disparos de `event` (o `counter++` dos ouvintes do original).
fn count_events<T: 'static>(disposables: &LeakGuard, event: Event<T>) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    let listener_counter = counter.clone();
    disposables.add(event.listen(move |_| listener_counter.set(listener_counter.get() + 1)));

    counter
}

/// O `onDidSave` com o `lastSaveEvent` e o `savedCounter` que os casos de salvar guardam.
fn track_saves(disposables: &LeakGuard, working_copy: &TestStoredFileWorkingCopy) -> (Rc<Cell<usize>>, Rc<RefCell<Option<StoredFileWorkingCopySaveEvent>>>) {
    let saved_counter = Rc::new(Cell::new(0));
    let last_save_event: Rc<RefCell<Option<StoredFileWorkingCopySaveEvent>>> = Rc::default();

    let (counter, last) = (saved_counter.clone(), last_save_event.clone());
    disposables.add(working_copy.on_did_save_stored().listen(move |event| {
        counter.set(counter.get() + 1);
        *last.borrow_mut() = Some(event.clone());
    }));

    (saved_counter, last_save_event)
}

/// `accessor.fileService.notExistsSet.set(resource, true)` e o `fireFileChanges` do `DELETED`.
fn mark_deleted(file_service: &TestFileService, resource: &URI) {
    file_service.not_exists_set.borrow_mut().set(resource.clone(), true);
    file_service.fire_file_changes(Rc::new(FileChangesEvent::new(&[FileChange::new(FileChangeType::Deleted, resource.clone())], false)));
}

/// `accessor.fileService.notExistsSet.delete(resource)` e o `fireFileChanges` do `ADDED`.
fn mark_added(file_service: &TestFileService, resource: &URI) {
    file_service.not_exists_set.borrow_mut().delete(resource);
    file_service.fire_file_changes(Rc::new(FileChangesEvent::new(&[FileChange::new(FileChangeType::Added, resource.clone())], false)));
}

/// As cinco asserções de estado que o caso `save (errors)` repete depois de cada passo.
#[track_caller]
fn assert_save_states(working_copy: &TestStoredFileWorkingCopy, error: bool, saved: bool, pending_save: bool, conflict: bool, dirty: bool) {
    assert_eq!(working_copy.has_state(ErrorState), error);
    assert_eq!(working_copy.has_state(Saved), saved);
    assert_eq!(working_copy.has_state(PendingSave), pending_save);
    assert_eq!(working_copy.has_state(Conflict), conflict);
    assert_eq!(working_copy.is_dirty(), dirty);
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `assert.strictEqual(workingCopy.isReadonly(), false)`.
#[track_caller]
fn assert_readonly_is_false(working_copy: &TestStoredFileWorkingCopy) {
    assert!(matches!(working_copy.is_readonly(), Readonly::Bool(false)));
}

//#endregion

//#region StoredFileWorkingCopy (with custom save)

#[test]
fn save_custom_implemented() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let (_accessor, working_copy) = setup_custom_save(&disposables);

        let (saved_counter, last_save_event) = track_saves(&disposables, &working_copy);
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        // unresolved
        working_copy.save_stored(None).await.unwrap();
        assert_eq!(saved_counter.get(), 0);
        assert_eq!(save_error_counter.get(), 0);

        // simple
        working_copy.resolve().await.unwrap();
        model_of(&working_copy).update_contents("hello save");
        working_copy.save_stored(None).await.unwrap();

        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
        assert_eq!(last_save_event.borrow().as_ref().expect("houve um salvar").event.reason, Some(SaveReason::Explicit));
        assert!(model_of(&working_copy).pushed_stack_element());
        assert_eq!(model_of(&working_copy).save_counter(), 1);

        // error
        model_of(&working_copy).update_contents("hello save error");
        model_of(&working_copy).set_throw_on_save(true);
        working_copy.save_stored(None).await.unwrap();

        assert_eq!(save_error_counter.get(), 1);
        assert!(working_copy.has_state(ErrorState));
    });
}

#[test]
fn save_cancelled_custom_implemented() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let (_accessor, working_copy) = setup_custom_save(&disposables);

        let (saved_counter, last_save_event) = track_saves(&disposables, &working_copy);
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        working_copy.resolve().await.unwrap();
        let (save_operation, resolve) = Promise::<()>::new();
        model_of(&working_copy).set_save_operation(save_operation);

        model_of(&working_copy).update_contents("first");
        let first_save = working_copy.save_stored(None);
        // cancel the first save by requesting a second while it is still mid operation
        model_of(&working_copy).update_contents("second");
        let second_save = working_copy.save_stored(None);
        resolve.resolve(());
        first_save.await.unwrap();
        second_save.await.unwrap();

        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
        assert_eq!(last_save_event.borrow().as_ref().expect("houve um salvar").event.reason, Some(SaveReason::Explicit));
        assert!(model_of(&working_copy).pushed_stack_element());
        assert_eq!(model_of(&working_copy).save_counter(), 1);
    });
}

//#endregion

//#region StoredFileWorkingCopy

#[test]
fn registers_with_working_copy_service() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&disposables);
    let working_copy = fixture.working_copy();

    assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 1);

    working_copy.dispose();

    assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 0);
}

#[test]
fn orphaned_tracking() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        assert!(!working_copy.has_state(Orphan));

        let on_did_change_orphaned = working_copy.on_did_change_orphaned().to_promise(None);
        mark_deleted(&file_service, &fixture.resource);

        on_did_change_orphaned.await.unwrap();
        assert!(working_copy.has_state(Orphan));

        let on_did_change_orphaned = working_copy.on_did_change_orphaned().to_promise(None);
        mark_added(&file_service, &fixture.resource);

        on_did_change_orphaned.await.unwrap();
        assert!(!working_copy.has_state(Orphan));
    });
}

#[test]
fn dirty_modified() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        assert!(!working_copy.is_modified());
        assert!(!working_copy.is_dirty());
        assert!(!working_copy.has_state(Dirty));

        working_copy.resolve().await.unwrap();
        assert!(working_copy.is_resolved());

        let change_dirty_counter = count_events(&disposables, working_copy.on_did_change_dirty());
        let content_change_counter = count_events(&disposables, working_copy.on_did_change_content());
        let saved_counter = count_events(&disposables, working_copy.on_did_save());

        // Dirty from: Model content change
        model_of(&working_copy).update_contents("hello dirty");
        assert_eq!(content_change_counter.get(), 1);

        assert!(working_copy.is_modified());
        assert!(working_copy.is_dirty());
        assert!(working_copy.has_state(Dirty));
        assert_eq!(change_dirty_counter.get(), 1);

        working_copy.save_stored(None).await.unwrap();

        assert!(!working_copy.is_modified());
        assert!(!working_copy.is_dirty());
        assert!(!working_copy.has_state(Dirty));
        assert_eq!(change_dirty_counter.get(), 2);
        assert_eq!(saved_counter.get(), 1);

        // Dirty from: Initial contents
        working_copy.resolve_stored(resolve_with_contents("hello dirty stream")).await.unwrap();

        assert_eq!(content_change_counter.get(), 2); // content of model did not change
        assert!(working_copy.is_modified());
        assert!(working_copy.is_dirty());
        assert!(working_copy.has_state(Dirty));
        assert_eq!(change_dirty_counter.get(), 3);

        working_copy.revert(soft_revert()).await.unwrap();

        assert!(!working_copy.is_modified());
        assert!(!working_copy.is_dirty());
        assert!(!working_copy.has_state(Dirty));
        assert_eq!(change_dirty_counter.get(), 4);

        // Modified from: API
        working_copy.mark_modified();

        assert!(working_copy.is_modified());
        assert!(working_copy.is_dirty());
        assert!(working_copy.has_state(Dirty));
        assert_eq!(change_dirty_counter.get(), 5);

        working_copy.revert(None).await.unwrap();

        assert!(!working_copy.is_modified());
        assert!(!working_copy.is_dirty());
        assert!(!working_copy.has_state(Dirty));
        assert_eq!(change_dirty_counter.get(), 6);
    });
}

#[test]
fn dirty_working_copy_marks_non_dirty_when_undo_reaches_saved_version_id() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        working_copy.resolve().await.unwrap();

        model_of(&working_copy).update_contents("hello saved state");
        working_copy.save_stored(None).await.unwrap();
        assert!(!working_copy.is_dirty());

        model_of(&working_copy).update_contents("changing content once");
        assert!(working_copy.is_dirty());

        // Simulate an undo that goes back to the last (saved) version ID
        model_of(&working_copy).decrement_version_id();

        model_of(&working_copy).fire_content_change_event(StoredFileWorkingCopyModelContentChangedEvent { is_redoing: false, is_undoing: true });
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn resolve_without_backup() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        let on_did_resolve_counter = count_events(&disposables, working_copy.on_did_resolve());

        // resolve from file
        working_copy.resolve().await.unwrap();
        assert!(working_copy.is_resolved());
        assert_eq!(on_did_resolve_counter.get(), 1);
        assert_eq!(contents_of(&working_copy), "Hello Html");

        // dirty resolve returns early
        model_of(&working_copy).update_contents("hello resolve");
        assert!(working_copy.is_dirty());
        working_copy.resolve().await.unwrap();
        assert_eq!(on_did_resolve_counter.get(), 1);
        assert_eq!(contents_of(&working_copy), "hello resolve");

        // dirty resolve with contents updates contents
        working_copy.resolve_stored(resolve_with_contents("hello initial contents")).await.unwrap();
        assert!(working_copy.is_dirty());
        assert_eq!(contents_of(&working_copy), "hello initial contents");
        assert_eq!(on_did_resolve_counter.get(), 2);

        // resolve with pending save returns directly
        let pending_save = working_copy.save_stored(None);
        working_copy.resolve().await.unwrap();
        pending_save.await.unwrap();
        assert!(!working_copy.is_dirty());
        assert_eq!(contents_of(&working_copy), "hello initial contents");
        assert_eq!(on_did_resolve_counter.get(), 2);

        // disposed resolve is not throwing an error
        working_copy.dispose();
        working_copy.resolve().await.unwrap();
        assert!(working_copy.is_disposed());
        assert_eq!(on_did_resolve_counter.get(), 2);
    });
}

#[test]
fn resolve_with_backup() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        working_copy.resolve_stored(resolve_with_contents("hello backup")).await.unwrap();

        let backup = working_copy.backup(&CancellationToken::None).await.unwrap();
        fixture.accessor.working_copy_backup_service.backup(&WorkingCopyIdentifier::of(&*working_copy), backup.content, None, backup.meta, None).await.unwrap();

        assert!(fixture.accessor.working_copy_backup_service.has_backup_sync(&WorkingCopyIdentifier::of(&*working_copy), None, None));

        working_copy.dispose();

        // first resolve loads from backup
        let working_copy = fixture.replace_working_copy();
        working_copy.resolve().await.unwrap();

        assert!(working_copy.is_dirty());
        assert_readonly_is_false(&working_copy);
        assert_eq!(contents_of(&working_copy), "hello backup");

        model_of(&working_copy).update_contents("hello updated");
        working_copy.save_stored(None).await.unwrap();

        // subsequent resolve ignores any backups
        working_copy.resolve().await.unwrap();

        assert!(!working_copy.is_dirty());
        assert_eq!(contents_of(&working_copy), "Hello Html");
    });
}

#[test]
fn resolve_with_backup_preserves_metadata_and_orphaned_state() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        working_copy.resolve_stored(resolve_with_contents("hello backup")).await.unwrap();

        let orphaned_promise = working_copy.on_did_change_orphaned().to_promise(None);

        mark_deleted(&fixture.accessor.file_service, &fixture.resource);

        orphaned_promise.await.unwrap();
        assert!(working_copy.has_state(Orphan));

        let backup = working_copy.backup(&CancellationToken::None).await.unwrap();
        fixture
            .accessor
            .working_copy_backup_service
            .backup(&WorkingCopyIdentifier::of(&*working_copy), backup.content.clone(), None, backup.meta.clone(), None)
            .await
            .unwrap();

        assert!(fixture.accessor.working_copy_backup_service.has_backup_sync(&WorkingCopyIdentifier::of(&*working_copy), None, None));

        working_copy.dispose();

        let working_copy = fixture.replace_working_copy();
        working_copy.resolve().await.unwrap();

        assert!(working_copy.has_state(Orphan));

        let backup2 = working_copy.backup(&CancellationToken::None).await.unwrap();
        assert_deep_strict_equal(&backup.meta.expect("o backup tem metadados"), &backup2.meta.expect("o backup tem metadados"));
    });
}

#[test]
fn resolve_updates_orphaned_state_accordingly() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        working_copy.resolve().await.unwrap();

        let orphaned_promise = working_copy.on_did_change_orphaned().to_promise(None);

        mark_deleted(&file_service, &fixture.resource);

        orphaned_promise.await.unwrap();
        assert!(working_copy.has_state(Orphan));

        // resolving clears orphaned state when successful
        file_service.not_exists_set.borrow_mut().delete(&fixture.resource);
        working_copy.resolve_stored(force_read_from_file()).await.unwrap();
        assert!(!working_copy.has_state(Orphan));

        // resolving adds orphaned state when fail to read
        *file_service.read_should_throw_error.borrow_mut() = Some(file_error("file not found", FileOperationResult::FileNotFound));
        let resolved = working_copy.resolve().await;
        *file_service.read_should_throw_error.borrow_mut() = None;
        resolved.unwrap();
        assert!(working_copy.has_state(Orphan));
    });
}

#[test]
fn stat_readonly_and_stat_locked_can_change_when_decreased_mtime_is_ignored() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        working_copy.resolve().await.unwrap();

        let stat = working_copy.last_resolved_file_stat().expect("a working copy resolvida tem o stat");
        let mut changed = stat.clone().into_stat();
        changed.mtime = Some(stat.mtime() - 1);
        changed.readonly = Some(!stat.readonly());
        changed.locked = Some(!stat.locked());
        *file_service.read_should_throw_error.borrow_mut() = Some(FileOperationError::not_modified_since("error", FileStatWithMetadata::new(changed)).into_error());
        let resolved = working_copy.resolve().await;
        *file_service.read_should_throw_error.borrow_mut() = None;
        resolved.unwrap();

        let last = working_copy.last_resolved_file_stat().expect("a working copy resolvida tem o stat");
        assert_eq!(last.mtime(), stat.mtime(), "mtime should not decrease");
        assert_ne!(last.readonly(), stat.readonly(), "readonly should have changed despite simultaneous attempt to decrease mtime");
        assert_ne!(last.locked(), stat.locked(), "locked should have changed despite simultaneous attempt to decrease mtime");
    });
}

#[test]
fn resolve_file_not_modified_since_can_be_handled_for_resolved_working_copies() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        working_copy.resolve().await.unwrap();

        *file_service.read_should_throw_error.borrow_mut() = Some(file_error("file not modified since", FileOperationResult::FileNotModifiedSince));
        let resolved = working_copy.resolve().await;
        *file_service.read_should_throw_error.borrow_mut() = None;
        resolved.unwrap();

        assert_eq!(contents_of(&working_copy), "Hello Html");
    });
}

#[test]
fn resolve_file_not_modified_since_still_updates_readonly_state() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        let readonly_change_counter = count_events(&disposables, working_copy.on_did_change_readonly());

        working_copy.resolve().await.unwrap();

        assert_readonly_is_false(&working_copy);

        let stat = file_service
            .resolve_with_metadata(&working_copy.resource(), Some(ResolveFileOptions { resolve_to: Vec::new(), resolve_single_child_descendants: false, resolve_metadata: true }))
            .await
            .unwrap();

        *file_service.read_should_throw_error.borrow_mut() = Some(FileOperationError::not_modified_since("file not modified since", stat_with_readonly(&stat, true)).into_error());
        let resolved = working_copy.resolve().await;
        *file_service.read_should_throw_error.borrow_mut() = None;
        resolved.unwrap();

        assert!(working_copy.is_readonly().is_readonly());
        assert_eq!(readonly_change_counter.get(), 1);

        *file_service.read_should_throw_error.borrow_mut() = Some(FileOperationError::not_modified_since("file not modified since", stat_with_readonly(&stat, false)).into_error());
        let resolved = working_copy.resolve().await;
        *file_service.read_should_throw_error.borrow_mut() = None;
        resolved.unwrap();

        assert_readonly_is_false(&working_copy);
        assert_eq!(readonly_change_counter.get(), 2);
    });
}

#[test]
fn resolve_does_not_alter_content_when_model_content_changed_in_parallel() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        working_copy.resolve().await.unwrap();

        let resolve_promise = working_copy.resolve();

        model_of(&working_copy).update_contents("changed content");

        resolve_promise.await.unwrap();

        assert!(working_copy.is_dirty());
        assert_eq!(contents_of(&working_copy), "changed content");
    });
}

#[test]
fn backup() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        working_copy.resolve().await.unwrap();
        model_of(&working_copy).update_contents("hello backup");

        let backup = working_copy.backup(&CancellationToken::None).await.unwrap();

        assert!(backup.meta.is_some());

        let backup_contents = match backup.content {
            Some(WorkingCopyBackupContent::Stream(stream)) => Some(stream_to_buffer(&*stream).await.unwrap().to_string()),
            Some(WorkingCopyBackupContent::Readable(readable)) => Some(readable_to_buffer(&mut **readable.borrow_mut()).to_string()),
            None => None,
        };

        assert_eq!(backup_contents.as_deref(), Some("hello backup"));
    });
}

#[test]
fn save_no_errors_simple() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        let (saved_counter, last_save_event) = track_saves(&disposables, &working_copy);
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        // unresolved
        working_copy.save_stored(None).await.unwrap();
        assert_eq!(saved_counter.get(), 0);
        assert_eq!(save_error_counter.get(), 0);

        // simple
        working_copy.resolve().await.unwrap();
        model_of(&working_copy).update_contents("hello save");
        working_copy.save_stored(None).await.unwrap();

        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
        assert_eq!(last_save_event.borrow().as_ref().expect("houve um salvar").event.reason, Some(SaveReason::Explicit));
        assert!(model_of(&working_copy).pushed_stack_element());
    });
}

#[test]
fn save_no_errors_save_reason() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        let (saved_counter, last_save_event) = track_saves(&disposables, &working_copy);
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        // save reason
        working_copy.resolve().await.unwrap();
        model_of(&working_copy).update_contents("hello save");

        let source = with_save_source_registry(|registry| registry.register_source("testSource", "Hello Save"));
        working_copy
            .save_stored(Some(StoredFileWorkingCopySaveAsOptions::from_save_options(SaveOptions {
                reason: Some(SaveReason::Auto),
                source: Some(source.clone()),
                ..SaveOptions::default()
            })))
            .await
            .unwrap();

        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
        let last_save_event = last_save_event.borrow();
        let last_save_event = last_save_event.as_ref().expect("houve um salvar");
        assert_eq!(last_save_event.event.reason, Some(SaveReason::Auto));
        assert_eq!(last_save_event.event.source, Some(source));
    });
}

#[test]
fn save_no_errors_multiple() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        let saved_counter = count_events(&disposables, working_copy.on_did_save_stored());
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        // multiple saves in parallel are fine and result
        // in a single save when content does not change
        working_copy.resolve().await.unwrap();
        model_of(&working_copy).update_contents("hello save");
        promises::settled(vec![
            working_copy.save_stored(with_reason(SaveReason::Auto)),
            working_copy.save_stored(with_reason(SaveReason::Explicit)),
            working_copy.save_stored(with_reason(SaveReason::WindowChange)),
        ])
        .await
        .unwrap();

        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn save_no_errors_multiple_cancellation() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        let saved_counter = count_events(&disposables, working_copy.on_did_save_stored());
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        // multiple saves in parallel are fine and result
        // in just one save operation (the second one
        // cancels the first)
        working_copy.resolve().await.unwrap();
        model_of(&working_copy).update_contents("hello save");
        let first_save = working_copy.save_stored(None);
        model_of(&working_copy).update_contents("hello save more");
        let second_save = working_copy.save_stored(None);

        promises::settled(vec![first_save, second_save]).await.unwrap();
        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn save_no_errors_not_forced_but_not_dirty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        let saved_counter = count_events(&disposables, working_copy.on_did_save_stored());
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        // no save when not forced and not dirty
        working_copy.resolve().await.unwrap();
        working_copy.save_stored(None).await.unwrap();
        assert_eq!(saved_counter.get(), 0);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn save_no_errors_forced_but_not_dirty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        let saved_counter = count_events(&disposables, working_copy.on_did_save_stored());
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        // save when forced even when not dirty
        working_copy.resolve().await.unwrap();
        working_copy.save_stored(forced()).await.unwrap();
        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn save_no_errors_save_clears_orphaned() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        let saved_counter = count_events(&disposables, working_copy.on_did_save_stored());
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        working_copy.resolve().await.unwrap();

        // save clears orphaned
        let orphaned_promise = working_copy.on_did_change_orphaned().to_promise(None);

        mark_deleted(&fixture.accessor.file_service, &fixture.resource);

        orphaned_promise.await.unwrap();
        assert!(working_copy.has_state(Orphan));

        working_copy.save_stored(forced()).await.unwrap();
        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 0);
        assert!(!working_copy.is_dirty());
        assert!(!working_copy.has_state(Orphan));
    });
}

#[test]
fn save_errors() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        let saved_counter = count_events(&disposables, working_copy.on_did_save_stored());
        let save_error_counter = count_events(&disposables, working_copy.on_did_save_error());

        working_copy.resolve().await.unwrap();

        // save error: any error marks working copy dirty
        *file_service.write_should_throw_error.borrow_mut() = Some(file_error("write error", FileOperationResult::FilePermissionDenied));
        let saved = working_copy.save_stored(forced()).await;
        *file_service.write_should_throw_error.borrow_mut() = None;
        saved.unwrap();

        assert_eq!(saved_counter.get(), 0);
        assert_eq!(save_error_counter.get(), 1);
        assert_save_states(&working_copy, true, false, false, false, true);

        // save is a no-op unless forced when in error case
        working_copy.save_stored(with_reason(SaveReason::Auto)).await.unwrap();
        assert_eq!(saved_counter.get(), 0);
        assert_eq!(save_error_counter.get(), 1);
        assert_save_states(&working_copy, true, false, false, false, true);

        // save clears error flags when successful
        working_copy.save_stored(with_reason(SaveReason::Explicit)).await.unwrap();
        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 1);
        assert_save_states(&working_copy, false, true, false, false, false);

        // save error: conflict
        *file_service.write_should_throw_error.borrow_mut() = Some(file_error("write error conflict", FileOperationResult::FileModifiedSince));
        let saved = working_copy.save_stored(forced()).await;
        *file_service.write_should_throw_error.borrow_mut() = None;
        drop(saved); // error is expected

        assert_eq!(saved_counter.get(), 1);
        assert_eq!(save_error_counter.get(), 2);
        assert_save_states(&working_copy, true, false, false, true, true);

        // save clears error flags when successful
        working_copy.save_stored(with_reason(SaveReason::Explicit)).await.unwrap();
        assert_eq!(saved_counter.get(), 2);
        assert_eq!(save_error_counter.get(), 2);
        assert_save_states(&working_copy, false, true, false, false, false);
    });
}

#[test]
fn save_errors_bubbles_up_with_ignore_error_handler() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        working_copy.resolve().await.unwrap();

        *file_service.write_should_throw_error.borrow_mut() = Some(file_error("write error", FileOperationResult::FilePermissionDenied));
        let saved = working_copy
            .save_stored(save_as(|options| {
                options.options.base.force = Some(true);
                options.options.ignore_error_handler = Some(true);
            }))
            .await;
        *file_service.write_should_throw_error.borrow_mut() = None;

        assert!(saved.is_err());
    });
}

#[test]
fn save_returns_false_when_save_fails() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        working_copy.resolve().await.unwrap();

        *file_service.write_should_throw_error.borrow_mut() = Some(file_error("write error", FileOperationResult::FilePermissionDenied));
        let saved = working_copy.save_stored(forced()).await;
        *file_service.write_should_throw_error.borrow_mut() = None;
        assert!(!saved.unwrap());

        let saved = working_copy.save_stored(forced()).await.unwrap();
        assert!(saved);
    });
}

#[test]
fn save_participant_registration() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let working_copy_file_service = fixture.accessor.working_copy_file_service.clone();

        working_copy.resolve().await.unwrap();

        assert!(!working_copy_file_service.has_save_participants());

        let participation_counter = Rc::new(Cell::new(0));
        let (counter, expected) = (participation_counter.clone(), working_copy.clone());
        let disposable = working_copy_file_service.add_save_participant(save_participant(move |wc, _context| {
            if std::ptr::addr_eq(Rc::as_ptr(&expected), Rc::as_ptr(&wc)) {
                counter.set(counter.get() + 1);
            }

            Promise::resolved(())
        }));

        assert!(working_copy_file_service.has_save_participants());

        working_copy.save_stored(forced()).await.unwrap();
        assert_eq!(participation_counter.get(), 1);

        working_copy
            .save_stored(save_as(|options| {
                options.options.base.force = Some(true);
                options.options.base.skip_save_participants = Some(true);
            }))
            .await
            .unwrap();
        assert_eq!(participation_counter.get(), 1);

        disposable.dispose();
        assert!(!working_copy_file_service.has_save_participants());

        working_copy.save_stored(forced()).await.unwrap();
        assert_eq!(participation_counter.get(), 1);
    });
}

/// `testSaveFromSaveParticipant(workingCopy, async)`.
async fn test_save_from_save_participant(accessor: &TestServiceAccessor, working_copy: &Rc<TestStoredFileWorkingCopy>, is_async: bool) {
    let from = uri_file("testFrom");
    assert!(!accessor.working_copy_file_service.has_save_participants());

    let participant_working_copy = working_copy.clone();
    let disposable = accessor.working_copy_file_service.add_save_participant(save_participant(move |_wc, _context| {
        let working_copy = participant_working_copy.clone();

        async_fn(async move {
            if is_async {
                timeout(10).await?;
            }

            working_copy.save_stored(forced()).await?;
            Ok(())
        })
    }));

    assert!(accessor.working_copy_file_service.has_save_participants());

    working_copy
        .save_stored(save_as(|options| {
            options.options.base.force = Some(true);
            options.from = Some(from);
        }))
        .await
        .unwrap();

    disposable.dispose();
}

#[test]
fn save_participant_calling_save_from_within_is_unsupported_but_does_not_explode_sync_save() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        working_copy.resolve().await.unwrap();

        test_save_from_save_participant(&fixture.accessor, &working_copy, false).await;
    });
}

#[test]
fn save_participant_calling_save_from_within_is_unsupported_but_does_not_explode_async_save() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        working_copy.resolve().await.unwrap();

        test_save_from_save_participant(&fixture.accessor, &working_copy, true).await;
    });
}

#[test]
fn save_participant_carries_context() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let working_copy_file_service = fixture.accessor.working_copy_file_service.clone();

        working_copy.resolve().await.unwrap();

        let from = uri_file("testFrom");
        assert!(!working_copy_file_service.has_save_participants());

        let received = Rc::new(RefCell::new(None));
        let received_context = received.clone();
        let disposable = working_copy_file_service.add_save_participant(save_participant(move |_wc, context| {
            *received_context.borrow_mut() = Some(context);

            Promise::resolved(())
        }));

        assert!(working_copy_file_service.has_save_participants());

        working_copy
            .save_stored(save_as(|options| {
                options.options.base.force = Some(true);
                options.from = Some(from.clone());
            }))
            .await
            .unwrap();

        let context = received.borrow_mut().take().expect("o participante foi chamado");
        assert_eq!(context.reason, SaveReason::Explicit);
        assert_eq!(context.saved_from.as_ref().map(uri_string), Some(uri_string(&from)));

        disposable.dispose();
    });
}

#[test]
fn revert() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        working_copy.resolve().await.unwrap();
        model_of(&working_copy).update_contents("hello revert");

        let reverted_counter = count_events(&disposables, working_copy.on_did_revert());

        // revert: soft
        working_copy.revert(soft_revert()).await.unwrap();

        assert_eq!(reverted_counter.get(), 1);
        assert!(!working_copy.is_dirty());
        assert_eq!(contents_of(&working_copy), "hello revert");

        // revert: not forced
        working_copy.revert(None).await.unwrap();
        assert_eq!(reverted_counter.get(), 1);
        assert_eq!(contents_of(&working_copy), "hello revert");

        // revert: forced
        working_copy.revert(forced_revert()).await.unwrap();
        assert_eq!(reverted_counter.get(), 2);
        assert_eq!(contents_of(&working_copy), "Hello Html");

        // revert: forced, error
        model_of(&working_copy).update_contents("hello revert");
        *file_service.read_should_throw_error.borrow_mut() = Some(file_error("error", FileOperationResult::FilePermissionDenied));
        let reverted = working_copy.revert(forced_revert()).await;
        *file_service.read_should_throw_error.borrow_mut() = None;
        drop(reverted); // expected (our error)

        assert_eq!(reverted_counter.get(), 2);
        assert!(working_copy.is_dirty());

        // revert: forced, file not found error is ignored
        model_of(&working_copy).update_contents("hello revert");
        *file_service.read_should_throw_error.borrow_mut() = Some(file_error("error", FileOperationResult::FileNotFound));
        let reverted = working_copy.revert(forced_revert()).await;
        *file_service.read_should_throw_error.borrow_mut() = None;
        drop(reverted); // expected (our error)

        assert_eq!(reverted_counter.get(), 3);
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn state() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        assert!(working_copy.has_state(Saved));

        working_copy.resolve_stored(resolve_with_contents("hello state")).await.unwrap();
        assert!(working_copy.has_state(Dirty));

        let save_promise = working_copy.save_stored(None);
        assert!(working_copy.has_state(Dirty));
        assert!(!working_copy.has_state(Saved));
        assert!(working_copy.has_state(PendingSave));

        save_promise.await.unwrap();

        assert!(!working_copy.has_state(Dirty));
        assert!(working_copy.has_state(Saved));
        assert!(!working_copy.has_state(PendingSave));
    });
}

#[test]
fn join_state() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        working_copy.resolve_stored(resolve_with_contents("hello state")).await.unwrap();

        let _save_promise = working_copy.save_stored(None);
        assert!(working_copy.has_state(PendingSave));

        working_copy.join_state(PendingSave).await.unwrap();

        assert!(!working_copy.has_state(Dirty));
        assert!(working_copy.has_state(Saved));
        assert!(!working_copy.has_state(PendingSave));
    });
}

#[test]
fn is_readonly_is_resolved_dispose_is_disposed() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();

        assert!(!working_copy.is_resolved());
        assert_readonly_is_false(&working_copy);
        assert!(!working_copy.is_disposed());

        working_copy.resolve().await.unwrap();

        assert!(working_copy.model().is_some());
        assert!(working_copy.is_resolved());
        assert_readonly_is_false(&working_copy);
        assert!(!working_copy.is_disposed());

        let disposed_event = Rc::new(Cell::new(false));
        let listener_flag = disposed_event.clone();
        disposables.add(working_copy.on_will_dispose().listen(move |_| listener_flag.set(true)));

        let disposed_model_event = Rc::new(Cell::new(false));
        let listener_flag = disposed_model_event.clone();
        disposables.add(model_of(&working_copy).on_will_dispose().listen(move |_| listener_flag.set(true)));

        working_copy.dispose();

        assert!(working_copy.is_disposed());
        assert!(disposed_event.get());
        assert!(disposed_model_event.get());
    });
}

#[test]
fn readonly_change_event() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let working_copy = fixture.working_copy();
        let file_service = fixture.accessor.file_service.clone();

        file_service.readonly.set(true);

        working_copy.resolve().await.unwrap();

        assert!(working_copy.is_readonly().is_readonly());

        file_service.readonly.set(false);

        let readonly_event = Rc::new(Cell::new(false));
        let listener_flag = readonly_event.clone();
        disposables.add(working_copy.on_did_change_readonly().listen(move |_| listener_flag.set(true)));

        working_copy.resolve().await.unwrap();

        assert_readonly_is_false(&working_copy);
        assert!(readonly_event.get());
    });
}

//#endregion
