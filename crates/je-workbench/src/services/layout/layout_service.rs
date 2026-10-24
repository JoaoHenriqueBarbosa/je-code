//! Porte de `src/vs/workbench/services/layout/browser/layoutService.ts`: as partes do workbench ([`Parts`]),
//! as posições ([`Position`]), as configurações de layout, as margens dos painéis flutuantes (Modern UI) e a
//! interface do serviço de layout ([`IWorkbenchLayoutService`]).
//!
//! Divergências:
//! - **Enums de texto** (`const enum Parts`, `ZenModeSettings`, `LayoutSettings`, `ModernUIEditorTabStyle`,
//!   `ModernUIDensity`, `ActivityBarPosition`, `EditorTabsMode`, `EditorActionsLocation` e o tipo
//!   `PanelAlignment`) são enums Rust com os métodos `as_str` (o valor do original, por exemplo
//!   `Parts::TitlebarPart.as_str() == "workbench.parts.titlebar"`) e `from_value` (o caminho inverso, `None`
//!   para um texto desconhecido). Os nomes das variantes são o nome do original em `PascalCase`
//!   (`TITLEBAR_PART` vira `TitlebarPart`, `MODERN_UI_FROSTED_GLASS` vira `ModernUiFrostedGlass`).
//! - `ModernUIFrostedGlassOpacity` (numérico) é [`ModernUiFrostedGlassOpacity`], com os mesmos valores.
//! - **`Window`** é o identificador [`WindowId`] (o `vscodeWindowId` da janela), e a janela principal é
//!   [`MAIN_WINDOW`] (o `mainWindow` do original). **`HTMLElement`** é o identificador opaco
//!   [`LayoutContainerId`]. **`IDimension`** é o `Dimension` do `je-editor` e **`IViewSize`** é o
//!   `ViewDimensions` do grid do `je-widgets`.
//! - **Sobrecargas** do `IWorkbenchLayoutService` (`focusPart`, `getContainer`, `isVisible`) são um método
//!   com o `Window` opcional: `None` é a forma de uma parte de janela única, `Some(janela)` a de uma parte de
//!   várias janelas. As partes de várias janelas são [`MULTI_WINDOW_PARTS`] ([`is_multi_window_part`]); o tipo
//!   `SINGLE_WINDOW_PARTS` (o `Exclude<Parts, MULTI_WINDOW_PARTS>`) não tem equivalente em Rust e é o que
//!   [`is_multi_window_part`] nega. A sobrecarga `getContainer(window, part)` é
//!   [`IWorkbenchLayoutService::get_container_of_part`], para não colidir com o `getContainer(window)` do
//!   [`ILayoutService`].
//! - **`ILayoutService`** (`platform/layout/browser/layoutService.ts`, que o `IWorkbenchLayoutService`
//!   estende) ainda não tem porte próprio, então o trait [`ILayoutService`] mora aqui, com os mesmos membros,
//!   e sai daqui quando a camada `platform/layout` for portada.
//! - **`registerPart(part: Part)`**: `Part` (`browser/part.ts`) é um widget do DOM, ainda não portado. O
//!   layout só lê dele o `getId()`, e o trait [`LayoutPart`] é isso.
//! - **`shouldShowCustomTitleBar`** lê configuração (por [`IConfigurationService`], como o original) e
//!   também `platform/window/common/window.ts` (`hasCustomTitlebar`, `hasNativeTitlebar`, `hasNativeMenu`,
//!   `getMenuBarVisibility`), `base/browser/browser.ts` (`isFullscreen`, `isWCOEnabled`), `isAuxiliaryWindow` e
//!   as constantes de `platform.ts` (`isWeb`, `isMacintosh`, `isNative`), nada disso portado: o trait
//!   [`TitleBarHost`] é o que o original pergunta a esses módulos.
//! - `positionToString` e `partOpensMaximizedSettingToString` têm o ramo `default` do `switch` no original;
//!   o `match` de Rust é exaustivo e não precisa dele. `positionFromString` e `partOpensMaximizedFromString`
//!   devolvem `Option` (o `undefined` do original para um texto desconhecido).
//! - O resultado `{ left, right }` e `{ top, bottom }` das funções de margem e borda é [`HorizontalPair`] e
//!   [`VerticalPair`]; os números do JavaScript são `f64`.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::promise::Promise;
use je_editor::core_2d::dimension::Dimension;
use je_platform::configuration::service::IConfigurationService;
use je_widgets::grid::Direction;
use je_widgets::gridview::ViewDimensions;

