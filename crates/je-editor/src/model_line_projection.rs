//! Porte de `src/vs/editor/common/viewModel/modelLineProjection.ts`: `IModelLineProjection`,
//! `createModelLineProjection`, `ModelLineProjection` (quebra de linha e texto injetado),
//! `IdentityModelLineProjection` (a linha do modelo sem mudança) e `HiddenModelLineProjection` (a linha
//! escondida, por exemplo dentro de uma dobra).
//!
//! Decisões de tradução:
//! - `ISimpleModel` vira o trait [`SimpleModel`], que estende [`CursorSimpleModel`] (`getLineContent`,
//!   `getLineMinColumn` e `getLineMaxColumn` já moram lá) e acrescenta `getLineTokens` (o
//!   `tokenization.getLineTokens`), `getLineLength` e `getValueInRange` (sem o `eol`, que o original nunca
//!   passa). O `TextModel` o atende.
//! - Os singletons `INSTANCE` somem: [`IdentityModelLineProjection`] e [`HiddenModelLineProjection`] são
//!   estruturas unitárias e `createModelLineProjection` devolve `Box<dyn IModelLineProjection>`.
//!   `setVisible` recebe `self: Box<Self>`, porque o original "invalida a instância atual" e pode devolvê-la.
//! - `throw new Error('Not supported')` vira `panic!` (os acessos a uma linha escondida são erro de uso).
//! - `ViewLineData` vem de `view_model_types` (`viewModel.ts`); `startVisibleColumn` é `i32` lá, então o valor
//!   `f64` de `breakOffsetsVisibleColumn` é truncado ao copiar.
//! - `getLineTokensWithInjections` (de `textModel.ts`) mora em `text_model.rs`.
//! - Os contextos do `InjectedTextInlineDecorationsComputer` são uma estrutura com cópias dos dados da
//!   projeção (o original usa closures sobre o mesmo objeto).
//! - `getViewLinesData` escreve em `result[globalIndex]`; como o `Array` do JavaScript cresce sozinho, aqui o
//!   vetor é estendido com `None` até o índice.

use std::rc::Rc;

use je_base::JsString;

use crate::cursor_common::CursorSimpleModel;
use crate::inline_decorations::{
    IInjectedTextInlineDecorationsComputerContext, IInlineDecorationsComputer, InjectedTextInlineDecorationsComputer,
    InlineDecoration,
};
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::model::{EndOfLinePreference, InjectedTextOptions, PositionAffinity};
use crate::model_line_projection_data::{InjectedText, ModelLineProjectionData};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::{TextModel, get_line_tokens_with_injections};
use crate::text_model_events::LineInjectedText;
use crate::view_model_types::ViewLineData;

/// `ISimpleModel`: o que a projeção lê do modelo. O resto (`getLineContent`, `getLineMinColumn`,
/// `getLineMaxColumn`) vem de [`CursorSimpleModel`].
pub trait SimpleModel: CursorSimpleModel {
    /// `tokenization.getLineTokens(lineNumber)`.
    fn get_line_tokens(&self, line_number: i32) -> LineTokens;
    /// `getLineLength(lineNumber)`.
    fn get_line_length(&self, line_number: i32) -> usize;
    /// `getValueInRange(range)`.
    fn get_value_in_range(&self, range: &Range) -> JsString;
}

impl SimpleModel for TextModel {
    fn get_line_tokens(&self, line_number: i32) -> LineTokens {
        self.tokenization().get_line_tokens(line_number)
    }

    fn get_line_length(&self, line_number: i32) -> usize {
        // A coluna máxima é o comprimento mais um (e valida o número da linha).
        (CursorSimpleModel::get_line_max_column(self, line_number) - 1) as usize
    }

    fn get_value_in_range(&self, range: &Range) -> JsString {
        self.get_value_in_range_with(range, EndOfLinePreference::TextDefined)
    }
}

/// `IModelLineProjection`.
pub trait IModelLineProjection {
    fn is_visible(&self) -> bool;

    /// Isto invalida a instância atual (potencialmente a reaproveita e a devolve de novo).
    fn set_visible(self: Box<Self>, is_visible: bool) -> Box<dyn IModelLineProjection>;

