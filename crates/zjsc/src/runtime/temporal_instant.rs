//! Porte de `runtime/TemporalInstant.{h,cpp}`, `TemporalInstantConstructor.{h,cpp}` e
//! `TemporalInstantPrototype.{h,cpp}`: `Temporal.Instant`, a célula com o instante em nanossegundos
//! (`ISO8601::ExactTime`), o construtor (`from`, `fromEpochMilliseconds`, `fromEpochNanoseconds`, `compare`) e o
//! protótipo.
//!
//! DIVERGÊNCIAS (mesmo padrão de `date_instance.rs` e `date_constructor_natives.rs`):
//!
//! - `instantPrototypeTable` e `temporalInstantConstructorTable` ficam no `ClassInfo` e a `Structure` leva
//!   `HasStaticPropertyTable`: as entradas reificam no primeiro acesso. A célula registra-se como `CellEntry::TemporalInstant`.
//! - `m_instantStructure` (o `LazyClassStructure`) é o campo `instant_structure` de `TemporalGlobalData`,
//!   preenchido por [`install_instant`].
//! - `toInstant` com `String` usa `parse_iso_date_time` de `iso8601.rs` (os tokenizadores de `ISO8601.cpp`); com
//!   `ZonedDateTime` é o instante dele.
//! - `toString({ timeZone })` resolve o fuso por `to_temporal_time_zone_identifier` e o deslocamento por
//!   `get_offset_nanoseconds_for` (`temporal_time_zone.rs`).
//! - `toLocaleString` delega ao `IntlDateTimeFormat` com `Instant` (`intl_date_time_format/temporal.rs`).

use std::rc::Rc;

use crate::custom_getter;
use crate::host_function;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{constructor_cannot_be_called_as_function, put_to_string_tag};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intl_date_time_format::temporal::to_locale_string as intl_to_locale_string;
use crate::runtime::intl_date_time_format::{Defaults, Required};
use crate::runtime::intl_support::{
    get_options_object, get_property, str_value, to_rust_string, wtf_to_rust,
};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iso8601::{parse_iso_date_time, ExactTime, TemporalProduction, TemporalProductionSet};
use crate::runtime::internal_function::{get_derived_structure_in_realm, InternalFunction, InternalFunctionRef, PropertyAdditionMode};
use crate::runtime::js_big_int::{ImplResult, JSBigInt};
use crate::runtime::js_big_int_ops::{big_int_of, impl_result_value, to_big_int};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, DONT_DELETE, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_core_duration::temporal_duration_from_internal;
use crate::runtime::temporal_core_instant::instant_to_string;
use crate::runtime::temporal_core_rounding::maximum_instant_increment;
use crate::runtime::temporal_calendar::ISO8601_CALENDAR_ID;
use crate::runtime::temporal_duration::TemporalDuration;
use crate::runtime::temporal_time_zone::get_offset_nanoseconds_for;
use crate::runtime::temporal_zoned_date_time::{create_temporal_zoned_date_time, to_temporal_time_zone_identifier, TemporalZonedDateTime};
use crate::runtime::temporal_object::{
    ellipsize_at, extract_difference_options, string_units, temporal_fractional_second_digits, temporal_rounding_increment,
    temporal_rounding_mode, temporal_unit_type, temporal_unit_valued, throw_range_error_with_units,
    to_seconds_string_precision_record, validate_temporal_unit_value, AllowedUnit, DifferenceOperation, Inclusivity,
    Precision, RoundingMode, TemporalUnit, TemporalUnitDefault, UnitGroup, UnitOption,
};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo TemporalInstant::s_info`.
pub static TEMPORAL_INSTANT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo TemporalInstantPrototype::s_info`.
pub static TEMPORAL_INSTANT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Temporal.Instant", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: Some(&PROTOTYPE_TABLE), inherits_js_type_range: None };

/// `const ClassInfo TemporalInstantConstructor::s_info`.
pub static TEMPORAL_INSTANT_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&CONSTRUCTOR_TABLE), inherits_js_type_range: None,
};

/// `class TemporalInstant final : public JSNonFinalObject`.
pub struct TemporalInstant {
    base: JSNonFinalObject,
    /// `m_exactTime`: imutável depois de criado.
    exact_time: ExactTime,
}

/// O `TemporalInstant*`.
pub type TemporalInstantRef = Rc<TemporalInstant>;

impl std::fmt::Debug for TemporalInstant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TemporalInstant")
            .field("cell_id", &self.base.cell_id())
            .field("exact_time", &self.exact_time)
            .finish()
    }
}

