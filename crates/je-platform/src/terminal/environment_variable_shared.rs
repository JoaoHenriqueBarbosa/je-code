//! Porte de `src/vs/platform/terminal/common/environmentVariableShared.ts`: a serialização das coleções
//! de variáveis de ambiente (arquivo compartilhado entre o renderer e o host de extensões).

use je_base::map::OrderedMap;

use super::environment_variable::{
    IEnvironmentVariableCollection, IEnvironmentVariableCollectionDescription, IEnvironmentVariableMutator, ISerializableEnvironmentDescriptionMap,
    ISerializableEnvironmentVariableCollection, ISerializableEnvironmentVariableCollections,
};

pub fn serialize_environment_variable_collection(collection: &OrderedMap<String, IEnvironmentVariableMutator>) -> ISerializableEnvironmentVariableCollection {
    collection.iter().map(|(key, mutator)| (key.clone(), mutator.clone())).collect()
}

pub fn serialize_environment_description_map(
    description_map: Option<&OrderedMap<String, IEnvironmentVariableCollectionDescription>>,
) -> ISerializableEnvironmentDescriptionMap {
    description_map.map(|map| map.iter().map(|(key, description)| (key.clone(), description.clone())).collect()).unwrap_or_default()
}

/// `new Map(entries)`: uma chave repetida sobrescreve o valor e mantém a primeira posição.
fn ordered_map_from_entries<V>(entries: impl IntoIterator<Item = (String, V)>) -> OrderedMap<String, V> {
    let mut map = OrderedMap::new();
    for (key, value) in entries {
        map.insert(key, value);
    }
    map
}

pub fn deserialize_environment_variable_collection(serialized_collection: ISerializableEnvironmentVariableCollection) -> OrderedMap<String, IEnvironmentVariableMutator> {
    ordered_map_from_entries(serialized_collection)
}

pub fn deserialize_environment_description_map(
    serializable_environment_description: Option<ISerializableEnvironmentDescriptionMap>,
) -> OrderedMap<String, IEnvironmentVariableCollectionDescription> {
    ordered_map_from_entries(serializable_environment_description.unwrap_or_default())
}

pub fn serialize_environment_variable_collections(collections: &OrderedMap<String, IEnvironmentVariableCollection>) -> ISerializableEnvironmentVariableCollections {
    collections
        .iter()
        .map(|(extension, collection)| {
            (
                extension.clone(),
                serialize_environment_variable_collection(&collection.map),
                serialize_environment_description_map(collection.description_map.as_ref()),
            )
        })
        .collect()
}

pub fn deserialize_environment_variable_collections(serialized_collection: ISerializableEnvironmentVariableCollections) -> OrderedMap<String, IEnvironmentVariableCollection> {
    ordered_map_from_entries(serialized_collection.into_iter().map(|(extension, map, description_map)| {
        (
            extension,
            IEnvironmentVariableCollection {
                map: deserialize_environment_variable_collection(map),
                description_map: Some(deserialize_environment_description_map(Some(description_map))),
            },
        )
    }))
}
