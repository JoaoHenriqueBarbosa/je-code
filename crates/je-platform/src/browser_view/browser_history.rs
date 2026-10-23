//! Porte de `src/vs/platform/browserView/common/browserHistory.ts`: o histórico por sessão do navegador
//! embutido, dividido em dois repositórios (as entradas e o mapa de favicons, que é grande e só é regravado
//! quando uma imagem entra ou sai) e o repositório que os une.
//!
//! Divergências:
//! - Os números do original (`id`, `time`, `maxEntries`) são `f64`, como o `number` do JavaScript: o que vem
//!   do disco passa só por `typeof === 'number'`, então ids não inteiros e `NaN` valem.
//! - O `hydrate` recebe o dado cru do disco (`unknown` no original) como [`Dynamic`], e é ele que
//!   `isValidEntry` valida; os snapshots tipados têm `to_dynamic` para isso. Campos desconhecidos de uma
//!   entrada válida não são mantidos (o original guarda o objeto lido inteiro).
//! - `explicit?: true` é `explicit: bool` (`false` é o campo ausente); `icon?` é `Option<String>`.
//! - `items` devolve uma cópia da lista (no original, a lista viva só de leitura).
//! - `update` do repositório de entradas recebe [`BrowserHistoryEntryPatch`], em que `faviconHash?: string | null`
//!   é `Option<Option<String>>` (`None` deixa como está, `Some(None)` limpa). O mesmo vale para o
//!   `favicon` de [`BrowserHistoryUpdate`].
//! - O handle `NOOP_HANDLE` (id `-1`, sem efeito) é um [`BrowserHistoryItemHandle`] sem alvo.
//! - `Date.now()` é [`je_base::runtime::date_now`], que obedece ao relógio virtual dos testes.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::hash::StringSha1;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::runtime::date_now;

/// On-disk shape of a single history entry.
/// BACKWARDS COMPATIBLE. When evolving this interface, ensure older versions can still be handled gracefully.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedBrowserHistoryEntry {
    pub id: f64,
    pub url: String,
    /// Epoch ms when the entry was most recently visited.
    pub time: f64,
    pub title: String,
    /// Content hash key into the sibling favicons map.
    pub icon: Option<String>,
    /// Set when the navigation was initiated by the user (typing in the URL bar, picking a suggestion,
    /// opening a new tab with a URL) rather than by page script or link clicks. Always omitted when
    /// false to keep entries small.
    pub explicit: bool,
}

impl SerializedBrowserHistoryEntry {
    /// A forma em disco como valor dinâmico: `icon` e `explicit` só existem quando têm valor.
    pub fn to_dynamic(&self) -> Dynamic {
        let mut fields = vec![
            ("id", Dynamic::Number(self.id)),
            ("url", Dynamic::string(&self.url)),
            ("time", Dynamic::Number(self.time)),
            ("title", Dynamic::string(&self.title)),
        ];
        if let Some(icon) = &self.icon {
            fields.push(("icon", Dynamic::string(icon)));
        }
        if self.explicit {
            fields.push(("explicit", Dynamic::Bool(true)));
        }
        Dynamic::object(fields)
    }

    /// `isValidEntry`: o `typeof` de cada campo e `explicit` só `undefined` ou `true`. `None` é entrada inválida.
    fn from_dynamic(value: &Dynamic) -> Option<Self> {
        if !matches!(value, Dynamic::Object(_)) {
            return None;
        }
        let Dynamic::Number(id) = value.get_str("id") else { return None };
        let Dynamic::String(url) = value.get_str("url") else { return None };
        let Dynamic::Number(time) = value.get_str("time") else { return None };
        let Dynamic::String(title) = value.get_str("title") else { return None };
        let icon = match value.get_str("icon") {
            Dynamic::Undefined => None,
            Dynamic::String(icon) => Some(icon.to_string_lossy()),
            _ => return None,
        };
        let explicit = match value.get_str("explicit") {
            Dynamic::Undefined => false,
            Dynamic::Bool(true) => true,
            _ => return None,
        };
        Some(Self { id, url: url.to_string_lossy(), time, title: title.to_string_lossy(), icon, explicit })
    }
}

/// In-memory representation of a history entry. Currently identical to the on-disk shape; the split
/// exists so future in-memory-only fields can be added here without changing the wire format.
pub type BrowserHistoryEntry = SerializedBrowserHistoryEntry;

