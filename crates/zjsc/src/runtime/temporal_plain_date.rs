//! Porte de `runtime/TemporalPlainDate.{h,cpp}`: a célula `Temporal.PlainDate` (`TemporalPlainDate`, um
//! `JSNonFinalObject` com o `ISO8601::PlainDate` em `m_plainDate` e o `CalendarID` em `m_calendarID`),
//! `createTemporalDate`, `validateAndCreateISODateRecord`, `from` (`ToTemporalDate`), `until` e `since`
//! (`DifferenceTemporalPlainDate`).
//!
//! DIVERGÊNCIAS:
//! - Qualquer calendário embutido: a célula carrega o `CalendarID` e todas as operações despacham por ele
//!   (`calendar_date_add`, `calendar_date_until`, `calendar_fields`). Os três `create` do C++ (sem
//!   calendário, com `String&&` e com `CalendarID`) são um só, com o `CalendarID`.
//! - `ToTemporalDate` trata `ZonedDateTime` (passo 2.b) e `PlainDateTime` (passo 2.c).
//! - `fromImpl` do C++ repete os atalhos de objeto tipado que `from` já tratou (código morto ali); aqui só o
//!   caminho de propriedades existe, em `from_property_bag`.
//! - `validateAndCreateISODateRecord(globalObject, duration)` recebia os três campos num `ISO8601::Duration` só
//!   para carregá-los; aqui são três `f64` (o `Duration` saturaria em `i64` o que o C++ guarda como `double`, com o
//!   mesmo resultado nos testes de faixa).
//! - `toString()` é `String` do Rust (o resultado é sempre ASCII).

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::iso8601::{
    days_in_month, is_date_time_within_limits, parse_iso_date_time, temporal_date_to_string, Duration, InternalDuration, PlainDate, PlainTime,
    TemporalProduction, MAX_YEAR, MIN_YEAR,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{
    calendar_id_to_string, calendar_is_iso, get_temporal_calendar_identifier_with_iso_default, is_builtin_calendar,
    read_calendar_fields_from_object, CalendarID, FieldSetType, ISO8601_CALENDAR_ID,
};
use crate::runtime::temporal_core_calendar_fields::date_from_fields;
use crate::runtime::temporal_core_duration::{get_utc_epoch_nanoseconds, round_relative_duration, temporal_duration_from_internal};
use crate::runtime::temporal_calendar_icu::calendar_date_until;
use crate::runtime::temporal_core_iso_date::iso_date_compare;
use crate::runtime::temporal_object::{
    extract_difference_options, string_units, throw_range_error_with_units, to_temporal_overflow, to_temporal_overflow_value,
    DifferenceOperation, TemporalOverflow, TemporalUnit, UnitGroup,
};
use crate::runtime::temporal_plain_date_time::TemporalPlainDateTime;
use crate::runtime::temporal_zoned_date_time::TemporalZonedDateTime;
use crate::runtime::vm::VM;
use crate::wtf::option_set::OptionSet;

/// `const ClassInfo TemporalPlainDate::s_info` (`"Object"`).
pub static TEMPORAL_PLAIN_DATE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class TemporalPlainDate final : public JSNonFinalObject`.
pub struct TemporalPlainDate {
    base: JSNonFinalObject,
    /// `m_plainDate`.
    plain_date: PlainDate,
    /// `m_calendarID`.
    calendar_id: CalendarID,
}

/// A referência à célula, o `*` do C++.
pub type TemporalPlainDateRef = Rc<TemporalPlainDate>;

impl std::ops::Deref for TemporalPlainDate {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSGlobalObject {
    /// `plainDateStructure()`: o `LazyClassStructure` de `Temporal.PlainDate`.
    pub fn plain_date_structure(&self) -> StructureRef {
        crate::runtime::temporal_object::lazy_temporal_structure(self, "PlainDate", |data| data.plain_date_structure.clone())
    }
}

/// O que `ToTemporalDate` recebe como segundo argumento: o `Variant<JSObject*, TemporalOverflow>` do C++, as
/// opções (ainda por ler) ou o `overflow` já lido.
#[derive(Clone, Copy)]
enum OptionsOrOverflow {
    /// `JSObject*`: `None` é `nullptr` (sem opções).
    Options(Option<JSValue>),
    Overflow(TemporalOverflow),
}

impl TemporalPlainDate {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainDate::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_DATE_S_INFO,
        )
    }

    /// `create(vm, structure, plainDate, calendarID)` (e o `finishCreation`): a célula e o registro dela.
    pub fn create(vm: &VM, structure: &StructureRef, plain_date: PlainDate, calendar_id: CalendarID) -> TemporalPlainDateRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(TemporalPlainDate { base: JSNonFinalObject::new(vm, Rc::clone(structure)), plain_date, calendar_id });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TemporalPlainDate(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<TemporalPlainDate>(value)`.
    pub fn from_value(value: &JSValue) -> Option<TemporalPlainDateRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::TemporalPlainDate(cell)) => Some(cell),
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

    /// `calendarID()`.
    pub fn calendar_id(&self) -> CalendarID {
        self.calendar_id
    }

    /// `toString()`: `TemporalDateToString(this, ~auto~)`, com o `[u-ca=...]` quando o calendário não é ISO.
    pub fn to_string(&self) -> String {
        let base = temporal_date_to_string(self.plain_date);
        if calendar_is_iso(self.calendar_id) {
            return base;
        }
        format!("{base}[u-ca={}]", calendar_id_to_string(self.calendar_id))
    }

    /// `from(globalObject, itemValue, optionsValue)` (`ToTemporalDate`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-totemporaldate
    /// Sempre uma célula nova, mesmo quando `item` já é um `PlainDate`.
    pub fn from(global_object: &JSGlobalObject, item_value: JSValue, options_value: JSValue) -> Result<TemporalPlainDateRef, Thrown> {
        let vm = global_object.vm();

        // Passo 2: se `item` é um objeto.
        if item_value.is_object() {
            // Passo 2.a: `[[InitializedTemporalDate]]`: `GetOptionsObject` e `GetTemporalOverflowOption` (validam,
            // o resultado não serve) e `CreateTemporalDate` com os mesmos campos, uma instância nova.
            if let Some(existing) = TemporalPlainDate::from_value(&item_value) {
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainDate::create(vm, &global_object.plain_date_structure(), existing.plain_date, existing.calendar_id));
            }

            // Passo 2.b: `[[InitializedTemporalZonedDateTime]]`: `GetISODateTimeFor(timeZone, epochNanoseconds)`,
            // `GetOptionsObject` e `GetTemporalOverflowOption` (validam) e `CreateTemporalDate(isoDateTime.[[ISODate]], calendar)`.
            if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&item_value) {
                let iso_date_time = zoned_date_time.get_local_date_time()?;
                to_temporal_overflow_value(global_object, options_value)?;
                return create_temporal_date(global_object, iso_date_time.date, zoned_date_time.calendar_id(), None);
            }

            // Passo 2.c: `[[InitializedTemporalDateTime]]`: `GetOptionsObject` e `GetTemporalOverflowOption` (validam) e
            // `CreateTemporalDate(item.[[ISODateTime]].[[ISODate]], item.[[Calendar]])`.
            if let Some(plain_date_time) = TemporalPlainDateTime::from_value(&item_value) {
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainDate::create(
                    vm,
                    &global_object.plain_date_structure(),
                    plain_date_time.plain_date(),
                    plain_date_time.calendar_id(),
                ));
            }

            // Passos 2.d a 2.i: o objeto de propriedades. `PrepareCalendarFields` lê os campos antes das opções;
            // o `TypeError` de `options` que não é objeto só vem depois das leituras (ordem observável).
            let options = if options_value.is_undefined() {
                None
            } else if !options_value.is_object() {
                TemporalPlainDate::from_property_bag(global_object, item_value, OptionsOrOverflow::Overflow(TemporalOverflow::Constrain))?;
                return Err(Thrown::type_error("options must be an object"));
            } else {
                Some(options_value)
            };
            return TemporalPlainDate::from_property_bag(global_object, item_value, OptionsOrOverflow::Options(options));
        }

        // Passo 3: quem não é objeto nem `String` é `TypeError`.
        if !item_value.is_string() {
            return Err(Thrown::type_error("can only convert to PlainDate from object or string values"));
        }

        // Passo 4: `ParseISODateTime(item, « TemporalDateTimeString[~Zoned] »)`.
        let string = string_units(global_object, item_value)?;
        let Some(parsed) = parse_iso_date_time(&string, OptionSet::new(&[TemporalProduction::DateTimeUnzoned])) else {
            // `ParseISODateTime` falhou: `RangeError`.
            return Err(Thrown::range_error("invalid date string"));
        };
        let plain_date = parsed.date.expect("TemporalDateTimeString[~Zoned] sempre tem data");

        // Passos 5 a 7: `calendar = result.[[Calendar]]`, `~empty~` vira `"iso8601"`, e `CanonicalizeCalendar`.
        let mut calendar_id = ISO8601_CALENDAR_ID;
        if let Some(calendar) = parsed.calendar {
            let raw_calendar: Vec<u16> = calendar.to_ascii_lowercase().into_iter().map(u16::from).collect();
            let Some(canonicalized) = is_builtin_calendar(&raw_calendar) else {
                return Err(throw_range_error_with_units(global_object, "'", &raw_calendar, "' is not a valid calendar identifier"));
            };
            calendar_id = canonicalized;
        }

        // Passos 8 e 9: `GetOptionsObject` e `GetTemporalOverflowOption` (o resultado não serve para `String`).
        to_temporal_overflow_value(global_object, options_value)?;

        // Passos 10 e 11: `CreateTemporalDate(isoDate, calendar)`.
        create_temporal_date(global_object, plain_date, calendar_id, None)
    }

    /// `fromImpl(globalObject, itemValue, optionsOrOverflow)` (passos 2.d a 2.i de `ToTemporalDate`): o objeto de
    /// propriedades.
    fn from_property_bag(
        global_object: &JSGlobalObject,
        item_value: JSValue,
        options_or_overflow: OptionsOrOverflow,
    ) -> Result<TemporalPlainDateRef, Thrown> {
        // Passo 2.d: `calendar = ? GetTemporalCalendarIdentifierWithISODefault(item)`.
        let calendar_id = get_temporal_calendar_identifier_with_iso_default(global_object, item_value)?;

        // Passo 2.e: `fields = ? PrepareCalendarFields(...)`, os campos antes das opções (ordem da spec).
        let fields = read_calendar_fields_from_object(global_object, item_value, calendar_id, FieldSetType::Date, None)?;

        // Passos 2.f e 2.g: `GetOptionsObject(options)` e `GetTemporalOverflowOption(resolvedOptions)`.
        let overflow = match options_or_overflow {
            OptionsOrOverflow::Overflow(overflow) => overflow,
            OptionsOrOverflow::Options(None) => TemporalOverflow::Constrain,
            OptionsOrOverflow::Options(Some(options)) => to_temporal_overflow(global_object, Some(options))?,
        };

        // Passo 2.h: `isoDate = ? CalendarDateFromFields(calendar, fields, overflow)`.
        let resolved = date_from_fields(calendar_id, &fields, overflow)?;

        // Passo 2.i: `CreateTemporalDate(isoDate, calendar)`. `dateFromFields` já conferiu os limites (passo 3).
        Ok(TemporalPlainDate::create(global_object.vm(), &global_object.plain_date_structure(), resolved.iso_date, resolved.calendar_id))
    }

    /// `until(globalObject, other, options)`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.until
    pub fn until(&self, global_object: &JSGlobalObject, other: &TemporalPlainDate, options_value: JSValue) -> Result<Duration, Thrown> {
        self.difference_temporal_plain_date(global_object, other, options_value, DifferenceOperation::Until)
    }

    /// `since(globalObject, other, options)`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.since
    pub fn since(&self, global_object: &JSGlobalObject, other: &TemporalPlainDate, options_value: JSValue) -> Result<Duration, Thrown> {
        self.difference_temporal_plain_date(global_object, other, options_value, DifferenceOperation::Since)
    }

    /// `differenceTemporalPlainDate<op>(globalObject, other, optionsValue)` (`DifferenceTemporalPlainDate`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-differencetemporalplaindate
    /// O passo 1 (`ToTemporalDate(other)`) é de quem chama. Usa o `flip-mode` e nega no fim para `since` (ao
    /// contrário de `PlainTime`, que troca os operandos).
    fn difference_temporal_plain_date(
        &self,
        global_object: &JSGlobalObject,
        other: &TemporalPlainDate,
        options_value: JSValue,
        operation: DifferenceOperation,
    ) -> Result<Duration, Thrown> {
        // Passo 2: `CalendarEquals` falso é `RangeError`.
        if self.calendar_id != other.calendar_id {
            return Err(Thrown::range_error("cannot compute difference between dates with different calendars"));
        }

        // Passos 3 e 4: `GetOptionsObject(options)` e `GetDifferenceSettings(operation, resolvedOptions, ~date~, «»,
        // ~day~, ~day~)`.
        let (smallest_unit, largest_unit, rounding_mode, increment) =
            extract_difference_options(global_object, options_value, UnitGroup::Date, TemporalUnit::Day, TemporalUnit::Day, operation)?;

        // Passo 5: `CompareISODate = 0` devolve a duração zero.
        if iso_date_compare(self.plain_date, other.plain_date) == 0 {
            return Ok(Duration::default());
        }

        // Passo 6: `dateDifference = CalendarDateUntil(calendar, this, other, largestUnit)`.
        let date_difference = calendar_date_until(self.calendar_id, self.plain_date, other.plain_date, largest_unit)?;

        // Passo 7: `duration = CombineDateAndTimeDuration(dateDifference, 0)`.
        let mut duration = InternalDuration::new(date_difference, 0);

        // Passo 8: `smallestUnit` diferente de `~day~` ou `increment` diferente de 1: `RoundRelativeDuration`
        // (os passos 8.a a 8.e montam `isoDateTime`, `originEpochNs` e `destEpochNs`).
        if smallest_unit != TemporalUnit::Day || increment != 1.0 {
            // `RoundRelativeDuration` ciente do calendário: `CalendarDateAdd` e `CalendarDateUntil` por candidato.
            let iso_date = self.plain_date;
            let origin_epoch_ns = get_utc_epoch_nanoseconds(iso_date, PlainTime::default());
            let dest_epoch_ns = get_utc_epoch_nanoseconds(other.plain_date, PlainTime::default());
            round_relative_duration(
                self.calendar_id,
                &mut duration,
                origin_epoch_ns,
                dest_epoch_ns,
                iso_date,
                PlainTime::default(),
                largest_unit,
                increment,
                smallest_unit,
                rounding_mode,
            )?;
        }

        // Passo 9: `result = ! TemporalDurationFromInternal(duration, ~day~)`.
        let mut result = temporal_duration_from_internal(&duration, TemporalUnit::Day)?;

        // Passos 10 e 11: `since` nega o resultado.
        if operation == DifferenceOperation::Since {
            result = -result;
        }
        Ok(result)
    }
}

