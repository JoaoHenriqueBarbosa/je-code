//! Porte de `src/vs/platform/userDataSync/common/snippetsMerge.ts`: o merge de três vias dos snippets
//! (local, remoto e base) da sincronização de dados do usuário.
//!
//! Representação:
//! - `IStringDictionary<string>` é `OrderedMap<JsString, JsString>`: a ordem de inserção do objeto do
//!   JavaScript decide a ordem de `removed` e de `conflicts` no resultado, então um `HashMap` não serve.
//! - `Set<string>` também vira `OrderedMap<JsString, ()>` (ver [`KeySet`]), pelo mesmo motivo.
//! - `remote` e `base` nulos do original são `Option<&Dictionary>`.

use je_base::JsString;
use je_base::map::OrderedMap;

/// `IStringDictionary<string>`: nome do arquivo de snippet para o conteúdo dele.
pub type Dictionary = OrderedMap<JsString, JsString>;

/// `Set<string>` com a ordem de inserção.
type KeySet = OrderedMap<JsString, ()>;

/// Uma das duas pontas do resultado (`IMergeResult.local` e `IMergeResult.remote`).
#[derive(Clone, Debug, Default)]
pub struct SideResult {
    pub added: Dictionary,
    pub updated: Dictionary,
    pub removed: Vec<JsString>,
}

/// `IMergeResult`.
#[derive(Clone, Debug, Default)]
pub struct MergeResult {
    pub local: SideResult,
    pub remote: SideResult,
    pub conflicts: Vec<JsString>,
}

/// O que `compare` devolve: as chaves adicionadas, removidas e atualizadas de `from` para `to`.
struct Comparison {
    added: KeySet,
    removed: KeySet,
    updated: KeySet,
}

impl Comparison {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.updated.is_empty()
    }
}

fn keys_of(set: &KeySet) -> Vec<JsString> {
    set.keys().cloned().collect()
}

fn add(set: &mut KeySet, key: &JsString) {
    set.insert(key.clone(), ());
}

/// `merge`.
pub fn merge(local: &Dictionary, remote: Option<&Dictionary>, base: Option<&Dictionary>) -> MergeResult {
    let mut local_added = Dictionary::new();
    let mut local_updated = Dictionary::new();
    let mut local_removed = KeySet::new();

    let Some(remote) = remote else {
        return MergeResult {
            local: SideResult { added: local_added, updated: local_updated, removed: keys_of(&local_removed) },
            remote: SideResult { added: local.clone(), updated: Dictionary::new(), removed: Vec::new() },
            conflicts: Vec::new(),
        };
    };

    let local_to_remote = compare(Some(local), Some(remote));
    if local_to_remote.is_empty() {
        // Nenhuma mudança entre o local e o remoto.
        return MergeResult {
            local: SideResult { added: local_added, updated: local_updated, removed: keys_of(&local_removed) },
            remote: SideResult::default(),
            conflicts: Vec::new(),
        };
    }

    let base_to_local = compare(base, Some(local));
    let base_to_remote = compare(base, Some(remote));

    let mut remote_added = Dictionary::new();
    let mut remote_updated = Dictionary::new();
    let mut remote_removed = KeySet::new();

    let mut conflicts = KeySet::new();

    // Snippets removidos no local.
    for key in base_to_local.removed.keys() {
        if base_to_remote.updated.contains_key(key) {
            // Conflito: foi atualizado no remoto. Adiciona no local.
            if let Some(value) = remote.get(key) {
                local_added.insert(key.clone(), value.clone());
            }
        } else {
            // Remove no remoto.
            add(&mut remote_removed, key);
        }
    }

    // Snippets removidos no remoto.
    for key in base_to_remote.removed.keys() {
        if conflicts.contains_key(key) {
            continue;
        }
        if base_to_local.updated.contains_key(key) {
            // Conflito: foi atualizado no local.
            add(&mut conflicts, key);
        } else {
            // Remove também no local.
            add(&mut local_removed, key);
        }
    }

    // Snippets atualizados no local.
    for key in base_to_local.updated.keys() {
        if conflicts.contains_key(key) {
            continue;
        }
        if base_to_remote.updated.contains_key(key) {
            // Também foi atualizado no remoto; conflito se os valores são diferentes.
            if local_to_remote.updated.contains_key(key) {
                add(&mut conflicts, key);
            }
        } else if let Some(value) = local.get(key) {
            remote_updated.insert(key.clone(), value.clone());
        }
    }

    // Snippets atualizados no remoto.
    for key in base_to_remote.updated.keys() {
        if conflicts.contains_key(key) {
            continue;
        }
        if base_to_local.updated.contains_key(key) {
            // Também foi atualizado no local; conflito se os valores são diferentes.
            if local_to_remote.updated.contains_key(key) {
                add(&mut conflicts, key);
            }
        } else if local.contains_key(key) {
            if let Some(value) = remote.get(key) {
                local_updated.insert(key.clone(), value.clone());
            }
        }
    }

    // Snippets adicionados no local.
    for key in base_to_local.added.keys() {
        if conflicts.contains_key(key) {
            continue;
        }
        if base_to_remote.added.contains_key(key) {
            // Também foi adicionado no remoto; conflito se os valores são diferentes.
            if local_to_remote.updated.contains_key(key) {
                add(&mut conflicts, key);
            }
        } else if let Some(value) = local.get(key) {
            remote_added.insert(key.clone(), value.clone());
        }
    }

    // Snippets adicionados no remoto.
    for key in base_to_remote.added.keys() {
        if conflicts.contains_key(key) {
            continue;
        }
        if base_to_local.added.contains_key(key) {
            // Também foi adicionado no local; conflito se os valores são diferentes.
            if local_to_remote.updated.contains_key(key) {
                add(&mut conflicts, key);
            }
        } else if let Some(value) = remote.get(key) {
            local_added.insert(key.clone(), value.clone());
        }
    }

    MergeResult {
        local: SideResult { added: local_added, removed: keys_of(&local_removed), updated: local_updated },
        remote: SideResult { added: remote_added, removed: keys_of(&remote_removed), updated: remote_updated },
        conflicts: keys_of(&conflicts),
    }
}

fn compare(from: Option<&Dictionary>, to: Option<&Dictionary>) -> Comparison {
    let mut added = KeySet::new();
    let mut removed = KeySet::new();
    let mut updated = KeySet::new();

    if let Some(to) = to {
        for key in to.keys() {
            if !from.is_some_and(|f| f.contains_key(key)) {
                add(&mut added, key);
            }
        }
    }
    if let Some(from) = from {
        for (key, from_snippet) in from.iter() {
            match to.and_then(|t| t.get(key)) {
                None => add(&mut removed, key),
                Some(to_snippet) if to_snippet != from_snippet => add(&mut updated, key),
                Some(_) => {}
            }
        }
    }

    Comparison { added, removed, updated }
}

/// `areSame`.
pub fn are_same(a: &Dictionary, b: &Dictionary) -> bool {
    compare(Some(a), Some(b)).is_empty()
}
