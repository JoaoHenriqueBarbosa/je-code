//! `strftime` da glibc no locale C, a versão única que as crates usavam em cópia (awk, jq, git, ps,
//! tree).
//!
//! A entrada é neutra: [`StrfTime`] leva a data civil já decomposta no fuso desejado (um [`Civil`]),
//! o deslocamento em segundos a leste de UTC (`%z`) e a abreviação do fuso (`%Z`). Os segundos desde a
//! época (`%s`) vão à parte: [`strftime`] recebe o valor pronto e [`strftime_lazy`] recebe uma função,
//! chamada só quando (e cada vez que) o formato pede `%s`, para quem calcula o valor com um `mktime`
//! que guarda estado. Quem chama monta a struct do jeito que o seu fuso pede; aqui só se formata.
//!
//! A semântica é a medida no glibc 2.41 em `C.UTF-8`:
//!
//! - conversões `a A b B h c C d D e F g G H I j k l m M n p P r R s S t T u U V w W x X y Y z Z %`;
//! - flags `_ - 0 ^ #`, largura (`%10Y`) e os modificadores `E`/`O`, aceitos só nas conversões em que
//!   o glibc os aceita (`%Ed` sai literal);
//! - conversão desconhecida, ou formato terminado no meio de uma conversão, sai literal, como
//!   escrita, com a largura e a caixa que já tinham sido decididas;
//! - o ano usa a aritmética de `int` do glibc (`tm_year + 1900` estoura e dá a volta), então `%Y`,
//!   `%C` e `%G` acompanham o estouro e `%y` lê o `tm_year`;
//! - campos fora da faixa (o `jq` entrega o que o usuário escreveu no array) seguem as contas do C:
//!   nome de dia ou mês inválido vira `?`, e `%I` não normaliza horas acima de 24.

use super::Civil;

const WEEKDAYS: [&[u8]; 7] = [b"Sunday", b"Monday", b"Tuesday", b"Wednesday", b"Thursday", b"Friday", b"Saturday"];
const MONTHS: [&[u8]; 12] = [
    b"January", b"February", b"March", b"April", b"May", b"June", b"July", b"August", b"September", b"October",
    b"November", b"December",
];

/// Um instante decomposto, pronto para o [`strftime`].
#[derive(Clone, Copy, Debug)]
pub struct StrfTime<'a> {
    /// Data e hora civis no fuso do instante (`mon` de 1 a 12, `wday` 0 = domingo, `yday` de 0).
    pub civil: Civil,
    /// Deslocamento em segundos a leste de UTC, o que o `%z` imprime.
    pub gmtoff: i64,
    /// Abreviação do fuso, o que o `%Z` imprime (vazia quando o chamador não tem uma).
    pub zone: &'a [u8],
}

impl StrfTime<'_> {
    /// O `tm_year` da `struct tm` (ano menos 1900, num `int`).
    fn tm_year(&self) -> i32 {
        (self.civil.year - 1900) as i32
    }

    /// `tm_year + 1900` com o estouro do `int` que o glibc tem ao imprimir o ano.
    fn wrapped_year(&self) -> i32 {
        self.tm_year().wrapping_add(1900)
    }
}

/// Formata `t` segundo `fmt`; `epoch` são os segundos desde a época que o `%s` imprime.
pub fn strftime(fmt: &[u8], t: &StrfTime, epoch: i64) -> Vec<u8> {
    strftime_lazy(fmt, t, &mut || epoch, usize::MAX).unwrap_or_default()
}

/// Como [`strftime`], mas o `%s` chama `epoch` a cada ocorrência, e a saída passa de `limit` bytes
/// devolve `None` sem chegar a montá-la (o limite de buffer do gawk, por exemplo).
pub fn strftime_lazy(fmt: &[u8], t: &StrfTime, epoch: &mut dyn FnMut() -> i64, limit: usize) -> Option<Vec<u8>> {
    let mut out = Output { buf: Vec::new(), max: limit, overflow: false };
    format_into(&mut out, fmt, t, epoch);
    if out.overflow { None } else { Some(out.buf) }
}

struct Output {
    buf: Vec<u8>,
    max: usize,
    overflow: bool,
}

