//! Porte de `src/vs/workbench/services/textfile/test/browser/textFileEditorModelManager.test.ts` (suíte `Files - TextFileEditorModelManager`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois. O `teardown`
//!   (`disposables.clear()`) é o descarte do store do guarda, que roda quando o guarda sai de escopo e antes de ele conferir os
//!   vazamentos. Os casos `async` rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService`, `TestServiceAccessor` e `TestTextFileService`.** Estão em
//!   `tests/support/text_file_support.rs` (o `TestTextFileService` é montado sobre o `AbstractTextFileService` real).
//!   `accessor.textFileService.files as ITestTextFileEditorModelManager` é [`TextFileTestAccessor::files`] (o gerenciador
//!   concreto, que tem `add`).
//! - **`disposables.add(toDisposable(() => accessor.textFileService.files as ...))`.** O descartável do `setup` não faz nada
//!   (a função só devolve o gerenciador); o porte o mantém como um descartável vazio que segura o gerenciador.
//! - **`instantiationService.createInstance(TextFileEditorModel, resource, 'utf8', undefined)`** é
//!   [`TextFileTestAccessor::create_model`].
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título completo do caso
//!   (a suíte mais o título do caso) é o argumento de [`to_resource`].
//! - **`manager.resolve(resource, options)`** é o `resolve` do contrato `ITextFileEditorModelManager` (devolve o modelo como
//!   `ITextFileEditorModel`); onde o original converte para `TextFileEditorModel` (`as TextFileEditorModel`) o caso usa o
//!   `resolve_model` do gerenciador, que devolve o modelo concreto. O `instanceof TextFileEditorModel` é o `downcast_ref` do
//!   `as_any` do modelo.
//! - **`createTextBufferFactory('...')`** é [`text_buffer_factory`]; `{ encoding }`, `{ languageId }`, `{ contents }` e
//!   `{ reload: { async } }` são [`with_encoding`], [`with_language`], [`with_contents`] e [`with_reload`].
//! - **`new Promise<void>(resolve => disposables.add(manager.onDidResolve(...)))`.** [`on_did_resolve_of`] cria o ouvinte e a
//!   promessa; a bandeira `didResolve` é um `Rc<Cell<bool>>`.
//! - **`canDispose`** devolve `Option<Promise<()>>` (`None` é o `true` imediato): `assert.ok(x instanceof Promise)` é
//!   `is_some()` e `assert.strictEqual(x, true)` é `is_none()`. O `(async () => { canDispose = await canDisposePromise; })()` é
//!   um `async_fn` disparado sem esperar.
//! - **`model1 === model2`** é a igualdade de endereço dos dois `Rc` ([`same_model`]).
//! - **`try/catch` do `await manager.resolve(...)`** é o `Result` da promessa: o erro vira `Some(error)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::{LanguageExtensionPoint, PLAINTEXT_LANGUAGE_ID};
use je_editor::services_model::IModelService;
use je_editor::text_model::create_text_buffer_factory;
use je_platform::files::files::{FileChange, FileChangeType, FileChangesEvent, FileOperationError, FileOperationResult};
use je_platform::resources::{host_environment, uri_file};
use je_workbench::common::editor::RevertOptions;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::services::textfile::text_file_editor_model::TextFileEditorModel;
use je_workbench::services::textfile::text_file_editor_model_manager::TextFileEditorModelManager;
use je_workbench::services::textfile::textfiles::{
    ITextFileEditorModel, ITextFileEditorModelManager, TextBufferFactoryRef, TextFileEditorModelResolveOrCreateOptions, TextFileReloadOptions, TextFileResolveOptions,
};
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use je_workbench::test::common::workbench_test_services::TestFileService;
use support::text_file_support::TextFileTestAccessor;

//#region Utilitários dos casos

/// O prefixo do `this.test.fullTitle()` do mocha: o título da suíte.
const SUITE_TITLE: &str = "Files - TextFileEditorModelManager";

