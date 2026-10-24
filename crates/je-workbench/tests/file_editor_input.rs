//! Porte de `src/vs/workbench/contrib/files/test/browser/fileEditorInput.test.ts` (suíte `Files - FileEditorInput`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois. O `teardown`
//!   (`disposables.clear()`) roda quando o guarda sai de escopo, antes de ele conferir os vazamentos; o primeiro descartável
//!   registrado descarta os modelos do gerenciador, como no `setup` de `text_file_editor_model.rs`. Os casos `async` rodam em
//!   `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService({ textEditorService })` e `TestServiceAccessor`.** Estão em
//!   `tests/support/text_file_support.rs` ([`TextFileTestAccessor`], com o `instantiationService` e o `ITextModelService`).
//!   O `TestTextEditorService extends TextEditorService` sobrescreve `createTextEditor` e `resolveTextEditor` por completo
//!   (as duas só criam a `FileEditorInput` do recurso), então o [`TestTextEditorService`] implementa o contrato
//!   `ITextEditorService` direto, sem a classe base, e entra na coleção de serviços como a sobrescrita `textEditorService`.
//! - **`createFileInput(...)`.** [`create_file_input_with`] (os argumentos opcionais são [`Preferences`]), com a entrada
//!   registrada no store do guarda.
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título do caso (a suíte
//!   mais o título do caso) é o argumento de [`to_resource`].
//! - **`input.resolve()`.** É [`FileEditorInput::resolve_file`], que devolve o modelo como [`IEditorModel`] (o
//!   `as TextFileEditorModel` e o `instanceof` são o `downcast_ref` e o `is` por `IEditorModel::as_any`); `disposables.add(model)`
//!   é [`track_model`].
//! - **`assert(input.resource instanceof URI)`** é garantido pelo tipo (`input.resource` é um `URI`).
//! - **`getLastResolvedFileStat(resolved)`** é `TextFileEditorModel::last_resolved_file_stat`. A última asserção de `Basics`
//!   (`stat !== getLastResolvedFileStat(resolved)`, "o `resolve` sempre vai ao servidor") compara a identidade de dois
//!   objetos, e o stat do porte é um valor; o porte confere o mesmo fato, que o `resolve` leu o arquivo de novo (o
//!   `readFileStream` do `TestFileService` foi chamado mais uma vez).
//! - **`try/finally`** de `DisposableStore.DISABLE_DISPOSED_WARNING` e do `readShouldThrowError`: o valor é desfeito em seguida,
//!   sem `finally` (uma falha de asserção já reprova o caso, e o `teardown` descarta os serviços).
//! - **`let e: Error | undefined`** de `resolve throws for too large files` é o `is_err()` da promessa.
//! - **`(editorSerializer.deserialize(...) as FileEditorInput)`** é o `downcast_ref` por `EditorInput::as_any`;
//!   `Registry.as<IEditorFactoryRegistry>(EditorExtensions.EditorFactory)` é [`editor_factory_registry`].
//! - **`assert.fail()`** de `File Editor Input Serializer missing` e de `Unexpected serialized file input` é o `expect` do
//!   `Option`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_workbench::services::textfile::textfiles::ITextFileEditorModelManager;
use std::cell::Cell;
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::{LanguageExtensionPoint, PLAINTEXT_LANGUAGE_ID};
use je_editor::text_model::TextSource;
use je_instantiation::InstantiationService;
use je_platform::files::files::FileOperationError;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::resources::{fs_path, host_environment, uri_file};
use je_workbench::common::editor::binary_editor_model::BinaryEditorModel;
use je_workbench::common::editor::editor_factory_registry::editor_factory_registry;
use je_workbench::common::editor::editor_input::{EditorInput, UntypedEditorOptions};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::{EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, UntypedEditorInput, Verbosity};
use je_workbench::contrib::files::browser::editors::file_editor_handler::FileEditorInputSerializer;
use je_workbench::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::textfile::text_editor_service::{ITextEditorService, I_TEXT_EDITOR_SERVICE};
use je_workbench::services::textfile::text_file_editor_model::TextFileEditorModel;
use je_workbench::services::textfile::textfiles::{
    EncodingMode, IEncodingSupport, ILanguageSupport, ITextFileEditorModel, TextFileOperationError, TextFileOperationResult,
};
use support::text_file_support::TextFileTestAccessor;

//#region Utilitários dos casos

/// O prefixo do `this.test.fullTitle()` do mocha: o título da suíte.
const SUITE_TITLE: &str = "Files - FileEditorInput";

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

/// `a === b` para os modelos (o endereço do objeto, sem o `vtable`).
fn same_model<A: ?Sized, B: ?Sized>(a: &Rc<A>, b: &Rc<B>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `resource.with({ scheme })`.
fn with_scheme(resource: &URI, scheme: &str) -> URI {
    resource.with(&UriChange { scheme: Field::from(scheme), ..UriChange::default() })
}

/// A entrada sem tipo `{ preserveViewState: 0 }` de `toUntyped`.
fn preserve_view_state() -> UntypedEditorOptions {
    UntypedEditorOptions { preserve_view_state: Some(0), ..UntypedEditorOptions::default() }
}

/// A `FileEditorInput` como `EditorInput | IUntypedEditorInput`.
fn typed(input: &Rc<FileEditorInput>) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(input.clone())
}

/// `(model as TextFileEditorModel)` do modelo que `FileEditorInput.resolve` devolve.
fn as_text_file_editor_model(model: &Rc<dyn IEditorModel>) -> &TextFileEditorModel {
    model.as_any().downcast_ref::<TextFileEditorModel>().expect("o modelo é um TextFileEditorModel")
}

/// `disposables.add(model)` do modelo que `FileEditorInput.resolve` devolve.
fn track_model(disposables: &LeakGuard, model: &Rc<dyn IEditorModel>) {
    let model = model.clone();
    disposables.add(to_disposable(move || model.dispose()));
}

/// `model.textEditorModel!.getValue()`.
fn editor_value(model: &TextFileEditorModel) -> String {
    ITextFileEditorModel::text_editor_model(model).expect("o modelo de texto existe").get_value().to_string_lossy()
}

/// `model.textEditorModel!.getLanguageId()`.
fn editor_language_id(model: &TextFileEditorModel) -> String {
    ITextFileEditorModel::text_editor_model(model).expect("o modelo de texto existe").get_language_id()
}

/// `model.textEditorModel!.setValue(value)`.
fn set_editor_value(model: &TextFileEditorModel, value: &str) {
    ITextFileEditorModel::text_editor_model(model).expect("o modelo de texto existe").set_value(TextSource::from(value));
}

/// Conta os disparos de `event`.
fn count<T: 'static>(disposables: &LeakGuard, event: je_base::event::Event<T>) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    let listener_counter = counter.clone();
    disposables.add(event.listen(move |_| listener_counter.set(listener_counter.get() + 1)));

    counter
}

