//! `Intl.DisplayNames` sem ICU (`IntlDisplayNames.cpp`, `IntlDisplayNamesPrototype.cpp`,
//! `IntlDisplayNamesConstructor.cpp`): `of` e `resolvedOptions` para `language`, `region`, `script`,
//! `currency`, `calendar` e `dateTimeField`.
//!
//! LOCALES cobertas: 69 de `intl_available_locales_data.rs` em `intl_display_names_data_more.rs` (gerado do
//! bun por `scripts/gen-display-names-data.js`; en-GB, pt-PT, es-MX, fr-CA, de-AT e zh-TW guardam só o que
//! difere do pai). As tabelas à mão de en e pt em `intl_display_names_data.rs` só servem de fonte dos códigos
//! do gerador e dos testes unitários (`table: None`); o símbolo de moeda delas vem de `default_number_format.rs`.
//!
//! DIVERGÊNCIA: o C++ delega a composição do nome de língua ao `uldn_localeDisplayName`; aqui a regra do
//! ICU (nome de dialeto de `língua-escrita-região`, `língua-região` ou `língua-escrita`, senão o nome da
//! língua com os detalhes entre parênteses separados por vírgula) está em [`language_display_name`]. Um
//! código sem dado nas tabelas se comporta como um código que o ICU não conhece (`fallback`).

