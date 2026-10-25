//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/view/cellParts`, sobre o `CellContentPart` e o `CellOverlayPart` de
//! `view/cell_part.rs`.
//!
//! Portados até aqui (um arquivo por fonte): `cellDnd.ts` (só a lógica de reordenação), `cellOutput.ts`, `codeCell.ts` (só o
//! `CodeCellLayout`), `cellEditorOptions.ts`, `cellContextKeys.ts`, `cellFocusIndicator.ts`, `cellDecorations.ts`, `cellFocus.ts`,
//! `cellWidgets.ts`, `cellEditorOverflowWidgets.ts`, `collapsedCellInput.ts`, `collapsedCellOutput.ts`, `cellProgressBar.ts`,
//! `cellToolbars.ts` (`BetweenCellToolbar` e `CellTitleToolbarPart`), `cellStatusPart.ts` (`CellEditorStatusBar`), `cellExecution.ts`
//! (`CellExecutionPart`), `cellToolbarStickyScroll.ts` (que `cellToolbars.ts` importa), `cellActionView.ts` (`CodiconActionViewItem`,
//! `ActionViewWithLabel` e `UnifiedSubmenuActionView`, sobre as classes-base de `platform/actions/menu_entry_action_view_item.rs`),
//! `codeCellRunToolbar.ts` (`RunToolbar`, sobre `platform/actions/dropdown_with_primary_action_view_item.rs`), `foldedCellHint.ts`,
//! `cellComments.ts`, `cellDragRenderer.ts`, `codeCellExecutionIcon.ts` e `markupCell.ts` (`MarkupCell`, com os contratos estreitos do
//! editor da célula, da fábrica do editor e do `IActiveNotebookEditorDelegate`, e o `MarkdownCellRenderTemplate`).
//!
//! Faltam, nesta ordem: `codeCell.ts` (a classe `CodeCell`, além do `CodeCellLayout`; fatia 4b, que reaproveita
//! `in_composite_editor` e os contratos do editor de `markup_cell.rs`); depois o `cellRenderer.ts` (que monta os templates e fornece as
//! implementações dos contratos estreitos abaixo, inclusive o `IMarkupCellEditorFactory`, o `IMarkupCellEditorHost` e o
//! `IRenderedEditors`) e os 5 casos de `cellEditorOverflowWidgets.test.ts` (fatia 5).
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
//!   O `instanceof CodeCellViewModel` e o `instanceof MarkupCellViewModel` são `match` e `as_code`; os campos de `layoutInfo` que as duas
//!   formas compartilham são lidos por `common_layout_info` (`view/cell_part.rs`).
//! - **Partes de sobreposição** (`BetweenCellToolbar`, `CellTitleToolbarPart`) usam `impl_cell_overlay_part!`, o par de
//!   `impl_cell_content_part!` para o `CellOverlayPart`.
//! - **Contexto de ação das barras** (`{ ui, cell, notebookEditor, source, $mid }`) é `INotebookActionContext::to_dynamic`.

/// Implementa o `IDisposable` que toda parte repete: descarta o [`CellPartState`] da parte (campo `state`).
macro_rules! impl_cell_part_disposable {
    ($part:ty, $name:literal) => {
        impl je_base::lifecycle::IDisposable for $part {
            fn dispose(&self) {
                je_base::lifecycle::IDisposable::dispose(&self.state);
            }

            fn constructor_name(&self) -> &'static str {
                $name
            }
        }
    };
}

/// Implementa o que toda parte de conteúdo repete: o `IDisposable` (que descarta o [`CellPartState`] da parte, campo `state`) e o
/// `CellContentPart` com os métodos padrão. O `CellPart` fica com quem usa a macro, porque é nele que a parte sobrescreve
/// `did_render_cell`, `update_state` e os outros ganchos.
macro_rules! impl_cell_content_part {
    ($part:ty, $name:literal) => {
        impl_cell_part_disposable!($part, $name);

        impl $crate::contrib::notebook::browser::view::cell_part::CellContentPart for $part {}
    };
}

/// O mesmo de `impl_cell_content_part`, para as partes de sobreposição (`CellOverlayPart`).
macro_rules! impl_cell_overlay_part {
    ($part:ty, $name:literal) => {
        impl_cell_part_disposable!($part, $name);

        impl $crate::contrib::notebook::browser::view::cell_part::CellOverlayPart for $part {}
    };
}

pub mod cell_action_view;
pub mod cell_comments;
pub mod cell_decorations;
pub mod cell_dnd;
pub mod cell_drag_renderer;
pub mod cell_editor_options;
pub mod cell_editor_overflow_widgets;
pub mod cell_execution;
pub mod cell_focus;
pub mod cell_focus_indicator;
pub mod cell_output;
pub mod cell_progress_bar;
pub mod cell_status_part;
pub mod cell_toolbar_sticky_scroll;
pub mod cell_toolbars;
pub mod cell_widgets;
pub mod code_cell_execution_icon;
pub mod code_cell_layout;
pub mod code_cell_run_toolbar;
pub mod collapsed_cell_input;
pub mod collapsed_cell_output;
pub mod cell_context_keys;
pub mod folded_cell_hint;
pub mod markup_cell;
