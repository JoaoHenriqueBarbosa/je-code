//! Porte de `src/vs/workbench/contrib/customEditor/test/common/customEditorModelManager.test.ts` (suíte `CustomEditorModelManager`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O `LeakGuard` no começo de cada caso. A referência que o gerenciador
//!   devolve é um `plain_disposable` (o original é um objeto literal), então as referências que os casos não descartam não
//!   contam como vazamento, como no original.
//! - **`class TestCustomEditorModel extends mock<ICustomEditorModel>()`.** É o [`TestCustomEditorModel`]: só `resource`,
//!   `viewType` e `dispose` existem, e os outros membros lançam `TypeError` como o `mock` do original.
//! - **`async` e `timeout(0)`.** O corpo dos casos roda em `run_with_faked_timers`.
//! - **`assert.deepStrictEqual({ ... })`.** São `assert_eq!` sobre as tuplas dos campos, na ordem do objeto.
//! - **`assert.throws` e `assert.rejects` com regex.** O [`errors::catch`] e a conferência da mensagem do `Error`.
//! - **`first.object === model`.** É a igualdade do ponteiro do `Rc` ([`same_model`]).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::URI;
use je_platform::resources::uri_file;
use je_workbench::common::editor::{RevertOptions, SaveOptions};
use je_workbench::contrib::custom_editor::custom_editor::{CustomEditorModelReference, ICustomEditorModel, ICustomEditorModelManager};
use je_workbench::contrib::custom_editor::custom_editor_model_manager::CustomEditorModelManager;
use je_workbench::services::files_configuration::files_configuration_service::Readonly;

const VIEW_TYPE: &str = "test.customEditor";
const OTHER_VIEW_TYPE: &str = "test.otherCustomEditor";

fn not_a_function(member: &str) -> ! {
    errors::throw(Error::named("TypeError", format!("{member} is not a function")))
}

struct TestCustomEditorModel {
    name: String,
    resource: URI,
    view_type: String,
    dispose_count: Cell<u32>,
}

impl TestCustomEditorModel {
    fn new(name: &str, resource: &URI, view_type: &str) -> Rc<Self> {
        Rc::new(Self { name: name.to_owned(), resource: resource.clone(), view_type: view_type.to_owned(), dispose_count: Cell::new(0) })
    }

    fn dispose_count(&self) -> u32 {
        self.dispose_count.get()
    }
}

impl IDisposable for TestCustomEditorModel {
    fn dispose(&self) {
        self.dispose_count.set(self.dispose_count.get() + 1);
    }
}

impl ICustomEditorModel for TestCustomEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn view_type(&self) -> String {
        self.view_type.clone()
    }

    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn backup_id(&self) -> Option<String> {
        not_a_function("backupId")
    }

    fn can_hot_exit(&self) -> bool {
        not_a_function("canHotExit")
    }

    fn is_readonly(&self) -> Readonly {
        not_a_function("isReadonly")
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        not_a_function("onDidChangeReadonly")
    }

    fn is_orphaned(&self) -> bool {
        not_a_function("isOrphaned")
    }

    fn on_did_change_orphaned(&self) -> Event<()> {
        not_a_function("onDidChangeOrphaned")
    }

    fn is_dirty(&self) -> bool {
        not_a_function("isDirty")
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        not_a_function("onDidChangeDirty")
    }

    fn revert(&self, _options: Option<RevertOptions>) -> Promise<()> {
        not_a_function("revert")
    }

    fn save_custom_editor(&self, _options: Option<SaveOptions>) -> Promise<Option<URI>> {
        not_a_function("saveCustomEditor")
    }

    fn save_custom_editor_as(&self, _resource: &URI, _target_resource: &URI, _current_options: Option<SaveOptions>) -> Promise<bool> {
        not_a_function("saveCustomEditorAs")
    }
}

fn as_model(model: &Rc<TestCustomEditorModel>) -> Rc<dyn ICustomEditorModel> {
    model.clone()
}

