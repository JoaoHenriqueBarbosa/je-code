//! Porte de `src/vs/platform/userDataSync/common/settingsMerge.ts`: o merge de três vias do `settings.json`
//! (local, remoto e base) da sincronização de dados do usuário. Comentários, ordem das chaves e formatação
//! saem como no original.
//!
//! Representação:
//! - O conteúdo é `&str`/`String` (UTF-8); os deslocamentos dos nós (`startOffset`, `endOffset`, `commaOffset`)
//!   e as edições são em unidades UTF-16, como no original, então o texto é convertido para UTF-16 onde
//!   `JSON` e `jsonEdit` trabalham.
//! - O que `parse` devolve é [`Value`] (`None` onde o original devolve `undefined`). Ler `obj[key]` é
//!   [`property`]: objetos e arrays (por índice) respondem, o resto vale como `undefined`. `Object.keys` de
//!   `undefined` ou `null` lança `TypeError`, como no original; de um texto, número ou booleano só devolve
//!   lista vazia (o original devolveria os índices das letras de um texto).
//! - `Set<string>` e `Map<string, ...>` do JavaScript iteram em ordem de inserção, e essa ordem decide a
//!   ordem dos conflitos e das edições; por isso são [`OrderedMap`]. O `Set` de chaves ignoradas é a própria
//!   lista recebida (só serve para `has`).
//! - Os nós de [`parse_settings`] são comparados por posição na lista, não por identidade (`indexOf`).
//!   `startOffset` e `key` do original são `undefined` até a primeira propriedade do nível 0; aqui o
//!   deslocamento vale 0 e a chave é `None`, e uma chave `None` se lê como `"undefined"`, como em `obj[undefined]`.
//! - As exceções do `jsonEdit` (`Malformed AST`, `Overlapping edit`) viram [`errors::throw`].

use std::ops::ControlFlow;

use je_base::JsString;
use je_base::arrays;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::json::{self, ParseOptions, Value, Visitor};
use je_base::json_edit::{EditSegment, JsonEditError, apply_edits, set_property, with_formatting};
use je_base::json_formatter::{Edit, FormattingOptions, get_eol, json_stringify};
use je_base::map::OrderedMap;
use je_base::objects;

use super::content;
use super::user_data_sync::{IConflictSetting, get_disallowed_ignored_settings};
use crate::configuration::service::IConfigurationService;
use crate::configuration::ConfigurationOverrides;
use crate::js::{Key, from_json};

type StringSet = OrderedMap<JsString, ()>;

/// `IMergeResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct IMergeResult {
    pub local_content: Option<String>,
    pub remote_content: Option<String>,
    pub has_conflicts: bool,
    pub conflicts_settings: Vec<IConflictSetting>,
}

/// Um elemento de `resolvedConflicts`: `{ key, value }` (`value` `None` é `undefined`: remover a chave).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedConflict {
    pub key: JsString,
    pub value: Option<Value>,
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn from_utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// O erro do `jsonEdit` sobe como exceção.
fn thrown<T>(result: Result<T, JsonEditError>) -> T {
    result.unwrap_or_else(|e| errors::throw(Error::new(e.to_string())))
}

/// `content.substring(start, end)` (troca os limites se vierem invertidos e corta no tamanho do texto).
fn substring(content: &[u16], start: usize, end: usize) -> JsString {
    let (start, end) = if start > end { (end, start) } else { (start, end) };
    JsString::from_units(content[start.min(content.len())..end.min(content.len())].to_vec())
}

fn is_dash(unit: Option<&u16>) -> bool {
    unit == Some(&u16::from(b'-'))
}

/// A veracidade (`!!value`) de um valor JSON.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `parse(content)`.
fn parse(content: &str) -> Option<Value> {
    let mut errors = Vec::new();
    json::parse_str(content, &mut errors)
}

/// `parse(content) || {}`.
fn parse_or_empty(content: &str) -> Value {
    match parse(content) {
        Some(value) if is_truthy(&value) => value,
        _ => Value::Object(json::Object::new()),
    }
}

