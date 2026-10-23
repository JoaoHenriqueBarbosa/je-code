//! Porte de `src/vs/editor/test/browser/testCommand.ts`: aplicar um comando do editor num modelo e conferir o
//! texto e a seleção resultantes, e extrair as operações de edição de um comando.
//!
//! Notas de tradução:
//! - O original cria um `ICodeEditor` de teste (`instantiateTestCodeEditor`) e executa o comando pelo
//!   `viewModel.executeCommand`, que cai no `CommandExecutor.executeCommands` do `cursor.ts` (o executor de
//!   comandos do cursor) com um cursor só. Ainda não há editor, view model nem cursor no porte, então
//!   [`execute_command`] porta aqui o caminho que esse único cursor percorre: `_getEditOperationsFromCommand`
//!   (a numeração `major`/`minor`, o descarte de edição vazia sem texto, `trackSelection` com a aderência do
//!   original), `_getLoserCursorMap` (para um comando só, qualquer par de operações sobrepostas o descarta),
//!   `pushEditOperations` com o `ICursorStateComputer` e a remoção dos intervalos rastreados. A seleção
//!   inicial e a final passam pela validação que `SingleCursorState` faz (`validatePosition` na âncora e na
//!   posição).
//!   TODO(integração): trocar por `CommandExecutor::execute_commands` e pelo `ViewModel` quando o cursor
//!   for portado.
//! - O parâmetro `prepare` (`(accessor, disposables) => void`, que mexe nos serviços de instanciação) não
//!   existe: não há `IInstantiationService` no porte, e o único chamador atual não o usa. A `commandFactory`
//!   perde o `accessor` pelo mesmo motivo e recebe só a seleção.
//! - `languageId: string | null` é `Option<&str>`; o `DisposableStore` do original é o `dispose` do modelo.

use je_editor::cursor_common::CursorSimpleModel;
use std::panic::AssertUnwindSafe;

use je_base::JsString;
use je_base::errors;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use je_editor::model::{
    RelaxedTextModelCreationOptions, SingleEditOperationIdentifier, TrackedRangeStickiness, ValidEditOperation,
};
use je_editor::range::Range;
use je_editor::selection::{Selection, SelectionDirection};
use je_editor::text_model::TextModel;

use super::test_text_model::create_text_model_with;

/// `testCommand(lines, languageId, selection, commandFactory, expectedLines, expectedSelection, forceTokenization)`.
pub fn test_command(
    lines: &[&str],
    language_id: Option<&str>,
    selection: Selection,
    command_factory: impl FnOnce(Selection) -> Box<dyn Command>,
    expected_lines: &[&str],
    expected_selection: Selection,
    force_tokenization: bool,
) {
    let model = create_text_model_with(lines.join("\n").as_str(), language_id, RelaxedTextModelCreationOptions::default(), None);

    test_command_on_model(&model, selection, command_factory, expected_lines, expected_selection, force_tokenization);
}

/// O `testCommand` sobre um modelo já criado: é o caminho dos casos cujo `prepare` do original registra
/// linguagens nos serviços (`ILanguageService`, `ILanguageConfigurationService`) que o modelo e o comando
/// compartilham. Quem chama monta esses serviços e o modelo (`instantiate_text_model_with_languages`).
/// Descarta o modelo no fim.
pub fn test_command_on_model(
    model: &TextModel,
    selection: Selection,
    command_factory: impl FnOnce(Selection) -> Box<dyn Command>,
    expected_lines: &[&str],
    expected_selection: Selection,
    force_tokenization: bool,
) {
    if force_tokenization {
        model.tokenization().force_tokenization(model.get_line_count());
    }

    // `viewModel.setSelections('tests', [selection])` e `viewModel.getSelection()`.
    let selection = validate_selection(model, selection);

    let mut command = command_factory(selection);
    let actual_selection = execute_command(model, selection, command.as_mut());

    let expected: Vec<JsString> = expected_lines.iter().map(|line| JsString::from(*line)).collect();
    assert_eq!(model.get_lines_content(), expected);

    assert_eq!(actual_selection.to_string(), expected_selection.to_string());

    model.dispose();
}

/// `getEditOperation(model, command)`: as edições que `command` faria se rodasse em `model`.
pub fn get_edit_operation(model: &TextModel, command: &mut dyn Command) -> Vec<SingleEditOperation> {
    struct CollectingBuilder {
        operations: Vec<SingleEditOperation>,
    }

    impl EditOperationBuilder for CollectingBuilder {
        fn add_edit_operation(&mut self, range: Range, text: Option<JsString>, force_move_markers: bool) {
            self.operations.push(SingleEditOperation::new(range, text, force_move_markers));
        }

        fn add_tracked_edit_operation(&mut self, range: Range, text: Option<JsString>, force_move_markers: bool) {
            self.operations.push(SingleEditOperation::new(range, text, force_move_markers));
        }

        fn track_selection(&mut self, _selection: Selection, _track_previous_on_empty: Option<bool>) -> String {
            String::new()
        }
    }

    let mut builder = CollectingBuilder { operations: Vec::new() };
    command.get_edit_operations(model, &mut builder);
    builder.operations
}

