//! Porte de `src/vs/workbench/browser/parts/editor/multiEditorTabsControl.ts`.
//!
//! Fatia 1: o modelo de rótulos das
//! abas (`computeTabLabels`, `shortenTabLabels`, `getLabelConfigFlags`, `equalsEditorInputLabel`), o estado desenhado de cada
//! aba (`redrawTab`, `redrawTabAction`, `redrawTabLabel`, `redrawTabSelectedActiveAndDirty`, `redrawTabBorders`), a escolha da
//! ação da aba com o Alt e a aba sob o mouse, os dimensionamentos `fit`, `shrink` e `fixed`, a largura das abas fixadas, o
//! agendamento do layout, a rolagem para revelar a aba ativa (`doLayoutTabsNonWrapping`), a rolagem do mouse para trocar de
//! aba e o que `handleOpenedEditors`, `handleClosedEditors`, `moveEditor`, `pinEditor` e as trocas de estado decidem.
//!
//! Fatia 2: `registerTabListeners` (clique, toque, duplo clique, botão do meio, teclado, menu de contexto, entrada e saída do
//! mouse), `registerTabsContainerListeners` (rolagem, duplo clique no vazio, Alt, roda do mouse, menu da barra), a seleção
//! múltipla (`selectEditor`, `selectEditorsBetween`, `unselectEditor`, `unselectAllEditors`), o arrastar e soltar entre abas
//! e grupos (`isSupportedDropTransfer`, `updateDropFeedback`, `updateDropTarget`, `computeDropTarget`,
//! `getTabDragOverLocation`, `onDrop`), `doLayout`, `doLayoutTabs`, `doLayoutTabsWrapping`, `updateWrappedAddTabAlignment`,
//! `layoutFitTabsWithoutReservedActions`, `getFitTabWidth` (com as medidas), o recorte das abas conectadas
//! (`clearConnectedTabClipping`, `updateConnectedTabClipping`, e as duas funções de `connectedTabClipping.ts`),
//! `originatesFromTabActionBar` e `preventEditorClose` (de `common/editor.ts`).
//!
//! Divergências:
//! - **Sem DOM.** A árvore de abas é `tabs` ([`Tab`]): uma linha por aba com o que o original escreve no `HTMLElement` (classes,
//!   `--tab-sizing-fit-width`, `left`, bordas, `aria-*`, `clip-path`), e [`TabsContainerView`] guarda o que ele escreve nos
//!   contêineres. O desenho nativo lê essas estruturas; as medidas que o original pede ao navegador (`offsetWidth`,
//!   `scrollWidth`, `offsetLeft`, `getBoundingClientRect`, `getComputedStyle`) chegam pelo [`TabsMeasurer`] (e por
//!   [`TabsMetrics`] no layout sem quebra), que responde pelo estado vigente a cada leitura, porque o layout reflui quando o
//!   controle escreve uma classe (por exemplo `wrapping`).
//! - **Cores do tema** (`getColor`) chegam como [`TabColors`], já resolvidas pelo chamador (o `Themable`).
//! - **Serviços e grupo** (`IPathService`, `IEditorService`, `IKeybindingService`, `IContextMenuService`, o `groupView` e o
//!   `editorPartsView`) chegam como o separador de caminho ([`PathSeparator`]), as consultas de [`TabsControlHost`] e as
//!   ações de [`TabsGroupOps`] (abrir, selecionar, fixar, mover, copiar, mesclar, menus); a montagem dos widgets
//!   (`ActionBar`, `ScrollableElement`, `ResourceLabels.create`) fica com a renderização nativa.
//! - **Eventos.** Cada listener é um método que recebe o evento já decodificado ([`TabPointerEvent`], [`TabKey`], [`TabTap`],
//!   [`DragModifiers`], [`DropTransfer`], [`WheelInput`]) e devolve o que o original faz com o `Event` (interromper, impedir o
//!   padrão, o `dropEffect`). `EventHelper.stop` e `preventDefault` ficam com quem entrega o evento.
//! - **Transferências de arrastar** (`LocalSelectionTransfer`, `DataTransfer`) são [`DropTransfer`], e o que o `onDrop` faz
//!   com recursos e itens de árvore (`ResourcesDropHandler`, `ITreeViewsDnDService`) é o [`TabsGroupOps`].
//! - **Agendamento.** `scheduleAtNextAnimationFrame` é o [`LayoutScheduler`]: guarda as opções do layout pedido e quem
//!   executa o quadro chama [`LayoutScheduler::take`]. O `RunOnceScheduler` dos rótulos é [`LabelUpdateScheduler`].
//! - **Assíncrono.** As funções `async` do original (`selectEditor`, `onDrop`...) são síncronas: o que elas aguardam
//!   (`groupView.openEditor`, `setSelection`) é do [`TabsGroupOps`], que o host resolve.
//! - **Ficam com a renderização nativa**: o `onGroupDragStart` e o `onGroupDragEnd` (da base, ver
//!   `editor_tabs_control.rs`), `doFillResourceDataTransfers` e `applyDragImage`.
//!
//! Plano das fatias seguintes (o que NÃO está aqui):
//! - Fatia 3: o porte do teste `multiEditorTabsControl.test.ts` (86 casos) sobre este controle.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::Error;
use je_base::labels::shorten;
use je_base::lifecycle::id_of;
use je_base::platform::Platform;
use je_base::scrollable::ScrollbarVisibility;
use je_editor::core_2d::dimension::Dimension;

use crate::browser::parts::editor::editor_tabs_control::{DragModifiers, EditorTabsControlBase, EditorTitleControlDimensions, TabsControlHost};
use crate::common::editor::editor_group_model::{EditorOrIndex, IReadonlyEditorGroupModel};
use crate::common::editor::{
    EditorCloseMethod, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, EditorsOrder, GroupIdentifier, PreventPinnedEditorClose, SideBySideEditor, Verbosity,
};
use crate::common::editor::EditorResourceAccessor;
use crate::common::editor::EditorResourceAccessorOptions;
use crate::services::editor::editor_groups_service::{
    DoubleClickTabAction, EditorLabelFormat, EditorPartOptions, MergeGroupMode, MergeGroupOptions, PinnedTabSizing, TabActionLocation, TabSizing, TitleScrollbarSizing,
    TitleScrollbarVisibility,
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
    /// `drop-target-left` e `drop-target-right` (`updateDropTarget`).
    pub drop_target_left: bool,
    pub drop_target_right: bool,
    /// `tab-action-sizing-rest`: a aba é medida sem a coluna da ação (`layoutFitTabsWithoutReservedActions`).
    pub tab_action_sizing_rest: bool,
    /// `connected-tab-last`, `connected-tab-upper-row` e `connected-tab-top-row`.
    pub connected_tab_last: bool,
    pub connected_tab_upper_row: bool,
    pub connected_tab_top_row: bool,
    /// `last-in-row` (só com as abas quebradas em linhas).
    pub last_in_row: bool,
    /// `style.clipPath` das abas que passam da borda das abas conectadas.
    pub clip_path: Option<TabClipPath>,
    /// As classes `connected-tab-*` que `updateConnectedTabClipping` liga na aba ativa.
    pub connected_clipping: ConnectedTabClasses,
}

/// `style.clipPath` de uma aba: `inset(-stroke <fim> -stroke <início>)`, onde o lado do fim recebe `clipped` pixels e o outro
/// recebe o ombro (`--modern-ui-connected-tab-shoulder-radius` mais a espessura do traço), invertidos com o texto da direita
/// para a esquerda.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabClipPath {
    pub clipped: f64,
    pub right_to_left: bool,
}

/// As classes que `updateConnectedTabClipping` alterna na aba conectada.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConnectedTabClasses {
    pub left_edge: bool,
    pub right_edge: bool,
    pub left_clipped: bool,
    pub right_clipped: bool,
    pub hidden: bool,
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
#[derive(Clone, Debug, PartialEq)]
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

/// A primeira metade de `doLayoutTabsNonWrapping`: o espaço das abas fixadas e o que sobra para as demais.
#[derive(Clone, Debug, PartialEq)]
pub struct StickyTabsLayout {
    /// `style.left` de cada aba fixada.
    pub sticky_lefts: Vec<f64>,
    /// `stickyTabsWidth` (zero quando o posicionamento fixo é desligado).
    pub sticky_tabs_width: f64,
    /// A classe `disable-sticky-tabs`.
    pub disable_sticky_tabs: bool,
    /// `availableTabsContainerWidth`.
    pub available_tabs_container_width: f64,
    /// `activeTabPositionStatic`.
    pub active_tab_position_static: bool,
    /// `visibleEditorTabsWidth`.
    pub visible_editor_tabs_width: f64,
}

/// Posiciona as abas fixadas (`style.left`), soma a largura delas e decide se o posicionamento fixo precisa ser desligado
/// por falta de espaço.
pub fn layout_sticky_tabs(
    metrics: &TabsMetrics,
    sticky_count: usize,
    pinned_tab_sizing: PinnedTabSizing,
    has_modern_ui_tabs: bool,
    connected_tab_labels: bool,
    active_tab_index: Option<usize>,
    active_tab_sticky: bool,
) -> StickyTabsLayout {
    let visible_editor_tabs_width = metrics.visible_tabs_width - metrics.connected_add_tab_width;

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

    StickyTabsLayout { sticky_lefts, sticky_tabs_width, disable_sticky_tabs, available_tabs_container_width, active_tab_position_static, visible_editor_tabs_width }
}

/// O que as abas conectadas mudam na conta que revela a aba ativa (`connectedTabBounds`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConnectedReveal {
    pub fill_left: f64,
    pub fill_right: f64,
    pub shoulder_extent: f64,
    /// `previousElementSibling` existe e não é a última da linha: o ombro esquerdo conta.
    pub has_previous_tab_in_row: bool,
    /// A aba ativa tem a classe `connected-tab-last`.
    pub tab_connected_last: bool,
}

/// A segunda metade de `doLayoutTabsNonWrapping`: a rolagem que revela a aba ativa, ou `None` se a revelação é pulada ou
/// desnecessária. `active` é `(offsetLeft, offsetWidth)` da aba ativa (já `None` com `blockRevealActiveTab`).
#[allow(clippy::too_many_arguments)]
pub fn reveal_active_tab_scroll(
    options: Option<LayoutOptions>,
    active: Option<(f64, f64)>,
    sticky_tabs_width: f64,
    available_tabs_container_width: f64,
    active_tab_position_static: bool,
    block_reveal_active_tab: bool,
    dimensions_changed: bool,
    scroll_left: f64,
    connected: Option<ConnectedReveal>,
) -> Option<f64> {
    // Revealing the active tab is skipped under some conditions
    let (active_tab_pos_x, active_tab_width) = active?;
    if block_reveal_active_tab || active_tab_position_static || (!dimensions_changed && !options.is_some_and(|options| options.force_reveal_active_tab)) {
        return None;
    }

    // Reveal the active one
    let adjusted_active_tab_pos_x = match connected {
        // Round outwards so fractional bounds cannot leave either shoulder clipped.
        Some(c) => (c.fill_left - if c.has_previous_tab_in_row { c.shoulder_extent } else { 0.0 }).floor(),
        None => active_tab_pos_x,
    } - sticky_tabs_width;
    let active_tab_right = match connected {
        Some(c) => (c.fill_right + c.shoulder_extent).ceil() + if c.tab_connected_last { 1.0 } else { 0.0 } - sticky_tabs_width,
        None => adjusted_active_tab_pos_x + active_tab_width,
    };
    let active_tab_fits = active_tab_right - adjusted_active_tab_pos_x <= available_tabs_container_width;

    // Tab is overflowing to the right: Scroll minimally until the element is fully visible to the right. Note: only try to do
    // this if we actually have enough width to give to show the tab fully!
    if active_tab_fits && scroll_left + available_tabs_container_width < active_tab_right {
        Some(scroll_left + (active_tab_right - (scroll_left + available_tabs_container_width)))
    }
    // Tab is overflowing to the left or does not fit: Scroll it into view to the left
    else if scroll_left > adjusted_active_tab_pos_x || !active_tab_fits {
        Some(adjusted_active_tab_pos_x)
    } else {
        None
    }
}

/// `doLayoutTabsNonWrapping` sem o recorte das abas conectadas: posiciona as abas fixadas, mede o espaço e decide a rolagem
/// que revela a aba ativa. Com as abas conectadas, [`MultiEditorTabsControlState::do_layout_tabs_non_wrapping`] compõe as
/// mesmas duas metades com o recorte no meio.
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
    let sticky = layout_sticky_tabs(metrics, sticky_count, pinned_tab_sizing, has_modern_ui_tabs, connected_tab_labels, active_tab_index, active_tab_sticky);
    let active = if block_reveal_active_tab { None } else { metrics.active_tab };
    let (old_visible_tabs_width, old_all_tabs_width) = old_scroll;
    let dimensions_changed = old_visible_tabs_width != visible_tabs_width || old_all_tabs_width != all_tabs_width;

    NonWrappingLayout {
        reveal_scroll_left: reveal_active_tab_scroll(
            options,
            active,
            sticky.sticky_tabs_width,
            sticky.available_tabs_container_width,
            sticky.active_tab_position_static,
            block_reveal_active_tab,
            dimensions_changed,
            scroll_left,
            None,
        ),
        sticky_lefts: sticky.sticky_lefts,
        sticky_tabs_width: sticky.sticky_tabs_width,
        disable_sticky_tabs: sticky.disable_sticky_tabs,
        scroll_width: all_tabs_width,
        scroll_visible_width: visible_tabs_width,
        block_reveal_active_tab: false,
    }
}

/// Os três primeiros testes do `MOUSE_WHEEL`: há editores suficientes, o Shift está como a configuração pede e o último evento
/// não foi recente demais. Só com `true` o tempo do último evento é atualizado.
#[allow(clippy::too_many_arguments)]
pub fn mouse_wheel_gate_passes(
    editor_count: usize,
    has_active_editor: bool,
    scroll_to_switch_tabs: bool,
    shift_key: bool,
    now_ms: f64,
    last_event_ms: f64,
    delta_x: f64,
    delta_y: f64,
) -> bool {
    if !has_active_editor || editor_count < 2 {
        return false; // need at least 2 open editors
    }

    // Shift-key enables or disables this behaviour depending on the setting
    if scroll_to_switch_tabs {
        if shift_key {
            return false; // 'on': only enable this when Shift-key is not pressed
        }
    } else if !shift_key {
        return false; // 'off': only enable this when Shift-key is pressed
    }

    // Ignore event if the last one happened too recently (https://github.com/microsoft/vscode/issues/96409). The restriction is
    // relaxed according to the absolute value of `deltaX` and `deltaY` to support discrete (mouse wheel) and contiguous
    // scrolling (touchpad) equally well
    now_ms - last_event_ms >= MOUSE_WHEEL_EVENT_THRESHOLD - 2.0 * (delta_x.abs() + delta_y.abs())
}

/// A direção da rolagem (`-1` ou `1`), ou `None` se for sutil demais.
pub fn mouse_wheel_direction(delta_x: f64, delta_y: f64) -> Option<i64> {
    // Figure out scrolling direction but ignore it if too subtle
    if delta_x + delta_y < -MOUSE_WHEEL_DISTANCE_THRESHOLD {
        Some(-1)
    } else if delta_x + delta_y > MOUSE_WHEEL_DISTANCE_THRESHOLD {
        Some(1)
    } else {
        None
    }
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
    if !mouse_wheel_gate_passes(editor_count, has_active_editor, scroll_to_switch_tabs, shift_key, now_ms, last_event_ms, delta_x, delta_y) {
        return None;
    }

    mouse_wheel_direction(delta_x, delta_y)
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
    /// O modelo de abas é o grupo inteiro, só as fixadas ou só as não fixadas (`StickyEditorGroupModel` e
    /// `UnstickyEditorGroupModel`).
    pub model_kind: TabsModelKind,
    /// `lastSingleSelectSelectedEditor` (a âncora da seleção com Shift).
    pub last_single_select_selected_editor: Option<EditorInputRef>,
    /// `dropTarget`.
    pub drop_target: Option<DropTarget>,
    /// O que o original escreve nos contêineres (classes e variáveis CSS).
    pub container: TabsContainerView,
    /// `connectedTabBounds`.
    pub connected_tab_bounds: Option<ConnectedTabBounds>,
    /// `isMacintosh` e `isLinux`.
    pub is_macintosh: bool,
    pub is_linux: bool,
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
            model_kind: TabsModelKind::Group,
            last_single_select_selected_editor: None,
            drop_target: None,
            container: TabsContainerView::default(),
            connected_tab_bounds: None,
            is_macintosh: Platform::detect().is_macintosh,
            is_linux: Platform::detect().is_linux,
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

/// `tabsModel`: o grupo inteiro, só as abas fixadas (`StickyEditorGroupModel`) ou só as não fixadas
/// (`UnstickyEditorGroupModel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsModelKind {
    Group,
    Sticky,
    Unsticky,
}