fn resolved(model: &Rc<TestCustomEditorModel>) -> Promise<Rc<dyn ICustomEditorModel>> {
    Promise::resolved(as_model(model))
}

fn same_model(actual: &Rc<dyn ICustomEditorModel>, expected: &Rc<TestCustomEditorModel>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(actual), Rc::as_ptr(expected))
}

fn name_of(model: &Rc<dyn ICustomEditorModel>) -> String {
    model.as_any().downcast_ref::<TestCustomEditorModel>().expect("TestCustomEditorModel").name.clone()
}

fn names(models: &[Rc<dyn ICustomEditorModel>]) -> Vec<String> {
    models.iter().map(name_of).collect()
}

fn could_not_open() -> Error {
    Error::new("Could not open")
}

/// O `message` do erro rejeitado, depois de conferir que ele casa com `pattern` (o `assert.rejects(..., /pattern/)`).
fn rejected_message<T>(result: Result<T, Error>, pattern: &str) -> String {
    match result {
        Ok(_) => panic!("a promessa deveria ter sido rejeitada com /{pattern}/"),
        Err(error) => {
            assert!(error.message.contains(pattern), "mensagem inesperada: {}", error.message);
            error.message
        }
    }
}

fn resource() -> URI {
    uri_file("/workspace/file.custom")
}

fn other_resource() -> URI {
    uri_file("/workspace/other.custom")
}

#[test]
fn try_retain_returns_undefined_when_there_is_no_model() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let manager = CustomEditorModelManager::new();

    assert!(manager.try_retain(&resource(), VIEW_TYPE).is_none());
}

#[test]
fn shares_a_model_between_references_and_disposes_it_with_the_last_reference() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let resource = resource();
        let manager = CustomEditorModelManager::new();
        let model = TestCustomEditorModel::new("model", &resource, VIEW_TYPE);

        let first = manager.add(&resource, VIEW_TYPE, resolved(&model)).await.unwrap();
        let second = manager.try_retain(&resource, VIEW_TYPE).unwrap().await.unwrap();
        first.dispose();
        first.dispose();
        timeout(0).await.unwrap();
        let known = manager.get(&resource, VIEW_TYPE).await.unwrap();
        let after_first_disposed = (model.dispose_count(), known.map(|known| same_model(&known, &model)) == Some(true));

        second.dispose();
        timeout(0).await.unwrap();

        let same = same_model(&first.object, &model) && same_model(&second.object, &model);
        let after_last_disposed = (model.dispose_count(), manager.try_retain(&resource, VIEW_TYPE).is_none());

        assert_eq!((same, after_first_disposed, after_last_disposed), (true, (0, true), (1, true)));
    });
}

#[test]
fn add_throws_when_a_model_already_exists_for_the_resource_and_view_type() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let resource = resource();
        let manager = CustomEditorModelManager::new();
        let reference = manager.add(&resource, VIEW_TYPE, resolved(&TestCustomEditorModel::new("first", &resource, VIEW_TYPE))).await.unwrap();

        let second = resolved(&TestCustomEditorModel::new("second", &resource, VIEW_TYPE));
        let error = errors::catch(|| manager.add(&resource, VIEW_TYPE, second)).err().expect("add deveria lançar");
        assert!(error.message.contains("Model already exists"), "mensagem inesperada: {}", error.message);

        reference.dispose();
    });
}

