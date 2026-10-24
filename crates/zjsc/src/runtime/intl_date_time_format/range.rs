//! `Intl.DateTimeFormat.prototype.formatRange` e `formatRangeToParts` sem o `DateIntervalFormat` do ICU
//! (`IntlDateTimeFormat::formatRange`, `formatRangeToParts`, `prepareDateRange`,
//! `buildFormattedDateIntervalParts`).
//!
//! O ICU (`DateIntervalFormat::formatImpl`) acha o maior campo do calendário que difere entre os dois
//! instantes (era, ano, mês, dia, AM/PM, hora, minuto, segundo; os milissegundos não contam):
//!
//! - nenhum, ou um campo mais fino que o mais fino do padrão: os extremos são "praticamente iguais" e o
//!   resultado é o `format` do início, todo `shared` (`dateFieldsPracticallyEqual`);
//! - há padrão de intervalo do CLDR para a combinação (`yMMMd` com `d`: `MMM d - d, y`): o trecho que
//!   difere sai nos dois lados e o resto uma vez, `shared`;
//! - não há: o `intervalFormatFallback` com as duas datas inteiras. No mesmo dia, com data e hora, a data
//!   sai uma vez e a hora faz o intervalo.
//!
//! As origens (`source`) seguem o `UFIELD_CATEGORY_DATE_INTERVAL_SPAN`: a primeira data é `startRange`, a
//! segunda `endRange`, o separador e o que se repete só uma vez `shared`.
//!
//! LACUNAS: os padrões de intervalo são os de `en` e `pt`, de memória do CLDR, para as combinações
//! comuns (data numérica, com mês por extenso e `weekday`, `hour` e `hour`+`minute`); as outras (era,
//! segundos, fuso, `dayPeriod`, `weekday` sem dia) caem no fallback, que no ICU às vezes tem padrão
//! próprio. O fallback do português é `{0} - {1}` e os padrões de intervalo usam o traço de faixa
//! (U+2013) com espaços; nada disso foi conferido contra o bun.
//!
//! Fuso (`timeZoneName`, medido no bun em 65 locales): com a data diferente ou com segundos o fallback repete o fuso nas
//! duas pontas (`zone_date_range`, `data_range` com os cenários `zone_*_seconds`); sem segundos o fuso sai uma vez, no
//! lugar que o molde do locale diz (`zone_wrap`: ` GMT-3` depois em de, `(GMT-3)` em ja, `GMT-3 ` antes em zh). Com
//! `dateStyle` e `timeStyle: "short"` a hora é a do intervalo da hora sozinha (`styled_short_time_range`, ` Uhr` em de).
//! Famílias que o ICU escreve de outro jeito (`x` em `zone_date_*|ends`, molde sem acordo entre os estilos de fuso) caem
//! no caminho antigo.
//!
//! Calendário `iso8601` (`PlainYearMonth`, `PlainMonthDay`): sem padrão de intervalo, vale o fallback da raiz com os
//! dois `format` inteiros (`iso_date_range`), igual em todo locale. Com hora vale `iso_time_range`. Com o mês por extenso
//! (`dateStyle` medium, long e full, `month` com `day`, `weekday`, `era`) vale o molde de intervalo da raiz
//! (`iso_interval_range`: `2024  5\u{2013}7`, `2024  5 \u{2013}  7`, `2024  5, sexta-feira \u{2013}  7, domingo`, com a era
//! `" 2024  5\u{2013}7"`; o traço é o U+2013 e o mês vazio deixa dois espaços).
//!
//! Calendário não gregoriano (inclusive o padrão de `th` e `fa`): `data_range` lê `intl_calendar_range`, medida
//! por locale e calendário, e a maior diferença sai dos campos do próprio calendário. Ainda sem medida: era, fuso
//! e `dayPeriod` nesses calendários (`special_range` desiste) e o fallback de `date_range`, que formata gregoriano.

use super::*;
use crate::runtime::intl_support::RangeSource;

/// Os campos do calendário que o `DateIntervalFormat` compara, do maior para o menor.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Level {
    Era,
    Year,
    Month,
    Day,
    DayPeriod,
    Hour,
    Minute,
    Second,
}

/// O separador dos padrões de intervalo do CLDR (`h:mm - h:mm a`, `MMM d - d, y`).
const PATTERN_SEPARATOR: &str = " \u{2013} ";

/// O `intervalFormatFallback`: `{0} - {1}` em português, `{0} \u{2013} {1}` em inglês.
fn fallback_separator(language: Language) -> &'static str {
    match language {
        Language::English => PATTERN_SEPARATOR,
        Language::Portuguese => " - ",
    }
}

/// O maior campo da data que difere. Num calendário não gregoriano os campos são os do próprio calendário
/// (`tabela de intl_calendar_range` mede o cenário pelo mesmo critério): em 9/11/2024 o hebraico já está em outro ano.
fn largest_difference(state: &DateTimeFormatState, first: &Moment, second: &Moment) -> Option<Level> {
    let (a, b) = (&first.civil, &second.civil);
    let native = state.native.date(&state.locale, a).zip(state.native.date(&state.locale, b));
    let date_level = match &native {
        Some((x, y)) if x.era_names != y.era_names => Some(Level::Era),
        Some((x, y)) if x.year != y.year || x.related_year != y.related_year => Some(Level::Year),
        Some((x, y)) if x.month_text != y.month_text => Some(Level::Month),
        Some((x, y)) if x.day != y.day => Some(Level::Day),
        Some(_) => None,
        None if (a.year < 1) != (b.year < 1) => Some(Level::Era),
        None if a.year != b.year => Some(Level::Year),
        None if a.mon != b.mon => Some(Level::Month),
        None if a.mday != b.mday => Some(Level::Day),
        None => None,
    };
    if date_level.is_some() {
        date_level
    } else if a.hour / 12 != b.hour / 12 {
        Some(Level::DayPeriod)
    } else if a.hour != b.hour {
        Some(Level::Hour)
    } else if a.min != b.min {
        Some(Level::Minute)
    } else if a.sec != b.sec {
        Some(Level::Second)
    } else {
        None
    }
}

/// O campo mais fino do padrão (`isFieldUnitIgnored`): o que difere abaixo dele não aparece.
fn finest_level(fields: &Fields) -> Level {
    if fields.second.is_some() || fields.fractional_second_digits > 0 {
        Level::Second
    } else if fields.minute.is_some() {
        Level::Minute
    } else if fields.hour.is_some() {
        Level::Hour
    } else if fields.day_period.is_some() {
        Level::DayPeriod
    } else if fields.day.is_some() || fields.weekday.is_some() {
        Level::Day
    } else if fields.month.is_some() {
        Level::Month
    } else if fields.year.is_some() {
        Level::Year
    } else {
        Level::Era
    }
}

fn tag(parts: Vec<Part>, source: RangeSource) -> Vec<RangePart> {
    parts.into_iter().map(|(kind, text)| (kind, text, source)).collect()
}

/// `início`, o separador e `fim`, com as origens `startRange`, `shared` e `endRange`.
fn pair(start: Vec<Part>, separator: &str, end: Vec<Part>) -> Vec<RangePart> {
    let mut parts = tag(start, RangeSource::StartRange);
    parts.push(("literal".to_string(), separator.to_string(), RangeSource::Shared));
    parts.extend(tag(end, RangeSource::EndRange));
    parts
}

/// As partes inteiras (data e hora) de um extremo. Com só hora no padrão o ICU prefixa `yMd`.
fn full_parts(state: &DateTimeFormatState, moment: &Moment) -> Vec<Part> {
    if state.fields.has_date() {
        return parts_with_fields(state, &state.fields, moment);
    }
    // O estado com a data pedida também em `user_fields`: é dele que o padrão medido do locale (`data_key`) tira a chave, e sem
    // isso a data prefixada saía pelo molde de `date_parts` (`05/03/2024, 07:08` em fr, onde o locale diz `05/03/2024 07:08`).
    let mut dated = state.clone();
    for fields in [&mut dated.fields, &mut dated.user_fields] {
        fields.year = Some(Digits2::Numeric);
        fields.month = Some(Month::Numeric);
        fields.day = Some(Digits2::Numeric);
    }
    dated.fields = resolve_fields(state.language, state.hour_cycle.unwrap_or(HourCycle::H23), dated.fields);
    let fields = dated.fields;
    let parts = parts_with_fields(&dated, &fields, moment);
    // A largura da hora vem do padrão de intervalo do idioma (`7:08` em es e ja, `07:08` em pt, medido em
    // `range|days_time|hourpad`), e não do `format`.
    match intl_date_time_data::locale_data_for(&state.locale) {
        Some(data) => apply_hour_width(state, data, if fields.minute.is_some() { "days_time" } else { "days_hour" }, &parts),
        None => parts,
    }
}

/// A data difere (era, ano, mês ou dia).
fn date_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Vec<RangePart> {
    let fields = &state.fields;
    let language = state.language;
    let mut start = date_parts(language, fields, first);
    let mut end = date_parts(language, fields, second);
    let text_month = matches!(fields.month, Some(Month::Narrow | Month::Short | Month::Long));
    if fields.era.is_some() || (text_month && fields.weekday.is_some() && fields.day.is_none()) {
        return pair(start, fallback_separator(language), end);
    }
    if !text_month || level <= Level::Year {
        return pair(start, PATTERN_SEPARATOR, end);
    }

    // Mês por extenso: o ano é o que sobra uma vez (`, y` em inglês, ` de y` em português).
    let tail = if fields.year.is_some() {
        end.truncate(end.len() - 2);
        let at = start.len() - 2;
        start.split_off(at)
    } else {
        Vec::new()
    };
    if level == Level::Day && fields.weekday.is_none() {
        match language {
            // `MMM d - d`: o segundo lado é só o dia.
            Language::English => end = end.split_off(end.len() - 1),
            // `d - d de MMM`: o primeiro lado é só o dia.
            Language::Portuguese => start.truncate(1),
        }
    }
    let mut parts = pair(start, PATTERN_SEPARATOR, end);
    parts.extend(tag(tail, RangeSource::Shared));
    parts
}

