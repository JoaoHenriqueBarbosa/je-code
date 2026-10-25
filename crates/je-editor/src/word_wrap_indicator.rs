//! Porte de `src/vs/editor/browser/viewParts/wordWrapIndicator/wordWrapIndicator.ts`: o `WordWrapIndicatorOverlay`,
//! que pinta um glifo pequeno na coluna de quebra de cada linha da visão quebrada por soft wrap, para distinguir a
//! quebra de uma quebra de linha de verdade.
//!
//! Divergências:
//! - `render` devolve o `<div class="wwi" style="left:..px;height:..px;">↩</div>` no original. A UI é nativa, sem
//!   DOM: o resultado de cada linha é um [`WordWrapIndicator`] (`left`, `height` e o glifo, o que o `style` e o
//!   conteúdo do `div` dizem) e a linha sem indicador é `None` (a string vazia). O `wordWrapIndicator.css` é só
//!   especificação de estilo (cor, fonte do glifo), que o pintor nativo lê ao desenhar.
//! - `ViewEventHandler extends Disposable`: o descarte é o [`IDisposable`] à parte (ver `view_events.rs`). Como o
//!   construtor se registra no `ViewContext` (`addEventHandler(this)`), [`WordWrapIndicatorOverlay::new`] devolve
//!   um `Rc<Self>` e guarda uma referência fraca a si mesmo para `removeEventHandler(this)` no `dispose`.
//! - O estado mutável (`_options`, `_renderResult`) fica em `RefCell`, porque o despachante compartilha o handler.
//! - `onConfigurationChanged` recebe o evento, que o original também ignora (só relê a configuração do contexto).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::IDisposable;

use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::simple_keys::WORD_WRAP_INDICATOR;
use crate::config::editor_options::wrapping_info::WRAPPING_INFO;
use crate::cursor_common::EditorConfiguration;
use crate::dynamic_view_overlay::DynamicViewOverlay;
use crate::rendering_context::RenderingContext;
use crate::view_context::ViewContext;
use crate::view_events::{
    ShouldRenderState, ViewConfigurationChangedEvent, ViewDecorationsChangedEvent, ViewEventHandler, ViewFlushedEvent,
    ViewLineMappingChangedEvent, ViewLinesChangedEvent, ViewLinesDeletedEvent, ViewLinesInsertedEvent,
    ViewScrollChangedEvent, ViewTokensChangedEvent, ViewZonesChangedEvent,
};
use crate::view_model_types::IViewModel;

/// U+21A9 (LEFTWARDS ARROW WITH HOOK).
pub const WORD_WRAP_INDICATOR_CHAR: char = '\u{21A9}';

/// O indicador de uma linha da visão: onde e com que altura o glifo é pintado.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WordWrapIndicator {
    /// Deslocamento horizontal do glifo (a coluna de quebra vezes a largura típica de um caractere).
    pub left: f64,
    /// A altura da linha da visão.
    pub height: f64,
    /// O glifo (sempre [`WORD_WRAP_INDICATOR_CHAR`]).
    pub glyph: char,
}

/// O `WordWrapIndicatorOverlay`.
pub struct WordWrapIndicatorOverlay<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> {
    context: Rc<ViewContext<VM>>,
    options: RefCell<WordWrapIndicatorOptions>,
    render_result: RefCell<Option<Vec<Option<WordWrapIndicator>>>>,
    should_render_state: ShouldRenderState,
    this: Weak<Self>,
}

impl<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> WordWrapIndicatorOverlay<VM> {
    pub fn new(context: Rc<ViewContext<VM>>) -> Rc<Self> {
        let options = WordWrapIndicatorOptions::new(&*context.configuration);
        let overlay = Rc::new_cyclic(|this| Self {
            context,
            options: RefCell::new(options),
            render_result: RefCell::new(None),
            should_render_state: ShouldRenderState::default(),
            this: this.clone(),
        });
        let handler: Rc<dyn ViewEventHandler> = overlay.clone();
        overlay.context.add_event_handler(handler);
        overlay
    }

    /// Se a parte pinta alguma coisa. Enquanto não pinta, todo evento da visão é respondido com `false`, então a
    /// visão nunca é chamada a repintar por causa desta parte.
    fn is_enabled(&self) -> bool {
        let options = self.options.borrow();
        options.word_wrap_indicator && options.is_wrapping
    }