/// Os argumentos opcionais de `createFileInput(resource, preferredResource?, preferredLanguageId?, preferredName?,
/// preferredDescription?, preferredContents?)`.
#[derive(Default)]
struct Preferences<'a> {
    resource: Option<URI>,
    language_id: Option<&'a str>,
    name: Option<&'a str>,
    description: Option<&'a str>,
    contents: Option<&'a str>,
}

/// `createFileInput(resource, ...)`: `disposables.add(instantiationService.createInstance(FileEditorInput, resource,
/// preferredResource, preferredName, preferredDescription, undefined, preferredLanguageId, preferredContents))`.
fn create_file_input_with(accessor: &TextFileTestAccessor, disposables: &LeakGuard, resource: &URI, preferences: Preferences) -> Rc<FileEditorInput> {
    disposables.add_rc(accessor.instantiation_service.create_instance::<FileEditorInput>((
        resource.clone(),
        preferences.resource,
        preferences.name.map(str::to_string),
        preferences.description.map(str::to_string),
        None,
        preferences.language_id.map(str::to_string),
        preferences.contents.map(str::to_string),
    )))
}

/// `createFileInput(resource)`.
fn create_file_input(accessor: &TextFileTestAccessor, disposables: &LeakGuard, resource: &URI) -> Rc<FileEditorInput> {
    create_file_input_with(accessor, disposables, resource, Preferences::default())
}

