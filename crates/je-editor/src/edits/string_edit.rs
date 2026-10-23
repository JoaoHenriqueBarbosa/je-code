//! Porte de `src/vs/editor/common/core/edits/stringEdit.ts`: `BaseStringEdit`, `BaseStringReplacement`,
//! `StringEdit`, `StringReplacement`, `applyEditsToRanges`, `IEditData`, `VoidEditData`,
//! `AnnotatedStringEdit` e `AnnotatedStringReplacement`.
//!
//! O texto é UTF-16 ([`JsString`] e `&[u16]`), então deslocamentos e comprimentos contam unidades UTF-16 como
//! no JavaScript. As classes abstratas viram traits ([`BaseStringEdit`] estende `BaseEdit`;
//! [`BaseStringReplacement`] estende `BaseReplacement`), com `Self` no lugar de `TEdit` e `T`.
//!
//! Diferenças inevitáveis:
//! - `StringEdit.compose(edits)` (estático) é [`StringEdit::compose_all`], porque o nome `compose` é o do método
//!   de instância do trait.
//! - `AnnotatedStringEdit.compose(edits)` (estático) é [`AnnotatedStringEdit::compose_all`], pelo mesmo motivo.
//! - `data` das substituições anotadas é comparado com `PartialEq`, no lugar da identidade `===`.
//! - `toEdit` de uma substituição anotada devolve um `StringEdit` com o mesmo intervalo e texto (o original
//!   guarda o próprio objeto anotado dentro do `StringEdit`).
//! - `normalizeEOL` recebe o enum [`Eol`] no lugar de `'\r\n' | '\n'`.

use std::collections::VecDeque;
use std::fmt;

use je_base::JsString;
use je_base::js_string::{self, is_js_whitespace};
use je_base::strings::{common_prefix_length, common_suffix_length};

use super::edit::{BaseEdit, BaseReplacement, EditPair, validate_replacements};
use crate::offset_range::OffsetRange;
use crate::text::abstract_text::StringText;

/// `'\r\n' | '\n'`: o fim de linha do `normalizeEOL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    CrLf,
    Lf,
}

impl Eol {
    fn units(self) -> &'static [u16] {
        match self {
            Eol::CrLf => &[13, 10],
            Eol::Lf => &[10],
        }
    }
}

/// `JSON.stringify(text)` de uma string: aspas, escapes curtos, `\u00xx` para controles e `\udxxx` para
/// surrogates isolados.
fn json_stringify(units: &[u16]) -> String {
    let mut out = String::from("\"");
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok('\t') => out.push_str("\\t"),
            Ok(c) if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            Ok(c) => out.push(c),
            Err(lone) => out.push_str(&format!("\\u{:04x}", lone.unpaired_surrogate())),
        }
    }
    out.push('"');
    out
}