    fn render_line(&self, ctx: &RenderingContext<VM>, line_number: i32) -> Option<WordWrapIndicator> {
        if !ctx.viewport_data.get_view_line_continues_with_wrapped_line(line_number) {
            // A linha termina com uma quebra de linha de verdade, ou é a última linha do modelo.
            return None;
        }
        Some(WordWrapIndicator {
            left: self.options.borrow().indicator_left,
            height: ctx.get_line_height_for_line_number(line_number),
            glyph: WORD_WRAP_INDICATOR_CHAR,
        })
    }
}

impl<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> IDisposable for WordWrapIndicatorOverlay<VM> {
    fn dispose(&self) {
        if let Some(this) = self.this.upgrade() {
            let handler: Rc<dyn ViewEventHandler> = this;
            self.context.remove_event_handler(&handler);
        }
        *self.render_result.borrow_mut() = None;
    }
}

impl<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> ViewEventHandler for WordWrapIndicatorOverlay<VM> {
    fn should_render_state(&self) -> &ShouldRenderState {
        &self.should_render_state
    }

    // --- begin event handlers

    fn on_configuration_changed(&self, _e: &ViewConfigurationChangedEvent) -> bool {
        let was_enabled = self.is_enabled();
        let new_options = WordWrapIndicatorOptions::new(&*self.context.configuration);
        let options_changed = *self.options.borrow() != new_options;
        *self.options.borrow_mut() = new_options;
        (was_enabled || self.is_enabled()) && options_changed
    }
    fn on_decorations_changed(&self, _e: &ViewDecorationsChangedEvent) -> bool {
        self.is_enabled()
    }
    fn on_flushed(&self, _e: &ViewFlushedEvent) -> bool {
        self.is_enabled()
    }
    fn on_line_mapping_changed(&self, _e: &ViewLineMappingChangedEvent) -> bool {
        // Quais linhas continuam com uma linha quebrada é decidido pelo mapeamento de linhas.
        self.is_enabled()
    }
    fn on_lines_changed(&self, _e: &ViewLinesChangedEvent) -> bool {
        self.is_enabled()
    }
    fn on_lines_deleted(&self, _e: &ViewLinesDeletedEvent) -> bool {
        self.is_enabled()
    }
    fn on_lines_inserted(&self, _e: &ViewLinesInsertedEvent) -> bool {
        self.is_enabled()
    }
    fn on_scroll_changed(&self, e: &ViewScrollChangedEvent) -> bool {
        self.is_enabled() && e.scroll_top_changed
    }
    fn on_tokens_changed(&self, _e: &ViewTokensChangedEvent) -> bool {
        false
    }
    fn on_zones_changed(&self, _e: &ViewZonesChangedEvent) -> bool {
        self.is_enabled()
    }
    // --- end event handlers
}

impl<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> DynamicViewOverlay<VM> for WordWrapIndicatorOverlay<VM> {
    type Output = Option<WordWrapIndicator>;

    fn prepare_render(&self, ctx: &RenderingContext<VM>) {
        if !self.is_enabled() {
            *self.render_result.borrow_mut() = None;
            return;
        }
        let viewport_data = &ctx.viewport_data;
        let result = (viewport_data.start_line_number..=viewport_data.end_line_number)
            .map(|line_number| self.render_line(ctx, line_number))
            .collect();
        *self.render_result.borrow_mut() = Some(result);
    }

    fn render(&self, start_line_number: i32, line_number: i32) -> Option<WordWrapIndicator> {
        let render_result = self.render_result.borrow();
        let render_result = render_result.as_ref()?;
        let line_index = usize::try_from(line_number - start_line_number).ok()?;
        render_result.get(line_index).copied().flatten()
    }
}

/// O subconjunto da configuração do editor que a parte lê, copiado para distinguir uma mudança de configuração de
/// uma que deixa o resultado pintado intacto.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WordWrapIndicatorOptions {
    word_wrap_indicator: bool,
    is_wrapping: bool,
    indicator_left: f64,
}

impl WordWrapIndicatorOptions {
    fn new(config: &dyn EditorConfiguration) -> Self {
        let options = config.options();
        let font_info = options.get(FONT_INFO);
        let wrapping_column = options.get(WRAPPING_INFO).wrapping_column;
        Self {
            word_wrap_indicator: *options.get(WORD_WRAP_INDICATOR),
            is_wrapping: wrapping_column != -1.0,
            indicator_left: wrapping_column * font_info.typical_halfwidth_character_width,
        }
    }
}
