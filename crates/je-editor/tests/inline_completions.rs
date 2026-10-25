//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/inlineCompletions.test.ts` (suítes `Inline Completions`,
//! `Forward Stability` e `Multi Cursor Support`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado dentro do corpo de cada caso, e `runWithFakedTimers`
//!   (`fakeClock: true`) é `run_with_faked_timers`; o primeiro caso, sem `fakeClock`, roda em `run_with_real_timers`.
//! - `timeout(ms)` é a função local de mesmo nome sobre o `timeout` de `common` (relógio virtual).
//! - `assert.deepStrictEqual` do histórico do provedor compara `Dynamic` por `assert_deep_strict_equal`; o dos estados de
//!   visão compara listas de texto.
//! - `model.accept(editor)` não é aguardado no original, e aqui também não: a parte síncrona do `accept` aplica a edição.
//! - O primeiro caso usa o `DataChannelForwardingTelemetry` do `support` no lugar do `DataChannelForwardingTelemetryService`
//!   (ver o topo de `inline_completions_test_utils.rs`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{assert_deep_strict_equal, run_with_faked_timers, run_with_real_timers};
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable as _;
use je_base::observable::observable_value;
use je_base::observable::{Observable as _, SettableExt as _};
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime::date_now;
use je_editor::code_action::languages::ProviderId;
use je_editor::code_editor_widget::ICodeEditor as _;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::inline_completions::inline_completions_model::TriggerOptions;
use je_editor::inline_completions::inline_completions_source::InlineCompletionsSource;
use je_editor::inline_completions::provide_inline_completions::{InlineCompletionEditorType, InlineSuggestRequestInfo};
use je_editor::language_feature_debounce::IFeatureDebounceInformation;
use je_editor::language_providers::ProviderResult;
use je_editor::languages::{
    IInlineCompletionChangeHint, InlineCompletion, InlineCompletionContext, InlineCompletionInsertText, InlineCompletionTriggerKind,
    InlineCompletions, InlineCompletionsDisposeReason, InlineCompletionsProvider,
};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_base::JsString;
use support::inline_completions_test_utils::{
    GhostTextContext, InlineCompletionsTestContext, InlineCompletionsTestOptions, MockInlineCompletionsProvider,
    ensure_no_leaks_after_global_registrations, with_async_test_code_editor_and_inline_completions_model,
};

// #region utilitários

/// `timeout(ms)` do `async.ts` sobre o relógio virtual.
async fn timeout(ms: u64) {
    common::timeout(ms).await.expect("o timer resolve");
}

/// `{ insertText, range }` do provedor.
fn completion(insert_text: &str, range: Option<Range>) -> InlineCompletion {
    InlineCompletion { insert_text: Some(InlineCompletionInsertText::Text(insert_text.to_owned())), range, ..InlineCompletion::default() }
}

/// `{ position, text, triggerKind }` do histórico de chamadas.
fn call(position: &str, text: &str, trigger_kind: i32) -> Dynamic {
    Dynamic::object([
        ("position", Dynamic::from(position)),
        ("text", Dynamic::from(text)),
        ("triggerKind", Dynamic::from(trigger_kind)),
    ])
}

#[track_caller]
fn assert_calls(actual: Vec<Dynamic>, expected: Vec<Dynamic>) {
    assert_deep_strict_equal(&Dynamic::array(actual), &Dynamic::array(expected));
}

#[track_caller]
fn assert_states(actual: Vec<String>, expected: &[&str]) {
    let actual: Vec<&str> = actual.iter().map(String::as_str).collect();
    assert_eq!(actual, expected);
}

/// O `async` do caso, com o `LeakGuard` e o relógio virtual.
fn run_case<F, Fut>(provider: &Rc<MockInlineCompletionsProvider>, options: InlineCompletionsTestOptions, body: F)
where
    F: FnOnce(InlineCompletionsTestContext, Rc<MockInlineCompletionsProvider>) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    let provider = Rc::clone(provider);
    run_with_faked_timers(async move {
        let _guard = ensure_no_leaks_after_global_registrations();
        with_async_test_code_editor_and_inline_completions_model("", options, move |ctx| body(ctx, provider)).await;
    });
}

