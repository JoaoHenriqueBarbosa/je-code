//! Porte de `runtime/TemporalObject.{h,cpp}` e de `runtime/temporal/core/TemporalEnums.h`: o namespace
//! `Temporal` (com `@@toStringTag`), os enums de unidade, arredondamento e opções, e os auxiliares puros
//! (`temporalUnitType`, `toSecondsStringPrecisionRecord`, `formatSecondsString*`, `ellipsizeAt`).
//!
//! DIVERGÊNCIAS desta fatia (lista completa em `wip-notes/temporal-plan.md`):
//! - A tabela estática `temporalObjectTable` (`Duration`, `Instant`, `Now`, `PlainDate`, `PlainDateTime`,
//!   `PlainTime`, `PlainMonthDay`, `PlainYearMonth`, `ZonedDateTime`, todas `DontEnum|PropertyCallback`)
//!   é preguiçosa como no C++: cada callback roda o `install_*` da classe no primeiro acesso. Os
//!   `LazyClassStructure` do global (`plain_date_structure()` e irmãs) reificam a entrada quando a estrutura
//!   é pedida antes de alguém ler `Temporal.X` (`lazy_temporal_structure`).
//! - `TemporalUnit::ALL`, `UnitOption` e `temporal_unit_valued`, `validate_temporal_unit_value`,
//!   `temporal_rounding_mode`, `temporal_rounding_increment` e `temporal_fractional_second_digits` (as opções
//!   que `Temporal.Duration` lê) e `extract_difference_options` estão aqui, sobre `intl_support`
//!   (`intlStringOption`, `intlOption`).
//! - O que depende de `JSGlobalObject`/Intl e das classes (`toTemporalOverflow`,
//!   `temporalShowCalendarName`, `toTemporalCalendarIdentifier`, `toTemporalDisambiguation`, `toTemporalOffset`,
//!   `isPartialTemporalObject`, `toTemporalTimeZoneIdentifier`, `temporalType`) espera as classes e o calendário.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind, LazyPropertyCallback};
use crate::runtime::lookup::{lazy_entry};
use crate::runtime::property_attribute::{DONT_ENUM, PROPERTY_CALLBACK};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::runtime::host_call::{pending_or, Thrown};
use crate::runtime::intl_support::{get_property, option_enum, option_string, to_number_checked, IntlEnum};

/// `numberOfTemporalUnits` e a ordem da tabela de unidades como um array indexável: `ALL[i]` é a unidade do
/// campo `i` de `ISO8601::Duration` (`operator[](size_t)` e `setField(size_t, double)` do C++).
impl TemporalUnit {
    pub const ALL: [TemporalUnit; NUMBER_OF_TEMPORAL_UNITS] = [
        TemporalUnit::Year,
        TemporalUnit::Month,
        TemporalUnit::Week,
        TemporalUnit::Day,
        TemporalUnit::Hour,
        TemporalUnit::Minute,
        TemporalUnit::Second,
        TemporalUnit::Millisecond,
        TemporalUnit::Microsecond,
        TemporalUnit::Nanosecond,
    ];
}

/// `temporalUnitPluralPropertyName(vm, unit)`: `vm.propertyNames->years` e irmãs.
pub fn temporal_unit_plural_property_name(vm: &VM, unit: TemporalUnit) -> crate::runtime::property_name::PropertyName {
    let names = &vm.property_names;
    let identifier = match unit {
        TemporalUnit::Year => &names.years,
        TemporalUnit::Month => &names.months,
        TemporalUnit::Week => &names.weeks,
        TemporalUnit::Day => &names.days,
        TemporalUnit::Hour => &names.hours,
        TemporalUnit::Minute => &names.minutes,
        TemporalUnit::Second => &names.seconds,
        TemporalUnit::Millisecond => &names.milliseconds,
        TemporalUnit::Microsecond => &names.microseconds,
        TemporalUnit::Nanosecond => &names.nanoseconds,
    };
    crate::runtime::property_name::PropertyName::from_identifier(identifier)
}

/// `JSValue::toIntegerWithTruncation(globalObject)`: https://tc39.es/proposal-temporal/#sec-tointegerwithtruncation
/// `trunc(toNumber(value) + 0.0)`: o `+ 0.0` normaliza -0, e `NaN` e infinitos seguem como estão (quem chama
/// lança o `RangeError` de "não finito").
pub fn to_integer_with_truncation(global_object: &JSGlobalObject, value: JSValue) -> Result<f64, Thrown> {
    Ok((to_number_checked(global_object, value)? + 0.0).trunc())
}

/// `temporalUnitSingularPropertyName(vm, unit)`: `vm.propertyNames->year` e irmãs.
pub fn temporal_unit_singular_property_name(vm: &VM, unit: TemporalUnit) -> crate::runtime::property_name::PropertyName {
    let names = &vm.property_names;
    let identifier = match unit {
        TemporalUnit::Year => &names.year,
        TemporalUnit::Month => &names.month,
        TemporalUnit::Week => &names.week,
        TemporalUnit::Day => &names.day,
        TemporalUnit::Hour => &names.hour,
        TemporalUnit::Minute => &names.minute,
        TemporalUnit::Second => &names.second,
        TemporalUnit::Millisecond => &names.millisecond,
        TemporalUnit::Microsecond => &names.microsecond,
        TemporalUnit::Nanosecond => &names.nanosecond,
    };
    crate::runtime::property_name::PropertyName::from_identifier(identifier)
}

/// `Variant<TemporalAuto, std::optional<TemporalUnit>>`, o valor de `GetTemporalUnitValuedOption`:
/// `Auto` é `TemporalAuto::Auto`, `Unset` é o `std::optional` vazio (`isAbsentUnit`) e `Unit` o valor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitOption {
    Auto,
    Unset,
    Unit(TemporalUnit),
}

impl UnitOption {
    /// `isAbsentUnit(unit)`.
    pub fn is_absent(self) -> bool {
        self == UnitOption::Unset
    }
}

