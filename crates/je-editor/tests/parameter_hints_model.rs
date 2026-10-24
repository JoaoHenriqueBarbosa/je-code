//! Porte de `src/vs/editor/contrib/parameterHints/test/browser/parameterHintsModel.test.ts` (suíte
//! `ParameterHintsModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` são [`Fixture`]: o registro novo e o `store` (o `LeakGuard`, que também faz o papel de
//!   `ensureNoDisposablesAreLeakedInTestSuite()`); o `disposables.clear()` do `teardown` é o descarte do `LeakGuard` no
//!   fim do caso.
//! - `createMockEditor(fileContents)` é [`create_mock_editor`]: o modelo com a URI `test:somefile.ttt` e o editor de
//!   teste; o `serviceCollection` do original (telemetria nula e armazenamento em memória) não é lido pelo modelo de
//!   dicas e não existe no porte.
//! - `runWithFakedTimers({ useFakeTimers: true }, async () => ...)` é o corpo `async` de [`run_with_faked_timers`], que o
//!   executa no event loop do porte com o relógio virtual. `promiseWithResolvers` é `Promise::new()`.
//! - As classes anônimas que implementam `SignatureHelpProvider` são o [`TestProvider`]: os caracteres de gatilho e de
//!   regatilho e o corpo de `provideSignatureHelp` (sem o `token` e o `context` quando o caso não os lê). O
//!   `try { ... } catch (err) { console.error(err); throw err; }` em volta de cada corpo só repassa o erro: uma asserção
//!   que falha dentro do provedor é um pânico, que `provide_signature_help` reporta como erro inesperado, e o caso
//!   reprova (o `done` nunca chega ou o `LeakGuard` acusa o erro).
//! - `assert.strictEqual(context.activeSignatureHelp, emptySigHelp)` compara a identidade do objeto (`Rc::ptr_eq`):
//!   `emptySigHelp` é o `Rc<SignatureHelp>` criado no começo do caso e `emptySigHelpResult` é o resultado que o guarda.
//! - `getNextHint(model)` é [`get_next_hint`]: a promessa da próxima mudança de dicas (o `SignatureHelpResult` que o
//!   original monta em volta do valor é o próprio `Option<Rc<SignatureHelp>>`).
//! - `editor.trigger('keyboard', Handler.Type, { text })` é [`type_text`]. `hintModel.cancel()` é `cancel(false)`.
//! - `assert.fail('Unexpected invoke')` é `panic!`; `new Promise<...>(resolve => setTimeout(..., ms))` é
//!   `Promise::with_executor` com `runtime::set_timeout`.
//! - No caso `Should cancel existing request when new request comes in`, o `try/catch` com `reject(e)` do ouvinte é o
//!   `errors::catch` com `done.reject(error)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::code_editor_widget::{Handler, ICodeEditor};
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::ProviderResult;
use je_editor::language_selector::{LanguageFilter, LanguageSelector};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::parameter_hints::languages::{
    ParameterInformation, ParameterLabel, SignatureHelp, SignatureHelpContext, SignatureHelpProvider, SignatureHelpResult,
    SignatureHelpTriggerKind, SignatureInformation,
};
use je_editor::parameter_hints::parameter_hints_model::{ParameterHintsModel, TriggerContext};
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, create_test_code_editor};
use support::test_text_model::create_text_model_with;

type Registry = Rc<LanguageFeatureRegistry<dyn SignatureHelpProvider>>;

/// `{ scheme: 'test' }` (o `mockFileSelector`).
fn mock_file_selector() -> LanguageSelector {
    LanguageSelector::Filter(LanguageFilter { scheme: Some("test".to_string()), ..LanguageFilter::default() })
}

/// `emptySigHelp`.
fn new_empty_sig_help() -> Rc<SignatureHelp> {
    Rc::new(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: "none".to_string(),
            documentation: None,
            parameters: Vec::new(),
            active_parameter: None,
        }],
        active_parameter: 0,
        active_signature: 0,
    })
}

