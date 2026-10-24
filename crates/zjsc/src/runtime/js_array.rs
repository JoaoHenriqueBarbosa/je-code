//! Tradução de `runtime/JSArray.{h,cpp}` e `JSArrayInlines.h`, a superfície que o interpretador usa para
//! `op_new_array`, `op_new_array_with_size`, `op_spread` e `op_get_by_val` em array: `tryCreate`/`create`,
//! `createStructure`, `length`, `setLength`, `push`/`pushInline`, `pop`, `getOwnPropertySlot`, `put`,
//! `tryGetIndexQuickly`, `initializeIndex`, `constructArray`, `constructArrayPair`,
//! `mergeIndexingTypesForCopying` e `isHole`. `JSCellButterfly::createFromArray` (o coração do
//! `op_spread`) está em `js_cell_butterfly.rs`.
//!
//! DIVERGÊNCIAS (sem heap e sem GC, camada 3):
//!
//! - `JSArray` é um `JSObject` registrado pelo `JSObject::allocate` como `CellEntry::Object`, com o
//!   `JSType` `ArrayType` vindo da `Structure` (é o que `js_object.rs` prevê para "arrays"). O
//!   `JSArray` Rust é só um embrulho de `JSObjectRef` com `Deref` para `JSObject`; o `JSValue::Cell`
//!   é o `cell_id` do objeto, então `JSObject::from_value` também enxerga o array. Não há variante nova
//!   em `cell_registry`.
//! - O armazenamento indexado, as conversões de forma e `putByIndex` são os de `JSObject`
//!   (`js_object.rs`); este módulo só os orquestra como o `JSArray.cpp` faz. Para isso `js_object.rs`
//!   ganhou visibilidade `pub(crate)` em `set_public_length`, `count_elements`, `set_vector_value`,
//!   `set_vector_double` e `put_by_index_beyond_vector_length_without_attributes`.
//! - O C++ escolhe a `Structure` pelo `JSGlobalObject` (`arrayStructureForIndexingTypeDuringAllocation`,
//!   `arrayStructureForProfileDuringAllocation`). O `JSGlobalObject` do porte guarda as tabelas por forma
//!   (`array_structure_for_indexing_type_during_allocation`, que depois do `haveABadTime` é a de
//!   `SlowPutArrayStorage`), mas não o `ArrayAllocationProfile`: quem chama passa a `Structure`. A troca para
//!   `ArrayStorage` em `constructEmptyArray` com comprimento grande também fica com o chamador.
//! - `tryCreate` e `tryCreateUninitializedRestricted` aceitam as formas `ArrayStorage` e `SlowPutArrayStorage`
//!   (o vetor do primeiro tem o tamanho-base e nenhum valor, o do segundo o tamanho ótimo para o comprimento
//!   e `m_numValuesInVector = initialLength`). `eagerlyInitializeButterfly` existe para evitar a
//!   inicialização dupla sob GC e `ObjectInitializationScope`; sem GC, nas formas de vetor
//!   `try_create_uninitialized_restricted` é `try_create` (os elementos nascem buracos) e
//!   `initialize_index` escreve por cima.
//! - `vectorLength` segue o tamanho de classe do `MarkedSpace` (`available_contiguous_vector_length`): `tryCreate` com
//!   `vectorLengthHint` reserva ao menos o hint. O valor não aparece em JS, mas o relógio de bytes do coletor o conta.
//!   `reallocateAndShrinkButterfly` de `setLength` vira o mesmo laço de limpeza do ramo curto, porque
//!   encolher o `Vec` não é observável.
//! - O C++ lança `RangeError`/`TypeError`/`OutOfMemoryError` pelo `ThrowScope`. Aqui as funções que podem
//!   lançar devolvem `Result<_, ArrayError>`: `ArrayError::Put` carrega o `PutError` de `js_object.rs`
//!   (incluindo `Unported`) e `ArrayError::RangeError` é o `createRangeError`. `ToUint32`/`ToNumber` de
//!   `put("length", v)` só tratam primitivos (número, `undefined`, `null`, booleano); string e objeto
//!   (que chamariam `valueOf`) dão `Unported`.
//! - `isLengthWritable` lê o `LengthIsReadOnly` do `SparseArrayValueMap` do `ArrayStorage`
//!   (`sparse_array_value_map.rs`, ligado ao butterfly em `js_object_array_storage.rs`). As formas
//!   `ArrayWithArrayStorage`/`ArrayWithSlowPutArrayStorage` estão portadas em `setLength`, `push`, `pop`,
//!   `defineOwnProperty` e `initializeIndex`.
//!
//! Fora desta fatia, e por quê: `deleteProperty`, `getOwnSpecialPropertyNames` (enumeração),
//! `fastShift`, `shiftCount`, `unshiftCount`, `fastSlice`, `fastFill`, `fastToReversed`, `fastWith`,
//! `fastIncludes`, `fastCopyWithin`, `fastToSpliced`, `fastToString`, `fastFlat`, `appendMemcpy`,
//! `fillArgList`, `copyToArguments`, `isIteratorProtocolFastAndNonObservable`,
//! `isToPrimitiveFastAndNonObservable`, `holesMustForwardToPrototype`, `canFastCopy`, `canFastAppend`,
//! `canDoFastIndexedAccess`, `definitelyNegativeOneMiss` (consultam `JSGlobalObject`, watchpoints e
//! `MarkedArgumentBuffer`), `moveArrayElements`/`copyArrayElements`/`tryCloneArrayFromFast`,
//! `constructArrayNegativeIndexed` e `toLength` (dependem de `putDirectIndex`, `JSGlobalObject` e do
//! `ToLength` com chamada de usuário). Entram com os builtins de `Array.prototype`.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::error_messages::{
    READONLY_PROPERTY_CHANGE_ERROR, READONLY_PROPERTY_WRITE_ERROR, UNABLE_TO_DELETE_PROPERTY_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR,
};
use crate::runtime::indexing_type::{
    has_any_array_storage, has_contiguous, has_double, has_int32, has_undecided, IndexingType, ARRAY_CLASS,
    ARRAY_STORAGE_SHAPE, ARRAY_WITH_ARRAY_STORAGE, ARRAY_WITH_CONTIGUOUS, ARRAY_WITH_DOUBLE, ARRAY_WITH_INT32,
    ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE, ARRAY_WITH_UNDECIDED, COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS,
    COPY_ON_WRITE_ARRAY_WITH_DOUBLE, COPY_ON_WRITE_ARRAY_WITH_INT32, CONTIGUOUS_SHAPE, DOUBLE_SHAPE, INT32_SHAPE,
    IS_ARRAY, NON_ARRAY, SLOW_PUT_ARRAY_STORAGE_SHAPE,
};
use crate::runtime::identifier::MAX_ARRAY_INDEX;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{
    is_dense_enough_for_vector, type_error, IndexedStorage, JSNonFinalObject, JSObject, JSObjectRef, PutError,
    JS_NON_FINAL_OBJECT_S_INFO, MAX_STORAGE_VECTOR_LENGTH, MIN_SPARSE_ARRAY_INDEX,
};
use crate::runtime::js_array_storage::ArrayStorage;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{
    TypeInfo, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES, OVERRIDES_PUT,
};
use crate::runtime::js_value::{pnan, JSValue};
use crate::runtime::math_common::to_uint32;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::put_property_slot::PutPropertySlot;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `LengthExceededTheMaximumArrayLengthError`.
pub const LENGTH_EXCEEDED_THE_MAXIMUM_ARRAY_LENGTH_ERROR: &str = "Length exceeded the maximum array length";

