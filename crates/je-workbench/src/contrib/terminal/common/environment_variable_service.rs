//! Porte de `src/vs/workbench/contrib/terminal/common/environmentVariableService.ts`: o `EnvironmentVariableService`, que
//! guarda as coleções de variáveis de ambiente que as extensões definem, as persiste no armazenamento do workspace e
//! avisa quando elas mudam. Os demais arquivos que o original importa já existem: `environmentVariableCollection.ts` e
//! `environmentVariableShared.ts` (em `je-platform`, `terminal/`) e `environmentVariable.ts` (`environment_variable.rs`).
//!
//! Divergências:
//! - **Decorators.** `@throttle(1000)` de `_persistCollectionsEventually` e `@debounce(1000)` de
//!   `_notifyCollectionUpdatesEventually` são os campos [`Throttle`] e [`Debounce`] de `je-base`, criados com o método
//!   fechado sobre um `Weak` do serviço.
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`].
//! - **`protected _persistCollections` e `_notifyCollectionUpdates`.** O `TestEnvironmentVariableService` do teste os expõe
//!   por subclasse; aqui são os métodos públicos [`EnvironmentVariableService::persist_collections`] e
//!   [`EnvironmentVariableService::notify_collection_updates`].
//! - **`mergedCollection` e `collections`.** São os métodos do trait (ver `environment_variable.rs`).
//! - **`JSON.stringify` e `JSON.parse`.** Os tipos serializáveis do porte são tuplas; a forma de texto que vai para o
//!   armazenamento é a do original (`{ extensionIdentifier, collection: [key, mutator][], description: [key, desc][] }`),
//!   com as funções `mutator_to_json` e companhia. Um texto inválido lança como o `JSON.parse` do construtor.
//! - **`_invalidateExtensionCollections`.** É uma tarefa local do runtime; uma rejeição de `whenInstalledExtensionsRegistered`
//!   vai para o tratador de erros inesperados (a rejeição sem consumidor do original).
//! - **Teste.** [`EnvironmentVariableService::on_did_change_collections_size_for_testing`] expõe o `_size` do emissor
//!   `_onDidChangeCollections`, que `terminalProcessManager.test.ts` lê por colchete.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::decorators::{Debounce, Throttle};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;
use je_base::runtime;
use je_base::uri::URI;
use je_instantiation::{Injectable, service_impl};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::terminal::environment_variable::{
    EnvironmentVariableMutatorType, EnvironmentVariableScope, IEnvironmentVariableCollection, IEnvironmentVariableCollectionDescription,
    IEnvironmentVariableMutator, IEnvironmentVariableMutatorOptions, IMergedEnvironmentVariableCollection, ISerializableEnvironmentDescriptionMap,
    ISerializableEnvironmentVariableCollection,
};
use je_platform::terminal::environment_variable_collection::MergedEnvironmentVariableCollection;
use je_platform::terminal::environment_variable_shared::{
    deserialize_environment_description_map, deserialize_environment_variable_collection, serialize_environment_description_map,
    serialize_environment_variable_collection,
};
use je_platform::workspace::WorkspaceFolder;
use serde_json::{Map, Value};

use super::environment_variable::{IEnvironmentVariableCollectionWithPersistence, IEnvironmentVariableService};
use super::terminal_storage_keys;
use crate::contrib::testing::common::serialized_json::{uri_from_json, uri_to_json};
use crate::services::extensions::extensions::IExtensionService;

/// O tempo do `@throttle` e do `@debounce` do original.
const DELAY: u64 = 1000;

type Obj = Map<String, Value>;

fn invalid(what: &str) -> Error {
    Error::new(format!("Unexpected environment variable collection storage format: {what}"))
}

/// `ISerializableExtensionEnvironmentVariableCollection`.
struct SerializableExtensionEnvironmentVariableCollection {
    extension_identifier: String,
    collection: ISerializableEnvironmentVariableCollection,
    description: Option<ISerializableEnvironmentDescriptionMap>,
}

fn scope_to_json(scope: &EnvironmentVariableScope) -> Value {
    let mut object = Obj::new();
    if let Some(folder) = &scope.workspace_folder {
        let mut folder_object = Obj::new();
        folder_object.insert("uri".to_owned(), uri_to_json(&folder.uri.to_json()));
        folder_object.insert("name".to_owned(), Value::String(folder.name.clone()));
        folder_object.insert("index".to_owned(), Value::from(folder.index as u64));
        object.insert("workspaceFolder".to_owned(), Value::Object(folder_object));
    }
    Value::Object(object)
}

fn scope_from_json(value: &Value) -> Result<EnvironmentVariableScope, Error> {
    let object = value.as_object().ok_or_else(|| invalid("scope"))?;
    let workspace_folder = match object.get("workspaceFolder") {
        None | Some(Value::Null) => None,
        Some(folder) => {
            let folder = folder.as_object().ok_or_else(|| invalid("workspaceFolder"))?;
            let state = uri_from_json(folder.get("uri").ok_or_else(|| invalid("workspaceFolder.uri"))?)?;
            let uri = URI::revive(&state, je_platform::resources::host_platform());
            let name = folder.get("name").and_then(Value::as_str).ok_or_else(|| invalid("workspaceFolder.name"))?;
            let index = folder.get("index").and_then(Value::as_u64).ok_or_else(|| invalid("workspaceFolder.index"))?;
            Some(WorkspaceFolder::new(uri, name, index as usize, None))
        }
    };
    Ok(EnvironmentVariableScope { workspace_folder })
}

fn options_to_json(options: &IEnvironmentVariableMutatorOptions) -> Value {
    let mut object = Obj::new();
    if let Some(value) = options.apply_at_process_creation {
        object.insert("applyAtProcessCreation".to_owned(), Value::Bool(value));
    }
    if let Some(value) = options.apply_at_shell_integration {
        object.insert("applyAtShellIntegration".to_owned(), Value::Bool(value));
    }
    Value::Object(object)
}

fn options_from_json(value: &Value) -> Result<IEnvironmentVariableMutatorOptions, Error> {
    let object = value.as_object().ok_or_else(|| invalid("options"))?;
    Ok(IEnvironmentVariableMutatorOptions {
        apply_at_process_creation: object.get("applyAtProcessCreation").and_then(Value::as_bool),
        apply_at_shell_integration: object.get("applyAtShellIntegration").and_then(Value::as_bool),
    })
}

fn mutator_to_json(mutator: &IEnvironmentVariableMutator) -> Value {
    let mut object = Obj::new();
    object.insert("variable".to_owned(), Value::String(mutator.variable.clone()));
    object.insert("value".to_owned(), Value::String(mutator.value.clone()));
    object.insert("type".to_owned(), Value::from(mutator.mutator_type as i32));
    if let Some(scope) = &mutator.scope {
        object.insert("scope".to_owned(), scope_to_json(scope));
    }
    if let Some(options) = &mutator.options {
        object.insert("options".to_owned(), options_to_json(options));
    }
    Value::Object(object)
}

fn mutator_from_json(value: &Value) -> Result<IEnvironmentVariableMutator, Error> {
    let object = value.as_object().ok_or_else(|| invalid("mutator"))?;
    let mutator_type = match object.get("type").and_then(Value::as_i64) {
        Some(1) => EnvironmentVariableMutatorType::Replace,
        Some(2) => EnvironmentVariableMutatorType::Append,
        Some(3) => EnvironmentVariableMutatorType::Prepend,
        _ => return Err(invalid("mutator.type")),
    };
    Ok(IEnvironmentVariableMutator {
        variable: object.get("variable").and_then(Value::as_str).ok_or_else(|| invalid("mutator.variable"))?.to_owned(),
        value: object.get("value").and_then(Value::as_str).ok_or_else(|| invalid("mutator.value"))?.to_owned(),
        mutator_type,
        scope: object.get("scope").filter(|scope| !scope.is_null()).map(scope_from_json).transpose()?,
        options: object.get("options").filter(|options| !options.is_null()).map(options_from_json).transpose()?,
    })
}

fn description_to_json(description: &IEnvironmentVariableCollectionDescription) -> Value {
    let mut object = Obj::new();
    if let Some(text) = &description.description {
        object.insert("description".to_owned(), Value::String(text.clone()));
    }
    if let Some(scope) = &description.scope {
        object.insert("scope".to_owned(), scope_to_json(scope));
    }
    Value::Object(object)
}

fn description_from_json(value: &Value) -> Result<IEnvironmentVariableCollectionDescription, Error> {
    let object = value.as_object().ok_or_else(|| invalid("description"))?;
    Ok(IEnvironmentVariableCollectionDescription {
        description: object.get("description").and_then(Value::as_str).map(str::to_owned),
        scope: object.get("scope").filter(|scope| !scope.is_null()).map(scope_from_json).transpose()?,
    })
}

/// `[key, value][]` como a lista de pares do `JSON.stringify` de um `[...map.entries()]`.
fn pairs_to_json<V>(pairs: &[(String, V)], to_json: impl Fn(&V) -> Value) -> Value {
    Value::Array(pairs.iter().map(|(key, value)| Value::Array(vec![Value::String(key.clone()), to_json(value)])).collect())
}

fn pairs_from_json<V>(value: &Value, what: &str, from_json: impl Fn(&Value) -> Result<V, Error>) -> Result<Vec<(String, V)>, Error> {
    value
        .as_array()
        .ok_or_else(|| invalid(what))?
        .iter()
        .map(|pair| {
            let pair = pair.as_array().filter(|pair| pair.len() == 2).ok_or_else(|| invalid(what))?;
            let key = pair[0].as_str().ok_or_else(|| invalid(what))?.to_owned();
            Ok((key, from_json(&pair[1])?))
        })
        .collect()
}

fn extension_collection_from_json(value: &Value) -> Result<SerializableExtensionEnvironmentVariableCollection, Error> {
    let object = value.as_object().ok_or_else(|| invalid("collection"))?;
    Ok(SerializableExtensionEnvironmentVariableCollection {
        extension_identifier: object.get("extensionIdentifier").and_then(Value::as_str).ok_or_else(|| invalid("extensionIdentifier"))?.to_owned(),
        collection: pairs_from_json(object.get("collection").ok_or_else(|| invalid("collection"))?, "collection", mutator_from_json)?,
        description: match object.get("description") {
            None | Some(Value::Null) => None,
            Some(description) => Some(pairs_from_json(description, "description", description_from_json)?),
        },
    })
}

/// Tracks and persists environment variable collections as defined by extensions.
pub struct EnvironmentVariableService {
    this: Weak<Self>,
    base: Rc<Disposable>,
    extension_service: Rc<dyn IExtensionService>,
    storage_service: Rc<dyn StorageService>,

    collections: RefCell<OrderedMap<String, IEnvironmentVariableCollectionWithPersistence>>,
    merged_collection: RefCell<Rc<dyn IMergedEnvironmentVariableCollection>>,

    on_did_change_collections: Rc<Emitter<Rc<dyn IMergedEnvironmentVariableCollection>>>,
    persist_collections_eventually: Throttle<(), ()>,
    notify_collection_updates_eventually: Debounce<(), ()>,
}

impl Injectable for EnvironmentVariableService {
    type Args = ();
    type Deps = (Rc<dyn IExtensionService>, Rc<dyn StorageService>);

    fn create(_: (), (extension_service, storage_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_collections = Emitter::new();
        base.register(on_did_change_collections.clone());

        storage_service.remove(terminal_storage_keys::DEPRECATED_ENVIRONMENT_VARIABLE_COLLECTIONS, StorageScope::Workspace);
        let serialized_persisted_collections = storage_service.get(terminal_storage_keys::ENVIRONMENT_VARIABLE_COLLECTIONS, StorageScope::Workspace, None);
        let mut collections: OrderedMap<String, IEnvironmentVariableCollectionWithPersistence> = OrderedMap::new();
        let has_persisted_collections = serialized_persisted_collections.as_deref().is_some_and(|serialized| !serialized.is_empty());
        if let Some(serialized) = serialized_persisted_collections.filter(|serialized| !serialized.is_empty()) {
            let parsed = serde_json::from_str::<Value>(&serialized)
                .map_err(|error| Error::named("SyntaxError", error.to_string()))
                .and_then(|json| json.as_array().ok_or_else(|| invalid("collections"))?.iter().map(extension_collection_from_json).collect::<Result<Vec<_>, _>>());
            let collections_json = parsed.unwrap_or_else(|error| errors::throw(error));
            for collection in collections_json {
                collections.insert(
                    collection.extension_identifier,
                    IEnvironmentVariableCollectionWithPersistence {
                        persistent: true,
                        map: deserialize_environment_variable_collection(collection.collection),
                        description_map: Some(deserialize_environment_description_map(collection.description)),
                    },
                );
            }
        }

        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let persist_weak = weak.clone();
            let notify_weak = weak.clone();
            let merged_collection = Self::resolve_merged(&collections);
            Self {
                this: weak.clone(),
                base: base.clone(),
                extension_service,
                storage_service,
                collections: RefCell::new(collections),
                merged_collection: RefCell::new(merged_collection),
                on_did_change_collections,
                persist_collections_eventually: Throttle::new(
                    DELAY,
                    |_, _| (),
                    None,
                    move |_| {
                        if let Some(this) = persist_weak.upgrade() {
                            this.persist_collections();
                        }
                    },
                ),
                notify_collection_updates_eventually: Debounce::without_reducer(DELAY, move |_| {
                    if let Some(this) = notify_weak.upgrade() {
                        this.notify_collection_updates();
                    }
                }),
            }
        });

        if has_persisted_collections {
            // Asynchronously invalidate collections where extensions have been uninstalled, this is async to avoid making all
            // functions on the service synchronous and because extensions being uninstalled is rare.
            this.invalidate_extension_collections();
        }

        // Listen for uninstalled/disabled extensions
        let weak = Rc::downgrade(&this);
        this.base.register(this.extension_service.on_did_change_extensions().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.invalidate_extension_collections();
            }
        }));

        this
    }
}

service_impl!(EnvironmentVariableService => dyn IEnvironmentVariableService, disposable);

impl IDisposable for EnvironmentVariableService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EnvironmentVariableService {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço de variáveis de ambiente foi descartado da memória")
    }

    fn update_collections(&self) {
        self.persist_collections_eventually.call(());
        *self.merged_collection.borrow_mut() = Self::resolve_merged(&self.collections.borrow());
        self.notify_collection_updates_eventually.call(());
    }

    /// `_persistCollections` (`protected` no original; o `TestEnvironmentVariableService` do teste o expõe).
    pub fn persist_collections(&self) {
        let collections_json: Vec<Value> = self
            .collections
            .borrow()
            .iter()
            .filter(|(_, collection)| collection.persistent)
            .map(|(extension_identifier, collection)| {
                let mut object = Obj::new();
                object.insert("extensionIdentifier".to_owned(), Value::String(extension_identifier.clone()));
                object.insert("collection".to_owned(), pairs_to_json(&serialize_environment_variable_collection(&collection.map), mutator_to_json));
                object.insert("description".to_owned(), pairs_to_json(&serialize_environment_description_map(collection.description_map.as_ref()), description_to_json));
                Value::Object(object)
            })
            .collect();
        let stringified_json = Value::Array(collections_json).to_string();
        self.storage_service.store(
            terminal_storage_keys::ENVIRONMENT_VARIABLE_COLLECTIONS,
            stringified_json.as_str().into(),
            StorageScope::Workspace,
            StorageTarget::Machine,
        );
    }

    /// `_notifyCollectionUpdates` (`protected` no original; o `TestEnvironmentVariableService` do teste o expõe).
    pub fn notify_collection_updates(&self) {
        let merged = self.merged_collection.borrow().clone();
        self.on_did_change_collections.fire(merged);
    }

    fn resolve_merged(collections: &OrderedMap<String, IEnvironmentVariableCollectionWithPersistence>) -> Rc<dyn IMergedEnvironmentVariableCollection> {
        let mut plain: OrderedMap<String, IEnvironmentVariableCollection> = OrderedMap::new();
        for (extension_identifier, collection) in collections.iter() {
            plain.insert(extension_identifier.clone(), collection.collection());
        }
        Rc::new(MergedEnvironmentVariableCollection::new(plain))
    }

    fn invalidate_extension_collections(&self) {
        let this = self.rc();
        runtime::spawn_local(async move {
            if let Err(error) = this.extension_service.when_installed_extensions_registered().await {
                errors::on_unexpected_error(&error);
                return;
            }
            let registered_extensions = this.extension_service.extensions();
            let unregistered: Vec<String> = this
                .collections
                .borrow()
                .iter()
                .map(|(extension_identifier, _)| extension_identifier.clone())
                .filter(|extension_identifier| !registered_extensions.iter().any(|registered| registered.identifier.value == *extension_identifier))
                .collect();
            let changes = !unregistered.is_empty();
            for extension_identifier in unregistered {
                this.collections.borrow_mut().remove(&extension_identifier);
            }
            if changes {
                this.update_collections();
            }
        });
    }

    /// O `_size` do emissor `_onDidChangeCollections`, que `terminalProcessManager.test.ts` lê por colchete.
    #[doc(hidden)]
    pub fn on_did_change_collections_size_for_testing(&self) -> usize {
        self.on_did_change_collections.size_for_testing()
    }
}

impl IEnvironmentVariableService for EnvironmentVariableService {
    fn collections(&self) -> OrderedMap<String, IEnvironmentVariableCollectionWithPersistence> {
        self.collections.borrow().clone()
    }

    fn merged_collection(&self) -> Rc<dyn IMergedEnvironmentVariableCollection> {
        self.merged_collection.borrow().clone()
    }

    fn on_did_change_collections(&self) -> Event<Rc<dyn IMergedEnvironmentVariableCollection>> {
        self.on_did_change_collections.event()
    }

    fn set(&self, extension_identifier: &str, collection: IEnvironmentVariableCollectionWithPersistence) {
        self.collections.borrow_mut().insert(extension_identifier.to_owned(), collection);
        self.update_collections();
    }

    fn delete(&self, extension_identifier: &str) {
        self.collections.borrow_mut().remove(&extension_identifier.to_owned());
        self.update_collections();
    }
}
