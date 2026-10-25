//! Porte de `src/vs/editor/browser/viewParts/viewZones/viewZones.ts`: a parte `ViewZones` da visão, que traduz cada
//! [`IViewZone`] numa whitespace do layout do view model (`changeWhitespace`), calcula a altura e a posição da zona e
//! avisa o dono dela (`onComputedHeight`, `onDomNodeTop`).
//!
//! Representação:
//! - `ViewZones` é compartilhada (`Rc`) e muda de estado por trás de `&self` (`Cell`/`RefCell`), como o resto do
//!   crate. É o `ViewEventHandler` do original (`ViewPart`): o dono a registra no view model com
//!   `add_view_event_handler`, e ela só reage a `onConfigurationChanged` e `onLineMappingChanged` (recalcula as
//!   whitespaces); os demais handlers devolvem o que o original devolve.
//! - O `ViewContext` do original traz configuração, view model e layout. Aqui `ViewZones` recebe a configuração e o
//!   view model e pede o layout ao view model (`view_layout()`), que é o que o `ViewContext` faz no construtor.
//! - `ViewZones` é genérica no view model (`IViewModel` tem tipos associados, ver `view_model_types.rs`).
//! - Sem DOM, o `domNode` e o `marginDomNode` de cada zona são o dado que o pintor nativo lê
//!   ([`IViewZone::dom_node`]). O que o `render` do original escreve nos nós (`top`, `height`, `display`, o atributo
//!   `monaco-visible-view-zone` e a largura dos contêineres) é o [`ViewZonesRender`] que [`ViewZones::render`] devolve,
//!   com o `ctx` do original reduzido ao que ele lê ([`ViewZonesRenderInput`]). `setPosition('absolute')`,
//!   `width: 100%`, os atributos `monaco-view-zone`, `role` e `aria-hidden` e `appendChild`/`remove` não têm análogo.
//! - Os ids de zona são os ids de whitespace do layout (`String`), e `_zones` guarda as zonas na ordem de inserção
//!   (os ids crescem, então é a ordem de `Object.keys`).
//! - A altura em pixels é `f64` (a altura de linha pode ser fracionária); o layout a recebe reduzida por `| 0`, e o
//!   `onComputedHeight` recebe o valor sem reduzir, como no original.
//! - O acessor do `changeViewZones` é emprestado só durante o callback (o original o invalida depois com `invalidFunc`).
//! - `try { ... } catch (e) { onUnexpectedError(e); }` (o `safeInvoke1Arg` e os `_safeCall...`) é `errors::catch` seguido
//!   de `errors::on_unexpected_error`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;

use crate::config::editor_options::EditorOption;
use crate::config::editor_options::layout_info::LAYOUT_INFO;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::cursor_common::{CursorSimpleModel, EditorConfiguration};
use crate::editor_browser::{IViewZone, IViewZoneChangeAccessor};
use crate::position::Position;
use crate::view_events::{
    ShouldRenderState, ViewConfigurationChangedEvent, ViewEventHandler, ViewLineMappingChangedEvent, ViewLinesDeletedEvent,
    ViewLinesInsertedEvent, ViewScrollChangedEvent, ViewZonesChangedEvent,
};
use crate::view_model_types::{IEditorWhitespace, IViewModel, IViewWhitespaceViewportData, IWhitespaceChangeAccessor};

/// O `ordinal` padrão de uma zona sem `ordinal` nem `afterColumn`.
const DEFAULT_ZONE_ORDINAL: i32 = 10000;

/// `IMyViewZone`. O `top`, o `height` e o `is_displayed` são o estado que o `render` escreve no `domNode`.
struct MyViewZone {
    whitespace_id: String,
    delegate: Rc<dyn IViewZone>,
    is_in_hidden_area: Cell<bool>,
    is_visible: Cell<bool>,
    top: Cell<f64>,
    height: Cell<f64>,
    is_displayed: Cell<bool>,
}

/// `IComputedViewZoneProps`.
struct ComputedViewZoneProps {
    is_in_hidden_area: bool,
    after_view_line_number: i32,
    height_in_px: f64,
    min_width_in_px: f64,
}

