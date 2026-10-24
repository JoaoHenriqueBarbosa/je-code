//! Porte de `src/vs/workbench/browser/parts/statusbar/statusbarModel.ts`: o modelo de visão da barra de
//! status ([`StatusbarViewModel`]), que guarda as entradas na ordem de exibição (prioridade primária e
//! secundária, ou posição relativa a outra entrada), lembra quais estão escondidas (no armazenamento do
//! perfil, chave `workbench.statusbar.hidden`) e controla o foco por teclado.
//!
//! Divergências:
//! - **`HTMLElement` das entradas.** `container` e `labelContainer` são identificadores opacos
//!   ([`StatusbarElementId`]): quem desenha a barra os atribui e resolve para um nó da sua árvore de widgets.
//!   O modelo só os compara (`findEntry`) e os repassa a [`StatusbarEntryDom`].
//! - **O que o original faz com o DOM** vira o trait [`StatusbarEntryDom`], passado ao construtor (o original
//!   só recebe o `IStorageService`): `hide(container)` e `show(container)` de `dom.ts`
//!   ([`StatusbarEntryDom::set_visible`]), `isAncestorOfActiveElement(container)`
//!   ([`StatusbarEntryDom::is_ancestor_of_active_element`]), `labelContainer.focus()`
//!   ([`StatusbarEntryDom::focus`]) e as classes CSS `first-visible-item` e `last-visible-item` do
//!   `classList` ([`StatusbarEntryDom::remove_visible_edge_marks`] e
//!   [`StatusbarEntryDom::add_visible_edge_mark`]). A ordem das chamadas é a do original.
//! - **Identidade das entradas.** O original compara objetos (`indexOf(entry)`, `remove(entry)`); aqui as
//!   entradas são `Rc<StatusbarViewModelEntry>` e a identidade é `Rc::ptr_eq`. Como no original, duas entradas
//!   podem ter o mesmo `id` (por isso a lista não é um mapa).
//! - **`hidden`** é um `Set<string>` do JavaScript, então é uma lista que mantém a ordem de inserção sem
//!   repetidos (a ordem vai para o JSON gravado). Ao restaurar, o `JSON.parse` que não resulta em uma lista
//!   de textos é tratado como o original o trata: `null` esvazia, um texto vira o conjunto dos seus
//!   caracteres, outros valores (número, objeto, booleano) lançam `TypeError` no `new Set(...)` e são
//!   ignorados. Itens de uma lista que não são texto (nunca casam com um `id`) são descartados em vez de
//!   guardados.
//! - **Ordenação.** `Array.prototype.sort` com o comparador do original é [`sort_by_compare`], que segue a
//!   sequência de chamadas do V8 (inclusive para prioridades `NaN`, que o original trata como empate).
//! - `onDidChangeEntryVisibility` é [`StatusbarViewModel::on_did_change_entry_visibility`], e `entries`,
//!   `lastFocusedEntry` e `entries.slice(0)` são os métodos de mesmo nome em snake case.
//! - As sobrecargas privadas `updateVisibility(id, trigger)` e `updateVisibility(entry, trigger)` são
//!   [`StatusbarViewModel::update_visibility_by_id`] e [`StatusbarViewModel::update_visibility_of_entry`].

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::sort_by_compare;
use je_base::event::{Emitter, Event};
use je_base::json::{Value, parse_str};
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

use crate::services::statusbar::statusbar::{StatusbarAlignment, StatusbarEntryPriority, StatusbarEntryPrimaryPriority};

/// O `HTMLElement` de uma entrada, como identificador opaco: o `container` (o item inteiro) e o
/// `labelContainer` (o elemento que recebe o foco) de [`StatusbarViewModelEntry`]. Quem desenha a barra
/// escolhe os valores e sabe a que nó cada um corresponde.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StatusbarElementId(pub u64);

