//! Porte de `src/vs/workbench/services/textmodelResolver/test/browser/textModelResolverService.test.ts` (suíte `Workbench - TextModelResolverService`), caso a caso e na mesma ordem.
//!
//! Casos de fora (sete `#[test]` expandidos, quatro `test(...)` do original), todos porque dependem do
//! `TestDiffProviderFactoryService` (`editor/test/browser/diff/testDiffProviderFactoryService.ts`), que ainda não existe no porte
//! (o `MultiDiffEditorViewModel`, o `DiffItemSource` e o `RefCounted` já existem); sem ele o caso não pode ser fiel:
//! - `multi-diff synthetic side shares its lifetime with document references (added: false)` e `(added: true)`;
//! - `multi-diff releases pending synthetic sides (dispose parent: false, side: 1)`, `(false, 2)`, `(true, 1)` e `(true, 2)`;
//! - `multi-diff releases a partially resolved item when remaining resolution fails after disposal`.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada
//!   caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois: o
//!   `workbenchInstantiationService`, o `TestServiceAccessor` e o `disposables.add(accessor.textFileService.files)`. O
//!   `teardown` (`disposables.clear()`) roda quando o guarda sai de escopo, antes de ele conferir os vazamentos. Os casos
//!   rodam em tempo real (`run_with_real_timers`, o mocha não usa relógio virtual aqui) e `await` é `.await.unwrap()`.
//! - **`accessor.textModelResolverService`.** O `workbenchInstantiationService` do original registra o
//!   `TextModelResolverService` como `ITextModelService`; aqui o [`setup`] o cria por `createInstance` sobre o acessor de
//!   `tests/support/text_file_support.rs` e o troca no lugar do `TestTextModelService` dele (junto com o
//!   `IUriIdentityService` que o serviço injeta, que o acessor não registra).
//! - **`IReference` e `disposables.add(reference)`.** A referência é o `Reference` de `je_base::lifecycle`, que se clona
//!   dividindo o descarte de chamada única; `disposables.add(ref)` é registrar uma cópia no guarda ([`track`]).
//! - **`ITextModelContentProvider` literal.** O objeto `{ provideTextContent }` é o [`ContentProvider`], que guarda o fecho.
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título do caso (a suíte
//!   mais o título do caso) é o argumento de [`to_resource`].
//! - **`model.textEditorModel` de `ref.object`.** É `ITextEditorModel::text_editor_model` (um `Option`, aqui sempre `Some`
//!   porque a referência é de um modelo resolvido), e o `getLanguageId()` do `ITextEditorModel`, que o trait do porte não
//!   tem, é o método do `TextResourceEditorModel` lido por [`IEditorModel::as_any`].
//! - **`Event.once(model.onWillDispose)(...)` e `Event.toPromise`.** São `Event::once().listen` e `Event::to_promise(None)`;
//!   `new Promise(resolve => disposables.add(model.onWillDispose(resolve)))` é a promessa de [`will_dispose`].
//! - **`assert.strictEqual(a, b)` entre modelos** é a identidade do objeto ([`same_model`]); `assert.rejects(p, /regex/)` é
//!   a conferência de que a promessa rejeita com a mensagem (`error.to_string().contains(...)`).
//! - **`textModel.revert()` sem `await`** (os casos de arquivo sujo) é `let _ = model.revert(None)`: a promessa corre sozinha.
//! - **`mock<ILanguageDetectionService>()` que lança no construtor** é o [`ThrowingLanguageDetectionService`], um serviço
//!   cuja construção lança (`errors::throw`), registrado por `SyncDescriptor` no `createChild`; os membros do trait nunca
//!   são chamados porque a construção falha antes.
//! - **`for (const ...of [false, true])`.** Cada valor do laço é um `#[test]` que chama o mesmo corpo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::path::PathModule;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::{URI, UriComponents};
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::text_model::{TextModel, TextSource, create_text_buffer_factory};
use je_instantiation::{Injectable, ServiceCollection, SyncDescriptor, service_impl};
use je_platform::resources::{host_environment, uri_file};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::text_resource_editor_input::TextResourceEditorInput;
use je_workbench::common::editor::text_resource_editor_model::TextResourceEditorModel;
use je_workbench::services::environment::environment_service::{IWorkbenchEnvironmentService, I_WORKBENCH_ENVIRONMENT_SERVICE};
use je_workbench::services::language_detection::language_detection_worker_service::{ILanguageDetectionService, I_LANGUAGE_DETECTION_SERVICE};
use je_workbench::services::textfile::text_file_editor_model::TextFileEditorModel;
use je_workbench::services::textfile::textfiles::{TextBufferFactoryRef, snapshot_to_string};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelContentProvider, ITextModelService, TextModelReference, I_TEXT_MODEL_SERVICE};
use je_workbench::services::textmodel_resolver::text_model_resolver_service::TextModelResolverService;
use je_workbench::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use je_workbench::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use support::text_file_support::TextFileTestAccessor;

