//! Porte de `src/vs/editor/common/cursorCommon.ts`: a base da pilha do cursor (configuração do cursor,
//! estados do cursor no modelo e na visão, resultado de uma operação de edição).
//!
//! Decisões de tradução:
//!
//! - `ICursorSimpleModel` vira o trait [`CursorSimpleModel`] (sem o `I`, como `Command` em
//!   `editor_common.rs`), e `ICommand` é o [`Command`] de `editor_common.rs`.
//! - `IEditorConfiguration` (`config/editorConfiguration.ts`) tem `MenuId`, `IDimension`, `IEditorOptions` e
//!   eventos, que não existem no porte. O trait [`EditorConfiguration`] traz só o membro que o cursor lê,
//!   `options` (o `IComputedEditorOptions`).
//! - `ISelection` e `Selection` são o mesmo tipo (ver `editor_common.rs`), então `Selection.liftSelection` é a
//!   identidade e `fromModelSelection` recebe o [`Selection`].
//! - Os campos `_xxxBrand: void` do original (marcas de tipagem nominal) não existem.
//! - Textos de linha e caracteres são UTF-16: `ch: string` é `&[u16]` (uma unidade, ou vazio) e o conteúdo de
//!   uma linha é [`JsString`]. As estratégias de opção (`EditorAutoClosingStrategy`, `EditorAutoClosingEditStrategy`,
//!   `EditorAutoSurroundStrategy`, `'spread' | 'full'`) ficam como `String`, o tipo que o registro de opções
//!   guarda para elas. `number` de medidas (altura de linha, tamanho de página, limite de multi-cursor) é `f64`.
//! - `electricCharacter` de `ResolvedLanguageConfiguration` nunca é nulo no porte (é criado na primeira
//!   leitura), então os ramos `?.`/`!electricCharacterSupport` do original não têm equivalente.
//! - `InputMode` (singleton de `inputMode.ts`) está em `input_mode.rs`.
//! - `EditOperationResult.followUp` é um `Box<dyn Fn>`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::JsString;

use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::empty_selection_clipboard::EMPTY_SELECTION_CLIPBOARD;
use crate::config::editor_options::layout_info::LAYOUT_INFO;
use crate::config::editor_options::registry::{
    AUTO_CLOSING_BRACKETS, AUTO_CLOSING_COMMENTS, AUTO_CLOSING_DELETE, AUTO_CLOSING_OVERTYPE, AUTO_CLOSING_QUOTES,
    AUTO_INDENT, AUTO_SURROUND, COPY_WITH_SYNTAX_HIGHLIGHTING, MULTI_CURSOR_LIMIT, MULTI_CURSOR_MERGE_OVERLAPPING,
    MULTI_CURSOR_PASTE, OVERTYPE_ON_PASTE, READ_ONLY, STICKY_TAB_STOPS, TRIM_WHITESPACE_ON_DELETE, USE_TAB_STOPS,
    WORD_SEPARATORS,
};
use crate::config::editor_options::word_segmenter_locales::WORD_SEGMENTER_LOCALES;
use crate::config::editor_options::{
    ComputedEditorOptions, ConfigurationChangedEvent, EditorAutoIndentStrategy, EditorOption, InputMode,
};
use crate::cursor_columns;
use crate::editor_common::Command;
use crate::electric_character::ElectricAction;
use crate::indentation::normalize_indentation;
use crate::input_mode;
use crate::language_configuration::AutoClosingPairs;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::line_tokens::LineTokens;
use crate::model::{PositionAffinity, TextModelResolvedOptions};
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::supports::create_scoped_line_tokens;

/// `IColumnSelectData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnSelectData {
    pub is_real: bool,
    pub from_view_line_number: i32,
    pub from_view_visual_column: i32,
    pub to_view_line_number: i32,
    pub to_view_visual_column: i32,
}

/// `EditOperationType`: um tipo de operação gravado para desfazer e refazer. A meta é abrir uma parada de
/// desfazer quando o controlador troca de um tipo de operação para outro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOperationType {
    Other = 0,
    DeletingLeft = 2,
    DeletingRight = 3,
    TypingOther = 4,
    TypingFirstSpace = 5,
    TypingConsecutiveSpace = 6,
}

/// `CharacterMap`: `{ [char: string]: string }`.
pub type CharacterMap = HashMap<JsString, JsString>;

/// Uma das funções `(ch: string) => boolean` de `shouldAutoCloseBefore`.
pub type ShouldAutoClose = Rc<dyn Fn(&[u16]) -> bool>;

/// `shouldAutoCloseBefore`: `{ quote, bracket, comment }`.
#[derive(Clone)]
pub struct ShouldAutoCloseBefore {
    pub quote: ShouldAutoClose,
    pub bracket: ShouldAutoClose,
    pub comment: ShouldAutoClose,
}

/// `IEditorConfiguration`, só o que o cursor lê: as opções calculadas (ver a nota no topo do módulo).
pub trait EditorConfiguration {
    /// `options`.
    fn options(&self) -> &ComputedEditorOptions;
}

/// `autoCloseBeforeWhitespace`.
fn auto_close_before_whitespace(chr: &[u16]) -> bool {
    chr == [u16::from(b' ')] || chr == [u16::from(b'\t')]
}

/// `autoCloseBeforeSet.indexOf(c) !== -1`: a busca de `indexOf` sobre unidades UTF-16 (o texto vazio é
/// achado na posição 0).
fn index_of_found(set: &[u16], c: &[u16]) -> bool {
    c.is_empty() || set.windows(c.len()).any(|window| window == c)
}

/// `Math.max(a, b)` do JavaScript: `NaN` se algum dos dois é `NaN`.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `_getLanguageDefinedShouldAutoClose`.
fn get_language_defined_should_auto_close(
    service: &Rc<dyn ILanguageConfigurationService>,
    language_id: &str,
    for_quotes: bool,
) -> ShouldAutoClose {
    let auto_close_before_set: Vec<u16> = service
        .get_language_configuration(language_id)
        .get_auto_close_before_set(for_quotes)
        .encode_utf16()
        .collect();
    Rc::new(move |c: &[u16]| index_of_found(&auto_close_before_set, c))
}

