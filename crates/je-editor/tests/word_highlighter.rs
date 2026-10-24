//! Porte de `src/vs/editor/contrib/wordHighlighter/test/browser/wordHighlighter.test.ts`, suíte `DocumentHighlightProviders`
//! (a consulta aos provedores de destaque), caso a caso e na mesma ordem.
//!
//! A suíte `WordHighlighterContribution` (três casos: `cancels in-flight merged results when the setting is disabled`,
//! `does not start highlighting when occurrences are disabled` e `discards merged results from a replaced model`) não está
//! portada: exercita o `WordHighlighter` e o `WordHighlighterContribution`, que dependem de serviços do editor que o porte
//! ainda não tem (ver "Divergências" em `je_editor::word_highlighter`).
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` com o `setup` (um registro novo e o modelo `word word word\nword`) é
//!   [`run_test`], que também roda o corpo com o relógio virtual: os casos esperam promessas. O `store` é o `LeakGuard`.
//! - `store.add(registry.register(selector, provider))` é `suite.guard.add(registry.register(...))`.
//! - O provedor literal `{ provideDocumentHighlights: () => ... }` é [`TestProvider`] (um fecho que devolve a promessa de
//!   destaques); `undefined` e `null` do retorno são `None`, e um valor lançado de forma síncrona é uma promessa
//!   rejeitada (o resultado é o mesmo que o do original).
//! - `new DeferredPromise<DocumentHighlight[]>()` e `.p` são o `DeferredPromise` (e a sua `promise`), que carrega o
//!   `Option<Vec<...>>` do retorno do provedor.
//! - O `for (const { name, value } of [...])` que gera dois casos por valor é a dupla de funções [`merges_other_providers`] e
//!   [`does_not_use_textual_fallback`] chamadas pelos seis `#[test]`; `undefined` e `null` são `None` e o array vazio é
//!   `Some(vec![])`.
//! - `result?.get(model.uri)` é `result.get(&model.uri())` (o `Option<&Vec<_>>` do `ResourceMap`), comparado com o `Some`
//!   esperado; os objetos de `deepStrictEqual` viram tuplas.
//! - `errorHandler.getUnexpectedErrorHandler()`/`setUnexpectedErrorHandler(...)` é a troca do tratador de erros inesperados
//!   do `LeakGuard` pelo coletor do caso, devolvido no descarte (`record_unexpected_errors`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, set_unexpected_error_handler};
use je_base::lifecycle::to_disposable;
use je_base::promise::{DeferredPromise, Promise};
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::{DocumentHighlight, DocumentHighlightKind, DocumentHighlightProvider, ProviderResult};
use je_editor::language_selector::{LanguageFilter, LanguageSelector};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::textual_highlight_provider::TextualDocumentHighlightProvider;
use je_editor::word_highlighter::get_occurrences_at_position;
use support::test_text_model::create_text_model;

/// Os `let` da suíte `DocumentHighlightProviders`.
struct Suite {
    guard: LeakGuard,
    registry: LanguageFeatureRegistry<dyn DocumentHighlightProvider>,
    model: Rc<TextModel>,
}

/// O que o mocha faz em volta de cada `test(...)` da suíte: o `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o
/// corpo (com o relógio virtual) e o `teardown`, nessa ordem.
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<Suite>) -> F) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    // setup
    let registry = LanguageFeatureRegistry::new(None);
    let model = create_text_model("word word word\nword");
    let owned = Rc::clone(&model);
    guard.add(to_disposable(move || owned.dispose()));
    let suite = Rc::new(Suite { guard, registry, model });

    run_with_faked_timers(body(Rc::clone(&suite)));
}

fn position() -> Position {
    Position::new(1, 2)
}

fn first() -> DocumentHighlight {
    DocumentHighlight { range: Range::new(1, 1, 1, 5), kind: Some(DocumentHighlightKind::Read) }
}

fn second() -> DocumentHighlight {
    DocumentHighlight { range: Range::new(1, 6, 1, 10), kind: Some(DocumentHighlightKind::Write) }
}

