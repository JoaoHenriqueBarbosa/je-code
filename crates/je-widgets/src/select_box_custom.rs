//! Porte de `src/vs/base/browser/ui/selectBox/selectBoxCustom.ts` (e do `selectBoxCustom.css`): o
//! [`SelectBoxList`], a caixa de seleção cuja lista suspensa é desenhada numa visão de contexto, com a
//! [`List`] já portada, navegação por teclado, separadores, `detail`/`decoratorRight`, opções desabilitadas,
//! painel de detalhes (ou dicas à direita) e posição acima ou abaixo da âncora conforme o espaço.
//!
//! É a única implementação da caixa (o `SelectBoxNative` virou esta, ver [`crate::select_box`]).
//!
//! # Como o DOM e o CSS viram [`je_ui`]
//!
//! - O `<select>` é um nó (`select_element`, classe `monaco-select-box`, papel `ComboBox`) com um filho para o
//!   texto da opção selecionada e outro `chevron-down` (o `.select-container:after` do `style.css` do workbench,
//!   que no original é filho do contêiner; aqui é do próprio nó, o que é igual quando o contêiner abraça a
//!   caixa, como no item de uma barra de ações). O que o navegador faz sozinho vira estado do widget:
//!   [`SelectBoxList::selected_index`] (`selectedIndex`), [`SelectBoxList::aria_expanded`] (`aria-expanded`),
//!   [`SelectBoxList::tab_index`], `disabled` (por [`SelectBoxList::set_enabled`], que também grava o atributo `disabled` no nó, como o
//!   `selectElement.disabled` reflete no DOM; um `select` desabilitado não recebe clique nem tecla). Um clique no nó (evento `Click`) abre ou fecha a lista.
//! - O contêiner da lista (`.monaco-select-box-dropdown-container`) é um nó destacado que o `render` da visão de
//!   contexto anexa. O CSS `display: none` / `.visible` é `style.visible` mais a classe `visible`. Sem motor de
//!   layout, o tamanho (largura ótima, altura da lista, altura do painel de detalhes) é calculado como o
//!   original calcula e escrito nos nós por `apply_geometry`; o nó pai (a `.context-view`) é dimensionado para a
//!   caixa, o que o `display: flex` do original faz sozinho.
//! - A folha de estilo injetada (`initStyleSheet`) e as regras do `selectBoxCustom.css` viram código: a linha
//!   é refeita por `refresh_row` (posições, `text-overflow: ellipsis`, cores) quando renderiza e quando o foco
//!   muda; `:hover` de uma linha equivale ao foco (o mouse sobre uma opção a foca, `onMouseOver`), então a regra
//!   `:not(.focused):hover` nunca tem efeito distinto e não existe. O contorno de foco e de hover é uma borda de
//!   1px por dentro (`outline-offset: -1px`).
//!   A borda de cima/de baixo do painel de detalhes é um nó de 1px.
//! - O `::after` do separador é o nó `option-separator-line` (metade da altura da linha, 1px).
//!
//! # Diferenças
//!
//! - **Ponteiro e foco.** O `FOCUS_OUT` do original confere `relatedTarget`; o `Blur` do `je-ui` não o traz,
//!   então perder o foco da lista cancela sempre (o foco só sai da lista para fora da caixa, porque a barra de
//!   rolagem não toma foco). Toque (`touchstart`/`touchend`), `draggable` e `dragstart` não existem no `je-ui`.
//! - **Teclado do `<select>` fechado.** O nativo muda a seleção com as setas e dispara `change`; aqui as setas
//!   para cima e para baixo (sem Alt) fazem isso, pulando opções desabilitadas. Início, fim e digitar para
//!   achar não existem fora da lista aberta.
//! - **Largura.** O elemento de controle de largura (`.select-box-dropdown-container-width-control`) é só uma
//!   medição: a largura da maior opção (texto mais `decoratorRight`) mais 16px de preenchimento sai de
//!   `measure_text`, sem nós.
//! - **Markdown.** `descriptionIsMarkdown` não tem renderizador (TODO(integração): `renderMarkdown`): a
//!   descrição aparece como texto, quebrada por palavra.
//! - **Sombra** (`box-shadow: shadow-lg`) e a fonte herdada (`getComputedStyle(...).fontSize`, sempre 13px,
//!   11px no macOS) não existem. A cor do texto das linhas é a de `selectForeground` (no original, a herdada do
//!   workbench).
//! - Medidas do `<select>` (padding, altura mínima): as do `.monaco-action-bar .action-item .monaco-select-box`
//!   quando há um `.action-item` entre os antepassados; fora dele, as do `<select>` sem CSS (altura da linha
//!   mais a borda) com o espaço do chevron à direita. `opacity: 0.7` do `select:disabled` é a folha UA do
//!   Chromium, NÃO MEDIDO no oráculo.
//! - Dica do `title` (`setupManagedHover`) e da descrição (`showDelayedHover`) usam os delegados globais de
//!   [`crate::hover_api`] (sem serviço, nada aparece).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::{KeyCode, KeyCodeUtils};
use je_base::layout::AnchorPosition;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable, to_disposable};
use je_base::nls::localize;
use je_base::runtime;
use je_base::scrollable::ScrollbarVisibility;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Border, Content, Cursor, Event, EventKind, NodeId, Role, Ui};

use crate::common::{js_round, registered_emitter};
use crate::context_view_api::{Anchor, IContextViewDelegate, IContextViewProvider};
use crate::dom::{StandardKeyboardEvent, clear_node, create_el, window_size};
use crate::hover_api::{HoverContent, HoverPlacement, HoverPosition, HoverTarget, IHoverLifecycleOptions, IHoverOptions, IHoverPositionOptions, IHoverWidget, IManagedHover, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::hover_widget::wrap_text;
use crate::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails, ListEvent, TemplateData};
use crate::list::list_view::ListViewOptions;
use crate::list::list_widget::{List, ListOptions};
use crate::select_box::{CORNER_RADIUS_LARGE, CORNER_RADIUS_SMALL, ISelectBoxDelegate, ISelectBoxOptions, ISelectBoxStyles, ISelectData, ISelectOptionItem};
use crate::text::{clear_node_content, clip_text, ellipsize, measure_text, natural_line_height, set_node_text, ui_font};

const SELECT_OPTION_ENTRY_TEMPLATE_ID: &str = "selectOption.entry.template";
const SELECT_OPTION_HEIGHT: f64 = 22.0;
const DEFAULT_DROPDOWN_MINIMUM_BOTTOM_MARGIN: f64 = 32.0;
const DEFAULT_DROPDOWN_MINIMUM_TOP_MARGIN: f64 = 2.0;
const DEFAULT_MINIMUM_VISIBLE_OPTIONS: i64 = 3;
/// O `font-size` do `<select>` e da lista (`--vscode-font-size`).
const FONT_SIZE: f32 = 13.0;
/// O `font-size` do `<select>` fechado: `.monaco-workbench select` só herda a família, e o tamanho fica o da
/// folha UA do Chromium (`-webkit-small-control`, 13,3333 px; medido no oráculo, H26).
const SELECT_ELEMENT_FONT_SIZE: f32 = 40.0 / 3.0;
/// A cor do texto quando o estilo não define `selectForeground` (o `foreground` do tema escuro).
const FALLBACK_FOREGROUND: Rgba = Rgba::new(204, 204, 204, 255);
/// A largura e a altura do chevron.
const CHEVRON_SIZE: f64 = 16.0;

/// O número de unidades UTF-16 (o `length` de uma string do JavaScript).
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// As medidas CSS do `<select>` que dependem de onde ele está.
#[derive(Clone, Copy)]
struct SelectMetrics {
    /// `padding`: cima, direita, baixo, esquerda.
    padding: [f64; 4],
    min_width: f64,
    min_height: f64,
}

/// `.monaco-action-bar .action-item .monaco-select-box`.
const ACTION_ITEM_METRICS: SelectMetrics = SelectMetrics { padding: [2.0, 23.0, 2.0, 8.0], min_width: 100.0, min_height: 18.0 };
/// O `<select>` sem CSS de contexto.
const PLAIN_METRICS: SelectMetrics = SelectMetrics { padding: [0.0, 23.0, 0.0, 8.0], min_width: 0.0, min_height: 18.0 };