/// O que o `render` lê do `RestrictedRenderingContext`.
pub struct ViewZonesRenderInput<'a> {
    /// `ctx.viewportData.whitespaceViewportData`.
    pub whitespace_viewport_data: &'a [IViewWhitespaceViewportData],
    /// `ctx.bigNumbersDelta`.
    pub big_numbers_delta: f64,
    /// `ctx.scrollWidth`.
    pub scroll_width: f64,
    /// `ctx.getScrolledTopFromAbsoluteTop(absoluteTop)`.
    pub scrolled_top_from_absolute_top: &'a dyn Fn(f64) -> f64,
}

/// O estado de uma zona depois do `render`: o que o original escreve no `domNode` e no `marginDomNode` dela.
pub struct RenderedViewZone {
    /// O id da zona.
    pub id: String,
    /// A zona (o pintor lê o `domNode` e o `marginDomNode` dela).
    pub delegate: Rc<dyn IViewZone>,
    /// `domNode.setTop`.
    pub top: f64,
    /// `domNode.setHeight`.
    pub height: f64,
    /// `domNode.setDisplay`: `block` (verdadeiro) ou `none`.
    pub is_displayed: bool,
    /// O atributo `monaco-visible-view-zone`.
    pub is_visible: bool,
}

/// O resultado de [`ViewZones::render`].
pub struct ViewZonesRender {
    /// As zonas, na ordem de inserção.
    pub zones: Vec<RenderedViewZone>,
    /// `domNode.setWidth` (só muda quando há uma zona visível; senão fica a última largura).
    pub width: f64,
    /// `marginDomNode.setWidth` (idem).
    pub margin_width: f64,
}

/// `ViewZones`.
pub struct ViewZones<VM: IViewModel + 'static> {
    should_render: ShouldRenderState,
    configuration: Rc<dyn EditorConfiguration>,
    view_model: Rc<VM>,
    zones: RefCell<Vec<Rc<MyViewZone>>>,
    line_height: Cell<f64>,
    content_width: Cell<f64>,
    content_left: Cell<f64>,
    width: Cell<f64>,
    margin_width: Cell<f64>,
}

/// O `IViewZoneChangeAccessor` do `changeViewZones`: cada chamada vai para a whitespace do layout.
struct ZonesChangeAccessor<'a, VM: IViewModel + 'static> {
    zones: &'a ViewZones<VM>,
    whitespace: &'a mut dyn IWhitespaceChangeAccessor,
    zones_have_changed: &'a mut bool,
}

impl<VM: IViewModel + 'static> IViewZoneChangeAccessor for ZonesChangeAccessor<'_, VM> {
    fn add_zone(&mut self, zone: Rc<dyn IViewZone>) -> String {
        *self.zones_have_changed = true;
        self.zones.add_zone(&mut *self.whitespace, zone)
    }

    fn remove_zone(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        *self.zones_have_changed = self.zones.remove_zone(&mut *self.whitespace, id) || *self.zones_have_changed;
    }

    fn layout_zone(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        *self.zones_have_changed = self.zones.layout_zone(&mut *self.whitespace, id) || *self.zones_have_changed;
    }
}

