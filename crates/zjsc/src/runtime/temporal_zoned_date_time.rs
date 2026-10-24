//! Porte de `runtime/TemporalZonedDateTime.{h,cpp}`: a célula `Temporal.ZonedDateTime` (`TemporalZonedDateTime`,
//! um `JSNonFinalObject` com `m_exactTime`, `m_timeZone` e `m_calendarID`), `createTemporalZonedDateTime`,
//! `toString` (`TemporalZonedDateTimeToString`), `timeZoneFromRecord`, `from` (`ToTemporalZonedDateTime`), e as
//! partes de `TemporalObject.cpp` e `TemporalCalendar.cpp` que o fuso usa: `toTemporalTimeZoneIdentifier`,
//! `timeZoneFromIdentifierParseRecord` e `readZonedDateTimeFieldsFromObject`.
//!
//! DIVERGÊNCIAS:
//! - A célula aceita qualquer calendário embutido: `from` com objeto (campos de calendário, `era`/`eraYear`/
//!   `monthCode`, via `interpret_temporal_date_time_fields`), `from` com string (`[u-ca=]`), construtor e
//!   `withCalendar` guardam o `CalendarID`, e a aritmética de data despacha por ele no núcleo. Os dois `create` do
//!   C++ (intrínseco e com `newTarget`) são um só, com `new_target` opcional.
//! - O fuso é o `TimeZone` de `temporal_time_zone.rs` (jiff no lugar do ICU; ver as divergências de lá).
//! - `toString` é a função livre [`zoned_date_time_to_string`] (testável sem `VM`); `showOffset`, `showTimeZone` e
//!   `showCalendar` são enums no lugar das `StringView` do C++.
//! - `toEpochArgsFromString` e `toEpochArgsFromPropertyBag` do C++ são `epoch_args_from_string` e
//!   `epoch_args_from_property_bag`.
//! - `getEpochNanosecondsFor(globalObject, ...)` estático do C++ só embrulha o núcleo com o `RangeError`; o `?` de
//!   `TemporalError` para `Thrown` faz isso, sem função de repasse.

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{pending_or, Thrown};
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::intl_support::{get_property, to_rust_string};
use crate::runtime::iso8601::{
    format_time_zone_offset_string, parse_iso_date_time, parse_temporal_time_zone_string, parse_utc_offset, temporal_date_to_string,
    temporal_time_to_string, ExactTime, ISOStringTimeZoneParseRecord, PlainDate, PlainDateTime, PlainTime, SubMinutePrecision,
    TemporalProduction, TimeZoneIdentifierParseRecord, TimeZoneNameOrOffset,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{
    calendar_id_to_string, calendar_is_iso, calendar_has_eras, get_temporal_calendar_identifier_with_iso_default, interpret_temporal_date_time_fields,
    is_builtin_calendar, parse_month_code_value, CalendarID, CalendarNameOption, ISO8601_CALENDAR_ID,
};
use crate::runtime::temporal_core_calendar_fields::{CalendarFieldsIn, TimeFieldsIn};
use crate::runtime::temporal_core_rounding::round_number_to_increment_as_if_positive;
use crate::runtime::temporal_core_types::TemporalResult;
use crate::runtime::temporal_core_zoned_date_time::{interpret_iso_date_time_offset, MatchBehaviour, UseStartOfDay};
use crate::runtime::temporal_object::{
    ellipsize_at, length_in_nanoseconds, string_units, throw_range_error_with_units, to_integer_with_truncation, to_seconds_string_precision_record,
    to_temporal_disambiguation, to_temporal_offset, to_temporal_overflow, OffsetBehaviour, PrecisionData, RoundingMode, TemporalDisambiguation,
    TemporalOffsetDisambiguation, TemporalOverflow,
};
use crate::runtime::temporal_time_zone::{
    exact_time_to_local_date_and_time, get_iso_date_time_for, get_offset_nanoseconds_for, intl_resolve_time_zone_id, TimeZone,
};
use crate::runtime::vm::VM;
use crate::wtf::option_set::OptionSet;

/// `const ClassInfo TemporalZonedDateTime::s_info` (`"Object"`).
pub static TEMPORAL_ZONED_DATE_TIME_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class TemporalZonedDateTime final : public JSNonFinalObject`.
pub struct TemporalZonedDateTime {
    base: JSNonFinalObject,
    /// `m_exactTime`.
    exact_time: ExactTime,
    /// `m_timeZone`.
    time_zone: TimeZone,
    /// `m_calendarID`.
    calendar_id: CalendarID,
}

/// A referência à célula, o `*` do C++.
pub type TemporalZonedDateTimeRef = Rc<TemporalZonedDateTime>;

impl std::ops::Deref for TemporalZonedDateTime {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSGlobalObject {
    /// `zonedDateTimeStructure()`: o `LazyClassStructure` de `Temporal.ZonedDateTime`.
    pub fn zoned_date_time_structure(&self) -> StructureRef {
        crate::runtime::temporal_object::lazy_temporal_structure(self, "ZonedDateTime", |data| data.zoned_date_time_structure.clone())
    }
}

crate::intl_enum! {
    /// Os valores de `offset` de `GetTemporalShowOffsetOption`.
    ShowOffsetOption { Auto => "auto", Never => "never" }
}

crate::intl_enum! {
    /// Os valores de `timeZoneName` de `GetTemporalShowTimeZoneNameOption`.
    ShowTimeZoneNameOption { Auto => "auto", Never => "never", Critical => "critical" }
}

impl TemporalZonedDateTime {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalZonedDateTime::STRUCTURE_FLAGS),
            &TEMPORAL_ZONED_DATE_TIME_S_INFO,
        )
    }

    /// `create(vm, structure, exactTime, timeZone, calendarID)` (e o `finishCreation`): a célula e o registro dela.
    pub fn create(
        vm: &VM,
        structure: &StructureRef,
        exact_time: ExactTime,
        time_zone: TimeZone,
        calendar_id: CalendarID,
    ) -> TemporalZonedDateTimeRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(TemporalZonedDateTime { base: JSNonFinalObject::new(vm, Rc::clone(structure)), exact_time, time_zone, calendar_id });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TemporalZonedDateTime(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<TemporalZonedDateTime>(value)`.
    pub fn from_value(value: &JSValue) -> Option<TemporalZonedDateTimeRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::TemporalZonedDateTime(cell)) => Some(cell),
                _ => None,
            },
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

    /// `timeZone()`.
    pub fn time_zone(&self) -> &TimeZone {
        &self.time_zone
    }

    /// `timeZoneId()`.
    pub fn time_zone_id(&self) -> String {
        self.time_zone.to_string()
    }

    /// `calendarID()`.
    pub fn calendar_id(&self) -> CalendarID {
        self.calendar_id
    }

    /// `calendarId()`.
    pub fn calendar_id_string(&self) -> &'static str {
        calendar_id_to_string(self.calendar_id)
    }

    /// `getOffsetNanoseconds(globalObject)`.
    pub fn get_offset_nanoseconds(&self) -> Result<i64, Thrown> {
        Ok(get_offset_nanoseconds_for(&self.time_zone, self.exact_time)?)
    }

    /// `getLocalDateTime(globalObject)` (`GetISODateTimeFor`): https://tc39.es/proposal-temporal/#sec-temporal-getisodatetimefor
    pub fn get_local_date_time(&self) -> Result<PlainDateTime, Thrown> {
        Ok(get_iso_date_time_for(&self.time_zone, self.exact_time)?)
    }

    /// `toString(globalObject)`: tudo `~auto~` (o que `toJSON` e o `toString` sem argumento dão).
    pub fn to_string_default(&self) -> Result<String, Thrown> {
        Ok(zoned_date_time_to_string(
            self.exact_time,
            &self.time_zone,
            self.calendar_id,
            to_seconds_string_precision_record(None, None),
            RoundingMode::Trunc,
            ShowOffsetOption::Auto,
            ShowTimeZoneNameOption::Auto,
            CalendarNameOption::Auto,
        )?)
    }
}