/// O texto de uma `String` do WTF como unidades UTF-16 (o `StringView` do C++ lê Latin1 ou UTF-16).
pub fn string_units(global_object: &JSGlobalObject, value: JSValue) -> Result<Vec<u16>, Thrown> {
    let text = pending_or(global_object, value.to_wtf_string())?;
    Ok(text.characters_without_null_termination().unwrap_or_default())
}

/// `makeString(...)` com um trecho UTF-16 do usuário no meio (`'`, o texto cortado por `ellipsizeAt`, a
/// continuação), lançado como `RangeError`. `Thrown::RangeError` só carrega ASCII; aqui o texto do usuário
/// passa inteiro. Devolve `Thrown::Pending` com a exceção já lançada.
pub fn throw_range_error_with_units(global_object: &JSGlobalObject, prefix: &str, units: &[u16], suffix: &str) -> Thrown {
    use crate::runtime::error::create_range_error;
    use crate::runtime::throw_scope::{throw_exception, ThrowScope};
    use crate::wtf::text::wtf_string::String as WtfString;

    let mut message: Vec<u16> = prefix.encode_utf16().collect();
    message.extend_from_slice(units);
    message.extend(suffix.encode_utf16());
    let mut scope = ThrowScope::new(global_object.vm());
    let error = create_range_error(global_object, &WtfString::from_utf16(&message));
    throw_exception(global_object, &mut scope, error);
    Thrown::Pending
}

/// `GetTemporalUnitValuedOption(options, key, default)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-gettemporalunitvaluedoption
pub fn temporal_unit_valued(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    key: &str,
    default_value: TemporalUnitDefault,
) -> Result<UnitOption, Thrown> {
    // Passos 1 a 3: `intlStringOption` com a lista vazia aceita qualquer texto; a conferência contra a tabela
    // de unidades fica em `temporalUnitType` abaixo, com o mesmo efeito observável.
    let unit = option_string(global_object, options, key, &[], "")?;

    // Passo 4: se o valor é `undefined`.
    let Some(unit) = unit else {
        if default_value == TemporalUnitDefault::Required {
            return Err(Thrown::RangeError(format!("'{key}' option is required")));
        }
        return Ok(UnitOption::Unset);
    };

    // Passo 5: `"auto"` devolve `~auto~`.
    if unit == "auto" {
        return Ok(UnitOption::Auto);
    }

    // Passo 6: a unidade da tabela (singular ou plural).
    match temporal_unit_type(&unit) {
        Some(unit) => Ok(UnitOption::Unit(unit)),
        None => Err(Thrown::range_error("invalid Temporal unit")),
    }
}

/// `ValidateTemporalUnitValue(unit, unitGroup, extraValue, valueName)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-validatetemporalunitvaluedoption
pub fn validate_temporal_unit_value(
    unit: UnitOption,
    unit_group: UnitGroup,
    extra_value: AllowedUnit,
    value_name: &str,
) -> Result<(), Thrown> {
    // Passo 1: se o valor é `~unset~`, `~unused~`.
    if unit.is_absent() {
        return Ok(());
    }
    // Passo 2: se `extraValues` contém o valor, `~unused~`.
    if extra_value == AllowedUnit::Auto && unit == UnitOption::Auto {
        return Ok(());
    }
    if extra_value == AllowedUnit::Day && unit == UnitOption::Unit(TemporalUnit::Day) {
        return Ok(());
    }
    // Passo 3: a categoria do valor; sem linha na tabela (`~auto~`), `RangeError`.
    let UnitOption::Unit(actual_unit) = unit else {
        return Err(Thrown::RangeError(format!("{value_name} cannot be \"auto\"")));
    };
    // Passo 4: categoria `~date~` com grupo `~date~` ou `~datetime~`.
    if actual_unit <= TemporalUnit::Day && matches!(unit_group, UnitGroup::Date | UnitGroup::DateTime) {
        return Ok(());
    }
    // Passo 5: categoria `~time~` com grupo `~time~` ou `~datetime~`.
    if actual_unit > TemporalUnit::Day && matches!(unit_group, UnitGroup::Time | UnitGroup::DateTime) {
        return Ok(());
    }
    // Passo 6: `RangeError`.
    Err(Thrown::RangeError(format!("{value_name} is a disallowed unit")))
}

impl IntlEnum for RoundingMode {
    const NAMES: &'static [&'static str] =
        &["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"];

    fn parse(text: &str) -> Option<RoundingMode> {
        match text {
            "ceil" => Some(RoundingMode::Ceil),
            "floor" => Some(RoundingMode::Floor),
            "expand" => Some(RoundingMode::Expand),
            "trunc" => Some(RoundingMode::Trunc),
            "halfCeil" => Some(RoundingMode::HalfCeil),
            "halfFloor" => Some(RoundingMode::HalfFloor),
            "halfExpand" => Some(RoundingMode::HalfExpand),
            "halfTrunc" => Some(RoundingMode::HalfTrunc),
            "halfEven" => Some(RoundingMode::HalfEven),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            RoundingMode::Ceil => "ceil",
            RoundingMode::Floor => "floor",
            RoundingMode::Expand => "expand",
            RoundingMode::Trunc => "trunc",
            RoundingMode::HalfCeil => "halfCeil",
            RoundingMode::HalfFloor => "halfFloor",
            RoundingMode::HalfExpand => "halfExpand",
            RoundingMode::HalfTrunc => "halfTrunc",
            RoundingMode::HalfEven => "halfEven",
        }
    }
}

/// `temporalRoundingMode(globalObject, options, fallback)` (`GetRoundingModeOption`):
/// https://tc39.es/proposal-temporal/#sec-temporal-getroundingmodeoption
pub fn temporal_rounding_mode(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    fallback: RoundingMode,
) -> Result<RoundingMode, Thrown> {
    let mode = option_enum::<RoundingMode>(
        global_object,
        options,
        "roundingMode",
        "roundingMode must be \"ceil\", \"floor\", \"expand\", \"trunc\", \"halfCeil\", \"halfFloor\", \"halfExpand\", \"halfTrunc\", or \"halfEven\"",
    )?;
    Ok(mode.unwrap_or(fallback))
}