/// Define um enum cujas variantes têm um valor de texto no original (`const enum X { A = 'a' }`).
macro_rules! string_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($(#[$variant_meta:meta])* $variant:ident = $value:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        $vis enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            /// O valor de texto do original.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }

            /// O caminho inverso de `as_str`: `None` para um texto que não é de nenhum valor.
            pub fn from_value(value: &str) -> Option<Self> {
                match value {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

string_enum! {
    /// `Parts`.
    pub enum Parts {
        TitlebarPart = "workbench.parts.titlebar",
        BannerPart = "workbench.parts.banner",
        ActivitybarPart = "workbench.parts.activitybar",
        SidebarPart = "workbench.parts.sidebar",
        PanelPart = "workbench.parts.panel",
        AuxiliarybarPart = "workbench.parts.auxiliarybar",
        SessionsPart = "workbench.parts.sessions",
        CustomViewGridPart = "workbench.parts.customViewGrid",
        EditorPart = "workbench.parts.editor",
        StatusbarPart = "workbench.parts.statusbar",
    }
}

string_enum! {
    /// `ZenModeSettings`.
    pub enum ZenModeSettings {
        ShowTabs = "zenMode.showTabs",
        HideLinenumbers = "zenMode.hideLineNumbers",
        HideStatusbar = "zenMode.hideStatusBar",
        HideActivitybar = "zenMode.hideActivityBar",
        CenterLayout = "zenMode.centerLayout",
        Fullscreen = "zenMode.fullScreen",
        Restore = "zenMode.restore",
        SilentNotifications = "zenMode.silentNotifications",
    }
}

string_enum! {
    /// `LayoutSettings`.
    pub enum LayoutSettings {
        ActivityBarLocation = "workbench.activityBar.location",
        ActivityBarAutoHide = "workbench.activityBar.autoHide",
        ActivityBarCompact = "workbench.activityBar.compact",
        EditorTabsMode = "workbench.editor.showTabs",
        EditorActionsLocation = "workbench.editor.editorActionsLocation",
        CommandCenter = "window.commandCenter",
        LayoutActions = "workbench.layoutControl.enabled",
        Shadows = "workbench.shadows",
        ModernUi = "workbench.experimental.modernUI",
        ModernUiDensity = "window.density.layout",
        ModernUiFrostedGlass = "workbench.modernUIFrostedGlass",
        ModernUiFrostedGlassOpacity = "workbench.modernUIFrostedGlassOpacity",
        ModernUiEditorTabStyle = "workbench.experimental.modernUIEditorTabStyle",
        ModernUiUppercaseViewHeaders = "workbench.experimental.modernUIUppercaseViewHeaders",
    }
}

/// `ModernUIFrostedGlassOpacity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModernUiFrostedGlassOpacity {
    Minimum = 50,
    Maximum = 100,
    Default = 80,
}

string_enum! {
    /// `ModernUIEditorTabStyle`.
    pub enum ModernUiEditorTabStyle {
        Connected = "connected",
        Pill = "pill",
    }
}

string_enum! {
    /// `ModernUIDensity`.
    pub enum ModernUiDensity {
        Default = "default",
        Compact = "compact",
    }
}

/// The margin (in pixels) reserved on each side of a part when the Modern UI Update
/// experiment (`LayoutSettings.MODERN_UI`) is enabled. Parts grow or shrink their
/// content by this amount to leave room for the margin/border applied in CSS
/// (`src/vs/workbench/browser/media/floatingPanels.css`, `.floating-panels`).
/// Keep in sync with the `--vscode-spacing-size40` (4px) token used there.
pub const FLOATING_PANEL_MARGIN: f64 = 4.0;

/// Compact Modern UI density removes the gaps between cards.
pub const COMPACT_FLOATING_PANEL_MARGIN: f64 = 0.0;

/// Compact Modern UI density connects the card cluster directly to the window chrome.
pub const COMPACT_FLOATING_PANEL_OUTER_MARGIN: f64 = 0.0;

/// The trailing card margin (in pixels) when the Modern UI Update experiment is
/// enabled. Together with the next card's leading [`FLOATING_PANEL_MARGIN`],
/// it forms the 4px inter-card gap. Keep in sync with the
/// `--vscode-spacing-sizeNone` (0px) token used in `floatingPanels.css`.
pub const FLOATING_PANEL_INNER_MARGIN: f64 = 0.0;

/// `getFloatingPanelMargin(layoutService)`.
pub fn get_floating_panel_margin(layout_service: &dyn IWorkbenchLayoutService) -> f64 {
    if layout_service.is_modern_ui_compact() { COMPACT_FLOATING_PANEL_MARGIN } else { FLOATING_PANEL_MARGIN }
}

/// The density-specific perimeter gutter (in pixels) on an edge facing window chrome. Keep in sync with
/// `--modern-ui-floating-card-outer-margin` in `floatingPanels.css`.
pub fn get_floating_panel_outer_margin(layout_service: &dyn IWorkbenchLayoutService) -> f64 {
    if layout_service.is_modern_ui_compact() { COMPACT_FLOATING_PANEL_OUTER_MARGIN } else { FLOATING_PANEL_MARGIN }
}

string_enum! {
    /// `ActivityBarPosition`.
    pub enum ActivityBarPosition {
        Default = "default",
        Top = "top",
        Bottom = "bottom",
        Hidden = "hidden",
    }
}

string_enum! {
    /// `EditorTabsMode`.
    pub enum EditorTabsMode {
        Multiple = "multiple",
        Single = "single",
        None = "none",
    }
}

string_enum! {
    /// `EditorActionsLocation`.
    pub enum EditorActionsLocation {
        Default = "default",
        Titlebar = "titleBar",
        Hidden = "hidden",
    }
}

/// `Position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Position {
    Left,
    Right,
    Bottom,
    Top,
}

impl Position {
    /// Todas as posições, na ordem do `const enum`.
    pub const ALL: [Position; 4] = [Position::Left, Position::Right, Position::Bottom, Position::Top];
}

/// `isHorizontal(position)`.
pub fn is_horizontal(position: Position) -> bool {
    position == Position::Bottom || position == Position::Top
}

/// `PartOpensMaximizedOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartOpensMaximizedOptions {
    Always,
    Never,
    RememberLast,
}

impl PartOpensMaximizedOptions {
    /// Todas as opções, na ordem do `const enum`.
    pub const ALL: [PartOpensMaximizedOptions; 3] = [Self::Always, Self::Never, Self::RememberLast];
}

string_enum! {
    /// `PanelAlignment`: `'left' | 'center' | 'right' | 'justify'`.
    pub enum PanelAlignment {
        Left = "left",
        Center = "center",
        Right = "right",
        Justify = "justify",
    }
}

/// `positionToString(position)`.
pub fn position_to_string(position: Position) -> &'static str {
    match position {
        Position::Left => "left",
        Position::Right => "right",
        Position::Bottom => "bottom",
        Position::Top => "top",
    }
}

