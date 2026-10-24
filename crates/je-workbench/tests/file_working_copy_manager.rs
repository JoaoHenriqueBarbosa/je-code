//! Porte de `src/vs/workbench/services/workingCopy/test/browser/fileWorkingCopyManager.test.ts` (suíte `FileWorkingCopyManager`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada
//!   caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois; o `teardown`
//!   (`disposables.clear()`) é o descarte do guarda. Os casos `async` rodam em `run_with_faked_timers`; `await` é
//!   `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService`, `TestServiceAccessor` e os dublês.** Estão em
//!   `tests/support/untitled_file_working_copy_support.rs` (o acessor compõe o de `stored_file_working_copy_support`, troca o
//!   `IWorkingCopyFileService` pelo real e acrescenta os diálogos, o serviço de caminho, o ambiente e as decorações). Os dois
//!   `registerProvider(...)` do `setup` são [`FileWorkingCopyManagerAccessor::register_providers`] (o provedor em memória do
//!   porte entra no guarda de vazamentos).
//! - **`manager.resolve(...)`.** As quatro sobrecargas de `resolve` são o argumento `FileWorkingCopyResolveArg`, e o resultado é a
//!   união `FileWorkingCopy`: [`resolve_stored`] e [`resolve_untitled`] escolhem o ramo (o `instanceof` do caso `resolve` é o
//!   `into_stored` e o `into_untitled`, que reprovam se o ramo for outro).
//! - **`manager.get(...)`, `manager.workingCopies`.** Devolvem a união `FileWorkingCopy`; `a === b` é `is_same` (união) ou
//!   `Rc::ptr_eq` (working copy concreta); `get(...)` indefinido é `is_none()`.
//! - **`result?.resource.toString()`.** É o texto do recurso do resultado, ou `None` se o "Salvar como" foi cancelado.
//! - **`accessor.uriIdentityService.extUri.isEqual(source, target)`.** É `ext_uri().is_equal(...)` do serviço de identidade de URI.
//! - **`testSaveAsFile`.** É a função [`test_save_as_file`], que os onze casos de "Salvar como" de arquivo chamam.
//! - **`URI.file(...)`.** É `uri_file(...)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::buffer::{VSBuffer, buffer_to_stream};
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::uri::{URI, UriComponents};
use je_platform::resources::uri_file;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::services::untitled::untitled_text_editor_service::AssociatedResource;
use je_workbench::services::working_copy::abstract_file_working_copy_manager::IBaseFileWorkingCopyManager;
use je_workbench::services::working_copy::file_working_copy_manager::{FileWorkingCopy, FileWorkingCopyResolveArg};
use je_workbench::services::working_copy::stored_file_working_copy_manager::IStoredFileWorkingCopyManager;
use je_workbench::services::working_copy::untitled_file_working_copy::UntitledFileWorkingCopyInitialContents;
use je_workbench::services::working_copy::untitled_file_working_copy_manager::{
    IUntitledFileWorkingCopyManager, NewOrExistingUntitledFileWorkingCopyOptions, NewUntitledFileWorkingCopyOptions, NewUntitledFileWorkingCopyWithAssociatedResourceOptions,
    UntitledFileWorkingCopyResolveOptions,
};
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use support::stored_file_working_copy_support::TestStoredFileWorkingCopy;
use support::untitled_file_working_copy_support::{FileWorkingCopyManagerAccessor, TestFileWorkingCopyManager, TestUntitledFileWorkingCopy, count_events};

//#region Utilitários dos casos

/// O que o `setup` da suíte monta: o acessor e o gerenciador.
struct Fixture {
    accessor: FileWorkingCopyManagerAccessor,
    manager: Rc<TestFileWorkingCopyManager>,
}

/// O `setup`: o acessor, os provedores de arquivo e o `disposables.add(new FileWorkingCopyManager(...))`.
fn setup(disposables: &LeakGuard) -> Fixture {
    let accessor = FileWorkingCopyManagerAccessor::new(&disposables.store());
    accessor.register_providers(&disposables.store());
    let manager = disposables.add_rc(accessor.new_manager("testFileWorkingCopyType"));

    Fixture { accessor, manager }
}

