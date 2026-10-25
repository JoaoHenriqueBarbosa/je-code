//! Porte de `src/vs/editor/contrib/zoneWidget/browser/zoneWidget.ts`: o `ZoneWidget`, um widget que ocupa uma faixa de
//! linhas do editor (uma view zone) e se desenha por cima dela, com a seta que aponta para a linha, a moldura e a alça
//! de redimensionamento. É a base do peek (`PeekViewWidget`) e dos widgets de erro, de comentários e de exceção.
//!
//! Representação:
//! - `abstract class ZoneWidget` é a struct [`ZoneWidget`] mais o trait [`ZoneWidgetHooks`], que reúne os membros que as
//!   subclasses sobrescrevem (`_fillContainer` abstrato, `_onWidth`, `_doLayout`, `_applyStyles`, `_getWidth`,
//!   `_getMaximumHeightInLines`, `revealRange` e `_getResizeBounds`). A subclasse é dona do `Rc<ZoneWidget>` e se registra
//!   como os ganchos com [`ZoneWidget::set_hooks`] (um `Weak`, para não formar ciclo). Cada gancho recebe o `&ZoneWidget`
//!   e o corpo padrão chama a versão base (`apply_styles_base`, `get_width_base`...), que é o `super._applyStyles()` do
//!   original. Os métodos públicos que a subclasse sobrescreve (`show`, `hide`, `dispose`...) ficam na subclasse, que chama
//!   o da base.
//! - **DOM vira a árvore de `je-ui`.** `domNode` e `container` são nós da árvore de widgets ([`ZoneWidget::dom_node`] e
//!   [`ZoneWidget::container`]); as classes `zone-widget` e `zone-widget-container` são as classes dos nós; o `top`, o
//!   `left`, o `width` e o `height` do `style` são os limites do nó ([`ZoneDomBox`]), reescritos a cada mudança; a moldura
//!   (`border-top` e `border-bottom` do contêiner) é a borda do nó; `overflow: hidden` é o `clip`. O nó nasce solto e entra
//!   no contêiner dos widgets de overlay do editor ([`ZoneWidgetHost::overlay_container`]) quando o overlay é adicionado,
//!   e sai quando ele é removido. `aria-hidden` e `role=presentation` são o atributo e o papel do nó.
//! - **O overlay e a zona.** O `OverlayWidgetDelegate` do original é [`OverlayWidgetDelegate`] (sem `getDomNode`, que não
//!   existe: o nó é [`OverlayWidgetDelegate::dom_node`]). O `domNode` da view zone (a `div` com `overflow: hidden` que só
//!   reserva o espaço) é um nó de [`ZoneViewZoneNode`], que o pintor lê por `downcast` do [`IViewZone::dom_node`].
//! - **A seta.** `Arrow` é uma decoração de classe `.arrow-decoration-N` mais uma regra de CSS gerada na hora
//!   (`createCSSRule`). A regra é o dado [`ZoneArrowRule`] (cor da ponta e altura, de onde saem `border-width`, `bottom` e
//!   `margin-left`), registrado por thread com o nome da classe e lido pelo pintor com [`zone_arrow_rule`]; descartar a seta
//!   remove a regra (`removeCSSRulesContainingSelector`).
//! - O `Sash` é o de `je-widgets`, e o `IHorizontalSashLayoutProvider` é um provedor privado que lê os limites do nó.
//! - `show(rangeOrPos, heightInLines)` e `updatePositionAndHeight(rangeOrPos, heightInLines?)` recebem sempre um [`Range`]
//!   (uma posição é o intervalo vazio nela, `Range.fromPositions(pos)`), que é o que o original faz com elas.
//! - `IOptions.frameColor` é `Color | string`: todo produtor passa um `Color`, então o campo é `Color`.
//! - `getHorizontalSashTop` lê `parseInt(domNode.style.height)`; sem altura ainda, o nó não tem altura e o valor é zero.
//! - A view zone só existe se o editor tem modelo (`changeViewZones` não roda o callback sem ele), como no original.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::id_generator::IdGenerator;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_render::kurbo::Rect;
use je_theme::color::{Color, Srgba};
use je_theme::to_rgba8;
use je_ui::{Border, NodeId, Role, Ui};
use je_widgets::dom::create_el;
use je_widgets::sash::{HorizontalSashLayoutProvider, Sash, SashEvent, SashOptions, SashState};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor, SelectionOrRange};
use crate::config::editor_options::layout_info::EditorLayoutInfo;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_browser::{IOverlayWidget, IViewZone, IViewZoneChangeAccessor, OverlayWidgetPosition};
use crate::editor_common::ScrollType;
use crate::editor_decorations_collection::EditorDecorationsCollection;
use crate::model::{ModelDecorationOptionsInit, ModelDeltaDecoration, TrackedRangeStickiness};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::ModelDecorationOptions;

const WIDGET_ID: &str = "vs.editor.contrib.zoneWidget";

/// `defaultColor`: `new Color(new RGBA(0, 122, 204))`.
fn default_color() -> Color {
    Color::from(Srgba::opaque(0, 122, 204))
}