/// `doubleNumberOption(globalObject, options, property, defaultValue)`: o caso `number` de `GetOption`
/// (https://tc39.es/proposal-temporal/#sec-getoption).
fn double_number_option(global_object: &JSGlobalObject, options: Option<JSValue>, property: &str, default_value: f64) -> Result<f64, Thrown> {
    let Some(options) = options else { return Ok(default_value) };
    let value = get_property(global_object, options, property)?;
    if value.is_undefined() {
        return Ok(default_value);
    }
    let double_value = to_number_checked(global_object, value)?;
    if double_value.is_nan() {
        return Err(Thrown::RangeError(format!("{property} is NaN")));
    }
    Ok(double_value)
}

/// `temporalRoundingIncrement(globalObject, options)` (`GetRoundingIncrementOption`):
/// https://tc39.es/proposal-temporal/#sec-temporal-getroundingincrementoption
pub fn temporal_rounding_increment(global_object: &JSGlobalObject, options: Option<JSValue>) -> Result<f64, Thrown> {
    // Passos 1 e 2: lê `roundingIncrement`; `undefined` devolve 1.
    let increment = double_number_option(global_object, options, "roundingIncrement", 1.0)?;

    // Passo 3: `ToIntegerWithTruncation`, não finito é `RangeError`.
    if !increment.is_finite() {
        return Err(Thrown::range_error("roundingIncrement must be a finite integer"));
    }
    let integer_increment = increment.trunc();

    // Passo 4: fora de `1` a `10^9`, `RangeError`.
    if !(1.0..=1e9).contains(&integer_increment) {
        return Err(Thrown::range_error("roundingIncrement must be in the range 1 to 10^9 inclusive"));
    }

    // Passo 5.
    Ok(integer_increment)
}

/// `temporalFractionalSecondDigits(globalObject, options)` (`GetTemporalFractionalSecondDigitsOption`):
/// https://tc39.es/proposal-temporal/#sec-temporal-gettemporalfractionalseconddigitsoption
/// `None` é `auto` (e também a ausência de `options`).
pub fn temporal_fractional_second_digits(global_object: &JSGlobalObject, options: Option<JSValue>) -> Result<Option<u32>, Thrown> {
    let Some(options) = options else { return Ok(None) };
    let value = get_property(global_object, options, "fractionalSecondDigits")?;
    if value.is_undefined() {
        return Ok(None);
    }

    if value.is_number() {
        let double_value = value.as_number().floor();
        if !(0.0..=9.0).contains(&double_value) {
            let shown = crate::wtf::text::wtf_string::String::number_f64(double_value);
            let shown = String::from_utf8_lossy(&shown.latin1()).into_owned();
            return Err(Thrown::RangeError(format!("fractionalSecondDigits must be 'auto' or 0 through 9, not {shown}")));
        }
        return Ok(Some(double_value as u32));
    }

    let units = string_units(global_object, value)?;
    if units != "auto".encode_utf16().collect::<Vec<u16>>() {
        let shown = ellipsize_at(100, &units);
        return Err(throw_range_error_with_units(
            global_object,
            "fractionalSecondDigits must be 'auto' or 0 through 9, not ",
            &shown,
            "",
        ));
    }
    Ok(None)
}

/// `extractDifferenceOptions(globalObject, optionsValue, unitGroup, fallbackSmallestUnit,
/// smallestLargestDefaultUnit, operation)` (`GetDifferenceSettings`): devolve `(smallestUnit, largestUnit,
/// roundingMode, roundingIncrement)`. https://tc39.es/proposal-temporal/#sec-temporal-getdifferencesettings
pub fn extract_difference_options(
    global_object: &JSGlobalObject,
    options_value: JSValue,
    unit_group: UnitGroup,
    fallback_smallest_unit: TemporalUnit,
    smallest_largest_default_unit: TemporalUnit,
    operation: DifferenceOperation,
) -> Result<(TemporalUnit, TemporalUnit, RoundingMode, f64), Thrown> {
    use crate::runtime::intl_support::get_options_object;
    use crate::runtime::temporal_core_rounding::{maximum_rounding_increment, negate_temporal_rounding_mode, validate_temporal_rounding_increment};

    // `disallowedUnits` derivado do `unitGroup` (a tabela do C++ na ordem `DateTime`, `Date`, `Time`).
    let is_disallowed = |unit: TemporalUnit| match unit_group {
        UnitGroup::DateTime => false,
        UnitGroup::Date => unit > TemporalUnit::Day,
        UnitGroup::Time => unit <= TemporalUnit::Day,
    };

    let options = get_options_object(options_value)?;

    // Passo 1: as opções em ordem alfabética. Passo 2: `largestUnit`.
    let mut largest_unit_maybe_auto = temporal_unit_valued(global_object, options, "largestUnit", TemporalUnitDefault::Unset)?;
    // Passo 3.
    let rounding_increment = temporal_rounding_increment(global_object, options)?;
    // Passo 4.
    let mut rounding_mode = temporal_rounding_mode(global_object, options, RoundingMode::Trunc)?;
    // Passo 5: `smallestUnit`.
    let smallest_unit_maybe_auto = temporal_unit_valued(global_object, options, "smallestUnit", TemporalUnitDefault::Unset)?;

    // Passo 6.
    validate_temporal_unit_value(largest_unit_maybe_auto, unit_group, AllowedUnit::Auto, "largestUnit")?;
    // Passo 7: `largestUnit` ausente é `auto`.
    if largest_unit_maybe_auto.is_absent() {
        largest_unit_maybe_auto = UnitOption::Auto;
    }
    // Passo 8.
    if let UnitOption::Unit(unit) = largest_unit_maybe_auto {
        if is_disallowed(unit) {
            return Err(Thrown::range_error("largestUnit is a disallowed unit"));
        }
    }

    // Passo 9.
    validate_temporal_unit_value(smallest_unit_maybe_auto, unit_group, AllowedUnit::None, "smallestUnit")?;
    // Passo 10.
    let smallest_unit = match smallest_unit_maybe_auto {
        UnitOption::Unit(unit) => unit,
        UnitOption::Unset => fallback_smallest_unit,
        UnitOption::Auto => unreachable!("smallestUnit validado sem `auto`"),
    };
    // Passo 11.
    if is_disallowed(smallest_unit) {
        return Err(Thrown::range_error("smallestUnit is a disallowed unit"));
    }

    // Passos 12 e 13: `LargerOfTwoTemporalUnits(smallestLargestDefaultUnit, smallestUnit)` é o `min` da ordem.
    let largest_unit = match largest_unit_maybe_auto {
        UnitOption::Unit(unit) => unit,
        _ => smallest_largest_default_unit.min(smallest_unit),
    };

    // Passo 14.
    if smallest_unit < largest_unit {
        return Err(Thrown::range_error("smallestUnit must be smaller than largestUnit"));
    }

    // Passos 15 e 16.
    if let Some(maximum) = maximum_rounding_increment(smallest_unit) {
        validate_temporal_rounding_increment(rounding_increment, Some(f64::from(maximum)), Inclusivity::Exclusive)?;
    }

    // Passo 17.
    if operation == DifferenceOperation::Since {
        rounding_mode = negate_temporal_rounding_mode(rounding_mode);
    }
    // Passo 18.
    Ok((smallest_unit, largest_unit, rounding_mode, rounding_increment))
}

