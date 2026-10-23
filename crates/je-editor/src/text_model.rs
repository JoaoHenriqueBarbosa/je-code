//! Porte de `src/vs/editor/common/model/textModel.ts`: o modelo de texto do editor.
//!
//! Decisões de tradução:
//! - `TextModel` é um objeto compartilhado (`Rc<TextModel>`) com mutabilidade interior, como o grafo de
//!   objetos do original (ver `docs/design.md`): listeners podem chamar o modelo de dentro de um evento
//!   (os testes `change while emitting events` fazem isso). Nenhum `RefCell` fica emprestado enquanto um
//!   evento é entregue.
//! - Texto é UTF-16 ([`JsString`]); `getEOL(): string` vira [`Eol`]; os números de versão são `i64`.
//! - As exceções do original viram pânico com [`Error`] (`BugIndicatingError` na validação de linha e no
//!   modelo descartado).
//! - O `IUndoRedoService` injetado é o [`UndoRedoService`] concreto. `ILanguageService` e
//!   `ILanguageConfigurationService` chegam por [`TextModelServices`] (`new_with_language_service`); sem eles
//!   o modelo cria os seus e os descarta junto. `IInstantiationService` não existe: a parte de tokenização
//!   é criada no construtor, com a definição de palavra por linguagem vinda do serviço de configuração.
//! - `TextModel::tokenization()` é o `ITokenizationTextModelPart` (`tokenization_text_model_part.rs`):
//!   `getLineTokens`, `forceTokenization`, `getLanguageId`/`setLanguageId`, tokens semânticos e eventos.
//!
//! `bracketPairs`, `guides` e o provedor de decorações de colorização de pares de colchetes ligam-se ao
//! modelo (`bracket_pairs()`, `guides()`, e as decorações somadas em `get_*_decorations`).
//!
//! Pendente, por depender de serviços ainda não portados: o provedor de decorações de fontes por token,
//! `getFormattingOptions`
//! (`languages.ts`), `edit(TextEdit)` (`edits/textEdit.ts`) e a criação do buffer a partir de stream
//! (`createTextBufferFactoryFromStream`, depende de `stream.ts`).

use std::cell::{Cell, OnceCell, Ref, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event, Listener};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::strings;
use je_base::uri::URI;

use crate::bracket_pairs_text_model_part::BracketPairsTextModelPart;
use crate::cursor_common::CursorSimpleModel;
use crate::text_model_defaults::EDITOR_MODEL_DEFAULTS;
use crate::bracket_pairs_text_model_part::colorized_bracket_pairs_decoration_provider::ColorizedBracketPairsDecorationProvider;
use crate::guides_text_model_part::GuidesTextModelPart;
use crate::attached_views::{AttachedView, AttachedViews};
use crate::edit_operation::SingleEditOperation;
use crate::edit_stack::EditStack;
use crate::eol_counter::count_eol;
use crate::indentation::normalize_indentation;
use crate::indentation_guesser::guess_indentation;
use crate::interval_tree::{IntervalArena, IntervalNode, IntervalTree, NodeId, recompute_max_end};
use crate::model::{
    DecorationColor, DefaultEndOfLine, EndOfLinePreference, EndOfLineSequence, Eol,
    GlyphMarginLane, IndentSize, InjectedTextOptions, MarkdownString, MinimapPosition, MinimapSectionHeaderStyle,
    ModelDecoration, ModelDecorationGlyphMarginInit, ModelDecorationMinimapInit, ModelDecorationOptionsInit,
    ModelDecorationOverviewRulerInit, ModelDeltaDecoration, OverviewRulerLane, PositionAffinity, TextDirection,
    TextModelCreationOptions, TextModelResolvedOptions, TextModelUpdateOptions, TextSnapshot, TrackedRangeStickiness,
    ValidAnnotatedEditOperation, ValidEditOperation, CursorStateComputer, FindMatch, SearchData,
};
use crate::language_configuration_registry::{ILanguageConfigurationService, LanguageConfigurationService};
use crate::language_service::{ILanguageService, LanguageService};
use crate::piece_tree_text_buffer::PieceTreeTextBuffer;
use crate::tokenization_text_model_part::{TokenizationTextModelPart, WordDefinitionProvider};
use crate::piece_tree_text_buffer_builder::{PieceTreeTextBufferBuilder, PieceTreeTextBufferFactory};
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_change::TextChange;
use crate::text_model_edit_source::{EditSources, TextModelEditSource};
use crate::text_model_events::{
    InternalModelContentChangeEvent, LineInjectedText, ModelContentChange, ModelContentChangedEvent,
    ModelDecorationsChangedEvent, ModelFontChanged, ModelFontChangedEvent, ModelInjectedTextChangedEvent,
    ModelLineHeightChanged, ModelLineHeightChangedEvent, ModelOptionsChangedEvent, ModelRawChange,
    ModelRawContentChangedEvent, ModelRawLineChanged, ModelRawLinesDeleted, ModelRawLinesInserted,
};
use crate::text_model_search::{SearchParams, TextModelSearch};
use crate::undo_redo::{ResourceEditStackSnapshot, UndoRedoGroup};
use crate::undo_redo_service::{UndoRedoService, UndoRedoTarget};
use crate::word_helper::WordAtPosition;

/// `Constants.MAX_SAFE_SMALL_INTEGER` do `uint.ts`.
// TODO(integração): mover para je_base::uint.
const MAX_SAFE_SMALL_INTEGER: usize = 1 << 30;


const LINE_HEIGHT_CEILING: f64 = 300.0;

/// `cleanClassName`: troca cada unidade UTF-16 fora de `[a-z0-9\-_]` (sem distinguir caixa) por um espaço.
fn clean_class_name(class_name: &str) -> String {
    class_name
        .encode_utf16()
        .map(|unit| match unit {
            0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A | 0x2D | 0x5F => char::from(unit as u8),
            _ => ' ',
        })
        .collect()
}

/// `ModelDecorationOverviewRulerOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelDecorationOverviewRulerOptions {
    pub color: DecorationColor,
    pub dark_color: DecorationColor,
    pub position: OverviewRulerLane,
}

impl ModelDecorationOverviewRulerOptions {
    pub fn new(options: &ModelDecorationOverviewRulerInit) -> Self {
        Self {
            color: options.color.clone().filter(|c| !is_empty_color(c)).unwrap_or_else(|| DecorationColor::Color(String::new())),
            dark_color: options
                .dark_color
                .clone()
                .filter(|c| !is_empty_color(c))
                .unwrap_or_else(|| DecorationColor::Color(String::new())),
            position: options.position.unwrap_or(OverviewRulerLane::Center),
        }
    }

    /// `!!options.overviewRuler.color`: a cor existe e não é a cadeia vazia.
    pub fn has_color(&self) -> bool {
        !is_empty_color(&self.color)
    }
}

fn is_empty_color(color: &DecorationColor) -> bool {
    matches!(color, DecorationColor::Color(value) if value.is_empty())
}

/// `ModelDecorationGlyphMarginOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelDecorationGlyphMarginOptions {
    pub position: GlyphMarginLane,
    pub persist_lane: Option<bool>,
}

impl ModelDecorationGlyphMarginOptions {
    pub fn new(options: Option<&ModelDecorationGlyphMarginInit>) -> Self {
        Self {
            position: options.map_or(GlyphMarginLane::Center, |o| o.position),
            persist_lane: options.and_then(|o| o.persist_lane),
        }
    }
}

/// `ModelDecorationMinimapOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelDecorationMinimapOptions {
    pub color: DecorationColor,
    pub dark_color: DecorationColor,
    pub position: MinimapPosition,
    pub section_header_style: Option<MinimapSectionHeaderStyle>,
    pub section_header_text: Option<String>,
}

impl ModelDecorationMinimapOptions {
    pub fn new(options: &ModelDecorationMinimapInit) -> Self {
        Self {
            color: options.color.clone().filter(|c| !is_empty_color(c)).unwrap_or_else(|| DecorationColor::Color(String::new())),
            dark_color: options
                .dark_color
                .clone()
                .filter(|c| !is_empty_color(c))
                .unwrap_or_else(|| DecorationColor::Color(String::new())),
            position: options.position,
            section_header_style: options.section_header_style,
            section_header_text: options.section_header_text.clone(),
        }
    }
}

/// `ModelDecorationInjectedTextOptions.from`: normaliza `widthInEm` (só número finito e não negativo vale)
/// e a cadeia vazia de `inlineClassName` vira `None`.
pub fn normalize_injected_text_options(options: &InjectedTextOptions) -> Rc<InjectedTextOptions> {
    Rc::new(InjectedTextOptions {
        content: options.content.clone(),
        tokens: options.tokens.clone(),
        inline_class_name: options.inline_class_name.clone().filter(|name| !name.is_empty()),
        inline_class_name_affects_letter_spacing: options.inline_class_name_affects_letter_spacing,
        width_in_em: options.width_in_em.filter(|width| width.is_finite() && *width >= 0.0),
        cursor_stops: options.cursor_stops,
    })
}

/// `ModelDecorationOptions`: as opções de uma decoração depois da normalização.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelDecorationOptions {
    pub description: String,
    pub block_class_name: Option<String>,
    pub block_is_after_end: Option<bool>,
    pub block_does_not_collapse: Option<bool>,
    pub block_padding: Option<[i32; 4]>,
    pub stickiness: TrackedRangeStickiness,
    pub z_index: i32,
    pub class_name: Option<String>,
    pub should_fill_line_on_line_break: Option<bool>,
    pub hover_message: Option<Vec<MarkdownString>>,
    pub glyph_margin_hover_message: Option<Vec<MarkdownString>>,
    pub is_whole_line: bool,
    pub line_height: Option<f64>,
    pub font_size: Option<String>,
    pub show_if_collapsed: bool,
    pub collapse_on_replace_edit: bool,
    pub overview_ruler: Option<ModelDecorationOverviewRulerOptions>,
    pub minimap: Option<ModelDecorationMinimapOptions>,
    pub glyph_margin: Option<ModelDecorationGlyphMarginOptions>,
    pub glyph_margin_class_name: Option<String>,
    pub lines_decorations_class_name: Option<String>,
    pub line_number_class_name: Option<String>,
    pub line_number_hover_message: Option<Vec<MarkdownString>>,
    pub lines_decorations_tooltip: Option<String>,
    pub first_line_decoration_class_name: Option<String>,
    pub margin_class_name: Option<String>,
    pub inline_class_name: Option<String>,
    pub inline_class_name_affects_letter_spacing: bool,
    pub before_content_class_name: Option<String>,
    pub after_content_class_name: Option<String>,
    pub after: Option<Rc<InjectedTextOptions>>,
    pub before: Option<Rc<InjectedTextOptions>>,
    pub hide_in_comment_tokens: Option<bool>,
    pub hide_in_string_tokens: Option<bool>,
    pub affects_font: Option<bool>,
    pub text_direction: Option<TextDirection>,
}

impl ModelDecorationOptions {
    /// `ModelDecorationOptions.register(options)`.
    pub fn register(options: &ModelDecorationOptionsInit) -> Rc<ModelDecorationOptions> {
        Rc::new(Self::new(options))
    }

    /// `ModelDecorationOptions.createDynamic(options)`.
    pub fn create_dynamic(options: &ModelDecorationOptionsInit) -> Rc<ModelDecorationOptions> {
        Rc::new(Self::new(options))
    }

    /// `ModelDecorationOptions.EMPTY`.
    pub fn empty() -> Rc<ModelDecorationOptions> {
        Self::register(&ModelDecorationOptionsInit {
            description: "empty".to_owned(),
            ..Default::default()
        })
    }

    /// `options.overviewRuler?.color` verdadeiro.
    pub fn in_overview_ruler(&self) -> bool {
        self.overview_ruler.as_ref().is_some_and(ModelDecorationOverviewRulerOptions::has_color)
    }

    /// `!!options.after || !!options.before`.
    pub fn is_injected_text(&self) -> bool {
        self.after.is_some() || self.before.is_some()
    }

