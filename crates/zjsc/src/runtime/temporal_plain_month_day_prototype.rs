//! Porte de `runtime/TemporalPlainMonthDayPrototype.{h,cpp}`: `Temporal.PlainMonthDay.prototype` (um
//! `JSNonFinalObject` com o `ClassInfo` `"Temporal.PlainMonthDay"`): `toPlainDate`, `toString`, `toJSON`,
//! `toLocaleString`, `with`, `equals`, `valueOf`, os acessores `calendarId`, `day` e `monthCode`
//! (`DontEnum|ReadOnly|CustomAccessor`) e `@@toStringTag`.
//!
//! DIVERGÊNCIAS:
//! - `toLocaleString` delega ao `IntlDateTimeFormat` com `PlainMonthDay` (`intl_date_time_format/temporal.rs`).
//! - `toPlainDate` e os acessores só têm o ramo do calendário ISO (toda célula é ISO; ver `temporal_calendar.rs`):
//!   `era` e `eraYear` do `item` não são lidos porque `calendarHasEras` é falso para `iso8601`.
//! - O acessor `calendarId` usa a mensagem de marca `...prototype.calendar called on...`, como o C++.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_to_string_tag};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_date_time_format::temporal::to_locale_string as intl_to_locale_string;
use crate::runtime::intl_date_time_format::{Defaults, Required};
use crate::runtime::intl_support::{get_property, str_value, to_rust_string};
use crate::runtime::iso8601::parse_month_code;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{calendar_has_eras, calendar_id_to_string, calendar_is_iso, read_calendar_fields_from_object, FieldSetType};
use crate::runtime::temporal_core_calendar_fields::{date_from_fields, plain_month_day_with, CalendarFieldsIn};
use crate::runtime::temporal_object::{is_partial_temporal_object, to_integer_with_truncation, to_temporal_overflow_value, TemporalOverflow};
use crate::runtime::temporal_plain_date::create_temporal_date;
use crate::runtime::temporal_plain_month_day::{create_temporal_month_day, TemporalPlainMonthDay, TemporalPlainMonthDayRef};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainMonthDayPrototype::s_info` (`"Temporal.PlainMonthDay"`).
pub static TEMPORAL_PLAIN_MONTH_DAY_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal.PlainMonthDay",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&PROTOTYPE_TABLE), inherits_js_type_range: None,
};

/// O `dynamicDowncast<TemporalPlainMonthDay>(callFrame->thisValue())` com o `TypeError` de marca de cada membro.
fn this_month_day(this_value: JSValue, member: &str) -> Result<TemporalPlainMonthDayRef, Thrown> {
    TemporalPlainMonthDay::from_value(&this_value)
        .ok_or_else(|| Thrown::TypeError(format!("Temporal.PlainMonthDay.prototype.{member} called on value that's not a PlainMonthDay")))
}

/// `temporalPlainMonthDayPrototypeFuncToString`: https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday.prototype.tostring
fn to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let month_day = this_month_day(call.this_value(), "toString")?;
    // Passos 3 a 5: `GetOptionsObject`, `GetTemporalShowCalendarNameOption` e `TemporalMonthDayToString`.
    Ok(str_value(global_object.vm(), &month_day.to_string_with_options(global_object, call.argument(0))?))
}

/// `temporalPlainMonthDayPrototypeFuncToJSON`: https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday.prototype.tojson
fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let month_day = this_month_day(call.this_value(), "toJSON")?;
    Ok(str_value(global_object.vm(), &month_day.to_string()))
}

/// `temporalPlainMonthDayPrototypeFuncToLocaleString`: https://tc39.es/proposal-temporal/#sup-temporal.plainmonthday.prototype.tolocalestring
fn to_locale_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    this_month_day(call.this_value(), "toLocaleString")?;
    let text = intl_to_locale_string(global_object, call.argument(0), call.argument(1), Required::Date, Defaults::Date, call.this_value())?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalPlainMonthDayPrototypeFuncWith`: https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday.prototype.with