/// As classes que marcam as pontas visíveis de cada lado da barra no `classList` do `container`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibleEdgeMark {
    /// `first-visible-item`.
    First,
    /// `last-visible-item`.
    Last,
}

impl VisibleEdgeMark {
    /// O nome da classe CSS do original.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::First => "first-visible-item",
            Self::Last => "last-visible-item",
        }
    }
}

/// Tudo que o `StatusbarViewModel` faz com `HTMLElement` no original. Quem desenha a barra o implementa sobre
/// a sua árvore de widgets. Os métodos recebem `&self`: o modelo é compartilhado, e a implementação guarda o
/// estado que precisar atrás de uma célula.
pub trait StatusbarEntryDom {
    /// `hide(container)` (`visible == false`) e `show(container)` (`visible == true`) de `dom.ts`.
    fn set_visible(&self, container: StatusbarElementId, visible: bool);

    /// `isAncestorOfActiveElement(container)`: se o foco do teclado está dentro do `container`.
    fn is_ancestor_of_active_element(&self, container: StatusbarElementId) -> bool;

    /// `labelContainer.focus()`.
    fn focus(&self, label_container: StatusbarElementId);

    /// `container.classList.remove('first-visible-item', 'last-visible-item')`.
    fn remove_visible_edge_marks(&self, container: StatusbarElementId);

    /// `container.classList.add('first-visible-item')` ou `container.classList.add('last-visible-item')`.
    fn add_visible_edge_mark(&self, container: StatusbarElementId, mark: VisibleEdgeMark);
}

/// `IStatusbarViewModelEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusbarViewModelEntry {
    pub id: String,
    pub extension_id: Option<String>,
    pub name: String,
    pub has_command: bool,
    pub alignment: StatusbarAlignment,
    pub priority: StatusbarEntryPriority,
    pub container: StatusbarElementId,
    pub label_container: StatusbarElementId,
}

/// O `{ id: string; visible: boolean }` de `onDidChangeEntryVisibility`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusbarEntryVisibilityChange {
    pub id: String,
    pub visible: bool,
}

type EntryRef = Rc<StatusbarViewModelEntry>;

const HIDDEN_ENTRIES_KEY: &str = "workbench.statusbar.hidden";

/// O `new Set(JSON.parse(hiddenRaw))` do original, a partir do valor já lido.
fn hidden_from_json(value: Value) -> Option<Vec<String>> {
    let mut hidden: Vec<String> = Vec::new();
    let mut insert = |id: String| {
        if !hidden.contains(&id) {
            hidden.push(id);
        }
    };
    match value {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                if let Value::String(text) = item {
                    insert(text.to_string_lossy());
                }
            }
        }
        Value::String(text) => {
            for character in text.to_string_lossy().chars() {
                insert(character.to_string());
            }
        }
        // `new Set(true)`, `new Set(1)` e `new Set({})` lançam `TypeError` (não iterável)
        Value::Bool(_) | Value::Number(_) | Value::Object(_) => return None,
    }
    Some(hidden)
}

fn entry_at(entries: &[EntryRef], index: isize) -> Option<&EntryRef> {
    usize::try_from(index).ok().and_then(|index| entries.get(index))
}

/// A prioridade primária como número: a própria, ou a de reserva da localização referenciada.
fn primary_priority_number(entry: &StatusbarViewModelEntry) -> f64 {
    match &entry.priority.primary {
        StatusbarEntryPrimaryPriority::Number(priority) => *priority,
        StatusbarEntryPrimaryPriority::Location(location) => location.location.priority,
    }
}

/// `entries.sort((entryA, entryB) => entryB.priority.secondary - entryA.priority.secondary)`.
fn sort_by_secondary_priority(entries: &mut [EntryRef]) {
    sort_by_compare(entries, |entry_a, entry_b| entry_b.priority.secondary - entry_a.priority.secondary);
}