    fn get_projection_data(&self) -> Option<&ModelLineProjectionData>;
    fn get_view_line_count(&self) -> usize;
    fn get_view_line_content(&self, model: &dyn SimpleModel, model_line_number: i32, output_line_index: usize) -> JsString;
    fn get_view_line_length(&self, model: &dyn SimpleModel, model_line_number: i32, output_line_index: usize) -> usize;
    fn get_view_line_min_column(&self, model: &dyn SimpleModel, model_line_number: i32, output_line_index: usize) -> i32;
    fn get_view_line_max_column(&self, model: &dyn SimpleModel, model_line_number: i32, output_line_index: usize) -> i32;
    fn get_view_line_continues_with_wrapped_line(&self, output_line_index: usize) -> bool;
    fn get_view_line_data(
        &self,
        model: &dyn SimpleModel,
        model_line_number: i32,
        output_line_index: usize,
        base_view_line_number: i32,
    ) -> ViewLineData;
    #[allow(clippy::too_many_arguments)]
    fn get_view_lines_data(
        &self,
        model: &dyn SimpleModel,
        model_line_number: i32,
        output_line_idx: usize,
        line_count: usize,
        base_view_line_number: i32,
        global_start_index: usize,
        needed: &[bool],
        result: &mut Vec<Option<ViewLineData>>,
    );

    fn get_model_column_of_view_position(&self, output_line_index: usize, output_column: i32) -> i32;
    fn get_view_position_of_model_position(
        &self,
        delta_line_number: i32,
        input_column: i32,
        affinity: Option<PositionAffinity>,
    ) -> Position;
    fn get_view_line_number_of_model_position(&self, delta_line_number: i32, input_column: i32) -> i32;
    fn normalize_position(&self, output_line_index: usize, output_position: Position, affinity: PositionAffinity) -> Position;

    fn get_injected_text_at(&self, output_line_index: usize, column: i32) -> Option<InjectedText>;
}

/// `createModelLineProjection(lineBreakData, isVisible)`.
pub fn create_model_line_projection(
    line_break_data: Option<ModelLineProjectionData>,
    is_visible: bool,
) -> Box<dyn IModelLineProjection> {
    match line_break_data {
        None => {
            // No mapping needed
            if is_visible {
                Box::new(IdentityModelLineProjection)
            } else {
                Box::new(HiddenModelLineProjection)
            }
        }
        Some(line_break_data) => Box::new(ModelLineProjection::new(line_break_data, is_visible)),
    }
}

/// O `spaces(count)` do original (a versão dele guarda um cache de cadeias).
fn spaces(count: usize) -> JsString {
    JsString::from_units(vec![u16::from(b' '); count])
}

/// `spaces(indentLength) + text`.
fn with_wrapped_indent(indent_length: usize, text: &JsString) -> JsString {
    let mut result = spaces(indent_length);
    result.push_units(text.units());
    result
}

/// Os deslocamentos e as opções das injeções, quando há injeções (`injectionOffsets !== null`).
fn injections(data: &ModelLineProjectionData) -> Option<(&[usize], &[InjectedTextOptions])> {
    match (&data.injection_offsets, &data.injection_options) {
        (Some(offsets), Some(options)) => Some((offsets, options)),
        _ => None,
    }
}

/// Escreve `value` em `result[index]`, estendendo o vetor com `None` (o `Array` do JavaScript cresce sozinho).
fn set_result(result: &mut Vec<Option<ViewLineData>>, index: usize, value: Option<ViewLineData>) {
    if result.len() <= index {
        result.resize_with(index + 1, || None);
    }
    result[index] = value;
}

/// `IInjectedTextInlineDecorationsComputerContext` sobre uma cópia dos dados da projeção. O original ignora o
/// número de linha do modelo, porque o contexto é de uma linha só.
struct ProjectionInlineDecorationsContext {
    injection_options: Option<Vec<InjectedTextOptions>>,
    injection_offsets: Option<Vec<i32>>,
    break_offsets: Vec<i32>,
    wrapped_text_indent_length: i32,
    base_view_line_number: i32,
}

impl ProjectionInlineDecorationsContext {
    fn new(data: &ModelLineProjectionData, base_view_line_number: i32) -> Self {
        Self {
            injection_options: data.injection_options.clone(),
            injection_offsets: data.injection_offsets.as_ref().map(|offsets| offsets.iter().map(|offset| *offset as i32).collect()),
            break_offsets: data.break_offsets.iter().map(|offset| *offset as i32).collect(),
            wrapped_text_indent_length: data.wrapped_text_indent_length as i32,
            base_view_line_number,
        }
    }
}

impl IInjectedTextInlineDecorationsComputerContext for ProjectionInlineDecorationsContext {
    fn get_injection_options(&self, _model_line_number: i32) -> Option<Vec<InjectedTextOptions>> {
        self.injection_options.clone()
    }

    fn get_injection_offsets(&self, _model_line_number: i32) -> Option<Vec<i32>> {
        self.injection_offsets.clone()
    }

    fn get_break_offsets(&self, _model_line_number: i32) -> Vec<i32> {
        self.break_offsets.clone()
    }

    fn get_wrapped_text_indent_length(&self, _model_line_number: i32) -> i32 {
        self.wrapped_text_indent_length
    }

    fn get_base_view_line_number(&self, _model_line_number: i32) -> i32 {
        self.base_view_line_number
    }
}

