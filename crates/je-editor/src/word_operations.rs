//! Porte de `src/vs/editor/contrib/wordOperations/browser/wordOperations.ts`: os comandos de mover o cursor por
//! palavra (`MoveWordCommand` e as suas `CursorWord...`), os de apagar por palavra (`DeleteWordCommand`,
//! `DeleteWordLeft`, `DeleteWordRight`...), a ação `DeleteInsideWord` e as chamadas de registro do fim do arquivo.
//!
//! As operações em si são as de [`crate::cursor_word_operations`] (`WordOperations`); o registro é o de
//! [`crate::editor_extensions`]. O que o carregamento do módulo faz no original (os `registerEditorCommand` e o
//! `registerEditorAction`) é [`register_word_operations`].
//!
//! Representação:
//! - `abstract class MoveWordCommand` com as subclasses `WordLeftCommand`/`WordRightCommand` (que só diferem no
//!   `_move`) é a struct [`MoveWordCommand`], com a direção ([`WordDirection`]) como campo. As classes concretas
//!   (`CursorWordStartLeft`...) só diferem em dados e viram funções que montam o comando
//!   ([`cursor_word_start_left`]...). As quatro `CursorWordAccessibility...` sobrescrevem o `_move` para trocar os
//!   separadores pelo valor padrão da opção `wordSeparators` (mantendo os locales do segmentador): é o campo
//!   [`MoveWordOptions::accessibility`].
//! - `abstract class DeleteWordCommand` com `DeleteWordLeftCommand`/`DeleteWordRightCommand` é a struct
//!   [`DeleteWordCommand`], também com a direção como campo, e as concretas (`DeleteWordStartLeft`...) são funções
//!   ([`delete_word_start_left`]...).
//! - `MoveWordOptions`/`DeleteWordOptions` (que estendem `ICommandOptions`) são [`MoveWordOptions`] e
//!   [`DeleteWordOptions`], com o `ICommandOptions` no campo `command`.
//! - `DeleteInsideWord extends EditorAction` é a struct [`DeleteInsideWord`], que implementa o trait [`EditorAction`].
//! - `KeyMod.CtrlCmd | KeyCode.X` é `key(modifiers, key_code)` de `core_commands`. `IsWindowsContext` e
//!   `CONTEXT_ACCESSIBILITY_MODE_ENABLED` (chaves `isWindows` e `accessibilityModeEnabled`) são as expressões
//!   `has(chave)`, como as demais chaves de contexto do porte (ver `core_commands`).
//!
//! Divergências:
//! - **Serviço de configuração de linguagem.** `accessor?.get(ILanguageConfigurationService)` do
//!   `DeleteWordCommand` lê o serviço que o editor recebeu ([`CodeEditorWidget::language_configuration_service`]),
//!   porque o serviço ainda não tem identificador no porte. Sem `accessor` o comando não faz nada, como no original.
//! - `viewModel.getCursorAutoClosedCharacters()` é lido uma vez antes do laço das seleções (o original o lê dentro
//!   do `map`), porque nada muda o estado do cursor até o `executeCommands`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::action::localize2;
use je_platform::commands::{CommandArgument, CommandDescription, CommandMetadata};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor, IEditorCommand};
use crate::config::editor_options::registry::{
    AUTO_CLOSING_BRACKETS, AUTO_CLOSING_DELETE, AUTO_CLOSING_QUOTES, WORD_SEPARATORS,
};
use crate::config::editor_options::word_segmenter_locales::WORD_SEGMENTER_LOCALES;
use crate::core_commands::{key, platform_keybindings, text_input_focus, writable};
use crate::cursor_common::{CursorSimpleModel, CursorState, PartialCursorState};
use crate::cursor_word_operations::{DeleteWordContext, WordNavigationType, WordOperations};
use crate::editor_common::{Command as EditCommand, ScrollType};
use crate::editor_extensions::{
    ActionLabel, ActionOptions, Command, CommandKeybindingsOptions, CommandOptions, EditorAction, EditorActionBase,
    EditorCommand, ServicesAccessor, register_editor_action, register_editor_command,
};
use crate::position::Position;
use crate::range::Range;
use crate::replace_command::ReplaceCommand;
use crate::selection::Selection;
use crate::view_model_types::{CursorChangeReason, IViewModel};
use crate::word_character_classifier::{WordCharacterClassifier, get_map_for_word_separators};
use crate::word_helper::USUAL_WORD_SEPARATORS;

