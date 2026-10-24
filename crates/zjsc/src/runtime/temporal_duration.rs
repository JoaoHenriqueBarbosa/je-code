//! Porte de `runtime/TemporalDuration.{h,cpp}`: a célula `Temporal.Duration` (`TemporalDuration`, um
//! `JSNonFinalObject` com o `ISO8601::Duration` em `m_duration`), `createTemporalDuration`,
//! `toTemporalDurationRecord`, `from`, `compare`, `with`, `addDurations`, `round`, `total` e `toString`.
//!
//! DIVERGÊNCIAS:
//! - `relativeTo` (`getTemporalRelativeToOption`, `computePlainRelativeTarget`, `differencePlainDateTimeWithTotal`
//!   e os ramos de `compare`/`round`/`total` com `plainRelativeTo` ou `zonedRelativeTo`) funciona para `PlainDate`,
//!   `PlainDateTime`, `ZonedDateTime`, cadeias (com ou sem anotação de fuso) e objetos de campos, em qualquer
//!   calendário embutido: a soma e a diferença de data passam por `calendar_date_add` e `calendar_date_until`
//!   com o `CalendarID` do `relativeTo` (`calendarAwareDateAdd` do C++). O `Get(options, "relativeTo")` roda sempre
//!   (a ordem observável das leituras vale) e
//!   `undefined` segue o caminho sem `relativeTo`. Cadeias passam por `parseISODateTime` e lançam os mesmos
//!   `TypeError`/`RangeError` do C++ (inválida, fuso, calendário, fora do limite).
//!   Sem `relativeTo` o resultado é exatamente o do C++, incluindo os `RangeError` de calendário sem `relativeTo`.
//! - `TemporalDuration::toString(globalObject, ...)` devolve `String` do Rust (o resultado é sempre ASCII).
//! - A estrutura intrínseca (`globalObject->durationStructure()`) mora em `TemporalGlobalData`.
//! - `JSC_DEFINE_TEMPORAL_DURATION_FIELD` (`years()`...`nanoseconds()`, `setYears`...) é `Duration::field`;
//!   os `setX` só serviam a construção e não têm chamador.

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::intl_support::{get_options_object, option_string, to_number_checked, to_rust_string};
use crate::runtime::iso8601::{
    is_date_time_within_limits, is_valid_duration, parse_duration, parse_iso_date_time, round_time_duration, Duration, ExactTime,
    InternalDuration, PlainDate, PlainTime, TemporalProduction, TemporalProductionSet,
};
use crate::runtime::temporal_calendar::{is_builtin_calendar, CalendarID, ISO8601_CALENDAR_ID};
use crate::runtime::temporal_calendar_icu::{calendar_date_add, calendar_date_until};
use crate::runtime::temporal_calendar::{get_temporal_calendar_identifier_with_iso_default, interpret_temporal_date_time_fields};
use crate::runtime::temporal_core_duration::nudge_to_calendar_unit;
use crate::runtime::temporal_core_zoned_date_time::{difference_zoned_date_time_with_rounding, interpret_iso_date_time_offset, MatchBehaviour, UseStartOfDay};
use crate::runtime::iso8601::PlainDateTime;
use crate::runtime::temporal_object::{OffsetBehaviour, TemporalDisambiguation, TemporalOffsetDisambiguation};
use crate::runtime::temporal_time_zone::{add_zoned_date_time, get_iso_date_time_for, TimeZone};
use crate::runtime::temporal_zoned_date_time::{
    create_temporal_zoned_date_time, read_zoned_date_time_fields_from_object, time_zone_from_record, TemporalZonedDateTime,
    TemporalZonedDateTimeRef, ZonedDateTimeFieldMode,
};
use crate::runtime::temporal_core_plain_date_time::{difference_plain_date_time_with_rounding, difference_plain_date_time_with_total};
use crate::runtime::temporal_plain_date::TemporalPlainDate;
use crate::runtime::temporal_plain_date_time::TemporalPlainDateTime;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::math_common::is_integer;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_core_duration::{
    add_24_hour_days_to_time_duration, duration_sign, largest_subduration, plain_time_from_subday_ns, split_time_duration,
    temporal_duration_from_internal, time_duration_from_components, to_internal_duration, to_internal_duration_record,
    to_internal_duration_record_with_24_hour_days, total_time_duration,
};
use crate::runtime::temporal_core_rounding::{maximum_rounding_increment, round_number_to_increment_i128, validate_temporal_rounding_increment};
use crate::runtime::temporal_object::{
    ellipsize_at, format_seconds_string_fraction, is_calendar_unit, string_units, temporal_fractional_second_digits,
    temporal_rounding_increment, temporal_rounding_mode, temporal_unit_plural_property_name, temporal_unit_type, temporal_unit_valued,
    throw_range_error_with_units, to_seconds_string_precision_record, validate_temporal_unit_value, AddOrSubtract, AllowedUnit, Inclusivity,
    Precision, RoundingMode, TemporalOverflow, TemporalUnit, TemporalUnitDefault, UnitGroup, UnitOption, TEMPORAL_UNITS_IN_TABLE_ORDER,
};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalDuration::s_info` (`"Object"`).
pub static TEMPORAL_DURATION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class TemporalDuration final : public JSNonFinalObject`.
pub struct TemporalDuration {
    base: JSNonFinalObject,
    /// `m_duration`.
    duration: Duration,
}

/// A referência à célula, o `*` do C++.
pub type TemporalDurationRef = Rc<TemporalDuration>;

impl std::ops::Deref for TemporalDuration {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

/// `std::array<std::optional<double>, numberOfTemporalUnits>`: o resultado de
/// `ToTemporalPartialDurationRecord`, indexado por `TemporalUnit as usize`.
type PartialDuration = [Option<f64>; crate::runtime::temporal_object::NUMBER_OF_TEMPORAL_UNITS];

const NOT_VALID_DURATION_MESSAGE: &str = "Temporal.Duration properties must be finite and of consistent sign";

impl TemporalDuration {
    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(ObjectType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &TEMPORAL_DURATION_S_INFO,
        )
    }