/// `_getShouldAutoClose`. O original não tem `default` no `switch`: a opção já chega validada, então um
/// texto fora das quatro estratégias é erro de programação.
fn get_should_auto_close(
    service: &Rc<dyn ILanguageConfigurationService>,
    language_id: &str,
    auto_close_config: &str,
    for_quotes: bool,
) -> ShouldAutoClose {
    match auto_close_config {
        "beforeWhitespace" => Rc::new(auto_close_before_whitespace),
        "languageDefined" => get_language_defined_should_auto_close(service, language_id, for_quotes),
        "always" => Rc::new(|_: &[u16]| true),
        "never" => Rc::new(|_: &[u16]| false),
        other => panic!("estratégia de fechamento automático inválida: {other}"),
    }
}

/// `CursorConfiguration`.
pub struct CursorConfiguration {
    pub language_configuration_service: Rc<dyn ILanguageConfigurationService>,

    pub read_only: bool,
    pub tab_size: i32,
    pub indent_size: i32,
    pub insert_spaces: bool,
    pub sticky_tab_stops: bool,
    pub page_size: f64,
    pub line_height: f64,
    pub typical_halfwidth_character_width: f64,
    pub use_tab_stops: bool,
    pub trim_whitespace_on_delete: bool,
    pub word_separators: String,
    pub empty_selection_clipboard: bool,
    pub copy_with_syntax_highlighting: bool,
    pub multi_cursor_merge_overlapping: bool,
    /// `'spread' | 'full'`.
    pub multi_cursor_paste: String,
    pub multi_cursor_limit: f64,
    /// `EditorAutoClosingStrategy`.
    pub auto_closing_brackets: String,
    /// `EditorAutoClosingStrategy`.
    pub auto_closing_comments: String,
    /// `EditorAutoClosingStrategy`.
    pub auto_closing_quotes: String,
    /// `EditorAutoClosingEditStrategy`.
    pub auto_closing_delete: String,
    /// `EditorAutoClosingEditStrategy`.
    pub auto_closing_overtype: String,
    /// `EditorAutoSurroundStrategy`.
    pub auto_surround: String,
    pub auto_indent: EditorAutoIndentStrategy,
    pub auto_closing_pairs: AutoClosingPairs,
    pub surrounding_pairs: CharacterMap,
    pub block_comment_start_token: Option<String>,
    pub should_auto_close_before: ShouldAutoCloseBefore,
    pub word_segmenter_locales: Vec<String>,
    pub overtype_on_paste: bool,

    language_id: String,
    electric_chars: RefCell<Option<Rc<HashSet<JsString>>>>,
}

impl CursorConfiguration {
    /// `shouldRecreate`.
    pub fn should_recreate(e: &ConfigurationChangedEvent) -> bool {
        [
            EditorOption::LayoutInfo,
            EditorOption::WordSeparators,
            EditorOption::EmptySelectionClipboard,
            EditorOption::MultiCursorMergeOverlapping,
            EditorOption::MultiCursorPaste,
            EditorOption::MultiCursorLimit,
            EditorOption::AutoClosingBrackets,
            EditorOption::AutoClosingComments,
            EditorOption::AutoClosingQuotes,
            EditorOption::AutoClosingDelete,
            EditorOption::AutoClosingOvertype,
            EditorOption::AutoSurround,
            EditorOption::UseTabStops,
            EditorOption::TrimWhitespaceOnDelete,
            EditorOption::FontInfo,
            EditorOption::ReadOnly,
            EditorOption::WordSegmenterLocales,
            EditorOption::OvertypeOnPaste,
        ]
        .into_iter()
        .any(|id| e.has_changed(id))
    }