    fn new(options: &ModelDecorationOptionsInit) -> Self {
        let non_empty = |value: &Option<String>| value.clone().filter(|v| !v.is_empty());
        let class = |value: &Option<String>| value.as_deref().filter(|v| !v.is_empty()).map(clean_class_name);
        let messages = |value: &Option<Vec<MarkdownString>>| value.clone();
        Self {
            description: options.description.clone(),
            block_class_name: class(&options.block_class_name),
            block_does_not_collapse: options.block_does_not_collapse,
            block_is_after_end: options.block_is_after_end,
            block_padding: options.block_padding,
            stickiness: options.stickiness.unwrap_or(TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges),
            z_index: options.z_index,
            class_name: class(&options.class_name),
            should_fill_line_on_line_break: options.should_fill_line_on_line_break,
            hover_message: messages(&options.hover_message),
            glyph_margin_hover_message: messages(&options.glyph_margin_hover_message),
            line_number_hover_message: messages(&options.line_number_hover_message),
            is_whole_line: options.is_whole_line,
            line_height: options.line_height.filter(|h| *h != 0.0 && !h.is_nan()).map(|h| h.min(LINE_HEIGHT_CEILING)),
            font_size: non_empty(&options.font_size),
            affects_font: Some(
                non_empty(&options.font_size).is_some()
                    || non_empty(&options.font_family).is_some()
                    || non_empty(&options.font_weight).is_some()
                    || non_empty(&options.font_style).is_some(),
            ),
            show_if_collapsed: options.show_if_collapsed,
            collapse_on_replace_edit: options.collapse_on_replace_edit,
            overview_ruler: options.overview_ruler.as_ref().map(ModelDecorationOverviewRulerOptions::new),
            minimap: options.minimap.as_ref().map(ModelDecorationMinimapOptions::new),
            glyph_margin: if non_empty(&options.glyph_margin_class_name).is_some() {
                Some(ModelDecorationGlyphMarginOptions::new(options.glyph_margin.as_ref()))
            } else {
                None
            },
            glyph_margin_class_name: class(&options.glyph_margin_class_name),
            lines_decorations_class_name: class(&options.lines_decorations_class_name),
            line_number_class_name: class(&options.line_number_class_name),
            lines_decorations_tooltip: non_empty(&options.lines_decorations_tooltip).map(|tooltip| {
                let units: Vec<u16> = tooltip.encode_utf16().collect();
                strings::html_attribute_encode_value(&units).to_string_lossy()
            }),
            first_line_decoration_class_name: class(&options.first_line_decoration_class_name),
            margin_class_name: class(&options.margin_class_name),
            inline_class_name: class(&options.inline_class_name),
            inline_class_name_affects_letter_spacing: options.inline_class_name_affects_letter_spacing,
            before_content_class_name: class(&options.before_content_class_name),
            after_content_class_name: class(&options.after_content_class_name),
            after: options.after.as_ref().map(normalize_injected_text_options),
            before: options.before.as_ref().map(normalize_injected_text_options),
            hide_in_comment_tokens: Some(options.hide_in_comment_tokens.unwrap_or(false)),
            hide_in_string_tokens: Some(options.hide_in_string_tokens.unwrap_or(false)),
            text_direction: options.text_direction,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Criação do buffer
// ---------------------------------------------------------------------------------------------------------

/// `string | ITextBufferFactory | ITextSnapshot`: de onde vem o conteúdo de um modelo.
pub enum TextSource {
    Text(JsString),
    Snapshot(Box<dyn TextSnapshot>),
    Factory(PieceTreeTextBufferFactory),
}

impl From<&str> for TextSource {
    fn from(text: &str) -> Self {
        TextSource::Text(JsString::from(text))
    }
}

impl From<String> for TextSource {
    fn from(text: String) -> Self {
        TextSource::Text(JsString::from(text))
    }
}

impl From<JsString> for TextSource {
    fn from(text: JsString) -> Self {
        TextSource::Text(text)
    }
}

impl From<PieceTreeTextBufferFactory> for TextSource {
    fn from(factory: PieceTreeTextBufferFactory) -> Self {
        TextSource::Factory(factory)
    }
}

/// `createTextBufferFactory(text)`.
pub fn create_text_buffer_factory(text: &[u16]) -> PieceTreeTextBufferFactory {
    let mut builder = PieceTreeTextBufferBuilder::new();
    builder.accept_chunk(text);
    builder.finish(true)
}

/// `createTextBufferFactoryFromSnapshot(snapshot)`.
pub fn create_text_buffer_factory_from_snapshot(snapshot: &mut dyn TextSnapshot) -> PieceTreeTextBufferFactory {
    let mut builder = PieceTreeTextBufferBuilder::new();
    while let Some(chunk) = snapshot.read() {
        builder.accept_chunk(chunk.units());
    }
    builder.finish(true)
}

/// `createTextBuffer(value, defaultEOL)`.
pub fn create_text_buffer(value: TextSource, default_eol: DefaultEndOfLine) -> PieceTreeTextBuffer {
    let factory = match value {
        TextSource::Text(text) => create_text_buffer_factory(text.units()),
        TextSource::Snapshot(mut snapshot) => create_text_buffer_factory_from_snapshot(snapshot.as_mut()),
        TextSource::Factory(factory) => factory,
    };
    factory.create(default_eol)
}

const LONG_LINE_BOUNDARY: usize = 10_000;
const LARGE_FILE_SIZE_THRESHOLD: usize = 20 * 1024 * 1024; // 20 MB
const LARGE_FILE_LINE_COUNT_THRESHOLD: usize = 300 * 1000; // 300 mil linhas
const LARGE_FILE_HEAP_OPERATION_THRESHOLD: usize = 256 * 1024 * 1024; // 256 M caracteres, em geral mais de 512 MB de memória
/// `TextModel._MODEL_SYNC_LIMIT`: 50 MB (usado em testes).
pub const MODEL_SYNC_LIMIT: usize = 50 * 1024 * 1024;

thread_local! {
    static MODEL_ID: Cell<u64> = const { Cell::new(0) };
}

/// `TextModelSnapshot`: junta os pedaços do snapshot do buffer em blocos de ao menos 64 KB.
struct TextModelSnapshot {
    source: Box<dyn TextSnapshot>,
    eos: bool,
}

impl TextSnapshot for TextModelSnapshot {
    fn read(&mut self) -> Option<JsString> {
        if self.eos {
            return None;
        }

        let mut result = JsString::new();
        let mut result_count = 0usize;
        loop {
            let Some(tmp) = self.source.read() else {
                // fim do fluxo
                self.eos = true;
                return if result_count == 0 { None } else { Some(result) };
            };

            if !tmp.is_empty() {
                result.push_units(tmp.units());
                result_count += 1;
            }

            if result.len() >= 64 * 1024 {
                return Some(result);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StringOffsetValidationType {
    /// Permitido até no meio de um par substituto.
    Relaxed,
    /// Não permitido no meio de um par substituto.
    SurrogatePairs,
}

/// `IViewModel`, só o que o `TextModel` chama.
pub trait ViewModel {
    fn on_did_change_content_or_injected_text(&self, e: &ContentOrInjectedTextEvent);
    fn emit_content_change_event(&self, e: &ContentOrInjectedTextEvent);
}

/// `InternalModelContentChangeEvent | ModelInjectedTextChangedEvent`.
#[derive(Clone, Debug)]
pub enum ContentOrInjectedTextEvent {
    Content(InternalModelContentChangeEvent),
    InjectedText(ModelInjectedTextChangedEvent),
}

/// `DidChangeContentEmitter`: acumula os eventos enquanto a emissão está adiada e dispara um só, fundido.
struct DidChangeContentEmitter {
    emitter: Rc<Emitter<InternalModelContentChangeEvent>>,
    deferred_cnt: Cell<usize>,
    deferred_event: RefCell<Option<InternalModelContentChangeEvent>>,
}

impl DidChangeContentEmitter {
    fn new() -> Self {
        Self {
            emitter: Emitter::new(),
            deferred_cnt: Cell::new(0),
            deferred_event: RefCell::new(None),
        }
    }

    fn has_listeners(&self) -> bool {
        self.emitter.has_listeners()
    }

    fn begin_deferred_emit(&self) {
        self.deferred_cnt.set(self.deferred_cnt.get() + 1);
    }

    fn end_deferred_emit(&self, resulting_selection: Option<Vec<Selection>>) {
        self.deferred_cnt.set(self.deferred_cnt.get() - 1);
        if self.deferred_cnt.get() == 0 {
            let deferred = self.deferred_event.borrow_mut().take();
            if let Some(mut event) = deferred {
                event.raw_content_changed_event.resulting_selection = resulting_selection;
                self.emitter.fire(event);
            }
        }
    }

    fn fire(&self, e: InternalModelContentChangeEvent) {
        if self.deferred_cnt.get() > 0 {
            let mut deferred = self.deferred_event.borrow_mut();
            *deferred = Some(match deferred.take() {
                Some(previous) => previous.merge(&e),
                None => e,
            });
            return;
        }
        self.emitter.fire(e);
    }
}

/// `LineHeightChangingDecoration`.
#[derive(Clone, Debug, PartialEq)]
pub struct LineHeightChangingDecoration {
    pub owner_id: i32,
    pub decoration_id: String,
    pub line_number: i32,
    pub line_height: Option<f64>,
}

/// `LineFontChangingDecoration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineFontChangingDecoration {
    pub owner_id: i32,
    pub decoration_id: String,
    pub line_number: i32,
}

/// O `SetWithKey` do original: um conjunto ordenado por inserção; repetir a chave troca o valor sem mudar a posição.
struct KeyedSet<T> {
    index: HashMap<String, usize>,
    items: Vec<T>,
}

impl<T> KeyedSet<T> {
    fn new() -> Self {
        Self {
            index: HashMap::new(),
            items: Vec::new(),
        }
    }

    fn add(&mut self, key: String, value: T) {
        match self.index.get(&key) {
            Some(&position) => self.items[position] = value,
            None => {
                self.index.insert(key, self.items.len());
                self.items.push(value);
            }
        }
    }
}

/// O que o emitter de decorações acumulou durante a emissão adiada.
#[derive(Default)]
struct DecorationsEmitterState {
    deferred_cnt: usize,
    should_fire_deferred: bool,
    affects_minimap: bool,
    affects_overview_ruler: bool,
    affects_glyph_margin: bool,
    affects_line_number: bool,
    affected_injected_text_lines: Option<Vec<i32>>,
    affected_line_heights: Option<KeyedSet<LineHeightChangingDecoration>>,
    affected_font_lines: Option<KeyedSet<LineFontChangingDecoration>>,
}

/// `DidChangeDecorationsEmitter`.
struct DidChangeDecorationsEmitter {
    actual: Rc<Emitter<ModelDecorationsChangedEvent>>,
    state: RefCell<DecorationsEmitterState>,
}

/// O que `handleBeforeFire` recebe.
struct AffectedByDecorations {
    injected_text_lines: Option<Vec<i32>>,
    line_heights: Option<Vec<LineHeightChangingDecoration>>,
    font_lines: Option<Vec<LineFontChangingDecoration>>,
}

impl DidChangeDecorationsEmitter {
    fn new() -> Self {
        Self {
            actual: Emitter::new(),
            state: RefCell::new(DecorationsEmitterState::default()),
        }
    }

    fn has_listeners(&self) -> bool {
        self.actual.has_listeners()
    }

    fn begin_deferred_emit(&self) {
        self.state.borrow_mut().deferred_cnt += 1;
    }

    fn end_deferred_emit(&self, model: &TextModel) {
        let fire_now = {
            let mut state = self.state.borrow_mut();
            state.deferred_cnt -= 1;
            state.deferred_cnt == 0 && state.should_fire_deferred
        };
        if fire_now {
            self.do_fire(model);
        }
        let mut state = self.state.borrow_mut();
        if state.deferred_cnt == 0 {
            state.affected_injected_text_lines = None;
            state.affected_line_heights = None;
            state.affected_font_lines = None;
        }
    }

    fn record_line_affected_by_injected_text(&self, line_number: i32) {
        let mut state = self.state.borrow_mut();
        let lines = state.affected_injected_text_lines.get_or_insert_with(Vec::new);
        if !lines.contains(&line_number) {
            lines.push(line_number);
        }
    }

    fn record_line_affected_by_line_height_change(&self, owner_id: i32, decoration_id: &str, line_number: i32, line_height: Option<f64>) {
        let mut state = self.state.borrow_mut();
        state.affected_line_heights.get_or_insert_with(KeyedSet::new).add(
            format!("{owner_id};{decoration_id};{line_number}"),
            LineHeightChangingDecoration {
                owner_id,
                decoration_id: decoration_id.to_owned(),
                line_number,
                line_height,
            },
        );
    }

    fn record_line_affected_by_font_change(&self, owner_id: i32, decoration_id: &str, line_number: i32) {
        let mut state = self.state.borrow_mut();
        state.affected_font_lines.get_or_insert_with(KeyedSet::new).add(
            format!("{owner_id};{decoration_id};{line_number}"),
            LineFontChangingDecoration {
                owner_id,
                decoration_id: decoration_id.to_owned(),
                line_number,
            },
        );
    }

    fn check_affected_and_fire(&self, model: &TextModel, options: &ModelDecorationOptions) {
        {
            let mut state = self.state.borrow_mut();
            state.affects_minimap |= options.minimap.as_ref().is_some();
            state.affects_overview_ruler |= options.overview_ruler.as_ref().is_some_and(|r| r.has_color());
            state.affects_glyph_margin |= options.glyph_margin_class_name.is_some();
            state.affects_line_number |= options.line_number_class_name.is_some();
        }
        self.try_fire(model);
    }

    fn fire(&self, model: &TextModel) {
        {
            let mut state = self.state.borrow_mut();
            state.affects_minimap = true;
            state.affects_overview_ruler = true;
            state.affects_glyph_margin = true;
        }
        self.try_fire(model);
    }

    fn try_fire(&self, model: &TextModel) {
        let deferred = self.state.borrow().deferred_cnt != 0;
        if deferred {
            self.state.borrow_mut().should_fire_deferred = true;
        } else {
            self.do_fire(model);
        }
    }

    fn do_fire(&self, model: &TextModel) {
        let affected = {
            let state = self.state.borrow();
            AffectedByDecorations {
                injected_text_lines: state.affected_injected_text_lines.clone(),
                line_heights: state.affected_line_heights.as_ref().map(|set| set.items.clone()),
                font_lines: state.affected_font_lines.as_ref().map(|set| set.items.clone()),
            }
        };
        model.handle_before_fire_decorations_changed_event(&affected);

        let event = {
            let mut state = self.state.borrow_mut();
            let event = ModelDecorationsChangedEvent {
                affects_minimap: state.affects_minimap,
                affects_overview_ruler: state.affects_overview_ruler,
                affects_glyph_margin: state.affects_glyph_margin,
                affects_line_number: state.affects_line_number,
            };
            state.should_fire_deferred = false;
            state.affects_minimap = false;
            state.affects_overview_ruler = false;
            state.affects_glyph_margin = false;
            event
        };
        self.actual.fire(event);
    }
}

// ---------------------------------------------------------------------------------------------------------
// Árvores de decorações
// ---------------------------------------------------------------------------------------------------------

/// `IDecorationsTreesHost`: o que as árvores precisam do modelo.
trait DecorationsTreesHost {
    fn get_version_id(&self) -> i64;
    fn get_range_at(&self, start: i64, end: i64) -> Range;
}

fn is_node_in_overview_ruler(node: &IntervalNode) -> bool {
    node.options.as_ref().is_some_and(|options| options.in_overview_ruler())
}

fn is_node_injected_text(node: &IntervalNode) -> bool {
    node.options.as_ref().is_some_and(|options| options.is_injected_text())
}

/// `DecorationsTrees`: três árvores sobre uma arena só. A 0 guarda as decorações que não aparecem na régua
/// de visão geral, a 1 as que aparecem, a 2 as que injetam texto.
struct DecorationsTrees {
    arena: IntervalArena,
    tree0: IntervalTree,
    tree1: IntervalTree,
    injected_text_tree: IntervalTree,
}

impl DecorationsTrees {
    fn new() -> Self {
        Self {
            arena: IntervalArena::new(),
            tree0: IntervalTree::new(),
            tree1: IntervalTree::new(),
            injected_text_tree: IntervalTree::new(),
        }
    }

    /// A árvore a que o nó pertence, junto da arena (os dois campos são disjuntos, por isso a tupla).
    fn tree_and_arena(&mut self, node: NodeId) -> (&mut IntervalTree, &mut IntervalArena) {
        let node = self.arena.get(node);
        let tree = if is_node_injected_text(node) {
            &mut self.injected_text_tree
        } else if is_node_in_overview_ruler(node) {
            &mut self.tree1
        } else {
            &mut self.tree0
        };
        (tree, &mut self.arena)
    }

    fn ensure_nodes_have_ranges(&mut self, host: &dyn DecorationsTreesHost, nodes: &[NodeId]) {
        for &id in nodes {
            let node = self.arena.get_mut(id);
            if node.range.is_none() {
                node.range = Some(host.get_range_at(node.cached_absolute_start, node.cached_absolute_end));
            }
        }
    }

    fn to_decorations(&self, nodes: &[NodeId]) -> Vec<ModelDecoration> {
        nodes
            .iter()
            .map(|&id| {
                let node = self.arena.get(id);
                ModelDecoration {
                    id: node.id.clone(),
                    owner_id: node.owner_id,
                    range: node.range.expect("o intervalo foi resolvido por ensure_nodes_have_ranges"),
                    options: node.options.clone().expect("todo nó da árvore tem opções"),
                }
            })
            .collect()
    }

    fn resolved(&mut self, host: &dyn DecorationsTreesHost, nodes: Vec<NodeId>) -> Vec<NodeId> {
        self.ensure_nodes_have_ranges(host, &nodes);
        nodes
    }

    fn ensure_all_nodes_have_ranges(&mut self, host: &dyn DecorationsTreesHost) {
        self.get_all(host, 0, false, false, false, false);
    }

    #[allow(clippy::too_many_arguments)]
    fn get_all_in_interval(
        &mut self,
        host: &dyn DecorationsTreesHost,
        start: i64,
        end: i64,
        filter_owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        only_margin_decorations: bool,
    ) -> Vec<ModelDecoration> {
        let version_id = host.get_version_id();
        let result = self.interval_search(
            start,
            end,
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            version_id,
            only_margin_decorations,
        );
        let result = self.resolved(host, result);
        self.to_decorations(&result)
    }

    #[allow(clippy::too_many_arguments)]
    fn interval_search(
        &mut self,
        start: i64,
        end: i64,
        filter_owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        cached_version_id: i64,
        only_margin_decorations: bool,
    ) -> Vec<NodeId> {
        let arena = &mut self.arena;
        let mut result = self.tree0.interval_search(
            arena,
            start,
            end,
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            cached_version_id,
            only_margin_decorations,
        );
        result.extend(self.tree1.interval_search(
            arena,
            start,
            end,
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            cached_version_id,
            only_margin_decorations,
        ));
        result.extend(self.injected_text_tree.interval_search(
            arena,
            start,
            end,
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            cached_version_id,
            only_margin_decorations,
        ));
        result
    }

    fn get_injected_text_in_interval(&mut self, host: &dyn DecorationsTreesHost, start: i64, end: i64, filter_owner_id: i32) -> Vec<ModelDecoration> {
        let version_id = host.get_version_id();
        let result = self
            .injected_text_tree
            .interval_search(&mut self.arena, start, end, filter_owner_id, false, false, version_id, false);
        let result = self.resolved(host, result);
        self.to_decorations(&result)
            .into_iter()
            .filter(|decoration| decoration.options.show_if_collapsed || !decoration.range.is_empty())
            .collect()
    }

    fn get_font_decorations_in_interval(&mut self, host: &dyn DecorationsTreesHost, start: i64, end: i64, filter_owner_id: i32) -> Vec<ModelDecoration> {
        let version_id = host.get_version_id();
        let decorations = self
            .tree0
            .interval_search(&mut self.arena, start, end, filter_owner_id, false, false, version_id, false);
        let decorations = self.resolved(host, decorations);
        self.to_decorations(&decorations)
            .into_iter()
            .filter(|decoration| decoration.options.affects_font.unwrap_or(false))
            .collect()
    }

    fn get_all_injected_text(&mut self, host: &dyn DecorationsTreesHost, filter_owner_id: i32) -> Vec<ModelDecoration> {
        let version_id = host.get_version_id();
        let result = self
            .injected_text_tree
            .search(&mut self.arena, filter_owner_id, false, false, version_id, false);
        let result = self.resolved(host, result);
        self.to_decorations(&result)
            .into_iter()
            .filter(|decoration| decoration.options.show_if_collapsed || !decoration.range.is_empty())
            .collect()
    }

    fn get_all_custom_line_heights(&mut self, host: &dyn DecorationsTreesHost, filter_owner_id: i32) -> Vec<ModelDecoration> {
        let version_id = host.get_version_id();
        let result = self.search(filter_owner_id, false, false, false, version_id, false);
        let result = self.resolved(host, result);
        self.to_decorations(&result)
            .into_iter()
            .filter(|decoration| decoration.options.line_height.is_some())
            .collect()
    }

    fn get_custom_line_heights_in_interval(&mut self, host: &dyn DecorationsTreesHost, start: i64, end: i64, filter_owner_id: i32) -> Vec<ModelDecoration> {
        let version_id = host.get_version_id();
        let result = self.interval_search(start, end, filter_owner_id, false, false, version_id, false);
        let result = self.resolved(host, result);
        self.to_decorations(&result)
            .into_iter()
            .filter(|decoration| decoration.options.line_height.is_some())
            .collect()
    }

    fn get_all(
        &mut self,
        host: &dyn DecorationsTreesHost,
        filter_owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        overview_ruler_only: bool,
        only_margin_decorations: bool,
    ) -> Vec<ModelDecoration> {
        let version_id = host.get_version_id();
        let result = self.search(
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            overview_ruler_only,
            version_id,
            only_margin_decorations,
        );
        let result = self.resolved(host, result);
        self.to_decorations(&result)
    }

    fn search(
        &mut self,
        filter_owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        overview_ruler_only: bool,
        cached_version_id: i64,
        only_margin_decorations: bool,
    ) -> Vec<NodeId> {
        let arena = &mut self.arena;
        if overview_ruler_only {
            return self.tree1.search(
                arena,
                filter_owner_id,
                filter_out_validation,
                filter_font_decorations,
                cached_version_id,
                only_margin_decorations,
            );
        }
        let mut result = self.tree0.search(
            arena,
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            cached_version_id,
            only_margin_decorations,
        );
        result.extend(self.tree1.search(
            arena,
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            cached_version_id,
            only_margin_decorations,
        ));
        result.extend(self.injected_text_tree.search(
            arena,
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            cached_version_id,
            only_margin_decorations,
        ));
        result
    }

    fn collect_nodes_from_owner(&self, owner_id: i32) -> Vec<NodeId> {
        let mut result = self.tree0.collect_nodes_from_owner(&self.arena, owner_id);
        result.extend(self.tree1.collect_nodes_from_owner(&self.arena, owner_id));
        result.extend(self.injected_text_tree.collect_nodes_from_owner(&self.arena, owner_id));
        result
    }

    fn collect_nodes_post_order(&self) -> Vec<NodeId> {
        let mut result = self.tree0.collect_nodes_post_order(&self.arena);
        result.extend(self.tree1.collect_nodes_post_order(&self.arena));
        result.extend(self.injected_text_tree.collect_nodes_post_order(&self.arena));
        result
    }

    fn insert(&mut self, node: NodeId) {
        let (tree, arena) = self.tree_and_arena(node);
        tree.insert(arena, node);
    }

    fn delete(&mut self, node: NodeId) {
        let (tree, arena) = self.tree_and_arena(node);
        tree.delete(arena, node);
    }

    fn get_node_range(&mut self, host: &dyn DecorationsTreesHost, node: NodeId) -> Range {
        let version_id = host.get_version_id();
        if self.arena.get(node).cached_version_id != version_id {
            self.resolve_node(node, version_id);
        }
        let n = self.arena.get_mut(node);
        if n.range.is_none() {
            n.range = Some(host.get_range_at(n.cached_absolute_start, n.cached_absolute_end));
        }
        n.range.expect("o intervalo acabou de ser calculado")
    }

    fn resolve_node(&mut self, node: NodeId, cached_version_id: i64) {
        let (tree, arena) = self.tree_and_arena(node);
        tree.resolve_node(arena, node, cached_version_id);
    }

    fn accept_replace(&mut self, offset: i64, length: i64, text_length: i64, force_move_markers: bool) {
        let arena = &mut self.arena;
        self.tree0.accept_replace(arena, offset, length, text_length, force_move_markers);
        self.tree1.accept_replace(arena, offset, length, text_length, force_move_markers);
        self.injected_text_tree
            .accept_replace(arena, offset, length, text_length, force_move_markers);
    }
}

// ---------------------------------------------------------------------------------------------------------
// TextModel
// ---------------------------------------------------------------------------------------------------------

/// As decorações do modelo: o mapa de id para nó e as três árvores.
struct DecorationsState {
    /// `_decorations`.
    nodes: HashMap<String, NodeId>,
    trees: DecorationsTrees,
}

impl DecorationsState {
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            trees: DecorationsTrees::new(),
        }
    }
}

/// `TRACKED_RANGE_OPTIONS`: a ordem casa com os valores do enum.
fn tracked_range_options(stickiness: TrackedRangeStickiness) -> Rc<ModelDecorationOptions> {
    let description = match stickiness {
        TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges => "tracked-range-always-grows-when-typing-at-edges",
        TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges => "tracked-range-never-grows-when-typing-at-edges",
        TrackedRangeStickiness::GrowsOnlyWhenTypingBefore => "tracked-range-grows-only-when-typing-before",
        TrackedRangeStickiness::GrowsOnlyWhenTypingAfter => "tracked-range-grows-only-when-typing-after",
    };
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: description.to_owned(),
        stickiness: Some(stickiness),
        ..Default::default()
    })
}

/// Os serviços de linguagem que o `TextModel` recebe por injeção.
#[derive(Clone)]
pub struct TextModelServices {
    pub language_service: Rc<dyn ILanguageService>,
    pub language_configuration_service: Rc<dyn ILanguageConfigurationService>,
}

/// O `TextModel`.
pub struct TextModel {
    me: Weak<TextModel>,
    /// A base `Disposable` do original: dona dos emitters.
    base: Rc<Disposable>,

    id: String,
    is_for_simple_widget: bool,
    associated_resource: URI,
    attached_editor_count: Cell<usize>,
    buffer: RefCell<PieceTreeTextBuffer>,
    buffer_disposable: RefCell<DisposableRef>,
    options: Cell<TextModelResolvedOptions>,
    language_service: Rc<dyn ILanguageService>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    /// Os serviços criados pelo próprio modelo (quando ninguém os injetou), descartados com ele.
    owned_services: Option<(Rc<LanguageService>, Rc<LanguageConfigurationService>)>,
    attached_views: Rc<AttachedViews>,
    on_did_change_attached: Rc<Emitter<()>>,
    tokenization: OnceCell<Rc<TokenizationTextModelPart>>,
    bracket_pairs: Rc<BracketPairsTextModelPart>,
    guides: GuidesTextModelPart,
    decoration_provider: Rc<ColorizedBracketPairsDecorationProvider>,

    is_disposed: Cell<bool>,
    is_disposing: Cell<bool>,
    version_id: Cell<i64>,
    /// Ao contrário do `versionId`, este pode baixar (desfazer) ou voltar a valores antigos (refazer).
    alternative_version_id: Cell<i64>,
    initial_undo_redo_snapshot: RefCell<Option<ResourceEditStackSnapshot>>,
    is_too_large_for_syncing: bool,
    is_too_large_for_tokenization: bool,
    is_too_large_for_heap_operation: bool,

    undo_redo_service: Rc<UndoRedoService>,
    command_manager: EditStack,
    is_undoing: Cell<bool>,
    is_redoing: Cell<bool>,
    trim_auto_whitespace_lines: RefCell<Option<Vec<i32>>>,

    /// Contorna clientes quebrados que usam o id de uma decoração gerado por outro modelo: não é único
    /// no mundo, só limita o prefixo a um caractere.
    instance_id: String,
    delta_decoration_call_cnt: Cell<i32>,
    last_decoration_id: Cell<u64>,
    decorations: RefCell<DecorationsState>,

    view_models: RefCell<Vec<Rc<dyn ViewModel>>>,

    on_will_dispose: Rc<Emitter<()>>,
    on_did_change_decorations: DidChangeDecorationsEmitter,
    on_did_change_options: Rc<Emitter<ModelOptionsChangedEvent>>,
    on_did_change_line_height: Rc<Emitter<ModelLineHeightChangedEvent>>,
    on_did_change_font: Rc<Emitter<ModelFontChangedEvent>>,
    event_emitter: DidChangeContentEmitter,
}

impl DecorationsTreesHost for TextModel {
    fn get_version_id(&self) -> i64 {
        self.version_id.get()
    }

    fn get_range_at(&self, start: i64, end: i64) -> Range {
        self.buffer.borrow().get_range_at(start as usize, (end - start) as usize)
    }
}

impl TextModel {
    /// `TextModel.DEFAULT_CREATION_OPTIONS`.
    pub fn default_creation_options() -> TextModelCreationOptions {
        TextModelCreationOptions {
            is_for_simple_widget: false,
            tab_size: EDITOR_MODEL_DEFAULTS.tab_size,
            indent_size: IndentSize::Size(EDITOR_MODEL_DEFAULTS.indent_size),
            insert_spaces: EDITOR_MODEL_DEFAULTS.insert_spaces,
            detect_indentation: false,
            default_eol: DefaultEndOfLine::Lf,
            trim_auto_whitespace: EDITOR_MODEL_DEFAULTS.trim_auto_whitespace,
            large_file_optimizations: EDITOR_MODEL_DEFAULTS.large_file_optimizations,
            bracket_pair_colorization_options: EDITOR_MODEL_DEFAULTS.bracket_pair_colorization_options,
        }
    }

    /// `TextModel.resolveOptions(textBuffer, options)`.
    pub fn resolve_options(text_buffer: &PieceTreeTextBuffer, options: &TextModelCreationOptions) -> TextModelResolvedOptions {
        if options.detect_indentation {
            let guessed = guess_indentation(text_buffer, options.tab_size, options.insert_spaces);
            return TextModelResolvedOptions::new(
                guessed.tab_size,
                IndentSize::TabSize,
                guessed.insert_spaces,
                options.default_eol,
                options.trim_auto_whitespace,
                options.bracket_pair_colorization_options,
            );
        }
        TextModelResolvedOptions::new(
            options.tab_size,
            options.indent_size,
            options.insert_spaces,
            options.default_eol,
            options.trim_auto_whitespace,
            options.bracket_pair_colorization_options,
        )
    }

    /// O construtor com um `ILanguageService` próprio do modelo (criado e descartado com ele).
    pub fn new(
        source: TextSource,
        language_id: &str,
        creation_options: TextModelCreationOptions,
        associated_resource: Option<URI>,
        undo_redo_service: Rc<UndoRedoService>,
    ) -> Rc<TextModel> {
        Self::new_with_language_service(source, language_id, creation_options, associated_resource, undo_redo_service, None)
    }

    /// O construtor com os serviços de linguagem injetados (`ILanguageService` e
    /// `ILanguageConfigurationService`). Sem eles, o modelo cria os seus e os descarta junto.
    pub fn new_with_language_service(
        source: TextSource,
        language_id: &str,
        creation_options: TextModelCreationOptions,
        associated_resource: Option<URI>,
        undo_redo_service: Rc<UndoRedoService>,
        services: Option<TextModelServices>,
    ) -> Rc<TextModel> {
        let (language_service, language_configuration_service, owned_services) = match services {
            Some(services) => (services.language_service, services.language_configuration_service, None),
            None => {
                let language_service = LanguageService::new(false);
                let configuration_service = LanguageConfigurationService::new(language_service.clone());
                (
                    language_service.clone() as Rc<dyn ILanguageService>,
                    configuration_service.clone() as Rc<dyn ILanguageConfigurationService>,
                    Some((language_service, configuration_service)),
                )
            }
        };
        // Gera um id novo e único para o modelo
        let model_id = MODEL_ID.with(|counter| {
            counter.set(counter.get() + 1);
            counter.get()
        });
        let associated_resource =
            associated_resource.unwrap_or_else(|| URI::parse(format!("inmemory://model/{model_id}"), false));

        let buffer = create_text_buffer(source, creation_options.default_eol);
        let buffer_disposable = buffer.disposable();

        let buffer_line_count = buffer.get_line_count() as i32;
        let buffer_text_length = buffer.get_value_length_in_range(
            &Range::new(1, 1, buffer_line_count, buffer.get_line_length(buffer_line_count) as i32 + 1),
            EndOfLinePreference::TextDefined,
        );

        // !!! Decide no construtor e respeita a decisão para sempre !!!
        // Um modelo grande demais na criação nunca será tokenizado, em hipótese alguma.
        let (is_too_large_for_tokenization, is_too_large_for_heap_operation) = if creation_options.large_file_optimizations {
            (
                buffer_text_length > LARGE_FILE_SIZE_THRESHOLD || buffer_line_count as usize > LARGE_FILE_LINE_COUNT_THRESHOLD,
                buffer_text_length > LARGE_FILE_HEAP_OPERATION_THRESHOLD,
            )
        } else {
            (false, false)
        };

        let options = Self::resolve_options(&buffer, &creation_options);

        let base = Disposable::new();
        let on_will_dispose = Emitter::new();
        let on_did_change_options = Emitter::new();
        let on_did_change_line_height = Emitter::new();
        let on_did_change_font = Emitter::new();
        let on_did_change_decorations = DidChangeDecorationsEmitter::new();
        let event_emitter = DidChangeContentEmitter::new();
        base.register(on_will_dispose.clone());
        base.register(on_did_change_options.clone());
        base.register(on_did_change_line_height.clone());
        base.register(on_did_change_font.clone());
        base.register(on_did_change_decorations.actual.clone());
        base.register(event_emitter.emitter.clone());
        let on_did_change_attached = Emitter::new();
        base.register(on_did_change_attached.clone());
        let attached_views = AttachedViews::new();
        base.register(attached_views.clone());

        let model = Rc::new_cyclic(|me| TextModel {
            me: me.clone(),
            base,
            id: format!("$model{model_id}"),
            is_for_simple_widget: creation_options.is_for_simple_widget,
            associated_resource,
            attached_editor_count: Cell::new(0),
            buffer: RefCell::new(buffer),
            buffer_disposable: RefCell::new(buffer_disposable),
            options: Cell::new(options),
            language_service: language_service.clone(),
            language_configuration_service: language_configuration_service.clone(),
            owned_services,
            attached_views: attached_views.clone(),
            on_did_change_attached,
            tokenization: OnceCell::new(),
            bracket_pairs: BracketPairsTextModelPart::new(me.clone(), language_configuration_service.clone()),
            guides: GuidesTextModelPart::new(me.clone(), language_configuration_service.clone()),
            decoration_provider: ColorizedBracketPairsDecorationProvider::new(me.clone(), options.bracket_pair_colorization_options),
            is_disposed: Cell::new(false),
            is_disposing: Cell::new(false),
            version_id: Cell::new(1),
            alternative_version_id: Cell::new(1),
            initial_undo_redo_snapshot: RefCell::new(None),
            is_too_large_for_syncing: buffer_text_length > MODEL_SYNC_LIMIT,
            is_too_large_for_tokenization,
            is_too_large_for_heap_operation,
            command_manager: EditStack::new(me.clone(), undo_redo_service.clone()),
            undo_redo_service,
            is_undoing: Cell::new(false),
            is_redoing: Cell::new(false),
            trim_auto_whitespace_lines: RefCell::new(None),
            instance_id: strings::single_letter_hash(model_id as usize).to_string_lossy(),
            delta_decoration_call_cnt: Cell::new(0),
            last_decoration_id: Cell::new(0),
            decorations: RefCell::new(DecorationsState::new()),
            view_models: RefCell::new(Vec::new()),
            on_will_dispose,
            on_did_change_decorations,
            on_did_change_options,
            on_did_change_line_height,
            on_did_change_font,
            event_emitter,
        });
        let configuration_for_words = language_configuration_service.clone();
        let word_definition: WordDefinitionProvider =
            Rc::new(move |language_id| configuration_for_words.get_language_configuration(language_id).get_word_definition());
        let part = TokenizationTextModelPart::new(model.me.clone(), language_id, &attached_views, &language_service, word_definition);
        let _ = model.tokenization.set(part);
        language_service.request_rich_language_features(language_id);

        model.base.register(model.bracket_pairs.clone());
        model.base.register(model.decoration_provider.clone());
        let provider = model.decoration_provider.clone();
        model.base.register(model.bracket_pairs.on_did_change().listen(move |_| provider.fire_did_change()));
        let weak = model.me.clone();
        model.base.register(model.decoration_provider.on_did_change().listen(move |_| {
            if let Some(model) = weak.upgrade() {
                model.on_did_change_decorations.begin_deferred_emit();
                model.on_did_change_decorations.fire(&model);
                model.on_did_change_decorations.end_deferred_emit(&model);
            }
        }));

        let weak = model.me.clone();
        model.base.register(language_configuration_service.on_did_change().listen(move |e| {
            if let Some(model) = weak.upgrade() {
                model.bracket_pairs.handle_language_configuration_service_change(e);
                model.tokenization().handle_language_configuration_service_change(|language_id| e.affects(language_id));
            }
        }));
        model
    }

    /// O `Weak` do próprio modelo (o elemento de desfazer o guarda).
    pub fn downgrade(&self) -> Weak<TextModel> {
        self.me.clone()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn is_for_simple_widget(&self) -> bool {
        self.is_for_simple_widget
    }

    /// `uri`.
    pub fn uri(&self) -> URI {
        self.associated_resource.clone()
    }

    // #region Eventos

    pub fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    pub fn on_did_change_decorations(&self) -> Event<ModelDecorationsChangedEvent> {
        self.on_did_change_decorations.actual.event()
    }

    pub fn on_did_change_options(&self) -> Event<ModelOptionsChangedEvent> {
        self.on_did_change_options.event()
    }

    pub fn on_did_change_line_height(&self) -> Event<ModelLineHeightChangedEvent> {
        self.on_did_change_line_height.event()
    }

    pub fn on_did_change_font(&self) -> Event<ModelFontChangedEvent> {
        self.on_did_change_font.event()
    }

    /// `onDidChangeContent(listener)`.
    pub fn on_did_change_content(&self) -> Event<ModelContentChangedEvent> {
        let inner = self.event_emitter.emitter.event();
        Event::new(move |listener: Listener<ModelContentChangedEvent>| inner.listen(move |e: &InternalModelContentChangeEvent| listener(&e.content_changed_event)))
    }

    /// O evento com a parte crua (o `InternalModelContentChangeEvent` que os view models recebem).
    pub fn on_did_change_content_internal(&self) -> Event<InternalModelContentChangeEvent> {
        self.event_emitter.emitter.event()
    }

    // #endregion

    pub fn dispose(&self) {
        self.is_disposing.set(true);
        self.on_will_dispose.fire(());
        IDisposable::dispose(&**self.tokenization());
        self.is_disposed.set(true);
        IDisposable::dispose(&*self.base);
        if let Some((language_service, configuration_service)) = &self.owned_services {
            IDisposable::dispose(&**configuration_service);
            IDisposable::dispose(&**language_service);
        }
        self.buffer_disposable.borrow().dispose();
        self.is_disposing.set(false);
        // Solta a referência ao buffer anterior para evitar vazamento grande caso alguém retenha o modelo
        let empty_disposed_text_buffer = PieceTreeTextBuffer::new(Vec::new(), JsString::new(), Eol::Lf, false, false, true, true);
        empty_disposed_text_buffer.dispose();
        *self.buffer_disposable.borrow_mut() = empty_disposed_text_buffer.disposable();
        *self.buffer.borrow_mut() = empty_disposed_text_buffer;
    }

    /// `_isDisposing()`.
    pub fn is_disposing(&self) -> bool {
        self.is_disposing.get()
    }

    /// `_hasListeners()`.
    pub fn has_listeners(&self) -> bool {
        self.on_will_dispose.has_listeners()
            || self.on_did_change_decorations.has_listeners()
            || self.tokenization().has_listeners()
            || self.on_did_change_options.has_listeners()
            || self.on_did_change_attached.has_listeners()
            || self.on_did_change_line_height.has_listeners()
            || self.on_did_change_font.has_listeners()
            || self.event_emitter.has_listeners()
    }

    pub(crate) fn assert_not_disposed(&self) {
        if self.is_disposed.get() {
            errors::throw(Error::bug_indicating(Some("Model is disposed!")));
        }
    }

    pub fn register_view_model(&self, view_model: Rc<dyn ViewModel>) {
        self.view_models.borrow_mut().push(view_model);
    }

    pub fn unregister_view_model(&self, view_model: &Rc<dyn ViewModel>) {
        self.view_models.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, view_model));
    }

    /// `equalsTextBuffer(other)`.
    pub fn equals_text_buffer(&self, other: &PieceTreeTextBuffer) -> bool {
        self.assert_not_disposed();
        self.buffer.borrow().equals(other)
    }

    /// `getTextBuffer()`: empresta o buffer para leitura.
    pub fn get_text_buffer(&self) -> Ref<'_, PieceTreeTextBuffer> {
        self.assert_not_disposed();
        self.buffer.borrow()
    }

