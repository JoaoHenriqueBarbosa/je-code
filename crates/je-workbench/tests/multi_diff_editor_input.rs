//! Porte de `src/vs/workbench/contrib/multiDiffEditor/test/browser/multiDiffEditorInput.test.ts` (suíte `MultiDiffEditorInput`), caso a
//! caso e na mesma ordem (5 definições de teste, 7 casos: o laço de `ignores stale document resolutions after ${update}` roda 3).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (o `disposables` do original é
//!   o guarda); os casos rodam em tempo real (`run_with_real_timers`) e `await` é `.await.unwrap()`.
//! - **`new MultiDiffEditorInput(source, label, resources, false, ...serviços)`.** É o [`new_input`], que chama o construtor
//!   [`Injectable::create`] com os cinco serviços, na ordem do original (`ITextModelService`, `ITextResourceConfigurationService`,
//!   `IInstantiationService`, `IMultiDiffSourceResolverService`, `ITextFileService`), e registra a entrada no guarda.
//! - **`createCodeEditorServices(disposables, services)`.** É o `create_code_editor_services` do `je-editor`, incluído por `#[path]`
//!   (o módulo `editor_support`), mais os serviços que o `createCodeEditorServices` do original registra e o do porte não
//!   ([`code_editor_services`]): o `IModelService`, o `IAccessibilityService` e o `ITextModelService` (o `InMemoryTextModelService`).
//!   O `TestDiffProviderFactoryService` entra só nos casos que o original o registra.
//! - **`mock<T>()`.** Não existe em Rust: cada dublê implementa o contrato inteiro e o membro que o original não define é
//!   `unreachable!` (o `TypeError` do original). O `mock<IInstantiationService>()` do segundo caso (nunca usado: a entrada é
//!   descartada antes de criar o view model) é um [`InstantiationService`] vazio. O `isDirty` que o original não define em nenhum
//!   `ITextFileService` e que a entrada chama ao observar o estado sujo responde `false`.
//! - **`DeferredPromise`.** É o par de `Promise::new()` (a promessa e o resolvedor).
//! - **`assert.rejects(p, CancellationError)`.** É a conferência de que a promessa rejeita com o erro de cancelamento.
//! - **`observableFromValueWithChangeEvent(input, model.documents)`.** É o `observable_from_value_with_change_event` com um dono de
//!   depuração (o dono do original só serve ao rótulo).
//! - **`for (const update of [...])`.** Cada valor do laço é um `#[test]` que chama o mesmo corpo.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de uma struct com os mesmos campos, na mesma ordem.
//! - **`documentItem.options?.hideOriginalLineNumbers`.** Os valores são [`Dynamic`] (`undefined` é `Dynamic::Undefined`).

#[allow(dead_code)]
#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code, unused_imports)]
mod editor_input_services;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

