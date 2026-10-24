//! Porte de `runtime/TemporalPlainMonthDay.{h,cpp}`: a célula `Temporal.PlainMonthDay`
//! (`TemporalPlainMonthDay`, um `JSNonFinalObject` com o `ISO8601::PlainMonthDay` em `m_plainMonthDay` e o
//! `CalendarID` em `m_calendarID`), `createTemporalMonthDay`, `toString` e `from` (`ToTemporalMonthDay`).
//!
//! DIVERGÊNCIAS:
//! - O calendário não ISO vale na criação (`new` guarda os campos ISO do usuário, `from` com objeto guarda a data
//!   de referência até 1972-12-31); `from` com `String` e calendário não ISO usa `plainMonthDayFromISODate`. No
//!   calendário ISO o ano guardado é sempre o de referência 1972 (no construtor, o `referenceISOYear` do usuário).
//!   Os dois `create` do C++ (intrínseco e com `TemporalNewTarget`) são um só, com `new_target` opcional.
//! - `from` com `PlainMonthDay` e com objeto de propriedades cria a célula sem repetir as conferências de
//!   `createTemporalMonthDay`, como o C++.

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::intl_support::{get_options_object, IntlEnum};
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::iso8601::{
    is_date_time_within_limits, is_valid_iso_date, parse_iso_date_time, temporal_month_day_to_string, PlainDate, PlainMonthDay,
    TemporalProduction,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{
    get_temporal_calendar_identifier_with_iso_default, calendar_is_iso, is_builtin_calendar, read_calendar_fields_from_object,
    temporal_show_calendar_name, CalendarID, FieldSetType, ISO8601_CALENDAR_ID,
};
use crate::runtime::temporal_calendar_icu::{calendar_fields, CalendarDateFields};
use crate::runtime::temporal_core_calendar_fields::{
    month_day_from_fields, plain_month_day_from_iso_date, ISO_MONTH_DAY_REFERENCE_LEAP_YEAR,
};
use crate::runtime::temporal_object::{
    string_units, throw_range_error_with_units, to_temporal_overflow_after_fields, to_temporal_overflow_value, TemporalOverflow,
};
use crate::runtime::vm::VM;
use crate::wtf::option_set::OptionSet;

/// `const ClassInfo TemporalPlainMonthDay::s_info` (`"Object"`).
pub static TEMPORAL_PLAIN_MONTH_DAY_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class TemporalPlainMonthDay final : public JSNonFinalObject`.
pub struct TemporalPlainMonthDay {
    base: JSNonFinalObject,
    /// `m_plainMonthDay`.
    plain_month_day: PlainMonthDay,
    /// `m_calendarID`.
    calendar_id: CalendarID,
}

/// A referência à célula, o `*` do C++.
pub type TemporalPlainMonthDayRef = Rc<TemporalPlainMonthDay>;

impl std::ops::Deref for TemporalPlainMonthDay {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSGlobalObject {
    /// `plainMonthDayStructure()`: o `LazyClassStructure` de `Temporal.PlainMonthDay`.
    pub fn plain_month_day_structure(&self) -> StructureRef {
        crate::runtime::temporal_object::lazy_temporal_structure(self, "PlainMonthDay", |data| data.plain_month_day_structure.clone())
    }
}

impl TemporalPlainMonthDay {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainMonthDay::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_MONTH_DAY_S_INFO,
        )
    }

    /// `create(vm, structure, plainMonthDay)` (e o `finishCreation`) com o `setCalendarID`: a célula e o registro
    /// dela.
    pub fn create(vm: &VM, structure: &StructureRef, plain_month_day: PlainMonthDay, calendar_id: CalendarID) -> TemporalPlainMonthDayRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(TemporalPlainMonthDay { base: JSNonFinalObject::new(vm, Rc::clone(structure)), plain_month_day, calendar_id });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TemporalPlainMonthDay(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<TemporalPlainMonthDay>(value)`.
    pub fn from_value(value: &JSValue) -> Option<TemporalPlainMonthDayRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::TemporalPlainMonthDay(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `plainMonthDay()`.
    pub fn plain_month_day(&self) -> PlainMonthDay {
        self.plain_month_day
    }

    /// `calendarID()`.
    pub fn calendar_id(&self) -> CalendarID {
        self.calendar_id
    }

    /// Os campos de calendário (`monthCode`, `day`) da data de referência guardada, pelo despacho único de
    /// `temporal_calendar_icu.rs`.
    pub fn calendar_date_fields(&self) -> CalendarDateFields {
        calendar_fields(self.calendar_id, self.plain_month_day.iso_plain_date())
    }

    /// `month()`.
    pub fn month(&self) -> u8 {
        self.plain_month_day.month()
    }

    /// `day()`.
    pub fn day(&self) -> u32 {
        self.plain_month_day.day()
    }

    /// `toString()`: `temporalMonthDayToString(m_plainMonthDay, "auto", m_calendarID)`.
    pub fn to_string(&self) -> String {
        temporal_month_day_to_string(self.plain_month_day, "auto", self.calendar_id)
    }

    /// `toString(globalObject, optionsValue)` do protótipo (passos 3 a 5): `GetOptionsObject` e
    /// `GetTemporalShowCalendarNameOption`.
    pub fn to_string_with_options(&self, global_object: &JSGlobalObject, options_value: JSValue) -> Result<String, Thrown> {
        let Some(options) = get_options_object(options_value)? else {
            return Ok(self.to_string());
        };
        let calendar_name = temporal_show_calendar_name(global_object, Some(options))?;
        Ok(temporal_month_day_to_string(self.plain_month_day, calendar_name.as_str(), self.calendar_id))
    }

    /// `from(globalObject, itemValue, optionsValue)` (`ToTemporalMonthDay`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-totemporalmonthday
    /// Sempre uma célula nova, mesmo quando `item` já é um `PlainMonthDay`.
    pub fn from(global_object: &JSGlobalObject, item_value: JSValue, options_value: JSValue) -> Result<TemporalPlainMonthDayRef, Thrown> {
        let vm = global_object.vm();

        // Passos 4 a 14 (a `String`) antes do objeto, para o `RangeError` do parse preceder o `TypeError` das opções.
        if item_value.is_string() {
            // Passo 4: `ParseISODateTime(item, « TemporalMonthDayString »)`.
            let string = string_units(global_object, item_value)?;
            let Some(parsed) = parse_iso_date_time(&string, OptionSet::new(&[TemporalProduction::MonthDay])) else {
                return Err(throw_range_error_with_units(global_object, "Temporal.PlainMonthDay.from: invalid date string ", &string, ""));
            };
            let plain_date = parsed.date.expect("TemporalMonthDayString sempre tem data");

            // Passos 5 a 7: `calendar = result.[[Calendar]]`, `~empty~` vira `"iso8601"`, e `CanonicalizeCalendar`. O
            // `parseISODateTime` já recusou a forma curta com calendário diferente de `iso8601` (passo 4.a.ii.4).
            let mut calendar_id = ISO8601_CALENDAR_ID;
            if let Some(calendar) = parsed.calendar {
                let raw_calendar: Vec<u16> = calendar.to_ascii_lowercase().into_iter().map(u16::from).collect();
                let Some(canonicalized) = is_builtin_calendar(&raw_calendar) else {
                    return Err(throw_range_error_with_units(global_object, "'", &raw_calendar, "' is not a valid calendar identifier"));
                };
                calendar_id = canonicalized;
            }

            // Passos 8 e 9: `GetOptionsObject` e `GetTemporalOverflowOption` (validam, o resultado não serve para
            // `String`).
            to_temporal_overflow_value(global_object, options_value)?;

            // Passo 10 (calendário ISO): o ano de referência 1972 e `CreateTemporalMonthDay`.
            if calendar_is_iso(calendar_id) {
                return create_temporal_month_day(
                    global_object,
                    PlainDate::new(i64::from(ISO_MONTH_DAY_REFERENCE_LEAP_YEAR), u32::from(plain_date.month()), u32::from(plain_date.day())),
                    calendar_id,
                    None,
                );
            }
            // Passos 11 e 12 (não ISO): `ISODateWithinLimits` da data completa.
            if !is_date_time_within_limits(plain_date.year(), plain_date.month(), plain_date.day(), 12, 0, 0, 0, 0, 0) {
                return Err(Thrown::range_error("Date is not within ISO date time limits"));
            }
            // Passos 13 a 15: `plainMonthDayFromISODate` (`ISODateToFields` e `CalendarMonthDayFromFields(~constrain~)`).
            let resolved = plain_month_day_from_iso_date(calendar_id, plain_date, TemporalOverflow::Constrain)?;
            return create_temporal_month_day(global_object, resolved.iso_date, resolved.calendar_id, None);
        }

        // Passo 2: se `item` é um objeto.
        if item_value.is_object() {
            // Passo 2.a: `[[InitializedTemporalMonthDay]]`: valida as opções (o resultado não serve) e devolve uma
            // instância nova com os mesmos campos.
            if let Some(existing) = TemporalPlainMonthDay::from_value(&item_value) {
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainMonthDay::create(
                    vm,
                    &global_object.plain_month_day_structure(),
                    existing.plain_month_day,
                    existing.calendar_id,
                ));
            }

            // Passo 2.b: `calendar = ? GetTemporalCalendarIdentifierWithISODefault(item)`.
            let calendar_id = get_temporal_calendar_identifier_with_iso_default(global_object, item_value)?;

            // Passo 2.c: `fields = ? PrepareCalendarFields(calendar, item, «year, month, monthCode, day», «», «»)`.
            let fields = read_calendar_fields_from_object(global_object, item_value, calendar_id, FieldSetType::MonthDay, None)?;

            // Passos 2.d e 2.e: `GetOptionsObject` e `GetTemporalOverflowOption`, depois dos campos.
            let overflow = to_temporal_overflow_after_fields(global_object, options_value)?;

            // Passos 2.f e 2.g: `CalendarMonthDayFromFields` e `CreateTemporalMonthDay(isoDate, calendar)`.
            let resolved = month_day_from_fields(calendar_id, &fields, overflow)?;
            return Ok(TemporalPlainMonthDay::create(
                vm,
                &global_object.plain_month_day_structure(),
                PlainMonthDay::from_date(resolved.iso_date),
                resolved.calendar_id,
            ));
        }

        // Passo 3: quem não é objeto nem `String` é `TypeError`.
        Err(Thrown::type_error("can only convert to PlainMonthDay from object or string values"))
    }
}

