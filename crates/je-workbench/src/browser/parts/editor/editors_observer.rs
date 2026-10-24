//! Porte de `src/vs/workbench/browser/parts/editor/editorsObserver.ts`: o [`EditorsObserver`], que observa os editores
//! abertos em todos os grupos de editor por uso mais recente, conta quantos editores abertos há por recurso e fecha os mais
//! antigos quando o limite `workbench.editor.limit` é ultrapassado.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`]; o `editorGroupsContainer`
//!   (`IEditorGroupsContainer | undefined`) é o argumento estático, um `Option<EditorGroupsContainer>`. A classe herda de
//!   `Disposable`, aqui um `Disposable` guardado no campo `base`.
//! - **Chaves por identidade.** O original usa o objeto `IEditorIdentifier` como chave do `LinkedMap` (o `keyMap` garante um
//!   objeto por par grupo e editor). Aqui a chave é o par `(groupId, identidade do editor)` e o valor é o
//!   [`EditorIdentifier`]; o `keyMap` guarda o identificador por par. A identidade do editor é a do ponteiro (`id_of`).
//! - **Lado a lado.** `editor instanceof SideBySideEditorInput` inclui o editor de diff, que herda dele
//!   ([`side_by_side_input_sides`]).
//! - **`async`.** `ensureOpenedEditorsLimit`, `doEnsureOpenedEditorsLimit` e `loadState` são [`async_fn`]: o corpo roda já,
//!   de forma síncrona, até o primeiro `await`. O limite (`number`) é `f64`, como no original.
//! - **JSON do armazenamento.** `JSON.stringify` e `JSON.parse` são `stringify_json` e `parse_json` do sincronizador de
//!   configurações. Uma lista sem `entries` lança `TypeError`, como o `for...of` do original.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{Object as JsonObject, Value as JsonValue};
use je_base::lifecycle::{Disposable, DisposableId, DisposableStore, IDisposable, id_of};
use je_base::map::{LinkedMap, ResourceMap, Touch};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::JsString;
use je_instantiation::Injectable;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::user_data_sync::abstract_synchronizer::{parse_json, stringify_json};
use je_storage::StorageValue;

use crate::common::editor::editor_factory_registry::editor_factory_registry;
use crate::common::editor::{
    EditorIdentifier, EditorInputCapabilities, EditorInputRef, EditorsOrder, GroupIdentifier, GroupModelChangeKind, side_by_side_input_sides,
};
use crate::services::editor::editor_groups_container::EditorGroupsContainer;
use crate::services::editor::editor_groups_service::{
    CloseEditorOptions, CloseEditorsTarget, EditorPartOptionsChangeEvent, GroupsOrder, IEditorGroup, IEditorGroupsService,
};
use crate::services::editor::editor_service::ResourceEditorInputIdentifier;

const STORAGE_KEY: &str = "editors.mru";

/// A chave de um editor aberto: o grupo e a identidade do editor.
type EditorKey = (GroupIdentifier, DisposableId);

fn key_of(identifier: &EditorIdentifier) -> EditorKey {
    (identifier.group_id, id_of(&identifier.editor))
}

/// `toIdentifier(typeId, editorId)`.
fn to_identifier(type_id: &str, editor_id: Option<&str>) -> String {
    match editor_id {
        Some(editor_id) if !editor_id.is_empty() => format!("{type_id}/{editor_id}"),
        _ => type_id.to_string(),
    }
}

/// `EditorsObserver`: a observer of opened editors across all editor groups by most recently used. Rules:
/// - the last editor in the list is the one most recently activated
/// - the first editor in the list is the one that was activated the longest time ago
/// - an editor that opens inactive will be placed behind the currently active editor
///
/// The observer may start to close editors based on the workbench.editor.limit setting.
pub struct EditorsObserver {
    this: Weak<EditorsObserver>,
    base: Rc<Disposable>,