/// O suporte de editor do `je-editor/tests/support` que o `createCodeEditorServices` precisa, no mesmo módulo pai (os arquivos se
/// referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_diff_provider_factory_service;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use editor_input_services::TestAccessibilityService;
use editor_support::test_code_editor::{CodeEditorServices, create_code_editor_services};
use editor_support::test_diff_provider_factory_service::TestDiffProviderFactoryService;
use editor_support::test_text_model::instantiate_text_model;
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::VSBufferReadableStream;
use je_base::dynamic::Dynamic;
use je_base::errors::is_cancellation_error;
use je_base::event::{ConstValueWithChangeEvent, Emitter, Event, ValueWithChangeEventLike};
use je_base::lifecycle::{DisposableRef, IDisposable, Reference, plain_disposable, to_disposable};
use je_base::observable::{
    DebugOwner, IObservable, Observable, SettableExt, ValueWithChangeEventFromObservable, autorun, observable_from_value_with_change_event, observable_value,
    wait_for_state,
};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::diff_provider_factory_service::{I_DIFF_PROVIDER_FACTORY_SERVICE, IDiffProviderFactoryService};
use je_editor::model::{RelaxedTextModelCreationOptions, TextSnapshot};
use je_editor::model_service::ModelService;
use je_editor::multi_diff_editor::model::{DocumentsValue, IDocumentDiffItem};
use je_editor::services_model::{I_MODEL_SERVICE, IModelService};
use je_editor::text_model::TextModel;
use je_editor::text_resource_configuration_service::{ITextResourceConfigurationChangeEvent, ITextResourceConfigurationService};
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{I_INSTANTIATION_SERVICE, Injectable, InstantiationService, ServiceCollection};
use je_platform::accessibility::{I_ACCESSIBILITY_SERVICE, IAccessibilityService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{ConfigurationTarget, ConfigurationValue};
use je_platform::files::files::FileStatWithMetadata;
use je_workbench::common::editor::RevertOptions;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::contrib::multi_diff_editor::browser::multi_diff_editor_input::MultiDiffEditorInput;
use je_workbench::contrib::multi_diff_editor::browser::multi_diff_source_resolver_service::{
    IMultiDiffSourceResolver, IMultiDiffSourceResolverService, IResolvedMultiDiffSource, MultiDiffEditorItem, MultiDiffResources,
};
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::textfile::collaborators::FileOperationUndoRedoInfo;
use je_workbench::services::textfile::textfiles::{
    EncodedContent, IResourceEncodings, ITextFileEditorModelManager, ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions, ResolvedDecoding,
    ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileOperationError, TextFileOperationResult, TextFileSaveAsOptions, TextFileSaveOptions,
    TextFileStreamContent, TextOrSnapshot, WriteTextFileOptions,
};
use je_workbench::services::textmodel_resolver::in_memory_text_model_service::InMemoryTextModelService;
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference, I_TEXT_MODEL_SERVICE};
use je_workbench::services::untitled::untitled_text_editor_service::IUntitledTextEditorModelManager;
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;
use main_thread_support::StubTextFileService;

// region: dublês

/// O `IResolvedTextEditorModel` literal `{ textEditorModel, isReadonly() }` (o que o original não define é `undefined`/ausente).
struct ResolvedTextEditorModel {
    text_model: Option<Rc<TextModel>>,
    readonly: bool,
}

impl IDisposable for ResolvedTextEditorModel {
    fn dispose(&self) {}
}

impl IEditorModel for ResolvedTextEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        Event::none()
    }

    fn resolve(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn is_resolved(&self) -> bool {
        true
    }

    fn is_disposed(&self) -> bool {
        false
    }
}

impl ITextEditorModel for ResolvedTextEditorModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        self.text_model.clone()
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(self.readonly)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        None
    }
}

/// `{ object, dispose }`: a referência ao modelo dublê, com o descarte dado.
fn model_reference(text_model: Option<Rc<TextModel>>, readonly: bool, release: DisposableRef) -> TextModelReference {
    let object: Rc<dyn ITextEditorModel> = Rc::new(ResolvedTextEditorModel { text_model, readonly });
    Reference::new(object, release)
}

/// O `mock<ITextModelService>()` que só define `createModelReference`.
struct ClosureTextModelService(Box<dyn Fn(&URI) -> Promise<TextModelReference>>);

impl ITextModelService for ClosureTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        (self.0)(resource)
    }
}

/// O `mock<ITextModelService>()` sem membros.
fn unused_text_model_service() -> Rc<dyn ITextModelService> {
    Rc::new(ClosureTextModelService(Box::new(|_| unreachable!("createModelReference is not a function"))))
}

/// O `mock<ITextResourceConfigurationService>()` com `onDidChangeConfiguration` e `getValue`.
struct ConfigurationServiceDouble {
    on_did_change_configuration: Event<Rc<dyn ITextResourceConfigurationChangeEvent>>,
    value: Box<dyn Fn() -> Dynamic>,
}

impl ConfigurationServiceDouble {
    /// `{ onDidChangeConfiguration, getValue }`.
    fn new(on_did_change_configuration: Event<Rc<dyn ITextResourceConfigurationChangeEvent>>, value: impl Fn() -> Dynamic + 'static) -> Rc<dyn ITextResourceConfigurationService> {
        Rc::new(Self { on_did_change_configuration, value: Box::new(value) })
    }

