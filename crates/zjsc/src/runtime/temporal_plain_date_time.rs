//! Porte de `runtime/TemporalPlainDateTime.{h,cpp}`: a célula `Temporal.PlainDateTime` (`TemporalPlainDateTime`,
//! um `JSNonFinalObject` com a data em `m_plainDate`, a hora em `m_plainTime` e o `CalendarID` em
//! `m_calendarID`), `createTemporalDateTime`, `from` (`ToTemporalDateTime`), `toString` e
//! `differenceTemporalPlainDateTime`.
//!
//! DIVERGÊNCIAS:
//! - Qualquer calendário embutido: a célula carrega o `CalendarID` e as operações despacham por ele. Os dois
//!   `create` do C++ (com e sem `CalendarID`) são um só.
//! - `ToTemporalDateTime` trata `ZonedDateTime` (passo 2.b) e `PlainDate` (passo 2.c).
//! - `fromImpl` do C++ só tem o objeto de propriedades e a cadeia; aqui são `from_property_bag` e `from_string`.
//! - `toString()` sem opções (a versão do cabeçalho, que o `toJSON` usa) não leva a anotação `[u-ca=...]`, como no
//!   C++; só `toString(globalObject, options)` a acrescenta quando o calendário não é ISO.
//! - `toString` devolve `String` do Rust (o resultado é sempre ASCII).
//! - `until` e `since` do C++ são o template `differenceTemporalPlainDateTime<op>`; aqui é um método só, com a
//!   operação como argumento.

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::intl_support::get_options_object;
use crate::runtime::iso8601::{
    parse_iso_date_time, temporal_date_time_to_string, Duration, PlainDate, PlainTime, TemporalProduction,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{
    calendar_id_to_string, calendar_is_iso, get_temporal_calendar_identifier_with_iso_default, interpret_temporal_date_time_fields,
    is_builtin_calendar, read_calendar_fields_from_object, temporal_show_calendar_name, CalendarID, CalendarNameOption,
    FieldSetType, ISO8601_CALENDAR_ID,
};
use crate::runtime::temporal_core_calendar_fields::TimeFieldsIn;
use crate::runtime::temporal_core_iso_date::round_iso_date_time;
use crate::runtime::temporal_core_plain_date_time as core_plain_date_time;
use crate::runtime::temporal_core_plain_date_time::iso_date_time_within_limits;
use crate::runtime::temporal_object::{
    extract_difference_options, length_in_nanoseconds, string_units, temporal_fractional_second_digits, temporal_rounding_mode,
    temporal_unit_valued, throw_range_error_with_units, to_seconds_string_precision_record, to_temporal_overflow_value,
    validate_temporal_unit_value, AllowedUnit, DifferenceOperation, Precision, RoundingMode, TemporalUnit, TemporalUnitDefault, UnitGroup,
    UnitOption,
};
use crate::runtime::temporal_plain_date::TemporalPlainDate;
use crate::runtime::temporal_zoned_date_time::TemporalZonedDateTime;
use crate::runtime::vm::VM;
use crate::wtf::option_set::OptionSet;

/// `const ClassInfo TemporalPlainDateTime::s_info` (`"Object"`).
pub static TEMPORAL_PLAIN_DATE_TIME_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class TemporalPlainDateTime final : public JSNonFinalObject`.
pub struct TemporalPlainDateTime {
    base: JSNonFinalObject,
    /// `m_plainDate`.
    plain_date: PlainDate,
    /// `m_plainTime`.
    plain_time: PlainTime,
    /// `m_calendarID`.
    calendar_id: CalendarID,
}

/// A referência à célula, o `*` do C++.
pub type TemporalPlainDateTimeRef = Rc<TemporalPlainDateTime>;

impl std::ops::Deref for TemporalPlainDateTime {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSGlobalObject {
    /// `plainDateTimeStructure()`: o `LazyClassStructure` de `Temporal.PlainDateTime`.
    pub fn plain_date_time_structure(&self) -> StructureRef {
        crate::runtime::temporal_object::lazy_temporal_structure(self, "PlainDateTime", |data| data.plain_date_time_structure.clone())
    }
}

/// O texto de `base` com a anotação de calendário que `showCalendar` pede: `never` sem anotação, `always` com
/// `[u-ca=<id>]`, `critical` com `[!u-ca=<id>]` e `auto` só se o calendário não é ISO.
fn with_calendar_annotation(base: String, calendar_id: CalendarID, show_calendar: CalendarNameOption) -> String {
    let identifier = calendar_id_to_string(calendar_id);
    match show_calendar {
        CalendarNameOption::Never => base,
        CalendarNameOption::Always => format!("{base}[u-ca={identifier}]"),
        CalendarNameOption::Critical => format!("{base}[!u-ca={identifier}]"),
        CalendarNameOption::Auto if !calendar_is_iso(calendar_id) => format!("{base}[u-ca={identifier}]"),
        CalendarNameOption::Auto => base,
    }
}

impl TemporalPlainDateTime {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainDateTime::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_DATE_TIME_S_INFO,
        )
    }

    /// `create(vm, structure, plainDate, plainTime, calendarID)` (e o `finishCreation`): a célula e o registro dela.
    pub fn create(
        vm: &VM,
        structure: &StructureRef,
        plain_date: PlainDate,
        plain_time: PlainTime,
        calendar_id: CalendarID,
    ) -> TemporalPlainDateTimeRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(TemporalPlainDateTime {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            plain_date,
            plain_time,
            calendar_id,
        });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TemporalPlainDateTime(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<TemporalPlainDateTime>(value)`.
    pub fn from_value(value: &JSValue) -> Option<TemporalPlainDateTimeRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::TemporalPlainDateTime(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `plainDate()`.
    pub fn plain_date(&self) -> PlainDate {
        self.plain_date
    }

    /// `plainTime()`.
    pub fn plain_time(&self) -> PlainTime {
        self.plain_time
    }

    /// `calendarID()`.
    pub fn calendar_id(&self) -> CalendarID {
        self.calendar_id
    }

    /// `toString()`: `temporalDateTimeToString(date, time, ~auto~)`, sem a anotação de calendário (ver o cabeçalho).
    pub fn to_string(&self) -> String {
        temporal_date_time_to_string(self.plain_date, self.plain_time, (Precision::Auto, 0))
    }

    /// `toString(globalObject, options)`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.tostring
    /// A marca de `this` é do chamador.
    pub fn to_string_with_options(&self, global_object: &JSGlobalObject, options_value: JSValue) -> Result<String, Thrown> {
        // Passo 3: `resolvedOptions = ? GetOptionsObject(options)`.
        let Some(options) = get_options_object(options_value)? else {
            // Caminho curto: sem opções é `Precision::Auto`, o arredondamento é a identidade.
            return Ok(with_calendar_annotation(self.to_string(), self.calendar_id, CalendarNameOption::Auto));
        };

        // Passos 4 e 5: as opções em ordem alfabética; `GetTemporalShowCalendarNameOption`.
        let show_calendar = temporal_show_calendar_name(global_object, Some(options))?;
        // Passo 6: `GetTemporalFractionalSecondDigitsOption`.
        let digits = temporal_fractional_second_digits(global_object, Some(options))?;
        // Passo 7: `GetRoundingModeOption(resolvedOptions, ~trunc~)`.
        let rounding_mode = temporal_rounding_mode(global_object, Some(options), RoundingMode::Trunc)?;
        // Passo 8: `GetTemporalUnitValuedOption(resolvedOptions, "smallestUnit", ~unset~)`.
        let smallest_unit_result = temporal_unit_valued(global_object, Some(options), "smallestUnit", TemporalUnitDefault::Unset)?;

        // Passo 9: `ValidateTemporalUnitValue(smallestUnit, ~time~)`.
        validate_temporal_unit_value(smallest_unit_result, UnitGroup::Time, AllowedUnit::None, "smallestUnit")?;
        let smallest_unit = match smallest_unit_result {
            UnitOption::Unit(unit) => Some(unit),
            _ => None,
        };
        // Passo 10: `hour` é `RangeError`.
        if smallest_unit == Some(TemporalUnit::Hour) {
            return Err(Thrown::range_error("smallestUnit cannot be \"hour\" for PlainDateTime.toString"));
        }

        // Passo 11: `ToSecondsStringPrecisionRecord(smallestUnit, digits)`.
        let data = to_seconds_string_precision_record(smallest_unit, digits);

        // Passos 12 a 14 (caminho curto): `auto` é incremento de 1 ns, `RoundISODateTime` é a identidade.
        if data.precision.0 == Precision::Auto {
            return Ok(with_calendar_annotation(self.to_string(), self.calendar_id, show_calendar));
        }

        // Passo 12: `RoundISODateTime(isoDateTime, precision.[[Increment]], precision.[[Unit]], roundingMode)`.
        let increment_ns = length_in_nanoseconds(data.unit) * i128::from(data.increment);
        let (rounded_date, rounded_time) = round_iso_date_time(self.plain_date, self.plain_time, increment_ns, data.unit, rounding_mode);

        // Passo 13: fora de `ISODateTimeWithinLimits` é `RangeError`.
        if !iso_date_time_within_limits(rounded_date, rounded_time) {
            return Err(Thrown::range_error("Rounding result is outside the representable range"));
        }

        // Passo 14: `ISODateTimeToString(result, calendar, precision.[[Precision]], showCalendar)`.
        Ok(with_calendar_annotation(temporal_date_time_to_string(rounded_date, rounded_time, data.precision), self.calendar_id, show_calendar))
    }

    /// `from(globalObject, itemValue, optionsValue)` (`ToTemporalDateTime`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-totemporaldatetime
    /// Sempre uma célula nova, mesmo quando `item` já é um `PlainDateTime`.
    pub fn from(global_object: &JSGlobalObject, item_value: JSValue, options_value: JSValue) -> Result<TemporalPlainDateTimeRef, Thrown> {
        let vm = global_object.vm();

        // Passo 2: se `item` é um objeto.
        if item_value.is_object() {
            // Passo 2.a: `[[InitializedTemporalDateTime]]`: `GetOptionsObject` e `GetTemporalOverflowOption` (validam,
            // o resultado não serve) e uma instância nova com os mesmos campos.
            if let Some(existing) = TemporalPlainDateTime::from_value(&item_value) {
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainDateTime::create(
                    vm,
                    &global_object.plain_date_time_structure(),
                    existing.plain_date,
                    existing.plain_time,
                    existing.calendar_id,
                ));
            }

            // Passo 2.b: `[[InitializedTemporalZonedDateTime]]`: `GetISODateTimeFor(timeZone, epochNanoseconds)`,
            // `GetOptionsObject` e `GetTemporalOverflowOption` (validam) e `CreateTemporalDateTime(isoDateTime, calendar)`.
            if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&item_value) {
                let iso_date_time = zoned_date_time.get_local_date_time()?;
                to_temporal_overflow_value(global_object, options_value)?;
                return create_temporal_date_time(global_object, iso_date_time.date, iso_date_time.time, zoned_date_time.calendar_id(), None);
            }

            // Passo 2.c: `[[InitializedTemporalDate]]`: as opções são validadas e a hora é a meia-noite.
            if let Some(plain_date) = TemporalPlainDate::from_value(&item_value) {
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainDateTime::create(
                    vm,
                    &global_object.plain_date_time_structure(),
                    plain_date.plain_date(),
                    PlainTime::default(),
                    plain_date.calendar_id(),
                ));
            }

            // Passos 2.d a 2.h: o objeto de propriedades, todos os campos lidos antes das opções.
            return TemporalPlainDateTime::from_property_bag(global_object, item_value, options_value);
        }

        // Passo 3: quem não é objeto nem `String` é `TypeError`.
        if !item_value.is_string() {
            return Err(Thrown::type_error("can only convert to PlainDateTime from object or string values"));
        }
        TemporalPlainDateTime::from_string(global_object, item_value, options_value)
    }

    /// Os passos 2.d a 2.h de `ToTemporalDateTime`: o objeto de propriedades.
    fn from_property_bag(global_object: &JSGlobalObject, item_value: JSValue, options_value: JSValue) -> Result<TemporalPlainDateTimeRef, Thrown> {
        // Passo 2.d: `calendar = ? GetTemporalCalendarIdentifierWithISODefault(item)`.
        let calendar_id = get_temporal_calendar_identifier_with_iso_default(global_object, item_value)?;

        // Passo 2.e: `fields = ? PrepareCalendarFields(...)`, em ordem alfabética.
        let mut time_fields = TimeFieldsIn::default();
        let date_fields = read_calendar_fields_from_object(global_object, item_value, calendar_id, FieldSetType::DateTime, Some(&mut time_fields))?;

        // Passos 2.f e 2.g: `GetOptionsObject` e `GetTemporalOverflowOption`, depois de todos os campos.
        let overflow = to_temporal_overflow_value(global_object, options_value)?;

        // Passo 2.h: `InterpretTemporalDateTimeFields(calendar, fields, overflow)` e `CreateTemporalDateTime`.
        let date_time = interpret_temporal_date_time_fields(calendar_id, &date_fields, &time_fields, overflow)?;
        create_temporal_date_time(global_object, date_time.date, date_time.time, calendar_id, None)
    }

    /// Os passos 4 a 13 de `ToTemporalDateTime`: a cadeia.
    fn from_string(global_object: &JSGlobalObject, item_value: JSValue, options_value: JSValue) -> Result<TemporalPlainDateTimeRef, Thrown> {
        let string = string_units(global_object, item_value)?;

        // Passo 4: `ParseISODateTime(item, « TemporalDateTimeString[~Zoned] »)`.
        let Some(parsed) = parse_iso_date_time(&string, OptionSet::new(&[TemporalProduction::DateTimeUnzoned])) else {
            return Err(Thrown::range_error("invalid date string"));
        };
        let plain_date = parsed.date.expect("TemporalDateTimeString[~Zoned] sempre tem data");

        // Passos 5 a 7: `[[Calendar]]` do texto, `~empty~` vira `"iso8601"`, e `CanonicalizeCalendar`.
        let mut calendar_id = ISO8601_CALENDAR_ID;
        if let Some(calendar) = parsed.calendar {
            let raw_calendar: Vec<u16> = calendar.to_ascii_lowercase().into_iter().map(u16::from).collect();
            let Some(canonicalized) = is_builtin_calendar(&raw_calendar) else {
                return Err(throw_range_error_with_units(global_object, "'", &raw_calendar, "' is not a valid calendar identifier"));
            };
            calendar_id = canonicalized;
        }

        // Passo 8: `GetOptionsObject` e `GetTemporalOverflowOption` (depois do parse; o resultado não serve).
        to_temporal_overflow_value(global_object, options_value)?;

        // Passos 9 a 13: sem hora no texto é a meia-noite; `CreateTemporalDateTime`.
        create_temporal_date_time(global_object, plain_date, parsed.time.unwrap_or_default(), calendar_id, None)
    }

    /// `differenceTemporalPlainDateTime<op>(globalObject, other, optionsValue)` (`DifferenceTemporalPlainDateTime`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-differencetemporalplaindatetime
    /// O passo 1 (`ToTemporalDateTime(other)`) é de quem chama.
    pub fn difference_temporal_plain_date_time(
        &self,
        global_object: &JSGlobalObject,
        operation: DifferenceOperation,
        other: &TemporalPlainDateTime,
        options_value: JSValue,
    ) -> Result<Duration, Thrown> {
        // Passo 2: `CalendarEquals` falso é `RangeError`.
        if self.calendar_id != other.calendar_id {
            return Err(Thrown::range_error("cannot compute difference between date-times with different calendars"));
        }

        // Passos 3 e 4: `GetOptionsObject` e `GetDifferenceSettings(operation, options, ~datetime~, «», ~nanosecond~, ~day~)`.
        let (smallest_unit, largest_unit, rounding_mode, increment) =
            extract_difference_options(global_object, options_value, UnitGroup::DateTime, TemporalUnit::Nanosecond, TemporalUnit::Day, operation)?;

        // Passos 5 a 9: o núcleo (zero, `DifferencePlainDateTimeWithRounding`, `TemporalDurationFromInternal`, negação).
        Ok(core_plain_date_time::difference_temporal_plain_date_time(
            self.calendar_id,
            operation,
            self.plain_date,
            self.plain_time,
            other.plain_date,
            other.plain_time,
            smallest_unit,
            largest_unit,
            rounding_mode,
            increment,
        )?)
    }
}

