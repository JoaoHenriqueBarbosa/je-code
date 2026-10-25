//! Porte de `src/vs/editor/contrib/peekView/browser/peekView.ts`: o `PeekViewWidget` (o `ZoneWidget` com cabeçalho de
//! título e ações e um corpo), o `PeekContext` (as chaves de contexto que dizem se o editor atual vive dentro de um peek),
//! o `PeekContextController`, a `IPeekViewService` e os identificadores das cores do peek.
//!
//! Representação:
//! - `abstract class PeekViewWidget extends ZoneWidget` é a struct [`PeekViewWidget`], que contém o [`ZoneWidget`] (acessado
//!   por [`PeekViewWidget::zone`]: `show`, `hide`, `position`, `updatePositionAndHeight`, `hasFocus` e os demais membros
//!   públicos herdados ficam lá), mais o trait [`PeekViewHooks`] com os membros que as subclasses sobrescrevem (`_fillHead`,
//!   `_fillBody` abstrato, `_fillTitleIcon`, `_getActionBarOptions`, `_onTitleClick`, `_doLayoutHead`, `_doLayoutBody` e os
//!   membros do `ZoneWidget`: `_applyStyles`, `_onWidth`, `_getWidth`, `_getMaximumHeightInLines`, `revealRange` e
//!   `_getResizeBounds`). A subclasse é dona do `Rc<PeekViewWidget>` e se registra com [`PeekViewWidget::set_hooks`]; o
//!   corpo padrão de cada gancho é o da base (`fill_head_base`, `apply_styles_base`...), que é o `super.x()` do original. O
//!   próprio `PeekViewWidget` é o gancho do `ZoneWidget` ([`ZoneWidgetHooks`]) e repassa à subclasse.
//! - **`dispose` virtual.** `this.dispose()` do original (a ação `peekview.close`, o `_doLayout` de uma zona dobrada e o
//!   `addExclusiveWidget`) despacha para a subclasse, então [`PeekViewHooks`] exige [`IDisposable`] e o widget chama o da
//!   subclasse ([`PeekViewWidget::dispose_widget`]). O `dispose` da subclasse chama o `IDisposable::dispose` do
//!   `PeekViewWidget`, que é o `super.dispose()` (e não chama o gancho, para não recursar).
//! - **DOM vira a árvore de `je-ui`.** `_headElement`, `_titleElement`, `_primaryHeading`, `_secondaryHeading`,
//!   `_metaHeading`, `_bodyElement` e o contêiner das ações são nós da árvore. As classes (`peekview-widget`, `head`, `body`,
//!   `peekview-title`, `clickable`, `filename`, `dirname`, `meta`, `peekview-actions`) são as classes dos nós. O `innerText` de
//!   um título é o rótulo do nó ([`je_ui::Tree::set_label`]) e o atributo `title` é o atributo do nó; esconder o `meta` é
//!   `display: none` (`visible`). O `style.backgroundColor` do cabeçalho é o fundo do nó, e a borda do corpo (`border-top:
//!   1px solid` na cor da moldura) é a borda do nó. O `style.color` dos dois títulos não tem campo em `NodeStyle`: o pintor
//!   lê as cores em [`PeekViewWidget::options`]. `height` e `lineHeight` do cabeçalho são os limites do nó (a altura da
//!   linha do texto é a altura do nó). O `flex` do cabeçalho (título à esquerda com 20px de margem e ações à direita com 2px
//!   de recuo) é o cálculo de [`PeekViewWidget::do_layout_head_base`].
//! - **`ActionBar`.** É o de `je-widgets`. `createActionViewItem.bind(undefined, instantiationService)` (o
//!   `actionViewItemProvider` padrão, que cria os itens das ações de menu) vive no workbench, de que o `je-editor` não
//!   depende: a base não instala provedor (cada ação usa o item padrão) e uma subclasse com ações de menu dá o provedor
//!   dela em [`PeekViewHooks::get_action_bar_options`].
//! - `onDidClose` carrega o próprio widget no original (`Emitter<PeekViewWidget>`); aqui o evento é `Event<()>`, porque
//!   quem escuta já tem o widget em mãos.
//! - `observableCodeEditor(this.editor).openedPeekWidgets` é o observável de `ObservableCodeEditor`, contado na criação e
//!   no `dispose`.
//! - **Cores.** Cada `registerColor(...)` já está em `je_theme::registered_colors`; este módulo guarda os identificadores
//!   ([`PEEK_VIEW_BORDER`] e companhia), para quem pinta ou estiliza pedir a cor ao tema.
//! - **Registro.** `registerSingleton(IPeekViewService, ..., Delayed)` e `registerEditorContribution(PeekContextController.ID,
//!   ..., Eager)` do carregamento do módulo são [`register`], idempotente por thread. A contribuição precisa do
//!   `IContextKeyService` do editor, que o dono do registro entrega por função.
//!
//! As chaves de contexto são `thread_local` e as funções devolvem cópias (o mesmo padrão de `editor_context_keys`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::observable::{Observable, SettableExt};
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_id, service_impl};
use je_platform::contextkey::{ContextKeyExpression, IContextKey, IContextKeyService, RawContextKey};
use je_render::kurbo::Rect;
use je_theme::color::Color;
use je_theme::to_rgba8;
use je_ui::{Border, Event as UiEvent, EventKind, NodeId, Role};
use je_widgets::action_bar::{ActionBar, ActionsOrientation, IActionBarOptions, IActionOptions};
use je_widgets::action_view_items::IActionViewItemOptions;
use je_widgets::dom::{create_detached_el, create_el, hide, show};

