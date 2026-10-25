//! Porte da primeira metade de `src/vs/workbench/browser/parts/editor/multiEditorTabsControl.ts`: o modelo de rótulos das
//! abas (`computeTabLabels`, `shortenTabLabels`, `getLabelConfigFlags`, `equalsEditorInputLabel`), o estado desenhado de cada
//! aba (`redrawTab`, `redrawTabAction`, `redrawTabLabel`, `redrawTabSelectedActiveAndDirty`, `redrawTabBorders`), a escolha da
//! ação da aba com o Alt e a aba sob o mouse, os dimensionamentos `fit`, `shrink` e `fixed`, a largura das abas fixadas, o
//! agendamento do layout, a rolagem para revelar a aba ativa (`doLayoutTabsNonWrapping`), a rolagem do mouse para trocar de
//! aba e o que `handleOpenedEditors`, `handleClosedEditors`, `moveEditor`, `pinEditor` e as trocas de estado decidem.
//!
//! Divergências:
//! - **Sem DOM.** A árvore de abas é o [`TabsView`]: uma linha por aba com o que o original escreve no `HTMLElement` (classes,
//!   `--tab-sizing-fit-width`, `left`, bordas, `aria-*`). O desenho nativo lê essas linhas; as medidas que o original pede ao
//!   navegador (`offsetWidth`, `scrollWidth`, `offsetLeft`) chegam como [`TabsMetrics`].
//! - **Cores do tema** (`getColor`) chegam como [`TabColors`], já resolvidas pelo chamador (o `Themable`).
//! - **Serviços** (`IPathService`, `IEditorService`, `IKeybindingService`, `IContextMenuService`) chegam como o separador de
//!   caminho ([`PathSeparator`]) e as decisões que eles tomariam; a abertura de editor, o menu e a montagem dos widgets
//!   (`ActionBar`, `ScrollableElement`, `ResourceLabels.create`) ficam com a renderização nativa.
//! - **Agendamento.** `scheduleAtNextAnimationFrame` é o [`LayoutScheduler`]: guarda as opções do layout pedido e quem
//!   executa o quadro chama [`LayoutScheduler::take`]. O `RunOnceScheduler` dos rótulos é [`LabelUpdateScheduler`].
//! - **Aba conectada** (`connectedTabBounds`, `clearConnectedTabClipping`, `updateConnectedTabClipping`) mede retângulos do DOM
//!   e fica na fatia 2.
//!
//! Plano das fatias seguintes (o que NÃO está aqui):
//! - Fatia 2, o resto da produção: `registerTabListeners` (clique, duplo clique, toque, teclado, seleção com Ctrl e Shift:
//!   `selectEditor`, `selectEditorsBetween`, `unselectEditor`, `unselectAllEditors`), o arrastar e soltar de uma aba
//!   (`isSupportedDropTransfer`, `updateDropFeedback`, `computeDropTarget`, `getTabDragOverLocation`, `onDrop`), o menu de
//!   contexto da barra, `doLayoutTabsWrapping` e `updateWrappedAddTabAlignment`, `getFitTabWidth` (medição), o recorte das
//!   abas conectadas, `originatesFromTabActionBar` e `dispose`.
//! - Fatia 3: o porte do teste `multiEditorTabsControl.test.ts` (86 casos) sobre este controle.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::labels::shorten;
use je_base::lifecycle::id_of;
use je_base::scrollable::ScrollbarVisibility;
use je_editor::core_2d::dimension::Dimension;

use crate::browser::parts::editor::editor_tabs_control::{EditorTabsControlBase, EditorTitleControlDimensions, TabsControlHost};
use crate::common::editor::editor_group_model::{EditorOrIndex, IReadonlyEditorGroupModel};
use crate::common::editor::{EditorInputCapabilities, EditorInputRef, EditorsOrder, SideBySideEditor, Verbosity};
use crate::common::editor::EditorResourceAccessor;
use crate::common::editor::EditorResourceAccessorOptions;
use crate::services::editor::editor_groups_service::{
    EditorLabelFormat, EditorPartOptions, PinnedTabSizing, TabActionLocation, TabSizing, TitleScrollbarSizing, TitleScrollbarVisibility,
};

/// `MultiEditorTabsControl.SCROLLBAR_SIZES`.
pub struct ScrollbarSizes;

impl ScrollbarSizes {
    pub const DEFAULT: f64 = 3.0;
    pub const LARGE: f64 = 10.0;
}

/// `MultiEditorTabsControl.TAB_WIDTH`.
pub struct TabWidth;

impl TabWidth {
    pub const COMPACT: f64 = 38.0;
    pub const SHRINK: f64 = 80.0;
    pub const FIT: f64 = 120.0;
}

/// `MODERN_UI_COMPACT_PINNED_TAB_WIDTH`.
pub const MODERN_UI_COMPACT_PINNED_TAB_WIDTH: f64 = 28.0;
/// `DRAG_OVER_OPEN_TAB_THRESHOLD` (ms).
pub const DRAG_OVER_OPEN_TAB_THRESHOLD: u64 = 1500;
/// `MOUSE_WHEEL_EVENT_THRESHOLD` (ms).
pub const MOUSE_WHEEL_EVENT_THRESHOLD: f64 = 150.0;
/// `MOUSE_WHEEL_DISTANCE_THRESHOLD`.
pub const MOUSE_WHEEL_DISTANCE_THRESHOLD: f64 = 1.5;

/// `IEditorInputLabel`.
#[derive(Clone)]
pub struct EditorInputLabel {
    pub editor: EditorInputRef,
    pub name: Option<String>,
    /// `None` é o `undefined`/`null` do original (descrição inválida).
    pub description: Option<String>,
    pub force_description: bool,
    pub title: Option<String>,
    pub aria_label: Option<String>,
}

/// `IMultiEditorTabsControlLayoutOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutOptions {
    /// Whether to force revealing the active tab, even when the dimensions have not changed. This can be the case when a tab
    /// was made active and needs to be revealed.
    pub force_reveal_active_tab: bool,
}

/// O separador de caminho do sistema (`this.path.sep`): o do Windows ou o do POSIX, conforme o sistema operacional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathSeparator {
    Windows,
    Posix,
}

impl PathSeparator {
    pub fn as_str(self) -> &'static str {
        match self {
            PathSeparator::Windows => "\\",
            PathSeparator::Posix => "/",
        }
    }

    /// `isWindows ? win32 : posix`.
    pub fn for_current_os() -> Self {
        if cfg!(windows) { PathSeparator::Windows } else { PathSeparator::Posix }
    }
}

/// `getLabelConfigFlags`.
pub struct LabelConfigFlags {
    pub verbosity: Verbosity,
    pub shorten_duplicates: bool,
}

/// `getLabelConfigFlags(value)`.
pub fn get_label_config_flags(value: EditorLabelFormat) -> LabelConfigFlags {
    match value {
        EditorLabelFormat::Short => LabelConfigFlags { verbosity: Verbosity::Short, shorten_duplicates: false },
        EditorLabelFormat::Medium => LabelConfigFlags { verbosity: Verbosity::Medium, shorten_duplicates: false },
        EditorLabelFormat::Long => LabelConfigFlags { verbosity: Verbosity::Long, shorten_duplicates: false },
        EditorLabelFormat::Default => LabelConfigFlags { verbosity: Verbosity::Medium, shorten_duplicates: true },
    }
}

