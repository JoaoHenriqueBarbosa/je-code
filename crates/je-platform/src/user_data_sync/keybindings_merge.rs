//! Porte de `src/vs/platform/userDataSync/common/keybindingsMerge.ts`: o merge de três vias do
//! `keybindings.json` (local, remoto e base) da sincronização de dados do usuário.
//!
//! Representação:
//! - O `IUserFriendlyKeybinding` do original é lido de um valor JSON qualquer (o `parse` não valida), então
//!   cada entrada guarda o valor JSON cru (`raw`, o que se escreve de volta, com as propriedades extras e a
//!   ordem delas) e os campos que a comparação olha (`key`, `command`, `when`, `args`). Campo que não é
//!   texto vale como ausente (o original lançaria `TypeError` em `command[0]`).
//! - `Set<string>` e `Map<string, ...>` do JavaScript iteram em ordem de inserção, e essa ordem decide a
//!   ordem das entradas no texto final; por isso [`StringSet`] e [`Grouped`] são listas ordenadas.
//! - `IStringDictionary<string>` é `HashMap<JsString, JsString>`. O contrato do serviço devolve uma entrada por
//!   tecla pedida; uma tecla sem entrada vale como texto vazio (no original seria `undefined`).
//! - O conteúdo é `&str`/`String` (UTF-8); a edição converte para UTF-16 internamente.

use std::collections::HashMap;

use je_base::JsString;
use je_base::arrays;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::json::{self, Value};
use je_base::json_edit::EditSegment;
use je_base::json_formatter::FormattingOptions;
use je_base::objects;

use super::content;
use super::user_data_sync::IUserDataSyncUtilService;
use crate::contextkey::ContextKeyExpr;
use crate::js::from_json;

type NormalizedKeys = HashMap<JsString, JsString>;

/// Um `IUserFriendlyKeybinding` lido do JSON.
#[derive(Clone)]
struct Keybinding {
    key: JsString,
    command: JsString,
    when: Option<String>,
    args: Dynamic,
    raw: Value,
}

fn is_dash(unit: Option<&u16>) -> bool {
    unit == Some(&u16::from(b'-'))
}

impl Keybinding {
    fn from_value(value: &Value) -> Self {
        let object = match value {
            Value::Object(object) => Some(object),
            _ => None,
        };
        let text = |name: &str| match object.and_then(|o| o.get(name)) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        Self {
            key: text("key").unwrap_or_default(),
            command: text("command").unwrap_or_default(),
            when: text("when").map(|w| w.to_string_lossy()),
            args: object.and_then(|o| o.get("args")).map_or(Dynamic::Undefined, from_json),
            raw: value.clone(),
        }
    }

    /// `{ ...keybinding, key }`.
    fn with_key(&self, key: &JsString) -> Self {
        Self { key: key.clone(), ..self.clone() }
    }

    /// `command[0] === '-'`: a entrada remove o atalho do comando.
    fn is_negated(&self) -> bool {
        is_dash(self.command.units().first())
    }

    /// O comando sem o `-` do início, se tiver.
    fn target_command(&self) -> JsString {
        if self.is_negated() { self.command.slice(1, self.command.len()) } else { self.command.clone() }
    }

    /// `command === ${name} || command === -${name}`.
    fn is_for_command(&self, name: &JsString) -> bool {
        self.command == *name || self.command == negated_command(name)
    }
}

/// `-${command}`.
fn negated_command(command: &JsString) -> JsString {
    let mut out = JsString::from("-");
    out.push_units(command.units());
    out
}

/// `Set<string>` na ordem de inserção.
#[derive(Clone, Default)]
struct StringSet(Vec<JsString>);

impl StringSet {
    fn insert(&mut self, value: JsString) {
        if !self.contains(&value) {
            self.0.push(value);
        }
    }