/// A mudança de [`BrowserHistoryStore`] por [`BrowserHistoryItemHandle::update`].
#[derive(Clone, Debug, Default)]
pub struct BrowserHistoryUpdate {
    /// URL may be updated e.g. during a redirect or in-page navigation.
    pub url: Option<String>,
    pub title: Option<String>,
    /// Favicon data URI; hashed and deduped against the sibling favicons store. `Some(None)` explicitly clears.
    pub favicon: Option<Option<String>>,
}

/// A mudança de [`BrowserHistoryEntriesStore::update`] (`{ url?, title?, faviconHash? }`).
#[derive(Clone, Debug, Default)]
pub struct BrowserHistoryEntryPatch {
    pub url: Option<String>,
    pub title: Option<String>,
    pub favicon_hash: Option<Option<String>>,
}

/// On-disk shape of an entries snapshot. See [`SerializedBrowserHistoryEntry`] for the
/// backwards-compatibility rules; the same constraints apply here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SerializedBrowserHistoryEntriesSnapshot {
    pub items: Vec<SerializedBrowserHistoryEntry>,
}

impl SerializedBrowserHistoryEntriesSnapshot {
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([("items", Dynamic::array(self.items.iter().map(SerializedBrowserHistoryEntry::to_dynamic)))])
    }
}

/// On-disk shape of a favicons snapshot. See [`SerializedBrowserHistoryEntry`] for the
/// backwards-compatibility rules; the same constraints apply here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SerializedBrowserFaviconsSnapshot {
    /// Map from content hash to data URI.
    pub map: IndexMap<String, String>,
}

impl SerializedBrowserFaviconsSnapshot {
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([("map", Dynamic::object(self.map.iter().map(|(hash, uri)| (hash.as_str(), Dynamic::string(uri)))))])
    }
}

const DEFAULT_MAX_ENTRIES: f64 = 200.0;

pub struct BrowserHistoryEntriesStore {
    base: Rc<Disposable>,
    next_id: Cell<f64>,
    items: RefCell<Vec<BrowserHistoryEntry>>,
    max_entries: Cell<f64>,
    on_did_change: Rc<Emitter<()>>,
}

