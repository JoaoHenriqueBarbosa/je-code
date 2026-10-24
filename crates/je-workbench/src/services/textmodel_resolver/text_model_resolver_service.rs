//! Porte de `src/vs/workbench/services/textmodelResolver/common/textModelResolverService.ts`: o `TextModelResolverService`
//! (o `ITextModelService` do workbench) e a `ResourceModelCollection`, a `ReferenceCollection` que cria e descarta os
//! modelos de editor dos recursos e despacha pelo esquema: `untitled` pelo serviço de modelos sem título, recursos com
//! provedor de arquivo pelo serviço de arquivos de texto, `inmemory` e esquemas com provedor de conteúdo pelo
//! `TextResourceEditorModel`, e os demais (depois de tentar ativar o provedor de arquivo do esquema) rejeitam.
//!
//! Divergências:
//! - **`ModelUndoRedoParticipant`.** O construtor do original registra o participante, que só age sobre os
//!   `MultiModelEditStackElement` da pilha de desfazer (`setDelegate` e `prepareUndoRedo`); esse elemento e o
//!   `IUndoRedoDelegate` ainda não existem no porte (ver `je_editor::edit_stack`), então não há elemento sobre o qual o
//!   participante aja, e o serviço não injeta o `IUndoRedoService` nem o registra. Entra com o `MultiModelEditStackElement`.
//! - **Herança.** `ResourceModelCollection extends ReferenceCollection` é a composição de [`ResourceModelState`] (o estado
//!   e o `createReferencedObject`/`destroyReferencedObject` da subclasse, que implementa [`ReferencedObjects`]) com a
//!   [`ReferenceCollection`] que o guarda por [`ReferenceCollection::with_factory`]. O `createReferencedObject(key, value?,
//!   languageSelection)` com argumentos extras é o fecho de [`ReferenceCollection::acquire_with`]; o sem argumentos é o
//!   `doCreateReferencedObject`.
//! - **`instanceof TextFileEditorModel` e `instanceof UntitledTextEditorModel`** são o `downcast_ref` por
//!   [`IEditorModel::as_any`]. O gerenciador devolve um `ITextFileEditorModel`, que não tem o `ITextEditorModel` como
//!   supertrait; o modelo de editor do recurso é o `TextFileEditorModel` concreto (único implementador), recuperado por
//!   [`TextFileEditorModel::rc`]. Um `ITextFileEditorModel` de outro tipo rejeita com o `Unable to resolve resource` do
//!   original (que aceitaria qualquer um).
//! - **`activateProvider`.** É o `IFileService::activate_provider`.
//! - **`async` e `await`.** As funções `async` do original são [`async_fn`] (o corpo roda já, até o primeiro `await`), e
//!   `return this.doCreateReferencedObject(key, true)` é a adoção de [`MaybePromise::Promise`].
//! - **`Disposable`.** O serviço guarda o [`Disposable`] base e o descarta no `dispose` (o `_register` do original).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{async_fn, async_fn_maybe, await_maybe};
use je_base::errors::{self, Error};
use je_base::lifecycle::{AsyncReferenceCollection, Disposable, DisposableRef, IDisposable, ReferenceCollection, ReferencedObjects, to_disposable};
use je_base::network::schemas;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::{URI, UriComponents};
use je_base::uuid::generate_uuid;
use je_editor::language_service::ILanguageSelection;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextSource;
use je_instantiation::{Injectable, InstantiationService, service_impl};
use je_platform::files::i_file_service::IFileService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::text_resource_editor_model::TextResourceEditorModel;
use crate::services::textfile::text_file_editor_model::TextFileEditorModel;
use crate::services::textfile::textfiles::{ITextFileEditorModel, ITextFileEditorModelManager, ITextFileService, TextFileEditorModelResolveOrCreateOptions, TextFileResolveOptions, TextFileResolveReason};
use crate::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelContentProvider, ITextModelService, TextModelReference, is_resolved_text_editor_model};
use crate::services::untitled::untitled_text_editor_model::{IUntitledTextEditorModel, UntitledTextEditorModel};
use crate::services::untitled::untitled_text_editor_service::{IUntitledTextEditorService, UntitledTextEditorOptions};

