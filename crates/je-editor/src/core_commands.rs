//! Porte de `src/vs/editor/browser/coreCommands.ts`: `CoreEditorCommand`, `EditorScroll_`, `RevealLine_`,
//! `EditorOrNativeTextInputCommand`, `NavigationCommandRevealType`, o namespace `CoreNavigationCommands` (de
//! `MoveTo` até `SetSelection`), `registerColumnSelection`, o namespace `CoreEditingCommands`,
//! `EditorHandlerCommand`, `registerOverwritableCommand` e as chamadas de registro do fim do arquivo.
//!
//! Os comandos chamam as funções de [`crate::cursor_move_commands`] (`CursorMoveCommands`),
//! [`crate::cursor_column_selection`] (`ColumnSelection`) e o `ViewModel` do editor
//! ([`EditorViewModel`]); o registro é o de [`crate::editor_extensions`]. O que o carregamento do módulo faz no
//! original (criar os comandos, `registerColumnSelection`, `registerOverwritableCommand`) é [`register_core_commands`].
//!
//! Representação:
//! - `abstract class CoreEditorCommand<T>` é a struct [`CoreEditorCommand<T>`], que guarda o `runCoreEditorCommand`
//!   como fechamento (`Box<dyn Fn(&EditorViewModel, &T)>`). As subclasses do original (`BaseMoveToCommand`,
//!   `ColumnSelectCommand` e as suas, `CursorMoveBasedCommand`, `HomeCommand`...) são funções que criam o
//!   comando com o fechamento certo; o que a subclasse guarda em campo (`_inSelectionMode`, `_isPaged`,
//!   `_staticArgs`) o fechamento captura. O `IViewModel` do `runCoreEditorCommand` é o [`EditorViewModel`]
//!   concreto, porque o trait [`crate::view_model_types::IViewModel`] tem tipos associados e não vira objeto.
//! - `HomeCommand`, `LineStartCommand`, `EndCommand`, `LineEndCommand`, `TopCommand`, `BottomCommand`, `WordCommand`,
//!   `LineCommand`, `LastCursorLineCommand` e os `ScrollLineUp`... (classes que só diferem em dados e em uma
//!   chamada) são funções que montam o comando por um corpo comum (`cursor_states_command`,
//!   `selection_gesture_command`, `last_cursor_command`, `scroll_command`), com a diferença virando parâmetro.
//! - `abstract class CoreEditingCommand` é a struct [`CoreEditingCommand`], com o `runCoreEditingCommand` como
//!   fechamento. `UndoCommand`/`RedoCommand` do namespace `CoreEditingCommands` são um tipo só,
//!   [`core_editing_commands::UndoRedoCommand`], com a direção como campo. `Undo`/`Redo`/`SelectAll`
//!   (`EditorOrNativeTextInputCommand`) são os structs que implementam o trait.
//! - O `Partial<T>` dos argumentos é uma struct por comando (`MoveCommandOptions`, `ColumnSelectCommandOptions`,
//!   `CursorMoveCommandOptions`, `CreateCursorCommandOptions`, `BaseCommandOptions`, [`CursorMoveArgs`]), com
//!   todos os campos opcionais. O trait [`CoreCommandArgs`] os lê do `Dynamic` do `runEditorCommand` (`args || {}`),
//!   e `trigger`/`executeCommand` entregam esse `Dynamic`. `IPosition` é [`Position`]. `source` é `Option<String>`.
//! - O namespace `CoreNavigationCommands` é o módulo [`core_navigation_commands`], com a struct de comandos
//!   registrados e a instância por thread de [`core_navigation_commands::instance`] (o registro acontece no
//!   primeiro acesso, que é o que o carregamento do módulo faz). `EditorScroll_` e `RevealLine_` são os módulos
//!   [`editor_scroll`] e [`reveal_line`], com `metadata`, as constantes, `RawArguments` (o `Partial<RawArguments>`),
//!   `parse` e os enums.
//! - `ICommandMetadata` é o `CommandMetadata` de `je_platform::commands`. O de `CursorMove_.metadata` vem do tipo
//!   próprio de `cursor_move_commands` e é convertido em [`convert_cursor_move_metadata`]. A restrição de tipo
//!   (`constraint: isEditorScrollArgs`) é uma função [`FunctionRef`] que lê o `Dynamic`.
//! - `EditorOrNativeTextInputCommand` é o trait [`EditorOrNativeTextInputCommand`], e o construtor que registra as
//!   três implementações no `MultiCommand` é [`register_editor_or_native_text_input_command`].
//! - `KeyMod.CtrlCmd | KeyCode.X` é `KeyMod::CTRL_CMD | KeyCode::X.bits()`, aqui `key(modifiers, key_code)`.
//!   `EditorContextKeys.textInputFocus` é [`text_input_focus`].
//!
//! Divergências:
//! - **DOM.** `getActiveElement()`/`isEditableElement()` não existem: a segunda implementação do
//!   `EditorOrNativeTextInputCommand` (`generic-dom-input-textarea`, foco em um `input`/`textarea` qualquer) é
//!   registrada e nunca trata, e `runDOMCommand(activeElement)` perde o parâmetro e nunca é chamado. A decisão
//!   da primeira implementação (`code-editor`) é o foco do próprio editor (`hasTextFocus()`, que o widget guarda
//!   em `has_text_focus`) e a da terceira (`generic-dom`) é o editor ativo. `isFirefox` e `aria.status` (usados
//!   pelos comandos da parte B) também são DOM: o `status(...)` de `RemoveSecondaryCursors` (a mensagem de
//!   acessibilidade "Removed secondary cursors") não é emitido.
//! - **Serviços.** `ICodeEditorService` é o [`crate::editor_extensions::ICodeEditorLookup`] (ver
//!   `editor_extensions`). `EditorContextKeys` (`editorContextKeys.ts`) ainda não tem porte; uma `RawContextKey`
//!   é a expressão `has(chave)` (e `toNegated()` é `not(chave)`), que [`text_input_focus`] e as demais funções
//!   de chave montam (sem o registro de metadados da chave).
//! - `TextEditorSelectionSource` (`platform/editor/common/editor.ts`, só os três valores de texto) é o módulo
//!   [`text_editor_selection_source`], porque `cursorMove` com `noHistory` usa o `PROGRAMMATIC`.
//! - `viewModel.model` (o `ITextModel`) é o `Rc<TextModel>` de [`crate::view_model_types::IViewModel::model`].
//! - `ISelection` (o `selection` de `setSelection`) é lida do `Dynamic` pelos quatro números; um objeto sem os
//!   quatro números não é uma seleção e o comando não faz nada. `parseInt` de `revealLine` é [`js_parse_int`]; um
//!   `lineNumber` de texto sem dígitos (`NaN`) faria o `getLineMaxColumn` do original lançar, e aqui o comando
//!   não faz nada. `executeCommands` aceita `(ICommand | null)[]` como no original (o `Vec<Option<...>>`).

use std::rc::Rc;

use je_base::dynamic::{Dynamic, FunctionRef, Truthy};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::scrollable::INewScrollPosition;
use je_base::types::{is_boolean, is_number, is_object, is_string, is_undefined};
use je_platform::commands::{CommandArgument, CommandDescription, CommandMetadata};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::keybinding::keybindings_registry::{
    KeybindingRule, KeybindingWeight, Keybindings, PlatformKeybindings, keybindings_registry,
};

use crate::code_editor_widget::{CodeEditorWidget, EditorViewModel, Handler, IEditorCommand, ICodeEditor};
use crate::config::editor_options::registry::READ_ONLY;
use crate::cursor_column_selection::{self as column_selection, ColumnSelectResult};
use crate::cursor_common::{
    ColumnSelectData, CursorConfiguration, CursorSimpleModel, CursorState, EditOperationType, PartialCursorState,
};
use crate::cursor_delete_operations::DeleteOperations;
use crate::cursor_move_commands::{self as move_commands, cursor_move};
use crate::cursor_type_edit_operations::{EnterOperation, TabOperation};
use crate::cursor_type_operations::TypeOperations;
use crate::editor_common::{Command as EditCommand, ScrollType};
use crate::editor_extensions::{
    Command, CommandImplementationResult, CommandKeybindingsOptions, CommandOptions, EditorCommand, I_CODE_EDITOR_LOOKUP,
    ICommand, MultiCommand, ServicesAccessor, position_from_dynamic, redo_command, register_command, register_editor_command,
    select_all_command, undo_command,
};
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;
use crate::view_model_types::{CursorChangeReason, IViewModel, VerticalRevealType};

/// `const CORE_WEIGHT = KeybindingWeight.EditorCore`.
pub const CORE_WEIGHT: i64 = KeybindingWeight::EDITOR_CORE;

/// `EditorContextKeys.textInputFocus`: a chave `textInputFocus` (ver "Divergências").
pub fn text_input_focus() -> ContextKeyExpression {
    ContextKeyExpr::has("textInputFocus")
}

/// `TextEditorSelectionSource` (`platform/editor/common/editor.ts`).
pub mod text_editor_selection_source {
    /// Uma mudança de seleção que não veio do usuário por teclado ou mouse, mas pela API do editor.
    pub const PROGRAMMATIC: &str = "api";
    /// Uma mudança causada por algum comando ou componente de UI, como uma árvore de outline.
    pub const NAVIGATION: &str = "code.navigation";
    /// Uma mudança causada de dentro do editor para outro lugar, como "Go to definition".
    pub const JUMP: &str = "code.jump";
}

/// `KeyMod.X | KeyCode.Y`: o atalho em codificação binária.
pub fn key(modifiers: u32, key_code: KeyCode) -> u32 {
    modifiers | key_code.bits()
}

/// Um atalho de `kbOpts` com peso do núcleo e `kbExpr: EditorContextKeys.textInputFocus`, que é o que todos os
/// comandos de cursor usam.
pub fn text_input_keybindings(keybindings: Keybindings) -> Vec<CommandKeybindingsOptions> {
    vec![CommandKeybindingsOptions::new(CORE_WEIGHT, Some(text_input_focus()), keybindings)]
}

/// `{ primary }` ou `{ primary, secondary }` de uma plataforma (`mac: {...}`, `linux: {...}`).
pub fn platform_keybindings(primary: u32, secondary: Option<Vec<u32>>) -> Option<PlatformKeybindings> {
    Some(PlatformKeybindings { primary: Some(primary), secondary })
}

/// As opções de um comando do núcleo com atalhos de `text_input_keybindings` e sem precondição nem metadados.
fn text_input_command_options(id: &str, keybindings: Keybindings) -> CommandOptions {
    CommandOptions { kb_opts: text_input_keybindings(keybindings), ..CommandOptions::new(id) }
}

/// `EditorContextKeys.hasNonEmptySelection` (a chave `editorHasSelection`).
pub fn has_non_empty_selection() -> ContextKeyExpression {
    ContextKeyExpr::has("editorHasSelection")
}

/// `EditorContextKeys.hasMultipleSelections` (a chave `editorHasMultipleSelections`).
pub fn has_multiple_selections() -> ContextKeyExpression {
    ContextKeyExpr::has("editorHasMultipleSelections")
}

/// `EditorContextKeys.writable`: `readOnly.toNegated()` (a chave `editorReadonly`).
pub fn writable() -> ContextKeyExpression {
    ContextKeyExpr::not("editorReadonly")
}

/// `EditorContextKeys.editorTextFocus` (a chave `editorTextFocus`).
pub fn editor_text_focus() -> ContextKeyExpression {
    ContextKeyExpr::has("editorTextFocus")
}

/// `EditorContextKeys.tabDoesNotMoveFocus`: `tabMovesFocus.toNegated()` (a chave `editorTabMovesFocus`).
pub fn tab_does_not_move_focus() -> ContextKeyExpression {
    ContextKeyExpr::not("editorTabMovesFocus")
}

/// `EditorContextKeys.columnSelection` (a chave `editorColumnSelection`).
pub fn column_selection_enabled() -> ContextKeyExpression {
    ContextKeyExpr::has("editorColumnSelection")
}

/// `parseInt(text)` (base 10, ou 16 com o prefixo `0x`): `NaN` se não há dígito no começo do texto (depois dos
/// espaços e do sinal).
fn js_parse_int(text: &str) -> f64 {
    let trimmed = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (sign, unsigned) = match trimmed.as_bytes().first().copied() {
        Some(b'-') => (-1.0, &trimmed[1..]),
        Some(b'+') => (1.0, &trimmed[1..]),
        _ => (1.0, trimmed),
    };
    let (radix, digits) = match unsigned.strip_prefix("0x").or_else(|| unsigned.strip_prefix("0X")) {
        Some(rest) => (16, rest),
        None => (10, unsigned),
    };
    let mut value: Option<f64> = None;
    for digit in digits.chars().map_while(|c| c.to_digit(radix)) {
        value = Some(value.unwrap_or(0.0) * f64::from(radix) + f64::from(digit));
    }
    value.map_or(f64::NAN, |value| sign * value)
}

