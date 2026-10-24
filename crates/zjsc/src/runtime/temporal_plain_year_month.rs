//! Porte de `runtime/TemporalPlainYearMonth.{h,cpp}`: a célula `Temporal.PlainYearMonth`
//! (`TemporalPlainYearMonth`, um `JSNonFinalObject` com o `ISO8601::PlainYearMonth` em `m_plainYearMonth` e o
//! `CalendarID` em `m_calendarID`), `createTemporalYearMonth`, `toString` e `from` (`ToTemporalYearMonth`).
//!
//! DIVERGÊNCIAS:
//! - O calendário não ISO vale na criação (`new` com calendário guarda os campos ISO, `from` com objeto guarda o
//!   primeiro dia do mês do calendário); `from` com `String` e calendário não ISO usa
//!   `plainYearMonthFromISODate`. Os dois `create` do C++ (intrínseco e com `TemporalNewTarget`) são um só, com `new_target`
//!   opcional.
//! - O C++ guarda a `PlainYearMonth` e a data ISO completa no mesmo campo; aqui também (`PlainYearMonth` é uma
//!   `PlainDate` com dia 1 em `iso8601.rs`).
//! - `from` com `PlainYearMonth` e com objeto de propriedades cria a célula sem repetir `ISOYearMonthWithinLimits`,
//!   como o C++ (`TemporalPlainYearMonth::create` direto, e `yearMonthFromFields` já conferiu a faixa).

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::intl_support::{get_options_object, IntlEnum};
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::iso8601::{
    is_year_month_within_limits, parse_iso_date_time, temporal_year_month_to_string, PlainDate, PlainYearMonth, TemporalProduction,
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
use crate::runtime::temporal_core_calendar_fields::{plain_year_month_from_iso_date, year_month_from_fields};
use crate::runtime::temporal_object::{string_units, throw_range_error_with_units, to_temporal_overflow_after_fields, to_temporal_overflow_value};
use crate::runtime::vm::VM;
use crate::wtf::option_set::OptionSet;

/// `const ClassInfo TemporalPlainYearMonth::s_info` (`"Object"`).
pub static TEMPORAL_PLAIN_YEAR_MONTH_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class TemporalPlainYearMonth final : public JSNonFinalObject`.
pub struct TemporalPlainYearMonth {
    base: JSNonFinalObject,
    /// `m_plainYearMonth`.
    plain_year_month: PlainYearMonth,
    /// `m_calendarID`.
    calendar_id: CalendarID,
}

/// A referência à célula, o `*` do C++.
pub type TemporalPlainYearMonthRef = Rc<TemporalPlainYearMonth>;

impl std::ops::Deref for TemporalPlainYearMonth {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSGlobalObject {
    /// `plainYearMonthStructure()`: o `LazyClassStructure` de `Temporal.PlainYearMonth`.
    pub fn plain_year_month_structure(&self) -> StructureRef {
        crate::runtime::temporal_object::lazy_temporal_structure(self, "PlainYearMonth", |data| data.plain_year_month_structure.clone())
    }
}

impl TemporalPlainYearMonth {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainYearMonth::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_YEAR_MONTH_S_INFO,
        )
    }

    /// `create(vm, structure, plainYearMonth)` (e o `finishCreation`) com o `setCalendarID`: a célula e o registro
    /// dela.
    pub fn create(vm: &VM, structure: &StructureRef, plain_year_month: PlainYearMonth, calendar_id: CalendarID) -> TemporalPlainYearMonthRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(TemporalPlainYearMonth { base: JSNonFinalObject::new(vm, Rc::clone(structure)), plain_year_month, calendar_id });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TemporalPlainYearMonth(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<TemporalPlainYearMonth>(value)`.
    pub fn from_value(value: &JSValue) -> Option<TemporalPlainYearMonthRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::TemporalPlainYearMonth(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `plainYearMonth()`.
    pub fn plain_year_month(&self) -> PlainYearMonth {
        self.plain_year_month
    }

    /// `calendarID()`.
    pub fn calendar_id(&self) -> CalendarID {
        self.calendar_id
    }

    /// Os campos de calendário (`year`, `month`, `monthCode`, `daysInMonth`, `era`...) da data guardada, pelo
    /// despacho único de `temporal_calendar_icu.rs`.
    pub fn calendar_date_fields(&self) -> CalendarDateFields {
        calendar_fields(self.calendar_id, self.plain_year_month.iso_plain_date())
    }

    /// `year()` (o campo `JSC_TEMPORAL_PLAIN_YEAR_MONTH_UNITS`).
    pub fn year(&self) -> i32 {
        self.plain_year_month.year()
    }

    /// `month()`.
    pub fn month(&self) -> u8 {
        self.plain_year_month.month()
    }

    /// `toString()`: `temporalYearMonthToString(m_plainYearMonth, "auto", m_calendarID)`.
    pub fn to_string(&self) -> String {
        temporal_year_month_to_string(self.plain_year_month, "auto", self.calendar_id)
    }

    /// `toString(globalObject, optionsValue)`: `GetOptionsObject` e `GetTemporalShowCalendarNameOption`.
    pub fn to_string_with_options(&self, global_object: &JSGlobalObject, options_value: JSValue) -> Result<String, Thrown> {
        let Some(options) = get_options_object(options_value)? else {
            return Ok(self.to_string());
        };
        let calendar_name = temporal_show_calendar_name(global_object, Some(options))?;
        Ok(temporal_year_month_to_string(self.plain_year_month, calendar_name.as_str(), self.calendar_id))
    }

    /// `from(globalObject, item, optionsValue)` (`ToTemporalYearMonth`):
    /// https://tc39.es/proposal-temporal/#sec-temporal-totemporalyearmonth
    /// Sempre uma célula nova, mesmo quando `item` já é um `PlainYearMonth`.
    pub fn from(global_object: &JSGlobalObject, item: JSValue, options_value: JSValue) -> Result<TemporalPlainYearMonthRef, Thrown> {
        let vm = global_object.vm();

        // Passos 2 e 3, na ordem do C++ (`String` antes de objeto: um valor não é os dois, sem efeito observável).
        if item.is_string() {
            // Passo 4: `ParseISODateTime(item, « TemporalYearMonthString »)`.
            let string = string_units(global_object, item)?;
            let Some(parsed) = parse_iso_date_time(&string, OptionSet::new(&[TemporalProduction::YearMonth])) else {
                return Err(throw_range_error_with_units(global_object, "Temporal.PlainYearMonth.from: invalid year-month string ", &string, ""));
            };
            let plain_date = parsed.date.expect("TemporalYearMonthString sempre tem data");

            // Passos 5 a 7: `calendar = result.[[Calendar]]`, `~empty~` vira `"iso8601"`, e `CanonicalizeCalendar`. O
            // `parseISODateTime` já recusou a forma curta com calendário diferente de `iso8601` (passo 4.a.ii.3).
            let mut calendar_id = ISO8601_CALENDAR_ID;
            if let Some(calendar) = parsed.calendar {
                let raw_calendar: Vec<u16> = calendar.to_ascii_lowercase().into_iter().map(u16::from).collect();
                let Some(canonicalized) = is_builtin_calendar(&raw_calendar) else {
                    return Err(throw_range_error_with_units(global_object, "'", &raw_calendar, "' is not a valid calendar identifier"));
                };
                calendar_id = canonicalized;
            }

            // Passos 16 e 17: `GetOptionsObject` e `GetTemporalOverflowOption` (validam, o resultado não serve para
            // `String`: ela dá uma data ISO sem ambiguidade).
            to_temporal_overflow_value(global_object, options_value)?;

            // Passos 10 a 15: o dia 1 e `CreateTemporalYearMonth`; no calendário não ISO,
            // `plainYearMonthFromISODate` (`ISODateToFields` e `CalendarYearMonthFromFields(~constrain~)`).
            if !calendar_is_iso(calendar_id) {
                let resolved = plain_year_month_from_iso_date(calendar_id, plain_date)?;
                return create_temporal_year_month(global_object, resolved.iso_date, resolved.calendar_id, None);
            }
            return create_temporal_year_month(
                global_object,
                PlainDate::new(i64::from(plain_date.year()), u32::from(plain_date.month()), 1),
                calendar_id,
                None,
            );
        }

        // Passo 2: se `item` é um objeto.
        if item.is_object() {
            // Passo 2.a: `[[InitializedTemporalYearMonth]]`: valida as opções (o resultado não serve) e devolve uma
            // instância nova com os mesmos campos.
            if let Some(existing) = TemporalPlainYearMonth::from_value(&item) {
                to_temporal_overflow_value(global_object, options_value)?;
                return Ok(TemporalPlainYearMonth::create(
                    vm,
                    &global_object.plain_year_month_structure(),
                    existing.plain_year_month,
                    existing.calendar_id,
                ));
            }

            // Passo 2.b: `calendar = ? GetTemporalCalendarIdentifierWithISODefault(item)`.
            let calendar_id = get_temporal_calendar_identifier_with_iso_default(global_object, item)?;

            // Passo 2.c: `fields = ? PrepareCalendarFields(calendar, item, «year, month, monthCode», «», «»)`.
            let fields = read_calendar_fields_from_object(global_object, item, calendar_id, FieldSetType::YearMonth, None)?;

            // Passos 2.d e 2.e: `GetOptionsObject` e `GetTemporalOverflowOption`, depois dos campos.
            let overflow = to_temporal_overflow_after_fields(global_object, options_value)?;

            // Passo 2.f: `isoDate = ? CalendarYearMonthFromFields(calendar, fields, overflow)`.
            let resolved = year_month_from_fields(calendar_id, &fields, overflow)?;

            // Passo 2.g: `CreateTemporalYearMonth(isoDate, calendar)`.
            return Ok(TemporalPlainYearMonth::create(
                vm,
                &global_object.plain_year_month_structure(),
                PlainYearMonth::from_date(resolved.iso_date),
                resolved.calendar_id,
            ));
        }

        // Passo 3: quem não é objeto nem `String` é `TypeError`.
        Err(Thrown::type_error("can only convert to PlainYearMonth from object or string values"))
    }
}

/// `createTemporalYearMonth(globalObject, plainDate, calendarID[, newTarget])`:
/// https://tc39.es/proposal-temporal/#sec-temporal-createtemporalyearmonth
/// `new_target` é `Some((newTarget, jsCallee))` na construção e `None` na criação intrínseca.
pub fn create_temporal_year_month(
    global_object: &JSGlobalObject,
    plain_date: PlainDate,
    calendar_id: CalendarID,
    new_target: Option<(JSValue, usize)>,
) -> Result<TemporalPlainYearMonthRef, Thrown> {
    // Passo 1: `ISOYearMonthWithinLimits(isoDate)` falso é `RangeError`.
    if !is_year_month_within_limits(plain_date.year(), i32::from(plain_date.month())) {
        return Err(Thrown::range_error("PlainYearMonth is out of range of ECMAScript representation"));
    }

    // Passos 2 e 3: `newTarget` ausente é `%Temporal.PlainYearMonth%`; `OrdinaryCreateFromConstructor`.
    let structure = match new_target {
        None => global_object.plain_year_month_structure(),
        Some((new_target, constructor)) => {
            get_derived_structure_in_realm(global_object, new_target, constructor, |realm| realm.plain_year_month_structure())?
        }
    };

    // Passos 4 a 6: `[[ISODate]]` e `[[Calendar]]`.
    Ok(TemporalPlainYearMonth::create(global_object.vm(), &structure, PlainYearMonth::from_date(plain_date), calendar_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_month_to_string_follows_calendar_name() {
        let year_month = PlainYearMonth::new(2020, 3);
        assert_eq!(temporal_year_month_to_string(year_month, "auto", ISO8601_CALENDAR_ID), "2020-03");
        assert_eq!(temporal_year_month_to_string(year_month, "never", ISO8601_CALENDAR_ID), "2020-03");
        assert_eq!(temporal_year_month_to_string(year_month, "always", ISO8601_CALENDAR_ID), "2020-03-01[u-ca=iso8601]");
        assert_eq!(temporal_year_month_to_string(year_month, "critical", ISO8601_CALENDAR_ID), "2020-03-01[!u-ca=iso8601]");
        // Calendário não ISO: a data ISO completa, com a anotação em `auto` e sem ela em `never`.
        let gregory = 6;
        assert_eq!(temporal_year_month_to_string(year_month, "auto", gregory), "2020-03-01[u-ca=gregory]");
        assert_eq!(temporal_year_month_to_string(year_month, "never", gregory), "2020-03-01");
    }

    #[test]
    fn year_month_to_string_uses_six_digit_years_outside_four_digits() {
        assert_eq!(temporal_year_month_to_string(PlainYearMonth::new(-1, 12), "auto", ISO8601_CALENDAR_ID), "-000001-12");
        assert_eq!(temporal_year_month_to_string(PlainYearMonth::new(10000, 1), "auto", ISO8601_CALENDAR_ID), "+010000-01");
        assert_eq!(temporal_year_month_to_string(PlainYearMonth::new(0, 1), "auto", ISO8601_CALENDAR_ID), "0000-01");
    }

    #[test]
    fn year_month_limits() {
        assert!(is_year_month_within_limits(-271821, 4));
        assert!(!is_year_month_within_limits(-271821, 3));
        assert!(is_year_month_within_limits(275760, 9));
        assert!(!is_year_month_within_limits(275760, 10));
    }
}