/// `TestTextEditorService`: `createTextEditor` e `resolveTextEditor` criam a `FileEditorInput` do recurso.
struct TestTextEditorService {
    instantiation_service: Weak<InstantiationService>,
    disposables: Rc<DisposableStore>,
}

impl TestTextEditorService {
    /// `createFileInput(input.resource)`.
    fn create_file_input(&self, input: &UntypedEditorInput) -> EditorInputRef {
        let instantiation_service = self.instantiation_service.upgrade().expect("o serviço de instanciação está vivo");
        let resource = input.resource.clone().expect("a entrada sem tipo tem recurso");

        let input = self.disposables.add_rc(instantiation_service.create_instance::<FileEditorInput>((resource, None, None, None, None, None, None)));
        input
    }
}

impl ITextEditorService for TestTextEditorService {
    fn create_text_editor(&self, input: UntypedEditorInput) -> EditorInputRef {
        self.create_file_input(&input)
    }

    fn resolve_text_editor(&self, input: UntypedEditorInput) -> Promise<EditorInputRef> {
        Promise::resolved(self.create_file_input(&input))
    }
}

/// O `setup`: `workbenchInstantiationService({ textEditorService: ... }, disposables)` e o `TestServiceAccessor`. O primeiro
/// descartável registrado descarta os modelos do gerenciador antes do restante.
fn setup(disposables: &LeakGuard) -> TextFileTestAccessor {
    let manager_slot: Rc<std::cell::RefCell<Option<Rc<je_workbench::services::textfile::text_file_editor_model_manager::TextFileEditorModelManager>>>> = Rc::default();
    let slot = manager_slot.clone();
    disposables.add(to_disposable(move || {
        let manager = slot.borrow_mut().take();
        if let Some(manager) = manager {
            for text_file_editor_model in manager.models() {
                text_file_editor_model.dispose();
            }
        }
    }));

    let accessor = TextFileTestAccessor::new(&disposables.store());

    let files = accessor.files();
    *manager_slot.borrow_mut() = Some(files.clone());
    disposables.add_rc(files);

    let file_service = accessor.file_service.clone();
    let content = file_service.get_content();
    disposables.add(to_disposable(move || file_service.set_content(&content)));

    accessor.collection.set(
        I_TEXT_EDITOR_SERVICE,
        Rc::new(TestTextEditorService { instantiation_service: Rc::downgrade(&accessor.instantiation_service), disposables: disposables.store() }) as Rc<dyn ITextEditorService>,
    );

    accessor
}

//#endregion

#[test]
fn basics() {
    const TITLE: &str = "Basics";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/file.js"));
        let other_input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "foo/bar/otherfile.js"));
        let other_input_same = create_file_input(&accessor, &disposables, &to_resource(TITLE, "foo/bar/file.js"));

        assert!(input.matches(&typed(&input)));
        assert!(input.matches(&typed(&other_input_same)));
        assert!(!input.matches(&typed(&other_input)));
        assert!(!input.get_name().is_empty());
        assert!(input.get_description(None).is_some_and(|description| !description.is_empty()));
        assert!(!input.get_title(Some(Verbosity::Short)).is_empty());

        assert!(!input.has_capability(EditorInputCapabilities::UNTITLED));
        assert!(!input.has_capability(EditorInputCapabilities::READONLY));
        assert!(!input.is_readonly().is_readonly());
        assert!(!input.has_capability(EditorInputCapabilities::SINGLETON));
        assert!(!input.has_capability(EditorInputCapabilities::REQUIRES_TRUST));

        let untyped_input = input.to_untyped(Some(&preserve_view_state())).expect("a entrada tem forma sem tipo");
        assert_eq!(uri_string(&untyped_input.resource.expect("a entrada sem tipo tem recurso")), uri_string(&input.resource));

        assert_eq!("file.js", input.get_name());

        assert_eq!(fs_path(&to_resource(TITLE, "/foo/bar/file.js")), fs_path(&input.resource));

        let _input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar.html"));

        let input_to_resolve = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/file.js"));
        let same_other_input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/file.js"));

        let mut resolved = input_to_resolve.resolve_file(None).await.unwrap();
        assert!(input_to_resolve.is_resolved());

        let resolved_model_a = resolved.clone();
        resolved = input_to_resolve.resolve_file(None).await.unwrap();
        assert!(same_model(&resolved_model_a, &resolved)); // OK: Resolved Model cached globally per input

        DisposableStore::set_disable_disposed_warning(true); // prevent unwanted warning output from occurring

        let other_resolved = same_other_input.resolve_file(None).await.unwrap();
        assert!(same_model(&other_resolved, &resolved_model_a)); // OK: Resolved Model cached globally per input
        input_to_resolve.dispose();

        resolved = input_to_resolve.resolve_file(None).await.unwrap();
        assert!(same_model(&resolved_model_a, &resolved)); // Model is still the same because we had 2 clients
        input_to_resolve.dispose();
        same_other_input.dispose();
        resolved_model_a.dispose();

        resolved = input_to_resolve.resolve_file(None).await.unwrap();
        assert!(!same_model(&resolved_model_a, &resolved)); // Different instance, because input got disposed

        let reads_before = accessor.file_service.read_file_stream_operations.borrow().len();
        input_to_resolve.resolve_file(None).await.unwrap();
        timeout(0).await.unwrap();
        assert!(accessor.file_service.read_file_stream_operations.borrow().len() > reads_before); // Different stat, because resolve always goes to the server for refresh

        DisposableStore::set_disable_disposed_warning(false);
    });
}

