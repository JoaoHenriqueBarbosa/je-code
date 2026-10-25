//! Porte de `src/vs/workbench/contrib/format/test/browser/formatActionsMultiple.test.ts` (suíte `Format Actions Multiple`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o [`LeakGuard`] criado no começo do caso. O `import
//!   '../../browser/formatActionsMultiple.js'` (que registra as ações ao carregar o módulo) é o `register()` do módulo, chamado
//!   antes do guarda (os registros globais não são descartados ao fim do caso).
//! - `new ServiceCollection([IContextKeyService, ...], [IAccessibilitySignalService, mock], [IEditorCancellationTokens, ...])` é
//!   a `ServiceCollection` que `with_async_test_code_editor` recebe. O `MockContextKeyService` com `contextMatchesRules` sempre
//!   verdadeiro é o [`MatchingContextKeyService`], que delega ao mock e sobrescreve só esse método. O
//!   `{ add: () => () => { } }` do `IEditorCancellationTokens` é o `EditorCancellationTokens` real sobre esse serviço de chaves.
//! - O `IAccessibilitySignalService` é o dublê [`UnusedAccessibilitySignalService`] (o `mock<>()` sem métodos: tocar um sinal
//!   falha). O formato com edições vazias não toca sinal.
//! - O `IEditorWorkerService`, o `ILogService` e o `ILanguageFeaturesService` que `formatDocumentWithProvider` lê do accessor
//!   são o [`EchoEditorWorkerService`] (devolve as edições recebidas, que é o que o worker faz com edições vazias), o
//!   `NullLogService` e uma `LanguageFeaturesService` nova, compartilhada com o corpo do caso. O `ICodeEditorService` que o
//!   comando consulta para achar o editor é o [`FocusedEditorLookup`] (o recorte `ICodeEditorLookup` do porte, ver
//!   `editor_extensions.rs`), que devolve o editor do caso.
//! - `CommandsRegistry.getCommand(id).handler` chamado em `invokeFunction` é o `handler` do comando registrado dentro de
//!   `invoke_function`. O handler do porte devolve `Dynamic` e descarta a promessa da ação, então o caso espera o `timeout` do
//!   relógio virtual para o formato terminar antes de descartar o editor (o `await` do original).
//! - `createProvider(extensionId)` é o [`RecordingProvider`], que anota o `extensionId` ao ser chamado e devolve `[]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

/// O suporte de editor do `je-editor/tests/support` que o `with_async_test_code_editor` precisa, no mesmo módulo pai (os arquivos
/// se referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use editor_support::test_code_editor::{TestCodeEditorInstantiationOptions, with_async_test_code_editor};
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit;
use je_editor::code_action::platform::{AccessibilitySignal, I_ACCESSIBILITY_SIGNAL_SERVICE, IAccessibilitySignalService};
use je_editor::code_editor_widget::CodeEditorWidget;
use je_editor::editor_extensions::{ICodeEditorLookup, I_CODE_EDITOR_LOOKUP};
use je_editor::editor_worker::{I_EDITOR_WORKER_SERVICE, IEditorWorkerService, WordRanges};
use je_editor::keybinding_cancellation::EditorCancellationTokens;
use je_editor::language_features_service::{DocumentFormattingEditProvider, FormattingOptions, LanguageFeaturesService};
use je_editor::language_providers::ProviderResult;
use je_editor::language_selector::LanguageSelector;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_extensions::manifest::ExtensionIdentifier;
use je_instantiation::ServiceCollection;
use je_platform::commands::commands::commands_registry;
use je_platform::contextkey::{
    ContextKeyExpression, I_CONTEXT_KEY_SERVICE, IContext, IContextKey, IContextKeyChangeEvent, IContextKeyService,
    IContextKeyServiceTarget, IScopedContextKeyService,
};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_workbench::api::common::editor_execute_commands::I_LANGUAGE_FEATURES_SERVICE;
use je_workbench::contrib::format::format_actions_multiple::{I_EDITOR_CANCELLATION_TOKENS, register};

/// `new class extends MockContextKeyService { override contextMatchesRules(): boolean { return true; } }`.
struct MatchingContextKeyService {
    inner: Rc<MockContextKeyService>,
}

impl IContextKeyService for MatchingContextKeyService {
    fn on_did_change_context(&self) -> Event<Rc<dyn IContextKeyChangeEvent>> {
        self.inner.on_did_change_context()
    }

    fn buffer_change_events(&self, callback: &dyn Fn()) {
        self.inner.buffer_change_events(callback)
    }

    fn create_key(&self, key: &str, default_value: Dynamic) -> Rc<dyn IContextKey> {
        self.inner.create_key(key, default_value)
    }

    fn context_matches_rules(&self, _rules: Option<&ContextKeyExpression>) -> bool {
        true
    }

    fn get_context_key_value(&self, key: &str) -> Dynamic {
        self.inner.get_context_key_value(key)
    }

