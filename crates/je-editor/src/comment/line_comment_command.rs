//! Porte de `src/vs/editor/contrib/comment/browser/lineCommentCommand.ts`: comentar, descomentar e alternar
//! comentários de linha, com o comentário de bloco como alternativa quando a linguagem não tem o de linha.
//!
//! Notas de tradução:
//! - O `@ILanguageConfigurationService` injetado no construtor é o parâmetro explícito
//!   `language_configuration_service` (o primeiro, como no original).
//! - `ignoreFirstLine?: boolean` é `Option<bool>`.
//! - `ISimpleModel` (só `getLineContent`) é o [`CursorSimpleModel`], que o `TextModel` implementa.
//! - `IPreflightData` (`supported: true | false`) é o enum [`PreflightData`]; `IPreflightDataSupported` é
//!   [`PreflightDataSupported`] sem o campo `supported`.
//! - `IInsertionPoint` é o trait [`IInsertionPoint`], porque o original aceita qualquer objeto com `ignore` e
//!   `commentStrOffset` (e passa os `ILinePreflightData` ali); [`InsertionPoint`] é o objeto simples.
//! - O `commentStr` é [`JsString`] e `commentStrLength` é `i32`. Os tokens de bloco são `Vec<u16>`.
//! - O JavaScript muta o vetor `lines` e o devolve dentro do resultado; `_analyzeLines` recebe o vetor por
//!   valor e o devolve em [`PreflightDataSupported`], e `_normalizeInsertionPoint` recebe `&mut [T]`.
//! - No segundo laço de `_normalizeInsertionPoint` o `j` é o declarado fora dos laços (o do primeiro laço é
//!   outro, que o esconde), e depois do laço ele vale o último índice alcançado; a tradução mantém isso.

use std::rc::Rc;

use je_base::JsString;
use je_base::char_code as cc;
use je_base::js_string::{char_code_at, index_of, last_index_of};
use je_base::strings::first_non_whitespace_index;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;

use super::block_comment_command::{BlockCommentCommand, token_units};
use super::{from_index, js_index};
use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `IInsertionPoint`.
pub trait IInsertionPoint {
    fn ignore(&self) -> bool;
    fn comment_str_offset(&self) -> i32;
    fn set_comment_str_offset(&mut self, offset: i32);
}

/// Um objeto `{ ignore, commentStrOffset }` simples, o que o original passa onde pede um `IInsertionPoint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertionPoint {
    pub ignore: bool,
    pub comment_str_offset: i32,
}

impl IInsertionPoint for InsertionPoint {
    fn ignore(&self) -> bool {
        self.ignore
    }

    fn comment_str_offset(&self) -> i32 {
        self.comment_str_offset
    }

    fn set_comment_str_offset(&mut self, offset: i32) {
        self.comment_str_offset = offset;
    }
}

/// `ILinePreflightData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinePreflightData {
    pub ignore: bool,
    pub comment_str: JsString,
    pub comment_str_offset: i32,
    pub comment_str_length: i32,
}

impl IInsertionPoint for LinePreflightData {
    fn ignore(&self) -> bool {
        self.ignore
    }

    fn comment_str_offset(&self) -> i32 {
        self.comment_str_offset
    }

    fn set_comment_str_offset(&mut self, offset: i32) {
        self.comment_str_offset = offset;
    }
}

/// `IPreflightDataSupported`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightDataSupported {
    pub should_remove_comments: bool,
    pub lines: Vec<LinePreflightData>,
}

/// `IPreflightData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreflightData {
    Supported(PreflightDataSupported),
    Unsupported,
}

/// `Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Toggle = 0,
    ForceAdd = 1,
    ForceRemove = 2,
}

/// `LineCommentCommand`.
pub struct LineCommentCommand {
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    selection: Selection,
    indent_size: i32,
    type_: Type,
    insert_space: bool,
    ignore_empty_lines: bool,
    selection_id: Option<String>,
    delta_column: i32,
    move_end_position_down: bool,
    ignore_first_line: bool,
}

