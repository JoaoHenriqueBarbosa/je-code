//! Porte de `src/vs/editor/common/viewLayout/viewLayout.ts`: o layout do editor (dimensões de rolagem, altura e
//! largura do conteúdo, viewport) sobre o [`LinesLayout`] e o [`Scrollable`].
//!
//! Decisões de tradução:
//! - `EditorScrollDimensions` guarda `f64` já reduzidos por `| 0` (`ToInt32`), como o `Viewport`, e é `Copy`.
//! - `EditorScrollable` guarda as dimensões num `Cell` e o `Scrollable` num `Rc`; nenhum empréstimo atravessa uma
//!   chamada que dispara evento, porque os ouvintes reentram no layout (`getScrollWidth` e companhia). Os métodos
//!   do original que só repassavam ao `Scrollable` (`validateScrollPosition`, `setScrollPositionNow` etc.) não
//!   existem nele: [`ViewLayout`] chama [`EditorScrollable::get_scrollable`] direto (regra DRY do projeto).
//! - `ViewLayout` é compartilhado (`Rc`) e implementa [`IViewLayout`] com `&self`: o [`LinesLayout`] fica num
//!   `RefCell` e os campos escalares em `Cell`. `changeWhitespace` empresta o [`LinesLayout`] só durante cada
//!   operação do acessor, então o callback pode ler o layout e rolar o editor (o `keepCursorStable` do ghost text faz
//!   isso), vendo o estado ainda sem as mudanças pendentes, como no original. `changeSpecialLineHeights` segura o
//!   empréstimo durante o callback: um callback dele que reentre no layout dá pânico.
//! - `onDidScroll` e `onDidContentSizeChange` (campos de `Event` no original) são os métodos
//!   [`ViewLayout::on_did_scroll`] e [`ViewLayout::on_did_content_size_change`], porque o `Event` do porte não
//!   é clonável.
//! - `includeViewZones: boolean = false` é um `bool` obrigatório nos métodos próprios, e `Option<bool>` nos que
//!   implementam [`IViewLayout`] (que declara o parâmetro opcional); `None` vale `false`.
//! - `IEditorConfiguration` é o trait [`EditorConfiguration`] de `cursor_common`, que só traz `options`.
//!   `onDidChangeConfiguration` e o resto do original não são lidos por este arquivo.
//! - `saveState()` devolve a struct [`ViewLayoutState`].
//! - `Math.max(a, b, c)` de `_computeContentWidth` usa `f64::max`, que ignora `NaN` em vez de propagá-lo; as
//!   três parcelas vêm de inteiros e de opções numéricas, sem `NaN` em uso normal.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::scrollable::{
    INewScrollDimensions, INewScrollPosition, IScrollPosition, IScrollableOptions, ScheduleAtNextAnimationFrame,
    ScrollEvent, Scrollable, ScrollbarVisibility,
};

use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::layout_info::LAYOUT_INFO;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::minimap::{MINIMAP, MinimapSide};
use crate::config::editor_options::padding::PADDING;
use crate::config::editor_options::registry::{SCROLL_BEYOND_LAST_COLUMN, SMOOTH_SCROLLING};
use crate::config::editor_options::scrollbar::SCROLLBAR;
use crate::config::editor_options::simple_keys::SCROLL_BEYOND_LAST_LINE;
use crate::config::editor_options::wrapping_info::WRAPPING_INFO;
use crate::config::editor_options::{ConfigurationChangedEvent, EditorOption};
use crate::cursor_common::EditorConfiguration;
use crate::editor_common::ScrollType;
use crate::line_heights::CustomLineHeightData;
use crate::lines_layout::LinesLayout;
use crate::view_model_event_dispatcher::ContentSizeChangedEvent;
use crate::view_model_types::{
    IEditorWhitespace, ILineHeightChangeAccessor, IPartialViewLinesViewportData, IViewLayout,
    IViewWhitespaceViewportData, IWhitespaceChangeAccessor, Viewport,
};

const SMOOTH_SCROLLING_TIME: f64 = 125.0;

/// `x | 0` do JavaScript sobre um `number`: `ToInt32` (trunca, dá a volta em 2^32; `NaN` e infinito viram 0).
fn to_int32(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    f64::from(value.trunc().rem_euclid(4_294_967_296.0) as u32 as i32)
}

/// `EditorScrollDimensions`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EditorScrollDimensions {
    width: f64,
    content_width: f64,
    scroll_width: f64,

    height: f64,
    content_height: f64,
    scroll_height: f64,
}