/// `manager.resolve(resource, options?)` de um recurso que não é `untitled`.
async fn resolve_stored(manager: &TestFileWorkingCopyManager, resource: &URI) -> Rc<TestStoredFileWorkingCopy> {
    manager.resolve(FileWorkingCopyResolveArg::resource(resource, None)).await.unwrap().into_stored().expect("o resultado é uma working copy armazenada")
}

/// `manager.resolve(options?)` de uma working copy sem título.
async fn resolve_untitled(manager: &TestFileWorkingCopyManager, options: Option<UntitledFileWorkingCopyResolveOptions>) -> Rc<TestUntitledFileWorkingCopy> {
    manager.resolve(FileWorkingCopyResolveArg::Untitled(options)).await.unwrap().into_untitled().expect("o resultado é uma working copy sem título")
}

/// `{ untitledResource }`.
fn existing(untitled_resource: URI) -> UntitledFileWorkingCopyResolveOptions {
    NewOrExistingUntitledFileWorkingCopyOptions { options: NewUntitledFileWorkingCopyOptions::default(), untitled_resource, is_scratchpad: None }.into()
}

/// `{ associatedResource: { path } }`.
fn associated(path: &str) -> UntitledFileWorkingCopyResolveOptions {
    NewUntitledFileWorkingCopyWithAssociatedResourceOptions {
        options: NewUntitledFileWorkingCopyOptions::default(),
        associated_resource: AssociatedResource { path: path.to_string(), ..AssociatedResource::default() },
    }
    .into()
}

/// `{ contents: { value: bufferToStream(VSBuffer.fromString(text)) } }`.
fn with_contents(text: &str) -> UntitledFileWorkingCopyResolveOptions {
    NewUntitledFileWorkingCopyOptions { contents: Some(UntitledFileWorkingCopyInitialContents { value: buffer_to_stream(VSBuffer::from_string(text)), mark_modified: None }) }.into()
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `URI.from({ scheme: Schemas.file, path })`.
fn file_uri_from(path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(schemas::FILE)), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

/// `testSaveAsFile(source, target, resolveSource, resolveTarget)`.
async fn test_save_as_file(disposables: &LeakGuard, fixture: &Fixture, source: &URI, target: &URI, resolve_source: bool, resolve_target: bool) {
    let (accessor, manager) = (&fixture.accessor, &fixture.manager);

    let mut source_working_copy: Option<Rc<TestStoredFileWorkingCopy>> = None;
    if resolve_source {
        let working_copy = disposables.add_rc(resolve_stored(manager, source).await);
        working_copy.model().unwrap().update_contents("hello world");
        assert!(working_copy.is_dirty());
        source_working_copy = Some(working_copy);
    }

    let mut target_working_copy: Option<Rc<TestStoredFileWorkingCopy>> = None;
    if resolve_target {
        let working_copy = disposables.add_rc(resolve_stored(manager, target).await);
        working_copy.model().unwrap().update_contents("hello world");
        assert!(working_copy.is_dirty());
        target_working_copy = Some(working_copy);
    }

    let result = manager.save_as(source, Some(target), None).await.unwrap();
    let result_resource = result.as_ref().map(|working_copy| uri_string(&working_copy.resource()));
    let is_equal = accessor.uri_identity_service.ext_uri().is_equal(Some(source), Some(target), false);
    if is_equal && resolve_source {
        // if the uris are considered equal (different case on macOS/Windows)
        // and the source is to be resolved, the resulting working copy resource
        // will be the source resource because we consider file working copies
        // the same in that case
        assert_eq!(Some(uri_string(source)), result_resource);
    } else if resolve_source || resolve_target {
        assert_eq!(Some(uri_string(target)), result_resource);
    } else if is_equal {
        assert_eq!(None, result_resource);
    } else {
        assert_eq!(Some(uri_string(target)), result_resource);
    }

    if resolve_source {
        assert!(!source_working_copy.unwrap().is_dirty());
    }

    if resolve_target {
        assert!(!target_working_copy.unwrap().is_dirty());
    }

    if let Some(result) = result {
        result.dispose();
    }
}

//#endregion

#[test]
fn on_did_create_get_working_copies() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let create_counter = count_events(&disposables.store(), manager.on_did_create());

        let file_uri = uri_file("/test.html");

        assert_eq!(manager.working_copies().len(), 0);
        assert!(manager.get(&file_uri).is_none());

        let file_working_copy = resolve_stored(manager, &file_uri).await;
        let untitled_file_working_copy = resolve_untitled(manager, None).await;

        assert_eq!(manager.working_copies().len(), 2);
        assert_eq!(create_counter.get(), 2);
        assert!(manager.get(&file_working_copy.resource()).unwrap().is_same(&FileWorkingCopy::Stored(file_working_copy.clone())));
        assert!(manager.get(&untitled_file_working_copy.resource()).unwrap().is_same(&FileWorkingCopy::Untitled(untitled_file_working_copy.clone())));

        let same_file_working_copy = disposables.add_rc(resolve_stored(manager, &file_uri).await);
        let same_untitled_file_working_copy = disposables.add_rc(resolve_untitled(manager, Some(existing(untitled_file_working_copy.resource()))).await);
        assert!(Rc::ptr_eq(&same_file_working_copy, &file_working_copy));
        assert!(Rc::ptr_eq(&same_untitled_file_working_copy, &untitled_file_working_copy));
        assert_eq!(manager.working_copies().len(), 2);
        assert_eq!(create_counter.get(), 2);
    });
}

