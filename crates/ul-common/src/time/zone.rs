//! Fuso horário local do sandbox e formatação de datas pelo `jiff`, sem tocar o host.
//!
//! Resolve como o glibc: `TZ` do ambiente (ou, sem `TZ`, o fuso do sandbox via `local_timezone`);
//! vazio ou `UTC` é UTC; `:` inicial é ignorado; caminho absoluto lê o TZif do sistema de arquivos do
//! sandbox; nome lê `/usr/share/zoneinfo/<nome>` do sandbox e, na falta, o banco embutido do jiff (a
//! mesma tzdata 2026c do oráculo); o resto é tentado como regra POSIX (`EST5EDT,M3.2.0,M11.1.0`). Um
//! valor que não resolve vira UTC, como no glibc.

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::{TimeZone, TimeZoneDatabase};
use sysabi::sys;

use super::strftime::StrfTime;
use super::{Civil, days_from_civil};

/// Fuso local do processo corrente.
pub fn local() -> TimeZone {
    let spec = match sys::try_current() {
        Some(s) => s.getenv(b"TZ").unwrap_or_else(|| s.local_timezone()),
        None => Vec::new(),
    };
    from_spec(&spec)
}

/// Fuso a partir de um valor no formato da variável `TZ`.
pub fn from_spec(spec: &[u8]) -> TimeZone {
    let spec = spec.strip_prefix(b":").unwrap_or(spec);
    let Ok(s) = std::str::from_utf8(spec) else { return TimeZone::UTC };
    if s.is_empty() || s == "UTC" {
        return TimeZone::UTC;
    }
    if s.starts_with('/') {
        return read_tzif(s, spec).unwrap_or(TimeZone::UTC);
    }
    let safe_name = !s.split('/').any(|c| c == "..");
    if safe_name {
        let path = format!("/usr/share/zoneinfo/{s}");
        if let Some(tz) = read_tzif(s, path.as_bytes()) {
            return tz;
        }
        if let Ok(tz) = TimeZoneDatabase::bundled().get(s) {
            return tz;
        }
    }
    TimeZone::posix(s).unwrap_or(TimeZone::UTC)
}

fn read_tzif(name: &str, path: &[u8]) -> Option<TimeZone> {
    sys::try_current()?;
    let data = sys::read_file(path).ok()?;
    TimeZone::tzif(name, &data).ok()
}

/// Formata um instante (segundos e nanossegundos desde a época) com `strftime` do jiff no fuso dado.
/// Fora da faixa do jiff, cai pra época.
pub fn format(sec: i64, nsec: u32, tz: &TimeZone, fmt: &str) -> String {
    let ts = Timestamp::new(sec, nsec as i32).unwrap_or(Timestamp::UNIX_EPOCH);
    ts.to_zoned(tz.clone()).strftime(fmt).to_string()
}

/// Campos civis (ano, mês, dia, hora, minuto, segundo) de um instante no fuso dado.
pub fn civil(sec: i64, tz: &TimeZone) -> (i64, u32, u32, u32, u32, u32) {
    let ts = Timestamp::from_second(sec).unwrap_or(Timestamp::UNIX_EPOCH);
    let z = ts.to_zoned(tz.clone());
    (z.year() as i64, z.month() as u32, z.day() as u32, z.hour() as u32, z.minute() as u32, z.second() as u32)
}

/// Uma data e hora do jiff como [`Civil`] (com dia da semana e dia do ano).
pub fn civil_of(dt: &DateTime) -> Civil {
    Civil {
        year: i64::from(dt.year()),
        mon: i64::from(dt.month()),
        mday: i64::from(dt.day()),
        hour: i64::from(dt.hour()),
        min: i64::from(dt.minute()),
        sec: i64::from(dt.second()),
        wday: i64::from(dt.weekday().to_sunday_zero_offset()),
        yday: i64::from(dt.day_of_year()) - 1,
    }
}

