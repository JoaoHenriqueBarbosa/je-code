//! Porte de `src/vs/workbench/services/layout/test/browser/layoutService.test.ts` (suítes `LayoutService - ...`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `TestLayoutService` (do `workbenchTestServices.ts`) e as subclasses que o teste declara
//!   (`VisibilityLayoutService`, `ConfigurableLayoutService`, `HorizontalMarginLayoutService`,
//!   `SiblingStatusLayoutService`, `VerticalMarginLayoutService`) viram um único dublê, [`TestLayoutService`],
//!   com um campo para cada método que alguma subclasse sobrescreve. O `Default` é o `TestLayoutService` do
//!   original (`isFloatingPanelsEnabled` e `isModernUICompact` falsos, `getSideBarPosition` e
//!   `getPanelPosition` em `LEFT`, `getPanelAlignment` `center`, `isVisible` sempre verdadeiro) e cada
//!   subclasse é uma função que parte desse padrão e troca o que a classe do original troca. O que o
//!   original não observa (os eventos, os containers, os tamanhos) está no dublê só porque o trait exige, com os
//!   valores do `TestLayoutService`.
//! - O `isVisible(part)` do dublê ignora a janela, como o `override isVisible(part: Parts)` do original;
//!   `visibleParts` (um `Set<Parts>`) é `Option<HashSet<Parts>>`, e `None` é o `isVisible` de qualquer parte
//!   do `TestLayoutService` (sem `visibleParts`).
//! - `mainWindow` é `MAIN_WINDOW`. Os literais de objeto comparados com `assert.deepStrictEqual` são listas de
//!   pares (rótulo, valor), na ordem das chaves do original.
//! - `Set#delete` e `Set#clear` são `HashSet::remove` e `HashSet::clear`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::HashSet;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, none};
use je_base::promise::Promise;
use je_editor::core_2d::dimension::Dimension;
use je_widgets::grid::Direction;
use je_widgets::gridview::ViewDimensions;
use je_workbench::services::layout::layout_service::{
    AddedContainer, COMPACT_FLOATING_PANEL_MARGIN, COMPACT_FLOATING_PANEL_OUTER_MARGIN, ContainerLayout, FLOATING_PANEL_INNER_MARGIN, FLOATING_PANEL_MARGIN,
    HorizontalPair, ILayoutService, IWorkbenchLayoutService, LayoutContainerId, LayoutOffsetInfo, LayoutPart, MAIN_WINDOW, PanelAlignment, PartVisibilityChangeEvent, Parts,
    Position, SidebarSiblingToEditorStatus, VerticalPair, WindowId, WindowMaximizedChange, get_floating_editor_vertical_margins, get_floating_outer_edge_owners,
    get_floating_pane_composite_horizontal_margins, get_floating_pane_composite_vertical_margins, get_floating_panel_margin, get_floating_panel_outer_margin,
    get_floating_sidebar_sibling_to_editor_status, is_floating_top_edge_exposed,
};

/// O `TestLayoutService` do original, mais as sobrescritas das subclasses do teste.
struct TestLayoutService {
    floating_panels_enabled: bool,
    modern_ui_compact: bool,
    side_bar_position: Position,
    panel_position: Position,
    panel_alignment: PanelAlignment,
    /// `None`: o `isVisible` do `TestLayoutService` (sempre verdadeiro).
    visible_parts: Option<HashSet<Parts>>,
}

impl Default for TestLayoutService {
    fn default() -> Self {
        Self {
            floating_panels_enabled: false,
            modern_ui_compact: false,
            side_bar_position: Position::Left,
            panel_position: Position::Left,
            panel_alignment: PanelAlignment::Center,
            visible_parts: None,
        }
    }
}

fn visible(parts: &[Parts]) -> Option<HashSet<Parts>> {
    Some(parts.iter().copied().collect())
}

impl TestLayoutService {
    /// `visibleParts` (o `Set<Parts>` das subclasses) para `add`, `delete` e `clear`.
    fn visible_parts_mut(&mut self) -> &mut HashSet<Parts> {
        self.visible_parts.as_mut().expect("a subclasse do teste define visibleParts")
    }

    /// `VisibilityLayoutService`.
    fn visibility(visible_parts: &[Parts]) -> Self {
        Self { visible_parts: visible(visible_parts), ..Self::default() }
    }

    /// `ConfigurableLayoutService`.
    fn configurable() -> Self {
        Self { floating_panels_enabled: true, panel_position: Position::Bottom, visible_parts: visible(&[]), ..Self::default() }
    }

    /// `HorizontalMarginLayoutService`.
    fn horizontal_margin() -> Self {
        Self { floating_panels_enabled: true, panel_position: Position::Bottom, visible_parts: visible(&[]), ..Self::default() }
    }