#[test]
fn resolve() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let file_working_copy = disposables.add_rc(resolve_stored(manager, &uri_file("/test.html")).await);
        assert!(Rc::ptr_eq(&manager.stored.resolve(&file_working_copy.resource(), None).await.unwrap(), &file_working_copy));

        let untitled_file_working_copy = disposables.add_rc(resolve_untitled(manager, None).await);
        assert!(Rc::ptr_eq(&manager.untitled.resolve(Some(existing(untitled_file_working_copy.resource()))).await.unwrap(), &untitled_file_working_copy));
        let by_resource = manager.resolve(FileWorkingCopyResolveArg::resource(&untitled_file_working_copy.resource(), None)).await.unwrap().into_untitled().unwrap();
        assert!(Rc::ptr_eq(&by_resource, &untitled_file_working_copy));
    });
}

#[test]
fn destroy() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);

        assert_eq!(accessor.working_copy_service.working_copies().len(), 0);

        resolve_stored(manager, &uri_file("/test.html")).await;
        resolve_untitled(manager, Some(with_contents("Hello Untitled"))).await;

        assert_eq!(accessor.working_copy_service.working_copies().len(), 2);
        assert_eq!(manager.stored.working_copies().len(), 1);
        assert_eq!(manager.untitled.working_copies().len(), 1);

        manager.destroy().await.unwrap();

        assert_eq!(accessor.working_copy_service.working_copies().len(), 0);
        assert_eq!(manager.stored.working_copies().len(), 0);
        assert_eq!(manager.untitled.working_copies().len(), 0);
    });
}

#[test]
fn save_as_file_same_target_unresolved_source_unresolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");

        test_save_as_file(&disposables, &fixture, &source, &source, false, false).await;
    });
}

#[test]
fn save_as_file_same_target_different_case_unresolved_source_unresolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");
        let target = uri_file("/path/SOURCE.txt");

        test_save_as_file(&disposables, &fixture, &source, &target, false, false).await;
    });
}

#[test]
fn save_as_file_different_target_unresolved_source_unresolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");
        let target = uri_file("/path/target.txt");

        test_save_as_file(&disposables, &fixture, &source, &target, false, false).await;
    });
}