/// A validação que o cursor faz da seleção (`SingleCursorState`): âncora e posição passam por
/// `validatePosition`, e a direção se mantém.
fn validate_selection(model: &TextModel, selection: Selection) -> Selection {
    Selection::from_positions(
        model.validate_position(selection.get_selection_start()),
        model.validate_position(selection.get_position()),
    )
}

/// `IExecContext`.
struct ExecContext<'a> {
    model: &'a TextModel,
    selections_before: Vec<Selection>,
    tracked_ranges: Vec<String>,
    tracked_ranges_direction: Vec<SelectionDirection>,
}

/// `viewModel.executeCommand(command, 'tests')` seguido de `viewModel.getSelection()`, com um cursor só.
fn execute_command(model: &TextModel, selection: Selection, command: &mut dyn Command) -> Selection {
    let mut ctx = ExecContext {
        model,
        selections_before: vec![selection],
        tracked_ranges: Vec::new(),
        tracked_ranges_direction: Vec::new(),
    };

    // `CommandExecutor.executeCommands`
    let result = inner_execute_command(&mut ctx, command);

    for tracked_range in &ctx.tracked_ranges {
        model.set_tracked_range(Some(tracked_range), None, TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges);
    }

    // `_interpretCommandResult`: sem resultado, vale a seleção anterior.
    match result.as_deref() {
        Some([first, ..]) => validate_selection(model, *first),
        _ => selection,
    }
}

/// `CommandExecutor._innerExecuteCommands` para um cursor e um comando.
fn inner_execute_command(ctx: &mut ExecContext<'_>, command: &mut dyn Command) -> Option<Vec<Selection>> {
    let (mut operations, had_tracked_edit_operation) = get_edit_operations_from_command(ctx, 0, command);
    if operations.is_empty() {
        return None;
    }

    if has_loser_cursor(&operations) {
        // Os comandos estão muito errados
        eprintln!("Ignoring commands");
        return None;
    }

    // Dá ao modelo a dica de que as edições são rastreadas.
    if had_tracked_edit_operation {
        operations[0].is_tracked = true;
    }

    let model = ctx.model;
    let selections_before = ctx.selections_before.clone();
    let tracked_ranges = &ctx.tracked_ranges;
    let tracked_ranges_direction = &ctx.tracked_ranges_direction;

    let mut compute_cursor_selections = |inverse_edit_operations: &[ValidEditOperation]| -> Option<Vec<Selection>> {
        // Agrupa pelo cursor (`major`); as operações sem identificador (aparo de espaço automático) saem.
        let mut grouped_inverse_edit_operations: Vec<ValidEditOperation> = inverse_edit_operations
            .iter()
            .filter(|op| op.identifier.is_some_and(|identifier| identifier.major == 0))
            .cloned()
            .collect();

        let cursor_selection = if grouped_inverse_edit_operations.is_empty() {
            selections_before[0]
        } else {
            grouped_inverse_edit_operations.sort_by_key(|op| op.identifier.map_or(0, |identifier| identifier.minor));
            command.compute_cursor_state(
                model,
                &TrackedSelections {
                    model,
                    tracked_ranges,
                    tracked_ranges_direction,
                    inverse_edit_operations: grouped_inverse_edit_operations,
                },
            )
        };
        Some(vec![cursor_selection])
    };

    let selections_after = model.push_edit_operations(Some(selections_before.clone()), &operations, Some(&mut compute_cursor_selections), None, None);
    Some(selections_after.unwrap_or(selections_before))
}

/// O `ICursorStateComputerData` que o executor entrega ao comando.
struct TrackedSelections<'a> {
    model: &'a TextModel,
    tracked_ranges: &'a [String],
    tracked_ranges_direction: &'a [SelectionDirection],
    inverse_edit_operations: Vec<ValidEditOperation>,
}