impl LineCommentCommand {
    pub fn new(
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
        selection: Selection,
        indent_size: i32,
        type_: Type,
        insert_space: bool,
        ignore_empty_lines: bool,
        ignore_first_line: Option<bool>,
    ) -> Self {
        Self {
            language_configuration_service,
            selection,
            indent_size,
            type_,
            insert_space,
            selection_id: None,
            delta_column: 0,
            move_end_position_down: false,
            ignore_empty_lines,
            ignore_first_line: ignore_first_line.unwrap_or(false),
        }
    }

    /// Faz uma primeira passada pelas linhas e junta os dados do token de comentário de linha.
    /// Devolve `None` se alguma linha não suporta comentário de linha.
    fn gather_preflight_comment_strings(
        model: &TextModel,
        start_line_number: i32,
        end_line_number: i32,
        language_configuration_service: &dyn ILanguageConfigurationService,
    ) -> Option<Vec<LinePreflightData>> {
        model.tokenization().tokenize_if_cheap(start_line_number);
        let language_id = model.tokenization().get_language_id_at_position(start_line_number, 1);

        let configuration = language_configuration_service.get_language_configuration(&language_id);
        let comment_str = configuration.comments.as_ref().and_then(|config| token_units(config.line_comment_token.as_ref()));
        // A linguagem não suporta comentário de linha
        let comment_str = JsString::from_units(comment_str?);

        let line_count = (end_line_number - start_line_number + 1).max(0);
        Some(
            (0..line_count)
                .map(|_| LinePreflightData {
                    ignore: false,
                    comment_str: comment_str.clone(),
                    comment_str_offset: 0,
                    comment_str_length: comment_str.len() as i32,
                })
                .collect(),
        )
    }

    /// Analisa as linhas e decide quais importam e o que o alternar deve fazer.
    /// Também monta os deslocamentos e tamanhos úteis para gerar as operações de edição.
    #[allow(clippy::too_many_arguments)]
    pub fn analyze_lines(
        type_: Type,
        insert_space: bool,
        model: &dyn CursorSimpleModel,
        mut lines: Vec<LinePreflightData>,
        start_line_number: i32,
        ignore_empty_lines: bool,
        ignore_first_line: bool,
        language_configuration_service: &dyn ILanguageConfigurationService,
        language_id: &str,
    ) -> PreflightData {
        let mut only_whitespace_lines = true;

        let configuration = language_configuration_service.get_language_configuration(language_id);
        let line_comment_no_indent = configuration.comments.as_ref().and_then(|config| config.line_comment_no_indent).unwrap_or(false);

        let mut should_remove_comments = match type_ {
            Type::Toggle => true,
            Type::ForceAdd => false,
            Type::ForceRemove => true,
        };

        for (i, line_data) in lines.iter_mut().enumerate() {
            let line_number = start_line_number + i as i32;

            if line_number == start_line_number && ignore_first_line {
                // primeira linha ignorada
                line_data.ignore = true;
                continue;
            }

            let line_content = model.get_line_content(line_number);
            let line_content = line_content.units();
            let line_content_start_offset = first_non_whitespace_index(line_content).map_or(-1, |index| index as i32);

            if line_content_start_offset == -1 {
                // Linha vazia ou só com espaço em branco
                line_data.ignore = ignore_empty_lines;
                line_data.comment_str_offset = if line_comment_no_indent { 0 } else { line_content.len() as i32 };
                continue;
            }

            only_whitespace_lines = false;
            let offset = if line_comment_no_indent { 0 } else { line_content_start_offset };
            line_data.ignore = false;
            line_data.comment_str_offset = offset;

            if should_remove_comments && !BlockCommentCommand::haystack_has_needle_at_offset(line_content, line_data.comment_str.units(), offset) {
                match type_ {
                    // Todas as linhas até aqui eram comentário de linha, mas esta não é
                    Type::Toggle => should_remove_comments = false,
                    // Não acontece
                    Type::ForceAdd => {}
                    Type::ForceRemove => line_data.ignore = true,
                }
            }

            if should_remove_comments && insert_space {
                // Remove um espaço seguinte, se houver
                let comment_str_end_offset = line_content_start_offset + line_data.comment_str_length;
                if (comment_str_end_offset as usize) < line_content.len() && char_code_at(line_content, i64::from(comment_str_end_offset)) == Some(cc::Space) {
                    line_data.comment_str_length += 1;
                }
            }
        }

        if type_ == Type::Toggle && only_whitespace_lines {
            // Nas linhas só com espaço em branco, insere comentários
            should_remove_comments = false;

            // E também deixa de ignorá-las
            for line in lines.iter_mut() {
                line.ignore = false;
            }
        }

        PreflightData::Supported(PreflightDataSupported { should_remove_comments, lines })
    }