#[test]
fn reports_as_untitled_without_supported_file_scheme() {
    const TITLE: &str = "reports as untitled without supported file scheme";
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    let input = create_file_input(&accessor, &disposables, &with_scheme(&to_resource(TITLE, "/foo/bar/file.js"), "someTestingScheme"));

    assert!(input.has_capability(EditorInputCapabilities::UNTITLED));
    assert!(!input.has_capability(EditorInputCapabilities::READONLY));
    assert!(!input.is_readonly().is_readonly());
}

#[test]
fn reports_as_readonly_with_readonly_file_scheme() {
    const TITLE: &str = "reports as readonly with readonly file scheme";
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    let in_memory_filesystem_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
    in_memory_filesystem_provider.set_read_only(true);

    disposables.add(accessor.file_service.register_provider("someTestingReadonlyScheme", in_memory_filesystem_provider.clone()).unwrap());
    let input = create_file_input(&accessor, &disposables, &with_scheme(&to_resource(TITLE, "/foo/bar/file.js"), "someTestingReadonlyScheme"));

    assert!(!input.has_capability(EditorInputCapabilities::UNTITLED));
    assert!(input.has_capability(EditorInputCapabilities::READONLY));
    assert!(input.is_readonly().is_readonly());
}

#[test]
fn preferred_resource() {
    const TITLE: &str = "preferred resource";
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    let resource = to_resource(TITLE, "/foo/bar/updatefile.js");
    let preferred_resource = to_resource(TITLE, "/foo/bar/UPDATEFILE.js");

    let input_without_preferred_resource = create_file_input(&accessor, &disposables, &resource);
    assert_eq!(uri_string(&input_without_preferred_resource.resource), uri_string(&resource));
    assert_eq!(uri_string(&input_without_preferred_resource.preferred_resource().expect("a entrada tem recurso preferido")), uri_string(&resource));

    let input_with_preferred_resource = create_file_input_with(&accessor, &disposables, &resource, Preferences { resource: Some(preferred_resource.clone()), ..Preferences::default() });

    assert_eq!(uri_string(&input_with_preferred_resource.resource), uri_string(&resource));
    assert_eq!(uri_string(&input_with_preferred_resource.preferred_resource().expect("a entrada tem recurso preferido")), uri_string(&preferred_resource));

    let did_change_label = count(&disposables, input_with_preferred_resource.on_did_change_label());

    assert_eq!(input_with_preferred_resource.get_name(), "UPDATEFILE.js");

    let other_preferred_resource = to_resource(TITLE, "/FOO/BAR/updateFILE.js");
    input_with_preferred_resource.set_preferred_resource(&other_preferred_resource);

    assert_eq!(uri_string(&input_with_preferred_resource.resource), uri_string(&resource));
    assert_eq!(uri_string(&input_with_preferred_resource.preferred_resource().expect("a entrada tem recurso preferido")), uri_string(&other_preferred_resource));
    assert_eq!(input_with_preferred_resource.get_name(), "updateFILE.js");
    assert!(did_change_label.get() > 0);
}