fn units_of(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `text.replace(/from/g, to)` com `from` literal (e `text.replaceAll(from, to)`).
pub(crate) fn replace_all_units(text: &[u16], from: &[u16], to: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

/// `text.replace(/\r\n|\n/g, eol)`.
fn normalize_eol_units(text: &[u16], eol: Eol) -> JsString {
    let mut out: Vec<u16> = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == 13 && text.get(i + 1) == Some(&10) {
            out.extend_from_slice(eol.units());
            i += 2;
        } else if text[i] == 10 {
            out.extend_from_slice(eol.units());
            i += 1;
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    JsString::from_units(out)
}

/// `Warning: Be careful when changing this type, as it is used for serialization!`
pub type SerializedStringEdit = Vec<SerializedStringReplacement>;

/// `ISerializedStringReplacement`.
///
/// `Warning: Be careful when changing this type, as it is used for serialization!`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedStringReplacement {
    pub txt: JsString,
    pub pos: i32,
    pub len: i32,
}

fn describe_string_replacement<R: BaseStringReplacement>(replacement: &R) -> String {
    format!("{} -> {}", replacement.replace_range(), json_stringify(replacement.new_text().units()))
}

/// `BaseStringReplacement<T>`.
pub trait BaseStringReplacement: BaseReplacement {
    /// `newText`.
    fn new_text(&self) -> &JsString;

    fn replace(&self, str: &[u16]) -> JsString {
        let range = self.replace_range();
        let mut result = JsString::from(js_string::substring(str, 0, i64::from(range.start)));
        result.push_units(self.new_text().units());
        result.push_units(js_string::substring(str, i64::from(range.end_exclusive), str.len() as i64));
        result
    }

    /// Confere se a edição não produziria mudança ao ser aplicada ao texto dado.
    fn is_neutral_on(&self, text: &[u16]) -> bool {
        self.new_text().units() == self.replace_range().substring(text)
    }

    fn remove_common_suffix_prefix(&self, original_text: &[u16]) -> StringReplacement {
        let old_text = self.replace_range().substring(original_text);
        let new_text = self.new_text().units();

        let prefix_len = common_prefix_length(old_text, new_text) as i32;
        let suffix_len = (old_text.len() as i32 - prefix_len)
            .min(new_text.len() as i32 - prefix_len)
            .min(common_suffix_length(old_text, new_text) as i32);

        let replace_range = OffsetRange::new(
            self.replace_range().start + prefix_len,
            self.replace_range().end_exclusive - suffix_len,
        );
        let new_text = JsString::from(js_string::substring(
            new_text,
            i64::from(prefix_len),
            new_text.len() as i64 - i64::from(suffix_len),
        ));

        StringReplacement::new(replace_range, new_text)
    }

    fn normalize_eol(&self, eol: Eol) -> StringReplacement {
        StringReplacement::new(self.replace_range(), normalize_eol_units(self.new_text().units(), eol))
    }

    fn remove_common_suffix_and_prefix(&self, source: &[u16]) -> Self {
        self.remove_common_suffix(source).remove_common_prefix(source)
    }

    fn remove_common_prefix(&self, source: &[u16]) -> Self {
        let old_text = self.replace_range().substring(source);

        let prefix_len = common_prefix_length(old_text, self.new_text().units()) as i32;
        if prefix_len == 0 {
            return self.clone();
        }

        self.slice(
            self.replace_range().delta_start(prefix_len),
            Some(OffsetRange::new(prefix_len, self.new_text().len() as i32)),
        )
    }

    fn remove_common_suffix(&self, source: &[u16]) -> Self {
        let old_text = self.replace_range().substring(source);

        let suffix_len = common_suffix_length(old_text, self.new_text().units()) as i32;
        if suffix_len == 0 {
            return self.clone();
        }
        self.slice(
            self.replace_range().delta_end(-suffix_len),
            Some(OffsetRange::new(0, self.new_text().len() as i32 - suffix_len)),
        )
    }

    fn to_edit(&self) -> StringEdit {
        StringEdit::new(vec![StringReplacement::new(self.replace_range(), self.new_text().clone())])
    }

    fn to_json(&self) -> SerializedStringReplacement {
        SerializedStringReplacement {
            txt: self.new_text().clone(),
            pos: self.replace_range().start,
            len: self.replace_range().length(),
        }
    }
}

/// `BaseStringEdit<T, TEdit>`.
pub trait BaseStringEdit: BaseEdit
where
    Self::Replacement: BaseStringReplacement,
{
    /// `BaseStringEdit.composeOrUndefined`.
    fn compose_or_undefined(edits: &[Self]) -> Option<Self> {
        let (first, rest) = edits.split_first()?;
        let mut result = first.clone();
        for edit in rest {
            result = result.compose(edit);
        }
        Some(result)
    }

    /// `r := trySwap(e1, e2); e1.compose(e2) === r.e1.compose(r.e2)`.
    fn try_swap<B: BaseStringEdit>(e1: &Self, e2: &B) -> Option<EditPair<StringEdit>>
    where
        B::Replacement: BaseStringReplacement,
    {
        // TODO do original: tornar mais eficiente.
        let e1_inv = e1.inverse_on_slice(|start, end_ex| JsString::from_units(vec![32; (end_ex - start) as usize]));

        let e1_ = e2.try_rebase(&e1_inv)?;
        let e2_ = e1.try_rebase(&e1_)?;

        Some(EditPair { e1: e1_, e2: e2_ })
    }

    fn apply(&self, base: &[u16]) -> JsString {
        let mut result_text = JsString::new();
        let mut pos = 0;
        for edit in self.replacements() {
            result_text.push_units(js_string::substring(base, pos, i64::from(edit.replace_range().start)));
            result_text.push_units(edit.new_text().units());
            pos = i64::from(edit.replace_range().end_exclusive);
        }
        result_text.push_units(js_string::substring(base, pos, base.len() as i64));
        result_text
    }

    /// Cria uma edição que desfaz esta.
    fn inverse_on_slice(&self, get_original_slice: impl Fn(i32, i32) -> JsString) -> StringEdit {
        let mut edits: Vec<StringReplacement> = Vec::new();
        let mut offset = 0;
        for e in self.replacements() {
            edits.push(StringReplacement::new(
                OffsetRange::of_start_and_length(e.replace_range().start + offset, e.new_text().len() as i32),
                get_original_slice(e.replace_range().start, e.replace_range().end_exclusive),
            ));
            offset += e.new_text().len() as i32 - e.replace_range().length();
        }
        StringEdit::new(edits)
    }

    /// Cria uma edição que desfaz esta.
    fn inverse(&self, original: &[u16]) -> StringEdit {
        self.inverse_on_slice(|start, end_ex| {
            JsString::from(js_string::substring(original, i64::from(start), i64::from(end_ex)))
        })
    }

    fn rebase_skip_conflicting(&self, base: &StringEdit) -> StringEdit {
        try_rebase_impl(self, base, false).expect("sem conflito exigido, a rebase sempre resolve")
    }

    fn try_rebase(&self, base: &StringEdit) -> Option<StringEdit> {
        try_rebase_impl(self, base, true)
    }

    fn to_json(&self) -> SerializedStringEdit {
        self.replacements().iter().map(BaseStringReplacement::to_json).collect()
    }

    fn is_neutral_on(&self, text: &[u16]) -> bool {
        self.replacements().iter().all(|e| e.is_neutral_on(text))
    }

    fn remove_common_suffix_prefix(&self, original_text: &[u16]) -> StringEdit {
        let mut edits: Vec<StringReplacement> = Vec::new();
        for e in self.replacements() {
            let edit = e.remove_common_suffix_prefix(original_text);
            if !BaseReplacement::is_empty(&edit) {
                edits.push(edit);
            }
        }
        StringEdit::new(edits)
    }

    fn normalize_eol(&self, eol: Eol) -> StringEdit {
        StringEdit::new(self.replacements().iter().map(|edit| edit.normalize_eol(eol)).collect())
    }

    /// Se `e1.apply(source) === e2.apply(source)`, então
    /// `e1.normalizeOnSource(source).equals(e2.normalizeOnSource(source))`.
    fn normalize_on_source(&self, source: &[u16]) -> StringEdit {
        let result = self.apply(source);

        let edit = StringReplacement::new(OffsetRange::of_length(source.len() as i32), result);
        let e = edit.remove_common_suffix_and_prefix(source);
        if BaseReplacement::is_empty(&e) {
            return StringEdit::empty();
        }
        e.to_edit()
    }

    fn remove_common_suffix_and_prefix(&self, source: &[u16]) -> Self {
        Self::create_new(self.replacements().iter().map(|e| e.remove_common_suffix_and_prefix(source)).collect()).normalize()
    }

    fn apply_on_text(&self, doc_contents: &StringText) -> StringText {
        StringText::new(self.apply(doc_contents.value.units()))
    }

    fn map_data<D: IEditData>(&self, f: impl Fn(&Self::Replacement) -> D) -> AnnotatedStringEdit<D> {
        AnnotatedStringEdit::new(
            self.replacements()
                .iter()
                .map(|e| AnnotatedStringReplacement::new(e.replace_range(), e.new_text().clone(), f(e)))
                .collect(),
        )
    }
}

/// `_tryRebase`.
fn try_rebase_impl<E: BaseStringEdit>(this: &E, base: &StringEdit, no_overlap: bool) -> Option<StringEdit>
where
    E::Replacement: BaseStringReplacement,
{
    let ours = this.replacements();
    let bases = base.replacements();
    let mut new_edits: Vec<StringReplacement> = Vec::new();

    let mut base_idx = 0;
    let mut our_idx = 0;
    let mut offset = 0;

    while our_idx < ours.len() || base_idx < bases.len() {
        // pega a edição que começa primeiro
        let base_edit = bases.get(base_idx);
        let our_edit = ours.get(our_idx);

        let Some(our_edit) = our_edit else {
            // Processamos todas as nossas edições
            break;
        };
        let our_range = our_edit.replace_range();
        let Some(base_edit) = base_edit else {
            // não há mais edições da base
            let transformed_range = our_range.delta(offset);
            new_edits.push(StringReplacement::new(transformed_range, our_edit.new_text().clone()));
            our_idx += 1;
            continue;
        };
        let base_range = base_edit.replace_range;

        if our_range.intersects(base_range)
            || are_concurrent_inserts(our_range, base_range)
            || is_insert_strictly_inside_range(our_range, base_range)
            || is_insert_strictly_inside_range(base_range, our_range)
        {
            our_idx += 1; // Não pega a nossa edição, pois ela conflita: pula
            if no_overlap {
                return None;
            }
        } else if our_range.start < base_range.start || (our_range.is_empty() && our_range.start == base_range.start) {
            // A nossa edição começa primeiro, ou é uma inserção no início do intervalo da base
            let transformed_range = our_range.delta(offset);
            // Confere se a edição transformada violaria a invariante de ordenada e disjunta
            new_edits.push(StringReplacement::new(transformed_range, our_edit.new_text().clone()));
            our_idx += 1;
        } else {
            base_idx += 1;
            offset += base_edit.new_text.len() as i32 - base_range.length();
        }
    }

    Some(StringEdit::new(new_edits))
}

/// `StringEdit`: um conjunto de substituições de uma string. Todas são aplicadas de uma vez.
#[derive(Clone, Debug)]
pub struct StringEdit {
    replacements: Vec<StringReplacement>,
}

impl StringEdit {
    /// O construtor.
    pub fn new(replacements: Vec<StringReplacement>) -> Self {
        validate_replacements(&replacements);
        Self { replacements }
    }

    /// Lê uma edição da sua representação em texto.
    /// Por exemplo `[[2, 12) -> "fgh", [14, 20) -> "qrst", [22, 22) -> "de\n"]`.
    pub fn parse(to_string_value: &[u16]) -> StringEdit {
        let mut replacements: Vec<StringReplacement> = Vec::new();

        // O `regex.exec` global: procura o próximo casamento a partir do fim do anterior.
        let mut i = 0;
        while i < to_string_value.len() {
            let Some((start, end_ex, text, next)) = match_serialized_replacement(to_string_value, i) else {
                i += 1;
                continue;
            };
            let text = replace_all_units(text, &units_of("\\n"), &[10]);
            let text = replace_all_units(&text, &units_of("\\r"), &[13]);
            let text = replace_all_units(&text, &units_of("\\\\"), &[92]);
            replacements.push(StringReplacement::new(OffsetRange::new(start, end_ex), JsString::from_units(text)));
            i = next;
        }

        StringEdit::new(replacements)
    }

    /// `StringEdit.empty`.
    pub fn empty() -> StringEdit {
        StringEdit::new(Vec::new())
    }

    pub fn create(replacements: Vec<StringReplacement>) -> StringEdit {
        StringEdit::new(replacements)
    }

    pub fn single(replacement: StringReplacement) -> StringEdit {
        StringEdit::new(vec![replacement])
    }

    pub fn replace(range: OffsetRange, replacement: impl Into<JsString>) -> StringEdit {
        StringEdit::new(vec![StringReplacement::new(range, replacement)])
    }

    pub fn insert(offset: i32, replacement: impl Into<JsString>) -> StringEdit {
        StringEdit::new(vec![StringReplacement::new(OffsetRange::empty_at(offset), replacement)])
    }

    pub fn delete(range: OffsetRange) -> StringEdit {
        StringEdit::new(vec![StringReplacement::new(range, JsString::new())])
    }

    pub fn from_json(data: &[SerializedStringReplacement]) -> StringEdit {
        StringEdit::new(data.iter().map(StringReplacement::from_json).collect())
    }

    /// `StringEdit.compose(edits)`: o estático do original.
    pub fn compose_all(edits: &[StringEdit]) -> StringEdit {
        let Some((first, rest)) = edits.split_first() else {
            return StringEdit::empty();
        };
        let mut result = first.clone();
        for edit in rest {
            result = result.compose(edit);
        }
        result
    }

    /// As substituições são aplicadas em ordem! Equivale a
    /// `StringEdit.compose(replacements.map(r => r.toEdit()))`, mas é muito mais rápido.
    pub fn compose_sequential_replacements(replacements: &[StringReplacement]) -> StringEdit {
        let mut edit = StringEdit::empty();
        let mut cur_edit_replacements: Vec<StringReplacement> = Vec::new(); // Estão em ordem reversa

        for r in replacements {
            let is_next_in_reverse =
                cur_edit_replacements.last().is_none_or(|last| r.replace_range.is_before(last.replace_range));
            if is_next_in_reverse {
                // Detecta subsequências de substituições em ordem reversa
                cur_edit_replacements.push(r.clone());
            } else {
                // Quando a subsequência quebra, compõe as substituições atuais e procura uma nova subsequência.
                cur_edit_replacements.reverse();
                edit = edit.compose(&StringEdit::create(std::mem::take(&mut cur_edit_replacements)));
                cur_edit_replacements = vec![r.clone()];
            }
        }

        cur_edit_replacements.reverse();
        edit.compose(&StringEdit::create(cur_edit_replacements))
    }
}

impl BaseEdit for StringEdit {
    type Replacement = StringReplacement;

    fn replacements(&self) -> &[StringReplacement] {
        &self.replacements
    }

    fn create_new(replacements: Vec<StringReplacement>) -> Self {
        StringEdit::new(replacements)
    }
}

impl BaseStringEdit for StringEdit {}

impl fmt::Display for StringEdit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

/// Casa `\[(\d+),\s*(\d+)\)\s*->\s*"([^"]*)"` em `from`: o início, o fim, o texto e o índice depois do casamento.
fn match_serialized_replacement(s: &[u16], from: usize) -> Option<(i32, i32, &[u16], usize)> {
    fn expect(s: &[u16], i: usize, unit: char) -> Option<usize> {
        (s.get(i) == Some(&(unit as u16))).then_some(i + 1)
    }
    fn skip_whitespace(s: &[u16], mut i: usize) -> usize {
        while s.get(i).is_some_and(|&u| is_js_whitespace(u)) {
            i += 1;
        }
        i
    }
    // `parseInt(digits, 10)`, saturando no maior `i32`.
    fn digits(s: &[u16], mut i: usize) -> Option<(i32, usize)> {
        let first = i;
        let mut value: i64 = 0;
        while let Some(&u) = s.get(i)
            && (0x30..=0x39).contains(&u)
        {
            value = (value * 10 + i64::from(u - 0x30)).min(i64::from(i32::MAX));
            i += 1;
        }
        (i > first).then_some((value as i32, i))
    }

    let i = expect(s, from, '[')?;
    let (start, i) = digits(s, i)?;
    let i = expect(s, i, ',')?;
    let i = skip_whitespace(s, i);
    let (end_ex, i) = digits(s, i)?;
    let i = expect(s, i, ')')?;
    let i = skip_whitespace(s, i);
    let i = expect(s, i, '-')?;
    let i = expect(s, i, '>')?;
    let i = skip_whitespace(s, i);
    let i = expect(s, i, '"')?;
    let text_end = i + s[i..].iter().position(|&u| u == '"' as u16)?;
    Some((start, end_ex, &s[i..text_end], text_end + 1))
}

/// `StringReplacement`: a troca de um intervalo por um texto.
#[derive(Clone, Debug)]
pub struct StringReplacement {
    pub replace_range: OffsetRange,
    pub new_text: JsString,
}

impl StringReplacement {
    pub fn new(range: OffsetRange, new_text: impl Into<JsString>) -> Self {
        Self { replace_range: range, new_text: new_text.into() }
    }

    pub fn insert(offset: i32, text: impl Into<JsString>) -> StringReplacement {
        StringReplacement::new(OffsetRange::empty_at(offset), text)
    }

    pub fn replace(range: OffsetRange, text: impl Into<JsString>) -> StringReplacement {
        StringReplacement::new(range, text)
    }

    pub fn delete(range: OffsetRange) -> StringReplacement {
        StringReplacement::new(range, JsString::new())
    }

    pub fn from_json(data: &SerializedStringReplacement) -> StringReplacement {
        StringReplacement::new(OffsetRange::of_start_and_length(data.pos, data.len), data.txt.clone())
    }
}

impl BaseReplacement for StringReplacement {
    fn replace_range(&self) -> OffsetRange {
        self.replace_range
    }

    fn get_new_length(&self) -> i32 {
        self.new_text.len() as i32
    }

    fn describe(&self) -> String {
        describe_string_replacement(self)
    }

    fn equals(&self, other: &Self) -> bool {
        self.replace_range.equals(other.replace_range) && self.new_text == other.new_text
    }

    fn try_join_touching(&self, other: &Self) -> Option<Self> {
        let mut text = self.new_text.clone();
        text.push_units(other.new_text.units());
        Some(StringReplacement::new(self.replace_range.join_right_touching(other.replace_range), text))
    }

    fn slice(&self, range: OffsetRange, range_in_replacement: Option<OffsetRange>) -> Self {
        StringReplacement::new(
            range,
            range_in_replacement.map_or_else(
                || self.new_text.clone(),
                |r| JsString::from(r.substring(self.new_text.units())),
            ),
        )
    }
}

impl BaseStringReplacement for StringReplacement {
    fn new_text(&self) -> &JsString {
        &self.new_text
    }
}

impl fmt::Display for StringReplacement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

/// `applyEditsToRanges`.
pub fn apply_edits_to_ranges(sorted_ranges: &[OffsetRange], edit: &StringEdit) -> Vec<OffsetRange> {
    let mut sorted_ranges: VecDeque<OffsetRange> = sorted_ranges.iter().copied().collect();

    // trata as edições como a remoção do intervalo substituído e depois uma inserção que estende o primeiro intervalo
    let mut result: Vec<OffsetRange> = Vec::new();

    let mut offset = 0;

    for e in edit.replacements() {
        let new_len = e.new_text.len() as i32;
        loop {
            // intervalos antes da edição atual
            let Some(&r) = sorted_ranges.front() else {
                break;
            };
            if r.end_exclusive >= e.replace_range.start {
                break;
            }
            sorted_ranges.pop_front();
            result.push(r.delta(offset));
        }

        let mut intersecting: Vec<OffsetRange> = Vec::new();
        loop {
            let Some(&r) = sorted_ranges.front() else {
                break;
            };
            if !r.intersects_or_touches(e.replace_range) {
                break;
            }
            sorted_ranges.pop_front();
            intersecting.push(r);
        }

        for i in (0..intersecting.len()).rev() {
            let mut r = intersecting[i];

            let overlap = r
                .intersect(e.replace_range)
                .expect("o intervalo cruza ou toca a substituição")
                .length();
            r = r.delta_end(-overlap + if i == 0 { new_len } else { 0 });

            let range_ahead_of_replace_range = r.start - e.replace_range.start;
            if range_ahead_of_replace_range > 0 {
                r = r.delta(-range_ahead_of_replace_range);
            }

            if i != 0 {
                r = r.delta(new_len);
            }

            // Já levamos o nosso deslocamento em conta.
            // Como devolvemos r à fila (que soma o deslocamento de novo),
            // temos de tirá-lo aqui.
            r = r.delta(-(new_len - e.replace_range.length()));

            sorted_ranges.push_front(r);
        }

        offset += new_len - e.replace_range.length();
    }

    for r in sorted_ranges {
        result.push(r.delta(offset));
    }

    result
}

/// `IEditData<T>`: dado associado a uma única edição, que sobrevive a certas operações de edição.
pub trait IEditData: Clone + PartialEq + fmt::Debug {
    fn join(&self, other: &Self) -> Option<Self>;
}

/// `VoidEditData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoidEditData;

impl IEditData for VoidEditData {
    fn join(&self, _other: &VoidEditData) -> Option<VoidEditData> {
        Some(self.clone())
    }
}

/// `AnnotatedStringEdit<T>`: um conjunto de substituições de uma string, cada uma com o seu dado.
#[derive(Clone, Debug)]
pub struct AnnotatedStringEdit<D: IEditData> {
    replacements: Vec<AnnotatedStringReplacement<D>>,
}

impl<D: IEditData> AnnotatedStringEdit<D> {
    pub fn new(replacements: Vec<AnnotatedStringReplacement<D>>) -> Self {
        validate_replacements(&replacements);
        Self { replacements }
    }

    /// `AnnotatedStringEdit.empty`.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn create(replacements: Vec<AnnotatedStringReplacement<D>>) -> Self {
        Self::new(replacements)
    }

    pub fn single(replacement: AnnotatedStringReplacement<D>) -> Self {
        Self::new(vec![replacement])
    }

    pub fn replace(range: OffsetRange, replacement: impl Into<JsString>, data: D) -> Self {
        Self::new(vec![AnnotatedStringReplacement::new(range, replacement, data)])
    }

    pub fn insert(offset: i32, replacement: impl Into<JsString>, data: D) -> Self {
        Self::new(vec![AnnotatedStringReplacement::new(OffsetRange::empty_at(offset), replacement, data)])
    }

    pub fn delete(range: OffsetRange, data: D) -> Self {
        Self::new(vec![AnnotatedStringReplacement::new(range, JsString::new(), data)])
    }

    /// `AnnotatedStringEdit.compose(edits)`: o estático do original.
    pub fn compose_all(edits: &[AnnotatedStringEdit<D>]) -> AnnotatedStringEdit<D> {
        let Some((first, rest)) = edits.split_first() else {
            return AnnotatedStringEdit::empty();
        };
        let mut result = first.clone();
        for edit in rest {
            result = result.compose(edit);
        }
        result
    }

    pub fn to_string_edit(&self, filter: Option<&dyn Fn(&AnnotatedStringReplacement<D>) -> bool>) -> StringEdit {
        let mut new_replacements: Vec<StringReplacement> = Vec::new();
        for r in &self.replacements {
            if filter.is_none_or(|f| f(r)) {
                new_replacements.push(StringReplacement::new(r.replace_range, r.new_text.clone()));
            }
        }
        StringEdit::new(new_replacements)
    }
}

impl<D: IEditData> BaseEdit for AnnotatedStringEdit<D> {
    type Replacement = AnnotatedStringReplacement<D>;

    fn replacements(&self) -> &[AnnotatedStringReplacement<D>] {
        &self.replacements
    }

    fn create_new(replacements: Vec<AnnotatedStringReplacement<D>>) -> Self {
        Self::new(replacements)
    }
}

impl<D: IEditData> BaseStringEdit for AnnotatedStringEdit<D> {}

/// `AnnotatedStringReplacement<T>`.
#[derive(Clone, Debug)]
pub struct AnnotatedStringReplacement<D: IEditData> {
    pub replace_range: OffsetRange,
    pub new_text: JsString,
    pub data: D,
}

impl<D: IEditData> AnnotatedStringReplacement<D> {
    pub fn new(range: OffsetRange, new_text: impl Into<JsString>, data: D) -> Self {
        Self { replace_range: range, new_text: new_text.into(), data }
    }

    pub fn insert(offset: i32, text: impl Into<JsString>, data: D) -> Self {
        Self::new(OffsetRange::empty_at(offset), text, data)
    }

    pub fn replace(range: OffsetRange, text: impl Into<JsString>, data: D) -> Self {
        Self::new(range, text, data)
    }

    pub fn delete(range: OffsetRange, data: D) -> Self {
        Self::new(range, JsString::new(), data)
    }
}

impl<D: IEditData> BaseReplacement for AnnotatedStringReplacement<D> {
    fn replace_range(&self) -> OffsetRange {
        self.replace_range
    }

    fn get_new_length(&self) -> i32 {
        self.new_text.len() as i32
    }

    fn describe(&self) -> String {
        describe_string_replacement(self)
    }

    fn equals(&self, other: &Self) -> bool {
        self.replace_range.equals(other.replace_range) && self.new_text == other.new_text && self.data == other.data
    }

    fn try_join_touching(&self, other: &Self) -> Option<Self> {
        let joined = self.data.join(&other.data)?;
        let mut text = self.new_text.clone();
        text.push_units(other.new_text.units());
        Some(Self::new(self.replace_range.join_right_touching(other.replace_range), text, joined))
    }

    fn slice(&self, range: OffsetRange, range_in_replacement: Option<OffsetRange>) -> Self {
        Self::new(
            range,
            range_in_replacement.map_or_else(
                || self.new_text.clone(),
                |r| JsString::from(r.substring(self.new_text.units())),
            ),
            self.data.clone(),
        )
    }
}

impl<D: IEditData> BaseStringReplacement for AnnotatedStringReplacement<D> {
    fn new_text(&self) -> &JsString {
        &self.new_text
    }
}

/// Verdadeiro se os dois intervalos são vazios (inserções) exatamente na mesma posição.
/// Nesse caso, embora não se "cruzem" no sentido tradicional, eles conflitam porque a ordem da inserção importa.
fn are_concurrent_inserts(r1: OffsetRange, r2: OffsetRange) -> bool {
    r1.is_empty() && r2.is_empty() && r1.start == r2.start
}

/// Verdadeiro se `insert` é um intervalo vazio (inserção) estritamente dentro de `range`.
/// Por exemplo, a inserção na posição 5 está dentro de `[3, 7)`, mas não de `[5, 7)` nem de `[3, 5)`.
fn is_insert_strictly_inside_range(insert: OffsetRange, range: OffsetRange) -> bool {
    insert.is_empty() && range.start < insert.start && insert.start < range.end_exclusive
}