/// O lado para onde o comando anda: `WordLeftCommand` ou `WordRightCommand` (e, nos de apagar,
/// `DeleteWordLeftCommand` ou `DeleteWordRightCommand`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordDirection {
    Left,
    Right,
}

/// `getMapForWordSeparators(editor.getOption(wordSeparators), editor.getOption(wordSegmenterLocales))`.
fn word_separators_of(editor: &CodeEditorWidget) -> Rc<WordCharacterClassifier> {
    let separators: Vec<u16> = editor.get_option(WORD_SEPARATORS).encode_utf16().collect();
    get_map_for_word_separators(&separators, editor.get_option(WORD_SEGMENTER_LOCALES))
}

// #region MoveWordCommand

/// `MoveWordOptions`: as opções do comando mais `inSelectionMode` e `wordNavigationType` (e a direção e o
/// `accessibility`, que no original são a subclasse).
pub struct MoveWordOptions {
    pub command: CommandOptions,
    pub in_selection_mode: bool,
    pub word_navigation_type: WordNavigationType,
    pub direction: WordDirection,
    /// As `CursorWordAccessibility...`: usam os separadores padrão da opção `wordSeparators`.
    pub accessibility: bool,
}

impl MoveWordOptions {
    /// As opções de um comando sem atalho nem precondição (`precondition: undefined`).
    pub fn new(
        id: &str,
        direction: WordDirection,
        in_selection_mode: bool,
        word_navigation_type: WordNavigationType,
    ) -> Self {
        Self {
            command: CommandOptions::new(id),
            in_selection_mode,
            word_navigation_type,
            direction,
            accessibility: false,
        }
    }
}

/// `abstract class MoveWordCommand extends EditorCommand` (com `WordLeftCommand` e `WordRightCommand`).
pub struct MoveWordCommand {
    base: Command,
    in_selection_mode: bool,
    word_navigation_type: WordNavigationType,
    direction: WordDirection,
    accessibility: bool,
}

impl MoveWordCommand {
    pub fn new(opts: MoveWordOptions) -> Self {
        Self {
            base: Command::new(opts.command),
            in_selection_mode: opts.in_selection_mode,
            word_navigation_type: opts.word_navigation_type,
            direction: opts.direction,
            accessibility: opts.accessibility,
        }
    }

    /// `_moveTo(from, to, inSelectionMode)`.
    fn move_to(from: &Selection, to: Position, in_selection_mode: bool) -> Selection {
        if in_selection_mode {
            // move just position
            Selection::new(from.selection_start_line_number, from.selection_start_column, to.line_number, to.column)
        } else {
            // move everything
            Selection::new(to.line_number, to.column, to.line_number, to.column)
        }
    }

    /// `_move(wordSeparators, model, position, wordNavigationType, hasMulticursor)` de `WordLeftCommand` e de
    /// `WordRightCommand`, com o desvio das `CursorWordAccessibility...`.
    fn move_position(
        &self,
        word_separators: &WordCharacterClassifier,
        model: &dyn CursorSimpleModel,
        position: Position,
        has_multicursor: bool,
    ) -> Position {
        let default_separators;
        let word_separators = if self.accessibility {
            let usual: Vec<u16> = USUAL_WORD_SEPARATORS.encode_utf16().collect();
            default_separators = get_map_for_word_separators(&usual, &word_separators.intl_segmenter_locales);
            &*default_separators
        } else {
            word_separators
        };
        match self.direction {
            WordDirection::Left => {
                WordOperations::move_word_left(word_separators, model, position, self.word_navigation_type, has_multicursor)
            }
            WordDirection::Right => {
                WordOperations::move_word_right(word_separators, model, position, self.word_navigation_type)
            }
        }
    }
}

impl IEditorCommand for MoveWordCommand {
    fn run_editor_command(
        &self,
        _accessor: Option<&ServicesAccessor>,
        editor: &Rc<CodeEditorWidget>,
        _args: &Dynamic,
    ) -> Promise<()> {
        let (Some(model), Some(selections), Some(view_model)) =
            (editor.get_model(), editor.get_selections(), editor.get_view_model())
        else {
            return Promise::resolved(());
        };
        let word_separators = word_separators_of(editor);
        let has_multicursor = selections.len() > 1;
        let result: Vec<Selection> = selections
            .iter()
            .map(|sel| {
                let in_position = Position::new(sel.position_line_number, sel.position_column);
                let out_position = self.move_position(&word_separators, &*model, in_position, has_multicursor);
                Self::move_to(sel, out_position, self.in_selection_mode)
            })
            .collect();

        model.push_stack_element();
        view_model.set_cursor_states(
            Some("moveWordCommand"),
            CursorChangeReason::Explicit,
            Some(result.iter().map(|r| PartialCursorState::Model(CursorState::from_model_selection(*r))).collect()),
        );
        if let [only] = result.as_slice() {
            editor.reveal_position(Position::new(only.position_line_number, only.position_column), Some(ScrollType::Smooth));
        }
        Promise::resolved(())
    }
}