    /// `SiblingStatusLayoutService`.
    fn sibling_status() -> Self {
        Self::default()
    }

    /// `VerticalMarginLayoutService`.
    fn vertical_margin() -> Self {
        Self {
            floating_panels_enabled: true,
            panel_position: Position::Bottom,
            visible_parts: visible(&[Parts::TitlebarPart, Parts::StatusbarPart, Parts::EditorPart]),
            ..Self::default()
        }
    }
}

const BODY: LayoutContainerId = LayoutContainerId(1);

impl ILayoutService for TestLayoutService {
    fn on_did_layout_main_container(&self) -> Event<Dimension> {
        Event::none()
    }

    fn on_did_layout_container(&self) -> Event<ContainerLayout> {
        Event::none()
    }

    fn on_did_layout_active_container(&self) -> Event<Dimension> {
        Event::none()
    }

    fn on_did_add_container(&self) -> Event<AddedContainer> {
        Event::none()
    }

    fn on_did_change_active_container(&self) -> Event<()> {
        Event::none()
    }

    fn main_container_dimension(&self) -> Dimension {
        Dimension { width: 800.0, height: 600.0 }
    }

    fn active_container_dimension(&self) -> Dimension {
        Dimension { width: 800.0, height: 600.0 }
    }

    fn main_container(&self) -> LayoutContainerId {
        BODY
    }

    fn active_container(&self) -> LayoutContainerId {
        BODY
    }

    fn containers(&self) -> Vec<LayoutContainerId> {
        vec![BODY]
    }

    fn get_container(&self, _window: WindowId) -> LayoutContainerId {
        BODY
    }

    fn when_container_styles_loaded(&self, _window: WindowId) -> Option<Promise<()>> {
        None
    }

    fn main_container_offset(&self) -> LayoutOffsetInfo {
        LayoutOffsetInfo { top: 0.0, quick_pick_top: 0.0 }
    }

    fn active_container_offset(&self) -> LayoutOffsetInfo {
        LayoutOffsetInfo { top: 0.0, quick_pick_top: 0.0 }
    }

    fn focus(&self) {}
}

impl IWorkbenchLayoutService for TestLayoutService {
    fn on_did_change_zen_mode(&self) -> Event<bool> {
        Event::none()
    }

    fn on_did_change_window_maximized(&self) -> Event<WindowMaximizedChange> {
        Event::none()
    }

    fn on_did_change_main_editor_centered_layout(&self) -> Event<bool> {
        Event::none()
    }

    fn on_did_change_panel_position(&self) -> Event<String> {
        Event::none()
    }

    fn on_did_change_panel_alignment(&self) -> Event<PanelAlignment> {
        Event::none()
    }

    fn on_did_change_part_visibility(&self) -> Event<PartVisibilityChangeEvent> {
        Event::none()
    }

    fn on_did_change_notifications_visibility(&self) -> Event<bool> {
        Event::none()
    }

    fn on_did_change_auxiliary_bar_maximized(&self) -> Event<()> {
        Event::none()
    }

    fn opened_default_editors(&self) -> bool {
        false
    }

    fn layout(&self) {}

    fn is_restored(&self) -> bool {
        true
    }

