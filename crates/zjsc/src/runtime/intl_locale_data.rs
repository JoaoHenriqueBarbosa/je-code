//! Dados de locale do `Intl` sem ICU: análise e canonicalização de tags BCP 47 (`unicode_locale_id` do
//! UTS 35), a lista de locales disponíveis, o `BestAvailableLocale` e a resolução do locale pedido, e as
//! tabelas de subtags prováveis do `Intl.Locale` (`maximize` e `minimize`).
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - A disponibilidade de locale vem dos dados compilados do icu4x (CLDR): uma língua é suportada se
//!   consta nos subtags prováveis (`icu_locale::LocaleExpander`), e o `BestAvailableLocale` mantém a
//!   região pedida só quando o par existe na lista medida no bun (`AVAILABLE_LOCALES`: `en-GB`, `pt-PT`,
//!   `zh-Hant-TW`); `en-ZZ` cai em `en` e `zh-Hant-ZZ` em `zh-Hant`, como o `bestAvailableLocale` do JSC;
//!   um locale de língua sem dados cai em `en-US`.
//! - As extensões `-u-` só entram no locale resolvido quando a classe as honra
//!   ([`ResolvedLocale::tag_with`]); as outras são descartadas, como a especificação manda para chave
//!   relevante com valor não suportado.
//! - Os aliases de língua, região, valor `-u-` (inclusive `tz`) e tag grandfathered vêm de
//!   `intl_locale_aliases_data.rs`, medidos no bun (`scripts/gen-locale-aliases.js`); as tabelas de
//!   subtags prováveis cobrem as línguas mais comuns, não o CLDR inteiro.

use super::intl_available_locales_data::{AVAILABLE_LANGUAGES, AVAILABLE_LOCALES};
use super::intl_locale_aliases_data;

/// A língua cujos dados existem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    Portuguese,
}

impl Language {
    /// A língua dos dados que atendem `locale` (`pt` e `pt-*` são português; o resto é inglês).
    pub fn of_locale(locale: &str) -> Language {
        if locale == "pt" || locale.starts_with("pt-") {
            Language::Portuguese
        } else {
            Language::English
        }
    }
}

/// `DefaultLocale()`: o locale do ICU de um processo sem `LANG` do Debian.
pub const DEFAULT_LOCALE: &str = "en-US";

thread_local! {
    /// O locale padrão posto pelo hospedeiro (`Emulation.setLocaleOverride` do CDP), já canônico e com dados.
    static DEFAULT_LOCALE_OVERRIDE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// O gancho do hospedeiro: troca o locale padrão do `Intl` (`DateTimeFormat`, `NumberFormat`, `Collator`,
/// `PluralRules`..., e `toLocaleString`, `localeCompare`, `toLocaleUpperCase` sem `locales`) nesta thread, que é a
/// do realm da página, ou volta ao `en-US` com `None`. Vale já para o código que roda depois da chamada. A tag
/// inválida, ou sem dados, deixa o padrão como estava e devolve `false`.
pub fn set_default_locale(tag: Option<&str>) -> bool {
    let chosen = match tag {
        None => None,
        Some(tag) => {
            let Some(canonical) = canonicalize_tag(tag) else { return false };
            let Some(parsed) = parse_language_tag(&canonical) else { return false };
            let Some(found) = best_available_locale(&parsed.base_name()) else { return false };
            Some(found)
        }
    };
    DEFAULT_LOCALE_OVERRIDE.with(|cell| *cell.borrow_mut() = chosen);
    true
}

/// `DefaultLocale()` em vigor: o do hospedeiro, ou `en-US`.
pub fn default_locale() -> String {
    DEFAULT_LOCALE_OVERRIDE.with(|cell| cell.borrow().clone()).unwrap_or_else(|| DEFAULT_LOCALE.to_string())
}

/// Se o hospedeiro trocou o locale padrão (os atalhos de `en-US` deixam de valer).
pub fn default_locale_is_overridden() -> bool {
    DEFAULT_LOCALE_OVERRIDE.with(|cell| cell.borrow().is_some())
}

/// Se `language` está na lista de `intlAvailableLocales` medida no bun
/// (`intl_available_locales_data::AVAILABLE_LANGUAGES`, gerada por `scripts/gen-available-locales.js`).
/// É o único conjunto de locales disponíveis das classes do Intl (o Collator tem o seu, em
/// `intl_collator_tailoring::collator_locale`, que passa por `best_available_by` do mesmo jeito).
/// `und` não é um locale com dados.
fn language_has_data(language: &str) -> bool {
    crate::runtime::intl_table_lookup::contains_sorted(&AVAILABLE_LANGUAGES, language)
}

/// A cadeia de fallback de dados do CLDR de `tag` (`en-GB`, `en-001`, `en`, `und`), do icu4x
/// (`LocaleFallbacker`); é a ordem em que os módulos de formatação procuram dados.
pub fn data_fallback_chain(tag: &str) -> Vec<String> {
    use icu_locale::fallback::{LocaleFallbackConfig, LocaleFallbacker};
    use icu_locale::Locale;
    let Ok(locale) = Locale::try_from_str(tag) else { return Vec::new() };
    let fallbacker = LocaleFallbacker::new().for_config(LocaleFallbackConfig::default());
    let mut iterator = fallbacker.fallback_for((&locale).into());
    let mut chain = Vec::new();
    while !iterator.get().is_unknown() {
        chain.push(iterator.get().to_string());
        iterator.step();
    }
    chain
}

/// Uma tag de idioma analisada e já em minúsculas, com a caixa canônica aplicada no `script` e na
/// `region`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageTag {
    pub language: String,
    pub script: Option<String>,
    pub region: Option<String>,
    pub variants: Vec<String>,
    /// As extensões (singleton e subtags), na ordem da tag.
    pub extensions: Vec<(char, Vec<String>)>,
    pub private_use: Vec<String>,
}

