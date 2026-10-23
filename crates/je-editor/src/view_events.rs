//! Porte de `src/vs/editor/common/viewEvents.ts`: os eventos que o view model manda para a visão, e o
//! `ViewEventHandler` (de `viewEventHandler.ts`), a base de quem os recebe.
//!
//! Decisões de tradução:
//! - Cada classe `ViewXxxEvent` vira uma struct, e o union type `ViewEvent` vira o `enum` [`ViewEvent`], com
//!   uma variante por classe. O campo `type` do original é [`ViewEvent::event_type`].
//! - `ViewEventHandler extends Disposable` vira o trait [`ViewEventHandler`]. O estado `_shouldRender` mora
//!   em quem implementa, num [`ShouldRenderState`] (que nasce verdadeiro, como o construtor do original),
//!   devolvido por [`ViewEventHandler::should_render_state`]. Os métodos têm `&self`, porque o despachante
//!   compartilha o handler (`Rc<dyn ViewEventHandler>`) e o handler muda o próprio estado por `Cell`.
//!   O descarte (`Disposable`) fica por conta de quem implementa o trait, com [`IDisposable`] à parte.
//! - `ViewConfigurationChangedEvent._source` é um `Rc<ConfigurationChangedEvent>`: o evento de configuração
//!   é compartilhado entre os eventos que o carregam, como o mesmo objeto no original.
//! - `ViewThemeChangedEvent.theme` é o `IColorTheme`, aqui o tema do porte (`je_theme::ColorTheme`).
//! - Os números de linha são `i32`, como no resto do crate.

use je_theme::ColorTheme;
use std::cell::Cell;
use std::rc::Rc;

use je_base::scrollable::ScrollEvent;

use crate::config::editor_options::{ConfigurationChangedEvent, EditorOption};
use crate::editor_common::ScrollType;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model_events::ModelDecorationsChangedEvent;
pub use crate::view_model_types::VerticalRevealType;
use crate::view_model_types::CursorChangeReason;

/// `ViewEventType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewEventType {
    ViewCompositionStart,
    ViewCompositionEnd,
    ViewConfigurationChanged,
    ViewCursorStateChanged,
    ViewDecorationsChanged,
    ViewFlushed,
    ViewFocusChanged,
    ViewLanguageConfigurationChanged,
    ViewLineMappingChanged,
    ViewLinesChanged,
    ViewLinesDeleted,
    ViewLinesInserted,
    ViewRevealRangeRequest,
    ViewScrollChanged,
    ViewThemeChanged,
    ViewTokensChanged,
    ViewTokensColorsChanged,
    ViewZonesChanged,
}

/// `ViewCompositionStartEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewCompositionStartEvent;

/// `ViewCompositionEndEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewCompositionEndEvent;

/// `ViewConfigurationChangedEvent`.
#[derive(Clone)]
pub struct ViewConfigurationChangedEvent {
    /// `_source`.
    pub source: Rc<ConfigurationChangedEvent>,
}

impl ViewConfigurationChangedEvent {
    pub fn new(source: ConfigurationChangedEvent) -> Self {
        Self { source: Rc::new(source) }
    }

    pub fn has_changed(&self, id: EditorOption) -> bool {
        self.source.has_changed(id)
    }
}

/// `ViewCursorStateChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewCursorStateChangedEvent {
    pub selections: Vec<Selection>,
    pub model_selections: Vec<Selection>,
    pub reason: CursorChangeReason,
}

impl ViewCursorStateChangedEvent {
    pub fn new(selections: Vec<Selection>, model_selections: Vec<Selection>, reason: CursorChangeReason) -> Self {
        Self { selections, model_selections, reason }
    }
}

/// `ViewDecorationsChangedEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewDecorationsChangedEvent {
    pub affects_minimap: bool,
    pub affects_overview_ruler: bool,
    pub affects_glyph_margin: bool,
    pub affects_line_number: bool,
}

impl ViewDecorationsChangedEvent {
    /// Sem origem (`null`), tudo é afetado.
    pub fn new(source: Option<&ModelDecorationsChangedEvent>) -> Self {
        match source {
            Some(source) => Self {
                affects_minimap: source.affects_minimap,
                affects_overview_ruler: source.affects_overview_ruler,
                affects_glyph_margin: source.affects_glyph_margin,
                affects_line_number: source.affects_line_number,
            },
            None => Self { affects_minimap: true, affects_overview_ruler: true, affects_glyph_margin: true, affects_line_number: true },
        }
    }
}

