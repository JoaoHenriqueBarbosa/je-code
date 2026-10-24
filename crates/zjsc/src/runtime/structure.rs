//! Tradução de `runtime/Structure.{h,cpp}`, `StructureInlines.h`, `StructureInlinesLight.h` e
//! `StructureCreateInlines.h`: a forma dos objetos, as transições por propriedade e as transições que
//! não são de propriedade (indexing type, `preventExtensions`, `seal`, `freeze`, `becomePrototype`,
//! `changePrototype`), com as constantes de `DEFINE_BITFIELD` que o bytecompiler já consultava.
//!
//! DIVERGÊNCIAS (sem heap, sem GC, sem JIT; camada 3):
//!
//! - `Structure` não é `JSCell` com `structureStructure`: é um valor compartilhado por `Rc`
//!   (`StructureRef`), e o `StructureID` é um número sequencial por thread (`id()`), sem o nuke/encode
//!   de ponteiro. As células guardam o `StructureRef` direto (`js_cell::JSCell`).
//! - A tabela de propriedades é guardada por estrutura e copiada a cada transição de propriedade (o C++
//!   a "toma" da estrutura anterior ou a rematerializa pela cadeia de `m_previous`/`m_transitionPropertyName`
//!   a partir do `StructureRareData`/`PropertyTable` com `materializePropertyTable`). A cadeia `previous`
//!   é mantida (contagem de transições), mas a rematerialização some porque a tabela nunca é descartada.
//! - Modo dicionário: `toDictionaryTransition` (cacheável e não cacheável), `removePropertyTransition`,
//!   `attributeChangeTransition` em dicionário, `add/remove/attributeChangeWithoutTransition`,
//!   `addOrReplacePropertyWithoutTransition`, `flattenDictionaryStructure` e os limiares
//!   `shouldDoCacheableDictionaryTransitionFor*` (128/512 transições para adicionar, 4096 para remover e
//!   mudar atributo). O `pin` do C++ também zera `m_previousOrRareData` e `m_transitionPropertyName`: isso
//!   só é feito nas estruturas novas das transições (`pin_local`); numa estrutura que já está na cadeia
//!   (`addPropertyWithoutTransition` sobre uma estrutura que não é dicionário) o `pin` só liga o bit, porque
//!   `previous` e o nome da transição são imutáveis (a rematerialização da tabela que os usaria não existe).
//!   As funções `*WithoutTransition` não recebem o callback `func` do C++: o `setMaxOffset` acontece dentro
//!   delas, e quem precisa do `newMaxOffset` (o `JSObject`, para crescer o butterfly) lê `maxOffset()` depois.
//!   `didReplaceProperty` (watchpoint de substituição) não existe.
//! - Fora desta fatia: `StructureRareData` (cache de enumerador, watchpoints de substituição), `Watchpoint`/`DeferredStructure
//!   TransitionWatchpointFire`, `m_seenProperties`/`m_propertyHash`, poly proto, `StructureChain`,
//!   `toStructureShape`,
//!   `getPropertyNamesFromStructure` e `dump`. O `m_realm` é o `cell_id` do `JSGlobalObject`, resolvido
//!   por `realm()` no `cell_registry`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::runtime::branded_structure::BrandedStructure;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::indexing_type::{
    has_any_array_storage, has_indexed_properties, is_copy_on_write, IndexingType, ALL_ARRAY_TYPES, ALL_WRITABLE_ARRAY_TYPES, COPY_ON_WRITE,
    MAY_HAVE_INDEXED_ACCESSORS, NON_ARRAY,
};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{
    ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE, DONT_DELETE, DONT_ENUM, READ_ONLY, READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE,
};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::{
    check_offset_with_inline_capacity, is_valid_offset, number_of_out_of_line_slots_for_max_offset,
    number_of_slots_for_max_offset, offset_for_property_number, validate_offset_with_inline_capacity, PropertyOffset,
    FIRST_OUT_OF_LINE_OFFSET, INVALID_OFFSET,
};
use crate::runtime::property_table::{PropertyTable, PropertyTableEntry};
use crate::runtime::put_property_slot::PutContext;
use crate::runtime::structure_transition_table::{
    changes_indexing_type, new_indexing_type, prevents_extensions, sets_dont_delete_on_all_properties,
    sets_read_only_on_non_accessor_properties, PointerKey, StructureTransitionTable, TransitionKind,
};
use crate::bytecode::watchpoint::{InlineWatchpointSet, StringFireDetail, WatchpointRef, WatchpointSet, WatchpointSetRef, WatchpointState};
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;

/// `Structure*`.
pub type StructureRef = Rc<Structure>;

/// `Structure::initialOutOfLineCapacity`.
pub const INITIAL_OUT_OF_LINE_CAPACITY: u32 = 4;
/// `Structure::outOfLineGrowthFactor`.
pub const OUT_OF_LINE_GROWTH_FACTOR: u32 = 2;

/// `Structure::s_maxTransitionLength`.
pub const MAX_TRANSITION_LENGTH: i32 = 128;
/// `Structure::s_maxTransitionLengthForNonEvalPutById`.
pub const MAX_TRANSITION_LENGTH_FOR_NON_EVAL_PUT_BY_ID: i32 = 512;
/// `Structure::s_maxTransitionLengthForRemove`.
pub const MAX_TRANSITION_LENGTH_FOR_REMOVE: i32 = 4096;

/// `Structure::DictionaryKind`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictionaryKind {
    NoneDictionaryKind = 0,
    CachedDictionaryKind = 1,
    UncachedDictionaryKind = 2,
}

/// `class Structure`.
pub struct Structure {
    /// `id()`: o `StructureID`, sequencial por thread (começa em 1).
    id: u32,
    /// `m_blob`/`m_outOfLineTypeFlags`: o `TypeInfo`.
    type_info: TypeInfo,
    /// `m_classInfo`.
    class_info: &'static ClassInfo,
    /// `m_prototype` (mono proto).
    prototype: JSValue,
    /// `m_blob.indexingModeIncludingHistory()`.
    indexing_mode_including_history: IndexingType,
    /// `m_inlineCapacity`.
    inline_capacity: u8,
    /// `m_bitField`.
    bit_field: Cell<u32>,
    /// `m_transitionPropertyName`.
    transition_property_name: Option<UniquedKey>,
    /// `m_transitionPropertyName` é privado (o `SymbolImpl::isPrivate()` que o `StringImpl` não guarda).
    transition_property_is_private: bool,
    /// `transitionPropertyAttributes()`.
    transition_property_attributes: u8,
    /// `transitionKind()`.
    transition_kind: TransitionKind,
    /// `previousID()`.
    previous: Option<StructureRef>,
    /// `m_realm`: o `cell_id` do `JSGlobalObject` (0 é o `nullptr`).
    realm: Cell<usize>,
    /// `maxOffset()`.
    max_offset: Cell<PropertyOffset>,
    /// `transitionOffset()`.
    transition_offset: Cell<PropertyOffset>,
    /// `m_propertyTableUnsafe`: `None` é a tabela vazia ainda não criada.
    property_table: RefCell<Option<PropertyTable>>,
    /// `m_transitionTable`.
    transition_table: RefCell<StructureTransitionTable>,
    /// `variant() == StructureVariant::Branded`: o `m_brand` e o `m_parentBrand` do `BrandedStructure`
    /// (a subclasse do C++ vira dado aqui, porque o `Structure` não é polimórfico).
    branded: Option<BrandedStructure>,
    /// `m_transitionWatchpointSet`: nasce `IsWatched` nos três construtores do C++.
    transition_watchpoint_set: RefCell<InlineWatchpointSet>,
    /// `StructureRareData::m_replacementWatchpointSets`: um set por offset vigiado para substituição.
    replacement_watchpoint_sets: RefCell<HashMap<PropertyOffset, WatchpointSetRef>>,
    /// `StructureRareData::m_activeReplacementWatchpointSet` (que sustenta o bit `isWatchingReplacement`).
    active_replacement_watchpoint_set_count: Cell<u32>,
}

/// A recursão pelas tabelas de transição daria um laço (pai, filho, `previous`): só o essencial.
impl fmt::Debug for Structure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Structure")
            .field("id", &self.id)
            .field("class_name", &self.class_info.class_name)
            .field("max_offset", &self.max_offset.get())
            .finish()
    }
}

thread_local! {
    /// O contador que dá o `StructureID`.
    static NEXT_STRUCTURE_ID: Cell<u32> = const { Cell::new(1) };
}

