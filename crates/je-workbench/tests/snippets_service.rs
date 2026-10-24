//! Porte de `src/vs/workbench/contrib/snippets/test/browser/snippetsService.test.ts` (suíte `SnippetsService`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup` e o `teardown` da suíte são o [`run_test`]: ele cria o
//!   `LeakGuard`, o [`Fixture`] (o `ILanguageService` com `fooLang` registrada, os dois snippets padrão do `setup` e o
//!   `DisposableStore` `disposables`) e roda o corpo com o relógio virtual (`runWithFakedTimers`), porque o
//!   `provideCompletionItems` é uma promessa. Os casos síncronos (`getSnippetsSync`) usam o mesmo caminho. O
//!   `disposables.dispose()` do `teardown` roda no fim do corpo, antes de o guarda conferir os vazamentos.
//! - `createModelServices(disposables)` é o `create_language_model_services` de `je-editor/tests/support`, e
//!   `instantiateTextModel(instantiationService, text, 'fooLang')` é o `instantiate_text_model_with_languages`. O
//!   `TestLanguageConfigurationService` do provedor é um serviço à parte do que o modelo usa, como no original (o
//!   registro de `wordPattern` e `brackets` vale só para o provedor).
//! - `new Snippet(false, ['fooLang'], name, prefix, '', body, '', source, generateUuid(), include, exclude)` é
//!   [`new_snippet`] (e [`new_pattern_snippet`] para os casos de `include` e `exclude`, que passam `'test'` como origem e
//!   `'snippet'` como corpo); o `generateUuid()` é o de `je_base::uuid`.
//! - `SimpleSnippetService` é a struct do mesmo nome; os métodos que o original lança (`throw new Error()`) lançam o
//!   mesmo erro (`errors::throw`). `getSnippetsSync(languageId!, resourceUri)` ignora o idioma, como o original.
//! - `asCompletionModel` é [`as_completion_model`]: o `new CompletionItem(position, s, list, provider)` é o
//!   `CompletionItem::new` de `suggest.rs`, `WordDistance.None` é `<dyn WordDistance>::none()`,
//!   `EditorOptions.suggest.defaultValue` é o `default_value` do `EditorSuggest` e
//!   `EditorOptions.snippetSuggestions.defaultValue` é `inline`.
//! - `(x.range as CompletionItemRanges).insert` é o `CompletionItemRange::InsertReplace` (par `insert` e `replace`);
//!   `deepStrictEqual` de rótulos compara o `CompletionLabel`. `completions.items[i].textLabel`, `editStart`,
//!   `editInsertEnd` e `editReplaceEnd` são os campos `text_label`, `edit_start`, `edit_insert_end` e `edit_replace_end`.
//! - `(<SnippetCompletion>x).label.label` é o `label_text` do rótulo do item: o provedor devolve o item de sugestão do
//!   porte, e não o `SnippetCompletion`.
//! - `model.dispose()` no meio de um caso é o `dispose()` do modelo; o que o original registra em `disposables`
//!   (`disposables.add(instantiateTextModel(...))`) é registrado em [`Fixture::add_model`].
//!
//! Divergências:
//! - `test.skip('Snippets disappear with . key #145960', ...)` fica como função comum, sem `#[test]` (o original o
//!   pula), com o corpo portado para o dia em que o caso for ativado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_language_configuration_service.rs"]
mod test_language_configuration_service;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::config::editor_options::EditorOptionImpl;
use je_editor::config::editor_options::suggest::EditorSuggest;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::EditOperation;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::languages::{
    CompletionContext, CompletionItem as LanguageCompletionItem, CompletionItemLabel, CompletionItemProvider, CompletionItemRange, CompletionLabel,
    CompletionList, CompletionTriggerKind,
};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::suggest::completion_model::{CompletionModel, LineContext};
use je_editor::suggest::suggest::{CompletionItem, label_text};
use je_editor::suggest::word_distance::WordDistance;
use je_editor::text_model::TextModel;
use je_platform::resources::uri_file;
use je_workbench::contrib::snippets::snippet_completion_provider::SnippetCompletionProvider;
use je_workbench::contrib::snippets::snippets::{ISnippetGetOptions, ISnippetsService};
use je_workbench::contrib::snippets::snippets_file::{Snippet, SnippetFile, SnippetSource};
use test_language_configuration_service::TestLanguageConfigurationService;
use test_text_model::{LanguageModelServices, create_language_model_services, instantiate_text_model_with_languages};

/// `new Snippet(false, ['fooLang'], name, prefix, '', body, '', source, generateUuid())`.
fn new_snippet(name: &str, prefix: &str, body: &str, source: SnippetSource) -> Rc<Snippet> {
    Rc::new(Snippet::new(false, vec!["fooLang".to_owned()], name, prefix, "", body, "", source, &generate_uuid(), None, None, None))
}

/// `new Snippet(false, ['fooLang'], name, prefix, '', 'snippet', 'test', SnippetSource.User, generateUuid(), include, exclude)`.
fn new_pattern_snippet(name: &str, prefix: &str, include: Option<&[&str]>, exclude: Option<&[&str]>) -> Rc<Snippet> {
    let patterns = |list: Option<&[&str]>| list.map(|list| list.iter().map(|pattern| (*pattern).to_owned()).collect::<Vec<String>>());
    Rc::new(Snippet::new(
        false,
        vec!["fooLang".to_owned()],
        name,
        prefix,
        "",
        "snippet",
        "test",
        SnippetSource::User,
        &generate_uuid(),
        patterns(include),
        patterns(exclude),
        None,
    ))
}

