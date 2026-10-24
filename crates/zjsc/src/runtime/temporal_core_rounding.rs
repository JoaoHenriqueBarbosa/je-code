//! Porte de `runtime/temporal/core/Rounding.{h,cpp}`: o arredondamento de Temporal sobre inteiros
//! (`applyUnsignedRoundingMode`, `roundNumberToIncrementInt128`, `roundNumberToIncrementAsIfPositive`,
//! `roundNumberToIncrementDouble` (o de `RoundTime`), `negateTemporalRoundingMode`, `maximumRoundingIncrement`,
//! `validateTemporalRoundingIncrement`) e o
//! `maximumInstantIncrement` de `InstantCore.h`.
//!
//! DIVERGÊNCIA: `Int128` é `i128`; `std::optional<unsigned>` é `Option<u32>`.

use crate::runtime::temporal_core_types::{range_error, TemporalResult};
use crate::runtime::temporal_object::{get_unsigned_rounding_mode, length_in_nanoseconds, Inclusivity, RoundingMode, TemporalUnit, UnsignedRoundingMode};

/// `applyUnsignedRoundingMode(xNumerator, xDenominator, r1, r2, unsignedRoundingMode)`:
/// https://tc39.es/proposal-temporal/#sec-applyunsignedroundingmode
pub fn apply_unsigned_rounding_mode(
    x_numerator: i128,
    x_denominator: i128,
    r1: i128,
    r2: i128,
    unsigned_rounding_mode: UnsignedRoundingMode,
) -> i128 {
    debug_assert!(x_denominator > 0);
    let scaled_r1 = r1 * x_denominator;
    // 1. If x = r1, return r1.
    if x_numerator == scaled_r1 {
        return r1;
    }
    // 2. Assert: r1 < x < r2. 3. Assert: unsignedRoundingMode is not undefined.
    debug_assert!(scaled_r1 < x_numerator && x_numerator < r2 * x_denominator);
    // 4. If unsignedRoundingMode is ~zero~, return r1.
    if unsigned_rounding_mode == UnsignedRoundingMode::Zero {
        return r1;
    }
    // 5. If unsignedRoundingMode is ~infinity~, return r2.
    if unsigned_rounding_mode == UnsignedRoundingMode::Infinity {
        return r2;
    }
    // 6-9. d1 = x - r1, d2 = r2 - x; d1 < d2 iff 2x < r1 + r2.
    let doubled_x = x_numerator * 2;
    let sum_of_bounds = (r1 + r2) * x_denominator;
    if doubled_x < sum_of_bounds {
        return r1;
    }
    if doubled_x > sum_of_bounds {
        return r2;
    }
    // 10. Assert: d1 is equal to d2.
    // 11. If unsignedRoundingMode is ~half-zero~, return r1.
    if unsigned_rounding_mode == UnsignedRoundingMode::HalfZero {
        return r1;
    }
    // 12. If unsignedRoundingMode is ~half-infinity~, return r2.
    if unsigned_rounding_mode == UnsignedRoundingMode::HalfInfinity {
        return r2;
    }
    // 13. Assert: unsignedRoundingMode is ~half-even~.
    debug_assert!(unsigned_rounding_mode == UnsignedRoundingMode::HalfEven);
    // 14-16. cardinality = (r1 / (r2 - r1)) modulo 2.
    if (r1 / (r2 - r1)) % 2 == 0 {
        r1
    } else {
        r2
    }
}

/// `negateTemporalRoundingMode(roundingMode)` (`NegateRoundingMode`):
/// https://tc39.es/proposal-temporal/#sec-temporal-negateroundingmode
pub fn negate_temporal_rounding_mode(rounding_mode: RoundingMode) -> RoundingMode {
    match rounding_mode {
        RoundingMode::Ceil => RoundingMode::Floor,
        RoundingMode::Floor => RoundingMode::Ceil,
        RoundingMode::HalfCeil => RoundingMode::HalfFloor,
        RoundingMode::HalfFloor => RoundingMode::HalfCeil,
        other => other,
    }
}