impl EditorScrollDimensions {
    fn new(width: f64, content_width: f64, height: f64, content_height: f64) -> Self {
        let width = to_int32(width).max(0.0);
        let content_width = to_int32(content_width).max(0.0);
        let height = to_int32(height).max(0.0);
        let content_height = to_int32(content_height).max(0.0);

        Self {
            width,
            content_width,
            scroll_width: width.max(content_width),
            height,
            content_height,
            scroll_height: height.max(content_height),
        }
    }
}

/// `EditorScrollable`: o [`Scrollable`] com as dimensões do editor e o evento de mudança do tamanho do conteúdo.
struct EditorScrollable {
    base: Rc<Disposable>,
    scrollable: Rc<Scrollable>,
    dimensions: Cell<EditorScrollDimensions>,
    on_did_content_size_change: Rc<Emitter<ContentSizeChangedEvent>>,
}

impl EditorScrollable {
    fn new(smooth_scroll_duration: f64, schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_content_size_change = Emitter::new();
        base.register(on_did_content_size_change.clone());
        let scrollable = Scrollable::new(IScrollableOptions {
            force_integer_values: true,
            smooth_scroll_duration,
            schedule_at_next_animation_frame,
        });
        base.register(scrollable.clone());
        Rc::new(Self {
            base,
            scrollable,
            dimensions: Cell::new(EditorScrollDimensions::new(0.0, 0.0, 0.0, 0.0)),
            on_did_content_size_change,
        })
    }

    fn get_scrollable(&self) -> &Rc<Scrollable> {
        &self.scrollable
    }

    fn get_scroll_dimensions(&self) -> EditorScrollDimensions {
        self.dimensions.get()
    }

    fn set_scroll_dimensions(&self, dimensions: EditorScrollDimensions) {
        let old_dimensions = self.dimensions.get();
        if old_dimensions == dimensions {
            return;
        }
        self.dimensions.set(dimensions);

        self.scrollable.set_scroll_dimensions(
            &INewScrollDimensions {
                width: Some(dimensions.width),
                scroll_width: Some(dimensions.scroll_width),
                height: Some(dimensions.height),
                scroll_height: Some(dimensions.scroll_height),
            },
            true,
        );

        let content_width_changed = old_dimensions.content_width != dimensions.content_width;
        let content_height_changed = old_dimensions.content_height != dimensions.content_height;
        if content_width_changed || content_height_changed {
            self.on_did_content_size_change.fire(ContentSizeChangedEvent::new(
                old_dimensions.content_width,
                old_dimensions.content_height,
                dimensions.content_width,
                dimensions.content_height,
            ));
        }
    }
}

impl IDisposable for EditorScrollable {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "EditorScrollable"
    }
}

/// O resultado de `saveState()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewLayoutState {
    pub scroll_top: f64,
    pub scroll_top_without_view_zones: f64,
    pub scroll_left: f64,
}

/// `ViewLayout`.
pub struct ViewLayout {
    base: Rc<Disposable>,

    configuration: Rc<dyn EditorConfiguration>,
    lines_layout: RefCell<LinesLayout>,
    max_line_width: Cell<f64>,
    overlay_widgets_min_width: Cell<f64>,

    scrollable: Rc<EditorScrollable>,
}

impl ViewLayout {
    pub fn new(
        configuration: Rc<dyn EditorConfiguration>,
        line_count: i32,
        custom_line_height_data: &[CustomLineHeightData],
        schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let options = configuration.options();
        let layout_info = *options.get(LAYOUT_INFO);
        let padding = *options.get(PADDING);

        let lines_layout = LinesLayout::new(
            line_count,
            *options.get(LINE_HEIGHT),
            padding.top,
            padding.bottom,
            custom_line_height_data,
        );

        let scrollable = EditorScrollable::new(0.0, schedule_at_next_animation_frame);
        base.register(scrollable.clone());

        let this = Rc::new(Self {
            base,
            configuration: configuration.clone(),
            lines_layout: RefCell::new(lines_layout),
            max_line_width: Cell::new(0.0),
            overlay_widgets_min_width: Cell::new(0.0),
            scrollable,
        });
        this.configure_smooth_scroll_duration();

        this.scrollable.set_scroll_dimensions(EditorScrollDimensions::new(
            layout_info.content_width,
            0.0,
            layout_info.height,
            0.0,
        ));

        this.update_height();
        this
    }

    /// `onDidScroll`.
    pub fn on_did_scroll(&self) -> Event<ScrollEvent> {
        self.scrollable.get_scrollable().on_scroll()
    }

