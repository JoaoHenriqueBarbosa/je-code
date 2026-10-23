//! Porte de `src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeTextBuffer.ts`: o buffer de texto do
//! editor (API de `ITextBuffer`) sobre o piece tree.
//!
//! Diferenças de forma, nenhuma de comportamento: o buffer é um valor (não um `Rc`), com `&mut self` nas
//! escritas; o `Disposable` do original vira um [`Disposable`] guardado dentro (o handle de descarte sai
//! de [`PieceTreeTextBuffer::disposable`]) com o emissor `onDidChangeContent` registrado nele.
//! `ITextBuffer.equals(other)` é só com outro `PieceTreeTextBuffer` (o `instanceof` do original).

use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::strings;

use crate::eol_counter::{StringEol, count_eol};
use crate::model::{
    ApplyEditsResult, EndOfLinePreference, Eol, FindMatch, InternalModelContentChange, SearchData, SingleEditOperationIdentifier,
    ValidAnnotatedEditOperation, ValidEditOperation,
};
use crate::piece_tree_base::{PieceTreeBase, PieceTreeSnapshot, StringBuffer, replace_line_breaks};
use crate::position::Position;
use crate::range::Range;
use crate::text_change::TextChange;

/// `IValidatedEditOperation`.
#[derive(Clone, Debug)]
pub struct ValidatedEditOperation {
    pub sort_index: usize,
    pub identifier: Option<SingleEditOperationIdentifier>,
    pub range: Range,
    pub range_offset: usize,
    pub range_length: usize,
    pub text: JsString,
    pub eol_count: usize,
    pub first_line_length: usize,
    pub last_line_length: usize,
    pub force_move_markers: bool,
    pub is_auto_whitespace_edit: bool,
}

/// O buffer de texto do editor.
pub struct PieceTreeTextBuffer {
    base: Rc<Disposable>,
    piece_tree: PieceTreeBase,
    bom: JsString,
    might_contain_rtl: bool,
    might_contain_unusual_line_terminators: bool,
    might_contain_non_basic_ascii: bool,
    on_did_change_content: Rc<Emitter<()>>,
}

