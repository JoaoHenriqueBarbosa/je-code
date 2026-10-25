//! Porte de `src/vs/workbench/test/browser/parts/editor/multiEditorTabsControl.test.ts` (suíte `MultiEditorTabsControl`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **Sem DOM.** O original mede o navegador (`offsetWidth`, `getBoundingClientRect`, `getComputedStyle`) e lê classes dos
//!   elementos. Aqui as medidas vêm de [`Layout`], um `TabsMeasurer` determinístico (largura por caractere, coluna da ação,
//!   linhas de quebra, margem do ombro da última aba conectada, zoom e direção), que responde pelo estado vigente a cada
//!   leitura como o navegador faz, e as classes e variáveis que o original lê são o estado que o controle escreve
//!   (`TabState`, `TabsContainerView`). Cada caso confere o que o CONTROLE decide (altura em cache, `connected-tab-last`,
//!   `wrapping`, `--last-tab-add-tab-width`, `clip-path` por aba, a ação da aba); a pintura (cor, gradiente, raio, `::before`,
//!   `::after`, borda computada) é da renderização nativa e não é estado do controle.
//! - **Quadro de animação.** `await layoutConnectedGroup(...)` é [`Fixture::layout`]: guarda as dimensões (`layout`) e executa
//!   o layout agendado (`LayoutScheduler::take` seguido de `do_layout`), que é o que o `scheduleAtNextAnimationFrame` faz.
//! - **Entradas.** `class extends TestFileEditorInput { getName() ... }` é [`NamedInput`], com nome e descrições curta e longa.
//! - **Menu do Add Tab** (`IMenuService`, `addMenuVisible`, `addMenuChange`): o botão é `container.add_tab` (`Visible`,
//!   `Hidden`, `None`), e o disparo do evento do menu é [`Fixture::set_add_visible`], que escreve a presença e agenda o layout.
//! - **Classes da raiz** (`modern-ui-connected-editor-tabs`, `dir`, `style.zoom`) são [`TabsParentTraits`], a direção e o zoom
//!   do [`Layout`].
//! - **`ModifierKeyEmitter` e `hostService.setFocus`** chegam como `set_alt_pressed` e `handle_window_focus_change`.
//! - **Largura do rótulo e coluna da ação.** `Layout::action_column_width` é o `--modern-ui-tab-action-fit-padding` (28px, e no
//!   estilo conectado `calc((tab-height + size40 * 2 + strokeThickness - size200) / 2 + size240)`, com `--editor-group-tab-height`
//!   de 24px ou 20px no compacto) e `Layout::label_width` é a largura do `.tab-label` com ou sem a coluna visível. O selo do
//!   `DecorationsService` é `Layout::badged` (o provedor) com `handle_decorations_change` (o `onDidChangeDecorations`), e o texto
//!   da descrição só entra na largura nos casos que a medem (`show_descriptions`).
//!
//! Casos `modern editor headers retain an opaque backing ...` e `editor group owns the header backing ...`: o primeiro confere
//! as declarações de `tabs.css` e `connectedEditorTabs.css` (`modern_header_backing`) e o segundo o `updateStyles` do
//! `EditorGroupView` (o `titleContainer` e as cores de tema do cabeçalho) sobre uma parte criada por `create_editor_part`,
//! com o `getComputedStyle` do `.title` lido de `computed_title_background`. Nos casos
//! `Add Tab spacing ...`, `scrolling tab content is clipped ...` e `wrapped Add Tab stays unobscured ...` só a pintura sai
//! (`::before` do botão, raio, fundo do contêiner, centralização vertical do botão com o toolbar). As medidas de largura,
//! margem e folga do `Layout` vêm de `tab_box_style` e `layout_tabs_row` (`multieditortabscontrol.css`, o `flex` do
//! `tabs-container`), com o que `tabs.css` e `connectedEditorTabs.css` somam, e com as variáveis que `connectedGroup()`
//! declara (raio médio 6px, traço 1px, `size20` 2px, `size40` 4px).
//!
//! Casos de `connected single rows use panel height ...` em diante: leem as abas de todas as barras como `querySelectorAll` lê o
//! documento (`Fixture::live_bars` e `Fixture::tab_views`), com a caixa da `.tab`, do `.tab-fill` e do `.action-label` em coordenadas do
//! grupo e com o zoom da raiz. O contorno do preenchimento (`tab_fill_shape`), o fundo da aba inativa e da ativa
//! (`connected_inactive_tab_background`, `connected_active_tab_paint`), o `overflow` do título (`connected_title_overflow`), o recuo do
//! botão de nova aba (`connected_add_tab_button_insets`) e o realce de topo (`dirty_top_accent`) vêm do que o controle resolve para a
//! pintura nativa. O tema só troca cor e traço: os laços de temas percorrem todos e a geometria vale para cada um.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
mod editor_input_services;
#[macro_use]
#[path = "editor_group_model_suite/mod.rs"]
mod suite;
#[path = "../../je-editor/tests/support/test_color_theme_service.rs"]
mod test_color_theme_service;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use editor_input_services::{WorkbenchServices, uri_identity_service};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::{I_THEME_SERVICE, IThemeService};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::resources::uri_file;
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageService};
use je_platform::uri_identity::uri_identity::I_URI_IDENTITY_SERVICE;
use je_theme::color::Color;
use je_ui::Ui;
use je_workbench::browser::parts::editor::editor::default_editor_part_options;
use je_workbench::browser::parts::editor::editor_group_view::EditorGroupView;
use je_workbench::browser::parts::editor::editor_tabs_control::{
    EditorTabsControlBase, EditorTitleControlDimensions, TabsControlHost, TabsParentTraits, compute_tab_height,
};
use je_workbench::browser::parts::editor::multi_editor_tabs_control::{
    ConnectedActiveTabPaint, ConnectedTabColors, ConnectedTabPaintInput, ModernSpacing, ShoulderShadow, TabDividerInput, connected_active_tab_paint, tab_divider,
};
use je_workbench::browser::parts::editor::multi_editor_tabs_control::{
    AddTabAlignmentMeasure, AddTabPresence, AlignTabMeasure, BoxRect, ConnectedActiveTabMeasure, ConnectedClippingMeasure, CssLength, FitTabMetrics, LayoutOptions,
    MultiEditorTabsControlState, OverflowMode, StrokeSide, Tab, TabAction, TabActionLabelContext, TabBoxStyle, TabClipPath, TabColors, TabCssClasses, TabFillShape,
    TabFillStrokeInput, TabFlexItem, TabSizingParams, TabActionGeometry, TabBoxContext, TabStrokeColor, TabStrokeStyle, TabsContainerView, TabsMeasurer, TabsMetrics,
    WrapTabMeasure, WrappingMeasure, connected_add_tab_button_insets, connected_inactive_tab_background, connected_title_overflow, dirty_top_accent,
    high_contrast_connected_tab_fill_stroke, layout_tabs_row, tab_action_geometry, tab_action_label_opacity, tab_box_style, tab_fill_rect, tab_fill_shape,
};
use je_workbench::browser::parts::editor::multi_row_editor_tabs_control::MultiRowEditorControl;
use je_workbench::common::editor::editor_group_model::{EditorGroupModel, EditorOrIndex, IReadonlyEditorGroupModel};
use je_workbench::common::editor::editor_factory_registry::editor_factory_registry;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::{EditorInputOrUntyped, EditorInputRef, EditorsOrder, GroupIdentifier, Verbosity};
use je_workbench::contrib::modern_ui::resolved_style::css_rgb;
use je_workbench::contrib::modern_ui::tabs_header::{ModernHeaderBacking, modern_header_backing};
use je_workbench::services::editor::editor_groups_service::{
    EditorActionsLocation, EditorLabelFormat, EditorPartDecorationOptions, EditorPartOptions, IEditorGroup, IEditorPart, PinnedTabSizing, TabActionLocation, TabHeight, TabSizing,
};
use je_workbench::services::files_configuration::files_configuration_service::{I_FILES_CONFIGURATION_SERVICE, IFilesConfigurationService};
use je_workbench::test::browser::workbench_test_services::{TestEditorPart, TestFileEditorInput, create_editor_part};
use suite::Suite;
use test_color_theme_service::{TestColorTheme, TestThemeService};

//#region Entradas e dublês

/// `new class extends TestFileEditorInput { getName() ...; getDescription(verbosity) ... }(URI.file(path), 'testEditorInput')`.
struct NamedInput {
    base: EditorInputBase,
    resource: URI,
    name: String,
    /// A descrição curta e a longa (`Verbosity.LONG`), se o caso as define.
    descriptions: Option<(String, String)>,
    /// `TestFileEditorInput.dirty`.
    dirty: Cell<bool>,
}

impl NamedInput {
    fn new(name: &str, path: &str, descriptions: Option<(&str, &str)>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            base: EditorInputBase::new(this.clone()),
            resource: uri_file(path),
            name: name.to_string(),
            descriptions: descriptions.map(|(short, long)| (short.to_string(), long.to_string())),
            dirty: Cell::new(false),
        })
    }

    /// `TestFileEditorInput.setDirty()`.
    fn set_dirty(&self) {
        self.dirty.set(true);
    }
}

impl IDisposable for NamedInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for NamedInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        "testEditorInput".to_string()
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn get_description(&self, verbosity: Option<Verbosity>) -> Option<String> {
        self.descriptions.as_ref().map(|(short, long)| if verbosity == Some(Verbosity::Long) { long.clone() } else { short.clone() })
    }

    fn is_dirty(&self) -> bool {
        self.dirty.get()
    }
}

/// O `groupView` do caso (`mock<IEditorGroupView>`): só o que a base lê, sobre o modelo.
struct Host {
    model: Rc<EditorGroupModel>,
    options: Rc<RefCell<EditorPartOptions>>,
}

fn typed(editor: &EditorInputRef) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(editor.clone())
}

impl TabsControlHost for Host {
    fn group_id(&self) -> GroupIdentifier {
        self.model.id()
    }

    fn group_label(&self) -> String {
        "Editor Group 1".to_string()
    }

    fn group_aria_label(&self) -> String {
        "Editor Group 1".to_string()
    }

    fn group_count(&self) -> usize {
        self.model.count()
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        self.model.active_editor()
    }

    fn selected_editors(&self) -> Vec<EditorInputRef> {
        self.model.selected_editors()
    }

    fn group_editors(&self) -> Vec<EditorInputRef> {
        self.model.get_editors(EditorsOrder::Sequential, None)
    }

    fn index_of_editor(&self, editor: &EditorInputRef) -> i64 {
        self.model.index_of(Some(&typed(editor)), None, None)
    }

    fn is_active(&self, editor: &EditorInputRef) -> bool {
        self.model.is_active(&typed(editor))
    }

    fn is_pinned(&self, editor: &EditorInputRef) -> bool {
        self.model.is_pinned(&EditorOrIndex::Editor(editor.clone()))
    }

    fn is_sticky_at(&self, index: usize) -> bool {
        self.model.is_sticky(&EditorOrIndex::Index(index as i64))
    }

    fn is_group_active(&self) -> bool {
        true
    }

    fn part_options(&self) -> EditorPartOptions {
        self.options.borrow().clone()
    }

    fn editor_parts_count(&self) -> usize {
        1
    }

    fn relayout(&self) {}
}

//#endregion

//#region Layout determinístico (o navegador do original)

const CHAR_WIDTH: f64 = 7.0;
const TAB_PADDING: f64 = 16.0;
const ACTION_COLUMN: f64 = 28.0;
const ADD_TAB_WIDTH: f64 = 28.0;
/// `--modern-ui-connected-tab-shoulder-radius`: o raio médio de `connectedGroup()` (6px) mais o traço (1px). É a largura do
/// `::before` e do `::after` da aba ativa, e portanto o `shoulderExtent` que o controle lê.
const SHOULDER_RADIUS: f64 = 7.0;
/// `--vscode-strokeThickness` de `connectedGroup()`.
const STROKE: f64 = 1.0;
/// `--vscode-spacing-size20` de `connectedGroup()`: a folga da ponta do preenchimento das abas inativas e do botão de nova aba.
const SPACING_SIZE20: f64 = 2.0;
/// `--vscode-spacing-size40`: o `--modern-ui-tab-terminal-gutter` do Modern UI sem o estilo conectado.
const TAB_GUTTER: f64 = 4.0;
/// `margin-inline-end` da última aba conectada fora da quebra: o ombro do terminal (`shoulder-radius + strokeThickness`).
const SHOULDER_MARGIN: f64 = SHOULDER_RADIUS + STROKE;
/// O espaçamento vertical do `tabs-and-actions-container` no Modern UI.
const ROW_PADDING: f64 = 3.0;
/// `padding-top` do `tabs-and-actions-container` quando as abas quebram (`connectedEditorTabs.css` L303: `size20`) e `padding-bottom`
/// do `.tabs-container` (L184: `strokeThickness`). Somados dão o `ROW_PADDING` do estilo conectado.
const STRIP_PADDING_TOP: f64 = SPACING_SIZE20;
const TABS_PADDING_BOTTOM: f64 = STROKE;
/// `bottom` do `::after` do `tabs-and-actions-container` (a linha que separa o cabeçalho do documento, `tabs.css` L286 a L297), cuja
/// altura é `strokeThickness`.
const SEPARATOR_BOTTOM: f64 = 0.0;
/// A largura do selo de decoração (`::after` com a letra e a margem de 5px dos dois lados).
const BADGE_WIDTH: f64 = 17.0;

/// As posições de cada aba, na ordem lógica do texto.
struct Geometry {
    widths: Vec<f64>,
    margins: Vec<f64>,
    /// A caixa de cada aba que o flex recebeu (`tab_box_style`, com a margem do terminal): o `padding` computado dela.
    styles: Vec<TabBoxStyle>,
    offsets: Vec<f64>,
    tops: Vec<f64>,
    rows: usize,
    add_width: f64,
    /// `margin-inline-end` do botão de nova aba (a região do toolbar quando as abas quebram).
    add_margin: f64,
    add_top: f64,
    visible_width: f64,
    content_width: f64,
}

/// O `TabsMeasurer` do teste.
struct Layout {
    options: Rc<RefCell<EditorPartOptions>>,
    parent: Cell<TabsParentTraits>,
    width: Cell<f64>,
    toolbar: Cell<f64>,
    rtl: Cell<bool>,
    zoom: Cell<f64>,
    /// O `clipPath` de cada aba no instante de cada leitura de `measure_connected_clipping`.
    clip_reads: RefCell<Vec<Vec<Option<TabClipPath>>>>,
    /// Os índices de cada leitura em lote de `measure_fit_tabs` (`getFitTabWidths`).
    fit_reads: RefCell<Vec<Vec<usize>>>,
    /// O recurso que o provedor de decorações marca com um selo (`provideDecorations`), se o caso registra um.
    badged: RefCell<Option<URI>>,
    /// O rótulo inclui o texto da descrição (`label-description`), nos casos que medem a descrição.
    show_descriptions: Cell<bool>,
    /// `--modern-ui-connected-tab-shoulder-radius` (mais o traço, na conta da lacuna final das abas quebradas): o valor de
    /// `connectedGroup()`, que um caso pode sobrescrever na raiz.
    shoulder_radius: Cell<f64>,
}

fn name_chars(tab: &Tab) -> f64 {
    let name = tab.state.label.name.as_deref().map_or(0, |name| name.chars().count());
    let prefix = tab.state.label.name_prefix.as_deref().map_or(0, |prefix| prefix.chars().count());
    (name + prefix) as f64
}

impl Layout {
    fn new(options: Rc<RefCell<EditorPartOptions>>, parent: TabsParentTraits) -> Rc<Self> {
        Rc::new(Self {
            options,
            parent: Cell::new(parent),
            width: Cell::new(800.0),
            toolbar: Cell::new(0.0),
            rtl: Cell::new(false),
            zoom: Cell::new(1.0),
            clip_reads: RefCell::new(Vec::new()),
            fit_reads: RefCell::new(Vec::new()),
            badged: RefCell::new(None),
            show_descriptions: Cell::new(false),
            shoulder_radius: Cell::new(SHOULDER_RADIUS),
        })
    }

    /// `--editor-group-tab-height` do Modern UI (`tabs.css`): 24px, ou 20px no modo compacto.
    fn css_tab_height(&self) -> f64 {
        if self.options.borrow().tab_height == TabHeight::Compact { 20.0 } else { 24.0 }
    }

    /// `--modern-ui-tab-action-fit-padding`: 28px (`--vscode-spacing-size280`, o valor inicial) e, no estilo conectado,
    /// `calc((tab-height + size40 * 2 + strokeThickness - size200) / 2 + size240)` (`connectedEditorTabs.css`).
    fn action_column_width(&self) -> f64 {
        TabBoxContext { connected: self.is_connected(), editor_group_tab_height: self.css_tab_height(), ..TabBoxContext::default() }.action_fit_padding()
    }

    /// `.modern-ui-tabs.modern-ui-connected-editor-tabs` na raiz.
    fn is_connected(&self) -> bool {
        let parent = self.parent.get();
        parent.in_modern_ui_tabs && parent.in_connected_editor_tabs
    }

    /// As variáveis, classes do contêiner e da linha que as regras de geometria de `connectedEditorTabs.css` leem, sem a medida
    /// da aba (`connectedGroup()` põe `.modern-ui` na raiz e o grupo `.active`).
    fn base_context(&self, container: &TabsContainerView, tab: &Tab) -> TabBoxContext {
        TabBoxContext {
            connected: self.is_connected(),
            modern_ui_root: true,
            modern_ui_tabs: self.parent.get().in_modern_ui_tabs,
            two_tab_bars: container.bars.two_tab_bars,
            first_tab_bar: container.bars.top_tab_bar,
            editor_group_tab_height: self.css_tab_height(),
            wrapping: container.wrapping,
            upper_bar: container.connected_tab_upper_bar,
            upper_row: tab.state.connected_tab_upper_row,
            active: tab.state.active,
            ..TabBoxContext::default()
        }
    }

    /// O contexto de geometria da aba `index`, com a largura que o layout deu a ela (`getBoundingClientRect()`) e a altura da
    /// linha (`--modern-ui-connected-tab-row-height`, que é também a altura da aba conectada).
    fn box_context(&self, container: &TabsContainerView, tabs: &[Tab], index: usize, tab_width: f64) -> TabBoxContext {
        let mut context = self.base_context(container, &tabs[index]);
        context.starts_row = index == 0 || tabs[index - 1].state.last_in_row;
        context.tab_width = tab_width;
        context.tab_height = if context.connected { context.connected_row_height() } else { self.tab_height() };
        context
    }

    /// O contexto de geometria da aba `index` de uma barra, com a largura que o layout deu a ela.
    fn bar_box_context(&self, bar: &MultiEditorTabsControlState, index: usize) -> TabBoxContext {
        let tab_width = self.geometry(&bar.container, &bar.tabs).widths[index];

        self.box_context(&bar.container, &bar.tabs, index, tab_width)
    }

    /// `(left, right)` do `.monaco-icon-label-container` dentro da aba: o rótulo vai do recuo ao início da coluna da ação (ação
    /// à direita) ou do fim da coluna ao recuo do outro lado (ação à esquerda).
    fn label_span(&self, box_width: f64, action_padding: Option<f64>, actions_left: bool) -> (f64, f64) {
        let side = TAB_PADDING / 2.0;
        if actions_left { (action_padding.unwrap_or(side), box_width - side) } else { (side, side + self.label_width(box_width, action_padding)) }
    }

    /// `--modern-ui-tab-action-padding` computado da aba: o recuo real do lado da ação, que difere do `fit-padding` no estilo
    /// conectado. Na linha superior vale `size280`, ou a fórmula do `fit-padding` com a aba suja ou fixada
    /// (`connectedEditorTabs.css` L702 e L742). Fora da linha superior e da quebra vale `inset + size240` (L521), com
    /// `inset = (row-height - fill-block-inset + strokeThickness - size200) / 2`, onde a barra superior usa `tab-height + size40`
    /// com `fill-block-inset = size20` (L119 e L125) e o resto `tab-height + size40 * 2 - strokeThickness` com
    /// `fill-block-inset = -strokeThickness` (L158 e L164). Na quebra vale `size280` (L121).
    fn action_padding(&self, tab: &Tab, container: &TabsContainerView) -> f64 {
        self.base_context(container, tab).action_padding(&TabCssClasses::of(&tab.state, container))
    }

    /// A aba aloca a coluna da ação na largura: sem o Modern UI a coluna é sempre alocada, com ele só com a reserva de espaço
    /// ou com uma ação persistente (suja ou fixada), e `rest` é a medida da aba sem a coluna (`tab-action-sizing-rest`).
    fn has_action_column(&self, tab: &Tab, rest: bool) -> bool {
        let persistent = tab.state.dirty || tab.state.sticky;
        let reserved = !self.parent.get().in_modern_ui_tabs || self.options.borrow().tab_action_reserve_space || persistent;
        reserved && !rest
    }

    /// A largura de uma aba sem a restrição da linha (`max-width` solto).
    fn natural_width(&self, container: &TabsContainerView, tab: &Tab, rest: bool) -> f64 {
        let column = if self.has_action_column(tab, rest) { self.action_padding(tab, container) } else { 0.0 };
        let badge = if self.has_badge(tab) { BADGE_WIDTH } else { 0.0 };
        let description = if self.show_descriptions.get() { tab.state.label.description.chars().count() as f64 * CHAR_WIDTH } else { 0.0 };
        TAB_PADDING + name_chars(tab) * CHAR_WIDTH + column + badge + description
    }

    /// O rótulo da aba mostra o selo: as decorações com selo estão ligadas e o provedor decora o recurso da aba.
    fn has_badge(&self, tab: &Tab) -> bool {
        tab.state.label.file_decoration_badges && self.badged.borrow().as_ref().is_some_and(|uri| tab.label.editor.resource().as_ref() == Some(uri))
    }

    /// A largura do `.tab-label` de uma aba: a caixa menos o recuo de cada lado e, com a coluna da ação visível (reserva,
    /// ação persistente ou ação transitória sob `:hover`, `:focus-within` ou aba ativa), a coluna da ação no lugar do recuo
    /// daquele lado.
    fn label_width(&self, box_width: f64, action_padding: Option<f64>) -> f64 {
        let side = TAB_PADDING / 2.0;
        box_width - side - action_padding.unwrap_or(side)
    }

    /// `--modern-ui-tab-terminal-gutter`: o vão depois da última aba quebrada (só no Modern UI).
    fn terminal_gutter(&self, container: &TabsContainerView) -> f64 {
        let parent = self.parent.get();
        match (parent.in_modern_ui_tabs, parent.in_connected_editor_tabs && !container.connected_tab_upper_bar) {
            (false, _) => 0.0,
            (true, true) => self.shoulder_radius.get(),
            (true, false) => TAB_GUTTER,
        }
    }

    /// O que `tabs.css` e `connectedEditorTabs.css` somam ao `margin-inline-end` da última aba: fora da quebra só o estilo
    /// conectado reserva o ombro; na quebra vale o `--modern-ui-tab-terminal-gutter`. A região do toolbar
    /// (`--last-tab-margin-right`, a não ser que o botão de nova aba a assuma) é do `margin-inline-end` de
    /// `multieditortabscontrol.css`, que [`tab_box_style`] resolve.
    fn terminal_extra_margin(&self, container: &TabsContainerView) -> f64 {
        if !container.wrapping {
            return if self.parent.get().in_connected_editor_tabs { SHOULDER_MARGIN } else { 0.0 };
        }

        self.terminal_gutter(container)
    }

    /// A ponta do botão de nova aba do lado da última aba (`getBoundingClientRect()` do `.action-label`, em px da tela) com a
    /// aba terminal em `tab` (`(left, right)`): a aba de nova aba começa depois da margem do terminal e o rótulo recua
    /// `size20 - shoulder-radius - strokeThickness` (`connectedEditorTabs.css`, regra fora da quebra).
    fn add_button_near_edge(&self, tab: (f64, f64), terminal_margin: f64) -> f64 {
        let zoom = self.zoom.get();
        let offset = (terminal_margin + SPACING_SIZE20 - SHOULDER_RADIUS - STROKE) * zoom;
        if self.rtl.get() { tab.0 - offset } else { tab.1 + offset }
    }

    fn tab_height(&self) -> f64 {
        compute_tab_height(self.options.borrow().tab_height == TabHeight::Compact, self.parent.get(), false)
    }

    /// A altura de uma linha de abas: no estilo conectado `--modern-ui-connected-tab-row-height`, que muda com a quebra e com a
    /// barra superior (`TabBoxContext::connected_row_height`), e nos demais a altura da aba.
    fn row_height(&self, container: &TabsContainerView) -> f64 {
        if self.is_connected() {
            TabBoxContext {
                connected: true,
                editor_group_tab_height: self.css_tab_height(),
                wrapping: container.wrapping,
                upper_bar: container.connected_tab_upper_bar,
                ..TabBoxContext::default()
            }
            .connected_row_height()
        } else {
            self.tab_height()
        }
    }

    /// `(tabsContainer.offsetHeight, tabsAndActionsContainer.offsetHeight)` com `rows` linhas. No estilo conectado o `.tabs-container`
    /// soma o `padding-bottom` de `strokeThickness` (zero na barra superior, `connectedEditorTabs.css` L184 e L187) às linhas e o
    /// `tabs-and-actions-container` soma o `padding-top` de `size20` quando quebra ou é a barra superior (L298 e L303); nos demais
    /// estilos vale a altura da aba, e o Modern UI soma o espaçamento vertical da linha.
    fn header_heights(&self, container: &TabsContainerView, rows: usize) -> (f64, f64) {
        if self.is_connected() {
            let tabs = rows as f64 * self.row_height(container) + self.tabs_padding_bottom(container);

            return (tabs, self.strip_padding_top(container) + tabs);
        }
        let tabs = if container.wrapping { rows as f64 * self.tab_height() } else { self.tab_height() };

        (tabs, tabs + if self.parent.get().in_modern_ui_tabs { ROW_PADDING } else { 0.0 })
    }

    /// `padding-bottom` do `.tabs-container` no estilo conectado: `strokeThickness` (`connectedEditorTabs.css` L184), zero na barra
    /// superior (L187).
    fn tabs_padding_bottom(&self, container: &TabsContainerView) -> f64 {
        if container.connected_tab_upper_bar { 0.0 } else { TABS_PADDING_BOTTOM }
    }

    /// `padding-top` do `tabs-and-actions-container` no Modern UI: o estilo Pill dá `size20` só na quebra (`tabs.css` L283 e L319) e
    /// com duas barras nenhuma das duas o tem (L305 a L307); o conectado zera o recuo (`connectedEditorTabs.css` L149) e devolve
    /// `size20` na quebra sem duas barras e na barra superior (L301 a L304).
    fn strip_padding_top(&self, container: &TabsContainerView) -> f64 {
        let modern_ui_tabs = self.parent.get().in_modern_ui_tabs;
        let wrapping_alone = container.wrapping && !container.bars.two_tab_bars;
        if modern_ui_tabs && (wrapping_alone || self.is_connected() && container.connected_tab_upper_bar) { STRIP_PADDING_TOP } else { 0.0 }
    }

    /// `padding-left` do `tabs-and-actions-container`: o estilo Pill dá `size20` sempre (`tabs.css` L283) e o conectado zera o recuo
    /// (L148) e o devolve na quebra e na barra superior (L296 a L299).
    fn strip_padding_left(&self, container: &TabsContainerView) -> f64 {
        let parent = self.parent.get();
        if parent.in_modern_ui_tabs && (!parent.in_connected_editor_tabs || container.wrapping || container.connected_tab_upper_bar) { SPACING_SIZE20 } else { 0.0 }
    }

    /// `getBoundingClientRect()` do `.tab-label` na vertical, relativo ao topo da aba: o rótulo é um item flex com `margin: auto` na
    /// caixa de conteúdo da aba (`border-block-width: 0` no estilo conectado), então o centro é o centro da aba.
    fn label_center(&self, tab_height: f64) -> f64 {
        tab_height / 2.0
    }

