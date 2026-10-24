//! Porte de `src/vs/workbench/services/untitled/common/untitledTextEditorService.ts`: o `UntitledTextEditorService`, que
//! cria, guarda por recurso e devolve os modelos de editor de texto sem título, nomeia os novos (`Untitled-N`), repassa os
//! eventos dos modelos e diz quando um modelo pode ser descartado.
//!
//! Divergências:
//! - **Sobrecargas de `create` e `resolve`.** As três sobrecargas (`INewUntitledTextEditorOptions`,
//!   `INewUntitledTextEditorWithAssociatedResourceOptions` e `IExistingUntitledTextEditorOptions`) usam um tipo só nas
//!   opções, o [`UntitledTextEditorOptions`] (a interseção `IInternalUntitledTextEditorOptions` que a classe já usa por
//!   dentro): cada campo é um `Option`, e `untitledResource` só vale com o esquema `untitled`.
//! - **Retorno.** O contrato [`IUntitledTextEditorService`] devolve `Rc<dyn IUntitledTextEditorModel>`; o `get` do
//!   original devolve o `UntitledTextEditorModel`, que aqui é o mesmo objeto visto pelo contrato.
//! - **Injeção.** `IInstantiationService` é o tipo concreto [`InstantiationService`] (ver `je_instantiation`) e o modelo
//!   nasce por `create_instance`, com o `IConfigurationService` ao lado.
//! - **`DisposableResourceMap<UntitledTextEditorModel>`** (que o `lifecycle.ts` do porte ainda não traz) é o
//!   [`DisposableMap`] com a chave `uri.toString()`, que é a chave do `ResourceMap` do original.
//! - **`canDispose(model): true | Promise<true>`** é `Option<Promise<()>>`: `None` é o `true` imediato, como o
//!   `canDispose` do gerenciador de arquivos de texto.
//! - **`IFilesConfiguration`** (`configuration.files?.defaultLanguage`) é a leitura do [`Dynamic`] da configuração.
//! - **Escutas dos modelos.** As escutas que o serviço instala no modelo seguram o modelo por `Weak` (o `Rc` do modelo
//!   guarda os emissores onde as escutas vivem), e as do serviço seguram o serviço por `Weak`.
//! - **`registerSingleton(..., InstantiationType.Delayed)`** entra com a montagem do workbench, que também decide como o
//!   mesmo objeto atende o `IUntitledTextEditorService` e o `IUntitledTextEditorModelManager` (aqui os dois traits).

use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_instantiation::{Injectable, InstantiationService, service_id, service_impl};
use je_platform::configuration::service::IConfigurationService;

use crate::common::editor::editor_model::IEditorModel;
use crate::services::textmodel_resolver::resolver_service::ITextEditorModel;
use crate::services::untitled::untitled_text_editor_model::{IUntitledTextEditorModel, UntitledTextEditorModel};
use crate::services::working_copy::working_copy::IWorkingCopy;

service_id!(pub I_UNTITLED_TEXT_EDITOR_SERVICE: dyn IUntitledTextEditorService = "untitledTextEditorService");

/// `{ authority, path, query, fragment }` de `associatedResource`: resource components to associate with the untitled
/// editor. When saving the untitled editor, the associated components will be used and the user is not being asked to
/// provide a file path.
///
/// Note: currently it is not possible to specify the `scheme` to use. The untitled editor will saved to the default
/// local or remote resource.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssociatedResource {
    pub authority: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

/// Um `URI` serve como `associatedResource` (o original aceita qualquer objeto com os quatro componentes).
impl From<&URI> for AssociatedResource {
    fn from(resource: &URI) -> Self {
        Self {
            authority: resource.authority().to_string_lossy(),
            path: resource.path().to_string_lossy(),
            query: resource.query().to_string_lossy(),
            fragment: resource.fragment().to_string_lossy(),
        }
    }
}