/// `IOptions`. Todo campo é opcional, e [`ZoneWidgetOptions::mixin_defaults`] preenche os que faltam com os padrões do
/// original.
#[derive(Clone, Default)]
pub struct ZoneWidgetOptions {
    pub show_frame: Option<bool>,
    pub show_arrow: Option<bool>,
    pub frame_width: Option<f64>,
    pub class_name: Option<String>,
    pub is_accessible: Option<bool>,
    pub is_resizeable: Option<bool>,
    pub frame_color: Option<Color>,
    pub arrow_color: Option<Color>,
    pub keep_editor_selection: Option<bool>,
    pub ordinal: Option<i32>,
    pub show_in_hidden_areas: Option<bool>,
}

impl ZoneWidgetOptions {
    /// `objects.mixin(this.options, defaultOptions, false)`: só os campos ausentes recebem o padrão.
    pub fn mixin_defaults(&mut self) {
        self.show_arrow.get_or_insert(true);
        self.show_frame.get_or_insert(true);
        self.class_name.get_or_insert_with(String::new);
        self.frame_color.get_or_insert_with(default_color);
        self.arrow_color.get_or_insert_with(default_color);
        self.keep_editor_selection.get_or_insert(false);
    }

    /// `options.showArrow`.
    pub fn show_arrow(&self) -> bool {
        self.show_arrow.unwrap_or(false)
    }

    /// `options.showFrame`.
    pub fn show_frame(&self) -> bool {
        self.show_frame.unwrap_or(false)
    }
}

/// `IStyles`: `null` e `undefined` são `None`.
#[derive(Clone, Default)]
pub struct ZoneWidgetStyles {
    pub frame_color: Option<Color>,
    pub arrow_color: Option<Color>,
}

/// O resultado de `_getResizeBounds`: `{ minLines, maxLines }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneResizeBounds {
    pub min_lines: f64,
    pub max_lines: f64,
}

/// Onde o `ZoneWidget` vive na árvore de widgets: o [`Ui`] dono dela e o nó que contém os widgets de overlay do editor (o
/// que o `addOverlayWidget` do original usa para pendurar o `domNode`).
#[derive(Clone)]
pub struct ZoneWidgetHost {
    pub ui: Ui,
    pub overlay_container: NodeId,
}

/// Os limites do `domNode` (`style.top`, `left`, `width` e `height`). `height` é `None` até a primeira altura.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ZoneDomBox {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: Option<f64>,
}

/// O `top` e o `height` do `container` (`style.top` e `style.height`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ZoneContainerBox {
    top: f64,
    height: Option<f64>,
}

// #region Seta

thread_local! {
    /// `Arrow._IdGenerator`.
    static ARROW_ID_GENERATOR: IdGenerator = IdGenerator::new(".arrow-decoration-");
    /// As regras de CSS que `Arrow._updateStyle` cria, pelo nome da classe (sem o ponto inicial).
    static ARROW_RULES: RefCell<HashMap<String, ZoneArrowRule>> = RefCell::new(HashMap::new());
}

/// A regra de CSS da seta: `border-style: solid; border-color: transparent; border-bottom-color: <color>;
/// border-width: <height>px; bottom: -<height>px !important; margin-left: -<height>px`. `color` é `None` enquanto a cor não
/// foi dada (o `null` do original).
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneArrowRule {
    pub color: Option<Color>,
    pub height: f64,
}

/// A regra da seta da classe `class_name` (com ou sem o ponto inicial, que a limpeza da classe da decoração tira).
pub fn zone_arrow_rule(class_name: &str) -> Option<ZoneArrowRule> {
    let key = class_name.trim_start_matches('.');
    ARROW_RULES.with(|rules| rules.borrow().get(key).cloned())
}

/// `Arrow`.
struct ZoneArrow {
    rule_name: String,
    decorations: EditorDecorationsCollection,
    color: RefCell<Option<Color>>,
    height: Cell<f64>,
}

impl ZoneArrow {
    fn new(editor: &Rc<CodeEditorWidget>) -> Rc<Self> {
        Rc::new(Self {
            rule_name: ARROW_ID_GENERATOR.with(IdGenerator::next_id),
            decorations: editor.create_decorations_collection(None),
            color: RefCell::new(None),
            height: Cell::new(-1.0),
        })
    }

    fn rule_key(&self) -> &str {
        self.rule_name.trim_start_matches('.')
    }

    /// `set color(value)`.
    fn set_color(&self, value: &Color) {
        if self.color.borrow().as_ref() != Some(value) {
            *self.color.borrow_mut() = Some(value.clone());
            self.update_style();
        }
    }

    /// `set height(value)`.
    fn set_height(&self, value: f64) {
        if self.height.get() != value {
            self.height.set(value);
            self.update_style();
        }
    }

    /// `_updateStyle()`: troca a regra da seta.
    fn update_style(&self) {
        let rule = ZoneArrowRule { color: self.color.borrow().clone(), height: self.height.get() };
        ARROW_RULES.with(|rules| rules.borrow_mut().insert(self.rule_key().to_owned(), rule));
    }

