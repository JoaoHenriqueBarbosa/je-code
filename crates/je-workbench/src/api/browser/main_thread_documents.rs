//! Porte de `src/vs/workbench/api/browser/mainThreadDocuments.ts`: o [`BoundModelReferenceCollection`] (a coleção que
//! segura as referências de modelo que o host abriu e as solta por idade, por tamanho total e por quantidade), o
//! `ModelTracker` e o [`MainThreadDocuments`] (o lado do workbench do RPC `ExtHostDocuments`).
//!
//! Divergências:
//! - `IReference<unknown>` só tem o `dispose` lido (o `object` nunca é usado), então `add` recebe a [`DisposableRef`]
//!   da referência.
//! - `dispose()` do original é `dispose(this._data)`, que itera o array enquanto cada entrada se remove dele (e pula
//!   entradas alternadas). Aqui o descarte é de uma cópia das entradas, que é o que a intenção pede: nenhuma entrada
//!   fica com o timer vivo.
//! - Os parâmetros opcionais do construtor (`maxAge`, `maxLength`, `maxSize`) são os de [`BoundModelReferenceLimits`], com
//!   o `Default` com os valores do original.
//! - `setTimeout` e `clearTimeout` são os de `je_base::runtime` (o relógio virtual dos testes os controla).
//! - **`MainThreadDocuments`.** O construtor `(extHostContext, @IModelService, ...)` é [`MainThreadDocuments::new`] com os
//!   nove argumentos na ordem do original (o `IExtHostContext` é o [`IMainContext`], que dá o `getProxy`). O
//!   `IModelService` é o [`ModelService`] concreto: o trait do porte não tem `onModelLanguageChanged`. Os eventos
//!   `onDidSave`, `onDidChangeDirty` e `onDidChangeEncoding` do `files` (e o `untitled.onDidChangeEncoding`) chegam por
//!   `ITextFileService::files()` e `untitled()`; o `Event.any` dos dois de encoding vira dois ouvintes com o mesmo corpo,
//!   porque os modelos têm tipos diferentes.
//! - **`ModelTracker`.** `ISerializedModelContentChangedEvent.detailedReason` é um objeto [`Dynamic`] com as entradas do
//!   `metadata` da primeira origem de edição (`undefined` vira ausente). O `onUnexpectedError(new Error(...))` do original,
//!   quando `detailedReasonsChangeLengths.length !== 1`, é o mesmo.
//! - **Métodos `async`** (`$tryOpenDocument`, `_handleAsResourceInput`...) são [`async_fn`], que roda de forma síncrona até
//!   o primeiro `await`, como o original. O `Boolean(target)` de `$trySaveDocument` é `target.is_some()`.
//! - `Event.once(model.onDidRevert)(...)` é um [`Event::once`] cujo ouvinte é descartado junto com o `MainThreadDocuments`.
//! - `@extHostCustomer`/`extHostContext.set(...)` do registro não existem (ver `main_thread_documents_and_editors.rs`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::errors::{on_unexpected_error, Error};
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::resources::{to_local_resource, ExtUri};
use je_base::runtime::{clear_timeout, set_timeout, TimerId};
use je_base::uri::URI;
use je_editor::language_feature_registry::should_synchronize_model;
use je_editor::model_service::{ModelLanguageChanged, ModelService};
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_editor::text_model_events::ModelContentChangedEvent;
use je_platform::files::files::FileOperation;
use je_platform::files::i_file_service::IFileService;
use je_platform::path::path_service::IPathService;
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use crate::api::common::ext_host_protocol::{
    CreateDocumentOptions, ExtHostContext, ExtHostDocumentsShape, IMainContext, MainThreadDocumentsShape, OpenDocumentOptions,
    SerializedModelContentChangedEvent,
};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::textfile::collaborators::IWorkingCopyFileService;
use crate::services::textfile::textfiles::{EncodingMode, ITextFileService, TextFileEditorModelResolveOrCreateOptions, TextFileResolveOptions, TextFileResolveReason};
use crate::services::textmodel_resolver::resolver_service::ITextModelService;
use crate::services::untitled::untitled_text_editor_service::{AssociatedResource, UntitledTextEditorOptions};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// Os limites do construtor: `_maxAge`, `_maxLength` e `_maxSize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundModelReferenceLimits {
    /// Descarta pela idade, em milissegundos.
    pub max_age: u64,
    /// Descarta pelo tamanho total.
    pub max_length: usize,
    /// Descarta pelo número de referências.
    pub max_size: usize,
}