impl IntlEnum for TemporalOverflow {
    const NAMES: &'static [&'static str] = &["constrain", "reject"];

    fn parse(text: &str) -> Option<TemporalOverflow> {
        match text {
            "constrain" => Some(TemporalOverflow::Constrain),
            "reject" => Some(TemporalOverflow::Reject),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            TemporalOverflow::Constrain => "constrain",
            TemporalOverflow::Reject => "reject",
        }
    }
}

/// `toTemporalOverflow(globalObject, JSObject* options)` (`GetTemporalOverflowOption`):
/// https://tc39.es/proposal-temporal/#sec-temporal-totemporaloverflow
pub fn to_temporal_overflow(global_object: &JSGlobalObject, options: Option<JSValue>) -> Result<TemporalOverflow, Thrown> {
    let overflow = option_enum::<TemporalOverflow>(global_object, options, "overflow", "overflow must be either \"constrain\" or \"reject\"")?;
    Ok(overflow.unwrap_or(TemporalOverflow::Constrain))
}

/// `toTemporalOverflow(globalObject, JSValue val)`: `GetOptionsObject(val)` e depois o `GetTemporalOverflowOption`.
pub fn to_temporal_overflow_value(global_object: &JSGlobalObject, value: JSValue) -> Result<TemporalOverflow, Thrown> {
    let options = crate::runtime::intl_support::get_options_object(value)?;
    to_temporal_overflow(global_object, options)
}

impl IntlEnum for TemporalDisambiguation {
    const NAMES: &'static [&'static str] = &["compatible", "earlier", "later", "reject"];

    fn parse(text: &str) -> Option<TemporalDisambiguation> {
        match text {
            "compatible" => Some(TemporalDisambiguation::Compatible),
            "earlier" => Some(TemporalDisambiguation::Earlier),
            "later" => Some(TemporalDisambiguation::Later),
            "reject" => Some(TemporalDisambiguation::Reject),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            TemporalDisambiguation::Compatible => "compatible",
            TemporalDisambiguation::Earlier => "earlier",
            TemporalDisambiguation::Later => "later",
            TemporalDisambiguation::Reject => "reject",
        }
    }
}

impl IntlEnum for TemporalOffsetDisambiguation {
    const NAMES: &'static [&'static str] = &["use", "prefer", "ignore", "reject"];

    fn parse(text: &str) -> Option<TemporalOffsetDisambiguation> {
        match text {
            "use" => Some(TemporalOffsetDisambiguation::Use),
            "prefer" => Some(TemporalOffsetDisambiguation::Prefer),
            "ignore" => Some(TemporalOffsetDisambiguation::Ignore),
            "reject" => Some(TemporalOffsetDisambiguation::Reject),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            TemporalOffsetDisambiguation::Use => "use",
            TemporalOffsetDisambiguation::Prefer => "prefer",
            TemporalOffsetDisambiguation::Ignore => "ignore",
            TemporalOffsetDisambiguation::Reject => "reject",
        }
    }
}

/// `toTemporalDisambiguation(globalObject, options)` (`GetTemporalDisambiguationOption`):
/// https://tc39.es/proposal-temporal/#sec-temporal-totemporaldisambiguation
pub fn to_temporal_disambiguation(global_object: &JSGlobalObject, options: Option<JSValue>) -> Result<TemporalDisambiguation, Thrown> {
    let disambiguation = option_enum::<TemporalDisambiguation>(
        global_object,
        options,
        "disambiguation",
        "disambiguation must be one of \"compatible\", \"earlier\", \"later\", or \"reject\"",
    )?;
    Ok(disambiguation.unwrap_or(TemporalDisambiguation::Compatible))
}

/// `toTemporalOffset(globalObject, options, fallback)` (`GetTemporalOffsetOption`):
/// https://tc39.es/proposal-temporal/#sec-temporal-totemporaloffset
pub fn to_temporal_offset(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    fallback: TemporalOffsetDisambiguation,
) -> Result<TemporalOffsetDisambiguation, Thrown> {
    let offset = option_enum::<TemporalOffsetDisambiguation>(
        global_object,
        options,
        "offset",
        "offset must be one of \"use\", \"prefer\", \"ignore\", or \"reject\"",
    )?;
    Ok(offset.unwrap_or(fallback))
}