/// O índice de array canônico que `key` representa, se representa.
fn array_index(key: &JsString) -> Option<usize> {
    let text = key.to_string_lossy();
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `value[key]`.
fn property<'a>(value: Option<&'a Value>, key: &JsString) -> Option<&'a Value> {
    match value? {
        Value::Object(object) => object.get_js(key),
        Value::Array(items) => array_index(key).and_then(|index| items.get(index)),
        _ => None,
    }
}

/// `Object.keys(value)`; lança `TypeError` para `undefined` e `null`.
fn object_keys(value: Option<&Value>) -> Vec<JsString> {
    match value {
        None | Some(Value::Null) => errors::throw(Error::type_error("Cannot convert undefined or null to object")),
        Some(Value::Object(object)) => object.iter().map(|(key, _)| key.clone()).collect(),
        Some(Value::Array(items)) => (0..items.len()).map(|index| JsString::from(index.to_string())).collect(),
        Some(_) => Vec::new(),
    }
}

/// `objects.equals(a, b)` sobre valores que podem ser `undefined`.
fn values_equal(a: Option<&Value>, b: Option<&Value>) -> bool {
    let to_dynamic = |value: Option<&Value>| value.map_or(Dynamic::Undefined, from_json);
    objects::equals(&to_dynamic(a), &to_dynamic(b))
}

fn edit_key(content: &str, key: &JsString, value: Option<Value>, formatting_options: &FormattingOptions) -> String {
    content::edit(content, &[EditSegment::Property(key.clone())], value, formatting_options)
}

pub fn get_ignored_settings(
    default_ignored_settings: &[JsString],
    configuration_service: &dyn IConfigurationService,
    settings_content: Option<&str>,
) -> Vec<JsString> {
    let value = match settings_content.filter(|content| !content.is_empty()) {
        Some(content) => get_ignored_settings_from_content(content),
        None => get_ignored_settings_from_config(configuration_service),
    };
    let mut added: Vec<JsString> = Vec::new();
    let mut removed: Vec<JsString> = get_disallowed_ignored_settings();
    if let Dynamic::Array(keys) = &value {
        for key in keys.to_vec() {
            let Dynamic::String(key) = key else {
                errors::throw(Error::type_error("key.startsWith is not a function"));
            };
            if is_dash(key.units().first()) {
                removed.push(key.slice(1, key.len()));
            } else {
                added.push(key);
            }
        }
    }
    let settings: Vec<JsString> =
        default_ignored_settings.iter().chain(added.iter()).filter(|setting| !removed.contains(setting)).cloned().collect();
    arrays::distinct(&settings, Clone::clone)
}

fn get_ignored_settings_from_config(configuration_service: &dyn IConfigurationService) -> Dynamic {
    let user_value = |key: &str| configuration_service.inspect(key, &ConfigurationOverrides::default()).user_value();
    let value = user_value("settingsSync.ignoredSettings");
    if !value.is_undefined() {
        return value;
    }
    let value = user_value("sync.ignoredSettings");
    if !value.is_undefined() {
        return value;
    }
    let value = configuration_service.get_value(Some("settingsSync.ignoredSettings"), None);
    if value.is_truthy() { value } else { Dynamic::array([]) }
}

fn get_ignored_settings_from_content(settings_content: &str) -> Dynamic {
    if let Some(parsed) = parse(settings_content).filter(is_truthy) {
        for name in ["settingsSync.ignoredSettings", "sync.ignoredSettings"] {
            if let Some(value) = property(Some(&parsed), &JsString::from(name)).filter(|value| is_truthy(value)) {
                return from_json(value);
            }
        }
    }
    Dynamic::array([])
}

pub fn remove_comments(content: &str, formatting_options: &FormattingOptions) -> String {
    let source = parse_or_empty(content);
    let mut result = "{}".to_string();
    for key in object_keys(Some(&source)) {
        let text = utf16(&result);
        let edits = thrown(set_property(
            &text,
            &[EditSegment::Property(key.clone())],
            property(Some(&source), &key).cloned(),
            formatting_options,
            None,
        ));
        result = from_utf16(&thrown(apply_edits(&text, &edits)));
    }
    result
}