/// `preventEditorClose` (de `common/editor.ts`): só editores fixados podem ter o fechamento barrado.
pub fn prevent_editor_close(group: &dyn IReadonlyEditorGroupModel, editor: &EditorInputRef, method: EditorCloseMethod, options: &EditorPartOptions) -> bool {
    if !group.is_sticky(&EditorOrIndex::Editor(editor.clone())) {
        return false; // only interested in sticky editors
    }

    match options.prevent_pinned_editor_close {
        PreventPinnedEditorClose::KeyboardAndMouse => method == EditorCloseMethod::Mouse || method == EditorCloseMethod::Keyboard,
        PreventPinnedEditorClose::Mouse => method == EditorCloseMethod::Mouse,
        PreventPinnedEditorClose::Keyboard => method == EditorCloseMethod::Keyboard,
        PreventPinnedEditorClose::Never => false,
    }
}

/// `a.matches(b)` entre dois editores.
fn editors_match(a: &EditorInputRef, b: &EditorInputRef) -> bool {
    a.matches(&EditorInputOrUntyped::Typed(b.clone()))
}

/// O `editors[index]` de JavaScript: índice negativo ou fora do intervalo é `undefined`.
fn editor_at(editors: &[EditorInputRef], index: i64) -> Option<EditorInputRef> {
    usize::try_from(index).ok().and_then(|index| editors.get(index)).cloned()
}

/// Os modificadores de um evento de mouse, toque ou teclado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputModifiers {
    pub shift_key: bool,
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub alt_key: bool,
}

impl InputModifiers {
    /// `(e.ctrlKey && !isMacintosh) || (e.metaKey && isMacintosh)`.
    pub fn is_ctrl_cmd(self, is_macintosh: bool) -> bool {
        (self.ctrl_key && !is_macintosh) || (self.meta_key && is_macintosh)
    }

    /// `StandardKeyboardEvent.equals(keyCode)` compara a tecla sem modificadores: nenhum pode estar pressionado.
    pub fn is_none(self) -> bool {
        !self.shift_key && !self.ctrl_key && !self.meta_key && !self.alt_key
    }
}

/// Um clique, toque ou soltar do mouse sobre uma aba.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabPointerEvent {
    /// `e.button`; `None` é um toque (`GestureEvent`).
    pub button: Option<u8>,
    pub modifiers: InputModifiers,
    /// `originatesFromTabActionBar(e)`.
    pub from_action_bar: bool,
}

impl TabPointerEvent {
    /// O teste que os listeners repetem: botão do meio, direito ou, no macOS, Ctrl com clique (menu de contexto).
    fn is_secondary_button(&self, is_macintosh: bool) -> bool {
        self.button.is_some_and(|button| button != 0 || (is_macintosh && self.modifiers.ctrl_key))
    }
}

/// `originatesFromTabActionBar`: `findParentWithClass(element, 'action-item', 'tab')`, com `target_to_root` sendo as classes
/// de cada elemento do alvo do evento até a raiz.
pub fn originates_from_tab_action_bar(target_to_root: &[&[&str]]) -> bool {
    for classes in target_to_root {
        if classes.contains(&"action-item") {
            return true;
        }

        if classes.contains(&"tab") {
            return false; // stop at the tab itself
        }
    }

    false
}

/// As teclas que os listeners das abas distinguem (`KeyCode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabKey {
    Enter,
    Space,
    LeftArrow,
    RightArrow,
    UpArrow,
    DownArrow,
    Home,
    End,
    F10,
    Other,
}

/// Duplo clique (`DBLCLICK`) ou toque (`TouchEventType.Tap`, que só conta com `tapCount` igual a 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabTap {
    DoubleClick,
    Tap { tap_count: u32 },
}

/// `IEditorOptions` do `openEditor` do grupo que os listeners das abas pedem.
#[derive(Clone, Default)]
pub struct TabOpenRequest {
    pub preserve_focus: bool,
    /// `activation: EditorActivation.ACTIVATE`.
    pub activate: bool,
    /// `inactiveSelection`.
    pub inactive_selection: Option<Vec<EditorInputRef>>,
    /// `focusTabControl`.
    pub focus_tab_control: bool,
}

/// O âncora do menu de contexto da barra de abas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContextMenuAnchor {
    /// O ponto do mouse (`StandardMouseEvent`).
    Point { x: f64, y: f64 },
    /// O contêiner de abas (o gesto de menu de contexto por toque ou o teclado).
    TabsContainer,
}

/// `DraggedEditorIdentifier`: um editor arrastado e o grupo de onde ele vem.
#[derive(Clone)]
pub struct DraggedEditor {
    pub group_id: GroupIdentifier,
    pub editor: EditorInputRef,
}

/// O que as transferências locais (`LocalSelectionTransfer`) e o `DataTransfer` carregam ao soltar ou arrastar sobre as abas.
#[derive(Clone, Default)]
pub struct DropTransfer {
    /// `groupTransfer`: `Some` é `hasData`, com os `DraggedEditorGroupIdentifier.identifier`.
    pub group: Option<Vec<GroupIdentifier>>,
    /// `editorTransfer`: `Some` é `hasData`.
    pub editors: Option<Vec<DraggedEditor>>,
    /// `treeItemsTransfer`: `Some` é `hasData`, com a quantidade de itens.
    pub tree_items: Option<usize>,
    /// `e.dataTransfer.types.length`.
    pub external_types: usize,
}

/// `dropEffect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropEffect {
    None,
    Copy,
}

/// O que `onDragEnter` decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragEnterOutcome {
    /// O `dropEffect` a escrever no evento, se ele muda.
    pub drop_effect: Option<DropEffect>,
    /// O feedback de soltar foi atualizado.
    pub feedback_updated: bool,
}

/// O lado da aba sobre o qual o arrasto passa (`getTabDragOverLocation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragLocation {
    Left,
    Right,
}

/// `getTabDragOverLocation`: `rect.left`, `rect.width` e `e.clientX`.
pub fn tab_drag_over_location(client_x: f64, rect_left: f64, rect_width: f64) -> DragLocation {
    if client_x - rect_left <= rect_width / 2.0 { DragLocation::Left } else { DragLocation::Right }
}

/// `dropTarget`: as abas (por índice) que recebem `drop-target-left` e `drop-target-right`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropTarget {
    pub left_index: Option<usize>,
    pub right_index: Option<usize>,
}

/// `computeDropTarget`.
pub fn compute_drop_target(tab_index: usize, tab_count: usize, location: DragLocation) -> DropTarget {
    let is_left_side_of_tab = location == DragLocation::Left;
    let is_last_tab = tab_index + 1 == tab_count;
    let is_first_tab = tab_index == 0;

    // Before first tab
    if is_left_side_of_tab && is_first_tab {
        return DropTarget { left_index: None, right_index: Some(tab_index) };
    }

    // After last tab
    if !is_left_side_of_tab && is_last_tab {
        return DropTarget { left_index: Some(tab_index), right_index: None };
    }

    // Between two tabs
    let tab_before = if is_left_side_of_tab { tab_index.checked_sub(1) } else { Some(tab_index) };
    let tab_after = if is_left_side_of_tab { Some(tab_index) } else { Some(tab_index + 1) };

    DropTarget { left_index: tab_before, right_index: tab_after }
}

/// A imagem do arrastar de uma aba.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabDragImage {
    /// `applyDragImage(e, tab, label)`: uma imagem de texto limpo.
    Label(String),
    /// `setDragImage(tab, 0, 0)`: a própria aba, com o canto superior esquerdo no cursor.
    TabElement,
}

/// O que `onDragStart` de uma aba decide.
#[derive(Clone)]
pub struct TabDragStart {
    /// `isNewWindowOperation`.
    pub is_new_window_operation: bool,
    /// Os editores selecionados, que viram os `DraggedEditorIdentifier` da transferência local e os dados de recurso.
    pub selected_editors: Vec<DraggedEditor>,
    pub drag_image: TabDragImage,
}

/// O `groupView` e o `editorPartsView` que os listeners das abas acionam: tudo que muda o grupo, abre o menu ou move
/// editores. As consultas ao grupo ficam em [`TabsControlHost`].
pub trait TabsGroupOps {
    /// `groupView.openEditor(editor, options, internalOptions)`.
    fn open_editor(&self, editor: &EditorInputRef, request: TabOpenRequest);
    /// `groupView.setSelection(activeSelectedEditor, inactiveSelectedEditors)`.
    fn set_selection(&self, active_selected_editor: &EditorInputRef, inactive_selected_editors: Vec<EditorInputRef>);
    /// `groupView.pinEditor(editor)`.
    fn pin_editor(&self, editor: &EditorInputRef);
    /// `groupsView.toggleMaximizeGroup(groupView)`.
    fn toggle_maximize_group(&self);
    /// `groupsView.toggleExpandGroup(groupView)`.
    fn toggle_expand_group(&self);
    /// `groupView.focus()`.
    fn focus_group(&self);
    /// `groupView.getEditors(EditorsOrder.MOST_RECENTLY_ACTIVE)`.
    fn group_editors_most_recently_active(&self) -> Vec<EditorInputRef>;
    /// `groupView.stickyCount`.
    fn group_sticky_count(&self) -> usize;
    /// `groupView.isSticky(editor)`.
    fn group_is_sticky(&self, editor: &EditorInputRef) -> bool;
    /// `groupView.unstickEditor(editor)`.
    fn unstick_editor(&self, editor: &EditorInputRef);
    /// `closeEditorAction.run({ groupId, editorIndex })`.
    fn run_close_editor_action(&self, editor_index: i64);
    /// `editorService.openEditor({ resource: undefined, options: { pinned: true, index: groupView.count, override: DEFAULT_EDITOR_ASSOCIATION.id } }, groupView.id)`.
    fn open_new_editor_at_end(&self);
    /// `onTabContextMenu(editor, e, tab)`.
    fn show_tab_context_menu(&self, editor: &EditorInputRef);
    /// `contextMenuService.showContextMenu` com o menu `EditorTabsBarContext`.
    fn show_tabs_bar_context_menu(&self, anchor: ContextMenuAnchor);
    /// `editorPartsView.getGroup(id)` existe.
    fn has_group(&self, group_id: GroupIdentifier) -> bool;
    /// `sourceGroup.getIndexOfEditor(editor)`.
    fn group_index_of_editor(&self, group_id: GroupIdentifier, editor: &EditorInputRef) -> i64;
    /// `sourceGroup.moveEditor(editor, groupView, options)`.
    fn move_editor_from(&self, source_group_id: GroupIdentifier, editor: &EditorInputRef, options: DropEditorOptions);
    /// `sourceGroup.copyEditor(editor, groupView, options)`.
    fn copy_editor_from(&self, source_group_id: GroupIdentifier, editor: &EditorInputRef, options: DropEditorOptions);
    /// `groupsView.mergeGroup(sourceGroup, groupView, options)`.
    fn merge_group(&self, source_group_id: GroupIdentifier, options: MergeGroupOptions);
    /// `treeViewsDragAndDropService` e `editorService.openEditors(editors, groupView, { validateTrust: true })` com cada
    /// editor `{ pinned: true, index }`.
    fn open_tree_items_drop(&self, options: DropEditorOptions);
    /// `ResourcesDropHandler.handleDrop(e, window, () => groupView, () => groupView.focus(), options)`.
    fn handle_resources_drop(&self, options: DropEditorOptions);
    /// `groupTransfer.clearData(DraggedEditorGroupIdentifier.prototype)`.
    fn clear_group_transfer(&self);
    /// `editorTransfer.clearData(DraggedEditorIdentifier.prototype)`.
    fn clear_editor_transfer(&self);
    /// `treeItemsTransfer.clearData(DraggedTreeItemsIdentifier.prototype)`.
    fn clear_tree_items_transfer(&self);
    /// O fim de `onDragEnd` com a operação de janela nova: `editorTransfer.getData` seguido de `clearData`.
    fn take_dragged_editors(&self) -> Vec<DraggedEditor>;
    /// `maybeCreateAuxiliaryEditorPartAt(e, tab)`: o id do grupo ativo da janela auxiliar, ou `None` se foi recusada.
    fn create_auxiliary_part_for_drag(&self) -> Option<GroupIdentifier>;
    /// `groupView.moveEditors(prepareMoveCopyEditors(groupView, editors), targetGroup)`.
    fn move_editors_to(&self, editors: &[EditorInputRef], target_group_id: GroupIdentifier);
    /// `groupView.copyEditors(prepareMoveCopyEditors(groupView, editors), targetGroup)`.
    fn copy_editors_to(&self, editors: &[EditorInputRef], target_group_id: GroupIdentifier);
    /// `targetGroup.focus()`.
    fn focus_other_group(&self, group_id: GroupIdentifier);
}

/// `IEditorOptions` do `onDrop` (`sticky` e `index`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropEditorOptions {
    pub sticky: bool,
    pub index: i64,
}

/// As rolagens do mouse (`WheelEvent`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelInput {
    pub shift_key: bool,
    pub delta_x: f64,
    pub delta_y: f64,
}

impl MultiEditorTabsControlState {
    /// `groupView.isSelected(editor)`.
    fn group_is_selected(&self, editor: &EditorInputRef) -> bool {
        self.host().selected_editors().iter().any(|selected| id_of(selected) == id_of(editor))
    }

    /// `assertReturnsDefined(groupView.activeEditor)`.
    fn assert_active_editor(&self) -> Result<EditorInputRef, Error> {
        self.host().active_editor().ok_or_else(|| Error::new("Assertion Failed: argument is undefined or null".to_string()))
    }

    /// `blockRevealActiveTabOnce`: ao fechar abas em sequência, revelar a ativa a cada fechamento atrapalharia.
    pub fn block_reveal_active_tab_once(&mut self) {
        self.block_reveal_active_tab = true;
    }

    /// `setAltPressed`.
    pub fn set_alt_pressed(&mut self, is_alt_pressed: bool) {
        if let Some(index) = self.hover_alt.set_alt_pressed(is_alt_pressed) {
            self.redraw_tab_action_at_index(index);
        }
    }

    /// `hostService.onDidChangeFocus`: o Alt pode ficar preso quando o foco sai da janela com ele segurado (Alt+Tab para outro
    /// aplicativo), porque o `keyup` nunca chega aqui. Desiste da troca por completo, para que uma aba ainda sob o mouse não
    /// mantenha a ação "Close Others" (https://github.com/microsoft/vscode/issues/331979).
    pub fn handle_window_focus_change(&mut self, has_focus: bool) {
        if !has_focus {
            self.set_alt_pressed(false);
            self.set_hovered_tab(None);
        }
    }

    /// `setHoveredTab`.
    pub fn set_hovered_tab(&mut self, tab_index: Option<usize>) {
        if self.hover_alt.hovered_tab_index == tab_index {
            return;
        }

        for index in self.hover_alt.set_hovered_tab(tab_index) {
            self.redraw_tab_action_at_index(index);
        }
        self.update_connected_tab_overflow_hover();
    }

    /// `updateConnectedTabOverflowHover`.
    pub fn update_connected_tab_overflow_hover(&mut self) {
        let host = self.host();
        let hovered_active = self
            .hover_alt
            .hovered_tab_index
            .and_then(|index| self.base.tabs_model.get_editor_by_index(index as i64))
            .is_some_and(|editor| host.is_active(&editor));
        self.container.overflow_edge.hovered = hovered_active;
    }

    /// O `MOUSE_DOWN` e o `MOUSE_MOVE` do contêiner de abas, em fase de captura: confere o Alt, que o evento sempre carrega.
    pub fn handle_tabs_container_mouse_event(&mut self, alt_key: bool) {
        self.set_alt_pressed(alt_key);
    }

    /// O `MOUSE_DOWN` do contêiner de abas: o botão do meio não pode iniciar a rolagem automática
    /// (https://github.com/microsoft/vscode/issues/16690). Devolve se o evento deve ter o padrão impedido.
    pub fn tabs_container_mouse_down_prevents_default(&self, button: u8) -> bool {
        button == 1
    }

    /// O `MOUSE_UP` do contêiner de abas no Linux: o botão do meio não pode colar (https://github.com/microsoft/vscode/issues/201696).
    pub fn tabs_container_mouse_up_prevents_default(&self, button: u8) -> bool {
        self.is_linux && button == 1
    }

    /// O `MOUSE_LEAVE` do contêiner de abas: limpa a aba sob o mouse e, com o tamanho `fixed`, solta as larguras fixadas.
    pub fn handle_tabs_container_mouse_leave(&mut self) {
        self.set_hovered_tab(None);

        if self.options().tab_sizing == TabSizing::Fixed {
            self.is_mouse_over_tabs = false;
            self.update_tabs_fixed_width(false, &[]);
        }
    }

    /// O `MOUSE_ENTER` do contêiner de abas com o tamanho `fixed` (`updateTabSizing`).
    pub fn handle_tabs_container_mouse_enter(&mut self) {
        if self.options().tab_sizing == TabSizing::Fixed {
            self.is_mouse_over_tabs = true;
        }
    }

    /// O `SCROLL` do contêiner de abas: durante o arrastar o contêiner rola, então a barra customizada acompanha.
    pub fn handle_tabs_container_scroll(&mut self, scroll_left: f64, measurer: &dyn TabsMeasurer) {
        if self.container.scroll_mode {
            self.container.scroll_left = scroll_left;
            self.update_connected_tab_clipping(scroll_left, measurer);
        }
    }

    /// O duplo clique ou toque no contêiner de abas (não nas abas) abre um arquivo novo no fim. `target_is_container` é
    /// `e.target === tabsContainer` (ou `initialTarget`, no toque). Devolve se o evento foi interrompido.
    pub fn handle_tabs_container_double_click(&self, tap: TabTap, target_is_container: bool, ops: &dyn TabsGroupOps) -> bool {
        if let TabTap::Tap { tap_count } = tap
            && tap_count != 2
        {
            return false; // ignore single taps
        }

        if !target_is_container {
            return false; // ignore if target is not tabs container
        }

        ops.open_new_editor_at_end();

        true
    }