/// `validateAndCreateISODateRecord(globalObject, duration)` (`IsValidISODate` e `CreateISODateRecord`):
/// https://tc39.es/proposal-temporal/#sec-temporal-isvalidisodate
pub fn validate_and_create_iso_date_record(year: f64, month: f64, day: f64) -> Result<PlainDate, Thrown> {
    // `isYearWithinLimits(yearDouble)`.
    if !(year >= f64::from(MIN_YEAR) && year <= f64::from(MAX_YEAR)) {
        return Err(Thrown::range_error("year is out of range"));
    }
    let year = year as i32;

    // Passo 1 de `IsValidISODate`: `month < 1` ou `month > 12`.
    if !(month >= 1.0 && month <= 12.0) {
        return Err(Thrown::range_error("month is out of range"));
    }
    let month = month as u8;

    // Passos 2 e 3: `day` fora de `[1, ISODaysInMonth(year, month)]`.
    if !(day >= 1.0 && day <= f64::from(days_in_month(year, month))) {
        return Err(Thrown::range_error("day is out of range"));
    }

    // `CreateISODateRecord(year, month, day)`.
    Ok(PlainDate::new(i64::from(year), u32::from(month), day as u32))
}

/// `isValidPlainDateOrThrow(globalObject, scope, plainDate)`: `ISODateWithinLimits`.
fn check_plain_date_within_limits(plain_date: PlainDate) -> Result<(), Thrown> {
    if !is_date_time_within_limits(plain_date.year(), plain_date.month(), plain_date.day(), 12, 0, 0, 0, 0, 0) {
        return Err(Thrown::range_error("date time is out of range of ECMAScript representation"));
    }
    Ok(())
}