impl EditorCommand for MoveWordCommand {
    fn base(&self) -> &Command {
        &self.base
    }
}

/// `kbExpr` dos atalhos de cursor por palavra: `ContextKeyExpr.and(EditorContextKeys.textInputFocus,
/// ContextKeyExpr.and(CONTEXT_ACCESSIBILITY_MODE_ENABLED, IsWindowsContext)?.negate())`.
fn move_word_keybindings_when() -> Option<ContextKeyExpression> {
    ContextKeyExpr::and([
        Some(text_input_focus()),
        ContextKeyExpr::and([Some(ContextKeyExpr::has("accessibilityModeEnabled")), Some(ContextKeyExpr::has("isWindows"))])
            .map(|expression| expression.negate()),
    ])
}

/// O `kbOpts` dos comandos de cursor por palavra: `CtrlCmd` (e `Alt` no mac) mais `modifiers` e a tecla.
fn move_word_keybindings(key_code: KeyCode, modifiers: u32) -> Vec<CommandKeybindingsOptions> {
    vec![CommandKeybindingsOptions::new(
        KeybindingWeight::EDITOR_CONTRIB,
        move_word_keybindings_when(),
        Keybindings {
            primary: Some(key(KeyMod::CTRL_CMD | modifiers, key_code)),
            mac: platform_keybindings(key(KeyMod::ALT | modifiers, key_code), None),
            ..Keybindings::default()
        },
    )]
}

/// As opções de um comando de mover por palavra com `kbOpts`.
fn move_word_options_with_keybindings(
    id: &str,
    direction: WordDirection,
    in_selection_mode: bool,
    word_navigation_type: WordNavigationType,
    kb_opts: Vec<CommandKeybindingsOptions>,
) -> MoveWordOptions {
    MoveWordOptions {
        command: CommandOptions { kb_opts, ..CommandOptions::new(id) },
        ..MoveWordOptions::new(id, direction, in_selection_mode, word_navigation_type)
    }
}

/// `CursorWordStartLeft`.
pub fn cursor_word_start_left() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions::new(
        "cursorWordStartLeft",
        WordDirection::Left,
        false,
        WordNavigationType::WordStart,
    ))
}

/// `CursorWordEndLeft`.
pub fn cursor_word_end_left() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions::new("cursorWordEndLeft", WordDirection::Left, false, WordNavigationType::WordEnd))
}

/// `CursorWordLeft`.
pub fn cursor_word_left() -> MoveWordCommand {
    MoveWordCommand::new(move_word_options_with_keybindings(
        "cursorWordLeft",
        WordDirection::Left,
        false,
        WordNavigationType::WordStartFast,
        move_word_keybindings(KeyCode::LeftArrow, 0),
    ))
}

/// `CursorWordStartLeftSelect`.
pub fn cursor_word_start_left_select() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions::new(
        "cursorWordStartLeftSelect",
        WordDirection::Left,
        true,
        WordNavigationType::WordStart,
    ))
}

/// `CursorWordEndLeftSelect`.
pub fn cursor_word_end_left_select() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions::new(
        "cursorWordEndLeftSelect",
        WordDirection::Left,
        true,
        WordNavigationType::WordEnd,
    ))
}

/// `CursorWordLeftSelect`.
pub fn cursor_word_left_select() -> MoveWordCommand {
    MoveWordCommand::new(move_word_options_with_keybindings(
        "cursorWordLeftSelect",
        WordDirection::Left,
        true,
        WordNavigationType::WordStartFast,
        move_word_keybindings(KeyCode::LeftArrow, KeyMod::SHIFT),
    ))
}

/// `CursorWordAccessibilityLeft`: os comandos de acessibilidade só devem valer no Windows, porque foram
/// ajustados ao que o NVDA espera.
pub fn cursor_word_accessibility_left() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions {
        accessibility: true,
        ..MoveWordOptions::new("cursorWordAccessibilityLeft", WordDirection::Left, false, WordNavigationType::WordAccessibility)
    })
}

