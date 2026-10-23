//! Porte de `src/vs/platform/terminal/common/environmentVariableCollection.ts`: a coleção que mescla as
//! variáveis de ambiente contribuídas pelas extensões (um mutador por extensão e variável), aplica o
//! resultado a um ambiente de processo e calcula a diferença entre duas coleções.
//!
//! O `isWindows` do original, lido ao carregar o módulo, é o `Platform::detect().is_windows` do processo
//! atual, calculado uma vez.

use std::sync::OnceLock;

use je_base::map::OrderedMap;
use je_base::platform::Platform;
use je_base::process::ProcessEnvironment;

use super::environment_variable::{
    ApplyFuture, EnvironmentVariableMutatorType, EnvironmentVariableScope, IEnvironmentVariableCollection, IExtensionOwnedEnvironmentDescriptionMutator,
    IExtensionOwnedEnvironmentVariableMutator, IMergedEnvironmentVariableCollection, IMergedEnvironmentVariableCollectionDiff, VariableResolver,
};

const PYTHON_ACTIVATION_VARS_PREFIX: &str = "VSCODE_PYTHON_";
const PYTHON_ACTIVATION_VARS_SHELLS: [&str; 4] = ["PWSH", "ZSH", "BASH", "FISH"];
const PYTHON_ACTIVATION_VARS_SUFFIX: &str = "_ACTIVATE";
const PYTHON_ENV_EXTENSION_ID: &str = "ms-python.vscode-python-envs";

/// `isWindows` do `platform.ts`.
fn is_windows() -> bool {
    static IS_WINDOWS: OnceLock<bool> = OnceLock::new();
    *IS_WINDOWS.get_or_init(|| Platform::detect().is_windows)
}

/// `mutatorTypeToLabelMap`.
fn mutator_type_to_label(mutator_type: EnvironmentVariableMutatorType) -> &'static str {
    match mutator_type {
        EnvironmentVariableMutatorType::Append => "APPEND",
        EnvironmentVariableMutatorType::Prepend => "PREPEND",
        EnvironmentVariableMutatorType::Replace => "REPLACE",
    }
}

/// `PYTHON_ACTIVATION_VARS_PATTERN.test(variable)`: `/^VSCODE_PYTHON_(PWSH|ZSH|BASH|FISH)_ACTIVATE/`.
fn matches_python_activation_vars(variable: &str) -> bool {
    let Some(rest) = variable.strip_prefix(PYTHON_ACTIVATION_VARS_PREFIX) else {
        return false;
    };
    PYTHON_ACTIVATION_VARS_SHELLS
        .iter()
        .any(|shell| rest.strip_prefix(shell).is_some_and(|after| after.starts_with(PYTHON_ACTIVATION_VARS_SUFFIX)))
}

/// Represents an environment variable collection that results from merging several collections
/// together.
pub struct MergedEnvironmentVariableCollection {
    map: OrderedMap<String, Vec<IExtensionOwnedEnvironmentVariableMutator>>,
    description_map: OrderedMap<String, Vec<IExtensionOwnedEnvironmentDescriptionMutator>>,
    collections: OrderedMap<String, IEnvironmentVariableCollection>,
}

impl MergedEnvironmentVariableCollection {
    pub fn new(collections: OrderedMap<String, IEnvironmentVariableCollection>) -> Self {
        let mut merged = Self { map: OrderedMap::new(), description_map: OrderedMap::new(), collections: OrderedMap::new() };
        for (extension_identifier, collection) in collections.iter() {
            merged.populate_description_map(collection, extension_identifier);
            for (key, mutator) in collection.map.iter() {
                if Self::block_python_activation_var(key, extension_identifier) {
                    continue;
                }

                if !merged.map.contains_key(key) {
                    merged.map.insert(key.clone(), Vec::new());
                }
                let entry = merged.map.get_mut(key).expect("acabou de inserir");

                // If the first item in the entry is replace ignore any other entries as they would
                // just get replaced by this one.
                if entry.first().is_some_and(|first| first.mutator_type == EnvironmentVariableMutatorType::Replace) {
                    continue;
                }

                let extension_mutator = IExtensionOwnedEnvironmentVariableMutator {
                    extension_identifier: extension_identifier.clone(),
                    value: mutator.value.clone(),
                    mutator_type: mutator.mutator_type,
                    scope: mutator.scope.clone(),
                    variable: mutator.variable.clone(),
                    options: mutator.options,
                };
                // Mutators get applied in the reverse order than they are created
                entry.insert(0, extension_mutator);
            }
        }
        merged.collections = collections;
        merged
    }

    fn encode_colons(value: &str) -> String {
        value.replace(':', "\\x3a")
    }