/// Esta projeção é usada para
/// * quebrar as linhas do modelo
/// * injetar texto
pub struct ModelLineProjection {
    projection_data: ModelLineProjectionData,
    is_visible: bool,
}

impl ModelLineProjection {
    pub fn new(line_break_data: ModelLineProjectionData, is_visible: bool) -> Self {
        Self { projection_data: line_break_data, is_visible }
    }

    /// Os deslocamentos, no texto com as injeções, onde a linha de saída começa e termina.
    fn input_range_with_injections(&self, output_line_index: usize) -> (usize, usize) {
        let start = if output_line_index > 0 { self.projection_data.break_offsets[output_line_index - 1] } else { 0 };
        (start, self.projection_data.break_offsets[output_line_index])
    }

    fn assert_visible(&self) {
        if !self.is_visible {
            panic!("Not supported");
        }
    }

    /// `_getViewLineData`.
    fn view_line_data_of(
        &self,
        line_with_injections: &LineTokens,
        inline_decorations: Option<Vec<InlineDecoration>>,
        output_line_index: usize,
    ) -> ViewLineData {
        self.assert_visible();
        let line_break_data = &self.projection_data;
        let delta_start_index = if output_line_index > 0 { line_break_data.wrapped_text_indent_length as i32 } else { 0 };

        let (line_start_offset_in_input_with_injections, line_end_offset_in_input_with_injections) =
            self.input_range_with_injections(output_line_index);
        let tokens = line_with_injections.slice_and_inflate(
            line_start_offset_in_input_with_injections as i32,
            line_end_offset_in_input_with_injections as i32,
            delta_start_index,
        );

        let mut line_content = tokens.get_line_content();
        if output_line_index > 0 {
            line_content = with_wrapped_indent(line_break_data.wrapped_text_indent_length, &line_content);
        }

        let min_column = line_break_data.get_min_output_offset(output_line_index) as i32 + 1;
        let max_column = line_content.len() as i32 + 1;
        let continues_with_wrapped_line = self.get_view_line_continues_with_wrapped_line(output_line_index);
        let start_visible_column =
            if output_line_index == 0 { 0 } else { line_break_data.break_offsets_visible_column[output_line_index - 1] as i32 };

        ViewLineData::new(
            line_content,
            continues_with_wrapped_line,
            min_column,
            max_column,
            start_visible_column,
            Rc::new(tokens),
            inline_decorations,
        )
    }
}

impl IModelLineProjection for ModelLineProjection {
    fn is_visible(&self) -> bool {
        self.is_visible
    }

    fn set_visible(mut self: Box<Self>, is_visible: bool) -> Box<dyn IModelLineProjection> {
        self.is_visible = is_visible;
        self
    }

    fn get_projection_data(&self) -> Option<&ModelLineProjectionData> {
        Some(&self.projection_data)
    }

    fn get_view_line_count(&self) -> usize {
        if !self.is_visible {
            return 0;
        }
        self.projection_data.get_output_line_count()
    }

    fn get_view_line_content(&self, model: &dyn SimpleModel, model_line_number: i32, output_line_index: usize) -> JsString {
        self.assert_visible();

        let (start_offset_in_input_with_injections, end_offset_in_input_with_injections) =
            self.input_range_with_injections(output_line_index);

        let mut r = match injections(&self.projection_data) {
            Some((injection_offsets, injection_options)) => {
                let injected_texts: Vec<LineInjectedText> = injection_offsets
                    .iter()
                    .zip(injection_options)
                    .map(|(offset, options)| LineInjectedText {
                        owner_id: 0,
                        line_number: 0,
                        column: *offset as i32 + 1,
                        options: options.clone(),
                        order: 0,
                    })
                    .collect();
                let line_with_injections =
                    LineInjectedText::apply_injected_text(&model.get_line_content(model_line_number), Some(&injected_texts));
                // `substring` do JavaScript limita os índices ao comprimento do texto.
                let end = end_offset_in_input_with_injections.min(line_with_injections.len());
                line_with_injections.slice(start_offset_in_input_with_injections.min(end), end)
            }
            None => model.get_value_in_range(&Range::new(
                model_line_number,
                start_offset_in_input_with_injections as i32 + 1,
                model_line_number,
                end_offset_in_input_with_injections as i32 + 1,
            )),
        };

        if output_line_index > 0 {
            r = with_wrapped_indent(self.projection_data.wrapped_text_indent_length, &r);
        }

        r
    }

    fn get_view_line_length(&self, _model: &dyn SimpleModel, _model_line_number: i32, output_line_index: usize) -> usize {
        self.assert_visible();
        self.projection_data.get_line_length(output_line_index)
    }

