//! Porte de `src/vs/editor/contrib/wordHighlighter/browser/wordHighlighter.ts`, a parte que decide os destaques: a
//! consulta aos provedores ([`get_occurrences_at_position`] e [`get_occurrences_across_multiple_models`]), a fusão dos
//! resultados (`mergeHighlights`) e os pedidos de ocorrências (`OccurenceAtPositionRequest` com as subclasses
//! `SemanticOccurenceAtPositionRequest` e `MultiModelOccurenceRequest`, criados por [`compute_occurences_at_position`] e
//! [`compute_occurences_multi_model`]).
//!
//! Representação:
//! - `ResourceMap<DocumentHighlight[]>` (o retorno compartilhado por referência) é o [`HighlightMap`] (um
//!   `Rc<ResourceMap<...>>`). O `ResourceMap | null | undefined` das funções de consulta nunca é `null` nem `undefined` no
//!   original (todo caminho devolve um mapa, vazio quando não há resposta), então [`get_occurrences_at_position`] devolve
//!   um mapa sempre; em [`get_occurrences_across_multiple_models`] o `null | undefined` (nenhum provedor respondeu) é
//!   `None`.
//! - `OccurenceAtPositionRequest` (classe abstrata com o `_compute` abstrato) e as duas subclasses são a struct
//!   [`OccurenceAtPositionRequest`], que guarda o `_compute` como um fecho montado por cada função `compute_*` (que
//!   captura o registro e as opções da subclasse). `IOccurenceAtPositionRequest` é o conjunto de métodos dela. O `result`
//!   (calculado na primeira leitura, uma `CancelablePromise`) é [`OccurenceAtPositionRequest::result`].
//! - O `instanceof TextualDocumentHighlightProvider` é [`DocumentHighlightProvider::is_textual_provider`]. Um provedor
//!   que lança de forma síncrona devolve uma promessa rejeitada; o resultado é o mesmo (o erro vai para
//!   `onUnexpectedExternalError`, na ordem dos provedores).
//! - `computeOccurencesAtPosition` e `computeOccurencesMultiModel` são os construtores dos dois pedidos.
//! - `first(factories, shouldStop)` é [`first`](je_base::async_::first) com `T = Option<...>` e o `shouldStop` do
//!   original (`!== undefined && !== null`, que é `is_some`).
//!
//! Divergências (o que este módulo ainda não porta do arquivo):
//! - `WordHighlighter` e `WordHighlighterContribution` (e as ações `editor.action.wordHighlight.next`, `.prev` e `.trigger`)
//!   não estão portados: dependem de serviços que o porte ainda não tem, `ICodeEditorService` com `listCodeEditors` e
//!   `getFocusedCodeEditor` (o recorte do porte, `ICodeEditorLookup`, só traz o editor com foco e o ativo),
//!   `IConfigurationService` com o valor `editor.occurrencesHighlightDelay` e o `IContextKeyService` ligado ao editor
//!   (`hasWordHighlights`), além de um `ILanguageFeaturesService` nos serviços do editor de teste.
//! - O comando `_executeDocumentHighlights` (`registerModelAndPositionCommand`) devolve a promessa dos destaques a quem o
//!   chama; o retorno do `CommandHandler` é um `Dynamic`, que não carrega a promessa.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::arrays::distinct;
use je_base::async_::{CancelablePromise, PromiseFactory, create_cancelable_promise, first};
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, on_unexpected_external_error};
use je_base::map::ResourceMap;
use je_base::promise::Promise;

use crate::editor_decorations_collection::EditorDecorationsCollection;
use crate::language_feature_registry::{LanguageFeatureRegistry, should_synchronize_model};
use crate::language_providers::{DocumentHighlight, DocumentHighlightProvider, HighlightMap, MultiDocumentHighlightProvider};
use crate::language_selector::score;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// O `.then(undefined, onUnexpectedExternalError)` de uma consulta: a rejeição vai para o tratador e vira `undefined`.
fn report_provider_failure(
    result: Result<Option<Vec<DocumentHighlight>>, Error>,
) -> Result<Option<Vec<DocumentHighlight>>, Error> {
    match result {
        Ok(highlights) => Ok(highlights),
        Err(error) => {
            on_unexpected_external_error(&error);
            Ok(None)
        }
    }
}

