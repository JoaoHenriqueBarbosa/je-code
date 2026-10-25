//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellFocusIndicator.ts`: `CellFocusIndicator`, as faixas de foco
//! (esquerda, direita, topo e base) da célula e os dois indicadores clicáveis (entrada e saída) que recolhem a entrada ou a saída.
//!
//! Divergências:
//! - `FastDomNode<HTMLElement>` é o [`IndicatorNode`]: um nó da árvore do `je-ui` com os dois escritores que a parte usa,
//!   `setHeight` e `style.transform = translateY(...)`. Como a árvore não tem `transform`, `translateY(y)` desloca o `y0` do nó em `y`
//!   sobre a posição que ele tinha quando o [`IndicatorNode`] foi criado (o `top` do CSS), mantendo a altura.
//! - `DOM.append(left.domNode, DOM.$('.codeOutput-focus-indicator-container', undefined, DOM.$('.codeOutput-focus-indicator.code-focus-indicator')))`
//!   é um `create_el` do contêiner com o indicador dentro; o `FastDomNode` é o contêiner (o que `DOM.append` devolve).
//! - `e.offsetY` do duplo clique é a coordenada y do ponteiro menos o topo absoluto do alvo; `e.target !== this.left.domNode` compara
//!   o nó alvo.
//! - `INotebookEditorDelegate` é o contrato estreito [`ICellFocusIndicatorEditor`] (`hasModel`, `notebookOptions` e
//!   `textModel?.viewType`) e `CellTitleToolbarPart` (de `cellToolbars.ts`, próxima fatia) é o contrato [`ICellTitleToolbar`]
//!   (`onDidUpdateActions` e `hasActions`).
//! - `(this.currentCell.layoutInfo as CodeCellLayoutInfo).outputContainerOffset` numa célula de markup é `undefined` no original, e
//!   `offsetY < undefined` é falso: o duplo clique recolhe a saída.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::event::Event;
use je_render::kurbo::Rect;
use je_ui::{Event as UiEvent, EventKind, NodeId, Role, Ui};
use je_widgets::dom::create_el;

use crate::contrib::notebook::browser::notebook_options::NotebookOptions;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;

/// O que `CellFocusIndicator` chama do `INotebookEditorDelegate`.
pub trait ICellFocusIndicatorEditor {
    /// `hasModel()`.
    fn has_model(&self) -> bool;
    /// `notebookOptions`.
    fn notebook_options(&self) -> Rc<NotebookOptions>;
    /// `textModel?.viewType`.
    fn text_model_view_type(&self) -> Option<String>;
}

/// O que `CellFocusIndicator` chama do `CellTitleToolbarPart`.
pub trait ICellTitleToolbar {
    /// `onDidUpdateActions`.
    fn on_did_update_actions(&self) -> Event<()>;
    /// `hasActions`.
    fn has_actions(&self) -> bool;
}

/// `FastDomNode<HTMLElement>`, só com `setHeight` e o `transform: translateY(...)` (ver as divergências do módulo).
pub struct IndicatorNode {
    ui: Ui,
    node: NodeId,
    /// O `y0` do nó quando o `IndicatorNode` foi criado: a origem do `translateY`.
    base_top: Cell<f64>,
}

impl IndicatorNode {
    pub fn new(ui: &Ui, node: NodeId) -> Rc<Self> {
        let base_top = ui.tree().bounds(node).y0;
        Rc::new(Self { ui: ui.clone(), node, base_top: Cell::new(base_top) })
    }

    /// `domNode`.
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// `setHeight(height)`.
    pub fn set_height(&self, height: f64) {
        let mut tree = self.ui.tree_mut();
        let bounds = tree.bounds(self.node);
        tree.set_bounds(self.node, Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y0 + height));
    }

    /// `domNode.style.transform = translateY(${y}px)`.
    pub fn set_translate_y(&self, y: f64) {
        let mut tree = self.ui.tree_mut();
        let bounds = tree.bounds(self.node);
        let top = self.base_top.get() + y;
        tree.set_bounds(self.node, Rect::new(bounds.x0, top, bounds.x1, top + bounds.height()));
    }
}

/// `layoutInfo.chatHeight` de qualquer uma das duas formas de célula.
fn chat_height(cell: &CellViewModel) -> f64 {
    match cell {
        CellViewModel::Code(cell) => cell.layout_info().chat_height,
        CellViewModel::Markup(cell) => cell.layout_info().chat_height,
    }
}

/// `CellFocusIndicator`.
pub struct CellFocusIndicator {
    state: CellPartState,
    notebook_editor: Rc<dyn ICellFocusIndicatorEditor>,
    title_toolbar: Rc<dyn ICellTitleToolbar>,
    pub top: Rc<IndicatorNode>,
    pub left: Rc<IndicatorNode>,
    pub right: Rc<IndicatorNode>,
    pub bottom: Rc<IndicatorNode>,
    pub code_focus_indicator: Rc<IndicatorNode>,
    pub output_focus_indicator: Rc<IndicatorNode>,
}

