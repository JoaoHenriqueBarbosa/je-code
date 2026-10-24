//! Porte de `src/vs/workbench/api/common/extHostTextEditor.ts`: o editor de texto que a extensão enxerga
//! (`vscode.TextEditor`), as opções dele (`vscode.TextEditorOptions`), o construtor de edições
//! (`vscode.TextEditorEdit`) e o tipo de decoração (`vscode.TextEditorDecorationType`).
//!
//! Divergências:
//! - O objeto `value` do original (um literal congelado com getters e setters que apontam para o estado
//!   privado) é um tipo próprio: [`TextEditor`] para o editor e o próprio [`ExtHostTextEditorOptions`] para as
//!   opções, ambos com o estado atrás de um `Rc`, como o [`TextDocument`] de `ext_host_document_data`. O que o
//!   original chama de interno (`_acceptOptions`, `dispose`...) fica em [`ExtHostTextEditor`], que só o
//!   `ExtHostDocumentsAndEditors` possui. Os setters que o original rejeita com `ReadonlyError` (`document`,
//!   `visibleRanges`, `viewColumn`) não existem: escrevê-los é erro de compilação. As verificações de tipo dos
//!   setters (`value instanceof Selection`, `Array.isArray(value)`) também não existem: o tipo as garante.
//!   Os setters `selection` e `selections` devolvem a promessa que o original descarta.
//! - Os valores de `vscode.TextEditorOptions` que o original valida em tempo de execução (`tabSize: number |
//!   string`, `indentSize`, `insertSpaces: boolean | string`, que a extensão pode preencher com qualquer valor)
//!   são [`Dynamic`], com a validação do original (`parseInt`, `Math.floor`, `Boolean`). `assign` recebe o
//!   objeto parcial como [`TextEditorOptionsInput`] (`None` é `undefined`).
//! - `ExtHostTextEditorOptions._validateTabSize` e `_validateIndentSize` repetem o ramo de número e de texto; ele
//!   é o `positive_integer` único. O `parseInt(value, 10)` é `parse_int_base10` (local, como os outros portes).
//! - O `callback` de `edit` recebe o [`TextEditorEdit`] num `Rc`, com o estado interno mutável, para que usá-lo
//!   depois do retorno lance `Edit is only valid while callback runs` como no original. Os métodos que lançam
//!   devolvem [`Result`]. `replace`, `insert` e `delete` recebem [`EditLocation`], [`Position`] e
//!   [`RangeOrSelection`], e o `Unrecognized location` do original não existe (o tipo garante a classe); do mesmo
//!   modo o `illegalArgument('endOfLine')` de `setEndOfLine`. Uma `Selection` é um `Range` (herança): o intervalo
//!   dela.
//! - `insertSnippet(snippet, where?)`: `Position | readonly Position[] | Range | readonly Range[]` é
//!   [`SnippetLocation`] (a lista aceita os dois tipos misturados, como o laço do original). O `options` recebe
//!   o `keepWhitespace` ausente como `false`, sem mudar o objeto da extensão.
//! - `setDecorations(type, ranges)` recebe [`DecorationRanges`]; o `isDecorationOptionsArr` é o `match` (ver
//!   `ext_host_type_converters::text_editor`). `revealRange(range, revealType)` recebe o tipo opcional: o `||`
//!   do original (`revealType || Default`) é o `unwrap_or`. `show`, `hide`, `setDecorations` e `revealRange`
//!   não devolvem a promessa (o original devolve `void`).
//! - `_runOnProxy` devolve `ExtHostTextEditor | undefined | null`: aqui é `Option<TextEditor>`, e `undefined`
//!   (editor fechado) e `null` (erro do proxy) são ambos `None`; ninguém no original os distingue.
//! - `ILogService` é o [`LogServiceRef`]; `IExtensionDescription` é o `ExtensionDescription` de `je_extensions`.
//!   O `IdGenerator` estático de `TextEditorDecorationType` é um `thread_local!`.
//! - `Symbol.for('debug.description')` é [`TextEditor::debug_description`].
//! - `dispose()` e `ok(!this._disposed)`: [`ExtHostTextEditor`] implementa [`IDisposable`], e a asserção que
//!   lança é pânico com [`Error`], como no original.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::JsString;
use je_base::assert::ok;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::id_generator::IdGenerator;
use je_base::js_string::is_js_whitespace;
use je_base::lazy::Lazy;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::config::editor_options::cursor_style::TextEditorCursorStyle;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::range::Range as EditorRange;
use je_editor::selection::Selection as EditorSelection;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogMessage, LogServiceRef, Logger};

