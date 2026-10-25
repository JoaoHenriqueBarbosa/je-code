//! Contratos de `src/vs/workbench/browser/parts/paneCompositePart.ts` (`IPaneCompositePart`) e de
//! `paneCompositeBar.ts` (`IPaneCompositeBarOptions`, `PaneCompositeBar`) e `globalCompositeBar.ts`
//! (`GlobalCompositeBar`), no recorte que a barra de atividades lê.
//!
//! Divergências:
//! - **`PaneCompositeBar` e `GlobalCompositeBar` como traits.** As classes do original montam DOM; aqui o que a
//!   `ActivitybarPart` e a `ActivityBarCompositeBar` usam delas é o contrato: [`IPaneCompositeBar`] e
//!   [`IGlobalCompositeBar`]. A fórmula de `GlobalCompositeBar.getHeight` é [`global_composite_bar_height`].
//! - **`HTMLElement`.** O contêiner onde a barra se cria é o [`ContentElement`] da parte (o `.content` do original), um
//!   estado nativo com o tamanho que `size(element, w, h)` aplica.
//! - **`fillExtraContextMenuActions(actions, e?: MouseEvent | GestureEvent)`.** O evento do ponteiro não tem equivalente
//!   nativo; do `e?.target` o gancho só precisa saber se o alvo está dentro do menu compacto, e isso é o
//!   [`ContextMenuTarget`] (ausente quando o menu foi aberto sem ponteiro).
//! - **`IPaneCompositePart`** traz o que a barra consulta da parte (`partId` e as listas de identificadores).

use std::cell::Cell;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_editor::services::theme_service::IColorTheme;
use je_theme::color::Color;
use je_ui::NodeId;
use je_widgets::action_bar::ActionsOrientation;
use je_widgets::hover_api::HoverPosition;

use crate::browser::dnd::{CompositeDragAndDropData, DragEvent};
use crate::services::layout::layout_service::Parts;

/// O que `fillExtraContextMenuActions` lê do `e.target`: o resultado de `isAncestor(e.target, menuBarContainer)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContextMenuTarget {
    /// O alvo do ponteiro está dentro do contêiner do menu compacto.
    pub in_menu_bar_container: bool,
}

/// `IPaneCompositePart`, no recorte que a barra de painéis lê.
pub trait IPaneCompositePart {
    /// `partId`.
    fn part_id(&self) -> Parts;

    /// `getPinnedPaneCompositeIds()`.
    fn get_pinned_pane_composite_ids(&self) -> Vec<String>;

    /// `getVisiblePaneCompositeIds()`.
    fn get_visible_pane_composite_ids(&self) -> Vec<String>;

    /// `getPaneCompositeIds()`.
    fn get_pane_composite_ids(&self) -> Vec<String>;
}

/// O `.content` da parte: o contêiner que recebe a barra de painéis. `size(element, width, height)` do original.
#[derive(Default)]
pub struct ContentElement {
    width: Cell<f64>,
    height: Cell<f64>,
    node: Cell<Option<NodeId>>,
}

impl ContentElement {
    /// O nó da árvore nativa que o elemento representa (onde `appendChild` põe a barra), quando a parte já o tem.
    pub fn node(&self) -> Option<NodeId> {
        self.node.get()
    }

    /// Liga o elemento ao nó da árvore nativa.
    pub fn set_node(&self, node: NodeId) {
        self.node.set(Some(node));
    }

    /// `size(element, width, height)`.
    pub fn set_size(&self, width: f64, height: f64) {
        self.width.set(width);
        self.height.set(height);
    }

    /// A largura aplicada em `size`.
    pub fn width(&self) -> f64 {
        self.width.get()
    }

    /// A altura aplicada em `size`.
    pub fn height(&self) -> f64 {
        self.height.get()
    }
}

/// O `{ readonly actionHeight: number; readonly actionGap: number }` de `globalActivities`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalActivities {
    pub action_height: f64,
    pub action_gap: f64,
}