/// `ViewFlushedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewFlushedEvent;

/// `ViewFocusChangedEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewFocusChangedEvent {
    pub is_focused: bool,
}

impl ViewFocusChangedEvent {
    pub fn new(is_focused: bool) -> Self {
        Self { is_focused }
    }
}

/// `ViewLanguageConfigurationEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewLanguageConfigurationEvent;

/// `ViewLineMappingChangedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewLineMappingChangedEvent;

/// `ViewLinesChangedEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewLinesChangedEvent {
    /// A primeira linha que mudou.
    pub from_line_number: i32,
    /// O número de linhas que mudaram.
    pub count: i32,
}

impl ViewLinesChangedEvent {
    pub fn new(from_line_number: i32, count: i32) -> Self {
        Self { from_line_number, count }
    }
}

/// `ViewLinesDeletedEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewLinesDeletedEvent {
    /// Em que linha a remoção começou (inclusive).
    pub from_line_number: i32,
    /// Em que linha a remoção terminou (inclusive).
    pub to_line_number: i32,
}

impl ViewLinesDeletedEvent {
    pub fn new(from_line_number: i32, to_line_number: i32) -> Self {
        Self { from_line_number, to_line_number }
    }
}

/// `ViewLinesInsertedEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewLinesInsertedEvent {
    /// Antes de que linha a inserção começou.
    pub from_line_number: i32,
    /// `to_line_number - from_line_number + 1` é o número de linhas inseridas.
    pub to_line_number: i32,
}

impl ViewLinesInsertedEvent {
    pub fn new(from_line_number: i32, to_line_number: i32) -> Self {
        Self { from_line_number, to_line_number }
    }
}

/// `ViewRevealRangeRequestEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewRevealRangeRequestEvent {
    /// Origem da chamada que causou o evento (`string | null | undefined`).
    pub source: Option<String>,
    /// Reduz a revelação ao mínimo (por exemplo, não rola se a caixa já está visível perto da borda).
    pub minimal_reveal: bool,
    /// Intervalo a revelar.
    pub range: Option<Range>,
    /// Seleções a revelar.
    pub selections: Option<Vec<Selection>>,
    /// A estratégia da revelação vertical.
    pub vertical_type: VerticalRevealType,
    /// Verdadeiro: revela na horizontal e na vertical. Falso: só na vertical.
    pub reveal_horizontal: bool,
    /// O tipo de rolagem.
    pub scroll_type: ScrollType,
}

impl ViewRevealRangeRequestEvent {
    pub fn new(
        source: Option<String>,
        minimal_reveal: bool,
        range: Option<Range>,
        selections: Option<Vec<Selection>>,
        vertical_type: VerticalRevealType,
        reveal_horizontal: bool,
        scroll_type: ScrollType,
    ) -> Self {
        Self { source, minimal_reveal, range, selections, vertical_type, reveal_horizontal, scroll_type }
    }
}

/// `ViewScrollChangedEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewScrollChangedEvent {
    pub scroll_width: f64,
    pub scroll_left: f64,
    pub scroll_height: f64,
    pub scroll_top: f64,

    pub scroll_width_changed: bool,
    pub scroll_left_changed: bool,
    pub scroll_height_changed: bool,
    pub scroll_top_changed: bool,
}

impl ViewScrollChangedEvent {
    pub fn new(source: &ScrollEvent) -> Self {
        Self {
            scroll_width: source.scroll_width,
            scroll_left: source.scroll_left,
            scroll_height: source.scroll_height,
            scroll_top: source.scroll_top,

            scroll_width_changed: source.scroll_width_changed,
            scroll_left_changed: source.scroll_left_changed,
            scroll_height_changed: source.scroll_height_changed,
            scroll_top_changed: source.scroll_top_changed,
        }
    }
}

/// `ViewThemeChangedEvent`.
#[derive(Clone, Debug)]
pub struct ViewThemeChangedEvent {
    pub theme: Rc<ColorTheme>,
}

impl ViewThemeChangedEvent {
    pub fn new(theme: Rc<ColorTheme>) -> Self {
        Self { theme }
    }
}

