//! Tradução de `runtime/JSObject.{h,cpp}`, `JSObjectInlines.h` e `ArrayConventions.h`, a superfície que
//! `var o = {a: 1}; o.a` e os arrays simples exigem: `JSObject`/`JSFinalObject` com armazenamento de
//! propriedades nomeadas (inline e fora de linha) e indexadas (Undecided, Int32, Double, Contiguous),
//! `getDirect`/`putDirect`/`putDirectInternal`, `getOwnPropertySlot`, `getPropertySlot`, `get`, `put`
//! (`putInlineForJSObject`, `putInlineSlow`, `putInlineFast`, `definePropertyOnReceiver`),
//! `putByIndex` e as conversões de forma do `Butterfly`.
//!
//! DIVERGÊNCIAS (sem heap e sem GC, camada 3):
//!
//! - `JSObject` é a base de todas as células-objeto, como valor (`JSObject::new`), que as subclasses
//!   embutem por composição e expõem por `Deref` (`JSNonFinalObject`, `JSScope`, `JSCallee`...). O objeto
//!   comum (`JSFinalObject`, arrays) é um `Rc<JSObject>` (`JSObjectRef`) registrado no `cell_registry`
//!   como `CellEntry::Object`; a subclasse se registra sozinha e grava o `cell_id` na base com
//!   `set_cell_id`. `JSObject::from_value`/`from_cell_id` devolvem um `JSObjectHandle` (a entrada do
//!   registro, com `Deref` para a base), que `CellEntry::as_js_object` resolve: cada variante de
//!   subclasse ganha um braço lá. `PropertySlot`/`PutPropertySlot` guardam o `cell_id` do `slotBase`/`base`.
//!   `JSObject` e `JSFinalObject` são o mesmo tipo Rust: o que muda é o `TypeInfo` da `Structure`
//!   (`ObjectType` x `FinalObjectType`) e o `inlineCapacity`; `JSFinalObject` é um espaço de nomes com as
//!   constantes e o `create`.
//! - O `Butterfly` é um `Vec<JSValue>` de propriedades fora de linha (a posição é `offset - 64`, veja
//!   `property_offset::out_of_line_index`; o C++ usa índices negativos a partir do butterfly), mais o
//!   armazenamento indexado (`IndexedStorage`) e o `IndexingHeader::publicLength`. O `vectorLength` é o
//!   comprimento do vetor. O armazenamento inline é um `Vec<JSValue>` com `inlineCapacity` posições.
//!   `availableContiguousVectorLength` (arredondamento ao tamanho de classe do `MarkedSpace`) é a identidade:
//!   o comprimento do vetor não é observável.
//! - O C++ lança `TypeError`/`RangeError` pelo `ThrowScope`/`JSGlobalObject`. Aqui as funções que podem
//!   lançar devolvem `Result<bool, PutError>`: `Ok(b)` é o `bool` do C++ e `Err(PutError::TypeError(msg))`
//!   é o `typeError(globalObject, scope, shouldThrow=true, msg)`; quem tiver o `JSGlobalObject` converte
//!   o erro em exceção. `PutError::Unported` nomeia, uma a uma, as saídas do C++ que dependem de código
//!   ainda não portado (nunca é um valor-padrão silencioso).
//!
//! Em `js_object_array_storage.rs` (mesmo `impl JSObject`): `ArrayStorage`/`SlowPutArrayStorage` com o
//! `SparseArrayValueMap` (`enterDictionaryIndexingMode`, `defineOwnIndexedProperty`, `putDirectIndex`,
//! `putByIndexBeyondVectorLengthWithArrayStorage`, `preventExtensions`, `seal`, `freeze`).
//!
//! Fora desta fatia, e por quê: propriedades
//! estáticas (`HasStaticPropertyTable`, `reifyAllStaticProperties`), `ordinarySetSlow` (vive em
//! `proxy_object.rs`, com os auxiliares de `[[Set]]` por `Thrown`),
//! `createDataProperty`, `getOwnPropertyNames`/enumeração (em `own_property_names.rs`),
//! `JSNonFinalObject`, `putDirectNativeFunction*`/`putDirectBuiltinFunction*`,
//! `getPrototype(globalObject)`/`setPrototypeWithCycleCheck`, `GlobalProxy`, typed arrays, `Proxy`,
//! `visitChildren`/`estimatedSize`/`analyzeHeap` (GC), `invalidateStructureChainIntegrity`
//! (`StructureChain` não existe).
//!
//! Modo dicionário e accessors (esta fatia): `convertToDictionary`/`convertToUncacheableDictionary`,
//! `flattenDictionaryObject`, o ramo de dicionário de `putDirectInternal`, `putDirectToDictionaryWithout
//! Extensibility`, `putDirectWithoutTransition`, `putDirectAccessor`/`putDirectNonIndexAccessor`,
//! `deleteProperty`, `getOwnPropertyDescriptor`, `defineOwnProperty` (dado e accessor, nome não indexado)
//! e `validateAndApplyPropertyDescriptor`. O `isExtensible` é o da `Structure` (sem `Proxy` nem
//! `JSGlobalProxy`, que sobrescrevem). O que chamaria uma função JS (`callGetter`/`callSetter` com função
//! de verdade) devolve `PutError::Unported`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::error_messages::{
    NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR, READONLY_PROPERTY_CHANGE_ERROR, READONLY_PROPERTY_WRITE_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_GETTER_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_SETTER_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR,
};
use crate::runtime::delete_property_slot::DeletePropertySlot;
use crate::runtime::identifier::{Identifier, MAX_ARRAY_INDEX};
use crate::runtime::indexing_type::{
    has_contiguous, has_double, has_indexed_properties, has_int32, has_slow_put_array_storage, has_undecided,
    is_copy_on_write, IndexingType,
    ARRAY_STORAGE_SHAPE, ARRAY_WITH_CONTIGUOUS, ARRAY_WITH_DOUBLE, ARRAY_WITH_INT32, CONTIGUOUS_SHAPE, DOUBLE_SHAPE,
    INDEXING_SHAPE_MASK, INT32_SHAPE, NON_ARRAY, NO_INDEXING_SHAPE, SLOW_PUT_ARRAY_STORAGE_SHAPE, UNDECIDED_SHAPE,
};
use crate::runtime::js_array_storage::ArrayStorage;
use crate::runtime::js_cell::JSCell;
use crate::runtime::custom_getter_setter::CustomGetterSetter;
use crate::runtime::js_getter_setter::{GetterSetter, GetterSetterRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::generic_arguments;
use crate::runtime::js_global_proxy;
use crate::runtime::js_module_namespace_object;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::proxy_object::{get_property_slot_from_proxy, ordinary_set_slow, put_error_from_thrown, put_from_proxy, ProxyObject};
use crate::runtime::js_value::{js_null, pnan, JSValue};
use crate::runtime::operations::same_value;
use crate::runtime::property_attribute::{
    ACCESSOR, ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE, CUSTOM_ACCESSOR, CUSTOM_ACCESSOR_OR_VALUE, CUSTOM_VALUE, DONT_DELETE,
    READ_ONLY, READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR,
};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::{
    is_inline_offset, is_valid_offset, out_of_line_index, validate_offset, PropertyOffset, INVALID_OFFSET,
};
use crate::runtime::property_slot::{attributes_for_structure, InternalMethodType, PropertySlot};
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::structure_transition_table::TransitionKind;
use crate::runtime::typed_array_dispatch;
use crate::runtime::typed_array_type::is_typed_view;
use crate::runtime::vm::VM;

// ArrayConventions.h.

/// `MAX_STORAGE_VECTOR_LENGTH` (`IndexingHeader::maximumLength`).
pub const MAX_STORAGE_VECTOR_LENGTH: u32 = 0x1000_0000;
/// `MAX_STORAGE_VECTOR_INDEX`.
pub const MAX_STORAGE_VECTOR_INDEX: u32 = MAX_STORAGE_VECTOR_LENGTH - 1;
/// `MIN_SPARSE_ARRAY_INDEX`.
pub const MIN_SPARSE_ARRAY_INDEX: u32 = 100000;
/// `BASE_CONTIGUOUS_VECTOR_LEN`.
pub const BASE_CONTIGUOUS_VECTOR_LEN: u32 = 3;
/// `BASE_CONTIGUOUS_VECTOR_LEN_EMPTY`.
pub const BASE_CONTIGUOUS_VECTOR_LEN_EMPTY: u32 = 5;
/// `MIN_BEYOND_LENGTH_SPARSE_INDEX`.
pub const MIN_BEYOND_LENGTH_SPARSE_INDEX: u32 = 1000;
/// `minDensityMultiplier`.
pub const MIN_DENSITY_MULTIPLIER: u32 = 8;

/// `isDenseEnoughForVector(length, numValues)`.
pub fn is_dense_enough_for_vector(length: u32, num_values: u32) -> bool {
    length / MIN_DENSITY_MULTIPLIER <= num_values
}

/// `indexIsSufficientlyBeyondLengthForSparseMap(i, length)`.
pub fn index_is_sufficiently_beyond_length_for_sparse_map(i: u32, length: u32) -> bool {
    i >= MIN_BEYOND_LENGTH_SPARSE_INDEX && i > length
}

/// `nextLength(size_t)` de ButterflyInlines.h.
pub fn next_length(length: usize) -> usize {
    length + length / 2
}

/// `Butterfly::availableContiguousVectorLength(propertyCapacity, vectorLength)`: arredonda a `Butterfly` ao tamanho de
/// classe do `MarkedSpace` e devolve quantos elementos cabem nela. O valor não aparece em JS, mas o relógio de bytes
/// do coletor (`heap_size`) conta a `Butterfly` inteira, então o tamanho exato muda o momento da coleta.
pub fn available_contiguous_vector_length(property_capacity: usize, vector_length: u32) -> u32 {
    // `Butterfly::totalSize(0, propertyCapacity, true, payload)`: propriedades fora de linha, cabeçalho de indexação (8
    // bytes) e o vetor.
    let total_size = |payload: usize| property_capacity * 8 + 8 + payload;
    let cell_size = crate::runtime::heap_size::optimal_size_for(total_size(8 * vector_length as usize));
    ((cell_size - total_size(0)) / 8) as u32
}

/// `Butterfly::optimalContiguousVectorLength(propertyCapacity, vectorLength)`.
pub fn optimal_contiguous_vector_length(property_capacity: usize, vector_length: u32) -> u32 {
    let vector_length = if vector_length == 0 {
        BASE_CONTIGUOUS_VECTOR_LEN_EMPTY
    } else {
        BASE_CONTIGUOUS_VECTOR_LEN.max(vector_length)
    };
    available_contiguous_vector_length(property_capacity, vector_length)
}

/// `JSObject::s_info`.
pub static JS_OBJECT_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSObjectWithButterfly::s_info`.
pub static JS_OBJECT_WITH_BUTTERFLY_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_OBJECT_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSFinalObject::s_info`.
pub static JS_FINAL_OBJECT_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_OBJECT_WITH_BUTTERFLY_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSObject::PutMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutMode {
    PutModePut,
    PutModeDefineOwnProperty,
}

/// O resultado de erro das operações que o C++ faz lançar (veja o cabeçalho do módulo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PutError {
    /// `typeError(globalObject, scope, true, message)`.
    TypeError(&'static str),
    /// `throwStackOverflowError(globalObject, scope)`.
    StackOverflow,
    /// `throwOutOfMemoryError(globalObject, scope)`.
    OutOfMemory,
    /// `throwException(globalObject, scope, createRangeError(globalObject, message))`: o `Invalid array length`
    /// de um `JSArray::put` de `length` alcançado por `put_inline_slow` (protótipo `Array` na cadeia).
    RangeError(&'static str),
    /// `RETURN_IF_EXCEPTION(scope, ...)`: a exceção (a de um trap de `Proxy`, por exemplo) já está pendente
    /// no `VM`; quem recebe só propaga.
    Pending,
    /// Caminho do C++ que depende de código ainda não portado; a mensagem diz qual.
    Unported(&'static str),
}

/// `typeError(globalObject, scope, shouldThrow, message)`: `false` sem lançar quando `shouldThrow` é falso.
pub(crate) fn type_error(should_throw: bool, message: &'static str) -> Result<bool, PutError> {
    if should_throw {
        return Err(PutError::TypeError(message));
    }
    Ok(false)
}

/// O armazenamento indexado do butterfly: `contiguous()` (`WriteBarrier<Unknown>`) ou
/// `contiguousDouble()` (`double`) ou o `ArrayStorage` (`ArrayStorage`/`SlowPutArrayStorage`); o
/// comprimento do vetor é o do `Vec`.
#[derive(Debug, Default)]
pub(crate) enum IndexedStorage {
    /// Sem butterfly indexado (`NonArray`, `ArrayClass`).
    #[default]
    None,
    Values(Vec<JSValue>),
    Doubles(Vec<f64>),
    ArrayStorage(ArrayStorage),
}

impl IndexedStorage {
    pub(crate) fn vector_length(&self) -> u32 {
        match self {
            IndexedStorage::None => 0,
            IndexedStorage::Values(values) => values.len() as u32,
            IndexedStorage::Doubles(doubles) => doubles.len() as u32,
            IndexedStorage::ArrayStorage(storage) => storage.vector_length(),
        }
    }
}

/// O armazenamento de `Values` com `length` buracos, ou `None` se a memória falta.
fn empty_values_storage(length: usize) -> Option<IndexedStorage> {
    crate::runtime::fallible_alloc::try_filled_vec(JSValue::empty(), length).map(IndexedStorage::Values)
}

/// O armazenamento de `Doubles` com `length` buracos (`PNaN`), ou `None` se a memória falta.
fn pnan_doubles_storage(length: usize) -> Option<IndexedStorage> {
    crate::runtime::fallible_alloc::try_filled_vec(pnan(), length).map(IndexedStorage::Doubles)
}

/// O armazenamento que uma forma de indexação pede (`ArrayWithDouble` usa `f64`, os demais `JSValue`).
fn indexed_storage_for_shape(indexing_type: IndexingType) -> IndexedStorage {
    match indexing_type & INDEXING_SHAPE_MASK {
        NO_INDEXING_SHAPE => IndexedStorage::None,
        DOUBLE_SHAPE => IndexedStorage::Doubles(Vec::new()),
        ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => IndexedStorage::ArrayStorage(ArrayStorage::empty()),
        _ => IndexedStorage::Values(Vec::new()),
    }
}

/// `class Butterfly`.
#[derive(Debug, Default)]
pub(crate) struct Butterfly {
    /// O armazenamento de propriedades fora de linha.
    out_of_line: Vec<JSValue>,
    pub(crate) indexed: IndexedStorage,
    /// `IndexingHeader::publicLength`; na forma `ArrayStorage` o comprimento é o do próprio
    /// `ArrayStorage` (o C++ guarda os dois no mesmo cabeçalho).
    public_length: u32,
}

/// `class JSObject` (a base de `JSFinalObject`, `JSNonFinalObject`, `JSObjectWithButterfly` e das
/// subclasses, que a embutem por composição e a expõem por `Deref`).
#[derive(Debug)]
pub struct JSObject {
    cell: JSCell,
    /// O `cell_id` da célula que contém este objeto (a própria, ou a subclasse que a embute): 0 até quem
    /// registra a célula no `cell_registry` chamar `set_cell_id`.
    cell_id: Cell<usize>,
    /// O armazenamento inline (`JSFinalObject::inlineStorage`): `inlineCapacity` posições.
    inline_storage: RefCell<Vec<JSValue>>,
    pub(crate) butterfly: RefCell<Butterfly>,
    /// O que o `installObjectPropertyChangeAdaptiveWatchpoint` vigia neste objeto: os offsets das
    /// propriedades e o conjunto que dispara quando uma delas é escrita, redefinida ou apagada.
    replacement_watch: RefCell<Option<ReplacementWatch>>,
}

/// Os offsets vigiados de um objeto e o `WatchpointSet` (`true` = disparou, ou seja, invalidado).
#[derive(Debug)]
struct ReplacementWatch {
    offsets: Vec<PropertyOffset>,
    fired: Rc<Cell<bool>>,
}

/// Referência compartilhada a um objeto "folha" (`JSFinalObject`, registrado como `CellEntry::Object`),
/// o `JSObject*` do C++ para os objetos comuns.
pub type JSObjectRef = Rc<JSObject>;

/// O `JSObject*` de qualquer célula que seja objeto (comum ou subclasse): segura a entrada do registro
/// viva e faz `Deref` para a base `JSObject` dela (`CellEntry::as_js_object`).
#[derive(Clone)]
pub struct JSObjectHandle(CellEntry);

/// `&brand->uid()`: a identidade do `UniquedStringImpl` do símbolo privado, que as marcas comparam.
fn private_brand_key(brand: &crate::runtime::symbol::Symbol) -> crate::wtf::text::string_impl::UniquedKey {
    crate::wtf::text::string_impl::UniquedKey(Rc::clone(brand.uid().string_impl()))
}

impl JSObjectHandle {
    fn new(entry: CellEntry) -> Option<JSObjectHandle> {
        entry.as_js_object()?;
        Some(JSObjectHandle(entry))
    }
}

impl std::ops::Deref for JSObjectHandle {
    type Target = JSObject;

    fn deref(&self) -> &JSObject {
        // Invariante do `new`: só se cria o handle de uma entrada que é objeto.
        self.0.as_js_object().expect("JSObjectHandle sobre entrada que não é objeto")
    }
}

impl std::fmt::Debug for JSObjectHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("JSObjectHandle").field(&self.cell_id()).finish()
    }
}

/// O objeto corrente de um laço pela cadeia de protótipos que começa em `start`.
fn chain_object<'a>(start: &'a JSObject, current: &'a Option<JSObjectHandle>) -> &'a JSObject {
    match current {
        Some(handle) => handle,
        None => start,
    }
}

/// `JSNonFinalObject::s_info` (a classe-base dos objetos com subclasse: escopos, callees, funções).
pub static JS_NON_FINAL_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_OBJECT_WITH_BUTTERFLY_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSNonFinalObject : public JSObjectWithButterfly`: a base das classes derivadas de `JSObject`
/// (`JSScope`, `JSCallee`...), que a embutem e fazem `Deref` para ela.
#[derive(Debug)]
pub struct JSNonFinalObject {
    base: JSObject,
}

impl std::ops::Deref for JSNonFinalObject {
    type Target = JSObject;

    fn deref(&self) -> &JSObject {
        &self.base
    }
}

impl JSNonFinalObject {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSObject::STRUCTURE_FLAGS;

    /// `JSNonFinalObject(VM&, Structure*, Butterfly* = nullptr)`.
    pub fn new(vm: &VM, structure: StructureRef) -> JSNonFinalObject {
        JSNonFinalObject { base: JSObject::new(vm, &structure) }
    }

    /// `JSNonFinalObject::createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &JS_NON_FINAL_OBJECT_S_INFO,
        )
    }
}

/// O `getPrototype` da tabela de métodos de uma classe com `OverridesGetPrototype`.
pub type GetPrototypeMethod = fn(&JSObject, &JSGlobalObject) -> Result<JSValue, Thrown>;