    fn block_python_activation_var(variable: &str, extension_identifier: &str) -> bool {
        // Only Python env extension can modify Python activate env var.
        matches_python_activation_vars(variable) && PYTHON_ENV_EXTENSION_ID != extension_identifier
    }

    fn populate_description_map(&mut self, collection: &IEnvironmentVariableCollection, extension_identifier: &str) {
        let Some(description_map) = &collection.description_map else {
            return;
        };
        for (key, mutator) in description_map.iter() {
            if !self.description_map.contains_key(key) {
                self.description_map.insert(key.clone(), Vec::new());
            }
            let entry = self.description_map.get_mut(key).expect("acabou de inserir");
            entry.push(IExtensionOwnedEnvironmentDescriptionMutator {
                extension_identifier: extension_identifier.to_string(),
                scope: mutator.scope.clone(),
                description: mutator.description.clone(),
            });
        }
    }
}

impl IMergedEnvironmentVariableCollection for MergedEnvironmentVariableCollection {
    fn collections(&self) -> &OrderedMap<String, IEnvironmentVariableCollection> {
        &self.collections
    }

    fn apply_to_process_environment<'a>(
        &'a self,
        env: &'a mut ProcessEnvironment,
        scope: Option<&'a EnvironmentVariableScope>,
        variable_resolver: Option<VariableResolver<'a>>,
    ) -> ApplyFuture<'a> {
        Box::pin(async move {
            let windows = is_windows();
            let mut lower_to_actual_variable_names: OrderedMap<String, String> = OrderedMap::new();
            if windows {
                for name in env.keys() {
                    lower_to_actual_variable_names.insert(name.to_lowercase(), name.clone());
                }
            }
            for (variable, mutators) in self.get_variable_map(scope).iter() {
                let actual_variable = if windows {
                    match lower_to_actual_variable_names.get(&variable.to_lowercase()) {
                        Some(actual) if !actual.is_empty() => actual.clone(),
                        _ => variable.clone(),
                    }
                } else {
                    variable.clone()
                };
                for mutator in mutators {
                    let value = match variable_resolver {
                        Some(resolver) => resolver(&mutator.value).await?,
                        None => mutator.value.clone(),
                    };

                    if Self::block_python_activation_var(&mutator.variable, &mutator.extension_identifier) {
                        continue;
                    }

                    // Default: true
                    if mutator.options.and_then(|options| options.apply_at_process_creation).unwrap_or(true) {
                        let current = env.get(&actual_variable).cloned().unwrap_or_default();
                        match mutator.mutator_type {
                            EnvironmentVariableMutatorType::Append => {
                                env.insert(actual_variable.clone(), current + &value);
                            }
                            EnvironmentVariableMutatorType::Prepend => {
                                env.insert(actual_variable.clone(), value.clone() + &current);
                            }
                            EnvironmentVariableMutatorType::Replace => {
                                env.insert(actual_variable.clone(), value.clone());
                            }
                        }
                    }
                    // Default: false
                    if mutator.options.and_then(|options| options.apply_at_shell_integration).unwrap_or(false) {
                        let key = format!("VSCODE_ENV_{}", mutator_type_to_label(mutator.mutator_type));
                        let previous = env.get(&key).filter(|existing| !existing.is_empty()).map(|existing| format!("{existing}:")).unwrap_or_default();
                        env.insert(key, format!("{previous}{variable}={}", Self::encode_colons(&value)));
                    }
                }
            }
            Ok(())
        })
    }

    fn diff(&self, other: &dyn IMergedEnvironmentVariableCollection, scope: Option<&EnvironmentVariableScope>) -> Option<IMergedEnvironmentVariableCollectionDiff> {
        let mut added = OrderedMap::new();
        let mut changed = OrderedMap::new();
        let mut removed = OrderedMap::new();

        let current_map = self.get_variable_map(scope);
        let other_map = other.get_variable_map(scope);

        // Find added
        for (variable, other_mutators) in other_map.iter() {
            let current_mutators = current_map.get(variable);
            if let Some(result) = get_missing_mutators_from_array(other_mutators, current_mutators) {
                added.insert(variable.clone(), result);
            }
        }

        // Find removed
        for (variable, current_mutators) in current_map.iter() {
            let other_mutators = other_map.get(variable);
            if let Some(result) = get_missing_mutators_from_array(current_mutators, other_mutators) {
                removed.insert(variable.clone(), result);
            }
        }

        // Find changed
        for (variable, current_mutators) in current_map.iter() {
            let other_mutators = other_map.get(variable);
            if let Some(result) = get_changed_mutators_from_array(current_mutators, other_mutators) {
                changed.insert(variable.clone(), result);
            }
        }

        if added.is_empty() && changed.is_empty() && removed.is_empty() {
            return None;
        }

        Some(IMergedEnvironmentVariableCollectionDiff { added, changed, removed })
    }

    fn get_variable_map(&self, scope: Option<&EnvironmentVariableScope>) -> OrderedMap<String, Vec<IExtensionOwnedEnvironmentVariableMutator>> {
        let mut result = OrderedMap::new();
        for mutators in self.map.values() {
            let filtered_mutators: Vec<IExtensionOwnedEnvironmentVariableMutator> =
                mutators.iter().filter(|mutator| filter_scope(mutator.scope.as_ref(), scope, false)).cloned().collect();
            if let Some(first) = filtered_mutators.first() {
                // All of these mutators are for the same variable because they are in the same scope, hence choose anyone to form a key.
                result.insert(first.variable.clone(), filtered_mutators);
            }
        }
        result
    }

    fn get_description_map(&self, scope: Option<&EnvironmentVariableScope>) -> OrderedMap<String, Option<String>> {
        let mut result = OrderedMap::new();
        for mutators in self.description_map.values() {
            for mutator in mutators.iter().filter(|mutator| filter_scope(mutator.scope.as_ref(), scope, true)) {
                result.insert(mutator.extension_identifier.clone(), mutator.description.clone());
            }
        }
        result
    }
}