/// `computeEditorAriaLabel(input, index, group, groupCount)` (de `workbench/browser/editor.ts`).
pub fn compute_editor_aria_label(host: &dyn TabsControlHost, tabs_model: &dyn IReadonlyEditorGroupModel, editor: &EditorInputRef, index: usize) -> String {
    let mut aria_label = editor.get_aria_label();
    if !host.is_pinned(editor) {
        aria_label = je_base::nls::localize("preview", "{0}, preview", &[aria_label.as_str()]);
    }

    if tabs_model.is_sticky(&EditorOrIndex::Index(index as i64)) {
        aria_label = je_base::nls::localize("pinned", "{0}, pinned", &[aria_label.as_str()]);
    }

    // Apply group information to help identify in which group we are (only if more than one group is actually opened)
    let group_count = host.editor_parts_count();
    if group_count > 1 {
        aria_label = format!("{}, {}", aria_label, host.group_aria_label());
    }

    aria_label
}

/// `equalsEditorInputLabel`.
pub fn equals_editor_input_label(a: Option<&EditorInputLabel>, b: Option<&EditorInputLabel>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.name == b.name && a.description == b.description && a.force_description == b.force_description && a.title == b.title && a.aria_label == b.aria_label,
        _ => false,
    }
}

/// `shortenTabLabels`: encurta (ou limpa) as descrições dos rótulos que repetem o nome.
pub fn shorten_tab_labels(labels: &mut [EditorInputLabel], separator: &str) {
    // Gather duplicate titles, while filtering out invalid descriptions
    let mut name_to_duplicates: Vec<(Option<String>, Vec<usize>)> = Vec::new();
    for index in 0..labels.len() {
        if labels[index].description.is_some() {
            let name = labels[index].name.clone();
            match name_to_duplicates.iter_mut().find(|(key, _)| *key == name) {
                Some((_, group)) => group.push(index),
                None => name_to_duplicates.push((name, vec![index])),
            }
        } else {
            labels[index].description = Some(String::new());
        }
    }

    // Identify duplicate names and shorten descriptions
    for (_, duplicates) in &name_to_duplicates {
        // Remove description if the title isn't duplicated and we have no indication to enforce description
        if duplicates.len() == 1 && !labels[duplicates[0]].force_description {
            labels[duplicates[0]].description = Some(String::new());
            continue;
        }

        // Identify duplicate descriptions
        let mut description_to_duplicates = group_by_description(labels, duplicates);

        // For editors with duplicate descriptions, check whether any long descriptions differ
        let mut use_long_descriptions = false;
        for (_, group) in &description_to_duplicates {
            if !use_long_descriptions && group.len() > 1 {
                let longs: Vec<Option<String>> = group.iter().map(|&index| labels[index].editor.get_description(Some(Verbosity::Long))).collect();
                let first = &longs[0];
                use_long_descriptions = longs[1..].iter().any(|description| description != first);
            }
        }

        // If so, replace all descriptions with long descriptions
        if use_long_descriptions {
            for &index in duplicates {
                labels[index].description = labels[index].editor.get_description(Some(Verbosity::Long));
            }
            description_to_duplicates = group_by_description(labels, duplicates);
        }

        // Obtain final set of descriptions
        let descriptions: Vec<Option<String>> = description_to_duplicates.iter().map(|(description, _)| description.clone()).collect();

        // Remove description if all descriptions are identical unless forced
        if descriptions.len() == 1 {
            for &index in &description_to_duplicates[0].1 {
                if !labels[index].force_description {
                    labels[index].description = Some(String::new());
                }
            }
            continue;
        }

        // Shorten descriptions
        let inputs: Vec<Option<&str>> = descriptions.iter().map(|description| description.as_deref()).collect();
        let shortened = shorten(&inputs, separator);
        for (position, (_, group)) in description_to_duplicates.iter().enumerate() {
            for &index in group {
                labels[index].description = shortened[position].clone();
            }
        }
    }
}

/// O `Map<string, IEditorInputLabel[]>` por descrição, na ordem de inserção.
fn group_by_description(labels: &[EditorInputLabel], indexes: &[usize]) -> Vec<(Option<String>, Vec<usize>)> {
    let mut groups: Vec<(Option<String>, Vec<usize>)> = Vec::new();
    for &index in indexes {
        let description = labels[index].description.clone();
        match groups.iter_mut().find(|(key, _)| *key == description) {
            Some((_, group)) => group.push(index),
            None => groups.push((description, vec![index])),
        }
    }

    groups
}

/// `computeTabLabels`: devolve os rótulos e o índice do ativo.
pub fn compute_tab_labels(
    host: &dyn TabsControlHost,
    tabs_model: &dyn IReadonlyEditorGroupModel,
    options: &EditorPartOptions,
    separator: PathSeparator,
) -> (Vec<EditorInputLabel>, Option<usize>) {
    let flags = get_label_config_flags(options.label_format);

    // Build labels and descriptions for each editor
    let mut labels: Vec<EditorInputLabel> = Vec::new();
    let mut active_editor_tab_index: Option<usize> = None;
    let active = tabs_model.active_editor();
    for (tab_index, editor) in tabs_model.get_editors(EditorsOrder::Sequential, None).into_iter().enumerate() {
        labels.push(EditorInputLabel {
            name: Some(editor.get_name()),
            description: editor.get_description(Some(flags.verbosity)),
            force_description: editor.has_capability(EditorInputCapabilities::FORCE_DESCRIPTION),
            title: Some(editor.get_title(Some(Verbosity::Long))),
            aria_label: Some(compute_editor_aria_label(host, tabs_model, &editor, tab_index)),
            editor: editor.clone(),
        });

        if active.as_ref().is_some_and(|active| id_of(active) == id_of(&editor)) {
            active_editor_tab_index = Some(tab_index);
        }
    }

    // Shorten labels as needed
    if flags.shorten_duplicates {
        shorten_tab_labels(&mut labels, separator.as_str());
    }

    (labels, active_editor_tab_index)
}

/// As cores do tema que o desenho das abas lê (`getColor`), já resolvidas. `None` é `null`/`undefined`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabColors {
    pub editor_group_header_tabs_border: Option<String>,
    pub tab_border: Option<String>,
    pub contrast_border: Option<String>,
    pub active_contrast_border: Option<String>,
    pub tab_last_pinned_border: Option<String>,
    pub tab_active_border: Option<String>,
    pub tab_unfocused_active_border: Option<String>,
    pub tab_active_border_top: Option<String>,
    pub tab_unfocused_active_border_top: Option<String>,
    pub tab_selected_border_top: Option<String>,
    pub tab_active_modified_border: Option<String>,
    pub tab_inactive_modified_border: Option<String>,
    pub tab_unfocused_active_modified_border: Option<String>,
    pub tab_unfocused_inactive_modified_border: Option<String>,
}

