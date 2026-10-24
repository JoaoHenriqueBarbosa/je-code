//! Porte de `src/vs/workbench/contrib/terminal/common/environmentVariable.ts`: o serviço que guarda as coleções de
//! variáveis de ambiente das extensões (`IEnvironmentVariableService`) e a informação de uma coleção que um terminal usa
//! (`IEnvironmentVariableInfo`).
//!
//! Divergências:
//! - `collections` e `mergedCollection` são métodos (`ReadonlyMap` é o [`OrderedMap`] que o método devolve por valor; a
//!   coleção mesclada é um `Rc` do trait, que o gerenciador de processo guarda).
//! - `IEnvironmentVariableCollectionWithPersistence` é a struct de campos `map`, `description_map` e `persistent` (Rust não
//!   tem `extends`); [`IEnvironmentVariableCollectionWithPersistence::collection`] devolve a parte `IEnvironmentVariableCollection`.
//! - O `instanceof EnvironmentVariableInfoStale` do gerenciador de processo é o `requiresAction`, que só a informação
//!   "stale" tem verdadeiro.

use std::rc::Rc;

use je_base::event::Event;
use je_base::map::OrderedMap;
use je_instantiation::service_id;
use je_platform::terminal::environment_variable::{
    EnvironmentVariableScope, IEnvironmentVariableCollection, IEnvironmentVariableCollectionDescription, IEnvironmentVariableMutator,
    IMergedEnvironmentVariableCollection,
};

use super::terminal::ITerminalStatus;

/// `IEnvironmentVariableCollectionWithPersistence`: uma coleção de extensão e o aviso de que ela deve ser guardada entre sessões.
#[derive(Clone, Debug, Default)]
pub struct IEnvironmentVariableCollectionWithPersistence {
    pub map: OrderedMap<String, IEnvironmentVariableMutator>,
    pub description_map: Option<OrderedMap<String, IEnvironmentVariableCollectionDescription>>,
    pub persistent: bool,
}

impl IEnvironmentVariableCollectionWithPersistence {
    /// A coleção sem o `persistent` (a parte `IEnvironmentVariableCollection` do original).
    pub fn collection(&self) -> IEnvironmentVariableCollection {
        IEnvironmentVariableCollection { map: self.map.clone(), description_map: self.description_map.clone() }
    }
}

/// `IEnvironmentVariableService`: tracks and persists environment variable collections as defined by extensions.
pub trait IEnvironmentVariableService {
    /// Gets all the collections, by extension.
    fn collections(&self) -> OrderedMap<String, IEnvironmentVariableCollectionWithPersistence>;

    /// Gets a single collection constructed by merging all environment variable collections into one.
    fn merged_collection(&self) -> Rc<dyn IMergedEnvironmentVariableCollection>;

    /// An event that is fired when an extension's environment variable collection changes, the event provides the new
    /// merged collection.
    fn on_did_change_collections(&self) -> Event<Rc<dyn IMergedEnvironmentVariableCollection>>;

    /// Sets an extension's environment variable collection.
    fn set(&self, extension_identifier: &str, collection: IEnvironmentVariableCollectionWithPersistence);

    /// Deletes an extension's environment variable collection.
    fn delete(&self, extension_identifier: &str);
}

service_id!(pub I_ENVIRONMENT_VARIABLE_SERVICE: dyn IEnvironmentVariableService = "environmentVariableService");

/// `IEnvironmentVariableInfo`.
pub trait IEnvironmentVariableInfo {
    fn requires_action(&self) -> bool;
    fn get_status(&self, scope: Option<&EnvironmentVariableScope>) -> ITerminalStatus;
}
