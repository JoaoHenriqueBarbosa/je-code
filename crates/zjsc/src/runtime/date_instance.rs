//! Porte de `runtime/DateInstance.h`, `DateInstanceInlines.h` e `DateInstance.cpp`: o objeto `Date`,
//! um `JSNonFinalObject` com o valor de tempo em milissegundos (`m_internalNumber`) e duas datas
//! desmembradas em cache (local e UTC).
//!
//! DIVERGÊNCIAS (heap ausente, camada 3; mesmo padrão de `reg_exp_object.rs`):
//!
//! - `m_internalNumber` e os dois `PlainGregorianDateTime` ficam em `Cell`. `offsetOf*`,
//!   `subspaceFor` e `DECLARE_EXPORT_INFO` existem só para o layout de memória e para os JITs e somem.
//!   O registro é `CellEntry::DateInstance`.
//! - O `DateCache` vem por parâmetro (o C++ o pega do `VM`, que ainda não tem o campo `dateCache`).

use std::cell::Cell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_date_math::{DateCache, PlainGregorianDateTime, UseSharedCache};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::date_math::{time_clip, TimeType};

/// `const ClassInfo DateInstance::s_info`.
pub static DATE_INSTANCE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Date", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class DateInstance final : public JSNonFinalObject`.
pub struct DateInstance {
    base: JSNonFinalObject,
    /// `m_internalNumber`.
    internal_number: Cell<f64>,
    /// `m_cachedGregorianDateTime`.
    cached_gregorian_date_time: Cell<PlainGregorianDateTime>,
    /// A geração do fuso em que `cached_gregorian_date_time` foi calculado.
    local_time_zone_generation: Cell<u64>,
    /// `m_cachedGregorianDateTimeUTC`.
    cached_gregorian_date_time_utc: Cell<PlainGregorianDateTime>,
}

/// O `DateInstance*`.
pub type DateInstanceRef = Rc<DateInstance>;

impl std::fmt::Debug for DateInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateInstance")
            .field("cell_id", &self.base.cell_id())
            .field("internal_number", &self.internal_number.get())
            .finish()
    }
}

impl std::ops::Deref for DateInstance {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl DateInstance {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)` (DateInstanceInlines.h).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::JSDateType, DateInstance::STRUCTURE_FLAGS),
            &DATE_INSTANCE_S_INFO,
        )
    }

    /// O construtor com `finishCreation(vm, time)` ou `finishCreation(vm)` (`time` ausente é NaN), e o
    /// registro da célula.
    fn allocate(vm: &VM, structure: StructureRef, internal_number: f64) -> DateInstanceRef {
        let cell_id = cell_registry::reserve();
        let object = Rc::new(DateInstance {
            base: JSNonFinalObject::new(vm, structure),
            internal_number: Cell::new(internal_number),
            cached_gregorian_date_time: Cell::new(PlainGregorianDateTime::default()),
            local_time_zone_generation: Cell::new(0),
            cached_gregorian_date_time_utc: Cell::new(PlainGregorianDateTime::default()),
        });
        object.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::DateInstance(Rc::clone(&object)));
        debug_assert_eq!(object.type_(), JSType::JSDateType);
        object
    }

    /// `create(vm, structure, date)`: o valor passa por `timeClip`.
    pub fn create(vm: &VM, structure: StructureRef, date: f64) -> DateInstanceRef {
        DateInstance::allocate(vm, structure, time_clip(date))
    }

    /// `create(vm, structure)`: o valor de tempo nasce NaN (`Invalid Date`).
    pub fn create_invalid(vm: &VM, structure: StructureRef) -> DateInstanceRef {
        DateInstance::allocate(vm, structure, f64::NAN)
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell` (`dynamicDowncast<DateInstance>`).
    pub fn from_cell_id(cell_id: usize) -> Option<DateInstanceRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::DateInstance(object)) => Some(object),
            _ => None,
        }
    }

    /// `dynamicDowncast<DateInstance>(value)`.
    pub fn from_value(value: &JSValue) -> Option<DateInstanceRef> {
        match value {
            JSValue::Cell(cell_id) => DateInstance::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `internalNumber()`.
    pub fn internal_number(&self) -> f64 {
        self.internal_number.get()
    }

    /// `setInternalNumber(value)`. As datas desmembradas viram obsoletas em vez de zeradas: o payload
    /// zerado quer dizer que a instância nunca desmembrou nada, o que diz ao `DateCache` que vale
    /// consultar o cache compartilhado.
    pub fn set_internal_number(&self, value: f64) {
        self.internal_number.set(value);
        self.cached_gregorian_date_time.set(PlainGregorianDateTime::stale_marker());
        self.cached_gregorian_date_time_utc.set(PlainGregorianDateTime::stale_marker());
    }

    /// `gregorianDateTime(cache)`.
    pub fn gregorian_date_time(&self, cache: &DateCache) -> PlainGregorianDateTime {
        let cached = self.cached_gregorian_date_time.get();
        // O desmembrado local de antes de uma troca de fuso vale o fuso velho: recalcula.
        if cached.is_valid() && self.local_time_zone_generation.get() == cache.time_zone_generation() {
            return cached;
        }
        self.calculate_gregorian_date_time(cache)
    }

    /// `gregorianDateTimeUTC(cache)`.
    pub fn gregorian_date_time_utc(&self, cache: &DateCache) -> PlainGregorianDateTime {
        let cached = self.cached_gregorian_date_time_utc.get();
        if cached.is_valid() {
            return cached;
        }
        self.calculate_gregorian_date_time_utc(cache)
    }

    /// `invalidateCachedLocalGregorianDateTime()`: zerado, não obsoleto. O `DateCache` acabou de largar o
    /// cache compartilhado, então a próxima decomposição pode repovoá-lo.
    pub fn invalidate_cached_local_gregorian_date_time(&self) {
        self.cached_gregorian_date_time.set(PlainGregorianDateTime::default());
    }

    /// `useSharedCacheFor(cached)`: uma instância que nunca desmembrou nada ainda tem o valor com que
    /// nasceu, e outro `Date` pode já tê-lo desmembrado. Depois de andar para a frente, os valores são
    /// só dela e o cache compartilhado só erraria.
    fn use_shared_cache_for(cached: PlainGregorianDateTime) -> UseSharedCache {
        if cached.has_never_been_computed() { UseSharedCache::Yes } else { UseSharedCache::No }
    }

    fn calculate_gregorian_date_time(&self, cache: &DateCache) -> PlainGregorianDateTime {
        let milli = self.internal_number();
        if milli.is_nan() {
            return PlainGregorianDateTime::default();
        }

        let computed = cache.ms_to_gregorian_date_time(
            milli,
            TimeType::LocalTime,
            DateInstance::use_shared_cache_for(self.cached_gregorian_date_time.get()),
        );
        self.cached_gregorian_date_time.set(computed);
        self.local_time_zone_generation.set(cache.time_zone_generation());
        cache.note_cached_local_gregorian_date_time();
        computed
    }

    fn calculate_gregorian_date_time_utc(&self, cache: &DateCache) -> PlainGregorianDateTime {
        let milli = self.internal_number();
        if milli.is_nan() {
            return PlainGregorianDateTime::default();
        }

        let computed = cache.ms_to_gregorian_date_time(
            milli,
            TimeType::UTCTime,
            DateInstance::use_shared_cache_for(self.cached_gregorian_date_time_utc.get()),
        );
        self.cached_gregorian_date_time_utc.set(computed);
        computed
    }
}
