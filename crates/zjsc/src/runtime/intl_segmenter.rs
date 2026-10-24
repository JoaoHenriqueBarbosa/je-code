//! `Intl.Segmenter` sobre a crate `icu_segmenter` (`IntlSegmenter.cpp`, `IntlSegments.cpp`,
//! `IntlSegmentIterator.cpp` e os protótipos): a segmentação em grafemas, palavras e sentenças, o objeto
//! `Segments` (`containing` e `[Symbol.iterator]`) e o iterador de segmentos.
//!
//! LOCALES cobertas: os dados do ICU4X para segmentação são os da raiz (o `WordSegmenter` automático
//! escolhe dicionário para chinês e japonês e LSTM para tailandês pelo script, como o `ubrk_open` do
//! ICU); o `resolvedOptions().locale` é `en-US` ou `pt-BR` conforme a resolução de `intl_locale_data.rs`.
//!
//! DIVERGÊNCIAS e LACUNAS: o ICU4X 2.3 implementa o UAX 29 do Unicode 17 com dicionários próprios, que
//! podem divergir do ICU4C do bun em textos CJK e tailandeses raros; `tests/segmenter_bun_golden.rs`
//! mede isso contra o bun.
//! - Os protótipos de `Segments` e do iterador são criados na instalação do `Intl` e as estruturas das
//!   instâncias ficam no global (`segments_structure`, `segment_iterator_structure`, os `LazyProperty`
//!   `m_segmentsStructure` e `m_segmentIteratorStructure`), como no C++.

use std::cell::Cell;
use crate::runtime::lookup::{native_entry};
use std::rc::Rc;

use icu_segmenter::options::{SentenceBreakInvariantOptions, WordBreakInvariantOptions};
use icu_segmenter::{GraphemeClusterSegmenter, SentenceSegmenter, WordSegmenter};

use crate::host_function;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::intl_support::{
    construct_instance, get_options_object, new_object, option_enum, put, read_locale_matcher, resolve_locale_from,
    str_value, with_instance, IntlClass, IntlEnum, IntlInstance,
};
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::create_iterator_result_object;
use crate::runtime::structure::{Structure, StructureRef};
use crate::wtf::text::wtf_string::String as WtfString;

crate::intl_enum!(Granularity { Grapheme => "grapheme", Word => "word", Sentence => "sentence" });


/// Os segmentos de `units`: o deslocamento de cada início em unidades UTF-16 (com o fim no final) e,
/// para palavras, se o segmento parece uma palavra.
fn segment_boundaries(units: &[u16], granularity: Granularity) -> (Vec<usize>, Vec<bool>) {
    let mut boundaries: Vec<usize> = Vec::new();
    let mut word_like: Vec<bool> = Vec::new();
    match granularity {
        Granularity::Grapheme => boundaries.extend(GraphemeClusterSegmenter::new().segment_utf16(units)),
        Granularity::Sentence => boundaries.extend(SentenceSegmenter::new(SentenceBreakInvariantOptions::default()).segment_utf16(units)),
        Granularity::Word => {
            // O ICU4C do bun segmenta chinês, japonês, tailandês e os demais por dicionário (medido: `สวัสดี` é uma
            // palavra só; o LSTM do `new_auto` a parte); `word_type` do limite descreve o segmento que o precede
            // (o primeiro limite, 0, não tem segmento).
            let mut iterator = WordSegmenter::new_dictionary(WordBreakInvariantOptions::default()).segment_utf16(units);
            while let Some(boundary) = iterator.next() {
                if boundaries.last().is_some() {
                    word_like.push(iterator.is_word_like());
                }
                boundaries.push(boundary);
            }
        }
    }
    if boundaries.first() != Some(&0) {
        boundaries.insert(0, 0);
    }
    if boundaries.last() != Some(&units.len()) {
        boundaries.push(units.len());
    }
    boundaries.dedup();
    word_like.resize(boundaries.len() - 1, false);
    // Medido no bun 1.4.2: um segmento que abre com letra ou dígito é palavra mesmo seguido de marca combinante (`é`
    // decomposto: o ICU4X marca `false`, o ICU4C `true`).
    if granularity == Granularity::Word {
        for (index, like) in word_like.iter_mut().enumerate() {
            let first = char::decode_utf16(units[boundaries[index]..boundaries[index + 1]].iter().copied()).next();
            *like = *like || first.is_some_and(|scalar| scalar.is_ok_and(char::is_alphanumeric));
        }
    }
    (boundaries, word_like)
}

// ---------------------------------------------------------------------------------------------
// As classes
// ---------------------------------------------------------------------------------------------