fn new_provider() -> Rc<MockInlineCompletionsProvider> {
    MockInlineCompletionsProvider::new(false)
}

fn trigger(model: &Rc<je_editor::inline_completions::inline_completions_model::InlineCompletionsModel>) {
    let _ = model.trigger(None, TriggerOptions::default());
}

fn trigger_explicitly(model: &Rc<je_editor::inline_completions::inline_completions_model::InlineCompletionsModel>) {
    let _ = model.trigger_explicitly(None, false);
}

// #endregion

// #region Inline Completions

/// `{ get: () => 0, update: () => 0, default: () => 0 }`.
struct ZeroDebounce;

impl IFeatureDebounceInformation for ZeroDebounce {
    fn get(&self, _model: &TextModel) -> f64 {
        0.0
    }
    fn update(&self, _model: &TextModel, _value: f64) -> f64 {
        0.0
    }
    fn default_value(&self) -> f64 {
        0.0
    }
}

/// O provedor literal do primeiro caso.
struct DeferredResponseProvider {
    provider_id: ProviderId,
    started: Rc<DeferredPromise<()>>,
    response: Rc<DeferredPromise<Option<Rc<InlineCompletions>>>>,
}

impl InlineCompletionsProvider for DeferredResponseProvider {
    fn provide_inline_completions(
        &self,
        _model: &Rc<TextModel>,
        _position: Position,
        _context: &InlineCompletionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<InlineCompletions>> {
        let _ = self.started.complete(());
        self.response.promise.clone()
    }

    fn dispose_inline_completions(&self, _completions: &Rc<InlineCompletions>, _reason: InlineCompletionsDisposeReason) {}

    fn provider_id(&self) -> Option<&ProviderId> {
        Some(&self.provider_id)
    }
}

#[test]
fn inline_completions_emits_empty_response_telemetry_after_instantiation_service_disposal() {
    run_with_real_timers(async {
        let _guard = ensure_no_leaks_after_global_registrations();
        let provider_started = Rc::new(DeferredPromise::<()>::new());
        let provider_response = Rc::new(DeferredPromise::<Option<Rc<InlineCompletions>>>::new());
        let provider_id = ProviderId::from_extension_id(Some("GitHub.copilot"));
        let provider: Rc<dyn InlineCompletionsProvider> = Rc::new(DeferredResponseProvider {
            provider_id: provider_id.clone(),
            started: Rc::clone(&provider_started),
            response: Rc::clone(&provider_response),
        });
        let sent_channel_ids: Rc<RefCell<Vec<String>>> = Rc::default();
        let send_data: Rc<dyn Fn(&str)> = {
            let sent = Rc::clone(&sent_channel_ids);
            Rc::new(move |channel_id: &str| sent.borrow_mut().push(channel_id.to_owned()))
        };
        let options = InlineCompletionsTestOptions {
            configuration: Some(Dynamic::object([("github.copilot.enable", Dynamic::object([("*", Dynamic::Bool(true))]))])),
            data_channel_send: Some(send_data),
            ..InlineCompletionsTestOptions::with_provider(Rc::clone(&provider))
        };

        with_async_test_code_editor_and_inline_completions_model("", options, move |ctx| async move {
            let InlineCompletionsTestContext { editor, model, store, instantiation_service, source_services, .. } = ctx;
            let cursor_position = observable_value::<Position, ()>("testCursorPosition", editor.get_position().expect("posição"));
            let source = store.add_rc(InlineCompletionsSource::new(
                Rc::clone(&model.text_model),
                Rc::clone(&model.text_model_version_id),
                Rc::new(ZeroDebounce),
                cursor_position.to_observable(),
                Some("github.copilot.enable".to_owned()),
                source_services,
            ));
            let request = source.fetch(
                vec![Rc::clone(&provider)],
                None,
                InlineCompletionContext {
                    trigger_kind: InlineCompletionTriggerKind::Explicit,
                    selected_suggestion_info: None,
                    user_prompt: None,
                    request_uuid: String::new(),
                    include_inline_edits: false,
                    include_inline_completions: true,
                    request_issued_date_time: date_now() as f64,
                    earliest_shown_date_time: 0.0,
                    change_hint: None,
                },
                None,
                false,
                observable_value::<bool, ()>("userJumpedToActiveCompletion", false).to_observable(),
                InlineSuggestRequestInfo {
                    start_time: date_now() as f64,
                    sku: None,
                    editor_type: InlineCompletionEditorType::TextEditor,
                    language_id: "plaintext".to_owned(),
                    available_providers: vec![provider_id],
                    reason: String::new(),
                    typing_interval: 0.0,
                    typing_interval_character_count: 0,
                },
            );
            provider_started.promise.clone().await.expect("o provedor começou");
            instantiation_service.dispose();
            provider_response.complete(Some(Rc::new(InlineCompletions::default()))).await.expect("a resposta completa");
            request.await.expect("o pedido termina");
        })
        .await;

        assert_eq!(*sent_channel_ids.borrow(), vec!["editTelemetry".to_owned()]);
    });
}

#[test]
fn inline_completions_does_not_trigger_automatically_if_disabled() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider).with_inline_suggest_enabled(false), |ctx, provider| async move {
        ctx.context.keyboard_type("foo");
        timeout(1000).await;

        // Provider is not called, no ghost text is shown.
        assert_calls(provider.get_and_clear_call_history(), vec![]);
        assert_states(ctx.context.get_and_clear_view_states(), &[""]);
    });
}