/// Returns whether a mutator matches with the scope provided.
/// `mutator_scope`: scope of the mutator to filter.
/// `scope`: scope to be used for querying.
/// `strict_filter`: if true, mutators with global scope is not returned when querying for workspace scope,
/// i.e. whether mutator scope should always exactly match with query scope.
fn filter_scope(mutator_scope: Option<&EnvironmentVariableScope>, scope: Option<&EnvironmentVariableScope>, strict_filter: bool) -> bool {
    let Some(mutator_scope) = mutator_scope else {
        if strict_filter {
            // `scope === mutator.scope`, com `mutator.scope` indefinido.
            return scope.is_none();
        }
        return true;
    };
    // If a mutator is scoped to a workspace folder, only apply it if the workspace
    // folder matches.
    if let (Some(mutator_folder), Some(folder)) = (&mutator_scope.workspace_folder, scope.and_then(|scope| scope.workspace_folder.as_ref()))
        && mutator_folder.index == folder.index
    {
        return true;
    }
    false
}

fn get_missing_mutators_from_array(
    current: &[IExtensionOwnedEnvironmentVariableMutator],
    other: Option<&Vec<IExtensionOwnedEnvironmentVariableMutator>>,
) -> Option<Vec<IExtensionOwnedEnvironmentVariableMutator>> {
    // If it doesn't exist, all are removed
    let Some(other) = other else {
        return Some(current.to_vec());
    };

    // Create a map to help
    let other_mutator_extensions: OrderedMap<&str, ()> = other.iter().fold(OrderedMap::new(), |mut set, mutator| {
        set.insert(mutator.extension_identifier.as_str(), ());
        set
    });

    // Find entries removed from other
    let result: Vec<IExtensionOwnedEnvironmentVariableMutator> =
        current.iter().filter(|mutator| !other_mutator_extensions.contains_key(&mutator.extension_identifier.as_str())).cloned().collect();

    if result.is_empty() { None } else { Some(result) }
}

fn get_changed_mutators_from_array(
    current: &[IExtensionOwnedEnvironmentVariableMutator],
    other: Option<&Vec<IExtensionOwnedEnvironmentVariableMutator>>,
) -> Option<Vec<IExtensionOwnedEnvironmentVariableMutator>> {
    // If it doesn't exist, none are changed (they are removed)
    let other = other?;

    // Create a map to help
    let mut other_mutator_extensions: OrderedMap<&str, &IExtensionOwnedEnvironmentVariableMutator> = OrderedMap::new();
    for mutator in other {
        other_mutator_extensions.insert(mutator.extension_identifier.as_str(), mutator);
    }

    // Find entries that exist in both but are not equal
    let folder_index = |mutator: &IExtensionOwnedEnvironmentVariableMutator| mutator.scope.as_ref().and_then(|scope| scope.workspace_folder.as_ref()).map(|folder| folder.index);
    let mut result = Vec::new();
    for mutator in current {
        if let Some(other_mutator) = other_mutator_extensions.get(&mutator.extension_identifier.as_str())
            && (mutator.mutator_type != other_mutator.mutator_type || mutator.value != other_mutator.value || folder_index(mutator) != folder_index(other_mutator))
        {
            // Return the new result, not the old one
            result.push((*other_mutator).clone());
        }
    }

    if result.is_empty() { None } else { Some(result) }
}