    fn emit_content_changed_event(&self, raw_change: ModelRawContentChangedEvent, change: ModelContentChangedEvent, resulting_selection: Option<Vec<Selection>>) {
        if self.is_disposing.get() {
            // Não confunde os listeners emitindo evento depois de descartar
            return;
        }
        self.tokenization().handle_did_change_content(&change);
        self.bracket_pairs.handle_did_change_content(&change);
        let mut content_change_event = InternalModelContentChangeEvent {
            raw_content_changed_event: raw_change,
            content_changed_event: change,
        };
        // Define `resultingSelection` cedo para os view models posicionarem o cursor
        if let Some(selection) = resulting_selection {
            content_change_event.raw_content_changed_event.resulting_selection = Some(selection);
        }
        self.on_did_change_content_or_injected_text(&ContentOrInjectedTextEvent::Content(content_change_event.clone()));
        self.event_emitter.fire(content_change_event);
    }

    /// `setValue(value, reason)`.
    pub fn set_value(&self, value: TextSource) {
        self.set_value_with_reason(value, EditSources::set_value());
    }

    pub fn set_value_with_reason(&self, value: TextSource, reason: TextModelEditSource) {
        self.assert_not_disposed();
        let text_buffer = create_text_buffer(value, self.options.get().default_eol);
        self.set_value_from_text_buffer(text_buffer, reason);
    }