// #region Leitura dos argumentos (`Partial<T>`)

/// `args.<key>` de um `Partial<T>`: `undefined` se o argumento não é um objeto (o `args || {}` do original).
fn property(args: &Dynamic, key: &str) -> Dynamic {
    if matches!(args, Dynamic::Object(_)) { args.get_str(key) } else { Dynamic::Undefined }
}

/// `args.<key>` quando é texto.
fn text_property(args: &Dynamic, key: &str) -> Option<String> {
    match property(args, key) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `args.<key>` quando é número.
fn number_property(args: &Dynamic, key: &str) -> Option<f64> {
    match property(args, key) {
        Dynamic::Number(number) => Some(number),
        _ => None,
    }
}

/// `args.<key>` quando está definido, como o valor de `!!args.<key>`.
fn flag_property(args: &Dynamic, key: &str) -> Option<bool> {
    let value = property(args, key);
    if value.is_undefined() { None } else { Some(value.is_truthy()) }
}

/// `args.<key>` como `IPosition`.
fn position_property(args: &Dynamic, key: &str) -> Option<Position> {
    position_from_dynamic(&property(args, key))
}

/// O `Partial<T>` de um comando do núcleo, lido do argumento `unknown` do `runEditorCommand`.
pub trait CoreCommandArgs: Default + 'static {
    /// Lê os campos do argumento; um campo ausente fica `None` (ou `false`), como o `Partial` do original.
    fn from_dynamic(args: &Dynamic) -> Self;

    /// `args.source`.
    fn source(&self) -> Option<&str>;
}

/// `BaseCommandOptions`: `source?: 'mouse' | 'keyboard' | string`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BaseCommandOptions {
    pub source: Option<String>,
}

impl CoreCommandArgs for BaseCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self { source: text_property(args, "source") }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `NavigationCommandRevealType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationCommandRevealType {
    /// Do regular revealing.
    Regular = 0,
    /// Do only minimal revealing.
    Minimal = 1,
    /// Do not reveal the position.
    None = 2,
}

impl NavigationCommandRevealType {
    fn from_number(number: f64) -> Option<Self> {
        match number as i32 {
            0 => Some(Self::Regular),
            1 => Some(Self::Minimal),
            2 => Some(Self::None),
            _ => None,
        }
    }
}

/// `CoreNavigationCommands.MoveCommandOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveCommandOptions {
    pub source: Option<String>,
    pub position: Option<Position>,
    pub view_position: Option<Position>,
    pub reveal_type: Option<NavigationCommandRevealType>,
}

impl CoreCommandArgs for MoveCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self {
            source: text_property(args, "source"),
            position: position_property(args, "position"),
            view_position: position_property(args, "viewPosition"),
            reveal_type: number_property(args, "revealType").and_then(NavigationCommandRevealType::from_number),
        }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `CoreNavigationCommands.ColumnSelectCommandOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnSelectCommandOptions {
    pub source: Option<String>,
    pub position: Option<Position>,
    pub view_position: Option<Position>,
    pub mouse_column: Option<i32>,
    pub do_column_select: bool,
}

impl CoreCommandArgs for ColumnSelectCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self {
            source: text_property(args, "source"),
            position: position_property(args, "position"),
            view_position: position_property(args, "viewPosition"),
            mouse_column: number_property(args, "mouseColumn").map(|column| column as i32),
            do_column_select: flag_property(args, "doColumnSelect").unwrap_or(false),
        }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `CoreNavigationCommands.CursorMoveCommandOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CursorMoveCommandOptions {
    pub source: Option<String>,
    pub page_size: Option<f64>,
}

impl CoreCommandArgs for CursorMoveCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self { source: text_property(args, "source"), page_size: number_property(args, "pageSize") }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `CoreNavigationCommands.CreateCursorCommandOptions`: o `MoveCommandOptions` mais `wholeLine`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreateCursorCommandOptions {
    pub options: MoveCommandOptions,
    pub whole_line: bool,
}

impl CoreCommandArgs for CreateCursorCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self {
            options: MoveCommandOptions::from_dynamic(args),
            whole_line: flag_property(args, "wholeLine").unwrap_or(false),
        }
    }

    fn source(&self) -> Option<&str> {
        self.options.source()
    }
}

/// `Partial<BaseCommandOptions & CursorMove_.RawArguments>`: os argumentos do comando `cursorMove`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CursorMoveArgs {
    pub source: Option<String>,
    pub arguments: cursor_move::PartialRawArguments,
}

impl CoreCommandArgs for CursorMoveArgs {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self {
            source: text_property(args, "source"),
            arguments: cursor_move::PartialRawArguments {
                to: text_property(args, "to"),
                select: flag_property(args, "select"),
                by: text_property(args, "by"),
                value: number_property(args, "value"),
                no_history: flag_property(args, "noHistory"),
            },
        }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `args.<key>` como `ISelection` (ver "Divergências").
fn selection_property(args: &Dynamic, key: &str) -> Option<Selection> {
    let selection = property(args, key);
    if !matches!(selection, Dynamic::Object(_)) {
        return None;
    }
    let number = |name: &str| number_property(&selection, name).map(|number| number as i32);
    Some(Selection::new(
        number("selectionStartLineNumber")?,
        number("selectionStartColumn")?,
        number("positionLineNumber")?,
        number("positionColumn")?,
    ))
}

/// `CoreNavigationCommands.EndCommandOptions`: o `BaseCommandOptions` mais `sticky`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndCommandOptions {
    pub source: Option<String>,
    pub sticky: bool,
}

impl CoreCommandArgs for EndCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self { source: text_property(args, "source"), sticky: flag_property(args, "sticky").unwrap_or(false) }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `CoreNavigationCommands.EditorScrollCommandOptions`: `EditorScroll_.RawArguments & BaseCommandOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorScrollCommandOptions {
    pub source: Option<String>,
    pub arguments: editor_scroll::RawArguments,
}

impl CoreCommandArgs for EditorScrollCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self { source: text_property(args, "source"), arguments: editor_scroll::RawArguments::from_dynamic(args) }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `CoreNavigationCommands.RevealLineCommandOptions`: `RevealLine_.RawArguments & BaseCommandOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RevealLineCommandOptions {
    pub source: Option<String>,
    pub arguments: reveal_line::RawArguments,
}

impl CoreCommandArgs for RevealLineCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self { source: text_property(args, "source"), arguments: reveal_line::RawArguments::from_dynamic(args) }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `CoreNavigationCommands.SetSelectionCommandOptions`: o `BaseCommandOptions` mais `selection`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetSelectionCommandOptions {
    pub source: Option<String>,
    pub selection: Option<Selection>,
}