use crate::code_editor_widget::{CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution};
use crate::config::editor_options::layout_info::EditorLayoutInfo;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::editor_extensions::register_editor_contribution;
use crate::embedded_code_editor_widget::is_embedded_code_editor;
use crate::observable_code_editor::observable_code_editor;
use crate::range::Range;
use crate::zone_widget::{ZoneResizeBounds, ZoneWidget, ZoneWidgetHooks, ZoneWidgetHost, ZoneWidgetOptions, ZoneWidgetStyles};

// #region Cores

/// `peekViewTitleBackground`.
pub const PEEK_VIEW_TITLE_BACKGROUND: &str = "peekViewTitle.background";
/// `peekViewTitleForeground`.
pub const PEEK_VIEW_TITLE_FOREGROUND: &str = "peekViewTitleLabel.foreground";
/// `peekViewTitleInfoForeground`.
pub const PEEK_VIEW_TITLE_INFO_FOREGROUND: &str = "peekViewTitleDescription.foreground";
/// `peekViewBorder`.
pub const PEEK_VIEW_BORDER: &str = "peekView.border";
/// `peekViewResultsBackground`.
pub const PEEK_VIEW_RESULTS_BACKGROUND: &str = "peekViewResult.background";
/// `peekViewResultsMatchForeground`.
pub const PEEK_VIEW_RESULTS_MATCH_FOREGROUND: &str = "peekViewResult.lineForeground";
/// `peekViewResultsFileForeground`.
pub const PEEK_VIEW_RESULTS_FILE_FOREGROUND: &str = "peekViewResult.fileForeground";
/// `peekViewResultsSelectionBackground`.
pub const PEEK_VIEW_RESULTS_SELECTION_BACKGROUND: &str = "peekViewResult.selectionBackground";
/// `peekViewResultsSelectionForeground`.
pub const PEEK_VIEW_RESULTS_SELECTION_FOREGROUND: &str = "peekViewResult.selectionForeground";
/// `peekViewEditorBackground`.
pub const PEEK_VIEW_EDITOR_BACKGROUND: &str = "peekViewEditor.background";
/// `peekViewEditorGutterBackground`.
pub const PEEK_VIEW_EDITOR_GUTTER_BACKGROUND: &str = "peekViewEditorGutter.background";
/// `peekViewEditorStickyScrollBackground`.
pub const PEEK_VIEW_EDITOR_STICKY_SCROLL_BACKGROUND: &str = "peekViewEditorStickyScroll.background";
/// `peekViewEditorStickyScrollGutterBackground`.
pub const PEEK_VIEW_EDITOR_STICKY_SCROLL_GUTTER_BACKGROUND: &str = "peekViewEditorStickyScrollGutter.background";
/// `peekViewResultsMatchHighlight`.
pub const PEEK_VIEW_RESULTS_MATCH_HIGHLIGHT: &str = "peekViewResult.matchHighlightBackground";
/// `peekViewEditorMatchHighlight`.
pub const PEEK_VIEW_EDITOR_MATCH_HIGHLIGHT: &str = "peekViewEditor.matchHighlightBackground";
/// `peekViewEditorMatchHighlightBorder`.
pub const PEEK_VIEW_EDITOR_MATCH_HIGHLIGHT_BORDER: &str = "peekViewEditor.matchHighlightBorder";

