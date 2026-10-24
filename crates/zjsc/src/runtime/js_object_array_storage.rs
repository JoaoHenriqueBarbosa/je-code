//! A parte de `JSObject.cpp`, `JSObjectInlines.h` e `JSObject.h` que trata do `ArrayStorage` e do mapa
//! esparso (`SparseArrayValueMap`): `enterDictionaryIndexingMode`, `ensureArrayStorage*`, as conversões
//! de forma para `ArrayStorage`, `increaseVectorLength`, `putByIndexBeyondVectorLengthWithArrayStorage`,
//! `putDirectIndex*`, `defineOwnIndexedProperty`, `attemptToInterceptPutByIndexOnHole*`,
//! `notifyPresenceOfIndexedAccessors`, e `preventExtensions`, `seal` e `freeze`.
//!
//! DIVERGÊNCIAS (sem heap e sem GC, camada 3):
//!
//! - As quatro `convert{Undecided,Int32,Double,Contiguous}ToArrayStorage` são uma só
//!   (`convert_to_array_storage`), que escolhe a cópia pela forma atual: o corpo é o mesmo laço.
//!   `nukeStructureAndSetButterfly`, `DeferGC`, `DeferredStructureTransitionWatchpointFire` e
//!   `storeStoreFence` são do coletor e dos watchpoints e não existem.
//! - `getNewVectorLength` guarda `lastArraySize` num `thread_local` (no C++ é um `static` do arquivo).
//!   `increaseVectorLength` não distingue o ramo com pré-capacidade (`indexBias`) do sem: o vetor é um
//!   `Vec`, só cresce, e o `indexBias` nasce zero e só é zerado/decaído, nunca ganha pré-capacidade
//!   (`unshift` do `ArrayStorage` não existe no porte).
//! - Nenhum objeto do porte é `Proxy` nem `TypedArray`: o laço de `attemptToInterceptPutByIndexOnHole
//!   ForPrototype` não tem os ramos deles, e `ensureArrayStorageSlow` não tem o `hijacksIndexingHeader`.
//! - `notifyPresenceOfIndexedAccessors` de objeto que pode ser protótipo chama `JSGlobalObject::
//!   haveABadTime` (`js_global_object_bad_time.rs`). O de `JSGlobalObject` repassa ao `globalThis()`, o
//!   `JSGlobalProxy` (`js_global_proxy.rs`). Os demais passos da função são os do C++.
//! - `JSObject::freeze`/`seal` não invalidam a integridade da cadeia de estruturas (`StructureChain` não
//!   existe, como em `js_object.rs`).

use std::cell::Cell;

