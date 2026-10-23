//! Porte de `src/vs/editor/test/common/modes/testLanguageConfigurationService.ts`:
//! `TestLanguageConfigurationService`, um `ILanguageConfigurationService` só sobre o registro (sem
//! `ILanguageService`) que devolve uma configuração vazia, de nome `unknown`, para a linguagem sem
//! contribuições.

use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::{
    ILanguageConfigurationService, LanguageConfigurationRegistry, LanguageConfigurationServiceChangeEvent,
    ResolvedLanguageConfiguration,
};

pub struct TestLanguageConfigurationService {
    base: Rc<Disposable>,
    registry: Rc<LanguageConfigurationRegistry>,
    on_did_change: Rc<Emitter<LanguageConfigurationServiceChangeEvent>>,
}

impl TestLanguageConfigurationService {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let registry = LanguageConfigurationRegistry::new();
        base.register(registry.clone());
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let emitter = on_did_change.clone();
        base.register(registry.on_did_change().listen(move |e| {
            emitter.fire(LanguageConfigurationServiceChangeEvent::new(Some(&e.language_id)));
        }));
        Rc::new(Self { base, registry, on_did_change })
    }
}

impl IDisposable for TestLanguageConfigurationService {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

impl ILanguageConfigurationService for TestLanguageConfigurationService {
    fn on_did_change(&self) -> Event<LanguageConfigurationServiceChangeEvent> {
        self.on_did_change.event()
    }

    fn register(&self, language_id: &str, configuration: LanguageConfiguration, priority: Option<i32>) -> DisposableRef {
        self.registry.register(language_id, configuration, priority)
    }

    fn get_language_configuration(&self, language_id: &str) -> Rc<ResolvedLanguageConfiguration> {
        self.registry
            .get_language_configuration(language_id)
            .unwrap_or_else(|| Rc::new(ResolvedLanguageConfiguration::new("unknown", LanguageConfiguration::default())))
    }
}