/// A hora difere no mesmo dia: a data (se há) sai uma vez e a hora faz o intervalo.
fn time_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Vec<RangePart> {
    let fields = &state.fields;
    let language = state.language;
    let mut parts: Vec<RangePart> = Vec::new();
    if fields.has_date() {
        parts.extend(tag(date_parts(language, fields, first), RangeSource::Shared));
        let joiner = match same_day_joiner(state) {
            Some("") => Vec::new(),
            Some(text) => vec![literal(text)],
            None => vec![date_time_joiner(state, fields)],
        };
        parts.extend(tag(joiner, RangeSource::Shared));
    }
    let mut start = time_parts(state, fields, first);
    let mut end = time_parts(state, fields, second);
    // Os padrões `h`, `hm`, `H` e `Hm` do CLDR; com segundos, fuso ou `dayPeriod` o ICU cai no fallback.
    let has_pattern = fields.hour.is_some()
        && fields.day_period.is_none()
        && fields.second.is_none()
        && fields.fractional_second_digits == 0
        && fields.time_zone_name.is_none();
    if !has_pattern {
        parts.extend(pair(start, fallback_separator(language), end));
        return parts;
    }
    let twelve = matches!(state.hour_cycle, Some(HourCycle::H11 | HourCycle::H12));
    // `h:mm - h:mm a`: com o mesmo AM/PM ele sai uma vez, no fim.
    let tail = if twelve && level != Level::DayPeriod {
        end.truncate(end.len() - 2);
        let at = start.len() - 2;
        start.split_off(at)
    } else {
        Vec::new()
    };
    parts.extend(pair(start, PATTERN_SEPARATOR, end));
    parts.extend(tag(tail, RangeSource::Shared));
    parts
}

/// Separa os literais do fim de `parts` (` г.`, `分`, ` Uhr`) do resto.
fn split_trailing_literals(parts: &[Part]) -> (&[Part], &[Part]) {
    let at = parts.iter().rposition(|(kind, _)| kind != "literal").map_or(0, |index| index + 1);
    parts.split_at(at)
}

/// Os campos de data e hora de `parts` desde a hora (ou o `dayPeriod`, que em coreano a precede).
fn time_start(parts: &[Part]) -> Option<usize> {
    parts.iter().position(|(kind, _)| kind == "hour" || kind == "dayPeriod")
}

/// `cabeça` (`shared`), `início`, `separador` (`shared`), `fim` e `cauda` (`shared`).
fn assemble(head: &[Part], start: &[Part], separator: &str, end: &[Part], tail: &[Part]) -> Vec<RangePart> {
    let mut parts = tag(head.to_vec(), RangeSource::Shared);
    parts.extend(pair(start.to_vec(), separator, end.to_vec()));
    parts.extend(tag(tail.to_vec(), RangeSource::Shared));
    parts
}

/// Como `assemble`, com o AM/PM igual nos dois extremos saindo uma vez: no fim (`10:13 \u{2013} 10:43 PM`) ou,
/// onde o idioma o põe antes da hora, no começo (`오후 10:13 ~ 10:43`). O literal que o separa da hora
/// vai com ele.
fn assemble_shared_day_period(head: &[Part], start: &[Part], separator: &str, end: &[Part], tail: &[Part]) -> Vec<RangePart> {
    let is_period = |part: Option<&Part>| part.is_some_and(|(kind, _)| kind == "dayPeriod");
    let (mut head, mut start, mut end, mut tail) = (head.to_vec(), start, end, tail.to_vec());
    if is_period(start.last()) && start.last() == end.last() {
        let mut cut = 1;
        if start.len() > 1 && start.len() == end.len() && start[start.len() - 2] == end[end.len() - 2] && start[start.len() - 2].0 == "literal" {
            cut = 2;
        }
        tail.splice(0..0, start[start.len() - cut..].iter().cloned());
        (start, end) = (&start[..start.len() - cut], &end[..end.len() - cut]);
    } else if is_period(start.first()) && start.first() == end.first() {
        let cut = if start.get(1).is_some_and(|part| part.0 == "literal") && start.get(1) == end.get(1) { 2 } else { 1 };
        head.extend(start[..cut].iter().cloned());
        (start, end) = (&start[cut..], &end[cut..]);
    }
    assemble(&head, start, separator, end, &tail)
}

/// O intervalo pelos separadores medidos no bun (`range|cenário|sep` e `range|cenário|collapse`, em
/// `intl_date_time_data`), para as línguas que têm a tabela. O separador medido é todo o trecho
/// `shared` entre o último campo do início e o primeiro do fim (`.\u{2013}` em alemão, `分～` em japonês); os
/// literais que fecham os dois lados (` г.`, `分`) saem do início, que o separador já traz, e do fim
/// viram a cauda `shared`. `None` quando a língua não tem tabela ou o caso não foi medido.
fn data_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Option<Vec<RangePart>> {
    let skeleton = short_time_skeleton_state(state, Digits2::Numeric);
    let state = skeleton.as_ref().unwrap_or(state);
    let data = intl_date_time_data::locale_data_for(&state.locale)?;
    let time_level = level >= Level::DayPeriod;
    if !time_level && !state.fields.has_date() {
        return None;
    }
    if time_level && state.date_style.is_some() && state.time_style == Some(StyleWidth::Short) {
        if let Some(parts) = styled_short_time_range(state, level, first, second) {
            return Some(parts);
        }
    }
    // `dayPeriod` tem padrão próprio no ICU: ainda não medido.
    if state.fields.day_period.is_some() {
        return None;
    }
    // Era com o ano que difere (`6–4 av. J.-C.` em fr, `6 a.C. – 4 a.C.` em pt): só ano, ou ano com mês curto e dia, medidos à parte.
    let era_scenario = match (state.fields.era.is_some(), level) {
        (false, _) => None,
        (true, Level::Year) if !state.fields.has_time() && state.fields.weekday.is_none() => {
            let user = &state.user_fields;
            match (user.year, user.month, user.day) {
                (Some(Digits2::Numeric), None, None) => Some("era_year"),
                (Some(Digits2::Numeric), Some(Month::Short), Some(Digits2::Numeric)) => Some("era_date"),
                _ => return None,
            }
        }
        (true, _) => return None,
    };
    // Com segundos ou fração não há padrão de intervalo: o fallback mede outro separador e não junta o AM/PM.
    let seconds = state.fields.second.is_some() || state.fields.fractional_second_digits > 0;
    // Fuso: com a data diferente o fallback repete o fuso nas duas pontas (`zone_date_range`); com segundos também
    // (cenários `zone_*_seconds`); sem segundos o fuso sai uma vez, acrescentado por `special_range`.
    let zone = state.fields.time_zone_name.is_some();
    if zone && !time_level {
        return zone_date_range(state, data, first, second);
    }
    if zone && !seconds {
        return None;
    }
    // O cenário é a maior diferença (`intervalFormats` y, M, d, a, h/m do CLDR): com o mesmo AM/PM no ciclo
    // de 12 horas o padrão é outro, e o AM/PM comum sai uma vez.
    let same_period = time_level
        && matches!(level, Level::Hour | Level::Minute)
        && matches!(state.hour_cycle, Some(HourCycle::H11 | HourCycle::H12))
        && state.fields.day_period.is_none()
        && !seconds;
    let scenario = match (time_level, state.fields.has_date(), level) {
        _ if era_scenario.is_some() => era_scenario.unwrap_or_default(),
        (true, true, _) if seconds => "same_day_seconds",
        (true, false, _) if seconds => "time_only_seconds",
        (true, true, _) if same_period => "same_period",
        (true, false, _) if same_period => "same_period_time",
        (true, true, _) => "same_day",
        (true, false, _) => "time_only",
        (false, _, Level::Day) if !state.fields.has_time() => "same_month",
        (false, _, Level::Month) if !state.fields.has_time() => "same_year",
        _ => "other_years",
    };
    // Com `weekday` o ICU usa outros padrões de intervalo (`yMMMEd`), medidos à parte.
    // Com mês numérico (`yMd`, o `dateStyle: "short"`) o CLDR quase nunca tem `d\u{2013}d`: outros padrões, também medidos.
    let numeric_month = matches!(state.fields.month, Some(Month::Numeric | Month::TwoDigit));
    let scenario = if time_level {
        if zone { format!("zone_{scenario}") } else { scenario.to_string() }
    } else if era_scenario.is_some() {
        scenario.to_string()
    } else if state.fields.weekday.is_some() {
        format!("{scenario}_weekday")
    } else if numeric_month {
        format!("{scenario}_numeric")
    } else {
        scenario.to_string()
    };
    let scenario = scenario.as_str();
    let separator = range_value(state, data, scenario, "sep")?;
    let mut start = endpoint_parts(state, first)?;
    let mut end = endpoint_parts(state, second)?;
    if time_level {
        start = apply_hour_width(state, data, scenario, &start);
        end = apply_hour_width(state, data, scenario, &end);
    }
    if time_level {
        let (head, start_core) = start.split_at(time_start(&start)?);
        let (_, end_core) = end.split_at(time_start(&end)?);
        let (start_core, _) = split_trailing_literals(start_core);
        let (end_core, tail) = split_trailing_literals(end_core);
        // O literal entre a hora e o minuto do padrão de intervalo (`時` em ja) não é o do `format` (`:`): vem medido.
        let cycle_label = if matches!(state.cycle, HourCycle::H11 | HourCycle::H12) { "12" } else { "24" };
        let (start_core, end_core) = match range_value(state, data, scenario, &format!("hmlit|{cycle_label}")) {
            Some(text) => (with_hour_minute_literal(start_core, text), with_hour_minute_literal(end_core, text)),
            None => (start_core.to_vec(), end_core.to_vec()),
        };
        let (start_core, end_core) = (&start_core[..], &end_core[..]);
        // O padrão de intervalo pode fechar com um literal que o `format` do extremo não tem (` Uhr` em de): vem medido.
        let measured_tail = [literal(range_value(state, data, scenario, if matches!(state.cycle, HourCycle::H11 | HourCycle::H12) { "tailtext|12" } else { "tailtext|24" }).unwrap_or(""))];
        let tail = if tail.is_empty() && !measured_tail[0].1.is_empty() { &measured_tail[..] } else { tail };
        let head = &with_same_day_joiner(state, head)[..];
        if same_period && range_value(state, data, scenario, "collapse") == Some("1") {
            return Some(assemble_shared_day_period(head, start_core, separator, end_core, tail));
        }
        return Some(assemble(head, start_core, separator, end_core, tail));
    }
    // Com ano e dia o ICU escolhe o padrão `yMMMd`/`yMd`: quantas partes abrem e fecham o intervalo uma vez só e
    // quantas fazem cada extremo, medidas no bun. O ICU não junta o maior prefixo e sufixo iguais (pt
    // `5 de mar. \u{2013} 9 de mar. de 2024` junta só o ano).
    let measured = |name: &str| range_value(state, data, scenario, name).and_then(|value| value.parse::<usize>().ok());
    if state.fields.year.is_some() && (state.fields.day.is_some() || era_scenario.is_some()) {
        let (head, start_len, end_len, tail) = (measured("head")?, measured("startlen")?, measured("endlen")?, measured("tail")?);
        if head + start_len > start.len() || tail + end_len > end.len() {
            return None;
        }
        let end_at = end.len() - tail;
        return Some(assemble(&start[..head], &start[head..head + start_len], separator, &end[end_at - end_len..end_at], &end[end_at..]));
    }
    // Outras combinações (sem ano ou sem dia): o que os dois extremos têm igual no começo e no fim sai uma vez.
    if matches!(scenario, "same_month" | "same_year" | "same_month_weekday" | "same_year_weekday") && range_value(state, data, scenario, "collapse") == Some("1") {
        let head_len = start.iter().zip(&end).take_while(|(left, right)| left == right).count();
        let tail_len = start[head_len..].iter().rev().zip(end[head_len..].iter().rev()).take_while(|(left, right)| left == right).count();
        let (start_core, end_core) = (&start[head_len..start.len() - tail_len], &end[head_len..end.len() - tail_len]);
        if start_core.is_empty() || end_core.is_empty() {
            return None;
        }
        return Some(assemble(&start[..head_len], start_core, separator, end_core, &end[end.len() - tail_len..]));
    }
    let (start_core, _) = split_trailing_literals(&start);
    let (end_core, tail) = split_trailing_literals(&end);
    Some(assemble(&[], start_core, separator, end_core, tail))
}