    #[allow(clippy::too_many_arguments)]
    fn create_content_changed2(
        &self,
        range: Range,
        range_offset: usize,
        range_length: usize,
        text: JsString,
        is_undoing: bool,
        is_redoing: bool,
        is_flush: bool,
        is_eol_change: bool,
        reason: TextModelEditSource,
    ) -> ModelContentChangedEvent {
        ModelContentChangedEvent {
            changes: vec![ModelContentChange {
                range,
                range_offset,
                range_length,
                text,
            }],
            eol: self.buffer.borrow().get_eol(),
            is_eol_change,
            version_id: self.get_version_id(),
            is_undoing,
            is_redoing,
            is_flush,
            detailed_reasons: vec![reason],
            detailed_reasons_change_lengths: vec![1],
        }
    }

    fn set_value_from_text_buffer(&self, text_buffer: PieceTreeTextBuffer, reason: TextModelEditSource) {
        self.assert_not_disposed();
        let old_full_model_range = self.get_full_model_range();
        let old_model_value_length = self.get_value_length_in_range(&old_full_model_range);
        let end_line_number = self.get_line_count();
        let end_column = self.get_line_max_column(end_line_number);

        let text_buffer_disposable = text_buffer.disposable();
        *self.buffer.borrow_mut() = text_buffer;
        std::mem::replace(&mut *self.buffer_disposable.borrow_mut(), text_buffer_disposable).dispose();
        self.increase_version_id();

        // Destrói todas as minhas decorações
        *self.decorations.borrow_mut() = DecorationsState::new();

        // Destrói meu histórico de edição e as configurações
        self.command_manager.clear();
        *self.trim_auto_whitespace_lines.borrow_mut() = None;

        let value = self.get_value();
        self.emit_content_changed_event(
            ModelRawContentChangedEvent::new(vec![ModelRawChange::Flush], self.version_id.get(), false, false),
            self.create_content_changed2(
                Range::new(1, 1, end_line_number, end_column),
                0,
                old_model_value_length,
                value,
                false,
                false,
                true,
                false,
                reason,
            ),
            None,
        );
    }

    /// `setEOL(eol)`.
    pub fn set_eol(&self, eol: EndOfLineSequence) {
        self.assert_not_disposed();
        let new_eol = eol.eol();
        if self.buffer.borrow().get_eol() == new_eol {
            // Nada a fazer
            return;
        }

        let old_full_model_range = self.get_full_model_range();
        let old_model_value_length = self.get_value_length_in_range(&old_full_model_range);
        let end_line_number = self.get_line_count();
        let end_column = self.get_line_max_column(end_line_number);

        self.on_before_eol_change();
        self.buffer.borrow_mut().set_eol(new_eol);
        self.increase_version_id();
        self.on_after_eol_change();

        let value = self.get_value();
        self.emit_content_changed_event(
            ModelRawContentChangedEvent::new(vec![ModelRawChange::EolChanged], self.version_id.get(), false, false),
            self.create_content_changed2(
                Range::new(1, 1, end_line_number, end_column),
                0,
                old_model_value_length,
                value,
                false,
                false,
                false,
                true,
                EditSources::eol_change(),
            ),
            None,
        );
    }

    fn on_before_eol_change(&self) {
        // Garante que todas as decorações tenham o `range` definido
        self.decorations.borrow_mut().trees.ensure_all_nodes_have_ranges(self);
    }

    fn on_after_eol_change(&self) {
        // Transforma de volta o `range` em deslocamentos
        let version_id = self.get_version_id();
        let mut decorations = self.decorations.borrow_mut();
        let all_decorations = decorations.trees.collect_nodes_post_order();
        let buffer = self.buffer.borrow();
        for id in all_decorations {
            let node = decorations.trees.arena.get_mut(id);
            let range = node.range.expect("o intervalo existe por causa de on_before_eol_change");

            let delta = node.cached_absolute_start - node.start;

            let start_offset = buffer.get_offset_at(range.start_line_number, range.start_column) as i64;
            let end_offset = buffer.get_offset_at(range.end_line_number, range.end_column) as i64;

            node.cached_absolute_start = start_offset;
            node.cached_absolute_end = end_offset;
            node.cached_version_id = version_id;

            node.start = start_offset - delta;
            node.end = end_offset - delta;

            recompute_max_end(&mut decorations.trees.arena, id);
        }
    }

    pub fn is_attached_to_editor(&self) -> bool {
        self.attached_editor_count.get() > 0
    }

    pub fn get_attached_editor_count(&self) -> usize {
        self.attached_editor_count.get()
    }

    pub fn is_too_large_for_syncing(&self) -> bool {
        self.is_too_large_for_syncing
    }

    pub fn is_too_large_for_tokenization(&self) -> bool {
        self.is_too_large_for_tokenization
    }

    pub fn is_too_large_for_heap_operation(&self) -> bool {
        self.is_too_large_for_heap_operation
    }

    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    /// `isDominatedByLongLines()`.
    pub fn is_dominated_by_long_lines(&self) -> bool {
        self.assert_not_disposed();
        if self.is_too_large_for_tokenization() {
            // Não dá para quebrar linha em arquivo enorme mesmo, então tanto faz
            return false;
        }
        let mut small_line_char_count = 0usize;
        let mut long_line_char_count = 0usize;

        let buffer = self.buffer.borrow();
        let line_count = buffer.get_line_count() as i32;
        for line_number in 1..=line_count {
            let line_length = buffer.get_line_length(line_number);
            if line_length >= LONG_LINE_BOUNDARY {
                long_line_char_count += line_length;
            } else {
                small_line_char_count += line_length;
            }
        }

        long_line_char_count > small_line_char_count
    }

    // #region Opções

    pub fn get_options(&self) -> TextModelResolvedOptions {
        self.assert_not_disposed();
        self.options.get()
    }

    /// `updateOptions(newOpts)`.
    pub fn update_options(&self, new_opts: TextModelUpdateOptions) {
        self.assert_not_disposed();
        let current = self.options.get();
        let tab_size = new_opts.tab_size.unwrap_or(current.tab_size);
        let indent_size = new_opts.indent_size.unwrap_or_else(|| current.original_indent_size());
        let insert_spaces = new_opts.insert_spaces.unwrap_or(current.insert_spaces);
        let trim_auto_whitespace = new_opts.trim_auto_whitespace.unwrap_or(current.trim_auto_whitespace);
        let bracket_pair_colorization_options = new_opts
            .bracket_colorization_options
            .unwrap_or(current.bracket_pair_colorization_options);

        let new_opts = TextModelResolvedOptions::new(
            tab_size,
            indent_size,
            insert_spaces,
            current.default_eol,
            trim_auto_whitespace,
            bracket_pair_colorization_options,
        );

        if current == new_opts {
            return;
        }

        let e = current.create_change_event(&new_opts);
        self.options.set(new_opts);

        self.bracket_pairs.handle_did_change_options(&e);
        self.decoration_provider.handle_did_change_options(&e);
        self.on_did_change_options.fire(e);
    }

    /// `detectIndentation(defaultInsertSpaces, defaultTabSize)`.
    pub fn detect_indentation(&self, default_insert_spaces: bool, default_tab_size: i32) {
        self.assert_not_disposed();
        let guessed = guess_indentation(&self.buffer.borrow(), default_tab_size, default_insert_spaces);
        self.update_options(TextModelUpdateOptions {
            insert_spaces: Some(guessed.insert_spaces),
            tab_size: Some(guessed.tab_size),
            // TODO@Alex do original: adivinhar o indentSize independente do tabSize
            indent_size: Some(IndentSize::Size(guessed.tab_size)),
            ..Default::default()
        });
    }

    /// `normalizeIndentation(str)`.
    pub fn normalize_indentation(&self, str: &[u16]) -> JsString {
        self.assert_not_disposed();
        normalize_indentation(str, self.options.get().indent_size, self.options.get().insert_spaces)
    }

    // #endregion

    // #region Leitura

    /// `getVersionId()`.
    pub fn get_version_id(&self) -> i64 {
        self.assert_not_disposed();
        self.version_id.get()
    }

    pub fn might_contain_rtl(&self) -> bool {
        self.buffer.borrow().might_contain_rtl()
    }

    pub fn might_contain_unusual_line_terminators(&self) -> bool {
        self.buffer.borrow().might_contain_unusual_line_terminators()
    }

    /// `removeUnusualLineTerminators(selections)`.
    pub fn remove_unusual_line_terminators(&self, selections: Option<Vec<Selection>>) {
        let matches = self.find_matches(
            "[\u{2028}\u{2029}]",
            FindScope::Whole,
            true,
            false,
            None,
            false,
            MAX_SAFE_SMALL_INTEGER,
        );
        self.buffer.borrow_mut().reset_might_contain_unusual_line_terminators();
        let operations: Vec<SingleEditOperation> = matches
            .iter()
            .map(|m| SingleEditOperation::new(m.range, None, false))
            .collect();
        self.push_edit_operations(selections, &operations, None, None, None);
    }

    pub fn might_contain_non_basic_ascii(&self) -> bool {
        self.buffer.borrow().might_contain_non_basic_ascii()
    }

    /// `getAlternativeVersionId()`.
    pub fn get_alternative_version_id(&self) -> i64 {
        self.assert_not_disposed();
        self.alternative_version_id.get()
    }

    /// `getInitialUndoRedoSnapshot()`.
    pub fn get_initial_undo_redo_snapshot(&self) -> Option<ResourceEditStackSnapshot> {
        self.assert_not_disposed();
        self.initial_undo_redo_snapshot.borrow().clone()
    }

    /// `getOffsetAt(position)`.
    pub fn get_offset_at(&self, raw_position: Position) -> usize {
        self.assert_not_disposed();
        let position = self.validate_position_impl(raw_position.line_number, raw_position.column, StringOffsetValidationType::Relaxed);
        self.buffer.borrow().get_offset_at(position.line_number, position.column)
    }

    /// `getPositionAt(offset)`.
    pub fn get_position_at(&self, raw_offset: i64) -> Position {
        self.assert_not_disposed();
        let buffer = self.buffer.borrow();
        let offset = (buffer.get_length() as i64).min(raw_offset.max(0));
        buffer.get_position_at(offset)
    }

