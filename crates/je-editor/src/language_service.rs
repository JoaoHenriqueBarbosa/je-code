//! Porte de `src/vs/editor/common/languages/language.ts` (`ILanguageService`, `ILanguageSelection`) e de
//! `src/vs/editor/common/services/languageService.ts` (`LanguageService`).
//!
//! Divergência: `LanguageSelection` do original é um observável derivado do sinal de mudança do serviço;
//! aqui `language_id()` recalcula o seletor a cada leitura e `on_did_change()` assina o evento do serviço e
//! só dispara quando o valor muda (o que o `Event.fromObservable` faz).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::uri::URI;

use crate::languages::ILanguageIdCodec;
use crate::languages_registry::{LanguageNameIdPair, LanguagesRegistry};
use crate::modes_registry::{LanguageExtensionPoint, LanguageIcon, PLAINTEXT_LANGUAGE_ID};
use crate::tokenization_registry::tokenization_registry;

/// `ILanguageSelection`.
pub trait ILanguageSelection {
    fn language_id(&self) -> String;
    fn on_did_change(&self) -> Event<String>;
}

/// `ILanguageService`.
pub trait ILanguageService {
    fn language_id_codec(&self) -> Rc<dyn ILanguageIdCodec>;
    fn on_did_request_basic_language_features(&self) -> Event<String>;
    fn on_did_request_rich_language_features(&self) -> Event<String>;
    fn on_did_change(&self) -> Event<()>;
    fn register_language(&self, def: LanguageExtensionPoint) -> DisposableRef;
    fn is_registered_language_id(&self, language_id: Option<&str>) -> bool;
    fn get_registered_language_ids(&self) -> Vec<String>;
    fn get_sorted_registered_language_names(&self) -> Vec<LanguageNameIdPair>;
    fn get_language_name(&self, language_id: &str) -> Option<String>;
    fn get_mime_type(&self, language_id: &str) -> Option<String>;
    fn get_icon(&self, language_id: &str) -> Option<LanguageIcon>;
    fn get_extensions(&self, language_id: &str) -> Vec<String>;
    fn get_filenames(&self, language_id: &str) -> Vec<String>;
    fn get_configuration_files(&self, language_id: &str) -> Vec<URI>;
    fn get_language_id_by_language_name(&self, language_name: &str) -> Option<String>;
    fn get_language_id_by_mime_type(&self, mime_type: Option<&str>) -> Option<String>;
    fn guess_language_id_by_filepath_or_first_line(&self, resource: Option<&URI>, first_line: Option<&JsString>) -> Option<String>;
    fn create_by_id(&self, language_id: Option<&str>) -> Rc<dyn ILanguageSelection>;
    fn create_by_mime_type(&self, mime_type: Option<&str>) -> Rc<dyn ILanguageSelection>;
    fn create_by_filepath_or_first_line(&self, resource: Option<&URI>, first_line: Option<&JsString>) -> Rc<dyn ILanguageSelection>;
    fn request_basic_language_features(&self, language_id: &str);
    fn request_rich_language_features(&self, language_id: &str);
}

je_instantiation::service_id!(pub I_LANGUAGE_SERVICE: dyn ILanguageService = "languageService");

thread_local! {
    static INSTANCE_COUNT: Cell<i32> = const { Cell::new(0) };
}

/// `LanguageService`.
pub struct LanguageService {
    me: Weak<LanguageService>,
    base: Rc<Disposable>,
    on_did_request_basic_language_features: Rc<Emitter<String>>,
    on_did_request_rich_language_features: Rc<Emitter<String>>,
    on_did_change: Rc<Emitter<()>>,
    requested_basic_languages: RefCell<HashSet<String>>,
    requested_rich_languages: RefCell<HashSet<String>>,
    registry: Rc<LanguagesRegistry>,
}

impl LanguageService {
    /// `LanguageService.instanceCount`.
    pub fn instance_count() -> i32 {
        INSTANCE_COUNT.with(Cell::get)
    }

    pub fn new(warn_on_overwrite: bool) -> Rc<LanguageService> {
        INSTANCE_COUNT.with(|c| c.set(c.get() + 1));
        let base = Disposable::new();
        let on_did_request_basic_language_features = Emitter::new();
        let on_did_request_rich_language_features = Emitter::new();
        let on_did_change = Emitter::new();
        base.register(on_did_request_basic_language_features.clone());
        base.register(on_did_request_rich_language_features.clone());
        base.register(on_did_change.clone());
        let registry = LanguagesRegistry::new(true, warn_on_overwrite);
        base.register(registry.clone());
        let forward = on_did_change.clone();
        base.register(registry.on_did_change().listen(move |_| forward.fire(())));
        Rc::new_cyclic(|me| LanguageService {
            me: me.clone(),
            base,
            on_did_request_basic_language_features,
            on_did_request_rich_language_features,
            on_did_change,
            requested_basic_languages: RefCell::new(HashSet::new()),
            requested_rich_languages: RefCell::new(HashSet::new()),
            registry,
        })
    }

    /// `WorkbenchLanguageService`: as linguagens contribuídas pelas extensões (`setDynamicLanguages`).
    pub fn set_dynamic_languages(&self, def: Vec<LanguageExtensionPoint>) {
        self.registry.set_dynamic_languages(def);
    }

    /// Só para teste: o `languageService['_onDidChange']['_size']` do original.
    #[doc(hidden)]
    pub fn on_did_change_listener_count_for_testing(&self) -> usize {
        self.on_did_change.size_for_testing()
    }

