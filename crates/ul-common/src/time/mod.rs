//! Calendário gregoriano proléptico e fusos horários, a versão única que os programas usavam em
//! cópia (jq, awk, git, shell, sqlite, archive, diff, net, misc, host).
//!
//! - Dias desde 1970-01-01 e data civil se convertem pelo algoritmo de Howard Hinnant, exato para
//!   qualquer `i64` de segundos (o dia sempre cabe num `i64`).
//! - [`Civil`] decompõe um instante UTC (ou um instante já deslocado pelo fuso) em ano, mês, dia,
//!   hora, minuto, segundo, dia da semana e dia do ano; quem precisa de uma `struct tm` própria (com
//!   `isdst`, `gmtoff`, abreviação, ano menos 1900) monta a sua em cima dele.
//! - `strftime` formata uma data civil como o glibc em `C.UTF-8` (flags, largura, `E`/`O`), a partir
//!   de um [`strftime::StrfTime`] neutro.
//! - `zone` (feature `zone`) resolve o fuso do sandbox como a glibc e converte com o banco de fusos
//!   embutido do jiff.

pub mod strftime;
#[cfg(feature = "zone")]
pub mod zone;

/// Nanossegundos desde a época no `CLOCK_REALTIME` do pseudo-processo (o mesmo relógio do `date`, que o
/// kernel do sandbox e o `TestKit` controlam). Fora de um pseudo-processo (testes de uma crate isolada),
/// o relógio do host.
pub fn realtime_nanos() -> i128 {
    sys_clock_nanos(sysabi::Clock::Realtime).unwrap_or_else(|| match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    })
}

/// Microssegundos de 1601 (a época do tempo WebKit, a do Chromium) até 1970.
pub const WEBKIT_EPOCH_US: i64 = 11_644_473_600_000_000;

/// O relógio de parede no tempo WebKit: microssegundos desde 1601.
pub fn webkit_now_micros() -> i64 {
    (realtime_nanos() / 1000) as i64 + WEBKIT_EPOCH_US
}

/// Nanossegundos no `CLOCK_MONOTONIC` do pseudo-processo (o tempo desde o boot do sandbox). Fora de um
/// pseudo-processo, o tempo desde a primeira leitura.
pub fn monotonic_nanos() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    sys_clock_nanos(sysabi::Clock::Monotonic).unwrap_or_else(|| START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as i128) as u64
}

fn sys_clock_nanos(clock: sysabi::Clock) -> Option<i128> {
    let spec = sysabi::sys::try_current()?.clock_gettime(clock).ok()?;
    Some(i128::from(spec.sec) * 1_000_000_000 + i128::from(spec.nsec))
}

/// Dorme `duration` no relógio do pseudo-processo (`nanosleep`, que cede a vez e entrega sinais); fora de um
/// sandbox, dorme a thread do host.
pub fn sleep(duration: std::time::Duration) {
    match sysabi::sys::try_current() {
        Some(sys) => {
            let _ = sys.nanosleep(duration);
        }
        None => std::thread::sleep(duration),
    }
}

/// Segundos num dia.
pub const SECS_PER_DAY: i64 = 86_400;

/// Ano bissexto do calendário gregoriano.
pub fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Dias do mês `month` (1 a 12) do ano `year`.
pub fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

/// Dias desde 1970-01-01 da data civil `year`-`month`-`day` (mês de 1 a 12; o dia pode sair do
/// mês e transborda para os seguintes).
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverso de [`days_from_civil`]: (ano, mês de 1 a 12, dia).
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Dia da semana de um número de dias desde a época (0 = domingo; 1970-01-01 foi quinta).
pub fn weekday(days: i64) -> i64 {
    (days + 4).rem_euclid(7)
}

/// Data e hora civis decompostas, no fuso em que os segundos foram dados (UTC se não houve
/// deslocamento).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    /// De 1 a 12.
    pub mon: i64,
    pub mday: i64,
    pub hour: i64,
    pub min: i64,
    pub sec: i64,
    /// 0 = domingo.
    pub wday: i64,
    /// De 0 a 365.
    pub yday: i64,
}

impl Civil {
    /// De um número de dias desde a época e dos segundos já transcorridos no dia (0 a 86399).
    pub fn from_days(days: i64, secs_of_day: i64) -> Civil {
        let (year, mon, mday) = civil_from_days(days);
        Civil {
            year,
            mon,
            mday,
            hour: secs_of_day / 3600,
            min: secs_of_day % 3600 / 60,
            sec: secs_of_day % 60,
            wday: weekday(days),
            yday: days - days_from_civil(year, 1, 1),
        }
    }

    /// De segundos desde a época (negativos valem, antes de 1970).
    pub fn from_secs(secs: i64) -> Civil {
        Civil::from_days(secs.div_euclid(SECS_PER_DAY), secs.rem_euclid(SECS_PER_DAY))
    }