/// O `setup`: `workbenchInstantiationService(undefined, disposables)`, o `TestServiceAccessor` e o descartável vazio.
fn setup(disposables: &LeakGuard) -> TextFileTestAccessor {
    let accessor = TextFileTestAccessor::new(&disposables.store());

    let files = accessor.files();
    disposables.add(to_disposable(move || {
        let _manager: &Rc<TextFileEditorModelManager> = &files;
    }));

    accessor
}

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();
    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `createTextBufferFactory(text)`.
fn text_buffer_factory(text: &str) -> TextBufferFactoryRef {
    let units: Vec<u16> = text.encode_utf16().collect();
    TextBufferFactoryRef::new(create_text_buffer_factory(&units))
}

/// `{ encoding }`.
fn with_encoding(encoding: &str) -> Option<TextFileEditorModelResolveOrCreateOptions> {
    Some(TextFileEditorModelResolveOrCreateOptions { encoding: Some(encoding.to_string()), ..TextFileEditorModelResolveOrCreateOptions::default() })
}

/// `{ languageId }`.
fn with_language(language_id: &str) -> Option<TextFileEditorModelResolveOrCreateOptions> {
    Some(TextFileEditorModelResolveOrCreateOptions { language_id: Some(language_id.to_string()), ..TextFileEditorModelResolveOrCreateOptions::default() })
}

/// `{ contents: createTextBufferFactory(text) }`.
fn with_contents(text: &str) -> Option<TextFileEditorModelResolveOrCreateOptions> {
    Some(TextFileEditorModelResolveOrCreateOptions {
        resolve: TextFileResolveOptions { contents: Some(text_buffer_factory(text)), ..TextFileResolveOptions::default() },
        ..TextFileEditorModelResolveOrCreateOptions::default()
    })
}

/// `{ reload: { async } }`.
fn with_reload(async_: bool) -> Option<TextFileEditorModelResolveOrCreateOptions> {
    Some(TextFileEditorModelResolveOrCreateOptions { reload: Some(TextFileReloadOptions { async_ }), ..TextFileEditorModelResolveOrCreateOptions::default() })
}

/// `{ soft: true }` das opções de reverter.
fn soft_revert() -> Option<RevertOptions> {
    Some(RevertOptions { soft: Some(true), ..RevertOptions::default() })
}

/// `new FileOperationError('fail', FileOperationResult.FILE_OTHER_ERROR)`.
fn fail_error() -> FileOperationError {
    FileOperationError::new("fail", FileOperationResult::FileOtherError)
}

/// `a === b` para os modelos (o endereço do objeto, sem o `vtable`).
fn same_model<A: ?Sized, B: ?Sized>(a: &Rc<A>, b: &Rc<B>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `accessor.fileService.fireFileChanges(new FileChangesEvent([{ resource, type }], false))`.
fn fire_file_change(file_service: &TestFileService, resource: &URI, change_type: FileChangeType) {
    file_service.fire_file_changes(Rc::new(FileChangesEvent::new(&[FileChange::new(change_type, resource.clone())], false)));
}

/// `new Promise<void>(resolve => disposables.add(manager.onDidResolve(({ model }) => { if (model.resource.toString() ===
/// resource.toString()) { didResolve = true; resolve(); } })))`: devolve a bandeira `didResolve` e a promessa, que cumpre
/// quando o recurso foi resolvido `resolves_needed` vezes.
fn on_did_resolve_of(disposables: &LeakGuard, manager: &TextFileEditorModelManager, resource: &URI, resolves_needed: usize) -> (Rc<Cell<bool>>, Promise<()>) {
    let did_resolve = Rc::new(Cell::new(false));
    let resolved_counter = Rc::new(Cell::new(0));
    let (promise, resolver) = Promise::<()>::new();

    let (flag, resource) = (did_resolve.clone(), uri_string(resource));
    disposables.add(manager.on_did_resolve().listen(move |event| {
        if uri_string(&event.model.resource()) == resource {
            resolved_counter.set(resolved_counter.get() + 1);
            if resolved_counter.get() >= resolves_needed {
                flag.set(true);
                resolver.resolve(());
            }
        }
    }));

    (did_resolve, promise)
}

/// Conta os disparos de `event` que `matches` aceita (o `if (...) counter++` dos ouvintes do original).
fn count_matching<T: 'static>(disposables: &LeakGuard, event: Event<T>, matches: impl Fn(&T) -> bool + 'static) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    let listener_counter = counter.clone();
    disposables.add(event.listen(move |value| {
        if matches(value) {
            listener_counter.set(listener_counter.get() + 1);
        }
    }));

    counter
}