/// `parts` com o literal entre a hora e o minuto trocado por `text`.
fn with_hour_minute_literal(parts: &[Part], text: &str) -> Vec<Part> {
    let mut out = parts.to_vec();
    if let Some(hour) = out.iter().position(|(kind, _)| kind == "hour") {
        if out.get(hour + 1).is_some_and(|part| part.0 == "literal") && out.get(hour + 2).is_some_and(|part| part.0 == "minute") {
            out[hour + 1].1 = text.to_string();
        }
    }
    out
}

/// `dateStyle` com `timeStyle: "short"` no mesmo dia: a data (com a cola do dia, `same_day_joiner`) sai uma vez e o resto
/// é o intervalo da hora sozinha, o mesmo de `{ hour: "numeric", minute: "numeric" }` (`04:08–16:08 Uhr` em de,
/// `4時08分～16時08分` em ja), e não o `format` do estilo. Medido no bun em 65 locales, quatro `dateStyle`, três
/// ciclos e quatro pares de instantes: o trecho da hora é idêntico, texto e origens, onde a data do `format` e a do
/// intervalo coincidem. `None` fora desse caso (calendário nativo, locale sem cola).
fn styled_short_time_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Option<Vec<RangePart>> {
    same_day_joiner(state)?;
    if state.native.is_native() {
        return None;
    }
    let mut time_only = state.clone();
    let mut date_only = state.clone();
    time_only.date_style = None;
    date_only.time_style = None;
    for fields in [&mut time_only.fields, &mut time_only.user_fields] {
        (fields.weekday, fields.era, fields.year, fields.month, fields.day) = (None, None, None, None, None);
    }
    for fields in [&mut date_only.fields, &mut date_only.user_fields] {
        (fields.day_period, fields.hour, fields.minute, fields.second) = (None, None, None, None);
        fields.fractional_second_digits = 0;
    }
    // Onde a hora vem antes da data (vi, `full` e `long`: `07:08–19:08 Thứ Ba, 5 tháng 3, 2024`) o intervalo é outro.
    if endpoint_parts(state, first)?.first().is_some_and(|(kind, _)| kind == "hour" || kind == "dayPeriod") {
        return None;
    }
    // Com `dateStyle` o intervalo é o de `yMd` com `Hm`, e o `mm` de dois dígitos escolhe em ja o padrão `7時08分` (com o minuto `numeric` sairia `7:08`).
    let time_state = short_time_skeleton_state(&time_only, Digits2::TwoDigit)?;
    let time = data_range(&time_state, level, first, second)?;
    with_interval_date_fields(&mut date_only);
    let head = with_same_day_joiner(state, &endpoint_parts(&date_only, first)?);
    Some(join_shared(tag(head, RangeSource::Shared), time))
}

/// Fuso com a data diferente: o ICU cai no fallback com as duas pontas inteiras, fuso incluído (`Mar 5, 2024, GMT-3 –
/// Mar 9, 2024, GMT-3`). Por locale e família de campos (`zone_date_*`, medidas no bun) o `sep` é o literal entre as
/// pontas (` a el ` em es-AR, `-` em da) e `ends` diz se as pontas são o `format` (`fmt`) ou o `format` com mês
/// numérico (`num`, em ja, zh, fi, cs); `x`, família não medida ou calendário nativo dá `None`.
fn zone_date_range(state: &DateTimeFormatState, data: &intl_date_time_data::LocaleData, first: &Moment, second: &Moment) -> Option<Vec<RangePart>> {
    let (fields, user) = (&state.fields, &state.user_fields);
    if state.date_style.is_some() || state.time_style.is_some() || fields.era.is_some() || fields.day_period.is_some() {
        return None;
    }
    if fields.second.is_some() || fields.fractional_second_digits > 0 || user.year != Some(Digits2::Numeric) || user.day != Some(Digits2::Numeric) {
        return None;
    }
    let timed = user.hour.is_some();
    if timed && (user.hour != Some(Digits2::Numeric) || user.minute != Some(Digits2::TwoDigit)) || !timed && fields.minute.is_some() {
        return None;
    }
    let family = match (user.month?, user.weekday, timed) {
        (Month::Short, None, false) => "short",
        (Month::Long, None, false) => "long",
        (Month::Short, Some(TextWidth::Short), false) => "weekday",
        (Month::Short, None, true) => match state.cycle {
            HourCycle::H12 => "time",
            HourCycle::H23 => "time24",
            _ => return None,
        },
        _ => return None,
    };
    let scenario = format!("zone_date_{family}");
    let separator = range_value(state, data, &scenario, "sep")?;
    let mut ends = state.clone();
    match range_value(state, data, &scenario, "ends")? {
        "fmt" => {}
        "num" => {
            for fields in [&mut ends.fields, &mut ends.user_fields] {
                fields.month = Some(Month::Numeric);
            }
        }
        _ => return None,
    }
    Some(pair(endpoint_parts(&ends, first)?, separator, endpoint_parts(&ends, second)?))
}

/// `left` seguido de `right`, com o literal `shared` que fecha um e o que abre o outro juntos numa parte só (o ICU
/// emite ` Uhr ` e `分(` inteiros, não ` Uhr` e ` `).
fn join_shared(mut left: Vec<RangePart>, mut right: Vec<RangePart>) -> Vec<RangePart> {
    if let (Some(end), Some(start)) = (left.last(), right.first()) {
        if end.0 == "literal" && start.0 == "literal" && end.2 == RangeSource::Shared && start.2 == RangeSource::Shared {
            let text = format!("{}{}", end.1, start.1);
            let at = left.len() - 1;
            left[at].1 = text;
            right.remove(0);
        }
    }
    left.extend(right);
    left
}