impl<VM: IViewModel + 'static> ViewZones<VM> {
    pub fn new(configuration: Rc<dyn EditorConfiguration>, view_model: Rc<VM>) -> Rc<Self> {
        let (line_height, content_width, content_left) = Self::read_configuration(&*configuration);
        Rc::new(Self {
            should_render: ShouldRenderState::default(),
            configuration,
            view_model,
            zones: RefCell::new(Vec::new()),
            line_height: Cell::new(line_height),
            content_width: Cell::new(content_width),
            content_left: Cell::new(content_left),
            width: Cell::new(0.0),
            margin_width: Cell::new(0.0),
        })
    }

    /// `lineHeight`, `layoutInfo.contentWidth` e `layoutInfo.contentLeft`.
    fn read_configuration(configuration: &dyn EditorConfiguration) -> (f64, f64, f64) {
        let options = configuration.options();
        let layout_info = options.get(LAYOUT_INFO);
        (*options.get(LINE_HEIGHT), layout_info.content_width, layout_info.content_left)
    }

    /// `_recomputeWhitespacesProps()`: devolve se alguma zona mudou de linha ou de altura.
    fn recompute_whitespaces_props(&self) -> bool {
        let old_whitespaces: HashMap<String, IEditorWhitespace> =
            self.view_model.view_layout().get_whitespaces().into_iter().map(|whitespace| (whitespace.id.clone(), whitespace)).collect();
        let mut had_a_change = false;
        self.view_model.change_whitespace(&mut |whitespace_accessor| {
            let zones: Vec<Rc<MyViewZone>> = self.zones.borrow().clone();
            for zone in zones {
                let props = self.compute_whitespace_props(&*zone.delegate);
                zone.is_in_hidden_area.set(props.is_in_hidden_area);
                if let Some(old_whitespace) = old_whitespaces.get(&zone.whitespace_id)
                    && (old_whitespace.after_line_number != props.after_view_line_number
                        || f64::from(old_whitespace.height) != props.height_in_px)
                {
                    whitespace_accessor.change_one_whitespace(
                        &zone.whitespace_id,
                        props.after_view_line_number,
                        props.height_in_px as i32,
                    );
                    Self::safe_call_on_computed_height(&*zone.delegate, props.height_in_px);
                    had_a_change = true;
                }
            }
        });
        had_a_change
    }

    /// `_getZoneOrdinal(zone)`: `zone.ordinal ?? zone.afterColumn ?? 10000`.
    fn zone_ordinal(zone: &dyn IViewZone) -> i32 {
        zone.ordinal().or_else(|| zone.after_column()).unwrap_or(DEFAULT_ZONE_ORDINAL)
    }

    /// `_computeWhitespaceProps(zone)`.
    fn compute_whitespace_props(&self, zone: &dyn IViewZone) -> ComputedViewZoneProps {
        if zone.after_line_number() == 0 {
            return ComputedViewZoneProps {
                is_in_hidden_area: false,
                after_view_line_number: 0,
                height_in_px: self.height_in_pixels(zone),
                min_width_in_px: Self::min_width_in_pixels(zone),
            };
        }

        let model = self.view_model.model();
        let zone_after_model_position = match zone.after_column() {
            Some(after_column) => model.validate_position(Position::new(zone.after_line_number(), after_column)),
            None => {
                let valid_after_line_number = model.validate_position(Position::new(zone.after_line_number(), 1)).line_number;
                Position::new(valid_after_line_number, model.get_line_max_column(valid_after_line_number))
            }
        };

        let zone_before_model_position =
            if zone_after_model_position.column == model.get_line_max_column(zone_after_model_position.line_number) {
                model.validate_position(Position::new(zone_after_model_position.line_number + 1, 1))
            } else {
                model.validate_position(Position::new(zone_after_model_position.line_number, zone_after_model_position.column + 1))
            };

        let coordinates_converter = self.view_model.coordinates_converter();
        let view_position = coordinates_converter.convert_model_position_to_view_position(
            zone_after_model_position,
            zone.after_column_affinity(),
            Some(true),
            None,
        );
        let is_visible =
            zone.show_in_hidden_areas() || coordinates_converter.model_position_is_visible(zone_before_model_position);
        ComputedViewZoneProps {
            is_in_hidden_area: !is_visible,
            after_view_line_number: view_position.line_number,
            height_in_px: if is_visible { self.height_in_pixels(zone) } else { 0.0 },
            min_width_in_px: Self::min_width_in_pixels(zone),
        }
    }

    /// `changeViewZones(callback)`: devolve se alguma zona foi adicionada, removida ou reposicionada.
    pub fn change_view_zones(&self, callback: &mut dyn FnMut(&mut dyn IViewZoneChangeAccessor)) -> bool {
        let mut zones_have_changed = false;

        self.view_model.change_whitespace(&mut |whitespace_accessor| {
            let mut change_accessor =
                ZonesChangeAccessor { zones: self, whitespace: whitespace_accessor, zones_have_changed: &mut zones_have_changed };

            if let Err(error) = errors::catch(|| callback(&mut change_accessor)) {
                errors::on_unexpected_error(&error);
            }
        });

        zones_have_changed
    }

    /// `_addZone(whitespaceAccessor, zone)`.
    fn add_zone(&self, whitespace_accessor: &mut dyn IWhitespaceChangeAccessor, zone: Rc<dyn IViewZone>) -> String {
        let props = self.compute_whitespace_props(&*zone);
        let whitespace_id = whitespace_accessor.insert_whitespace(
            props.after_view_line_number,
            Self::zone_ordinal(&*zone),
            props.height_in_px as i32,
            props.min_width_in_px as i32,
        );

        let my_zone = Rc::new(MyViewZone {
            whitespace_id: whitespace_id.clone(),
            delegate: zone,
            is_in_hidden_area: Cell::new(props.is_in_hidden_area),
            is_visible: Cell::new(false),
            top: Cell::new(0.0),
            height: Cell::new(0.0),
            is_displayed: Cell::new(false),
        });

        Self::safe_call_on_computed_height(&*my_zone.delegate, props.height_in_px);

        self.zones.borrow_mut().push(my_zone);

        self.set_should_render();

        whitespace_id
    }

    /// `_removeZone(whitespaceAccessor, id)`.
    fn remove_zone(&self, whitespace_accessor: &mut dyn IWhitespaceChangeAccessor, id: &str) -> bool {
        let removed = {
            let mut zones = self.zones.borrow_mut();
            zones.iter().position(|zone| zone.whitespace_id == id).map(|index| zones.remove(index))
        };
        let Some(zone) = removed else {
            return false;
        };
        whitespace_accessor.remove_whitespace(&zone.whitespace_id);

        self.set_should_render();

        true
    }

    /// `_layoutZone(whitespaceAccessor, id)`.
    fn layout_zone(&self, whitespace_accessor: &mut dyn IWhitespaceChangeAccessor, id: &str) -> bool {
        let Some(zone) = self.find_zone(id) else {
            return false;
        };
        let props = self.compute_whitespace_props(&*zone.delegate);
        zone.is_in_hidden_area.set(props.is_in_hidden_area);
        // const newOrdinal = this._getZoneOrdinal(zone.delegate);
        // O original também não troca o ordinal da zona aqui (só a linha e a altura).
        whitespace_accessor.change_one_whitespace(&zone.whitespace_id, props.after_view_line_number, props.height_in_px as i32);

        Self::safe_call_on_computed_height(&*zone.delegate, props.height_in_px);
        self.set_should_render();

        true
    }

    /// `this._zones[id]`.
    fn find_zone(&self, id: &str) -> Option<Rc<MyViewZone>> {
        self.zones.borrow().iter().find(|zone| zone.whitespace_id == id).cloned()
    }

    /// `shouldSuppressMouseDownOnViewZone(id)`.
    pub fn should_suppress_mouse_down_on_view_zone(&self, id: &str) -> bool {
        self.find_zone(id).is_some_and(|zone| zone.delegate.suppress_mouse_down())
    }

    /// `_heightInPixels(zone)`.
    fn height_in_pixels(&self, zone: &dyn IViewZone) -> f64 {
        if let Some(height_in_px) = zone.height_in_px() {
            return height_in_px;
        }
        if let Some(height_in_lines) = zone.height_in_lines() {
            return self.line_height.get() * height_in_lines;
        }
        self.line_height.get()
    }

    /// `_minWidthInPixels(zone)`.
    fn min_width_in_pixels(zone: &dyn IViewZone) -> f64 {
        zone.min_width_in_px().unwrap_or(0.0)
    }

    /// `_safeCallOnComputedHeight(zone, height)`.
    fn safe_call_on_computed_height(zone: &dyn IViewZone, height: f64) {
        Self::safe_invoke(|| zone.on_computed_height(height));
    }

    /// `_safeCallOnDomNodeTop(zone, top)`.
    fn safe_call_on_dom_node_top(zone: &dyn IViewZone, top: f64) {
        Self::safe_invoke(|| zone.on_dom_node_top(top));
    }

    /// O `try { ... } catch (e) { onUnexpectedError(e); }` dos `_safeCall...`.
    fn safe_invoke(callback: impl FnOnce()) {
        if let Err(error) = errors::catch(callback) {
            errors::on_unexpected_error(&error);
        }
    }

    /// `prepareRender(ctx)`: não há nada para ler, então não existe. `render(ctx)`: posiciona cada zona a partir das
    /// whitespaces visíveis do viewport, avisa o dono (`onDomNodeTop`) e devolve o estado que o pintor lê.
    pub fn render(&self, ctx: &ViewZonesRenderInput<'_>) -> ViewZonesRender {
        let mut visible_zones: HashMap<&str, &IViewWhitespaceViewportData> = HashMap::new();

        let mut has_visible_zone = false;
        for visible_whitespace in ctx.whitespace_viewport_data {
            let Some(zone) = self.find_zone(&visible_whitespace.id) else {
                errors::throw(Error::new("Cannot read properties of undefined (reading 'isInHiddenArea')"));
            };
            if zone.is_in_hidden_area.get() {
                continue;
            }
            visible_zones.insert(visible_whitespace.id.as_str(), visible_whitespace);
            has_visible_zone = true;
        }

        let zones: Vec<Rc<MyViewZone>> = self.zones.borrow().clone();
        let mut rendered = Vec::with_capacity(zones.len());
        for zone in zones {
            let mut new_top = 0.0;
            let mut new_height = 0.0;
            let mut new_display = false;
            if let Some(visible_zone) = visible_zones.get(zone.whitespace_id.as_str()) {
                new_top = visible_zone.vertical_offset - ctx.big_numbers_delta;
                new_height = f64::from(visible_zone.height);
                new_display = true;
                // zone is visible
                zone.is_visible.set(true);
                Self::safe_call_on_dom_node_top(
                    &*zone.delegate,
                    (ctx.scrolled_top_from_absolute_top)(visible_zone.vertical_offset),
                );
            } else {
                zone.is_visible.set(false);
                Self::safe_call_on_dom_node_top(&*zone.delegate, (ctx.scrolled_top_from_absolute_top)(-1_000_000.0));
            }
            zone.top.set(new_top);
            zone.height.set(new_height);
            zone.is_displayed.set(new_display);

            rendered.push(RenderedViewZone {
                id: zone.whitespace_id.clone(),
                delegate: Rc::clone(&zone.delegate),
                top: zone.top.get(),
                height: zone.height.get(),
                is_displayed: zone.is_displayed.get(),
                is_visible: zone.is_visible.get(),
            });
        }

        if has_visible_zone {
            self.width.set(ctx.scroll_width.max(self.content_width.get()));
            self.margin_width.set(self.content_left.get());
        }

        ViewZonesRender { zones: rendered, width: self.width.get(), margin_width: self.margin_width.get() }
    }
}