/// `CursorWordAccessibilityLeftSelect`.
pub fn cursor_word_accessibility_left_select() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions {
        accessibility: true,
        ..MoveWordOptions::new(
            "cursorWordAccessibilityLeftSelect",
            WordDirection::Left,
            true,
            WordNavigationType::WordAccessibility,
        )
    })
}

/// `CursorWordStartRight`.
pub fn cursor_word_start_right() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions::new(
        "cursorWordStartRight",
        WordDirection::Right,
        false,
        WordNavigationType::WordStart,
    ))
}

/// `CursorWordEndRight`.
pub fn cursor_word_end_right() -> MoveWordCommand {
    MoveWordCommand::new(move_word_options_with_keybindings(
        "cursorWordEndRight",
        WordDirection::Right,
        false,
        WordNavigationType::WordEnd,
        move_word_keybindings(KeyCode::RightArrow, 0),
    ))
}

/// `CursorWordRight`.
pub fn cursor_word_right() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions::new("cursorWordRight", WordDirection::Right, false, WordNavigationType::WordEnd))
}

/// `CursorWordStartRightSelect`.
pub fn cursor_word_start_right_select() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions::new(
        "cursorWordStartRightSelect",
        WordDirection::Right,
        true,
        WordNavigationType::WordStart,
    ))
}

/// `CursorWordEndRightSelect`.
pub fn cursor_word_end_right_select() -> MoveWordCommand {
    MoveWordCommand::new(move_word_options_with_keybindings(
        "cursorWordEndRightSelect",
        WordDirection::Right,
        true,
        WordNavigationType::WordEnd,
        move_word_keybindings(KeyCode::RightArrow, KeyMod::SHIFT),
    ))
}

/// `CursorWordRightSelect`.
pub fn cursor_word_right_select() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions::new(
        "cursorWordRightSelect",
        WordDirection::Right,
        true,
        WordNavigationType::WordEnd,
    ))
}

/// `CursorWordAccessibilityRight`.
pub fn cursor_word_accessibility_right() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions {
        accessibility: true,
        ..MoveWordOptions::new(
            "cursorWordAccessibilityRight",
            WordDirection::Right,
            false,
            WordNavigationType::WordAccessibility,
        )
    })
}

/// `CursorWordAccessibilityRightSelect`.
pub fn cursor_word_accessibility_right_select() -> MoveWordCommand {
    MoveWordCommand::new(MoveWordOptions {
        accessibility: true,
        ..MoveWordOptions::new(
            "cursorWordAccessibilityRightSelect",
            WordDirection::Right,
            true,
            WordNavigationType::WordAccessibility,
        )
    })
}

// #endregion

// #region DeleteWordCommand

/// `DeleteWordOptions`: as opções do comando mais `whitespaceHeuristics` e `wordNavigationType` (e a direção, que no
/// original é a subclasse).
pub struct DeleteWordOptions {
    pub command: CommandOptions,
    pub whitespace_heuristics: bool,
    pub word_navigation_type: WordNavigationType,
    pub direction: WordDirection,
}

impl DeleteWordOptions {
    /// As opções de um comando de apagar sem atalho: `precondition: EditorContextKeys.writable` e o
    /// `canTriggerInlineEdits: true` que o construtor de `DeleteWordCommand` acrescenta.
    pub fn new(
        id: &str,
        direction: WordDirection,
        whitespace_heuristics: bool,
        word_navigation_type: WordNavigationType,
    ) -> Self {
        Self {
            command: CommandOptions {
                precondition: Some(writable()),
                can_trigger_inline_edits: true,
                ..CommandOptions::new(id)
            },
            whitespace_heuristics,
            word_navigation_type,
            direction,
        }
    }
}

/// `abstract class DeleteWordCommand extends EditorCommand` (com `DeleteWordLeftCommand` e `DeleteWordRightCommand`).
pub struct DeleteWordCommand {
    base: Command,
    whitespace_heuristics: bool,
    word_navigation_type: WordNavigationType,
    direction: WordDirection,
}

impl DeleteWordCommand {
    pub fn new(opts: DeleteWordOptions) -> Self {
        Self {
            base: Command::new(opts.command),
            whitespace_heuristics: opts.whitespace_heuristics,
            word_navigation_type: opts.word_navigation_type,
            direction: opts.direction,
        }
    }