    fn increase_version_id(&self) {
        self.version_id.set(self.version_id.get() + 1);
        self.alternative_version_id.set(self.version_id.get());
    }

    /// `_overwriteVersionId`.
    pub fn overwrite_version_id(&self, version_id: i64) {
        self.version_id.set(version_id);
    }

    /// `_overwriteAlternativeVersionId`.
    pub fn overwrite_alternative_version_id(&self, new_alternative_version_id: i64) {
        self.alternative_version_id.set(new_alternative_version_id);
    }

    /// `_overwriteInitialUndoRedoSnapshot`.
    pub fn overwrite_initial_undo_redo_snapshot(&self, snapshot: Option<ResourceEditStackSnapshot>) {
        *self.initial_undo_redo_snapshot.borrow_mut() = snapshot;
    }

    /// `getValue()`.
    pub fn get_value(&self) -> JsString {
        self.get_value_with(EndOfLinePreference::TextDefined, false)
    }

    /// `getValue(eol, preserveBOM)`.
    pub fn get_value_with(&self, eol: EndOfLinePreference, preserve_bom: bool) -> JsString {
        self.assert_not_disposed();
        if self.is_too_large_for_heap_operation() {
            errors::throw(Error::bug_indicating(Some("Operation would exceed heap memory limits")));
        }

        let full_model_range = self.get_full_model_range();
        let full_model_value = self.get_value_in_range_with(&full_model_range, eol);

        if preserve_bom {
            let mut value = self.buffer.borrow().get_bom().clone();
            value.push_units(full_model_value.units());
            return value;
        }

        full_model_value
    }

    /// `createSnapshot(preserveBOM)`.
    pub fn create_snapshot(&self, preserve_bom: bool) -> Box<dyn TextSnapshot> {
        Box::new(TextModelSnapshot {
            source: Box::new(self.buffer.borrow().create_snapshot(preserve_bom)),
            eos: false,
        })
    }

    /// `getValueLength()`.
    pub fn get_value_length(&self) -> usize {
        self.get_value_length_with(EndOfLinePreference::TextDefined, false)
    }

    /// `getValueLength(eol, preserveBOM)`.
    pub fn get_value_length_with(&self, eol: EndOfLinePreference, preserve_bom: bool) -> usize {
        self.assert_not_disposed();
        let full_model_range = self.get_full_model_range();
        let full_model_value = self.get_value_length_in_range_with(&full_model_range, eol);

        if preserve_bom {
            return self.buffer.borrow().get_bom().len() + full_model_value;
        }

        full_model_value
    }

    /// `getValueInRange(range)`.
    pub fn get_value_in_range(&self, raw_range: &Range) -> JsString {
        self.get_value_in_range_with(raw_range, EndOfLinePreference::TextDefined)
    }

    pub fn get_value_in_range_with(&self, raw_range: &Range, eol: EndOfLinePreference) -> JsString {
        self.assert_not_disposed();
        self.buffer.borrow().get_value_in_range(&self.validate_range(raw_range), eol)
    }

    /// `getValueLengthInRange(range)`.
    pub fn get_value_length_in_range(&self, raw_range: &Range) -> usize {
        self.get_value_length_in_range_with(raw_range, EndOfLinePreference::TextDefined)
    }

    pub fn get_value_length_in_range_with(&self, raw_range: &Range, eol: EndOfLinePreference) -> usize {
        self.assert_not_disposed();
        self.buffer.borrow().get_value_length_in_range(&self.validate_range(raw_range), eol)
    }

    /// `getCharacterCountInRange(range)`.
    pub fn get_character_count_in_range(&self, raw_range: &Range) -> usize {
        self.get_character_count_in_range_with(raw_range, EndOfLinePreference::TextDefined)
    }

    pub fn get_character_count_in_range_with(&self, raw_range: &Range, eol: EndOfLinePreference) -> usize {
        self.assert_not_disposed();
        self.buffer.borrow().get_character_count_in_range(&self.validate_range(raw_range), eol)
    }


    fn assert_line_number(&self, line_number: i32) {
        if line_number < 1 || line_number > self.get_line_count() {
            errors::throw(Error::bug_indicating(Some("Illegal value for lineNumber")));
        }
    }


    /// `getLineLength(lineNumber)`.
    pub fn get_line_length(&self, line_number: i32) -> usize {
        self.assert_not_disposed();
        self.assert_line_number(line_number);
        self.buffer.borrow().get_line_length(line_number)
    }

    /// `getLinesContent()`.
    pub fn get_lines_content(&self) -> Vec<JsString> {
        self.assert_not_disposed();
        if self.is_too_large_for_heap_operation() {
            errors::throw(Error::bug_indicating(Some("Operation would exceed heap memory limits")));
        }
        self.buffer.borrow().get_lines_content()
    }

    /// `getEOL()`.
    pub fn get_eol(&self) -> Eol {
        self.assert_not_disposed();
        self.buffer.borrow().get_eol()
    }

    /// `getEndOfLineSequence()`.
    pub fn get_end_of_line_sequence(&self) -> EndOfLineSequence {
        self.assert_not_disposed();
        EndOfLineSequence::from(self.buffer.borrow().get_eol())
    }





    /// `_validateRangeRelaxedNoAllocations(range)`: valida o intervalo dentro do buffer, mas deixa ele cair
    /// entre pares substitutos etc.
    pub fn validate_range_relaxed(&self, range: &Range) -> Range {
        let lines_count = self.buffer.borrow().get_line_count() as i32;
        let clamp = |line_number: i32, column: i32| -> (i32, i32) {
            if line_number < 1 {
                (1, 1)
            } else if line_number > lines_count {
                (lines_count, self.get_line_max_column(lines_count))
            } else if column <= 1 {
                (line_number, 1)
            } else {
                let max_column = self.get_line_max_column(line_number);
                (line_number, if column >= max_column { max_column } else { column })
            }
        };
        let (start_line_number, start_column) = clamp(range.start_line_number, range.start_column);
        let (end_line_number, end_column) = clamp(range.end_line_number, range.end_column);
        Range::new(start_line_number, start_column, end_line_number, end_column)
    }

    fn is_valid_position(&self, line_number: i32, column: i32, validation_type: StringOffsetValidationType) -> bool {
        if line_number < 1 || column < 1 {
            return false;
        }

        let buffer = self.buffer.borrow();
        if line_number > buffer.get_line_count() as i32 {
            return false;
        }

        if column == 1 {
            return true;
        }

        let max_column = buffer.get_line_length(line_number) as i32 + 1;
        if column > max_column {
            return false;
        }

        if validation_type == StringOffsetValidationType::SurrogatePairs {
            // !!Aqui, column > 1
            let char_code_before = buffer.get_line_char_code(line_number, column - 2);
            if char_code_before.is_some_and(strings::is_high_surrogate) {
                return false;
            }
        }

        true
    }

    fn validate_position_impl(&self, line_number: i32, column: i32, validation_type: StringOffsetValidationType) -> Position {
        let line_count = self.buffer.borrow().get_line_count() as i32;

        if line_number < 1 {
            return Position::new(1, 1);
        }

        if line_number > line_count {
            return Position::new(line_count, self.get_line_max_column(line_count));
        }

        if column <= 1 {
            return Position::new(line_number, 1);
        }

        let max_column = self.get_line_max_column(line_number);
        if column >= max_column {
            return Position::new(line_number, max_column);
        }

        if validation_type == StringOffsetValidationType::SurrogatePairs {
            // Se a posição cair no meio de um par substituto, move para antes do par
            // !!Aqui, column > 1
            let char_code_before = self.buffer.borrow().get_line_char_code(line_number, column - 2);
            if char_code_before.is_some_and(strings::is_high_surrogate) {
                return Position::new(line_number, column - 1);
            }
        }

        Position::new(line_number, column)
    }

    /// `validatePosition(position)`.
    pub fn validate_position(&self, position: Position) -> Position {
        self.assert_not_disposed();
        self.validate_position_impl(position.line_number, position.column, StringOffsetValidationType::SurrogatePairs)
    }

    /// `isValidRange(range)`.
    pub fn is_valid_range(&self, range: &Range) -> bool {
        self.is_valid_range_impl(range, StringOffsetValidationType::SurrogatePairs)
    }

    fn is_valid_range_impl(&self, range: &Range, validation_type: StringOffsetValidationType) -> bool {
        let (start_line_number, start_column) = (range.start_line_number, range.start_column);
        let (end_line_number, end_column) = (range.end_line_number, range.end_column);

        if !self.is_valid_position(start_line_number, start_column, StringOffsetValidationType::Relaxed) {
            return false;
        }
        if !self.is_valid_position(end_line_number, end_column, StringOffsetValidationType::Relaxed) {
            return false;
        }

        if validation_type == StringOffsetValidationType::SurrogatePairs {
            let (start_inside_surrogate_pair, end_inside_surrogate_pair) =
                self.inside_surrogate_pair(start_line_number, start_column, end_line_number, end_column);
            return !start_inside_surrogate_pair && !end_inside_surrogate_pair;
        }

        true
    }

    /// Se o começo e o fim caem no meio de um par substituto.
    fn inside_surrogate_pair(&self, start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> (bool, bool) {
        let buffer = self.buffer.borrow();
        let char_code_before_start = if start_column > 1 {
            buffer.get_line_char_code(start_line_number, start_column - 2)
        } else {
            None
        };
        let char_code_before_end = if end_column > 1 && end_column <= buffer.get_line_length(end_line_number) as i32 {
            buffer.get_line_char_code(end_line_number, end_column - 2)
        } else {
            None
        };
        (
            char_code_before_start.is_some_and(strings::is_high_surrogate),
            char_code_before_end.is_some_and(strings::is_high_surrogate),
        )
    }

    /// `validateRange(range)`.
    pub fn validate_range(&self, range: &Range) -> Range {
        self.assert_not_disposed();

        // Evita alocação no caso mais provável
        if self.is_valid_range_impl(range, StringOffsetValidationType::SurrogatePairs) {
            return *range;
        }

        let start = self.validate_position_impl(range.start_line_number, range.start_column, StringOffsetValidationType::Relaxed);
        let end = self.validate_position_impl(range.end_line_number, range.end_column, StringOffsetValidationType::Relaxed);

        let (start_line_number, start_column) = (start.line_number, start.column);
        let (end_line_number, end_column) = (end.line_number, end.column);

        let (start_inside_surrogate_pair, end_inside_surrogate_pair) =
            self.inside_surrogate_pair(start_line_number, start_column, end_line_number, end_column);

        if !start_inside_surrogate_pair && !end_inside_surrogate_pair {
            return Range::new(start_line_number, start_column, end_line_number, end_column);
        }

        if start_line_number == end_line_number && start_column == end_column {
            // não expande um intervalo colapsado, só move para uma posição válida
            return Range::new(start_line_number, start_column - 1, end_line_number, end_column - 1);
        }

        if start_inside_surrogate_pair && end_inside_surrogate_pair {
            // expande o intervalo nas duas pontas
            return Range::new(start_line_number, start_column - 1, end_line_number, end_column + 1);
        }

        if start_inside_surrogate_pair {
            // expande só no começo
            return Range::new(start_line_number, start_column - 1, end_line_number, end_column);
        }

        // expande só no fim
        Range::new(start_line_number, start_column, end_line_number, end_column + 1)
    }

    /// `modifyPosition(position, offset)`.
    pub fn modify_position(&self, raw_position: Position, offset: i64) -> Position {
        self.assert_not_disposed();
        let candidate = self.get_offset_at(raw_position) as i64 + offset;
        let length = self.buffer.borrow().get_length() as i64;
        self.get_position_at(length.min(candidate.max(0)))
    }

    /// `getFullModelRange()`.
    pub fn get_full_model_range(&self) -> Range {
        self.assert_not_disposed();
        let line_count = self.get_line_count();
        Range::new(1, 1, line_count, self.get_line_max_column(line_count))
    }

    /// `getRangeAt(start, end)`.
    pub fn get_range_at(&self, start: usize, end: usize) -> Range {
        self.buffer.borrow().get_range_at(start, end - start)
    }



    /// `getLanguageId()`.
    pub fn get_language_id(&self) -> String {
        self.tokenization().get_language_id()
    }

    /// `setLanguage(languageId, source)`.
    pub fn set_language(&self, language_id: &str, source: Option<&str>) {
        self.tokenization().set_language_id(language_id, source.unwrap_or("api"));
        self.language_service.request_rich_language_features(language_id);
    }

    /// `getLanguageIdAtPosition(lineNumber, column)`.
    pub fn get_language_id_at_position(&self, line_number: i32, column: i32) -> String {
        self.tokenization().get_language_id_at_position(line_number, column)
    }

    /// `tokenization`: a parte de tokenização (`ITokenizationTextModelPart`).
    /// `bracketPairs` (`IBracketPairsTextModelPart`).
    pub fn bracket_pairs(&self) -> &Rc<BracketPairsTextModelPart> {
        &self.bracket_pairs
    }

    /// `guides` (`IGuidesTextModelPart`).
    pub fn guides(&self) -> &GuidesTextModelPart {
        &self.guides
    }

    pub fn tokenization(&self) -> &Rc<TokenizationTextModelPart> {
        self.tokenization.get().expect("a parte de tokenização é criada no construtor")
    }

    /// O `ILanguageConfigurationService` do modelo.
    pub fn language_configuration_service(&self) -> &Rc<dyn ILanguageConfigurationService> {
        &self.language_configuration_service
    }

    /// O `ILanguageService` do modelo.
    pub fn language_service(&self) -> &Rc<dyn ILanguageService> {
        &self.language_service
    }

    pub fn on_did_change_language(&self) -> Event<crate::text_model_events::ModelLanguageChangedEvent> {
        self.tokenization().on_did_change_language()
    }

    pub fn on_did_change_language_configuration(&self) -> Event<crate::text_model_events::ModelLanguageConfigurationChangedEvent> {
        self.tokenization().on_did_change_language_configuration()
    }

    pub fn on_did_change_tokens(&self) -> Event<crate::text_model_events::ModelTokensChangedEvent> {
        self.tokenization().on_did_change_tokens()
    }

    pub fn on_did_change_attached(&self) -> Event<()> {
        self.on_did_change_attached.event()
    }

    /// `onBeforeAttached()`: um editor passa a mostrar o modelo.
    pub fn on_before_attached(&self) -> Rc<AttachedView> {
        self.attached_editor_count.set(self.attached_editor_count.get() + 1);
        if self.attached_editor_count.get() == 1 {
            self.tokenization().handle_did_change_attached();
            self.on_did_change_attached.fire(());
        }
        self.attached_views.attach_view()
    }

    /// `onBeforeDetached(view)`.
    pub fn on_before_detached(&self, view: &Rc<AttachedView>) {
        self.attached_editor_count.set(self.attached_editor_count.get() - 1);
        if self.attached_editor_count.get() == 0 {
            self.tokenization().handle_did_change_attached();
            self.on_did_change_attached.fire(());
        }
        self.attached_views.detach_view(view);
    }

    /// `getWordAtPosition(position)`.
    pub fn get_word_at_position(&self, position: Position) -> Option<WordAtPosition> {
        self.tokenization().get_word_at_position(position)
    }

    /// `getWordUntilPosition(position)`.
    pub fn get_word_until_position(&self, position: Position) -> WordAtPosition {
        self.tokenization().get_word_until_position(position)
    }

    // #endregion
}

impl std::fmt::Display for TextModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextModel({})", self.associated_resource.to_string(false))
    }
}

/// `indentOfLine(line)`: quantos espaços e tabs há no começo da linha.
pub fn indent_of_line(line: &JsString) -> usize {
    line.units().iter().take_while(|&&unit| unit == 0x20 || unit == 0x09).count()
}

/// O que `findMatches` aceita como `rawSearchScope`: `null`/booleano (o modelo todo) ou intervalos.
pub enum FindScope {
    Whole,
    Ranges(Vec<Range>),
}

impl TextModel {
    // #region Busca

    fn find_matches_line_by_line(&self, search_range: &Range, search_data: &SearchData, capture_matches: bool, limit_result_count: usize) -> Vec<FindMatch> {
        self.buffer
            .borrow()
            .find_matches_line_by_line(search_range, search_data, capture_matches, limit_result_count)
    }