impl CursorStateComputerData for TrackedSelections<'_> {
    fn get_inverse_edit_operations(&self) -> Vec<ValidEditOperation> {
        self.inverse_edit_operations.clone()
    }

    fn get_tracked_selection(&self, id: &str) -> Selection {
        let idx: usize = id.parse().expect("o id de uma seleção rastreada é o seu índice");
        let range = self
            .model
            .get_tracked_range(&self.tracked_ranges[idx])
            .expect("o intervalo rastreado existe até o fim do comando");
        if self.tracked_ranges_direction[idx] == SelectionDirection::Ltr {
            Selection::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column)
        } else {
            Selection::new(range.end_line_number, range.end_column, range.start_line_number, range.start_column)
        }
    }
}

/// O `editOperationBuilder` de `_getEditOperationsFromCommand`.
struct CommandEditOperationBuilder<'a, 'b> {
    ctx: &'b mut ExecContext<'a>,
    major_identifier: i32,
    operation_minor: i32,
    inserts_auto_whitespace: bool,
    operations: Vec<SingleEditOperation>,
    had_tracked_edit_operation: bool,
}

impl EditOperationBuilder for CommandEditOperationBuilder<'_, '_> {
    fn add_edit_operation(&mut self, range: Range, text: Option<JsString>, force_move_markers: bool) {
        if range.is_empty() && text.as_ref().is_some_and(JsString::is_empty) {
            // Este comando quer adicionar uma operação nula: dispensa.
            return;
        }
        self.operations.push(SingleEditOperation {
            identifier: Some(SingleEditOperationIdentifier { major: self.major_identifier, minor: self.operation_minor }),
            range,
            text,
            force_move_markers,
            is_auto_whitespace_edit: self.inserts_auto_whitespace,
            is_tracked: false,
        });
        self.operation_minor += 1;
    }

    fn add_tracked_edit_operation(&mut self, range: Range, text: Option<JsString>, force_move_markers: bool) {
        self.had_tracked_edit_operation = true;
        self.add_edit_operation(range, text, force_move_markers);
    }

    fn track_selection(&mut self, selection: Selection, track_previous_on_empty: Option<bool>) -> String {
        let stickiness = if selection.is_empty() {
            match track_previous_on_empty {
                Some(true) => TrackedRangeStickiness::GrowsOnlyWhenTypingBefore,
                Some(false) => TrackedRangeStickiness::GrowsOnlyWhenTypingAfter,
                None => {
                    // Tenta prender no texto em volta
                    let max_line_column = self.ctx.model.get_line_max_column(selection.start_line_number);
                    if selection.start_column == max_line_column {
                        TrackedRangeStickiness::GrowsOnlyWhenTypingBefore
                    } else {
                        TrackedRangeStickiness::GrowsOnlyWhenTypingAfter
                    }
                }
            }
        } else {
            TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges
        };

        let l = self.ctx.tracked_ranges.len();
        let id = self
            .ctx
            .model
            .set_tracked_range(None, Some(&*selection), stickiness)
            .expect("criar um intervalo rastreado devolve o id");
        self.ctx.tracked_ranges.push(id);
        self.ctx.tracked_ranges_direction.push(selection.get_direction());
        l.to_string()
    }
}

/// `_getEditOperationsFromCommand`: roda como uma transação, se o comando falhar tudo o que fez é ignorado.
fn get_edit_operations_from_command(
    ctx: &mut ExecContext<'_>,
    major_identifier: i32,
    command: &mut dyn Command,
) -> (Vec<SingleEditOperation>, bool) {
    let model = ctx.model;
    let mut builder = CommandEditOperationBuilder {
        ctx,
        major_identifier,
        operation_minor: 0,
        inserts_auto_whitespace: command.inserts_auto_whitespace(),
        operations: Vec::new(),
        had_tracked_edit_operation: false,
    };

    match errors::catch(AssertUnwindSafe(|| command.get_edit_operations(model, &mut builder))) {
        Ok(()) => (builder.operations, builder.had_tracked_edit_operation),
        Err(error) => {
            errors::on_unexpected_error(&error);
            (Vec::new(), false)
        }
    }
}

/// `_getLoserCursorMap(operations).hasOwnProperty('0')` com um comando só (`major` 0): ordena as
/// operações com a última primeiro e, se alguma começa antes do fim da seguinte, elas se sobrepõem.
fn has_loser_cursor(operations: &[SingleEditOperation]) -> bool {
    // `Range.compareRangesUsingEnds`, com o sinal trocado.
    let ends_key = |range: &Range| (range.end_line_number, range.end_column, range.start_line_number, range.start_column);
    let mut sorted: Vec<&Range> = operations.iter().map(|operation| &operation.range).collect();
    sorted.sort_by(|a, b| ends_key(b).cmp(&ends_key(a)));

    sorted
        .windows(2)
        .any(|pair| (pair[0].start_line_number, pair[0].start_column) < (pair[1].end_line_number, pair[1].end_column))
}