/// `roundNumberToIncrementAsIfPositive(x, increment, mode)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-roundnumbertoincrementasifpositive
pub fn round_number_to_increment_as_if_positive(x: i128, increment: i128, mode: RoundingMode) -> i128 {
    // 1. Let quotient be x / increment.
    let quotient = x / increment;
    // Caso exato: `x` divisível pelo incremento devolve `x` antes de calcular r1 e r2.
    if x % increment == 0 {
        return x;
    }
    // 2. Let unsignedRoundingMode be GetUnsignedRoundingMode(roundingMode, ~positive~).
    let unsigned_rounding_mode = get_unsigned_rounding_mode(mode, false);
    // 3-4. r1 é o maior inteiro <= quotient e r2 o menor > quotient (a divisão trunca para zero).
    let (r1, r2) = if x < 0 { (quotient - 1, quotient) } else { (quotient, quotient + 1) };
    // 5-6. rounded × increment.
    apply_unsigned_rounding_mode(x, increment, r1, r2, unsigned_rounding_mode) * increment
}

/// `maximumInstantIncrement(smallestUnit)` (`InstantCore.h`): o dividendo de
/// `ValidateTemporalRoundingIncrement` para `Instant.prototype.round`.
pub fn maximum_instant_increment(smallest_unit: TemporalUnit) -> f64 {
    // Quantas unidades de `smallestUnit` cabem em um dia.
    length_in_nanoseconds(TemporalUnit::Day) as i64 as f64 / length_in_nanoseconds(smallest_unit) as i64 as f64
}

/// `maximumRoundingIncrement(unit)` (`MaximumTemporalDurationRoundingIncrement`):
/// https://tc39.es/proposal-temporal/#sec-temporal-maximumtemporaldurationroundingincrement
pub fn maximum_rounding_increment(unit: TemporalUnit) -> Option<u32> {
    // Year, Month, Week e Day não têm máximo (`~unset~`); as unidades de tempo têm valores fixos.
    if unit <= TemporalUnit::Day {
        return None;
    }
    if unit == TemporalUnit::Hour {
        return Some(24);
    }
    if unit <= TemporalUnit::Second {
        return Some(60);
    }
    Some(1000)
}

/// `roundNumberToIncrementDouble(x, increment, mode)`: o mesmo arredondamento sobre `double` (o de
/// `RoundTime`). `quotient` inteiro sai direto; senão `truncatedQuotient` e `expandedQuotient` são `r1` e
/// `r2` com o sinal, e os modos `half*` decidem pela parte fracionária.
pub fn round_number_to_increment_double(x: f64, increment: f64, mode: RoundingMode) -> f64 {
    let quotient = x / increment;
    let truncated_quotient = quotient.trunc();
    if truncated_quotient == quotient {
        return truncated_quotient * increment;
    }

    let is_negative = quotient < 0.0;
    let expanded_quotient = if is_negative { truncated_quotient - 1.0 } else { truncated_quotient + 1.0 };

    if matches!(
        mode,
        RoundingMode::HalfCeil | RoundingMode::HalfFloor | RoundingMode::HalfExpand | RoundingMode::HalfTrunc | RoundingMode::HalfEven
    ) {
        let unsigned_fractional_part = (quotient - truncated_quotient).abs();
        if unsigned_fractional_part < 0.5 {
            return truncated_quotient * increment;
        }
        if unsigned_fractional_part > 0.5 {
            return expanded_quotient * increment;
        }
    }

    match mode {
        RoundingMode::Ceil | RoundingMode::HalfCeil => (if is_negative { truncated_quotient } else { expanded_quotient }) * increment,
        RoundingMode::Floor | RoundingMode::HalfFloor => (if is_negative { expanded_quotient } else { truncated_quotient }) * increment,
        RoundingMode::Expand | RoundingMode::HalfExpand => expanded_quotient * increment,
        RoundingMode::Trunc | RoundingMode::HalfTrunc => truncated_quotient * increment,
        RoundingMode::HalfEven => (if truncated_quotient % 2.0 == 0.0 { truncated_quotient } else { expanded_quotient }) * increment,
    }
}