pub fn update_ignored_settings(
    target_content: &str,
    source_content: &str,
    ignored_settings: &[JsString],
    formatting_options: &FormattingOptions,
) -> String {
    let mut target_content = target_content.to_string();
    if !ignored_settings.is_empty() {
        let source_tree = parse_settings(source_content);
        let source = parse_or_empty(source_content);
        let Some(target) = parse(&target_content).filter(is_truthy) else {
            return target_content;
        };
        let mut settings_to_add: Vec<&Node> = Vec::new();
        for key in ignored_settings {
            let source_value = property(Some(&source), key);
            let target_value = property(Some(&target), key);

            // Remove in target
            if source_value.is_none() {
                target_content = edit_key(&target_content, key, None, formatting_options);
            }
            // Update in target
            else if target_value.is_some() {
                target_content = edit_key(&target_content, key, source_value.cloned(), formatting_options);
            } else {
                match find_setting_node(Some(key), &source_tree) {
                    Some(index) => settings_to_add.push(&source_tree[index]),
                    None => errors::throw(Error::type_error("Cannot read properties of undefined (reading 'setting')")),
                }
            }
        }

        settings_to_add.sort_by_key(|node| node.start_offset);
        for node in settings_to_add {
            let key = match node.setting.as_ref().and_then(|setting| setting.key.as_ref()) {
                Some(key) => key.clone(),
                None => JsString::from("undefined"),
            };
            target_content = add_setting(&key, source_content, &target_content, formatting_options);
        }
    }
    target_content
}

pub fn merge(
    original_local_content: &str,
    original_remote_content: &str,
    base_content: Option<&str>,
    ignored_settings: &[JsString],
    resolved_conflicts: &[ResolvedConflict],
    formatting_options: &FormattingOptions,
) -> IMergeResult {
    let local_content_without_ignored_settings =
        update_ignored_settings(original_local_content, original_remote_content, ignored_settings, formatting_options);
    let local_forwarded = base_content != Some(local_content_without_ignored_settings.as_str());
    let remote_forwarded = base_content != Some(original_remote_content);

    let result = |local_content, remote_content| IMergeResult {
        conflicts_settings: Vec::new(),
        local_content,
        remote_content,
        has_conflicts: false,
    };

    /* no changes */
    if !local_forwarded && !remote_forwarded {
        return result(None, None);
    }

    /* local has changed and remote has not */
    if local_forwarded && !remote_forwarded {
        return result(None, Some(local_content_without_ignored_settings));
    }

    /* remote has changed and local has not */
    if remote_forwarded && !local_forwarded {
        return result(
            Some(update_ignored_settings(original_remote_content, original_local_content, ignored_settings, formatting_options)),
            None,
        );
    }

    /* local is empty and not synced before */
    if base_content.is_none() && is_empty(original_local_content) {
        let local_content = if are_same(original_local_content, original_remote_content, ignored_settings) {
            None
        } else {
            Some(update_ignored_settings(original_remote_content, original_local_content, ignored_settings, formatting_options))
        };
        return result(local_content, None);
    }

    /* remote and local has changed */
    let local = parse(original_local_content);
    let remote = parse(original_remote_content);
    let base = base_content.filter(|content| !content.is_empty()).map(parse);

    let local_to_remote = compare(local.as_ref(), remote.as_ref(), ignored_settings);
    let base_to_local = compare(base.as_ref().and_then(Option::as_ref), local.as_ref(), ignored_settings);
    let base_to_remote = compare(base.as_ref().and_then(Option::as_ref), remote.as_ref(), ignored_settings);

    let mut state = MergeState {
        local: local.as_ref(),
        remote: remote.as_ref(),
        resolved_conflicts,
        formatting_options,
        local_content: original_local_content.to_string(),
        remote_content: original_remote_content.to_string(),
        handled_conflicts: StringSet::new(),
        conflicts: OrderedMap::new(),
    };

    // Removed settings in Local
    for key in base_to_local.removed.keys() {
        // Conflict - Got updated in remote.
        if base_to_remote.updated.contains_key(key) {
            state.handle_conflict(key);
        }
        // Also remove in remote
        else {
            state.remote_content = edit_key(&state.remote_content, key, None, formatting_options);
        }
    }

    // Removed settings in Remote
    for key in base_to_remote.removed.keys() {
        if state.handled_conflicts.contains_key(key) {
            continue;
        }
        // Conflict - Got updated in local
        if base_to_local.updated.contains_key(key) {
            state.handle_conflict(key);
        }
        // Also remove in locals
        else {
            state.local_content = edit_key(&state.local_content, key, None, formatting_options);
        }
    }

    // Updated settings in Local
    for key in base_to_local.updated.keys() {
        if state.handled_conflicts.contains_key(key) {
            continue;
        }
        // Got updated in remote
        if base_to_remote.updated.contains_key(key) {
            // Has different value
            if local_to_remote.updated.contains_key(key) {
                state.handle_conflict(key);
            }
        } else {
            let value = property(state.local, key).cloned();
            state.remote_content = edit_key(&state.remote_content, key, value, formatting_options);
        }
    }

    // Updated settings in Remote
    for key in base_to_remote.updated.keys() {
        if state.handled_conflicts.contains_key(key) {
            continue;
        }
        // Got updated in local
        if base_to_local.updated.contains_key(key) {
            // Has different value
            if local_to_remote.updated.contains_key(key) {
                state.handle_conflict(key);
            }
        } else {
            let value = property(state.remote, key).cloned();
            state.local_content = edit_key(&state.local_content, key, value, formatting_options);
        }
    }

    // Added settings in Local
    for key in base_to_local.added.keys() {
        if state.handled_conflicts.contains_key(key) {
            continue;
        }
        // Got added in remote
        if base_to_remote.added.contains_key(key) {
            // Has different value
            if local_to_remote.updated.contains_key(key) {
                state.handle_conflict(key);
            }
        } else {
            state.remote_content = add_setting(key, &state.local_content, &state.remote_content, formatting_options);
        }
    }

    // Added settings in remote
    for key in base_to_remote.added.keys() {
        if state.handled_conflicts.contains_key(key) {
            continue;
        }
        // Got added in local
        if base_to_local.added.contains_key(key) {
            // Has different value
            if local_to_remote.updated.contains_key(key) {
                state.handle_conflict(key);
            }
        } else {
            state.local_content = add_setting(key, &state.remote_content, &state.local_content, formatting_options);
        }
    }

    let has_conflicts =
        !state.conflicts.is_empty() || !are_same(&state.local_content, &state.remote_content, ignored_settings);
    let has_local_changed = has_conflicts || !are_same(&state.local_content, original_local_content, &[]);
    let has_remote_changed = has_conflicts || !are_same(&state.remote_content, original_remote_content, &[]);
    IMergeResult {
        local_content: has_local_changed.then_some(state.local_content),
        remote_content: has_remote_changed.then_some(state.remote_content),
        conflicts_settings: state.conflicts.values().cloned().collect(),
        has_conflicts,
    }
}

