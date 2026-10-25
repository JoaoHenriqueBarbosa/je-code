//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/foldedCellHint.ts`: `FoldedCellHint`, a dica que uma célula de
//! markup recolhida (por um cabeçalho dobrado) mostra: o botão de executar a seção, o texto "N cells hidden" e o botão de expandir.
//!
//! Divergências:
//! - **DOM é a árvore do `je-ui`** e não há motor de layout: o `display: flex; align-items: center` do `.notebook-folded-hint` é a
//!   posição calculada em `layout_children` (botão de executar, rótulo e botão de expandir, da esquerda para a direita, na altura
//!   `foldHintHeight` da célula). As medidas são as de `notebookFolding.css` e `notebook.css`: botões de 16px com `padding: 2px`
//!   (20px), `margin-right: 4px` no de executar e `margin-left: 4px` no de expandir, rótulo com `opacity: 0.7`. O `:hover` dos botões
//!   não é pintado. `style.top = '...px'` é o [`StickyTopNode`] (o mesmo substituto de `FastDomNode` de `cellToolbarStickyScroll.ts`).
//! - `DOM.reset(container, ...)` é `clear_node` e a criação dos filhos em ordem; `DOM.$('span.classe')` é `create_el`.
//! - **Ícones por classe** (`classList.add(...ThemeIcon.asClassNameArray(icon))`) são o span de ícone de `render_icon` dentro do nó do
//!   botão, e `className = ''` mais as classes novas é refazer o ícone do nó.
//! - `INotebookEditor` é o contrato estreito [`IFoldedCellHintEditor`], e o `FoldingController` (`controller/foldingController.ts`, sem
//!   porte) é o contrato [`INotebookFoldingController`]. `hasModel()` mais `getViewModel()` é [`IFoldedCellHintEditor::active_view_model`].
//! - `executingStateIcon` e `ThemeIcon.modify(..., 'spin')` vêm de `code_cell_execution_icon.rs` (`notebookIcons.ts`, sem porte).
//! - `typeof idx === 'number'` do `getCellIndex` é o `Option` de [`IFoldedCellHintEditor::get_cell_index`].
//! - `element: MarkupCellViewModel` é o [`CellViewModel`] com `as_markup()`; uma célula de código não faz nada.
//! - A primeira chamada de `getRunFoldedSectionButton` sem células de código devolve `undefined` e deixa os ouvintes anteriores
//!   como estavam (o original só os troca quando cria o botão); o porte mantém isso.

use std::rc::{Rc, Weak};