/// `strftime` da glibc de um instante (segundos desde a época) no fuso dado: `%z` e `%Z` vêm do
/// deslocamento e da abreviação do fuso naquele instante. Fora da faixa do jiff, o instante cai
/// pra época (o `%s` continua mostrando `sec`).
pub fn strftime(fmt: &[u8], sec: i64, tz: &TimeZone) -> Vec<u8> {
    let ts = Timestamp::from_second(sec).unwrap_or(Timestamp::UNIX_EPOCH);
    let info = tz.to_offset_info(ts);
    let when = StrfTime {
        civil: civil_of(&tz.to_datetime(ts)),
        gmtoff: i64::from(info.offset().seconds()),
        zone: info.abbreviation().as_bytes(),
    };
    super::strftime::strftime(fmt, &when, sec)
}

/// Deslocamento UTC em segundos de um instante no fuso dado.
pub fn offset_seconds(sec: i64, tz: &TimeZone) -> i32 {
    let ts = Timestamp::from_second(sec).unwrap_or(Timestamp::UNIX_EPOCH);
    tz.to_offset(ts).seconds()
}

/// `mktime` da glibc com `tm_isdst = -1`: os campos podem sair da faixa (mês 13, dia 0, segundo 62)
/// e são normalizados; a hora local vira instante no fuso dado. Numa lacuna de horário de verão vale
/// o deslocamento de antes, e numa sobreposição, o primeiro dos dois instantes.
pub fn mktime(year: i64, mon0: i64, mday: i64, hour: i64, min: i64, sec: i64, tz: &TimeZone) -> i64 {
    let y = year + mon0.div_euclid(12);
    let m = mon0.rem_euclid(12);
    let naive = (days_from_civil(y, m + 1, 1) + mday - 1) * 86_400 + hour * 3600 + min * 60 + sec;
    let Ok(ts) = Timestamp::from_second(naive) else { return naive };
    let dt = ts.to_zoned(TimeZone::UTC).datetime();
    match tz.to_ambiguous_zoned(dt).compatible() {
        Ok(z) => z.timestamp().as_second(),
        Err(_) => naive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_and_named_zones() {
        let utc = from_spec(b"UTC");
        assert_eq!(format(1_768_478_400, 0, &utc, "%Y-%m-%d %H:%M:%S.%N %z"), "2026-01-15 12:00:00.000000000 +0000");
        let sp = from_spec(b"America/Sao_Paulo");
        assert_eq!(format(1_768_478_400, 5, &sp, "%Y-%m-%d %H:%M:%S.%N %z"), "2026-01-15 09:00:00.000000005 -0300");
        let posix = from_spec(b"EST5EDT,M3.2.0,M11.1.0");
        assert_eq!(offset_seconds(1_768_478_400, &posix), -5 * 3600);
        assert_eq!(civil(0, &utc), (1970, 1, 1, 0, 0, 0));
        assert_eq!(mktime(2026, 0, 15, 12, 0, 0, &utc), 1_768_478_400);
        assert_eq!(mktime(2025, 12, 15, 12, 0, 0, &utc), 1_768_478_400);
        assert_eq!(mktime(2026, 0, 15, 9, 0, 0, &sp), 1_768_478_400);
    }

    #[test]
    fn strftime_uses_the_zone_offset_and_abbreviation() {
        let sp = from_spec(b"America/Sao_Paulo");
        let out = strftime(b"%F %T %z %Z %s %j %a", 1_768_478_400, &sp);
        assert_eq!(out, b"2026-01-15 09:00:00 -0300 -03 1768478400 015 Thu");
        let utc = from_spec(b"UTC");
        assert_eq!(strftime(b"%c|%Z|%z", 0, &utc), b"Thu Jan  1 00:00:00 1970|UTC|+0000");
        let dt = jiff::civil::date(2024, 12, 30).at(0, 0, 0, 0);
        let c = civil_of(&dt);
        assert_eq!((c.year, c.mon, c.mday, c.wday, c.yday), (2024, 12, 30, 1, 364));
    }

    #[test]
    fn unresolvable_specs_become_utc() {
        assert_eq!(offset_seconds(0, &from_spec(b"")), 0);
        assert_eq!(offset_seconds(0, &from_spec(b":UTC")), 0);
        assert_eq!(offset_seconds(0, &from_spec(b"../etc/passwd")), 0);
        assert_eq!(offset_seconds(0, &from_spec(&[0xff, 0xfe])), 0);
    }
}