/// `Promise<IResolvedTextEditorModel>`: o objeto que a coleção guarda por recurso.
type ModelPromise = Promise<Rc<dyn ITextEditorModel>>;

/// `resource.toString()`: a chave do recurso na coleção.
fn key_of(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `await this.textFileService.files.canDispose(model)`: `None` é o `true` imediato (um `await` de valor, uma rodada).
fn wait_until_can_dispose(can_dispose: Option<Promise<()>>) -> Promise<()> {
    can_dispose.unwrap_or_else(|| Promise::resolved(()))
}

/// `ensureResolvedModel(model, key)`.
fn ensure_resolved_model(model: &Rc<dyn ITextEditorModel>, key: &str) -> Result<(), Error> {
    if !is_resolved_text_editor_model(&**model) {
        return Err(Error::new(format!("Unable to resolve resource {key}")));
    }

    Ok(())
}

/// O estado de `ResourceModelCollection` e a fábrica de objetos da `ReferenceCollection` dela.
pub struct ResourceModelState {
    this: Weak<ResourceModelState>,
    instantiation_service: Rc<InstantiationService>,
    text_file_service: Rc<dyn ITextFileService>,
    file_service: Rc<dyn IFileService>,
    model_service: Rc<dyn IModelService>,
    providers: RefCell<HashMap<String, Vec<Rc<dyn ITextModelContentProvider>>>>,
    models_to_dispose: RefCell<HashMap<String, ModelPromise>>,
}

impl ResourceModelState {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("a coleção está viva enquanto cria modelos")
    }

    /// `createSyntheticDocument(resource, value, languageSelection)`.
    fn create_synthetic_document(&self, resource: URI, value: TextSource, language_selection: Option<Rc<dyn ILanguageSelection>>) -> ModelPromise {
        let this = self.rc();

        async_fn(async move {
            // ReferenceCollection installs the entry after invoking its factory. Establish
            // the creator's reference before createModel can reenter via onModelAdded.
            Promise::resolved(()).await?;
            let text_model = this.model_service.create_model(value, language_selection.as_deref(), Some(&resource), false);

            let created = errors::catch(|| -> Result<Rc<dyn ITextEditorModel>, Error> {
                let model: Rc<dyn ITextEditorModel> = this.instantiation_service.create_instance::<TextResourceEditorModel>((resource.clone(),));
                ensure_resolved_model(&model, &key_of(&resource))?;
                Ok(model)
            })
            .and_then(|result| result);

            match created {
                Ok(model) => Ok(model),
                Err(error) => {
                    text_model.dispose();
                    Err(error)
                }
            }
        })
    }

    /// `doCreateReferencedObject(key, skipActivateProvider)`.
    fn do_create_referenced_object(&self, key: String, skip_activate_provider: bool) -> ModelPromise {
        let this = self.rc();

        async_fn_maybe(async move {
            let resource = URI::parse(key.as_str(), false);
            let scheme = resource.scheme().to_string_lossy();

            // Untrack as being disposed
            let pending_model = this.models_to_dispose.borrow_mut().remove(&key);

            // Untitled Schema: go through untitled text service
            if scheme == schemas::UNTITLED {
                let options = UntitledTextEditorOptions { untitled_resource: Some(resource), ..UntitledTextEditorOptions::default() };
                let model = this.text_file_service.untitled().resolve(Some(options)).await?;
                let model: Rc<dyn ITextEditorModel> = model;
                ensure_resolved_model(&model, &key)?;
                return Ok(MaybePromise::Value(model));
            }

            // File or remote file: go through text file service
            if this.file_service.has_provider(&resource) {
                let options = TextFileEditorModelResolveOrCreateOptions {
                    resolve: TextFileResolveOptions { reason: Some(TextFileResolveReason::Reference), ..TextFileResolveOptions::default() },
                    ..TextFileEditorModelResolveOrCreateOptions::default()
                };
                let file_model = this.text_file_service.files().resolve(&resource, Some(options)).await?;
                let Some(text_file_editor_model) = file_model.as_any().downcast_ref::<TextFileEditorModel>() else {
                    return Err(Error::new(format!("Unable to resolve resource {key}")));
                };
                let model: Rc<dyn ITextEditorModel> = text_file_editor_model.rc();
                ensure_resolved_model(&model, &key)?;
                return Ok(MaybePromise::Value(model));
            }

            // In-Memory / Virtual documents
            if scheme == schemas::IN_MEMORY || this.providers.borrow().contains_key(&scheme) {
                this.ensure_resolved_text_model_content(resource.clone()).await?; // throws if failing to resolve content

                let mut model: Option<Rc<dyn ITextEditorModel>> = None;
                if let Some(pending_model) = pending_model {
                    // if we have a pending model for this key, we try to await that and prevent
                    // creating a new model so that we are not leaking models. we only do this for
                    // in-memory or virtual documents where we create models here, the others are
                    // already shared by their respective services.
                    if let Ok(pending) = pending_model.await {
                        model = Some(pending);
                    }
                    // else: ignore and re-create below
                }

                let model = match model {
                    Some(model) => model,
                    None => this.instantiation_service.create_instance::<TextResourceEditorModel>((resource,)),
                };

                ensure_resolved_model(&model, &key)?;
                return Ok(MaybePromise::Value(model));
            }

            // Either unknown schema, or not yet registered, try to activate
            if !skip_activate_provider {
                this.file_service.activate_provider(&scheme).await?;

                return Ok(MaybePromise::Promise(this.do_create_referenced_object(key, true)));
            }

            Err(Error::new(format!("Unable to resolve resource {key}")))
        })
    }

    /// `ensureResolvedTextModelContent(resource)`.
    fn ensure_resolved_text_model_content(&self, resource: URI) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let scheme = resource.scheme().to_string_lossy();

            // in-memory based
            if scheme == schemas::IN_MEMORY && this.model_service.get_model(&resource).is_some() {
                return Ok(());
            }

            // provider based
            let providers_for_scheme = this.providers.borrow().get(&scheme).cloned().unwrap_or_default();

            for provider in providers_for_scheme {
                if await_maybe(provider.provide_text_content(&resource)).await?.is_some() {
                    return Ok(());
                }
            }

            Err(Error::new(format!("Unable to resolve text model content for resource {}", key_of(&resource))))
        })
    }

    /// O corpo do IIFE `async` de `destroyReferencedObject`, sem o `catch` e o `finally`.
    async fn dispose_when_possible(&self, key: &str, model_promise: ModelPromise) -> Result<(), Error> {
        let model = model_promise.await?;

        if !self.models_to_dispose.borrow().contains_key(key) {
            // return if model has been acquired again meanwhile
            return Ok(());
        }

        if let Some(text_file_editor_model) = model.as_any().downcast_ref::<TextFileEditorModel>() {
            // text file models have conditions that prevent them
            // from dispose, so we have to wait until we can dispose
            let text_file_editor_model: Rc<dyn ITextFileEditorModel> = text_file_editor_model.rc();
            wait_until_can_dispose(self.text_file_service.files().can_dispose(text_file_editor_model)).await?;
        } else if let Some(untitled_model) = model.as_any().downcast_ref::<UntitledTextEditorModel>() {
            // untitled file models have conditions that prevent them
            // from dispose, so we have to wait until we can dispose
            let untitled_model: Rc<dyn IUntitledTextEditorModel> = untitled_model.rc();
            wait_until_can_dispose(self.text_file_service.untitled().can_dispose(&untitled_model)).await?;
        }

        if !self.models_to_dispose.borrow().contains_key(key) {
            // return if model has been acquired again meanwhile
            return Ok(());
        }

        // Finally we can dispose the model
        model.dispose();

        Ok(())
    }
}