    /// O `getValue<T>() { return {} as T }` com `onDidChangeConfiguration = Event.None`.
    fn empty() -> Rc<dyn ITextResourceConfigurationService> {
        Self::new(Event::none(), empty_object)
    }

    /// O `mock<ITextResourceConfigurationService>()` sem membros.
    fn unused() -> Rc<dyn ITextResourceConfigurationService> {
        Self::new(Event::none(), || unreachable!("getValue is not a function"))
    }
}

/// `{}`.
fn empty_object() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

impl ITextResourceConfigurationService for ConfigurationServiceDouble {
    fn on_did_change_configuration(&self) -> Event<Rc<dyn ITextResourceConfigurationChangeEvent>> {
        self.on_did_change_configuration.clone()
    }

    fn get_value(&self, _resource: Option<&URI>, _section: Option<&str>) -> Dynamic {
        (self.value)()
    }

    fn get_value_at(&self, _resource: Option<&URI>, _position: Option<&je_editor::position::Position>, _section: Option<&str>) -> Dynamic {
        unreachable!("getValue is not overloaded by the suite")
    }

    fn inspect(&self, _resource: Option<&URI>, _position: Option<&je_editor::position::Position>, _section: &str) -> Rc<dyn ConfigurationValue> {
        unreachable!("inspect is not a function")
    }

    fn update_value(&self, _resource: Option<&URI>, _key: &str, _value: Dynamic, _configuration_target: Option<ConfigurationTarget>) -> Promise<()> {
        unreachable!("updateValue is not a function")
    }
}

/// O evento `{ affectedKeys, affectsConfiguration }` literal.
struct ConfigurationChangeEvent {
    affected_keys: Vec<JsString>,
    affects_configuration: Box<dyn Fn(Option<&URI>, &str) -> bool>,
}

impl ITextResourceConfigurationChangeEvent for ConfigurationChangeEvent {
    fn affected_keys(&self) -> &[JsString] {
        &self.affected_keys
    }

    fn affects_configuration(&self, resource: Option<&URI>, section: &str) -> bool {
        (self.affects_configuration)(resource, section)
    }
}

/// O `ITextFileService` dos casos: o `mock<ITextFileService>()` com `files.onDidChangeDirty = Event.None`, o `isDirty` que responde
/// `false` e o `save` que conta as chamadas (o do último caso).
struct TestTextFileService {
    save_call_count: Rc<Cell<usize>>,
}

impl TestTextFileService {
    fn new(save_call_count: Rc<Cell<usize>>) -> Rc<dyn ITextFileService> {
        Rc::new(Self { save_call_count })
    }
}

impl ITextFileService for TestTextFileService {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        StubTextFileService.files()
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        unreachable!("untitled is not defined by the suite")
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        unreachable!("encoding is not defined by the suite")
    }

    fn is_dirty(&self, _resource: &URI) -> bool {
        false
    }

    fn save(&self, _resource: &URI, _options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        self.save_call_count.set(self.save_call_count.get() + 1);
        Promise::resolved(None)
    }

    fn save_as(&self, _resource: &URI, _target_resource: Option<&URI>, _options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        unreachable!("saveAs is not defined by the suite")
    }

    fn revert(&self, _resource: &URI, _options: Option<RevertOptions>) -> Promise<()> {
        unreachable!("revert is not defined by the suite")
    }

    fn read(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        unreachable!("read is not defined by the suite")
    }

    fn read_stream(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        unreachable!("readStream is not defined by the suite")
    }

    fn write(&self, _resource: &URI, _value: TextOrSnapshot, _options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        unreachable!("write is not defined by the suite")
    }

    fn create(&self, _operations: Vec<TextFileCreateOperation>, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        unreachable!("create is not defined by the suite")
    }

    fn get_encoded_readable(&self, _resource: Option<&URI>, _value: Option<TextOrSnapshot>, _options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        unreachable!("getEncodedReadable is not defined by the suite")
    }

    fn get_decoded_stream(
        &self,
        _resource: Option<&URI>,
        _value: VSBufferReadableStream,
        _options: Option<ReadTextFileEncodingOptions>,
    ) -> Promise<Rc<dyn je_base::stream::ReadableStream<String>>> {
        unreachable!("getDecodedStream is not defined by the suite")
    }

    fn get_encoding(&self, _resource: &URI) -> String {
        unreachable!("getEncoding is not defined by the suite")
    }

    fn resolve_decoding(&self, _resource: Option<&URI>, _options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        unreachable!("resolveDecoding is not defined by the suite")
    }

    fn resolve_encoding(&self, _resource: Option<&URI>, _options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        unreachable!("resolveEncoding is not defined by the suite")
    }

    fn validate_detected_encoding(&self, _resource: Option<&URI>, _detected_encoding: &str, _options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        unreachable!("validateDetectedEncoding is not defined by the suite")
    }
}