use je_base::lifecycle::{IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_render::kurbo::Rect;
use je_ui::{EventKind, NodeId, Role, Ui};
use je_widgets::dom::{clear_node, create_el, hide, show};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::icon_labels::{InlineStyle, LabelPart, Rendered, build_parts, layout_parts, render_icon};
use je_widgets::text::WORKBENCH_LINE_HEIGHT;

use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view::cell_parts::cell_toolbar_sticky_scroll::StickyTopNode;
use crate::contrib::notebook::browser::view::cell_parts::code_cell_execution_icon::spinning_executing_state_icon;
use crate::contrib::notebook::browser::view_model::cell_view_model_types::CellFoldingState;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use crate::contrib::notebook::common::notebook_common::{CellKind, NotebookCellExecutionState};
use crate::contrib::notebook::common::notebook_execution_state_service::INotebookExecutionStateService;
use crate::contrib::notebook::common::notebook_range::ICellRange;

/// O lado da caixa de um botão da dica: o ícone de 16px mais o `padding: 2px` dos dois lados.
const BUTTON_SIZE: f64 = 20.0;
/// O lado do ícone dentro do botão.
const BUTTON_ICON_SIZE: f64 = 16.0;
/// O `padding: 2px` do botão.
const BUTTON_PADDING: f64 = 2.0;
/// O `margin-right: 4px` do botão de executar e o `margin-left: 4px` do de expandir.
const BUTTON_MARGIN: f64 = 4.0;

/// O `FoldingController` (`controller/foldingController.ts`), só com o que `FoldedCellHint` chama.
pub trait INotebookFoldingController {
    /// `setFoldingStateDown(index, state, levels)`.
    fn set_folding_state_down(&self, index: i64, state: CellFoldingState, levels: i64);
}

/// O que `FoldedCellHint` chama do `INotebookEditor`.
pub trait IFoldedCellHintEditor {
    /// `hasModel()` mais `getViewModel()`: `None` sem modelo.
    fn active_view_model(&self) -> Option<Rc<NotebookViewModel>>;
    /// `getCellIndex(cell)`: `None` quando a célula não está no editor.
    fn get_cell_index(&self, cell: &CellViewModel) -> Option<i64>;
    /// `getCellsInRange(range)`.
    fn get_cells_in_range(&self, range: ICellRange) -> Vec<CellViewModel>;
    /// `executeNotebookCells(cells)`.
    fn execute_notebook_cells(&self, cells: Vec<CellViewModel>);
    /// `getContribution<FoldingController>(FoldingController.id)`.
    fn folding_controller(&self) -> Rc<dyn INotebookFoldingController>;
}

/// `FoldedCellHint`.
pub struct FoldedCellHint {
    state: CellPartState,
    me: Weak<FoldedCellHint>,
    ui: Ui,
    notebook_editor: Rc<dyn IFoldedCellHintEditor>,
    container: Rc<StickyTopNode>,
    label_styles: HighlightedLabelStyles,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    run_button_listener: Rc<MutableDisposable<dyn IDisposable>>,
    cell_execution_listener: Rc<MutableDisposable<dyn IDisposable>>,
}

impl FoldedCellHint {
    /// `label_styles` é o estilo do texto (cor, fonte) que o CSS do contêiner daria; chega de quem tem o tema.
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn IFoldedCellHintEditor>,
        container: NodeId,
        label_styles: HighlightedLabelStyles,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let state = CellPartState::new();
        let run_button_listener = MutableDisposable::<dyn IDisposable>::new();
        let cell_execution_listener = MutableDisposable::<dyn IDisposable>::new();
        state.register(run_button_listener.clone());
        state.register(cell_execution_listener.clone());
        Rc::new_cyclic(|me: &Weak<Self>| Self {
            state,
            me: me.clone(),
            ui: ui.clone(),
            notebook_editor,
            container: StickyTopNode::new(ui, container),
            label_styles,
            notebook_execution_state_service,
            run_button_listener,
            cell_execution_listener,
        })
    }

    fn clear_listeners(&self) {
        self.cell_execution_listener.set(None);
        self.run_button_listener.set(None);
    }

    fn update(&self, element: &CellViewModel) {
        let Some(markup) = element.as_markup() else { return };
        let Some(view_model) = self.notebook_editor.active_view_model() else {
            self.clear_listeners();
            return;
        };

        let container = self.container.node();
        if element.base().is_input_collapsed() || element.base().get_edit_state() == CellEditState::Editing {
            self.clear_listeners();
            hide(&mut self.ui.tree_mut(), container);
        } else if markup.folding_state(element) == CellFoldingState::Collapsed {
            let idx = view_model.get_cell_index(element);
            let length = view_model.get_folded_length(idx);

            clear_node(&mut self.ui.tree_mut(), container);
            let run_section_button = self.add_run_folded_section_button(ICellRange { start: idx, end: idx + length + 1 });
            let label = self.add_hidden_cells_label(length);
            let expand_button = self.add_hidden_cell_hint_button(element);

            show(&mut self.ui.tree_mut(), container);
            self.layout_children(run_section_button, label, expand_button, markup.layout_info().fold_hint_height);

            let fold_hint_top = markup.layout_info().preview_height;
            self.container.set_top(Some(fold_hint_top));
        } else {
            self.clear_listeners();
            hide(&mut self.ui.tree_mut(), container);
        }
    }

    /// O estilo em fluxo do texto e dos ícones da dica.
    fn inline_style(&self) -> InlineStyle {
        InlineStyle {
            size_px: self.label_styles.font_size,
            weight: self.label_styles.font_weight,
            color: self.label_styles.foreground,
            line_height: self.label_styles.line_height.unwrap_or(WORKBENCH_LINE_HEIGHT),
            italic: self.label_styles.italic,
            strikethrough: self.label_styles.strikethrough,
        }
    }

    /// Refaz o ícone do botão: `classList.add(...ThemeIcon.asClassNameArray(icon))` num botão sem classes de ícone.
    fn set_button_icon(&self, button: NodeId, icon: &ThemeIcon) {
        let style = self.inline_style();
        let mut tree = self.ui.tree_mut();
        clear_node(&mut tree, button);
        let rendered = build_parts(&mut tree, button, &[LabelPart::Span(render_icon(icon, false))], &style);
        if let Some(Rendered::Icon { node }) = rendered.first() {
            tree.set_bounds(*node, Rect::new(BUTTON_PADDING, BUTTON_PADDING, BUTTON_PADDING + BUTTON_ICON_SIZE, BUTTON_PADDING + BUTTON_ICON_SIZE));
        }
    }

    /// `getHiddenCellsLabel(num)`.
    fn add_hidden_cells_label(&self, num: i64) -> NodeId {
        let num_text = num.to_string();
        let text = if num == 1 { localize("hiddenCellsLabel", "1 cell hidden", &[]) } else { localize("hiddenCellsLabelPlural", "{0} cells hidden", &[num_text.as_str()]) };

        let style = self.inline_style();
        let mut tree = self.ui.tree_mut();
        let label = create_el(&mut tree, self.container.node(), Role::Generic, "notebook-folded-hint-label");
        let rendered = build_parts(&mut tree, label, &[LabelPart::Text(text)], &style);
        let width = layout_parts(&mut tree, &rendered, style.line_height, None);
        tree.set_bounds(label, Rect::new(0.0, 0.0, width, style.line_height));
        tree.style_mut(label).opacity = 0.7;
        label
    }

    /// `getHiddenCellHintButton(element)`.
    fn add_hidden_cell_hint_button(&self, element: &CellViewModel) -> NodeId {
        let expand_icon = {
            let mut tree = self.ui.tree_mut();
            create_el(&mut tree, self.container.node(), Role::Button, "cell-expand-part-button")
        };
        self.set_button_icon(expand_icon, &ThemeIcon::from_id("more"));

        let (editor, element) = (self.notebook_editor.clone(), element.clone());
        self.state.register(self.ui.listen(expand_icon, EventKind::Click, false, move |_| {
            let controller = editor.folding_controller();
            let idx = editor.get_cell_index(&element);
            if let Some(idx) = idx {
                controller.set_folding_state_down(idx, CellFoldingState::Expanded, 1);
            }
        }));

        expand_icon
    }

    /// Algum dos `cells` está executando.
    fn is_running(service: &dyn INotebookExecutionStateService, cells: &[CellViewModel]) -> bool {
        cells.iter().any(|cell| service.get_cell_execution(&cell.uri()).is_some_and(|cell_execution| cell_execution.state() == NotebookCellExecutionState::Executing))
    }

    /// O ícone do botão de executar a seção: o de execução girando enquanto roda, ou o de reproduzir.
    fn run_section_icon(is_running: bool) -> ThemeIcon {
        if is_running { spinning_executing_state_icon() } else { ThemeIcon::from_id("play") }
    }

    /// `getRunFoldedSectionButton(range)`.
    fn add_run_folded_section_button(&self, range: ICellRange) -> Option<NodeId> {
        let cells = self.notebook_editor.get_cells_in_range(range);

        // Check if any cells are code cells, if not, we won't show the run button
        let has_code_cells = cells.iter().any(|cell| cell.cell_kind() == CellKind::Code);
        if !has_code_cells {
            return None;
        }

        let run_all_container = {
            let mut tree = self.ui.tree_mut();
            create_el(&mut tree, self.container.node(), Role::Button, "folded-cell-run-section-button")
        };
        let is_running = Self::is_running(&*self.notebook_execution_state_service, &cells);
        self.set_button_icon(run_all_container, &Self::run_section_icon(is_running));

        {
            let (editor, cells) = (self.notebook_editor.clone(), cells.clone());
            self.run_button_listener.set(Some(self.ui.listen(run_all_container, EventKind::Click, false, move |_| {
                editor.execute_notebook_cells(cells.clone());
            })));
        }

        let me = self.me.clone();
        self.cell_execution_listener.set(Some(self.notebook_execution_state_service.on_did_change_execution().listen(move |_| {
            let Some(part) = me.upgrade() else { return };
            let is_running = Self::is_running(&*part.notebook_execution_state_service, &cells);
            part.set_button_icon(run_all_container, &Self::run_section_icon(is_running));
        })));

        Some(run_all_container)
    }

    /// O `display: flex; align-items: center` do `.notebook-folded-hint`: botão de executar, rótulo e botão de expandir, da esquerda
    /// para a direita, centrados na altura `height` da dica.
    fn layout_children(&self, run_section_button: Option<NodeId>, label: NodeId, expand_button: NodeId, height: f64) {
        let mut tree = self.ui.tree_mut();
        let mut x = 0.0;
        if let Some(button) = run_section_button {
            let y = (height - BUTTON_SIZE) / 2.0;
            tree.set_bounds(button, Rect::new(x, y, x + BUTTON_SIZE, y + BUTTON_SIZE));
            x += BUTTON_SIZE + BUTTON_MARGIN;
        }
        let label_bounds = tree.bounds(label);
        let y = (height - label_bounds.height()) / 2.0;
        tree.set_bounds(label, Rect::new(x, y, x + label_bounds.width(), y + label_bounds.height()));
        x += label_bounds.width() + BUTTON_MARGIN;
        let y = (height - BUTTON_SIZE) / 2.0;
        tree.set_bounds(expand_button, Rect::new(x, y, x + BUTTON_SIZE, y + BUTTON_SIZE));
        x += BUTTON_SIZE;

        let container = tree.bounds(self.container.node());
        tree.set_bounds(self.container.node(), Rect::new(container.x0, container.y0, container.x0 + x, container.y0 + height));
    }
}

impl CellPart for FoldedCellHint {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        self.update(element);
    }

    fn update_internal_layout_now(&self, element: &CellViewModel) {
        self.update(element);
    }
}

impl_cell_content_part!(FoldedCellHint, "FoldedCellHint");