fn is_alpha(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn is_digit(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_alphanumeric(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// `Title` para o `script`: primeira letra maiúscula, o resto minúsculo.
fn title_case(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
        None => String::new(),
    }
}

/// `isStructurallyValidLanguageTag` e a análise: `None` se a tag não é um `unicode_locale_id` bem
/// formado.
pub fn parse_language_tag(tag: &str) -> Option<LanguageTag> {
    if tag.is_empty() || !tag.is_ascii() {
        return None;
    }
    let lower = tag.to_ascii_lowercase();
    let subtags: Vec<&str> = lower.split('-').collect();
    if subtags.iter().any(|subtag| subtag.len() > 8 || !is_alphanumeric(subtag)) {
        return None;
    }

    let language = subtags[0];
    if !is_alpha(language) || !((2..=3).contains(&language.len()) || (5..=8).contains(&language.len())) {
        return None;
    }
    let mut result = LanguageTag { language: language.to_string(), ..LanguageTag::default() };
    let mut index = 1;

    if let Some(subtag) = subtags.get(index) {
        if subtag.len() == 4 && is_alpha(subtag) {
            result.script = Some(title_case(subtag));
            index += 1;
        }
    }
    if let Some(subtag) = subtags.get(index) {
        if (subtag.len() == 2 && is_alpha(subtag)) || (subtag.len() == 3 && is_digit(subtag)) {
            result.region = Some(subtag.to_ascii_uppercase());
            index += 1;
        }
    }
    while let Some(subtag) = subtags.get(index) {
        let is_variant = (5..=8).contains(&subtag.len()) || (subtag.len() == 4 && subtag.as_bytes()[0].is_ascii_digit());
        if !is_variant {
            break;
        }
        if result.variants.iter().any(|variant| variant == subtag) {
            return None;
        }
        result.variants.push((*subtag).to_string());
        index += 1;
    }

    while index < subtags.len() {
        let subtag = subtags[index];
        if subtag.len() != 1 {
            return None;
        }
        let singleton = subtag.chars().next()?;
        index += 1;
        let start = index;
        if singleton == 'x' {
            if start == subtags.len() {
                return None;
            }
            result.private_use = subtags[start..].iter().map(|subtag| (*subtag).to_string()).collect();
            break;
        }
        if result.extensions.iter().any(|(existing, _)| *existing == singleton) {
            return None;
        }
        while index < subtags.len() && subtags[index].len() >= 2 {
            index += 1;
        }
        if index == start {
            return None;
        }
        result.extensions.push((singleton, subtags[start..index].iter().map(|subtag| (*subtag).to_string()).collect()));
    }
    Some(result)
}

/// Aliases de língua (`languageAlias` do CLDR), medidos no bun 1.4.2 sobre todas as línguas de 2 e 3 letras
/// (`scripts/gen-locale-aliases.js`). `tl`, `cmn`, `arb`, `swc`, `sh` e `no` não estão: ficam como vieram.
fn language_alias(language: &str) -> Option<&'static str> {
    lookup_pair(&intl_locale_aliases_data::LANGUAGE_ALIASES, language)
}

/// Aliases de região (`territoryAlias` do CLDR) medidos no bun: só `BU`, `DD`, `FX`, `TP`, `YD`, `ZR`.
fn region_alias(region: &str) -> Option<&'static str> {
    lookup_pair(&intl_locale_aliases_data::REGION_ALIASES, region)
}

fn lookup_pair(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(candidate, _)| *candidate == key).map(|(_, value)| *value)
}

/// O valor novo de uma chave `-u-` (`ca-islamicc` dá `islamic-civil`; `kb-yes` dá o valor vazio, que
/// a tag escreve só como `kb`). `value` é o valor com os subtags unidos por `-`.
fn unicode_value_alias(key: &str, value: &str) -> Option<&'static str> {
    if key == "tz" {
        return lookup_pair(&intl_locale_aliases_data::TIMEZONE_ALIASES, value);
    }
    intl_locale_aliases_data::UNICODE_VALUE_ALIASES
        .iter()
        .find(|(alias_key, alias_value, _)| *alias_key == key && *alias_value == value)
        .map(|(_, _, replacement)| *replacement)
}