impl std::ops::Deref for TemporalInstant {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl TemporalInstant {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalInstant::STRUCTURE_FLAGS),
            &TEMPORAL_INSTANT_S_INFO,
        )
    }

    /// `create(vm, structure, exactTime)`: o construtor e o registro da célula.
    pub fn create(vm: &VM, structure: StructureRef, exact_time: ExactTime) -> TemporalInstantRef {
        debug_assert!(exact_time.is_valid());
        let cell_id = cell_registry::reserve();
        let object = Rc::new(TemporalInstant { base: JSNonFinalObject::new(vm, structure), exact_time });
        object.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TemporalInstant(Rc::clone(&object)));
        debug_assert_eq!(object.type_(), JSType::ObjectType);
        object
    }

    /// `dynamicDowncast<TemporalInstant>` pelo `cell_id` de `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<TemporalInstantRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::TemporalInstant(object)) => Some(object),
            _ => None,
        }
    }

    /// `dynamicDowncast<TemporalInstant>(value)`.
    pub fn from_value(value: &JSValue) -> Option<TemporalInstantRef> {
        match value {
            JSValue::Cell(cell_id) => TemporalInstant::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `exactTime()`.
    pub fn exact_time(&self) -> ExactTime {
        self.exact_time
    }

    /// `toString(precision = { { Auto, 0 }, Nanosecond, 1 })`.
    pub fn to_string_default(&self) -> String {
        instant_to_string(self.exact_time, None, to_seconds_string_precision_record(None, None))
    }
}

impl JSGlobalObject {
    /// `instantStructure()`: o `LazyClassStructure` de `Temporal.Instant`.
    pub fn instant_structure(&self) -> StructureRef {
        crate::runtime::temporal_object::lazy_temporal_structure(self, "Instant", |data| data.instant_structure.clone())
    }
}

// ---------------------------------------------------------------------------------------------
// TemporalInstant.cpp
// ---------------------------------------------------------------------------------------------

/// `createTemporalInstant(globalObject, exactTime, newTarget)`: a estrutura vem do `new.target`.
fn create_temporal_instant(global_object: &JSGlobalObject, exact_time: ExactTime, call: &HostCall) -> Result<TemporalInstantRef, Thrown> {
    // Passos 1 a 3: `OrdinaryCreateFromConstructor(newTarget, "%Temporal.Instant.prototype%", ...)`.
    debug_assert!(exact_time.is_valid());
    let structure = get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| realm.instant_structure())?;
    // Passos 4 e 5.
    Ok(TemporalInstant::create(global_object.vm(), structure, exact_time))
}

/// `TemporalInstant::create(vm, globalObject->instantStructure(), exactTime)`.
fn create_instant(global_object: &JSGlobalObject, exact_time: ExactTime) -> TemporalInstantRef {
    TemporalInstant::create(global_object.vm(), global_object.instant_structure(), exact_time)
}

/// O `ExactTime` de um `JSBigInt` dentro da faixa de `Temporal.Instant`, ou `None` se não cabe.
fn exact_time_from_big_int(big_int: &JSBigInt) -> Option<ExactTime> {
    // Duas palavras de 64 bits cobrem o intervalo; a segunda com o bit alto aceso estoura o `Int128`.
    let length = big_int.length();
    if length > 2 || (length > 1 && big_int.digit(1) & 0x8000_0000_0000_0000 != 0) {
        return None;
    }
    let low = if length > 0 { u128::from(big_int.digit(0)) } else { 0 };
    let high = if length > 1 { u128::from(big_int.digit(1)) } else { 0 };
    let magnitude = (high << 64 | low) as i128;
    let exact_time = ExactTime::new(if big_int.sign() { -magnitude } else { magnitude });
    exact_time.is_valid().then_some(exact_time)
}

/// `bigIntValueToExactTime(globalObject, bigIntValue, typeName)`: o `RangeError` é lançado aqui.
pub fn big_int_value_to_exact_time(global_object: &JSGlobalObject, big_int_value: JSValue, type_name: &str) -> Result<ExactTime, Thrown> {
    let big_int = big_int_of(big_int_value).expect("bigIntValueToExactTime sem BigInt");
    match exact_time_from_big_int(&big_int) {
        Some(exact_time) => Ok(exact_time),
        None => {
            // O texto do número, ou a frase genérica se a conversão para decimal lança.
            let shown = big_int.to_string(global_object, 10);
            let units = shown.characters_without_null_termination().unwrap_or_default();
            let shown = ellipsize_at(100, &units);
            Err(throw_range_error_with_units(
                global_object,
                "",
                &shown,
                &format!(" epoch nanoseconds is outside of the supported range for {type_name}"),
            ))
        }
    }
}