//#endregion

#[test]
fn add_remove_clear_get_get_all() {
    const TITLE: &str = "add, remove, clear, get, getAll";
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    let manager = accessor.files();

    let model1 = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/random1.txt"), Some("utf8"), None));
    let model2 = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/random2.txt"), Some("utf8"), None));
    let model3 = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/random3.txt"), Some("utf8"), None));

    manager.add(&uri_file("/test.html"), &model1);
    manager.add(&uri_file("/some/other.html"), &model2);
    manager.add(&uri_file("/some/this.txt"), &model3);

    let file_upper = uri_file("/TEST.html");

    assert!(manager.get(&uri_file("foo")).is_none());
    assert!(same_model(&manager.get(&uri_file("/test.html")).unwrap(), &model1));

    assert!(manager.get(&file_upper).is_none());

    let results = manager.models();
    assert_eq!(3, results.len());

    let result = manager.get(&uri_file("/yes"));
    assert!(result.is_none());

    let result = manager.get(&uri_file("/some/other.txt"));
    assert!(result.is_none());

    let result = manager.get(&uri_file("/some/other.html"));
    assert!(result.is_some());

    let result = manager.get(&file_upper);
    assert!(result.is_none());

    manager.remove(&uri_file(""));

    let results = manager.models();
    assert_eq!(3, results.len());

    manager.remove(&uri_file("/some/other.html"));
    let results = manager.models();
    assert_eq!(2, results.len());

    manager.remove(&file_upper);
    let results = manager.models();
    assert_eq!(2, results.len());

    manager.dispose();
    let results = manager.models();
    assert_eq!(0, results.len());
}

#[test]
fn resolve() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/test.html");
        let encoding = "utf8";

        let events: Rc<RefCell<Vec<Rc<dyn ITextFileEditorModel>>>> = Rc::default();
        let created = events.clone();
        disposables.add(manager.on_did_create().listen(move |model| created.borrow_mut().push(model.clone())));

        let model_promise = manager.resolve(&resource, with_encoding(encoding));
        assert!(manager.get(&resource).is_some()); // model known even before resolved()

        let model1 = model_promise.await.unwrap();
        assert_eq!(model1.get_encoding().as_deref(), Some(encoding));
        assert!(same_model(&manager.get(&resource).unwrap(), &model1));

        let model2 = manager.resolve(&resource, with_encoding(encoding)).await.unwrap();
        assert!(same_model(&model2, &model1));
        model1.dispose();

        let model3 = manager.resolve(&resource, with_encoding(encoding)).await.unwrap();
        assert!(!same_model(&model3, &model2));
        assert!(same_model(&manager.get(&resource).unwrap(), &model3));
        model3.dispose();

        assert_eq!(events.borrow().len(), 2);
        assert_eq!(uri_string(&events.borrow()[0].resource()), uri_string(&model1.resource()));
        assert_eq!(uri_string(&events.borrow()[1].resource()), uri_string(&model2.resource()));
    });
}