/// O provedor literal `{ provideDocumentHighlights: () => ... }`.
struct TestProvider {
    provide: Box<dyn Fn() -> ProviderResult<Vec<DocumentHighlight>>>,
}

impl DocumentHighlightProvider for TestProvider {
    fn provide_document_highlights(
        &self,
        _model: &Rc<TextModel>,
        _position: Position,
        _token: &CancellationToken,
    ) -> ProviderResult<Vec<DocumentHighlight>> {
        (self.provide)()
    }
}

fn provider(provide: impl Fn() -> ProviderResult<Vec<DocumentHighlight>> + 'static) -> Rc<TestProvider> {
    Rc::new(TestProvider { provide: Box::new(provide) })
}

/// Um provedor que sempre responde `highlights` (`None` é `undefined` ou `null`).
fn answering(highlights: Option<Vec<DocumentHighlight>>) -> Rc<TestProvider> {
    provider(move || Promise::resolved(highlights.clone()))
}

/// Um provedor que registra `name` em `calls` e responde `highlights`.
fn recording(calls: &Rc<RefCell<Vec<&'static str>>>, name: &'static str, highlights: Vec<DocumentHighlight>) -> Rc<TestProvider> {
    let calls = Rc::clone(calls);
    provider(move || {
        calls.borrow_mut().push(name);
        Promise::resolved(Some(highlights.clone()))
    })
}

fn plaintext() -> LanguageSelector {
    LanguageSelector::from("plaintext")
}

fn wildcard() -> LanguageSelector {
    LanguageSelector::from("*")
}

fn highlights_of(suite: &Suite, result: &je_editor::language_providers::HighlightMap) -> Option<Vec<DocumentHighlight>> {
    result.get(&suite.model.uri()).cloned()
}

async fn occurrences(suite: &Suite, merge_providers: bool) -> je_editor::language_providers::HighlightMap {
    get_occurrences_at_position(&suite.registry, &suite.model, position(), &CancellationToken::None, merge_providers)
        .await
        .expect("a consulta não rejeita")
}

#[test]
fn keeps_the_first_answer_when_merging_is_disabled() {
    run_test(|suite| async move {
        let calls = Rc::new(RefCell::new(Vec::new()));
        suite.guard.add(suite.registry.register(wildcard(), recording(&calls, "lower", vec![second()])));
        suite.guard.add(suite.registry.register(plaintext(), recording(&calls, "higher", vec![first()])));

        let result = occurrences(&suite, false).await;
        assert_eq!((calls.borrow().clone(), highlights_of(&suite, &result)), (vec!["higher"], Some(vec![first()])));
    });
}

#[test]
fn an_empty_answer_still_stops_the_default_provider_search() {
    run_test(|suite| async move {
        let calls = Rc::new(RefCell::new(Vec::new()));
        suite.guard.add(suite.registry.register(wildcard(), recording(&calls, "lower", vec![second()])));
        suite.guard.add(suite.registry.register(plaintext(), recording(&calls, "higher", Vec::new())));

        let result = occurrences(&suite, false).await;
        assert_eq!((calls.borrow().clone(), highlights_of(&suite, &result)), (vec!["higher"], Some(Vec::new())));
    });
}