/// `TemporalInstant::toInstant(globalObject, item)` (`ToTemporalInstant`):
/// https://tc39.es/proposal-temporal/#sec-temporal-totemporalinstant
pub fn to_instant(global_object: &JSGlobalObject, item: JSValue) -> Result<TemporalInstantRef, Thrown> {
    let mut item = item;
    // Passo 1: se `item` é objeto.
    if item.is_object() {
        // Passo 1.a: o spec devolve uma instância NOVA, também para o instante de um `ZonedDateTime`.
        if let Some(instant) = TemporalInstant::from_value(&item) {
            return Ok(create_instant(global_object, instant.exact_time()));
        }
        if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&item) {
            return Ok(create_instant(global_object, zoned_date_time.exact_time()));
        }
        // Passo 1.c: `ToPrimitive(item, STRING)`.
        item = pending_or(global_object, item.to_primitive_preferred(PreferredPrimitiveType::PreferString))?;
    }

    // Passo 2: se não é `String`, `TypeError`.
    if !item.is_string() {
        return Err(Thrown::type_error("can only convert to Instant from object or string values"));
    }

    let units = string_units(global_object, item)?;
    let invalid = || throw_range_error_with_units(global_object, "'", &ellipsize_at(100, &units), "' is not a valid Temporal.Instant string");

    // Passo 3: `ParseISODateTime(item, « TemporalInstantString »)`.
    let Some(parsed) = parse_iso_date_time(&units, TemporalProductionSet::from(TemporalProduction::Instant)) else {
        return Err(invalid());
    };
    let (Some(plain_date), Some(plain_time), Some(time_zone)) = (parsed.date, parsed.time, parsed.time_zone) else {
        unreachable!("parseISODateTime com a produção Instant sempre devolve data, hora e fuso");
    };

    // Passo 5: `Z` é deslocamento 0; senão o deslocamento já lido.
    let offset_nanoseconds = if time_zone.z { 0 } else { time_zone.offset.expect("Instant sem Z traz o deslocamento") };

    // Passos 8 a 11: `fromISOPartsAndOffset` e `IsValidEpochNanoseconds`.
    let exact_time = ExactTime::from_iso_parts_and_offset(
        plain_date.year(),
        plain_date.month(),
        plain_date.day(),
        plain_time.hour(),
        plain_time.minute(),
        plain_time.second(),
        plain_time.millisecond(),
        plain_time.microsecond(),
        plain_time.nanosecond(),
        offset_nanoseconds,
    );
    if !exact_time.is_valid() {
        return Err(invalid());
    }

    // Passo 12: `CreateTemporalInstant`.
    Ok(create_instant(global_object, exact_time))
}

/// `TemporalInstant::fromEpochMilliseconds(globalObject, value)`:
/// https://tc39.es/proposal-temporal/#sec-temporal.instant.fromepochmilliseconds
fn from_epoch_milliseconds(global_object: &JSGlobalObject, value: JSValue) -> Result<TemporalInstantRef, Thrown> {
    // Passo 1: `ToNumber`.
    let epoch_milliseconds = pending_or(global_object, value.to_number())?;

    // Passo 2: `NumberToBigInt` embutido (`isInteger`, depois o inteiro).
    if !crate::runtime::math_common::is_integer(epoch_milliseconds) {
        let shown = wtf_to_rust(&js_number(crate::runtime::js_value::purify_nan(epoch_milliseconds)).to_wtf_string());
        return Err(Thrown::RangeError(format!("{shown} is not a valid integer number of epoch milliseconds")));
    }

    // Passo 3: `epochMilliseconds × 10^6`.
    let exact_time = ExactTime::from_epoch_milliseconds(epoch_milliseconds as i64);

    // Passo 4: `IsValidEpochNanoseconds`.
    if !exact_time.is_valid() {
        return Err(Thrown::RangeError(format!(
            "{} epoch nanoseconds is outside of supported range for Temporal.Instant",
            exact_time.as_string()
        )));
    }

    // Passo 5.
    Ok(create_instant(global_object, exact_time))
}

/// `TemporalInstant::fromEpochNanoseconds(globalObject, value)`:
/// https://tc39.es/proposal-temporal/#sec-temporal.instant.fromepochnanoseconds
fn from_epoch_nanoseconds(global_object: &JSGlobalObject, value: JSValue) -> Result<TemporalInstantRef, Thrown> {
    // Passo 1: `ToBigInt`.
    let big_int_value = pending_or(global_object, to_big_int(value))?;
    // Passo 2: estreita para `Int128` e confere a faixa.
    let exact_time = big_int_value_to_exact_time(global_object, big_int_value, "Temporal.Instant")?;
    // Passo 3.
    Ok(create_instant(global_object, exact_time))
}

/// `TemporalInstant::compare(globalObject, one, two)`:
/// https://tc39.es/proposal-temporal/#sec-temporal.instant.compare
fn compare(global_object: &JSGlobalObject, one: JSValue, two: JSValue) -> Result<JSValue, Thrown> {
    let one = to_instant(global_object, one)?;
    let two = to_instant(global_object, two)?;
    Ok(js_number(match one.exact_time().cmp(&two.exact_time()) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
    }))
}

// ---------------------------------------------------------------------------------------------
// TemporalInstantConstructor.cpp
// ---------------------------------------------------------------------------------------------

/// `callTemporalInstant`.
fn call_temporal_instant_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("Instant")
}

