//! Porte de `src/vs/workbench/browser/parts/editor/multiRowEditorTabsControl.ts`: duas barras de abas sobre o mesmo grupo, a das
//! abas fixadas (`StickyEditorGroupModel`) e a das não fixadas (`UnstickyEditorGroupModel`), cada uma um
//! [`MultiEditorTabsControlState`].
//!
//! Divergências:
//! - **Contrato.** O original implementa `IEditorTabsControl`. O trait Rust (`IEditorTabsControl`, em `editor_tabs_control.rs`) é de
//!   `&self`, e o controle de uma barra é um estado de `&mut self` que recebe como argumento o que o navegador mede
//!   ([`TabsMeasurer`]) e a lista de editores que o original ignora. Por isso o controle de duas barras expõe as mesmas
//!   operações, com os mesmos nomes e a mesma ordem de chamadas às duas barras, mas em `&mut self`:
//!   - `openEditors(editors)` é [`open_editors`](MultiRowEditorControl::open_editors) e `closeEditor(editor)` e
//!     `closeEditors(editors)` são [`close_editors`](MultiRowEditorControl::close_editors): o controle de uma barra relê o modelo
//!     de abas e descarta a lista, então a divisão em fixados e não fixados do original não tem efeito observável.
//!   - `setActive(isActive)` é [`set_active`](MultiRowEditorControl::set_active): quem diz se o grupo está ativo é o
//!     [`TabsControlHost`], como na barra única.
//!   - `layout(dimensions)` e as operações que refazem o layout (`updateEditorDirty`, `updateEditorCapabilities`,
//!     `updateEditorSelections`) recebem o [`TabsMeasurer`], e `getHeight()` também.
//! - **Elemento pai.** O `HTMLElement` pai que as duas barras compartilham guarda a classe `two-tab-bars` e a
//!   `connected-tabs-wrapping`, e é ele que diz a cada barra se ela é a primeira não vazia (`firstVisibleTabBar`) e se a irmã
//!   quebra linhas. Aqui o controle guarda as duas classes ([`two_tab_bars`](MultiRowEditorControl::two_tab_bars) e
//!   [`connected_tabs_wrapping`](MultiRowEditorControl::connected_tabs_wrapping)) e entrega a cada barra o seu
//!   [`TabsBarContext`] antes do layout agendado dela.
//! - **Quadro de animação.** Cada barra agenda o próprio layout (`scheduleAtNextAnimationFrame`);
//!   [`run_frame`](MultiRowEditorControl::run_frame) executa os layouts agendados, a barra fixada primeiro (a ordem em que o
//!   `layout` agenda).
//! - **`groupView`** é o [`TabsControlHost`] (`count`, `relayout`) mais o modelo (`stickyCount`).
//! - **`IInstantiationService`** não existe aqui: as duas barras são montadas direto.

use std::cell::Cell;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_editor::core_2d::dimension::Dimension;

use crate::browser::parts::editor::editor_tabs_control::{EditorTabsControlBase, EditorTitleControlDimensions, TabsControlHost, TabsOpenOptions, TabsParentTraits};
use crate::browser::parts::editor::multi_editor_tabs_control::{MultiEditorTabsControlState, TabsBarContext, TabsMeasurer, TabsModelKind, UpdateOptionsOutcome};
use crate::common::editor::EditorInputRef;
use crate::common::editor::editor_group_model::{EditorOrIndex, IReadonlyEditorGroupModel};
use crate::common::editor::filtered_editor_group_model::{StickyEditorGroupModel, UnstickyEditorGroupModel};
use crate::services::editor::editor_groups_service::EditorPartOptions;

/// Uma das duas barras: `stickyEditorTabsControl` ou `unstickyEditorTabsControl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsBar {
    Sticky,
    Unsticky,
}