    fn get_view_line_min_column(&self, _model: &dyn SimpleModel, _model_line_number: i32, output_line_index: usize) -> i32 {
        self.assert_visible();
        self.projection_data.get_min_output_offset(output_line_index) as i32 + 1
    }

    fn get_view_line_max_column(&self, _model: &dyn SimpleModel, _model_line_number: i32, output_line_index: usize) -> i32 {
        self.assert_visible();
        self.projection_data.get_max_output_offset(output_line_index) as i32 + 1
    }

    fn get_view_line_continues_with_wrapped_line(&self, output_line_index: usize) -> bool {
        output_line_index + 1 < self.get_view_line_count()
    }

    /// Prefira [`IModelLineProjection::get_view_lines_data`].
    fn get_view_line_data(
        &self,
        model: &dyn SimpleModel,
        model_line_number: i32,
        output_line_index: usize,
        base_view_line_number: i32,
    ) -> ViewLineData {
        let mut arr: Vec<Option<ViewLineData>> = Vec::new();
        self.get_view_lines_data(model, model_line_number, output_line_index, 1, base_view_line_number, 0, &[true], &mut arr);
        arr.swap_remove(0).expect("a linha pedida está marcada como necessária")
    }

    fn get_view_lines_data(
        &self,
        model: &dyn SimpleModel,
        model_line_number: i32,
        output_line_idx: usize,
        line_count: usize,
        base_view_line_number: i32,
        global_start_index: usize,
        needed: &[bool],
        result: &mut Vec<Option<ViewLineData>>,
    ) {
        self.assert_visible();

        let context = Rc::new(ProjectionInlineDecorationsContext::new(&self.projection_data, base_view_line_number));
        let computer = InjectedTextInlineDecorationsComputer::new(context);
        let line_inline_decorations = computer.get_inline_decorations(model_line_number);
        let line_tokens = model.get_line_tokens(model_line_number);
        let injection_offsets: Option<Vec<i32>> =
            self.projection_data.injection_offsets.as_ref().map(|offsets| offsets.iter().map(|offset| *offset as i32).collect());
        let line_with_injections = get_line_tokens_with_injections(
            &line_tokens,
            self.projection_data.injection_options.as_deref(),
            injection_offsets.as_deref(),
        );

        for output_line_index in output_line_idx..output_line_idx + line_count {
            let global_index = global_start_index + output_line_index - output_line_idx;
            if !needed.get(global_index).copied().unwrap_or(false) {
                set_result(result, global_index, None);
                continue;
            }
            // Sem texto injetado o computador devolve uma lista vazia, que é o `null` do original.
            let view_line_data = self.view_line_data_of(
                &line_with_injections,
                line_inline_decorations.get(output_line_index).cloned(),
                output_line_index,
            );
            set_result(result, global_index, Some(view_line_data));
        }
    }

    fn get_model_column_of_view_position(&self, output_line_index: usize, output_column: i32) -> i32 {
        self.assert_visible();
        self.projection_data.translate_to_input_offset(output_line_index, (output_column - 1) as usize) as i32 + 1
    }

    fn get_view_position_of_model_position(
        &self,
        delta_line_number: i32,
        input_column: i32,
        affinity: Option<PositionAffinity>,
    ) -> Position {
        self.assert_visible();
        let r = self
            .projection_data
            .translate_to_output_position((input_column - 1) as usize, affinity.unwrap_or(PositionAffinity::None));
        r.to_position(delta_line_number)
    }

    fn get_view_line_number_of_model_position(&self, delta_line_number: i32, input_column: i32) -> i32 {
        self.assert_visible();
        let r = self.projection_data.translate_to_output_position((input_column - 1) as usize, PositionAffinity::None);
        delta_line_number + r.output_line_index as i32
    }

    fn normalize_position(&self, output_line_index: usize, output_position: Position, affinity: PositionAffinity) -> Position {
        let base_view_line_number = output_position.line_number - output_line_index as i32;
        let normalized_output_position =
            self.projection_data.normalize_output_position(output_line_index, (output_position.column - 1) as usize, affinity);
        normalized_output_position.to_position(base_view_line_number)
    }

    fn get_injected_text_at(&self, output_line_index: usize, output_column: i32) -> Option<InjectedText> {
        self.projection_data.get_injected_text(output_line_index, (output_column - 1) as usize)
    }
}

/// Esta projeção não muda a linha do modelo.
pub struct IdentityModelLineProjection;

impl IModelLineProjection for IdentityModelLineProjection {
    fn is_visible(&self) -> bool {
        true
    }

    fn set_visible(self: Box<Self>, is_visible: bool) -> Box<dyn IModelLineProjection> {
        if is_visible {
            return self;
        }
        Box::new(HiddenModelLineProjection)
    }

    fn get_projection_data(&self) -> Option<&ModelLineProjectionData> {
        None
    }

    fn get_view_line_count(&self) -> usize {
        1
    }