/// `createTemporalMonthDay(globalObject, plainDate, calendarID[, newTarget])`:
/// https://tc39.es/proposal-temporal/#sec-temporal-createtemporalmonthday
/// `new_target` é `Some((newTarget, jsCallee))` na construção e `None` na criação intrínseca.
pub fn create_temporal_month_day(
    global_object: &JSGlobalObject,
    plain_date: PlainDate,
    calendar_id: CalendarID,
    new_target: Option<(JSValue, usize)>,
) -> Result<TemporalPlainMonthDayRef, Thrown> {
    // Fora dos passos da spec: `PlainDate` não valida mês e dia, e `daysInMonth()` indexa a tabela de 12 entradas.
    if !is_valid_iso_date(f64::from(plain_date.year()), f64::from(plain_date.month()), f64::from(plain_date.day())) {
        return Err(Thrown::range_error("PlainMonthDay: invalid date"));
    }

    // Passo 1: `ISODateWithinLimits(isoDate)` falso é `RangeError`.
    if !is_date_time_within_limits(plain_date.year(), plain_date.month(), plain_date.day(), 12, 0, 0, 0, 0, 0) {
        return Err(Thrown::range_error("PlainMonthDay: date out of range of ECMAScript representation"));
    }

    // Passos 2 e 3: `newTarget` ausente é `%Temporal.PlainMonthDay%`; `OrdinaryCreateFromConstructor`.
    let structure = match new_target {
        None => global_object.plain_month_day_structure(),
        Some((new_target, constructor)) => {
            get_derived_structure_in_realm(global_object, new_target, constructor, |realm| realm.plain_month_day_structure())?
        }
    };

    // Passos 4 a 6: `[[ISODate]]` e `[[Calendar]]`.
    Ok(TemporalPlainMonthDay::create(global_object.vm(), &structure, PlainMonthDay::from_date(plain_date), calendar_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_day_to_string_follows_calendar_name() {
        let month_day = PlainMonthDay::from_date(PlainDate::new(1972, 2, 29));
        assert_eq!(temporal_month_day_to_string(month_day, "auto", ISO8601_CALENDAR_ID), "02-29");
        assert_eq!(temporal_month_day_to_string(month_day, "never", ISO8601_CALENDAR_ID), "02-29");
        assert_eq!(temporal_month_day_to_string(month_day, "always", ISO8601_CALENDAR_ID), "1972-02-29[u-ca=iso8601]");
        assert_eq!(temporal_month_day_to_string(month_day, "critical", ISO8601_CALENDAR_ID), "1972-02-29[!u-ca=iso8601]");
        // Calendário não ISO: a data ISO completa, com a anotação em `auto` e sem ela em `never`.
        let hebrew = 7;
        assert_eq!(temporal_month_day_to_string(month_day, "auto", hebrew), "1972-02-29[u-ca=hebrew]");
        assert_eq!(temporal_month_day_to_string(month_day, "never", hebrew), "1972-02-29");
    }

    #[test]
    fn month_day_new_uses_year_two_and_pads() {
        let month_day = PlainMonthDay::new(1, 5);
        assert_eq!((month_day.month(), month_day.day()), (1, 5));
        assert_eq!(temporal_month_day_to_string(month_day, "auto", ISO8601_CALENDAR_ID), "01-05");
    }

    #[test]
    fn invalid_month_day_dates_are_rejected_by_the_validity_check() {
        // 30 de fevereiro nunca existe, nem no ano de referência bissexto.
        assert!(!is_valid_iso_date(1972.0, 2.0, 30.0));
        assert!(is_valid_iso_date(1972.0, 2.0, 29.0));
        assert!(!is_valid_iso_date(1972.0, 13.0, 1.0));
    }
}