/// O que o fuso acrescenta ao intervalo sem ele, pelo molde medido do locale (`range|zone_*|before` e `after`, com `{z}`
/// no lugar do nome): ` {z}` depois em de, `({z})` em ja, `{z} ` antes em zh. Vale para hora (e minuto) com ou sem a
/// data `yMMMd`; `None` quando o locale não tem molde único nos estilos de fuso (aí vale a diferença do `format`).
fn zone_wrap(state: &DateTimeFormatState, with: &DateTimeFormatState, parts: &[RangePart], first: &Moment) -> Option<Vec<RangePart>> {
    let (fields, user) = (&state.fields, &state.user_fields);
    if fields.era.is_some() || fields.day_period.is_some() || fields.second.is_some() || state.native.is_native() || user.hour != Some(Digits2::Numeric) {
        return None;
    }
    if user.minute.is_some() && user.minute != Some(Digits2::TwoDigit) {
        return None;
    }
    let dated = fields.has_date();
    if dated && !(user.year == Some(Digits2::Numeric) && user.month == Some(Month::Short) && user.day == Some(Digits2::Numeric) && user.weekday.is_none()) {
        return None;
    }
    let data = intl_date_time_data::locale_data_for(&state.locale)?;
    let scenario = format!("zone_{}{}", if dated { "day" } else { "time" }, if user.minute.is_some() { "" } else { "_hour" });
    let (before, after) = (range_value(state, data, &scenario, "before")?, range_value(state, data, &scenario, "after")?);
    let zone = parts_with_fields(with, &with.fields, first).into_iter().find(|(kind, _)| kind == "timeZoneName")?.1;
    let build = |template: &str| -> Vec<RangePart> {
        let (prefix, suffix) = template.split_once("{z}").unwrap_or((template, ""));
        let mut out: Vec<Part> = Vec::new();
        if !prefix.is_empty() {
            out.push(literal(prefix));
        }
        if template.contains("{z}") {
            out.push(("timeZoneName".to_string(), zone.clone()));
        }
        if !suffix.is_empty() {
            out.push(literal(suffix));
        }
        tag(out, RangeSource::Shared)
    };
    Some(join_shared(join_shared(build(before), parts.to_vec()), build(after)))
}

/// O campo `name` do cenário `scenario` de `formatRange`: a tabela do calendário não gregoriano do formatador
/// (`intl_calendar_range`, pela cadeia de locales do ICU) ou, no gregoriano, o `range|cenário|campo` dos `extras`
/// do locale. A data nativa (fa persa, th budista) também tem tabela própria: o padrão e o separador mudam com o calendário.
fn range_value(state: &DateTimeFormatState, data: &intl_date_time_data::LocaleData, scenario: &str, name: &str) -> Option<&'static str> {
    if !state.native.is_native() {
        return data.extra(&format!("range|{scenario}|{name}"));
    }
    icu_locale_chain(state)?.iter().find_map(|locale| intl_calendar_range::value(locale, state.native.name(), scenario, name))
}

/// As partes de um extremo do intervalo: o padrão medido do locale no gregoriano, ou o padrão do calendário nativo
/// (a mesma data que `format` mostra) com a hora do formatador.
fn endpoint_parts(state: &DateTimeFormatState, moment: &Moment) -> Option<Vec<Part>> {
    if state.native.is_native() {
        Some(parts_with_fields(state, &state.fields, moment))
    } else {
        locale_data_parts(state, moment)
    }
}

/// A largura da hora nas pontas do intervalo: o padrão de intervalo do CLDR tem a sua (`HH:mm` em pt, `H:mm` em th)
/// e não a do `format`; medida no bun por cenário e ciclo (`range|cenário|hourpad|12` e `|24`). Só vale com a
/// hora `numeric` pedida: `2-digit` já sai com dois dígitos.
fn apply_hour_width(state: &DateTimeFormatState, data: &intl_date_time_data::LocaleData, scenario: &str, parts: &[Part]) -> Vec<Part> {
    let cycle = if matches!(state.cycle, HourCycle::H11 | HourCycle::H12) { "12" } else { "24" };
    let pad = match range_value(state, data, scenario, &format!("hourpad|{cycle}")) {
        Some(value) if state.user_fields.hour == Some(Digits2::Numeric) => value == "1",
        _ => return parts.to_vec(),
    };
    let zero = icu_number::digits_of(state.locale.split("-u-").next().unwrap_or(""), &state.numbering).map_or('0', |digits| digits[0]);
    parts
        .iter()
        .map(|(kind, text)| {
            let mut text = text.clone();
            if kind == "hour" {
                let width = text.chars().count();
                if pad && width == 1 {
                    text.insert(0, zero);
                } else if !pad && width == 2 && text.starts_with(zero) {
                    text.remove(0);
                }
            }
            (kind.clone(), text)
        })
        .collect()
}

/// `timeStyle: "short"` sozinho: o `DateIntervalFormat` usa o skeleton `Hm`/`hm` e não o padrão do estilo, então
/// os lados saem como `22時13分～23時13分` em ja e ` Uhr` só no fim em de (`22:13\u{2013}23:13 Uhr`), enquanto o `format`
/// diz `22:13`. Medido no bun contra `{ hour: "numeric", minute: "numeric" }`: o resultado é idêntico em ja, de,
/// en, ar, zh e ko. Com segundos (`medium` em diante) não há padrão de intervalo e o fallback usa o estilo.
/// `None` fora desse caso.
fn short_time_skeleton_state(state: &DateTimeFormatState, minute: Digits2) -> Option<DateTimeFormatState> {
    if state.date_style.is_some() || state.time_style != Some(StyleWidth::Short) {
        return None;
    }
    let mut skeleton = state.clone();
    skeleton.time_style = None;
    skeleton.user_fields.hour = Some(Digits2::Numeric);
    skeleton.user_fields.minute = Some(minute);
    Some(skeleton)
}

/// A cola do intervalo do mesmo dia para o `dateStyle` do estado: a tabela da `língua-REGIÃO`, depois da língua
/// (`en-GB` e as variantes de `en` têm a mesma cola). `None` sem `dateStyle` ou sem a língua na tabela.
fn same_day_joiner(state: &DateTimeFormatState) -> Option<&'static str> {
    same_day_joiner_at(state, date_style_index(state)?)
}

/// 0 a 3 para `full`, `long`, `medium` e `short`: o índice do `sdj` e do `sdd` nos `extras` do locale.
fn date_style_index(state: &DateTimeFormatState) -> Option<usize> {
    Some(match state.date_style? {
        StyleWidth::Full => 0,
        StyleWidth::Long => 1,
        StyleWidth::Medium => 2,
        StyleWidth::Short => 3,
    })
}

/// Troca o `dateStyle` de `state` pelos campos do skeleton que o `DateIntervalFormat` usa para a data do mesmo dia (`sdd`), quando
/// não é o do próprio estilo (ja e zh: `2024/3/5` para `long`, `2024/3/5火曜日` para `full`).
fn with_interval_date_fields(state: &mut DateTimeFormatState) {
    let Some(spec) = date_style_index(state).and_then(|index| intl_date_time_data::locale_data_for(&state.locale)?.extra(&format!("sdd|{index}"))) else {
        return;
    };
    state.date_style = None;
    for fields in [&mut state.fields, &mut state.user_fields] {
        (fields.weekday, fields.era, fields.year, fields.month, fields.day) = (None, None, None, None, None);
        for pair in spec.split(';') {
            let Some((field, value)) = pair.split_once('=') else { continue };
            match field {
                "weekday" => fields.weekday = TextWidth::parse(value),
                "year" => fields.year = Digits2::parse(value),
                "month" => fields.month = Month::parse(value),
                "day" => fields.day = Digits2::parse(value),
                _ => {}
            }
        }
    }
}

/// A cola `standard` do intervalo do mesmo dia no calendário `iso8601`: com `dateStyle` a do estilo; sem ele, a que o
/// mês pedido escolhe (por extenso como `long`, abreviado como `medium`, numérico ou ausente como `short`), medida
/// no bun nos campos `year`/`month`/`day` com `hour`/`minute`.
fn iso_same_day_joiner(state: &DateTimeFormatState, fields: &Fields) -> Option<&'static str> {
    if state.date_style.is_some() {
        return same_day_joiner(state);
    }
    let index = match fields.month {
        Some(Month::Long) => 1,
        Some(Month::Short | Month::Narrow) => 2,
        _ => 3,
    };
    same_day_joiner_at(state, index)
}

/// O `sdj|index` (0 a 3 = full, long, medium, short) dos `extras` do locale: a cola `standard` do intervalo do mesmo
/// dia com `dateStyle`, medida no bun por `gen-datetime-data.js` (o `DateIntervalFormat` nunca usa a `atTime` do
/// `format`: `Tuesday, November 14, 2023, 10:13 \u{2013} 11:13 PM`).
fn same_day_joiner_at(state: &DateTimeFormatState, index: usize) -> Option<&'static str> {
    intl_date_time_data::locale_data_for(&state.locale)?.extra(&format!("sdj|{index}"))
}

/// `head` com o literal que fecha a data trocado pela cola do intervalo do mesmo dia (`dateStyle` com hora).
fn with_same_day_joiner(state: &DateTimeFormatState, head: &[Part]) -> Vec<Part> {
    // Sem `dateStyle` a cola `standard` é a do estilo que o mês pedido escolhe (pt `5 de mar. de 2024 07:08`, sem vírgula).
    let Some(joiner) = iso_same_day_joiner(state, &state.fields) else { return head.to_vec() };
    if head.is_empty() {
        return Vec::new();
    }
    let keep = head.iter().rposition(|(kind, _)| kind != "literal").map_or(0, |index| index + 1);
    let mut parts = head[..keep].to_vec();
    if !joiner.is_empty() {
        parts.push(literal(joiner));
    }
    parts
}

/// A língua primária da tag (`pt` de `pt-BR`).
fn primary_language(state: &DateTimeFormatState) -> &str {
    state.locale.split(['-', '_']).next().unwrap_or("")
}