/// `positionFromString(str)` (o `positionsByString` do original, montado a partir de `positionToString`).
pub fn position_from_string(str: &str) -> Option<Position> {
    Position::ALL.into_iter().find(|position| position_to_string(*position) == str)
}

/// `partOpensMaximizedSettingToString(setting)`.
fn part_opens_maximized_setting_to_string(setting: PartOpensMaximizedOptions) -> &'static str {
    match setting {
        PartOpensMaximizedOptions::Always => "always",
        PartOpensMaximizedOptions::Never => "never",
        PartOpensMaximizedOptions::RememberLast => "preserve",
    }
}

/// `partOpensMaximizedFromString(str)` (o `partOpensMaximizedByString` do original).
pub fn part_opens_maximized_from_string(str: &str) -> Option<PartOpensMaximizedOptions> {
    PartOpensMaximizedOptions::ALL.into_iter().find(|setting| part_opens_maximized_setting_to_string(*setting) == str)
}

/// `MULTI_WINDOW_PARTS`: as partes que existem em cada janela.
pub const MULTI_WINDOW_PARTS: [Parts; 3] = [Parts::EditorPart, Parts::StatusbarPart, Parts::TitlebarPart];

/// `isMultiWindowPart(part)`.
pub fn is_multi_window_part(part: Parts) -> bool {
    MULTI_WINDOW_PARTS.contains(&part)
}

/// A janela do original (`Window`): o `vscodeWindowId` dela.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(pub u32);

/// `mainWindow`.
pub const MAIN_WINDOW: WindowId = WindowId(1);

/// O `HTMLElement` de um container (o da janela ou o de uma parte), como identificador opaco: quem desenha o
/// workbench o atribui e sabe a que nó da árvore de widgets ele corresponde.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayoutContainerId(pub u64);

/// O par `{ left, right }` do original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HorizontalPair<T> {
    pub left: T,
    pub right: T,
}

/// O par `{ top, bottom }` do original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerticalPair<T> {
    pub top: T,
    pub bottom: T,
}

/// O `{ sideBar: boolean; auxBar: boolean }` de [`get_floating_sidebar_sibling_to_editor_status`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SidebarSiblingToEditorStatus {
    pub side_bar: bool,
    pub aux_bar: bool,
}

/// Whether the floating cards sit against the top window edge and take the density-specific
/// outer gutter. Both grid rows above the middle section (title bar and banner) must be hidden.
pub fn is_floating_top_edge_exposed(layout_service: &dyn IWorkbenchLayoutService, target_window: WindowId) -> bool {
    !layout_service.is_visible(Parts::TitlebarPart, Some(target_window)) && !layout_service.is_visible(Parts::BannerPart, None)
}

/// Determines which horizontal edge of the floating-card cluster is owned by its outermost
/// card when the Modern UI Update experiment is enabled, and which [`Parts`] owns it.
/// The owning part receives the density-specific outer gutter. Compact density includes the
/// default-position Activity Bar in the connected card cluster.
///
/// The horizontal order of the parts is reconstructed from the same inputs the grid
/// layout uses (mirrors `Layout.adjustPartPositions` in `src/vs/workbench/browser/layout.ts`): the activity bar and
/// primary side bar sit on `getSideBarPosition()`, the secondary side bar on the opposite side, the editor in
/// the middle, and a vertical (left/right) panel immediately next to the editor on its
/// placement side. The outermost *visible* part on each edge wins; a visible Activity Bar is the
/// outermost card of the side bar cluster. A hidden editor is skipped, so a maximized side bar
/// (which spans the full content width) is correctly detected as the owner on both edges.
///
/// Consumed by `AbstractPaneCompositePart` (side bars and panel) and `EditorPart`
/// (main editor) so the outer-gutter decision stays in sync between them.
pub fn get_floating_outer_edge_owners(layout_service: &dyn IWorkbenchLayoutService) -> HorizontalPair<Option<Parts>> {
    if !layout_service.is_floating_panels_enabled() {
        return HorizontalPair { left: None, right: None };
    }

    let side_bar_left = layout_service.get_side_bar_position() == Position::Left;
    let panel_position = layout_service.get_panel_position();
    let vertical_panel_visible = !is_horizontal(panel_position) && layout_service.is_visible(Parts::PanelPart, None);

    // A visible vertical panel sits immediately outside the editor on its placement
    // side (between the editor and the side/aux bar on that side).
    let panel_in_left_sequence = vertical_panel_visible && panel_position == Position::Left;
    let panel_in_right_sequence = vertical_panel_visible && panel_position == Position::Right;

    // The full window order of the floatable parts, left -> right: the activity bar and
    // primary side bar sit together on `getSideBarPosition()` (activity bar outermost), the
    // secondary side bar on the opposite side, a vertical panel immediately beside the editor
    // on its placement side, and the editor in the middle. Each edge is resolved by walking
    // this order inward to the first *visible* card, so a hidden editor (e.g. a maximized side
    // bar that spans the full content width) is skipped and the spanning card is detected on
    // both edges.
    let side_bar_group = [Parts::ActivitybarPart, Parts::SidebarPart];
    let panel_group = [Parts::PanelPart];
    let panel_when = |in_sequence: bool| -> Vec<Parts> { if in_sequence { panel_group.to_vec() } else { Vec::new() } };
    let mut full_order: Vec<Parts> = Vec::new();
    if side_bar_left {
        full_order.extend(side_bar_group);
        full_order.extend(panel_when(panel_in_left_sequence));
        full_order.push(Parts::EditorPart);
        full_order.extend(panel_when(panel_in_right_sequence));
        full_order.push(Parts::AuxiliarybarPart);
    } else {
        full_order.push(Parts::AuxiliarybarPart);
        full_order.extend(panel_when(panel_in_left_sequence));
        full_order.push(Parts::EditorPart);
        full_order.extend(panel_when(panel_in_right_sequence));
        full_order.extend(side_bar_group.into_iter().rev()); // activity bar is outermost on the right edge
    }

    let reversed_order: Vec<Parts> = full_order.iter().rev().copied().collect();
    HorizontalPair {
        left: resolve_floating_outer_owner(layout_service, &full_order),
        right: resolve_floating_outer_owner(layout_service, &reversed_order),
    }
}