/// `createTemporalZonedDateTime(globalObject, exactTime, timeZone, calendarID[, newTarget])`:
/// https://tc39.es/proposal-temporal/#sec-temporal-createtemporalzoneddatetime
/// `new_target` é `Some((newTarget, jsCallee))` na construção e `None` na criação intrínseca.
pub fn create_temporal_zoned_date_time(
    global_object: &JSGlobalObject,
    exact_time: ExactTime,
    time_zone: TimeZone,
    calendar_id: CalendarID,
    new_target: Option<(JSValue, usize)>,
) -> Result<TemporalZonedDateTimeRef, Thrown> {
    // Passo 1: `IsValidEpochNanoseconds`.
    debug_assert!(exact_time.is_valid());

    // Passos 2 e 3: `newTarget` ausente é `%Temporal.ZonedDateTime%`; `OrdinaryCreateFromConstructor`.
    let structure = match new_target {
        None => global_object.zoned_date_time_structure(),
        Some((new_target, constructor)) => {
            get_derived_structure_in_realm(global_object, new_target, constructor, |realm| realm.zoned_date_time_structure())?
        }
    };

    // Passos 4 a 7.
    Ok(TemporalZonedDateTime::create(global_object.vm(), &structure, exact_time, time_zone, calendar_id))
}