    fn when_restored(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn has_focus(&self, _part: Parts) -> bool {
        false
    }

    fn is_floating_panels_enabled(&self) -> bool {
        self.floating_panels_enabled
    }

    fn is_modern_ui_compact(&self) -> bool {
        self.modern_ui_compact
    }

    fn focus_part(&self, _part: Parts, _target_window: Option<WindowId>) {}

    fn get_container_of_part(&self, _target_window: WindowId, _part: Option<Parts>) -> Option<LayoutContainerId> {
        Some(BODY)
    }

    fn is_visible(&self, part: Parts, _target_window: Option<WindowId>) -> bool {
        self.visible_parts.as_ref().is_none_or(|visible_parts| visible_parts.contains(&part))
    }

    fn set_part_hidden(&self, _hidden: bool, _part: Parts) {}

    fn is_secondary_side_bar_visible(&self) -> bool {
        false
    }

    fn toggle_secondary_side_bar(&self) {}

    fn toggle_maximized_panel(&self) {}

    fn is_panel_maximized(&self) -> bool {
        false
    }

    fn toggle_maximized_auxiliary_bar(&self) {}

    fn set_auxiliary_bar_maximized(&self, _maximized: bool) -> bool {
        false
    }

    fn is_auxiliary_bar_maximized(&self) -> bool {
        false
    }

    fn has_main_window_border(&self) -> bool {
        false
    }

    fn get_main_window_border_radius(&self) -> Option<String> {
        None
    }

    fn get_side_bar_position(&self) -> Position {
        self.side_bar_position
    }

    fn toggle_menu_bar(&self) {}

    fn get_panel_position(&self) -> Position {
        self.panel_position
    }

    fn set_panel_position(&self, _position: Position) {}

    fn get_panel_alignment(&self) -> PanelAlignment {
        self.panel_alignment
    }

    fn set_panel_alignment(&self, _alignment: PanelAlignment) {}

    fn get_maximum_editor_dimensions(&self, _container: LayoutContainerId) -> Dimension {
        panic!("not implemented")
    }

    fn toggle_zen_mode(&self) {}

    fn is_main_editor_layout_centered(&self) -> bool {
        false
    }

    fn center_main_editor_layout(&self, _active: bool) {}

    fn get_size(&self, _part: Parts) -> ViewDimensions {
        panic!("Method not implemented.")
    }

    fn set_size(&self, _part: Parts, _size: ViewDimensions) {
        panic!("Method not implemented.")
    }

    fn resize_part(&self, _part: Parts, _size_change_width: f64, _size_change_height: f64) {}

    fn register_part(&self, _part: Rc<dyn LayoutPart>) -> DisposableRef {
        none()
    }

    fn is_window_maximized(&self, _target_window: WindowId) -> bool {
        false
    }

    fn update_window_maximized_state(&self, _target_window: WindowId, _maximized: bool) {}

    fn get_visible_neighbor_part(&self, _part: Parts, _direction: Direction) -> Option<Parts> {
        None
    }
}

fn horizontal(left: f64, right: f64) -> HorizontalPair<f64> {
    HorizontalPair { left, right }
}

fn vertical(top: f64, bottom: f64) -> VerticalPair<f64> {
    VerticalPair { top, bottom }
}

fn owner_pair(left: Option<Parts>, right: Option<Parts>) -> HorizontalPair<Option<Parts>> {
    HorizontalPair { left, right }
}

mod is_floating_top_edge_exposed_suite {
    use super::*;

    fn top_edge_exposed(visible_parts: &[Parts]) -> bool {
        let service = TestLayoutService::visibility(visible_parts);
        is_floating_top_edge_exposed(&service, MAIN_WINDOW)
    }

    #[test]
    fn exposed_only_when_both_the_title_bar_and_the_banner_are_hidden() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = [
            ("bothHidden", top_edge_exposed(&[])),
            ("titleBarVisible", top_edge_exposed(&[Parts::TitlebarPart])),
            ("bannerVisible", top_edge_exposed(&[Parts::BannerPart])),
            ("bothVisible", top_edge_exposed(&[Parts::TitlebarPart, Parts::BannerPart])),
        ];

        // A visible banner gives the cards a top edge to sit against, same as a visible title bar.
        assert_eq!(actual, [("bothHidden", true), ("titleBarVisible", false), ("bannerVisible", false), ("bothVisible", false)]);
    }
}

mod floating_panel_spacing_suite {
    use super::*;

    #[test]
    fn uses_density_specific_inter_card_and_perimeter_gaps() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let compact_layout_service = TestLayoutService { modern_ui_compact: true, ..TestLayoutService::default() };

        assert_eq!(
            [
                ("leadingMargin", FLOATING_PANEL_MARGIN),
                ("trailingMargin", FLOATING_PANEL_INNER_MARGIN),
                ("gap", FLOATING_PANEL_MARGIN + FLOATING_PANEL_INNER_MARGIN),
                ("outerMargin", get_floating_panel_outer_margin(&TestLayoutService::default())),
                ("compactMargin", get_floating_panel_margin(&compact_layout_service)),
                ("compactOuterMargin", get_floating_panel_outer_margin(&compact_layout_service)),
            ],
            [("leadingMargin", 4.0), ("trailingMargin", 0.0), ("gap", 4.0), ("outerMargin", 4.0), ("compactMargin", 0.0), ("compactOuterMargin", 0.0)]
        );
    }
}

mod get_floating_outer_edge_owners_suite {
    use super::*;

    fn owners(configure: impl FnOnce(&mut TestLayoutService)) -> HorizontalPair<Option<Parts>> {
        let mut service = TestLayoutService::configurable();
        configure(&mut service);
        get_floating_outer_edge_owners(&service)
    }