/// Walks the given window order (outermost -> innermost) and returns the first visible card
/// owning the cluster edge. A visible default-position Activity Bar is a cluster card in both
/// densities: it is the outermost card of the side bar cluster and carries the outer gutter.
fn resolve_floating_outer_owner(layout_service: &dyn IWorkbenchLayoutService, ordered_parts: &[Parts]) -> Option<Parts> {
    ordered_parts.iter().copied().find(|part| {
        // The editor is the only multi-window part in this order; its main-window visibility
        // is what matters for the main-window floating layout.
        if *part == Parts::EditorPart { layout_service.is_visible(Parts::EditorPart, Some(MAIN_WINDOW)) } else { layout_service.is_visible(*part, None) }
    })
}

/// The horizontal cluster edges on which the given part is the outermost floating card and
/// should therefore receive the density-specific outer gutter. A part can own both edges (notably
/// a horizontal bottom/top panel that spans the full width when the bars beside it are
/// hidden or not full-height). Convenience wrapper around [`get_floating_outer_edge_owners`].
pub fn get_floating_outer_gutter_edges(layout_service: &dyn IWorkbenchLayoutService, part_id: Parts) -> HorizontalPair<bool> {
    if !layout_service.is_floating_panels_enabled() {
        return HorizontalPair { left: false, right: false };
    }

    // A horizontal (bottom/top) panel can reach both window edges simultaneously, so it
    // is not captured by the single-owner-per-edge model and is resolved separately.
    if part_id == Parts::PanelPart && is_horizontal(layout_service.get_panel_position()) {
        return get_floating_horizontal_panel_outer_edges(layout_service);
    }

    let owners = get_floating_outer_edge_owners(layout_service);
    HorizontalPair { left: owners.left == Some(part_id), right: owners.right == Some(part_id) }
}

/// Horizontal margins (in pixels) a floating pane composite reserves, mirroring the
/// margins in `floatingPanels.css`.
pub fn get_floating_pane_composite_horizontal_margins(layout_service: &dyn IWorkbenchLayoutService, part_id: Parts) -> HorizontalPair<f64> {
    if !layout_service.is_floating_panels_enabled() {
        return HorizontalPair { left: 0.0, right: 0.0 };
    }

    let outer_gutter = get_floating_outer_gutter_edges(layout_service, part_id);
    let margin = get_floating_panel_margin(layout_service);
    let outer_margin = get_floating_panel_outer_margin(layout_service);

    // The primary side bar meets the activity bar rail flush so the two read as one connected
    // surface. Only a left-positioned side bar needs this explicitly; on the right the seam is
    // the side bar's trailing edge, which already uses the inner margin.
    let meets_activity_bar_rail = part_id == Parts::SidebarPart
        && layout_service.get_side_bar_position() == Position::Left
        && layout_service.is_visible(Parts::ActivitybarPart, None);
    let leading = if meets_activity_bar_rail { FLOATING_PANEL_INNER_MARGIN } else { margin };

    let panel_meets_right_activity_bar = part_id == Parts::PanelPart
        && is_horizontal(layout_service.get_panel_position())
        && layout_service.get_side_bar_position() == Position::Right
        && layout_service.is_visible(Parts::ActivitybarPart, None)
        && layout_service.is_visible(Parts::SidebarPart, None)
        && get_floating_sidebar_sibling_to_editor_status(layout_service).side_bar;
    let trailing = if panel_meets_right_activity_bar { margin } else { FLOATING_PANEL_INNER_MARGIN };

    HorizontalPair {
        left: if outer_gutter.left { outer_margin } else { leading },
        right: if outer_gutter.right { outer_margin } else { trailing },
    }
}