impl Output {
    fn room(&mut self, n: usize) -> bool {
        if self.overflow || self.buf.len().saturating_add(n) > self.max {
            self.overflow = true;
            return false;
        }
        true
    }

    fn push(&mut self, bytes: &[u8]) {
        if self.room(bytes.len()) {
            self.buf.extend_from_slice(bytes);
        }
    }

    fn fill(&mut self, c: u8, n: usize) {
        if self.room(n) {
            self.buf.resize(self.buf.len() + n, c);
        }
    }
}

/// Preenchimento pedido pelas flags `_` (espaço), `-` (nenhum) e `0` (zeros).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pad {
    Default,
    Space,
    None,
    Zero,
}

#[derive(Clone, Copy, Debug)]
struct Spec {
    pad: Pad,
    width: Option<usize>,
    upcase: bool,
    swap_case: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Keep,
    Upper,
    Lower,
}

/// Texto com largura: completa à esquerda com zeros se a flag for `0`, senão com espaços.
fn put_text(out: &mut Output, text: &[u8], spec: &Spec, case: Case) {
    if let Some(width) = spec.width
        && width > text.len()
    {
        out.fill(if spec.pad == Pad::Zero { b'0' } else { b' ' }, width - text.len());
    }
    match case {
        Case::Keep => out.push(text),
        Case::Upper => out.push(&text.to_ascii_uppercase()),
        Case::Lower => out.push(&text.to_ascii_lowercase()),
    }
}

/// Número com `digits` dígitos mínimos. Com zeros (o padrão), a largura vira o mínimo de dígitos,
/// depois do sinal; com `_`, espaços até `digits` e depois até a largura; com `-`, só a largura.
fn put_number(out: &mut Output, value: i64, digits: usize, spec: &Spec, default_pad: Pad) {
    let pad = if spec.pad == Pad::Default { default_pad } else { spec.pad };
    let sign: &[u8] = if value < 0 { b"-" } else { b"" };
    let magnitude = value.unsigned_abs().to_string();
    let len = sign.len() + magnitude.len();
    let mut text = Vec::with_capacity(len.max(digits));
    match pad {
        Pad::Space => {
            text.resize(digits.saturating_sub(len), b' ');
            text.extend_from_slice(sign);
            text.extend_from_slice(magnitude.as_bytes());
        }
        Pad::None => {
            text.extend_from_slice(sign);
            text.extend_from_slice(magnitude.as_bytes());
        }
        Pad::Zero | Pad::Default => {
            let total = digits.max(spec.width.unwrap_or(0));
            if !out.room(total.max(len)) {
                return;
            }
            text.extend_from_slice(sign);
            text.resize(sign.len() + total.saturating_sub(len), b'0');
            text.extend_from_slice(magnitude.as_bytes());
            out.push(&text);
            return;
        }
    }
    put_text(out, &text, &Spec { pad, ..*spec }, Case::Keep);
}

/// Conversões aceitas com cada modificador (`E`, `O`) no glibc 2.41, medidas uma a uma.
fn conversion_allowed(conv: u8, modifier: u8) -> bool {
    let plain = b"ABCDFGHIMPRSTUVWXYZabcdeghjklmnprstuwxyz%".contains(&conv);
    match modifier {
        0 => plain,
        b'E' => b"CPRTXYZcnprstuxyz%".contains(&conv),
        _ => b"BCGHIMPRSTUVWZbdeghjklmnprstuwyz%".contains(&conv),
    }
}

fn iso_week_days(yday: i64, wday: i64) -> i64 {
    // Dias desde a segunda-feira da semana 1 do ISO 8601 (a semana que contém a quinta-feira). O 378
    // é o múltiplo de 7 grande o bastante do glibc, que mantém positivo o operando do `%`.
    yday - (yday - wday + 4 + 378) % 7 + 3
}

/// Ano e semana ISO com a aritmética de `int` do glibc sobre o ano já estourado.
fn iso_week(t: &StrfTime) -> (i32, i64) {
    let (yday, wday) = (t.civil.yday, t.civil.wday);
    let leap = |year: i32| 365 + i64::from(super::is_leap(i64::from(year)));
    let mut year = t.wrapped_year();
    let mut days = iso_week_days(yday, wday);
    if days < 0 {
        year = year.wrapping_sub(1);
        days = iso_week_days(yday + leap(year), wday);
    } else {
        let next = iso_week_days(yday - leap(year), wday);
        if next >= 0 {
            year = year.wrapping_add(1);
            days = next;
        }
    }
    (year, days / 7 + 1)
}