#[test]
fn resolve_async() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/path/index.txt");

        disposables.add(manager.resolve(&resource, None).await.unwrap());

        let (did_resolve, on_did_resolve) = on_did_resolve_of(&disposables, &manager, &resource, 1);

        drop(manager.resolve(&resource, with_reload(true)));

        on_did_resolve.await.unwrap();

        assert!(did_resolve.get());
    });
}

#[test]
fn resolve_sync() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/path/index.txt");

        disposables.add(manager.resolve(&resource, None).await.unwrap());

        let did_resolve = Rc::new(Cell::new(false));
        let (flag, resource_key) = (did_resolve.clone(), uri_string(&resource));
        disposables.add(manager.on_did_resolve().listen(move |event| {
            if uri_string(&event.model.resource()) == resource_key {
                flag.set(true);
            }
        }));

        manager.resolve(&resource, with_reload(false)).await.unwrap();
        assert!(did_resolve.get());
    });
}

#[test]
fn resolve_sync_model_disposed_when_error_and_first_call_to_resolve() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/path/index.txt");

        accessor.text_file_service.set_read_stream_error_once(fail_error());

        let mut error: Option<Error> = None;
        match manager.resolve(&resource, None).await {
            Ok(model) => {
                disposables.add(model);
            }
            Err(e) => error = Some(e),
        }

        assert!(error.is_some());
        assert_eq!(manager.models().len(), 0);
    });
}

#[test]
fn resolve_sync_model_not_disposed_when_error_and_model_existed_before() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/path/index.txt");

        disposables.add(manager.resolve(&resource, None).await.unwrap());

        accessor.text_file_service.set_read_stream_error_once(fail_error());

        let mut error: Option<Error> = None;
        match manager.resolve(&resource, with_reload(false)).await {
            Ok(model) => {
                disposables.add(model);
            }
            Err(e) => error = Some(e),
        }

        assert!(error.is_some());
        assert_eq!(manager.models().len(), 1);
    });
}

#[test]
fn resolve_with_initial_contents() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/test.html");

        let model = manager.resolve(&resource, with_contents("Hello World")).await.unwrap();
        disposables.add(model.clone());
        assert_eq!(model.text_editor_model().map(|text_model| text_model.get_value().to_string_lossy()).as_deref(), Some("Hello World"));
        assert!(model.is_dirty());

        disposables.add(manager.resolve(&resource, with_contents("More Changes")).await.unwrap());
        assert_eq!(model.text_editor_model().map(|text_model| text_model.get_value().to_string_lossy()).as_deref(), Some("More Changes"));
        assert!(model.is_dirty());
    });
}

#[test]
fn multiple_resolves_execute_in_sequence() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/test.html");

        let resolved_model: Rc<RefCell<Option<Rc<dyn ITextFileEditorModel>>>> = Rc::default();

        let contents: Rc<RefCell<Vec<String>>> = Rc::default();
        let (store, resolved_slot, collected, resource_key) = (disposables.store(), resolved_model.clone(), contents.clone(), uri_string(&resource));
        disposables.add(manager.on_did_resolve().listen(move |event| {
            if uri_string(&event.model.resource()) == resource_key {
                store.add(event.model.clone());
                *resolved_slot.borrow_mut() = Some(event.model.clone());
                collected.borrow_mut().push(event.model.text_editor_model().unwrap().get_value().to_string_lossy());
            }
        }));

        Promise::all(vec![
            manager.resolve(&resource, None),
            manager.resolve(&resource, with_contents("Hello World")),
            manager.resolve(&resource, with_reload(false)),
            manager.resolve(&resource, with_contents("More Changes")),
        ])
        .await
        .unwrap();

        let resolved_model = resolved_model.borrow().clone().expect("o modelo foi resolvido");
        assert!(resolved_model.as_any().downcast_ref::<TextFileEditorModel>().is_some());

        assert_eq!(resolved_model.text_editor_model().map(|text_model| text_model.get_value().to_string_lossy()).as_deref(), Some("More Changes"));
        assert!(resolved_model.is_dirty());

        let contents = contents.borrow();
        assert_eq!(contents[0], "Hello Html");
        assert_eq!(contents[1], "Hello World");
        assert_eq!(contents[2], "More Changes");
    });
}