/// O que `redrawTab*` escreve em uma aba.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabState {
    pub sizing_fit: bool,
    pub sizing_shrink: bool,
    pub sizing_fixed: bool,
    pub has_icon: bool,
    pub sticky: bool,
    pub sticky_normal: bool,
    pub sticky_compact: bool,
    pub sticky_shrink: bool,
    /// `style.left`: `Some(px)` ou `None` (`auto`).
    pub left: Option<f64>,
    pub pinned_action_off: bool,
    pub close_action_off: bool,
    pub cannot_close: bool,
    pub tab_actions_left: bool,
    pub tab_actions_right: bool,
    pub active: bool,
    pub selected: bool,
    pub multi_selected: bool,
    pub dirty: bool,
    pub dirty_border_top: bool,
    pub dirty_border_top_color: Option<String>,
    pub tab_border_bottom: bool,
    pub tab_border_bottom_color: String,
    pub tab_border_top: bool,
    pub tab_border_top_color: String,
    pub border_right: String,
    pub outline_color: String,
    /// `--tab-sizing-fit-width`.
    pub fit_width: Option<f64>,
    /// `--tab-sizing-current-width` (o `fixed` com o mouse sobre as abas).
    pub current_width: Option<f64>,
    pub aria_selected: bool,
    pub focusable: bool,
    pub aria_label: Option<String>,
    pub last_tab: bool,
    pub resource_name: Option<String>,
    /// A ação da aba: `unpin`, `close` ou `closeOthers`.
    pub action: Option<TabAction>,
    /// O rótulo desenhado.
    pub label: TabLabelDraw,
}

/// A ação mostrada na barra de ações de uma aba.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAction {
    Unpin,
    Close,
    CloseOthers,
}

/// O que `redrawTabLabel` passa ao `setResource`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabLabelDraw {
    pub name: Option<String>,
    pub name_prefix: Option<String>,
    pub description: String,
    pub force_label: bool,
    pub file_decoration_badges: bool,
    pub file_decoration_colors: bool,
    pub italic: bool,
    pub hide_icon: bool,
    pub extra_classes: Vec<String>,
}

/// `getStickyTabWidth`.
pub fn get_sticky_tab_width(pinned_tab_sizing: PinnedTabSizing, has_modern_ui_tabs: bool) -> f64 {
    match pinned_tab_sizing {
        PinnedTabSizing::Compact => {
            if has_modern_ui_tabs {
                MODERN_UI_COMPACT_PINNED_TAB_WIDTH
            } else {
                TabWidth::COMPACT
            }
        }
        PinnedTabSizing::Shrink => TabWidth::SHRINK,
        PinnedTabSizing::Normal => 0.0,
    }
}

/// `getTabsScrollbarSizing`.
pub fn tabs_scrollbar_sizing(options: &EditorPartOptions) -> f64 {
    if options.title_scrollbar_sizing != TitleScrollbarSizing::Large { ScrollbarSizes::DEFAULT } else { ScrollbarSizes::LARGE }
}

/// `getTabsScrollbarVisibility`.
pub fn tabs_scrollbar_visibility(options: &EditorPartOptions) -> ScrollbarVisibility {
    match options.title_scrollbar_visibility {
        TitleScrollbarVisibility::Visible => ScrollbarVisibility::Visible,
        TitleScrollbarVisibility::Hidden => ScrollbarVisibility::Hidden,
        TitleScrollbarVisibility::Auto => ScrollbarVisibility::Auto,
    }
}

/// O Alt segurado e a aba sob o mouse (`isAltPressed`, `hoveredTabIndex`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HoverAltState {
    pub hovered_tab_index: Option<usize>,
    pub is_alt_pressed: bool,
}

impl HoverAltState {
    /// `setAltPressed`: devolve a aba cuja ação precisa ser redesenhada.
    pub fn set_alt_pressed(&mut self, is_alt_pressed: bool) -> Option<usize> {
        if self.is_alt_pressed == is_alt_pressed {
            return None;
        }

        self.is_alt_pressed = is_alt_pressed;

        // `updateTabActionForHoveredTab`
        self.hovered_tab_index
    }

    /// `setHoveredTab`: devolve as abas cuja ação precisa ser redesenhada.
    pub fn set_hovered_tab(&mut self, tab_index: Option<usize>) -> Vec<usize> {
        if self.hovered_tab_index == tab_index {
            return Vec::new();
        }

        let previous = self.hovered_tab_index;
        self.hovered_tab_index = tab_index;

        if !self.is_alt_pressed {
            return Vec::new(); // Alt is not held, no action swap in effect to redraw
        }

        previous.into_iter().chain(tab_index).collect()
    }

    /// O começo de `handleClosedEditors`: um índice velho deixaria uma aba com "Close Others".
    pub fn clear_if_stale(&mut self, tab_count: usize) -> Vec<usize> {
        if self.hovered_tab_index.is_some_and(|index| index >= tab_count) {
            return self.set_hovered_tab(None);
        }

        Vec::new()
    }
}

/// O agendamento do layout (`layoutScheduler`): no máximo um pendente, com as opções acumuladas.
#[derive(Default)]
pub struct LayoutScheduler {
    pending: RefCell<Option<LayoutOptions>>,
}

impl LayoutScheduler {
    /// Agenda (ou atualiza) o layout pendente. Devolve `true` quando este é o primeiro pedido do quadro.
    pub fn schedule(&self, options: Option<LayoutOptions>) -> bool {
        let mut pending = self.pending.borrow_mut();
        let first = pending.is_none();
        if first {
            *pending = Some(options.unwrap_or_default());
        }

        // Make sure to keep options updated
        if options.is_some_and(|options| options.force_reveal_active_tab)
            && let Some(current) = pending.as_mut()
        {
            current.force_reveal_active_tab = true;
        }

        first
    }

    /// O quadro chegou: devolve as opções do layout pendente e o libera.
    pub fn take(&self) -> Option<LayoutOptions> {
        self.pending.borrow_mut().take()
    }

    /// `layoutScheduler.value` existe.
    pub fn is_pending(&self) -> bool {
        self.pending.borrow().is_some()
    }
}

/// O `RunOnceScheduler` da atualização de rótulos: várias chamadas do mesmo quadro viram uma.
#[derive(Default)]
pub struct LabelUpdateScheduler {
    scheduled: RefCell<bool>,
}

impl LabelUpdateScheduler {
    /// `schedule()`: devolve `true` se este pedido acorda o agendador.
    pub fn schedule(&self) -> bool {
        !self.scheduled.replace(true)
    }

    /// O disparo: devolve se havia pedido.
    pub fn fire(&self) -> bool {
        self.scheduled.replace(false)
    }
}

/// As medidas que o original pede ao navegador no `doLayoutTabsNonWrapping`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabsMetrics {
    /// `tabsContainer.offsetWidth`.
    pub visible_tabs_width: f64,
    /// `tabsContainer.scrollWidth`.
    pub all_tabs_width: f64,
    /// `addTabContainer.offsetWidth` quando o contêiner de abas é conectado (senão 0).
    pub connected_add_tab_width: f64,
    /// A posição e a largura da aba ativa (`offsetLeft`, `offsetWidth`), se medidas.
    pub active_tab: Option<(f64, f64)>,
    /// As larguras `offsetWidth` das abas fixadas, em ordem (só lidas com as abas conectadas).
    pub sticky_tab_widths: Vec<f64>,
}