    /// `onDidContentSizeChange`.
    pub fn on_did_content_size_change(&self) -> Event<ContentSizeChangedEvent> {
        self.scrollable.on_did_content_size_change.event()
    }

    pub fn on_height_maybe_changed(&self) {
        self.update_height();
    }

    /// `_configureSmoothScrollDuration`.
    fn configure_smooth_scroll_duration(&self) {
        let duration = if *self.configuration.options().get(SMOOTH_SCROLLING) { SMOOTH_SCROLLING_TIME } else { 0.0 };
        self.scrollable.get_scrollable().set_smooth_scroll_duration(duration);
    }

    // ---- begin view event handlers

    pub fn on_configuration_changed(&self, e: &ConfigurationChangedEvent) {
        let options = self.configuration.options();
        if e.has_changed(EditorOption::LineHeight) {
            self.lines_layout.borrow_mut().set_default_line_height(*options.get(LINE_HEIGHT));
        }
        if e.has_changed(EditorOption::Padding) {
            let padding = *options.get(PADDING);
            self.lines_layout.borrow_mut().set_padding(padding.top, padding.bottom);
        }
        if e.has_changed(EditorOption::LayoutInfo) {
            let layout_info = *options.get(LAYOUT_INFO);
            let width = layout_info.content_width;
            let height = layout_info.height;
            let scroll_dimensions = self.scrollable.get_scroll_dimensions();
            let content_width = scroll_dimensions.content_width;
            self.scrollable.set_scroll_dimensions(EditorScrollDimensions::new(
                width,
                scroll_dimensions.content_width,
                height,
                self.get_content_height_for(width, height, content_width),
            ));
        } else {
            self.update_height();
        }
        if e.has_changed(EditorOption::SmoothScrolling) {
            self.configure_smooth_scroll_duration();
        }
    }

    pub fn on_flushed(&self, line_count: i32, custom_line_height_data: &[CustomLineHeightData]) {
        self.lines_layout.borrow_mut().on_flushed(line_count, custom_line_height_data);
    }

    pub fn on_lines_deleted(&self, from_line_number: i32, to_line_number: i32) {
        self.lines_layout.borrow_mut().on_lines_deleted(from_line_number, to_line_number);
    }

    pub fn on_lines_inserted(&self, from_line_number: i32, to_line_number: i32) {
        self.lines_layout.borrow_mut().on_lines_inserted(from_line_number, to_line_number);
    }

    // ---- end view event handlers

    /// `_getHorizontalScrollbarHeight`.
    fn get_horizontal_scrollbar_height(&self, width: f64, scroll_width: f64) -> f64 {
        let scrollbar = *self.configuration.options().get(SCROLLBAR);
        if scrollbar.horizontal == ScrollbarVisibility::Hidden {
            // horizontal scrollbar not visible
            return 0.0;
        }
        if width >= scroll_width {
            // horizontal scrollbar not visible
            return 0.0;
        }
        scrollbar.horizontal_scrollbar_size
    }

    /// `_getContentHeight(width, height, contentWidth)`. O `getContentHeight()` público, sem argumentos, é
    /// [`ViewLayout::get_content_height`].
    fn get_content_height_for(&self, width: f64, height: f64, content_width: f64) -> f64 {
        let options = self.configuration.options();

        let mut result = self.lines_layout.borrow_mut().get_lines_total_height();
        if *options.get(SCROLL_BEYOND_LAST_LINE) {
            result += (height - *options.get(LINE_HEIGHT) - options.get(PADDING).bottom).max(0.0);
        } else if !options.get(SCROLLBAR).ignore_horizontal_scrollbar_in_content_height {
            result += self.get_horizontal_scrollbar_height(width, content_width);
        }

        result
    }

    /// `_updateHeight`.
    fn update_height(&self) {
        let scroll_dimensions = self.scrollable.get_scroll_dimensions();
        let width = scroll_dimensions.width;
        let height = scroll_dimensions.height;
        let content_width = scroll_dimensions.content_width;
        self.scrollable.set_scroll_dimensions(EditorScrollDimensions::new(
            width,
            scroll_dimensions.content_width,
            height,
            self.get_content_height_for(width, height, content_width),
        ));
    }

    // ---- Layouting logic