#[test]
fn inline_completions_ghost_text_is_shown_after_trigger() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("foo");
        provider.set_return_value(Some(completion("foobar", Some(Range::new(1, 1, 1, 4)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 1)]);
        assert_states(ctx.context.get_and_clear_view_states(), &["", "foo[bar]"]);
    });
}

#[test]
fn inline_completions_ghost_text_is_shown_automatically_when_configured() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider).with_inline_suggest_enabled(true), |ctx, provider| async move {
        ctx.context.keyboard_type("foo");

        provider.set_return_value(Some(completion("foobar", Some(Range::new(1, 1, 1, 4)))));
        timeout(1000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &["", "foo[bar]"]);
    });
}

#[test]
fn inline_completions_ghost_text_is_updated_automatically() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        provider.set_return_value(Some(completion("foobar", Some(Range::new(1, 1, 1, 4)))));
        ctx.context.keyboard_type("foo");
        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        provider.set_return_value(Some(completion("foobizz", Some(Range::new(1, 1, 1, 6)))));
        ctx.context.keyboard_type("b");
        ctx.context.keyboard_type("i");
        timeout(1000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 1), call("(1,6)", "foobi", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &["", "foo[bar]", "foob[ar]", "foobi", "foobi[zz]"]);
    });
}

#[test]
fn inline_completions_unindent_whitespace() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("  ");
        provider.set_return_value(Some(completion("foo", Some(Range::new(1, 2, 1, 3)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        assert_states(ctx.context.get_and_clear_view_states(), &["", "  [foo]"]);

        let _ = ctx.model.accept(ctx.editor.widget(), false);

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,3)", "  ", 1)]);

        assert_states(ctx.context.get_and_clear_view_states(), &[" foo"]);
    });
}

#[test]
fn inline_completions_unindent_tab() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("\t\t");
        provider.set_return_value(Some(completion("foo", Some(Range::new(1, 2, 1, 3)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        assert_states(ctx.context.get_and_clear_view_states(), &["", "\t\t[foo]"]);

        let _ = ctx.model.accept(ctx.editor.widget(), false);

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,3)", "\t\t", 1)]);

        assert_states(ctx.context.get_and_clear_view_states(), &["\tfoo"]);
    });
}

#[test]
fn inline_completions_no_unindent_after_indentation() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("buzz  ");
        provider.set_return_value(Some(completion("foo", Some(Range::new(1, 6, 1, 7)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        assert_states(ctx.context.get_and_clear_view_states(), &[""]);

        let _ = ctx.model.accept(ctx.editor.widget(), false);

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,7)", "buzz  ", 1)]);

        assert_states(ctx.context.get_and_clear_view_states(), &[]);
    });
}