/// O estado de um `IntlSegmenter`.
struct SegmenterState {
    locale: String,
    granularity: Granularity,
}

/// O estado de um `IntlSegments`: a string e os limites dos segmentos.
struct SegmentsData {
    input: WtfString,
    granularity: Granularity,
    boundaries: Vec<usize>,
    word_like: Vec<bool>,
    units: Vec<u16>,
}

struct SegmentsState {
    data: Rc<SegmentsData>,
}

/// O estado de um `IntlSegmentIterator`: o próximo segmento a devolver.
struct SegmentIteratorState {
    data: Rc<SegmentsData>,
    next: Cell<usize>,
}

/// `createSegmentDataObject`: `{ segment, index, input }` e `isWordLike` nas palavras. O texto vai do início do segmento
/// `first` ao fim do segmento `segment` (iguais, salvo na quirk de `containing`); `isWordLike` é o de `segment`.
fn segment_data_object(global_object: &JSGlobalObject, data: &SegmentsData, first: usize, segment: usize) -> JSValue {
    let vm = global_object.vm();
    let (start, end) = (data.boundaries[first], data.boundaries[segment + 1]);
    let object = new_object(global_object);
    let text = WtfString::from_utf16(&data.units[start..end]);
    put(global_object, &object, "segment", JSValue::from_js_string(crate::runtime::js_string::js_string(vm, &text)));
    put(global_object, &object, "index", js_number(start as f64));
    put(global_object, &object, "input", JSValue::from_js_string(crate::runtime::js_string::js_string(vm, &data.input)));
    if data.granularity == Granularity::Word {
        put(global_object, &object, "isWordLike", js_boolean(data.word_like[segment]));
    }
    object.as_value()
}

/// `IntlSegmenter::initializeSegmenter`.
fn initialize(global_object: &JSGlobalObject, locales: JSValue, options_value: JSValue) -> Result<SegmenterState, Thrown> {
    let resolved = resolve_locale_from(global_object, locales, &[])?;
    // `intlGetOptionsObject`: um valor primitivo é `TypeError`, não é coagido a objeto.
    let options = get_options_object(options_value)?;
    read_locale_matcher(global_object, options)?;
    let granularity = option_enum::<Granularity>(
        global_object,
        options,
        "granularity",
        "granularity must be either \"grapheme\", \"word\", or \"sentence\"",
    )?
    .unwrap_or(Granularity::Grapheme);
    Ok(SegmenterState { locale: resolved.locale, granularity })
}

fn construct_segmenter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

fn call_segmenter_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    crate::runtime::collection_support::constructor_cannot_be_called_as_function("Segmenter")
}

/// `Intl.Segmenter.prototype.segment(string)`: a estrutura de `Segments` vem do global (`segmentsStructure()`).
fn segment_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<SegmenterState, _>(
        call.this_value(),
        "Intl.Segmenter.prototype.segment called on value that's not a Segmenter",
        |state, _| {
            let vm = global_object.vm();
            let input = call.argument(0).to_wtf_string();
            if vm.exception().is_some() {
                return Err(Thrown::Pending);
            }
            let units: Vec<u16> = (0..input.length()).map(|index| input.code_unit_at(index)).collect();
            let (boundaries, word_like) = segment_boundaries(&units, state.granularity);
            let data = Rc::new(SegmentsData { input, granularity: state.granularity, boundaries, word_like, units });
            let structure = global_object.segments_structure();
            Ok(IntlInstance::create(vm, &structure, Box::new(SegmentsState { data })).as_value())
        },
    )
}

fn segmenter_resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<SegmenterState, _>(
        call.this_value(),
        "Intl.Segmenter.prototype.resolvedOptions called on value that's not a Segmenter",
        |state, _| {
            let vm = global_object.vm();
            let options = new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(global_object, &options, "granularity", str_value(vm, state.granularity.as_str()));
            Ok(options.as_value())
        },
    )
}

/// `Segments.prototype.containing(index)`.
fn containing_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<SegmentsState, _>(
        call.this_value(),
        "%Segments.prototype%.containing called on value that's not a Segments",
        |state, _| {
            let index = call.argument(0).to_integer_or_infinity();
            crate::runtime::host_call::pending_or(global_object, ())?;
            let data = &state.data;
            if index < 0.0 || index >= data.units.len() as f64 {
                return Ok(JSValue::undefined());
            }
            let index = index as usize;
            let segment = data.boundaries.partition_point(|&boundary| boundary <= index) - 1;
            // Medido no bun 1.4.2: um `index` que abre um segmento iniciado por caractere suplementar (par substituto)
            // devolve o texto desde o início do segmento anterior (`ubrk_preceding` parte do meio do par).
            let opens_pair = data.boundaries[segment] == index
                && segment > 0
                && (0xD800..0xDC00).contains(&data.units[index])
                && data.units.get(index + 1).is_some_and(|unit| (0xDC00..0xE000).contains(unit));
            Ok(segment_data_object(global_object, data, if opens_pair { segment - 1 } else { segment }, segment))
        },
    )
}