    /// O `MOUSE_WHEEL` do contêiner de abas: troca de aba se a configuração e o Shift deixam. Devolve se o evento foi
    /// interrompido (a rolagem normal é desligada, o `openEditor` já revela a aba).
    pub fn handle_mouse_wheel(&mut self, wheel: WheelInput, now_ms: f64, ops: &dyn TabsGroupOps) -> bool {
        let host = self.host();
        let Some(active_editor) = host.active_editor() else { return false };
        if !mouse_wheel_gate_passes(
            host.group_count(),
            true,
            self.options().scroll_to_switch_tabs,
            wheel.shift_key,
            now_ms,
            self.last_mouse_wheel_event_time,
            wheel.delta_x,
            wheel.delta_y,
        ) {
            return false;
        }

        self.last_mouse_wheel_event_time = now_ms;

        let Some(direction) = mouse_wheel_direction(wheel.delta_x, wheel.delta_y) else { return false };
        let Some(next_editor) = editor_at(&host.group_editors(), host.index_of_editor(&active_editor) + direction) else { return false };

        // Open it
        ops.open_editor(&next_editor, TabOpenRequest::default());

        true
    }

    /// O menu de contexto do contêiner de abas (`showContextMenu`): `point` é a posição do mouse, ausente no gesto de toque.
    pub fn show_tabs_bar_context_menu(&self, point: Option<(f64, f64)>, ops: &dyn TabsGroupOps) {
        ops.show_tabs_bar_context_menu(match point {
            Some((x, y)) => ContextMenuAnchor::Point { x, y },
            None => ContextMenuAnchor::TabsContainer,
        });
    }

    /// `handleClickOrTouch`. Devolve se o padrão do evento deve ser impedido (o botão do meio, para não rolar sozinho).
    pub fn handle_click_or_touch(&mut self, tab_index: usize, e: &TabPointerEvent, preserve_focus: bool, ops: &dyn TabsGroupOps) -> Result<bool, Error> {
        if e.is_secondary_button(self.is_macintosh) {
            return Ok(e.button == Some(1)); // required to prevent auto-scrolling (https://github.com/microsoft/vscode/issues/16690)
        }

        if e.from_action_bar {
            return Ok(false); // not when clicking on actions
        }

        // Open tabs editor
        let Some(editor) = self.base.tabs_model.get_editor_by_index(tab_index as i64) else { return Ok(false) };
        if e.modifiers.shift_key {
            let last_selected = self
                .last_single_select_selected_editor
                .clone()
                .filter(|last| self.base.tabs_model.is_selected(&EditorOrIndex::Editor(last.clone())));
            let anchor = match last_selected {
                // The last selected editor is the anchor
                Some(last) => last,
                // The active editor is the anchor
                None => {
                    let active_editor = self.assert_active_editor()?;
                    self.last_single_select_selected_editor = Some(active_editor.clone());
                    active_editor
                }
            };
            self.select_editors_between(&editor, &anchor, ops)?;
        } else if e.modifiers.is_ctrl_cmd(self.is_macintosh) {
            if self.base.tabs_model.is_selected(&EditorOrIndex::Editor(editor.clone())) {
                self.unselect_editor(&editor, ops)?;
            } else {
                self.select_editor(&editor, ops);
                self.last_single_select_selected_editor = Some(editor);
            }
        } else {
            // Even if focus is preserved make sure to activate the group. If a new active editor is selected, keep the current
            // selection on key down such that drag and drop can operate over the selection. The selection is removed on key
            // up in this case.
            let inactive_selection = if self.base.tabs_model.is_selected(&EditorOrIndex::Editor(editor.clone())) {
                self.host().selected_editors().into_iter().filter(|selected| !editors_match(selected, &editor)).collect()
            } else {
                Vec::new()
            };
            ops.open_editor(&editor, TabOpenRequest { preserve_focus, activate: true, inactive_selection: Some(inactive_selection), focus_tab_control: true });
        }

        Ok(false)
    }

    /// O `MOUSE_UP` de uma aba: soltar sem Ctrl ou Shift desfaz a seleção múltipla.
    pub fn handle_tab_mouse_up(&mut self, e: &TabPointerEvent, ops: &dyn TabsGroupOps) -> Result<(), Error> {
        if e.is_secondary_button(self.is_macintosh) {
            return Ok(());
        }

        if e.from_action_bar {
            return Ok(()); // not when clicking on actions
        }

        if !e.modifiers.is_ctrl_cmd(self.is_macintosh) && !e.modifiers.shift_key && self.host().selected_editors().len() > 1 {
            self.unselect_all_editors(ops)?;
        }

        Ok(())
    }

    /// O `MOUSE_ENTER` de uma aba: congela a largura `fit` e marca a aba sob o mouse.
    pub fn handle_tab_mouse_enter(&mut self, tab_index: usize, measurer: &dyn TabsMeasurer) {
        self.freeze_fit_tab_width_without_reserved_actions(tab_index, measurer);
        self.set_hovered_tab(Some(tab_index));
    }

    /// O `MOUSE_LEAVE` de uma aba.
    pub fn handle_tab_mouse_leave(&mut self, tab_index: usize) {
        if self.hover_alt.hovered_tab_index == Some(tab_index) {
            self.set_hovered_tab(None);
        }
    }

    /// O `FOCUS` (em captura) de uma aba.
    pub fn handle_tab_focus(&mut self, tab_index: usize, measurer: &dyn TabsMeasurer) {
        self.freeze_fit_tab_width_without_reserved_actions(tab_index, measurer);
    }

    /// O `CHANGE` do toque numa aba: a rolagem acompanha o dedo. Devolve a nova posição da barra.
    pub fn handle_tab_touch_scroll(&mut self, translation_x: f64) -> f64 {
        self.container.scroll_left -= translation_x;
        self.container.scroll_left
    }

    /// O `AUXCLICK` de uma aba: o botão do meio fecha. Devolve se o evento foi interrompido.
    pub fn handle_tab_aux_click(&mut self, tab_index: usize, button: u8, ops: &dyn TabsGroupOps) -> bool {
        if button != 1 {
            return false;
        }

        if let Some(editor) = self.base.tabs_model.get_editor_by_index(tab_index as i64) {
            if editor.has_capability(EditorInputCapabilities::CANNOT_CLOSE)
                || prevent_editor_close(self.base.tabs_model.as_ref(), &editor, EditorCloseMethod::Mouse, &self.options())
            {
                return true;
            }

            self.block_reveal_active_tab_once();
            ops.run_close_editor_action(self.host().index_of_editor(&editor));
        }

        true
    }

    /// O `KEY_DOWN` de uma aba: Shift+F10 abre o menu de contexto. Devolve se o evento foi interrompido.
    pub fn handle_tab_key_down(&self, tab_index: usize, key: TabKey, modifiers: InputModifiers, ops: &dyn TabsGroupOps) -> bool {
        if modifiers.shift_key && key == TabKey::F10 {
            return self.show_tab_context_menu(tab_index, ops);
        }

        false
    }

    /// `showContextMenu` e o `CONTEXT_MENU` de uma aba. Devolve se o evento foi interrompido (sempre).
    pub fn show_tab_context_menu(&self, tab_index: usize, ops: &dyn TabsGroupOps) -> bool {
        if let Some(editor) = self.base.tabs_model.get_editor_by_index(tab_index as i64) {
            ops.show_tab_context_menu(&editor);
        }

        true
    }

    /// O `KEY_UP` de uma aba: Enter e Espaço abrem, as setas, Home e End navegam. Devolve se o evento foi tratado (e
    /// interrompido). Depois dele a barra de rolagem customizada acompanha o `scrollLeft` do contêiner.
    pub fn handle_tab_key_up(&self, tab_index: usize, key: TabKey, modifiers: InputModifiers, ops: &dyn TabsGroupOps) -> bool {
        let mut handled = false;

        // Run action on Enter/Space
        if modifiers.is_none() && matches!(key, TabKey::Enter | TabKey::Space) {
            handled = true;
            if let Some(editor) = self.base.tabs_model.get_editor_by_index(tab_index as i64) {
                ops.open_editor(&editor, TabOpenRequest::default());
            }
        }
        // Navigate in editors
        else if modifiers.is_none() && matches!(key, TabKey::LeftArrow | TabKey::RightArrow | TabKey::UpArrow | TabKey::DownArrow | TabKey::Home | TabKey::End) {
            let host = self.host();
            let editor_index = match key {
                TabKey::LeftArrow | TabKey::UpArrow => self.to_editor_index(tab_index) - 1,
                TabKey::RightArrow | TabKey::DownArrow => self.to_editor_index(tab_index) + 1,
                TabKey::Home => 0,
                _ => host.group_count() as i64 - 1,
            };

            if let Some(target) = editor_at(&host.group_editors(), editor_index) {
                handled = true;
                ops.open_editor(&target, TabOpenRequest { preserve_focus: true, focus_tab_control: true, ..TabOpenRequest::default() });
            }
        }

        handled
    }

    /// O duplo clique ou o toque duplo numa aba: fixa a aba ou, se já fixada, alterna o tamanho do grupo. Devolve se o
    /// evento foi interrompido (só o `DBLCLICK`).
    pub fn handle_tab_double_click(&self, tab_index: usize, tap: TabTap, ops: &dyn TabsGroupOps) -> bool {
        if let TabTap::Tap { tap_count } = tap
            && tap_count != 2
        {
            return false; // ignore single taps
        }

        match self.base.tabs_model.get_editor_by_index(tab_index as i64) {
            Some(editor) if self.base.tabs_model.is_pinned(&EditorOrIndex::Editor(editor.clone())) => match self.options().double_click_tab_to_toggle_editor_group_sizes {
                DoubleClickTabAction::Maximize => ops.toggle_maximize_group(),
                DoubleClickTabAction::Expand => ops.toggle_expand_group(),
                DoubleClickTabAction::Off => {}
            },
            Some(editor) => ops.pin_editor(&editor),
            None => {}
        }

        tap == TabTap::DoubleClick
    }

    /// `selectEditor`.
    pub fn select_editor(&self, editor: &EditorInputRef, ops: &dyn TabsGroupOps) {
        if self.host().is_active(editor) {
            return;
        }

        ops.set_selection(editor, self.host().selected_editors());
    }

    /// `selectEditorsBetween`.
    pub fn select_editors_between(&self, target: &EditorInputRef, anchor: &EditorInputRef, ops: &dyn TabsGroupOps) -> Result<(), Error> {
        let host = self.host();
        let editor_index = host.index_of_editor(target);
        if editor_index == -1 {
            return Err(Error::bug_indicating(None));
        }

        let anchor_editor_index = host.index_of_editor(anchor);
        if anchor_editor_index == -1 {
            return Err(Error::bug_indicating(None));
        }

        let editors = host.group_editors();
        let mut selection = host.selected_editors();

        // Unselect editors on other side of anchor in relation to the target
        let mut current_editor_index = anchor_editor_index;
        while current_editor_index >= 0 && current_editor_index <= host.group_count() as i64 - 1 {
            current_editor_index = if anchor_editor_index < editor_index { current_editor_index - 1 } else { current_editor_index + 1 };

            let Some(current_editor) = editor_at(&editors, current_editor_index) else { break };
            if !self.group_is_selected(&current_editor) {
                break;
            }

            selection.retain(|editor| !editors_match(editor, &current_editor));
        }

        // Select editors between anchor and target
        let from_editor_index = anchor_editor_index.min(editor_index);
        let to_editor_index = anchor_editor_index.max(editor_index);

        for editor in editors.iter().skip(from_editor_index as usize).take((to_editor_index - from_editor_index + 1) as usize) {
            if !self.group_is_selected(editor) {
                selection.push(editor.clone());
            }
        }

        let inactive_selected_editors = selection.into_iter().filter(|editor| !editors_match(editor, target)).collect();
        ops.set_selection(target, inactive_selected_editors);

        Ok(())
    }

    /// `unselectEditor`.
    pub fn unselect_editor(&self, editor: &EditorInputRef, ops: &dyn TabsGroupOps) -> Result<(), Error> {
        let host = self.host();
        let is_unselecting_active_editor = host.is_active(editor);

        // If there is only one editor selected, do not unselect it
        if is_unselecting_active_editor && host.selected_editors().len() == 1 {
            return Ok(());
        }

        let mut new_active_editor = self.assert_active_editor()?;

        // If active editor is bing unselected then find the most recently opened selected editor that is not the editor being
        // unselected
        if is_unselecting_active_editor {
            let recent_editors = ops.group_editors_most_recently_active();
            for recent_editor in recent_editors.iter().skip(1) {
                // First one is the active editor
                if self.group_is_selected(recent_editor) {
                    new_active_editor = recent_editor.clone();
                    break;
                }
            }
        }

        let inactive_selected_editors = host
            .selected_editors()
            .into_iter()
            .filter(|selected| !editors_match(selected, editor) && !editors_match(selected, &new_active_editor))
            .collect();
        ops.set_selection(&new_active_editor, inactive_selected_editors);

        Ok(())
    }

    /// `unselectAllEditors`.
    pub fn unselect_all_editors(&self, ops: &dyn TabsGroupOps) -> Result<(), Error> {
        if self.host().selected_editors().len() > 1 {
            let active_editor = self.assert_active_editor()?;
            ops.set_selection(&active_editor, Vec::new());
        }

        Ok(())
    }

    /// `isSupportedDropTransfer`.
    pub fn is_supported_drop_transfer(&self, transfer: &DropTransfer) -> bool {
        if let Some(data) = &transfer.group {
            if let Some(group_id) = data.first()
                && *group_id == self.host().group_id()
            {
                return false; // groups cannot be dropped on group it originates from
            }

            return true;
        }

        if transfer.editors.is_some() {
            return true; // (local) editors can always be dropped
        }

        // optimistically allow external data (// see https://github.com/microsoft/vscode/issues/25789)
        transfer.external_types > 0
    }

    /// `updateDropFeedback`: `tab` é a aba sob o arrasto e o lado dela; sem ela o alvo é depois da última aba. Sem `is_dnd`
    /// o feedback é limpo.
    pub fn update_drop_feedback(&mut self, is_dnd: bool, tab: Option<(usize, DragLocation)>) {
        let drop_target = if !is_dnd {
            None
        } else if let Some((tab_index, location)) = tab {
            Some(compute_drop_target(tab_index, self.base.tabs_model.count(), location))
        } else {
            Some(DropTarget { left_index: self.tabs.len().checked_sub(1), right_index: None })
        };

        self.update_drop_target(drop_target);
    }

    /// `updateDropTarget`.
    pub fn update_drop_target(&mut self, new_target: Option<DropTarget>) {
        if self.drop_target == new_target {
            return;
        }

        if let Some(old) = self.drop_target {
            if let Some(tab) = old.left_index.and_then(|index| self.tabs.get_mut(index)) {
                tab.state.drop_target_left = false;
            }
            if let Some(tab) = old.right_index.and_then(|index| self.tabs.get_mut(index)) {
                tab.state.drop_target_right = false;
            }
        }

        if let Some(new) = new_target {
            if let Some(tab) = new.left_index.and_then(|index| self.tabs.get_mut(index)) {
                tab.state.drop_target_left = true;
            }
            if let Some(tab) = new.right_index.and_then(|index| self.tabs.get_mut(index)) {
                tab.state.drop_target_right = true;
            }
        }

        self.drop_target = new_target;
    }

    /// O `onDragEnter` comum: sem suporte o efeito é `none`; sem dados locais só dá para copiar.
    fn drag_enter_effect(&self, transfer: &DropTransfer) -> Result<Option<DropEffect>, ()> {
        if !self.is_supported_drop_transfer(transfer) {
            return Err(());
        }

        Ok(if transfer.editors.is_none() { Some(DropEffect::Copy) } else { None })
    }

    /// O `onDragStart` de uma aba. Depois dele, no próximo quadro, o chamador limpa o feedback com
    /// `update_drop_feedback(false, None)`.
    pub fn on_tab_drag_start(&self, tab_index: usize, modifiers: DragModifiers) -> Option<TabDragStart> {
        let editor = self.base.tabs_model.get_editor_by_index(tab_index as i64)?;
        let is_new_window_operation = self.base.is_new_window_operation(modifiers);
        let host = self.host();
        let selected_editors = host.selected_editors();
        let group_id = host.group_id();

        let drag_image = if selected_editors.len() > 1 {
            TabDragImage::Label(format!("{} + {}", editor.get_name(), selected_editors.len() - 1))
        } else {
            let options = self.options();
            let is_tab_sticky = self.base.tabs_model.is_sticky(&EditorOrIndex::Index(tab_index as i64));
            let is_shrink_sizing = options.tab_sizing == TabSizing::Shrink || (is_tab_sticky && options.pinned_tab_sizing == PinnedTabSizing::Shrink);
            if is_shrink_sizing {
                // When tab sizing is 'shrink', the tab label may be truncated. Using the tab DOM element as a drag image can
                // cause parts of the tab header UI to visually drag along. Instead, use a clean text-only drag image with the
                // editor name.
                TabDragImage::Label(editor.get_name())
            } else {
                TabDragImage::TabElement // top left corner of dragged tab set to cursor position to make room for drop-border feedback
            }
        };

        Some(TabDragStart {
            is_new_window_operation,
            selected_editors: selected_editors.into_iter().map(|editor| DraggedEditor { group_id, editor }).collect(),
            drag_image,
        })
    }