    pub fn new(
        language_id: &str,
        model_options: &TextModelResolvedOptions,
        configuration: &dyn EditorConfiguration,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Self {
        let options = configuration.options();
        let layout_info = options.get(LAYOUT_INFO);
        let font_info = options.get(FONT_INFO);

        let line_height = font_info.line_height;
        let auto_closing_brackets = options.get(AUTO_CLOSING_BRACKETS).clone();
        let auto_closing_comments = options.get(AUTO_CLOSING_COMMENTS).clone();
        let auto_closing_quotes = options.get(AUTO_CLOSING_QUOTES).clone();

        let should_auto_close_before = ShouldAutoCloseBefore {
            quote: get_should_auto_close(&language_configuration_service, language_id, &auto_closing_quotes, true),
            comment: get_should_auto_close(&language_configuration_service, language_id, &auto_closing_comments, false),
            bracket: get_should_auto_close(&language_configuration_service, language_id, &auto_closing_brackets, false),
        };

        let language_configuration = language_configuration_service.get_language_configuration(language_id);
        let auto_closing_pairs = language_configuration.get_auto_closing_pairs();

        let mut surrounding_pairs = CharacterMap::new();
        for pair in language_configuration.get_surrounding_pairs() {
            surrounding_pairs.insert(
                JsString::from_units(pair.open.encode_utf16().collect()),
                JsString::from_units(pair.close.encode_utf16().collect()),
            );
        }

        let block_comment_start_token = language_configuration
            .comments
            .as_ref()
            .and_then(|comments| comments.block_comment_start_token.clone());

        Self {
            language_configuration_service,

            read_only: *options.get(READ_ONLY),
            tab_size: model_options.tab_size,
            indent_size: model_options.indent_size,
            insert_spaces: model_options.insert_spaces,
            sticky_tab_stops: *options.get(STICKY_TAB_STOPS),
            page_size: js_max(1.0, (layout_info.height / line_height).floor() - 2.0),
            line_height,
            typical_halfwidth_character_width: font_info.typical_halfwidth_character_width,
            use_tab_stops: *options.get(USE_TAB_STOPS),
            trim_whitespace_on_delete: *options.get(TRIM_WHITESPACE_ON_DELETE),
            word_separators: options.get(WORD_SEPARATORS).clone(),
            empty_selection_clipboard: *options.get(EMPTY_SELECTION_CLIPBOARD),
            copy_with_syntax_highlighting: *options.get(COPY_WITH_SYNTAX_HIGHLIGHTING),
            multi_cursor_merge_overlapping: *options.get(MULTI_CURSOR_MERGE_OVERLAPPING),
            multi_cursor_paste: options.get(MULTI_CURSOR_PASTE).clone(),
            multi_cursor_limit: *options.get(MULTI_CURSOR_LIMIT),
            auto_closing_brackets,
            auto_closing_comments,
            auto_closing_quotes,
            auto_closing_delete: options.get(AUTO_CLOSING_DELETE).clone(),
            auto_closing_overtype: options.get(AUTO_CLOSING_OVERTYPE).clone(),
            auto_surround: options.get(AUTO_SURROUND).clone(),
            auto_indent: *options.get(AUTO_INDENT),
            auto_closing_pairs,
            surrounding_pairs,
            block_comment_start_token,
            should_auto_close_before,
            word_segmenter_locales: options.get(WORD_SEGMENTER_LOCALES).clone(),
            overtype_on_paste: *options.get(OVERTYPE_ON_PASTE),

            language_id: language_id.to_owned(),
            electric_chars: RefCell::new(None),
        }
    }

    /// `electricChars`: os caracteres elétricos da linguagem, calculados na primeira leitura.
    pub fn electric_chars(&self) -> Rc<HashSet<JsString>> {
        if let Some(electric_chars) = self.electric_chars.borrow().as_ref() {
            return electric_chars.clone();
        }
        let electric_chars: HashSet<JsString> = self
            .language_configuration_service
            .get_language_configuration(&self.language_id)
            .electric_character()
            .get_electric_characters()
            .into_iter()
            .collect();
        let electric_chars = Rc::new(electric_chars);
        *self.electric_chars.borrow_mut() = Some(electric_chars.clone());
        electric_chars
    }

    /// `inputMode`.
    pub fn input_mode(&self) -> InputMode {
        input_mode::get_input_mode()
    }

    /// `onElectricCharacter`: devolve o tipo de colchete de abertura com o qual casar a indentação.
    pub fn on_electric_character(
        &self,
        character: &[u16],
        context: &Rc<LineTokens>,
        column: i32,
    ) -> Option<ElectricAction> {
        let scoped_line_tokens = create_scoped_line_tokens(context.clone(), column - 1);
        let electric_character_support = self
            .language_configuration_service
            .get_language_configuration(&scoped_line_tokens.language_id)
            .electric_character();
        electric_character_support.on_electric_character(
            character,
            &scoped_line_tokens,
            column - scoped_line_tokens.first_char_offset,
        )
    }

    /// `normalizeIndentation`.
    pub fn normalize_indentation(&self, str: &[u16]) -> JsString {
        normalize_indentation(str, self.indent_size, self.insert_spaces)
    }

    /// Devolve a coluna visível de uma coluna.
    /// Ver [`cursor_columns`].
    pub fn visible_column_from_column(&self, model: &dyn CursorSimpleModel, position: Position) -> i32 {
        cursor_columns::visible_column_from_column(
            model.get_line_content(position.line_number).units(),
            position.column,
            self.tab_size,
        )
    }

    /// Devolve a coluna de uma coluna visível, limitada às colunas mínima e máxima da linha.
    /// Ver [`cursor_columns`].
    pub fn column_from_visible_column(
        &self,
        model: &dyn CursorSimpleModel,
        line_number: i32,
        visible_column: i32,
    ) -> i32 {
        let result = cursor_columns::column_from_visible_column(
            model.get_line_content(line_number).units(),
            visible_column,
            self.tab_size,
        );

        let min_column = model.get_line_min_column(line_number);
        if result < min_column {
            return min_column;
        }

        let max_column = model.get_line_max_column(line_number);
        if result > max_column {
            return max_column;
        }

        result
    }
}

/// `ICursorSimpleModel`: um modelo simples (o modelo ou o modelo de visão).
pub trait CursorSimpleModel {
    fn get_line_count(&self) -> i32;
    fn get_line_content(&self, line_number: i32) -> JsString;
    fn get_line_min_column(&self, line_number: i32) -> i32;
    fn get_line_max_column(&self, line_number: i32) -> i32;
    fn get_line_first_non_whitespace_column(&self, line_number: i32) -> i32;
    fn get_line_last_non_whitespace_column(&self, line_number: i32) -> i32;
    fn normalize_position(&self, position: Position, affinity: PositionAffinity) -> Position;

    /// A coluna em que a indentação de uma linha termina (`@internal` no original).
    fn get_line_indent_column(&self, line_number: i32) -> i32;
}

/// `PartialCursorState`: `CursorState | PartialModelCursorState | PartialViewCursorState`.
pub enum PartialCursorState {
    Full(CursorState),
    Model(PartialModelCursorState),
    View(PartialViewCursorState),
}

impl PartialCursorState {
    /// `modelState` (`null` no estado parcial da visão).
    pub fn model_state(&self) -> Option<&SingleCursorState> {
        match self {
            PartialCursorState::Full(state) => Some(&state.model_state),
            PartialCursorState::Model(state) => Some(&state.model_state),
            PartialCursorState::View(_) => None,
        }
    }

    /// `viewState` (`null` no estado parcial do modelo).
    pub fn view_state(&self) -> Option<&SingleCursorState> {
        match self {
            PartialCursorState::Full(state) => Some(&state.view_state),
            PartialCursorState::Model(_) => None,
            PartialCursorState::View(state) => Some(&state.view_state),
        }
    }
}

/// `CursorState`.
#[derive(Clone, Copy, Debug)]
pub struct CursorState {
    pub model_state: SingleCursorState,
    pub view_state: SingleCursorState,
}

impl CursorState {
    pub fn from_model_state(model_state: SingleCursorState) -> PartialModelCursorState {
        PartialModelCursorState::new(model_state)
    }

    pub fn from_view_state(view_state: SingleCursorState) -> PartialViewCursorState {
        PartialViewCursorState::new(view_state)
    }

    pub fn from_model_selection(model_selection: Selection) -> PartialModelCursorState {
        let selection = model_selection;
        let model_state = SingleCursorState::new(
            Range::from_positions(selection.get_selection_start(), selection.get_selection_start()),
            SelectionStartKind::Simple,
            0,
            selection.get_position(),
            0,
        );
        CursorState::from_model_state(model_state)
    }

    pub fn from_model_selections(model_selections: &[Selection]) -> Vec<PartialModelCursorState> {
        model_selections
            .iter()
            .map(|model_selection| CursorState::from_model_selection(*model_selection))
            .collect()
    }

    pub fn new(model_state: SingleCursorState, view_state: SingleCursorState) -> Self {
        Self { model_state, view_state }
    }