/// `createTemporalDateTime(globalObject, plainDate, plainTime, calendarID[, newTarget])`:
/// https://tc39.es/proposal-temporal/#sec-temporal-createtemporaldatetime
/// `new_target` é `Some((newTarget, jsCallee))` na construção e `None` na criação intrínseca.
pub fn create_temporal_date_time(
    global_object: &JSGlobalObject,
    plain_date: PlainDate,
    plain_time: PlainTime,
    calendar_id: CalendarID,
    new_target: Option<(JSValue, usize)>,
) -> Result<TemporalPlainDateTimeRef, Thrown> {
    // Passo 1: `ISODateTimeWithinLimits(isoDateTime)` falso é `RangeError`.
    if !iso_date_time_within_limits(plain_date, plain_time) {
        return Err(Thrown::range_error("date time is out of range of ECMAScript representation"));
    }
    // Passos 2 e 3: `newTarget` ausente é `%Temporal.PlainDateTime%`; `OrdinaryCreateFromConstructor`.
    let structure = match new_target {
        None => global_object.plain_date_time_structure(),
        Some((new_target, constructor)) => {
            get_derived_structure_in_realm(global_object, new_target, constructor, |realm| realm.plain_date_time_structure())?
        }
    };

    // Passos 4 a 6: `[[ISODateTime]]` e `[[Calendar]]`.
    Ok(TemporalPlainDateTime::create(global_object.vm(), &structure, plain_date, plain_time, calendar_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_annotation_follows_show_calendar() {
        let iso = ISO8601_CALENDAR_ID;
        let gregory = crate::runtime::temporal_calendar::AVAILABLE_CALENDARS.iter().position(|name| *name == "gregory").unwrap() as CalendarID;
        let base = || "2020-01-01T00:00:00".to_string();
        assert_eq!(with_calendar_annotation(base(), iso, CalendarNameOption::Auto), "2020-01-01T00:00:00");
        assert_eq!(with_calendar_annotation(base(), iso, CalendarNameOption::Never), "2020-01-01T00:00:00");
        assert_eq!(with_calendar_annotation(base(), iso, CalendarNameOption::Always), "2020-01-01T00:00:00[u-ca=iso8601]");
        assert_eq!(with_calendar_annotation(base(), iso, CalendarNameOption::Critical), "2020-01-01T00:00:00[!u-ca=iso8601]");
        assert_eq!(with_calendar_annotation(base(), gregory, CalendarNameOption::Auto), "2020-01-01T00:00:00[u-ca=gregory]");
        assert_eq!(with_calendar_annotation(base(), gregory, CalendarNameOption::Never), "2020-01-01T00:00:00");
    }

    #[test]
    fn date_time_text_joins_date_and_time() {
        let text = temporal_date_time_to_string(PlainDate::new(2020, 2, 29), PlainTime::new(1, 2, 3, 4, 5, 6), (Precision::Auto, 0));
        assert_eq!(text, "2020-02-29T01:02:03.004005006");
        let text = temporal_date_time_to_string(PlainDate::new(2020, 2, 29), PlainTime::default(), (Precision::Minute, 0));
        assert_eq!(text, "2020-02-29T00:00");
        let text = temporal_date_time_to_string(PlainDate::new(-1, 1, 1), PlainTime::default(), (Precision::Fixed, 3));
        assert_eq!(text, "-000001-01-01T00:00:00.000");
    }

    #[test]
    fn rounded_text_uses_the_rounded_date_time() {
        // `toString({ smallestUnit: "minute" })` de 23:59:40 com `halfExpand` rola para o dia seguinte.
        let (date, time) = round_iso_date_time(
            PlainDate::new(2020, 12, 31),
            PlainTime::new(23, 59, 40, 0, 0, 0),
            length_in_nanoseconds(TemporalUnit::Minute),
            TemporalUnit::Minute,
            RoundingMode::HalfExpand,
        );
        assert_eq!(temporal_date_time_to_string(date, time, (Precision::Minute, 0)), "2021-01-01T00:00");
    }

    #[test]
    fn limits_are_checked_on_the_pair() {
        assert!(iso_date_time_within_limits(PlainDate::new(2020, 1, 1), PlainTime::default()));
        assert!(!iso_date_time_within_limits(PlainDate::new(275760, 9, 14), PlainTime::default()));
    }
}