/// `constructTemporalInstant`: https://tc39.es/proposal-temporal/#sec-temporal.instant
fn construct_temporal_instant_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 2: `ToBigInt(epochNanoseconds)`.
    let big_int_value = pending_or(global_object, to_big_int(call.argument(0)))?;
    // Passo 3: `IsValidEpochNanoseconds`.
    let exact_time = big_int_value_to_exact_time(global_object, big_int_value, "Temporal.Instant")?;
    // Passo 4: `CreateTemporalInstant(epochNanoseconds, NewTarget)`.
    Ok(create_temporal_instant(global_object, exact_time, call)?.as_value())
}

/// `temporalInstantConstructorFuncFrom`.
fn constructor_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(to_instant(global_object, call.argument(0))?.as_value())
}

/// `temporalInstantConstructorFuncFromEpochMilliseconds`.
fn constructor_from_epoch_milliseconds_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(from_epoch_milliseconds(global_object, call.argument(0))?.as_value())
}

/// `temporalInstantConstructorFuncFromEpochNanoseconds`.
fn constructor_from_epoch_nanoseconds_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(from_epoch_nanoseconds(global_object, call.argument(0))?.as_value())
}

/// `temporalInstantConstructorFuncCompare`.
fn constructor_compare_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    compare(global_object, call.argument(0), call.argument(1))
}

host_function!(call_temporal_instant, call_temporal_instant_body);
host_function!(construct_temporal_instant, construct_temporal_instant_body);
host_function!(temporal_instant_constructor_func_from, constructor_from_body);
host_function!(temporal_instant_constructor_func_from_epoch_milliseconds, constructor_from_epoch_milliseconds_body);
host_function!(temporal_instant_constructor_func_from_epoch_nanoseconds, constructor_from_epoch_nanoseconds_body);
host_function!(temporal_instant_constructor_func_compare, constructor_compare_body);

/// `temporalInstantConstructorTableValues`, na ordem do `@begin`.
static CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 4] = [
    native_entry("from", temporal_instant_constructor_func_from, 1),
    native_entry("fromEpochMilliseconds", temporal_instant_constructor_func_from_epoch_milliseconds, 1),
    native_entry("fromEpochNanoseconds", temporal_instant_constructor_func_from_epoch_nanoseconds, 1),
    native_entry("compare", temporal_instant_constructor_func_compare, 2),
];

/// `temporalInstantConstructorTable`.
static CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &CONSTRUCTOR_TABLE_VALUES };

/// `TemporalInstantConstructor::create(vm, structure, instantPrototype)` com o `finishCreation`: as funções de
/// `temporalInstantConstructorTable` reificam no primeiro acesso.
fn create_instant_constructor(
    vm: &VM,
    _global_object: &JSGlobalObject,
    structure: StructureRef,
    instant_prototype: &JSObject,
) -> InternalFunctionRef {
    let constructor = InternalFunction::new(vm, structure, call_temporal_instant, Some(construct_temporal_instant));
    constructor.finish_creation(vm, 1, &WtfString::from_latin1(b"Instant"), PropertyAdditionMode::WithoutStructureTransition);
    constructor.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.prototype),
        instant_prototype.as_value(),
        DONT_ENUM | DONT_DELETE | READ_ONLY,
    );
    constructor
}

// ---------------------------------------------------------------------------------------------
// TemporalInstantPrototype.cpp
// ---------------------------------------------------------------------------------------------

/// `dynamicDowncast<TemporalInstant>(thisValue)` ou o `TypeError` de `Temporal.Instant.prototype.NAME`.
fn this_instant(call: &HostCall, member: &str) -> Result<TemporalInstantRef, Thrown> {
    TemporalInstant::from_value(&call.this_value()).ok_or_else(|| Thrown::TypeError(not_an_instant_message(member, "a")))
}

/// A mensagem de `this` inválido. O upstream escreve "a Instant" em quase todos os membros e
/// "an Instant" só em `toZonedDateTimeISO` (`TemporalInstantPrototype.cpp`), por isso o artigo é parâmetro.
fn not_an_instant_message(member: &str, article: &str) -> String {
    format!("Temporal.Instant.prototype.{member} called on value that's not {article} Instant")
}

/// `AddInstantOperation`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AddInstantOperation {
    Add,
    Subtract,
}