/// Tag grandfathered que o bun troca por uma língua (`art-lojban` dá `jbo`, `zh-guoyu` dá `cmn`): o
/// resto (`i-*`, `no-bok`, `sgn-BE-FR`, `en-GB-oed`...) já é rejeitado pela análise.
fn grandfathered_language(language: &str, variant: &str) -> Option<&'static str> {
    let key = format!("{language}-{variant}");
    let replacement = intl_locale_aliases_data::GRANDFATHERED.iter().find(|(tag, _)| *tag == key)?.1?;
    (replacement != key).then_some(replacement)
}

/// Canonicaliza o `-u-`: atributos em ordem, chaves em ordem (a primeira de cada chave vale) e o valor
/// `true` descartado.
fn canonical_unicode_extension(subtags: &[String]) -> Vec<String> {
    let mut attributes: Vec<String> = Vec::new();
    let mut keywords: Vec<(String, Vec<String>)> = Vec::new();
    for subtag in subtags {
        if subtag.len() == 2 {
            keywords.push((subtag.clone(), Vec::new()));
        } else if let Some((_, value)) = keywords.last_mut() {
            value.push(subtag.clone());
        } else {
            attributes.push(subtag.clone());
        }
    }
    attributes.sort();
    attributes.dedup();
    let mut seen: Vec<String> = Vec::new();
    let mut unique: Vec<(String, Vec<String>)> = Vec::new();
    for (key, value) in keywords {
        if !seen.contains(&key) {
            seen.push(key.clone());
            unique.push((key, value));
        }
    }
    unique.sort_by(|a, b| a.0.cmp(&b.0));

    let mut result = attributes;
    for (key, value) in unique {
        let value = match unicode_value_alias(&key, &value.join("-")) {
            Some("") => Vec::new(),
            Some(replacement) => replacement.split('-').map(str::to_string).collect(),
            None => value,
        };
        result.push(key);
        if !(value.len() == 1 && value[0] == "true") {
            result.extend(value);
        }
    }
    result
}

impl LanguageTag {
    /// A tag com os aliases de língua e região trocados.
    pub fn with_aliases_replaced(mut self) -> LanguageTag {
        if self.script.is_none() && self.region.is_none() && self.variants.len() == 1 {
            if let Some(replacement) = grandfathered_language(&self.language, &self.variants[0]) {
                self.language = replacement.to_string();
                self.variants.clear();
            }
        }
        // A variante `posix`, sozinha, migra para a extensão: `en-US-posix` vira `en-US-u-va-posix`.
        if self.variants.len() == 1 && self.variants[0] == "posix" {
            self.variants.clear();
            let keyword = ["va".to_string(), "posix".to_string()];
            match self.extensions.iter_mut().find(|(singleton, _)| *singleton == 'u') {
                Some((_, subtags)) => subtags.extend(keyword),
                None => self.extensions.push(('u', keyword.to_vec())),
            }
        }
        if let Some(replacement) = language_alias(&self.language) {
            self.language = replacement.to_string();
        }
        if let Some(region) = &self.region {
            if let Some(replacement) = region_alias(region) {
                self.region = Some(replacement.to_string());
            }
        }
        self
    }

    /// `language[-script][-region][-variants]`: o `baseName` do `Intl.Locale`, com as variantes em
    /// ordem alfabética.
    pub fn base_name(&self) -> String {
        let mut parts: Vec<String> = vec![self.language.clone()];
        parts.extend(self.script.clone());
        parts.extend(self.region.clone());
        let mut variants = self.variants.clone();
        variants.sort();
        parts.extend(variants);
        parts.join("-")
    }

    /// A tag canônica: base, extensões em ordem de singleton e o uso privado no fim.
    pub fn canonical(&self) -> String {
        let mut parts: Vec<String> = vec![self.base_name()];
        let mut extensions = self.extensions.clone();
        extensions.sort_by_key(|(singleton, _)| *singleton);
        for (singleton, subtags) in extensions {
            parts.push(singleton.to_string());
            if singleton == 'u' {
                parts.extend(canonical_unicode_extension(&subtags));
            } else {
                parts.extend(subtags);
            }
        }
        if !self.private_use.is_empty() {
            parts.push("x".to_string());
            parts.extend(self.private_use.clone());
        }
        parts.join("-")
    }

    /// O valor de uma chave do `-u-` (`"nu"` dá `"latn"`; chave sem valor dá `""`).
    pub fn unicode_keyword(&self, key: &str) -> Option<String> {
        let (_, subtags) = self.extensions.iter().find(|(singleton, _)| *singleton == 'u')?;
        let canonical = canonical_unicode_extension(subtags);
        let position = canonical.iter().position(|subtag| subtag == key && subtag.len() == 2)?;
        let value: Vec<&str> = canonical[position + 1..].iter().take_while(|subtag| subtag.len() > 2).map(String::as_str).collect();
        Some(value.join("-"))
    }
}