    fn contains(&self, value: &JsString) -> bool {
        self.0.contains(value)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn values(&self) -> Vec<JsString> {
        self.0.clone()
    }
}

impl FromIterator<JsString> for StringSet {
    fn from_iter<T: IntoIterator<Item = JsString>>(iter: T) -> Self {
        let mut set = Self::default();
        for value in iter {
            set.insert(value);
        }
        set
    }
}

/// `Map<string, IUserFriendlyKeybinding[]>` na ordem de inserção.
#[derive(Default)]
struct Grouped(Vec<(JsString, Vec<Keybinding>)>);

impl Grouped {
    fn push(&mut self, key: JsString, keybinding: Keybinding) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(keybinding),
            None => self.0.push((key, vec![keybinding])),
        }
    }

    fn get(&self, key: &JsString) -> Option<&[Keybinding]> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, group)| group.as_slice())
    }

    fn keys(&self) -> Vec<JsString> {
        self.0.iter().map(|(k, _)| k.clone()).collect()
    }
}

/// `ICompareResult`.
#[derive(Default)]
struct CompareResult {
    added: StringSet,
    removed: StringSet,
    updated: StringSet,
}

impl CompareResult {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.updated.is_empty()
    }

    /// O que se compara contra a ausência de base: tudo que existe é adicionado.
    fn all_added(keys: Vec<JsString>) -> Self {
        Self { added: keys.into_iter().collect(), ..Self::default() }
    }
}

/// O que `computeMergeResult` devolve.
#[derive(Default)]
struct MergeSets {
    added: StringSet,
    removed: StringSet,
    updated: StringSet,
    conflicts: StringSet,
}

/// `IMergeResult`: só o que o merge usa depois (`conflicts`).
struct KeybindingsMergeResult {
    has_local_forwarded: bool,
    has_remote_forwarded: bool,
    conflicts: StringSet,
}

/// O resultado de [`merge`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeResult {
    pub merge_content: String,
    pub has_changes: bool,
    pub has_conflicts: bool,
}

/// `parseKeybindings`: `parse(content) || []`. Conteúdo que não é uma lista vale como lista vazia.
fn parse_keybindings(content: &str) -> Vec<Keybinding> {
    let mut errors = Vec::new();
    match json::parse_str(content, &mut errors) {
        Some(Value::Array(items)) => items.iter().map(Keybinding::from_value).collect(),
        _ => Vec::new(),
    }
}

/// `normalizedKeys[key]`.
fn normalized(keys: &NormalizedKeys, key: &JsString) -> JsString {
    keys.get(key).cloned().unwrap_or_default()
}