    fn geometry(&self, container: &TabsContainerView, tabs: &[Tab]) -> Geometry {
        let parent = self.parent.get();
        let connected = parent.in_connected_editor_tabs;
        let add_width = if container.add_tab == AddTabPresence::Visible { ADD_TAB_WIDTH } else { 0.0 };
        let total = self.width.get();
        let toolbar = self.toolbar.get();
        let visible_width = if container.wrapping {
            total
        } else if connected {
            total - toolbar
        } else {
            total - toolbar - add_width
        };

        // Cada aba é um item flex do `tabs-container`: a caixa vem da cascata de `multieditortabscontrol.css` e a largura
        // intrínseca do modelo de texto deste teste (`natural_width`).
        let options = self.options.borrow();
        let params = TabSizingParams {
            fixed_min_width: options.tab_sizing_fixed_min_width,
            fixed_max_width: options.tab_sizing_fixed_max_width,
            current_width: None,
            fit_width: None,
            modern_ui_tabs: parent.in_modern_ui_tabs,
            connected_editor_tabs: connected,
            tabs_container_width: visible_width,
            last_tab_margin_right: container.last_tab_margin_right,
            last_tab_add_tab_width: container.last_tab_add_tab_width,
            terminal_gutter: self.terminal_gutter(container),
        };
        let extra_margin = self.terminal_extra_margin(container);
        let mut items: Vec<TabFlexItem> = tabs
            .iter()
            .map(|tab| {
                let classes = TabCssClasses::of(&tab.state, container);
                let params = TabSizingParams { current_width: tab.state.current_width, fit_width: tab.state.fit_width, ..params };
                let mut style = tab_box_style(&classes, &params);
                // `.tab.connected-tab-last:not(.connected-tab-upper-row)` (`connectedEditorTabs.css` L568): fora da quebra a linha de
                // cima não reserva o ombro do terminal.
                let upper_row_without_shoulder = connected && !container.wrapping && tab.state.connected_tab_upper_row;
                if tab.state.last_tab && !upper_row_without_shoulder {
                    style.margin_end += extra_margin;
                }

                TabFlexItem { style, content_width: self.natural_width(container, tab, tab.state.tab_action_sizing_rest) }
            })
            .collect();
        let margins: Vec<f64> = items.iter().map(|item| item.style.margin_end).collect();
        let styles: Vec<TabBoxStyle> = items.iter().map(|item| item.style).collect();

        // `.tabs-bar-add-tab { flex: 0 0 auto }` é um filho do `tabs-container` na quebra, com a mesma margem final da última aba.
        // O `tabs.css` soma o `--modern-ui-tab-terminal-gutter` à região do toolbar, também no botão de nova aba.
        let add_margin = if container.wrapping && add_width > 0.0 { container.last_tab_margin_right + self.terminal_gutter(container) } else { 0.0 };
        if container.wrapping && add_width > 0.0 {
            let style = TabBoxStyle {
                width: Some(add_width),
                min_width: CssLength::Auto,
                max_width: CssLength::Auto,
                flex_grow: 0.0,
                flex_shrink: 0.0,
                flex_basis: None,
                padding_left: 0.0,
                padding_right: 0.0,
                margin_end: add_margin,
                position_sticky: false,
            };
            items.push(TabFlexItem { style, content_width: add_width });
        }

        let row = layout_tabs_row(&items, visible_width, container.wrapping);
        let widths: Vec<f64> = row.widths[..tabs.len()].to_vec();

        let row_height = self.row_height(container);
        let mut offsets = Vec::new();
        let mut cursor = 0.0;
        let mut current_line = 0usize;
        for index in 0..tabs.len() {
            // Na quebra cada linha recomeça na borda de início.
            if row.lines[index] != current_line {
                current_line = row.lines[index];
                cursor = 0.0;
            }
            offsets.push(cursor);
            cursor += widths[index] + margins[index];
        }
        let content_width = widths.iter().zip(&margins).map(|(width, margin)| width + margin).sum::<f64>() + if connected { add_width } else { 0.0 };

        let tops: Vec<f64> = (0..tabs.len()).map(|index| row.lines[index] as f64 * row_height).collect();
        let last_line = row.lines.last().copied().unwrap_or(0);
        let (rows, add_top) = if container.wrapping { (last_line + 1, last_line as f64 * row_height) } else { (1usize, 0.0) };

        Geometry { widths, margins, styles, offsets, tops, rows, add_width, add_margin, add_top, visible_width, content_width }
    }

    /// `getBoundingClientRect()` de uma aba: `(left, right)`, com a rolagem, o zoom e a direção do texto.
    fn tab_rect(&self, geometry: &Geometry, index: usize, scroll: f64) -> (f64, f64) {
        let zoom = self.zoom.get();
        let (x, width) = (geometry.offsets[index], geometry.widths[index]);
        let left = if self.rtl.get() { (geometry.visible_width - (x + width) + scroll) * zoom } else { (x - scroll) * zoom };

        (left, left + width * zoom)
    }

    /// `getBoundingClientRect()` do botão de nova aba: `(left, right)`.
    fn add_rect(&self, geometry: &Geometry) -> (f64, f64) {
        let zoom = self.zoom.get();
        if self.rtl.get() { (0.0, geometry.add_width * zoom) } else { ((geometry.visible_width - geometry.add_width) * zoom, geometry.visible_width * zoom) }
    }
}

impl TabsMeasurer for Layout {
    fn measure_fit_tabs(&self, container: &TabsContainerView, tabs: &[Tab], indexes: &[usize]) -> Vec<FitTabMetrics> {
        self.fit_reads.borrow_mut().push(indexes.to_vec());
        indexes
            .iter()
            .map(|&index| {
                let tab = &tabs[index];
                let name = name_chars(tab) * CHAR_WIDTH;
                // O recuo do lado da ação inclui a coluna quando a aba a aloca (o `padding` computado dela).
                let side = TAB_PADDING / 2.0;
                let column = if self.has_action_column(tab, tab.state.tab_action_sizing_rest) { self.action_padding(tab, container) } else { 0.0 };
                let (padding_left, padding_right) = if tab.state.tab_actions_left { (side + column, side) } else { (side, side + column) };
                // O selo fica no `.tab-label` fora do `.monaco-icon-label-container`: é o adorno que `getFitTabWidth` subtrai.
                let badge = if self.has_badge(tab) { BADGE_WIDTH } else { 0.0 };
                FitTabMetrics {
                    width: self.natural_width(container, tab, tab.state.tab_action_sizing_rest),
                    font_size: 13.0,
                    padding_left,
                    padding_right,
                    border_left_width: 0.0,
                    border_right_width: 0.0,
                    action_fit_padding: Some(self.action_column_width()),
                    name_width: Some(name),
                    label_widths: Some((name + badge, name)),
                }
            })
            .collect()
    }

    fn measure_tabs(&self, container: &TabsContainerView, tabs: &[Tab]) -> TabsMetrics {
        let geometry = self.geometry(container, tabs);
        let sticky_tab_widths = tabs.iter().enumerate().filter(|(_, tab)| tab.state.sticky).map(|(index, _)| geometry.widths[index]).collect();

        TabsMetrics {
            visible_tabs_width: geometry.visible_width,
            all_tabs_width: geometry.content_width.max(geometry.visible_width),
            connected_add_tab_width: if self.parent.get().in_connected_editor_tabs { geometry.add_width } else { 0.0 },
            active_tab: tabs.iter().position(|tab| tab.state.active).map(|index| (geometry.offsets[index], geometry.widths[index])),
            sticky_tab_widths,
        }
    }

    fn measure_wrapping(&self, container: &TabsContainerView, tabs: &[Tab]) -> WrappingMeasure {
        let geometry = self.geometry(container, tabs);
        let (tabs_container_height, tabs_and_actions_height) = self.header_heights(container, geometry.rows);

        WrappingMeasure {
            visible_tabs_width: geometry.visible_width,
            all_tabs_width: if container.wrapping { geometry.visible_width } else { geometry.content_width.max(geometry.visible_width) },
            tabs_container_height,
            tabs_and_actions_height,
            tabs_and_actions_content_width: self.width.get(),
            editor_toolbar_width: self.toolbar.get(),
            add_tab_offset_width: geometry.add_width,
            add_tab_total_width: geometry.add_width + geometry.add_margin,
            add_tab_offset_top: geometry.add_top,
            tabs: (0..tabs.len())
                .map(|index| WrapTabMeasure {
                    total_width: geometry.widths[index] + geometry.margins[index],
                    offset_width: geometry.widths[index],
                    offset_top: geometry.tops[index],
                })
                .collect(),
        }
    }

    fn measure_add_tab_alignment(&self, container: &TabsContainerView, tabs: &[Tab]) -> AddTabAlignmentMeasure {
        let geometry = self.geometry(container, tabs);
        let total = self.width.get();

        AddTabAlignmentMeasure {
            add_tab_top: geometry.add_top,
            tabs: (0..tabs.len())
                .map(|index| AlignTabMeasure {
                    top: geometry.tops[index],
                    left: geometry.offsets[index],
                    right: geometry.offsets[index] + geometry.widths[index],
                    padding_left: TAB_PADDING / 2.0,
                    padding_right: TAB_PADDING / 2.0,
                })
                .collect(),
            right_to_left: self.rtl.get(),
            tabs_bounds_width: geometry.visible_width * self.zoom.get(),
            tabs_offset_width: geometry.visible_width,
            add_tab_left: geometry.visible_width - geometry.add_width,
            add_tab_right: geometry.visible_width,
            toolbar_left: total - self.toolbar.get(),
            toolbar_right: total,
            backing_end_inset: None,
        }
    }

    fn measure_connected_active_tab(&self, container: &TabsContainerView, tabs: &[Tab], active_tab_index: usize) -> Option<ConnectedActiveTabMeasure> {
        let geometry = self.geometry(container, tabs);
        let zoom = self.zoom.get();
        let (left, right) = self.tab_rect(&geometry, active_tab_index, container.scroll_left);
        let height = self.tab_height() * zoom;

        Some(ConnectedActiveTabMeasure {
            container_scroll_left: container.scroll_left,
            tabs_left: 0.0,
            tabs_width: geometry.visible_width * zoom,
            fill_left: left,
            fill_right: right,
            fill_top: 0.0,
            fill_bottom: height,
            scrollable_top: 0.0,
            scrollable_bottom: height,
            tab_border: String::new(),
            overflow_top_border: String::new(),
            overflow_top_background: String::new(),
            shoulder_extent: SHOULDER_RADIUS,
        })
    }

    fn measure_connected_clipping(&self, container: &TabsContainerView, tabs: &[Tab], active_tab_index: Option<usize>) -> ConnectedClippingMeasure {
        self.clip_reads.borrow_mut().push(tabs.iter().map(|tab| tab.state.clip_path).collect());

        let geometry = self.geometry(container, tabs);
        let zoom = self.zoom.get();
        let scroll = container.scroll_left;
        let (add_left, add_right) = self.add_rect(&geometry);

        ConnectedClippingMeasure {
            add_tab_offset_width: (container.add_tab != AddTabPresence::None).then_some(geometry.add_width),
            tabs_offset_width: geometry.visible_width,
            tabs_left: 0.0,
            tabs_width: geometry.visible_width * zoom,
            add_tab_left: add_left,
            add_tab_right: add_right,
            right_to_left: self.rtl.get(),
            shoulder_space: geometry.margins.last().copied().unwrap_or(0.0),
            tab_bounds: (0..tabs.len()).map(|index| self.tab_rect(&geometry, index, scroll)).collect(),
            active_tab_left: active_tab_index.map_or(0.0, |index| self.tab_rect(&geometry, index, scroll).0),
        }
    }
}

//#endregion

//#region Fixture (o setup da suíte)

/// O `groupView` do caso sobre o modelo e as opções vigentes.
fn group_host(model: &Rc<EditorGroupModel>, options: &Rc<RefCell<EditorPartOptions>>) -> Rc<dyn TabsControlHost> {
    Rc::new(Host { model: model.clone(), options: options.clone() })
}

/// O modelo do grupo como a visão somente leitura que os controles de abas leem.
fn readonly_model(model: &Rc<EditorGroupModel>) -> Rc<dyn IReadonlyEditorGroupModel> {
    model.clone()
}

/// `IEditorTitleControlDimensions` de `layoutConnectedGroup(group, width)`: `{ container: Dimension(width, 33), available:
/// Dimension(width, 300) }`. Os casos que restringem a altura passam a deles em `available_height`.
fn title_dimensions(width: f64, available_height: f64) -> EditorTitleControlDimensions {
    EditorTitleControlDimensions { container: Some(Dimension { width, height: 33.0 }), available: Some(Dimension { width, height: available_height }) }
}

/// A altura disponível de `layoutConnectedGroup` (`new Dimension(width, 300)`).
const AVAILABLE_HEIGHT: f64 = 300.0;

fn build_state(
    model: &Rc<EditorGroupModel>,
    options: &Rc<RefCell<EditorPartOptions>>,
    parent: TabsParentTraits,
    add_tab: AddTabPresence,
) -> MultiEditorTabsControlState {
    let base = EditorTabsControlBase::new(group_host(model, options), readonly_model(model), parent, false, false);
    let mut state = MultiEditorTabsControlState::new(base);
    state.container.add_tab = add_tab;

    state
}

struct Fixture {
    suite: Suite,
    model: Rc<EditorGroupModel>,
    options: Rc<RefCell<EditorPartOptions>>,
    layout: Rc<Layout>,
    state: MultiEditorTabsControlState,
    /// O `MultiRowEditorControl` dos casos que o criam (`create_multi_row`).
    row: Option<MultiRowEditorControl>,
    parent: TabsParentTraits,
    add_visible: bool,
}

/// O `disposables.add(...)` do controle de duas barras: descartado antes de o `Suite` conferir os vazamentos.
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(row) = &self.row {
            row.dispose();
        }
    }
}

impl Fixture {
    /// O `setup`: opções padrão, o modelo com `file0.txt` (ativo) e `file1.txt`, e o primeiro controle.
    fn new() -> Self {
        let suite = Suite::new();
        let options = Rc::new(RefCell::new(default_editor_part_options()));
        let model = suite.group();
        for index in 0..2 {
            let concrete = suite.disposables.add_rc(NamedInput::new(&format!("file{index}.txt"), &format!("/path/file{index}.txt"), None));
            let editor: EditorInputRef = concrete;
            open!(model, editor, pinned: true, active: index == 0);
        }

        // `container = $('.title.tabs')`: a classe `tabs` existe, o resto vem de `connectedGroup()`.
        let parent = TabsParentTraits { has_tabs_class: true, in_modern_ui_tabs: false, in_connected_editor_tabs: false };
        let layout = Layout::new(options.clone(), parent);
        let state = build_state(&model, &options, parent, AddTabPresence::None);
        let mut fixture = Self { suite, model, options, layout, state, row: None, parent, add_visible: true };
        fixture.open_editors();

        fixture
    }

    /// `connectedGroup()`: o contêiner passa a viver numa raiz `modern-ui-tabs modern-ui-connected-editor-tabs`.
    fn connected_group(&mut self) {
        self.set_parent(TabsParentTraits { has_tabs_class: true, in_modern_ui_tabs: true, in_connected_editor_tabs: true });
    }

    /// `root.classList.toggle('modern-ui-connected-editor-tabs', connected)`.
    fn set_connected(&mut self, connected: bool) {
        self.set_parent(TabsParentTraits { in_connected_editor_tabs: connected, ..self.parent });
    }

    fn set_parent(&mut self, parent: TabsParentTraits) {
        self.parent = parent;
        self.state.base.parent = parent;
        if let Some(row) = &mut self.row {
            row.sticky.base.parent = parent;
            row.unsticky.base.parent = parent;
        }
        self.layout.parent.set(parent);
    }

    /// `tabsControl.dispose()` do controle de duas barras.
    fn dispose_row(&mut self) {
        if let Some(row) = self.row.take() {
            row.dispose();
        }
    }

    /// `control.dispose(); reset(container); control = createControl(menuIds)`.
    fn create_control(&mut self, has_add_tab: bool) {
        self.dispose_row();
        let add_tab = match (has_add_tab, self.add_visible) {
            (false, _) => AddTabPresence::None,
            (true, true) => AddTabPresence::Visible,
            (true, false) => AddTabPresence::Hidden,
        };
        self.state = build_state(&self.model, &self.options, self.parent, add_tab);
        self.layout.clip_reads.borrow_mut().clear();
        self.open_editors();
    }

    /// `control.openEditors(model.getEditors(EditorsOrder.SEQUENTIAL))`.
    fn open_editors(&mut self) {
        self.state.open_editors();
    }

    /// `model.closeEditor(editor); control.closeEditor(editor)`.
    fn close_editor(&mut self, editor: &EditorInputRef) {
        self.model.close_editor(editor, None, None);
        self.state.handle_closed_editors();
    }

    /// `openNamedEditors(names)`.
    fn open_named_editors(&mut self, names: &[String]) -> Vec<EditorInputRef> {
        self.open_named_editors_described(names, Some(("src", "/workspace/project/src")))
    }

    /// O começo de `openNamedEditors`: `model.closeEditor(editor); control.closeEditor(editor)` em cada entrada.
    fn close_all_editors(&mut self) {
        for editor in self.model.get_editors(EditorsOrder::Sequential, None) {
            self.close_editor(&editor);
        }
    }

    /// `openNamedEditors(names)` com as descrições curta e longa que o caso escolhe (`getDescription = () => undefined`).
    fn open_named_editors_described(&mut self, names: &[String], descriptions: Option<(&str, &str)>) -> Vec<EditorInputRef> {
        self.close_all_editors();
        let mut editors = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let concrete = self.suite.disposables.add_rc(NamedInput::new(name, &format!("/workspace/project/src/{name}"), descriptions));
            let editor: EditorInputRef = concrete;
            open!(self.model, editor, index: index as i64, pinned: true, active: index == names.len() - 1);
            editors.push(editor);
        }
        self.open_editors();

        editors
    }

    /// `openNamedEditors(names)` devolvendo as entradas concretas, para os casos que marcam uma entrada como modificada
    /// (`getName()` devolve o nome pedido, não o do recurso).
    fn open_file_editors(&mut self, names: &[String]) -> Vec<Rc<NamedInput>> {
        self.close_all_editors();
        let mut editors = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let concrete = self.suite.disposables.add_rc(NamedInput::new(name, &format!("/workspace/project/src/{name}"), Some(("src", "/workspace/project/src"))));
            let editor: EditorInputRef = concrete.clone();
            open!(self.model, editor, index: index as i64, pinned: true, active: index == names.len() - 1);
            editors.push(concrete);
        }
        self.open_editors();

        editors
    }

    /// `model.moveEditor(editor, to); control.moveEditor(editor, from, to)`.
    fn move_editor(&mut self, editor: &EditorInputRef, from: usize, to: usize) {
        self.model.move_editor(editor, to as i64);
        self.state.move_editor(from, to);
    }

    /// `partOptions = { ...partOptions, ... }`: devolve as opções de antes e de depois (`oldOptions` e `partOptions`).
    fn set_options(&self, change: impl FnOnce(&mut EditorPartOptions)) -> (EditorPartOptions, EditorPartOptions) {
        let old = self.options.borrow().clone();
        let mut new = old.clone();
        change(&mut new);
        *self.options.borrow_mut() = new.clone();

        (old, new)
    }

    /// `partOptions = { ...partOptions, ... }; control.updateOptions(oldOptions, partOptions)`.
    fn update_options(&mut self, change: impl FnOnce(&mut EditorPartOptions)) {
        let (old, new) = self.set_options(change);
        let _ = self.state.update_options(&old, &new);
    }

    /// O quadro de animação: executa o layout agendado com as dimensões guardadas.
    fn frame(&mut self) {
        self.state.run_frame(&*self.layout);
    }

    /// `computeHeight()` com o que o navegador mede.
    fn computed_height(&self) -> f64 {
        self.state.computed_height(&*self.layout)
    }

    /// `control.getHeight()`.
    fn get_height(&self) -> f64 {
        self.state.get_height(self.computed_height())
    }

    /// `row.offsetHeight`.
    fn rendered_height(&self) -> f64 {
        self.layout.measure_wrapping(&self.state.container, &self.state.tabs).tabs_and_actions_height
    }

    /// `layoutConnectedGroup(group, width)`.
    fn layout(&mut self, width: f64) {
        self.layout_with_available(width, AVAILABLE_HEIGHT);
    }

    /// `control.layout({ container: new Dimension(width, 33), available: new Dimension(width, available_height) })` e o quadro seguinte.
    fn layout_with_available(&mut self, width: f64, available_height: f64) {
        self.layout.width.set(width);
        let computed_height = self.computed_height();
        self.state.layout(title_dimensions(width, available_height), None, computed_height);
        self.frame();
    }

    /// `addMenuVisible = visible; addMenuChange.fire()` e o quadro seguinte.
    fn set_add_visible(&mut self, visible: bool) {
        self.add_visible = visible;
        self.state.container.add_tab = if visible { AddTabPresence::Visible } else { AddTabPresence::Hidden };
        self.state.layout_scheduler.schedule(None);
        self.frame();
    }

    /// `tabs.scrollLeft = scrollLeft; tabs.dispatchEvent(new Event('scroll'))`.
    fn scroll_to(&mut self, scroll_left: f64) {
        self.state.handle_tabs_container_scroll(scroll_left, &*self.layout);
    }

    fn geometry(&self) -> Geometry {
        self.layout.geometry(&self.state.container, &self.state.tabs)
    }

    /// `add.offsetWidth`.
    fn add_width(&self) -> f64 {
        self.geometry().add_width
    }

    fn clip_paths(&self) -> Vec<Option<TabClipPath>> {
        self.state.tabs.iter().map(|tab| tab.state.clip_path).collect()
    }

    /// `tabActions()`.
    fn tab_actions(&self) -> Vec<&'static str> {
        self.state
            .tabs
            .iter()
            .map(|tab| match tab.state.action {
                Some(TabAction::CloseOthers) => "closeOthers",
                Some(TabAction::Close) => "close",
                _ => "unknown",
            })
            .collect()
    }

    /// `action.focus()` sobre o botão da aba: o `FOCUS` em captura da aba congela a largura `fit`.
    fn focus_action(&mut self, tab_index: usize) {
        self.state.handle_tab_focus(tab_index, &*self.layout);
    }

    /// `control.updateEditorLabel(editor)`: o pedido que o quadro seguinte executa.
    fn update_editor_label(&mut self) {
        self.state.update_editor_label();
    }

    /// O evento de decorações dos rótulos (`onDidChangeDecorations`) e o quadro seguinte.
    fn decorations_changed(&mut self) {
        let computed_height = self.computed_height();
        self.state.handle_decorations_change(computed_height);
        self.frame();
    }

    /// A coluna da ação (`--modern-ui-tab-action-padding` da aba) quando está visível: a reserva de espaço, uma ação
    /// persistente (suja ou fixada) ou a ação transitória da aba ativa ou sob `:hover` e `:focus-within`.
    fn action_padding(&self, tab_index: usize, transient: bool) -> Option<f64> {
        let tab = &self.state.tabs[tab_index];
        (self.layout.has_action_column(tab, false) || transient || tab.state.active).then(|| self.layout.action_padding(tab, &self.state.container))
    }

    /// `getBoundingClientRect().width` do `.tab-label` da aba.
    fn label_width(&self, tab_index: usize, transient: bool) -> f64 {
        self.layout.label_width(self.geometry().widths[tab_index], self.action_padding(tab_index, transient))
    }

    /// O contexto de geometria da aba `tab_index` com a largura que o layout deu a ela.
    fn box_context(&self, tab_index: usize) -> TabBoxContext {
        self.layout.bar_box_context(&self.state, tab_index)
    }

    /// `tab.querySelector('.action-label')` e `.tab-actions` em coordenadas da aba, e o `.monaco-icon-label-container`.
    fn action_geometry(&self, tab_index: usize) -> TabActionGeometry {
        let classes = TabCssClasses::of(&self.state.tabs[tab_index].state, &self.state.container);
        tab_action_geometry(&classes, &self.box_context(tab_index)).expect("a aba mostra o `.tab-actions`")
    }

    /// `right` do `.monaco-icon-label-container` da aba, com a coluna da ação visível (a aba ativa ou com ação persistente).
    fn label_right(&self, tab_index: usize) -> f64 {
        let tab = &self.state.tabs[tab_index];
        self.layout.label_span(self.geometry().widths[tab_index], self.action_padding(tab_index, false), tab.state.tab_actions_left).1
    }

    /// `hoverTab(tabIndex)`.
    fn hover_tab(&mut self, tab_index: usize) {
        self.state.handle_tab_mouse_enter(tab_index, &*self.layout);
    }

    /// `moveMouseOverTabs(altKey)` e `mouseDownOnTabAction(tabIndex, altKey)`: os dois eventos chegam ao contêiner em captura.
    fn mouse_event_over_tabs(&mut self, alt_key: bool) {
        self.state.handle_tabs_container_mouse_event(alt_key);
    }

    /// `alt(pressed)`.
    fn alt(&mut self, pressed: bool) {
        self.state.set_alt_pressed(pressed);
    }

    /// `control.layout({ container: new Dimension(width, 33), available: new Dimension(width, 300) }, { forceRevealActiveTab: true })`
    /// e o quadro seguinte.
    fn reveal(&mut self, width: f64) {
        self.layout.width.set(width);
        let computed_height = self.computed_height();
        self.state.layout(title_dimensions(width, AVAILABLE_HEIGHT), Some(LayoutOptions { force_reveal_active_tab: true }), computed_height);
        self.frame();
    }

    /// `control.dispose(); reset(container); instantiationService.createInstance(MultiRowEditorControl, container, editorPartsView,
    /// groupsView, groupView, model, menuIds, false, false)` sobre o modelo, as opções e a raiz vigentes: `has_add_tab` é o
    /// `menuIds` com o `tabsBarAddTab` (as duas barras recebem o botão de nova aba, `Visible` como `addMenuVisible` do `setup`).
    /// O controle de uma barra do `setup` fica sem uso (o `Fixture` não tem descarte para ele).
    fn create_multi_row(&mut self, has_add_tab: bool) {
        self.dispose_row();
        let mut row = MultiRowEditorControl::new(group_host(&self.model, &self.options), readonly_model(&self.model), self.parent, false, false);
        if has_add_tab {
            row.sticky.container.add_tab = AddTabPresence::Visible;
            row.unsticky.container.add_tab = AddTabPresence::Visible;
        }
        self.row = Some(row);
    }

    fn row(&self) -> &MultiRowEditorControl {
        self.row.as_ref().expect("o caso cria o controle de duas barras")
    }

    fn row_mut(&mut self) -> &mut MultiRowEditorControl {
        self.row.as_mut().expect("o caso cria o controle de duas barras")
    }

    /// `multiRowControl.openEditors(model.getEditors(EditorsOrder.SEQUENTIAL))`.
    fn row_open_editors(&mut self) {
        self.row_mut().open_editors();
    }

    /// `multiRowControl.updateOptions(oldOptions, partOptions)`.
    fn row_update_options(&mut self, old: &EditorPartOptions, new: &EditorPartOptions) {
        self.row_mut().update_options(old, new);
    }

    /// O quadro de animação das duas barras.
    fn row_frame(&mut self) {
        let row = self.row.as_mut().expect("o caso cria o controle de duas barras");
        row.run_frame(&*self.layout);
    }

    /// `layoutConnectedGroup(group, width, multiRowControl)`.
    fn row_layout(&mut self, width: f64) {
        self.layout.width.set(width);
        let row = self.row.as_mut().expect("o caso cria o controle de duas barras");
        row.layout(title_dimensions(width, AVAILABLE_HEIGHT), &*self.layout);
        self.row_frame();
    }

    /// `multiRowControl.updateEditorDirty(editor)`.
    fn row_update_editor_dirty(&mut self, editor: &EditorInputRef) {
        let row = self.row.as_mut().expect("o caso cria o controle de duas barras");
        row.update_editor_dirty(editor, &*self.layout);
    }

    /// `disposables.add(new TestFileEditorInput(URI.file(path), 'testEditorInput'))` seguido de `model.openEditor(editor, { pinned: true, active })`.
    fn open_pinned_file(&mut self, name: &str, active: bool) -> EditorInputRef {
        let concrete = self.suite.disposables.add_rc(NamedInput::new(name, &format!("/path/{name}"), None));
        let editor: EditorInputRef = concrete;
        open!(self.model, editor, pinned: true, active: active);

        editor
    }

    /// `model.openEditor(model.getEditorByIndex(index), { active: true }); control.openEditors(model.getEditors(EditorsOrder.SEQUENTIAL))`.
    fn activate(&mut self, index: usize) {
        let editor = self.model.get_editor_by_index(index as i64).expect("aba");
        open!(self.model, editor, active: true);
        self.open_editors();
    }

    fn active_index(&self) -> usize {
        self.state.tabs.iter().position(|tab| tab.state.active).expect("aba ativa")
    }

    /// `(left, right)` do `.tab-fill` da aba em coordenadas da tela (`getBoundingClientRect()`), com a rolagem e o zoom.
    fn fill_client_span(&self, tab_index: usize) -> (f64, f64) {
        let zoom = self.layout.zoom.get();
        let (tab_left, _) = self.layout.tab_rect(&self.geometry(), tab_index, self.state.container.scroll_left);
        let fill = tab_fill_rect(&self.box_context(tab_index));

        (tab_left + fill.left * zoom, tab_left + fill.right * zoom)
    }

    /// `action.getBoundingClientRect()` do `.action-label` da aba, em coordenadas da tela: `[x, y, width, height]`, com a
    /// rolagem, o zoom e a direção do texto. Com o `.tab-actions` em `display: none` a caixa é vazia (`[0; 4]`), como no navegador.
    /// A caixa vem da árvore nativa de widgets da aba (`tab_action_geometry`), não de um elemento.
    fn action_bounds(&self, tab_index: usize) -> [f64; 4] {
        let geometry = self.geometry();
        let context = self.box_context(tab_index);
        let classes = TabCssClasses::of(&self.state.tabs[tab_index].state, &self.state.container);
        let Some(action) = tab_action_geometry(&classes, &context) else {
            return [0.0; 4];
        };
        let zoom = self.layout.zoom.get();
        let label = if self.layout.rtl.get() { action.label.mirrored(context.tab_width) } else { action.label };
        let (tab_left, _) = self.layout.tab_rect(&geometry, tab_index, self.state.container.scroll_left);

        [tab_left + label.left * zoom, geometry.tops[tab_index] + label.top * zoom, label.width() * zoom, label.height() * zoom]
    }

    /// `{ tab: tab.getBoundingClientRect(), action: ... .action-label.getBoundingClientRect() }` da aba:
    /// `[left, right, top, action.x, action.y, action.width, action.height]`.
    fn tab_and_action_bounds(&self, tab_index: usize) -> [f64; 7] {
        let geometry = self.geometry();
        let (left, right) = self.layout.tab_rect(&geometry, tab_index, self.state.container.scroll_left);
        let [x, y, width, height] = self.action_bounds(tab_index);

        [left, right, geometry.tops[tab_index], x, y, width, height]
    }

    /// `control.updateEditorSelections()`.
    fn update_editor_selections(&mut self) {
        let computed_height = self.computed_height();
        self.state.update_editor_selections(computed_height);
    }

    /// `model.setSelection(editor, selection)`.
    fn set_selection(&mut self, editor: &EditorInputRef, selection: &[EditorInputRef]) {
        self.model.set_selection(editor, selection);
    }
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

//#endregion