//#region Utilitários dos casos

/// O título da suíte, o prefixo do `this.test.fullTitle()` do mocha.
const SUITE_TITLE: &str = "Workbench - TextModelResolverService";

/// O que o `setup` do original monta: o `accessor` e o `TextModelResolverService` dele (`accessor.textModelResolverService`).
struct Fixture {
    accessor: TextFileTestAccessor,
    resolver: Rc<TextModelResolverService>,
}

/// `setup`: `workbenchInstantiationService(undefined, disposables)`, o `TestServiceAccessor` e
/// `disposables.add(<TextFileEditorModelManager>accessor.textFileService.files)`.
fn setup(disposables: &LeakGuard) -> Fixture {
    let accessor = TextFileTestAccessor::new(&disposables.store());
    disposables.add_rc(accessor.files());

    // `instantiationService.stub(ITextModelService, disposables.add(instantiationService.createInstance(TextModelResolverService)))`
    let uri_identity_service: Rc<dyn IUriIdentityService> = accessor.uri_identity_service.clone();
    accessor.collection.set(I_URI_IDENTITY_SERVICE, uri_identity_service);
    // O `workbenchInstantiationService` registra o `IWorkbenchEnvironmentService` (o `UntitledTextEditorInput` o injeta).
    accessor.collection.set(I_WORKBENCH_ENVIRONMENT_SERVICE, TestEnvironmentService::new() as Rc<dyn IWorkbenchEnvironmentService>);
    let resolver = disposables.add_rc(accessor.instantiation_service.create_instance::<TextModelResolverService>(()));
    let text_model_service: Rc<dyn ITextModelService> = resolver.clone();
    accessor.collection.set(I_TEXT_MODEL_SERVICE, text_model_service);

    Fixture { accessor, resolver }
}

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();
    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `URI.from({ scheme, authority: null!, path })`.
fn uri_of(scheme: &str, path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(scheme)), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

/// O `ITextModelContentProvider` literal `{ provideTextContent: ... }`.
struct ContentProvider<F: Fn(&URI) -> MaybePromise<Option<Rc<TextModel>>>>(F);

impl<F: Fn(&URI) -> MaybePromise<Option<Rc<TextModel>>>> ITextModelContentProvider for ContentProvider<F> {
    fn provide_text_content(&self, resource: &URI) -> MaybePromise<Option<Rc<TextModel>>> {
        (self.0)(resource)
    }
}

/// `disposables.add(accessor.textModelResolverService.registerTextModelContentProvider(scheme, { provideTextContent }))`.
fn register_provider(
    disposables: &LeakGuard,
    fixture: &Fixture,
    scheme: &str,
    provide_text_content: impl Fn(&URI) -> MaybePromise<Option<Rc<TextModel>>> + 'static,
) {
    disposables.add(fixture.resolver.register_text_model_content_provider(scheme, Rc::new(ContentProvider(provide_text_content))));
}

