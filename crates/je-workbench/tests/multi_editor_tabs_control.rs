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
use je_ui::Ui;
use je_workbench::browser::parts::editor::editor::default_editor_part_options;
use je_workbench::browser::parts::editor::editor_group_view::EditorGroupView;
use je_workbench::browser::parts::editor::editor_tabs_control::{
    EditorTabsControlBase, EditorTitleControlDimensions, TabsControlHost, TabsParentTraits, compute_tab_height,
};
use je_workbench::browser::parts::editor::multi_editor_tabs_control::{
    AddTabAlignmentMeasure, AddTabPresence, AlignTabMeasure, ConnectedActiveTabMeasure, ConnectedClippingMeasure, CssLength, EditorInputLabel, FitTabMetrics,
    MultiEditorTabsControlState, Tab, TabAction, TabActionLabelContext, TabBoxStyle, TabClipPath, TabCssClasses, TabFlexItem, TabSizingParams, TabState,
    TabsContainerView, TabsMeasurer, TabsMetrics, WrapTabMeasure, WrappingMeasure, layout_tabs_row, tab_action_label_opacity, tab_box_style,
};
use je_workbench::common::editor::editor_group_model::{EditorGroupModel, EditorOrIndex, IReadonlyEditorGroupModel};
use je_workbench::common::editor::editor_factory_registry::editor_factory_registry;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::{EditorInputOrUntyped, EditorInputRef, EditorsOrder, GroupIdentifier, Verbosity};
use je_workbench::contrib::modern_ui::tabs_header::{ModernHeaderBacking, modern_header_backing};
use je_workbench::services::editor::editor_groups_service::{
    EditorActionsLocation, EditorPartOptions, IEditorGroup, IEditorPart, PinnedTabSizing, TabHeight, TabSizing,
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
}

impl NamedInput {
    fn new(name: &str, path: &str, descriptions: Option<(&str, &str)>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            base: EditorInputBase::new(this.clone()),
            resource: uri_file(path),
            name: name.to_string(),
            descriptions: descriptions.map(|(short, long)| (short.to_string(), long.to_string())),
        })
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