impl BrowserHistoryEntriesStore {
    /// `new BrowserHistoryEntriesStore(maxEntries)`: sem argumento vale 200.
    pub fn new(max_entries: Option<f64>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        Rc::new(Self {
            base,
            next_id: Cell::new(1.0),
            items: RefCell::new(Vec::new()),
            max_entries: Cell::new(max_entries.unwrap_or(DEFAULT_MAX_ENTRIES)),
            on_did_change,
        })
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `items`: as entradas, da mais antiga para a mais nova.
    pub fn items(&self) -> Vec<BrowserHistoryEntry> {
        self.items.borrow().clone()
    }

    pub fn max_entries(&self) -> f64 {
        self.max_entries.get()
    }

    pub fn set_max_entries(&self, max: f64) {
        if max < 0.0 || max == self.max_entries.get() {
            return;
        }
        self.max_entries.set(max);
        if self.evict_if_needed() {
            self.on_did_change.fire(());
        }
    }

    pub fn add(&self, url: &str, title: &str, favicon_hash: Option<&str>, user_initiated: bool) -> BrowserHistoryEntry {
        let id = self.next_id.get();
        self.next_id.set(id + 1.0);
        let entry = BrowserHistoryEntry {
            id,
            url: url.to_owned(),
            time: date_now() as f64,
            title: title.to_owned(),
            icon: favicon_hash.map(str::to_owned),
            explicit: user_initiated,
        };
        self.items.borrow_mut().push(entry.clone());
        self.evict_if_needed();
        self.on_did_change.fire(());
        entry
    }

    pub fn update(&self, id: f64, patch: &BrowserHistoryEntryPatch) -> bool {
        let mut items = self.items.borrow_mut();
        let Some(idx) = Self::index_of(&items, id) else {
            return false;
        };
        let existing = items[idx].clone();
        let next_title = patch.title.as_ref().filter(|t| !t.is_empty()).unwrap_or(&existing.title).clone();
        let next_url = patch.url.as_ref().filter(|u| !u.is_empty()).unwrap_or(&existing.url).clone();
        // Distinguish "leave alone" (None) from explicit clear (Some(None)).
        let next_favicon_hash = match &patch.favicon_hash {
            None => existing.icon.clone(),
            Some(hash) => hash.clone(),
        };
        // Update the time if the URL has been updated.
        let next_time = if patch.url.as_ref().is_some_and(|u| !u.is_empty()) { date_now() as f64 } else { existing.time };
        if next_url == existing.url && next_title == existing.title && next_favicon_hash == existing.icon && next_time == existing.time {
            return false;
        }
        items[idx] = BrowserHistoryEntry { url: next_url, title: next_title, icon: next_favicon_hash, time: next_time, ..existing };
        drop(items);
        self.on_did_change.fire(());
        true
    }

    pub fn delete(&self, id: f64) -> bool {
        let mut items = self.items.borrow_mut();
        let Some(idx) = Self::index_of(&items, id) else {
            return false;
        };
        items.remove(idx);
        drop(items);
        self.on_did_change.fire(());
        true
    }

    pub fn clear(&self) {
        if self.items.borrow().is_empty() && self.next_id.get() == 1.0 {
            return;
        }
        self.items.borrow_mut().clear();
        self.next_id.set(1.0);
        self.on_did_change.fire(());
    }

    pub fn serialize(&self) -> SerializedBrowserHistoryEntriesSnapshot {
        SerializedBrowserHistoryEntriesSnapshot { items: self.items() }
    }

    /// `hydrate(snapshot)`: `snapshot` é o dado cru lido do disco; entradas malformadas são descartadas.
    pub fn hydrate(&self, snapshot: Option<&Dynamic>) {
        self.items.borrow_mut().clear();
        self.next_id.set(1.0);
        if let Some(snapshot) = snapshot.filter(|s| s.is_truthy())
            && let Dynamic::Array(raw_items) = snapshot.get_str("items")
        {
            let mut items = self.items.borrow_mut();
            items.extend(raw_items.to_vec().iter().filter_map(SerializedBrowserHistoryEntry::from_dynamic));
            // Restored ids must not collide with future adds.
            for entry in items.iter() {
                if entry.id >= self.next_id.get() {
                    self.next_id.set(entry.id + 1.0);
                }
            }
            drop(items);
            self.evict_if_needed();
        }
        self.on_did_change.fire(());
    }

    /// Walk newest-first; mutations target the just-added entry in the common case.
    fn index_of(items: &[BrowserHistoryEntry], id: f64) -> Option<usize> {
        items.iter().rposition(|entry| entry.id == id)
    }

    fn evict_if_needed(&self) -> bool {
        let mut items = self.items.borrow_mut();
        let max_entries = self.max_entries.get();
        if items.len() as f64 > max_entries {
            let excess = ((items.len() as f64 - max_entries) as usize).min(items.len());
            items.drain(..excess);
            return true;
        }
        false
    }
}

impl IDisposable for BrowserHistoryEntriesStore {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// Lives separately from [`BrowserHistoryEntriesStore`] so the (large) favicon map is only rewritten
/// when an image is added or removed, not on every navigation.
pub struct BrowserFaviconsStore {
    base: Rc<Disposable>,
    by_hash: RefCell<IndexMap<String, String>>,
    on_did_change: Rc<Emitter<()>>,
}

impl BrowserFaviconsStore {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        Rc::new(Self { base, by_hash: RefCell::new(IndexMap::new()), on_did_change })
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    pub fn get(&self, hash: &str) -> Option<String> {
        self.by_hash.borrow().get(hash).cloned()
    }

    pub fn register(&self, data_uri: &str) -> String {
        let mut sha = StringSha1::new();
        sha.update(&data_uri.encode_utf16().collect::<Vec<u16>>());
        let hash = sha.digest();
        let added = {
            let mut by_hash = self.by_hash.borrow_mut();
            let missing = !by_hash.contains_key(&hash);
            if missing {
                by_hash.insert(hash.clone(), data_uri.to_owned());
            }
            missing
        };
        if added {
            self.on_did_change.fire(());
        }
        hash
    }

    pub fn gc(&self, referenced: &HashSet<String>) {
        let changed = {
            let mut by_hash = self.by_hash.borrow_mut();
            if by_hash.is_empty() {
                return;
            }
            let before = by_hash.len();
            by_hash.retain(|hash, _| referenced.contains(hash));
            by_hash.len() != before
        };
        if changed {
            self.on_did_change.fire(());
        }
    }

    pub fn clear(&self) {
        if self.by_hash.borrow().is_empty() {
            return;
        }
        self.by_hash.borrow_mut().clear();
        self.on_did_change.fire(());
    }

    pub fn serialize(&self) -> SerializedBrowserFaviconsSnapshot {
        SerializedBrowserFaviconsSnapshot { map: self.by_hash.borrow().clone() }
    }

