//! Porte de `runtime/temporal/core/InstantCore.{h,cpp}`: `instantToString` (`TemporalInstantToString`).
//! `maximumInstantIncrement` mora em `temporal_core_rounding.rs`, com o resto do arredondamento.
//!
//! DIVERGÊNCIA: o C++ decompõe o instante num `GregorianDateTime`; aqui a decomposição é o
//! `ul_common::time::Civil` (o mesmo calendário civil proléptico, com o dia da semana e do ano que esta função
//! não lê).

use ul_common::time::Civil;

use crate::runtime::iso8601::{format_time_zone_offset_string, ExactTime};
use crate::runtime::temporal_object::{format_seconds_string_part, PrecisionData};

const MS_PER_SECOND: i64 = 1000;
const SECONDS_PER_DAY: i64 = 86_400;

/// `instantToString(exactTime, offsetNs, precision)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-temporalinstanttostring
///
/// `offset_ns` é o deslocamento já resolvido pelo chamador (`GetOffsetNanosecondsFor`); `None` é o fuso
/// `undefined`, que escreve `Z`.
pub fn instant_to_string(exact_time: ExactTime, offset_ns: Option<i64>, precision: PrecisionData) -> String {
    // Passos 1 a 4: soma o deslocamento ao instante em milissegundos e decompõe em data e hora.
    let epoch_ms = match offset_ns {
        Some(offset) => exact_time.floor_epoch_milliseconds() + offset / ExactTime::NS_PER_MILLISECOND as i64,
        None => exact_time.floor_epoch_milliseconds(),
    };
    let days = epoch_ms.div_euclid(SECONDS_PER_DAY * MS_PER_SECOND);
    let seconds_of_day = epoch_ms.div_euclid(MS_PER_SECOND).rem_euclid(SECONDS_PER_DAY);
    let civil = Civil::from_days(days, seconds_of_day);

    // Passo 5: `ISODateTimeToString(isoDateTime, "iso8601", precision, ~never~)`, embutido.
    let mut builder = String::new();
    let mut year_length = 4;
    if civil.year > 9999 || civil.year < 0 {
        builder.push(if civil.year < 0 { '-' } else { '+' });
        year_length = 6;
    }
    builder.push_str(&format!(
        "{:0year_length$}-{:02}-{:02}T{:02}:{:02}",
        civil.year.abs(),
        civil.mon,
        civil.mday,
        civil.hour,
        civil.min
    ));

    // Passo 5.4: a fração em nanossegundos do instante, trazida para `[0, nsPerSecond)` nos instantes
    // anteriores à época.
    let mut fraction = exact_time.nanoseconds_fraction();
    if fraction < 0 {
        fraction += ExactTime::NS_PER_SECOND as i32;
    }
    format_seconds_string_part(&mut builder, civil.sec as u32, fraction as u32, precision);

    // Passos 6 e 7: `Z`, ou o deslocamento arredondado ao minuto (`FormatDateTimeUTCOffsetRounded`).
    match offset_ns {
        Some(raw_offset) => {
            let ns_per_minute = ExactTime::NS_PER_MINUTE as i64;
            let sign = if raw_offset < 0 { -1 } else { 1 };
            let minutes = (raw_offset.abs() + ns_per_minute / 2) / ns_per_minute;
            builder.push_str(&format_time_zone_offset_string(sign * minutes * ns_per_minute));
        }
        None => builder.push('Z'),
    }

    // Passo 8.
    builder
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::temporal_object::to_seconds_string_precision_record;

    #[test]
    fn utc_auto_precision() {
        let auto = to_seconds_string_precision_record(None, None);
        assert_eq!(instant_to_string(ExactTime::new(0), None, auto), "1970-01-01T00:00:00Z");
        assert_eq!(instant_to_string(ExactTime::new(1_500_000_000), None, auto), "1970-01-01T00:00:01.5Z");
    }

    #[test]
    fn before_the_epoch_keeps_the_fraction_positive() {
        let auto = to_seconds_string_precision_record(None, None);
        assert_eq!(instant_to_string(ExactTime::new(-1), None, auto), "1969-12-31T23:59:59.999999999Z");
    }

    #[test]
    fn offset_is_rounded_to_the_minute() {
        let fixed = to_seconds_string_precision_record(None, Some(3));
        let three_hours_behind = -3 * ExactTime::NS_PER_HOUR as i64;
        assert_eq!(instant_to_string(ExactTime::new(0), Some(three_hours_behind), fixed), "1969-12-31T21:00:00.000-03:00");
    }

    #[test]
    fn extended_years_carry_a_sign() {
        let minute = to_seconds_string_precision_record(Some(crate::runtime::temporal_object::TemporalUnit::Minute), None);
        assert_eq!(instant_to_string(ExactTime::new(ExactTime::MAX_VALUE), None, minute), "+275760-09-13T00:00Z");
    }
}