/// O que cada linha da lista guarda (o `ISelectListTemplateData`).
struct RowTemplate {
    root: NodeId,
    text: NodeId,
    detail: NodeId,
    decorator_right: NodeId,
    separator_line: NodeId,
    index: Cell<Option<usize>>,
    element: RefCell<Option<ISelectOptionItem>>,
}

/// `SelectListRenderer`. O `elements: Map<option, HTMLElement>` do original vira o mapa de linhas por nó, de
/// onde [`SelectListRenderer::get_element`] acha a linha de um índice da lista.
struct SelectListRenderer {
    ui: Ui,
    styles: ISelectBoxStyles,
    row_width: Rc<Cell<f64>>,
    rows: RefCell<HashMap<NodeId, Rc<RowTemplate>>>,
}

impl SelectListRenderer {
    /// `getElement`: a linha renderizada do índice da lista.
    fn get_element(&self, list_index: usize) -> Option<NodeId> {
        self.rows.borrow().values().find(|row| row.index.get() == Some(list_index)).map(|row| row.root)
    }

    fn refresh_all(&self) {
        let rows: Vec<_> = self.rows.borrow().values().cloned().collect();
        for row in rows {
            self.refresh_row(&row);
        }
    }

    /// As regras de `selectBoxCustom.css` e da folha injetada para uma linha: posição dos três trechos de texto
    /// (`float: left`, `float: left`, `float: right`) com reticências, cores do foco, o separador.
    fn refresh_row(&self, row: &RowTemplate) {
        let Some(option) = row.element.borrow().clone() else { return };
        let styles = &self.styles;
        let row_width = self.row_width.get();
        let mut tree = self.ui.tree_mut();
        let focused = tree.has_class(row.root, "focused");
        let disabled = tree.has_class(row.root, "option-disabled");
        let separator = option.is_separator;
        // `.option-disabled.focused` limpa fundo, cor e contorno.
        let show_focus = focused && !disabled;
        let base = styles.select_foreground.unwrap_or(FALLBACK_FOREGROUND);
        let row_color = if show_focus { styles.list_focus_foreground.unwrap_or(base) } else { base };

        {
            let style = tree.style_mut(row.root);
            style.background = if show_focus { styles.list_focus_background } else { None };
            style.border = if show_focus { styles.list_focus_outline.filter(|c| c.a > 0).map(|color| Border { width: 1.0, color }) } else { None };
            style.radius = 0.0;
            style.clip = true;
            style.cursor = Some(if separator { Cursor::Default } else { Cursor::Pointer });
        }

        let font = ui_font(FONT_SIZE, 400);
        let decorator_font = ui_font(if separator { 12.0 } else { FONT_SIZE }, 400);
        let decorator = option.decorator_right.clone().unwrap_or_default();
        let decorator_width = measure_text(&decorator, &decorator_font);
        // `padding-right: 10px` do `.option-decorator-right`.
        let decorator_total = if decorator.is_empty() { 0.0 } else { decorator_width + 10.0 };
        let available_left = (row_width - decorator_total).max(0.0);
        // `padding-left: 2px` da linha mais `padding-left: 6px` do trecho.
        let left_start = 8.0;

        let mut text_width = 0.0;
        if separator {
            // `visibility: hidden; width: 0`
            tree.style_mut(row.text).visible = false;
        } else {
            let text = ellipsize(&option.text, &font, (available_left - left_start).max(0.0));
            tree.style_mut(row.text).visible = true;
            text_width = set_node_text(&mut tree, row.text, &text, &font, row_color, SELECT_OPTION_HEIGHT);
            tree.set_bounds(row.text, Rect::new(left_start, 0.0, left_start + text_width, SELECT_OPTION_HEIGHT));
        }

        let detail = option.detail.clone().unwrap_or_default();
        if separator || detail.is_empty() {
            tree.style_mut(row.detail).visible = false;
            clear_node_content(&mut tree, row.detail);
        } else {
            let x = left_start + text_width + 6.0;
            let text = ellipsize(&detail, &font, (available_left - x).max(0.0));
            let style = tree.style_mut(row.detail);
            style.visible = true;
            style.opacity = 0.7;
            let width = set_node_text(&mut tree, row.detail, &text, &font, row_color, SELECT_OPTION_HEIGHT);
            tree.set_bounds(row.detail, Rect::new(x, 0.0, x + width, SELECT_OPTION_HEIGHT));
        }

        if decorator.is_empty() {
            tree.style_mut(row.decorator_right).visible = false;
            clear_node_content(&mut tree, row.decorator_right);
        } else {
            let color = if show_focus {
                row_color
            } else if separator {
                styles.decorator_right_foreground.or(styles.description_foreground).unwrap_or(row_color)
            } else {
                styles.decorator_right_foreground.unwrap_or(row_color)
            };
            tree.style_mut(row.decorator_right).visible = true;
            set_node_text(&mut tree, row.decorator_right, &decorator, &decorator_font, color, SELECT_OPTION_HEIGHT);
            tree.set_bounds(row.decorator_right, Rect::new(0.0, 0.0, decorator_width, SELECT_OPTION_HEIGHT));
            tree.set_right(row.decorator_right, Some(10.0));
        }

        // `::after` do separador: `left: 0; right: 0; top: 50%; height: 1px`.
        tree.style_mut(row.separator_line).visible = separator;
        if separator {
            tree.set_fill(row.separator_line, true, false);
            tree.set_bounds(row.separator_line, Rect::new(0.0, SELECT_OPTION_HEIGHT / 2.0, 0.0, SELECT_OPTION_HEIGHT / 2.0 + 1.0));
            tree.style_mut(row.separator_line).background = styles.menu_separator_background;
        }
    }

    fn template_of(&self, template_data: &TemplateData) -> Rc<RowTemplate> {
        template_data.clone().downcast::<RowTemplate>().unwrap_or_else(|_| crate::common::throw("template de linha inesperado"))
    }
}

impl IListRenderer<ISelectOptionItem> for SelectListRenderer {
    fn template_id(&self) -> String {
        SELECT_OPTION_ENTRY_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let mut tree = self.ui.tree_mut();
        let text = create_el(&mut tree, container, Role::Generic, "option-text");
        let detail = create_el(&mut tree, container, Role::Generic, "option-detail");
        let decorator_right = create_el(&mut tree, container, Role::Generic, "option-decorator-right");
        let separator_line = create_el(&mut tree, container, Role::Generic, "option-separator-line");
        tree.style_mut(separator_line).visible = false;
        let row = Rc::new(RowTemplate { root: container, text, detail, decorator_right, separator_line, index: Cell::new(None), element: RefCell::new(None) });
        self.rows.borrow_mut().insert(container, row.clone());
        row
    }

    fn render_element(&self, element: &ISelectOptionItem, index: usize, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        let row = self.template_of(template_data);
        row.index.set(Some(index));
        *row.element.borrow_mut() = Some(element.clone());

        {
            let mut tree = self.ui.tree_mut();
            // pseudo-select disabled option; o separador também é desabilitado.
            tree.toggle_class(row.root, "option-disabled", element.is_disabled || element.is_separator);
            tree.toggle_class(row.root, "option-separator", element.is_separator);
            tree.set_role(row.root, Role::Option);
            tree.set_label(row.root, Self::aria_label(element));
        }
        self.refresh_row(&row);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let row = self.template_of(template_data);
        self.rows.borrow_mut().remove(&row.root);
    }
}

impl SelectListRenderer {
    /// `accessibilityProvider.getAriaLabel`.
    fn aria_label(element: &ISelectOptionItem) -> String {
        if element.is_separator {
            return localize("selectBoxSeparator", "separator", &[]);
        }
        let mut label = element.text.clone();
        for part in [&element.detail, &element.decorator_right, &element.description].into_iter().flatten().filter(|p| !p.is_empty()) {
            label.push_str(". ");
            label.push_str(part);
        }
        label
    }
}