/// Whether the primary sidebar and auxiliary bar are each in the same grid row as the
/// editor (sibling to the editor) for a horizontal panel. A bar that is a sibling is not
/// full-height; it sits above or below the panel row rather than spanning the full height.
/// Mirrors the sideBarSiblingToEditor / auxiliaryBarSiblingToEditor formula used in
/// adjustPartPositions() in layout.ts.
pub fn get_floating_sidebar_sibling_to_editor_status(layout_service: &dyn IWorkbenchLayoutService) -> SidebarSiblingToEditorStatus {
    let alignment = layout_service.get_panel_alignment();
    let side_bar_on_left = layout_service.get_side_bar_position() == Position::Left;
    SidebarSiblingToEditorStatus {
        side_bar: !(alignment == PanelAlignment::Center
            || (side_bar_on_left && alignment == PanelAlignment::Right)
            || (!side_bar_on_left && alignment == PanelAlignment::Left)),
        aux_bar: !(alignment == PanelAlignment::Center
            || (!side_bar_on_left && alignment == PanelAlignment::Right)
            || (side_bar_on_left && alignment == PanelAlignment::Left)),
    }
}

/// Vertical margins (in pixels) a floating pane composite (primary side bar, secondary side
/// bar or panel) reserves, mirroring the margins in `floatingPanels.css`. Compact density
/// uses the perimeter gutter whenever the edge faces window chrome rather than another card.
pub fn get_floating_pane_composite_vertical_margins(layout_service: &dyn IWorkbenchLayoutService, part_id: Parts, target_window: WindowId) -> VerticalPair<f64> {
    if !layout_service.is_floating_panels_enabled() {
        return VerticalPair { top: 0.0, bottom: 0.0 };
    }

    let outer_edges = get_floating_pane_composite_vertical_outer_edges(layout_service, part_id, target_window);
    let status_bar_visible = layout_service.is_visible(Parts::StatusbarPart, Some(target_window));
    let margin = get_floating_panel_margin(layout_service);
    let outer_margin = get_floating_panel_outer_margin(layout_service);

    VerticalPair {
        top: if outer_edges.top {
            if is_floating_top_edge_exposed(layout_service, target_window) { outer_margin } else { FLOATING_PANEL_INNER_MARGIN }
        } else {
            margin
        },
        bottom: if outer_edges.bottom {
            if status_bar_visible { margin } else { outer_margin }
        } else {
            FLOATING_PANEL_INNER_MARGIN
        },
    }
}

/// `getFloatingPaneCompositeVerticalOuterEdges(layoutService, partId, targetWindow)`.
pub fn get_floating_pane_composite_vertical_outer_edges(layout_service: &dyn IWorkbenchLayoutService, part_id: Parts, target_window: WindowId) -> VerticalPair<bool> {
    if !layout_service.is_floating_panels_enabled() {
        return VerticalPair { top: false, bottom: false };
    }

    let panel_position = layout_service.get_panel_position();
    let panel_visible = layout_service.is_visible(Parts::PanelPart, None);
    let is_side_bar = part_id == Parts::SidebarPart || part_id == Parts::AuxiliarybarPart;
    let sibling_status = get_floating_sidebar_sibling_to_editor_status(layout_service);
    let is_sibling_to_editor = if part_id == Parts::SidebarPart { sibling_status.side_bar } else { sibling_status.aux_bar };
    let editor_visible = layout_service.is_visible(Parts::EditorPart, Some(target_window));
    let faces_panel_above = panel_visible && panel_position == Position::Top && is_side_bar && is_sibling_to_editor;
    let faces_editor_above = part_id == Parts::PanelPart && panel_position == Position::Bottom && editor_visible;
    let faces_editor_below = part_id == Parts::PanelPart && panel_position == Position::Top && editor_visible;
    let faces_panel_below = panel_visible && panel_position == Position::Bottom && is_side_bar && is_sibling_to_editor;

    VerticalPair { top: !faces_panel_above && !faces_editor_above, bottom: !faces_editor_below && !faces_panel_below }
}

/// Vertical margins (in pixels) the floating main editor reserves, mirroring the margins in
/// `floatingPanels.css`. A panel above or below takes the place of the corresponding window edge.
pub fn get_floating_editor_vertical_margins(layout_service: &dyn IWorkbenchLayoutService, target_window: WindowId) -> VerticalPair<f64> {
    if !layout_service.is_floating_panels_enabled() {
        return VerticalPair { top: 0.0, bottom: 0.0 };
    }

    let outer_edges = get_floating_editor_vertical_outer_edges(layout_service);
    let margin = get_floating_panel_margin(layout_service);
    let outer_margin = get_floating_panel_outer_margin(layout_service);

    VerticalPair {
        top: if outer_edges.top {
            if is_floating_top_edge_exposed(layout_service, target_window) { outer_margin } else { FLOATING_PANEL_INNER_MARGIN }
        } else {
            margin
        },
        bottom: if outer_edges.bottom {
            if layout_service.is_visible(Parts::StatusbarPart, Some(target_window)) { margin } else { outer_margin }
        } else {
            FLOATING_PANEL_INNER_MARGIN
        },
    }
}

/// `getFloatingEditorVerticalOuterEdges(layoutService)`.
pub fn get_floating_editor_vertical_outer_edges(layout_service: &dyn IWorkbenchLayoutService) -> VerticalPair<bool> {
    if !layout_service.is_floating_panels_enabled() {
        return VerticalPair { top: false, bottom: false };
    }

    let panel_visible = layout_service.is_visible(Parts::PanelPart, None);
    let panel_position = layout_service.get_panel_position();
    VerticalPair { top: !(panel_visible && panel_position == Position::Top), bottom: !(panel_visible && panel_position == Position::Bottom) }
}