/// O `mock<IMultiDiffSourceResolverService>()`, com o `resolve` quando o caso o define.
struct SourceResolverService {
    resolve: Option<Box<dyn Fn() -> Promise<Option<Rc<dyn IResolvedMultiDiffSource>>>>>,
}

impl SourceResolverService {
    fn resolving(resolve: impl Fn() -> Promise<Option<Rc<dyn IResolvedMultiDiffSource>>> + 'static) -> Rc<dyn IMultiDiffSourceResolverService> {
        Rc::new(Self { resolve: Some(Box::new(resolve)) })
    }

    fn unused() -> Rc<dyn IMultiDiffSourceResolverService> {
        Rc::new(Self { resolve: None })
    }
}

impl IMultiDiffSourceResolverService for SourceResolverService {
    fn register_resolver(&self, _resolver: Rc<dyn IMultiDiffSourceResolver>) -> DisposableRef {
        unreachable!("registerResolver is not a function")
    }

    fn resolve(&self, _uri: &URI) -> Promise<Option<Rc<dyn IResolvedMultiDiffSource>>> {
        match &self.resolve {
            Some(resolve) => resolve(),
            None => unreachable!("resolve is not a function"),
        }
    }
}

/// O `IResolvedMultiDiffSource` literal `{ resources, label? }`.
struct ResolvedSource {
    resources: MultiDiffResources,
    label: Option<Rc<dyn ValueWithChangeEventLike<String>>>,
}

impl IResolvedMultiDiffSource for ResolvedSource {
    fn resources(&self) -> MultiDiffResources {
        self.resources.clone()
    }

    fn label(&self) -> Option<Rc<dyn ValueWithChangeEventLike<String>>> {
        self.label.clone()
    }
}

// endregion

// region: montagem

/// Os cinco serviços decorados do construtor de `MultiDiffEditorInput`, na ordem do original.
struct InputServices {
    text_model_service: Rc<dyn ITextModelService>,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    instantiation_service: Rc<InstantiationService>,
    multi_diff_source_resolver_service: Rc<dyn IMultiDiffSourceResolverService>,
    text_file_service: Rc<dyn ITextFileService>,
}

/// `disposables.add(new MultiDiffEditorInput(URI.parse('multi-diff-editor:test'), label, resources, false, ...services))`.
fn new_input(disposables: &LeakGuard, label: &str, initial_resources: Option<Vec<MultiDiffEditorItem>>, services: InputServices) -> Rc<MultiDiffEditorInput> {
    let InputServices {
        text_model_service,
        text_resource_configuration_service,
        instantiation_service,
        multi_diff_source_resolver_service,
        text_file_service,
    } = services;
    disposables.add_rc(MultiDiffEditorInput::create(
        (URI::parse("multi-diff-editor:test", false), Some(label.to_string()), initial_resources, false),
        (text_model_service, text_resource_configuration_service, instantiation_service, multi_diff_source_resolver_service, text_file_service),
    ))
}

