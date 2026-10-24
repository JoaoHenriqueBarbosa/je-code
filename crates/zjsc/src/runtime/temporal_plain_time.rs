//! Porte de `runtime/TemporalPlainTime.{h,cpp}`: a célula `Temporal.PlainTime` (`TemporalPlainTime`, um
//! `JSNonFinalObject` com o `ISO8601::PlainTime` em `m_plainTime`), `createTemporalTime`,
//! `validateAndCreateTimeRecord`, `balanceTime`, `roundTime`, `toTemporalTimeRecord`, `toPartialTime`,
//! `regulateTime`, `from` (`ToTemporalTime`), `compare`, `addTime`, `differenceTemporalPlainTime`, `round` e
//! `toString`.
//!
//! DIVERGÊNCIAS:
//! - `toTemporalTimeRecord` e `toPartialTime` repetem o mesmo laço no C++; aqui `to_temporal_time_record` é o
//!   `to_partial_time` com os campos ausentes em zero (a ordem das leituras, os erros e o `any` são os mesmos).
//! - `until` e `since` do C++ só repassam a `differenceTemporalPlainTime` com a operação; o protótipo chama
//!   `difference_temporal_plain_time` direto.
//! - `TemporalPlainTime::from` trata `TemporalZonedDateTime` (passo 2.c) e `TemporalPlainDateTime` (passo 2.b).
//! - `toString(globalObject, options)` devolve `String` do Rust (o resultado é sempre ASCII).
//! - A estrutura intrínseca (`globalObject->plainTimeStructure()`) mora em `TemporalGlobalData`.
//! - `JSC_DEFINE_TEMPORAL_PLAIN_TIME_FIELD` (`hour()`...`nanosecond()`) é o acessor de `PlainTime`, que o
//!   protótipo lê direto de `plain_time()`.

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::intl_support::{get_options_object, to_rust_string};
use crate::runtime::iso8601::{
    parse_iso_date_time, round_time_duration, temporal_time_to_string, Duration, InternalDuration, PlainTime, TemporalProduction,
};
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_core_duration::{temporal_duration_from_internal, time_duration_from_components};
use crate::runtime::temporal_core_rounding::{maximum_rounding_increment, round_number_to_increment_double, validate_temporal_rounding_increment};
use crate::runtime::temporal_object::{
    extract_difference_options, temporal_fractional_second_digits, temporal_rounding_increment, temporal_rounding_mode,
    temporal_unit_singular_property_name, temporal_unit_type, temporal_unit_valued, to_integer_with_truncation, to_seconds_string_precision_record,
    to_temporal_overflow, to_temporal_overflow_value, validate_temporal_unit_value, AllowedUnit, DifferenceOperation, Inclusivity, Precision,
    RoundingMode, TemporalOverflow, TemporalUnit, TemporalUnitDefault, UnitGroup, UnitOption, TEMPORAL_UNITS_IN_TABLE_ORDER,
};
use crate::runtime::temporal_plain_date_time::TemporalPlainDateTime;
use crate::runtime::temporal_zoned_date_time::TemporalZonedDateTime;
use crate::runtime::vm::VM;
use crate::wtf::option_set::OptionSet;

/// `const ClassInfo TemporalPlainTime::s_info` (`"Object"`).
pub static TEMPORAL_PLAIN_TIME_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `numberOfTemporalPlainTimeUnits`: hora, minuto, segundo, milissegundo, microssegundo e nanossegundo.
pub const NUMBER_OF_TEMPORAL_PLAIN_TIME_UNITS: usize = 6;

/// `class TemporalPlainTime final : public JSNonFinalObject`.
pub struct TemporalPlainTime {
    base: JSNonFinalObject,
    /// `m_plainTime`.
    plain_time: PlainTime,
}

/// A referência à célula, o `*` do C++.
pub type TemporalPlainTimeRef = Rc<TemporalPlainTime>;

