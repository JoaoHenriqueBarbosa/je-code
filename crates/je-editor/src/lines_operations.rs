//! Porte de `src/vs/editor/contrib/linesOperations/browser/linesOperations.ts`: as ações de linha do editor
//! (`CopyLinesUp/Down`, `DuplicateSelection`, `MoveLinesUp/Down`, `SortLinesAscending/Descending`,
//! `DeleteDuplicateLines`, `ReverseLines`, `TrimTrailingWhitespace`, `DeleteLines`, `IndentLines`, `OutdentLines`,
//! `InsertLineBefore/After`, `DeleteAllLeft/Right`, `JoinLines`, `Transpose` e as de caixa: `UpperCase`,
//! `LowerCase`, `TitleCase`, `SnakeCase`, `CamelCase`, `PascalCase`, `KebabCase`) e as chamadas de registro do fim
//! do arquivo ([`register_lines_operations`]).
//!
//! Representação:
//! - `abstract class AbstractCopyLinesAction` (com `CopyLinesUpAction` e `CopyLinesDownAction`, que só diferem em
//!   dados) é a struct [`CopyLinesAction`] com o `down` como campo, e as classes concretas são as funções
//!   [`copy_lines_up_action`] e [`copy_lines_down_action`]. O mesmo vale para [`MoveLinesAction`] e
//!   [`SortLinesAction`] (`descending`).
//! - `abstract class AbstractDeleteAllToBoundaryAction` é a struct [`DeleteAllToBoundaryAction`] genérica sobre o
//!   trait [`DeleteAllBoundary`] (os dois métodos abstratos `_getEndCursorState` e `_getRangesToDelete`), com
//!   [`DeleteAllLeft`] e [`DeleteAllRight`] como implementações. `abstract class AbstractCaseAction` é a struct
//!   [`CaseAction`] genérica sobre o trait [`CaseTransform`] (o `_modifyText` abstrato).
//! - `BackwardsCompatibleRegExp` é a struct de mesmo nome sobre o `zyarr::RegExp`. Os campos `static` das classes
//!   (`TitleCaseAction.titleBoundary`, `SnakeCaseAction.caseBoundary`...) são campos de cada [`CaseTransform`], e a
//!   checagem `isSupported()` do registro é [`CaseTransform::is_supported`] (cada ação confere só as regex que o
//!   original confere).
//! - `String.prototype.replace` com `$1_$2` é [`replace_template`], e `String.prototype.split` com regex é
//!   [`split_by_regexp`] (o laço de `RegExp.prototype[Symbol.split]`). As colunas e os comprimentos são unidades
//!   UTF-16, como no original.
//! - `KeyMod.CtrlCmd | KeyCode.X` é `key(modifiers, key_code)` de `core_commands`. `KeyChord(a, b)` é `key_chord`.
//!
//! Divergências:
//! - **Serviço de configuração de linguagem.** `accessor.get(ILanguageConfigurationService)` de
//!   `AbstractMoveLinesAction` lê o serviço que o editor recebeu ([`CodeEditorWidget::language_configuration_service`]),
//!   porque o serviço ainda não tem identificador no porte (a mesma decisão de `word_operations`).
//! - `IDeleteLinesOperation.selectionStartColumn` é calculado no original mas nunca lido, e por isso não existe aqui.
//! - `TrimTrailingWhitespaceAction.run` exige o `accessor` (o original lê o `IConfigurationService` dele e falharia
//!   sem ele); sem `accessor` o porte para com uma mensagem.
//! - `toLocaleUpperCase()`/`toLocaleLowerCase()` sem locale são a conversão Unicode completa de
//!   `JsString::to_upper_case`/`to_lower_case`.

use std::cell::{Cell, OnceCell};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::js_regexp::replace_all;
use je_base::js_string::is_js_whitespace;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::action::localize2;
use je_platform::actions::menu_ids;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::I_CONFIGURATION_SERVICE;
use je_platform::contextkey::ContextKeyExpression;
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};
use zyarr::RegExp;

use crate::code_editor_widget::{
    CodeEditorWidget, EndCursorState, ExecuteEditsSource, ICodeEditor, IEditorCommand,
};
use crate::config::editor_options::registry::{AUTO_INDENT, WORD_SEPARATORS};
use crate::copy_lines_command::CopyLinesCommand;
use crate::core_commands::{core_editing_commands, editor_text_focus, key, platform_keybindings, text_input_focus, writable};
use crate::cursor_common::CursorSimpleModel;
use crate::cursor_type_edit_operations::EnterOperation;
use crate::cursor_type_operations::TypeOperations;
use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::editor_common::Command as EditCommand;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandMenuOptions, CommandOptions, EditorAction,
    EditorActionBase, ServicesAccessor, register_editor_action, register_instantiated_editor_action,
};
use crate::move_lines_command::MoveLinesCommand;
use crate::position::Position;
use crate::range::Range;
use crate::replace_command::{ReplaceCommand, ReplaceCommandThatPreservesSelection, ReplaceCommandThatSelectsText};
use crate::selection::Selection;
use crate::sort_lines_command::SortLinesCommand;
use crate::text_model::TextModel;
use crate::trim_trailing_whitespace_command::TrimTrailingWhitespaceCommand;
use crate::view_model_types::IViewModel;

type Commands = Vec<Option<Box<dyn EditCommand>>>;

// #region Auxiliares

/// Um comando como o `ICommand | null` de `executeCommands`.
fn boxed(command: impl EditCommand + 'static) -> Option<Box<dyn EditCommand>> {
    Some(Box::new(command))
}

/// `ICommand[]` como o `(ICommand | null)[]` de `executeCommands`.
fn nullable(commands: Vec<Box<dyn EditCommand>>) -> Commands {
    commands.into_iter().map(Some).collect()
}

/// `Range.compareRangesUsingStarts` como o `Ordering` que o `sort` do porte pede.
fn compare_ranges_using_starts(a: &Range, b: &Range) -> Ordering {
    Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0)
}

/// `new Selection(1, 1, model.getLineCount(), model.getLineMaxColumn(model.getLineCount()))`: o documento todo.
fn whole_document(model: &TextModel) -> Selection {
    let line_count = model.get_line_count();
    Selection::new(1, 1, line_count, model.get_line_max_column(line_count))
}

/// `selections.length === 1 && selections[0].isSingleLine()`: o `applies to whole document` das ações de ordenar,
/// remover duplicadas e inverter. Devolve o documento todo no lugar da seleção, ou `None` quando ela vale como está.
fn whole_document_if_single_line(model: &TextModel, selections: &[Selection]) -> Option<Vec<Selection>> {
    match selections {
        [only] if only.is_single_line() => Some(vec![whole_document(model)]),
        _ => None,
    }
}

/// `text.split('').reverse().join('')`: as unidades UTF-16 em ordem inversa.
fn reverse_units(text: &JsString) -> JsString {
    JsString::from_units(text.units().iter().rev().copied().collect())
}

/// `text.join('\n')`.
fn join_lines(lines: &[JsString]) -> JsString {
    let mut joined = JsString::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            joined.push_unit(u16::from(b'\n'));
        }
        joined.push_units(line.units());
    }
    joined
}

/// `IActionOptions` de uma ação com precondição `writable` e `canTriggerInlineEdits: true`.
fn writable_command(id: &str) -> CommandOptions {
    CommandOptions { precondition: Some(writable()), can_trigger_inline_edits: true, ..CommandOptions::new(id) }
}

/// O rótulo `nls.localize2(key, label)` de uma ação.
fn lines_action_options(command: CommandOptions, label_key: &str, label: &str) -> ActionOptions {
    ActionOptions {
        command,
        context_menu_opts: Vec::new(),
        label: ActionLabel::Localized { label: localize2(label_key, label), alias: None },
    }
}

/// O `kbOpts` com `weight: KeybindingWeight.EditorContrib`.
fn editor_contrib_keybindings(kb_expr: ContextKeyExpression, keybindings: Keybindings) -> Vec<CommandKeybindingsOptions> {
    vec![CommandKeybindingsOptions::new(KeybindingWeight::EDITOR_CONTRIB, Some(kb_expr), keybindings)]
}

/// O `menuOpts` do menu `Selection` com o grupo `2_line`.
fn selection_menu_item(order: f64, label_key: &str, label: &str) -> Vec<CommandMenuOptions> {
    vec![CommandMenuOptions::new(menu_ids::menubar_selection_menu(), "2_line", order, localize(label_key, label, &[]))]
}