/// `{ value, dispose: () => { } }`: um resultado de provedor que já está pronto.
fn result_of(value: Rc<SignatureHelp>) -> ProviderResult<Rc<SignatureHelpResult>> {
    Promise::resolved(Some(Rc::new(SignatureHelpResult::new(value, || {}))))
}

/// O corpo de `provideSignatureHelp` de um provedor de teste.
type ProvideFn = dyn Fn(&SignatureHelpContext, &CancellationToken) -> ProviderResult<Rc<SignatureHelpResult>>;

/// As classes anônimas que implementam `SignatureHelpProvider` nos casos.
struct TestProvider {
    signature_help_trigger_characters: Vec<String>,
    signature_help_retrigger_characters: Vec<String>,
    provide: Box<ProvideFn>,
}

impl TestProvider {
    fn new(
        trigger_characters: &[&str],
        retrigger_characters: &[&str],
        provide: impl Fn(&SignatureHelpContext, &CancellationToken) -> ProviderResult<Rc<SignatureHelpResult>> + 'static,
    ) -> Rc<Self> {
        Rc::new(Self {
            signature_help_trigger_characters: trigger_characters.iter().map(|ch| (*ch).to_string()).collect(),
            signature_help_retrigger_characters: retrigger_characters.iter().map(|ch| (*ch).to_string()).collect(),
            provide: Box::new(provide),
        })
    }
}

impl SignatureHelpProvider for TestProvider {
    fn signature_help_trigger_characters(&self) -> &[String] {
        &self.signature_help_trigger_characters
    }

    fn signature_help_retrigger_characters(&self) -> &[String] {
        &self.signature_help_retrigger_characters
    }

    fn provide_signature_help(
        &self,
        _model: &Rc<je_editor::text_model::TextModel>,
        _position: je_editor::position::Position,
        token: &CancellationToken,
        context: &SignatureHelpContext,
    ) -> ProviderResult<Rc<SignatureHelpResult>> {
        (self.provide)(context, token)
    }
}

/// O que `setup()` deixa em variáveis da suíte.
struct Fixture {
    disposables: LeakGuard,
    registry: Registry,
}

impl Fixture {
    fn new() -> Self {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let registry: Registry = Rc::new(LanguageFeatureRegistry::new(None));
        Self { disposables, registry }
    }

    /// `createMockEditor(fileContents)`.
    fn create_mock_editor(&self, file_contents: &str) -> Rc<TestCodeEditor> {
        let text_model = create_text_model_with(
            file_contents,
            None,
            RelaxedTextModelCreationOptions::default(),
            Some(URI::parse("test:somefile.ttt", false)),
        );
        let owned = text_model.clone();
        self.disposables.add(to_disposable(move || owned.dispose()));
        let editor = create_test_code_editor(Some(text_model), TestCodeEditorInstantiationOptions::default());
        self.disposables.add_rc(editor.clone());
        editor
    }

    /// `disposables.add(new ParameterHintsModel(editor, registry, delay))`.
    fn create_model(&self, editor: &TestCodeEditor, delay: Option<u64>) -> Rc<ParameterHintsModel> {
        let model = ParameterHintsModel::new(editor.widget(), &self.registry, delay);
        self.disposables.add_rc(model.clone());
        model
    }

    /// `disposables.add(registry.register(mockFileSelector, provider))`.
    fn register(&self, provider: Rc<TestProvider>) {
        self.disposables.add(self.registry.register(mock_file_selector(), provider));
    }

    /// `getNextHint(model)`.
    fn get_next_hint(&self, model: &ParameterHintsModel) -> Promise<Option<Rc<SignatureHelp>>> {
        get_next_hint(&self.disposables, model)
    }
}