#[test]
fn preferred_language() {
    const TITLE: &str = "preferred language";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let language_id = "file-input-test";
        disposables.add(accessor.language_service.register_language(LanguageExtensionPoint::new(language_id)));

        let input = create_file_input_with(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/file.js"), Preferences { language_id: Some(language_id), ..Preferences::default() });
        assert_eq!(input.get_preferred_language_id().as_deref(), Some(language_id));

        let resolved = input.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved);
        let model = as_text_file_editor_model(&resolved);
        assert_eq!(editor_language_id(model), language_id);

        input.set_language_id("text", None);
        assert_eq!(input.get_preferred_language_id().as_deref(), Some("text"));
        assert_eq!(editor_language_id(model), PLAINTEXT_LANGUAGE_ID);

        let input2 = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/file.js"));
        input2.set_preferred_language_id(language_id);

        let resolved2 = input2.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved2);
        let model2 = as_text_file_editor_model(&resolved2);
        assert_eq!(editor_language_id(model2), language_id);
    });
}

#[test]
fn preferred_contents() {
    const TITLE: &str = "preferred contents";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input_with(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/file.js"), Preferences { contents: Some("My contents"), ..Preferences::default() });

        let resolved = input.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved);
        let model = as_text_file_editor_model(&resolved);
        assert_eq!(editor_value(model), "My contents");
        assert!(input.is_dirty());

        let untyped_input = input.to_untyped(Some(&preserve_view_state())).expect("a entrada tem forma sem tipo");
        assert_eq!(untyped_input.contents.as_deref(), Some("My contents"));

        let untyped_input_without_contents = input.to_untyped(None).expect("a entrada tem forma sem tipo");
        assert_eq!(untyped_input_without_contents.contents, None);

        input.set_preferred_contents("Other contents");
        input.resolve_file(None).await.unwrap();
        assert_eq!(editor_value(model), "Other contents");

        set_editor_value(model, "Changed contents");
        input.resolve_file(None).await.unwrap();
        assert_eq!(editor_value(model), "Changed contents"); // preferred contents only used once

        let input2 = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/file.js"));
        input2.set_preferred_contents("My contents");

        let resolved2 = input2.resolve_file(None).await.unwrap();
        let model2 = as_text_file_editor_model(&resolved2);
        assert_eq!(editor_value(model2), "My contents");
        assert!(input2.is_dirty());
    });
}

#[test]
fn matches() {
    const TITLE: &str = "matches";
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    let input1 = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));
    let input2 = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));
    let input3 = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/other.js"));
    let input2_upper = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/UPDATEFILE.js"));

    assert!(input1.matches(&typed(&input1)));
    assert!(input1.matches(&typed(&input2)));
    assert!(!input1.matches(&typed(&input3)));

    assert!(!input1.matches(&typed(&input2_upper)));
}

#[test]
fn get_encoding_set_encoding() {
    const TITLE: &str = "getEncoding/setEncoding";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));

        input.set_encoding("utf16", EncodingMode::Encode).await.unwrap();
        assert_eq!(IEncodingSupport::get_encoding(&*input).as_deref(), Some("utf16"));

        let resolved = input.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved);
        let model = as_text_file_editor_model(&resolved);
        assert_eq!(IEncodingSupport::get_encoding(&*input), ITextFileEditorModel::get_encoding(model));
    });
}

#[test]
fn save() {
    const TITLE: &str = "save";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));

        let resolved = input.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved);
        set_editor_value(as_text_file_editor_model(&resolved), "changed");
        assert!(input.is_dirty());
        assert!(input.is_modified());

        input.save(0, None).await.unwrap();
        assert!(!input.is_dirty());
        assert!(!input.is_modified());
    });
}

