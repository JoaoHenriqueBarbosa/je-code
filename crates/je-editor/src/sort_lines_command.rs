//! Porte de `src/vs/editor/contrib/linesOperations/browser/sortLinesCommand.ts`.
//!
//! A ordenação usa o `Intl.Collator` padrão (`safeIntl.Collator()`, sem locale nem opções), como o
//! original: [`je_base::collator::Collator`] com as opções padrão, e `compare` de `-1`, `0` ou `1`. O
//! `Array.prototype.sort` do JavaScript é estável, como o `sort_by` do Rust.

use std::cmp::Ordering;

use je_base::JsString;
use je_base::collator::{Collator, CollatorOptions};

use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

thread_local! {
    /// `SortLinesCommand._COLLATOR`: o colador, criado na primeira vez que alguém ordena (o `Lazy` do original).
    static COLLATOR: Collator = Collator::new(None, CollatorOptions::default());
}

/// `SortLinesCommand._COLLATOR.value.compare`.
fn collate(a: &JsString, b: &JsString) -> Ordering {
    COLLATOR.with(|collator| collator.compare(a.units(), b.units()).cmp(&0))
}

/// `SortLinesCommand`.
pub struct SortLinesCommand {
    selection: Selection,
    descending: bool,
    selection_id: Option<String>,
}

impl SortLinesCommand {
    pub fn new(selection: Selection, descending: bool) -> Self {
        Self { selection, descending, selection_id: None }
    }

    /// `SortLinesCommand.canRun(model, selection, descending)`.
    pub fn can_run(model: Option<&TextModel>, selection: &Selection, descending: bool) -> bool {
        let Some(model) = model else {
            return false;
        };

        let Some(data) = get_sort_data(model, selection, descending) else {
            return false;
        };

        data.before.iter().zip(&data.after).any(|(before, after)| before != after)
    }
}

impl Command for SortLinesCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        if let Some(op) = sort_lines(model, &self.selection, self.descending) {
            builder.add_edit_operation(op.range, op.text, false);
        }

        self.selection_id = Some(builder.track_selection(self.selection, None));
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"))
    }
}

/// O que `getSortData` devolve.
struct SortData {
    start_line_number: i32,
    end_line_number: i32,
    before: Vec<JsString>,
    after: Vec<JsString>,
}

fn get_sort_data(model: &TextModel, selection: &Selection, descending: bool) -> Option<SortData> {
    let start_line_number = selection.start_line_number;
    let mut end_line_number = selection.end_line_number;

    if selection.end_column == 1 {
        end_line_number -= 1;
    }

    // Nada a ordenar se o usuário não selecionou nada.
    if start_line_number >= end_line_number {
        return None;
    }

    // O conteúdo da seleção a ordenar.
    let lines_to_sort: Vec<JsString> = (start_line_number..=end_line_number)
        .map(|line_number| model.get_line_content(line_number))
        .collect();

    let mut sorted = lines_to_sort.clone();
    sorted.sort_by(collate);

    // Se for decrescente, inverte a ordem.
    if descending {
        sorted.reverse();
    }

    Some(SortData { start_line_number, end_line_number, before: lines_to_sort, after: sorted })
}

/// Gera a operação de edição que ordena as linhas num modelo.
fn sort_lines(model: &TextModel, selection: &Selection, descending: bool) -> Option<SingleEditOperation> {
    let data = get_sort_data(model, selection, descending)?;

    // `data.after.join('\n')`
    let mut text = JsString::new();
    for (i, line) in data.after.iter().enumerate() {
        if i > 0 {
            text.push_unit(10);
        }
        text.push_units(line.units());
    }

    Some(EditOperation::replace(
        Range::new(data.start_line_number, 1, data.end_line_number, model.get_line_max_column(data.end_line_number)),
        Some(text),
    ))
}
