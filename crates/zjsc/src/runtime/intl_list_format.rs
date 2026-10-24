//! `Intl.ListFormat` sem ICU (`IntlListFormat.cpp`, `IntlListFormatPrototype.cpp`,
//! `IntlListFormatConstructor.cpp`): as listas `conjunction`, `disjunction` e `unit` nos estilos `long`,
//! `short` e `narrow`.
//!
//! Os padrões vêm da tabela gerada em `intl_list_format_data` (medida no ICU do bun por
//! `scripts/gen-list-format-patterns.js`, porque os dados do CLDR do icu4x divergem dos do ICU em vários locales),
//! para qualquer locale. O `locale` de `resolvedOptions` ainda sai do `intl_locale_data` (só `en-US` e `pt-BR`):
//! ver `wip-notes/intl-gaps.md`.

use icu_locale_core::Locale;
use crate::runtime::lookup::{native_entry};

use crate::host_function;
use crate::runtime::intl_list_format_data::{Predicate, CONDITIONALS, HEBREW_RANGES, PATTERNS, TAGS};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_support::{
    array_of, canonicalize_locale_list, construct_instance, get_options_object, new_object, option_enum, put, read_locale_matcher,
    resolve_locale_from, str_value, units_value, with_instance, IntlClass, IntlEnum,
};
use crate::runtime::iterator_operations::for_each_in_iterable;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;

crate::intl_enum!(ListType { Conjunction => "conjunction", Disjunction => "disjunction", Unit => "unit" });
crate::intl_enum!(ListStyle { Long => "long", Short => "short", Narrow => "narrow" });

/// O estado de um `IntlListFormat`.
struct ListFormatState {
    locale: String,
    /// A primeira tag pedida (ou a resolvida, sem pedido): o locale dos padrões da lista.
    format_locale: String,
    kind: ListType,
    style: ListStyle,
}

/// As tags a tentar, da mais específica à língua: `l-s-r`, `l-s`, `l` (com escrita) ou `l-r`, `l` (sem). Tag inválida
/// vira só `en`. Variantes e extensões não entram. É a mesma resolução por truncamento do gerador.
fn candidate_tags(tag: &str) -> Vec<String> {
    let Ok(locale) = Locale::try_from_str(tag) else {
        return vec!["en".to_string()];
    };
    let language = locale.id.language.as_str();
    let script = locale.id.script.map(|script| script.to_string());
    let region = locale.id.region.map(|region| region.to_string());
    let mut tags = Vec::new();
    if let (Some(script), Some(region)) = (&script, &region) {
        tags.push(format!("{language}-{script}-{region}"));
    }
    match (&script, &region) {
        (Some(script), _) => tags.push(format!("{language}-{script}")),
        (None, Some(region)) => tags.push(format!("{language}-{region}")),
        (None, None) => {}
    }
    tags.push(language.to_string());
    tags
}

/// O primeiro candidato presente na tabela (ordenada por tag).
fn lookup<T: Copy>(table: &[(&str, T)], tags: &[String]) -> Option<T> {
    tags.iter().find_map(|tag| table.binary_search_by(|(key, _)| (*key).cmp(tag.as_str())).ok().map(|index| table[index].1))
}

/// O predicado, sobre o próximo elemento, das trocas condicionais de literal (es `y` por `e` e `o` por `u`, he `ו`
/// por `ו-`), medido contra o ICU do bun pelo `scripts/gen-list-format-patterns.js`.
fn predicate_holds(predicate: Predicate, chars: &[char]) -> bool {
    let at = |index: usize, set: &str| chars.get(index).is_some_and(|c| set.contains(*c));
    match predicate {
        Predicate::SpanishE => at(0, "iI") || (at(0, "hH") && at(1, "iI") && !at(2, "aAeE")),
        Predicate::SpanishU => {
            at(0, "oO8") || (at(0, "hH") && at(1, "oO")) || (chars.starts_with(&['1', '1']) && (chars.len() == 2 || chars[2] == ' '))
        }
        Predicate::NotHebrew => {
            chars.first().is_some_and(|c| !HEBREW_RANGES.iter().any(|&(low, high)| (low..=high).contains(&(*c as u32))))
        }
    }
}