    #[test]
    fn edge_ownership_across_layouts() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let full = [Parts::ActivitybarPart, Parts::SidebarPart, Parts::EditorPart, Parts::AuxiliarybarPart];
        let actual = [
            // Experiment disabled: no owners regardless of layout.
            (
                "disabled",
                owners(|s| {
                    s.floating_panels_enabled = false;
                    s.visible_parts = visible(&[Parts::AuxiliarybarPart]);
                }),
            ),
            // Default full layout (side bar left): the activity bar is the outermost card of the
            // side bar cluster and owns the left edge, the secondary side bar owns the right edge.
            ("defaultFull", owners(|s| s.visible_parts = visible(&full))),
            (
                "defaultFullSideBarRight",
                owners(|s| {
                    s.side_bar_position = Position::Right;
                    s.visible_parts = visible(&full);
                }),
            ),
            // Compact density resolves ownership identically.
            (
                "compactFull",
                owners(|s| {
                    s.modern_ui_compact = true;
                    s.visible_parts = visible(&full);
                }),
            ),
            (
                "compactFullSideBarRight",
                owners(|s| {
                    s.modern_ui_compact = true;
                    s.side_bar_position = Position::Right;
                    s.visible_parts = visible(&full);
                }),
            ),
            // Maximized aux bar with the activity bar in its default (visible) position: the
            // activity bar owns the left edge, the aux bar owns the right edge.
            ("maximizedAuxWithActivityBar", owners(|s| s.visible_parts = visible(&[Parts::ActivitybarPart, Parts::AuxiliarybarPart]))),
            // Maximized aux bar with the activity bar not in its default position (hidden from
            // the side column): the aux bar spans the full width and owns both edges.
            ("maximizedAuxNoActivityBar", owners(|s| s.visible_parts = visible(&[Parts::AuxiliarybarPart]))),
            // Same, but the side bar is on the right: the aux bar still spans and owns both edges.
            (
                "maximizedAuxNoActivityBarSideBarRight",
                owners(|s| {
                    s.side_bar_position = Position::Right;
                    s.visible_parts = visible(&[Parts::AuxiliarybarPart]);
                }),
            ),
            // Only the editor visible with the activity bar hidden: the editor is the sole card
            // and owns both edges.
            ("editorOnly", owners(|s| s.visible_parts = visible(&[Parts::EditorPart]))),
            // Full layout with a visible left vertical panel: the panel sits between the editor
            // and the side bar, so it never reaches an edge.
            (
                "verticalPanelFull",
                owners(|s| {
                    s.panel_position = Position::Left;
                    s.visible_parts = visible(&[Parts::ActivitybarPart, Parts::SidebarPart, Parts::PanelPart, Parts::EditorPart, Parts::AuxiliarybarPart]);
                }),
            ),
            // Maximized left vertical panel with the activity bar hidden: the panel spans the
            // full width and owns both edges.
            (
                "maximizedVerticalPanel",
                owners(|s| {
                    s.panel_position = Position::Left;
                    s.visible_parts = visible(&[Parts::PanelPart]);
                }),
            ),
            // Visible horizontal (bottom) panel: not part of the vertical order, so it owns no
            // edge; the secondary side bar still owns the right edge.
            (
                "horizontalPanelVisible",
                owners(|s| {
                    s.panel_position = Position::Bottom;
                    s.visible_parts = visible(&[Parts::SidebarPart, Parts::EditorPart, Parts::PanelPart, Parts::AuxiliarybarPart]);
                }),
            ),
        ];

        assert_eq!(
            actual,
            [
                ("disabled", owner_pair(None, None)),
                ("defaultFull", owner_pair(Some(Parts::ActivitybarPart), Some(Parts::AuxiliarybarPart))),
                ("defaultFullSideBarRight", owner_pair(Some(Parts::AuxiliarybarPart), Some(Parts::ActivitybarPart))),
                ("compactFull", owner_pair(Some(Parts::ActivitybarPart), Some(Parts::AuxiliarybarPart))),
                ("compactFullSideBarRight", owner_pair(Some(Parts::AuxiliarybarPart), Some(Parts::ActivitybarPart))),
                ("maximizedAuxWithActivityBar", owner_pair(Some(Parts::ActivitybarPart), Some(Parts::AuxiliarybarPart))),
                ("maximizedAuxNoActivityBar", owner_pair(Some(Parts::AuxiliarybarPart), Some(Parts::AuxiliarybarPart))),
                ("maximizedAuxNoActivityBarSideBarRight", owner_pair(Some(Parts::AuxiliarybarPart), Some(Parts::AuxiliarybarPart))),
                ("editorOnly", owner_pair(Some(Parts::EditorPart), Some(Parts::EditorPart))),
                ("verticalPanelFull", owner_pair(Some(Parts::ActivitybarPart), Some(Parts::AuxiliarybarPart))),
                ("maximizedVerticalPanel", owner_pair(Some(Parts::PanelPart), Some(Parts::PanelPart))),
                ("horizontalPanelVisible", owner_pair(Some(Parts::SidebarPart), Some(Parts::AuxiliarybarPart))),
            ]
        );
    }
}

mod get_floating_pane_composite_horizontal_margins_suite {
    use super::*;

    fn margins(part_id: Parts, visible_parts: &[Parts], side_bar_position: Position, compact: bool, panel_alignment: PanelAlignment) -> HorizontalPair<f64> {
        let mut service = TestLayoutService::horizontal_margin();
        service.side_bar_position = side_bar_position;
        service.visible_parts = visible(visible_parts);
        service.modern_ui_compact = compact;
        service.panel_alignment = panel_alignment;
        get_floating_pane_composite_horizontal_margins(&service, part_id)
    }

