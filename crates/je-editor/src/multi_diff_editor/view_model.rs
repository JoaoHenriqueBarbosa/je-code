//! O recorte de `DocumentDiffItemViewModel` (`multiDiffEditorViewModel.ts`) que o editor de diff de vários arquivos
//! consome na camada do editor.
//!
//! O view model do original mora em `editor/browser`, mas o porte o guarda no `je-workbench`
//! (`je_workbench::contrib::multi_diff_editor::browser::multi_diff_editor_view_model`), porque ele depende do
//! `ITextModelService`, e o `je-editor` não enxerga o workbench. O template e o widget do `je-editor` leem o item pelo
//! trait [`IDocumentDiffItemViewModel`], que a struct do workbench implementa, e guardam o item como
//! [`DocumentDiffItem`].
//!
//! Divergência: `lastTemplateData.get().expandedContentHeight` (o único campo de `lastTemplateData` que o template lê) é
//! [`IDocumentDiffItemViewModel::expanded_content_height`].

use std::rc::Rc;

use je_base::observable::{IObservable, ISettableObservable, ITransaction};
use je_base::lifecycle::RefCounted;
use je_base::uri::URI;

use crate::diff_editor_view_model::DiffEditorViewModel;
use crate::multi_diff_editor::model::IDocumentDiffItem;

/// `DocumentDiffItemViewModel`, como o editor de diff de vários arquivos o enxerga.
pub trait IDocumentDiffItemViewModel {
    /// `documentDiffItem`.
    fn document_diff_item(&self) -> &Rc<dyn IDocumentDiffItem>;

    /// `diffEditorViewModel`.
    fn diff_editor_view_model(&self) -> &Rc<DiffEditorViewModel>;

    /// `diffEditorViewModelRef`.
    fn diff_editor_view_model_ref(&self) -> &Rc<RefCounted<Rc<DiffEditorViewModel>>>;

    /// `originalUri`.
    fn original_uri(&self) -> Option<URI>;

    /// `modifiedUri`.
    fn modified_uri(&self) -> Option<URI>;

    /// `isBinary`.
    fn is_binary(&self) -> bool;

    /// `collapsed`.
    fn collapsed(&self) -> ISettableObservable<bool>;

    /// `isActive`.
    fn is_active(&self) -> IObservable<bool>;

    /// `isFirst`.
    fn is_first(&self) -> IObservable<bool>;

    /// `lastTemplateData.get().expandedContentHeight`.
    fn expanded_content_height(&self) -> f64;

    /// `setActive(tx)`.
    fn set_active(&self, tx: Option<&dyn ITransaction>);
}

/// O item do editor de diff de vários arquivos (`DocumentDiffItemViewModel`), comparado por identidade.
pub type DocumentDiffItem = Rc<dyn IDocumentDiffItemViewModel>;