/// As entradas relativas a `alignment` do lugar da referenciada, na ordem do `secondary`.
fn relative_entries_to(relative_entries: &[EntryRef], alignment: StatusbarAlignment) -> Vec<EntryRef> {
    let mut entries: Vec<EntryRef> = relative_entries
        .iter()
        .filter(|entry| entry.priority.primary.location().is_some_and(|location| location.alignment == alignment))
        .cloned()
        .collect();
    sort_by_secondary_priority(&mut entries);
    entries
}

/// `StatusbarViewModel`.
pub struct StatusbarViewModel {
    base: Rc<Disposable>,
    storage_service: Rc<dyn StorageService>,
    dom: Rc<dyn StatusbarEntryDom>,
    on_did_change_entry_visibility: Rc<Emitter<StatusbarEntryVisibilityChange>>,
    /// Intentionally not using a map here since multiple entries can have the same ID.
    entries: RefCell<Vec<EntryRef>>,
    last_focused_entry: RefCell<Option<EntryRef>>,
    /// O `Set<string>` dos ids escondidos, na ordem de inserção.
    hidden: RefCell<Vec<String>>,
}

impl StatusbarViewModel {
    pub fn new(storage_service: Rc<dyn StorageService>, dom: Rc<dyn StatusbarEntryDom>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_entry_visibility: Rc<Emitter<StatusbarEntryVisibilityChange>> = Emitter::new();
        base.register(on_did_change_entry_visibility.clone());

        let this = Rc::new(Self {
            base,
            storage_service,
            dom,
            on_did_change_entry_visibility,
            entries: RefCell::new(Vec::new()),
            last_focused_entry: RefCell::new(None),
            hidden: RefCell::new(Vec::new()),
        });

        this.restore_state();
        this.register_listeners();

        this
    }

    /// `onDidChangeEntryVisibility`.
    pub fn on_did_change_entry_visibility(&self) -> Event<StatusbarEntryVisibilityChange> {
        self.on_did_change_entry_visibility.event()
    }

    /// `get entries()`: uma cópia da lista (`slice(0)`).
    pub fn entries(&self) -> Vec<EntryRef> {
        self.entries.borrow().clone()
    }

    /// `get lastFocusedEntry()`.
    pub fn last_focused_entry(&self) -> Option<EntryRef> {
        self.last_focused_entry.borrow().clone().filter(|entry| !self.is_hidden(&entry.id))
    }

    fn restore_state(&self) {
        let hidden_raw = self.storage_service.get(HIDDEN_ENTRIES_KEY, StorageScope::Profile, None).filter(|raw| !raw.is_empty());
        if let Some(hidden_raw) = hidden_raw {
            // ignore parsing errors
            let mut errors = Vec::new();
            let parsed = parse_str(&hidden_raw, &mut errors).filter(|_| errors.is_empty());
            if let Some(hidden) = parsed.and_then(hidden_from_json) {
                *self.hidden.borrow_mut() = hidden;
            }
        }
    }