/// Whether a visible horizontal (bottom/top) panel reaches each horizontal cluster edge and
/// should therefore receive the density-specific outer gutter so it aligns with the editor above.
/// The panel spans underneath a bar that is not full-height, and reaches an edge whenever
/// the bar on that side is hidden or not full-height (and, on the side bar side, the
/// activity bar is absent). The full-height/sibling computation mirrors `Layout.adjustPartPositions`.
fn get_floating_horizontal_panel_outer_edges(layout_service: &dyn IWorkbenchLayoutService) -> HorizontalPair<bool> {
    if !layout_service.is_visible(Parts::PanelPart, None) {
        return HorizontalPair { left: false, right: false };
    }

    let side_bar_left = layout_service.get_side_bar_position() == Position::Left;
    let SidebarSiblingToEditorStatus { side_bar: side_bar_sibling_to_editor, aux_bar: aux_sibling_to_editor } = get_floating_sidebar_sibling_to_editor_status(layout_service);

    let side_bar_side_reached =
        !layout_service.is_visible(Parts::ActivitybarPart, None) && (!layout_service.is_visible(Parts::SidebarPart, None) || side_bar_sibling_to_editor);
    let aux_side_reached = !layout_service.is_visible(Parts::AuxiliarybarPart, None) || aux_sibling_to_editor;

    if side_bar_left {
        HorizontalPair { left: side_bar_side_reached, right: aux_side_reached }
    } else {
        HorizontalPair { left: aux_side_reached, right: side_bar_side_reached }
    }
}

/// O `source` de [`PartVisibilityChangeEvent`]: `'resize'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartVisibilityChangeSource {
    Resize,
}

/// `IPartVisibilityChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartVisibilityChangeEvent {
    pub part_id: String,
    pub visible: bool,
    pub source: Option<PartVisibilityChangeSource>,
}

/// O `{ readonly windowId: number; readonly maximized: boolean }` de `onDidChangeWindowMaximized`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowMaximizedChange {
    pub window_id: WindowId,
    pub maximized: bool,
}

/// `ILayoutOffsetInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutOffsetInfo {
    /// Generic top offset.
    pub top: f64,
    /// Quick pick specific top offset.
    pub quick_pick_top: f64,
}

/// O `{ readonly container: HTMLElement; readonly dimension: IDimension }` de `onDidLayoutContainer`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContainerLayout {
    pub container: LayoutContainerId,
    pub dimension: Dimension,
}

/// O `{ readonly container: HTMLElement; readonly disposables: DisposableStore }` de `onDidAddContainer`.
#[derive(Clone)]
pub struct AddedContainer {
    pub container: LayoutContainerId,
    pub disposables: Rc<DisposableStore>,
}

/// `ILayoutService` (`platform/layout/browser/layoutService.ts`, ver as divergências do módulo).
pub trait ILayoutService {
    /// An event that is emitted when the main container is layed out.
    fn on_did_layout_main_container(&self) -> Event<Dimension>;

    /// An event that is emitted when any container is layed out.
    fn on_did_layout_container(&self) -> Event<ContainerLayout>;

    /// An event that is emitted when the active container is layed out.
    fn on_did_layout_active_container(&self) -> Event<Dimension>;

    /// An event that is emitted when a new container is added. This can happen in multi-window environments.
    fn on_did_add_container(&self) -> Event<AddedContainer>;

    /// An event that is emitted when the active container changes.
    fn on_did_change_active_container(&self) -> Event<()>;

    /// The dimensions of the main container.
    fn main_container_dimension(&self) -> Dimension;

    /// The dimensions of the active container.
    fn active_container_dimension(&self) -> Dimension;

    /// Main container of the application.
    fn main_container(&self) -> LayoutContainerId;

    /// Active container of the application. When multiple windows are opened, will return the container of
    /// the active, focused window.
    fn active_container(&self) -> LayoutContainerId;

    /// All the containers of the application. There can be one container per window.
    fn containers(&self) -> Vec<LayoutContainerId>;

    /// Get the container for the given window.
    fn get_container(&self, window: WindowId) -> LayoutContainerId;

    /// Ensures that the styles for the container associated to the window have loaded. For the main window,
    /// this will resolve instantly, but for floating windows, this will resolve once the styles have been
    /// loaded and helps for when certain layout assumptions are made.
    fn when_container_styles_loaded(&self, window: WindowId) -> Option<Promise<()>>;

    /// An offset to use for positioning elements inside the main container.
    fn main_container_offset(&self) -> LayoutOffsetInfo;

    /// An offset to use for positioning elements inside the container.
    fn active_container_offset(&self) -> LayoutOffsetInfo;

    /// Focus the primary component of the active container.
    fn focus(&self);
}

/// O que o layout lê de uma `Part` (`browser/part.ts`) ao registrá-la: o identificador.
pub trait LayoutPart {
    /// `getId()`.
    fn get_id(&self) -> String;
}

/// `IWorkbenchLayoutService`.
pub trait IWorkbenchLayoutService: ILayoutService {
    /// Emits when the zen mode is enabled or disabled.
    fn on_did_change_zen_mode(&self) -> Event<bool>;

    /// Emits when the target window is maximized or unmaximized.
    fn on_did_change_window_maximized(&self) -> Event<WindowMaximizedChange>;

    /// Emits when main editor centered layout is enabled or disabled.
    fn on_did_change_main_editor_centered_layout(&self) -> Event<bool>;

    /// Emit when panel position changes.
    fn on_did_change_panel_position(&self) -> Event<String>;

    /// Emit when panel alignment changes.
    fn on_did_change_panel_alignment(&self) -> Event<PanelAlignment>;