use super::ext_host_document_data::TextDocument;
use super::ext_host_protocol::{
    ApplyEditsOptions, IndentSizeValue, InsertSpacesValue, MainThreadTextEditorsShape, ResolvedTextEditorConfiguration, SnippetOptions, TabSizeValue,
    TextEditorConfigurationUpdate, UndoStopOptions,
};
use super::ext_host_type_converters as type_converters;
use super::ext_host_types::{
    self as types, DecorationRanges, EndOfLine, Position, Range, RangeOrPosition, Selection, SnippetString, TextEditorDiffInformation,
    TextEditorRevealType,
};

thread_local! {
    static KEYS: IdGenerator = IdGenerator::new("TextEditorDecorationType");
}

/// `vscode.TextEditorDecorationType`: a chave da decoração e o `dispose` que a remove.
#[derive(Clone)]
pub struct DecorationTypeHandle {
    key: String,
    proxy: Rc<dyn MainThreadTextEditorsShape>,
}

impl DecorationTypeHandle {
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl IDisposable for DecorationTypeHandle {
    fn dispose(&self) {
        self.proxy.rpc_remove_text_editor_decoration_type(&self.key);
    }
}

/// `TextEditorDecorationType`.
pub struct TextEditorDecorationType {
    pub value: DecorationTypeHandle,
}

impl TextEditorDecorationType {
    pub fn new(proxy: &Rc<dyn MainThreadTextEditorsShape>, extension: &ExtensionDescription, options: &types::DecorationRenderOptions) -> Self {
        let key = KEYS.with(IdGenerator::next_id);
        proxy.rpc_register_text_editor_decoration_type(&extension.identifier, &key, type_converters::DecorationRenderOptions::from(options));
        Self { value: DecorationTypeHandle { key, proxy: proxy.clone() } }
    }
}

/// `ITextEditOperation`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITextEditOperation {
    pub range: Range,
    pub text: Option<String>,
    pub force_move_markers: bool,
}

/// `IEditData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IEditData {
    pub document_version_id: f64,
    pub edits: Vec<ITextEditOperation>,
    pub set_end_of_line: Option<EndOfLine>,
    pub undo_stop_before: bool,
    pub undo_stop_after: bool,
}

/// `Position | Range | Selection`: o `location` de `TextEditorEdit#replace`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditLocation {
    Position(Position),
    Range(Range),
    Selection(Selection),
}

impl From<Position> for EditLocation {
    fn from(position: Position) -> Self {
        EditLocation::Position(position)
    }
}

impl From<Range> for EditLocation {
    fn from(range: Range) -> Self {
        EditLocation::Range(range)
    }
}

impl From<Selection> for EditLocation {
    fn from(selection: Selection) -> Self {
        EditLocation::Selection(selection)
    }
}

/// `Range | Selection`: o `location` de `TextEditorEdit#delete`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeOrSelection {
    Range(Range),
    Selection(Selection),
}

impl From<Range> for RangeOrSelection {
    fn from(range: Range) -> Self {
        RangeOrSelection::Range(range)
    }
}

impl From<Selection> for RangeOrSelection {
    fn from(selection: Selection) -> Self {
        RangeOrSelection::Selection(selection)
    }
}

struct EditState {
    collected_edits: Vec<ITextEditOperation>,
    set_end_of_line: Option<EndOfLine>,
    finalized: bool,
}

/// `TextEditorEdit`.
pub struct TextEditorEdit {
    document: TextDocument,
    document_version_id: f64,
    undo_stop_before: bool,
    undo_stop_after: bool,
    state: RefCell<EditState>,
}

impl TextEditorEdit {
    pub fn new(document: TextDocument, options: UndoStopOptions) -> Rc<Self> {
        Rc::new(Self {
            document_version_id: document.version(),
            document,
            undo_stop_before: options.undo_stop_before,
            undo_stop_after: options.undo_stop_after,
            state: RefCell::new(EditState { collected_edits: Vec::new(), set_end_of_line: None, finalized: false }),
        })
    }