    fn register_listeners(self: &Rc<Self>) {
        let store = DisposableStore::new();
        self.base.register(store.clone());

        let weak = Rc::downgrade(self);
        let listener = self.storage_service.on_did_change_value(StorageScope::Profile, Some(HIDDEN_ENTRIES_KEY), &store).listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_storage_value_change();
            }
        });
        self.base.register(listener);
    }

    fn on_did_storage_value_change(&self) {
        // Keep current hidden entries
        let currently_hidden = self.hidden.borrow().clone();

        // Load latest state of hidden entries
        self.hidden.borrow_mut().clear();
        self.restore_state();

        let hidden = self.hidden.borrow().clone();
        let mut changed: Vec<String> = Vec::new();

        // Check for each entry that is now visible
        for id in &currently_hidden {
            if !hidden.contains(id) && !changed.contains(id) {
                changed.push(id.clone());
            }
        }

        // Check for each entry that is now hidden
        for id in &hidden {
            if !currently_hidden.contains(id) && !changed.contains(id) {
                changed.push(id.clone());
            }
        }

        // Update visibility for entries have changed
        if !changed.is_empty() {
            for entry in self.entries() {
                if changed.contains(&entry.id) {
                    self.update_visibility_by_id(&entry.id, true);

                    changed.retain(|id| *id != entry.id);
                }
            }
        }
    }

    pub fn add(&self, entry: EntryRef) {
        // Add to set of entries
        self.entries.borrow_mut().push(entry.clone());

        // Update visibility directly
        self.update_visibility_of_entry(&entry, false);

        // Sort according to priority
        self.sort();

        // Mark first/last visible entry
        self.mark_first_last_visible_entry();
    }

    pub fn remove(&self, entry: &EntryRef) {
        let index = self.entries.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, entry));
        if let Some(index) = index {
            // Remove from entries
            self.entries.borrow_mut().remove(index);

            // Re-sort entries if this one was used
            // as reference from other entries
            let used_as_reference = self
                .entries
                .borrow()
                .iter()
                .any(|other_entry| other_entry.priority.primary.location().is_some_and(|location| location.location.id == entry.id));
            if used_as_reference {
                self.sort();
            }

            // Mark first/last visible entry
            self.mark_first_last_visible_entry();
        }
    }

    pub fn is_hidden(&self, id: &str) -> bool {
        self.hidden.borrow().iter().any(|hidden| hidden == id)
    }

    pub fn hide(&self, id: &str) {
        if !self.is_hidden(id) {
            self.hidden.borrow_mut().push(id.to_owned());

            self.update_visibility_by_id(id, true);

            self.save_state();
        }
    }

    pub fn show(&self, id: &str) {
        let position = self.hidden.borrow().iter().position(|hidden| hidden == id);
        if let Some(position) = position {
            self.hidden.borrow_mut().remove(position);

            self.update_visibility_by_id(id, true);

            self.save_state();
        }
    }

    pub fn find_entry(&self, container: StatusbarElementId) -> Option<EntryRef> {
        self.entries.borrow().iter().find(|entry| entry.container == container).cloned()
    }

    pub fn get_entries(&self, alignment: StatusbarAlignment) -> Vec<EntryRef> {
        self.entries.borrow().iter().filter(|entry| entry.alignment == alignment).cloned().collect()
    }

    pub fn focus_next_entry(&self) {
        self.focus_entry(1, 0);
    }

    pub fn focus_previous_entry(&self) {
        self.focus_entry(-1, self.entries.borrow().len() as isize - 1);
    }

    pub fn is_entry_focused(&self) -> bool {
        self.get_focused_entry().is_some()
    }

    fn get_focused_entry(&self) -> Option<EntryRef> {
        self.entries().into_iter().find(|entry| self.dom.is_ancestor_of_active_element(entry.container))
    }

    fn focus_entry(&self, delta: isize, restart_position: isize) {
        let entries = self.entries();

        let get_visible_entry = |start: isize| {
            let mut index_to_focus = start;
            let mut entry = entry_at(&entries, index_to_focus);
            while let Some(candidate) = entry
                && self.is_hidden(&candidate.id)
            {
                index_to_focus += delta;
                entry = entry_at(&entries, index_to_focus);
            }

            entry.cloned()
        };

        let focused = self.get_focused_entry();
        if let Some(focused) = focused {
            let position = entries.iter().position(|entry| Rc::ptr_eq(entry, &focused)).map_or(-1, |position| position as isize);
            if let Some(entry) = get_visible_entry(position + delta) {
                *self.last_focused_entry.borrow_mut() = Some(entry.clone());

                self.dom.focus(entry.label_container);

                return;
            }
        }

        if let Some(entry) = get_visible_entry(restart_position) {
            *self.last_focused_entry.borrow_mut() = Some(entry.clone());
            self.dom.focus(entry.label_container);
        }
    }

    /// `updateVisibility(id, trigger)`: por identificador, todas as entradas com esse `id`.
    fn update_visibility_by_id(&self, id: &str, trigger: bool) {
        for entry in self.entries() {
            if entry.id == id {
                self.update_visibility_of_entry(&entry, trigger);
            }
        }
    }

    /// `updateVisibility(entry, trigger)`: por entrada.
    fn update_visibility_of_entry(&self, entry: &EntryRef, trigger: bool) {
        let is_hidden = self.is_hidden(&entry.id);

        // Use CSS to show/hide item container
        self.dom.set_visible(entry.container, !is_hidden);

        if trigger {
            self.on_did_change_entry_visibility.fire(StatusbarEntryVisibilityChange { id: entry.id.clone(), visible: !is_hidden });
        }

        // Mark first/last visible entry
        self.mark_first_last_visible_entry();
    }

    fn save_state(&self) {
        let hidden = self.hidden.borrow().clone();
        if !hidden.is_empty() {
            let raw = json_stringify(&Value::Array(hidden.iter().map(|id| Value::String(JsString::from(id.as_str()))).collect()), None);
            self.storage_service.store(HIDDEN_ENTRIES_KEY, StorageValue::String(raw), StorageScope::Profile, StorageTarget::User);
        } else {
            self.storage_service.remove(HIDDEN_ENTRIES_KEY, StorageScope::Profile);
        }
    }

    fn sort(&self) {
        let current_entries = self.entries();
        let all_entry_ids: HashSet<&str> = current_entries.iter().map(|entry| entry.id.as_str()).collect();

        // Split up entries into 2 buckets:
        // - those with priority as number that can be compared or with a missing relative entry
        // - those with a relative priority that must be sorted relative to another entry that exists
        //
        // O primeiro balde é o `Map<entry, index>` do original: a mesma entrada (mesmo `Rc`) adicionada
        // mais de uma vez fica na posição da primeira inserção, com o índice da última.
        let mut entries_with_numbered_priority: Vec<(usize, EntryRef)> = Vec::new();
        let mut entries_with_relative_priority: OrderedMap<String /* id of entry to position after */, OrderedMap<String, EntryRef>> = OrderedMap::new();
        for (index, entry) in current_entries.iter().enumerate() {
            let relative_location = entry.priority.primary.location().filter(|location| all_entry_ids.contains(location.location.id.as_str()));
            let Some(relative_location) = relative_location else {
                match entries_with_numbered_priority.iter_mut().find(|(_, candidate)| Rc::ptr_eq(candidate, entry)) {
                    Some(existing) => existing.0 = index,
                    None => entries_with_numbered_priority.push((index, entry.clone())),
                }
                continue;
            };

            let reference_entry_id = &relative_location.location.id;

            // It is possible that this entry references another entry
            // that itself references an entry. In that case, we want
            // to add it to the entries of the referenced entry.
            let existing_owner_key = if entries_with_relative_priority.contains_key(reference_entry_id) {
                Some(reference_entry_id.clone())
            } else {
                entries_with_relative_priority
                    .iter()
                    .find(|(_, relative_entries)| relative_entries.contains_key(reference_entry_id))
                    .map(|(owner_key, _)| owner_key.clone())
            };
            let owner_key = existing_owner_key.unwrap_or_else(|| {
                entries_with_relative_priority.insert(reference_entry_id.clone(), OrderedMap::new());
                reference_entry_id.clone()
            });
            if let Some(entries) = entries_with_relative_priority.get_mut(&owner_key) {
                entries.insert(entry.id.clone(), entry.clone());
            }
        }

        // Sort the entries with `priority: number` or referencing a missing entry accordingly
        sort_by_compare(&mut entries_with_numbered_priority, |(index_a, entry_a), (index_b, entry_b)| {
            if entry_a.alignment == entry_b.alignment {
                // Sort by primary/secondary priority: higher values move towards the left

                let entry_a_primary_priority = primary_priority_number(entry_a);
                let entry_b_primary_priority = primary_priority_number(entry_b);

                if entry_a_primary_priority != entry_b_primary_priority {
                    return entry_b_primary_priority - entry_a_primary_priority;
                }

                if entry_a.priority.secondary != entry_b.priority.secondary {
                    return entry_b.priority.secondary - entry_a.priority.secondary;
                }

                // otherwise maintain stable order (both values known to be in map)
                return *index_a as f64 - *index_b as f64;
            }

            if entry_a.alignment == StatusbarAlignment::Left {
                return -1.0;
            }

            if entry_b.alignment == StatusbarAlignment::Left {
                return 1.0;
            }

            0.0
        });
        let sorted_entries_with_numbered_priority: Vec<EntryRef> = entries_with_numbered_priority.into_iter().map(|(_, entry)| entry).collect();

        // Entries with location: sort in accordingly
        let sorted_entries = if !entries_with_relative_priority.is_empty() {
            let mut sorted_entries: Vec<EntryRef> = Vec::new();

            for entry in sorted_entries_with_numbered_priority {
                let relative_entries: Option<Vec<EntryRef>> =
                    entries_with_relative_priority.get(&entry.id).map(|relative_entries| relative_entries.iter().map(|(_, relative_entry)| relative_entry.clone()).collect());

                // Fill relative entries to LEFT
                if let Some(relative_entries) = &relative_entries {
                    sorted_entries.extend(relative_entries_to(relative_entries, StatusbarAlignment::Left));
                }

                // Fill referenced entry
                let entry_id = entry.id.clone();
                sorted_entries.push(entry);

                // Fill relative entries to RIGHT
                if let Some(relative_entries) = &relative_entries {
                    sorted_entries.extend(relative_entries_to(relative_entries, StatusbarAlignment::Right));
                }

                // Delete from map to mark as handled
                entries_with_relative_priority.remove(&entry_id);
            }

            // Finally, just append all entries that reference another entry
            // that does not exist to the end of the list
            //
            // Note: this should really not happen because of our check in
            // `allEntryIds`, but we play it safe here to really consume
            // all entries.
            //
            for (_, entries) in entries_with_relative_priority.iter() {
                let mut remaining: Vec<EntryRef> = entries.iter().map(|(_, entry)| entry.clone()).collect();
                sort_by_secondary_priority(&mut remaining);
                sorted_entries.extend(remaining);
            }

            sorted_entries
        }
        // No entries with relative priority: take sorted entries as is
        else {
            sorted_entries_with_numbered_priority
        };

        // Take over as new truth of entries
        *self.entries.borrow_mut() = sorted_entries;
    }

    fn mark_first_last_visible_entry(&self) {
        self.do_mark_first_last_visible_statusbar_item(&self.get_entries(StatusbarAlignment::Left));
        self.do_mark_first_last_visible_statusbar_item(&self.get_entries(StatusbarAlignment::Right));
    }

    fn do_mark_first_last_visible_statusbar_item(&self, entries: &[EntryRef]) {
        let mut first_visible_item: Option<&EntryRef> = None;
        let mut last_visible_item: Option<&EntryRef> = None;

        for entry in entries {
            // Clear previous first
            self.dom.remove_visible_edge_marks(entry.container);

            let is_visible = !self.is_hidden(&entry.id);
            if is_visible {
                if first_visible_item.is_none() {
                    first_visible_item = Some(entry);
                }

                last_visible_item = Some(entry);
            }
        }

        // Mark: first visible item
        if let Some(entry) = first_visible_item {
            self.dom.add_visible_edge_mark(entry.container, VisibleEdgeMark::First);
        }

        // Mark: last visible item
        if let Some(entry) = last_visible_item {
            self.dom.add_visible_edge_mark(entry.container, VisibleEdgeMark::Last);
        }
    }
}

impl IDisposable for StatusbarViewModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}
