//! Porte parcial de `src/vs/editor/common/services/resolverService.ts`: o `ITextEditorModel` (o modelo de editor que
//! guarda um modelo de texto), a referência a ele (`IReference<IResolvedTextEditorModel>`) e o `ITextModelService`
//! reduzido ao `createModelReference`.
//!
//! Divergências:
//! - **`ITextEditorModel extends IResolvableEditorModel`.** É o trait [`ITextEditorModel`], que tem o [`IEditorModel`]
//!   como supertrait (o modelo de editor do workbench, que cobre `resolve` e `isResolved`).
//! - **`IResolvedTextEditorModel`** (`textEditorModel` nunca nulo) é o mesmo trait: [`is_resolved_text_editor_model`] é o
//!   `isResolvedTextEditorModel`, e quem recebe um modelo resolvido lê `text_editor_model()` sabendo que não é `None`.
//! - **`createSnapshot`** tem duas sobrecargas por `this` (`IResolvedTextEditorModel` devolve `ITextSnapshot`,
//!   `ITextEditorModel` devolve `ITextSnapshot | null`); aqui é uma só, que devolve `Option`.
//! - **`IReference<T>`** é a [`TextModelReference`] (o objeto e o descarte da referência, de chamada única, que o
//!   `ReferenceCollection` do serviço de resolução monta).
//! - **`ITextModelService`** só tem `createModelReference`: `createSyntheticDocument`, `registerTextModelContentProvider`,
//!   `canHandleResource` e `hasTextModelContentProvider` (e o `ITextModelContentProvider`) entram com o porte do
//!   `TextModelResolverService`.

use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model::TextSnapshot;
use je_editor::text_model::TextModel;
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
#[derive(Clone)]
pub struct TextModelReference {
    /// `object`.
    pub object: Rc<dyn ITextEditorModel>,
    release: DisposableRef,
}

impl TextModelReference {
    /// `release` é de chamada única, como o `dispose` de uma `IReference`.
    pub fn new(object: Rc<dyn ITextEditorModel>, release: DisposableRef) -> Self {
        Self { object, release }
    }
}

impl IDisposable for TextModelReference {
    fn dispose(&self) {
        self.release.dispose();
    }
}

/// `ITextModelService`.
pub trait ITextModelService {
    /// Provided a resource URI, it will return a model reference which should be disposed once not needed anymore.
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference>;
}

service_id!(pub I_TEXT_MODEL_SERVICE: dyn ITextModelService = "textModelService");