/// O intervalo de linhas de [`ViewTokensChangedEvent`] (`{ fromLineNumber, toLineNumber }`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewTokensChangedRange {
    /// A linha inicial do intervalo.
    pub from_line_number: i32,
    /// A linha final do intervalo.
    pub to_line_number: i32,
}

/// `ViewTokensChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewTokensChangedEvent {
    pub ranges: Vec<ViewTokensChangedRange>,
}

impl ViewTokensChangedEvent {
    pub fn new(ranges: Vec<ViewTokensChangedRange>) -> Self {
        Self { ranges }
    }
}

/// `ViewTokensColorsChangedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewTokensColorsChangedEvent;

/// `ViewZonesChangedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewZonesChangedEvent;

/// `ViewEvent`: o union type das classes acima.
#[derive(Clone)]
pub enum ViewEvent {
    CompositionStart(ViewCompositionStartEvent),
    CompositionEnd(ViewCompositionEndEvent),
    ConfigurationChanged(ViewConfigurationChangedEvent),
    CursorStateChanged(ViewCursorStateChangedEvent),
    DecorationsChanged(ViewDecorationsChangedEvent),
    Flushed(ViewFlushedEvent),
    FocusChanged(ViewFocusChangedEvent),
    LanguageConfigurationChanged(ViewLanguageConfigurationEvent),
    LineMappingChanged(ViewLineMappingChangedEvent),
    LinesChanged(ViewLinesChangedEvent),
    LinesDeleted(ViewLinesDeletedEvent),
    LinesInserted(ViewLinesInsertedEvent),
    RevealRangeRequest(ViewRevealRangeRequestEvent),
    ScrollChanged(ViewScrollChangedEvent),
    ThemeChanged(ViewThemeChangedEvent),
    TokensChanged(ViewTokensChangedEvent),
    TokensColorsChanged(ViewTokensColorsChangedEvent),
    ZonesChanged(ViewZonesChangedEvent),
}

impl ViewEvent {
    /// O campo `type` do evento.
    pub fn event_type(&self) -> ViewEventType {
        match self {
            ViewEvent::CompositionStart(_) => ViewEventType::ViewCompositionStart,
            ViewEvent::CompositionEnd(_) => ViewEventType::ViewCompositionEnd,
            ViewEvent::ConfigurationChanged(_) => ViewEventType::ViewConfigurationChanged,
            ViewEvent::CursorStateChanged(_) => ViewEventType::ViewCursorStateChanged,
            ViewEvent::DecorationsChanged(_) => ViewEventType::ViewDecorationsChanged,
            ViewEvent::Flushed(_) => ViewEventType::ViewFlushed,
            ViewEvent::FocusChanged(_) => ViewEventType::ViewFocusChanged,
            ViewEvent::LanguageConfigurationChanged(_) => ViewEventType::ViewLanguageConfigurationChanged,
            ViewEvent::LineMappingChanged(_) => ViewEventType::ViewLineMappingChanged,
            ViewEvent::LinesChanged(_) => ViewEventType::ViewLinesChanged,
            ViewEvent::LinesDeleted(_) => ViewEventType::ViewLinesDeleted,
            ViewEvent::LinesInserted(_) => ViewEventType::ViewLinesInserted,
            ViewEvent::RevealRangeRequest(_) => ViewEventType::ViewRevealRangeRequest,
            ViewEvent::ScrollChanged(_) => ViewEventType::ViewScrollChanged,
            ViewEvent::ThemeChanged(_) => ViewEventType::ViewThemeChanged,
            ViewEvent::TokensChanged(_) => ViewEventType::ViewTokensChanged,
            ViewEvent::TokensColorsChanged(_) => ViewEventType::ViewTokensColorsChanged,
            ViewEvent::ZonesChanged(_) => ViewEventType::ViewZonesChanged,
        }
    }
}

/// O `_shouldRender` de `ViewEventHandler`: nasce verdadeiro, como no construtor do original.
#[derive(Debug)]
pub struct ShouldRenderState(Cell<bool>);

impl Default for ShouldRenderState {
    fn default() -> Self {
        Self(Cell::new(true))
    }
}

impl ShouldRenderState {
    pub fn get(&self) -> bool {
        self.0.get()
    }

    pub fn set(&self, value: bool) {
        self.0.set(value);
    }
}

/// `ViewEventHandler`: recebe os eventos da visão. Cada `on_xxx` devolve se o handler precisa de um novo
/// render por causa do evento.
pub trait ViewEventHandler {
    /// O estado `_shouldRender`, guardado por quem implementa o trait.
    fn should_render_state(&self) -> &ShouldRenderState;