/// `IPaneCompositeBarOptions.colors(theme)`: o resultado de `ICompositeBarColors`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompositeBarColors {
    pub active_background_color: Option<Color>,
    pub inactive_background_color: Option<Color>,
    pub active_border_bottom_color: Option<Color>,
    pub active_foreground_color: Option<Color>,
    pub inactive_foreground_color: Option<Color>,
    pub active_border_color: Option<Color>,
    pub active_background: Option<Color>,
    pub badge_background: Option<Color>,
    pub badge_foreground: Option<Color>,
    pub drag_and_drop_border: Option<Color>,
}

/// O `activityHoverOptions` de `IPaneCompositeBarOptions`: a posição do hover, calculada a cada uso.
#[derive(Clone)]
pub struct ActivityHoverOptions {
    pub position: Rc<dyn Fn() -> HoverPosition>,
}

/// `IPaneCompositeBarOptions` (`ICompositeBarOptions` e as chaves de armazenamento).
#[derive(Clone)]
pub struct PaneCompositeBarOptions {
    pub part_container_class: String,
    pub pinned_view_containers_key: String,
    pub placeholder_view_containers_key: String,
    pub view_containers_workspace_state_key: String,
    pub orientation: ActionsOrientation,
    pub icon: bool,
    pub icon_size: f64,
    pub activity_hover_options: ActivityHoverOptions,
    pub prevent_loop_navigation: bool,
    pub recompute_sizes: bool,
    pub fill_extra_context_menu_actions: Rc<dyn Fn(&mut Vec<ActionRef>, Option<ContextMenuTarget>)>,
    pub composite_size: f64,
    pub colors: Rc<dyn Fn(&dyn IColorTheme) -> CompositeBarColors>,
    pub overflow_action_size: f64,
    /// `compact` (do `ICompositeBarOptions`): ausente é falso.
    pub compact: bool,
}

/// O que `PaneCompositeBar` expõe à parte que a hospeda.
pub trait IPaneCompositeBar: IDisposable {
    /// `create(parent)`.
    fn create(&self, parent: &Rc<ContentElement>);

    /// `layout(width, height)`.
    fn layout(&self, width: f64, height: f64);

    /// `focus(index?)`.
    fn focus(&self, index: Option<usize>);

    /// `getPinnedPaneCompositeIds()`.
    fn get_pinned_pane_composite_ids(&self) -> Vec<String>;

    /// `getVisiblePaneCompositeIds()`.
    fn get_visible_pane_composite_ids(&self) -> Vec<String>;

    /// `getPaneCompositeIds()`.
    fn get_pane_composite_ids(&self) -> Vec<String>;

    /// `getContextMenuActions()`: as ações do menu de contexto da barra (o padrão é a barra sem menu).
    fn get_context_menu_actions(&self) -> Vec<ActionRef> {
        Vec::new()
    }

    /// `dndHandler.onDragEnter(data, undefined, event)`: se o dado arrastado pode ser solto na barra.
    fn on_drag_enter(&self, _data: &CompositeDragAndDropData, _event: &Rc<DragEvent>) -> bool {
        false
    }

    /// `dndHandler.drop(data, undefined, event)`.
    fn on_drop(&self, _data: &CompositeDragAndDropData, _event: &Rc<DragEvent>) {}
}

/// O que `GlobalCompositeBar` expõe à `ActivityBarCompositeBar`.
pub trait IGlobalCompositeBar: IDisposable {
    /// `getHeight(actionHeight, actionGap)`: ver [`global_composite_bar_height`].
    fn get_height(&self, action_height: f64, action_gap: f64) -> f64;

    /// `element.clientWidth`.
    fn client_width(&self) -> f64;

    /// `create(parent)`.
    fn create(&self, parent: &Rc<ContentElement>);

    /// `focus()`.
    fn focus(&self);

    /// `getContextMenuActions()`.
    fn get_context_menu_actions(&self) -> Vec<ActionRef>;

    /// `onDidChange`.
    fn on_did_change(&self) -> Event<()>;
}

/// O corpo de `GlobalCompositeBar.getHeight`: `count` ações empilhadas, com `actionGap` entre duas vizinhas. Não mede o
/// DOM, para o layout não forçar medição síncrona.
pub fn global_composite_bar_height(count: usize, action_height: f64, action_gap: f64) -> f64 {
    count as f64 * action_height + (count.saturating_sub(1)) as f64 * action_gap
}