/// Os atalhos de `Alt+Shift+Seta` e `Alt+Seta` de copiar e mover: `primary` com `modifiers` mais a tecla, e a
/// versão do Linux que o original declara.
fn arrow_keybindings(primary_modifiers: u32, linux_modifiers: u32, key_code: KeyCode) -> Vec<CommandKeybindingsOptions> {
    editor_contrib_keybindings(
        editor_text_focus(),
        Keybindings {
            primary: Some(key(primary_modifiers, key_code)),
            linux: platform_keybindings(key(linux_modifiers, key_code), None),
            ..Keybindings::default()
        },
    )
}

// #endregion

// #region Copiar linhas

/// `abstract class AbstractCopyLinesAction` (com `CopyLinesUpAction` e `CopyLinesDownAction`).
pub struct CopyLinesAction {
    action: EditorActionBase,
    down: bool,
}

impl CopyLinesAction {
    pub fn new(down: bool, opts: ActionOptions) -> Self {
        Self { action: EditorActionBase::new(opts), down }
    }
}

/// Uma seleção com o índice original, para `AbstractCopyLinesAction.run`.
struct IndexedSelection {
    selection: Selection,
    index: usize,
    ignore: bool,
}

impl EditorAction for CopyLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(selections) = editor.get_selections().filter(|_| editor.has_model()) else {
            return Promise::resolved(());
        };

        let mut selections: Vec<IndexedSelection> = selections
            .into_iter()
            .enumerate()
            .map(|(index, selection)| IndexedSelection { selection, index, ignore: false })
            .collect();
        selections.sort_by(|a, b| compare_ranges_using_starts(&a.selection, &b.selection));

        // Remove selections that would result in copying the same line
        let mut prev = 0;
        for current in 1..selections.len() {
            if selections[prev].selection.end_line_number == selections[current].selection.start_line_number {
                // these two selections would copy the same line
                if selections[prev].index < selections[current].index {
                    // prev wins
                    selections[current].ignore = true;
                } else {
                    // curr wins
                    selections[prev].ignore = true;
                    prev = current;
                }
            }
        }

        let commands: Commands = selections
            .iter()
            .map(|selection| boxed(CopyLinesCommand::new(selection.selection, self.down, Some(selection.ignore))))
            .collect();

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

/// `CopyLinesUpAction`.
pub fn copy_lines_up_action() -> CopyLinesAction {
    CopyLinesAction::new(
        false,
        lines_action_options(
            CommandOptions {
                kb_opts: arrow_keybindings(KeyMod::ALT | KeyMod::SHIFT, KeyMod::CTRL_CMD | KeyMod::ALT | KeyMod::SHIFT, KeyCode::UpArrow),
                menu_opts: selection_menu_item(1.0, "miCopyLinesUp", "&&Copy Line Up"),
                ..writable_command("editor.action.copyLinesUpAction")
            },
            "lines.copyUp",
            "Copy Line Up",
        ),
    )
}

/// `CopyLinesDownAction`.
pub fn copy_lines_down_action() -> CopyLinesAction {
    CopyLinesAction::new(
        true,
        lines_action_options(
            CommandOptions {
                kb_opts: arrow_keybindings(KeyMod::ALT | KeyMod::SHIFT, KeyMod::CTRL_CMD | KeyMod::ALT | KeyMod::SHIFT, KeyCode::DownArrow),
                menu_opts: selection_menu_item(2.0, "miCopyLinesDown", "Co&&py Line Down"),
                ..writable_command("editor.action.copyLinesDownAction")
            },
            "lines.copyDown",
            "Copy Line Down",
        ),
    )
}

/// `DuplicateSelectionAction`.
pub struct DuplicateSelectionAction {
    action: EditorActionBase,
}

impl DuplicateSelectionAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                CommandOptions {
                    menu_opts: selection_menu_item(5.0, "miDuplicateSelection", "&&Duplicate Selection"),
                    ..writable_command("editor.action.duplicateSelection")
                },
                "duplicateSelection",
                "Duplicate Selection",
            )),
        }
    }
}

impl Default for DuplicateSelectionAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for DuplicateSelectionAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return Promise::resolved(());
        };

        let commands: Commands = selections
            .iter()
            .map(|selection| {
                if selection.is_empty() {
                    boxed(CopyLinesCommand::new(*selection, true, None))
                } else {
                    let insert_position = Range::new(
                        selection.end_line_number,
                        selection.end_column,
                        selection.end_line_number,
                        selection.end_column,
                    );
                    boxed(ReplaceCommandThatSelectsText::new(insert_position, model.get_value_in_range(selection)))
                }
            })
            .collect();

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

// #endregion

// #region Mover linhas

/// `abstract class AbstractMoveLinesAction` (com `MoveLinesUpAction` e `MoveLinesDownAction`).
pub struct MoveLinesAction {
    action: EditorActionBase,
    down: bool,
}

impl MoveLinesAction {
    pub fn new(down: bool, opts: ActionOptions) -> Self {
        Self { action: EditorActionBase::new(opts), down }
    }
}

impl EditorAction for MoveLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let language_configuration_service = editor.language_configuration_service();

        // `editor.getSelections() || []`
        let selections = editor.get_selections().unwrap_or_default();
        let auto_indent = *editor.get_option(AUTO_INDENT);

        let commands: Commands = selections
            .iter()
            .map(|selection| {
                boxed(MoveLinesCommand::new(*selection, self.down, auto_indent, language_configuration_service.clone()))
            })
            .collect();

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

/// `MoveLinesUpAction`.
pub fn move_lines_up_action() -> MoveLinesAction {
    MoveLinesAction::new(
        false,
        lines_action_options(
            CommandOptions {
                kb_opts: arrow_keybindings(KeyMod::ALT, KeyMod::ALT, KeyCode::UpArrow),
                menu_opts: selection_menu_item(3.0, "miMoveLinesUp", "Mo&&ve Line Up"),
                ..writable_command("editor.action.moveLinesUpAction")
            },
            "lines.moveUp",
            "Move Line Up",
        ),
    )
}

/// `MoveLinesDownAction`.
pub fn move_lines_down_action() -> MoveLinesAction {
    MoveLinesAction::new(
        true,
        lines_action_options(
            CommandOptions {
                kb_opts: arrow_keybindings(KeyMod::ALT, KeyMod::ALT, KeyCode::DownArrow),
                menu_opts: selection_menu_item(4.0, "miMoveLinesDown", "Move &&Line Down"),
                ..writable_command("editor.action.moveLinesDownAction")
            },
            "lines.moveDown",
            "Move Line Down",
        ),
    )
}

// #endregion

// #region Ordenar, remover duplicadas e inverter

/// `abstract class AbstractSortLinesAction` (com `SortLinesAscendingAction` e `SortLinesDescendingAction`).
pub struct SortLinesAction {
    action: EditorActionBase,
    descending: bool,
}

impl SortLinesAction {
    pub fn new(descending: bool, opts: ActionOptions) -> Self {
        Self { action: EditorActionBase::new(opts), descending }
    }
}

impl EditorAction for SortLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(model), Some(mut selections)) = (editor.get_model(), editor.get_selections()) else {
            return Promise::resolved(());
        };
        if let Some(whole) = whole_document_if_single_line(&model, &selections) {
            // Apply to whole document.
            selections = whole;
        }

        for selection in &selections {
            if !SortLinesCommand::can_run(Some(&*model), selection, self.descending) {
                return Promise::resolved(());
            }
        }

        let commands: Commands =
            selections.iter().map(|selection| boxed(SortLinesCommand::new(*selection, self.descending))).collect();

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

/// `SortLinesAscendingAction`.
pub fn sort_lines_ascending_action() -> SortLinesAction {
    SortLinesAction::new(
        false,
        lines_action_options(writable_command("editor.action.sortLinesAscending"), "lines.sortAscending", "Sort Lines Ascending"),
    )
}

/// `SortLinesDescendingAction`.
pub fn sort_lines_descending_action() -> SortLinesAction {
    SortLinesAction::new(
        true,
        lines_action_options(writable_command("editor.action.sortLinesDescending"), "lines.sortDescending", "Sort Lines Descending"),
    )
}

/// `DeleteDuplicateLinesAction`.
pub struct DeleteDuplicateLinesAction {
    action: EditorActionBase,
}

impl DeleteDuplicateLinesAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                writable_command("editor.action.removeDuplicateLines"),
                "lines.deleteDuplicates",
                "Delete Duplicate Lines",
            )),
        }
    }
}

impl Default for DeleteDuplicateLinesAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for DeleteDuplicateLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(model), Some(mut selections)) = (editor.get_model(), editor.get_selections()) else {
            return Promise::resolved(());
        };
        if model.get_line_count() == 1 && model.get_line_max_column(1) == 1 {
            return Promise::resolved(());
        }

        let mut edits: Vec<SingleEditOperation> = Vec::new();
        let mut end_cursor_state: Vec<Selection> = Vec::new();

        let mut lines_deleted = 0;
        let mut update_selection = true;

        if let Some(whole) = whole_document_if_single_line(&model, &selections) {
            // Apply to whole document.
            selections = whole;
            update_selection = false;
        }

        for selection in &selections {
            let mut unique_lines: HashSet<JsString> = HashSet::new();
            let mut lines: Vec<JsString> = Vec::new();

            for line_number in selection.start_line_number..=selection.end_line_number {
                let line = model.get_line_content(line_number);

                if unique_lines.contains(&line) {
                    continue;
                }

                unique_lines.insert(line.clone());
                lines.push(line);
            }

            let selection_to_replace = Range::new(
                selection.start_line_number,
                1,
                selection.end_line_number,
                model.get_line_max_column(selection.end_line_number),
            );

            let adjusted_selection_start = selection.start_line_number - lines_deleted;
            let lines_count = lines.len() as i32;
            let last_line_length = lines.last().map_or(0, JsString::len) as i32;
            let final_selection = Selection::new(
                adjusted_selection_start,
                1,
                adjusted_selection_start + lines_count - 1,
                last_line_length + 1,
            );

            edits.push(EditOperation::replace(selection_to_replace, Some(join_lines(&lines))));
            end_cursor_state.push(final_selection);

            lines_deleted += (selection.end_line_number - selection.start_line_number + 1) - lines_count;
        }

        editor.push_undo_stop();
        editor.execute_edits(
            ExecuteEditsSource::Name(Some(&self.action.command.id)),
            edits,
            update_selection.then_some(EndCursorState::Selections(end_cursor_state)),
        );
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

/// `ReverseLinesAction`.
pub struct ReverseLinesAction {
    action: EditorActionBase,
}

impl ReverseLinesAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                writable_command("editor.action.reverseLines"),
                "lines.reverseLines",
                "Reverse lines",
            )),
        }
    }
}

impl Default for ReverseLinesAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for ReverseLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(model), Some(original_selections)) = (editor.get_model(), editor.get_selections()) else {
            return Promise::resolved(());
        };
        // Apply to whole document.
        let selections = whole_document_if_single_line(&model, &original_selections).unwrap_or_else(|| original_selections.clone());

        let mut edits: Vec<SingleEditOperation> = Vec::new();
        let mut resulting_selections: Vec<Selection> = Vec::new();

        for (selection, original_selection) in selections.iter().zip(&original_selections) {
            let mut end_line_number = selection.end_line_number;
            if selection.start_line_number < selection.end_line_number && selection.end_column == 1 {
                end_line_number -= 1;
            }

            let mut range = Range::new(selection.start_line_number, 1, end_line_number, model.get_line_max_column(end_line_number));

            // Exclude last line if empty and we're at the end of the document
            if end_line_number == model.get_line_count() && model.get_line_content(range.end_line_number).is_empty() {
                range = range.set_end_position(range.end_line_number - 1, model.get_line_max_column(range.end_line_number - 1));
            }

            let lines: Vec<JsString> =
                (range.start_line_number..=range.end_line_number).rev().map(|line_number| model.get_line_content(line_number)).collect();
            edits.push(EditOperation::replace(range, Some(join_lines(&lines))));

            let update_line_number = |line_number: i32| {
                if line_number <= range.end_line_number {
                    range.end_line_number - line_number + range.start_line_number
                } else {
                    line_number
                }
            };
            resulting_selections.push(if original_selection.is_empty() {
                // keep just the cursor
                let line_number = update_line_number(original_selection.position_line_number);
                Selection::new(line_number, original_selection.position_column, line_number, original_selection.position_column)
            } else {
                // keep selection - maintain direction by creating backward selection
                Selection::new(
                    update_line_number(original_selection.selection_start_line_number),
                    original_selection.selection_start_column,
                    update_line_number(original_selection.position_line_number),
                    original_selection.position_column,
                )
            });
        }

        editor.push_undo_stop();
        editor.execute_edits(
            ExecuteEditsSource::Name(Some(&self.action.command.id)),
            edits,
            Some(EndCursorState::Selections(resulting_selections)),
        );
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

// #endregion

// #region Aparar espaço no fim

/// `TrimTrailingWhitespaceAction`.
pub struct TrimTrailingWhitespaceAction {
    action: EditorActionBase,
}

impl TrimTrailingWhitespaceAction {
    /// `TrimTrailingWhitespaceAction.ID`.
    pub const ID: &'static str = "editor.action.trimTrailingWhitespace";

    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                CommandOptions {
                    precondition: Some(writable()),
                    kb_opts: editor_contrib_keybindings(
                        editor_text_focus(),
                        Keybindings {
                            primary: Some(key_chord(
                                key(KeyMod::CTRL_CMD, KeyCode::KeyK),
                                key(KeyMod::CTRL_CMD, KeyCode::KeyX),
                            )),
                            ..Keybindings::default()
                        },
                    ),
                    ..CommandOptions::new(Self::ID)
                },
                "lines.trimTrailingWhitespace",
                "Trim Trailing Whitespace",
            )),
        }
    }
}

impl Default for TrimTrailingWhitespaceAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for TrimTrailingWhitespaceAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    /// O `args` é o `TrimTrailingWhitespaceArgs`: `reason?: 'auto-save'`.
    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let mut cursors: Vec<Position> = Vec::new();
        let is_auto_save = matches!(args, Dynamic::Object(_))
            && matches!(args.get_str("reason"), Dynamic::String(reason) if reason == "auto-save");
        if is_auto_save {
            // See https://github.com/editorconfig/editorconfig-vscode/issues/47
            // It is very convenient for the editor config extension to invoke this action.
            // So, if we get a reason:'auto-save' passed in, let's preserve cursor positions.
            cursors = editor
                .get_selections()
                .unwrap_or_default()
                .iter()
                .map(|selection| Position::new(selection.position_line_number, selection.position_column))
                .collect();
        }

        let Some(selection) = editor.get_selection() else {
            return Promise::resolved(());
        };

        let accessor = accessor.expect("TrimTrailingWhitespaceAction precisa do accessor para ler o IConfigurationService");
        let config = accessor.get(I_CONFIGURATION_SERVICE);
        let model = editor.get_model();
        let overrides = ConfigurationOverrides {
            override_identifier: model.as_ref().map(|model| JsString::from(model.get_language_id())),
            resource: model.as_ref().map(|model| model.uri()),
        };
        let trim_in_regex_and_strings =
            config.get_value(Some("files.trimTrailingWhitespaceInRegexAndStrings"), Some(&overrides)).is_truthy();

        let command = TrimTrailingWhitespaceCommand::new(selection, cursors, trim_in_regex_and_strings);

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), vec![boxed(command)]);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

// #endregion

// #region Apagar linhas

/// `IDeleteLinesOperation`.
#[derive(Clone, Copy)]
struct DeleteLinesOperation {
    start_line_number: i32,
    end_line_number: i32,
    position_column: i32,
}

/// `DeleteLinesAction`.
pub struct DeleteLinesAction {
    action: EditorActionBase,
}