use crate::runtime::error_messages::{
    NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR, READONLY_PROPERTY_CHANGE_ERROR, READONLY_PROPERTY_WRITE_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_GETTER_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_SETTER_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR,
};
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PrivateSymbolMode, PropertyNameMode};
use crate::runtime::identifier::Identifier;
use crate::runtime::indexing_type::{
    has_any_array_storage, has_indexed_properties, has_slow_put_array_storage, is_copy_on_write, IndexingType, ARRAY_STORAGE_SHAPE,
    CONTIGUOUS_SHAPE, DOUBLE_SHAPE, INT32_SHAPE, NO_INDEXING_SHAPE, SLOW_PUT_ARRAY_STORAGE_SHAPE, UNDECIDED_SHAPE,
};
use crate::runtime::js_array::{ArrayError, JSArray};
use crate::runtime::js_array_storage::{ArrayStorage, FIRST_ARRAY_STORAGE_VECTOR_GROW};
use crate::runtime::js_getter_setter::{GetterSetter, GetterSetterRef};
use crate::runtime::js_object::{
    index_is_sufficiently_beyond_length_for_sparse_map, is_dense_enough_for_vector, type_error, IndexedStorage, JSObject,
    JSObjectHandle, PutError, MAX_STORAGE_VECTOR_LENGTH, MIN_SPARSE_ARRAY_INDEX,
};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::operations::same_value;
use crate::runtime::options::Options;
use crate::runtime::own_property_names::get_own_property_names;
use crate::runtime::property_attribute::{ACCESSOR, DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::proxy_object::put_by_index_from_proxy;
use crate::runtime::sparse_array_value_map::{PutDirectIndexMode, SparseArrayEntry, SparseArrayValueMap};
use crate::runtime::structure::Structure;
use crate::runtime::structure_transition_table::TransitionKind;
use crate::runtime::vm::VM;
use crate::wtf::math_extras::times_three_plus_one_divided_by_two;

thread_local! {
    /// `static unsigned lastArraySize` de `JSObject.cpp`.
    static LAST_ARRAY_SIZE: Cell<u32> = const { Cell::new(0) };
}

impl JSObject {
    // ---------------------------------------------------------------------------------------------
    // Acesso ao ArrayStorage.
    // ---------------------------------------------------------------------------------------------

    /// `butterfly()->arrayStorage()` para leitura: o objeto tem de estar em uma forma `ArrayStorage`.
    pub(crate) fn with_array_storage<R>(&self, f: impl FnOnce(&ArrayStorage) -> R) -> R {
        let butterfly = self.butterfly.borrow();
        match &butterfly.indexed {
            IndexedStorage::ArrayStorage(storage) => f(storage),
            _ => unreachable!("arrayStorage() em objeto sem ArrayStorage (RELEASE_ASSERT)"),
        }
    }

    /// `butterfly()->arrayStorage()` para escrita.
    pub(crate) fn with_array_storage_mut<R>(&self, f: impl FnOnce(&mut ArrayStorage) -> R) -> R {
        let mut butterfly = self.butterfly.borrow_mut();
        match &mut butterfly.indexed {
            IndexedStorage::ArrayStorage(storage) => f(storage),
            _ => unreachable!("arrayStorage() em objeto sem ArrayStorage (RELEASE_ASSERT)"),
        }
    }

    /// `m_sparseMap.get()` para escrita: o mapa tem de existir (`RELEASE_ASSERT(map)`).
    fn with_sparse_map_mut<R>(&self, f: impl FnOnce(&mut SparseArrayValueMap) -> R) -> R {
        self.with_array_storage_mut(|storage| f(storage.sparse_map_mut().expect("RELEASE_ASSERT(map)")))
    }

    /// `hasSparseMap()`.
    pub fn has_sparse_map(&self) -> bool {
        has_any_array_storage(self.cell().indexing_type()) && self.with_array_storage(|storage| storage.sparse_map().is_some())
    }

    /// O primeiro índice `>= index` que pode ter elemento próprio num objeto em `ArrayStorage` (o vetor, depois as
    /// chaves do mapa esparso); `None` quando nenhum índice a partir de `index` tem elemento ou o objeto não está
    /// nessa forma. Permite aos laços genéricos pular os buracos de um array de comprimento enorme sem observar
    /// diferença (só vale sem indexados na cadeia de protótipos, o que o chamador confere).
    pub fn next_own_indexed_candidate(&self, index: u32) -> Option<Option<u32>> {
        if self.shape() != ARRAY_STORAGE_SHAPE {
            return None;
        }
        Some(self.with_array_storage(|storage| {
            if index < storage.vector_length() {
                Some(index)
            } else {
                storage.sparse_map().and_then(|map| map.first_index_at_or_after(index))
            }
        }))
    }

    /// `inSparseIndexingMode()`.
    pub fn in_sparse_indexing_mode(&self) -> bool {
        match self.shape() {
            NO_INDEXING_SHAPE | UNDECIDED_SHAPE | INT32_SHAPE | DOUBLE_SHAPE | CONTIGUOUS_SHAPE => false,
            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => self.with_array_storage(|storage| storage.in_sparse_mode()),
            shape => unreachable!("inSparseIndexingMode em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// `suggestedArrayStorageTransition()`.
    fn suggested_array_storage_transition(&self) -> TransitionKind {
        if self.needs_slow_put_indexing() {
            return TransitionKind::AllocateSlowPutArrayStorage;
        }
        TransitionKind::AllocateArrayStorage
    }

    /// `allocateSparseIndexMap(vm)`.
    fn allocate_sparse_index_map(&self) {
        self.with_array_storage_mut(|storage| storage.set_sparse_map(Some(SparseArrayValueMap::default())));
    }

    /// `deallocateSparseIndexMap()`.
    pub(crate) fn deallocate_sparse_index_map(&self) {
        if has_any_array_storage(self.cell().indexing_type()) {
            self.with_array_storage_mut(|storage| storage.set_sparse_map(None));
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Criação e conversão para ArrayStorage.
    // ---------------------------------------------------------------------------------------------

    /// `createArrayStorage(vm, length, vectorLength)`: `false` é a falta de memória (`tryCreateArrayButterfly`
    /// devolvendo `nullptr`); o objeto fica como estava.
    pub(crate) fn create_array_storage(&self, vm: &VM, length: u32, vector_length: u32) -> bool {
        debug_assert!(!has_indexed_properties(self.cell().indexing_type()));
        let Some(storage) = ArrayStorage::try_new(length, vector_length) else {
            return false;
        };
        self.butterfly.borrow_mut().indexed = IndexedStorage::ArrayStorage(storage);
        self.transition_indexing(vm, self.suggested_array_storage_transition());
        true
    }

    /// `createInitialArrayStorage(vm)`: `false` é a falta de memória.
    fn create_initial_array_storage(&self, vm: &VM) -> bool {
        let property_capacity = self.structure().out_of_line_capacity() as usize;
        self.create_array_storage(vm, 0, ArrayStorage::optimal_vector_length(0, property_capacity, 0))
    }

    /// `convertUndecidedToArrayStorage`, `convertInt32ToArrayStorage`, `convertDoubleToArrayStorage` e
    /// `convertContiguousToArrayStorage(vm, transition)` (veja o cabeçalho do módulo). `false` é a falta de
    /// memória para o vetor novo (o objeto fica como estava).
    fn convert_to_array_storage(&self, vm: &VM, transition: TransitionKind) -> bool {
        let public_length = self.public_length();
        let shape = self.shape();
        let storage = {
            let butterfly = self.butterfly.borrow();
            // `constructConvertedArrayStorageWithoutCopyingElements`: sem esparso, viés 0, nenhum valor.
            let Some(mut storage) = ArrayStorage::try_new(public_length, butterfly.indexed.vector_length()) else {
                return false;
            };
            let mut count = 0;
            match (shape, &butterfly.indexed) {
                // Undecided só tem buracos: o laço do C++ os limpa e não conta nada.
                (UNDECIDED_SHAPE, _) => {}
                (INT32_SHAPE | CONTIGUOUS_SHAPE, IndexedStorage::Values(values)) => {
                    for (slot, value) in storage.vector_mut().iter_mut().zip(values.iter()) {
                        *slot = *value;
                        if !value.is_empty() {
                            count += 1;
                        }
                    }
                }
                (DOUBLE_SHAPE, IndexedStorage::Doubles(doubles)) => {
                    for (slot, value) in storage.vector_mut().iter_mut().zip(doubles.iter()) {
                        if value.is_nan() {
                            *slot = JSValue::empty();
                            continue;
                        }
                        *slot = JSValue::double_number(*value);
                        count += 1;
                    }
                }
                (shape, _) => unreachable!("convertToArrayStorage em forma {shape:#x} (ASSERT no C++)"),
            }
            storage.set_num_values_in_vector(count);
            storage
        };
        self.butterfly.borrow_mut().indexed = IndexedStorage::ArrayStorage(storage);
        self.transition_indexing(vm, transition);
        true
    }

    /// `ensureArrayStorageSlow(vm)`: `false` é o `nullptr` (objeto que não tem indexados tradicionais, que
    /// o porte não tem) ou a falta de memória.
    pub(crate) fn ensure_array_storage_slow(&self, vm: &VM) -> bool {
        self.ensure_writable(vm);

        match self.shape() {
            NO_INDEXING_SHAPE => {
                if self.indexing_should_be_sparse() {
                    return self.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(vm).is_ok();
                }
                self.create_initial_array_storage(vm)
            }
            UNDECIDED_SHAPE | INT32_SHAPE | DOUBLE_SHAPE | CONTIGUOUS_SHAPE => {
                debug_assert!(!self.indexing_should_be_sparse());
                debug_assert!(!self.needs_slow_put_indexing());
                self.convert_to_array_storage(vm, self.suggested_array_storage_transition())
            }
            shape => unreachable!("ensureArrayStorageSlow em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// `ensureArrayStorage(vm)`.
    pub fn ensure_array_storage(&self, vm: &VM) -> bool {
        if has_any_array_storage(self.cell().indexing_type()) {
            return true;
        }
        self.ensure_array_storage_slow(vm)
    }

    /// `ensureArrayStorageExistsAndEnterDictionaryIndexingMode(vm)`: `OutOfMemory` é a falta de memória.
    pub(crate) fn ensure_array_storage_exists_and_enter_dictionary_indexing_mode(&self, vm: &VM) -> Result<(), PutError> {
        self.ensure_writable(vm);

        match self.shape() {
            NO_INDEXING_SHAPE => {
                if !self.create_array_storage(vm, 0, 0) {
                    return Err(PutError::OutOfMemory);
                }
                self.allocate_sparse_index_map();
                self.with_sparse_map_mut(|map| map.set_sparse_mode());
            }
            UNDECIDED_SHAPE | INT32_SHAPE | DOUBLE_SHAPE | CONTIGUOUS_SHAPE => {
                if !self.convert_to_array_storage(vm, self.suggested_array_storage_transition()) {
                    return Err(PutError::OutOfMemory);
                }
                self.enter_dictionary_indexing_mode_when_array_storage_already_exists();
            }
            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                self.enter_dictionary_indexing_mode_when_array_storage_already_exists();
            }
            shape => unreachable!("ensureArrayStorageExistsAndEnterDictionaryIndexingMode em forma {shape:#x} (CRASH)"),
        }
        Ok(())
    }

    /// `enterDictionaryIndexingModeWhenArrayStorageAlreadyExists(vm, storage)`.
    fn enter_dictionary_indexing_mode_when_array_storage_already_exists(&self) {
        self.with_array_storage_mut(|storage| {
            if storage.sparse_map().is_none() {
                storage.set_sparse_map(Some(SparseArrayValueMap::default()));
            }
            if storage.in_sparse_mode() {
                return;
            }
            storage.sparse_map_mut().expect("mapa recém-criado").set_sparse_mode();

            let used_vector_length = storage.length().min(storage.vector_length());
            let values: Vec<(u32, JSValue)> = (0..used_vector_length)
                .filter_map(|i| {
                    let value = storage.vector()[i as usize];
                    (!value.is_empty()).then_some((i, value))
                })
                .collect();
            let map = storage.sparse_map_mut().expect("mapa recém-criado");
            for (i, value) in values {
                // This will always be a new entry in the map, so no need to check we can write,
                // and attributes are default so no need to set them.
                map.add(i);
                map.force_set(i, Some(value), 0);
            }

            storage.set_vector_length(0);
            storage.set_index_bias(0);
        });
    }

    /// `enterDictionaryIndexingMode(vm)`.
    pub fn enter_dictionary_indexing_mode(&self, vm: &VM) {
        match self.shape() {
            NO_INDEXING_SHAPE => {
                // No indexed properties to convert. Once the caller makes the structure non-extensible,
                // indexingShouldBeSparse() lazily handles later indexed writes. JSArray code paths
                // assume this method allocated ArrayStorage, so only non-arrays return early.
                if !matches!(self.type_(), JSType::ArrayType | JSType::DerivedArrayType) {
                    return;
                }
                if self.ensure_array_storage_slow(vm) {
                    self.enter_dictionary_indexing_mode_when_array_storage_already_exists();
                }
            }
            UNDECIDED_SHAPE | INT32_SHAPE | DOUBLE_SHAPE | CONTIGUOUS_SHAPE => {
                if self.ensure_array_storage_slow(vm) {
                    self.enter_dictionary_indexing_mode_when_array_storage_already_exists();
                }
            }
            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                self.enter_dictionary_indexing_mode_when_array_storage_already_exists();
            }
            shape => unreachable!("enterDictionaryIndexingMode em forma {shape:#x}"),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Crescimento do vetor.
    // ---------------------------------------------------------------------------------------------

    /// `getNewVectorLength(indexBias, currentVectorLength, currentLength, desiredLength)`.
    fn get_new_vector_length_for(
        &self,
        index_bias: u32,
        current_vector_length: u32,
        current_length: u32,
        desired_length: u32,
    ) -> u32 {
        debug_assert!(desired_length <= MAX_STORAGE_VECTOR_LENGTH);

        let max_init_length = current_length.min(100000);
        let increased_length = if desired_length < max_init_length {
            max_init_length
        } else if current_vector_length == 0 {
            desired_length.max(LAST_ARRAY_SIZE.with(Cell::get))
        } else {
            times_three_plus_one_divided_by_two(desired_length)
        };
        debug_assert!(increased_length >= desired_length);

        LAST_ARRAY_SIZE.with(|last| last.set(increased_length.min(FIRST_ARRAY_STORAGE_VECTOR_GROW)));

        ArrayStorage::optimal_vector_length(
            index_bias,
            self.structure().out_of_line_capacity() as usize,
            increased_length.min(MAX_STORAGE_VECTOR_LENGTH),
        )
    }

    /// `getNewVectorLength(desiredLength)`.
    pub(crate) fn get_new_vector_length(&self, desired_length: u32) -> u32 {
        let mut index_bias = 0;
        let mut vector_length = 0;
        let mut length = 0;

        if has_indexed_properties(self.cell().indexing_type()) {
            if has_any_array_storage(self.cell().indexing_type()) {
                index_bias = self.with_array_storage(|storage| storage.index_bias());
            }
            vector_length = self.vector_length();
            length = self.public_length();
        }

        self.get_new_vector_length_for(index_bias, vector_length, length, desired_length)
    }

    /// `increaseVectorLength(vm, newLength)`: `false` é a falha de alocação ou a decisão de não usar vetor.
    fn increase_vector_length(&self, new_length: u32) -> bool {
        let (vector_length, num_values, index_bias) = self
            .with_array_storage(|storage| (storage.vector_length(), storage.num_values_in_vector(), storage.index_bias()));
        let property_capacity = self.structure().out_of_line_capacity() as usize;

        let available_vector_length = ArrayStorage::available_vector_length(index_bias, property_capacity, vector_length);
        if available_vector_length >= new_length {
            self.with_array_storage_mut(|storage| storage.set_vector_length(available_vector_length));
            return true;
        }

        // This function leaves the array in an internally inconsistent state, because it does not move any
        // values from sparse value map to the vector. Callers have to account for that.
        if new_length > MAX_STORAGE_VECTOR_LENGTH {
            return false;
        }

        if new_length >= MIN_SPARSE_ARRAY_INDEX && !is_dense_enough_for_vector(new_length, num_values) {
            return false;
        }

        debug_assert!(new_length > vector_length);
        let new_vector_length = self.get_new_vector_length(new_length);

        // Fast case - there is no precapacity. Otherwise remove some, but not all of the precapacity,
        // capped to not overflow array length.
        let new_index_bias = if index_bias == 0 {
            0
        } else {
            (index_bias >> 1).min(MAX_STORAGE_VECTOR_LENGTH - new_vector_length)
        };
        self.with_array_storage_mut(|storage| {
            storage.set_vector_length(new_vector_length);
            storage.set_index_bias(new_index_bias);
        });
        true
    }

    // ---------------------------------------------------------------------------------------------
    // putByIndex em ArrayStorage.
    // ---------------------------------------------------------------------------------------------

    /// O ramo `NonArrayWithArrayStorage`/`ArrayWithArrayStorage` e `...WithSlowPutArrayStorage` de
    /// `putByIndex`: `None` quando o índice está além do vetor (o chamador cai no
    /// `putByIndexBeyondVectorLength`).
    pub(crate) fn put_by_index_in_array_storage_vector(
        &self,
        vm: &VM,
        shape: IndexingType,
        index: u32,
        value: JSValue,
        should_throw: bool,
    ) -> Result<Option<bool>, PutError> {
        let (in_vector, length, is_hole) = self.with_array_storage(|storage| {
            let in_vector = index < storage.vector_length();
            (in_vector, storage.length(), in_vector && storage.vector()[index as usize].is_empty())
        });
        if !in_vector {
            return Ok(None);
        }

        // Update length & m_numValuesInVector as necessary.
        if shape == SLOW_PUT_ARRAY_STORAGE_SHAPE && (index >= length || is_hole) {
            if let Some(put_result) = self.attempt_to_intercept_put_by_index_on_hole(vm, index, value, should_throw)? {
                return Ok(Some(put_result));
            }
        }
        self.with_array_storage_mut(|storage| {
            if index >= length {
                storage.set_length(index + 1);
            }
            if index >= length || is_hole {
                let count = storage.num_values_in_vector();
                storage.set_num_values_in_vector(count + 1);
            }
            storage.vector_mut()[index as usize] = value;
        });
        Ok(Some(true))
    }

    /// `SparseArrayEntry::put(globalObject, thisValue, map, value, shouldThrow)` sobre a entrada `entry` do
    /// mapa deste objeto.
    fn sparse_entry_put(
        &self,
        this_value: JSValue,
        entry: SparseArrayEntry,
        value: JSValue,
        should_throw: bool,
    ) -> Result<bool, PutError> {
        if entry.attributes() & ACCESSOR == 0 {
            if entry.attributes() & READ_ONLY != 0 {
                return type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR);
            }

            self.with_sparse_map_mut(|map| map.set_value(entry.index(), value));
            return Ok(true);
        }

        let getter_setter = GetterSetter::from_value(&entry.value()).expect("entrada de accessor sem GetterSetter");
        getter_setter.call_setter(this_value, value, should_throw)
    }

    /// `SparseArrayValueMap::putEntry(globalObject, array, i, value, shouldThrow)`.
    fn sparse_map_put_entry(&self, i: u32, value: JSValue, should_throw: bool) -> Result<bool, PutError> {
        let (is_new_entry, entry) = self.with_sparse_map_mut(|map| {
            let is_new_entry = map.add(i);
            (is_new_entry, map.find(i).expect("entrada recém-adicionada"))
        });

        // To save a separate find & add, we first always add to the sparse map. In the uncommon case that this
        // is a new property, and the array is not extensible, this is not the right thing to have done - so
        // remove again.
        if is_new_entry && !self.is_structure_extensible() {
            self.with_sparse_map_mut(|map| map.remove(i));
            return type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR);
        }

        self.sparse_entry_put(self.as_value(), entry, value, should_throw)
    }

    /// `SparseArrayValueMap::putDirect(globalObject, array, i, value, attributes, mode)`.
    fn sparse_map_put_direct(
        &self,
        i: u32,
        value: JSValue,
        attributes: u32,
        mode: PutDirectIndexMode,
    ) -> Result<bool, PutError> {
        let should_throw = mode == PutDirectIndexMode::PutDirectIndexShouldThrow;

        let (is_new_entry, entry) = self.with_sparse_map_mut(|map| {
            let is_new_entry = map.add(i);
            (is_new_entry, map.find(i).expect("entrada recém-adicionada"))
        });

        // To save a separate find & add, we first always add to the sparse map. In the uncommon case that this
        // is a new property, and the array is not extensible, this is not the right thing to have done - so
        // remove again.
        if mode != PutDirectIndexMode::PutDirectIndexLikePutDirect && is_new_entry && !self.is_structure_extensible() {
            self.with_sparse_map_mut(|map| map.remove(i));
            return type_error(should_throw, NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR);
        }

        if entry.attributes() & READ_ONLY != 0 {
            return type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR);
        }

        self.with_sparse_map_mut(|map| map.force_set(i, Some(value), attributes));
        Ok(true)
    }

    /// Move os valores do mapa (fora do modo esparso e sem atributos) para o vetor, apaga o mapa e guarda
    /// `value` em `i` (o fim comum de `putByIndexBeyondVectorLengthWithArrayStorage` e
    /// `putDirectIndexBeyondVectorLengthWithArrayStorage`).
    fn move_sparse_map_into_vector_and_store(&self, i: u32, value: JSValue, num_values_in_array: u32) {
        self.with_array_storage_mut(|storage| {
            // Reread m_storage after increaseVectorLength, update m_numValuesInVector.
            storage.set_num_values_in_vector(num_values_in_array);

            // Copy all values from the map into the vector, and delete the map.
            let map = storage.take_sparse_map().expect("mapa esparso");
            for entry in map.iter() {
                storage.vector_mut()[entry.index() as usize] = entry.get_non_sparse_mode();
            }

            // Store the new property into the vector.
            if storage.vector()[i as usize].is_empty() {
                let count = storage.num_values_in_vector();
                storage.set_num_values_in_vector(count + 1);
            }
            storage.vector_mut()[i as usize] = value;
        });
    }

    /// `putByIndexBeyondVectorLengthWithArrayStorage(globalObject, i, value, shouldThrow, storage)`.
    pub(crate) fn put_by_index_beyond_vector_length_with_array_storage(
        &self,
        vm: &VM,
        i: u32,
        value: JSValue,
        should_throw: bool,
    ) -> Result<bool, PutError> {
        let _ = vm;
        debug_assert!(!is_copy_on_write(self.cell().indexing_mode()));
        // i should be a valid array index that is outside of the current vector.
        debug_assert!(i <= crate::runtime::identifier::MAX_ARRAY_INDEX);
        debug_assert!(i >= self.vector_length());

        // First, handle cases where we don't currently have a sparse map.
        if !self.has_sparse_map() {
            // If the array is not extensible, we should have entered dictionary mode, and created the sparse map.
            debug_assert!(self.is_structure_extensible());

            // Update m_length if necessary.
            let (vector_length, num_values) = self.with_array_storage_mut(|storage| {
                if i >= storage.length() {
                    storage.set_length(i + 1);
                }
                (storage.vector_length(), storage.num_values_in_vector())
            });

            // Check that it is sensible to still be using a vector, and then try to grow the vector.
            if !index_is_sufficiently_beyond_length_for_sparse_map(i, vector_length)
                && is_dense_enough_for_vector(i, num_values)
                && self.increase_vector_length(i + 1)
            {
                // success! - reread m_storage since it has likely been reallocated, and store to the vector.
                self.with_array_storage_mut(|storage| {
                    storage.vector_mut()[i as usize] = value;
                    let count = storage.num_values_in_vector();
                    storage.set_num_values_in_vector(count + 1);
                });
                return Ok(true);
            }
            // We don't want to, or can't use a vector to hold this property - allocate a sparse map & add the value.
            self.allocate_sparse_index_map();
            return self.sparse_map_put_entry(i, value, should_throw);
        }

        // Update m_length if necessary.
        let (mut length, num_values, map_size, length_is_read_only, sparse_mode) = self.with_array_storage(|storage| {
            let map = storage.sparse_map().expect("mapa esparso");
            (
                storage.length(),
                storage.num_values_in_vector(),
                map.size() as u32,
                map.length_is_read_only(),
                map.sparse_mode(),
            )
        });
        if i >= length {
            // Prohibit growing the array if length is not writable.
            if length_is_read_only || !self.is_structure_extensible() {
                return type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR);
            }
            length = i + 1;
            self.with_array_storage_mut(|storage| storage.set_length(length));
        }

        // We are currently using a map - check whether we still want to be doing so. We will continue to use a
        // sparse map if SparseMode is set, a vector would be too sparse, or if allocation fails.
        let num_values_in_array = num_values + map_size;
        if sparse_mode || !is_dense_enough_for_vector(length, num_values_in_array) || !self.increase_vector_length(length) {
            return self.sparse_map_put_entry(i, value, should_throw);
        }

        self.move_sparse_map_into_vector_and_store(i, value, num_values_in_array);
        Ok(true)
    }

    /// `putDirectIndexBeyondVectorLengthWithArrayStorage(globalObject, i, value, attributes, mode, storage)`.
    fn put_direct_index_beyond_vector_length_with_array_storage(
        &self,
        i: u32,
        value: JSValue,
        attributes: u32,
        mode: PutDirectIndexMode,
    ) -> Result<bool, PutError> {
        // i should be a valid array index that is outside of the current vector.
        debug_assert!(has_any_array_storage(self.cell().indexing_type()));
        debug_assert!(i >= self.vector_length() || attributes != 0);
        debug_assert!(i <= crate::runtime::identifier::MAX_ARRAY_INDEX);

        // First, handle cases where we don't currently have a sparse map.
        if !self.has_sparse_map() {
            // If the array is not extensible, we should have entered dictionary mode, and created the spare map.
            debug_assert!(self.is_structure_extensible());

            // Update m_length if necessary.
            let (vector_length, num_values) = self.with_array_storage_mut(|storage| {
                if i >= storage.length() {
                    storage.set_length(i + 1);
                }
                (storage.vector_length(), storage.num_values_in_vector())
            });

            // Check that it is sensible to still be using a vector, and then try to grow the vector.
            if attributes == 0
                && is_dense_enough_for_vector(i, num_values)
                && !index_is_sufficiently_beyond_length_for_sparse_map(i, vector_length)
                && self.increase_vector_length(i + 1)
            {
                // success! - reread m_storage since it has likely been reallocated, and store to the vector.
                self.with_array_storage_mut(|storage| {
                    storage.vector_mut()[i as usize] = value;
                    let count = storage.num_values_in_vector();
                    storage.set_num_values_in_vector(count + 1);
                });
                return Ok(true);
            }
            // We don't want to, or can't use a vector to hold this property - allocate a sparse map & add the value.
            self.allocate_sparse_index_map();
            return self.sparse_map_put_direct(i, value, attributes, mode);
        }

        // Update m_length if necessary.
        let (mut length, num_values, map_size, length_is_read_only, sparse_mode) = self.with_array_storage(|storage| {
            let map = storage.sparse_map().expect("mapa esparso");
            (
                storage.length(),
                storage.num_values_in_vector(),
                map.size() as u32,
                map.length_is_read_only(),
                map.sparse_mode(),
            )
        });
        if i >= length {
            if mode != PutDirectIndexMode::PutDirectIndexLikePutDirect {
                // Prohibit growing the array if length is not writable.
                let should_throw = mode == PutDirectIndexMode::PutDirectIndexShouldThrow;
                if length_is_read_only {
                    return type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR);
                }
                if !self.is_structure_extensible() {
                    return type_error(should_throw, NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR);
                }
            }
            length = i + 1;
            self.with_array_storage_mut(|storage| storage.set_length(length));
        }

        // We are currently using a map - check whether we still want to be doing so. We will continue to use a
        // sparse map if SparseMode is set, a vector would be too sparse, or if allocation fails.
        let num_values_in_array = num_values + map_size;
        if sparse_mode
            || attributes != 0
            || !is_dense_enough_for_vector(length, num_values_in_array)
            || !self.increase_vector_length(length)
        {
            return self.sparse_map_put_direct(i, value, attributes, mode);
        }

        self.move_sparse_map_into_vector_and_store(i, value, num_values_in_array);
        Ok(true)
    }

    // ---------------------------------------------------------------------------------------------
    // putDirectIndex.
    // ---------------------------------------------------------------------------------------------

    /// `putDirectIndex(globalObject, propertyName, value, attributes, mode)` (JSObjectInlines.h).
    pub fn put_direct_index(
        &self,
        vm: &VM,
        property_name: u32,
        value: JSValue,
        attributes: u32,
        mode: PutDirectIndexMode,
    ) -> Result<bool, PutError> {
        let can_set_index_quickly_for_put_direct = match self.shape() {
            NO_INDEXING_SHAPE | UNDECIDED_SHAPE => false,
            INT32_SHAPE | DOUBLE_SHAPE | CONTIGUOUS_SHAPE | ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                !is_copy_on_write(self.cell().indexing_mode()) && property_name < self.vector_length()
            }
            shape => unreachable!("putDirectIndex em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        };

        if attributes == 0 && can_set_index_quickly_for_put_direct {
            self.set_index_quickly(vm, property_name, value);
            return Ok(true);
        }
        self.put_direct_index_slow_or_beyond_vector_length(vm, property_name, value, attributes, mode)
    }

    /// `canDoFastPutDirectIndex(object)`.
    fn can_do_fast_put_direct_index(&self) -> bool {
        if TypeInfo::is_arguments_type(self.type_()) {
            return true;
        }

        if self.in_sparse_indexing_mode() {
            return false;
        }

        (self.type_() == JSType::ArrayType && !is_copy_on_write(self.cell().indexing_mode()))
            || self.type_() == JSType::FinalObjectType
    }

    /// `methodTable()->defineOwnProperty(this, globalObject, Identifier::from(vm, i), descriptor, throwException)`
    /// para um nome de índice: o de `JSArray` confere o `length` gravável antes do ordinário.
    fn define_own_index_property_by_class(
        &self,
        vm: &VM,
        i: u32,
        descriptor: &PropertyDescriptor,
        throw_exception: bool,
    ) -> Result<bool, PutError> {
        // `ProxyObject::defineOwnProperty`: o `Proxy` responde pelo trap `defineProperty`.
        if self.type_() == JSType::ProxyObjectType {
            let name = PropertyName::from_identifier(&Identifier::from_u32(vm, i));
            return crate::runtime::proxy_object::define_own_property_from_proxy(self, &name, descriptor, throw_exception);
        }
        // `JSGenericTypedArrayView::defineOwnProperty`: fora dos limites lança, dentro grava o valor.
        {
            let name = PropertyName::from_identifier(&Identifier::from_u32(vm, i));
            if let Some(result) =
                crate::runtime::typed_array_dispatch::define_own_property(self, &name, descriptor, throw_exception)
            {
                return result;
            }
        }
        if matches!(self.type_(), JSType::ArrayType | JSType::DerivedArrayType) {
            if let Some(array) = JSArray::from_cell_id_by_class(self.cell_id()) {
                let name = PropertyName::from_identifier(&Identifier::from_u32(vm, i));
                return match array.define_own_property(vm, &name, descriptor, throw_exception) {
                    Ok(result) => Ok(result),
                    Err(ArrayError::Put(error)) => Err(error),
                    // Invariante: o `RangeError` só nasce do nome `length`; um índice u32 nunca o dispara.
                    Err(ArrayError::RangeError(_)) => unreachable!("RangeError ao definir índice de Array"),
                };
            }
        }
        self.define_own_indexed_property(vm, i, descriptor, throw_exception)
    }

    /// `putDirectIndexSlowOrBeyondVectorLength(globalObject, i, value, attributes, mode)`.
    fn put_direct_index_slow_or_beyond_vector_length(
        &self,
        vm: &VM,
        i: u32,
        value: JSValue,
        attributes: u32,
        mode: PutDirectIndexMode,
    ) -> Result<bool, PutError> {
        if !self.can_do_fast_put_direct_index() {
            let mut descriptor = PropertyDescriptor::default();
            descriptor.set_descriptor(value, attributes);
            return self.define_own_index_property_by_class(
                vm,
                i,
                &descriptor,
                mode == PutDirectIndexMode::PutDirectIndexShouldThrow,
            );
        }

        // i should be a valid array index that is outside of the current vector.
        debug_assert!(i <= crate::runtime::identifier::MAX_ARRAY_INDEX);

        if attributes & (READ_ONLY | ACCESSOR) != 0 {
            self.notify_presence_of_indexed_accessors(vm)?;
        }

        match self.shape() {
            NO_INDEXING_SHAPE => {
                if self.indexing_should_be_sparse() || attributes != 0 {
                    self.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(vm)?;
                    return self.put_direct_index_beyond_vector_length_with_array_storage(i, value, attributes, mode);
                }
                if index_is_sufficiently_beyond_length_for_sparse_map(i, 0) || i >= MIN_SPARSE_ARRAY_INDEX {
                    if !self.create_array_storage(vm, 0, 0) {
                        return Err(PutError::OutOfMemory);
                    }
                    return self.put_direct_index_beyond_vector_length_with_array_storage(i, value, attributes, mode);
                }
                if self.needs_slow_put_indexing() {
                    if !self.create_array_storage(vm, i + 1, self.get_new_vector_length(i + 1)) {
                        return Err(PutError::OutOfMemory);
                    }
                    self.with_array_storage_mut(|storage| {
                        storage.vector_mut()[i as usize] = value;
                        let count = storage.num_values_in_vector();
                        storage.set_num_values_in_vector(count + 1);
                    });
                    return Ok(true);
                }

                if !self.create_initial_for_value_and_set(vm, i, value) {
                    return Err(PutError::OutOfMemory);
                }
                Ok(true)
            }

            UNDECIDED_SHAPE => {
                self.convert_undecided_for_value(vm, value);
                // Reloop.
                self.put_direct_index(vm, i, value, attributes, mode)
            }

            INT32_SHAPE => {
                debug_assert!(!self.indexing_should_be_sparse());
                if attributes != 0 {
                    self.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(vm)?;
                    return self.put_direct_index_beyond_vector_length_with_array_storage(i, value, attributes, mode);
                }
                if !value.is_int32() {
                    self.convert_int32_for_value(vm, value);
                    return self.put_direct_index_slow_or_beyond_vector_length(vm, i, value, attributes, mode);
                }
                self.put_by_index_beyond_vector_length_without_attributes(vm, INT32_SHAPE, i, value)?;
                Ok(true)
            }

            DOUBLE_SHAPE => {
                debug_assert!(Options::with(|options| options.allow_double_shape));
                debug_assert!(!self.indexing_should_be_sparse());
                if attributes != 0 {
                    self.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(vm)?;
                    return self.put_direct_index_beyond_vector_length_with_array_storage(i, value, attributes, mode);
                }
                if !value.is_number() || value.as_number().is_nan() {
                    self.convert_double_to_contiguous(vm);
                    return self.put_direct_index_slow_or_beyond_vector_length(vm, i, value, attributes, mode);
                }
                self.put_by_index_beyond_vector_length_without_attributes(vm, DOUBLE_SHAPE, i, value)?;
                Ok(true)
            }

            CONTIGUOUS_SHAPE => {
                debug_assert!(!self.indexing_should_be_sparse());
                if attributes != 0 {
                    self.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(vm)?;
                    return self.put_direct_index_beyond_vector_length_with_array_storage(i, value, attributes, mode);
                }
                self.put_by_index_beyond_vector_length_without_attributes(vm, CONTIGUOUS_SHAPE, i, value)?;
                Ok(true)
            }

            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                if attributes != 0 {
                    self.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(vm)?;
                }
                self.put_direct_index_beyond_vector_length_with_array_storage(i, value, attributes, mode)
            }

            shape => unreachable!("putDirectIndexSlowOrBeyondVectorLength em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Interceptação de put em buraco pelos protótipos (SlowPutArrayStorage).
    // ---------------------------------------------------------------------------------------------

    /// `attemptToInterceptPutByIndexOnHole(globalObject, i, value, shouldThrow, putResult)`: `Some(putResult)`
    /// quando algum protótipo interceptou.
    pub(crate) fn attempt_to_intercept_put_by_index_on_hole(
        &self,
        vm: &VM,
        i: u32,
        value: JSValue,
        should_throw: bool,
    ) -> Result<Option<bool>, PutError> {
        // `self` não é `Proxy`: o `put` do `Proxy` não passa por aqui.
        debug_assert!(!self.structure().type_info().overrides_get_prototype());
        let prototype_value = self.get_prototype_direct();
        if prototype_value.is_null() {
            return Ok(None);
        }
        // Invariante: setPrototypeOf/__proto__ só gravam objeto ou null (ciclo é rejeitado antes), então o
        // protótipo direto é sempre objeto aqui.
        let prototype = JSObject::from_value(&prototype_value).expect("protótipo que não é objeto nem null");
        prototype.attempt_to_intercept_put_by_index_on_hole_for_prototype(vm, self.as_value(), i, value, should_throw)
    }

    /// `attemptToInterceptPutByIndexOnHoleForPrototype(globalObject, thisValue, i, value, shouldThrow, putResult)`.
    pub(crate) fn attempt_to_intercept_put_by_index_on_hole_for_prototype(
        &self,
        _vm: &VM,
        this_value: JSValue,
        i: u32,
        value: JSValue,
        should_throw: bool,
    ) -> Result<Option<bool>, PutError> {
        let mut current: Option<JSObjectHandle> = None;
        loop {
            let object: &JSObject = match &current {
                Some(handle) => handle,
                None => self,
            };

            // This has the same behavior with respect to prototypes as JSObject::put(). It only allows a
            // prototype to intercept a put if (a) the prototype declares the property we're after rather than
            // intercepting it via an override of JSObject::put(), and (b) that property is declared as ReadOnly
            // or Accessor.
            if has_any_array_storage(object.cell().indexing_type()) {
                let entry = object.with_array_storage(|storage| storage.sparse_map().and_then(|map| map.find(i)));
                if let Some(entry) = entry {
                    if entry.attributes() & (ACCESSOR | READ_ONLY) != 0 {
                        return object.sparse_entry_put(this_value, entry, value, should_throw).map(Some);
                    }
                }
            }

            // `ProxyObjectType`: o `putByIndexCommon` do `Proxy` decide. Nenhum objeto do porte é `TypedArray`
            // (veja o cabeçalho do módulo).
            if object.type_() == JSType::ProxyObjectType {
                return put_by_index_from_proxy(object, this_value, i, value, should_throw).map(Some);
            }

            // Só o `Proxy` sobrescreve `getPrototype`, e ele saiu acima.
            let prototype_value = object.get_prototype_direct();
            if prototype_value.is_null() {
                return Ok(None);
            }
            current = Some(JSObject::from_value(&prototype_value).expect("protótipo que não é objeto nem null"));
        }
    }

    // ---------------------------------------------------------------------------------------------
    // defineOwnIndexedProperty.
    // ---------------------------------------------------------------------------------------------

    /// `notifyPresenceOfIndexedAccessors(vm)`.
    pub fn notify_presence_of_indexed_accessors(&self, vm: &VM) -> Result<(), PutError> {
        if self.type_() == JSType::GlobalObjectType {
            let global_this = self
                .structure()
                .realm()
                .and_then(|realm| realm.global_this())
                .expect("JSGlobalObject sem globalThis (o JSGlobalProxy nasce no finishCreation)");
            return global_this.notify_presence_of_indexed_accessors(vm);
        }

        if self.structure().may_intercept_indexed_accesses() {
            return Ok(());
        }

        let Some(global_object) = self.structure().realm() else {
            return Ok(());
        };

        self.transition_indexing(vm, TransitionKind::AddIndexedAccessors);

        if !self.may_be_prototype() {
            return Ok(());
        }

        global_object.have_a_bad_time(vm);
        Ok(())
    }

    /// `switchToSlowPutArrayStorage(vm)`: o `Array` sem armazenamento ganha `ArrayStorage` antes, as formas
    /// `Undecided`/`Int32`/`Double`/`Contiguous` convertem direto para `SlowPutArrayStorage`, e o
    /// `ArrayStorage` só troca de estrutura.
    pub fn switch_to_slow_put_array_storage(&self, vm: &VM) {
        self.ensure_writable(vm);

        match self.shape() {
            NO_INDEXING_SHAPE => {
                // `ArrayClass`: o `CRASH()` do C++ cobre o não-array, que o `hasBrokenIndexing` já descartou.
                self.ensure_array_storage(vm);
                assert!(has_any_array_storage(self.cell().indexing_type()), "switchToSlowPutArrayStorage sem ArrayStorage");
                if has_slow_put_array_storage(self.cell().indexing_type()) {
                    return;
                }
                self.switch_to_slow_put_array_storage(vm);
            }
            UNDECIDED_SHAPE | INT32_SHAPE | DOUBLE_SHAPE | CONTIGUOUS_SHAPE => {
                // Residual (veja unsafe-audit.md): o chamador (`haveABadTime`) não tem como falhar, como no C++
                // (`RELEASE_ASSERT`); se a cópia não couber, o objeto segue sem `ArrayStorage`.
                let _converted = self.convert_to_array_storage(vm, TransitionKind::AllocateSlowPutArrayStorage);
            }
            ARRAY_STORAGE_SHAPE => self.transition_indexing(vm, TransitionKind::SwitchToSlowPutArrayStorage),
            shape => unreachable!("switchToSlowPutArrayStorage em forma {shape:#x} (CRASH)"),
        }
    }

    /// `putIndexedDescriptor(globalObject, map, entryInMap, descriptor, oldDescriptor)`.
    fn put_indexed_descriptor(&self, vm: &VM, index: u32, descriptor: &PropertyDescriptor, old_descriptor: &PropertyDescriptor) {
        if descriptor.is_data_descriptor() {
            let attributes = descriptor.attributes_overriding_current(old_descriptor) & !ACCESSOR;
            let value = if !descriptor.value().is_empty() {
                Some(descriptor.value())
            } else if old_descriptor.is_accessor_descriptor() {
                Some(js_undefined())
            } else {
                None
            };
            self.with_sparse_map_mut(|map| map.force_set(index, value, attributes));
            return;
        }

        if descriptor.is_accessor_descriptor() {
            let getter = if descriptor.getter_present() {
                descriptor.getter()
            } else if old_descriptor.is_accessor_descriptor() {
                old_descriptor.getter()
            } else {
                JSValue::undefined()
            };
            let setter = if descriptor.setter_present() {
                descriptor.setter()
            } else if old_descriptor.is_accessor_descriptor() {
                old_descriptor.setter()
            } else {
                JSValue::undefined()
            };

            let accessor: GetterSetterRef = GetterSetter::create_from_values(vm, getter, setter);
            let attributes = descriptor.attributes_overriding_current(old_descriptor) & !READ_ONLY;
            self.with_sparse_map_mut(|map| map.force_set(index, Some(accessor.as_value()), attributes));
            return;
        }

        debug_assert!(descriptor.is_generic_descriptor());
        let attributes = descriptor.attributes_overriding_current(old_descriptor);
        self.with_sparse_map_mut(|map| map.force_set(index, None, attributes));
    }

    /// `defineOwnIndexedProperty(globalObject, index, descriptor, throwException)`.
    pub fn define_own_indexed_property(
        &self,
        vm: &VM,
        index: u32,
        descriptor: &PropertyDescriptor,
        throw_exception: bool,
    ) -> Result<bool, PutError> {
        debug_assert!(index <= crate::runtime::identifier::MAX_ARRAY_INDEX);

        self.ensure_writable(vm);

        if !self.in_sparse_indexing_mode() {
            let empty_attributes_descriptor = PropertyDescriptor::new(js_undefined(), 0);
            debug_assert!(empty_attributes_descriptor.attributes() == 0);

            // Fast case: we're putting a regular property to a regular array
            if !descriptor.value().is_empty()
                && (descriptor.attributes() == 0
                    || (self.can_get_index_quickly(index)
                        && descriptor.attributes_overriding_current(&empty_attributes_descriptor) == 0))
                && self.can_do_fast_put_direct_index()
            {
                debug_assert!(!descriptor.is_accessor_descriptor());
                let mode = if throw_exception {
                    PutDirectIndexMode::PutDirectIndexShouldThrow
                } else {
                    PutDirectIndexMode::PutDirectIndexShouldNotThrow
                };
                return self.put_direct_index(vm, index, descriptor.value(), 0, mode);
            }

            self.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(vm)?;
        }

        if descriptor.attributes() & (READ_ONLY | ACCESSOR) != 0 {
            self.notify_presence_of_indexed_accessors(vm)?;
        }

        // 1. Let current be the result of calling the [[GetOwnProperty]] internal method of O with property name P.
        let is_new_entry = self.with_sparse_map_mut(|map| map.add(index));

        // 2. Let extensible be the value of the [[Extensible]] internal property of O.
        // 3. If current is undefined and extensible is false, then Reject.
        // 4. If current is undefined and extensible is true, then
        if is_new_entry {
            if !self.is_structure_extensible() {
                self.with_sparse_map_mut(|map| map.remove(index));
                return type_error(throw_exception, NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR);
            }

            // 4.a. If IsGenericDescriptor(Desc) or IsDataDescriptor(Desc) is true, then create an own data
            // property named P of object O whose [[Value]], [[Writable]], [[Enumerable]] and [[Configurable]]
            // attribute values are described by Desc. If the value of an attribute field of Desc is absent, the
            // attribute of the newly created property is set to its default value.
            // 4.b. Else, Desc must be an accessor Property Descriptor so, create an own accessor property named
            // P of object O whose [[Get]], [[Set]], [[Enumerable]] and [[Configurable]] attribute values are
            // described by Desc. If the value of an attribute field of Desc is absent, the attribute of the
            // newly created property is set to its default value.
            // 4.c. Return true.
            let defaults = PropertyDescriptor::new(js_undefined(), DONT_DELETE | DONT_ENUM | READ_ONLY);
            self.put_indexed_descriptor(vm, index, descriptor, &defaults);
            if index >= self.public_length() {
                self.set_public_length(index + 1);
            }
            return Ok(true);
        }

        // 5. Return true, if every field in Desc is absent.
        // 6. Return true, if every field in Desc also occurs in current and the value of every field in Desc is
        // the same value as the corresponding field in current when compared using the SameValue algorithm (9.12).
        let mut current = PropertyDescriptor::default();
        self.with_sparse_map_mut(|map| map.find(index).expect("entrada existente")).get_descriptor(&mut current);
        let is_empty_or_equal = descriptor.is_empty() || descriptor.equal_to(&current);
        if is_empty_or_equal {
            return Ok(true);
        }

        // 7. If the [[Configurable]] field of current is false then
        if !current.configurable() {
            // 7.a. Reject, if the [[Configurable]] field of Desc is true.
            if descriptor.configurable_present() && descriptor.configurable() {
                return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR);
            }
            // 7.b. Reject, if the [[Enumerable]] field of Desc is present and the [[Enumerable]] fields of
            // current and Desc are the Boolean negation of each other.
            if descriptor.enumerable_present() && current.enumerable() != descriptor.enumerable() {
                return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR);
            }
        }

        // 8. If IsGenericDescriptor(Desc) is true, then no further validation is required.
        if !descriptor.is_generic_descriptor() {
            // 9. Else, if IsDataDescriptor(current) and IsDataDescriptor(Desc) have different results, then
            if current.is_data_descriptor() != descriptor.is_data_descriptor() {
                // 9.a. Reject, if the [[Configurable]] field of current is false.
                if !current.configurable() {
                    return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR);
                }
                // 9.b. If IsDataDescriptor(current) is true, then convert the property named P of object O from a
                // data property to an accessor property. Preserve the existing values of the converted property's
                // [[Configurable]] and [[Enumerable]] attributes and set the rest of the property's attributes to
                // their default values.
                // 9.c. Else, convert the property named P of object O from an accessor property to a data
                // property. Preserve the existing values of the converted property's [[Configurable]] and
                // [[Enumerable]] attributes and set the rest of the property's attributes to their default values.
            } else if current.is_data_descriptor() && descriptor.is_data_descriptor() {
                // 10. Else, if IsDataDescriptor(current) and IsDataDescriptor(Desc) are both true, then
                // 10.a. If the [[Configurable]] field of current is false, then
                if !current.configurable() && !current.writable() {
                    // 10.a.i. Reject, if the [[Writable]] field of current is false and the [[Writable]] field
                    // of Desc is true.
                    if descriptor.writable() {
                        return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR);
                    }
                    // 10.a.ii. If the [[Writable]] field of current is false, then
                    // 10.a.ii.1. Reject, if the [[Value]] field of Desc is present and SameValue(Desc.[[Value]],
                    // current.[[Value]]) is false.
                    if !descriptor.value().is_empty() && !same_value(descriptor.value(), current.value()) {
                        return type_error(throw_exception, READONLY_PROPERTY_CHANGE_ERROR);
                    }
                }
                // 10.b. else, the [[Configurable]] field of current is true, so any change is acceptable.
            } else {
                debug_assert!(current.is_accessor_descriptor() && current.getter_present() && current.setter_present());
                // 11. Else, IsAccessorDescriptor(current) and IsAccessorDescriptor(Desc) are both true so, if the
                // [[Configurable]] field of current is false, then
                if !current.configurable() {
                    // 11.i. Reject, if the [[Set]] field of Desc is present and SameValue(Desc.[[Set]],
                    // current.[[Set]]) is false.
                    if descriptor.setter_present() && descriptor.setter() != current.setter() {
                        return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_SETTER_ERROR);
                    }
                    // 11.ii. Reject, if the [[Get]] field of Desc is present and SameValue(Desc.[[Get]],
                    // current.[[Get]]) is false.
                    if descriptor.getter_present() && descriptor.getter() != current.getter() {
                        return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_GETTER_ERROR);
                    }
                }
            }
        }

        // 12. For each attribute field of Desc that is present, set the correspondingly named attribute of the
        // property named P of object O to the value of the field.
        self.put_indexed_descriptor(vm, index, descriptor, &current);
        // 13. Return true.
        Ok(true)
    }

    // ---------------------------------------------------------------------------------------------
    // preventExtensions, seal, freeze.
    // ---------------------------------------------------------------------------------------------

    /// `JSObject::preventExtensions(object, globalObject)`: sempre tem sucesso (`true`).
    pub fn prevent_extensions(&self, vm: &VM) {
        if !self.is_structure_extensible() {
            // We've already set the internal [[PreventExtensions]] field to false. We don't call the
            // methodTable isExtensible here because it's not defined that way in the specification.
            return;
        }

        self.enter_dictionary_indexing_mode(vm);
        self.set_structure(vm, &Structure::prevent_extensions_transition(vm, &self.structure()));
    }

    /// `materializeLazyOwnProperties(vm)`.
    fn materialize_lazy_own_properties(&self, vm: &VM) {
        if !self.structure().type_info().overrides_get_own_special_property_names() {
            return;
        }

        // Force reifying lazy properties: special properties are materialized onto the object as a side effect of
        // enumerating them via getOwnPropertyNames.
        let mut property_names = PropertyNameArrayBuilder::new(vm, PropertyNameMode::StringsAndSymbols, PrivateSymbolMode::Exclude);
        if get_own_property_names(vm, self, &mut property_names, DontEnumPropertiesMode::Include).is_err() {
            panic!("materializeLazyOwnProperties: exceção inesperada (releaseAssertNoExceptionExceptTermination)");
        }
    }

    /// `JSObject::seal(vm)`.
    pub fn seal(&self, vm: &VM) {
        if self.structure().is_sealed() {
            return;
        }
        self.materialize_lazy_own_properties(vm);
        self.enter_dictionary_indexing_mode(vm);
        self.set_structure(vm, &Structure::seal_transition(vm, &self.structure()));
    }

    /// `JSObject::freeze(vm)`.
    pub fn freeze(&self, vm: &VM) {
        if self.structure().is_frozen() {
            return;
        }
        self.materialize_lazy_own_properties(vm);
        self.enter_dictionary_indexing_mode(vm);
        self.set_structure(vm, &Structure::freeze_transition(vm, &self.structure()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_object::JSFinalObject;
    use crate::runtime::js_value::js_number_i32;

    fn plain_object(vm: &VM) -> crate::runtime::js_object::JSObjectRef {
        let structure = JSFinalObject::create_structure(vm, None, JSValue::null(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
        JSFinalObject::create(vm, &structure)
    }

    #[test]
    fn prevent_extensions_on_plain_object_keeps_blank_indexing() {
        let vm = VM::new();
        let object = plain_object(&vm);
        object.prevent_extensions(&vm);
        assert!(!object.is_structure_extensible());
        assert_eq!(object.cell().indexing_type() & crate::runtime::indexing_type::INDEXING_SHAPE_MASK, NO_INDEXING_SHAPE);
    }

    #[test]
    fn freeze_with_indexed_elements_enters_sparse_mode() {
        let vm = VM::new();
        let object = plain_object(&vm);
        object.put_by_index(&vm, 0, js_number_i32(10), true).unwrap();
        object.put_by_index(&vm, 1, js_number_i32(20), true).unwrap();
        object.prevent_extensions(&vm);
        assert!(has_any_array_storage(object.cell().indexing_type()));
        assert!(object.in_sparse_indexing_mode());
        assert_eq!(object.public_length(), 2);
        assert_eq!(object.vector_length(), 0);
        assert!(object.with_array_storage(|storage| storage.sparse_map().unwrap().size() == 2));
    }

    #[test]
    fn define_own_indexed_property_on_sparse_object_applies_attributes() {
        let vm = VM::new();
        let object = plain_object(&vm);
        object.put_by_index(&vm, 0, js_number_i32(1), true).unwrap();
        object.enter_dictionary_indexing_mode(&vm);
        object.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(&vm).unwrap();

        let mut descriptor = PropertyDescriptor::default();
        descriptor.set_writable(false);
        descriptor.set_configurable(false);
        assert_eq!(object.define_own_indexed_property(&vm, 0, &descriptor, true), Ok(true));

        let entry = object.with_array_storage(|storage| storage.sparse_map().unwrap().find(0).unwrap());
        assert_eq!(entry.attributes() & (READ_ONLY | DONT_DELETE), READ_ONLY | DONT_DELETE);
        assert_eq!(entry.value().as_int32(), 1);

        // Mudar o valor de uma propriedade não configurável e não gravável é erro.
        let mut change = PropertyDescriptor::default();
        change.set_value(js_number_i32(2));
        assert_eq!(
            object.define_own_indexed_property(&vm, 0, &change, true),
            Err(PutError::TypeError(READONLY_PROPERTY_CHANGE_ERROR))
        );
    }

    #[test]
    fn put_by_index_into_frozen_sparse_object_is_rejected() {
        let vm = VM::new();
        let object = plain_object(&vm);
        object.put_by_index(&vm, 0, js_number_i32(1), true).unwrap();
        object.freeze(&vm);
        let mut descriptor = PropertyDescriptor::default();
        descriptor.set_writable(false);
        descriptor.set_configurable(false);
        object.define_own_indexed_property(&vm, 0, &descriptor, true).unwrap();
        assert_eq!(
            object.put_by_index(&vm, 0, js_number_i32(5), true),
            Err(PutError::TypeError(READONLY_PROPERTY_WRITE_ERROR))
        );
        assert_eq!(object.put_by_index(&vm, 0, js_number_i32(5), false), Ok(false));
    }
}
