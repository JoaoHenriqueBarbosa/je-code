//! Porte de `src/vs/editor/common/services/inMemoryTextModelService.ts`: o `ITextModelService` mínimo que só serve modelos que já
//! existem no `IModelService` (ou que ele cria para um documento sintético), com contagem de referências. É o serviço que o
//! `createCodeEditorServices` dos testes do editor registra como `ITextModelService`.
//!
//! O original mora em `editor/common`, mas o `ITextEditorModel` que ele devolve depende do `IEditorModel` do workbench; por isso
//! este arquivo vive aqui, junto do contrato ([`resolver_service`](super::resolver_service)).
//!
//! Divergências:
//! - **Herança.** `InMemoryModelCollection extends ReferenceCollection` é a composição de [`InMemoryModelCollection`] (que
//!   implementa [`ReferencedObjects`]) com a [`ReferenceCollection`] que o guarda. O `createReferencedObject(key, value?,
//!   languageSelection)` com argumentos extras é o fecho de [`ReferenceCollection::acquire_with`] sobre
//!   [`InMemoryModelCollection::lazy_model`]; o sem argumentos é o [`ReferencedObjects::create_referenced_object`].
//! - **`SimpleModel extends Disposable`.** Guarda o [`EditorModel`] (o `Disposable` do workbench que já tem o `onWillDispose`) e
//!   herda dele o `isDisposed`; o `dispose` do original é o [`IDisposable`] desta struct.
//! - **`Lazy`.** É o `Lazy` de `je_base::lazy`; o `throw new Error('Model not found')` do executor é o `errors::throw`, capturado
//!   pelo `try/catch` de `_acquire` (`errors::catch`).
//! - **`createSnapshot()`.** O original chama `textEditorModel.createSnapshot()` (sem argumentos); o do porte pede o `preserveBOM`,
//!   que o `ITextModel.createSnapshot` do original assume `false`.
//! - **`registerTextModelContentProvider()`** devolve `Disposable.None` ([`lifecycle::none`]).

use std::any::Any;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lazy::Lazy;
use je_base::lifecycle::{self, DisposableRef, IDisposable, Reference, ReferenceCollection, ReferencedObjects};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_base::uuid::generate_uuid;
use je_editor::language_service::ILanguageSelection;
use je_editor::model::TextSnapshot;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_instantiation::{Injectable, service_impl};

use crate::common::editor::editor_model::{EditorModel, IEditorModel};
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelContentProvider, ITextModelService, TextModelReference};

/// `resource.toString()`: a chave do recurso na coleção.
fn key_of(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `SimpleModel`.
pub struct SimpleModel {
    base: EditorModel,
    text_editor_model: Rc<TextModel>,
    owns_model: bool,
}

impl SimpleModel {
    fn new(text_editor_model: Rc<TextModel>, owns_model: bool) -> Rc<Self> {
        Rc::new(Self { base: EditorModel::new_inner(), text_editor_model, owns_model })
    }

    /// `getLanguageId()`.
    pub fn get_language_id(&self) -> String {
        self.text_editor_model.get_language_id()
    }
}

impl IDisposable for SimpleModel {
    fn dispose(&self) {
        if IEditorModel::is_disposed(&self.base) {
            return;
        }
        // `_onWillDispose.fire()` e `super.dispose()`.
        self.base.dispose();
        if self.owns_model {
            self.text_editor_model.dispose();
        }
    }
}

impl IEditorModel for SimpleModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }

    fn resolve(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn is_resolved(&self) -> bool {
        true
    }

    fn is_disposed(&self) -> bool {
        IEditorModel::is_disposed(&self.base)
    }
}

impl ITextEditorModel for SimpleModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        Some(self.text_editor_model.clone())
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        Some(self.text_editor_model.create_snapshot(false))
    }
}

/// `Lazy<SimpleModel>`: o objeto que a coleção guarda por recurso.
type LazyModel = Rc<Lazy<Rc<SimpleModel>>>;