    /// `_computeContentWidth`.
    fn compute_content_width(&self) -> f64 {
        let options = self.configuration.options();
        let max_line_width = self.max_line_width.get();
        let wrapping_info = options.get(WRAPPING_INFO);
        let font_info = options.get(FONT_INFO);
        let layout_info = options.get(LAYOUT_INFO);
        if wrapping_info.is_viewport_wrapping {
            let minimap = options.get(MINIMAP);
            if max_line_width > layout_info.content_width + font_info.typical_halfwidth_character_width
                && minimap.enabled
                && minimap.side == MinimapSide::Right
            {
                // Viewport wrapping está ligado, mas a linha passa do viewport: acomoda a largura da scrollbar.
                return max_line_width + layout_info.vertical_scrollbar_width;
            }
            max_line_width
        } else {
            let extra_horizontal_space = *options.get(SCROLL_BEYOND_LAST_COLUMN) * font_info.typical_halfwidth_character_width;
            let whitespace_min_width = f64::from(self.lines_layout.borrow_mut().get_whitespace_min_width());
            (max_line_width + extra_horizontal_space + layout_info.vertical_scrollbar_width)
                .max(whitespace_min_width)
                .max(self.overlay_widgets_min_width.get())
        }
    }

    /// `_updateContentWidth`.
    fn update_content_width(&self) {
        let scroll_dimensions = self.scrollable.get_scroll_dimensions();
        self.scrollable.set_scroll_dimensions(EditorScrollDimensions::new(
            scroll_dimensions.width,
            self.compute_content_width(),
            scroll_dimensions.height,
            scroll_dimensions.content_height,
        ));

        // The height might depend on the fact that there is a horizontal scrollbar or not
        self.update_height();
    }

    // ---- view state

    pub fn save_state(&self) -> ViewLayoutState {
        let current_scroll_position = self.scrollable.get_scrollable().get_future_scroll_position();
        let scroll_top = current_scroll_position.scroll_top;
        let mut lines_layout = self.lines_layout.borrow_mut();
        let first_line_number_in_viewport = lines_layout.get_line_number_at_or_after_vertical_offset(scroll_top);
        let whitespace_above_first_line =
            lines_layout.get_whitespace_accumulated_height_before_line_number(first_line_number_in_viewport);
        ViewLayoutState {
            scroll_top,
            scroll_top_without_view_zones: scroll_top - whitespace_above_first_line,
            scroll_left: current_scroll_position.scroll_left,
        }
    }

    // ----

    /// `changeWhitespace`: aplica as mudanças e, se houve alguma, reavalia a altura.
    pub fn change_whitespace(&self, callback: impl FnOnce(&mut dyn IWhitespaceChangeAccessor)) -> bool {
        let had_a_change = LinesLayout::change_whitespace_of(&self.lines_layout, callback);
        if had_a_change {
            self.on_height_maybe_changed();
        }
        had_a_change
    }

    /// `changeSpecialLineHeights`.
    pub fn change_special_line_heights(&self, callback: impl FnOnce(&mut dyn ILineHeightChangeAccessor)) -> bool {
        let had_a_change = self.lines_layout.borrow_mut().change_line_heights(callback);
        if had_a_change {
            self.on_height_maybe_changed();
        }
        had_a_change
    }

    // ----

    pub fn get_content_width(&self) -> f64 {
        self.scrollable.get_scroll_dimensions().content_width
    }

    pub fn get_content_height(&self) -> f64 {
        self.scrollable.get_scroll_dimensions().content_height
    }
}

impl IViewLayout for ViewLayout {
    fn get_scrollable(&self) -> Rc<Scrollable> {
        self.scrollable.get_scrollable().clone()
    }

    fn get_scroll_width(&self) -> f64 {
        self.scrollable.get_scroll_dimensions().scroll_width
    }

    fn get_scroll_height(&self) -> f64 {
        self.scrollable.get_scroll_dimensions().scroll_height
    }

    fn get_current_scroll_left(&self) -> f64 {
        self.scrollable.get_scrollable().get_current_scroll_position().scroll_left
    }

    fn get_current_scroll_top(&self) -> f64 {
        self.scrollable.get_scrollable().get_current_scroll_position().scroll_top
    }

    fn get_current_viewport(&self) -> Viewport {
        let scroll_dimensions = self.scrollable.get_scroll_dimensions();
        let current_scroll_position = self.scrollable.get_scrollable().get_current_scroll_position();
        Viewport::new(
            current_scroll_position.scroll_top,
            current_scroll_position.scroll_left,
            scroll_dimensions.width,
            scroll_dimensions.height,
        )
    }