/// `addDurationToInstant(operation, instant, temporalDurationLike)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-adddurationtoinstant
fn add_duration_to_instant(
    global_object: &JSGlobalObject,
    operation: AddInstantOperation,
    instant: &TemporalInstant,
    duration_like: JSValue,
) -> HostResult {
    // Passo 1: `ToTemporalDuration(temporalDurationLike)`.
    let mut duration = TemporalDuration::to_temporal_duration_record(global_object, duration_like)?;

    // Passo 2: `subtract` nega a duração.
    if operation == AddInstantOperation::Subtract {
        duration = -duration;
    }

    // Passos 3 e 4: as unidades de data (`Y`, `Mo`, `W`, `D`) são do `ZonedDateTime`.
    for (unit, name) in [
        (TemporalUnit::Year, "years"),
        (TemporalUnit::Month, "months"),
        (TemporalUnit::Week, "weeks"),
        (TemporalUnit::Day, "days"),
    ] {
        if duration.field(unit) != 0.0 {
            return Err(Thrown::RangeError(format!(
                "Adding {name} not supported by Temporal.Instant. Try Temporal.ZonedDateTime instead"
            )));
        }
    }

    // Passos 5 e 6: `ToInternalDurationRecordWith24HourDays` e `AddInstant`, ambos em `ExactTime::add`.
    let Some(new_exact_time) = instant.exact_time().add(&duration) else {
        return Err(Thrown::range_error(match operation {
            AddInstantOperation::Add => "Addition is outside of supported range for Temporal.Instant",
            AddInstantOperation::Subtract => "Subtraction is outside of supported range for Temporal.Instant",
        }));
    };

    // Passo 7: `CreateTemporalInstant(ns)`; a faixa já foi validada por `ExactTime::add`.
    Ok(create_instant(global_object, new_exact_time).as_value())
}

/// `differenceTemporalInstant(operation, instant, other, options)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-differencetemporalinstant
fn difference_temporal_instant(
    global_object: &JSGlobalObject,
    operation: DifferenceOperation,
    instant: &TemporalInstant,
    other_value: JSValue,
    options_value: JSValue,
) -> HostResult {
    // Passo 1: `ToTemporalInstant(other)`.
    let other = to_instant(global_object, other_value)?;

    // Passos 2 e 3: `GetOptionsObject` e `GetDifferenceSettings` (o `since` nega o modo lá dentro).
    let (smallest_unit, largest_unit, rounding_mode, increment) = extract_difference_options(
        global_object,
        options_value,
        UnitGroup::Time,
        TemporalUnit::Nanosecond,
        TemporalUnit::Second,
        operation,
    )?;

    // Passo 4: `DifferenceInstant`.
    let internal_duration = instant.exact_time().difference(other.exact_time(), increment as u32, smallest_unit, rounding_mode)?;

    // Passo 5: `TemporalDurationFromInternal(internalDuration, largestUnit)`.
    let mut result = temporal_duration_from_internal(&internal_duration, largest_unit)?;

    // Passo 6: `since` nega o resultado.
    if operation == DifferenceOperation::Since {
        result = -result;
    }

    // Passo 7: `CreateTemporalDuration(result)`.
    let duration = TemporalDuration::create(global_object.vm(), &global_object.duration_structure(), result);
    Ok(JSValue::from_cell(duration.cell_id()))
}

/// `temporalInstantPrototypeFuncAdd`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.add
fn prototype_add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let instant = this_instant(call, "add")?;
    add_duration_to_instant(global_object, AddInstantOperation::Add, &instant, call.argument(0))
}

/// `temporalInstantPrototypeFuncSubtract`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.subtract
fn prototype_subtract_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let instant = this_instant(call, "subtract")?;
    add_duration_to_instant(global_object, AddInstantOperation::Subtract, &instant, call.argument(0))
}

/// `temporalInstantPrototypeFuncUntil`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.until
fn prototype_until_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let instant = this_instant(call, "until")?;
    difference_temporal_instant(global_object, DifferenceOperation::Until, &instant, call.argument(0), call.argument(1))
}

/// `temporalInstantPrototypeFuncSince`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.since
fn prototype_since_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let instant = this_instant(call, "since")?;
    difference_temporal_instant(global_object, DifferenceOperation::Since, &instant, call.argument(0), call.argument(1))
}