impl ReferencedObjects<ModelPromise> for ResourceModelState {
    /// `createReferencedObject(key)` sem valor: o `doCreateReferencedObject(key)`.
    fn create_referenced_object(&self, key: &str) -> ModelPromise {
        self.do_create_referenced_object(key.to_string(), false)
    }

    /// `destroyReferencedObject(key, modelPromise)`.
    fn destroy_referenced_object(&self, key: &str, model_promise: &ModelPromise) {
        // Track as being disposed before waiting for model to load
        // to handle the case that the reference is acquired again
        self.models_to_dispose.borrow_mut().insert(key.to_string(), model_promise.clone());

        let this = self.rc();
        let key = key.to_string();
        let model_promise = model_promise.clone();
        runtime::spawn_local(async move {
            // `catch` ignora o erro e o `finally` solta o recurso do `modelsToDispose`
            let _ = this.dispose_when_possible(&key, model_promise).await;
            this.models_to_dispose.borrow_mut().remove(&key); // Untrack as being disposed
        });
    }
}

/// `ResourceModelCollection`: os modelos de editor por recurso, com contagem de referências.
pub struct ResourceModelCollection {
    state: Rc<ResourceModelState>,
    collection: Rc<ReferenceCollection<ModelPromise, ResourceModelState>>,
}

