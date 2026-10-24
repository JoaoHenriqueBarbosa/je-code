//! Porte de `src/vs/workbench/common/editor/diffEditorModel.ts`: o modelo base do diff editor, feito de dois modelos de
//! editor, a versão original e a modificada.
//!
//! Divergências: `IResolvableEditorModel | undefined` é `Option<Rc<dyn IEditorModel>>` (todo modelo de editor resolve e sabe
//! se já resolveu). A herança de `EditorModel` é um [`EditorModel`] guardado em `base`.

use std::any::Any;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;

use crate::common::editor::editor_model::{EditorModel, IEditorModel};

/// `DiffEditorModel`.
pub struct DiffEditorModel {
    base: EditorModel,
    original_model: Option<Rc<dyn IEditorModel>>,
    modified_model: Option<Rc<dyn IEditorModel>>,
}

impl DiffEditorModel {
    pub fn new(original_model: Option<Rc<dyn IEditorModel>>, modified_model: Option<Rc<dyn IEditorModel>>) -> Rc<Self> {
        Rc::new(Self { base: EditorModel::new_inner(), original_model, modified_model })
    }

    /// `originalModel`.
    pub fn original_model(&self) -> Option<Rc<dyn IEditorModel>> {
        self.original_model.clone()
    }

    /// `modifiedModel`.
    pub fn modified_model(&self) -> Option<Rc<dyn IEditorModel>> {
        self.modified_model.clone()
    }
}

impl IDisposable for DiffEditorModel {
    fn dispose(&self) {
        // Do not propagate the dispose() call to the two models inside. We never created the two models (original and
        // modified) so we can not dispose them without sideeffects. Rather rely on the models getting disposed when their
        // related inputs get disposed from the diffEditorInput.

        self.base.dispose();
    }
}

impl IEditorModel for DiffEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }

    fn resolve(&self) -> Promise<()> {
        let resolves: Vec<Promise<()>> = [&self.original_model, &self.modified_model].into_iter().flatten().map(|model| model.resolve()).collect();

        Promise::all(resolves).then(|_| ())
    }

    fn is_resolved(&self) -> bool {
        self.original_model.as_ref().is_some_and(|model| model.is_resolved()) && self.modified_model.as_ref().is_some_and(|model| model.is_resolved())
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }
}