    /// O `onDragEnter` de uma aba.
    pub fn on_tab_drag_enter(&mut self, tab_index: usize, location: DragLocation, transfer: &DropTransfer) -> DragEnterOutcome {
        match self.drag_enter_effect(transfer) {
            Err(()) => DragEnterOutcome { drop_effect: Some(DropEffect::None), feedback_updated: false },
            Ok(drop_effect) => {
                self.update_drop_feedback(true, Some((tab_index, location)));
                DragEnterOutcome { drop_effect, feedback_updated: true }
            }
        }
    }

    /// O `onDragOver` de uma aba: arrastar sobre ela por tempo demais a abre.
    pub fn on_tab_drag_over(&mut self, tab_index: usize, location: DragLocation, drag_duration_ms: u64, ops: &dyn TabsGroupOps) {
        if drag_duration_ms >= DRAG_OVER_OPEN_TAB_THRESHOLD
            && let Some(dragged_over_tab) = self.base.tabs_model.get_editor_by_index(tab_index as i64)
            && self.base.tabs_model.active_editor().is_none_or(|active| id_of(&active) != id_of(&dragged_over_tab))
        {
            ops.open_editor(&dragged_over_tab, TabOpenRequest { preserve_focus: true, ..TabOpenRequest::default() });
        }

        self.update_drop_feedback(true, Some((tab_index, location)));
    }

    /// O `onDragEnd` de uma aba: com a operação de janela nova solta o arrasto numa janela auxiliar.
    pub fn on_tab_drag_end(&mut self, is_new_window_operation: bool, is_window_dragged_over: bool, last_drag_modifiers: DragModifiers, ops: &dyn TabsGroupOps) {
        self.update_drop_feedback(false, None);
        let dragged_editors = ops.take_dragged_editors();

        if !is_new_window_operation || is_window_dragged_over || dragged_editors.is_empty() {
            return; // drag to open in new window is disabled
        }

        let Some(target_group_id) = ops.create_auxiliary_part_for_drag() else { return };

        let editors: Vec<EditorInputRef> = dragged_editors.iter().map(|dragged| dragged.editor.clone()).collect();
        if self.base.is_move_operation(last_drag_modifiers, target_group_id, Some(&dragged_editors[0].editor), self.is_macintosh) {
            ops.move_editors_to(&editors, target_group_id);
        } else {
            ops.copy_editors_to(&editors, target_group_id);
        }

        ops.focus_other_group(target_group_id);
    }

    /// O `onDrop` de uma aba: o índice é o da aba, mais um se o soltar foi na metade direita.
    pub fn on_tab_drop(&mut self, tab_index: usize, location: DragLocation, modifiers: DragModifiers, transfer: &DropTransfer, ops: &dyn TabsGroupOps) {
        self.update_drop_feedback(false, None);

        // compute the target index
        let target_index = if location == DragLocation::Right { tab_index + 1 } else { tab_index };

        self.on_drop(modifiers, target_index, transfer, ops);
    }

    /// O `onDragEnter` do contêiner de abas: liga o modo de rolagem durante o arrasto e só dá feedback se o alvo é o contêiner.
    pub fn on_container_drag_enter(&mut self, target_is_container: bool, transfer: &DropTransfer) -> DragEnterOutcome {
        // Always enable support to scroll while dragging
        self.container.scroll_mode = true;

        // Return if the target is not on the tabs container
        if !target_is_container {
            return DragEnterOutcome { drop_effect: None, feedback_updated: false };
        }

        match self.drag_enter_effect(transfer) {
            Err(()) => DragEnterOutcome { drop_effect: Some(DropEffect::None), feedback_updated: false },
            Ok(drop_effect) => {
                self.update_drop_feedback(true, None);
                DragEnterOutcome { drop_effect, feedback_updated: true }
            }
        }
    }

    /// O `onDragLeave` e o `onDragEnd` do contêiner de abas.
    pub fn on_container_drag_leave(&mut self) {
        self.update_drop_feedback(false, None);
        self.container.scroll_mode = false;
    }

    /// O `onDrop` do contêiner de abas: soltar no vazio vai para o fim (do grupo, se for um grupo arrastado).
    pub fn on_container_drop(&mut self, target_is_container: bool, modifiers: DragModifiers, transfer: &DropTransfer, ops: &dyn TabsGroupOps) {
        self.update_drop_feedback(false, None);
        self.container.scroll_mode = false;

        if target_is_container {
            let is_group_transfer = transfer.group.is_some();
            let target_index = if is_group_transfer { self.host().group_count() } else { self.base.tabs_model.count() };
            self.on_drop(modifiers, target_index, transfer, ops);
        }
    }

    /// `onDrop`: aplica o que foi solto (um grupo, editores, itens de árvore ou recursos) no índice da aba de destino.
    pub fn on_drop(&mut self, modifiers: DragModifiers, target_tab_index: usize, transfer: &DropTransfer, ops: &dyn TabsGroupOps) {
        self.update_drop_feedback(false, None);
        self.container.scroll_mode = false;

        let host = self.host();
        let mut target_editor_index = if self.model_kind == TabsModelKind::Unsticky { (target_tab_index + ops.group_sticky_count()) as i64 } else { target_tab_index as i64 };
        let options = DropEditorOptions { sticky: self.model_kind == TabsModelKind::Sticky && self.base.tabs_model.sticky_count() as i64 == target_editor_index, index: target_editor_index };

        // Check for group transfer
        if let Some(data) = &transfer.group {
            if let Some(&source_group_id) = data.first() {
                if ops.has_group(source_group_id) {
                    let mut merge_group_options = MergeGroupOptions { index: usize::try_from(target_editor_index).ok(), ..MergeGroupOptions::default() };
                    if !self.base.is_move_operation(modifiers, source_group_id, None, self.is_macintosh) {
                        merge_group_options.mode = Some(MergeGroupMode::CopyEditors);
                    }

                    ops.merge_group(source_group_id, merge_group_options);
                }

                ops.focus_group();
                ops.clear_group_transfer();
            }
        }
        // Check for editor transfer
        else if let Some(data) = &transfer.editors {
            if let Some(first) = data.first()
                && ops.has_group(first.group_id)
            {
                let source_group_id = first.group_id;
                for dragged in data {
                    let editor = &dragged.editor;

                    // Only allow moving/copying from a single group source
                    if source_group_id != dragged.group_id {
                        continue;
                    }

                    // Keep the same order when moving / copying editors within the same group
                    let source_editor_index = ops.group_index_of_editor(source_group_id, editor);
                    if source_group_id == host.group_id() && source_editor_index < target_editor_index {
                        target_editor_index -= 1;
                    }

                    let editor_options = DropEditorOptions { index: target_editor_index, ..options };
                    if self.base.is_move_operation(modifiers, dragged.group_id, Some(editor), self.is_macintosh) {
                        ops.move_editor_from(source_group_id, editor, editor_options);

                        if self.model_kind == TabsModelKind::Unsticky && ops.group_is_sticky(editor) {
                            ops.unstick_editor(editor);
                        }
                    } else {
                        ops.copy_editor_from(source_group_id, editor, editor_options);
                    }

                    target_editor_index += 1;
                }
            }

            ops.focus_group();
            ops.clear_editor_transfer();
        }
        // Check for tree items
        else if let Some(count) = transfer.tree_items {
            if count > 0 {
                ops.open_tree_items_drop(options);
            }

            ops.clear_tree_items_transfer();
        }
        // Check for URI transfer
        else {
            ops.handle_resources_drop(options);
        }
    }
}

/// O contêiner de abas e o contêiner de ações e abas: o que o original escreve neles (classes, variáveis CSS e estilos) e
/// que o desenho nativo lê.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabsContainerView {
    /// `tabs-and-actions-container.wrapping`.
    pub wrapping: bool,
    /// `tabs-container.scroll`: o contêiner rola durante o arrastar.
    pub scroll_mode: bool,
    /// `--last-tab-add-tab-width`.
    pub last_tab_add_tab_width: f64,
    /// `--last-tab-margin-right`.
    pub last_tab_margin_right: f64,
    /// `stickyTabsBackground.style.width`.
    pub sticky_tabs_background_width: f64,
    /// `tabs-container.disable-sticky-tabs`.
    pub disable_sticky_tabs: bool,
    /// `connected-tab-upper-bar`.
    pub connected_tab_upper_bar: bool,
    /// `has-add-tab`.
    pub has_add_tab: bool,
    /// `connected-tabs-labels` no pai.
    pub connected_tabs_labels: bool,
    /// `connected-tabs-wrapping` no pai.
    pub connected_tabs_wrapping: bool,
    /// `style.overflow = 'visible'` (senão `hidden`) no contêiner de abas e na barra de rolagem.
    pub overflow_visible: bool,
    /// `tabsScrollbar.getScrollDimensions()`: `(width, scrollWidth)`.
    pub scroll_dimensions: (f64, f64),
    /// `tabsScrollbar.getScrollPosition().scrollLeft`.
    pub scroll_left: f64,
    /// `controls-only-row` no `addTabContainer`.
    pub add_tab_controls_only_row: bool,
    /// `--modern-ui-editor-tab-left-inset`, se definida.
    pub add_tab_left_inset: Option<f64>,
    /// O botão de nova aba (`addTabContainer`).
    pub add_tab: AddTabPresence,
    /// A barra de abas entre as barras do pai.
    pub bars: TabsBarContext,
    /// O `connectedTabOverflowEdge`.
    pub overflow_edge: ConnectedOverflowEdge,
}

/// O `addTabContainer`: inexistente, escondido (`hidden`) ou visível.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AddTabPresence {
    #[default]
    None,
    Hidden,
    Visible,
}

/// O que `doLayoutTabs` lê dos irmãos desta barra de abas no elemento pai.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabsBarContext {
    /// Esta é a primeira barra de abas visível do pai (`firstVisibleTabBar === tabsAndActionsContainer`).
    pub top_tab_bar: bool,
    /// `parent.classList.contains('two-tab-bars')`.
    pub two_tab_bars: bool,
    /// Alguma outra barra de abas do pai tem a classe `wrapping`.
    pub other_bar_wrapping: bool,
}

/// O `connectedTabOverflowEdge`: posição, variáveis e classes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectedOverflowEdge {
    pub top: Option<f64>,
    pub right: Option<f64>,
    pub bottom: Option<f64>,
    pub left: Option<f64>,
    /// `--modern-ui-connected-tab-border`.
    pub border: Option<String>,
    /// `--modern-ui-connected-tab-overflow-top-border`.
    pub overflow_top_border: Option<String>,
    /// `--modern-ui-connected-tab-overflow-top-background`.
    pub overflow_top_background: Option<String>,
    pub left_clipped: bool,
    pub right_clipped: bool,
    /// `connected-tab-hovered`.
    pub hovered: bool,
    /// `connected-tab-top-row`.
    pub top_row: bool,
    /// `connected-tab-adjacent-sticky`.
    pub adjacent_sticky: bool,
}

/// `IConnectedTabBounds` (de `connectedTabClipping.ts`): `tab` é o índice da aba ativa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConnectedTabBounds {
    pub tab: usize,
    pub fill_left: f64,
    pub fill_right: f64,
    pub viewport_left: f64,
    pub viewport_right: f64,
    pub shoulder_extent: f64,
}

/// As classes que `updateConnectedTabClipping` (de `connectedTabClipping.ts`) alterna na aba e no `overflowEdge`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConnectedClipping {
    pub tab: ConnectedTabClasses,
    pub edge_left_clipped: bool,
    pub edge_right_clipped: bool,
}

/// `updateConnectedTabClipping(bounds, scrollLeft)`, sem DOM: devolve as classes.
pub fn connected_tab_clipping(bounds: &ConnectedTabBounds, scroll_left: f64) -> ConnectedClipping {
    let ConnectedTabBounds { fill_left, fill_right, viewport_left, viewport_right, shoulder_extent, .. } = *bounds;
    let visible_left = scroll_left + viewport_left;
    let visible_right = scroll_left + viewport_right;
    let visible_fill_left = fill_left.max(visible_left);
    let visible_fill_right = fill_right.min(visible_right);
    let left_clipped = fill_left < visible_left;
    let right_clipped = fill_right > visible_right;
    let left_edge = fill_left - shoulder_extent < visible_left;
    let right_edge = fill_right + shoulder_extent > visible_right;
    let hidden = visible_fill_left + shoulder_extent >= visible_fill_right;

    ConnectedClipping {
        tab: ConnectedTabClasses { left_edge, right_edge, left_clipped, right_clipped, hidden },
        edge_left_clipped: left_clipped && !hidden,
        edge_right_clipped: right_edge && !hidden,
    }
}

/// As medidas de uma aba `fit` que `getFitTabWidth` pede ao navegador (`getComputedStyle`), com o `max-width` solto
/// (`getFitTabWidths`: a largura intrínseca, não a restrição da linha atual).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FitTabMetrics {
    /// `Number.parseFloat(style.width)`.
    pub width: f64,
    pub font_size: f64,
    pub padding_left: f64,
    pub padding_right: f64,
    pub border_left_width: f64,
    pub border_right_width: f64,
    /// `--modern-ui-tab-action-fit-padding`, `None` se vazia ou inválida (o `|| 28` do original).
    pub action_fit_padding: Option<f64>,
    /// A largura de `.monaco-icon-name-container` dentro de `.tab-label`, se existe.
    pub name_width: Option<f64>,
    /// As larguras de `.tab-label` e de `.monaco-icon-label-container`, se os dois existem.
    pub label_widths: Option<(f64, f64)>,
}

/// `getFitTabWidth`, com as medidas já lidas e o estado da aba.
pub fn get_fit_tab_width(state: &TabState, metrics: &FitTabMetrics) -> f64 {
    let width = metrics.width;
    let has_persistent_action = (state.dirty && !(state.close_action_off && state.dirty_border_top)) || (state.sticky && !state.pinned_action_off);
    if (state.close_action_off && !has_persistent_action) || state.sticky_compact {
        return width;
    }

    let label_padding = if state.tab_actions_left { metrics.padding_right } else { metrics.padding_left };
    let action_padding = metrics.action_fit_padding.filter(|padding| padding.is_finite() && *padding != 0.0).unwrap_or(28.0);
    let current_action_padding = if state.tab_actions_left { metrics.padding_left } else { metrics.padding_right };
    let intrinsic_width = if has_persistent_action { width + (action_padding - current_action_padding).max(0.0) } else { width };
    let name_width = metrics.name_width.unwrap_or(0.0);
    let adornment_width = metrics.label_widths.map_or(0.0, |(label, container)| label - container);
    let border_width = metrics.border_left_width + metrics.border_right_width;
    let short_name_width = metrics.font_size.max(TabWidth::FIT - label_padding - action_padding - adornment_width - border_width);
    let minimum_label_width = adornment_width + name_width.min(short_name_width);

    // Keep short fit names complete without reserving the action column on naturally wider tabs.
    intrinsic_width.max(label_padding + action_padding + minimum_label_width + border_width)
}

/// As medidas de uma aba no layout com quebra de linha.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WrapTabMeasure {
    /// `getTotalWidth(tab)`: largura mais as margens.
    pub total_width: f64,
    /// `tab.offsetWidth`.
    pub offset_width: f64,
    /// `tab.offsetTop`.
    pub offset_top: f64,
}

/// O que `doLayoutTabsWrapping` e `computeHeight` leem do navegador, refeito a cada leitura (o layout reflui quando a classe
/// `wrapping` muda).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WrappingMeasure {
    /// `tabsContainer.offsetWidth`.
    pub visible_tabs_width: f64,
    /// `tabsContainer.scrollWidth`.
    pub all_tabs_width: f64,
    /// `tabsContainer.offsetHeight`.
    pub tabs_container_height: f64,
    /// `tabsAndActionsContainer.offsetHeight`.
    pub tabs_and_actions_height: f64,
    /// `getContentWidth(tabsAndActionsContainer)`.
    pub tabs_and_actions_content_width: f64,
    /// `editorToolbarsContainer.offsetWidth`.
    pub editor_toolbar_width: f64,
    /// `addTabContainer?.offsetWidth ?? 0`.
    pub add_tab_offset_width: f64,
    /// `getTotalWidth(addTabContainer)`.
    pub add_tab_total_width: f64,
    /// `addTabContainer.offsetTop`.
    pub add_tab_offset_top: f64,
    pub tabs: Vec<WrapTabMeasure>,
}

/// As medidas de uma aba para `updateWrappedAddTabAlignment`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AlignTabMeasure {
    pub top: f64,
    /// `getBoundingClientRect().left` e `.right`.
    pub left: f64,
    pub right: f64,
    pub padding_left: f64,
    pub padding_right: f64,
}