#[test]
fn keeps_connected_layout_current_when_an_add_tab_toolbar_follows_the_editor_tabs() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.closest('.monaco-workbench').classList.remove('modern-ui')`: as classes do estilo conectado ficam.
    let editors = fx.model.get_editors(EditorsOrder::Sequential, None);
    for editor in &editors {
        fx.model.close_editor(editor, None, None);
    }
    fx.create_control(true);
    fx.layout(600.0);
    let empty_height = fx.get_height();
    for (index, editor) in editors.iter().enumerate() {
        open!(fx.model, editor, pinned: true, active: index == 0);
    }
    fx.open_editors();

    #[derive(Debug, PartialEq)]
    struct Row {
        compact: bool,
        width: f64,
        cached_height: f64,
        rendered_height: f64,
        visible_editors: bool,
        terminal_editor: Vec<bool>,
        add_tab_visible: bool,
    }

    let mut results = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        fx.update_options(|options| options.tab_height = tab_height);
        for width in [600.0, 220.0, 600.0] {
            fx.layout(width);
            let geometry = fx.geometry();
            results.push(Row {
                compact: tab_height == TabHeight::Compact,
                width,
                cached_height: fx.get_height(),
                rendered_height: fx.rendered_height(),
                visible_editors: geometry.widths.iter().all(|width| *width > 0.0) && fx.state.tabs.iter().all(|tab| !tab.state.connected_clipping.hidden),
                terminal_editor: fx.state.tabs.iter().map(|tab| tab.state.connected_tab_last).collect(),
                add_tab_visible: fx.state.container.add_tab == AddTabPresence::Visible,
            });
        }
    }

    assert_eq!(empty_height, 0.0);
    let expected: Vec<Row> = [false, true]
        .into_iter()
        .flat_map(|compact| {
            [600.0, 220.0, 600.0].into_iter().map(move |width| Row {
                compact,
                width,
                cached_height: if compact { 28.0 } else { 32.0 },
                rendered_height: if compact { 28.0 } else { 32.0 },
                visible_editors: true,
                terminal_editor: vec![false, true],
                add_tab_visible: true,
            })
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn add_tab_spacing_matches_adjacent_tab_fill_insets_without_losing_the_terminal_shoulder() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.create_control(true);

    #[derive(Debug, PartialEq)]
    struct Row {
        active_index: usize,
        rtl: bool,
        width: f64,
        gap: f64,
        expected_gap: f64,
        terminal_margin: f64,
        opposite_margin: f64,
        terminal_is_last: bool,
    }

    let mut results = Vec::new();
    for active_index in [0usize, 1] {
        for rtl in [false, true] {
            fx.layout.rtl.set(rtl);
            let editor = fx.model.get_editor_by_index(active_index as i64).expect("editor");
            fx.model.set_active(Some(&editor));
            fx.state.set_active(); // control.setActive(true)
            fx.open_editors(); // control.openEditor(model.activeEditor)
            for width in [500.0, 300.0, 500.0] {
                fx.layout(width);
                let geometry = fx.geometry();
                let last = fx.state.tabs.len() - 1;
                let zoom = fx.layout.zoom.get();
                let (left, right) = fx.layout.tab_rect(&geometry, last, 0.0);
                // `.tab-fill` com `inset-inline` de `--modern-ui-connected-tab-fill-inline-inset`: 0 na aba ativa, `size20` nas demais.
                let fill_inset = if fx.state.tabs[last].state.active { 0.0 } else { SPACING_SIZE20 } * zoom;
                let (fill_left, fill_right) = (left + fill_inset, right - fill_inset);
                let button_edge = fx.layout.add_button_near_edge((left, right), geometry.margins[last]);
                let (gap, expected_inset) = if rtl { (fill_left - button_edge, fill_left - left) } else { (button_edge - fill_right, right - fill_right) };
                results.push(Row {
                    active_index,
                    rtl,
                    width,
                    gap,
                    expected_gap: expected_inset + SPACING_SIZE20 * zoom,
                    terminal_margin: geometry.margins[last],
                    opposite_margin: geometry.margins[0],
                    terminal_is_last: fx.state.tabs[last].state.connected_tab_last,
                });
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row { gap: result.expected_gap, terminal_margin: 8.0, opposite_margin: 0.0, terminal_is_last: true, ..*result })
        .collect();
    assert_eq!(results, expected);
}

// A parte de `Add Tab spacing` que confere o `::before` do botão (largura igual à do botão, raio de 4px, contêiner sem fundo)
// é pintura do botão nativo e não é estado do controle.

/// A leitura de `readClipping(direction, zoom)`.
#[derive(Debug, PartialEq)]
struct ClippingRead {
    has_clipped_tab: bool,
    has_clipped_active: bool,
    rtl_cap_at_boundary: bool,
    opposite_shoulder_present_when_visible: bool,
    matching_clip: bool,
}

/// `readClipping(direction, zoom)`. O `overlay` é o `ConnectedOverflowEdge` do contêiner e o ombro oposto da aba ativa é o
/// que `connectedEditorTabs.css` deixa existir: o `::before` (início) some na primeira aba e quando o controle marca
/// `connected-tab-left-edge`; o `::after` (fim) não tem regra que o suprima.
fn read_clipping(fx: &Fixture, rtl: bool, zoom: f64) -> ClippingRead {
    let geometry = fx.geometry();
    let scroll = fx.state.container.scroll_left;
    let (add_left, add_right) = fx.layout.add_rect(&geometry);
    let boundary = if rtl { add_right + 8.0 * zoom } else { add_left - 8.0 * zoom };
    let rects: Vec<(f64, f64)> = (0..fx.state.tabs.len()).map(|index| fx.layout.tab_rect(&geometry, index, scroll)).collect();
    let clipped = |(left, right): (f64, f64)| if rtl { left < boundary } else { right > boundary };

    let tabs_right = geometry.visible_width * zoom;
    let active_index = fx.state.tabs.iter().position(|tab| tab.state.active).expect("aba ativa");
    let (fill_left, fill_right) = rects[active_index];
    let shoulder_width = SHOULDER_RADIUS * zoom;
    let active_visible_width = fill_right.min(tabs_right) - fill_left.max(boundary);
    let rtl_cap_expected = rtl && active_visible_width > shoulder_width;
    let overlay = &fx.state.container.overflow_edge;
    let overlay_left = overlay.left.unwrap_or(0.0) * zoom;
    let opposite_shoulder_outside_viewport = if rtl { fill_right + shoulder_width > tabs_right } else { fill_left - shoulder_width < 0.0 };
    let opposite_shoulder_exists = rtl || !(active_index == 0 || fx.state.tabs[active_index].state.connected_clipping.left_edge);

    ClippingRead {
        has_clipped_tab: rects.iter().any(|rect| clipped(*rect)),
        has_clipped_active: fx.state.tabs.iter().zip(&rects).any(|(tab, rect)| tab.state.active && clipped(*rect)),
        rtl_cap_at_boundary: !rtl || if rtl_cap_expected { (overlay_left - boundary).abs() < 1.0 / 32.0 } else { !overlay.left_clipped },
        opposite_shoulder_present_when_visible: opposite_shoulder_outside_viewport || opposite_shoulder_exists,
        matching_clip: fx.state.tabs.iter().zip(&rects).all(|(tab, &(left, right))| {
            let expected = ((if rtl { boundary - left } else { right - boundary }) / zoom).max(0.0);
            let clip = tab.state.clip_path;
            if expected < 1.0 / 32.0 {
                clip.is_none_or(|clip| clip.clipped < 1.0 / 32.0)
            } else {
                clip.is_some_and(|clip| (clip.clipped - expected).abs() < 1.0 / 32.0 && clip.right_to_left == rtl)
            }
        }),
    }
}

#[test]
fn scrolling_tab_content_is_clipped_before_the_rounded_sticky_add_control() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.create_control(true);
    let editors = fx.open_named_editors(&names(&[
        "F.ts",
        "a-long-inactive-label-that-scrolls-beneath-the-Add-button.ts",
        "another-long-inactive-label-that-scrolls-beneath-the-Add-button.ts",
        "last.ts",
    ]));
    open!(fx.model, editors[1], active: true);
    fx.open_editors();
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fit;
        options.wrap_tabs = false;
        options.tab_action_reserve_space = true;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.layout(300.0);

    let mut results = Vec::new();
    for rtl in [false, true] {
        for zoom in [1.0, 1.25] {
            fx.layout.rtl.set(rtl);
            fx.layout.zoom.set(zoom);
            fx.layout(300.0);
            fx.state.container.scroll_mode = true; // tabs.classList.add('scroll')
            fx.scroll_to(0.0);
            let resting = read_clipping(&fx, rtl, zoom);
            fx.scroll_to(90.0);
            results.push((rtl, zoom, resting, read_clipping(&fx, rtl, zoom)));
        }
    }

    fx.set_add_visible(false);
    let hidden_add_clipping_cleared = fx.clip_paths().iter().all(Option::is_none);
    fx.set_add_visible(true);
    fx.update_options(|options| options.wrap_tabs = true);
    fx.layout(300.0);
    let wrapped_clipping_cleared = fx.clip_paths().iter().all(Option::is_none);

    let expected_read = || ClippingRead {
        has_clipped_tab: true,
        has_clipped_active: true,
        rtl_cap_at_boundary: true,
        opposite_shoulder_present_when_visible: true,
        matching_clip: true,
    };
    let expected: Vec<(bool, f64, ClippingRead, ClippingRead)> =
        results.iter().map(|(rtl, zoom, _, _)| (*rtl, *zoom, expected_read(), expected_read())).collect();
    assert_eq!(results, expected);
    assert!(hidden_add_clipping_cleared);
    assert!(wrapped_clipping_cleared);
}

#[test]
fn scrolling_add_clipping_reads_every_tab_before_changing_clip_styles() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.create_control(true);
    let long_names: Vec<String> = (0..40).map(|index| format!("long-filename-{index}-for-scrolling.ts")).collect();
    fx.open_named_editors(&long_names);
    fx.layout(300.0);
    fx.state.container.scroll_mode = true; // scrollContainer.classList.add('scroll')

    let before = fx.clip_paths();
    fx.layout.clip_reads.borrow_mut().clear();
    fx.scroll_to(90.0);

    // Cada atualização lê as bordas de TODAS as abas numa única chamada, antes de qualquer escrita: o `clipPath` visto na
    // leitura é o de antes da atualização, e a leitura devolve uma medida por aba.
    let reads = fx.layout.clip_reads.borrow().clone();
    let all_reads_before_writes = !reads.is_empty() && reads.iter().all(|snapshot| *snapshot == before);
    let read_all_tabs = reads.iter().all(|snapshot| snapshot.len() == fx.state.tabs.len());
    assert_eq!(
        (read_all_tabs, all_reads_before_writes, fx.clip_paths() != before),
        (true, true, true),
    );
}

#[test]
fn add_menu_visibility_updates_wrapped_width_and_scrolling_clipping_without_resize_or_scroll() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.create_control(true);
    let long_label = format!("a-{}.ts", "long-label-".repeat(25));
    let editors = fx.open_named_editors(&["F.ts".to_string(), long_label, "last.ts".to_string()]);
    open!(fx.model, editors[1], active: true);
    fx.open_editors();

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        rtl: bool,
        wrap_tabs: bool,
        visible: bool,
        hidden: bool,
        width_current: bool,
        clipping_current: bool,
    }

    let mut results = Vec::new();
    for connected in [false, true] {
        fx.set_connected(connected);
        for rtl in [false, true] {
            fx.layout.rtl.set(rtl);
            for wrap_tabs in [false, true] {
                fx.update_options(|options| {
                    options.wrap_tabs = wrap_tabs;
                    options.tab_sizing = TabSizing::Fit;
                    options.tab_action_reserve_space = false;
                });
                fx.layout(300.0);
                for visible in [false, true, false, true] {
                    fx.set_add_visible(visible);
                    results.push(Row {
                        connected,
                        rtl,
                        wrap_tabs,
                        visible,
                        hidden: fx.state.container.add_tab == AddTabPresence::Hidden,
                        width_current: fx.state.container.last_tab_add_tab_width == fx.add_width(),
                        clipping_current: fx.clip_paths().iter().any(Option::is_some) == (connected && !wrap_tabs && visible),
                    });
                }
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            connected: result.connected,
            rtl: result.rtl,
            wrap_tabs: result.wrap_tabs,
            visible: result.visible,
            hidden: !result.visible,
            width_current: true,
            clipping_current: true,
        })
        .collect();
    assert_eq!(results, expected);
}

/// `openNamedEditors(['F.ts', `a-${'long-label-'.repeat(25)}.ts`])`.
fn open_short_and_oversized(fx: &mut Fixture) {
    let oversized = format!("a-{}.ts", "long-label-".repeat(25));
    fx.open_named_editors(&["F.ts".to_string(), oversized]);
}

/// `partOptions = { ...partOptions, wrapTabs: true, tabSizing: 'fit', tabActionReserveSpace: false }`.
fn wrap_fit_without_reserved_actions(fx: &mut Fixture) {
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fit;
        options.tab_action_reserve_space = false;
    });
}

#[test]
fn wrapped_fit_check_includes_the_terminal_gutter_and_the_independent_controls_row() {
    let mut fx = Fixture::new();
    fx.connected_group();

    #[derive(Debug, PartialEq)]
    struct Row {
        has_add: bool,
        connected: bool,
        rtl: bool,
        delta: f64,
        wrapped: bool,
    }

    let mut results = Vec::new();
    for has_add in [false, true] {
        fx.create_control(has_add);
        open_short_and_oversized(&mut fx);
        wrap_fit_without_reserved_actions(&mut fx);
        fx.layout.toolbar.set(56.0); // toolbar.style.width = '56px'
        for connected in [false, true] {
            fx.set_connected(connected);
            for rtl in [false, true] {
                fx.layout.rtl.set(rtl);
                fx.layout(300.0);
                let geometry = fx.geometry();
                let toolbar = fx.layout.toolbar.get();
                let add_width = if has_add { geometry.add_width } else { 0.0 };
                let terminal_margin = *geometry.margins.last().expect("aba terminal");
                let gutter = terminal_margin - if has_add { 0.0 } else { toolbar };
                let threshold = if has_add { 80f64.max(toolbar + add_width) } else { 80.0 + toolbar } + gutter;
                for delta in [-2.0, 2.0, -2.0, 2.0] {
                    fx.layout(threshold + delta);
                    results.push(Row { has_add, connected, rtl, delta, wrapped: fx.state.container.wrapping });
                }
            }
        }
    }

    let expected: Vec<Row> = results.iter().map(|result| Row { wrapped: result.delta > 0.0, ..*result }).collect();
    assert_eq!(results, expected);
}

#[test]
fn wrapped_add_tab_stays_unobscured_beside_the_terminal_tab_or_on_its_own_final_row() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.create_control(true);
    open_short_and_oversized(&mut fx);
    wrap_fit_without_reserved_actions(&mut fx);
    fx.layout.toolbar.set(56.0);

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        rtl: bool,
        compact: bool,
        width: f64,
        wrapped: bool,
        same_row: bool,
        adjacent_gap: Option<bool>,
        separate_row: Option<bool>,
        contained: bool,
        visible_button_gap: Option<bool>,
    }

    let mut results = Vec::new();
    for connected in [false, true] {
        for rtl in [false, true] {
            for tab_height in [TabHeight::Default, TabHeight::Compact] {
                fx.update_options(|options| options.tab_height = tab_height);
                fx.layout.rtl.set(rtl);
                fx.set_connected(connected);
                for width in [320.0, 900.0, 320.0, 1500.0] {
                    fx.layout(width);
                    let geometry = fx.geometry();
                    let last = fx.state.tabs.len() - 1;
                    let terminal = &fx.state.tabs[last];
                    let (left, right) = fx.layout.tab_rect(&geometry, last, 0.0);
                    let same_row = geometry.tops[last] == geometry.add_top;
                    // O botão de nova aba na quebra: depois da margem do terminal na mesma linha, ou no início da linha final.
                    let add_width = geometry.add_width;
                    let margin = geometry.margins[last];
                    let (add_left, add_right) = match (same_row, rtl) {
                        (true, false) => (right + margin, right + margin + add_width),
                        (true, true) => (left - margin - add_width, left - margin),
                        (false, false) => (0.0, add_width),
                        (false, true) => (geometry.visible_width - add_width, geometry.visible_width),
                    };
                    // `.tab-fill` com `inset-inline` 0 na aba ativa fora da linha superior e `size20` nas demais.
                    let fill_inset = if connected && terminal.state.active && !terminal.state.connected_tab_top_row { 0.0 } else { SPACING_SIZE20 };
                    // O `.action-label` recua `size20 - shoulder-radius` no estilo conectado da quebra.
                    let label_start = SPACING_SIZE20 - SHOULDER_RADIUS;
                    let (fill_left, fill_right) = (left + fill_inset, right - fill_inset);
                    let button_gap = if rtl { fill_left - (add_right - label_start) } else { (add_left + label_start) - fill_right };
                    let adjacent_gap = (if rtl { left - add_right } else { add_left - right } - if connected { 7.0 } else { 4.0 }).abs() <= 1.0 / 32.0;
                    results.push(Row {
                        connected,
                        rtl,
                        compact: tab_height == TabHeight::Compact,
                        width,
                        wrapped: fx.state.container.wrapping,
                        same_row,
                        adjacent_gap: same_row.then_some(adjacent_gap),
                        separate_row: (!same_row).then(|| geometry.add_top > geometry.tops[last]),
                        contained: add_left >= -1.0 / 64.0 && add_right <= fx.layout.width.get() + 1.0 / 64.0,
                        visible_button_gap: same_row.then(|| !connected || (button_gap - fill_inset - SPACING_SIZE20).abs() <= 1.0 / 32.0),
                    });
                }
            }
        }
    }

    // O alinhamento vertical do botão com o centro do toolbar (`toolbarAligned`) é centralização de linha flex do navegador,
    // sem estado no controle: o que o controle decide aqui é a linha do botão (`add_top`), conferida em `same_row` e
    // `separate_row`.
    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            wrapped: true,
            adjacent_gap: result.same_row.then_some(true),
            separate_row: (!result.same_row).then_some(true),
            contained: true,
            visible_button_gap: result.same_row.then_some(true),
            ..*result
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn wrapped_terminal_fit_tabs_leave_room_for_editor_actions_without_an_add_tab_control() {
    let mut fx = Fixture::new();
    fx.connected_group();
    open_short_and_oversized(&mut fx);
    wrap_fit_without_reserved_actions(&mut fx);
    fx.layout.toolbar.set(56.0);

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        rtl: bool,
        width: f64,
        wrapped: bool,
        before_toolbar: bool,
        close_clickable: bool,
        no_add_tab: bool,
    }

    let mut results = Vec::new();
    for connected in [false, true] {
        for rtl in [false, true] {
            fx.layout.rtl.set(rtl);
            fx.set_connected(connected);
            for width in [300.0, 600.0, 300.0] {
                fx.layout(width);
                let geometry = fx.geometry();
                let last = fx.state.tabs.len() - 1;
                let (left, right) = fx.layout.tab_rect(&geometry, last, 0.0);
                // O `.action-label` fecha a aba na borda de fim, dentro do preenchimento da aba.
                let close_width = ACTION_COLUMN - TAB_PADDING / 2.0;
                let (close_left, close_right) = if rtl { (left + TAB_PADDING / 2.0, left + TAB_PADDING / 2.0 + close_width) } else { (right - TAB_PADDING / 2.0 - close_width, right - TAB_PADDING / 2.0) };
                // O toolbar ocupa a borda de fim da linha: `(total - toolbar, total)` no texto da esquerda, `(0, toolbar)` no inverso.
                let toolbar = fx.layout.toolbar.get();
                let (toolbar_left, toolbar_right) = if rtl { (0.0, toolbar) } else { (width - toolbar, width) };
                let center = (close_left + close_right) / 2.0;
                results.push(Row {
                    connected,
                    rtl,
                    width,
                    wrapped: fx.state.container.wrapping,
                    before_toolbar: if rtl { close_left >= toolbar_right } else { close_right <= toolbar_left },
                    close_clickable: !(center >= toolbar_left && center <= toolbar_right),
                    no_add_tab: fx.state.container.add_tab == AddTabPresence::None,
                });
            }
        }
    }

    let expected: Vec<Row> = results.iter().map(|result| Row { wrapped: true, before_toolbar: true, close_clickable: true, no_add_tab: true, ..*result }).collect();
    assert_eq!(results, expected);
}

#[test]
fn modern_editor_headers_retain_an_opaque_backing_during_narrow_and_wide_layouts() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.style.setProperty('--vscode-editor-background', '#123456')` e as duas cores de cabeçalho em `rgba(...)`: o que
    // `var(...)` resolve para o `getComputedStyle`.
    let (editor_background, tabs_background, no_tabs_background) = ("rgb(18, 52, 86)", "rgba(255, 0, 0, 0.25)", "rgba(0, 255, 0, 0.25)");

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        width: f64,
        show_tabs: bool,
        backing: ModernHeaderBacking,
    }

    let mut results = Vec::new();
    for connected in [false, true] {
        fx.set_connected(connected);
        for (width, show_tabs) in [500.0, 180.0, 500.0].into_iter().flat_map(|width| [false, true].map(|show_tabs| (width, show_tabs))) {
            // `container.classList.toggle('tabs', showTabs)`
            fx.set_parent(TabsParentTraits { has_tabs_class: show_tabs, ..fx.parent });
            fx.layout(width);
            results.push(Row {
                connected,
                width,
                show_tabs,
                backing: modern_header_backing(editor_background, tabs_background, no_tabs_background, fx.parent.in_connected_editor_tabs, fx.parent.has_tabs_class),
            });
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            backing: ModernHeaderBacking {
                backing: "rgb(18, 52, 86)".to_string(),
                theme_layer: if result.show_tabs {
                    if result.connected {
                        "none".to_string()
                    } else {
                        "linear-gradient(rgba(255, 0, 0, 0.25), rgba(255, 0, 0, 0.25))".to_string()
                    }
                } else {
                    "linear-gradient(rgba(0, 255, 0, 0.25), rgba(0, 255, 0, 0.25))".to_string()
                },
                connected_row_theme_layer: (result.connected && result.show_tabs).then(|| "rgba(255, 0, 0, 0.25)".to_string()),
            },
            ..*result
        })
        .collect();
    assert_eq!(results, expected);
}

/// O `workbenchInstantiationService` que `createEditorPart` usa, com o tema do caso: a configuração, o armazenamento, o
/// `IContextKeyService`, os serviços que o grupo lê na hora de usar e o `IThemeService` (o `TestThemeService`).
fn create_header_part(suite: &Suite, theme: &Rc<TestThemeService>) -> Rc<TestEditorPart> {
    let configuration = TestConfigurationService::new(None);
    let workbench_services = WorkbenchServices::new(&suite.disposables, &configuration);
    let files_configuration_service: Rc<dyn IFilesConfigurationService> = workbench_services.files_configuration_service.clone();
    let configuration_service: Rc<dyn IConfigurationService> = configuration;
    let storage = InMemoryStorageService::new();
    let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::scopable();
    let theme_service: Rc<dyn IThemeService> = theme.clone();

    let services = ServiceCollection::new()
        .with(I_FILES_CONFIGURATION_SERVICE, files_configuration_service)
        .with(I_URI_IDENTITY_SERVICE, uri_identity_service())
        .with(I_CONFIGURATION_SERVICE, configuration_service)
        .with(I_STORAGE_SERVICE, storage_service)
        .with(I_CONTEXT_KEY_SERVICE, context_key_service)
        .with(I_THEME_SERVICE, theme_service);
    let instantiation_service = InstantiationService::new(services.clone());
    suite.disposables.add_rc(storage);
    suite.disposables.add_rc(instantiation_service.clone());
    instantiation_service.invoke_function(|accessor| editor_factory_registry().start(accessor));

    create_editor_part(&Ui::new(), &instantiation_service, &suite.disposables.store())
}

/// `part.activeGroup` como a vista do grupo (`root.querySelector('.editor-group-container')`).
fn active_group_view(part: &TestEditorPart) -> Rc<EditorGroupView> {
    let group: Rc<dyn IEditorGroup> = part.active_group();
    match group.as_any_rc().and_then(|any| any.downcast::<EditorGroupView>().ok()) {
        Some(view) => view,
        None => panic!("o grupo ativo da parte é um EditorGroupView"),
    }
}

#[test]
fn editor_group_owns_the_header_backing_while_modern_ui_can_toggle_without_an_important_override() {
    let suite = Suite::new();
    let theme = TestThemeService::new(TestColorTheme::new(&[
        ("editor.background", "#123456"),
        ("editorGroupHeader.tabsBackground", "#44556680"),
        ("editorGroupHeader.noTabsBackground", "#66778880"),
    ]));
    let part = create_header_part(&suite, &theme);
    let group = active_group_view(&part);
    // `root.style.setProperty('--vscode-editor-background', '#123456')`: o que `tabs.css` resolve com `.modern-ui-tabs`.
    let modern_title_background = "rgb(18, 52, 86)";

    #[derive(Debug, PartialEq)]
    struct Row {
        show_tabs: &'static str,
        modern: bool,
        owning_inline_color: String,
        background: String,
        backing_priority: &'static str,
    }

    let mut results = Vec::new();
    for show_tabs in ["multiple", "single", "none"] {
        let options = Dynamic::Object(ObjectRef::from_entries(vec![("showTabs", Dynamic::string(show_tabs))]));
        let override_options = suite.disposables.add(part.enforce_part_options(options));
        for modern in [false, true, false] {
            // `root.classList.toggle('modern-ui-tabs', modern)`
            let title = group.title_container_style();
            results.push(Row {
                show_tabs,
                modern,
                owning_inline_color: title.background_color.clone(),
                background: group.computed_title_background(modern.then_some(modern_title_background)),
                backing_priority: if title.background_color.contains("!important") { "important" } else { "" },
            });
        }
        override_options.dispose();
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| {
            let header = if result.show_tabs == "multiple" { "rgba(68, 85, 102, 0.5)" } else { "rgba(102, 119, 136, 0.5)" };
            Row {
                show_tabs: result.show_tabs,
                modern: result.modern,
                owning_inline_color: format!("var(--modern-ui-editor-title-background, {header})"),
                background: if result.modern { "rgb(18, 52, 86)".to_string() } else { header.to_string() },
                backing_priority: "",
            }
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn connected_labels_use_end_ellipsis_before_the_badge_and_action() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 20.0;
        options.tab_sizing_fixed_max_width = 20.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
        options.has_icons = true;
        options.show_tab_index = true;
    });
    fx.layout(200.0);
    // Os rótulos de recurso são redesenhados na entrada do modo conectado.
    fx.layout(200.0);

    let summary = |fx: &Fixture| {
        let tab = &fx.state.tabs[0];
        let label = &tab.state.label;
        let name = format!("{}{}", label.name_prefix.as_deref().unwrap_or(""), label.name.as_deref().unwrap_or(""));
        (name, tab.state.has_icon && !label.hide_icon, label.description.clone())
    };

    let (name, icon_visible, suffix) = summary(&fx);
    let geometry = fx.geometry();
    let truncated = name.chars().count() as f64 * CHAR_WIDTH > geometry.widths[0];
    assert_eq!((name.as_str(), icon_visible, suffix.as_str(), truncated), ("1: file0.txt", true, "", true));
    assert!(fx.state.tabs[0].state.aria_label.as_deref().is_some_and(|label| label.contains("file0.txt")));
    // O `text-overflow: ellipsis`, o `::after` do selo e a ordem do rótulo antes da ação são pintura da renderização nativa.

    fx.set_connected(false);
    fx.layout(200.0);
    let (name, icon_visible, _) = summary(&fx);
    assert_eq!((name.as_str(), icon_visible), ("1: file0.txt", true));

    fx.set_parent(TabsParentTraits { in_modern_ui_tabs: false, ..fx.parent });
    fx.layout(200.0);
    let (name, icon_visible, _) = summary(&fx);
    assert_eq!((name.as_str(), icon_visible), ("1: file0.txt", true));
}

#[test]
fn modern_tabs_preserve_file_icons_across_styles_sizing_modes_and_wrapping() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let editor_names: Vec<String> = (0..8).map(|index| format!("filename-for-layout-{index}.ts")).collect();
    fx.open_named_editors(&editor_names);

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        tab_sizing: TabSizing,
        wrap_tabs: bool,
        width: f64,
        all_icons_visible: bool,
    }

    let mut results = Vec::new();
    for connected in [false, true] {
        fx.set_connected(connected);
        for tab_sizing in [TabSizing::Fit, TabSizing::Fixed, TabSizing::Shrink] {
            for wrap_tabs in [false, true] {
                for width in [420.0, 1200.0] {
                    fx.update_options(|options| {
                        options.tab_sizing = tab_sizing;
                        options.wrap_tabs = wrap_tabs;
                        options.tab_sizing_fixed_min_width = 80.0;
                        options.tab_sizing_fixed_max_width = 130.0;
                        options.editor_actions_location = EditorActionsLocation::Hidden;
                        options.has_icons = true;
                    });
                    fx.layout(width);
                    results.push(Row {
                        connected,
                        tab_sizing,
                        wrap_tabs,
                        width,
                        all_icons_visible: fx.state.tabs.iter().all(|tab| tab.state.has_icon && !tab.state.label.hide_icon),
                    });
                }
            }
        }
    }

    let expected: Vec<Row> = results.iter().map(|result| Row { all_icons_visible: true, ..*result }).collect();
    assert_eq!(results, expected);
}

