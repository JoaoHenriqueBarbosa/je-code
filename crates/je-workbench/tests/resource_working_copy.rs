//! Porte de `src/vs/workbench/services/workingCopy/test/browser/resourceWorkingCopy.test.ts` (suíte
//! `ResourceWorkingCopy`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso e o `setup` ([`setup`]) logo depois; o [`Fixture`] descarta o `disposables` do original no `Drop`, antes
//!   do guarda conferir os vazamentos.
//! - **`workbenchInstantiationService` e `TestServiceAccessor`.** Só o `fileService` é lido, e é o `TestFileService`.
//! - **`TestResourceWorkingCopy extends ResourceWorkingCopy`** é a struct [`TestResourceWorkingCopy`], que guarda o
//!   [`ResourceWorkingCopy`] e implementa o [`IWorkingCopy`] (os membros abstratos); `Event.None` é `Event::none()`.
//! - **`Event.toPromise`** é `Event::to_promise`, e `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`new FileChangesEvent([{ resource, type }], false)`** é `FileChangesEvent::new(&[FileChange::new(type, resource)],
//!   false)`, e `notExistsSet.set(resource, true)` e `.delete(resource)` operam o `ResourceMap` do dublê.
//! - O caso `dispose, isDisposed` é `async` no original sem nenhum `await`: aqui é um teste comum.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::files::{FileChange, FileChangeType, FileChangesEvent};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::uri_file;
use je_workbench::common::editor::{RevertOptions, SaveOptions};
use je_workbench::services::working_copy::resource_working_copy::{IResourceWorkingCopy, ResourceWorkingCopy};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackup, WorkingCopyCapabilities, WorkingCopySaveEvent};
use je_workbench::test::common::workbench_test_services::TestFileService;
use je_base::errors::Error;

struct TestResourceWorkingCopy {
    base: Rc<ResourceWorkingCopy>,
}

impl TestResourceWorkingCopy {
    fn new(resource: URI, file_service: Rc<dyn IFileService>) -> Rc<Self> {
        Rc::new(Self { base: ResourceWorkingCopy::new(resource, file_service) })
    }
}

impl IDisposable for TestResourceWorkingCopy {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IResourceWorkingCopy for TestResourceWorkingCopy {
    fn resource_working_copy(&self) -> &ResourceWorkingCopy {
        &self.base
    }
}

impl IWorkingCopy for TestResourceWorkingCopy {
    fn working_copy_type_id(&self) -> String {
        "testTypeId".to_string()
    }

    fn resource(&self) -> URI {
        self.base.resource().clone()
    }

    fn name(&self) -> String {
        "testName".to_string()
    }

    fn capabilities(&self) -> WorkingCopyCapabilities {
        WorkingCopyCapabilities::NONE
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_content(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        Event::none()
    }

    fn is_dirty(&self) -> bool {
        false
    }

    fn is_modified(&self) -> bool {
        self.is_dirty()
    }

    fn backup(&self, _token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        Promise::rejected(Error::new("Method not implemented."))
    }

    fn save(&self, _options: Option<SaveOptions>) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn revert(&self, _options: Option<RevertOptions>) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O que o `setup` do original monta: o `disposables`, o `accessor.fileService` e o `workingCopy`.
struct Fixture {
    disposables: Rc<DisposableStore>,
    file_service: Rc<TestFileService>,
    working_copy: Rc<TestResourceWorkingCopy>,
    resource: URI,
}

impl Drop for Fixture {
    /// O `teardown`: `disposables.clear()`.
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

fn setup() -> Fixture {
    let disposables = DisposableStore::new();
    let resource = uri_file("test/resource");
    let file_service = disposables.add_rc(TestFileService::new());

    let working_copy = disposables.add_rc(TestResourceWorkingCopy::new(resource.clone(), file_service.clone()));

    Fixture { disposables, file_service, working_copy, resource }
}

#[test]
fn orphaned_tracking() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { file_service, working_copy, resource, .. } = &setup();

        assert!(!working_copy.is_orphaned());

        let on_did_change_orphaned_promise = working_copy.on_did_change_orphaned().to_promise(None);
        file_service.not_exists_set.borrow_mut().set(resource.clone(), true);
        file_service.fire_file_changes(Rc::new(FileChangesEvent::new(&[FileChange::new(FileChangeType::Deleted, resource.clone())], false)));

        on_did_change_orphaned_promise.promise.clone().await.unwrap();
        assert!(working_copy.is_orphaned());

        let on_did_change_orphaned_promise = working_copy.on_did_change_orphaned().to_promise(None);
        file_service.not_exists_set.borrow_mut().delete(resource);
        file_service.fire_file_changes(Rc::new(FileChangesEvent::new(&[FileChange::new(FileChangeType::Added, resource.clone())], false)));

        on_did_change_orphaned_promise.promise.clone().await.unwrap();
        assert!(!working_copy.is_orphaned());
    });
}

#[test]
fn dispose_is_disposed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup();
    let working_copy = &fixture.working_copy;

    assert!(!working_copy.is_disposed());

    let disposed_event = Rc::new(Cell::new(false));
    let flag = disposed_event.clone();
    fixture.disposables.add(working_copy.on_will_dispose().listen(move |_| flag.set(true)));

    working_copy.dispose();

    assert!(working_copy.is_disposed());
    assert!(disposed_event.get());
}