/// O que `updateWrappedAddTabAlignment` lê do navegador.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddTabAlignmentMeasure {
    /// `addTabContainer.offsetTop`.
    pub add_tab_top: f64,
    pub tabs: Vec<AlignTabMeasure>,
    /// `getComputedStyle(tabsContainer).direction === 'rtl'`.
    pub right_to_left: bool,
    /// `tabsContainer.getBoundingClientRect().width` e `tabsContainer.offsetWidth`.
    pub tabs_bounds_width: f64,
    pub tabs_offset_width: f64,
    pub add_tab_left: f64,
    pub add_tab_right: f64,
    pub toolbar_left: f64,
    pub toolbar_right: f64,
    /// `inset-inline-end` do `::before` do `addTabContainer`, se for um número.
    pub backing_end_inset: Option<f64>,
}

/// `updateWrappedAddTabAlignment`: a classe `controls-only-row` e `--modern-ui-editor-tab-left-inset`.
pub fn compute_add_tab_alignment(tabs_wrap_multi_line: bool, measure: &AddTabAlignmentMeasure) -> (bool, Option<f64>) {
    let preceding_row_top = measure.tabs.iter().map(|tab| tab.top).filter(|top| *top < measure.add_tab_top).fold(f64::NEG_INFINITY, f64::max);
    let preceding_row_tabs: Vec<&AlignTabMeasure> = measure.tabs.iter().filter(|tab| tab.top == preceding_row_top).collect();
    let controls_only_row = tabs_wrap_multi_line && !preceding_row_tabs.is_empty() && measure.tabs.iter().all(|tab| tab.top != measure.add_tab_top);
    if !controls_only_row {
        return (false, None);
    }

    let right_to_left = measure.right_to_left;
    let Some(leading_tab) = preceding_row_tabs.iter().copied().reduce(|leading, tab| {
        if right_to_left {
            if tab.right > leading.right { tab } else { leading }
        } else if tab.left < leading.left {
            tab
        } else {
            leading
        }
    }) else {
        return (false, None);
    };

    let desired_inset = if right_to_left { leading_tab.padding_right } else { leading_tab.padding_left };
    let scale = if measure.tabs_offset_width > 0.0 { measure.tabs_bounds_width / measure.tabs_offset_width } else { 1.0 };
    let backing_overflow = measure.backing_end_inset.filter(|inset| inset.is_finite()).map_or(0.0, |inset| (-inset).max(0.0));
    let available_inset =
        (if right_to_left { measure.add_tab_left - measure.toolbar_right } else { measure.toolbar_left - measure.add_tab_right }) / scale - backing_overflow;

    (true, Some(desired_inset.min(available_inset.max(0.0))))
}

/// As medidas da aba ativa conectada (`doLayoutTabsNonWrapping`, com `closest(CONNECTED_EDITOR_TABS_SELECTOR)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectedActiveTabMeasure {
    /// `tabsContainer.scrollLeft`: reflete o ajuste nativo antes de a barra customizada atualizar.
    pub container_scroll_left: f64,
    /// `tabsContainer.getBoundingClientRect()`: `left` e `width`.
    pub tabs_left: f64,
    pub tabs_width: f64,
    /// `activeTab.firstElementChild.getBoundingClientRect()`.
    pub fill_left: f64,
    pub fill_right: f64,
    pub fill_top: f64,
    pub fill_bottom: f64,
    /// `tabsScrollbar.getDomNode().getBoundingClientRect()`: `top` e `bottom`.
    pub scrollable_top: f64,
    pub scrollable_bottom: f64,
    /// `--modern-ui-connected-tab-border` calculada na aba ativa.
    pub tab_border: String,
    /// `borderTopColor` e `backgroundImage` do irmão seguinte do preenchimento.
    pub overflow_top_border: String,
    pub overflow_top_background: String,
    /// A largura do `::after` do preenchimento (o ombro).
    pub shoulder_extent: f64,
}

/// As medidas de `updateConnectedTabClipping`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectedClippingMeasure {
    /// `addTabContainer?.offsetWidth`, `None` sem o botão de nova aba.
    pub add_tab_offset_width: Option<f64>,
    /// `tabsContainer.offsetWidth`.
    pub tabs_offset_width: f64,
    /// `tabsContainer.getBoundingClientRect()`: `left` e `width`.
    pub tabs_left: f64,
    pub tabs_width: f64,
    /// `addTabContainer.getBoundingClientRect()`: `left` e `right`.
    pub add_tab_left: f64,
    pub add_tab_right: f64,
    pub right_to_left: bool,
    /// `marginInlineEnd` da última aba.
    pub shoulder_space: f64,
    /// `getBoundingClientRect()` de cada aba: `(left, right)`.
    pub tab_bounds: Vec<(f64, f64)>,
    /// `connectedTabBounds.tab.getBoundingClientRect().left`.
    pub active_tab_left: f64,
}

/// O que o desenho nativo mede para o controle de abas. Cada método responde com o layout vigente do estado recebido (as
/// classes e variáveis que o controle já escreveu), como as leituras de `offsetWidth` e `getBoundingClientRect` do original.
pub trait TabsMeasurer {
    /// `getFitTabWidths`: as medidas de cada aba pedida, com o `max-width` solto e a classe `tab-action-sizing-rest` do estado.
    fn measure_fit_tabs(&self, tabs: &[Tab], indexes: &[usize]) -> Vec<FitTabMetrics>;
    /// As leituras de `doLayoutTabsNonWrapping`.
    fn measure_tabs(&self, container: &TabsContainerView, tabs: &[Tab]) -> TabsMetrics;
    /// As leituras de `doLayoutTabsWrapping` e de `computeHeight`.
    fn measure_wrapping(&self, container: &TabsContainerView, tabs: &[Tab]) -> WrappingMeasure;
    /// As leituras de `updateWrappedAddTabAlignment`.
    fn measure_add_tab_alignment(&self, container: &TabsContainerView, tabs: &[Tab]) -> AddTabAlignmentMeasure;
    /// As leituras do recorte da aba ativa conectada, `None` se o preenchimento da aba não existe.
    fn measure_connected_active_tab(&self, container: &TabsContainerView, tabs: &[Tab], active_tab_index: usize) -> Option<ConnectedActiveTabMeasure>;
    /// As leituras de `updateConnectedTabClipping`.
    fn measure_connected_clipping(&self, container: &TabsContainerView, tabs: &[Tab], active_tab_index: Option<usize>) -> ConnectedClippingMeasure;
}

/// `compute_last_in_row`: marca a última aba de cada linha pelas posições `offsetTop`; a última de todas é sempre a última da
/// linha.
pub fn compute_last_in_row(tab_tops: &[f64]) -> Vec<bool> {
    let mut last_in_row = vec![false; tab_tops.len()];
    let mut current_tabs_pos_y: Option<f64> = None;
    let mut last_tab: Option<usize> = None;
    for (index, &tab_pos_y) in tab_tops.iter().enumerate() {
        // Marks a new or the first row of tabs
        if current_tabs_pos_y != Some(tab_pos_y) {
            current_tabs_pos_y = Some(tab_pos_y);
            if let Some(previous) = last_tab {
                last_in_row[previous] = true; // previous tab must be last in row then
            }
        }

        // Always remember last tab and ensure the last-in-row class is not present until we know the tab is last
        last_tab = Some(index);
        last_in_row[index] = false;
    }

    // Last tab overally is always last-in-row
    if let Some(last) = last_tab {
        last_in_row[last] = true;
    }

    last_in_row
}

//#region Regras de tamanho das abas (`multieditortabscontrol.css`)

/// A especificidade do prefixo comum das regras de aba (`.monaco-workbench .part.editor > .content
/// .editor-group-container > .title .tabs-container > .tab`): oito classes. Cada regra soma as classes e pseudoclasses
/// que acrescenta (`:not(X)` e `:is(...)` contam como o argumento).
const TAB_SPECIFICITY: u32 = 8;
/// O prefixo das regras com `.title > .tabs-and-actions-container.wrapping .tabs-container > .tab`: dez classes.
const WRAPPING_TAB_SPECIFICITY: u32 = 10;
/// `.tab > .tab-actions { width: 28px }`.
const TAB_ACTIONS_WIDTH: f64 = 28.0;
/// `.tab-fade-hider { width: 5px }`.
const TAB_FADE_HIDER_WIDTH: f64 = 5.0;
/// O `min-width` das abas `shrink` e `fixed` com a ação à esquerda, e a largura da aba `sticky-shrink`.
const TAB_SHRINK_MIN_WIDTH: f64 = 80.0;
/// `.tab.sizing-fixed { min-width: ...50px; max-width: ...160px }`: os padrões de `--tab-sizing-fixed-min-width` e
/// `--tab-sizing-fixed-max-width`.
pub const TAB_FIXED_DEFAULT_MIN_WIDTH: f64 = 50.0;
pub const TAB_FIXED_DEFAULT_MAX_WIDTH: f64 = 160.0;

/// Um comprimento de `width`, `min-width` ou `max-width`: `auto` (ou `none`), pixels ou `fit-content`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CssLength {
    Auto,
    Px(f64),
    FitContent,
}

/// As classes que os seletores de `multieditortabscontrol.css` testam em uma aba e no contêiner dela.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabCssClasses {
    pub sizing_fit: bool,
    pub sizing_shrink: bool,
    pub sizing_fixed: bool,
    pub has_icon: bool,
    pub sticky: bool,
    pub sticky_compact: bool,
    pub sticky_shrink: bool,
    pub dirty: bool,
    pub dirty_border_top: bool,
    pub tab_actions_left: bool,
    pub tab_actions_right: bool,
    pub close_action_off: bool,
    pub last_in_row: bool,
    pub last_tab: bool,
    /// `:last-child`: a última aba é o último filho só sem o botão de nova aba (escondido ou não) depois dela.
    pub last_child: bool,
    /// `tabs-and-actions-container.wrapping`.
    pub wrapping: bool,
    /// `tabs-and-actions-container.has-add-tab`.
    pub has_add_tab: bool,
    /// `tabs-container.disable-sticky-tabs`.
    pub disable_sticky_tabs: bool,
}

impl TabCssClasses {
    /// As classes de uma aba, lidas do estado que o controle escreveu nela e nos contêineres.
    pub fn of(state: &TabState, container: &TabsContainerView) -> Self {
        Self {
            sizing_fit: state.sizing_fit,
            sizing_shrink: state.sizing_shrink,
            sizing_fixed: state.sizing_fixed,
            has_icon: state.has_icon,
            sticky: state.sticky,
            sticky_compact: state.sticky_compact,
            sticky_shrink: state.sticky_shrink,
            dirty: state.dirty,
            dirty_border_top: state.dirty_border_top,
            tab_actions_left: state.tab_actions_left,
            tab_actions_right: state.tab_actions_right,
            close_action_off: state.close_action_off,
            last_in_row: state.last_in_row,
            last_tab: state.last_tab,
            last_child: state.last_tab && container.add_tab == AddTabPresence::None,
            wrapping: container.wrapping,
            has_add_tab: container.has_add_tab,
            disable_sticky_tabs: container.disable_sticky_tabs,
        }
    }

    fn has_sizing(&self) -> bool {
        self.sizing_fit || self.sizing_shrink || self.sizing_fixed
    }

    fn sizing_shrink_or_fixed(&self) -> bool {
        self.sizing_shrink || self.sizing_fixed
    }

    /// `.tab.close-action-off:not(.dirty) > .tab-actions`, `.tab.sticky-compact > .tab-actions` e
    /// `.tab.close-action-off.dirty-border-top > .tab-actions` (`display: none`).
    pub fn tab_actions_hidden(&self) -> bool {
        self.sticky_compact || (self.close_action_off && (!self.dirty || self.dirty_border_top))
    }

    /// `.tab-fade-hider` mostrado: `display: flex` nas abas `shrink` e `fixed` com a ação à esquerda ou desligada.
    pub fn tab_fade_hider_shown(&self) -> bool {
        self.sizing_shrink_or_fixed() && (self.tab_actions_left || self.close_action_off)
    }
}

/// O que as regras leem além das classes: as variáveis CSS que o controle escreve, as classes da raiz e o que as regras de
/// porcentagem medem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabSizingParams {
    /// `--tab-sizing-fixed-min-width` e `--tab-sizing-fixed-max-width`.
    pub fixed_min_width: f64,
    pub fixed_max_width: f64,
    /// `--tab-sizing-current-width` (`fixed` com o mouse sobre as abas).
    pub current_width: Option<f64>,
    /// `--tab-sizing-fit-width`.
    pub fit_width: Option<f64>,
    /// `.modern-ui-tabs` na raiz: as regras de tamanho de `tabs.css`.
    pub modern_ui_tabs: bool,
    /// `.modern-ui-connected-editor-tabs` na raiz: a regra de `connectedEditorTabs.css` que tira o `min-width` das abas.
    pub connected_editor_tabs: bool,
    /// `100%` nas regras de `max-width`: a largura do contêiner de abas.
    pub tabs_container_width: f64,
    /// `--last-tab-margin-right`, `--last-tab-add-tab-width` e `--modern-ui-tab-terminal-gutter`.
    pub last_tab_margin_right: f64,
    pub last_tab_add_tab_width: f64,
    pub terminal_gutter: f64,
}

impl Default for TabSizingParams {
    fn default() -> Self {
        Self {
            fixed_min_width: TAB_FIXED_DEFAULT_MIN_WIDTH,
            fixed_max_width: TAB_FIXED_DEFAULT_MAX_WIDTH,
            current_width: None,
            fit_width: None,
            modern_ui_tabs: false,
            connected_editor_tabs: false,
            tabs_container_width: 0.0,
            last_tab_margin_right: 0.0,
            last_tab_add_tab_width: 0.0,
            terminal_gutter: 0.0,
        }
    }
}

/// O valor vigente de uma propriedade depois da cascata: `!important` vence, depois a especificidade, depois a ordem de
/// declaração (cada regra é declarada na ordem do arquivo, e a mais tardia vence o empate).
struct Cascade<T: Copy> {
    winner: Option<(bool, u32, T)>,
}

impl<T: Copy> Cascade<T> {
    fn new() -> Self {
        Self { winner: None }
    }

    fn declare(&mut self, specificity: u32, value: T) {
        self.declare_flagged(false, specificity, value);
    }

    fn declare_important(&mut self, specificity: u32, value: T) {
        self.declare_flagged(true, specificity, value);
    }

    fn declare_flagged(&mut self, important: bool, specificity: u32, value: T) {
        let wins = self.winner.is_none_or(|(current_important, current_specificity, _)| (important, specificity) >= (current_important, current_specificity));
        if wins {
            self.winner = Some((important, specificity, value));
        }
    }

    fn value_or(&self, initial: T) -> T {
        self.winner.map_or(initial, |(_, _, value)| value)
    }
}

/// O que a cascata de `multieditortabscontrol.css` decide para a caixa de uma aba (um item flex da linha de abas).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabBoxStyle {
    /// `width`, `None` é `auto`.
    pub width: Option<f64>,
    pub min_width: CssLength,
    pub max_width: CssLength,
    pub flex_grow: f64,
    pub flex_shrink: f64,
    /// `flex-basis`, `None` é `auto`.
    pub flex_basis: Option<f64>,
    pub padding_left: f64,
    pub padding_right: f64,
    /// `margin-inline-end`.
    pub margin_end: f64,
    /// `position: sticky` (as abas compactas e `shrink` fixadas, a não ser com `disable-sticky-tabs`).
    pub position_sticky: bool,
}