/// `createCodeEditorServices(disposables, services)` com os serviços que o original registra por padrão e o do porte não.
fn code_editor_services(disposables: &LeakGuard, with_diff_provider: bool) -> Rc<CodeEditorServices> {
    let test_configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;
    let model_service: Rc<dyn IModelService> = disposables.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service),
        UndoRedoService::for_tests(),
    ));

    let collection = ServiceCollection::new();
    collection.set(I_MODEL_SERVICE, model_service.clone());
    collection.set(I_ACCESSIBILITY_SERVICE, Rc::new(TestAccessibilityService) as Rc<dyn IAccessibilityService>);
    collection.set(I_TEXT_MODEL_SERVICE, InMemoryTextModelService::create((), (model_service,)) as Rc<dyn ITextModelService>);
    if with_diff_provider {
        collection.set(I_DIFF_PROVIDER_FACTORY_SERVICE, Rc::new(TestDiffProviderFactoryService) as Rc<dyn IDiffProviderFactoryService>);
    }

    create_code_editor_services(&disposables.store(), Some(collection))
}

/// O `IInstantiationService` do `createCodeEditorServices`.
fn instantiation_service_of(services: &CodeEditorServices) -> Rc<InstantiationService> {
    services.get(I_INSTANTIATION_SERVICE)
}

/// `new MultiDiffEditorItem(original, modified, undefined)`.
fn item(original: Option<URI>, modified: Option<URI>) -> MultiDiffEditorItem {
    MultiDiffEditorItem::new(original, modified, None, None, None)
}

// endregion

#[test]
fn updates_its_name_from_the_resolved_source_label() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let source_label = observable_value::<String, ()>("sourceLabel", "Current Turn Changes".to_string());
        let label_observable: IObservable<String> = source_label.clone();
        let source_resolver_service = SourceResolverService::resolving(move || {
            let label: Rc<dyn ValueWithChangeEventLike<String>> = Rc::new(ValueWithChangeEventFromObservable { observable: label_observable.clone() });
            let source: Rc<dyn IResolvedMultiDiffSource> =
                Rc::new(ResolvedSource { resources: Rc::new(ConstValueWithChangeEvent::new(Rc::new(Vec::<MultiDiffEditorItem>::new()))), label: Some(label) });
            Promise::resolved(Some(source))
        });
        let services = code_editor_services(&disposables, false);
        let input = new_input(
            &disposables,
            "Fallback",
            None,
            InputServices {
                text_model_service: unused_text_model_service(),
                text_resource_configuration_service: ConfigurationServiceDouble::unused(),
                instantiation_service: instantiation_service_of(&services),
                multi_diff_source_resolver_service: source_resolver_service,
                text_file_service: TestTextFileService::new(Rc::default()),
            },
        );
        input.get_view_model().await.unwrap();

        let names = Rc::new(RefCell::new(vec![input.get_name()]));
        let (listener_names, listener_input) = (names.clone(), Rc::downgrade(&input));
        disposables.add(input.on_did_change_label().listen(move |_| {
            if let Some(input) = listener_input.upgrade() {
                listener_names.borrow_mut().push(input.get_name());
            }
        }));
        source_label.set("Last Turn Changes".to_string(), None);

        assert_eq!(*names.borrow(), vec!["Current Turn Changes (0 files)".to_string(), "Last Turn Changes (0 files)".to_string()]);
    });
}

#[test]
fn disposes_models_that_finish_resolving_after_input_disposal() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let (reference_requested, reference_requested_resolver) = Promise::<()>::new();
        let (reference_result, reference_result_resolver) = Promise::<TextModelReference>::new();
        let reference_disposed = Rc::new(Cell::new(false));
        let text_model_service: Rc<dyn ITextModelService> = Rc::new(ClosureTextModelService(Box::new(move |_| {
            reference_requested_resolver.resolve(());
            reference_result.clone()
        })));
        let input = new_input(
            &disposables,
            "Test",
            Some(vec![item(None, Some(URI::parse("file:///modified.ts", false)))]),
            InputServices {
                text_model_service,
                text_resource_configuration_service: ConfigurationServiceDouble::unused(),
                instantiation_service: disposables.add_rc(InstantiationService::new(ServiceCollection::new())),
                multi_diff_source_resolver_service: SourceResolverService::unused(),
                text_file_service: TestTextFileService::new(Rc::default()),
            },
        );

        let view_model_promise = input.get_view_model();
        reference_requested.await.unwrap();
        input.dispose();
        let flag = reference_disposed.clone();
        reference_result_resolver.resolve(model_reference(None, false, plain_disposable(move || flag.set(true))));

        let error = view_model_promise.await.err().expect("a promessa do view model rejeita");
        assert!(is_cancellation_error(&error), "{error}");
        assert!(reference_disposed.get());
    });
}