use crate::host_function;
use crate::runtime::lookup::{native_entry};
use crate::runtime::default_number_format::{currency_narrow_symbol, currency_symbol};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_display_names_data::{
    bcp47_calendar_to_icu, calendar_name, currency_long_name, date_field_name, dialect_name, extra_narrow_symbol, has_currency,
    language_name, region_name, script_name, FieldWidth,
};
use crate::runtime::intl_display_names_data_more::{table_for_locale, STRINGS};
use crate::runtime::intl_table_lookup::sorted_position_by;
use crate::runtime::intl_locale_data::{canonicalize_tag, parse_language_tag, Language};
use crate::runtime::intl_number_format::is_unicode_locale_identifier_type;
use crate::runtime::intl_support::{
    construct_instance, get_options_object, new_object, option_enum, put, read_locale_matcher, resolve_locale_from,
    str_value, to_rust_string, with_instance, IntlClass, IntlEnum,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;

crate::intl_enum!(Style { Narrow => "narrow", Short => "short", Long => "long" });
crate::intl_enum!(DisplayType {
    Language => "language",
    Region => "region",
    Script => "script",
    Currency => "currency",
    Calendar => "calendar",
    DateTimeField => "dateTimeField"
});
crate::intl_enum!(Fallback { Code => "code", None => "none" });
crate::intl_enum!(LanguageDisplay { Dialect => "dialect", Standard => "standard" });

/// O estado de um `IntlDisplayNames`.
struct DisplayNamesState {
    locale: String,
    language: Language,
    /// A tabela medida do locale (ou da língua dele); `None` para as línguas sem tabela, que caem em en.
    table: Option<&'static MoreTable>,
    style: Style,
    kind: DisplayType,
    fallback: Fallback,
    language_display: LanguageDisplay,
}

fn is_alpha(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// `isUnicodeRegionSubtag`: duas letras ou três dígitos.
fn is_region_subtag(text: &str) -> bool {
    is_alpha(text, 2) || (text.len() == 3 && text.bytes().all(|byte| byte.is_ascii_digit()))
}

/// O nome de língua composto, como o `uldn_localeDisplayName` com `UDISPCTX_NO_SUBSTITUTE`: `None` se a
/// língua ou algum detalhe não tem nome (ou se há variantes, que não têm dados aqui).
fn language_display_name(state: &DisplayNamesState, canonical: &str) -> Option<String> {
    let tag = parse_language_tag(canonical)?;
    if !tag.variants.is_empty() {
        return None;
    }
    let dialects = state.language_display == LanguageDisplay::Dialect;
    let script = tag.script.as_deref();
    let region = tag.region.as_deref();
    let dialect = |key: String| if dialects { dialect_of(state, &key) } else { None };

    // Medido no bun: com script e região juntos só vale a chave completa `língua-escrita-região`; a
    // chave `língua-região` só vale sem escrita e `língua-escrita` só sem região.
    let dialect_key = match (script, region) {
        (Some(script), Some(region)) => format!("{}-{script}-{region}", tag.language),
        (None, Some(region)) => format!("{}-{region}", tag.language),
        (Some(script), None) => format!("{}-{script}", tag.language),
        (None, None) => String::new(),
    };
    let dialect_base = if dialect_key.is_empty() { None } else { dialect(dialect_key) };
    let (script_detail, region_detail) = if dialect_base.is_some() { (None, None) } else { (script, region) };
    let base = match dialect_base {
        Some(name) => name,
        None => language_of(state, &tag.language)?,
    };
    let mut details: Vec<String> = Vec::new();
    if let Some(script) = script_detail {
        let name = script_of(state, script)?;
        // O ICU compõe a escrita entre parênteses sem a maiúscula inicial que ela tem sozinha (`ru`).
        details.push(match state.table {
            Some(table) if table.lowercase_script_detail => lowercase_first(name),
            _ => name.to_string(),
        });
    }
    if let Some(region) = region_detail {
        details.push(region_of(state, region)?.to_string());
    }
    let (open, separator, close) = state.table.map_or((" (", ", ", ")"), |table| table.details);
    // Parênteses dentro dos detalhes viram colchetes quando o padrão também usa parênteses (`pa`: `ਸੰਯੁਕਤ ਰਾਜ [ਅਮਰੀਕਾ]`).
    let joined = details.join(separator);
    let joined = if open.contains('(') { joined.replace('(', "[").replace(')', "]") } else { joined };
    let composed = if details.is_empty() { base.to_string() } else { format!("{base}{open}{joined}{close}") };
    // As tabelas medidas já trazem a capitalização do ICU; só as de en e pt dependem desta regra.
    Some(if state.table.is_some() { composed } else { capitalize_standalone(&composed) })
}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Os dados de uma língua além de en e pt, medidos no bun (`intl_display_names_data_more.rs`). Um nome
/// curto vazio é "igual ao longo"; uma moeda sem nome longo ou símbolo tem o campo vazio.
pub struct MoreTable {
    pub locale: &'static str,
    /// A tabela do locale pai (`en` para `en-GB`, `zh` para `zh-TW`): as linhas que faltam aqui valem as dela.
    /// Uma linha com o nome longo vazio é lápide: o nome não existe neste locale mesmo que o pai o tenha.
    pub parent: Option<&'static MoreTable>,
    /// Abertura, separador e fechamento dos detalhes de um nome de língua (` (`, `, `, `)`).
    pub details: (&'static str, &'static str, &'static str),
    pub lowercase_script_detail: bool,
    pub languages: &'static [PoolRow3],
    pub dialects: &'static [PoolRow3],
    pub scripts: &'static [PoolRow2],
    pub regions: &'static [PoolRow3],
    pub currencies: &'static [PoolRow4],
    pub calendars: &'static [PoolRow2],
    /// Os campos de `dateTimeField`: (código, nome long, nome short, nome narrow).
    pub fields: &'static [PoolRow4],
}

/// Linhas das tabelas: índices em `STRINGS` (a chave primeiro; as listas vêm ordenadas por ela).
pub type PoolRow2 = (u16, u16);
pub type PoolRow3 = (u16, u16, u16);
pub type PoolRow4 = (u16, u16, u16, u16);

/// O texto de um índice do pool (0 é a string vazia).
fn pooled(index: u16) -> &'static str {
    STRINGS[usize::from(index)]
}

fn long_or_short(long: u16, short: u16, use_short: bool) -> &'static str {
    pooled(if use_short && short != 0 { short } else { long })
}

impl MoreTable {
    /// A linha de `code` nesta tabela ou, na falta, na do pai. `None` se ninguém a tem ou se é lápide.
    fn lookup<T: Copy>(&self, rows: fn(&MoreTable) -> &'static [T], key: fn(&T) -> u16, tombstone: fn(&T) -> bool, code: &str) -> Option<T> {
        let own = rows(self);
        match sorted_position_by(own, |row| pooled(key(row)), code).map(|position| own[position]) {
            Some(row) if tombstone(&row) => None,
            Some(row) => Some(row),
            None => self.parent.and_then(|parent| parent.lookup(rows, key, tombstone, code)),
        }
    }

    fn language(&self, code: &str, short: bool) -> Option<&'static str> {
        self.lookup(|t| t.languages, |r| r.0, |r| r.1 == 0, code).map(|row| long_or_short(row.1, row.2, short))
    }

    fn dialect(&self, key: &str, short: bool) -> Option<&'static str> {
        self.lookup(|t| t.dialects, |r| r.0, |r| r.1 == 0, key).map(|row| long_or_short(row.1, row.2, short))
    }

    fn script(&self, code: &str) -> Option<&'static str> {
        self.lookup(|t| t.scripts, |r| r.0, |r| r.1 == 0, code).map(|row| pooled(row.1))
    }

    fn region(&self, code: &str, short: bool) -> Option<&'static str> {
        self.lookup(|t| t.regions, |r| r.0, |r| r.1 == 0, code).map(|row| long_or_short(row.1, row.2, short))
    }

    fn calendar(&self, key: &str) -> Option<&'static str> {
        self.lookup(|t| t.calendars, |r| r.0, |r| r.1 == 0, key).map(|row| pooled(row.1))
    }

    fn date_field(&self, code: &str, style: Style) -> Option<&'static str> {
        let row = self.lookup(|t| t.fields, |r| r.0, |_| false, code)?;
        Some(pooled(match style {
            Style::Long => row.1,
            Style::Short => row.2,
            Style::Narrow => row.3,
        }))
    }

    /// O nome da moeda em `style`; sem dado de símbolo, o próprio código (como o `ucurr_getName`).
    fn currency(&self, code: &str, style: Style) -> Option<String> {
        let row = self.lookup(|t| t.currencies, |r| r.0, |_| false, code)?;
        let field = pooled(match style {
            Style::Long => row.1,
            Style::Short => row.2,
            Style::Narrow => row.3,
        });
        match (field.is_empty(), style) {
            (false, _) => Some(field.to_string()),
            (true, Style::Long) => None,
            (true, _) => Some(code.to_string()),
        }
    }
}