impl CoreCommandArgs for SetSelectionCommandOptions {
    fn from_dynamic(args: &Dynamic) -> Self {
        Self { source: text_property(args, "source"), selection: selection_property(args, "selection") }
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

// #endregion

// #region CoreEditorCommand

/// `abstract class CoreEditorCommand<T> extends EditorCommand`: um comando que age nos cursores do `ViewModel`.
pub struct CoreEditorCommand<T: CoreCommandArgs> {
    base: Command,
    run: Box<dyn Fn(&EditorViewModel, &T)>,
}

impl<T: CoreCommandArgs> CoreEditorCommand<T> {
    /// Cria o comando; `run` é o `runCoreEditorCommand(viewModel, args)`.
    pub fn new(opts: CommandOptions, run: impl Fn(&EditorViewModel, &T) + 'static) -> Self {
        Self { base: Command::new(opts), run: Box::new(run) }
    }

    /// `runCoreEditorCommand(viewModel, args)`.
    pub fn run_core_editor_command(&self, view_model: &EditorViewModel, args: &T) {
        (self.run)(view_model, args)
    }
}

impl<T: CoreCommandArgs> IEditorCommand for CoreEditorCommand<T> {
    /// `runEditorCommand(accessor, editor, args?)`.
    fn run_editor_command(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let Some(view_model) = editor.get_view_model() else {
            // the editor has no view => has no cursors
            return Promise::resolved(());
        };
        self.run_core_editor_command(&view_model, &T::from_dynamic(args));
        Promise::resolved(())
    }
}

impl<T: CoreCommandArgs> EditorCommand for CoreEditorCommand<T> {
    fn base(&self) -> &Command {
        &self.base
    }
}

/// `registerEditorCommand(new X(...))` de um comando do núcleo.
fn register_core_command<T: CoreCommandArgs>(command: CoreEditorCommand<T>) -> Rc<CoreEditorCommand<T>> {
    register_editor_command(Rc::new(command))
}

// #endregion

// #region Metadados

/// Os itens do `schema` JSON: uma lista de textos.
fn json_strings(values: &[&str]) -> Dynamic {
    Dynamic::array(values.iter().map(|value| Dynamic::from(*value)))
}

/// `{ type, enum? }` de uma propriedade do `schema`.
fn json_property(ty: &str, values: Option<&[&str]>, default: Option<Dynamic>) -> Dynamic {
    let mut entries = vec![("type", Dynamic::from(ty))];
    if let Some(values) = values {
        entries.push(("enum", json_strings(values)));
    }
    if let Some(default) = default {
        entries.push(("default", default));
    }
    Dynamic::object(entries)
}

/// O primeiro argumento de uma função (`undefined` se faltar).
fn first_argument(args: &[Dynamic]) -> Dynamic {
    args.first().cloned().unwrap_or(Dynamic::Undefined)
}

/// O argumento de um comando com um objeto de propriedades (`name`, `description`, `constraint`, `schema`).
fn object_argument(name: &str, description: &str, constraint: fn(&Dynamic) -> bool, schema: Dynamic) -> CommandArgument {
    CommandArgument {
        name: name.to_string(),
        is_optional: false,
        description: Some(description.to_string()),
        constraint: Some(
            FunctionRef::native("constraint", 1, move |args: &[Dynamic]| Dynamic::Bool(constraint(&first_argument(args)))).into(),
        ),
        schema: Some(schema),
    }
}

/// `CommandMetadata` com um argumento só.
fn metadata_with_argument(description: &str, argument: CommandArgument) -> CommandMetadata {
    let mut metadata = CommandMetadata::new(CommandDescription::Plain(description.to_string()));
    metadata.args = Some(vec![argument]);
    metadata
}

/// `CommandMetadata` com o argumento `{ name: 'args', schema }` (sem descrição nem restrição de tipo).
fn args_metadata(description: &str, schema: Dynamic) -> CommandMetadata {
    metadata_with_argument(
        description,
        CommandArgument { name: "args".to_string(), is_optional: false, description: None, constraint: None, schema: Some(schema) },
    )
}

/// `CursorMove_.metadata` (o tipo próprio de `cursor_move_commands`) no `ICommandMetadata` de plataforma.
pub fn convert_cursor_move_metadata(metadata: move_commands::CommandMetadata) -> CommandMetadata {
    fn to_dynamic(value: &move_commands::ArgumentValue) -> Dynamic {
        match value {
            move_commands::ArgumentValue::Undefined => Dynamic::Undefined,
            move_commands::ArgumentValue::Null => Dynamic::Null,
            move_commands::ArgumentValue::Boolean(value) => Dynamic::Bool(*value),
            move_commands::ArgumentValue::Number(value) => Dynamic::Number(*value),
            move_commands::ArgumentValue::String(value) => Dynamic::from(value.as_str()),
            move_commands::ArgumentValue::Array(items) => Dynamic::array(items.iter().map(to_dynamic)),
            move_commands::ArgumentValue::Object(entries) => {
                Dynamic::object(entries.iter().map(|(name, value)| (name.as_str(), to_dynamic(value))))
            }
        }
    }

    fn from_dynamic(value: &Dynamic) -> move_commands::ArgumentValue {
        use move_commands::ArgumentValue;
        match value {
            Dynamic::Undefined => ArgumentValue::Undefined,
            Dynamic::Null => ArgumentValue::Null,
            Dynamic::Bool(value) => ArgumentValue::Boolean(*value),
            Dynamic::Number(value) => ArgumentValue::Number(*value),
            Dynamic::String(value) => ArgumentValue::String(value.to_string_lossy()),
            Dynamic::Array(items) => ArgumentValue::Array(items.to_vec().iter().map(from_dynamic).collect()),
            Dynamic::Object(object) => ArgumentValue::Object(
                object.entries().iter().map(|(name, value)| (name.to_string_lossy(), from_dynamic(value))).collect(),
            ),
            // Funções, `RegExp`, `Date` e `Uint8Array` não são argumentos de comando: nenhum predicado os trata.
            _ => ArgumentValue::Undefined,
        }
    }

    let args = metadata.args.map(|args| {
        args.into_iter()
            .map(|argument| CommandArgument {
                name: argument.name,
                is_optional: argument.is_optional.unwrap_or(false),
                description: argument.description,
                constraint: argument.constraint.map(|constraint| match constraint {
                    move_commands::TypeConstraint::TypeName(name) => je_base::types::TypeConstraint::TypeOf(name.to_string()),
                    move_commands::TypeConstraint::Predicate(predicate) => FunctionRef::native(
                        "constraint",
                        1,
                        move |args: &[Dynamic]| Dynamic::Bool(predicate(&from_dynamic(&first_argument(args)))),
                    )
                    .into(),
                }),
                schema: argument.schema.as_ref().map(to_dynamic),
            })
            .collect()
    });
    CommandMetadata { description: CommandDescription::Plain(metadata.description), args, returns: metadata.returns }
}

// #endregion

// #region EditorScroll_

/// `EditorScroll_`: o namespace dos argumentos do comando `editorScroll`.
pub mod editor_scroll {
    use super::*;

    /// `isEditorScrollArgs`.
    fn is_editor_scroll_args(arg: &Dynamic) -> bool {
        if !is_object(arg) {
            return false;
        }
        if !is_string(&arg.get_str("to")) {
            return false;
        }
        let by = arg.get_str("by");
        if !is_undefined(&by) && !is_string(&by) {
            return false;
        }
        let value = arg.get_str("value");
        if !is_undefined(&value) && !is_number(&value) {
            return false;
        }
        let reveal_cursor = arg.get_str("revealCursor");
        if !is_undefined(&reveal_cursor) && !is_boolean(&reveal_cursor) {
            return false;
        }
        true
    }

    /// `metadata`.
    pub fn metadata() -> CommandMetadata {
        const DESCRIPTION: &str = concat!(
            "Property-value pairs that can be passed through this argument:\n",
            "\t\t\t\t\t* 'to': A mandatory direction value.\n",
            "\t\t\t\t\t\t```\n",
            "\t\t\t\t\t\t'up', 'down'\n",
            "\t\t\t\t\t\t```\n",
            "\t\t\t\t\t* 'by': Unit to move. Default is computed based on 'to' value.\n",
            "\t\t\t\t\t\t```\n",
            "\t\t\t\t\t\t'line', 'wrappedLine', 'page', 'halfPage', 'editor'\n",
            "\t\t\t\t\t\t```\n",
            "\t\t\t\t\t* 'value': Number of units to move. Default is '1'.\n",
            "\t\t\t\t\t* 'revealCursor': If 'true' reveals the cursor if it is outside view port.\n",
            "\t\t\t\t",
        );

        let schema = Dynamic::object([
            ("type", Dynamic::from("object")),
            ("required", json_strings(&["to"])),
            (
                "properties",
                Dynamic::object([
                    ("to", json_property("string", Some(&["up", "down"]), None)),
                    ("by", json_property("string", Some(&["line", "wrappedLine", "page", "halfPage", "editor"]), None)),
                    ("value", json_property("number", None, Some(Dynamic::from(1)))),
                    ("revealCursor", json_property("boolean", None, None)),
                ]),
            ),
        ]);

        metadata_with_argument(
            "Scroll editor in the given direction",
            object_argument("Editor scroll argument object", DESCRIPTION, is_editor_scroll_args, schema),
        )
    }

    /// `RawDirection`: as direções do argumento `to`.
    pub mod raw_direction {
        pub const UP: &str = "up";
        pub const RIGHT: &str = "right";
        pub const DOWN: &str = "down";
        pub const LEFT: &str = "left";
    }

    /// `RawUnit`: as unidades do argumento `by`.
    pub mod raw_unit {
        pub const LINE: &str = "line";
        pub const WRAPPED_LINE: &str = "wrappedLine";
        pub const PAGE: &str = "page";
        pub const HALF_PAGE: &str = "halfPage";
        pub const EDITOR: &str = "editor";
        pub const COLUMN: &str = "column";
    }

    /// `Partial<RawArguments>`: os argumentos do comando (o `to` é obrigatório no original, mas todo uso passa
    /// por `Partial`).
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct RawArguments {
        pub to: Option<String>,
        pub by: Option<String>,
        pub value: Option<f64>,
        pub reveal_cursor: Option<bool>,
        pub select: Option<bool>,
    }

    impl RawArguments {
        /// Lê os argumentos de um objeto (o `args` de `runEditorCommand`).
        pub fn from_dynamic(args: &Dynamic) -> Self {
            Self {
                to: text_property(args, "to"),
                by: text_property(args, "by"),
                value: number_property(args, "value"),
                reveal_cursor: flag_property(args, "revealCursor"),
                select: flag_property(args, "select"),
            }
        }
    }

    /// `parse(args)`: `None` (o `null` do original) para argumentos ilegais.
    pub fn parse(args: &RawArguments) -> Option<ParsedArguments> {
        let direction = match args.to.as_deref() {
            Some(raw_direction::UP) => Direction::Up,
            Some(raw_direction::RIGHT) => Direction::Right,
            Some(raw_direction::DOWN) => Direction::Down,
            Some(raw_direction::LEFT) => Direction::Left,
            // Illegal arguments
            _ => return None,
        };

        let unit = match args.by.as_deref() {
            Some(raw_unit::LINE) => Unit::Line,
            Some(raw_unit::WRAPPED_LINE) => Unit::WrappedLine,
            Some(raw_unit::PAGE) => Unit::Page,
            Some(raw_unit::HALF_PAGE) => Unit::HalfPage,
            Some(raw_unit::EDITOR) => Unit::Editor,
            Some(raw_unit::COLUMN) => Unit::Column,
            _ => Unit::WrappedLine,
        };

        // `Math.floor(args.value || 1)`: zero e `NaN` caem em 1.
        let value = match args.value {
            Some(value) if value != 0.0 && !value.is_nan() => value.floor(),
            _ => 1.0,
        };

        Some(ParsedArguments {
            direction,
            unit,
            value,
            reveal_cursor: args.reveal_cursor.unwrap_or(false),
            select: args.select.unwrap_or(false),
        })
    }

    /// `ParsedArguments`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct ParsedArguments {
        pub direction: Direction,
        pub unit: Unit,
        pub value: f64,
        pub reveal_cursor: bool,
        pub select: bool,
    }

    /// `Direction` (um `const enum` no original): as direções da visão do comando de rolagem.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Direction {
        Up = 1,
        Right = 2,
        Down = 3,
        Left = 4,
    }

    /// `Unit` (um `const enum` no original).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Unit {
        Line = 1,
        WrappedLine = 2,
        Page = 3,
        HalfPage = 4,
        Editor = 5,
        Column = 6,
    }
}

// #endregion

// #region RevealLine_

/// `RevealLine_`: o namespace dos argumentos do comando `revealLine`.
pub mod reveal_line {
    use super::*;

    /// `isRevealLineArgs`.
    fn is_reveal_line_args(arg: &Dynamic) -> bool {
        if !is_object(arg) {
            return false;
        }
        let line_number = arg.get_str("lineNumber");
        if !is_number(&line_number) && !is_string(&line_number) {
            return false;
        }
        let at = arg.get_str("at");
        if !is_undefined(&at) && !is_string(&at) {
            return false;
        }
        true
    }

    /// `metadata`.
    pub fn metadata() -> CommandMetadata {
        const DESCRIPTION: &str = concat!(
            "Property-value pairs that can be passed through this argument:\n",
            "\t\t\t\t\t* 'lineNumber': A mandatory line number value.\n",
            "\t\t\t\t\t* 'at': Logical position at which line has to be revealed.\n",
            "\t\t\t\t\t\t```\n",
            "\t\t\t\t\t\t'top', 'center', 'bottom'\n",
            "\t\t\t\t\t\t```\n",
            "\t\t\t\t",
        );

        let schema = Dynamic::object([
            ("type", Dynamic::from("object")),
            ("required", json_strings(&["lineNumber"])),
            (
                "properties",
                Dynamic::object([
                    (
                        "lineNumber",
                        Dynamic::object([("type", json_strings(&["number", "string"]))]),
                    ),
                    ("at", json_property("string", Some(&["top", "center", "bottom"]), None)),
                ]),
            ),
        ]);

        metadata_with_argument(
            "Reveal the given line at the given logical position",
            object_argument("Reveal line argument object", DESCRIPTION, is_reveal_line_args, schema),
        )
    }

    /// O `lineNumber` dos argumentos: `number | string`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum LineNumber {
        Number(f64),
        Text(String),
    }

    /// `RawArguments`: os argumentos do comando.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct RawArguments {
        pub line_number: Option<LineNumber>,
        pub at: Option<String>,
    }

    impl RawArguments {
        /// Lê os argumentos de um objeto (o `args` de `runEditorCommand`).
        pub fn from_dynamic(args: &Dynamic) -> Self {
            Self {
                line_number: match property(args, "lineNumber") {
                    Dynamic::Number(number) => Some(LineNumber::Number(number)),
                    Dynamic::String(text) => Some(LineNumber::Text(text.to_string_lossy())),
                    _ => None,
                },
                at: text_property(args, "at"),
            }
        }
    }

    /// `RawAtArgument`: os valores do argumento `at`.
    pub mod raw_at_argument {
        pub const TOP: &str = "top";
        pub const CENTER: &str = "center";
        pub const BOTTOM: &str = "bottom";
    }
}

// #endregion

// #region EditorOrNativeTextInputCommand

/// `abstract class EditorOrNativeTextInputCommand`: um comando que age no editor com foco, no `input` ou
/// `textarea` com foco ou, por padrão, no editor ativo.
pub trait EditorOrNativeTextInputCommand {
    /// `runDOMCommand(activeElement)` (ver "Divergências": nunca é chamado, porque não há elemento DOM).
    fn run_dom_command(&self);

    /// `runEditorCommand(accessor, editor, args)`: `void | Promise<void>`.
    fn run_editor_command(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> MaybePromise<()>;

    /// `_runEditorCommand(accessor, editor, args)`: o resultado de `runEditorCommand` se for verdadeiro (uma
    /// promessa), senão `true`.
    fn run_editor_command_implementation(
        &self,
        accessor: Option<&ServicesAccessor>,
        editor: &Rc<CodeEditorWidget>,
        args: &Dynamic,
    ) -> CommandImplementationResult {
        match self.run_editor_command(accessor, editor, args) {
            MaybePromise::Promise(promise) => CommandImplementationResult::Promise(promise),
            MaybePromise::Value(()) => CommandImplementationResult::Bool(true),
        }
    }
}

/// O construtor de `EditorOrNativeTextInputCommand`: registra no `target` as três implementações.
pub fn register_editor_or_native_text_input_command<C: EditorOrNativeTextInputCommand + 'static>(
    command: Rc<C>,
    target: &MultiCommand,
) {
    // 1. handle case when focus is in editor.
    let editor_command = Rc::clone(&command);
    target.add_implementation(
        10000,
        "code-editor",
        Rc::new(move |accessor, args| {
            // Only if editor text focus (i.e. not if editor has widget focus).
            match accessor.get(I_CODE_EDITOR_LOOKUP).get_focused_code_editor() {
                Some(focused_editor) if focused_editor.has_text_focus() => {
                    editor_command.run_editor_command_implementation(Some(accessor), &focused_editor, args)
                }
                _ => CommandImplementationResult::Bool(false),
            }
        }),
        None,
    );

    // 2. handle case when focus is in some other `input` / `textarea`: não há elemento DOM editável, então
    // esta implementação nunca trata (ver "Divergências").
    target.add_implementation(1000, "generic-dom-input-textarea", Rc::new(|_accessor, _args| CommandImplementationResult::Bool(false)), None);

    // 3. (default) handle case when focus is somewhere else.
    target.add_implementation(
        0,
        "generic-dom",
        Rc::new(move |accessor, args| {
            // Redirecting to active editor
            match accessor.get(I_CODE_EDITOR_LOOKUP).get_active_code_editor() {
                Some(active_editor) => {
                    active_editor.focus();
                    command.run_editor_command_implementation(Some(accessor), &active_editor, args)
                }
                None => CommandImplementationResult::Bool(false),
            }
        }),
        None,
    );
}