struct SimpleSnippetService {
    snippets: Vec<Rc<Snippet>>,
}

impl ISnippetsService for SimpleSnippetService {
    fn get_snippet_files(&self) -> Promise<Vec<Rc<SnippetFile>>> {
        errors::throw(Error::new(""))
    }

    fn is_enabled(&self, _snippet: &Snippet) -> bool {
        errors::throw(Error::new(""))
    }

    fn update_enablement(&self, _snippet: &Snippet, _enabled: bool) {
        errors::throw(Error::new(""))
    }

    fn update_usage_timestamp(&self, _snippet: &Snippet) {
        errors::throw(Error::new(""))
    }

    fn get_snippets(&self, language_id: Option<&str>, resource_uri: Option<&URI>, _opt: Option<&ISnippetGetOptions>) -> Promise<Vec<Rc<Snippet>>> {
        Promise::resolved(self.get_snippets_sync(language_id.unwrap_or_default(), resource_uri, None))
    }

    fn get_snippets_sync(&self, _language_id: &str, resource_uri: Option<&URI>, _opt: Option<&ISnippetGetOptions>) -> Vec<Rc<Snippet>> {
        // Filter snippets based on resourceUri if provided
        if let Some(resource_uri) = resource_uri {
            return self.snippets.iter().filter(|snippet| snippet.is_file_included(resource_uri)).cloned().collect();
        }
        self.snippets.clone()
    }
}

/// Os serviços e o estado que o `setup` da suíte cria.
struct Fixture {
    disposables: Rc<DisposableStore>,
    services: LanguageModelServices,
    snippet_service: RefCell<Rc<dyn ISnippetsService>>,
}

impl Fixture {
    /// `snippetService = new SimpleSnippetService([...])`.
    fn set_snippets(&self, snippets: Vec<Rc<Snippet>>) {
        *self.snippet_service.borrow_mut() = Rc::new(SimpleSnippetService { snippets }) as Rc<dyn ISnippetsService>;
    }

    fn snippet_service(&self) -> Rc<dyn ISnippetsService> {
        Rc::clone(&*self.snippet_service.borrow())
    }

    /// `new SnippetCompletionProvider(languageService, snippetService, languageConfigurationService)`.
    fn provider_with(&self, language_configuration_service: Rc<dyn ILanguageConfigurationService>) -> Rc<SnippetCompletionProvider> {
        let language_service: Rc<dyn ILanguageService> = self.services.language_service.clone();
        Rc::new(SnippetCompletionProvider::new(language_service, self.snippet_service(), language_configuration_service))
    }

    /// `new SnippetCompletionProvider(languageService, snippetService, disposables.add(new TestLanguageConfigurationService()))`.
    fn provider(&self) -> Rc<SnippetCompletionProvider> {
        let language_configuration_service = self.disposables.add_rc(TestLanguageConfigurationService::new());
        self.provider_with(language_configuration_service)
    }

    /// `instantiateTextModel(instantiationService, text, 'fooLang')`.
    fn create_model(&self, text: &str) -> Rc<TextModel> {
        instantiate_text_model_with_languages(&self.services, text, "fooLang")
    }

    /// `disposables.add(model)`.
    fn add_model(&self, model: Rc<TextModel>) -> Rc<TextModel> {
        let owned = Rc::clone(&model);
        self.disposables.add(to_disposable(move || owned.dispose()));
        model
    }

    /// O `teardown`: `disposables.dispose()`.
    fn teardown(&self) {
        self.disposables.dispose();
    }
}

/// O que o mocha faz em volta de cada `test(...)` (ver as notas no topo do arquivo).
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<Fixture>) -> F + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    // setup
    let services = create_language_model_services(&guard);
    let disposables = DisposableStore::new();
    disposables.add(services.language_service.register_language(LanguageExtensionPoint {
        id: "fooLang".to_owned(),
        extensions: Some(vec![".fooLang".to_owned()]),
        ..LanguageExtensionPoint::default()
    }));
    let snippet_service: Rc<dyn ISnippetsService> = Rc::new(SimpleSnippetService {
        snippets: vec![
            new_snippet("barTest", "bar", "barCodeSnippet", SnippetSource::User),
            new_snippet("bazzTest", "bazz", "bazzCodeSnippet", SnippetSource::User),
        ],
    });
    let fixture = Rc::new(Fixture { disposables, services, snippet_service: RefCell::new(snippet_service) });

    run_with_faked_timers(async move {
        body(Rc::clone(&fixture)).await;

        // teardown
        fixture.teardown();
    });
    drop(guard);
}

/// `defaultCompletionContext`.
fn default_completion_context() -> CompletionContext {
    CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None }
}

/// `provider.provideCompletionItems(model, position, context)`, já esperada.
async fn provide(provider: &Rc<SnippetCompletionProvider>, model: &Rc<TextModel>, position: Position, context: &CompletionContext) -> Rc<CompletionList> {
    provider
        .provide_completion_items(model, position, context, &CancellationToken::None)
        .await
        .expect("provideCompletionItems")
        .expect("a lista de sugestões")
}

