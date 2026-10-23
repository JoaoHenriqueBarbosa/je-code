//! Porte de `src/vs/editor/contrib/comment/browser/blockCommentCommand.ts`: comentar e descomentar a seleção
//! com o comentário de bloco da linguagem.
//!
//! Notas de tradução:
//! - O `@ILanguageConfigurationService` injetado no construtor é o parâmetro explícito
//!   `language_configuration_service`.
//! - `ISingleEditOperation` é [`SingleEditOperation`]; o `forceMoveMarkers` que `EditOperation.insert` marca
//!   não chega ao construtor de edições, porque o original só repassa `range` e `text` ao `builder`.
//! - Os tokens são `Vec<u16>` (UTF-16). Um token vazio na configuração vale como ausente, como o `!token`
//!   do original.
//! - `charCodeAt` fora do texto (`NaN`) é `None`, que nunca é igual ao espaço.

use std::rc::Rc;

use je_base::JsString;
use je_base::char_code as cc;

use je_base::js_string::{char_code_at, index_of, last_index_of, substring};

use super::{from_index, js_index};
use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `BlockCommentCommand`.
pub struct BlockCommentCommand {
    selection: Selection,
    insert_space: bool,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    used_end_token: Option<Vec<u16>>,
}

/// O token de configuração em UTF-16; vazio ou ausente é `None`.
pub(crate) fn token_units(token: Option<&String>) -> Option<Vec<u16>> {
    token.filter(|token| !token.is_empty()).map(|token| token.encode_utf16().collect())
}

fn units_text(units: Vec<u16>) -> JsString {
    JsString::from_units(units)
}

impl BlockCommentCommand {
    pub fn new(selection: Selection, insert_space: bool, language_configuration_service: Rc<dyn ILanguageConfigurationService>) -> Self {
        Self { selection, insert_space, language_configuration_service, used_end_token: None }
    }

    /// `_haystackHasNeedleAtOffset`: compara sem diferenciar maiúsculas de minúsculas ASCII.
    pub fn haystack_has_needle_at_offset(haystack: &[u16], needle: &[u16], offset: i32) -> bool {
        if offset < 0 {
            return false;
        }
        let offset = offset as usize;
        if offset + needle.len() > haystack.len() {
            return false;
        }

        let upper_a = u16::from(b'A');
        let upper_z = u16::from(b'Z');
        for (i, &code_b) in needle.iter().enumerate() {
            let code_a = haystack[offset + i];

            if code_a == code_b {
                continue;
            }
            if code_a >= upper_a && code_a <= upper_z && code_a + 32 == code_b {
                // codeA é a variante maiúscula de codeB
                continue;
            }
            if code_b >= upper_a && code_b <= upper_z && code_b + 32 == code_a {
                // codeB é a variante maiúscula de codeA
                continue;
            }

            return false;
        }
        true
    }

    fn create_operations_for_block_comment(
        &mut self,
        selection: Range,
        start_token: Vec<u16>,
        end_token: Vec<u16>,
        insert_space: bool,
        model: &TextModel,
        builder: &mut dyn EditOperationBuilder,
    ) {
        let mut start_token = start_token;
        let mut end_token = end_token;

        let start_line_number = selection.start_line_number;
        let start_column = selection.start_column;
        let end_line_number = selection.end_line_number;
        let end_column = selection.end_column;

        let start_line_content = model.get_line_content(start_line_number);
        let end_line_content = model.get_line_content(end_line_number);
        let start_line_text = start_line_content.units();
        let end_line_text = end_line_content.units();

        let mut start_token_index =
            js_index(last_index_of(start_line_text, &start_token, from_index(start_column - 1 + start_token.len() as i32)));
        let mut end_token_index = js_index(index_of(end_line_text, &end_token, from_index(end_column - 1 - end_token.len() as i32)));

        if start_token_index != -1 && end_token_index != -1 {
            if start_line_number == end_line_number {
                let line_between_tokens =
                    substring(start_line_text, i64::from(start_token_index) + start_token.len() as i64, i64::from(end_token_index));

                if index_of(line_between_tokens, &end_token, 0).is_some() {
                    // força a adicionar um comentário de bloco
                    start_token_index = -1;
                    end_token_index = -1;
                }
            } else {
                let start_line_after_start_token =
                    substring(start_line_text, i64::from(start_token_index) + start_token.len() as i64, start_line_text.len() as i64);
                let end_line_before_end_token = substring(end_line_text, 0, i64::from(end_token_index));

                if index_of(start_line_after_start_token, &end_token, 0).is_some() || index_of(end_line_before_end_token, &end_token, 0).is_some() {
                    // força a adicionar um comentário de bloco
                    start_token_index = -1;
                    end_token_index = -1;
                }
            }
        }

        let ops: Vec<SingleEditOperation> = if start_token_index != -1 && end_token_index != -1 {
            // Considera os espaços como parte dos tokens de comentário
            if insert_space
                && ((start_token_index + start_token.len() as i32) as usize) < start_line_text.len()
                && char_code_at(start_line_text, i64::from(start_token_index) + start_token.len() as i64) == Some(cc::Space)
            {
                // Finge que o token de início tem um espaço no fim
                start_token.push(cc::Space);
            }

            if insert_space && end_token_index > 0 && char_code_at(end_line_text, i64::from(end_token_index) - 1) == Some(cc::Space) {
                // Finge que o token de fim tem um espaço no começo
                end_token.insert(0, cc::Space);
                end_token_index -= 1;
            }
            Self::create_remove_block_comment_operations(
                Range::new(start_line_number, start_token_index + start_token.len() as i32 + 1, end_line_number, end_token_index + 1),
                &start_token,
                &end_token,
            )
        } else {
            let ops = Self::create_add_block_comment_operations(selection, &start_token, &end_token, self.insert_space);
            self.used_end_token = if ops.len() == 1 { Some(end_token) } else { None };
            ops
        };

        for op in ops {
            builder.add_tracked_edit_operation(op.range, op.text, false);
        }
    }