#[test]
fn removed_from_cache_when_model_disposed() {
    const TITLE: &str = "removed from cache when model disposed";
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);
    let manager = accessor.files();

    let model1 = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/random1.txt"), Some("utf8"), None));
    let model2 = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/random2.txt"), Some("utf8"), None));
    let model3 = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/random3.txt"), Some("utf8"), None));

    manager.add(&uri_file("/test.html"), &model1);
    manager.add(&uri_file("/some/other.html"), &model2);
    manager.add(&uri_file("/some/this.txt"), &model3);

    assert!(same_model(&manager.get(&uri_file("/test.html")).unwrap(), &model1));

    model1.dispose();
    assert!(manager.get(&uri_file("/test.html")).is_none());
}

#[test]
fn events() {
    const TITLE: &str = "events";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();

        let resource1 = to_resource(TITLE, "/path/index.txt");
        let resource2 = to_resource(TITLE, "/path/other.txt");

        let (key1, key2) = (uri_string(&resource1), uri_string(&resource2));

        let resolved_counter = count_matching(&disposables, manager.on_did_resolve(), {
            let key1 = key1.clone();
            move |event| uri_string(&event.model.resource()) == key1
        });

        let removed_counter = count_matching(&disposables, manager.on_did_remove(), {
            let (key1, key2) = (key1.clone(), key2.clone());
            move |resource| uri_string(resource) == key1 || uri_string(resource) == key2
        });

        let got_dirty_counter = count_matching(&disposables, manager.on_did_change_dirty(), {
            let key1 = key1.clone();
            move |model| uri_string(&model.resource()) == key1 && model.is_dirty()
        });

        let got_non_dirty_counter = count_matching(&disposables, manager.on_did_change_dirty(), {
            let key1 = key1.clone();
            move |model| uri_string(&model.resource()) == key1 && !model.is_dirty()
        });

        let reverted_counter = count_matching(&disposables, manager.on_did_revert(), {
            let key1 = key1.clone();
            move |model| uri_string(&model.resource()) == key1
        });

        let saved_counter = count_matching(&disposables, manager.on_did_save(), {
            let key1 = key1.clone();
            move |event| uri_string(&event.model.resource()) == key1
        });

        let encoding_counter = count_matching(&disposables, manager.on_did_change_encoding(), {
            let key1 = key1.clone();
            move |model| uri_string(&model.resource()) == key1
        });

        let model1 = manager.resolve_model(&resource1, with_encoding("utf8")).await.unwrap();
        assert_eq!(resolved_counter.get(), 1);

        fire_file_change(&accessor.file_service, &resource1, FileChangeType::Deleted);
        fire_file_change(&accessor.file_service, &resource1, FileChangeType::Added);

        let model2 = manager.resolve_model(&resource2, with_encoding("utf8")).await.unwrap();
        assert_eq!(resolved_counter.get(), 2);

        model1.update_text_editor_model(Some(&text_buffer_factory("changed")), None, None);
        model1.update_preferred_encoding(Some("utf16"));

        model1.revert(None).await.unwrap();
        model1.update_text_editor_model(Some(&text_buffer_factory("changed again")), None, None);

        model1.save(None).await.unwrap();
        model1.dispose();
        model2.dispose();

        model1.revert(None).await.unwrap();
        assert_eq!(removed_counter.get(), 2);
        assert_eq!(got_dirty_counter.get(), 2);
        assert_eq!(got_non_dirty_counter.get(), 2);
        assert_eq!(reverted_counter.get(), 1);
        assert_eq!(saved_counter.get(), 1);
        assert_eq!(encoding_counter.get(), 2);

        model1.dispose();
        model2.dispose();
        assert!(accessor.model_service.get_model(&resource1).is_none());
        assert!(accessor.model_service.get_model(&resource2).is_none());
    });
}