#[test]
fn finds_models_by_resource_and_view_type() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let (resource, other_resource) = (resource(), other_resource());
        let manager = CustomEditorModelManager::new();
        let references = Promise::all(vec![
            manager.add(&resource, VIEW_TYPE, resolved(&TestCustomEditorModel::new("a", &resource, VIEW_TYPE))),
            manager.add(&resource, OTHER_VIEW_TYPE, resolved(&TestCustomEditorModel::new("b", &resource, OTHER_VIEW_TYPE))),
            manager.add(&other_resource, VIEW_TYPE, resolved(&TestCustomEditorModel::new("c", &other_resource, VIEW_TYPE))),
        ])
        .await
        .unwrap();

        let get = manager.get(&resource, OTHER_VIEW_TYPE).await.unwrap().map(|model| name_of(&model));
        let get_missing = manager.get(&other_resource, OTHER_VIEW_TYPE).await.unwrap().is_none();
        let all_for_resource = names(&manager.get_all_models(&resource).await.unwrap());
        let all_for_other_resource = names(&manager.get_all_models(&other_resource).await.unwrap());

        assert_eq!(
            (get, get_missing, all_for_resource, all_for_other_resource),
            (Some("b".to_owned()), true, vec!["a".to_owned(), "b".to_owned()], vec!["c".to_owned()])
        );

        references.iter().for_each(|reference: &CustomEditorModelReference| reference.dispose());
    });
}

#[test]
fn dispose_all_models_for_view_only_disposes_and_forgets_models_of_that_view_type() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let (resource, other_resource) = (resource(), other_resource());
        let manager = CustomEditorModelManager::new();
        let a = TestCustomEditorModel::new("a", &resource, VIEW_TYPE);
        let b = TestCustomEditorModel::new("b", &resource, OTHER_VIEW_TYPE);
        let c = TestCustomEditorModel::new("c", &other_resource, VIEW_TYPE);
        let b_reference = manager.add(&resource, OTHER_VIEW_TYPE, resolved(&b)).await.unwrap();
        manager.add(&resource, VIEW_TYPE, resolved(&a)).await.unwrap();
        manager.add(&other_resource, VIEW_TYPE, resolved(&c)).await.unwrap();

        manager.dispose_all_models_for_view(VIEW_TYPE);
        timeout(0).await.unwrap();

        let disposed = [&a, &b, &c].map(|model| model.dispose_count());
        let mut remaining = names(&manager.get_all_models(&resource).await.unwrap());
        remaining.extend(names(&manager.get_all_models(&other_resource).await.unwrap()));

        assert_eq!((disposed, remaining), ([1, 0, 1], vec!["b".to_owned()]));

        b_reference.dispose();
    });
}

#[test]
fn dispose_all_models_for_resource_only_disposes_and_forgets_models_of_that_resource() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let (resource, other_resource) = (resource(), other_resource());
        let manager = CustomEditorModelManager::new();
        let a = TestCustomEditorModel::new("a", &resource, VIEW_TYPE);
        let b = TestCustomEditorModel::new("b", &resource, OTHER_VIEW_TYPE);
        let c = TestCustomEditorModel::new("c", &other_resource, VIEW_TYPE);
        let c_reference = manager.add(&other_resource, VIEW_TYPE, resolved(&c)).await.unwrap();
        manager.add(&resource, VIEW_TYPE, resolved(&a)).await.unwrap();
        manager.add(&resource, OTHER_VIEW_TYPE, resolved(&b)).await.unwrap();

        manager.dispose_all_models_for_resource(&resource);
        timeout(0).await.unwrap();

        let disposed = [&a, &b, &c].map(|model| model.dispose_count());
        let mut remaining = names(&manager.get_all_models(&resource).await.unwrap());
        remaining.extend(names(&manager.get_all_models(&other_resource).await.unwrap()));

        assert_eq!((disposed, remaining), ([1, 1, 0], vec!["c".to_owned()]));

        c_reference.dispose();
    });
}

#[test]
fn forgets_a_model_that_fails_to_resolve_so_that_it_can_be_created_again_250622() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let resource = resource();
        let manager = CustomEditorModelManager::new();

        rejected_message(manager.add(&resource, VIEW_TYPE, Promise::rejected(could_not_open())).await, "Could not open");
        let after_failure = (manager.try_retain(&resource, VIEW_TYPE).is_none(), manager.get(&resource, VIEW_TYPE).await.unwrap().is_none());

        let model = TestCustomEditorModel::new("retry", &resource, VIEW_TYPE);
        let reference = manager.add(&resource, VIEW_TYPE, resolved(&model)).await.unwrap();

        assert_eq!((after_failure, same_model(&reference.object, &model)), ((true, true), true));

        reference.dispose();
    });
}