fn is_short(state: &DisplayNamesState) -> bool {
    state.style != Style::Long
}

/// Cada busca abaixo usa a tabela medida da língua do locale, ou as tabelas de en e pt.
fn language_of(state: &DisplayNamesState, code: &str) -> Option<&'static str> {
    match state.table {
        Some(table) => table.language(code, is_short(state)),
        None => language_name(code, state.language),
    }
}

fn dialect_of(state: &DisplayNamesState, key: &str) -> Option<&'static str> {
    match state.table {
        Some(table) => table.dialect(key, is_short(state)),
        None => dialect_name(key, state.language, is_short(state)),
    }
}

fn script_of(state: &DisplayNamesState, code: &str) -> Option<&'static str> {
    match state.table {
        Some(table) => table.script(code),
        None => script_name(code, state.language),
    }
}

fn region_of(state: &DisplayNamesState, code: &str) -> Option<&'static str> {
    match state.table {
        Some(table) => table.region(code, is_short(state)),
        None => region_name(code, state.language, is_short(state)),
    }
}

fn calendar_of(state: &DisplayNamesState, key: &str) -> Option<&'static str> {
    match state.table {
        Some(table) => table.calendar(key),
        None => calendar_name(key, state.language),
    }
}

/// `UDISPCTX_CAPITALIZATION_FOR_STANDALONE`: o português do CLDR capitaliza a primeira letra de nomes de
/// língua e de calendário (`Inglês (Estados Unidos)`, `Calendário Gregoriano`), não os de escrita.
fn capitalize_standalone(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// O nome de moeda em `style`: `None` quando o `ucurr_getName` devolveria o próprio código sem dados.
fn currency_display_name(state: &DisplayNamesState, code: &str) -> Option<String> {
    if let Some(table) = state.table {
        return table.currency(code, state.style);
    }
    let language = state.language;
    match state.style {
        Style::Long => currency_long_name(code, language).map(str::to_string),
        Style::Short => currency_symbol(code, language)
            .or_else(|| (code == "XXX").then_some("\u{a4}"))
            .map(str::to_string)
            .or_else(|| has_currency(code).then(|| code.to_string())),
        Style::Narrow => currency_narrow_symbol(code, language)
            // Medido no bun: o `pt` mantém `NT$` no estreito do dólar taiwanês, o `en` usa `$`.
            .map(|symbol| if code == "TWD" && language == Language::Portuguese { "NT$" } else { symbol })
            .or_else(|| extra_narrow_symbol(code))
            .or_else(|| (code == "XXX").then_some("\u{a4}"))
            .map(str::to_string)
            .or_else(|| has_currency(code).then(|| code.to_string())),
    }
}

/// `IntlDisplayNames::of`: `Ok(None)` é `undefined`, `Err` é o `RangeError`.
fn display_name_of(state: &DisplayNamesState, code: &str) -> Result<Option<String>, &'static str> {
    // `fallback`: o código canônico, ou `undefined`.
    let fallback = |canonical: String| Ok(if state.fallback == Fallback::Code { Some(canonical) } else { None });
    match state.kind {
        DisplayType::Language => {
            let parsed = parse_language_tag(code).filter(|tag| tag.extensions.is_empty() && tag.private_use.is_empty());
            if parsed.is_none() {
                return Err("argument is not a language id");
            }
            let canonical = canonicalize_tag(code).expect("tag já analisada");
            match language_display_name(state, &canonical) {
                Some(name) => Ok(Some(name)),
                None => fallback(canonical),
            }
        }
        DisplayType::Region => {
            if !is_region_subtag(code) {
                return Err("argument is not a region subtag");
            }
            let canonical = code.to_ascii_uppercase();
            match region_of(state, &canonical) {
                Some(name) => Ok(Some(name.to_string())),
                None => fallback(canonical),
            }
        }
        DisplayType::Script => {
            if !is_alpha(code, 4) {
                return Err("argument is not a script subtag");
            }
            let canonical = code[..1].to_ascii_uppercase() + &code[1..].to_ascii_lowercase();
            match script_of(state, &canonical) {
                Some(name) => Ok(Some(name.to_string())),
                None => fallback(canonical),
            }
        }
        DisplayType::Currency => {
            if !is_alpha(code, 3) {
                return Err("argument is not a well-formed currency code");
            }
            let canonical = code.to_ascii_uppercase();
            match currency_display_name(state, &canonical) {
                Some(name) => Ok(Some(name)),
                None => fallback(canonical),
            }
        }
        DisplayType::Calendar => {
            if !is_unicode_locale_identifier_type(code) {
                return Err("argument is not a calendar code");
            }
            let lowered = code.to_ascii_lowercase();
            let canonical = bcp47_calendar_to_icu(&lowered).to_string();
            match calendar_of(state, &canonical) {
                Some(name) => Ok(Some(name.to_string())),
                None => fallback(canonical),
            }
        }
        DisplayType::DateTimeField => {
            let width = match state.style {
                Style::Long => FieldWidth::Wide,
                Style::Short => FieldWidth::Abbreviated,
                Style::Narrow => FieldWidth::Narrow,
            };
            let name = match state.table {
                Some(table) => table.date_field(code, state.style),
                None => date_field_name(code, state.language, width),
            };
            match name {
                Some(name) => Ok(Some(name.to_string())),
                None => Err("argument is not a dateTimeField code"),
            }
        }
    }
}