#[test]
fn queries_different_priorities_concurrently_and_deduplicates_ranges_in_provider_order() {
    run_test(|suite| async move {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let higher = Rc::new(DeferredPromise::<Option<Vec<DocumentHighlight>>>::new());
        let lower = Rc::new(DeferredPromise::<Option<Vec<DocumentHighlight>>>::new());
        let text = DocumentHighlight { range: Range::new(1, 11, 1, 15), kind: Some(DocumentHighlightKind::Text) };
        let unspecified = DocumentHighlight { range: Range::new(2, 1, 2, 5), kind: None };
        let overlapping = DocumentHighlight { range: Range::new(1, 2, 1, 5), kind: Some(DocumentHighlightKind::Write) };
        let higher_highlights = vec![first(), first(), text, unspecified];

        let lower_calls = Rc::clone(&calls);
        let lower_promise = lower.promise.clone();
        suite.guard.add(suite.registry.register(
            wildcard(),
            provider(move || {
                lower_calls.borrow_mut().push("lower");
                lower_promise.clone()
            }),
        ));
        let higher_calls = Rc::clone(&calls);
        let higher_promise = higher.promise.clone();
        suite.guard.add(suite.registry.register(
            plaintext(),
            provider(move || {
                higher_calls.borrow_mut().push("higher");
                higher_promise.clone()
            }),
        ));

        let pending = get_occurrences_at_position(&suite.registry, &suite.model, position(), &CancellationToken::None, true);
        let calls_before_completion = calls.borrow().clone();
        lower
            .complete(Some(vec![
                DocumentHighlight { range: first().range, kind: Some(DocumentHighlightKind::Write) },
                second(),
                DocumentHighlight { range: text.range, kind: Some(DocumentHighlightKind::Read) },
                DocumentHighlight { range: unspecified.range, kind: Some(DocumentHighlightKind::Write) },
                overlapping,
            ]))
            .await
            .expect("completa");
        higher.complete(Some(higher_highlights)).await.expect("completa");
        let result = pending.await.expect("a consulta não rejeita");

        assert_eq!(
            (calls_before_completion, highlights_of(&suite, &result)),
            (vec!["higher", "lower"], Some(vec![first(), text, unspecified, second(), overlapping]))
        );
    });
}

/// `merges other providers when a higher-priority provider returns ${name}`.
fn merges_other_providers(value: Option<Vec<DocumentHighlight>>) {
    run_test(|suite| async move {
        suite.guard.add(suite.registry.register(wildcard(), answering(Some(vec![second()]))));
        suite.guard.add(suite.registry.register(plaintext(), answering(value)));

        let result = occurrences(&suite, true).await;
        assert_eq!(highlights_of(&suite, &result), Some(vec![second()]));
    });
}

/// `does not use textual fallback when a matching provider returns ${name}`.
fn does_not_use_textual_fallback(value: Option<Vec<DocumentHighlight>>) {
    run_test(|suite| async move {
        suite.guard.add(suite.registry.register(wildcard(), Rc::new(TextualDocumentHighlightProvider::new())));
        suite.guard.add(suite.registry.register(plaintext(), answering(value.clone())));

        let result = occurrences(&suite, true).await;
        // `value === null ? undefined : value`
        assert_eq!(highlights_of(&suite, &result), value);
    });
}

#[test]
fn merges_other_providers_when_a_higher_priority_provider_returns_undefined() {
    merges_other_providers(None);
}

#[test]
fn does_not_use_textual_fallback_when_a_matching_provider_returns_undefined() {
    does_not_use_textual_fallback(None);
}

#[test]
fn merges_other_providers_when_a_higher_priority_provider_returns_null() {
    merges_other_providers(None);
}

#[test]
fn does_not_use_textual_fallback_when_a_matching_provider_returns_null() {
    does_not_use_textual_fallback(None);
}

#[test]
fn merges_other_providers_when_a_higher_priority_provider_returns_an_empty_array() {
    merges_other_providers(Some(Vec::new()));
}

#[test]
fn does_not_use_textual_fallback_when_a_matching_provider_returns_an_empty_array() {
    does_not_use_textual_fallback(Some(Vec::new()));
}