/// A decisão de `doLayoutTabsNonWrapping` para as abas fixadas e a rolagem.
#[derive(Clone, Debug, PartialEq)]
pub struct NonWrappingLayout {
    /// `style.left` de cada aba fixada.
    pub sticky_lefts: Vec<f64>,
    /// `stickyTabsBackground.style.width`.
    pub sticky_tabs_width: f64,
    /// A classe `disable-sticky-tabs`.
    pub disable_sticky_tabs: bool,
    /// `tabsScrollbar.setScrollDimensions`.
    pub scroll_width: f64,
    pub scroll_visible_width: f64,
    /// `tabsScrollbar.setScrollPosition` (a nova rolagem), se a aba ativa precisa ser revelada.
    pub reveal_scroll_left: Option<f64>,
    /// O novo `blockRevealActiveTab`.
    pub block_reveal_active_tab: bool,
}

/// `doLayoutTabsNonWrapping` sem o recorte das abas conectadas (fatia 2): posiciona as abas fixadas, mede o espaço e decide a
/// rolagem que revela a aba ativa.
#[allow(clippy::too_many_arguments)]
pub fn layout_tabs_non_wrapping(
    options: Option<LayoutOptions>,
    metrics: &TabsMetrics,
    sticky_count: usize,
    pinned_tab_sizing: PinnedTabSizing,
    has_modern_ui_tabs: bool,
    connected_tab_labels: bool,
    active_tab_index: Option<usize>,
    active_tab_sticky: bool,
    block_reveal_active_tab: bool,
    old_scroll: (f64, f64),
    scroll_left: f64,
) -> NonWrappingLayout {
    let visible_tabs_width = metrics.visible_tabs_width;
    let all_tabs_width = metrics.all_tabs_width;
    let visible_editor_tabs_width = visible_tabs_width - metrics.connected_add_tab_width;

    // Compute slot width of sticky tabs depending on pinned tab sizing
    // - compact: sticky-tabs * compact slot width
    // -  shrink: sticky-tabs * shrink slot width
    // -  normal: 0 (sticky tabs inherit look and feel from non-sticky tabs)
    let mut sticky_tabs_width = 0.0;
    let mut sticky_lefts = Vec::new();
    if sticky_count > 0 {
        let sticky_tab_width = get_sticky_tab_width(pinned_tab_sizing, has_modern_ui_tabs);

        for tab_index in 0..sticky_count {
            let tab_width = metrics.sticky_tab_widths.get(tab_index).copied();
            sticky_lefts.push(sticky_tabs_width);
            sticky_tabs_width += match tab_width {
                Some(width) if connected_tab_labels && sticky_tab_width > 0.0 => width,
                _ => sticky_tab_width,
            };
        }
    }

    // Figure out if active tab is positioned static which has an impact on whether to reveal the tab or not later
    let mut active_tab_position_static = pinned_tab_sizing != PinnedTabSizing::Normal && active_tab_index.is_some() && active_tab_sticky;

    // Special case: we have sticky tabs but the available space for showing tabs is little enough that we need to disable
    // sticky tabs sticky positioning so that tabs can be scrolled at naturally.
    let mut available_tabs_container_width = visible_editor_tabs_width - sticky_tabs_width;
    let mut disable_sticky_tabs = false;
    if sticky_count > 0 && available_tabs_container_width < TabWidth::FIT {
        disable_sticky_tabs = true;
        available_tabs_container_width = visible_editor_tabs_width;
        sticky_tabs_width = 0.0;
        active_tab_position_static = false;
    }

    let active = if block_reveal_active_tab { None } else { metrics.active_tab };

    let (old_visible_tabs_width, old_all_tabs_width) = old_scroll;
    let dimensions_changed = old_visible_tabs_width != visible_tabs_width || old_all_tabs_width != all_tabs_width;

    let skip = block_reveal_active_tab
        || active.is_none()
        || active_tab_position_static
        || (!dimensions_changed && !options.is_some_and(|options| options.force_reveal_active_tab));

    let mut layout = NonWrappingLayout {
        sticky_lefts,
        sticky_tabs_width,
        disable_sticky_tabs,
        scroll_width: all_tabs_width,
        scroll_visible_width: visible_tabs_width,
        reveal_scroll_left: None,
        block_reveal_active_tab: false,
    };

    let Some((active_tab_pos_x, active_tab_width)) = active else { return layout };
    if skip {
        return layout;
    }

    // Reveal the active one
    let adjusted_active_tab_pos_x = active_tab_pos_x - sticky_tabs_width;
    let active_tab_right = adjusted_active_tab_pos_x + active_tab_width;
    let active_tab_fits = active_tab_right - adjusted_active_tab_pos_x <= available_tabs_container_width;

    // Tab is overflowing to the right: Scroll minimally until the element is fully visible to the right. Note: only try to do
    // this if we actually have enough width to give to show the tab fully!
    if active_tab_fits && scroll_left + available_tabs_container_width < active_tab_right {
        layout.reveal_scroll_left = Some(scroll_left + (active_tab_right - (scroll_left + available_tabs_container_width)));
    }
    // Tab is overflowing to the left or does not fit: Scroll it into view to the left
    else if scroll_left > adjusted_active_tab_pos_x || !active_tab_fits {
        layout.reveal_scroll_left = Some(adjusted_active_tab_pos_x);
    }

    layout
}

/// A mudança de aba pelo mouse wheel (`MOUSE_WHEEL`): `Some(direção)` (`-1` ou `1`) ou `None` se o evento é ignorado.
#[allow(clippy::too_many_arguments)]
pub fn mouse_wheel_tab_direction(
    editor_count: usize,
    has_active_editor: bool,
    scroll_to_switch_tabs: bool,
    shift_key: bool,
    now_ms: f64,
    last_event_ms: f64,
    delta_x: f64,
    delta_y: f64,
) -> Option<i64> {
    if !has_active_editor || editor_count < 2 {
        return None; // need at least 2 open editors
    }

    // Shift-key enables or disables this behaviour depending on the setting
    if scroll_to_switch_tabs {
        if shift_key {
            return None; // 'on': only enable this when Shift-key is not pressed
        }
    } else if !shift_key {
        return None; // 'off': only enable this when Shift-key is pressed
    }

    // Ignore event if the last one happened too recently (https://github.com/microsoft/vscode/issues/96409). The restriction is
    // relaxed according to the absolute value of `deltaX` and `deltaY` to support discrete (mouse wheel) and contiguous
    // scrolling (touchpad) equally well
    if now_ms - last_event_ms < MOUSE_WHEEL_EVENT_THRESHOLD - 2.0 * (delta_x.abs() + delta_y.abs()) {
        return None;
    }

    // Figure out scrolling direction but ignore it if too subtle
    if delta_x + delta_y < -MOUSE_WHEEL_DISTANCE_THRESHOLD {
        Some(-1)
    } else if delta_x + delta_y > MOUSE_WHEEL_DISTANCE_THRESHOLD {
        Some(1)
    } else {
        None
    }
}

