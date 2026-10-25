//! Porte do `EditorMemento` de `src/vs/workbench/browser/parts/editor/editorPane.ts`: o estado de visualização que um painel
//! de editor guarda por recurso e por grupo, num cache LRU persistido no memento do painel.
//!
//! O `EditorPane` em si (o ciclo `createEditor`/`setInput`/`clearInput`/`setVisible`) é o [`EditorPane`] lógico de
//! `editor_panes.rs`; aqui fica o que o `editorPane.ts` tem além dele.
//!
//! Divergências:
//! - **`T`.** O estado é qualquer valor que vira [`Dynamic`] ao gravar e volta dele ao carregar (o JSON do memento); o
//!   próprio [`Dynamic`] serve quando o estado é um objeto solto.
//! - **Sobrecargas.** `saveEditorState(group, resource | editor, state)` e as demais aceitam [`EditorStateTarget`], que se
//!   cria de `&URI` ou de `&EditorInputRef`.
//! - **`MapGroupToMemento`.** Um mapa compartilhado (`Rc<RefCell<..>>`) do identificador do grupo para o estado: o cache
//!   guarda a mesma instância que o original muta no lugar.
//! - **`_serviceBrand` e `IEditorMemento`.** Sem marca de serviço; a interface é o próprio [`EditorMemento`].
//! - **`getEditorMemento` do `EditorPane`.** O mapa estático `EDITOR_MEMENTOS` e o `saveState` do painel dependem do
//!   `Composite` e do memento do painel, que o painel lógico não tem; o chamador cria o [`EditorMemento`] sobre o objeto
//!   de memento que tiver.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableId, DisposableRef, IDisposable, id_of};
use je_base::map::LRUCache;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_platform::resources::ext_uri;

use crate::common::editor::{EditorInputRef, GroupIdentifier};
use crate::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService};

/// `MapGroupToMemento<T>`: o estado de um recurso, por grupo.
type MapGroupToMemento<T> = Rc<RefCell<BTreeMap<GroupIdentifier, T>>>;

/// O recurso ou o editor (`URI | EditorInput`) a que o estado se refere.
#[derive(Clone, Copy)]
pub enum EditorStateTarget<'a> {
    Resource(&'a URI),
    Editor(&'a EditorInputRef),
}

impl<'a> From<&'a URI> for EditorStateTarget<'a> {
    fn from(resource: &'a URI) -> Self {
        Self::Resource(resource)
    }
}

impl<'a> From<&'a EditorInputRef> for EditorStateTarget<'a> {
    fn from(editor: &'a EditorInputRef) -> Self {
        Self::Editor(editor)
    }
}

impl EditorStateTarget<'_> {
    /// `doGetResource(resourceOrEditor)`.
    fn resource(&self) -> Option<URI> {
        match self {
            Self::Resource(resource) => Some((*resource).clone()),
            Self::Editor(editor) => editor.resource(),
        }
    }
}

/// `EditorMemento<T>`.
pub struct EditorMemento<T: Clone + Into<Dynamic> + From<Dynamic> + 'static> {
    this: Weak<EditorMemento<T>>,
    base: Rc<Disposable>,
    /// `id`: o identificador do painel dono do memento.
    pub id: String,
    key: String,
    memento: ObjectRef,
    limit: usize,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    configuration_service: Rc<dyn ITextResourceConfigurationService>,
    cache: RefCell<Option<Rc<LRUCache<String, MapGroupToMemento<T>>>>>,
    cleaned_up: Cell<bool>,
    editor_disposables: RefCell<HashMap<DisposableId, DisposableRef>>,
    share_editor_state: Cell<bool>,
}