impl DeleteLinesAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                CommandOptions {
                    kb_opts: editor_contrib_keybindings(
                        text_input_focus(),
                        Keybindings {
                            primary: Some(key(KeyMod::CTRL_CMD | KeyMod::SHIFT, KeyCode::KeyK)),
                            ..Keybindings::default()
                        },
                    ),
                    ..writable_command("editor.action.deleteLines")
                },
                "lines.delete",
                "Delete Line",
            )),
        }
    }

    /// `_getLinesToRemove(editor)`: as operações de apagar, ordenadas e com as adjacentes ou sobrepostas fundidas.
    fn get_lines_to_remove(selections: &[Selection]) -> Vec<DeleteLinesOperation> {
        // Construct delete operations
        let mut operations: Vec<DeleteLinesOperation> = selections
            .iter()
            .map(|selection| {
                let mut end_line_number = selection.end_line_number;
                if selection.start_line_number < selection.end_line_number && selection.end_column == 1 {
                    end_line_number -= 1;
                }

                DeleteLinesOperation {
                    start_line_number: selection.start_line_number,
                    end_line_number,
                    position_column: selection.position_column,
                }
            })
            .collect();

        // Sort delete operations
        operations.sort_by(|a, b| {
            if a.start_line_number == b.start_line_number {
                return a.end_line_number.cmp(&b.end_line_number);
            }
            a.start_line_number.cmp(&b.start_line_number)
        });

        // Merge delete operations which are adjacent or overlapping
        let mut merged_operations: Vec<DeleteLinesOperation> = Vec::new();
        let mut previous_operation = operations[0];
        for operation in &operations[1..] {
            if previous_operation.end_line_number + 1 >= operation.start_line_number {
                // Merge current operations into the previous one
                previous_operation.end_line_number = operation.end_line_number;
            } else {
                // Push previous operation
                merged_operations.push(previous_operation);
                previous_operation = *operation;
            }
        }
        // Push the last operation
        merged_operations.push(previous_operation);

        merged_operations
    }
}

impl Default for DeleteLinesAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for DeleteLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return Promise::resolved(());
        };

        let ops = Self::get_lines_to_remove(&selections);

        if model.get_line_count() == 1 && model.get_line_max_column(1) == 1 {
            // Model is empty
            return Promise::resolved(());
        }

        let mut lines_deleted = 0;
        let mut edits: Vec<SingleEditOperation> = Vec::new();
        let mut cursor_state: Vec<Selection> = Vec::new();
        for op in &ops {
            let mut start_line_number = op.start_line_number;
            let mut end_line_number = op.end_line_number;

            let mut start_column = 1;
            let mut end_column = model.get_line_max_column(end_line_number);
            if end_line_number < model.get_line_count() {
                end_line_number += 1;
                end_column = 1;
            } else if start_line_number > 1 {
                start_line_number -= 1;
                start_column = model.get_line_max_column(start_line_number);
            }

            edits.push(EditOperation::replace(
                Range::new(start_line_number, start_column, end_line_number, end_column),
                Some(JsString::new()),
            ));
            cursor_state.push(Selection::new(
                start_line_number - lines_deleted,
                op.position_column,
                start_line_number - lines_deleted,
                op.position_column,
            ));
            lines_deleted += op.end_line_number - op.start_line_number + 1;
        }

        editor.push_undo_stop();
        editor.execute_edits(
            ExecuteEditsSource::Name(Some(&self.action.command.id)),
            edits,
            Some(EndCursorState::Selections(cursor_state)),
        );
        editor.reveal_all_cursors(true, None);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

// #endregion

// #region Indentar e desindentar

/// `IndentLinesAction`.
pub struct IndentLinesAction {
    action: EditorActionBase,
}

impl IndentLinesAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                CommandOptions {
                    kb_opts: editor_contrib_keybindings(
                        editor_text_focus(),
                        Keybindings {
                            primary: Some(key(KeyMod::CTRL_CMD, KeyCode::BracketRight)),
                            ..Keybindings::default()
                        },
                    ),
                    ..writable_command("editor.action.indentLines")
                },
                "lines.indent",
                "Indent Line",
            )),
        }
    }
}

impl Default for IndentLinesAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for IndentLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(view_model) = editor.get_view_model() else {
            return Promise::resolved(());
        };
        let model = editor.get_model();
        let selections = editor.get_selections();
        editor.push_undo_stop();
        editor.execute_commands(
            Some(&self.action.command.id),
            nullable(TypeOperations::indent(
                &view_model.cursor_config(),
                model.as_deref().map(|model| model as &dyn CursorSimpleModel),
                selections.as_deref(),
            )),
        );
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

/// `OutdentLinesAction`: roda o comando `outdent` do núcleo.
pub struct OutdentLinesAction {
    action: EditorActionBase,
}

impl OutdentLinesAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                CommandOptions {
                    kb_opts: editor_contrib_keybindings(
                        editor_text_focus(),
                        Keybindings {
                            primary: Some(key(KeyMod::CTRL_CMD, KeyCode::BracketLeft)),
                            ..Keybindings::default()
                        },
                    ),
                    ..writable_command("editor.action.outdentLines")
                },
                "lines.outdent",
                "Outdent Line",
            )),
        }
    }
}

impl Default for OutdentLinesAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for OutdentLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        core_editing_commands::instance().outdent.run_editor_command(accessor, editor, &Dynamic::Null)
    }
}

// #endregion

// #region Inserir linha

/// `InsertLineBeforeAction` e `InsertLineAfterAction`: só diferem no `lineInsertBefore`/`lineInsertAfter`.
pub struct InsertLineAction {
    action: EditorActionBase,
    before: bool,
}

impl InsertLineAction {
    /// `InsertLineBeforeAction.ID`.
    pub const BEFORE_ID: &'static str = "editor.action.insertLineBefore";
    /// `InsertLineAfterAction.ID`.
    pub const AFTER_ID: &'static str = "editor.action.insertLineAfter";

    fn new(before: bool, id: &str, key_modifiers: u32, label_key: &str, label: &str) -> Self {
        Self {
            before,
            action: EditorActionBase::new(lines_action_options(
                CommandOptions {
                    kb_opts: editor_contrib_keybindings(
                        editor_text_focus(),
                        Keybindings { primary: Some(key(key_modifiers, KeyCode::Enter)), ..Keybindings::default() },
                    ),
                    ..writable_command(id)
                },
                label_key,
                label,
            )),
        }
    }
}

impl EditorAction for InsertLineAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(view_model) = editor.get_view_model() else {
            return Promise::resolved(());
        };
        let model = editor.get_model();
        let selections = editor.get_selections();
        editor.push_undo_stop();
        let config = view_model.cursor_config();
        let commands = if self.before {
            EnterOperation::line_insert_before(&config, model.as_deref(), selections.as_deref())
        } else {
            EnterOperation::line_insert_after(&config, model.as_deref(), selections.as_deref())
        };
        editor.execute_commands(Some(&self.action.command.id), nullable(commands));
        Promise::resolved(())
    }
}

/// `InsertLineBeforeAction`.
pub fn insert_line_before_action() -> InsertLineAction {
    InsertLineAction::new(
        true,
        InsertLineAction::BEFORE_ID,
        KeyMod::CTRL_CMD | KeyMod::SHIFT,
        "lines.insertBefore",
        "Insert Line Above",
    )
}

/// `InsertLineAfterAction`.
pub fn insert_line_after_action() -> InsertLineAction {
    InsertLineAction::new(false, InsertLineAction::AFTER_ID, KeyMod::CTRL_CMD, "lines.insertAfter", "Insert Line Below")
}

// #endregion

// #region Apagar tudo à esquerda e à direita

/// Os dois métodos abstratos de `AbstractDeleteAllToBoundaryAction`.
pub trait DeleteAllBoundary {
    /// `_getEndCursorState(primaryCursor, rangesToDelete)`: o estado do cursor depois das operações.
    fn get_end_cursor_state(&self, primary_cursor: &Range, ranges_to_delete: &[Range]) -> Vec<Selection>;

    /// `_getRangesToDelete(editor)`.
    fn get_ranges_to_delete(&self, editor: &Rc<CodeEditorWidget>) -> Vec<Range>;
}

/// `abstract class AbstractDeleteAllToBoundaryAction`.
pub struct DeleteAllToBoundaryAction<B: DeleteAllBoundary> {
    action: EditorActionBase,
    boundary: B,
}

impl<B: DeleteAllBoundary> DeleteAllToBoundaryAction<B> {
    pub fn new(opts: ActionOptions, boundary: B) -> Self {
        Self { action: EditorActionBase::new(opts), boundary }
    }
}