impl std::ops::Deref for TemporalPlainTime {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSGlobalObject {
    /// `plainTimeStructure()`: o `LazyClassStructure` de `Temporal.PlainTime`.
    pub fn plain_time_structure(&self) -> StructureRef {
        crate::runtime::temporal_object::lazy_temporal_structure(self, "PlainTime", |data| data.plain_time_structure.clone())
    }
}

/// `std::array<std::optional<double>, numberOfTemporalPlainTimeUnits>`: o resultado de `toPartialTime`,
/// indexado por `unit - TemporalUnit::Hour`.
pub type PartialTime = [Option<f64>; NUMBER_OF_TEMPORAL_PLAIN_TIME_UNITS];

/// A posição de uma unidade de tempo em `PartialTime`.
fn partial_time_index(unit: TemporalUnit) -> usize {
    unit as usize - TemporalUnit::Hour as usize
}

/// `balanceTime(hour, minute, second, millisecond, microsecond, nanosecond)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-balancetime
/// O excesso de dias vai para o campo `days` da `Duration` devolvida (o `[[Days]]` do registro).
fn balance_time(mut hour: i128, mut minute: i128, mut second: i128, mut millisecond: i128, mut microsecond: i128, mut nanosecond: i128) -> Duration {
    // Passos 1 e 2: `microsecond += floor(nanosecond / 1000)` e `nanosecond %= 1000`.
    microsecond += nanosecond / 1000;
    nanosecond %= 1000;
    if nanosecond < 0 {
        microsecond -= 1;
        nanosecond += 1000;
    }

    // Passos 3 e 4.
    millisecond += microsecond / 1000;
    microsecond %= 1000;
    if microsecond < 0 {
        millisecond -= 1;
        microsecond += 1000;
    }

    // Passos 5 e 6.
    second += millisecond / 1000;
    millisecond %= 1000;
    if millisecond < 0 {
        second -= 1;
        millisecond += 1000;
    }

    // Passos 7 e 8.
    minute += second / 60;
    second %= 60;
    if second < 0 {
        minute -= 1;
        second += 60;
    }

    // Passos 9 e 10.
    hour += minute / 60;
    minute %= 60;
    if minute < 0 {
        hour -= 1;
        minute += 60;
    }

    // Passos 11 e 12: `deltaDays = floor(hour / 24)` e `hour %= 24`.
    let mut days = hour / 24;
    hour %= 24;
    if hour < 0 {
        days -= 1;
        hour += 24;
    }

    // Passo 13: `CreateTimeRecord`, com `deltaDays` em `[[Days]]`.
    Duration::new(0, 0, 0, days as i64, hour as i64, minute as i64, second as i64, millisecond as i64, microsecond, nanosecond)
}

/// `constrainTime(duration)` (o passo 1 de `RegulateTime`): cada campo preso à sua faixa.
fn constrain_time(duration: &Duration) -> PlainTime {
    PlainTime::new(
        duration.hours().clamp(0, 23) as u32,
        duration.minutes().clamp(0, 59) as u32,
        duration.seconds().clamp(0, 59) as u32,
        duration.milliseconds().clamp(0, 999) as u32,
        duration.microseconds().clamp(0, 999) as u32,
        duration.nanoseconds().clamp(0, 999) as u32,
    )
}

/// `differenceTime(time1, time2)` (`DifferenceTime`): `time2 - time1` em nanossegundos.
/// https://tc39.es/proposal-temporal/#sec-temporal-differencetime
pub fn difference_time(time1: PlainTime, time2: PlainTime) -> i128 {
    // Passos 1 a 6: a diferença campo a campo.
    let difference = |a: u32, b: u32| f64::from(b) - f64::from(a);
    // Passos 7 a 9: `TimeDurationFromComponents` (o `|result| < nsPerDay` do passo 8 vale para registros válidos).
    time_duration_from_components(
        difference(time1.hour(), time2.hour()),
        difference(time1.minute(), time2.minute()),
        difference(time1.second(), time2.second()),
        difference(time1.millisecond(), time2.millisecond()),
        difference(time1.microsecond(), time2.microsecond()),
        difference(time1.nanosecond(), time2.nanosecond()),
    )
}

/// `validateAndCreateTimeRecord(globalObject, duration)` (`IsValidTime` e `CreateTimeRecord`):
/// https://tc39.es/proposal-temporal/#sec-temporal-isvalidtime
pub fn validate_and_create_time_record(duration: &Duration) -> Result<PlainTime, Thrown> {
    let hour = duration.hours() as f64;
    let minute = duration.minutes() as f64;
    let second = duration.seconds() as f64;
    let millisecond = duration.milliseconds() as f64;
    let microsecond = duration.microseconds() as f64;
    let nanosecond = duration.nanoseconds() as f64;
    // Passos 1 a 6: cada campo na sua faixa.
    if !(0.0..=23.0).contains(&hour) {
        return Err(Thrown::range_error("hour is out of range"));
    }
    if !(0.0..=59.0).contains(&minute) {
        return Err(Thrown::range_error("minute is out of range"));
    }
    if !(0.0..=59.0).contains(&second) {
        return Err(Thrown::range_error("second is out of range"));
    }
    if !(0.0..=999.0).contains(&millisecond) {
        return Err(Thrown::range_error("millisecond is out of range"));
    }
    if !(0.0..=999.0).contains(&microsecond) {
        return Err(Thrown::range_error("microsecond is out of range"));
    }
    if !(0.0..=999.0).contains(&nanosecond) {
        return Err(Thrown::range_error("nanosecond is out of range"));
    }
    // Passo 7: o registro com os campos validados.
    Ok(PlainTime::new(hour as u32, minute as u32, second as u32, millisecond as u32, microsecond as u32, nanosecond as u32))
}

/// `createTemporalTime(globalObject, plainTime, newTarget)`: https://tc39.es/proposal-temporal/#sec-temporal-createtemporaltime
/// `new_target` é `(newTarget, constructor)`; ausente é `%Temporal.PlainTime%`.
pub fn create_temporal_time(
    global_object: &JSGlobalObject,
    plain_time: PlainTime,
    new_target: Option<(JSValue, usize)>,
) -> Result<TemporalPlainTimeRef, Thrown> {
    // Passos 1 e 2: `OrdinaryCreateFromConstructor(newTarget, "%Temporal.PlainTime.prototype%", ...)`.
    let structure = match new_target {
        None => global_object.plain_time_structure(),
        Some((new_target, constructor)) => {
            get_derived_structure_in_realm(global_object, new_target, constructor, |realm| realm.plain_time_structure())?
        }
    };

    // Passos 3 e 4.
    Ok(TemporalPlainTime::create(global_object.vm(), &structure, plain_time))
}

impl TemporalPlainTime {
    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(ObjectType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_TIME_S_INFO,
        )
    }