    /// Analisa todas as linhas e decide exatamente o que fazer: não suportado, inserir ou remover
    /// comentários de linha.
    #[allow(clippy::too_many_arguments)]
    pub fn gather_preflight_data(
        type_: Type,
        insert_space: bool,
        model: &TextModel,
        start_line_number: i32,
        end_line_number: i32,
        ignore_empty_lines: bool,
        ignore_first_line: bool,
        language_configuration_service: &dyn ILanguageConfigurationService,
    ) -> PreflightData {
        let lines = Self::gather_preflight_comment_strings(model, start_line_number, end_line_number, language_configuration_service);
        let language_id = model.tokenization().get_language_id_at_position(start_line_number, 1);
        let Some(lines) = lines else {
            return PreflightData::Unsupported;
        };

        Self::analyze_lines(
            type_,
            insert_space,
            model,
            lines,
            start_line_number,
            ignore_empty_lines,
            ignore_first_line,
            language_configuration_service,
            &language_id,
        )
    }

    /// Dada uma análise bem-sucedida, insere ou remove os comentários de linha.
    fn execute_line_comments(
        &mut self,
        model: &dyn CursorSimpleModel,
        builder: &mut dyn EditOperationBuilder,
        data: PreflightDataSupported,
        s: Selection,
    ) {
        let mut data = data;

        let ops = if data.should_remove_comments {
            Self::create_remove_line_comments_operations(&data.lines, s.start_line_number)
        } else {
            Self::normalize_insertion_point(model, &mut data.lines, s.start_line_number, self.indent_size);
            self.create_add_line_comments_operations(&data.lines, s.start_line_number)
        };

        let cursor_position = Position::new(s.position_line_number, s.position_column);

        for op in ops {
            builder.add_edit_operation(op.range, op.text.clone(), false);
            if op.range.is_empty() && op.range.start_line_number == cursor_position.line_number && op.range.start_column == cursor_position.column {
                let line_content = model.get_line_content(cursor_position.line_number);
                if line_content.len() as i32 + 1 == cursor_position.column {
                    self.delta_column = op.text.as_ref().map_or(0, |text| text.len() as i32);
                }
            }
        }

        self.selection_id = Some(builder.track_selection(s, None));
    }

    fn attempt_remove_block_comment(&self, model: &TextModel, s: Selection, start_token: &[u16], end_token: &[u16]) -> Option<Vec<SingleEditOperation>> {
        let mut start_token = start_token.to_vec();
        let mut end_token = end_token.to_vec();
        let mut start_line_number = s.start_line_number;
        let mut end_line_number = s.end_line_number;

        let start_token_allowed_before_column =
            end_token.len() as i32 + model.get_line_first_non_whitespace_column(s.start_line_number).max(s.start_column);

        let mut start_token_index = js_index(last_index_of(
            model.get_line_content(start_line_number).units(),
            &start_token,
            from_index(start_token_allowed_before_column - 1),
        ));
        let mut end_token_index = js_index(index_of(
            model.get_line_content(end_line_number).units(),
            &end_token,
            from_index(s.end_column - 1 - start_token.len() as i32),
        ));

        if start_token_index != -1 && end_token_index == -1 {
            end_token_index = js_index(index_of(
                model.get_line_content(start_line_number).units(),
                &end_token,
                from_index(start_token_index + start_token.len() as i32),
            ));
            end_line_number = start_line_number;
        }

        if start_token_index == -1 && end_token_index != -1 {
            start_token_index =
                js_index(last_index_of(model.get_line_content(end_line_number).units(), &start_token, from_index(end_token_index)));
            start_line_number = end_line_number;
        }

        if s.is_empty() && (start_token_index == -1 || end_token_index == -1) {
            start_token_index = js_index(index_of(model.get_line_content(start_line_number).units(), &start_token, 0));
            if start_token_index != -1 {
                end_token_index = js_index(index_of(
                    model.get_line_content(start_line_number).units(),
                    &end_token,
                    from_index(start_token_index + start_token.len() as i32),
                ));
            }
        }

        // Ajusta para o possível espaço em branco interno.
        // Para um espaço depois do startToken, soma o espaço ao startToken: a conta dos intervalos fecha.
        if start_token_index != -1
            && char_code_at(model.get_line_content(start_line_number).units(), i64::from(start_token_index) + start_token.len() as i64)
                == Some(cc::Space)
        {
            start_token.push(cc::Space);
        }

        // Para um espaço antes do endToken, soma o espaço antes do endToken e move o índice um para a esquerda.
        if end_token_index != -1
            && char_code_at(model.get_line_content(end_line_number).units(), i64::from(end_token_index) - 1) == Some(cc::Space)
        {
            end_token.insert(0, cc::Space);
            end_token_index -= 1;
        }

        if start_token_index != -1 && end_token_index != -1 {
            return Some(BlockCommentCommand::create_remove_block_comment_operations(
                Range::new(start_line_number, start_token_index + start_token.len() as i32 + 1, end_line_number, end_token_index + 1),
                &start_token,
                &end_token,
            ));
        }

        None
    }