#[test]
fn inline_completions_next_previous() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        let (model, context, logger) = (&ctx.model, &ctx.context, &ctx.logger);
        context.keyboard_type("foo");
        provider.set_return_value(Some(completion("foobar1", Some(Range::new(1, 1, 1, 4)))));
        let _ = logger.log_run("() => model.trigger()", || model.trigger(None, TriggerOptions::default()));
        timeout(1000).await;

        assert_states(context.get_and_clear_view_states(), &["", "foo[bar1]"]);

        provider.set_return_values(vec![
            completion("foobar1", Some(Range::new(1, 1, 1, 4))),
            completion("foobizz2", Some(Range::new(1, 1, 1, 4))),
            completion("foobuzz3", Some(Range::new(1, 1, 1, 4))),
        ]);

        let _ = logger.log_run("() => model.next()", || model.next());
        timeout(1000).await;
        assert_states(context.get_and_clear_view_states(), &["foo[bizz2]"]);

        let _ = logger.log_run("() => model.next()", || model.next());
        timeout(1000).await;
        assert_states(context.get_and_clear_view_states(), &["foo[buzz3]"]);

        let _ = logger.log_run("() => model.next()", || model.next());
        timeout(1000).await;
        assert_states(context.get_and_clear_view_states(), &["foo[bar1]"]);

        let _ = logger.log_run("() => model.previous()", || model.previous());
        timeout(1000).await;
        assert_states(context.get_and_clear_view_states(), &["foo[buzz3]"]);

        let _ = logger.log_run("() => model.previous()", || model.previous());
        timeout(1000).await;
        assert_states(context.get_and_clear_view_states(), &["foo[bizz2]"]);

        let _ = logger.log_run("() => model.previous()", || model.previous());
        timeout(1000).await;
        assert_states(context.get_and_clear_view_states(), &["foo[bar1]"]);

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 0), call("(1,4)", "foo", 1)]);
    });
}

#[test]
fn inline_completions_calling_the_provider_is_debounced() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        trigger(&ctx.model);

        ctx.context.keyboard_type("f");
        timeout(40).await;
        ctx.context.keyboard_type("o");
        timeout(40).await;
        ctx.context.keyboard_type("o");
        timeout(40).await;

        // The provider is not called
        assert_calls(provider.get_and_clear_call_history(), vec![]);

        timeout(400).await;
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 0)]);

        provider.assert_not_called_twice_within_50ms();
    });
}

#[test]
fn inline_completions_backspace_is_debounced() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider).with_inline_suggest_enabled(true), |ctx, provider| async move {
        ctx.context.keyboard_type("foo");

        provider.set_return_value(Some(completion("foobar", Some(Range::new(1, 1, 1, 4)))));
        timeout(1000).await;

        for _ in 0..2 {
            for _ in 0..3 {
                ctx.context.left_delete();
                timeout(5).await;
            }

            ctx.context.keyboard_type("bar");
        }

        timeout(400).await;

        provider.assert_not_called_twice_within_50ms();
    });
}

// #endregion

// #region Forward Stability

#[test]
fn forward_stability_typing_agrees() {
    // The user types the text as suggested and the provider is forward-stable
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        provider.set_return_value(Some(completion("foobar", None)));
        ctx.context.keyboard_type("foo");
        trigger(&ctx.model);
        timeout(1000).await;
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &["", "foo[bar]"]);

        ctx.context.keyboard_type("b");
        assert_states(ctx.context.get_and_clear_view_states(), &["foob[ar]"]);
        timeout(1000).await;
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,5)", "foob", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &[]);

        ctx.context.keyboard_type("a");
        assert_states(ctx.context.get_and_clear_view_states(), &["fooba[r]"]);
        timeout(1000).await;
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,6)", "fooba", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &[]);
    });
}

async fn setup_scenario(ctx: &InlineCompletionsTestContext, provider: &Rc<MockInlineCompletionsProvider>) {
    assert_states(ctx.context.get_and_clear_view_states(), &[""]);
    provider.set_return_value(Some(completion("foo bar", None)));
    ctx.context.keyboard_type("f");
    trigger_explicitly(&ctx.model);
    timeout(10000).await;
    assert_calls(provider.get_and_clear_call_history(), vec![call("(1,2)", "f", 1)]);
    assert_states(ctx.context.get_and_clear_view_states(), &["f[oo bar]"]);

    provider.set_return_value(Some(completion("foo baz", None)));
    timeout(10000).await;
}