/// O `intervalFormatFallback` medido no bun com as duas pontas inteiras: a data de cada ponta quando a
/// diferença é maior que a hora (`time_fallback`), ou a era (`era_fallback`). Sem tag a língua decide.
fn time_fallback(state: &DateTimeFormatState) -> &'static str {
    match primary_language(state) {
        "ja" => "\u{ff5e}",
        "ko" => " ~ ",
        "sv" => "\u{2013}",
        "el" => " - ",
        "" => fallback_separator(state.language),
        _ => PATTERN_SEPARATOR,
    }
}

/// Só hora (e minuto) em dias diferentes: o ICU prefixa `yMd` e o intervalo é `format` + separador + `format`. O
/// separador entre os dois `format` é medido por locale (`range|days_hour|pairsep` e `range|days_time|pairsep`:
/// ` \u{2013} ` na maioria, `\u{ff5e}` em ja, ` ~ ` em ko, ` a el ` em es-AR, ` - ` em th). Os literais que fecham o
/// início (` h` em fr, ` Uhr` em de, `時` em ja) entram no separador `shared` e os que fecham o fim viram a cauda `shared`
/// (`07` `startRange`, ` h \u{2013} ` `shared`, `19` `endRange`, ` h` `shared`). Sem o dado (segundos, fuso, calendário
/// nativo, locale sem tabela) vale o fallback com as pontas inteiras.
fn days_time_range(state: &DateTimeFormatState, start: Vec<Part>, end: Vec<Part>) -> Vec<RangePart> {
    let fields = &state.fields;
    let plain = !(fields.has_date() || fields.second.is_some() || fields.fractional_second_digits > 0 || fields.time_zone_name.is_some());
    let measured = intl_date_time_data::locale_data_for(&state.locale)
        .filter(|_| plain)
        .and_then(|data| range_value(state, data, if fields.minute.is_some() { "days_time" } else { "days_hour" }, "pairsep"));
    let Some(pair_separator) = measured else {
        return pair(start, time_fallback(state), end);
    };
    let (start_core, start_trailing) = split_trailing_literals(&start);
    let (end_core, tail) = split_trailing_literals(&end);
    let separator: String = start_trailing.iter().map(|(_, text)| text.as_str()).chain([pair_separator]).collect();
    assemble(&[], start_core, &separator, end_core, tail)
}

/// O fallback quando a era difere (`6 a.C. \u{2013} 5 d.C.`): em francês `à`, em japonês `\u{ff5e}`, em coreano `~`.
fn era_fallback(state: &DateTimeFormatState) -> &'static str {
    match primary_language(state) {
        "fr" => " \u{00e0} ",
        "ja" => "\u{ff5e}",
        "ko" => " ~ ",
        _ => PATTERN_SEPARATOR,
    }
}

/// Uma cópia de `state` sem a era, o fuso ou o `dayPeriod` pedidos: o intervalo sem eles tem padrão no ICU.
fn plain_state(state: &DateTimeFormatState, era: bool, zone: bool, period: bool) -> DateTimeFormatState {
    let mut plain = state.clone();
    for fields in [&mut plain.fields, &mut plain.user_fields] {
        if era {
            fields.era = None;
        }
        if zone {
            fields.time_zone_name = None;
        }
        if period {
            fields.day_period = None;
        }
    }
    plain
}

/// O ICU usa o nome curto do fuso no intervalo (`zzzz` vira `z`, `vvvv` vira `v`).
fn short_zone_state(mut state: DateTimeFormatState) -> DateTimeFormatState {
    for fields in [&mut state.fields, &mut state.user_fields] {
        fields.time_zone_name = fields.time_zone_name.map(|kind| match kind {
            TimeZoneName::Long => TimeZoneName::Short,
            TimeZoneName::LongGeneric => TimeZoneName::ShortGeneric,
            other => other,
        });
    }
    state
}

/// `texto` com o `valor` da parte `kind` destacado, os literais em volta preservados.
fn split_around(text: &str, kind: &str, value: &str) -> Vec<Part> {
    if text.is_empty() {
        return Vec::new();
    }
    let Some(at) = text.find(value) else {
        return vec![("literal".to_string(), text.to_string())];
    };
    let (before, after) = (&text[..at], &text[at + value.len()..]);
    let mut parts = Vec::new();
    for (part_kind, part_text) in [("literal", before), (kind, value), ("literal", after)] {
        if !part_text.is_empty() {
            parts.push((part_kind.to_string(), part_text.to_string()));
        }
    }
    parts
}

/// O que o campo `kind` acrescenta ao texto de `with` em relação a `without`: o que vem antes e o que vem
/// depois (`UTC ` em chinês, ` UTC` em inglês, `西暦` em japonês, ` AD` em inglês).
fn decoration(with: &[Part], without: &[Part], kind: &str) -> Option<(Vec<Part>, Vec<Part>)> {
    let text = |parts: &[Part]| parts.iter().map(|(_, value)| value.as_str()).collect::<String>();
    let (full, base) = (text(with), text(without));
    let (before, after) = if let Some(rest) = full.strip_suffix(&base) {
        (rest.to_string(), String::new())
    } else {
        (String::new(), full.strip_prefix(&base)?.to_string())
    };
    let value = &with.iter().find(|(part_kind, _)| part_kind == kind)?.1;
    Some((split_around(&before, kind, value), split_around(&after, kind, value)))
}

/// Era, fuso e `dayPeriod` no intervalo (medido no bun, 12 locales): o ICU escolhe o padrão do intervalo
/// sem esses campos e os acrescenta onde o padrão completo os põe, uma vez (`shared`): `4:08 AM \u{2013} 4:08 PM
/// GMT-3`, `UTC 07:08\u{2013}19:08`, `2020 \u{2013} 2024 AD`, `\u{7d00}\u{5143}\u{524d}6\u{5e74}\u{ff5e}4\u{5e74}`. `timeZoneName` `shortOffset` e `longOffset`
/// somem do intervalo; o `dayPeriod` (`B`) só troca o texto do AM/PM. Com a era diferente, ou a data
/// diferente (fuso e `dayPeriod`), vale o fallback com as duas pontas inteiras (`pair` no chamador).
fn special_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Option<Vec<RangePart>> {
    if state.date_style.is_some() || state.time_style.is_some() || state.native.date(&state.locale, &first.civil).is_some() {
        return None;
    }
    // O molde de intervalo da raiz já traz a era (`G`) no próprio texto: não há decoração a acrescentar.
    if iso_interval_template(state, level).is_some() {
        return None;
    }
    // A era com o ano que difere tem padrão de intervalo medido por locale (`era_year`, `era_date`): vale antes da decoração.
    if state.fields.era.is_some() {
        if let Some(parts) = data_range(state, level, first, second) {
            return Some(parts);
        }
    }
    let fields = &state.fields;
    // Com segundos (sem padrão de intervalo) ou com a data diferente e hora, o ICU cai no fallback com as duas pontas
    // inteiras e o fuso, de qualquer estilo, fica nas duas (`4:08:09 AM GMT-3 – 4:08:09 PM GMT-3`): não há o que acrescentar.
    let seconds = fields.second.is_some() || fields.fractional_second_digits > 0;
    let keeps_zone = seconds || (level < Level::DayPeriod && fields.has_time());
    let offset_zone = matches!(fields.time_zone_name, Some(TimeZoneName::ShortOffset | TimeZoneName::LongOffset)) && !keeps_zone;
    let era = fields.era.is_some() && level > Level::Era;
    let zone = fields.time_zone_name.is_some() && !offset_zone && !keeps_zone && level >= Level::DayPeriod;
    let period = fields.day_period.is_some() && level >= Level::DayPeriod;
    if !(offset_zone || era || zone || period) {
        return None;
    }
    let drop_zone = offset_zone || zone;
    let plain = plain_state(state, era, drop_zone, period);
    let mut parts = plain_range(&plain, level, first, second);
    let decorate = |parts: Vec<RangePart>, with: &DateTimeFormatState, without: &DateTimeFormatState, kind: &str| {
        let with_parts = parts_with_fields(with, &with.fields, first);
        let without_parts = parts_with_fields(without, &without.fields, first);
        let (before, after) = decoration(&with_parts, &without_parts, kind)?;
        let mut out = tag(before, RangeSource::Shared);
        out.extend(parts);
        out.extend(tag(after, RangeSource::Shared));
        Some(out)
    };
    if zone {
        let with = short_zone_state(plain_state(state, era, false, period));
        // O molde medido do locale (`zone_wrap`) vence a diferença entre `format` com e sem fuso, que erra onde o
        // `format` muda a largura da hora (`04:08 GMT-3` contra `4:08` em de) ou o ICU embrulha o fuso (`(GMT-3)` em ja).
        let wrapped = if era || period { None } else { zone_wrap(state, &with, &parts, first) };
        parts = match wrapped {
            Some(wrapped) => wrapped,
            None => decorate(parts, &with, &plain_state(state, era, true, period), "timeZoneName")?,
        };
    }
    if era {
        let with = plain_state(state, false, drop_zone, period);
        parts = decorate(parts, &with, &plain_state(state, true, drop_zone, period), "era")?;
    }
    if period {
        let named = plain_state(state, era, drop_zone, false);
        let period_text = |moment: &Moment| parts_with_fields(&named, &named.fields, moment).into_iter().find(|(kind, _)| kind == "dayPeriod");
        let (start_period, end_period) = (period_text(first)?, period_text(second)?);
        for (kind, text, source) in parts.iter_mut().filter(|(kind, _, _)| kind == "dayPeriod") {
            let replacement = if *source == RangeSource::EndRange { &end_period } else { &start_period };
            *kind = replacement.0.clone();
            *text = replacement.1.clone();
        }
    }
    Some(parts)
}