    /// Os valores padrão dos parâmetros do `margins` do original: `Position.LEFT`, `false` e `'center'`.
    fn margins_default(part_id: Parts, visible_parts: &[Parts]) -> HorizontalPair<f64> {
        margins(part_id, visible_parts, Position::Left, false, PanelAlignment::Center)
    }

    #[test]
    fn horizontal_margins_across_densities_and_side_bar_positions() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let full = [Parts::ActivitybarPart, Parts::SidebarPart, Parts::EditorPart, Parts::AuxiliarybarPart];
        let side_bar_with_rail = [Parts::ActivitybarPart, Parts::SidebarPart, Parts::EditorPart];
        let with_panel = [Parts::ActivitybarPart, Parts::SidebarPart, Parts::EditorPart, Parts::PanelPart, Parts::AuxiliarybarPart];
        let actual = [
            ("activityBarLeft", margins_default(Parts::AuxiliarybarPart, &full)),
            ("activityBarRight", margins(Parts::AuxiliarybarPart, &full, Position::Right, false, PanelAlignment::Center)),
            ("secondarySideBarOnly", margins_default(Parts::AuxiliarybarPart, &[Parts::AuxiliarybarPart])),
            // Default density: the primary side bar meets the activity bar rail flush on the
            // facing edge, and falls back to the outer gutter when the rail is hidden.
            ("primarySideBarLeft", margins(Parts::SidebarPart, &side_bar_with_rail, Position::Left, false, PanelAlignment::Center)),
            ("primarySideBarRight", margins(Parts::SidebarPart, &side_bar_with_rail, Position::Right, false, PanelAlignment::Center)),
            ("primarySideBarLeftNoActivityBar", margins(Parts::SidebarPart, &[Parts::SidebarPart, Parts::EditorPart], Position::Left, false, PanelAlignment::Center)),
            ("compactSecondarySideBarOnly", margins(Parts::AuxiliarybarPart, &[Parts::AuxiliarybarPart], Position::Left, true, PanelAlignment::Center)),
            ("compactPrimarySideBarLeft", margins(Parts::SidebarPart, &side_bar_with_rail, Position::Left, true, PanelAlignment::Center)),
            ("compactPrimarySideBarRight", margins(Parts::SidebarPart, &side_bar_with_rail, Position::Right, true, PanelAlignment::Center)),
            ("justifiedPanelSideBarRight", margins(Parts::PanelPart, &with_panel, Position::Right, false, PanelAlignment::Justify)),
            ("rightAlignedPanelSideBarRight", margins(Parts::PanelPart, &with_panel, Position::Right, false, PanelAlignment::Right)),
            ("leftAlignedPanelSideBarRight", margins(Parts::PanelPart, &with_panel, Position::Right, false, PanelAlignment::Left)),
            (
                "justifiedPanelSideBarHidden",
                margins(
                    Parts::PanelPart,
                    &[Parts::ActivitybarPart, Parts::EditorPart, Parts::PanelPart, Parts::AuxiliarybarPart],
                    Position::Right,
                    false,
                    PanelAlignment::Justify,
                ),
            ),
            ("compactJustifiedPanel", margins(Parts::PanelPart, &with_panel, Position::Left, true, PanelAlignment::Justify)),
            ("compactJustifiedPanelSideBarRight", margins(Parts::PanelPart, &with_panel, Position::Right, true, PanelAlignment::Justify)),
        ];

        assert_eq!(
            actual,
            [
                // The default density keeps its 4px perimeter and inter-card gaps.
                ("activityBarLeft", horizontal(4.0, 4.0)),
                ("activityBarRight", horizontal(4.0, 0.0)),
                ("secondarySideBarOnly", horizontal(4.0, 4.0)),
                ("primarySideBarLeft", horizontal(FLOATING_PANEL_INNER_MARGIN, FLOATING_PANEL_INNER_MARGIN)),
                ("primarySideBarRight", horizontal(4.0, FLOATING_PANEL_INNER_MARGIN)),
                ("primarySideBarLeftNoActivityBar", horizontal(4.0, FLOATING_PANEL_INNER_MARGIN)),
                ("compactSecondarySideBarOnly", horizontal(COMPACT_FLOATING_PANEL_OUTER_MARGIN, COMPACT_FLOATING_PANEL_OUTER_MARGIN)),
                ("compactPrimarySideBarLeft", horizontal(COMPACT_FLOATING_PANEL_MARGIN, FLOATING_PANEL_INNER_MARGIN)),
                ("compactPrimarySideBarRight", horizontal(COMPACT_FLOATING_PANEL_MARGIN, FLOATING_PANEL_INNER_MARGIN)),
                ("justifiedPanelSideBarRight", horizontal(FLOATING_PANEL_MARGIN, FLOATING_PANEL_MARGIN)),
                ("rightAlignedPanelSideBarRight", horizontal(FLOATING_PANEL_MARGIN, FLOATING_PANEL_MARGIN)),
                ("leftAlignedPanelSideBarRight", horizontal(FLOATING_PANEL_MARGIN, FLOATING_PANEL_INNER_MARGIN)),
                ("justifiedPanelSideBarHidden", horizontal(FLOATING_PANEL_MARGIN, FLOATING_PANEL_INNER_MARGIN)),
                ("compactJustifiedPanel", horizontal(COMPACT_FLOATING_PANEL_MARGIN, COMPACT_FLOATING_PANEL_OUTER_MARGIN)),
                ("compactJustifiedPanelSideBarRight", horizontal(COMPACT_FLOATING_PANEL_OUTER_MARGIN, FLOATING_PANEL_INNER_MARGIN)),
            ]
        );
    }
}