#[test]
fn forward_stability_support_forward_instability() {
    // The user types the text as suggested and the provider reports a different suggestion.
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        setup_scenario(&ctx, &provider).await;

        ctx.context.keyboard_type("o");
        assert_states(ctx.context.get_and_clear_view_states(), &["fo[o bar]"]);
        timeout(10000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,3)", "fo", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &["fo[o baz]"]);
    });
}

#[test]
fn forward_stability_when_accepting_word_by_word() {
    // The user types the text as suggested and the provider reports a different suggestion.
    // Even when triggering explicitly, we want to keep the suggestion.
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        setup_scenario(&ctx, &provider).await;

        ctx.model.accept_next_word().await.expect("acceptNextWord");
        assert_states(ctx.context.get_and_clear_view_states(), &["foo[ bar]"]);

        timeout(10000).await;
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &[]);

        ctx.model.trigger_explicitly(None, false).await.expect("triggerExplicitly"); // reset to provider truth
        timeout(10000).await;
        assert_states(ctx.context.get_and_clear_view_states(), &[]);
    });
}

#[test]
fn forward_stability_when_accepting_undo() {
    // The user types the text as suggested and the provider reports a different suggestion.
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        setup_scenario(&ctx, &provider).await;

        ctx.model.accept_next_word().await.expect("acceptNextWord");
        assert_states(ctx.context.get_and_clear_view_states(), &["foo[ bar]"]);

        timeout(10000).await;
        assert_states(ctx.context.get_and_clear_view_states(), &[]);
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 0)]);

        ctx.editor.get_model().expect("modelo").undo();
        timeout(10000).await;
        assert_states(ctx.context.get_and_clear_view_states(), &["f[oo bar]"]);
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,2)", "f", 0)]);

        ctx.editor.get_model().expect("modelo").redo();
        timeout(10000).await;
        assert_states(ctx.context.get_and_clear_view_states(), &["foo[ bar]"]);
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 0)]);
    });
}

#[test]
fn forward_stability_support_backward_instability() {
    // The user deletes text and the suggestion changes
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("fooba");

        provider.set_return_value(Some(completion("foobar", Some(Range::new(1, 1, 1, 6)))));

        trigger_explicitly(&ctx.model);
        timeout(1000).await;
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,6)", "fooba", 1)]);
        assert_states(ctx.context.get_and_clear_view_states(), &["", "fooba[r]"]);

        provider.set_return_value(Some(completion("foobaz", Some(Range::new(1, 1, 1, 5)))));
        ctx.context.left_delete();
        timeout(1000).await;
        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,5)", "foob", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &["foob[ar]", "foob[az]"]);
    });
}

#[test]
fn forward_stability_push_item_to_preserve_to_front() {
    let provider = MockInlineCompletionsProvider::new(true);
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        provider.set_return_value(Some(completion("foobar", Some(Range::new(1, 1, 1, 4)))));
        ctx.context.keyboard_type("foo");
        timeout(1000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 0)]);
        assert_states(ctx.context.get_and_clear_view_states(), &["", "foo[bar]"]);

        provider.set_return_values(vec![
            completion("foobar1", Some(Range::new(1, 1, 1, 4))),
            completion("foobar", Some(Range::new(1, 1, 1, 4))),
        ]);

        ctx.model.trigger_explicitly(None, false).await.expect("triggerExplicitly");
        timeout(1000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,4)", "foo", 1)]);
        assert_states(ctx.context.get_and_clear_view_states(), &[]);
    });
}

// #endregion

#[test]
fn inline_completions_no_race_conditions() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("h");
        provider.set_return_value_with_delay(Some(completion("helloworld", Some(Range::new(1, 1, 1, 2)))), 1000);

        trigger_explicitly(&ctx.model);

        timeout(1030).await;
        ctx.context.keyboard_type("ello");
        provider.set_return_value_with_delay(Some(completion("helloworld", Some(Range::new(1, 1, 1, 6)))), 1000);

        // after 20ms: Inline completion provider answers back
        // after 50ms: Debounce is triggered
        timeout(2000).await;

        assert_states(ctx.context.get_and_clear_view_states(), &["", "hello[world]"]);
    });
}

