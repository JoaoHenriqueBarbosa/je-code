//! Porte de `src/vs/editor/browser/gpu/objectCollectionBuffer.ts`: uma coleção de objetos de propriedades
//! numéricas guardada num buffer plano de `f32`, com rastreio das regiões sujas.
//!
//! Divergências:
//! - `Float32Array` vira `Vec<f32>` (`view`), e `ArrayBufferLike` (`buffer`) não existe: o único uso do
//!   buffer é o tamanho em bytes, exposto por [`ObjectCollectionBuffer::buffer_byte_length`]
//!   (`buffer.byteLength`, que é `view.len() * 4`). O `subarray(0, viewUsedSize)` do teste é o fatiamento
//!   `view[..view_used_size]`.
//! - A spec dinâmica por nome (`T extends ObjectCollectionBufferPropertySpec[]`, com o tipo dos valores
//!   derivado dos nomes) vira [`ObjectCollectionBufferPropertySpec`] com `name: String`, e os valores de
//!   um objeto `{ a: 1, b: 2 }` viram `&[("a", 1.0), ("b", 2.0)]`. Propriedade ausente nos dados vira
//!   `NaN` (o `undefined` que o `Float32Array` converte), e nome extra é ignorado. Como no original, a
//!   escrita fora do tamanho do `view` é descartada, como num array tipado, e a leitura fora dele dá `NaN`.
//! - `IObjectCollectionBuffer` e `IObjectCollectionBufferEntry` têm uma única implementação cada, então não
//!   há trait: [`ObjectCollectionBuffer`] e [`ObjectCollectionBufferEntry`] são os tipos concretos.
//!   `createObjectCollectionBuffer` continua existindo como [`create_object_collection_buffer`].
//! - `dirtyTracker: IBufferDirtyTrackerReader` é um `Rc<dyn IBufferDirtyTrackerReader>`.
//! - No original o `view` entregue à entrada na criação fica velho quando o buffer cresce
//!   (`_expandBuffer` troca o `Float32Array`), e as entradas anteriores passam a escrever num array
//!   descartado. Aqui as entradas compartilham o `view` atual com o buffer, então esse defeito não é
//!   reproduzido (em Rust uma referência velha para um `Vec` realocado nem existe).
//! - `get`/`set` com nome desconhecido lançam `TypeError` (o `!` do original sobre `Map.get`), e `setRaw`
//!   devolve `Err` em vez de lançar, para os dois erros que o original lança (tamanho diferente e
//!   `RangeError` do `TypedArray.set` quando não cabe).
//! - O deslocamento à esquerda ao descartar uma entrada usa o `+ 2` literal do original (e não o tamanho
//!   da entrada), idêntico para entradas de duas propriedades, como o teste usa.

use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, dispose_array, dispose_iterator, to_disposable};
use je_base::linked_list::LinkedList;

use super::buffer_dirty_tracker::{BufferDirtyTracker, IBufferDirtyTrackerReader};

/// `ObjectCollectionBufferPropertySpec`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectCollectionBufferPropertySpec {
    pub name: String,
}

impl ObjectCollectionBufferPropertySpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// O estado que o buffer e as entradas compartilham: o `view`, o rastreador de regiões sujas e o mapa
/// nome da propriedade para deslocamento (na ordem de inserção do `Map`).
struct Shared {
    view: RefCell<Vec<f32>>,
    dirty_tracker: Rc<BufferDirtyTracker>,
    property_offsets: Vec<(String, usize)>,
    /// `_entrySize`: o número de especificações.
    entry_size: usize,
}

impl Shared {
    /// O `_propertySpecsMap.size` que as entradas usam como passo.
    fn stride(&self) -> usize {
        self.property_offsets.len()
    }

    fn offset_of(&self, property_name: &str) -> usize {
        match self.property_offsets.iter().find(|(name, _)| name == property_name) {
            Some((_, offset)) => *offset,
            None => errors::throw(Error::type_error("Cannot read properties of undefined (reading 'offset')")),
        }
    }

    /// `view[index] = value`: o array tipado descarta a escrita fora dos limites.
    fn write(&self, index: usize, value: f32) {
        if let Some(slot) = self.view.borrow_mut().get_mut(index) {
            *slot = value;
        }
    }

    /// `view[index]`: fora dos limites o JS devolve `undefined`.
    fn read(&self, index: usize) -> f32 {
        self.view.borrow().get(index).copied().unwrap_or(f32::NAN)
    }
}