async fn as_completion_model(
    model: &Rc<TextModel>,
    position: Position,
    provider: &Rc<SnippetCompletionProvider>,
    context: &CompletionContext,
) -> CompletionModel {
    let list = provide(provider, model, position, context).await;
    let provider_contract: Rc<dyn CompletionItemProvider> = provider.clone();

    let items: Vec<CompletionItem> =
        list.suggestions.iter().map(|suggestion| CompletionItem::new(position, suggestion.clone(), Rc::clone(&list), Rc::clone(&provider_contract))).collect();

    // `model.getLineContent(position.lineNumber).substring(0, position.column - 1)`
    let line = model.get_line_content(position.line_number);
    let end = usize::try_from(position.column - 1).unwrap_or(0).min(line.len());

    CompletionModel::new(
        items,
        position.column,
        LineContext { leading_line_content: line.slice(0, end).to_string_lossy(), character_count_delta: 0 },
        <dyn WordDistance>::none(),
        EditorSuggest::new().default_value(),
        "inline",
        None,
        None,
    )
}

/// `{ label, description }`.
fn label(label: &str, description: &str) -> CompletionLabel {
    CompletionLabel::Item(CompletionItemLabel { label: label.to_owned(), detail: None, description: Some(description.to_owned()) })
}

/// `x.range as CompletionItemRanges`: o par `insert` e `replace`.
fn ranges(range: &CompletionItemRange) -> (Range, Range) {
    match range {
        CompletionItemRange::InsertReplace { insert, replace } => (*insert, *replace),
        CompletionItemRange::Single(_) => panic!("o intervalo deveria ter insert e replace"),
    }
}

fn insert_range(item: &LanguageCompletionItem) -> Range {
    ranges(&item.range).0
}

fn replace_range(item: &LanguageCompletionItem) -> Range {
    ranges(&item.range).1
}

/// `completions.items[i].completion`.
fn completion_of(item: &CompletionItem) -> LanguageCompletionItem {
    item.completion.borrow().clone()
}

// suite('SnippetsService')

#[test]
fn snippet_completions_simple() {
    run_test(|f| async move {
        let provider = f.provider();
        let model = f.add_model(f.create_model(""));
        let context = default_completion_context();

        let result = provide(&provider, &model, Position::new(1, 1), &context).await;
        assert_eq!(result.incomplete, None);
        assert_eq!(result.suggestions.len(), 2);

        let completions = as_completion_model(&model, Position::new(1, 1), &provider, &context).await;
        assert_eq!(completions.items().len(), 2);
    });
}

#[test]
fn snippet_completions_simple_2() {
    run_test(|f| async move {
        let provider = f.provider();
        let model = f.add_model(f.create_model("hello "));
        let context = default_completion_context();

        let result = provide(&provider, &model, Position::new(1, 6) /* hello| */, &context).await;
        assert_eq!(result.incomplete, None);
        assert_eq!(result.suggestions.len(), 0);

        let result = provide(&provider, &model, Position::new(1, 7) /* hello |*/, &context).await;
        assert_eq!(result.incomplete, None);
        assert_eq!(result.suggestions.len(), 2);

        let completions1 = as_completion_model(&model, Position::new(1, 6) /* hello| */, &provider, &context).await;
        assert_eq!(completions1.items().len(), 0);

        let completions2 = as_completion_model(&model, Position::new(1, 7) /* hello |*/, &provider, &context).await;
        assert_eq!(completions2.items().len(), 2);
    });
}

#[test]
fn snippet_completions_with_prefix() {
    run_test(|f| async move {
        let provider = f.provider();
        let model = f.add_model(f.create_model("bar"));
        let context = default_completion_context();

        let result = provide(&provider, &model, Position::new(1, 4), &context).await;
        assert_eq!(result.incomplete, None);
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].label, label("bar", "barTest"));
        assert_eq!(insert_range(&result.suggestions[0]).start_column, 1);
        assert_eq!(result.suggestions[0].insert_text, "barCodeSnippet");

        let completions = as_completion_model(&model, Position::new(1, 4), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(completion_of(&items[0]).label, label("bar", "barTest"));
        assert_eq!(insert_range(&completion_of(&items[0])).start_column, 1);
        assert_eq!(completion_of(&items[0]).insert_text, "barCodeSnippet");
    });
}