thread_local! {
    /// As classes de fora do motor com `OverridesGetPrototype` e o `getPrototype` de cada uma (a entrada da tabela
    /// de métodos que o C++ guarda na `ClassInfo`).
    static GET_PROTOTYPE_METHODS: RefCell<Vec<(&'static ClassInfo, GetPrototypeMethod)>> = const { RefCell::new(Vec::new()) };
}

/// Registra o `getPrototype` de `class_info` (as estruturas dela levam `OVERRIDES_GET_PROTOTYPE`).
pub fn register_get_prototype_method(class_info: &'static ClassInfo, method: GetPrototypeMethod) {
    GET_PROTOTYPE_METHODS.with(|methods| {
        let mut methods = methods.borrow_mut();
        if !methods.iter().any(|(class, _)| std::ptr::eq(*class, class_info)) {
            methods.push((class_info, method));
        }
    });
}

/// `class JSFinalObject`: espaço de nomes das constantes e do `create` (veja o cabeçalho do módulo).
pub struct JSFinalObject;

impl JSFinalObject {
    /// `JSFinalObject::defaultSizeInBytes`.
    pub const DEFAULT_SIZE_IN_BYTES: u32 = 64;
    /// `JSFinalObject::maxSizeInBytes`.
    pub const MAX_SIZE_IN_BYTES: u32 = 512;
    /// `sizeof(JSObjectWithButterfly)`: o cabeçalho de 8 bytes do `JSCell` mais o ponteiro do butterfly.
    const OBJECT_WITH_BUTTERFLY_SIZE: u32 = 16;
    /// `JSFinalObject::defaultInlineCapacity`.
    pub const DEFAULT_INLINE_CAPACITY: u32 = (Self::DEFAULT_SIZE_IN_BYTES - Self::OBJECT_WITH_BUTTERFLY_SIZE) / 8;
    /// `JSFinalObject::maxInlineCapacity`.
    pub const MAX_INLINE_CAPACITY: u32 = (Self::MAX_SIZE_IN_BYTES - Self::OBJECT_WITH_BUTTERFLY_SIZE) / 8;
    /// `JSFinalObject::defaultIndexingType`.
    pub const DEFAULT_INDEXING_TYPE: IndexingType = NON_ARRAY;

    /// `JSFinalObject::typeInfo()`.
    pub const fn type_info() -> TypeInfo {
        TypeInfo::new(JSType::FinalObjectType, 0)
    }

    /// `JSFinalObject::createStructure(vm, globalObject, prototype, inlineCapacity)`.
    pub fn create_structure(
        vm: &VM,
        global_object: Option<&JSGlobalObject>,
        prototype: JSValue,
        inline_capacity: u32,
    ) -> StructureRef {
        Structure::create_with_indexing_type(
            vm,
            global_object,
            prototype,
            JSFinalObject::type_info(),
            &JS_FINAL_OBJECT_INFO,
            JSFinalObject::DEFAULT_INDEXING_TYPE,
            inline_capacity,
        )
    }

    /// `JSFinalObject::create(vm, structure)`.
    pub fn create(vm: &VM, structure: &StructureRef) -> JSObjectRef {
        debug_assert!(structure.type_() == JSType::FinalObjectType);
        JSObject::allocate(vm, structure)
    }
}

impl JSObject {
    /// `JSObject::visitChildren`: o protótipo da estrutura, o armazenamento inline e o butterfly (propriedades
    /// fora de linha e o indexado). Um empréstimo de escrita em curso faz a coleta desistir.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.structure().stored_prototype());
        match (self.inline_storage.try_borrow(), self.butterfly.try_borrow()) {
            (Ok(inline_storage), Ok(butterfly)) => {
                marker.values(&inline_storage);
                marker.values(&butterfly.out_of_line);
                match &butterfly.indexed {
                    IndexedStorage::None | IndexedStorage::Doubles(_) => {}
                    IndexedStorage::Values(values) => marker.values(values),
                    IndexedStorage::ArrayStorage(storage) => storage.visit_children(marker),
                }
            }
            _ => marker.abort(),
        }
    }

    /// Bytes reservados pelo armazenamento indexado do butterfly (capacidade dos `Vec`; para o `ArrayStorage`,
    /// o `vectorLength`). Diagnóstico de vazamento: `cell_registry::live_butterfly_bytes`. Um butterfly em
    /// edição (`borrow_mut` ativo) conta zero.
    pub fn indexed_storage_bytes(&self) -> usize {
        let Ok(butterfly) = self.butterfly.try_borrow() else {
            return 0;
        };
        match &butterfly.indexed {
            IndexedStorage::None => 0,
            IndexedStorage::Values(values) => values.capacity() * std::mem::size_of::<JSValue>(),
            IndexedStorage::Doubles(doubles) => doubles.capacity() * std::mem::size_of::<f64>(),
            IndexedStorage::ArrayStorage(storage) => storage.vector_length() as usize * std::mem::size_of::<JSValue>(),
        }
    }

    /// `JSCell::StructureFlags` (nenhuma flag), a base de `JSObject::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = 0;

    /// `JSObject::createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, JSObject::STRUCTURE_FLAGS),
            &JS_OBJECT_INFO,
        )
    }

    /// `JSObject(VM&, Structure*)`: o armazenamento inline começa vazio (`JSValue()`), o fora de linha
    /// com a capacidade da estrutura e o indexado vazio, no formato que o `indexingType` da estrutura
    /// pede. O `cell_id` fica 0 até o registro da célula (`set_cell_id`).
    pub fn new(vm: &VM, structure: &StructureRef) -> JSObject {
        JSObject {
            cell: JSCell::new(vm, structure),
            cell_id: Cell::new(0),
            inline_storage: RefCell::new(vec![JSValue::empty(); structure.inline_capacity() as usize]),
            butterfly: RefCell::new(Butterfly {
                out_of_line: vec![JSValue::empty(); structure.out_of_line_capacity() as usize],
                indexed: indexed_storage_for_shape(structure.indexing_type()),
                public_length: 0,
            }),
            replacement_watch: RefCell::new(None),
        }
    }

    /// `installObjectPropertyChangeAdaptiveWatchpoint(setupAdaptiveWatchpoint(this, object, name), set)` para
    /// cada nome: o `fired` passa a `true` quando alguma das propriedades é substituída (escrita, redefinida,
    /// mudança de atributo) ou apagada, mesmo com o mesmo valor. Por offset, o que o `put_direct_offset`
    /// vê em toda escrita de slot existente (inclusive a do `delete`, que esvazia o slot).
    pub fn watch_property_replacement(&self, vm: &VM, names: &[PropertyName], fired: &Rc<Cell<bool>>) {
        let offsets = names
            .iter()
            .map(|name| self.get_direct_offset(vm, name))
            .filter(|offset| is_valid_offset(*offset))
            .collect();
        *self.replacement_watch.borrow_mut() = Some(ReplacementWatch { offsets, fired: Rc::clone(fired) });
    }

    /// `JSObject::finishCreation(vm)`: só tem `ASSERT`s.
    pub fn finish_creation(&self, _vm: &VM) {
        debug_assert!(self.structure().is_object());
    }

    /// `allocateCell<JSObject>` mais o construtor: cria o objeto e o registra no `cell_registry` como
    /// `CellEntry::Object`. Para os objetos comuns (`JSFinalObject`, arrays...); as subclasses com campos
    /// próprios se registram sozinhas e chamam `set_cell_id` na base.
    pub fn allocate(vm: &VM, structure: &StructureRef) -> JSObjectRef {
        let cell_id = cell_registry::reserve();
        let object = Rc::new(JSObject::new(vm, structure));
        object.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::Object(Rc::clone(&object)));
        object
    }

    /// Procura o objeto pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSObjectHandle> {
        JSObjectHandle::new(cell_registry::get(cell_id)?)
    }

    /// `JSValue::getObject()`: o objeto, se o valor é uma célula que é objeto.
    pub fn from_value(value: &JSValue) -> Option<JSObjectHandle> {
        match value {
            JSValue::Cell(cell_id) => JSObject::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id.get()
    }

    /// Grava o `cell_id` da célula que contém este objeto, depois de registrada no `cell_registry`.
    pub fn set_cell_id(&self, cell_id: usize) {
        self.cell_id.set(cell_id);
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        debug_assert!(self.cell_id() != 0, "objeto sem cell_id (célula ainda não registrada)");
        JSValue::from_cell(self.cell_id())
    }

    /// O cabeçalho `JSCell`.
    pub fn cell(&self) -> &JSCell {
        &self.cell
    }

    /// `structure()`.
    pub fn structure(&self) -> StructureRef {
        self.cell.structure()
    }

    /// `butterfly() != nullptr`: há armazenamento fora de linha ou indexado alocado.
    pub fn has_butterfly(&self) -> bool {
        let butterfly = self.butterfly.borrow();
        !butterfly.out_of_line.is_empty() || !matches!(butterfly.indexed, IndexedStorage::None)
    }

    /// `setStructure(vm, structure)`.
    pub fn set_structure(&self, vm: &VM, structure: &StructureRef) {
        self.cell.set_structure(vm, structure);
    }

    /// `type()`.
    pub fn type_(&self) -> JSType {
        self.cell.type_()
    }

    /// `hasPrivateBrand(globalObject, brand)` (JSObjectInlines.h): a estrutura é com marca e a cadeia de
    /// marcas tem o `uid()` do símbolo privado.
    pub fn has_private_brand(&self, brand: &crate::runtime::symbol::Symbol) -> bool {
        debug_assert!(brand.uid().is_private());
        crate::runtime::branded_structure::BrandedStructure::check_brand(&self.structure(), &private_brand_key(brand))
    }

    /// `checkPrivateBrand(globalObject, brand)`: `Err` leva a mensagem de `createPrivateMethodAccessError`
    /// (o chamador a lança como `TypeError`).
    pub fn check_private_brand(&self, brand: &crate::runtime::symbol::Symbol) -> Result<(), &'static str> {
        if self.has_private_brand(brand) {
            Ok(())
        } else {
            Err(crate::runtime::error_messages::PRIVATE_METHOD_ACCESS_ERROR)
        }
    }

    /// `setPrivateBrand(globalObject, brand)`: `Err` leva a mensagem de `createReinstallPrivateMethodError`
    /// ou a do objeto de WebAssembly GC; o chamador a lança como `TypeError`.
    pub fn set_private_brand(&self, vm: &VM, brand: &crate::runtime::symbol::Symbol) -> Result<(), &'static str> {
        if self.has_private_brand(brand) {
            return Err(crate::runtime::error_messages::REINSTALL_PRIVATE_METHOD_ERROR);
        }

        if self.type_() == JSType::WebAssemblyGCObjectType {
            return Err(crate::runtime::error_messages::PRIVATE_METHOD_ON_WEB_ASSEMBLY_GC_OBJECT_ERROR);
        }

        let structure = self.structure();
        let new_structure = Structure::set_brand_transition(vm, &structure, &private_brand_key(brand));
        debug_assert!(new_structure.is_branded_structure());
        debug_assert!(new_structure.out_of_line_capacity() != 0 || structure.out_of_line_capacity() == 0);
        self.set_structure(vm, &new_structure);
        Ok(())
    }

    /// `classInfo()`.
    pub fn class_info(&self) -> &'static ClassInfo {
        self.cell.class_info()
    }

    // Prototype.

    /// `getPrototypeDirect()`.
    pub fn get_prototype_direct(&self) -> JSValue {
        self.structure().stored_prototype()
    }

    /// `getPrototype(globalObject)`: o `getPrototypeDirect` salvo quando o tipo sobrescreve o método
    /// (`OverridesGetPrototype`: o `Proxy` responde pelo trap `getPrototypeOf`).
    pub fn get_prototype(&self, global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
        if !self.structure().type_info().overrides_get_prototype() {
            return Ok(self.get_prototype_direct());
        }
        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            return target_object.get_prototype(global_object);
        }
        if let Some(proxy) = ProxyObject::from_cell_id(self.cell_id()) {
            return proxy.perform_get_prototype(global_object);
        }
        // O `methodTable()->getPrototype` das classes que o hospedeiro registrou (no bun, o `ImportMeta`).
        let class_info = self.class_info();
        let registered = GET_PROTOTYPE_METHODS.with(|methods| {
            methods.borrow().iter().find(|(class, _)| std::ptr::eq(*class, class_info)).map(|(_, method)| *method)
        });
        if let Some(method) = registered {
            return method(self, global_object);
        }
        // `WebAssemblyGCObjectBase::getPrototype`: `jsNull()`. Os únicos tipos do motor com `OverridesGetPrototype`
        // no C++ são `ProxyObject`, `JSGlobalProxy` e `WebAssemblyGCObjectBase`.
        assert!(
            self.type_() == JSType::WebAssemblyGCObjectType,
            "OverridesGetPrototype em tipo que não é Proxy, JSGlobalProxy nem WebAssemblyGCObject"
        );
        Ok(js_null())
    }

    /// `mayBePrototype()`.
    pub fn may_be_prototype(&self) -> bool {
        self.structure().may_be_prototype()
    }

    /// `didBecomePrototype(vm)` (StructureCreateInlines.h).
    pub fn did_become_prototype(&self, vm: &VM) {
        let old_structure = self.structure();
        if !old_structure.may_be_prototype() {
            let new_structure = Structure::become_prototype_transition(vm, &old_structure);
            self.set_structure(vm, &new_structure);
        }

        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            target_object.did_become_prototype(vm);
        }
    }

    /// `setPrototypeDirect(vm, prototype)`.
    pub fn set_prototype_direct(&self, vm: &VM, prototype: JSValue) {
        // `JSFunction` é objeto e entra em `JSObject::from_value` (`CellEntry::as_js_object` inclui `Function`;
        // só a `ObjectRef` a distingue, para materializar `name`/`length`): `class A extends B` com `B` função
        // tem a função como protótipo.
        let function = prototype.as_js_function();
        debug_assert!(JSObject::from_value(&prototype).is_some() || function.is_some() || prototype.is_null());
        if let Some(object) = JSObject::from_value(&prototype) {
            object.did_become_prototype(vm);
        } else if let Some(function) = function {
            let object: &JSObject = &function;
            object.did_become_prototype(vm);
        } else if !prototype.is_null() {
            // Conservative hardening.
            return;
        }

        // `hasMonoProto()` é sempre verdadeiro aqui.
        let structure = self.structure();
        let new_structure = Structure::change_prototype_transition(vm, &structure, prototype);
        self.set_structure(vm, &new_structure);

        // Quando algum objeto da cadeia pode interceptar acessos indexados: o protótipo que muda faz o realm
        // ter um bad time, e o objeto com indexados vira `SlowPutArrayStorage`.
        if !self.any_object_in_chain_may_intercept_indexed_accesses() {
            return;
        }

        if self.may_be_prototype() {
            if let Some(global_object) = self.structure().realm() {
                global_object.have_a_bad_time(vm);
            }
            return;
        }

        if !has_indexed_properties(self.cell.indexing_type()) || has_slow_put_array_storage(self.cell.indexing_type()) {
            return;
        }

        self.switch_to_slow_put_array_storage(vm);
    }

    /// `anyObjectInChainMayInterceptIndexedAccesses()`.
    pub fn any_object_in_chain_may_intercept_indexed_accesses(&self) -> bool {
        let mut current_structure = self.structure();
        loop {
            if current_structure.may_intercept_indexed_accesses() {
                return true;
            }

            match JSObject::from_value(&current_structure.stored_prototype()) {
                Some(prototype) => current_structure = prototype.structure(),
                None => return false,
            }
        }
    }

    /// `needsSlowPutIndexing()`: sem realm, só o ramo `anyObjectInChainMayInterceptIndexedAccesses`.
    pub fn needs_slow_put_indexing(&self) -> bool {
        self.any_object_in_chain_may_intercept_indexed_accesses()
    }

    /// `isStructureExtensible()`.
    pub fn is_structure_extensible(&self) -> bool {
        self.structure().is_structure_extensible()
    }

    /// `indexingShouldBeSparse()`.
    pub fn indexing_should_be_sparse(&self) -> bool {
        !self.is_structure_extensible()
            || self.structure().type_info().intercepts_get_own_property_slot_by_index_even_when_length_is_not_zero()
    }

    // Storage of named properties.

    /// `getDirect(PropertyOffset)`.
    pub fn get_direct(&self, offset: PropertyOffset) -> JSValue {
        validate_offset(offset);
        if is_inline_offset(offset) {
            return self.inline_storage.borrow()[offset as usize];
        }
        self.butterfly.borrow().out_of_line[out_of_line_index(offset)]
    }

    /// `putDirectOffset(vm, offset, value)`.
    pub fn put_direct_offset(&self, vm: &VM, offset: PropertyOffset, value: JSValue) {
        validate_offset(offset);
        // `structure->didReplaceProperty(offset)`: o C++ chama no caminho de substituição do
        // `putDirectInternal`; aqui o ponto único de escrita por offset cobre também o `delete`.
        self.structure().did_replace_property(vm, offset);
        if let Some(watch) = self.replacement_watch.borrow().as_ref() {
            if watch.offsets.contains(&offset) {
                watch.fired.set(true);
            }
        }
        if is_inline_offset(offset) {
            self.inline_storage.borrow_mut()[offset as usize] = value;
            return;
        }
        self.butterfly.borrow_mut().out_of_line[out_of_line_index(offset)] = value;
    }

    /// `allocateMoreOutOfLineStorage(vm, oldSize, newSize)`: as posições novas começam vazias.
    pub fn allocate_more_out_of_line_storage(&self, _vm: &VM, old_size: usize, new_size: usize) {
        debug_assert!(new_size > old_size);
        // O C++ aloca uma `Butterfly` nova inteira (`AuxiliarySpace`): o relógio de bytes do coletor conta o total.
        let (vector_length, has_indexing) = self.butterfly_shape();
        crate::runtime::heap_size::did_allocate_butterfly(new_size, vector_length, has_indexing);
        let mut butterfly = self.butterfly.borrow_mut();
        debug_assert!(butterfly.out_of_line.len() >= old_size.min(butterfly.out_of_line.len()));
        butterfly.out_of_line.resize(new_size, JSValue::empty());
    }

    /// `getDirectOffset(vm, propertyName)`.
    pub fn get_direct_offset(&self, vm: &VM, property_name: &PropertyName) -> PropertyOffset {
        let structure = self.structure();
        structure.get(vm, property_name)
    }

    /// `getDirectOffset(vm, propertyName, attributes)`.
    pub fn get_direct_offset_with_attributes(&self, vm: &VM, property_name: &PropertyName) -> (PropertyOffset, u32) {
        let structure = self.structure();
        structure.get_with_attributes(vm, property_name)
    }

    /// `getDirect(vm, propertyName)`: `JSValue()` (vazio) se não existir.
    pub fn get_direct_by_name(&self, vm: &VM, property_name: &PropertyName) -> JSValue {
        let offset = self.get_direct_offset(vm, property_name);
        if offset != INVALID_OFFSET {
            self.get_direct(offset)
        } else {
            JSValue::empty()
        }
    }

    /// `putDirect(vm, propertyName, value, attributes)`.
    pub fn put_direct(&self, vm: &VM, property_name: &PropertyName, value: JSValue, attributes: u32) -> bool {
        debug_assert!(property_name.parse_index().is_none());
        let mut slot = PutPropertySlot::new(self.as_value(), false, PutContext::UnknownContext, false);
        self.put_direct_internal(vm, property_name, value, attributes, &mut slot, PutMode::PutModeDefineOwnProperty)
            .is_none()
    }

    /// `putDirect(vm, propertyName, value, attributes, slot)`.
    pub fn put_direct_with_slot(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        attributes: u32,
        slot: &mut PutPropertySlot,
    ) -> bool {
        debug_assert!(property_name.parse_index().is_none());
        self.put_direct_internal(vm, property_name, value, attributes, slot, PutMode::PutModeDefineOwnProperty).is_none()
    }

    /// `putDirectInternal<mode>(vm, propertyName, value, attributes, slot)`: `None` é o `ASCIILiteral`
    /// nulo (sucesso); `Some(mensagem)` é o erro. Sem o `invalidateStructureChainIntegrity`.
    pub fn put_direct_internal(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        new_attributes: u32,
        slot: &mut PutPropertySlot,
        mode: PutMode,
    ) -> Option<&'static str> {
        debug_assert!(!value.is_empty());
        debug_assert!(GetterSetter::from_value(&value).is_some() == (new_attributes & ACCESSOR != 0));
        debug_assert!(property_name.parse_index().is_none());

        let structure = self.structure();
        if structure.is_dictionary() {
            debug_assert!(!is_copy_on_write(self.structure().indexing_mode()));
            if mode == PutMode::PutModePut && !self.is_structure_extensible() {
                return self.put_direct_to_dictionary_without_extensibility(vm, property_name, value, slot);
            }

            let old_out_of_line_capacity = structure.out_of_line_capacity() as usize;
            let (offset, attributes, is_added) =
                structure.add_or_replace_property_without_transition(vm, property_name, new_attributes);
            if is_added {
                // O callback do C++: cresce o armazenamento fora de linha quando o `newMaxOffset` pede.
                let new_out_of_line_capacity = structure.out_of_line_capacity() as usize;
                if new_out_of_line_capacity != old_out_of_line_capacity {
                    self.allocate_more_out_of_line_storage(vm, old_out_of_line_capacity, new_out_of_line_capacity);
                }
                // This assertion verifies that the concurrent GC won't read garbage if the concurrentGC
                // is running at the same time we put without transitioning.
                debug_assert!(self.get_direct(offset).is_empty());
            }

            if !is_added {
                if mode == PutMode::PutModePut && attributes & READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR != 0 {
                    return Some(READONLY_PROPERTY_CHANGE_ERROR);
                }

                self.put_direct_offset(vm, offset, value);

                // FIXME do C++: Check attributes against PropertyAttribute::CustomAccessorOrValue. Changing
                // GetterSetter should work w/o transition.
                if mode == PutMode::PutModeDefineOwnProperty
                    && (new_attributes != attributes || new_attributes & ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE != 0)
                {
                    let new_structure = Structure::attribute_change_transition(vm, &structure, property_name, new_attributes);
                    self.set_structure(vm, &new_structure);
                } else {
                    debug_assert!(attributes & ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE == 0);
                    slot.set_existing_property(self, offset);
                }
                return None;
            }

            validate_offset(offset);
            self.put_direct_offset(vm, offset, value);
            slot.set_new_property(self, offset);
            if attributes & READ_ONLY != 0 {
                self.structure().set_contains_read_only_properties();
            }
            return None;
        }

        if let Some((new_structure, offset)) =
            Structure::add_property_transition_to_existing_structure(&structure, property_name, new_attributes)
        {
            if structure.out_of_line_capacity() != new_structure.out_of_line_capacity() {
                debug_assert!(!Rc::ptr_eq(&new_structure, &self.structure()));
                self.allocate_more_out_of_line_storage(
                    vm,
                    structure.out_of_line_capacity() as usize,
                    new_structure.out_of_line_capacity() as usize,
                );
            }

            validate_offset(offset);
            debug_assert!(new_structure.is_valid_offset(offset));

            // This assertion verifies that the concurrent GC won't read garbage if the concurrentGC
            // is running at the same time we put without transitioning.
            debug_assert!(self.get_direct(offset).is_empty());
            self.put_direct_offset(vm, offset, value);
            self.set_structure(vm, &new_structure);
            slot.set_new_property(self, offset);
            return None;
        }

        let (offset, current_attributes) = structure.get_with_attributes(vm, property_name);
        if offset != INVALID_OFFSET {
            if mode == PutMode::PutModePut && current_attributes & READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR != 0 {
                return Some(READONLY_PROPERTY_CHANGE_ERROR);
            }

            self.put_direct_offset(vm, offset, value);

            // FIXME do C++: Check attributes against PropertyAttribute::CustomAccessorOrValue. Changing
            // GetterSetter should work w/o transition.
            if mode == PutMode::PutModeDefineOwnProperty
                && (new_attributes != current_attributes || new_attributes & ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE != 0)
            {
                let new_structure = Structure::attribute_change_transition(vm, &structure, property_name, new_attributes);
                self.set_structure(vm, &new_structure);
            } else {
                debug_assert!(current_attributes & ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE == 0);
                slot.set_existing_property(self, offset);
            }

            return None;
        }

        if mode == PutMode::PutModePut && !self.is_structure_extensible() {
            return Some(NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR);
        }

        let (new_structure, offset) =
            Structure::add_new_property_transition(vm, &structure, property_name, new_attributes, slot.context());

        validate_offset(offset);
        debug_assert!(new_structure.is_valid_offset(offset));
        let old_capacity = structure.out_of_line_capacity() as usize;
        let new_capacity = new_structure.out_of_line_capacity() as usize;
        debug_assert!(old_capacity <= new_capacity);
        if old_capacity != new_capacity {
            self.allocate_more_out_of_line_storage(vm, old_capacity, new_capacity);
        }

        debug_assert!(self.get_direct(offset).is_empty());
        self.put_direct_offset(vm, offset, value);
        self.set_structure(vm, &new_structure);
        slot.set_new_property(self, offset);
        if new_attributes & READ_ONLY != 0 {
            new_structure.set_contains_read_only_properties();
        }
        None
    }

    // Reading properties.

    /// `getOwnNonIndexPropertySlot(vm, structure, propertyName, slot)`: sem tabela estática de
    /// propriedades e sem `CustomGetterSetter`; valor de accessor (`GetterSetter`) vira um slot de getter.
    pub fn get_own_non_index_property_slot(
        &self,
        vm: &VM,
        structure: &StructureRef,
        property_name: &PropertyName,
        slot: &mut PropertySlot,
    ) -> bool {
        let (offset, attributes) = structure.get_with_attributes(vm, property_name);
        if !is_valid_offset(offset) {
            return false;
        }

        // getPropertySlot relies on this method never returning index properties!
        debug_assert!(property_name.parse_index().is_none());

        let value = self.get_direct(offset);
        if let Some(getter_setter) = GetterSetter::from_value(&value) {
            debug_assert!(attributes & ACCESSOR != 0);
            self.fill_getter_property_slot(slot, getter_setter, attributes, offset);
            return true;
        }
        if let Some(custom_getter_setter) = CustomGetterSetter::from_value(&value) {
            debug_assert!(attributes & CUSTOM_ACCESSOR_OR_VALUE != 0);
            self.fill_custom_getter_property_slot(slot, &custom_getter_setter, attributes, &structure);
            return true;
        }

        slot.set_value_at_offset(self, attributes, value, offset);
        true
    }

    /// `fillCustomGetterPropertySlot(slot, customGetterSetter, attributes, structure)` (sem
    /// `DOMAttributeGetterSetter`).
    pub fn fill_custom_getter_property_slot(
        &self,
        slot: &mut PropertySlot,
        custom_getter_setter: &CustomGetterSetter,
        attributes: u32,
        structure: &Structure,
    ) {
        debug_assert!(attributes & CUSTOM_ACCESSOR_OR_VALUE != 0);
        if structure.is_uncacheable_dictionary() {
            slot.set_custom(self, attributes, custom_getter_setter.getter(), custom_getter_setter.setter());
        } else {
            slot.set_cacheable_custom(self, attributes, custom_getter_setter.getter(), custom_getter_setter.setter());
        }
    }

    /// `fillGetterPropertySlot(vm, slot, getterSetter, attributes, offset)`.
    pub fn fill_getter_property_slot(
        &self,
        slot: &mut PropertySlot,
        getter_setter: GetterSetterRef,
        attributes: u32,
        offset: PropertyOffset,
    ) {
        if self.structure().is_uncacheable_dictionary() {
            slot.set_getter_slot(self, attributes, getter_setter);
            return;
        }

        // This access is cacheable because Structure requires an attributeChangedTransition
        // if this property stops being an accessor.
        slot.set_cacheable_getter_slot(self, attributes, getter_setter, offset);
    }

    /// `getOwnPropertySlotImpl` / `JSObject::getOwnPropertySlot(object, globalObject, propertyName, slot)`.
    pub fn get_own_property_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            return target_object.get_own_property_slot(vm, property_name, slot);
        }
        // `JSGenericTypedArrayView::getOwnPropertySlot`.
        if let Some(answer) = typed_array_dispatch::get_own_property_slot(self, vm, property_name, slot) {
            return answer;
        }
        // `ClonedArguments::getOwnPropertySlot` e `GenericArgumentsImpl::getOwnPropertySlot`: os especiais antes de materializarem.
        if let Some(answer) = crate::runtime::js_arguments_objects::special_own_slot(vm, self, property_name, slot) {
            return answer;
        }
        let mut structure = self.structure();
        // `ErrorInstance::getOwnPropertySlot`: a primeira leitura de `stack` materializa a pilha (que
        // acrescenta propriedades, então a `Structure` é lida de novo).
        if crate::runtime::error_instance::materialize_for_property(self, vm, property_name) {
            structure = self.structure();
        }
        // `JSArray::getOwnPropertySlot`: o `length` é virtual (vem do butterfly, não da `Structure`). A resposta de
        // `JSArray::get_own_property_slot` para esse nome não volta a este método, então não há laço.
        if matches!(self.type_(), JSType::ArrayType | JSType::DerivedArrayType) && *property_name == vm.property_names.length {
            if let Some(array) = crate::runtime::js_array::JSArray::from_cell_id_by_class(self.cell_id()) {
                return array.get_own_property_slot(vm, property_name, slot);
            }
        }
        // `RegExpObject::getOwnPropertySlot`: o `lastIndex` vive num campo do objeto, não na `Structure`.
        // A resposta de `RegExpObject::get_own_property_slot` para esse nome não volta a este método.
        if self.type_() == JSType::RegExpObjectType && *property_name == vm.property_names.last_index {
            if let Some(reg_exp) = crate::runtime::reg_exp_object::RegExpObject::from_cell_id(self.cell_id()) {
                return reg_exp.get_own_property_slot(vm, property_name, slot);
            }
        }
        if let Some(answer) = self.string_object_own_slot(vm, property_name, slot) {
            return answer;
        }
        // `JSModuleNamespaceObject::getOwnPropertySlotCommon`: o namespace adiado avalia o módulo antes de responder.
        if let Some(namespace) = js_module_namespace_object::exotic_of(self) {
            if !namespace.prepare_own_property_slot(vm, property_name, slot) {
                return false;
            }
        }
        if self.get_own_non_index_property_slot(vm, &structure, property_name, slot) {
            return true;
        }
        // `getOwnStaticPropertySlot`: só a checagem do flag no caminho comum.
        if self.has_non_reified_static_properties(&structure) && self.get_own_static_property_slot(vm, property_name, slot) {
            return true;
        }
        // `JSGlobalObject::getOwnPropertySlot`: `Base::getOwnPropertySlot` e depois `symbolTableGet`. As variáveis
        // `var` e funções do script global vivem na `SymbolTable` do global, não na `Structure`.
        // `JSGlobalLexicalEnvironment::getOwnPropertySlot` faz o mesmo com a tabela do registro léxico (let/const/class).
        if matches!(self.type_(), JSType::GlobalObjectType | JSType::GlobalLexicalEnvironmentType) {
            if let (Some(key), Some(scope)) = (property_name.uid(), crate::runtime::js_scope::JSScope::from_cell_id(self.cell_id())) {
                if let Some((value, attributes)) = scope.symbol_table_get(key) {
                    slot.set_value(self, attributes, value);
                    return true;
                }
            }
        }
        if let Some(index) = property_name.parse_index() {
            return self.get_own_property_slot_by_index(vm, index, slot);
        }
        false
    }

    /// `hasNonReifiedStaticProperties()`: a `Structure` tem o flag `HasStaticPropertyTable` e o bit
    /// `staticPropertiesReified` ainda está desligado.
    pub fn has_non_reified_static_properties(&self, structure: &StructureRef) -> bool {
        structure.type_info().has_static_property_table() && !structure.static_properties_reified()
    }

    /// `JSObject::getOwnStaticPropertySlot`: percorre `class_info` e os pais, e para na primeira tabela que
    /// responde (`getStaticPropertySlotFromTable` reifica o nome e preenche o slot).
    fn get_own_static_property_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        let mut class_info = Some(self.class_info());
        while let Some(current) = class_info {
            if let Some(table) = current.static_prop_hash_table {
                if crate::runtime::lookup::get_static_property_slot_from_table(vm, table, self, property_name, slot) {
                    return true;
                }
            }
            class_info = current.parent_class;
        }
        false
    }

    /// O trecho de `putInlineFastReplacingStaticPropertyIfNeeded` e do `defineOwnProperty`: com propriedades
    /// estáticas ainda não reificadas, o nome da tabela é reificado antes de a escrita ou a redefinição
    /// ler a `Structure`. Sem tabela em nenhum `ClassInfo`, não faz nada.
    fn reify_static_property_named(&self, vm: &VM, property_name: &PropertyName) {
        if self.has_non_reified_static_properties(&self.structure()) {
            let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
            self.get_own_static_property_slot(vm, property_name, &mut slot);
        }
    }

    /// A entrada `CustomAccessor` de uma tabela estática que a `Structure` ainda não tem (a leitura não a reifica:
    /// `getStaticPropertySlotFromTable` só preenche o slot).
    fn non_reified_custom_accessor_entry(&self, vm: &VM, property_name: &PropertyName) -> Option<&'static crate::runtime::lookup::HashTableValue> {
        let structure = self.structure();
        if !self.has_non_reified_static_properties(&structure) {
            return None;
        }
        let uid = property_name.public_name()?;
        let key = crate::runtime::identifier::Identifier::from_uid(vm, Some(uid)).utf8();
        let key = std::str::from_utf8(&key).ok()?;
        if is_valid_offset(structure.get_with_attributes(vm, property_name).0) {
            return None;
        }
        let mut class_info = Some(self.class_info());
        while let Some(current) = class_info {
            if let Some(entry) = current.static_prop_hash_table.and_then(|table| table.entry(key)) {
                return matches!(entry.kind, crate::runtime::lookup::Kind::CustomAccessor { .. }).then_some(entry);
            }
            class_info = current.parent_class;
        }
        None
    }

    /// O `putInlineFastReplacingStaticPropertyIfNeeded` para a entrada `CustomAccessor` ainda fora da `Structure`:
    /// `ReadOnly` lança, senão o setter da tabela roda (sem setter, a escrita falha em silêncio). `None` quando o
    /// nome não é uma dessas entradas.
    fn put_non_reified_custom_accessor(&self, vm: &VM, property_name: &PropertyName, value: JSValue, strict: bool) -> Option<Result<bool, PutError>> {
        let entry = self.non_reified_custom_accessor_entry(vm, property_name)?;
        let crate::runtime::lookup::Kind::CustomAccessor { setter, .. } = entry.kind else {
            return None;
        };
        if entry.attributes & READ_ONLY != 0 {
            return Some(type_error(strict, READONLY_PROPERTY_WRITE_ERROR));
        }
        let Some(setter) = setter else {
            return Some(Ok(false));
        };
        let realm = self.structure().realm().expect("objeto sem realm na Structure");
        Some(Ok(setter(&realm, self.as_value().encode(), value.encode(), property_name)))
    }

    /// `JSObject::reifyAllStaticProperties`: converte para dicionário, reifica na ordem da tabela (filho primeiro,
    /// depois os pais) cada entrada que a `Structure` ainda não tem e liga `staticPropertiesReified`. Sem tabela
    /// em nenhum `ClassInfo` só liga o bit.
    pub fn reify_all_static_properties(&self, vm: &VM) {
        let structure = self.structure();
        if structure.static_properties_reified() {
            return;
        }
        if !structure.type_info().has_static_property_table() {
            structure.set_static_properties_reified(true);
            return;
        }
        if !structure.is_dictionary() {
            self.convert_to_dictionary(vm);
        }
        let mut class_info = Some(self.class_info());
        while let Some(current) = class_info {
            if let Some(table) = current.static_prop_hash_table {
                for entry in table.iter() {
                    let identifier = crate::runtime::identifier::Identifier::from_span(vm, entry.key.as_bytes());
                    let name = PropertyName::from_identifier(&identifier);
                    if !is_valid_offset(self.structure().get_with_attributes(vm, &name).0) {
                        crate::runtime::lookup::reify_static_property(vm, self, entry);
                    }
                }
            }
            class_info = current.parent_class;
        }
        self.structure().set_static_properties_reified(true);
    }

    /// O trecho de `JSObject::deleteProperty` das propriedades estáticas: `Some(false)` quando o nome está na
    /// tabela como `DontDelete` (nada é reificado); senão reifica tudo e devolve `None`.
    fn reify_before_delete(&self, vm: &VM, property_name: &PropertyName) -> Option<bool> {
        let structure = self.structure();
        if !self.has_non_reified_static_properties(&structure) {
            return None;
        }
        let uid = property_name.public_name()?;
        let key = crate::runtime::identifier::Identifier::from_uid(vm, Some(uid)).utf8();
        let key = std::str::from_utf8(&key).ok()?;
        let mut class_info = Some(self.class_info());
        while let Some(current) = class_info {
            if let Some(entry) = current.static_prop_hash_table.and_then(|table| table.entry(key)) {
                if entry.attributes & DONT_DELETE != 0 {
                    return Some(false);
                }
                self.reify_all_static_properties(vm);
                return None;
            }
            class_info = current.parent_class;
        }
        None
    }

    /// O `StringObject` desta célula (`is<StringObject>`), quando é um (inclui `DerivedStringObject`).
    fn string_object_of(&self) -> Option<crate::runtime::string_object::StringObjectRef> {
        if !matches!(self.type_(), JSType::StringObjectType | JSType::DerivedStringObjectType) {
            return None;
        }
        crate::runtime::string_object::StringObject::from_cell_id(self.cell_id())
    }

    /// `StringObject::getOwnPropertySlot` para `length` e os caracteres (`isStringOwnProperty`); o resto cai
    /// na base. `StringObject::get_own_property_slot` responde sem voltar aqui para esses nomes.
    fn string_object_own_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> Option<bool> {
        if !matches!(self.type_(), JSType::StringObjectType | JSType::DerivedStringObjectType) {
            return None;
        }
        let string_object = crate::runtime::string_object::StringObject::from_cell_id(self.cell_id())?;
        if string_object.is_string_own_property(vm, property_name) {
            return Some(string_object.get_own_property_slot(vm, property_name, slot));
        }
        None
    }

    /// `JSObject::getOwnPropertySlotByIndex(thisObject, globalObject, i, slot)`.
    pub fn get_own_property_slot_by_index(&self, vm: &VM, i: u32, slot: &mut PropertySlot) -> bool {
        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            return target_object.get_own_property_slot_by_index(vm, i, slot);
        }
        // `JSGenericTypedArrayView::getOwnPropertySlotByIndex`.
        if let Some(answer) = typed_array_dispatch::get_own_property_slot_by_index(self, i, slot) {
            return answer;
        }
        if let Some(arguments) = generic_arguments::exotic_of(self) {
            return generic_arguments::get_own_property_slot_by_index(&*arguments, vm, i, slot);
        }
        // O nome só nasce para `StringObject`: atomizar o índice a cada objeto da cadeia, em cada buraco, custava
        // um acesso à tabela de átomos por elemento (a leitura de buracos de um array grande ficava lenta).
        if i <= MAX_ARRAY_INDEX && matches!(self.type_(), JSType::StringObjectType | JSType::DerivedStringObjectType) {
            let index_name = PropertyName::from_identifier(&Identifier::from_u32(vm, i));
            if let Some(answer) = self.string_object_own_slot(vm, &index_name, slot) {
                return answer;
            }
        }
        if let Some(namespace) = js_module_namespace_object::exotic_of(self) {
            if namespace.get_own_property_slot_by_index(i, slot) {
                return true;
            }
        }
        // NB. The fact that we're directly consulting our indexed storage implies that it is not legal for
        // anyone to override getOwnPropertySlot() without also overriding getOwnPropertySlotByIndex().
        if i > MAX_ARRAY_INDEX {
            let property_name = PropertyName::from_identifier(&Identifier::from_u32(vm, i));
            return self.get_own_property_slot(vm, &property_name, slot);
        }

        match self.shape() {
            NO_INDEXING_SHAPE | UNDECIDED_SHAPE => false,
            INT32_SHAPE | CONTIGUOUS_SHAPE => {
                let value = {
                    let butterfly = self.butterfly.borrow();
                    match &butterfly.indexed {
                        IndexedStorage::Values(values) => match values.get(i as usize) {
                            Some(value) => *value,
                            None => return false,
                        },
                        _ => return false,
                    }
                };
                if !value.is_empty() {
                    slot.set_value(self, 0, value);
                    return true;
                }
                false
            }
            DOUBLE_SHAPE => {
                let value = {
                    let butterfly = self.butterfly.borrow();
                    match &butterfly.indexed {
                        IndexedStorage::Doubles(doubles) => match doubles.get(i as usize) {
                            Some(value) => *value,
                            None => return false,
                        },
                        _ => return false,
                    }
                };
                if value == value {
                    slot.set_value(self, 0, JSValue::double_number(value));
                    return true;
                }
                false
            }
            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                let butterfly = self.butterfly.borrow();
                let IndexedStorage::ArrayStorage(storage) = &butterfly.indexed else {
                    unreachable!("forma ArrayStorage sem ArrayStorage")
                };
                if i >= storage.length() {
                    return false;
                }
                if (i as usize) < storage.vector().len() {
                    let value = storage.vector()[i as usize];
                    if !value.is_empty() {
                        slot.set_value(self, 0, value);
                        return true;
                    }
                } else if let Some(entry) = storage.sparse_map().and_then(|map| map.find(i)) {
                    entry.get_slot(self, slot);
                    return true;
                }
                false
            }
            shape => unreachable!("getOwnPropertySlotByIndex em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// `getPropertySlot(globalObject, propertyName, slot)` (JSObject.h, o template com
    /// `checkNullStructure = false`).
    pub fn get_property_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        // Nome de índice: o laço abaixo só lê nomes que não são índice, e um protótipo com
        // `OverridesGetOwnPropertySlot` (Array) responderia pelo índice dele antes do armazenamento
        // indexado do próprio objeto (`Object.create([1,2,3])` com `o[1] = 5` lia 2).
        if let Some(index) = property_name.parse_index() {
            return self.get_property_slot_by_index(vm, index, slot);
        }
        let mut current: Option<JSObjectHandle> = None;
        loop {
            let object = chain_object(self, &current);
            // O `Proxy` (`OverridesGetOwnPropertySlot`) responde pelos traps, a partir dele. O nome privado não passa
            // pelos traps: mora no armazenamento do próprio `Proxy`, como os campos `#privados` do JSC.
            if object.type_() == JSType::ProxyObjectType && !property_name.is_private_name() {
                return get_property_slot_from_proxy(object, property_name, slot);
            }
            // Fora o `JSGlobalProxy` (e os tipos com a flag, despachados abaixo), nenhum tipo sobrescreve
            // `getOwnPropertySlot`. O `JSGlobalProxy`
            // repassa ao alvo (o `get_own_property_slot` dele já cobre o nome de índice), e os
            // `TypedArray`: um nome de índice recomeça de `self` pelos acessos por índice (este laço só lê
            // nomes que não são índice) e um nome numérico canônico que a visão responde `false` segue
            // para o protótipo sem consultar a `Structure`.
            // `JSFunction::getOwnPropertySlot`: um elo da cadeia que é função materializa `name`/`length`
            // preguiçosos antes da leitura (`Object.create(B).name`, com `B` uma classe, lê o `name` de `B` e
            // não o de `Function.prototype`).
            let structure = object.structure();
            // O `JSFunction::getOwnPropertySlot` inteiro (também o `prototype` preguiçoso), pelo `methodTable`.
            let function = if object.type_() == JSType::JSFunctionType { JSValue::Cell(object.cell_id()).as_js_function() } else { None };
            if let Some(function) = function {
                if function.get_own_property_slot(&function.realm(), property_name, slot) {
                    return true;
                }
                if vm.exception().is_some() {
                    return false;
                }
            } else if is_typed_view(object.type_()) {
                if let Some(index) = property_name.parse_index() {
                    return self.get_property_slot_by_index(vm, index, slot);
                }
                match typed_array_dispatch::get_own_property_slot(object, vm, property_name, slot) {
                    Some(true) => return true,
                    // `[[HasProperty]]`/`[[Get]]` do objeto exótico de índice inteiro: o nome numérico canônico fora do alcance
                    // (`"1.5"`, `"-0"`, `"NaN"`) responde `false` sem seguir para o protótipo (medido no bun 1.4.2).
                    Some(false) => {
                        if crate::runtime::js_generic_typed_array_view::is_canonical_numeric_index_string(vm, property_name).is_some() {
                            return false;
                        }
                    }
                    None => {
                        if object.get_own_non_index_property_slot(vm, &structure, property_name, slot) {
                            return true;
                        }
                    }
                }
            } else {
                let found = match js_global_proxy::target_of(object) {
                    Some(target) => {
                        let target_object: &JSObject = &target;
                        target_object.get_own_property_slot(vm, property_name, slot)
                    }
                    None if matches!(
                        object.type_(),
                        JSType::RegExpObjectType
                            | JSType::StringObjectType
                            | JSType::DerivedStringObjectType
                            | JSType::DirectArgumentsType
                            | JSType::ScopedArgumentsType
                            | JSType::ClonedArgumentsType
                    ) || TypeInfo::overrides_get_own_property_slot_of(object.cell.inline_type_flags())
                        || object.has_non_reified_static_properties(&structure) =>
                    {
                        // Os tipos que ligam `OverridesGetOwnPropertySlot` (`ErrorInstance`, ambientes
                        // léxicos, global, array, `String`, `RegExp`...) e os objetos com propriedades
                        // estáticas ainda não reificadas (`getOwnStaticPropertySlot` dentro de
                        // `JSObject::getOwnPropertySlot`) passam pelo despacho central
                        // `get_own_property_slot`, o mesmo que o acesso público usa, em vez da leitura crua
                        // da `Structure` (que perderia, por exemplo, a materialização de `stack`).
                        object.get_own_property_slot(vm, property_name, slot)
                    }
                    None => object.get_own_non_index_property_slot(vm, &structure, property_name, slot),
                };
                if found {
                    return true;
                }
            }

            // FIXME do C++: This doesn't look like it's following the specification:
            // https://bugs.webkit.org/show_bug.cgi?id=172572
            let prototype = structure.stored_prototype();
            match JSObject::from_value(&prototype) {
                Some(next) => current = Some(next),
                None => break,
            }
        }
        // A cadeia inteira falhou: o acesso nomeado da janela, se o hospedeiro instalou um neste global.
        crate::runtime::global_named_lookup::lookup(self, property_name, slot)
    }

    /// `getPropertySlot(globalObject, unsigned propertyName, slot)`.
    pub fn get_property_slot_by_index(&self, vm: &VM, property_name: u32, slot: &mut PropertySlot) -> bool {
        let mut current: Option<JSObjectHandle> = None;
        loop {
            let object = chain_object(self, &current);
            if object.type_() == JSType::ProxyObjectType {
                let property_name = PropertyName::from_identifier(&Identifier::from_u32(vm, property_name));
                return get_property_slot_from_proxy(object, &property_name, slot);
            }
            let has_slot = object.get_own_property_slot_by_index(vm, property_name, slot);
            if has_slot {
                return true;
            }
            if slot.is_vm_inquiry() && slot.is_tainted_by_opaque_object() {
                return false;
            }
            // Visão tipada: o índice fora do alcance não segue para o protótipo (`4 in typedArray`, o `for-in` e o
            // `[[Get]]` com um `4` no protótipo; medido no bun 1.4.2).
            if is_typed_view(object.type_()) {
                return false;
            }
            let structure = object.structure();
            // O `getPrototype` sobrescrito é o do `Proxy` (já tratado acima) e o do `JSGlobalProxy`, cujo
            // protótipo direto é o do alvo (`setTarget`).
            debug_assert!(!structure.type_info().overrides_get_prototype() || object.type_() == JSType::GlobalProxyType);
            let prototype = object.get_prototype_direct();
            match JSObject::from_value(&prototype) {
                Some(next) => current = Some(next),
                None => return false,
            }
        }
    }

    /// `get(globalObject, propertyName)`: `undefined` se a propriedade não existe.
    pub fn get(&self, vm: &VM, property_name: &PropertyName) -> JSValue {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::Get);
        if self.get_property_slot(vm, property_name, &mut slot) {
            return slot.get_value_for(property_name);
        }
        JSValue::undefined()
    }

    /// `get(globalObject, unsigned propertyName)`.
    pub fn get_by_index(&self, vm: &VM, property_name: u32) -> JSValue {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::Get);
        if self.get_property_slot_by_index(vm, property_name, &mut slot) {
            return slot.get_value_for_index(vm, property_name);
        }
        JSValue::undefined()
    }

    /// `hasProperty(globalObject, propertyName)`.
    pub fn has_property(&self, vm: &VM, property_name: &PropertyName) -> bool {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::HasProperty);
        self.get_property_slot(vm, property_name, &mut slot)
    }

    /// `hasProperty(globalObject, unsigned propertyName)`.
    pub fn has_property_by_index(&self, vm: &VM, property_name: u32) -> bool {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::HasProperty);
        self.get_property_slot_by_index(vm, property_name, &mut slot)
    }

    /// `hasOwnProperty(globalObject, propertyName)`.
    pub fn has_own_property(&self, vm: &VM, property_name: &PropertyName) -> bool {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
        self.get_own_property_slot(vm, property_name, &mut slot)
    }

    /// `hasOwnProperty(globalObject, unsigned propertyName)`.
    pub fn has_own_property_by_index(&self, vm: &VM, property_name: u32) -> bool {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
        self.get_own_property_slot_by_index(vm, property_name, &mut slot)
    }

    // Writing properties.

    /// `isThisValueAltered(slot, baseObject)` (`JSGlobalProxy.h`): o `this` do slot é o próprio objeto ou o
    /// `JSGlobalProxy` dele; qualquer outro objeto, ou primitivo, é "alterado".
    fn is_this_value_altered(slot: &PutPropertySlot, base_object: &JSObject) -> bool {
        js_global_proxy::is_this_value_altered(slot.this_value(), base_object)
    }

    /// `canPerformFastPutInlineExcludingProto()`.
    fn can_perform_fast_put_inline_excluding_proto(&self) -> bool {
        // Check if there are any setters or getters in the prototype chain.
        let mut current: Option<JSObjectHandle> = None;
        loop {
            let obj = chain_object(self, &current);
            let structure = obj.structure();
            if structure.has_read_only_or_getter_setter_properties_excluding_proto()
                || structure.type_info().overrides_get_prototype()
            {
                return false;
            }
            if !std::ptr::eq(obj, self) && structure.type_info().overrides_put() {
                return false;
            }

            let prototype = obj.get_prototype_direct();
            match JSObject::from_value(&prototype) {
                Some(next) => current = Some(next),
                None => {
                    debug_assert!(prototype.is_null());
                    return true;
                }
            }
        }
    }

    /// `canPerformFastPutInline(vm, propertyName)`.
    fn can_perform_fast_put_inline(&self, vm: &VM, property_name: &PropertyName) -> bool {
        if *property_name == vm.property_names.underscore_proto {
            return false;
        }
        self.can_perform_fast_put_inline_excluding_proto()
    }

    /// `JSObject::put(cell, globalObject, propertyName, value, slot)`, o `putInlineForJSObject`
    /// (https://tc39.es/ecma262/#sec-ordinaryset).
    pub fn put(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Result<bool, PutError> {
        debug_assert!(!value.is_empty());

        // Nome privado (o `@nome` dos builtins, o símbolo do `createPrivateSymbol`): a escrita direta no próprio
        // objeto, sem cadeia, sem armadilha de `Proxy` e sem olhar a extensibilidade, como o `putDirect` que o JSC
        // usa para nomes privados.
        if property_name.is_private_name() {
            self.put_direct(vm, property_name, value, 0);
            return Ok(true);
        }

        // `ProxyObject::put` (virtual no C++): o próprio `Proxy` como receptor responde pelo trap `set`.
        // O `perform_put` sem trap escreve no alvo (`object_set`), nunca de volta no `Proxy`.
        if self.type_() == JSType::ProxyObjectType {
            return put_from_proxy(self, property_name, value, slot);
        }

        // `WebAssemblyGCObjectBase::put`: o objeto Wasm GC lança em qualquer escrita.
        if let Some(result) = crate::runtime::js_web_assembly_gc_object::put(self) {
            return result;
        }

        // `JSGlobalProxy::put`: o alvo responde (`target->methodTable()->put`, o `JSGlobalObject::put` que consulta a
        // `SymbolTable` antes do `JSObject::put`), com o `this` do slot intacto.
        if let Some(target) = js_global_proxy::target_of(self) {
            return crate::runtime::js_global_object::JSGlobalObject::put(&target, vm, property_name, value, slot);
        }
        // `JSModuleNamespaceObject::put`: nunca grava, lança em modo estrito.
        if let Some(namespace) = js_module_namespace_object::exotic_of(self) {
            return namespace.put(slot.is_strict_mode());
        }
        // `process.env`: o valor vira texto, a chave '' some e a chave símbolo lança.
        let value = if crate::runtime::process_env::is_env(self) {
            match crate::runtime::process_env::before_put(vm, Some(property_name), value)? {
                Some(text) => text,
                None => return Ok(true),
            }
        } else {
            value
        };

        // `ClonedArguments::put` e `GenericArgumentsImpl::put`: os especiais materializam antes.
        crate::runtime::js_arguments_objects::materialize_specials_for_property(vm, self, property_name)?;
        // `ErrorInstance::put`: escrever `stack`, `line` ou `column` materializa a pilha antes.
        if crate::runtime::error_instance::materialize_for_property(self, vm, property_name) && vm.exception().is_some() {
            return Err(PutError::Pending);
        }

        // `ErrorConstructor::put`: `Error.stackTraceLimit = n` atualiza o espelho do global antes do `Base::put`.
        crate::runtime::error_natives::error_constructor_put(self, vm, property_name, value);

        // `RegExpObject::put`: o `lastIndex` vive num campo do objeto, não na `Structure`.
        if self.type_() == JSType::RegExpObjectType && *property_name == vm.property_names.last_index {
            if let Some(reg_exp) = crate::runtime::reg_exp_object::RegExpObject::from_cell_id(self.cell_id()) {
                if let Some(result) = reg_exp.put(vm, property_name, value, slot) {
                    return result;
                }
            }
        }

        // `JSGenericTypedArrayView::put` (os nomes de índice e os numéricos canônicos).
        if let Some(result) =
            typed_array_dispatch::put(self, property_name, value, slot.this_value(), slot.is_strict_mode())
        {
            return result;
        }

        // `StringObject::put`: `length` é somente leitura; com o `this` do slot alterado cai no
        // `JSObject::put`, o resto dos índices passa por `putByIndex` (que protege os caracteres).
        if self.string_object_of().is_some() && *property_name == vm.property_names.length {
            return type_error(slot.is_strict_mode(), READONLY_PROPERTY_WRITE_ERROR);
        }

        // Try indexed put first. This is required for correctness, since loads on property names that
        // appear like valid indices will never look in the named property storage.
        if let Some(index) = property_name.parse_index() {
            if JSObject::is_this_value_altered(slot, self) {
                let realm = self.structure().realm().expect("objeto sem realm na Structure");
                return ordinary_set_slow(&realm, self, property_name, value, slot.this_value(), slot.is_strict_mode())
                    .map_err(|thrown| put_error_from_thrown(&realm, thrown));
            }
            return self.put_by_index(vm, index, value, slot.is_strict_mode());
        }

        // `putInlineFastReplacingStaticPropertyIfNeeded`: o nome da tabela estática é reificado antes.
        if let Some(result) = self.put_non_reified_custom_accessor(vm, property_name, value, slot.is_strict_mode()) {
            return result;
        }
        self.reify_static_property_named(vm, property_name);
        if !self.can_perform_fast_put_inline(vm, property_name) {
            return self.put_inline_slow(vm, property_name, value, slot);
        }
        if JSObject::is_this_value_altered(slot, self) {
            return self.define_property_on_receiver(vm, property_name, value, slot);
        }
        self.put_inline_fast(vm, property_name, value, slot)
    }

    /// `putInlineFast(globalObject, propertyName, value, slot)`.
    pub fn put_inline_fast(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Result<bool, PutError> {
        let error = self.put_direct_internal(vm, property_name, value, 0, slot, PutMode::PutModePut);
        if let Some(error) = error {
            return type_error(slot.is_strict_mode(), error);
        }
        Ok(true)
    }

    /// `putInlineSlow(globalObject, propertyName, value, slot)`: sem propriedades estáticas, accessors
    /// nem custom.
    pub fn put_inline_slow(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Result<bool, PutError> {
        debug_assert!(property_name.parse_index().is_none());

        if !vm.is_safe_to_recurse() {
            return Err(PutError::StackOverflow);
        }

        let mut current: Option<JSObjectHandle> = None;
        loop {
            let obj = chain_object(self, &current);
            let structure = obj.structure();
            if !std::ptr::eq(obj, self) && structure.type_info().overrides_put() {
                if obj.type_() == JSType::ProxyObjectType {
                    return put_from_proxy(obj, property_name, value, slot);
                }
                if let Some(target) = js_global_proxy::target_of(obj) {
                    return crate::runtime::js_global_object::JSGlobalObject::put(&target, vm, property_name, value, slot);
                }
                // `obj->methodTable()->put(obj, globalObject, propertyName, value, slot)` de um `TypedArray`:
                // o nome que a visão não trata é do `Base::put`, o `put` comum de `obj`.
                if is_typed_view(obj.type_()) {
                    return typed_array_dispatch::put(obj, property_name, value, slot.this_value(), slot.is_strict_mode())
                        .unwrap_or_else(|| obj.put(vm, property_name, value, slot));
                }
                // `obj->methodTable()->put(obj, globalObject, propertyName, value, slot)`: o `JSArray::put` para
                // um Array (`length` por `setLength`); JSFunction, ErrorInstance e os demais tipos que ligam
                // OverridesPut respondem pelo `put` comum de `obj`, com o `this` do slot intacto.
                return crate::runtime::js_array::put_through_method_table(vm, obj, property_name, value, slot).map_err(
                    |error| match error {
                        crate::runtime::js_array::ArrayError::Put(error) => error,
                        crate::runtime::js_array::ArrayError::RangeError(message) => PutError::RangeError(message),
                    },
                );
            }

            let (offset, attributes) = structure.get_with_attributes(vm, property_name);
            if is_valid_offset(offset) {
                let custom_setter = if attributes & CUSTOM_ACCESSOR_OR_VALUE != 0 {
                    let Some(custom) = CustomGetterSetter::from_value(&obj.get_direct(offset)) else {
                        unreachable!("propriedade com o atributo Custom cujo valor não é um CustomGetterSetter");
                    };
                    custom.setter()
                } else {
                    None
                };
                if attributes & READ_ONLY != 0 {
                    return type_error(slot.is_strict_mode(), READONLY_PROPERTY_WRITE_ERROR);
                }
                if attributes & ACCESSOR != 0 {
                    debug_assert!(is_valid_offset(offset));
                    // We need to make sure that we decide to cache this property before we potentially
                    // execute arbitrary JS.
                    if !self.structure().is_uncacheable_dictionary() {
                        slot.set_cacheable_setter(obj, offset);
                    }
                    let Some(getter_setter) = GetterSetter::from_value(&obj.get_direct(offset)) else {
                        unreachable!("propriedade com o atributo Accessor cujo valor não é um GetterSetter");
                    };
                    return getter_setter.call_setter(slot.this_value(), value, slot.is_strict_mode());
                }
                if attributes & CUSTOM_ACCESSOR != 0 {
                    // FIXME do C++: Remove this after WebIDL generator is fixed to set ReadOnly for
                    // [RuntimeConditionallyReadWrite] attributes.
                    let Some(custom_setter) = custom_setter else {
                        return Ok(false);
                    };
                    slot.set_custom_accessor(obj, custom_setter);
                    let realm = structure.realm().expect("CustomAccessor em objeto sem realm na Structure");
                    custom_setter(&realm, slot.this_value().encode(), value.encode(), property_name);
                    return Ok(true);
                }
                if attributes & CUSTOM_VALUE != 0 && !JSObject::is_this_value_altered(slot, obj) {
                    if let Some(custom_setter) = custom_setter {
                        slot.set_custom_value(obj, custom_setter);
                        let realm = structure.realm().expect("CustomValue em objeto sem realm na Structure");
                        return Ok(custom_setter(&realm, obj.as_value().encode(), value.encode(), property_name));
                    }
                    // Avoid PutModePut because it fails for non-extensible structures.
                    obj.put_direct_with_slot(
                        vm,
                        property_name,
                        value,
                        attributes_for_structure(attributes) & !CUSTOM_VALUE,
                        slot,
                    );
                    return Ok(true);
                }
                // If there's an existing writable property on the base object, or on one of its
                // prototypes, we should attempt to store the property on the receiver.
                break;
            }

            // `else if (structure->hasNonReifiedStaticProperties())`: a entrada `CustomAccessor` da tabela
            // estática de um objeto da cadeia (o protótipo do DataView, por exemplo) que a `Structure` ainda
            // não tem. `ReadOnly` lança; sem setter a escrita falha em silêncio; senão o setter roda com o
            // `this` do slot.
            if let Some(entry) = obj.non_reified_custom_accessor_entry(vm, property_name) {
                if entry.attributes & READ_ONLY != 0 {
                    return type_error(slot.is_strict_mode(), READONLY_PROPERTY_WRITE_ERROR);
                }
                let crate::runtime::lookup::Kind::CustomAccessor { setter, .. } = entry.kind else {
                    unreachable!("non_reified_custom_accessor_entry devolve só CustomAccessor");
                };
                let Some(custom_setter) = setter else {
                    return Ok(false);
                };
                slot.set_custom_accessor(obj, custom_setter);
                let realm = structure.realm().expect("CustomAccessor em objeto sem realm na Structure");
                custom_setter(&realm, slot.this_value().encode(), value.encode(), property_name);
                return Ok(true);
            }

            // `obj->getPrototype(globalObject)`: o `this` pode ser um `JSGlobalProxy` (sobrescreve
            // `getPrototype` mas é o primeiro da cadeia, logo não saiu pelo `overridesPut`).
            let prototype = if structure.type_info().overrides_get_prototype() {
                let global_object = structure.realm().expect("objeto sem realm na Structure");
                match obj.get_prototype(&global_object) {
                    Ok(prototype) => prototype,
                    Err(thrown) => return Err(put_error_from_thrown(&global_object, thrown)),
                }
            } else {
                obj.get_prototype_direct()
            };
            match JSObject::from_value(&prototype) {
                Some(next) => current = Some(next),
                None => {
                    debug_assert!(prototype.is_null());
                    break;
                }
            }
        }

        if JSObject::is_this_value_altered(slot, self) {
            return self.define_property_on_receiver(vm, property_name, value, slot);
        }
        self.put_inline_fast(vm, property_name, value, slot)
    }

    /// `definePropertyOnReceiver(globalObject, propertyName, value, slot)` (https://tc39.es/ecma262/#sec-ordinaryset,
    /// passo 3): sem `defineOwnProperty` sobrescrito nem propriedade custom. O receptor `JSGlobalProxy`
    /// (`globalThis = v` com um protótipo exótico na cadeia do global) é trocado pelo alvo, como o
    /// `if (receiver->type() == GlobalProxyType) receiver = target()` do C++: o proxy não tem propriedades
    /// próprias, a escrita tem de cair no `JSGlobalObject`.
    pub fn define_property_on_receiver(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Result<bool, PutError> {
        debug_assert!(property_name.parse_index().is_none());

        let Some(receiver) = JSObject::from_value(&slot.this_value()) else {
            // FIXME do C++: For a failure due to primitive receiver, the error message is misleading.
            return type_error(slot.is_strict_mode(), READONLY_PROPERTY_WRITE_ERROR);
        };
        if let Some(target) = js_global_proxy::target_of(&receiver) {
            let target_object: &JSObject = &target;
            return target_object.put_inline_fast(vm, property_name, value, slot);
        }

        // `receiver->methodTable()->defineOwnProperty != JSObject::defineOwnProperty` do C++: um receptor exótico (o namespace de
        // módulo, que o `super[chave] = v` de um construtor que devolveu o namespace usa) tem o `[[DefineOwnProperty]]` próprio, então
        // vale o caminho da especificação (`OrdinarySetWithOwnDescriptor`): o descritor próprio decide e só o valor é definido.
        if js_module_namespace_object::exotic_of(&receiver).is_some() {
            let mut existing = PropertyDescriptor::default();
            if receiver.get_own_property_descriptor(vm, property_name, &mut existing) {
                if existing.is_accessor_descriptor() || (existing.writable_present() && !existing.writable()) {
                    return type_error(slot.is_strict_mode(), READONLY_PROPERTY_WRITE_ERROR);
                }
                let mut value_descriptor = PropertyDescriptor::default();
                value_descriptor.set_value(value);
                return receiver.define_own_property(vm, property_name, &value_descriptor, slot.is_strict_mode());
            }
            return receiver.define_own_property(vm, property_name, &PropertyDescriptor::new(value, 0), slot.is_strict_mode());
        }

        debug_assert!(!slot.is_tainted_by_opaque_object());
        receiver.put_inline_fast(vm, property_name, value, slot)
    }

    // Dictionary mode.

    /// `convertToDictionary(vm)`.
    pub fn convert_to_dictionary(&self, vm: &VM) {
        let old_structure = self.structure();
        let new_structure = Structure::to_cacheable_dictionary_transition(vm, &old_structure);
        self.set_structure(vm, &new_structure);
    }

    /// `convertToUncacheableDictionary(vm)`.
    pub fn convert_to_uncacheable_dictionary(&self, vm: &VM) {
        let old_structure = self.structure();
        if old_structure.is_uncacheable_dictionary() {
            return;
        }
        let new_structure = Structure::to_uncacheable_dictionary_transition(vm, &old_structure);
        self.set_structure(vm, &new_structure);
    }

    /// `flattenDictionaryObject(vm)`.
    pub fn flatten_dictionary_object(&self, vm: &VM) {
        Structure::flatten_dictionary_structure(&self.structure(), vm, self);
    }

    /// A parte do `flattenDictionaryStructure` que zera o espaço de propriedades que deixou de ser usado
    /// (`gcSafeZeroMemory` do armazenamento inline e do fora de linha), a partir de `inline_size` e
    /// `out_of_line_size` posições em uso.
    pub(crate) fn clear_unused_property_storage(&self, inline_size: usize, out_of_line_size: usize) {
        for value in self.inline_storage.borrow_mut().iter_mut().skip(inline_size) {
            *value = JSValue::empty();
        }
        for value in self.butterfly.borrow_mut().out_of_line.iter_mut().skip(out_of_line_size) {
            *value = JSValue::empty();
        }
    }

    /// O `shiftButterflyAfterFlattening`/`setButterfly(nullptr)` do `flattenDictionaryStructure`: o
    /// armazenamento fora de linha passa a ter `new_capacity` posições (as primeiras, que são as dos
    /// primeiros offsets).
    pub(crate) fn shrink_out_of_line_storage(&self, new_capacity: usize) {
        let mut butterfly = self.butterfly.borrow_mut();
        debug_assert!(new_capacity <= butterfly.out_of_line.len());
        butterfly.out_of_line.truncate(new_capacity);
    }

    /// `prepareToPutDirectWithoutTransition(vm, propertyName, attributes, structureID, structure)`:
    /// acrescenta a propriedade à própria estrutura (sem transição), cresce o armazenamento fora de
    /// linha se preciso e devolve o offset.
    pub(crate) fn prepare_to_put_direct_without_transition(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        attributes: u32,
        structure: &Structure,
    ) -> PropertyOffset {
        let old_out_of_line_capacity = structure.out_of_line_capacity() as usize;
        let result = structure.add_property_without_transition(vm, property_name, attributes);
        let new_out_of_line_capacity = structure.out_of_line_capacity() as usize;
        if new_out_of_line_capacity != old_out_of_line_capacity {
            self.allocate_more_out_of_line_storage(vm, old_out_of_line_capacity, new_out_of_line_capacity);
        }

        // This assertion verifies that the concurrent GC won't read garbage if the concurrentGC is
        // running at the same time we put without transitioning.
        debug_assert!(self.get_direct(result).is_empty());
        result
    }

    /// `putDirectWithoutTransition(vm, propertyName, value, attributes)`.
    pub fn put_direct_without_transition(&self, vm: &VM, property_name: &PropertyName, value: JSValue, attributes: u32) {
        debug_assert!(GetterSetter::from_value(&value).is_none() && attributes & ACCESSOR == 0);
        let structure = self.structure();
        let offset = self.prepare_to_put_direct_without_transition(vm, property_name, attributes, &structure);
        self.put_direct_offset(vm, offset, value);
        if attributes & READ_ONLY != 0 {
            structure.set_contains_read_only_properties();
        }
    }

    /// `putDirectNonIndexAccessorWithoutTransition(vm, propertyName, accessor, attributes)`.
    pub fn put_direct_non_index_accessor_without_transition(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        accessor: &GetterSetter,
        attributes: u32,
    ) {
        debug_assert!(attributes & ACCESSOR != 0);
        let structure = self.structure();
        let offset = self.prepare_to_put_direct_without_transition(vm, property_name, attributes, &structure);
        self.put_direct_offset(vm, offset, accessor.as_value());
        if attributes & READ_ONLY != 0 {
            structure.set_contains_read_only_properties();
        }

        structure
            .set_has_any_kind_of_getter_setter_properties_with_proto_check(*property_name == vm.property_names.underscore_proto);
    }

    /// `putDirectToDictionaryWithoutExtensibility(vm, propertyName, value, slot)`.
    fn put_direct_to_dictionary_without_extensibility(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Option<&'static str> {
        let structure = self.structure();
        let (offset, current_attributes) = structure.get_with_attributes(vm, property_name);
        if offset != INVALID_OFFSET {
            if current_attributes & READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR != 0 {
                return Some(READONLY_PROPERTY_CHANGE_ERROR);
            }

            self.put_direct_offset(vm, offset, value);

            // FIXME do C++: Check attributes against PropertyAttribute::CustomAccessorOrValue. Changing
            // GetterSetter should work w/o transition.
            debug_assert!(current_attributes & ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE == 0);
            slot.set_existing_property(self, offset);
            return None;
        }

        Some(NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR)
    }

    // Accessors.

    /// `putDirectAccessor(globalObject, propertyName, accessor, attributes)`.
    pub fn put_direct_accessor(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        accessor: GetterSetterRef,
        attributes: u32,
    ) -> Result<bool, PutError> {
        debug_assert!(attributes & ACCESSOR != 0);

        if let Some(index) = property_name.parse_index() {
            // `putDirectIndex(globalObject, index, accessor, attributes, PutDirectIndexLikePutDirect)`.
            return self.put_direct_index(
                vm,
                index,
                accessor.as_value(),
                attributes,
                crate::runtime::sparse_array_value_map::PutDirectIndexMode::PutDirectIndexLikePutDirect,
            );
        }

        Ok(self.put_direct_non_index_accessor(vm, property_name, &accessor, attributes))
    }

    /// `putDirectNonIndexAccessor(vm, propertyName, accessor, attributes)`.
    pub fn put_direct_non_index_accessor(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        accessor: &GetterSetter,
        attributes: u32,
    ) -> bool {
        debug_assert!(attributes & ACCESSOR != 0);
        let mut slot = PutPropertySlot::new(self.as_value(), false, PutContext::UnknownContext, false);
        let result = self
            .put_direct_internal(vm, property_name, accessor.as_value(), attributes, &mut slot, PutMode::PutModeDefineOwnProperty)
            .is_none();

        let structure = self.structure();
        if attributes & READ_ONLY != 0 {
            structure.set_contains_read_only_properties();
        }

        structure
            .set_has_any_kind_of_getter_setter_properties_with_proto_check(*property_name == vm.property_names.underscore_proto);
        result
    }

    // Delete.

    /// `deleteProperty(cell, globalObject, propertyName, slot)`. O `Err` é o `Unported` de índice em
    /// `ArrayStorage`. Sem `VM::deletePropertyMode` (`IgnoreConfigurable` é só do `Heap`/`JSLock`
    /// durante a destruição do VM) e sem propriedades estáticas, nem `invalidateStructureChainIntegrity`.
    pub fn delete_property(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        slot: &mut DeletePropertySlot,
    ) -> Result<bool, PutError> {
        // `ProxyObject::deleteProperty`; o nome privado sai do armazenamento do próprio `Proxy`.
        if self.type_() == JSType::ProxyObjectType && !property_name.is_private_name() {
            return crate::runtime::proxy_object::delete_from_proxy(self, property_name);
        }
        if let Some(result) = crate::runtime::js_web_assembly_gc_object::delete_property(self) {
            return result;
        }
        // `ClonedArguments::deleteProperty` e `GenericArgumentsImpl::deleteProperty`.
        crate::runtime::js_arguments_objects::materialize_specials_for_property(vm, self, property_name)?;
        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            return target_object.delete_property(vm, property_name, slot);
        }
        if crate::runtime::process_exit::is_process_object(self) && *property_name == PropertyName::from_identifier(&Identifier::from_span(vm, b"exitCode")) {
            return Err(PutError::TypeError(crate::runtime::error_messages::UNABLE_TO_DELETE_PROPERTY_ERROR));
        }
        // `ErrorConstructor::deleteProperty`: apagar `stackTraceLimit` limpa o espelho do global.
        crate::runtime::error_natives::error_constructor_delete_property(self, vm, property_name);
        // `JSGenericTypedArrayView::deleteProperty`.
        if let Some(deleted) = typed_array_dispatch::delete_property(self, vm, property_name) {
            return Ok(deleted);
        }
        // `RegExpObject::deleteProperty`: o `lastIndex` não é configurável.
        if self.type_() == JSType::RegExpObjectType && *property_name == vm.property_names.last_index {
            if let Some(reg_exp) = crate::runtime::reg_exp_object::RegExpObject::from_cell_id(self.cell_id()) {
                if let Some(deleted) = reg_exp.delete_property(vm, property_name) {
                    return Ok(deleted);
                }
            }
        }
        // `ErrorInstance::deleteProperty`.
        crate::runtime::error_instance::materialize_for_property(self, vm, property_name);
        // `StringObject::deleteProperty`: `length` e os caracteres não se apagam.
        if let Some(string_object) = self.string_object_of() {
            if *property_name == vm.property_names.length {
                return Ok(false);
            }
            if let Some(index) = property_name.parse_index() {
                if string_object.can_get_index(index) {
                    return Ok(false);
                }
            }
        }
        // `JSSymbolTableObject::deleteProperty`: o binding da `SymbolTable` do global (`var` e função do script, `let`,
        // `const`, `class`) não se apaga, mesmo por `delete globalThis.x`.
        if matches!(self.type_(), JSType::GlobalObjectType | JSType::GlobalLexicalEnvironmentType) {
            if let (Some(key), Some(scope)) = (property_name.uid(), crate::runtime::js_scope::JSScope::from_cell_id(self.cell_id())) {
                if scope.symbol_table().is_some_and(|table| table.borrow().contains(key)) {
                    return Ok(false);
                }
            }
        }
        if let Some(index) = property_name.parse_index() {
            return self.delete_property_by_index(vm, index);
        }

        if let Some(namespace) = js_module_namespace_object::exotic_of(self) {
            namespace.before_delete_property(vm, property_name)?;
        }
        // `hasNonReifiedStaticProperties`: nome `DontDelete` da tabela falha; outro nome reifica tudo antes.
        if let Some(result) = self.reify_before_delete(vm, property_name) {
            slot.set_nonconfigurable();
            return Ok(result);
        }
        let structure = self.structure();

        let (current_offset, attributes) = structure.get_with_attributes(vm, property_name);
        let property_is_present = is_valid_offset(current_offset);
        if property_is_present {
            if attributes & DONT_DELETE != 0 {
                slot.set_nonconfigurable();
                return Ok(false);
            }

            if structure.is_uncacheable_dictionary() {
                let offset = structure.remove_property_without_transition(vm, property_name);
                debug_assert!(!is_valid_offset(structure.get(vm, property_name)));
                if offset != INVALID_OFFSET {
                    self.put_direct_offset(vm, offset, JSValue::empty());
                }
            } else {
                let (new_structure, offset) = Structure::remove_property_transition(vm, &structure, property_name);
                slot.set_hit(offset);
                debug_assert!(new_structure.out_of_line_capacity() != 0 || self.structure().out_of_line_capacity() == 0);
                self.set_structure(vm, &new_structure);
                debug_assert!(!is_valid_offset(new_structure.get(vm, property_name)));
                if offset != INVALID_OFFSET {
                    self.put_direct_offset(vm, offset, JSValue::empty());
                }
            }
        } else {
            slot.set_configurable_miss();
        }

        Ok(true)
    }

    /// `deletePropertyByIndex(cell, globalObject, i)`: as formas de vetor (Int32, Double, Contiguous, e as
    /// `CopyOnWrite`) e o `ArrayStorage` (vetor e mapa esparso).
    pub fn delete_property_by_index(&self, vm: &VM, i: u32) -> Result<bool, PutError> {
        // `ProxyObject::deletePropertyByIndex`.
        if self.type_() == JSType::ProxyObjectType {
            let name = PropertyName::from_identifier(&Identifier::from_u32(vm, i));
            return crate::runtime::proxy_object::delete_from_proxy(self, &name);
        }
        if let Some(result) = crate::runtime::js_web_assembly_gc_object::delete_property(self) {
            return result;
        }
        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            return target_object.delete_property_by_index(vm, i);
        }
        // `JSGenericTypedArrayView::deletePropertyByIndex`.
        if let Some(deleted) = typed_array_dispatch::delete_property_by_index(self, i) {
            return Ok(deleted);
        }
        if let Some(arguments) = generic_arguments::exotic_of(self) {
            return generic_arguments::delete_property_by_index(&*arguments, vm, i);
        }
        if let Some(namespace) = js_module_namespace_object::exotic_of(self) {
            return namespace.delete_property_by_index(i);
        }
        // `StringObject::deletePropertyByIndex`.
        if let Some(string_object) = self.string_object_of() {
            if let Some(deleted) = string_object.delete_property_by_index(i) {
                return Ok(deleted);
            }
        }
        debug_assert!(i <= MAX_ARRAY_INDEX);

        match self.shape() {
            NO_INDEXING_SHAPE | UNDECIDED_SHAPE => Ok(true),
            INT32_SHAPE | CONTIGUOUS_SHAPE | DOUBLE_SHAPE => {
                if i >= self.vector_length() {
                    return Ok(true);
                }
                self.ensure_writable(vm);
                if self.shape() == DOUBLE_SHAPE {
                    self.set_vector_double(i, pnan());
                } else {
                    self.set_vector_value(i, JSValue::empty());
                }
                Ok(true)
            }
            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                let mut butterfly = self.butterfly.borrow_mut();
                let IndexedStorage::ArrayStorage(storage) = &mut butterfly.indexed else {
                    unreachable!("forma ArrayStorage sem ArrayStorage")
                };
                if i < storage.vector_length() {
                    let value_slot = &mut storage.vector_mut()[i as usize];
                    if !value_slot.is_empty() {
                        *value_slot = JSValue::empty();
                        let count = storage.num_values_in_vector();
                        storage.set_num_values_in_vector(count - 1);
                    }
                } else if let Some(map) = storage.sparse_map_mut() {
                    if let Some(entry) = map.find(i) {
                        if entry.attributes() & DONT_DELETE != 0 {
                            return Ok(false);
                        }
                        map.remove(i);
                    }
                }
                Ok(true)
            }
            shape => unreachable!("deletePropertyByIndex em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    // Property descriptors.

    /// `getOwnPropertyDescriptor(globalObject, propertyName, descriptor)`.
    pub fn get_own_property_descriptor(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        descriptor: &mut PropertyDescriptor,
    ) -> bool {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);

        if !self.get_own_property_slot(vm, property_name, &mut slot) {
            return false;
        }

        descriptor.set_property_slot(&slot, property_name);
        true
    }

    /// `JSObject::isExtensible(globalObject)`: o `Proxy` responde pelo trap `isExtensible`, no realm da
    /// `Structure` dele (o `JSObject` do porte não recebe o `globalObject`); o `JSGlobalProxy` repassa ao alvo.
    fn is_extensible(&self) -> Result<bool, PutError> {
        if !self.structure().type_info().overrides_is_extensible() {
            return Ok(self.is_structure_extensible());
        }
        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            return target_object.is_extensible();
        }
        let Some(proxy) = ProxyObject::from_cell_id(self.cell_id()) else {
            // `WebAssemblyGCObjectBase::isExtensible`: `false`. Os únicos tipos com `OverridesIsExtensible`
            // no C++ são `ProxyObject`, `JSGlobalProxy` e `WebAssemblyGCObjectBase`.
            assert!(
                self.type_() == JSType::WebAssemblyGCObjectType,
                "OverridesIsExtensible em tipo que não é Proxy, JSGlobalProxy nem WebAssemblyGCObject"
            );
            return Ok(false);
        };
        let realm = self.structure().realm().expect("Proxy sem realm na Structure");
        match proxy.perform_is_extensible(&realm) {
            Ok(is_extensible) => Ok(is_extensible),
            Err(thrown) => Err(put_error_from_thrown(&realm, thrown)),
        }
    }

    /// `defineOwnNonIndexProperty(globalObject, propertyName, descriptor, throwException)`.
    pub fn define_own_non_index_property(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        throw_exception: bool,
    ) -> Result<bool, PutError> {
        let mut current = PropertyDescriptor::default();
        let is_current_defined = self.get_own_property_descriptor(vm, property_name, &mut current);
        let is_extensible = self.is_extensible()?;
        validate_and_apply_property_descriptor(
            vm,
            Some(self),
            property_name,
            is_extensible,
            descriptor,
            is_current_defined,
            &current,
            throw_exception,
        )
    }

    /// A primeira metade de `JSGlobalObject::defineOwnProperty`: se a `SymbolTable` do global tem a chave,
    /// valida contra o descritor atual (valor e atributos da variável), grava o valor na variável
    /// (`symbolTablePutTouchWatchpointSet` ignorando somente leitura) e, se `writable: false`, marca a entrada
    /// como somente leitura. `None` quando a chave não está na tabela (segue o `JSObject::defineOwnProperty`).
    /// O `varReadOnlyWatchpointSet().fireAll` não existe no porte (sem JIT que o observe).
    fn define_own_symbol_table_property(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        throw_exception: bool,
    ) -> Option<Result<bool, PutError>> {
        let key = property_name.uid()?;
        let scope = crate::runtime::js_scope::JSScope::from_cell_id(self.cell_id())?;
        let (value, attributes) = scope.symbol_table_get(key)?;
        let current = PropertyDescriptor::new(value, attributes);
        let was_read_only = attributes & READ_ONLY != 0;
        let compatible = match validate_and_apply_property_descriptor(vm, None, property_name, false, descriptor, true, &current, throw_exception) {
            Ok(compatible) => compatible,
            Err(error) => return Some(Err(error)),
        };
        if !compatible {
            return Some(Ok(false));
        }
        if !descriptor.value().is_empty() {
            scope.symbol_table_put(key, descriptor.value(), throw_exception, true);
        }
        if descriptor.writable_present() && !descriptor.writable() && !was_read_only {
            if let Some(table) = scope.symbol_table() {
                table.borrow_mut().set_read_only_for(key);
            }
        }
        Some(Ok(true))
    }

    /// `defineOwnProperty(object, globalObject, propertyName, descriptor, throwException)`: o
    /// `[[DefineOwnProperty]]` ordinário (`JSObject::defineOwnProperty`). O de `Array` (`length`) é o
    /// `JSArray::define_own_property`, que quem despacha por classe escolhe.
    pub fn define_own_property(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        throw_exception: bool,
    ) -> Result<bool, PutError> {
        // `ProxyObject::defineOwnProperty`; o nome privado é do armazenamento do próprio `Proxy`.
        if self.type_() == JSType::ProxyObjectType && !property_name.is_private_name() {
            return crate::runtime::proxy_object::define_own_property_from_proxy(self, property_name, descriptor, throw_exception);
        }
        if let Some(result) = crate::runtime::js_web_assembly_gc_object::define_own_property(self, throw_exception) {
            return result;
        }
        // `JSModuleNamespaceObject::defineOwnProperty`.
        if let Some(namespace) = js_module_namespace_object::exotic_of(self) {
            if let Some(result) = namespace.define_own_property(vm, property_name, descriptor, throw_exception) {
                return result;
            }
        }
        // `process.env`: só o descritor de dados completo, com o valor já texto.
        let env_descriptor;
        let descriptor = if crate::runtime::process_env::is_env(self) {
            env_descriptor = crate::runtime::process_env::before_define(vm, descriptor)?;
            &env_descriptor
        } else {
            descriptor
        };
        // `ClonedArguments::defineOwnProperty` e `GenericArgumentsImpl::defineOwnProperty`.
        crate::runtime::js_arguments_objects::materialize_specials_for_property(vm, self, property_name)?;
        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            return target_object.define_own_property(vm, property_name, descriptor, throw_exception);
        }
        // `JSGlobalObject::defineOwnProperty`: o binding da `SymbolTable` do global (`var` e função do script).
        if self.type_() == JSType::GlobalObjectType {
            if let Some(result) = self.define_own_symbol_table_property(vm, property_name, descriptor, throw_exception) {
                return result;
            }
        }
        // `JSGenericTypedArrayView::defineOwnProperty`.
        if let Some(result) = typed_array_dispatch::define_own_property(self, property_name, descriptor, throw_exception) {
            return result;
        }
        // `RegExpObject::defineOwnProperty`.
        if self.type_() == JSType::RegExpObjectType && *property_name == vm.property_names.last_index {
            if let Some(reg_exp) = crate::runtime::reg_exp_object::RegExpObject::from_cell_id(self.cell_id()) {
                if let Some(result) = reg_exp.define_own_property(vm, property_name, descriptor, throw_exception) {
                    return result;
                }
            }
        }
        // `StringObject::defineOwnProperty`: `length` e os caracteres são sempre
        // {writable: false, enumerable: true (não em `length`), configurable: false}; só valida.
        if let Some(string_object) = self.string_object_of() {
            if string_object.is_string_own_property(vm, property_name) {
                let mut current = PropertyDescriptor::default();
                let is_current_defined = self.get_own_property_descriptor(vm, property_name, &mut current);
                let is_extensible = self.is_structure_extensible();
                return validate_and_apply_property_descriptor(
                    vm,
                    None,
                    property_name,
                    is_extensible,
                    descriptor,
                    is_current_defined,
                    &current,
                    throw_exception,
                );
            }
        }
        // `ErrorInstance::defineOwnProperty`.
        if crate::runtime::error_instance::materialize_for_property(self, vm, property_name) && vm.exception().is_some() {
            return Err(PutError::Pending);
        }
        // If it's an array index, then use the indexed property storage.
        if let Some(index) = property_name.parse_index() {
            if let Some(arguments) = generic_arguments::exotic_of(self) {
                return generic_arguments::define_own_indexed_property(&*arguments, vm, index, descriptor, throw_exception);
            }
            return self.define_own_indexed_property(vm, index, descriptor, throw_exception);
        }

        // `defineOwnProperty` lê o descritor atual por `getOwnPropertySlot`, que reifica o nome da tabela.
        if let Some(entry) = self.non_reified_custom_accessor_entry(vm, property_name) {
            crate::runtime::lookup::reify_static_property(vm, self, entry);
        }
        self.reify_static_property_named(vm, property_name);
        self.define_own_non_index_property(vm, property_name, descriptor, throw_exception)
    }

    // Indexed storage.

    /// `indexingType() & IndexingShapeMask`.
    pub(crate) fn shape(&self) -> IndexingType {
        self.cell.indexing_type() & INDEXING_SHAPE_MASK
    }

    /// `butterfly()->publicLength()`.
    pub fn public_length(&self) -> u32 {
        let butterfly = self.butterfly.borrow();
        match &butterfly.indexed {
            IndexedStorage::ArrayStorage(storage) => storage.length(),
            _ => butterfly.public_length,
        }
    }

    /// `butterfly()->setPublicLength(length)`.
    pub(crate) fn set_public_length(&self, length: u32) {
        let mut guard = self.butterfly.borrow_mut();
        let butterfly = &mut *guard;
        match &mut butterfly.indexed {
            IndexedStorage::ArrayStorage(storage) => storage.set_length(length),
            _ => butterfly.public_length = length,
        }
    }

    /// `butterfly()->vectorLength()`.
    pub fn vector_length(&self) -> u32 {
        self.butterfly.borrow().indexed.vector_length()
    }

    /// O comprimento de vetor e se há cabeçalho de indexação, para a contabilidade de bytes do coletor (`heap_size`);
    /// `(0, false)` quando a butterfly está emprestada para escrita (a coleta não lê o que não pode provar).
    pub(crate) fn butterfly_shape(&self) -> (usize, bool) {
        let Ok(butterfly) = self.butterfly.try_borrow() else { return (0, false) };
        match &butterfly.indexed {
            IndexedStorage::None => (0, false),
            indexed => (indexed.vector_length() as usize, true),
        }
    }

    /// `nonPropertyTransition` aplicado a este objeto.
    pub(crate) fn transition_indexing(&self, vm: &VM, transition_kind: TransitionKind) {
        let old_structure = self.structure();
        let new_structure = Structure::non_property_transition(vm, &old_structure, transition_kind);
        self.set_structure(vm, &new_structure);
    }

    /// `createInitialIndexedStorage(vm, length)` e o `createInitial<Shape>` que a chama: o butterfly indexado
    /// novo, com `publicLength = length`, o `vectorLength` ótimo e a transição de forma pedida. `false` é a
    /// falta de memória (`tryAllocate` recusado), sem abortar.
    fn create_initial_indexed_storage(
        &self,
        vm: &VM,
        length: u32,
        make_storage: fn(usize) -> Option<IndexedStorage>,
        transition: TransitionKind,
    ) -> bool {
        debug_assert!(length <= MAX_STORAGE_VECTOR_LENGTH);
        debug_assert!(!has_indexed_properties(self.cell.indexing_type()));
        debug_assert!(!self.needs_slow_put_indexing());
        debug_assert!(!self.indexing_should_be_sparse());
        let property_capacity = self.structure().out_of_line_capacity() as usize;
        let vector_length = optimal_contiguous_vector_length(property_capacity, length);
        // Falta de memória (`tryAllocate` devolvendo `nullptr`): o objeto fica como estava.
        let Some(storage) = make_storage(vector_length as usize) else {
            return false;
        };
        self.butterfly.borrow_mut().indexed = storage;
        self.set_public_length(length);
        self.transition_indexing(vm, transition);
        true
    }

    /// `createInitialUndecided(vm, length)`: `false` é a falta de memória.
    pub fn create_initial_undecided(&self, vm: &VM, length: u32) -> bool {
        self.create_initial_indexed_storage(vm, length, empty_values_storage, TransitionKind::AllocateUndecided)
    }

    /// `createInitialInt32(vm, length)`: `false` é a falta de memória.
    pub fn create_initial_int32(&self, vm: &VM, length: u32) -> bool {
        self.create_initial_indexed_storage(vm, length, empty_values_storage, TransitionKind::AllocateInt32)
    }

    /// `createInitialDouble(vm, length)`: `false` é a falta de memória.
    pub fn create_initial_double(&self, vm: &VM, length: u32) -> bool {
        self.create_initial_indexed_storage(vm, length, pnan_doubles_storage, TransitionKind::AllocateDouble)
    }

    /// `createInitialContiguous(vm, length)`: `false` é a falta de memória.
    pub fn create_initial_contiguous(&self, vm: &VM, length: u32) -> bool {
        self.create_initial_indexed_storage(vm, length, empty_values_storage, TransitionKind::AllocateContiguous)
    }


    /// `convertUndecidedToInt32(vm)`.
    pub fn convert_undecided_to_int32(&self, vm: &VM) {
        debug_assert!(has_undecided(self.cell.indexing_type()));
        if let IndexedStorage::Values(values) = &mut self.butterfly.borrow_mut().indexed {
            values.iter_mut().for_each(|value| *value = JSValue::empty());
        }
        self.transition_indexing(vm, TransitionKind::AllocateInt32);
    }

    /// `convertUndecidedToDouble(vm)`.
    pub fn convert_undecided_to_double(&self, vm: &VM) {
        debug_assert!(has_undecided(self.cell.indexing_type()));
        {
            let mut butterfly = self.butterfly.borrow_mut();
            let vector_length = butterfly.indexed.vector_length() as usize;
            butterfly.indexed = IndexedStorage::Doubles(vec![pnan(); vector_length]);
        }
        self.transition_indexing(vm, TransitionKind::AllocateDouble);
    }

    /// `convertUndecidedToContiguous(vm)`.
    pub fn convert_undecided_to_contiguous(&self, vm: &VM) {
        debug_assert!(has_undecided(self.cell.indexing_type()));
        if let IndexedStorage::Values(values) = &mut self.butterfly.borrow_mut().indexed {
            values.iter_mut().for_each(|value| *value = JSValue::empty());
        }
        self.transition_indexing(vm, TransitionKind::AllocateContiguous);
    }

    /// `convertUndecidedForValue(vm, value)`.
    pub fn convert_undecided_for_value(&self, vm: &VM, value: JSValue) {
        let type_ = crate::runtime::indexing_type::indexing_type_for_value(value);
        if type_ == INT32_SHAPE {
            self.convert_undecided_to_int32(vm);
            return;
        }

        if type_ == DOUBLE_SHAPE {
            debug_assert!(crate::runtime::options::Options::with(|options| options.allow_double_shape));
            self.convert_undecided_to_double(vm);
            return;
        }

        debug_assert!(type_ == CONTIGUOUS_SHAPE);
        self.convert_undecided_to_contiguous(vm);
    }

    /// `convertInt32ToDouble(vm)`.
    pub fn convert_int32_to_double(&self, vm: &VM) {
        debug_assert!(has_int32(self.cell.indexing_type()));
        debug_assert!(!is_copy_on_write(self.cell.indexing_mode()));
        {
            let mut butterfly = self.butterfly.borrow_mut();
            let doubles = match &butterfly.indexed {
                IndexedStorage::Values(values) => values
                    .iter()
                    .map(|value| if value.is_int32() { value.as_int32() as f64 } else { pnan() })
                    .collect(),
                _ => Vec::new(),
            };
            butterfly.indexed = IndexedStorage::Doubles(doubles);
        }
        self.transition_indexing(vm, TransitionKind::AllocateDouble);
    }

    /// `convertInt32ToContiguous(vm)`.
    pub fn convert_int32_to_contiguous(&self, vm: &VM) {
        debug_assert!(has_int32(self.cell.indexing_type()));
        self.transition_indexing(vm, TransitionKind::AllocateContiguous);
    }

    /// `convertDoubleToContiguous(vm)`.
    pub fn convert_double_to_contiguous(&self, vm: &VM) {
        debug_assert!(has_double(self.cell.indexing_type()));
        debug_assert!(!is_copy_on_write(self.cell.indexing_mode()));
        {
            let mut butterfly = self.butterfly.borrow_mut();
            let values = match &butterfly.indexed {
                IndexedStorage::Doubles(doubles) => doubles
                    .iter()
                    .map(|value| if *value != *value { JSValue::empty() } else { JSValue::double_number(*value) })
                    .collect(),
                _ => Vec::new(),
            };
            butterfly.indexed = IndexedStorage::Values(values);
        }
        self.transition_indexing(vm, TransitionKind::AllocateContiguous);
    }

    /// `convertInt32ForValue(vm, value)`.
    pub fn convert_int32_for_value(&self, vm: &VM, value: JSValue) {
        debug_assert!(!value.is_int32());

        if value.is_double()
            && !value.as_double().is_nan()
            && crate::runtime::options::Options::with(|options| options.allow_double_shape)
        {
            self.convert_int32_to_double(vm);
            return;
        }

        self.convert_int32_to_contiguous(vm);
    }

    /// `convertFromCopyOnWrite(vm)`: o butterfly deste porte já é próprio (não é compartilhado com um
    /// `JSCellButterfly`), então só a transição de forma é necessária; o `vectorLength` fica o do
    /// butterfly copiado, como no C++.
    pub fn convert_from_copy_on_write(&self, vm: &VM) {
        debug_assert!(is_copy_on_write(self.cell.indexing_mode()));
        debug_assert!(self.structure().indexing_mode() == self.cell.indexing_mode());

        let transition = match self.cell.indexing_type() {
            ARRAY_WITH_INT32 => TransitionKind::AllocateInt32,
            ARRAY_WITH_DOUBLE => TransitionKind::AllocateDouble,
            ARRAY_WITH_CONTIGUOUS => TransitionKind::AllocateContiguous,
            other => unreachable!("convertFromCopyOnWrite em forma {other:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        };
        self.transition_indexing(vm, transition);
    }

    /// `ensureWritable(vm)`.
    pub fn ensure_writable(&self, vm: &VM) {
        if is_copy_on_write(self.cell.indexing_mode()) {
            self.convert_from_copy_on_write(vm);
        }
    }

    /// `createInitialForValueAndSet(vm, index, value)`: `false` é a falta de memória.
    pub fn create_initial_for_value_and_set(&self, vm: &VM, index: u32, value: JSValue) -> bool {
        if value.is_int32() {
            if !self.create_initial_int32(vm, index + 1) {
                return false;
            }
            self.set_vector_value(index, value);
            return true;
        }

        if value.is_double() && crate::runtime::options::Options::with(|options| options.allow_double_shape) {
            let double_value = value.as_number();
            if double_value == double_value {
                if !self.create_initial_double(vm, index + 1) {
                    return false;
                }
                self.set_vector_double(index, double_value);
                return true;
            }
        }

        if !self.create_initial_contiguous(vm, index + 1) {
            return false;
        }
        self.set_vector_value(index, value);
        true
    }

    /// `butterfly->contiguous().at(this, i) = value` (sem tocar no `publicLength`).
    pub(crate) fn set_vector_value(&self, i: u32, value: JSValue) {
        if let IndexedStorage::Values(values) = &mut self.butterfly.borrow_mut().indexed {
            values[i as usize] = value;
        }
    }

    /// `butterfly->contiguousDouble().at(this, i) = value` (sem tocar no `publicLength`).
    pub(crate) fn set_vector_double(&self, i: u32, value: f64) {
        if let IndexedStorage::Doubles(doubles) = &mut self.butterfly.borrow_mut().indexed {
            doubles[i as usize] = value;
        }
    }

    /// `canGetIndexQuickly(i)`.
    pub fn can_get_index_quickly(&self, i: u32) -> bool {
        let butterfly = self.butterfly.borrow();
        match self.shape() {
            NO_INDEXING_SHAPE | UNDECIDED_SHAPE => false,
            INT32_SHAPE | CONTIGUOUS_SHAPE => match &butterfly.indexed {
                IndexedStorage::Values(values) => values.get(i as usize).is_some_and(|value| !value.is_empty()),
                _ => false,
            },
            DOUBLE_SHAPE => match &butterfly.indexed {
                IndexedStorage::Doubles(doubles) => doubles.get(i as usize).is_some_and(|value| value == value),
                _ => false,
            },
            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => match &butterfly.indexed {
                IndexedStorage::ArrayStorage(storage) => {
                    storage.vector().get(i as usize).is_some_and(|value| !value.is_empty())
                }
                _ => false,
            },
            shape => unreachable!("canGetIndexQuickly em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// `getIndexQuickly(i)`.
    pub fn get_index_quickly(&self, i: u32) -> JSValue {
        let butterfly = self.butterfly.borrow();
        match (self.shape(), &butterfly.indexed) {
            (INT32_SHAPE | CONTIGUOUS_SHAPE, IndexedStorage::Values(values)) => values[i as usize],
            (DOUBLE_SHAPE, IndexedStorage::Doubles(doubles)) => JSValue::double_number(doubles[i as usize]),
            (ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE, IndexedStorage::ArrayStorage(storage)) => {
                storage.vector()[i as usize]
            }
            (shape, _) => unreachable!("getIndexQuickly em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// `setIndexQuickly(vm, i, v)`: o índice tem de estar dentro do `vectorLength`.
    pub fn set_index_quickly(&self, vm: &VM, i: u32, v: JSValue) {
        debug_assert!(!is_copy_on_write(self.cell.indexing_mode()));
        match self.shape() {
            INT32_SHAPE => {
                debug_assert!(i < self.vector_length());
                if !v.is_int32() {
                    self.convert_int32_to_double_or_contiguous_while_performing_set_index(vm, i, v);
                    return;
                }
                self.store_contiguous(i, v);
            }
            CONTIGUOUS_SHAPE => {
                debug_assert!(i < self.vector_length());
                self.store_contiguous(i, v);
            }
            DOUBLE_SHAPE => {
                debug_assert!(i < self.vector_length());
                if !v.is_number() {
                    self.convert_double_to_contiguous_while_performing_set_index(vm, i, v);
                    return;
                }
                let value = v.as_number();
                if value != value {
                    self.convert_double_to_contiguous_while_performing_set_index(vm, i, v);
                    return;
                }
                self.set_vector_double(i, value);
                if i >= self.public_length() {
                    self.set_public_length(i + 1);
                }
            }
            ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                if let IndexedStorage::ArrayStorage(storage) = &mut self.butterfly.borrow_mut().indexed {
                    // `setIndexQuicklyForArrayStorageIndexingType`: um slot antes vazio conta como valor novo
                    // e estende o comprimento (o `putDirectIndex` de um literal com buraco, `[0,,2]`, escreve
                    // além do comprimento inicial).
                    let was_empty = storage.vector()[i as usize].is_empty();
                    storage.vector_mut()[i as usize] = v;
                    if was_empty {
                        storage.set_num_values_in_vector(storage.num_values_in_vector() + 1);
                        if i >= storage.length() {
                            storage.set_length(i + 1);
                        }
                    }
                }
            }
            shape => unreachable!("setIndexQuickly em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// O corpo comum de `ALL_CONTIGUOUS_INDEXING_TYPES` em `setIndexQuickly` e `putByIndex`.
    fn store_contiguous(&self, i: u32, v: JSValue) {
        self.set_vector_value(i, v);
        if i >= self.public_length() {
            self.set_public_length(i + 1);
        }
    }

    /// `setIndexQuicklyToUndecided(vm, index, value)`.
    pub fn set_index_quickly_to_undecided(&self, vm: &VM, index: u32, value: JSValue) {
        debug_assert!(index < self.public_length());
        debug_assert!(index < self.vector_length());
        self.convert_undecided_for_value(vm, value);
        self.set_index_quickly(vm, index, value);
    }

    /// `convertInt32ToDoubleOrContiguousWhilePerformingSetIndex(vm, index, value)`.
    pub fn convert_int32_to_double_or_contiguous_while_performing_set_index(&self, vm: &VM, index: u32, value: JSValue) {
        debug_assert!(!value.is_int32());
        self.convert_int32_for_value(vm, value);
        self.set_index_quickly(vm, index, value);
    }

    /// `convertDoubleToContiguousWhilePerformingSetIndex(vm, index, value)`.
    pub fn convert_double_to_contiguous_while_performing_set_index(&self, vm: &VM, index: u32, value: JSValue) {
        debug_assert!(!value.is_number() || value.as_number() != value.as_number());
        self.convert_double_to_contiguous(vm);
        self.set_index_quickly(vm, index, value);
    }

    /// `ensureLength(vm, length)`: `false` é a falta de memória.
    pub fn ensure_length(&self, vm: &VM, length: u32) -> bool {
        assert!(length <= MAX_STORAGE_VECTOR_LENGTH);
        debug_assert!({
            let indexing_type = self.cell.indexing_type();
            has_contiguous(indexing_type) || has_int32(indexing_type) || has_double(indexing_type) || has_undecided(indexing_type)
        });

        if self.vector_length() < length || is_copy_on_write(self.cell.indexing_mode()) {
            if !self.ensure_length_slow(vm, length) {
                return false;
            }
        }

        if self.public_length() < length {
            self.set_public_length(length);
        }
        true
    }

    /// `ensureLengthSlow(vm, length)`.
    fn ensure_length_slow(&self, vm: &VM, length: u32) -> bool {
        if is_copy_on_write(self.cell.indexing_mode()) {
            self.convert_from_copy_on_write(vm);
            if self.vector_length() >= length {
                return true;
            }
        }

        debug_assert!(length <= MAX_STORAGE_VECTOR_LENGTH);
        debug_assert!(length > self.vector_length());

        let old_vector_length = self.vector_length();
        let property_capacity = self.structure().out_of_line_capacity() as usize;

        let available_old_length = available_contiguous_vector_length(property_capacity, old_vector_length);
        let new_vector_length = if available_old_length >= length {
            // This is the case where someone else selected a vector length that caused internal
            // fragmentation. If we did our jobs right, this would never happen. But I bet we will mess
            // this up, so this defense should stay.
            available_old_length
        } else {
            optimal_contiguous_vector_length(
                property_capacity,
                next_length(length as usize).min(MAX_STORAGE_VECTOR_LENGTH as usize) as u32,
            )
        };

        self.resize_vector(property_capacity, new_vector_length)
    }

    /// Aloca o vetor indexado com `new_vector_length` elementos (a `Butterfly` nova do C++): `JSArray::tryCreate` o usa
    /// direto, com o comprimento ótimo, e `ensureLengthSlow` com o comprimento crescido.
    pub(crate) fn allocate_vector(&self, vector_length: u32) -> bool {
        let property_capacity = self.structure().out_of_line_capacity() as usize;
        self.resize_vector(property_capacity, optimal_contiguous_vector_length(property_capacity, vector_length))
    }

    fn resize_vector(&self, property_capacity: usize, new_vector_length: u32) -> bool {
        // `Vec::resize` abortaria o processo se o alocador recusasse (até 2 GiB aqui); `try_resize` devolve
        // `false`, que `ensure_length` repassa como a falta de memória. O C++ aloca uma `Butterfly` nova do tamanho
        // inteiro: o relógio de bytes do coletor conta ela.
        crate::runtime::heap_size::did_allocate_butterfly(property_capacity, new_vector_length as usize, true);
        let mut butterfly = self.butterfly.borrow_mut();
        match &mut butterfly.indexed {
            IndexedStorage::Doubles(doubles) => {
                crate::runtime::fallible_alloc::try_resize(doubles, new_vector_length as usize, pnan())
            }
            IndexedStorage::Values(values) => {
                crate::runtime::fallible_alloc::try_resize(values, new_vector_length as usize, JSValue::empty())
            }
            IndexedStorage::None | IndexedStorage::ArrayStorage(_) => false,
        }
    }

    /// `countElements<shape>(butterfly)` de JSObject.cpp.
    pub(crate) fn count_elements(&self) -> u32 {
        let butterfly = self.butterfly.borrow();
        match &butterfly.indexed {
            IndexedStorage::Values(values) => values.iter().filter(|value| !value.is_empty()).count() as u32,
            IndexedStorage::Doubles(doubles) => doubles.iter().filter(|value| **value == **value).count() as u32,
            IndexedStorage::None | IndexedStorage::ArrayStorage(_) => 0,
        }
    }

    /// `JSObject::putByIndex(cell, globalObject, propertyName, value, shouldThrow)`.
    pub fn put_by_index(&self, vm: &VM, property_name: u32, value: JSValue, should_throw: bool) -> Result<bool, PutError> {
        // `ProxyObject::putByIndex`: `putByIndexCommon` com o próprio `Proxy` como `this`.
        if self.type_() == JSType::ProxyObjectType {
            return crate::runtime::proxy_object::put_by_index_from_proxy(self, self.as_value(), property_name, value, should_throw);
        }
        if let Some(result) = crate::runtime::js_web_assembly_gc_object::put(self) {
            return result;
        }
        if let Some(target) = js_global_proxy::target_of(self) {
            let target_object: &JSObject = &target;
            return target_object.put_by_index(vm, property_name, value, should_throw);
        }
        // `JSGenericTypedArrayView::putByIndex`.
        if let Some(result) = typed_array_dispatch::put_by_index(self, property_name, value) {
            return result;
        }
        if let Some(arguments) = generic_arguments::exotic_of(self) {
            return generic_arguments::put_by_index(&*arguments, vm, property_name, value, should_throw);
        }
        if let Some(namespace) = js_module_namespace_object::exotic_of(self) {
            return namespace.put_by_index(should_throw);
        }
        // `process.env`: o valor vira texto também nas chaves de índice.
        let value = if crate::runtime::process_env::is_env(self) {
            crate::runtime::process_env::before_put(vm, None, value)?.unwrap_or(value)
        } else {
            value
        };
        // `StringObject::putByIndex`: os caracteres da string são somente leitura.
        if let Some(string_object) = self.string_object_of() {
            if string_object.can_get_index(property_name) {
                return type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR);
            }
        }
        if property_name > MAX_ARRAY_INDEX {
            let mut slot = PutPropertySlot::new(self.as_value(), should_throw, PutContext::UnknownContext, false);
            let name = PropertyName::from_identifier(&Identifier::from_u32(vm, property_name));
            return self.put(vm, &name, value, &mut slot);
        }

        self.ensure_writable(vm);

        match self.shape() {
            NO_INDEXING_SHAPE => {}

            UNDECIDED_SHAPE => {
                self.convert_undecided_for_value(vm, value);
                // Reloop.
                return self.put_by_index(vm, property_name, value, should_throw);
            }

            shape @ (INT32_SHAPE | CONTIGUOUS_SHAPE) => {
                if shape == INT32_SHAPE && !value.is_int32() {
                    self.convert_int32_for_value(vm, value);
                    return self.put_by_index(vm, property_name, value, should_throw);
                }

                if property_name < self.vector_length() {
                    self.store_contiguous(property_name, value);
                    return Ok(true);
                }
            }

            DOUBLE_SHAPE => {
                if !value.is_number() {
                    self.convert_double_to_contiguous(vm);
                    // Reloop.
                    return self.put_by_index(vm, property_name, value, should_throw);
                }

                let value_as_double = value.as_number();
                if value_as_double != value_as_double {
                    self.convert_double_to_contiguous(vm);
                    // Reloop.
                    return self.put_by_index(vm, property_name, value, should_throw);
                }
                if property_name < self.vector_length() {
                    self.set_vector_double(property_name, value_as_double);
                    if property_name >= self.public_length() {
                        self.set_public_length(property_name + 1);
                    }
                    return Ok(true);
                }
            }

            shape @ (ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE) => {
                if let Some(result) = self.put_by_index_in_array_storage_vector(vm, shape, property_name, value, should_throw)? {
                    return Ok(result);
                }
            }

            shape => unreachable!("putByIndex em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }

        self.put_by_index_beyond_vector_length(vm, property_name, value, should_throw)
    }

    /// `putByIndexBeyondVectorLength(globalObject, i, value, shouldThrow)`.
    fn put_by_index_beyond_vector_length(
        &self,
        vm: &VM,
        i: u32,
        value: JSValue,
        should_throw: bool,
    ) -> Result<bool, PutError> {
        assert!(!is_copy_on_write(self.cell.indexing_mode()));

        // i should be a valid array index that is outside of the current vector.
        debug_assert!(i <= MAX_ARRAY_INDEX);

        match self.shape() {
            NO_INDEXING_SHAPE => {
                if self.indexing_should_be_sparse() {
                    self.ensure_array_storage_exists_and_enter_dictionary_indexing_mode(vm)?;
                    if self.shape() != SLOW_PUT_ARRAY_STORAGE_SHAPE {
                        return self.put_by_index_beyond_vector_length_with_array_storage(vm, i, value, should_throw);
                    }
                } else if index_is_sufficiently_beyond_length_for_sparse_map(i, 0) || i >= MIN_SPARSE_ARRAY_INDEX {
                    if !self.create_array_storage(vm, 0, 0) {
                        return Err(PutError::OutOfMemory);
                    }
                    if self.shape() != SLOW_PUT_ARRAY_STORAGE_SHAPE {
                        return self.put_by_index_beyond_vector_length_with_array_storage(vm, i, value, should_throw);
                    }
                } else if self.needs_slow_put_indexing() {
                    // Convert the indexing type to the SlowPutArrayStorage and retry.
                    if !self.create_array_storage(vm, i + 1, self.get_new_vector_length(i + 1)) {
                        return Err(PutError::OutOfMemory);
                    }
                } else {
                    if !self.create_initial_for_value_and_set(vm, i, value) {
                        return Err(PutError::OutOfMemory);
                    }
                    return Ok(true);
                }
                // Fallback with SlowPutArrayStorage.
                self.put_by_index(vm, i, value, should_throw)
            }

            UNDECIDED_SHAPE => unreachable!("putByIndexBeyondVectorLength em Undecided (CRASH no C++)"),

            shape @ (INT32_SHAPE | DOUBLE_SHAPE | CONTIGUOUS_SHAPE) => {
                self.put_by_index_beyond_vector_length_without_attributes(vm, shape, i, value)
            }

            SLOW_PUT_ARRAY_STORAGE_SHAPE => {
                // No own property present in the vector, but there might be in the sparse map!
                let in_sparse_map = self.with_array_storage(|storage| storage.sparse_map().is_some_and(|map| map.contains(i)));
                if !in_sparse_map {
                    if let Some(put_result) = self.attempt_to_intercept_put_by_index_on_hole(vm, i, value, should_throw)? {
                        return Ok(put_result);
                    }
                }
                self.put_by_index_beyond_vector_length_with_array_storage(vm, i, value, should_throw)
            }

            ARRAY_STORAGE_SHAPE => self.put_by_index_beyond_vector_length_with_array_storage(vm, i, value, should_throw),

            shape => unreachable!("putByIndexBeyondVectorLength em forma {shape:#x} (RELEASE_ASSERT_NOT_REACHED)"),
        }
    }

    /// `putByIndexBeyondVectorLengthWithoutAttributes<indexingShape>(globalObject, i, value)`.
    pub(crate) fn put_by_index_beyond_vector_length_without_attributes(
        &self,
        vm: &VM,
        indexing_shape: IndexingType,
        i: u32,
        value: JSValue,
    ) -> Result<bool, PutError> {
        assert!(!is_copy_on_write(self.cell.indexing_mode()));
        debug_assert!(self.shape() == indexing_shape);
        debug_assert!(!self.indexing_should_be_sparse());

        // For us to get here, the index is either greater than the public length, or greater than or
        // equal to the vector length.
        let vector_length = self.vector_length();
        debug_assert!(i >= vector_length);

        if i > MAX_STORAGE_VECTOR_INDEX
            || (i >= MIN_SPARSE_ARRAY_INDEX && !is_dense_enough_for_vector(i, self.count_elements()))
            || index_is_sufficiently_beyond_length_for_sparse_map(i, vector_length)
        {
            debug_assert!(i <= MAX_ARRAY_INDEX);
            if !self.ensure_array_storage_slow(vm) {
                return Err(PutError::OutOfMemory);
            }
            return self.put_by_index_beyond_vector_length_with_array_storage(vm, i, value, false);
        }

        if !self.ensure_length(vm, i + 1) {
            return Err(PutError::OutOfMemory);
        }

        assert!(i < self.vector_length());
        match indexing_shape {
            INT32_SHAPE => {
                debug_assert!(value.is_int32());
                self.set_vector_value(i, value);
            }
            DOUBLE_SHAPE => {
                debug_assert!(crate::runtime::options::Options::with(|options| options.allow_double_shape));
                debug_assert!(value.is_number());
                let value_as_double = value.as_number();
                debug_assert!(value_as_double == value_as_double);
                self.set_vector_double(i, value_as_double);
            }
            CONTIGUOUS_SHAPE => self.set_vector_value(i, value),
            _ => unreachable!("putByIndexBeyondVectorLengthWithoutAttributes com forma {indexing_shape:#x} (CRASH no C++)"),
        }
        Ok(true)
    }
}

/// `validateAndApplyPropertyDescriptor(globalObject, object, propertyName, isExtensible, descriptor,
/// isCurrentDefined, current, throwException)` (https://tc39.es/ecma262/#sec-validateandapplypropertydescriptor).
/// `object` é `None` no `nullptr` do C++ (só valida).
#[allow(clippy::too_many_arguments)]
pub fn validate_and_apply_property_descriptor(
    vm: &VM,
    object: Option<&JSObject>,
    property_name: &PropertyName,
    is_extensible: bool,
    descriptor: &PropertyDescriptor,
    is_current_defined: bool,
    current: &PropertyDescriptor,
    throw_exception: bool,
) -> Result<bool, PutError> {
    // If we have a new property we can just put it on normally
    // Step 2.
    if !is_current_defined {
        // unless extensions are prevented!
        // Step 2.a
        if !is_extensible {
            return type_error(throw_exception, NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR);
        }

        if let Some(object) = object {
            if descriptor.is_accessor_descriptor() {
                let attributes = (descriptor.attributes() | ACCESSOR) & !READ_ONLY;
                object.put_direct_accessor(vm, property_name, descriptor.slow_getter_setter(vm), attributes)?;
            } else {
                debug_assert!(descriptor.is_generic_descriptor() || descriptor.is_data_descriptor());
                let value = if !descriptor.value().is_empty() { descriptor.value() } else { JSValue::undefined() };
                object.put_direct(vm, property_name, value, descriptor.attributes() & !ACCESSOR);
            }
        }

        return Ok(true);
    }
    // Step 3.
    if descriptor.is_empty() {
        return Ok(true);
    }

    if current.equal_to(descriptor) {
        return Ok(true);
    }

    // Step 4.
    if !current.configurable() {
        if descriptor.configurable() {
            return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR);
        }
        if descriptor.enumerable_present() && descriptor.enumerable() != current.enumerable() {
            return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR);
        }
    }

    if descriptor.is_generic_descriptor() {
        // Step 5.
        // Changing [[Enumerable]] and [[Configurable]] attributes of an existing property
    } else if current.is_data_descriptor() != descriptor.is_data_descriptor() {
        // Step 6.
        // Changing between a data property and accessor property
        if !current.configurable() {
            return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR);
        }
    } else if current.is_data_descriptor() && descriptor.is_data_descriptor() {
        // Step 7.
        // Changing the value and attributes of an existing data property
        if !current.configurable() && !current.writable() {
            if descriptor.writable() {
                return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR);
            }
            if !descriptor.value().is_empty() && !same_value(descriptor.value(), current.value()) {
                return type_error(throw_exception, READONLY_PROPERTY_CHANGE_ERROR);
            }

            return Ok(true);
        }
    } else {
        // Step 8.
        // Changing the accessor functions and attributes of an existing accessor property
        debug_assert!(descriptor.is_accessor_descriptor());
        if !current.configurable() {
            if descriptor.setter_present() && descriptor.setter().encode() != current.setter().encode() {
                return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_SETTER_ERROR);
            }
            if descriptor.getter_present() && descriptor.getter().encode() != current.getter().encode() {
                return type_error(throw_exception, UNCONFIGURABLE_PROPERTY_CHANGE_GETTER_ERROR);
            }

            return Ok(true);
        }
    }

    let Some(object) = object else {
        return Ok(true);
    };
    // Step 9.
    let attributes = descriptor.attributes_overriding_current(current);
    if descriptor.is_accessor_descriptor() || (current.is_accessor_descriptor() && !descriptor.is_data_descriptor()) {
        debug_assert!(attributes & ACCESSOR != 0);
        let getter = if descriptor.getter_present() {
            descriptor.getter()
        } else if current.getter_present() {
            current.getter()
        } else {
            JSValue::undefined()
        };
        let setter = if descriptor.setter_present() {
            descriptor.setter()
        } else if current.setter_present() {
            current.setter()
        } else {
            JSValue::undefined()
        };
        let getter_setter = GetterSetter::create_from_values(vm, getter, setter);
        object.put_direct_accessor(vm, property_name, getter_setter, attributes & !READ_ONLY)?;
    } else {
        debug_assert!(descriptor.is_generic_descriptor() || descriptor.is_data_descriptor());
        let value = if !descriptor.value().is_empty() {
            descriptor.value()
        } else if !current.value().is_empty() {
            current.value()
        } else {
            JSValue::undefined()
        };
        object.put_direct(vm, property_name, value, attributes & !ACCESSOR);
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(vm: &VM, text: &[u8]) -> PropertyName {
        PropertyName::from_identifier(&Identifier::from_span(vm, text))
    }

    fn plain_object(vm: &VM) -> JSObjectRef {
        let structure = JSFinalObject::create_structure(vm, None, JSValue::null(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
        JSFinalObject::create(vm, &structure)
    }

    #[test]
    fn final_object_capacities() {
        assert_eq!(JSFinalObject::DEFAULT_INLINE_CAPACITY, 6);
        assert_eq!(JSFinalObject::MAX_INLINE_CAPACITY, 62);
    }

    #[test]
    fn empty_object_has_type_and_class() {
        let vm = VM::new();
        let object = plain_object(&vm);
        assert_eq!(object.type_(), JSType::FinalObjectType);
        assert_eq!(object.cell().class_name(), "Object");
        assert!(object.cell().is_object());
        assert!(JSObject::from_value(&object.as_value()).is_some());
        assert!(JSObject::from_value(&JSValue::Int32(1)).is_none());
    }

    #[test]
    fn put_direct_and_get_like_object_literal() {
        // var o = {a: 1}; o.a
        let vm = VM::new();
        let object = plain_object(&vm);
        let a = name(&vm, b"a");
        assert!(object.put_direct(&vm, &a, JSValue::Int32(1), 0));
        assert_eq!(object.get(&vm, &a), JSValue::Int32(1));
        assert_eq!(object.get(&vm, &name(&vm, b"b")), JSValue::undefined());
        assert!(object.has_own_property(&vm, &a));
        assert!(!object.has_own_property(&vm, &name(&vm, b"b")));
    }

    #[test]
    fn same_shape_objects_share_structure() {
        let vm = VM::new();
        let structure = JSFinalObject::create_structure(&vm, None, JSValue::null(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
        let first = JSFinalObject::create(&vm, &structure);
        let second = JSFinalObject::create(&vm, &structure);
        let a = name(&vm, b"a");
        let b = name(&vm, b"b");
        first.put_direct(&vm, &a, JSValue::Int32(1), 0);
        first.put_direct(&vm, &b, JSValue::Int32(2), 0);
        second.put_direct(&vm, &a, JSValue::Int32(3), 0);
        second.put_direct(&vm, &b, JSValue::Int32(4), 0);
        assert!(Rc::ptr_eq(&first.structure(), &second.structure()));
        assert_eq!(first.get(&vm, &b), JSValue::Int32(2));
        assert_eq!(second.get(&vm, &a), JSValue::Int32(3));
    }

    #[test]
    fn out_of_line_properties_grow_the_butterfly() {
        let vm = VM::new();
        let structure = JSFinalObject::create_structure(&vm, None, JSValue::null(), 2);
        let object = JSFinalObject::create(&vm, &structure);
        for i in 0..10 {
            let property = name(&vm, format!("p{i}").as_bytes());
            assert!(object.put_direct(&vm, &property, JSValue::Int32(i), 0));
        }
        for i in 0..10 {
            let property = name(&vm, format!("p{i}").as_bytes());
            assert_eq!(object.get(&vm, &property), JSValue::Int32(i));
        }
        assert_eq!(object.structure().max_offset(), 64 + 7);
        assert_eq!(object.structure().out_of_line_capacity(), 8);
    }

    #[test]
    fn put_updates_existing_and_walks_the_prototype_chain() {
        let vm = VM::new();
        let proto = plain_object(&vm);
        let inherited = name(&vm, b"inherited");
        proto.put_direct(&vm, &inherited, JSValue::Int32(7), 0);

        let structure =
            JSFinalObject::create_structure(&vm, None, proto.as_value(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
        let object = JSFinalObject::create(&vm, &structure);
        assert_eq!(object.get(&vm, &inherited), JSValue::Int32(7));
        assert!(proto.may_be_prototype());

        let mut slot = PutPropertySlot::new(object.as_value(), true, PutContext::UnknownContext, false);
        assert_eq!(object.put(&vm, &inherited, JSValue::Int32(8), &mut slot), Ok(true));
        // A escrita cria a propriedade própria e não altera o protótipo.
        assert_eq!(object.get(&vm, &inherited), JSValue::Int32(8));
        assert_eq!(proto.get(&vm, &inherited), JSValue::Int32(7));

        let mut slot = PutPropertySlot::new(object.as_value(), true, PutContext::UnknownContext, false);
        assert_eq!(object.put(&vm, &inherited, JSValue::Int32(9), &mut slot), Ok(true));
        assert_eq!(object.get(&vm, &inherited), JSValue::Int32(9));
    }

    #[test]
    fn read_only_property_rejects_put() {
        let vm = VM::new();
        let object = plain_object(&vm);
        let fixed = name(&vm, b"fixed");
        object.put_direct(&vm, &fixed, JSValue::Int32(1), READ_ONLY);

        let mut slot = PutPropertySlot::new(object.as_value(), true, PutContext::UnknownContext, false);
        assert_eq!(object.put(&vm, &fixed, JSValue::Int32(2), &mut slot), Err(PutError::TypeError(READONLY_PROPERTY_WRITE_ERROR)));
        let mut slot = PutPropertySlot::new(object.as_value(), false, PutContext::UnknownContext, false);
        assert_eq!(object.put(&vm, &fixed, JSValue::Int32(2), &mut slot), Ok(false));
        assert_eq!(object.get(&vm, &fixed), JSValue::Int32(1));
    }

    #[test]
    fn indexed_put_converts_shapes() {
        let vm = VM::new();
        let object = plain_object(&vm);
        assert_eq!(object.put_by_index(&vm, 0, JSValue::Int32(10), true), Ok(true));
        assert_eq!(object.cell().indexing_type(), INT32_SHAPE);
        assert_eq!(object.put_by_index(&vm, 1, JSValue::Int32(20), true), Ok(true));
        assert_eq!(object.public_length(), 2);

        // Um double converte Int32 para Double.
        assert_eq!(object.put_by_index(&vm, 2, JSValue::double_number(1.5), true), Ok(true));
        assert_eq!(object.cell().indexing_type(), DOUBLE_SHAPE);
        assert_eq!(object.get_by_index(&vm, 0), JSValue::double_number(10.0));
        assert_eq!(object.get_by_index(&vm, 2), JSValue::double_number(1.5));

        // Um não número converte Double para Contiguous.
        let text = JSValue::Bool(true);
        assert_eq!(object.put_by_index(&vm, 3, text, true), Ok(true));
        assert_eq!(object.cell().indexing_type(), CONTIGUOUS_SHAPE);
        assert_eq!(object.get_by_index(&vm, 3), text);
        assert_eq!(object.public_length(), 4);
        assert!(object.has_own_property_by_index(&vm, 3));
        assert!(!object.has_own_property_by_index(&vm, 4));
        assert_eq!(object.get_by_index(&vm, 4), JSValue::undefined());
    }

    #[test]
    fn indexed_name_goes_through_put_by_index() {
        let vm = VM::new();
        let object = plain_object(&vm);
        let mut slot = PutPropertySlot::new(object.as_value(), false, PutContext::UnknownContext, false);
        assert_eq!(object.put(&vm, &name(&vm, b"2"), JSValue::Int32(5), &mut slot), Ok(true));
        assert_eq!(object.get(&vm, &name(&vm, b"2")), JSValue::Int32(5));
        assert_eq!(object.public_length(), 3);
        assert!(!object.has_own_property_by_index(&vm, 0));
    }

    #[test]
    fn far_index_needs_array_storage() {
        let vm = VM::new();
        let object = plain_object(&vm);
        assert_eq!(object.put_by_index(&vm, 4_000_000, JSValue::Int32(1), true), Ok(true));
        assert_eq!(object.cell().indexing_type(), ARRAY_STORAGE_SHAPE);
        assert_eq!(object.public_length(), 4_000_001);
        assert_eq!(object.get_by_index(&vm, 4_000_000), JSValue::Int32(1));
        assert!(!object.has_own_property_by_index(&vm, 3_999_999));
        assert_eq!(object.delete_property_by_index(&vm, 4_000_000), Ok(true));
        assert!(!object.has_own_property_by_index(&vm, 4_000_000));
    }

    #[test]
    fn set_prototype_direct_changes_lookup() {
        let vm = VM::new();
        let proto = plain_object(&vm);
        let key = name(&vm, b"k");
        proto.put_direct(&vm, &key, JSValue::Int32(3), 0);
        let object = plain_object(&vm);
        assert_eq!(object.get(&vm, &key), JSValue::undefined());
        object.set_prototype_direct(&vm, proto.as_value());
        assert_eq!(object.get_prototype_direct(), proto.as_value());
        assert_eq!(object.get(&vm, &key), JSValue::Int32(3));
    }

    #[test]
    fn dictionary_object_adds_replaces_deletes_and_flattens() {
        let vm = VM::new();
        let object = plain_object(&vm);
        let a = name(&vm, b"a");
        let b = name(&vm, b"b");
        assert!(object.put_direct(&vm, &a, JSValue::Int32(1), 0));

        object.convert_to_dictionary(&vm);
        assert!(object.structure().is_cacheable_dictionary());
        assert_eq!(object.get(&vm, &a), JSValue::Int32(1));

        assert!(object.put_direct(&vm, &b, JSValue::Int32(2), 0));
        assert!(object.put_direct(&vm, &a, JSValue::Int32(3), 0));
        assert_eq!(object.get(&vm, &a), JSValue::Int32(3));
        assert_eq!(object.get(&vm, &b), JSValue::Int32(2));

        object.convert_to_uncacheable_dictionary(&vm);
        assert!(object.structure().is_uncacheable_dictionary());
        let mut slot = DeletePropertySlot::default();
        assert_eq!(object.delete_property(&vm, &a, &mut slot), Ok(true));
        assert!(!object.has_own_property(&vm, &a));
        assert_eq!(object.get(&vm, &b), JSValue::Int32(2));

        object.flatten_dictionary_object(&vm);
        assert!(!object.structure().is_dictionary());
        assert_eq!(object.get(&vm, &b), JSValue::Int32(2));
        assert_eq!(object.structure().get(&vm, &b), 0);
    }

    #[test]
    fn delete_property_uses_a_removal_transition() {
        let vm = VM::new();
        let object = plain_object(&vm);
        let (a, b, c) = (name(&vm, b"a"), name(&vm, b"b"), name(&vm, b"c"));
        assert!(object.put_direct(&vm, &a, JSValue::Int32(1), 0));
        assert!(object.put_direct(&vm, &b, JSValue::Int32(2), 0));
        assert!(object.put_direct(&vm, &c, JSValue::Int32(3), DONT_DELETE));

        let mut slot = DeletePropertySlot::default();
        assert_eq!(object.delete_property(&vm, &a, &mut slot), Ok(true));
        assert!(slot.is_delete_hit());
        assert!(!object.has_own_property(&vm, &a));
        assert_eq!(object.get(&vm, &b), JSValue::Int32(2));

        let mut slot = DeletePropertySlot::default();
        assert_eq!(object.delete_property(&vm, &name(&vm, b"zz"), &mut slot), Ok(true));
        assert!(slot.is_configurable_delete_miss());

        let mut slot = DeletePropertySlot::default();
        assert_eq!(object.delete_property(&vm, &c, &mut slot), Ok(false));
        assert!(slot.is_nonconfigurable());
        assert_eq!(object.get(&vm, &c), JSValue::Int32(3));
    }

    #[test]
    fn define_own_property_data_and_accessor_branches() {
        let vm = VM::new();
        let object = plain_object(&vm);

        // Dado novo, com os atributos padrão do descritor (somente leitura, não enumerável, não configurável).
        let a = name(&vm, b"a");
        let mut data = PropertyDescriptor::default();
        data.set_value(JSValue::Int32(7));
        assert_eq!(object.define_own_property(&vm, &a, &data, true), Ok(true));
        assert_eq!(object.get(&vm, &a), JSValue::Int32(7));

        // O mesmo valor passa; outro valor numa propriedade não configurável e somente leitura não.
        assert_eq!(object.define_own_property(&vm, &a, &data, true), Ok(true));
        data.set_value(JSValue::Int32(8));
        assert_eq!(
            object.define_own_property(&vm, &a, &data, true),
            Err(PutError::TypeError(READONLY_PROPERTY_CHANGE_ERROR))
        );
        assert_eq!(object.define_own_property(&vm, &a, &data, false), Ok(false));
        assert_eq!(object.get(&vm, &a), JSValue::Int32(7));

        // Accessor novo.
        let b = name(&vm, b"b");
        let getter = plain_object(&vm);
        let mut accessor = PropertyDescriptor::default();
        accessor.set_getter(getter.as_value());
        accessor.set_enumerable(true);
        accessor.set_configurable(true);
        assert_eq!(object.define_own_property(&vm, &b, &accessor, true), Ok(true));

        let mut found = PropertyDescriptor::default();
        assert!(object.get_own_property_descriptor(&vm, &b, &mut found));
        assert!(found.is_accessor_descriptor() && found.configurable() && found.enumerable());
        assert_eq!(found.getter(), getter.as_value());
        assert!(found.setter().is_undefined());

        // Acrescentar o setter mantém o getter (passo 9, ramo de accessor).
        let setter = plain_object(&vm);
        let mut only_setter = PropertyDescriptor::default();
        only_setter.set_setter(setter.as_value());
        assert_eq!(object.define_own_property(&vm, &b, &only_setter, true), Ok(true));
        let mut found = PropertyDescriptor::default();
        assert!(object.get_own_property_descriptor(&vm, &b, &mut found));
        assert_eq!(found.getter(), getter.as_value());
        assert_eq!(found.setter(), setter.as_value());

        // Objeto não extensível não ganha propriedade nova.
        object.set_structure(&vm, &Structure::prevent_extensions_transition(&vm, &object.structure()));
        assert_eq!(
            object.define_own_property(&vm, &name(&vm, b"c"), &data, true),
            Err(PutError::TypeError(NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR))
        );
    }
}