    /// `findMatches(searchString, searchScope, isRegex, matchCase, wordSeparators, captureMatches, limitResultCount)`.
    #[allow(clippy::too_many_arguments)]
    pub fn find_matches(
        &self,
        search_string: impl Into<JsString>,
        raw_search_scope: FindScope,
        is_regex: bool,
        match_case: bool,
        word_separators: Option<&str>,
        capture_matches: bool,
        limit_result_count: usize,
    ) -> Vec<FindMatch> {
        self.assert_not_disposed();
        let search_string = search_string.into();

        let mut search_ranges: Vec<Range> = match raw_search_scope {
            FindScope::Whole => vec![self.get_full_model_range()],
            FindScope::Ranges(ranges) => ranges.iter().map(|search_scope| self.validate_range(search_scope)).collect(),
        };
        if search_ranges.is_empty() {
            search_ranges = vec![self.get_full_model_range()];
        }

        search_ranges.sort_by(|d1, d2| {
            d1.start_line_number
                .cmp(&d2.start_line_number)
                .then(d1.start_column.cmp(&d2.start_column))
        });

        let mut unique_search_ranges: Vec<Range> = Vec::new();
        let mut ranges = search_ranges.into_iter();
        let mut previous = ranges.next().expect("há ao menos um intervalo");
        for current in ranges {
            if Range::are_intersecting(&previous, &current) {
                previous = previous.plus_range(&current);
            } else {
                unique_search_ranges.push(previous);
                previous = current;
            }
        }
        unique_search_ranges.push(previous);

        let params = SearchParams::new(search_string.clone(), is_regex, match_case, word_separators);
        let mut result = Vec::new();
        if !is_regex && !search_string.units().contains(&10) {
            // nem regex, nem várias linhas
            let Some(search_data) = params.parse_search_request() else {
                return Vec::new();
            };
            for search_range in &unique_search_ranges {
                result.extend(self.find_matches_line_by_line(search_range, &search_data, capture_matches, limit_result_count));
            }
        } else {
            for search_range in &unique_search_ranges {
                result.extend(TextModelSearch::find_matches(self, &params, search_range, capture_matches, limit_result_count));
            }
        }
        result
    }

    /// `findNextMatch(searchString, searchStart, isRegex, matchCase, wordSeparators, captureMatches)`.
    pub fn find_next_match(
        &self,
        search_string: impl Into<JsString>,
        raw_search_start: Position,
        is_regex: bool,
        match_case: bool,
        word_separators: Option<&str>,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        self.assert_not_disposed();
        let search_string = search_string.into();
        let search_start = self.validate_position(raw_search_start);
        let params = SearchParams::new(search_string.clone(), is_regex, match_case, word_separators);

        if !is_regex && !search_string.units().contains(&10) {
            let search_data = params.parse_search_request()?;

            let line_count = self.get_line_count();
            let mut search_range = Range::new(search_start.line_number, search_start.column, line_count, self.get_line_max_column(line_count));
            let ret = self.find_matches_line_by_line(&search_range, &search_data, capture_matches, 1);
            if let Some(first) = ret.into_iter().next() {
                return Some(first);
            }

            search_range = Range::new(1, 1, search_start.line_number, self.get_line_max_column(search_start.line_number));
            return self
                .find_matches_line_by_line(&search_range, &search_data, capture_matches, 1)
                .into_iter()
                .next();
        }

        TextModelSearch::find_next_match(self, &params, search_start, capture_matches)
    }

    /// `findPreviousMatch(searchString, searchStart, isRegex, matchCase, wordSeparators, captureMatches)`.
    pub fn find_previous_match(
        &self,
        search_string: impl Into<JsString>,
        raw_search_start: Position,
        is_regex: bool,
        match_case: bool,
        word_separators: Option<&str>,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        self.assert_not_disposed();
        let search_start = self.validate_position(raw_search_start);
        TextModelSearch::find_previous_match(
            self,
            &SearchParams::new(search_string.into(), is_regex, match_case, word_separators),
            search_start,
            capture_matches,
        )
    }

    // #endregion

    // #region Edição

    /// Roda `body` com a emissão de eventos de conteúdo e de decorações adiada, como o
    /// `try { begin...; body } finally { end... }` do original: o fim da emissão acontece mesmo se `body`
    /// lançar, e depois o erro segue adiante. `cleanup` roda antes do fim da emissão.
    fn run_deferred<R>(&self, resulting_selection: Option<Vec<Selection>>, cleanup: impl FnOnce(), body: impl FnOnce() -> R) -> R {
        self.on_did_change_decorations.begin_deferred_emit();
        self.event_emitter.begin_deferred_emit();
        let outcome = errors::catch(body);
        cleanup();
        self.event_emitter.end_deferred_emit(resulting_selection);
        self.on_did_change_decorations.end_deferred_emit(self);
        match outcome {
            Ok(value) => value,
            Err(error) => errors::throw(error),
        }
    }

    pub fn push_stack_element(&self) {
        self.command_manager.push_stack_element();
    }

    pub fn pop_stack_element(&self) {
        self.command_manager.pop_stack_element();
    }

    /// `pushEOL(eol)`.
    pub fn push_eol(&self, eol: EndOfLineSequence) {
        let current_eol = EndOfLineSequence::from(self.get_eol());
        if current_eol == eol {
            return;
        }
        self.run_deferred(
            None,
            || {},
            || {
                if self.initial_undo_redo_snapshot.borrow().is_none() {
                    *self.initial_undo_redo_snapshot.borrow_mut() = Some(self.undo_redo_service.create_snapshot(&self.uri()));
                }
                self.command_manager.push_eol(eol);
            },
        );
    }

    /// `_validateEditOperation`.
    fn validate_edit_operation(&self, raw_operation: &SingleEditOperation) -> ValidAnnotatedEditOperation {
        let validated_range = self.validate_range(&raw_operation.range);

        // Normaliza a edição quando o texto de troca termina com um CR solto e o intervalo termina logo
        // antes de um CRLF do buffer: tira o CR final do texto de troca.
        let mut op_text = raw_operation.text.clone().filter(|text| !text.is_empty());
        if let Some(text) = &op_text {
            let ends_with_lone_cr = text.units().last() == Some(&13);
            let remove_trailing_cr = self.get_eol() == Eol::CrLf
                && ends_with_lone_cr
                && validated_range.end_column == self.get_line_max_column(validated_range.end_line_number);
            if remove_trailing_cr {
                op_text = Some(text.slice(0, text.len() - 1));
            }
        } else {
            op_text = raw_operation.text.clone();
        }

        ValidAnnotatedEditOperation {
            identifier: raw_operation.identifier,
            range: validated_range,
            text: op_text,
            force_move_markers: raw_operation.force_move_markers,
            is_auto_whitespace_edit: raw_operation.is_auto_whitespace_edit,
            is_tracked: raw_operation.is_tracked,
        }
    }

    fn validate_edit_operations(&self, raw_operations: &[SingleEditOperation]) -> Vec<ValidAnnotatedEditOperation> {
        raw_operations.iter().map(|op| self.validate_edit_operation(op)).collect()
    }

    /// `pushEditOperations(beforeCursorState, editOperations, cursorStateComputer, group, reason)`.
    pub fn push_edit_operations(
        &self,
        before_cursor_state: Option<Vec<Selection>>,
        edit_operations: &[SingleEditOperation],
        cursor_state_computer: Option<CursorStateComputer<'_>>,
        group: Option<&UndoRedoGroup>,
        reason: Option<TextModelEditSource>,
    ) -> Option<Vec<Selection>> {
        self.run_deferred(
            None,
            || {},
            || {
                let operations = self.validate_edit_operations(edit_operations);
                self.push_edit_operations_impl(before_cursor_state, operations, cursor_state_computer, group, reason)
            },
        )
    }

    fn push_edit_operations_impl(
        &self,
        before_cursor_state: Option<Vec<Selection>>,
        mut edit_operations: Vec<ValidAnnotatedEditOperation>,
        cursor_state_computer: Option<CursorStateComputer<'_>>,
        group: Option<&UndoRedoGroup>,
        reason: Option<TextModelEditSource>,
    ) -> Option<Vec<Selection>> {
        let trim_lines = self.trim_auto_whitespace_lines.borrow().clone();
        if self.options.get().trim_auto_whitespace
            && let Some(trim_lines) = trim_lines
        {
            // Passa por cada número de linha guardado e insere uma edição que apara o espaço em branco,
            // se for seguro (sem conflito com as outras edições).
            let incoming_edits: Vec<(Range, Option<JsString>)> = edit_operations
                .iter()
                .map(|op| (self.validate_range(&op.range), op.text.clone()))
                .collect();

            // Às vezes os formatadores mudam intervalos sozinhos, o que pode causar aparo indesejado perto
            // do cursor. Heurística: se as edições ocorrem perto do cursor, pode aparar.
            let mut edits_are_near_cursors = true;
            if let Some(before_cursor_state) = &before_cursor_state {
                for sel in before_cursor_state {
                    let mut found_edit_near_sel = false;
                    for (edit_range, _) in &incoming_edits {
                        let sel_is_above = edit_range.start_line_number > sel.end_line_number;
                        let sel_is_below = sel.start_line_number > edit_range.end_line_number;
                        if !sel_is_above && !sel_is_below {
                            found_edit_near_sel = true;
                            break;
                        }
                    }
                    if !found_edit_near_sel {
                        edits_are_near_cursors = false;
                        break;
                    }
                }
            }

            if edits_are_near_cursors {
                for &trim_line_number in &trim_lines {
                    let max_line_column = self.get_line_max_column(trim_line_number);

                    let mut allow_trim_line = true;
                    for (edit_range, edit_text) in &incoming_edits {
                        if trim_line_number < edit_range.start_line_number || trim_line_number > edit_range.end_line_number {
                            // `trimLine` está totalmente fora desta edição
                            continue;
                        }

                        // Aqui: editRange.startLineNumber <= trimLine <= editRange.endLineNumber

                        if trim_line_number == edit_range.start_line_number
                            && edit_range.start_column == max_line_column
                            && edit_range.is_empty()
                            && edit_text.as_ref().is_some_and(|text| text.units().first() == Some(&10))
                        {
                            // Esta edição insere uma linha nova (e talvez mais texto) depois de `trimLine`
                            continue;
                        }

                        if trim_line_number == edit_range.start_line_number
                            && edit_range.start_column == 1
                            && edit_range.is_empty()
                            && edit_text.as_ref().is_some_and(|text| text.units().last() == Some(&10))
                        {
                            // Esta edição insere uma linha nova (e talvez mais texto) antes de `trimLine`
                            continue;
                        }

                        // Não dá para aparar esta linha: atrapalharia uma edição que está chegando
                        allow_trim_line = false;
                        break;
                    }

                    if allow_trim_line {
                        let trim_range = Range::new(trim_line_number, 1, trim_line_number, max_line_column);
                        edit_operations.push(ValidAnnotatedEditOperation {
                            identifier: None,
                            range: trim_range,
                            text: None,
                            force_move_markers: false,
                            is_auto_whitespace_edit: false,
                            is_tracked: false,
                        });
                    }
                }
            }

            *self.trim_auto_whitespace_lines.borrow_mut() = None;
        }
        if self.initial_undo_redo_snapshot.borrow().is_none() {
            *self.initial_undo_redo_snapshot.borrow_mut() = Some(self.undo_redo_service.create_snapshot(&self.uri()));
        }
        self.command_manager
            .push_edit_operation(before_cursor_state, edit_operations, cursor_state_computer, group, reason)
    }

    /// `_applyUndo`.
    pub fn apply_undo(&self, changes: &[TextChange], eol: EndOfLineSequence, resulting_alternative_version_id: i64, resulting_selection: Option<&[Selection]>) {
        let edits: Vec<SingleEditOperation> = changes
            .iter()
            .map(|change| {
                let range_start = self.get_position_at(change.new_position as i64);
                let range_end = self.get_position_at(change.new_end() as i64);
                SingleEditOperation::new(
                    Range::new(range_start.line_number, range_start.column, range_end.line_number, range_end.column),
                    Some(change.old_text.clone()),
                    false,
                )
            })
            .collect();
        self.apply_undo_redo_edits(&edits, eol, true, false, resulting_alternative_version_id, resulting_selection);
    }

    /// `_applyRedo`.
    pub fn apply_redo(&self, changes: &[TextChange], eol: EndOfLineSequence, resulting_alternative_version_id: i64, resulting_selection: Option<&[Selection]>) {
        let edits: Vec<SingleEditOperation> = changes
            .iter()
            .map(|change| {
                let range_start = self.get_position_at(change.old_position as i64);
                let range_end = self.get_position_at(change.old_end() as i64);
                SingleEditOperation::new(
                    Range::new(range_start.line_number, range_start.column, range_end.line_number, range_end.column),
                    Some(change.new_text.clone()),
                    false,
                )
            })
            .collect();
        self.apply_undo_redo_edits(&edits, eol, false, true, resulting_alternative_version_id, resulting_selection);
    }

    fn apply_undo_redo_edits(
        &self,
        edits: &[SingleEditOperation],
        eol: EndOfLineSequence,
        is_undoing: bool,
        is_redoing: bool,
        resulting_alternative_version_id: i64,
        resulting_selection: Option<&[Selection]>,
    ) {
        let selection = resulting_selection.map(<[Selection]>::to_vec);
        self.run_deferred(
            selection.clone(),
            || {
                self.is_undoing.set(false);
                self.is_redoing.set(false);
            },
            || {
                self.is_undoing.set(is_undoing);
                self.is_redoing.set(is_redoing);
                let operations = self.validate_edit_operations(edits);
                self.do_apply_edits(operations, false, EditSources::apply_edits(), selection.clone());
                self.set_eol(eol);
                self.overwrite_alternative_version_id(resulting_alternative_version_id);
            },
        );
    }

    /// `applyEdits(operations)`.
    pub fn apply_edits(&self, raw_operations: &[SingleEditOperation]) {
        self.apply_edits_impl(raw_operations, false, EditSources::apply_edits());
    }

    /// `applyEdits(operations, true)`: devolve as operações inversas.
    pub fn apply_edits_computing_undo(&self, raw_operations: &[SingleEditOperation]) -> Vec<ValidEditOperation> {
        self.apply_edits_impl(raw_operations, true, EditSources::apply_edits())
            .expect("computeUndoEdits devolve as operações inversas")
    }

    /// `applyEdits(operations, computeUndoEdits, reason)`.
    pub fn apply_edits_with_reason(&self, raw_operations: &[SingleEditOperation], compute_undo_edits: bool, reason: TextModelEditSource) -> Option<Vec<ValidEditOperation>> {
        self.apply_edits_impl(raw_operations, compute_undo_edits, reason)
    }

    fn apply_edits_impl(&self, raw_operations: &[SingleEditOperation], compute_undo_edits: bool, reason: TextModelEditSource) -> Option<Vec<ValidEditOperation>> {
        self.run_deferred(
            None,
            || {},
            || {
                let operations = self.validate_edit_operations(raw_operations);
                self.do_apply_edits(operations, compute_undo_edits, reason, None)
            },
        )
    }

    /// `applyEdits` para operações já validadas (o caminho do `EditStack`).
    pub(crate) fn apply_valid_edits(&self, operations: Vec<ValidAnnotatedEditOperation>, compute_undo_edits: bool, reason: TextModelEditSource) -> Option<Vec<ValidEditOperation>> {
        self.run_deferred(None, || {}, || self.do_apply_edits(operations, compute_undo_edits, reason, None))
    }