    pub fn equals(&self, other: &CursorState) -> bool {
        self.view_state.equals(&other.view_state) && self.model_state.equals(&other.model_state)
    }
}

/// `PartialModelCursorState`: só o estado do modelo (o `viewState` do original é `null`).
#[derive(Clone, Copy, Debug)]
pub struct PartialModelCursorState {
    pub model_state: SingleCursorState,
}

impl PartialModelCursorState {
    pub fn new(model_state: SingleCursorState) -> Self {
        Self { model_state }
    }
}

/// `PartialViewCursorState`: só o estado da visão (o `modelState` do original é `null`).
#[derive(Clone, Copy, Debug)]
pub struct PartialViewCursorState {
    pub view_state: SingleCursorState,
}

impl PartialViewCursorState {
    pub fn new(view_state: SingleCursorState) -> Self {
        Self { view_state }
    }
}

/// `SelectionStartKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionStartKind {
    Simple,
    Word,
    Line,
}

/// `SingleCursorState`: o estado do cursor no modelo ou no modelo de visão.
#[derive(Clone, Copy, Debug)]
pub struct SingleCursorState {
    pub selection_start: Range,
    pub selection_start_kind: SelectionStartKind,
    pub selection_start_leftover_visible_columns: i32,
    pub position: Position,
    pub leftover_visible_columns: i32,

    pub selection: Selection,
}

impl SingleCursorState {
    pub fn new(
        selection_start: Range,
        selection_start_kind: SelectionStartKind,
        selection_start_leftover_visible_columns: i32,
        position: Position,
        leftover_visible_columns: i32,
    ) -> Self {
        Self {
            selection_start,
            selection_start_kind,
            selection_start_leftover_visible_columns,
            position,
            leftover_visible_columns,
            selection: Self::compute_selection(&selection_start, &position),
        }
    }

    pub fn equals(&self, other: &SingleCursorState) -> bool {
        self.selection_start_leftover_visible_columns == other.selection_start_leftover_visible_columns
            && self.leftover_visible_columns == other.leftover_visible_columns
            && self.selection_start_kind == other.selection_start_kind
            && self.position == other.position
            && self.selection_start == other.selection_start
    }

    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty() || !self.selection_start.is_empty()
    }

    pub fn move_to(
        &self,
        in_selection_mode: bool,
        line_number: i32,
        column: i32,
        leftover_visible_columns: i32,
    ) -> SingleCursorState {
        if in_selection_mode {
            // Move só a posição.
            SingleCursorState::new(
                self.selection_start,
                self.selection_start_kind,
                self.selection_start_leftover_visible_columns,
                Position::new(line_number, column),
                leftover_visible_columns,
            )
        } else {
            // Move tudo.
            SingleCursorState::new(
                Range::new(line_number, column, line_number, column),
                SelectionStartKind::Simple,
                leftover_visible_columns,
                Position::new(line_number, column),
                leftover_visible_columns,
            )
        }
    }

    fn compute_selection(selection_start: &Range, position: &Position) -> Selection {
        if selection_start.is_empty() || !position.is_before_or_equal(&selection_start.get_start_position()) {
            Selection::from_positions(selection_start.get_start_position(), *position)
        } else {
            Selection::from_positions(selection_start.get_end_position(), *position)
        }
    }
}

/// O `followUp` de [`EditOperationResult`]: calculado a partir das seleções resultantes da edição e
/// executado logo depois dela. Devolve `None` para pular a edição seguinte.
pub type EditOperationFollowUp = Box<dyn Fn(&[Selection]) -> Option<EditOperationResult>>;

/// O objeto `opts` do construtor de [`EditOperationResult`].
pub struct EditOperationResultOptions {
    pub should_push_stack_element_before: bool,
    pub should_push_stack_element_after: bool,
    pub follow_up: Option<EditOperationFollowUp>,
}

/// `EditOperationResult`.
pub struct EditOperationResult {
    pub r#type: EditOperationType,
    pub commands: Vec<Option<Box<dyn Command>>>,
    pub should_push_stack_element_before: bool,
    pub should_push_stack_element_after: bool,
    /// Uma operação de edição opcional, calculada a partir das seleções resultantes da execução desta e
    /// executada logo depois dela. Pode devolver `None` para pular a edição seguinte.
    pub follow_up: Option<EditOperationFollowUp>,
}

impl EditOperationResult {
    pub fn new(
        r#type: EditOperationType,
        commands: Vec<Option<Box<dyn Command>>>,
        opts: EditOperationResultOptions,
    ) -> Self {
        Self {
            r#type,
            commands,
            should_push_stack_element_before: opts.should_push_stack_element_before,
            should_push_stack_element_after: opts.should_push_stack_element_after,
            follow_up: opts.follow_up,
        }
    }
}