#[test]
fn connected_shrink_tabs_preserve_icons_as_the_editor_width_changes() {
    let mut fx = Fixture::new();
    fx.connected_group();
    for index in 2..10 {
        let concrete = fx.suite.disposables.add_rc(NamedInput::new(&format!("file{index}.txt"), &format!("/path/file{index}.txt"), None));
        let editor: EditorInputRef = concrete;
        open!(fx.model, editor, pinned: true, active: false);
    }
    fx.open_editors();
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Shrink;
        options.has_icons = true;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });

    #[derive(Debug, PartialEq)]
    struct State {
        visible_icons: usize,
        trimmed_names: usize,
        full_names: bool,
    }

    let mut states = Vec::new();
    for width in [1200.0, 420.0, 1200.0, 420.0] {
        fx.layout(width);
        let geometry = fx.geometry();
        let tabs = &fx.state.tabs;
        states.push(State {
            visible_icons: tabs.iter().filter(|tab| tab.state.has_icon && !tab.state.label.hide_icon).count(),
            // O nome é cortado (`scrollWidth > clientWidth`) quando a aba fica mais estreita que o conteúdo.
            trimmed_names: tabs
                .iter()
                .enumerate()
                .filter(|(index, tab)| fx.layout.natural_width(&fx.state.container, tab, tab.state.tab_action_sizing_rest) > geometry.widths[*index])
                .count(),
            full_names: tabs.iter().all(|tab| tab.state.label.name.as_deref().is_some_and(|name| name.ends_with(".txt"))),
        });
    }

    assert_eq!(
        states,
        vec![
            State { visible_icons: 10, trimmed_names: 0, full_names: true },
            State { visible_icons: 10, trimmed_names: 10, full_names: true },
            State { visible_icons: 10, trimmed_names: 0, full_names: true },
            State { visible_icons: 10, trimmed_names: 10, full_names: true },
        ]
    );
}

/// `tabs.map(tab => getComputedStyle(tab.querySelector('.action-label')).opacity)`: a cascata de opacidade de cada aba, com o
/// estado de classes e foco que `adjust` acrescenta (o que o caso escreve direto no elemento: `classList.add('dirty')`,
/// `focus()`). A janela está em foco, então `:focus` vale.
fn tab_opacities(fx: &Fixture, group_active: bool, high_contrast: bool, adjust: impl Fn(usize, &mut TabActionLabelContext)) -> Vec<f64> {
    fx.state
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            let state = &tab.state;
            let mut context = TabActionLabelContext {
                modern_ui_tabs: fx.parent.in_modern_ui_tabs,
                connected_editor_tabs: fx.parent.in_connected_editor_tabs,
                high_contrast,
                group_active,
                title_reserves_action_space: fx.state.base.parent_style.tab_actions_reserve_space,
                active: state.active,
                dirty: state.dirty,
                sticky: state.sticky,
                pinned_action_off: state.pinned_action_off,
                close_action_off: state.close_action_off,
                ..TabActionLabelContext::default()
            };
            adjust(index, &mut context);

            tab_action_label_opacity(&context)
        })
        .collect()
}

#[test]
fn connected_actions_keep_active_and_dirty_visible_but_inactive_clean_quiet() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `container.classList.add('tab-actions-reserve-space')`
    fx.state.base.parent_style.tab_actions_reserve_space = true;
    fx.layout(400.0);

    let clean = tab_opacities(&fx, true, false, |_, _| {});
    // `tabs[1].classList.add('dirty')`
    let dirty = tab_opacities(&fx, true, false, |index, context| context.dirty |= index == 1);
    // `action.tabIndex = 0; action.focus()` (a janela de teste está em foco, então `:focus` e `:focus-within` valem)
    let focused = tab_opacities(&fx, true, false, |index, context| {
        if index == 1 {
            context.label_focused = true;
            context.focus_within = true;
        }
    });

    assert_eq!((clean, dirty, focused), (vec![1.0, 0.0], vec![1.0, 1.0], vec![1.0, 1.0]));
}

#[test]
fn action_reservation_does_not_make_inactive_clean_close_buttons_visible() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/a-naturally-wide-modified-editor.ts"), "testEditorInput"));
    concrete.set_dirty();
    let modified: EditorInputRef = concrete;
    open!(fx.model, modified, pinned: true, active: false);
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/a-naturally-wide-sticky-editor.ts"), "testEditorInput"));
    let sticky: EditorInputRef = concrete;
    open!(fx.model, sticky, pinned: true, active: false);
    fx.model.stick(&sticky);
    fx.open_editors();

    #[derive(Debug, PartialEq)]
    enum Measurement {
        AtRest { style: &'static str, theme: &'static str, tab_action_reserve_space: bool, active_group: bool, at_rest: Vec<bool> },
        Focus { style: &'static str, theme: &'static str, tab_action_reserve_space: bool, focused: bool, keyboard_focus: bool },
    }

    let mut measurements = Vec::new();
    let mut expected = Vec::new();
    for style in ["legacy", "pill", "connected"] {
        // `root.classList.toggle('modern-ui-tabs', style !== 'legacy')` e `modern-ui-connected-editor-tabs`
        fx.set_parent(TabsParentTraits { has_tabs_class: true, in_modern_ui_tabs: style != "legacy", in_connected_editor_tabs: style == "connected" });
        for theme in ["vs", "vs-dark", "hc-black", "hc-light"] {
            let high_contrast = theme.starts_with("hc");
            for tab_action_reserve_space in [false, true] {
                fx.update_options(|options| {
                    options.tab_action_reserve_space = tab_action_reserve_space;
                    options.pinned_tab_sizing = PinnedTabSizing::Normal;
                    options.tab_action_unpin_visibility = true;
                });
                fx.layout(600.0);
                let tabs = &fx.state.tabs;
                let active = tabs.iter().position(|tab| tab.state.active).expect("aba ativa");
                let clean = tabs.iter().position(|tab| !tab.state.active && !tab.state.dirty && !tab.state.sticky).expect("aba limpa");
                let dirty = tabs.iter().position(|tab| tab.state.dirty).expect("aba suja");
                let pinned = tabs.iter().position(|tab| tab.state.sticky).expect("aba fixada");
                let visible = |opacities: &[f64], index: usize| opacities[index] > 0.0;
                for active_group in [true, false] {
                    let opacities = tab_opacities(&fx, active_group, high_contrast, |_, context| context.title_reserves_action_space = tab_action_reserve_space);
                    measurements.push(Measurement::AtRest {
                        style,
                        theme,
                        tab_action_reserve_space,
                        active_group,
                        at_rest: [active, clean, dirty, pinned].iter().map(|&index| visible(&opacities, index)).collect(),
                    });
                    expected.push(Measurement::AtRest { style, theme, tab_action_reserve_space, active_group, at_rest: vec![true, false, true, true] });
                }

                // `group.classList.add('active')` e o foco no botão da aba limpa
                let opacities = tab_opacities(&fx, true, high_contrast, |index, context| {
                    context.title_reserves_action_space = tab_action_reserve_space;
                    if index == clean {
                        context.label_focused = true;
                        context.focus_within = true;
                    }
                });
                measurements.push(Measurement::Focus { style, theme, tab_action_reserve_space, focused: true, keyboard_focus: visible(&opacities, clean) });
                expected.push(Measurement::Focus { style, theme, tab_action_reserve_space, focused: true, keyboard_focus: true });
            }
        }
    }

    assert_eq!(measurements, expected);
}

const HC_TAB_BORDER: &str = "#00ffff";
const HC_CONTRAST_ACTIVE_BORDER: &str = "#ffaa00";
const HC_INACTIVE_MODIFIED_BORDER: &str = "#22d3ee";

/// `getComputedStyle(...).borderXColor` como texto: a cor do tema ou o nome da variável de pintura que sobrou.
fn describe_stroke_color(color: TabStrokeColor) -> String {
    match color {
        TabStrokeColor::Color(color) => color.to_string(),
        TabStrokeColor::Transparent => "rgba(0, 0, 0, 0)".to_string(),
        other => format!("{other:?}"),
    }
}

#[test]
fn hc_modified_pills_retain_hover_and_selection_side_strokes_independently_of_their_top_bar() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `model.getEditorByIndex(0)` é um `TestFileEditorInput` que o caso marca como modificado; o fixture daqui abre `NamedInput`,
    // então as duas entradas são refeitas na mesma ordem (a modificada em 0 e `last` ativa).
    for editor in fx.model.get_editors(EditorsOrder::Sequential, None) {
        fx.close_editor(&editor);
    }
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/file0.txt"), "testEditorInput"));
    concrete.set_dirty();
    let modified: EditorInputRef = concrete;
    open!(fx.model, modified, index: 0, pinned: true, active: false);
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/last.ts"), "testEditorInput"));
    let last: EditorInputRef = concrete;
    open!(fx.model, last, index: 1, pinned: true, active: true);
    fx.open_editors();
    // `root.style.setProperty('--vscode-tab-border', '#00ffff')`, `--vscode-contrastActiveBorder` e
    // `new TestColorTheme({ 'tab.inactiveModifiedBorder': '#22d3ee' })`
    fx.state.set_colors(TabColors {
        tab_border: Some(HC_TAB_BORDER.to_string()),
        active_contrast_border: Some(HC_CONTRAST_ACTIVE_BORDER.to_string()),
        tab_inactive_modified_border: Some(HC_INACTIVE_MODIFIED_BORDER.to_string()),
        ..TabColors::default()
    });

    #[derive(Debug, PartialEq)]
    struct Row {
        theme: &'static str,
        compact: bool,
        wrap_tabs: bool,
        selected: bool,
        /// `[borderLeftStyle, borderRightStyle, borderBottomStyle]`
        styles: [TabStrokeStyle; 3],
        /// `[borderLeftColor, borderRightColor, borderBottomColor]`
        colors: [String; 3],
        /// `[accent.color, accent.height, accent.topOffset]`
        accent: Option<(String, f64, f64)>,
        geometry_stable: bool,
    }

    let sides = [StrokeSide::Left, StrokeSide::Right, StrokeSide::Bottom];
    let mut results = Vec::new();
    for theme in ["hc-black", "hc-light"] {
        for tab_height in [TabHeight::Default, TabHeight::Compact] {
            for wrap_tabs in [false, true] {
                fx.update_options(|options| {
                    options.tab_height = tab_height;
                    options.wrap_tabs = wrap_tabs;
                    options.highlight_modified_tabs = true;
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                    options.tab_sizing = TabSizing::Fixed;
                    options.tab_sizing_fixed_min_width = 120.0;
                    options.tab_sizing_fixed_max_width = 120.0;
                });
                for selected in [false, true] {
                    // `model.setSelection(last, selected ? [modified] : [])` e `control.updateEditorSelections()`
                    let selection: Vec<EditorInputRef> = if selected { fx.model.get_editors(EditorsOrder::Sequential, None).into_iter().take(1).collect() } else { Vec::new() };
                    let active = fx.model.active_editor().expect("aba ativa");
                    fx.model.set_selection(&active, &selection);
                    fx.state.set_active();
                    fx.layout(if wrap_tabs { 150.0 } else { 600.0 });

                    let index = fx.state.tabs.iter().position(|tab| tab.state.dirty_border_top).expect("aba com `dirty-border-top`");
                    let rect = |fx: &Fixture| fx.layout.tab_rect(&fx.geometry(), index, 0.0);
                    let before = rect(&fx);
                    // `tab.classList.add('hovered')`
                    let state = &fx.state.tabs[index].state;
                    let stroke = high_contrast_connected_tab_fill_stroke(&TabFillStrokeInput {
                        wrapping: fx.state.container.wrapping,
                        upper_bar: fx.state.container.connected_tab_upper_bar,
                        active: state.active,
                        selected: state.selected,
                        multi_selected: state.multi_selected,
                        dirty_border_top: state.dirty_border_top,
                        upper_row: state.connected_tab_upper_row,
                        hovered: true,
                        tab_border: Some(HC_TAB_BORDER),
                        contrast_active_border: Some(HC_CONTRAST_ACTIVE_BORDER),
                        dirty_border_top_color: state.dirty_border_top_color.as_deref(),
                        spacing_size20: SPACING_SIZE20,
                        ..TabFillStrokeInput::default()
                    });
                    results.push(Row {
                        theme,
                        compact: tab_height == TabHeight::Compact,
                        wrap_tabs,
                        selected,
                        styles: sides.map(|side| stroke.style(side)),
                        colors: sides.map(|side| describe_stroke_color(stroke.color(side))),
                        accent: stroke.top_accent.map(|accent| (describe_stroke_color(accent.color), accent.height, accent.top_offset)),
                        geometry_stable: before == rect(&fx),
                    });
                }
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            theme: result.theme,
            compact: result.compact,
            wrap_tabs: result.wrap_tabs,
            selected: result.selected,
            styles: [if result.selected { TabStrokeStyle::Solid } else { TabStrokeStyle::Dashed }; 3],
            colors: std::array::from_fn(|_| HC_CONTRAST_ACTIVE_BORDER.to_string()),
            accent: Some((HC_INACTIVE_MODIFIED_BORDER.to_string(), 2.0, 0.0)),
            geometry_stable: true,
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn short_fit_tab_names_remain_complete_beside_transient_close_actions() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `root.classList.add('agent-sessions-workbench', 'dock-detail-panel')` e o `::before` vazio do ícone são pintura.
    let editors = fx.open_named_editors_described(&names(&["F.ts", "Files.ts", "Browser", "Search"]), None);
    for _ in &editors {
        fx.update_editor_label();
    }

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        tab_action_reserve_space: bool,
        tab_action_location: TabActionLocation,
        name: String,
        full_name: bool,
        stable_width: bool,
    }

    let mut results = Vec::new();
    for connected in [false, true] {
        fx.set_connected(connected);
        for tab_action_reserve_space in [false, true] {
            for tab_action_location in [TabActionLocation::Left, TabActionLocation::Right] {
                fx.update_options(|options| {
                    options.tab_sizing = TabSizing::Fit;
                    options.tab_action_reserve_space = tab_action_reserve_space;
                    options.tab_action_location = tab_action_location;
                    options.has_icons = true;
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                });
                fx.layout(800.0);
                let widths = fx.geometry().widths;
                for index in 0..fx.state.tabs.len() {
                    fx.hover_tab(index);
                    fx.focus_action(index);
                    let label = &fx.state.tabs[index].state.label;
                    let name = format!("{}{}", label.name_prefix.as_deref().unwrap_or(""), label.name.as_deref().unwrap_or(""));
                    let name_width = name.chars().count() as f64 * CHAR_WIDTH;
                    results.push(Row {
                        connected,
                        tab_action_reserve_space,
                        tab_action_location,
                        full_name: name_width <= fx.label_width(index, true) + 1.0 / 64.0,
                        stable_width: (widths[index] - fx.geometry().widths[index]).abs() <= 1.0 / 64.0,
                        name,
                    });
                }
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            connected: result.connected,
            tab_action_reserve_space: result.tab_action_reserve_space,
            tab_action_location: result.tab_action_location,
            name: result.name.clone(),
            full_name: true,
            stable_width: true,
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn wrapped_fit_labels_ellipsize_oversized_names_within_the_header() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let oversized = format!("Very long {}.ts", "filename segment ".repeat(30));
    fx.open_named_editors(&[oversized, "F.ts".to_string()]);
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fit;
        options.tab_action_reserve_space = false;
        options.wrap_tabs = true;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });

    #[derive(Debug, PartialEq)]
    struct Row {
        width: f64,
        wrapped: bool,
        contained: bool,
        trimmed: bool,
    }

    let mut results = Vec::new();
    for width in [300.0, 900.0, 300.0] {
        fx.layout(width);
        let geometry = fx.geometry();
        let (_, right) = fx.layout.tab_rect(&geometry, 0, 0.0);
        let tab = &fx.state.tabs[0];
        results.push(Row {
            width,
            wrapped: fx.state.container.wrapping,
            contained: right <= geometry.visible_width + 1.0 / 64.0,
            // `scrollWidth > clientWidth` do nome: o conteúdo natural passa da caixa. O `text-overflow: ellipsis` é pintura.
            trimmed: fx.layout.natural_width(&fx.state.container, tab, tab.state.tab_action_sizing_rest) > geometry.widths[0],
        });
    }

    let expected: Vec<Row> = results.iter().map(|result| Row { width: result.width, wrapped: true, contained: true, trimmed: true }).collect();
    assert_eq!(results, expected);
}

#[test]
fn modern_tab_labels_preserve_icons_and_ellipsis_with_transient_actions_across_sizing_modes() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // O `<style>` com o `::before` do ícone e o `::after` do selo é pintura, e a classe `monaco-decoration-badge` é do rótulo.
    fx.open_named_editors(&names(&["Files", "a-long-tab-name-that-needs-truncation.ts"]));

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        tab_sizing: TabSizing,
        tab_action_reserve_space: bool,
        tab_action_location: TabActionLocation,
        icon_visible: bool,
        text_fits_label: bool,
    }

    let mut results = Vec::new();
    for connected in [false, true] {
        fx.set_connected(connected);
        for tab_sizing in [TabSizing::Fit, TabSizing::Fixed, TabSizing::Shrink] {
            for tab_action_reserve_space in [false, true] {
                for tab_action_location in [TabActionLocation::Left, TabActionLocation::Right] {
                    fx.update_options(|options| {
                        options.tab_sizing = tab_sizing;
                        options.tab_action_reserve_space = tab_action_reserve_space;
                        options.tab_action_location = tab_action_location;
                        options.has_icons = true;
                        options.tab_sizing_fixed_min_width = 80.0;
                        options.tab_sizing_fixed_max_width = 80.0;
                    });
                    fx.layout(200.0);
                    // `tab.style.removeProperty('--tab-sizing-fit-width')` e o layout seguinte
                    fx.state.tabs[0].state.fit_width = None;
                    fx.layout(200.0);
                    fx.hover_tab(0);
                    fx.focus_action(0);
                    let state = &fx.state.tabs[0].state;
                    results.push(Row {
                        connected,
                        tab_sizing,
                        tab_action_reserve_space,
                        tab_action_location,
                        icon_visible: state.has_icon && !state.label.hide_icon,
                        // O nome fica dentro do rótulo (`min-width: 0` e `overflow: hidden`): o rótulo nunca passa da aba.
                        text_fits_label: fx.label_width(0, true) >= 0.0,
                    });
                }
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            connected: result.connected,
            tab_sizing: result.tab_sizing,
            tab_action_reserve_space: result.tab_action_reserve_space,
            tab_action_location: result.tab_action_location,
            icon_visible: true,
            text_fits_label: true,
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn unreserved_fit_widths_refresh_when_a_decoration_provider_adds_or_removes_a_badge() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let resource = fx.model.get_editor_by_index(1).expect("aba 1").resource().expect("recurso da aba 1");
    {
        let mut options = fx.options.borrow_mut();
        options.tab_action_reserve_space = false;
        options.tab_sizing = TabSizing::Fit;
        options.decorations = EditorPartDecorationOptions { badges: true, colors: true };
    }
    // `control.dispose(); reset(container); control = createControl()`
    fx.create_control(false);
    fx.layout(600.0);
    let original_width = fx.geometry().widths[1];

    let mut widths = Vec::new();
    for enabled in [true, false] {
        // `decorated = enabled; changed.fire([resource])` e o quadro em que o layout assenta
        *fx.layout.badged.borrow_mut() = enabled.then(|| resource.clone());
        fx.decorations_changed();
        assert_eq!(fx.state.tabs[1].state.label.file_decoration_badges, true);
        widths.push(fx.geometry().widths[1]);
    }

    assert_eq!((widths[0] > original_width, (widths[1] - original_width).abs() < 1.0 / 32.0), (true, true));
}

#[test]
fn connected_fit_tabs_reallocate_filename_width_to_transient_actions() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let editors = fx.open_named_editors(&names(&[
        "a-natural-width-label-longer-than-the-short-fit-minimum.ts",
        "another-natural-width-label-longer-than-the-short-fit-minimum.ts",
    ]));
    open!(fx.model, editors[0], active: true);
    fx.open_editors();
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fit;
        options.tab_action_reserve_space = false;
    });
    fx.layout(400.0);

    // `tab.style.removeProperty('--tab-sizing-fit-width')`
    fx.state.tabs[1].state.fit_width = None;
    let resting = (fx.geometry().widths[1], fx.label_width(1, false));
    fx.hover_tab(1);
    fx.focus_action(1);
    let focused = (fx.geometry().widths[1], fx.label_width(1, true));
    let cached = fx.state.tabs[1].state.fit_width;

    assert_eq!(
        (
            (focused.0 - resting.0).abs() < 1.0 / 32.0,
            focused.1 < resting.1,
            cached.is_some_and(|width| (width - resting.0).abs() < 1.0 / 32.0),
        ),
        (true, true, true),
        "{resting:?} {focused:?} {cached:?}"
    );
}

#[test]
fn unreserved_fit_widths_follow_editor_names_when_tab_slots_are_reused_after_closing_or_reordering() {
    let mut fx = Fixture::new();
    fx.connected_group();

    #[derive(Debug, PartialEq)]
    struct Row {
        style: &'static str,
        operation: &'static str,
        distinct_natural_widths: bool,
        first_name: String,
        long_width_preserved: bool,
        remaining_widths_preserved: bool,
    }

    let mut results = Vec::new();
    for style in ["pill", "connected"] {
        fx.set_connected(style == "connected");
        for operation in ["close", "reorder"] {
            fx.update_options(|options| {
                options.tab_sizing = TabSizing::Fit;
                options.tab_action_reserve_space = false;
                options.editor_actions_location = EditorActionsLocation::Hidden;
            });
            let editors = fx.open_named_editors(&names(&["a.ts", "a-significantly-longer-filename.ts", "active.ts"]));
            fx.layout(1000.0);
            let widths = fx.geometry().widths;
            if operation == "close" {
                fx.close_editor(&editors[0]);
            } else {
                fx.move_editor(&editors[1], 1, 0);
            }
            fx.layout(1000.0);
            let reordered = fx.geometry().widths;
            results.push(Row {
                style,
                operation,
                distinct_natural_widths: widths[1] > widths[0],
                first_name: fx.state.tabs[0].state.label.name.clone().unwrap_or_default(),
                long_width_preserved: reordered[0] == widths[1],
                remaining_widths_preserved: if operation == "close" { reordered[1] == widths[2] } else { reordered[1] == widths[0] && reordered[2] == widths[2] },
            });
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            style: result.style,
            operation: result.operation,
            distinct_natural_widths: true,
            first_name: "a-significantly-longer-filename.ts".to_string(),
            long_width_preserved: true,
            remaining_widths_preserved: true,
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn unreserved_fit_widths_refresh_when_tab_index_and_label_format_options_change() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // O rótulo desenhado inclui o texto da descrição.
    fx.layout.show_descriptions.set(true);

    #[derive(Debug, PartialEq)]
    struct Row {
        style: &'static str,
        indexed_name: String,
        description: String,
        index_adds_width: bool,
        description_adds_width: bool,
        restored_width: bool,
    }

    let mut results = Vec::new();
    for style in ["pill", "connected"] {
        fx.set_connected(style == "connected");
        fx.update_options(|options| {
            options.tab_sizing = TabSizing::Fit;
            options.tab_action_reserve_space = false;
            options.show_tab_index = false;
            options.label_format = EditorLabelFormat::Short;
            options.editor_actions_location = EditorActionsLocation::Hidden;
        });
        fx.open_named_editors(&names(&["a.ts", "active.ts"]));
        fx.layout(1000.0);
        let resting_width = fx.geometry().widths[0];
        fx.update_options(|options| options.show_tab_index = true);
        fx.layout(1000.0);
        let indexed_width = fx.geometry().widths[0];
        let label = &fx.state.tabs[0].state.label;
        let indexed_name = format!("{}{}", label.name_prefix.as_deref().unwrap_or(""), label.name.as_deref().unwrap_or(""));
        fx.update_options(|options| options.label_format = EditorLabelFormat::Long);
        fx.layout(1000.0);
        let long_width = fx.geometry().widths[0];
        let description = fx.state.tabs[0].state.label.description.clone();
        fx.update_options(|options| {
            options.show_tab_index = false;
            options.label_format = EditorLabelFormat::Short;
        });
        fx.layout(1000.0);
        results.push(Row {
            style,
            indexed_name,
            description,
            index_adds_width: indexed_width > resting_width,
            description_adds_width: long_width > indexed_width,
            restored_width: fx.geometry().widths[0] == resting_width,
        });
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            style: result.style,
            indexed_name: "1: a.ts".to_string(),
            description: "/workspace/project/src".to_string(),
            index_adds_width: true,
            description_adds_width: true,
            restored_width: true,
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn unreserved_fit_widths_use_css_coordinates_at_zoom_during_layout_hover_and_focus() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let editors = fx.open_named_editors(&names(&["a-wide-natural-fit-label.ts", "another-wide-natural-fit-label.ts"]));
    open!(fx.model, editors[0], active: true);
    fx.open_editors();
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fit;
        options.tab_action_reserve_space = false;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.layout(1000.0);

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Trigger {
        Layout,
        Hover,
        Focus,
    }

    #[derive(Debug, PartialEq)]
    struct Row {
        style: &'static str,
        zoom: f64,
        trigger: Trigger,
        cached_css_width: bool,
        stable_target_width: bool,
    }

    // `tab.getBoundingClientRect().width / zoom`: o retângulo inclui o zoom, a variável da largura `fit` não.
    let css_width = |fx: &Fixture, zoom: f64| {
        let (left, right) = fx.layout.tab_rect(&fx.geometry(), 1, 0.0);
        (right - left) / zoom
    };
    let mut results = Vec::new();
    for style in ["pill", "connected"] {
        fx.set_connected(style == "connected");
        for zoom in [1.25, 1.6] {
            // `root.style.zoom = String(zoom)`
            fx.layout.zoom.set(zoom);
            for trigger in [Trigger::Layout, Trigger::Hover, Trigger::Focus] {
                // `action.blur()` e o `MOUSE_LEAVE` da aba
                fx.state.handle_tab_mouse_leave(1);
                if trigger != Trigger::Focus {
                    fx.state.tabs[1].state.fit_width = None;
                }
                let natural_width = css_width(&fx, zoom);
                match trigger {
                    Trigger::Layout => fx.layout(1000.0),
                    Trigger::Hover => fx.hover_tab(1),
                    Trigger::Focus => fx.focus_action(1),
                }
                results.push(Row {
                    style,
                    zoom,
                    trigger,
                    cached_css_width: fx.state.tabs[1].state.fit_width.is_some_and(|width| (width - natural_width).abs() < 1.0 / 32.0),
                    stable_target_width: (css_width(&fx, zoom) - natural_width).abs() < 1.0 / 32.0,
                });
            }
        }
    }

    // O `document.activeElement === action` do foco é do DOM.
    let expected: Vec<Row> = results.iter().map(|result| Row { style: result.style, zoom: result.zoom, trigger: result.trigger, cached_css_width: true, stable_target_width: true }).collect();
    assert_eq!(results, expected);
}

#[test]
fn uncached_fit_widths_release_all_wrapping_constraints_before_measuring_and_restore_them_together() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let labels: Vec<String> = (0..20).map(|index| format!("long-fit-label-{index}.ts")).collect();
    fx.open_named_editors(&labels);
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fit;
        options.tab_action_reserve_space = false;
    });
    fx.layout(300.0);
    // `tab.style.removeProperty('--tab-sizing-fit-width')` em cada aba
    for tab in &mut fx.state.tabs {
        tab.state.fit_width = None;
    }
    fx.layout.fit_reads.borrow_mut().clear();
    fx.layout(300.0);

    // O `maxWidth = 'none'` de cada aba e a volta ao valor anterior são o estilo inline do navegador (a medida da aba solta, e a
    // `TabsMeasurer` responde sempre pela largura intrínseca). O que o controle decide é medir todas as abas sem largura em
    // um lote só, antes de gravar qualquer largura.
    let first_read = fx.layout.fit_reads.borrow().first().cloned();
    let all_widths_cached = fx.state.tabs.iter().all(|tab| tab.state.fit_width.is_some_and(|width| width > 0.0));
    assert_eq!((first_read, all_widths_cached), (Some((0..20).collect::<Vec<usize>>()), true));
}