/// `TemporalZonedDateTimeToString(zonedDateTime, precision, showCalendar, showTimeZone, showOffset, roundingMode)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-temporalzoneddatetimetostring
#[allow(clippy::too_many_arguments)]
pub fn zoned_date_time_to_string(
    exact_time: ExactTime,
    time_zone: &TimeZone,
    calendar_id: CalendarID,
    precision: PrecisionData,
    rounding_mode: RoundingMode,
    show_offset: ShowOffsetOption,
    show_time_zone: ShowTimeZoneNameOption,
    show_calendar: CalendarNameOption,
) -> TemporalResult<String> {
    // Passos 4 e 5: `epochNs = RoundTemporalInstant(epochNs, increment, unit, roundingMode)`.
    let increment_ns = length_in_nanoseconds(precision.unit) * i128::from(precision.increment);
    let mut epoch_ns = exact_time.epoch_nanoseconds();
    if increment_ns > 0 {
        epoch_ns = round_number_to_increment_as_if_positive(epoch_ns, increment_ns, rounding_mode);
    }
    let rounded_exact_time = ExactTime::new(epoch_ns);

    // Passos 6 e 7: `offsetNanoseconds = GetOffsetNanosecondsFor(timeZone, epochNs)`.
    let offset_ns = get_offset_nanoseconds_for(time_zone, rounded_exact_time)?;

    // Passo 8: `isoDateTime = GetISODateTimeFor(timeZone, epochNs)`.
    let PlainDateTime { date, time } = exact_time_to_local_date_and_time(rounded_exact_time, offset_ns);

    // Passo 9: `ISODateTimeToString(isoDateTime, "iso8601", precision, ~never~)`.
    let mut result = format!("{}T{}", temporal_date_to_string(date), temporal_time_to_string(time, precision.precision));

    // Passos 10 e 11: o deslocamento arredondado ao minuto (`FormatDateTimeUTCOffsetRounded`), salvo `never`.
    if show_offset != ShowOffsetOption::Never {
        const NS_PER_MINUTE: i64 = 60_000_000_000;
        let mut offset_minutes = offset_ns / NS_PER_MINUTE;
        let remainder = offset_ns % NS_PER_MINUTE;
        if remainder > NS_PER_MINUTE / 2 || (remainder == NS_PER_MINUTE / 2 && offset_ns > 0) {
            offset_minutes += 1;
        } else if remainder < -(NS_PER_MINUTE / 2) || (remainder == -(NS_PER_MINUTE / 2) && offset_ns < 0) {
            offset_minutes -= 1;
        }
        result.push_str(&format_time_zone_offset_string(offset_minutes * NS_PER_MINUTE));
    }

    // Passos 12 e 13: `[timeZone]`, com `!` em `critical`, salvo `never`.
    if show_time_zone != ShowTimeZoneNameOption::Never {
        result.push('[');
        if show_time_zone == ShowTimeZoneNameOption::Critical {
            result.push('!');
        }
        result.push_str(&time_zone.to_string());
        result.push(']');
    }

    // Passo 14: `FormatCalendarAnnotation(calendar, showCalendar)`.
    let annotate = match show_calendar {
        CalendarNameOption::Always | CalendarNameOption::Critical => true,
        CalendarNameOption::Auto => !calendar_is_iso(calendar_id),
        CalendarNameOption::Never => false,
    };
    if annotate {
        result.push('[');
        if show_calendar == CalendarNameOption::Critical {
            result.push('!');
        }
        result.push_str("u-ca=");
        result.push_str(calendar_id_to_string(calendar_id));
        result.push(']');
    }

    // Passo 15.
    Ok(result)
}

/// `timeZoneFromRecord(record)`: o fuso do `[[TimeZoneAnnotation]]` de um `ISO String Time Zone Parse Record`; `None`
/// se o nome entre colchetes não é um fuso IANA conhecido.
pub fn time_zone_from_record(record: &ISOStringTimeZoneParseRecord) -> Option<TimeZone> {
    match &record.name_or_offset {
        TimeZoneNameOrOffset::Offset(offset_nanoseconds) => Some(TimeZone::UtcOffset(*offset_nanoseconds)),
        TimeZoneNameOrOffset::Name(name) => {
            debug_assert!(!name.is_empty());
            intl_resolve_time_zone_id(name).map(TimeZone::Id)
        }
    }
}

/// `timeZoneFromIdentifierParseRecord(parseRecord)`: `FormatOffsetTimeZoneIdentifier` do deslocamento, ou
/// `GetAvailableNamedTimeZoneIdentifier` do nome (`None` é o `RangeError` do chamador).
pub fn time_zone_from_identifier_parse_record(parse_record: &TimeZoneIdentifierParseRecord) -> Option<TimeZone> {
    if let Some(offset_minutes) = parse_record.offset_minutes {
        return Some(TimeZone::UtcOffset(offset_minutes * ExactTime::NS_PER_MINUTE as i64));
    }
    intl_resolve_time_zone_id(&parse_record.name).map(TimeZone::Id)
}

/// `toTemporalTimeZoneIdentifier(globalObject, item)` (`ToTemporalTimeZoneIdentifier`):
/// https://tc39.es/proposal-temporal/#sec-temporal-totemporaltimezoneidentifier
pub fn to_temporal_time_zone_identifier(global_object: &JSGlobalObject, item: JSValue) -> Result<TimeZone, Thrown> {
    // Passo 1: `ZonedDateTime` devolve o `[[TimeZone]]` dele.
    if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&item) {
        return Ok(zoned_date_time.time_zone().clone());
    }

    // Passo 2: quem não é `String` é `TypeError`.
    if !item.is_string() {
        return Err(Thrown::type_error("time zone must be a string or ZonedDateTime"));
    }
    let time_zone_string = string_units(global_object, item)?;
    let invalid = || {
        throw_range_error_with_units(global_object, "'", &ellipsize_at(100, &time_zone_string), "' is not a valid time zone identifier")
    };

    // Passo 3: `parseResult = ? ParseTemporalTimeZoneString(temporalTimeZoneLike)`.
    let Some(parse_result) = parse_temporal_time_zone_string(&time_zone_string) else {
        return Err(invalid());
    };

    // Passos 4 a 9: o deslocamento ou o nome disponível; indisponível é o `RangeError` do passo 8.
    time_zone_from_identifier_parse_record(&parse_result).ok_or_else(invalid)
}

/// `enum class ZonedDateTimeFieldMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZonedDateTimeFieldMode {
    /// `from()`: `timeZone` é o único campo obrigatório.
    Full,
    /// `with()`: tudo opcional, e `anyFieldSet` vale (`~partial~`).
    Partial,
    /// `Duration.relativeTo`: `timeZone` opcional; `day`, `year` e `month` ou `monthCode` ficam para o
    /// `CalendarResolveFields`.
    RelativeToDuration,
}

/// `struct ZonedDateTimeFields`: o que se lê de um objeto de propriedades de `ZonedDateTime` (`from()` e `with()`).
/// Os sinais de presença do C++ (`dayPresent`...) só serviam a `anyFieldSet` e a `timeZonePresent`, que aqui são o
/// `Option` do fuso e o `any_field_set`.
#[derive(Clone, Debug, Default)]
pub struct ZonedDateTimeFields {
    /// `dateFields`: os campos de calendário (de `PrepareCalendarFields`).
    pub date_fields: CalendarFieldsIn,
    /// Os campos de hora lidos (`std::optional<double>` do C++).
    pub time_fields: TimeFieldsIn,
    /// `offsetNs`: o `offset`, já em nanossegundos.
    pub offset_ns: Option<i64>,
    /// `timeZone` quando `timeZonePresent`.
    pub time_zone: Option<TimeZone>,
    /// Pelo menos um campo veio com valor (`with()`).
    pub any_field_set: bool,
}

/// `readTimeField(name, field)` de `readZonedDateTimeFieldsFromObject`: o campo de hora, `toIntegerWithTruncation` e
/// finito.
fn read_time_field(
    global_object: &JSGlobalObject,
    bag: JSValue,
    name: &str,
    field: &mut Option<f64>,
    any_field_set: &mut bool,
) -> Result<(), Thrown> {
    let value = get_property(global_object, bag, name)?;
    if value.is_undefined() {
        return Ok(());
    }
    let number = to_integer_with_truncation(global_object, value)?;
    if !number.is_finite() {
        return Err(Thrown::range_error("Temporal time properties must be finite"));
    }
    *field = Some(number);
    *any_field_set = true;
    Ok(())
}

