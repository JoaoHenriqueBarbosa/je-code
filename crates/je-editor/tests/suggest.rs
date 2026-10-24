//! Porte de `src/vs/editor/contrib/suggest/test/browser/suggest.test.ts` (suíte `Suggest`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` são [`Fixture`]: o registro novo, o modelo com a URI `foo:bar/path` e o provedor de teste
//!   registrado; o `teardown` descarta o registro do provedor e o modelo. `ensureNoDisposablesAreLeakedInTestSuite()` é
//!   o `LeakGuard`.
//! - O texto `'FOO\nbar\BAR\nfoo'` do original tem um `\B` que o JavaScript lê como `B`: a segunda linha é `barBAR`.
//! - Os casos `async` são o corpo `async` de [`run_with_faked_timers`], que o executa no event loop do porte.
//! - `new CompletionOptions(order, kindFilter, providerFilter)` é o literal de [`CompletionOptions`] com o resto do
//!   `Default` (`undefined` é o valor padrão do argumento).
//! - Os provedores são estruturas que implementam [`CompletionItemProvider`]. O item `jjj` do caso `only from` não tem
//!   `range` nem `kind` no original (o provedor é um `any`, e `provideSuggestionItems` preenche o `range` com a palavra
//!   sob a posição); o `range` do `languages::CompletionItem` do porte é obrigatório, então o item leva o `range` que
//!   esse preenchimento daria (`insert` até a posição e `replace` pela palavra `FOO`), e o `kind` é `Property`
//!   (o `type: 'property'` do original).
//! - `assert.ok(items[0].provider === foo)` é a comparação da identidade do provedor ([`provider_id`]).
//! - `new Range(0, 0, 0, 0)` e `new Position(0, 0)` são valores fora do modelo (linha 0): o `Position` e o `Range` do
//!   porte os guardam como vêm, e o `provideSuggestionItems` não os valida.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::ProviderResult;
use je_editor::language_selector::{LanguageFilter, LanguageSelector};
use je_editor::languages::{
    CompletionContext, CompletionItem as LanguageCompletionItem, CompletionItemKind, CompletionItemProvider,
    CompletionItemRange, CompletionLabel, CompletionList,
};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::suggest::suggest::{
    CompletionItem, CompletionItemModel, CompletionOptions, ProviderSet, SnippetSortOrder, label_text,
    provide_suggestion_items, provider_id,
};
use je_editor::text_model::TextModel;
use support::test_text_model::create_text_model_with;

type Registry = Rc<LanguageFeatureRegistry<dyn CompletionItemProvider>>;

/// `{ pattern: 'bar/path', scheme: 'foo' }`.
fn bar_path_selector() -> LanguageSelector {
    LanguageSelector::Filter(LanguageFilter {
        pattern: Some("bar/path".into()),
        scheme: Some("foo".to_string()),
        ..LanguageFilter::default()
    })
}

/// Uma sugestão `{ label, kind, insertText: label, range }` do original.
fn suggestion(label: &str, kind: CompletionItemKind, range: CompletionItemRange) -> LanguageCompletionItem {
    LanguageCompletionItem {
        label: CompletionLabel::Text(label.to_string()),
        kind,
        range,
        tags: None,
        detail: None,
        documentation: None,
        sort_text: None,
        filter_text: None,
        preselect: None,
        insert_text: label.to_string(),
        insert_text_rules: None,
        commit_characters: None,
    }
}

/// A lista que os provedores de teste devolvem: `{ incomplete: false, suggestions }`.
fn list(suggestions: Vec<LanguageCompletionItem>) -> ProviderResult<Rc<CompletionList>> {
    Promise::resolved(Some(Rc::new(CompletionList { suggestions, incomplete: Some(false), dispose: None, duration: None })))
}

/// O provedor do `setup`: `aaa` e `zzz` (snippets) e `fff` (propriedade), todos em `Range.fromPositions(pos)`.
struct SetupProvider;

impl CompletionItemProvider for SetupProvider {
    fn debug_display_name(&self) -> &str {
        "test"
    }

    fn provide_completion_items(
        &self,
        _model: &Rc<TextModel>,
        position: Position,
        _context: &CompletionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<CompletionList>> {
        let range = CompletionItemRange::Single(Range::from_positions(position, position));
        list(vec![
            suggestion("aaa", CompletionItemKind::Snippet, range),
            suggestion("zzz", CompletionItemKind::Snippet, range),
            suggestion("fff", CompletionItemKind::Property, range),
        ])
    }
}

/// O que `setup()` deixa em variáveis da suíte (`_ds` é o `LeakGuard`, conferido ao fim do caso).
struct Fixture {
    _ds: LeakGuard,
    model: Rc<TextModel>,
    registration: DisposableRef,
    registry: Registry,
}

impl Fixture {
    fn new() -> Self {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let registry: Registry = Rc::new(LanguageFeatureRegistry::new(None));
        let model = create_text_model_with(
            "FOO\nbarBAR\nfoo",
            None,
            RelaxedTextModelCreationOptions::default(),
            Some(URI::parse("foo:bar/path", false)),
        );
        let registration = registry.register(bar_path_selector(), Rc::new(SetupProvider));
        Self { _ds: ds, model, registration, registry }
    }

    /// `teardown()`.
    fn teardown(&self) {
        self.registration.dispose();
        self.model.dispose();
    }