/// `temporalInstantPrototypeFuncRound`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.round
fn prototype_round_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2.
    let instant = this_instant(call, "round")?;

    // Passo 3: `roundTo` ausente é `TypeError`.
    let round_to_value = call.argument(0);
    if round_to_value.is_undefined() {
        return Err(Thrown::type_error("Temporal.Instant.prototype.round requires a roundTo option"));
    }

    let mut round_to: Option<JSValue> = None;
    let mut smallest: Option<TemporalUnit> = None;
    if round_to_value.is_string() {
        // Passo 4: o texto é o `smallestUnit` (sem o objeto intermediário).
        let text = to_rust_string(global_object, round_to_value)?;
        smallest = Some(
            temporal_unit_type(&text).ok_or_else(|| Thrown::range_error("smallestUnit is an invalid Temporal unit"))?,
        );
    } else {
        // Passo 5: `GetOptionsObject(roundTo)`.
        round_to = get_options_object(round_to_value)?;
    }

    // Passos 6 a 8: as opções em ordem alfabética.
    let rounding_increment = temporal_rounding_increment(global_object, round_to)?;
    let rounding_mode = temporal_rounding_mode(global_object, round_to, RoundingMode::HalfExpand)?;

    let smallest_unit = match smallest {
        None => {
            // Passos 9 e 10: `GetTemporalUnitValuedOption(roundTo, "smallestUnit", REQUIRED)` e a validação.
            let unit = temporal_unit_valued(global_object, round_to, "smallestUnit", TemporalUnitDefault::Required)?;
            validate_temporal_unit_value(unit, UnitGroup::Time, AllowedUnit::None, "smallestUnit")?;
            match unit {
                UnitOption::Unit(unit) => unit,
                UnitOption::Auto | UnitOption::Unset => unreachable!("smallestUnit REQUIRED validado como unidade de tempo"),
            }
        }
        Some(unit) => {
            // Passo 10 (caminho do texto).
            validate_temporal_unit_value(UnitOption::Unit(unit), UnitGroup::Time, AllowedUnit::None, "smallestUnit")?;
            unit
        }
    };

    // Passos 11 a 17: `ValidateTemporalRoundingIncrement(roundingIncrement, maximum, true)`.
    crate::runtime::temporal_core_rounding::validate_temporal_rounding_increment(
        rounding_increment,
        Some(maximum_instant_increment(smallest_unit)),
        Inclusivity::Inclusive,
    )?;

    // Passos 18 e 19: `RoundTemporalInstant` e `CreateTemporalInstant`.
    let rounded = instant.exact_time().round(rounding_increment as u32, smallest_unit, rounding_mode)?;
    Ok(create_instant(global_object, rounded).as_value())
}

/// `temporalInstantPrototypeFuncEquals`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.equals
fn prototype_equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let instant = this_instant(call, "equals")?;
    let other = to_instant(global_object, call.argument(0))?;
    Ok(js_boolean(instant.exact_time() == other.exact_time()))
}

/// `temporalInstantPrototypeFuncToString`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.tostring
fn prototype_to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    // Passos 1 e 2.
    let instant = this_instant(call, "toString")?;

    // Passo 3: `GetOptionsObject(options)`.
    let resolved_options = get_options_object(call.argument(0))?;

    // Caminho rápido: sem opções, precisão `auto`, sem arredondamento e sem fuso.
    let Some(resolved_options) = resolved_options else {
        return Ok(str_value(vm, &instant.to_string_default()));
    };

    // Passos 4 e 5: as opções em ordem alfabética.
    let digits = temporal_fractional_second_digits(global_object, Some(resolved_options))?;
    // Passo 6.
    let rounding_mode = temporal_rounding_mode(global_object, Some(resolved_options), RoundingMode::Trunc)?;
    // Passo 7.
    let smallest_unit_option = temporal_unit_valued(global_object, Some(resolved_options), "smallestUnit", TemporalUnitDefault::Unset)?;
    // Passo 8.
    let time_zone_value = get_property(global_object, resolved_options, "timeZone")?;

    // Passo 9: `ValidateTemporalUnitValue(smallestUnit, time)`.
    validate_temporal_unit_value(smallest_unit_option, UnitGroup::Time, AllowedUnit::None, "smallestUnit")?;
    let smallest_unit = match smallest_unit_option {
        UnitOption::Unit(unit) => Some(unit),
        UnitOption::Unset => None,
        UnitOption::Auto => unreachable!("smallestUnit validado sem `auto`"),
    };

    // Passo 10: `hour` é `RangeError`.
    if smallest_unit == Some(TemporalUnit::Hour) {
        return Err(Thrown::range_error("smallestUnit cannot be \"hour\" for Instant.toString"));
    }

    // Passo 11: `timeZone` diferente de `undefined` é `ToTemporalTimeZoneIdentifier(timeZone)`.
    let time_zone = if time_zone_value.is_undefined() { None } else { Some(to_temporal_time_zone_identifier(global_object, time_zone_value)?) };

    // Passo 12: `ToSecondsStringPrecisionRecord(smallestUnit, digits)`.
    let data = to_seconds_string_precision_record(smallest_unit, digits);

    // Passos 13 e 14: com precisão `auto` o incremento é 1 ns, e arredondar é no-op.
    let mut rounded_exact_time = instant.exact_time();
    if data.precision.0 != Precision::Auto {
        rounded_exact_time = rounded_exact_time.round(data.increment, data.unit, rounding_mode)?;
    }

    // Passo 15: `TemporalInstantToString(roundedInstant, timeZone, precision)`, o deslocamento do fuso se há um.
    let offset_ns = match &time_zone {
        Some(time_zone) => Some(get_offset_nanoseconds_for(time_zone, rounded_exact_time)?),
        None => None,
    };
    Ok(str_value(vm, &instant_to_string(rounded_exact_time, offset_ns, data)))
}