/// `readZonedDateTimeFieldsFromObject<mode>(globalObject, bag, calendarId)` (`PrepareCalendarFields` de
/// `ZonedDateTime`): lê os 15 campos em ordem alfabética (`calendar` não é lido aqui, mas sim por
/// `GetTemporalCalendarIdentifierWithISODefault`; depois `day`, `era`, `eraYear`, `hour`, `microsecond`,
/// `millisecond`, `minute`, `month`, `monthCode`, `nanosecond`, `offset`, `second`, `timeZone`, `year`).
/// https://tc39.es/proposal-temporal/#sec-temporal-preparecalendarfields
pub fn read_zoned_date_time_fields_from_object(
    global_object: &JSGlobalObject,
    bag: JSValue,
    calendar_id: CalendarID,
    mode: ZonedDateTimeFieldMode,
) -> Result<ZonedDateTimeFields, Thrown> {
    let mut result = ZonedDateTimeFields::default();

    // `day` (`~to-positive-integer-with-truncation~`); ausente não é erro aqui, o `CalendarResolveFields` confere.
    let day = get_property(global_object, bag, "day")?;
    if !day.is_undefined() {
        let day = to_integer_with_truncation(global_object, day)?;
        if !(day > 0.0 && day.is_finite()) {
            return Err(Thrown::range_error("day property must be positive and finite"));
        }
        // `clampTo<uint8_t>`: a conversão do Rust satura.
        result.date_fields.day = Some(day as u8);
        result.any_field_set = true;
    }

    // `era` (`~to-string~`) e `eraYear`, entre `day` e `hour`, só em calendário com eras.
    if calendar_has_eras(calendar_id) {
        let era = get_property(global_object, bag, "era")?;
        if !era.is_undefined() {
            result.date_fields.era = Some(to_rust_string(global_object, era)?);
            result.any_field_set = true;
        }
        let era_year = get_property(global_object, bag, "eraYear")?;
        if !era_year.is_undefined() {
            let era_year = to_integer_with_truncation(global_object, era_year)?;
            if !era_year.is_finite() {
                return Err(Thrown::range_error("eraYear property must be finite"));
            }
            result.date_fields.era_year = Some(era_year as i32);
            result.any_field_set = true;
        }
    }

    // `hour`, `microsecond`, `millisecond`, `minute`.
    read_time_field(global_object, bag, "hour", &mut result.time_fields.hour, &mut result.any_field_set)?;
    read_time_field(global_object, bag, "microsecond", &mut result.time_fields.microsecond, &mut result.any_field_set)?;
    read_time_field(global_object, bag, "millisecond", &mut result.time_fields.millisecond, &mut result.any_field_set)?;
    read_time_field(global_object, bag, "minute", &mut result.time_fields.minute, &mut result.any_field_set)?;

    // `month` (`~to-positive-integer-with-truncation~`).
    let month = get_property(global_object, bag, "month")?;
    if !month.is_undefined() {
        let month = to_integer_with_truncation(global_object, month)?;
        if !(month.is_finite() && month > 0.0) {
            return Err(Thrown::range_error("month property must be a positive finite integer"));
        }
        // `clampTo<uint32_t>`.
        result.date_fields.month = Some(month as u32);
        result.any_field_set = true;
    }

    // `monthCode` (`~to-month-code~`).
    let month_code = get_property(global_object, bag, "monthCode")?;
    if !month_code.is_undefined() {
        result.date_fields.month_code = Some(parse_month_code_value(global_object, month_code)?);
        result.any_field_set = true;
    }

    // `nanosecond`.
    read_time_field(global_object, bag, "nanosecond", &mut result.time_fields.nanosecond, &mut result.any_field_set)?;

    // `offset` (`~to-offset-string~`: `ToPrimitive`, `String` e `ParseDateTimeUTCOffset`), já em nanossegundos.
    let offset = get_property(global_object, bag, "offset")?;
    if !offset.is_undefined() {
        let primitive = pending_or(global_object, offset.to_primitive_preferred(PreferredPrimitiveType::PreferString))?;
        if !primitive.is_string() {
            return Err(Thrown::type_error("offset must be a string"));
        }
        let offset_string = string_units(global_object, primitive)?;
        let Some(parsed) = parse_utc_offset(&offset_string, SubMinutePrecision::Yes) else {
            return Err(throw_range_error_with_units(
                global_object,
                "'",
                &ellipsize_at(100, &offset_string),
                "' is not a valid UTC offset string",
            ));
        };
        result.offset_ns = Some(parsed);
        result.any_field_set = true;
    }

    // `second`.
    read_time_field(global_object, bag, "second", &mut result.time_fields.second, &mut result.any_field_set)?;

    // `timeZone` (`~to-temporal-time-zone-identifier~`): obrigatório em `Full`, opcional em `RelativeToDuration` e
    // nem lido em `Partial` (`with()` não tem `timeZone`).
    if mode != ZonedDateTimeFieldMode::Partial {
        let time_zone = get_property(global_object, bag, "timeZone")?;
        if mode == ZonedDateTimeFieldMode::Full && time_zone.is_undefined() {
            return Err(Thrown::type_error("Temporal.ZonedDateTime.from: timeZone property is required"));
        }
        if !time_zone.is_undefined() {
            result.time_zone = Some(to_temporal_time_zone_identifier(global_object, time_zone)?);
        }
    }

    // `year` (`~to-integer-with-truncation~`).
    let year = get_property(global_object, bag, "year")?;
    if !year.is_undefined() {
        let year = to_integer_with_truncation(global_object, year)?;
        if !year.is_finite() {
            return Err(Thrown::range_error("year property must be finite"));
        }
        // `clampTo<int32_t>`.
        result.date_fields.year = Some(year as i32);
        result.any_field_set = true;
    }

    // Passo 10: `~partial~` sem nenhum campo é `TypeError`.
    if mode == ZonedDateTimeFieldMode::Partial && !result.any_field_set {
        return Err(Thrown::type_error("at least one Temporal field must be provided"));
    }

    // Passo 11.
    Ok(result)
}

/// `struct ZDTEpochArgs`: tudo que os passos 6 a 12 de `ToTemporalZonedDateTime` precisam, vindo da cadeia ou do
/// objeto de propriedades.
struct ZonedEpochArgs {
    plain_date: PlainDate,
    plain_time: PlainTime,
    time_zone: TimeZone,
    calendar_id: CalendarID,
    offset_behaviour: OffsetBehaviour,
    inline_offset_ns: i64,
    match_behaviour: MatchBehaviour,
    use_start_of_day: UseStartOfDay,
    disambiguation: TemporalDisambiguation,
    offset_opt: TemporalOffsetDisambiguation,
}

/// Os passos de `GetOptionsObject` e das três opções de `ToTemporalZonedDateTime` (`disambiguation`, `offset` com
/// `~reject~` e `overflow`, nesta ordem alfabética): `options` ausente dá os padrões.
fn read_from_options(
    global_object: &JSGlobalObject,
    options_value: JSValue,
) -> Result<(TemporalDisambiguation, TemporalOffsetDisambiguation, TemporalOverflow), Thrown> {
    if options_value.is_undefined() {
        return Ok((TemporalDisambiguation::Compatible, TemporalOffsetDisambiguation::Reject, TemporalOverflow::Constrain));
    }
    if !options_value.is_object() {
        return Err(Thrown::type_error("Temporal.ZonedDateTime.from: options must be an object"));
    }
    let options = Some(options_value);
    let disambiguation = to_temporal_disambiguation(global_object, options)?;
    let offset_opt = to_temporal_offset(global_object, options, TemporalOffsetDisambiguation::Reject)?;
    let overflow = to_temporal_overflow(global_object, options)?;
    Ok((disambiguation, offset_opt, overflow))
}