/// `CanonicalizeUnicodeLocaleId` sobre uma tag em texto: `None` se ela não é bem formada.
pub fn canonicalize_tag(tag: &str) -> Option<String> {
    Some(parse_language_tag(tag)?.with_aliases_replaced().canonical())
}

/// `BestAvailableLocale(availableLocales, locale)` com o conjunto dado por `is_available`: corta o último
/// subtag (e o singleton que o precede) até achar um locale disponível.
pub fn best_available_by(locale: &str, is_available: impl Fn(&str) -> bool) -> Option<String> {
    let mut candidate = locale.to_string();
    loop {
        if is_available(&candidate) {
            return Some(candidate);
        }
        let position = candidate.rfind('-')?;
        let mut cut = position;
        if position >= 2 && candidate.as_bytes()[position - 2] == b'-' {
            cut = position - 2;
        }
        candidate.truncate(cut);
    }
}

/// `BestAvailableLocale` sobre os locales que têm dados.
pub fn best_available_locale(locale: &str) -> Option<String> {
    best_available_by(locale, |candidate| {
        if candidate.contains('-') {
            crate::runtime::intl_table_lookup::contains_sorted(&AVAILABLE_LOCALES, candidate)
        } else {
            language_has_data(candidate)
        }
    })
}

/// O `BestAvailableLocale` da tag canônica `tag`, sem as extensões.
fn best_available_for_tag(tag: &str) -> Option<String> {
    let parsed = parse_language_tag(tag)?;
    best_available_locale(&parsed.base_name())
}

/// `SupportedLocales(availableLocales, requestedLocales, options)`: os pedidos que têm dados, na ordem
/// em que vieram.
pub fn supported_locales(requested: &[String]) -> Vec<String> {
    requested.iter().filter(|tag| best_available_for_tag(tag).is_some()).cloned().collect()
}

/// O resultado de `ResolveLocale`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedLocale {
    /// O locale disponível escolhido (`en`, `en-US`, `pt`, `pt-BR`).
    pub locale: String,
    pub language: Language,
    /// Os valores das chaves `-u-` pedidas na tag que casou, na ordem de `relevant_keys`.
    pub keywords: Vec<(String, String)>,
}

impl ResolvedLocale {
    /// O locale com as chaves `-u-` que a classe honrou (`[("hc", "h23")]` dá `en-US-u-hc-h23`).
    pub fn tag_with(&self, honored: &[(&str, &str)]) -> String {
        if honored.is_empty() {
            return self.locale.clone();
        }
        let mut sorted: Vec<(&str, &str)> = honored.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let mut tag = format!("{}-u", self.locale);
        for (key, value) in sorted {
            tag.push('-');
            tag.push_str(key);
            if value != "true" {
                tag.push('-');
                tag.push_str(value);
            }
        }
        tag
    }

    /// O valor pedido para `key` na tag que casou.
    pub fn keyword(&self, key: &str) -> Option<&str> {
        self.keywords.iter().find(|(candidate, _)| candidate == key).map(|(_, value)| value.as_str())
    }
}

/// `ResolveLocale(availableLocales, requestedLocales, ...)` com o `lookup`: o primeiro pedido que tem
/// dados, senão o locale padrão.
pub fn resolve_locale(requested: &[String], relevant_keys: &[&str]) -> ResolvedLocale {
    for tag in requested {
        let Some(parsed) = parse_language_tag(tag) else { continue };
        if let Some(found) = best_available_locale(&parsed.base_name()) {
            let keywords = relevant_keys
                .iter()
                .filter_map(|key| parsed.unicode_keyword(key).map(|value| ((*key).to_string(), value)))
                .collect();
            let language = Language::of_locale(&found);
            return ResolvedLocale { locale: found, language, keywords };
        }
    }
    let locale = default_locale();
    let language = Language::of_locale(&locale);
    ResolvedLocale { locale, language, keywords: Vec::new() }
}

/// A região usada para os dados por região (`getWeekInfo`, `getCalendars`, `getTimeZones`): a da tag ou
/// a que o `maximize` acrescentaria.
pub fn region_or_likely(tag: &LanguageTag) -> Option<String> {
    tag.region.clone().or_else(|| maximize(tag).region)
}

/// A entrada de `LIKELY_SUBTAGS` para `key` (`pt`, `uz-Cyrl`, `zh-TW`, `und-Latn-RU`).
fn likely_entry(key: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    use crate::runtime::intl_likely_subtags_data::LIKELY_SUBTAGS;
    LIKELY_SUBTAGS.binary_search_by(|row| row.0.cmp(key)).ok().map(|index| &LIKELY_SUBTAGS[index])
}

/// `Intl.Locale.prototype.maximize` (`uloc_addLikelySubtags`, medido em `scripts/gen-likely-subtags.js`): `ZZ` e
/// `Zzzz` valem como ausentes; com língua, escrita e região a tag fica como veio, salvo os apelidos (a língua
/// `pmk` vira `crr`, a região `QU` vira `EU`); senão a primeira chave achada entre `L-S-R`, `L-S`, `L-R` e `L` dá
/// a tripla, que só preenche o que falta. Língua sem chave nenhuma (`xx`, `sh`, `no`) fica como veio, sem apelidos.
/// Língua, escrita e região são os da própria entrada (`tl` vira `fil`).
pub fn maximize(tag: &LanguageTag) -> LanguageTag {
    maximize_with(tag, false)
}