impl CellFocusIndicator {
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn ICellFocusIndicatorEditor>,
        title_toolbar: Rc<dyn ICellTitleToolbar>,
        top: Rc<IndicatorNode>,
        left: Rc<IndicatorNode>,
        right: Rc<IndicatorNode>,
        bottom: Rc<IndicatorNode>,
    ) -> Rc<Self> {
        let (code_focus_indicator, output_focus_indicator) = {
            let mut tree = ui.tree_mut();
            let mut append_indicator = |inner_class_name: &str| {
                let container = create_el(&mut tree, left.node(), Role::Generic, "codeOutput-focus-indicator-container");
                create_el(&mut tree, container, Role::Generic, inner_class_name);
                container
            };
            (append_indicator("codeOutput-focus-indicator code-focus-indicator"), append_indicator("codeOutput-focus-indicator output-focus-indicator"))
        };
        let code_focus_indicator = IndicatorNode::new(ui, code_focus_indicator);
        let output_focus_indicator = IndicatorNode::new(ui, output_focus_indicator);

        Rc::new_cyclic(|me: &Weak<Self>| {
            let state = CellPartState::new();

            {
                let me = me.clone();
                state.register(ui.listen(code_focus_indicator.node(), EventKind::Click, false, move |_| {
                    if let Some(cell) = me.upgrade().and_then(|part| part.state.current_cell()) {
                        cell.base().set_input_collapsed(!cell.base().is_input_collapsed());
                    }
                }));
            }
            {
                let me = me.clone();
                state.register(ui.listen(output_focus_indicator.node(), EventKind::Click, false, move |_| {
                    if let Some(cell) = me.upgrade().and_then(|part| part.state.current_cell()) {
                        cell.base().set_output_collapsed(!cell.base().is_output_collapsed());
                    }
                }));
            }

            {
                let (me, ui, left) = (me.clone(), ui.clone(), left.node());
                state.register(ui.clone().listen(left, EventKind::DoubleClick, false, move |e: &mut UiEvent| {
                    let Some(part) = me.upgrade() else { return };
                    let Some(cell) = part.state.current_cell() else { return };
                    if !part.notebook_editor.has_model() {
                        return;
                    }

                    if e.target != left {
                        // Don't allow dblclick on the codeFocusIndicator/outputFocusIndicator
                        return;
                    }

                    let offset_y = e.pointer.map_or(0.0, |pointer| pointer.y) - ui.tree().absolute_bounds(left).y0;
                    let clicked_on_input = cell.as_code().is_some_and(|code| offset_y < code.layout_info().output_container_offset);
                    if clicked_on_input {
                        cell.base().set_input_collapsed(!cell.base().is_input_collapsed());
                    } else {
                        cell.base().set_output_collapsed(!cell.base().is_output_collapsed());
                    }
                }));
            }

            {
                let me = me.clone();
                state.register(title_toolbar.on_did_update_actions().listen(move |_| {
                    if let Some(part) = me.upgrade() {
                        part.update_focus_indicators_for_title_menu();
                    }
                }));
            }

            Self { state, notebook_editor, title_toolbar, top, left, right, bottom, code_focus_indicator, output_focus_indicator }
        })
    }

    fn update_focus_indicators_for_title_menu(&self) {
        let y = self.state.current_cell().map_or(0.0, |cell| chat_height(&cell)) + self.get_indicator_top_margin();
        self.left.set_translate_y(y);
        self.right.set_translate_y(y);
    }

    fn get_indicator_top_margin(&self) -> f64 {
        let layout_info = self.notebook_editor.notebook_options().get_layout_configuration();

        if self.title_toolbar.has_actions() { layout_info.editor_toolbar_height + layout_info.cell_top_margin } else { layout_info.cell_top_margin }
    }
}

impl CellPart for CellFocusIndicator {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn update_internal_layout_now(&self, element: &CellViewModel) {
        let options = self.notebook_editor.notebook_options();
        let view_type = self.notebook_editor.text_model_view_type();

        match element {
            CellViewModel::Markup(cell) => {
                let layout_info = cell.layout_info();
                let indicator_position = options.compute_indicator_position(layout_info.total_height, layout_info.fold_hint_height, view_type.as_deref());
                self.bottom.set_translate_y(indicator_position.bottom_indicator_top + 6.0);
                self.left.set_height(indicator_position.vertical_indicator_height);
                self.right.set_height(indicator_position.vertical_indicator_height);
                self.code_focus_indicator.set_height(indicator_position.vertical_indicator_height - self.get_indicator_top_margin() * 2.0 - layout_info.chat_height);
            }
            CellViewModel::Code(cell) => {
                let layout_info = cell.layout_info();
                let layout_configuration = options.get_layout_configuration();
                let bottom_toolbar_dimensions = options.compute_bottom_toolbar_dimensions(view_type.as_deref());
                let indicator_height = layout_info.code_indicator_height + layout_info.output_indicator_height + layout_info.comment_height;
                self.left.set_height(indicator_height);
                self.right.set_height(indicator_height);
                self.code_focus_indicator.set_height(layout_info.code_indicator_height);
                self.output_focus_indicator.set_height((layout_info.output_indicator_height - cell.view_context.notebook_options.get_layout_configuration().focus_indicator_gap).max(0.0));
                self.bottom.set_translate_y(layout_info.total_height - bottom_toolbar_dimensions.bottom_toolbar_gap - layout_configuration.cell_bottom_margin);
            }
        }

        self.update_focus_indicators_for_title_menu();
    }
}

impl_cell_content_part!(CellFocusIndicator, "CellFocusIndicator");
