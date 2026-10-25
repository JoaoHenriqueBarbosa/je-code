//! Porte de `src/vs/editor/common/services/semanticTokensStyling.ts` (`ISemanticTokensStylingService`) e do tipo
//! `DocumentTokensProvider` de `src/vs/editor/common/services/model.ts`.
//!
//! `DocumentTokensProvider` é a união `DocumentSemanticTokensProvider | DocumentRangeSemanticTokensProvider` do
//! original: aqui é o enum [`DocumentTokensProvider`], que expõe o que o serviço lê (`getLegend()`) e a identidade do
//! provedor (a chave do `WeakMap` do serviço).

use std::rc::{Rc, Weak};

use crate::language_features_service::{DocumentRangeSemanticTokensProvider, DocumentSemanticTokensProvider, SemanticTokensLegend};
use crate::semantic_tokens_provider_styling::SemanticTokensProviderStyling;

/// `DocumentTokensProvider`.
#[derive(Clone)]
pub enum DocumentTokensProvider {
    Document(Rc<dyn DocumentSemanticTokensProvider>),
    Range(Rc<dyn DocumentRangeSemanticTokensProvider>),
}

impl DocumentTokensProvider {
    /// `getLegend()`.
    pub fn get_legend(&self) -> SemanticTokensLegend {
        match self {
            Self::Document(provider) => provider.get_legend(),
            Self::Range(provider) => provider.get_legend(),
        }
    }

    /// A identidade do provedor (o `===` do JavaScript).
    pub fn identity(&self) -> usize {
        match self {
            Self::Document(provider) => Rc::as_ptr(provider).cast::<()>() as usize,
            Self::Range(provider) => Rc::as_ptr(provider).cast::<()>() as usize,
        }
    }

    /// A referência fraca ao provedor, para a chave de um `WeakMap`.
    pub fn downgrade(&self) -> WeakDocumentTokensProvider {
        match self {
            Self::Document(provider) => WeakDocumentTokensProvider::Document(Rc::downgrade(provider)),
            Self::Range(provider) => WeakDocumentTokensProvider::Range(Rc::downgrade(provider)),
        }
    }
}

impl From<Rc<dyn DocumentSemanticTokensProvider>> for DocumentTokensProvider {
    fn from(provider: Rc<dyn DocumentSemanticTokensProvider>) -> Self {
        Self::Document(provider)
    }
}

impl From<Rc<dyn DocumentRangeSemanticTokensProvider>> for DocumentTokensProvider {
    fn from(provider: Rc<dyn DocumentRangeSemanticTokensProvider>) -> Self {
        Self::Range(provider)
    }
}

/// A chave fraca de um [`DocumentTokensProvider`].
pub enum WeakDocumentTokensProvider {
    Document(Weak<dyn DocumentSemanticTokensProvider>),
    Range(Weak<dyn DocumentRangeSemanticTokensProvider>),
}

impl WeakDocumentTokensProvider {
    /// O provedor já foi solto (a entrada do `WeakMap` sumiria).
    pub fn is_dead(&self) -> bool {
        match self {
            Self::Document(provider) => provider.strong_count() == 0,
            Self::Range(provider) => provider.strong_count() == 0,
        }
    }
}

/// `ISemanticTokensStylingService`.
pub trait ISemanticTokensStylingService {
    /// `getStyling(provider)`: o estilo (com o cache) do provedor; o mesmo objeto enquanto o tema não mudar.
    fn get_styling(&self, provider: &DocumentTokensProvider) -> Rc<SemanticTokensProviderStyling>;
}

je_instantiation::service_id!(pub I_SEMANTIC_TOKENS_STYLING_SERVICE: dyn ISemanticTokensStylingService = "semanticTokensStylingService");
