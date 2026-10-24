//! Porte de `src/vs/editor/contrib/wordHighlighter/browser/textualHighlightProvider.ts`: o provedor de destaques por texto
//! (`TextualDocumentHighlightProvider`, que implementa `DocumentHighlightProvider` e `MultiDocumentHighlightProvider`) e o
//! recurso que o registra para todas as linguagens (`TextualMultiDocumentHighlightFeature`).
//!
//! Representação:
//! - O `instanceof TextualDocumentHighlightProvider` de `getOccurrencesAtPosition` é
//!   [`DocumentHighlightProvider::is_textual_provider`], que só este provedor responde com `true`.
//! - `selector: LanguageFilter = { language: '*' }` é o seletor guardado em [`TextualDocumentHighlightProvider::selector`].
//! - `ILanguageFeaturesService` é o par de registros que o construtor de [`TextualMultiDocumentHighlightFeature`] recebe
//!   (`documentHighlightProvider` e `multiDocumentHighlightProvider`, os dois que ele lê).
//! - `provideDocumentHighlights` e `provideMultiDocumentHighlights` devolvem `undefined` com o modelo descartado (`None`) e
//!   uma promessa resolvida ou um valor (sempre promessa aqui) nos demais casos.
//!
//! Divergências:
//! - O `registerEditorFeature(TextualMultiDocumentHighlightFeature)` do fim do arquivo (o registro de recursos de editor,
//!   que instancia o recurso com o `IInstantiationService`) não existe: quem monta os serviços cria o recurso.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceMap;
use je_base::promise::Promise;

use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_providers::{
    DocumentHighlight, DocumentHighlightKind, DocumentHighlightProvider, HighlightMap, MultiDocumentHighlightProvider,
    ProviderResult,
};
use crate::language_selector::{LanguageFilter, LanguageSelector};
use crate::position::Position;
use crate::text_model::{FindScope, TextModel};
use crate::text_model_search::LIMIT_FIND_COUNT;
use crate::word_helper::USUAL_WORD_SEPARATORS;

/// `TextualDocumentHighlightProvider`.
pub struct TextualDocumentHighlightProvider {
    selector: LanguageSelector,
}

impl TextualDocumentHighlightProvider {
    pub fn new() -> Self {
        Self { selector: LanguageSelector::Filter(LanguageFilter { language: Some("*".to_owned()), ..LanguageFilter::default() }) }
    }
}

impl Default for TextualDocumentHighlightProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// `model.findMatches(word.word, true, false, true, USUAL_WORD_SEPARATORS, false)` como destaques de texto.
fn text_highlights(model: &TextModel, word: &je_base::JsString) -> Vec<DocumentHighlight> {
    model
        .find_matches(word.clone(), FindScope::Whole, false, true, Some(USUAL_WORD_SEPARATORS), false, LIMIT_FIND_COUNT)
        .into_iter()
        .map(|m| DocumentHighlight { range: m.range, kind: Some(DocumentHighlightKind::Text) })
        .collect()
}

impl DocumentHighlightProvider for TextualDocumentHighlightProvider {
    fn provide_document_highlights(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        _token: &CancellationToken,
    ) -> ProviderResult<Vec<DocumentHighlight>> {
        if model.is_disposed() {
            return Promise::resolved(None);
        }

        let Some(word) = model.get_word_at_position(position) else {
            return Promise::resolved(Some(Vec::new()));
        };

        Promise::resolved(Some(text_highlights(model, &word.word)))
    }

    fn is_textual_provider(&self) -> bool {
        true
    }
}

impl MultiDocumentHighlightProvider for TextualDocumentHighlightProvider {
    fn selector(&self) -> &LanguageSelector {
        &self.selector
    }

    fn provide_multi_document_highlights(
        &self,
        primary_model: &Rc<TextModel>,
        position: Position,
        other_models: &[Rc<TextModel>],
        _token: &CancellationToken,
    ) -> ProviderResult<HighlightMap> {
        if primary_model.is_disposed() {
            return Promise::resolved(None);
        }

        let mut result: ResourceMap<Vec<DocumentHighlight>> = ResourceMap::new();

        let Some(word) = primary_model.get_word_at_position(position) else {
            return Promise::resolved(Some(Rc::new(result)));
        };

        for model in std::iter::once(primary_model).chain(other_models) {
            if model.is_disposed() {
                continue;
            }

            result.set(model.uri(), text_highlights(model, &word.word));
        }

        Promise::resolved(Some(Rc::new(result)))
    }
}

/// `TextualMultiDocumentHighlightFeature`.
pub struct TextualMultiDocumentHighlightFeature {
    base: Rc<Disposable>,
}

impl TextualMultiDocumentHighlightFeature {
    /// `new TextualMultiDocumentHighlightFeature(languageFeaturesService)`, com os dois registros que ele lê.
    pub fn new(
        document_highlight_provider: &LanguageFeatureRegistry<dyn DocumentHighlightProvider>,
        multi_document_highlight_provider: &LanguageFeatureRegistry<dyn MultiDocumentHighlightProvider>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        base.register(document_highlight_provider.register(LanguageSelector::from("*"), Rc::new(TextualDocumentHighlightProvider::new())));
        base.register(
            multi_document_highlight_provider.register(LanguageSelector::from("*"), Rc::new(TextualDocumentHighlightProvider::new())),
        );
        Rc::new(Self { base })
    }
}

impl IDisposable for TextualMultiDocumentHighlightFeature {
    fn dispose(&self) {
        self.base.dispose();
    }
}