/// A cascata de `multieditortabscontrol.css` (com as regras de tamanho de `tabs.css` e `connectedEditorTabs.css` quando a raiz
/// as liga) para a caixa de uma aba. Cada propriedade lista as regras na ordem do arquivo, com a especificidade do seletor.
pub fn tab_box_style(classes: &TabCssClasses, params: &TabSizingParams) -> TabBoxStyle {
    let c = classes;
    let modern = params.modern_ui_tabs;
    let sticky_compact_or_shrink = c.has_sizing() && (c.sticky_compact || c.sticky_shrink);
    let modern_fit = modern && c.sizing_fit && !c.sticky_compact;
    let modern_connected_sizing = modern && params.connected_editor_tabs && c.sizing_shrink_or_fixed() && !c.sticky_compact;
    let fixed_current = params.current_width.unwrap_or(params.fixed_min_width);
    let fixed_current_max = params.current_width.unwrap_or(params.fixed_max_width);

    // width
    let mut width = Cascade::<Option<f64>>::new();
    if c.sizing_fit {
        width.declare(TAB_SPECIFICITY + 1, Some(TabWidth::FIT)); // L211
    }
    if c.has_sizing() && c.sticky_compact {
        width.declare(TAB_SPECIFICITY + 2, Some(TabWidth::COMPACT)); // L257
    }
    if c.has_sizing() && c.sticky_shrink {
        width.declare(TAB_SPECIFICITY + 2, Some(TabWidth::SHRINK)); // L267
    }
    if modern_fit {
        width.declare_important(TAB_SPECIFICITY - 1, params.fit_width); // tabs.css: `var(--tab-sizing-fit-width, auto) !important`
    }
    if modern && c.sticky_compact {
        width.declare_important(TAB_SPECIFICITY - 2, Some(MODERN_UI_COMPACT_PINNED_TAB_WIDTH)); // tabs.css: `.tab.sticky-compact`
    }

    // min-width
    let mut min_width = Cascade::<CssLength>::new();
    if c.sizing_fit {
        min_width.declare(TAB_SPECIFICITY + 1, CssLength::FitContent); // L211
    }
    if c.sizing_fixed {
        min_width.declare(TAB_SPECIFICITY + 1, CssLength::Px(fixed_current)); // L217
    }
    if c.sizing_fixed && c.last_in_row {
        min_width.declare(TAB_SPECIFICITY + 2, CssLength::Px(fixed_current - 1.0)); // L223
    }
    if c.sizing_shrink {
        min_width.declare(TAB_SPECIFICITY + 1, CssLength::Px(TAB_SHRINK_MIN_WIDTH)); // L234
    }
    if c.has_sizing() && c.sticky_compact {
        min_width.declare(TAB_SPECIFICITY + 2, CssLength::Px(TabWidth::COMPACT)); // L257
    }
    if c.has_sizing() && c.sticky_shrink {
        min_width.declare(TAB_SPECIFICITY + 2, CssLength::Px(TabWidth::SHRINK)); // L267
    }
    if c.sizing_shrink_or_fixed() && c.tab_actions_left {
        min_width.declare(TAB_SPECIFICITY + 2, CssLength::Px(TAB_SHRINK_MIN_WIDTH)); // L312
    }
    if modern_connected_sizing {
        // connectedEditorTabs.css: `min-width: max(80px, var(--tab-sizing-fixed-min-width, 0px))`, 13 classes
        min_width.declare(13, CssLength::Px(TAB_SHRINK_MIN_WIDTH.max(params.fixed_min_width)));
    }
    if modern_fit {
        min_width.declare_important(TAB_SPECIFICITY - 1, CssLength::Px(0.0)); // tabs.css: `min-width: 0 !important`
    }
    if modern && c.sticky_compact {
        min_width.declare_important(TAB_SPECIFICITY - 2, CssLength::Px(MODERN_UI_COMPACT_PINNED_TAB_WIDTH));
    }

    // max-width
    let mut max_width = Cascade::<CssLength>::new();
    if c.sizing_fixed {
        max_width.declare(TAB_SPECIFICITY + 1, CssLength::Px(fixed_current_max)); // L217
    }
    if c.sizing_shrink {
        max_width.declare(TAB_SPECIFICITY + 1, CssLength::FitContent); // L234
    }
    if c.has_sizing() && c.sticky_compact {
        max_width.declare(TAB_SPECIFICITY + 2, CssLength::Px(TabWidth::COMPACT)); // L257
    }
    if c.has_sizing() && c.sticky_shrink {
        max_width.declare(TAB_SPECIFICITY + 2, CssLength::Px(TabWidth::SHRINK)); // L267
    }
    if modern && c.wrapping && c.sizing_fit && !c.sticky_compact {
        // tabs.css: `max-width: 100%` e, na última aba, `max(80px, 100% - margem - botão - vão)`
        max_width.declare(15, CssLength::Px(params.tabs_container_width));
        if c.last_tab {
            let room = params.tabs_container_width - params.last_tab_margin_right - params.last_tab_add_tab_width - params.terminal_gutter;
            max_width.declare(17, CssLength::Px(TabWidth::SHRINK.max(room)));
        }
    }
    if modern && c.sticky_compact {
        max_width.declare_important(TAB_SPECIFICITY - 2, CssLength::Px(MODERN_UI_COMPACT_PINNED_TAB_WIDTH));
    }

    // flex-grow, flex-shrink e flex-basis (`flex: 1 0 0` é a abreviação das três)
    let mut flex_grow = Cascade::<f64>::new();
    let mut flex_shrink = Cascade::<f64>::new();
    let mut flex_basis = Cascade::<Option<f64>>::new();
    if c.sizing_fit {
        flex_shrink.declare(TAB_SPECIFICITY + 1, 0.0); // L215
    }
    if c.sizing_fixed {
        flex_grow.declare(TAB_SPECIFICITY + 1, 1.0); // L220: `flex: 1 0 0`
        flex_shrink.declare(TAB_SPECIFICITY + 1, 0.0);
        flex_basis.declare(TAB_SPECIFICITY + 1, Some(0.0));
    }
    if c.wrapping && c.sizing_fit && c.last_in_row && !c.last_child {
        flex_grow.declare(WRAPPING_TAB_SPECIFICITY + 3, 1.0); // L231
    }
    if c.sizing_shrink {
        flex_basis.declare(TAB_SPECIFICITY + 1, Some(0.0)); // L234
        flex_grow.declare(TAB_SPECIFICITY + 1, 1.0);
    }
    if sticky_compact_or_shrink {
        flex_basis.declare(TAB_SPECIFICITY + 2, Some(0.0)); // L241
        flex_grow.declare(TAB_SPECIFICITY + 2, 0.0);
    }
    if modern_fit {
        // tabs.css: `flex: 0 0 auto !important`
        flex_grow.declare_important(TAB_SPECIFICITY - 1, 0.0);
        flex_shrink.declare_important(TAB_SPECIFICITY - 1, 0.0);
        flex_basis.declare_important(TAB_SPECIFICITY - 1, None);
    }
    if modern && c.sticky_compact {
        // tabs.css: `flex: 0 0 28px !important`
        flex_grow.declare_important(TAB_SPECIFICITY - 2, 0.0);
        flex_shrink.declare_important(TAB_SPECIFICITY - 2, 0.0);
        flex_basis.declare_important(TAB_SPECIFICITY - 2, Some(MODERN_UI_COMPACT_PINNED_TAB_WIDTH));
    }

    // padding-left
    let mut padding_left = Cascade::<f64>::new();
    padding_left.declare(TAB_SPECIFICITY, 10.0); // L122
    if c.sizing_shrink_or_fixed() && c.has_icon && c.tab_actions_right {
        padding_left.declare(TAB_SPECIFICITY + 3, 5.0); // L204
    }
    if c.sizing_shrink_or_fixed() && c.has_icon && c.close_action_off && !c.sticky_compact {
        padding_left.declare(TAB_SPECIFICITY + 4, 5.0); // L205
    }
    if c.tab_actions_left && !c.sticky_compact {
        padding_left.declare(TAB_SPECIFICITY + 2, 0.0); // L318
    }

    // padding-right (sem regra base: o inicial é 0)
    let mut padding_right = Cascade::<f64>::new();
    if c.sizing_shrink_or_fixed() && c.tab_actions_left {
        padding_right.declare(TAB_SPECIFICITY + 2, 5.0); // L312
    }
    if c.tab_actions_left && !c.sticky_compact {
        padding_right.declare(TAB_SPECIFICITY + 2, 10.0); // L318
    }
    if c.close_action_off {
        padding_right.declare(TAB_SPECIFICITY + 1, 10.0); // L529
    }
    if c.sizing_shrink_or_fixed() && c.close_action_off && !c.sticky_compact {
        padding_right.declare(TAB_SPECIFICITY + 3, 5.0); // L533
    }
    if c.close_action_off && c.dirty && !c.dirty_border_top && !c.sticky_compact {
        padding_right.declare(TAB_SPECIFICITY + 4, 0.0); // L542
    }
    if modern && c.sticky_compact {
        padding_left.declare_important(TAB_SPECIFICITY - 2, 0.0);
        padding_right.declare_important(TAB_SPECIFICITY - 2, 0.0);
    }

    // margin-inline-end (a margem do ombro e do vão do terminal, que as regras do Modern UI acrescentam, é de quem mede)
    let mut margin_end = Cascade::<f64>::new();
    if c.wrapping && c.last_tab {
        margin_end.declare(WRAPPING_TAB_SPECIFICITY + 1, params.last_tab_margin_right); // L192
    }
    if c.wrapping && c.has_add_tab && c.last_tab {
        margin_end.declare(WRAPPING_TAB_SPECIFICITY + 2, 0.0); // L196
    }

    TabBoxStyle {
        width: width.value_or(None),
        min_width: min_width.value_or(CssLength::Auto),
        max_width: max_width.value_or(CssLength::Auto),
        flex_grow: flex_grow.value_or(0.0),
        flex_shrink: flex_shrink.value_or(1.0),
        flex_basis: flex_basis.value_or(None),
        padding_left: padding_left.value_or(0.0),
        padding_right: padding_right.value_or(0.0),
        margin_end: margin_end.value_or(0.0),
        position_sticky: sticky_compact_or_shrink && !c.disable_sticky_tabs,
    }
}

//#region Traço do `.tab-fill` em alto contraste (`tabs.css` e `connectedEditorTabs.css`)

/// Um lado da borda do `.tab-fill`. `border-inline-color` declara [`StrokeSide::Right`] e [`StrokeSide::Left`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeSide {
    Top,
    Right,
    Bottom,
    Left,
}

impl StrokeSide {
    const ALL: [StrokeSide; 4] = [StrokeSide::Top, StrokeSide::Right, StrokeSide::Bottom, StrokeSide::Left];
    const INLINE_AND_BOTTOM: [StrokeSide; 3] = [StrokeSide::Right, StrokeSide::Bottom, StrokeSide::Left];
}

/// `border-style` de um lado do `.tab-fill`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabStrokeStyle {
    Solid,
    Dashed,
}

/// `border-color` de um lado do `.tab-fill`. `CurrentColor` é o que sobra quando a variável do tema não existe (a declaração
/// fica inválida na hora do cálculo e a propriedade volta ao valor inicial).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabStrokeColor<'a> {
    Transparent,
    CurrentColor,
    /// `var(--modern-ui-editor-tab-border)`: a cor da borda base, definida pela pintura do tema.
    EditorTabBorder,
    /// `var(--modern-ui-connected-tab-border)`.
    ConnectedTabBorder,
    Color(&'a str),
}

impl<'a> TabStrokeColor<'a> {
    fn of(color: Option<&'a str>) -> Self {
        color.map_or(Self::CurrentColor, Self::Color)
    }
}

/// O que as regras do `.tab-fill` leem: as classes da aba e do contêiner, os estados do ponteiro e do foco, as cores do tema.
#[derive(Clone, Copy, Debug, Default)]
pub struct TabFillStrokeInput<'a> {
    /// `tabs-and-actions-container.wrapping` e `.connected-tab-upper-bar`.
    pub wrapping: bool,
    pub upper_bar: bool,
    pub active: bool,
    pub selected: bool,
    pub multi_selected: bool,
    pub dirty_border_top: bool,
    /// `connected-tab-upper-row`.
    pub upper_row: bool,
    /// `:hover` e a classe `hovered`, que as regras tratam à parte.
    pub pointer_hover: bool,
    pub hovered: bool,
    /// `:focus` e `:focus-within`.
    pub focused: bool,
    pub focus_within: bool,
    pub tab_border: Option<&'a str>,
    pub contrast_active_border: Option<&'a str>,
    pub focus_border: Option<&'a str>,
    /// `--tab-dirty-border-top-color`.
    pub dirty_border_top_color: Option<&'a str>,
    /// `--vscode-spacing-size20`.
    pub spacing_size20: f64,
}

/// O realce de topo das abas modificadas: o gradiente de `linear-gradient(accent size20, transparent size20)` da regra
/// `.tab-fill` com `dirty-border-top`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabTopAccent<'a> {
    pub color: TabStrokeColor<'a>,
    pub height: f64,
    pub top_offset: f64,
}

/// A borda computada do `.tab-fill`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabFillStroke<'a> {
    styles: [TabStrokeStyle; 4],
    colors: [TabStrokeColor<'a>; 4],
    pub top_accent: Option<TabTopAccent<'a>>,
}

impl<'a> TabFillStroke<'a> {
    pub fn style(&self, side: StrokeSide) -> TabStrokeStyle {
        self.styles[side as usize]
    }

    pub fn color(&self, side: StrokeSide) -> TabStrokeColor<'a> {
        self.colors[side as usize]
    }
}

fn declare_sides<T: Copy>(cascades: &mut [Cascade<T>; 4], sides: &[StrokeSide], important: bool, specificity: u32, value: T) {
    for side in sides {
        cascades[*side as usize].declare_flagged(important, specificity, value);
    }
}

/// A cascata da borda do `.tab-fill` de uma aba do estilo conectado em alto contraste (`hc-black` e `hc-light`), com a
/// especificidade de cada seletor contada no CSS: `tabs.css` (L148, L935, L940, L946, L952) e `connectedEditorTabs.css` (L244,
/// L706, L746, L759, L764, L771, L802). A aba ativa fora da linha de cima desenha o traço no `.tab-connected-edge`, não aqui.
pub fn high_contrast_connected_tab_fill_stroke<'a>(input: &TabFillStrokeInput<'a>) -> TabFillStroke<'a> {
    let i = input;
    let hover = i.pointer_hover || i.hovered;
    let contrast = TabStrokeColor::of(i.contrast_active_border);
    let accent = TabStrokeColor::of(i.dirty_border_top_color);
    let mut styles = [Cascade::new(), Cascade::new(), Cascade::new(), Cascade::new()];
    let mut colors = [Cascade::new(), Cascade::new(), Cascade::new(), Cascade::new()];

    // L148 `.tab > .tab-fill { border: strokeThickness solid var(--modern-ui-editor-tab-border) }` (sete classes)
    declare_sides(&mut styles, &StrokeSide::ALL, false, 7, TabStrokeStyle::Solid);
    declare_sides(&mut colors, &StrokeSide::ALL, false, 7, TabStrokeColor::EditorTabBorder);
    // L706 `.tab.connected-tab-upper-row > .tab-fill { border: strokeThickness solid transparent }` (treze)
    if i.upper_row {
        declare_sides(&mut styles, &StrokeSide::ALL, false, 13, TabStrokeStyle::Solid);
        declare_sides(&mut colors, &StrokeSide::ALL, false, 13, TabStrokeColor::Transparent);
    }
    // L244 (vinte): fora da quebra e da barra superior, a aba inativa em repouso fica sem borda
    if !i.wrapping && !i.upper_bar && !i.active && !i.selected && !i.pointer_hover && !i.hovered && !i.focus_within {
        declare_sides(&mut colors, &StrokeSide::ALL, false, 20, TabStrokeColor::Transparent);
    }
    // L935 (dezesseis)
    if !i.active && !i.selected && !i.focused && !i.pointer_hover {
        declare_sides(&mut colors, &StrokeSide::ALL, false, 16, TabStrokeColor::of(i.tab_border));
    }
    // L746 (quinze)
    if i.upper_row && i.active {
        declare_sides(&mut colors, &StrokeSide::ALL, false, 15, TabStrokeColor::ConnectedTabBorder);
    }
    // L940 (dezesseis, `!important`): a aba selecionada inativa sem realce de topo
    if i.selected && !i.active && !i.focused && !i.dirty_border_top {
        declare_sides(&mut styles, &StrokeSide::ALL, true, 16, TabStrokeStyle::Solid);
        declare_sides(&mut colors, &StrokeSide::ALL, true, 16, contrast);
    }
    // L946 (dezesseis, `!important`): o `:hover` real, não a classe `hovered`
    if !i.active && !i.selected && !i.focused && i.pointer_hover && !i.dirty_border_top {
        declare_sides(&mut styles, &StrokeSide::ALL, true, 16, TabStrokeStyle::Dashed);
        declare_sides(&mut colors, &StrokeSide::ALL, true, 16, contrast);
    }
    // L952 (`!important`): o foco da aba inativa
    if !i.active && i.focused {
        declare_sides(&mut styles, &StrokeSide::ALL, true, 16, TabStrokeStyle::Solid);
        declare_sides(&mut colors, &StrokeSide::ALL, true, 16, TabStrokeColor::of(i.focus_border));
    }
    // L759 (dezesseis)
    if i.selected && !i.active && i.dirty_border_top {
        declare_sides(&mut colors, &StrokeSide::INLINE_AND_BOTTOM, false, 16, contrast);
    }
    // L764 (dezessete): `border-style: dashed` e depois `border-top-style: solid`, na mesma regra
    if !i.active && !i.selected && i.dirty_border_top && hover {
        declare_sides(&mut colors, &StrokeSide::INLINE_AND_BOTTOM, false, 17, contrast);
        declare_sides(&mut styles, &StrokeSide::ALL, false, 17, TabStrokeStyle::Dashed);
        declare_sides(&mut styles, &[StrokeSide::Top], false, 17, TabStrokeStyle::Solid);
    }
    // L771 (catorze): o realce de topo da aba modificada
    let top_accent = (i.dirty_border_top && (i.upper_row || !i.active)).then_some(TabTopAccent { color: accent, height: i.spacing_size20, top_offset: 0.0 });
    if top_accent.is_some() {
        declare_sides(&mut colors, &[StrokeSide::Top], false, 14, accent);
        declare_sides(&mut styles, &[StrokeSide::Top], false, 14, TabStrokeStyle::Solid);
    }
    // L802 (dezessete): a aba ativa multisselecionada na linha de cima
    if i.upper_row && i.active && i.multi_selected {
        declare_sides(&mut colors, &StrokeSide::ALL, false, 17, contrast);
        declare_sides(&mut colors, &[StrokeSide::Top], false, 17, if i.dirty_border_top { accent } else { contrast });
    }

    TabFillStroke {
        styles: styles.map(|style| style.value_or(TabStrokeStyle::Solid)),
        colors: colors.map(|color| color.value_or(TabStrokeColor::CurrentColor)),
        top_accent,
    }
}

//#endregion