/// `accessor.modelService.createModel(content, accessor.languageService.createById('json'), resource)`.
fn create_json_model(fixture: &Fixture, content: &str, resource: &URI) -> Rc<TextModel> {
    let language_selection = fixture.accessor.language_service.create_by_id(Some("json"));
    fixture.accessor.model_service.create_model(content, Some(language_selection.as_ref()), Some(resource), false)
}

/// `createTextBufferFactory(text)`.
fn text_buffer_factory(text: &str) -> TextBufferFactoryRef {
    let units: Vec<u16> = text.encode_utf16().collect();
    TextBufferFactoryRef::new(create_text_buffer_factory(&units))
}

/// `model.updateTextEditorModel(createTextBufferFactory(text))`.
fn update_model(model: &TextFileEditorModel, text: &str) {
    model.update_text_editor_model(Some(&text_buffer_factory(text)), None, None);
}

/// `disposables.add(reference)`: uma cópia da referência no guarda (o descarte é de chamada única e dividido).
fn track(disposables: &LeakGuard, reference: &TextModelReference) {
    disposables.add(Rc::new(reference.clone()));
}

/// `reference.object.textEditorModel`.
fn text_model_of(reference: &TextModelReference) -> Rc<TextModel> {
    reference.object.text_editor_model().expect("o modelo de texto existe")
}

/// `a === b` para os modelos (o endereço do objeto, sem o `vtable`).
fn same_model<A: ?Sized, B: ?Sized>(a: &Rc<A>, b: &Rc<B>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `let flag = false; Event.once(event)(() => flag = true)`.
fn once_flag<T: 'static>(event: Event<T>) -> Rc<Cell<bool>> {
    let flag = Rc::new(Cell::new(false));
    let listener_flag = flag.clone();
    event.once().listen(move |_| listener_flag.set(true));

    flag
}

/// `new Promise<void>(resolve => disposables.add(event(resolve)))`.
fn will_dispose(disposables: &LeakGuard, text_model: &TextModel) -> Promise<()> {
    let (promise, resolver) = Promise::new();
    disposables.add(text_model.on_will_dispose().listen(move |_| resolver.resolve(())));

    promise
}

/// `disposables.add(model.onWillDispose(() => disposalCount++))`.
fn count_will_dispose(disposables: &LeakGuard, text_model: &TextModel) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    let listener_counter = counter.clone();
    disposables.add(text_model.on_will_dispose().listen(move |_| listener_counter.set(listener_counter.get() + 1)));

    counter
}

/// `accessor.textModelResolverService.createSyntheticDocument(value, languageSelection)`.
async fn create_synthetic_document(fixture: &Fixture, value: TextSource, language_selection: Option<Rc<dyn je_editor::language_service::ILanguageSelection>>) -> TextModelReference {
    fixture.resolver.create_synthetic_document(value, language_selection).await.unwrap()
}

/// O `ILanguageDetectionService` do teste `failed synthetic wrapper construction releases the text model`: o construtor
/// lança, então nenhum membro chega a ser chamado.
struct ThrowingLanguageDetectionService;

impl Injectable for ThrowingLanguageDetectionService {
    type Args = ();
    type Deps = ();

    fn create(_: Self::Args, _: Self::Deps) -> Rc<Self> {
        errors::throw(Error::new("Wrapper construction failed"))
    }
}

service_impl!(ThrowingLanguageDetectionService => dyn ILanguageDetectionService);

impl ILanguageDetectionService for ThrowingLanguageDetectionService {
    fn is_enabled_for_language(&self, _language_id: &str) -> bool {
        false
    }

    fn detect_language(&self, _resource: &URI, _supported_langs: Option<&[String]>) -> Promise<Option<String>> {
        Promise::resolved(None)
    }
}

//#endregion