/// O delegado de alturas da lista (o `SelectBoxList` implementa `IListVirtualDelegate`: toda opção tem 22px).
struct OptionHeights;

impl IListVirtualDelegate<ISelectOptionItem> for OptionHeights {
    fn get_height(&self, _element: &ISelectOptionItem) -> f64 {
        SELECT_OPTION_HEIGHT
    }

    fn get_template_id(&self, _element: &ISelectOptionItem) -> String {
        SELECT_OPTION_ENTRY_TEMPLATE_ID.to_owned()
    }
}

/// O `IDelegate` que o `showContextView` recebe. A posição é a que o `_dropDownPosition` tinha na chamada
/// (o objeto literal do original lê o campo na hora de criá-lo).
struct DropDownDelegate {
    owner: Weak<SelectBoxList>,
    position: AnchorPosition,
    pre_layout: bool,
}

impl IContextViewDelegate for DropDownDelegate {
    fn get_anchor(&self) -> Anchor {
        self.owner.upgrade().map_or(Anchor::Mouse { x: 0.0, y: 0.0 }, |owner| Anchor::Node(owner.select_element))
    }

    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        self.owner.upgrade().map(|owner| owner.render_select_drop_down(container, self.pre_layout))
    }

    fn layout(&self) {
        if let Some(owner) = self.owner.upgrade() {
            owner.layout_select_drop_down(false);
        }
    }

    fn on_hide(&self, _did_cancel: Option<bool>) {
        if let Some(owner) = self.owner.upgrade() {
            owner.hide_popup_node();
        }
    }

    fn anchor_position(&self) -> Option<AnchorPosition> {
        Some(self.position)
    }

    fn layer(&self) -> i32 {
        self.owner.upgrade().and_then(|owner| owner.select_box_options.borrow().context_view_layer).unwrap_or(0)
    }
}

/// `SelectBoxList`.
pub struct SelectBoxList {
    base: Rc<Disposable>,
    weak_self: Weak<SelectBoxList>,
    ui: Ui,
    is_visible: Cell<bool>,
    select_box_options: RefCell<ISelectBoxOptions>,
    select_element: NodeId,
    select_text: NodeId,
    chevron: NodeId,
    container: Cell<Option<NodeId>>,
    options: RefCell<Vec<ISelectOptionItem>>,
    selected: Cell<usize>,
    /// `selectElement.selectedIndex` (-1 sem opções).
    selected_index: Cell<i32>,
    enabled: Cell<bool>,
    tab_index: Cell<i32>,
    aria_expanded: Cell<Option<bool>>,
    on_did_select: Rc<Emitter<ISelectData>>,
    styles: ISelectBoxStyles,
    renderer: Rc<SelectListRenderer>,
    row_width: Rc<Cell<f64>>,
    context_view_provider: Rc<dyn IContextViewProvider>,
    select_drop_down_container: NodeId,
    select_drop_down_list_container: Cell<Option<NodeId>>,
    select_list: RefCell<Option<Rc<List<ISelectOptionItem>>>>,
    selection_details_pane: NodeId,
    list_option_indexes: RefCell<Vec<usize>>,
    current_selection: Cell<usize>,
    drop_down_position: Cell<AnchorPosition>,
    has_details: Cell<bool>,
    option_description_hover: Rc<MutableDisposable<dyn IHoverWidget>>,
    skip_layout: Cell<bool>,
    cached_max_details_height: Cell<Option<f64>>,
    hover: RefCell<Option<Rc<dyn IManagedHover>>>,
    /// Largura da caixa da lista (`selectDropDownContainer.style.width`).
    popup_width: Cell<f64>,
    /// Altura pedida da lista (`selectList.getHTMLElement().style.height`).
    list_height: Cell<f64>,
    /// Altura em que a lista é de fato desenhada (sem a borda da caixa, que recorta o excesso).
    list_height_drawn: Cell<f64>,
    /// `selectionDetailsPane.offsetHeight`.
    details_height: Cell<f64>,
}

impl SelectBoxList {
    /// `new SelectBoxList(options, selected, contextViewProvider, styles, selectBoxOptions)`.
    pub fn new(ui: &Ui, options: Vec<ISelectOptionItem>, selected: usize, context_view_provider: Rc<dyn IContextViewProvider>, styles: ISelectBoxStyles, select_box_options: Option<ISelectBoxOptions>) -> Rc<Self> {
        let mut select_box_options = select_box_options.unwrap_or_default();
        select_box_options.min_bottom_margin = Some(match select_box_options.min_bottom_margin {
            None => DEFAULT_DROPDOWN_MINIMUM_BOTTOM_MARGIN,
            Some(margin) => margin.max(0.0),
        });

        let base = Disposable::new();
        let (select_element, select_text, chevron, popup, pane) = {
            let mut tree = ui.tree_mut();
            let select_element = tree.create_detached(Role::ComboBox);
            tree.set_class_name(select_element, "monaco-select-box");
            let select_text = create_el(&mut tree, select_element, Role::Generic, "select-box-text");
            let chevron = create_el(&mut tree, select_element, Role::Generic, "select-box-chevron");
            let popup = tree.create_detached(Role::Generic);
            tree.set_class_name(popup, "monaco-select-box-dropdown-container");
            tree.style_mut(popup).visible = false;
            let pane = create_el(&mut tree, popup, Role::Generic, "select-box-details-pane");
            if let Some(label) = &select_box_options.aria_label {
                tree.set_label(select_element, label.clone());
            }
            (select_element, select_text, chevron, popup, pane)
        };
        let row_width = Rc::new(Cell::new(0.0));
        let renderer = Rc::new(SelectListRenderer { ui: ui.clone(), styles: styles.clone(), row_width: row_width.clone(), rows: RefCell::default() });

        let this = Rc::new_cyclic(|weak| Self {
            base: base.clone(),
            weak_self: weak.clone(),
            ui: ui.clone(),
            is_visible: Cell::new(false),
            select_box_options: RefCell::new(select_box_options),
            select_element,
            select_text,
            chevron,
            container: Cell::new(None),
            options: RefCell::new(Vec::new()),
            selected: Cell::new(selected),
            selected_index: Cell::new(-1),
            enabled: Cell::new(true),
            tab_index: Cell::new(0),
            aria_expanded: Cell::new(None),
            on_did_select: registered_emitter(&base),
            styles,
            renderer,
            row_width,
            context_view_provider,
            select_drop_down_container: popup,
            select_drop_down_list_container: Cell::new(None),
            select_list: RefCell::new(None),
            selection_details_pane: pane,
            list_option_indexes: RefCell::default(),
            current_selection: Cell::new(0),
            // Always default to below position
            drop_down_position: Cell::new(AnchorPosition::Below),
            has_details: Cell::new(false),
            option_description_hover: registered_mutable(&base),
            skip_layout: Cell::new(false),
            cached_max_details_height: Cell::new(None),
            hover: RefCell::new(None),
            popup_width: Cell::new(0.0),
            list_height: Cell::new(0.0),
            list_height_drawn: Cell::new(0.0),
            details_height: Cell::new(0.0),
        });
        this.register_listeners();
        this.set_options(options, Some(selected));
        this.style_popup();
        this
    }

    /// `this._register(dom.addDisposableListener(...))` com o `this` fraco.
    fn listen(&self, node: NodeId, kind: EventKind, capture: bool, handler: impl Fn(&SelectBoxList, &mut Event) + 'static) {
        let weak = self.weak_self.clone();
        self.base.register(self.ui.listen(node, kind, capture, move |e| {
            if let Some(this) = weak.upgrade() {
                handler(&this, e);
            }
        }));
    }

    // ---- O `<select>` ----

    /// O nó do `<select>` (`container.querySelector('select')`).
    pub fn select_element(&self) -> NodeId {
        self.select_element
    }

    /// `selectElement.selectedIndex`: -1 sem opções.
    pub fn selected_index(&self) -> i32 {
        self.selected_index.get()
    }