fn with_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let month_day = this_month_day(call.this_value(), "with")?;

    // Passo 3: `IsPartialTemporalObject(temporalMonthDayLike)` falso é `TypeError`.
    let temporal_month_day_like = call.argument(0);
    if !is_partial_temporal_object(global_object, temporal_month_day_like)? {
        return Err(Thrown::type_error("First argument to Temporal.PlainMonthDay.prototype.with must be a partial Temporal object"));
    }

    // Passo 4: `calendar = plainMonthDay.[[Calendar]]`, guardado no receptor.
    let calendar_id = month_day.calendar_id();

    // Passo 6: `PrepareCalendarFields(calendar, temporalMonthDayLike, «year, month, monthCode, day», «», ~partial~)`.
    let partial_fields = read_calendar_fields_from_object(global_object, temporal_month_day_like, calendar_id, FieldSetType::MonthDay, None)?;
    // `~partial~` lança `TypeError` se nenhum dos campos pedidos veio com valor.
    if partial_fields.day.is_none()
        && partial_fields.month.is_none()
        && partial_fields.month_code.is_none()
        && partial_fields.year.is_none()
        && partial_fields.era.is_none()
        && partial_fields.era_year.is_none()
    {
        return Err(Thrown::type_error("Object must contain at least one Temporal date property"));
    }

    // Passos 8 e 9: `GetOptionsObject` e `GetTemporalOverflowOption`.
    let overflow = to_temporal_overflow_value(global_object, call.argument(1))?;

    // Passos 5, 7 e 10: `ISODateToFields`, `CalendarMergeFields` e `CalendarMonthDayFromFields`.
    let resolved = plain_month_day_with(calendar_id, *month_day.plain_month_day().iso_plain_date(), &partial_fields, overflow)?;

    // Passo 11: `CreateTemporalMonthDay(isoDate, calendar)`.
    Ok(create_temporal_month_day(global_object, resolved.iso_date, calendar_id, None)?.as_value())
}

/// `temporalPlainMonthDayPrototypeFuncEquals`: https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday.prototype.equals
fn equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let month_day = this_month_day(call.this_value(), "equals")?;
    // Passo 3: `other = ? ToTemporalMonthDay(other)`.
    let other = TemporalPlainMonthDay::from(global_object, call.argument(0), JSValue::Undefined)?;
    // Passo 4: `CompareISODate` diferente de 0 é falso.
    if month_day.plain_month_day() != other.plain_month_day() {
        return Ok(js_boolean(false));
    }
    // Passo 5: `CalendarEquals`.
    Ok(js_boolean(month_day.calendar_id() == other.calendar_id()))
}

/// `temporalPlainMonthDayPrototypeFuncToPlainDate`: https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday.prototype.toplaindate
fn to_plain_date_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let month_day = this_month_day(call.this_value(), "toPlainDate")?;

    // Passo 3: `item` que não é objeto é `TypeError`.
    let item = call.argument(0);
    if !item.is_object() {
        return Err(Thrown::type_error("Temporal.PlainMonthDay.prototype.toPlainDate: item is not an object"));
    }

    // Passo 4: `calendar = plainMonthDay.[[Calendar]]`.
    let calendar_id = month_day.calendar_id();

    // Passos 5 a 7: `PrepareCalendarFields(calendar, item, «year», «», «»)` lê só `year` (o mês e o dia vêm do
    // receptor, e reler do `item` quebraria a ordem de operações), e `CalendarMergeFields` junta os dois.
    // `CalendarExtraFields` expande `year` para `era`, `eraYear` e `year` (ordem alfabética) nos calendários com era.
    let mut merged = CalendarFieldsIn::default();
    if calendar_has_eras(calendar_id) {
        let era_property = get_property(global_object, item, "era")?;
        if !era_property.is_undefined() {
            merged.era = Some(to_rust_string(global_object, era_property)?);
        }
        let era_year_property = get_property(global_object, item, "eraYear")?;
        if !era_year_property.is_undefined() {
            let era_year = to_integer_with_truncation(global_object, era_year_property)?;
            if !era_year.is_finite() {
                return Err(Thrown::range_error("eraYear must be finite"));
            }
            merged.era_year = Some(era_year as i32);
        }
    }
    let year_property = get_property(global_object, item, "year")?;
    if !year_property.is_undefined() {
        let year = to_integer_with_truncation(global_object, year_property)?;
        if !year.is_finite() {
            return Err(Thrown::range_error("year must be finite"));
        }
        // `clampTo<int32_t>`: a conversão do Rust satura.
        merged.year = Some(year as i32);
    }
    if merged.year.is_none() && (merged.era.is_none() || merged.era_year.is_none()) {
        return Err(Thrown::type_error("Temporal.PlainMonthDay.prototype.toPlainDate: item does not have a year or era/eraYear field"));
    }

    // Passo 5 (lado do receptor): o mês e o dia da data ISO guardada; no calendário não ISO, o `monthCode` e o dia
    // do calendário.
    if calendar_is_iso(calendar_id) {
        merged.month = Some(u32::from(month_day.month()));
        merged.day = u8::try_from(month_day.day()).ok();
    } else {
        let receiver_fields = month_day.calendar_date_fields();
        merged.month_code = parse_month_code(&receiver_fields.month_code.encode_utf16().collect::<Vec<u16>>());
        merged.day = Some(receiver_fields.day);
    }
    // Passo 8: `isoDate = ? CalendarDateFromFields(calendar, merged, ~constrain~)`.
    let resolved = date_from_fields(calendar_id, &merged, TemporalOverflow::Constrain)?;

    // Passo 9: `CreateTemporalDate(isoDate, calendar)`.
    Ok(create_temporal_date(global_object, resolved.iso_date, resolved.calendar_id, None)?.as_value())
}