#[test]
fn snippet_completions_with_different_prefixes() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("barTest", "bar", "s1", SnippetSource::User),
            new_snippet("name", "bar-bar", "s2", SnippetSource::User),
        ]);

        let provider = f.provider();
        let model = f.add_model(f.create_model("bar-bar"));
        let context = default_completion_context();

        {
            let result = provide(&provider, &model, Position::new(1, 3), &context).await;
            assert_eq!(result.incomplete, None);
            assert_eq!(result.suggestions.len(), 2);
            assert_eq!(result.suggestions[0].label, label("bar", "barTest"));
            assert_eq!(result.suggestions[0].insert_text, "s1");
            assert_eq!(insert_range(&result.suggestions[0]).start_column, 1);
            assert_eq!(result.suggestions[1].label, label("bar-bar", "name"));
            assert_eq!(result.suggestions[1].insert_text, "s2");
            assert_eq!(insert_range(&result.suggestions[1]).start_column, 1);

            let completions = as_completion_model(&model, Position::new(1, 3), &provider, &context).await;
            let items = completions.items();
            assert_eq!(items.len(), 2);
            assert_eq!(completion_of(&items[0]).label, label("bar", "barTest"));
            assert_eq!(completion_of(&items[0]).insert_text, "s1");
            assert_eq!(insert_range(&completion_of(&items[0])).start_column, 1);
            assert_eq!(completion_of(&items[1]).label, label("bar-bar", "name"));
            assert_eq!(completion_of(&items[1]).insert_text, "s2");
            assert_eq!(insert_range(&completion_of(&items[1])).start_column, 1);
        }

        {
            let result = provide(&provider, &model, Position::new(1, 5), &context).await;
            assert_eq!(result.incomplete, None);
            assert_eq!(result.suggestions.len(), 2);

            let (first, second) = (&result.suggestions[0], &result.suggestions[1]);

            assert_eq!(first.label, label("bar", "barTest"));
            assert_eq!(first.insert_text, "s1");
            assert_eq!(insert_range(first).start_column, 5);

            assert_eq!(second.label, label("bar-bar", "name"));
            assert_eq!(second.insert_text, "s2");
            assert_eq!(insert_range(second).start_column, 1);

            let completions = as_completion_model(&model, Position::new(1, 5), &provider, &context).await;
            let items = completions.items();
            assert_eq!(items.len(), 2);

            let (first, second) = (completion_of(&items[0]), completion_of(&items[1]));

            assert_eq!(first.label, label("bar-bar", "name"));
            assert_eq!(first.insert_text, "s2");
            assert_eq!(insert_range(&first).start_column, 1);

            assert_eq!(second.label, label("bar", "barTest"));
            assert_eq!(second.insert_text, "s1");
            assert_eq!(insert_range(&second).start_column, 5);
        }

        {
            let result = provide(&provider, &model, Position::new(1, 6), &context).await;
            assert_eq!(result.incomplete, None);
            assert_eq!(result.suggestions.len(), 2);
            assert_eq!(result.suggestions[0].label, label("bar", "barTest"));
            assert_eq!(result.suggestions[0].insert_text, "s1");
            assert_eq!(insert_range(&result.suggestions[0]).start_column, 5);
            assert_eq!(result.suggestions[1].label, label("bar-bar", "name"));
            assert_eq!(result.suggestions[1].insert_text, "s2");
            assert_eq!(insert_range(&result.suggestions[1]).start_column, 1);

            let completions = as_completion_model(&model, Position::new(1, 6), &provider, &context).await;
            let items = completions.items();
            assert_eq!(items.len(), 2);
            assert_eq!(completion_of(&items[0]).label, label("bar-bar", "name"));
            assert_eq!(completion_of(&items[0]).insert_text, "s2");
            assert_eq!(insert_range(&completion_of(&items[0])).start_column, 1);
            assert_eq!(completion_of(&items[1]).label, label("bar", "barTest"));
            assert_eq!(completion_of(&items[1]).insert_text, "s1");
            assert_eq!(insert_range(&completion_of(&items[1])).start_column, 5);
        }
    });
}

#[test]
fn cannot_use_php_as_user_snippet_prefix_anymore_26275() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("", "<?php", "insert me", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.create_model("\t<?php");
        let result = provide(&provider, &model, Position::new(1, 7), &context).await;
        assert_eq!(result.suggestions.len(), 1);
        let completions1 = as_completion_model(&model, Position::new(1, 7), &provider, &context).await;
        assert_eq!(completions1.items().len(), 1);

        model.dispose();
        let model = f.create_model("\t<?");
        let result = provide(&provider, &model, Position::new(1, 4), &context).await;
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(insert_range(&result.suggestions[0]).start_column, 2);
        let completions2 = as_completion_model(&model, Position::new(1, 4), &provider, &context).await;
        let items = completions2.items();
        assert_eq!(items.len(), 1);
        assert_eq!(insert_range(&completion_of(&items[0])).start_column, 2);

        model.dispose();
        let model = f.create_model("a<?");
        let result = provide(&provider, &model, Position::new(1, 4), &context).await;
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(insert_range(&result.suggestions[0]).start_column, 2);
        let completions3 = as_completion_model(&model, Position::new(1, 4), &provider, &context).await;
        let items = completions3.items();
        assert_eq!(items.len(), 1);
        assert_eq!(insert_range(&completion_of(&items[0])).start_column, 2);
        model.dispose();
    });
}

#[test]
fn no_user_snippets_in_suggestions_when_inside_the_code_30508() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("", "foo", "<foo>$0</foo>", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.add_model(f.create_model("<head>\n\t\n>/head>"));
        let result = provide(&provider, &model, Position::new(1, 1), &context).await;
        assert_eq!(result.suggestions.len(), 1);
        let completions = as_completion_model(&model, Position::new(1, 1), &provider, &context).await;
        assert_eq!(completions.items().len(), 1);

        let result = provide(&provider, &model, Position::new(2, 2), &context).await;
        assert_eq!(result.suggestions.len(), 1);
        let completions2 = as_completion_model(&model, Position::new(2, 2), &provider, &context).await;
        assert_eq!(completions2.items().len(), 1);
    });
}