/// A mensagem de `createRangeError(globalObject, "Invalid array length"_s)`.
pub const INVALID_ARRAY_LENGTH_ERROR: &str = "Invalid array length";

/// `JSArray::s_info`.
pub static JS_ARRAY_S_INFO: ClassInfo =
    ClassInfo { class_name: "Array", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// O resultado de erro das operações de `JSArray` que o C++ faz lançar (veja o cabeçalho do módulo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrayError {
    /// Os erros de `JSObject` (`TypeError`, falta de memória, `Unported`).
    Put(PutError),
    /// `throwException(globalObject, scope, createRangeError(globalObject, message))`.
    RangeError(&'static str),
}

impl From<PutError> for ArrayError {
    fn from(error: PutError) -> ArrayError {
        ArrayError::Put(error)
    }
}

/// `enum class ArrayFillMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayFillMode {
    Undefined,
    Empty,
}

/// `isHole(double)`: o buraco de um array `Double` é um NaN.
pub fn is_hole(value: f64) -> bool {
    value.is_nan()
}

/// `mergeIndexingTypesForCopying(type, other, allowPromotion)`: `NON_ARRAY` quando as formas não são
/// compatíveis para cópia.
pub fn merge_indexing_types_for_copying(
    indexing_type: IndexingType,
    other: IndexingType,
    allow_promotion: bool,
) -> IndexingType {
    if indexing_type & IS_ARRAY == 0 || other & IS_ARRAY == 0 {
        return NON_ARRAY;
    }

    if has_any_array_storage(indexing_type) || has_any_array_storage(other) {
        return NON_ARRAY;
    }

    if indexing_type == ARRAY_WITH_UNDECIDED {
        return other;
    }

    if other == ARRAY_WITH_UNDECIDED {
        return indexing_type;
    }

    // We can memcpy an Int32 and a Contiguous into a Contiguous array since
    // both share the same memory layout for Int32 numbers.
    if (indexing_type == ARRAY_WITH_INT32 || indexing_type == ARRAY_WITH_CONTIGUOUS)
        && (other == ARRAY_WITH_INT32 || other == ARRAY_WITH_CONTIGUOUS)
    {
        if other == ARRAY_WITH_CONTIGUOUS {
            return other;
        }
        return indexing_type;
    }

    if allow_promotion
        && (indexing_type == ARRAY_WITH_INT32 || indexing_type == ARRAY_WITH_DOUBLE)
        && (other == ARRAY_WITH_INT32 || other == ARRAY_WITH_DOUBLE)
    {
        if indexing_type == other {
            return indexing_type;
        }
        return ARRAY_WITH_DOUBLE;
    }

    if indexing_type != other {
        return NON_ARRAY;
    }

    indexing_type
}

/// `class JSArray : public JSNonFinalObject`: embrulho de `JSObjectRef` (veja o cabeçalho do módulo).
#[derive(Clone, Debug)]
pub struct JSArray {
    object: JSObjectRef,
}

impl std::ops::Deref for JSArray {
    type Target = JSObject;

    fn deref(&self) -> &JSObject {
        &self.object
    }
}

impl JSArray {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot |
    /// OverridesGetOwnSpecialPropertyNames | OverridesPut`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES
        | OVERRIDES_PUT;

    /// `shiftThreshold`.
    pub const SHIFT_THRESHOLD: u32 = 128;