// #endregion

// #region CoreNavigationCommands

/// `CoreNavigationCommands`: os comandos de navegação do núcleo (o namespace do original).
pub mod core_navigation_commands {
    use super::*;

    /// `Constants.PAGE_SIZE_MARKER`: `-1` marca o tamanho da página.
    const PAGE_SIZE_MARKER: i32 = -1;

    /// `BaseMoveToCommand`: o `_moveTo` e o `_moveToSelect`.
    fn base_move_to_command(id: &str, in_selection_mode: bool) -> CoreEditorCommand<MoveCommandOptions> {
        CoreEditorCommand::new(CommandOptions::new(id), move |view_model: &EditorViewModel, args: &MoveCommandOptions| {
            let Some(position) = args.position else {
                return;
            };
            view_model.model().push_stack_element();
            let cursor_state_changed = view_model.set_cursor_states(
                args.source(),
                CursorChangeReason::Explicit,
                Some(vec![move_commands::move_to(
                    view_model,
                    &view_model.get_primary_cursor_state(),
                    in_selection_mode,
                    position,
                    args.view_position,
                )]),
            );
            if cursor_state_changed && args.reveal_type != Some(NavigationCommandRevealType::None) {
                view_model.reveal_all_cursors(args.source(), true, Some(true));
            }
        })
    }

    /// `ColumnSelectCommand`: o `runCoreEditorCommand` comum, com o `_getColumnSelectResult` de cada comando.
    fn column_select_command<T: CoreCommandArgs>(
        opts: CommandOptions,
        get_column_select_result: impl Fn(&EditorViewModel, &CursorState, &ColumnSelectData, &T) -> Option<ColumnSelectResult> + 'static,
    ) -> CoreEditorCommand<T> {
        CoreEditorCommand::new(opts, move |view_model, args| {
            view_model.model().push_stack_element();
            let result = get_column_select_result(
                view_model,
                &view_model.get_primary_cursor_state(),
                &view_model.get_cursor_column_select_data(),
                args,
            );
            let Some(result) = result else {
                // invalid arguments
                return;
            };
            view_model.set_cursor_states(
                args.source(),
                CursorChangeReason::Explicit,
                Some(
                    result
                        .view_states
                        .iter()
                        .map(|view_state| PartialCursorState::View(CursorState::from_view_state(*view_state)))
                        .collect(),
                ),
            );
            view_model.set_cursor_column_select_data(ColumnSelectData {
                is_real: true,
                from_view_line_number: result.from_line_number,
                from_view_visual_column: result.from_visual_column,
                to_view_line_number: result.to_line_number,
                to_view_visual_column: result.to_visual_column,
            });
            if result.reversed {
                view_model.reveal_top_most_cursor(args.source());
            } else {
                view_model.reveal_bottom_most_cursor(args.source());
            }
        })
    }

    /// As opções de um comando `cursorColumnSelect*`: o atalho `CtrlCmd | Shift | Alt | tecla` (desligado no
    /// Linux, onde o sistema já usa a combinação).
    fn column_select_options(id: &str, key_code: KeyCode) -> CommandOptions {
        CommandOptions {
            kb_opts: text_input_keybindings(Keybindings {
                primary: Some(key(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::ALT, key_code)),
                linux: platform_keybindings(0, None),
                ..Keybindings::default()
            }),
            ..CommandOptions::new(id)
        }
    }

    /// `ColumnSelectUpCommand` e `ColumnSelectDownCommand`: as versões por linha e por página.
    fn column_select_vertical_command(
        id: &str,
        key_code: KeyCode,
        is_paged: bool,
        select: fn(&CursorConfiguration, &dyn CursorSimpleModel, &ColumnSelectData, bool) -> ColumnSelectResult,
    ) -> CoreEditorCommand<BaseCommandOptions> {
        column_select_command(column_select_options(id, key_code), move |view_model, _primary, prev_column_select_data, _args| {
            Some(select(&view_model.cursor_config(), view_model, prev_column_select_data, is_paged))
        })
    }

    /// `CursorMoveBasedCommand`: um movimento simples com os argumentos fixos (e o tamanho da página, se for o
    /// marcador).
    fn cursor_move_based_command(
        id: &str,
        static_args: cursor_move::SimpleMoveArguments,
        keybindings: Keybindings,
    ) -> CoreEditorCommand<CursorMoveCommandOptions> {
        CoreEditorCommand::new(
            CommandOptions { kb_opts: text_input_keybindings(keybindings), ..CommandOptions::new(id) },
            move |view_model: &EditorViewModel, dynamic_args: &CursorMoveCommandOptions| {
                let mut args = static_args;
                if static_args.value == PAGE_SIZE_MARKER {
                    // -1 is a marker for page size: `dynamicArgs.pageSize || viewModel.cursorConfig.pageSize`
                    let page_size = match dynamic_args.page_size {
                        Some(page_size) if page_size != 0.0 && !page_size.is_nan() => page_size,
                        _ => view_model.cursor_config().page_size,
                    };
                    args.value = page_size as i32;
                }

                view_model.model().push_stack_element();
                view_model.set_cursor_states(
                    dynamic_args.source(),
                    CursorChangeReason::Explicit,
                    move_commands::simple_move(
                        view_model,
                        &view_model.get_cursor_states(),
                        args.direction,
                        args.select,
                        args.value,
                        args.unit,
                    ),
                );
                view_model.reveal_all_cursors(dynamic_args.source(), true, None);
            },
        )
    }

    /// Os argumentos fixos de um `CursorMoveBasedCommand`.
    fn move_args(direction: cursor_move::Direction, unit: cursor_move::Unit, select: bool, value: i32) -> cursor_move::SimpleMoveArguments {
        cursor_move::SimpleMoveArguments { direction, unit, select, value }
    }

    /// `CursorMoveImpl._move(viewModel, cursors, args)`.
    fn move_cursors(
        view_model: &EditorViewModel,
        cursors: &[CursorState],
        args: &cursor_move::ParsedArguments,
    ) -> Option<Vec<PartialCursorState>> {
        use cursor_move::Direction;

        let in_selection_mode = args.select;
        let value = args.value;

        match args.direction {
            Direction::Left
            | Direction::Right
            | Direction::Up
            | Direction::Down
            | Direction::PrevBlankLine
            | Direction::NextBlankLine
            | Direction::WrappedLineStart
            | Direction::WrappedLineFirstNonWhitespaceCharacter
            | Direction::WrappedLineColumnCenter
            | Direction::WrappedLineEnd
            | Direction::WrappedLineLastNonWhitespaceCharacter => {
                move_commands::simple_move(view_model, cursors, args.direction, in_selection_mode, value, args.unit)
            }

            Direction::ViewPortTop | Direction::ViewPortBottom | Direction::ViewPortCenter | Direction::ViewPortIfOutside => {
                move_commands::viewport_move(view_model, cursors, args.direction, in_selection_mode, value)
            }
        }
    }

    /// `CursorMoveImpl._runCursorMove(viewModel, source, args)`.
    fn run_cursor_move(view_model: &EditorViewModel, source: Option<&str>, args: &cursor_move::ParsedArguments) {
        // If noHistory is true, use PROGRAMMATIC source to prevent adding to navigation history
        let effective_source = if args.no_history { Some(text_editor_selection_source::PROGRAMMATIC) } else { source };

        view_model.model().push_stack_element();
        view_model.set_cursor_states(
            effective_source,
            CursorChangeReason::Explicit,
            move_cursors(view_model, &view_model.get_cursor_states(), args),
        );
        view_model.reveal_all_cursors(effective_source, true, None);
    }

    /// `CreateCursor.runCoreEditorCommand`.
    fn run_create_cursor(view_model: &EditorViewModel, args: &CreateCursorCommandOptions) {
        let Some(position) = args.options.position else {
            return;
        };
        let new_state = if args.whole_line {
            move_commands::line(view_model, &view_model.get_primary_cursor_state(), false, position, args.options.view_position)
        } else {
            move_commands::move_to(view_model, &view_model.get_primary_cursor_state(), false, position, args.options.view_position)
        };

        let mut states: Vec<PartialCursorState> = view_model.get_cursor_states().into_iter().map(PartialCursorState::Full).collect();

        // Check if we should remove a cursor (sort of like a toggle)
        if states.len() > 1 {
            let new_model_position = new_state.model_state().map(|state| state.position);
            let new_view_position = new_state.view_state().map(|state| state.position);

            for index in 0..states.len() {
                let state = &states[index];

                if new_model_position
                    .is_some_and(|position| !state.model_state().is_some_and(|state| state.selection.contains_position(&position)))
                {
                    continue;
                }

                if new_view_position
                    .is_some_and(|position| !state.view_state().is_some_and(|state| state.selection.contains_position(&position)))
                {
                    continue;
                }

                // => Remove the cursor
                states.remove(index);

                view_model.model().push_stack_element();
                view_model.set_cursor_states(args.source(), CursorChangeReason::Explicit, Some(states));
                return;
            }
        }

        // => Add the new cursor
        states.push(new_state);

        view_model.model().push_stack_element();
        view_model.set_cursor_states(args.source(), CursorChangeReason::Explicit, Some(states));
    }

    /// `LastCursorMoveToSelect`, `LastCursorWordSelect` e `LastCursorLineCommand`: trocam o estado do último cursor
    /// adicionado pelo que `compute` devolve (que recebe o estado atual e a posição do argumento).
    fn last_cursor_command(
        id: &str,
        compute: impl Fn(&EditorViewModel, &CursorState, &MoveCommandOptions, Position) -> PartialCursorState + 'static,
    ) -> CoreEditorCommand<MoveCommandOptions> {
        CoreEditorCommand::new(CommandOptions::new(id), move |view_model: &EditorViewModel, args: &MoveCommandOptions| {
            let Some(position) = args.position else {
                return;
            };
            let last_added_cursor_index = view_model.get_last_added_cursor_index();

            let states = view_model.get_cursor_states();
            let mut new_states: Vec<PartialCursorState> = states.iter().copied().map(PartialCursorState::Full).collect();
            new_states[last_added_cursor_index] = compute(view_model, &states[last_added_cursor_index], args, position);

            view_model.model().push_stack_element();
            view_model.set_cursor_states(args.source(), CursorChangeReason::Explicit, Some(new_states));
        })
    }

    /// `HomeCommand`, `LineStartCommand`, `EndCommand`, `LineEndCommand`, `TopCommand`, `BottomCommand`,
    /// `CancelSelection` e `RemoveSecondaryCursors`: empilha um ponto de desfazer, troca os estados dos cursores
    /// pelos que `compute` devolve e revela os cursores.
    fn cursor_states_command<T: CoreCommandArgs>(
        opts: CommandOptions,
        compute: impl Fn(&EditorViewModel, &T) -> Vec<PartialCursorState> + 'static,
    ) -> CoreEditorCommand<T> {
        CoreEditorCommand::new(opts, move |view_model: &EditorViewModel, args: &T| {
            view_model.model().push_stack_element();
            view_model.set_cursor_states(args.source(), CursorChangeReason::Explicit, Some(compute(view_model, args)));
            view_model.reveal_all_cursors(args.source(), true, None);
        })
    }

    /// `HomeCommand`, `TopCommand` e `BottomCommand`: um `CursorMoveCommands.moveTo...` que só recebe o modo de seleção.
    fn move_to_boundary_command(
        id: &str,
        in_selection_mode: bool,
        keybindings: Keybindings,
        move_to_boundary: fn(&EditorViewModel, &[CursorState], bool) -> Vec<PartialCursorState>,
    ) -> CoreEditorCommand<BaseCommandOptions> {
        cursor_states_command(
            text_input_command_options(id, keybindings),
            move |view_model: &EditorViewModel, _args: &BaseCommandOptions| {
                move_to_boundary(view_model, &view_model.get_cursor_states(), in_selection_mode)
            },
        )
    }