fn format_into(out: &mut Output, fmt: &[u8], t: &StrfTime, epoch: &mut dyn FnMut() -> i64) {
    let mut i = 0;
    while i < fmt.len() && !out.overflow {
        if fmt[i] != b'%' {
            let next = fmt[i..].iter().position(|&c| c == b'%').map_or(fmt.len(), |n| i + n);
            out.push(&fmt[i..next]);
            i = next;
            continue;
        }
        let start = i;
        i += 1;
        let mut spec = Spec { pad: Pad::Default, width: None, upcase: false, swap_case: false };
        while let Some(&c) = fmt.get(i) {
            match c {
                b'_' => spec.pad = Pad::Space,
                b'-' => spec.pad = Pad::None,
                b'0' => spec.pad = Pad::Zero,
                b'^' => spec.upcase = true,
                b'#' => spec.swap_case = true,
                _ => break,
            }
            i += 1;
        }
        if fmt.get(i).is_some_and(u8::is_ascii_digit) {
            let mut width: usize = 0;
            while let Some(&c) = fmt.get(i).filter(|c| c.is_ascii_digit()) {
                width = width.saturating_mul(10).saturating_add(usize::from(c - b'0')).min(i32::MAX as usize);
                i += 1;
            }
            spec.width = Some(width);
        }
        let mut modifier = 0;
        if let Some(&c @ (b'E' | b'O')) = fmt.get(i) {
            modifier = c;
            i += 1;
        }
        let Some(&conv) = fmt.get(i) else {
            // `%` incompleto no fim: sai literal.
            let case = if spec.upcase { Case::Upper } else { Case::Keep };
            put_text(out, &fmt[start..], &spec, case);
            break;
        };
        i += 1;
        if !conversion_allowed(conv, modifier) {
            // Conversão desconhecida ou modificador recusado: o trecho sai literal, mas a caixa
            // já decidida vale (`^` em qualquer uma; `#` em nome de mês, que o glibc trata antes
            // de recusar o modificador, ao contrário do nome do dia).
            let names = b"bBh".contains(&conv);
            let case = if spec.upcase || (names && spec.swap_case) { Case::Upper } else { Case::Keep };
            put_text(out, &fmt[start..i], &spec, case);
            continue;
        }
        format_one(out, conv, &spec, t, epoch);
    }
}

/// Os três primeiros bytes de um nome (o `?` de campo inválido fica inteiro).
fn abbreviation(name: &[u8]) -> &[u8] {
    &name[..name.len().min(3)]
}