/// A largura intrínseca (`max-content`, a caixa de borda) de uma aba: os paddings, o rótulo (com o `padding-right` de 5px das
/// abas `shrink` e a margem do selo das `shrink` e `fixed`), a coluna das ações (28px, a não ser com `display: none`) e o
/// `tab-fade-hider` (5px). `label_width` é a largura do conteúdo do rótulo (ícone e nome) e `has_badge` diz se o rótulo tem
/// o selo da decoração.
pub fn tab_intrinsic_width(classes: &TabCssClasses, style: &TabBoxStyle, label_width: f64, has_badge: bool, modern_ui_tabs: bool) -> f64 {
    let c = classes;

    // `.tab.sizing-shrink:not(.tab-actions-left):not(.close-action-off) .tab-label { padding-right: 5px }` (12 classes) contra
    // `tabs.css` (`.tab:is(.sizing-shrink, .sizing-fixed) > .tab-label { padding-right: 0 }`, 11 classes)
    let mut label_padding_right = Cascade::<f64>::new();
    if c.sizing_shrink && !c.tab_actions_left && !c.close_action_off {
        label_padding_right.declare(TAB_SPECIFICITY + 4, 5.0);
    }
    if modern_ui_tabs && c.sizing_shrink_or_fixed() {
        label_padding_right.declare(TAB_SPECIFICITY + 3, 0.0);
    }
    let badge_margin = if has_badge && c.sizing_shrink_or_fixed() { 5.0 } else { 0.0 };
    let actions = if c.tab_actions_hidden() { 0.0 } else { TAB_ACTIONS_WIDTH };
    let fade_hider = if c.tab_fade_hider_shown() { TAB_FADE_HIDER_WIDTH } else { 0.0 };

    style.padding_left + style.padding_right + label_width + label_padding_right.value_or(0.0) + badge_margin + actions + fade_hider
}

/// Um item flex da linha de abas: a caixa da aba e a largura intrínseca (`max-content`, que é também o `min-content`: o
/// rótulo não quebra, `white-space: nowrap`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabFlexItem {
    pub style: TabBoxStyle,
    pub content_width: f64,
}

/// O resultado de [`layout_tabs_row`]: a largura (caixa de borda) e a linha de cada item.
#[derive(Clone, Debug, PartialEq)]
pub struct TabsRowLayout {
    pub widths: Vec<f64>,
    /// A linha de cada item (`0` sem quebra).
    pub lines: Vec<usize>,
}

/// Um item com as medidas já resolvidas (§9.2 e §9.7 de CSS Flexbox).
struct ResolvedFlexItem {
    base: f64,
    hypothetical: f64,
    min: f64,
    max: f64,
    grow: f64,
    shrink: f64,
    margin: f64,
}

impl ResolvedFlexItem {
    fn new(item: &TabFlexItem) -> Self {
        let style = &item.style;
        let content = item.content_width;
        let padding = style.padding_left + style.padding_right;
        let min = match style.min_width {
            // O mínimo automático de um item flex: o menor entre o tamanho do conteúdo e o `width` especificado.
            CssLength::Auto => content.min(style.width.unwrap_or(content)),
            CssLength::Px(value) => value,
            CssLength::FitContent => content,
        }
        .max(padding);
        let max = match style.max_width {
            CssLength::Auto => f64::INFINITY,
            CssLength::Px(value) => value,
            CssLength::FitContent => content,
        };
        let base = style.flex_basis.or(style.width).unwrap_or(content).max(padding);
        let hypothetical = base.min(max).max(min);

        Self { base, hypothetical, min, max, grow: style.flex_grow, shrink: style.flex_shrink, margin: style.margin_end }
    }

    fn outer_hypothetical(&self) -> f64 {
        self.hypothetical + self.margin
    }
}

/// `resolve the flexible lengths` (CSS Flexbox §9.7) de uma linha de itens.
fn resolve_flexible_lengths(items: &[ResolvedFlexItem], line: &[usize], available: f64) -> Vec<f64> {
    let growing = line.iter().map(|&index| items[index].outer_hypothetical()).sum::<f64>() < available;
    let factor = |item: &ResolvedFlexItem| if growing { item.grow } else { item.shrink };
    let mut target = vec![0.0; line.len()];
    let mut frozen = vec![false; line.len()];

    // Size inflexible items.
    for (slot, &index) in line.iter().enumerate() {
        let item = &items[index];
        let inflexible = factor(item) == 0.0 || (growing && item.base > item.hypothetical) || (!growing && item.base < item.hypothetical);
        if inflexible {
            frozen[slot] = true;
            target[slot] = item.hypothetical;
        }
    }

    let used_space = |target: &[f64], frozen: &[bool]| -> f64 {
        line.iter().enumerate().map(|(slot, &index)| (if frozen[slot] { target[slot] } else { items[index].base }) + items[index].margin).sum()
    };
    let initial_free_space = available - used_space(&target, &frozen);

    // Loop: distribute the free space and freeze the items that violate a min or max constraint.
    while frozen.iter().any(|is_frozen| !is_frozen) {
        let unfrozen: Vec<usize> = (0..line.len()).filter(|&slot| !frozen[slot]).collect();
        let sum_factors: f64 = unfrozen.iter().map(|&slot| factor(&items[line[slot]])).sum();
        let mut remaining = available - used_space(&target, &frozen);
        if sum_factors < 1.0 {
            let scaled = initial_free_space * sum_factors;
            if scaled.abs() < remaining.abs() {
                remaining = scaled;
            }
        }

        let sum_scaled_shrink: f64 = unfrozen.iter().map(|&slot| items[line[slot]].shrink * items[line[slot]].base).sum();
        for &slot in &unfrozen {
            let item = &items[line[slot]];
            target[slot] = if remaining == 0.0 {
                item.base
            } else if growing {
                item.base + remaining * item.grow / sum_factors
            } else if sum_scaled_shrink > 0.0 {
                item.base - remaining.abs() * (item.shrink * item.base) / sum_scaled_shrink
            } else {
                item.base
            };
        }

        // Fix min/max violations.
        let mut violations = vec![0.0; line.len()];
        let mut total_violation = 0.0;
        for &slot in &unfrozen {
            let item = &items[line[slot]];
            let clamped = target[slot].min(item.max).max(item.min);
            violations[slot] = clamped - target[slot];
            total_violation += violations[slot];
            target[slot] = clamped;
        }
        for &slot in &unfrozen {
            if total_violation == 0.0 || (total_violation > 0.0 && violations[slot] > 0.0) || (total_violation < 0.0 && violations[slot] < 0.0) {
                frozen[slot] = true;
            }
        }
    }

    target
}

/// A linha de abas como contêiner flex (`.tabs-container { display: flex }`, com `flex-wrap: wrap` quando
/// `tabs-and-actions-container.wrapping`): quebra os itens em linhas pela largura hipotética e resolve os comprimentos
/// flexíveis de cada linha. `container_width` é a largura interna do contêiner de abas.
pub fn layout_tabs_row(items: &[TabFlexItem], container_width: f64, wrap: bool) -> TabsRowLayout {
    let resolved: Vec<ResolvedFlexItem> = items.iter().map(ResolvedFlexItem::new).collect();

    // Collect flex items into flex lines.
    let mut lines: Vec<Vec<usize>> = Vec::new();
    for (index, item) in resolved.iter().enumerate() {
        let outer = item.outer_hypothetical();
        let starts_new_line = match lines.last() {
            None => true,
            Some(line) => wrap && line.iter().map(|&other| resolved[other].outer_hypothetical()).sum::<f64>() + outer > container_width,
        };
        if starts_new_line {
            lines.push(Vec::new());
        }
        if let Some(line) = lines.last_mut() {
            line.push(index);
        }
    }

    let mut widths = vec![0.0; items.len()];
    let mut line_of = vec![0usize; items.len()];
    for (line_index, line) in lines.iter().enumerate() {
        let targets = resolve_flexible_lengths(&resolved, line, container_width);
        for (slot, &index) in line.iter().enumerate() {
            widths[index] = targets[slot];
            line_of[index] = line_index;
        }
    }

    TabsRowLayout { widths, lines: line_of }
}

/// O que os seletores da opacidade de `.tab > .tab-actions .action-label` testam: as classes da raiz, do grupo e do contêiner,
/// e o estado da aba (`:hover` e `:focus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabActionLabelContext {
    /// `.modern-ui-tabs` na raiz.
    pub modern_ui_tabs: bool,
    /// `.modern-ui-connected-editor-tabs` na raiz.
    pub connected_editor_tabs: bool,
    /// `.hc-black` ou `.hc-light` na raiz.
    pub high_contrast: bool,
    /// `.editor-group-container.active`.
    pub group_active: bool,
    /// `.title.tab-actions-reserve-space`.
    pub title_reserves_action_space: bool,
    pub active: bool,
    pub dirty: bool,
    pub sticky: bool,
    pub pinned_action_off: bool,
    pub close_action_off: bool,
    /// `.tab:hover`.
    pub hovered: bool,
    /// `.tab:focus-within`.
    pub focus_within: bool,
    /// `.action-label:focus`.
    pub label_focused: bool,
}

/// A opacidade computada do `.action-label` de uma aba: a cascata das regras de `multieditortabscontrol.css` (linhas 480 a
/// 525), de `tabs.css` (linhas 709 a 992) e de `connectedEditorTabs.css` (linhas 550 a 566), cada uma com a especificidade do
/// seletor (as classes e pseudoclasses; `:not(X)` e `:is(...)` contam como o argumento) e `!important` onde a regra o traz.
pub fn tab_action_label_opacity(context: &TabActionLabelContext) -> f64 {
    let c = context;
    let mut opacity = Cascade::<f64>::new();
    let sticky_with_pin_action = c.sticky && !c.pinned_action_off;

    // multieditortabscontrol.css, L480: opacidade 1 no grupo ativo
    if c.group_active {
        if c.active {
            opacity.declare(12, 1.0);
        }
        if c.label_focused {
            opacity.declare(12, 1.0);
        }
        if c.hovered {
            opacity.declare(12, 1.0);
        }
        if c.active && c.hovered {
            opacity.declare(13, 1.0);
        }
        if sticky_with_pin_action {
            opacity.declare(13, 1.0);
        }
        if c.dirty {
            opacity.declare(12, 1.0);
        }
    }

    // multieditortabscontrol.css, L515: a ação esmaecida (grupo inativo)
    if c.active {
        opacity.declare(11, 0.5);
    }
    if c.active && c.hovered {
        opacity.declare(12, 0.5);
    }
    if c.dirty {
        opacity.declare(11, 0.5);
    }
    if sticky_with_pin_action {
        opacity.declare(12, 0.5);
    }
    if c.hovered {
        opacity.declare(11, 0.5);
    }

    // multieditortabscontrol.css, L523
    opacity.declare(10, 0.0);

    if c.modern_ui_tabs {
        let at_rest = !c.active && !c.dirty && !c.sticky && !c.hovered;

        // tabs.css, L709 e L714
        if at_rest && !c.label_focused {
            opacity.declare(16, 0.0);
        }
        if c.dirty {
            opacity.declare(12, 1.0);
        }
        if sticky_with_pin_action {
            opacity.declare(13, 1.0);
        }
        if !c.close_action_off && c.hovered {
            opacity.declare(13, 1.0);
        }
        if c.label_focused {
            opacity.declare(12, 1.0);
        }

        if c.high_contrast {
            // tabs.css, L977 e L984 (`!important`)
            if !c.connected_editor_tabs && !c.title_reserves_action_space && at_rest && !c.label_focused {
                opacity.declare_important(17, 0.0);
            }
            if c.dirty || sticky_with_pin_action || c.hovered || c.label_focused {
                opacity.declare_important(17, 1.0);
            }

            // tabs.css, L988 e L992: a aba ativa conectada, por grupo ativo ou inativo
            if c.connected_editor_tabs && c.active && !c.close_action_off {
                opacity.declare(17, if c.group_active { 1.0 } else { 0.5 });
            }
        }

        // connectedEditorTabs.css, L551 e L560
        if c.connected_editor_tabs {
            if at_rest && !c.focus_within {
                opacity.declare(18, 0.0);
            }
            if c.active && !c.close_action_off {
                opacity.declare(15, 1.0);
            }
            if c.dirty {
                opacity.declare(14, 1.0);
            }
        }
    }

    opacity.value_or(0.0)
}

//#endregion

impl MultiEditorTabsControlState {
    /// `getFitTabWidths`.
    fn fit_tab_widths(&self, indexes: &[usize], measurer: &dyn TabsMeasurer) -> Vec<f64> {
        measurer.measure_fit_tabs(&self.tabs, indexes).iter().zip(indexes).map(|(metrics, &index)| get_fit_tab_width(&self.tabs[index].state, metrics)).collect()
    }

    /// `layoutFitTabsWithoutReservedActions`.
    pub fn layout_fit_tabs_without_reserved_actions(&mut self, measurer: &dyn TabsMeasurer) {
        if self.base.parent_style.tab_actions_reserve_space || !self.base.parent.in_modern_ui_tabs {
            for tab in &mut self.tabs {
                tab.state.fit_width = None;
            }
            return;
        }

        let mut indexes = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if tab.state.sizing_fit && tab.state.fit_width.is_none() {
                tab.state.tab_action_sizing_rest = !tab.state.dirty && !tab.state.sticky;
                indexes.push(index);
            }
        }

