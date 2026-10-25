//! Porte de `src/vs/editor/common/services/semanticTokensStylingService.ts`: o `SemanticTokensStylingService`, que guarda
//! um [`SemanticTokensProviderStyling`] por provedor e descarta todos quando o tema de cores muda.
//!
//! Diferenças do original, nenhuma de comportamento:
//! - O `WeakMap<DocumentTokensProvider, ...>` é um `HashMap` pela identidade do provedor, com uma referência fraca a ele
//!   (a entrada de um provedor solto é varrida a cada inserção; a referência fraca impede o endereço de ser reaproveitado
//!   enquanto a entrada existe).
//! - `registerSingleton(ISemanticTokensStylingService, ...)` não existe: o serviço é criado por quem o usa.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::log::LogServiceRef;

use crate::language_service::ILanguageService;
use crate::semantic_tokens_provider_styling::SemanticTokensProviderStyling;
use crate::semantic_tokens_styling::{DocumentTokensProvider, ISemanticTokensStylingService, WeakDocumentTokensProvider};
use crate::services::theme_service::IThemeService;

type Caches = RefCell<HashMap<usize, (WeakDocumentTokensProvider, Rc<SemanticTokensProviderStyling>)>>;

/// `SemanticTokensStylingService`.
pub struct SemanticTokensStylingService {
    base: Rc<Disposable>,
    theme_service: Rc<dyn IThemeService>,
    log_service: LogServiceRef,
    language_service: Rc<dyn ILanguageService>,
    caches: Rc<Caches>,
}

impl SemanticTokensStylingService {
    pub fn new(theme_service: Rc<dyn IThemeService>, log_service: LogServiceRef, language_service: Rc<dyn ILanguageService>) -> Rc<Self> {
        let base = Disposable::new();
        let caches: Rc<Caches> = Rc::new(RefCell::new(HashMap::new()));
        let reset_caches = Rc::clone(&caches);
        base.register(theme_service.on_did_color_theme_change().listen(move |_| {
            *reset_caches.borrow_mut() = HashMap::new();
        }));
        Rc::new(Self { base, theme_service, log_service, language_service, caches })
    }
}

impl ISemanticTokensStylingService for SemanticTokensStylingService {
    fn get_styling(&self, provider: &DocumentTokensProvider) -> Rc<SemanticTokensProviderStyling> {
        let key = provider.identity();
        if let Some((_, styling)) = self.caches.borrow().get(&key) {
            return Rc::clone(styling);
        }
        let styling = Rc::new(SemanticTokensProviderStyling::new(
            provider.get_legend(),
            Rc::clone(&self.theme_service),
            Rc::clone(&self.language_service),
            Rc::clone(&self.log_service),
        ));
        let mut caches = self.caches.borrow_mut();
        caches.retain(|_, (weak_provider, _)| !weak_provider.is_dead());
        caches.insert(key, (provider.downgrade(), Rc::clone(&styling)));
        styling
    }
}

impl IDisposable for SemanticTokensStylingService {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}