/// `merge`: junta o conteúdo local e o remoto tendo `base_content` (a última versão sincronizada) como
/// ancestral comum. Um erro do serviço de utilidades (promessa rejeitada) volta como `Err`.
pub async fn merge(
    local_content: &str,
    remote_content: &str,
    base_content: Option<&str>,
    formatting_options: &FormattingOptions,
    user_data_sync_util_service: &dyn IUserDataSyncUtilService,
) -> Result<MergeResult, Error> {
    let local = parse_keybindings(local_content);
    let remote = parse_keybindings(remote_content);
    let base = base_content.filter(|b| !b.is_empty()).map(parse_keybindings);

    let userbindings: Vec<JsString> =
        local.iter().chain(remote.iter()).chain(base.iter().flatten()).map(|keybinding| keybinding.key.clone()).collect();
    let normalized_keys = user_data_sync_util_service.resolve_user_bindings(userbindings).await?;
    let keybindings_merge_result = compute_merge_result_by_keybinding(&local, &remote, base.as_deref(), &normalized_keys);

    if !keybindings_merge_result.has_local_forwarded && !keybindings_merge_result.has_remote_forwarded {
        // No changes found between local and remote.
        return Ok(MergeResult { merge_content: local_content.to_string(), has_changes: false, has_conflicts: false });
    }

    if !keybindings_merge_result.has_local_forwarded && keybindings_merge_result.has_remote_forwarded {
        return Ok(MergeResult { merge_content: remote_content.to_string(), has_changes: true, has_conflicts: false });
    }

    if keybindings_merge_result.has_local_forwarded && !keybindings_merge_result.has_remote_forwarded {
        // Local has moved forward and remote has not. Return local.
        return Ok(MergeResult { merge_content: local_content.to_string(), has_changes: true, has_conflicts: false });
    }

    // Both local and remote has moved forward.
    let local_by_command = by_command(&local);
    let remote_by_command = by_command(&remote);
    let base_by_command = base.as_deref().map(by_command);
    let local_to_remote_by_command = compare_by_command(&local_by_command, &remote_by_command, &normalized_keys);
    let base_to_local_by_command = match &base_by_command {
        Some(base) => compare_by_command(base, &local_by_command, &normalized_keys),
        None => CompareResult::all_added(local_by_command.keys()),
    };
    let base_to_remote_by_command = match &base_by_command {
        Some(base) => compare_by_command(base, &remote_by_command, &normalized_keys),
        None => CompareResult::all_added(remote_by_command.keys()),
    };

    let mut commands_merge_result =
        compute_merge_result(&local_to_remote_by_command, &base_to_local_by_command, &base_to_remote_by_command);
    let mut merge_content = local_content.to_string();

    // Removed commands in Remote
    for command in commands_merge_result.removed.values() {
        if commands_merge_result.conflicts.contains(&command) {
            continue;
        }
        merge_content = remove_keybindings(&merge_content, &command, formatting_options);
    }

    // Added commands in remote
    for command in commands_merge_result.added.values() {
        if commands_merge_result.conflicts.contains(&command) {
            continue;
        }
        let keybindings = remote_by_command.get(&command).expect("o comando adicionado vem do remoto");
        if has_conflicting_keybinding(keybindings, &command, &keybindings_merge_result.conflicts, &normalized_keys) {
            commands_merge_result.conflicts.insert(command);
            continue;
        }
        merge_content = add_keybindings(merge_content, keybindings, formatting_options);
    }

    // Updated commands in Remote
    for command in commands_merge_result.updated.values() {
        if commands_merge_result.conflicts.contains(&command) {
            continue;
        }
        let keybindings = remote_by_command.get(&command).expect("o comando atualizado vem do remoto");
        if has_conflicting_keybinding(keybindings, &command, &keybindings_merge_result.conflicts, &normalized_keys) {
            commands_merge_result.conflicts.insert(command);
            continue;
        }
        merge_content = update_keybindings(&merge_content, &command, keybindings, formatting_options);
    }

    Ok(MergeResult { merge_content, has_changes: true, has_conflicts: !commands_merge_result.conflicts.is_empty() })
}

/// Alguma entrada do comando (ignorando as negadas) usa um atalho em conflito.
fn has_conflicting_keybinding(
    keybindings: &[Keybinding],
    command: &JsString,
    conflicts: &StringSet,
    normalized_keys: &NormalizedKeys,
) -> bool {
    let negated = negated_command(command);
    keybindings.iter().any(|keybinding| keybinding.command != negated && conflicts.contains(&normalized(normalized_keys, &keybinding.key)))
}

fn compute_merge_result(local_to_remote: &CompareResult, base_to_local: &CompareResult, base_to_remote: &CompareResult) -> MergeSets {
    let mut result = MergeSets::default();

    // Removed keys in Local
    for key in base_to_local.removed.values() {
        // Got updated in remote
        if base_to_remote.updated.contains(&key) {
            result.conflicts.insert(key);
        }
    }

    // Removed keys in Remote
    for key in base_to_remote.removed.values() {
        if result.conflicts.contains(&key) {
            continue;
        }
        // Got updated in local
        if base_to_local.updated.contains(&key) {
            result.conflicts.insert(key);
        } else {
            // remove the key
            result.removed.insert(key);
        }
    }

    // Added keys in Local
    for key in base_to_local.added.values() {
        if result.conflicts.contains(&key) {
            continue;
        }
        // Got added in remote, with a different value
        if base_to_remote.added.contains(&key) && local_to_remote.updated.contains(&key) {
            result.conflicts.insert(key);
        }
    }

    // Added keys in remote
    for key in base_to_remote.added.values() {
        if result.conflicts.contains(&key) {
            continue;
        }
        // Got added in local
        if base_to_local.added.contains(&key) {
            // Has different value
            if local_to_remote.updated.contains(&key) {
                result.conflicts.insert(key);
            }
        } else {
            result.added.insert(key);
        }
    }

    // Updated keys in Local
    for key in base_to_local.updated.values() {
        if result.conflicts.contains(&key) {
            continue;
        }
        // Got updated in remote, with a different value
        if base_to_remote.updated.contains(&key) && local_to_remote.updated.contains(&key) {
            result.conflicts.insert(key);
        }
    }

    // Updated keys in Remote
    for key in base_to_remote.updated.values() {
        if result.conflicts.contains(&key) {
            continue;
        }
        // Got updated in local
        if base_to_local.updated.contains(&key) {
            // Has different value
            if local_to_remote.updated.contains(&key) {
                result.conflicts.insert(key);
            }
        } else {
            // updated key
            result.updated.insert(key);
        }
    }
    result
}