#[test]
fn disposing_model_takes_it_out_of_the_manager() {
    const TITLE: &str = "disposing model takes it out of the manager";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();

        let resource = to_resource(TITLE, "/path/index_something.txt");

        let model = manager.resolve(&resource, with_encoding("utf8")).await.unwrap();
        model.dispose();
        assert!(manager.get(&resource).is_none());
        assert!(accessor.model_service.get_model(&model.resource()).is_none());
    });
}

#[test]
fn can_dispose_with_dirty_model() {
    const TITLE: &str = "canDispose with dirty model";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();

        let resource = to_resource(TITLE, "/path/index_something.txt");

        let model = disposables.add_rc(manager.resolve_model(&resource, with_encoding("utf8")).await.unwrap());
        model.update_text_editor_model(Some(&text_buffer_factory("make dirty")), None, None);

        let can_dispose_promise = manager.can_dispose(model.clone());
        assert!(can_dispose_promise.is_some());

        let can_dispose = Rc::new(Cell::new(false));
        let flag = can_dispose.clone();
        let pending = can_dispose_promise.expect("o modelo sujo devolve uma promessa");
        drop(async_fn(async move {
            pending.await?;
            flag.set(true);
            Ok(())
        }));

        assert!(!can_dispose.get());
        drop(model.revert(soft_revert()));

        timeout(0).await.unwrap();

        assert!(can_dispose.get());

        let can_dispose2 = manager.can_dispose(model.clone());
        assert!(can_dispose2.is_none());
    });
}

#[test]
fn language() {
    const TITLE: &str = "language";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let language_id = "text-file-model-manager-test";
        disposables.add(accessor.language_service.register_language(LanguageExtensionPoint::new(language_id)));

        let manager = accessor.files();

        let resource = to_resource(TITLE, "/path/index_something.txt");

        let model = manager.resolve(&resource, with_language(language_id)).await.unwrap();
        disposables.add(model.clone());
        assert_eq!(model.text_editor_model().unwrap().get_language_id(), language_id);

        let model = manager.resolve(&resource, with_language("text")).await.unwrap();
        assert_eq!(model.text_editor_model().unwrap().get_language_id(), PLAINTEXT_LANGUAGE_ID);
    });
}

#[test]
fn file_change_events_trigger_reload_on_a_resolved_model() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/path/index.txt");

        disposables.add(manager.resolve(&resource, None).await.unwrap());

        let (did_resolve, on_did_resolve) = on_did_resolve_of(&disposables, &manager, &resource, 1);

        fire_file_change(&accessor.file_service, &resource, FileChangeType::Updated);

        on_did_resolve.await.unwrap();
        assert!(did_resolve.get());
    });
}

#[test]
fn file_change_events_trigger_reload_after_a_model_is_resolved() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let manager = accessor.files();
        let resource = uri_file("/path/index.txt");

        drop(manager.resolve(&resource, None));

        let did_resolve = Rc::new(Cell::new(false));
        let resolved_counter = Rc::new(Cell::new(0));
        let (promise, resolver) = Promise::<()>::new();
        let (store, flag, counter, resource_key) = (disposables.store(), did_resolve.clone(), resolved_counter.clone(), uri_string(&resource));
        disposables.add(manager.on_did_resolve().listen(move |event| {
            store.add(event.model.clone());
            if uri_string(&event.model.resource()) == resource_key {
                counter.set(counter.get() + 1);
                if counter.get() == 2 {
                    flag.set(true);
                    resolver.resolve(());
                }
            }
        }));

        fire_file_change(&accessor.file_service, &resource, FileChangeType::Updated);

        promise.await.unwrap();
        assert!(did_resolve.get());
    });
}