#[test]
fn transient_actions_do_not_resize_fixed_or_shrink_tabs_with_reservation_on_or_off() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let labels: Vec<String> = (0..8).map(|index| format!("tab-label-for-action-reservation-{index}.ts")).collect();
    fx.open_named_editors(&labels);

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        tab_sizing: TabSizing,
        tab_action_reserve_space: bool,
        wrap_tabs: bool,
        tab_action_location: TabActionLocation,
        rtl: bool,
        stable_widths: bool,
        text_clear_of_action: bool,
    }

    let mut results = Vec::new();
    for connected in [false, true] {
        fx.set_connected(connected);
        for tab_sizing in [TabSizing::Fixed, TabSizing::Shrink] {
            for tab_action_reserve_space in [false, true] {
                for wrap_tabs in [false, true] {
                    for tab_action_location in [TabActionLocation::Left, TabActionLocation::Right] {
                        for rtl in [false, true] {
                            // `root.dir = direction`
                            fx.layout.rtl.set(rtl);
                            fx.update_options(|options| {
                                options.tab_sizing = tab_sizing;
                                options.tab_action_reserve_space = tab_action_reserve_space;
                                options.wrap_tabs = wrap_tabs;
                                options.tab_action_location = tab_action_location;
                                options.tab_sizing_fixed_min_width = 80.0;
                                options.tab_sizing_fixed_max_width = 120.0;
                            });
                            fx.layout(if wrap_tabs { 500.0 } else { 1400.0 });
                            let before = fx.geometry().widths;
                            fx.focus_action(1);
                            let after = fx.geometry().widths;
                            // Em coordenadas lógicas: o texto vai do recuo ao início da coluna da ação (ação à direita) ou
                            // do fim da coluna ao recuo do outro lado (ação à esquerda), e fica dentro da aba.
                            let side = TAB_PADDING / 2.0;
                            let column = fx.layout.action_padding(&fx.state.tabs[1], &fx.state.container);
                            let text = fx.label_width(1, true);
                            let clear = match tab_action_location {
                                TabActionLocation::Right => side + text <= after[1] - column + 1.0 / 32.0,
                                TabActionLocation::Left => column + text <= after[1] - side + 1.0 / 32.0,
                            };
                            results.push(Row {
                                connected,
                                tab_sizing,
                                tab_action_reserve_space,
                                wrap_tabs,
                                tab_action_location,
                                rtl,
                                stable_widths: before.iter().zip(&after).all(|(before, after)| (before - after).abs() < 1.0 / 32.0),
                                text_clear_of_action: clear && text >= 0.0,
                            });
                        }
                    }
                }
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            connected: result.connected,
            tab_sizing: result.tab_sizing,
            tab_action_reserve_space: result.tab_action_reserve_space,
            wrap_tabs: result.wrap_tabs,
            tab_action_location: result.tab_action_location,
            rtl: result.rtl,
            stable_widths: true,
            text_clear_of_action: true,
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn unreserved_fit_tab_widths_and_wrapping_stay_stable_when_differently_named_clean_tabs_are_activated() {
    let mut fx = Fixture::new();
    fx.connected_group();

    #[derive(Debug, PartialEq)]
    struct Row {
        style: &'static str,
        wrap_tabs: bool,
        active_index: usize,
        baseline_widths: Vec<f64>,
        selected_widths: Vec<f64>,
        active_tab: bool,
        stable_widths: bool,
        stable_rows: bool,
        stable_wrapping: bool,
    }

    let mut results = Vec::new();
    for style in ["pill", "connected"] {
        fx.set_connected(style == "connected");
        for wrap_tabs in [false, true] {
            fx.update_options(|options| {
                options.tab_sizing = TabSizing::Fit;
                options.tab_action_reserve_space = false;
                options.wrap_tabs = wrap_tabs;
                options.editor_actions_location = EditorActionsLocation::Hidden;
            });
            let editors = fx.open_named_editors(&names(&["a.ts", "a-significantly-longer-filename.ts"]));
            let width = if wrap_tabs { 240.0 } else { 1000.0 };
            fx.layout(width);
            let baseline = fx.geometry();
            let baseline_wrapping = fx.state.container.wrapping;
            for active_index in [0, 1, 0, 1] {
                open!(fx.model, editors[active_index], active: true);
                fx.open_editors();
                fx.layout(width);
                let selected = fx.geometry();
                results.push(Row {
                    style,
                    wrap_tabs,
                    active_index,
                    baseline_widths: baseline.widths.clone(),
                    selected_widths: selected.widths.clone(),
                    active_tab: fx.state.tabs[active_index].state.active,
                    stable_widths: selected.widths.iter().zip(&baseline.widths).all(|(selected, baseline)| (selected - baseline).abs() < 1.0 / 32.0),
                    stable_rows: selected.tops == baseline.tops,
                    stable_wrapping: fx.state.container.wrapping == baseline_wrapping,
                });
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            style: result.style,
            wrap_tabs: result.wrap_tabs,
            active_index: result.active_index,
            baseline_widths: result.baseline_widths.clone(),
            selected_widths: result.selected_widths.clone(),
            active_tab: true,
            stable_widths: true,
            stable_rows: true,
            stable_wrapping: true,
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn action_reservation_changes_update_fit_widths_in_both_modern_styles_and_leave_legacy_unchanged() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.open_named_editors(&names(&["a-wide-natural-fit-label.ts", "another-wide-natural-fit-label.ts"]));
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/modified.ts"), "testEditorInput"));
    concrete.set_dirty();
    let modified: EditorInputRef = concrete;
    open!(fx.model, modified, pinned: true, active: false);
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/sticky.ts"), "testEditorInput"));
    let sticky: EditorInputRef = concrete;
    open!(fx.model, sticky, pinned: true, active: false);
    fx.model.stick(&sticky);
    fx.open_editors();

    #[derive(Debug, PartialEq)]
    struct Row {
        style: &'static str,
        tab_action_location: TabActionLocation,
        layout: &'static str,
        toggle_restores_reservation: bool,
        toggle_is_repeatable: bool,
        compact_without_reservation: bool,
        persistent_stable: bool,
        persistent_upper_rows: bool,
    }

    let mut results = Vec::new();
    for style in ["legacy", "pill", "connected"] {
        // `root.classList.toggle('modern-ui-tabs', style !== 'legacy')` e `modern-ui-connected-editor-tabs`
        fx.set_parent(TabsParentTraits { has_tabs_class: true, in_modern_ui_tabs: style != "legacy", in_connected_editor_tabs: style == "connected" });
        for tab_action_location in [TabActionLocation::Left, TabActionLocation::Right] {
            for layout in ["single", "wrapped", "pinned-wrapped"] {
                let wrapped = layout != "single";
                fx.update_options(|options| {
                    options.tab_sizing = TabSizing::Fit;
                    options.tab_action_reserve_space = true;
                    options.tab_action_location = tab_action_location;
                    options.tab_action_unpin_visibility = true;
                    options.pinned_tab_sizing = PinnedTabSizing::Normal;
                    options.wrap_tabs = wrapped;
                    options.pinned_tabs_on_separate_row = layout == "pinned-wrapped";
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                });
                fx.layout(if wrapped { 240.0 } else { 600.0 });
                let tab_index = fx.state.tabs.iter().position(|tab| !tab.state.active && !tab.state.dirty && !tab.state.sticky).expect("aba limpa e inativa");
                let persistent: Vec<usize> = fx
                    .state
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, tab)| (tab.state.dirty || tab.state.sticky) && (!wrapped || tab.state.connected_tab_upper_row))
                    .map(|(index, _)| index)
                    .collect();
                let persistent_widths: Vec<f64> = persistent.iter().map(|&index| fx.geometry().widths[index]).collect();
                let reserved = fx.geometry().widths[tab_index];
                let mut widths = Vec::new();
                let mut persistent_stable = true;
                for tab_action_reserve_space in [false, true, false] {
                    fx.update_options(|options| options.tab_action_reserve_space = tab_action_reserve_space);
                    fx.frame();
                    let geometry = fx.geometry();
                    widths.push(geometry.widths[tab_index]);
                    persistent_stable &= persistent.iter().zip(&persistent_widths).all(|(&index, width)| (geometry.widths[index] - width).abs() < 1.0 / 32.0);
                }
                results.push(Row {
                    style,
                    tab_action_location,
                    layout,
                    toggle_restores_reservation: widths[1] == reserved,
                    toggle_is_repeatable: widths[0] == widths[2],
                    compact_without_reservation: if style == "legacy" { widths[0] == reserved } else { widths[0] < reserved },
                    persistent_stable,
                    persistent_upper_rows: style != "connected" || !wrapped || !persistent.is_empty(),
                });
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            style: result.style,
            tab_action_location: result.tab_action_location,
            layout: result.layout,
            toggle_restores_reservation: true,
            toggle_is_repeatable: true,
            compact_without_reservation: true,
            persistent_stable: true,
            persistent_upper_rows: true,
        })
        .collect();
    assert_eq!(results, expected);
}

/// `{ name: ..., URI.file('/path/<name>') }` aberto sem ser ativo, duas vezes (`a.ts` e `b.ts`).
fn open_short_named_tabs(fx: &mut Fixture) {
    for name in ["a.ts", "b.ts"] {
        let concrete = fx.suite.disposables.add_rc(NamedInput::new(name, &format!("/path/{name}"), None));
        let editor: EditorInputRef = concrete;
        open!(fx.model, editor, pinned: true, active: false);
    }
    fx.open_editors();
}

#[test]
fn fit_sized_connected_tabs_retain_short_label_icons_and_match_pill_widths() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // O `::before` vazio do ícone é pintura.
    open_short_named_tabs(&mut fx);
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fit;
        options.has_icons = true;
        options.wrap_tabs = true;
        options.tab_action_reserve_space = true;
    });
    fx.set_connected(false);
    fx.layout(300.0);
    let last_two = |fx: &Fixture| fx.geometry().widths.split_off(fx.state.tabs.len() - 2);
    let pill_widths = last_two(&fx);
    fx.set_connected(true);
    fx.layout(300.0);

    assert_eq!(last_two(&fx), pill_widths);
    assert!(fx.state.tabs.iter().all(|tab| tab.state.has_icon && !tab.state.label.hide_icon));
}

#[test]
fn fit_sized_connected_tabs_resolve_calculated_action_reservation_before_caching_widths() {
    let mut fx = Fixture::new();
    fx.connected_group();
    open_short_named_tabs(&mut fx);
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fit;
        options.has_icons = false;
        options.tab_action_reserve_space = false;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    let count = fx.state.tabs.len();
    let measure_fit_widths = |fx: &mut Fixture, connected: bool| {
        fx.set_connected(connected);
        for tab in &mut fx.state.tabs[count - 2..] {
            tab.state.fit_width = None;
        }
        fx.layout(300.0);
        fx.state.tabs[count - 2..].iter().map(|tab| tab.state.fit_width.expect("largura `fit` guardada")).collect::<Vec<f64>>()
    };
    let pill_widths = measure_fit_widths(&mut fx, false);
    let connected_widths = measure_fit_widths(&mut fx, true);

    // `--modern-ui-tab-action-fit-padding` calculado (o `getComputedStyle` do original): 30.5px no estilo conectado.
    let reservation = fx.layout.action_column_width();
    let width_deltas: Vec<f64> = connected_widths.iter().zip(&pill_widths).map(|(connected, pill)| connected - pill).collect();
    assert_eq!((reservation, width_deltas), (30.5, vec![2.5, 2.5]));
}

#[test]
fn fit_sized_connected_tabs_keep_stable_reservations_across_density_and_wrapping_changes() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let editors = fx.open_file_editors(&names(&["a-long-persistent-editor-name.ts", "b.ts"]));
    editors[0].set_dirty();
    let persistent: EditorInputRef = editors[0].clone();
    let computed_height = fx.computed_height();
    fx.state.update_editor_dirty(&persistent, computed_height);
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fit;
        options.tab_action_reserve_space = false;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    let cached_widths = |fx: &Fixture| -> Vec<Option<f64>> { fx.state.tabs.iter().map(|tab| tab.state.fit_width).collect() };
    let clear_cached_widths = |fx: &mut Fixture| {
        for tab in &mut fx.state.tabs {
            tab.state.fit_width = None;
        }
    };

    fx.layout(600.0);
    let default_widths = cached_widths(&fx);
    fx.update_options(|options| options.tab_height = TabHeight::Compact);
    fx.layout(600.0);
    let compact_widths = cached_widths(&fx);
    fx.update_options(|options| options.tab_height = TabHeight::Default);
    fx.layout(600.0);
    let restored_widths = cached_widths(&fx);
    fx.update_options(|options| options.wrap_tabs = true);
    fx.layout(120.0);
    let wrapped_widths = cached_widths(&fx);
    let wrapping = fx.state.container.wrapping;
    clear_cached_widths(&mut fx);
    fx.layout(120.0);
    let wrapped_recached_widths = cached_widths(&fx);
    fx.update_options(|options| {
        options.wrap_tabs = false;
        options.tab_action_close_visibility = false;
    });
    fx.layout(600.0);
    let hidden_default_widths = cached_widths(&fx);
    fx.update_options(|options| options.wrap_tabs = true);
    fx.layout(120.0);
    clear_cached_widths(&mut fx);
    fx.layout(120.0);
    let hidden_wrapped_widths = cached_widths(&fx);
    let sticky = fx.model.get_editor_by_index(1).expect("aba 1");
    fx.model.stick(&sticky);
    let sticky_index = usize::try_from(fx.model.index_of(Some(&typed(&sticky)), None, None)).expect("aba fixada");
    fx.state.handle_sticky_editor_change(sticky_index);
    fx.update_options(|options| {
        options.wrap_tabs = false;
        options.tab_action_close_visibility = true;
        options.tab_action_location = TabActionLocation::Left;
    });
    fx.layout(600.0);
    let sticky_left_default_widths = cached_widths(&fx);
    fx.update_options(|options| options.wrap_tabs = true);
    fx.layout(120.0);
    clear_cached_widths(&mut fx);
    fx.layout(120.0);
    let sticky_left_wrapped_widths = cached_widths(&fx);

    let compact_deltas: Vec<Option<f64>> = compact_widths.iter().zip(&default_widths).map(|(compact, default)| compact.zip(*default).map(|(compact, default)| compact - default)).collect();
    assert_eq!(
        (
            compact_deltas,
            restored_widths,
            wrapping,
            wrapped_widths,
            wrapped_recached_widths,
            hidden_wrapped_widths,
            sticky_left_wrapped_widths,
        ),
        (
            vec![Some(-2.0), Some(-2.0)],
            default_widths.clone(),
            true,
            default_widths.clone(),
            default_widths,
            hidden_default_widths,
            sticky_left_default_widths,
        )
    );
}

/// O que `measure()` lê da aba ativa em `connected close actions keep consistent spacing ...`.
#[derive(Debug, PartialEq)]
struct ActionMeasure {
    top: f64,
    bottom: f64,
    right: f64,
    left: f64,
    width: f64,
    target: [f64; 2],
    action_insets: [f64; 2],
    padding: [f64; 4],
}

/// `measure()` do caso: as distâncias do `.action-label` às bordas da aba ativa e ao rótulo, a largura do `.tab-fill`, o alvo, o
/// recuo do botão dentro do `.tab-actions` e o `padding` computado.
fn measure_active_action(fx: &Fixture) -> ActionMeasure {
    let index = fx.state.tabs.iter().position(|tab| tab.state.active).expect("aba ativa");
    let context = fx.box_context(index);
    let geometry = fx.action_geometry(index);
    let (actions, label) = (geometry.actions, geometry.label);

    ActionMeasure {
        top: label.top,
        bottom: context.tab_height - label.bottom,
        right: context.tab_width - label.right,
        left: label.left - fx.label_right(index),
        width: tab_fill_rect(&context).width(),
        target: [label.width(), label.height()],
        action_insets: [label.left - actions.left - geometry.border_left, actions.right - geometry.border_right - label.right],
        padding: geometry.label_padding,
    }
}

#[test]
fn connected_close_actions_keep_consistent_spacing_across_terminal_and_wrapped_tabs() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.style.setProperty('--vscode-editorGroupHeader-tabsBorder', '#333333')` é pintura.
    fx.layout(400.0);
    let multiple = measure_active_action(&fx);
    let stroke = STROKE;

    let second = fx.model.get_editor_by_index(1).expect("aba 1");
    fx.close_editor(&second);
    fx.layout(400.0);
    let single = measure_active_action(&fx);

    open!(fx.model, second, pinned: true, active: true);
    fx.open_editors();
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 120.0;
        options.tab_sizing_fixed_max_width = 120.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.layout(150.0);
    let wrapped_bottom = measure_active_action(&fx);

    let first = fx.model.get_editor_by_index(0).expect("aba 0");
    open!(fx.model, first, active: true);
    fx.open_editors();
    fx.layout(150.0);
    let wrapped_upper = measure_active_action(&fx);

    fx.update_options(|options| {
        options.wrap_tabs = false;
        options.tab_sizing = TabSizing::Fit;
        options.tab_action_location = TabActionLocation::Left;
    });
    fx.layout(400.0);
    let left_multiple = measure_active_action(&fx);

    fx.close_editor(&second);
    fx.layout(400.0);
    let left_single = measure_active_action(&fx);

    #[derive(Debug, PartialEq)]
    struct Report {
        single: [bool; 4],
        clearance: Vec<[f64; 4]>,
        left_action: [bool; 4],
        balanced_action_surface: bool,
        action_padding: bool,
        targets: Vec<[f64; 2]>,
    }

    let measurements = [&multiple, &single, &wrapped_bottom, &wrapped_upper];
    let report = Report {
        single: [single.top == multiple.top, single.right == multiple.right, single.left == multiple.left, single.width == multiple.width],
        clearance: measurements.iter().map(|measurement| [measurement.top, measurement.right, measurement.bottom, measurement.left]).collect(),
        left_action: [left_single.top == left_multiple.top, left_single.right == left_multiple.right, left_single.left == left_multiple.left, left_single.width == left_multiple.width],
        balanced_action_surface: [&multiple, &single, &wrapped_bottom, &wrapped_upper, &left_multiple, &left_single]
            .iter()
            .all(|measurement| (measurement.action_insets[0] - measurement.action_insets[1]).abs() <= stroke),
        action_padding: measurements.iter().all(|measurement| measurement.padding.iter().all(|&value| value == measurement.padding[0]) && measurement.padding[0] == multiple.padding[0]),
        targets: measurements.iter().map(|measurement| measurement.target).collect(),
    };
    let expected = Report {
        single: [true; 4],
        clearance: vec![[5.5, 6.5, 5.5, 4.0], [5.5, 6.5, 5.5, 4.0], [4.0, 3.5, 4.0, 4.5], [4.0, 4.0, 4.0, 4.0]],
        left_action: [true; 4],
        balanced_action_surface: true,
        action_padding: true,
        targets: vec![[20.0, 20.0]; 4],
    };
    assert_eq!(report, expected);
}

#[test]
fn pin_and_close_actions_share_geometry_across_tab_styles_and_wrapping() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let sticky_editor = fx.model.get_editor_by_index(0).expect("aba 0");
    let active_editor = fx.model.get_editor_by_index(1).expect("aba 1");
    fx.model.stick(&sticky_editor);
    fx.state.handle_sticky_editor_change(0);
    open!(fx.model, active_editor, active: true);
    fx.open_editors();

    #[derive(Debug, PartialEq)]
    struct Row {
        connected: bool,
        tab_action_location: TabActionLocation,
        wrap_tabs: bool,
        targets_match: bool,
        clearance_matches: bool,
        labels: [TabAction; 2],
    }

    // O `clearance` do original: topo, a ponta da ação (a direita ou a esquerda) e a base, e a ação que o `aria-label` nomeia.
    let measure = |fx: &Fixture, index: usize, location: TabActionLocation| {
        let context = fx.box_context(index);
        let label = fx.action_geometry(index).label;
        let edge = if location == TabActionLocation::Right { context.tab_width - label.right } else { label.left };

        ([label.width(), label.height()], [label.top, edge, context.tab_height - label.bottom], fx.state.tabs[index].state.action.expect("ação da aba"))
    };

    let mut results = Vec::new();
    for connected in [true, false] {
        fx.set_connected(connected);
        for tab_action_location in [TabActionLocation::Right, TabActionLocation::Left] {
            for wrap_tabs in [false, true] {
                fx.update_options(|options| {
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                    options.pinned_tab_sizing = PinnedTabSizing::Normal;
                    options.tab_action_location = tab_action_location;
                    options.tab_action_unpin_visibility = true;
                    options.tab_sizing = if wrap_tabs { TabSizing::Fixed } else { TabSizing::Fit };
                    options.tab_sizing_fixed_min_width = 120.0;
                    options.tab_sizing_fixed_max_width = 120.0;
                    options.wrap_tabs = wrap_tabs;
                });
                fx.layout(if wrap_tabs { 150.0 } else { 400.0 });
                let sticky = fx.state.tabs.iter().position(|tab| tab.state.sticky).expect("aba fixada");
                let active = fx.state.tabs.iter().position(|tab| tab.state.active && !tab.state.sticky).expect("aba ativa");
                let (pin_target, pin_clearance, pin_action) = measure(&fx, sticky, tab_action_location);
                let (close_target, close_clearance, close_action) = measure(&fx, active, tab_action_location);
                results.push(Row {
                    connected,
                    tab_action_location,
                    wrap_tabs,
                    targets_match: pin_target == close_target,
                    clearance_matches: pin_clearance.iter().zip(&close_clearance).all(|(pin, close)| (pin - close).abs() <= 1.0),
                    labels: [pin_action, close_action],
                });
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            connected: result.connected,
            tab_action_location: result.tab_action_location,
            wrap_tabs: result.wrap_tabs,
            targets_match: true,
            clearance_matches: true,
            labels: [TabAction::Unpin, TabAction::Close],
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn close_hover_targets_stay_centered_in_hitboxes_across_wrapped_surface_shapes() {
    let mut fx = Fixture::new();
    fx.connected_group();

    #[derive(Debug, PartialEq)]
    struct Row {
        compact: bool,
        tab_action_location: TabActionLocation,
        wrap_tabs: bool,
        active_index: usize,
        top: f64,
        bottom: f64,
        trailing: f64,
        target: [f64; 2],
    }

    let mut measurements = Vec::new();
    let mut expected = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        for tab_action_location in [TabActionLocation::Right, TabActionLocation::Left] {
            for wrap_tabs in [false, true] {
                fx.update_options(|options| {
                    options.tab_height = tab_height;
                    options.tab_action_location = tab_action_location;
                    options.wrap_tabs = wrap_tabs;
                    options.tab_sizing = TabSizing::Fixed;
                    options.tab_sizing_fixed_min_width = 120.0;
                    options.tab_sizing_fixed_max_width = 120.0;
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                });
                for active_index in [0usize, 1] {
                    let editor = fx.model.get_editor_by_index(active_index as i64).expect("aba");
                    open!(fx.model, editor, active: true);
                    fx.open_editors();
                    fx.layout(if wrap_tabs { 150.0 } else { 400.0 });
                    let index = fx.state.tabs.iter().position(|tab| tab.state.active).expect("aba ativa");
                    let context = fx.box_context(index);
                    let label = fx.action_geometry(index).label;
                    // `leftBorder` é a cor computada do `.tab-fill`, pintura da renderização nativa.
                    measurements.push(Row {
                        compact: tab_height == TabHeight::Compact,
                        tab_action_location,
                        wrap_tabs,
                        active_index,
                        top: label.top,
                        bottom: context.tab_height - label.bottom,
                        trailing: if tab_action_location == TabActionLocation::Left { label.left } else { context.tab_width - label.right },
                        target: [label.width(), label.height()],
                    });
                    let compact = tab_height == TabHeight::Compact;
                    let clearance = match (wrap_tabs, compact) {
                        (true, true) => 2.0,
                        (true, false) => 4.0,
                        (false, true) => 3.5,
                        (false, false) => 5.5,
                    };
                    let trailing = match (tab_action_location, wrap_tabs, active_index, compact) {
                        (TabActionLocation::Left, ..) => 5.0,
                        (_, false, _, true) => 4.5,
                        (_, false, _, false) => 6.5,
                        (_, true, 1, true) => 1.5,
                        (_, true, 1, false) => 3.5,
                        (_, true, ..) => 4.0,
                    };
                    expected.push(Row { compact, tab_action_location, wrap_tabs, active_index, top: clearance, bottom: clearance, trailing, target: [20.0, 20.0] });
                }
            }
        }
    }
    assert_eq!(measurements, expected);
}

#[test]
fn reveals_the_active_tab_with_its_right_shoulder_outside_the_label_and_action() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 160.25;
        options.tab_sizing_fixed_max_width = 160.25;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.open_pinned_file("file2.txt", false);
    fx.open_editors();
    fx.layout(240.0);
    fx.activate(1);
    fx.reveal(240.0);

    let index = 1;
    let (_, fill_right) = fx.fill_client_span(index);
    let viewport_right = fx.geometry().visible_width * fx.layout.zoom.get();
    #[derive(Debug, PartialEq)]
    struct Report {
        label_before_action: bool,
        shoulder_visible: bool,
        clipped: bool,
    }

    let report = Report {
        label_before_action: fx.label_right(index) <= fx.action_geometry(index).actions.left,
        shoulder_visible: fill_right + SHOULDER_RADIUS <= viewport_right,
        clipped: fx.state.tabs[index].state.connected_clipping.right_edge,
    };
    assert_eq!(report, Report { label_before_action: true, shoulder_visible: true, clipped: false });
}

#[test]
fn reveals_the_terminal_connected_editor_close_action_and_shoulder_before_add_tab() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.closest('.monaco-workbench').classList.add('agent-sessions-workbench', 'dock-detail-panel')` só troca a pintura do
    // painel de encaixe; a geometria das abas vem do estilo conectado, que já vale.
    // `control.dispose(); reset(container); control = createControl({ tabsBarAddTab: ... })`
    fx.update_options(|options| {
        options.wrap_tabs = false;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 160.25;
        options.tab_sizing_fixed_max_width = 160.25;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.create_control(true);
    // `model.openEditor(third, { index: model.count, pinned: true, active: false })`: no fim, depois da aba terminal.
    let third: EditorInputRef = {
        let concrete = fx.suite.disposables.add_rc(NamedInput::new("browser.ts", "/path/browser.ts", None));
        concrete
    };
    open!(fx.model, third, index: fx.model.count() as i64, pinned: true, active: false);
    fx.open_editors();

    #[derive(Debug, PartialEq)]
    struct Row {
        compact: bool,
        zoom: f64,
        add_tab_is_last_child: bool,
        close_fully_visible: bool,
        close_receives_pointer: bool,
        shoulder_fully_visible: bool,
        no_clipping: bool,
        no_mask: bool,
    }

    let mut results = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        fx.update_options(|options| options.tab_height = tab_height);
        for zoom in [1.0, 1.25, 1.6] {
            fx.layout.zoom.set(zoom);
            fx.activate(0);
            fx.layout(240.0);
            open!(fx.model, third, active: true);
            fx.open_editors();
            fx.reveal(240.0);

            let index = fx.active_index();
            let (_, fill_right) = fx.fill_client_span(index);
            let (tab_left, _) = fx.layout.tab_rect(&fx.geometry(), index, fx.state.container.scroll_left);
            let close = fx.action_geometry(index).label;
            let (close_left, close_right) = (tab_left + close.left * zoom, tab_left + close.right * zoom);
            let (add_left, _) = fx.layout.add_rect(&fx.geometry());
            let center = (close_left + close_right) / 2.0;
            let state = &fx.state.tabs[index].state;
            results.push(Row {
                compact: tab_height == TabHeight::Compact,
                zoom,
                // `tab.nextElementSibling === addTab`: o botão é o último filho do contêiner de abas, depois da aba terminal.
                add_tab_is_last_child: index + 1 == fx.state.tabs.len() && fx.state.container.add_tab == AddTabPresence::Visible,
                close_fully_visible: close_left >= 0.0 && close_right <= add_left,
                // `elementFromPoint` no centro do botão: nada o cobre quando o centro está antes do botão de nova aba e a aba não é recortada.
                close_receives_pointer: center >= 0.0 && center < add_left && !state.connected_clipping.right_clipped,
                shoulder_fully_visible: fill_right + SHOULDER_RADIUS * zoom <= add_left + 1.0 / 64.0,
                no_clipping: !state.connected_clipping.right_clipped && !state.connected_clipping.right_edge,
                no_mask: !fx.state.container.overflow_edge.right_clipped,
            });
        }
    }
    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            compact: result.compact,
            zoom: result.zoom,
            add_tab_is_last_child: true,
            close_fully_visible: true,
            close_receives_pointer: true,
            shoulder_fully_visible: true,
            no_clipping: true,
            no_mask: true,
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn reveals_the_left_shoulder_of_non_first_tabs_and_keeps_the_first_tab_flush() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 160.25;
        options.tab_sizing_fixed_max_width = 160.25;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    for index in 2..4 {
        fx.open_pinned_file(&format!("file{index}.txt"), false);
    }
    // `group.style.width` é a largura do grupo, que o `Layout` guarda como a do contêiner.
    let reveal = |fx: &mut Fixture, index: usize, width: f64| {
        fx.activate(index);
        fx.reveal(width);
    };

    #[derive(Debug, PartialEq)]
    struct Row {
        width: f64,
        left_shoulder_visible: bool,
        right_shoulder_visible: bool,
    }

    let mut results = Vec::new();
    for (width, from) in [(240.0, 3), (172.0, 0), (120.0, 0)] {
        reveal(&mut fx, from, width);
        reveal(&mut fx, 1, width);
        let index = fx.active_index();
        let (fill_left, fill_right) = fx.fill_client_span(index);
        // `::before` existe fora da primeira aba e quando o controle não marca `connected-tab-left-edge`.
        let has_left_shoulder = index != 0 && !fx.state.tabs[index].state.connected_clipping.left_edge;
        results.push(Row {
            width,
            left_shoulder_visible: has_left_shoulder && fill_left - SHOULDER_RADIUS >= 0.0,
            right_shoulder_visible: fill_right + SHOULDER_RADIUS <= fx.geometry().visible_width,
        });
    }
    reveal(&mut fx, 0, 240.0);
    let first = fx.active_index();
    // `firstBorderColor` (`rgba(0, 0, 0, 0)`) é a cor computada do `.tab-fill`, pintura da renderização nativa.
    let first_border_inset = tab_fill_rect(&fx.box_context(first)).left;
    let first_shoulder = first != 0 && !fx.state.tabs[first].state.connected_clipping.left_edge;

    assert_eq!(
        (results, first_border_inset, first_shoulder),
        (
            vec![
                Row { width: 240.0, left_shoulder_visible: true, right_shoulder_visible: true },
                Row { width: 172.0, left_shoulder_visible: true, right_shoulder_visible: false },
                Row { width: 120.0, left_shoulder_visible: true, right_shoulder_visible: false },
            ],
            0.0,
            false
        )
    );
}

#[test]
fn keeps_replacement_connected_tabs_visible_after_closing_rightmost_scrolled_tabs() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `root.classList.add('hc-black')` com `--vscode-focusBorder: #ffaa00`: a borda conectada vira o `focusBorder`.
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 160.0;
        options.tab_sizing_fixed_max_width = 160.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    for index in 2..5 {
        fx.open_pinned_file(&format!("file{index}.txt"), true);
    }
    fx.open_editors();
    fx.layout(200.0);

    #[derive(Debug, PartialEq)]
    struct Row {
        clamped_before_layout: bool,
        active_index: usize,
        scrolled_to_end: bool,
        fill_left: f64,
        hidden: bool,
        fill_displayed: bool,
        outline_displayed: bool,
        outline_color: &'static str,
    }

    let scroll_end = |fx: &Fixture| {
        let geometry = fx.geometry();
        geometry.content_width.max(geometry.visible_width) - geometry.visible_width
    };
    let mut results = Vec::new();
    for count in (3..=5).rev() {
        // `tabs.classList.add('scroll'); tabs.scrollLeft = scrollWidth - clientWidth; tabs.dispatchEvent(new UIEvent('scroll'))`
        fx.state.container.scroll_mode = true;
        let end = scroll_end(&fx);
        fx.scroll_to(end);
        fx.state.container.scroll_mode = false;
        let old_scroll_left = fx.state.container.scroll_left;
        let closed = fx.model.get_editor_by_index(count as i64 - 1).expect("aba");
        fx.close_editor(&closed);
        // O navegador limita `scrollLeft` ao encolher o conteúdo, antes de o layout rodar.
        let clamped_before_layout = scroll_end(&fx) < old_scroll_left;
        fx.layout(200.0);

        let active = fx.active_index();
        let state = &fx.state.tabs[active].state;
        let (fill_left, _) = fx.fill_client_span(active);
        let hidden = state.connected_clipping.hidden;
        results.push(Row {
            clamped_before_layout,
            active_index: active,
            scrolled_to_end: fx.state.container.scroll_left == scroll_end(&fx),
            fill_left: fill_left + fx.state.container.scroll_left,
            hidden,
            fill_displayed: !hidden,
            outline_displayed: !hidden,
            outline_color: "rgb(255, 170, 0)",
        });
    }
    let expected: Vec<Row> = [4usize, 3, 2]
        .iter()
        .map(|&count| Row {
            clamped_before_layout: true,
            active_index: count - 1,
            scrolled_to_end: true,
            fill_left: (count - 1) as f64 * 160.0,
            hidden: false,
            fill_displayed: true,
            outline_displayed: true,
            outline_color: "rgb(255, 170, 0)",
        })
        .collect();
    assert_eq!(results, expected);
}