/// `getOccurrencesAtPosition(registry, model, position, token, mergeProviders)`.
pub fn get_occurrences_at_position(
    registry: &LanguageFeatureRegistry<dyn DocumentHighlightProvider>,
    model: &Rc<TextModel>,
    position: Position,
    token: &CancellationToken,
    merge_providers: bool,
) -> Promise<HighlightMap> {
    let ordered_by_score = registry.ordered(model, false);
    let model_uri = model.uri();

    if merge_providers {
        let providers: Vec<Rc<dyn DocumentHighlightProvider>> =
            ordered_by_score.iter().filter(|provider| !provider.is_textual_provider()).cloned().collect();
        let chosen = if !providers.is_empty() { &providers } else { &ordered_by_score };
        let requests: Vec<Promise<Option<Vec<DocumentHighlight>>>> = chosen
            .iter()
            .map(|provider| provider.provide_document_highlights(model, position, token).then_result(report_provider_failure))
            .collect();
        return Promise::all(requests).then(move |results| {
            let result: Vec<Vec<DocumentHighlight>> = results.into_iter().flatten().collect();
            let mut map = ResourceMap::new();
            if !result.is_empty() {
                map.set(model_uri, merge_highlights(&result));
            }
            Rc::new(map)
        });
    }

    // in order of score ask the occurrences provider
    // until someone response with a good result
    // (good = non undefined and non null value)
    // (result of size == 0 is valid, no highlights is a valid/expected result -- not a signal to fall back to other providers)
    let factories: Vec<PromiseFactory<Option<Vec<DocumentHighlight>>>> = ordered_by_score
        .into_iter()
        .map(|provider| {
            let (model, token) = (Rc::clone(model), token.clone());
            let factory: PromiseFactory<Option<Vec<DocumentHighlight>>> = Box::new(move || {
                provider.provide_document_highlights(&model, position, &token).then_result(report_provider_failure)
            });
            factory
        })
        .collect();
    first(factories, Option::is_some, None).then(move |result| {
        let mut map = ResourceMap::new();
        if let Some(Some(result)) = result {
            map.set(model_uri, result);
        }
        Rc::new(map)
    })
}

/// `mergeHighlights(results)`: junta as listas na ordem dos provedores e tira os intervalos repetidos (fica o primeiro).
fn merge_highlights(results: &[Vec<DocumentHighlight>]) -> Vec<DocumentHighlight> {
    let flat: Vec<DocumentHighlight> = results.iter().flatten().copied().collect();
    distinct(&flat, |highlight| {
        let range = highlight.range;
        format!("{}:{}:{}:{}", range.start_line_number, range.start_column, range.end_line_number, range.end_column)
    })
}

/// `getOccurrencesAcrossMultipleModels(registry, model, position, token, otherModels)`.
pub fn get_occurrences_across_multiple_models(
    registry: &LanguageFeatureRegistry<dyn MultiDocumentHighlightProvider>,
    model: &Rc<TextModel>,
    position: Position,
    token: &CancellationToken,
    other_models: &[Rc<TextModel>],
) -> Promise<Option<HighlightMap>> {
    let ordered_by_score = registry.ordered(model, false);

    // in order of score ask the occurrences provider
    // until someone response with a good result
    // (good = non undefined and non null ResourceMap)
    // (result of size == 0 is valid, no highlights is a valid/expected result -- not a signal to fall back to other providers)
    let factories: Vec<PromiseFactory<Option<HighlightMap>>> = ordered_by_score
        .into_iter()
        .map(|provider| {
            let (model, token, other_models) = (Rc::clone(model), token.clone(), other_models.to_vec());
            let factory: PromiseFactory<Option<HighlightMap>> = Box::new(move || {
                let filtered_models: Vec<Rc<TextModel>> = other_models
                    .iter()
                    .filter(|other_model| should_synchronize_model(other_model))
                    .filter(|other_model| {
                        score(Some(provider.selector()), &other_model.uri(), &other_model.get_language_id(), true, None, None) > 0
                    })
                    .cloned()
                    .collect();
                provider
                    .provide_multi_document_highlights(&model, position, &filtered_models, &token)
                    .then_result(|result| match result {
                        Ok(highlights) => Ok(highlights),
                        Err(error) => {
                            on_unexpected_external_error(&error);
                            Ok(None)
                        }
                    })
            });
            factory
        })
        .collect();
    first(factories, Option::is_some, None).then(Option::flatten)
}

/// O `_compute` de um pedido: o que as subclasses de `OccurenceAtPositionRequest` implementam.
type ComputeOccurences = Rc<dyn Fn(&Rc<TextModel>, &Selection, &str, CancellationToken) -> Promise<HighlightMap>>;