// #endregion

// #region PeekContext

thread_local! {
    static IN_PEEK_EDITOR: RawContextKey = RawContextKey::new(
        "inReferenceSearchEditor",
        true,
        localize("inReferenceSearchEditor", "Whether the current code editor is embedded inside peek", &[]),
    );
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `PeekContext.inPeekEditor`.
pub fn in_peek_editor() -> RawContextKey {
    IN_PEEK_EDITOR.with(Clone::clone)
}

/// `PeekContext.notInPeekEditor`: `inPeekEditor.toNegated()`.
pub fn not_in_peek_editor() -> ContextKeyExpression {
    in_peek_editor().to_negated()
}

/// `PeekContextController`: liga a chave `inReferenceSearchEditor` ao editor quando ele é embutido (o editor de dentro do
/// peek).
pub struct PeekContextController {
    /// A chave presa ao serviço do editor (o `bindTo` do construtor).
    _in_peek_editor: Option<Rc<dyn IContextKey>>,
}

impl PeekContextController {
    pub const ID: &'static str = "editor.contrib.referenceController";

    pub fn new(editor: &Rc<CodeEditorWidget>, context_key_service: &dyn IContextKeyService) -> Rc<Self> {
        Rc::new(Self { _in_peek_editor: is_embedded_code_editor(editor).then(|| in_peek_editor().bind_to(context_key_service)) })
    }
}

impl IDisposable for PeekContextController {
    fn dispose(&self) {}
}

impl IEditorContribution for PeekContextController {}

// #endregion

// #region IPeekViewService

/// O que a `IPeekViewService` e os consumidores precisam de um widget de peek: fechar e saber quando fechou.
pub trait IPeekViewWidget: IDisposable {
    /// `onDidClose`.
    fn on_did_close(&self) -> Event<()>;
}

/// `IPeekViewService`.
pub trait IPeekViewService {
    /// `addExclusiveWidget(editor, widget)`: um editor tem no máximo um peek; o anterior é fechado.
    fn add_exclusive_widget(&self, editor: &Rc<CodeEditorWidget>, widget: &Rc<dyn IPeekViewWidget>);
}

je_instantiation::delayed_proxy!(pub PeekViewServiceProxy for dyn IPeekViewService {
    methods {
        fn add_exclusive_widget(&self, editor: &Rc<CodeEditorWidget>, widget: &Rc<dyn IPeekViewWidget>);
    }
    events {}
});

service_id!(pub I_PEEK_VIEW_SERVICE: dyn IPeekViewService = "IPeekViewService", proxy PeekViewServiceProxy);

/// `{ widget, listener }`.
struct ExclusiveWidget {
    widget: Rc<dyn IPeekViewWidget>,
    listener: DisposableRef,
}

/// O serviço registrado por `registerSingleton(IPeekViewService, class implements IPeekViewService, Delayed)`. O mapa do
/// original é por editor (`Map<ICodeEditor, ...>`): aqui a chave é o `getId()` do editor.
pub struct PeekViewService {
    me: Weak<PeekViewService>,
    widgets: RefCell<HashMap<String, ExclusiveWidget>>,
}

impl Injectable for PeekViewService {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new_cyclic(|me| Self { me: me.clone(), widgets: RefCell::new(HashMap::new()) })
    }
}

service_impl!(PeekViewService => dyn IPeekViewService);

impl IPeekViewService for PeekViewService {
    fn add_exclusive_widget(&self, editor: &Rc<CodeEditorWidget>, widget: &Rc<dyn IPeekViewWidget>) {
        let key = editor.get_id();
        let existing = self.widgets.borrow_mut().remove(&key);
        if let Some(existing) = existing {
            existing.listener.dispose();
            existing.widget.dispose();
        }

        let (me, remove_key, remove_widget) = (self.me.clone(), key.clone(), Rc::downgrade(widget));
        let listener = widget.on_did_close().listen(move |_| {
            let Some(service) = me.upgrade() else {
                return;
            };
            // `const data = this._widgets.get(editor); if (data && data.widget === widget) { ... }`
            let removed = {
                let mut widgets = service.widgets.borrow_mut();
                let is_same = widgets.get(&remove_key).is_some_and(|data| std::ptr::addr_eq(Rc::as_ptr(&data.widget), remove_widget.as_ptr()));
                if is_same { widgets.remove(&remove_key) } else { None }
            };
            if let Some(removed) = removed {
                removed.listener.dispose();
            }
        });
        self.widgets.borrow_mut().insert(key, ExclusiveWidget { widget: Rc::clone(widget), listener });
    }
}