#[test]
fn snippet_suggest_ensure_extension_snippets_come_last() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("second", "second", "second", SnippetSource::Extension),
            new_snippet("first", "first", "first", SnippetSource::User),
        ]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.add_model(f.create_model(""));
        let result = provide(&provider, &model, Position::new(1, 1), &context).await;
        assert_eq!(result.suggestions.len(), 2);
        let (first, second) = (&result.suggestions[0], &result.suggestions[1]);
        assert_eq!(first.label, label("first", "first"));
        assert_eq!(second.label, label("second", "second"));

        let completions = as_completion_model(&model, Position::new(1, 1), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 2);
        let (first, second) = (completion_of(&items[0]), completion_of(&items[1]));
        assert_eq!(first.label, label("first", "first"));
        assert_eq!(second.label, label("second", "second"));
    });
}

#[test]
fn dash_in_snippets_prefix_broken_53945() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("p-a", "p-a", "second", SnippetSource::User)]);
        let provider = f.provider();
        let context = default_completion_context();

        let model = f.add_model(f.create_model("p-"));

        let result = provide(&provider, &model, Position::new(1, 2), &context).await;
        let completions = as_completion_model(&model, Position::new(1, 2), &provider, &context).await;
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(completions.items().len(), 1);

        let result = provide(&provider, &model, Position::new(1, 3), &context).await;
        let completions = as_completion_model(&model, Position::new(1, 3), &provider, &context).await;
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(completions.items().len(), 1);

        let result = provide(&provider, &model, Position::new(1, 3), &context).await;
        let completions = as_completion_model(&model, Position::new(1, 3), &provider, &context).await;
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(completions.items().len(), 1);
    });
}

#[test]
fn no_snippets_suggestion_on_long_lines_beyond_character_100_58807() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("bug", "bug", "second", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.add_model(f.create_model(
            "Thisisaverylonglinegoingwithmore100bcharactersandthismakesintellisensebecomea Thisisaverylonglinegoingwithmore100bcharactersandthismakesintellisensebecomea b",
        ));
        let result = provide(&provider, &model, Position::new(1, 158), &context).await;
        let completions = as_completion_model(&model, Position::new(1, 158), &provider, &context).await;

        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(completions.items().len(), 1);
    });
}

#[test]
fn type_colon_will_trigger_snippet_60746() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("bug", "bug", "second", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.add_model(f.create_model(":"));
        let result = provide(&provider, &model, Position::new(1, 2), &context).await;
        assert_eq!(result.suggestions.len(), 0);

        let completions = as_completion_model(&model, Position::new(1, 2), &provider, &context).await;
        assert_eq!(completions.items().len(), 0);
    });
}

#[test]
fn substring_of_prefix_can_not_trigger_snippet_60737() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("mytemplate", "mytemplate", "second", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.add_model(f.create_model("template"));
        let result = provide(&provider, &model, Position::new(1, 9), &context).await;

        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].label, label("mytemplate", "mytemplate"));

        let completions = as_completion_model(&model, Position::new(1, 9), &provider, &context).await;
        assert_eq!(completions.items().len(), 0);
    });
}

#[test]
fn no_snippets_suggestion_beyond_character_100_if_not_at_end_of_line_60247() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("bug", "bug", "second", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.add_model(f.create_model(
            "Thisisaverylonglinegoingwithmore100bcharactersandthismakesintellisensebecomea Thisisaverylonglinegoingwithmore100bcharactersandthismakesintellisensebecomea b text_after_b",
        ));

        let result = provide(&provider, &model, Position::new(1, 158), &context).await;
        assert_eq!(result.suggestions.len(), 1);

        let completions = as_completion_model(&model, Position::new(1, 158), &provider, &context).await;
        assert_eq!(completions.items().len(), 1);
    });
}

#[test]
fn issue_61296_vs_code_freezes_when_editing_css_file_with_emoji() {
    run_test(|f| async move {
        let language_configuration_service = f.disposables.add_rc(TestLanguageConfigurationService::new());
        f.disposables.add(language_configuration_service.register(
            "fooLang",
            LanguageConfiguration {
                word_pattern: Some(Rc::new(
                    JsRegExp::from_str(r"(#?-?\d*\.\d\w*%?)|(::?[\w-]*(?=[^,{;]*[,{]))|(([@#.!])?[\w\-?]+%?|[@#!.])", "g").expect("regex válida"),
                )),
                ..LanguageConfiguration::default()
            },
            None,
        ));

        f.set_snippets(vec![new_snippet("bug", "-a-bug", "second", SnippetSource::User)]);

        let provider = f.provider_with(language_configuration_service);
        let context = default_completion_context();

        let model = f.add_model(f.create_model(".🐷-a-b"));

        let result = provide(&provider, &model, Position::new(1, 8), &context).await;
        assert_eq!(result.suggestions.len(), 1);

        let completions = as_completion_model(&model, Position::new(1, 8), &provider, &context).await;
        assert_eq!(completions.items().len(), 1);
    });
}