#[test]
fn revert() {
    const TITLE: &str = "revert";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));

        let resolved = input.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved);
        set_editor_value(as_text_file_editor_model(&resolved), "changed");
        assert!(input.is_dirty());
        assert!(input.is_modified());

        input.revert(0, None).await.unwrap();
        assert!(!input.is_dirty());
        assert!(!input.is_modified());

        input.dispose();
        assert!(input.is_disposed());
    });
}

#[test]
fn resolve_handles_binary_files() {
    const TITLE: &str = "resolve handles binary files";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));

        accessor.text_file_service.set_read_stream_error_once(TextFileOperationError::new("error", TextFileOperationResult::FileIsBinary));

        let resolved = input.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved);
    });
}

#[test]
fn resolve_throws_for_too_large_files() {
    const TITLE: &str = "resolve throws for too large files";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));

        accessor.text_file_service.set_read_stream_error_once(FileOperationError::too_large("error", 1000));
        let result = input.resolve_file(None).await;
        assert!(result.is_err());
    });
}

#[test]
fn attaches_to_model_when_created_and_reports_dirty() {
    const TITLE: &str = "attaches to model when created and reports dirty";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));

        let listener_count = count(&disposables, input.on_did_change_dirty());

        // instead of going through file input resolve method
        // we resolve the model directly through the service
        let model = disposables.add_rc(accessor.files().resolve_model(&input.resource, None).await.unwrap());
        set_editor_value(&model, "hello world");

        assert_eq!(listener_count.get(), 1);
        assert!(input.is_dirty());
    });
}

#[test]
fn force_open_text_binary() {
    const TITLE: &str = "force open text/binary";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));
        input.set_force_open_as_binary();

        let mut resolved = input.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved);
        assert!(resolved.as_any().is::<BinaryEditorModel>());

        input.set_force_open_as_text();

        resolved = input.resolve_file(None).await.unwrap();
        track_model(&disposables, &resolved);
        assert!(resolved.as_any().is::<TextFileEditorModel>());
    });
}

#[test]
fn file_editor_serializer() {
    const TITLE: &str = "file editor serializer";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        accessor.instantiation_service.invoke_function(|accessor| editor_factory_registry().start(accessor));

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));
        let input_ref: EditorInputRef = input.clone();

        disposables.add(editor_factory_registry().register_editor_serializer::<FileEditorInputSerializer>("workbench.editors.files.fileEditorInput"));

        let editor_serializer = editor_factory_registry().get_editor_serializer_by_type_id(&input.editor_type_id()).expect("File Editor Input Serializer missing");

        assert!(editor_serializer.can_serialize(&input_ref));

        let input_serialized = editor_serializer.serialize(&input_ref).expect("Unexpected serialized file input");

        let input_deserialized = editor_serializer.deserialize(&accessor.instantiation_service, &input_serialized);
        assert!(input_deserialized.is_some_and(|deserialized| input.matches(&EditorInputOrUntyped::Typed(deserialized))));

        let preferred_resource = to_resource(TITLE, "/foo/bar/UPDATEfile.js");
        let input_with_preferred_resource = create_file_input_with(
            &accessor,
            &disposables,
            &to_resource(TITLE, "/foo/bar/updatefile.js"),
            Preferences { resource: Some(preferred_resource), ..Preferences::default() },
        );
        let input_with_preferred_resource_ref: EditorInputRef = input_with_preferred_resource.clone();

        let input_with_preferred_resource_serialized = editor_serializer.serialize(&input_with_preferred_resource_ref).expect("Unexpected serialized file input");

        let input_with_preferred_resource_deserialized =
            editor_serializer.deserialize(&accessor.instantiation_service, &input_with_preferred_resource_serialized).expect("o serializador devolve a entrada");
        let input_with_preferred_resource_deserialized =
            input_with_preferred_resource_deserialized.as_any().downcast_ref::<FileEditorInput>().expect("a entrada é uma FileEditorInput");
        assert_eq!(uri_string(&input_with_preferred_resource.resource), uri_string(&input_with_preferred_resource_deserialized.resource));
        assert_eq!(
            uri_string(&input_with_preferred_resource.preferred_resource().expect("a entrada tem recurso preferido")),
            uri_string(&input_with_preferred_resource_deserialized.preferred_resource().expect("a entrada tem recurso preferido"))
        );
    });
}