    /// `LineStartCommand` e `LineEndCommand`: move cada cursor para a coluna que `column_of` calcula na linha do
    /// modelo (a coluna 1, ou a coluna máxima).
    fn model_line_column_command(
        id: &str,
        in_selection_mode: bool,
        keybindings: Keybindings,
        column_of: fn(&TextModel, i32) -> i32,
    ) -> CoreEditorCommand<BaseCommandOptions> {
        cursor_states_command(
            text_input_command_options(id, keybindings),
            move |view_model: &EditorViewModel, _args: &BaseCommandOptions| {
                let model = view_model.model();
                view_model
                    .get_cursor_states()
                    .iter()
                    .map(|cursor| {
                        let line_number = cursor.model_state.position.line_number;
                        PartialCursorState::Model(CursorState::from_model_state(cursor.model_state.move_to(
                            in_selection_mode,
                            line_number,
                            column_of(&model, line_number),
                            0,
                        )))
                    })
                    .collect()
            },
        )
    }

    /// `EndCommand`: `moveToEndOfLine` com o argumento `sticky` (e a descrição do comando nos metadados).
    fn end_command(id: &str, in_selection_mode: bool, description: &str, keybindings: Keybindings) -> CoreEditorCommand<EndCommandOptions> {
        let sticky_description = localize("stickydesc", "Stick to the end even when going to longer lines", &[]);
        let schema = Dynamic::object([
            ("type", Dynamic::from("object")),
            (
                "properties",
                Dynamic::object([(
                    "sticky",
                    Dynamic::object([
                        ("description", Dynamic::from(sticky_description.as_str())),
                        ("type", Dynamic::from("boolean")),
                        ("default", Dynamic::Bool(false)),
                    ]),
                )]),
            ),
        ]);
        let opts = CommandOptions {
            kb_opts: text_input_keybindings(keybindings)
                .into_iter()
                .map(|opts| CommandKeybindingsOptions { args: Dynamic::object([("sticky", Dynamic::Bool(false))]), ..opts })
                .collect(),
            metadata: Some(args_metadata(description, schema)),
            ..CommandOptions::new(id)
        };
        cursor_states_command(opts, move |view_model: &EditorViewModel, args: &EndCommandOptions| {
            move_commands::move_to_end_of_line(view_model, &view_model.get_cursor_states(), in_selection_mode, args.sticky)
        })
    }

    /// `WordCommand` e `LineCommand`: a seleção por gesto (duplo clique, clique no número da linha) do cursor
    /// primário, na posição do argumento.
    fn selection_gesture_command(
        id: &str,
        reveal_horizontal: bool,
        select: impl Fn(&EditorViewModel, &MoveCommandOptions, Position) -> PartialCursorState + 'static,
    ) -> CoreEditorCommand<MoveCommandOptions> {
        CoreEditorCommand::new(CommandOptions::new(id), move |view_model: &EditorViewModel, args: &MoveCommandOptions| {
            let Some(position) = args.position else {
                return;
            };
            view_model.model().push_stack_element();
            view_model.set_cursor_states(args.source(), CursorChangeReason::Explicit, Some(vec![select(view_model, args, position)]));
            if args.reveal_type != Some(NavigationCommandRevealType::None) {
                view_model.reveal_all_cursors(args.source(), reveal_horizontal, Some(true));
            }
        })
    }

    /// O método de rolagem (vertical ou horizontal) que `determineScrollMethod` escolhe.
    type ScrollMethod = fn(&EditorViewModel, Option<&str>, &editor_scroll::ParsedArguments);

    /// `EditorScrollImpl.determineScrollMethod`: `None` (o `null` do original) se a unidade e a direção não combinam.
    fn determine_scroll_method(args: &editor_scroll::ParsedArguments) -> Option<ScrollMethod> {
        use editor_scroll::{Direction, Unit};

        let horizontal_units = [Unit::Column];
        let vertical_units = [Unit::Line, Unit::WrappedLine, Unit::Page, Unit::HalfPage, Unit::Editor];
        let horizontal_directions = [Direction::Left, Direction::Right];
        let vertical_directions = [Direction::Up, Direction::Down];

        if horizontal_units.contains(&args.unit) && horizontal_directions.contains(&args.direction) {
            return Some(run_horizontal_editor_scroll);
        }
        if vertical_units.contains(&args.unit) && vertical_directions.contains(&args.direction) {
            return Some(run_vertical_editor_scroll);
        }
        None
    }

    /// `EditorScrollImpl._runVerticalEditorScroll`.
    fn run_vertical_editor_scroll(view_model: &EditorViewModel, source: Option<&str>, args: &editor_scroll::ParsedArguments) {
        let desired_scroll_top = compute_desired_scroll_top(view_model, args);

        if args.reveal_cursor {
            // must ensure cursor is in new visible range
            let desired_visible_view_range = view_model.get_completely_visible_view_range_at_scroll_top(desired_scroll_top);
            let padded_range = view_model.get_view_range_with_cursor_padding(desired_visible_view_range);

            view_model.set_cursor_states(
                source,
                CursorChangeReason::Explicit,
                Some(vec![move_commands::find_position_in_viewport_if_outside(
                    view_model,
                    &view_model.get_primary_cursor_state(),
                    &padded_range,
                    args.select,
                )]),
            );
        }

        view_model
            .view_layout()
            .set_scroll_position(INewScrollPosition { scroll_left: None, scroll_top: Some(desired_scroll_top) }, ScrollType::Smooth);
    }

    /// `EditorScrollImpl._computeDesiredScrollTop`.
    fn compute_desired_scroll_top(view_model: &EditorViewModel, args: &editor_scroll::ParsedArguments) -> f64 {
        use editor_scroll::{Direction, Unit};

        let view_layout = view_model.view_layout();
        let cursor_config = view_model.cursor_config();

        if args.unit == Unit::Line {
            // scrolling by model lines
            let future_viewport = view_layout.get_future_viewport();
            let visible_view_range = view_model.get_completely_visible_view_range_at_scroll_top(future_viewport.top);
            let coordinates_converter = view_model.coordinates_converter();
            let visible_model_range = coordinates_converter.convert_view_range_to_model_range(visible_view_range);
            let visible_start_line_number = f64::from(visible_model_range.start_line_number);

            let desired_top_model_line_number = if args.direction == Direction::Up {
                // must go x model lines up
                1.0_f64.max(visible_start_line_number - args.value)
            } else {
                // must go x model lines down
                f64::from(view_model.model().get_line_count()).min(visible_start_line_number + args.value)
            };

            let view_position = coordinates_converter.convert_model_position_to_view_position(
                Position::new(desired_top_model_line_number as i32, 1),
                None,
                None,
                None,
            );
            return view_layout.get_vertical_offset_for_line_number(view_position.line_number, None);
        }

        if args.unit == Unit::Editor {
            let mut desired_top_model_line_number = 0.0;
            if args.direction == Direction::Down {
                desired_top_model_line_number = f64::from(view_model.model().get_line_count()) - cursor_config.page_size;
            }
            return view_layout.get_vertical_offset_for_line_number(desired_top_model_line_number as i32, None);
        }

        let no_of_lines = match args.unit {
            Unit::Page => cursor_config.page_size * args.value,
            // `Math.round(pageSize / 2)`
            Unit::HalfPage => (cursor_config.page_size / 2.0 + 0.5).floor() * args.value,
            _ => args.value,
        };
        let delta_lines = (if args.direction == Direction::Up { -1.0 } else { 1.0 }) * no_of_lines;
        view_layout.get_current_scroll_top() + delta_lines * cursor_config.line_height
    }

    /// `EditorScrollImpl._runHorizontalEditorScroll` (o `source` não é lido, como no original).
    fn run_horizontal_editor_scroll(view_model: &EditorViewModel, _source: Option<&str>, args: &editor_scroll::ParsedArguments) {
        let desired_scroll_left = compute_desired_scroll_left(view_model, args);
        view_model
            .view_layout()
            .set_scroll_position(INewScrollPosition { scroll_left: Some(desired_scroll_left), scroll_top: None }, ScrollType::Smooth);
    }

    /// `EditorScrollImpl._computeDesiredScrollLeft`.
    fn compute_desired_scroll_left(view_model: &EditorViewModel, args: &editor_scroll::ParsedArguments) -> f64 {
        let delta_columns = (if args.direction == editor_scroll::Direction::Left { -1.0 } else { 1.0 }) * args.value;
        view_model.view_layout().get_current_scroll_left() + delta_columns * view_model.cursor_config().typical_halfwidth_character_width
    }

    /// `EditorScrollImpl`: o comando `editorScroll`.
    fn editor_scroll_command() -> CoreEditorCommand<EditorScrollCommandOptions> {
        CoreEditorCommand::new(
            CommandOptions { metadata: Some(editor_scroll::metadata()), ..CommandOptions::new("editorScroll") },
            |view_model: &EditorViewModel, args: &EditorScrollCommandOptions| {
                let Some(parsed) = editor_scroll::parse(&args.arguments) else {
                    // illegal arguments
                    return;
                };
                let Some(run_editor_scroll) = determine_scroll_method(&parsed) else {
                    // Incompatible unit and direction
                    return;
                };
                run_editor_scroll(view_model, args.source(), &parsed);
            },
        )
    }

    /// `ScrollLineUp`, `ScrollPageUp`, `ScrollEditorTop`, `ScrollLineDown`, `ScrollPageDown`, `ScrollEditorBottom`,
    /// `ScrollLeft` e `ScrollRight`: chamam o `EditorScroll` (`scroller`) com a direção, a unidade e o valor fixos.
    fn scroll_command(
        id: &str,
        keybindings: Keybindings,
        to: &'static str,
        by: &'static str,
        value: f64,
        scroller: Rc<CoreEditorCommand<EditorScrollCommandOptions>>,
    ) -> CoreEditorCommand<BaseCommandOptions> {
        CoreEditorCommand::new(
            text_input_command_options(id, keybindings),
            move |view_model: &EditorViewModel, args: &BaseCommandOptions| {
                scroller.run_core_editor_command(
                    view_model,
                    &EditorScrollCommandOptions {
                        source: args.source.clone(),
                        arguments: editor_scroll::RawArguments {
                            to: Some(to.to_string()),
                            by: Some(by.to_string()),
                            value: Some(value),
                            reveal_cursor: Some(false),
                            select: Some(false),
                        },
                    },
                );
            },
        )
    }

    /// `RevealLine`: o comando `revealLine`.
    fn reveal_line_command() -> CoreEditorCommand<RevealLineCommandOptions> {
        CoreEditorCommand::new(
            CommandOptions { metadata: Some(reveal_line::metadata()), ..CommandOptions::new("revealLine") },
            |view_model: &EditorViewModel, args: &RevealLineCommandOptions| {
                use reveal_line::{LineNumber, raw_at_argument};

                // `revealLineArg.lineNumber || 0`, e `typeof lineNumberArg === 'number' ? n + 1 : parseInt(n) + 1`
                let mut line_number = match &args.arguments.line_number {
                    Some(LineNumber::Number(number)) if *number != 0.0 && !number.is_nan() => *number + 1.0,
                    Some(LineNumber::Text(text)) if !text.is_empty() => js_parse_int(text) + 1.0,
                    _ => 1.0,
                };
                if line_number.is_nan() {
                    // ver "Divergências"
                    return;
                }
                if line_number < 1.0 {
                    line_number = 1.0;
                }
                let model = view_model.model();
                let line_count = model.get_line_count();
                if line_number > f64::from(line_count) {
                    line_number = f64::from(line_count);
                }
                let line_number = line_number as i32;

                let range = Range::new(line_number, 1, line_number, model.get_line_max_column(line_number));

                let reveal_at = match args.arguments.at.as_deref() {
                    Some(raw_at_argument::TOP) => VerticalRevealType::Top,
                    Some(raw_at_argument::CENTER) => VerticalRevealType::Center,
                    Some(raw_at_argument::BOTTOM) => VerticalRevealType::Bottom,
                    _ => VerticalRevealType::Simple,
                };

                let view_range = view_model.coordinates_converter().convert_model_range_to_view_range(range, None);

                view_model.reveal_range(args.source(), false, view_range, reveal_at, ScrollType::Smooth);
            },
        )
    }

    /// `SelectAll`: o `EditorOrNativeTextInputCommand` do `SelectAllCommand`.
    pub struct SelectAll;

    impl SelectAll {
        /// `runCoreEditorCommand(viewModel, args)`.
        pub fn run_core_editor_command(&self, view_model: &EditorViewModel, _args: &Dynamic) {
            view_model.model().push_stack_element();
            view_model.set_cursor_states(
                Some("keyboard"),
                CursorChangeReason::Explicit,
                Some(vec![move_commands::select_all(view_model, &view_model.get_primary_cursor_state())]),
            );
        }
    }

