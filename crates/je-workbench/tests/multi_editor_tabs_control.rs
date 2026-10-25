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
//! Casos que não têm estado do controle para conferir (só pintura ou geometria CSS do botão) e portanto ficam de fora, na
//! posição do original: `Add Tab spacing matches adjacent tab fill insets ...` (folgas, margens e raio do `::before`),
//! `modern editor headers retain an opaque backing ...` e `editor group owns the header backing ...` (cores de fundo
//! computadas). No caso `scrolling tab content is clipped before the rounded sticky Add control` a conferência de
//! `rtlCapAtBoundary` e `oppositeShoulderPresentWhenVisible` (sobreposição e `::before`) também é pintura e não entra.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[macro_use]
#[path = "editor_group_model_suite/mod.rs"]
mod suite;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_editor::core_2d::dimension::Dimension;
use je_platform::resources::uri_file;
use je_workbench::browser::parts::editor::editor::default_editor_part_options;
use je_workbench::browser::parts::editor::editor_tabs_control::{
    EditorTabsControlBase, EditorTitleControlDimensions, TabsControlHost, TabsParentTraits, compute_tab_height,
};
use je_workbench::browser::parts::editor::multi_editor_tabs_control::{
    AddTabAlignmentMeasure, AddTabPresence, AlignTabMeasure, ConnectedActiveTabMeasure, ConnectedClippingMeasure, EditorInputLabel, FitTabMetrics,
    MultiEditorTabsControlState, Tab, TabAction, TabClipPath, TabState, TabsContainerView, TabsMeasurer, TabsMetrics, WrapTabMeasure, WrappingMeasure,
};
use je_workbench::common::editor::editor_group_model::{EditorGroupModel, EditorOrIndex, IReadonlyEditorGroupModel};
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::{EditorInputOrUntyped, EditorInputRef, EditorsOrder, GroupIdentifier, Verbosity};
use je_workbench::services::editor::editor_groups_service::{EditorActionsLocation, EditorPartOptions, TabHeight, TabSizing};
use suite::Suite;

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
/// `margin-inline-end` da última aba conectada: o ombro do terminal.
const SHOULDER_MARGIN: f64 = 8.0;
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

    fn tab_width(&self, tab: &Tab) -> f64 {
        let state = &tab.state;
        if state.sticky_compact {
            return if self.parent.get().in_modern_ui_tabs { 28.0 } else { 38.0 };
        }
        if state.sticky_shrink {
            return 80.0;
        }

        let natural = self.natural_width(tab, state.tab_action_sizing_rest);
        if state.sizing_fixed {
            let options = self.options.borrow();
            return state.current_width.unwrap_or_else(|| natural.max(options.tab_sizing_fixed_min_width).min(options.tab_sizing_fixed_max_width));
        }
        if state.sizing_shrink {
            return natural.max(80.0).min(120.0);
        }

        state.fit_width.unwrap_or(natural)
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

        let margins: Vec<f64> = (0..tabs.len())
            .map(|index| {
                if index + 1 != tabs.len() {
                    return 0.0;
                }
                let shoulder = if connected { SHOULDER_MARGIN } else { 0.0 };
                shoulder + if container.wrapping && container.add_tab != AddTabPresence::Visible { container.last_tab_margin_right } else { 0.0 }
            })
            .collect();
        let widths: Vec<f64> = tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let width = self.tab_width(tab);
                if container.wrapping && tab.state.sizing_fit && parent.in_modern_ui_tabs { width.min(80f64.max(visible_width - margins[index])) } else { width }
            })
            .collect();

        let mut offsets = Vec::new();
        let mut x = 0.0;
        for index in 0..tabs.len() {
            offsets.push(x);
            x += widths[index] + margins[index];
        }
        let content_width = x + if connected { add_width } else { 0.0 };

        let tab_height = self.tab_height();
        let mut tops = Vec::new();
        let mut rows = 1usize;
        let mut add_top = 0.0;
        if container.wrapping {
            let mut row = 0usize;
            let mut cursor = 0.0;
            for index in 0..tabs.len() {
                let size = widths[index] + margins[index];
                if cursor > 0.0 && cursor + size > visible_width {
                    row += 1;
                    cursor = 0.0;
                }
                tops.push(row as f64 * tab_height);
                cursor += size;
            }
            if add_width > 0.0 && cursor > 0.0 && cursor + add_width > visible_width {
                row += 1;
            }
            add_top = row as f64 * tab_height;
            rows = row + 1;
        } else {
            tops.resize(tabs.len(), 0.0);
        }

        Geometry { widths, margins, offsets, tops, rows, add_width, add_top, visible_width, content_width }
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
            add_tab_total_width: geometry.add_width,
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
            shoulder_extent: 6.0,
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

// `Add Tab spacing matches adjacent tab fill insets without losing the terminal shoulder`: sem estado do controle, ver o
// cabeçalho do arquivo.

/// A leitura de `readClipping(direction, zoom)` que cabe ao controle.
#[derive(Debug, PartialEq)]
struct ClippingRead {
    has_clipped_tab: bool,
    has_clipped_active: bool,
    matching_clip: bool,
}

/// `readClipping(direction, zoom)`, sem o que é pintura (sobreposição e ombros).
fn read_clipping(fx: &Fixture, rtl: bool, zoom: f64) -> ClippingRead {
    let geometry = fx.geometry();
    let scroll = fx.state.container.scroll_left;
    let (add_left, add_right) = fx.layout.add_rect(&geometry);
    let boundary = if rtl { add_right + 8.0 * zoom } else { add_left - 8.0 * zoom };
    let rects: Vec<(f64, f64)> = (0..fx.state.tabs.len()).map(|index| fx.layout.tab_rect(&geometry, index, scroll)).collect();
    let clipped = |(left, right): (f64, f64)| if rtl { left < boundary } else { right > boundary };

    ClippingRead {
        has_clipped_tab: rects.iter().any(|rect| clipped(*rect)),
        has_clipped_active: fx.state.tabs.iter().zip(&rects).any(|(tab, rect)| tab.state.active && clipped(*rect)),
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

    let expected_read = || ClippingRead { has_clipped_tab: true, has_clipped_active: true, matching_clip: true };
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

// O ponto de parada, na ordem do original, é o caso `wrapped fit check includes the terminal gutter and the independent
// controls row` (linha 458): os casos seguintes dependem do modelo de linhas e margens do `Layout` com o limiar de
// quebra calculado do estilo computado da aba terminal.

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