impl Default for BoundModelReferenceLimits {
    fn default() -> Self {
        Self { max_age: 1000 * 60 * 3, max_length: 1024 * 1024 * 80, max_size: 50 }
    }
}

/// O elemento de `_data`: `{ uri, length, dispose }`.
struct Entry {
    id: u64,
    uri: URI,
    length: usize,
    reference: DisposableRef,
    handle: Cell<Option<TimerId>>,
}

#[derive(Default)]
struct State {
    data: Vec<Rc<Entry>>,
    length: usize,
    next_id: u64,
}

/// `entry.dispose`: tira a entrada de `_data` (se ainda estiver lá), solta a referência e cancela o timer.
fn dispose_entry(state: &RefCell<State>, entry: &Entry) {
    let present = state.borrow().data.iter().any(|candidate| candidate.id == entry.id);
    if !present {
        return;
    }
    state.borrow_mut().length -= entry.length;
    entry.reference.dispose();
    if let Some(handle) = entry.handle.take() {
        clear_timeout(handle);
    }
    state.borrow_mut().data.retain(|candidate| candidate.id != entry.id);
}

/// `BoundModelReferenceCollection`.
pub struct BoundModelReferenceCollection {
    ext_uri: ExtUri,
    limits: BoundModelReferenceLimits,
    state: Rc<RefCell<State>>,
}

impl BoundModelReferenceCollection {
    pub fn new(ext_uri: ExtUri, limits: BoundModelReferenceLimits) -> Self {
        Self { ext_uri, limits, state: Rc::default() }
    }

    /// `remove`.
    pub fn remove(&self, uri: &URI) {
        // copy the entries because dispose will modify the list
        let entries: Vec<Rc<Entry>> = self.state.borrow().data.clone();
        for entry in entries {
            if self.ext_uri.is_equal_or_parent(&entry.uri, uri, false, false) {
                dispose_entry(&self.state, &entry);
            }
        }
    }

    /// `add`: `length` é o `length = 0` do original quando omitido.
    pub fn add(&self, uri: URI, reference: DisposableRef, length: usize) {
        let entry = {
            let mut state = self.state.borrow_mut();
            state.next_id += 1;
            Rc::new(Entry { id: state.next_id, uri, length, reference, handle: Cell::new(None) })
        };
        let timer_state = self.state.clone();
        let timer_entry = entry.clone();
        entry.handle.set(Some(set_timeout(move || dispose_entry(&timer_state, &timer_entry), self.limits.max_age)));

        {
            let mut state = self.state.borrow_mut();
            state.data.push(entry);
            state.length += length;
        }
        self.cleanup();
    }

    fn cleanup(&self) {
        // clean-up wrt total length
        while self.state.borrow().length > self.limits.max_length {
            let first = self.state.borrow().data[0].clone();
            dispose_entry(&self.state, &first);
        }
        // clean-up wrt number of documents
        let extra_size = (self.limits.max_size as f64 * 1.2).ceil() as usize;
        let count = self.state.borrow().data.len();
        if count >= extra_size {
            let extra: Vec<Rc<Entry>> = self.state.borrow().data[..extra_size - self.limits.max_size].to_vec();
            for entry in extra {
                dispose_entry(&self.state, &entry);
            }
        }
    }
}

impl IDisposable for BoundModelReferenceCollection {
    fn dispose(&self) {
        let entries: Vec<Rc<Entry>> = self.state.borrow().data.clone();
        for entry in entries {
            dispose_entry(&self.state, &entry);
        }
    }
}