    /// `provideSuggestionItems(registry, model, position, options)` com o contexto e o token padrão.
    fn provide(&self, position: Position, options: CompletionOptions) -> Promise<CompletionItemModel> {
        provide_suggestion_items(&self.registry, &self.model, position, options, CompletionContext::default(), &CancellationToken::None)
    }
}

/// `item.completion.label` de cada item.
fn labels(items: &[CompletionItem]) -> Vec<String> {
    items.iter().map(|item| label_text(&item.completion.borrow().label).to_string()).collect()
}

// suite('Suggest')

#[test]
fn sort_snippet_inline() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let result = fixture
            .provide(Position::new(1, 1), CompletionOptions { snippet_sort_order: SnippetSortOrder::Inline, ..CompletionOptions::default() })
            .await
            .expect("provideSuggestionItems");
        assert_eq!(result.items.len(), 3);
        assert_eq!(labels(&result.items), ["aaa", "fff", "zzz"]);
        result.disposable.dispose();
        fixture.teardown();
    });
}

#[test]
fn sort_snippet_top() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let result = fixture
            .provide(Position::new(1, 1), CompletionOptions { snippet_sort_order: SnippetSortOrder::Top, ..CompletionOptions::default() })
            .await
            .expect("provideSuggestionItems");
        assert_eq!(result.items.len(), 3);
        assert_eq!(labels(&result.items), ["aaa", "zzz", "fff"]);
        result.disposable.dispose();
        fixture.teardown();
    });
}

#[test]
fn sort_snippet_bottom() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let result = fixture
            .provide(Position::new(1, 1), CompletionOptions { snippet_sort_order: SnippetSortOrder::Bottom, ..CompletionOptions::default() })
            .await
            .expect("provideSuggestionItems");
        assert_eq!(result.items.len(), 3);
        assert_eq!(labels(&result.items), ["fff", "aaa", "zzz"]);
        result.disposable.dispose();
        fixture.teardown();
    });
}

#[test]
fn sort_snippet_none() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let result = fixture
            .provide(
                Position::new(1, 1),
                CompletionOptions { kind_filter: [CompletionItemKind::Snippet].into_iter().collect(), ..CompletionOptions::default() },
            )
            .await
            .expect("provideSuggestionItems");
        assert_eq!(result.items.len(), 1);
        assert_eq!(labels(&result.items), ["fff"]);
        result.disposable.dispose();
        fixture.teardown();
    });
}

/// O provedor `foo` do caso `only from`: `{ triggerCharacters: [], provideCompletionItems() {...} }`.
struct OnlyFromProvider {
    trigger_characters: Vec<String>,
}

impl CompletionItemProvider for OnlyFromProvider {
    fn debug_display_name(&self) -> &str {
        "foo"
    }

    fn trigger_characters(&self) -> Option<&[String]> {
        Some(&self.trigger_characters)
    }

    fn provide_completion_items(
        &self,
        _model: &Rc<TextModel>,
        _position: Position,
        _context: &CompletionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<CompletionList>> {
        // o `range` que `provideSuggestionItems` preencheria: `insert` até a posição e `replace` pela palavra
        let range = CompletionItemRange::InsertReplace { insert: Range::new(1, 1, 1, 1), replace: Range::new(1, 1, 1, 4) };
        list(vec![suggestion("jjj", CompletionItemKind::Property, range)])
    }
}

#[test]
fn only_from() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();

        let foo: Rc<dyn CompletionItemProvider> = Rc::new(OnlyFromProvider { trigger_characters: Vec::new() });
        let registration = fixture.registry.register(bar_path_selector(), Rc::clone(&foo));

        let mut provider_filter = ProviderSet::new();
        provider_filter.add(Rc::clone(&foo));
        let result = fixture
            .provide(Position::new(1, 1), CompletionOptions { provider_filter, ..CompletionOptions::default() })
            .await
            .expect("provideSuggestionItems");
        registration.dispose();

        assert_eq!(result.items.len(), 1);
        assert!(provider_id(&result.items[0].provider) == provider_id(&foo));
        result.disposable.dispose();
        fixture.teardown();
    });
}

/// O provedor `foo` do caso `Ctrl+space completions stopped working with the latest Insiders, #97650`.
struct InsertReplaceProvider;

impl CompletionItemProvider for InsertReplaceProvider {
    fn debug_display_name(&self) -> &str {
        "test"
    }

    fn trigger_characters(&self) -> Option<&[String]> {
        Some(&[])
    }

    fn provide_completion_items(
        &self,
        _model: &Rc<TextModel>,
        _position: Position,
        _context: &CompletionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<CompletionList>> {
        list(vec![
            suggestion(
                "one",
                CompletionItemKind::Class,
                CompletionItemRange::InsertReplace { insert: Range::new(0, 0, 0, 0), replace: Range::new(0, 0, 0, 10) },
            ),
            suggestion(
                "two",
                CompletionItemKind::Class,
                CompletionItemRange::InsertReplace { insert: Range::new(0, 0, 0, 0), replace: Range::new(0, 1, 0, 10) },
            ),
        ])
    }
}

#[test]
fn ctrl_space_completions_stopped_working_with_the_latest_insiders_97650() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();

        let foo: Rc<dyn CompletionItemProvider> = Rc::new(InsertReplaceProvider);
        let registration = fixture.registry.register(bar_path_selector(), Rc::clone(&foo));
        let mut provider_filter = ProviderSet::new();
        provider_filter.add(Rc::clone(&foo));
        let result = fixture
            .provide(Position::new(0, 0), CompletionOptions { provider_filter, ..CompletionOptions::default() })
            .await
            .expect("provideSuggestionItems");
        registration.dispose();

        assert_eq!(result.items.len(), 2);
        let [a, b] = &result.items[..] else { panic!("esperados 2 itens") };

        assert_eq!(labels(std::slice::from_ref(a)), ["one"]);
        assert!(!a.is_invalid);
        assert_eq!(labels(std::slice::from_ref(b)), ["two"]);
        assert!(b.is_invalid);
        result.disposable.dispose();
        fixture.teardown();
    });
}