    impl EditorOrNativeTextInputCommand for SelectAll {
        fn run_dom_command(&self) {
            // `execCommand('selectAll')` é DOM (ver "Divergências"): nunca é chamado.
        }

        fn run_editor_command(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> MaybePromise<()> {
            if let Some(view_model) = editor.get_view_model() {
                self.run_core_editor_command(&view_model, args);
            }
            // else: the editor has no view => has no cursors
            MaybePromise::Value(())
        }
    }

    /// Os comandos de navegação registrados (`MoveTo` até `SetSelection`).
    pub struct CoreNavigationCommands {
        pub move_to: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub move_to_select: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub column_select: Rc<CoreEditorCommand<ColumnSelectCommandOptions>>,
        pub cursor_column_select_left: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_column_select_right: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_column_select_up: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_column_select_page_up: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_column_select_down: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_column_select_page_down: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_move: Rc<CoreEditorCommand<CursorMoveArgs>>,
        pub cursor_left: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_left_select: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_right: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_right_select: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_up: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_up_select: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_page_up: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_page_up_select: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_down: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_down_select: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_page_down: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub cursor_page_down_select: Rc<CoreEditorCommand<CursorMoveCommandOptions>>,
        pub create_cursor: Rc<CoreEditorCommand<CreateCursorCommandOptions>>,
        pub last_cursor_move_to_select: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub cursor_home: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_home_select: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_line_start: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_line_start_select: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_end: Rc<CoreEditorCommand<EndCommandOptions>>,
        pub cursor_end_select: Rc<CoreEditorCommand<EndCommandOptions>>,
        pub cursor_line_end: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_line_end_select: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_top: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_top_select: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_bottom: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub cursor_bottom_select: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub editor_scroll: Rc<CoreEditorCommand<EditorScrollCommandOptions>>,
        pub scroll_line_up: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub scroll_page_up: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub scroll_editor_top: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub scroll_line_down: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub scroll_page_down: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub scroll_editor_bottom: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub scroll_left: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub scroll_right: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub word_select: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub word_select_drag: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub last_cursor_word_select: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub line_select: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub line_select_drag: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub last_cursor_line_select: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub last_cursor_line_select_drag: Rc<CoreEditorCommand<MoveCommandOptions>>,
        pub cancel_selection: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub remove_secondary_cursors: Rc<CoreEditorCommand<BaseCommandOptions>>,
        pub reveal_line: Rc<CoreEditorCommand<RevealLineCommandOptions>>,
        pub select_all: Rc<SelectAll>,
        pub set_selection: Rc<CoreEditorCommand<SetSelectionCommandOptions>>,
    }

    thread_local! {
        static COMMANDS: Rc<CoreNavigationCommands> = Rc::new(register());
    }

    /// Os comandos registrados (o registro acontece no primeiro acesso).
    pub fn instance() -> Rc<CoreNavigationCommands> {
        COMMANDS.with(Rc::clone)
    }