    fn get_view_line_content(&self, model: &dyn SimpleModel, model_line_number: i32, _output_line_index: usize) -> JsString {
        model.get_line_content(model_line_number)
    }

    fn get_view_line_length(&self, model: &dyn SimpleModel, model_line_number: i32, _output_line_index: usize) -> usize {
        model.get_line_length(model_line_number)
    }

    fn get_view_line_min_column(&self, model: &dyn SimpleModel, model_line_number: i32, _output_line_index: usize) -> i32 {
        model.get_line_min_column(model_line_number)
    }

    fn get_view_line_max_column(&self, model: &dyn SimpleModel, model_line_number: i32, _output_line_index: usize) -> i32 {
        model.get_line_max_column(model_line_number)
    }

    fn get_view_line_continues_with_wrapped_line(&self, _output_line_index: usize) -> bool {
        false
    }

    fn get_view_line_data(
        &self,
        model: &dyn SimpleModel,
        model_line_number: i32,
        _output_line_index: usize,
        _base_view_line_number: i32,
    ) -> ViewLineData {
        let line_tokens = model.get_line_tokens(model_line_number);
        let line_content = line_tokens.get_line_content();
        let max_column = line_content.len() as i32 + 1;
        ViewLineData::new(
            line_content,
            false,
            1,
            max_column,
            0,
            Rc::new(line_tokens.inflate()),
            None,
        )
    }

    fn get_view_lines_data(
        &self,
        model: &dyn SimpleModel,
        model_line_number: i32,
        _output_line_idx: usize,
        _line_count: usize,
        base_view_line_number: i32,
        global_start_index: usize,
        needed: &[bool],
        result: &mut Vec<Option<ViewLineData>>,
    ) {
        if !needed.get(global_start_index).copied().unwrap_or(false) {
            set_result(result, global_start_index, None);
            return;
        }
        let view_line_data = self.get_view_line_data(model, model_line_number, 0, base_view_line_number);
        set_result(result, global_start_index, Some(view_line_data));
    }

    fn get_model_column_of_view_position(&self, _output_line_index: usize, output_column: i32) -> i32 {
        output_column
    }

    fn get_view_position_of_model_position(
        &self,
        delta_line_number: i32,
        input_column: i32,
        _affinity: Option<PositionAffinity>,
    ) -> Position {
        Position::new(delta_line_number, input_column)
    }

    fn get_view_line_number_of_model_position(&self, delta_line_number: i32, _input_column: i32) -> i32 {
        delta_line_number
    }

    fn normalize_position(&self, _output_line_index: usize, output_position: Position, _affinity: PositionAffinity) -> Position {
        output_position
    }

    fn get_injected_text_at(&self, _output_line_index: usize, _output_column: i32) -> Option<InjectedText> {
        None
    }
}

/// Esta projeção esconde a linha do modelo.
pub struct HiddenModelLineProjection;

impl IModelLineProjection for HiddenModelLineProjection {
    fn is_visible(&self) -> bool {
        false
    }

    fn set_visible(self: Box<Self>, is_visible: bool) -> Box<dyn IModelLineProjection> {
        if !is_visible {
            return self;
        }
        Box::new(IdentityModelLineProjection)
    }

    fn get_projection_data(&self) -> Option<&ModelLineProjectionData> {
        None
    }

    fn get_view_line_count(&self) -> usize {
        0
    }

    fn get_view_line_content(&self, _model: &dyn SimpleModel, _model_line_number: i32, _output_line_index: usize) -> JsString {
        panic!("Not supported");
    }

    fn get_view_line_length(&self, _model: &dyn SimpleModel, _model_line_number: i32, _output_line_index: usize) -> usize {
        panic!("Not supported");
    }

    fn get_view_line_min_column(&self, _model: &dyn SimpleModel, _model_line_number: i32, _output_line_index: usize) -> i32 {
        panic!("Not supported");
    }

    fn get_view_line_max_column(&self, _model: &dyn SimpleModel, _model_line_number: i32, _output_line_index: usize) -> i32 {
        panic!("Not supported");
    }

    fn get_view_line_continues_with_wrapped_line(&self, _output_line_index: usize) -> bool {
        panic!("Not supported");
    }

    fn get_view_line_data(
        &self,
        _model: &dyn SimpleModel,
        _model_line_number: i32,
        _output_line_index: usize,
        _base_view_line_number: i32,
    ) -> ViewLineData {
        panic!("Not supported");
    }

    fn get_view_lines_data(
        &self,
        _model: &dyn SimpleModel,
        _model_line_number: i32,
        _output_line_idx: usize,
        _line_count: usize,
        _base_view_line_number: i32,
        _global_start_index: usize,
        _needed: &[bool],
        _result: &mut Vec<Option<ViewLineData>>,
    ) {
        panic!("Not supported");
    }