    /// `_delete(ctx, wordNavigationType)` de `DeleteWordLeftCommand` e de `DeleteWordRightCommand`.
    fn delete_range(&self, ctx: &DeleteWordContext) -> Range {
        match self.direction {
            WordDirection::Left => {
                WordOperations::delete_word_left(ctx, self.word_navigation_type).unwrap_or_else(|| Range::new(1, 1, 1, 1))
            }
            WordDirection::Right => WordOperations::delete_word_right(ctx, self.word_navigation_type).unwrap_or_else(|| {
                let model: &dyn CursorSimpleModel = ctx.model;
                let line_count = model.get_line_count();
                let max_column = model.get_line_max_column(line_count);
                Range::new(line_count, max_column, line_count, max_column)
            }),
        }
    }
}

impl IEditorCommand for DeleteWordCommand {
    fn run_editor_command(
        &self,
        accessor: Option<&ServicesAccessor>,
        editor: &Rc<CodeEditorWidget>,
        _args: &Dynamic,
    ) -> Promise<()> {
        let (Some(_), Some(model), Some(selections), Some(view_model)) =
            (accessor, editor.get_model(), editor.get_selections(), editor.get_view_model())
        else {
            return Promise::resolved(());
        };
        let word_separators = word_separators_of(editor);
        let auto_closing_pairs =
            editor.language_configuration_service().get_language_configuration(&model.get_language_id()).get_auto_closing_pairs();
        let auto_closed_characters = view_model.get_cursor_auto_closed_characters();

        let commands: Vec<Option<Box<dyn EditCommand>>> = selections
            .iter()
            .map(|sel| {
                let delete_range = self.delete_range(&DeleteWordContext {
                    word_separators: &word_separators,
                    model: &model,
                    selection: *sel,
                    whitespace_heuristics: self.whitespace_heuristics,
                    auto_closing_delete: editor.get_option(AUTO_CLOSING_DELETE),
                    auto_closing_brackets: editor.get_option(AUTO_CLOSING_BRACKETS),
                    auto_closing_quotes: editor.get_option(AUTO_CLOSING_QUOTES),
                    auto_closing_pairs: &auto_closing_pairs,
                    auto_closed_characters: &auto_closed_characters,
                });
                let command: Box<dyn EditCommand> = Box::new(ReplaceCommand::new(delete_range, JsString::new(), None));
                Some(command)
            })
            .collect();

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.base.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

impl EditorCommand for DeleteWordCommand {
    fn base(&self) -> &Command {
        &self.base
    }
}

/// O `kbOpts` de `DeleteWordLeft` e `DeleteWordRight`: `CtrlCmd` (e `Alt` no mac) mais a tecla, com o texto em foco.
fn delete_word_keybindings(key_code: KeyCode) -> Vec<CommandKeybindingsOptions> {
    vec![CommandKeybindingsOptions::new(
        KeybindingWeight::EDITOR_CONTRIB,
        Some(text_input_focus()),
        Keybindings {
            primary: Some(key(KeyMod::CTRL_CMD, key_code)),
            mac: platform_keybindings(key(KeyMod::ALT, key_code), None),
            ..Keybindings::default()
        },
    )]
}

/// `DeleteWordStartLeft`.
pub fn delete_word_start_left() -> DeleteWordCommand {
    DeleteWordCommand::new(DeleteWordOptions::new("deleteWordStartLeft", WordDirection::Left, false, WordNavigationType::WordStart))
}

/// `DeleteWordEndLeft`.
pub fn delete_word_end_left() -> DeleteWordCommand {
    DeleteWordCommand::new(DeleteWordOptions::new("deleteWordEndLeft", WordDirection::Left, false, WordNavigationType::WordEnd))
}

/// `DeleteWordLeft`.
pub fn delete_word_left() -> DeleteWordCommand {
    let mut opts = DeleteWordOptions::new("deleteWordLeft", WordDirection::Left, true, WordNavigationType::WordStart);
    opts.command.kb_opts = delete_word_keybindings(KeyCode::Backspace);
    DeleteWordCommand::new(opts)
}

/// `DeleteWordStartRight`.
pub fn delete_word_start_right() -> DeleteWordCommand {
    DeleteWordCommand::new(DeleteWordOptions::new(
        "deleteWordStartRight",
        WordDirection::Right,
        false,
        WordNavigationType::WordStart,
    ))
}

/// `DeleteWordEndRight`.
pub fn delete_word_end_right() -> DeleteWordCommand {
    DeleteWordCommand::new(DeleteWordOptions::new("deleteWordEndRight", WordDirection::Right, false, WordNavigationType::WordEnd))
}

/// `DeleteWordRight`.
pub fn delete_word_right() -> DeleteWordCommand {
    let mut opts = DeleteWordOptions::new("deleteWordRight", WordDirection::Right, true, WordNavigationType::WordEnd);
    opts.command.kb_opts = delete_word_keybindings(KeyCode::Delete);
    DeleteWordCommand::new(opts)
}

// #endregion

// #region DeleteInsideWord

/// `DeleteInsideWord extends EditorAction`.
pub struct DeleteInsideWord {
    action: EditorActionBase,
}

impl DeleteInsideWord {
    pub fn new() -> Self {
        let only_word_property = Dynamic::object(vec![
            ("type", Dynamic::from("boolean")),
            ("default", Dynamic::from(false)),
            (
                "description",
                Dynamic::from(
                    localize(
                        "deleteInsideWord.args.onlyWord",
                        "Delete only the word and leave surrounding whitespace",
                        &[],
                    )
                    .as_str(),
                ),
            ),
        ]);
        let schema = Dynamic::object(vec![
            ("type", Dynamic::from("object")),
            ("properties", Dynamic::object(vec![("onlyWord", only_word_property)])),
        ]);
        let metadata = CommandMetadata {
            description: CommandDescription::Localized(localize2(
                "deleteInsideWord.description",
                "Delete the word at the cursor",
            )),
            args: Some(vec![CommandArgument { schema: Some(schema), ..CommandArgument::new("args") }]),
            returns: None,
        };
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    precondition: Some(writable()),
                    metadata: Some(metadata),
                    ..CommandOptions::new("deleteInsideWord")
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("deleteInsideWord", "Delete Word"), alias: None },
            }),
        }
    }
}