impl<VM: IViewModel + 'static> ViewEventHandler for ViewZones<VM> {
    fn should_render_state(&self) -> &ShouldRenderState {
        &self.should_render
    }

    fn on_configuration_changed(&self, e: &ViewConfigurationChangedEvent) -> bool {
        let (line_height, content_width, content_left) = Self::read_configuration(&*self.configuration);
        self.line_height.set(line_height);
        self.content_width.set(content_width);
        self.content_left.set(content_left);

        if e.has_changed(EditorOption::LineHeight) {
            self.recompute_whitespaces_props();
        }

        true
    }

    fn on_line_mapping_changed(&self, _e: &ViewLineMappingChangedEvent) -> bool {
        self.recompute_whitespaces_props()
    }

    fn on_lines_deleted(&self, _e: &ViewLinesDeletedEvent) -> bool {
        true
    }

    fn on_scroll_changed(&self, e: &ViewScrollChangedEvent) -> bool {
        e.scroll_top_changed || e.scroll_width_changed
    }

    fn on_zones_changed(&self, _e: &ViewZonesChangedEvent) -> bool {
        true
    }

    fn on_lines_inserted(&self, _e: &ViewLinesInsertedEvent) -> bool {
        true
    }
}

impl<VM: IViewModel + 'static> IDisposable for ViewZones<VM> {
    fn dispose(&self) {
        self.zones.borrow_mut().clear();
    }
}
