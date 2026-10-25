//! Porte de `src/vs/editor/common/services/model.ts`: o `IModelService`.
//!
//! Divergências:
//! - O trait é um recorte do contrato: `getModel` (que o `TextResourceConfigurationService` usa), `getModels` e
//!   `onModelAdded` (que o `SearchResultImpl` usa), `onModelLanguageChanged` (que o `ResourceLabels` usa), `getCreationOptions`
//!   (as ações de indentação) e `createModel`,
//!   `updateModel` e `destroyModel` (o `BaseTextEditorModel`). O resto do contrato (`onModelRemoved`
//!   e o `DocumentTokensProvider`) é do `ModelService` (`model_service.rs`), que implementa este
//!   trait e traz esses membros como métodos próprios.
//!   `createModel` e `updateModel` do trait recebem o [`TextSource`] (`string | ITextBufferFactory | ITextSnapshot`).
//! - `ITextModel` é o [`TextModel`] concreto (o porte não tem a interface), e `ITextModel | null` é `Option`.
//! - `getCreationOptions(language, resource, isForSimpleWidget)` entrou para as ações de indentação
//!   (`ChangeIndentationSizeAction` e `DetectIndentation`).

use std::rc::Rc;

use je_base::event::Event;
use je_base::uri::URI;

use crate::language_service::ILanguageSelection;
use crate::model::TextModelCreationOptions;
use crate::model_service::ModelLanguageChanged;
use crate::text_model::{TextModel, TextSource};
use crate::text_model_edit_source::TextModelEditSource;

/// `IModelService` (recorte mínimo, ver as divergências do módulo).
pub trait IModelService {
    /// `getModel(resource)`: o modelo aberto para o recurso, se houver.
    fn get_model(&self, resource: &URI) -> Option<Rc<TextModel>>;

    /// `getModels()`: todos os modelos abertos.
    fn get_models(&self) -> Vec<Rc<TextModel>>;

    /// `onModelAdded`: disparado quando um modelo é criado.
    fn on_model_added(&self) -> Event<Rc<TextModel>>;

    /// `onModelRemoved`: disparado quando um modelo é destruído. O padrão é um evento que nunca dispara, para quem não
    /// tem o evento.
    fn on_model_removed(&self) -> Event<Rc<TextModel>> {
        Event::none()
    }

    /// `onModelLanguageChanged`: disparado quando o idioma de um modelo muda. O padrão é um evento que nunca dispara, para
    /// quem não o tem.
    fn on_model_language_changed(&self) -> Event<ModelLanguageChanged> {
        Event::none()
    }

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