#[test]
fn preferred_name_description() {
    const TITLE: &str = "preferred name/description";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        // Works with custom file input
        let custom_file_input = create_file_input_with(
            &accessor,
            &disposables,
            &with_scheme(&to_resource(TITLE, "/foo/bar/updatefile.js"), "test-custom"),
            Preferences { name: Some("My Name"), description: Some("My Description"), ..Preferences::default() },
        );

        let did_change_label_counter = count(&disposables, custom_file_input.on_did_change_label());

        assert_eq!(custom_file_input.get_name(), "My Name");
        assert_eq!(custom_file_input.get_description(None).as_deref(), Some("My Description"));

        custom_file_input.set_preferred_name("My Name 2");
        custom_file_input.set_preferred_description("My Description 2");

        assert_eq!(custom_file_input.get_name(), "My Name 2");
        assert_eq!(custom_file_input.get_description(None).as_deref(), Some("My Description 2"));

        assert_eq!(did_change_label_counter.get(), 2);

        custom_file_input.dispose();

        // Disallowed with local file input
        let file_input = create_file_input_with(
            &accessor,
            &disposables,
            &to_resource(TITLE, "/foo/bar/updatefile.js"),
            Preferences { name: Some("My Name"), description: Some("My Description"), ..Preferences::default() },
        );

        let did_change_label_counter = count(&disposables, file_input.on_did_change_label());

        assert_ne!(file_input.get_name(), "My Name");
        assert_ne!(file_input.get_description(None).as_deref(), Some("My Description"));

        file_input.set_preferred_name("My Name 2");
        file_input.set_preferred_description("My Description 2");

        assert_ne!(file_input.get_name(), "My Name 2");
        assert_ne!(file_input.get_description(None).as_deref(), Some("My Description 2"));

        assert_eq!(did_change_label_counter.get(), 0);
    });
}

#[test]
fn reports_readonly_changes() {
    const TITLE: &str = "reports readonly changes";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let input = create_file_input(&accessor, &disposables, &to_resource(TITLE, "/foo/bar/updatefile.js"));

        let listener_count = count(&disposables, input.on_did_change_capabilities());

        let model = disposables.add_rc(accessor.files().resolve_model(&input.resource, None).await.unwrap());

        assert!(matches!(model.is_readonly(), Readonly::Bool(false)));
        assert!(!input.has_capability(EditorInputCapabilities::READONLY));
        assert!(matches!(input.is_readonly(), Readonly::Bool(false)));

        let stat = accessor.file_service.resolve_with_metadata(&input.resource, Some(je_platform::files::files::ResolveFileOptions { resolve_metadata: true, ..Default::default() })).await.unwrap();

        let mut readonly_stat = stat.clone().into_stat();
        readonly_stat.readonly = Some(true);
        *accessor.file_service.read_should_throw_error.borrow_mut() =
            Some(FileOperationError::not_modified_since("file not modified since", je_platform::files::files::FileStatWithMetadata::new(readonly_stat)).into_error());
        input.resolve_file(None).await.unwrap();
        *accessor.file_service.read_should_throw_error.borrow_mut() = None;

        assert!(model.is_readonly().is_readonly());
        assert!(input.has_capability(EditorInputCapabilities::READONLY));
        assert!(input.is_readonly().is_readonly());
        assert_eq!(listener_count.get(), 1);

        let mut writable_stat = stat.clone().into_stat();
        writable_stat.readonly = Some(false);
        *accessor.file_service.read_should_throw_error.borrow_mut() =
            Some(FileOperationError::not_modified_since("file not modified since", je_platform::files::files::FileStatWithMetadata::new(writable_stat)).into_error());
        input.resolve_file(None).await.unwrap();
        *accessor.file_service.read_should_throw_error.borrow_mut() = None;

        assert!(matches!(model.is_readonly(), Readonly::Bool(false)));
        assert!(!input.has_capability(EditorInputCapabilities::READONLY));
        assert!(matches!(input.is_readonly(), Readonly::Bool(false)));
        assert_eq!(listener_count.get(), 2);
    });
}