/// O que `update` faz com os recursos depois que a resolução lenta começou.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Update {
    Replace,
    Remove,
    Dispose,
}

/// O corpo de `ignores stale document resolutions after ${update}`.
fn ignores_stale_document_resolutions_after(update: Update) {
    run_with_real_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let services = code_editor_services(&disposables, true);
        let create_resource = |name: &str| {
            item(Some(URI::parse(format!("inmemory:/original/{name}.ts").as_str(), false)), Some(URI::parse(format!("inmemory:/modified/{name}.ts").as_str(), false)))
        };
        let removed = create_resource("removed");
        let slow = create_resource("slow");
        let current = create_resource("current");
        let resources = observable_value::<Rc<Vec<MultiDiffEditorItem>>, ()>("resources", Rc::new(vec![removed.clone()]));
        let (slow_reference_requested, slow_reference_requested_resolver) = Promise::<()>::new();
        let (slow_reference_ready, slow_reference_ready_resolver) = Promise::<()>::new();
        let released_references: Rc<RefCell<Vec<String>>> = Rc::default();

        let (store, undo_redo_service, released) = (disposables.store(), services.undo_redo_service.clone(), released_references.clone());
        let text_model_service: Rc<dyn ITextModelService> = Rc::new(ClosureTextModelService(Box::new(move |resource: &URI| {
            let (resource, store, undo_redo_service, released) = (resource.clone(), store.clone(), undo_redo_service.clone(), released.clone());
            let (slow_reference_requested_resolver, slow_reference_ready) = (slow_reference_requested_resolver.clone(), slow_reference_ready.clone());
            async_fn(async move {
                let path = resource.path().to_string_lossy();
                if path.ends_with("/slow.ts") {
                    slow_reference_requested_resolver.resolve(());
                    slow_reference_ready.await?;
                }
                let model = store.add_rc(instantiate_text_model(undo_redo_service, path.clone(), None, RelaxedTextModelCreationOptions::default(), Some(resource)));
                let reference = store.add(to_disposable({
                    let model = model.clone();
                    move || {
                        released.borrow_mut().push(path);
                        model.dispose();
                    }
                }));
                Ok(model_reference(Some(model), true, plain_disposable(move || reference.dispose())))
            })
        })));
        let resources_observable: IObservable<Rc<Vec<MultiDiffEditorItem>>> = resources.clone();
        let source_resolver_service = SourceResolverService::resolving(move || {
            let source: Rc<dyn IResolvedMultiDiffSource> = Rc::new(ResolvedSource {
                resources: Rc::new(ValueWithChangeEventFromObservable { observable: resources_observable.clone() }),
                label: None,
            });
            Promise::resolved(Some(source))
        });
        let input = new_input(
            &disposables,
            "Test",
            None,
            InputServices {
                text_model_service,
                text_resource_configuration_service: ConfigurationServiceDouble::empty(),
                instantiation_service: instantiation_service_of(&services),
                multi_diff_source_resolver_service: source_resolver_service,
                text_file_service: TestTextFileService::new(Rc::default()),
            },
        );
        let view_model = input.get_view_model().await.unwrap();
        let documents = observable_from_value_with_change_event(DebugOwner::new("test"), view_model.model.documents());
        let published_documents: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
        let (observed_documents, published) = (documents.clone(), published_documents.clone());
        disposables.add(autorun(move |reader| {
            if let DocumentsValue::Items(items) = observed_documents.read(reader) {
                published.borrow_mut().push(items.iter().map(|document| modified_path(document.object().as_ref())).collect());
            }
        }));

        resources.set(Rc::new(vec![removed.clone(), slow.clone()]), None);
        slow_reference_requested.await.unwrap();
        if update == Update::Dispose {
            input.dispose();
        } else {
            resources.set(Rc::new(if update == Update::Replace { vec![current.clone()] } else { Vec::new() }), None);
            let current_path = current.modified_uri.as_ref().expect("o recurso atual tem o lado modificado").path().to_string_lossy();
            wait_for_state(
                documents.clone(),
                move |value| match value {
                    DocumentsValue::Loading => false,
                    DocumentsValue::Items(items) if update == Update::Replace => items.len() == 1 && modified_path_if_any(items[0].object().as_ref()) == Some(current_path.clone()),
                    DocumentsValue::Items(items) => items.is_empty(),
                },
                None,
                None,
            )
            .await
            .unwrap();
            view_model.wait_for_diff_or_1s().await.unwrap();
        }

        slow_reference_ready_resolver.resolve(());
        timeout(0).await.unwrap();

        let mut released_references = released_references.borrow().clone();
        released_references.sort();
        let mut expected_published = vec![vec!["/modified/removed.ts".to_string()]];
        if update != Update::Dispose {
            expected_published.push(if update == Update::Replace { vec!["/modified/current.ts".to_string()] } else { Vec::new() });
        }
        assert_eq!(
            (published_documents.borrow().clone(), released_references),
            (
                expected_published,
                vec!["/modified/removed.ts".to_string(), "/modified/slow.ts".to_string(), "/original/removed.ts".to_string(), "/original/slow.ts".to_string()],
            )
        );
    });
}