/// `InMemoryModelCollection`.
struct InMemoryModelCollection {
    model_service: Rc<dyn IModelService>,
}

impl InMemoryModelCollection {
    /// `createReferencedObject(key, value?, languageSelection)`.
    fn lazy_model(&self, key: &str, value: Option<(TextSource, Option<Rc<dyn ILanguageSelection>>)>) -> LazyModel {
        let model_service = self.model_service.clone();
        let key = key.to_string();
        Rc::new(Lazy::new(move || {
            let resource = URI::parse(key.as_str(), false);
            let owns_model = value.is_some();
            let model = match value {
                None => model_service.get_model(&resource),
                Some((value, language_selection)) => Some(model_service.create_model(value, language_selection.as_deref(), Some(&resource), false)),
            };
            match model {
                Some(model) => SimpleModel::new(model, owns_model),
                None => errors::throw(Error::new("Model not found")),
            }
        }))
    }
}

impl ReferencedObjects<LazyModel> for InMemoryModelCollection {
    fn create_referenced_object(&self, key: &str) -> LazyModel {
        self.lazy_model(key, None)
    }

    fn destroy_referenced_object(&self, _key: &str, model: &LazyModel) {
        if let Some(model) = model.raw_value() {
            model.dispose();
        }
    }
}

/// `InMemoryTextModelService`.
pub struct InMemoryTextModelService {
    state: Rc<InMemoryModelCollection>,
    models: ReferenceCollection<LazyModel, InMemoryModelCollection>,
}

impl Injectable for InMemoryTextModelService {
    type Args = ();
    type Deps = (Rc<dyn IModelService>,);

    fn create(_: Self::Args, (model_service,): Self::Deps) -> Rc<Self> {
        let state = Rc::new(InMemoryModelCollection { model_service });
        Rc::new(Self { models: ReferenceCollection::with_factory(state.clone()), state })
    }
}

service_impl!(InMemoryTextModelService => dyn ITextModelService);

impl InMemoryTextModelService {
    /// `_acquire(resource, value?, languageSelection)`.
    fn acquire(&self, resource: &URI, value: Option<(TextSource, Option<Rc<dyn ILanguageSelection>>)>) -> Promise<TextModelReference> {
        let key = key_of(resource);
        let reference = match value {
            Some(value) => {
                let (state, lazy_key) = (self.state.clone(), key.clone());
                self.models.acquire_with(&key, move || state.lazy_model(&lazy_key, Some(value)))
            }
            None => self.models.acquire(&key),
        };

        async_fn(async move {
            // The entry must exist before model creation, and reentrant opens must
            // wait for its lazy factory to finish.
            Promise::resolved(()).await?;
            match errors::catch(|| reference.object.value()) {
                Ok(model) => {
                    let object: Rc<dyn ITextEditorModel> = model;
                    let release: DisposableRef = Rc::new(reference);
                    Ok(Reference::new(object, release))
                }
                Err(error) => {
                    reference.dispose();
                    Err(error)
                }
            }
        })
    }
}

impl ITextModelService for InMemoryTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        self.acquire(resource, None)
    }

    fn create_synthetic_document(&self, value: TextSource, language_selection: Option<Rc<dyn ILanguageSelection>>) -> Promise<TextModelReference> {
        let components = UriComponents {
            scheme: Some(JsString::from(schemas::IN_MEMORY)),
            path: Some(JsString::from(format!("/synthetic/{}", generate_uuid()).as_str())),
            ..UriComponents::default()
        };
        self.acquire(&URI::from(&components, false), Some((value, language_selection)))
    }

    fn register_text_model_content_provider(&self, _scheme: &str, _provider: Rc<dyn ITextModelContentProvider>) -> DisposableRef {
        lifecycle::none()
    }

    fn can_handle_resource(&self, _resource: &URI) -> bool {
        false
    }
}