    /// `createStructure(vm, globalObject, prototype, indexingType)`.
    pub fn create_structure(
        vm: &VM,
        global_object: Option<&JSGlobalObject>,
        prototype: JSValue,
        indexing_type: IndexingType,
    ) -> StructureRef {
        Structure::create_with_indexing_type(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ArrayType, JSArray::STRUCTURE_FLAGS),
            &JS_ARRAY_S_INFO,
            indexing_type,
            0,
        )
    }

    /// `tryCreate(vm, structure, initialLength, vectorLengthHint)`: `None` é o `nullptr` de
    /// `vectorLengthHint > MAX_STORAGE_VECTOR_LENGTH`. Os `initialLength` primeiros elementos são buracos.
    /// Com `ArrayStorage`/`SlowPutArrayStorage` o C++ ignora o hint: o `tryCreateArrayButterfly` dá um vetor
    /// de `BASE_ARRAY_STORAGE_VECTOR_LEN` posições, sem valores (`m_numValuesInVector = 0`).
    pub fn try_create_with_hint(
        vm: &VM,
        structure: &StructureRef,
        initial_length: u32,
        vector_length_hint: u32,
    ) -> Option<JSArray> {
        debug_assert!(vector_length_hint >= initial_length);
        let indexing_type = structure.indexing_type();
        if has_any_array_storage(indexing_type) {
            debug_assert!(
                indexing_type == ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE || indexing_type == ARRAY_WITH_ARRAY_STORAGE
            );
            return Some(JSArray::allocate_with_array_storage(vm, structure, ArrayStorage::new_base(initial_length)));
        }
        debug_assert!(
            has_undecided(indexing_type)
                || has_int32(indexing_type)
                || has_double(indexing_type)
                || has_contiguous(indexing_type)
        );

        if vector_length_hint > MAX_STORAGE_VECTOR_LENGTH {
            return None;
        }

        let object = JSObject::allocate(vm, structure);
        // `Butterfly::clearRange(indexingType, butterfly, 0, vectorLength)`: o `ensureLength` já deixa os
        // elementos novos como buraco (`JSValue()` ou `PNaN`).
        if !object.allocate_vector(vector_length_hint) {
            return None;
        }
        object.set_public_length(initial_length);
        Some(JSArray { object })
    }

    /// `createWithButterfly(vm, deferralContext, structure, butterfly)` para um butterfly de `ArrayStorage`.
    fn allocate_with_array_storage(vm: &VM, structure: &StructureRef, storage: ArrayStorage) -> JSArray {
        let object = JSObject::allocate(vm, structure);
        object.butterfly.borrow_mut().indexed = IndexedStorage::ArrayStorage(storage);
        JSArray { object }
    }

    /// `tryCreate(vm, structure, initialLength)`.
    pub fn try_create(vm: &VM, structure: &StructureRef, initial_length: u32) -> Option<JSArray> {
        JSArray::try_create_with_hint(vm, structure, initial_length, initial_length)
    }

    /// `create(vm, structure, initialLength)`: esgotar a memória é fatal (`RELEASE_ASSERT_RESOURCE_AVAILABLE`).
    pub fn create(vm: &VM, structure: &StructureRef, initial_length: u32) -> JSArray {
        JSArray::try_create(vm, structure, initial_length)
            .expect("Crash intentionally because memory is exhausted.")
    }

    /// `tryCreateUninitializedRestricted(scope, structure, initialLength)` (veja o cabeçalho do módulo). Com
    /// `ArrayStorage` o vetor tem o tamanho ótimo para `initialLength` e `m_numValuesInVector` já é
    /// `initialLength`: o `initializeIndex` seguinte só grava (`setIndexQuickly`).
    pub fn try_create_uninitialized_restricted(
        vm: &VM,
        structure: &StructureRef,
        initial_length: u32,
    ) -> Option<JSArray> {
        if initial_length > MAX_STORAGE_VECTOR_LENGTH {
            return None;
        }
        let indexing_type = structure.indexing_type();
        if !has_any_array_storage(indexing_type) {
            return JSArray::try_create(vm, structure, initial_length);
        }
        debug_assert!(indexing_type == ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE || indexing_type == ARRAY_WITH_ARRAY_STORAGE);
        let vector_length =
            ArrayStorage::optimal_vector_length(0, structure.out_of_line_capacity() as usize, initial_length);
        let mut storage = ArrayStorage::try_new(initial_length, vector_length)?;
        storage.set_num_values_in_vector(initial_length);
        Some(JSArray::allocate_with_array_storage(vm, structure, storage))
    }

    /// `isJSArray(JSCell*)` sobre um `cell_id` (`type() == ArrayType`).
    pub fn from_cell_id(cell_id: usize) -> Option<JSArray> {
        match crate::runtime::cell_registry::get(cell_id) {
            Some(crate::runtime::cell_registry::CellEntry::Object(object)) if object.type_() == JSType::ArrayType => {
                Some(JSArray { object })
            }
            _ => None,
        }
    }

    /// `asArray(JSValue)` com a checagem do `isJSArray(JSValue)`: `None` se o valor não é um array.
    pub fn from_value(value: &JSValue) -> Option<JSArray> {
        match value {
            JSValue::Cell(cell_id) => JSArray::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// O `JSArray` visto pela tabela de métodos: `type() == ArrayType` ou `DerivedArrayType` (o `ArrayPrototype`
    /// herda `JSArray` e o `JSArray::put`/`defineOwnProperty`/`getOwnPropertySlot` valem para ele, mas
    /// `isJSArray` é falso). Quem faz `isJSArray` usa `from_cell_id`.
    pub fn from_cell_id_by_class(cell_id: usize) -> Option<JSArray> {
        match crate::runtime::cell_registry::get(cell_id) {
            Some(crate::runtime::cell_registry::CellEntry::Object(object))
                if matches!(object.type_(), JSType::ArrayType | JSType::DerivedArrayType) =>
            {
                Some(JSArray { object })
            }
            _ => None,
        }
    }

    /// `from_cell_id_by_class` sobre um `JSValue`.
    pub fn from_value_by_class(value: &JSValue) -> Option<JSArray> {
        match value {
            JSValue::Cell(cell_id) => JSArray::from_cell_id_by_class(*cell_id),
            _ => None,
        }
    }

    /// O `JSObject*` base.
    pub fn object(&self) -> &JSObjectRef {
        &self.object
    }

    /// `length()` (`getArrayLength()`).
    pub fn length(&self) -> u32 {
        self.object.public_length()
    }

    /// `isLengthWritable()`.
    fn is_length_writable(&self) -> bool {
        if !has_any_array_storage(self.cell().indexing_type()) {
            return true;
        }
        self.object.with_array_storage(|storage| storage.sparse_map().map_or(true, |map| !map.length_is_read_only()))
    }

    /// `setLengthWritable(globalObject, writable)`.
    fn set_length_writable(&self, vm: &VM, writable: bool) {
        debug_assert!(self.is_length_writable() || !writable);
        if !self.is_length_writable() || writable {
            return;
        }

        self.object.enter_dictionary_indexing_mode(vm);

        self.object
            .with_array_storage_mut(|storage| storage.sparse_map_mut().expect("ASSERT(map)").set_length_is_read_only());
    }

    /// `tryGetIndexQuickly(i)`: o valor, ou `JSValue::empty()` se for buraco ou estiver fora do vetor.
    pub fn try_get_index_quickly(&self, i: u32) -> JSValue {
        if self.object.can_get_index_quickly(i) {
            self.object.get_index_quickly(i)
        } else {
            JSValue::empty()
        }
    }

    /// O `get_by_val` de array com índice inteiro: o caminho rápido do `getIndexQuickly` e, no buraco ou
    /// fora do vetor, `JSObject::get(index)` (que consulta o protótipo).
    pub fn get_by_index(&self, vm: &VM, i: u32) -> JSValue {
        let value = self.try_get_index_quickly(i);
        if !value.is_empty() {
            return value;
        }
        self.object.get_by_index(vm, i)
    }

    /// `initializeIndex(scope, i, value)` (JSObjectInlines.h) sobre a forma atual do array.
    pub fn initialize_index(&self, vm: &VM, i: u32, value: JSValue) {
        match self.cell().indexing_type() & crate::runtime::indexing_type::INDEXING_SHAPE_MASK {
            crate::runtime::indexing_type::UNDECIDED_SHAPE => self.object.set_index_quickly_to_undecided(vm, i, value),
            INT32_SHAPE => {
                debug_assert!(i < self.object.public_length());
                debug_assert!(i < self.object.vector_length());
                if !value.is_int32() {
                    self.object.convert_int32_to_double_or_contiguous_while_performing_set_index(vm, i, value);
                    return;
                }
                self.object.set_vector_value(i, value);
            }
            CONTIGUOUS_SHAPE => {
                debug_assert!(i < self.object.public_length());
                debug_assert!(i < self.object.vector_length());
                self.object.set_vector_value(i, value);
            }
            DOUBLE_SHAPE => {
                debug_assert!(i < self.object.public_length());
                debug_assert!(i < self.object.vector_length());
                if !value.is_number() || value.as_number().is_nan() {
                    self.object.convert_double_to_contiguous_while_performing_set_index(vm, i, value);
                    return;
                }
                self.object.set_vector_double(i, value.as_number());
            }
            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                // `initializeIndex` com `ArrayStorage`: só grava, `tryCreateUninitializedRestricted` já
                // deixou `length` e `m_numValuesInVector` em `initialLength`.
                if let IndexedStorage::ArrayStorage(storage) = &mut self.object.butterfly.borrow_mut().indexed {
                    debug_assert!(i < storage.length());
                    debug_assert!(i < storage.num_values_in_vector());
                    storage.vector_mut()[i as usize] = value;
                }
            }
            shape => unreachable!("initializeIndex em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// `setLength(globalObject, newLength, throwException)`.
    pub fn set_length(&self, vm: &VM, new_length: u32, throw_exception: bool) -> Result<bool, ArrayError> {
        match self.cell().indexing_mode() {
            ARRAY_CLASS => {
                if new_length == 0 {
                    return Ok(true);
                }
                if new_length >= MIN_SPARSE_ARRAY_INDEX {
                    if !self.object.ensure_array_storage(vm) {
                        return Err(ArrayError::Put(PutError::OutOfMemory));
                    }
                    return self.set_length_with_array_storage(new_length, throw_exception);
                }
                if !self.object.create_initial_undecided(vm, new_length) {
                    return Err(ArrayError::Put(PutError::OutOfMemory));
                }
                Ok(true)
            }

            COPY_ON_WRITE_ARRAY_WITH_INT32 | COPY_ON_WRITE_ARRAY_WITH_DOUBLE | COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS => {
                if new_length == self.object.public_length() {
                    return Ok(true);
                }
                self.object.convert_from_copy_on_write(vm);
                self.set_length_writable_shape(vm, new_length, throw_exception)
            }

            ARRAY_WITH_UNDECIDED | ARRAY_WITH_INT32 | ARRAY_WITH_DOUBLE | ARRAY_WITH_CONTIGUOUS => {
                self.set_length_writable_shape(vm, new_length, throw_exception)
            }

            ARRAY_WITH_ARRAY_STORAGE | ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE => {
                self.set_length_with_array_storage(new_length, throw_exception)
            }

            other => unreachable!("setLength em forma {other:#x} (CRASH no C++)"),
        }
    }

    /// `setLengthWithArrayStorage(globalObject, newLength, throwException, storage)`.
    fn set_length_with_array_storage(&self, new_length: u32, throw_exception: bool) -> Result<bool, ArrayError> {
        let (length, has_map) = self.object.with_array_storage(|storage| (storage.length(), storage.sparse_map().is_some()));

        // If the length is read only then we enter sparse mode, so should enter the following 'if'.
        debug_assert!(self.is_length_writable() || has_map);

        if has_map {
            // Fail if the length is not writable.
            if self.object.with_array_storage(|storage| storage.sparse_map().is_some_and(|map| map.length_is_read_only())) {
                return Ok(type_error(throw_exception, READONLY_PROPERTY_WRITE_ERROR)?);
            }

            if new_length < length {
                // Copy any keys we might be interested in into a vector (the map iterates in ascending order).
                let (keys, sparse_mode) = self.object.with_array_storage(|storage| {
                    let map = storage.sparse_map().expect("mapa esparso");
                    let keys: Vec<u32> =
                        map.iter().map(|entry| entry.index()).filter(|&index| index < length && index >= new_length).collect();
                    (keys, map.sparse_mode())
                });

                // Check if the array is in sparse mode. If so there may be non-configurable properties, so we
                // have to perform deletion with caution, if not we can delete values in any order.
                if sparse_mode {
                    for &index in keys.iter().rev() {
                        let entry = self
                            .object
                            .with_array_storage(|storage| storage.sparse_map().and_then(|map| map.find(index)))
                            .expect("ASSERT(it != map->notFound())");
                        if entry.attributes() & DONT_DELETE != 0 {
                            self.object.with_array_storage_mut(|storage| storage.set_length(index + 1));
                            return Ok(type_error(throw_exception, UNABLE_TO_DELETE_PROPERTY_ERROR)?);
                        }
                        self.object.with_array_storage_mut(|storage| storage.sparse_map_mut().expect("mapa esparso").remove(index));
                    }
                } else {
                    let is_empty = self.object.with_array_storage_mut(|storage| {
                        let map = storage.sparse_map_mut().expect("mapa esparso");
                        for &key in &keys {
                            map.remove(key);
                        }
                        map.is_empty()
                    });
                    if is_empty {
                        self.object.deallocate_sparse_index_map();
                    }
                }
            }
        }

        self.object.with_array_storage_mut(|storage| {
            if new_length < length {
                // Delete properties from the vector.
                let used_vector_length = length.min(storage.vector_length());
                for i in new_length..used_vector_length {
                    let had_value = !storage.vector()[i as usize].is_empty();
                    storage.vector_mut()[i as usize] = JSValue::empty();
                    if had_value {
                        let count = storage.num_values_in_vector();
                        storage.set_num_values_in_vector(count - 1);
                    }
                }
            }

            storage.set_length(new_length);
        });

        Ok(true)
    }

    /// O ramo `ArrayWith{Undecided,Int32,Double,Contiguous}` de `setLength`.
    fn set_length_writable_shape(&self, vm: &VM, new_length: u32, throw_exception: bool) -> Result<bool, ArrayError> {
        let public_length = self.object.public_length();
        if new_length == public_length {
            return Ok(true);
        }
        if new_length > MAX_STORAGE_VECTOR_LENGTH // This check ensures that we can do fast push.
            || (new_length >= MIN_SPARSE_ARRAY_INDEX
                && !is_dense_enough_for_vector(new_length, self.object.count_elements()))
        {
            if !self.object.ensure_array_storage(vm) {
                return Err(PutError::OutOfMemory.into());
            }
            return self.set_length_with_array_storage(new_length, throw_exception);
        }
        if new_length > public_length {
            if !self.object.ensure_length(vm, new_length) {
                return Err(PutError::OutOfMemory.into());
            }
            return Ok(true);
        }

        if self.cell().indexing_type() == ARRAY_WITH_DOUBLE {
            for i in (new_length..public_length).rev() {
                self.object.set_vector_double(i, pnan());
            }
        } else {
            for i in new_length..public_length {
                self.object.set_vector_value(i, JSValue::empty());
            }
        }
        self.object.set_public_length(new_length);
        Ok(true)
    }

    /// `push(globalObject, value)` / `pushInline`.
    pub fn push(&self, vm: &VM, value: JSValue) -> Result<(), ArrayError> {
        self.object.ensure_writable(vm);

        match self.cell().indexing_mode() {
            ARRAY_CLASS => {
                if !self.object.create_initial_undecided(vm, 0) {
                    return Err(ArrayError::Put(PutError::OutOfMemory));
                }
                self.object.convert_undecided_for_value(vm, value);
                self.push(vm, value)
            }

            ARRAY_WITH_UNDECIDED => {
                self.object.convert_undecided_for_value(vm, value);
                self.push(vm, value)
            }

            ARRAY_WITH_INT32 => {
                if !value.is_int32() {
                    self.object.convert_int32_for_value(vm, value);
                    return self.push(vm, value);
                }
                self.push_in_contiguous_vector(vm, INT32_SHAPE, value)
            }

            ARRAY_WITH_CONTIGUOUS => self.push_in_contiguous_vector(vm, CONTIGUOUS_SHAPE, value),

            ARRAY_WITH_DOUBLE => {
                if !value.is_number() || value.as_number().is_nan() {
                    self.object.convert_double_to_contiguous(vm);
                    return self.push(vm, value);
                }
                self.push_in_contiguous_vector(vm, DOUBLE_SHAPE, value)
            }

            ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE => {
                let old_length = self.length();
                if self.object.attempt_to_intercept_put_by_index_on_hole(vm, old_length, value, true)?.is_some() {
                    if old_length < 0xFFFF_FFFF {
                        self.set_length(vm, old_length + 1, true)?;
                    }
                    return Ok(());
                }
                self.push_in_array_storage(vm, value)
            }

            ARRAY_WITH_ARRAY_STORAGE => self.push_in_array_storage(vm, value),

            other => unreachable!("pushInline em forma {other:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// O caso `ArrayWithArrayStorage` de `pushInline` (também o fim do `ArrayWithSlowPutArrayStorage`).
    fn push_in_array_storage(&self, vm: &VM, value: JSValue) -> Result<(), ArrayError> {
        // Fast case - push within vector, always update m_length & m_numValuesInVector.
        let (length, vector_length) = self.object.with_array_storage(|storage| (storage.length(), storage.vector_length()));
        if length < vector_length {
            self.object.with_array_storage_mut(|storage| {
                storage.vector_mut()[length as usize] = value;
                storage.set_length(length + 1);
                let count = storage.num_values_in_vector();
                storage.set_num_values_in_vector(count + 1);
            });
            return Ok(());
        }

        // Pushing to an array of invalid length (2^31-1) stores the property, but throws a range error.
        if length > MAX_ARRAY_INDEX {
            self.object.put_by_index(vm, length, value, true)?;
            // Per ES5.1 15.4.4.7 step 6 & 15.4.5.1 step 3.d.
            return Err(ArrayError::RangeError(LENGTH_EXCEEDED_THE_MAXIMUM_ARRAY_LENGTH_ERROR));
        }

        // Handled the same as putIndex.
        self.object.put_by_index_beyond_vector_length_with_array_storage(vm, length, value, true)?;
        Ok(())
    }

    /// O miolo comum dos casos `Int32`, `Contiguous` e `Double` de `pushInline`: dentro do vetor escreve
    /// e cresce o `publicLength`; no fim do vetor passa por `putByIndexBeyondVectorLengthWithoutAttributes`.
    fn push_in_contiguous_vector(&self, vm: &VM, shape: IndexingType, value: JSValue) -> Result<(), ArrayError> {
        let length = self.object.public_length();
        debug_assert!(length <= self.object.vector_length());
        if length < self.object.vector_length() {
            if shape == DOUBLE_SHAPE {
                self.object.set_vector_double(length, value.as_number());
            } else {
                self.object.set_vector_value(length, value);
            }
            self.object.set_public_length(length + 1);
            return Ok(());
        }

        if length > MAX_ARRAY_INDEX {
            self.object.put_by_index(vm, length, value, true)?;
            return Err(ArrayError::RangeError(LENGTH_EXCEEDED_THE_MAXIMUM_ARRAY_LENGTH_ERROR));
        }

        self.object.put_by_index_beyond_vector_length_without_attributes(vm, shape, length, value)?;
        Ok(())
    }

    /// `pop(globalObject)`.
    pub fn pop(&self, vm: &VM) -> Result<JSValue, ArrayError> {
        self.object.ensure_writable(vm);

        match self.cell().indexing_type() {
            ARRAY_CLASS => return Ok(JSValue::undefined()),

            ARRAY_WITH_UNDECIDED => {
                if self.object.public_length() == 0 {
                    return Ok(JSValue::undefined());
                }
                // We have nothing but holes. So, drop down to the slow version.
            }

            ARRAY_WITH_INT32 | ARRAY_WITH_CONTIGUOUS => {
                let mut length = self.object.public_length();
                if length == 0 {
                    return Ok(JSValue::undefined());
                }
                length -= 1;
                assert!(length < self.object.vector_length());
                let value = self.try_get_index_quickly(length);
                if !value.is_empty() {
                    self.object.set_vector_value(length, JSValue::empty());
                    self.object.set_public_length(length);
                    return Ok(value);
                }
            }

            ARRAY_WITH_DOUBLE => {
                let mut length = self.object.public_length();
                if length == 0 {
                    return Ok(JSValue::undefined());
                }
                length -= 1;
                assert!(length < self.object.vector_length());
                let value = self.try_get_index_quickly(length);
                if !value.is_empty() {
                    self.object.set_vector_double(length, pnan());
                    self.object.set_public_length(length);
                    return Ok(value);
                }
            }

            ARRAY_WITH_ARRAY_STORAGE | ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE => {
                let length = self.object.public_length();
                if length == 0 {
                    if !self.is_length_writable() {
                        return Err(PutError::TypeError(READONLY_PROPERTY_WRITE_ERROR).into());
                    }
                    return Ok(JSValue::undefined());
                }

                let index = length - 1;
                let length_is_writable = self.is_length_writable();
                let element = self.object.with_array_storage_mut(|storage| {
                    if index >= storage.vector_length() {
                        return None;
                    }
                    let element = storage.vector()[index as usize];
                    if element.is_empty() {
                        return None;
                    }
                    let count = storage.num_values_in_vector();
                    storage.set_num_values_in_vector(count - 1);
                    storage.vector_mut()[index as usize] = JSValue::empty();

                    assert!(length_is_writable);
                    storage.set_length(index);
                    Some(element)
                });
                if let Some(element) = element {
                    return Ok(element);
                }
            }

            other => unreachable!("pop em forma {other:#x} (CRASH no C++)"),
        }

        let index = self.length() - 1;
        // Let element be the result of calling the [[Get]] internal method of O with argument indx.
        let element = self.object.get_by_index(vm, index);
        // `RETURN_IF_EXCEPTION`: getter de índice que lança deixa a exceção pendente e devolve `empty`.
        if vm.has_exception() {
            return Err(PutError::Pending.into());
        }
        // Call the [[Delete]] internal method of O with arguments indx and true.
        if !self.object.delete_property_by_index(vm, index)? {
            return Err(PutError::TypeError(UNABLE_TO_DELETE_PROPERTY_ERROR).into());
        }
        // Call the [[Put]] internal method of O with arguments "length", indx, and true.
        self.set_length(vm, index, true)?;
        // Return element.
        Ok(element)
    }

    /// `JSArray::getOwnPropertySlot(object, globalObject, propertyName, slot)`.
    pub fn get_own_property_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        if *property_name == vm.property_names.length {
            let attributes = if self.is_length_writable() {
                DONT_DELETE | DONT_ENUM
            } else {
                DONT_DELETE | DONT_ENUM | crate::runtime::property_attribute::READ_ONLY
            };
            slot.set_value(&self.object, attributes, JSValue::from_u32(self.length()));
            return true;
        }

        self.object.get_own_property_slot(vm, property_name, slot)
    }

    /// `JSArray::defineOwnProperty(object, globalObject, propertyName, descriptor, throwException)`
    /// (https://tc39.es/ecma262/#sec-array-exotic-objects-defineownproperty-p-desc). O `length` só
    /// aceita valor primitivo (`ToNumber` de objeto chamaria `valueOf`; veja o cabeçalho do módulo).
    pub fn define_own_property(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        throw_exception: bool,
    ) -> Result<bool, ArrayError> {
        // 2. If P is "length", then
        // https://tc39.es/ecma262/#sec-arraysetlength
        if *property_name == vm.property_names.length {
            let mut new_length = self.length();
            if !descriptor.value().is_empty() {
                new_length = array_length_from_value(&descriptor.value())?;
            }

            // OrdinaryDefineOwnProperty (https://tc39.es/ecma262/#sec-validateandapplypropertydescriptor) at steps
            // 1.a, 11.a, and 15 is now performed:
            // 4. If current.[[Configurable]] is false, then
            // 4.a. If Desc.[[Configurable]] is present and its value is true, return false.
            if descriptor.configurable_present() && descriptor.configurable() {
                return Ok(type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR)?);
            }
            // 4.b. If Desc.[[Enumerable]] is present and SameValue(Desc.[[Enumerable]], current.[[Enumerable]]) is
            // false, return false.
            if descriptor.enumerable_present() && descriptor.enumerable() {
                return Ok(type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR)?);
            }
            // 6. Else if SameValue(IsDataDescriptor(current), IsDataDescriptor(Desc)) is false, then
            // 6.a. If current.[[Configurable]] is false, return false.
            if descriptor.is_accessor_descriptor() {
                return Ok(type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR)?);
            }
            // 7. Else if IsDataDescriptor(current) and IsDataDescriptor(Desc) are both true, then
            // 7.a. If current.[[Configurable]] is false and current.[[Writable]] is false, then
            if !self.is_length_writable() {
                // 7.a.i. If Desc.[[Writable]] is present and Desc.[[Writable]] is true, return false.
                if descriptor.writable_present() && descriptor.writable() {
                    return Ok(type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR)?);
                }
                // 7.a.ii. If Desc.[[Value]] is present and SameValue(Desc.[[Value]], current.[[Value]]) is false,
                // return false.
                if new_length != self.length() {
                    return Ok(type_error(throw_exception, READONLY_PROPERTY_CHANGE_ERROR)?);
                }
            }

            // setLength() clears indices >= newLength and sets correct "length" value if [[Delete]] fails
            // (step 17.b.i)
            // O C++ não confere a exceção pendente entre o `setLength` e o `setLengthWritable`: o
            // `writable: false` vale mesmo quando o `[[Delete]]` falhou e o TypeError foi lançado.
            let success = if new_length != self.length() {
                self.set_length(vm, new_length, throw_exception)
            } else {
                Ok(true)
            };
            if descriptor.writable_present() {
                self.set_length_writable(vm, descriptor.writable());
            }
            return success;
        }

        // 4. Else if P is an array index (15.4), then
        // a. Let index be ToUint32(P).
        if let Some(index) = property_name.parse_index() {
            // b. Reject if index >= oldLen and oldLenDesc.[[Writable]] is false.
            if index >= self.length() && !self.is_length_writable() {
                return Ok(type_error(
                    throw_exception,
                    "Attempting to define numeric property on array with non-writable length property.",
                )?);
            }
            // c. Let succeeded be the result of calling the default [[DefineOwnProperty]] internal method (8.12.9)
            // on A passing P, Desc, and false as arguments.
            // d. Reject if succeeded is false.
            // e. If index >= oldLen ... f. Return true.
            return Ok(self.object.define_own_indexed_property(vm, index, descriptor, throw_exception)?);
        }

        Ok(self.object.define_own_non_index_property(vm, property_name, descriptor, throw_exception)?)
    }

    /// `JSArray::put(cell, globalObject, propertyName, value, slot)`.
    pub fn put(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Result<bool, ArrayError> {
        self.object.ensure_writable(vm);

        if *property_name == vm.property_names.length {
            if !self.is_length_writable() {
                if slot.is_strict_mode() {
                    return Err(PutError::TypeError("Array length is not writable").into());
                }
                return Ok(false);
            }

            if slot.this_value() != self.object.as_value() {
                return Ok(self.object.define_property_on_receiver(vm, property_name, value, slot)?);
            }

            let new_length = array_length_from_value(&value)?;
            return self.set_length(vm, new_length, slot.is_strict_mode());
        }

        Ok(self.object.put(vm, property_name, value, slot)?)
    }

    /// `mergeIndexingTypeForCopying(other, allowPromotion)`.
    pub fn merge_indexing_type_for_copying(&self, other: IndexingType, allow_promotion: bool) -> IndexingType {
        merge_indexing_types_for_copying(self.cell().indexing_type(), other, allow_promotion)
    }
}

/// `object->methodTable()->put(object, globalObject, propertyName, value, slot)`: o único ponto de escrita
/// por nome que respeita o `JSArray::put` (`length` por `setLength`). Um `Array` vai por `JSArray::put`, o
/// resto pelo `JSObject::put`. `put_to_object`, `put_inline_slow` (protótipo na cadeia) e `object_set`
/// (`Reflect.set`) chamam esta função, nenhum despacha por conta própria.
pub fn put_through_method_table(
    vm: &VM,
    object: &JSObject,
    property_name: &PropertyName,
    value: JSValue,
    slot: &mut PutPropertySlot,
) -> Result<bool, ArrayError> {
    match JSArray::from_cell_id_by_class(object.cell_id()) {
        Some(array) => array.put(vm, property_name, value, slot),
        None => Ok(object.put(vm, property_name, value, slot)?),
    }
}

/// `value.toNumber(globalObject)` seguido do `RETURN_IF_EXCEPTION`: strings e objetos passam pela
/// conversão completa (`valueOf`/`toString` do usuário); a exceção pendente vira `PutError::Pending`.
fn primitive_to_number(value: &JSValue) -> Result<f64, ArrayError> {
    let number = value.to_number();
    if crate::runtime::current_realm::has_pending_exception() {
        return Err(PutError::Pending.into());
    }
    Ok(number)
}

/// `JSArray::defineOwnProperty`/`put` do `length`: `toUInt32` e depois `toNumber` (duas conversões, como o
/// C++, então um `valueOf` do usuário roda duas vezes); `RangeError` se os dois não coincidem.
fn array_length_from_value(value: &JSValue) -> Result<u32, ArrayError> {
    let new_length = to_uint32(primitive_to_number(value)?);
    let value_as_number = primitive_to_number(value)?;
    if value_as_number != f64::from(new_length) {
        return Err(ArrayError::RangeError(INVALID_ARRAY_LENGTH_ERROR));
    }
    Ok(new_length)
}

/// `isJSArray(JSValue)`.
pub fn is_js_array(value: &JSValue) -> bool {
    JSArray::from_value(value).is_some()
}

/// `constructArray(globalObject, structure, values, length)`.
pub fn construct_array(vm: &VM, structure: &StructureRef, values: &[JSValue]) -> JSArray {
    let length = u32::try_from(values.len()).expect("constructArray com mais de 2^32 elementos");
    let array = JSArray::try_create_uninitialized_restricted(vm, structure, length)
        .expect("Crash intentionally because memory is exhausted.");
    for (i, value) in values.iter().enumerate() {
        array.initialize_index(vm, i as u32, *value);
    }
    array
}

/// `constructArrayPair(globalObject, first, second)`: a `structure` é a de `ArrayWithContiguous` do
/// `JSGlobalObject`, que o chamador passa.
pub fn construct_array_pair(vm: &VM, structure: &StructureRef, first: JSValue, second: JSValue) -> JSArray {
    debug_assert!(structure.indexing_type() == ARRAY_WITH_CONTIGUOUS);
    construct_array(vm, structure, &[first, second])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(vm: &VM, indexing_type: IndexingType) -> StructureRef {
        JSArray::create_structure(vm, None, JSValue::null(), indexing_type)
    }

    #[test]
    fn new_array_literal_is_contiguous_and_readable() {
        // [1, "x"] -> op_new_array
        let vm = VM::new();
        let array = construct_array(&vm, &structure(&vm, ARRAY_WITH_CONTIGUOUS), &[JSValue::Int32(1), JSValue::Int32(2)]);
        assert_eq!(array.length(), 2);
        assert_eq!(array.type_(), JSType::ArrayType);
        assert!(is_js_array(&array.as_value()));
        assert_eq!(array.get_by_index(&vm, 1), JSValue::Int32(2));
        assert_eq!(array.try_get_index_quickly(5), JSValue::empty());
    }

    #[test]
    fn new_array_with_size_has_holes() {
        let vm = VM::new();
        let array = JSArray::create(&vm, &structure(&vm, ARRAY_WITH_UNDECIDED), 3);
        assert_eq!(array.length(), 3);
        assert_eq!(array.try_get_index_quickly(0), JSValue::empty());
    }

    #[test]
    fn push_and_pop_roundtrip_with_shape_changes() {
        let vm = VM::new();
        let array = JSArray::create(&vm, &structure(&vm, ARRAY_WITH_UNDECIDED), 0);
        array.push(&vm, JSValue::Int32(1)).unwrap();
        array.push(&vm, JSValue::Int32(2)).unwrap();
        assert_eq!(array.length(), 2);
        assert_eq!(array.pop(&vm).unwrap(), JSValue::Int32(2));
        assert_eq!(array.pop(&vm).unwrap(), JSValue::Int32(1));
        assert_eq!(array.pop(&vm).unwrap(), JSValue::undefined());
        assert_eq!(array.length(), 0);
    }

    #[test]
    fn set_length_shrinks_and_grows() {
        let vm = VM::new();
        let array = construct_array(
            &vm,
            &structure(&vm, ARRAY_WITH_CONTIGUOUS),
            &[JSValue::Int32(1), JSValue::Int32(2), JSValue::Int32(3)],
        );
        assert_eq!(array.set_length(&vm, 1, true), Ok(true));
        assert_eq!(array.length(), 1);
        assert_eq!(array.try_get_index_quickly(1), JSValue::empty());
        assert_eq!(array.set_length(&vm, 4, true), Ok(true));
        assert_eq!(array.length(), 4);
        assert_eq!(array.try_get_index_quickly(3), JSValue::empty());
    }

    #[test]
    fn array_creation_past_the_maximum_vector_length_is_out_of_memory_not_an_abort() {
        let vm = VM::new();
        let too_long = MAX_STORAGE_VECTOR_LENGTH + 1;
        assert!(JSArray::try_create(&vm, &structure(&vm, ARRAY_WITH_UNDECIDED), too_long).is_none());
        assert!(JSArray::try_create(&vm, &structure(&vm, ARRAY_WITH_CONTIGUOUS), u32::MAX).is_none());
        assert!(JSArray::try_create_uninitialized_restricted(&vm, &structure(&vm, ARRAY_WITH_ARRAY_STORAGE), too_long)
            .is_none());
        assert!(JSArray::try_create_with_hint(&vm, &structure(&vm, ARRAY_WITH_DOUBLE), 0, too_long).is_none());
    }

    #[test]
    fn create_initial_storage_reports_success_and_set_length_stays_ok() {
        let vm = VM::new();
        let array = JSArray::create(&vm, &structure(&vm, ARRAY_WITH_UNDECIDED), 0);
        assert!(array.object().ensure_length(&vm, 8));
        assert!(array.object().vector_length() >= 8);
    }

    #[test]
    fn merge_indexing_types() {
        assert_eq!(merge_indexing_types_for_copying(ARRAY_WITH_UNDECIDED, ARRAY_WITH_DOUBLE, false), ARRAY_WITH_DOUBLE);
        assert_eq!(merge_indexing_types_for_copying(ARRAY_WITH_INT32, ARRAY_WITH_CONTIGUOUS, false), ARRAY_WITH_CONTIGUOUS);
        assert_eq!(merge_indexing_types_for_copying(ARRAY_WITH_INT32, ARRAY_WITH_DOUBLE, false), NON_ARRAY);
        assert_eq!(merge_indexing_types_for_copying(ARRAY_WITH_INT32, ARRAY_WITH_DOUBLE, true), ARRAY_WITH_DOUBLE);
        assert_eq!(merge_indexing_types_for_copying(NON_ARRAY, ARRAY_WITH_INT32, true), NON_ARRAY);
    }
}