/// `createTemporalDate(globalObject, plainDate, calendarID[, newTarget])`:
/// https://tc39.es/proposal-temporal/#sec-temporal-createtemporaldate
/// `new_target` é `Some((newTarget, jsCallee))` na construção e `None` na criação intrínseca.
pub fn create_temporal_date(
    global_object: &JSGlobalObject,
    plain_date: PlainDate,
    calendar_id: CalendarID,
    new_target: Option<(JSValue, usize)>,
) -> Result<TemporalPlainDateRef, Thrown> {
    // Passo 1: `ISODateWithinLimits(isoDate)` falso é `RangeError`.
    check_plain_date_within_limits(plain_date)?;

    // Passos 2 e 3: `newTarget` ausente é `%Temporal.PlainDate%`; `OrdinaryCreateFromConstructor`.
    let structure = match new_target {
        None => global_object.plain_date_structure(),
        Some((new_target, constructor)) => {
            get_derived_structure_in_realm(global_object, new_target, constructor, |realm| realm.plain_date_structure())?
        }
    };

    // Passos 4 a 6: `[[ISODate]]` e `[[Calendar]]`.
    Ok(TemporalPlainDate::create(global_object.vm(), &structure, plain_date, calendar_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_constructor_fields() {
        let date = validate_and_create_iso_date_record(2020.0, 2.0, 29.0).unwrap();
        assert_eq!((date.year(), date.month(), date.day()), (2020, 2, 29));
        assert_eq!(validate_and_create_iso_date_record(2021.0, 2.0, 29.0), Err(Thrown::range_error("day is out of range")));
        assert_eq!(validate_and_create_iso_date_record(2021.0, 0.0, 1.0), Err(Thrown::range_error("month is out of range")));
        assert_eq!(validate_and_create_iso_date_record(275761.0, 1.0, 1.0), Err(Thrown::range_error("year is out of range")));
        assert!(validate_and_create_iso_date_record(f64::from(MIN_YEAR), 1.0, 1.0).is_ok());
    }

    #[test]
    fn date_limits() {
        assert!(check_plain_date_within_limits(PlainDate::new(2020, 1, 1)).is_ok());
        assert!(check_plain_date_within_limits(PlainDate::new(-271821, 4, 18)).is_err());
        assert!(check_plain_date_within_limits(PlainDate::new(-271821, 4, 19)).is_ok());
        assert!(check_plain_date_within_limits(PlainDate::new(275760, 9, 13)).is_ok());
        assert!(check_plain_date_within_limits(PlainDate::new(275760, 9, 14)).is_err());
        assert!(check_plain_date_within_limits(PlainDate::sentinel()).is_err());
    }
}