/// Uma entrada de um [`ObjectCollectionBuffer`]. Os valores das propriedades podem mudar e são
/// atualizados no buffer automaticamente.
pub struct ObjectCollectionBufferEntry {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<()>>,
    on_will_dispose: Rc<Emitter<()>>,
    shared: Rc<Shared>,
    /// O índice da entrada no buffer, que diminui quando uma entrada anterior é descartada.
    i: Cell<usize>,
}

impl ObjectCollectionBufferEntry {
    fn new(shared: Rc<Shared>, i: usize, data: &[(&str, f32)]) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let on_will_dispose = Emitter::new();
        base.register(on_will_dispose.clone());
        let stride = shared.stride();
        for (name, offset) in &shared.property_offsets {
            let value = data.iter().find(|(key, _)| *key == name.as_str()).map_or(f32::NAN, |(_, value)| *value);
            shared.write(i * stride + offset, value);
        }
        shared.dirty_tracker.flag_range(i * stride, stride);
        Rc::new(Self { base, on_did_change, on_will_dispose, shared, i: Cell::new(i) })
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    pub fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    pub fn set(&self, property_name: &str, value: f32) {
        let index = self.i.get() * self.shared.stride() + self.shared.offset_of(property_name);
        self.shared.write(self.shared.dirty_tracker.flag(index), value);
        self.on_did_change.fire(());
    }

    pub fn get(&self, property_name: &str) -> f32 {
        self.shared.read(self.i.get() * self.shared.stride() + self.shared.offset_of(property_name))
    }

    pub fn set_raw(&self, data: &[f32]) -> Result<(), Error> {
        let stride = self.shared.stride();
        if data.len() != stride {
            return Err(Error::new(format!(
                "Data length {} does not match the number of properties in the collection ({stride})",
                data.len()
            )));
        }
        let offset = self.i.get() * stride;
        {
            let mut view = self.shared.view.borrow_mut();
            let Some(target) = view.get_mut(offset..offset + data.len()) else {
                return Err(Error::named("RangeError", "offset is out of bounds"));
            };
            target.copy_from_slice(data);
        }
        self.shared.dirty_tracker.flag_range(offset, stride);
        Ok(())
    }
}

impl IDisposable for ObjectCollectionBufferEntry {
    fn dispose(&self) {
        self.on_will_dispose.fire(());
        self.base.dispose();
    }
}

/// A coleção de entradas num buffer plano.
pub struct ObjectCollectionBuffer {
    base: Rc<Disposable>,
    shared: Rc<Shared>,
    entries: Rc<LinkedList<Rc<ObjectCollectionBufferEntry>>>,
    property_specs: Vec<ObjectCollectionBufferPropertySpec>,
    capacity: Cell<usize>,
    on_did_change: Rc<Emitter<()>>,
    on_did_change_buffer: Rc<Emitter<()>>,
}

/// `createObjectCollectionBuffer`.
pub fn create_object_collection_buffer(
    property_specs: Vec<ObjectCollectionBufferPropertySpec>,
    capacity: usize,
) -> Rc<ObjectCollectionBuffer> {
    ObjectCollectionBuffer::new(property_specs, capacity)
}

impl ObjectCollectionBuffer {
    fn new(property_specs: Vec<ObjectCollectionBufferPropertySpec>, capacity: usize) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let on_did_change_buffer = Emitter::new();
        base.register(on_did_change_buffer.clone());

