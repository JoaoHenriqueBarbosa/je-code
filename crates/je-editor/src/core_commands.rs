//! Porte de `src/vs/editor/browser/coreCommands.ts`, linhas 1 a 1003 (parte A): `CoreEditorCommand`,
//! `EditorScroll_`, `RevealLine_`, `EditorOrNativeTextInputCommand`, `NavigationCommandRevealType` e o
//! namespace `CoreNavigationCommands` do início até `LastCursorMoveToSelect` (`MoveTo`, `MoveToSelect`,
//! `ColumnSelect`, `CursorColumnSelect*`, `CursorMove`, `CursorLeft`...`CursorPageDownSelect`, `CreateCursor`,
//! `LastCursorMoveToSelect`). A parte B (do `HomeCommand` em diante) acrescenta a este arquivo o que está
//! listado no comentário final.
//!
//! Os comandos chamam as funções de [`crate::cursor_move_commands`] (`CursorMoveCommands`),
//! [`crate::cursor_column_selection`] (`ColumnSelection`) e o `ViewModel` do editor
//! ([`EditorViewModel`]); o registro é o de [`crate::editor_extensions`].
//!
//! Representação:
//! - `abstract class CoreEditorCommand<T>` é a struct [`CoreEditorCommand<T>`], que guarda o `runCoreEditorCommand`
//!   como fechamento (`Box<dyn Fn(&EditorViewModel, &T)>`). As subclasses do original (`BaseMoveToCommand`,
//!   `ColumnSelectCommand` e as suas, `CursorMoveBasedCommand`, `HomeCommand`...) são funções que criam o
//!   comando com o fechamento certo; o que a subclasse guarda em campo (`_inSelectionMode`, `_isPaged`,
//!   `_staticArgs`) o fechamento captura. O `IViewModel` do `runCoreEditorCommand` é o [`EditorViewModel`]
//!   concreto, porque o trait [`crate::view_model_types::IViewModel`] tem tipos associados e não vira objeto.
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
//!   pelos comandos da parte B) também são DOM.
//! - **Serviços.** `ICodeEditorService` é o [`crate::editor_extensions::ICodeEditorLookup`] (ver
//!   `editor_extensions`). `EditorContextKeys` (`editorContextKeys.ts`) ainda não tem porte; uma `RawContextKey`
//!   é a expressão `has(chave)`, que [`text_input_focus`] monta (sem o registro de metadados da chave).
//! - `TextEditorSelectionSource` (`platform/editor/common/editor.ts`, só os três valores de texto) é o módulo
//!   [`text_editor_selection_source`], porque `cursorMove` com `noHistory` usa o `PROGRAMMATIC`.
//! - `viewModel.model` (o `ITextModel`) é o `Rc<TextModel>` de [`crate::view_model_types::IViewModel::model`].

use std::rc::Rc;

use je_base::dynamic::{Dynamic, FunctionRef, Truthy};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::promise::{MaybePromise, Promise};
use je_base::types::{is_boolean, is_number, is_object, is_string, is_undefined};
use je_platform::commands::{CommandArgument, CommandDescription, CommandMetadata};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings, PlatformKeybindings};

use crate::code_editor_widget::{CodeEditorWidget, EditorViewModel, IEditorCommand, ICodeEditor};
use crate::cursor_column_selection::{self as column_selection, ColumnSelectResult};
use crate::cursor_common::{ColumnSelectData, CursorConfiguration, CursorSimpleModel, CursorState, PartialCursorState};
use crate::cursor_move_commands::{self as move_commands, cursor_move};
use crate::editor_extensions::{
    Command, CommandImplementationResult, CommandKeybindingsOptions, CommandOptions, EditorCommand, I_CODE_EDITOR_LOOKUP,
    MultiCommand, ServicesAccessor, position_from_dynamic, register_editor_command,
};
use crate::position::Position;
use crate::view_model_types::{CursorChangeReason, IViewModel};

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

    /// `LastCursorMoveToSelect.runCoreEditorCommand`.
    fn run_last_cursor_move_to_select(view_model: &EditorViewModel, args: &MoveCommandOptions) {
        let Some(position) = args.position else {
            return;
        };
        let last_added_cursor_index = view_model.get_last_added_cursor_index();

        let states = view_model.get_cursor_states();
        let mut new_states: Vec<PartialCursorState> = states.iter().copied().map(PartialCursorState::Full).collect();
        new_states[last_added_cursor_index] =
            move_commands::move_to(view_model, &states[last_added_cursor_index], true, position, args.view_position);

        view_model.model().push_stack_element();
        view_model.set_cursor_states(args.source(), CursorChangeReason::Explicit, Some(new_states));
    }

    /// Os comandos de navegação registrados (`MoveTo` até `LastCursorMoveToSelect`).
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
        let last_cursor_move_to_select =
            register_core_command(CoreEditorCommand::new(CommandOptions::new("_lastCursorMoveToSelect"), run_last_cursor_move_to_select));

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
        }
    }
}

// #endregion

// A parte B (linhas 1004 a 2178 do coreCommands.ts) acrescenta a este arquivo, depois do módulo acima:
// - dentro de `core_navigation_commands` (e como campos de `CoreNavigationCommands`, com o registro em `register`):
//   o `HomeCommand` (`CursorHome`, `CursorHomeSelect`, `CursorLineStart`, `CursorLineStartSelect`...) e os
//   comandos seguintes na ordem do original: `CursorEnd`, `CursorEndSelect`, `CursorLineEnd`..., `CursorTop`,
//   `CursorTopSelect`, `CursorBottom`, `CursorBottomSelect`, `EditorScrollImpl` e os `EditorScroll*`
//   (`EditorScrollUp`, `EditorScrollDown`, `ScrollLineUp`...), `RevealLine` (com `reveal_line::RawArguments`),
//   `SelectAll`, `SetSelection`, `ExpandLineSelection`, `CancelSelection`, `RemoveSecondaryCursors`,
//   `LineSelect`, `LineSelectDrag`, `LastCursorLineSelect`, `LastCursorLineSelectDrag`, `WordSelect`,
//   `WordSelectDrag`, `LastCursorWordSelect`, `CursorUndo`, `CursorRedo` e os demais até `RemoveSecondaryCursors`;
// - `registerColumnSelection` (a seleção por coluna do mouse);
// - o módulo `core_editing_commands` (`CoreEditingCommands`): `LineBreakInsert`, `Outdent`, `Tab`, `DeleteLeft`,
//   `DeleteRight`, `Undo`, `Redo`, `SelectAll`..., criados com `EditorHandlerCommand` e
//   `registerOverwritableCommand`;
// - `EditorHandlerCommand` (o `Command` que despacha `handlerId` para `editor.trigger`) e
//   `registerOverwritableCommand(handlerId, description?)`, que usam o trait
//   [`EditorOrNativeTextInputCommand`] e [`register_editor_or_native_text_input_command`] acima para
//   `Undo`, `Redo` e `SelectAll` (alvos `undo_command()`, `redo_command()`, `select_all_command()` de
//   `editor_extensions`).