#[test]
fn inline_completions_do_not_reuse_cache_from_previous_session_132516() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider).with_inline_suggest_enabled(true), |ctx, provider| async move {
        ctx.context.keyboard_type("hello\n");
        ctx.context.cursor_left();
        ctx.context.keyboard_type("x");
        ctx.context.left_delete();
        provider.set_return_value_with_delay(Some(completion("helloworld", Some(Range::new(1, 1, 1, 6)))), 1000);
        timeout(2000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,6)", "hello\n", 0)]);

        provider.set_return_value_with_delay(Some(completion("helloworld", Some(Range::new(2, 1, 2, 6)))), 1000);

        ctx.context.cursor_down();
        ctx.context.keyboard_type("hello");
        timeout(40).await;

        assert_calls(provider.get_and_clear_call_history(), vec![]);

        // Update ghost text
        ctx.context.keyboard_type("w");
        ctx.context.left_delete();

        timeout(2000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(2,6)", "hello\nhello", 0)]);

        assert_states(ctx.context.get_and_clear_view_states(), &["", "hello[world]\n", "hello\n", "hello\nhello[world]"]);
    });
}

#[test]
fn inline_completions_additional_text_edits() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("buzz\nbaz");
        provider.set_return_value(Some(InlineCompletion {
            additional_text_edits: Some(vec![SingleEditOperation::new(Range::new(1, 1, 1, 5), Some(JsString::from("bla")), false)]),
            ..completion("bazz", Some(Range::new(2, 1, 2, 4)))
        }));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        let _ = ctx.model.accept(ctx.editor.widget(), false);

        assert_calls(provider.get_and_clear_call_history(), vec![call("(2,4)", "buzz\nbaz", 1)]);

        assert_states(ctx.context.get_and_clear_view_states(), &["", "buzz\nbaz[z]", "bla\nbazz"]);
    });
}

// #region Multi Cursor Support

fn editor_text(ctx: &InlineCompletionsTestContext) -> String {
    ctx.editor.get_value(None).to_string_lossy()
}

async fn accept_next_word(ctx: &InlineCompletionsTestContext, times_to_accept: usize) {
    for _ in 0..times_to_accept {
        trigger_explicitly(&ctx.model);
        timeout(1000).await;
        ctx.model.accept_next_word().await.expect("acceptNextWord");
    }
}

fn two_cursors_at_line_ends(ctx: &InlineCompletionsTestContext) {
    ctx.editor.set_selections(&[Selection::new(1, 1000, 1, 1000), Selection::new(2, 1000, 2, 1000)], None, None);
}

#[test]
fn multi_cursor_support_basic() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("console\nconsole\n");
        two_cursors_at_line_ends(&ctx);
        provider.set_return_value(Some(completion("console.log(\"hello\");", Some(Range::new(1, 1, 1, 1000)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;
        let _ = ctx.model.accept(ctx.editor.widget(), false);
        assert_eq!(editor_text(&ctx), ["console.log(\"hello\");", "console.log(\"hello\");", ""].join("\n"));
    });
}

#[test]
fn multi_cursor_support_multi_part() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("console.log()\nconsole.log\n");
        ctx.editor.set_selections(&[Selection::new(1, 12, 1, 12), Selection::new(2, 1000, 2, 1000)], None, None);
        provider.set_return_value(Some(completion("console.log(\"hello\");", Some(Range::new(1, 1, 1, 1000)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;
        let _ = ctx.model.accept(ctx.editor.widget(), false);
        assert_eq!(editor_text(&ctx), ["console.log(\"hello\");", "console.log", ""].join("\n"));
    });
}

#[test]
fn multi_cursor_support_multi_part_and_different_cursor_columns() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("console.log()\nconsole.warn\n");
        ctx.editor.set_selections(&[Selection::new(1, 12, 1, 12), Selection::new(2, 14, 2, 14)], None, None);
        provider.set_return_value(Some(completion("console.log(\"hello\");", Some(Range::new(1, 1, 1, 1000)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;
        let _ = ctx.model.accept(ctx.editor.widget(), false);
        assert_eq!(editor_text(&ctx), ["console.log(\"hello\");", "console.warn", ""].join("\n"));
    });
}

#[test]
fn multi_cursor_support_basic_partial_completion() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("let\nlet\n");
        two_cursors_at_line_ends(&ctx);

        provider.set_return_value(Some(completion("let a = 'some word'; ", Some(Range::new(1, 1, 1, 1000)))));

        accept_next_word(&ctx, 2).await;

        assert_eq!(editor_text(&ctx), ["let a", "let a", ""].join("\n"));
    });
}