    /// `show(where)`.
    fn show(&self, position: Position) {
        // the arrow isn't pretty at column 1 and we need to push it out a little
        let position = if position.column == 1 { Position::new(position.line_number, 2) } else { position };

        self.decorations.set(&[ModelDeltaDecoration {
            range: Range::from_positions(position, position),
            options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                description: "zone-widget-arrow".to_owned(),
                class_name: Some(self.rule_name.clone()),
                stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
                ..ModelDecorationOptionsInit::default()
            }),
        }]);
    }

    /// `hide()`.
    fn hide(&self) {
        self.decorations.clear();
    }
}

impl IDisposable for ZoneArrow {
    fn dispose(&self) {
        self.hide();
        ARROW_RULES.with(|rules| rules.borrow_mut().remove(self.rule_key()));
    }
}

// #endregion

// #region View zone e overlay

/// O `domNode` da view zone: a `div` de `overflow: hidden` que só reserva o espaço. O pintor o recebe por `downcast` do
/// [`IViewZone::dom_node`].
pub struct ZoneViewZoneNode {
    pub ui: Ui,
    pub node: NodeId,
}

/// `ViewZoneDelegate`. Os campos que o `ZoneWidget` muda depois de adicionar a zona ficam em `Cell`, porque o editor os lê a
/// cada chamada (ver [`IViewZone`]).
struct ViewZoneDelegate {
    zone: Weak<ZoneWidget>,
    dom_node: Rc<ZoneViewZoneNode>,
    id: RefCell<String>,
    after_line_number: Cell<i32>,
    after_column: Cell<i32>,
    height_in_lines: Cell<f64>,
    show_in_hidden_areas: Option<bool>,
    ordinal: Option<i32>,
}

impl ViewZoneDelegate {
    /// `id` (`''` até o `addZone` devolver o identificador).
    fn id(&self) -> String {
        self.id.borrow().clone()
    }

    /// Destrói o nó da zona.
    fn release_node(&self) {
        self.dom_node.ui.remove(self.dom_node.node);
    }
}

impl IViewZone for ViewZoneDelegate {
    fn after_line_number(&self) -> i32 {
        self.after_line_number.get()
    }

    fn after_column(&self) -> Option<i32> {
        Some(self.after_column.get())
    }

    fn show_in_hidden_areas(&self) -> bool {
        self.show_in_hidden_areas.unwrap_or(false)
    }

    fn ordinal(&self) -> Option<i32> {
        self.ordinal
    }

    fn height_in_lines(&self) -> Option<f64> {
        Some(self.height_in_lines.get())
    }

    fn dom_node(&self) -> Rc<dyn Any> {
        self.dom_node.clone()
    }

    fn on_dom_node_top(&self, top: f64) {
        if let Some(zone) = self.zone.upgrade() {
            zone.on_view_zone_top(top);
        }
    }

    fn on_computed_height(&self, height: f64) {
        if let Some(zone) = self.zone.upgrade() {
            zone.on_view_zone_height(height);
        }
    }
}

/// `OverlayWidgetDelegate`: o widget de overlay que registra o `domNode` do `ZoneWidget` no editor.
pub struct OverlayWidgetDelegate {
    id: String,
    dom_node: NodeId,
}

impl OverlayWidgetDelegate {
    pub fn new(id: String, dom_node: NodeId) -> Self {
        Self { id, dom_node }
    }

    /// `getDomNode()`: o nó do widget na árvore.
    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }
}

impl IOverlayWidget for OverlayWidgetDelegate {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_position(&self) -> Option<OverlayWidgetPosition> {
        None
    }
}

// #endregion

/// Os membros que as subclasses de `ZoneWidget` sobrescrevem (ver o topo do módulo). O corpo padrão de cada gancho é o da
/// base.
pub trait ZoneWidgetHooks {
    /// `_fillContainer(container)`: abstrato.
    fn fill_container(&self, zone: &ZoneWidget, container: NodeId);

    /// `_onWidth(widthInPixel)`.
    fn on_width(&self, _zone: &ZoneWidget, _width_in_pixel: f64) {}

    /// `_doLayout(heightInPixel, widthInPixel)`.
    fn do_layout(&self, _zone: &ZoneWidget, _height_in_pixel: f64, _width_in_pixel: f64) {}

    /// `_applyStyles()`.
    fn apply_styles(&self, zone: &ZoneWidget) {
        zone.apply_styles_base();
    }

    /// `_getWidth(info)`.
    fn get_width(&self, zone: &ZoneWidget, info: &EditorLayoutInfo) -> f64 {
        zone.get_width_base(info)
    }

    /// `_getMaximumHeightInLines()`.
    fn get_maximum_height_in_lines(&self, zone: &ZoneWidget) -> Option<f64> {
        zone.get_maximum_height_in_lines_base()
    }

    /// `revealRange(range, isLastLine)`.
    fn reveal_range(&self, zone: &ZoneWidget, range: Range, is_last_line: bool) {
        zone.reveal_range_base(range, is_last_line);
    }

    /// `_getResizeBounds()`.
    fn get_resize_bounds(&self, zone: &ZoneWidget) -> ZoneResizeBounds {
        zone.get_resize_bounds_base()
    }
}