/// O estado que o `handleConflict` do original captura: os dois conteúdos em construção, os conflitos já
/// tratados e os encontrados.
struct MergeState<'a> {
    local: Option<&'a Value>,
    remote: Option<&'a Value>,
    resolved_conflicts: &'a [ResolvedConflict],
    formatting_options: &'a FormattingOptions,
    local_content: String,
    remote_content: String,
    handled_conflicts: StringSet,
    conflicts: OrderedMap<JsString, IConflictSetting>,
}

impl MergeState<'_> {
    fn handle_conflict(&mut self, conflict_key: &JsString) {
        self.handled_conflicts.insert(conflict_key.clone(), ());
        match self.resolved_conflicts.iter().find(|resolved| resolved.key == *conflict_key) {
            Some(resolved) => {
                self.local_content = edit_key(&self.local_content, conflict_key, resolved.value.clone(), self.formatting_options);
                self.remote_content = edit_key(&self.remote_content, conflict_key, resolved.value.clone(), self.formatting_options);
            }
            None => {
                self.conflicts.insert(
                    conflict_key.clone(),
                    IConflictSetting {
                        key: conflict_key.clone(),
                        local_value: property(self.local, conflict_key).cloned(),
                        remote_value: property(self.remote, conflict_key).cloned(),
                    },
                );
            }
        }
    }
}