// #endregion

// #region PeekViewWidget

/// `IPeekViewStyles`: as cores que [`PeekViewWidget::style`] aceita (as da moldura e da seta vêm em `zone`).
#[derive(Clone, Default)]
pub struct PeekViewStyles {
    pub zone: ZoneWidgetStyles,
    pub header_background_color: Option<Color>,
    pub primary_heading_color: Option<Color>,
    pub secondary_heading_color: Option<Color>,
}

/// `IPeekViewOptions` (`IOptions & IPeekViewStyles & { supportOnTitleClick?: boolean }`). Os campos de `IOptions` estão em
/// `zone`, e o `defaultOptions` do original entra em [`PeekViewOptions::mixin_defaults`].
#[derive(Clone, Default)]
pub struct PeekViewOptions {
    pub zone: ZoneWidgetOptions,
    pub header_background_color: Option<Color>,
    pub primary_heading_color: Option<Color>,
    pub secondary_heading_color: Option<Color>,
    pub support_on_title_click: Option<bool>,
}

impl PeekViewOptions {
    /// `objects.mixin(this.options, defaultOptions, false)`.
    fn mixin_defaults(&mut self) {
        self.header_background_color.get_or_insert_with(Color::white);
        self.primary_heading_color.get_or_insert_with(|| Color::from_hex("#333333"));
        self.secondary_heading_color.get_or_insert_with(|| Color::from_hex("#6c6c6cb3"));
    }
}

/// A margem esquerda do título (`margin-left: 20px`).
const TITLE_MARGIN_LEFT: f64 = 20.0;
/// O recuo direito das ações (`padding-right: 2px`).
const ACTIONS_PADDING_RIGHT: f64 = 2.0;

/// Os membros de `PeekViewWidget` (e os de `ZoneWidget`) que as subclasses sobrescrevem (ver o topo do módulo). O corpo
/// padrão de cada gancho é o da base.
pub trait PeekViewHooks: IDisposable {
    /// `_fillBody(container)`: abstrato.
    fn fill_body(&self, container: NodeId);

    /// `_fillHead(container, noCloseAction?)`.
    fn fill_head(&self, peek: &PeekViewWidget, container: NodeId, no_close_action: bool) {
        peek.fill_head_base(container, no_close_action);
    }

    /// `_fillTitleIcon(container)`.
    fn fill_title_icon(&self, _peek: &PeekViewWidget, _container: NodeId) {}

    /// `_getActionBarOptions()`.
    fn get_action_bar_options(&self, peek: &PeekViewWidget) -> IActionBarOptions {
        peek.get_action_bar_options_base()
    }

    /// `_onTitleClick(event)`: só vale com a opção `supportOnTitleClick`.
    fn on_title_click(&self, _peek: &PeekViewWidget, _event: &mut UiEvent) {}

    /// `_doLayoutHead(heightInPixel, widthInPixel)`.
    fn do_layout_head(&self, peek: &PeekViewWidget, height_in_pixel: f64, width_in_pixel: f64) {
        peek.do_layout_head_base(height_in_pixel, width_in_pixel);
    }

    /// `_doLayoutBody(heightInPixel, widthInPixel)`.
    fn do_layout_body(&self, peek: &PeekViewWidget, height_in_pixel: f64, width_in_pixel: f64) {
        peek.do_layout_body_base(height_in_pixel, width_in_pixel);
    }

    /// `_onWidth(widthInPixel)`.
    fn on_width(&self, _peek: &PeekViewWidget, _width_in_pixel: f64) {}

    /// `_applyStyles()`.
    fn apply_styles(&self, peek: &PeekViewWidget) {
        peek.apply_styles_base();
    }

    /// `_getWidth(info)`.
    fn get_width(&self, peek: &PeekViewWidget, info: &EditorLayoutInfo) -> f64 {
        peek.zone().get_width_base(info)
    }

    /// `_getMaximumHeightInLines()`.
    fn get_maximum_height_in_lines(&self, peek: &PeekViewWidget) -> Option<f64> {
        peek.zone().get_maximum_height_in_lines_base()
    }