#[test]
fn resolve_resource() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let (model_service, language_service) = (fixture.accessor.model_service.clone(), fixture.accessor.language_service.clone());
        register_provider(&disposables, &fixture, "test", move |resource| {
            if resource.scheme() == "test" {
                let language_selection = language_service.create_by_id(Some("json"));
                let model = model_service.create_model("Hello Test", Some(language_selection.as_ref()), Some(resource), false);

                return MaybePromise::Promise(Promise::resolved(Some(model)));
            }

            MaybePromise::Promise(Promise::resolved(None))
        });

        let resource = uri_of("test", "thePath");
        let input = fixture.accessor.instantiation_service.create_instance::<TextResourceEditorInput>((
            resource,
            Some("The Name".to_string()),
            Some("The Description".to_string()),
            None,
            None,
        ));

        let model = input.resolve().await.unwrap().expect("o modelo");
        disposables.add(model.clone());
        let text_resource_model = model.as_any().downcast_ref::<TextResourceEditorModel>().expect("o modelo resolvido é um TextResourceEditorModel");
        let mut snapshot = text_resource_model.create_snapshot().expect("o snapshot");
        assert_eq!(snapshot_to_string(snapshot.as_mut()), "Hello Test");

        let disposed = Rc::new(Cell::new(false));
        let (disposed_promise, resolver) = Promise::<()>::new();
        let listener_disposed = disposed.clone();
        model.on_will_dispose().once().listen(move |_| {
            listener_disposed.set(true);
            resolver.resolve(());
        });
        input.dispose();

        disposed_promise.await.unwrap();
        assert!(disposed.get());
    });
}

#[test]
fn resolve_file() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = to_resource("resolve file", "/path/file_resolver.txt");
        let text_model = disposables.add_rc(fixture.accessor.create_model(&resource, Some("utf8"), None));
        fixture.accessor.files().add(&resource, &text_model);

        text_model.resolve().await.unwrap();

        let reference = fixture.resolver.create_model_reference(&resource).await.unwrap();

        let model = reference.object.clone();
        let editor_model = model.text_editor_model().expect("o modelo de texto existe");

        assert_eq!(editor_model.get_value().to_string_lossy(), "Hello Html");

        let disposed = once_flag(model.on_will_dispose());

        reference.dispose();
        timeout(0).await.unwrap(); // due to the reference resolving the model first which is async
        assert!(disposed.get());
    });
}

#[test]
fn resolved_dirty_file_eventually_disposes() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = to_resource("resolved dirty file eventually disposes", "/path/file_resolver.txt");
        let text_model = disposables.add_rc(fixture.accessor.create_model(&resource, Some("utf8"), None));
        fixture.accessor.files().add(&resource, &text_model);

        text_model.resolve().await.unwrap();

        update_model(&text_model, "make dirty");

        let reference = fixture.resolver.create_model_reference(&resource).await.unwrap();

        let disposed = once_flag(text_model.on_will_dispose());

        reference.dispose();
        timeout(0).await.unwrap();
        assert!(!disposed.get()); // not disposed because model still dirty

        let _ = text_model.revert(None);

        timeout(0).await.unwrap();
        assert!(disposed.get()); // now disposed because model got reverted
    });
}

#[test]
fn resolved_dirty_file_does_not_dispose_when_new_reference_created() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = to_resource("resolved dirty file does not dispose when new reference created", "/path/file_resolver.txt");
        let text_model = disposables.add_rc(fixture.accessor.create_model(&resource, Some("utf8"), None));
        fixture.accessor.files().add(&resource, &text_model);

        text_model.resolve().await.unwrap();

        update_model(&text_model, "make dirty");

        let ref1 = fixture.resolver.create_model_reference(&resource).await.unwrap();

        let disposed = once_flag(text_model.on_will_dispose());

        ref1.dispose();
        timeout(0).await.unwrap();
        assert!(!disposed.get()); // not disposed because model still dirty

        let ref2 = fixture.resolver.create_model_reference(&resource).await.unwrap();

        let _ = text_model.revert(None);

        timeout(0).await.unwrap();
        assert!(!disposed.get()); // not disposed because we got another ref meanwhile

        ref2.dispose();

        timeout(0).await.unwrap();
        assert!(disposed.get()); // now disposed because last ref got disposed
    });
}