/// Os ganchos de um `ZoneWidget` que ainda não tem subclasse registrada: só os corpos padrão.
struct NoHooks;

impl ZoneWidgetHooks for NoHooks {
    fn fill_container(&self, _zone: &ZoneWidget, _container: NodeId) {}
}

/// O que o arraste do sash lembra: `{ startY, heightInLines, minLines, maxLines }`.
#[derive(Clone, Copy)]
struct ResizeData {
    start_y: f64,
    height_in_lines: f64,
    min_lines: f64,
    max_lines: f64,
}

/// O `IHorizontalSashLayoutProvider` do widget (o `this` do original).
struct ZoneSashLayout {
    zone: Weak<ZoneWidget>,
}

impl HorizontalSashLayoutProvider for ZoneSashLayout {
    fn get_horizontal_sash_top(&self, _sash: &Sash) -> f64 {
        self.zone.upgrade().map_or(0.0, |zone| zone.horizontal_sash_top())
    }

    fn get_horizontal_sash_left(&self, _sash: &Sash) -> Option<f64> {
        Some(0.0)
    }

    fn get_horizontal_sash_width(&self, _sash: &Sash) -> Option<f64> {
        self.zone.upgrade().map(|zone| zone.horizontal_sash_width())
    }
}

/// `ZoneWidget`.
pub struct ZoneWidget {
    me: Weak<ZoneWidget>,
    editor: Rc<CodeEditorWidget>,
    host: ZoneWidgetHost,
    options: RefCell<ZoneWidgetOptions>,
    hooks: RefCell<Option<Weak<dyn ZoneWidgetHooks>>>,
    dom_node: NodeId,
    container: Cell<Option<NodeId>>,
    dom_box: Cell<ZoneDomBox>,
    container_box: Cell<ZoneContainerBox>,
    /// A largura do `border-top` e do `border-bottom` do contêiner.
    border_width: Cell<f64>,
    arrow: RefCell<Option<Rc<ZoneArrow>>>,
    overlay_widget: RefCell<Option<Rc<OverlayWidgetDelegate>>>,
    resize_sash: RefCell<Option<Rc<Sash>>>,
    is_sash_resize_height: Cell<bool>,
    position_marker: EditorDecorationsCollection,
    view_zone: RefCell<Option<Rc<ViewZoneDelegate>>>,
    disposables: Rc<DisposableStore>,
    is_showing: Cell<bool>,
    disposed: Cell<bool>,
}