    pub fn finalize(&self) -> IEditData {
        let mut state = self.state.borrow_mut();
        state.finalized = true;
        IEditData {
            document_version_id: self.document_version_id,
            edits: state.collected_edits.clone(),
            set_end_of_line: state.set_end_of_line,
            undo_stop_before: self.undo_stop_before,
            undo_stop_after: self.undo_stop_after,
        }
    }

    fn throw_if_finalized(&self) -> Result<(), Error> {
        if self.state.borrow().finalized {
            return Err(Error::new("Edit is only valid while callback runs"));
        }
        Ok(())
    }

    pub fn replace(&self, location: impl Into<EditLocation>, value: &str) -> Result<(), Error> {
        self.throw_if_finalized()?;
        let range = match location.into() {
            EditLocation::Position(position) => Range::new(position, position),
            EditLocation::Range(range) => range,
            EditLocation::Selection(selection) => *selection,
        };

        self.push_edit(range, Some(value), false);
        Ok(())
    }

    pub fn insert(&self, location: Position, value: &str) -> Result<(), Error> {
        self.throw_if_finalized()?;
        self.push_edit(Range::new(location, location), Some(value), true);
        Ok(())
    }

    pub fn delete(&self, location: impl Into<RangeOrSelection>) -> Result<(), Error> {
        self.throw_if_finalized()?;
        let range = match location.into() {
            RangeOrSelection::Range(range) => range,
            RangeOrSelection::Selection(selection) => *selection,
        };

        self.push_edit(range, None, true);
        Ok(())
    }

    fn push_edit(&self, range: Range, text: Option<&str>, force_move_markers: bool) {
        let valid_range = self.document.validate_range(&range);
        self.state.borrow_mut().collected_edits.push(ITextEditOperation { range: valid_range, text: text.map(str::to_owned), force_move_markers });
    }

    pub fn set_end_of_line(&self, end_of_line: EndOfLine) -> Result<(), Error> {
        self.throw_if_finalized()?;
        self.state.borrow_mut().set_end_of_line = Some(end_of_line);
        Ok(())
    }
}

/// `typeof value === 'number'` ou `'string'`: `Math.floor(value)` ou `parseInt(value, 10)`, se for positivo.
fn positive_integer(value: &Dynamic) -> Option<f64> {
    let r = match value {
        Dynamic::Number(number) => number.floor(),
        Dynamic::String(text) => parse_int_base10(&text.to_string_lossy()),
        _ => return None,
    };
    // `NaN` não é maior que zero
    (r > 0.0).then_some(r)
}

/// `parseInt(text, 10)`: o espaço em branco do começo, o sinal e os algarismos decimais; `NaN` sem algarismos.
fn parse_int_base10(text: &str) -> f64 {
    let trimmed = text.trim_start_matches(|c: char| u16::try_from(c as u32).is_ok_and(is_js_whitespace));
    let (negative, unsigned) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let digits = unsigned.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("");
    if digits.is_empty() {
        return f64::NAN;
    }
    let value = digits.bytes().fold(0.0_f64, |accumulated, digit| accumulated * 10.0 + f64::from(digit - b'0'));
    if negative { -value } else { value }
}

/// `value === 'auto'`.
fn is_text(value: &Dynamic, text: &str) -> bool {
    matches!(value, Dynamic::String(candidate) if candidate == text)
}

/// `_validateTabSize`: `None` é o `null` do original (chamada inválida).
fn validate_tab_size(value: &Dynamic) -> Option<TabSizeValue> {
    if is_text(value, "auto") {
        return Some(TabSizeValue::Auto);
    }
    positive_integer(value).map(TabSizeValue::Size)
}

/// `_validateIndentSize`: `None` é o `null` do original (chamada inválida).
fn validate_indent_size(value: &Dynamic) -> Option<IndentSizeValue> {
    if is_text(value, "tabSize") {
        return Some(IndentSizeValue::TabSize);
    }
    positive_integer(value).map(IndentSizeValue::Size)
}

/// `_validateInsertSpaces`.
fn validate_insert_spaces(value: &Dynamic) -> InsertSpacesValue {
    if is_text(value, "auto") {
        return InsertSpacesValue::Auto;
    }
    InsertSpacesValue::Bool(if is_text(value, "false") { false } else { value.is_truthy() })
}

/// O valor presente (`typeof value !== 'undefined'`) de um campo opcional.
fn defined(value: &Option<Dynamic>) -> Option<&Dynamic> {
    value.as_ref().filter(|value| !value.is_undefined())
}

