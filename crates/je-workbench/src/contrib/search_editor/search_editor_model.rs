//! Porte parcial de `src/vs/workbench/contrib/searchEditor/browser/searchEditorModel.ts`: `SearchEditorModel`,
//! `SearchConfigurationModel` e o registro de modelos por recurso (`searchEditorModelFactory.models`).
//!
//! Divergências:
//! - Os três `initializeModelFrom*` e o `tryFetchModelFromBackupService` da fábrica ficam de fora: dependem do
//!   `IModelService`, do `ILanguageService`, do `IWorkingCopyBackupService` e do `searchEditorSerialization.ts`, que ainda
//!   não têm porte. O registro (`models`) e as duas classes públicas são fiéis.
//! - `searchEditorModelFactory` é uma instância por thread ([`search_editor_model_factory`]), porque o mapa guarda `Rc`.
//! - `models` é um `RefCell<ResourceMap<..>>`; `SearchEditorData.resultsModel` é `Rc<TextModel>` e a
//!   `configurationModel` é `Rc<SearchConfigurationModel>`.
//! - `SearchEditorModel.resolve` devolve a `Promise` da referência (o `async` do original só repassa essa promessa).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::types::assert_returns_defined;
use je_base::uri::URI;
use je_editor::text_model::TextModel;

use super::constants::SearchConfiguration;

/// `SearchEditorData`.
#[derive(Clone)]
pub struct SearchEditorData {
    pub results_model: Rc<TextModel>,
    pub configuration_model: Rc<SearchConfigurationModel>,
}

/// `ISearchEditorModelReference`.
pub trait ISearchEditorModelReference {
    fn resolve(&self) -> Promise<SearchEditorData>;
}

/// `SearchConfigurationModel`.
pub struct SearchConfigurationModel {
    on_config_did_update: Rc<Emitter<SearchConfiguration>>,
    config: RefCell<SearchConfiguration>,
}

impl SearchConfigurationModel {
    pub fn new(config: SearchConfiguration) -> Self {
        Self { on_config_did_update: Emitter::new(), config: RefCell::new(config) }
    }

    /// `onConfigDidUpdate`.
    pub fn on_config_did_update(&self) -> Event<SearchConfiguration> {
        self.on_config_did_update.event()
    }

    /// `config`.
    pub fn config(&self) -> SearchConfiguration {
        self.config.borrow().clone()
    }

    pub fn update_config(&self, config: SearchConfiguration) {
        *self.config.borrow_mut() = config.clone();
        self.on_config_did_update.fire(config);
    }
}

/// `SearchEditorModel`.
pub struct SearchEditorModel {
    resource: URI,
    model_reference: Rc<dyn ISearchEditorModelReference>,
}

impl SearchEditorModel {
    pub fn new(resource: URI) -> Self {
        let model_reference = assert_returns_defined(search_editor_model_factory().models.borrow().get(&resource).cloned());
        Self { resource, model_reference }
    }

    pub fn resolve(&self) -> Promise<SearchEditorData> {
        self.model_reference.resolve()
    }

    pub fn dispose(&self) {
        let factory = search_editor_model_factory();
        let is_current = factory
            .models
            .borrow()
            .get(&self.resource)
            .is_some_and(|current| Rc::ptr_eq(current, &self.model_reference));
        if is_current {
            factory.models.borrow_mut().delete(&self.resource);
        }
    }
}

/// `SearchEditorModelFactory` (só o registro `models`).
pub struct SearchEditorModelFactory {
    pub models: RefCell<ResourceMap<Rc<dyn ISearchEditorModelReference>>>,
}

thread_local! {
    static SEARCH_EDITOR_MODEL_FACTORY: &'static SearchEditorModelFactory =
        Box::leak(Box::new(SearchEditorModelFactory { models: RefCell::new(ResourceMap::new()) }));
}

/// `searchEditorModelFactory`.
pub fn search_editor_model_factory() -> &'static SearchEditorModelFactory {
    SEARCH_EDITOR_MODEL_FACTORY.with(|factory| *factory)
}