/// O trecho de `ToTemporalDate`, `ToTemporalYearMonth` e `ToTemporalMonthDay` com objeto de propriedades (passos
/// "GetOptionsObject" e "GetTemporalOverflowOption" depois de `PrepareCalendarFields`): `undefined` é
/// `~constrain~` e quem não é objeto é `TypeError`, só depois de os campos terem sido lidos (ordem observável).
pub fn to_temporal_overflow_after_fields(global_object: &JSGlobalObject, options_value: JSValue) -> Result<TemporalOverflow, Thrown> {
    if options_value.is_undefined() {
        return Ok(TemporalOverflow::Constrain);
    }
    if !options_value.is_object() {
        return Err(Thrown::type_error("options must be an object"));
    }
    to_temporal_overflow(global_object, Some(options_value))
}

/// `ToIntegerWithTruncation` com o `RangeError` de valor não finito que a spec manda e o `JSValue` do JSC não
/// dá (o construtor de `PlainYearMonth` e o de `PlainMonthDay`, onde `undefined` vira `NaN` e cai aqui):
/// https://tc39.es/proposal-temporal/#sec-tointegerwithtruncation
pub fn to_finite_integer_with_truncation(global_object: &JSGlobalObject, value: JSValue, non_finite_message: &str) -> Result<f64, Thrown> {
    let number = to_integer_with_truncation(global_object, value)?;
    if !number.is_finite() {
        return Err(Thrown::RangeError(non_finite_message.to_string()));
    }
    Ok(number)
}

/// `JSC_DEFINE_CUSTOM_GETTER(temporalXxxPrototypeGetterYyy, ...)` dos protótipos de Temporal: a conferência de
/// marca (`this_fn(this, nome)`, o `TypeError` de `this`) e o valor. `$global` e `$cell` são os nomes que o `$value`
/// usa para o `JSGlobalObject` e a célula; `$host` é o `PutValueFunc` que o `CustomGetterSetter` recebe.
#[macro_export]
macro_rules! temporal_getter {
    ($host:ident, $body:ident, $this_fn:ident, $name:literal, |$global:ident, $cell:ident| $value:expr) => {
        fn $body(
            $global: &$crate::runtime::js_global_object::JSGlobalObject,
            this_value: $crate::runtime::js_value::JSValue,
            _property_name: &$crate::runtime::property_name::PropertyName,
        ) -> $crate::runtime::host_call::HostResult {
            let $cell = $this_fn(this_value, $name)?;
            Ok($value)
        }
        $crate::custom_getter!($host, $body);
    };
}

/// `isPartialTemporalObject(globalObject, value)`: https://tc39.es/proposal-temporal/#sec-temporal-ispartialtemporalobject
/// DIVERGÊNCIA: o passo 2 (`inherits<TemporalPlainDate>` e as outras classes com `[[InitializedTemporal*]]`) só
/// confere as classes já portadas (`PlainTime`); cada porte novo acrescenta a sua aqui.
pub fn is_partial_temporal_object(global_object: &JSGlobalObject, value: JSValue) -> Result<bool, Thrown> {
    // Passo 1: quem não é objeto não é parcial.
    if !value.is_object() {
        return Ok(false);
    }

    // Passo 2: objeto com slot `[[InitializedTemporal*]]` não é parcial.
    if crate::runtime::temporal_plain_date::TemporalPlainDate::from_value(&value).is_some() {
        return Ok(false);
    }
    if crate::runtime::temporal_plain_date_time::TemporalPlainDateTime::from_value(&value).is_some() {
        return Ok(false);
    }
    if crate::runtime::temporal_plain_time::TemporalPlainTime::from_value(&value).is_some() {
        return Ok(false);
    }
    if crate::runtime::temporal_plain_year_month::TemporalPlainYearMonth::from_value(&value).is_some() {
        return Ok(false);
    }
    if crate::runtime::temporal_plain_month_day::TemporalPlainMonthDay::from_value(&value).is_some() {
        return Ok(false);
    }
    if crate::runtime::temporal_zoned_date_time::TemporalZonedDateTime::from_value(&value).is_some() {
        return Ok(false);
    }

    // Passos 3 e 4: `calendar` definido não é parcial.
    if !get_property(global_object, value, "calendar")?.is_undefined() {
        return Ok(false);
    }

    // Passos 5 e 6: `timeZone` definido não é parcial.
    if !get_property(global_object, value, "timeZone")?.is_undefined() {
        return Ok(false);
    }

    // Passo 7.
    Ok(true)
}

/// Os membros do `JSGlobalObject` que o porte de `Temporal` preenche: `m_durationStructure` e
/// `m_instantStructure` (os `LazyClassStructure` de `Temporal.Duration` e `Temporal.Instant`).
#[derive(Debug, Default)]
pub struct TemporalGlobalData {
    pub(crate) duration_structure: Option<StructureRef>,
    pub(crate) instant_structure: Option<StructureRef>,
    /// `m_plainTimeStructure` (o `LazyClassStructure` de `Temporal.PlainTime`).
    pub(crate) plain_time_structure: Option<StructureRef>,
    /// `m_plainDateStructure` (o `LazyClassStructure` de `Temporal.PlainDate`).
    pub(crate) plain_date_structure: Option<StructureRef>,
    /// `m_plainDateTimeStructure` (o `LazyClassStructure` de `Temporal.PlainDateTime`).
    pub(crate) plain_date_time_structure: Option<StructureRef>,
    /// `m_plainYearMonthStructure` (o `LazyClassStructure` de `Temporal.PlainYearMonth`).
    pub(crate) plain_year_month_structure: Option<StructureRef>,
    /// `m_plainMonthDayStructure` (o `LazyClassStructure` de `Temporal.PlainMonthDay`).
    pub(crate) plain_month_day_structure: Option<StructureRef>,
    /// `m_zonedDateTimeStructure` (o `LazyClassStructure` de `Temporal.ZonedDateTime`).
    pub(crate) zoned_date_time_structure: Option<StructureRef>,
    /// O objeto `Temporal`, para o `LazyClassStructure` poder reificar a entrada da tabela quando a
    /// estrutura é pedida antes de alguém ler `Temporal.X`.
    pub(crate) temporal: Option<JSObjectRef>,
}