    fn do_apply_edits(
        &self,
        raw_operations: Vec<ValidAnnotatedEditOperation>,
        compute_undo_edits: bool,
        reason: TextModelEditSource,
        resulting_selection: Option<Vec<Selection>>,
    ) -> Option<Vec<ValidEditOperation>> {
        let old_line_count = self.buffer.borrow().get_line_count() as i32;
        let result = self
            .buffer
            .borrow_mut()
            .apply_edits(&raw_operations, self.options.get().trim_auto_whitespace, compute_undo_edits);
        let new_line_count = self.buffer.borrow().get_line_count() as i32;

        let content_changes = result.changes;
        *self.trim_auto_whitespace_lines.borrow_mut() = result.trim_auto_whitespace_line_numbers;

        if !content_changes.is_empty() {
            // Primeira passada para atualizar as decorações, porque na segunda passada, que emite os
            // eventos de conteúdo, queremos ler as decorações finais
            {
                let mut decorations = self.decorations.borrow_mut();
                for change in &content_changes {
                    decorations.trees.accept_replace(
                        change.range_offset as i64,
                        change.range_length as i64,
                        change.text.len() as i64,
                        change.force_move_markers,
                    );
                }
            }

            let mut raw_content_changes: Vec<ModelRawChange> = Vec::new();

            self.increase_version_id();

            let mut line_count = old_line_count;
            for change in &content_changes {
                let eol_count = count_eol(change.text.units()).eol_count as i32;
                self.on_did_change_decorations.fire(self);

                let start_line_number = change.range.start_line_number;
                let end_line_number = change.range.end_line_number;

                let deleting_lines_cnt = end_line_number - start_line_number;
                let inserting_lines_cnt = eol_count;
                let editing_lines_cnt = deleting_lines_cnt.min(inserting_lines_cnt);

                let change_line_count_delta = inserting_lines_cnt - deleting_lines_cnt;

                let current_edit_start_line_number = new_line_count - line_count - change_line_count_delta + start_line_number;

                for j in (0..=editing_lines_cnt).rev() {
                    let edit_line_number = start_line_number + j;
                    let current_edit_line_number = current_edit_start_line_number + j;

                    raw_content_changes.push(ModelRawChange::LineChanged(ModelRawLineChanged {
                        line_number: edit_line_number,
                        line_number_post_edit: current_edit_line_number,
                    }));
                }

                if editing_lines_cnt < deleting_lines_cnt {
                    // Precisa apagar algumas linhas
                    let splice_start_line_number = start_line_number + editing_lines_cnt;
                    let cnt = inserting_lines_cnt - deleting_lines_cnt;
                    let last_untouched_line_post_edit = new_line_count - line_count - cnt + splice_start_line_number;
                    raw_content_changes.push(ModelRawChange::LinesDeleted(ModelRawLinesDeleted {
                        from_line_number: splice_start_line_number + 1,
                        to_line_number: end_line_number,
                        last_untouched_line_post_edit,
                    }));
                }

                if editing_lines_cnt < inserting_lines_cnt {
                    // Precisa inserir algumas linhas
                    let splice_line_number = start_line_number + editing_lines_cnt;
                    let cnt = inserting_lines_cnt - editing_lines_cnt;
                    let from_line_number = new_line_count - line_count - cnt + splice_line_number + 1;
                    raw_content_changes.push(ModelRawChange::LinesInserted(ModelRawLinesInserted {
                        from_line_number: splice_line_number + 1,
                        from_line_number_post_edit: from_line_number,
                        count: cnt,
                    }));
                }

                line_count += change_line_count_delta;
            }

            let version_id = self.get_version_id();
            let eol = self.buffer.borrow().get_eol();
            self.emit_content_changed_event(
                ModelRawContentChangedEvent::new(raw_content_changes, version_id, self.is_undoing.get(), self.is_redoing.get()),
                ModelContentChangedEvent {
                    changes: content_changes.iter().map(ModelContentChange::from).collect(),
                    eol,
                    is_eol_change: false,
                    version_id,
                    is_undoing: self.is_undoing.get(),
                    is_redoing: self.is_redoing.get(),
                    is_flush: false,
                    detailed_reasons: vec![reason],
                    detailed_reasons_change_lengths: vec![content_changes.len()],
                },
                resulting_selection,
            );
        }

        result.reverse_edits
    }

    /// `undo()`.
    pub fn undo(&self) {
        self.undo_redo_service.undo(UndoRedoTarget::Resource(&self.uri()));
    }

    /// `canUndo()`.
    pub fn can_undo(&self) -> bool {
        self.undo_redo_service.can_undo(UndoRedoTarget::Resource(&self.uri()))
    }

    /// `redo()`.
    pub fn redo(&self) {
        self.undo_redo_service.redo(UndoRedoTarget::Resource(&self.uri()));
    }

    /// `canRedo()`.
    pub fn can_redo(&self) -> bool {
        self.undo_redo_service.can_redo(UndoRedoTarget::Resource(&self.uri()))
    }

    // #endregion
}

/// O accessor de `changeDecorations`: só vale dentro do callback (o empréstimo garante que não escapa).
pub struct ModelDecorationsChangeAccessor<'a> {
    model: &'a TextModel,
    owner_id: i32,
}

impl ModelDecorationsChangeAccessor<'_> {
    /// `addDecoration(range, options)`.
    pub fn add_decoration(&self, range: &Range, options: Rc<ModelDecorationOptions>) -> String {
        self.model
            .delta_decorations_impl(
                self.owner_id,
                &[],
                &[ModelDeltaDecoration {
                    range: *range,
                    options,
                }],
                false,
            )
            .remove(0)
    }

    /// `changeDecoration(id, newRange)`.
    pub fn change_decoration(&self, id: &str, new_range: &Range) {
        self.model.change_decoration_impl(self.owner_id, id, new_range);
    }

    /// `changeDecorationOptions(id, options)`.
    pub fn change_decoration_options(&self, id: &str, options: Rc<ModelDecorationOptions>) {
        self.model.change_decoration_options_impl(self.owner_id, id, options);
    }

    /// `removeDecoration(id)`.
    pub fn remove_decoration(&self, id: &str) {
        self.model.delta_decorations_impl(self.owner_id, &[id.to_owned()], &[], false);
    }

    /// `deltaDecorations(oldDecorations, newDecorations)`.
    pub fn delta_decorations(&self, old_decorations: &[String], new_decorations: &[ModelDeltaDecoration]) -> Vec<String> {
        if old_decorations.is_empty() && new_decorations.is_empty() {
            // nada a fazer
            return Vec::new();
        }
        self.model
            .delta_decorations_impl(self.owner_id, old_decorations, new_decorations, false)
    }
}

impl TextModel {
    // #region Decorações

    fn handle_before_fire_decorations_changed_event(&self, affected: &AffectedByDecorations) {
        // Chamado antes de o evento de decorações ser disparado.
        if let Some(lines) = affected.injected_text_lines.as_ref().filter(|lines| !lines.is_empty()) {
            let line_change_events = lines
                .iter()
                .map(|&line_number| ModelRawLineChanged {
                    line_number,
                    line_number_post_edit: line_number,
                })
                .collect();
            self.on_did_change_content_or_injected_text(&ContentOrInjectedTextEvent::InjectedText(ModelInjectedTextChangedEvent {
                changes: line_change_events,
            }));
        }
        self.fire_on_did_change_line_height(affected.line_heights.as_deref());
        self.fire_on_did_change_font(affected.font_lines.as_deref());
    }

    fn fire_on_did_change_line_height(&self, affected_line_heights: Option<&[LineHeightChangingDecoration]>) {
        if let Some(changes) = affected_line_heights.filter(|changes| !changes.is_empty()) {
            self.on_did_change_line_height.fire(ModelLineHeightChangedEvent {
                changes: changes
                    .iter()
                    .map(|change| ModelLineHeightChanged {
                        owner_id: change.owner_id,
                        decoration_id: change.decoration_id.clone(),
                        line_number: change.line_number,
                        line_height_multiplier: change.line_height,
                    })
                    .collect(),
            });
        }
    }

    fn fire_on_did_change_font(&self, affected_font_lines: Option<&[LineFontChangingDecoration]>) {
        if let Some(changes) = affected_font_lines.filter(|changes| !changes.is_empty()) {
            self.on_did_change_font.fire(ModelFontChangedEvent {
                changes: changes
                    .iter()
                    .map(|change| ModelFontChanged {
                        owner_id: change.owner_id,
                        line_number: change.line_number,
                    })
                    .collect(),
            });
        }
    }

    fn on_did_change_content_or_injected_text(&self, e: &ContentOrInjectedTextEvent) {
        let view_models: Vec<Rc<dyn ViewModel>> = self.view_models.borrow().clone();
        for view_model in &view_models {
            if let Err(error) = errors::catch(|| view_model.on_did_change_content_or_injected_text(e)) {
                errors::on_unexpected_error(&error);
            }
        }
        for view_model in &view_models {
            if let Err(error) = errors::catch(|| view_model.emit_content_change_event(e)) {
                errors::on_unexpected_error(&error);
            }
        }
    }