/// `toEpochArgsFromString(globalObject, item, optionsArg)`: os passos 5.b a 5.r e a resolução das opções.
fn epoch_args_from_string(global_object: &JSGlobalObject, item: JSValue, options_arg: JSValue) -> Result<ZonedEpochArgs, Thrown> {
    let string = string_units(global_object, item)?;

    // Passo 5.b: `ParseISODateTime(item, « TemporalDateTimeString[+Zoned] »)`; o `[+Zoned]` torna a anotação
    // de fuso obrigatória na gramática.
    let Some(parsed) = parse_iso_date_time(&string, OptionSet::new(&[TemporalProduction::DateTimeZoned])) else {
        return Err(throw_range_error_with_units(
            global_object,
            "'",
            &ellipsize_at(100, &string),
            "' is not a valid Temporal.ZonedDateTime string",
        ));
    };
    let plain_date = parsed.date.expect("TemporalDateTimeString[+Zoned] sempre tem data");
    let time_zone_record = parsed.time_zone.expect("TemporalDateTimeString[+Zoned] sempre tem anotação de fuso");

    // Passos 5.c a 5.e: `timeZone = ? ToTemporalTimeZoneIdentifier(annotation)`.
    let Some(time_zone) = time_zone_from_record(&time_zone_record) else {
        return Err(throw_range_error_with_units(
            global_object,
            "'",
            &ellipsize_at(100, &string),
            "' contains an invalid time zone identifier",
        ));
    };

    // Passos 5.f e 5.g: `offsetString` e `hasUTCDesignator`; passos 5.k e 5.l: `match-minutes`, ou `match-exactly`
    // com precisão de subminuto.
    let has_utc_designator = time_zone_record.z;
    let inline_offset_ns = time_zone_record.offset.unwrap_or(0);
    let match_behaviour =
        if time_zone_record.offset_has_sub_minute_precision { MatchBehaviour::MatchExactly } else { MatchBehaviour::MatchMinutes };

    // Passos 5.h a 5.j: `calendar = result.[[Calendar]]`; `~empty~` é `"iso8601"`; `CanonicalizeCalendar`.
    let mut calendar_id = ISO8601_CALENDAR_ID;
    if let Some(calendar) = &parsed.calendar {
        let raw_calendar: Vec<u16> = calendar.to_ascii_lowercase().into_iter().map(u16::from).collect();
        match is_builtin_calendar(&raw_calendar) {
            Some(canonicalized) => calendar_id = canonicalized,
            None => {
                return Err(throw_range_error_with_units(global_object, "'", &raw_calendar, "' is not a valid calendar identifier"));
            }
        }
    }

    // Passos 5.m a 5.p: `GetOptionsObject` e as três opções.
    let (disambiguation, offset_opt, _overflow) = read_from_options(global_object, options_arg)?;

    // Passos 5.q e 5.r: a hora, `~start-of-day~` quando a cadeia não tem.
    let plain_time = parsed.time.unwrap_or_default();

    // Passos 6 a 8: `offsetBehaviour`.
    let offset_behaviour = if has_utc_designator {
        OffsetBehaviour::Exact
    } else if time_zone_record.offset.is_none() {
        OffsetBehaviour::Wall
    } else {
        OffsetBehaviour::Option
    };
    let use_start_of_day = if parsed.time.is_none() && offset_behaviour == OffsetBehaviour::Wall { UseStartOfDay::Yes } else { UseStartOfDay::No };

    Ok(ZonedEpochArgs {
        plain_date,
        plain_time,
        time_zone,
        calendar_id,
        offset_behaviour,
        inline_offset_ns,
        match_behaviour,
        use_start_of_day,
        disambiguation,
        offset_opt,
    })
}

/// `toEpochArgsFromPropertyBag(globalObject, bag, optionsArg)`: os passos 4.b a 4.l.
fn epoch_args_from_property_bag(global_object: &JSGlobalObject, bag: JSValue, options_arg: JSValue) -> Result<ZonedEpochArgs, Thrown> {
    // Passo 4.b: `calendar = ? GetTemporalCalendarIdentifierWithISODefault(item)`.
    let calendar_id = get_temporal_calendar_identifier_with_iso_default(global_object, bag)?;

    // Passo 4.c: `PrepareCalendarFields`, os 15 campos numa passagem.
    let fields = read_zoned_date_time_fields_from_object(global_object, bag, calendar_id, ZonedDateTimeFieldMode::Full)?;

    // Passos 4.d e 4.e: `timeZone` e `offsetString`.
    let time_zone = fields.time_zone.clone().expect("o modo Full exige timeZone");

    // Passos 4.f a 4.i: as opções, depois dos campos.
    let (disambiguation, offset_opt, overflow) = read_from_options(global_object, options_arg)?;

    // Passos 4.j e 4.k: `dateTimeResult = ? InterpretTemporalDateTimeFields(calendar, fields, overflow)`.
    let PlainDateTime { date: plain_date, time: plain_time } =
        interpret_temporal_date_time_fields(calendar_id, &fields.date_fields, &fields.time_fields, overflow)?;

    // Passos 6 a 8: sem deslocamento é `wall`; com ele, `option` (o `offset` das opções decide `prefer`, `reject`,
    // `use` e `ignore`). A propriedade sempre casa `match-exactly` (o passo 4.j).
    let offset_behaviour = if fields.offset_ns.is_some() { OffsetBehaviour::Option } else { OffsetBehaviour::Wall };

    Ok(ZonedEpochArgs {
        plain_date,
        plain_time,
        time_zone,
        calendar_id,
        offset_behaviour,
        inline_offset_ns: fields.offset_ns.unwrap_or(0),
        match_behaviour: MatchBehaviour::MatchExactly,
        use_start_of_day: UseStartOfDay::No,
        disambiguation,
        offset_opt,
    })
}