/// `IntlDisplayNames::initializeDisplayNames`.
fn initialize(global_object: &JSGlobalObject, locales: JSValue, options_value: JSValue) -> Result<DisplayNamesState, Thrown> {
    let resolved = resolve_locale_from(global_object, locales, &[])?;
    let options = get_options_object(options_value)?;
    read_locale_matcher(global_object, options)?;
    let style = option_enum::<Style>(global_object, options, "style", "style must be either \"narrow\", \"short\", or \"long\"")?
        .unwrap_or(Style::Long);
    let kind = option_enum::<DisplayType>(
        global_object,
        options,
        "type",
        "type must be either \"language\", \"region\", \"script\", \"currency\", \"calendar\", or \"dateTimeField\"",
    )?
    .ok_or_else(|| Thrown::type_error("type must not be undefined"))?;
    let fallback = option_enum::<Fallback>(global_object, options, "fallback", "fallback must be either \"code\" or \"none\"")?
        .unwrap_or(Fallback::Code);
    let language_display = option_enum::<LanguageDisplay>(
        global_object,
        options,
        "languageDisplay",
        "languageDisplay must be either \"dialect\" or \"standard\"",
    )?
    .unwrap_or(LanguageDisplay::Dialect);
    let table = table_for_locale(&resolved.locale);
    Ok(DisplayNamesState { locale: resolved.locale, language: resolved.language, table, style, kind, fallback, language_display })
}

fn construct_display_names_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

fn call_display_names_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    crate::runtime::collection_support::constructor_cannot_be_called_as_function("DisplayNames")
}

fn of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DisplayNamesState, _>(
        call.this_value(),
        "Intl.DisplayNames.prototype.of called on value that's not a DisplayNames",
        |state, _| {
            let code = to_rust_string(global_object, call.argument(0))?;
            match display_name_of(state, &code).map_err(Thrown::range_error)? {
                Some(name) => Ok(str_value(global_object.vm(), &name)),
                None => Ok(JSValue::undefined()),
            }
        },
    )
}

