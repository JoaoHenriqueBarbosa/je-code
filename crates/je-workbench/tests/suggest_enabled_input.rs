//! Porte de `src/vs/workbench/contrib/codeEditor/test/browser/suggestEnabledInput.test.ts` (suíte `SuggestEnabledInput`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e o `disposables`** são o guarda criado no começo do caso, e o
//!   `teardown(() => disposables.clear())` é o descarte dele ao sair de escopo. O caso `async` roda em `run_with_faked_timers`.
//! - **`workbenchInstantiationService`, `ILanguageFeaturesService` e `IModelService`** são o `ModelService` do
//!   [`TextFileTestAccessor`] e um `LanguageFeaturesService` novo. O `stub(IUserInteractionService, ...)`, o `container` do DOM
//!   (`document.body.appendChild`) e o `remove()` dele não têm destino: o `SuggestEnabledInput` do porte é só a lógica de
//!   completação, sem widget (ver o cabeçalho de `suggest_enabled_input.rs`).
//! - **`input.inputWidget.getModel()`** é [`SuggestEnabledInput::input_model`].
//! - **`languageFeaturesService.completionProvider.all(inputModel).at(-1)`** é o último provedor de `all`.
//! - **`provideCompletionItems(...)`** devolve a promessa de `Option<Rc<CompletionList>>`; o `undefined` do original é
//!   `None`, e `inputCompletions?.suggestions.length` é o tamanho da lista.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::uri::URI;
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::languages::{CompletionContext, CompletionTriggerKind};
use je_editor::position::Position;
use je_workbench::contrib::code_editor::suggest_enabled_input::{SuggestEnabledInput, SuggestResultsProvider};
use support::text_file_support::TextFileTestAccessor;

#[test]
fn provides_completions_only_for_the_exact_input_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = TextFileTestAccessor::new(&disposables.store());
        let language_features_service = LanguageFeaturesService::new();
        let model_service = accessor.model_service.clone();

        let input = disposables.add_rc(SuggestEnabledInput::new(
            "suggest-enabled-input",
            SuggestResultsProvider::new(|_query| vec!["result".into()]),
            "test:searchinput",
            None,
            &language_features_service,
            &model_service,
        ));
        let nested_resource_model = disposables.add_rc(model_service.create_model("", None, Some(&URI::parse("test:nested/searchinput", false)), true));
        let input_model = input.input_model().clone();
        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };

        let provider = language_features_service.completion_provider.all(&input_model).last().cloned().expect("o provedor do campo está registrado");
        let input_completions = provider.provide_completion_items(&input_model, Position::new(1, 1), &context, &CancellationToken::None).await.unwrap();
        let nested_completions = provider.provide_completion_items(&nested_resource_model, Position::new(1, 1), &context, &CancellationToken::None).await.unwrap();

        assert_eq!(input_completions.map(|list| list.suggestions.len()), Some(1));
        assert!(nested_completions.is_none());
    });
}