/// `document.object.modified!.uri.path`.
fn modified_path(document: &dyn IDocumentDiffItem) -> String {
    modified_path_if_any(document).expect("o documento tem o lado modificado")
}

/// `document.object.modified?.uri.path`.
fn modified_path_if_any(document: &dyn IDocumentDiffItem) -> Option<String> {
    document.modified().map(|modified| modified.uri.path().to_string_lossy())
}

#[test]
fn ignores_stale_document_resolutions_after_replace() {
    ignores_stale_document_resolutions_after(Update::Replace);
}

#[test]
fn ignores_stale_document_resolutions_after_remove() {
    ignores_stale_document_resolutions_after(Update::Remove);
}

#[test]
fn ignores_stale_document_resolutions_after_dispose() {
    ignores_stale_document_resolutions_after(Update::Dispose);
}

#[test]
fn preserves_explicit_original_line_number_setting_values_and_updates() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let original_uri = URI::parse("file:///original.ts", false);
        let modified_uri = URI::parse("file:///modified.ts", false);
        let text_model_service: Rc<dyn ITextModelService> = Rc::new(ClosureTextModelService(Box::new(|_| {
            Promise::rejected(TextFileOperationError::new("binary", TextFileOperationResult::FileIsBinary))
        })));
        let configuration_changed: Rc<Emitter<Rc<dyn ITextResourceConfigurationChangeEvent>>> = disposables.add_rc(Emitter::new());
        let hide_original_line_numbers: Rc<RefCell<Dynamic>> = Rc::new(RefCell::new(Dynamic::Null));
        let configured = hide_original_line_numbers.clone();
        let text_resource_configuration_service = ConfigurationServiceDouble::new(configuration_changed.event(), move || {
            Dynamic::object([("editor", empty_object()), ("diffEditor", Dynamic::object([("hideOriginalLineNumbers", configured.borrow().clone())]))])
        });
        let services = code_editor_services(&disposables, true);
        let input = new_input(
            &disposables,
            "Test",
            Some(vec![item(Some(original_uri), Some(modified_uri))]),
            InputServices {
                text_model_service,
                text_resource_configuration_service,
                instantiation_service: instantiation_service_of(&services),
                multi_diff_source_resolver_service: SourceResolverService::unused(),
                text_file_service: TestTextFileService::new(Rc::default()),
            },
        );
        let view_model = input.get_view_model().await.unwrap();
        let document_item = view_model.items.get()[0].document_diff_item().clone();
        let hide_original_line_numbers_of = |document_item: &Rc<dyn IDocumentDiffItem>| {
            document_item.options().map_or(Dynamic::Undefined, |options| options.get_str("hideOriginalLineNumbers"))
        };
        let values = Rc::new(RefCell::new(vec![hide_original_line_numbers_of(&document_item)]));
        let (listener_values, listener_item) = (values.clone(), document_item.clone());
        disposables.add(
            document_item
                .on_options_did_change()
                .expect("o item do multi diff observa as opções")
                .listen(move |_| listener_values.borrow_mut().push(hide_original_line_numbers_of(&listener_item))),
        );

        let fire = |hide: Dynamic| {
            *hide_original_line_numbers.borrow_mut() = hide;
            configuration_changed.fire(Rc::new(ConfigurationChangeEvent {
                affected_keys: vec![JsString::from("diffEditor.hideOriginalLineNumbers")],
                affects_configuration: Box::new(|_resource, section| section == "diffEditor"),
            }));
        };
        fire(Dynamic::Bool(false));
        fire(Dynamic::Bool(true));
        fire(Dynamic::Null);

        assert_eq!(*values.borrow(), vec![Dynamic::Undefined, Dynamic::Bool(false), Dynamic::Bool(true), Dynamic::Undefined]);
    });
}