/// O `LazyClassStructure::get(globalObject)` dos membros de `Temporal`: se a estrutura ainda não existe, a
/// entrada `key` de `temporalObjectTable` é reificada (o callback roda o `install_*` da classe, que preenche
/// o membro) e o membro é lido de novo.
pub(crate) fn lazy_temporal_structure(
    global_object: &JSGlobalObject,
    key: &str,
    select: fn(&TemporalGlobalData) -> Option<StructureRef>,
) -> StructureRef {
    if let Some(structure) = select(&global_object.temporal_data.borrow()) {
        return structure;
    }
    let temporal = global_object.temporal_data.borrow().temporal.clone().expect("JSGlobalObject sem Temporal");
    let entry = TEMPORAL_OBJECT_TABLE_VALUES.iter().find(|entry| entry.key == key).expect("membro fora de temporalObjectTable");
    crate::runtime::lookup::reify_static_property(global_object.vm(), &temporal, entry);
    select(&global_object.temporal_data.borrow()).expect("install_* não preencheu a estrutura")
}

impl JSGlobalObject {
    /// `durationStructure()`: o `LazyClassStructure` de `Temporal.Duration`.
    pub fn duration_structure(&self) -> StructureRef {
        lazy_temporal_structure(self, "Duration", |data| data.duration_structure.clone())
    }
}

/// `enum class TemporalUnit`: a ordem da tabela de unidades (`Year` é a maior, `Nanosecond` a menor), que o
/// C++ compara com `<`/`<=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TemporalUnit {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
    Nanosecond,
}

/// `numberOfTemporalUnits`.
pub const NUMBER_OF_TEMPORAL_UNITS: usize = 10;

/// `temporalUnitsInTableOrder`: https://tc39.es/proposal-temporal/#table-temporal-temporaldurationlike-properties
pub const TEMPORAL_UNITS_IN_TABLE_ORDER: [TemporalUnit; NUMBER_OF_TEMPORAL_UNITS] = [
    TemporalUnit::Day,
    TemporalUnit::Hour,
    TemporalUnit::Microsecond,
    TemporalUnit::Millisecond,
    TemporalUnit::Minute,
    TemporalUnit::Month,
    TemporalUnit::Nanosecond,
    TemporalUnit::Second,
    TemporalUnit::Week,
    TemporalUnit::Year,
];

/// `lengthInNanoseconds(TemporalUnit)`: só de `Nanosecond` a `Day` (o resto é `RELEASE_ASSERT_NOT_REACHED`).
pub fn length_in_nanoseconds(unit: TemporalUnit) -> i128 {
    match unit {
        TemporalUnit::Nanosecond => 1,
        TemporalUnit::Microsecond => 1000,
        TemporalUnit::Millisecond => 1000 * length_in_nanoseconds(TemporalUnit::Microsecond),
        TemporalUnit::Second => 1000 * length_in_nanoseconds(TemporalUnit::Millisecond),
        TemporalUnit::Minute => 60 * length_in_nanoseconds(TemporalUnit::Second),
        TemporalUnit::Hour => 60 * length_in_nanoseconds(TemporalUnit::Minute),
        TemporalUnit::Day => 24 * length_in_nanoseconds(TemporalUnit::Hour),
        TemporalUnit::Week | TemporalUnit::Month | TemporalUnit::Year => {
            unreachable!("lengthInNanoseconds de unidade de calendário")
        }
    }
}

/// `isCalendarUnit`: https://tc39.es/proposal-temporal/#sec-temporal-iscalendarunit
pub fn is_calendar_unit(unit: TemporalUnit) -> bool {
    unit <= TemporalUnit::Week
}

/// `enum class RoundingMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingMode {
    Ceil,
    Floor,
    Expand,
    Trunc,
    HalfCeil,
    HalfFloor,
    HalfExpand,
    HalfTrunc,
    HalfEven,
}

/// `enum class UnsignedRoundingMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsignedRoundingMode {
    Infinity,
    Zero,
    HalfInfinity,
    HalfZero,
    HalfEven,
}

/// `getUnsignedRoundingMode(roundingMode, isNegative)`: https://tc39.es/proposal-temporal/#sec-getunsignedroundingmode
pub fn get_unsigned_rounding_mode(rounding_mode: RoundingMode, is_negative: bool) -> UnsignedRoundingMode {
    match rounding_mode {
        RoundingMode::Ceil => if is_negative { UnsignedRoundingMode::Zero } else { UnsignedRoundingMode::Infinity },
        RoundingMode::Floor => if is_negative { UnsignedRoundingMode::Infinity } else { UnsignedRoundingMode::Zero },
        RoundingMode::Expand => UnsignedRoundingMode::Infinity,
        RoundingMode::Trunc => UnsignedRoundingMode::Zero,
        RoundingMode::HalfCeil => {
            if is_negative { UnsignedRoundingMode::HalfZero } else { UnsignedRoundingMode::HalfInfinity }
        }
        RoundingMode::HalfFloor => {
            if is_negative { UnsignedRoundingMode::HalfInfinity } else { UnsignedRoundingMode::HalfZero }
        }
        RoundingMode::HalfExpand => UnsignedRoundingMode::HalfInfinity,
        RoundingMode::HalfTrunc => UnsignedRoundingMode::HalfZero,
        RoundingMode::HalfEven => UnsignedRoundingMode::HalfEven,
    }
}

/// `enum class Inclusivity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inclusivity {
    Inclusive,
    Exclusive,
}

/// `struct ParsedMonthCode` (`TemporalObject.h`): o número do mês e se é mês bissexto (`L`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedMonthCode {
    pub month_number: u8,
    pub is_leap_month: bool,
}

/// `enum class TemporalOverflow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalOverflow {
    Constrain,
    Reject,
}

/// `enum class DifferenceOperation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifferenceOperation {
    Since,
    Until,
}

/// `enum class TemporalDisambiguation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalDisambiguation {
    Compatible,
    Earlier,
    Later,
    Reject,
}