    /// Cria e registra cada comando, na ordem do original.
    fn register() -> CoreNavigationCommands {
        use cursor_move::{Direction, Unit};

        let move_to = register_core_command(base_move_to_command("_moveTo", false));
        let move_to_select = register_core_command(base_move_to_command("_moveToSelect", true));

        let column_select = register_core_command(column_select_command(
            CommandOptions::new("columnSelect"),
            |view_model, _primary, prev_column_select_data, args: &ColumnSelectCommandOptions| {
                let (Some(position), Some(view_position), Some(mouse_column)) = (args.position, args.view_position, args.mouse_column) else {
                    return None;
                };
                // validate `args`
                let validated_position = view_model.model().validate_position(position);
                let validated_view_position = view_model
                    .coordinates_converter()
                    .validate_view_position(Position::new(view_position.line_number, view_position.column), validated_position);

                let from_view_line_number =
                    if args.do_column_select { prev_column_select_data.from_view_line_number } else { validated_view_position.line_number };
                let from_view_visual_column =
                    if args.do_column_select { prev_column_select_data.from_view_visual_column } else { mouse_column - 1 };
                Some(column_selection::column_select(
                    &view_model.cursor_config(),
                    view_model,
                    from_view_line_number,
                    from_view_visual_column,
                    validated_view_position.line_number,
                    mouse_column - 1,
                ))
            },
        ));

        let cursor_column_select_left = register_core_command(column_select_command(
            column_select_options("cursorColumnSelectLeft", KeyCode::LeftArrow),
            |view_model, _primary, prev_column_select_data, _args: &BaseCommandOptions| {
                Some(column_selection::column_select_left(&view_model.cursor_config(), view_model, prev_column_select_data))
            },
        ));

        let cursor_column_select_right = register_core_command(column_select_command(
            column_select_options("cursorColumnSelectRight", KeyCode::RightArrow),
            |view_model, _primary, prev_column_select_data, _args: &BaseCommandOptions| {
                Some(column_selection::column_select_right(&view_model.cursor_config(), view_model, prev_column_select_data))
            },
        ));

        let cursor_column_select_up =
            register_core_command(column_select_vertical_command("cursorColumnSelectUp", KeyCode::UpArrow, false, column_selection::column_select_up));
        let cursor_column_select_page_up =
            register_core_command(column_select_vertical_command("cursorColumnSelectPageUp", KeyCode::PageUp, true, column_selection::column_select_up));
        let cursor_column_select_down =
            register_core_command(column_select_vertical_command("cursorColumnSelectDown", KeyCode::DownArrow, false, column_selection::column_select_down));
        let cursor_column_select_page_down = register_core_command(column_select_vertical_command(
            "cursorColumnSelectPageDown",
            KeyCode::PageDown,
            true,
            column_selection::column_select_down,
        ));

        let cursor_move = register_core_command(CoreEditorCommand::new(
            CommandOptions {
                metadata: Some(convert_cursor_move_metadata(cursor_move::metadata())),
                ..CommandOptions::new("cursorMove")
            },
            |view_model, args: &CursorMoveArgs| {
                let Some(parsed) = cursor_move::parse(&args.arguments) else {
                    // illegal arguments
                    return;
                };
                run_cursor_move(view_model, args.source(), &parsed);
            },
        ));

        let ctrl_cmd = KeyMod::CTRL_CMD;
        let shift = KeyMod::SHIFT;
        let win_ctrl = KeyMod::WIN_CTRL;

        let cursor_left = register_core_command(cursor_move_based_command(
            "cursorLeft",
            move_args(Direction::Left, Unit::None, false, 1),
            Keybindings {
                primary: Some(KeyCode::LeftArrow.bits()),
                mac: platform_keybindings(KeyCode::LeftArrow.bits(), Some(vec![key(win_ctrl, KeyCode::KeyB)])),
                ..Keybindings::default()
            },
        ));
        let cursor_left_select = register_core_command(cursor_move_based_command(
            "cursorLeftSelect",
            move_args(Direction::Left, Unit::None, true, 1),
            Keybindings { primary: Some(key(shift, KeyCode::LeftArrow)), ..Keybindings::default() },
        ));
        let cursor_right = register_core_command(cursor_move_based_command(
            "cursorRight",
            move_args(Direction::Right, Unit::None, false, 1),
            Keybindings {
                primary: Some(KeyCode::RightArrow.bits()),
                mac: platform_keybindings(KeyCode::RightArrow.bits(), Some(vec![key(win_ctrl, KeyCode::KeyF)])),
                ..Keybindings::default()
            },
        ));
        let cursor_right_select = register_core_command(cursor_move_based_command(
            "cursorRightSelect",
            move_args(Direction::Right, Unit::None, true, 1),
            Keybindings { primary: Some(key(shift, KeyCode::RightArrow)), ..Keybindings::default() },
        ));
        let cursor_up = register_core_command(cursor_move_based_command(
            "cursorUp",
            move_args(Direction::Up, Unit::WrappedLine, false, 1),
            Keybindings {
                primary: Some(KeyCode::UpArrow.bits()),
                mac: platform_keybindings(KeyCode::UpArrow.bits(), Some(vec![key(win_ctrl, KeyCode::KeyP)])),
                ..Keybindings::default()
            },
        ));
        let cursor_up_select = register_core_command(cursor_move_based_command(
            "cursorUpSelect",
            move_args(Direction::Up, Unit::WrappedLine, true, 1),
            Keybindings {
                primary: Some(key(shift, KeyCode::UpArrow)),
                secondary: Some(vec![key(ctrl_cmd | shift, KeyCode::UpArrow)]),
                mac: platform_keybindings(key(shift, KeyCode::UpArrow), None),
                linux: platform_keybindings(key(shift, KeyCode::UpArrow), None),
                ..Keybindings::default()
            },
        ));
        let cursor_page_up = register_core_command(cursor_move_based_command(
            "cursorPageUp",
            move_args(Direction::Up, Unit::WrappedLine, false, PAGE_SIZE_MARKER),
            Keybindings { primary: Some(KeyCode::PageUp.bits()), ..Keybindings::default() },
        ));
        let cursor_page_up_select = register_core_command(cursor_move_based_command(
            "cursorPageUpSelect",
            move_args(Direction::Up, Unit::WrappedLine, true, PAGE_SIZE_MARKER),
            Keybindings { primary: Some(key(shift, KeyCode::PageUp)), ..Keybindings::default() },
        ));
        let cursor_down = register_core_command(cursor_move_based_command(
            "cursorDown",
            move_args(Direction::Down, Unit::WrappedLine, false, 1),
            Keybindings {
                primary: Some(KeyCode::DownArrow.bits()),
                mac: platform_keybindings(KeyCode::DownArrow.bits(), Some(vec![key(win_ctrl, KeyCode::KeyN)])),
                ..Keybindings::default()
            },
        ));
        let cursor_down_select = register_core_command(cursor_move_based_command(
            "cursorDownSelect",
            move_args(Direction::Down, Unit::WrappedLine, true, 1),
            Keybindings {
                primary: Some(key(shift, KeyCode::DownArrow)),
                secondary: Some(vec![key(ctrl_cmd | shift, KeyCode::DownArrow)]),
                mac: platform_keybindings(key(shift, KeyCode::DownArrow), None),
                linux: platform_keybindings(key(shift, KeyCode::DownArrow), None),
                ..Keybindings::default()
            },
        ));
        let cursor_page_down = register_core_command(cursor_move_based_command(
            "cursorPageDown",
            move_args(Direction::Down, Unit::WrappedLine, false, PAGE_SIZE_MARKER),
            Keybindings { primary: Some(KeyCode::PageDown.bits()), ..Keybindings::default() },
        ));
        let cursor_page_down_select = register_core_command(cursor_move_based_command(
            "cursorPageDownSelect",
            move_args(Direction::Down, Unit::WrappedLine, true, PAGE_SIZE_MARKER),
            Keybindings { primary: Some(key(shift, KeyCode::PageDown)), ..Keybindings::default() },
        ));

        let create_cursor = register_core_command(CoreEditorCommand::new(CommandOptions::new("createCursor"), run_create_cursor));
        let last_cursor_move_to_select = register_core_command(last_cursor_command(
            "_lastCursorMoveToSelect",
            |view_model, state, args, position| move_commands::move_to(view_model, state, true, position, args.view_position),
        ));

        let alt = KeyMod::ALT;
        let left_arrow = KeyCode::LeftArrow;
        let right_arrow = KeyCode::RightArrow;
        let up_arrow = KeyCode::UpArrow;
        let down_arrow = KeyCode::DownArrow;

        let cursor_home = register_core_command(move_to_boundary_command(
            "cursorHome",
            false,
            Keybindings {
                primary: Some(KeyCode::Home.bits()),
                mac: platform_keybindings(KeyCode::Home.bits(), Some(vec![key(ctrl_cmd, left_arrow)])),
                ..Keybindings::default()
            },
            move_commands::move_to_beginning_of_line,
        ));
        let cursor_home_select = register_core_command(move_to_boundary_command(
            "cursorHomeSelect",
            true,
            Keybindings {
                primary: Some(key(shift, KeyCode::Home)),
                mac: platform_keybindings(key(shift, KeyCode::Home), Some(vec![key(ctrl_cmd | shift, left_arrow)])),
                ..Keybindings::default()
            },
            move_commands::move_to_beginning_of_line,
        ));

        let cursor_line_start = register_core_command(model_line_column_command(
            "cursorLineStart",
            false,
            Keybindings {
                primary: Some(0),
                mac: platform_keybindings(key(win_ctrl, KeyCode::KeyA), None),
                ..Keybindings::default()
            },
            |_model, _line_number| 1,
        ));
        let cursor_line_start_select = register_core_command(model_line_column_command(
            "cursorLineStartSelect",
            true,
            Keybindings {
                primary: Some(0),
                mac: platform_keybindings(key(win_ctrl | shift, KeyCode::KeyA), None),
                ..Keybindings::default()
            },
            |_model, _line_number| 1,
        ));

        let cursor_end = register_core_command(end_command(
            "cursorEnd",
            false,
            "Go to End",
            Keybindings {
                primary: Some(KeyCode::End.bits()),
                mac: platform_keybindings(KeyCode::End.bits(), Some(vec![key(ctrl_cmd, right_arrow)])),
                ..Keybindings::default()
            },
        ));
        let cursor_end_select = register_core_command(end_command(
            "cursorEndSelect",
            true,
            "Select to End",
            Keybindings {
                primary: Some(key(shift, KeyCode::End)),
                mac: platform_keybindings(key(shift, KeyCode::End), Some(vec![key(ctrl_cmd | shift, right_arrow)])),
                ..Keybindings::default()
            },
        ));

        let cursor_line_end = register_core_command(model_line_column_command(
            "cursorLineEnd",
            false,
            Keybindings {
                primary: Some(0),
                mac: platform_keybindings(key(win_ctrl, KeyCode::KeyE), None),
                ..Keybindings::default()
            },
            |model, line_number| model.get_line_max_column(line_number),
        ));
        let cursor_line_end_select = register_core_command(model_line_column_command(
            "cursorLineEndSelect",
            true,
            Keybindings {
                primary: Some(0),
                mac: platform_keybindings(key(win_ctrl | shift, KeyCode::KeyE), None),
                ..Keybindings::default()
            },
            |model, line_number| model.get_line_max_column(line_number),
        ));

        let cursor_top = register_core_command(move_to_boundary_command(
            "cursorTop",
            false,
            Keybindings {
                primary: Some(key(ctrl_cmd, KeyCode::Home)),
                mac: platform_keybindings(key(ctrl_cmd, up_arrow), None),
                ..Keybindings::default()
            },
            move_commands::move_to_beginning_of_buffer,
        ));
        let cursor_top_select = register_core_command(move_to_boundary_command(
            "cursorTopSelect",
            true,
            Keybindings {
                primary: Some(key(ctrl_cmd | shift, KeyCode::Home)),
                mac: platform_keybindings(key(ctrl_cmd | shift, up_arrow), None),
                ..Keybindings::default()
            },
            move_commands::move_to_beginning_of_buffer,
        ));
        let cursor_bottom = register_core_command(move_to_boundary_command(
            "cursorBottom",
            false,
            Keybindings {
                primary: Some(key(ctrl_cmd, KeyCode::End)),
                mac: platform_keybindings(key(ctrl_cmd, down_arrow), None),
                ..Keybindings::default()
            },
            move_commands::move_to_end_of_buffer,
        ));
        let cursor_bottom_select = register_core_command(move_to_boundary_command(
            "cursorBottomSelect",
            true,
            Keybindings {
                primary: Some(key(ctrl_cmd | shift, KeyCode::End)),
                mac: platform_keybindings(key(ctrl_cmd | shift, down_arrow), None),
                ..Keybindings::default()
            },
            move_commands::move_to_end_of_buffer,
        ));

        let editor_scroll = register_core_command(editor_scroll_command());

        let scroll_line_up = register_core_command(scroll_command(
            "scrollLineUp",
            Keybindings {
                primary: Some(key(ctrl_cmd, up_arrow)),
                mac: platform_keybindings(key(win_ctrl, KeyCode::PageUp), None),
                ..Keybindings::default()
            },
            editor_scroll::raw_direction::UP,
            editor_scroll::raw_unit::WRAPPED_LINE,
            1.0,
            Rc::clone(&editor_scroll),
        ));
        let scroll_page_up = register_core_command(scroll_command(
            "scrollPageUp",
            Keybindings {
                primary: Some(key(ctrl_cmd, KeyCode::PageUp)),
                win: platform_keybindings(key(alt, KeyCode::PageUp), None),
                linux: platform_keybindings(key(alt, KeyCode::PageUp), None),
                ..Keybindings::default()
            },
            editor_scroll::raw_direction::UP,
            editor_scroll::raw_unit::PAGE,
            1.0,
            Rc::clone(&editor_scroll),
        ));
        let scroll_editor_top = register_core_command(scroll_command(
            "scrollEditorTop",
            Keybindings::default(),
            editor_scroll::raw_direction::UP,
            editor_scroll::raw_unit::EDITOR,
            1.0,
            Rc::clone(&editor_scroll),
        ));
        let scroll_line_down = register_core_command(scroll_command(
            "scrollLineDown",
            Keybindings {
                primary: Some(key(ctrl_cmd, down_arrow)),
                mac: platform_keybindings(key(win_ctrl, KeyCode::PageDown), None),
                ..Keybindings::default()
            },
            editor_scroll::raw_direction::DOWN,
            editor_scroll::raw_unit::WRAPPED_LINE,
            1.0,
            Rc::clone(&editor_scroll),
        ));
        let scroll_page_down = register_core_command(scroll_command(
            "scrollPageDown",
            Keybindings {
                primary: Some(key(ctrl_cmd, KeyCode::PageDown)),
                win: platform_keybindings(key(alt, KeyCode::PageDown), None),
                linux: platform_keybindings(key(alt, KeyCode::PageDown), None),
                ..Keybindings::default()
            },
            editor_scroll::raw_direction::DOWN,
            editor_scroll::raw_unit::PAGE,
            1.0,
            Rc::clone(&editor_scroll),
        ));
        let scroll_editor_bottom = register_core_command(scroll_command(
            "scrollEditorBottom",
            Keybindings::default(),
            editor_scroll::raw_direction::DOWN,
            editor_scroll::raw_unit::EDITOR,
            1.0,
            Rc::clone(&editor_scroll),
        ));
        let scroll_left = register_core_command(scroll_command(
            "scrollLeft",
            Keybindings::default(),
            editor_scroll::raw_direction::LEFT,
            editor_scroll::raw_unit::COLUMN,
            2.0,
            Rc::clone(&editor_scroll),
        ));
        let scroll_right = register_core_command(scroll_command(
            "scrollRight",
            Keybindings::default(),
            editor_scroll::raw_direction::RIGHT,
            editor_scroll::raw_unit::COLUMN,
            2.0,
            Rc::clone(&editor_scroll),
        ));

        let word_select = register_core_command(selection_gesture_command("_wordSelect", true, |view_model, _args, position| {
            move_commands::word(view_model, &view_model.get_primary_cursor_state(), false, position)
        }));
        let word_select_drag = register_core_command(selection_gesture_command("_wordSelectDrag", true, |view_model, _args, position| {
            move_commands::word(view_model, &view_model.get_primary_cursor_state(), true, position)
        }));
        let last_cursor_word_select = register_core_command(last_cursor_command(
            "lastCursorWordSelect",
            |view_model, last_added_state, _args, position| {
                move_commands::word(view_model, last_added_state, last_added_state.model_state.has_selection(), position)
            },
        ));

        let line_select = register_core_command(selection_gesture_command("_lineSelect", false, |view_model, args, position| {
            move_commands::line(view_model, &view_model.get_primary_cursor_state(), false, position, args.view_position)
        }));
        let line_select_drag = register_core_command(selection_gesture_command("_lineSelectDrag", false, |view_model, args, position| {
            move_commands::line(view_model, &view_model.get_primary_cursor_state(), true, position, args.view_position)
        }));
        let last_cursor_line_select = register_core_command(last_cursor_command(
            "lastCursorLineSelect",
            |view_model, state, args, position| move_commands::line(view_model, state, false, position, args.view_position),
        ));
        let last_cursor_line_select_drag = register_core_command(last_cursor_command(
            "lastCursorLineSelectDrag",
            |view_model, state, args, position| move_commands::line(view_model, state, true, position, args.view_position),
        ));

        let cancel_selection = register_core_command(cursor_states_command(
            CommandOptions {
                precondition: Some(has_non_empty_selection()),
                kb_opts: text_input_keybindings(Keybindings {
                    primary: Some(KeyCode::Escape.bits()),
                    secondary: Some(vec![key(shift, KeyCode::Escape)]),
                    ..Keybindings::default()
                }),
                ..CommandOptions::new("cancelSelection")
            },
            |view_model: &EditorViewModel, _args: &BaseCommandOptions| {
                vec![move_commands::cancel_selection(view_model, &view_model.get_primary_cursor_state())]
            },
        ));

        let remove_secondary_cursors = register_core_command(cursor_states_command(
            CommandOptions {
                precondition: Some(has_multiple_selections()),
                kb_opts: vec![CommandKeybindingsOptions::new(
                    CORE_WEIGHT + 1,
                    Some(text_input_focus()),
                    Keybindings {
                        primary: Some(KeyCode::Escape.bits()),
                        secondary: Some(vec![key(shift, KeyCode::Escape)]),
                        ..Keybindings::default()
                    },
                )],
                ..CommandOptions::new("removeSecondaryCursors")
            },
            |view_model: &EditorViewModel, _args: &BaseCommandOptions| {
                // o `status("Removed secondary cursors")` do original é DOM (ver "Divergências")
                vec![PartialCursorState::Full(view_model.get_primary_cursor_state())]
            },
        ));

        let reveal_line = register_core_command(reveal_line_command());

        let select_all = Rc::new(SelectAll);
        register_editor_or_native_text_input_command(Rc::clone(&select_all), &select_all_command());

        let set_selection = register_core_command(CoreEditorCommand::new(
            CommandOptions::new("setSelection"),
            |view_model: &EditorViewModel, args: &SetSelectionCommandOptions| {
                let Some(selection) = args.selection else {
                    return;
                };
                view_model.model().push_stack_element();
                view_model.set_cursor_states(
                    args.source(),
                    CursorChangeReason::Explicit,
                    Some(vec![PartialCursorState::Model(CursorState::from_model_selection(selection))]),
                );
            },
        ));

        CoreNavigationCommands {
            move_to,
            move_to_select,
            column_select,
            cursor_column_select_left,
            cursor_column_select_right,
            cursor_column_select_up,
            cursor_column_select_page_up,
            cursor_column_select_down,
            cursor_column_select_page_down,
            cursor_move,
            cursor_left,
            cursor_left_select,
            cursor_right,
            cursor_right_select,
            cursor_up,
            cursor_up_select,
            cursor_page_up,
            cursor_page_up_select,
            cursor_down,
            cursor_down_select,
            cursor_page_down,
            cursor_page_down_select,
            create_cursor,
            last_cursor_move_to_select,
            cursor_home,
            cursor_home_select,
            cursor_line_start,
            cursor_line_start_select,
            cursor_end,
            cursor_end_select,
            cursor_line_end,
            cursor_line_end_select,
            cursor_top,
            cursor_top_select,
            cursor_bottom,
            cursor_bottom_select,
            editor_scroll,
            scroll_line_up,
            scroll_page_up,
            scroll_editor_top,
            scroll_line_down,
            scroll_page_down,
            scroll_editor_bottom,
            scroll_left,
            scroll_right,
            word_select,
            word_select_drag,
            last_cursor_word_select,
            line_select,
            line_select_drag,
            last_cursor_line_select,
            last_cursor_line_select_drag,
            cancel_selection,
            remove_secondary_cursors,
            reveal_line,
            select_all,
            set_selection,
        }
    }
}

// #endregion

// #region registerColumnSelection

/// `columnSelectionCondition`: `ContextKeyExpr.and(textInputFocus, columnSelection)`.
fn column_selection_condition() -> Option<ContextKeyExpression> {
    ContextKeyExpr::and([text_input_focus(), column_selection_enabled()])
}

/// `registerColumnSelection(id, keybinding)`.
fn register_column_selection(id: &str, keybinding: u32) {
    keybindings_registry().register_keybinding_rule(KeybindingRule {
        keybindings: Keybindings { primary: Some(keybinding), ..Keybindings::default() },
        id: id.to_string(),
        weight: CORE_WEIGHT + 1,
        args: Dynamic::Undefined,
        when: column_selection_condition(),
    });
}

// #endregion

// #region CoreEditingCommands

/// `CoreEditingCommands`: os comandos de edição do núcleo (o namespace do original).
pub mod core_editing_commands {
    use super::*;