impl Default for DeleteInsideWord {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for DeleteInsideWord {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return Promise::resolved(());
        };

        let only_word = matches!(args, Dynamic::Object(_)) && args.get_str("onlyWord").is_truthy();
        let word_separators = word_separators_of(editor);

        let commands: Vec<Option<Box<dyn EditCommand>>> = selections
            .iter()
            .map(|sel| {
                let delete_range = WordOperations::delete_inside_word(&word_separators, &model, *sel, only_word);
                let command: Box<dyn EditCommand> = Box::new(ReplaceCommand::new(delete_range, JsString::new(), None));
                Some(command)
            })
            .collect();

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

// #endregion

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro de comandos é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// As chamadas `registerEditorCommand(...)` e `registerEditorAction(DeleteInsideWord)` do fim do arquivo (o que o
/// carregamento do módulo faz no original). É idempotente: o registro acontece uma vez por thread.
pub fn register_word_operations() {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_command(Rc::new(cursor_word_start_left()));
    register_editor_command(Rc::new(cursor_word_end_left()));
    register_editor_command(Rc::new(cursor_word_left()));
    register_editor_command(Rc::new(cursor_word_start_left_select()));
    register_editor_command(Rc::new(cursor_word_end_left_select()));
    register_editor_command(Rc::new(cursor_word_left_select()));
    register_editor_command(Rc::new(cursor_word_start_right()));
    register_editor_command(Rc::new(cursor_word_end_right()));
    register_editor_command(Rc::new(cursor_word_right()));
    register_editor_command(Rc::new(cursor_word_start_right_select()));
    register_editor_command(Rc::new(cursor_word_end_right_select()));
    register_editor_command(Rc::new(cursor_word_right_select()));
    register_editor_command(Rc::new(cursor_word_accessibility_left()));
    register_editor_command(Rc::new(cursor_word_accessibility_left_select()));
    register_editor_command(Rc::new(cursor_word_accessibility_right()));
    register_editor_command(Rc::new(cursor_word_accessibility_right_select()));
    register_editor_command(Rc::new(delete_word_start_left()));
    register_editor_command(Rc::new(delete_word_end_left()));
    register_editor_command(Rc::new(delete_word_left()));
    register_editor_command(Rc::new(delete_word_start_right()));
    register_editor_command(Rc::new(delete_word_end_right()));
    register_editor_command(Rc::new(delete_word_right()));
    register_editor_action(DeleteInsideWord::new);
}
