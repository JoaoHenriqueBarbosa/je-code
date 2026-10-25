//! A superfície do editor de diff embutido que o `DiffEditorItemTemplate` usa. O `DiffEditorWidget`
//! (`widget/diffEditor/diffEditorWidget.ts`) e o `ICodeEditor` de cada lado dele ainda não existem no porte inteiros, e o
//! template só consome o recorte abaixo; ele é o contrato que o widget de diff implementa quando for portado.
//!
//! Divergências:
//! - O original cria o `DiffEditorWidget` com `instantiationService.createInstance(DiffEditorWidget, domNode, options,
//!   widgetOptions)`. O nó do DOM não existe (a UI é nativa) e os serviços injetados são de quem monta o widget, então a
//!   criação é uma closure ([`CreateDiffEditor`]) que recebe as opções ([`DiffEditorItemWidgetOptions`]) que o template
//!   passa: `fixedOverflowWidgets: true` e o `overflowWidgetsDomNode` (um nó do DOM, que some, o contêiner dos widgets de
//!   overflow é de quem cria o widget) entre as opções do editor, e `variant`, `runWithOriginalEditorScrollAnchor` e
//!   `runWithModifiedEditorScrollAnchor` entre as do widget.
//! - `observableCodeEditor(editor).isFocused` é [`IDiffSideEditor::is_focused`], e `getModel()!.getLineCount()` é
//!   [`IDiffSideEditor::get_line_count`].

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, RefCounted};
use je_base::observable::{IObservable, ITransaction};

use crate::config::editor_options::layout_info::EditorLayoutInfo;
use crate::diff_editor_view_model::DiffEditorViewModel;
use crate::multi_diff_editor_options::DiffEditorVariant;
use crate::view_model_event_dispatcher::ContentSizeChangedEvent;

/// O recorte de `ICodeEditor` que o template lê de cada lado (original e modificado) do editor de diff.
pub trait IDiffSideEditor {
    /// `onDidLayoutChange`.
    fn on_did_layout_change(&self) -> Event<EditorLayoutInfo>;

    /// `getLayoutInfo()`.
    fn get_layout_info(&self) -> EditorLayoutInfo;

    /// `getContentWidth()`.
    fn get_content_width(&self) -> f64;

    /// `getContentHeight()`.
    fn get_content_height(&self) -> f64;

    /// `getTopForLineNumber(lineNumber)`.
    fn get_top_for_line_number(&self, line_number: i32) -> f64;

    /// `getModel()!.getLineCount()`.
    fn get_line_count(&self) -> i32;

    /// `setScrollLeft(left)`.
    fn set_scroll_left(&self, left: f64);

    /// `setScrollTop(top)`.
    fn set_scroll_top(&self, top: f64);

    /// `observableCodeEditor(editor).isFocused`.
    fn is_focused(&self) -> IObservable<bool>;
}

/// O recorte de `DiffEditorWidget` que o template usa.
pub trait IDiffEditorWidget: IDisposable {
    /// `getOriginalEditor()`.
    fn get_original_editor(&self) -> Rc<dyn IDiffSideEditor>;

    /// `getModifiedEditor()`.
    fn get_modified_editor(&self) -> Rc<dyn IDiffSideEditor>;

    /// `onDidContentSizeChange`.
    fn on_did_content_size_change(&self) -> Event<ContentSizeChangedEvent>;

    /// `setDiffModel(viewModel, tx)`.
    fn set_diff_model(&self, view_model: Option<Rc<RefCounted<Rc<DiffEditorViewModel>>>>, tx: Option<&dyn ITransaction>);

    /// `updateOptions(newOptions)`.
    fn update_options(&self, new_options: &Dynamic);

    /// `layout({ width, height })`.
    fn layout(&self, width: f64, height: f64);

    /// `getContentHeight()`.
    fn get_content_height(&self) -> f64;

    /// `allUnchangedRegionsShown`.
    fn all_unchanged_regions_shown(&self) -> IObservable<bool>;

    /// `focus()`.
    fn focus(&self);
}

/// `(anchorLineNumber, update) => void` de `runWith{Original,Modified}EditorScrollAnchor`.
pub type ScrollAnchorRunner = Rc<dyn Fn(i32, &dyn Fn())>;

/// As opções com que o template cria o `DiffEditorWidget` (ver a nota do módulo).
#[derive(Clone)]
pub struct DiffEditorItemWidgetOptions {
    /// `variant`.
    pub variant: Option<DiffEditorVariant>,
    /// `fixedOverflowWidgets: true`.
    pub fixed_overflow_widgets: bool,
    /// `runWithOriginalEditorScrollAnchor`.
    pub run_with_original_editor_scroll_anchor: ScrollAnchorRunner,
    /// `runWithModifiedEditorScrollAnchor`.
    pub run_with_modified_editor_scroll_anchor: ScrollAnchorRunner,
}

/// Cria o `DiffEditorWidget` embutido de um template.
pub type CreateDiffEditor = Rc<dyn Fn(DiffEditorItemWidgetOptions) -> Rc<dyn IDiffEditorWidget>>;