/// `getNextHint(model)`: a promessa da próxima mudança de dicas; o ouvinte se descarta ao receber a primeira.
fn get_next_hint(disposables: &LeakGuard, model: &ParameterHintsModel) -> Promise<Option<Rc<SignatureHelp>>> {
    let (promise, resolve) = Promise::new();
    let sub: Rc<std::cell::RefCell<Option<DisposableRef>>> = Rc::default();
    let listened_sub = sub.clone();
    let listener = model.on_changed_hints().listen(move |e| {
        if let Some(sub) = listened_sub.borrow_mut().take() {
            sub.dispose();
        }
        resolve.resolve(e.clone());
    });
    *sub.borrow_mut() = Some(disposables.add(listener));
    promise
}

/// `editor.trigger('keyboard', Handler.Type, { text })`.
fn type_text(editor: &TestCodeEditor, text: &str) {
    editor.trigger(Some("keyboard"), Handler::TYPE, Some(Dynamic::object(vec![("text", Dynamic::from(text))])));
}

// suite('ParameterHintsModel')

#[test]
fn provider_should_get_trigger_character_on_type() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let (done_promise, done) = Promise::<()>::new();

        let trigger_char = "(";

        let editor = fixture.create_mock_editor("");
        fixture.create_model(&editor, None);

        fixture.register(TestProvider::new(&[trigger_char], &[], move |context, _token| {
            assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
            assert_eq!(context.trigger_character.as_deref(), Some(trigger_char));
            done.resolve(());
            Promise::resolved(None)
        }));

        type_text(&editor, trigger_char);
        done_promise.await.expect("done");
    });
}

#[test]
fn provider_should_be_retriggered_if_already_active() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let (done_promise, done) = Promise::<()>::new();

        let trigger_char = "(";

        let editor = fixture.create_mock_editor("");
        fixture.create_model(&editor, None);

        let empty_sig_help = new_empty_sig_help();
        let invoke_count = Rc::new(Cell::new(0));
        let (provider_editor, provider_help) = (editor.clone(), empty_sig_help.clone());
        fixture.register(TestProvider::new(&[trigger_char], &[], move |context, _token| {
            invoke_count.set(invoke_count.get() + 1);
            if invoke_count.get() == 1 {
                assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                assert_eq!(context.trigger_character.as_deref(), Some(trigger_char));
                assert!(!context.is_retrigger);
                assert!(context.active_signature_help.is_none());

                // Retrigger
                let editor = provider_editor.clone();
                runtime::set_timeout(move || type_text(&editor, trigger_char), 0);
            } else {
                assert_eq!(invoke_count.get(), 2);
                assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                assert!(context.is_retrigger);
                assert_eq!(context.trigger_character.as_deref(), Some(trigger_char));
                assert!(Rc::ptr_eq(context.active_signature_help.as_ref().expect("dicas ativas"), &provider_help));

                done.resolve(());
            }
            result_of(provider_help.clone())
        }));

        type_text(&editor, trigger_char);
        done_promise.await.expect("done");
    });
}

#[test]
fn provider_should_not_be_retriggered_if_previous_help_is_canceled_first() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let (done_promise, done) = Promise::<()>::new();

        let trigger_char = "(";

        let editor = fixture.create_mock_editor("");
        let hint_model = fixture.create_model(&editor, None);

        let empty_sig_help = new_empty_sig_help();
        let invoke_count = Rc::new(Cell::new(0));
        let (provider_editor, provider_help) = (editor.clone(), empty_sig_help.clone());
        fixture.register(TestProvider::new(&[trigger_char], &[], move |context, _token| {
            invoke_count.set(invoke_count.get() + 1);
            if invoke_count.get() == 1 {
                assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                assert_eq!(context.trigger_character.as_deref(), Some(trigger_char));
                assert!(!context.is_retrigger);
                assert!(context.active_signature_help.is_none());

                // Cancel and retrigger
                hint_model.cancel(false);
                type_text(&provider_editor, trigger_char);
            } else {
                assert_eq!(invoke_count.get(), 2);
                assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                assert_eq!(context.trigger_character.as_deref(), Some(trigger_char));
                assert!(context.is_retrigger);
                assert!(context.active_signature_help.is_none());
                done.resolve(());
            }
            result_of(provider_help.clone())
        }));

        type_text(&editor, trigger_char);
        done_promise.await.expect("done");
    });
}