/// O objeto que o `deepStrictEqual` de `keeps binary resources in the multi diff model` compara.
#[derive(Debug, PartialEq, Eq)]
struct BinaryItemObservation {
    item_count: usize,
    original_uri: Option<String>,
    modified_uri: Option<String>,
    is_binary: bool,
    original_source_uri: Option<String>,
    modified_source_uri: Option<String>,
    has_original_text_model: bool,
    has_modified_text_model: bool,
    save_call_count: usize,
}

#[test]
fn keeps_binary_resources_in_the_multi_diff_model() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let original_uri = URI::parse("file:///original.png", false);
        let modified_uri = URI::parse("file:///modified.png", false);
        let text_model_service: Rc<dyn ITextModelService> = Rc::new(ClosureTextModelService(Box::new(|_| {
            Promise::rejected(TextFileOperationError::new("binary", TextFileOperationResult::FileIsBinary))
        })));
        let save_call_count = Rc::new(Cell::new(0));
        let services = code_editor_services(&disposables, true);
        let input = new_input(
            &disposables,
            "Test",
            Some(vec![item(Some(original_uri.clone()), Some(modified_uri.clone()))]),
            InputServices {
                text_model_service,
                text_resource_configuration_service: ConfigurationServiceDouble::empty(),
                instantiation_service: instantiation_service_of(&services),
                multi_diff_source_resolver_service: SourceResolverService::unused(),
                text_file_service: TestTextFileService::new(save_call_count.clone()),
            },
        );

        let view_model = input.get_view_model().await.unwrap();
        let items = view_model.items.get();
        let item = &items[0];
        input.save(1, None).await.unwrap();

        let uri_text = |uri: Option<URI>| uri.map(|uri| uri.to_string(false).to_string_lossy());
        assert_eq!(
            BinaryItemObservation {
                item_count: view_model.items.get().len(),
                original_uri: uri_text(item.original_uri()),
                modified_uri: uri_text(item.modified_uri()),
                is_binary: item.is_binary(),
                original_source_uri: uri_text(item.document_diff_item().original().map(|source| source.uri.clone())),
                modified_source_uri: uri_text(item.document_diff_item().modified().map(|source| source.uri.clone())),
                has_original_text_model: item.document_diff_item().original().is_some_and(|source| source.text_model.is_some()),
                has_modified_text_model: item.document_diff_item().modified().is_some_and(|source| source.text_model.is_some()),
                save_call_count: save_call_count.get(),
            },
            BinaryItemObservation {
                item_count: 1,
                original_uri: uri_text(Some(original_uri.clone())),
                modified_uri: uri_text(Some(modified_uri.clone())),
                is_binary: true,
                original_source_uri: uri_text(Some(original_uri)),
                modified_source_uri: uri_text(Some(modified_uri)),
                has_original_text_model: false,
                has_modified_text_model: false,
                save_call_count: 0,
            }
        );
    });
}
