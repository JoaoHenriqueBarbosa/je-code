//! Calendários não gregorianos do `Intl.DateTimeFormat` (`calendar`, `-u-ca-`): a conversão do instante para
//! ano, mês, dia e era vem do `icu_calendar` (icu4x, dados compilados); os nomes de mês, era e ano cíclico vêm de
//! `intl_calendar_names` (medidos no bun por `scripts/gen-calendar-golden.js`).
//!
//! O que não está aqui: o padrão de cada locale para cada calendário (a ordem e a pontuação dos campos seguem a
//! do gregoriano do locale), e as eras japonesas anteriores a Meiji (o icu4x só tem as modernas).
use icu_calendar::{AnyCalendar, AnyCalendarKind, Date};
use ul_common::time::Civil;

use crate::runtime::intl_calendar_names as names;

/// O calendário pedido que o porte converte: `kind` é do icu4x e `name` o `calendar` do `resolvedOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NativeCalendar {
    kind: Option<AnyCalendarKind>,
    name: &'static str,
}

/// (valor pedido, calendário do icu4x, valor do `resolvedOptions().calendar`). `islamic` resolve para
/// `islamic-tbla` no bun.
const SUPPORTED: [(&str, AnyCalendarKind, &str); 15] = [
    ("buddhist", AnyCalendarKind::Buddhist, "buddhist"),
    ("chinese", AnyCalendarKind::Chinese, "chinese"),
    ("coptic", AnyCalendarKind::Coptic, "coptic"),
    ("dangi", AnyCalendarKind::Dangi, "dangi"),
    ("ethiopic", AnyCalendarKind::Ethiopian, "ethiopic"),
    ("ethioaa", AnyCalendarKind::EthiopianAmeteAlem, "ethioaa"),
    ("hebrew", AnyCalendarKind::Hebrew, "hebrew"),
    ("indian", AnyCalendarKind::Indian, "indian"),
    ("islamic", AnyCalendarKind::HijriTabularTypeIIThursday, "islamic-tbla"),
    ("islamic-civil", AnyCalendarKind::HijriTabularTypeIIFriday, "islamic-civil"),
    ("islamic-tbla", AnyCalendarKind::HijriTabularTypeIIThursday, "islamic-tbla"),
    ("islamic-umalqura", AnyCalendarKind::HijriUmmAlQura, "islamic-umalqura"),
    ("japanese", AnyCalendarKind::Japanese, "japanese"),
    ("persian", AnyCalendarKind::Persian, "persian"),
    ("roc", AnyCalendarKind::Roc, "roc"),
];