fn are_same(local_content: &str, remote_content: &str, ignored_settings: &[JsString]) -> bool {
    if local_content == remote_content {
        return true;
    }

    let local = parse(local_content);
    let remote = parse(remote_content);
    let not_ignored = |node: &Node| !node.setting.as_ref().is_some_and(|setting| setting.key.as_ref().is_some_and(|key| ignored_settings.contains(key)));
    let local_tree: Vec<Node> = parse_settings(local_content).into_iter().filter(not_ignored).collect();
    let remote_tree: Vec<Node> = parse_settings(remote_content).into_iter().filter(not_ignored).collect();

    if local_tree.len() != remote_tree.len() {
        return false;
    }

    for (local_node, remote_node) in local_tree.iter().zip(&remote_tree) {
        match (&local_node.setting, &remote_node.setting) {
            (Some(local_setting), Some(remote_setting)) => {
                if local_setting.key != remote_setting.key {
                    return false;
                }
                let key = key_text(local_setting.key.as_ref());
                if !values_equal(property(local.as_ref(), &key), property(remote.as_ref(), &key)) {
                    return false;
                }
            }
            (None, None) => {
                if local_node.value != remote_node.value {
                    return false;
                }
            }
            _ => return false,
        }
    }

    true
}

/// A chave como `obj[key]` a lê: uma chave ausente (`undefined`) vira o texto `"undefined"`.
fn key_text(key: Option<&JsString>) -> JsString {
    key.cloned().unwrap_or_else(|| JsString::from("undefined"))
}

pub fn is_empty(content: &str) -> bool {
    if content.is_empty() {
        return true;
    }
    parse_settings(content).is_empty()
}

/// O resultado de `compare`.
struct CompareResult {
    added: StringSet,
    removed: StringSet,
    updated: StringSet,
}

fn compare(from: Option<&Value>, to: Option<&Value>, ignored: &[JsString]) -> CompareResult {
    let keys_of = |value: Vec<JsString>| -> Vec<JsString> { value.into_iter().filter(|key| !ignored.contains(key)).collect() };
    let from = from.filter(|value| is_truthy(value));
    let from_keys = from.map_or_else(Vec::new, |from| keys_of(object_keys(Some(from))));
    let to_keys = keys_of(object_keys(to));
    let added: StringSet = to_keys.iter().filter(|key| !from_keys.contains(key)).map(|key| (key.clone(), ())).collect_set();
    let removed: StringSet = from_keys.iter().filter(|key| !to_keys.contains(key)).map(|key| (key.clone(), ())).collect_set();
    let mut updated = StringSet::new();

    if from.is_some() {
        for key in &from_keys {
            if removed.contains_key(key) {
                continue;
            }
            if !values_equal(property(from, key), property(to, key)) {
                updated.insert(key.clone(), ());
            }
        }
    }

    CompareResult { added, removed, updated }
}

/// Recolhe pares `(chave, ())` num [`StringSet`] na ordem de inserção.
trait CollectSet {
    fn collect_set(self) -> StringSet;
}

impl<I: Iterator<Item = (JsString, ())>> CollectSet for I {
    fn collect_set(self) -> StringSet {
        let mut set = StringSet::new();
        for (key, ()) in self {
            set.insert(key, ());
        }
        set
    }
}

pub fn add_setting(key: impl Key, source_content: &str, target_content: &str, formatting_options: &FormattingOptions) -> String {
    let key = key.to_js();
    let source = parse(source_content);
    let source_tree = parse_settings(source_content);
    let target_tree = parse_settings(target_content);
    let insert_location = get_insert_location(&key, &source_tree, &target_tree);
    insert_at_location(
        target_content,
        &key,
        property(source.as_ref(), &key),
        &insert_location,
        &target_tree,
        formatting_options,
    )
}

/// `InsertLocation`. `index` é `-1` quando a árvore de destino está vazia.
struct InsertLocation {
    index: i64,
    insert_after: bool,
}

impl InsertLocation {
    fn after(index: usize) -> Self {
        Self { index: index as i64, insert_after: true }
    }

    fn before(index: usize) -> Self {
        Self { index: index as i64, insert_after: false }
    }
}