    /// Dada uma análise sem sucesso, delega ao comando de comentário de bloco.
    fn execute_block_comment(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder, s: Selection) {
        model.tokenization().tokenize_if_cheap(s.start_line_number);
        let language_id = model.tokenization().get_language_id_at_position(s.start_line_number, 1);
        let configuration = self.language_configuration_service.get_language_configuration(&language_id);
        let Some(config) = configuration.comments.as_ref() else {
            // A linguagem não suporta comentário de bloco
            return;
        };
        let (Some(start_token), Some(end_token)) =
            (token_units(config.block_comment_start_token.as_ref()), token_units(config.block_comment_end_token.as_ref()))
        else {
            // A linguagem não suporta comentário de bloco
            return;
        };

        let ops = match self.attempt_remove_block_comment(model, s, &start_token, &end_token) {
            Some(ops) => ops,
            None => {
                let ops = if s.is_empty() {
                    let line_content = model.get_line_content(s.start_line_number);
                    let mut first_non_whitespace = first_non_whitespace_index(line_content.units()).map_or(-1, |index| index as i32);
                    if first_non_whitespace == -1 {
                        // A linha é vazia ou só tem espaço em branco
                        first_non_whitespace = line_content.len() as i32;
                    }
                    BlockCommentCommand::create_add_block_comment_operations(
                        Range::new(s.start_line_number, first_non_whitespace + 1, s.start_line_number, line_content.len() as i32 + 1),
                        &start_token,
                        &end_token,
                        self.insert_space,
                    )
                } else {
                    BlockCommentCommand::create_add_block_comment_operations(
                        Range::new(
                            s.start_line_number,
                            model.get_line_first_non_whitespace_column(s.start_line_number),
                            s.end_line_number,
                            model.get_line_max_column(s.end_line_number),
                        ),
                        &start_token,
                        &end_token,
                        self.insert_space,
                    )
                };

                if ops.len() == 1 {
                    // Deixa o cursor depois do token e do espaço
                    self.delta_column = start_token.len() as i32 + 1;
                }
                ops
            }
        };
        self.selection_id = Some(builder.track_selection(s, None));
        for op in ops {
            builder.add_edit_operation(op.range, op.text, false);
        }
    }

    /// Gera as operações de edição do caso de remover comentários de linha.
    pub fn create_remove_line_comments_operations(lines: &[LinePreflightData], start_line_number: i32) -> Vec<SingleEditOperation> {
        let mut res = Vec::new();

        for (i, line_data) in lines.iter().enumerate() {
            if line_data.ignore {
                continue;
            }

            let line_number = start_line_number + i as i32;
            res.push(EditOperation::delete(Range::new(
                line_number,
                line_data.comment_str_offset + 1,
                line_number,
                line_data.comment_str_offset + line_data.comment_str_length + 1,
            )));
        }

        res
    }