/// `temporalInstantPrototypeFuncToZonedDateTimeISO`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.tozoneddatetimeiso
fn prototype_to_zoned_date_time_iso_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2.
    let instant = TemporalInstant::from_value(&call.this_value())
        .ok_or_else(|| Thrown::TypeError(not_an_instant_message("toZonedDateTimeISO", "an")))?;
    // Passo 3: `timeZone = ? ToTemporalTimeZoneIdentifier(timeZoneIdentifier)`.
    let time_zone = to_temporal_time_zone_identifier(global_object, call.argument(0))?;
    // Passo 4: `CreateTemporalZonedDateTime(instant.[[EpochNanoseconds]], timeZone, "iso8601")`.
    Ok(create_temporal_zoned_date_time(global_object, instant.exact_time(), time_zone, ISO8601_CALENDAR_ID, None)?.as_value())
}

/// `temporalInstantPrototypeFuncToJSON`: https://tc39.es/proposal-temporal/#sec-temporal.instant.prototype.tojson
fn prototype_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let instant = this_instant(call, "toJSON")?;
    Ok(str_value(global_object.vm(), &instant.to_string_default()))
}

/// `temporalInstantPrototypeFuncToLocaleString`: https://tc39.es/proposal-temporal/#sup-temporal.instant.prototype.tolocalestring
/// Passo 3 (ECMA-402): `CreateDateTimeFormat(%Intl.DateTimeFormat%, locales, options, ~any~, ~all~)` e o
/// `FormatDateTime` do instante.
fn prototype_to_locale_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    this_instant(call, "toLocaleString")?;
    let text = intl_to_locale_string(global_object, call.argument(0), call.argument(1), Required::Any, Defaults::All, call.this_value())?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalInstantPrototypeFuncValueOf`: `Instant` não tem valor primitivo.
fn prototype_value_of_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error(
        "Temporal.Instant.prototype.valueOf must not be called. To compare Instant values, use Temporal.Instant.compare",
    ))
}

/// `temporalInstantPrototypeGetterEpochMilliseconds`: `𝔽(floor(ℝ(ns) / 10^6))`.
fn prototype_epoch_milliseconds(_global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    let instant = TemporalInstant::from_value(&this_value).ok_or_else(|| {
        Thrown::type_error("Temporal.Instant.prototype.epochMilliseconds called on value that's not a Instant")
    })?;
    Ok(js_number(instant.exact_time().floor_epoch_milliseconds() as f64))
}

/// `temporalInstantPrototypeGetterEpochNanoseconds`: `ℤ(instant.[[EpochNanoseconds]])`.
fn prototype_epoch_nanoseconds(global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    let instant = TemporalInstant::from_value(&this_value).ok_or_else(|| {
        Thrown::type_error("Temporal.Instant.prototype.epochNanoseconds called on value that's not a Instant")
    })?;
    let value = impl_result_value(JSBigInt::create_from_i128(instant.exact_time().epoch_nanoseconds()).map(ImplResult::Heap));
    pending_or(global_object, value)
}

host_function!(temporal_instant_prototype_func_add, prototype_add_body);
host_function!(temporal_instant_prototype_func_subtract, prototype_subtract_body);
host_function!(temporal_instant_prototype_func_until, prototype_until_body);
host_function!(temporal_instant_prototype_func_since, prototype_since_body);
host_function!(temporal_instant_prototype_func_round, prototype_round_body);
host_function!(temporal_instant_prototype_func_equals, prototype_equals_body);
host_function!(temporal_instant_prototype_func_to_zoned_date_time_iso, prototype_to_zoned_date_time_iso_body);
host_function!(temporal_instant_prototype_func_to_string, prototype_to_string_body);
host_function!(temporal_instant_prototype_func_to_json, prototype_to_json_body);
host_function!(temporal_instant_prototype_func_to_locale_string, prototype_to_locale_string_body);
host_function!(temporal_instant_prototype_func_value_of, prototype_value_of_body);
custom_getter!(temporal_instant_prototype_getter_epoch_milliseconds, prototype_epoch_milliseconds);
custom_getter!(temporal_instant_prototype_getter_epoch_nanoseconds, prototype_epoch_nanoseconds);

/// `instantPrototypeTableValues` de `TemporalInstantPrototype.lut.h`, na ordem do `@begin`.
static PROTOTYPE_TABLE_VALUES: [HashTableValue; 13] = [
    native_entry("add", temporal_instant_prototype_func_add, 1),
    native_entry("subtract", temporal_instant_prototype_func_subtract, 1),
    native_entry("until", temporal_instant_prototype_func_until, 1),
    native_entry("since", temporal_instant_prototype_func_since, 1),
    native_entry("round", temporal_instant_prototype_func_round, 1),
    native_entry("equals", temporal_instant_prototype_func_equals, 1),
    native_entry("toZonedDateTimeISO", temporal_instant_prototype_func_to_zoned_date_time_iso, 1),
    native_entry("toString", temporal_instant_prototype_func_to_string, 0),
    native_entry("toJSON", temporal_instant_prototype_func_to_json, 0),
    native_entry("toLocaleString", temporal_instant_prototype_func_to_locale_string, 0),
    native_entry("valueOf", temporal_instant_prototype_func_value_of, 0),
    HashTableValue {
        key: "epochMilliseconds",
        attributes: DONT_ENUM | READ_ONLY | CUSTOM_ACCESSOR,
        intrinsic: Intrinsic::NoIntrinsic,
        kind: Kind::CustomAccessor { getter: temporal_instant_prototype_getter_epoch_milliseconds, setter: None },
    },
    HashTableValue {
        key: "epochNanoseconds",
        attributes: DONT_ENUM | READ_ONLY | CUSTOM_ACCESSOR,
        intrinsic: Intrinsic::NoIntrinsic,
        kind: Kind::CustomAccessor { getter: temporal_instant_prototype_getter_epoch_nanoseconds, setter: None },
    },
];

/// `instantPrototypeTable`.
static PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &PROTOTYPE_TABLE_VALUES };

/// `TemporalInstantPrototype::create(vm, structure)` com o `finishCreation`: só o `@@toStringTag`; as funções e
/// os dois getters de `instantPrototypeTable` reificam no primeiro acesso.
fn create_instant_prototype(vm: &VM, global_object: &JSGlobalObject, object_prototype: JSValue) -> JSObjectRef {
    let structure = Structure::create(
        vm,
        Some(global_object),
        object_prototype,
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
        &TEMPORAL_INSTANT_PROTOTYPE_S_INFO,
    );
    let prototype = JSObject::allocate(vm, &structure);
    prototype.finish_creation(vm);
    put_to_string_tag(vm, &prototype, TEMPORAL_INSTANT_PROTOTYPE_S_INFO.class_name);
    prototype
}

/// O `Instant` do global (`m_instantStructure`, `LazyClassStructure`): o protótipo (`didBecomePrototype`), a
/// estrutura das instâncias, o construtor, o `constructor` do protótipo e a propriedade `Instant` do `Temporal`
/// (`DontEnum`, a entrada de `temporalObjectTable`).
pub fn install_temporal_instant(global_object: &JSGlobalObject, temporal: &JSObject, object_prototype: &JSObjectRef) {
    let vm = global_object.vm();
    let function_prototype = global_object.function_prototype().as_value();
    let prototype = create_instant_prototype(vm, global_object, object_prototype.as_value());
    prototype.did_become_prototype(vm);
    global_object.temporal_data.borrow_mut().instant_structure =
        Some(TemporalInstant::create_structure(vm, global_object, prototype.as_value()));

    let constructor_structure = Structure::create(
        vm,
        Some(global_object),
        function_prototype,
        TypeInfo::new(JSType::InternalFunctionType, InternalFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
        &TEMPORAL_INSTANT_CONSTRUCTOR_S_INFO,
    );
    let constructor = create_instant_constructor(vm, global_object, constructor_structure, &prototype);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    temporal.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"Instant".as_slice())), constructor.as_value(), DONT_ENUM);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_an_instant_message_article() {
        assert_eq!(
            not_an_instant_message("toZonedDateTimeISO", "an"),
            "Temporal.Instant.prototype.toZonedDateTimeISO called on value that's not an Instant"
        );
        assert_eq!(not_an_instant_message("add", "a"), "Temporal.Instant.prototype.add called on value that's not a Instant");
    }

    /// O mesmo caminho dos passos 3 a 11 de `toInstant`, sem a célula.
    fn epoch_nanoseconds(text: &str) -> Option<i128> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let parsed = parse_iso_date_time(&units, TemporalProductionSet::from(TemporalProduction::Instant))?;
        let (date, time, time_zone) = (parsed.date?, parsed.time?, parsed.time_zone?);
        let offset = if time_zone.z { 0 } else { time_zone.offset? };
        let exact = ExactTime::from_iso_parts_and_offset(
            date.year(),
            date.month(),
            date.day(),
            time.hour(),
            time.minute(),
            time.second(),
            time.millisecond(),
            time.microsecond(),
            time.nanosecond(),
            offset,
        );
        exact.is_valid().then(|| exact.epoch_nanoseconds())
    }

    #[test]
    fn instant_strings_use_z_or_the_offset() {
        assert_eq!(epoch_nanoseconds("1970-01-01T00:00:01Z"), Some(1_000_000_000));
        assert_eq!(epoch_nanoseconds("1970-01-01T00:00:01+01:00"), Some(1_000_000_000 - 3_600_000_000_000));
        assert_eq!(epoch_nanoseconds("1970-01-01T00:00:00.000000001Z"), Some(1));
        assert_eq!(epoch_nanoseconds("1970-01-01T00:00:00"), None);
        assert_eq!(epoch_nanoseconds("+275760-09-14T00:00:00Z"), None);
    }
}