    /// `selectElement.selectedOptions[0].text`.
    pub fn selected_option_text(&self) -> Option<String> {
        usize::try_from(self.selected_index.get()).ok().and_then(|i| self.options.borrow().get(i).map(|o| o.text.clone()))
    }

    /// `selectElement.getAttribute('aria-expanded')`: `None` até a primeira abertura.
    pub fn aria_expanded(&self) -> Option<bool> {
        self.aria_expanded.get()
    }

    /// `selectElement.tabIndex`.
    pub fn tab_index(&self) -> i32 {
        self.tab_index.get()
    }

    /// A caixa da lista suspensa (`.monaco-select-box-dropdown-container`).
    pub fn drop_down_node(&self) -> NodeId {
        self.select_drop_down_container
    }

    fn selected_text(&self) -> Option<String> {
        self.options.borrow().get(self.selected.get()).map(|o| o.text.clone()).filter(|t| !t.is_empty())
    }

    fn register_listeners(&self) {
        let select = self.select_element;

        // Intercept mouse events to override normal select actions on parents
        self.listen(select, EventKind::Click, false, |this, e| {
            e.stop(false);
            if !this.enabled.get() {
                return;
            }
            if this.is_visible.get() {
                this.cancel_select_drop_down(true);
            } else {
                this.show_select_drop_down();
            }
        });
        self.listen(select, EventKind::PointerDown, false, |_, e| e.stop(false));

        // Intercept keyboard handling
        self.listen(select, EventKind::KeyDown, false, |this, e| {
            let Some(key) = StandardKeyboardEvent::from_event(e) else { return };
            if !this.enabled.get() {
                return;
            }
            let (code, alt) = (key.key_code, key.alt_key);
            let show_drop_down = if crate::sash::is_macintosh() {
                code == KeyCode::DownArrow || code == KeyCode::UpArrow || code == KeyCode::Space || code == KeyCode::Enter
            } else {
                (code == KeyCode::DownArrow && alt) || (code == KeyCode::UpArrow && alt) || code == KeyCode::Space || code == KeyCode::Enter
            };
            if show_drop_down {
                // Create and drop down select list on keyboard select
                this.show_select_drop_down();
                e.stop(true);
            } else if !alt && (code == KeyCode::DownArrow || code == KeyCode::UpArrow) {
                this.change_native_selection(code == KeyCode::DownArrow);
            }
        });

        for kind in [EventKind::Focus, EventKind::Blur] {
            self.listen(select, kind, false, |this, _| this.refresh_select_style());
        }
    }

    /// O que o `<select>` nativo fechado faz com a seta (muda para a opção habilitada vizinha) e o `change`
    /// que o `registerListeners` do original escuta.
    fn change_native_selection(&self, forward: bool) {
        let target = {
            let options = self.options.borrow();
            let current = self.selected.get();
            if forward { (current + 1..options.len()).find(|i| !options[*i].is_disabled) } else { (0..current).rev().find(|i| !options[*i].is_disabled) }
        };
        let Some(index) = target else { return };
        self.selected.set(index);
        self.selected_index.set(index as i32);
        let text = self.options.borrow()[index].text.clone();
        self.on_did_select.fire(ISelectData { index, selected: text.clone() });
        if !text.is_empty() {
            self.set_title(&text);
        }
        self.layout();
    }

    fn set_title(&self, title: &str) {
        let existing = self.hover.borrow().clone();
        match existing {
            Some(hover) => hover.update(ManagedHoverContentOrFactory::from(title), None),
            None if !title.is_empty() => {
                let hover = get_base_layer_hover_delegate().setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), self.select_element, ManagedHoverContentOrFactory::from(title), None);
                let disposable: DisposableRef = hover.clone();
                self.base.register(disposable);
                *self.hover.borrow_mut() = Some(hover);
            }
            None => {}
        }
    }

}

impl ISelectBoxDelegate for SelectBoxList {
    fn on_did_select(&self) -> BaseEvent<ISelectData> {
        self.on_did_select.event()
    }

    fn set_options(&self, options: Vec<ISelectOptionItem>, selected: Option<usize>) {
        if *self.options.borrow() != options {
            let show_hovers = self.select_box_options.borrow().show_option_description_hovers;
            self.has_details.set(!show_hovers && options.iter().any(|o| o.description.is_some()));
            self.cached_max_details_height.set(None);
            self.selected_index.set(if options.is_empty() { -1 } else { 0 });
            *self.options.borrow_mut() = options;
            self.layout();
        }

        if let Some(selected) = selected {
            self.select(selected);
            // Set current = selected since this is not necessarily a user exit
            self.current_selection.set(self.selected.get());
        }
    }

    fn set_enabled(&self, enable: bool) {
        self.enabled.set(enable);
        {
            // `selectElement.disabled = !enable`: o atributo booleano `disabled` do `<select>`.
            let mut tree = self.ui.tree_mut();
            if enable {
                tree.remove_attribute(self.select_element, "disabled");
            } else {
                tree.set_attribute(self.select_element, "disabled", "");
            }
        }
        self.refresh_select_style();
    }

    /// `select(index)`: um índice depois do fim vale o último. (Sem opções nada muda.)
    fn select(&self, index: usize) {
        let len = self.options.borrow().len();
        if len > 0 {
            self.selected.set(index.min(len - 1));
        }
        self.selected_index.set(if self.selected.get() < len { self.selected.get() as i32 } else { -1 });
        if let Some(text) = self.selected_text() {
            self.set_title(&text);
        }
        self.layout();
    }

    fn set_aria_label(&self, label: &str) {
        self.select_box_options.borrow_mut().aria_label = Some(label.to_owned());
        self.ui.tree_mut().set_label(self.select_element, label.to_owned());
    }

    fn focus(&self) {
        self.tab_index.set(0);
        if self.enabled.get() {
            self.ui.focus(self.select_element);
        }
    }

    fn blur(&self) {
        self.tab_index.set(-1);
        if self.ui.focused() == Some(self.select_element) {
            self.ui.blur();
        }
    }

    fn set_focusable(&self, focusable: bool) {
        self.tab_index.set(if focusable { 0 } else { -1 });
    }

    /// `render(container)`.
    fn render(&self, container: NodeId) {
        self.container.set(Some(container));
        {
            let mut tree = self.ui.tree_mut();
            tree.add_class(container, "select-container");
            tree.append_child(container, self.select_element);
        }
        self.layout();
    }
}

impl SelectBoxList {
    // ---- Medidas e aparência do `<select>` ----

    fn metrics(&self) -> SelectMetrics {
        let tree = self.ui.tree();
        let mut ancestor = tree.parent(self.select_element);
        while let Some(node) = ancestor {
            if tree.has_class(node, "action-item") {
                return ACTION_ITEM_METRICS;
            }
            ancestor = tree.parent(node);
        }
        PLAIN_METRICS
    }

    fn font() -> je_text::TextStyle {
        ui_font(SELECT_ELEMENT_FONT_SIZE, 400)
    }

    /// O tamanho natural do `<select>` (largura e altura, em px): a maior opção mais o preenchimento e a
    /// borda, no mínimo `min-width` e `min-height`.
    pub fn natural_size(&self) -> (f64, f64) {
        let metrics = self.metrics();
        let font = Self::font();
        let height = (natural_line_height(&font) + metrics.padding[0] + metrics.padding[2] + 2.0).max(metrics.min_height);
        let widest = self.options.borrow().iter().map(|o| measure_text(&o.text, &font)).fold(0.0_f64, f64::max);
        ((widest + metrics.padding[1] + metrics.padding[3] + 2.0).max(metrics.min_width), height)
    }