fn format_one(out: &mut Output, conv: u8, spec: &Spec, t: &StrfTime, epoch: &mut dyn FnMut() -> i64) {
    let c = &t.civil;
    let name_case = if spec.upcase || spec.swap_case { Case::Upper } else { Case::Keep };
    // O glibc não normaliza a hora: acima de 12 tira 12, só o zero vira 12.
    let hour12 = if c.hour > 12 {
        c.hour - 12
    } else if c.hour == 0 {
        12
    } else {
        c.hour
    };
    let weekday: &[u8] = usize::try_from(c.wday).ok().and_then(|i| WEEKDAYS.get(i).copied()).unwrap_or(&b"?"[..]);
    let month: &[u8] = usize::try_from(c.mon - 1).ok().and_then(|i| MONTHS.get(i).copied()).unwrap_or(&b"?"[..]);
    match conv {
        b'a' => put_text(out, abbreviation(weekday), spec, name_case),
        b'A' => put_text(out, weekday, spec, name_case),
        b'b' | b'h' => put_text(out, abbreviation(month), spec, name_case),
        b'B' => put_text(out, month, spec, name_case),
        b'c' => put_compound(out, b"%a %b %e %H:%M:%S %Y", spec, t, epoch),
        b'C' => put_number(out, i64::from(t.wrapped_year()).div_euclid(100), 1, spec, Pad::Zero),
        b'd' => put_number(out, c.mday, 2, spec, Pad::Zero),
        b'D' | b'x' => put_compound(out, b"%m/%d/%y", spec, t, epoch),
        b'e' => put_number(out, c.mday, 2, spec, Pad::Space),
        b'F' => put_compound(out, b"%Y-%m-%d", spec, t, epoch),
        b'g' => put_number(out, i64::from(iso_week(t).0).rem_euclid(100), 2, spec, Pad::Zero),
        b'G' => put_number(out, i64::from(iso_week(t).0), 1, spec, Pad::Zero),
        b'H' => put_number(out, c.hour, 2, spec, Pad::Zero),
        b'I' => put_number(out, hour12, 2, spec, Pad::Zero),
        b'j' => put_number(out, c.yday + 1, 3, spec, Pad::Zero),
        b'k' => put_number(out, c.hour, 2, spec, Pad::Space),
        b'l' => put_number(out, hour12, 2, spec, Pad::Space),
        b'm' => put_number(out, c.mon, 2, spec, Pad::Zero),
        b'M' => put_number(out, c.min, 2, spec, Pad::Zero),
        b'n' => put_text(out, b"\n", spec, Case::Keep),
        b'p' | b'P' => {
            let text: &[u8] = if c.hour < 12 { b"AM" } else { b"PM" };
            let case = if conv == b'P' || spec.swap_case { Case::Lower } else { Case::Keep };
            put_text(out, text, spec, case);
        }
        b'r' => put_compound(out, b"%I:%M:%S %p", spec, t, epoch),
        b'R' => put_compound(out, b"%H:%M", spec, t, epoch),
        b's' => put_text(out, epoch().to_string().as_bytes(), spec, Case::Keep),
        b'S' => put_number(out, c.sec, 2, spec, Pad::Zero),
        b't' => put_text(out, b"\t", spec, Case::Keep),
        b'T' | b'X' => put_compound(out, b"%H:%M:%S", spec, t, epoch),
        b'u' => put_number(out, (c.wday + 6) % 7 + 1, 1, spec, Pad::Zero),
        b'U' => put_number(out, (c.yday - c.wday + 7) / 7, 2, spec, Pad::Zero),
        b'V' => put_number(out, iso_week(t).1, 2, spec, Pad::Zero),
        b'w' => put_number(out, c.wday, 1, spec, Pad::Zero),
        b'W' => put_number(out, (c.yday - (c.wday + 6) % 7 + 7) / 7, 2, spec, Pad::Zero),
        b'y' => put_number(out, i64::from(t.tm_year()).rem_euclid(100), 2, spec, Pad::Zero),
        b'Y' => put_number(out, i64::from(t.wrapped_year()), 1, spec, Pad::Zero),
        b'z' => {
            let (sign, diff): (&[u8], i64) = if t.gmtoff < 0 { (b"-", -t.gmtoff) } else { (b"+", t.gmtoff) };
            put_text(out, sign, spec, Case::Keep);
            let minutes = diff / 60;
            put_number(out, minutes / 60 * 100 + minutes % 60, 4, spec, Pad::Zero);
        }
        b'Z' => {
            let case = if spec.swap_case {
                Case::Lower
            } else if spec.upcase {
                Case::Upper
            } else {
                Case::Keep
            };
            put_text(out, t.zone, spec, case);
        }
        _ => put_text(out, b"%", spec, Case::Keep),
    }
}