/// O texto dos elementos da lista: `String` (os chamadores em Rust) ou UTF-16 (os elementos de JS, que podem ter
/// surrogate solto, preservado como no ICU).
pub trait ListText: Sized {
    /// Os três primeiros caracteres (o que os predicados dos literais condicionais olham).
    fn leading_chars(&self) -> Vec<char>;
    /// O literal do padrão como texto do mesmo tipo.
    fn from_literal(text: &str) -> Self;
}

impl ListText for String {
    fn leading_chars(&self) -> Vec<char> {
        self.chars().take(3).collect()
    }
    fn from_literal(text: &str) -> Self {
        text.to_string()
    }
}

impl ListText for Vec<u16> {
    fn leading_chars(&self) -> Vec<char> {
        char::decode_utf16(self.iter().copied()).take(3).map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
    }
    fn from_literal(text: &str) -> Self {
        text.encode_utf16().collect()
    }
}

/// Uma peça da lista: o literal do padrão ou um elemento.
enum Piece<'a, T> {
    Literal(&'a str),
    Element(&'a T),
}

/// As partes `element` e `literal` da lista (`ulistfmt_formatStringsToResult`), pela tabela medida no ICU do bun
/// (`intl_list_format_data`). `locale` é a tag pedida (o ICU formata pelo locale pedido, não pelo resolvido); língua
/// sem dados usa `en`. Também serve ao `Intl.DurationFormat` (a lista `unit`).
pub fn list_parts<T: ListText + Clone>(locale: &str, kind: ListType, style: ListStyle, items: &[T]) -> Vec<(String, T)> {
    let tags = candidate_tags(locale);
    let set = lookup(&TAGS, &tags).or_else(|| lookup(&TAGS, &["en".to_string()])).map_or(0, usize::from);
    let rules = lookup(&CONDITIONALS, &tags).unwrap_or(&[]);
    let patterns = &PATTERNS[set];
    let base = (kind as usize * 3 + style as usize) * 8;
    let literal = |slot: usize, next: &T| -> &str {
        let text = patterns[base + slot];
        let chars = next.leading_chars();
        rules.iter().find(|(from, _, predicate)| *from == text && predicate_holds(*predicate, &chars)).map_or(text, |(_, to, _)| to)
    };
    let mut parts: Vec<(String, T)> = Vec::new();
    // Medido no bun: o elemento vazio não sai e os literais que ficam vizinhos viram um só (`["a", "", "c"]` dá
    // `a`, `, , and `, `c`); o ICU também não emite literal vazio.
    let mut pending = String::new();
    let mut push = |piece: Piece<'_, T>| match piece {
        Piece::Literal(text) => pending.push_str(text),
        Piece::Element(text) if text.leading_chars().is_empty() => {}
        Piece::Element(text) => {
            if !pending.is_empty() {
                parts.push(("literal".to_string(), T::from_literal(&std::mem::take(&mut pending))));
            }
            parts.push(("element".to_string(), text.clone()));
        }
    };
    match items {
        [] => {}
        [only] => push(Piece::Element(only)),
        [first, second] => {
            push(Piece::Literal(patterns[base]));
            push(Piece::Element(first));
            push(Piece::Literal(literal(1, second)));
            push(Piece::Element(second));
            push(Piece::Literal(patterns[base + 2]));
        }
        [first, middle @ .., last] => {
            push(Piece::Literal(patterns[base + 3]));
            push(Piece::Element(first));
            for (index, item) in middle.iter().enumerate() {
                push(Piece::Literal(patterns[base + if index == 0 { 4 } else { 5 }]));
                push(Piece::Element(item));
            }
            push(Piece::Literal(literal(6, last)));
            push(Piece::Element(last));
            push(Piece::Literal(patterns[base + 7]));
        }
    }
    if !pending.is_empty() {
        parts.push(("literal".to_string(), T::from_literal(&pending)));
    }
    parts
}