#[test]
fn no_snippets_shown_when_triggering_completions_at_whitespace_on_line_that_already_has_text_62335() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("bug", "bug", "second", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.add_model(f.create_model("a "));

        let result = provide(&provider, &model, Position::new(1, 3), &context).await;
        assert_eq!(result.suggestions.len(), 1);

        let completions = as_completion_model(&model, Position::new(1, 3), &provider, &context).await;
        assert_eq!(completions.items().len(), 1);
    });
}

#[test]
fn snippet_prefix_with_special_chars_and_numbers_does_not_work_62906() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("noblockwdelay", "<<", "<= #dly\"", SnippetSource::User),
            new_snippet("noblockwdelay", "11", "eleven", SnippetSource::User),
        ]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.create_model(" <");

        let result = provide(&provider, &model, Position::new(1, 3), &context).await;
        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(insert_range(first).start_column, 2);

        let completions = as_completion_model(&model, Position::new(1, 3), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].edit_start.column, 2);

        model.dispose();
        let model = f.create_model("1");
        let result = provide(&provider, &model, Position::new(1, 2), &context).await;
        let completions = as_completion_model(&model, Position::new(1, 2), &provider, &context).await;

        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(insert_range(first).start_column, 1);
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].edit_start.column, 1);

        model.dispose();
    });
}

#[test]
fn snippet_replace_range() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("notWordTest", "not word", "not word snippet", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.create_model("not wordFoo bar");

        let result = provide(&provider, &model, Position::new(1, 3), &context).await;
        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(insert_range(first).end_column, 3);
        assert_eq!(replace_range(first).end_column, 9);

        let completions = as_completion_model(&model, Position::new(1, 3), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].edit_insert_end.column, 3);
        assert_eq!(items[0].edit_replace_end.column, 9);

        model.dispose();
        let model = f.create_model("not woFoo bar");
        let result = provide(&provider, &model, Position::new(1, 3), &context).await;

        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(insert_range(first).end_column, 3);
        assert_eq!(replace_range(first).end_column, 3);

        let completions = as_completion_model(&model, Position::new(1, 3), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].edit_insert_end.column, 3);
        assert_eq!(items[0].edit_replace_end.column, 3);

        model.dispose();
        let model = f.create_model("not word");
        let result = provide(&provider, &model, Position::new(1, 1), &context).await;

        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(insert_range(first).end_column, 1);
        assert_eq!(replace_range(first).end_column, 9);

        let completions = as_completion_model(&model, Position::new(1, 1), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].edit_insert_end.column, 1);
        assert_eq!(items[0].edit_replace_end.column, 9);

        model.dispose();
    });
}

#[test]
fn snippet_replace_range_incorrect_108894() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("eng", "eng", "<span></span>", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.create_model("filler e KEEP ng filler");
        let result = provide(&provider, &model, Position::new(1, 9), &context).await;
        let completions = as_completion_model(&model, Position::new(1, 9), &provider, &context).await;

        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(insert_range(first).end_column, 9);
        assert_eq!(replace_range(first).end_column, 9);

        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].edit_insert_end.column, 9);
        assert_eq!(items[0].edit_replace_end.column, 9);

        model.dispose();
    });
}

#[test]
fn snippet_will_replace_auto_closing_pair_if_specified_in_prefix() {
    run_test(|f| async move {
        let language_configuration_service = f.disposables.add_rc(TestLanguageConfigurationService::new());
        f.disposables.add(language_configuration_service.register(
            "fooLang",
            LanguageConfiguration {
                brackets: Some(vec![
                    ("{".to_owned(), "}".to_owned()),
                    ("[".to_owned(), "]".to_owned()),
                    ("(".to_owned(), ")".to_owned()),
                ]),
                ..LanguageConfiguration::default()
            },
            None,
        ));

        f.set_snippets(vec![new_snippet("PSCustomObject", "[PSCustomObject]", "[PSCustomObject] @{ Key = Value }", SnippetSource::User)]);

        let provider = f.provider_with(language_configuration_service);
        let context = default_completion_context();

        let model = f.create_model("[psc]");
        let result = provide(&provider, &model, Position::new(1, 5), &context).await;
        let completions = as_completion_model(&model, Position::new(1, 5), &provider, &context).await;

        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(insert_range(first).end_column, 5);
        // This is 6 because it should eat the `]` at the end of the text even if cursor is before it
        assert_eq!(replace_range(first).end_column, 6);

        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].edit_insert_end.column, 5);
        assert_eq!(items[0].edit_replace_end.column, 6);

        model.dispose();
    });
}

#[test]
fn leading_whitespace_in_snippet_prefix_123860() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("cite-name", " cite", "~\\cite{$CLIPBOARD}", SnippetSource::User)]);

        let provider = f.provider();
        let context = default_completion_context();

        let model = f.create_model(" ci");
        let result = provide(&provider, &model, Position::new(1, 4), &context).await;
        let completions = as_completion_model(&model, Position::new(1, 4), &provider, &context).await;

        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(label_text(&first.label), " cite");
        assert_eq!(insert_range(first).start_column, 1);

        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text_label, " cite");
        assert_eq!(items[0].edit_start.column, 1);

        model.dispose();
    });
}