/// Conversões compostas (`%c`, `%D`, `%F`, `%r`, `%R`, `%T`, `%x`, `%X`): formata o subformato e
/// aplica a largura ao resultado inteiro; só a flag `^` passa adiante.
fn put_compound(out: &mut Output, sub: &[u8], spec: &Spec, t: &StrfTime, epoch: &mut dyn FnMut() -> i64) {
    let mut inner = Output { buf: Vec::new(), max: out.max, overflow: false };
    format_into(&mut inner, sub, t, epoch);
    if inner.overflow {
        out.overflow = true;
        return;
    }
    let case = if spec.upcase { Case::Upper } else { Case::Keep };
    put_text(out, &inner.buf, spec, case);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quarta-feira 2026-04-08 08:04:05 UTC.
    const WED: i64 = 1_775_635_445;
    /// Quinta-feira 2026-01-15 12:00:00 UTC.
    const THU: i64 = 1_768_478_400;

    fn civil_at(secs: i64, gmtoff: i64) -> Civil {
        Civil::offtime(secs, gmtoff).unwrap()
    }

    fn at(secs: i64, gmtoff: i64, zone: &str, fmt: &str) -> String {
        let t = StrfTime { civil: civil_at(secs, gmtoff), gmtoff, zone: zone.as_bytes() };
        String::from_utf8(strftime(fmt.as_bytes(), &t, secs)).unwrap()
    }

    fn gmt(secs: i64, fmt: &str) -> String {
        at(secs, 0, "GMT", fmt)
    }

    #[test]
    fn conversions_in_the_c_locale() {
        assert_eq!(gmt(WED, "%a %A %b %B %h"), "Wed Wednesday Apr April Apr");
        assert_eq!(gmt(WED, "%c"), "Wed Apr  8 08:04:05 2026");
        assert_eq!(gmt(WED, "%C %d %D %e %F"), "20 08 04/08/26  8 2026-04-08");
        assert_eq!(gmt(WED, "%g %G %H %I %j %k %l %m %M"), "26 2026 08 08 098  8  8 04 04");
        assert_eq!(gmt(WED, "%p %P %r %R %s %S %T"), "AM am 08:04:05 AM 08:04 1775635445 05 08:04:05");
        assert_eq!(gmt(WED, "%u %U %V %w %W %x %X %y %Y %z %Z %%"), "3 14 15 3 14 04/08/26 08:04:05 26 2026 +0000 GMT %");
        assert_eq!(gmt(WED, "%n%t"), "\n\t");
        assert_eq!(gmt(THU, "%a|%j|%U|%W|%V|%G|%g|%u|%w"), "Thu|015|02|02|03|2026|26|4|4");
        assert_eq!(gmt(THU + 3 * 3600, "%I|%l|%k|%p"), "03| 3|15|PM");
        assert_eq!(gmt(THU - 12 * 3600, "%I|%l|%H|%p"), "12|12|00|AM");
    }

    #[test]
    fn the_jq_conformance_line() {
        assert_eq!(
            gmt(0, "%c|%x|%r|%e|%j|%G|%V|%U|%W|%u|%C|%5Y|%-d|%_d|%^a|%#b"),
            "Thu Jan  1 00:00:00 1970|01/01/70|12:00:00 AM| 1|001|1970|01|00|00|4|19|01970|1| 1|THU|JAN"
        );
    }

    #[test]
    fn flags_width_and_bad_formats() {
        assert_eq!(gmt(WED, "[%10Y|%-d|%_H|%012z|%_z|%-3z]"), "[0000002026|8| 8|00000000000+000000000000|+   0|  +  0]");
        assert_eq!(gmt(WED, "[%^a|%#a|%#Z|%^#p|%^P|%#10Z|%010a]"), "[WED|WED|gmt|am|am|       gmt|0000000Wed]");
        assert_eq!(
            gmt(WED, "[%Ed|%OY|%Oa|%EOd|%+|%q|%10q|%5_d|%5]"),
            "[%Ed|%OY|%Oa|%EOd|%+|%q|      %10q|  %5_d|  %5]"
        );
        // Literal de conversão recusada: `^` sempre muda a caixa, `#` só no nome do mês.
        assert_eq!(gmt(WED, "[%^q|%#Eb|%#Ea|%ET|%Eu]"), "[%^Q|%#EB|%#Ea|08:04:05|3]");
        assert_eq!(gmt(WED, "[%5"), "[   %5");
        assert_eq!(gmt(WED, "[%-E"), "[%-E");
        assert_eq!(gmt(WED, "[%Ec|%EY|%Od|%Ob|%Oz]"), "[Wed Apr  8 08:04:05 2026|2026|08|Apr|+0000]");
        assert_eq!(gmt(WED, "[%012R|%^c]"), "[000000008:04|WED APR  8 08:04:05 2026]");
        assert_eq!(gmt(WED, "%"), "%");
        assert_eq!(gmt(WED, "100%%|a%-"), "100%|a%-");
        assert_eq!(gmt(WED, "é%Y"), "é2026");
        // Byte que não é UTF-8 depois do `%` segue adiante intacto.
        let t = StrfTime { civil: civil_at(WED, 0), gmtoff: 0, zone: b"GMT" };
        assert_eq!(strftime(b"%\xc3\xa9", &t, WED), b"%\xc3\xa9");
        // O `-` não tira a largura pedida: só tira o zero e o espaço do preenchimento padrão.
        assert_eq!(gmt(WED, "%-10A|%-5d|%-d"), " Wednesday|    8|8");
    }

    #[test]
    fn epoch_is_lazy_and_output_is_bounded() {
        let t = StrfTime { civil: civil_at(0, 0), gmtoff: 0, zone: b"GMT" };
        let mut calls = 0;
        let mut epoch = || {
            calls += 1;
            -1
        };
        assert_eq!(strftime_lazy(b"%H", &t, &mut epoch, usize::MAX).unwrap(), b"00");
        assert_eq!(strftime_lazy(b"[%05s|%5s]", &t, &mut epoch, usize::MAX).unwrap(), b"[000-1|   -1]");
        assert_eq!(calls, 2);
        assert_eq!(strftime_lazy(b"%8191d", &t, &mut || 0, 8191).unwrap().len(), 8191);
        assert!(strftime_lazy(b"%8192d", &t, &mut || 0, 8191).is_none());
        assert!(strftime_lazy(b"%8192c", &t, &mut || 0, 8191).is_none());
    }

    #[test]
    fn iso_weeks_and_years() {
        // 2021-01-01 (sexta) ainda é a semana 53 de 2020.
        assert_eq!(gmt(1_609_459_200, "%G-%V-%g-%u"), "2020-53-20-5");
        assert_eq!(gmt(1_609_459_200, "%U|%W"), "00|00");
        // 2024-12-30 (segunda) já é a semana 1 de 2025.
        assert_eq!(gmt(1_735_516_800, "%G-%V"), "2025-01");
        // 2016-01-03 (domingo) é a semana 53 de 2015.
        assert_eq!(gmt(1_451_779_200, "%G-%V-%u"), "2015-53-7");
        // 2026 começa numa quinta e tem 53 semanas.
        assert_eq!(gmt(1_798_675_200, "%G-%V"), "2026-53");
    }

    #[test]
    fn offset_and_zone() {
        assert_eq!(at(THU, -10_800, "-03", "%H|%z|%Z|%s"), "09|-0300|-03|1768478400");
        assert_eq!(at(THU, 19_800, "IST", "%H:%M|%z"), "17:30|+0530");
        assert_eq!(at(THU, -1_800, "", "%z|%Z|"), "-0030||");
        assert_eq!(at(THU, 0, "est", "%^Z"), "EST");
    }

    #[test]
    fn extreme_years() {
        assert_eq!(gmt(-62_167_219_200, "%Y %C %y %G %g %F"), "0 0 00 -1 99 0-01-01");
        assert_eq!(gmt(-62_167_219_201, "%Y %C %y %3Y %12Y"), "-1 -1 99 -01 -00000000001");
        assert_eq!(
            gmt(67_768_036_191_676_792, "%Y|%C|%y|%G|%g"),
            "-2147481749|-21474818|47|-2147481748|52"
        );
        assert_eq!(gmt(-1, "%Y|%y|%C|%j|%a|%s"), "1969|69|19|365|Wed|-1");
    }

    #[test]
    fn out_of_range_fields_follow_the_c_arithmetic() {
        let mut civil = civil_at(0, 0);
        civil.mon = 13;
        civil.wday = 8;
        civil.hour = 25;
        let t = StrfTime { civil, gmtoff: 0, zone: b"GMT" };
        assert_eq!(strftime(b"%a|%B|%u|%I|%p", &t, 0), b"?|?|1|13|PM");
        civil.mon = 0;
        civil.wday = -1;
        civil.hour = -1;
        let t = StrfTime { civil, gmtoff: 0, zone: b"GMT" };
        assert_eq!(strftime(b"%A|%b|%u|%I|%p", &t, 0), b"?|?|6|-1|AM");
    }
}