fn next_structure_id() -> u32 {
    NEXT_STRUCTURE_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

/// `DEFINE_BITFIELD(type, lowerName, upperName, width, offset)`: `s_xShift`, `s_xMask`, `s_xBits` e
/// `s_bitWidthOfX`. A máscara do C++ é `(1 << (width - 1)) | ((1 << (width - 1)) - 1)`.
macro_rules! define_bitfield {
    ($($shift:ident, $mask:ident, $bits:ident, $width:ident, $w:expr, $offset:expr;)*) => {
        impl Structure {
            $(
                pub const $shift: u32 = $offset;
                pub const $mask: u32 = (1u32 << ($w - 1)) | ((1u32 << ($w - 1)) - 1);
                pub const $bits: u32 = Self::$mask << Self::$shift;
                pub const $width: u32 = $w;
            )*
        }
    };
}

define_bitfield! {
    DICTIONARY_KIND_SHIFT, DICTIONARY_KIND_MASK, DICTIONARY_KIND_BITS, BIT_WIDTH_OF_DICTIONARY_KIND, 2, 0;
    IS_PINNED_PROPERTY_TABLE_SHIFT, IS_PINNED_PROPERTY_TABLE_MASK, IS_PINNED_PROPERTY_TABLE_BITS, BIT_WIDTH_OF_IS_PINNED_PROPERTY_TABLE, 1, 2;
    HAS_ANY_KIND_OF_GETTER_SETTER_PROPERTIES_SHIFT, HAS_ANY_KIND_OF_GETTER_SETTER_PROPERTIES_MASK, HAS_ANY_KIND_OF_GETTER_SETTER_PROPERTIES_BITS, BIT_WIDTH_OF_HAS_ANY_KIND_OF_GETTER_SETTER_PROPERTIES, 1, 3;
    HAS_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_EXCLUDING_PROTO_SHIFT, HAS_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_EXCLUDING_PROTO_MASK, HAS_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_EXCLUDING_PROTO_BITS, BIT_WIDTH_OF_HAS_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_EXCLUDING_PROTO, 1, 4;
    IS_QUICK_PROPERTY_ACCESS_ALLOWED_FOR_ENUMERATION_SHIFT, IS_QUICK_PROPERTY_ACCESS_ALLOWED_FOR_ENUMERATION_MASK, IS_QUICK_PROPERTY_ACCESS_ALLOWED_FOR_ENUMERATION_BITS, BIT_WIDTH_OF_IS_QUICK_PROPERTY_ACCESS_ALLOWED_FOR_ENUMERATION, 1, 5;
    HAS_NON_ENUMERABLE_PROPERTIES_SHIFT, HAS_NON_ENUMERABLE_PROPERTIES_MASK, HAS_NON_ENUMERABLE_PROPERTIES_BITS, BIT_WIDTH_OF_HAS_NON_ENUMERABLE_PROPERTIES, 1, 6;
    HAS_SPECIAL_PROPERTIES_SHIFT, HAS_SPECIAL_PROPERTIES_MASK, HAS_SPECIAL_PROPERTIES_BITS, BIT_WIDTH_OF_HAS_SPECIAL_PROPERTIES, 1, 7;
    DEFINITELY_NON_THENABLE_STATE_SHIFT, DEFINITELY_NON_THENABLE_STATE_MASK, DEFINITELY_NON_THENABLE_STATE_BITS, BIT_WIDTH_OF_DEFINITELY_NON_THENABLE_STATE, 2, 8;
    TRANSITION_KIND_SHIFT, TRANSITION_KIND_MASK, TRANSITION_KIND_BITS, BIT_WIDTH_OF_TRANSITION_KIND, 5, 13;
    IS_WATCHING_REPLACEMENT_SHIFT, IS_WATCHING_REPLACEMENT_MASK, IS_WATCHING_REPLACEMENT_BITS, BIT_WIDTH_OF_IS_WATCHING_REPLACEMENT, 1, 18;
    MAY_BE_PROTOTYPE_SHIFT, MAY_BE_PROTOTYPE_MASK, MAY_BE_PROTOTYPE_BITS, BIT_WIDTH_OF_MAY_BE_PROTOTYPE, 1, 19;
    DID_PREVENT_EXTENSIONS_SHIFT, DID_PREVENT_EXTENSIONS_MASK, DID_PREVENT_EXTENSIONS_BITS, BIT_WIDTH_OF_DID_PREVENT_EXTENSIONS, 1, 20;
    DID_TRANSITION_SHIFT, DID_TRANSITION_MASK, DID_TRANSITION_BITS, BIT_WIDTH_OF_DID_TRANSITION, 1, 21;
    STATIC_PROPERTIES_REIFIED_SHIFT, STATIC_PROPERTIES_REIFIED_MASK, STATIC_PROPERTIES_REIFIED_BITS, BIT_WIDTH_OF_STATIC_PROPERTIES_REIFIED, 1, 22;
    HAS_BEEN_FLATTENED_BEFORE_SHIFT, HAS_BEEN_FLATTENED_BEFORE_MASK, HAS_BEEN_FLATTENED_BEFORE_BITS, BIT_WIDTH_OF_HAS_BEEN_FLATTENED_BEFORE, 1, 23;
    DID_WATCH_INTERNAL_PROPERTIES_SHIFT, DID_WATCH_INTERNAL_PROPERTIES_MASK, DID_WATCH_INTERNAL_PROPERTIES_BITS, BIT_WIDTH_OF_DID_WATCH_INTERNAL_PROPERTIES, 1, 24;
    TRANSITION_WATCHPOINT_IS_LIKELY_TO_BE_FIRED_SHIFT, TRANSITION_WATCHPOINT_IS_LIKELY_TO_BE_FIRED_MASK, TRANSITION_WATCHPOINT_IS_LIKELY_TO_BE_FIRED_BITS, BIT_WIDTH_OF_TRANSITION_WATCHPOINT_IS_LIKELY_TO_BE_FIRED, 1, 25;
    HAS_BEEN_DICTIONARY_SHIFT, HAS_BEEN_DICTIONARY_MASK, HAS_BEEN_DICTIONARY_BITS, BIT_WIDTH_OF_HAS_BEEN_DICTIONARY, 1, 26;
    PROTECT_PROPERTY_TABLE_WHILE_TRANSITIONING_SHIFT, PROTECT_PROPERTY_TABLE_WHILE_TRANSITIONING_MASK, PROTECT_PROPERTY_TABLE_WHILE_TRANSITIONING_BITS, BIT_WIDTH_OF_PROTECT_PROPERTY_TABLE_WHILE_TRANSITIONING, 1, 27;
    HAS_UNDERSCORE_PROTO_PROPERTY_EXCLUDING_ORIGINAL_PROTO_SHIFT, HAS_UNDERSCORE_PROTO_PROPERTY_EXCLUDING_ORIGINAL_PROTO_MASK, HAS_UNDERSCORE_PROTO_PROPERTY_EXCLUDING_ORIGINAL_PROTO_BITS, BIT_WIDTH_OF_HAS_UNDERSCORE_PROTO_PROPERTY_EXCLUDING_ORIGINAL_PROTO, 1, 28;
    HAS_NON_CONFIGURABLE_PROPERTIES_SHIFT, HAS_NON_CONFIGURABLE_PROPERTIES_MASK, HAS_NON_CONFIGURABLE_PROPERTIES_BITS, BIT_WIDTH_OF_HAS_NON_CONFIGURABLE_PROPERTIES, 1, 29;
    HAS_NON_CONFIGURABLE_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_SHIFT, HAS_NON_CONFIGURABLE_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_MASK, HAS_NON_CONFIGURABLE_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_BITS, BIT_WIDTH_OF_HAS_NON_CONFIGURABLE_READ_ONLY_OR_GETTER_SETTER_PROPERTIES, 1, 30;
}

/// Os pares `isX`/`setX` dos bits de um bit só do `m_bitField`.
macro_rules! bool_bit_accessors {
    ($($get:ident, $set:ident, $bits:ident;)*) => {
        impl Structure {
            $(
                pub fn $get(&self) -> bool {
                    self.bit_field.get() & Self::$bits != 0
                }

                pub fn $set(&self, value: bool) {
                    let field = self.bit_field.get();
                    self.bit_field.set(if value { field | Self::$bits } else { field & !Self::$bits });
                }
            )*
        }
    };
}

bool_bit_accessors! {
    is_pinned_property_table, set_is_pinned_property_table, IS_PINNED_PROPERTY_TABLE_BITS;
    has_any_kind_of_getter_setter_properties, set_has_any_kind_of_getter_setter_properties, HAS_ANY_KIND_OF_GETTER_SETTER_PROPERTIES_BITS;
    has_read_only_or_getter_setter_properties_excluding_proto, set_has_read_only_or_getter_setter_properties_excluding_proto, HAS_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_EXCLUDING_PROTO_BITS;
    is_quick_property_access_allowed_for_enumeration, set_is_quick_property_access_allowed_for_enumeration, IS_QUICK_PROPERTY_ACCESS_ALLOWED_FOR_ENUMERATION_BITS;
    has_non_enumerable_properties, set_has_non_enumerable_properties, HAS_NON_ENUMERABLE_PROPERTIES_BITS;
    has_special_properties, set_has_special_properties, HAS_SPECIAL_PROPERTIES_BITS;
    may_be_prototype, set_may_be_prototype, MAY_BE_PROTOTYPE_BITS;
    did_prevent_extensions, set_did_prevent_extensions, DID_PREVENT_EXTENSIONS_BITS;
    did_transition, set_did_transition, DID_TRANSITION_BITS;
    transition_watchpoint_is_likely_to_be_fired, set_transition_watchpoint_is_likely_to_be_fired, TRANSITION_WATCHPOINT_IS_LIKELY_TO_BE_FIRED_BITS;
    static_properties_reified, set_static_properties_reified, STATIC_PROPERTIES_REIFIED_BITS;
    has_been_flattened_before, set_has_been_flattened_before, HAS_BEEN_FLATTENED_BEFORE_BITS;
    has_been_dictionary, set_has_been_dictionary, HAS_BEEN_DICTIONARY_BITS;
    has_underscore_proto_property_excluding_original_proto, set_has_underscore_proto_property_excluding_original_proto, HAS_UNDERSCORE_PROTO_PROPERTY_EXCLUDING_ORIGINAL_PROTO_BITS;
    has_non_configurable_properties, set_has_non_configurable_properties, HAS_NON_CONFIGURABLE_PROPERTIES_BITS;
    has_non_configurable_read_only_or_getter_setter_properties, set_has_non_configurable_read_only_or_getter_setter_properties, HAS_NON_CONFIGURABLE_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_BITS;
}

impl Structure {
    /// `Structure(VM&, JSGlobalObject*, JSValue prototype, const TypeInfo&, const ClassInfo*,
    /// IndexingType, unsigned inlineCapacity)`: o `JSGlobalObject*` entra depois, em
    /// `create_with_indexing_type`.
    fn new_root(
        prototype: JSValue,
        type_info: TypeInfo,
        class_info: &'static ClassInfo,
        indexing_type: IndexingType,
        inline_capacity: u32,
    ) -> Structure {
        let is_array_storage = has_any_array_storage(indexing_type);
        crate::runtime::gc::retain_prototype(prototype);

        debug_assert!((inline_capacity as PropertyOffset) < FIRST_OUT_OF_LINE_OFFSET);

        let structure = Structure {
            id: next_structure_id(),
            type_info,
            class_info,
            prototype,
            indexing_mode_including_history: indexing_type,
            inline_capacity: inline_capacity as u8,
            bit_field: Cell::new(0),
            transition_property_name: None,
            transition_property_is_private: false,
            transition_property_attributes: 0,
            transition_kind: TransitionKind::Unknown,
            previous: None,
            realm: Cell::new(0),
            max_offset: Cell::new(INVALID_OFFSET),
            transition_offset: Cell::new(INVALID_OFFSET),
            property_table: RefCell::new(None),
            transition_table: RefCell::new(StructureTransitionTable::default()),
            branded: None,
            transition_watchpoint_set: RefCell::new(InlineWatchpointSet::new(WatchpointState::IsWatched)),
            replacement_watchpoint_sets: RefCell::new(HashMap::new()),
            active_replacement_watchpoint_set_count: Cell::new(0),
        };

        structure.set_transition_watchpoint_is_likely_to_be_fired(false);
        let has_any_kind_of_getter_setter =
            class_info.has_static_property_with_any_of_attributes(ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE);
        structure.set_has_any_kind_of_getter_setter_properties(has_any_kind_of_getter_setter);
        structure.set_has_read_only_or_getter_setter_properties_excluding_proto(
            has_any_kind_of_getter_setter || class_info.has_static_property_with_any_of_attributes(READ_ONLY),
        );
        structure.set_has_non_enumerable_properties(type_info.overrides_get_own_property_slot() || is_array_storage);
        structure.set_has_special_properties(false);
        structure.set_has_non_configurable_properties(type_info.overrides_get_own_property_slot() || is_array_storage);
        structure.set_has_non_configurable_read_only_or_getter_setter_properties(
            (type_info.overrides_get_own_property_slot() && type_info.type_() != JSType::ArrayType) || is_array_storage,
        );
        structure.set_has_underscore_proto_property_excluding_original_proto(false);
        structure.set_is_quick_property_access_allowed_for_enumeration(true);
        structure.set_may_be_prototype(false);
        structure.set_did_prevent_extensions(type_info.overrides_is_extensible());
        structure.set_did_transition(false);
        structure.set_static_properties_reified(false);
        structure.set_has_been_dictionary(false);
        structure
    }

    /// `Structure(VM&, StructureVariant, Structure* previous)`: a estrutura de uma transição. A tabela
    /// de propriedades e o `maxOffset` quem preenche é a transição.
    fn new_from_previous(vm: &VM, previous: &StructureRef) -> Structure {
        debug_assert!(!previous.type_info.structure_is_immortal());

        let structure = Structure {
            id: next_structure_id(),
            type_info: previous.type_info,
            class_info: previous.class_info,
            prototype: previous.prototype,
            indexing_mode_including_history: previous.indexing_mode_including_history,
            inline_capacity: previous.inline_capacity,
            bit_field: Cell::new(0),
            transition_property_name: None,
            transition_property_is_private: false,
            transition_property_attributes: 0,
            transition_kind: TransitionKind::Unknown,
            previous: Some(Rc::clone(previous)),
            realm: Cell::new(previous.realm.get()),
            max_offset: Cell::new(INVALID_OFFSET),
            transition_offset: Cell::new(INVALID_OFFSET),
            property_table: RefCell::new(None),
            transition_table: RefCell::new(StructureTransitionTable::default()),
            // `Structure::create(vm, previous)`: a variante da anterior decide a subclasse; a de uma
            // `BrandedStructure` copia o `m_brand` e o `m_parentBrand` dela.
            branded: previous.branded.as_ref().map(BrandedStructure::copy_of),
            transition_watchpoint_set: RefCell::new(InlineWatchpointSet::new(WatchpointState::IsWatched)),
            replacement_watchpoint_sets: RefCell::new(HashMap::new()),
            active_replacement_watchpoint_set_count: Cell::new(0),
        };

        // `previous->didTransitionFromThisStructureWithoutFiringWatchpoint()`, depois copia o bit
        // `transitionWatchpointIsLikelyToBeFired` dela.
        previous.did_transition_from_this_structure_without_firing_watchpoint();
        structure.set_transition_watchpoint_is_likely_to_be_fired(previous.transition_watchpoint_is_likely_to_be_fired());
        structure.set_dictionary_kind(previous.dictionary_kind());
        structure.set_is_pinned_property_table(false);
        structure.set_has_been_flattened_before(previous.has_been_flattened_before());
        structure.set_has_any_kind_of_getter_setter_properties(previous.has_any_kind_of_getter_setter_properties());
        structure.set_has_read_only_or_getter_setter_properties_excluding_proto(
            previous.has_read_only_or_getter_setter_properties_excluding_proto(),
        );
        structure.set_has_non_enumerable_properties(previous.has_non_enumerable_properties());
        structure.set_has_special_properties(previous.has_special_properties());
        structure.set_has_non_configurable_properties(previous.has_non_configurable_properties());
        structure.set_has_non_configurable_read_only_or_getter_setter_properties(
            previous.has_non_configurable_read_only_or_getter_setter_properties(),
        );
        structure.set_has_underscore_proto_property_excluding_original_proto(
            previous.has_underscore_proto_property_excluding_original_proto(),
        );
        structure.set_is_quick_property_access_allowed_for_enumeration(
            previous.is_quick_property_access_allowed_for_enumeration(),
        );
        structure.set_may_be_prototype(previous.may_be_prototype());
        structure.set_did_prevent_extensions(previous.did_prevent_extensions());
        structure.set_did_transition(true);
        structure.set_static_properties_reified(previous.static_properties_reified());
        structure.set_has_been_dictionary(previous.has_been_dictionary());
        // `Structure::finishCreation(vm, previous, deferred)` termina com
        // `previous->fireStructureTransitionWatchpoint(deferred)` (sem `deferred`, `fireAll` direto).
        previous.fire_structure_transition_watchpoint(vm);
        structure
    }

    /// `transitionWatchpointSet()`.
    pub fn transition_watchpoint_set(&self) -> &RefCell<InlineWatchpointSet> {
        &self.transition_watchpoint_set
    }

    /// `transitionWatchpointSetHasBeenInvalidated()`.
    pub fn transition_watchpoint_set_has_been_invalidated(&self) -> bool {
        self.transition_watchpoint_set.borrow().has_been_invalidated()
    }

    /// `transitionWatchpointSetIsStillValid()`.
    pub fn transition_watchpoint_set_is_still_valid(&self) -> bool {
        self.transition_watchpoint_set.borrow().is_still_valid()
    }

    /// `addTransitionWatchpoint(Watchpoint*)`.
    pub fn add_transition_watchpoint(&self, watchpoint: Option<WatchpointRef>) {
        debug_assert!(self.transition_watchpoint_set_is_still_valid());
        self.transition_watchpoint_set.borrow_mut().add(watchpoint);
    }

    /// `didTransitionFromThisStructureWithoutFiringWatchpoint()`: se alguém vigia a estrutura, as versões
    /// futuras dela avisam que vigiá-la é imprudente.
    pub fn did_transition_from_this_structure_without_firing_watchpoint(&self) {
        if self.transition_watchpoint_set.borrow().is_being_watched() {
            self.set_transition_watchpoint_is_likely_to_be_fired(true);
        }
    }

    /// `fireStructureTransitionWatchpoint(nullptr)`: `m_transitionWatchpointSet.fireAll(vm(), StructureFireDetail(this))`.
    pub fn fire_structure_transition_watchpoint(&self, vm: &VM) {
        let detail = StringFireDetail::new("Structure transition");
        InlineWatchpointSet::fire_all_shared(&self.transition_watchpoint_set, vm, &detail);
    }

    /// `propertyReplacementWatchpointSet(offset)` (StructureInlines.h:211): `nullptr` sem o `StructureRareData`
    /// ou sem set para o offset.
    pub fn property_replacement_watchpoint_set(&self, offset: PropertyOffset) -> Option<WatchpointSetRef> {
        self.replacement_watchpoint_sets.borrow().get(&offset).cloned()
    }

    /// `ensurePropertyReplacementWatchpointSet(vm, offset)` (Structure.cpp:1129): `nullptr` para offset
    /// inválido; senão cria o set `IsWatched` e conta mais um ativo (`incrementActiveReplacementWatchpointSet`
    /// e `setIsWatchingReplacement(true)`).
    pub fn ensure_property_replacement_watchpoint_set(&self, _vm: &VM, offset: PropertyOffset) -> Option<WatchpointSetRef> {
        debug_assert!(!self.is_uncacheable_dictionary());
        // Em alguns lugares é conveniente chamar com offset inválido; por isso a checagem aqui.
        if !self.is_valid_offset(offset) {
            return None;
        }
        let mut sets = self.replacement_watchpoint_sets.borrow_mut();
        let set = sets.entry(offset).or_insert_with(|| {
            self.active_replacement_watchpoint_set_count.set(self.active_replacement_watchpoint_set_count.get() + 1);
            WatchpointSet::create(WatchpointState::IsWatched)
        });
        Some(Rc::clone(set))
    }

    /// `firePropertyReplacementWatchpointSet(vm, offset, reason)` (Structure.cpp:1151): garante o set, e se
    /// ele ainda está `IsWatched` dispara e desconta um ativo (o bit `isWatchingReplacement` cai em zero).
    pub fn fire_property_replacement_watchpoint_set(&self, vm: &VM, offset: PropertyOffset, reason: &str) -> Option<WatchpointSetRef> {
        let set = self.ensure_property_replacement_watchpoint_set(vm, offset);
        if let Some(set) = &set {
            if set.borrow().state() == WatchpointState::IsWatched {
                WatchpointSet::fire_all_shared(set, vm, &StringFireDetail::new(reason));
                self.active_replacement_watchpoint_set_count.set(self.active_replacement_watchpoint_set_count.get() - 1);
            }
        }
        set
    }

    /// `isWatchingReplacement()`: o bit do C++ sobe com o primeiro set e cai quando o contador zera.
    pub fn is_watching_replacement(&self) -> bool {
        self.active_replacement_watchpoint_set_count.get() > 0
    }

    /// `startWatchingPropertyForReplacements(vm, offset)` (StructureInlines.h:666).
    pub fn start_watching_property_for_replacements(&self, vm: &VM, offset: PropertyOffset) {
        self.ensure_property_replacement_watchpoint_set(vm, offset);
    }

    /// `startWatchingPropertyForReplacements(vm, propertyName)` (Structure.cpp:1165).
    pub fn start_watching_property_for_replacements_by_name(&self, vm: &VM, property_name: &PropertyName) {
        debug_assert!(!self.is_uncacheable_dictionary());
        self.start_watching_property_for_replacements(vm, self.get(vm, property_name));
    }

    /// `didReplaceProperty(offset)` (StructureInlines.h:200): só entra no caminho lento se algum set de
    /// substituição está sendo vigiado.
    pub fn did_replace_property(&self, vm: &VM, offset: PropertyOffset) {
        if !self.is_watching_replacement() {
            return;
        }
        self.fire_property_replacement_watchpoint_set(vm, offset, "Property did get replaced");
    }

    /// `prototypeQueriesAreCacheable()` (Structure.h:336).
    pub fn prototype_queries_are_cacheable(&self) -> bool {
        !self.type_info.prohibits_property_caching()
    }

    /// `propertyAccessesAreCacheable()` (Structure.h:341).
    pub fn property_accesses_are_cacheable(&self) -> bool {
        !self.is_uncacheable_dictionary()
            && self.prototype_queries_are_cacheable()
            && !(self.type_info.get_own_property_slot_is_impure() && !self.type_info.new_impure_property_fires_watchpoints())
    }

    /// `needImpurePropertyWatchpoint()` (Structure.h:355).
    pub fn need_impure_property_watchpoint(&self) -> bool {
        self.property_accesses_are_cacheable()
            && self.type_info.get_own_property_slot_is_impure()
            && self.type_info.new_impure_property_fires_watchpoints()
    }

    /// `didTransitionFromThisStructure(nullptr)`.
    pub fn did_transition_from_this_structure(&self, vm: &VM) {
        self.did_transition_from_this_structure_without_firing_watchpoint();
        self.fire_structure_transition_watchpoint(vm);
    }

    /// `Structure::create(VM&, JSGlobalObject*, JSValue prototype, const TypeInfo&, const ClassInfo*)`:
    /// os argumentos padrão `IndexingType = NonArray` e `inlineCapacity = 0` de `create_with_indexing_type`.
    pub fn create(
        vm: &VM,
        global_object: Option<&JSGlobalObject>,
        prototype: JSValue,
        type_info: TypeInfo,
        class_info: &'static ClassInfo,
    ) -> StructureRef {
        Structure::create_with_indexing_type(vm, global_object, prototype, type_info, class_info, NON_ARRAY, 0)
    }

    /// `Structure::create(VM&, JSGlobalObject*, JSValue prototype, const TypeInfo&, const ClassInfo*,
    /// IndexingType, unsigned inlineCapacity)` (StructureCreateInlines.h). Se o protótipo é um objeto, ele
    /// "vira protótipo" (`didBecomePrototype`).
    pub fn create_with_indexing_type(
        vm: &VM,
        global_object: Option<&JSGlobalObject>,
        prototype: JSValue,
        type_info: TypeInfo,
        class_info: &'static ClassInfo,
        indexing_type: IndexingType,
        inline_capacity: u32,
    ) -> StructureRef {
        if let Some(object) = JSObject::from_value(&prototype) {
            object.did_become_prototype(vm);
        }
        let structure = Structure::new_root(prototype, type_info, class_info, indexing_type, inline_capacity);
        if let Some(global_object) = global_object {
            structure.realm.set(global_object.cell_id());
        }
        Rc::new(structure)
    }

    /// `realm()`: o `JSGlobalObject` da estrutura, se tem (o `m_realm`).
    pub fn realm(&self) -> Option<JSGlobalObjectRef> {
        match cell_registry::get(self.realm.get())? {
            CellEntry::Scope(JSScopeRef::GlobalObject(global_object)) => Some(global_object),
            _ => None,
        }
    }

    /// `m_realm.set(vm, this, globalObject)`.
    pub fn set_realm(&self, _vm: &VM, global_object: &JSGlobalObjectRef) {
        self.realm.set(global_object.cell_id());
    }

    /// `StructureID id()`.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// `typeInfo()`.
    pub fn type_info(&self) -> TypeInfo {
        self.type_info
    }

    /// `JSType type()` do `TypeInfo`.
    pub fn type_(&self) -> JSType {
        self.type_info.type_()
    }

    /// `isObject()`.
    pub fn is_object(&self) -> bool {
        self.type_info.is_object()
    }

    /// `classInfoForCells()` e `info()`.
    pub fn class_info(&self) -> &'static ClassInfo {
        self.class_info
    }

    /// `storedPrototype()` (mono proto).
    pub fn stored_prototype(&self) -> JSValue {
        self.prototype
    }

    /// `hasMonoProto()`: não há poly proto neste porte.
    pub fn has_mono_proto(&self) -> bool {
        true
    }

    /// `hasPolyProto()`.
    pub fn has_poly_proto(&self) -> bool {
        false
    }

    /// `indexingModeIncludingHistory()`.
    pub fn indexing_mode_including_history(&self) -> IndexingType {
        self.indexing_mode_including_history
    }

    /// `indexingType()`.
    pub fn indexing_type(&self) -> IndexingType {
        self.indexing_mode_including_history & ALL_WRITABLE_ARRAY_TYPES
    }

    /// `indexingMode()`.
    pub fn indexing_mode(&self) -> IndexingType {
        self.indexing_mode_including_history & ALL_ARRAY_TYPES
    }

    /// `mayInterceptIndexedAccesses()` (StructureArrayStorageInlines.h): os acessores indexados do histórico
    /// ou o `JSGlobalObject` da estrutura já em `haveABadTime` (o protótipo dele é tratado como se tivesse
    /// acessores indexados, ver o comentário do C++).
    pub fn may_intercept_indexed_accesses(&self) -> bool {
        if self.indexing_mode_including_history & MAY_HAVE_INDEXED_ACCESSORS != 0 {
            return true;
        }
        self.realm().is_some_and(|global_object| global_object.is_having_a_bad_time())
    }

    /// `dictionaryKind()`.
    pub fn dictionary_kind(&self) -> DictionaryKind {
        match (self.bit_field.get() >> Structure::DICTIONARY_KIND_SHIFT) & Structure::DICTIONARY_KIND_MASK {
            0 => DictionaryKind::NoneDictionaryKind,
            1 => DictionaryKind::CachedDictionaryKind,
            _ => DictionaryKind::UncachedDictionaryKind,
        }
    }

    /// `isDictionary()`.
    pub fn is_dictionary(&self) -> bool {
        self.dictionary_kind() != DictionaryKind::NoneDictionaryKind
    }

    /// `setDictionaryKind(kind)`.
    fn set_dictionary_kind(&self, kind: DictionaryKind) {
        let field = self.bit_field.get() & !Structure::DICTIONARY_KIND_BITS;
        self.bit_field.set(field | ((kind as u32) << Structure::DICTIONARY_KIND_SHIFT));
    }

    /// `isUncacheableDictionary()`.
    pub fn is_uncacheable_dictionary(&self) -> bool {
        self.dictionary_kind() == DictionaryKind::UncachedDictionaryKind
    }

    /// `isCacheableDictionary()`.
    pub fn is_cacheable_dictionary(&self) -> bool {
        self.dictionary_kind() == DictionaryKind::CachedDictionaryKind
    }

    /// `transitionCountEstimate()`: o número de transições costuma ser o do último offset (fora os
    /// `delete`), então o C++ não guarda os dois.
    pub fn transition_count_estimate(&self) -> i32 {
        number_of_slots_for_max_offset(self.max_offset(), self.inline_capacity as i32) as i32
    }

    /// `transitionCountHasOverflowed()`.
    pub fn transition_count_has_overflowed(&self) -> bool {
        let mut transition_count = 0;
        let mut structure: Option<&Structure> = Some(self);
        while let Some(current) = structure {
            transition_count += 1;
            if transition_count > MAX_TRANSITION_LENGTH {
                return true;
            }
            structure = current.previous.as_deref();
        }
        false
    }

    /// `shouldDoCacheableDictionaryTransitionForAdd(context)`.
    pub fn should_do_cacheable_dictionary_transition_for_add(&self, context: PutContext) -> bool {
        let max_transition_length = if context == PutContext::PutById {
            MAX_TRANSITION_LENGTH_FOR_NON_EVAL_PUT_BY_ID
        } else {
            MAX_TRANSITION_LENGTH
        };
        self.transition_count_estimate() > max_transition_length
    }

    /// `shouldDoCacheableDictionaryTransitionForRemoveAndAttributeChange()`.
    pub fn should_do_cacheable_dictionary_transition_for_remove_and_attribute_change(&self) -> bool {
        self.transition_count_estimate() > MAX_TRANSITION_LENGTH_FOR_REMOVE || self.transition_count_has_overflowed()
    }

    /// `pin(locker, vm, table)` numa estrutura já compartilhada: liga o bit (a tabela já é desta
    /// estrutura). O `clearPreviousID()` e o `m_transitionPropertyName = nullptr` só cabem em
    /// `pin_local` (veja o cabeçalho do módulo).
    fn pin(&self) {
        self.set_is_pinned_property_table(true);
    }

    /// `pin(locker, vm, table)` numa estrutura recém-criada, ainda não compartilhada: além do bit,
    /// solta o `previous` e o nome da transição.
    fn pin_local(&mut self) {
        self.set_is_pinned_property_table(true);
        self.previous = None;
        self.transition_property_name = None;
        self.transition_property_is_private = false;
    }

    /// `copyPropertyTableForPinning(vm)`: uma cópia da tabela (vazia se a estrutura ainda não tem).
    fn copy_property_table_for_pinning(&self) -> RefCell<Option<PropertyTable>> {
        RefCell::new(Some(self.property_table.borrow().clone().unwrap_or_default()))
    }

    /// `isStructureExtensible()`.
    pub fn is_structure_extensible(&self) -> bool {
        !self.did_prevent_extensions()
    }

    /// `setHasAnyKindOfGetterSetterPropertiesWithProtoCheck(is__proto__)`.
    pub fn set_has_any_kind_of_getter_setter_properties_with_proto_check(&self, is_underscore_proto: bool) {
        self.set_has_any_kind_of_getter_setter_properties(true);
        if !is_underscore_proto {
            self.set_has_read_only_or_getter_setter_properties_excluding_proto(true);
        }
    }

    /// `setContainsReadOnlyProperties()`.
    pub fn set_contains_read_only_properties(&self) {
        self.set_has_read_only_or_getter_setter_properties_excluding_proto(true);
    }

    /// `transitionKind()`.
    pub fn transition_kind(&self) -> TransitionKind {
        self.transition_kind
    }

    /// `transitionPropertyAttributes()`.
    pub fn transition_property_attributes(&self) -> u32 {
        self.transition_property_attributes as u32
    }

    /// `transitionPropertyName()`.
    pub fn transition_property_name(&self) -> Option<&UniquedKey> {
        self.transition_property_name.as_ref()
    }

    /// `previousID()`.
    pub fn previous_id(&self) -> Option<&StructureRef> {
        self.previous.as_ref()
    }

    /// Desfaz o ciclo `Rc` pai <-> filho: a tabela de transição do pai guarda o filho (`StructureRef`
    /// forte, no C++ é `WeakGCMap`) e o filho guarda o pai em `previous`. Sem GC nenhuma das duas pontas
    /// chega a zero. Solta a tabela desta estrutura e de todos os ancestrais; só o desmonte do programa
    /// chama (`CellEntry::break_realm_links`), depois disto a árvore de transições não cresce mais.
    pub fn break_transition_links(self: &Rc<Structure>) {
        let mut current = Some(Rc::clone(self));
        while let Some(structure) = current {
            let released = structure.transition_table.borrow_mut().take_all();
            drop(released);
            current = structure.previous.clone();
        }
    }

    /// `maxOffset()`.
    pub fn max_offset(&self) -> PropertyOffset {
        self.max_offset.get()
    }

    /// `setMaxOffset(vm, offset)`.
    pub fn set_max_offset(&self, offset: PropertyOffset) {
        self.max_offset.set(offset);
    }

    /// `transitionOffset()`.
    pub fn transition_offset(&self) -> PropertyOffset {
        self.transition_offset.get()
    }

    /// `setTransitionOffset(vm, offset)`.
    pub fn set_transition_offset(&self, offset: PropertyOffset) {
        self.transition_offset.set(offset);
    }

    /// `outOfLineCapacity(PropertyOffset maxOffset)`: o crescimento do armazenamento fora de linha.
    pub fn out_of_line_capacity_for_max_offset(max_offset: PropertyOffset) -> u32 {
        let out_of_line_size = Structure::out_of_line_size_for_max_offset(max_offset);

        // This algorithm completely determines the out-of-line property storage growth algorithm.
        // The JSObject code will only trigger a resize if the value returned by this algorithm
        // changed between the new and old structure. So, it's important to keep this simple because
        // it's on a fast path.
        if out_of_line_size == 0 {
            return 0;
        }

        if out_of_line_size <= INITIAL_OUT_OF_LINE_CAPACITY {
            return INITIAL_OUT_OF_LINE_CAPACITY;
        }

        debug_assert!(out_of_line_size > INITIAL_OUT_OF_LINE_CAPACITY);
        out_of_line_size.next_power_of_two()
    }

    /// `outOfLineSize(PropertyOffset maxOffset)`.
    pub fn out_of_line_size_for_max_offset(max_offset: PropertyOffset) -> u32 {
        number_of_out_of_line_slots_for_max_offset(max_offset) as u32
    }

    /// `outOfLineCapacity()`.
    pub fn out_of_line_capacity(&self) -> u32 {
        Structure::out_of_line_capacity_for_max_offset(self.max_offset())
    }

    /// `outOfLineSize()`.
    pub fn out_of_line_size(&self) -> u32 {
        Structure::out_of_line_size_for_max_offset(self.max_offset())
    }

    /// `hasInlineCapacity()`.
    pub fn has_inline_capacity(&self) -> bool {
        self.inline_capacity != 0
    }

    /// `hasAnyOfBitFieldFlags(flags)`: `m_bitField & flags`.
    pub fn has_any_of_bit_field_flags(&self, flags: u32) -> bool {
        self.bit_field.get() & flags != 0
    }

    /// `inlineCapacity()`.
    pub fn inline_capacity(&self) -> u32 {
        self.inline_capacity as u32
    }

    /// `inlineSize()`.
    pub fn inline_size(&self) -> u32 {
        ((self.max_offset() + 1) as u32).min(self.inline_capacity as u32)
    }

    /// `totalStorageCapacity()`.
    pub fn total_storage_capacity(&self) -> u32 {
        self.out_of_line_capacity() + self.inline_capacity()
    }

    /// `totalStorageSize()`.
    pub fn total_storage_size(&self) -> u32 {
        number_of_slots_for_max_offset(self.max_offset(), self.inline_capacity as i32) as u32
    }

    /// `isValidOffset(PropertyOffset)`.
    pub fn is_valid_offset(&self, offset: PropertyOffset) -> bool {
        is_valid_offset(offset)
            && offset <= self.max_offset()
            && (offset < self.inline_capacity as PropertyOffset || offset >= FIRST_OUT_OF_LINE_OFFSET)
    }

    /// `get(vm, propertyName, attributes)`: `(offset, attributes)`; `invalidOffset` e 0 se não existir.
    pub fn get_with_attributes(&self, _vm: &VM, property_name: &PropertyName) -> (PropertyOffset, u32) {
        let Some(uid) = property_name.uid() else {
            return (INVALID_OFFSET, 0);
        };
        match &*self.property_table.borrow() {
            Some(table) => table.get(uid),
            None => (INVALID_OFFSET, 0),
        }
    }

    /// `get(vm, propertyName)`.
    pub fn get(&self, vm: &VM, property_name: &PropertyName) -> PropertyOffset {
        self.get_with_attributes(vm, property_name).0
    }

    /// `Structure::add(vm, propertyName, attributes)`: o `add<ShouldPin::No>` de StructureInlines.h
    /// com o `setMaxOffset` como callback. Devolve o offset novo.
    fn add(&self, vm: &VM, property_name: &PropertyName, attributes: u32) -> PropertyOffset {
        let uid = property_name.uid().expect("add com PropertyName nulo (ASSERT do C++)");
        debug_assert!(!is_valid_offset(self.get(vm, property_name)));

        if attributes & DONT_ENUM != 0 || property_name.is_symbol() {
            self.set_is_quick_property_access_allowed_for_enumeration(false);
        }
        if attributes & READ_ONLY != 0 {
            self.set_contains_read_only_properties();
        }
        if attributes & DONT_ENUM != 0 {
            self.set_has_non_enumerable_properties(true);
        }
        if attributes & DONT_DELETE != 0 {
            self.set_has_non_configurable_properties(true);
            if attributes & READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE != 0 {
                self.set_has_non_configurable_read_only_or_getter_setter_properties(true);
            }
        }
        if *property_name == vm.property_names.underscore_proto {
            self.set_has_underscore_proto_property_excluding_original_proto(true);
        } else if *property_name == vm.property_names.then {
            self.set_has_special_properties(true);
        }

        let mut slot = self.property_table.borrow_mut();
        let table = slot.get_or_insert_with(PropertyTable::default);

        let new_offset = table.next_offset(self.inline_capacity as PropertyOffset);

        let (offset, _attributes, result) =
            table.add(PropertyTableEntry::new(uid.clone(), new_offset, attributes, property_name.is_private_name()));
        debug_assert!(result);
        debug_assert!(offset == new_offset);
        let _ = (offset, result);
        let new_max_offset = new_offset.max(self.max_offset());

        self.set_max_offset(new_max_offset);
        debug_assert!(self.max_offset() == new_max_offset);
        new_offset
    }

    /// `Structure::attributeChange(vm, propertyName, attributes)`.
    fn attribute_change(&self, vm: &VM, property_name: &PropertyName, attributes: u32) -> PropertyOffset {
        let uid = property_name.uid().expect("attributeChange com PropertyName nulo (ASSERT do C++)");
        debug_assert!(is_valid_offset(self.get(vm, property_name)));

        let offset = {
            let mut slot = self.property_table.borrow_mut();
            let table = slot.get_or_insert_with(PropertyTable::default);
            table.update_attribute_if_exists(uid, attributes)
        };
        if offset == INVALID_OFFSET {
            return offset;
        }

        if attributes & DONT_ENUM != 0 {
            self.set_has_non_enumerable_properties(true);
            self.set_is_quick_property_access_allowed_for_enumeration(false);
        }
        if attributes & DONT_DELETE != 0 {
            self.set_has_non_configurable_properties(true);
            if attributes & READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE != 0 {
                self.set_has_non_configurable_read_only_or_getter_setter_properties(true);
            }
        }
        if attributes & READ_ONLY != 0 {
            self.set_contains_read_only_properties();
        }

        let new_max_offset = self.max_offset();
        self.set_max_offset(new_max_offset);
        offset
    }

    /// `addPropertyTransitionToExistingStructure(structure, propertyName, attributes, offset)`.
    pub fn add_property_transition_to_existing_structure(
        structure: &StructureRef,
        property_name: &PropertyName,
        attributes: u32,
    ) -> Option<(StructureRef, PropertyOffset)> {
        debug_assert!(!structure.is_dictionary());
        debug_assert!(structure.is_object());

        if structure.has_been_dictionary() {
            return None;
        }

        let uid = property_name.uid()?;
        let existing_transition = structure.transition_table.borrow().get(
            PointerKey::Uid(uid.clone()),
            attributes,
            TransitionKind::PropertyAddition,
        )?;
        validate_offset_with_inline_capacity(existing_transition.transition_offset(), existing_transition.inline_capacity() as i32);
        let offset = existing_transition.transition_offset();
        Some((existing_transition, offset))
    }

    /// `addPropertyTransition(vm, structure, propertyName, attributes, offset)`.
    pub fn add_property_transition(
        vm: &VM,
        structure: &StructureRef,
        property_name: &PropertyName,
        attributes: u32,
    ) -> (StructureRef, PropertyOffset) {
        if let Some(existing) = Structure::add_property_transition_to_existing_structure(structure, property_name, attributes) {
            return existing;
        }

        Structure::add_new_property_transition(vm, structure, property_name, attributes, PutContext::UnknownContext)
    }

    /// `addNewPropertyTransition(vm, structure, propertyName, attributes, offset, context, deferred)`.
    pub fn add_new_property_transition(
        vm: &VM,
        structure: &StructureRef,
        property_name: &PropertyName,
        attributes: u32,
        context: PutContext,
    ) -> (StructureRef, PropertyOffset) {
        debug_assert!(!structure.is_dictionary());
        debug_assert!(structure.is_object());
        debug_assert!(Structure::add_property_transition_to_existing_structure(structure, property_name, attributes).is_none());

        if structure.should_do_cacheable_dictionary_transition_for_add(context) {
            debug_assert!(!is_copy_on_write(structure.indexing_mode()));
            let transition = Structure::to_cacheable_dictionary_transition(vm, structure);
            debug_assert!(!Rc::ptr_eq(structure, &transition));
            let offset = transition.add(vm, property_name, attributes);
            return (transition, offset);
        }

        let mut transition = Structure::new_from_previous(vm, structure);

        transition.indexing_mode_including_history = structure.indexing_mode_including_history & !COPY_ON_WRITE;
        transition.transition_property_name = property_name.uid().cloned();
        transition.transition_property_is_private = property_name.is_private_name();
        transition.transition_property_attributes = attributes as u8;
        transition.transition_kind = TransitionKind::PropertyAddition;
        // `takePropertyTableOrCloneIfPinned`: aqui sempre se copia.
        transition.property_table = RefCell::new(structure.property_table.borrow().clone());
        transition.set_max_offset(structure.max_offset());

        let offset = transition.add(vm, property_name, attributes);
        transition.set_transition_offset(offset);

        check_offset_with_inline_capacity(transition.transition_offset(), transition.inline_capacity() as i32);
        let transition = Rc::new(transition);
        if !structure.has_been_dictionary() {
            if let Some(uid) = property_name.uid() {
                structure.transition_table.borrow_mut().add(
                    PointerKey::Uid(uid.clone()),
                    attributes,
                    TransitionKind::PropertyAddition,
                    Rc::clone(&transition),
                );
            }
        }
        (transition, offset)
    }

    /// `attributeChangeTransitionToExistingStructure(structure, propertyName, attributes, offset)`.
    pub fn attribute_change_transition_to_existing_structure(
        structure: &StructureRef,
        property_name: &PropertyName,
        attributes: u32,
    ) -> Option<(StructureRef, PropertyOffset)> {
        debug_assert!(structure.is_object());

        if structure.has_been_dictionary() {
            return None;
        }

        let uid = property_name.uid()?;
        let existing_transition = structure.transition_table.borrow().get(
            PointerKey::Uid(uid.clone()),
            attributes,
            TransitionKind::PropertyAttributeChange,
        )?;
        let offset = existing_transition.transition_offset();
        Some((existing_transition, offset))
    }

    /// `attributeChangeTransition(vm, structure, propertyName, attributes, deferred)`.
    pub fn attribute_change_transition(
        vm: &VM,
        structure: &StructureRef,
        property_name: &PropertyName,
        attributes: u32,
    ) -> StructureRef {
        if structure.is_uncacheable_dictionary() {
            structure.attribute_change_without_transition(vm, property_name, attributes);
            return Rc::clone(structure);
        }

        if let Some((existing_transition, _offset)) =
            Structure::attribute_change_transition_to_existing_structure(structure, property_name, attributes)
        {
            validate_offset_with_inline_capacity(
                existing_transition.transition_offset(),
                existing_transition.inline_capacity() as i32,
            );
            return existing_transition;
        }

        if structure.should_do_cacheable_dictionary_transition_for_remove_and_attribute_change() {
            debug_assert!(!is_copy_on_write(structure.indexing_mode()));
            let transition = Structure::to_uncacheable_dictionary_transition(vm, structure);
            debug_assert!(!Rc::ptr_eq(structure, &transition));
            transition.attribute_change(vm, property_name, attributes);
            return transition;
        }

        // Even if the current structure is dictionary, we should perform transition since this changes
        // attributes of existing properties to keep structure still cacheable.
        let mut transition = Structure::new_from_previous(vm, structure);

        transition.indexing_mode_including_history = structure.indexing_mode_including_history & !COPY_ON_WRITE;
        transition.transition_property_name = property_name.uid().cloned();
        transition.transition_property_is_private = property_name.is_private_name();
        transition.transition_property_attributes = attributes as u8;
        transition.transition_kind = TransitionKind::PropertyAttributeChange;
        transition.property_table = RefCell::new(structure.property_table.borrow().clone());
        transition.set_max_offset(structure.max_offset());

        let offset = transition.attribute_change(vm, property_name, attributes);
        transition.set_transition_offset(offset);

        check_offset_with_inline_capacity(transition.transition_offset(), transition.inline_capacity() as i32);
        let transition = Rc::new(transition);
        if !structure.has_been_dictionary() {
            if let Some(uid) = property_name.uid() {
                structure.transition_table.borrow_mut().add(
                    PointerKey::Uid(uid.clone()),
                    attributes,
                    TransitionKind::PropertyAttributeChange,
                    Rc::clone(&transition),
                );
            }
        }
        transition
    }

    /// `Structure::remove(vm, propertyName)`: o `remove<ShouldPin::No>` de StructureInlines.h. Devolve o
    /// offset que a propriedade tinha (o offset passa a ser reaproveitável), ou `invalidOffset`.
    fn remove(&self, vm: &VM, property_name: &PropertyName) -> PropertyOffset {
        let uid = property_name.uid().expect("remove com PropertyName nulo (ASSERT do C++)");
        debug_assert!(is_valid_offset(self.get(vm, property_name)));

        let offset = {
            let mut slot = self.property_table.borrow_mut();
            let table = slot.get_or_insert_with(PropertyTable::default);
            let (offset, _attributes) = table.take(uid);
            if offset == INVALID_OFFSET {
                return INVALID_OFFSET;
            }
            table.add_deleted_offset(offset);
            offset
        };

        self.set_is_quick_property_access_allowed_for_enumeration(false);

        debug_assert!(!is_valid_offset(self.get(vm, property_name)));
        offset
    }

    /// `removePropertyTransitionFromExistingStructureImpl(structure, propertyName, attributes, offset)`.
    fn remove_property_transition_from_existing_structure_impl(
        structure: &StructureRef,
        property_name: &PropertyName,
        attributes: u32,
    ) -> Option<(StructureRef, PropertyOffset)> {
        debug_assert!(!structure.is_uncacheable_dictionary());
        debug_assert!(structure.is_object());

        if structure.has_been_dictionary() {
            return None;
        }

        let uid = property_name.uid()?;
        let existing_transition = structure.transition_table.borrow().get(
            PointerKey::Uid(uid.clone()),
            attributes,
            TransitionKind::PropertyDeletion,
        )?;
        validate_offset_with_inline_capacity(existing_transition.transition_offset(), existing_transition.inline_capacity() as i32);
        let offset = existing_transition.transition_offset();
        Some((existing_transition, offset))
    }

    /// `removePropertyTransitionFromExistingStructure(structure, propertyName, offset)`.
    pub fn remove_property_transition_from_existing_structure(
        structure: &StructureRef,
        property_name: &PropertyName,
    ) -> Option<(StructureRef, PropertyOffset)> {
        let uid = property_name.uid()?;
        let (offset, attributes) = match &*structure.property_table.borrow() {
            Some(table) => table.get(uid),
            None => (INVALID_OFFSET, 0),
        };
        if offset == INVALID_OFFSET {
            return None;
        }
        Structure::remove_property_transition_from_existing_structure_impl(structure, property_name, attributes)
    }

    /// `removePropertyTransition(vm, structure, propertyName, offset, deferred)`.
    pub fn remove_property_transition(
        vm: &VM,
        structure: &StructureRef,
        property_name: &PropertyName,
    ) -> (StructureRef, PropertyOffset) {
        if let Some(existing) = Structure::remove_property_transition_from_existing_structure(structure, property_name) {
            return existing;
        }

        Structure::remove_new_property_transition(vm, structure, property_name)
    }

    /// `removeNewPropertyTransition(vm, structure, propertyName, offset, deferred)`.
    pub fn remove_new_property_transition(
        vm: &VM,
        structure: &StructureRef,
        property_name: &PropertyName,
    ) -> (StructureRef, PropertyOffset) {
        debug_assert!(!structure.is_uncacheable_dictionary());
        debug_assert!(structure.is_object());
        debug_assert!(Structure::remove_property_transition_from_existing_structure(structure, property_name).is_none());
        debug_assert!(is_valid_offset(structure.get(vm, property_name)));

        if structure.should_do_cacheable_dictionary_transition_for_remove_and_attribute_change() {
            debug_assert!(!is_copy_on_write(structure.indexing_mode()));
            let transition = Structure::to_uncacheable_dictionary_transition(vm, structure);
            debug_assert!(!Rc::ptr_eq(structure, &transition));
            let offset = transition.remove(vm, property_name);
            return (transition, offset);
        }

        let mut transition = Structure::new_from_previous(vm, structure);

        transition.indexing_mode_including_history = structure.indexing_mode_including_history & !COPY_ON_WRITE;
        transition.transition_property_name = property_name.uid().cloned();
        transition.transition_property_is_private = property_name.is_private_name();
        // O C++ não chama `setTransitionPropertyAttributes` aqui: a chave da transição fica com atributos 0,
        // e `removePropertyTransitionFromExistingStructureImpl` procura com os atributos da propriedade.
        transition.transition_kind = TransitionKind::PropertyDeletion;
        transition.property_table = RefCell::new(structure.property_table.borrow().clone());
        transition.set_max_offset(structure.max_offset());

        let offset = transition.remove(vm, property_name);
        debug_assert!(offset != INVALID_OFFSET);
        transition.set_transition_offset(offset);

        check_offset_with_inline_capacity(transition.transition_offset(), transition.inline_capacity() as i32);
        let transition = Rc::new(transition);
        if !structure.has_been_dictionary() {
            if let Some(uid) = property_name.uid() {
                structure.transition_table.borrow_mut().add(
                    PointerKey::Uid(uid.clone()),
                    0,
                    TransitionKind::PropertyDeletion,
                    Rc::clone(&transition),
                );
            }
        }
        (transition, offset)
    }

    /// `addPropertyWithoutTransition(vm, propertyName, attributes, func)`: o `add<ShouldPin::Yes>`.
    /// Devolve o offset novo; o `newMaxOffset` do callback é `maxOffset()` depois da chamada.
    pub fn add_property_without_transition(&self, vm: &VM, property_name: &PropertyName, attributes: u32) -> PropertyOffset {
        self.pin();
        self.add(vm, property_name, attributes)
    }

    /// `removePropertyWithoutTransition(vm, propertyName, func)`: só em dicionário não cacheável.
    pub fn remove_property_without_transition(&self, vm: &VM, property_name: &PropertyName) -> PropertyOffset {
        debug_assert!(self.is_uncacheable_dictionary());
        debug_assert!(self.is_pinned_property_table());
        debug_assert!(self.property_table.borrow().is_some());

        self.pin();
        self.remove(vm, property_name)
    }

    /// `addOrReplacePropertyWithoutTransition(vm, propertyName, newAttributes, func)`: `(offset,
    /// attributes, isAdded)`. Se a propriedade já existe, devolve o offset e os atributos que ela tem e
    /// não muda nada.
    pub fn add_or_replace_property_without_transition(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        new_attributes: u32,
    ) -> (PropertyOffset, u32, bool) {
        let uid = property_name.uid().expect("addOrReplacePropertyWithoutTransition com PropertyName nulo (ASSERT do C++)");
        {
            let mut slot = self.property_table.borrow_mut();
            let table = slot.get_or_insert_with(PropertyTable::default);
            if let Some(entry) = table.find(uid) {
                return (entry.offset(), entry.attributes(), false);
            }
        }

        self.pin();
        let new_offset = self.add(vm, property_name, new_attributes);
        (new_offset, new_attributes, true)
    }

    /// `attributeChangeWithoutTransition(vm, propertyName, attributes, func)`: o `attributeChange<ShouldPin::Yes>`.
    pub fn attribute_change_without_transition(&self, vm: &VM, property_name: &PropertyName, attributes: u32) -> PropertyOffset {
        self.pin();
        self.attribute_change(vm, property_name, attributes)
    }

    /// `toDictionaryTransition(vm, structure, kind, deferred)`.
    fn to_dictionary_transition(vm: &VM, structure: &StructureRef, kind: DictionaryKind) -> StructureRef {
        let _ = vm;
        debug_assert!(!structure.is_uncacheable_dictionary());

        let mut transition = Structure::new_from_previous(vm, structure);

        transition.property_table = structure.copy_property_table_for_pinning();
        transition.pin_local();
        transition.set_max_offset(structure.max_offset());
        transition.set_dictionary_kind(kind);
        transition.set_has_been_dictionary(true);

        Rc::new(transition)
    }

    /// `toCacheableDictionaryTransition(vm, structure, deferred)`.
    pub fn to_cacheable_dictionary_transition(vm: &VM, structure: &StructureRef) -> StructureRef {
        Structure::to_dictionary_transition(vm, structure, DictionaryKind::CachedDictionaryKind)
    }

    /// `toUncacheableDictionaryTransition(vm, structure, deferred)`.
    pub fn to_uncacheable_dictionary_transition(vm: &VM, structure: &StructureRef) -> StructureRef {
        Structure::to_dictionary_transition(vm, structure, DictionaryKind::UncachedDictionaryKind)
    }

    /// `flattenDictionaryStructure(vm, object)`: tira a estrutura do modo dicionário. Num dicionário não
    /// cacheável compacta os offsets em ordem de inserção (movendo os valores do objeto e encolhendo o
    /// armazenamento fora de linha); num cacheável só desliga o modo. Devolve a própria estrutura.
    pub fn flatten_dictionary_structure(structure: &StructureRef, vm: &VM, object: &JSObject) -> StructureRef {
        debug_assert!(structure.is_dictionary());
        debug_assert!(Rc::ptr_eq(&object.structure(), structure));

        let before_out_of_line_capacity = structure.out_of_line_capacity();
        let mut after_out_of_line_capacity = before_out_of_line_capacity;
        if structure.is_uncacheable_dictionary() {
            let property_count = {
                let table = structure.property_table.borrow();
                table.as_ref().expect("dicionário não cacheável sem tabela (ASSERT do C++)").size()
            };
            let max_offset = if property_count > 0 {
                offset_for_property_number(property_count as i32 - 1, structure.inline_capacity as i32)
            } else {
                INVALID_OFFSET
            };
            after_out_of_line_capacity = Structure::out_of_line_capacity_for_max_offset(max_offset);

            // Copies out our values from their hashed locations, compacting property table offsets as we go.
            let (offset, values) = {
                let mut slot = structure.property_table.borrow_mut();
                let table = slot.as_mut().expect("dicionário não cacheável sem tabela (ASSERT do C++)");
                table.renumber_property_offsets(structure.inline_capacity as i32, |offset| object.get_direct(offset))
            };
            structure.set_max_offset(offset);
            debug_assert!(structure.transition_offset() == INVALID_OFFSET);

            // Copies in our values to their compacted locations.
            for (i, value) in values.into_iter().enumerate() {
                object.put_direct_offset(vm, offset_for_property_number(i as i32, structure.inline_capacity as i32), value);
            }

            // We need to zero our unused property space; otherwise the GC might see a stale pointer when we
            // add properties in the future.
            object.clear_unused_property_storage(structure.inline_size() as usize, structure.out_of_line_size() as usize);
        }

        structure.set_dictionary_kind(DictionaryKind::NoneDictionaryKind);
        structure.set_has_been_flattened_before(true);

        debug_assert!(structure.out_of_line_capacity() == after_out_of_line_capacity);

        if before_out_of_line_capacity != after_out_of_line_capacity {
            debug_assert!(before_out_of_line_capacity > after_out_of_line_capacity);
            object.shrink_out_of_line_storage(after_out_of_line_capacity as usize);
        }

        Rc::clone(structure)
    }

    /// `isValidPrototype(JSValue)`.
    pub fn is_valid_prototype(prototype: &JSValue) -> bool {
        // `JSFunction` é objeto (`CellEntry::as_js_object` inclui `Function`, então `JSObject::from_value`
        // também a cobre): `class B extends A` com `A` função tem a função como protótipo, e
        // `prototype.isObject()` do C++ a aceita. O ramo de `as_js_function` abaixo é redundante e fica
        // por explicitar o caso.
        prototype.is_null()
            || JSObject::from_value(prototype).is_some_and(|object| object.may_be_prototype())
            || prototype.as_js_function().is_some_and(|function| {
                let object: &JSObject = &function;
                object.may_be_prototype()
            })
    }

    /// `changePrototypeTransition(vm, structure, prototype, deferred)`.
    pub fn change_prototype_transition(vm: &VM, structure: &StructureRef, prototype: JSValue) -> StructureRef {
        let _ = vm;
        debug_assert!(Structure::is_valid_prototype(&prototype));

        let key = match (JSObject::from_value(&prototype), prototype.as_js_function()) {
            (Some(object), _) => PointerKey::Object(object.cell_id()),
            (None, Some(function)) => PointerKey::Object(function.cell_id()),
            (None, None) => PointerKey::Null,
        };

        let should_chain =
            !structure.has_poly_proto() && structure.type_() != JSType::GlobalObjectType && !structure.has_been_dictionary();
        if should_chain {
            debug_assert!(structure.is_object());
            if let Some(existing_transition) =
                structure.transition_table.borrow().get(key.clone(), 0, TransitionKind::ChangePrototype)
            {
                return existing_transition;
            }
        }

        // Changing [[Prototype]] means that we refresh this object completely. This is very likely that
        // this object will behaves differently from the previous one. Let's pin the table and break the
        // edge to the previous Structure.
        let mut transition = Structure::new_from_previous(vm, structure);
        transition.property_table = RefCell::new(structure.property_table.borrow().clone());
        transition.set_is_pinned_property_table(true);
        transition.previous = None;
        transition.prototype = prototype;
        crate::runtime::gc::retain_prototype(prototype);
        transition.transition_kind = TransitionKind::ChangePrototype;
        transition.set_max_offset(structure.max_offset());
        check_offset_with_inline_capacity(transition.transition_offset(), transition.inline_capacity() as i32);

        let transition = Rc::new(transition);
        if should_chain {
            structure.transition_table.borrow_mut().add(
                key,
                0,
                TransitionKind::ChangePrototype,
                Rc::clone(&transition),
            );
        }
        transition
    }

    /// `changeGlobalProxyTargetTransition(vm, structure, globalObject, deferred)`: a estrutura nova do
    /// `JSGlobalProxy` que troca de alvo. Fica fora da árvore de transições (`previous` desligado, como no
    /// `pin` do C++), com o `realm` do novo alvo e a tabela de propriedades copiada e fixada.
    pub fn change_global_proxy_target_transition(
        vm: &VM,
        structure: &StructureRef,
        global_object: &JSGlobalObjectRef,
    ) -> StructureRef {
        let mut transition = Structure::new_from_previous(vm, structure);
        transition.set_realm(vm, global_object);
        transition.property_table = RefCell::new(structure.property_table.borrow().clone());
        transition.set_is_pinned_property_table(true);
        transition.previous = None;
        transition.set_max_offset(structure.max_offset());
        check_offset_with_inline_capacity(transition.transition_offset(), transition.inline_capacity() as i32);
        Rc::new(transition)
    }

    /// `becomePrototypeTransition(vm, structure, deferred)`.
    pub fn become_prototype_transition(vm: &VM, structure: &StructureRef) -> StructureRef {
        Structure::non_property_transition(vm, structure, TransitionKind::BecomePrototype)
    }

    /// `preventExtensionsTransition(vm, structure, deferred)`.
    pub fn prevent_extensions_transition(vm: &VM, structure: &StructureRef) -> StructureRef {
        Structure::non_property_transition(vm, structure, TransitionKind::PreventExtensions)
    }

    /// `sealTransition(vm, structure, deferred)`.
    pub fn seal_transition(vm: &VM, structure: &StructureRef) -> StructureRef {
        Structure::non_property_transition(vm, structure, TransitionKind::Seal)
    }

    /// `freezeTransition(vm, structure, deferred)`.
    pub fn freeze_transition(vm: &VM, structure: &StructureRef) -> StructureRef {
        Structure::non_property_transition(vm, structure, TransitionKind::Freeze)
    }

    /// `nonPropertyTransition(vm, structure, transitionKind, deferred)` (StructureInlines.h): uma
    /// transição de indexação a partir de uma estrutura de array original do realm vai para a estrutura
    /// original da forma nova; o resto cai em `nonPropertyTransitionSlow`.
    pub fn non_property_transition(vm: &VM, structure: &StructureRef, transition_kind: TransitionKind) -> StructureRef {
        if changes_indexing_type(transition_kind) {
            if let Some(global_object) = structure.realm() {
                if global_object.is_original_array_structure(structure) {
                    let indexing_mode_including_history =
                        new_indexing_type(structure.indexing_mode_including_history(), transition_kind);
                    let result = global_object.original_array_structure_for_indexing_type(indexing_mode_including_history);
                    if result.indexing_mode_including_history() == indexing_mode_including_history {
                        structure.did_transition_from_this_structure(vm);
                        return result;
                    }
                }
            }
        }

        Structure::non_property_transition_slow(vm, structure, transition_kind)
    }

    /// `nonPropertyTransitionSlow(vm, structure, transitionKind, deferred)`.
    fn non_property_transition_slow(vm: &VM, structure: &StructureRef, transition_kind: TransitionKind) -> StructureRef {
        let indexing_mode_including_history = new_indexing_type(structure.indexing_mode_including_history, transition_kind);

        if !structure.is_dictionary() {
            if let Some(existing_transition) =
                structure.transition_table.borrow().get(PointerKey::Null, 0, transition_kind)
            {
                debug_assert!(existing_transition.transition_kind() == transition_kind);
                debug_assert!(existing_transition.indexing_mode_including_history() == indexing_mode_including_history);
                return existing_transition;
            }
        }

        let mut transition = Structure::new_from_previous(vm, structure);
        transition.transition_kind = transition_kind;
        transition.indexing_mode_including_history = indexing_mode_including_history;

        if changes_indexing_type(transition_kind) && has_any_array_storage(indexing_mode_including_history) {
            transition.set_has_non_enumerable_properties(true);
            transition.set_has_non_configurable_properties(true);
            transition.set_has_non_configurable_read_only_or_getter_setter_properties(true);
        }

        if prevents_extensions(transition_kind) {
            transition.set_did_prevent_extensions(true);
        }

        if transition_kind == TransitionKind::BecomePrototype {
            transition.set_may_be_prototype(true);
        }

        transition.property_table = RefCell::new(structure.property_table.borrow().clone());
        transition.set_max_offset(structure.max_offset());
        if sets_dont_delete_on_all_properties(transition_kind) || sets_read_only_on_non_accessor_properties(transition_kind) {
            // We pin the property table on transitions that do wholesale editing of the property
            // table, since our logic for walking the property transition chain to rematerialize the
            // table doesn't know how to take into account such wholesale edits.
            debug_assert!(transition_kind == TransitionKind::Seal || transition_kind == TransitionKind::Freeze);

            transition.set_is_pinned_property_table(true);
            {
                let mut slot = transition.property_table.borrow_mut();
                let table = slot.get_or_insert_with(PropertyTable::default);
                if transition_kind == TransitionKind::Seal {
                    table.seal();
                } else {
                    table.freeze();
                }
            }

            transition.set_has_non_enumerable_properties(true);
            transition.set_has_non_configurable_properties(true);
            transition.set_has_non_configurable_read_only_or_getter_setter_properties(true);
        } else {
            check_offset_with_inline_capacity(transition.max_offset(), transition.inline_capacity() as i32);
        }

        if sets_read_only_on_non_accessor_properties(transition_kind)
            && (*transition.property_table.borrow()).as_ref().is_some_and(|table| !table.is_empty())
        {
            transition.set_has_read_only_or_getter_setter_properties_excluding_proto(true);
        }

        if structure.is_dictionary() {
            transition.property_table.borrow_mut().get_or_insert_with(PropertyTable::default);
            transition.pin_local();
            return Rc::new(transition);
        }

        let transition = Rc::new(transition);
        structure.transition_table.borrow_mut().add(
            PointerKey::Null,
            0,
            transition_kind,
            Rc::clone(&transition),
        );
        transition
    }

    /// `isBrandedStructure()`.
    pub fn is_branded_structure(&self) -> bool {
        self.branded.is_some()
    }

    /// O `uncheckedDowncast<BrandedStructure>(this)`: o dado da subclasse, se a variante é `Branded`.
    pub fn branded(&self) -> Option<&BrandedStructure> {
        self.branded.as_ref()
    }

    /// `setBrandTransitionFromExistingStructureImpl(structure, brandID)`.
    pub fn set_brand_transition_from_existing_structure(structure: &StructureRef, brand_id: &UniquedKey) -> Option<StructureRef> {
        debug_assert!(structure.is_object());

        if structure.has_been_dictionary() {
            return None;
        }

        structure.transition_table.borrow().get(PointerKey::Uid(brand_id.clone()), 0, TransitionKind::SetBrand)
    }

    /// `setBrandTransition(vm, structure, brand, deferred)`: `brand` é o `uid()` do `Symbol` privado.
    pub fn set_brand_transition(vm: &VM, structure: &StructureRef, brand: &UniquedKey) -> StructureRef {
        if let Some(existing_transition) = Structure::set_brand_transition_from_existing_structure(structure, brand) {
            return existing_transition;
        }

        // `BrandedStructure::create`: o construtor da subclasse e o `finishCreation`.
        let mut transition = Structure::new_from_previous(vm, structure);
        transition.branded = Some(BrandedStructure::new(structure, brand.clone()));
        transition.transition_kind = TransitionKind::SetBrand;
        transition.indexing_mode_including_history = structure.indexing_mode_including_history;
        transition.transition_property_name = Some(brand.clone());
        transition.transition_property_is_private = true;
        transition.transition_property_attributes = 0;
        // `takePropertyTableOrCloneIfPinned`: aqui sempre se copia.
        transition.property_table = RefCell::new(structure.property_table.borrow().clone());
        transition.set_max_offset(structure.max_offset());
        check_offset_with_inline_capacity(transition.max_offset(), transition.inline_capacity() as i32);

        if structure.is_dictionary() {
            transition.property_table.borrow_mut().get_or_insert_with(PropertyTable::default);
            transition.pin_local();
            return Rc::new(transition);
        }

        let transition = Rc::new(transition);
        structure.transition_table.borrow_mut().add(
            PointerKey::Uid(brand.clone()),
            0,
            TransitionKind::SetBrand,
            Rc::clone(&transition),
        );
        transition
    }

    /// `isSealed(vm)`.
    pub fn is_sealed(&self) -> bool {
        if self.is_structure_extensible() {
            return false;
        }
        match &*self.property_table.borrow() {
            Some(table) => table.is_sealed(),
            None => true,
        }
    }

    /// `isFrozen(vm)`.
    pub fn is_frozen(&self) -> bool {
        if self.is_structure_extensible() {
            return false;
        }
        match &*self.property_table.borrow() {
            Some(table) => table.is_frozen(),
            None => true,
        }
    }

    /// `canPerformFastPropertyEnumerationCommon()`.
    pub fn can_perform_fast_property_enumeration_common(&self) -> bool {
        let type_info = self.type_info();
        !(type_info.overrides_get_own_property_slot()
            || type_info.overrides_any_form_of_get_own_property_names()
            || self.has_any_kind_of_getter_setter_properties()
            || self.is_uncacheable_dictionary()
            // Cannot perform fast [[Put]] to |target| if the property names of the |source| contain "__proto__".
            || self.has_underscore_proto_property_excluding_original_proto())
    }

    /// `canPerformFastPropertyEnumeration()`.
    pub fn can_perform_fast_property_enumeration(&self) -> bool {
        // FIXME do C++: indexed properties can be handled (bug 185358).
        self.can_perform_fast_property_enumeration_common() && !has_indexed_properties(self.indexing_type())
    }

    /// `forEachProperty`: as propriedades em ordem de inserção, `(chave, offset, atributos)`.
    pub fn properties(&self) -> Vec<(UniquedKey, PropertyOffset, u32)> {
        self.properties_with_privacy().into_iter().map(|(key, offset, attributes, _)| (key, offset, attributes)).collect()
    }

    /// `forEachProperty` com a marca de nome privado (`PropertyTableEntry::isPrivate`), que o
    /// `getPropertyNamesFromStructure` consulta para o `PrivateSymbolMode`.
    pub fn properties_with_privacy(&self) -> Vec<(UniquedKey, PropertyOffset, u32, bool)> {
        match &*self.property_table.borrow() {
            Some(table) => table
                .iter()
                .map(|entry| (entry.key().clone(), entry.offset(), entry.attributes(), entry.is_private()))
                .collect(),
            None => Vec::new(),
        }
    }

    /// `Structure::dump(PrintStream&)`: `0xPTR:[0xID/ID, Classe, (inline/capInline, fora/capFora){props}, Indexing, Transição, ...]`.
    /// ` Leaf`/` Shady leaf` conforme `transitionWatchpointSetIsStillValid()`, e ` (Watched)` conforme
    /// `transitionWatchpointSet().isBeingWatched()` (Structure.cpp:1566-1573).
    pub fn dump(&self) -> String {
        use crate::runtime::indexing_type::dump_indexing_type;
        use std::fmt::Write;
        let mut out = String::new();
        let _ = write!(
            out,
            "{:#x}:[{:#x}/{}, {}, ({}/{}, {}/{}){{",
            self as *const Structure as usize,
            self.id,
            self.id,
            self.class_info.class_name,
            self.inline_size(),
            self.inline_capacity(),
            self.out_of_line_size(),
            self.out_of_line_capacity()
        );
        for (index, (key, offset, _)) in self.properties().into_iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "{}:{}", String::from_utf8_lossy(&key.0.utf8(crate::wtf::text::conversion_mode::ConversionMode::LenientConversion)), offset);
        }
        let _ = write!(out, "}}, {}, {:?}", dump_indexing_type(self.indexing_mode()), self.transition_kind);
        if self.has_poly_proto() {
            // `knownPolyProtoOffset` não existe sem poly proto.
        } else if self.prototype.is_cell() {
            let _ = write!(out, ", Proto:{:#x}", self.prototype.as_cell());
        }
        match self.dictionary_kind() {
            DictionaryKind::NoneDictionaryKind => {
                if self.has_been_dictionary() {
                    out.push_str(", Has been dictionary");
                }
            }
            DictionaryKind::CachedDictionaryKind => out.push_str(", Dictionary"),
            DictionaryKind::UncachedDictionaryKind => out.push_str(", UncacheableDictionary"),
        }
        if self.transition_watchpoint_set_is_still_valid() {
            out.push_str(", Leaf");
        } else if self.transition_watchpoint_is_likely_to_be_fired() {
            out.push_str(", Shady leaf");
        }
        if self.transition_watchpoint_set.borrow().is_being_watched() {
            out.push_str(" (Watched)");
        }
        out.push(']');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks() {
        assert_eq!(Structure::DICTIONARY_KIND_MASK, 0b11);
        assert_eq!(Structure::TRANSITION_KIND_MASK, 0b11111);
        assert_eq!(Structure::DID_PREVENT_EXTENSIONS_BITS, 1 << 20);
        assert_eq!(Structure::HAS_NON_CONFIGURABLE_PROPERTIES_BITS, 1 << 29);
        assert_eq!(Structure::HAS_NON_CONFIGURABLE_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_BITS, 1 << 30);
    }

    #[test]
    fn out_of_line_capacity_growth() {
        assert_eq!(Structure::out_of_line_capacity_for_max_offset(63), 0);
        assert_eq!(Structure::out_of_line_capacity_for_max_offset(64), 4);
        assert_eq!(Structure::out_of_line_capacity_for_max_offset(67), 4);
        assert_eq!(Structure::out_of_line_capacity_for_max_offset(68), 8);
        assert_eq!(Structure::out_of_line_capacity_for_max_offset(72), 16);
    }

    // Os testes abaixo fixam o contrato que o `ObjectAdaptiveStructureWatchpoint` usa: toda estrutura
    // nova criada a partir de outra dispara o `transitionWatchpointSet` da anterior (o
    // `finishCreation(vm, previous, deferred)` do C++), e a mutação de um dicionário não dispara.
    // Veja wip/notes/transition-watchpoint.md.
    use crate::bytecode::watchpoint::{FireDetail, Watchpoint, WatchpointBody, WatchpointType};
    use crate::runtime::js_object::JSNonFinalObject;

    struct CountBody(Rc<Cell<u32>>);

    impl WatchpointBody for CountBody {
        fn fire_internal(&mut self, _vm: &VM, _detail: &dyn FireDetail) {
            self.0.set(self.0.get() + 1);
        }
    }

    /// Instala um watchpoint contador no `transitionWatchpointSet` e devolve o contador.
    fn watch(structure: &StructureRef) -> Rc<Cell<u32>> {
        let count = Rc::new(Cell::new(0));
        let watchpoint = Watchpoint::new(WatchpointType::ObjectAdaptiveStructure, Box::new(CountBody(Rc::clone(&count))));
        structure.add_transition_watchpoint(Some(watchpoint));
        count
    }

    fn root_structure(global: &JSGlobalObjectRef, vm: &VM) -> StructureRef {
        JSNonFinalObject::create_structure(vm, Some(global), global.object_prototype().as_value())
    }

    #[test]
    fn add_property_transition_fires_previous_transition_watchpoint() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let root = root_structure(&global, vm);
        let fired = watch(&root);
        let name = PropertyName::from_identifier(&vm.property_names.for_each);

        let (next, _) = Structure::add_property_transition(vm, &root, &name, 0);
        assert_eq!(fired.get(), 1);
        assert!(root.transition_watchpoint_set_has_been_invalidated());
        // A estrutura nova nasce vigiável.
        assert!(next.transition_watchpoint_set_is_still_valid());

        // A transição já existente não cria estrutura, então não dispara de novo.
        let (again, _) = Structure::add_property_transition(vm, &root, &name, 0);
        assert!(Rc::ptr_eq(&next, &again));
        assert_eq!(fired.get(), 1);
    }

    #[test]
    fn remove_property_transition_fires_previous_transition_watchpoint() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let root = root_structure(&global, vm);
        let name = PropertyName::from_identifier(&vm.property_names.for_each);
        let (with_property, _) = Structure::add_property_transition(vm, &root, &name, 0);
        let fired = watch(&with_property);

        let (_removed, _) = Structure::remove_property_transition(vm, &with_property, &name);
        assert_eq!(fired.get(), 1);
        assert!(with_property.transition_watchpoint_set_has_been_invalidated());
    }

    #[test]
    fn attribute_change_transition_fires_previous_transition_watchpoint() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let root = root_structure(&global, vm);
        let name = PropertyName::from_identifier(&vm.property_names.for_each);
        let (with_property, _) = Structure::add_property_transition(vm, &root, &name, 0);
        let fired = watch(&with_property);

        let changed = Structure::attribute_change_transition(vm, &with_property, &name, crate::runtime::property_attribute::READ_ONLY);
        assert!(!Rc::ptr_eq(&changed, &with_property));
        assert_eq!(fired.get(), 1);
    }

    #[test]
    fn change_prototype_transition_fires_previous_transition_watchpoint() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let root = root_structure(&global, vm);
        let fired = watch(&root);

        let changed = Structure::change_prototype_transition(vm, &root, JSValue::null());
        assert!(!Rc::ptr_eq(&changed, &root));
        assert_eq!(fired.get(), 1);
    }

    #[test]
    fn non_property_transition_fires_previous_transition_watchpoint() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let root = root_structure(&global, vm);
        let fired = watch(&root);

        let _ = Structure::non_property_transition(vm, &root, TransitionKind::PreventExtensions);
        assert_eq!(fired.get(), 1);
    }

    /// O atalho de `nonPropertyTransition` para as estruturas de array originais não cria estrutura, e
    /// por isso chama `didTransitionFromThisStructure` à mão.
    #[test]
    fn original_array_structure_shortcut_fires_transition_watchpoint() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let array = global.array_structure();
        assert!(global.is_original_array_structure(&array));
        let fired = watch(&array);

        let result = Structure::non_property_transition(vm, &array, TransitionKind::AllocateInt32);
        assert!(global.is_original_array_structure(&result));
        assert_eq!(fired.get(), 1);
    }

    #[test]
    fn to_dictionary_transition_fires_and_dictionary_mutation_does_not() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let root = root_structure(&global, vm);
        let fired = watch(&root);

        let dictionary = Structure::to_cacheable_dictionary_transition(vm, &root);
        assert_eq!(fired.get(), 1);

        // `addPropertyWithoutTransition` e companhia não tocam o set (Structure.cpp não os dispara).
        let dictionary_fired = watch(&dictionary);
        let name = PropertyName::from_identifier(&vm.property_names.for_each);
        dictionary.add_property_without_transition(vm, &name, 0);
        assert_eq!(dictionary_fired.get(), 0);
        assert!(dictionary.transition_watchpoint_set_is_still_valid());
    }
}
