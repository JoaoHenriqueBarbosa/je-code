//! Porte de `WordSegmenterLocales` (`editorOptions.ts`, linhas 5539 a 5598): as locales (etiquetas BCP 47)
//! usadas para segmentar linhas em palavras na navegação e nas operações por palavra.
//!
//! Divergência e pendência: `Intl.Segmenter.supportedLocalesOf(locale)` do original lança `RangeError` para
//! uma etiqueta inválida (o `catch` ignora) e devolve vazio para uma locale sem dados no ICU. Aqui a
//! pergunta "esta locale é suportada?" é um `fn(&str) -> bool` guardado na opção; o padrão
//! ([`is_structurally_valid_language_tag`]) cobre o lado do `RangeError` (a gramática de
//! `IsStructurallyValidLanguageTag` do ECMA-402) e aceita toda etiqueta bem formada, porque o `Segmenter` do ICU
//! cai na regra raiz para locales sem dados próprios. Quando o `icu_segmenter` entrar no `je-base`
//! (ver `docs/reuse-pseudo-linus.md`, linha de Intl), a lista de locales disponíveis troca esse predicado
//! sem mudar a opção.

use je_base::dynamic::Dynamic;

use crate::config::editor_options::{EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `wordSegmenterLocales`.
pub const WORD_SEGMENTER_LOCALES: Opt<Vec<String>> = Opt::new(EditorOption::WordSegmenterLocales);

/// `IsStructurallyValidLanguageTag` (ECMA-402, `unicode_locale_id`): `language [script] [region]
/// [variants] [extensions] [privateuse]`, com subtags separadas por `-`.
pub fn is_structurally_valid_language_tag(tag: &str) -> bool {
    let parts: Vec<&str> = tag.split('-').collect();
    if parts.iter().any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric())) {
        return false;
    }
    let all_alpha = |part: &str| part.bytes().all(|b| b.is_ascii_alphabetic());
    let all_digit = |part: &str| part.bytes().all(|b| b.is_ascii_digit());

    // language: alpha{2,3} | alpha{5,8}
    let language = parts[0];
    if !all_alpha(language) || !matches!(language.len(), 2..=3 | 5..=8) {
        return false;
    }
    let mut i = 1;
    // script: alpha{4}
    if i < parts.len() && parts[i].len() == 4 && all_alpha(parts[i]) {
        i += 1;
    }
    // region: alpha{2} | digit{3}
    if i < parts.len() && ((parts[i].len() == 2 && all_alpha(parts[i])) || (parts[i].len() == 3 && all_digit(parts[i]))) {
        i += 1;
    }
    // variants: alphanum{5,8} | digit alphanum{3}, sem repetir
    let mut variants: Vec<String> = Vec::new();
    while i < parts.len() {
        let part = parts[i];
        let is_variant = matches!(part.len(), 5..=8) || (part.len() == 4 && part.as_bytes()[0].is_ascii_digit());
        if !is_variant {
            break;
        }
        let lowered = part.to_ascii_lowercase();
        if variants.contains(&lowered) {
            return false;
        }
        variants.push(lowered);
        i += 1;
    }
    // extensions: singleton (`x` fora) seguido de alphanum{2,8}+, sem repetir o singleton; e privateuse
    let mut singletons: Vec<u8> = Vec::new();
    while i < parts.len() && parts[i].len() == 1 {
        let singleton = parts[i].as_bytes()[0].to_ascii_lowercase();
        i += 1;
        if singleton == b'x' {
            // privateuse: `x` seguido de alphanum{1,8}+ até o fim
            return i < parts.len() && parts[i..].iter().all(|part| part.len() <= 8);
        }
        if singletons.contains(&singleton) {
            return false;
        }
        singletons.push(singleton);
        let start = i;
        while i < parts.len() && matches!(parts[i].len(), 2..=8) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == parts.len()
}

/// `WordSegmenterLocales`.
pub struct WordSegmenterLocales {
    is_supported: fn(&str) -> bool,
    schema: Dynamic,
}

impl WordSegmenterLocales {
    /// Com o predicado padrão ([`is_structurally_valid_language_tag`]).
    pub fn new() -> Self {
        Self::with_support(is_structurally_valid_language_tag)
    }