    /// `revealRange(range, isLastLine)`.
    fn reveal_range(&self, peek: &PeekViewWidget, range: Range, is_last_line: bool) {
        peek.zone().reveal_range_base(range, is_last_line);
    }

    /// `_getResizeBounds()`.
    fn get_resize_bounds(&self, peek: &PeekViewWidget) -> ZoneResizeBounds {
        peek.zone().get_resize_bounds_base()
    }
}

/// Os ganchos de um `PeekViewWidget` que ainda não tem subclasse registrada: só os corpos padrão.
struct NoPeekHooks;

impl IDisposable for NoPeekHooks {
    fn dispose(&self) {}
}

impl PeekViewHooks for NoPeekHooks {
    fn fill_body(&self, _container: NodeId) {}
}

/// `PeekViewWidget`.
pub struct PeekViewWidget {
    me: Weak<PeekViewWidget>,
    zone: Rc<ZoneWidget>,
    hooks: RefCell<Option<Weak<dyn PeekViewHooks>>>,
    options: RefCell<PeekViewOptions>,
    on_did_close: Rc<Emitter<()>>,
    disposed: Cell<bool>,
    head_element: Cell<Option<NodeId>>,
    title_element: Cell<Option<NodeId>>,
    primary_heading: Cell<Option<NodeId>>,
    secondary_heading: Cell<Option<NodeId>>,
    meta_heading: Cell<Option<NodeId>>,
    actions_container: Cell<Option<NodeId>>,
    body_element: Cell<Option<NodeId>>,
    actionbar_widget: RefCell<Option<Rc<ActionBar>>>,
    /// A altura do cabeçalho da última medição (o corpo começa abaixo dela).
    head_height: Cell<f64>,
}