        let mut property_offsets: Vec<(String, usize)> = Vec::new();
        for (offset, spec) in property_specs.iter().enumerate() {
            match property_offsets.iter_mut().find(|entry| entry.0 == spec.name) {
                Some(existing) => existing.1 = offset,
                None => property_offsets.push((spec.name.clone(), offset)),
            }
        }
        let shared = Rc::new(Shared {
            view: RefCell::new(vec![0.0; capacity * property_specs.len()]),
            dirty_tracker: Rc::new(BufferDirtyTracker::new()),
            property_offsets,
            entry_size: property_specs.len(),
        });
        let entries: Rc<LinkedList<Rc<ObjectCollectionBufferEntry>>> = Rc::new(LinkedList::new());
        let entries_to_dispose = entries.clone();
        base.register(to_disposable(move || {
            dispose_iterator(entries_to_dispose.iter().map(|entry| entry as DisposableRef));
        }));
        Rc::new(Self {
            base,
            shared,
            entries,
            property_specs,
            capacity: Cell::new(capacity),
            on_did_change,
            on_did_change_buffer,
        })
    }

    pub fn property_specs(&self) -> &[ObjectCollectionBufferPropertySpec] {
        &self.property_specs
    }

    pub fn capacity(&self) -> usize {
        self.capacity.get()
    }

    /// Uma visão do buffer. **Não** deve ser modificada externamente.
    pub fn view(&self) -> Ref<'_, Vec<f32>> {
        self.shared.view.borrow()
    }

    /// O tamanho do buffer subjacente em bytes (`buffer.byteLength`).
    pub fn buffer_byte_length(&self) -> usize {
        self.shared.view.borrow().len() * size_of::<f32>()
    }

    /// O tamanho da parte usada do buffer, em bytes.
    pub fn buffer_used_size(&self) -> usize {
        self.view_used_size() * size_of::<f32>()
    }

    /// O tamanho da parte usada do `view`, em `f32`.
    pub fn view_used_size(&self) -> usize {
        self.entries.size() * self.shared.entry_size
    }

    /// O número de entradas no buffer.
    pub fn entry_count(&self) -> usize {
        self.entries.size()
    }

    /// O rastreador das regiões sujas do buffer.
    pub fn dirty_tracker(&self) -> Rc<dyn IBufferDirtyTrackerReader> {
        self.shared.dirty_tracker.clone()
    }

    /// Dispara quando o buffer é modificado.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// Dispara quando o buffer é recriado.
    pub fn on_did_change_buffer(&self) -> Event<()> {
        self.on_did_change_buffer.event()
    }

    /// Cria uma entrada na coleção: um objeto gerenciado que atualiza o buffer subjacente ao ser
    /// modificado.
    pub fn create_entry(&self, data: &[(&str, f32)]) -> Rc<ObjectCollectionBufferEntry> {
        if self.entries.size() == self.capacity.get() {
            self.expand_buffer();
            self.on_did_change_buffer.fire(());
        }

        let value = ObjectCollectionBufferEntry::new(self.shared.clone(), self.entries.size(), data);
        let remove_from_entries = self.entries.push(value.clone());
        let listeners: Rc<RefCell<Vec<DisposableRef>>> = Rc::default();
        listeners.borrow_mut().push(value.on_did_change().forward(&self.on_did_change));

        let entry = Rc::downgrade(&value);
        let shared = self.shared.clone();
        let entries = self.entries.clone();
        let listeners_to_dispose = listeners.clone();
        let on_will_dispose = value.on_will_dispose().listen(move |_| {
            let Some(entry) = entry.upgrade() else { return };
            let deleted_entry_index = entry.i.get();
            remove_from_entries.remove();

            // Desloca para a esquerda todas as entradas depois da removida.
            shift_entries_left(&shared, deleted_entry_index, entries.size());

            // Atualiza as entradas para refletir o novo `i`.
            entries.for_each_ref(|entry| {
                if entry.i.get() > deleted_entry_index {
                    entry.i.set(entry.i.get() - 1);
                }
            });
            shared
                .dirty_tracker
                .flag_range(deleted_entry_index, entries.size().saturating_sub(deleted_entry_index) * shared.entry_size);
            let to_dispose = std::mem::take(&mut *listeners_to_dispose.borrow_mut());
            dispose_array(&to_dispose);
        });
        listeners.borrow_mut().push(on_will_dispose);
        value
    }

    fn expand_buffer(&self) {
        self.capacity.set(self.capacity.get() * 2);
        self.shared.view.borrow_mut().resize(self.capacity.get() * self.shared.entry_size, 0.0);
    }
}

impl IDisposable for ObjectCollectionBuffer {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `view.set(view.subarray(deleted * entrySize + 2, entryCount * entrySize + 2), deleted * entrySize)`:
/// o `subarray` limita os índices ao tamanho do `view`, e o `set` lança `RangeError` se não couber.
fn shift_entries_left(shared: &Shared, deleted_entry_index: usize, entry_count: usize) {
    let mut view = shared.view.borrow_mut();
    let len = view.len();
    let begin = (deleted_entry_index * shared.entry_size + 2).min(len);
    let end = (entry_count * shared.entry_size + 2).min(len).max(begin);
    let target = deleted_entry_index * shared.entry_size;
    if target + (end - begin) > len {
        drop(view);
        errors::throw(Error::named("RangeError", "offset is out of bounds"));
    }
    view.copy_within(begin..end, target);
}
