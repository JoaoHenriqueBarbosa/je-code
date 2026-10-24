//! Porte de `src/vs/workbench/contrib/terminal/common/environmentVariable.ts`: o serviço que guarda as coleções de
//! variáveis de ambiente das extensões (`IEnvironmentVariableService`) e a informação de uma coleção que um terminal usa
//! (`IEnvironmentVariableInfo`).
//!
//! Divergências:
//! - `collections` e `mergedCollection` são métodos (`ReadonlyMap` é o [`OrderedMap`] que o método devolve por valor; a
//!   coleção mesclada é um `Rc` do trait, que o gerenciador de processo guarda).
//! - `IEnvironmentVariableCollectionWithPersistence` (usada só pelo serviço de armazenamento) fica fora.
//! - O `instanceof EnvironmentVariableInfoStale` do gerenciador de processo é o `requiresAction`, que só a informação
//!   "stale" tem verdadeiro.

use std::rc::Rc;

use je_base::event::Event;
use je_base::map::OrderedMap;
use je_instantiation::service_id;
use je_platform::terminal::environment_variable::{EnvironmentVariableScope, IEnvironmentVariableCollection, IMergedEnvironmentVariableCollection};

use super::terminal::ITerminalStatus;

/// `IEnvironmentVariableService`: tracks and persists environment variable collections as defined by extensions.
pub trait IEnvironmentVariableService {
    /// Gets all the collections, by extension.
    fn collections(&self) -> OrderedMap<String, IEnvironmentVariableCollection>;

    /// Gets a single collection constructed by merging all environment variable collections into one.
    fn merged_collection(&self) -> Rc<dyn IMergedEnvironmentVariableCollection>;

    /// An event that is fired when an extension's environment variable collection changes, the event provides the new
    /// merged collection.
    fn on_did_change_collections(&self) -> Event<Rc<dyn IMergedEnvironmentVariableCollection>>;

    /// Sets an extension's environment variable collection.
    fn set(&self, extension_identifier: &str, collection: IEnvironmentVariableCollection);

    /// Deletes an extension's environment variable collection.
    fn delete(&self, extension_identifier: &str);
}

service_id!(pub I_ENVIRONMENT_VARIABLE_SERVICE: dyn IEnvironmentVariableService = "environmentVariableService");

/// `IEnvironmentVariableInfo`.
pub trait IEnvironmentVariableInfo {
    fn requires_action(&self) -> bool;
    fn get_status(&self, scope: Option<&EnvironmentVariableScope>) -> ITerminalStatus;
}