    fn get_model_column_of_view_position(&self, _output_line_index: usize, _output_column: i32) -> i32 {
        panic!("Not supported");
    }

    fn get_view_position_of_model_position(
        &self,
        _delta_line_number: i32,
        _input_column: i32,
        _affinity: Option<PositionAffinity>,
    ) -> Position {
        panic!("Not supported");
    }

    fn get_view_line_number_of_model_position(&self, _delta_line_number: i32, _input_column: i32) -> i32 {
        panic!("Not supported");
    }

    fn normalize_position(&self, _output_line_index: usize, _output_position: Position, _affinity: PositionAffinity) -> Position {
        panic!("Not supported");
    }

    fn get_injected_text_at(&self, _output_line_index: usize, _output_column: i32) -> Option<InjectedText> {
        panic!("Not supported");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TextModelCreationOptions;
    use crate::text_model::TextSource;
    use crate::undo_redo_service::UndoRedoService;

    /// `createTextModel(text)`: as opções padrão, sem linguagem e sem URI (como `tests/support/test_text_model.rs`).
    fn create_model(text: &str) -> Rc<TextModel> {
        let defaults = TextModel::default_creation_options();
        let options = TextModelCreationOptions {
            tab_size: defaults.tab_size,
            indent_size: defaults.indent_size,
            insert_spaces: defaults.insert_spaces,
            detect_indentation: defaults.detect_indentation,
            trim_auto_whitespace: defaults.trim_auto_whitespace,
            default_eol: defaults.default_eol,
            is_for_simple_widget: defaults.is_for_simple_widget,
            large_file_optimizations: defaults.large_file_optimizations,
            bracket_pair_colorization_options: defaults.bracket_pair_colorization_options,
        };
        TextModel::new(TextSource::from(text), "plaintext", options, None, UndoRedoService::for_tests())
    }

    fn injected(content: &str, inline_class_name: Option<&str>) -> InjectedTextOptions {
        InjectedTextOptions {
            content: content.to_owned(),
            tokens: None,
            inline_class_name: inline_class_name.map(str::to_owned),
            inline_class_name_affects_letter_spacing: false,
            width_in_em: None,
            cursor_stops: None,
            ..InjectedTextOptions::default()
        }
    }

    /// "abcdefghij" quebrada em "abcd" / "efgh" / "ij", com indentação de 2 nas linhas quebradas.
    fn wrapped_data() -> ModelLineProjectionData {
        ModelLineProjectionData::new(None, None, vec![4, 8, 10], vec![4.0, 8.0, 10.0], 2)
    }

    /// "abcdefghij" com "XY" injetado no deslocamento 2 ("abXYcdefghij"), quebrada em "abXYcd" / "efghij".
    fn injected_data(inline_class_name: Option<&str>) -> ModelLineProjectionData {
        ModelLineProjectionData::new(
            Some(vec![2]),
            Some(vec![injected("XY", inline_class_name)]),
            vec![6, 12],
            vec![6.0, 12.0],
            0,
        )
    }

    fn text(value: JsString) -> String {
        value.to_string_lossy()
    }

    #[test]
    fn wrapped_view_line_count_and_visibility() {
        let projection = create_model_line_projection(Some(wrapped_data()), true);
        assert!(projection.is_visible());
        assert_eq!(projection.get_view_line_count(), 3);
        assert!(projection.get_projection_data().is_some());

        let hidden = projection.set_visible(false);
        assert!(!hidden.is_visible());
        assert_eq!(hidden.get_view_line_count(), 0);

        let visible = hidden.set_visible(true);
        assert_eq!(visible.get_view_line_count(), 3);
    }

    #[test]
    fn wrapped_view_line_content_length_and_columns() {
        let model = create_model("abcdefghij");
        let projection = create_model_line_projection(Some(wrapped_data()), true);
        let model_ref: &dyn SimpleModel = &*model;

        assert_eq!(text(projection.get_view_line_content(model_ref, 1, 0)), "abcd");
        assert_eq!(text(projection.get_view_line_content(model_ref, 1, 1)), "  efgh");
        assert_eq!(text(projection.get_view_line_content(model_ref, 1, 2)), "  ij");

        assert_eq!(projection.get_view_line_length(model_ref, 1, 0), 4);
        assert_eq!(projection.get_view_line_length(model_ref, 1, 1), 6);
        assert_eq!(projection.get_view_line_length(model_ref, 1, 2), 4);

        assert_eq!(projection.get_view_line_min_column(model_ref, 1, 0), 1);
        assert_eq!(projection.get_view_line_min_column(model_ref, 1, 1), 3);
        assert_eq!(projection.get_view_line_max_column(model_ref, 1, 0), 5);
        assert_eq!(projection.get_view_line_max_column(model_ref, 1, 1), 7);
        assert_eq!(projection.get_view_line_max_column(model_ref, 1, 2), 5);

        assert!(projection.get_view_line_continues_with_wrapped_line(0));
        assert!(projection.get_view_line_continues_with_wrapped_line(1));
        assert!(!projection.get_view_line_continues_with_wrapped_line(2));
        model.dispose();
    }

    #[test]
    fn wrapped_model_column_of_view_position() {
        let projection = create_model_line_projection(Some(wrapped_data()), true);
        assert_eq!(projection.get_model_column_of_view_position(0, 3), 3);
        // Antes da indentação, a coluna cai no começo do trecho (deslocamento 4).
        assert_eq!(projection.get_model_column_of_view_position(1, 1), 5);
        assert_eq!(projection.get_model_column_of_view_position(1, 3), 5);
        assert_eq!(projection.get_model_column_of_view_position(1, 4), 6);
        assert_eq!(projection.get_model_column_of_view_position(2, 5), 11);
    }

    #[test]
    fn wrapped_view_position_of_model_position() {
        let projection = create_model_line_projection(Some(wrapped_data()), true);
        // O deslocamento 3 fica na primeira linha de saída.
        assert_eq!(projection.get_view_position_of_model_position(10, 4, None), Position::new(10, 4));
        // O deslocamento 4 é o limite: sem afinidade vai para a linha seguinte, depois da indentação.
        assert_eq!(projection.get_view_position_of_model_position(10, 5, None), Position::new(11, 3));
        // Com afinidade à esquerda fica no fim da linha anterior.
        assert_eq!(
            projection.get_view_position_of_model_position(10, 5, Some(PositionAffinity::Left)),
            Position::new(10, 5)
        );
        // O fim da linha do modelo (deslocamento 10) fica no fim da última linha de saída.
        assert_eq!(projection.get_view_position_of_model_position(10, 11, None), Position::new(12, 5));

        assert_eq!(projection.get_view_line_number_of_model_position(10, 4), 10);
        assert_eq!(projection.get_view_line_number_of_model_position(10, 5), 11);
        assert_eq!(projection.get_view_line_number_of_model_position(10, 11), 12);
    }

    #[test]
    fn wrapped_normalize_position() {
        let projection = create_model_line_projection(Some(wrapped_data()), true);
        assert_eq!(projection.normalize_position(1, Position::new(11, 3), PositionAffinity::Left), Position::new(10, 5));
        assert_eq!(projection.normalize_position(0, Position::new(10, 5), PositionAffinity::Right), Position::new(11, 3));
        assert_eq!(projection.normalize_position(1, Position::new(11, 4), PositionAffinity::Left), Position::new(11, 4));
    }

    #[test]
    fn wrapped_view_line_data() {
        let model = create_model("abcdefghij");
        let projection = create_model_line_projection(Some(wrapped_data()), true);
        let model_ref: &dyn SimpleModel = &*model;

        let first = projection.get_view_line_data(model_ref, 1, 0, 5);
        assert_eq!(text(first.content.clone()), "abcd");
        assert!(first.continues_with_wrapped_line);
        assert_eq!((first.min_column, first.max_column), (1, 5));
        assert_eq!(first.start_visible_column, 0);
        assert_eq!(first.inline_decorations, None);

        let second = projection.get_view_line_data(model_ref, 1, 1, 5);
        assert_eq!(text(second.content.clone()), "  efgh");
        assert!(second.continues_with_wrapped_line);
        assert_eq!((second.min_column, second.max_column), (3, 7));
        assert_eq!(second.start_visible_column, 4);

        let third = projection.get_view_line_data(model_ref, 1, 2, 5);
        assert_eq!(text(third.content.clone()), "  ij");
        assert!(!third.continues_with_wrapped_line);
        assert_eq!((third.min_column, third.max_column), (3, 5));
        assert_eq!(third.start_visible_column, 8);
        model.dispose();
    }

    #[test]
    fn wrapped_view_lines_data_respects_needed() {
        let model = create_model("abcdefghij");
        let projection = create_model_line_projection(Some(wrapped_data()), true);
        let model_ref: &dyn SimpleModel = &*model;

        let mut result: Vec<Option<ViewLineData>> = Vec::new();
        projection.get_view_lines_data(model_ref, 1, 0, 3, 1, 4, &[false, false, false, false, true, false, true], &mut result);
        assert_eq!(result.len(), 7);
        assert!(result[4].is_some());
        assert!(result[5].is_none());
        assert_eq!(text(result[6].as_ref().expect("a linha 6 é necessária").content.clone()), "  ij");
        assert!(result[0].is_none());
        model.dispose();
    }

    #[test]
    fn injected_text_view_line_content_and_columns() {
        let model = create_model("abcdefghij");
        let projection = create_model_line_projection(Some(injected_data(None)), true);
        let model_ref: &dyn SimpleModel = &*model;

        assert_eq!(text(projection.get_view_line_content(model_ref, 1, 0)), "abXYcd");
        assert_eq!(text(projection.get_view_line_content(model_ref, 1, 1)), "efghij");
        assert_eq!(projection.get_view_line_length(model_ref, 1, 0), 6);

        // A coluna dentro do texto injetado cai no ponto da injeção.
        assert_eq!(projection.get_model_column_of_view_position(0, 4), 3);
        assert_eq!(projection.get_model_column_of_view_position(1, 1), 5);

        assert!(projection.get_injected_text_at(0, 4).is_some());
        assert!(projection.get_injected_text_at(1, 4).is_none());
        model.dispose();
    }

    #[test]
    fn injected_text_view_line_data_has_tokens_and_decorations() {
        let model = create_model("abcdefghij");
        let projection = create_model_line_projection(Some(injected_data(Some("cls"))), true);
        let model_ref: &dyn SimpleModel = &*model;

        let first = projection.get_view_line_data(model_ref, 1, 0, 7);
        assert_eq!(text(first.content.clone()), "abXYcd");
        assert_eq!(first.tokens.get_count(), 3);
        assert_eq!(
            first.inline_decorations,
            Some(vec![InlineDecoration::new(
                Range::new(7, 3, 7, 5),
                "cls",
                crate::inline_decorations::InlineDecorationType::Regular
            )])
        );

        let second = projection.get_view_line_data(model_ref, 1, 1, 7);
        assert_eq!(text(second.content.clone()), "efghij");
        assert_eq!(second.tokens.get_count(), 1);
        assert_eq!(second.inline_decorations, Some(Vec::new()));
        model.dispose();
    }

    #[test]
    fn identity_projection_passes_the_model_line_through() {
        let model = create_model("abc\nde");
        let projection = create_model_line_projection(None, true);
        let model_ref: &dyn SimpleModel = &*model;

        assert!(projection.is_visible());
        assert!(projection.get_projection_data().is_none());
        assert_eq!(projection.get_view_line_count(), 1);
        assert_eq!(text(projection.get_view_line_content(model_ref, 2, 0)), "de");
        assert_eq!(projection.get_view_line_length(model_ref, 2, 0), 2);
        assert_eq!(projection.get_view_line_min_column(model_ref, 2, 0), 1);
        assert_eq!(projection.get_view_line_max_column(model_ref, 2, 0), 3);
        assert!(!projection.get_view_line_continues_with_wrapped_line(0));
        assert_eq!(projection.get_model_column_of_view_position(0, 2), 2);
        assert_eq!(projection.get_view_position_of_model_position(7, 2, None), Position::new(7, 2));
        assert_eq!(projection.get_view_line_number_of_model_position(7, 2), 7);
        assert_eq!(projection.normalize_position(0, Position::new(7, 2), PositionAffinity::Left), Position::new(7, 2));
        assert!(projection.get_injected_text_at(0, 1).is_none());

        let data = projection.get_view_line_data(model_ref, 1, 0, 1);
        assert_eq!(text(data.content.clone()), "abc");
        assert!(!data.continues_with_wrapped_line);
        assert_eq!((data.min_column, data.max_column), (1, 4));
        assert_eq!(data.start_visible_column, 0);
        assert_eq!(data.inline_decorations, None);

        let mut result: Vec<Option<ViewLineData>> = Vec::new();
        projection.get_view_lines_data(model_ref, 2, 0, 1, 2, 3, &[false, false, false, true], &mut result);
        assert_eq!(text(result[3].as_ref().expect("a linha 3 é necessária").content.clone()), "de");
        model.dispose();
    }

    #[test]
    fn visibility_toggles_between_identity_and_hidden() {
        let hidden = create_model_line_projection(None, false);
        assert!(!hidden.is_visible());
        assert_eq!(hidden.get_view_line_count(), 0);
        assert!(hidden.get_projection_data().is_none());

        let identity = hidden.set_visible(true);
        assert!(identity.is_visible());
        assert_eq!(identity.get_view_line_count(), 1);

        let hidden_again = identity.set_visible(false);
        assert!(!hidden_again.is_visible());
    }

    #[test]
    #[should_panic(expected = "Not supported")]
    fn hidden_projection_rejects_view_queries() {
        let hidden = create_model_line_projection(None, false);
        hidden.get_model_column_of_view_position(0, 1);
    }

    #[test]
    #[should_panic(expected = "Not supported")]
    fn invisible_wrapped_projection_rejects_view_queries() {
        let projection = create_model_line_projection(Some(wrapped_data()), false);
        projection.get_view_line_number_of_model_position(1, 1);
    }
}