    /// Refaz o que depende da largura da caixa (texto com reticências, chevron, altura): chamar depois que o
    /// contêiner mudar de largura.
    pub fn layout(&self) {
        if self.container.get().is_none() {
            return;
        }
        let metrics = self.metrics();
        let font = Self::font();
        let (_, height) = self.natural_size();
        let mut tree = self.ui.tree_mut();
        tree.set_fill(self.select_element, true, false);
        tree.set_bounds(self.select_element, Rect::new(0.0, 0.0, 0.0, height));
        let width = tree.bounds(self.select_element).width();
        let text = self.selected_text().unwrap_or_default();
        let left = 1.0 + metrics.padding[3];
        let available = (width - 2.0 - metrics.padding[3] - metrics.padding[1]).max(0.0);
        // O `<select>` corta o texto na caixa de conteúdo, sem reticências.
        let shown = clip_text(&text, &font, available);
        let color = self.styles.select_foreground.unwrap_or(FALLBACK_FOREGROUND);
        let text_width = set_node_text(&mut tree, self.select_text, &shown, &font, color, height);
        tree.set_bounds(self.select_text, Rect::new(left, 0.0, left + text_width, height));
        let top = (height - CHEVRON_SIZE) / 2.0;
        tree.set_bounds(self.chevron, Rect::new(0.0, top, CHEVRON_SIZE, top + CHEVRON_SIZE));
        tree.set_right(self.chevron, Some(4.0));
        tree.style_mut(self.chevron).content = Content::Codicon { id: "chevron-down".to_owned(), size: CHEVRON_SIZE as f32, color };
        drop(tree);
        self.refresh_select_style();
    }

    /// `styleSelectElement()` mais as regras do `style.css`: `select { border: 1px solid }`, o contorno de foco
    /// (por dentro, sobre a borda) e o `cursor`.
    fn refresh_select_style(&self) {
        let styles = &self.styles;
        let focused = self.ui.focused() == Some(self.select_element);
        let foreground = styles.select_foreground.unwrap_or(FALLBACK_FOREGROUND);
        let border_color = if focused { styles.focus_border.or(styles.select_border) } else { styles.select_border }.unwrap_or(foreground);
        let mut tree = self.ui.tree_mut();
        let style = tree.style_mut(self.select_element);
        style.background = styles.select_background;
        style.border = Some(Border { width: 1.0, color: border_color });
        style.radius = CORNER_RADIUS_SMALL;
        style.cursor = Some(Cursor::Pointer);
        // `select:disabled` da folha UA do Chromium (NÃO MEDIDO no oráculo).
        style.opacity = if self.enabled.get() { 1.0 } else { 0.7 };
    }

    // ---- A lista suspensa ----

    fn list(&self) -> Option<Rc<List<ISelectOptionItem>>> {
        self.select_list.borrow().clone()
    }

    /// A cor de fundo da caixa, da lista e do painel (`styleList`).
    fn list_background(&self) -> Option<Rgba> {
        self.styles.select_list_background.or(self.styles.select_background)
    }

    /// A borda da caixa: só quando `selectBorder` existe e difere de `selectBackground` (`initStyleSheet`).
    fn popup_border_color(&self) -> Option<Rgba> {
        match (self.styles.select_background, self.styles.select_border) {
            (Some(background), Some(border)) if border != background => Some(border),
            _ => None,
        }
    }

    /// A cor do traço de cima ou de baixo do painel de detalhes.
    fn pane_border_color(&self) -> Option<Rgba> {
        self.popup_border_color().and(self.styles.select_border).or(self.styles.select_list_border)
    }

    fn style_popup(&self) {
        let background = self.list_background();
        let border = self.popup_border_color();
        let mut tree = self.ui.tree_mut();
        let style = tree.style_mut(self.select_drop_down_container);
        style.background = background;
        style.border = border.map(|color| Border { width: 1.0, color });
        style.radius = CORNER_RADIUS_LARGE;
        style.clip = true;
        tree.style_mut(self.selection_details_pane).background = background;
        if let Some(list_container) = self.select_drop_down_list_container.get() {
            let style = tree.style_mut(list_container);
            style.background = background;
            style.clip = true;
        }
    }

    /// Escreve o tamanho e a posição dos nós da caixa a partir de `popup_width`, `list_height` e da altura do
    /// painel de detalhes, e dimensiona o nó pai (a visão de contexto), que no original o `display: flex` faz.
    fn apply_geometry(&self) {
        let width = self.popup_width.get();
        let list_height = self.list_height.get();
        let border = if self.popup_border_color().is_some() { 1.0 } else { 0.0 };
        let has_details = self.has_details.get();
        let details_height = if has_details { self.details_height.get() } else { 0.0 };
        let total_height = if has_details { list_height + details_height + 2.0 * border } else { list_height };
        // `height: <listHeight>px` com `box-sizing: border-box`: a borda recorta o excesso da lista.
        let drawn = if has_details { list_height } else { (list_height - 2.0 * border).max(0.0) };
        self.list_height_drawn.set(drawn);
        let inner_width = (width - 2.0 * border).max(0.0);
        self.row_width.set(inner_width);

        let below = self.drop_down_position.get() == AnchorPosition::Below;
        let (list_top, details_top) = if below { (border, border + drawn) } else { (border + details_height, border) };
        {
            let mut tree = self.ui.tree_mut();
            let popup = self.select_drop_down_container;
            tree.set_bounds(popup, Rect::new(0.0, 0.0, width, total_height));
            if let Some(list_container) = self.select_drop_down_list_container.get() {
                tree.set_bounds(list_container, Rect::new(border, list_top, border + inner_width, list_top + drawn));
            }
            tree.set_bounds(self.selection_details_pane, Rect::new(border, details_top, border + inner_width, details_top + details_height));
            if let Some(parent) = tree.parent(popup) {
                let rect = tree.bounds(parent);
                tree.set_bounds(parent, Rect::new(rect.x0, rect.y0, rect.x0 + width, rect.y0 + total_height));
            }
        }
        self.renderer.refresh_all();
    }

    fn hide_popup_node(&self) {
        let mut tree = self.ui.tree_mut();
        tree.remove_class(self.select_drop_down_container, "visible");
        tree.style_mut(self.select_drop_down_container).visible = false;
    }

    /// `setOptionsList`: espelha as opções na lista (sem as desabilitadas com `hideDisabledOptions`).
    fn set_options_list(&self) {
        let hide_disabled = self.select_box_options.borrow().hide_disabled_options;
        let indexes: Vec<usize> = self.options.borrow().iter().enumerate().filter(|(_, o)| !hide_disabled || !o.is_disabled).map(|(i, _)| i).collect();
        let items: Vec<ISelectOptionItem> = {
            let options = self.options.borrow();
            indexes.iter().map(|i| options[*i].clone()).collect()
        };
        *self.list_option_indexes.borrow_mut() = indexes;
        if let Some(list) = self.list() {
            list.splice(0, list.length(), &items);
        }
    }

    fn get_list_index(&self, option_index: usize) -> Option<usize> {
        self.list_option_indexes.borrow().iter().position(|i| *i == option_index)
    }

    fn get_option_index(&self, list_index: usize) -> Option<usize> {
        self.list_option_indexes.borrow().get(list_index).copied()
    }

    fn focus_option(&self, option_index: usize) {
        let (Some(list), Some(list_index)) = (self.list(), self.get_list_index(option_index)) else { return };
        list.reveal(list_index, None, 0.0);
        list.set_focus(vec![list_index], None);
    }

    fn show_select_drop_down(&self) {
        clear_node(&mut self.ui.tree_mut(), self.selection_details_pane);

        if self.is_visible.get() {
            return;
        }

        // Lazily create and populate list only at open, moved from constructor
        self.create_select_list(self.select_drop_down_container);
        self.set_options_list();
        self.current_selection.set(self.selected.get());

        // This allows us to flip the position based on measurement
        // Set drop-down position above/below from required height and margins
        // If pre-layout cannot fit at least one option do not show drop-down
        self.show_context_view(true);

        // Hide so we can relay out
        self.is_visible.set(true);
        self.hide_select_drop_down(false);

        self.show_context_view(false);

        self.is_visible.set(true);
        self.aria_expanded.set(Some(true));
        let focused_list_index = self.list().and_then(|list| list.get_focus().first().copied());
        if focused_list_index.is_some() {
            self.show_option_description_hover(focused_list_index);
        }
    }