/// `Segments.prototype[@@iterator]()`: a estrutura do iterador vem do global (`segmentIteratorStructure()`).
fn segments_iterator_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<SegmentsState, _>(
        call.this_value(),
        "%Segments.prototype%[@@iterator] called on value that's not a Segments",
        |state, _| {
            let vm = global_object.vm();
            let structure = global_object.segment_iterator_structure();
            let iterator = SegmentIteratorState { data: Rc::clone(&state.data), next: Cell::new(0) };
            Ok(IntlInstance::create(vm, &structure, Box::new(iterator)).as_value())
        },
    )
}

/// `%SegmentIteratorPrototype%.next()`.
fn iterator_next_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<SegmentIteratorState, _>(
        call.this_value(),
        "Intl.SegmentIterator.prototype.next called on value that's not a SegmentIterator",
        |state, _| {
            let segment = state.next.get();
            if segment + 1 >= state.data.boundaries.len() {
                return Ok(create_iterator_result_object(global_object, JSValue::undefined(), true));
            }
            state.next.set(segment + 1);
            Ok(create_iterator_result_object(global_object, segment_data_object(global_object, &state.data, segment, segment), false))
        },
    )
}

host_function!(call_segmenter, call_segmenter_body);
host_function!(construct_segmenter, construct_segmenter_body);
host_function!(segmenter_proto_segment, segment_body);
host_function!(segmenter_proto_resolved_options, segmenter_resolved_options_body);
host_function!(segments_proto_containing, containing_body);
host_function!(segments_proto_iterator, segments_iterator_body);
host_function!(segment_iterator_proto_next, iterator_next_body);

crate::intl_prototype_s_info!(
    SEGMENTER_PROTOTYPE_S_INFO,
    "Intl.Segmenter",
    [
        native_entry("segment", segmenter_proto_segment, 1),
        native_entry("resolvedOptions", segmenter_proto_resolved_options, 0),
    ]
);
crate::intl_prototype_s_info!(SEGMENTS_PROTOTYPE_S_INFO, "%Segments%", [native_entry("containing", segments_proto_containing, 1)]);
crate::intl_prototype_s_info!(
    SEGMENT_ITERATOR_PROTOTYPE_S_INFO,
    "Segment String Iterator",
    [native_entry("next", segment_iterator_proto_next, 0)]
);

impl JSGlobalObject {
    /// `segmentsStructure()`: a estrutura das instâncias de `Segments` (protótipo `%Segments%`).
    pub fn segments_structure(&self) -> StructureRef {
        self.segments_structure.borrow().clone().expect("JSGlobalObject sem segmentsStructure")
    }

    /// `segmentIteratorStructure()`: a estrutura das instâncias do iterador (protótipo `%SegmentIteratorPrototype%`).
    pub fn segment_iterator_structure(&self) -> StructureRef {
        self.segment_iterator_structure.borrow().clone().expect("JSGlobalObject sem segmentIteratorStructure")
    }
}

/// `IntlXPrototype::createStructure` com `HasStaticPropertyTable` e a tabela do `ClassInfo`.
fn table_prototype(global_object: &JSGlobalObject, prototype: JSValue, info: &'static ClassInfo) -> JSObjectRef {
    let vm = global_object.vm();
    let structure = Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
        info,
    );
    let object = JSObject::allocate(vm, &structure);
    object.finish_creation(vm);
    object
}

