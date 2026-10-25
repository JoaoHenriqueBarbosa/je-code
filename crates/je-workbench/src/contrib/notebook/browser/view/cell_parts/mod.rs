//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/view/cellParts`, sobre o `CellContentPart` e o `CellOverlayPart` de
//! `view/cell_part.rs`.
//!
//! Portados até aqui (um arquivo por fonte): `cellDnd.ts` (só a lógica de reordenação), `cellOutput.ts`, `codeCell.ts` (só o
//! `CodeCellLayout`), `cellEditorOptions.ts`, `cellContextKeys.ts`, `cellFocusIndicator.ts`, `cellDecorations.ts`, `cellFocus.ts`,
//! `cellWidgets.ts`, `cellEditorOverflowWidgets.ts`, `collapsedCellInput.ts`, `collapsedCellOutput.ts` e `cellProgressBar.ts`.
//!
//! Fatias seguintes, nesta ordem: `cellToolbars`, `cellStatusPart`, `cellExecution`, `codeCellRunToolbar`, `foldedCellHint`,
//! `cellComments`, `cellDragRenderer`, `cellActionView`, `codeCellExecutionIcon`; depois `codeCell.ts` e `markupCell.ts`; depois o
//! `cellRenderer.ts` (que monta os templates e fornece as implementações dos contratos estreitos abaixo) e os 5 casos de
//! `cellEditorOverflowWidgets.test.ts`.
//!
//! Divergências comuns a todas as partes desta pasta:
//! - **DOM é a árvore do `je-ui`.** O `HTMLElement` que o construtor original recebe é um [`je_ui::NodeId`] da [`je_ui::Ui`] (o
//!   `DOM.append(parent, DOM.$('.classe'))` é `create_el`, o `classList` é o da árvore, o `addDisposableListener` é `Ui::listen`). As
//!   propriedades de estilo que a árvore não modela (`transform: translateY`, `innerText`, `title`) viram o equivalente nativo
//!   descrito em cada arquivo.
//! - **Serviços injetados chegam como argumentos.** O `@IContextKeyService`, `@IKeybindingService` e
//!   `@INotebookExecutionStateService` que o `createInstance` injeta são parâmetros do construtor (`Rc<dyn ...>`).
//! - **`INotebookEditor` e `INotebookEditorDelegate` são contratos estreitos por parte** (`ICellFocusEditor`,
//!   `ICellDecorationsEditor`...), só com o que cada parte chama. O widget implementa todos eles; é o `cellRenderer.ts` que os liga.
//! - **Partes são `Rc`.** Os ouvintes de evento guardam um `Weak` da parte (o `this` do original), então os construtores devolvem
//!   `Rc<Self>` (via `Rc::new_cyclic`). `_register(x)` é [`CellPartState::register`].
//! - **`ICellViewModel` é o [`CellViewModel`](crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel).**
//!   O `instanceof CodeCellViewModel` e o `instanceof MarkupCellViewModel` são `match` e `as_code`.

/// Implementa o que toda parte de conteúdo repete: o `IDisposable` (que descarta o [`CellPartState`] da parte, campo `state`) e o
/// `CellContentPart` com os métodos padrão. O `CellPart` fica com quem usa a macro, porque é nele que a parte sobrescreve
/// `did_render_cell`, `update_state` e os outros ganchos.
macro_rules! impl_cell_content_part {
    ($part:ty, $name:literal) => {
        impl je_base::lifecycle::IDisposable for $part {
            fn dispose(&self) {
                je_base::lifecycle::IDisposable::dispose(&self.state);
            }

            fn constructor_name(&self) -> &'static str {
                $name
            }
        }

        impl $crate::contrib::notebook::browser::view::cell_part::CellContentPart for $part {}
    };
}

pub mod cell_decorations;
pub mod cell_dnd;
pub mod cell_editor_options;
pub mod cell_editor_overflow_widgets;
pub mod cell_focus;
pub mod cell_focus_indicator;
pub mod cell_output;
pub mod cell_progress_bar;
pub mod cell_widgets;
pub mod code_cell_layout;
pub mod collapsed_cell_input;
pub mod collapsed_cell_output;
pub mod cell_context_keys;