    /// `create(vm, structure, plainTime)`.
    pub fn create(vm: &VM, structure: &StructureRef, plain_time: PlainTime) -> TemporalPlainTimeRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(TemporalPlainTime { base: JSNonFinalObject::new(vm, Rc::clone(structure)), plain_time });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TemporalPlainTime(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<TemporalPlainTime>(value)`.
    pub fn from_value(value: &JSValue) -> Option<TemporalPlainTimeRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::TemporalPlainTime(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `plainTime()`.
    pub fn plain_time(&self) -> PlainTime {
        self.plain_time
    }

    /// `roundTime(plainTime, increment, unit, roundingMode, dayLengthNs)` (`RoundTime`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-roundtime
    /// O C++ usa quantidades fracionárias em `double` (a spec usa inteiros de nanossegundos, o mesmo para a faixa
    /// de um `PlainTime`); `day_length_ns` deixa o `ZonedDateTime` dar um dia de outro tamanho.
    pub fn round_time(
        plain_time: PlainTime,
        increment: f64,
        unit: TemporalUnit,
        rounding_mode: RoundingMode,
        day_length_ns: Option<f64>,
    ) -> Duration {
        let fractional_second = f64::from(plain_time.second())
            + f64::from(plain_time.millisecond()) * 1e-3
            + f64::from(plain_time.microsecond()) * 1e-6
            + f64::from(plain_time.nanosecond()) * 1e-9;
        let hour = i128::from(plain_time.hour());
        let minute = i128::from(plain_time.minute());
        let second = i128::from(plain_time.second());
        let millisecond = i128::from(plain_time.millisecond());
        let microsecond = i128::from(plain_time.microsecond());

        match unit {
            TemporalUnit::Day => {
                // Passos 1 e 9: `quantity` é o tempo inteiro em nanossegundos sobre o tamanho do dia.
                let length = day_length_ns.unwrap_or(8.64 * 1e13);
                let quantity = (((((f64::from(plain_time.hour()) * 60.0 + f64::from(plain_time.minute())) * 60.0 + f64::from(plain_time.second()))
                    * 1000.0
                    + f64::from(plain_time.millisecond()))
                    * 1000.0
                    + f64::from(plain_time.microsecond()))
                    * 1000.0
                    + f64::from(plain_time.nanosecond()))
                    / length;
                let result = round_number_to_increment_double(quantity, increment, rounding_mode);
                debug_assert!(result.is_finite() && result >= i64::MIN as f64 && result <= i64::MAX as f64);
                Duration::new(0, 0, 0, result as i64, 0, 0, 0, 0, 0, 0)
            }
            TemporalUnit::Hour => {
                // Passos 1 e 10: horas fracionárias; `BalanceTime(result, 0, ...)`.
                let quantity = (fractional_second / 60.0 + f64::from(plain_time.minute())) / 60.0 + f64::from(plain_time.hour());
                let result = round_number_to_increment_double(quantity, increment, rounding_mode);
                debug_assert!(result.is_finite());
                balance_time(result as i128, 0, 0, 0, 0, 0)
            }
            TemporalUnit::Minute => {
                // Passos 2 e 11.
                let quantity = fractional_second / 60.0 + f64::from(plain_time.minute());
                let result = round_number_to_increment_double(quantity, increment, rounding_mode);
                debug_assert!(result.is_finite());
                balance_time(hour, result as i128, 0, 0, 0, 0)
            }
            TemporalUnit::Second => {
                // Passos 3 e 12.
                let result = round_number_to_increment_double(fractional_second, increment, rounding_mode);
                debug_assert!(result.is_finite());
                balance_time(hour, minute, result as i128, 0, 0, 0)
            }
            TemporalUnit::Millisecond => {
                // Passos 4 e 13.
                let quantity = f64::from(plain_time.millisecond()) + f64::from(plain_time.microsecond()) * 1e-3 + f64::from(plain_time.nanosecond()) * 1e-6;
                let result = round_number_to_increment_double(quantity, increment, rounding_mode);
                debug_assert!(result.is_finite());
                balance_time(hour, minute, second, result as i128, 0, 0)
            }
            TemporalUnit::Microsecond => {
                // Passos 5 e 14.
                let quantity = f64::from(plain_time.microsecond()) + f64::from(plain_time.nanosecond()) * 1e-3;
                let result = round_number_to_increment_double(quantity, increment, rounding_mode);
                debug_assert!(result.is_finite());
                balance_time(hour, minute, second, millisecond, result as i128, 0)
            }
            TemporalUnit::Nanosecond => {
                // Passos 6 e 16.
                let result = round_number_to_increment_double(f64::from(plain_time.nanosecond()), increment, rounding_mode);
                debug_assert!(result.is_finite());
                balance_time(hour, minute, second, millisecond, microsecond, result as i128)
            }
            TemporalUnit::Year | TemporalUnit::Month | TemporalUnit::Week => unreachable!("RoundTime só recebe dia ou unidade de tempo"),
        }
    }

    /// `round(globalObject, optionsValue)`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.round
    /// A marca e o `roundTo` `undefined` ficam com o chamador.
    pub fn round(&self, global_object: &JSGlobalObject, options_value: JSValue) -> Result<PlainTime, Thrown> {
        let mut options: Option<JSValue> = None;
        let mut smallest: Option<TemporalUnit> = None;
        if options_value.is_string() {
            // Passos 4.a a 4.c: `roundTo` `String` é `{ smallestUnit: <string> }`, decodificado direto.
            let string = to_rust_string(global_object, options_value)?;
            let Some(unit) = temporal_unit_type(&string) else {
                return Err(Thrown::range_error("smallestUnit is an invalid Temporal unit"));
            };

            // Passo 10 (caminho da `String`): `ValidateTemporalUnitValue(smallestUnit, ~time~)` recusa unidade de data.
            if unit <= TemporalUnit::Day {
                return Err(Thrown::range_error("smallestUnit is a disallowed unit"));
            }
            smallest = Some(unit);
        } else {
            // Passo 5: `GetOptionsObject(roundTo)`.
            options = get_options_object(options_value)?;
        }

        // Passo 6: as opções em ordem alfabética. Passo 7: `GetRoundingIncrementOption`.
        let rounding_increment = temporal_rounding_increment(global_object, options)?;
        // Passo 8: `GetRoundingModeOption(roundTo, ~half-expand~)`.
        let rounding_mode = temporal_rounding_mode(global_object, options, RoundingMode::HalfExpand)?;
        if smallest.is_none() {
            // Passo 9: `GetTemporalUnitValuedOption(roundTo, "smallestUnit", ~required~)`.
            let smallest_maybe_auto = temporal_unit_valued(global_object, options, "smallestUnit", TemporalUnitDefault::Required)?;
            // Passo 10 (caminho do objeto): `ValidateTemporalUnitValue(smallestUnit, ~time~)`.
            validate_temporal_unit_value(smallest_maybe_auto, UnitGroup::Time, AllowedUnit::None, "smallestUnit")?;
            let UnitOption::Unit(unit) = smallest_maybe_auto else {
                unreachable!("smallestUnit é obrigatório e `auto` não passa pela validação");
            };
            smallest = Some(unit);
        }
        let smallest_unit = smallest.expect("smallestUnit definido nos dois caminhos");

        // Passos 11 a 13: `MaximumTemporalDurationRoundingIncrement` e `ValidateTemporalRoundingIncrement(..., false)`.
        let maximum = maximum_rounding_increment(smallest_unit).map(f64::from);
        validate_temporal_rounding_increment(rounding_increment, maximum, Inclusivity::Exclusive)?;

        // Passos 14 e 15: `RoundTime` e `! CreateTemporalTime(result)`.
        let duration = TemporalPlainTime::round_time(self.plain_time, rounding_increment, smallest_unit, rounding_mode, None);
        validate_and_create_time_record(&duration)
    }

    /// `toString()`: `TimeRecordToString(this.[[Time]], ~auto~)`.
    pub fn to_string(&self) -> String {
        temporal_time_to_string(self.plain_time, (Precision::Auto, 0))
    }

    /// `toString(globalObject, optionsValue)`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.tostring
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
        // Passo 7: `GetTemporalUnitValuedOption(resolvedOptions, "smallestUnit", ~unset~)`.
        let smallest_unit_result = temporal_unit_valued(global_object, options, "smallestUnit", TemporalUnitDefault::Unset)?;

        // Passo 8: `ValidateTemporalUnitValue(smallestUnit, ~time~)`.
        validate_temporal_unit_value(smallest_unit_result, UnitGroup::Time, AllowedUnit::None, "smallestUnit")?;
        let smallest_unit = match smallest_unit_result {
            UnitOption::Unit(unit) => Some(unit),
            _ => None,
        };
        // Passo 9: `hour` é `RangeError`.
        if smallest_unit == Some(TemporalUnit::Hour) {
            return Err(Thrown::range_error("smallestUnit cannot be \"hour\" for PlainTime.toString"));
        }

        // Passo 10: `ToSecondsStringPrecisionRecord(smallestUnit, digits)`.
        let data = to_seconds_string_precision_record(smallest_unit, digits);

        // Passo 11 (caminho curto): com `auto`, `RoundTime` em nanossegundos com incremento 1 é a identidade.
        if data.precision.0 == Precision::Auto {
            return Ok(self.to_string());
        }

        // Passos 11 e 12: `RoundTime` e `TimeRecordToString`; `[[Days]]` não entra na saída.
        let duration = TemporalPlainTime::round_time(self.plain_time, f64::from(data.increment), data.unit, rounding_mode, None);
        let plain_time = validate_and_create_time_record(&duration)?;
        Ok(temporal_time_to_string(plain_time, data.precision))
    }

    /// `toPartialTime(globalObject, temporalTimeLike, skipRelevantPropertyCheck)` (`ToTemporalTimeRecord` com
    /// `~partial~`): https://tc39.es/proposal-temporal/#sec-temporal-totemporaltimerecord
    pub fn to_partial_time(global_object: &JSGlobalObject, temporal_time_like: JSValue, skip_relevant_property_check: bool) -> Result<PartialTime, Thrown> {
        let vm = global_object.vm();

        // Passos 1 a 3: cada campo `~unset~` e `any` falso.
        let mut has_any_fields = false;
        let mut partial_time: PartialTime = [None; NUMBER_OF_TEMPORAL_PLAIN_TIME_UNITS];
        // Passos 4 a 21: `Get` e `ToIntegerWithTruncation` de cada propriedade em ordem alfabética (a tabela).
        for unit in TEMPORAL_UNITS_IN_TABLE_ORDER {
            if unit < TemporalUnit::Hour {
                continue;
            }
            let value = get_value_property(global_object, temporal_time_like, &temporal_unit_singular_property_name(vm, unit))?;
            if value.is_undefined() {
                continue;
            }

            has_any_fields = true;
            let integer = to_integer_with_truncation(global_object, value)?;
            if !integer.is_finite() {
                return Err(Thrown::range_error("Temporal time properties must be finite"));
            }
            partial_time[partial_time_index(unit)] = Some(integer);
        }
        // Passo 22: sem nenhum campo, `TypeError`.
        if !has_any_fields && !skip_relevant_property_check {
            return Err(Thrown::type_error("Object must contain at least one Temporal time property"));
        }
        // Passo 23.
        Ok(partial_time)
    }

    /// `toTemporalTimeRecord(globalObject, temporalTimeLike, skipRelevantPropertyCheck)` (`ToTemporalTimeRecord` com
    /// `~complete~`): os campos ausentes valem 0.
    pub fn to_temporal_time_record(
        global_object: &JSGlobalObject,
        temporal_time_like: JSValue,
        skip_relevant_property_check: bool,
    ) -> Result<Duration, Thrown> {
        let partial_time = TemporalPlainTime::to_partial_time(global_object, temporal_time_like, skip_relevant_property_check)?;
        let mut duration = Duration::default();
        for unit in TEMPORAL_UNITS_IN_TABLE_ORDER {
            if unit < TemporalUnit::Hour {
                continue;
            }
            if let Some(value) = partial_time[partial_time_index(unit)] {
                duration.set_field(unit, value);
            }
        }
        Ok(duration)
    }

    /// `regulateTime(globalObject, duration, overflow)` (`RegulateTime`): https://tc39.es/proposal-temporal/#sec-temporal-regulatetime
    pub fn regulate_time(duration: &Duration, overflow: TemporalOverflow) -> Result<PlainTime, Thrown> {
        match overflow {
            // Passo 1: `constrain` prende cada campo na sua faixa.
            TemporalOverflow::Constrain => Ok(constrain_time(duration)),
            // Passo 2: `reject` confere `IsValidTime`.
            TemporalOverflow::Reject => validate_and_create_time_record(duration),
        }
    }

    /// `addTime(plainTime, duration)` (`AddTime`): https://tc39.es/proposal-temporal/#sec-temporal-addtime
    /// Soma campo a campo (os de subsegundo podem chegar a `MAX_SAFE_INTEGER`, por isso em `i128`) e balanceia.
    pub fn add_time(plain_time: PlainTime, duration: &Duration) -> Duration {
        balance_time(
            i128::from(plain_time.hour()) + i128::from(duration.hours()),
            i128::from(plain_time.minute()) + i128::from(duration.minutes()),
            i128::from(plain_time.second()) + i128::from(duration.seconds()),
            i128::from(plain_time.millisecond()) + i128::from(duration.milliseconds()),
            i128::from(plain_time.microsecond()) + duration.microseconds(),
            i128::from(plain_time.nanosecond()) + duration.nanoseconds(),
        )
    }

    /// `from(globalObject, item, options)` (`ToTemporalTime`): https://tc39.es/proposal-temporal/#sec-temporal-totemporaltime
    pub fn from(global_object: &JSGlobalObject, item_value: JSValue, options_value: JSValue) -> Result<TemporalPlainTimeRef, Thrown> {
        let vm = global_object.vm();

        // Passo 2: `item` é um objeto.
        if item_value.is_object() {
            // Passo 2.a: `[[InitializedTemporalTime]]`: `GetOptionsObject` e `GetTemporalOverflowOption` (o
            // resultado não é usado), depois `! CreateTemporalTime(item.[[Time]])`.
            if let Some(plain_time) = TemporalPlainTime::from_value(&item_value) {
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainTime::create(vm, &global_object.plain_time_structure(), plain_time.plain_time()));
            }

            // Passo 2.b: `[[InitializedTemporalDateTime]]`: `GetOptionsObject` e `GetTemporalOverflowOption` (o
            // resultado não é usado), depois `! CreateTemporalTime(item.[[ISODateTime]].[[Time]])`.
            if let Some(plain_date_time) = TemporalPlainDateTime::from_value(&item_value) {
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainTime::create(vm, &global_object.plain_time_structure(), plain_date_time.plain_time()));
            }

            // Passo 2.c: `[[InitializedTemporalZonedDateTime]]`: `GetISODateTimeFor`, `GetOptionsObject` e
            // `GetTemporalOverflowOption` (validam), depois `! CreateTemporalTime(isoDateTime.[[Time]])`.
            if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&item_value) {
                let iso_date_time = zoned_date_time.get_local_date_time()?;
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainTime::create(vm, &global_object.plain_time_structure(), iso_date_time.time));
            }

            // Passo 2.d: `ToTemporalTimeRecord(item)`, antes de `GetOptionsObject`: a leitura dos campos é observável.
            let duration = TemporalPlainTime::to_temporal_time_record(global_object, item_value, false)?;

            // Passos 2.e e 2.f: `GetOptionsObject(options)` e `GetTemporalOverflowOption`. `undefined` é
            // `constrain`; o que não é objeto mantém a mensagem exata do C++.
            let overflow = if options_value.is_undefined() {
                TemporalOverflow::Constrain
            } else if !options_value.is_object() {
                return Err(Thrown::type_error("options must be an object"));
            } else {
                to_temporal_overflow(global_object, Some(options_value))?
            };

            // Passo 2.g: `RegulateTime`. Passo 4: `! CreateTemporalTime(result)`.
            let plain_time = TemporalPlainTime::regulate_time(&duration, overflow)?;
            return Ok(TemporalPlainTime::create(vm, &global_object.plain_time_structure(), plain_time));
        }

        // Passo 3.a: o que não é objeto nem `String` é `TypeError`.
        if !item_value.is_string() {
            return Err(Thrown::type_error("can only convert to PlainTime from object or string values"));
        }

        // Passo 3.b: `ParseISODateTime(item, « TemporalTimeString »)`, que aceita a forma só de hora e a de data e hora.
        let units = crate::runtime::temporal_object::string_units(global_object, item_value)?;
        let Some(parsed) = parse_iso_date_time(&units, OptionSet::new(&[TemporalProduction::Time])) else {
            return Err(Thrown::range_error("invalid time string"));
        };
        // Passos 3.c e 3.d: `[[Time]]` nunca é `~start-of-day~` (a gramática exige a hora).
        let plain_time = parsed.time.expect("TemporalTimeString sempre tem hora");

        // Passos 3.f e 3.g: `GetOptionsObject` e `GetTemporalOverflowOption` (o resultado não é usado). Passo 4.
        to_temporal_overflow_value(global_object, options_value)?;
        Ok(TemporalPlainTime::create(vm, &global_object.plain_time_structure(), plain_time))
    }

    /// `compare(t1, t2)` (`CompareTimeRecord`): -1, 0 ou 1. https://tc39.es/proposal-temporal/#sec-temporal-comparetimerecord
    pub fn compare(t1: PlainTime, t2: PlainTime) -> i32 {
        let key = |time: PlainTime| (time.hour(), time.minute(), time.second(), time.millisecond(), time.microsecond(), time.nanosecond());
        key(t1).cmp(&key(t2)) as i32
    }

    /// `differenceTemporalPlainTime(operation, globalObject, other, optionsValue)` (`until` e `since`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-differencetemporalplaintime
    /// `other` já passou por `ToTemporalTime` (passo 1, do chamador).
    pub fn difference_temporal_plain_time(
        &self,
        operation: DifferenceOperation,
        global_object: &JSGlobalObject,
        other: &TemporalPlainTime,
        options_value: JSValue,
    ) -> Result<Duration, Thrown> {
        // Passos 2 e 3: `GetOptionsObject` e `GetDifferenceSettings`. A operação passada é sempre `until`: o modo
        // de arredondamento não é negado para `since`, porque a troca dos operandos abaixo já cuida do sinal.
        let (smallest_unit, largest_unit, rounding_mode, increment) = extract_difference_options(
            global_object,
            options_value,
            UnitGroup::Time,
            TemporalUnit::Nanosecond,
            TemporalUnit::Hour,
            DifferenceOperation::Until,
        )?;

        // Passos 4, 5 e 8 fundidos: `since` troca os operandos (o magnitude arredondada já sai com o sinal certo,
        // por `round(-x, mode) = -round(x, NegateRoundingMode(mode))`).
        let time_duration = if operation == DifferenceOperation::Since {
            difference_time(other.plain_time, self.plain_time)
        } else {
            difference_time(self.plain_time, other.plain_time)
        };

        // Passo 5 (cont.): `RoundTimeDuration(timeDuration, increment, smallestUnit, roundingMode)`.
        let rounded = round_time_duration(time_duration, increment as u32, smallest_unit, rounding_mode)?;
        // Passo 6: `CombineDateAndTimeDuration(ZeroDateDuration(), timeDuration)`.
        let duration = InternalDuration::new(Duration::default(), rounded);
        // Passos 7 e 9: `! TemporalDurationFromInternal(duration, largestUnit)` (o passo 8 está na troca acima).
        Ok(temporal_duration_from_internal(&duration, largest_unit)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(hour: u32, minute: u32, second: u32, millisecond: u32, microsecond: u32, nanosecond: u32) -> PlainTime {
        PlainTime::new(hour, minute, second, millisecond, microsecond, nanosecond)
    }

    #[test]
    fn balance_carries_and_borrows() {
        let result = balance_time(25, 61, 61, 1001, 1001, 1001);
        assert_eq!((result.days(), result.hours(), result.minutes(), result.seconds()), (1, 2, 2, 2));
        assert_eq!((result.milliseconds(), result.microseconds(), result.nanoseconds()), (2, 2, 1));
        let result = balance_time(0, 0, 0, 0, 0, -1);
        assert_eq!((result.days(), result.hours(), result.minutes(), result.seconds()), (-1, 23, 59, 59));
        assert_eq!((result.milliseconds(), result.microseconds(), result.nanoseconds()), (999, 999, 999));
    }

    #[test]
    fn rounds_to_units() {
        let result = TemporalPlainTime::round_time(time(14, 30, 15, 0, 0, 0), 1.0, TemporalUnit::Minute, RoundingMode::HalfExpand, None);
        assert_eq!((result.days(), result.hours(), result.minutes(), result.seconds()), (0, 14, 30, 0));
        let result = TemporalPlainTime::round_time(time(23, 45, 0, 0, 0, 0), 1.0, TemporalUnit::Hour, RoundingMode::HalfExpand, None);
        assert_eq!((result.days(), result.hours()), (1, 0));
        let result = TemporalPlainTime::round_time(time(1, 2, 3, 500, 0, 0), 1.0, TemporalUnit::Second, RoundingMode::HalfEven, None);
        assert_eq!(result.seconds(), 4);
        let result = TemporalPlainTime::round_time(time(1, 2, 3, 0, 0, 7), 5.0, TemporalUnit::Nanosecond, RoundingMode::Ceil, None);
        assert_eq!(result.nanoseconds(), 10);
        let result = TemporalPlainTime::round_time(time(18, 0, 0, 0, 0, 0), 1.0, TemporalUnit::Day, RoundingMode::HalfExpand, None);
        assert_eq!(result.days(), 1);
    }

    #[test]
    fn validates_and_regulates() {
        let mut duration = Duration::default();
        duration.set_field(TemporalUnit::Hour, 24.0);
        assert_eq!(validate_and_create_time_record(&duration), Err(Thrown::range_error("hour is out of range")));
        duration.set_field(TemporalUnit::Minute, 99.0);
        assert_eq!(TemporalPlainTime::regulate_time(&duration, TemporalOverflow::Constrain), Ok(time(23, 59, 0, 0, 0, 0)));
        assert!(TemporalPlainTime::regulate_time(&duration, TemporalOverflow::Reject).is_err());
    }

    #[test]
    fn compares_and_adds() {
        assert_eq!(TemporalPlainTime::compare(time(1, 0, 0, 0, 0, 0), time(1, 0, 0, 0, 0, 1)), -1);
        assert_eq!(TemporalPlainTime::compare(time(2, 0, 0, 0, 0, 0), time(1, 59, 59, 999, 999, 999)), 1);
        assert_eq!(TemporalPlainTime::compare(time(3, 4, 5, 6, 7, 8), time(3, 4, 5, 6, 7, 8)), 0);
        let mut duration = Duration::default();
        duration.set_field(TemporalUnit::Hour, -2.0);
        let result = TemporalPlainTime::add_time(time(1, 0, 0, 0, 0, 0), &duration);
        assert_eq!((result.days(), result.hours()), (-1, 23));
    }

    #[test]
    fn differences_are_signed_nanoseconds() {
        assert_eq!(difference_time(time(1, 0, 0, 0, 0, 0), time(2, 30, 0, 0, 0, 0)), 5_400_000_000_000);
        assert_eq!(difference_time(time(2, 0, 0, 0, 0, 0), time(1, 0, 0, 0, 0, 1)), -3_599_999_999_999);
    }
}