mod get_floating_sidebar_sibling_to_editor_status_suite {
    use super::*;

    fn sibling_status(configure: impl FnOnce(&mut TestLayoutService)) -> SidebarSiblingToEditorStatus {
        let mut service = TestLayoutService::sibling_status();
        configure(&mut service);
        get_floating_sidebar_sibling_to_editor_status(&service)
    }

    fn status(side_bar: bool, aux_bar: bool) -> SidebarSiblingToEditorStatus {
        SidebarSiblingToEditorStatus { side_bar, aux_bar }
    }

    #[test]
    fn sibling_to_editor_status_across_alignment_and_sidebar_position_combinations() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = [
            // center: neither bar is a sibling (both span full height)
            (
                "centerLeft",
                sibling_status(|s| {
                    s.side_bar_position = Position::Left;
                    s.panel_alignment = PanelAlignment::Center;
                }),
            ),
            (
                "centerRight",
                sibling_status(|s| {
                    s.side_bar_position = Position::Right;
                    s.panel_alignment = PanelAlignment::Center;
                }),
            ),
            // justify: both bars are siblings (panel spans the full width)
            (
                "justifyLeft",
                sibling_status(|s| {
                    s.side_bar_position = Position::Left;
                    s.panel_alignment = PanelAlignment::Justify;
                }),
            ),
            (
                "justifyRight",
                sibling_status(|s| {
                    s.side_bar_position = Position::Right;
                    s.panel_alignment = PanelAlignment::Justify;
                }),
            ),
            // left alignment, sidebar on LEFT: sidebar IS sibling, aux bar is NOT
            (
                "leftAlignSidebarLeft",
                sibling_status(|s| {
                    s.side_bar_position = Position::Left;
                    s.panel_alignment = PanelAlignment::Left;
                }),
            ),
            // left alignment, sidebar on RIGHT: sidebar is NOT sibling, aux bar IS
            (
                "leftAlignSidebarRight",
                sibling_status(|s| {
                    s.side_bar_position = Position::Right;
                    s.panel_alignment = PanelAlignment::Left;
                }),
            ),
            // right alignment, sidebar on LEFT: sidebar is NOT sibling, aux bar IS
            (
                "rightAlignSidebarLeft",
                sibling_status(|s| {
                    s.side_bar_position = Position::Left;
                    s.panel_alignment = PanelAlignment::Right;
                }),
            ),
            // right alignment, sidebar on RIGHT: sidebar IS sibling, aux bar is NOT
            (
                "rightAlignSidebarRight",
                sibling_status(|s| {
                    s.side_bar_position = Position::Right;
                    s.panel_alignment = PanelAlignment::Right;
                }),
            ),
        ];

        assert_eq!(
            actual,
            [
                ("centerLeft", status(false, false)),
                ("centerRight", status(false, false)),
                ("justifyLeft", status(true, true)),
                ("justifyRight", status(true, true)),
                ("leftAlignSidebarLeft", status(true, false)),
                ("leftAlignSidebarRight", status(false, true)),
                ("rightAlignSidebarLeft", status(false, true)),
                ("rightAlignSidebarRight", status(true, false)),
            ]
        );
    }
}

mod get_floating_pane_composite_vertical_margins_suite {
    use super::*;

    fn margins(part_id: Parts, configure: impl FnOnce(&mut TestLayoutService)) -> VerticalPair<f64> {
        let mut service = TestLayoutService::vertical_margin();
        configure(&mut service);
        get_floating_pane_composite_vertical_margins(&service, part_id, MAIN_WINDOW)
    }

    const INNER: f64 = FLOATING_PANEL_INNER_MARGIN;
    const MARGIN: f64 = FLOATING_PANEL_MARGIN;
    // The window-edge perimeter matches the inter-card gap at the default density.
    const OUTER: f64 = FLOATING_PANEL_MARGIN;