impl TemporalZonedDateTime {
    /// `from(globalObject, itemValue, optionsArg)` (`ToTemporalZonedDateTime`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-totemporalzoneddatetime
    /// Sempre uma célula nova, mesmo quando `item` já é um `ZonedDateTime`.
    pub fn from(global_object: &JSGlobalObject, item_value: JSValue, options_arg: JSValue) -> Result<TemporalZonedDateTimeRef, Thrown> {
        // Passo 5: `String` (o passo 4 antes dele não pode ser as duas coisas, a ordem não é observável).
        let args = if item_value.is_string() {
            epoch_args_from_string(global_object, item_value, options_arg)?
        } else if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&item_value) {
            // Passo 4.a: `[[InitializedTemporalZonedDateTime]]`: `GetOptionsObject` e as três opções (validam, o
            // resultado não serve) e `CreateTemporalZonedDateTime` com os mesmos campos.
            read_from_options(global_object, options_arg)?;
            return Ok(TemporalZonedDateTime::create(
                global_object.vm(),
                &global_object.zoned_date_time_structure(),
                zoned_date_time.exact_time(),
                zoned_date_time.time_zone().clone(),
                zoned_date_time.calendar_id(),
            ));
        } else if item_value.is_object() {
            // Passos 4.b a 4.l: o objeto de propriedades.
            epoch_args_from_property_bag(global_object, item_value, options_arg)?
        } else {
            return Err(Thrown::type_error("Temporal.ZonedDateTime.from: argument must be a ZonedDateTime, string, or object"));
        };

        // Passo 11: `epochNanoseconds = ? InterpretISODateTimeOffset(...)`.
        let exact_time = interpret_iso_date_time_offset(
            args.plain_date,
            args.plain_time,
            args.use_start_of_day,
            args.offset_behaviour,
            args.offset_opt,
            args.inline_offset_ns,
            args.match_behaviour,
            &args.time_zone,
            args.disambiguation,
        )?;