#[test]
fn resolve_untitled() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let service = fixture.accessor.untitled_text_editor_service.clone();
        let untitled_model = service.create(None);
        disposables.add(untitled_model.clone());
        let input = disposables.add_rc(fixture.accessor.instantiation_service.create_instance::<UntitledTextEditorInput>((untitled_model.clone(),)));

        input.resolve_untitled().await.unwrap();
        let reference = fixture.resolver.create_model_reference(&EditorInput::resource(&*input).expect("a entrada sem título tem recurso")).await.unwrap();
        let model = reference.object.clone();
        assert!(same_model(&untitled_model, &model));
        assert!(model.text_editor_model().is_some());
        reference.dispose();
        input.dispose();
        model.dispose();
    });
}

#[test]
fn even_loading_documents_should_be_refcounted() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let (wait_for_it, resolve_model) = Promise::<()>::new();

        let (model_service, language_service, store) = (fixture.accessor.model_service.clone(), fixture.accessor.language_service.clone(), disposables.store());
        register_provider(&disposables, &fixture, "test", move |resource| {
            let (wait_for_it, model_service, language_service, store, resource) =
                (wait_for_it.clone(), model_service.clone(), language_service.clone(), store.clone(), resource.clone());

            MaybePromise::Promise(async_fn(async move {
                wait_for_it.await?;

                let model_content = "Hello Test";
                let language_selection = language_service.create_by_id(Some("json"));
                let model = model_service.create_model(model_content, Some(language_selection.as_ref()), Some(&resource), false);
                store.add_rc(model.clone());

                Ok(Some(model))
            }))
        });

        let uri = uri_of("test", "thePath");

        let model_ref_promise1 = fixture.resolver.create_model_reference(&uri);
        let model_ref_promise2 = fixture.resolver.create_model_reference(&uri);

        resolve_model.resolve(());

        let model_ref1 = model_ref_promise1.await.unwrap();
        let model1 = model_ref1.object.clone();
        let model_ref2 = model_ref_promise2.await.unwrap();
        let model2 = model_ref2.object.clone();
        let text_model = model1.text_editor_model().expect("o modelo de texto existe");

        assert!(same_model(&model1, &model2), "they are the same model");
        assert!(!text_model.is_disposed(), "the text model should not be disposed");

        model_ref1.dispose();
        assert!(!text_model.is_disposed(), "the text model should still not be disposed");

        let p1 = will_dispose(&disposables, &text_model);
        model_ref2.dispose();

        p1.await.unwrap();
        assert!(text_model.is_disposed(), "the text model should finally be disposed");
    });
}

#[test]
fn resolve_in_memory() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = uri_of(schemas::IN_MEMORY, "/test/inMemoryDoc");
        disposables.add_rc(create_json_model(&fixture, "Hello InMemory", &resource));

        let reference = fixture.resolver.create_model_reference(&resource).await.unwrap();
        let model = reference.object.clone();
        let text_model = model.text_editor_model().expect("o modelo de texto existe");
        assert_eq!(text_model.get_value().to_string_lossy(), "Hello InMemory");
        assert!(!text_model.is_disposed(), "the inMemory text model should not be disposed before releasing the reference");

        let p = will_dispose(&disposables, &text_model);
        reference.dispose();

        p.await.unwrap();
        assert!(text_model.is_disposed(), "the inMemory text model should be disposed after the reference is released");
    });
}

/// O corpo de `synthetic document survives until both owners release (creator first: ${releaseCreatorFirst})`.
fn synthetic_document_survives_until_both_owners_release(release_creator_first: bool) {
    run_with_real_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let owner = create_synthetic_document(&fixture, "shared".into(), None).await;
        track(&disposables, &owner);
        let model = text_model_of(&owner);
        let disposal_count = count_will_dispose(&disposables, &model);
        let reference = fixture.resolver.create_model_reference(&model.uri()).await.unwrap();
        track(&disposables, &reference);
        if release_creator_first {
            owner.dispose();
        } else {
            reference.dispose();
            timeout(0).await.unwrap();
        }
        assert_eq!((model.get_value().to_string_lossy(), disposal_count.get()), ("shared".to_string(), 0));

        let disposed = model.on_will_dispose().to_promise(None);
        owner.dispose();
        reference.dispose();
        disposed.await.unwrap();
        assert_eq!((model.is_disposed(), disposal_count.get()), (true, 1));
    });
}