impl IDisposable for PieceTreeTextBuffer {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl PieceTreeTextBuffer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chunks: Vec<StringBuffer>,
        bom: JsString,
        eol: Eol,
        contains_rtl: bool,
        contains_unusual_line_terminators: bool,
        is_basic_ascii: bool,
        eol_normalized: bool,
    ) -> Self {
        let base = Disposable::new();
        let on_did_change_content = Emitter::<()>::new();
        base.register(on_did_change_content.clone());
        PieceTreeTextBuffer {
            base,
            piece_tree: PieceTreeBase::new(chunks, eol, eol_normalized),
            bom,
            might_contain_non_basic_ascii: !is_basic_ascii,
            might_contain_rtl: contains_rtl,
            might_contain_unusual_line_terminators: contains_unusual_line_terminators,
            on_did_change_content,
        }
    }

    /// O handle de descarte do buffer (o `disposable` que o `create` da fábrica devolve).
    pub fn disposable(&self) -> DisposableRef {
        self.base.clone()
    }

    /// `onDidChangeContent`.
    pub fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_content.event()
    }

    // #region TextBuffer
    pub fn equals(&self, other: &PieceTreeTextBuffer) -> bool {
        if self.bom != other.bom {
            return false;
        }
        if self.get_eol() != other.get_eol() {
            return false;
        }
        self.piece_tree.equal(&other.piece_tree)
    }

    pub fn might_contain_rtl(&self) -> bool {
        self.might_contain_rtl
    }

    pub fn might_contain_unusual_line_terminators(&self) -> bool {
        self.might_contain_unusual_line_terminators
    }

    pub fn reset_might_contain_unusual_line_terminators(&mut self) {
        self.might_contain_unusual_line_terminators = false;
    }

    pub fn might_contain_non_basic_ascii(&self) -> bool {
        self.might_contain_non_basic_ascii
    }

    pub fn get_bom(&self) -> &JsString {
        &self.bom
    }

    pub fn get_eol(&self) -> Eol {
        self.piece_tree.get_eol()
    }

    pub fn create_snapshot(&self, preserve_bom: bool) -> PieceTreeSnapshot {
        self.piece_tree.create_snapshot(if preserve_bom { self.bom.units() } else { &[] })
    }

    pub fn get_offset_at(&self, line_number: i32, column: i32) -> usize {
        self.piece_tree.get_offset_at(line_number, column)
    }

    pub fn get_position_at(&self, offset: i64) -> Position {
        self.piece_tree.get_position_at(offset)
    }

    pub fn get_range_at(&self, start: usize, length: usize) -> Range {
        let end = start + length;
        let start_position = self.get_position_at(start as i64);
        let end_position = self.get_position_at(end as i64);
        Range::new(
            start_position.line_number,
            start_position.column,
            end_position.line_number,
            end_position.column,
        )
    }

    pub fn get_value_in_range(&self, range: &Range, eol: EndOfLinePreference) -> JsString {
        if range.is_empty() {
            return JsString::new();
        }

        let line_ending = self.get_end_of_line(eol);
        self.piece_tree.get_value_in_range(range, Some(line_ending))
    }

    pub fn get_value_length_in_range(&self, range: &Range, eol: EndOfLinePreference) -> usize {
        if range.is_empty() {
            return 0;
        }

        if range.start_line_number == range.end_line_number {
            return (range.end_column - range.start_column) as usize;
        }

        let start_offset = self.get_offset_at(range.start_line_number, range.start_column);
        let end_offset = self.get_offset_at(range.end_line_number, range.end_column);

        // Os deslocamentos usam o fim de linha do texto, então compensa a diferença de comprimento
        // se o pedido não bate com o do texto.
        let mut eol_offset_compensation: isize = 0;
        let desired_eol = self.get_end_of_line(eol);
        let actual_eol = self.get_eol();
        if desired_eol.len() != actual_eol.len() {
            let delta = desired_eol.len() as isize - actual_eol.len() as isize;
            let eol_count = (range.end_line_number - range.start_line_number) as isize;
            eol_offset_compensation = delta * eol_count;
        }

        (end_offset as isize - start_offset as isize + eol_offset_compensation) as usize
    }

    pub fn get_character_count_in_range(&self, range: &Range, eol: EndOfLinePreference) -> usize {
        if self.might_contain_non_basic_ascii {
            // temos de contar iterando

            let mut result = 0;

            let from_line_number = range.start_line_number;
            let to_line_number = range.end_line_number;
            for line_number in from_line_number..=to_line_number {
                let line_content = self.get_line_content(line_number);
                let from_offset = if line_number == from_line_number {
                    (range.start_column - 1) as usize
                } else {
                    0
                };
                let to_offset = if line_number == to_line_number {
                    (range.end_column - 1) as usize
                } else {
                    line_content.len()
                };

                let mut offset = from_offset;
                while offset < to_offset {
                    if line_content
                        .units()
                        .get(offset)
                        .is_some_and(|&unit| strings::is_high_surrogate(unit))
                    {
                        result += 1;
                        offset += 1;
                    } else {
                        result += 1;
                    }
                    offset += 1;
                }
            }

            result += self.get_end_of_line(eol).len() * (to_line_number - from_line_number) as usize;

            return result;
        }

        self.get_value_length_in_range(range, eol)
    }

    pub fn get_nearest_chunk(&self, offset: usize) -> JsString {
        self.piece_tree.get_nearest_chunk(offset)
    }

    pub fn get_length(&self) -> usize {
        self.piece_tree.get_length()
    }

    pub fn get_line_count(&self) -> usize {
        self.piece_tree.get_line_count()
    }

    pub fn get_lines_content(&self) -> Vec<JsString> {
        self.piece_tree.get_lines_content()
    }

    pub fn get_line_content(&self, line_number: i32) -> JsString {
        self.piece_tree.get_line_content(line_number)
    }

    pub fn get_line_char_code(&self, line_number: i32, index: i32) -> Option<u16> {
        self.piece_tree.get_line_char_code(line_number, index)
    }

    pub fn get_char_code(&self, offset: usize) -> Option<u16> {
        self.piece_tree.get_char_code(offset)
    }

    pub fn get_line_length(&self, line_number: i32) -> usize {
        self.piece_tree.get_line_length(line_number)
    }

    pub fn get_line_min_column(&self, _line_number: i32) -> usize {
        1
    }

    pub fn get_line_max_column(&self, line_number: i32) -> usize {
        self.get_line_length(line_number) + 1
    }

    pub fn get_line_first_non_whitespace_column(&self, line_number: i32) -> usize {
        match strings::first_non_whitespace_index(self.get_line_content(line_number).units()) {
            None => 0,
            Some(result) => result + 1,
        }
    }

    pub fn get_line_last_non_whitespace_column(&self, line_number: i32) -> usize {
        let line = self.get_line_content(line_number);
        match strings::last_non_whitespace_index(line.units(), line.len() as i64 - 1) {
            None => 0,
            Some(result) => result + 2,
        }
    }

    fn get_end_of_line(&self, eol: EndOfLinePreference) -> Eol {
        match eol {
            EndOfLinePreference::Lf => Eol::Lf,
            EndOfLinePreference::CrLf => Eol::CrLf,
            EndOfLinePreference::TextDefined => self.get_eol(),
        }
    }

    pub fn set_eol(&mut self, new_eol: Eol) {
        self.piece_tree.set_eol(new_eol);
    }

    pub fn apply_edits(
        &mut self,
        raw_operations: &[ValidAnnotatedEditOperation],
        record_trim_auto_whitespace: bool,
        compute_undo_edits: bool,
    ) -> ApplyEditsResult {
        let mut might_contain_rtl = self.might_contain_rtl;
        let mut might_contain_unusual_line_terminators = self.might_contain_unusual_line_terminators;
        let mut might_contain_non_basic_ascii = self.might_contain_non_basic_ascii;
        let mut can_reduce_operations = true;

        let mut operations: Vec<ValidatedEditOperation> = Vec::with_capacity(raw_operations.len());
        for (i, op) in raw_operations.iter().enumerate() {
            if can_reduce_operations && op.is_tracked {
                can_reduce_operations = false;
            }
            let validated_range = op.range;
            let op_text = op.text.as_ref().filter(|text| !text.is_empty());
            if let Some(text) = op_text {
                let mut text_might_contain_non_basic_ascii = true;
                if !might_contain_non_basic_ascii {
                    text_might_contain_non_basic_ascii = !strings::is_basic_ascii(text.units());
                    might_contain_non_basic_ascii = text_might_contain_non_basic_ascii;
                }
                if !might_contain_rtl && text_might_contain_non_basic_ascii {
                    // confere se o texto inserido tem RTL
                    might_contain_rtl = strings::contains_rtl(text.units());
                }
                if !might_contain_unusual_line_terminators && text_might_contain_non_basic_ascii {
                    // confere se o texto inserido tem terminadores de linha incomuns
                    might_contain_unusual_line_terminators = strings::contains_unusual_line_terminators(text.units());
                }
            }

            let mut valid_text = JsString::new();
            let mut eol_count = 0;
            let mut first_line_length = 0;
            let mut last_line_length = 0;
            if let Some(text) = op_text {
                let counted = count_eol(text.units());
                eol_count = counted.eol_count;
                first_line_length = counted.first_line_length;
                last_line_length = counted.last_line_length;

                let buffer_eol = self.get_eol();
                let expected_str_eol = if buffer_eol == Eol::CrLf { StringEol::CRLF } else { StringEol::LF };
                if counted.eol == StringEol::UNKNOWN || counted.eol == expected_str_eol {
                    valid_text = text.clone();
                } else {
                    valid_text = JsString::from_units(replace_line_breaks(text.units(), buffer_eol.as_units()));
                }
            }

            operations.push(ValidatedEditOperation {
                sort_index: i,
                identifier: op.identifier,
                range: validated_range,
                range_offset: self.get_offset_at(validated_range.start_line_number, validated_range.start_column),
                range_length: self.get_value_length_in_range(&validated_range, EndOfLinePreference::TextDefined),
                text: valid_text,
                eol_count,
                first_line_length,
                last_line_length,
                force_move_markers: op.force_move_markers,
                is_auto_whitespace_edit: op.is_auto_whitespace_edit,
            });
        }

        // Ordena as operações em ordem crescente
        operations.sort_by(|a, b| Self::sort_ops_ascending(a, b).cmp(&0));

        let mut has_touching_ranges = false;
        for i in 0..operations.len().saturating_sub(1) {
            let range_end = operations[i].range.get_end_position();
            let next_range_start = operations[i + 1].range.get_start_position();

            if next_range_start.is_before_or_equal(&range_end) {
                if next_range_start.is_before(&range_end) {
                    // intervalos que se sobrepõem
                    errors::throw(Error::new("Overlapping ranges are not allowed!"));
                }
                has_touching_ranges = true;
            }
        }

        if can_reduce_operations {
            operations = self.reduce_operations(operations);
        }

        // Codifica as operações em delta
        let reverse_ranges = if compute_undo_edits || record_trim_auto_whitespace {
            Self::get_inverse_edit_ranges(&operations)
        } else {
            Vec::new()
        };
        let mut new_trim_auto_whitespace_candidates: Vec<(i32, JsString)> = Vec::new();
        if record_trim_auto_whitespace {
            for (op, reverse_range) in operations.iter().zip(&reverse_ranges) {
                if op.is_auto_whitespace_edit && op.range.is_empty() {
                    // Já registra os números de linha futuros que podem ser candidatos a remoção de
                    // espaço automático na próxima edição.
                    for line_number in reverse_range.start_line_number..=reverse_range.end_line_number {
                        let mut current_line_content = JsString::new();
                        if line_number == reverse_range.start_line_number {
                            current_line_content = self.get_line_content(op.range.start_line_number);
                            if strings::first_non_whitespace_index(current_line_content.units()).is_some() {
                                continue;
                            }
                        }
                        new_trim_auto_whitespace_candidates.push((line_number, current_line_content));
                    }
                }
            }
        }

        let mut reverse_operations: Option<Vec<ValidEditOperation>> = None;
        if compute_undo_edits {
            let mut reverse_range_delta_offset: isize = 0;
            let mut reverse_ops: Vec<(usize, ValidEditOperation)> = Vec::with_capacity(operations.len());
            for (op, reverse_range) in operations.iter().zip(&reverse_ranges) {
                let buffer_text = self.get_value_in_range(&op.range, EndOfLinePreference::TextDefined);
                let reverse_range_offset = (op.range_offset as isize + reverse_range_delta_offset) as usize;
                reverse_range_delta_offset += op.text.len() as isize - buffer_text.len() as isize;

                let text_change = TextChange::new(op.range_offset, buffer_text.clone(), reverse_range_offset, op.text.clone());
                reverse_ops.push((
                    op.sort_index,
                    ValidEditOperation {
                        identifier: op.identifier,
                        range: *reverse_range,
                        text: buffer_text,
                        text_change,
                    },
                ));
            }

            // Só dá para ordenar as operações reversas quando a ordem não importa
            if !has_touching_ranges {
                reverse_ops.sort_by_key(|(sort_index, _)| *sort_index);
            }
            reverse_operations = Some(reverse_ops.into_iter().map(|(_, op)| op).collect());
        }

        self.might_contain_rtl = might_contain_rtl;
        self.might_contain_unusual_line_terminators = might_contain_unusual_line_terminators;
        self.might_contain_non_basic_ascii = might_contain_non_basic_ascii;

        let content_changes = self.do_apply_edits(operations);

        let mut trim_auto_whitespace_line_numbers: Option<Vec<i32>> = None;
        if record_trim_auto_whitespace && !new_trim_auto_whitespace_candidates.is_empty() {
            // ordena de forma decrescente os números de linha candidatos à remoção na próxima edição
            new_trim_auto_whitespace_candidates.sort_by_key(|(line_number, _)| std::cmp::Reverse(*line_number));

            let mut line_numbers = Vec::new();
            for (i, (line_number, prev_content)) in new_trim_auto_whitespace_candidates.iter().enumerate() {
                if i > 0 && new_trim_auto_whitespace_candidates[i - 1].0 == *line_number {
                    // Não repete o mesmo número de linha
                    continue;
                }

                let line_content = self.get_line_content(*line_number);

                if line_content.is_empty()
                    || line_content == *prev_content
                    || strings::first_non_whitespace_index(line_content.units()).is_some()
                {
                    continue;
                }

                line_numbers.push(*line_number);
            }
            trim_auto_whitespace_line_numbers = Some(line_numbers);
        }

        self.on_did_change_content.fire(());

        ApplyEditsResult {
            reverse_edits: reverse_operations,
            changes: content_changes,
            trim_auto_whitespace_line_numbers,
        }
    }

    /// Transforma as operações em uma que faz a mesma edição lógica sem causar falta de memória.
    fn reduce_operations(&self, operations: Vec<ValidatedEditOperation>) -> Vec<ValidatedEditOperation> {
        if operations.len() < 1000 {
            // Sabemos por testes empíricos que mil edições funcionam bem, seja qual for a forma delas.
            return operations;
        }

        // Em certo ponto, pela forma como os eventos são emitidos e cada operação é tratada, algumas
        // operações disparam muitas alocações temporárias de cadeias que são editadas de novo em seguida,
        // por exemplo um formatador inserindo quantidades absurdas de \n num modelo de uma linha só.
        // A estratégia é então juntar todas as operações numa edição gigante.
        vec![self.to_single_edit_operation(&operations)]
    }

    pub fn to_single_edit_operation(&self, operations: &[ValidatedEditOperation]) -> ValidatedEditOperation {
        let mut force_move_markers = false;
        let first_edit_range = operations[0].range;
        let last_edit_range = operations[operations.len() - 1].range;
        let entire_edit_range = Range::new(
            first_edit_range.start_line_number,
            first_edit_range.start_column,
            last_edit_range.end_line_number,
            last_edit_range.end_column,
        );
        let mut last_end_line_number = first_edit_range.start_line_number;
        let mut last_end_column = first_edit_range.start_column;
        let mut result: Vec<u16> = Vec::new();

        for operation in operations {
            let range = operation.range;

            force_move_markers = force_move_markers || operation.force_move_markers;

            // (1) -- Acrescenta o texto antigo
            result.extend_from_slice(
                self.get_value_in_range(
                    &Range::new(last_end_line_number, last_end_column, range.start_line_number, range.start_column),
                    EndOfLinePreference::TextDefined,
                )
                .units(),
            );

            // (2) -- Acrescenta o texto novo
            if !operation.text.is_empty() {
                result.extend_from_slice(operation.text.units());
            }

            last_end_line_number = range.end_line_number;
            last_end_column = range.end_column;
        }

        let counted = count_eol(&result);

        ValidatedEditOperation {
            sort_index: 0,
            identifier: operations[0].identifier,
            range: entire_edit_range,
            range_offset: self.get_offset_at(entire_edit_range.start_line_number, entire_edit_range.start_column),
            range_length: self.get_value_length_in_range(&entire_edit_range, EndOfLinePreference::TextDefined),
            text: JsString::from_units(result),
            eol_count: counted.eol_count,
            first_line_length: counted.first_line_length,
            last_line_length: counted.last_line_length,
            force_move_markers,
            is_auto_whitespace_edit: false,
        }
    }

    fn do_apply_edits(&mut self, mut operations: Vec<ValidatedEditOperation>) -> Vec<InternalModelContentChange> {
        operations.sort_by(|a, b| Self::sort_ops_descending(a, b).cmp(&0));

        let mut content_changes: Vec<InternalModelContentChange> = Vec::new();

        // as operações vão de baixo para cima
        for op in &operations {
            let start_line_number = op.range.start_line_number;
            let start_column = op.range.start_column;
            let end_line_number = op.range.end_line_number;
            let end_column = op.range.end_column;

            if start_line_number == end_line_number && start_column == end_column && op.text.is_empty() {
                // sem efeito
                continue;
            }

            if !op.text.is_empty() {
                // substituição
                self.piece_tree.delete(op.range_offset, op.range_length);
                self.piece_tree.insert(op.range_offset, op.text.units(), true);
            } else {
                // remoção
                self.piece_tree.delete(op.range_offset, op.range_length);
            }

            let content_change_range = Range::new(start_line_number, start_column, end_line_number, end_column);
            content_changes.push(InternalModelContentChange {
                range: content_change_range,
                range_length: op.range_length,
                text: op.text.clone(),
                range_offset: op.range_offset,
                force_move_markers: op.force_move_markers,
            });
        }
        content_changes
    }

    pub fn find_matches_line_by_line(
        &self,
        search_range: &Range,
        search_data: &SearchData,
        capture_matches: bool,
        limit_result_count: usize,
    ) -> Vec<FindMatch> {
        self.piece_tree
            .find_matches_line_by_line(search_range, search_data, capture_matches, limit_result_count)
    }
    // #endregion

    // #region helper
    // para testes
    pub fn get_piece_tree(&self) -> &PieceTreeBase {
        &self.piece_tree
    }

    /// Para testes: o piece tree para escrita (`insert` e `delete` direto).
    pub fn get_piece_tree_mut(&mut self) -> &mut PieceTreeBase {
        &mut self.piece_tree
    }

    /// `_getInverseEditRange`.
    pub fn get_inverse_edit_range(range: &Range, text: &[u16]) -> Range {
        let start_line_number = range.start_line_number;
        let start_column = range.start_column;
        let counted = count_eol(text);

        if !text.is_empty() {
            // a operação insere algo
            let line_count = counted.eol_count as i32 + 1;

            if line_count == 1 {
                // inserção de uma linha
                Range::new(
                    start_line_number,
                    start_column,
                    start_line_number,
                    start_column + counted.first_line_length as i32,
                )
            } else {
                // inserção de várias linhas
                Range::new(
                    start_line_number,
                    start_column,
                    start_line_number + line_count - 1,
                    counted.last_line_length as i32 + 1,
                )
            }
        } else {
            // não há o que inserir
            Range::new(start_line_number, start_column, start_line_number, start_column)
        }
    }

    /// `_getInverseEditRanges`: supõe `operations` validadas e em ordem crescente.
    pub fn get_inverse_edit_ranges(operations: &[ValidatedEditOperation]) -> Vec<Range> {
        let mut result: Vec<Range> = Vec::with_capacity(operations.len());

        let mut prev_op_end_line_number: i32 = 0;
        let mut prev_op_end_column: i32 = 0;
        let mut prev_op: Option<&ValidatedEditOperation> = None;
        for op in operations {
            let start_line_number;
            let start_column;

            if let Some(prev_op) = prev_op {
                if prev_op.range.end_line_number == op.range.start_line_number {
                    start_line_number = prev_op_end_line_number;
                    start_column = prev_op_end_column + (op.range.start_column - prev_op.range.end_column);
                } else {
                    start_line_number = prev_op_end_line_number + (op.range.start_line_number - prev_op.range.end_line_number);
                    start_column = op.range.start_column;
                }
            } else {
                start_line_number = op.range.start_line_number;
                start_column = op.range.start_column;
            }

            let result_range = if !op.text.is_empty() {
                // a operação insere algo
                let line_count = op.eol_count as i32 + 1;

                if line_count == 1 {
                    // inserção de uma linha
                    Range::new(
                        start_line_number,
                        start_column,
                        start_line_number,
                        start_column + op.first_line_length as i32,
                    )
                } else {
                    // inserção de várias linhas
                    Range::new(
                        start_line_number,
                        start_column,
                        start_line_number + line_count - 1,
                        op.last_line_length as i32 + 1,
                    )
                }
            } else {
                // não há o que inserir
                Range::new(start_line_number, start_column, start_line_number, start_column)
            };

            prev_op_end_line_number = result_range.end_line_number;
            prev_op_end_column = result_range.end_column;

            result.push(result_range);
            prev_op = Some(op);
        }

        result
    }

    fn sort_ops_ascending(a: &ValidatedEditOperation, b: &ValidatedEditOperation) -> i64 {
        let r = Range::compare_ranges_using_ends(&a.range, &b.range);
        if r == 0 {
            return a.sort_index as i64 - b.sort_index as i64;
        }
        i64::from(r)
    }

    fn sort_ops_descending(a: &ValidatedEditOperation, b: &ValidatedEditOperation) -> i64 {
        let r = Range::compare_ranges_using_ends(&a.range, &b.range);
        if r == 0 {
            return b.sort_index as i64 - a.sort_index as i64;
        }
        -i64::from(r)
    }
    // #endregion
}