#[test]
fn save_as_file_same_target_resolved_source_unresolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");

        test_save_as_file(&disposables, &fixture, &source, &source, true, false).await;
    });
}

#[test]
fn save_as_file_same_target_different_case_resolved_source_unresolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");
        let target = uri_file("/path/SOURCE.txt");

        test_save_as_file(&disposables, &fixture, &source, &target, true, false).await;
    });
}

#[test]
fn save_as_file_different_target_resolved_source_unresolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");
        let target = uri_file("/path/target.txt");

        test_save_as_file(&disposables, &fixture, &source, &target, true, false).await;
    });
}

#[test]
fn save_as_file_same_target_unresolved_source_resolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");

        test_save_as_file(&disposables, &fixture, &source, &source, false, true).await;
    });
}

#[test]
fn save_as_file_same_target_different_case_unresolved_source_resolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");
        let target = uri_file("/path/SOURCE.txt");

        test_save_as_file(&disposables, &fixture, &source, &target, false, true).await;
    });
}

#[test]
fn save_as_file_different_target_unresolved_source_resolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");
        let target = uri_file("/path/target.txt");

        test_save_as_file(&disposables, &fixture, &source, &target, false, true).await;
    });
}

#[test]
fn save_as_file_same_target_resolved_source_resolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");

        test_save_as_file(&disposables, &fixture, &source, &source, true, true).await;
    });
}

#[test]
fn save_as_file_different_target_resolved_source_resolved_target() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let source = uri_file("/path/source.txt");
        let target = uri_file("/path/target.txt");

        test_save_as_file(&disposables, &fixture, &source, &target, true, true).await;
    });
}

#[test]
fn save_as_untitled_without_associated_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);

        let working_copy = disposables.add_rc(resolve_untitled(manager, None).await);
        working_copy.model().unwrap().update_contents("Simple Save As");

        let target = uri_file("simple/file.txt");
        accessor.file_dialog_service.set_pick_file_to_save(target.clone());

        let result = manager.save_as(&working_copy.resource(), None, None).await.unwrap().expect("o \"Salvar como\" não foi cancelado");
        assert_eq!(uri_string(&result.resource()), uri_string(&target));

        assert_eq!(result.model().unwrap().contents(), "Simple Save As");

        assert!(manager.untitled.get(&working_copy.resource()).is_none());

        result.dispose();
    });
}

#[test]
fn save_as_untitled_with_associated_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);

        let working_copy = disposables.add_rc(resolve_untitled(manager, Some(associated("/some/associated.txt"))).await);
        working_copy.model().unwrap().update_contents("Simple Save As with associated resource");

        let target = file_uri_from("/some/associated.txt");

        accessor.file_service.not_exists_set.borrow_mut().set(target.clone(), true);

        let result = manager.save_as(&working_copy.resource(), None, None).await.unwrap().expect("o \"Salvar como\" não foi cancelado");
        assert_eq!(uri_string(&result.resource()), uri_string(&target));

        assert_eq!(result.model().unwrap().contents(), "Simple Save As with associated resource");

        assert!(manager.untitled.get(&working_copy.resource()).is_none());

        result.dispose();
    });
}

#[test]
fn save_as_untitled_target_exists_and_is_resolved() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);

        let working_copy = disposables.add_rc(resolve_untitled(manager, None).await);
        working_copy.model().unwrap().update_contents("Simple Save As");

        let target = uri_file("simple/file.txt");
        let target_file_working_copy = resolve_stored(manager, &target).await;
        accessor.file_dialog_service.set_pick_file_to_save(target);

        let result = manager.save_as(&working_copy.resource(), None, None).await.unwrap().expect("o \"Salvar como\" não foi cancelado");
        assert!(Rc::ptr_eq(&result, &target_file_working_copy));

        assert_eq!(result.model().unwrap().contents(), "Simple Save As");

        assert!(manager.untitled.get(&working_copy.resource()).is_none());

        result.dispose();
    });
}