/// `MultiRowEditorControl`.
pub struct MultiRowEditorControl {
    base: Rc<Disposable>,
    host: Rc<dyn TabsControlHost>,
    model: Rc<dyn IReadonlyEditorGroupModel>,
    /// `stickyEditorTabsControl`.
    pub sticky: MultiEditorTabsControlState,
    /// `unstickyEditorTabsControl`.
    pub unsticky: MultiEditorTabsControlState,
    /// `activeControl`.
    active_control: Option<TabsBar>,
    /// `parent.classList.contains('two-tab-bars')`.
    two_tab_bars: Cell<bool>,
    /// `parent.classList.contains('connected-tabs-wrapping')`: a última barra que fez o layout das abas escreve o valor.
    connected_tabs_wrapping: bool,
}

impl MultiRowEditorControl {
    pub fn new(host: Rc<dyn TabsControlHost>, model: Rc<dyn IReadonlyEditorGroupModel>, parent: TabsParentTraits, breadcrumbs_in_header: bool, use_modern_ui_tabs: bool) -> Self {
        let base = Disposable::new();
        let sticky_model = StickyEditorGroupModel::new(model.clone());
        let unsticky_model = UnstickyEditorGroupModel::new(model.clone());
        base.register(sticky_model.clone());
        base.register(unsticky_model.clone());

        let create_bar = |tabs_model: Rc<dyn IReadonlyEditorGroupModel>, kind: TabsModelKind| {
            let bar_base = EditorTabsControlBase::new(host.clone(), tabs_model, parent, breadcrumbs_in_header, use_modern_ui_tabs);
            let mut state = MultiEditorTabsControlState::new(bar_base);
            state.model_kind = kind;
            // `create(parent)` fecha com `updateTabsControlVisibility()`.
            state.update_tabs_control_visibility();

            state
        };
        let sticky = create_bar(sticky_model, TabsModelKind::Sticky);
        let unsticky = create_bar(unsticky_model, TabsModelKind::Unsticky);

        let mut control = Self { base, host, model, sticky, unsticky, active_control: None, two_tab_bars: Cell::new(false), connected_tabs_wrapping: false };
        control.handle_tab_bars_state_change();

        control
    }

    /// `parent.classList.contains('two-tab-bars')`.
    pub fn two_tab_bars(&self) -> bool {
        self.two_tab_bars.get()
    }

    /// `parent.classList.contains('connected-tabs-wrapping')`.
    pub fn connected_tabs_wrapping(&self) -> bool {
        self.connected_tabs_wrapping
    }

    fn bar(&self, bar: TabsBar) -> &MultiEditorTabsControlState {
        match bar {
            TabsBar::Sticky => &self.sticky,
            TabsBar::Unsticky => &self.unsticky,
        }
    }

    fn bar_mut(&mut self, bar: TabsBar) -> &mut MultiEditorTabsControlState {
        match bar {
            TabsBar::Sticky => &mut self.sticky,
            TabsBar::Unsticky => &mut self.unsticky,
        }
    }

    /// `getEditorTabsController(editor)`.
    pub fn tabs_bar_of(&self, editor: &EditorInputRef) -> TabsBar {
        if self.model.is_sticky(&EditorOrIndex::Editor(editor.clone())) { TabsBar::Sticky } else { TabsBar::Unsticky }
    }

    /// `this.model.activeEditor ? this.getEditorTabsController(this.model.activeEditor) : undefined`.
    fn active_bar(&self) -> Option<TabsBar> {
        self.model.active_editor().map(|editor| self.tabs_bar_of(&editor))
    }

    /// `handleTabBarsStateChange`.
    fn handle_tab_bars_state_change(&mut self) {
        self.active_control = self.active_bar();
        self.handle_tab_bars_layout_change();
    }

    /// `handleTabBarsLayoutChange`.
    fn handle_tab_bars_layout_change(&self) {
        if self.host.group_count() == 0 {
            // Do nothing as no tab bar is visible
            return;
        }

        let had_two_tab_bars = self.two_tab_bars.get();
        let sticky_count = self.model.sticky_count();
        let has_two_tab_bars = self.host.group_count() != sticky_count && sticky_count > 0;

        // Ensure action toolbar is only visible once
        self.two_tab_bars.set(has_two_tab_bars);

        if had_two_tab_bars != has_two_tab_bars {
            self.host.relayout();
        }
    }

    /// `didActiveControlChange`.
    fn did_active_control_change(&self) -> bool {
        self.active_control != self.active_bar()
    }

