//! Porte de `src/vs/editor/common/viewModel/inlineDecorations.ts`: `InlineDecorationType`,
//! `InlineDecoration`, `IViewDecorationsCollection`, `IInlineDecorationsComputer`,
//! `InlineModelDecorationsComputer` e `InjectedTextInlineDecorationsComputer`.
//!
//! `ViewModelDecoration` e `isModelDecorationVisible` vêm de `view_model_types` (porte de
//! `viewModelDecoration.ts`); `ICoordinatesConverter`, de `coordinates_converter`.
//!
//! Representação: os contextos (`IInlineModelDecorationsComputerContext` e
//! `IInjectedTextInlineDecorationsComputerContext`) são traits compartilhadas por `Rc`. O cache de
//! decorações da visão fica num `RefCell`, porque `getDecorations` o muda por `&self`. O original
//! compartilha a mesma `InlineDecoration` entre as linhas que ela cobre; aqui cada linha recebe uma cópia
//! do valor.
// TODO(integração): unificar com view_model_types quando o viewModel for portado.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;

use crate::coordinates_converter::ICoordinatesConverter;
use crate::model::{InjectedTextOptions, ModelDecoration, PositionAffinity};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::view_model_types::{ViewModelDecoration, is_model_decoration_visible};

/// `InlineDecorationType` (enum constante no original; os valores numéricos indexam a ordem de
/// `LineDecoration::compare`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InlineDecorationType {
    Regular = 0,
    Before = 1,
    After = 2,
    RegularAffectingLetterSpacing = 3,
    /// Decoração que não cobre nenhum caractere e mesmo assim é renderizada como elemento próprio.
    /// Serve para texto injetado que só reserva espaço horizontal (por exemplo
    /// `{ content: '', widthInEm: 1 }`): não há caractere a decorar, então a largura vem da classe da
    /// decoração. Diferente de `Before`/`After`, não é um pseudo-elemento preso a uma decoração vizinha,
    /// e sim o próprio conteúdo injetado.
    WidthOnly = 4,
}

/// Uma decoração em linha sobre um intervalo da visão.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineDecoration {
    pub range: Range,
    pub inline_class_name: JsString,
    pub r#type: InlineDecorationType,
}

impl InlineDecoration {
    pub fn new(range: Range, inline_class_name: impl Into<JsString>, r#type: InlineDecorationType) -> Self {
        Self {
            range,
            inline_class_name: inline_class_name.into(),
            r#type,
        }
    }
}

/// `IViewDecorationsCollection`: uma coleção de decorações num intervalo de linhas.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewDecorationsCollection {
    /// Decorações no intervalo de linhas (sem agrupar).
    pub decorations: Vec<Rc<ViewModelDecoration>>,
    /// Decorações em linha (agrupadas por cada linha do intervalo).
    pub inline_decorations: Vec<Vec<InlineDecoration>>,
    /// Se as decorações afetam as fontes.
    pub has_variable_fonts: Vec<bool>,
}

/// `IInlineDecorationsComputer`.
pub trait IInlineDecorationsComputer {
    /// As decorações em linha de um número de linha do modelo, separadas por número de linha da visão.
    fn get_inline_decorations(&self, model_line_number: i32) -> Vec<Vec<InlineDecoration>>;
}

/// `IInlineModelDecorationsComputerContext`.
pub trait IInlineModelDecorationsComputerContext {
    /// As decorações do modelo para um intervalo da visão.
    fn get_model_decorations(&self, view_range: &Range, only_minimap_decorations: bool, only_margin_decorations: bool) -> Vec<ModelDecoration>;
}

/// `InlineModelDecorationsComputer`.
pub struct InlineModelDecorationsComputer {
    context: Rc<dyn IInlineModelDecorationsComputerContext>,
    model: Rc<TextModel>,
    coordinates_converter: Rc<dyn ICoordinatesConverter>,
    decorations_cache: RefCell<HashMap<String, Rc<ViewModelDecoration>>>,
}