impl<B: DeleteAllBoundary> EditorAction for DeleteAllToBoundaryAction<B> {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(primary_cursor), true) = (editor.get_selection(), editor.has_model()) else {
            return Promise::resolved(());
        };

        let mut ranges_to_delete = self.boundary.get_ranges_to_delete(editor);
        // merge overlapping selections
        let mut effective_ranges: Vec<Range> = Vec::new();

        for index in 0..ranges_to_delete.len().saturating_sub(1) {
            let range = ranges_to_delete[index];
            let next_range = ranges_to_delete[index + 1];

            if range.intersect_ranges(&next_range).is_none() {
                effective_ranges.push(range);
            } else {
                ranges_to_delete[index + 1] = range.plus_range(&next_range);
            }
        }

        let Some(last_range) = ranges_to_delete.last() else {
            return Promise::resolved(());
        };
        effective_ranges.push(*last_range);

        let end_cursor_state = self.boundary.get_end_cursor_state(&primary_cursor, &effective_ranges);

        let edits: Vec<SingleEditOperation> =
            effective_ranges.iter().map(|range| EditOperation::replace(*range, Some(JsString::new()))).collect();

        editor.push_undo_stop();
        editor.execute_edits(
            ExecuteEditsSource::Name(Some(&self.action.command.id)),
            edits,
            Some(EndCursorState::Selections(end_cursor_state)),
        );
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

/// A seleção colapsada no começo do intervalo: `new Selection(line, column, line, column)`.
fn collapsed_at(line_number: i32, column: i32) -> Selection {
    Selection::new(line_number, column, line_number, column)
}

/// O fim do `_getEndCursorState`: o cursor primário primeiro (`unshift`) e depois os demais.
fn primary_first(end_primary_cursor: Option<Selection>, mut end_cursor_state: Vec<Selection>) -> Vec<Selection> {
    if let Some(primary) = end_primary_cursor {
        end_cursor_state.insert(0, primary);
    }
    end_cursor_state
}

/// `DeleteAllLeftAction`: o `_getEndCursorState` e o `_getRangesToDelete` da esquerda.
pub struct DeleteAllLeft;

impl DeleteAllBoundary for DeleteAllLeft {
    fn get_end_cursor_state(&self, primary_cursor: &Range, ranges_to_delete: &[Range]) -> Vec<Selection> {
        let mut end_primary_cursor: Option<Selection> = None;
        let mut end_cursor_state: Vec<Selection> = Vec::new();
        let mut deleted_lines = 0;

        for range in ranges_to_delete {
            let end_cursor = if range.end_column == 1 && deleted_lines > 0 {
                collapsed_at(range.start_line_number - deleted_lines, range.start_column)
            } else {
                collapsed_at(range.start_line_number, range.start_column)
            };

            deleted_lines += range.end_line_number - range.start_line_number;

            if range.intersect_ranges(primary_cursor).is_some() {
                end_primary_cursor = Some(end_cursor);
            } else {
                end_cursor_state.push(end_cursor);
            }
        }

        primary_first(end_primary_cursor, end_cursor_state)
    }

    fn get_ranges_to_delete(&self, editor: &Rc<CodeEditorWidget>) -> Vec<Range> {
        let (Some(mut selections), Some(model)) = (editor.get_selections(), editor.get_model()) else {
            return Vec::new();
        };

        selections.sort_by(|a, b| compare_ranges_using_starts(a, b));
        selections
            .iter()
            .map(|selection| {
                if selection.is_empty() {
                    if selection.start_column == 1 {
                        let delete_from_line = (selection.start_line_number - 1).max(1);
                        let delete_from_column =
                            if selection.start_line_number == 1 { 1 } else { model.get_line_length(delete_from_line) as i32 + 1 };
                        Range::new(delete_from_line, delete_from_column, selection.start_line_number, 1)
                    } else {
                        Range::new(selection.start_line_number, 1, selection.start_line_number, selection.start_column)
                    }
                } else {
                    Range::new(selection.start_line_number, 1, selection.end_line_number, selection.end_column)
                }
            })
            .collect()
    }
}

/// `DeleteAllRightAction`: o `_getEndCursorState` e o `_getRangesToDelete` da direita.
pub struct DeleteAllRight;

impl DeleteAllBoundary for DeleteAllRight {
    fn get_end_cursor_state(&self, primary_cursor: &Range, ranges_to_delete: &[Range]) -> Vec<Selection> {
        let mut end_primary_cursor: Option<Selection> = None;
        let mut end_cursor_state: Vec<Selection> = Vec::new();
        for range in ranges_to_delete {
            // O `offset` do original começa em 0 e nunca muda.
            let end_cursor = collapsed_at(range.start_line_number, range.start_column);

            if range.intersect_ranges(primary_cursor).is_some() {
                end_primary_cursor = Some(end_cursor);
            } else {
                end_cursor_state.push(end_cursor);
            }
        }

        primary_first(end_primary_cursor, end_cursor_state)
    }

    fn get_ranges_to_delete(&self, editor: &Rc<CodeEditorWidget>) -> Vec<Range> {
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return Vec::new();
        };

        let mut ranges_to_delete: Vec<Range> = selections
            .iter()
            .map(|selection| {
                if selection.is_empty() {
                    let max_column = model.get_line_max_column(selection.start_line_number);

                    if selection.start_column == max_column {
                        Range::new(selection.start_line_number, selection.start_column, selection.start_line_number + 1, 1)
                    } else {
                        Range::new(selection.start_line_number, selection.start_column, selection.start_line_number, max_column)
                    }
                } else {
                    **selection
                }
            })
            .collect();

        ranges_to_delete.sort_by(compare_ranges_using_starts);
        ranges_to_delete
    }
}

/// `DeleteAllLeftAction`.
pub fn delete_all_left_action() -> DeleteAllToBoundaryAction<DeleteAllLeft> {
    DeleteAllToBoundaryAction::new(
        lines_action_options(
            CommandOptions {
                kb_opts: editor_contrib_keybindings(
                    text_input_focus(),
                    Keybindings {
                        primary: Some(0),
                        mac: platform_keybindings(key(KeyMod::CTRL_CMD, KeyCode::Backspace), None),
                        ..Keybindings::default()
                    },
                ),
                ..writable_command("deleteAllLeft")
            },
            "lines.deleteAllLeft",
            "Delete All Left",
        ),
        DeleteAllLeft,
    )
}

/// `DeleteAllRightAction`.
pub fn delete_all_right_action() -> DeleteAllToBoundaryAction<DeleteAllRight> {
    DeleteAllToBoundaryAction::new(
        lines_action_options(
            CommandOptions {
                kb_opts: editor_contrib_keybindings(
                    text_input_focus(),
                    Keybindings {
                        primary: Some(0),
                        mac: platform_keybindings(
                            key(KeyMod::WIN_CTRL, KeyCode::KeyK),
                            Some(vec![key(KeyMod::CTRL_CMD, KeyCode::Delete)]),
                        ),
                        ..Keybindings::default()
                    },
                ),
                ..writable_command("deleteAllRight")
            },
            "lines.deleteAllRight",
            "Delete All Right",
        ),
        DeleteAllRight,
    )
}

// #endregion

// #region Juntar linhas

/// `/[\s﻿\xA0]+$/g` trocado por `' '`: o espaço em branco do fim vira um espaço só.
fn replace_trailing_whitespace_with_space(text: &JsString) -> JsString {
    let units = text.units();
    let trailing = units.iter().rev().take_while(|unit| is_js_whitespace(**unit)).count();
    let mut replaced = JsString::from_units(units[..units.len() - trailing].to_vec());
    if trailing > 0 {
        replaced.push_unit(u16::from(b' '));
    }
    replaced
}

/// `JoinLinesAction`.
pub struct JoinLinesAction {
    action: EditorActionBase,
}

impl JoinLinesAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                CommandOptions {
                    kb_opts: editor_contrib_keybindings(
                        editor_text_focus(),
                        Keybindings {
                            primary: Some(0),
                            mac: platform_keybindings(key(KeyMod::WIN_CTRL, KeyCode::KeyJ), None),
                            ..Keybindings::default()
                        },
                    ),
                    ..writable_command("editor.action.joinLines")
                },
                "lines.joinLines",
                "Join Lines",
            )),
        }
    }

    /// O `selections.reduce(...)` do original: junta as seleções que tocam a mesma linha (ou a seguinte, se vazias).
    /// O cursor primário passa para a seleção que absorveu a dele.
    fn reduce_selections(selections: &[Selection], primary_cursor: &mut Selection) -> Vec<Selection> {
        let mut reduced_selections: Vec<Selection> = Vec::new();
        let mut previous_value = selections[0];

        for current_value in &selections[1..] {
            let current_value = *current_value;
            previous_value = if previous_value.is_empty() {
                if previous_value.end_line_number == current_value.start_line_number {
                    if *primary_cursor == previous_value {
                        *primary_cursor = current_value;
                    }
                    current_value
                } else if current_value.start_line_number > previous_value.end_line_number + 1 {
                    reduced_selections.push(previous_value);
                    current_value
                } else {
                    Selection::new(
                        previous_value.start_line_number,
                        previous_value.start_column,
                        current_value.end_line_number,
                        current_value.end_column,
                    )
                }
            } else if current_value.start_line_number > previous_value.end_line_number {
                reduced_selections.push(previous_value);
                current_value
            } else {
                Selection::new(
                    previous_value.start_line_number,
                    previous_value.start_column,
                    current_value.end_line_number,
                    current_value.end_column,
                )
            };
        }

        reduced_selections.push(previous_value);
        reduced_selections
    }
}