    fn create_scoped(&self, target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService> {
        self.inner.create_scoped(target)
    }

    fn create_overlay(&self, overlay: Vec<(String, Dynamic)>) -> Rc<dyn IContextKeyService> {
        self.inner.create_overlay(overlay)
    }

    fn get_context(&self, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> Rc<dyn IContext> {
        self.inner.get_context(target)
    }

    fn update_parent(&self, parent_context_key_service: Rc<dyn IContextKeyService>) {
        self.inner.update_parent(parent_context_key_service)
    }

    fn context_id(&self) -> i64 {
        self.inner.context_id()
    }

    fn get_context_values_container(&self, context_id: i64) -> Rc<dyn IContext> {
        self.inner.get_context_values_container(context_id)
    }
}

/// `new class extends mock<IAccessibilitySignalService>() { }`.
struct UnusedAccessibilitySignalService;

impl IAccessibilitySignalService for UnusedAccessibilitySignalService {
    fn play_signal(&self, _signal: AccessibilitySignal) {
        panic!("playSignal is not a function")
    }
}

/// O `IEditorWorkerService` que devolve as edições recebidas.
struct EchoEditorWorkerService;

impl IEditorWorkerService for EchoEditorWorkerService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        unreachable!("IEditorWorkerService.canComputeWordRanges não é usado por este teste")
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        unreachable!("IEditorWorkerService.computeWordRanges não é usado por este teste")
    }

    fn compute_more_minimal_edits(&self, _resource: &URI, edits: Option<Vec<TextEdit>>) -> Promise<Option<Vec<TextEdit>>> {
        Promise::resolved(edits)
    }
}

/// O `ICodeEditorService` que acha o editor com foco: o do caso.
struct FocusedEditorLookup {
    editor: Rc<CodeEditorWidget>,
}

impl ICodeEditorLookup for FocusedEditorLookup {
    fn get_focused_code_editor(&self) -> Option<Rc<CodeEditorWidget>> {
        Some(Rc::clone(&self.editor))
    }

    fn get_active_code_editor(&self) -> Option<Rc<CodeEditorWidget>> {
        Some(Rc::clone(&self.editor))
    }
}

/// `createProvider(extensionId)`: o `DocumentFormattingEditProvider` que anota o `extensionId` e devolve `[]`.
struct RecordingProvider {
    extension_id: ExtensionIdentifier,
    invoked_formatters: Rc<RefCell<Vec<String>>>,
}

impl DocumentFormattingEditProvider for RecordingProvider {
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        Some(&self.extension_id)
    }

    fn provide_document_formatting_edits(
        &self,
        _model: &Rc<TextModel>,
        _options: &FormattingOptions,
        _token: &je_base::cancellation::CancellationToken,
    ) -> ProviderResult<Vec<TextEdit>> {
        self.invoked_formatters.borrow_mut().push(self.extension_id.value.clone());
        Promise::resolved(Some(Vec::new()))
    }
}

#[test]
fn selects_formatter_argument_case_insensitively() {
    // O `import '../../browser/formatActionsMultiple.js'`.
    register();

    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let store = ds.store();
    let invoked_formatters: Rc<RefCell<Vec<String>>> = Rc::default();

    let context_key_service: Rc<dyn IContextKeyService> = Rc::new(MatchingContextKeyService { inner: MockContextKeyService::new() });
    let cancellation_context_key_service = Rc::clone(&context_key_service);
    let language_features_service = Rc::new(LanguageFeaturesService::new());

    let service_collection = ServiceCollection::new();
    service_collection.set(I_CONTEXT_KEY_SERVICE, context_key_service);
    let accessibility_signal_service: Rc<dyn IAccessibilitySignalService> = Rc::new(UnusedAccessibilitySignalService);
    service_collection.set(I_ACCESSIBILITY_SIGNAL_SERVICE, accessibility_signal_service);
    service_collection.set(I_EDITOR_CANCELLATION_TOKENS, EditorCancellationTokens::new(Box::new(move |_editor| Rc::clone(&cancellation_context_key_service))));
    let editor_worker_service: Rc<dyn IEditorWorkerService> = Rc::new(EchoEditorWorkerService);
    service_collection.set(I_EDITOR_WORKER_SERVICE, editor_worker_service);
    service_collection.set(I_LOG_SERVICE, NullLogService::shared());
    service_collection.set(I_LANGUAGE_FEATURES_SERVICE, Rc::clone(&language_features_service));

    let body_invoked_formatters = Rc::clone(&invoked_formatters);
    run_with_faked_timers(async move {
        let options = TestCodeEditorInstantiationOptions {
            options: Dynamic::object(std::iter::empty::<(&str, Dynamic)>()),
            service_collection: Some(service_collection),
        };
        with_async_test_code_editor("const value = 1;", options, move |editor, _view_model, instantiation_service| {
            async_fn(async move {
                let create_provider = |extension_id: &str| -> Rc<RecordingProvider> {
                    Rc::new(RecordingProvider {
                        extension_id: ExtensionIdentifier::new(extension_id),
                        invoked_formatters: Rc::clone(&body_invoked_formatters),
                    })
                };

                let lookup: Rc<dyn ICodeEditorLookup> = Rc::new(FocusedEditorLookup { editor: Rc::clone(editor.widget()) });
                instantiation_service.stub(I_CODE_EDITOR_LOOKUP, lookup);

                store.add(language_features_service.document_formatting_edit_provider.register(LanguageSelector::Id("*".to_owned()), create_provider("publisher.first")));
                store.add(language_features_service.document_formatting_edit_provider.register(LanguageSelector::Id("*".to_owned()), create_provider("publisher.requested")));

                let command = commands_registry().get_command("editor.action.formatDocument.multiple").expect("o comando foi registrado");

                let handler = command.handler.clone();
                let args = Dynamic::object([("formatter", Dynamic::string("PUBLISHER.REQUESTED"))]);
                instantiation_service.invoke_function(|accessor| handler(accessor, &[args]));
                timeout(1).await?;
                Ok(())
            })
        })
        .await
        .expect("o caso termina sem erro");
    });

    assert_eq!(*invoked_formatters.borrow(), vec!["publisher.requested".to_owned()]);
}