        let widths = self.fit_tab_widths(&indexes, measurer);
        for (index, width) in indexes.into_iter().zip(widths) {
            self.tabs[index].state.fit_width = Some(width);
            self.tabs[index].state.tab_action_sizing_rest = false;
        }
    }

    /// `freezeFitTabWidthWithoutReservedActions`.
    pub fn freeze_fit_tab_width_without_reserved_actions(&mut self, tab_index: usize, measurer: &dyn TabsMeasurer) {
        let Some(tab) = self.tabs.get(tab_index) else { return };
        if !tab.state.sizing_fit || tab.state.fit_width.is_some() || self.base.parent_style.tab_actions_reserve_space || !self.base.parent.in_modern_ui_tabs {
            return;
        }

        self.tabs[tab_index].state.tab_action_sizing_rest = !tab.state.dirty && !tab.state.sticky;
        let width = self.fit_tab_widths(&[tab_index], measurer)[0];
        self.tabs[tab_index].state.fit_width = Some(width);
        self.tabs[tab_index].state.tab_action_sizing_rest = false;
    }

    /// `tabsFitWrapped`: as abas (e o botão de nova aba) cabem quando quebradas em linhas?
    fn tabs_fit_wrapped(&self, measure: &WrappingMeasure, tabs_wrap_multi_line: bool, available_width: f64) -> bool {
        if self.tabs.is_empty() {
            return true; // no tab always fits
        }

        let editor_toolbar_width = measure.editor_toolbar_width;
        let last_item_is_last_tab = self.container.add_tab != AddTabPresence::Visible;
        let available_width = if tabs_wrap_multi_line { measure.visible_tabs_width.min(measure.tabs_and_actions_content_width).min(available_width) } else { available_width };
        let mut required_width = if last_item_is_last_tab { 0.0 } else { measure.add_tab_total_width + if tabs_wrap_multi_line { 0.0 } else { editor_toolbar_width } };
        let last_tab_index = self.tabs.len() - 1;
        for (index, tab) in self.tabs.iter().enumerate() {
            let Some(tab_measure) = measure.tabs.get(index) else { continue };
            let toolbar_reserve = if !tabs_wrap_multi_line && index == last_tab_index && last_item_is_last_tab { editor_toolbar_width } else { 0.0 };
            let margin = tab_measure.total_width - tab_measure.offset_width;
            let modern_fit = tab.state.sizing_fit && self.base.parent.in_modern_ui_tabs;
            let width = if modern_fit {
                tab_measure.offset_width.min(TabWidth::SHRINK.max(available_width - margin - toolbar_reserve))
            } else {
                tab_measure.offset_width
            };
            required_width = required_width.max(width + margin + toolbar_reserve);
        }

        let last_tab_overlap_with_toolbar_width = required_width - available_width;

        // Allow for slight rounding errors related to zooming here (https://github.com/microsoft/vscode/issues/116385)
        last_tab_overlap_with_toolbar_width <= 1.0
    }

    /// `updateTabsWrapping`.
    fn update_tabs_wrapping(&mut self, enabled: bool, measurer: &dyn TabsMeasurer) {
        // Toggle the `wrapped` class to enable wrapping
        self.container.wrapping = enabled;

        // Update `last-tab-margin-right` CSS variable to account for the absolute positioned editor actions container when
        // tabs wrap. The margin needs to be the width of the editor actions container to avoid screen cheese.
        self.container.last_tab_margin_right = if enabled { measurer.measure_wrapping(&self.container, &self.tabs).editor_toolbar_width } else { 0.0 };

        // Remove old css classes that are not needed anymore
        for tab in &mut self.tabs {
            tab.state.last_in_row = false;
        }
    }

    /// `doLayoutTabsWrapping`: decide se as abas quebram em linhas e marca a última de cada linha. Com `force` o resultado é
    /// imposto (usado pelo teste de largura). Devolve se quebram.
    pub fn layout_tabs_wrapping(&mut self, dimensions: EditorTitleControlDimensions, force_tabs_wrap_multi_line: Option<bool>, measurer: &dyn TabsMeasurer) -> bool {
        let available = dimensions.available.unwrap_or(Dimension { width: 0.0, height: 0.0 });

        // Handle wrapping tabs according to setting:
        // - enabled: only add class if tabs wrap and don't exceed available dimensions
        // - disabled: remove class and margin-right variable
        let did_tabs_wrap_multi_line = self.container.wrapping;
        let mut tabs_wrap_multi_line = did_tabs_wrap_multi_line;
        self.container.last_tab_add_tab_width = measurer.measure_wrapping(&self.container, &self.tabs).add_tab_offset_width;

        if let Some(enabled) = force_tabs_wrap_multi_line {
            self.update_tabs_wrapping(enabled, measurer);
            tabs_wrap_multi_line = enabled;
        }
        // Setting enabled: selectively enable wrapping if possible
        else if self.options().wrap_tabs {
            let measure = measurer.measure_wrapping(&self.container, &self.tabs);
            let visible_tabs_width = measure.visible_tabs_width;
            let all_tabs_width = measure.all_tabs_width;

            // If tabs wrap or should start to wrap (when width exceeds visible width) we must trigger `updateWrapping` to set
            // the `last-tab-margin-right` accordingly based on the number of actions. The margin is important to properly
            // position the last tab apart from the actions
            //
            // We already check here if the tabs would fit when wrapped given the editor toolbar will also show right next to
            // it. This ensures we are not enabling wrapping only to disable it again in the code below (this fixes flickering
            // issue https://github.com/microsoft/vscode/issues/115050)
            if tabs_wrap_multi_line || (all_tabs_width > visible_tabs_width && self.tabs_fit_wrapped(&measure, tabs_wrap_multi_line, available.width)) {
                self.update_tabs_wrapping(true, measurer);
                tabs_wrap_multi_line = true;
            }

            // Tabs wrap multiline: remove wrapping under certain size constraint conditions
            if tabs_wrap_multi_line {
                let measure = measurer.measure_wrapping(&self.container, &self.tabs);
                if measure.tabs_and_actions_height > available.height // if the complete wrapped title exceeds available height
                    || (all_tabs_width == visible_tabs_width && measure.tabs_container_height == self.base.tab_height()) // if wrapping is not needed anymore
                    || !self.tabs_fit_wrapped(&measure, true, available.width)
                // if a tab or the trailing controls do not fit anymore
                {
                    self.update_tabs_wrapping(false, measurer);
                    tabs_wrap_multi_line = false;
                }
            }
        }
        // Setting disabled: remove CSS traces only if tabs did wrap
        else if did_tabs_wrap_multi_line {
            self.update_tabs_wrapping(false, measurer);
            tabs_wrap_multi_line = false;
        }

        // If we transitioned from non-wrapping to wrapping, we need to update the scrollbar to have an equal `width` and
        // `scrollWidth`. Otherwise a scrollbar would appear which is never desired when wrapping.
        if tabs_wrap_multi_line && !did_tabs_wrap_multi_line {
            let visible_tabs_width = measurer.measure_wrapping(&self.container, &self.tabs).visible_tabs_width;
            self.set_scroll_dimensions(visible_tabs_width, visible_tabs_width);
        }

        // Update the `last-in-row` class on tabs when wrapping is enabled (it doesn't do any harm otherwise). This class
        // controls additional properties of tab when it is the last tab in a row
        if tabs_wrap_multi_line {
            let measure = measurer.measure_wrapping(&self.container, &self.tabs);
            let tops: Vec<f64> = measure.tabs.iter().map(|tab| tab.offset_top).collect();
            for (tab, last_in_row) in self.tabs.iter_mut().zip(compute_last_in_row(&tops)) {
                tab.state.last_in_row = last_in_row;
            }
        }

        tabs_wrap_multi_line
    }

    /// `tabsScrollbar.setScrollDimensions`: a posição de rolagem acompanha o limite novo (`Scrollable` a valida).
    fn set_scroll_dimensions(&mut self, width: f64, scroll_width: f64) {
        self.container.scroll_dimensions = (width, scroll_width);
        self.container.scroll_left = self.container.scroll_left.min(scroll_width - width).max(0.0);
    }

    /// `updateWrappedAddTabAlignment`.
    pub fn update_wrapped_add_tab_alignment(&mut self, tabs_wrap_multi_line: bool, measurer: &dyn TabsMeasurer) {
        if self.container.add_tab == AddTabPresence::None {
            return;
        }

        let measure = measurer.measure_add_tab_alignment(&self.container, &self.tabs);
        let (controls_only_row, left_inset) = compute_add_tab_alignment(tabs_wrap_multi_line, &measure);
        self.container.add_tab_controls_only_row = controls_only_row;
        self.container.add_tab_left_inset = left_inset;
    }

    /// `clearConnectedTabClipping`.
    pub fn clear_connected_tab_clipping(&mut self) {
        if let Some(bounds) = self.connected_tab_bounds.take()
            && let Some(tab) = self.tabs.get_mut(bounds.tab)
        {
            tab.state.connected_clipping = ConnectedTabClasses::default();
        }

        let edge = &mut self.container.overflow_edge;
        edge.top = None;
        edge.right = None;
        edge.bottom = None;
        edge.left = None;
        edge.left_clipped = false;
        edge.right_clipped = false;
        edge.hovered = false;
        edge.border = None;
        edge.overflow_top_border = None;
        edge.overflow_top_background = None;

        for tab in &mut self.tabs {
            tab.state.clip_path = None;
        }
    }

    /// `updateConnectedTabClipping(bounds, scrollLeft)` aplicado à aba e ao `overflowEdge`.
    fn apply_connected_clipping(&mut self, bounds: &ConnectedTabBounds, scroll_left: f64) {
        let clipping = connected_tab_clipping(bounds, scroll_left);
        if let Some(tab) = self.tabs.get_mut(bounds.tab) {
            tab.state.connected_clipping = clipping.tab;
        }
        self.container.overflow_edge.left_clipped = clipping.edge_left_clipped;
        self.container.overflow_edge.right_clipped = clipping.edge_right_clipped;
    }

    /// `updateConnectedTabClipping(scrollLeft)`.
    pub fn update_connected_tab_clipping(&mut self, scroll_left: f64, measurer: &dyn TabsMeasurer) {
        let measure = measurer.measure_connected_clipping(&self.container, &self.tabs, self.connected_tab_bounds.map(|bounds| bounds.tab));
        let add_tab_visible_width = measure.add_tab_offset_width.is_some_and(|width| width > 0.0);
        if add_tab_visible_width && !self.tabs.is_empty() && self.base.parent.in_connected_editor_tabs && !self.container.wrapping {
            let scale = if measure.tabs_offset_width > 0.0 { measure.tabs_width / measure.tabs_offset_width } else { 1.0 };
            let rtl = measure.right_to_left;
            let viewport_edge = if rtl { measure.add_tab_right + measure.shoulder_space * scale } else { measure.add_tab_left - measure.shoulder_space * scale };
            let clipping: Vec<f64> = measure
                .tab_bounds
                .iter()
                .map(|&(left, right)| ((if rtl { viewport_edge - left } else { right - viewport_edge }) / scale).max(0.0))
                .collect();
            if let Some(bounds) = self.connected_tab_bounds {
                let viewport_left = if rtl {
                    bounds.viewport_left.max(((if measure.active_tab_left < viewport_edge { viewport_edge } else { measure.add_tab_right }) - measure.tabs_left) / scale)
                } else {
                    bounds.viewport_left
                };
                let viewport_right = if rtl { measure.tabs_offset_width } else { (measure.add_tab_left - measure.tabs_left) / scale };
                self.container.overflow_edge.left = Some(viewport_left);
                self.container.overflow_edge.right = Some(measure.tabs_offset_width - viewport_right);
                self.apply_connected_clipping(&ConnectedTabBounds { viewport_left, viewport_right, ..bounds }, scroll_left);
            }

            for (tab, clipped) in self.tabs.iter_mut().zip(clipping) {
                tab.state.clip_path = (clipped > 0.0).then_some(TabClipPath { clipped, right_to_left: rtl });
            }
        } else {
            if let Some(bounds) = self.connected_tab_bounds {
                let viewport_right = if measure.add_tab_offset_width == Some(0.0) { measure.tabs_offset_width } else { bounds.viewport_right };
                self.container.overflow_edge.left = Some(bounds.viewport_left);
                self.container.overflow_edge.right = Some(measure.tabs_offset_width - viewport_right);
                self.apply_connected_clipping(&ConnectedTabBounds { viewport_right, ..bounds }, scroll_left);
            }

            for tab in &mut self.tabs {
                tab.state.clip_path = None;
            }
        }
    }

    /// `doLayoutTabsNonWrapping`: posiciona as abas fixadas, calcula o recorte da aba ativa conectada e revela a ativa.
    /// Devolve a nova posição da barra de rolagem, se ela mudou para revelar a aba.
    pub fn do_layout_tabs_non_wrapping(&mut self, options: Option<LayoutOptions>, measurer: &dyn TabsMeasurer) -> Option<f64> {
        //
        // Synopsis
        // - allTabsWidth:   sum of all tabs widths
        // - visibleTabsWidth: size of tabs container
        // - availableTabsContainerWidth: size of tabs container minus size of sticky tabs
        //
        // [only sticky tabs inside tabs container]
        //
        // | active-tab | sticky-tabs | sticky-tabs |
        //
        let metrics = measurer.measure_tabs(&self.container, &self.tabs);
        let visible_tabs_width = metrics.visible_tabs_width;
        let all_tabs_width = metrics.all_tabs_width;

        let pinned_tab_sizing = self.options().pinned_tab_sizing;
        let active_tab_index = self.base.tabs_model.active_editor().and_then(|editor| self.tab_index_of(&editor)).filter(|index| *index < self.tabs.len());
        let active_tab_sticky = active_tab_index.is_some_and(|index| self.base.tabs_model.is_sticky(&EditorOrIndex::Index(index as i64)));
        let sticky = layout_sticky_tabs(
            &metrics,
            self.base.tabs_model.sticky_count(),
            pinned_tab_sizing,
            self.base.parent.in_modern_ui_tabs,
            self.connected_tab_labels,
            active_tab_index,
            active_tab_sticky,
        );
        for (tab_index, left) in sticky.sticky_lefts.iter().enumerate() {
            if let Some(tab) = self.tabs.get_mut(tab_index) {
                tab.state.left = Some(*left);
            }
        }
        self.container.disable_sticky_tabs = sticky.disable_sticky_tabs;
        self.container.sticky_tabs_background_width = sticky.sticky_tabs_width;
        self.container.overflow_edge.adjacent_sticky = sticky.sticky_tabs_width > 0.0;

        self.clear_connected_tab_clipping();
        self.container.overflow_edge.top_row = active_tab_index.is_some_and(|index| self.tabs[index].state.connected_tab_top_row);
        if let Some(active_index) = active_tab_index
            && !self.tabs[active_index].state.connected_tab_upper_row
            && !sticky.active_tab_position_static
            && self.base.parent.in_connected_editor_tabs
            && let Some(measure) = measurer.measure_connected_active_tab(&self.container, &self.tabs, active_index)
        {
            let edge = &mut self.container.overflow_edge;
            edge.border = Some(measure.tab_border.clone());
            edge.overflow_top_border = Some(measure.overflow_top_border.clone());
            edge.overflow_top_background = Some(measure.overflow_top_background.clone());

            // DOM rectangles include CSS zoom; scroll offsets and viewport widths do not.
            let scale = if visible_tabs_width > 0.0 { measure.tabs_width / visible_tabs_width } else { 1.0 };
            let fill_left = (measure.fill_left - measure.tabs_left) / scale + measure.container_scroll_left;
            let viewport_right = sticky.visible_editor_tabs_width;
            edge.top = Some((measure.fill_top - measure.scrollable_top) / scale);
            edge.right = Some(visible_tabs_width - viewport_right);
            edge.bottom = Some((measure.scrollable_bottom - measure.fill_bottom) / scale);
            edge.left = Some(sticky.sticky_tabs_width);
            self.connected_tab_bounds = Some(ConnectedTabBounds {
                tab: active_index,
                fill_left,
                fill_right: (measure.fill_right - measure.tabs_left) / scale + measure.container_scroll_left,
                viewport_left: sticky.sticky_tabs_width,
                viewport_right,
                shoulder_extent: measure.shoulder_extent,
            });
            self.update_connected_tab_overflow_hover();
        }

        let active = if self.block_reveal_active_tab { None } else { metrics.active_tab };

        // Update scrollbar
        let (old_visible_tabs_width, old_all_tabs_width) = self.container.scroll_dimensions;
        self.set_scroll_dimensions(visible_tabs_width, all_tabs_width);
        self.update_connected_tab_clipping(self.container.scroll_left, measurer);
        let dimensions_changed = old_visible_tabs_width != visible_tabs_width || old_all_tabs_width != all_tabs_width;

        let connected_reveal = self.connected_tab_bounds.map(|bounds| ConnectedReveal {
            fill_left: bounds.fill_left,
            fill_right: bounds.fill_right,
            shoulder_extent: bounds.shoulder_extent,
            has_previous_tab_in_row: bounds.tab.checked_sub(1).is_some_and(|previous| !self.tabs[previous].state.last_in_row),
            tab_connected_last: self.tabs[bounds.tab].state.connected_tab_last,
        });
        let reveal = reveal_active_tab_scroll(
            options,
            active,
            sticky.sticky_tabs_width,
            sticky.available_tabs_container_width,
            sticky.active_tab_position_static,
            self.block_reveal_active_tab,
            dimensions_changed,
            self.container.scroll_left,
            connected_reveal,
        );

        // Whether revealing or skipping, the one-time block is consumed
        self.block_reveal_active_tab = false;

        if let Some(scroll_left) = reveal {
            self.container.scroll_left = scroll_left;
        }

        reveal
    }

    /// `doLayoutTabs`: marca as abas (última, linha de cima, linha do topo), mede as abas `fit`, decide a quebra de linha e
    /// segue para o layout sem quebra ou limpa o recorte conectado.
    pub fn layout_tabs(&mut self, dimensions: EditorTitleControlDimensions, options: Option<LayoutOptions>, measurer: &dyn TabsMeasurer) {
        let connected = self.base.parent.in_connected_editor_tabs;
        let bars = self.container.bars;
        let top_tab_bar = bars.top_tab_bar;
        let upper_tab_bar = bars.two_tab_bars && top_tab_bar;
        let count = self.base.tabs_model.count();
        self.container.connected_tab_upper_bar = connected && upper_tab_bar;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            tab.state.last_tab = index + 1 == count;
            tab.state.connected_tab_last = connected && index + 1 == count;
        }
        self.container.has_add_tab = self.container.add_tab == AddTabPresence::Visible;
        if connected && upper_tab_bar {
            for tab in &mut self.tabs {
                tab.state.connected_tab_upper_row = true;
            }
        }
        self.container.connected_tabs_labels = connected;
        if connected != self.connected_tab_labels {
            self.connected_tab_labels = connected;
            for index in 0..self.tabs.len() {
                self.redraw_tab_label_at(index);
            }
        }
        self.layout_fit_tabs_without_reserved_actions(measurer);

        // Always first layout tabs with wrapping support even if wrapping is disabled. The result indicates if tabs wrap and
        // if not, we need to proceed with the layout without wrapping because even if wrapping is enabled in settings, there
        // are cases where wrapping is disabled (e.g. due to space constraints)
        let tabs_wrap_multi_line = self.layout_tabs_wrapping(dimensions, None, measurer);
        let measure = measurer.measure_wrapping(&self.container, &self.tabs);
        let top = measure.tabs.first().map(|tab| tab.offset_top);
        let bottom = if tabs_wrap_multi_line && self.container.add_tab == AddTabPresence::Visible { Some(measure.add_tab_offset_top) } else { measure.tabs.last().map(|tab| tab.offset_top) };
        for (tab, tab_measure) in self.tabs.iter_mut().zip(&measure.tabs) {
            tab.state.connected_tab_upper_row = connected && (upper_tab_bar || Some(tab_measure.offset_top) != bottom);
            tab.state.connected_tab_top_row = connected && top_tab_bar && Some(tab_measure.offset_top) == top;
        }
        self.update_wrapped_add_tab_alignment(tabs_wrap_multi_line, measurer);
        self.container.connected_tabs_wrapping = connected && (bars.other_bar_wrapping || self.container.wrapping);
        self.container.overflow_visible = connected && tabs_wrap_multi_line;
        if !tabs_wrap_multi_line {
            self.do_layout_tabs_non_wrapping(options, measurer);
        } else {
            self.container.sticky_tabs_background_width = 0.0;
            self.clear_connected_tab_clipping();
        }
    }

    /// `doLayout`: desenha as abas e guarda a dimensão usada. Devolve se a altura mudou, e então o chamador precisa chamar
    /// `groupView.relayout()` para o controle do editor se ajustar.
    pub fn do_layout(&mut self, dimensions: EditorTitleControlDimensions, options: Option<LayoutOptions>, measurer: &dyn TabsMeasurer) -> bool {
        // Layout tabs
        if dimensions.container.is_some() && dimensions.available.is_some() {
            self.layout_tabs(dimensions, options, measurer);
        }

        // Remember the dimensions used in the control so that we can return it fast from the `layout` call without having to
        // compute it over and over again
        let wrapping_or_connected = (self.options().wrap_tabs && self.container.wrapping) || self.base.parent.in_connected_editor_tabs;
        let measured_height = wrapping_or_connected.then(|| measurer.measure_wrapping(&self.container, &self.tabs).tabs_and_actions_height);
        let height = self.compute_height(measured_height);

        self.finish_layout(dimensions.container.map_or(0.0, |container| container.width), height)
    }
}