//#region Alt (#331979), os quatro últimos casos do original

#[test]
fn alt_swaps_the_close_action_of_the_hovered_tab_only() {
    let mut fx = Fixture::new();
    let mut actions = vec![fx.tab_actions()];

    fx.hover_tab(0);
    fx.alt(true);
    actions.push(fx.tab_actions());

    fx.alt(false);
    actions.push(fx.tab_actions());

    assert_eq!(actions, vec![vec!["close", "close"], vec!["closeOthers", "close"], vec!["close", "close"]]);
}

#[test]
fn alt_does_not_stay_armed_when_the_window_loses_focus() {
    let mut fx = Fixture::new();
    fx.hover_tab(0);
    fx.alt(true);

    let mut actions = vec![fx.tab_actions()];

    // Alt+Tab para outro aplicativo: o `keyup` do Alt vai para ele e nunca chega aqui.
    fx.state.handle_window_focus_change(false);
    fx.alt(false); // ModifierKeyEmitter.getInstance().resetKeyStatus()
    actions.push(fx.tab_actions());

    // O Alt reportado de novo quando o foco volta não pode trocar a ação de uma aba que ainda estava sob o mouse.
    fx.state.handle_window_focus_change(true);
    fx.alt(true);
    actions.push(fx.tab_actions());

    // Passar o mouse numa aba de novo arma a troca como de costume.
    fx.hover_tab(0);
    actions.push(fx.tab_actions());

    assert_eq!(
        actions,
        vec![vec!["closeOthers", "close"], vec!["close", "close"], vec!["close", "close"], vec!["closeOthers", "close"]]
    );
}

#[test]
fn alt_is_revalidated_from_mouse_events_over_the_tabs() {
    let mut fx = Fixture::new();
    fx.hover_tab(0);
    fx.alt(true);

    let mut actions = vec![fx.tab_actions()];

    // O `keyup` do Alt foi para outro aplicativo: só o próximo evento do mouse revela que o Alt já não está pressionado.
    fx.mouse_event_over_tabs(false);
    actions.push(fx.tab_actions());

    assert_eq!(actions, vec![vec!["closeOthers", "close"], vec!["close", "close"]]);
}

#[test]
fn alt_is_revalidated_when_pressing_the_tab_action_without_moving_the_mouse() {
    let mut fx = Fixture::new();
    fx.hover_tab(0);
    fx.alt(true);

    let mut actions = vec![fx.tab_actions()];

    fx.mouse_event_over_tabs(false); // mouseDownOnTabAction(0, false)
    actions.push(fx.tab_actions());

    assert_eq!(actions, vec![vec!["closeOthers", "close"], vec!["close", "close"]]);
}

#[test]
fn connected_sticky_offsets_follow_rendered_widths_without_preserving_filename_extensions() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 20.0;
        options.tab_sizing_fixed_max_width = 20.0;
        options.pinned_tab_sizing = PinnedTabSizing::Shrink;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    // `getName()` devolve `example.dockerignore`.
    let long_extension = fx.suite.disposables.add_rc(NamedInput::new("example.dockerignore", "/path/example.dockerignore", None));
    let long_extension: EditorInputRef = long_extension;
    open!(fx.model, long_extension, index: 0, pinned: true, sticky: true);
    fx.open_editors();
    for (index, editor) in fx.model.get_editors(EditorsOrder::Sequential, None).into_iter().enumerate() {
        fx.model.stick(&editor);
        fx.state.handle_sticky_editor_change(index);
    }
    fx.layout(400.0);

    let widths = fx.geometry().widths;
    let label = &fx.state.tabs[0].state.label;
    // O `text-overflow: ellipsis` do contêiner do rótulo é pintura da renderização nativa.
    assert_eq!(
        (
            format!("{}{}", label.name_prefix.as_deref().unwrap_or(""), label.name.as_deref().unwrap_or("")),
            label.description.clone(),
            fx.state.tabs.iter().map(|tab| tab.state.left).collect::<Vec<_>>(),
        ),
        ("example.dockerignore".to_string(), String::new(), vec![Some(0.0), Some(widths[0]), Some(widths[0] + widths[1])])
    );
}

#[test]
fn marks_the_connected_clipping_edge_when_it_follows_compact_sticky_tabs() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.pinned_tab_sizing = PinnedTabSizing::Compact;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    let first = fx.model.get_editor_by_index(0).expect("aba 0");
    fx.model.stick(&first);
    fx.state.handle_sticky_editor_change(0);
    fx.activate(1);
    fx.layout(400.0);

    assert!(fx.state.container.overflow_edge.adjacent_sticky);
}

#[test]
fn only_the_bottom_wrapped_row_joins_the_document_and_upper_row_resets_when_unwrapped() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 120.0;
        options.tab_sizing_fixed_max_width = 120.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.layout(150.0);
    let upper_row = |fx: &Fixture| fx.state.tabs.iter().map(|tab| tab.state.connected_tab_upper_row).collect::<Vec<_>>();
    let top_row = |fx: &Fixture| fx.state.tabs.iter().map(|tab| tab.state.connected_tab_top_row).collect::<Vec<_>>();
    let wrapped = upper_row(&fx);
    let wrapped_top = top_row(&fx);
    // `getComputedStyle(fill).top` da aba superior: o `fill-block-inset` de `size20`. O `::after` do ombro (`content: none`) é pintura.
    let upper_inset = tab_fill_rect(&fx.box_context(0)).top;
    fx.layout(400.0);

    assert_eq!(
        (wrapped, wrapped_top, upper_inset, upper_row(&fx), top_row(&fx)),
        (vec![true, false], vec![true, false], 2.0, vec![false, false], vec![true, true])
    );
}

#[test]
fn connected_fixed_widths_settle_wrapping_on_the_first_scheduled_layout() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.close_all_editors();
    // Seis entradas de nome `.markdown` com recursos distintos, a primeira ativa.
    for index in 0..6usize {
        let concrete = fx.suite.disposables.add_rc(NamedInput::new(".markdown", &format!("/path/file{index}.markdown"), None));
        let editor: EditorInputRef = concrete;
        open!(fx.model, editor, index: index as i64, pinned: true, active: index == 0);
    }
    fx.open_editors();
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 50.0;
        options.tab_sizing_fixed_max_width = 160.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
        options.has_icons = false;
    });

    let mut unstable_layouts = Vec::new();
    for width in [230.0, 360.0, 400.0, 420.0, 440.0, 460.0, 480.0] {
        fx.layout(width);
        let first_pass = (fx.geometry().tops, fx.state.container.wrapping);
        fx.layout(width);
        let second_pass = (fx.geometry().tops, fx.state.container.wrapping);
        if first_pass != second_pass {
            unstable_layouts.push((width as i64, first_pass, second_pass));
        }
    }
    let widths_within_configured_range = fx.geometry().widths.iter().all(|width| (50.0..=160.0).contains(width));

    assert_eq!((unstable_layouts, widths_within_configured_range), (Vec::new(), true));
}

//#endregion

//#region Controle de duas barras (`MultiRowEditorControl`) e abas conectadas
//
// `createInstance(MultiRowEditorControl, container, editorPartsView, groupsView, groupView, model, undefined, false, false)` é
// `Fixture::create_multi_row`, e `layoutConnectedGroup(group, width, multiRowControl)` é `Fixture::row_layout`. As barras são
// `tabBars[0]` (`row().sticky`) e `tabBars[1]` (`row().unsticky`); as classes `empty`, `wrapping`, `connected-tab-top-row` e
// `connected-tabs-wrapping` do original são o estado que o controle escreve (`empty`, `container.wrapping`,
// `TabState::connected_tab_top_row` e `connected_tabs_wrapping()`).

#[test]
fn the_first_nonempty_connected_tab_bar_owns_the_top_row_after_the_pinned_row_empties() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.create_multi_row(false);
    fx.row_open_editors();
    fx.row_layout(400.0);
    let unsticky_top_rows = |fx: &Fixture| fx.row().unsticky.tabs.iter().map(|tab| tab.state.connected_tab_top_row).collect::<Vec<_>>();
    let initially_empty = (fx.row().sticky.empty, unsticky_top_rows(&fx));

    let sticky_editor = fx.model.get_editor_by_index(0).expect("aba 0");
    fx.model.stick(&sticky_editor);
    fx.row_mut().stick_editor(&sticky_editor);
    fx.row_layout(400.0);
    let with_pinned_row = unsticky_top_rows(&fx);

    fx.model.unstick(&sticky_editor);
    fx.row_mut().unstick_editor(&sticky_editor);
    fx.row_layout(400.0);

    assert_eq!(
        (initially_empty, with_pinned_row, fx.row().sticky.empty, unsticky_top_rows(&fx)),
        ((true, vec![true, true]), vec![false], true, vec![true, true])
    );
}

#[test]
fn connected_wrapping_state_aggregates_across_separate_pinned_and_unpinned_rows() {
    let mut fx = Fixture::new();
    fx.connected_group();
    for index in fx.model.count()..6 {
        fx.open_pinned_file(&format!("aggregate-{index}.ts"), false);
    }
    let (old_options, new_options) = fx.set_options(|options| {
        options.pinned_tabs_on_separate_row = true;
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 120.0;
        options.tab_sizing_fixed_max_width = 120.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.create_multi_row(false);
    fx.row_update_options(&old_options, &new_options);
    let sticky_editor = fx.model.get_editor_by_index(0).expect("aba 0");
    fx.model.stick(&sticky_editor);
    fx.row_open_editors();
    fx.row_layout(150.0);
    let initially_wrapped = (fx.row().sticky.container.wrapping, fx.row().unsticky.container.wrapping, fx.row().connected_tabs_wrapping());

    fx.row_update_editor_dirty(&sticky_editor);
    fx.row_frame();

    assert_eq!((initially_wrapped, fx.row().connected_tabs_wrapping()), ((false, true, true), true));
}

#[test]
fn connected_wrapped_last_tab_adds_its_shoulder_to_the_editor_actions_margin() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 120.0;
        options.tab_sizing_fixed_max_width = 120.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });

    fx.layout(150.0);
    // `tabsContainer.style.setProperty('--last-tab-margin-right', '17px')` e
    // `tabsContainer.style.setProperty('--modern-ui-connected-tab-shoulder-radius', '5px')`.
    fx.state.container.last_tab_margin_right = 17.0;
    fx.layout.shoulder_radius.set(5.0);
    let last_tab_active = fx.state.tabs.last().expect("última aba").state.active;
    let margin = *fx.geometry().margins.last().expect("margem da última aba");

    assert_eq!((fx.state.container.wrapping, last_tab_active, margin), (true, false, 22.0));
}

#[test]
fn connected_tab_positions_and_spacing_stay_fixed_when_changing_selection() {
    let mut fx = Fixture::new();
    fx.connected_group();
    for index in 2..6 {
        fx.open_pinned_file(&format!("file{index}.txt"), false);
    }
    fx.open_editors();
    // `{ left: offsetLeft, top: offsetTop, width: getBoundingClientRect().width, margin: marginRight }` de cada aba.
    let measure = |fx: &Fixture| -> Vec<[f64; 4]> {
        let geometry = fx.geometry();
        (0..fx.state.tabs.len()).map(|index| [geometry.offsets[index], geometry.tops[index], geometry.widths[index], geometry.margins[index]]).collect()
    };
    let mut mismatches: Vec<String> = Vec::new();
    // O original repete tudo com `hc-black` ligado e desligado na raiz. A classe só troca as cores e as bordas das abas
    // (pintura da renderização nativa) e a geometria do `Layout` não lê a raiz, então as duas voltas medem o mesmo e uma
    // só cobre as duas.
    for tab_sizing in [TabSizing::Fit, TabSizing::Fixed] {
        let sizing = if tab_sizing == TabSizing::Fit { "fit" } else { "fixed" };
        for wrap_tabs in [false, true] {
            fx.update_options(|options| {
                options.tab_sizing = tab_sizing;
                options.wrap_tabs = wrap_tabs;
                options.tab_sizing_fixed_min_width = 120.0;
                options.tab_sizing_fixed_max_width = 120.0;
                options.editor_actions_location = EditorActionsLocation::Hidden;
            });
            let widths: &[f64] = if wrap_tabs { &[245.0, 365.0] } else { &[1000.0] };
            for &width in widths {
                fx.layout(width);
                let baseline = measure(&fx);
                for active_index in 0..fx.state.tabs.len() {
                    fx.activate(active_index);
                    fx.layout(width);
                    let actual = measure(&fx);
                    if actual != baseline {
                        mismatches.push(format!("{sizing} wrap={wrap_tabs} width={width} active={active_index} baseline={baseline:?} actual={actual:?}"));
                    }

                    let geometry = fx.geometry();
                    let scroll = fx.state.container.scroll_left;
                    for index in 1..fx.state.tabs.len() {
                        let previous = fx.layout.tab_rect(&geometry, index - 1, scroll);
                        let current = fx.layout.tab_rect(&geometry, index, scroll);
                        if geometry.tops[index - 1] == geometry.tops[index] && (current.0 - previous.1).abs() > 0.01 {
                            mismatches.push(format!("{sizing} wrap={wrap_tabs} width={width} active={active_index} gap_after={} gap={}", index - 1, current.0 - previous.1));
                        }
                    }
                }
            }
        }
    }

    assert_eq!(mismatches, Vec::<String>::new());
}

// Os casos seguintes medem a caixa do `.action-label` e a do `.tab-fill`: o fixture as lê da árvore nativa de widgets da aba
// (`tab_action_geometry` e `tab_fill_rect` sobre o `TabBoxContext` que o `Layout` monta), como os anteriores medem as abas. A
// raiz do original ganha `vs`, `vs-dark`, `hc-black` e `hc-light` e `modern-ui-compact`; essas classes só trocam cor, borda e
// raio (pintura da renderização nativa) e o `Layout` não as lê, então cada volta de tema mede o mesmo e uma só cobre todas.

/// `[resting, focused]` do `.action-label` de cada aba: `resting` antes do `action.focus()` e `focused` com o foco nele.
fn measure_action_bounds(fx: &mut Fixture) -> Vec<[[f64; 4]; 2]> {
    (0..fx.state.tabs.len())
        .map(|index| {
            let resting = fx.action_bounds(index);
            fx.focus_action(index);
            let focused = fx.action_bounds(index);
            [resting, focused]
        })
        .collect()
}

#[test]
fn connected_close_action_bounds_stay_fixed_across_selection_and_focus_changes() {
    let mut fx = Fixture::new();
    fx.connected_group();
    for index in 2..6 {
        fx.open_pinned_file(&format!("file{index}.txt"), false);
    }
    fx.open_editors();
    let mut mismatches: Vec<String> = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        for tab_action_location in [TabActionLocation::Right, TabActionLocation::Left] {
            for wrap_tabs in [false, true] {
                fx.update_options(|options| {
                    options.tab_height = tab_height;
                    options.tab_action_location = tab_action_location;
                    options.wrap_tabs = wrap_tabs;
                    options.tab_sizing = TabSizing::Fixed;
                    options.tab_sizing_fixed_min_width = 120.0;
                    options.tab_sizing_fixed_max_width = 120.0;
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                });
                let width = if wrap_tabs { 245.0 } else { 1000.0 };
                fx.layout(width);
                let baseline: Vec<[[f64; 4]; 2]> = measure_action_bounds(&mut fx).into_iter().map(|[resting, _]| [resting, resting]).collect();
                for active_index in 0..fx.state.tabs.len() {
                    fx.activate(active_index);
                    fx.layout(width);
                    let actual = measure_action_bounds(&mut fx);
                    if actual != baseline {
                        mismatches.push(format!(
                            "{tab_height:?} {tab_action_location:?} wrap={wrap_tabs} active={active_index} baseline={baseline:?} actual={actual:?}"
                        ));
                    }
                }
            }
        }
    }

    assert_eq!((fx.state.tabs.len(), mismatches), (fx.model.count(), Vec::<String>::new()));
}

#[test]
fn connected_fills_share_pill_gutters_and_round_corners_away_from_the_frame() {
    let mut fx = Fixture::new();
    fx.connected_group();
    for index in 2..6 {
        fx.open_pinned_file(&format!("file{index}.txt"), false);
    }
    fx.open_editors();

    // `[compact, wrapTabs, activeIndex, index, insets]`. Os `corners` (`borderTopLeftRadius` e os demais) e o `leftBorder`
    // são pintura da renderização nativa; ficam as `insets`: `[surface.left - bounds.left, bounds.right - surface.right,
    // surface.top - bounds.top]` do `.tab-fill`.
    type Inset = (bool, usize, usize, [f64; 3]);
    let mut actual: Vec<Inset> = Vec::new();
    let mut expected: Vec<Inset> = Vec::new();
    for wrap_tabs in [false, true] {
        fx.update_options(|options| {
            options.wrap_tabs = wrap_tabs;
            options.tab_sizing = TabSizing::Fixed;
            options.tab_sizing_fixed_min_width = 120.0;
            options.tab_sizing_fixed_max_width = 120.0;
            options.editor_actions_location = EditorActionsLocation::Hidden;
        });
        for active_index in [0usize, 1, 4, 5] {
            fx.activate(active_index);
            fx.layout(if wrap_tabs { 245.0 } else { 1000.0 });
            let geometry = fx.geometry();
            for index in 0..fx.state.tabs.len() {
                let fill = tab_fill_rect(&fx.box_context(index));
                let row_start = index == 0 || geometry.tops[index - 1] != geometry.tops[index];
                let upper = fx.state.tabs[index].state.connected_tab_upper_row;
                let is_active = index == active_index;
                actual.push((wrap_tabs, active_index, index, [fill.left, geometry.widths[index] - fill.right, fill.top]));
                let insets = if is_active && !upper {
                    [if wrap_tabs && row_start { -2.0 } else { 0.0 }, 0.0, if wrap_tabs { 2.0 } else { -1.0 }]
                } else {
                    [2.0, 2.0, if wrap_tabs { 2.0 } else { 3.5 }]
                };
                expected.push((wrap_tabs, active_index, index, insets));
            }
        }
    }

    assert_eq!(actual, expected);
}

#[test]
fn fit_sized_connected_row_markers_stay_consistent_at_wrapping_boundaries() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fit;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    for index in 2..4 {
        fx.open_pinned_file(&format!("file{index}.txt"), false);
    }
    fx.open_editors();
    fx.layout(300.0);
    let mut mismatches: Vec<String> = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        // `container.classList.toggle('compact-height', ...)` é a classe que o `Layout` já lê de `tab_height`.
        fx.update_options(|options| options.tab_height = tab_height);
        fx.layout(300.0);
        let widths = fx.geometry().widths;
        let boundary = widths[0] + widths[1];
        let ascending: Vec<f64> = (0..21).map(|index| boundary - 10.0 + index as f64).collect();
        let sweep: Vec<f64> = ascending.iter().chain(ascending.iter().rev()).copied().collect();
        for width in sweep {
            for active_index in [0usize, 1, 3] {
                fx.activate(active_index);
                fx.layout(width);
                let wrapping = fx.state.container.wrapping;
                let tops = fx.geometry().tops;
                let count = fx.state.tabs.len();
                for index in 0..count {
                    let expected = (tops[index] == tops[0], tops[index] != tops[count - 1], wrapping && (index == count - 1 || tops[index] != tops[index + 1]));
                    let state = &fx.state.tabs[index].state;
                    let actual = (state.connected_tab_top_row, state.connected_tab_upper_row, state.last_in_row);
                    if actual != expected {
                        mismatches.push(format!("{tab_height:?} width={width} active={active_index} index={index} expected={expected:?} actual={actual:?}"));
                    }
                }
            }
        }
    }

    assert_eq!(mismatches, Vec::<String>::new());
}

#[test]
fn selected_wrapped_tabs_and_focused_actions_use_the_document_surface_on_every_row() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 120.0;
        options.tab_sizing_fixed_max_width = 120.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    // `[activeIndex, actionFocused, upperRow]`. As cores computadas do `.tab-fill` e do `.tab-actions`, o `:focus` do CSS e o
    // contorno `1px solid` dos temas `hc-*` são pintura da renderização nativa; o controle decide qual aba é a ativa, se ela
    // está na linha de cima (`connected-tab-upper-row`) e se a ação existe para receber o foco.
    let mut measurements = Vec::new();
    let mut expected = Vec::new();
    for active_index in [0usize, 1] {
        fx.activate(active_index);
        fx.layout(150.0);
        let index = fx.active_index();
        // `tab.querySelector('.tab-actions .action-label')` precisa existir: `action_geometry` falha sem ele.
        let has_action = tab_action_geometry(&TabCssClasses::of(&fx.state.tabs[index].state, &fx.state.container), &fx.box_context(index)).is_some();
        fx.focus_action(index);
        measurements.push((active_index, index, has_action, fx.state.tabs[index].state.connected_tab_upper_row));
        expected.push((active_index, active_index, true, active_index == 0));
    }

    assert_eq!(measurements, expected);
}

/// `connected_active_tab_paint` da aba `index`, com as classes que o controle escreveu nela e no contêiner. O grupo é o ativo
/// (`connectedGroup()` cria `.editor-group-container.active`).
fn active_tab_paint(layout: &Layout, bar: &MultiEditorTabsControlState, index: usize, colors: &ConnectedTabColors, high_contrast: bool, hovered: bool) -> ConnectedActiveTabPaint {
    let state = &bar.tabs[index].state;
    let input = ConnectedTabPaintInput {
        active: state.active,
        upper_row: state.connected_tab_upper_row,
        wrapping: bar.container.wrapping,
        upper_bar: bar.container.connected_tab_upper_bar,
        starts_row: layout.bar_box_context(bar, index).starts_row,
        left_edge: state.connected_clipping.left_edge,
        hovered,
        multi_selected: state.multi_selected,
        high_contrast,
        group_active: true,
    };

    connected_active_tab_paint(&input, colors, &ModernSpacing::default())
}

/// `getComputedStyle(...).boxShadow` de um ombro: a cor antes dos quatro comprimentos.
fn shadow_text(shadow: &ShoulderShadow) -> String {
    format!("{} {}px {}px {}px {}px", css_rgb(&shadow.color), shadow.offset_x, shadow.offset_y, shadow.blur, shadow.spread)
}