impl ZoneWidget {
    /// O construtor. `options` recebe os padrões do original ([`ZoneWidgetOptions::mixin_defaults`]).
    pub fn new(editor: &Rc<CodeEditorWidget>, host: ZoneWidgetHost, options: ZoneWidgetOptions) -> Rc<Self> {
        let mut options = options;
        options.mixin_defaults();

        let is_accessible = options.is_accessible.unwrap_or(false);
        let dom_node = {
            let mut tree = host.ui.tree_mut();
            let dom_node = tree.create_detached(if is_accessible { Role::Generic } else { Role::Presentation });
            if !is_accessible {
                tree.set_attribute(dom_node, "aria-hidden", "true");
            }
            dom_node
        };

        let disposables = DisposableStore::new();
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            editor: Rc::clone(editor),
            host,
            options: RefCell::new(options),
            hooks: RefCell::new(None),
            dom_node,
            container: Cell::new(None),
            dom_box: Cell::new(ZoneDomBox::default()),
            container_box: Cell::new(ZoneContainerBox::default()),
            border_width: Cell::new(0.0),
            arrow: RefCell::new(None),
            overlay_widget: RefCell::new(None),
            resize_sash: RefCell::new(None),
            is_sash_resize_height: Cell::new(false),
            position_marker: editor.create_decorations_collection(None),
            view_zone: RefCell::new(None),
            disposables: Rc::clone(&disposables),
            is_showing: Cell::new(false),
            disposed: Cell::new(false),
        });

        let weak = Rc::downgrade(&this);
        disposables.add(editor.on_did_layout_change().listen(move |info: &EditorLayoutInfo| {
            if let Some(zone) = weak.upgrade() {
                zone.on_did_layout_change(info);
            }
        }));
        this
    }

    /// Registra a subclasse como os ganchos do widget (ver o topo do módulo).
    pub fn set_hooks(&self, hooks: Weak<dyn ZoneWidgetHooks>) {
        *self.hooks.borrow_mut() = Some(hooks);
    }

    fn hooks(&self) -> Rc<dyn ZoneWidgetHooks> {
        let hooks: Option<Rc<dyn ZoneWidgetHooks>> = self.hooks.borrow().as_ref().and_then(Weak::upgrade);
        hooks.unwrap_or_else(|| Rc::new(NoHooks))
    }

    // #region Estado

    /// `editor`.
    pub fn editor(&self) -> &Rc<CodeEditorWidget> {
        &self.editor
    }

    /// Onde o widget vive na árvore de widgets.
    pub fn host(&self) -> &ZoneWidgetHost {
        &self.host
    }

    /// `domNode`.
    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// `container` (`null` antes de [`ZoneWidget::create`]).
    pub fn container(&self) -> Option<NodeId> {
        self.container.get()
    }

    /// `options`.
    pub fn options(&self) -> ZoneWidgetOptions {
        self.options.borrow().clone()
    }

    /// `_disposables`.
    pub fn disposables(&self) -> &Rc<DisposableStore> {
        &self.disposables
    }

    /// `_isShowing`.
    pub fn is_showing(&self) -> bool {
        self.is_showing.get()
    }

    /// Os limites do `domNode` (`style.top`, `left`, `width` e `height`).
    pub fn dom_box(&self) -> ZoneDomBox {
        self.dom_box.get()
    }

    /// `_usesResizeHeight`.
    pub fn uses_resize_height(&self) -> bool {
        self.is_sash_resize_height.get()
    }

    /// `_viewZone.heightInLines`.
    pub fn view_zone_height_in_lines(&self) -> Option<f64> {
        self.view_zone.borrow().as_ref().map(|view_zone| view_zone.height_in_lines.get())
    }

    /// `_viewZone.afterLineNumber`.
    pub fn view_zone_after_line_number(&self) -> Option<i32> {
        self.view_zone.borrow().as_ref().map(|view_zone| view_zone.after_line_number.get())
    }

    /// `_viewZone.afterLineNumber = value`: o editor lê o valor no próximo `layoutZone`.
    pub fn set_view_zone_after_line_number(&self, value: i32) {
        if let Some(view_zone) = self.view_zone.borrow().as_ref() {
            view_zone.after_line_number.set(value);
        }
    }

    /// `_viewZone.id`.
    pub fn view_zone_id(&self) -> Option<String> {
        self.view_zone.borrow().as_ref().map(|view_zone| view_zone.id())
    }

    /// `position`: o início do intervalo em que o widget foi mostrado.
    pub fn position(&self) -> Option<Position> {
        self.position_marker.get_range(0).map(|range| range.get_start_position())
    }

    /// `hasFocus()`: `domNode.contains(getActiveElement())`.
    pub fn has_focus(&self) -> bool {
        self.host.ui.focused().is_some_and(|focused| self.host.ui.tree().is_ancestor(self.dom_node, focused))
    }

    // #endregion

    // #region Nós

    /// Reescreve os limites do `domNode` e do contêiner (o `style` de cada um).
    fn sync_bounds(&self) {
        let dom = self.dom_box.get();
        let container_box = self.container_box.get();
        let mut tree = self.host.ui.tree_mut();
        tree.set_bounds(self.dom_node, Rect::new(dom.left, dom.top, dom.left + dom.width, dom.top + dom.height.unwrap_or(0.0)));
        if let Some(container) = self.container.get() {
            let top = container_box.top;
            tree.set_bounds(container, Rect::new(0.0, top, dom.width, top + container_box.height.unwrap_or(0.0)));
        }
    }

    fn update_dom_box(&self, change: impl FnOnce(&mut ZoneDomBox)) {
        let mut dom = self.dom_box.get();
        change(&mut dom);
        self.dom_box.set(dom);
        self.sync_bounds();
    }

    fn update_container_box(&self, change: impl FnOnce(&mut ZoneContainerBox)) {
        let mut container_box = self.container_box.get();
        change(&mut container_box);
        self.container_box.set(container_box);
        self.sync_bounds();
    }

    /// `container.style.borderTopColor/borderBottomColor` e `borderTopWidth/borderBottomWidth`.
    fn apply_container_border(&self) {
        let Some(container) = self.container.get() else {
            return;
        };
        let Some(frame_color) = self.options.borrow().frame_color.clone() else {
            return;
        };
        let width = self.border_width.get();
        let mut tree = self.host.ui.tree_mut();
        let style = tree.style_mut(container);
        style.border = Some(Border { width, color: to_rgba8(&frame_color) });
        style.border_widths = Some([width, 0.0, width, 0.0]);
    }

    /// `setCssClass(className, classToReplace?)`.
    pub fn set_css_class(&self, class_name: &str, class_to_replace: Option<&str>) {
        let Some(container) = self.container.get() else {
            return;
        };
        let mut tree = self.host.ui.tree_mut();
        if let Some(class_to_replace) = class_to_replace.filter(|class| !class.is_empty()) {
            tree.remove_class(container, class_to_replace);
        }
        tree.add_class(container, class_name);
    }

    /// Põe o `domNode` no contêiner dos overlays do editor (o `addOverlayWidget`).
    fn attach_dom_node(&self) {
        self.host.ui.tree_mut().append_child(self.host.overlay_container, self.dom_node);
    }

    /// `this.editor.removeOverlayWidget(this._overlayWidget); this._overlayWidget = null`, mais a saída do nó da árvore.
    fn remove_overlay_widget(&self) {
        let overlay = self.overlay_widget.borrow_mut().take();
        if let Some(overlay) = overlay {
            let widget: Rc<dyn IOverlayWidget> = overlay;
            self.editor.remove_overlay_widget(&widget);
            self.host.ui.tree_mut().detach(self.dom_node);
        }
    }

    // #endregion

    // #region Ciclo de vida

    /// `create()`.
    pub fn create(&self) {
        let options = self.options();
        let container = {
            let mut tree = self.host.ui.tree_mut();
            tree.add_class(self.dom_node, "zone-widget");
            if let Some(class_name) = options.class_name.as_deref().filter(|class| !class.is_empty()) {
                tree.add_class(self.dom_node, class_name);
            }
            create_el(&mut tree, self.dom_node, Role::Generic, "zone-widget-container")
        };
        self.container.set(Some(container));
        self.sync_bounds();
        if options.show_arrow() {
            let arrow = ZoneArrow::new(&self.editor);
            self.disposables.add_rc(Rc::clone(&arrow));
            *self.arrow.borrow_mut() = Some(arrow);
        }
        self.hooks().fill_container(self, container);
        self.init_sash();
        self.apply_styles();
    }

    /// `style(styles)`.
    pub fn style(&self, styles: ZoneWidgetStyles) {
        {
            let mut options = self.options.borrow_mut();
            if let Some(frame_color) = styles.frame_color {
                options.frame_color = Some(frame_color);
            }
            if let Some(arrow_color) = styles.arrow_color {
                options.arrow_color = Some(arrow_color);
            }
        }
        self.apply_styles();
    }

    /// `_applyStyles()`, pelo gancho da subclasse.
    pub fn apply_styles(&self) {
        self.hooks().apply_styles(self);
    }

    /// `ZoneWidget._applyStyles()`.
    pub fn apply_styles_base(&self) {
        self.apply_container_border();
        let arrow_color = self.options.borrow().arrow_color.clone();
        let arrow = self.arrow.borrow().clone();
        if let (Some(arrow), Some(arrow_color)) = (arrow, arrow_color) {
            arrow.set_color(&arrow_color);
        }
    }

    // #endregion

    // #region Medidas

    /// `ZoneWidget._getWidth(info)`.
    pub fn get_width_base(&self, info: &EditorLayoutInfo) -> f64 {
        info.width - info.minimap.minimap_width - info.vertical_scrollbar_width
    }

    /// `_getLeft(info)`.
    fn get_left(info: &EditorLayoutInfo) -> f64 {
        // If minimap is to the left, we move beyond it
        if info.minimap.minimap_width > 0.0 && info.minimap.minimap_left == 0.0 {
            return info.minimap.minimap_width;
        }
        0.0
    }

    fn on_did_layout_change(&self, info: &EditorLayoutInfo) {
        let hooks = self.hooks();
        let width = hooks.get_width(self, info);
        let left = Self::get_left(info);
        self.update_dom_box(|dom| {
            dom.width = width;
            dom.left = left;
        });
        hooks.on_width(self, width);
    }

    /// `_onViewZoneTop(top)`.
    fn on_view_zone_top(&self, top: f64) {
        self.update_dom_box(|dom| dom.top = top);
    }

    /// `_onViewZoneHeight(height)`.
    fn on_view_zone_height(&self, height: f64) {
        self.update_dom_box(|dom| dom.height = Some(height));

        if self.container.get().is_some() {
            let container_height = height - self.decorating_elements_height();
            self.update_container_box(|container| container.height = Some(container_height));
            let layout_info = self.editor.get_layout_info();
            let hooks = self.hooks();
            let width = hooks.get_width(self, &layout_info);
            hooks.do_layout(self, container_height, width);
        }

        let sash = self.resize_sash.borrow().clone();
        if let Some(sash) = sash {
            sash.layout();
        }
    }

    /// `_decoratingElementsHeight()`.
    pub fn decorating_elements_height(&self) -> f64 {
        let line_height = *self.editor.get_option(LINE_HEIGHT);
        let options = self.options();
        let mut result = 0.0;

        if options.show_arrow() {
            let arrow_height = (line_height / 3.0).round();
            result += 2.0 * arrow_height;
        }

        if options.show_frame() {
            let frame_thickness = options.frame_width.unwrap_or_else(|| (line_height / 9.0).round());
            result += 2.0 * frame_thickness;
        }

        result
    }

    /// `ZoneWidget._getMaximumHeightInLines()`: gets the maximum widget height in lines.
    pub fn get_maximum_height_in_lines_base(&self) -> Option<f64> {
        Some((self.editor.get_layout_info().height / *self.editor.get_option(LINE_HEIGHT) * 0.8).max(12.0))
    }

    /// `ZoneWidget._getResizeBounds()`.
    pub fn get_resize_bounds_base(&self) -> ZoneResizeBounds {
        ZoneResizeBounds { min_lines: 5.0, max_lines: 35.0 }
    }

    /// `ZoneWidget.revealRange(range, isLastLine)`.
    pub fn reveal_range_base(&self, range: Range, is_last_line: bool) {
        if is_last_line {
            self.editor.reveal_line_near_top(range.end_line_number, Some(ScrollType::Smooth));
        } else {
            self.editor.reveal_range(range, Some(ScrollType::Smooth), None, None);
        }
    }

    // #endregion

    // #region Mostrar, esconder e redimensionar

    /// `show(rangeOrPos, heightInLines)`.
    pub fn show(&self, range: Range, height_in_lines: f64) {
        self.is_showing.set(true);
        self.show_impl(range, height_in_lines);
        self.is_showing.set(false);
        self.position_marker.set(&[ModelDeltaDecoration { range, options: ModelDecorationOptions::empty() }]);
    }

    /// `updatePositionAndHeight(rangeOrPos, heightInLines?)`.
    pub fn update_position_and_height(&self, range: Range, height_in_lines: Option<f64>) {
        let view_zone = self.view_zone.borrow().clone();
        let Some(view_zone) = view_zone else {
            return;
        };
        let position = range.get_start_position();
        view_zone.after_line_number.set(position.line_number);
        view_zone.after_column.set(position.column);
        view_zone.height_in_lines.set(height_in_lines.unwrap_or_else(|| view_zone.height_in_lines.get()));

        let id = view_zone.id();
        self.editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| accessor.layout_zone(&id));
        self.position_marker.set(&[ModelDeltaDecoration {
            range: Range::from_positions(position, position),
            options: ModelDecorationOptions::empty(),
        }]);
        self.update_sash_enablement();
    }

    /// `hide()`.
    pub fn hide(&self) {
        let view_zone = self.view_zone.borrow().clone();
        if let Some(view_zone) = view_zone {
            let id = view_zone.id();
            self.editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| accessor.remove_zone(&id));
            *self.view_zone.borrow_mut() = None;
            view_zone.release_node();
        }
        self.remove_overlay_widget();
        let arrow = self.arrow.borrow().clone();
        if let Some(arrow) = arrow {
            arrow.hide();
        }
        self.position_marker.clear();
        self.is_sash_resize_height.set(false);
    }

    /// `_showImpl(where, heightInLines)`.
    fn show_impl(&self, range: Range, height_in_lines: f64) {
        let mut height_in_lines = height_in_lines;
        let position = range.get_start_position();
        let layout_info = self.editor.get_layout_info();
        let width = self.hooks().get_width(self, &layout_info);
        let left = Self::get_left(&layout_info);
        self.update_dom_box(|dom| {
            dom.width = width;
            dom.left = left;
        });

        // Render the widget as zone (rendering) and widget (lifecycle)
        let line_height = *self.editor.get_option(LINE_HEIGHT);

        // adjust heightInLines to viewport
        if let Some(max_height_in_lines) = self.hooks().get_maximum_height_in_lines(self) {
            height_in_lines = height_in_lines.min(max_height_in_lines);
        }

        let options = self.options();
        let mut arrow_height = 0.0;
        let mut frame_thickness = 0.0;

        // Render the arrow one 1/3 of an editor line height
        let arrow = self.arrow.borrow().clone();
        if let Some(arrow) = arrow.filter(|_| options.show_arrow()) {
            arrow_height = (line_height / 3.0).round();
            arrow.set_height(arrow_height);
            arrow.show(position);
        }

        // Render the frame as 1/9 of an editor line height
        if options.show_frame() {
            frame_thickness = (line_height / 9.0).round();
        }

        // insert zone widget
        self.editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| {
            let previous = self.view_zone.borrow().clone();
            if let Some(previous) = previous {
                accessor.remove_zone(&previous.id());
                previous.release_node();
            }
            self.remove_overlay_widget();
            self.update_dom_box(|dom| dom.top = -1000.0);
            // A `div` de `overflow: hidden` que reserva o espaço da zona.
            let view_zone_node = {
                let mut tree = self.host.ui.tree_mut();
                let node = tree.create_detached(Role::Generic);
                tree.style_mut(node).clip = true;
                Rc::new(ZoneViewZoneNode { ui: self.host.ui.clone(), node })
            };
            let view_zone = Rc::new(ViewZoneDelegate {
                zone: self.me.clone(),
                dom_node: view_zone_node,
                id: RefCell::new(String::new()),
                after_line_number: Cell::new(position.line_number),
                after_column: Cell::new(position.column),
                height_in_lines: Cell::new(height_in_lines),
                show_in_hidden_areas: options.show_in_hidden_areas,
                ordinal: options.ordinal,
            });
            *self.view_zone.borrow_mut() = Some(Rc::clone(&view_zone));
            // O `onComputedHeight` roda dentro do `addZone`, com o id ainda vazio.
            let id = accessor.add_zone(view_zone.clone());
            *view_zone.id.borrow_mut() = id.clone();
            let overlay = Rc::new(OverlayWidgetDelegate::new(format!("{WIDGET_ID}{id}"), self.dom_node));
            *self.overlay_widget.borrow_mut() = Some(Rc::clone(&overlay));
            self.attach_dom_node();
            self.editor.add_overlay_widget(overlay);
        });
        self.update_sash_enablement();

        if self.container.get().is_some() && options.show_frame() {
            let frame_width = options.frame_width.filter(|frame_width| *frame_width != 0.0).unwrap_or(frame_thickness);
            self.border_width.set(frame_width);
            self.apply_container_border();
        }

        let container_height = height_in_lines * line_height - self.decorating_elements_height();

        if let Some(container) = self.container.get() {
            self.update_container_box(|container_box| {
                container_box.top = arrow_height;
                container_box.height = Some(container_height);
            });
            self.host.ui.tree_mut().style_mut(container).clip = true;
        }

        self.hooks().do_layout(self, container_height, width);

        if !options.keep_editor_selection.unwrap_or(false) {
            self.editor.set_selection(SelectionOrRange::from(range), None);
        }

        if let Some(model) = self.editor.get_model() {
            let reveal = model.validate_range(&Range::new(range.start_line_number, 1, range.end_line_number + 1, 1));
            let is_last_line = reveal.start_line_number == model.get_line_count();
            self.hooks().reveal_range(self, reveal, is_last_line);
        }
    }

    /// `_relayout(newHeightInLines, useMax?)`.
    pub fn relayout(&self, new_height_in_lines: f64, use_max: bool) {
        let max_height_in_lines = self.hooks().get_maximum_height_in_lines(self);
        let new_height_in_lines = match max_height_in_lines {
            Some(max_height_in_lines) if use_max => max_height_in_lines.min(new_height_in_lines),
            _ => new_height_in_lines,
        };
        let view_zone = self.view_zone.borrow().clone();
        if let Some(view_zone) = view_zone
            && view_zone.height_in_lines.get() != new_height_in_lines
        {
            let id = view_zone.id();
            self.editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| {
                view_zone.height_in_lines.set(new_height_in_lines);
                accessor.layout_zone(&id);
            });
            self.update_sash_enablement();
        }
    }

    // #endregion

    // #region Sash

    /// `_initSash()`.
    fn init_sash(&self) {
        if self.resize_sash.borrow().is_some() {
            return;
        }
        let sash = Sash::new_horizontal(&self.host.ui, self.dom_node, Rc::new(ZoneSashLayout { zone: self.me.clone() }), SashOptions::default());
        self.disposables.add_rc(Rc::clone(&sash));
        *self.resize_sash.borrow_mut() = Some(Rc::clone(&sash));

        if !self.options().is_resizeable.unwrap_or(false) {
            sash.set_state(SashState::Disabled);
        }

        let data: Rc<Cell<Option<ResizeData>>> = Rc::new(Cell::new(None));

        let (weak, start_data) = (self.me.clone(), Rc::clone(&data));
        self.disposables.add(sash.on_did_start().listen(move |event: &SashEvent| {
            let Some(zone) = weak.upgrade() else {
                return;
            };
            let view_zone = zone.view_zone.borrow().clone();
            if let Some(view_zone) = view_zone {
                let bounds = zone.hooks().get_resize_bounds(&zone);
                start_data.set(Some(ResizeData {
                    start_y: event.start_y,
                    height_in_lines: view_zone.height_in_lines.get(),
                    min_lines: bounds.min_lines,
                    max_lines: bounds.max_lines,
                }));
            }
        }));

        let end_data = Rc::clone(&data);
        self.disposables.add(sash.on_did_end().listen(move |_| end_data.set(None)));

        let (weak, change_data) = (self.me.clone(), data);
        self.disposables.add(sash.on_did_change().listen(move |event: &SashEvent| {
            let (Some(data), Some(zone)) = (change_data.get(), weak.upgrade()) else {
                return;
            };
            let line_delta = (event.current_y - data.start_y) / *zone.editor.get_option(LINE_HEIGHT);
            let rounded_line_delta = if line_delta < 0.0 { line_delta.ceil() } else { line_delta.floor() };
            let new_height_in_lines = data.height_in_lines + rounded_line_delta;

            if new_height_in_lines > data.min_lines && new_height_in_lines < data.max_lines {
                zone.is_sash_resize_height.set(true);
                zone.relayout(new_height_in_lines, false);
            }
        }));
    }

    /// `_updateSashEnablement()`.
    fn update_sash_enablement(&self) {
        let sash = self.resize_sash.borrow().clone();
        if let Some(sash) = sash {
            let bounds = self.hooks().get_resize_bounds(self);
            sash.set_state(if bounds.min_lines == bounds.max_lines { SashState::Disabled } else { SashState::Enabled });
        }
    }

    /// `getHorizontalSashTop()`.
    fn horizontal_sash_top(&self) -> f64 {
        self.dom_box.get().height.map_or(0.0, f64::trunc) - self.decorating_elements_height() / 2.0
    }

    /// `getHorizontalSashWidth()`.
    fn horizontal_sash_width(&self) -> f64 {
        let layout_info = self.editor.get_layout_info();
        layout_info.width - layout_info.minimap.minimap_width
    }

    // #endregion
}

impl IDisposable for ZoneWidget {
    /// `dispose()`. O nó do widget sai da árvore no fim (o coletor de lixo do original faz isso sozinho).
    fn dispose(&self) {
        self.remove_overlay_widget();

        if self.view_zone.borrow().is_some() {
            self.editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| {
                let view_zone = self.view_zone.borrow_mut().take();
                if let Some(view_zone) = view_zone {
                    accessor.remove_zone(&view_zone.id());
                    view_zone.release_node();
                }
            });
        }

        self.position_marker.clear();

        self.disposables.dispose();

        if !self.disposed.replace(true) {
            self.host.ui.remove(self.dom_node);
        }
    }
}