    /// `openEditor`.
    pub fn open_editor(&mut self, editor: &EditorInputRef, options: Option<TabsOpenOptions>) -> bool {
        let did_active_control_change = self.did_active_control_change();
        let bar = self.tabs_bar_of(editor);
        let did_open_editor_change = self.bar_mut(bar).open_editor(editor, options);

        let did_change = did_open_editor_change || did_active_control_change;
        if did_change {
            self.handle_tab_bars_state_change();
        }

        did_change
    }

    /// `openEditors`.
    pub fn open_editors(&mut self) -> bool {
        let did_active_control_change = self.did_active_control_change();
        let did_change_open_editors_sticky = self.sticky.open_editors();
        let did_change_open_editors_unsticky = self.unsticky.open_editors();

        let did_change = did_change_open_editors_sticky || did_change_open_editors_unsticky || did_active_control_change;
        if did_change {
            self.handle_tab_bars_state_change();
        }

        did_change
    }

    /// `beforeCloseEditor`.
    pub fn before_close_editor(&mut self, editor: &EditorInputRef) {
        let bar = self.tabs_bar_of(editor);
        self.bar_mut(bar).before_close_editor(editor);
    }

    /// `closeEditor` e `closeEditors`.
    pub fn close_editors(&mut self) {
        // Has to be called on both tab bars as the editor could be either sticky or not
        self.sticky.handle_closed_editors();
        self.unsticky.handle_closed_editors();

        self.handle_tab_bars_state_change();
    }

    /// `moveEditor`.
    pub fn move_editor(&mut self, editor: &EditorInputRef, from_index: usize, target_index: usize, sticky_state_change: bool) {
        if sticky_state_change {
            // If sticky state changes, move editor between tab bars
            let to_sticky = self.model.is_sticky(&EditorOrIndex::Editor(editor.clone()));
            self.move_between_bars(editor, to_sticky);
        } else if self.model.is_sticky(&EditorOrIndex::Editor(editor.clone())) {
            self.sticky.move_editor(from_index, target_index);
        } else {
            let sticky_count = self.model.sticky_count();
            self.unsticky.move_editor(from_index.saturating_sub(sticky_count), target_index.saturating_sub(sticky_count));
        }
    }

    /// O editor sai de uma barra e entra na outra: `closeEditor` na de origem, `openEditor` na de destino e o estado das barras.
    fn move_between_bars(&mut self, editor: &EditorInputRef, to_sticky: bool) {
        let (from, to) = if to_sticky { (&mut self.unsticky, &mut self.sticky) } else { (&mut self.sticky, &mut self.unsticky) };
        from.handle_closed_editors();
        to.open_editor(editor, None);

        self.handle_tab_bars_state_change();
    }

    /// `pinEditor`.
    pub fn pin_editor(&mut self, editor: &EditorInputRef) {
        let bar = self.tabs_bar_of(editor);
        self.bar_mut(bar).pin_editor(editor);
    }

    /// `stickEditor`.
    pub fn stick_editor(&mut self, editor: &EditorInputRef) {
        self.move_between_bars(editor, true);
    }

    /// `unstickEditor`.
    pub fn unstick_editor(&mut self, editor: &EditorInputRef) {
        self.move_between_bars(editor, false);
    }

    /// `setActive`.
    pub fn set_active(&mut self) {
        self.sticky.set_active();
        self.unsticky.set_active();
    }

    /// `updateEditorSelections`.
    pub fn update_editor_selections(&mut self, measurer: &dyn TabsMeasurer) {
        let sticky_height = self.sticky.computed_height(measurer);
        self.sticky.update_editor_selections(sticky_height);
        let unsticky_height = self.unsticky.computed_height(measurer);
        self.unsticky.update_editor_selections(unsticky_height);
    }

    /// `updateEditorLabel`: devolve se o pedido acorda o agendador de rótulos da barra do editor.
    pub fn update_editor_label(&self, editor: &EditorInputRef) -> bool {
        let bar = self.tabs_bar_of(editor);

        self.bar(bar).update_editor_label()
    }