/// `roundNumberToIncrementInt128(x, increment, mode)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-roundnumbertoincrement
pub fn round_number_to_increment_i128(x: i128, increment: i128, mode: RoundingMode) -> i128 {
    // 1. Let quotient be x / increment.
    let quotient = x / increment;
    let remainder = x % increment;
    if remainder == 0 {
        return x;
    }
    // 2-3. Determine isNegative from x; work with abs(quotient) as unsigned quotient.
    let is_negative = x < 0;
    // 4. Let unsignedRoundingMode be GetUnsignedRoundingMode(roundingMode, isNegative).
    let unsigned_rounding_mode = get_unsigned_rounding_mode(mode, is_negative);
    // 5. Let r1 be the largest integer such that r1 ≤ abs(quotient).
    let r1 = quotient.abs();
    // 6. Let r2 be the smallest integer such that r2 > abs(quotient).
    let r2 = r1 + 1;
    // 7. Let rounded be ApplyUnsignedRoundingMode(abs(quotient), r1, r2, unsignedRoundingMode).
    let mut rounded = apply_unsigned_rounding_mode(x.abs(), increment, r1, r2, unsigned_rounding_mode);
    // 8. If isNegative is ~negative~, set rounded to -rounded.
    if is_negative {
        rounded = -rounded;
    }
    // 9. Return rounded × increment.
    rounded * increment
}

/// `validateTemporalRoundingIncrement(increment, dividend, isInclusive)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-validatetemporalroundingincrement
///
/// `dividend` é `None` quando `largestUnit <= Day` (nenhum máximo se aplica): o limite seguro passa a ser
/// `nsPerSecond`, o maior incremento válido em nanossegundos.
pub fn validate_temporal_rounding_increment(increment: f64, dividend: Option<f64>, is_inclusive: Inclusivity) -> TemporalResult<()> {
    // 1. If inclusive is true, then a. Let maximum be dividend.
    // 2. Else, b. Let maximum be dividend - 1.
    let maximum = match dividend {
        None => length_in_nanoseconds(TemporalUnit::Second) as f64,
        Some(dividend) if is_inclusive == Inclusivity::Inclusive => dividend,
        Some(dividend) if dividend > 1.0 => dividend - 1.0,
        Some(_) => 1.0,
    };

    let increment = increment.trunc();
    // 3. If increment > maximum, throw a RangeError exception.
    if increment < 1.0 || increment > maximum {
        return Err(range_error("rounding increment is out of range"));
    }
    // 4. If dividend modulo increment ≠ 0, throw a RangeError exception.
    if let Some(dividend) = dividend {
        if dividend % increment != 0.0 {
            return Err(range_error("roundingIncrement does not divide evenly"));
        }
    }
    // 5. Return ~unused~.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_to_increment_with_every_mode() {
        let ns = 1_000i128;
        // 2500 / 1000: metade exata entre 2 e 3.
        assert_eq!(round_number_to_increment_i128(2500, ns, RoundingMode::HalfExpand), 3000);
        assert_eq!(round_number_to_increment_i128(2500, ns, RoundingMode::HalfEven), 2000);
        assert_eq!(round_number_to_increment_i128(2500, ns, RoundingMode::HalfTrunc), 2000);
        assert_eq!(round_number_to_increment_i128(-2500, ns, RoundingMode::HalfExpand), -3000);
        assert_eq!(round_number_to_increment_i128(-2500, ns, RoundingMode::HalfCeil), -2000);
        assert_eq!(round_number_to_increment_i128(-2500, ns, RoundingMode::Floor), -3000);
        assert_eq!(round_number_to_increment_i128(2001, ns, RoundingMode::Ceil), 3000);
        assert_eq!(round_number_to_increment_i128(2999, ns, RoundingMode::Trunc), 2000);
        assert_eq!(round_number_to_increment_i128(3000, ns, RoundingMode::Expand), 3000);
    }

    #[test]
    fn maximum_increments() {
        assert_eq!(maximum_rounding_increment(TemporalUnit::Day), None);
        assert_eq!(maximum_rounding_increment(TemporalUnit::Hour), Some(24));
        assert_eq!(maximum_rounding_increment(TemporalUnit::Second), Some(60));
        assert_eq!(maximum_rounding_increment(TemporalUnit::Nanosecond), Some(1000));
    }

    #[test]
    fn increment_validation() {
        assert!(validate_temporal_rounding_increment(5.0, Some(60.0), Inclusivity::Exclusive).is_ok());
        assert!(validate_temporal_rounding_increment(60.0, Some(60.0), Inclusivity::Exclusive).is_err());
        assert!(validate_temporal_rounding_increment(7.0, Some(60.0), Inclusivity::Exclusive).is_err());
        assert!(validate_temporal_rounding_increment(7.0, None, Inclusivity::Exclusive).is_ok());
    }
}