/// `enum class TemporalOffsetDisambiguation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalOffsetDisambiguation {
    Use,
    Prefer,
    Ignore,
    Reject,
}

/// `enum class OffsetBehaviour`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetBehaviour {
    Wall,
    Exact,
    Option,
}

/// `enum class TemporalAuto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalAuto {
    Auto,
}

/// `enum class Precision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    Minute,
    Fixed,
    Auto,
}

/// `struct PrecisionData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrecisionData {
    pub precision: (Precision, u32),
    pub unit: TemporalUnit,
    pub increment: u32,
}

/// `enum class UnitGroup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitGroup {
    DateTime,
    Date,
    Time,
}

/// `enum class AllowedUnit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllowedUnit {
    Auto,
    Day,
    None,
}

/// `enum class TemporalUnitDefault`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalUnitDefault {
    Unset,
    Required,
}

/// `enum class AddOrSubtract : bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddOrSubtract {
    Add,
    Subtract,
}

/// `ellipsizeAt(maxLength, string)`: para mensagens de erro com valor sem limite. O tamanho é em unidades
/// UTF-16, como o `WTF::String`.
pub fn ellipsize_at(max_length: usize, string: &[u16]) -> Vec<u16> {
    if string.len() <= max_length {
        return string.to_vec();
    }
    let mut result = string[..max_length - 1].to_vec();
    result.push(0x2026);
    result
}

/// `temporalUnitType(StringView)`: o singular ou o plural (só o `s` final) de um nome de unidade.
pub fn temporal_unit_type(unit: &str) -> Option<TemporalUnit> {
    let singular = unit.strip_suffix('s').unwrap_or(unit);
    match singular {
        "year" => Some(TemporalUnit::Year),
        "month" => Some(TemporalUnit::Month),
        "week" => Some(TemporalUnit::Week),
        "day" => Some(TemporalUnit::Day),
        "hour" => Some(TemporalUnit::Hour),
        "minute" => Some(TemporalUnit::Minute),
        "second" => Some(TemporalUnit::Second),
        "millisecond" => Some(TemporalUnit::Millisecond),
        "microsecond" => Some(TemporalUnit::Microsecond),
        "nanosecond" => Some(TemporalUnit::Nanosecond),
        _ => None,
    }
}

/// `toSecondsStringPrecisionRecord(smallestUnit, fractionalDigitCount)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-tosecondsstringprecisionrecord
pub fn to_seconds_string_precision_record(
    smallest_unit: Option<TemporalUnit>,
    fractional_digit_count: Option<u32>,
) -> PrecisionData {
    let record = |precision, digits, unit, increment| PrecisionData { precision: (precision, digits), unit, increment };
    match smallest_unit {
        Some(TemporalUnit::Minute) => return record(Precision::Minute, 0, TemporalUnit::Minute, 1),
        Some(TemporalUnit::Second) => return record(Precision::Fixed, 0, TemporalUnit::Second, 1),
        Some(TemporalUnit::Millisecond) => return record(Precision::Fixed, 3, TemporalUnit::Millisecond, 1),
        Some(TemporalUnit::Microsecond) => return record(Precision::Fixed, 6, TemporalUnit::Microsecond, 1),
        Some(TemporalUnit::Nanosecond) => return record(Precision::Fixed, 9, TemporalUnit::Nanosecond, 1),
        _ => debug_assert!(smallest_unit.is_none()),
    }

    let Some(d) = fractional_digit_count else {
        return record(Precision::Auto, 0, TemporalUnit::Nanosecond, 1);
    };
    match d {
        0 => record(Precision::Fixed, 0, TemporalUnit::Second, 1),
        1..=3 => record(Precision::Fixed, d, TemporalUnit::Millisecond, 10u32.pow(3 - d)),
        4..=6 => record(Precision::Fixed, d, TemporalUnit::Microsecond, 10u32.pow(6 - d)),
        _ => record(Precision::Fixed, d, TemporalUnit::Nanosecond, 10u32.pow(9 - d)),
    }
}

/// `formatSecondsStringFraction(builder, fraction, precision)`.
pub fn format_seconds_string_fraction(builder: &mut String, fraction: u32, precision: (Precision, u32)) {
    let (precision_type, precision_value) = precision;
    if (precision_type == Precision::Auto && fraction != 0) || (precision_type == Precision::Fixed && precision_value != 0) {
        let padded = format!(".{fraction:09}");
        if precision_type == Precision::Fixed {
            builder.push_str(&padded[..padded.len() - (9 - precision_value as usize)]);
        } else {
            builder.push_str(padded.trim_end_matches('0'));
        }
    }
}

/// `formatSecondsStringPart(builder, second, fraction, precision)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-formatsecondsstringpart
pub fn format_seconds_string_part(builder: &mut String, second: u32, fraction: u32, precision: PrecisionData) {
    if precision.unit == TemporalUnit::Minute {
        return;
    }
    builder.push_str(&format!(":{second:02}"));
    format_seconds_string_fraction(builder, fraction, precision.precision);
}

/// `createXConstructor(vm, object)` do C++ (`PropertyCallback`): o `install_*` da classe cria protótipo,
/// estrutura e construtor e grava o construtor em `Temporal` (`DontEnum`); o callback devolve o valor gravado,
/// e a reificação da tabela o grava de novo com os mesmos atributos (substituição no lugar, sem transição).
macro_rules! lazy_temporal_class {
    ($callback:ident, $install:path, $name:literal) => {
        fn $callback(vm: &VM, temporal: &JSObject) -> JSValue {
            let global_object = temporal.structure().realm().expect("Temporal sem realm");
            $install(&global_object, temporal, &global_object.object_prototype());
            temporal.get_direct_by_name(vm, &crate::runtime::intl_support::prop(vm, $name))
        }
    };
}

