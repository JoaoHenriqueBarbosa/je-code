//! Porte de `src/vs/platform/terminal/common/environmentVariable.ts`: os tipos das variáveis de ambiente
//! que as extensões contribuem ao terminal (mutadores, escopos, coleções e a coleção mesclada).
//!
//! Representação: o `Map` do JavaScript, que itera na ordem de inserção, é o [`OrderedMap`]. Os campos
//! opcionais (`?`) são `Option`. A interface `IMergedEnvironmentVariableCollection` é um trait, e o
//! `applyToProcessEnvironment` (assíncrono no original) devolve um futuro com caixa para o trait continuar
//! utilizável como objeto. O `IProcessEnvironment` é o [`ProcessEnvironment`] do `je-base`.

use std::future::Future;
use std::pin::Pin;

use je_base::errors::Error;
use je_base::map::OrderedMap;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;

use crate::workspace::WorkspaceFolder;

/// `EnvironmentVariableMutatorType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvironmentVariableMutatorType {
    Replace = 1,
    Append = 2,
    Prepend = 3,
}

/// `IEnvironmentVariableMutatorOptions`: `None` é o `undefined` de cada campo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IEnvironmentVariableMutatorOptions {
    pub apply_at_process_creation: Option<bool>,
    pub apply_at_shell_integration: Option<bool>,
}

/// `EnvironmentVariableScope`: `{ workspaceFolder?: IWorkspaceFolderData }`.
#[derive(Clone, Debug, Default)]
pub struct EnvironmentVariableScope {
    pub workspace_folder: Option<WorkspaceFolder>,
}

impl PartialEq for EnvironmentVariableScope {
    /// Igualdade estrutural do `deepStrictEqual`: o `IWorkspaceFolderData` é `{ uri, name, index }`.
    fn eq(&self, other: &Self) -> bool {
        match (&self.workspace_folder, &other.workspace_folder) {
            (None, None) => true,
            (Some(a), Some(b)) => a.uri.to_string(false) == b.uri.to_string(false) && a.name == b.name && a.index == b.index,
            _ => false,
        }
    }
}

/// `IEnvironmentVariableMutator`.
#[derive(Clone, Debug, PartialEq)]
pub struct IEnvironmentVariableMutator {
    pub variable: String,
    pub value: String,
    pub mutator_type: EnvironmentVariableMutatorType,
    pub scope: Option<EnvironmentVariableScope>,
    pub options: Option<IEnvironmentVariableMutatorOptions>,
}

/// `IEnvironmentVariableCollectionDescription`.
#[derive(Clone, Debug, PartialEq)]
pub struct IEnvironmentVariableCollectionDescription {
    pub description: Option<String>,
    pub scope: Option<EnvironmentVariableScope>,
}

/// `IEnvironmentVariableCollection`: `descriptionMap` é opcional.
#[derive(Clone, Debug, Default)]
pub struct IEnvironmentVariableCollection {
    pub map: OrderedMap<String, IEnvironmentVariableMutator>,
    pub description_map: Option<OrderedMap<String, IEnvironmentVariableCollectionDescription>>,
}

/// `ISerializableEnvironmentVariableCollection`: `[variable, mutator][]`.
pub type ISerializableEnvironmentVariableCollection = Vec<(String, IEnvironmentVariableMutator)>;

/// `ISerializableEnvironmentDescriptionMap`.
pub type ISerializableEnvironmentDescriptionMap = Vec<(String, IEnvironmentVariableCollectionDescription)>;

/// `IExtensionOwnedEnvironmentDescriptionMutator`.
#[derive(Clone, Debug, PartialEq)]
pub struct IExtensionOwnedEnvironmentDescriptionMutator {
    pub description: Option<String>,
    pub scope: Option<EnvironmentVariableScope>,
    pub extension_identifier: String,
}

/// `ISerializableEnvironmentVariableCollections`: `[extension, collection, description][]`.
pub type ISerializableEnvironmentVariableCollections = Vec<(String, ISerializableEnvironmentVariableCollection, ISerializableEnvironmentDescriptionMap)>;

/// `IExtensionOwnedEnvironmentVariableMutator`.
#[derive(Clone, Debug, PartialEq)]
pub struct IExtensionOwnedEnvironmentVariableMutator {
    pub variable: String,
    pub value: String,
    pub mutator_type: EnvironmentVariableMutatorType,
    pub scope: Option<EnvironmentVariableScope>,
    pub options: Option<IEnvironmentVariableMutatorOptions>,
    pub extension_identifier: String,
}

/// `IMergedEnvironmentVariableCollectionDiff`.
#[derive(Clone, Debug, Default)]
pub struct IMergedEnvironmentVariableCollectionDiff {
    pub added: OrderedMap<String, Vec<IExtensionOwnedEnvironmentVariableMutator>>,
    pub changed: OrderedMap<String, Vec<IExtensionOwnedEnvironmentVariableMutator>>,
    pub removed: OrderedMap<String, Vec<IExtensionOwnedEnvironmentVariableMutator>>,
}

/// `VariableResolver`: `(str: string) => Promise<string>`.
pub type VariableResolver<'a> = &'a dyn Fn(&str) -> Promise<String>;

/// O futuro de `applyToProcessEnvironment`: a promessa que rejeita (o resolvedor de variáveis lançou)
/// vira `Err`.
pub type ApplyFuture<'a> = Pin<Box<dyn Future<Output = Result<(), Error>> + 'a>>;

/// Represents an environment variable collection that results from merging several collections
/// together.
pub trait IMergedEnvironmentVariableCollection {
    fn collections(&self) -> &OrderedMap<String, IEnvironmentVariableCollection>;

    /// Gets the variable map for a given scope.
    /// `scope`: the scope to get the variable map for. If `None`, the global scope is used.
    fn get_variable_map(&self, scope: Option<&EnvironmentVariableScope>) -> OrderedMap<String, Vec<IExtensionOwnedEnvironmentVariableMutator>>;

    /// Gets the description map for a given scope.
    /// `scope`: the scope to get the description map for. If `None`, description map for the
    /// global scope is returned.
    fn get_description_map(&self, scope: Option<&EnvironmentVariableScope>) -> OrderedMap<String, Option<String>>;

    /// Applies this collection to a process environment.
    /// `variable_resolver`: an optional function to use to resolve variables within the
    /// environment values.
    fn apply_to_process_environment<'a>(
        &'a self,
        env: &'a mut ProcessEnvironment,
        scope: Option<&'a EnvironmentVariableScope>,
        variable_resolver: Option<VariableResolver<'a>>,
    ) -> ApplyFuture<'a>;

    /// Generates a diff of this collection against another. Returns `None` if the collections are
    /// the same.
    fn diff(&self, other: &dyn IMergedEnvironmentVariableCollection, scope: Option<&EnvironmentVariableScope>) -> Option<IMergedEnvironmentVariableCollectionDiff>;
}