    /// `is_supported(locale)` responde `Intl.Segmenter.supportedLocalesOf(locale).length > 0`, e é falso
    /// onde o original lançaria.
    pub fn with_support(is_supported: fn(&str) -> bool) -> Self {
        let string_type = || Dynamic::object([("type", Dynamic::string("string"))]);
        let schema = Dynamic::object([
            (
                "anyOf",
                Dynamic::array([
                    string_type(),
                    Dynamic::object([("type", Dynamic::string("array")), ("items", string_type())]),
                ]),
            ),
            (
                "description",
                Dynamic::string("Locales to be used for word segmentation when doing word related navigations or operations. Specify the BCP 47 language tag of the word you wish to recognize (e.g., ja, zh-CN, zh-Hant-TW, etc.)."),
            ),
            ("type", Dynamic::string("array")),
            ("items", string_type()),
            ("default", Dynamic::array([])),
        ]);
        Self { is_supported, schema }
    }
}

impl Default for WordSegmenterLocales {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for WordSegmenterLocales {
    type Value = Vec<String>;

    fn id(&self) -> EditorOption {
        EditorOption::WordSegmenterLocales
    }

    fn name(&self) -> &str {
        "wordSegmenterLocales"
    }

    fn default_value(&self) -> Vec<String> {
        Vec::new()
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> Vec<String> {
        // `typeof input === 'string'` vira `[input]`; depois só um array é aceito.
        let items = match input {
            Dynamic::String(_) => vec![input.clone()],
            Dynamic::Array(array) => array.to_vec(),
            _ => return self.default_value(),
        };
        let mut valid_locales = Vec::new();
        for locale in items {
            if let Dynamic::String(locale) = locale {
                let locale = locale.to_string_lossy();
                if (self.is_supported)(&locale) {
                    valid_locales.push(locale);
                }
            }
        }
        valid_locales
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn a_string_becomes_a_single_item_array() {
        let option = WordSegmenterLocales::new();
        assert_eq!(option.validate(&text("ja")), strings(&["ja"]));
        assert_eq!(option.validate(&text("zh-Hant-TW")), strings(&["zh-Hant-TW"]));
        // Etiqueta inválida lança `RangeError` no original e é ignorada.
        assert_eq!(option.validate(&text("")), Vec::<String>::new());
        assert_eq!(option.validate(&text("not a locale")), Vec::<String>::new());
    }

    #[test]
    fn arrays_keep_the_valid_strings_in_order() {
        let option = WordSegmenterLocales::new();
        let input = Dynamic::array([text("ja"), Dynamic::Number(1.0), text("zh-CN"), text("x_y"), Dynamic::Null, text("en-US")]);
        assert_eq!(option.validate(&input), strings(&["ja", "zh-CN", "en-US"]));
        assert_eq!(option.validate(&Dynamic::array([])), Vec::<String>::new());
    }

    #[test]
    fn other_inputs_give_the_default() {
        let option = WordSegmenterLocales::new();
        assert_eq!(option.default_value(), Vec::<String>::new());
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(1.0), Dynamic::object([("ja", text("ja"))])] {
            assert_eq!(option.validate(&input), Vec::<String>::new());
        }
    }

    #[test]
    fn the_support_predicate_is_injectable() {
        let option = WordSegmenterLocales::with_support(|locale| locale.starts_with("ja"));
        assert_eq!(option.validate(&Dynamic::array([text("ja"), text("en"), text("ja-JP")])), strings(&["ja", "ja-JP"]));
    }

    #[test]
    fn language_tag_grammar() {
        for valid in ["en", "pt-BR", "zh-Hant-TW", "es-419", "de-DE-1996", "sl-rozaj-biske", "en-u-ca-gregory", "en-US-x-twain", "en-a-bbb", "eng", "abcde"] {
            assert!(is_structurally_valid_language_tag(valid), "{valid}");
        }
        for invalid in ["", "e", "en_US", "en--US", "-en", "en-", "abcd", "en-u", "en-a-bb-a-cc", "en-1996-1996", "en-x", "en-US-x", "pt-BR-é", "en US", "en-abcdefghi"] {
            assert!(!is_structurally_valid_language_tag(invalid), "{invalid}");
        }
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = WordSegmenterLocales::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["anyOf", "description", "type", "items", "default"]);
        let any_of = schema.get_str("anyOf");
        assert_eq!(any_of.get_str("length"), Dynamic::Number(2.0));
        assert_eq!(any_of.get_str("0").own_keys(), vec!["type"]);
        assert_eq!(any_of.get_str("1").own_keys(), vec!["type", "items"]);
        assert_eq!(schema.get_str("type"), text("array"));
        assert_eq!(schema.get_str("items").get_str("type"), text("string"));
        assert_eq!(schema.get_str("default").get_str("length"), Dynamic::Number(0.0));
    }
}