/// `OccurenceAtPositionRequest` (com as subclasses `SemanticOccurenceAtPositionRequest` e `MultiModelOccurenceRequest`).
pub struct OccurenceAtPositionRequest {
    model: Rc<TextModel>,
    selection: Selection,
    word_separators: String,
    word_range: Option<Range>,
    result: RefCell<Option<CancelablePromise<HighlightMap>>>,
    compute: ComputeOccurences,
}

impl OccurenceAtPositionRequest {
    fn new(model: &Rc<TextModel>, selection: Selection, word_separators: &str, compute: ComputeOccurences) -> Rc<Self> {
        Rc::new(Self {
            model: Rc::clone(model),
            selection,
            word_separators: word_separators.to_owned(),
            word_range: Self::get_current_word_range(model, &selection),
            result: RefCell::new(None),
            compute,
        })
    }

    /// `get result()`: a promessa dos destaques, criada na primeira leitura.
    pub fn result(&self) -> CancelablePromise<HighlightMap> {
        let mut result = self.result.borrow_mut();
        result
            .get_or_insert_with(|| {
                let (model, selection, word_separators, compute) =
                    (Rc::clone(&self.model), self.selection, self.word_separators.clone(), Rc::clone(&self.compute));
                create_cancelable_promise(move |token| compute(&model, &selection, &word_separators, token))
            })
            .clone()
    }

    /// `_getCurrentWordRange(model, selection)`.
    fn get_current_word_range(model: &TextModel, selection: &Selection) -> Option<Range> {
        let word = model.get_word_at_position(selection.get_position())?;
        Some(Range::new(selection.start_line_number, word.start_column, selection.start_line_number, word.end_column))
    }

    /// `isValid(model, selection, decorations)`.
    pub fn is_valid(&self, model: &TextModel, selection: &Selection, decorations: &EditorDecorationsCollection) -> bool {
        let line_number = selection.start_line_number;
        let start_column = selection.start_column;
        let end_column = selection.end_column;
        let current_word_range = Self::get_current_word_range(model, selection);

        let mut request_is_valid = self.word_range.is_some() && self.word_range == current_word_range;

        // Even if we are on a different word, if that word is in the decorations ranges, the request is still valid
        // (Same symbol)
        let len = decorations.length();
        let mut i = 0;
        while !request_is_valid && i < len {
            if let Some(range) = decorations.get_range(i)
                && range.start_line_number == line_number
                && range.start_column <= start_column
                && range.end_column >= end_column
            {
                request_is_valid = true;
            }
            i += 1;
        }

        request_is_valid
    }

    /// `cancel()`.
    pub fn cancel(&self) {
        self.result().cancel();
    }
}

/// `computeOccurencesAtPosition(registry, model, selection, wordSeparators, mergeProviders)`: o pedido com os provedores
/// semânticos (`SemanticOccurenceAtPositionRequest`).
pub fn compute_occurences_at_position(
    registry: &LanguageFeatureRegistry<dyn DocumentHighlightProvider>,
    model: &Rc<TextModel>,
    selection: Selection,
    word_separators: &str,
    merge_providers: bool,
) -> Rc<OccurenceAtPositionRequest> {
    let registry = registry.clone();
    OccurenceAtPositionRequest::new(
        model,
        selection,
        word_separators,
        Rc::new(
            move |model: &Rc<TextModel>, selection: &Selection, _word_separators: &str, token: CancellationToken| {
                get_occurrences_at_position(&registry, model, selection.get_position(), &token, merge_providers)
            },
        ),
    )
}

/// `computeOccurencesMultiModel(registry, model, selection, wordSeparators, otherModels)`: o pedido sobre vários modelos
/// (`MultiModelOccurenceRequest`).
pub fn compute_occurences_multi_model(
    registry: &LanguageFeatureRegistry<dyn MultiDocumentHighlightProvider>,
    model: &Rc<TextModel>,
    selection: Selection,
    word_separators: &str,
    other_models: Vec<Rc<TextModel>>,
) -> Rc<OccurenceAtPositionRequest> {
    let registry = registry.clone();
    OccurenceAtPositionRequest::new(
        model,
        selection,
        word_separators,
        Rc::new(
            move |model: &Rc<TextModel>, selection: &Selection, _word_separators: &str, token: CancellationToken| {
                get_occurrences_across_multiple_models(&registry, model, selection.get_position(), &token, &other_models)
                    .then(|value| value.unwrap_or_else(|| Rc::new(ResourceMap::new())))
            },
        ),
    )
}