    fn show_context_view(&self, pre_layout: bool) {
        let delegate = Rc::new(DropDownDelegate { owner: self.weak_self.clone(), position: self.drop_down_position.get(), pre_layout });
        let container = if self.select_box_options.borrow().options_as_children { self.container.get() } else { None };
        self.context_view_provider.show_context_view(delegate, container);
    }

    fn hide_select_drop_down(&self, focus_select: bool) {
        if !self.is_visible.get() {
            return;
        }

        self.is_visible.set(false);
        self.option_description_hover.clear();
        self.aria_expanded.set(Some(false));

        if focus_select {
            self.ui.focus(self.select_element);
        }

        self.context_view_provider.hide_context_view();
    }

    fn cancel_select_drop_down(&self, focus_select: bool) {
        self.select(self.current_selection.get());
        self.hide_select_drop_down(focus_select);
    }

    fn render_select_drop_down(&self, container: NodeId, pre_layout_position: bool) -> DisposableRef {
        self.ui.tree_mut().append_child(container, self.select_drop_down_container);

        // Pre-Layout allows us to change position
        self.layout_select_drop_down(pre_layout_position);

        let ui = self.ui.clone();
        let popup = self.select_drop_down_container;
        to_disposable(move || {
            // contextView will dispose itself if moving from one View to another
            let mut tree = ui.tree_mut();
            if tree.is_alive(popup) {
                tree.detach(popup); // remove to take out the CSS rules we add
            }
        })
    }

    /// `measureMaxDetailsHeight`: a maior altura do painel entre as descrições de todas as opções.
    fn measure_max_details_height(&self) -> f64 {
        let mut max_details_pane_height = 0.0_f64;
        for index in 0..self.options.borrow().len() {
            self.update_detail(Some(index));
            max_details_pane_height = max_details_pane_height.max(self.details_height.get());
        }
        max_details_pane_height
    }

    /// `layoutSelectDropDown(preLayoutPosition?)`.
    fn layout_select_drop_down(&self, pre_layout_position: bool) -> bool {
        // Avoid recursion from layout called in onListFocus
        if self.skip_layout.get() {
            return false;
        }

        // Layout ContextView drop down select list and container
        // Have to manage our vertical overflow, sizing, position below or above
        // Position has to be determined and set prior to contextView instantiation
        let Some(list) = self.list() else { return false };

        // Make visible to enable measurements
        {
            let mut tree = self.ui.tree_mut();
            tree.add_class(self.select_drop_down_container, "visible");
            tree.style_mut(self.select_drop_down_container).visible = true;
        }

        let (_, window_height) = window_size(&self.ui);
        let (select_top, select_height, select_width) = {
            let tree = self.ui.tree();
            let page = tree.absolute_bounds(self.select_element);
            (page.y0, page.height(), tree.bounds(self.select_element).width())
        };
        let (min_bottom_margin, max_visible_options, anchor_position) = {
            let options = self.select_box_options.borrow();
            (options.min_bottom_margin.unwrap_or(0.0), options.max_visible_options, options.anchor_position)
        };
        let max_select_drop_down_height_below = window_height - select_top - select_height - min_bottom_margin;
        let max_select_drop_down_height_above = select_top - DEFAULT_DROPDOWN_MINIMUM_TOP_MARGIN;

        // Determine optimal width - min(longest option), opt(parent select, excluding margins), max(ContextView controlled)
        let select_min_width = self.width_control_width();
        let select_optimal_width = select_min_width.max(js_round(select_width));
        self.popup_width.set(select_optimal_width);

        // Get initial list height and determine space above and below
        let mut list_height = list.content_height().min(max_visible_options.map_or(f64::INFINITY, |m| m as f64 * SELECT_OPTION_HEIGHT));

        if self.has_details.get() && self.cached_max_details_height.get().is_none() {
            self.cached_max_details_height.set(Some(self.measure_max_details_height()));
        }
        let max_details_pane_height = if self.has_details.get() { self.cached_max_details_height.get().unwrap_or(0.0) } else { 0.0 };

        let min_required_drop_down_height = list_height + max_details_pane_height;
        let max_visible_options_below = ((max_select_drop_down_height_below - max_details_pane_height) / SELECT_OPTION_HEIGHT).floor() as i64;
        let max_visible_options_above = ((max_select_drop_down_height_above - max_details_pane_height) / SELECT_OPTION_HEIGHT).floor() as i64;

        let out_of_viewport = (select_top + select_height) > (window_height - 22.0) || select_top < DEFAULT_DROPDOWN_MINIMUM_TOP_MARGIN;

        // If we are only doing pre-layout check/adjust position only
        // Calculate vertical space available, flip up if insufficient
        // Use reflected padding on parent select, ContextView style
        // properties not available before DOM attachment
        if pre_layout_position {
            // Check if select moved out of viewport , do not open
            // If at least one option cannot be shown, don't open the drop-down or hide/remove if open
            if out_of_viewport || (max_visible_options_below < 1 && max_visible_options_above < 1) {
                // Indicate we cannot open
                return false;
            }

            // Fall back to the other side when the preferred side cannot show enough options.
            // Always show complete list items - never more than Max available vertical height
            let prefer_above = anchor_position == Some(AnchorPosition::Above);
            let (preferred_visible_options, other_visible_options) = if prefer_above { (max_visible_options_above, max_visible_options_below) } else { (max_visible_options_below, max_visible_options_above) };
            let shown = (list.length() as f64).min(max_visible_options.map_or(f64::INFINITY, |m| m as f64));
            let flip = preferred_visible_options < DEFAULT_MINIMUM_VISIBLE_OPTIONS && other_visible_options > preferred_visible_options && shown > preferred_visible_options as f64;
            let above = if prefer_above { !flip } else { flip };
            self.drop_down_position.set(if above { AnchorPosition::Above } else { AnchorPosition::Below });
            self.reorder_popup_children(above);
            // Do full layout on showSelectDropDown only
            return true;
        }

        // Check if select out of viewport or cutting into status bar
        let position = self.drop_down_position.get();
        if out_of_viewport || (position == AnchorPosition::Below && max_visible_options_below < 1) || (position == AnchorPosition::Above && max_visible_options_above < 1) {
            // Cannot properly layout, close and hide
            self.hide_select_drop_down(true);
            return false;
        }

        // SetUp list dimensions and layout - account for container padding
        // Use position to check above or below available space
        if position == AnchorPosition::Below {
            if self.is_visible.get() && max_visible_options_below + max_visible_options_above < 1 {
                // If drop-down is visible, must be doing a DOM re-layout, hide since we don't fit
                // Hide drop-down, hide contextview, focus on parent select
                self.hide_select_drop_down(true);
                return false;
            }

            // Adjust list height to max from select bottom to margin (default/minBottomMargin)
            if min_required_drop_down_height > max_select_drop_down_height_below {
                list_height = max_visible_options_below as f64 * SELECT_OPTION_HEIGHT;
            }
        } else if min_required_drop_down_height > max_select_drop_down_height_above {
            list_height = max_visible_options_above as f64 * SELECT_OPTION_HEIGHT;
        }

        // Set adjusted list height and relayout
        self.list_height.set(list_height);
        self.apply_geometry();
        list.layout(Some(self.list_height_drawn.get()), None);
        list.dom_focus();

        // Finally set focus on selected item
        if list.length() > 0 {
            let selected_list_index = match self.get_list_index(self.selected.get()) {
                Some(index) => index,
                None => {
                    if let Some(option_index) = self.get_option_index(0) {
                        self.selected.set(option_index);
                        self.select(option_index);
                    }
                    0
                }
            };
            list.reveal(selected_list_index, None, 0.0);
            list.set_focus(vec![selected_list_index], None);
        }

        if self.has_details.get() {
            // Leave the selectDropDownContainer to size itself according to children (list + details) - #57447
            self.update_detail(Some(self.selected.get()));
        } else {
            self.details_height.set(0.0);
            self.ui.tree_mut().style_mut(self.selection_details_pane).visible = false;
            self.apply_geometry();
        }

        true
    }