impl Default for JoinLinesAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for JoinLinesAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(mut selections), Some(mut primary_cursor)) = (editor.get_selections(), editor.get_selection()) else {
            return Promise::resolved(());
        };

        selections.sort_by(|a, b| compare_ranges_using_starts(a, b));
        let reduced_selections = Self::reduce_selections(&selections, &mut primary_cursor);

        let Some(model) = editor.get_model() else {
            return Promise::resolved(());
        };

        let mut edits: Vec<SingleEditOperation> = Vec::new();
        let mut end_cursor_state: Vec<Selection> = Vec::new();
        let mut end_primary_cursor = primary_cursor;
        let mut line_offset = 0;

        for selection in &reduced_selections {
            let start_line_number = selection.start_line_number;
            let start_column = 1;
            let mut column_delta_offset = 0;
            let end_line_number;
            let end_column;

            let selection_end_position_offset = model.get_line_length(selection.end_line_number) as i32 - selection.end_column;

            if selection.is_empty() || selection.start_line_number == selection.end_line_number {
                let position = selection.get_start_position();
                if position.line_number < model.get_line_count() {
                    end_line_number = start_line_number + 1;
                    end_column = model.get_line_max_column(end_line_number);
                } else {
                    end_line_number = position.line_number;
                    end_column = model.get_line_max_column(position.line_number);
                }
            } else {
                end_line_number = selection.end_line_number;
                end_column = model.get_line_max_column(end_line_number);
            }

            let mut trimmed_lines_content = model.get_line_content(start_line_number);

            for line_number in start_line_number + 1..=end_line_number {
                let line_text = model.get_line_content(line_number);
                let first_non_whitespace_idx = model.get_line_first_non_whitespace_column(line_number);

                if first_non_whitespace_idx >= 1 {
                    let mut insert_space = true;
                    if trimmed_lines_content.is_empty() {
                        insert_space = false;
                    }

                    if insert_space
                        && matches!(trimmed_lines_content.units().last(), Some(&unit) if unit == u16::from(b' ') || unit == u16::from(b'\t'))
                    {
                        insert_space = false;
                        trimmed_lines_content = replace_trailing_whitespace_with_space(&trimmed_lines_content);
                    }

                    let line_text_without_indent = &line_text.units()[(first_non_whitespace_idx - 1) as usize..];

                    if insert_space {
                        trimmed_lines_content.push_unit(u16::from(b' '));
                    }
                    trimmed_lines_content.push_units(line_text_without_indent);

                    column_delta_offset = line_text_without_indent.len() as i32 + i32::from(insert_space);
                } else {
                    column_delta_offset = 0;
                }
            }

            let delete_selection = Range::new(start_line_number, start_column, end_line_number, end_column);

            if !delete_selection.is_empty() {
                let content_length = trimmed_lines_content.len() as i32;
                let result_selection = if selection.is_empty() {
                    edits.push(EditOperation::replace(delete_selection, Some(trimmed_lines_content)));
                    collapsed_at(delete_selection.start_line_number - line_offset, content_length - column_delta_offset + 1)
                } else if selection.start_line_number == selection.end_line_number {
                    edits.push(EditOperation::replace(delete_selection, Some(trimmed_lines_content)));
                    Selection::new(
                        selection.start_line_number - line_offset,
                        selection.start_column,
                        selection.end_line_number - line_offset,
                        selection.end_column,
                    )
                } else {
                    edits.push(EditOperation::replace(delete_selection, Some(trimmed_lines_content)));
                    Selection::new(
                        selection.start_line_number - line_offset,
                        selection.start_column,
                        selection.start_line_number - line_offset,
                        content_length - selection_end_position_offset,
                    )
                };

                if delete_selection.intersect_ranges(&primary_cursor).is_some() {
                    end_primary_cursor = result_selection;
                } else {
                    end_cursor_state.push(result_selection);
                }
            }

            line_offset += delete_selection.end_line_number - delete_selection.start_line_number;
        }

        end_cursor_state.insert(0, end_primary_cursor);
        editor.push_undo_stop();
        editor.execute_edits(
            ExecuteEditsSource::Name(Some(&self.action.command.id)),
            edits,
            Some(EndCursorState::Selections(end_cursor_state)),
        );
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

// #endregion

// #region Transpor

/// `TransposeAction`.
pub struct TransposeAction {
    action: EditorActionBase,
}

impl TransposeAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(lines_action_options(
                writable_command("editor.action.transpose"),
                "editor.transpose",
                "Transpose Characters around the Cursor",
            )),
        }
    }
}

impl Default for TransposeAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for TransposeAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(selections), Some(model)) = (editor.get_selections(), editor.get_model()) else {
            return Promise::resolved(());
        };

        let mut commands: Commands = Vec::new();

        for selection in &selections {
            if !selection.is_empty() {
                continue;
            }

            let cursor = selection.get_start_position();
            let max_column = model.get_line_max_column(cursor.line_number);

            if cursor.column >= max_column {
                if cursor.line_number == model.get_line_count() {
                    continue;
                }

                // The cursor is at the end of current line and current line is not empty
                // then we transpose the character before the cursor and the line break if there is any following line.
                let delete_selection = Range::new(cursor.line_number, (cursor.column - 1).max(1), cursor.line_number + 1, 1);
                let chars = reverse_units(&model.get_value_in_range(&delete_selection));

                commands.push(boxed(ReplaceCommand::new(delete_selection, chars, None)));
            } else {
                let delete_selection = Range::new(cursor.line_number, (cursor.column - 1).max(1), cursor.line_number, cursor.column + 1);
                let chars = reverse_units(&model.get_value_in_range(&delete_selection));
                commands.push(boxed(ReplaceCommandThatPreservesSelection::new(
                    delete_selection,
                    chars,
                    collapsed_at(cursor.line_number, cursor.column + 1),
                    None,
                )));
            }
        }

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

// #endregion

// #region Caixa do texto

/// `BackwardsCompatibleRegExp`: a regex criada (e avaliada) só no primeiro uso; `None` se o motor não a aceita.
pub struct BackwardsCompatibleRegExp {
    pattern: &'static str,
    flags: &'static str,
    actual: OnceCell<Option<RegExp>>,
}

impl BackwardsCompatibleRegExp {
    pub fn new(pattern: &'static str, flags: &'static str) -> Self {
        Self { pattern, flags, actual: OnceCell::new() }
    }

    /// `get()`.
    pub fn get(&self) -> Option<&RegExp> {
        self.actual
            .get_or_init(|| {
                let pattern: Vec<u16> = self.pattern.encode_utf16().collect();
                let flags: Vec<u16> = self.flags.encode_utf16().collect();
                // this browser does not support this regular expression
                RegExp::new(&pattern, &flags).ok()
            })
            .as_ref()
    }

    /// `isSupported()`.
    pub fn is_supported(&self) -> bool {
        self.get().is_some()
    }
}

/// `regex.test(text)` de uma regex sem a flag `g`: casa em algum ponto do texto?
fn regexp_test(regexp: &RegExp, text: &JsString) -> bool {
    regexp.exec(text.units(), 0).is_some()
}

/// `text.replace(regexp, template)` com os `$1`, `$2`... do `String.prototype.replace`.
fn replace_template(regexp: &RegExp, text: &JsString, template: &str) -> JsString {
    replace_all(regexp, text.units(), |captures| {
        let mut replacement = JsString::new();
        let mut chars = template.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '$' {
                if let Some(group) = chars.peek().and_then(|next| next.to_digit(10)) {
                    chars.next();
                    replacement.push_units(captures.group(group as usize).unwrap_or_default());
                    continue;
                }
            }
            let mut buffer = [0u16; 2];
            replacement.push_units(character.encode_utf16(&mut buffer));
        }
        replacement
    })
}