/// O argumento de `ExtHostTextEditorOptions#assign` e de `TextEditor#options = ...`: o `vscode.TextEditorOptions`
/// parcial (cada campo ausente é `undefined`).
#[derive(Clone, Default)]
pub struct TextEditorOptionsInput {
    pub tab_size: Option<Dynamic>,
    pub indent_size: Option<Dynamic>,
    pub insert_spaces: Option<Dynamic>,
    pub cursor_style: Option<TextEditorCursorStyle>,
    pub line_numbers: Option<types::TextEditorLineNumbersStyle>,
}

/// Os campos de `ExtHostTextEditorOptions` que o `_accept` troca de uma vez e os setters trocam um a um.
#[derive(Clone, Copy)]
struct OptionsState {
    tab_size: f64,
    indent_size: f64,
    original_indent_size: IndentSizeValue,
    insert_spaces: bool,
    cursor_style: TextEditorCursorStyle,
    line_numbers: types::TextEditorLineNumbersStyle,
}

impl OptionsState {
    fn from_source(source: &ResolvedTextEditorConfiguration) -> Self {
        Self {
            tab_size: source.tab_size,
            indent_size: source.indent_size,
            original_indent_size: source.original_indent_size,
            insert_spaces: source.insert_spaces,
            cursor_style: source.cursor_style,
            line_numbers: type_converters::TextEditorLineNumbersStyle::to(source.line_numbers),
        }
    }
}

/// `ExtHostTextEditorOptions`. O próprio objeto é o `value` (`vscode.TextEditorOptions`): os getters são os
/// métodos de leitura e os setters, os `set_*`.
pub struct ExtHostTextEditorOptions {
    proxy: Rc<dyn MainThreadTextEditorsShape>,
    id: String,
    log_service: LogServiceRef,
    state: Cell<OptionsState>,
}

