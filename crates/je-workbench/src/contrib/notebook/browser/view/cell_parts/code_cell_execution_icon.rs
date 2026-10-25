//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/codeCellExecutionIcon.ts`: `CollapsedCodeCellExecutionIcon`, o
//! ícone de estado de execução (sucesso, falha, pendente, executando) que a célula de código mostra ao lado da entrada recolhida.
//!
//! Divergências:
//! - O `HTMLElement` é um nó da árvore do `je-ui`. `style.display = ''` e `'none'` são `show` e `hide`; `DOM.reset(element,
//!   ...renderLabelWithIcons(text))` é o [`SimpleIconLabel`] sobre o nó (ele limpa os filhos e monta o ícone), e `DOM.reset(element)`
//!   sem partes é `clear_node`; `element.title` é o `set_title` do rótulo. O estilo do texto (cor, fonte) chega como o argumento
//!   `label_styles`, montado por quem tem o tema.
//! - **Ícones de estado** (`notebookIcons.ts`, sem porte): `registerIcon('notebook-state-success', Codicon.check)` e os outros três
//!   resolvem para o glifo padrão, que é o id usado em [`success_state_icon`], [`error_state_icon`], [`pending_state_icon`] e
//!   [`executing_state_icon`] (uma troca do ícone pelo tema não é honrada). `cellExecution.ts` e `foldedCellHint.ts` os importam daqui.
//! - `INotebookEditorDelegate` (o primeiro argumento do construtor, que o original não usa) não é parâmetro.
//! - `_cell: ICellViewModel` é o [`CellViewModel`]; `@INotebookExecutionStateService` é argumento do construtor. A classe estende
//!   `Disposable`, então o construtor devolve `Rc<Self>` e os ouvintes guardam um `Weak` (o `this` do original).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::themables::{IconModifier, ThemeIcon};
use je_ui::{NodeId, Ui};
use je_widgets::dom::{clear_node, hide, show};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::simple_icon_label::SimpleIconLabel;

use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::{ICell, NotebookCellExecutionState, NotebookCellInternalMetadata};
use crate::contrib::notebook::common::notebook_execution_state_service::{ExecutionStateChange, INotebookCellExecution, INotebookExecutionStateService};

/// `successStateIcon` (ver as divergências do módulo): `Codicon.check`.
pub fn success_state_icon() -> ThemeIcon {
    ThemeIcon::from_id("check")
}

/// `errorStateIcon`: `Codicon.error`.
pub fn error_state_icon() -> ThemeIcon {
    ThemeIcon::from_id("error")
}

/// `pendingStateIcon`: `Codicon.clock`.
pub fn pending_state_icon() -> ThemeIcon {
    ThemeIcon::from_id("clock")
}

/// `executingStateIcon`: `Codicon.sync`.
pub fn executing_state_icon() -> ThemeIcon {
    ThemeIcon::from_id("sync")
}

/// `executingStateIcon` girando: `ThemeIcon.modify(executingStateIcon, 'spin')`.
pub fn spinning_executing_state_icon() -> ThemeIcon {
    ThemeIcon::modify(&executing_state_icon(), Some(IconModifier::Spin))
}

/// `IExecutionItem`.
struct IExecutionItem {
    text: String,
    tooltip: Option<String>,
}

/// `CollapsedCodeCellExecutionIcon`.
pub struct CollapsedCodeCellExecutionIcon {
    base: Rc<Disposable>,
    ui: Ui,
    cell: CellViewModel,
    element: NodeId,
    icon_label: SimpleIconLabel,
    execution_state_service: Rc<dyn INotebookExecutionStateService>,
    visible: Cell<bool>,
}

impl CollapsedCodeCellExecutionIcon {
    pub fn new(
        ui: &Ui,
        cell: CellViewModel,
        element: NodeId,
        label_styles: HighlightedLabelStyles,
        execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            base: Disposable::new(),
            ui: ui.clone(),
            cell,
            element,
            icon_label: SimpleIconLabel::new(ui, element, label_styles),
            execution_state_service,
            visible: Cell::new(false),
        });

        this.update();
        {
            let weak: Weak<Self> = Rc::downgrade(&this);
            this.base.register(this.execution_state_service.on_did_change_execution().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                if let ExecutionStateChange::Cell(e) = e
                    && e.affects_cell(&this.cell.uri())
                {
                    this.update();
                }
            }));
        }
        {
            let weak: Weak<Self> = Rc::downgrade(&this);
            this.base.register(this.cell.model().on_did_change_internal_metadata().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update();
                }
            }));
        }

        this
    }

    pub fn set_visibility(&self, visible: bool) {
        self.visible.set(visible);
        self.update();
    }

    fn update(&self) {
        if !self.visible.get() {
            return;
        }

        let run_state = self.execution_state_service.get_cell_execution(&self.cell.uri());
        let item = Self::get_item_for_state(run_state.as_deref(), &self.cell.base().internal_metadata());
        if let Some(item) = item {
            show(&mut self.ui.tree_mut(), self.element);
            self.icon_label.set_text(&item.text);
            self.icon_label.set_title(item.tooltip.as_deref().unwrap_or(""));
        } else {
            hide(&mut self.ui.tree_mut(), self.element);
            clear_node(&mut self.ui.tree_mut(), self.element);
        }
    }

    fn get_item_for_state(run_state: Option<&dyn INotebookCellExecution>, internal_metadata: &NotebookCellInternalMetadata) -> Option<IExecutionItem> {
        let state = run_state.map(|run_state| run_state.state());
        let last_run_success = internal_metadata.last_run_success;
        if state.is_none() && last_run_success == Some(true) {
            Some(IExecutionItem { text: format!("$({})", success_state_icon().id), tooltip: Some(localize("notebook.cell.status.success", "Success", &[])) })
        } else if state.is_none() && last_run_success == Some(false) {
            Some(IExecutionItem { text: format!("$({})", error_state_icon().id), tooltip: Some(localize("notebook.cell.status.failure", "Failure", &[])) })
        } else if matches!(state, Some(NotebookCellExecutionState::Pending | NotebookCellExecutionState::Unconfirmed)) {
            Some(IExecutionItem { text: format!("$({})", pending_state_icon().id), tooltip: Some(localize("notebook.cell.status.pending", "Pending", &[])) })
        } else if state == Some(NotebookCellExecutionState::Executing) {
            Some(IExecutionItem { text: format!("$({})", spinning_executing_state_icon().id), tooltip: Some(localize("notebook.cell.status.executing", "Executing", &[])) })
        } else {
            None
        }
    }
}

impl IDisposable for CollapsedCodeCellExecutionIcon {
    fn dispose(&self) {
        self.icon_label.dispose();
        self.base.dispose();
    }
}