#[test]
fn provider_should_get_last_trigger_character_when_triggered_multiple_times_and_only_be_invoked_once() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let (done_promise, done) = Promise::<()>::new();

        let editor = fixture.create_mock_editor("");
        fixture.create_model(&editor, Some(5));

        let invoke_count = Rc::new(Cell::new(0));
        fixture.register(TestProvider::new(&["a", "b", "c"], &[], move |context, _token| {
            invoke_count.set(invoke_count.get() + 1);

            assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
            assert!(!context.is_retrigger);
            assert_eq!(context.trigger_character.as_deref(), Some("c"));

            // Give some time to allow for later triggers
            let (invoke_count, done) = (invoke_count.clone(), done.clone());
            runtime::set_timeout(
                move || {
                    assert_eq!(invoke_count.get(), 1);

                    done.resolve(());
                },
                50,
            );
            Promise::resolved(None)
        }));

        type_text(&editor, "a");
        type_text(&editor, "b");
        type_text(&editor, "c");

        done_promise.await.expect("done");
    });
}

#[test]
fn provider_should_be_retriggered_if_already_active_2() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let (done_promise, done) = Promise::<()>::new();

        let editor = fixture.create_mock_editor("");
        fixture.create_model(&editor, Some(5));

        let empty_sig_help = new_empty_sig_help();
        let invoke_count = Rc::new(Cell::new(0));
        let (provider_editor, provider_help) = (editor.clone(), empty_sig_help.clone());
        fixture.register(TestProvider::new(&["a", "b"], &[], move |context, _token| {
            invoke_count.set(invoke_count.get() + 1);
            match invoke_count.get() {
                1 => {
                    assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                    assert_eq!(context.trigger_character.as_deref(), Some("a"));

                    // retrigger after delay for widget to show up
                    let editor = provider_editor.clone();
                    runtime::set_timeout(move || type_text(&editor, "b"), 50);
                }
                2 => {
                    assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                    assert!(context.is_retrigger);
                    assert_eq!(context.trigger_character.as_deref(), Some("b"));
                    done.resolve(());
                }
                _ => panic!("Unexpected invoke"),
            }

            result_of(provider_help.clone())
        }));

        type_text(&editor, "a");
        done_promise.await.expect("done");
    });
}

#[test]
fn should_cancel_existing_request_when_new_request_comes_in() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let editor = fixture.create_mock_editor("abc def");
        let hints_model = fixture.create_model(&editor, None);

        let did_request_cancellation_of = Rc::new(Cell::new(-1));
        let invoke_count = Rc::new(Cell::new(0));
        let store = fixture.disposables.store();
        let (provider_model, provider_cancellation) = (hints_model.clone(), did_request_cancellation_of.clone());
        let long_running_provider = TestProvider::new(&[], &[], move |_context, token| {
            let count = invoke_count.get();
            invoke_count.set(count + 1);
            let cancellation = provider_cancellation.clone();
            store.add(token.on_cancellation_requested().listen(move |_| cancellation.set(count)));

            // retrigger on first request
            if count == 0 {
                provider_model.trigger(TriggerContext { trigger_kind: SignatureHelpTriggerKind::Invoke, trigger_character: None }, Some(0));
            }

            Promise::with_executor(move |resolver| {
                runtime::set_timeout(
                    move || {
                        resolver.resolve(Some(Rc::new(SignatureHelpResult::new(
                            Rc::new(SignatureHelp {
                                signatures: vec![SignatureInformation {
                                    label: count.to_string(),
                                    documentation: None,
                                    parameters: Vec::new(),
                                    active_parameter: None,
                                }],
                                active_parameter: 0,
                                active_signature: 0,
                            }),
                            || {},
                        ))));
                    },
                    100,
                );
            })
        });

        fixture.register(long_running_provider);

        hints_model.trigger(TriggerContext { trigger_kind: SignatureHelpTriggerKind::Invoke, trigger_character: None }, Some(0));
        assert_eq!(-1, did_request_cancellation_of.get());

        let (done_promise, done) = Promise::<()>::new();
        let cancellation = did_request_cancellation_of.clone();
        fixture.disposables.add(hints_model.on_changed_hints().listen(move |new_parameter_hints| {
            let outcome = errors::catch(|| {
                assert_eq!(0, cancellation.get());
                assert_eq!("1", new_parameter_hints.as_ref().expect("dicas").signatures[0].label);
            });
            match outcome {
                Ok(()) => done.resolve(()),
                Err(error) => done.reject(error),
            }
        }));
        done_promise.await.expect("done");
    });
}