/// Uma aba da árvore: o rótulo e o estado desenhado.
#[derive(Clone)]
pub struct Tab {
    pub label: EditorInputLabel,
    pub state: TabState,
}

/// O resultado de `handleOpenedEditors`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenedEditorsOutcome {
    /// Redesenhou todas as abas (`redraw({ forceRevealActiveTab: true })`).
    Redrawn,
    /// Só pediu o layout para revelar a aba ativa.
    LayoutOnly,
}

/// O estado do `MultiEditorTabsControl` que esta fatia cobre.
pub struct MultiEditorTabsControlState {
    pub base: EditorTabsControlBase,
    pub tabs: Vec<Tab>,
    pub tab_labels: Vec<EditorInputLabel>,
    pub active_tab_label: Option<EditorInputLabel>,
    pub hover_alt: HoverAltState,
    pub dimensions: TitleDimensions,
    pub layout_scheduler: Rc<LayoutScheduler>,
    pub label_scheduler: Rc<LabelUpdateScheduler>,
    pub block_reveal_active_tab: bool,
    pub path: PathSeparator,
    pub is_mouse_over_tabs: bool,
    pub last_mouse_wheel_event_time: f64,
    pub connected_tab_labels: bool,
    /// As cores do tema (o `Themable.getColor`), resolvidas pelo chamador a cada troca de tema.
    pub colors_cell: RefCell<TabColors>,
    /// O contêiner de abas está vazio (`tabs-and-actions-container.empty`).
    pub empty: bool,
}

/// `dimensions: IEditorTitleControlDimensions & { used?: Dimension }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleDimensions {
    pub container: Option<Dimension>,
    pub available: Option<Dimension>,
    pub used: Option<Dimension>,
}

impl Default for TitleDimensions {
    fn default() -> Self {
        Self { container: None, available: None, used: None }
    }
}

impl MultiEditorTabsControlState {
    pub fn new(base: EditorTabsControlBase) -> Self {
        Self {
            base,
            tabs: Vec::new(),
            tab_labels: Vec::new(),
            active_tab_label: None,
            hover_alt: HoverAltState::default(),
            dimensions: TitleDimensions::default(),
            layout_scheduler: Rc::new(LayoutScheduler::default()),
            label_scheduler: Rc::new(LabelUpdateScheduler::default()),
            block_reveal_active_tab: false,
            path: PathSeparator::for_current_os(),
            is_mouse_over_tabs: false,
            last_mouse_wheel_event_time: 0.0,
            connected_tab_labels: false,
            colors_cell: RefCell::new(TabColors::default()),
            empty: true,
        }
    }

    /// `updateStyles`: recebe as cores novas do tema e redesenha.
    pub fn set_colors(&mut self, colors: TabColors) {
        *self.colors_cell.borrow_mut() = colors;
        self.redraw(None);
    }

    fn host(&self) -> Rc<dyn TabsControlHost> {
        self.base.host.clone()
    }

    fn options(&self) -> EditorPartOptions {
        self.host().part_options()
    }

    /// `visible`.
    pub fn visible(&self) -> bool {
        self.base.tabs_model.count() > 0
    }

    /// `tabCount`.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// `updateTabsControlVisibility`.
    pub fn update_tabs_control_visibility(&mut self) {
        self.empty = !self.visible();

        // Reset dimensions if hidden
        if !self.visible() {
            self.dimensions.used = None;
        }
    }

    /// `computeTabLabels`.
    pub fn compute_tab_labels(&mut self) {
        let host = self.host();
        let (labels, active_index) = compute_tab_labels(host.as_ref(), self.base.tabs_model.as_ref(), &self.options(), self.path);
        self.active_tab_label = active_index.and_then(|index| labels.get(index).cloned());
        self.tab_labels = labels;
        self.sync_tab_labels();
    }

    /// Leva os rótulos calculados para as linhas da árvore.
    fn sync_tab_labels(&mut self) {
        for (tab, label) in self.tabs.iter_mut().zip(self.tab_labels.iter()) {
            tab.label = label.clone();
        }
    }

    /// `didActiveEditorChange`.
    pub fn did_active_editor_change(&self) -> bool {
        let model_active = self.base.tabs_model.active_editor();
        let label_editor = self.active_tab_label.as_ref().map(|label| &label.editor);

        (label_editor.is_none() && model_active.is_some())
            || (label_editor.is_some() && model_active.is_none())
            || label_editor.is_none_or(|editor| !self.base.tabs_model.is_active(&crate::common::editor::EditorInputOrUntyped::Typed(editor.clone())))
    }

    /// `handleOpenedEditors`: cria as abas que faltam, recomputa os rótulos e redesenha se algo mudou. Devolve se mudou.
    pub fn handle_opened_editors(&mut self, create_tab: &mut dyn FnMut(usize) -> Tab) -> bool {
        // Set tabs control visibility
        self.update_tabs_control_visibility();

        // Create tabs as needed
        for index in self.tab_count()..self.base.tabs_model.count() {
            let tab = create_tab(index);
            self.tabs.push(tab);
        }

        // Make sure to recompute tab labels and detect if a label change occurred that requires a redraw of tabs.
        let active_editor_changed = self.did_active_editor_change();
        let old_tab_labels = self.tab_labels.clone();
        self.compute_tab_labels();

        // Redraw and update in these cases
        let changed = active_editor_changed
            || old_tab_labels.len() != self.tab_labels.len()
            || old_tab_labels.iter().enumerate().any(|(index, label)| !equals_editor_input_label(Some(label), self.tab_labels.get(index)));

        if changed {
            self.redraw(Some(LayoutOptions { force_reveal_active_tab: true }));
        } else {
            self.layout_scheduler.schedule(Some(LayoutOptions { force_reveal_active_tab: true }));
        }

        changed
    }

    /// `handleClosedEditors`.
    pub fn handle_closed_editors(&mut self) {
        // A stale hovered tab index could otherwise leave a tab showing "Close Others" after the tabs it pointed past got
        // removed
        let count = self.base.tabs_model.count();
        for index in self.hover_alt.clear_if_stale(count) {
            self.redraw_tab_action_at_index(index);
        }

        // There are tabs to show
        if count > 0 {
            // Remove tabs that got closed (must be the last to keep indexes in order!)
            while self.tab_count() > count {
                self.tabs.pop();
            }

            // A removal of a label requires to recompute all labels
            self.compute_tab_labels();

            // Redraw all tabs
            self.redraw(Some(LayoutOptions { force_reveal_active_tab: true }));
        }
        // No tabs to show
        else {
            self.tabs.clear();
            self.tab_labels.clear();
            self.active_tab_label = None;

            self.base.toolbars.clear();
            self.update_tabs_control_visibility();
        }
    }