impl ExtHostTextEditorOptions {
    pub fn new(proxy: Rc<dyn MainThreadTextEditorsShape>, id: &str, source: &ResolvedTextEditorConfiguration, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { proxy, id: id.to_owned(), log_service, state: Cell::new(OptionsState::from_source(source)) })
    }

    pub fn accept(&self, source: &ResolvedTextEditorConfiguration) {
        self.state.set(OptionsState::from_source(source));
    }

    /// Troca os campos de `state` que `change` mexe.
    fn update(&self, change: impl FnOnce(&mut OptionsState)) {
        let mut state = self.state.get();
        change(&mut state);
        self.state.set(state);
    }

    // --- getters de `value`

    pub fn tab_size(&self) -> f64 {
        self.state.get().tab_size
    }

    pub fn indent_size(&self) -> f64 {
        self.state.get().indent_size
    }

    pub fn insert_spaces(&self) -> bool {
        self.state.get().insert_spaces
    }

    pub fn cursor_style(&self) -> TextEditorCursorStyle {
        self.state.get().cursor_style
    }

    pub fn line_numbers(&self) -> types::TextEditorLineNumbersStyle {
        self.state.get().line_numbers
    }

    // --- setters de `value`

    pub fn set_tab_size(&self, value: &Dynamic) {
        let Some(tab_size) = validate_tab_size(value) else {
            // ignore invalid call
            return;
        };
        if let TabSizeValue::Size(size) = tab_size {
            if self.tab_size() == size {
                // nothing to do
                return;
            }
            // reflect the new tabSize value immediately
            self.update(|state| state.tab_size = size);
        }
        self.warn_on_error("setTabSize", self.proxy.rpc_try_set_options(&self.id, TextEditorConfigurationUpdate { tab_size: Some(tab_size), ..Default::default() }));
    }

    pub fn set_indent_size(&self, value: &Dynamic) {
        let Some(indent_size) = validate_indent_size(value) else {
            // ignore invalid call
            return;
        };
        if let IndentSizeValue::Size(size) = indent_size {
            if self.state.get().original_indent_size == indent_size {
                // nothing to do
                return;
            }
            // reflect the new indentSize value immediately
            self.update(|state| {
                state.indent_size = size;
                state.original_indent_size = indent_size;
            });
        }
        self.warn_on_error(
            "setIndentSize",
            self.proxy.rpc_try_set_options(&self.id, TextEditorConfigurationUpdate { indent_size: Some(indent_size), ..Default::default() }),
        );
    }

    pub fn set_insert_spaces(&self, value: &Dynamic) {
        let insert_spaces = validate_insert_spaces(value);
        if let InsertSpacesValue::Bool(flag) = insert_spaces {
            if self.insert_spaces() == flag {
                // nothing to do
                return;
            }
            // reflect the new insertSpaces value immediately
            self.update(|state| state.insert_spaces = flag);
        }
        self.warn_on_error(
            "setInsertSpaces",
            self.proxy.rpc_try_set_options(&self.id, TextEditorConfigurationUpdate { insert_spaces: Some(insert_spaces), ..Default::default() }),
        );
    }

    pub fn set_cursor_style(&self, value: TextEditorCursorStyle) {
        if self.cursor_style() == value {
            // nothing to do
            return;
        }
        self.update(|state| state.cursor_style = value);
        self.warn_on_error(
            "setCursorStyle",
            self.proxy.rpc_try_set_options(&self.id, TextEditorConfigurationUpdate { cursor_style: Some(value), ..Default::default() }),
        );
    }

    pub fn set_line_numbers(&self, value: types::TextEditorLineNumbersStyle) {
        if self.line_numbers() == value {
            // nothing to do
            return;
        }
        self.update(|state| state.line_numbers = value);
        self.warn_on_error(
            "setLineNumbers",
            self.proxy.rpc_try_set_options(
                &self.id,
                TextEditorConfigurationUpdate { line_numbers: Some(type_converters::TextEditorLineNumbersStyle::from(value)), ..Default::default() },
            ),
        );
    }

    pub fn assign(&self, new_options: &TextEditorOptionsInput) {
        let mut bulk_configuration_update = TextEditorConfigurationUpdate::default();
        let mut has_update = false;

        if let Some(value) = defined(&new_options.tab_size) {
            match validate_tab_size(value) {
                Some(TabSizeValue::Auto) => {
                    has_update = true;
                    bulk_configuration_update.tab_size = Some(TabSizeValue::Auto);
                }
                Some(TabSizeValue::Size(size)) if self.tab_size() != size => {
                    // reflect the new tabSize value immediately
                    self.update(|state| state.tab_size = size);
                    has_update = true;
                    bulk_configuration_update.tab_size = Some(TabSizeValue::Size(size));
                }
                _ => {}
            }
        }

        if let Some(value) = defined(&new_options.indent_size) {
            match validate_indent_size(value) {
                Some(IndentSizeValue::TabSize) => {
                    has_update = true;
                    bulk_configuration_update.indent_size = Some(IndentSizeValue::TabSize);
                }
                Some(indent_size @ IndentSizeValue::Size(size)) if self.state.get().original_indent_size != indent_size => {
                    // reflect the new indentSize value immediately
                    self.update(|state| {
                        state.indent_size = size;
                        state.original_indent_size = indent_size;
                    });
                    has_update = true;
                    bulk_configuration_update.indent_size = Some(indent_size);
                }
                _ => {}
            }
        }

        if let Some(value) = defined(&new_options.insert_spaces) {
            match validate_insert_spaces(value) {
                InsertSpacesValue::Auto => {
                    has_update = true;
                    bulk_configuration_update.insert_spaces = Some(InsertSpacesValue::Auto);
                }
                InsertSpacesValue::Bool(flag) if self.insert_spaces() != flag => {
                    // reflect the new insertSpaces value immediately
                    self.update(|state| state.insert_spaces = flag);
                    has_update = true;
                    bulk_configuration_update.insert_spaces = Some(InsertSpacesValue::Bool(flag));
                }
                InsertSpacesValue::Bool(_) => {}
            }
        }

        if let Some(cursor_style) = new_options.cursor_style
            && self.cursor_style() != cursor_style
        {
            self.update(|state| state.cursor_style = cursor_style);
            has_update = true;
            bulk_configuration_update.cursor_style = Some(cursor_style);
        }

        if let Some(line_numbers) = new_options.line_numbers
            && self.line_numbers() != line_numbers
        {
            self.update(|state| state.line_numbers = line_numbers);
            has_update = true;
            bulk_configuration_update.line_numbers = Some(type_converters::TextEditorLineNumbersStyle::from(line_numbers));
        }

        if has_update {
            self.warn_on_error("setOptions", self.proxy.rpc_try_set_options(&self.id, bulk_configuration_update));
        }
    }

    fn warn_on_error(&self, action: &str, promise: Promise<()>) {
        let log_service = self.log_service.clone();
        let action = action.to_owned();
        promise.catch(move |err| {
            log_service.warn(&format!("ExtHostTextEditorOptions '{action}' failed:'"));
            log_service.warn_args(&LogMessage::from(&err), &[]);
        });
    }
}