fn resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DisplayNamesState, _>(
        call.this_value(),
        "Intl.DisplayNames.prototype.resolvedOptions called on value that's not a DisplayNames",
        |state, _| {
            let vm = global_object.vm();
            let options = new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(global_object, &options, "style", str_value(vm, state.style.as_str()));
            put(global_object, &options, "type", str_value(vm, state.kind.as_str()));
            put(global_object, &options, "fallback", str_value(vm, state.fallback.as_str()));
            if state.kind == DisplayType::Language {
                put(global_object, &options, "languageDisplay", str_value(vm, state.language_display.as_str()));
            }
            Ok(options.as_value())
        },
    )
}

host_function!(call_display_names, call_display_names_body);
host_function!(construct_display_names, construct_display_names_body);
host_function!(display_names_proto_of, of_body);
host_function!(display_names_proto_resolved_options, resolved_options_body);

crate::intl_prototype_s_info!(
    DISPLAY_NAMES_PROTOTYPE_S_INFO,
    "Intl.DisplayNames",
    [
        native_entry("of", display_names_proto_of, 1),
        native_entry("resolvedOptions", display_names_proto_resolved_options, 0),
    ]
);

/// `IntlDisplayNamesConstructor` e `IntlDisplayNamesPrototype` (o construtor tem `length` 2).
pub fn install_display_names(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "DisplayNames",
        length: 2,
        has_supported_locales_of: true,
        call: call_display_names,
        construct: construct_display_names,
    };
    class.install_with_table(global_object, intl, &DISPLAY_NAMES_PROTOTYPE_S_INFO);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(language: Language, kind: DisplayType, style: Style) -> DisplayNamesState {
        DisplayNamesState {
            locale: String::new(),
            language,
            table: None,
            style,
            kind,
            fallback: Fallback::Code,
            language_display: LanguageDisplay::Dialect,
        }
    }

    fn of(language: Language, kind: DisplayType, style: Style, code: &str) -> Option<String> {
        display_name_of(&state(language, kind, style), code).expect("sem RangeError")
    }

    const EN: Language = Language::English;
    const PT: Language = Language::Portuguese;

    #[test]
    fn languages() {
        let english = |code| of(EN, DisplayType::Language, Style::Long, code).unwrap();
        assert_eq!(english("en"), "English");
        assert_eq!(english("en-US"), "American English");
        assert_eq!(english("en-GB"), "British English");
        assert_eq!(english("pt-BR"), "Brazilian Portuguese");
        assert_eq!(english("zh-Hans"), "Simplified Chinese");
        assert_eq!(english("zh-Hans-CN"), "Chinese (Simplified, China)");
        assert_eq!(english("en-Latn-US"), "English (Latin, United States)");
        assert_eq!(english("fr-FR"), "French (France)");
        assert_eq!(english("fr-Latn-FR"), "French (Latin, France)");
        assert_eq!(of(EN, DisplayType::Language, Style::Short, "en-GB").unwrap(), "UK English");
        assert_eq!(of(PT, DisplayType::Language, Style::Long, "en").unwrap(), "Inglês");
        assert_eq!(of(PT, DisplayType::Language, Style::Long, "en-US").unwrap(), "Inglês (Estados Unidos)");
        assert_eq!(of(PT, DisplayType::Language, Style::Short, "en-US").unwrap(), "Inglês (EUA)");
        assert_eq!(of(PT, DisplayType::Language, Style::Long, "pt-BR").unwrap(), "Português (Brasil)");
        assert_eq!(of(PT, DisplayType::Language, Style::Long, "zh-Hant").unwrap(), "Chinês tradicional");
        assert_eq!(of(PT, DisplayType::Language, Style::Long, "zh-Hant-TW").unwrap(), "Chinês (tradicional, Taiwan)");
        assert_eq!(of(PT, DisplayType::Language, Style::Long, "de-DE").unwrap(), "Alemão (Alemanha)");
    }

    #[test]
    fn standard_language_display() {
        let mut standard = state(EN, DisplayType::Language, Style::Long);
        standard.language_display = LanguageDisplay::Standard;
        assert_eq!(display_name_of(&standard, "en-GB").unwrap().unwrap(), "English (United Kingdom)");
    }

    #[test]
    fn language_fallback_and_errors() {
        assert_eq!(of(EN, DisplayType::Language, Style::Long, "xx-YY"), Some("xx-YY".to_string()));
        let mut none = state(EN, DisplayType::Language, Style::Long);
        none.fallback = Fallback::None;
        assert_eq!(display_name_of(&none, "xx"), Ok(None));
        assert_eq!(display_name_of(&none, "en-u-ca-gregory"), Err("argument is not a language id"));
        assert_eq!(display_name_of(&none, "e"), Err("argument is not a language id"));
    }

    #[test]
    fn regions_and_scripts() {
        assert_eq!(of(EN, DisplayType::Region, Style::Long, "BR").unwrap(), "Brazil");
        assert_eq!(of(EN, DisplayType::Region, Style::Long, "br").unwrap(), "Brazil");
        assert_eq!(of(PT, DisplayType::Region, Style::Long, "US").unwrap(), "Estados Unidos");
        assert_eq!(of(EN, DisplayType::Region, Style::Short, "GB").unwrap(), "UK");
        assert_eq!(of(EN, DisplayType::Region, Style::Long, "419").unwrap(), "Latin America");
        assert_eq!(of(EN, DisplayType::Region, Style::Long, "QQ").unwrap(), "QQ");
        assert_eq!(of(EN, DisplayType::Script, Style::Long, "latn").unwrap(), "Latin");
        assert_eq!(of(PT, DisplayType::Script, Style::Long, "Cyrl").unwrap(), "cirílico");
        assert_eq!(display_name_of(&state(EN, DisplayType::Region, Style::Long), "USA"), Err("argument is not a region subtag"));
        assert_eq!(display_name_of(&state(EN, DisplayType::Script, Style::Long), "La"), Err("argument is not a script subtag"));
    }

    #[test]
    fn currencies() {
        assert_eq!(of(EN, DisplayType::Currency, Style::Long, "USD").unwrap(), "US Dollar");
        assert_eq!(of(PT, DisplayType::Currency, Style::Long, "brl").unwrap(), "Real brasileiro");
        assert_eq!(of(EN, DisplayType::Currency, Style::Short, "USD").unwrap(), "$");
        assert_eq!(of(PT, DisplayType::Currency, Style::Short, "USD").unwrap(), "US$");
        assert_eq!(of(EN, DisplayType::Currency, Style::Narrow, "CAD").unwrap(), "$");
        assert_eq!(of(EN, DisplayType::Currency, Style::Short, "AED").unwrap(), "AED");
        assert_eq!(of(EN, DisplayType::Currency, Style::Long, "QQQ").unwrap(), "QQQ");
        assert_eq!(
            display_name_of(&state(EN, DisplayType::Currency, Style::Long), "US"),
            Err("argument is not a well-formed currency code")
        );
    }

    #[test]
    fn calendars() {
        assert_eq!(of(EN, DisplayType::Calendar, Style::Long, "gregory").unwrap(), "Gregorian Calendar");
        assert_eq!(of(PT, DisplayType::Calendar, Style::Long, "japanese").unwrap(), "Calendário Japonês");
        assert_eq!(of(EN, DisplayType::Calendar, Style::Long, "islamic").unwrap(), "Hijri Calendar");
        assert_eq!(of(PT, DisplayType::Calendar, Style::Long, "islamic-tbla").unwrap(), "islamic-tbla");
        assert_eq!(of(EN, DisplayType::Calendar, Style::Long, "ethioaa").unwrap(), "Ethiopic Amete Alem Calendar");
        assert_eq!(of(EN, DisplayType::Calendar, Style::Long, "Bogus").unwrap(), "bogus");
        assert_eq!(display_name_of(&state(EN, DisplayType::Calendar, Style::Long), "a"), Err("argument is not a calendar code"));
    }

    #[test]
    fn date_time_fields() {
        assert_eq!(of(EN, DisplayType::DateTimeField, Style::Long, "year").unwrap(), "year");
        assert_eq!(of(EN, DisplayType::DateTimeField, Style::Short, "year").unwrap(), "yr.");
        assert_eq!(of(PT, DisplayType::DateTimeField, Style::Long, "weekOfYear").unwrap(), "semana");
        assert_eq!(of(PT, DisplayType::DateTimeField, Style::Long, "timeZoneName").unwrap(), "fuso horário");
        assert_eq!(
            display_name_of(&state(EN, DisplayType::DateTimeField, Style::Long), "week"),
            Err("argument is not a dateTimeField code")
        );
    }
}