    /// Ordem dos filhos e traço do painel de detalhes: abaixo, lista e depois painel (traço em cima); acima, o
    /// contrário (traço embaixo).
    fn reorder_popup_children(&self, above: bool) {
        let mut tree = self.ui.tree_mut();
        let (popup, pane) = (self.select_drop_down_container, self.selection_details_pane);
        let Some(list_container) = self.select_drop_down_list_container.get() else { return };
        tree.detach(list_container);
        tree.detach(pane);
        let (first, second) = if above { (pane, list_container) } else { (list_container, pane) };
        tree.append_child(popup, first);
        tree.append_child(popup, second);
        tree.remove_class(pane, if above { "border-top" } else { "border-bottom" });
        tree.add_class(pane, if above { "border-bottom" } else { "border-top" });
    }

    /// `setWidthControlElement`: a largura da maior opção (texto mais `decoratorRight`) com os 8px de
    /// preenchimento de cada lado de `.option-text-width-control`.
    fn width_control_width(&self) -> f64 {
        let options = self.options.borrow();
        let mut longest = 0;
        let mut longest_length = 0;
        for (index, option) in options.iter().enumerate() {
            let length = utf16_len(&option.text) + option.detail.as_deref().map_or(0, utf16_len) + option.decorator_right.as_deref().map_or(0, utf16_len);
            if length > longest_length {
                longest = index;
                longest_length = length;
            }
        }
        let Some(option) = options.get(longest) else { return 0.0 };
        let text = match option.decorator_right.as_deref().filter(|d| !d.is_empty()) {
            Some(decorator) => format!("{}{decorator} ", option.text),
            None => option.text.clone(),
        };
        measure_text(&text, &Self::font()) + 16.0
    }