        // Passo 12: `CreateTemporalZonedDateTime(epochNanoseconds, timeZone, calendar)`.
        create_temporal_zoned_date_time(global_object, exact_time, args.time_zone, args.calendar_id, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::temporal_object::{Precision, TemporalUnit};

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn zone(name: &str) -> TimeZone {
        TimeZone::Id(intl_resolve_time_zone_id(name.as_bytes()).expect("fuso conhecido"))
    }

    #[test]
    fn parses_time_zone_strings() {
        let identifier = parse_temporal_time_zone_string(&units("America/Sao_Paulo")).unwrap();
        assert_eq!(identifier.name, b"America/Sao_Paulo".to_vec());
        assert_eq!(identifier.offset_minutes, None);
        let offset = parse_temporal_time_zone_string(&units("-03:00")).unwrap();
        assert_eq!((offset.name.is_empty(), offset.offset_minutes), (true, Some(-180)));
        // Cadeia de data e hora: o fuso sai da anotação, do `Z` ou do deslocamento.
        let annotated = parse_temporal_time_zone_string(&units("2020-01-01T00:00:00+01:00[Europe/Paris]")).unwrap();
        assert_eq!(annotated.name, b"Europe/Paris".to_vec());
        let utc = parse_temporal_time_zone_string(&units("2020-01-01T00:00:00Z")).unwrap();
        assert_eq!(utc.name, b"UTC".to_vec());
        let numeric = parse_temporal_time_zone_string(&units("2020-01-01T00:00:00+05:30")).unwrap();
        assert_eq!(numeric.offset_minutes, Some(330));
        // Deslocamento com subminuto não vale como fuso.
        assert!(parse_temporal_time_zone_string(&units("2020-01-01T00:00:00+05:30:15")).is_none());
        assert!(parse_temporal_time_zone_string(&units("not a zone")).is_none());
    }

    #[test]
    fn resolves_time_zone_records() {
        let sao_paulo = parse_temporal_time_zone_string(&units("america/sao_paulo")).unwrap();
        assert_eq!(time_zone_from_identifier_parse_record(&sao_paulo), Some(zone("America/Sao_Paulo")));
        let unknown = parse_temporal_time_zone_string(&units("Mars/Olympus")).unwrap();
        assert_eq!(time_zone_from_identifier_parse_record(&unknown), None);
        let offset = parse_temporal_time_zone_string(&units("+01:30")).unwrap();
        assert_eq!(time_zone_from_identifier_parse_record(&offset), Some(TimeZone::UtcOffset(90 * 60_000_000_000)));
        let parsed = parse_iso_date_time(&units("2020-06-01T10:00[Asia/Calcutta]"), OptionSet::new(&[TemporalProduction::DateTimeZoned])).unwrap();
        assert_eq!(time_zone_from_record(&parsed.time_zone.unwrap()), Some(zone("Asia/Calcutta")));
    }

    #[test]
    fn formats_zoned_date_times() {
        let new_york = zone("America/New_York");
        // 2024-07-01T16:30:15.5Z em Nova York: 12:30:15.5-04:00.
        let epoch = crate::runtime::temporal_core_duration::get_utc_epoch_nanoseconds(PlainDate::new(2024, 7, 1), PlainTime::new(16, 30, 15, 500, 0, 0));
        let exact = ExactTime::new(epoch);
        let auto = to_seconds_string_precision_record(None, None);
        let text = |precision: PrecisionData, offset, tz_name, calendar| {
            zoned_date_time_to_string(exact, &new_york, ISO8601_CALENDAR_ID, precision, RoundingMode::Trunc, offset, tz_name, calendar).unwrap()
        };
        assert_eq!(
            text(auto, ShowOffsetOption::Auto, ShowTimeZoneNameOption::Auto, CalendarNameOption::Auto),
            "2024-07-01T12:30:15.5-04:00[America/New_York]"
        );
        assert_eq!(
            text(auto, ShowOffsetOption::Never, ShowTimeZoneNameOption::Critical, CalendarNameOption::Always),
            "2024-07-01T12:30:15.5[!America/New_York][u-ca=iso8601]"
        );
        assert_eq!(
            text(auto, ShowOffsetOption::Auto, ShowTimeZoneNameOption::Never, CalendarNameOption::Critical),
            "2024-07-01T12:30:15.5-04:00[!u-ca=iso8601]"
        );
        let seconds = to_seconds_string_precision_record(Some(TemporalUnit::Second), None);
        assert_eq!(seconds.precision.0, Precision::Fixed);
        assert_eq!(
            text(seconds, ShowOffsetOption::Auto, ShowTimeZoneNameOption::Auto, CalendarNameOption::Never),
            "2024-07-01T12:30:15-04:00[America/New_York]"
        );
        let minute = to_seconds_string_precision_record(Some(TemporalUnit::Minute), None);
        assert_eq!(
            text(minute, ShowOffsetOption::Auto, ShowTimeZoneNameOption::Auto, CalendarNameOption::Never),
            "2024-07-01T12:30-04:00[America/New_York]"
        );
    }

    #[test]
    fn formats_fixed_offsets_and_rounds_them_to_the_minute() {
        let exact = ExactTime::new(0);
        let auto = to_seconds_string_precision_record(None, None);
        let to_string = |offset_ns: i64| {
            zoned_date_time_to_string(
                exact,
                &TimeZone::UtcOffset(offset_ns),
                ISO8601_CALENDAR_ID,
                auto,
                RoundingMode::Trunc,
                ShowOffsetOption::Auto,
                ShowTimeZoneNameOption::Auto,
                CalendarNameOption::Auto,
            )
            .unwrap()
        };
        assert_eq!(to_string(-3 * 3_600_000_000_000), "1969-12-31T21:00:00-03:00[-03:00]");
        // +00:00:30 arredonda para cima (meio, longe de zero); -00:00:30 para baixo.
        assert_eq!(to_string(30_000_000_000), "1970-01-01T00:00:30+00:01[+00:00:30]");
        assert_eq!(to_string(-30_000_000_000), "1969-12-31T23:59:30-00:01[-00:00:30]");
        assert_eq!(to_string(29_000_000_000), "1970-01-01T00:00:29+00:00[+00:00:29]");
    }

    #[test]
    fn rounds_before_formatting() {
        let new_york = zone("America/New_York");
        // 2024-11-03T05:59:59.9Z é 01:59:59.9 EDT; arredondado ao segundo para cima vira 06:00:00Z, 01:00:00 EST.
        let epoch = crate::runtime::temporal_core_duration::get_utc_epoch_nanoseconds(PlainDate::new(2024, 11, 3), PlainTime::new(5, 59, 59, 900, 0, 0));
        let seconds = to_seconds_string_precision_record(Some(TemporalUnit::Second), None);
        let text = zoned_date_time_to_string(
            ExactTime::new(epoch),
            &new_york,
            ISO8601_CALENDAR_ID,
            seconds,
            RoundingMode::HalfExpand,
            ShowOffsetOption::Auto,
            ShowTimeZoneNameOption::Auto,
            CalendarNameOption::Auto,
        )
        .unwrap();
        assert_eq!(text, "2024-11-03T01:00:00-05:00[America/New_York]");
    }
}