#[test]
fn reports_provider_failures_without_discarding_successful_results() {
    run_test(|suite| async move {
        // `store.add(toDisposable(() => setUnexpectedErrorHandler(originalErrorHandler)))`
        suite.guard.add(to_disposable(common::record_unexpected_errors));
        let errors: Rc<RefCell<Vec<Error>>> = Rc::default();
        let collected = Rc::clone(&errors);
        set_unexpected_error_handler(move |error| collected.borrow_mut().push(error.clone()));
        let synchronous_error = Error::new("synchronous failure");
        let asynchronous_error = Error::new("asynchronous failure");
        suite.guard.add(suite.registry.register(plaintext(), answering(Some(vec![first()]))));
        suite.guard.add(suite.registry.register(plaintext(), provider(|| Promise::rejected(Error::cancellation()))));
        let rejection = asynchronous_error.clone();
        suite.guard.add(suite.registry.register(plaintext(), provider(move || Promise::rejected(rejection.clone()))));
        let failure = synchronous_error.clone();
        suite.guard.add(suite.registry.register(plaintext(), provider(move || Promise::rejected(failure.clone()))));

        let result = occurrences(&suite, true).await;
        assert_eq!(
            (errors.borrow().clone(), highlights_of(&suite, &result)),
            (vec![synchronous_error, asynchronous_error], Some(vec![first()]))
        );
    });
}

#[test]
fn uses_textual_fallback_only_when_no_other_provider_matches() {
    run_test(|suite| async move {
        let non_matching_calls = Rc::new(Cell::new(0));
        let counted = Rc::clone(&non_matching_calls);
        suite.guard.add(suite.registry.register(
            LanguageSelector::from("javascript"),
            provider(move || {
                counted.set(counted.get() + 1);
                Promise::resolved(Some(vec![first()]))
            }),
        ));
        suite.guard.add(suite.registry.register(wildcard(), Rc::new(TextualDocumentHighlightProvider::new())));

        let result = occurrences(&suite, true).await;
        let text = |range| DocumentHighlight { range, kind: Some(DocumentHighlightKind::Text) };
        assert_eq!(
            (non_matching_calls.get(), highlights_of(&suite, &result)),
            (
                0,
                Some(vec![
                    text(Range::new(1, 1, 1, 5)),
                    text(Range::new(1, 6, 1, 10)),
                    text(Range::new(1, 11, 1, 15)),
                    text(Range::new(2, 1, 2, 5)),
                ])
            )
        );
    });
}

#[test]
fn keeps_wildcard_extension_providers_but_excludes_the_built_in_textual_provider() {
    run_test(|suite| async move {
        suite.guard.add(suite.registry.register(wildcard(), answering(Some(vec![first()]))));
        suite.guard.add(suite.registry.register(wildcard(), Rc::new(TextualDocumentHighlightProvider::new())));

        let result = occurrences(&suite, true).await;
        assert_eq!(highlights_of(&suite, &result), Some(vec![first()]));
    });
}

#[test]
fn preserves_textual_fallback_after_a_declined_answer_when_merging_is_disabled() {
    run_test(|suite| async move {
        suite.guard.add(suite.registry.register(wildcard(), Rc::new(TextualDocumentHighlightProvider::new())));
        suite.guard.add(suite.registry.register(plaintext(), answering(None)));

        let result = occurrences(&suite, false).await;
        let ranges: Option<Vec<Range>> =
            highlights_of(&suite, &result).map(|highlights| highlights.iter().map(|highlight| highlight.range).collect());
        assert_eq!(
            ranges,
            Some(vec![Range::new(1, 1, 1, 5), Range::new(1, 6, 1, 10), Range::new(1, 11, 1, 15), Range::new(2, 1, 2, 5)])
        );
    });
}

#[test]
fn preserves_exclusive_document_selectors() {
    run_test(|suite| async move {
        let calls = Rc::new(RefCell::new(Vec::new()));
        suite.guard.add(suite.registry.register(wildcard(), recording(&calls, "wildcard", vec![second()])));
        suite.guard.add(suite.registry.register(
            LanguageSelector::Filter(LanguageFilter {
                language: Some("plaintext".to_owned()),
                exclusive: true,
                ..LanguageFilter::default()
            }),
            recording(&calls, "exclusive", vec![first()]),
        ));

        let result = occurrences(&suite, true).await;
        assert_eq!((calls.borrow().clone(), highlights_of(&suite, &result)), (vec!["exclusive"], Some(vec![first()])));
    });
}

#[test]
fn returns_an_empty_map_without_providers() {
    run_test(|suite| async move {
        let result = occurrences(&suite, true).await;
        assert_eq!(result.size(), 0);
    });
}