/// `ModelTracker`.
struct ModelTracker {
    base: Rc<Disposable>,
    model: Rc<TextModel>,
    known_version_id: Cell<i64>,
}

impl ModelTracker {
    fn new(
        model: Rc<TextModel>,
        on_is_caught_up_with_content_changes: Rc<Emitter<URI>>,
        proxy: Rc<dyn ExtHostDocumentsShape>,
        text_file_service: Rc<dyn ITextFileService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<ModelTracker>| {
            let base = Disposable::new();
            let tracker = me.clone();
            base.register(model.on_did_change_content().listen(move |e| {
                let Some(tracker) = tracker.upgrade() else {
                    return;
                };
                tracker.known_version_id.set(e.version_id);
                if e.detailed_reasons_change_lengths.len() != 1 {
                    let reasons: Vec<&str> = e.detailed_reasons.iter().map(|reason| reason.to_string_value()).collect();
                    on_unexpected_error(&Error::new(format!("Unexpected reasons: {}", reasons.join(","))));
                }

                let uri = tracker.model.uri();
                let event = serialize_content_change(e);
                proxy.rpc_accept_model_changed(&uri, event, text_file_service.is_dirty(&uri));
                if tracker.is_caught_up_with_content_changes() {
                    on_is_caught_up_with_content_changes.fire(uri);
                }
            }));
            Self { base, known_version_id: Cell::new(model.get_version_id()), model }
        })
    }

    fn is_caught_up_with_content_changes(&self) -> bool {
        self.model.get_version_id() == self.known_version_id.get()
    }
}

impl IDisposable for ModelTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O `ISerializedModelContentChangedEvent` de um `IModelContentChangedEvent`: `detailedReason` é o `metadata` da
/// primeira origem de edição.
fn serialize_content_change(e: &ModelContentChangedEvent) -> SerializedModelContentChangedEvent {
    let detailed_reason = e.detailed_reasons.first().map(|reason| {
        Dynamic::object(reason.metadata.iter().map(|(key, value)| {
            (key.clone(), value.as_ref().map_or(Dynamic::Undefined, |value| Dynamic::String(JsString::from(value.clone()))))
        }))
    });
    SerializedModelContentChangedEvent {
        changes: e.changes.clone(),
        eol: e.eol,
        version_id: e.version_id,
        is_undoing: e.is_undoing,
        is_redoing: e.is_redoing,
        is_flush: e.is_flush,
        is_eol_change: e.is_eol_change,
        detailed_reason,
    }
}

/// `MainThreadDocuments`.
pub struct MainThreadDocuments {
    me: Weak<MainThreadDocuments>,
    base: Rc<Disposable>,
    on_is_caught_up_with_content_changes: Rc<Emitter<URI>>,
    proxy: Rc<dyn ExtHostDocumentsShape>,
    model_trackers: RefCell<ResourceMap<Rc<ModelTracker>>>,
    model_reference_collection: Rc<BoundModelReferenceCollection>,
    model_service: Rc<ModelService>,
    text_file_service: Rc<dyn ITextFileService>,
    file_service: Rc<dyn IFileService>,
    text_model_resolver_service: Rc<dyn ITextModelService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    path_service: Rc<dyn IPathService>,
}