#[test]
fn reports_the_failure_of_a_pending_model_to_everyone_waiting_for_it_250622() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let resource = resource();
        let manager = CustomEditorModelManager::new();
        let pending: DeferredPromise<Rc<dyn ICustomEditorModel>> = DeferredPromise::new();

        let waiting = vec![manager.add(&resource, VIEW_TYPE, pending.promise.clone()), manager.try_retain(&resource, VIEW_TYPE).unwrap()];
        let _ = pending.error(could_not_open());
        let results = Promise::all_settled(waiting).await.unwrap();

        let messages: Vec<String> = results
            .into_iter()
            .map(|result| match result {
                Err(error) => error.message,
                Ok(_) => "fulfilled".to_owned(),
            })
            .collect();

        assert_eq!((messages, manager.try_retain(&resource, VIEW_TYPE).is_none()), (vec!["Could not open".to_owned(), "Could not open".to_owned()], true));
    });
}

#[test]
fn a_model_that_fails_after_it_was_disposed_does_not_forget_its_replacement() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let resource = resource();
        let manager = CustomEditorModelManager::new();
        let pending: DeferredPromise<Rc<dyn ICustomEditorModel>> = DeferredPromise::new();
        let failed = manager.add(&resource, VIEW_TYPE, pending.promise.clone());

        manager.dispose_all_models_for_resource(&resource);
        let replacement = TestCustomEditorModel::new("replacement", &resource, VIEW_TYPE);
        let reference = manager.add(&resource, VIEW_TYPE, resolved(&replacement)).await.unwrap();
        let _ = pending.error(could_not_open());
        rejected_message(failed.await, "Could not open");
        timeout(0).await.unwrap();

        let model = manager.get(&resource, VIEW_TYPE).await.unwrap().map(|model| same_model(&model, &replacement)) == Some(true);
        assert_eq!((model, replacement.dispose_count()), (true, 0));

        reference.dispose();
    });
}

#[test]
fn get_all_models_skips_models_that_fail_to_resolve() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let resource = resource();
        let manager = CustomEditorModelManager::new();
        let reference = manager.add(&resource, VIEW_TYPE, resolved(&TestCustomEditorModel::new("a", &resource, VIEW_TYPE))).await.unwrap();
        let pending: DeferredPromise<Rc<dyn ICustomEditorModel>> = DeferredPromise::new();
        let failed = manager.add(&resource, OTHER_VIEW_TYPE, pending.promise.clone());

        let all = manager.get_all_models(&resource);
        let _ = pending.error(could_not_open());
        rejected_message(failed.await, "Could not open");

        assert_eq!(names(&all.await.unwrap()), vec!["a".to_owned()]);

        reference.dispose();
    });
}

#[test]
fn releasing_a_reference_to_a_model_disposed_with_its_resource_leaves_the_new_model_alone() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let resource = resource();
        let manager = CustomEditorModelManager::new();
        let deleted = TestCustomEditorModel::new("deleted", &resource, VIEW_TYPE);
        let stale_reference = manager.add(&resource, VIEW_TYPE, resolved(&deleted)).await.unwrap();

        manager.dispose_all_models_for_resource(&resource);
        let recreated = TestCustomEditorModel::new("recreated", &resource, VIEW_TYPE);
        let reference = manager.add(&resource, VIEW_TYPE, resolved(&recreated)).await.unwrap();
        stale_reference.dispose();
        timeout(0).await.unwrap();

        let model = manager.get(&resource, VIEW_TYPE).await.unwrap().map(|model| name_of(&model));
        assert_eq!((model, [deleted.dispose_count(), recreated.dispose_count()]), (Some("recreated".to_owned()), [1, 0]));

        reference.dispose();
    });
}