/// `maximize`, e o `maximize` interno do `minimize` com `preserve`: sem apelidos de língua nem de região, e a
/// língua da tag não muda (só `und` toma a da entrada).
fn maximize_with(original: &LanguageTag, preserve: bool) -> LanguageTag {
    use crate::runtime::intl_likely_subtags_data::{LANGUAGE_ALIASES, REGION_ALIASES};
    let mut tag = original.clone();
    if tag.language.is_empty() {
        tag.language = "und".to_string();
    }
    if !preserve {
        if tag.language != "und" {
            if let Some((_, likely_language, _, _)) = likely_entry(&tag.language) {
                tag.language = (*likely_language).to_string();
            }
        }
        if let Some(region) = tag.region.as_deref() {
            if let Ok(index) = REGION_ALIASES.binary_search_by(|row| row.0.cmp(region)) {
                tag.region = Some(REGION_ALIASES[index].1.to_string());
            }
        }
    }
    let language = tag.language.as_str();
    let script = tag.script.as_deref().filter(|script| *script != "Zzzz");
    let region = tag.region.as_deref().filter(|region| *region != "ZZ");
    if language != "und" && script.is_some() && region.is_some() {
        if !preserve {
            if let Ok(index) = LANGUAGE_ALIASES.binary_search_by(|row| row.0.cmp(language)) {
                tag.language = LANGUAGE_ALIASES[index].1.to_string();
            }
        }
        return tag;
    }
    let mut keys = Vec::with_capacity(4);
    if let (Some(script), Some(region)) = (script, region) {
        keys.push(format!("{language}-{script}-{region}"));
    }
    if let Some(script) = script {
        keys.push(format!("{language}-{script}"));
    }
    if let Some(region) = region {
        keys.push(format!("{language}-{region}"));
    }
    keys.push(language.to_string());
    let Some((_, likely_language, likely_script, likely_region)) = keys.iter().find_map(|key| likely_entry(key)) else {
        // Sem chave nenhuma a tag fica como veio, sem o apelido da região (`fb-PZ`).
        let mut unchanged = original.clone();
        unchanged.language = tag.language;
        return unchanged;
    };
    let mut result = tag.clone();
    if !(preserve && language != "und") {
        result.language = (*likely_language).to_string();
    }
    result.script = Some(script.map_or(*likely_script, |script| script).to_string());
    result.region = Some(region.map_or(*likely_region, |region| region).to_string());
    result
}

/// O que `ResolveLocale` decide para uma chave `-u-` (`IntlObject.cpp`, `resolveLocale`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedKey {
    /// O valor resolvido (`None` é o `null` do `localeData`: a chave não foi pedida ou o padrão é nulo).
    pub value: Option<String>,
    /// A extensão da tag continua no locale resolvido (`supportedExtensionAddition`).
    pub keep_extension: bool,
}

/// `ResolveLocale` para uma chave: `extension` é o valor da chave na tag pedida (`Some("")` é a chave sem
/// valor), `option` o valor da opção, `data` o `localeData[key]` sem o primeiro elemento e `default` o
/// primeiro elemento (`None` quando é `null`). O valor da opção só desloca a extensão se está em `data` e
/// difere do valor da extensão.
pub fn resolve_key(extension: Option<&str>, option: Option<&str>, data: &[&str], default: Option<&str>) -> ResolvedKey {
    if extension.is_none() && option.is_none() {
        return ResolvedKey { value: None, keep_extension: false };
    }
    let mut value: Option<String> = default.map(str::to_string);
    let mut keep_extension = false;
    if let Some(extension) = extension {
        if extension.is_empty() || extension == "true" {
            // A chave sem valor vale `true` quando o `localeData` a aceita.
            if data.contains(&"true") {
                value = Some("true".to_string());
                keep_extension = true;
            }
        } else if data.contains(&extension) {
            value = Some(extension.to_string());
            keep_extension = true;
        }
    }
    if let Some(option) = option {
        if data.contains(&option) && value.as_deref() != Some(option) {
            value = Some(option.to_string());
            keep_extension = false;
        }
    }
    ResolvedKey { value, keep_extension }
}