/// `stringListFromIterable`: os elementos, que precisam ser strings.
fn string_list(global_object: &JSGlobalObject, iterable: JSValue) -> Result<Vec<Vec<u16>>, Thrown> {
    let mut items: Vec<Vec<u16>> = Vec::new();
    if iterable.is_undefined() {
        return Ok(items);
    }
    for_each_in_iterable(global_object, iterable, |value| {
        if !value.is_string() {
            return Err(Thrown::type_error("Iterable passed to ListFormat includes non String"));
        }
        let text = crate::runtime::intl_support::to_wtf_checked(global_object, value)?;
        items.push(text.characters_without_null_termination().expect("unidades UTF-16 do elemento"));
        Ok(())
    })?;
    Ok(items)
}

/// `IntlListFormat::initializeListFormat`.
fn initialize(global_object: &JSGlobalObject, locales: JSValue, options_value: JSValue) -> Result<ListFormatState, Thrown> {
    let requested = canonicalize_locale_list(global_object, locales)?;
    let resolved = resolve_locale_from(global_object, locales, &[])?;
    let format_locale = requested.into_iter().next().unwrap_or_else(|| resolved.locale.clone());
    // `intlGetOptionsObject`: um valor primitivo é `TypeError`, não é coagido a objeto.
    let options = get_options_object(options_value)?;
    read_locale_matcher(global_object, options)?;
    let kind = option_enum::<ListType>(
        global_object,
        options,
        "type",
        "type must be either \"conjunction\", \"disjunction\", or \"unit\"",
    )?
    .unwrap_or(ListType::Conjunction);
    let style = option_enum::<ListStyle>(
        global_object,
        options,
        "style",
        "style must be either \"long\", \"short\", or \"narrow\"",
    )?
    .unwrap_or(ListStyle::Long);
    Ok(ListFormatState { locale: resolved.locale, format_locale, kind, style })
}

fn construct_list_format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

fn call_list_format_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    crate::runtime::collection_support::constructor_cannot_be_called_as_function("ListFormat")
}

fn format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<ListFormatState, _>(
        call.this_value(),
        "Intl.ListFormat.prototype.format called on value that's not a ListFormat",
        |state, _| {
            let items = string_list(global_object, call.argument(0))?;
            let units: Vec<u16> =
                list_parts(&state.format_locale, state.kind, state.style, &items).into_iter().flat_map(|(_, text)| text).collect();
            Ok(units_value(global_object.vm(), &units))
        },
    )
}

fn format_to_parts_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<ListFormatState, _>(
        call.this_value(),
        "Intl.ListFormat.prototype.formatToParts called on value that's not a ListFormat",
        |state, _| {
            let items = string_list(global_object, call.argument(0))?;
            let vm = global_object.vm();
            let values: Vec<JSValue> = list_parts(&state.format_locale, state.kind, state.style, &items)
                .into_iter()
                .map(|(kind, text)| {
                    let object = new_object(global_object);
                    put(global_object, &object, "type", str_value(vm, &kind));
                    put(global_object, &object, "value", units_value(vm, &text));
                    object.as_value()
                })
                .collect();
            Ok(array_of(global_object, &values))
        },
    )
}

fn resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<ListFormatState, _>(
        call.this_value(),
        "Intl.ListFormat.prototype.resolvedOptions called on value that's not a ListFormat",
        |state, _| {
            let vm = global_object.vm();
            let options = new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(global_object, &options, "type", str_value(vm, state.kind.as_str()));
            put(global_object, &options, "style", str_value(vm, state.style.as_str()));
            Ok(options.as_value())
        },
    )
}

host_function!(call_list_format, call_list_format_body);
host_function!(construct_list_format, construct_list_format_body);
host_function!(list_format_proto_format, format_body);
host_function!(list_format_proto_format_to_parts, format_to_parts_body);
host_function!(list_format_proto_resolved_options, resolved_options_body);