#[test]
fn provider_should_be_retriggered_by_retrigger_character() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let (done_promise, done) = Promise::<()>::new();

        let trigger_char = "a";
        let retrigger_char = "b";

        let editor = fixture.create_mock_editor("");
        fixture.create_model(&editor, Some(5));

        let empty_sig_help = new_empty_sig_help();
        let invoke_count = Rc::new(Cell::new(0));
        let (provider_editor, provider_help) = (editor.clone(), empty_sig_help.clone());
        fixture.register(TestProvider::new(&[trigger_char], &[retrigger_char], move |context, _token| {
            invoke_count.set(invoke_count.get() + 1);
            match invoke_count.get() {
                1 => {
                    assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                    assert_eq!(context.trigger_character.as_deref(), Some(trigger_char));

                    // retrigger after delay for widget to show up
                    let editor = provider_editor.clone();
                    runtime::set_timeout(move || type_text(&editor, retrigger_char), 50);
                }
                2 => {
                    assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                    assert!(context.is_retrigger);
                    assert_eq!(context.trigger_character.as_deref(), Some(retrigger_char));
                    done.resolve(());
                }
                _ => panic!("Unexpected invoke"),
            }

            result_of(provider_help.clone())
        }));

        // This should not trigger anything
        type_text(&editor, retrigger_char);

        // But a trigger character should
        type_text(&editor, trigger_char);

        done_promise.await.expect("done");
    });
}

#[test]
fn should_use_first_result_from_multiple_providers() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let trigger_char = "a";
        let first_provider_id = "firstProvider";
        let second_provider_id = "secondProvider";
        let parameter_label = "parameter";

        let editor = fixture.create_mock_editor("");
        let model = fixture.create_model(&editor, Some(5));

        let provider_editor = editor.clone();
        fixture.register(TestProvider::new(&[trigger_char], &[], move |context, _token| {
            if !context.is_retrigger {
                // retrigger after delay for widget to show up
                let editor = provider_editor.clone();
                runtime::set_timeout(move || type_text(&editor, trigger_char), 50);

                return result_of(Rc::new(SignatureHelp {
                    active_parameter: 0,
                    active_signature: 0,
                    signatures: vec![SignatureInformation {
                        label: first_provider_id.to_string(),
                        documentation: None,
                        parameters: vec![ParameterInformation { label: ParameterLabel::Text(parameter_label.to_string()), documentation: None }],
                        active_parameter: None,
                    }],
                }));
            }

            Promise::resolved(None)
        }));

        fixture.register(TestProvider::new(&[trigger_char], &[], move |context, _token| {
            if context.is_retrigger {
                return result_of(Rc::new(SignatureHelp {
                    active_parameter: 0,
                    active_signature: context.active_signature_help.as_ref().map_or(0, |help| help.active_signature + 1),
                    signatures: vec![SignatureInformation {
                        label: second_provider_id.to_string(),
                        documentation: None,
                        parameters: context
                            .active_signature_help
                            .as_ref()
                            .map_or_else(Vec::new, |help| help.signatures[0].parameters.clone()),
                        active_parameter: None,
                    }],
                }));
            }

            Promise::resolved(None)
        }));

        type_text(&editor, trigger_char);

        let first_hint = fixture.get_next_hint(&model).await.expect("getNextHint").expect("dicas");
        assert_eq!(first_hint.signatures[0].label, first_provider_id);
        assert_eq!(first_hint.active_signature, 0);
        assert_eq!(first_hint.signatures[0].parameters[0].label, ParameterLabel::Text(parameter_label.to_string()));

        let second_hint = fixture.get_next_hint(&model).await.expect("getNextHint").expect("dicas");
        assert_eq!(second_hint.signatures[0].label, second_provider_id);
        assert_eq!(second_hint.active_signature, 1);
        assert_eq!(second_hint.signatures[0].parameters[0].label, ParameterLabel::Text(parameter_label.to_string()));
    });
}