impl Injectable for ResourceModelCollection {
    type Args = ();
    type Deps = (Rc<InstantiationService>, Rc<dyn ITextFileService>, Rc<dyn IFileService>, Rc<dyn IModelService>);

    fn create(_: Self::Args, (instantiation_service, text_file_service, file_service, model_service): Self::Deps) -> Rc<Self> {
        let state = Rc::new_cyclic(|this| ResourceModelState {
            this: this.clone(),
            instantiation_service,
            text_file_service,
            file_service,
            model_service,
            providers: RefCell::default(),
            models_to_dispose: RefCell::default(),
        });
        let collection = Rc::new(ReferenceCollection::with_factory(state.clone()));

        Rc::new(Self { state, collection })
    }
}

impl ResourceModelCollection {
    /// A `ReferenceCollection` que o `AsyncReferenceCollection` envolve.
    pub fn collection(&self) -> Rc<ReferenceCollection<ModelPromise, ResourceModelState>> {
        self.collection.clone()
    }

    /// `createReferencedObject(key, value, languageSelection)` com valor: o documento sintético.
    pub fn create_synthetic_document(&self, key: &str, value: TextSource, language_selection: Option<Rc<dyn ILanguageSelection>>) -> ModelPromise {
        self.state.create_synthetic_document(URI::parse(key, false), value, language_selection)
    }

    /// `registerTextModelContentProvider(scheme, provider)`.
    pub fn register_text_model_content_provider(&self, scheme: &str, provider: Rc<dyn ITextModelContentProvider>) -> DisposableRef {
        self.state.providers.borrow_mut().entry(scheme.to_string()).or_default().insert(0, provider.clone());

        let state = Rc::downgrade(&self.state);
        let scheme = scheme.to_string();
        to_disposable(move || {
            let Some(state) = state.upgrade() else { return };
            let mut providers = state.providers.borrow_mut();
            let Some(providers_for_scheme) = providers.get_mut(&scheme) else { return };

            let Some(index) = providers_for_scheme.iter().position(|candidate| Rc::ptr_eq(candidate, &provider)) else { return };

            providers_for_scheme.remove(index);

            if providers_for_scheme.is_empty() {
                providers.remove(&scheme);
            }
        })
    }