/// `text.split(regexp)` para uma regex sem grupos de captura: o laço de `RegExp.prototype[Symbol.split]`, que tenta
/// casar em cada posição `q` e ignora o casamento vazio que termina onde o último pedaço começou.
fn split_by_regexp(regexp: &RegExp, text: &JsString) -> Vec<JsString> {
    let input = text.units();
    let size = input.len();
    if size == 0 {
        return if regexp.exec(input, 0).is_some() { Vec::new() } else { vec![JsString::new()] };
    }

    let mut pieces: Vec<JsString> = Vec::new();
    let mut piece_start = 0;
    let mut position = 0;
    while position < size {
        // Procurar a partir de `position` equivale a tentar cada posição até a do primeiro casamento.
        let Some(found) = regexp.exec(input, position) else { break };
        let match_start = found.start();
        if match_start >= size {
            break;
        }
        let match_end = found.end().min(size);
        if match_end == piece_start {
            position = match_start + 1;
            continue;
        }
        pieces.push(text.slice(piece_start, match_start));
        piece_start = match_end;
        position = piece_start;
    }
    pieces.push(text.slice(piece_start, size));
    pieces
}

/// `word.charAt(0).toLocaleUpperCase() + word.slice(1)`: a primeira unidade em maiúscula e o resto como está.
fn capitalize_first_unit(word: &[u16]) -> JsString {
    let first = word.len().min(1);
    let mut capitalized = JsString::from_units(word[..first].to_vec()).to_upper_case();
    capitalized.push_units(&word[first..]);
    capitalized
}

/// O `_modifyText(text, wordSeparators)` abstrato de `AbstractCaseAction`.
pub trait CaseTransform {
    fn modify_text(&self, text: &JsString, word_separators: &JsString) -> JsString;

    /// O teste de `registerEditorAction` do fim do arquivo: as regex que a ação usa existem neste motor?
    fn is_supported(&self) -> bool {
        true
    }
}

/// `abstract class AbstractCaseAction`.
pub struct CaseAction<T: CaseTransform> {
    action: EditorActionBase,
    transform: T,
}

impl<T: CaseTransform> CaseAction<T> {
    pub fn new(opts: ActionOptions, transform: T) -> Self {
        Self { action: EditorActionBase::new(opts), transform }
    }

    /// `SnakeCaseAction.caseBoundary.isSupported() && ...` e as condições parecidas do registro.
    pub fn is_supported(&self) -> bool {
        self.transform.is_supported()
    }
}

impl<T: CaseTransform> EditorAction for CaseAction<T> {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(selections), Some(model)) = (editor.get_selections(), editor.get_model()) else {
            return Promise::resolved(());
        };

        let word_separators = JsString::from_units(editor.get_option(WORD_SEPARATORS).encode_utf16().collect());
        let mut text_edits: Vec<SingleEditOperation> = Vec::new();

        for selection in &selections {
            if selection.is_empty() {
                let cursor = selection.get_start_position();
                let Some(word) = editor.get_configured_word_at_position(cursor) else {
                    continue;
                };

                let word_range = Range::new(cursor.line_number, word.start_column, cursor.line_number, word.end_column);
                let text = model.get_value_in_range(&word_range);
                text_edits.push(EditOperation::replace(word_range, Some(self.transform.modify_text(&text, &word_separators))));
            } else {
                let text = model.get_value_in_range(selection);
                text_edits.push(EditOperation::replace(**selection, Some(self.transform.modify_text(&text, &word_separators))));
            }
        }

        editor.push_undo_stop();
        editor.execute_edits(ExecuteEditsSource::Name(Some(&self.action.command.id)), text_edits, None);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

/// As opções de uma ação de caixa: só `precondition`, `canTriggerInlineEdits` e o rótulo.
fn case_action_options(id: &str, label_key: &str, label: &str) -> ActionOptions {
    lines_action_options(writable_command(id), label_key, label)
}

/// O `_modifyText` de `UpperCaseAction`.
pub struct UpperCase;

impl CaseTransform for UpperCase {
    fn modify_text(&self, text: &JsString, _word_separators: &JsString) -> JsString {
        text.to_upper_case()
    }
}

/// `UpperCaseAction`.
pub fn upper_case_action() -> CaseAction<UpperCase> {
    CaseAction::new(
        case_action_options("editor.action.transformToUppercase", "editor.transformToUppercase", "Transform to Uppercase"),
        UpperCase,
    )
}

/// O `_modifyText` de `LowerCaseAction`.
pub struct LowerCase;

impl CaseTransform for LowerCase {
    fn modify_text(&self, text: &JsString, _word_separators: &JsString) -> JsString {
        text.to_lower_case()
    }
}

/// `LowerCaseAction`.
pub fn lower_case_action() -> CaseAction<LowerCase> {
    CaseAction::new(
        case_action_options("editor.action.transformToLowercase", "editor.transformToLowercase", "Transform to Lowercase"),
        LowerCase,
    )
}

/// O `_modifyText` de `TitleCaseAction`.
pub struct TitleCase {
    /// `TitleCaseAction.titleBoundary`.
    pub title_boundary: BackwardsCompatibleRegExp,
}

impl TitleCase {
    pub fn new() -> Self {
        Self { title_boundary: BackwardsCompatibleRegExp::new(r"(^|[^\p{L}\p{N}']|((^|\P{L})'))\p{L}", "gmu") }
    }
}

impl Default for TitleCase {
    fn default() -> Self {
        Self::new()
    }
}

impl CaseTransform for TitleCase {
    fn modify_text(&self, text: &JsString, _word_separators: &JsString) -> JsString {
        let Some(title_boundary) = self.title_boundary.get() else {
            // cannot support this
            return text.clone();
        };
        replace_all(title_boundary, text.to_lower_case().units(), |captures| JsString::from(captures.whole()).to_upper_case())
    }

    fn is_supported(&self) -> bool {
        self.title_boundary.is_supported()
    }
}

/// `TitleCaseAction`.
pub fn title_case_action() -> CaseAction<TitleCase> {
    CaseAction::new(
        case_action_options("editor.action.transformToTitlecase", "editor.transformToTitlecase", "Transform to Title Case"),
        TitleCase::new(),
    )
}

/// O `_modifyText` de `SnakeCaseAction`.
pub struct SnakeCase {
    /// `SnakeCaseAction.caseBoundary`.
    pub case_boundary: BackwardsCompatibleRegExp,
    /// `SnakeCaseAction.singleLetters`.
    pub single_letters: BackwardsCompatibleRegExp,
}

impl SnakeCase {
    pub fn new() -> Self {
        Self {
            case_boundary: BackwardsCompatibleRegExp::new(r"(\p{Ll})(\p{Lu})", "gmu"),
            single_letters: BackwardsCompatibleRegExp::new(r"(\p{Lu}|\p{N})(\p{Lu})(\p{Ll})", "gmu"),
        }
    }
}

impl Default for SnakeCase {
    fn default() -> Self {
        Self::new()
    }
}

impl CaseTransform for SnakeCase {
    fn modify_text(&self, text: &JsString, _word_separators: &JsString) -> JsString {
        let (Some(case_boundary), Some(single_letters)) = (self.case_boundary.get(), self.single_letters.get()) else {
            // cannot support this
            return text.clone();
        };
        let separated = replace_template(case_boundary, text, "$1_$2");
        replace_template(single_letters, &separated, "$1_$2$3").to_lower_case()
    }

    fn is_supported(&self) -> bool {
        self.case_boundary.is_supported() && self.single_letters.is_supported()
    }
}

/// `SnakeCaseAction`.
pub fn snake_case_action() -> CaseAction<SnakeCase> {
    CaseAction::new(
        case_action_options("editor.action.transformToSnakecase", "editor.transformToSnakecase", "Transform to Snake Case"),
        SnakeCase::new(),
    )
}

/// O `_modifyText` de `CamelCaseAction`.
pub struct CamelCase {
    /// `CamelCaseAction.singleLineWordBoundary`.
    pub single_line_word_boundary: BackwardsCompatibleRegExp,
    /// `CamelCaseAction.multiLineWordBoundary`.
    pub multi_line_word_boundary: BackwardsCompatibleRegExp,
    /// `CamelCaseAction.validWordStart`.
    pub valid_word_start: BackwardsCompatibleRegExp,
}