/// As partes do intervalo de `start_ms` a `end_ms`, com a origem de cada uma.
pub(super) fn format_range_parts_at(state: &DateTimeFormatState, start_ms: i64, end_ms: i64) -> Vec<RangePart> {
    let first = moment_at(&state.zone, start_ms);
    let second = moment_at(&state.zone, end_ms);
    let Some(level) = largest_difference(state, &first, &second) else {
        return tag(format_to_parts_at(state, start_ms), RangeSource::Shared);
    };
    if level > finest_level(&state.fields) {
        return tag(format_to_parts_at(state, start_ms), RangeSource::Shared);
    }
    if let Some(parts) = iso_era_range(state, level, &first, &second) {
        return parts;
    }
    special_range(state, level, &first, &second).unwrap_or_else(|| plain_range(state, level, &first, &second))
}

/// Os dois extremos de um intervalo sem padrão no ICU: o literal antes do primeiro campo e depois do último de cada ponta
/// (o espaço vazio da era, o `)` do rótulo) é `shared`, o literal entre campos é da ponta, e o `shared` que se
/// encontra com o separador vira um literal só (medido no bun: `" (day: "`, `5`, `") \u{2013}  (day: "`, `7`, `")"`).
fn bounded_pair(start: Vec<Part>, separator: &str, end: Vec<Part>) -> Vec<RangePart> {
    let mut out: Vec<RangePart> = Vec::new();
    let mut push = |kind: String, text: String, source: RangeSource| match out.last_mut() {
        Some(last) if last.0 == "literal" && kind == "literal" && last.2 == RangeSource::Shared && source == RangeSource::Shared => last.1.push_str(&text),
        _ => out.push((kind, text, source)),
    };
    for (side, joiner, parts) in [(RangeSource::StartRange, None, start), (RangeSource::EndRange, Some(separator), end)] {
        if let Some(joiner) = joiner {
            push("literal".to_string(), joiner.to_string(), RangeSource::Shared);
        }
        let first = parts.iter().position(|(kind, _)| kind != "literal");
        let last = parts.iter().rposition(|(kind, _)| kind != "literal");
        for (index, (kind, text)) in parts.into_iter().enumerate() {
            let edge = kind == "literal" && first.zip(last).map_or(true, |(first, last)| index < first || index > last);
            push(kind, text, if edge { RangeSource::Shared } else { side });
        }
    }
    out
}

/// A data de uma ponta quando só a era é pedida junto da hora no `iso8601`: o ICU repete só o campo que difere e os
/// maiores (`5`, `01-05`, `2024-01-05`), com a era vazia na frente.
fn iso_era_level_parts(state: &DateTimeFormatState, level: Level, moment: &Moment) -> Vec<Part> {
    let mut fields = state.fields;
    fields.day = Some(Digits2::Numeric);
    if level <= Level::Month {
        fields.month = Some(Month::Numeric);
    }
    if level <= Level::Year {
        fields.year = Some(Digits2::Numeric);
    }
    parts_with_fields(state, &fields, moment)
}

/// Calendário `iso8601` com era e um único campo de data, ou com era e hora (medido no bun em 6 locales e 12 widths): o
/// intervalo é o fallback da raiz com as duas pontas inteiras, `bounded_pair`. Com `month` as pontas levam o rótulo mesmo onde
/// o `format` não leva (`iso_era_field_parts`), e o mês `long`, que é vazio, vira um valor só. Com hora e a data diferente
/// (`level` até o dia), cada ponta traz a data do nível (`iso_era_level_parts`); no mesmo dia vale `iso_time_range`.
fn iso_era_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Option<Vec<RangePart>> {
    let fields = &state.fields;
    if !state.iso_calendar || fields.era.is_none() || state.date_style.is_some() || state.time_style.is_some() {
        return None;
    }
    if !fields.has_time() {
        let month = fields.month.is_some();
        let start = iso_era_field_parts(state, fields, first, false)?;
        if fields.month == Some(Month::Long) {
            return Some(tag(start, RangeSource::Shared));
        }
        let start = if month { iso_era_field_parts(state, fields, first, true)? } else { start };
        let end = iso_era_field_parts(state, fields, second, month)?;
        return Some(bounded_pair(start, PATTERN_SEPARATOR, end));
    }
    let era_time = !fields.has_date_without_era() && fields.has_time() && fields.time_zone_name.is_none() && fields.day_period.is_none();
    if !era_time {
        return None;
    }
    if level >= Level::DayPeriod {
        return Some(iso_time_range(state, level, first, second));
    }
    Some(bounded_pair(iso_era_level_parts(state, level, first), PATTERN_SEPARATOR, iso_era_level_parts(state, level, second)))
}

/// Data no calendário `iso8601` (`PlainYearMonth`, `PlainMonthDay`): o CLDR não tem padrão de intervalo para ele, e o
/// `DateIntervalFormat` usa o `intervalFormatFallback` da raiz, `{0} \u{2013} {1}`, o mesmo em todo locale (medido no bun
/// em en, fr, es, it, ko, zh e ar: `2024-01 \u{2013} 2024-03`, inclusive em ko, cujo gregoriano usa `~`). As duas pontas são o
/// `format` inteiro de cada instante (`startRange` e `endRange`), o separador `shared`. Quando as duas pontas saem
/// iguais (o mês por extenso ou abreviado some no `iso8601`: `year: "numeric", month: "long"` dá `2024 ` nas duas) o
/// resultado é um valor só, todo `shared`.
fn iso_date_range(state: &DateTimeFormatState, first: &Moment, second: &Moment) -> Vec<RangePart> {
    let start = parts_with_fields(state, &state.fields, first);
    let end = parts_with_fields(state, &state.fields, second);
    if start == end {
        return tag(start, RangeSource::Shared);
    }
    pair(start, PATTERN_SEPARATOR, end)
}

/// Os campos de um molde de intervalo da raiz do CLDR e o `type` da parte que cada um vira: `G` era (sempre vazia no
/// `iso8601`), `y` ano, `M` mês (vazio no longo e no abreviado, `J` no estreito), `d` dia, `E` dia da semana.
const INTERVAL_FIELDS: [(char, &str); 5] = [('G', "era"), ('y', "year"), ('M', "month"), ('d', "day"), ('E', "weekday")];

/// O molde de intervalo da raiz (início, separador, fim) para o `iso8601` com mês por extenso e sem hora, medido no bun
/// (`dateStyle` medium, long e full, `month` com `day`, `weekday`, `era`), igual em todo locale. O ICU escolhe pelo campo
/// que difere: dia (`y M d` + `\u{2013}` + `d`), mês (`y M d` + ` \u{2013} ` + `M d`) ou ano (`y M d` + ` \u{2013} ` + `y M d`, que
/// acrescenta o ano mesmo quando o padrão não tem). Com `weekday` o dia e o mês usam o mesmo molde (`M d, E`). Com era
/// e sem ano o molde repete a era (`G M d \u{2013} G M d`) em todos os níveis. Sem dia, o ano sozinho (com ou sem era, que
/// some) é `y\u{2013}y` no nível do ano, e o dia da semana sem dia usa `[G] [y] [M] E` dos dois lados em todo nível.
/// `None` fora do medido (mês numérico, hora, era com mês e sem dia da semana nem ano): quem
/// chama cai no fallback de `iso_date_range`. O dia da semana com dia e era usa a era no início (`G y M d, E`), e nos dois
/// lados quando não há ano.
fn iso_interval_template(state: &DateTimeFormatState, level: Level) -> Option<(&'static str, &'static str, &'static str)> {
    const DASH: &str = " \u{2013} ";
    let fields = &state.fields;
    let text_month = matches!(fields.month, Some(Month::Long | Month::Short | Month::Narrow));
    let (era, year, weekday) = (fields.era.is_some(), fields.year.is_some(), fields.weekday.is_some());
    // Sem mês, só o ano (com ou sem era) e o ano com dia da semana têm molde medido.
    let year_only = fields.month.is_none() && fields.day.is_none() && year;
    if !state.iso_calendar || !(text_month || year_only) || fields.has_time() || fields.time_zone_name.is_some() || fields.day_period.is_some() {
        return None;
    }
    if fields.day.is_none() {
        if weekday {
            // O dia da semana sem dia: o mesmo molde nos três níveis. A era sem ano esconde o mês estreito (`G E`), com ano
            // o mês fica (`G y M E`), e o mês por extenso é vazio em todos.
            let head = match (era, year, fields.month.is_some()) {
                (true, true, true) => "G y M E",
                (true, true, false) => "G y E",
                (true, false, _) => "G E",
                (false, true, true) => "y M E",
                (false, true, false) => "y E",
                (false, false, _) => "M E",
            };
            return matches!(level, Level::Year | Level::Month | Level::Day).then_some((head, DASH, head));
        }
        if year_only {
            return (level == Level::Year).then_some(("y", "\u{2013}", "y"));
        }
        let both = if era { "G y M" } else { "y M" };
        return (year && level == Level::Year).then_some((both, DASH, "y M"));
    }
    if !text_month {
        return None;
    }
    if weekday {
        // Com era e sem ano a era repete nos dois lados (`G M d, E`); com ano ela só entra no início.
        let head = match (era, year) {
            (true, true) => "G y M d, E",
            (true, false) => "G M d, E",
            (false, true) => "y M d, E",
            (false, false) => "M d, E",
        };
        let tail = if era && !year { "G M d, E" } else { "M d, E" };
        return match level {
            Level::Year if era && !year => Some((head, DASH, head)),
            Level::Year => Some((if era { "G y M d, E" } else { "y M d, E" }, DASH, "y M d, E")),
            Level::Month | Level::Day => Some((head, DASH, tail)),
            _ => None,
        };
    }
    if era && !year {
        return matches!(level, Level::Year | Level::Month | Level::Day).then_some(("G M d", DASH, "G M d"));
    }
    let head = match (era, year) {
        (true, _) => "G y M d",
        (false, true) => "y M d",
        (false, false) => "M d",
    };
    match level {
        Level::Day => Some((head, "\u{2013}", "d")),
        Level::Month => Some((head, DASH, "M d")),
        // Sem o campo do ano (e sem era, tratada acima) os anos diferentes não entram: `M d \u{2013} M d`.
        Level::Year if !year => Some(("M d", DASH, "M d")),
        Level::Year => Some((if era { "G y M d" } else { "y M d" }, DASH, "y M d")),
        _ => None,
    }
}