    /// `hasTextModelContentProvider(scheme)`.
    pub fn has_text_model_content_provider(&self, scheme: &str) -> bool {
        self.state.providers.borrow().contains_key(scheme)
    }
}

/// `AsyncReferenceCollection<IResolvedTextEditorModel>`.
type AsyncModelCollection = AsyncReferenceCollection<Rc<dyn ITextEditorModel>, ResourceModelState>;

/// `TextModelResolverService`.
pub struct TextModelResolverService {
    base: Rc<Disposable>,
    instantiation_service: Rc<InstantiationService>,
    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    resource_model_collection: RefCell<Option<Rc<ResourceModelCollection>>>,
    async_model_collection: RefCell<Option<Rc<AsyncModelCollection>>>,
}

impl Injectable for TextModelResolverService {
    type Args = ();
    type Deps = (Rc<InstantiationService>, Rc<dyn IFileService>, Rc<dyn IUriIdentityService>);

    fn create(_: Self::Args, (instantiation_service, file_service, uri_identity_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self {
            base: Disposable::new(),
            instantiation_service,
            file_service,
            uri_identity_service,
            resource_model_collection: RefCell::new(None),
            async_model_collection: RefCell::new(None),
        })
    }
}

service_impl!(TextModelResolverService => dyn ITextModelService, disposable);

impl IDisposable for TextModelResolverService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl TextModelResolverService {
    /// `resourceModelCollection`: criada no primeiro uso.
    fn resource_model_collection(&self) -> Rc<ResourceModelCollection> {
        let existing = self.resource_model_collection.borrow().clone();
        if let Some(collection) = existing {
            return collection;
        }

        let collection = self.instantiation_service.create_instance::<ResourceModelCollection>(());
        *self.resource_model_collection.borrow_mut() = Some(collection.clone());

        collection
    }

    /// `asyncModelCollection`: criada no primeiro uso.
    fn async_model_collection(&self) -> Rc<AsyncModelCollection> {
        let existing = self.async_model_collection.borrow().clone();
        if let Some(collection) = existing {
            return collection;
        }

        let collection = Rc::new(AsyncReferenceCollection::new(self.resource_model_collection().collection()));
        *self.async_model_collection.borrow_mut() = Some(collection.clone());

        collection
    }
}

impl ITextModelService for TextModelResolverService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        // From this moment on, only operate on the canonical resource
        // to ensure we reduce the chance of resolving the same resource
        // with different resource forms (e.g. path casing on Windows)
        let resource = self.uri_identity_service.as_canonical_uri(resource);

        let reference = self.async_model_collection().acquire(&key_of(&resource));
        async_fn(async move { reference.await })
    }

    fn create_synthetic_document(&self, value: TextSource, language_selection: Option<Rc<dyn ILanguageSelection>>) -> Promise<TextModelReference> {
        let components = UriComponents {
            scheme: Some(JsString::from(schemas::IN_MEMORY)),
            path: Some(JsString::from(format!("/synthetic/{}", generate_uuid()).as_str())),
            ..UriComponents::default()
        };
        let resource = self.uri_identity_service.as_canonical_uri(&URI::from(&components, false));

        let collection = self.resource_model_collection();
        self.async_model_collection().acquire_with(&key_of(&resource), || collection.create_synthetic_document(&key_of(&resource), value, language_selection))
    }

    fn register_text_model_content_provider(&self, scheme: &str, provider: Rc<dyn ITextModelContentProvider>) -> DisposableRef {
        self.resource_model_collection().register_text_model_content_provider(scheme, provider)
    }

    fn can_handle_resource(&self, resource: &URI) -> bool {
        let scheme = resource.scheme().to_string_lossy();
        if self.file_service.has_provider(resource) || scheme == schemas::UNTITLED || scheme == schemas::IN_MEMORY {
            return true; // we handle file://, untitled:// and inMemory:// automatically
        }

        self.resource_model_collection().has_text_model_content_provider(&scheme)
    }
}