#[test]
fn customized_connected_backgrounds_fill_the_shoulders_without_straight_masks_covering_their_curves() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.style.setProperty('--modern-ui-connected-tab-surface', '#123456')`, `--modern-ui-editor-tab-custom-active-background` e
    // `--modern-ui-editor-tab-custom-border`
    let colors = ConnectedTabColors {
        surface: Some(Color::from_hex("#123456")),
        custom_active_background: Some(Color::from_hex("#164e63")),
        custom_border: Some(Color::from_hex("#22d3ee")),
        ..ConnectedTabColors::default()
    };
    fx.activate(1);
    fx.layout(320.0);
    let paint = active_tab_paint(&fx.layout, &fx.state, fx.active_index(), &colors, false, false);

    #[derive(Debug, PartialEq)]
    struct Measurement {
        fill: String,
        fill_border: String,
        /// `[borderLeftColor, borderBottomWidth, clipPath]` do `.tab-connected-edge`.
        outline: [String; 3],
        shoulders: [String; 2],
        masks: [&'static str; 2],
    }

    let shoulders = [&paint.before, &paint.after].map(|shoulder| shadow_text(&shoulder.as_ref().expect("a aba ativa tem os dois ombros").shadow));
    let measurement = Measurement {
        fill: css_rgb(&paint.background),
        fill_border: css_rgb(&paint.fill_border_color),
        outline: [css_rgb(&paint.edge_border_left_color), format!("{}px", paint.edge_border_bottom_width), format!("inset(0px 0px {}px)", paint.edge_clip_bottom_inset)],
        shoulders,
        masks: [if paint.straight_masks { "\"\"" } else { "none" }; 2],
    };

    assert_eq!(
        measurement,
        Measurement {
            fill: "rgb(22, 78, 99)".to_string(),
            fill_border: "rgba(0, 0, 0, 0)".to_string(),
            outline: ["rgb(34, 211, 238)".to_string(), "0px".to_string(), "inset(0px 0px 7px)".to_string()],
            shoulders: ["rgb(22, 78, 99) 3.5px 3.5px 0px 3.5px".to_string(), "rgb(22, 78, 99) -3.5px 3.5px 0px 3.5px".to_string()],
            masks: ["none", "none"],
        }
    );
}

#[test]
fn hc_connected_multi_selection_keeps_the_selected_state_on_the_active_tab() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `root.classList.add('hc-black')` e as cores `contrastActiveBorder` e `focusBorder` só trocam o traço (pintura): o `outline`
    // do `.tab-fill`, a cor do contorno da tampa, as quatro bordas do `.tab-border-top-container` e o `blur()` não são estado.
    fx.open_editors();
    let active = fx.model.active_editor().expect("aba ativa");
    let second = fx.model.get_editor_by_index(1).expect("aba 1");
    fx.set_selection(&active, &[second]);
    fx.update_editor_selections();
    fx.layout(320.0);
    let state = &fx.state.tabs[fx.active_index()].state;

    assert_eq!((state.multi_selected, state.aria_selected), (true, true));
}

#[test]
fn regular_theme_connected_active_multi_selection_leaves_the_document_join_open() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `root.style.setProperty('--vscode-tab-activeBorderTop', ...)` e o `TestColorTheme` só definem a cor da tampa e do ombro
    // (`capColor`, `capBottom`, `fillBottom`, `shoulderColor`, pintura da renderização nativa). Fica o que o controle decide:
    // a aba ativa é multisselecionada e não fica na linha de cima, e por isso se junta ao documento (`attachedSelection`).
    fx.activate(1);
    let active = fx.model.get_editor_by_index(1).expect("aba 1");
    let first = fx.model.get_editor_by_index(0).expect("aba 0");
    let mut results = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        for wrap_tabs in [false, true] {
            fx.update_options(|options| {
                options.tab_height = tab_height;
                options.wrap_tabs = wrap_tabs;
                options.editor_actions_location = EditorActionsLocation::Hidden;
                options.tab_sizing = TabSizing::Fixed;
                options.tab_sizing_fixed_min_width = 120.0;
                options.tab_sizing_fixed_max_width = 120.0;
            });
            fx.set_selection(&active, &[first.clone()]);
            fx.update_editor_selections();
            fx.layout(if wrap_tabs { 150.0 } else { 600.0 });
            let state = &fx.state.tabs[fx.active_index()].state;
            results.push((tab_height == TabHeight::Compact, wrap_tabs, state.multi_selected && !state.connected_tab_upper_row));
        }
    }
    let expected: Vec<(bool, bool, bool)> = results.iter().map(|&(compact, wrap_tabs, _)| (compact, wrap_tabs, true)).collect();

    assert_eq!(results, expected);
}

#[test]
fn hc_connected_selection_keeps_its_outline_independent_of_modified_highlighting() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `--vscode-contrastBorder`, `--vscode-focusBorder`, `--vscode-contrastActiveBorder` e `--vscode-tab-selectedBorderTop` do
    // original viram as cores do tema. O `selectedStroke` (`rgb(255, 170, 0)`), a espessura do realce (o gradiente de
    // `size20`), o `clippedToFill` e as quatro bordas computadas são pintura da renderização nativa; ficam a cor do realce de
    // topo (`accent.color`, presente só nas abas modificadas com o destaque ligado), a estabilidade da geometria da aba e
    // da ação e o `aria-selected`.
    let mut editors = fx.model.get_editors(EditorsOrder::Sequential, None);
    for name in ["modifiedOnly", "modifiedSelected"] {
        let concrete = fx.suite.disposables.add_rc(NamedInput::new(name, &format!("/path/{name}.ts"), None));
        concrete.set_dirty();
        let editor: EditorInputRef = concrete;
        open!(fx.model, editor, index: fx.model.count() as i64, pinned: true, active: false);
        editors.push(editor);
    }
    fx.open_editors();
    fx.state.set_colors(TabColors {
        tab_active_modified_border: Some("#ffffff".to_string()),
        tab_inactive_modified_border: Some("#ffffff".to_string()),
        tab_selected_border_top: Some("#ffaa00".to_string()),
        ..TabColors::default()
    });

    type Context = (bool, bool, bool, usize, bool, usize);
    type Measure = (Context, Option<String>, bool, bool);
    let mut measurements: Vec<Measure> = Vec::new();
    let mut expected: Vec<Measure> = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        for wrap_tabs in [false, true] {
            for highlight_modified_tabs in [false, true] {
                fx.update_options(|options| {
                    options.tab_height = tab_height;
                    options.wrap_tabs = wrap_tabs;
                    options.highlight_modified_tabs = highlight_modified_tabs;
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                    options.tab_sizing = TabSizing::Fixed;
                    options.tab_sizing_fixed_min_width = 120.0;
                    options.tab_sizing_fixed_max_width = 120.0;
                });
                let width = if wrap_tabs { 150.0 } else { 600.0 };
                for active_index in [0usize, 3] {
                    fx.activate(active_index);
                    fx.set_selection(&editors[active_index], &[]);
                    fx.update_editor_selections();
                    fx.layout(width);
                    let geometry: Vec<[f64; 7]> = (0..fx.state.tabs.len()).map(|index| fx.tab_and_action_bounds(index)).collect();
                    for multi_selected in [false, true, false] {
                        let selection: Vec<EditorInputRef> = if multi_selected {
                            editors.iter().enumerate().filter(|&(index, _)| index != active_index && index != 2).map(|(_, editor)| editor.clone()).collect()
                        } else {
                            Vec::new()
                        };
                        fx.set_selection(&editors[active_index], &selection);
                        fx.update_editor_selections();
                        fx.layout(width);
                        for index in 0..fx.state.tabs.len() {
                            let state = &fx.state.tabs[index].state;
                            let selected = multi_selected && index != 2;
                            let modified = index >= 2 && highlight_modified_tabs;
                            let context = (tab_height == TabHeight::Compact, wrap_tabs, highlight_modified_tabs, active_index, multi_selected, index);
                            measurements.push((
                                context,
                                if state.dirty_border_top { state.dirty_border_top_color.clone() } else { None },
                                geometry[index] == fx.tab_and_action_bounds(index),
                                state.aria_selected,
                            ));
                            expected.push((context, modified.then(|| "#ffffff".to_string()), true, index == active_index || selected));
                        }
                    }
                }
            }
        }
    }

    assert_eq!(measurements, expected);
}

#[test]
fn freestanding_connected_neighbors_keep_one_stroke_each_and_equal_gutters_without_moving_hit_targets() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `root.classList.add('hc-light')` e o `borderLeftWidth` do `.tab-fill` vizinho da ativa (`1px`, o traço de
    // `strokeThickness`) são pintura da renderização nativa; ficam a folga entre os preenchimentos e o contato dos alvos.
    let concrete = fx.suite.disposables.add_rc(NamedInput::new("third.ts", "/path/third.ts", None));
    let third: EditorInputRef = concrete;
    let count = fx.model.count() as i64;
    open!(fx.model, third, index: count, pinned: true, active: true);
    fx.open_editors();
    fx.layout(600.0);
    let (_, first_right) = fx.fill_client_span(0);
    let (second_left, _) = fx.fill_client_span(1);
    let inactive_gap = second_left - first_right;
    fx.activate(0);
    fx.layout(600.0);
    let geometry = fx.geometry();
    let scroll = fx.state.container.scroll_left;
    let hit_targets_contiguous = (1..fx.state.tabs.len()).all(|index| fx.layout.tab_rect(&geometry, index, scroll).0 == fx.layout.tab_rect(&geometry, index - 1, scroll).1);

    assert_eq!((inactive_gap, hit_targets_contiguous), (4.0, true));
}

#[test]
fn single_row_connected_inactive_tabs_use_dividers_while_retaining_high_contrast_interaction_strokes() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `root.style.setProperty('--vscode-tab-border', '#abcdef')`, `--vscode-tab-divider`, `--vscode-focusBorder` e
    // `--vscode-contrastActiveBorder`, com o `rgb(...)` que o `getComputedStyle` devolve.
    let rgb = |hex: &str| css_rgb(&Color::from_hex(hex));
    let (tab_border, focus_border, contrast_active_border) = (rgb("#abcdef"), rgb("#ffaa00"), rgb("#ffaa00"));
    let divider_color = Color::from_hex("#abcdef");
    fx.open_named_editors(&names(&["first.ts", "second.ts", "third.ts"]));
    fx.activate(0);
    let peer = 1;
    let sides = [StrokeSide::Top, StrokeSide::Right, StrokeSide::Bottom, StrokeSide::Left];

    // O `.tab-fill` do vizinho com as classes `hovered` e `selected` e o foco que o caso liga e desliga no elemento.
    let stroke = |fx: &Fixture, high_contrast: bool, hovered: bool, selected: bool, focused: bool| {
        let state = &fx.state.tabs[peer].state;
        high_contrast_connected_tab_fill_stroke(&TabFillStrokeInput {
            wrapping: fx.state.container.wrapping,
            upper_bar: fx.state.container.connected_tab_upper_bar,
            active: state.active,
            selected,
            multi_selected: state.multi_selected,
            dirty_border_top: state.dirty_border_top,
            upper_row: state.connected_tab_upper_row,
            hovered,
            focused,
            focus_within: focused,
            tab_border: Some(tab_border.as_str()),
            contrast_active_border: Some(contrast_active_border.as_str()),
            focus_border: Some(focus_border.as_str()),
            spacing_size20: SPACING_SIZE20,
            regular_theme: !high_contrast,
            ..TabFillStrokeInput::default()
        })
    };
    // `getComputedStyle(tabs[index].querySelector('.tab-divider'))`
    let divider_of = |fx: &Fixture, index: usize, high_contrast: bool| {
        let tabs = &fx.state.tabs;
        tab_divider(
            &TabDividerInput {
                modern_ui_tabs: fx.parent.in_modern_ui_tabs,
                connected: fx.layout.is_connected(),
                high_contrast,
                wrapping: fx.state.container.wrapping,
                upper_bar: fx.state.container.connected_tab_upper_bar,
                active: tabs[index].state.active,
                previous_active: index > 0 && tabs[index - 1].state.active,
                follows_tab_in_row: index > 0 && !tabs[index - 1].state.last_in_row,
            },
            &ModernSpacing::default(),
            Some(&divider_color),
        )
    };
    let rect = |fx: &Fixture, index: usize| fx.layout.tab_rect(&fx.geometry(), index, fx.state.container.scroll_left);

    #[derive(Debug, PartialEq)]
    struct Row {
        theme: &'static str,
        rtl: bool,
        compact: bool,
        resting_border: [String; 4],
        /// `[display, backgroundColor, width, height]`
        divider: [String; 4],
        divider_at_boundary: bool,
        active_neighbor_divider: &'static str,
        css_focused: bool,
        interaction_strokes: Option<[String; 3]>,
        geometry_stable: bool,
        control_height: f64,
    }

    let mut results = Vec::new();
    for theme in ["vs", "vs-dark", "hc-black", "hc-light"] {
        let high_contrast = theme.starts_with("hc-");
        for rtl in [false, true] {
            // `root.dir = direction`
            fx.layout.rtl.set(rtl);
            for tab_height in [TabHeight::Default, TabHeight::Compact] {
                fx.update_options(|options| {
                    options.wrap_tabs = false;
                    options.tab_height = tab_height;
                });
                fx.layout(600.0);
                let resting = stroke(&fx, high_contrast, false, false, false);
                let resting_border = sides.map(|side| describe_stroke_color(resting.color(side)));
                let bounds = rect(&fx, peer);
                let hover_stroke = describe_stroke_color(stroke(&fx, high_contrast, true, false, false).color(StrokeSide::Left));
                let selected_stroke = describe_stroke_color(stroke(&fx, high_contrast, false, true, false).color(StrokeSide::Left));
                // `peer.focus()`: o elemento tem `tabindex`, então com a janela em foco vale `:focus`.
                fx.focus_action(peer);
                let css_focused = true;
                let focus_stroke = describe_stroke_color(stroke(&fx, high_contrast, false, false, true).color(StrokeSide::Left));
                let divider = divider_of(&fx, 2, high_contrast);
                // `dividerBounds` com `inset-inline-start: 0`: a ponta de início do divisor é a da própria aba.
                let geometry = fx.geometry();
                let scroll = fx.state.container.scroll_left;
                let (divider_tab_left, divider_tab_right) = fx.layout.tab_rect(&geometry, 2, scroll);
                let (neighbor_left, neighbor_right) = fx.layout.tab_rect(&geometry, peer, scroll);
                let distance = if rtl { divider_tab_right - neighbor_left } else { divider_tab_left - neighbor_right };
                results.push(Row {
                    theme,
                    rtl,
                    compact: tab_height == TabHeight::Compact,
                    resting_border,
                    divider: [
                        if divider.displayed { "block" } else { "none" }.to_string(),
                        css_rgb(&divider.background),
                        format!("{}px", divider.width),
                        format!("{}px", divider.height),
                    ],
                    divider_at_boundary: distance.abs() < 1.0 / 64.0,
                    active_neighbor_divider: if divider_of(&fx, peer, high_contrast).displayed { "block" } else { "none" },
                    css_focused,
                    interaction_strokes: high_contrast.then(|| [hover_stroke, selected_stroke, focus_stroke]),
                    geometry_stable: bounds == rect(&fx, peer),
                    control_height: fx.get_height(),
                });
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            theme: result.theme,
            rtl: result.rtl,
            compact: result.compact,
            resting_border: std::array::from_fn(|_| "rgba(0, 0, 0, 0)".to_string()),
            divider: ["block", "rgb(171, 205, 239)", "1px", "16px"].map(str::to_string),
            divider_at_boundary: true,
            active_neighbor_divider: "none",
            css_focused: result.css_focused,
            interaction_strokes: result.theme.starts_with("hc-").then(|| {
                [
                    "rgb(171, 205, 239)".to_string(),
                    "rgb(255, 170, 0)".to_string(),
                    if result.css_focused { "rgb(255, 170, 0)" } else { "rgb(171, 205, 239)" }.to_string(),
                ]
            }),
            geometry_stable: true,
            control_height: if result.compact { 28.0 } else { 32.0 },
        })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn resting_selected_dirty_and_pinned_single_row_pills_center_their_labels_and_actions_on_the_painted_fill() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.style.setProperty('--modern-ui-editor-tab-custom-inactive-background', '#234567')` só troca a cor do preenchimento.
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/dirty.ts"), "testEditorInput"));
    concrete.set_dirty();
    let dirty_editor: EditorInputRef = concrete;
    open!(fx.model, dirty_editor, pinned: true, active: false);
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/pinned.ts"), "testEditorInput"));
    let pinned_editor: EditorInputRef = concrete;
    open!(fx.model, pinned_editor, pinned: true, active: false);
    fx.model.stick(&pinned_editor);

    #[derive(Debug, PartialEq)]
    struct Row {
        theme: &'static str,
        compact: bool,
        label_centered: bool,
        action_centered: bool,
        fill_height: f64,
    }

    let mut results = Vec::new();
    // O tema só troca cor, borda e raio (pintura): a geometria do preenchimento e do rótulo vale para os quatro.
    for theme in ["vs", "vs-dark", "hc-black", "hc-light"] {
        for tab_height in [TabHeight::Default, TabHeight::Compact] {
            fx.update_options(|options| {
                options.tab_height = tab_height;
                options.wrap_tabs = false;
                options.pinned_tab_sizing = PinnedTabSizing::Normal;
                options.tab_action_unpin_visibility = true;
            });
            fx.open_editors();
            fx.layout(900.0);
            let tabs = &fx.state.tabs;
            // `.tab:not(.active):not(.dirty):not(.sticky)`, que o caso marca com a classe `selected` (o preenchimento é o mesmo),
            // `.tab.dirty` e `.tab.sticky`.
            let selected = tabs.iter().position(|tab| !tab.state.active && !tab.state.dirty && !tab.state.sticky).expect("aba inativa e limpa");
            let dirty = tabs.iter().position(|tab| tab.state.dirty).expect("aba suja");
            let sticky = tabs.iter().position(|tab| tab.state.sticky).expect("aba fixada");
            for index in [selected, dirty, sticky] {
                let context = fx.box_context(index);
                let fill = tab_fill_rect(&context);
                let fill_center = (fill.top + fill.bottom) / 2.0;
                let action = fx.action_geometry(index).label;
                results.push(Row {
                    theme,
                    compact: tab_height == TabHeight::Compact,
                    label_centered: (fx.layout.label_center(context.tab_height) - fill_center).abs() < 1.0 / 32.0,
                    action_centered: ((action.top + action.bottom) / 2.0 - fill_center).abs() < 1.0 / 32.0,
                    fill_height: fill.height(),
                });
            }
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            theme: result.theme,
            compact: result.compact,
            label_centered: true,
            action_centered: true,
            fill_height: if result.compact { 20.0 } else { 24.0 },
        })
        .collect();
    assert_eq!(results, expected);
}

/// `[tab.top, tab.bottom, fill.top, fill.bottom]` de cada aba em coordenadas do `tabs-and-actions-container`
/// (`getBoundingClientRect()` menos o topo do contêiner).
fn strip_tab_boxes(fx: &Fixture) -> Vec<[f64; 4]> {
    let geometry = fx.geometry();
    let padding_top = fx.layout.strip_padding_top(&fx.state.container);
    (0..fx.state.tabs.len())
        .map(|index| {
            let context = fx.box_context(index);
            let fill = tab_fill_rect(&context);
            let top = padding_top + geometry.tops[index];
            [top, top + context.tab_height, top + fill.top, top + fill.bottom]
        })
        .collect()
}

#[test]
fn wrapped_fills_retain_equal_content_heights_and_only_the_active_bottom_cap_joins_the_document() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.style.setProperty('--modern-ui-connected-tab-surface', '#ffffff')` e `--modern-ui-editor-tab-custom-border`
    let colors = ConnectedTabColors {
        surface: Some(Color::from_hex("#ffffff")),
        custom_border: Some(Color::from_hex("#22d3ee")),
        ..ConnectedTabColors::default()
    };
    fx.activate(1);

    #[derive(Debug, PartialEq)]
    struct Measurement {
        compact: bool,
        strip_height: f64,
        wrapping: bool,
        upper_row: bool,
        gap: f64,
        clipping_gap: f64,
        bottom_radius: String,
        shoulder: &'static str,
        /// `[borderLeftColor, borderBottomColor]` do `::after` do `.tab-fill`.
        shoulder_stroke: [String; 2],
        shoulder_baseline_offset: f64,
        tab_content_heights: Vec<f64>,
        hitbox_heights: Vec<f64>,
        bottom_join: f64,
        row_gap: f64,
        overflow: [&'static str; 2],
        separator_offset: f64,
        connection_overlap: f64,
    }

    let mut measurements = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        fx.update_options(|options| {
            options.wrap_tabs = true;
            options.tab_height = tab_height;
            options.tab_sizing = TabSizing::Fixed;
            options.tab_sizing_fixed_min_width = 120.0;
            options.tab_sizing_fixed_max_width = 120.0;
            options.editor_actions_location = EditorActionsLocation::Hidden;
        });
        fx.layout(150.0);
        let index = fx.active_index();
        let paint = active_tab_paint(&fx.layout, &fx.state, index, &colors, false, false);
        let strip_height = fx.rendered_height();
        let boxes = strip_tab_boxes(&fx);
        let [_, tab_bottom, _, fill_bottom] = boxes[index];
        // `getBoundingClientRect().bottom` do `.tabs-container` e do `.monaco-scrollable-element`, que acabam juntos.
        let clipping_bottom = fx.layout.strip_padding_top(&fx.state.container) + fx.layout.measure_wrapping(&fx.state.container, &fx.state.tabs).tabs_container_height;
        // `::after` do `tabs-and-actions-container`: `bottom: 0` e `height: strokeThickness`.
        let separator_top = strip_height - SEPARATOR_BOTTOM - STROKE;
        let after = paint.after.as_ref().expect("a aba ativa fora da linha de cima tem o ombro de fim");
        measurements.push(Measurement {
            compact: tab_height == TabHeight::Compact,
            strip_height,
            wrapping: fx.state.container.wrapping,
            upper_row: fx.state.tabs[index].state.connected_tab_upper_row,
            gap: strip_height - fill_bottom,
            clipping_gap: strip_height - clipping_bottom,
            bottom_radius: format!("{}px", paint.fill_bottom_radius),
            shoulder: if paint.after.is_some() { "\"\"" } else { "none" },
            shoulder_stroke: [css_rgb(&after.stroke), css_rgb(&after.stroke)],
            // O `::after` do ombro tem `bottom: 0` dentro do `.tab-fill`.
            shoulder_baseline_offset: fill_bottom - paint.fill_border_bottom_width - 0.0 - (separator_top + STROKE),
            tab_content_heights: boxes.iter().enumerate().map(|(position, [_, _, top, bottom])| bottom - top - if position == 1 { 3.0 } else { 0.0 }).collect(),
            hitbox_heights: boxes.iter().map(|[top, bottom, _, _]| bottom - top).collect(),
            bottom_join: fill_bottom - tab_bottom + 2.0,
            row_gap: boxes[1][2] - boxes[0][3],
            overflow: [if fx.state.container.overflow_visible { "visible" } else { "hidden" }; 2],
            separator_offset: separator_top - tab_bottom,
            connection_overlap: fill_bottom - strip_height,
        });
    }

    let expected = |compact: bool| Measurement {
        compact,
        strip_height: if compact { 51.0 } else { 59.0 },
        wrapping: true,
        upper_row: false,
        gap: 0.0,
        clipping_gap: 0.0,
        bottom_radius: "0px".to_string(),
        shoulder: "\"\"",
        shoulder_stroke: std::array::from_fn(|_| "rgb(34, 211, 238)".to_string()),
        shoulder_baseline_offset: 0.0,
        tab_content_heights: if compact { vec![20.0, 20.0] } else { vec![24.0, 24.0] },
        hitbox_heights: if compact { vec![24.0, 24.0] } else { vec![28.0, 28.0] },
        bottom_join: 3.0,
        row_gap: 4.0,
        overflow: ["visible", "visible"],
        separator_offset: 0.0,
        connection_overlap: 0.0,
    };
    assert_eq!(measurements, vec![expected(false), expected(true)]);
}

#[test]
fn wrapped_connected_tabs_include_title_padding_in_the_available_height_constraint() {
    let mut fx = Fixture::new();
    fx.connected_group();
    fx.update_options(|options| {
        options.wrap_tabs = true;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 120.0;
        options.tab_sizing_fixed_max_width = 120.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.layout(150.0);
    // `{ strip: strip.offsetHeight, tabs: tabs.offsetHeight }`
    let wrapped_heights = (fx.rendered_height(), fx.layout.measure_wrapping(&fx.state.container, &fx.state.tabs).tabs_container_height);

    // `control.layout({ container: new Dimension(150, 33), available: new Dimension(150, wrappedHeights.tabs) })` e o quadro seguinte
    fx.layout_with_available(150.0, wrapped_heights.1);
    let constrained_by_tabs_height = fx.state.container.wrapping;

    fx.layout_with_available(150.0, wrapped_heights.0);

    assert_eq!((wrapped_heights, constrained_by_tabs_height, fx.state.container.wrapping), ((59.0, 57.0), false, true));
}

#[test]
fn three_wrapped_rows_retain_equal_tab_heights_without_a_fixed_strip_height() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.style.setProperty('--modern-ui-connected-tab-surface', '#ffffff')` só troca a cor do fundo.
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/third.ts"), "testEditorInput"));
    let third: EditorInputRef = concrete;
    // `model.openEditor(editor, { index: model.count, pinned: true, active: true })`: no fim, e ativa.
    open!(fx.model, third, index: fx.model.count() as i64, pinned: true, active: true);
    fx.open_editors();

    #[derive(Debug, PartialEq)]
    struct Measurement {
        compact: bool,
        strip_height: f64,
        tab_content_heights: Vec<f64>,
        hitbox_heights: Vec<f64>,
        bottom_cap_active: bool,
        row_gaps: Vec<f64>,
    }

    let mut measurements = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        fx.update_options(|options| {
            options.wrap_tabs = true;
            options.tab_height = tab_height;
            options.tab_sizing = TabSizing::Fixed;
            options.tab_sizing_fixed_min_width = 120.0;
            options.tab_sizing_fixed_max_width = 120.0;
            options.editor_actions_location = EditorActionsLocation::Hidden;
        });
        fx.layout(150.0);
        let boxes = strip_tab_boxes(&fx);
        measurements.push(Measurement {
            compact: tab_height == TabHeight::Compact,
            strip_height: fx.rendered_height(),
            tab_content_heights: boxes.iter().enumerate().map(|(position, [_, _, top, bottom])| bottom - top - if position == 2 { 3.0 } else { 0.0 }).collect(),
            hitbox_heights: boxes.iter().map(|[top, bottom, _, _]| bottom - top).collect(),
            bottom_cap_active: fx.state.tabs.iter().find(|tab| tab.state.connected_tab_last).expect("aba terminal").state.active,
            row_gaps: boxes.windows(2).map(|pair| pair[1][2] - pair[0][3]).collect(),
        });
    }

    assert_eq!(
        measurements,
        vec![
            Measurement { compact: false, strip_height: 87.0, tab_content_heights: vec![24.0; 3], hitbox_heights: vec![28.0; 3], bottom_cap_active: true, row_gaps: vec![4.0, 4.0] },
            Measurement { compact: true, strip_height: 75.0, tab_content_heights: vec![20.0; 3], hitbox_heights: vec![24.0; 3], bottom_cap_active: true, row_gaps: vec![4.0, 4.0] },
        ]
    );
}

#[test]
fn connected_tabs_do_not_overflow_vertically_while_dragging() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.style.setProperty('--modern-ui-connected-tab-surface', '#ffffff')` só troca a cor do fundo.
    fx.activate(1);

    #[derive(Debug, PartialEq)]
    struct Measurement {
        wrap_tabs: bool,
        wrapping: bool,
        vertical_overflow: f64,
    }

    let mut measurements = Vec::new();
    for (wrap_tabs, width) in [(false, 320.0), (true, 150.0)] {
        fx.update_options(|options| {
            options.wrap_tabs = wrap_tabs;
            options.tab_sizing = TabSizing::Fixed;
            options.tab_sizing_fixed_min_width = 120.0;
            options.tab_sizing_fixed_max_width = 120.0;
            options.editor_actions_location = EditorActionsLocation::Hidden;
        });
        fx.layout(width);
        // `tabs.classList.add('scroll')`
        fx.state.container.scroll_mode = true;
        // `tabs.scrollHeight - tabs.clientHeight`: o conteúdo vai do topo do contêiner de abas à base da última linha, mais o
        // `padding-bottom`, e a altura do contêiner é a que o layout mede.
        let padding_top = fx.layout.strip_padding_top(&fx.state.container);
        let content_bottom = strip_tab_boxes(&fx).iter().map(|[_, bottom, _, _]| bottom - padding_top).fold(0.0, f64::max);
        let client_height = fx.layout.measure_wrapping(&fx.state.container, &fx.state.tabs).tabs_container_height;
        measurements.push(Measurement {
            wrap_tabs,
            wrapping: fx.state.container.wrapping,
            vertical_overflow: (content_bottom + TABS_PADDING_BOTTOM - client_height).max(0.0),
        });
    }

    assert_eq!(
        measurements,
        vec![
            Measurement { wrap_tabs: false, wrapping: false, vertical_overflow: 0.0 },
            Measurement { wrap_tabs: true, wrapping: true, vertical_overflow: 0.0 },
        ]
    );
}

//#endregion

//#region Barras vivas (o que o original consulta com `querySelectorAll`)
//
// Os casos seguintes leem as abas de todas as barras do controle (`.tabs-and-actions-container` sem `empty`) em coordenadas do grupo,
// como `getBoundingClientRect()` devolve. [`LiveBar`] é uma barra com o topo dela (a soma das alturas das barras de cima),
// [`TabView`] é a aba com a caixa, o `.tab-fill`, o `.action-label`, o contorno e o fundo que o controle resolve para a pintura
// nativa, e a leitura `getComputedStyle(fill).backgroundColor` é [`connected_inactive_tab_background`] ou o fundo da
// `connected_active_tab_paint`. As medidas usam `Layout` como as anteriores: texto da esquerda para a direita, sem rolagem.

/// `1 / 32`, a tolerância de pixel que o original usa nas comparações de geometria com zoom.
const TOLERANCE: f64 = 1.0 / 32.0;

/// Uma barra de abas mostrada com o topo dela no grupo.
struct LiveBar<'a> {
    state: &'a MultiEditorTabsControlState,
    /// `getBoundingClientRect().top` do `.tabs-and-actions-container`, em px de layout.
    top: f64,
}

/// A aba como o original a lê do navegador, em coordenadas da tela do grupo (origem no canto do `.editor-group-container`, com o zoom
/// da raiz).
struct TabView {
    context: TabBoxContext,
    /// `getBoundingClientRect()` da `.tab` e do `.tab-fill`.
    hitbox: BoxRect,
    fill: BoxRect,
    /// `getBoundingClientRect()` do `.action-label`, ou `None` com o `.tab-actions` em `display: none`.
    label: Option<BoxRect>,
    shape: TabFillShape,
    /// `getComputedStyle(fill).backgroundColor`.
    background: Color,
}

impl Fixture {
    /// As barras que o navegador mostra, de cima para baixo: as do controle de duas barras que não estão `empty`, ou a barra única.
    fn live_bars(&self) -> Vec<LiveBar<'_>> {
        let states: Vec<&MultiEditorTabsControlState> = match &self.row {
            Some(row) => [&row.sticky, &row.unsticky].into_iter().filter(|bar| !bar.empty).collect(),
            None => vec![&self.state],
        };
        let mut top = 0.0;

        states
            .into_iter()
            .map(|state| {
                let bar = LiveBar { state, top };
                top += self.layout.measure_wrapping(&state.container, &state.tabs).tabs_and_actions_height;

                bar
            })
            .collect()
    }

    /// As abas de uma barra: a `.tab` começa depois do `padding-left` e do `padding-top` do `.tabs-and-actions-container`, e o zoom da
    /// raiz escala tudo.
    fn tab_views_of(&self, bar: &LiveBar, colors: &ConnectedTabColors) -> Vec<TabView> {
        let state = bar.state;
        let geometry = self.layout.geometry(&state.container, &state.tabs);
        let zoom = self.layout.zoom.get();
        let padding_left = self.layout.strip_padding_left(&state.container) * zoom;
        let padding_top = self.layout.strip_padding_top(&state.container);
        let spacing = ModernSpacing::default();
        let connected = self.layout.is_connected();
        let cap_radius = spacing.cap_radius(state.container.wrapping, state.container.connected_tab_upper_bar);

        (0..state.tabs.len())
            .map(|index| {
                let context = self.layout.box_context(&state.container, &state.tabs, index, geometry.widths[index]);
                let classes = TabCssClasses::of(&state.tabs[index].state, &state.container);
                let (tab_left, tab_right) = self.layout.tab_rect(&geometry, index, state.container.scroll_left);
                let left = tab_left + padding_left;
                let top = (bar.top + padding_top + geometry.tops[index]) * zoom;
                let at = |local: BoxRect| BoxRect { left: left + local.left * zoom, top: top + local.top * zoom, right: left + local.right * zoom, bottom: top + local.bottom * zoom };
                let background = if context.active {
                    active_tab_paint(&self.layout, state, index, colors, false, false).background
                } else {
                    connected_inactive_tab_background(colors, true)
                };

                TabView {
                    hitbox: BoxRect { left, top, right: tab_right + padding_left, bottom: top + context.tab_height * zoom },
                    fill: at(tab_fill_rect(&context)),
                    label: tab_action_geometry(&classes, &context).map(|action| at(action.label)),
                    shape: tab_fill_shape(connected, context.active, context.upper_row, cap_radius, &spacing),
                    background,
                    context,
                }
            })
            .collect()
    }

    /// As abas de todas as barras mostradas, na ordem do documento.
    fn tab_views(&self, colors: &ConnectedTabColors) -> Vec<TabView> {
        self.live_bars().iter().flat_map(|bar| self.tab_views_of(bar, colors)).collect()
    }
}

/// `getComputedStyle(fill).borderRadius`: um valor se os quatro cantos são iguais, senão os quatro em sequência.
fn css_border_radius(radius: [f64; 4]) -> String {
    let px = |value: f64| format!("{value}px");
    if radius.iter().all(|value| *value == radius[0]) { px(radius[0]) } else { radius.map(px).join(" ") }
}

/// Os cinco layouts de `connected single rows use panel height while wrapped and upper pinned rows retain pill height`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RowLayout {
    Single,
    Wrapped,
    Pinned,
    PinnedWrapped,
    PinnedWrappedEmpty,
}

impl RowLayout {
    const ALL: [Self; 5] = [Self::Single, Self::Wrapped, Self::Pinned, Self::PinnedWrapped, Self::PinnedWrappedEmpty];

    /// `layout === 'wrapped' || layout.startsWith('pinned-wrapped')`.
    fn wrapped(self) -> bool {
        matches!(self, Self::Wrapped | Self::PinnedWrapped | Self::PinnedWrappedEmpty)
    }

    /// `layout.startsWith('pinned')`.
    fn separate_pinned_row(self) -> bool {
        matches!(self, Self::Pinned | Self::PinnedWrapped | Self::PinnedWrappedEmpty)
    }
}

#[test]
fn connected_single_rows_use_panel_height_while_wrapped_and_upper_pinned_rows_retain_pill_height() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `group.style.setProperty('--modern-ui-editor-tab-custom-active-background', '#164e63')` e o inativo `#1e293b`
    let colors = ConnectedTabColors {
        custom_active_background: Some(Color::from_hex("#164e63")),
        custom_inactive_background: Some(Color::from_hex("#1e293b")),
        ..ConnectedTabColors::default()
    };
    // `model.openEditor(third, { pinned: true, active: false })`: depois da ativa, então o índice 1 é a `third.ts`.
    fx.open_pinned_file("third.ts", false);
    let first = fx.model.get_editor_by_index(0).expect("aba 0");

    #[derive(Debug, PartialEq)]
    struct Measurement {
        compact: bool,
        layout: RowLayout,
        rows: usize,
        tab_heights: Vec<f64>,
        fill_heights: Vec<f64>,
        content_heights: Vec<f64>,
        backgrounds: Vec<String>,
        action_targets: Vec<[f64; 2]>,
    }

    let mut measurements = Vec::new();
    for tab_height in [TabHeight::Default, TabHeight::Compact] {
        for layout in RowLayout::ALL {
            let wrapped = layout.wrapped();
            let separate_pinned_row = layout.separate_pinned_row();
            fx.set_options(|options| {
                options.tab_height = tab_height;
                options.wrap_tabs = wrapped;
                options.pinned_tabs_on_separate_row = separate_pinned_row;
                options.pinned_tab_sizing = PinnedTabSizing::Normal;
                options.tab_action_unpin_visibility = true;
                options.tab_sizing = TabSizing::Fixed;
                options.tab_sizing_fixed_min_width = 120.0;
                options.tab_sizing_fixed_max_width = 120.0;
                options.editor_actions_location = EditorActionsLocation::Hidden;
            });
            if separate_pinned_row && layout != RowLayout::PinnedWrappedEmpty {
                fx.model.stick(&first);
            } else {
                fx.model.unstick(&first);
            }
            let second = fx.model.get_editor_by_index(1).expect("aba 1");
            open!(fx.model, second, active: true);
            let width = if wrapped { 150.0 } else { 400.0 };
            if separate_pinned_row {
                // `instantiationService.createInstance(MultiRowEditorControl, ...)`
                fx.create_multi_row(false);
                fx.row_open_editors();
                fx.row_layout(width);
            } else {
                fx.create_control(false);
                fx.layout(width);
            }

            let views = fx.tab_views(&colors);
            let tops: HashSet<u64> = views.iter().map(|view| view.hitbox.top.to_bits()).collect();
            measurements.push(Measurement {
                compact: tab_height == TabHeight::Compact,
                layout,
                rows: tops.len(),
                tab_heights: views.iter().map(|view| view.hitbox.bottom - view.hitbox.top).collect(),
                fill_heights: views.iter().map(|view| view.fill.height()).collect(),
                content_heights: views
                    .iter()
                    .map(|view| {
                        let connected_cap = view.context.active && !view.context.upper_row;
                        if connected_cap {
                            // `tab.offsetHeight - parseFloat(getComputedStyle(fill).top) - (wrapped ? 2 : 0)`
                            (view.hitbox.bottom - view.hitbox.top) - (view.fill.top - view.hitbox.top) - if wrapped { 2.0 } else { 0.0 }
                        } else {
                            view.fill.height()
                        }
                    })
                    .collect(),
                backgrounds: views.iter().map(|view| css_rgb(&view.background)).collect(),
                action_targets: views.iter().map(|view| view.label.map_or([0.0; 2], |label| [label.width(), label.height()])).collect(),
            });
        }
    }

    let expected: Vec<Measurement> = [TabHeight::Default, TabHeight::Compact]
        .into_iter()
        .flat_map(|tab_height| {
            let compact = tab_height == TabHeight::Compact;
            RowLayout::ALL.into_iter().map(move |layout| {
                let single_row = matches!(layout, RowLayout::Single | RowLayout::Pinned);
                Measurement {
                    compact,
                    layout,
                    rows: match layout {
                        RowLayout::Single => 1,
                        RowLayout::Pinned => 2,
                        _ => 3,
                    },
                    tab_heights: (0..3)
                        .map(|index| if layout == RowLayout::Single || layout == RowLayout::Pinned && index > 0 { if compact { 27.0 } else { 31.0 } } else if compact { 24.0 } else { 28.0 })
                        .collect(),
                    fill_heights: (0..3)
                        .map(|index| {
                            let upper = if layout == RowLayout::Pinned { index == 0 } else { layout != RowLayout::Single && index < 2 };
                            let extension = if single_row { 9.0 } else { 3.0 };
                            (if compact { 20.0 } else { 24.0 }) + if !upper && index == 1 { extension } else { 0.0 }
                        })
                        .collect(),
                    content_heights: (0..3).map(|index| if single_row && index == 1 { if compact { 28.0 } else { 32.0 } } else if compact { 20.0 } else { 24.0 }).collect(),
                    backgrounds: ["rgb(30, 41, 59)", "rgb(22, 78, 99)", "rgb(30, 41, 59)"].map(str::to_string).to_vec(),
                    action_targets: vec![[20.0, 20.0]; 3],
                }
            })
        })
        .collect();
    assert_eq!(measurements, expected);
}