lazy_temporal_class!(create_duration_constructor, crate::runtime::temporal_duration_constructor::install_duration, "Duration");
lazy_temporal_class!(create_instant_constructor, crate::runtime::temporal_instant::install_temporal_instant, "Instant");
lazy_temporal_class!(create_now_object, crate::runtime::temporal_now::install_temporal_now, "Now");
lazy_temporal_class!(create_plain_date_constructor, crate::runtime::temporal_plain_date_constructor::install_plain_date, "PlainDate");
lazy_temporal_class!(
    create_plain_date_time_constructor,
    crate::runtime::temporal_plain_date_time_constructor::install_plain_date_time,
    "PlainDateTime"
);
lazy_temporal_class!(create_plain_time_constructor, crate::runtime::temporal_plain_time_constructor::install_plain_time, "PlainTime");
lazy_temporal_class!(
    create_plain_month_day_constructor,
    crate::runtime::temporal_plain_month_day_constructor::install_plain_month_day,
    "PlainMonthDay"
);
lazy_temporal_class!(
    create_plain_year_month_constructor,
    crate::runtime::temporal_plain_year_month_constructor::install_plain_year_month,
    "PlainYearMonth"
);
lazy_temporal_class!(
    create_zoned_date_time_constructor,
    crate::runtime::temporal_zoned_date_time_constructor::install_zoned_date_time,
    "ZonedDateTime"
);

/// `temporalObjectTable`, na ordem do `@begin`.
static TEMPORAL_OBJECT_TABLE_VALUES: [HashTableValue; 9] = [
    lazy_entry("Duration", create_duration_constructor),
    lazy_entry("Instant", create_instant_constructor),
    lazy_entry("Now", create_now_object),
    lazy_entry("PlainDate", create_plain_date_constructor),
    lazy_entry("PlainDateTime", create_plain_date_time_constructor),
    lazy_entry("PlainTime", create_plain_time_constructor),
    lazy_entry("PlainMonthDay", create_plain_month_day_constructor),
    lazy_entry("PlainYearMonth", create_plain_year_month_constructor),
    lazy_entry("ZonedDateTime", create_zoned_date_time_constructor),
];

static TEMPORAL_OBJECT_TABLE: HashTable = HashTable { class_for_this: None, values: &TEMPORAL_OBJECT_TABLE_VALUES };

/// `const ClassInfo TemporalObject::s_info` (`"Temporal"`, `&temporalObjectTable`).
pub static TEMPORAL_OBJECT_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&TEMPORAL_OBJECT_TABLE),
    inherits_js_type_range: None,
};

/// `class TemporalObject final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalObject;

impl TemporalObject {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject)`: o protótipo é `globalObject->objectPrototype()`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, object_prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            object_prototype,
            TypeInfo::new(JSType::ObjectType, TemporalObject::STRUCTURE_FLAGS),
            &TEMPORAL_OBJECT_S_INFO,
        )
    }

    /// `create(vm, structure)`: `TemporalObject(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, structure: &StructureRef) -> JSObjectRef {
        let object = JSObject::allocate(vm, structure);
        TemporalObject::finish_creation(&object, vm);
        object
    }

    /// `finishCreation(vm)`: `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
    fn finish_creation(object: &JSObject, vm: &VM) {
        object.finish_creation(vm);
        put_to_string_tag(vm, object, TEMPORAL_OBJECT_S_INFO.class_name);
    }
}

/// A propriedade global `Temporal` (`DontEnum`) com o `TemporalObject`.
/// DIVERGÊNCIA: o C++ cria o objeto na primeira leitura (`PropertyCallback`) e só com `Options::useTemporal()`;
/// aqui nasce com o global, e o chamador decide pela opção.
pub fn install_temporal(global_object: &JSGlobalObject, object_prototype: &JSObjectRef) -> JSObjectRef {
    let vm = global_object.vm();
    let structure = TemporalObject::create_structure(vm, global_object, object_prototype.as_value());
    let temporal = TemporalObject::create(vm, &structure);
    global_object.put_direct(
        vm,
        &crate::runtime::property_name::PropertyName::from_identifier(&Identifier::from_span(vm, b"Temporal".as_slice())),
        temporal.as_value(),
        crate::runtime::property_attribute::DONT_ENUM,
    );
    // As nove entradas de `temporalObjectTable` nascem no primeiro acesso (`reify_static_property`).
    global_object.temporal_data.borrow_mut().temporal = Some(temporal.clone());
    temporal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_names_accept_singular_and_plural() {
        assert_eq!(temporal_unit_type("days"), Some(TemporalUnit::Day));
        assert_eq!(temporal_unit_type("nanosecond"), Some(TemporalUnit::Nanosecond));
        assert_eq!(temporal_unit_type("hourss"), None);
        assert_eq!(temporal_unit_type("s"), None);
    }

    #[test]
    fn precision_records() {
        let p = to_seconds_string_precision_record(None, Some(2));
        assert_eq!((p.precision, p.unit, p.increment), ((Precision::Fixed, 2), TemporalUnit::Millisecond, 10));
        let p = to_seconds_string_precision_record(None, Some(9));
        assert_eq!((p.unit, p.increment), (TemporalUnit::Nanosecond, 1));
        assert_eq!(to_seconds_string_precision_record(None, None).precision.0, Precision::Auto);
    }

    #[test]
    fn seconds_fraction_formatting() {
        let mut s = String::new();
        format_seconds_string_part(&mut s, 5, 120_000_000, to_seconds_string_precision_record(None, None));
        assert_eq!(s, ":05.12");
        let mut s = String::new();
        format_seconds_string_fraction(&mut s, 120_000_000, (Precision::Fixed, 5));
        assert_eq!(s, ".12000");
    }

    #[test]
    fn unit_lengths_and_order() {
        assert_eq!(length_in_nanoseconds(TemporalUnit::Day), 86_400_000_000_000);
        assert!(is_calendar_unit(TemporalUnit::Week) && !is_calendar_unit(TemporalUnit::Day));
        assert_eq!(ellipsize_at(3, &[b'a' as u16, b'b' as u16, b'c' as u16, b'd' as u16]), vec![97, 98, 0x2026]);
    }
}
