//! Porte de `src/vs/workbench/common/editor/textResourceEditorModel.ts`: o `TextResourceEditorModel`, um modelo de editor
//! para conteúdo de texto em memória e somente leitura, apoiado num modelo de editor que já existe.
//!
//! Divergências:
//! - **Herança.** `TextResourceEditorModel extends BaseTextEditorModel` guarda a [`BaseTextEditorModel`] (campo `base`) e
//!   herda os contratos dela pela macro [`impl_base_text_editor_model`](crate::impl_base_text_editor_model). O `dispose`
//!   que a classe sobrescreve é o do [`IDisposable`] desta struct.
//! - **Injeção.** O recurso (`resource`) é o argumento estático ([`Injectable::Args`]) e os parâmetros decorados do
//!   construtor são a tupla `Deps`, na ordem do original.

use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_editor::services_model::IModelService;
use je_instantiation::Injectable;
use je_platform::accessibility::IAccessibilityService;

use crate::common::editor::text_editor_model::BaseTextEditorModel;
use crate::services::language_detection::language_detection_worker_service::ILanguageDetectionService;

/// `TextResourceEditorModel`: an editor model for in-memory, readonly text content that is backed by an existing editor
/// model.
pub struct TextResourceEditorModel {
    base: Rc<BaseTextEditorModel>,
}

impl Injectable for TextResourceEditorModel {
    type Args = (URI,);
    type Deps = (Rc<dyn ILanguageService>, Rc<dyn IModelService>, Rc<dyn ILanguageDetectionService>, Rc<dyn IAccessibilityService>);

    fn create(
        (resource,): Self::Args,
        (language_service, model_service, language_detection_service, accessibility_service): Self::Deps,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| {
            let outer: Weak<dyn IDisposable> = this.clone();
            Self { base: BaseTextEditorModel::new(outer, model_service, language_service, language_detection_service, accessibility_service, Some(resource)) }
        })
    }
}

impl IDisposable for TextResourceEditorModel {
    fn dispose(&self) {
        // force this class to dispose the underlying model
        if let Some(text_editor_model_handle) = self.base.text_editor_model_handle() {
            self.base.model_service.destroy_model(&text_editor_model_handle);
        }

        self.base.dispose();
    }
}

crate::impl_base_text_editor_model!(TextResourceEditorModel, base);

/// A herança de métodos: o que o `BaseTextEditorModel` traz (`updateTextEditorModel`, `getLanguageId`,
/// `hasLanguageSetExplicitly`...) é lido pela subclasse.
impl Deref for TextResourceEditorModel {
    type Target = BaseTextEditorModel;

    fn deref(&self) -> &BaseTextEditorModel {
        &self.base
    }
}