/// O estado de um editor, dividido entre o [`ExtHostTextEditor`] (que o atualiza) e o [`TextEditor`] (que a
/// extensão lê e escreve).
struct EditorState {
    id: String,
    proxy: Rc<dyn MainThreadTextEditorsShape>,
    log_service: LogServiceRef,
    document: Lazy<TextDocument>,
    selections: RefCell<Vec<Selection>>,
    options: Rc<ExtHostTextEditorOptions>,
    visible_ranges: RefCell<Vec<Range>>,
    view_column: Cell<Option<f64>>,
    disposed: Cell<bool>,
    has_decorations_for_key: RefCell<HashSet<String>>,
    diff_information: RefCell<Option<Vec<TextEditorDiffInformation>>>,
}

/// `vscode.TextEditor`.
#[derive(Clone)]
pub struct TextEditor(Rc<EditorState>);

/// `ExtHostTextEditor`.
pub struct ExtHostTextEditor {
    state: Rc<EditorState>,
}

impl ExtHostTextEditor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        proxy: Rc<dyn MainThreadTextEditorsShape>,
        log_service: LogServiceRef,
        document: Lazy<TextDocument>,
        selections: Vec<Selection>,
        options: &ResolvedTextEditorConfiguration,
        visible_ranges: Vec<Range>,
        view_column: Option<f64>,
    ) -> Self {
        let options = ExtHostTextEditorOptions::new(proxy.clone(), id, options, log_service.clone());
        Self {
            state: Rc::new(EditorState {
                id: id.to_owned(),
                proxy,
                log_service,
                document,
                selections: RefCell::new(selections),
                options,
                visible_ranges: RefCell::new(visible_ranges),
                view_column: Cell::new(view_column),
                disposed: Cell::new(false),
                has_decorations_for_key: RefCell::new(HashSet::new()),
                diff_information: RefCell::new(None),
            }),
        }
    }

    pub fn id(&self) -> &str {
        &self.state.id
    }

    /// `value`: o `vscode.TextEditor` que a extensão enxerga.
    pub fn value(&self) -> TextEditor {
        TextEditor(self.state.clone())
    }

    // --- incoming: extension host MUST accept what the renderer says

    pub fn accept_options(&self, options: &ResolvedTextEditorConfiguration) {
        ok(&!self.state.disposed.get(), None);
        self.state.options.accept(options);
    }

    pub fn accept_visible_ranges(&self, value: Vec<Range>) {
        ok(&!self.state.disposed.get(), None);
        *self.state.visible_ranges.borrow_mut() = value;
    }

    pub fn accept_view_column(&self, value: f64) {
        ok(&!self.state.disposed.get(), None);
        self.state.view_column.set(Some(value));
    }

    pub fn accept_selections(&self, selections: Vec<Selection>) {
        ok(&!self.state.disposed.get(), None);
        *self.state.selections.borrow_mut() = selections;
    }

    pub fn accept_diff_information(&self, diff_information: Option<Vec<TextEditorDiffInformation>>) {
        ok(&!self.state.disposed.get(), None);
        *self.state.diff_information.borrow_mut() = diff_information;
    }
}

impl IDisposable for ExtHostTextEditor {
    fn dispose(&self) {
        ok(&!self.state.disposed.get(), None);
        self.state.disposed.set(true);
    }
}

/// O intervalo vazio na posição, no formato do editor (`{ startLineNumber: lineNumber, startColumn: column, ... }`).
fn collapsed_range(position: &Position) -> EditorRange {
    let position = type_converters::Position::from(position);
    EditorRange {
        start_line_number: position.line_number,
        start_column: position.column,
        end_line_number: position.line_number,
        end_column: position.column,
    }
}

/// O destino de `TextEditor#insertSnippet`: `Position | readonly Position[] | Range | readonly Range[]`.
#[derive(Clone, Debug, PartialEq)]
pub enum SnippetLocation {
    Position(Position),
    Range(Range),
    Many(Vec<RangeOrPosition>),
}