fn get_insert_location(key: &JsString, source_tree: &[Node], target_tree: &[Node]) -> InsertLocation {
    let source_node_index = source_tree.iter().position(|node| node_key(node) == Some(key));

    if let Some(source_node_index) = source_node_index
        && source_node_index > 0
    {
        let source_previous_node = &source_tree[source_node_index - 1];
        if let Some(setting) = &source_previous_node.setting {
            /*
                Previous node in source is a setting.
                Find the same setting in the target.
                Insert it after that setting
            */
            if let Some(target_previous_setting) = find_setting_node(setting.key.as_ref(), target_tree) {
                /* Insert after target's previous setting */
                return InsertLocation::after(target_previous_setting);
            }
        }
        /* Previous node in source is a comment */
        else if let Some(source_previous_setting_node) = find_previous_setting_node(source_node_index, source_tree) {
            /*
                Source has a setting defined before the setting to be added.
                Find the same previous setting in the target.
                If found, insert before its next setting so that comments are retrieved.
                Otherwise, insert at the end.
            */
            let previous_key = source_tree[source_previous_setting_node].setting.as_ref().and_then(|setting| setting.key.as_ref());
            if let Some(target_previous_setting) = find_setting_node(previous_key, target_tree) {
                let target_next_setting = find_next_setting_node(target_previous_setting, target_tree);
                let source_comment_nodes = find_nodes_between(source_tree, source_previous_setting_node, source_node_index);
                if let Some(target_next_setting) = target_next_setting {
                    let target_comment_nodes = find_nodes_between(target_tree, target_previous_setting, target_next_setting);
                    return match find_last_matching_target_comment_node(
                        source_tree,
                        &source_comment_nodes,
                        target_tree,
                        &target_comment_nodes,
                    ) {
                        /* Insert after comment */
                        Some(target_comment_node) => InsertLocation::after(target_comment_node),
                        /* Insert before target next setting */
                        None => InsertLocation::before(target_next_setting),
                    };
                }
                let last = target_tree.len() - 1;
                let target_comment_nodes = find_nodes_between(target_tree, target_previous_setting, last);
                return match find_last_matching_target_comment_node(
                    source_tree,
                    &source_comment_nodes,
                    target_tree,
                    &target_comment_nodes,
                ) {
                    /* Insert after comment */
                    Some(target_comment_node) => InsertLocation::after(target_comment_node),
                    /* Insert at the end */
                    None => InsertLocation::after(last),
                };
            }
        }

        if let Some(source_next_node) = source_tree.get(source_node_index + 1) {
            if let Some(setting) = &source_next_node.setting {
                /*
                    Next node in source is a setting.
                    Find the same setting in the target.
                    Insert it before that setting
                */
                if let Some(target_next_setting) = find_setting_node(setting.key.as_ref(), target_tree) {
                    /* Insert before target's next setting */
                    return InsertLocation::before(target_next_setting);
                }
            }
            /* Next node in source is a comment */
            else if let Some(source_next_setting_node) = find_next_setting_node(source_node_index, source_tree) {
                /*
                    Source has a setting defined after the setting to be added.
                    Find the same next setting in the target.
                    If found, insert after its previous setting so that comments are retrieved.
                    Otherwise, insert at the beginning.
                */
                let next_key = source_tree[source_next_setting_node].setting.as_ref().and_then(|setting| setting.key.as_ref());
                if let Some(target_next_setting) = find_setting_node(next_key, target_tree) {
                    let target_previous_setting = find_previous_setting_node(target_next_setting, target_tree);
                    let mut source_comment_nodes = find_nodes_between(source_tree, source_node_index, source_next_setting_node);
                    source_comment_nodes.reverse();
                    let (before_target_comment, fallback) = match target_previous_setting {
                        Some(target_previous_setting) => {
                            /* Insert after target previous setting */
                            (find_nodes_between(target_tree, target_previous_setting, target_next_setting), InsertLocation::after(target_previous_setting))
                        }
                        /* Insert at the beginning */
                        None => (find_nodes_between(target_tree, 0, target_next_setting), InsertLocation::before(0)),
                    };
                    let mut target_comment_nodes = before_target_comment;
                    target_comment_nodes.reverse();
                    return match find_last_matching_target_comment_node(
                        source_tree,
                        &source_comment_nodes,
                        target_tree,
                        &target_comment_nodes,
                    ) {
                        /* Insert before comment */
                        Some(target_comment_node) => InsertLocation::before(target_comment_node),
                        None => fallback,
                    };
                }
            }
        }
    }
    /* Insert at the end */
    InsertLocation { index: target_tree.len() as i64 - 1, insert_after: true }
}