impl CamelCase {
    pub fn new() -> Self {
        Self {
            single_line_word_boundary: BackwardsCompatibleRegExp::new(r"[_\s-]+", "gm"),
            multi_line_word_boundary: BackwardsCompatibleRegExp::new(r"[_-]+", "gm"),
            valid_word_start: BackwardsCompatibleRegExp::new(r"^(\p{Lu}[^\p{Lu}])", "gmu"),
        }
    }
}

impl Default for CamelCase {
    fn default() -> Self {
        Self::new()
    }
}

impl CaseTransform for CamelCase {
    fn modify_text(&self, text: &JsString, _word_separators: &JsString) -> JsString {
        let is_multi_line = text.units().iter().any(|unit| *unit == u16::from(b'\n') || *unit == u16::from(b'\r'));
        let word_boundary =
            if is_multi_line { self.multi_line_word_boundary.get() } else { self.single_line_word_boundary.get() };
        let (Some(word_boundary), Some(valid_word_start)) = (word_boundary, self.valid_word_start.get()) else {
            // cannot support this
            return text.clone();
        };
        let mut words = split_by_regexp(word_boundary, text).into_iter();
        // `split` sempre devolve ao menos um pedaço.
        let first_word = words.next().unwrap_or_default();
        let mut camel_case = replace_all(valid_word_start, first_word.units(), |captures| {
            JsString::from(captures.whole()).to_lower_case()
        });
        for word in words {
            camel_case.push_units(capitalize_first_unit(word.units()).units());
        }
        camel_case
    }

    fn is_supported(&self) -> bool {
        self.single_line_word_boundary.is_supported() && self.multi_line_word_boundary.is_supported()
    }
}

/// `CamelCaseAction`.
pub fn camel_case_action() -> CaseAction<CamelCase> {
    CaseAction::new(
        case_action_options("editor.action.transformToCamelcase", "editor.transformToCamelcase", "Transform to Camel Case"),
        CamelCase::new(),
    )
}

/// O `_modifyText` de `PascalCaseAction`.
pub struct PascalCase {
    /// `PascalCaseAction.wordBoundary`.
    pub word_boundary: BackwardsCompatibleRegExp,
    /// `PascalCaseAction.wordBoundaryToMaintain`.
    pub word_boundary_to_maintain: BackwardsCompatibleRegExp,
    /// `PascalCaseAction.upperCaseWordMatcher`.
    pub upper_case_word_matcher: BackwardsCompatibleRegExp,
}

impl PascalCase {
    pub fn new() -> Self {
        Self {
            word_boundary: BackwardsCompatibleRegExp::new(r"[_ \t-]", "gm"),
            word_boundary_to_maintain: BackwardsCompatibleRegExp::new(r"(?<=\.)", "gm"),
            upper_case_word_matcher: BackwardsCompatibleRegExp::new(r"^\p{Lu}+$", "mu"),
        }
    }
}

impl Default for PascalCase {
    fn default() -> Self {
        Self::new()
    }
}

impl CaseTransform for PascalCase {
    fn modify_text(&self, text: &JsString, _word_separators: &JsString) -> JsString {
        let (Some(word_boundary), Some(word_boundary_to_maintain), Some(upper_case_word_matcher)) =
            (self.word_boundary.get(), self.word_boundary_to_maintain.get(), self.upper_case_word_matcher.get())
        else {
            // cannot support this
            return text.clone();
        };

        let words: Vec<JsString> = split_by_regexp(word_boundary_to_maintain, text)
            .iter()
            .flat_map(|word| split_by_regexp(word_boundary, word))
            .collect();

        let mut pascal_case = JsString::new();
        for word in &words {
            let normalized_word = capitalize_first_unit(word.units());
            let is_all_caps = normalized_word.len() > 1 && regexp_test(upper_case_word_matcher, &normalized_word);
            if is_all_caps {
                let units = normalized_word.units();
                pascal_case.push_units(&units[..1]);
                pascal_case.push_units(JsString::from(&units[1..]).to_lower_case().units());
            } else {
                pascal_case.push_units(normalized_word.units());
            }
        }
        pascal_case
    }

    fn is_supported(&self) -> bool {
        self.word_boundary.is_supported()
    }
}

/// `PascalCaseAction`.
pub fn pascal_case_action() -> CaseAction<PascalCase> {
    CaseAction::new(
        case_action_options("editor.action.transformToPascalcase", "editor.transformToPascalcase", "Transform to Pascal Case"),
        PascalCase::new(),
    )
}

/// O `_modifyText` de `KebabCaseAction`.
pub struct KebabCase {
    case_boundary: BackwardsCompatibleRegExp,
    single_letters: BackwardsCompatibleRegExp,
    underscore_boundary: BackwardsCompatibleRegExp,
}

impl KebabCase {
    pub fn new() -> Self {
        Self {
            case_boundary: BackwardsCompatibleRegExp::new(r"(\p{Ll})(\p{Lu})", "gmu"),
            single_letters: BackwardsCompatibleRegExp::new(r"(\p{Lu}|\p{N})(\p{Lu}\p{Ll})", "gmu"),
            underscore_boundary: BackwardsCompatibleRegExp::new(r"(\S)(_)(\S)", "gm"),
        }
    }
}

impl Default for KebabCase {
    fn default() -> Self {
        Self::new()
    }
}

impl CaseTransform for KebabCase {
    fn modify_text(&self, text: &JsString, _word_separators: &JsString) -> JsString {
        let (Some(case_boundary), Some(single_letters), Some(underscore_boundary)) =
            (self.case_boundary.get(), self.single_letters.get(), self.underscore_boundary.get())
        else {
            // one or more regexps aren't supported
            return text.clone();
        };

        let without_underscores = replace_template(underscore_boundary, text, "$1-$3");
        let with_case_boundaries = replace_template(case_boundary, &without_underscores, "$1-$2");
        replace_template(single_letters, &with_case_boundaries, "$1-$2").to_lower_case()
    }

    /// `KebabCaseAction.isSupported()`.
    fn is_supported(&self) -> bool {
        [&self.case_boundary, &self.single_letters, &self.underscore_boundary].iter().all(|regexp| regexp.is_supported())
    }
}

/// `KebabCaseAction`.
pub fn kebab_case_action() -> CaseAction<KebabCase> {
    CaseAction::new(
        case_action_options("editor.action.transformToKebabcase", "editor.transformToKebabcase", "Transform to Kebab Case"),
        KebabCase::new(),
    )
}

// #endregion

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro de ações é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Registra a ação de caixa só se as regex dela existem neste motor (os `if (X.isSupported())` do fim do arquivo).
fn register_case_action_if_supported<T: CaseTransform + 'static>(action: CaseAction<T>) {
    if action.is_supported() {
        register_instantiated_editor_action(Rc::new(action));
    }
}

/// As chamadas `registerEditorAction(...)` do fim do arquivo (o que o carregamento do módulo faz no original). É
/// idempotente: o registro acontece uma vez por thread.
pub fn register_lines_operations() {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_action(copy_lines_up_action);
    register_editor_action(copy_lines_down_action);
    register_editor_action(DuplicateSelectionAction::new);
    register_editor_action(move_lines_up_action);
    register_editor_action(move_lines_down_action);
    register_editor_action(sort_lines_ascending_action);
    register_editor_action(sort_lines_descending_action);
    register_editor_action(DeleteDuplicateLinesAction::new);
    register_editor_action(TrimTrailingWhitespaceAction::new);
    register_editor_action(DeleteLinesAction::new);
    register_editor_action(IndentLinesAction::new);
    register_editor_action(OutdentLinesAction::new);
    register_editor_action(insert_line_before_action);
    register_editor_action(insert_line_after_action);
    register_editor_action(delete_all_left_action);
    register_editor_action(delete_all_right_action);
    register_editor_action(JoinLinesAction::new);
    register_editor_action(TransposeAction::new);
    register_editor_action(upper_case_action);
    register_editor_action(lower_case_action);
    register_editor_action(ReverseLinesAction::new);

    register_case_action_if_supported(snake_case_action());
    register_case_action_if_supported(camel_case_action());
    register_case_action_if_supported(pascal_case_action());
    register_case_action_if_supported(title_case_action());
    register_case_action_if_supported(kebab_case_action());
}