impl InlineModelDecorationsComputer {
    pub fn new(
        context: Rc<dyn IInlineModelDecorationsComputerContext>,
        model: Rc<TextModel>,
        coordinates_converter: Rc<dyn ICoordinatesConverter>,
    ) -> Self {
        Self {
            context,
            model,
            coordinates_converter,
            decorations_cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn get_decorations(&self, view_range: Range, only_minimap_decorations: bool, only_margin_decorations: bool) -> ViewDecorationsCollection {
        let model_decorations = self.context.get_model_decorations(&view_range, only_minimap_decorations, only_margin_decorations);
        let start_line_number = view_range.start_line_number;
        let end_line_number = view_range.end_line_number;

        let mut decorations_in_viewport: Vec<Rc<ViewModelDecoration>> = Vec::new();
        let line_count = (end_line_number - start_line_number + 1).max(0) as usize;
        let mut inline_decorations: Vec<Vec<InlineDecoration>> = vec![Vec::new(); line_count];
        let mut has_variable_fonts: Vec<bool> = vec![false; line_count];
        let index_of = |line_number: i32| (line_number - start_line_number) as usize;

        for model_decoration in &model_decorations {
            let decoration_options = &model_decoration.options;

            if !is_model_decoration_visible(&self.model, model_decoration) {
                continue;
            }

            let view_model_decoration = self.get_or_create_view_model_decoration(model_decoration);
            let view_range = view_model_decoration.range;

            decorations_in_viewport.push(view_model_decoration);

            if let Some(inline_class_name) = decoration_options.inline_class_name.as_ref().filter(|name| !name.is_empty()) {
                let inline_decoration = InlineDecoration::new(
                    view_range,
                    inline_class_name.clone(),
                    if decoration_options.inline_class_name_affects_letter_spacing {
                        InlineDecorationType::RegularAffectingLetterSpacing
                    } else {
                        InlineDecorationType::Regular
                    },
                );
                let intersected_start_line_number = start_line_number.max(view_range.start_line_number);
                let intersected_end_line_number = end_line_number.min(view_range.end_line_number);
                for j in intersected_start_line_number..=intersected_end_line_number {
                    inline_decorations[index_of(j)].push(inline_decoration.clone());
                    if decoration_options.affects_font.unwrap_or(false) {
                        has_variable_fonts[index_of(j)] = true;
                    }
                }
            }
            if let Some(before_content_class_name) = decoration_options.before_content_class_name.as_ref().filter(|name| !name.is_empty())
                && start_line_number <= view_range.start_line_number
                && view_range.start_line_number <= end_line_number
            {
                let inline_decoration = InlineDecoration::new(
                    Range::new(view_range.start_line_number, view_range.start_column, view_range.start_line_number, view_range.start_column),
                    before_content_class_name.clone(),
                    InlineDecorationType::Before,
                );
                inline_decorations[index_of(view_range.start_line_number)].push(inline_decoration);
                if decoration_options.affects_font.unwrap_or(false) {
                    has_variable_fonts[index_of(view_range.start_line_number)] = true;
                }
            }
            if let Some(after_content_class_name) = decoration_options.after_content_class_name.as_ref().filter(|name| !name.is_empty())
                && start_line_number <= view_range.end_line_number
                && view_range.end_line_number <= end_line_number
            {
                let inline_decoration = InlineDecoration::new(
                    Range::new(view_range.end_line_number, view_range.end_column, view_range.end_line_number, view_range.end_column),
                    after_content_class_name.clone(),
                    InlineDecorationType::After,
                );
                inline_decorations[index_of(view_range.end_line_number)].push(inline_decoration);
                if decoration_options.affects_font.unwrap_or(false) {
                    has_variable_fonts[index_of(view_range.end_line_number)] = true;
                }
            }
        }

        ViewDecorationsCollection {
            decorations: decorations_in_viewport,
            inline_decorations,
            has_variable_fonts,
        }
    }

    pub fn reset(&self) {
        self.decorations_cache.borrow_mut().clear();
    }

    pub fn on_model_decorations_changed(&self) {
        self.reset();
    }

    pub fn on_line_mapping_changed(&self) {
        self.reset();
    }

    fn get_or_create_view_model_decoration(&self, model_decoration: &ModelDecoration) -> Rc<ViewModelDecoration> {
        if let Some(cached) = self.decorations_cache.borrow().get(&model_decoration.id) {
            return cached.clone();
        }
        let model_range = model_decoration.range;
        let options = &model_decoration.options;
        let view_range = if options.is_whole_line {
            let start = self.coordinates_converter.convert_model_position_to_view_position(
                Position::new(model_range.start_line_number, 1),
                Some(PositionAffinity::Left),
                Some(false),
                Some(true),
            );
            let end = self.coordinates_converter.convert_model_position_to_view_position(
                Position::new(model_range.end_line_number, self.model.get_line_max_column(model_range.end_line_number)),
                Some(PositionAffinity::Right),
                None,
                None,
            );
            Range::new(start.line_number, start.column, end.line_number, end.column)
        } else {
            // For backwards compatibility reasons, we want injected text before any decoration.
            // Thus, move decorations to the right.
            self.coordinates_converter.convert_model_range_to_view_range(model_range, Some(PositionAffinity::Right))
        };
        let created = Rc::new(ViewModelDecoration::new(view_range, options.clone()));
        self.decorations_cache.borrow_mut().insert(model_decoration.id.clone(), created.clone());
        created
    }
}

impl IInlineDecorationsComputer for InlineModelDecorationsComputer {
    fn get_inline_decorations(&self, model_line_number: i32) -> Vec<Vec<InlineDecoration>> {
        let model_range = Range::new(model_line_number, 1, model_line_number, self.model.get_line_max_column(model_line_number));
        let view_range = self.coordinates_converter.convert_model_range_to_view_range(model_range, None);
        self.get_decorations(view_range, false, false).inline_decorations
    }
}

/// `IInjectedTextInlineDecorationsComputerContext`.
pub trait IInjectedTextInlineDecorationsComputerContext {
    /// As opções de injeção de um número de linha do modelo.
    fn get_injection_options(&self, model_line_number: i32) -> Option<Vec<InjectedTextOptions>>;
    /// Os deslocamentos de injeção de um número de linha do modelo.
    fn get_injection_offsets(&self, model_line_number: i32) -> Option<Vec<i32>>;
    /// Os deslocamentos de quebra de um número de linha do modelo.
    fn get_break_offsets(&self, model_line_number: i32) -> Vec<i32>;
    /// O tamanho da indentação do texto quebrado de um número de linha do modelo.
    fn get_wrapped_text_indent_length(&self, model_line_number: i32) -> i32;
    /// O número de linha da visão da primeira linha de saída de uma linha do modelo.
    fn get_base_view_line_number(&self, model_line_number: i32) -> i32;
}

/// `InjectedTextInlineDecorationsComputer`.
pub struct InjectedTextInlineDecorationsComputer {
    context: Rc<dyn IInjectedTextInlineDecorationsComputerContext>,
}

impl InjectedTextInlineDecorationsComputer {
    pub fn new(context: Rc<dyn IInjectedTextInlineDecorationsComputerContext>) -> Self {
        Self { context }
    }
}

impl IInlineDecorationsComputer for InjectedTextInlineDecorationsComputer {
    fn get_inline_decorations(&self, model_line_number: i32) -> Vec<Vec<InlineDecoration>> {
        let Some(injection_offsets) = self.context.get_injection_offsets(model_line_number) else {
            return Vec::new();
        };
        let mut line_inline_decorations: Vec<Vec<InlineDecoration>> = Vec::new();
        let mut total_injected_text_length_before: i32 = 0;
        let mut current_injected_offset: usize = 0;

        let injection_options = self.context.get_injection_options(model_line_number);
        let break_offsets = self.context.get_break_offsets(model_line_number);

        for output_line_index in 0..break_offsets.len() {
            let mut inline_decorations: Vec<InlineDecoration> = Vec::new();

            let line_start_offset_in_input_with_injections = if output_line_index > 0 { break_offsets[output_line_index - 1] } else { 0 };
            let line_end_offset_in_input_with_injections = break_offsets[output_line_index];

            while current_injected_offset < injection_offsets.len() {
                let options = &injection_options.as_ref().expect("há deslocamentos de injeção, então há opções")[current_injected_offset];
                // `content.length` do original conta unidades UTF-16.
                let length = options.content.encode_utf16().count() as i32;
                let injected_text_start_offset_in_input_with_injections =
                    injection_offsets[current_injected_offset] + total_injected_text_length_before;
                let injected_text_end_offset_in_input_with_injections = injected_text_start_offset_in_input_with_injections + length;
                let is_width_only = length == 0 && options.width_in_em.is_some();
                let is_last_output_line = output_line_index == break_offsets.len() - 1;
                let is_at_internal_wrap_boundary = injected_text_start_offset_in_input_with_injections
                    == line_end_offset_in_input_with_injections
                    && !is_last_output_line;

                if injected_text_start_offset_in_input_with_injections > line_end_offset_in_input_with_injections
                    || (is_width_only && is_at_internal_wrap_boundary)
                {
                    // Injected text only starts in later wrapped lines.
                    break;
                }

                let is_in_line = if is_width_only {
                    line_start_offset_in_input_with_injections <= injected_text_start_offset_in_input_with_injections
                        && (injected_text_start_offset_in_input_with_injections < line_end_offset_in_input_with_injections || is_last_output_line)
                } else {
                    line_start_offset_in_input_with_injections < injected_text_end_offset_in_input_with_injections
                };
                if is_in_line {
                    // Injected text ends after or in this line (but also starts in or before this line).
                    if let Some(inline_class_name) = options.inline_class_name.as_ref().filter(|name| !name.is_empty()) {
                        let wrapped_text_indent_length = self.context.get_wrapped_text_indent_length(model_line_number);
                        let offset = if output_line_index > 0 { wrapped_text_indent_length } else { 0 };
                        let start = offset
                            + (injected_text_start_offset_in_input_with_injections - line_start_offset_in_input_with_injections).max(0);
                        let end = offset
                            + (injected_text_end_offset_in_input_with_injections - line_start_offset_in_input_with_injections)
                                .min(line_end_offset_in_input_with_injections - line_start_offset_in_input_with_injections);
                        if start != end || is_width_only {
                            let view_line_number = self.context.get_base_view_line_number(model_line_number) + output_line_index as i32;
                            let range = Range::new(view_line_number, start + 1, view_line_number, end + 1);
                            let r#type = if is_width_only {
                                InlineDecorationType::WidthOnly
                            } else if options.inline_class_name_affects_letter_spacing {
                                InlineDecorationType::RegularAffectingLetterSpacing
                            } else {
                                InlineDecorationType::Regular
                            };
                            inline_decorations.push(InlineDecoration::new(range, inline_class_name.clone(), r#type));
                        }
                    }
                }
                if injected_text_end_offset_in_input_with_injections <= line_end_offset_in_input_with_injections {
                    total_injected_text_length_before += length;
                    current_injected_offset += 1;
                } else {
                    // injected text breaks into next line, process it again
                    break;
                }
            }
            line_inline_decorations.push(inline_decorations);
        }
        line_inline_decorations
    }
}