    /// `moveEditor`: move o rótulo e redesenha as abas no intervalo.
    pub fn move_editor(&mut self, from_tab_index: usize, target_tab_index: usize) {
        // Move the editor label
        if from_tab_index < self.tab_labels.len() {
            let label = self.tab_labels.remove(from_tab_index);
            let at = target_tab_index.min(self.tab_labels.len());
            self.tab_labels.insert(at, label);
        }
        self.sync_tab_labels();

        // Redraw tabs in the range of the move
        let (from, to) = (from_tab_index.min(target_tab_index), from_tab_index.max(target_tab_index));
        for index in from..=to {
            self.redraw_tab_at(index);
        }

        // Moving an editor requires a layout to keep the active editor visible
        self.layout_scheduler.schedule(Some(LayoutOptions { force_reveal_active_tab: true }));
    }

    /// `doHandleStickyEditorChange`.
    pub fn handle_sticky_editor_change(&mut self, tab_index: usize) {
        // Update tab
        self.redraw_tab_at(tab_index);

        // Sticky change has an impact on each tab's border because it potentially moves the border to the last pinned tab
        for index in 0..self.tabs.len() {
            self.redraw_tab_borders_at(index);
        }

        // A change to the sticky state requires a layout to keep the active editor visible
        self.layout_scheduler.schedule(Some(LayoutOptions { force_reveal_active_tab: true }));
    }

    /// `beforeCloseEditor`.
    pub fn before_close_editor(&mut self, editor: &EditorInputRef) {
        // Fix tabs width if the mouse is over tabs and before closing a tab (except the last tab) when tab sizing is 'fixed'.
        // This helps keeping the close button stable under the mouse and allows for rapid closing of tabs.
        if self.is_mouse_over_tabs && self.options().tab_sizing == TabSizing::Fixed {
            let closing_last_tab = self.base.tabs_model.is_last(editor, None);
            self.update_tabs_fixed_width(!closing_last_tab, &[]);
        }
    }

