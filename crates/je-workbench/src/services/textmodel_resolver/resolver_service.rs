//! Porte de `src/vs/editor/common/services/resolverService.ts`: o `ITextEditorModel` (o modelo de editor que guarda um
//! modelo de texto), a referência a ele (`IReference<IResolvedTextEditorModel>`), o `ITextModelContentProvider` e o
//! `ITextModelService`. A implementação do serviço é o `TextModelResolverService`
//! ([`text_model_resolver_service`](super::text_model_resolver_service)).
//!
//! Divergências:
//! - **`ITextEditorModel extends IResolvableEditorModel`.** É o trait [`ITextEditorModel`], que tem o [`IEditorModel`]
//!   como supertrait (o modelo de editor do workbench, que cobre `resolve` e `isResolved`). O `getLanguageId()` do
//!   original não está no trait (cada modelo concreto o tem como método próprio); quem precisa dele lê o modelo concreto
//!   por [`IEditorModel::as_any`].
//! - **`IResolvedTextEditorModel`** (`textEditorModel` nunca nulo) é o mesmo trait: [`is_resolved_text_editor_model`] é o
//!   `isResolvedTextEditorModel`, e quem recebe um modelo resolvido lê `text_editor_model()` sabendo que não é `None`.
//! - **`createSnapshot`** tem duas sobrecargas por `this` (`IResolvedTextEditorModel` devolve `ITextSnapshot`,
//!   `ITextEditorModel` devolve `ITextSnapshot | null`); aqui é uma só, que devolve `Option`.
//! - **`IReference<T>`** é o [`Reference`] de `je_base::lifecycle` (o objeto e o descarte da referência, de chamada
//!   única); [`TextModelReference`] é o `IReference<IResolvedTextEditorModel>`.
//! - **`ITextModelService`.** Os três membros além do `createModelReference` (`createSyntheticDocument`,
//!   `registerTextModelContentProvider` e `canHandleResource`) têm corpo padrão que rejeita com "não suportado": é o que um
//!   dublê que só implementa o `createModelReference` (o `mock<ITextModelService>()` do original) responde, e deixa os
//!   dublês existentes valendo. O `TextModelResolverService` sobrescreve os quatro.
//! - **`ITextModelContentProvider.provideTextContent`** devolve `Promise<ITextModel | null> | null`, que é o
//!   [`MaybePromise`] de `Option<Rc<TextModel>>` (o `ITextModel` é o [`TextModel`] concreto).
//! - **`ITextBufferFactory | string`** do `createSyntheticDocument` é o [`TextSource`], e o `ILanguageSelection | null` é
//!   `Option<Rc<dyn ILanguageSelection>>`.

use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, Reference};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::language_service::ILanguageSelection;
use je_editor::model::TextSnapshot;
use je_editor::text_model::{TextModel, TextSource};
use je_instantiation::service_id;

use crate::common::editor::editor_model::IEditorModel;
use crate::services::files_configuration::files_configuration_service::Readonly;

/// `ITextEditorModel`.
pub trait ITextEditorModel: IEditorModel {
    /// `textEditorModel`: provides access to the underlying `ITextModel` (`None` quando o modelo não foi resolvido).
    fn text_editor_model(&self) -> Option<Rc<TextModel>>;

    /// `isReadonly()`: devolve se o modelo é somente leitura, e o motivo quando há um (`IMarkdownString`).
    fn is_readonly(&self) -> Readonly;

    /// `createSnapshot()`: a snapshot of the text of the underlying model, `None` quando não há modelo.
    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>>;
}

/// `isResolvedTextEditorModel(model)`.
pub fn is_resolved_text_editor_model(model: &dyn ITextEditorModel) -> bool {
    model.text_editor_model().is_some()
}

/// `IReference<IResolvedTextEditorModel>`: o modelo e o descarte da referência.
pub type TextModelReference = Reference<Rc<dyn ITextEditorModel>>;

/// `ITextModelContentProvider`: a provider of text content for a scheme.
pub trait ITextModelContentProvider {
    /// Given a resource, return the content of the resource as `ITextModel` (`None` quando o provedor não tem o conteúdo).
    fn provide_text_content(&self, resource: &URI) -> MaybePromise<Option<Rc<TextModel>>>;
}

/// `ITextModelService`.
pub trait ITextModelService {
    /// Provided a resource URI, it will return a model reference which should be disposed once not needed anymore.
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference>;

    /// Creates a synthetic (in-memory) document with the given content and language, and returns a reference to its
    /// model. The document gets a unique `inmemory` URI and lives until its last reference is disposed.
    fn create_synthetic_document(&self, _value: TextSource, _language_selection: Option<Rc<dyn ILanguageSelection>>) -> Promise<TextModelReference> {
        Promise::rejected(not_supported("createSyntheticDocument"))
    }

    /// Registers a specific `scheme` content provider.
    fn register_text_model_content_provider(&self, _scheme: &str, _provider: Rc<dyn ITextModelContentProvider>) -> DisposableRef {
        errors::throw(not_supported("registerTextModelContentProvider"))
    }

    /// Check if the given resource can be resolved to a text model.
    fn can_handle_resource(&self, _resource: &URI) -> bool {
        errors::throw(not_supported("canHandleResource"))
    }
}

/// O erro de um membro de `ITextModelService` que o dublê não implementa.
fn not_supported(member: &str) -> Error {
    Error::new(format!("ITextModelService.{member} is not supported by this service"))
}

service_id!(pub I_TEXT_MODEL_SERVICE: dyn ITextModelService = "textModelService");