/// O comparador do `sort` de `_applyEdit`: ordem crescente pelo fim e depois pelo início.
fn compare_ranges(a: &Range, b: &Range) -> Ordering {
    let key = |range: &Range| [range.end().line(), range.end().character(), range.start().line(), range.start().character()];
    key(a)
        .iter()
        .zip(key(b).iter())
        .map(|(x, y)| x.partial_cmp(y).unwrap_or(Ordering::Equal))
        .find(|ordering| ordering.is_ne())
        .unwrap_or(Ordering::Equal)
}

impl EditorState {
    fn try_set_selection(self: &Rc<Self>) -> Promise<TextEditor> {
        let this = self.clone();
        async_fn(async move {
            let selection: Vec<EditorSelection> = this.selections.borrow().iter().map(type_converters::Selection::from).collect();
            this.run_on_proxy(|| this.proxy.rpc_try_set_selections(&this.id, selection)).await?;
            Ok(TextEditor(this))
        })
    }

    fn apply_edit(&self, edit_builder: &TextEditorEdit) -> Promise<bool> {
        let edit_data = edit_builder.finalize();

        // return when there is nothing to do
        if edit_data.edits.is_empty() && edit_data.set_end_of_line.is_none() {
            return Promise::resolved(true);
        }

        // check that the edits are not overlapping (i.e. illegal)
        let mut edit_ranges: Vec<Range> = edit_data.edits.iter().map(|edit| edit.range).collect();

        // sort ascending (by end and then by start)
        edit_ranges.sort_by(compare_ranges);

        // check that no edits are overlapping
        for pair in edit_ranges.windows(2) {
            let range_end = pair[0].end();
            let next_range_start = pair[1].start();

            if next_range_start.is_before(&range_end) {
                // overlapping ranges
                return Promise::rejected(Error::new("Overlapping ranges are not allowed!"));
            }
        }

        // prepare data for serialization
        let edits: Vec<SingleEditOperation> = edit_data
            .edits
            .iter()
            .map(|edit| SingleEditOperation::new(type_converters::Range::from(&edit.range), edit.text.as_deref().map(JsString::from), edit.force_move_markers))
            .collect();

        self.proxy.rpc_try_apply_edits(
            &self.id,
            edit_data.document_version_id,
            edits,
            ApplyEditsOptions {
                set_end_of_line: edit_data.set_end_of_line.map(type_converters::EndOfLine::from),
                undo_stop_before: edit_data.undo_stop_before,
                undo_stop_after: edit_data.undo_stop_after,
            },
        )
    }

    fn run_on_proxy(self: &Rc<Self>, callback: impl FnOnce() -> Promise<()>) -> Promise<Option<TextEditor>> {
        if self.disposed.get() {
            self.log_service.warn("TextEditor is closed/disposed");
            return Promise::resolved(None);
        }

        let this = self.clone();
        callback().then_result(move |result| match result {
            Ok(()) => Ok(Some(TextEditor(this))),
            Err(err) => {
                if err.name != "DISPOSED" {
                    this.log_service.warn_args(&LogMessage::from(&err), &[]);
                }
                Ok(None)
            }
        })
    }
}

impl TextEditor {
    /// O identificador do editor (`ExtHostTextEditor#id`). O original compara editores pela identidade do objeto; quem não pode
    /// guardar o objeto (a ponte do host de extensões para o motor JavaScript) os identifica por ele.
    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub fn document(&self) -> TextDocument {
        self.0.document.value()
    }

    // --- selection

    /// `selection`: `that._selections && that._selections[0]`, ausente num editor sem seleções.
    pub fn selection(&self) -> Option<Selection> {
        self.0.selections.borrow().first().copied()
    }

    pub fn set_selection(&self, value: Selection) -> Promise<TextEditor> {
        *self.0.selections.borrow_mut() = vec![value];
        self.0.try_set_selection()
    }

    pub fn selections(&self) -> Vec<Selection> {
        self.0.selections.borrow().clone()
    }

    pub fn set_selections(&self, value: Vec<Selection>) -> Promise<TextEditor> {
        let value = if value.is_empty() {
            vec![Selection::from_coordinates(0.0, 0.0, 0.0, 0.0).expect("(0, 0, 0, 0) é uma seleção válida")]
        } else {
            value
        };
        *self.0.selections.borrow_mut() = value;
        self.0.try_set_selection()
    }

    // --- visible ranges

    pub fn visible_ranges(&self) -> Vec<Range> {
        self.0.visible_ranges.borrow().clone()
    }

    pub fn diff_information(&self) -> Option<Vec<TextEditorDiffInformation>> {
        self.0.diff_information.borrow().clone()
    }