/// A data de um instante num calendário, com os nomes já escolhidos (o índice é a largura: longo, curto, estreito).
pub struct NativeDate {
    /// O ano da era (`6` em Reiwa 6, `2567` no budista); o ano do ciclo (1 a 60) nos calendários cíclicos.
    pub year: i64,
    /// O mês numérico (a posição no ano no hebraico, o número do mês nos demais).
    pub month_number: i64,
    /// O mês como o ICU o escreve em `month: "numeric"`: `2bis` no mês bissexto chinês.
    pub month_text: String,
    pub day: i64,
    /// `relatedYear` (calendários cíclicos): o ano gregoriano em que o ano chinês começa.
    pub related_year: Option<i64>,
    pub year_name: Option<&'static str>,
    pub month_names: [Option<&'static str>; 3],
    /// O nome do mês junto do ano (`d MMMM y`), quando difere do com dia (`fa`: `مهٔ` e `مه`); o token `{month:long:year}`.
    pub month_names_with_year: [Option<&'static str>; 3],
    /// O `ja` escreve o ano 1 do calendário japonês como `元` quando o padrão põe `年` logo depois do ano (o ICU
    /// aplica o numbering `jpanyear` ao `y` seguido de `年`, em qualquer largura do ano).
    pub gannen: bool,
    pub era_names: [Option<&'static str>; 3],
}

/// As partes de `date` formatada com `pattern` (de `intl_calendar_patterns`): literais e tokens `{tipo:argumento}`.
/// `weekday` devolve o nome do dia da semana na largura pedida (`long`, `short`, `narrow`), que não depende do
/// calendário; o segundo argumento é `true` no token `{weekday:largura:s}` (forma isolada). Um nome que a tabela de nomes não tem (era antiga, mês sem nome) cai no número, como o ICU.
/// Os tokens de hora (`hour`, `minute`, `second`, `dayPeriod`, `dayPeriodFlex`, `tz`) não são do calendário: `time`
/// recebe `(tipo, argumento)` e devolve a parte pronta.
pub fn render_pattern(
    pattern: &str,
    date: &NativeDate,
    weekday: &dyn Fn(&str, bool) -> String,
    time: &dyn Fn(&str, &str) -> Option<(String, String)>,
) -> Vec<(String, String)> {
    let width_index = |argument: &str| match argument {
        "short" => 1,
        "narrow" => 2,
        _ => 0,
    };
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut literal = String::new();
    let mut rest = pattern;
    while let Some(character) = rest.chars().next() {
        if character != '{' {
            literal.push(character);
            rest = &rest[character.len_utf8()..];
            continue;
        }
        let end = rest.find('}').unwrap_or(rest.len());
        if !literal.is_empty() {
            parts.push(("literal".to_string(), std::mem::take(&mut literal)));
        }
        let mut fields = rest[1..end].split(':');
        let kind = fields.next().unwrap_or("");
        let argument = fields.next().unwrap_or("");
        let two_digit = argument == "2-digit";
        let part = match kind {
            "weekday" => Some(("weekday", weekday(argument, fields.next() == Some("s")))),
            "month" if matches!(argument, "long" | "short" | "narrow") => {
                let index = width_index(argument);
                let dated = if fields.next() == Some("year") { date.month_names_with_year[index] } else { None };
                Some(("month", dated.or(date.month_names[index]).map_or_else(|| date.month_text.clone(), str::to_string)))
            }
            "month" if two_digit && !date.month_text.ends_with("bis") => Some(("month", format!("{:02}", date.month_number))),
            "month" => Some(("month", date.month_text.clone())),
            "day" => Some(("day", if two_digit { format!("{:02}", date.day) } else { date.day.to_string() })),
            "year" if date.gannen && date.year == 1 && rest[end + 1..].starts_with('年') => Some(("year", "元".to_string())),
            "year" | "relatedYear" => {
                let year = if kind == "relatedYear" { date.related_year.unwrap_or(date.year) } else { date.year };
                let value = if two_digit { format!("{:02}", year.rem_euclid(100)) } else { year.to_string() };
                Some((if kind == "year" { "year" } else { "relatedYear" }, value))
            }
            "yearName" => date.year_name.map(|name| ("yearName", name.to_string())),
            "era" => date.era_names[width_index(argument)].map(|name| ("era", name.to_string())),
            "hour" | "minute" | "second" | "dayPeriod" | "dayPeriodFlex" | "tz" => {
                parts.extend(time(kind, argument));
                None
            }
            _ => None,
        };
        if let Some((name, value)) = part {
            parts.push((name.to_string(), value));
        }
        rest = &rest[(end + 1).min(rest.len())..];
    }
    if !literal.is_empty() {
        parts.push(("literal".to_string(), literal));
    }
    parts
}

impl NativeCalendar {
    /// O calendário que o porte converte, ou `None` (gregoriano, `iso8601` e o que o ICU não conhece).
    pub fn parse(requested: &str) -> Option<NativeCalendar> {
        SUPPORTED
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(requested))
            .map(|&(_, kind, resolved)| NativeCalendar { kind: Some(kind), name: resolved })
    }

    /// Valores de `calendar` que o ICU reconhece mas o porte formata como gregoriano (o `locale` resolvido
    /// ainda leva `-u-ca-islamic-rgsa`, medido no bun).
    pub fn is_recognized_without_data(requested: &str) -> bool {
        requested.eq_ignore_ascii_case("islamic-rgsa")
    }

    pub fn is_native(&self) -> bool {
        self.kind.is_some()
    }

    /// O `calendar` do `resolvedOptions` (vazio no gregoriano).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Converte o dia civil gregoriano de `civil` e escolhe os nomes da língua de `locale`.
    pub fn date(&self, locale: &str, civil: &Civil) -> Option<NativeDate> {
        let kind = self.kind?;
        let iso = Date::try_new_iso(i32::try_from(civil.year).ok()?, u8::try_from(civil.mon).ok()?, u8::try_from(civil.mday).ok()?).ok()?;
        let date = iso.to_calendar(AnyCalendar::new(kind));
        let hebrew = self.name == "hebrew";
        let month = date.month();
        let leap = month.to_input().is_leap();
        let month_number = if hebrew { i64::from(month.ordinal) } else { i64::from(month.number()) };
        let month_text = if leap && !hebrew { format!("{month_number}bis") } else { month_number.to_string() };
        let month_key = if hebrew && date.months_in_year() == 13 { format!("{month_number}L") } else { month_text.clone() };
        let year_info = date.year();
        let language = locale.split('-').next().unwrap_or("en");
        let name_of = |kind: &str, key: &str| -> Option<&'static str> {
            names::lookup(language, self.name_key(), kind, key).or_else(|| names::lookup("en", self.name_key(), kind, key))
        };
        let widths = |kinds: [&str; 3], key: &str| kinds.map(|kind| name_of(kind, key));
        let (year, related_year, year_name) = match year_info.cyclic() {
            // O `y` do calendário cíclico é o ano do ciclo (1 a 60, `year=41` no `th` e no `zh`); `r` é o relacionado.
            Some(cyclic) => (
                i64::from(cyclic.year),
                Some(i64::from(cyclic.related_iso)),
                name_of("yn", &cyclic.year.to_string()),
            ),
            None => (i64::from(year_info.era().map_or(date.extended_year(), |era| era.year)), None, None),
        };
        let era_names = match year_info.era() {
            Some(era) => widths(["el", "es", "en"], era.era.as_str()),
            None => [None; 3],
        };
        Some(NativeDate {
            year,
            month_number,
            month_text,
            day: i64::from(date.day_of_month().0),
            related_year,
            year_name,
            month_names: widths(["ml", "ms", "mn"], &month_key),
            month_names_with_year: widths(["mly", "msy", "mny"], &month_key),
            gannen: self.name == "japanese" && language == "ja",
            era_names,
        })
    }

    /// O calendário nas tabelas de nomes (o `chinese` e o `dangi` têm tabelas próprias; as variantes islâmicas,
    /// também).
    fn name_key(&self) -> &'static str {
        SUPPORTED.iter().find(|(_, _, resolved)| *resolved == self.name).map_or(self.name, |&(requested, _, _)| requested)
    }
}