impl PeekViewWidget {
    /// O construtor: `options.zone` recebe os padrões do `ZoneWidget`, e o resto os do peek.
    pub fn new(editor: &Rc<CodeEditorWidget>, host: ZoneWidgetHost, options: PeekViewOptions) -> Rc<Self> {
        let mut options = options;
        let zone = ZoneWidget::new(editor, host, options.zone.clone());
        options.mixin_defaults();

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            zone: Rc::clone(&zone),
            hooks: RefCell::new(None),
            options: RefCell::new(options),
            on_did_close: Emitter::new(),
            disposed: Cell::new(false),
            head_element: Cell::new(None),
            title_element: Cell::new(None),
            primary_heading: Cell::new(None),
            secondary_heading: Cell::new(None),
            meta_heading: Cell::new(None),
            actions_container: Cell::new(None),
            body_element: Cell::new(None),
            actionbar_widget: RefCell::new(None),
            head_height: Cell::new(0.0),
        });
        zone.set_hooks(Rc::downgrade(&this) as Weak<dyn ZoneWidgetHooks>);

        let observable_editor = observable_code_editor(editor);
        let opened_peek_widgets = &observable_editor.opened_peek_widgets;
        opened_peek_widgets.set(opened_peek_widgets.get() + 1, None);
        this
    }

    /// Registra a subclasse como os ganchos do peek (ver o topo do módulo).
    pub fn set_hooks(&self, hooks: Weak<dyn PeekViewHooks>) {
        *self.hooks.borrow_mut() = Some(hooks);
    }

    fn peek_hooks_opt(&self) -> Option<Rc<dyn PeekViewHooks>> {
        self.hooks.borrow().as_ref().and_then(Weak::upgrade)
    }

    fn peek_hooks(&self) -> Rc<dyn PeekViewHooks> {
        self.peek_hooks_opt().unwrap_or_else(|| Rc::new(NoPeekHooks))
    }

    // #region Estado

    /// O `ZoneWidget` que o peek estende.
    pub fn zone(&self) -> &Rc<ZoneWidget> {
        &self.zone
    }

    /// `editor`.
    pub fn editor(&self) -> &Rc<CodeEditorWidget> {
        self.zone.editor()
    }

    /// `options` do peek (as cores dos títulos e do cabeçalho, que o pintor lê, e a opção `supportOnTitleClick`).
    pub fn options(&self) -> PeekViewOptions {
        self.options.borrow().clone()
    }

    /// `onDidClose`.
    pub fn on_did_close(&self) -> Event<()> {
        self.on_did_close.event()
    }

    /// `_headElement`.
    pub fn head_element(&self) -> Option<NodeId> {
        self.head_element.get()
    }

    /// `_titleElement`.
    pub fn title_element(&self) -> Option<NodeId> {
        self.title_element.get()
    }

    /// `_primaryHeading`.
    pub fn primary_heading(&self) -> Option<NodeId> {
        self.primary_heading.get()
    }

    /// `_secondaryHeading`.
    pub fn secondary_heading(&self) -> Option<NodeId> {
        self.secondary_heading.get()
    }

    /// `_metaHeading`.
    pub fn meta_heading(&self) -> Option<NodeId> {
        self.meta_heading.get()
    }

    /// O contêiner `.peekview-actions` do cabeçalho.
    pub fn actions_container(&self) -> Option<NodeId> {
        self.actions_container.get()
    }

    /// `_bodyElement`.
    pub fn body_element(&self) -> Option<NodeId> {
        self.body_element.get()
    }

    /// `_actionbarWidget`.
    pub fn actionbar_widget(&self) -> Option<Rc<ActionBar>> {
        self.actionbar_widget.borrow().clone()
    }

    // #endregion

    // #region Ciclo de vida

    /// `create()`.
    pub fn create(&self) {
        self.zone.create();
    }

    /// `this.dispose()` do original, que despacha para a subclasse (ver o topo do módulo).
    pub fn dispose_widget(&self) {
        match self.peek_hooks_opt() {
            Some(hooks) => hooks.dispose(),
            None => self.dispose(),
        }
    }

    /// `style(styles)`.
    pub fn style(&self, styles: PeekViewStyles) {
        {
            let mut options = self.options.borrow_mut();
            if let Some(color) = styles.header_background_color {
                options.header_background_color = Some(color);
            }
            if let Some(color) = styles.primary_heading_color {
                options.primary_heading_color = Some(color);
            }
            if let Some(color) = styles.secondary_heading_color {
                options.secondary_heading_color = Some(color);
            }
        }
        self.zone.style(styles.zone);
    }

    /// `PeekViewWidget._applyStyles()`.
    pub fn apply_styles_base(&self) {
        self.zone.apply_styles_base();
        let header_background_color = self.options.borrow().header_background_color.clone();
        let frame_color = self.zone.options().frame_color;
        let ui = &self.zone.host().ui;
        if let (Some(head), Some(color)) = (self.head_element.get(), header_background_color) {
            ui.tree_mut().style_mut(head).background = Some(to_rgba8(&color));
        }
        // O `style.color` dos dois títulos é lido pelo pintor em `options()`.
        if let (Some(body), Some(color)) = (self.body_element.get(), frame_color) {
            let mut tree = ui.tree_mut();
            let style = tree.style_mut(body);
            style.border = Some(Border { width: 1.0, color: to_rgba8(&color) });
            style.border_widths = Some([1.0, 0.0, 0.0, 0.0]);
        }
    }

    // #endregion

    // #region Cabeçalho e corpo

    /// `_fillContainer(container)`.
    fn fill_container_base(&self, container: NodeId) {
        self.zone.set_css_class("peekview-widget", None);

        let ui = &self.zone.host().ui;
        let (head, body) = {
            let mut tree = ui.tree_mut();
            (create_detached_el(&mut tree, Role::Generic, "head"), create_detached_el(&mut tree, Role::Generic, "body"))
        };
        self.head_element.set(Some(head));
        self.body_element.set(Some(body));

        let hooks = self.peek_hooks();
        hooks.fill_head(self, head, false);
        hooks.fill_body(body);

        let mut tree = ui.tree_mut();
        tree.append_child(container, head);
        tree.append_child(container, body);
    }

    /// `PeekViewWidget._fillHead(container, noCloseAction?)`.
    pub fn fill_head_base(&self, _container: NodeId, no_close_action: bool) {
        let Some(head) = self.head_element.get() else {
            return;
        };
        let ui = self.zone.host().ui.clone();
        let support_on_title_click = self.options.borrow().support_on_title_click.unwrap_or(false);

        let title = {
            let mut tree = ui.tree_mut();
            let title = create_el(&mut tree, head, Role::Generic, "peekview-title");
            if support_on_title_click {
                tree.add_class(title, "clickable");
            }
            title
        };
        self.title_element.set(Some(title));
        if support_on_title_click {
            let weak = self.me.clone();
            self.zone.disposables().add(ui.listen(title, EventKind::Click, false, move |event| {
                if let Some(peek) = weak.upgrade() {
                    peek.peek_hooks().on_title_click(&peek, event);
                }
            }));
        }

        self.peek_hooks().fill_title_icon(self, title);
        let (primary, secondary, meta, actions) = {
            let mut tree = ui.tree_mut();
            (
                create_el(&mut tree, title, Role::Generic, "filename"),
                create_el(&mut tree, title, Role::Generic, "dirname"),
                create_el(&mut tree, title, Role::Generic, "meta"),
                create_el(&mut tree, head, Role::Generic, "peekview-actions"),
            )
        };
        self.primary_heading.set(Some(primary));
        self.secondary_heading.set(Some(secondary));
        self.meta_heading.set(Some(meta));
        self.actions_container.set(Some(actions));

        let action_bar_options = self.peek_hooks().get_action_bar_options(self);
        let action_bar = ActionBar::new(&ui, actions, action_bar_options);
        self.zone.disposables().add_rc(Rc::clone(&action_bar));
        *self.actionbar_widget.borrow_mut() = Some(Rc::clone(&action_bar));

        if !no_close_action {
            let weak = self.me.clone();
            let close = Action::new(
                "peekview.close",
                &localize("label.close", "Close", &[]),
                &ThemeIcon::from_id("close").as_class_name(),
                true,
                Some(Rc::new(move |_| {
                    if let Some(peek) = weak.upgrade() {
                        peek.dispose_widget();
                    }
                    MaybePromise::from(Dynamic::Undefined)
                })),
            );
            let close = self.zone.disposables().add_rc(close);
            let close: ActionRef = close;
            action_bar.push_action(
                close,
                IActionOptions { item: IActionViewItemOptions { label: Some(false), icon: Some(true), ..IActionViewItemOptions::default() }, index: None },
            );
        }
    }

    /// `PeekViewWidget._getActionBarOptions()`.
    pub fn get_action_bar_options_base(&self) -> IActionBarOptions {
        IActionBarOptions { orientation: Some(ActionsOrientation::Horizontal), ..IActionBarOptions::default() }
    }

    /// `setTitle(primaryHeading, secondaryHeading?)`.
    pub fn set_title(&self, primary_heading: &str, secondary_heading: Option<&str>) {
        let (Some(primary), Some(secondary)) = (self.primary_heading.get(), self.secondary_heading.get()) else {
            return;
        };
        let mut tree = self.zone.host().ui.tree_mut();
        tree.set_label(primary, primary_heading);
        tree.set_attribute(primary, "title", primary_heading);
        match secondary_heading.filter(|heading| !heading.is_empty()) {
            Some(heading) => tree.set_label(secondary, heading),
            None => tree.clear_label(secondary),
        }
    }

    /// `setMetaTitle(value)`.
    pub fn set_meta_title(&self, value: &str) {
        let Some(meta) = self.meta_heading.get() else {
            return;
        };
        let mut tree = self.zone.host().ui.tree_mut();
        if value.is_empty() {
            hide(&mut tree, meta);
        } else {
            tree.set_label(meta, value);
            show(&mut tree, meta);
        }
    }

    // #endregion

    // #region Layout

    /// `PeekViewWidget._doLayout(heightInPixel, widthInPixel)`.
    fn do_layout_base(&self, height_in_pixel: f64, width_in_pixel: f64) {
        if !self.zone.is_showing() && height_in_pixel < 0.0 {
            // Looks like the view zone got folded away!
            self.dispose_widget();
            return;
        }

        let head_height = (*self.zone.editor().get_option(LINE_HEIGHT) * 1.2).ceil();
        let body_height = (height_in_pixel - (head_height + 1.0 /* the border-top width */)).round();

        let hooks = self.peek_hooks();
        hooks.do_layout_head(self, head_height, width_in_pixel);
        hooks.do_layout_body(self, body_height, width_in_pixel);
    }

    /// `PeekViewWidget._doLayoutHead(heightInPixel, widthInPixel)`: o cabeçalho ocupa a largura toda; o título fica à
    /// esquerda (com a margem de 20px) e as ações à direita (com o recuo de 2px), como o `flex` do CSS.
    pub fn do_layout_head_base(&self, height_in_pixel: f64, width_in_pixel: f64) {
        let Some(head) = self.head_element.get() else {
            return;
        };
        self.head_height.set(height_in_pixel);
        let ui = &self.zone.host().ui;
        ui.tree_mut().set_bounds(head, Rect::new(0.0, 0.0, width_in_pixel, height_in_pixel));

        let (Some(title), Some(actions)) = (self.title_element.get(), self.actions_container.get()) else {
            return;
        };
        let action_bar = self.actionbar_widget();
        let actions_right = (width_in_pixel - ACTIONS_PADDING_RIGHT).max(0.0);
        let mut actions_width = 0.0;
        if let Some(action_bar) = &action_bar {
            ui.tree_mut().set_bounds(actions, Rect::new(0.0, 0.0, actions_right, height_in_pixel));
            action_bar.layout();
            actions_width = (0..action_bar.length()).map(|index| action_bar.get_width(index)).sum::<f64>();
        }
        let actions_left = (actions_right - actions_width).max(0.0);
        {
            let mut tree = ui.tree_mut();
            tree.set_bounds(actions, Rect::new(actions_left, 0.0, actions_right.max(actions_left), height_in_pixel));
            tree.set_bounds(title, Rect::new(TITLE_MARGIN_LEFT, 0.0, actions_left.max(TITLE_MARGIN_LEFT), height_in_pixel));
        }
        if let Some(action_bar) = action_bar {
            action_bar.layout();
        }
    }

    /// `PeekViewWidget._doLayoutBody(heightInPixel, widthInPixel)`: o corpo começa abaixo do cabeçalho e a borda de cima
    /// (1px) soma à altura.
    pub fn do_layout_body_base(&self, height_in_pixel: f64, width_in_pixel: f64) {
        if let Some(body) = self.body_element.get() {
            let top = self.head_height.get();
            self.zone.host().ui.tree_mut().set_bounds(body, Rect::new(0.0, top, width_in_pixel, top + height_in_pixel + 1.0));
        }
    }

    // #endregion
}