/// `Intl.Locale.prototype.minimize`: tira a escrita e a região que o `maximize` repõe (`uloc_minimizeSubtags`),
/// na ordem língua, língua + região, língua + escrita, e a primeira cujo `maximize` dá a forma máxima da tag
/// vence. A língua do candidato é a da tag (`zir` fica `zir`), salvo `und`, que toma a da forma máxima; a região
/// e a escrita são as da tag, ou as da forma máxima quando a tag não as traz. Sem redução a tag fica como veio.
pub fn minimize(tag: &LanguageTag) -> LanguageTag {
    let maximized = maximize_with(tag, true);
    let kept_language = if tag.language.is_empty() || tag.language == "und" { maximized.language.clone() } else { tag.language.clone() };
    let candidate = |script: Option<String>, region: Option<String>| -> LanguageTag {
        let mut candidate = tag.clone();
        candidate.language = kept_language.clone();
        candidate.script = script;
        candidate.region = region;
        candidate
    };
    let attempts = [
        candidate(None, None),
        candidate(None, tag.region.clone().or_else(|| maximized.region.clone())),
        candidate(tag.script.clone().or_else(|| maximized.script.clone()), None),
    ];
    for attempt in attempts {
        if maximize_with(&attempt, true).base_name() == maximized.base_name() {
            return attempt;
        }
    }
    candidate(tag.script.clone(), tag.region.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_case_and_aliases() {
        assert_eq!(canonicalize_tag("EN-us").as_deref(), Some("en-US"));
        assert_eq!(canonicalize_tag("zh-hant-tw").as_deref(), Some("zh-Hant-TW"));
        assert_eq!(canonicalize_tag("iw").as_deref(), Some("he"));
        assert_eq!(canonicalize_tag("en-u-kn-true").as_deref(), Some("en-u-kn"));
        assert_eq!(canonicalize_tag("de-u-co-phonebk-ca-gregory").as_deref(), Some("de-u-ca-gregory-co-phonebk"));
    }

    #[test]
    fn canonicalizes_values_variants_and_grandfathered_tags_as_bun() {
        let cases = [
            ("en-u-ca-islamicc", "en-u-ca-islamic-civil"),
            ("en-u-ca-ethiopic-amete-alem", "en-u-ca-ethioaa"),
            ("en-u-ms-imperial", "en-u-ms-uksystem"),
            ("en-u-tz-aqams", "en-u-tz-aqmcm"),
            ("en-u-tz-cnckg-ca-roc", "en-u-ca-roc-tz-cnsha"),
            ("en-u-ks-primary-kn-yes", "en-u-kn-ks-level1"),
            ("en-US-posix", "en-US-u-va-posix"),
            ("en-US-posix-u-ca-gregory", "en-US-u-ca-gregory-va-posix"),
            ("en-posix-1996", "en-1996-posix"),
            ("art-lojban", "jbo"),
            ("zh-guoyu-u-ca-roc", "cmn-u-ca-roc"),
            ("zh-hakka", "hak"),
            ("zh-xiang", "hsn"),
            ("cel-gaulish", "cel-gaulish"),
            ("aam", "aas"),
        ];
        for (tag, expected) in cases {
            assert_eq!(canonicalize_tag(tag).as_deref(), Some(expected), "{tag}");
        }
        for tag in ["i-klingon", "no-bok", "no-nyn", "sgn-BE-FR", "en-GB-oed", "zh-min-nan", "zh-guoyu-CN"] {
            assert_eq!(canonicalize_tag(tag), None, "{tag}");
        }
    }

    #[test]
    fn rejects_malformed_tags() {
        for tag in ["", "e", "en_US", "en-", "-en", "en-US-US-x", "abcdefghi", "en-a", "en-u-a-u-b", "en-x"] {
            assert_eq!(canonicalize_tag(tag), None, "{tag}");
        }
    }

    #[test]
    fn best_available_truncates_subtags() {
        assert_eq!(best_available_locale("en-GB").as_deref(), Some("en-GB"));
        assert_eq!(best_available_locale("pt-BR").as_deref(), Some("pt-BR"));
        assert_eq!(best_available_locale("pt-PT").as_deref(), Some("pt-PT"));
        assert_eq!(best_available_locale("fr").as_deref(), Some("fr"));
        assert_eq!(best_available_locale("zh-Hant-TW").as_deref(), Some("zh-Hant-TW"));
        assert_eq!(best_available_locale("xx"), None);
        assert_eq!(best_available_locale("und"), None);
    }

    #[test]
    fn supports_the_cldr_languages() {
        let requested: Vec<String> =
            ["fr", "de", "ja", "ar", "hi", "en-GB", "pt-PT", "xx", "pt-BR-u-nu-latn"].iter().map(|tag| tag.to_string()).collect();
        assert_eq!(
            supported_locales(&requested),
            ["fr", "de", "ja", "ar", "hi", "en-GB", "pt-PT", "pt-BR-u-nu-latn"]
        );
    }

    #[test]
    fn resolves_the_requested_region() {
        for tag in ["en-GB", "pt-PT", "fr", "ja", "ar", "hi", "de"] {
            assert_eq!(resolve_locale(&[tag.to_string()], &[]).locale, tag);
        }
    }

    #[test]
    fn data_fallback_follows_the_cldr_parents() {
        assert_eq!(data_fallback_chain("en-GB"), ["en-GB", "en-001", "en"]);
        assert_eq!(data_fallback_chain("pt-AO"), ["pt-AO", "pt-PT", "pt"]);
        assert_eq!(data_fallback_chain("fr"), ["fr"]);
    }

    /// Medido no bun (`new Intl.PluralRules(tag).resolvedOptions().locale`, mesmo resultado de
    /// `DateTimeFormat`, `NumberFormat`, `ListFormat`...): a tag pedida volta intacta para toda língua do CLDR.
    #[test]
    fn resolves_the_requested_tag_for_cldr_languages_as_bun() {
        let tags = [
            "sv", "sv-SE", "ar", "ar-EG", "ru", "ru-RU", "pl", "pl-PL", "es", "es-MX", "es-419", "fr", "fr-CA", "de",
            "de-AT", "it", "ko-KR", "nl-BE", "tr", "uk", "hi-IN", "en-001", "en-AU", "zh-HK", "zh-Hant-HK", "sr-Cyrl",
            "sr-Latn", "fil", "cs", "cy",
        ];
        assert_eq!(tags.len(), 30);
        for tag in tags {
            assert_eq!(resolve_locale(&[tag.to_string()], &[]).locale, tag, "{tag}");
        }
    }

    #[test]
    fn resolves_aliases_extensions_and_unknown_languages_as_bun() {
        // `iw` vira `he` na canonicalização; a extensão `-u-` sai do locale base.
        assert_eq!(canonicalize_tag("iw").as_deref(), Some("he"));
        assert_eq!(resolve_locale(&["he".to_string()], &[]).locale, "he");
        assert_eq!(resolve_locale(&["en-US-u-nu-arab".to_string()], &["nu"]).locale, "en-US");
        assert_eq!(resolve_locale(&["pt-BR-u-ca-gregory".to_string()], &[]).locale, "pt-BR");
        assert_eq!(resolve_locale(&["ar-u-nu-latn".to_string()], &["nu"]).locale, "ar");
        for tag in ["xx", "tlh", "und"] {
            assert_eq!(resolve_locale(&[tag.to_string()], &[]).locale, "en-US", "{tag}");
        }
    }

    #[test]
    fn resolves_to_the_default_without_data() {
        let resolved = resolve_locale(&["xx".to_string(), "pt-BR".to_string()], &[]);
        assert_eq!(resolved.locale, "pt-BR");
        let resolved = resolve_locale(&["xx".to_string()], &[]);
        assert_eq!(resolved.locale, "en-US");
    }

    #[test]
    fn maximizes_and_minimizes() {
        let tag = parse_language_tag("en").unwrap();
        assert_eq!(maximize(&tag).canonical(), "en-Latn-US");
        let tag = parse_language_tag("zh-TW").unwrap();
        assert_eq!(maximize(&tag).canonical(), "zh-Hant-TW");
        let tag = parse_language_tag("en-Latn-US").unwrap();
        assert_eq!(minimize(&tag).canonical(), "en");
        let tag = parse_language_tag("pt-Latn-BR").unwrap();
        assert_eq!(minimize(&tag).canonical(), "pt");
    }

    #[test]
    fn maximizes_pseudo_subtags_and_unknown_languages_as_bun() {
        let canonical = |tag: &str| maximize(&parse_language_tag(tag).unwrap()).canonical();
        assert_eq!(canonical("en-ZZ"), "en-Latn-US");
        assert_eq!(canonical("en-Zzzz"), "en-Latn-US");
        assert_eq!(canonical("und-Latn-RU"), "krl-Latn-RU");
        assert_eq!(canonical("und-AQ"), "en-Latn-AQ");
        assert_eq!(canonical("uz-AF"), "uz-Arab-AF");
        assert_eq!(canonical("tl"), "fil-Latn-PH");
        assert_eq!(canonical("xx-ZZ"), "xx-ZZ");
        assert_eq!(canonical("xx-Cyrl"), "xx-Cyrl");
        assert_eq!(canonical("sh"), "sh");
    }

    #[test]
    fn best_available_drops_regions_without_data() {
        assert_eq!(best_available_locale("en-ZZ").as_deref(), Some("en"));
        assert_eq!(best_available_locale("zh-Hant-ZZ").as_deref(), Some("zh-Hant"));
        assert_eq!(best_available_locale("pt-ZZ").as_deref(), Some("pt"));
        assert_eq!(best_available_locale("sr-Latn-ZZ").as_deref(), Some("sr-Latn"));
    }

    #[test]
    fn maximizes_by_script_and_region() {
        let canonical = |tag: &str| maximize(&parse_language_tag(tag).unwrap()).canonical();
        assert_eq!(canonical("zh-Hant"), "zh-Hant-TW");
        assert_eq!(canonical("zh-HK"), "zh-Hant-HK");
        assert_eq!(canonical("sr-Latn"), "sr-Latn-RS");
        assert_eq!(canonical("sr-ME"), "sr-Latn-ME");
        assert_eq!(canonical("pa-Arab"), "pa-Arab-PK");
        assert_eq!(canonical("und-DE"), "de-Latn-DE");
        assert_eq!(canonical("und-Cyrl"), "ru-Cyrl-RU");
        assert_eq!(canonical("und"), "en-Latn-US");
        assert_eq!(canonical("az"), "az-Latn-AZ");
        assert_eq!(canonical("en-u-ca-gregory"), "en-Latn-US-u-ca-gregory");
    }

    #[test]
    fn minimizes_from_the_maximal_form() {
        let canonical = |tag: &str| minimize(&parse_language_tag(tag).unwrap()).canonical();
        assert_eq!(canonical("zh-Hant-TW"), "zh-TW");
        assert_eq!(canonical("und-Latn-US"), "en");
        assert_eq!(canonical("sr-Latn-RS"), "sr-Latn");
    }

    #[test]
    fn resolve_key_follows_resolve_locale() {
        let boolean = ["false", "true"];
        // Sem extensão nem opção: nada pedido.
        assert_eq!(resolve_key(None, None, &boolean, Some("false")), ResolvedKey { value: None, keep_extension: false });
        // A chave sem valor vale `true` e a extensão fica.
        assert_eq!(
            resolve_key(Some(""), None, &boolean, Some("false")),
            ResolvedKey { value: Some("true".to_string()), keep_extension: true }
        );
        // A opção igual à extensão não a desloca.
        assert_eq!(
            resolve_key(Some(""), Some("true"), &boolean, Some("false")),
            ResolvedKey { value: Some("true".to_string()), keep_extension: true }
        );
        // A opção diferente a desloca.
        assert_eq!(
            resolve_key(Some("true"), Some("false"), &boolean, Some("false")),
            ResolvedKey { value: Some("false".to_string()), keep_extension: false }
        );
        // Valor fora do `localeData` é ignorado, na extensão e na opção.
        let collations = ["emoji", "eor"];
        assert_eq!(resolve_key(Some("phonebk"), None, &collations, None), ResolvedKey { value: None, keep_extension: false });
        assert_eq!(resolve_key(None, Some("phonebk"), &collations, None), ResolvedKey { value: None, keep_extension: false });
        assert_eq!(
            resolve_key(Some("emoji"), None, &collations, None),
            ResolvedKey { value: Some("emoji".to_string()), keep_extension: true }
        );
    }

    /// O resultado do script, lido de `R`, no realm vivo.
    fn read(global_object: &crate::runtime::js_global_object::JSGlobalObjectRef, source: &str) -> String {
        use crate::api::eval::{evaluate, program_source, read_global_result};
        let _ = evaluate(global_object, &program_source(source));
        let value = read_global_result(global_object, "R").unwrap_or_else(|_| panic!("leitura de R"));
        String::from_utf8_lossy(&value.to_wtf_string().utf8(crate::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned()
    }

    /// Medido no bun 1.4.2 com o locale pedido explícito (o padrão posto pelo hospedeiro tem de dar o mesmo
    /// que pedir o locale em cada chamada, no realm que já estava vivo).
    #[test]
    fn default_locale_changed_with_the_realm_alive_equals_asking_for_it() {
        const SCRIPT: &str = "var d = new Date(Date.UTC(2026, 0, 15, 15, 4, 5)); var R = [ \
            new Intl.DateTimeFormat(undefined, { timeZone: 'UTC' }).resolvedOptions().locale, new Intl.NumberFormat().resolvedOptions().locale, \
            new Intl.Collator().resolvedOptions().locale, new Intl.PluralRules().resolvedOptions().locale, \
            (1234567.891).toLocaleString(), (1234567n).toLocaleString(), d.toLocaleString(undefined, { timeZone: 'UTC' }), \
            d.toLocaleDateString(undefined, { timeZone: 'UTC' }), ['b', 'a', 'ä', 'z'].sort((a, b) => a.localeCompare(b)).join(), \
            'I'.toLocaleLowerCase(), new Intl.NumberFormat(undefined, { style: 'currency', currency: 'BRL' }).format(12.5)].join('|')";
        crate::runtime::cell_registry::run_program(|| {
            let (vm, global_object) = crate::api::eval::new_global_object();
            let _finalizer = crate::api::eval::VmFinalizer::new(&vm);
            assert_eq!(
                read(&global_object, SCRIPT),
                "en-US|en-US|en-US|en-US|1,234,567.891|1,234,567|1/15/2026, 3:04:05 PM|1/15/2026|a,ä,b,z|i|R$12.50"
            );
            assert!(set_default_locale(Some("pt-BR")));
            assert_eq!(
                read(&global_object, SCRIPT),
                "pt-BR|pt-BR|pt|pt-BR|1.234.567,891|1.234.567|15/01/2026, 15:04:05|15/01/2026|a,ä,b,z|i|R$\u{a0}12,50"
            );
            assert!(set_default_locale(Some("tr")));
            assert_eq!(
                read(&global_object, SCRIPT),
                "tr|tr|tr|tr|1.234.567,891|1.234.567|15.01.2026 15:04:05|15.01.2026|a,ä,b,z|ı|R$12,50"
            );
            assert!(!set_default_locale(Some("not a tag")));
            assert_eq!(default_locale(), "tr");
            assert!(set_default_locale(None));
            assert_eq!(default_locale(), "en-US");
        });
    }
}