    /// Emit when part visibility changes.
    fn on_did_change_part_visibility(&self) -> Event<PartVisibilityChangeEvent>;

    /// Emit when notifications (toasts or center) visibility changes.
    fn on_did_change_notifications_visibility(&self) -> Event<bool>;

    /// Emit when auxiliary bar maximized state changes.
    fn on_did_change_auxiliary_bar_maximized(&self) -> Event<()>;

    /// True if a default layout with default editors was applied at startup.
    fn opened_default_editors(&self) -> bool;

    /// Run a layout of the workbench.
    fn layout(&self);

    /// Asks the part service if all parts have been fully restored. For editor part this means that the
    /// contents of visible editors have loaded.
    fn is_restored(&self) -> bool;

    /// A promise for to await the `isRestored()` condition to be `true`.
    fn when_restored(&self) -> Promise<()>;

    /// Returns whether the given part has the keyboard focus or not.
    fn has_focus(&self, part: Parts) -> bool;

    /// Returns whether the floating panels presentation is enabled for this workbench, i.e. whether the
    /// Modern UI Update experiment (`LayoutSettings.MODERN_UI`) is on. Always `false` for the agents window,
    /// which has its own floating card design and must not apply the experiment's content insets.
    fn is_floating_panels_enabled(&self) -> bool;

    /// Returns whether Modern UI uses its compact density.
    fn is_modern_ui_compact(&self) -> bool;

    /// Focuses the part in the target window. If the part is not visible this is a noop. `target_window` é
    /// `None` para uma parte de janela única e `Some` para uma parte de várias janelas.
    fn focus_part(&self, part: Parts, target_window: Option<WindowId>);

    /// A sobrecarga `getContainer(targetWindow, part)`: returns the target window container or parts HTML
    /// element within, if there is one. Com `part` `None` é o container da janela.
    fn get_container_of_part(&self, target_window: WindowId, part: Option<Parts>) -> Option<LayoutContainerId>;

    /// Returns if the part is visible in the target window. `target_window` é `None` para uma parte de
    /// janela única e `Some` para uma parte de várias janelas.
    fn is_visible(&self, part: Parts, target_window: Option<WindowId>) -> bool;

    /// Set part hidden or not in the target window.
    fn set_part_hidden(&self, hidden: bool, part: Parts);

    /// Returns whether the layout surface that represents the secondary sidebar is visible.
    fn is_secondary_side_bar_visible(&self) -> bool;

    /// Toggles the layout surface that represents the secondary sidebar.
    fn toggle_secondary_side_bar(&self);

    /// Maximizes the panel height if the panel is not already maximized. Shrinks the panel to the default
    /// starting size if the panel is maximized.
    fn toggle_maximized_panel(&self);

    /// Returns true if the panel is maximized.
    fn is_panel_maximized(&self) -> bool;

    /// Maximizes the auxiliary sidebar by hiding the editor and panel areas. Restores the previous layout if
    /// the auxiliary sidebar is already maximized.
    fn toggle_maximized_auxiliary_bar(&self);

    /// Maximizes or restores the auxiliary sidebar.
    ///
    /// Returns `true` if there was a change in the maximization state.
    fn set_auxiliary_bar_maximized(&self, maximized: bool) -> bool;

    /// Returns true if the auxiliary sidebar is maximized.
    fn is_auxiliary_bar_maximized(&self) -> bool;

    /// Returns true if the main window has a border.
    fn has_main_window_border(&self) -> bool;

    /// Returns the main window border radius if any.
    fn get_main_window_border_radius(&self) -> Option<String>;

    /// Gets the current side bar position. Note that the sidebar can be hidden too.
    fn get_side_bar_position(&self) -> Position;

    /// Toggles the menu bar visibility.
    fn toggle_menu_bar(&self);

    /// Gets the current panel position. Note that the panel can be hidden too.
    fn get_panel_position(&self) -> Position;

    /// Sets the panel position.
    fn set_panel_position(&self, position: Position);

    /// Gets the panel alignement.
    fn get_panel_alignment(&self) -> PanelAlignment;

    /// Sets the panel alignment.
    fn set_panel_alignment(&self, alignment: PanelAlignment);

    /// Gets the maximum possible size for editor in the given container.
    fn get_maximum_editor_dimensions(&self, container: LayoutContainerId) -> Dimension;

    /// Toggles the workbench in and out of zen mode - parts get hidden and window goes fullscreen.
    fn toggle_zen_mode(&self);

    /// Returns whether the centered editor layout is active on the main editor part.
    fn is_main_editor_layout_centered(&self) -> bool;

    /// Sets the main editor part in and out of centered layout.
    fn center_main_editor_layout(&self, active: bool);

    /// Get the provided parts size in the main window.
    fn get_size(&self, part: Parts) -> ViewDimensions;

    /// Set the provided parts size in the main window.
    fn set_size(&self, part: Parts, size: ViewDimensions);

    /// Resize the provided part in the main window.
    fn resize_part(&self, part: Parts, size_change_width: f64, size_change_height: f64);

    /// Register a part to participate in the layout.
    fn register_part(&self, part: Rc<dyn LayoutPart>) -> DisposableRef;

    /// Returns whether the target window is maximized.
    fn is_window_maximized(&self, target_window: WindowId) -> bool;

    /// Updates the maximized state of the target window.
    fn update_window_maximized_state(&self, target_window: WindowId, maximized: bool);

    /// Returns the next visible view part in a given direction in the main window.
    fn get_visible_neighbor_part(&self, part: Parts, direction: Direction) -> Option<Parts>;
}