    /// Gera as operações de edição do caso de inserir comentários de linha.
    fn create_add_line_comments_operations(&self, lines: &[LinePreflightData], start_line_number: i32) -> Vec<SingleEditOperation> {
        let mut res = Vec::new();

        for (i, line_data) in lines.iter().enumerate() {
            if line_data.ignore {
                continue;
            }

            let mut text = line_data.comment_str.clone();
            if self.insert_space {
                text.push_units(&[cc::Space]);
            }
            res.push(EditOperation::insert(Position::new(start_line_number + i as i32, line_data.comment_str_offset + 1), text));
        }

        res
    }

    fn next_visible_column(current_visible_column: i32, indent_size: i32, is_tab: bool, column_size: i32) -> i32 {
        if is_tab {
            return current_visible_column + (indent_size - (current_visible_column % indent_size));
        }
        current_visible_column + column_size
    }

    /// Ajusta os pontos de inserção para ficarem alinhados na vertical no caso de inserir comentários de linha.
    pub fn normalize_insertion_point<T: IInsertionPoint>(model: &dyn CursorSimpleModel, lines: &mut [T], start_line_number: i32, indent_size: i32) {
        let mut min_visible_column = MAX_SAFE_SMALL_INTEGER;

        for (i, line) in lines.iter().enumerate() {
            if line.ignore() {
                continue;
            }

            let line_content = model.get_line_content(start_line_number + i as i32);
            let line_content = line_content.units();

            let mut current_visible_column = 0;
            let mut j = 0;
            let len_j = line.comment_str_offset();
            while current_visible_column < min_visible_column && j < len_j {
                current_visible_column =
                    Self::next_visible_column(current_visible_column, indent_size, char_code_at(line_content, i64::from(j)) == Some(cc::Tab), 1);
                j += 1;
            }

            if current_visible_column < min_visible_column {
                min_visible_column = current_visible_column;
            }
        }

        min_visible_column = (min_visible_column / indent_size) * indent_size;

        for (i, line) in lines.iter_mut().enumerate() {
            if line.ignore() {
                continue;
            }

            let line_content = model.get_line_content(start_line_number + i as i32);
            let line_content = line_content.units();

            let mut current_visible_column = 0;
            let mut j = 0;
            let len_j = line.comment_str_offset();
            while current_visible_column < min_visible_column && j < len_j {
                current_visible_column =
                    Self::next_visible_column(current_visible_column, indent_size, char_code_at(line_content, i64::from(j)) == Some(cc::Tab), 1);
                j += 1;
            }

            if current_visible_column > min_visible_column {
                line.set_comment_str_offset(j - 1);
            } else {
                line.set_comment_str_offset(j);
            }
        }
    }
}

impl Command for LineCommentCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let mut s = self.selection;
        self.move_end_position_down = false;

        if s.start_line_number == s.end_line_number && self.ignore_first_line {
            let text = if s.start_line_number == model.get_line_count() { JsString::new() } else { JsString::from("\n") };
            builder.add_edit_operation(
                Range::new(s.start_line_number, model.get_line_max_column(s.start_line_number), s.start_line_number + 1, 1),
                Some(text),
                false,
            );
            self.selection_id = Some(builder.track_selection(s, None));
            return;
        }

        if s.start_line_number < s.end_line_number && s.end_column == 1 {
            self.move_end_position_down = true;
            s = s.set_end_position(s.end_line_number - 1, model.get_line_max_column(s.end_line_number - 1));
        }

        let data = Self::gather_preflight_data(
            self.type_,
            self.insert_space,
            model,
            s.start_line_number,
            s.end_line_number,
            self.ignore_empty_lines,
            self.ignore_first_line,
            &*self.language_configuration_service,
        );

        match data {
            PreflightData::Supported(data) => self.execute_line_comments(model, builder, data, s),
            PreflightData::Unsupported => self.execute_block_comment(model, builder, s),
        }
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let mut result = helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"));

        if self.move_end_position_down {
            result = result.set_end_position(result.end_line_number + 1, 1);
        }

        Selection::new(
            result.selection_start_line_number,
            result.selection_start_column + self.delta_column,
            result.position_line_number,
            result.position_column + self.delta_column,
        )
    }
}