#[test]
fn multi_cursor_support_partial_multi_part_completion() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("for ()\nfor \n");
        ctx.editor.set_selections(&[Selection::new(1, 5, 1, 5), Selection::new(2, 1000, 2, 1000)], None, None);

        provider.set_return_value(Some(completion("for (let i = 0; i < 10; i++) {", Some(Range::new(1, 1, 1, 1000)))));

        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        accept_next_word(&ctx, 3).await;

        assert_eq!(editor_text(&ctx), ["for (let i)", "for ", ""].join("\n"));
    });
}

#[test]
fn multi_cursor_support_partial_mutli_part_and_different_cursor_columns_completion() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider), |ctx, provider| async move {
        ctx.context.keyboard_type("console.log()\nconsole.warnnnn\n");
        ctx.editor.set_selections(&[Selection::new(1, 12, 1, 12), Selection::new(2, 16, 2, 16)], None, None);

        provider.set_return_value(Some(completion("console.log(\"hello\" + \" \" + \"world\");", Some(Range::new(1, 1, 1, 1000)))));

        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        accept_next_word(&ctx, 4).await;

        assert_eq!(editor_text(&ctx), ["console.log(\"hello\" + )", "console.warnnnn", ""].join("\n"));
    });
}

#[test]
fn multi_cursor_support_change_hint_is_passed_from_on_did_change_to_provide_inline_completions() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider).with_inline_suggest_enabled(true), |ctx, provider| async move {
        ctx.context.keyboard_type("foo");
        provider.set_return_value(Some(completion("foobar", Some(Range::new(1, 1, 1, 4)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        let first_call_history = provider.get_and_clear_call_history();
        assert_eq!(first_call_history.len(), 1);
        assert!(first_call_history[0].get_str("changeHint").is_undefined());

        // Change cursor position to avoid cache hit
        ctx.editor.set_position(Position::new(1, 3), None);

        let change_hint_data = Dynamic::object([("reason", Dynamic::from("modelUpdated")), ("version", Dynamic::from(42))]);
        provider.set_return_value(Some(completion("foobaz", Some(Range::new(1, 1, 1, 4)))));
        provider.fire_on_did_change(Some(IInlineCompletionChangeHint { data: Some(change_hint_data) }));
        timeout(1000).await;

        let second_call_history = provider.get_and_clear_call_history();

        let expected = Dynamic::object([
            (
                "changeHint",
                Dynamic::object([(
                    "data",
                    Dynamic::object([("reason", Dynamic::from("modelUpdated")), ("version", Dynamic::from(42))]),
                )]),
            ),
            ("position", Dynamic::from("(1,3)")),
            ("text", Dynamic::from("foo")),
            ("triggerKind", Dynamic::from(0)),
        ]);
        assert_calls(second_call_history, vec![expected]);
    });
}

#[test]
fn multi_cursor_support_change_hint_is_undefined_when_on_did_change_fires_without_hint() {
    let provider = new_provider();
    run_case(&provider, InlineCompletionsTestOptions::mock(&provider).with_inline_suggest_enabled(true), |ctx, provider| async move {
        ctx.context.keyboard_type("foo");
        provider.set_return_value(Some(completion("foobar", Some(Range::new(1, 1, 1, 4)))));
        trigger_explicitly(&ctx.model);
        timeout(1000).await;

        provider.get_and_clear_call_history();

        // Change cursor position to avoid cache hit
        ctx.editor.set_position(Position::new(1, 3), None);

        provider.set_return_value(Some(completion("foobaz", Some(Range::new(1, 1, 1, 4)))));
        provider.fire_on_did_change(None);
        timeout(1000).await;

        assert_calls(provider.get_and_clear_call_history(), vec![call("(1,3)", "foo", 0)]);
    });
}

// #endregion