    /// `updateEditorCapabilities`.
    pub fn update_editor_capabilities(&mut self, editor: &EditorInputRef, measurer: &dyn TabsMeasurer) {
        let bar = self.tabs_bar_of(editor);
        let computed_height = self.bar(bar).computed_height(measurer);
        self.bar_mut(bar).update_editor_capabilities(editor, computed_height);
    }

    /// `updateEditorDirty`.
    pub fn update_editor_dirty(&mut self, editor: &EditorInputRef, measurer: &dyn TabsMeasurer) {
        let bar = self.tabs_bar_of(editor);
        let computed_height = self.bar(bar).computed_height(measurer);
        self.bar_mut(bar).update_editor_dirty(editor, computed_height);
    }

    /// `updateOptions`: o que cada barra pede ao desenho nativo, a fixada primeiro.
    pub fn update_options(&mut self, old_options: &EditorPartOptions, new_options: &EditorPartOptions) -> (UpdateOptionsOutcome, UpdateOptionsOutcome) {
        let sticky = self.sticky.update_options(old_options, new_options);
        let unsticky = self.unsticky.update_options(old_options, new_options);

        (sticky, unsticky)
    }

    /// `layout`: a barra fixada ocupa o que precisa e a outra recebe o espaço que sobra.
    pub fn layout(&mut self, dimensions: EditorTitleControlDimensions, measurer: &dyn TabsMeasurer) -> Dimension {
        let sticky_computed_height = self.sticky.computed_height(measurer);
        let sticky_height = self.sticky.layout(dimensions, None, sticky_computed_height).map_or(0.0, |used| used.height);

        let unsticky_available_dimensions = EditorTitleControlDimensions {
            container: dimensions.container,
            available: dimensions.available.map(|available| Dimension { width: available.width, height: available.height - sticky_height }),
        };
        let unsticky_computed_height = self.unsticky.computed_height(measurer);
        let unsticky_height = self.unsticky.layout(unsticky_available_dimensions, None, unsticky_computed_height).map_or(0.0, |used| used.height);

        Dimension { width: dimensions.container.map_or(0.0, |container| container.width), height: sticky_height + unsticky_height }
    }

    /// `getHeight`.
    pub fn get_height(&self, measurer: &dyn TabsMeasurer) -> f64 {
        self.sticky.get_height(self.sticky.computed_height(measurer)) + self.unsticky.get_height(self.unsticky.computed_height(measurer))
    }

    /// O que `doLayoutTabs` lê do elemento pai de cada barra: se ela é a primeira não vazia, se há `two-tab-bars` e se a irmã
    /// quebra linhas.
    fn tabs_bar_context(&self, bar: TabsBar) -> TabsBarContext {
        let (this, other) = match bar {
            TabsBar::Sticky => (&self.sticky, &self.unsticky),
            TabsBar::Unsticky => (&self.unsticky, &self.sticky),
        };

        // `firstVisibleTabBar === this.tabsAndActionsContainer`: a barra fixada vem antes no pai.
        TabsBarContext { top_tab_bar: !this.empty && (bar == TabsBar::Sticky || other.empty), two_tab_bars: self.two_tab_bars.get(), other_bar_wrapping: other.container.wrapping }
    }

    /// O quadro de animação das duas barras: cada uma recebe o contexto do pai, executa o layout agendado e, se fez o layout
    /// das abas, escreve o `connected-tabs-wrapping` do pai. Uma mudança de altura pede `groupView.relayout()`.
    pub fn run_frame(&mut self, measurer: &dyn TabsMeasurer) {
        let mut relayout = false;
        for bar in [TabsBar::Sticky, TabsBar::Unsticky] {
            let context = self.tabs_bar_context(bar);
            self.bar_mut(bar).container.bars = context;

            let outcome = self.bar_mut(bar).run_frame(measurer);
            if outcome.laid_out_tabs {
                self.connected_tabs_wrapping = self.bar(bar).container.connected_tabs_wrapping;
            }
            relayout |= outcome.relayout;
        }

        if relayout {
            self.host.relayout();
        }
    }
}

impl IDisposable for MultiRowEditorControl {
    fn dispose(&self) {
        self.two_tab_bars.set(false);

        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MultiRowEditorControl"
    }
}