    // --- options

    pub fn options(&self) -> Rc<ExtHostTextEditorOptions> {
        self.0.options.clone()
    }

    pub fn set_options(&self, value: &TextEditorOptionsInput) {
        if !self.0.disposed.get() {
            self.0.options.assign(value);
        }
    }

    // --- view column

    pub fn view_column(&self) -> Option<f64> {
        self.0.view_column.get()
    }

    // --- edit

    pub fn edit(&self, callback: impl FnOnce(Rc<TextEditorEdit>), options: Option<UndoStopOptions>) -> Promise<bool> {
        if self.0.disposed.get() {
            return Promise::rejected(Error::new("TextEditor#edit not possible on closed editors"));
        }
        let edit = TextEditorEdit::new(self.document(), options.unwrap_or_default());
        callback(edit.clone());
        self.0.apply_edit(&edit)
    }

    // --- snippet edit

    pub fn insert_snippet(&self, snippet: &SnippetString, where_: Option<SnippetLocation>, options: Option<SnippetOptions>) -> Promise<bool> {
        if self.0.disposed.get() {
            return Promise::rejected(Error::new("TextEditor#insertSnippet not possible on closed editors"));
        }

        let ranges: Vec<EditorRange> = match where_ {
            None => self.0.selections.borrow().iter().map(|range| type_converters::Range::from(range)).collect(),
            Some(SnippetLocation::Many(locations)) if locations.is_empty() => {
                self.0.selections.borrow().iter().map(|range| type_converters::Range::from(range)).collect()
            }
            Some(SnippetLocation::Position(position)) => vec![collapsed_range(&position)],
            Some(SnippetLocation::Range(range)) => vec![type_converters::Range::from(&range)],
            Some(SnippetLocation::Many(locations)) => locations
                .iter()
                .map(|location| match location {
                    RangeOrPosition::Range(range) => type_converters::Range::from(range),
                    RangeOrPosition::Position(position) => collapsed_range(position),
                })
                .collect(),
        };
        let mut options = options.unwrap_or_default();
        if options.keep_whitespace.is_none() {
            options.keep_whitespace = Some(false);
        }
        self.0.proxy.rpc_try_insert_snippet(&self.0.id, self.document().version(), &snippet.value, &ranges, options)
    }

    pub fn set_decorations(&self, decoration_type: &DecorationTypeHandle, ranges: &DecorationRanges) {
        let will_be_empty = ranges.is_empty();
        {
            let mut has_decorations_for_key = self.0.has_decorations_for_key.borrow_mut();
            if will_be_empty && !has_decorations_for_key.contains(decoration_type.key()) {
                // avoid no-op call to the renderer
                return;
            }
            if will_be_empty {
                has_decorations_for_key.remove(decoration_type.key());
            } else {
                has_decorations_for_key.insert(decoration_type.key().to_owned());
            }
        }
        let _ = self.0.run_on_proxy(|| match ranges {
            DecorationRanges::Ranges(items) if !items.is_empty() => {
                let flat_ranges: Vec<f64> = items
                    .iter()
                    .flat_map(|range| [range.start().line() + 1.0, range.start().character() + 1.0, range.end().line() + 1.0, range.end().character() + 1.0])
                    .collect();
                self.0.proxy.rpc_try_set_decorations_fast(&self.0.id, decoration_type.key(), flat_ranges)
            }
            _ => self.0.proxy.rpc_try_set_decorations(&self.0.id, decoration_type.key(), type_converters::from_range_or_range_with_message(ranges)),
        });
    }

    pub fn reveal_range(&self, range: &Range, reveal_type: Option<TextEditorRevealType>) {
        let _ = self.0.run_on_proxy(|| {
            self.0.proxy.rpc_try_reveal_range(&self.0.id, type_converters::Range::from(range), reveal_type.unwrap_or(TextEditorRevealType::Default))
        });
    }

    pub fn show(&self, column: Option<f64>) {
        let _ = self.0.proxy.rpc_try_show_editor(&self.0.id, type_converters::ViewColumn::from(column));
    }

    pub fn hide(&self) {
        let _ = self.0.proxy.rpc_try_hide_editor(&self.0.id);
    }

    /// `[Symbol.for('debug.description')]()`.
    pub fn debug_description(&self) -> String {
        format!("TextEditor({})", self.document().uri().to_string(false))
    }
}