impl MainThreadDocuments {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ext_host_context: &Rc<dyn IMainContext>,
        model_service: Rc<ModelService>,
        text_file_service: Rc<dyn ITextFileService>,
        file_service: Rc<dyn IFileService>,
        text_model_resolver_service: Rc<dyn ITextModelService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
        path_service: Rc<dyn IPathService>,
    ) -> Rc<Self> {
        let base = Disposable::new();

        let on_is_caught_up_with_content_changes: Rc<Emitter<URI>> = Emitter::new();
        base.register(on_is_caught_up_with_content_changes.clone());

        let model_reference_collection = Rc::new(BoundModelReferenceCollection::new(uri_identity_service.ext_uri(), BoundModelReferenceLimits::default()));
        base.register(model_reference_collection.clone());

        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_DOCUMENTS);

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            on_is_caught_up_with_content_changes,
            proxy,
            model_trackers: RefCell::new(ResourceMap::new()),
            model_reference_collection,
            model_service,
            text_file_service,
            file_service,
            text_model_resolver_service,
            environment_service,
            uri_identity_service,
            path_service,
        });

        let weak = this.me.clone();
        this.base.register(this.model_service.on_model_language_changed().listen({
            let weak = weak.clone();
            move |event| {
                if let Some(this) = weak.upgrade() {
                    this.on_model_mode_changed(event);
                }
            }
        }));

        let files = this.text_file_service.files();
        this.base.register(files.on_did_save().listen({
            let weak = weak.clone();
            move |e| {
                if let Some(this) = weak.upgrade() {
                    let resource = e.model.resource();
                    if this.should_handle_file_event(&resource) {
                        this.proxy.rpc_accept_model_saved(&resource);
                    }
                }
            }
        }));
        this.base.register(files.on_did_change_dirty().listen({
            let weak = weak.clone();
            move |m| {
                if let Some(this) = weak.upgrade() {
                    let resource = m.resource();
                    if this.should_handle_file_event(&resource) {
                        this.proxy.rpc_accept_dirty_state_changed(&resource, m.is_dirty());
                    }
                }
            }
        }));
        this.base.register(files.on_did_change_encoding().listen({
            let weak = weak.clone();
            move |m| {
                if let Some(this) = weak.upgrade() {
                    this.accept_encoding_changed(&m.resource(), m.get_encoding());
                }
            }
        }));
        this.base.register(this.text_file_service.untitled().on_did_change_encoding().listen({
            let weak = weak.clone();
            move |m| {
                if let Some(this) = weak.upgrade() {
                    this.accept_encoding_changed(&m.resource(), m.get_encoding());
                }
            }
        }));

        this.base.register(working_copy_file_service.on_did_run_working_copy_file_operation().listen({
            let weak = weak.clone();
            move |e| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let is_move = e.operation == FileOperation::Move;
                if is_move || e.operation == FileOperation::Delete {
                    for pair in &e.files {
                        let removed = if is_move { pair.source.as_ref() } else { Some(&pair.target) };
                        if let Some(removed) = removed {
                            this.model_reference_collection.remove(removed);
                        }
                    }
                }
            }
        }));

        this
    }

    /// O `this` do original, para os métodos `async` que o seguram.
    fn this(&self) -> Rc<MainThreadDocuments> {
        self.me.upgrade().expect("MainThreadDocuments descartado")
    }

    /// O corpo comum dos dois ouvintes de `Event.any(files.onDidChangeEncoding, untitled.onDidChangeEncoding)`.
    fn accept_encoding_changed(&self, resource: &URI, encoding: Option<String>) {
        if self.should_handle_file_event(resource) {
            if let Some(encoding) = encoding.filter(|encoding| !encoding.is_empty()) {
                self.proxy.rpc_accept_encoding_changed(resource, &encoding);
            }
        }
    }

    /// `onIsCaughtUpWithContentChanges`.
    pub fn on_is_caught_up_with_content_changes(&self) -> Event<URI> {
        self.on_is_caught_up_with_content_changes.event()
    }

    /// `isCaughtUpWithContentChanges(resource)`.
    pub fn is_caught_up_with_content_changes(&self, resource: &URI) -> bool {
        let tracker = self.model_trackers.borrow().get(resource).cloned();
        match tracker {
            Some(tracker) => tracker.is_caught_up_with_content_changes(),
            None => true,
        }
    }

    fn should_handle_file_event(&self, resource: &URI) -> bool {
        self.model_service.get_model(resource).is_some_and(|model| should_synchronize_model(&model))
    }

    /// `handleModelAdded(model)`.
    pub fn handle_model_added(&self, model: &Rc<TextModel>) {
        // Same filter as in mainThreadEditorsTracker
        if !should_synchronize_model(model) {
            // don't synchronize too large models
            return;
        }
        let tracker = ModelTracker::new(
            model.clone(),
            self.on_is_caught_up_with_content_changes.clone(),
            self.proxy.clone(),
            self.text_file_service.clone(),
        );
        self.model_trackers.borrow_mut().set(model.uri(), tracker);
    }

    fn on_model_mode_changed(&self, event: &ModelLanguageChanged) {
        let model = &event.model;
        if !self.model_trackers.borrow().has(&model.uri()) {
            return;
        }
        self.proxy.rpc_accept_model_language_changed(&model.uri(), &model.get_language_id());
    }

    /// `handleModelRemoved(modelUrl)`.
    pub fn handle_model_removed(&self, model_url: &URI) {
        let tracker = self.model_trackers.borrow().get(model_url).cloned();
        let Some(tracker) = tracker else {
            return;
        };
        tracker.dispose();
        self.model_trackers.borrow_mut().delete(model_url);
    }

    // --- from extension host process

    /// `_handleAsResourceInput(uri, options)`.
    fn handle_as_resource_input(&self, uri: URI, options: Option<OpenDocumentOptions>) -> Promise<URI> {
        let this = self.this();
        async_fn(async move {
            if let Some(encoding) = options.and_then(|options| options.encoding).filter(|encoding| !encoding.is_empty()) {
                let resolve = TextFileEditorModelResolveOrCreateOptions {
                    resolve: TextFileResolveOptions { reason: Some(TextFileResolveReason::Reference), ..TextFileResolveOptions::default() },
                    encoding: Some(encoding.clone()),
                    ..TextFileEditorModelResolveOrCreateOptions::default()
                };
                let model = this.text_file_service.files().resolve(&uri, Some(resolve)).await?;
                if model.is_dirty() {
                    return Err(Error::no_telemetry("Cannot re-open a dirty text document with different encoding. Save it first."));
                }
                model.set_encoding(&encoding, EncodingMode::Decode).await?;
            }

            let reference = this.text_model_resolver_service.create_model_reference(&uri).await?;
            let text_editor_model = reference.object.text_editor_model().ok_or_else(|| Error::new("o modelo da referência não foi resolvido"))?;
            let length = text_editor_model.get_value_length();
            let reference_disposable: DisposableRef = Rc::new(reference);
            this.model_reference_collection.add(uri, reference_disposable, length);
            Ok(text_editor_model.uri())
        })
    }

    /// `_handleUntitledScheme(uri, options)`.
    fn handle_untitled_scheme(&self, uri: URI, options: Option<OpenDocumentOptions>) -> Promise<URI> {
        let this = self.this();
        async_fn(async move {
            let as_local_uri = to_local_resource(&uri, this.environment_service.remote_authority().as_deref(), &this.path_service.default_uri_scheme());
            let exists = this.file_service.exists(&as_local_uri).await?;
            if exists {
                // don't create a new file ontop of an existing file
                return Err(Error::new("file already exists"));
            }
            let associated_resource = if uri.path().is_empty() { None } else { Some(uri.clone()) };
            let options = CreateDocumentOptions { encoding: options.and_then(|options| options.encoding), ..CreateDocumentOptions::default() };
            this.do_create_untitled(associated_resource, Some(options)).await
        })
    }

    /// `_doCreateUntitled(associatedResource?, options?)`.
    fn do_create_untitled(&self, associated_resource: Option<URI>, options: Option<CreateDocumentOptions>) -> Promise<URI> {
        let this = self.this();
        async_fn(async move {
            let options = options.unwrap_or_default();
            let model = this.text_file_service.untitled().create(Some(UntitledTextEditorOptions {
                associated_resource: associated_resource.as_ref().map(AssociatedResource::from),
                language_id: options.language.clone(),
                initial_value: options.content.clone(),
                encoding: options.encoding.clone(),
                ..UntitledTextEditorOptions::default()
            }));
            if let Some(encoding) = options.encoding.as_deref().filter(|encoding| !encoding.is_empty()) {
                model.set_encoding(encoding).await?;
            }
            let resource = model.resource();
            let reference = this.text_model_resolver_service.create_model_reference(&resource).await?;
            if !this.model_trackers.borrow().has(&resource) {
                reference.dispose();
                return Err(Error::new(format!("expected URI {} to have come to LIFE", resource.to_string(false).to_string_lossy())));
            }
            let length = reference
                .object
                .text_editor_model()
                .ok_or_else(|| Error::new("o modelo da referência não foi resolvido"))?
                .get_value_length();
            let reference_disposable: DisposableRef = Rc::new(reference);
            this.model_reference_collection.add(resource.clone(), reference_disposable, length);
            let weak = this.me.clone();
            let reverted = resource.clone();
            model.on_did_revert().once().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.model_reference_collection.remove(&reverted);
                }
            });
            this.proxy.rpc_accept_dirty_state_changed(&resource, true); // mark as dirty
            Ok(resource)
        })
    }
}