fn compute_merge_result_by_keybinding(
    local: &[Keybinding],
    remote: &[Keybinding],
    base: Option<&[Keybinding]>,
    normalized_keys: &NormalizedKeys,
) -> KeybindingsMergeResult {
    let forwarded = |has_local_forwarded, has_remote_forwarded| KeybindingsMergeResult {
        has_local_forwarded,
        has_remote_forwarded,
        conflicts: StringSet::default(),
    };
    let local_by_keybinding = by_keybinding(local, normalized_keys);
    let remote_by_keybinding = by_keybinding(remote, normalized_keys);
    let base_by_keybinding = base.map(|base| by_keybinding(base, normalized_keys));

    let local_to_remote_by_keybinding = compare_by_keybinding(&local_by_keybinding, &remote_by_keybinding);
    if local_to_remote_by_keybinding.is_empty() {
        return forwarded(false, false);
    }

    let base_to_local_by_keybinding = match &base_by_keybinding {
        Some(base) => compare_by_keybinding(base, &local_by_keybinding),
        None => CompareResult::all_added(local_by_keybinding.keys()),
    };
    if base_to_local_by_keybinding.is_empty() {
        // Remote has moved forward and local has not.
        return forwarded(false, true);
    }

    let base_to_remote_by_keybinding = match &base_by_keybinding {
        Some(base) => compare_by_keybinding(base, &remote_by_keybinding),
        None => CompareResult::all_added(remote_by_keybinding.keys()),
    };
    if base_to_remote_by_keybinding.is_empty() {
        return forwarded(true, false);
    }

    let MergeSets { conflicts, .. } =
        compute_merge_result(&local_to_remote_by_keybinding, &base_to_local_by_keybinding, &base_to_remote_by_keybinding);
    KeybindingsMergeResult { has_local_forwarded: true, has_remote_forwarded: true, conflicts }
}

fn by_keybinding(keybindings: &[Keybinding], keys: &NormalizedKeys) -> Grouped {
    let mut map = Grouped::default();
    for keybinding in keybindings {
        map.push(normalized(keys, &keybinding.key), keybinding.clone());
    }
    map
}

fn by_command(keybindings: &[Keybinding]) -> Grouped {
    let mut map = Grouped::default();
    for keybinding in keybindings {
        map.push(keybinding.target_command(), keybinding.clone());
    }
    map
}

/// `compareByKeybinding` e `compareByCommand`: as chaves que entraram, saíram ou mudaram de `from` para
/// `to`. `is_updated` decide, para uma chave presente nos dois lados, se o valor mudou.
fn compare(from: &Grouped, to: &Grouped, is_updated: impl Fn(&JsString, &[Keybinding], &[Keybinding]) -> bool) -> CompareResult {
    let from_keys = from.keys();
    let to_keys = to.keys();
    let added: StringSet = to_keys.iter().filter(|key| !from_keys.contains(key)).cloned().collect();
    let removed: StringSet = from_keys.iter().filter(|key| !to_keys.contains(key)).cloned().collect();
    let mut updated = StringSet::default();

    for key in &from_keys {
        if removed.contains(key) {
            continue;
        }
        let from_value = from.get(key).expect("a chave vem de `from`");
        let to_value = to.get(key).expect("a chave não foi removida");
        if is_updated(key, from_value, to_value) {
            updated.insert(key.clone());
        }
    }

    CompareResult { added, removed, updated }
}