    /// O `__offtime` da glibc: `t + off` em data civil, ou `None` quando o ano menos 1900 não cabe
    /// num `int` (o `EOVERFLOW` do original). A soma é em 128 bits, porque `t + off` pode passar
    /// do `i64`.
    pub fn offtime(t: i64, off: i64) -> Option<Civil> {
        let total = i128::from(t) + i128::from(off);
        let days = i64::try_from(total.div_euclid(i128::from(SECS_PER_DAY))).ok()?;
        let rem = i64::try_from(total.rem_euclid(i128::from(SECS_PER_DAY))).ok()?;
        let c = Civil::from_days(days, rem);
        i32::try_from(c.year - 1900).ok()?;
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for d in [-1_000_000i64, -146_097, -719_469, -1, 0, 1, 59, 60, 19_000, 2_932_896, 100_000_000_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d, "dia {d}");
        }
        // Todo dia de 1600 a 2500 volta para si mesmo e avança um dia por vez.
        let mut prev = days_from_civil(1600, 1, 1) - 1;
        for d in days_from_civil(1600, 1, 1)..days_from_civil(2500, 1, 1) {
            assert_eq!(d, prev + 1);
            prev = d;
            let (y, m, dd) = civil_from_days(d);
            assert!((1..=12).contains(&m) && (1..=days_in_month(y, m)).contains(&dd));
            assert_eq!(days_from_civil(y, m, dd), d);
        }
    }

    #[test]
    fn known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(1, 1, 1), -719_162);
        assert_eq!(days_from_civil(0, 3, 1), -719_468);
        assert_eq!(civil_from_days(-719_468), (0, 3, 1));
        assert_eq!(civil_from_days(-719_469), (0, 2, 29));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // O dia que sai do mês transborda para o seguinte.
        assert_eq!(days_from_civil(2026, 2, 30), days_from_civil(2026, 3, 2));
    }

    #[test]
    fn leap_years_and_month_lengths() {
        assert!(is_leap(2000) && is_leap(2024) && is_leap(0) && is_leap(-4) && is_leap(-400));
        assert!(!is_leap(1900) && !is_leap(2023) && !is_leap(2100) && !is_leap(-100) && !is_leap(-1));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn weekday_and_fields() {
        assert_eq!(weekday(0), 4);
        assert_eq!(weekday(-1), 3);
        assert_eq!(weekday(-4), 0);
        let c = Civil::from_secs(1_700_000_000);
        assert_eq!((c.year, c.mon, c.mday, c.hour, c.min, c.sec, c.wday, c.yday), (2023, 11, 14, 22, 13, 20, 2, 317));
        // Antes de 1970 os segundos negativos recuam para o dia anterior.
        let c = Civil::from_secs(-1);
        assert_eq!((c.year, c.mon, c.mday, c.hour, c.min, c.sec, c.wday, c.yday), (1969, 12, 31, 23, 59, 59, 3, 364));
        let c = Civil::from_secs(-62_135_596_800);
        assert_eq!((c.year, c.mon, c.mday, c.wday, c.yday), (1, 1, 1, 1, 0));
        // 29 de fevereiro de 2024 é o dia 59 (de zero) de um ano bissexto.
        let c = Civil::from_days(days_from_civil(2024, 2, 29), 0);
        assert_eq!((c.yday, c.wday), (59, 4));
        // Extremos do `i64` não estouram.
        let c = Civil::from_secs(i64::MIN);
        assert_eq!(days_from_civil(c.year, c.mon, c.mday), i64::MIN.div_euclid(SECS_PER_DAY));
        let c = Civil::from_secs(i64::MAX);
        assert_eq!(days_from_civil(c.year, c.mon, c.mday), i64::MAX.div_euclid(SECS_PER_DAY));
    }

    #[test]
    fn offtime_limits() {
        let c = Civil::offtime(0, 0).unwrap();
        assert_eq!((c.year, c.mon, c.mday, c.wday, c.yday), (1970, 1, 1, 4, 0));
        assert_eq!(Civil::offtime(0, -3 * 3600).unwrap().hour, 21);
        // O ano menos 1900 tem que caber num `i32`.
        assert!(Civil::offtime(i64::MIN, 0).is_none());
        assert!(Civil::offtime(i64::MAX, 0).is_none());
        assert!(Civil::offtime(i64::MAX, i64::MAX).is_none());
        let c = Civil::offtime(-62_135_596_800, 0).unwrap();
        assert_eq!((c.year, c.mon, c.mday), (1, 1, 1));
        // Os limites do ano que cabe no `int` da `struct tm` (medidos no glibc).
        assert!(Civil::offtime(67_768_036_191_676_792, 0).is_some());
        assert!(Civil::offtime(67_768_036_191_676_800, 0).is_none());
    }
}