    /// `keyMap`.
    key_map: RefCell<HashMap<GroupIdentifier, HashMap<DisposableId, EditorIdentifier>>>,
    /// `mostRecentEditorsMap`.
    most_recent_editors_map: LinkedMap<EditorKey, EditorIdentifier>,
    /// `editorsPerResourceCounter`: por recurso, o contador de editores abertos por `typeId/editorId`.
    editors_per_resource_counter: RefCell<ResourceMap<HashMap<String, usize>>>,

    on_did_most_recently_active_editors_change: Rc<Emitter<()>>,

    editor_groups_container: EditorGroupsContainer,
    is_scoped: bool,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    storage_service: Rc<dyn StorageService>,
}

impl Injectable for EditorsObserver {
    type Args = Option<EditorGroupsContainer>;
    type Deps = (Rc<dyn IEditorGroupsService>, Rc<dyn StorageService>);

    fn create(editor_groups_container: Self::Args, (editor_group_service, storage_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_most_recently_active_editors_change = Emitter::new();
        base.register(on_did_most_recently_active_editors_change.clone());

        let is_scoped = editor_groups_container.is_some();
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            key_map: RefCell::new(HashMap::new()),
            most_recent_editors_map: LinkedMap::new(),
            editors_per_resource_counter: RefCell::new(ResourceMap::new()),
            on_did_most_recently_active_editors_change,
            editor_groups_container: editor_groups_container.unwrap_or_else(|| EditorGroupsContainer::Service(editor_group_service.clone())),
            is_scoped,
            editor_group_service,
            storage_service,
        });

        this.register_listeners();
        // `this.loadState()`: a promessa não é aguardada.
        drop(this.load_state());

        this
    }
}

impl IDisposable for EditorsObserver {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorsObserver {
    fn rc(&self) -> Rc<EditorsObserver> {
        self.this.upgrade().expect("EditorsObserver vivo")
    }

    /// `onDidMostRecentlyActiveEditorsChange`.
    pub fn on_did_most_recently_active_editors_change(&self) -> Event<()> {
        self.on_did_most_recently_active_editors_change.event()
    }

    /// `count`.
    pub fn count(&self) -> usize {
        self.most_recent_editors_map.size()
    }

    /// `editors`.
    pub fn editors(&self) -> Vec<EditorIdentifier> {
        self.most_recent_editors_map.values().collect()
    }

    /// `hasEditor(editor)`.
    pub fn has_editor(&self, editor: &ResourceEditorInputIdentifier) -> bool {
        let counter = self.editors_per_resource_counter.borrow();
        counter.get(&editor.resource).is_some_and(|editors| editors.contains_key(&to_identifier(&editor.type_id, editor.editor_id.as_deref())))
    }

    /// `hasEditors(resource)`.
    pub fn has_editors(&self, resource: &URI) -> bool {
        self.editors_per_resource_counter.borrow().has(resource)
    }