fn insert_at_location(
    content: &str,
    key: &JsString,
    value: Option<&Value>,
    location: &InsertLocation,
    tree: &[Node],
    formatting_options: &FormattingOptions,
) -> String {
    let text = utf16(content);
    let edits: Vec<Edit> = if location.index == -1 {
        /* Insert at the end */
        thrown(set_property(&text, &[EditSegment::Property(key.clone())], value.cloned(), formatting_options, None))
    } else {
        get_edit_to_insert_at_location(content, key, value, location, tree, formatting_options)
            .into_iter()
            .map(|edit| thrown(with_formatting(&text, edit, formatting_options)).remove(0))
            .collect()
    };
    from_utf16(&thrown(apply_edits(&text, &edits)))
}

fn get_edit_to_insert_at_location(
    content: &str,
    key: &JsString,
    value: Option<&Value>,
    location: &InsertLocation,
    tree: &[Node],
    formatting_options: &FormattingOptions,
) -> Vec<Edit> {
    // `JSON.stringify(undefined)` é `undefined`, que o literal de texto escreve como "undefined".
    let value_text = value.map_or_else(|| "undefined".to_string(), |value| json_stringify(value, None));
    let new_property = format!("{}: {}", json_stringify(&Value::String(key.clone()), None), value_text);
    let eol = get_eol(formatting_options, &utf16(content)).to_string_lossy();
    let index = location.index as usize;
    let node = &tree[index];
    let insert = |offset: usize, text: String| Edit { offset, length: 0, content: JsString::from(text) };

    if location.insert_after {
        let mut edits: Vec<Edit> = Vec::new();

        /* Insert after a setting */
        if node.setting.is_some() {
            edits.push(insert(node.end_offset, format!(",{new_property}")));
        }
        /* Insert after a comment */
        else {
            let next_setting_node = find_next_setting_node(index, tree);
            let previous_setting_node = find_previous_setting_node(index, tree);
            let previous_setting_comma_offset =
                previous_setting_node.and_then(|previous| tree[previous].setting.as_ref().and_then(|setting| setting.comma_offset));

            /* If there is a previous setting and it does not has comma then add it */
            if let Some(previous) = previous_setting_node
                && previous_setting_comma_offset.is_none()
            {
                edits.push(insert(tree[previous].end_offset, ",".to_string()));
            }

            let is_previous_setting_includes_comment = previous_setting_comma_offset.is_some_and(|comma| comma > node.end_offset);
            let offset = match previous_setting_comma_offset {
                Some(comma) if is_previous_setting_includes_comment => comma + 1,
                _ => node.end_offset,
            };
            let text = if next_setting_node.is_some() { format!("{eol}{new_property},") } else { format!("{eol}{new_property}") };
            edits.push(insert(offset, text));
        }

        return edits;
    }

    /* Insert before a setting */
    if node.setting.is_some() {
        return vec![insert(node.start_offset, format!("{new_property},"))];
    }

    /* Insert before a comment */
    let previous_is_comment = index > 0 && tree[index - 1].setting.is_none();
    let text = format!(
        "{}{}{}{}",
        if previous_is_comment { eol.as_str() } else { "" },
        new_property,
        if find_next_setting_node(index, tree).is_some() { "," } else { "" },
        eol
    );
    vec![insert(node.start_offset, text)]
}

/// A chave do nó, se ele é um setting (`node.setting?.key`).
fn node_key(node: &Node) -> Option<&JsString> {
    node.setting.as_ref().and_then(|setting| setting.key.as_ref())
}

/// O índice do primeiro nó cuja chave é `key` (`node.setting?.key === key`).
fn find_setting_node(key: Option<&JsString>, tree: &[Node]) -> Option<usize> {
    tree.iter().position(|node| node_key(node) == key)
}

fn find_previous_setting_node(index: usize, tree: &[Node]) -> Option<usize> {
    (0..index.min(tree.len())).rev().find(|&i| tree[i].setting.is_some())
}