fn compare_by_keybinding(from: &Grouped, to: &Grouped) -> CompareResult {
    compare(from, to, |key, value1, value2| {
        let retag = |values: &[Keybinding]| values.iter().map(|keybinding| keybinding.with_key(key)).collect::<Vec<_>>();
        !same_keybindings(&retag(value1), &retag(value2))
    })
}

fn compare_by_command(from: &Grouped, to: &Grouped, normalized_keys: &NormalizedKeys) -> CompareResult {
    compare(from, to, |_, value1, value2| {
        let retag = |values: &[Keybinding]| {
            values.iter().map(|keybinding| keybinding.with_key(&normalized(normalized_keys, &keybinding.key))).collect::<Vec<_>>()
        };
        !are_same_keybindings_with_same_command(&retag(value1), &retag(value2))
    })
}

fn same_keybindings(value1: &[Keybinding], value2: &[Keybinding]) -> bool {
    arrays::equals(Some(value1), Some(value2), is_same_keybinding)
}

fn are_same_keybindings_with_same_command(value1: &[Keybinding], value2: &[Keybinding]) -> bool {
    let split = |values: &[Keybinding]| -> (Vec<Keybinding>, Vec<Keybinding>) {
        values.iter().cloned().partition(|keybinding| !keybinding.is_negated())
    };
    let ((adding1, removing1), (adding2, removing2)) = (split(value1), split(value2));
    // Compare entries adding keybindings
    if !same_keybindings(&adding1, &adding2) {
        return false;
    }
    // Compare entries removing keybindings
    same_keybindings(&removing1, &removing2)
}

fn is_same_keybinding(a: &Keybinding, b: &Keybinding) -> bool {
    if a.command != b.command {
        return false;
    }
    if a.key != b.key {
        return false;
    }
    let when_a = ContextKeyExpr::deserialize(a.when.as_deref());
    let when_b = ContextKeyExpr::deserialize(b.when.as_deref());
    match (&when_a, &when_b) {
        (Some(_), None) | (None, Some(_)) => return false,
        (Some(when_a), Some(when_b)) if !when_a.equals(when_b) => return false,
        _ => {}
    }
    objects::equals(&a.args, &b.args)
}

fn add_keybindings(mut content: String, keybindings: &[Keybinding], formatting_options: &FormattingOptions) -> String {
    for keybinding in keybindings {
        content = content::edit(&content, &[EditSegment::Index(-1)], Some(keybinding.raw.clone()), formatting_options);
    }
    content
}

fn remove_keybindings(content: &str, command: &JsString, formatting_options: &FormattingOptions) -> String {
    let keybindings = parse_keybindings(content);
    let mut content = content.to_string();
    for (index, keybinding) in keybindings.iter().enumerate().rev() {
        if keybinding.is_for_command(command) {
            content = content::edit(&content, &[EditSegment::Index(index as i64)], None, formatting_options);
        }
    }
    content
}

fn update_keybindings(content: &str, command: &JsString, keybindings: &[Keybinding], formatting_options: &FormattingOptions) -> String {
    let all_keybindings = parse_keybindings(content);
    let location = all_keybindings.iter().position(|keybinding| keybinding.is_for_command(command)).map_or(-1, |i| i as i64);
    let mut content = content.to_string();
    // Remove all entries with this command
    for (index, keybinding) in all_keybindings.iter().enumerate().rev() {
        if keybinding.is_for_command(command) {
            content = content::edit(&content, &[EditSegment::Index(index as i64)], None, formatting_options);
        }
    }
    // add all entries at the same location where the entry with this command was located.
    for keybinding in keybindings.iter().rev() {
        content = content::edit(&content, &[EditSegment::Index(location)], Some(keybinding.raw.clone()), formatting_options);
    }
    content
}