/// O que `shouldShowCustomTitleBar` pergunta, no original, a `platform/window/common/window.ts`,
/// `base/browser/browser.ts`, `base/browser/window.ts` e `base/common/platform.ts` (ver as divergências do
/// módulo).
pub trait TitleBarHost {
    /// `isWeb`.
    fn is_web(&self) -> bool;

    /// `isMacintosh`.
    fn is_macintosh(&self) -> bool;

    /// `isNative`.
    fn is_native(&self) -> bool;

    /// `hasCustomTitlebar(configurationService)`.
    fn has_custom_titlebar(&self) -> bool;

    /// `hasNativeTitlebar(configurationService)`.
    fn has_native_titlebar(&self) -> bool;

    /// `hasNativeMenu(configurationService)`.
    fn has_native_menu(&self) -> bool;

    /// `isFullscreen(window)`.
    fn is_fullscreen(&self, window: WindowId) -> bool;

    /// `isWCOEnabled()`.
    fn is_wco_enabled(&self) -> bool;

    /// `isAuxiliaryWindow(window)`.
    fn is_auxiliary_window(&self, window: WindowId) -> bool;

    /// `getMenuBarVisibility(configurationService)`: `'classic' | 'visible' | 'toggle' | 'hidden' | 'compact'`;
    /// `None` quando o valor não é um texto.
    fn get_menu_bar_visibility(&self) -> Option<String>;
}

/// `TitleBarSetting.CUSTOM_TITLE_BAR_VISIBILITY` (`platform/window/common/window.ts`).
const CUSTOM_TITLE_BAR_VISIBILITY_SETTING: &str = "window.customTitleBarVisibility";

/// `getValue(key) === value`, a comparação estrita do original.
fn setting_is(configuration_service: &dyn IConfigurationService, key: &str, value: &str) -> bool {
    configuration_service.get_value(Some(key), None).strict_equals(&Dynamic::from(value))
}

/// `shouldShowCustomTitleBar(configurationService, window, menuBarToggled)`.
pub fn should_show_custom_title_bar(configuration_service: &dyn IConfigurationService, host: &dyn TitleBarHost, window: WindowId, menu_bar_toggled: Option<bool>) -> bool {
    if !host.has_custom_titlebar() {
        return false;
    }

    let in_fullscreen = host.is_fullscreen(window);
    let native_title_bar_enabled = host.has_native_titlebar();
    let menu_bar_toggled = menu_bar_toggled.unwrap_or(false);

    if !host.is_web() {
        // `CustomTitleBarVisibility.NEVER` e `CustomTitleBarVisibility.WINDOWED`
        let never = setting_is(configuration_service, CUSTOM_TITLE_BAR_VISIBILITY_SETTING, "never");
        let windowed = setting_is(configuration_service, CUSTOM_TITLE_BAR_VISIBILITY_SETTING, "windowed");
        if never && native_title_bar_enabled || windowed && in_fullscreen {
            return false;
        }
    }

    if !is_title_bar_empty(configuration_service) {
        return true;
    }

    // Hide custom title bar when native title bar enabled and custom title bar is empty
    if native_title_bar_enabled && host.has_native_menu() {
        return false;
    }

    // macOS desktop does not need a title bar when full screen
    if host.is_macintosh() && host.is_native() {
        return !in_fullscreen;
    }

    // non-fullscreen native must show the title bar
    if host.is_native() && !in_fullscreen {
        return true;
    }

    // if WCO is visible, we have to show the title bar
    if host.is_wco_enabled() && !in_fullscreen {
        return true;
    }

    // remaining behavior is based on menubar visibility
    let menu_bar_visibility = if !host.is_auxiliary_window(window) { host.get_menu_bar_visibility() } else { Some("hidden".to_owned()) };
    match menu_bar_visibility.as_deref() {
        Some("classic") => !in_fullscreen || menu_bar_toggled,
        Some("compact") | Some("hidden") => false,
        Some("toggle") => menu_bar_toggled,
        Some("visible") => true,
        _ => {
            if host.is_web() {
                false
            } else {
                !in_fullscreen || menu_bar_toggled
            }
        }
    }
}

fn is_title_bar_empty(configuration_service: &dyn IConfigurationService) -> bool {
    // with the command center enabled, we should always show
    if configuration_service.get_value(Some(LayoutSettings::CommandCenter.as_str()), None).is_truthy() {
        return false;
    }

    // with the activity bar on top, we should always show
    let activity_bar_location = LayoutSettings::ActivityBarLocation.as_str();
    if setting_is(configuration_service, activity_bar_location, ActivityBarPosition::Top.as_str())
        || setting_is(configuration_service, activity_bar_location, ActivityBarPosition::Bottom.as_str())
    {
        return false;
    }

    // with the editor actions on top, we should always show
    let editor_actions_location = LayoutSettings::EditorActionsLocation.as_str();
    if setting_is(configuration_service, editor_actions_location, EditorActionsLocation::Titlebar.as_str())
        || setting_is(configuration_service, editor_actions_location, EditorActionsLocation::Default.as_str())
            && setting_is(configuration_service, LayoutSettings::EditorTabsMode.as_str(), EditorTabsMode::None.as_str())
    {
        return false;
    }

    // with the layout actions on top, we should always show
    if configuration_service.get_value(Some(LayoutSettings::LayoutActions.as_str()), None).is_truthy() {
        return false;
    }

    true
}