impl IDisposable for PeekViewWidget {
    /// `dispose()`: o `super.dispose()` da subclasse.
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        // prevent consumers who dispose on onDidClose from looping
        self.zone.dispose();
        self.on_did_close.fire(());
        self.on_did_close.dispose();

        let observable_editor = observable_code_editor(self.zone.editor());
        let opened_peek_widgets = &observable_editor.opened_peek_widgets;
        opened_peek_widgets.set(opened_peek_widgets.get() - 1, None);
    }
}

impl IPeekViewWidget for PeekViewWidget {
    fn on_did_close(&self) -> Event<()> {
        self.on_did_close.event()
    }
}

impl ZoneWidgetHooks for PeekViewWidget {
    fn fill_container(&self, _zone: &ZoneWidget, container: NodeId) {
        self.fill_container_base(container);
    }

    fn on_width(&self, _zone: &ZoneWidget, width_in_pixel: f64) {
        self.peek_hooks().on_width(self, width_in_pixel);
    }

    fn do_layout(&self, _zone: &ZoneWidget, height_in_pixel: f64, width_in_pixel: f64) {
        self.do_layout_base(height_in_pixel, width_in_pixel);
    }

    fn apply_styles(&self, _zone: &ZoneWidget) {
        self.peek_hooks().apply_styles(self);
    }