#[test]
fn still_show_suggestions_in_string_when_disable_string_suggestion_136611() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("aaa", "aaa", "value", SnippetSource::User),
            new_snippet("bbb", "bbb", "value", SnippetSource::User),
            // new Snippet(['fooLang'], '\'ccc', '\'ccc', '', 'value', '', SnippetSource.User, generateUuid())
        ]);

        let provider = f.provider();

        let model = f.create_model("''");
        let context = CompletionContext { trigger_kind: CompletionTriggerKind::TriggerCharacter, trigger_character: Some("'".to_owned()) };
        let result = provide(&provider, &model, Position::new(1, 2), &context).await;

        assert_eq!(result.suggestions.len(), 0);
        model.dispose();
    });
}

#[test]
fn still_show_suggestions_in_string_when_disable_string_suggestion_136611_part_2() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("aaa", "aaa", "value", SnippetSource::User),
            new_snippet("bbb", "bbb", "value", SnippetSource::User),
            new_snippet("'ccc", "'ccc", "value", SnippetSource::User),
        ]);

        let provider = f.provider();

        let model = f.create_model("''");

        let context = CompletionContext { trigger_kind: CompletionTriggerKind::TriggerCharacter, trigger_character: Some("'".to_owned()) };
        let result = provide(&provider, &model, Position::new(1, 2), &context).await;

        assert_eq!(result.suggestions.len(), 1);

        let completions = as_completion_model(&model, Position::new(1, 2), &provider, &context).await;
        assert_eq!(completions.items().len(), 1);

        model.dispose();
    });
}

#[test]
fn snippet_suggestions_are_too_eager_138707_word() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("tys", "tys", "value", SnippetSource::User),
            new_snippet("hell_or_tell", "hell_or_tell", "value", SnippetSource::User),
            new_snippet("^y", "^y", "value", SnippetSource::User),
        ]);

        let provider = f.provider();
        let model = f.create_model("'hellot'");
        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };

        let result = provide(&provider, &model, Position::new(1, 8), &context).await;

        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(label_text(&result.suggestions[0].label), "hell_or_tell");

        let completions = as_completion_model(&model, Position::new(1, 8), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text_label, "hell_or_tell");

        model.dispose();
    });
}

#[test]
fn snippet_suggestions_are_too_eager_138707_no_word() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("tys", "tys", "value", SnippetSource::User),
            new_snippet("t", "t", "value", SnippetSource::User),
            new_snippet("^y", "^y", "value", SnippetSource::User),
        ]);

        let provider = f.provider();
        let model = f.create_model(")*&^");
        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };

        let result = provide(&provider, &model, Position::new(1, 5), &context).await;

        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(label_text(&result.suggestions[0].label), "^y");

        let completions = as_completion_model(&model, Position::new(1, 5), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text_label, "^y");

        model.dispose();
    });
}

#[test]
fn snippet_suggestions_are_too_eager_138707_word_word() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("async arrow function", "async arrow function", "value", SnippetSource::User),
            new_snippet("foobarrrrrr", "foobarrrrrr", "value", SnippetSource::User),
        ]);

        let provider = f.provider();
        let model = f.create_model("foobar");
        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };

        let result = provide(&provider, &model, Position::new(1, 7), &context).await;

        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(label_text(&result.suggestions[0].label), "foobarrrrrr");

        let completions = as_completion_model(&model, Position::new(1, 7), &provider, &context).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text_label, "foobarrrrrr");
        model.dispose();
    });
}

#[test]
fn strange_and_useless_autosuggestion_region_endregion_php_140039() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("reg", "#region", "value", SnippetSource::User)]);

        let provider = f.provider();
        let model = f.create_model("function abc(w)");
        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };
        let result = provide(&provider, &model, Position::new(1, 15), &context).await;

        assert_eq!(result.suggestions.len(), 0);
        model.dispose();
    });
}

/// `test.skip('Snippets disappear with . key #145960', ...)`: o original pula o caso (ver as divergências do arquivo).
#[allow(dead_code)]
async fn snippets_disappear_with_dot_key_145960(f: Rc<Fixture>) {
    f.set_snippets(vec![
        new_snippet("div", "div", "div", SnippetSource::User),
        new_snippet("div.", "div.", "div.", SnippetSource::User),
        new_snippet("div#", "div#", "div#", SnippetSource::User),
    ]);

    let provider = f.provider();
    let model = f.create_model("di");
    let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };
    let result = provide(&provider, &model, Position::new(1, 3), &context).await;

    assert_eq!(result.suggestions.len(), 3);

    model.apply_edits(&[EditOperation::insert(Position::new(1, 3), ".")]);
    assert_eq!(model.get_value().to_string_lossy(), "di.");
    let context = CompletionContext { trigger_kind: CompletionTriggerKind::TriggerCharacter, trigger_character: Some(".".to_owned()) };
    let result2 = provide(&provider, &model, Position::new(1, 4), &context).await;

    assert_eq!(result2.suggestions.len(), 1);
    assert_eq!(result2.suggestions[0].insert_text, "div.");

    model.dispose();
}

#[test]
fn hyphen_in_snippet_prefix_de_indents_snippet_139016() {
    run_test(|f| async move {
        f.set_snippets(vec![new_snippet("foo", "Foo- Bar", "Foo", SnippetSource::User)]);
        let model = f.add_model(f.create_model("    bar"));
        let provider = f.provider();
        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };
        let result = provide(&provider, &model, Position::new(1, 8), &context).await;

        assert_eq!(result.suggestions.len(), 1);
        let first = &result.suggestions[0];
        assert_eq!(insert_range(first).start_column, 5);

        let completions = as_completion_model(&model, Position::new(1, 8), &provider, &default_completion_context()).await;
        let items = completions.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].edit_start.column, 5);
    });
}