    fn should_render(&self) -> bool {
        self.should_render_state().get()
    }

    fn force_should_render(&self) {
        self.should_render_state().set(true);
    }

    /// `setShouldRender` (protegido no original).
    fn set_should_render(&self) {
        self.should_render_state().set(true);
    }

    fn on_did_render(&self) {
        self.should_render_state().set(false);
    }

    // --- begin event handlers

    fn on_composition_start(&self, _e: &ViewCompositionStartEvent) -> bool {
        false
    }
    fn on_composition_end(&self, _e: &ViewCompositionEndEvent) -> bool {
        false
    }
    fn on_configuration_changed(&self, _e: &ViewConfigurationChangedEvent) -> bool {
        false
    }
    fn on_cursor_state_changed(&self, _e: &ViewCursorStateChangedEvent) -> bool {
        false
    }
    fn on_decorations_changed(&self, _e: &ViewDecorationsChangedEvent) -> bool {
        false
    }
    fn on_flushed(&self, _e: &ViewFlushedEvent) -> bool {
        false
    }
    fn on_focus_changed(&self, _e: &ViewFocusChangedEvent) -> bool {
        false
    }
    fn on_language_configuration_changed(&self, _e: &ViewLanguageConfigurationEvent) -> bool {
        false
    }
    fn on_line_mapping_changed(&self, _e: &ViewLineMappingChangedEvent) -> bool {
        false
    }
    fn on_lines_changed(&self, _e: &ViewLinesChangedEvent) -> bool {
        false
    }
    fn on_lines_deleted(&self, _e: &ViewLinesDeletedEvent) -> bool {
        false
    }
    fn on_lines_inserted(&self, _e: &ViewLinesInsertedEvent) -> bool {
        false
    }
    fn on_reveal_range_request(&self, _e: &ViewRevealRangeRequestEvent) -> bool {
        false
    }
    fn on_scroll_changed(&self, _e: &ViewScrollChangedEvent) -> bool {
        false
    }
    fn on_theme_changed(&self, _e: &ViewThemeChangedEvent) -> bool {
        false
    }
    fn on_tokens_changed(&self, _e: &ViewTokensChangedEvent) -> bool {
        false
    }
    fn on_tokens_colors_changed(&self, _e: &ViewTokensColorsChangedEvent) -> bool {
        false
    }
    fn on_zones_changed(&self, _e: &ViewZonesChangedEvent) -> bool {
        false
    }

    // --- end event handlers

    fn handle_events(&self, events: &[ViewEvent]) {
        let mut should_render = false;

        for event in events {
            let handled = match event {
                ViewEvent::CompositionStart(e) => self.on_composition_start(e),
                ViewEvent::CompositionEnd(e) => self.on_composition_end(e),
                ViewEvent::ConfigurationChanged(e) => self.on_configuration_changed(e),
                ViewEvent::CursorStateChanged(e) => self.on_cursor_state_changed(e),
                ViewEvent::DecorationsChanged(e) => self.on_decorations_changed(e),
                ViewEvent::Flushed(e) => self.on_flushed(e),
                ViewEvent::FocusChanged(e) => self.on_focus_changed(e),
                ViewEvent::LanguageConfigurationChanged(e) => self.on_language_configuration_changed(e),
                ViewEvent::LineMappingChanged(e) => self.on_line_mapping_changed(e),
                ViewEvent::LinesChanged(e) => self.on_lines_changed(e),
                ViewEvent::LinesDeleted(e) => self.on_lines_deleted(e),
                ViewEvent::LinesInserted(e) => self.on_lines_inserted(e),
                ViewEvent::RevealRangeRequest(e) => self.on_reveal_range_request(e),
                ViewEvent::ScrollChanged(e) => self.on_scroll_changed(e),
                ViewEvent::TokensChanged(e) => self.on_tokens_changed(e),
                ViewEvent::ThemeChanged(e) => self.on_theme_changed(e),
                ViewEvent::TokensColorsChanged(e) => self.on_tokens_colors_changed(e),
                ViewEvent::ZonesChanged(e) => self.on_zones_changed(e),
            };
            if handled {
                should_render = true;
            }
        }

        if should_render {
            self.should_render_state().set(true);
        }
    }
}