#[test]
fn synthetic_document_survives_until_both_owners_release_creator_first_false() {
    synthetic_document_survives_until_both_owners_release(false);
}

#[test]
fn synthetic_document_survives_until_both_owners_release_creator_first_true() {
    synthetic_document_survives_until_both_owners_release(true);
}

#[test]
fn synthetic_document_is_retained_while_a_resolver_reference_is_still_opening() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let owner = create_synthetic_document(&fixture, "shared".into(), None).await;
        track(&disposables, &owner);
        let model = text_model_of(&owner);
        let pending = fixture.resolver.create_model_reference(&model.uri());
        owner.dispose();
        let reference = pending.await.unwrap();
        track(&disposables, &reference);
        assert_eq!(text_model_of(&reference).get_value().to_string_lossy(), "shared");
        let disposed = model.on_will_dispose().to_promise(None);
        reference.dispose();
        disposed.await.unwrap();
    });
}

#[test]
fn synthetic_document_can_be_reacquired_during_pending_resolver_disposal() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let owner = create_synthetic_document(&fixture, "shared".into(), None).await;
        track(&disposables, &owner);
        let model = text_model_of(&owner);
        owner.dispose();
        let pending = fixture.resolver.create_model_reference(&model.uri());
        let second = pending.await.unwrap();
        track(&disposables, &second);
        assert_eq!(text_model_of(&second).get_value().to_string_lossy(), "shared");
        let disposed = model.on_will_dispose().to_promise(None);
        second.dispose();
        disposed.await.unwrap();
    });
}

#[test]
fn synthetic_creation_establishes_the_reference_before_on_model_added() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let pending: Rc<RefCell<Option<Promise<TextModelReference>>>> = Rc::default();
        let (listener_pending, resolver) = (pending.clone(), fixture.resolver.clone());
        disposables.add(fixture.accessor.model_service.on_model_added().listen(move |model| {
            *listener_pending.borrow_mut() = Some(resolver.create_model_reference(&model.uri()));
        }));
        let owner = create_synthetic_document(&fixture, "".into(), None).await;
        track(&disposables, &owner);
        let pending = pending.borrow().clone();
        let pending = pending.expect("a referência foi pedida no onModelAdded");
        let reference = pending.await.unwrap();
        track(&disposables, &reference);
        assert!(same_model(&reference.object, &owner.object));
        reference.dispose();
        timeout(0).await.unwrap();
        assert!(!text_model_of(&owner).is_disposed());
        let disposed = text_model_of(&owner).on_will_dispose().to_promise(None);
        owner.dispose();
        disposed.await.unwrap();
    });
}

#[test]
fn synthetic_documents_have_unique_uris_and_preserve_content_and_language() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        disposables.add(fixture.accessor.language_service.register_language(LanguageExtensionPoint::new("json")));
        let language = fixture.accessor.language_service.create_by_id(Some("json"));
        let units: Vec<u16> = "{}".encode_utf16().collect();
        let first = create_synthetic_document(&fixture, TextSource::Factory(create_text_buffer_factory(&units)), Some(language)).await;
        track(&disposables, &first);
        let second = create_synthetic_document(&fixture, "".into(), None).await;
        track(&disposables, &second);
        let first_model = text_model_of(&first);
        let language_id = first.object.as_any().downcast_ref::<TextResourceEditorModel>().expect("o modelo é um TextResourceEditorModel").get_language_id();
        assert_eq!(
            (
                first_model.get_value().to_string_lossy(),
                language_id,
                first_model.uri().to_string(false).to_string_lossy() != text_model_of(&second).uri().to_string(false).to_string_lossy(),
                first_model.uri().scheme().to_string_lossy(),
            ),
            ("{}".to_string(), Some("json".to_string()), true, schemas::IN_MEMORY.to_string())
        );
        let disposed_first = first_model.on_will_dispose().to_promise(None);
        let disposed_second = text_model_of(&second).on_will_dispose().to_promise(None);
        first.dispose();
        second.dispose();
        disposed_first.await.unwrap();
        disposed_second.await.unwrap();
    });
}