    /// `createSelectList(parent)`.
    fn create_select_list(&self, parent: NodeId) {
        // If we have already constructive list on open, skip
        if self.select_list.borrow().is_some() {
            return;
        }

        // SetUp container for list
        let list_container = create_el(&mut self.ui.tree_mut(), parent, Role::Generic, "select-box-dropdown-list-container");
        self.select_drop_down_list_container.set(Some(list_container));

        let list = List::new(
            "SelectBoxCustom",
            &self.ui,
            list_container,
            Rc::new(OptionHeights),
            vec![self.renderer.clone()],
            ListOptions { view: ListViewOptions { use_shadows: false, vertical_scroll_mode: ScrollbarVisibility::Visible, mouse_support: false, ..ListViewOptions::default() }, keyboard_support: Some(false), ..ListOptions::default() },
        );
        self.base.register(list.clone());
        let list_node = list.get_html_element();
        {
            let mut tree = self.ui.tree_mut();
            tree.set_role(list_node, Role::ListBox);
            let label = self.select_box_options.borrow().aria_label.clone().unwrap_or_default();
            tree.set_label(list_node, label);
        }
        *self.select_list.borrow_mut() = Some(list.clone());

        // SetUp list keyboard controller - control navigation, disabled items, focus
        self.listen(list_container, EventKind::KeyDown, false, |this, e| this.on_drop_down_key_down(e));

        // SetUp list mouse controller - control navigation, disabled items, focus
        self.listen(list_node, EventKind::PointerUp, false, |this, e| this.on_pointer_up(e));

        let weak = self.weak_self.clone();
        self.base.register(list.view.on_mouse_move().listen(move |e| {
            let (Some(this), Some(index)) = (weak.upgrade(), e.index) else { return };
            let enabled = e.element.as_ref().is_some_and(|o| !o.is_disabled);
            if let Some(list) = this.list()
                && enabled
                && list.get_focus() != [index]
            {
                list.set_focus(vec![index], None);
            }
        }));
        let weak = self.weak_self.clone();
        self.base.register(list.on_did_change_focus().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.renderer.refresh_all();
                this.on_list_focus(e);
            }
        }));

        self.listen(self.select_drop_down_container, EventKind::Blur, true, |this, _| {
            if this.is_visible.get() {
                this.on_list_blur();
            }
        });

        self.style_popup();
    }

    // ---- Mouse e foco da lista ----

    /// List mouse controller - active exit, select option, fire onDidSelect if change, return focus to parent select
    fn on_pointer_up(&self, e: &mut Event) {
        let Some(list) = self.list() else { return };
        if list.length() == 0 {
            return;
        }

        e.stop(false);

        let target = e.target;
        // Check our mouse event is on an option (not scrollbar)
        let row = {
            let tree = self.ui.tree();
            if tree.has_class(target, "slider") {
                return;
            }
            let mut node = Some(target);
            while let Some(n) = node.filter(|n| !tree.has_class(*n, "monaco-list-row")) {
                node = tree.parent(n);
            }
            let Some(row) = node else { return };
            (row, tree.has_class(row, "option-disabled"))
        };
        let (row, disabled) = row;
        let Some(index) = list.view.index_of_row_node(row).and_then(|i| self.get_option_index(i)) else { return };

        // Ignore mouse selection of disabled options
        if index < self.options.borrow().len() && !disabled {
            self.selected.set(index);
            self.select(index);

            self.focus_option(index);

            // Only fire if selection change
            if index != self.current_selection.get() {
                // Set current = selected
                self.current_selection.set(index);
                let text = self.options.borrow()[index].text.clone();
                self.on_did_select.fire(ISelectData { index: self.selected_index.get().max(0) as usize, selected: text.clone() });
                if !text.is_empty() {
                    self.set_title(&text);
                }
            }

            self.hide_select_drop_down(true);
        }
    }

    /// List Exit - passive - implicit no selection change, hide drop-down
    fn on_list_blur(&self) {
        self.cancel_select_drop_down(false);
    }

    /// List Focus Change - passive - update details pane with newly focused element's data
    fn on_list_focus(&self, e: &ListEvent<ISelectOptionItem>) {
        // Skip during initial layout
        if !self.is_visible.get() {
            return;
        }

        let list_index = e.indexes.first().copied();
        let option_index = list_index.and_then(|i| self.get_option_index(i));
        if self.has_details.get() {
            self.update_detail(option_index);
        }
        self.show_option_description_hover(list_index);
    }

    fn show_option_description_hover(&self, list_index: Option<usize>) {
        if !self.select_box_options.borrow().show_option_description_hovers {
            return;
        }
        let option = list_index.and_then(|i| self.get_option_index(i)).and_then(|i| self.options.borrow().get(i).cloned());
        let description = option.and_then(|o| o.description).filter(|d| !d.is_empty());
        let target = list_index.and_then(|i| self.renderer.get_element(i));
        let hover = match (description, target) {
            (Some(description), Some(target)) => {
                let mut options = IHoverOptions::new(HoverContent::Text(description), HoverTarget::Node(target));
                options.position = Some(IHoverPositionOptions { hover_position: Some(HoverPosition::Right), ..IHoverPositionOptions::default() });
                get_base_layer_hover_delegate().show_delayed_hover(options, HoverTarget::Node(target), IHoverLifecycleOptions { group_id: Some("select-box-option-description".to_owned()), ..IHoverLifecycleOptions::default() })
            }
            _ => None,
        };
        self.option_description_hover.set(hover);
    }

    /// `updateDetail(selectedIndex)`: o painel de detalhes mostra a descrição da opção (some se não há).
    fn update_detail(&self, option_index: Option<usize>) {
        let description = option_index.and_then(|i| self.options.borrow().get(i).and_then(|o| o.description.clone())).unwrap_or_default();
        {
            let mut tree = self.ui.tree_mut();
            let pane = self.selection_details_pane;
            // Reset
            clear_node(&mut tree, pane);
            if description.is_empty() {
                tree.style_mut(pane).visible = false;
                self.details_height.set(0.0);
            } else {
                // TODO(integração): `descriptionIsMarkdown` precisa do `renderMarkdown`; por ora é texto.
                let font = Self::font();
                let line_height = natural_line_height(&font);
                let border_width = if self.popup_border_color().is_some() { 2.0 } else { 0.0 };
                let lines = wrap_text(&description, &font, (self.popup_width.get() - border_width - 12.0).max(0.0));
                let border = self.pane_border_color();
                let border_height = if border.is_some() { 1.0 } else { 0.0 };
                let height = border_height + 10.0 + lines.len() as f64 * line_height;
                let color = self.styles.select_foreground.unwrap_or(FALLBACK_FOREGROUND);
                let below = self.drop_down_position.get() == AnchorPosition::Below;
                let text_top = 5.0 + if below { border_height } else { 0.0 };
                for (i, line) in lines.iter().enumerate() {
                    let node = create_el(&mut tree, pane, Role::Generic, "select-box-details-line");
                    let width = set_node_text(&mut tree, node, line, &font, color, line_height);
                    let top = text_top + i as f64 * line_height;
                    tree.set_bounds(node, Rect::new(6.0, top, 6.0 + width, top + line_height));
                }
                if let Some(color) = border {
                    let node = create_el(&mut tree, pane, Role::Generic, "select-box-details-border");
                    tree.set_fill(node, true, false);
                    let top = if below { 0.0 } else { height - 1.0 };
                    tree.set_bounds(node, Rect::new(0.0, top, 0.0, top + 1.0));
                    tree.style_mut(node).background = Some(color);
                }
                tree.style_mut(pane).visible = true;
                self.details_height.set(height);
            }
        }

        // Avoid recursion
        self.skip_layout.set(true);
        self.apply_geometry();
        self.context_view_provider.layout();
        self.skip_layout.set(false);
    }

    // ---- Teclado da lista ----

    fn on_drop_down_key_down(&self, e: &mut Event) {
        let Some(list) = self.list() else { return };
        if list.length() == 0 {
            return;
        }
        let Some(key) = StandardKeyboardEvent::from_event(e) else { return };
        let code = key.key_code;
        if code == KeyCode::Enter || code == KeyCode::Tab {
            // Tab should behave the same as enter, #79339
            self.on_enter(e);
        } else if code == KeyCode::Escape {
            self.on_escape(e);
        } else if code == KeyCode::UpArrow {
            self.on_up_arrow(e);
        } else if code == KeyCode::DownArrow {
            self.on_down_arrow(e);
        } else if code == KeyCode::PageDown {
            self.on_page(e, true);
        } else if code == KeyCode::PageUp {
            self.on_page(e, false);
        } else if code == KeyCode::Home {
            self.on_home_end(e, false);
        } else if code == KeyCode::End {
            self.on_home_end(e, true);
        } else if (code >= KeyCode::Digit0 && code <= KeyCode::KeyZ) || (code >= KeyCode::Semicolon && code <= KeyCode::NumpadDivide) {
            self.on_character(e, code);
        }
    }

    fn is_option_disabled(&self, option_index: usize) -> bool {
        self.options.borrow().get(option_index).is_some_and(|o| o.is_disabled)
    }

    /// List exit - active - hide ContextView dropdown, reset selection, return focus to parent select
    fn on_escape(&self, e: &mut Event) {
        e.stop(false);

        // Reset selection to value when opened
        self.cancel_select_drop_down(true);
    }

    /// List exit - active - hide ContextView dropdown, return focus to parent select, fire onDidSelect if change
    fn on_enter(&self, e: &mut Event) {
        e.stop(false);

        // Ignore if current selection is disabled (e.g. separator)
        if self.is_option_disabled(self.selected.get()) {
            self.hide_select_drop_down(true);
            return;
        }

        // Only fire if selection change
        if self.selected.get() != self.current_selection.get() {
            self.current_selection.set(self.selected.get());
            let text = self.options.borrow()[self.selected.get()].text.clone();
            self.on_did_select.fire(ISelectData { index: self.selected_index.get().max(0) as usize, selected: text.clone() });
            if !text.is_empty() {
                self.set_title(&text);
            }
        }

        self.hide_select_drop_down(true);
    }

    /// List navigation - have to handle disabled options (jump over)
    fn on_down_arrow(&self, e: &mut Event) {
        let len = self.options.borrow().len();
        if self.selected.get() + 1 < len {
            e.stop(true);

            // Skip over all contiguous disabled options
            let mut next = self.selected.get() + 1;
            while next < len && self.is_option_disabled(next) {
                next += 1;
            }

            if next >= len {
                return;
            }

            // Set focus/selection - only fire event when closing drop-down or on blur
            self.select(next);
            self.focus_option(next);
        }
    }

    fn on_up_arrow(&self, e: &mut Event) {
        if self.selected.get() > 0 {
            e.stop(true);

            // Skip over all contiguous disabled options
            let mut prev = self.selected.get() as i64 - 1;
            while prev >= 0 && self.is_option_disabled(prev as usize) {
                prev -= 1;
            }

            if prev < 0 {
                return;
            }

            // Set focus/selection - only fire event when closing drop-down or on blur
            self.select(prev as usize);
            self.focus_option(prev as usize);
        }
    }

    /// `onPageUp`/`onPageDown`: move uma página e, quando a rolagem assenta, pula opções desabilitadas.
    fn on_page(&self, e: &mut Event, down: bool) {
        e.stop(false);
        let Some(list) = self.list() else { return };

        if down {
            list.focus_next_page(None, None);
        } else {
            list.focus_previous_page(None, None, Rc::new(|| 0.0));
        }

        // Allow scrolling to settle
        let weak = self.weak_self.clone();
        runtime::set_timeout(
            move || {
                let Some(this) = weak.upgrade() else { return };
                let Some(list) = this.list() else { return };
                let Some(mut candidate) = list.get_focus().first().copied() else { return };

                // Shift selection if we land on a disabled option
                let disabled = |this: &SelectBoxList, candidate: usize| this.get_option_index(candidate).is_some_and(|o| this.is_option_disabled(o));
                if down {
                    while candidate + 1 < list.length() && disabled(&this, candidate) {
                        candidate += 1;
                    }
                } else {
                    while candidate > 0 && disabled(&this, candidate) {
                        candidate -= 1;
                    }
                }
                let Some(option_index) = this.get_option_index(candidate) else { return };
                if this.is_option_disabled(option_index) {
                    return;
                }
                this.selected.set(option_index);
                list.reveal(candidate, None, 0.0);
                list.set_focus(vec![candidate], None);
                this.select(option_index);
            },
            1,
        );
    }

    /// `onHome`/`onEnd`.
    fn on_home_end(&self, e: &mut Event, end: bool) {
        e.stop(false);

        let len = self.options.borrow().len();
        if len < 2 {
            return;
        }
        let mut candidate = if end { len - 1 } else { 0 };
        if end {
            while candidate > 0 && self.is_option_disabled(candidate) {
                candidate -= 1;
            }
        } else {
            while candidate < len - 1 && self.is_option_disabled(candidate) {
                candidate += 1;
            }
        }
        if self.is_option_disabled(candidate) {
            return;
        }
        self.selected.set(candidate);
        self.focus_option(candidate);
        self.select(candidate);
    }

    /// Mimic option first character navigation of native select
    fn on_character(&self, e: &mut Event, code: KeyCode) {
        let Some(ch) = KeyCodeUtils::to_string(code) else { return };
        let len = self.options.borrow().len();
        for i in 0..len.saturating_sub(1) {
            let option_index = (i + self.selected.get() + 1) % len;
            let matches = {
                let options = self.options.borrow();
                let option = &options[option_index];
                !option.is_disabled && option.text.chars().next().is_some_and(|c| c.to_uppercase().to_string() == ch)
            };
            if matches {
                self.select(option_index);
                self.focus_option(option_index);
                e.stop(false);
                break;
            }
        }
    }
}

/// `this._register(new MutableDisposable<T>())`.
fn registered_mutable<T: IDisposable + ?Sized + 'static>(base: &Rc<Disposable>) -> Rc<MutableDisposable<T>> {
    let value = MutableDisposable::<T>::new();
    base.register(value.clone());
    value
}

impl IDisposable for SelectBoxList {
    fn dispose(&self) {
        self.hide_select_drop_down(false);
        self.base.dispose();
        let mut tree = self.ui.tree_mut();
        if tree.is_alive(self.select_drop_down_container) {
            tree.remove(self.select_drop_down_container);
        }
    }
}