    /// `create(vm, structure, duration)`.
    pub fn create(vm: &VM, structure: &StructureRef, duration: Duration) -> TemporalDurationRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(TemporalDuration { base: JSNonFinalObject::new(vm, Rc::clone(structure)), duration });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TemporalDuration(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<TemporalDuration>(value)`.
    pub fn from_value(value: &JSValue) -> Option<TemporalDurationRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::TemporalDuration(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `duration()`.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// `sign()`: `TemporalCore::durationSign(m_duration)`.
    pub fn sign(&self) -> i32 {
        duration_sign(&self.duration)
    }

    /// `toTemporalDurationRecord(globalObject, item)`: https://tc39.es/proposal-temporal/#sec-temporal-totemporalduration
    /// (devolve o registro puro, sem a célula).
    pub fn to_temporal_duration_record(global_object: &JSGlobalObject, item: JSValue) -> Result<Duration, Thrown> {
        // Passo 1: se `item` é um objeto com `[[InitializedTemporalDuration]]`, o registro dele.
        if item.is_object() {
            if let Some(duration) = TemporalDuration::from_value(&item) {
                return Ok(duration.duration);
            }

            // Passos 3 a 15 (ramo objeto): `ToTemporalPartialDurationRecord(item)` e o resultado com zero nos
            // campos ausentes.
            let partial = to_temporal_partial_duration_record(global_object, item)?;
            let mut result = Duration::default();
            for (unit, value) in TemporalUnit::ALL.into_iter().zip(partial) {
                if let Some(value) = value {
                    result.set_field(unit, value);
                }
            }

            // Passo 15: `CreateTemporalDuration`, que confere `IsValidDuration`.
            if !is_valid_duration(&result) {
                return Err(Thrown::range_error(NOT_VALID_DURATION_MESSAGE));
            }
            return Ok(result);
        }

        // Passo 2.a: nem objeto nem `String` é `TypeError`.
        if !item.is_string() {
            return Err(Thrown::type_error("can only convert to Duration from object or string values"));
        }

        // Passo 2.b: `ParseTemporalDurationString(item)`.
        let units = string_units(global_object, item)?;
        let Some(parsed) = parse_duration(&units) else {
            // 3090: 308 dígitos * 10 campos + 10 designadores.
            return Err(throw_range_error_with_units(global_object, "'", &ellipsize_at(3090, &units), "' is not a valid Duration string"));
        };

        if !is_valid_duration(&parsed) {
            return Err(Thrown::range_error(NOT_VALID_DURATION_MESSAGE));
        }
        Ok(parsed)
    }

    /// `from(globalObject, item)`: https://tc39.es/proposal-temporal/#sec-temporal.duration.from
    /// Sempre uma célula nova, mesmo quando `item` já é uma `Duration` (o `CreateTemporalDuration` da spec).
    pub fn from(global_object: &JSGlobalObject, item: JSValue) -> Result<TemporalDurationRef, Thrown> {
        let record = TemporalDuration::to_temporal_duration_record(global_object, item)?;
        Ok(TemporalDuration::create(global_object.vm(), &global_object.duration_structure(), record))
    }

    /// `compare(globalObject, one, two, options)`: https://tc39.es/proposal-temporal/#sec-temporal.duration.compare
    pub fn compare(global_object: &JSGlobalObject, value_one: JSValue, value_two: JSValue, options_value: JSValue) -> Result<JSValue, Thrown> {
        // Passos 1 e 2: `ToTemporalDuration` dos dois, no formato de registro (`compare` só lê campos).
        let one = TemporalDuration::to_temporal_duration_record(global_object, value_one)?;
        let two = TemporalDuration::to_temporal_duration_record(global_object, value_two)?;

        // O tipo de `options` é conferido sempre, mesmo sem `relativeTo`.
        let options = get_options_object(options_value)?;

        // `relativeTo` é lido antes do atalho de igualdade, na ordem da spec.
        let relative_to = match options {
            Some(options) => get_temporal_relative_to_option(global_object, options)?,
            None => RelativeToRecord::default(),
        };

        // Depois de ler `relativeTo`, durações idênticas são sempre iguais.
        if TemporalUnit::ALL.into_iter().all(|unit| one.field(unit) == two.field(unit)) {
            return Ok(js_number(0));
        }

        // Com `zonedRelativeTo` e unidade de data (anos, meses, semanas ou dias), `AddZonedDateTime` decide.
        let has_date_unit = |duration: &Duration| duration.years() != 0 || duration.months() != 0 || duration.weeks() != 0 || duration.days() != 0;
        if let Some(zoned_date_time) = &relative_to.zoned {
            if has_date_unit(&one) || has_date_unit(&two) {
                let ns_one = compute_zoned_relative_endpoints(zoned_date_time, &one)?.end_exact.epoch_nanoseconds();
                let ns_two = compute_zoned_relative_endpoints(zoned_date_time, &two)?.end_exact.epoch_nanoseconds();
                return Ok(js_number(match ns_one.cmp(&ns_two) {
                    std::cmp::Ordering::Greater => 1,
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                }));
            }
        }
        // Sem unidade de calendário, o caminho rápido: comparação de tempo com dias de 24 horas.
        let has_calendar_units = [TemporalUnit::Year, TemporalUnit::Month, TemporalUnit::Week]
            .into_iter()
            .any(|unit| one.field(unit) != 0.0 || two.field(unit) != 0.0);
        if !has_calendar_units {
            let time_duration_one = add_24_hour_days_to_time_duration(to_internal_duration(&one).time(), one.days() as f64)?;
            let time_duration_two = add_24_hour_days_to_time_duration(to_internal_duration(&two).time(), two.days() as f64)?;
            return Ok(js_number(match time_duration_one.cmp(&time_duration_two) {
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
            }));
        }

        let Some(relative_to) = relative_to.plain else {
            return Err(Thrown::range_error("Cannot compare a duration of years, months, or weeks without a relativeTo option"));
        };

        // `PlainDate` como `relativeTo`: `DateDurationDays(dateDuration, plainDate)` de cada lado.
        let time_duration_one = plain_relative_time_duration(relative_to.calendar_id, &one, relative_to.date)?;
        let time_duration_two = plain_relative_time_duration(relative_to.calendar_id, &two, relative_to.date)?;
        Ok(js_number(match time_duration_one.cmp(&time_duration_two) {
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
        }))
    }

    /// `with(globalObject, durationLike)`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.with
    /// O chamador confere que `durationLike` é um objeto.
    pub fn with(&self, global_object: &JSGlobalObject, duration_like: JSValue) -> Result<Duration, Thrown> {
        // Passo 3: `ToTemporalPartialDurationRecord(temporalDurationLike)`.
        let partial = to_temporal_partial_duration_record(global_object, duration_like)?;

        // Passos 4 a 23: o campo parcial, quando existe, senão o da duração.
        let mut result = Duration::default();
        for (unit, value) in TemporalUnit::ALL.into_iter().zip(partial) {
            result.set_field(unit, value.unwrap_or_else(|| self.duration.field(unit)));
        }

        // Passo 24: `CreateTemporalDuration` (quem chama).
        Ok(result)
    }

    /// `addDurations<op>(globalObject, other)` (`AddDurations`): https://tc39.es/proposal-temporal/#sec-temporal-adddurations
    pub fn add_durations(&self, global_object: &JSGlobalObject, operation: AddOrSubtract, other_value: JSValue) -> Result<Duration, Thrown> {
        // Passo 1: `other = ? ToTemporalDuration(other)`.
        let mut other = TemporalDuration::to_temporal_duration_record(global_object, other_value)?;

        // Passo 2: `subtract` nega `other` (`CreateNegatedTemporalDuration`).
        if operation == AddOrSubtract::Subtract {
            other = -other;
        }

        // Passos 3 a 5: `largestUnit = LargerOfTwoTemporalUnits(DefaultTemporalLargestUnit(duration), ...)`;
        // no enum a unidade maior tem o índice menor.
        let largest_unit = largest_subduration(&self.duration).min(largest_subduration(&other));

        // Passo 6: unidade de calendário é `RangeError`.
        if is_calendar_unit(largest_unit) {
            return Err(Thrown::range_error("Cannot add or subtract durations with calendar units (years, months, or weeks)"));
        }

        // Passos 7 e 8: `ToInternalDurationRecordWith24HourDays` dos dois.
        let d1 = to_internal_duration_record_with_24_hour_days(&self.duration)?;
        let d2 = to_internal_duration_record_with_24_hour_days(&other)?;

        // Passo 9: `AddTimeDuration` é `a + b` seguido da conferência contra `maxTimeDuration`.
        let time_result = d1.time() + d2.time();
        if time_result.abs() > InternalDuration::MAX_TIME_DURATION {
            return Err(Thrown::range_error("Sum of durations exceeds maximum time duration"));
        }

        // Passos 10 e 11: `CombineDateAndTimeDuration(ZeroDateDuration(), timeResult)` e
        // `TemporalDurationFromInternal(result, largestUnit)`.
        let result = InternalDuration::new(Duration::default(), time_result);
        Ok(temporal_duration_from_internal(&result, largest_unit)?)
    }

    /// `round(globalObject, options)`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.round
    pub fn round(&self, global_object: &JSGlobalObject, options_value: JSValue) -> Result<Duration, Thrown> {
        // Passo 3: `roundTo` `undefined` é `TypeError`.
        if options_value.is_undefined() {
            return Err(Thrown::type_error("Temporal.Duration.prototype.round requires a roundTo option"));
        }

        let mut options: Option<JSValue> = None;
        let mut smallest: Option<TemporalUnit> = None;

        if options_value.is_string() {
            // Passo 4: `roundTo` `String` é o `smallestUnit` direto (sem o objeto intermediário).
            let string = to_rust_string(global_object, options_value)?;
            smallest = temporal_unit_type(&string);
            if smallest.is_none() {
                return Err(Thrown::range_error("smallestUnit is an invalid Temporal unit"));
            }
        } else {
            // Passo 5: `GetOptionsObject(roundTo)`.
            options = get_options_object(options_value)?;
        }

        // Passos 6 e 7: `smallestUnitPresent` e `largestUnitPresent` começam verdadeiros.
        let mut largest_unit_present = true;

        // Passos 8 e 9: as opções são lidas em ordem alfabética; `largestUnit` primeiro.
        let largest_unit_maybe_auto = temporal_unit_valued(global_object, options, "largestUnit", TemporalUnitDefault::Unset)?;
        // Passos 10 a 12: `GetTemporalRelativeToOption(roundTo)`.
        let relative_to = match options {
            Some(options) => get_temporal_relative_to_option(global_object, options)?,
            None => RelativeToRecord::default(),
        };
        // Passo 13: `GetRoundingIncrementOption`. Passo 14: `GetRoundingModeOption(roundTo, ~half-expand~)`.
        let rounding_increment = temporal_rounding_increment(global_object, options)?;
        let rounding_mode = temporal_rounding_mode(global_object, options, RoundingMode::HalfExpand)?;

        if smallest.is_none() {
            // Passos 15 e 16: `smallestUnit` e `ValidateTemporalUnitValue(smallestUnit, ~datetime~)`.
            let smallest_unit_maybe_auto = temporal_unit_valued(global_object, options, "smallestUnit", TemporalUnitDefault::Unset)?;
            validate_temporal_unit_value(smallest_unit_maybe_auto, UnitGroup::DateTime, AllowedUnit::None, "smallestUnit")?;
            if let UnitOption::Unit(unit) = smallest_unit_maybe_auto {
                smallest = Some(unit);
            }
        } else {
            // Passo 16 (caminho da `String`).
            validate_temporal_unit_value(UnitOption::Unit(smallest.unwrap_or(TemporalUnit::Nanosecond)), UnitGroup::DateTime, AllowedUnit::None, "smallestUnit")?;
        }

        // Passo 17: sem `smallestUnit`, `smallestUnitPresent = false` e `smallestUnit = ~nanosecond~`.
        let smallest_unit_present = smallest.is_some();
        let smallest_unit = smallest.unwrap_or(TemporalUnit::Nanosecond);

        // Passos 18 e 19: `existingLargestUnit` e `defaultLargestUnit`.
        let existing_largest_unit = largest_subduration(&self.duration);
        let default_largest_unit = existing_largest_unit.min(smallest_unit);

        // Passos 20 e 21: `largestUnit` de `~unset~`, `~auto~` ou explícito.
        let largest_unit = match largest_unit_maybe_auto {
            UnitOption::Unset => {
                largest_unit_present = false;
                default_largest_unit
            }
            UnitOption::Auto => default_largest_unit,
            UnitOption::Unit(unit) => unit,
        };

        // Passo 22: sem `smallestUnit` nem `largestUnit`, `RangeError`.
        if !smallest_unit_present && !largest_unit_present {
            return Err(Thrown::range_error("Cannot round without a smallestUnit or largestUnit option"));
        }

        // Passo 23: `LargerOfTwoTemporalUnits(largestUnit, smallestUnit)` tem de ser `largestUnit`.
        if smallest_unit < largest_unit {
            return Err(Thrown::range_error("smallestUnit must be smaller than largestUnit"));
        }

        // Passos 24 e 25: o máximo do incremento da unidade menor.
        if let Some(maximum) = maximum_rounding_increment(smallest_unit) {
            validate_temporal_rounding_increment(rounding_increment, Some(f64::from(maximum)), Inclusivity::Exclusive)?;
        }

        // Passo 26: incremento maior que 1 com `largestUnit` diferente de `smallestUnit` e unidade de data.
        if rounding_increment > 1.0 && largest_unit != smallest_unit && smallest_unit <= TemporalUnit::Day {
            return Err(Thrown::range_error("Incompatible rounding increment and largest/smallest units"));
        }

        // Passo 27: `zonedRelativeTo`.
        if let Some(zoned_date_time) = &relative_to.zoned {
            // Passos 27.a a 27.e: `AddZonedDateTime` dá os extremos.
            let endpoints = compute_zoned_relative_endpoints(zoned_date_time, &self.duration)?;
            // Passo 27 (atalho): saída só de tempo dispensa a diferença de calendário.
            if largest_unit > TemporalUnit::Day {
                let time_only = InternalDuration::new(
                    Duration::default(),
                    endpoints.end_exact.epoch_nanoseconds() - endpoints.start_exact.epoch_nanoseconds(),
                );
                let rounded = round_internal_duration(time_only, rounding_increment, smallest_unit, rounding_mode)?;
                return Ok(temporal_duration_from_internal(&rounded, largest_unit)?);
            }
            // Passo 27.f: `DifferenceZonedDateTimeWithRounding`.
            let internal = difference_zoned_date_time_with_rounding(
                zoned_date_time.calendar_id(),
                endpoints.start_exact,
                endpoints.end_exact,
                zoned_date_time.time_zone(),
                largest_unit,
                smallest_unit,
                rounding_mode,
                rounding_increment,
            )?;
            // Passos 27.g e 27.h: unidade de data vira `~hour~`.
            let effective_largest_unit = if largest_unit <= TemporalUnit::Day { TemporalUnit::Hour } else { largest_unit };
            return Ok(temporal_duration_from_internal(&internal, effective_largest_unit)?);
        }

        // Passo 28: `plainRelativeTo`.
        if let Some(relative_to) = relative_to.plain {
            // Passo 28.a: `ToInternalDurationRecordWith24HourDays(duration)`.
            let internal_duration = to_internal_duration_record_with_24_hour_days(&self.duration)?;
            // Passos 28.b a 28.e: `{ targetDate, targetTime }`.
            let (target_date, target_time) = compute_plain_relative_target(relative_to.calendar_id, relative_to.date, &internal_duration)?;
            // Passos 28.f a 28.h: `DifferencePlainDateTimeWithRounding` a partir da meia-noite de `plainDate`.
            let diff = difference_plain_date_time_with_rounding(
                relative_to.calendar_id,
                relative_to.date,
                PlainTime::default(),
                target_date,
                target_time,
                largest_unit,
                smallest_unit,
                rounding_mode,
                rounding_increment,
            )?;
            // Passo 28.i: `TemporalDurationFromInternal(internalDuration, largestUnit)`.
            return Ok(temporal_duration_from_internal(&diff, largest_unit)?);
        }

        // Passo 29: unidade de calendário sem `relativeTo` é `RangeError`.
        if self.duration.years() != 0 || self.duration.months() != 0 || self.duration.weeks() != 0 || is_calendar_unit(largest_unit) {
            return Err(Thrown::range_error("Cannot round a duration of years, months, or weeks without a relativeTo option"));
        }

        // Passo 30: `IsCalendarUnit(smallestUnit)` é falso. Passos 31 a 34: arredonda por `smallestUnit` e
        // `TemporalDurationFromInternal(internalDuration, largestUnit)`.
        debug_assert!(!is_calendar_unit(smallest_unit));
        let internal_duration = to_internal_duration_record_with_24_hour_days(&self.duration)?;
        let result = round_internal_duration(internal_duration, rounding_increment, smallest_unit, rounding_mode)?;
        Ok(temporal_duration_from_internal(&result, largest_unit)?)
    }

    /// `total(globalObject, options)`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.total
    pub fn total(&self, global_object: &JSGlobalObject, options_value: JSValue) -> Result<f64, Thrown> {
        // Passos 3 a 7: `totalOf` pode ser uma `String` (tratada como `{ unit }`) ou um objeto de opções.
        let mut unit_string: Option<String> = None;
        let mut options: Option<JSValue> = None;
        if options_value.is_string() {
            unit_string = Some(to_rust_string(global_object, options_value)?);
        } else {
            options = get_options_object(options_value)?;
        }

        // Passos 8 a 10: `relativeTo` é lido antes de `unit` (ordem alfabética).
        let mut relative_to = RelativeToRecord::default();
        if let Some(options) = options {
            relative_to = get_temporal_relative_to_option(global_object, options)?;
            // Passo 11: `unit = ? GetTemporalUnitValuedOption(totalOf, "unit", unset)`.
            unit_string = option_string(global_object, Some(options), "unit", &[], "")?;
        }

        // Passo 12: `ValidateTemporalUnitValue(unit, ~datetime~)`.
        let Some(unit) = unit_string.as_deref().and_then(temporal_unit_type) else {
            return Err(Thrown::range_error("unit is an invalid Temporal unit"));
        };

        // Passo 13 (sem `relativeTo`): rejeita unidades de calendário.
        if relative_to.zoned.is_none() && relative_to.plain.is_none() {
            if is_calendar_unit(unit) || self.duration.years() != 0 || self.duration.months() != 0 || self.duration.weeks() != 0 {
                return Err(Thrown::range_error("Cannot total a duration of years, months, or weeks without a relativeTo option"));
            }
            // Passos 13.b e 13.c: `ToInternalDurationRecordWith24HourDays` e `TotalTimeDuration`.
            let internal_duration = to_internal_duration_record_with_24_hour_days(&self.duration)?;
            return Ok(total_time_duration(internal_duration.time(), unit));
        }
        // Passo 14: `zonedRelativeTo`.
        if let Some(zoned_date_time) = &relative_to.zoned {
            let endpoints = compute_zoned_relative_endpoints(zoned_date_time, &self.duration)?;
            return difference_zoned_date_time_with_total(zoned_date_time, &endpoints, unit);
        }
        let relative_to = relative_to.plain.expect("sem zonedRelativeTo, o plainRelativeTo existe");
        // Passo 15: `plainRelativeTo`. Passo 15.a: `ToInternalDurationRecordWith24HourDays`.
        let internal_duration = to_internal_duration_record_with_24_hour_days(&self.duration)?;
        // Passos 15.b a 15.e: `{ targetDate, targetTime }`.
        let (target_date, target_time) = compute_plain_relative_target(relative_to.calendar_id, relative_to.date, &internal_duration)?;
        // Passos 15.f a 15.h: `DifferencePlainDateTimeWithTotal`.
        Ok(difference_plain_date_time_with_total(relative_to.calendar_id, relative_to.date, target_date, target_time, unit)?)
    }

    /// `toString(globalObject, options)`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.tostring
    pub fn to_string_with_options(&self, global_object: &JSGlobalObject, options_value: JSValue) -> Result<String, Thrown> {
        // Passo 3: `GetOptionsObject(options)`.
        let options = get_options_object(options_value)?;
        if options.is_none() {
            return Ok(self.to_string());
        }

        // Passos 4 e 5: as opções em ordem alfabética; `GetTemporalFractionalSecondDigitsOption`.
        let digits = temporal_fractional_second_digits(global_object, options)?;
        // Passo 6: `GetRoundingModeOption(resolvedOptions, ~trunc~)`.
        let rounding_mode = temporal_rounding_mode(global_object, options, RoundingMode::Trunc)?;
        // Passos 7 e 8: `smallestUnit` e `ValidateTemporalUnitValue(smallestUnit, ~time~)`.
        let smallest_unit_result = temporal_unit_valued(global_object, options, "smallestUnit", TemporalUnitDefault::Unset)?;
        validate_temporal_unit_value(smallest_unit_result, UnitGroup::Time, AllowedUnit::None, "smallestUnit")?;
        let smallest_unit = match smallest_unit_result {
            UnitOption::Unit(unit) => Some(unit),
            UnitOption::Auto | UnitOption::Unset => None,
        };
        // Passo 9: `hour` ou `minute` é `RangeError`.
        if matches!(smallest_unit, Some(TemporalUnit::Hour | TemporalUnit::Minute)) {
            return Err(Thrown::range_error("smallestUnit must not be \"minute\" or larger"));
        }

        // Passo 10: `ToSecondsStringPrecisionRecord(smallestUnit, digits)`.
        let data = to_seconds_string_precision_record(smallest_unit, digits);

        // Passo 11: precisão `auto` é incremento 1 ns, sem efeito para qualquer modo de arredondamento.
        if data.precision.0 == Precision::Auto {
            return Ok(self.to_string());
        }

        // Passo 12: `DefaultTemporalLargestUnit(duration)`.
        let largest_unit = largest_subduration(&self.duration);

        // Passo 13: `ToInternalDurationRecord(duration)`.
        let internal_duration = to_internal_duration_record(&self.duration);

        // Passo 14: `RoundTimeDuration(internalDuration.[[Time]], precision.[[Increment]], precision.[[Unit]], roundingMode)`.
        let rounded_time = round_time_duration(internal_duration.time(), data.increment, data.unit, rounding_mode)?;

        // Passo 15: `CombineDateAndTimeDuration(internalDuration.[[Date]], timeDuration)`.
        let internal_duration = InternalDuration::new(internal_duration.date_duration(), rounded_time);

        // Passo 16: `LargerOfTwoTemporalUnits(largestUnit, ~second~)`.
        let rounded_largest_unit = largest_unit.min(TemporalUnit::Second);

        // Passo 17: `TemporalDurationFromInternal` chama `CreateTemporalDuration`, que confere `IsValidDuration`;
        // dias e tempo juntos acima de 2^53 segundos depois da decomposição são `RangeError`.
        let rounded_duration = temporal_duration_from_internal(&internal_duration, rounded_largest_unit)?;

        // Passo 18: `TemporalDurationToString(roundedDuration, precision.[[Precision]])`.
        Ok(temporal_duration_to_string(&rounded_duration, data.precision))
    }

    /// `toString(globalObject)` com a precisão padrão `{ Auto, 0 }` (`toJSON` e o ramo sem opções).
    pub fn to_string(&self) -> String {
        temporal_duration_to_string(&self.duration, (Precision::Auto, 0))
    }
}

/// `createTemporalDuration(globalObject, duration)` e a variante com `TemporalNewTarget { newTarget,
/// constructor }`: https://tc39.es/proposal-temporal/#sec-temporal-createtemporalduration
/// `new_target` é `Some((newTarget, jsCallee))` na construção e `None` na criação intrínseca.
pub fn create_temporal_duration(
    global_object: &JSGlobalObject,
    duration: Duration,
    new_target: Option<(JSValue, usize)>,
) -> Result<TemporalDurationRef, Thrown> {
    // Passo 1: `IsValidDuration` falso é `RangeError`.
    if !is_valid_duration(&duration) {
        return Err(Thrown::range_error(NOT_VALID_DURATION_MESSAGE));
    }

    // Passos 2 e 3: `newTarget` ausente é `%Temporal.Duration%`; `OrdinaryCreateFromConstructor`.
    let structure = match new_target {
        None => global_object.duration_structure(),
        Some((new_target, constructor)) => {
            get_derived_structure_in_realm(global_object, new_target, constructor, |realm| realm.duration_structure())?
        }
    };

    // Passos 4 a 14.
    Ok(TemporalDuration::create(global_object.vm(), &structure, duration))
}

/// `ToTemporalPartialDurationRecord(temporalDurationLike)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-totemporalpartialdurationrecord
fn to_temporal_partial_duration_record(global_object: &JSGlobalObject, duration_like: JSValue) -> Result<PartialDuration, Thrown> {
    let vm = global_object.vm();

    // Passo 2: cada campo começa `undefined` (o `std::optional` vazio).
    let mut result: PartialDuration = Default::default();

    // Passo 3 NOTE: as propriedades são lidas em ordem alfabética (`days`, `hours`, `microseconds`...);
    // `temporalUnitsInTableOrder`, apesar do nome, é essa ordem.
    let mut any = false;
    for unit in TEMPORAL_UNITS_IN_TABLE_ORDER {
        let value = get_value_property(global_object, duration_like, &temporal_unit_plural_property_name(vm, unit))?;
        if value.is_undefined() {
            continue;
        }

        any = true;
        // `ToIntegerIfIntegral`: o `+ 0.0` faz o passo 4 (-0 vira +0); `!isInteger` cobre os passos 2 e 3
        // (NaN e ±∞ não são inteiros).
        let v = to_number_checked(global_object, value)? + 0.0;
        if !is_integer(v) {
            return Err(Thrown::range_error("Temporal.Duration properties must be integers"));
        }
        result[unit as usize] = Some(v);
    }

    // Passo 23: tudo `undefined` é `TypeError`.
    if !any {
        return Err(Thrown::type_error("Object must contain at least one Temporal.Duration property"));
    }

    // Passo 24.
    Ok(result)
}

/// `struct PlainRelativeTo`: a data e o calendário do `relativeTo` sem fuso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlainRelativeTo {
    date: PlainDate,
    calendar_id: CalendarID,
}

/// `struct RelativeToRecord`: `[[ZonedRelativeTo]]` e `[[PlainRelativeTo]]`, no máximo um dos dois.
#[derive(Default)]
struct RelativeToRecord {
    zoned: Option<TemporalZonedDateTimeRef>,
    plain: Option<PlainRelativeTo>,
}

/// `struct ZonedRelativeEndpoints`: o instante de `zonedRelativeTo` e o dele somado à duração.
struct ZonedRelativeEndpoints {
    start_exact: ExactTime,
    end_exact: ExactTime,
}

/// `computeZonedRelativeEndpoints(globalObject, zdt, duration)`.
fn compute_zoned_relative_endpoints(zoned_date_time: &TemporalZonedDateTime, duration: &Duration) -> Result<ZonedRelativeEndpoints, Thrown> {
    let start_exact = zoned_date_time.exact_time();
    let end_exact = add_zoned_date_time(start_exact, zoned_date_time.time_zone(), zoned_date_time.calendar_id(), duration, TemporalOverflow::Constrain)?;
    Ok(ZonedRelativeEndpoints { start_exact, end_exact })
}

/// `differenceZonedDateTimeWithTotal(globalObject, zdt, endpoints, unit)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-differencezoneddatetimewithtotal
fn difference_zoned_date_time_with_total(
    zoned_date_time: &TemporalZonedDateTime,
    endpoints: &ZonedRelativeEndpoints,
    unit: TemporalUnit,
) -> Result<f64, Thrown> {
    let ns_a = endpoints.start_exact.epoch_nanoseconds();
    let ns_b = endpoints.end_exact.epoch_nanoseconds();
    // Passo 1: unidade de tempo é `TotalTimeDuration`.
    if unit > TemporalUnit::Day {
        return Ok(total_time_duration(ns_b - ns_a, unit));
    }
    let time_zone = zoned_date_time.time_zone();
    // Passo 2: `DifferenceZonedDateTime` (sem arredondar).
    let difference = difference_zoned_date_time_with_rounding(
        zoned_date_time.calendar_id(),
        endpoints.start_exact,
        endpoints.end_exact,
        time_zone,
        unit,
        unit,
        RoundingMode::Trunc,
        1.0,
    )?;
    // Passo 3: `dateTime = GetISODateTimeFor(tz, nsA)`.
    let PlainDateTime { date, time } = get_iso_date_time_for(time_zone, endpoints.start_exact)?;
    // Passo 4: `TotalRelativeDuration` por `nudgeToCalendarUnit` com `~trunc~` e incremento 1.
    let sign = if ns_b < ns_a { -1 } else { 1 };
    let nudged =
        nudge_to_calendar_unit(zoned_date_time.calendar_id(), sign, &difference, ns_a, ns_b, date, time, 1.0, unit, RoundingMode::Trunc, Some(time_zone))?;
    Ok(nudged.total)
}

/// `getTemporalRelativeToOption(globalObject, options)`: https://tc39.es/proposal-temporal/#sec-temporal-gettemporalrelativetooption
/// O registro vazio é o dos dois campos `undefined`.
fn get_temporal_relative_to_option(global_object: &JSGlobalObject, options: JSValue) -> Result<RelativeToRecord, Thrown> {
    // Passo 1: `Get(options, "relativeTo")`.
    let value = crate::runtime::intl_support::get_property(global_object, options, "relativeTo")?;
    // Passo 2: `undefined` devolve o registro com os dois campos `undefined`.
    if value.is_undefined() {
        return Ok(RelativeToRecord::default());
    }

    // Os dados dos passos 3 a 6: a data, a hora (`None` é `~start-of-day~`), o calendário e o que vem do fuso.
    let date: PlainDate;
    let time: Option<PlainTime>;
    let mut calendar_id = ISO8601_CALENDAR_ID;
    let mut time_zone: Option<TimeZone> = None;
    let mut offset_ns = 0;
    let mut offset_behaviour = OffsetBehaviour::Option;
    let mut match_behaviour = MatchBehaviour::MatchExactly;
    let mut source: Option<Vec<u16>> = None;

    if value.is_object() {
        // Passo 5.a: `ZonedDateTime`.
        if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&value) {
            return Ok(RelativeToRecord { zoned: Some(zoned_date_time), plain: None });
        }
        // Passo 5.b: `PlainDate`.
        if let Some(plain_date) = TemporalPlainDate::from_value(&value) {
            return Ok(RelativeToRecord { zoned: None, plain: Some(PlainRelativeTo { date: plain_date.plain_date(), calendar_id: plain_date.calendar_id() }) });
        }
        // Passo 5.c: `PlainDateTime`.
        if let Some(plain_date_time) = TemporalPlainDateTime::from_value(&value) {
            return Ok(RelativeToRecord {
                zoned: None,
                plain: Some(PlainRelativeTo { date: plain_date_time.plain_date(), calendar_id: plain_date_time.calendar_id() }),
            });
        }
        // Passos 5.d a 5.k: o objeto de campos.
        calendar_id = get_temporal_calendar_identifier_with_iso_default(global_object, value)?;
        let fields = read_zoned_date_time_fields_from_object(global_object, value, calendar_id, ZonedDateTimeFieldMode::RelativeToDuration)?;
        let result = interpret_temporal_date_time_fields(calendar_id, &fields.date_fields, &fields.time_fields, TemporalOverflow::Constrain)?;
        time_zone = fields.time_zone;
        match fields.offset_ns {
            Some(offset) => offset_ns = offset,
            None => offset_behaviour = OffsetBehaviour::Wall,
        }
        date = result.date;
        time = Some(result.time);
    } else {
        // Passo 6.a: o que não é `String` é `TypeError`.
        if !value.is_string() {
            return Err(Thrown::type_error("relativeTo must be a string or Temporal object"));
        }
        // Passo 6.b: `toWTFString`, depois a validação da cadeia.
        let units = string_units(global_object, value)?;
        let production_set = TemporalProductionSet::from(TemporalProduction::DateTimeZoned) | TemporalProductionSet::from(TemporalProduction::DateTimeUnzoned);
        let parsed = parse_iso_date_time(&units, production_set).ok_or_else(|| {
            throw_range_error_with_units(global_object, "'", &ellipsize_at(200, &units), "' is not a valid date or ZonedDateTime string")
        })?;
        date = parsed.date.expect("DateTimeString sempre traz a data");
        time = parsed.time;

        // Passos 6.c a 6.f: só a produção `[+Zoned]` carrega a anotação de fuso.
        if parsed.matched == TemporalProduction::DateTimeZoned {
            let record = parsed.time_zone.as_ref().expect("DateTimeString[+Zoned] sempre traz o fuso");
            time_zone = Some(time_zone_from_record(record).ok_or_else(|| {
                throw_range_error_with_units(global_object, "'", &ellipsize_at(200, &units), "' contains an invalid time zone identifier")
            })?);
            if record.z {
                offset_behaviour = OffsetBehaviour::Exact;
            } else if record.offset.is_none() {
                offset_behaviour = OffsetBehaviour::Wall;
            }
            offset_ns = record.offset.unwrap_or(0);
            match_behaviour =
                if record.offset_has_sub_minute_precision { MatchBehaviour::MatchExactly } else { MatchBehaviour::MatchMinutes };
        }

        // Passos 6.g a 6.i: `CanonicalizeCalendar`.
        if let Some(calendar) = &parsed.calendar {
            let raw_calendar: Vec<u16> = calendar.iter().map(|byte| u16::from(byte.to_ascii_lowercase())).collect();
            calendar_id = is_builtin_calendar(&raw_calendar)
                .ok_or_else(|| throw_range_error_with_units(global_object, "'", &raw_calendar, "' is not a valid calendar identifier"))?;
        }
        source = Some(units);
    }

    // Passo 7: sem fuso, `CreateTemporalDate` confere `isDateTimeWithinLimits(..., 12:00)`.
    let Some(time_zone) = time_zone else {
        if !is_date_time_within_limits(date.year(), date.month(), date.day(), 12, 0, 0, 0, 0, 0) {
            let units = source.unwrap_or_default();
            return Err(throw_range_error_with_units(
                global_object,
                "'",
                &ellipsize_at(200, &units),
                "' is outside the representable range for a relativeTo parameter",
            ));
        }
        return Ok(RelativeToRecord { zoned: None, plain: Some(PlainRelativeTo { date, calendar_id }) });
    };

    // Passos 8 a 10: `InterpretISODateTimeOffset(isoDate, time, offsetBehaviour, offsetNs, timeZone, ~compatible~,
    // ~reject~, matchBehaviour)`.
    let offset_ns = if offset_behaviour == OffsetBehaviour::Option { offset_ns } else { 0 };
    let exact_time = interpret_iso_date_time_offset(
        date,
        time.unwrap_or_default(),
        if time.is_some() { UseStartOfDay::No } else { UseStartOfDay::Yes },
        offset_behaviour,
        TemporalOffsetDisambiguation::Reject,
        offset_ns,
        match_behaviour,
        &time_zone,
        TemporalDisambiguation::Compatible,
    )?;
    // Passos 11 e 12: `CreateTemporalZonedDateTime(epochNanoseconds, timeZone, calendar)`.
    let zoned_date_time = create_temporal_zoned_date_time(global_object, exact_time, time_zone, calendar_id, None)?;
    Ok(RelativeToRecord { zoned: Some(zoned_date_time), plain: None })
}

/// A falha dos passos 6.b a 7 de `getTemporalRelativeToOption`, antes de existir um `PlainDate` ou `ZonedDateTime`.
#[derive(Debug, PartialEq, Eq)]
enum RelativeToStringError {
    /// `ParseISODateTime` falhou.
    NotValid,
    /// A cadeia casou `TemporalDateTimeString[+Zoned]`: o passo 6.f precisa de `timeZoneFromRecord` (ICU), não portado.
    TimeZoneUnported,
    /// Passo 6.h: o calendário (já em minúsculas ASCII, como no C++) não é nativo.
    InvalidCalendar(Vec<u16>),
    /// Passo 7: `isDateTimeWithinLimits` falhou.
    OutOfRange,
}

/// Passos 6.b a 7 de `getTemporalRelativeToOption` sobre uma cadeia: tudo o que lança sem precisar de
/// `PlainDate`, `ZonedDateTime` ou fuso, na ordem do C++. Só os testes a usam: `get_temporal_relative_to_option`
/// trata a produção `[+Zoned]` direto.
#[allow(dead_code)]
fn validate_relative_to_string(units: &[u16]) -> Result<PlainRelativeTo, RelativeToStringError> {
    let production_set = TemporalProductionSet::from(TemporalProduction::DateTimeZoned) | TemporalProductionSet::from(TemporalProduction::DateTimeUnzoned);
    // Passo 6.b.
    let Some(parsed) = parse_iso_date_time(units, production_set) else {
        return Err(RelativeToStringError::NotValid);
    };
    let date = parsed.date.expect("DateTimeString sempre traz a data");

    // Passos 6.c a 6.f: só a produção `[+Zoned]` carrega fuso.
    if parsed.matched == TemporalProduction::DateTimeZoned {
        return Err(RelativeToStringError::TimeZoneUnported);
    }

    // Passos 6.g a 6.i: `CanonicalizeCalendar`.
    let mut calendar_id = ISO8601_CALENDAR_ID;
    if let Some(calendar) = &parsed.calendar {
        let raw_calendar: Vec<u16> = calendar.iter().map(|byte| u16::from(byte.to_ascii_lowercase())).collect();
        match is_builtin_calendar(&raw_calendar) {
            Some(id) => calendar_id = id,
            None => return Err(RelativeToStringError::InvalidCalendar(raw_calendar)),
        }
    }

    // Passo 7: `CreateTemporalDate` confere `isDateTimeWithinLimits(..., 12:00)`.
    if !is_date_time_within_limits(date.year(), date.month(), date.day(), 12, 0, 0, 0, 0, 0) {
        return Err(RelativeToStringError::OutOfRange);
    }
    Ok(PlainRelativeTo { date, calendar_id })
}

/// `calendarDateAdd(calendarId, date, duration, ~constrain~)`: o despacho único de `temporal_calendar_icu`.
fn plain_relative_date_add(calendar_id: CalendarID, date: PlainDate, duration: &Duration) -> Result<PlainDate, Thrown> {
    Ok(calendar_date_add(calendar_id, date, duration, TemporalOverflow::Constrain)?)
}

/// O lado de `compare` com `PlainDate` como `relativeTo`: `DateDurationDays` somado ao tempo, em nanossegundos.
fn plain_relative_time_duration(calendar_id: CalendarID, duration: &Duration, plain_date: PlainDate) -> Result<i128, Thrown> {
    let date_duration = Duration::new(duration.years(), duration.months(), duration.weeks(), duration.days(), 0, 0, 0, 0, 0, 0);
    let end_date = plain_relative_date_add(calendar_id, plain_date, &date_duration)?;
    let days_diff = calendar_date_until(calendar_id, plain_date, end_date, TemporalUnit::Day)?;
    Ok(add_24_hour_days_to_time_duration(to_internal_duration(duration).time(), days_diff.days() as f64)?)
}

/// `computePlainRelativeTarget(calendarId, plainDate, internalDuration)`: a data e a hora que `plainDate` à
/// meia-noite alcança somando `internalDuration` (passos 28.b a 28.e de `round` e 15.b a 15.e de `total`).
fn compute_plain_relative_target(
    calendar_id: CalendarID,
    plain_date: PlainDate,
    internal_duration: &InternalDuration,
) -> Result<(PlainDate, PlainTime), Thrown> {
    let intermediate_date = plain_relative_date_add(calendar_id, plain_date, &internal_duration.date_duration())?;
    let (overflow_days, subday_ns) = split_time_duration(internal_duration.time());
    let day_duration = Duration::new(0, 0, 0, overflow_days, 0, 0, 0, 0, 0, 0);
    let target_date = plain_relative_date_add(calendar_id, intermediate_date, &day_duration)?;
    Ok((target_date, plain_time_from_subday_ns(subday_ns)))
}

/// `TemporalDuration::round(globalObject, internalDuration, increment, unit, mode)` (estática): o despachante
/// de dia e unidade de tempo do caminho sem `relativeTo`. https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.round
fn round_internal_duration(
    internal_duration: InternalDuration,
    increment: f64,
    unit: TemporalUnit,
    mode: RoundingMode,
) -> Result<InternalDuration, Thrown> {
    debug_assert!(unit >= TemporalUnit::Day);

    // Passo 32: `smallestUnit` é `~day~`.
    if unit == TemporalUnit::Day {
        // Passos 32.a e 32.b sobre o racional exato: passando de 128 dias, um ULP de `double` de dias excede
        // 1 ns em dias, e materializar `fractionalDays` mexeria o valor antes de arredondar.
        let fractional_days_numerator = internal_duration.time();
        let fractional_days_denominator = ExactTime::NS_PER_DAY;
        let rounding_increment_ns = fractional_days_denominator * (increment.trunc() as i128);
        let rounded_ns = round_number_to_increment_i128(fractional_days_numerator, rounding_increment_ns, mode);
        // Passos 32.c e 32.d: a conferência de faixa de dias é de `TemporalDurationFromInternal`.
        let days = Duration::new(0, 0, 0, (rounded_ns / fractional_days_denominator) as i64, 0, 0, 0, 0, 0, 0);
        return Ok(InternalDuration::new(days, 0));
    }

    // Passo 33: unidade de tempo, `RoundTimeDuration` e `CombineDateAndTimeDuration`.
    let time_duration = round_time_duration(internal_duration.time(), increment as u32, unit, mode)?;
    Ok(InternalDuration::new(Duration::default(), time_duration))
}

/// `appendInteger(globalObject, builder, value)`: o valor absoluto de um inteiro como `double`, sempre em
/// dígitos decimais (acima de `maxSafeInteger` o C++ passa por `JSBigInt::createFrom(double)`, que escreve os
/// dígitos exatos do `double`).
fn append_integer(builder: &mut String, value: f64) {
    debug_assert!(value.is_finite());
    builder.push_str(&(value.abs() as i128).to_string());
}

/// `TemporalDurationToString(duration, precision)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-temporaldurationtostring
fn temporal_duration_to_string(duration: &Duration, precision: (Precision, u32)) -> String {
    debug_assert!(precision.0 == Precision::Auto || precision.1 < 10);

    let mut builder = String::new();
    let sign = duration_sign(duration);
    if sign < 0 {
        builder.push('-');
    }

    builder.push('P');
    for (value, designator) in [
        (duration.years(), 'Y'),
        (duration.months(), 'M'),
        (duration.weeks(), 'W'),
        (duration.days(), 'D'),
    ] {
        if value != 0 {
            append_integer(&mut builder, value as f64);
            builder.push(designator);
        }
    }

    let seconds_duration = time_duration_from_components(
        0.0,
        0.0,
        duration.seconds() as f64,
        duration.milliseconds() as f64,
        duration.microseconds() as f64,
        duration.nanoseconds() as f64,
    );

    if duration.hours() == 0 && duration.minutes() == 0 && seconds_duration == 0 && sign != 0 && precision.0 == Precision::Auto {
        return builder;
    }

    builder.push('T');
    if duration.hours() != 0 {
        append_integer(&mut builder, duration.hours() as f64);
        builder.push('H');
    }
    if duration.minutes() != 0 {
        append_integer(&mut builder, duration.minutes() as f64);
        builder.push('M');
    }

    let zero_minutes_and_higher = largest_subduration(duration) >= TemporalUnit::Second;

    if seconds_duration != 0 || zero_minutes_and_higher || precision.0 != Precision::Auto {
        let seconds_part = ((seconds_duration / 1_000_000_000) as i64 as f64).abs();
        let sub_seconds_part = ((seconds_duration % 1_000_000_000) as i64 as f64).abs();
        append_integer(&mut builder, seconds_part);
        format_seconds_string_fraction(&mut builder, sub_seconds_part as u32, precision);
        builder.push('S');
    }

    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    fn duration(years: i64, days: i64, hours: i64, seconds: i64, ms: i64, ns: i128) -> Duration {
        Duration::new(years, 0, 0, days, hours, 0, seconds, ms, 0, ns)
    }

    #[test]
    fn formats_like_the_spec() {
        let auto = (Precision::Auto, 0);
        assert_eq!(temporal_duration_to_string(&Duration::default(), auto), "PT0S");
        assert_eq!(temporal_duration_to_string(&duration(1, 2, 0, 0, 0, 0), auto), "P1Y2D");
        assert_eq!(temporal_duration_to_string(&duration(0, 0, 1, 5, 120, 0), auto), "PT1H5.12S");
        assert_eq!(temporal_duration_to_string(&-duration(0, 0, 0, 0, 0, 1), auto), "-PT0.000000001S");
        assert_eq!(temporal_duration_to_string(&duration(0, 0, 0, 5, 0, 0), (Precision::Fixed, 3)), "PT5.000S");
    }

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn relative_to_string_errors_follow_the_cpp_order() {
        let iso = |year, month, day| PlainRelativeTo { date: PlainDate::new(year, month, day), calendar_id: ISO8601_CALENDAR_ID };
        assert_eq!(validate_relative_to_string(&units("2020-01-01")), Ok(iso(2020, 1, 1)));
        assert_eq!(validate_relative_to_string(&units("2020-01-01T10:00")), Ok(iso(2020, 1, 1)));
        assert_eq!(validate_relative_to_string(&units("nope")), Err(RelativeToStringError::NotValid));
        assert_eq!(validate_relative_to_string(&units("2020-01-01T10:00[UTC]")), Err(RelativeToStringError::TimeZoneUnported));
        assert_eq!(validate_relative_to_string(&units("2020-01-01[u-ca=iso8601]")), Ok(iso(2020, 1, 1)));
        assert_eq!(
            validate_relative_to_string(&units("2020-01-01[u-ca=NoSuch]")),
            Err(RelativeToStringError::InvalidCalendar(units("nosuch")))
        );
        assert_eq!(validate_relative_to_string(&units("+275760-09-30")), Err(RelativeToStringError::OutOfRange));
        // Calendário nativo não ISO passa na validação.
        let gregory = validate_relative_to_string(&units("2020-01-01[u-ca=gregory]")).unwrap();
        assert!(!crate::runtime::temporal_calendar::calendar_is_iso(gregory.calendar_id));
    }

    #[test]
    fn plain_relative_target_adds_dates_then_overflow_days() {
        // 2020-01-31 + P1M (constrain: 02-29) + 1 dia e 6 horas de tempo = 03-01T06:00.
        let internal = InternalDuration::new(Duration::new(0, 1, 0, 0, 0, 0, 0, 0, 0, 0), (24 + 6) * 3_600_000_000_000);
        let (date, time) = compute_plain_relative_target(ISO8601_CALENDAR_ID, PlainDate::new(2020, 1, 31), &internal).unwrap();
        assert_eq!(date, PlainDate::new(2020, 3, 1));
        assert_eq!(time, PlainTime::new(6, 0, 0, 0, 0, 0));
    }

    #[test]
    fn plain_relative_compare_uses_the_calendar_length_of_each_side() {
        // A partir de 2020-01-01, P1M são 31 dias; P2M são 31 + 29.
        let start = PlainDate::new(2020, 1, 1);
        let one_month = Duration::new(0, 1, 0, 0, 0, 0, 0, 0, 0, 0);
        let thirty_one_days = Duration::new(0, 0, 0, 31, 0, 0, 0, 0, 0, 0);
        let thirty_days = Duration::new(0, 0, 0, 30, 0, 0, 0, 0, 0, 0);
        let day_ns = ExactTime::NS_PER_DAY;
        let iso_time_duration = |duration: &Duration, date: PlainDate| plain_relative_time_duration(ISO8601_CALENDAR_ID, duration, date).unwrap();
        assert_eq!(iso_time_duration(&one_month, start), 31 * day_ns);
        assert_eq!(iso_time_duration(&one_month, start), iso_time_duration(&thirty_one_days, start));
        assert!(iso_time_duration(&one_month, start) > iso_time_duration(&thirty_days, start));
        // A partir de 2020-02-01, P1M são 29 dias.
        assert_eq!(iso_time_duration(&one_month, PlainDate::new(2020, 2, 1)), 29 * day_ns);
    }

    #[test]
    fn plain_relative_round_and_total_end_to_end() {
        // P40D relativo a 2020-01-01 com `largestUnit: month`, `smallestUnit: month`, `halfExpand`:
        // alvo 2020-02-10, diferença 1 mês e 9 dias; 9/29 do mês é menos da metade, então 1 mês.
        let start = PlainDate::new(2020, 1, 1);
        let internal = to_internal_duration_record_with_24_hour_days(&Duration::new(0, 0, 0, 40, 0, 0, 0, 0, 0, 0)).unwrap();
        let (target_date, target_time) = compute_plain_relative_target(ISO8601_CALENDAR_ID, start, &internal).unwrap();
        let diff = difference_plain_date_time_with_rounding(
            ISO8601_CALENDAR_ID,
            start,
            PlainTime::default(),
            target_date,
            target_time,
            TemporalUnit::Month,
            TemporalUnit::Month,
            RoundingMode::HalfExpand,
            1.0,
        )
        .unwrap();
        let rounded = temporal_duration_from_internal(&diff, TemporalUnit::Month).unwrap();
        assert_eq!((rounded.months(), rounded.days()), (1, 0));
        let total = difference_plain_date_time_with_total(ISO8601_CALENDAR_ID, start, target_date, target_time, TemporalUnit::Month).unwrap();
        // 1 + 9/29 (de 2020-02-01 a 2020-03-01).
        assert!((total - (1.0 + 9.0 / 29.0)).abs() < 1e-12, "total = {total}");
    }

    #[test]
    fn rounds_internal_durations() {
        let internal = InternalDuration::new(Duration::default(), 90 * 60 * 1_000_000_000);
        let hours = round_internal_duration(internal, 1.0, TemporalUnit::Hour, RoundingMode::HalfExpand).unwrap();
        assert_eq!(hours.time(), 2 * 3_600_000_000_000);
        let days = round_internal_duration(internal, 1.0, TemporalUnit::Day, RoundingMode::Ceil).unwrap();
        assert_eq!(days.date_duration().days(), 1);
    }
}