    /// `changeDecorations(callback, ownerId)`: devolve `None` se o callback lançou (o erro vai para o
    /// `onUnexpectedError`).
    pub fn change_decorations<T>(&self, callback: impl FnOnce(&ModelDecorationsChangeAccessor<'_>) -> T, owner_id: i32) -> Option<T> {
        self.assert_not_disposed();

        self.on_did_change_decorations.begin_deferred_emit();
        let accessor = ModelDecorationsChangeAccessor { model: self, owner_id };
        let result = match errors::catch(|| callback(&accessor)) {
            Ok(value) => Some(value),
            Err(error) => {
                errors::on_unexpected_error(&error);
                None
            }
        };
        self.on_did_change_decorations.end_deferred_emit(self);
        result
    }

    /// `deltaDecorations(oldDecorations, newDecorations, ownerId)`.
    pub fn delta_decorations(&self, old_decorations: &[String], new_decorations: &[ModelDeltaDecoration], owner_id: i32) -> Vec<String> {
        self.assert_not_disposed();
        if old_decorations.is_empty() && new_decorations.is_empty() {
            // nada a fazer
            return Vec::new();
        }

        self.delta_decoration_call_cnt.set(self.delta_decoration_call_cnt.get() + 1);
        if self.delta_decoration_call_cnt.get() > 1 {
            eprintln!("Invoking deltaDecorations recursively could lead to leaking decorations.");
            errors::on_unexpected_error(&Error::new("Invoking deltaDecorations recursively could lead to leaking decorations."));
        }
        self.on_did_change_decorations.begin_deferred_emit();
        let result = self.delta_decorations_impl(owner_id, old_decorations, new_decorations, false);
        self.on_did_change_decorations.end_deferred_emit(self);
        self.delta_decoration_call_cnt.set(self.delta_decoration_call_cnt.get() - 1);
        result
    }

    /// `_getTrackedRange(id)`.
    pub fn get_tracked_range(&self, id: &str) -> Option<Range> {
        self.get_decoration_range(id)
    }

    /// `_setTrackedRange(id, newRange, newStickiness)`.
    pub fn set_tracked_range(&self, id: Option<&str>, new_range: Option<&Range>, new_stickiness: TrackedRangeStickiness) -> Option<String> {
        let node = id.and_then(|id| self.decorations.borrow().nodes.get(id).copied());

        let Some(node) = node else {
            let new_range = new_range?;
            // o nó não existe e o pedido é de criar: adiciona o intervalo rastreado
            return Some(
                self.delta_decorations_impl(
                    0,
                    &[],
                    &[ModelDeltaDecoration {
                        range: *new_range,
                        options: tracked_range_options(new_stickiness),
                    }],
                    true,
                )
                .remove(0),
            );
        };

        let Some(new_range) = new_range else {
            // o nó existe e o pedido é de apagar: apaga o nó
            let mut decorations = self.decorations.borrow_mut();
            decorations.trees.delete(node);
            let node_id = decorations.trees.arena.get(node).id.clone();
            decorations.nodes.remove(&node_id);
            decorations.trees.arena.free(node);
            return None;
        };

        // o nó existe e o pedido é de definir: muda o intervalo rastreado e as opções
        let range = self.validate_range_relaxed(new_range);
        let (start_offset, end_offset) = {
            let buffer = self.buffer.borrow();
            (
                buffer.get_offset_at(range.start_line_number, range.start_column) as i64,
                buffer.get_offset_at(range.end_line_number, range.end_column) as i64,
            )
        };
        let version_id = self.get_version_id();
        let mut decorations = self.decorations.borrow_mut();
        decorations.trees.delete(node);
        let n = decorations.trees.arena.get_mut(node);
        n.reset(version_id, start_offset, end_offset, Some(range));
        n.set_options(tracked_range_options(new_stickiness));
        let id = n.id.clone();
        decorations.trees.insert(node);
        Some(id)
    }

    /// `removeAllDecorationsWithOwnerId(ownerId)`.
    pub fn remove_all_decorations_with_owner_id(&self, owner_id: i32) {
        if self.is_disposed.get() {
            return;
        }
        let mut decorations = self.decorations.borrow_mut();
        for node in decorations.trees.collect_nodes_from_owner(owner_id) {
            decorations.trees.delete(node);
            let id = decorations.trees.arena.get(node).id.clone();
            decorations.nodes.remove(&id);
            decorations.trees.arena.free(node);
        }
    }

    /// `getDecorationOptions(decorationId)`.
    pub fn get_decoration_options(&self, decoration_id: &str) -> Option<Rc<ModelDecorationOptions>> {
        let decorations = self.decorations.borrow();
        let node = *decorations.nodes.get(decoration_id)?;
        decorations.trees.arena.get(node).options.clone()
    }

    /// `getDecorationRange(decorationId)`.
    pub fn get_decoration_range(&self, decoration_id: &str) -> Option<Range> {
        let mut decorations = self.decorations.borrow_mut();
        let node = *decorations.nodes.get(decoration_id)?;
        Some(decorations.trees.get_node_range(self, node))
    }

    /// `getLineDecorations(lineNumber, ownerId, filterOutValidation, filterFontDecorations)`.
    pub fn get_line_decorations(&self, line_number: i32, owner_id: i32, filter_out_validation: bool, filter_font_decorations: bool) -> Vec<ModelDecoration> {
        if line_number < 1 || line_number > self.get_line_count() {
            return Vec::new();
        }
        self.get_lines_decorations(line_number, line_number, owner_id, filter_out_validation, filter_font_decorations, false)
    }

    /// `getLinesDecorations(startLineNumber, endLineNumber, ownerId, ...)`.
    pub fn get_lines_decorations(
        &self,
        start_line_number: i32,
        end_line_number: i32,
        owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        only_margin_decorations: bool,
    ) -> Vec<ModelDecoration> {
        let line_count = self.get_line_count();
        let start_line_number = line_count.min(start_line_number.max(1));
        let end_line_number = line_count.min(end_line_number.max(1));
        let end_column = self.get_line_max_column(end_line_number);
        let range = Range::new(start_line_number, 1, end_line_number, end_column);

        // O provedor de decorações de fontes por token ainda não existe.
        let mut decorations =
            self.get_decorations_in_range_impl(&range, owner_id, filter_out_validation, filter_font_decorations, only_margin_decorations);
        decorations.extend(self.decoration_provider.get_decorations_in_range(
            &range,
            Some(owner_id),
            filter_out_validation,
            filter_font_decorations,
            false,
        ));
        decorations
    }

    /// `getDecorationsInRange(range, ownerId, filterOutValidation, filterFontDecorations, onlyMinimapDecorations, onlyMarginDecorations)`.
    pub fn get_decorations_in_range(
        &self,
        range: &Range,
        owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        only_minimap_decorations: bool,
        only_margin_decorations: bool,
    ) -> Vec<ModelDecoration> {
        let validated_range = self.validate_range(range);
        let mut decorations =
            self.get_decorations_in_range_impl(&validated_range, owner_id, filter_out_validation, filter_font_decorations, only_margin_decorations);
        decorations.extend(self.decoration_provider.get_decorations_in_range(
            &validated_range,
            Some(owner_id),
            filter_out_validation,
            filter_font_decorations,
            only_minimap_decorations,
        ));
        decorations
    }

    /// `getOverviewRulerDecorations(ownerId, filterOutValidation, filterFontDecorations)`.
    pub fn get_overview_ruler_decorations(&self, owner_id: i32, filter_out_validation: bool, filter_font_decorations: bool) -> Vec<ModelDecoration> {
        self.decorations
            .borrow_mut()
            .trees
            .get_all(self, owner_id, filter_out_validation, filter_font_decorations, true, false)
    }

    /// `getInjectedTextDecorations(ownerId)`.
    pub fn get_injected_text_decorations(&self, owner_id: i32) -> Vec<ModelDecoration> {
        self.decorations.borrow_mut().trees.get_all_injected_text(self, owner_id)
    }

    /// `getCustomLineHeightsDecorations(ownerId)`.
    pub fn get_custom_line_heights_decorations(&self, owner_id: i32) -> Vec<ModelDecoration> {
        self.decorations.borrow_mut().trees.get_all_custom_line_heights(self, owner_id)
    }

    /// `getCustomLineHeightsDecorationsInRange(range, ownerId)`.
    pub fn get_custom_line_heights_decorations_in_range(&self, range: &Range, owner_id: i32) -> Vec<ModelDecoration> {
        let start = self.get_offset_at(range.get_start_position()) as i64;
        let end = self.get_offset_at(range.get_end_position()) as i64;
        self.decorations
            .borrow_mut()
            .trees
            .get_custom_line_heights_in_interval(self, start, end, owner_id)
    }

    /// `getLineInjectedText(lineNumber, ownerId)`.
    pub fn get_line_injected_text(&self, line_number: i32, owner_id: i32) -> Vec<LineInjectedText> {
        let (start_offset, end_offset) = {
            let buffer = self.buffer.borrow();
            let start_offset = buffer.get_offset_at(line_number, 1);
            (start_offset as i64, (start_offset + buffer.get_line_length(line_number)) as i64)
        };

        let result = self
            .decorations
            .borrow_mut()
            .trees
            .get_injected_text_in_interval(self, start_offset, end_offset, owner_id);
        LineInjectedText::from_decorations(&result)
            .into_iter()
            .filter(|text| text.line_number == line_number)
            .collect()
    }

    /// `getFontDecorationsInRange(range, ownerId)`.
    pub fn get_font_decorations_in_range(&self, range: &Range, owner_id: i32) -> Vec<ModelDecoration> {
        let (start_offset, end_offset) = {
            let buffer = self.buffer.borrow();
            (
                buffer.get_offset_at(range.start_line_number, range.start_column) as i64,
                buffer.get_offset_at(range.end_line_number, range.end_column) as i64,
            )
        };
        self.decorations
            .borrow_mut()
            .trees
            .get_font_decorations_in_interval(self, start_offset, end_offset, owner_id)
    }

    /// `getAllDecorations(ownerId, filterOutValidation, filterFontDecorations)`.
    pub fn get_all_decorations(&self, owner_id: i32, filter_out_validation: bool, filter_font_decorations: bool) -> Vec<ModelDecoration> {
        let mut result = self
            .decorations
            .borrow_mut()
            .trees
            .get_all(self, owner_id, filter_out_validation, filter_font_decorations, false, false);
        result.extend(self.decoration_provider.get_all_decorations(Some(owner_id), filter_out_validation, filter_font_decorations));
        result
    }

    /// `getAllMarginDecorations(ownerId)`.
    pub fn get_all_margin_decorations(&self, owner_id: i32) -> Vec<ModelDecoration> {
        self.decorations.borrow_mut().trees.get_all(self, owner_id, false, false, false, true)
    }

    /// `_getDecorationsInRange`.
    fn get_decorations_in_range_impl(
        &self,
        filter_range: &Range,
        filter_owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        only_margin_decorations: bool,
    ) -> Vec<ModelDecoration> {
        let (start_offset, end_offset) = {
            let buffer = self.buffer.borrow();
            (
                buffer.get_offset_at(filter_range.start_line_number, filter_range.start_column) as i64,
                buffer.get_offset_at(filter_range.end_line_number, filter_range.end_column) as i64,
            )
        };
        self.decorations.borrow_mut().trees.get_all_in_interval(
            self,
            start_offset,
            end_offset,
            filter_owner_id,
            filter_out_validation,
            filter_font_decorations,
            only_margin_decorations,
        )
    }

    /// A linha do começo e a do fim de um nó, para registrar o que as decorações de texto injetado, de altura
    /// de linha e de fonte afetam.
    fn node_range(&self, node: NodeId) -> Range {
        self.decorations.borrow_mut().trees.get_node_range(self, node)
    }

    fn node_options(&self, node: NodeId) -> Rc<ModelDecorationOptions> {
        self.decorations
            .borrow()
            .trees
            .arena
            .get(node)
            .options
            .clone()
            .expect("todo nó da árvore tem opções")
    }

    /// Registra, no emitter, as linhas que as opções do nó afetam (a contabilidade de texto injetado, altura
    /// de linha e fonte que `_changeDecorationImpl`, `_changeDecorationOptionsImpl` e
    /// `_deltaDecorationsImpl` fazem).
    fn record_affected_lines(&self, owner_id: i32, decoration_id: &str, options: &ModelDecorationOptions, range: &Range, line_height: Option<Option<f64>>) {
        if options.after.is_some() {
            self.on_did_change_decorations.record_line_affected_by_injected_text(range.end_line_number);
        }
        if options.before.is_some() {
            self.on_did_change_decorations.record_line_affected_by_injected_text(range.start_line_number);
        }
        if options.line_height.is_some() {
            self.on_did_change_decorations.record_line_affected_by_line_height_change(
                owner_id,
                decoration_id,
                range.start_line_number,
                line_height.unwrap_or(options.line_height),
            );
        }
        if options.affects_font.unwrap_or(false) {
            self.on_did_change_decorations
                .record_line_affected_by_font_change(owner_id, decoration_id, range.start_line_number);
        }
    }

    /// `_changeDecorationImpl`.
    fn change_decoration_impl(&self, owner_id: i32, decoration_id: &str, new_range: &Range) {
        let Some(node) = self.decorations.borrow().nodes.get(decoration_id).copied() else {
            return;
        };

        let options = self.node_options(node);
        let old_range = (options.after.is_some() || options.before.is_some() || options.line_height.is_some() || options.affects_font.unwrap_or(false))
            .then(|| self.node_range(node));
        if let Some(old_range) = &old_range {
            // O que as opções afetavam no intervalo antigo: a altura de linha some (`null`).
            self.record_affected_lines(owner_id, decoration_id, &options, old_range, Some(None));
        }

        let range = self.validate_range_relaxed(new_range);
        let (start_offset, end_offset) = {
            let buffer = self.buffer.borrow();
            (
                buffer.get_offset_at(range.start_line_number, range.start_column) as i64,
                buffer.get_offset_at(range.end_line_number, range.end_column) as i64,
            )
        };

        let version_id = self.get_version_id();
        {
            let mut decorations = self.decorations.borrow_mut();
            decorations.trees.delete(node);
            decorations.trees.arena.get_mut(node).reset(version_id, start_offset, end_offset, Some(range));
            decorations.trees.insert(node);
        }
        self.on_did_change_decorations.check_affected_and_fire(self, &options);

        self.record_affected_lines(owner_id, decoration_id, &options, &range, None);
    }

    /// `_changeDecorationOptionsImpl`.
    fn change_decoration_options_impl(&self, owner_id: i32, decoration_id: &str, options: Rc<ModelDecorationOptions>) {
        let Some(node) = self.decorations.borrow().nodes.get(decoration_id).copied() else {
            return;
        };
        let node_options = self.node_options(node);

        let node_was_in_overview_ruler = node_options.in_overview_ruler();
        let node_is_in_overview_ruler = options.in_overview_ruler();

        self.on_did_change_decorations.check_affected_and_fire(self, &node_options);
        self.on_did_change_decorations.check_affected_and_fire(self, &options);

        if node_options.after.is_some() || options.after.is_some() {
            let node_range = self.node_range(node);
            self.on_did_change_decorations.record_line_affected_by_injected_text(node_range.end_line_number);
        }
        if node_options.before.is_some() || options.before.is_some() {
            let node_range = self.node_range(node);
            self.on_did_change_decorations.record_line_affected_by_injected_text(node_range.start_line_number);
        }
        if node_options.line_height.is_some() || options.line_height.is_some() {
            let node_range = self.node_range(node);
            self.on_did_change_decorations
                .record_line_affected_by_line_height_change(owner_id, decoration_id, node_range.start_line_number, options.line_height);
        }
        if node_options.affects_font.unwrap_or(false) || options.affects_font.unwrap_or(false) {
            let node_range = self.node_range(node);
            self.on_did_change_decorations
                .record_line_affected_by_font_change(owner_id, decoration_id, node_range.start_line_number);
        }

        let moved_in_overview_ruler = node_was_in_overview_ruler != node_is_in_overview_ruler;
        let changed_whether_injected_text = options.is_injected_text() != node_options.is_injected_text();
        let mut decorations = self.decorations.borrow_mut();
        if moved_in_overview_ruler || changed_whether_injected_text {
            decorations.trees.delete(node);
            decorations.trees.arena.get_mut(node).set_options(options);
            decorations.trees.insert(node);
        } else {
            decorations.trees.arena.get_mut(node).set_options(options);
        }
    }

    /// `_deltaDecorationsImpl`.
    fn delta_decorations_impl(&self, owner_id: i32, old_decorations_ids: &[String], new_decorations: &[ModelDeltaDecoration], suppress_events: bool) -> Vec<String> {
        let version_id = self.get_version_id();

        let old_decorations_len = old_decorations_ids.len();
        let mut old_decoration_index = 0usize;

        let new_decorations_len = new_decorations.len();
        let mut new_decoration_index = 0usize;

        self.on_did_change_decorations.begin_deferred_emit();
        let mut result: Vec<String> = Vec::with_capacity(new_decorations_len);
        while old_decoration_index < old_decorations_len || new_decoration_index < new_decorations_len {
            let mut node: Option<NodeId> = None;

            if old_decoration_index < old_decorations_len {
                // (1) pega um nó antigo
                loop {
                    let decoration_id = &old_decorations_ids[old_decoration_index];
                    old_decoration_index += 1;
                    node = self.decorations.borrow().nodes.get(decoration_id).copied();
                    if node.is_some() || old_decoration_index >= old_decorations_len {
                        break;
                    }
                }

                // (2) tira o nó da árvore (se existir)
                if let Some(node) = node {
                    let node_options = self.node_options(node);
                    let decoration_id = self.decorations.borrow().trees.arena.get(node).id.clone();
                    if node_options.after.is_some() || node_options.before.is_some() || node_options.line_height.is_some() || node_options.affects_font.unwrap_or(false) {
                        let node_range = self.node_range(node);
                        self.record_affected_lines(owner_id, &decoration_id, &node_options, &node_range, Some(None));
                    }
                    self.decorations.borrow_mut().trees.delete(node);

                    if !suppress_events {
                        self.on_did_change_decorations.check_affected_and_fire(self, &node_options);
                    }
                }
            }

            if new_decoration_index < new_decorations_len {
                // (3) cria um nó novo se for preciso
                let node = match node {
                    Some(node) => node,
                    None => {
                        self.last_decoration_id.set(self.last_decoration_id.get() + 1);
                        let decoration_id = format!("{};{}", self.instance_id, self.last_decoration_id.get());
                        let mut decorations = self.decorations.borrow_mut();
                        let node = decorations.trees.arena.alloc(decoration_id.clone(), 0, 0);
                        decorations.nodes.insert(decoration_id, node);
                        node
                    }
                };

                // (4) inicializa o nó
                let new_decoration = &new_decorations[new_decoration_index];
                let range = self.validate_range_relaxed(&new_decoration.range);
                let options = new_decoration.options.clone();
                let (start_offset, end_offset) = {
                    let buffer = self.buffer.borrow();
                    (
                        buffer.get_offset_at(range.start_line_number, range.start_column) as i64,
                        buffer.get_offset_at(range.end_line_number, range.end_column) as i64,
                    )
                };

                let decoration_id = {
                    let mut decorations = self.decorations.borrow_mut();
                    let n = decorations.trees.arena.get_mut(node);
                    n.owner_id = owner_id;
                    n.reset(version_id, start_offset, end_offset, Some(range));
                    n.set_options(options.clone());
                    n.id.clone()
                };

                self.record_affected_lines(owner_id, &decoration_id, &options, &range, None);
                if !suppress_events {
                    self.on_did_change_decorations.check_affected_and_fire(self, &options);
                }

                let mut decorations = self.decorations.borrow_mut();
                decorations.trees.insert(node);

                result.push(decoration_id);

                new_decoration_index += 1;
            } else if let Some(node) = node {
                let mut decorations = self.decorations.borrow_mut();
                let id = decorations.trees.arena.get(node).id.clone();
                decorations.nodes.remove(&id);
                decorations.trees.arena.free(node);
            }
        }

        self.on_did_change_decorations.end_deferred_emit(self);
        result
    }

    // #endregion
}

impl crate::bracket_pairs_text_model_part::bracket_pairs_tree::tokenizer::ITokenizerSource for TextModel {
    fn get_value(&self) -> JsString {
        TextModel::get_value(self)
    }

    fn get_line_count(&self) -> i32 {
        <TextModel as CursorSimpleModel>::get_line_count(self)
    }

    fn get_line_length(&self, line_number: i32) -> usize {
        TextModel::get_line_length(self, line_number)
    }

    fn get_line_tokens(&self, line_number: i32) -> crate::line_tokens::LineTokens {
        self.tokenization().get_line_tokens(line_number)
    }
}

/// `getLineTokensWithInjections(tokens, injectionOptions, injectionOffsets)`: os tokens da linha com os
/// textos injetados inseridos. Sem tokens próprios, o texto injetado entra com o metadado de token padrão.
pub fn get_line_tokens_with_injections(
    tokens: &crate::line_tokens::LineTokens,
    injection_options: Option<&[InjectedTextOptions]>,
    injection_offsets: Option<&[i32]>,
) -> crate::line_tokens::LineTokens {
    let Some(injection_offsets) = injection_offsets else {
        return tokens.clone();
    };
    let injection_options = injection_options.expect("há deslocamentos de injeção, então há opções");

    let mut tokens_to_insert: Vec<crate::line_tokens::InsertToken> = Vec::new();
    for (offset, options) in injection_offsets.iter().zip(injection_options) {
        let content: Vec<u16> = options.content.encode_utf16().collect();
        match &options.tokens {
            Some(token_array) => token_array.for_each(|range, info| {
                tokens_to_insert.push(crate::line_tokens::InsertToken {
                    offset: *offset,
                    text: JsString::from_units(range.substring(&content).to_vec()),
                    token_metadata: info.metadata,
                });
            }),
            None => tokens_to_insert.push(crate::line_tokens::InsertToken {
                offset: *offset,
                text: JsString::from_units(content),
                token_metadata: crate::line_tokens::DEFAULT_TOKEN_METADATA,
            }),
        }
    }
    tokens.with_inserted(&tokens_to_insert)
}

/// O `ITextModel` do original atende o `ICursorSimpleModel` por tipagem estrutural; aqui os métodos moram no trait.
impl CursorSimpleModel for TextModel {
    /// `getLineCount()`.
    fn get_line_count(&self) -> i32 {
        self.assert_not_disposed();
        self.buffer.borrow().get_line_count() as i32
    }

    /// `getLineContent(lineNumber)`.
    fn get_line_content(&self, line_number: i32) -> JsString {
        self.assert_not_disposed();
        self.assert_line_number(line_number);
        self.buffer.borrow().get_line_content(line_number)
    }

    /// `getLineMinColumn(lineNumber)`.
    fn get_line_min_column(&self, _line_number: i32) -> i32 {
        self.assert_not_disposed();
        1
    }

    /// `getLineMaxColumn(lineNumber)`.
    fn get_line_max_column(&self, line_number: i32) -> i32 {
        self.assert_not_disposed();
        self.assert_line_number(line_number);
        self.buffer.borrow().get_line_length(line_number) as i32 + 1
    }

    /// `getLineFirstNonWhitespaceColumn(lineNumber)`.
    fn get_line_first_non_whitespace_column(&self, line_number: i32) -> i32 {
        self.assert_not_disposed();
        self.assert_line_number(line_number);
        self.buffer.borrow().get_line_first_non_whitespace_column(line_number) as i32
    }

    /// `getLineLastNonWhitespaceColumn(lineNumber)`.
    fn get_line_last_non_whitespace_column(&self, line_number: i32) -> i32 {
        self.assert_not_disposed();
        self.assert_line_number(line_number);
        self.buffer.borrow().get_line_last_non_whitespace_column(line_number) as i32
    }

    /// `getLineIndentColumn(lineNumber)`: a coluna em que a indentação termina.
    fn get_line_indent_column(&self, line_number: i32) -> i32 {
        // As colunas começam em 1.
        indent_of_line(&self.get_line_content(line_number)) as i32 + 1
    }

    /// `normalizePosition(position, affinity)`.
    fn normalize_position(&self, position: Position, _affinity: PositionAffinity) -> Position {
        position
    }
}