#[test]
fn wrapped_headers_contain_horizontal_tab_painting_while_preserving_vertical_shoulders() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `openNamedEditors([`a-${'long-wrapped-tab-name-'.repeat(12)}.txt`, 'short.txt'])`
    fx.open_named_editors(&[format!("a-{}.txt", "long-wrapped-tab-name-".repeat(12)), "short.txt".to_string()]);
    fx.update_options(|options| {
        options.tab_sizing = TabSizing::Fit;
        options.wrap_tabs = true;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });

    #[derive(Debug, PartialEq)]
    struct Row {
        width: f64,
        wrapped: bool,
        long_label: bool,
        paints_outside_group: bool,
        horizontal_overflow: OverflowMode,
        vertical_overflow: OverflowMode,
    }

    let colors = ConnectedTabColors::default();
    let shoulder = ModernSpacing::default().shoulder_radius();
    let mut results = Vec::new();
    for width in [500.0, 240.0, 350.0, 240.0] {
        fx.layout(width);
        let zoom = fx.layout.zoom.get();
        // `container.getBoundingClientRect().right`: o título ocupa a largura do grupo.
        let title_right = width * zoom;
        let views = fx.tab_views(&colors);
        // `range.selectNodeContents(name).getBoundingClientRect()` do primeiro `.label-name`: o texto inteiro, sem o corte da elipse.
        let first_tab = &fx.state.tabs[0];
        let label_left = fx.layout.label_span(fx.geometry().widths[0], fx.action_padding(0, false), first_tab.state.tab_actions_left).0;
        let name_right = views[0].hitbox.left + (label_left + name_chars(first_tab) * CHAR_WIDTH) * zoom;
        let overflow = connected_title_overflow(fx.layout.is_connected(), fx.state.container.connected_tabs_wrapping);
        // `elementFromPoint(titleBounds.right + 8, ...)`: o que a pintura nativa deixa passar da borda de fim do título são as abas
        // e o ombro da aba ativa fora da linha de cima, e só com o `overflow-x` visível.
        let reach = views.iter().map(|view| view.hitbox.right + if view.context.active && !view.context.upper_row { shoulder * zoom } else { 0.0 }).fold(0.0, f64::max);
        results.push(Row {
            width,
            wrapped: fx.state.container.connected_tabs_wrapping,
            long_label: name_right > title_right + 8.0,
            paints_outside_group: overflow.horizontal == OverflowMode::Visible && reach > title_right + 8.0,
            horizontal_overflow: overflow.horizontal,
            vertical_overflow: overflow.vertical,
        });
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row { wrapped: true, long_label: true, paints_outside_group: false, horizontal_overflow: OverflowMode::Clip, vertical_overflow: OverflowMode::Visible, ..*result })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn add_tab_leaves_separate_pinned_row_caps_and_action_hit_targets_unobstructed() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let first = fx.model.get_editor_by_index(0).expect("aba 0");
    fx.model.stick(&first);
    // `control = createControl(menuIds)` e `control.dispose()` não deixam estado: o que vale é o controle de duas barras com o
    // `tabsBarAddTab` nas duas barras.
    fx.set_options(|options| {
        options.pinned_tabs_on_separate_row = true;
        options.pinned_tab_sizing = PinnedTabSizing::Normal;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 120.0;
        options.tab_sizing_fixed_max_width = 120.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.create_multi_row(true);
    fx.row_open_editors();

    #[derive(Debug, PartialEq)]
    struct Row {
        theme: &'static str,
        wrap_tabs: bool,
        positive_inset: String,
        cap_before_button: bool,
        backplate_inside_control: String,
        clickable: bool,
    }

    let colors = ConnectedTabColors::default();
    let spacing = ModernSpacing::default();
    let mut results = Vec::new();
    // O tema só troca cor e traço (pintura): a geometria do botão e do contêiner vale para os três.
    for theme in ["vs-dark", "hc-black", "hc-light"] {
        for wrap_tabs in [false, true] {
            let (old_options, new_options) = fx.set_options(|options| options.wrap_tabs = wrap_tabs);
            fx.row_update_options(&old_options, &new_options);
            fx.row_layout(500.0);

            let bars = fx.live_bars();
            // `container.querySelector('.connected-tab-upper-bar')`
            let upper = bars.iter().find(|bar| bar.state.container.connected_tab_upper_bar).expect("a barra superior");
            let views = fx.tab_views_of(upper, &colors);
            let geometry = fx.layout.geometry(&upper.state.container, &upper.state.tabs);
            let insets = connected_add_tab_button_insets(upper.state.container.wrapping, upper.state.container.connected_tab_upper_bar, &spacing);
            // O botão de nova aba é o `.tabs-bar-add-tab` colado à borda de fim (`position: sticky`) e o `.action-label` recua a margem
            // de cada lado.
            let (add_left, add_right) = fx.layout.add_rect(&geometry);
            let zoom = fx.layout.zoom.get();
            let (button_left, button_right) = (add_left + insets.margin_start * zoom, add_right - spacing.size20 * zoom);
            let center = (button_left + button_right) / 2.0;
            // `elementFromPoint` no centro do botão: a área do controle de nova aba, antes do toolbar de ações que ocupa a borda de fim.
            let toolbar_left = fx.layout.width.get() * zoom - fx.layout.toolbar.get() * zoom;
            results.push(Row {
                theme,
                wrap_tabs,
                positive_inset: format!("{}px", insets.margin_start),
                cap_before_button: views[0].fill.right <= button_left,
                backplate_inside_control: format!("{}px", insets.backplate_inset_start),
                clickable: add_left <= center && center <= add_right && center <= toolbar_left,
            });
        }
    }

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row { positive_inset: "2px".to_string(), cap_before_button: true, backplate_inside_control: "2px".to_string(), clickable: true, ..*result })
        .collect();
    assert_eq!(results, expected);
}

#[test]
fn separate_pinned_bars_share_upper_pill_geometry_and_reserve_no_document_seam() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `root.style.setProperty('--vscode-tab-border', '#00ffff')` e `--vscode-contrastActiveBorder` só trocam o traço (pintura
    // nativa): a geometria e o contorno dos preenchimentos valem para todas as cores.
    let pinned_editors = fx.model.get_editors(EditorsOrder::Sequential, None);
    for editor in &pinned_editors {
        fx.model.stick(editor);
    }
    for index in 2..6 {
        fx.open_pinned_file(&format!("file{index}.ts"), false);
    }
    // `control.dispose(); reset(container)`: as duas barras são o controle do caso.
    fx.set_options(|options| {
        options.pinned_tabs_on_separate_row = true;
        options.pinned_tab_sizing = PinnedTabSizing::Normal;
        options.tab_action_unpin_visibility = true;
        options.tab_sizing = TabSizing::Fixed;
        options.tab_sizing_fixed_min_width = 120.0;
        options.tab_sizing_fixed_max_width = 120.0;
        options.editor_actions_location = EditorActionsLocation::Hidden;
    });
    fx.create_multi_row(false);
    fx.row_open_editors();

    #[derive(Debug, PartialEq)]
    struct Row {
        theme: &'static str,
        compact: bool,
        wrap_tabs: bool,
        width: f64,
        zoom: f64,
        active_index: usize,
        upper_bar: bool,
        all_upper_pills: bool,
        hitboxes: bool,
        painted_heights: bool,
        left_inset: bool,
        top_inset: bool,
        aligned_with_wrapped_pills: Option<bool>,
        shared_row_gap: Option<bool>,
        corners: Vec<String>,
        equal_left_stroke: bool,
        no_document_seam: bool,
        control_height_matches: bool,
        targets: bool,
    }

    let colors = ConnectedTabColors::default();
    let near = |value: f64, expected: f64| (value - expected).abs() < TOLERANCE;
    let mut results = Vec::new();
    for theme in ["vs", "vs-dark", "hc-black", "hc-light"] {
        for tab_height in [TabHeight::Default, TabHeight::Compact] {
            for wrap_tabs in [false, true] {
                let (old_options, new_options) = fx.set_options(|options| {
                    options.tab_height = tab_height;
                    options.wrap_tabs = wrap_tabs;
                });
                fx.row_update_options(&old_options, &new_options);
                for width in if wrap_tabs { vec![150.0, 260.0] } else { vec![600.0] } {
                    for zoom in if wrap_tabs { vec![1.0, 1.6] } else { vec![1.0] } {
                        // `root.style.zoom = String(zoom)`
                        fx.layout.zoom.set(zoom);
                        for active_index in [0usize, 2] {
                            let editor = fx.model.get_editor_by_index(active_index as i64).expect("aba");
                            open!(fx.model, editor, active: true);
                            fx.row_open_editors();
                            fx.row_layout(width);

                            let bars = fx.live_bars();
                            let pinned = fx.tab_views_of(&bars[0], &colors);
                            let lower_fill = fx.tab_views_of(&bars[1], &colors)[0].fill;
                            let row_height = if tab_height == TabHeight::Compact { 24.0 } else { 28.0 };
                            let bars_height: f64 = bars.iter().map(|bar| fx.layout.measure_wrapping(&bar.state.container, &bar.state.tabs).tabs_and_actions_height).sum();
                            results.push(Row {
                                theme,
                                compact: tab_height == TabHeight::Compact,
                                wrap_tabs,
                                width,
                                zoom,
                                active_index,
                                upper_bar: bars[0].state.container.connected_tab_upper_bar,
                                all_upper_pills: pinned.iter().all(|view| view.context.upper_row),
                                hitboxes: pinned.iter().all(|view| near((view.hitbox.bottom - view.hitbox.top) / zoom, row_height)),
                                painted_heights: pinned.iter().all(|view| near(view.fill.height() / zoom, row_height - 4.0)),
                                // `group.getBoundingClientRect()` é a origem das coordenadas.
                                left_inset: near(pinned[0].fill.left / zoom, 4.0),
                                top_inset: near(pinned[0].fill.top / zoom, 4.0),
                                aligned_with_wrapped_pills: wrap_tabs.then(|| near(pinned[0].fill.left - lower_fill.left, 0.0)),
                                shared_row_gap: wrap_tabs.then(|| near((lower_fill.top - pinned[1].fill.bottom) / zoom, 4.0)),
                                corners: pinned.iter().map(|view| css_border_radius(view.shape.border_radius)).collect(),
                                equal_left_stroke: pinned[0].shape.border_left_width == pinned[1].shape.border_left_width,
                                no_document_seam: fx.layout.tabs_padding_bottom(&bars[0].state.container) == 0.0,
                                control_height_matches: fx.row().get_height(&*fx.layout) == bars_height,
                                targets: pinned.iter().all(|view| view.label.is_some_and(|label| near(label.width() / zoom, 20.0) && near(label.height() / zoom, 20.0))),
                            });
                        }
                    }
                }
            }
        }
    }

    fx.row_layout(260.0);
    let height_with_pinned_bar = fx.row().get_height(&*fx.layout);
    for editor in &pinned_editors {
        fx.model.unstick(editor);
        fx.row_mut().unstick_editor(editor);
    }
    fx.row_layout(260.0);
    // `Number(root.style.zoom)`: o último zoom escrito pelo laço.
    let zoom = fx.layout.zoom.get();
    // `.tabs-and-actions-container:not(.empty) .tab-fill`: o primeiro preenchimento da primeira barra que sobrou.
    let remaining_fill = {
        let bars = fx.live_bars();
        fx.tab_views_of(&bars[0], &colors)[0].fill
    };
    let same_outer_top = near(remaining_fill.top / zoom, 4.0);
    let height_stable = fx.row().get_height(&*fx.layout) == height_with_pinned_bar;
    // `root.classList.remove('modern-ui-connected-editor-tabs')`
    fx.set_connected(false);
    fx.row_layout(600.0);
    let visible_marker_cleared_for_pill = !fx.live_bars().iter().any(|bar| bar.state.container.connected_tab_upper_bar);

    let expected: Vec<Row> = results
        .iter()
        .map(|result| Row {
            upper_bar: true,
            all_upper_pills: true,
            hitboxes: true,
            painted_heights: true,
            left_inset: true,
            top_inset: true,
            aligned_with_wrapped_pills: result.wrap_tabs.then_some(true),
            shared_row_gap: result.wrap_tabs.then_some(true),
            corners: vec!["4px".to_string(), "4px".to_string()],
            equal_left_stroke: true,
            no_document_seam: true,
            control_height_matches: true,
            targets: true,
            ..*result
        })
        .collect();
    assert_eq!((results, same_outer_top, height_stable, visible_marker_cleared_for_pill), (expected, true, true, true));
}

/// A leitura de uma aba do caso `wrapped Connected pills share Pill padding, gutters, and close targets`.
#[derive(Debug, PartialEq)]
struct PillRead {
    /// `[paddingLeft, paddingRight]` da `.tab`.
    padding: [f64; 2],
    /// `[surface.left - hitbox.left, hitbox.right - surface.right, surface.top - hitbox.top, hitbox.bottom - surface.bottom]`.
    insets: [f64; 4],
    /// `[surface.left - group.left, surface.top - group.top]`.
    outer_offset: [f64; 2],
    height: f64,
    radius: String,
    target: [f64; 2],
    action_offset: f64,
}

/// `read()` do caso: as duas primeiras abas da barra.
fn read_pills(fx: &Fixture, colors: &ConnectedTabColors, location: TabActionLocation) -> Vec<PillRead> {
    let bars = fx.live_bars();
    let bar = &bars[0];
    let geometry = fx.layout.geometry(&bar.state.container, &bar.state.tabs);

    fx.tab_views_of(bar, colors)
        .iter()
        .take(2)
        .enumerate()
        .map(|(index, view)| {
            let label = view.label.expect("a aba tem a ação");
            let style = &geometry.styles[index];

            PillRead {
                padding: [style.padding_left, style.padding_right],
                insets: [view.fill.left - view.hitbox.left, view.hitbox.right - view.fill.right, view.fill.top - view.hitbox.top, view.hitbox.bottom - view.fill.bottom],
                outer_offset: [view.fill.left, view.fill.top],
                height: view.fill.height(),
                radius: css_border_radius(view.shape.border_radius),
                target: [label.width(), label.height()],
                action_offset: if location == TabActionLocation::Left { label.left - view.hitbox.left } else { view.hitbox.right - label.right },
            }
        })
        .collect()
}

#[test]
fn wrapped_connected_pills_share_pill_padding_gutters_and_close_targets() {
    let mut fx = Fixture::new();
    fx.connected_group();
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/third.ts"), "testEditorInput"));
    let third: EditorInputRef = concrete;
    // `model.openEditor(third, { index: model.count, pinned: true, active: true })`: no fim, e ativa.
    open!(fx.model, third, index: fx.model.count() as i64, pinned: true, active: true);
    fx.open_editors();

    #[derive(Debug, PartialEq)]
    struct Row {
        theme: &'static str,
        compact: bool,
        tab_action_location: TabActionLocation,
        same: bool,
    }

    let colors = ConnectedTabColors::default();
    let mut results = Vec::new();
    // O tema só troca cor e traço (pintura): a geometria das abas Pill e conectada vale para os quatro.
    for theme in ["vs", "vs-dark", "hc-black", "hc-light"] {
        for tab_height in [TabHeight::Default, TabHeight::Compact] {
            for tab_action_location in [TabActionLocation::Left, TabActionLocation::Right] {
                fx.update_options(|options| {
                    options.wrap_tabs = true;
                    options.tab_height = tab_height;
                    options.tab_action_location = tab_action_location;
                    options.tab_sizing = TabSizing::Fixed;
                    options.tab_sizing_fixed_min_width = 120.0;
                    options.tab_sizing_fixed_max_width = 120.0;
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                });
                // `root.classList.remove('modern-ui-connected-editor-tabs')`: o estilo Pill.
                fx.set_connected(false);
                fx.layout(150.0);
                let pill = read_pills(&fx, &colors, tab_action_location);
                fx.set_connected(true);
                fx.layout(150.0);
                let connected = read_pills(&fx, &colors, tab_action_location);
                results.push(Row { theme, compact: tab_height == TabHeight::Compact, tab_action_location, same: pill == connected });
            }
        }
    }

    let expected: Vec<Row> = results.iter().map(|result| Row { same: true, ..*result }).collect();
    assert_eq!(results, expected);
}

impl Fixture {
    /// `container.querySelector('.tab.active')` para escrever nele (`tab.classList.add(...)`).
    fn active_tab_mut(&mut self) -> &mut Tab {
        let bars: Vec<&mut MultiEditorTabsControlState> = match &mut self.row {
            Some(row) => vec![&mut row.sticky, &mut row.unsticky],
            None => vec![&mut self.state],
        };

        bars.into_iter().flat_map(|bar| bar.tabs.iter_mut()).find(|tab| tab.state.active).expect("aba ativa")
    }
}

#[test]
fn wrapped_freestanding_fills_own_equal_gutters_and_clipped_accents_across_themes_and_densities() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `root.style.setProperty('--vscode-tab-border', '#00ffff')` só troca o traço (pintura nativa).
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/modified.ts"), "testEditorInput"));
    concrete.set_dirty();
    let modified: EditorInputRef = concrete;
    open!(fx.model, modified, index: fx.model.count() as i64, pinned: true, active: false);
    let concrete = fx.suite.disposables.add_rc(TestFileEditorInput::new(uri_file("/path/fourth.ts"), "testEditorInput"));
    let fourth: EditorInputRef = concrete;
    open!(fx.model, fourth, index: fx.model.count() as i64, pinned: true, active: false);
    fx.open_editors();
    // `themeService.setTheme(new TestColorTheme({ 'tab.activeModifiedBorder': '#ffffff', 'tab.inactiveModifiedBorder': '#ffffff' }))`
    fx.state.set_colors(TabColors {
        tab_active_modified_border: Some("#ffffff".to_string()),
        tab_inactive_modified_border: Some("#ffffff".to_string()),
        ..TabColors::default()
    });

    #[derive(Debug, PartialEq)]
    struct Measurement {
        theme: &'static str,
        compact_layout: bool,
        compact: bool,
        zoom: f64,
        /// `[|tab.height / zoom - nominal| < 1/32, |fill.height / zoom - painted| < 1/32]` de cada aba.
        heights: Vec<[bool; 2]>,
        equal_gutters: bool,
        inside_hitboxes: bool,
        corners: Vec<String>,
        first_stroke: f64,
        /// `[insideFill, color, height ≈ 2, overflow do preenchimento]` do realce de topo da terceira aba.
        accent: (bool, String, bool, OverflowMode),
        targets: Vec<bool>,
    }

    let spacing = ModernSpacing::default();
    let colors = ConnectedTabColors::default();
    let near = |value: f64, expected: f64| (value - expected).abs() < TOLERANCE;
    let mut measurements = Vec::new();
    let mut expected = Vec::new();
    for theme in ["vs", "vs-dark", "hc-black", "hc-light"] {
        // `root.classList.toggle('modern-ui-compact', compactLayout)`: só a densidade do tema, a geometria da aba não muda.
        for compact_layout in [false, true] {
            for tab_height in [TabHeight::Default, TabHeight::Compact] {
                fx.update_options(|options| {
                    options.tab_height = tab_height;
                    options.wrap_tabs = true;
                    options.highlight_modified_tabs = true;
                    options.tab_sizing = TabSizing::Fixed;
                    options.tab_sizing_fixed_min_width = 120.0;
                    options.tab_sizing_fixed_max_width = 120.0;
                    options.editor_actions_location = EditorActionsLocation::Hidden;
                });
                for zoom in [1.0, 1.25, 1.6] {
                    fx.layout.zoom.set(zoom);
                    fx.layout(260.0);
                    let views = fx.tab_views(&colors);
                    let nominal_height = if tab_height == TabHeight::Compact { 24.0 } else { 28.0 };
                    let painted_height = nominal_height - 4.0;
                    let gap = 4.0;
                    let modified_tab = &fx.state.tabs[2].state;
                    let accent = dirty_top_accent(fx.layout.is_connected(), modified_tab.dirty_border_top_color.as_deref(), &spacing);
                    let accent_color = match accent.color {
                        TabStrokeColor::Color(color) => css_rgb(&Color::from_hex(color)),
                        other => describe_stroke_color(other),
                    };
                    measurements.push(Measurement {
                        theme,
                        compact_layout,
                        compact: tab_height == TabHeight::Compact,
                        zoom,
                        heights: views
                            .iter()
                            .map(|view| [near((view.hitbox.bottom - view.hitbox.top) / zoom, nominal_height), near(view.fill.height() / zoom, painted_height)])
                            .collect(),
                        equal_gutters: near((views[1].fill.left - views[0].fill.right) / zoom, gap) && near((views[2].fill.top - views[0].fill.bottom) / zoom, gap),
                        inside_hitboxes: views.iter().all(|view| view.fill.top >= view.hitbox.top && view.fill.bottom <= view.hitbox.bottom),
                        corners: views.iter().map(|view| css_border_radius(view.shape.border_radius)).collect(),
                        first_stroke: views[0].shape.border_left_width,
                        accent: (
                            fx.layout.is_connected(),
                            accent_color,
                            (accent.height - 2.0).abs() < 1.0 / 64.0,
                            if views[2].shape.overflow_hidden { OverflowMode::Hidden } else { OverflowMode::Visible },
                        ),
                        targets: views.iter().map(|view| view.label.is_some_and(|label| near(label.width() / zoom, 20.0) && near(label.height() / zoom, 20.0))).collect(),
                    });
                    expected.push(Measurement {
                        theme,
                        compact_layout,
                        compact: tab_height == TabHeight::Compact,
                        zoom,
                        heights: vec![[true, true]; 4],
                        equal_gutters: true,
                        inside_hitboxes: true,
                        corners: vec!["4px".to_string(); 4],
                        first_stroke: views[1].shape.border_left_width,
                        accent: (true, "rgb(255, 255, 255)".to_string(), true, OverflowMode::Hidden),
                        targets: vec![true; 4],
                    });
                }
            }
        }
    }

    assert_eq!(measurements, expected);
}

#[test]
fn modified_top_indicators_follow_the_fill_without_moving_connected_close_actions() {
    let mut fx = Fixture::new();
    fx.connected_group();
    // `control.dispose()`: cada volta cria o controle dela. `model.openEditor(third, { pinned: true, active: false })`: depois da ativa.
    fx.open_pinned_file("third.ts", false);
    let first = fx.model.get_editor_by_index(0).expect("aba 0");
    let spacing = ModernSpacing::default();
    let colors = ConnectedTabColors::default();

    #[derive(Debug, PartialEq)]
    struct Measurement {
        connected: bool,
        compact: bool,
        layout: RowLayout,
        tab_action_location: TabActionLocation,
        top_offset: f64,
        thickness: f64,
        fill_stable: bool,
        action_shift: f64,
        target: [f64; 2],
    }

    let mut measurements = Vec::new();
    let mut expected = Vec::new();
    for connected in [false, true] {
        // `root.classList.toggle('modern-ui-connected-editor-tabs', connected)`
        fx.set_connected(connected);
        for tab_height in [TabHeight::Default, TabHeight::Compact] {
            for layout in [RowLayout::Single, RowLayout::Wrapped, RowLayout::PinnedWrapped] {
                for tab_action_location in [TabActionLocation::Left, TabActionLocation::Right] {
                    let wrapped = layout.wrapped();
                    let pinned = layout.separate_pinned_row();
                    fx.set_options(|options| {
                        options.tab_height = tab_height;
                        options.tab_action_location = tab_action_location;
                        options.wrap_tabs = wrapped;
                        options.pinned_tabs_on_separate_row = pinned;
                        options.pinned_tab_sizing = PinnedTabSizing::Normal;
                        options.tab_sizing = TabSizing::Fixed;
                        options.tab_sizing_fixed_min_width = 120.0;
                        options.tab_sizing_fixed_max_width = 120.0;
                        options.editor_actions_location = EditorActionsLocation::Hidden;
                    });
                    if pinned {
                        fx.model.stick(&first);
                    } else {
                        fx.model.unstick(&first);
                    }
                    let second = fx.model.get_editor_by_index(1).expect("aba 1");
                    open!(fx.model, second, active: true);
                    let width = if wrapped { 150.0 } else { 400.0 };
                    if pinned {
                        fx.create_multi_row(false);
                        fx.row_open_editors();
                        fx.row_layout(width);
                    } else {
                        fx.create_control(false);
                        fx.layout(width);
                    }

                    let before = fx.tab_views(&colors);
                    let active_index = before.iter().position(|view| view.context.active).expect("aba ativa");
                    // `tab.classList.add('dirty', 'dirty-border-top')` e `--tab-dirty-border-top-color`
                    let tab = fx.active_tab_mut();
                    tab.state.dirty = true;
                    tab.state.dirty_border_top = true;
                    tab.state.dirty_border_top_color = Some("#22d3ee".to_string());
                    let after = fx.tab_views(&colors);
                    let accent = dirty_top_accent(connected, Some("#22d3ee"), &spacing);
                    let (before_action, after_action) = (before[active_index].label.expect("a ação"), after[active_index].label.expect("a ação"));
                    let compact = tab_height == TabHeight::Compact;
                    measurements.push(Measurement {
                        connected,
                        compact,
                        layout,
                        tab_action_location,
                        top_offset: accent.top_offset,
                        thickness: accent.height,
                        fill_stable: before[active_index].fill == after[active_index].fill,
                        action_shift: after_action.top - before_action.top,
                        target: [after_action.width(), after_action.height()],
                    });
                    expected.push(Measurement {
                        connected,
                        compact,
                        layout,
                        tab_action_location,
                        top_offset: 0.0,
                        thickness: 2.0,
                        fill_stable: true,
                        action_shift: if connected { 0.0 } else { 1.0 },
                        target: [20.0, 20.0],
                    });
                    // `tabsControl.dispose()`
                    fx.dispose_row();
                }
            }
        }
    }

    assert_eq!(measurements, expected);
}

//#endregion