crate::intl_prototype_s_info!(
    LIST_FORMAT_PROTOTYPE_S_INFO,
    "Intl.ListFormat",
    [
        native_entry("format", list_format_proto_format, 1),
        native_entry("formatToParts", list_format_proto_format_to_parts, 1),
        native_entry("resolvedOptions", list_format_proto_resolved_options, 0),
    ]
);

/// `IntlListFormatConstructor` e `IntlListFormatPrototype`.
pub fn install_list_format(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "ListFormat",
        length: 0,
        has_supported_locales_of: true,
        call: call_list_format,
        construct: construct_list_format,
    };
    class.install_with_table(global_object, intl, &LIST_FORMAT_PROTOTYPE_S_INFO);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(tag: &str, kind: ListType, style: ListStyle, items: &[&str]) -> String {
        let items: Vec<String> = items.iter().map(|item| (*item).to_string()).collect();
        list_parts(tag, kind, style, &items).into_iter().map(|(_, text)| text).collect()
    }

    #[test]
    fn english_lists() {
        let english = |kind, style, items: &[&str]| format("en", kind, style, items);
        assert_eq!(english(ListType::Conjunction, ListStyle::Long, &["a", "b", "c"]), "a, b, and c");
        assert_eq!(english(ListType::Conjunction, ListStyle::Long, &["a", "b"]), "a and b");
        assert_eq!(english(ListType::Conjunction, ListStyle::Long, &["a"]), "a");
        assert_eq!(english(ListType::Disjunction, ListStyle::Long, &["a", "b", "c"]), "a, b, or c");
        assert_eq!(english(ListType::Unit, ListStyle::Narrow, &["a", "b", "c"]), "a b c");
        assert_eq!(english(ListType::Conjunction, ListStyle::Short, &["a", "b", "c"]), "a, b, & c");
        assert_eq!(english(ListType::Conjunction, ListStyle::Narrow, &["a", "b", "c"]), "a, b, c");
        assert_eq!(english(ListType::Unit, ListStyle::Long, &["a", "b"]), "a, b");
        assert_eq!(english(ListType::Disjunction, ListStyle::Long, &[]), "");
    }

    #[test]
    fn parts_alternate_elements_and_literals() {
        let items: Vec<String> = ["x", "y", "z"].iter().map(|item| (*item).to_string()).collect();
        let kinds: Vec<String> =
            list_parts("en", ListType::Conjunction, ListStyle::Long, &items).into_iter().map(|(kind, _)| kind).collect();
        assert_eq!(kinds, ["element", "literal", "element", "literal", "element"]);
    }

    #[test]
    fn portuguese_lists() {
        assert_eq!(format("pt", ListType::Conjunction, ListStyle::Long, &["a", "b", "c"]), "a, b e c");
        assert_eq!(format("pt", ListType::Disjunction, ListStyle::Long, &["a", "b"]), "a ou b");
        // Medido no bun: o estreito do `pt` usa só vírgulas, sem o `e`.
        assert_eq!(format("pt", ListType::Conjunction, ListStyle::Narrow, &["a", "b"]), "a, b");
        assert_eq!(format("pt", ListType::Conjunction, ListStyle::Narrow, &["a", "b", "c"]), "a, b, c");
        assert_eq!(format("pt", ListType::Unit, ListStyle::Short, &["a", "b", "c"]), "a, b e c");
        assert_eq!(format("pt", ListType::Unit, ListStyle::Narrow, &["a", "b", "c"]), "a b c");
    }

    /// Medido no bun 1.4.2 com `new Intl.ListFormat(tag, { type, style }).format(["a", "b", "c"])`.
    #[test]
    fn other_locales_match_bun() {
        use ListStyle::{Long, Narrow, Short};
        use ListType::{Conjunction, Disjunction, Unit};
        let cases: [(&str, ListType, ListStyle, &str); 30] = [
            ("es", Conjunction, Long, "a, b y c"),
            ("es", Disjunction, Short, "a, b o c"),
            ("es", Unit, Short, "a, b, c"),
            ("es", Unit, Narrow, "a b c"),
            ("es", Conjunction, Narrow, "a, b y c"),
            ("fr", Conjunction, Long, "a, b et c"),
            ("fr", Conjunction, Narrow, "a, b, c"),
            ("fr", Disjunction, Long, "a, b ou c"),
            ("fr", Unit, Short, "a, b et c"),
            ("fr", Unit, Narrow, "a b c"),
            ("de", Conjunction, Short, "a, b und c"),
            ("de", Disjunction, Narrow, "a, b oder c"),
            ("de", Unit, Long, "a, b und c"),
            ("de", Unit, Narrow, "a, b und c"),
            ("de", Conjunction, Long, "a, b und c"),
            ("ar", Conjunction, Long, "a وb وc"),
            ("ar", Disjunction, Long, "a أو b أو c"),
            ("ar", Unit, Long, "a، وb، وc"),
            ("ar", Unit, Narrow, "a وb وc"),
            ("ar", Disjunction, Narrow, "a أو b أو c"),
            ("ru", Conjunction, Long, "a, b и c"),
            ("ru", Conjunction, Narrow, "a, b, c"),
            ("ru", Disjunction, Short, "a, b или c"),
            ("ru", Unit, Long, "a b c"),
            ("ru", Unit, Narrow, "a b c"),
            ("pl", Conjunction, Short, "a, b i c"),
            ("pl", Disjunction, Long, "a, b lub c"),
            ("pl", Unit, Long, "a, b i c"),
            ("pl", Unit, Narrow, "a, b i c"),
            ("pl", Conjunction, Narrow, "a, b i c"),
        ];
        for (tag, kind, style, expected) in cases {
            assert_eq!(format(tag, kind, style, &["a", "b", "c"]), expected, "{tag} {kind:?} {style:?}");
        }
    }

    #[test]
    fn two_items_in_other_locales_match_bun() {
        assert_eq!(format("es", ListType::Conjunction, ListStyle::Long, &["a", "b"]), "a y b");
        assert_eq!(format("fr", ListType::Disjunction, ListStyle::Short, &["a", "b"]), "a ou b");
        assert_eq!(format("de", ListType::Unit, ListStyle::Long, &["a", "b"]), "a, b");
        assert_eq!(format("ar", ListType::Conjunction, ListStyle::Long, &["a", "b"]), "a وb");
        assert_eq!(format("ru", ListType::Conjunction, ListStyle::Narrow, &["a", "b"]), "a, b");
        assert_eq!(format("pl", ListType::Disjunction, ListStyle::Narrow, &["a", "b"]), "a lub b");
    }

    /// Medido no bun: `es` troca `y` por `e` antes de `i`/`hi` e `o` por `u` antes de `o`/`ho`/`8`/`11`; `he` põe
    /// hífen depois do `ו` quando o próximo elemento não começa em letra hebraica.
    #[test]
    fn conditional_literals_match_bun() {
        let long = |tag, kind, items: &[&str]| format(tag, kind, ListStyle::Long, items);
        assert_eq!(long("es", ListType::Conjunction, &["z", "hi"]), "z e hi");
        assert_eq!(long("es", ListType::Conjunction, &["z", "hie"]), "z y hie");
        assert_eq!(long("es", ListType::Disjunction, &["z", "11"]), "z u 11");
        assert_eq!(long("es", ListType::Disjunction, &["z", "110"]), "z o 110");
        assert_eq!(long("he", ListType::Conjunction, &["z", "x"]), "z ו-x");
        assert_eq!(long("he", ListType::Conjunction, &["z", "אב"]), "z ואב");
        assert_eq!(long("he", ListType::Disjunction, &["z", "x"]), "z או x");
        // Língua sem dados cai em `en`; região sem dados próprios cai na língua.
        assert_eq!(long("xx", ListType::Conjunction, &["a", "b", "c"]), "a, b, and c");
        assert_eq!(long("en-GB", ListType::Conjunction, &["a", "b", "c"]), "a, b and c");
    }
}