#[test]
fn failed_synthetic_wrapper_construction_releases_the_text_model() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let store = disposables.store();
        let services = fixture.accessor.instantiation_service.create_child(
            ServiceCollection::new().with(I_LANGUAGE_DETECTION_SERVICE, SyncDescriptor::<dyn ILanguageDetectionService>::new::<ThrowingLanguageDetectionService>(())),
            Some(&*store),
        );
        let resolver = disposables.add_rc(services.create_instance::<TextModelResolverService>(()));
        let error = resolver.create_synthetic_document("".into(), None).await.err().expect("a criação do documento sintético rejeita");
        assert!(error.to_string().contains("Wrapper construction failed"), "{error}");
        assert_eq!(fixture.accessor.model_service.get_models().len(), 0);
    });
}

#[test]
fn resolve_in_memory_throws_when_model_not_found() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = uri_of(schemas::IN_MEMORY, "/test/nonExistent");

        let error = fixture.resolver.create_model_reference(&resource).await.err().expect("a resolução rejeita");
        assert!(error.to_string().contains("Unable to resolve text model content for resource"), "{error}");
    });
}

#[test]
fn resolve_in_memory_disposes_when_last_reference_released() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = uri_of(schemas::IN_MEMORY, "/test/inMemoryDispose");
        create_json_model(&fixture, "Hello InMemory", &resource);

        let reference = fixture.resolver.create_model_reference(&resource).await.unwrap();
        let text_model = text_model_of(&reference);
        assert!(!text_model.is_disposed());

        let p = will_dispose(&disposables, &text_model);
        reference.dispose();

        p.await.unwrap();
        assert!(text_model.is_disposed(), "the inMemory text model should be disposed after last reference is released");
    });
}

#[test]
fn resolve_in_memory_is_refcounted() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = uri_of(schemas::IN_MEMORY, "/test/inMemoryRefcount");
        create_json_model(&fixture, "Hello InMemory", &resource);

        let ref1 = fixture.resolver.create_model_reference(&resource).await.unwrap();
        let ref2 = fixture.resolver.create_model_reference(&resource).await.unwrap();
        let text_model = text_model_of(&ref1);

        assert!(same_model(&ref1.object, &ref2.object), "they are the same model");
        assert!(!text_model.is_disposed());

        ref1.dispose();
        assert!(!text_model.is_disposed(), "should not dispose while ref2 is still alive");

        let p = will_dispose(&disposables, &text_model);
        ref2.dispose();

        p.await.unwrap();
        assert!(text_model.is_disposed(), "should dispose after last reference released");
    });
}

#[test]
fn resolve_in_memory_reuses_model_when_re_acquired_during_dispose() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = uri_of(schemas::IN_MEMORY, "/test/inMemoryReuse");
        create_json_model(&fixture, "Hello Reuse", &resource);

        let ref1 = fixture.resolver.create_model_reference(&resource).await.unwrap();
        let model1 = ref1.object.clone();

        // Release last reference, starts async dispose
        ref1.dispose();

        // Immediately re-acquire before the async dispose completes
        let ref2 = fixture.resolver.create_model_reference(&resource).await.unwrap();
        let model2 = ref2.object.clone();

        let text_model = model2.text_editor_model().expect("o modelo de texto existe");
        assert_eq!(text_model.get_value().to_string_lossy(), "Hello Reuse");
        assert!(same_model(&model1, &model2), "should reuse the same model instance");

        let p = will_dispose(&disposables, &text_model);
        ref2.dispose();

        p.await.unwrap();
        assert!(text_model.is_disposed());
    });
}