/// `temporalPlainMonthDayPrototypeFuncValueOf`: https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday.prototype.valueof
fn value_of_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error(
        "Temporal.PlainMonthDay.prototype.valueOf must not be called. To compare PlainMonthDay values, use Temporal.PlainDate.compare on the corresponding PlainDate objects.",
    ))
}

host_function!(temporal_plain_month_day_prototype_func_to_plain_date, to_plain_date_body);
host_function!(temporal_plain_month_day_prototype_func_to_string, to_string_body);
host_function!(temporal_plain_month_day_prototype_func_to_json, to_json_body);
host_function!(temporal_plain_month_day_prototype_func_to_locale_string, to_locale_string_body);
host_function!(temporal_plain_month_day_prototype_func_with, with_body);
host_function!(temporal_plain_month_day_prototype_func_equals, equals_body);
host_function!(temporal_plain_month_day_prototype_func_value_of, value_of_body);

// https://tc39.es/proposal-temporal/#sec-get-temporal.plainmonthday.prototype.calendarid
// A mensagem de marca do C++ diz `calendar`, não `calendarId`.
crate::temporal_getter!(temporal_plain_month_day_prototype_getter_calendar_id, calendar_id_body, this_month_day, "calendar", |global, md| str_value(
    global.vm(),
    calendar_id_to_string(md.calendar_id())
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainmonthday.prototype.day
crate::temporal_getter!(temporal_plain_month_day_prototype_getter_day, day_body, this_month_day, "day", |_global, md| js_number(md.calendar_date_fields().day));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainmonthday.prototype.monthcode
crate::temporal_getter!(temporal_plain_month_day_prototype_getter_month_code, month_code_body, this_month_day, "monthCode", |global, md| str_value(
    global.vm(),
    &md.calendar_date_fields().month_code
));

/// `plain month dayPrototypeTableValues`, na ordem do `@begin`: os métodos (`DontEnum|Function`) e os acessores
/// (`DontEnum|ReadOnly|CustomAccessor`, sem setter).
static PROTOTYPE_TABLE_VALUES: [HashTableValue; 10] = [
    native_entry("toPlainDate", temporal_plain_month_day_prototype_func_to_plain_date, 1),
    native_entry("toString", temporal_plain_month_day_prototype_func_to_string, 0),
    native_entry("toJSON", temporal_plain_month_day_prototype_func_to_json, 0),
    native_entry("toLocaleString", temporal_plain_month_day_prototype_func_to_locale_string, 0),
    native_entry("with", temporal_plain_month_day_prototype_func_with, 1),
    native_entry("equals", temporal_plain_month_day_prototype_func_equals, 1),
    native_entry("valueOf", temporal_plain_month_day_prototype_func_value_of, 0),
    custom_getter_entry("calendarId", temporal_plain_month_day_prototype_getter_calendar_id),
    custom_getter_entry("day", temporal_plain_month_day_prototype_getter_day),
    custom_getter_entry("monthCode", temporal_plain_month_day_prototype_getter_month_code),
];

/// A tabela estática do protótipo.
static PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &PROTOTYPE_TABLE_VALUES };

/// `class TemporalPlainMonthDayPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalPlainMonthDayPrototype;

impl TemporalPlainMonthDayPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`; sem `HasStaticPropertyTable` porque os
    /// membros são postos direto em `finishCreation`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | crate::runtime::js_type_info::HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainMonthDayPrototype::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_MONTH_DAY_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: o construtor e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        TemporalPlainMonthDayPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm)` com `plainMonthDayPrototypeTable`: os métodos (`DontEnum|Function`), os acessores
    /// (`DontEnum|ReadOnly|CustomAccessor`) e `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        put_to_string_tag(vm, prototype, TEMPORAL_PLAIN_MONTH_DAY_PROTOTYPE_S_INFO.class_name);
    }
}