/// O intervalo pelo molde da raiz de `iso_interval_template`. Um campo que está nas duas metades do molde sai
/// `startRange` na primeira e `endRange` na segunda; o que está numa só sai `shared`. Campo de texto vazio (mês, era) some,
/// e o texto entre dois campos que restam é `startRange` ou `endRange` quando os dois vizinhos são da mesma ponta, e
/// `shared` no resto (o separador, o que vem antes do primeiro campo ou depois do último).
fn iso_interval_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Option<Vec<RangePart>> {
    enum Piece {
        Text(String),
        Field(&'static str, String, RangeSource),
    }
    let (head, separator, tail) = iso_interval_template(state, level)?;
    let mut fields = state.fields;
    if fields.year.is_none() {
        fields.year = Some(Digits2::Numeric);
    }
    let (first_parts, second_parts) = (parts_with_fields(state, &fields, first), parts_with_fields(state, &fields, second));
    let letters = |text: &str| -> Vec<char> { text.chars().filter(|letter| INTERVAL_FIELDS.iter().any(|(known, _)| known == letter)).collect() };
    let (head_letters, tail_letters) = (letters(head), letters(tail));
    let mut pieces: Vec<Piece> = Vec::new();
    let push_text = |pieces: &mut Vec<Piece>, text: &str| match pieces.last_mut() {
        Some(Piece::Text(last)) => last.push_str(text),
        _ => pieces.push(Piece::Text(text.to_string())),
    };
    let halves = [(head, &tail_letters, RangeSource::StartRange, &first_parts), (tail, &head_letters, RangeSource::EndRange, &second_parts)];
    for (index, (template, other, side, parts)) in halves.into_iter().enumerate() {
        if index == 1 {
            push_text(&mut pieces, separator);
        }
        for letter in template.chars() {
            let Some(&(_, kind)) = INTERVAL_FIELDS.iter().find(|(known, _)| *known == letter) else {
                push_text(&mut pieces, &letter.to_string());
                continue;
            };
            let value = parts.iter().find(|(part_kind, _)| part_kind.as_str() == kind && kind != "era").map(|(_, text)| text.clone()).unwrap_or_default();
            if !value.is_empty() {
                let source = if other.contains(&letter) { side } else { RangeSource::Shared };
                pieces.push(Piece::Field(kind, value, source));
            }
        }
    }
    let field_source = |piece: Option<&Piece>| match piece {
        Some(Piece::Field(_, _, source)) => Some(*source),
        _ => None,
    };
    let mut out: Vec<RangePart> = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Field(kind, text, source) => out.push((kind.to_string(), text.clone(), *source)),
            Piece::Text(text) => {
                let before = field_source(index.checked_sub(1).and_then(|previous| pieces.get(previous)));
                let source = match (before, field_source(pieces.get(index + 1))) {
                    (Some(a), Some(b)) if a == b && a != RangeSource::Shared => a,
                    _ => RangeSource::Shared,
                };
                out.push(("literal".to_string(), text.clone(), source));
            }
        }
    }
    Some(out)
}

/// As partes inteiras de um extremo no `iso8601`: só hora no padrão ganha a data numérica na frente (`2024-01-05, 3 PM`).
fn iso_full_parts(state: &DateTimeFormatState, moment: &Moment) -> Vec<Part> {
    let mut fields = state.fields;
    if !fields.has_date() {
        fields.year = Some(Digits2::Numeric);
        fields.month = Some(Month::Numeric);
        fields.day = Some(Digits2::Numeric);
    }
    parts_with_fields(state, &fields, moment)
}

/// Calendário `iso8601` com hora (medido no bun em en, pt, de, fr, ja e ko): dias diferentes dão o fallback da raiz
/// com os dois `format` inteiros (`iso_full_parts`), ` \u{2013} ` `shared`. No mesmo dia a data sai uma vez, com a cola
/// `standard` do intervalo (`iso_same_day_joiner`, não a `at` do `format`), e a hora faz o intervalo: `h`, `hm`, `H`
/// e `Hm` unem os lados com o traço de faixa colado (`15:04\u{2013}17:30`) e, no ciclo de 12 horas com o mesmo AM/PM, o
/// AM/PM sai uma vez no fim (`3:04\u{2013}5:30 PM`, `3:04\u{2013}5:30 \u{c624}\u{d6c4}`). Com segundos, fração ou `dayPeriod` não há
/// padrão e as duas horas saem inteiras com ` \u{2013} `. O AM/PM diferente no ciclo de 12 horas (`9:04 AM \u{2013} 3:30 PM`,
/// `11:04 AM \u{2013} 12:30 PM`, `9:04 \u{c624}\u{c804} \u{2013} 3:30 \u{c624}\u{d6c4}`, medido em en, ko, ja e de) sai com ` \u{2013} ` entre as
/// horas inteiras; o ciclo de 24 horas atravessando o meio-dia é o `Hm` comum (`09:04\u{2013}15:30`, `09\u{2013}15`).
fn iso_time_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Vec<RangePart> {
    if level < Level::DayPeriod {
        return pair(iso_full_parts(state, first), PATTERN_SEPARATOR, iso_full_parts(state, second));
    }
    let fields = &state.fields;
    let mut parts: Vec<RangePart> = Vec::new();
    if fields.has_date() {
        parts.extend(tag(iso_date_parts(state, fields, first), RangeSource::Shared));
        let joiner = match iso_same_day_joiner(state, fields) {
            Some("") => Vec::new(),
            Some(text) => vec![literal(text)],
            None => vec![iso_date_time_joiner(state, fields)],
        };
        parts.extend(tag(joiner, RangeSource::Shared));
    }
    let (mut start, mut end) = (time_parts(state, fields, first), time_parts(state, fields, second));
    let has_pattern = fields.hour.is_some()
        && fields.day_period.is_none()
        && fields.second.is_none()
        && fields.fractional_second_digits == 0
        && fields.time_zone_name.is_none();
    if !has_pattern {
        parts.extend(pair(start, PATTERN_SEPARATOR, end));
        return parts;
    }
    let twelve = matches!(state.hour_cycle, Some(HourCycle::H11 | HourCycle::H12));
    if twelve && level == Level::DayPeriod {
        parts.extend(pair(start, PATTERN_SEPARATOR, end));
        return parts;
    }
    let tail = if twelve {
        end.truncate(end.len() - 2);
        let at = start.len() - 2;
        start.split_off(at)
    } else {
        Vec::new()
    };
    parts.extend(pair(start, "\u{2013}", end));
    parts.extend(tag(tail, RangeSource::Shared));
    parts
}