/// `isQuote`: `'`, `"` ou `` ` ``.
pub fn is_quote(ch: &[u16]) -> bool {
    ch == [u16::from(b'\'')] || ch == [u16::from(b'"')] || ch == [u16::from(b'`')]
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::collections::HashMap;

    use je_base::event::{Emitter, Event};
    use je_base::lifecycle::{mark_as_singleton, to_disposable, DisposableRef};

    use super::*;
    use crate::character_pair::CharacterPairSupport;
    use crate::config::editor_options::layout_info::{EditorLayoutInfo, EditorMinimapLayoutInfo, OverviewRulerPosition, RenderMinimap};
    use crate::config::editor_options::option_support::testing;
    use crate::language_configuration::{AutoClosingPair, CommentRule, LanguageConfiguration};
    use crate::language_configuration_registry::{
        LanguageConfigurationServiceChangeEvent, ResolvedLanguageConfiguration,
    };
    use crate::model::{BracketPairColorizationOptions, DefaultEndOfLine, IndentSize};

    // ---------------------------------------------------------------------------------------------
    // Montagem

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn js(text: &str) -> JsString {
        JsString::from_units(units(text))
    }

    /// `ILanguageConfigurationService` de teste: guarda as configurações registradas por linguagem.
    struct TestLanguageConfigurationService {
        configurations: RefCell<HashMap<String, Rc<ResolvedLanguageConfiguration>>>,
        on_did_change: Rc<Emitter<LanguageConfigurationServiceChangeEvent>>,
    }

    impl TestLanguageConfigurationService {
        fn new() -> Rc<Self> {
            Rc::new(Self { configurations: RefCell::new(HashMap::new()), on_did_change: Emitter::new() })
        }
    }

    impl ILanguageConfigurationService for TestLanguageConfigurationService {
        fn on_did_change(&self) -> Event<LanguageConfigurationServiceChangeEvent> {
            self.on_did_change.event()
        }

        fn register(
            &self,
            language_id: &str,
            configuration: LanguageConfiguration,
            _priority: Option<i32>,
        ) -> DisposableRef {
            self.configurations.borrow_mut().insert(
                language_id.to_owned(),
                Rc::new(ResolvedLanguageConfiguration::new(language_id, configuration)),
            );
            mark_as_singleton(to_disposable(|| {}))
        }

        fn get_language_configuration(&self, language_id: &str) -> Rc<ResolvedLanguageConfiguration> {
            if let Some(configuration) = self.configurations.borrow().get(language_id) {
                return configuration.clone();
            }
            Rc::new(ResolvedLanguageConfiguration::new(language_id, LanguageConfiguration::default()))
        }
    }

    struct TestConfiguration {
        options: ComputedEditorOptions,
    }

    impl EditorConfiguration for TestConfiguration {
        fn options(&self) -> &ComputedEditorOptions {
            &self.options
        }
    }

    fn layout(height: f64) -> EditorLayoutInfo {
        EditorLayoutInfo {
            width: 600.0,
            height,
            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,
            line_numbers_left: 0.0,
            line_numbers_width: 0.0,
            decorations_left: 0.0,
            decorations_width: 0.0,
            content_left: 0.0,
            content_width: 600.0,
            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::None,
                minimap_left: 0.0,
                minimap_width: 0.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 1.0,
                minimap_line_height: 1.0,
                minimap_canvas_inner_width: 0.0,
                minimap_canvas_inner_height: 0.0,
                minimap_canvas_outer_width: 0.0,
                minimap_canvas_outer_height: 0.0,
            },
            viewport_column: 80.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            overview_ruler: OverviewRulerPosition { width: 0.0, height: 0.0, top: 0.0, right: 0.0 },
        }
    }

    fn text(value: &str) -> Rc<dyn Any> {
        testing::value(value.to_owned())
    }

    /// Opções com os três modos de fechamento dados e a altura do editor; o resto com valores comuns.
    fn options(quotes: &str, brackets: &str, comments: &str, height: f64) -> ComputedEditorOptions {
        testing::options_with(vec![
            (EditorOption::LayoutInfo, testing::value(layout(height))),
            (EditorOption::FontInfo, testing::value(testing::font_info(true, 14.0, "normal"))),
            (EditorOption::ReadOnly, testing::value(false)),
            (EditorOption::StickyTabStops, testing::value(true)),
            (EditorOption::UseTabStops, testing::value(true)),
            (EditorOption::TrimWhitespaceOnDelete, testing::value(false)),
            (EditorOption::WordSeparators, text("`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?")),
            (EditorOption::EmptySelectionClipboard, testing::value(true)),
            (EditorOption::CopyWithSyntaxHighlighting, testing::value(true)),
            (EditorOption::MultiCursorMergeOverlapping, testing::value(true)),
            (EditorOption::MultiCursorPaste, text("spread")),
            (EditorOption::MultiCursorLimit, testing::value(10000.0_f64)),
            (EditorOption::AutoClosingBrackets, text(brackets)),
            (EditorOption::AutoClosingComments, text(comments)),
            (EditorOption::AutoClosingQuotes, text(quotes)),
            (EditorOption::AutoClosingDelete, text("auto")),
            (EditorOption::AutoClosingOvertype, text("auto")),
            (EditorOption::AutoSurround, text("languageDefined")),
            (EditorOption::AutoIndent, testing::value(EditorAutoIndentStrategy::Advanced)),
            (EditorOption::WordSegmenterLocales, testing::value(Vec::<String>::new())),
            (EditorOption::OvertypeOnPaste, testing::value(false)),
        ])
    }

    fn model_options(tab_size: i32, insert_spaces: bool) -> TextModelResolvedOptions {
        TextModelResolvedOptions::new(
            tab_size,
            IndentSize::TabSize,
            insert_spaces,
            DefaultEndOfLine::Lf,
            true,
            BracketPairColorizationOptions { enabled: false, independent_color_pool_per_bracket_type: false },
        )
    }

    fn configuration_with(
        service: &Rc<TestLanguageConfigurationService>,
        quotes: &str,
        brackets: &str,
        comments: &str,
    ) -> CursorConfiguration {
        let service: Rc<dyn ILanguageConfigurationService> = service.clone();
        CursorConfiguration::new(
            "plaintext",
            &model_options(4, true),
            &TestConfiguration { options: options(quotes, brackets, comments, 200.0) },
            service,
        )
    }

    /// Modelo simples de teste: as linhas e a coluna mínima.
    struct TestModel {
        lines: Vec<&'static str>,
        min_column: i32,
    }

    impl CursorSimpleModel for TestModel {
        fn get_line_count(&self) -> i32 {
            self.lines.len() as i32
        }

        fn get_line_content(&self, line_number: i32) -> JsString {
            js(self.lines[line_number as usize - 1])
        }

        fn get_line_min_column(&self, _line_number: i32) -> i32 {
            self.min_column
        }

        fn get_line_max_column(&self, line_number: i32) -> i32 {
            self.get_line_content(line_number).len() as i32 + 1
        }

        fn get_line_first_non_whitespace_column(&self, line_number: i32) -> i32 {
            let content = self.get_line_content(line_number);
            content
                .units()
                .iter()
                .position(|&u| u != u16::from(b' ') && u != u16::from(b'\t'))
                .map_or(0, |index| index as i32 + 1)
        }

        fn get_line_last_non_whitespace_column(&self, line_number: i32) -> i32 {
            let content = self.get_line_content(line_number);
            content
                .units()
                .iter()
                .rposition(|&u| u != u16::from(b' ') && u != u16::from(b'\t'))
                .map_or(0, |index| index as i32 + 2)
        }

        fn normalize_position(&self, position: Position, _affinity: PositionAffinity) -> Position {
            position
        }

        fn get_line_indent_column(&self, line_number: i32) -> i32 {
            match self.get_line_first_non_whitespace_column(line_number) {
                0 => self.get_line_max_column(line_number),
                column => column,
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // SingleCursorState e CursorState

    fn state(start: Range, position: Position) -> SingleCursorState {
        SingleCursorState::new(start, SelectionStartKind::Simple, 0, position, 0)
    }

    #[test]
    fn single_state_selection_follows_the_anchor_range() {
        // Âncora vazia ou posição depois do começo da âncora: do começo da âncora até a posição.
        let forward = state(Range::new(1, 2, 1, 5), Position::new(1, 3));
        assert_eq!(forward.selection, Selection::new(1, 2, 1, 3));
        let empty_anchor = state(Range::new(1, 4, 1, 4), Position::new(1, 1));
        assert_eq!(empty_anchor.selection, Selection::new(1, 4, 1, 1));
        // Posição antes (ou no) começo de uma âncora não vazia: do fim da âncora até a posição.
        let backward = state(Range::new(1, 2, 1, 5), Position::new(1, 1));
        assert_eq!(backward.selection, Selection::new(1, 5, 1, 1));
        let at_start = state(Range::new(1, 2, 1, 5), Position::new(1, 2));
        assert_eq!(at_start.selection, Selection::new(1, 5, 1, 2));
    }

    #[test]
    fn single_state_has_selection() {
        assert!(!state(Range::new(2, 3, 2, 3), Position::new(2, 3)).has_selection());
        assert!(state(Range::new(2, 3, 2, 3), Position::new(2, 4)).has_selection());
        assert!(state(Range::new(2, 3, 2, 6), Position::new(2, 3)).has_selection());
    }

    #[test]
    fn single_state_equals_compares_every_field_but_the_derived_selection() {
        let base = SingleCursorState::new(Range::new(1, 2, 1, 5), SelectionStartKind::Word, 3, Position::new(1, 4), 7);
        let same = SingleCursorState::new(Range::new(1, 2, 1, 5), SelectionStartKind::Word, 3, Position::new(1, 4), 7);
        assert!(base.equals(&same));
        let other_kind =
            SingleCursorState::new(Range::new(1, 2, 1, 5), SelectionStartKind::Line, 3, Position::new(1, 4), 7);
        assert!(!base.equals(&other_kind));
        let other_start_leftover =
            SingleCursorState::new(Range::new(1, 2, 1, 5), SelectionStartKind::Word, 4, Position::new(1, 4), 7);
        assert!(!base.equals(&other_start_leftover));
        let other_leftover =
            SingleCursorState::new(Range::new(1, 2, 1, 5), SelectionStartKind::Word, 3, Position::new(1, 4), 8);
        assert!(!base.equals(&other_leftover));
        let other_position =
            SingleCursorState::new(Range::new(1, 2, 1, 5), SelectionStartKind::Word, 3, Position::new(1, 5), 7);
        assert!(!base.equals(&other_position));
        let other_start =
            SingleCursorState::new(Range::new(1, 2, 1, 6), SelectionStartKind::Word, 3, Position::new(1, 4), 7);
        assert!(!base.equals(&other_start));
    }

    #[test]
    fn single_state_move_in_selection_mode_keeps_the_anchor() {
        let base = SingleCursorState::new(Range::new(1, 2, 1, 5), SelectionStartKind::Word, 3, Position::new(1, 4), 7);
        let moved = base.move_to(true, 4, 6, 9);
        assert_eq!(moved.selection_start, Range::new(1, 2, 1, 5));
        assert_eq!(moved.selection_start_kind, SelectionStartKind::Word);
        assert_eq!(moved.selection_start_leftover_visible_columns, 3);
        assert_eq!(moved.position, Position::new(4, 6));
        assert_eq!(moved.leftover_visible_columns, 9);
    }

    #[test]
    fn single_state_move_outside_selection_mode_moves_everything() {
        let base = SingleCursorState::new(Range::new(1, 2, 1, 5), SelectionStartKind::Word, 3, Position::new(1, 4), 7);
        let moved = base.move_to(false, 4, 6, 9);
        assert_eq!(moved.selection_start, Range::new(4, 6, 4, 6));
        assert_eq!(moved.selection_start_kind, SelectionStartKind::Simple);
        assert_eq!(moved.selection_start_leftover_visible_columns, 9);
        assert_eq!(moved.position, Position::new(4, 6));
        assert_eq!(moved.leftover_visible_columns, 9);
        assert_eq!(moved.selection, Selection::new(4, 6, 4, 6));
        assert!(!moved.has_selection());
    }

    #[test]
    fn cursor_state_from_model_selection_anchors_at_the_selection_start() {
        let partial = CursorState::from_model_selection(Selection::new(1, 5, 1, 2));
        let model_state = partial.model_state;
        assert_eq!(model_state.selection_start, Range::new(1, 5, 1, 5));
        assert_eq!(model_state.selection_start_kind, SelectionStartKind::Simple);
        assert_eq!(model_state.selection_start_leftover_visible_columns, 0);
        assert_eq!(model_state.position, Position::new(1, 2));
        assert_eq!(model_state.leftover_visible_columns, 0);
        assert_eq!(model_state.selection, Selection::new(1, 5, 1, 2));

        let all = CursorState::from_model_selections(&[Selection::new(1, 1, 1, 1), Selection::new(2, 2, 3, 3)]);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].model_state.selection, Selection::new(2, 2, 3, 3));
    }

    #[test]
    fn cursor_state_equals_compares_model_and_view() {
        let model = state(Range::new(1, 1, 1, 1), Position::new(1, 1));
        let view = state(Range::new(2, 1, 2, 1), Position::new(2, 1));
        let a = CursorState::new(model, view);
        assert!(a.equals(&CursorState::new(model, view)));
        assert!(!a.equals(&CursorState::new(model, model)));
        assert!(!a.equals(&CursorState::new(view, view)));
    }

    #[test]
    fn partial_states_expose_only_their_side() {
        let model = state(Range::new(1, 1, 1, 1), Position::new(1, 1));
        let view = state(Range::new(2, 1, 2, 1), Position::new(2, 1));

        let from_model = PartialCursorState::Model(CursorState::from_model_state(model));
        assert!(from_model.model_state().is_some_and(|s| s.equals(&model)));
        assert!(from_model.view_state().is_none());

        let from_view = PartialCursorState::View(CursorState::from_view_state(view));
        assert!(from_view.model_state().is_none());
        assert!(from_view.view_state().is_some_and(|s| s.equals(&view)));

        let full = PartialCursorState::Full(CursorState::new(model, view));
        assert!(full.model_state().is_some_and(|s| s.equals(&model)));
        assert!(full.view_state().is_some_and(|s| s.equals(&view)));
    }

    // ---------------------------------------------------------------------------------------------
    // Pequenas peças

    #[test]
    fn is_quote_accepts_the_three_quote_characters() {
        assert!(is_quote(&units("'")));
        assert!(is_quote(&units("\"")));
        assert!(is_quote(&units("`")));
        assert!(!is_quote(&units("a")));
        assert!(!is_quote(&units("''")));
        assert!(!is_quote(&[]));
    }

    #[test]
    fn edit_operation_type_values_follow_the_original() {
        assert_eq!(
            [
                EditOperationType::Other as i32,
                EditOperationType::DeletingLeft as i32,
                EditOperationType::DeletingRight as i32,
                EditOperationType::TypingOther as i32,
                EditOperationType::TypingFirstSpace as i32,
                EditOperationType::TypingConsecutiveSpace as i32,
            ],
            [0, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn edit_operation_result_keeps_its_options_and_follow_up() {
        let result = EditOperationResult::new(
            EditOperationType::TypingOther,
            vec![None],
            EditOperationResultOptions {
                should_push_stack_element_before: true,
                should_push_stack_element_after: false,
                follow_up: Some(Box::new(|selections| {
                    assert_eq!(selections, [Selection::new(1, 1, 1, 1)]);
                    None
                })),
            },
        );
        assert_eq!(result.r#type, EditOperationType::TypingOther);
        assert_eq!(result.commands.len(), 1);
        assert!(result.should_push_stack_element_before);
        assert!(!result.should_push_stack_element_after);
        let follow_up = result.follow_up.as_ref().expect("followUp");
        assert!(follow_up(&[Selection::new(1, 1, 1, 1)]).is_none());
    }

    // ---------------------------------------------------------------------------------------------
    // CursorConfiguration

    #[test]
    fn should_recreate_follows_the_listed_options() {
        let changed = |id: EditorOption| {
            let mut values = vec![false; EditorOption::COUNT];
            values[id as usize] = true;
            CursorConfiguration::should_recreate(&ConfigurationChangedEvent::new(values))
        };
        for id in [
            EditorOption::LayoutInfo,
            EditorOption::WordSeparators,
            EditorOption::EmptySelectionClipboard,
            EditorOption::MultiCursorMergeOverlapping,
            EditorOption::MultiCursorPaste,
            EditorOption::MultiCursorLimit,
            EditorOption::AutoClosingBrackets,
            EditorOption::AutoClosingComments,
            EditorOption::AutoClosingQuotes,
            EditorOption::AutoClosingDelete,
            EditorOption::AutoClosingOvertype,
            EditorOption::AutoSurround,
            EditorOption::UseTabStops,
            EditorOption::TrimWhitespaceOnDelete,
            EditorOption::FontInfo,
            EditorOption::ReadOnly,
            EditorOption::WordSegmenterLocales,
            EditorOption::OvertypeOnPaste,
        ] {
            assert!(changed(id), "{id:?}");
        }
        // `autoIndent` e `stickyTabStops` não estão na lista.
        assert!(!changed(EditorOption::AutoIndent));
        assert!(!changed(EditorOption::StickyTabStops));
        assert!(!CursorConfiguration::should_recreate(&ConfigurationChangedEvent::new(vec![
            false;
            EditorOption::COUNT
        ])));
    }

    #[test]
    fn constructor_reads_model_and_editor_options() {
        let service = TestLanguageConfigurationService::new();
        let configuration = configuration_with(&service, "always", "never", "beforeWhitespace");
        assert_eq!(configuration.tab_size, 4);
        assert_eq!(configuration.indent_size, 4);
        assert!(configuration.insert_spaces);
        assert!(!configuration.read_only);
        assert!(configuration.sticky_tab_stops);
        assert_eq!(configuration.line_height, 19.0);
        assert_eq!(configuration.typical_halfwidth_character_width, 8.0);
        // `Math.max(1, Math.floor(200 / 19) - 2)`: floor(10.52) = 10, menos 2.
        assert_eq!(configuration.page_size, 8.0);
        assert_eq!(configuration.multi_cursor_paste, "spread");
        assert_eq!(configuration.multi_cursor_limit, 10000.0);
        assert_eq!(configuration.auto_indent, EditorAutoIndentStrategy::Advanced);
        assert_eq!(configuration.auto_closing_quotes, "always");
        assert_eq!(configuration.auto_closing_brackets, "never");
        assert_eq!(configuration.auto_closing_comments, "beforeWhitespace");
    }

    #[test]
    fn page_size_has_a_floor_of_one() {
        let service: Rc<dyn ILanguageConfigurationService> = TestLanguageConfigurationService::new();
        let configuration = CursorConfiguration::new(
            "plaintext",
            &model_options(4, true),
            &TestConfiguration { options: options("always", "always", "always", 20.0) },
            service,
        );
        // floor(20 / 19) - 2 = -1, e o piso é 1.
        assert_eq!(configuration.page_size, 1.0);
    }

    #[test]
    fn should_auto_close_before_honors_always_never_and_before_whitespace() {
        let service = TestLanguageConfigurationService::new();
        let configuration = configuration_with(&service, "always", "never", "beforeWhitespace");
        let a = units("a");
        assert!((configuration.should_auto_close_before.quote)(&a));
        assert!(!(configuration.should_auto_close_before.bracket)(&a));
        assert!(!(configuration.should_auto_close_before.bracket)(&units(" ")));
        assert!((configuration.should_auto_close_before.comment)(&units(" ")));
        assert!((configuration.should_auto_close_before.comment)(&units("\t")));
        assert!(!(configuration.should_auto_close_before.comment)(&a));
        assert!(!(configuration.should_auto_close_before.comment)(&units("\n")));
    }

    #[test]
    fn should_auto_close_before_language_defined_uses_the_default_sets() {
        // Padrões de `characterPair.ts`: aspas `;:.,=}])> \n\t`, colchetes com `'"\`` a mais.
        let service = TestLanguageConfigurationService::new();
        let configuration = configuration_with(&service, "languageDefined", "languageDefined", "languageDefined");
        let quote = &configuration.should_auto_close_before.quote;
        let bracket = &configuration.should_auto_close_before.bracket;
        for ch in [";", ":", ".", ",", "=", "}", "]", ")", ">", " ", "\n", "\t"] {
            assert!(quote(&units(ch)), "aspas antes de {ch:?}");
            assert!(bracket(&units(ch)), "colchete antes de {ch:?}");
        }
        for ch in ["'", "\"", "`"] {
            assert!(!quote(&units(ch)), "aspas antes de {ch:?}");
            assert!(bracket(&units(ch)), "colchete antes de {ch:?}");
        }
        assert!(!quote(&units("a")));
        assert!(!bracket(&units("a")));
        // `indexOf('')` acha na posição 0.
        assert!(quote(&[]));
    }

    #[test]
    fn should_auto_close_before_language_defined_uses_the_language_auto_close_before() {
        let service = TestLanguageConfigurationService::new();
        service.register(
            "plaintext",
            LanguageConfiguration { auto_close_before: Some("xy".to_owned()), ..LanguageConfiguration::default() },
            None,
        );
        let configuration = configuration_with(&service, "languageDefined", "languageDefined", "languageDefined");
        assert!((configuration.should_auto_close_before.quote)(&units("x")));
        assert!((configuration.should_auto_close_before.bracket)(&units("y")));
        assert!(!(configuration.should_auto_close_before.comment)(&units(";")));
        assert!(!(configuration.should_auto_close_before.quote)(&units(" ")));
    }

    #[test]
    fn language_defaults_match_the_character_pair_constants() {
        assert_eq!(CharacterPairSupport::DEFAULT_AUTOCLOSE_BEFORE_WHITESPACE, " \n\t");
    }

    #[test]
    fn constructor_reads_surrounding_pairs_and_block_comment_token() {
        let service = TestLanguageConfigurationService::new();
        service.register(
            "plaintext",
            LanguageConfiguration {
                surrounding_pairs: Some(vec![AutoClosingPair::new("(", ")"), AutoClosingPair::new("<!--", "-->")]),
                comments: Some(CommentRule {
                    line_comment: None,
                    block_comment: Some(("/*".to_owned(), "*/".to_owned())),
                }),
                ..LanguageConfiguration::default()
            },
            None,
        );
        let configuration = configuration_with(&service, "always", "always", "always");
        assert_eq!(configuration.surrounding_pairs.get(&js("(")), Some(&js(")")));
        assert_eq!(configuration.surrounding_pairs.get(&js("<!--")), Some(&js("-->")));
        assert_eq!(configuration.surrounding_pairs.len(), 2);
        assert_eq!(configuration.block_comment_start_token.as_deref(), Some("/*"));

        let plain = configuration_with(&TestLanguageConfigurationService::new(), "always", "always", "always");
        assert!(plain.surrounding_pairs.is_empty());
        assert_eq!(plain.block_comment_start_token, None);
    }

    #[test]
    fn electric_chars_are_the_last_character_of_each_closing_bracket() {
        let service = TestLanguageConfigurationService::new();
        service.register(
            "plaintext",
            LanguageConfiguration {
                brackets: Some(vec![("{".to_owned(), "}".to_owned()), ("[".to_owned(), "]".to_owned())]),
                ..LanguageConfiguration::default()
            },
            None,
        );
        let configuration = configuration_with(&service, "always", "always", "always");
        let electric_chars = configuration.electric_chars();
        assert_eq!(electric_chars.len(), 2);
        assert!(electric_chars.contains(&js("}")));
        assert!(electric_chars.contains(&js("]")));
        assert!(!electric_chars.contains(&js("{")));
        // A segunda leitura devolve o mesmo conjunto calculado.
        assert!(Rc::ptr_eq(&electric_chars, &configuration.electric_chars()));

        let empty = configuration_with(&TestLanguageConfigurationService::new(), "always", "always", "always");
        assert!(empty.electric_chars().is_empty());
    }

    #[test]
    fn input_mode_reads_the_singleton() {
        let service = TestLanguageConfigurationService::new();
        let configuration = configuration_with(&service, "always", "always", "always");
        assert_eq!(configuration.input_mode(), InputMode::Insert);
        input_mode::set_input_mode(InputMode::Overtype);
        assert_eq!(configuration.input_mode(), InputMode::Overtype);
        input_mode::set_input_mode(InputMode::Insert);
        assert_eq!(configuration.input_mode(), InputMode::Insert);
    }

    #[test]
    fn normalize_indentation_uses_the_model_options() {
        let service = TestLanguageConfigurationService::new();
        let configuration = configuration_with(&service, "always", "always", "always");
        assert_eq!(configuration.normalize_indentation(&units("\tx")), js("    x"));
        assert_eq!(configuration.normalize_indentation(&units("  y")), js("  y"));
    }

    #[test]
    fn visible_column_conversions_use_tabs_and_clamp_to_the_line() {
        let service = TestLanguageConfigurationService::new();
        let configuration = configuration_with(&service, "always", "always", "always");
        // `|<TAB>|<TAB>|T|ext` com tabulação de 4 (documentação de `cursorColumns.ts`).
        let model = TestModel { lines: vec!["\t\tText"], min_column: 1 };
        assert_eq!(configuration.visible_column_from_column(&model, Position::new(1, 1)), 0);
        assert_eq!(configuration.visible_column_from_column(&model, Position::new(1, 2)), 4);
        assert_eq!(configuration.visible_column_from_column(&model, Position::new(1, 3)), 8);
        assert_eq!(configuration.visible_column_from_column(&model, Position::new(1, 4)), 9);
        assert_eq!(configuration.column_from_visible_column(&model, 1, 9), 4);
        assert_eq!(configuration.column_from_visible_column(&model, 1, 4), 2);
        // Passa do fim da linha: fica na coluna máxima.
        assert_eq!(configuration.column_from_visible_column(&model, 1, 100), 7);

        // Antes da coluna mínima: fica na coluna mínima.
        let indented = TestModel { lines: vec!["\t\tText"], min_column: 3 };
        assert_eq!(configuration.column_from_visible_column(&indented, 1, 0), 3);
        assert_eq!(configuration.column_from_visible_column(&indented, 1, 8), 3);
    }
}