/// `INewUntitledTextEditorOptions`, `IExistingUntitledTextEditorOptions` e
/// `INewUntitledTextEditorWithAssociatedResourceOptions` (ver as divergências do módulo).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UntitledTextEditorOptions {
    /// Initial value of the untitled editor. An untitled editor with initial value is dirty right from the beginning.
    pub initial_value: Option<String>,

    /// Preferred language id to use when saving the untitled editor.
    pub language_id: Option<String>,

    /// Preferred encoding to use when saving the untitled editor.
    pub encoding: Option<String>,

    /// A resource to identify the untitled editor to create or return if already existing.
    ///
    /// Note: the resource will not be used unless the scheme is `untitled`.
    pub untitled_resource: Option<URI>,

    /// Resource components to associate with the untitled editor.
    pub associated_resource: Option<AssociatedResource>,
}

/// `IUntitledTextEditorModelSaveEvent`.
#[derive(Clone, Debug)]
pub struct UntitledTextEditorModelSaveEvent {
    /// The source untitled file that was saved. It is disposed at this point.
    pub source: URI,

    /// The target file the untitled was saved to. Is never untitled.
    pub target: URI,
}

/// `IUntitledTextEditorService`.
pub trait IUntitledTextEditorService {
    /// An event for when an untitled editor model was saved to disk. At the point the event fires, the untitled editor
    /// model is disposed.
    fn on_did_save(&self) -> Event<UntitledTextEditorModelSaveEvent>;

    /// Events for when untitled text editors change (e.g. getting dirty, saved or reverted).
    fn on_did_change_dirty(&self) -> Event<Rc<dyn IUntitledTextEditorModel>>;

    /// Events for when untitled text editor encodings change.
    fn on_did_change_encoding(&self) -> Event<Rc<dyn IUntitledTextEditorModel>>;

    /// Events for when untitled text editor labels change.
    fn on_did_change_label(&self) -> Event<Rc<dyn IUntitledTextEditorModel>>;

    /// Events for when untitled text editor models are created.
    fn on_did_create(&self) -> Event<Rc<dyn IUntitledTextEditorModel>>;

    /// Events for when untitled text editors are about to be disposed.
    fn on_will_dispose(&self) -> Event<Rc<dyn IUntitledTextEditorModel>>;

    /// Creates a new untitled editor model with the provided options. If the `untitledResource` property is provided and
    /// the untitled editor exists, it will return that existing instance instead of creating a new one.
    fn create(&self, options: Option<UntitledTextEditorOptions>) -> Rc<dyn IUntitledTextEditorModel>;

    /// Returns an existing untitled editor model if already created before.
    fn get(&self, resource: &URI) -> Option<Rc<dyn IUntitledTextEditorModel>>;

    /// Returns the value of the untitled editor, `None` if none exists.
    fn get_value(&self, resource: &URI) -> Option<String>;

    /// Resolves an untitled editor model from the provided options. If the `untitledResource` property is provided and
    /// the untitled editor exists, it will return that existing instance instead of creating a new one.
    fn resolve(&self, options: Option<UntitledTextEditorOptions>) -> Promise<Rc<dyn IUntitledTextEditorModel>>;

    /// Figures out if the given resource has an associated resource or not.
    fn is_untitled_with_associated_resource(&self, resource: &URI) -> bool;

    /// Waits for the model to be ready to be disposed. There may be conditions under which the model cannot be disposed,
    /// e.g. when it is dirty. Once the promise is settled, it is safe to dispose the model. `None` é o `true` imediato.
    fn can_dispose(&self, model: &Rc<dyn IUntitledTextEditorModel>) -> Option<Promise<()>>;
}

/// `IUntitledTextEditorModelManager`.
pub trait IUntitledTextEditorModelManager: IUntitledTextEditorService {
    /// Internal method: triggers the onDidSave event.
    fn notify_did_save(&self, source: &URI, target: &URI);
}