/// O intervalo escolhido pelo padrão: a tabela medida, o fallback com as duas pontas, ou o de data.
fn plain_range(state: &DateTimeFormatState, level: Level, first: &Moment, second: &Moment) -> Vec<RangePart> {
    if state.fields.era.is_some() && level == Level::Era {
        return pair(full_parts(state, first), era_fallback(state), full_parts(state, second));
    }
    if state.iso_calendar && state.fields.era.is_none() && state.fields.has_time() {
        return iso_time_range(state, level, first, second);
    }
    if let Some(parts) = iso_interval_range(state, level, first, second) {
        return parts;
    }
    // Era com o mês numérico ou ausente (` 2024-01-05 \u{2013} 2024-01-07`): o mesmo fallback. Era com mês por extenso
    // fora do molde medido (`iso_interval_template`) e com outros campos juntos ainda não tem tratamento; era com um só
    // campo (`month` ou `day`) e era com hora saem em `iso_era_range`, antes de chegar aqui.
    let era_text_month = state.fields.era.is_some() && matches!(state.fields.month, Some(Month::Narrow | Month::Short | Month::Long));
    if state.iso_calendar && !era_text_month && !state.fields.has_time() {
        return iso_date_range(state, first, second);
    }
    // Medido no bun 1.4.2 (en e pt-BR): `dateStyle` com `timeStyle` e a data diferente é o fallback com as duas pontas
    // inteiras (`1/5/24, 3:04 PM \u{2013} 1/9/24, 3:30 AM`), em todo `timeStyle`.
    if state.date_style.is_some() && state.time_style.is_some() && level < Level::DayPeriod && !state.native.is_native() {
        return pair(full_parts(state, first), PATTERN_SEPARATOR, full_parts(state, second));
    }
    // O ano que difere sem estar no padrão (`month` com `day`): o ICU acrescenta o ano às duas pontas
    // (`January 5, 2024 \u{2013} December 31, 2025`, `1/5/2024 \u{2013} 12/31/2025`).
    let fields = &state.fields;
    if level == Level::Year
        && fields.year.is_none()
        && (fields.month.is_some() || fields.day.is_some())
        && !fields.has_time()
        && state.date_style.is_none()
        && !state.native.is_native()
    {
        let mut with_year = state.clone();
        for fields in [&mut with_year.fields, &mut with_year.user_fields] {
            fields.year = Some(Digits2::Numeric);
        }
        return pair(full_parts(&with_year, first), PATTERN_SEPARATOR, full_parts(&with_year, second));
    }
    if let Some(parts) = data_range(state, level, first, second) {
        return parts;
    }
    if level >= Level::DayPeriod {
        return time_range(state, level, first, second);
    }
    if state.fields.has_time() {
        return days_time_range(state, full_parts(state, first), full_parts(state, second));
    }
    date_range(state, level, first, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 14/11/2023 22:13:20 UTC e os instantes que o seguem.
    const BASE: i64 = 1_700_000_000_000;
    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;

    fn state(language: Language, fields: Fields, cycle: HourCycle) -> DateTimeFormatState {
        let fields = resolve_fields(language, cycle, fields);
        DateTimeFormatState {
            locale: String::new(),
            language,
            zone: TimeZone::UTC,
            zone_name: "UTC".to_string(),
            hour_cycle: fields.hour.map(|_| cycle),
            fields,
            date_style: None,
            time_style: None,
            user_fields: Fields::default(),
            any_present: false,
            cycle,
            iso_calendar: false,
            native: crate::runtime::intl_calendar::NativeCalendar::default(),
            numbering: String::new(),
        }
    }

    fn text(state: &DateTimeFormatState, start: i64, end: i64) -> String {
        format_range_parts_at(state, start, end).into_iter().map(|(_, text, _)| text).collect()
    }

    fn numeric_date() -> Fields {
        Fields {
            year: Some(Digits2::Numeric),
            month: Some(Month::Numeric),
            day: Some(Digits2::Numeric),
            ..Fields::default()
        }
    }

    fn long_date() -> Fields {
        Fields { month: Some(Month::Long), ..numeric_date() }
    }

    fn hour_minute() -> Fields {
        Fields { hour: Some(Digits2::Numeric), minute: Some(Digits2::Numeric), ..Fields::default() }
    }

    #[test]
    fn numeric_dates_repeat_on_both_sides() {
        let english = state(Language::English, numeric_date(), HourCycle::H12);
        assert_eq!(text(&english, BASE, BASE + 2 * DAY), "11/14/2023 \u{2013} 11/16/2023");
        let portuguese = state(Language::Portuguese, numeric_date(), HourCycle::H23);
        assert_eq!(text(&portuguese, BASE, BASE + 2 * DAY), "14/11/2023 \u{2013} 16/11/2023");
    }

    #[test]
    fn text_month_shares_what_does_not_differ() {
        let english = state(Language::English, long_date(), HourCycle::H12);
        assert_eq!(text(&english, BASE, BASE + 2 * DAY), "November 14 \u{2013} 16, 2023");
        assert_eq!(text(&english, BASE, BASE + 30 * DAY), "November 14 \u{2013} December 14, 2023");
        assert_eq!(text(&english, BASE, BASE + 365 * DAY), "November 14, 2023 \u{2013} November 13, 2024");
        let portuguese = state(Language::Portuguese, long_date(), HourCycle::H23);
        assert_eq!(text(&portuguese, BASE, BASE + 2 * DAY), "14 \u{2013} 16 de novembro de 2023");
        assert_eq!(text(&portuguese, BASE, BASE + 30 * DAY), "14 de novembro \u{2013} 14 de dezembro de 2023");
    }

    #[test]
    fn same_hour_cycle_shares_the_day_period() {
        let english = state(Language::English, hour_minute(), HourCycle::H12);
        assert_eq!(text(&english, BASE, BASE + 30 * 60_000), "10:13 \u{2013} 10:43 PM");
        assert_eq!(text(&english, BASE - 11 * HOUR, BASE), "11:13 AM \u{2013} 10:13 PM");
        let portuguese = state(Language::Portuguese, hour_minute(), HourCycle::H23);
        assert_eq!(text(&portuguese, BASE, BASE + HOUR), "22:13 \u{2013} 23:13");
    }

    #[test]
    fn difference_finer_than_the_pattern_is_a_single_value() {
        let english = state(Language::English, numeric_date(), HourCycle::H12);
        assert_eq!(text(&english, BASE, BASE + HOUR), "11/14/2023");
        let parts = format_range_parts_at(&english, BASE, BASE + HOUR);
        assert!(parts.iter().all(|part| part.2 == RangeSource::Shared));
        assert_eq!(text(&english, BASE, BASE), "11/14/2023");
    }

    #[test]
    fn date_and_time_on_the_same_day_share_the_date() {
        let fields = Fields { hour: hour_minute().hour, minute: hour_minute().minute, ..numeric_date() };
        let english = state(Language::English, fields, HourCycle::H12);
        assert_eq!(text(&english, BASE, BASE + 30 * 60_000), "11/14/2023, 10:13 \u{2013} 10:43 PM");
        assert_eq!(
            text(&english, BASE, BASE + 2 * DAY),
            "11/14/2023, 10:13 PM \u{2013} 11/16/2023, 10:13 PM"
        );
    }

    #[test]
    fn seconds_fall_back_to_the_full_values() {
        let fields = Fields { second: Some(Digits2::Numeric), ..hour_minute() };
        let english = state(Language::English, fields, HourCycle::H12);
        assert_eq!(text(&english, BASE, BASE + 1000), "10:13:20 PM \u{2013} 10:13:21 PM");
        let portuguese = state(Language::Portuguese, fields, HourCycle::H23);
        assert_eq!(text(&portuguese, BASE, BASE + 1000), "22:13:20 - 22:13:21");
    }

    #[test]
    fn time_only_across_days_prefixes_the_date() {
        let english = state(Language::English, hour_minute(), HourCycle::H12);
        assert_eq!(text(&english, BASE, BASE + DAY), "11/14/2023, 10:13 PM \u{2013} 11/15/2023, 10:13 PM");
    }

    #[test]
    fn iso_calendar_joins_the_two_formats_with_the_root_fallback() {
        let year_month = Fields { year: Some(Digits2::Numeric), month: Some(Month::Numeric), ..Fields::default() };
        for language in [Language::English, Language::Portuguese] {
            let mut iso = state(language, year_month, HourCycle::H23);
            iso.iso_calendar = true;
            assert_eq!(text(&iso, BASE, BASE + 60 * DAY), "2023-11 \u{2013} 2024-01");
            assert_eq!(text(&iso, BASE, BASE + HOUR), "2023-11");
        }
        let mut iso = state(Language::English, year_month, HourCycle::H12);
        iso.iso_calendar = true;
        let parts = format_range_parts_at(&iso, BASE, BASE + 60 * DAY);
        let kinds: Vec<(&str, &str, RangeSource)> = parts.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), *c)).collect();
        assert_eq!(
            kinds,
            vec![
                ("year", "2023", RangeSource::StartRange),
                ("literal", "-", RangeSource::StartRange),
                ("month", "11", RangeSource::StartRange),
                ("literal", " \u{2013} ", RangeSource::Shared),
                ("year", "2024", RangeSource::EndRange),
                ("literal", "-", RangeSource::EndRange),
                ("month", "01", RangeSource::EndRange),
            ]
        );
    }

    #[test]
    fn iso_calendar_without_a_month_name_is_a_single_shared_value() {
        let long_month = Fields { year: Some(Digits2::Numeric), month: Some(Month::Long), ..Fields::default() };
        let mut iso = state(Language::English, long_month, HourCycle::H12);
        iso.iso_calendar = true;
        // Medido no bun 1.4.2 (`en-US`, `iso8601`, 2023-11-15): no mesmo ano o mês invisível colapsa as pontas;
        // em anos diferentes saem as duas.
        assert_eq!(text(&iso, BASE, BASE + 5 * DAY), "2023 ");
        assert!(format_range_parts_at(&iso, BASE, BASE + 5 * DAY).iter().all(|part| part.2 == RangeSource::Shared));
        assert_eq!(text(&iso, BASE, BASE + 60 * DAY), "2023  \u{2013} 2024 ");
        // Com o dia o ICU ainda separa as pontas, e o dia segue o pedido (`" 5"`): sem o nome do mês sobra o espaço.
        let month_day = Fields { month: Some(Month::Long), day: Some(Digits2::Numeric), ..Fields::default() };
        let mut iso = state(Language::English, month_day, HourCycle::H12);
        iso.iso_calendar = true;
        assert_eq!(text(&iso, BASE, BASE + 60 * DAY), " 14 \u{2013}  13");
    }

    #[test]
    fn sources_mark_the_endpoints() {
        let english = state(Language::English, long_date(), HourCycle::H12);
        let parts = format_range_parts_at(&english, BASE, BASE + 2 * DAY);
        let tags: Vec<(&str, &str, RangeSource)> = parts.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), *c)).collect();
        assert_eq!(
            tags,
            vec![
                ("month", "November", RangeSource::StartRange),
                ("literal", " ", RangeSource::StartRange),
                ("day", "14", RangeSource::StartRange),
                ("literal", " \u{2013} ", RangeSource::Shared),
                ("day", "16", RangeSource::EndRange),
                ("literal", ", ", RangeSource::Shared),
                ("year", "2023", RangeSource::Shared),
            ]
        );
    }
}
