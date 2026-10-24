//! Porte de `src/vs/editor/common/services/model.ts`: o `IModelService`.
//!
//! Divergências:
//! - O trait é um recorte do contrato: `getModel` (que o `TextResourceConfigurationService` usa), `getCreationOptions`
//!   (as ações de indentação) e `createModel`, `updateModel` e `destroyModel` (o `BaseTextEditorModel`). O resto do
//!   contrato (`getModels`, `onModelAdded`, `onModelRemoved`, `onModelLanguageChanged` e o `DocumentTokensProvider`)
//!   é do `ModelService` (`model_service.rs`), que implementa este trait e traz esses membros como métodos próprios.
//!   `createModel` e `updateModel` do trait recebem o [`TextSource`] (`string | ITextBufferFactory | ITextSnapshot`).
//! - `ITextModel` é o [`TextModel`] concreto (o porte não tem a interface), e `ITextModel | null` é `Option`.
//! - `getCreationOptions(language, resource, isForSimpleWidget)` entrou para as ações de indentação
//!   (`ChangeIndentationSizeAction` e `DetectIndentation`).

use std::rc::Rc;

use je_base::uri::URI;

use crate::language_service::ILanguageSelection;
use crate::model::TextModelCreationOptions;
use crate::text_model::{TextModel, TextSource};
use crate::text_model_edit_source::TextModelEditSource;

/// `IModelService` (recorte mínimo, ver as divergências do módulo).
pub trait IModelService {
    /// `getModel(resource)`: o modelo aberto para o recurso, se houver.
    fn get_model(&self, resource: &URI) -> Option<Rc<TextModel>>;

    /// `createModel(value, languageSelection, resource?, isForSimpleWidget = false)`: o `BaseTextEditorModel` cria os
    /// modelos que ele guarda por aqui.
    fn create_model(&self, value: TextSource, language_selection: Option<&dyn ILanguageSelection>, resource: Option<&URI>, is_for_simple_widget: bool) -> Rc<TextModel>;

    /// `updateModel(model, value, reason?)`: leva o modelo ao texto dado com as edições mínimas.
    fn update_model(&self, model: &TextModel, value: TextSource, reason: Option<TextModelEditSource>);

    /// `destroyModel(resource)`.
    fn destroy_model(&self, resource: &URI);

    /// `getCreationOptions(language, resource, isForSimpleWidget)`: as opções de criação de um modelo
    /// (tamanho de tabulação, espaços, detecção de indentação...) conforme a configuração.
    fn get_creation_options(&self, language: &str, resource: Option<&URI>, is_for_simple_widget: bool) -> TextModelCreationOptions;
}

je_instantiation::service_id!(pub I_MODEL_SERVICE: dyn IModelService = "modelService");