impl<T: Clone + Into<Dynamic> + From<Dynamic> + 'static> EditorMemento<T> {
    /// `SHARED_EDITOR_STATE`: um número < 0, fora da faixa de ids de grupo.
    const SHARED_EDITOR_STATE: GroupIdentifier = -1;

    pub fn new(
        id: &str,
        key: &str,
        memento: ObjectRef,
        limit: usize,
        editor_group_service: Rc<dyn IEditorGroupsService>,
        configuration_service: Rc<dyn ITextResourceConfigurationService>,
    ) -> Rc<Self> {
        let editor_memento = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            id: id.to_string(),
            key: key.to_string(),
            memento,
            limit,
            editor_group_service,
            configuration_service,
            cache: RefCell::new(None),
            cleaned_up: Cell::new(false),
            editor_disposables: RefCell::new(HashMap::new()),
            share_editor_state: Cell::new(false),
        });

        editor_memento.update_configuration();
        editor_memento.register_listeners();

        editor_memento
    }

    fn register_listeners(&self) {
        let this = self.this.clone();
        let listener = self.configuration_service.on_did_change_configuration().listen(move |e| {
            if let Some(this) = this.upgrade()
                && e.affects_configuration(None, "workbench.editor.sharedViewState")
            {
                this.update_configuration();
            }
        });
        self.base.register(listener);
    }

    /// `updateConfiguration(e)`: o chamador já decidiu que `!e || e.affectsConfiguration(...)`.
    fn update_configuration(&self) {
        self.share_editor_state.set(self.configuration_service.get_value(None, Some("workbench.editor.sharedViewState")) == Dynamic::Bool(true));
    }

    /// `saveEditorState(group, resourceOrEditor, state)`.
    pub fn save_editor_state<'a>(&self, group: &dyn IEditorGroup, resource_or_editor: impl Into<EditorStateTarget<'a>>, state: T) {
        let target = resource_or_editor.into();
        let Some(resource) = target.resource() else {
            return; // we are not in a good state to save any state for a resource
        };

        let cache = self.do_load();

        // Ensure mementos for resource map
        let key = resource.to_string(false).to_string_lossy();
        let mementos_for_resource = cache.get(&key).unwrap_or_else(|| {
            let created: MapGroupToMemento<T> = Rc::default();
            cache.set(key, created.clone());
            created
        });

        // Store state for group
        mementos_for_resource.borrow_mut().insert(group.id(), state.clone());

        // Store state as most recent one based on settings
        if self.share_editor_state.get() {
            mementos_for_resource.borrow_mut().insert(Self::SHARED_EDITOR_STATE, state);
        }

        // Automatically clear when editor input gets disposed if any
        if let EditorStateTarget::Editor(editor) = target {
            self.clear_editor_state_on_dispose(&resource, editor);
        }
    }

    /// `loadEditorState(group, resourceOrEditor)`.
    pub fn load_editor_state<'a>(&self, group: &dyn IEditorGroup, resource_or_editor: impl Into<EditorStateTarget<'a>>) -> Option<T> {
        let resource = resource_or_editor.into().resource()?; // we are not in a good state to load any state for a resource

        let cache = self.do_load();

        let mementos_for_resource = cache.get(&resource.to_string(false).to_string_lossy())?;
        let mementos = mementos_for_resource.borrow();

        // Return state for group if present
        if let Some(memento_for_resource_and_group) = mementos.get(&group.id()) {
            return Some(memento_for_resource_and_group.clone());
        }

        // Return most recent state based on settings otherwise
        if self.share_editor_state.get() {
            return mementos.get(&Self::SHARED_EDITOR_STATE).cloned();
        }

        None
    }

    /// `clearEditorState(resourceOrEditor, group?)`.
    pub fn clear_editor_state<'a>(&self, resource_or_editor: impl Into<EditorStateTarget<'a>>, group: Option<&dyn IEditorGroup>) {
        let target = resource_or_editor.into();
        if let EditorStateTarget::Editor(editor) = target {
            self.editor_disposables.borrow_mut().remove(&id_of(editor));
        }

        let Some(resource) = target.resource() else {
            return;
        };
        let cache = self.do_load();
        let key = resource.to_string(false).to_string_lossy();

        match group {
            // Clear state for group
            Some(group) => {
                if let Some(mementos_for_resource) = cache.get(&key) {
                    mementos_for_resource.borrow_mut().remove(&group.id());

                    if mementos_for_resource.borrow().is_empty() {
                        cache.delete(&key);
                    }
                }
            }

            // Clear state across all groups for resource
            None => {
                cache.delete(&key);
            }
        }
    }

    /// `clearEditorStateOnDispose(resource, editor)`.
    pub fn clear_editor_state_on_dispose(&self, resource: &URI, editor: &EditorInputRef) {
        let editor_id = id_of(editor);
        if self.editor_disposables.borrow().contains_key(&editor_id) {
            return;
        }

        let (this, resource) = (self.this.clone(), resource.clone());
        let subscription = editor.on_will_dispose().once().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.clear_editor_state(&resource, None);
                this.editor_disposables.borrow_mut().remove(&editor_id);
            }
        });
        self.editor_disposables.borrow_mut().insert(editor_id, subscription);
    }

    /// `moveEditorState(source, target, comparer)`.
    pub fn move_editor_state(&self, source: &URI, target: &URI, comparer: &ExtUri) {
        let cache = self.do_load();

        // We need a copy of the keys to not iterate over newly inserted elements.
        let cache_keys: Vec<String> = cache.keys().collect();
        for cache_key in cache_keys {
            let resource = URI::parse(cache_key.as_str(), false);

            if !comparer.is_equal_or_parent(&resource, source, false, false) {
                continue; // not matching our resource
            }

            // Determine new resulting target resource
            let target_resource = if ext_uri().is_equal(Some(source), Some(&resource), false) {
                target.clone() // file got moved
            } else {
                // parent folder got moved
                let (resource_path, source_path) = (resource.path(), source.path());
                let index = je_base::extpath::index_of_path(&resource_path.to_string_lossy(), &source_path.to_string_lossy(), false).unwrap_or(0);
                let rest = JsString::from_units(resource_path.units().iter().skip(index + source_path.len() + 1).copied().collect());
                je_platform::resources::join_path(target, &[rest.to_string_lossy().as_str()])
            };

            // Don't modify LRU state
            if let Some(value) = cache.peek(&cache_key) {
                cache.delete(&cache_key);
                cache.set(target_resource.to_string(false).to_string_lossy(), value);
            }
        }
    }

    /// `doLoad()`.
    fn do_load(&self) -> Rc<LRUCache<String, MapGroupToMemento<T>>> {
        if let Some(cache) = self.cache.borrow().as_ref() {
            return cache.clone();
        }

        let cache = Rc::new(LRUCache::new(self.limit, 1.0));

        // Restore from serialized map state
        if let Dynamic::Array(raw_editor_memento) = self.memento.get(&JsString::from(self.key.as_str())).unwrap_or(Dynamic::Undefined) {
            cache.from_json(raw_editor_memento.to_vec().iter().filter_map(Self::entry_from_json).collect());
        }

        *self.cache.borrow_mut() = Some(cache.clone());
        cache
    }

    /// Um item de `LRUCache.fromJSON`: `[resource, { [group]: state }]`.
    fn entry_from_json(entry: &Dynamic) -> Option<(String, MapGroupToMemento<T>)> {
        let Dynamic::Array(pair) = entry else {
            return None;
        };
        let Dynamic::String(resource) = pair.get(0) else {
            return None;
        };
        let mementos: BTreeMap<GroupIdentifier, T> = pair
            .get(1)
            .own_entries()
            .into_iter()
            .filter_map(|(group, state)| Some((group.to_string_lossy().parse().ok()?, T::from(state))))
            .collect();

        Some((resource.to_string_lossy(), Rc::new(RefCell::new(mementos))))
    }

    /// `LRUCache.toJSON()` de um item: `[resource, { [group]: state }]`.
    fn entry_to_json((resource, mementos): (String, MapGroupToMemento<T>)) -> Dynamic {
        let states = mementos.borrow().iter().map(|(group, state)| (group.to_string(), state.clone().into())).collect::<Vec<(String, Dynamic)>>();

        Dynamic::array([Dynamic::string(&resource), Dynamic::object(states)])
    }

    /// `saveState()`.
    pub fn save_state(&self) {
        let cache = self.do_load();

        // Cleanup once during session
        if !self.cleaned_up.get() {
            self.clean_up();
            self.cleaned_up.set(true);
        }

        self.memento.set(&JsString::from(self.key.as_str()), Dynamic::array(cache.to_json().into_iter().map(Self::entry_to_json)));
    }

    fn clean_up(&self) {
        let cache = self.do_load();

        // Remove groups from states that no longer exist. Since we modify the cache and its is a LRU cache make a copy to
        // ensure iteration succeeds
        for (resource, map_group_to_mementos) in cache.entries().collect::<Vec<_>>() {
            let group_ids: Vec<GroupIdentifier> = map_group_to_mementos.borrow().keys().copied().collect();
            for group_id in group_ids {
                if group_id == Self::SHARED_EDITOR_STATE && self.share_editor_state.get() {
                    continue; // skip over shared entries if sharing is enabled
                }

                if self.editor_group_service.get_group(group_id).is_none() {
                    map_group_to_mementos.borrow_mut().remove(&group_id);
                    if map_group_to_mementos.borrow().is_empty() {
                        cache.delete(&resource);
                    }
                }
            }
        }
    }
}

impl<T: Clone + Into<Dynamic> + From<Dynamic> + 'static> IDisposable for EditorMemento<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