    #[test]
    fn bottom_panel_top_margin_across_editor_visibility_and_top_edge() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let bottom_panel = |configure: fn(&mut TestLayoutService)| {
            margins(Parts::PanelPart, |s| {
                s.visible_parts_mut().insert(Parts::PanelPart);
                configure(s);
            })
        };
        let actual = [
            // Editor above the panel: the gap is between two cards.
            ("editorVisible", bottom_panel(|_| {})),
            // Maximized panel (editor hidden) below a title bar: it takes over that row.
            (
                "maximizedUnderTitleBar",
                bottom_panel(|s| {
                    s.visible_parts_mut().remove(&Parts::EditorPart);
                }),
            ),
            // Maximized panel with nothing above it: the top is now a window edge.
            (
                "maximizedAtTopEdge",
                bottom_panel(|s| {
                    s.visible_parts_mut().remove(&Parts::EditorPart);
                    s.visible_parts_mut().remove(&Parts::TitlebarPart);
                }),
            ),
            // Maximized panel with a banner above it: still not a window edge.
            (
                "maximizedUnderBanner",
                bottom_panel(|s| {
                    s.visible_parts_mut().remove(&Parts::EditorPart);
                    s.visible_parts_mut().remove(&Parts::TitlebarPart);
                    s.visible_parts_mut().insert(Parts::BannerPart);
                }),
            ),
        ];

        assert_eq!(
            actual,
            [
                ("editorVisible", vertical(MARGIN, MARGIN)),
                ("maximizedUnderTitleBar", vertical(INNER, MARGIN)),
                ("maximizedAtTopEdge", vertical(OUTER, MARGIN)),
                ("maximizedUnderBanner", vertical(INNER, MARGIN)),
            ]
        );
    }

    #[test]
    fn margins_across_panel_positions() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = [
            // Top panel: its bottom faces the editor, so it never reaches the window bottom.
            (
                "topPanelStatusBarHidden",
                margins(Parts::PanelPart, |s| {
                    s.panel_position = Position::Top;
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::StatusbarPart);
                }),
            ),
            // Vertical panel: full height, so both edges are window edges.
            (
                "leftPanelAtBothEdges",
                margins(Parts::PanelPart, |s| {
                    s.panel_position = Position::Left;
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::TitlebarPart);
                    s.visible_parts_mut().remove(&Parts::StatusbarPart);
                }),
            ),
            // Side bar beside a top panel, center alignment: full height, so it starts at the top edge.
            (
                "sideBarTopPanelCentered",
                margins(Parts::SidebarPart, |s| {
                    s.panel_position = Position::Top;
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::TitlebarPart);
                }),
            ),
            // Same but justified: the bar is a sibling, so its top faces the panel card.
            (
                "sideBarTopPanelJustified",
                margins(Parts::SidebarPart, |s| {
                    s.panel_position = Position::Top;
                    s.panel_alignment = PanelAlignment::Justify;
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::TitlebarPart);
                }),
            ),
            // Sibling bar above a bottom panel: its bottom faces the panel, not the window.
            (
                "sideBarBottomPanelJustified",
                margins(Parts::SidebarPart, |s| {
                    s.panel_alignment = PanelAlignment::Justify;
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::StatusbarPart);
                }),
            ),
            // Full-height bar with the status bar hidden: it does reach the window bottom.
            (
                "sideBarBottomPanelCentered",
                margins(Parts::SidebarPart, |s| {
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::StatusbarPart);
                }),
            ),
            // Maximized top panel: the editor it normally faces is hidden, so the panel now owns
            // the cluster's bottom edge and takes the perimeter gutter there.
            (
                "topPanelMaximized",
                margins(Parts::PanelPart, |s| {
                    s.panel_position = Position::Top;
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::EditorPart);
                    s.visible_parts_mut().remove(&Parts::StatusbarPart);
                }),
            ),
            // Experiment off: the parts are not cards at all.
            (
                "disabled",
                margins(Parts::SidebarPart, |s| {
                    s.floating_panels_enabled = false;
                    s.visible_parts_mut().clear();
                }),
            ),
        ];

        assert_eq!(
            actual,
            [
                ("topPanelStatusBarHidden", vertical(INNER, INNER)),
                ("leftPanelAtBothEdges", vertical(OUTER, OUTER)),
                ("sideBarTopPanelCentered", vertical(OUTER, MARGIN)),
                ("sideBarTopPanelJustified", vertical(MARGIN, MARGIN)),
                ("sideBarBottomPanelJustified", vertical(INNER, INNER)),
                ("sideBarBottomPanelCentered", vertical(INNER, OUTER)),
                ("topPanelMaximized", vertical(INNER, OUTER)),
                ("disabled", vertical(0.0, 0.0)),
            ]
        );
    }

    #[test]
    fn compact_density_keeps_internal_seams_and_window_edges_flush() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(
            [
                (
                    "atWindowEdges",
                    margins(Parts::PanelPart, |service| {
                        service.modern_ui_compact = true;
                        service.panel_position = Position::Left;
                        service.visible_parts_mut().insert(Parts::PanelPart);
                        service.visible_parts_mut().remove(&Parts::TitlebarPart);
                        service.visible_parts_mut().remove(&Parts::StatusbarPart);
                    }),
                ),
                (
                    "betweenTitleAndStatusBars",
                    margins(Parts::PanelPart, |service| {
                        service.modern_ui_compact = true;
                        service.panel_position = Position::Left;
                        service.visible_parts_mut().insert(Parts::PanelPart);
                    }),
                ),
                (
                    "betweenEditorAndStatusBar",
                    margins(Parts::PanelPart, |service| {
                        service.modern_ui_compact = true;
                        service.visible_parts_mut().insert(Parts::PanelPart);
                    }),
                ),
            ],
            [("atWindowEdges", vertical(0.0, 0.0)), ("betweenTitleAndStatusBars", vertical(0.0, 0.0)), ("betweenEditorAndStatusBar", vertical(0.0, 0.0))]
        );
    }
}