    fn get_future_viewport(&self) -> Viewport {
        let scroll_dimensions = self.scrollable.get_scroll_dimensions();
        let current_scroll_position = self.scrollable.get_scrollable().get_future_scroll_position();
        Viewport::new(
            current_scroll_position.scroll_top,
            current_scroll_position.scroll_left,
            scroll_dimensions.width,
            scroll_dimensions.height,
        )
    }

    fn set_scroll_position(&self, position: INewScrollPosition, r#type: ScrollType) {
        let scrollable = self.scrollable.get_scrollable();
        if r#type == ScrollType::Immediate {
            scrollable.set_scroll_position_now(&position);
        } else {
            scrollable.set_scroll_position_smooth(&position, false);
        }
    }

    fn delta_scroll_now(&self, delta_scroll_left: f64, delta_scroll_top: f64) {
        let scrollable = self.scrollable.get_scrollable();
        let current_scroll_position = scrollable.get_current_scroll_position();
        scrollable.set_scroll_position_now(&INewScrollPosition {
            scroll_left: Some(current_scroll_position.scroll_left + delta_scroll_left),
            scroll_top: Some(current_scroll_position.scroll_top + delta_scroll_top),
        });
    }

    fn validate_scroll_position(&self, scroll_position: INewScrollPosition) -> IScrollPosition {
        self.scrollable.get_scrollable().validate_scroll_position(&scroll_position)
    }

    fn set_max_line_width(&self, max_line_width: f64) {
        self.max_line_width.set(max_line_width);
        self.update_content_width();
    }

    fn set_overlay_widgets_min_width(&self, overlay_widgets_min_width: f64) {
        self.overlay_widgets_min_width.set(overlay_widgets_min_width);
        self.update_content_width();
    }

    fn get_lines_viewport_data(&self) -> IPartialViewLinesViewportData {
        let visible_box = self.get_current_viewport();
        self.lines_layout.borrow_mut().get_lines_viewport_data(visible_box.top, visible_box.top + visible_box.height)
    }

    fn get_lines_viewport_data_at_scroll_top(&self, scroll_top: f64) -> IPartialViewLinesViewportData {
        // do some minimal validations on scrollTop
        let mut scroll_top = scroll_top;
        let scroll_dimensions = self.scrollable.get_scroll_dimensions();
        if scroll_top + scroll_dimensions.height > scroll_dimensions.scroll_height {
            scroll_top = scroll_dimensions.scroll_height - scroll_dimensions.height;
        }
        if scroll_top < 0.0 {
            scroll_top = 0.0;
        }
        self.lines_layout.borrow_mut().get_lines_viewport_data(scroll_top, scroll_top + scroll_dimensions.height)
    }

    fn get_whitespaces(&self) -> Vec<IEditorWhitespace> {
        self.lines_layout.borrow().get_whitespaces()
    }

    fn is_after_lines(&self, vertical_offset: f64) -> bool {
        self.lines_layout.borrow_mut().is_after_lines(vertical_offset)
    }

    fn is_in_top_padding(&self, vertical_offset: f64) -> bool {
        self.lines_layout.borrow().is_in_top_padding(vertical_offset)
    }

    fn is_in_bottom_padding(&self, vertical_offset: f64) -> bool {
        self.lines_layout.borrow_mut().is_in_bottom_padding(vertical_offset)
    }

    fn get_line_number_at_vertical_offset(&self, vertical_offset: f64) -> i32 {
        self.lines_layout.borrow_mut().get_line_number_at_or_after_vertical_offset(vertical_offset)
    }

    fn get_vertical_offset_for_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64 {
        self.lines_layout
            .borrow_mut()
            .get_vertical_offset_for_line_number(line_number, include_view_zones.unwrap_or(false))
    }

    fn get_vertical_offset_after_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64 {
        self.lines_layout
            .borrow_mut()
            .get_vertical_offset_after_line_number(line_number, include_view_zones.unwrap_or(false))
    }

    fn get_line_height_for_line_number(&self, line_number: i32) -> f64 {
        self.lines_layout.borrow_mut().get_line_height_for_line_number(line_number)
    }

    fn get_whitespace_at_vertical_offset(&self, vertical_offset: f64) -> Option<IViewWhitespaceViewportData> {
        self.lines_layout.borrow_mut().get_whitespace_at_vertical_offset(vertical_offset)
    }

    fn get_whitespace_viewport_data(&self) -> Vec<IViewWhitespaceViewportData> {
        let visible_box = self.get_current_viewport();
        self.lines_layout
            .borrow_mut()
            .get_whitespace_viewport_data(visible_box.top, visible_box.top + visible_box.height)
    }
}

impl IDisposable for ViewLayout {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ViewLayout"
    }
}
