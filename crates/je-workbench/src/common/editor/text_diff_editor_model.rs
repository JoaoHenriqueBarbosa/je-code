//! Porte de `src/vs/workbench/common/editor/textDiffEditorModel.ts`: o modelo base de texto do diff editor, feito de dois
//! modelos de editor de texto, a versão original e a modificada.
//!
//! Divergências:
//! - **Herança.** `TextDiffEditorModel extends DiffEditorModel` guarda o [`DiffEditorModel`] (campo `base`) e repassa a ele o
//!   que a classe não sobrescreve (`onWillDispose`, `isDisposed` e a parte do `resolve` que resolve os dois modelos).
//! - **`BaseTextEditorModel`.** Os dois modelos são o contrato [`ITextEditorModel`] (que cobre `isResolved`, `textEditorModel` e
//!   `isReadonly` da `BaseTextEditorModel`); o `instanceof BaseTextEditorModel` do `DiffEditorInput.createModel` é
//!   [`IEditorModel::into_text_editor_model`].
//! - **`IDiffEditorModel`.** O objeto `{ original, modified }` de `editorCommon.ts` é o [`IDiffEditorModel`], com os dois
//!   modelos de texto atualizáveis no lugar (como o `this._textDiffEditorModel.original = ...` do original), e a identidade do
//!   objeto é a do `Rc`.
//! - **`isReadonly()`.** `boolean | IMarkdownString` é o [`Readonly`].

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::text_model::TextModel;

use crate::common::editor::diff_editor_model::DiffEditorModel;
use crate::common::editor::editor_model::IEditorModel;
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::textmodel_resolver::resolver_service::ITextEditorModel;

/// `IDiffEditorModel`: the original and the modified text model of a diff editor.
pub struct IDiffEditorModel {
    original: RefCell<Rc<TextModel>>,
    modified: RefCell<Rc<TextModel>>,
}

impl IDiffEditorModel {
    pub fn new(original: Rc<TextModel>, modified: Rc<TextModel>) -> Rc<Self> {
        Rc::new(Self { original: RefCell::new(original), modified: RefCell::new(modified) })
    }

    /// `original`.
    pub fn original(&self) -> Rc<TextModel> {
        self.original.borrow().clone()
    }

    /// `modified`.
    pub fn modified(&self) -> Rc<TextModel> {
        self.modified.borrow().clone()
    }

    fn update(&self, original: Rc<TextModel>, modified: Rc<TextModel>) {
        *self.original.borrow_mut() = original;
        *self.modified.borrow_mut() = modified;
    }
}

/// `TextDiffEditorModel`: the base text editor model for the diff editor. It is made up of two text editor models, the
/// original version and the modified version.
pub struct TextDiffEditorModel {
    this: Weak<TextDiffEditorModel>,
    base: Rc<DiffEditorModel>,
    original_model: Rc<dyn ITextEditorModel>,
    modified_model: Rc<dyn ITextEditorModel>,
    text_diff_editor_model: RefCell<Option<Rc<IDiffEditorModel>>>,
}

impl TextDiffEditorModel {
    pub fn new(original_model: Rc<dyn ITextEditorModel>, modified_model: Rc<dyn ITextEditorModel>) -> Rc<Self> {
        let base = DiffEditorModel::new(
            Some(original_model.clone() as Rc<dyn IEditorModel>),
            Some(modified_model.clone() as Rc<dyn IEditorModel>),
        );

        let this = Rc::new_cyclic(|this| Self { this: this.clone(), base, original_model, modified_model, text_diff_editor_model: RefCell::new(None) });

        this.update_text_diff_editor_model();

        this
    }

    /// `originalModel`.
    pub fn original_model(&self) -> Rc<dyn ITextEditorModel> {
        self.original_model.clone()
    }

    /// `modifiedModel`.
    pub fn modified_model(&self) -> Rc<dyn ITextEditorModel> {
        self.modified_model.clone()
    }

    /// `textDiffEditorModel`.
    pub fn text_diff_editor_model(&self) -> Option<Rc<IDiffEditorModel>> {
        self.text_diff_editor_model.borrow().clone()
    }

    fn update_text_diff_editor_model(&self) {
        if self.original_model.is_resolved() && self.modified_model.is_resolved() {
            let (Some(original), Some(modified)) = (self.original_model.text_editor_model(), self.modified_model.text_editor_model()) else {
                return;
            };

            let existing = self.text_diff_editor_model.borrow().clone();
            match existing {
                // Create new
                None => *self.text_diff_editor_model.borrow_mut() = Some(IDiffEditorModel::new(original, modified)),

                // Update existing
                Some(existing) => existing.update(original, modified),
            }
        }
    }

    /// `isReadonly()`.
    pub fn is_readonly(&self) -> Readonly {
        self.modified_model.is_readonly()
    }
}

impl IDisposable for TextDiffEditorModel {
    fn dispose(&self) {
        // Free the diff editor model but do not propagate the dispose() call to the two models
        // inside. We never created the two models (original and modified) so we can not dispose
        // them without sideeffects. Rather rely on the models getting disposed when their related
        // inputs get disposed from the diffEditorInput.
        *self.text_diff_editor_model.borrow_mut() = None;

        self.base.dispose();
    }
}

impl IEditorModel for TextDiffEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }

    fn resolve(&self) -> Promise<()> {
        let this = self.this.clone();

        self.base.resolve().then(move |_| {
            if let Some(this) = this.upgrade() {
                this.update_text_diff_editor_model();
            }
        })
    }

    fn is_resolved(&self) -> bool {
        self.text_diff_editor_model.borrow().is_some()
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }
}