/// As posições de cada aba, na ordem lógica do texto.
struct Geometry {
    widths: Vec<f64>,
    margins: Vec<f64>,
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
        })
    }

    /// A largura de uma aba sem a restrição da linha (`max-width` solto).
    fn natural_width(&self, tab: &Tab, rest: bool) -> f64 {
        let persistent = tab.state.dirty || tab.state.sticky;
        let reserved = self.options.borrow().tab_action_reserve_space || persistent;
        TAB_PADDING + name_chars(tab) * CHAR_WIDTH + if reserved && !rest { ACTION_COLUMN } else { 0.0 }
    }

    /// `--modern-ui-tab-terminal-gutter`: o vão depois da última aba quebrada (só no Modern UI).
    fn terminal_gutter(&self, container: &TabsContainerView) -> f64 {
        let parent = self.parent.get();
        match (parent.in_modern_ui_tabs, parent.in_connected_editor_tabs && !container.connected_tab_upper_bar) {
            (false, _) => 0.0,
            (true, true) => SHOULDER_RADIUS,
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
                if tab.state.last_tab {
                    style.margin_end += extra_margin;
                }

                TabFlexItem { style, content_width: self.natural_width(tab, tab.state.tab_action_sizing_rest) }
            })
            .collect();
        let margins: Vec<f64> = items.iter().map(|item| item.style.margin_end).collect();

        // `.tabs-bar-add-tab { flex: 0 0 auto }` é um filho do `tabs-container` na quebra, com a mesma margem final da última aba.
        let add_margin = if container.wrapping && add_width > 0.0 { container.last_tab_margin_right } else { 0.0 };
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

        let tab_height = self.tab_height();
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

        let tops: Vec<f64> = (0..tabs.len()).map(|index| row.lines[index] as f64 * tab_height).collect();
        let last_line = row.lines.last().copied().unwrap_or(0);
        let (rows, add_top) = if container.wrapping { (last_line + 1, last_line as f64 * tab_height) } else { (1usize, 0.0) };

        Geometry { widths, margins, offsets, tops, rows, add_width, add_margin, add_top, visible_width, content_width }
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
    fn measure_fit_tabs(&self, tabs: &[Tab], indexes: &[usize]) -> Vec<FitTabMetrics> {
        indexes
            .iter()
            .map(|&index| {
                let tab = &tabs[index];
                let name = name_chars(tab) * CHAR_WIDTH;
                FitTabMetrics {
                    width: self.natural_width(tab, tab.state.tab_action_sizing_rest),
                    font_size: 13.0,
                    padding_left: TAB_PADDING / 2.0,
                    padding_right: TAB_PADDING / 2.0,
                    border_left_width: 0.0,
                    border_right_width: 0.0,
                    action_fit_padding: Some(ACTION_COLUMN),
                    name_width: Some(name),
                    label_widths: Some((name, name)),
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
        let tab_height = self.tab_height();
        let tabs_container_height = if container.wrapping { geometry.rows as f64 * tab_height } else { tab_height };

        WrappingMeasure {
            visible_tabs_width: geometry.visible_width,
            all_tabs_width: if container.wrapping { geometry.visible_width } else { geometry.content_width.max(geometry.visible_width) },
            tabs_container_height,
            tabs_and_actions_height: tabs_container_height + if self.parent.get().in_modern_ui_tabs { ROW_PADDING } else { 0.0 },
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

fn build_state(
    model: &Rc<EditorGroupModel>,
    options: &Rc<RefCell<EditorPartOptions>>,
    parent: TabsParentTraits,
    add_tab: AddTabPresence,
) -> MultiEditorTabsControlState {
    let host: Rc<dyn TabsControlHost> = Rc::new(Host { model: model.clone(), options: options.clone() });
    let tabs_model: Rc<dyn IReadonlyEditorGroupModel> = model.clone();
    let base = EditorTabsControlBase::new(host, tabs_model, parent, false, false);
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
    parent: TabsParentTraits,
    add_visible: bool,
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
        let mut fixture = Self { suite, model, options, layout, state, parent, add_visible: true };
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
        self.layout.parent.set(parent);
    }

    /// `control.dispose(); reset(container); control = createControl(menuIds)`.
    fn create_control(&mut self, has_add_tab: bool) {
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
        let model = self.model.clone();
        let mut create_tab = |index: usize| {
            let editor = model.get_editor_by_index(index as i64).expect("tab index out of range");
            Tab {
                label: EditorInputLabel { editor, name: None, description: None, force_description: false, title: None, aria_label: None },
                state: TabState::default(),
            }
        };
        self.state.handle_opened_editors(&mut create_tab);
    }

    /// `model.closeEditor(editor); control.closeEditor(editor)`.
    fn close_editor(&mut self, editor: &EditorInputRef) {
        self.model.close_editor(editor, None, None);
        self.state.handle_closed_editors();
    }

    /// `openNamedEditors(names)`.
    fn open_named_editors(&mut self, names: &[String]) -> Vec<EditorInputRef> {
        for editor in self.model.get_editors(EditorsOrder::Sequential, None) {
            self.close_editor(&editor);
        }
        let mut editors = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let concrete = self
                .suite
                .disposables
                .add_rc(NamedInput::new(name, &format!("/workspace/project/src/{name}"), Some(("src", "/workspace/project/src"))));
            let editor: EditorInputRef = concrete;
            open!(self.model, editor, index: index as i64, pinned: true, active: index == names.len() - 1);
            editors.push(editor);
        }
        self.open_editors();

        editors
    }

    /// `partOptions = { ...partOptions, ... }; control.updateOptions(oldOptions, partOptions)`.
    fn update_options(&mut self, change: impl FnOnce(&mut EditorPartOptions)) {
        let old = self.options.borrow().clone();
        let mut new = old.clone();
        change(&mut new);
        *self.options.borrow_mut() = new.clone();
        let _ = self.state.update_options(&old, &new);
    }

    /// O quadro de animação: executa o layout agendado com as dimensões guardadas.
    fn frame(&mut self) {
        if let Some(options) = self.state.layout_scheduler.take() {
            let dimensions = EditorTitleControlDimensions { container: self.state.dimensions.container, available: self.state.dimensions.available };
            self.state.do_layout(dimensions, Some(options), &*self.layout);
        }
    }

    /// `computeHeight()` com o que o navegador mede.
    fn computed_height(&self) -> f64 {
        let wrapping_or_connected = (self.options.borrow().wrap_tabs && self.state.container.wrapping) || self.parent.in_connected_editor_tabs;
        let measured = wrapping_or_connected.then(|| self.layout.measure_wrapping(&self.state.container, &self.state.tabs).tabs_and_actions_height);

        self.state.compute_height(measured)
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
        self.layout.width.set(width);
        let dimensions = EditorTitleControlDimensions {
            container: Some(Dimension { width, height: 33.0 }),
            available: Some(Dimension { width, height: 300.0 }),
        };
        let computed_height = self.computed_height();
        self.state.layout(dimensions, None, computed_height);
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
                        separate_row: (!same_row).then(|| geometry.add_top > geometry.tops[last] && (!connected || terminal.state.connected_tab_top_row)),
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
                .filter(|(index, tab)| fx.layout.natural_width(tab, tab.state.tab_action_sizing_rest) > geometry.widths[*index])
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

// `HC modified pills retain hover and selection side strokes independently of their top bar` (linha 823) é o próximo caso:
// confere as bordas computadas do `.tab-fill` (estilo, cor) e o acento superior da aba, pintura do Modern UI que o controle
// não decide, e depende de uma cascata de bordas (`tabs.css`, `connectedEditorTabs.css`, regras de `hc-black` e `hc-light`)
// que ainda não tem função nativa.

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

//#endregion