    fn register_listeners(&self) {
        let weak = self.this.clone();
        self.base.register(self.editor_groups_container.on_did_add_group().listen(move |group| {
            if let Some(this) = weak.upgrade() {
                this.on_group_added(group);
            }
        }));

        let weak = self.this.clone();
        self.base.register(self.editor_group_service.on_did_change_editor_part_options().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_editor_part_options(event);
            }
        }));

        let weak = self.this.clone();
        self.base.register(self.storage_service.on_will_save_state().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.save_state();
            }
        }));
    }

    fn on_group_added(&self, group: &Rc<dyn IEditorGroup>) {
        // Make sure to add any already existing editor of the new group into our list in LRU order
        let group_editors_mru = group.get_editors(EditorsOrder::MostRecentlyActive, None);
        for editor in group_editors_mru.iter().rev() {
            self.add_most_recent_editor(group, editor, false /* is not active */, true /* is new */);
        }

        // Make sure that active editor is put as first if group is active
        if self.editor_groups_container.active_group().id() == group.id()
            && let Some(active_editor) = group.active_editor()
        {
            self.add_most_recent_editor(group, &active_editor, true /* is active */, false /* already added before */);
        }

        // Group Listeners
        self.register_group_listeners(group);
    }

    fn register_group_listeners(&self, group: &Rc<dyn IEditorGroup>) {
        let group_disposables = DisposableStore::new();

        let (weak, group_ref) = (self.this.clone(), group.clone());
        group_disposables.add(group.on_did_model_change().listen(move |event| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            match event.kind {
                // Group gets active: put active editor as most recent
                GroupModelChangeKind::GroupActive => {
                    if this.editor_groups_container.active_group().id() == group_ref.id()
                        && let Some(active_editor) = group_ref.active_editor()
                    {
                        this.add_most_recent_editor(&group_ref, &active_editor, true /* is active */, false /* editor already opened */);
                    }
                }

                // Editor opens: put it as second most recent
                //
                // Also check for maximum allowed number of editors and start to close oldest ones if needed.
                GroupModelChangeKind::EditorOpen => {
                    if let Some(editor) = &event.editor {
                        this.add_most_recent_editor(&group_ref, editor, false /* is not active */, true /* is new */);
                        drop(this.ensure_opened_editors_limit(Some(EditorIdentifier { group_id: group_ref.id(), editor: editor.clone() }), Some(group_ref.id())));
                    }
                }

                _ => {}
            }
        }));

        // Editor closes: remove from recently opened
        let (weak, group_ref) = (self.this.clone(), group.clone());
        group_disposables.add(group.on_did_close_editor().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.remove_most_recent_editor(&group_ref, &event.editor);
            }
        }));

        // Editor gets active: put active editor as most recent if group is active, otherwise second most recent
        let (weak, group_ref) = (self.this.clone(), group.clone());
        group_disposables.add(group.on_did_active_editor_change().listen(move |event| {
            if let (Some(this), Some(editor)) = (weak.upgrade(), &event.editor) {
                let is_active = this.editor_groups_container.active_group().id() == group_ref.id();
                this.add_most_recent_editor(&group_ref, editor, is_active, false /* editor already opened */);
            }
        }));

        // Make sure to cleanup on dispose
        let group_disposables_to_dispose = group_disposables.clone();
        group.on_will_dispose().once().listen(move |_| group_disposables_to_dispose.dispose());
    }

    fn on_did_change_editor_part_options(&self, event: &EditorPartOptionsChangeEvent) {
        if event.new_part_options.limit != event.old_part_options.limit {
            let active_group = self.editor_groups_container.active_group();
            let exclude = active_group.active_editor().map(|editor| EditorIdentifier { group_id: active_group.id(), editor });

            drop(self.rc().ensure_opened_editors_limit(exclude, None));
        }
    }

    fn add_most_recent_editor(&self, group: &Rc<dyn IEditorGroup>, editor: &EditorInputRef, is_active: bool, is_new: bool) {
        let key = self.ensure_key(group, editor);
        let most_recent_editor = self.most_recent_editors_map.first();

        match most_recent_editor {
            // Otherwise: insert before most recent
            Some(most_recent_editor) if !is_active => {
                // we have most recent editors. as such we put this newly opened editor right before the current most recent
                // one because it cannot be the most recently active one unless it becomes active. but it is still more
                // active then any other editor in the list.
                self.most_recent_editors_map.set(key_of(&key), key, Touch::AsOld /* make first */);
                self.most_recent_editors_map.set(key_of(&most_recent_editor), most_recent_editor, Touch::AsOld /* make first */);
            }

            // Active or first entry: add to end of map
            most_recent_editor => {
                // `Touch.AsOld /* make first */` when there is a most recent editor
                let touch = if most_recent_editor.is_some() { Touch::AsOld } else { Touch::None };
                self.most_recent_editors_map.set(key_of(&key), key, touch);
            }
        }

        // Update in resource map if this is a new editor
        if is_new {
            self.update_editor_resources_map(editor, true);
        }

        // Event
        self.on_did_most_recently_active_editors_change.fire(());
    }

    fn update_editor_resources_map(&self, editor: &EditorInputRef, add: bool) {
        // Distill the editor resource and type id with support for side by side editor's primary side too.
        let target = side_by_side_input_sides(editor).map_or_else(|| editor.clone(), |(primary, _)| primary);
        let Some(resource) = target.resource() else {
            return; // require a resource
        };
        let identifier = to_identifier(&target.editor_type_id(), target.editor_id().as_deref());

        let mut editors_per_resource_counter = self.editors_per_resource_counter.borrow_mut();

        // Add entry
        if add {
            let mut editors_per_resource = editors_per_resource_counter.get(&resource).cloned().unwrap_or_default();
            *editors_per_resource.entry(identifier).or_insert(0) += 1;
            editors_per_resource_counter.set(resource, editors_per_resource);
        }
        // Remove entry
        else if let Some(mut editors_per_resource) = editors_per_resource_counter.get(&resource).cloned() {
            let counter = editors_per_resource.get(&identifier).copied().unwrap_or(0);
            if counter > 1 {
                editors_per_resource.insert(identifier, counter - 1);
                editors_per_resource_counter.set(resource, editors_per_resource);
            } else {
                editors_per_resource.remove(&identifier);

                if editors_per_resource.is_empty() {
                    editors_per_resource_counter.delete(&resource);
                } else {
                    editors_per_resource_counter.set(resource, editors_per_resource);
                }
            }
        }
    }

    fn remove_most_recent_editor(&self, group: &Rc<dyn IEditorGroup>, editor: &EditorInputRef) {
        // Update in resource map
        self.update_editor_resources_map(editor, false);

        // Update in MRU list
        let Some(key) = self.find_key(group, editor) else {
            return;
        };

        // Remove from most recent editors
        self.most_recent_editors_map.delete(&key_of(&key));

        // Remove from key map
        let mut key_map = self.key_map.borrow_mut();
        if let Some(map) = key_map.get_mut(&group.id())
            && map.remove(&id_of(editor)).is_some()
            && map.is_empty()
        {
            key_map.remove(&group.id());
        }
        drop(key_map);

        // Event
        self.on_did_most_recently_active_editors_change.fire(());
    }

    fn find_key(&self, group: &Rc<dyn IEditorGroup>, editor: &EditorInputRef) -> Option<EditorIdentifier> {
        self.key_map.borrow().get(&group.id())?.get(&id_of(editor)).cloned()
    }

    fn ensure_key(&self, group: &Rc<dyn IEditorGroup>, editor: &EditorInputRef) -> EditorIdentifier {
        self.key_map
            .borrow_mut()
            .entry(group.id())
            .or_default()
            .entry(id_of(editor))
            .or_insert_with(|| EditorIdentifier { group_id: group.id(), editor: editor.clone() })
            .clone()
    }

    /// `ensureOpenedEditorsLimit(exclude, groupId?)`.
    fn ensure_opened_editors_limit(self: &Rc<Self>, exclude: Option<EditorIdentifier>, group_id: Option<GroupIdentifier>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let part_options = this.editor_group_service.part_options();
            if !part_options.limit.enabled || part_options.limit.value <= 0.0 {
                return Ok(()); // return early if not enabled or invalid
            }

            let limit = part_options.limit.value;

            // In editor group
            if part_options.limit.per_editor_group {
                // For specific editor groups
                if let Some(group_id) = group_id {
                    if let Some(group) = this.editor_groups_container.get_group(group_id) {
                        let editors = group.get_editors(EditorsOrder::MostRecentlyActive, None).into_iter().map(|editor| EditorIdentifier { group_id, editor }).collect();
                        this.do_ensure_opened_editors_limit(limit, editors, exclude).await?;
                    }
                }
                // For all editor groups
                else {
                    for group in this.editor_groups_container.groups() {
                        this.ensure_opened_editors_limit(exclude.clone(), Some(group.id())).await?;
                    }
                }
            }
            // Across all editor groups
            else {
                this.do_ensure_opened_editors_limit(limit, this.editors(), exclude).await?;
            }

            Ok(())
        })
    }

    /// `doEnsureOpenedEditorsLimit(limit, mostRecentEditors, exclude?)`.
    fn do_ensure_opened_editors_limit(self: &Rc<Self>, limit: f64, most_recent_editors: Vec<EditorIdentifier>, exclude: Option<EditorIdentifier>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let part_options = this.editor_group_service.part_options();

            // Editors that opt out of the limit (e.g. the Agents window's managed docked tabs) never count towards it and are
            // never auto-closed.
            let most_recent_editors_counting_for_limit: Vec<EditorIdentifier> = most_recent_editors
                .into_iter()
                .filter(|EditorIdentifier { editor, .. }| {
                    if editor.has_capability(EditorInputCapabilities::EXCLUDE_FROM_EDITOR_LIMIT) {
                        return false;
                    }

                    // Check for `excludeDirty` setting and apply it by excluding any recent editor that is dirty from the
                    // opened editors limit
                    if part_options.limit.exclude_dirty && ((editor.is_dirty() && !editor.is_saving()) || editor.has_capability(EditorInputCapabilities::SCRATCHPAD)) {
                        return false; // not dirty editors (unless in the process of saving) or scratchpads
                    }

                    true
                })
                .collect();

            if limit >= most_recent_editors_counting_for_limit.len() as f64 {
                return Ok(()); // only if opened editors exceed setting and is valid and enabled
            }

            // Extract least recently used editors that can be closed
            let mut editors_to_close_count = most_recent_editors_counting_for_limit.len() as f64 - limit;
            let least_recently_closable_editors = most_recent_editors_counting_for_limit.into_iter().rev().filter(|EditorIdentifier { group_id, editor }| {
                if (editor.is_dirty() && !editor.is_saving()) || editor.has_capability(EditorInputCapabilities::SCRATCHPAD) {
                    return false; // not dirty editors (unless in the process of saving) or scratchpads
                }

                if exclude.as_ref().is_some_and(|exclude| id_of(editor) == id_of(&exclude.editor) && *group_id == exclude.group_id) {
                    return false; // never the editor that should be excluded
                }

                if this.editor_groups_container.get_group(*group_id).is_some_and(|group| group.is_sticky(&editor.into())) {
                    return false; // never sticky editors
                }

                true
            });

            // Close editors until we reached the limit again
            let mut map_group_to_editors_to_close: Vec<(GroupIdentifier, Vec<EditorInputRef>)> = Vec::new();
            for EditorIdentifier { group_id, editor } in least_recently_closable_editors {
                match map_group_to_editors_to_close.iter_mut().find(|(id, _)| *id == group_id) {
                    Some((_, editors_in_group_to_close)) => editors_in_group_to_close.push(editor),
                    None => map_group_to_editors_to_close.push((group_id, vec![editor])),
                }
                editors_to_close_count -= 1.0;

                if editors_to_close_count == 0.0 {
                    break; // limit reached
                }
            }

            for (group_id, editors) in map_group_to_editors_to_close {
                if let Some(group) = this.editor_groups_container.get_group(group_id) {
                    group.close_editors(CloseEditorsTarget::Editors(editors), Some(CloseEditorOptions { preserve_focus: Some(true), force: None })).await?;
                }
            }

            Ok(())
        })
    }

    fn save_state(&self) {
        if self.is_scoped {
            return; // do not persist state when scoped
        }

        if self.most_recent_editors_map.is_empty() {
            self.storage_service.remove(STORAGE_KEY, StorageScope::Workspace);
        } else {
            self.storage_service.store(STORAGE_KEY, StorageValue::String(stringify_json(&self.serialize())), StorageScope::Workspace, StorageTarget::Machine);
        }
    }

    /// `serialize()`: o `ISerializedEditorsList`.
    fn serialize(&self) -> JsonValue {
        let registry = editor_factory_registry();

        let mut map_group_to_serializable_editors_of_group: HashMap<GroupIdentifier, Vec<EditorInputRef>> = HashMap::new();
        let mut entries = Vec::new();

        for EditorIdentifier { group_id, editor } in self.editors() {
            // Find group for entry
            let Some(group) = self.editor_groups_container.get_group(group_id) else {
                continue;
            };

            // Find serializable editors of group
            let serializable_editors_of_group = map_group_to_serializable_editors_of_group.entry(group_id).or_insert_with(|| {
                group
                    .get_editors(EditorsOrder::Sequential, None)
                    .into_iter()
                    .filter(|editor| registry.get_editor_serializer(editor).is_some_and(|serializer| serializer.can_serialize(editor)))
                    .collect()
            });

            // Only store the index of the editor of that group which can be undefined if the editor is not serializable
            let Some(index) = serializable_editors_of_group.iter().position(|candidate| id_of(candidate) == id_of(&editor)) else {
                continue;
            };

            let mut entry = JsonObject::new();
            entry.insert(JsString::from("groupId"), JsonValue::Number(group_id as f64));
            entry.insert(JsString::from("index"), JsonValue::Number(index as f64));
            entries.push(JsonValue::Object(entry));
        }

        let mut serialized = JsonObject::new();
        serialized.insert(JsString::from("entries"), JsonValue::Array(entries));
        JsonValue::Object(serialized)
    }

    /// `loadState()`.
    fn load_state(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if this.editor_groups_container.is_main_part_of(&this.editor_group_service) {
                this.editor_group_service.when_ready().await?;
            }

            // Previous state: Load editors map from persisted state unless we are running in scoped mode
            let mut has_restorable_state = false;
            if !this.is_scoped
                && let Some(serialized) = this.storage_service.get(STORAGE_KEY, StorageScope::Workspace, None).filter(|serialized| !serialized.is_empty())
            {
                has_restorable_state = true;
                this.deserialize(&parse_json(&serialized)?)?;
            }

            // No previous state: best we can do is add each editor from oldest to most recently used editor group
            if !has_restorable_state {
                let groups = this.editor_groups_container.get_groups(GroupsOrder::MostRecentlyActive);
                for group in groups.iter().rev() {
                    let group_editors_mru = group.get_editors(EditorsOrder::MostRecentlyActive, None);
                    for editor in group_editors_mru.iter().rev() {
                        this.add_most_recent_editor(group, editor, true /* enforce as active to preserve order */, true /* is new */);
                    }
                }
            }

            // Ensure we listen on group changes for those that exist on startup
            for group in this.editor_groups_container.groups() {
                this.register_group_listeners(&group);
            }

            Ok(())
        })
    }

    /// `deserialize(serialized)`.
    fn deserialize(&self, serialized: &JsonValue) -> Result<(), Error> {
        let Some(JsonValue::Array(entries)) = (match serialized {
            JsonValue::Object(serialized) => serialized.get("entries"),
            _ => None,
        }) else {
            return Err(Error::type_error("serialized.entries is not iterable"));
        };

        let mut map_values: Vec<(EditorKey, EditorIdentifier)> = Vec::new();

        for entry in entries {
            let JsonValue::Object(entry) = entry else {
                return Err(Error::type_error("Cannot destructure property 'groupId' of 'undefined' or 'null'."));
            };
            let (Some(JsonValue::Number(group_id)), Some(JsonValue::Number(index))) = (entry.get("groupId"), entry.get("index")) else {
                continue;
            };

            // Find group for entry
            let Some(group) = self.editor_groups_container.get_group(*group_id as GroupIdentifier) else {
                continue;
            };

            // Find editor for entry
            if *index < 0.0 || index.fract() != 0.0 {
                continue;
            }
            let Some(editor) = group.get_editor_by_index(*index as usize) else {
                continue;
            };

            // Make sure key is registered as well
            let editor_identifier = self.ensure_key(&group, &editor);
            map_values.push((key_of(&editor_identifier), editor_identifier));

            // Update in resource map
            self.update_editor_resources_map(&editor, true);
        }

        // Fill map with deserialized values
        self.most_recent_editors_map.from_json(map_values);

        Ok(())
    }
}