/// A chave de um recurso no mapa de modelos (a do `ResourceMap`: `uri.toString()`).
fn key_of(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `/Untitled-\d+/.test(path)`: o padrão não é ancorado, então vale uma ocorrência de `Untitled-` seguida de dígito em
/// qualquer ponto do caminho.
fn is_untitled_without_associated_resource_path(path: &[u16]) -> bool {
    let needle: Vec<u16> = "Untitled-".encode_utf16().collect();
    path.windows(needle.len()).enumerate().any(|(index, window)| window == needle.as_slice() && path.get(index + needle.len()).is_some_and(|unit| (0x30..=0x39).contains(unit)))
}

/// `configuration.files?.defaultLanguage` quando existe e não é vazio.
fn default_language_of(configuration: &Dynamic) -> Option<String> {
    let Dynamic::Object(_) = configuration else {
        return None;
    };
    let files = configuration.get_str("files");
    let Dynamic::Object(_) = files else {
        return None;
    };
    match files.get_str("defaultLanguage") {
        Dynamic::String(default_language) if !default_language.is_empty() => Some(default_language.to_string_lossy()),
        _ => None,
    }
}

/// `UntitledTextEditorService`.
pub struct UntitledTextEditorService {
    this: Weak<UntitledTextEditorService>,
    base: Rc<Disposable>,

    on_did_save: Rc<Emitter<UntitledTextEditorModelSaveEvent>>,
    on_did_change_dirty: Rc<Emitter<Rc<dyn IUntitledTextEditorModel>>>,
    on_did_change_encoding: Rc<Emitter<Rc<dyn IUntitledTextEditorModel>>>,
    on_did_create: Rc<Emitter<Rc<dyn IUntitledTextEditorModel>>>,
    on_will_dispose: Rc<Emitter<Rc<dyn IUntitledTextEditorModel>>>,
    on_did_change_label: Rc<Emitter<Rc<dyn IUntitledTextEditorModel>>>,

    map_resource_to_model: Rc<DisposableMap<String, UntitledTextEditorModel>>,

    instantiation_service: Rc<InstantiationService>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl UntitledTextEditorService {
    /// `constructor(instantiationService, configurationService)`.
    pub fn new(instantiation_service: Rc<InstantiationService>, configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        let base = Disposable::new();

        let on_did_save = Emitter::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_change_encoding = Emitter::new();
        let on_did_create = Emitter::new();
        let on_will_dispose = Emitter::new();
        let on_did_change_label = Emitter::new();
        base.register(on_did_save.clone());
        base.register(on_did_change_dirty.clone());
        base.register(on_did_change_encoding.clone());
        base.register(on_did_create.clone());
        base.register(on_will_dispose.clone());
        base.register(on_did_change_label.clone());

        let map_resource_to_model: Rc<DisposableMap<String, UntitledTextEditorModel>> = DisposableMap::new();
        base.register(map_resource_to_model.clone());

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_save,
            on_did_change_dirty,
            on_did_change_encoding,
            on_did_create,
            on_will_dispose,
            on_did_change_label,
            map_resource_to_model,
            instantiation_service,
            configuration_service,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    fn do_create_or_get(&self, options: Option<UntitledTextEditorOptions>) -> Rc<UntitledTextEditorModel> {
        let massaged_options = self.massage_options(&options.unwrap_or_default());

        // Return existing instance if asked for it
        if let Some(untitled_resource) = &massaged_options.untitled_resource
            && let Some(existing) = self.map_resource_to_model.get(&key_of(untitled_resource))
        {
            return existing;
        }

        // Create new instance otherwise
        self.do_create(massaged_options)
    }

    fn massage_options(&self, options: &UntitledTextEditorOptions) -> UntitledTextEditorOptions {
        let mut massaged_options = UntitledTextEditorOptions::default();

        // Figure out associated and untitled resource
        if let Some(associated_resource) = &options.associated_resource {
            massaged_options.untitled_resource = Some(URI::from(
                &UriComponents {
                    scheme: Some(JsString::from(schemas::UNTITLED)),
                    authority: Some(JsString::from(associated_resource.authority.as_str())),
                    fragment: Some(JsString::from(associated_resource.fragment.as_str())),
                    path: Some(JsString::from(associated_resource.path.as_str())),
                    query: Some(JsString::from(associated_resource.query.as_str())),
                },
                false,
            ));
            massaged_options.associated_resource = Some(associated_resource.clone());
        } else if let Some(untitled_resource) = &options.untitled_resource
            && *untitled_resource.scheme() == schemas::UNTITLED
        {
            massaged_options.untitled_resource = Some(untitled_resource.clone());
        }

        // Language id
        if let Some(language_id) = options.language_id.as_ref().filter(|language_id| !language_id.is_empty()) {
            massaged_options.language_id = Some(language_id.clone());
        } else if massaged_options.associated_resource.is_none() {
            let configuration = self.configuration_service.get_value(None, None);
            if let Some(default_language) = default_language_of(&configuration) {
                massaged_options.language_id = Some(default_language);
            }
        }

        // Take over encoding and initial value
        massaged_options.encoding = options.encoding.clone();
        massaged_options.initial_value = options.initial_value.clone();

        massaged_options
    }

    fn do_create(&self, options: UntitledTextEditorOptions) -> Rc<UntitledTextEditorModel> {
        // Create a new untitled resource if none is provided
        let untitled_resource = match options.untitled_resource.clone() {
            Some(untitled_resource) => untitled_resource,
            None => {
                let mut counter = 1;
                loop {
                    let candidate = URI::from(
                        &UriComponents {
                            scheme: Some(JsString::from(schemas::UNTITLED)),
                            path: Some(JsString::from(format!("Untitled-{counter}"))),
                            ..UriComponents::default()
                        },
                        false,
                    );
                    counter += 1;

                    if !self.map_resource_to_model.has(&key_of(&candidate)) {
                        break candidate;
                    }
                }
            }
        };

        // Create new model with provided options
        let model = self.instantiation_service.create_instance::<UntitledTextEditorModel>((
            untitled_resource,
            options.associated_resource.is_some(),
            options.initial_value,
            options.language_id,
            options.encoding,
        ));

        self.register_model(&model);

        model
    }

    fn register_model(&self, model: &Rc<UntitledTextEditorModel>) {
        // Install model listeners
        let model_listeners = DisposableStore::new();
        let weak_model = Rc::downgrade(model);

        let forward = |emitter: &Rc<Emitter<Rc<dyn IUntitledTextEditorModel>>>, event: Event<()>| -> DisposableRef {
            let (emitter, weak_model) = (emitter.clone(), weak_model.clone());
            event.listen(move |_| {
                if let Some(model) = weak_model.upgrade() {
                    let model: Rc<dyn IUntitledTextEditorModel> = model;
                    emitter.fire(model);
                }
            })
        };
        model_listeners.add(forward(&self.on_did_change_dirty, IWorkingCopy::on_did_change_dirty(model.as_ref())));
        model_listeners.add(forward(&self.on_did_change_label, IUntitledTextEditorModel::on_did_change_name(model.as_ref())));
        model_listeners.add(forward(&self.on_did_change_encoding, IUntitledTextEditorModel::on_did_change_encoding(model.as_ref())));
        model_listeners.add(forward(&self.on_will_dispose, IEditorModel::on_will_dispose(model.as_ref())));

        // Remove from cache on dispose
        let weak_service = self.this.clone();
        let resource_key = key_of(&IWorkingCopy::resource(model.as_ref()));
        let listeners_to_dispose = model_listeners.clone();
        IEditorModel::on_will_dispose(model.as_ref()).once().listen(move |_| {
            // Registry
            if let Some(service) = weak_service.upgrade() {
                service.map_resource_to_model.delete_and_leak(&resource_key); // model is being disposed in this callback already
            }

            // Listeners
            listeners_to_dispose.dispose();
        });

        // Add to cache
        self.map_resource_to_model.set(key_of(&IWorkingCopy::resource(model.as_ref())), model.clone(), false);

        // Emit as event
        let created: Rc<dyn IUntitledTextEditorModel> = model.clone();
        self.on_did_create.fire(created.clone());

        // If the model is dirty right from the beginning,
        // make sure to emit this as an event
        if IWorkingCopy::is_dirty(model.as_ref()) {
            self.on_did_change_dirty.fire(created);
        }
    }

    fn do_can_dispose(&self, model: Rc<dyn IUntitledTextEditorModel>) -> Promise<()> {
        // dirty model: we do not allow to dispose dirty models to prevent
        // data loss cases. dirty models can only be disposed when they are
        // either saved or reverted
        if IWorkingCopy::is_dirty(model.as_ref()) {
            let this = self.rc();
            let dirty_changed = IWorkingCopy::on_did_change_dirty(model.as_ref()).to_promise(None);

            return async_fn(async move {
                dirty_changed.await?;

                match this.can_dispose(&model) {
                    Some(can_dispose) => can_dispose.await,
                    None => Ok(()),
                }
            });
        }

        Promise::resolved(())
    }
}

impl Injectable for UntitledTextEditorService {
    type Args = ();
    type Deps = (Rc<InstantiationService>, Rc<dyn IConfigurationService>);

    fn create((): Self::Args, (instantiation_service, configuration_service): Self::Deps) -> Rc<Self> {
        Self::new(instantiation_service, configuration_service)
    }
}

service_impl!(UntitledTextEditorService => dyn IUntitledTextEditorService, disposable);

impl IDisposable for UntitledTextEditorService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUntitledTextEditorService for UntitledTextEditorService {
    fn on_did_save(&self) -> Event<UntitledTextEditorModelSaveEvent> {
        self.on_did_save.event()
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_encoding(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        self.on_did_change_encoding.event()
    }

    fn on_did_change_label(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        self.on_did_change_label.event()
    }

    fn on_did_create(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        self.on_did_create.event()
    }

    fn on_will_dispose(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        self.on_will_dispose.event()
    }

    fn create(&self, options: Option<UntitledTextEditorOptions>) -> Rc<dyn IUntitledTextEditorModel> {
        self.do_create_or_get(options)
    }

    fn get(&self, resource: &URI) -> Option<Rc<dyn IUntitledTextEditorModel>> {
        self.map_resource_to_model.get(&key_of(resource)).map(|model| -> Rc<dyn IUntitledTextEditorModel> { model })
    }

    fn get_value(&self, resource: &URI) -> Option<String> {
        let model = self.map_resource_to_model.get(&key_of(resource))?;
        model.text_editor_model().map(|text_editor_model| text_editor_model.get_value().to_string_lossy())
    }

    fn resolve(&self, options: Option<UntitledTextEditorOptions>) -> Promise<Rc<dyn IUntitledTextEditorModel>> {
        let model = self.do_create_or_get(options);
        model.resolve().then(move |()| -> Rc<dyn IUntitledTextEditorModel> { model })
    }

    fn is_untitled_with_associated_resource(&self, resource: &URI) -> bool {
        *resource.scheme() == schemas::UNTITLED && resource.path().len() > 1 && !is_untitled_without_associated_resource_path(resource.path().units())
    }

    fn can_dispose(&self, model: &Rc<dyn IUntitledTextEditorModel>) -> Option<Promise<()>> {
        if IEditorModel::is_disposed(model.as_ref()) {
            return None; // quick return if model already disposed
        }

        // promise based return in all other cases
        Some(self.do_can_dispose(model.clone()))
    }
}

impl IUntitledTextEditorModelManager for UntitledTextEditorService {
    fn notify_did_save(&self, source: &URI, target: &URI) {
        self.on_did_save.fire(UntitledTextEditorModelSaveEvent { source: source.clone(), target: target.clone() });
    }
}