    /// `updateTabsFixedWidth(fixed)`: com `fixed`, fixa a largura atual de cada aba (`widths`, na ordem); sem, solta.
    pub fn update_tabs_fixed_width(&mut self, fixed: bool, widths: &[f64]) {
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            tab.state.current_width = if fixed { widths.get(index).copied() } else { None };
        }
    }

    /// O que `updateOptions` decide além da base.
    pub fn update_options(&mut self, old_options: &EditorPartOptions, new_options: &EditorPartOptions) -> UpdateOptionsOutcome {
        let toolbars_changed = self.base.update_options(old_options, new_options);
        let mut outcome = UpdateOptionsOutcome { toolbars_changed, ..UpdateOptionsOutcome::default() };

        if old_options.tab_height != new_options.tab_height {
            self.clear_fit_widths();
        }

        // A change to a label format options requires to recompute all labels
        if old_options.label_format != new_options.label_format {
            self.compute_tab_labels();
        }

        // Update tabs scrollbar sizing
        outcome.scrollbar_sizing = (old_options.title_scrollbar_sizing != new_options.title_scrollbar_sizing).then(|| tabs_scrollbar_sizing(new_options));

        // Update tabs scrollbar visibility
        outcome.scrollbar_visibility = (old_options.title_scrollbar_visibility != new_options.title_scrollbar_visibility).then(|| tabs_scrollbar_visibility(new_options));

        // Update editor actions
        outcome.update_editor_actions = old_options.always_show_editor_actions != new_options.always_show_editor_actions;

        // Update tabs sizing
        outcome.update_tab_sizing = old_options.tab_sizing_fixed_min_width != new_options.tab_sizing_fixed_min_width
            || old_options.tab_sizing_fixed_max_width != new_options.tab_sizing_fixed_max_width
            || old_options.tab_sizing != new_options.tab_sizing;

        // Redraw tabs when other options change
        if old_options.label_format != new_options.label_format
            || old_options.tab_action_location != new_options.tab_action_location
            || old_options.tab_action_reserve_space != new_options.tab_action_reserve_space
            || old_options.tab_action_close_visibility != new_options.tab_action_close_visibility
            || old_options.tab_action_unpin_visibility != new_options.tab_action_unpin_visibility
            || old_options.tab_sizing != new_options.tab_sizing
            || old_options.pinned_tab_sizing != new_options.pinned_tab_sizing
            || old_options.show_icons != new_options.show_icons
            || old_options.has_icons != new_options.has_icons
            || old_options.highlight_modified_tabs != new_options.highlight_modified_tabs
            || old_options.wrap_tabs != new_options.wrap_tabs
            || old_options.show_tab_index != new_options.show_tab_index
            || old_options.decorations != new_options.decorations
        {
            self.redraw(None);
            outcome.redrawn = true;
        }

        outcome
    }

    /// `doHandleDecorationsChange` e o começo de `updateOptions`: solta a largura `fit` de cada aba.
    pub fn clear_fit_widths(&mut self) {
        for tab in &mut self.tabs {
            tab.state.fit_width = None;
        }
    }

    /// `updateEditorDirty`.
    pub fn update_editor_dirty(&mut self, editor: &EditorInputRef) {
        let Some(index) = self.tab_index_of(editor) else { return };
        self.tabs[index].state.fit_width = None;
        self.redraw_tab_selected_active_and_dirty_at(index);
    }

    /// `redrawTabLabel` de uma aba (os rótulos já calculados).
    pub fn pin_editor(&mut self, editor: &EditorInputRef) {
        if let Some(index) = self.tab_index_of(editor) {
            self.redraw_tab_label_at(index);
        }
    }

    /// `doUpdateEditorLabels`.
    pub fn do_update_editor_labels(&mut self) {
        // A change to a label requires to recompute all labels
        self.compute_tab_labels();

        // As such we need to redraw each label
        for index in 0..self.tabs.len() {
            self.redraw_tab_label_at(index);
        }

        // A change to a label requires a layout to keep the active editor visible
        self.layout_scheduler.schedule(None);
    }

    /// `setActive`.
    pub fn set_active(&mut self) {
        for index in 0..self.tabs.len() {
            self.redraw_tab_selected_active_and_dirty_at(index);
        }

        self.layout_scheduler.schedule(Some(LayoutOptions { force_reveal_active_tab: true }));
    }

    /// `layout(dimensions, options)`: guarda as medidas, agenda o desenho e devolve as dimensões usadas.
    pub fn layout(&mut self, dimensions: EditorTitleControlDimensions, options: Option<LayoutOptions>, computed_height: f64) -> Option<Dimension> {
        // Remember dimensions that we get
        self.dimensions.container = dimensions.container;
        self.dimensions.available = dimensions.available;

        if self.visible() {
            self.layout_scheduler.schedule(options);
        }

        // First time layout: compute the dimensions and store it
        if self.dimensions.used.is_none() {
            let width = dimensions.container.map_or(0.0, |container| container.width);
            self.dimensions.used = Some(Dimension { width, height: computed_height });
        }

        self.dimensions.used
    }

    /// O fim de `doLayout`: guarda a dimensão usada e devolve se a altura mudou (então o grupo precisa de `relayout`).
    pub fn finish_layout(&mut self, container_width: f64, computed_height: f64) -> bool {
        let old = self.dimensions.used;
        let new = Dimension { width: container_width, height: computed_height };
        self.dimensions.used = Some(new);

        old.is_some_and(|old| old.height != new.height)
    }

    /// `getHeight`.
    pub fn get_height(&self, computed_height: f64) -> f64 {
        // Return quickly if our used dimensions are known
        match self.dimensions.used {
            Some(used) => used.height,
            None => computed_height,
        }
    }

    /// `computeHeight`: `wrapping_or_connected_height` é o `offsetHeight` do contêiner quando há quebra de linha ou abas
    /// conectadas.
    pub fn compute_height(&self, wrapping_or_connected_height: Option<f64>) -> f64 {
        if !self.visible() {
            0.0
        } else if let Some(height) = wrapping_or_connected_height {
            height
        } else {
            self.base.tab_height()
        }
    }

    /// `prepareEditorActions`: com o grupo inativo, só "Unlock" e as secundárias (ou todas com `alwaysShowEditorActions`).
    pub fn prepare_editor_actions(&self, actions: crate::services::editor::editor_groups_service::ToolbarActions, unlock_group_command_id: &str) -> crate::services::editor::editor_groups_service::ToolbarActions {
        // Active: allow all actions
        if self.host().is_group_active() {
            return actions;
        }

        // Inactive: only show "Unlock" and secondary actions
        crate::services::editor::editor_groups_service::ToolbarActions {
            primary: if self.options().always_show_editor_actions { actions.primary } else { actions.primary.into_iter().filter(|action| action.id() == unlock_group_command_id).collect() },
            secondary: actions.secondary,
        }
    }

    /// `toEditorIndex`: o índice do editor no grupo inteiro, dado o índice da aba.
    pub fn to_editor_index(&self, tab_index: usize) -> i64 {
        let editor = self.base.tabs_model.get_editor_by_index(tab_index as i64).expect("tab index out of range");
        self.host().index_of_editor(&editor)
    }

    fn tab_index_of(&self, editor: &EditorInputRef) -> Option<usize> {
        usize::try_from(self.base.tabs_model.index_of(Some(&crate::common::editor::EditorInputOrUntyped::Typed(editor.clone())), None, None)).ok()
    }

    /// `redraw`: redesenha cada aba e agenda o layout.
    pub fn redraw(&mut self, options: Option<LayoutOptions>) {
        for index in 0..self.tabs.len() {
            self.redraw_tab_at(index);
        }

        self.layout_scheduler.schedule(options);
    }

    fn redraw_tab_action_at_index(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.redraw_tab_action_at(index);
        }
    }

    /// `redrawTab`.
    pub fn redraw_tab_at(&mut self, index: usize) {
        if index >= self.tabs.len() || index >= self.tab_labels.len() {
            return;
        }
        let Some(editor) = self.base.tabs_model.get_editor_by_index(index as i64) else { return };
        let is_tab_sticky = self.base.tabs_model.is_sticky(&EditorOrIndex::Index(index as i64));
        let options = self.options();

        // Label
        self.redraw_tab_label_at(index);

        // Action
        self.redraw_tab_action_at(index);

        let tab_sizing = if is_tab_sticky && options.pinned_tab_sizing == PinnedTabSizing::Shrink { TabSizing::Shrink /* treat sticky shrink tabs as tabSizing: 'shrink' */ } else { options.tab_sizing };
        let sticky_left = if !options.wrap_tabs && is_tab_sticky && options.pinned_tab_sizing != PinnedTabSizing::Normal {
            // If not wrapping tabs, sticky compact/shrink tabs need a position to remain at their location when scrolling to
            // stay in view (requirement for position: sticky)
            Some(index as f64 * get_sticky_tab_width(options.pinned_tab_sizing, self.base.parent.in_modern_ui_tabs))
        } else {
            None
        };

        let state = &mut self.tabs[index].state;
        state.sizing_fit = tab_sizing == TabSizing::Fit;
        state.sizing_shrink = tab_sizing == TabSizing::Shrink;
        state.sizing_fixed = tab_sizing == TabSizing::Fixed;
        state.has_icon = options.show_icons && options.has_icons;
        state.sticky = is_tab_sticky;
        state.sticky_normal = is_tab_sticky && options.pinned_tab_sizing == PinnedTabSizing::Normal;
        state.sticky_compact = is_tab_sticky && options.pinned_tab_sizing == PinnedTabSizing::Compact;
        state.sticky_shrink = is_tab_sticky && options.pinned_tab_sizing == PinnedTabSizing::Shrink;
        state.left = sticky_left;

        // Borders / outline
        self.redraw_tab_borders_at(index);

        // Selection / active / dirty state
        self.redraw_tab_selected_active_and_dirty_at(index);
        let _ = editor;
    }

    /// `redrawTabAction`: escolhe a ação da aba e as classes dela.
    pub fn redraw_tab_action_at(&mut self, index: usize) {
        let Some(editor) = self.base.tabs_model.get_editor_by_index(index as i64) else { return };
        if index >= self.tabs.len() {
            return;
        }
        let is_tab_sticky = self.base.tabs_model.is_sticky(&EditorOrIndex::Index(index as i64));
        let is_closeable = !editor.has_capability(EditorInputCapabilities::CANNOT_CLOSE);
        let options = self.options();

        let has_unpin_action = is_tab_sticky && options.tab_action_unpin_visibility;
        let has_close_action = is_closeable && !has_unpin_action && options.tab_action_close_visibility;
        let has_action = has_unpin_action || has_close_action;

        // Alt swaps a visible Close action to Close Others, but only for the currently hovered tab; Unpin is unaffected.
        let wants_close_others_action = has_close_action && self.hover_alt.is_alt_pressed && Some(index) == self.hover_alt.hovered_tab_index;

        let action = if has_action {
            if has_unpin_action {
                TabAction::Unpin
            } else if wants_close_others_action {
                TabAction::CloseOthers
            } else {
                TabAction::Close
            }
        } else if is_tab_sticky {
            // Even if the action is not visible, add it as it contains the dirty indicator
            TabAction::Unpin
        } else {
            TabAction::Close
        };

        let state = &mut self.tabs[index].state;
        state.action = Some(action);
        state.pinned_action_off = is_tab_sticky && !has_unpin_action;
        state.close_action_off = !has_unpin_action && !has_close_action;
        state.cannot_close = !is_closeable;
        state.tab_actions_left = has_action && options.tab_action_location == TabActionLocation::Left;
        state.tab_actions_right = has_action && options.tab_action_location == TabActionLocation::Right;
    }

    /// `redrawTabLabel`.
    pub fn redraw_tab_label_at(&mut self, index: usize) {
        let Some(editor) = self.base.tabs_model.get_editor_by_index(index as i64) else { return };
        let (Some(tab_label), true) = (self.tab_labels.get(index).cloned(), index < self.tabs.len()) else { return };
        let options = self.options();
        let is_sticky = self.base.tabs_model.is_sticky(&EditorOrIndex::Index(index as i64));

        // Unless tabs are sticky compact, show the full label and description. Sticky compact tabs will only show an icon if
        // icons are enabled or their first character of the name otherwise
        let mut name_prefix = None;
        let mut force_label = false;
        let mut file_decoration_badges = options.decorations.badges;
        let file_decoration_colors = options.decorations.colors;
        let (name, description);
        if options.pinned_tab_sizing == PinnedTabSizing::Compact && is_sticky {
            let is_showing_icons = options.show_icons && options.has_icons;
            name = if is_showing_icons { Some(String::new()) } else { tab_label.name.as_deref().map(|name| name.chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default()) };
            description = String::new();
            force_label = true;
            file_decoration_badges = false; // not enough space when sticky tabs are compact
        } else {
            name = tab_label.name.clone();
            if options.show_tab_index {
                name_prefix = Some(format!("{}: ", self.to_editor_index(index) + 1));
            }
            description = tab_label.description.clone().unwrap_or_default();
        }

        let resource = EditorResourceAccessor::get_original_uri(
            Some(&crate::common::editor::EditorInputOrUntyped::Typed(editor.clone())),
            Some(&EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), ..Default::default() }),
        );
        let mut extra_classes = vec!["tab-label".to_string()];
        if file_decoration_badges {
            extra_classes.push("tab-label-has-badge".to_string());
        }
        extra_classes.extend(editor.get_label_extra_classes());

        let italic = !self.base.tabs_model.is_pinned(&EditorOrIndex::Index(index as i64));
        let state = &mut self.tabs[index].state;
        state.fit_width = None;
        state.aria_label = tab_label.aria_label.clone().filter(|label| !label.is_empty());
        state.label = TabLabelDraw {
            name,
            name_prefix,
            description,
            force_label,
            file_decoration_badges,
            file_decoration_colors,
            italic,
            hide_icon: !options.show_icons,
            extra_classes,
        };

        // Tests helper
        state.resource_name = resource.map(|resource| je_platform::resources::ext_uri().basename_or_authority(&resource));
    }

    /// `redrawTabSelectedActiveAndDirty`.
    pub fn redraw_tab_selected_active_and_dirty_at(&mut self, index: usize) {
        let Some(editor) = self.base.tabs_model.get_editor_by_index(index as i64) else { return };
        if index >= self.tabs.len() {
            return;
        }
        let is_group_active = self.host().is_group_active();
        let is_tab_active = self.base.tabs_model.is_active(&crate::common::editor::EditorInputOrUntyped::Typed(editor.clone()));
        let has_modified_border_top = self.redraw_tab_dirty(is_group_active, is_tab_active, index, &editor);

        self.redraw_tab_active(is_group_active, !has_modified_border_top, index, &editor);
    }

    /// `doRedrawTabActive`, com as cores resolvidas em [`TabColors`].
    fn redraw_tab_active(&mut self, is_group_active: bool, allow_border_top: bool, index: usize, editor: &EditorInputRef) {
        let colors = self.colors();
        let editor_or_index = EditorOrIndex::Editor(editor.clone());
        let is_active = self.base.tabs_model.is_active(&crate::common::editor::EditorInputOrUntyped::Typed(editor.clone()));
        let is_selected = self.base.tabs_model.is_selected(&editor_or_index);
        let multi = is_selected && self.host().selected_editors().len() > 1;

        let state = &mut self.tabs[index].state;
        state.active = is_active;
        state.selected = is_selected;
        state.multi_selected = multi;
        state.aria_selected = is_selected;
        state.focusable = is_active; // Only active tab can be focused into

        // Set border BOTTOM if theme defined color
        if is_active {
            let color = if is_group_active { colors.tab_active_border.clone() } else { colors.tab_unfocused_active_border.clone() };
            state.tab_border_bottom = color.is_some();
            state.tab_border_bottom_color = color.unwrap_or_default();
        }

        // Set border TOP if theme defined color
        let mut border_top: Option<String> = None;
        if allow_border_top {
            if is_active {
                border_top = if is_group_active { colors.tab_active_border_top.clone() } else { colors.tab_unfocused_active_border_top.clone() };
            }

            if border_top.is_none() && is_selected {
                border_top = colors.tab_selected_border_top.clone();
            }
        }

        state.tab_border_top = border_top.is_some();
        state.tab_border_top_color = border_top.unwrap_or_default();
    }

    /// `doRedrawTabDirty`: devolve se a aba tem a borda superior de "modificada".
    fn redraw_tab_dirty(&mut self, is_group_active: bool, is_tab_active: bool, index: usize, editor: &EditorInputRef) -> bool {
        let colors = self.colors();
        let highlight = self.options().highlight_modified_tabs;
        let state = &mut self.tabs[index].state;
        let mut has_modified_border_color = false;

        // Tab: dirty (unless saving)
        if editor.is_dirty() && !editor.is_saving() {
            state.dirty = true;

            // Highlight modified tabs with a border if configured
            if highlight {
                let modified_border_color = match (is_group_active, is_tab_active) {
                    (true, true) => colors.tab_active_modified_border.clone(),
                    (true, false) => colors.tab_inactive_modified_border.clone(),
                    (false, true) => colors.tab_unfocused_active_modified_border.clone(),
                    (false, false) => colors.tab_unfocused_inactive_modified_border.clone(),
                };

                if let Some(color) = modified_border_color {
                    has_modified_border_color = true;
                    state.dirty_border_top = true;
                    state.dirty_border_top_color = Some(color);
                }
            } else {
                state.dirty_border_top = false;
                state.dirty_border_top_color = None;
            }
        }
        // Tab: not dirty
        else {
            state.dirty = false;
            state.dirty_border_top = false;
            state.dirty_border_top_color = None;
        }

        has_modified_border_color
    }

    /// `redrawTabBorders`.
    pub fn redraw_tab_borders_at(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let colors = self.colors();
        let model = &self.base.tabs_model;
        let is_tab_sticky = model.is_sticky(&EditorOrIndex::Index(index as i64));
        let is_tab_last_sticky = is_tab_sticky && model.sticky_count() == index + 1;
        let show_last_sticky_tab_border_color = model.sticky_count() != model.count();

        // Borders / Outline
        let border_right = (if is_tab_last_sticky && show_last_sticky_tab_border_color { colors.tab_last_pinned_border.clone() } else { None })
            .or_else(|| colors.tab_border.clone())
            .or_else(|| colors.contrast_border.clone());

        let state = &mut self.tabs[index].state;
        state.border_right = border_right.map(|color| format!("1px solid {color}")).unwrap_or_default();
        state.outline_color = colors.active_contrast_border.clone().unwrap_or_default();
    }

    /// As cores do tema, definidas pelo chamador via [`MultiEditorTabsControlState::set_colors`].
    fn colors(&self) -> TabColors {
        self.colors_cell.borrow().clone()
    }
}

/// O que `updateOptions` pede ao desenho nativo além do que o estado já atualizou.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UpdateOptionsOutcome {
    pub toolbars_changed: bool,
    pub scrollbar_sizing: Option<f64>,
    pub scrollbar_visibility: Option<ScrollbarVisibility>,
    pub update_editor_actions: bool,
    pub update_tab_sizing: bool,
    pub redrawn: bool,
}