    /// `_createRemoveBlockCommentOperations`: só o tamanho dos tokens importa.
    pub fn create_remove_block_comment_operations(r: Range, start_token: &[u16], end_token: &[u16]) -> Vec<SingleEditOperation> {
        let mut res = Vec::new();

        if !r.is_empty() {
            // Remove o começo do comentário de bloco
            res.push(EditOperation::delete(Range::new(
                r.start_line_number,
                r.start_column - start_token.len() as i32,
                r.start_line_number,
                r.start_column,
            )));

            // Remove o fim do comentário de bloco
            res.push(EditOperation::delete(Range::new(
                r.end_line_number,
                r.end_column,
                r.end_line_number,
                r.end_column + end_token.len() as i32,
            )));
        } else {
            // Remove os dois de uma vez
            res.push(EditOperation::delete(Range::new(
                r.start_line_number,
                r.start_column - start_token.len() as i32,
                r.end_line_number,
                r.end_column + end_token.len() as i32,
            )));
        }

        res
    }

    /// `_createAddBlockCommentOperations`.
    pub fn create_add_block_comment_operations(r: Range, start_token: &[u16], end_token: &[u16], insert_space: bool) -> Vec<SingleEditOperation> {
        let mut res = Vec::new();

        if !r.is_empty() {
            // Insere o começo do comentário de bloco
            let mut start_text = start_token.to_vec();
            if insert_space {
                start_text.push(cc::Space);
            }
            res.push(EditOperation::insert(Position::new(r.start_line_number, r.start_column), units_text(start_text)));

            // Insere o fim do comentário de bloco
            let mut end_text = Vec::with_capacity(end_token.len() + 1);
            if insert_space {
                end_text.push(cc::Space);
            }
            end_text.extend_from_slice(end_token);
            res.push(EditOperation::insert(Position::new(r.end_line_number, r.end_column), units_text(end_text)));
        } else {
            // Insere os dois de uma vez
            let mut text = start_token.to_vec();
            text.push(cc::Space);
            text.push(cc::Space);
            text.extend_from_slice(end_token);
            res.push(EditOperation::replace(
                Range::new(r.start_line_number, r.start_column, r.end_line_number, r.end_column),
                Some(units_text(text)),
            ));
        }

        res
    }
}

impl Command for BlockCommentCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let start_line_number = self.selection.start_line_number;
        let start_column = self.selection.start_column;

        model.tokenization().tokenize_if_cheap(start_line_number);
        let language_id = model.tokenization().get_language_id_at_position(start_line_number, start_column);
        let configuration = self.language_configuration_service.get_language_configuration(&language_id);
        let Some(config) = configuration.comments.as_ref() else {
            // A linguagem não tem comentário de bloco
            return;
        };
        let (Some(start_token), Some(end_token)) =
            (token_units(config.block_comment_start_token.as_ref()), token_units(config.block_comment_end_token.as_ref()))
        else {
            // A linguagem não tem comentário de bloco
            return;
        };

        let selection = *self.selection;
        self.create_operations_for_block_comment(selection, start_token, end_token, self.insert_space, model, builder);
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let inverse_edit_operations = helper.get_inverse_edit_operations();
        if inverse_edit_operations.len() == 2 {
            let start_token_edit_operation = &inverse_edit_operations[0];
            let end_token_edit_operation = &inverse_edit_operations[1];

            Selection::new(
                start_token_edit_operation.range.end_line_number,
                start_token_edit_operation.range.end_column,
                end_token_edit_operation.range.start_line_number,
                end_token_edit_operation.range.start_column,
            )
        } else {
            let src_range = inverse_edit_operations[0].range;
            // menos 1 do espaço antes do endToken
            let delta_column = self.used_end_token.as_ref().map_or(0, |token| -(token.len() as i32) - 1);
            Selection::new(
                src_range.end_line_number,
                src_range.end_column + delta_column,
                src_range.end_line_number,
                src_range.end_column + delta_column,
            )
        }
    }
}