    /// `hydrate(snapshot)`: `snapshot` é o dado cru lido do disco; valores que não são texto são descartados.
    pub fn hydrate(&self, snapshot: Option<&Dynamic>) {
        let mut by_hash = self.by_hash.borrow_mut();
        by_hash.clear();
        if let Some(snapshot) = snapshot.filter(|s| !s.is_nullish()) {
            let map = snapshot.get_str("map");
            if map.is_truthy() && map.type_of() == "object" {
                for (key, value) in map.own_entries() {
                    if let Dynamic::String(value) = value {
                        by_hash.insert(key.to_string_lossy(), value.to_string_lossy());
                    }
                }
            }
        }
        drop(by_hash);
        self.on_did_change.fire(());
    }
}

impl IDisposable for BrowserFaviconsStore {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// Handle returned by [`BrowserHistoryStore::add`]. `update` and `delete` are no-ops once the underlying
/// entry has been evicted, and always when the store is disabled (max entries = 0), caso em que o handle
/// não tem alvo e o id é `-1` (o `NOOP_HANDLE` do original).
pub struct BrowserHistoryItemHandle {
    id: f64,
    target: Option<HandleTarget>,
}

struct HandleTarget {
    entries: Rc<BrowserHistoryEntriesStore>,
    favicons: Rc<BrowserFaviconsStore>,
}

impl BrowserHistoryItemHandle {
    pub fn id(&self) -> f64 {
        self.id
    }

    pub fn update(&self, patch: &BrowserHistoryUpdate) {
        let Some(target) = &self.target else {
            return;
        };
        let mut next = BrowserHistoryEntryPatch::default();
        if patch.url.is_some() {
            next.url = patch.url.clone();
        }
        if patch.title.is_some() {
            next.title = patch.title.clone();
        }
        if let Some(favicon) = &patch.favicon {
            // None is the explicit-clear sentinel; a data URI registers and references it.
            next.favicon_hash = Some(favicon.as_deref().map(|uri| target.favicons.register(uri)));
        }
        target.entries.update(self.id, &next);
    }

    pub fn delete(&self) {
        if let Some(target) = &self.target {
            target.entries.delete(self.id);
        }
    }
}

/// Per-session browser history. The two sub-stores are exposed directly so persistence layers can flush
/// them independently.
pub struct BrowserHistoryStore {
    base: Rc<Disposable>,
    pub entries: Rc<BrowserHistoryEntriesStore>,
    pub favicons: Rc<BrowserFaviconsStore>,
    on_did_change: Rc<Emitter<()>>,
}

impl BrowserHistoryStore {
    pub fn new(max_entries: Option<f64>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let entries = BrowserHistoryEntriesStore::new(max_entries);
        base.register(entries.clone());
        let favicons = BrowserFaviconsStore::new();
        base.register(favicons.clone());

        let entries_ref = Rc::downgrade(&entries);
        let favicons_ref = favicons.clone();
        let changed = on_did_change.clone();
        base.register(entries.on_did_change().listen(move |_| {
            if let Some(entries) = Weak::upgrade(&entries_ref) {
                gc_favicons(&entries, &favicons_ref);
            }
            changed.fire(());
        }));
        let changed = on_did_change.clone();
        base.register(favicons.on_did_change().listen(move |_| changed.fire(())));

        Rc::new(Self { base, entries, favicons, on_did_change })
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `add(url, title, favicon?, userInitiated = false)`.
    pub fn add(&self, url: &str, title: &str, favicon: Option<&str>, user_initiated: bool) -> BrowserHistoryItemHandle {
        if self.entries.max_entries() == 0.0 {
            // History disabled: skip favicon hashing and entry creation entirely.
            return BrowserHistoryItemHandle { id: -1.0, target: None };
        }
        let favicon_hash = favicon.filter(|f| !f.is_empty()).map(|f| self.favicons.register(f));
        let entry = self.entries.add(url, title, favicon_hash.as_deref(), user_initiated);
        BrowserHistoryItemHandle { id: entry.id, target: Some(HandleTarget { entries: self.entries.clone(), favicons: self.favicons.clone() }) }
    }

    pub fn set_max_entries(&self, max: f64) {
        self.entries.set_max_entries(max);
    }

    pub fn clear(&self) {
        self.entries.clear();
        self.favicons.clear();
    }
}

impl IDisposable for BrowserHistoryStore {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// Descarta os favicons que nenhuma entrada referencia.
fn gc_favicons(entries: &BrowserHistoryEntriesStore, favicons: &BrowserFaviconsStore) {
    let referenced: HashSet<String> = entries.items().into_iter().filter_map(|entry| entry.icon).filter(|icon| !icon.is_empty()).collect();
    favicons.gc(&referenced);
}