    fn create_and_get_language_identifier(&self, language_id: Option<&str>) -> String {
        match language_id {
            Some(id) if !id.is_empty() && self.is_registered_language_id(Some(id)) => id.to_owned(),
            _ => PLAINTEXT_LANGUAGE_ID.to_owned(),
        }
    }

    fn selection(&self, selector: impl Fn(&LanguageService) -> String + 'static) -> Rc<dyn ILanguageSelection> {
        Rc::new(LanguageSelection { service: self.me.clone(), selector: Rc::new(selector) })
    }
}

impl IDisposable for LanguageService {
    fn dispose(&self) {
        INSTANCE_COUNT.with(|c| c.set(c.get() - 1));
        IDisposable::dispose(&*self.base);
    }
}

impl ILanguageService for LanguageService {
    fn language_id_codec(&self) -> Rc<dyn ILanguageIdCodec> {
        self.registry.language_id_codec.clone()
    }

    fn on_did_request_basic_language_features(&self) -> Event<String> {
        self.on_did_request_basic_language_features.event()
    }

    fn on_did_request_rich_language_features(&self) -> Event<String> {
        self.on_did_request_rich_language_features.event()
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn register_language(&self, def: LanguageExtensionPoint) -> DisposableRef {
        self.registry.register_language(def)
    }

    fn is_registered_language_id(&self, language_id: Option<&str>) -> bool {
        self.registry.is_registered_language_id(language_id)
    }

    fn get_registered_language_ids(&self) -> Vec<String> {
        self.registry.get_registered_language_ids()
    }

    fn get_sorted_registered_language_names(&self) -> Vec<LanguageNameIdPair> {
        self.registry.get_sorted_registered_language_names()
    }

    fn get_language_name(&self, language_id: &str) -> Option<String> {
        self.registry.get_language_name(language_id)
    }

    fn get_mime_type(&self, language_id: &str) -> Option<String> {
        self.registry.get_mime_type(language_id)
    }

    fn get_icon(&self, language_id: &str) -> Option<LanguageIcon> {
        self.registry.get_icon(language_id)
    }

    fn get_extensions(&self, language_id: &str) -> Vec<String> {
        self.registry.get_extensions(language_id)
    }

    fn get_filenames(&self, language_id: &str) -> Vec<String> {
        self.registry.get_filenames(language_id)
    }

    fn get_configuration_files(&self, language_id: &str) -> Vec<URI> {
        self.registry.get_configuration_files(language_id)
    }

    fn get_language_id_by_language_name(&self, language_name: &str) -> Option<String> {
        self.registry.get_language_id_by_language_name(language_name)
    }

    fn get_language_id_by_mime_type(&self, mime_type: Option<&str>) -> Option<String> {
        self.registry.get_language_id_by_mime_type(mime_type)
    }

    fn guess_language_id_by_filepath_or_first_line(&self, resource: Option<&URI>, first_line: Option<&JsString>) -> Option<String> {
        self.registry.guess_language_id_by_filepath_or_first_line(resource, first_line).into_iter().next()
    }

    fn create_by_id(&self, language_id: Option<&str>) -> Rc<dyn ILanguageSelection> {
        let language_id = language_id.map(str::to_owned);
        self.selection(move |s| s.create_and_get_language_identifier(language_id.as_deref()))
    }

    fn create_by_mime_type(&self, mime_type: Option<&str>) -> Rc<dyn ILanguageSelection> {
        let mime_type = mime_type.map(str::to_owned);
        self.selection(move |s| {
            let language_id = s.get_language_id_by_mime_type(mime_type.as_deref());
            s.create_and_get_language_identifier(language_id.as_deref())
        })
    }

    fn create_by_filepath_or_first_line(&self, resource: Option<&URI>, first_line: Option<&JsString>) -> Rc<dyn ILanguageSelection> {
        let (resource, first_line) = (resource.cloned(), first_line.cloned());
        self.selection(move |s| {
            let language_id = s.guess_language_id_by_filepath_or_first_line(resource.as_ref(), first_line.as_ref());
            s.create_and_get_language_identifier(language_id.as_deref())
        })
    }

    fn request_basic_language_features(&self, language_id: &str) {
        if self.requested_basic_languages.borrow_mut().insert(language_id.to_owned()) {
            self.on_did_request_basic_language_features.fire(language_id.to_owned());
        }
    }

    fn request_rich_language_features(&self, language_id: &str) {
        if self.requested_rich_languages.borrow_mut().insert(language_id.to_owned()) {
            // Ensure basic features are requested
            self.request_basic_language_features(language_id);

            // Ensure tokenizers are created
            let _ = tokenization_registry().get_or_create(language_id);

            // Ask for other features
            self.on_did_request_rich_language_features.fire(language_id.to_owned());
        }
    }
}

struct LanguageSelection {
    service: Weak<LanguageService>,
    selector: Rc<dyn Fn(&LanguageService) -> String>,
}

impl ILanguageSelection for LanguageSelection {
    fn language_id(&self) -> String {
        match self.service.upgrade() {
            Some(service) => (self.selector)(&service),
            None => PLAINTEXT_LANGUAGE_ID.to_owned(),
        }
    }

    fn on_did_change(&self) -> Event<String> {
        let (service, selector) = (self.service.clone(), self.selector.clone());
        Event::new(move |listener| {
            let Some(strong) = service.upgrade() else { return je_base::lifecycle::none() };
            let last = Rc::new(RefCell::new(selector(&strong)));
            let (service, selector) = (service.clone(), selector.clone());
            strong.on_did_change().listen(move |_| {
                let Some(service) = service.upgrade() else { return };
                let value = selector(&service);
                if *last.borrow() != value {
                    *last.borrow_mut() = value.clone();
                    listener(&value);
                }
            })
        })
    }
}
