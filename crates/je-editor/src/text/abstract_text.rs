//! Porte de `src/vs/editor/common/core/text/abstractText.ts`: `AbstractText`, `LineBasedText`, `ArrayText` e
//! `StringText`.
//!
//! Notas de tradução:
//! - `AbstractText` (classe abstrata) é o trait [`AbstractText`], usado como `&dyn AbstractText`. O campo
//!   `_transformer` da base (cache do `getTransformer`) vira o método exigido [`AbstractText::transformer_cache`],
//!   que devolve a célula de cada implementação;
//! - o texto é UTF-16 ([`JsString`]), como no JavaScript: colunas e comprimentos contam unidades;
//! - `Number.MAX_SAFE_INTEGER` das colunas vira `i32::MAX`, a maior coluna que o `Range` representa;
//! - `ArrayText extends LineBasedText` não acrescenta estado nem método, então [`ArrayText::new`] devolve um
//!   [`LineBasedText`];
//! - `StringText.getTransformer` (que devolve o transformador criado no construtor) é o caso da célula já
//!   preenchida em [`StringText::new`], sem override.

use std::cell::OnceCell;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::js_string::substring;
use je_base::strings::split_lines;

use crate::line_range::LineRange;
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::position_to_offset::{PositionOffsetTransformer, PositionOffsetTransformerBase, TextLength};
use crate::range::Range;

/// `AbstractText`: um texto que se consulta por intervalo.
pub trait AbstractText {
    fn get_value_of_range(&self, range: &Range) -> JsString;

    /// `length`.
    fn length(&self) -> TextLength;

    /// O `_transformer` da classe base: a célula onde o [`AbstractText::get_transformer`] guarda o resultado.
    fn transformer_cache(&self) -> &OnceCell<PositionOffsetTransformer>;

    /// `endPositionExclusive`.
    fn end_position_exclusive(&self) -> Position {
        self.length().add_to_position(Position::new(1, 1))
    }

    /// `lineRange`.
    fn line_range(&self) -> LineRange {
        self.length().to_line_range()
    }

    fn get_value(&self) -> JsString {
        self.get_value_of_range(&self.length().to_range())
    }

    fn get_value_of_offset_range(&self, range: OffsetRange) -> JsString {
        self.get_value_of_range(&self.get_transformer().get_range(range))
    }

    fn get_line_length(&self, line_number: i32) -> i32 {
        self.get_value_of_range(&Range::new(line_number, 1, line_number, i32::MAX)).len() as i32
    }

    fn get_transformer(&self) -> &PositionOffsetTransformer {
        self.transformer_cache()
            .get_or_init(|| PositionOffsetTransformer::new(&self.get_value().to_string_lossy()))
    }

    fn get_line_at(&self, line_number: i32) -> JsString {
        self.get_value_of_range(&Range::new(line_number, 1, line_number, i32::MAX))
    }

    fn get_lines(&self) -> Vec<JsString> {
        let value = self.get_value();
        split_lines(value.units()).into_iter().map(JsString::from).collect()
    }

    fn get_lines_of_range(&self, range: &LineRange) -> Vec<JsString> {
        range.map_to_line_array(|line_number| self.get_line_at(line_number))
    }

    /// `this === other` vira a identidade do objeto.
    fn equals(&self, other: &dyn AbstractText) -> bool {
        if std::ptr::addr_eq(self, other) {
            return true;
        }
        self.get_value() == other.get_value()
    }
}

/// `LineBasedText`: um texto dado por uma função que devolve o conteúdo de cada linha.
pub struct LineBasedText {
    get_line_content: Box<dyn Fn(i32) -> JsString>,
    line_count: i32,
    transformer: OnceCell<PositionOffsetTransformer>,
}

impl LineBasedText {
    /// O construtor exige pelo menos uma linha.
    pub fn new(get_line_content: Box<dyn Fn(i32) -> JsString>, line_count: i32) -> Self {
        if line_count < 1 {
            errors::throw(Error::bug_indicating(Some("Assertion Failed: unexpected state")));
        }
        Self { get_line_content, line_count, transformer: OnceCell::new() }
    }

    fn line_content(&self, line_number: i32) -> JsString {
        (self.get_line_content)(line_number)
    }
}

impl AbstractText for LineBasedText {
    fn get_value_of_range(&self, range: &Range) -> JsString {
        if range.start_line_number == range.end_line_number {
            let line = self.line_content(range.start_line_number);
            return JsString::from(substring(
                line.units(),
                i64::from(range.start_column - 1),
                i64::from(range.end_column - 1),
            ));
        }
        let first = self.line_content(range.start_line_number);
        let mut result = JsString::from(substring(first.units(), i64::from(range.start_column - 1), first.len() as i64));
        for i in range.start_line_number + 1..range.end_line_number {
            result.push_unit(u16::from(b'\n'));
            result.push_units(self.line_content(i).units());
        }
        result.push_unit(u16::from(b'\n'));
        let last = self.line_content(range.end_line_number);
        result.push_units(substring(last.units(), 0, i64::from(range.end_column - 1)));
        result
    }

    fn get_line_length(&self, line_number: i32) -> i32 {
        self.line_content(line_number).len() as i32
    }

    fn length(&self) -> TextLength {
        let last_line = self.line_content(self.line_count);
        TextLength::new(self.line_count - 1, last_line.len() as i32)
    }

    fn transformer_cache(&self) -> &OnceCell<PositionOffsetTransformer> {
        &self.transformer
    }
}

/// `ArrayText`: um [`LineBasedText`] sobre um vetor de linhas.
pub struct ArrayText;

impl ArrayText {
    pub fn new(lines: Vec<JsString>) -> LineBasedText {
        let line_count = lines.len() as i32;
        LineBasedText::new(Box::new(move |line_number| lines[(line_number - 1) as usize].clone()), line_count)
    }
}

/// `StringText`: um texto guardado numa string só.
pub struct StringText {
    pub value: JsString,
    transformer: OnceCell<PositionOffsetTransformer>,
}

impl StringText {
    pub fn new(value: impl Into<JsString>) -> Self {
        let value = value.into();
        let transformer = OnceCell::new();
        let _ = transformer.set(PositionOffsetTransformer::new(&value.to_string_lossy()));
        Self { value, transformer }
    }

    fn transformer(&self) -> &PositionOffsetTransformer {
        self.transformer.get().expect("preenchido no construtor")
    }
}

impl AbstractText for StringText {
    fn get_value_of_range(&self, range: &Range) -> JsString {
        JsString::from(self.transformer().get_offset_range(range).substring(self.value.units()))
    }

    fn length(&self) -> TextLength {
        self.transformer().text_length()
    }

    fn transformer_cache(&self) -> &OnceCell<PositionOffsetTransformer> {
        &self.transformer
    }
}