mod get_floating_editor_vertical_margins_suite {
    use super::*;

    fn margins(configure: impl FnOnce(&mut TestLayoutService)) -> VerticalPair<f64> {
        let mut service = TestLayoutService::vertical_margin();
        configure(&mut service);
        get_floating_editor_vertical_margins(&service, MAIN_WINDOW)
    }

    const INNER: f64 = FLOATING_PANEL_INNER_MARGIN;
    const MARGIN: f64 = FLOATING_PANEL_MARGIN;
    // The window-edge perimeter matches the inter-card gap at the default density.
    const OUTER: f64 = FLOATING_PANEL_MARGIN;

    #[test]
    fn margins_across_panel_positions_title_bar_banner_and_status_bar() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = [
            // Windowed default: a title bar above and a status bar below.
            ("titleAndStatusBarVisible", margins(|_| {})),
            // Native fullscreen: nothing above the editor.
            (
                "titleBarHidden",
                margins(|s| {
                    s.visible_parts_mut().remove(&Parts::TitlebarPart);
                }),
            ),
            // A banner keeps the editor off the window edge.
            (
                "bannerInsteadOfTitleBar",
                margins(|s| {
                    s.visible_parts_mut().remove(&Parts::TitlebarPart);
                    s.visible_parts_mut().insert(Parts::BannerPart);
                }),
            ),
            // A top panel takes the place of the title bar, so the gap stays an inter-card one.
            (
                "topPanelAtTopEdge",
                margins(|s| {
                    s.panel_position = Position::Top;
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::TitlebarPart);
                }),
            ),
            // Status bar hidden: the editor reaches the window bottom.
            (
                "statusBarHidden",
                margins(|s| {
                    s.visible_parts_mut().remove(&Parts::StatusbarPart);
                }),
            ),
            // A bottom panel takes the place of the status bar.
            (
                "bottomPanelStatusBarHidden",
                margins(|s| {
                    s.visible_parts_mut().insert(Parts::PanelPart);
                    s.visible_parts_mut().remove(&Parts::StatusbarPart);
                }),
            ),
            // Experiment off.
            (
                "disabled",
                margins(|s| {
                    s.floating_panels_enabled = false;
                    s.visible_parts_mut().clear();
                }),
            ),
        ];

        assert_eq!(
            actual,
            [
                ("titleAndStatusBarVisible", vertical(INNER, MARGIN)),
                ("titleBarHidden", vertical(OUTER, MARGIN)),
                ("bannerInsteadOfTitleBar", vertical(INNER, MARGIN)),
                ("topPanelAtTopEdge", vertical(MARGIN, MARGIN)),
                ("statusBarHidden", vertical(INNER, OUTER)),
                ("bottomPanelStatusBarHidden", vertical(INNER, INNER)),
                ("disabled", vertical(0.0, 0.0)),
            ]
        );
    }

    #[test]
    fn compact_density_sits_flush_against_window_chrome() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(
            [
                (
                    "betweenTitleAndStatusBars",
                    margins(|service| {
                        service.modern_ui_compact = true;
                    }),
                ),
                (
                    "atWindowEdges",
                    margins(|service| {
                        service.modern_ui_compact = true;
                        service.visible_parts_mut().remove(&Parts::TitlebarPart);
                        service.visible_parts_mut().remove(&Parts::StatusbarPart);
                    }),
                ),
            ],
            [("betweenTitleAndStatusBars", vertical(0.0, 0.0)), ("atWindowEdges", vertical(0.0, 0.0))]
        );
    }
}
