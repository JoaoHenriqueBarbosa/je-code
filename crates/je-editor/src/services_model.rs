//! Porte de `src/vs/editor/common/services/model.ts`: o `IModelService`.
//!
//! Divergências:
//! - Só `getModel(resource)` existe no trait. É o único membro que o `TextResourceConfigurationService` usa. O resto
//!   do contrato (`createModel`, `updateModel`, `destroyModel`, `getModels`, `getCreationOptions` e os eventos
//!   `onModelAdded`, `onModelRemoved` e `onModelLanguageChanged`) é do `ModelService` (`model_service.rs`), que
//!   implementa este trait e traz esses membros como métodos próprios, para o trait não mudar para quem só lê
//!   `getModel`. O tipo `DocumentTokensProvider` fica para o consumidor dele.
//! - `ITextModel` é o [`TextModel`] concreto (o porte não tem a interface), e `ITextModel | null` é `Option`.

use std::rc::Rc;

use je_base::uri::URI;

use crate::text_model::TextModel;

/// `IModelService` (recorte mínimo, ver as divergências do módulo).
pub trait IModelService {
    /// `getModel(resource)`: o modelo aberto para o recurso, se houver.
    fn get_model(&self, resource: &URI) -> Option<Rc<TextModel>>;
}

je_instantiation::service_id!(pub I_MODEL_SERVICE: dyn IModelService = "modelService");