    /// `abstract class CoreEditingCommand extends EditorCommand`: um comando que edita pelo editor e pelo
    /// `ViewModel`; `run` é o `runCoreEditingCommand(editor, viewModel, args)` (e recebe também o `id`, que o
    /// original lê de `this.id` para `executeCommands`).
    pub struct CoreEditingCommand {
        base: Command,
        run: Box<dyn Fn(&str, &Rc<CodeEditorWidget>, &EditorViewModel, &Dynamic)>,
    }

    impl CoreEditingCommand {
        /// Cria o comando; `run` é o `runCoreEditingCommand`.
        pub fn new(
            opts: CommandOptions,
            run: impl Fn(&str, &Rc<CodeEditorWidget>, &EditorViewModel, &Dynamic) + 'static,
        ) -> Self {
            Self { base: Command::new(opts), run: Box::new(run) }
        }
    }

    impl IEditorCommand for CoreEditingCommand {
        /// `runEditorCommand(accessor, editor, args)`.
        fn run_editor_command(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
            let Some(view_model) = editor.get_view_model() else {
                // the editor has no view => has no cursors
                return Promise::resolved(());
            };
            // `args || {}`
            let args = if args.is_truthy() { args.clone() } else { Dynamic::object(Vec::<(&str, Dynamic)>::new()) };
            (self.run)(&self.base.id, editor, &*view_model, &args);
            Promise::resolved(())
        }
    }

    impl EditorCommand for CoreEditingCommand {
        fn base(&self) -> &Command {
            &self.base
        }
    }

    /// `registerEditorCommand(new class extends CoreEditingCommand { ... })`.
    fn register_core_editing_command(command: CoreEditingCommand) -> Rc<CoreEditingCommand> {
        register_editor_command(Rc::new(command))
    }

    /// `viewModel.getCursorStates().map(s => s.modelState.selection)`.
    fn model_selections(view_model: &EditorViewModel) -> Vec<Selection> {
        view_model.get_cursor_states().iter().map(|state| state.model_state.selection).collect()
    }

    /// `ICommand[]` como o `(ICommand | null)[]` de `executeCommands`.
    fn nullable_commands(commands: Vec<Box<dyn EditCommand>>) -> Vec<Option<Box<dyn EditCommand>>> {
        commands.into_iter().map(Some).collect()
    }

    /// `Outdent` e `Tab`: o atalho só vale com o texto em foco e a tecla Tab sem mover o foco.
    fn tab_options(id: &str, primary: u32) -> CommandOptions {
        CommandOptions {
            precondition: Some(writable()),
            kb_opts: vec![CommandKeybindingsOptions::new(
                CORE_WEIGHT,
                ContextKeyExpr::and([editor_text_focus(), tab_does_not_move_focus()]),
                Keybindings { primary: Some(primary), ..Keybindings::default() },
            )],
            ..CommandOptions::new(id)
        }
    }

    /// `UndoCommand` e `RedoCommand` do `CoreEditingCommands.Undo`/`Redo` (um `EditorOrNativeTextInputCommand`).
    pub struct UndoRedoCommand {
        is_redo: bool,
    }

    impl EditorOrNativeTextInputCommand for UndoRedoCommand {
        fn run_dom_command(&self) {
            // `execCommand('undo')`/`execCommand('redo')` são DOM (ver "Divergências"): nunca é chamado.
        }

        fn run_editor_command(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> MaybePromise<()> {
            if !editor.has_model() || *editor.get_option(READ_ONLY) {
                return MaybePromise::Value(());
            }
            if let Some(model) = editor.get_model() {
                if self.is_redo {
                    model.redo();
                } else {
                    model.undo();
                }
            }
            MaybePromise::Value(())
        }
    }

    /// Os comandos de edição registrados.
    pub struct CoreEditingCommands {
        pub line_break_insert: Rc<CoreEditingCommand>,
        pub outdent: Rc<CoreEditingCommand>,
        pub tab: Rc<CoreEditingCommand>,
        pub delete_left: Rc<CoreEditingCommand>,
        pub delete_right: Rc<CoreEditingCommand>,
        pub undo: Rc<UndoRedoCommand>,
        pub redo: Rc<UndoRedoCommand>,
    }

    thread_local! {
        static COMMANDS: Rc<CoreEditingCommands> = Rc::new(register());
    }

    /// Os comandos registrados (o registro acontece no primeiro acesso).
    pub fn instance() -> Rc<CoreEditingCommands> {
        COMMANDS.with(Rc::clone)
    }

    /// Cria e registra cada comando, na ordem do original.
    fn register() -> CoreEditingCommands {
        let line_break_insert = register_core_editing_command(CoreEditingCommand::new(
            CommandOptions {
                precondition: Some(writable()),
                kb_opts: text_input_keybindings(Keybindings {
                    primary: Some(0),
                    mac: platform_keybindings(key(KeyMod::WIN_CTRL, KeyCode::KeyO), None),
                    ..Keybindings::default()
                }),
                ..CommandOptions::new("lineBreakInsert")
            },
            |id, editor, view_model, _args| {
                editor.push_undo_stop();
                let model = view_model.model();
                editor.execute_commands(
                    Some(id),
                    nullable_commands(EnterOperation::line_break_insert(&view_model.cursor_config(), &model, &model_selections(view_model))),
                );
            },
        ));

        let outdent = register_core_editing_command(CoreEditingCommand::new(
            tab_options("outdent", key(KeyMod::SHIFT, KeyCode::Tab)),
            |id, editor, view_model, _args| {
                editor.push_undo_stop();
                let model = view_model.model();
                editor.execute_commands(
                    Some(id),
                    nullable_commands(TypeOperations::outdent(&view_model.cursor_config(), &*model, &model_selections(view_model))),
                );
                editor.push_undo_stop();
            },
        ));

        let tab = register_core_editing_command(CoreEditingCommand::new(
            tab_options("tab", KeyCode::Tab.bits()),
            |id, editor, view_model, _args| {
                editor.push_undo_stop();
                let model = view_model.model();
                editor.execute_commands(
                    Some(id),
                    nullable_commands(TabOperation::get_commands(&view_model.cursor_config(), &model, &model_selections(view_model))),
                );
                editor.push_undo_stop();
            },
        ));

        let delete_left = register_core_editing_command(CoreEditingCommand::new(
            text_input_command_options(
                "deleteLeft",
                Keybindings {
                    primary: Some(KeyCode::Backspace.bits()),
                    secondary: Some(vec![key(KeyMod::SHIFT, KeyCode::Backspace)]),
                    mac: platform_keybindings(
                        KeyCode::Backspace.bits(),
                        Some(vec![
                            key(KeyMod::SHIFT, KeyCode::Backspace),
                            key(KeyMod::WIN_CTRL, KeyCode::KeyH),
                            key(KeyMod::WIN_CTRL, KeyCode::Backspace),
                        ]),
                    ),
                    ..Keybindings::default()
                },
            ),
            |id, editor, view_model, _args| {
                let model = view_model.model();
                let (should_push_stack_element_before, commands) = DeleteOperations::delete_left(
                    view_model.get_prev_edit_operation_type(),
                    &view_model.cursor_config(),
                    &*model,
                    &model_selections(view_model),
                    &view_model.get_cursor_auto_closed_characters(),
                );
                if should_push_stack_element_before {
                    editor.push_undo_stop();
                }
                editor.execute_commands(Some(id), commands);
                view_model.set_prev_edit_operation_type(EditOperationType::DeletingLeft);
            },
        ));

        let delete_right = register_core_editing_command(CoreEditingCommand::new(
            text_input_command_options(
                "deleteRight",
                Keybindings {
                    primary: Some(KeyCode::Delete.bits()),
                    mac: platform_keybindings(
                        KeyCode::Delete.bits(),
                        Some(vec![key(KeyMod::WIN_CTRL, KeyCode::KeyD), key(KeyMod::WIN_CTRL, KeyCode::Delete)]),
                    ),
                    ..Keybindings::default()
                },
            ),
            |id, editor, view_model, _args| {
                let model = view_model.model();
                let (should_push_stack_element_before, commands) = DeleteOperations::delete_right(
                    view_model.get_prev_edit_operation_type(),
                    &view_model.cursor_config(),
                    &*model,
                    &model_selections(view_model),
                );
                if should_push_stack_element_before {
                    editor.push_undo_stop();
                }
                editor.execute_commands(Some(id), commands);
                view_model.set_prev_edit_operation_type(EditOperationType::DeletingRight);
            },
        ));

        let undo = Rc::new(UndoRedoCommand { is_redo: false });
        register_editor_or_native_text_input_command(Rc::clone(&undo), &undo_command());

        let redo = Rc::new(UndoRedoCommand { is_redo: true });
        register_editor_or_native_text_input_command(Rc::clone(&redo), &redo_command());

        CoreEditingCommands { line_break_insert, outdent, tab, delete_left, delete_right, undo, redo }
    }
}

// #endregion

// #region EditorHandlerCommand

/// `class EditorHandlerCommand extends Command`: um comando que chama um comando (`handlerId`) do editor com foco.
struct EditorHandlerCommand {
    base: Command,
    handler_id: String,
}

impl EditorHandlerCommand {
    fn new(id: &str, handler_id: &str, metadata: Option<CommandMetadata>) -> Self {
        Self { base: Command::new(CommandOptions { metadata, ..CommandOptions::new(id) }), handler_id: handler_id.to_string() }
    }
}

impl ICommand for EditorHandlerCommand {
    fn base(&self) -> &Command {
        &self.base
    }

    fn run_command(&self, accessor: &ServicesAccessor, args: &Dynamic) -> Promise<()> {
        let Some(editor) = accessor.get(I_CODE_EDITOR_LOOKUP).get_focused_code_editor() else {
            return Promise::resolved(());
        };

        editor.trigger(Some("keyboard"), &self.handler_id, Some(args.clone()));
        Promise::resolved(())
    }
}

/// `registerOverwritableCommand(handlerId, metadata?)`: registra `default:<handlerId>` e `<handlerId>`.
fn register_overwritable_command(handler_id: &str, metadata: Option<CommandMetadata>) {
    register_command(Rc::new(EditorHandlerCommand::new(&format!("default:{handler_id}"), handler_id, None)));
    register_command(Rc::new(EditorHandlerCommand::new(handler_id, handler_id, metadata)));
}

// #endregion

thread_local! {
    /// O que o carregamento do módulo faz no original: criar os comandos de navegação e de edição e fazer os
    /// registros do fim do arquivo (`registerColumnSelection`, `registerOverwritableCommand`).
    static CORE_COMMANDS_REGISTERED: () = register_all();
}

/// Registra os comandos do núcleo do editor (uma vez por thread): `CoreNavigationCommands`,
/// `registerColumnSelection`, `CoreEditingCommands` e `registerOverwritableCommand`, na ordem do original.
pub fn register_core_commands() {
    CORE_COMMANDS_REGISTERED.with(|_| {});
}

fn register_all() {
    let navigation = core_navigation_commands::instance();

    for (command, keybinding) in [
        (navigation.cursor_column_select_left.base().id.as_str(), KeyMod::SHIFT | KeyCode::LeftArrow.bits()),
        (navigation.cursor_column_select_right.base().id.as_str(), KeyMod::SHIFT | KeyCode::RightArrow.bits()),
        (navigation.cursor_column_select_up.base().id.as_str(), KeyMod::SHIFT | KeyCode::UpArrow.bits()),
        (navigation.cursor_column_select_page_up.base().id.as_str(), KeyMod::SHIFT | KeyCode::PageUp.bits()),
        (navigation.cursor_column_select_down.base().id.as_str(), KeyMod::SHIFT | KeyCode::DownArrow.bits()),
        (navigation.cursor_column_select_page_down.base().id.as_str(), KeyMod::SHIFT | KeyCode::PageDown.bits()),
    ] {
        register_column_selection(command, keybinding);
    }

    core_editing_commands::instance();

    let type_schema = Dynamic::object([
        ("type", Dynamic::from("object")),
        ("required", json_strings(&["text"])),
        ("properties", Dynamic::object([("text", Dynamic::object([("type", Dynamic::from("string"))]))])),
    ]);
    register_overwritable_command(Handler::TYPE, Some(args_metadata("Type", type_schema)));
    register_overwritable_command(Handler::REPLACE_PREVIOUS_CHAR, None);
    register_overwritable_command(Handler::COMPOSITION_TYPE, None);
    register_overwritable_command(Handler::COMPOSITION_START, None);
    register_overwritable_command(Handler::COMPOSITION_END, None);
    register_overwritable_command(Handler::PASTE, None);
    register_overwritable_command(Handler::CUT, None);
}