/// `IntlSegmenterConstructor`, `IntlSegmenterPrototype` e os protótipos de `Segments` e do iterador
/// (`m_segmentsStructure` e `m_segmentIteratorStructure` do global).
pub fn install_segmenter(global_object: &JSGlobalObject, intl: &JSObject) {
    let vm = global_object.vm();
    let object_prototype_value = global_object.object_prototype().as_value();

    // `IntlSegmentIteratorPrototype`: herda de `%IteratorPrototype%`; `next` pela tabela, `@@toStringTag` eager.
    let iterator_prototype =
        table_prototype(global_object, global_object.iterator_prototype().as_value(), &SEGMENT_ITERATOR_PROTOTYPE_S_INFO);
    put_to_string_tag(vm, &iterator_prototype, "Segment String Iterator");
    *global_object.segment_iterator_structure.borrow_mut() =
        Some(IntlInstance::create_structure(vm, Some(global_object), iterator_prototype.as_value()));

    // `IntlSegmentsPrototype`: `containing` pela tabela, `[Symbol.iterator]` eager no `finishCreation`.
    let segments_prototype = table_prototype(global_object, object_prototype_value, &SEGMENTS_PROTOTYPE_S_INFO);
    let iterate = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_utf8(b"[Symbol.iterator]"),
        segments_proto_iterator,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    segments_prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.iterator_symbol), iterate.as_value(), DONT_ENUM);
    *global_object.segments_structure.borrow_mut() =
        Some(IntlInstance::create_structure(vm, Some(global_object), segments_prototype.as_value()));

    let class = IntlClass {
        name: "Segmenter",
        length: 0,
        has_supported_locales_of: true,
        call: call_segmenter,
        construct: construct_segmenter,
    };
    class.install_with_table(global_object, intl, &SEGMENTER_PROTOTYPE_S_INFO);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(text: &str, granularity: Granularity) -> Vec<(String, bool)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let (boundaries, word_like) = segment_boundaries(&units, granularity);
        (0..boundaries.len() - 1)
            .map(|index| (String::from_utf16(&units[boundaries[index]..boundaries[index + 1]]).unwrap(), word_like[index]))
            .collect()
    }

    fn texts(text: &str, granularity: Granularity) -> Vec<String> {
        pieces(text, granularity).into_iter().map(|(piece, _)| piece).collect()
    }

    #[test]
    fn graphemes() {
        assert_eq!(texts("abc", Granularity::Grapheme), ["a", "b", "c"]);
        assert_eq!(texts("e\u{301}x", Granularity::Grapheme), ["e\u{301}", "x"]);
        assert_eq!(texts("\r\n", Granularity::Grapheme), ["\r\n"]);
        assert_eq!(texts("\u{1f1e7}\u{1f1f7}\u{1f1fa}\u{1f1f8}", Granularity::Grapheme), ["\u{1f1e7}\u{1f1f7}", "\u{1f1fa}\u{1f1f8}"]);
        assert_eq!(
            texts("\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}!", Granularity::Grapheme),
            ["\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}", "!"]
        );
        assert_eq!(texts("\u{1100}\u{1161}\u{11a8}", Granularity::Grapheme).len(), 1);
    }

    #[test]
    fn words() {
        let pieces = pieces("Hello, world! It's 3.14", Granularity::Word);
        let words: Vec<(&str, bool)> = pieces.iter().map(|(piece, word_like)| (piece.as_str(), *word_like)).collect();
        assert_eq!(
            words,
            [
                ("Hello", true),
                (",", false),
                (" ", false),
                ("world", true),
                ("!", false),
                (" ", false),
                ("It's", true),
                (" ", false),
                ("3.14", true)
            ]
        );
    }

    #[test]
    fn sentences() {
        assert_eq!(texts("Hi there. How are you? Fine!", Granularity::Sentence), ["Hi there. ", "How are you? ", "Fine!"]);
        assert_eq!(texts("Pi is 3.14 ok. Next", Granularity::Sentence), ["Pi is 3.14 ok. ", "Next"]);
        assert_eq!(texts("see e.g. this one", Granularity::Sentence), ["see e.g. this one"]);
        // SB7: `Upper|Lower ATerm × Upper`; SB6: `ATerm × Numeric`.
        assert_eq!(texts("Hello.World", Granularity::Sentence), ["Hello.World"]);
        assert_eq!(texts("v1.2 now. Go", Granularity::Sentence), ["v1.2 now. ", "Go"]);
        // SB8: o que não é letra no meio não impede a continuação por minúscula.
        assert_eq!(texts("Open at 5. 6 apples", Granularity::Sentence), ["Open at 5. 6 apples"]);
        // SB8a: nenhum terminador termina a sentença antes de `SContinue`.
        assert_eq!(texts("Is it? - yes", Granularity::Sentence), ["Is it? - yes"]);
        assert_eq!(texts("Is it? Yes, fine", Granularity::Sentence), ["Is it? ", "Yes, fine"]);
    }

    #[test]
    fn crlf_is_one_word_segment() {
        assert_eq!(texts("a\r\nb", Granularity::Word), ["a", "\r\n", "b"]);
    }

    #[test]
    fn word_like_marks_letters_and_numbers_only() {
        let marks: Vec<(String, bool)> = pieces("a1 -b", Granularity::Word);
        assert_eq!(
            marks,
            [("a1".to_string(), true), (" ".to_string(), false), ("-".to_string(), false), ("b".to_string(), true)]
        );
    }
}