#[test]
fn quick_typing_should_use_the_first_trigger_character() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let editor = fixture.create_mock_editor("");
        let model = fixture.create_model(&editor, Some(50));

        let trigger_character = "a";

        let empty_sig_help = new_empty_sig_help();
        let invoke_count = Rc::new(Cell::new(0));
        fixture.register(TestProvider::new(&[trigger_character], &[], move |context, _token| {
            invoke_count.set(invoke_count.get() + 1);

            if invoke_count.get() == 1 {
                assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                assert_eq!(context.trigger_character.as_deref(), Some(trigger_character));
            } else {
                panic!("Unexpected invoke");
            }

            result_of(empty_sig_help.clone())
        }));

        type_text(&editor, trigger_character);
        type_text(&editor, "x");

        fixture.get_next_hint(&model).await.expect("getNextHint");
    });
}

#[test]
fn retrigger_while_a_pending_resolve_is_still_going_on_should_preserve_last_active_signature_96702() {
    run_with_faked_timers(async move {
        let fixture = Fixture::new();
        let (done_promise, done) = Promise::<()>::new();

        let editor = fixture.create_mock_editor("");
        let model = fixture.create_model(&editor, Some(50));

        let trigger_character = "a";
        let retrigger_character = "b";

        let empty_sig_help = new_empty_sig_help();
        let invoke_count = Rc::new(Cell::new(0));
        let (provider_editor, provider_help) = (editor.clone(), empty_sig_help.clone());
        fixture.register(TestProvider::new(&[trigger_character], &[retrigger_character], move |context, _token| {
            invoke_count.set(invoke_count.get() + 1);

            match invoke_count.get() {
                1 => {
                    assert_eq!(context.trigger_kind, SignatureHelpTriggerKind::TriggerCharacter);
                    assert_eq!(context.trigger_character.as_deref(), Some(trigger_character));
                    let editor = provider_editor.clone();
                    runtime::set_timeout(move || type_text(&editor, retrigger_character), 50);
                }
                2 => {
                    // Trigger again while we wait for resolve to take place
                    let editor = provider_editor.clone();
                    runtime::set_timeout(move || type_text(&editor, retrigger_character), 50);
                    let help = provider_help.clone();
                    return Promise::with_executor(move |resolver| {
                        runtime::set_timeout(move || resolver.resolve(Some(Rc::new(SignatureHelpResult::new(help, || {})))), 1000);
                    });
                }
                3 => {
                    // Make sure that in a retrigger during a pending resolve, we still have the old active signature.
                    assert!(Rc::ptr_eq(context.active_signature_help.as_ref().expect("dicas ativas"), &provider_help));
                    done.resolve(());
                }
                _ => panic!("Unexpected invoke"),
            }

            result_of(provider_help.clone())
        }));

        type_text(&editor, trigger_character);

        fixture.get_next_hint(&model).await.expect("getNextHint");
        fixture.get_next_hint(&model).await.expect("getNextHint");

        done_promise.await.expect("done");
    });
}