#[test]
fn autocomplete_suggests_based_on_the_last_letter_of_a_word_and_it_depends_on_the_typing_speed_191070() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_snippet("/whiletrue", "/whiletrue", "one", SnippetSource::User),
            new_snippet("/sc not expanding", "/sc not expanding", "two", SnippetSource::User),
        ]);

        let provider = f.provider();
        let model = f.add_model(f.create_model(""));
        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };

        {
            // PREFIX: w
            model.set_value("w".into());
            let result1 = provide(&provider, &model, Position::new(1, 2), &context).await;
            assert_eq!(result1.suggestions[0].insert_text, "one");
            assert_eq!(result1.suggestions.len(), 1);
        }

        {
            // PREFIX: where
            model.set_value("where".into());
            let result2 = provide(&provider, &model, Position::new(1, 6), &context).await;
            assert_eq!(result2.suggestions[0].insert_text, "one"); // /whiletrue matches where (WHilEtRuE)
            assert_eq!(result2.suggestions.len(), 1);
        }
    });
}

#[test]
fn get_snippets_sync_include_pattern() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_pattern_snippet("TestSnippet", "test", Some(&["**/*.test.ts"]), None),
            new_pattern_snippet("SpecSnippet", "spec", Some(&["**/*.spec.ts"]), None),
            new_pattern_snippet("AllSnippet", "all", None, None),
        ]);
        let snippet_service = f.snippet_service();

        // Test file should only get TestSnippet and AllSnippet
        let mut snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.test.ts")), None);
        assert_eq!(snippets.len(), 2);
        assert!(snippets.iter().any(|s| s.name == "TestSnippet"));
        assert!(snippets.iter().any(|s| s.name == "AllSnippet"));

        // Spec file should only get SpecSnippet and AllSnippet
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.spec.ts")), None);
        assert_eq!(snippets.len(), 2);
        assert!(snippets.iter().any(|s| s.name == "SpecSnippet"));
        assert!(snippets.iter().any(|s| s.name == "AllSnippet"));

        // Regular file should only get AllSnippet
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.ts")), None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "AllSnippet");

        // Without URI, all snippets should be returned (backward compatibility)
        snippets = snippet_service.get_snippets_sync("fooLang", None, None);
        assert_eq!(snippets.len(), 3);
    });
}

#[test]
fn get_snippets_sync_exclude_pattern() {
    run_test(|f| async move {
        f.set_snippets(vec![
            new_pattern_snippet("ProdSnippet", "prod", None, Some(&["**/*.min.js", "**/dist/**"])),
            new_pattern_snippet("AllSnippet", "all", None, None),
        ]);
        let snippet_service = f.snippet_service();

        // Regular .js file should get both snippets
        let mut snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.js")), None);
        assert_eq!(snippets.len(), 2);

        // Minified file should only get AllSnippet (ProdSnippet is excluded)
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.min.js")), None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "AllSnippet");

        // File in dist folder should only get AllSnippet
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/dist/bundle.js")), None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "AllSnippet");
    });
}

#[test]
fn get_snippets_sync_include_and_exclude_patterns_together() {
    run_test(|f| async move {
        f.set_snippets(vec![new_pattern_snippet("TestSnippet", "test", Some(&["**/*.test.ts", "**/*.spec.ts"]), Some(&["**/*.perf.test.ts"]))]);
        let snippet_service = f.snippet_service();

        // Regular test file should get the snippet
        let mut snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.test.ts")), None);
        assert_eq!(snippets.len(), 1);

        // Spec file should get the snippet
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.spec.ts")), None);
        assert_eq!(snippets.len(), 1);

        // Performance test file should NOT get the snippet (excluded)
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.perf.test.ts")), None);
        assert_eq!(snippets.len(), 0);

        // Regular file should NOT get the snippet (not included)
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.ts")), None);
        assert_eq!(snippets.len(), 0);
    });
}

#[test]
fn get_snippets_sync_filename_only_patterns_no_path_separator() {
    run_test(|f| async move {
        // Patterns without '/' should match on filename only (like files.associations)
        f.set_snippets(vec![
            new_pattern_snippet("TestSnippet", "test", Some(&["*.test.ts"]), None),
            new_pattern_snippet("ConfigSnippet", "config", Some(&["config.json"]), None),
        ]);
        let snippet_service = f.snippet_service();

        // *.test.ts should match any file ending in .test.ts regardless of path
        let mut snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/src/foo.test.ts")), None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "TestSnippet");

        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/other/deep/path/bar.test.ts")), None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "TestSnippet");

        // config.json should match filename exactly
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/config.json")), None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "ConfigSnippet");

        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/deep/nested/path/config.json")), None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "ConfigSnippet");

        // myconfig.json should NOT match config.json pattern
        snippets = snippet_service.get_snippets_sync("fooLang", Some(&uri_file("/project/myconfig.json")), None);
        assert_eq!(snippets.len(), 0);
    });
}