impl IExtHostDocumentSaveDelegate for MainThreadDocuments {
    /// `$trySaveDocument(uri)`.
    fn rpc_try_save_document(&self, uri: &URI) -> Promise<bool> {
        self.text_file_service.save(uri, None).then(|target| target.is_some())
    }
}

impl MainThreadDocumentsShape for MainThreadDocuments {
    /// `$tryCreateDocument(options)`.
    fn rpc_try_create_document(&self, options: Option<CreateDocumentOptions>) -> Promise<URI> {
        self.do_create_untitled(None, options)
    }

    /// `$tryOpenDocument(uriData, options)`.
    fn rpc_try_open_document(&self, uri_data: &URI, options: Option<OpenDocumentOptions>) -> Promise<URI> {
        let this = self.this();
        let input_uri = uri_data.clone();
        async_fn(async move {
            if input_uri.scheme().is_empty() || (je_platform::resources::fs_path(&input_uri).is_empty() && input_uri.authority().is_empty()) {
                return Err(Error::no_telemetry("Invalid uri. Scheme and authority or path must be set."));
            }

            let canonical_uri = this.uri_identity_service.as_canonical_uri(&input_uri);
            let canonical_text = canonical_uri.to_string(false).to_string_lossy();

            let promise = if canonical_uri.scheme().to_string_lossy() == schemas::UNTITLED {
                this.handle_untitled_scheme(canonical_uri.clone(), options)
            } else {
                this.handle_as_resource_input(canonical_uri.clone(), options)
            };

            let document_uri = match promise.await {
                Ok(document_uri) => document_uri,
                Err(err) => {
                    return Err(Error::no_telemetry(format!("cannot open {canonical_text}. Detail: {}", to_error_message_of(&err, false))));
                }
            };
            // `!documentUri` do original não existe: o `Promise<URI>` sempre resolve com um URI.
            if !ext_uri().is_equal(Some(&document_uri), Some(&canonical_uri), false) {
                Err(Error::no_telemetry(format!(
                    "cannot open {canonical_text}. Detail: Actual document opened as {}",
                    document_uri.to_string(false).to_string_lossy()
                )))
            } else if !this.model_trackers.borrow().has(&canonical_uri) {
                Err(Error::no_telemetry(format!("cannot open {canonical_text}. Detail: Files above 50MB cannot be synchronized with extensions.")))
            } else {
                Ok(canonical_uri)
            }
        })
    }
}

impl IDisposable for MainThreadDocuments {
    fn dispose(&self) {
        let trackers: Vec<Rc<ModelTracker>> = self.model_trackers.borrow().values().cloned().collect();
        for tracker in trackers {
            tracker.dispose();
        }
        self.model_trackers.borrow_mut().clear();
        self.base.dispose();
    }
}