    fn get_width(&self, _zone: &ZoneWidget, info: &EditorLayoutInfo) -> f64 {
        self.peek_hooks().get_width(self, info)
    }

    fn get_maximum_height_in_lines(&self, _zone: &ZoneWidget) -> Option<f64> {
        self.peek_hooks().get_maximum_height_in_lines(self)
    }

    fn reveal_range(&self, _zone: &ZoneWidget, range: Range, is_last_line: bool) {
        self.peek_hooks().reveal_range(self, range, is_last_line);
    }

    fn get_resize_bounds(&self, _zone: &ZoneWidget) -> ZoneResizeBounds {
        self.peek_hooks().get_resize_bounds(self)
    }
}

// #endregion

/// `registerSingleton(IPeekViewService, ..., Delayed)` e `registerEditorContribution(PeekContextController.ID, ..., Eager)`
/// do carregamento do módulo (ver o topo do módulo). `context_key_service` entrega o `IContextKeyService` de cada editor.
pub fn register(context_key_service: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> Rc<dyn IContextKeyService>>) {
    if REGISTERED.replace(true) {
        return;
    }
    register_singleton::<PeekViewService>(I_PEEK_VIEW_SERVICE, InstantiationType::Delayed);
    register_editor_contribution(
        PeekContextController::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            PeekContextController::new(editor, &*context_key_service(editor))
        }),
        EditorContributionInstantiation::Eager,
    );
}