fn find_next_setting_node(index: usize, tree: &[Node]) -> Option<usize> {
    (index + 1..tree.len()).find(|&i| tree[i].setting.is_some())
}

/// Os índices dos nós entre `from` e `till`, sem eles.
fn find_nodes_between(nodes: &[Node], from: usize, till: usize) -> Vec<usize> {
    (0..nodes.len()).filter(|&index| from < index && index < till).collect()
}

fn find_last_matching_target_comment_node(
    source: &[Node],
    source_comments: &[usize],
    target: &[Node],
    target_comments: &[usize],
) -> Option<usize> {
    if source_comments.is_empty() || target_comments.is_empty() {
        return None;
    }
    let mut index = 0;
    while index < target_comments.len() && index < source_comments.len() {
        if source[source_comments[index]].value != target[target_comments[index]].value {
            return index.checked_sub(1).map(|previous| target_comments[previous]);
        }
        index += 1;
    }
    Some(target_comments[index - 1])
}

#[derive(Clone, Debug)]
struct Setting {
    key: Option<JsString>,
    comma_offset: Option<usize>,
}

/// `INode`.
#[derive(Clone, Debug)]
struct Node {
    start_offset: usize,
    end_offset: usize,
    value: JsString,
    setting: Option<Setting>,
}

struct SettingsVisitor<'a> {
    content: &'a [u16],
    nodes: Vec<Node>,
    hierarchy_level: i64,
    start_offset: usize,
    key: Option<JsString>,
}

impl SettingsVisitor<'_> {
    fn push_setting(&mut self, end_offset: usize) {
        self.nodes.push(Node {
            start_offset: self.start_offset,
            end_offset,
            value: substring(self.content, self.start_offset, end_offset),
            setting: Some(Setting { key: self.key.clone(), comma_offset: None }),
        });
    }
}

impl Visitor for SettingsVisitor<'_> {
    fn on_object_begin(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        self.hierarchy_level += 1;
        ControlFlow::Continue(())
    }

    fn on_object_property(&mut self, name: &JsString, offset: usize, _length: usize) -> ControlFlow<()> {
        if self.hierarchy_level == 0 {
            // this is setting key
            self.start_offset = offset;
            self.key = Some(name.clone());
        }
        ControlFlow::Continue(())
    }

    fn on_object_end(&mut self, offset: usize, length: usize) -> ControlFlow<()> {
        self.hierarchy_level -= 1;
        if self.hierarchy_level == 0 {
            self.push_setting(offset + length);
        }
        ControlFlow::Continue(())
    }

    fn on_array_begin(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        self.hierarchy_level += 1;
        ControlFlow::Continue(())
    }

    fn on_array_end(&mut self, offset: usize, length: usize) -> ControlFlow<()> {
        self.hierarchy_level -= 1;
        if self.hierarchy_level == 0 {
            self.push_setting(offset + length);
        }
        ControlFlow::Continue(())
    }

    fn on_literal_value(&mut self, _value: &Value, offset: usize, length: usize) -> ControlFlow<()> {
        if self.hierarchy_level == 0 {
            self.push_setting(offset + length);
        }
        ControlFlow::Continue(())
    }

    fn on_separator(&mut self, separator: char, offset: usize, _length: usize) -> ControlFlow<()> {
        if self.hierarchy_level == 0
            && separator == ','
            && let Some(index) = self.nodes.iter().rposition(|node| node.setting.is_some())
            && let Some(setting) = self.nodes[index].setting.as_mut()
        {
            setting.comma_offset = Some(offset);
        }
        ControlFlow::Continue(())
    }

    fn on_comment(&mut self, offset: usize, length: usize) -> ControlFlow<()> {
        if self.hierarchy_level == 0 {
            self.nodes.push(Node {
                start_offset: offset,
                end_offset: offset + length,
                value: substring(self.content, offset, offset + length),
                setting: None,
            });
        }
        ControlFlow::Continue(())
    }
}

fn parse_settings(content: &str) -> Vec<Node> {
    let text = utf16(content);
    let mut visitor = SettingsVisitor { content: &text, nodes: Vec::new(), hierarchy_level: -1, start_offset: 0, key: None };
    json::visit(&text, &mut visitor, ParseOptions::default());
    visitor.nodes
}
