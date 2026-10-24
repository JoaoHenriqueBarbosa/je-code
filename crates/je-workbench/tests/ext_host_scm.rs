//! Porte de `src/vs/workbench/api/test/common/extHostSCM.test.ts` (suíte `ExtHostSCM`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado em [`run_test`]; os casos são `async` e rodam em
//!   `run_with_faked_timers`. O `ExtHostDocuments` do original é `{} as ExtHostDocuments` (nada construído); aqui é o real,
//!   criado ANTES do guarda (o `setup` do original roda antes de o rastreador existir), para os `DisposableStore` dele não
//!   contarem como vazamento. Nenhum caso o usa.
//! - `new TestRPCProtocol()` e `rpc.set(...)` são o [`TestMainContext`] de `support` (com o `sync()` dele). Os proxies
//!   respondem na própria chamada (no original a chamada remota só chega à thread principal numa macrotarefa), então
//!   `handle`, `registrations` e `updates` já estão preenchidos antes do `await rpc.sync()`; os casos mantêm o `sync` onde o
//!   original o tem.
//! - `new class extends mock<MainThreadSCMShape>() { ... }` é [`MockMainThreadSCM`]: só `$registerSourceControl`,
//!   `$unregisterSourceControl` e (onde o caso a sobrescreve) `$updateSourceControl` respondem; os demais métodos falham
//!   com `panic!`, como o método ausente do objeto vazio lançaria `TypeError`. O mesmo vale para [`MockMainThreadCommands`]
//!   (`$registerCommand` e `$unregisterCommand`) e [`MockExtHostTelemetry`].
//! - Nos dois últimos casos o original passa um `ExtHostCommands` de mentira (`mock<ExtHostCommands>()` só com
//!   `registerArgumentProcessor`); aqui é o `ExtHostCommands` real sobre o [`MockMainThreadCommands`], porque o
//!   `ExtHostSCM` guarda o conversor de comandos dele (nenhum desses casos chama o conversor).
//! - `{ ...nullExtensionDescription, enabledApiProposals: [...] }` é a `null_extension_description()` com
//!   `manifest.enabled_api_proposals`.
//! - `commands.converter.getActualCommand('test.artifact /1')` é `get_actual_command` com o texto como primeiro argumento;
//!   `commands.converter.fromInternal(dto)` é `from_internal`. O `assert.strictEqual(fromInternal(...), current[0].command)`
//!   (identidade do objeto) compara os campos do `Command`, com os argumentos por identidade ([`Dynamic::strict_equals`]).
//! - `DeferredPromise` e `result.p` são o [`DeferredPromise`] e o `promise` dele; o `provideArtifacts` que devolve a promessa de
//!   `SourceControlArtifact[]` devolve `Option<Vec<_>>` (`ProviderResult`), e o `error === expected` de `assert.rejects` compara
//!   os erros por valor. O `try/finally` do primeiro caso, que chama o `dispose` também ao falhar, vira o `dispose` ao fim: uma
//!   asserção que falha derruba o caso de qualquer forma, e o guarda descarta a fonte.
//! - O objeto observado no último caso (`{ activeRepositoryName, updates }`) é uma tupla: o `null` de `activeRepositoryName` é
//!   `Some(None)` no [`SCMProviderFeatures`], e `registrations.indexOf(handle)` é a posição (`Option<usize>`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, LeakGuard};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::path::Platform;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, CreateDocumentOptions, ExtHostSCMShape, ICommandDto, IExtHostTelemetry, IMainContext, MainContext, MainThreadCommandsShape,
    MainThreadDocumentsShape, MainThreadSCMShape, MainThreadTelemetryShape, OpenDocumentOptions, SCMGroupFeatures, SCMGroupRegistration,
    SCMHistoryItemRefDto, SCMHistoryItemRefsChangeEventDto, SCMProviderFeatures, SCMRawResourceSplices,
};
use je_workbench::api::common::ext_host_protocol::{IconPathDto, InputValidationType};
use je_workbench::api::common::ext_host_scm::{ExtHostSCM, ExtHostSourceControl, SourceControlArtifact, SourceControlArtifactGroup, SourceControlArtifactProvider};
use je_workbench::api::common::ext_host_type_converters::ICommandsConverter;
use je_workbench::api::common::ext_host_types::Command;
use je_workbench::services::extensions::extensions::null_extension_description;
use je_extensions::scanner::ExtensionDescription;
use je_editor::languages::StringOrMarkdown;
use support::ext_host_workspace_support::{MockMainThreadTelemetry, TestMainContext};

// ---- os dublês --------------------------------------------------------------------------------------

/// `new class extends mock<MainThreadSCMShape>() { ... }`: `$registerSourceControl` e `$unregisterSourceControl` sempre; o
/// `$updateSourceControl` só quando o caso o sobrescreve (`overrides_update`).
struct MockMainThreadSCM {
    overrides_update: bool,
    registrations: RefCell<Vec<u32>>,
    updates: RefCell<Vec<(u32, SCMProviderFeatures)>>,
}

impl IDisposable for MockMainThreadSCM {
    fn dispose(&self) {}
}

impl MainThreadSCMShape for MockMainThreadSCM {
    fn rpc_register_source_control(
        &self,
        handle: u32,
        _parent_handle: Option<u32>,
        _id: &str,
        _label: &str,
        _root_uri: Option<URI>,
        _icon_path: Option<IconPathDto>,
        _is_hidden: Option<bool>,
        _input_box_document_uri: URI,
    ) -> Promise<()> {
        self.registrations.borrow_mut().push(handle);
        Promise::resolved(())
    }

    fn rpc_update_source_control(&self, handle: u32, features: SCMProviderFeatures) -> Promise<()> {
        if !self.overrides_update {
            panic!("$updateSourceControl is not a function");
        }
        self.updates.borrow_mut().push((handle, features));
        Promise::resolved(())
    }

    fn rpc_unregister_source_control(&self, _handle: u32) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_register_groups(&self, _source_control_handle: u32, _groups: Vec<SCMGroupRegistration>, _splices: Vec<SCMRawResourceSplices>) -> Promise<()> {
        panic!("$registerGroups is not a function")
    }

    fn rpc_update_group(&self, _source_control_handle: u32, _handle: u32, _features: SCMGroupFeatures) -> Promise<()> {
        panic!("$updateGroup is not a function")
    }

    fn rpc_update_group_label(&self, _source_control_handle: u32, _handle: u32, _label: &str) -> Promise<()> {
        panic!("$updateGroupLabel is not a function")
    }

    fn rpc_unregister_group(&self, _source_control_handle: u32, _handle: u32) -> Promise<()> {
        panic!("$unregisterGroup is not a function")
    }

    fn rpc_splice_resource_states(&self, _source_control_handle: u32, _splices: Vec<SCMRawResourceSplices>) -> Promise<()> {
        panic!("$spliceResourceStates is not a function")
    }

    fn rpc_set_input_box_value(&self, _source_control_handle: u32, _value: &str) -> Promise<()> {
        panic!("$setInputBoxValue is not a function")
    }

    fn rpc_set_input_box_placeholder(&self, _source_control_handle: u32, _placeholder: &str) -> Promise<()> {
        panic!("$setInputBoxPlaceholder is not a function")
    }

    fn rpc_set_input_box_enablement(&self, _source_control_handle: u32, _enabled: bool) -> Promise<()> {
        panic!("$setInputBoxEnablement is not a function")
    }

    fn rpc_set_input_box_visibility(&self, _source_control_handle: u32, _visible: bool) -> Promise<()> {
        panic!("$setInputBoxVisibility is not a function")
    }

    fn rpc_show_validation_message(&self, _source_control_handle: u32, _message: StringOrMarkdown, _validation_type: InputValidationType) -> Promise<()> {
        panic!("$showValidationMessage is not a function")
    }

    fn rpc_set_validation_provider_is_enabled(&self, _source_control_handle: u32, _enabled: bool) -> Promise<()> {
        panic!("$setValidationProviderIsEnabled is not a function")
    }

    fn rpc_on_did_change_history_provider_current_history_item_refs(
        &self,
        _source_control_handle: u32,
        _history_item_ref: Option<SCMHistoryItemRefDto>,
        _history_item_remote_ref: Option<SCMHistoryItemRefDto>,
        _history_item_base_ref: Option<SCMHistoryItemRefDto>,
    ) -> Promise<()> {
        panic!("$onDidChangeHistoryProviderCurrentHistoryItemRefs is not a function")
    }

    fn rpc_on_did_change_history_provider_history_item_refs(&self, _source_control_handle: u32, _history_item_refs: SCMHistoryItemRefsChangeEventDto) -> Promise<()> {
        panic!("$onDidChangeHistoryProviderHistoryItemRefs is not a function")
    }

    fn rpc_on_did_change_artifacts(&self, _source_control_handle: u32, _groups: Vec<String>) -> Promise<()> {
        panic!("$onDidChangeArtifacts is not a function")
    }
}

/// `new class extends mock<MainThreadCommandsShape>() { override async $registerCommand(): Promise<void> { } }` (e o
/// `$unregisterCommand`, que o `dispose` de um comando chama).
struct MockMainThreadCommands;

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
    fn rpc_register_command(&self, _id: &str) {}

    fn rpc_unregister_command(&self, _id: &str) {}

    fn rpc_fire_command_activation_event(&self, _id: &str) {
        panic!("$fireCommandActivationEvent is not a function")
    }

    fn rpc_execute_command(&self, _id: &str, _args: CommandArgs, _retry: bool) -> Promise<Dynamic> {
        panic!("$executeCommand is not a function")
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        panic!("$getCommands is not a function")
    }
}

/// O `undefined!` que o original passa como telemetria do `ExtHostCommands`: nenhum caso chega a `onExtensionError`.
struct MockExtHostTelemetry;

impl IExtHostTelemetry for MockExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        panic!("onExtensionError is not a function")
    }
}

/// O `MainThreadDocuments` que o `ExtHostDocuments` real pede e que nenhum caso chama.
struct UnusedMainThreadDocuments;

impl IDisposable for UnusedMainThreadDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for UnusedMainThreadDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("o proxy não deveria ser chamado");
    }
}

impl MainThreadDocumentsShape for UnusedMainThreadDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }
}

// ---- o ambiente de cada caso -----------------------------------------------------------------------

/// O que o `setup` de cada caso monta.
struct Env {
    rpc: Rc<TestMainContext>,
    commands: Rc<ExtHostCommands>,
    scm: Rc<ExtHostSCM>,
    main_thread_scm: Rc<MockMainThreadSCM>,
    /// O `store` da suíte.
    guard: Rc<LeakGuard>,
}

/// O corpo de um caso: o `setup`, o caso e o `teardown`.
fn run_test<Fut: Future<Output = ()> + 'static>(overrides_update: bool, body: impl FnOnce(Env) -> Fut + 'static) {
    let rpc = Rc::new(TestMainContext::new());
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, Rc::new(UnusedMainThreadDocuments) as Rc<dyn MainThreadDocumentsShape>);
    let context: Rc<dyn IMainContext> = rpc.clone();
    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(context, NullLogService::shared()));
    let documents = ExtHostDocuments::new(&*rpc, documents_and_editors);

    let guard = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());

    let main_thread_scm = Rc::new(MockMainThreadSCM { overrides_update, registrations: RefCell::new(Vec::new()), updates: RefCell::new(Vec::new()) });
    rpc.set(&MainContext::MAIN_THREAD_SCM, main_thread_scm.clone() as Rc<dyn MainThreadSCMShape>);
    rpc.set(&MainContext::MAIN_THREAD_TELEMETRY, Rc::new(MockMainThreadTelemetry) as Rc<dyn MainThreadTelemetryShape>);
    rpc.set(&MainContext::MAIN_THREAD_COMMANDS, Rc::new(MockMainThreadCommands) as Rc<dyn MainThreadCommandsShape>);
    let commands = ExtHostCommands::new(&*rpc, NullLogService::shared(), Rc::new(MockExtHostTelemetry));
    let scm = ExtHostSCM::new(&*rpc, commands.clone(), documents, NullLogService::shared());

    let env = Env { rpc, commands, scm, main_thread_scm, guard: guard.clone() };
    run_with_faked_timers(async move {
        body(env).await;
    });
}

impl Env {
    /// `rpc.sync()`.
    async fn sync(&self) {
        self.rpc.sync().await.expect("sync");
    }
}

/// O que `createArtifactSourceControl(provider)` devolve (sem o `rpc`, que é o do [`Env`]).
struct ArtifactSourceControl {
    source_control: Rc<ExtHostSourceControl>,
    handle: u32,
}

/// `createArtifactSourceControl(provider)`.
async fn create_artifact_source_control(env: &Env, provider: Rc<dyn SourceControlArtifactProvider>) -> ArtifactSourceControl {
    let mut extension = null_extension_description();
    extension.manifest.enabled_api_proposals = Some(vec!["scmArtifactProvider".to_owned()]);
    let source_control = env.guard.add_rc(env.scm.create_source_control(&extension, "test", "Test", None, None, None, None));
    source_control.set_artifact_provider(Some(provider)).expect("scmArtifactProvider");
    env.sync().await;
    // `assert.notStrictEqual(handle, -1)`: o `$registerSourceControl` registrou a fonte.
    let handle = env.main_thread_scm.registrations.borrow().last().copied().expect("handle");
    ArtifactSourceControl { source_control, handle }
}

type ProvideArtifacts = Box<dyn Fn(&str, &CancellationToken) -> MaybePromise<Option<Vec<SourceControlArtifact>>>>;

/// `{ onDidChangeArtifacts: Event.None, provideArtifactGroups: () => [], provideArtifacts }`.
struct TestArtifactProvider {
    provide_artifacts: ProvideArtifacts,
}

impl SourceControlArtifactProvider for TestArtifactProvider {
    fn on_did_change_artifacts(&self) -> Event<Vec<String>> {
        Event::none()
    }

    fn provide_artifact_groups(&self, _token: &CancellationToken) -> MaybePromise<Option<Vec<SourceControlArtifactGroup>>> {
        MaybePromise::Value(Some(Vec::new()))
    }

    fn provide_artifacts(&self, group: &str, token: &CancellationToken) -> MaybePromise<Option<Vec<SourceControlArtifact>>> {
        (self.provide_artifacts)(group, token)
    }
}

/// `artifactProvider(provideArtifacts)`.
fn artifact_provider(provide_artifacts: impl Fn(&str, &CancellationToken) -> MaybePromise<Option<Vec<SourceControlArtifact>>> + 'static) -> Rc<dyn SourceControlArtifactProvider> {
    Rc::new(TestArtifactProvider { provide_artifacts: Box::new(provide_artifacts) })
}

/// `artifacts(id)`.
fn artifacts(id: &str) -> Vec<SourceControlArtifact> {
    vec![SourceControlArtifact {
        id: id.to_owned(),
        name: id.to_owned(),
        description: None,
        icon: None,
        timestamp: None,
        command: Some(Command {
            title: "Open".to_owned(),
            command: "test.artifact".to_owned(),
            tooltip: None,
            arguments: Some(vec![Dynamic::object([("id", Dynamic::string(id))])]),
        }),
    }]
}

/// `commands.converter.getActualCommand('test.artifact /1')`.
fn actual_command(env: &Env, id: &str) -> Option<Command> {
    env.commands.converter().get_actual_command(&[Dynamic::string(id)])
}

/// `commands.converter.fromInternal(command)`.
fn from_internal(env: &Env, command: &ICommandDto) -> Option<Command> {
    env.commands.converter().from_internal(command)
}

/// O `assert.strictEqual(a, b)` entre dois `Command` (a identidade do original: os campos, e os argumentos por identidade).
#[track_caller]
fn assert_same_command(actual: &Command, expected: &Command) {
    assert_eq!(actual.command, expected.command);
    assert_eq!(actual.title, expected.title);
    assert_eq!(actual.tooltip, expected.tooltip);
    let (actual_arguments, expected_arguments) = (actual.arguments.as_deref().unwrap_or(&[]), expected.arguments.as_deref().unwrap_or(&[]));
    assert_eq!(actual_arguments.len(), expected_arguments.len());
    for (actual_argument, expected_argument) in actual_arguments.iter().zip(expected_arguments) {
        assert!(actual_argument.strict_equals(expected_argument), "os argumentos do comando não são o mesmo objeto");
    }
}

/// `{ ...nullExtensionDescription, identifier: new ExtensionIdentifier('vscode.git'), name: 'git', displayName: 'Git',
/// extensionLocation: URI.file('/extension'), isBuiltin: true }`.
fn git_extension() -> ExtensionDescription {
    let mut extension = null_extension_description();
    extension.id = "vscode.git".to_owned();
    extension.identifier = ExtensionIdentifier::new("vscode.git");
    extension.manifest.name = "git".to_owned();
    extension.manifest.display_name = Some("Git".to_owned());
    extension.extension_location = uri("/extension").to_string(false).to_string_lossy();
    extension.is_builtin = true;
    extension
}

/// `URI.file(path)`.
fn uri(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

// ---- os casos --------------------------------------------------------------------------------------

#[test]
fn does_not_cache_artifact_commands_returned_after_source_control_disposal() {
    run_test(true, |env| async move {
        let result = Rc::new(DeferredPromise::<Option<Vec<SourceControlArtifact>>>::new());
        let provider_result = result.clone();
        let ArtifactSourceControl { source_control, handle } =
            create_artifact_source_control(&env, artifact_provider(move |_, _| MaybePromise::Promise(provider_result.promise.clone()))).await;
        let pending = env.scm.rpc_provide_artifacts(handle, "group", CancellationToken::None);
        source_control.dispose();
        result.complete(Some(artifacts("retired"))).await.expect("complete");
        assert!(pending.await.expect("pending").is_none());
        assert!(actual_command(&env, "test.artifact /1").is_none());
        // Also clean up late stores on the unfixed implementation during the red run.
        source_control.dispose();
    });
}

#[test]
fn does_not_replace_live_artifact_commands_with_a_retired_provider_result() {
    run_test(true, |env| async move {
        let result = Rc::new(DeferredPromise::<Option<Vec<SourceControlArtifact>>>::new());
        let provider_result = result.clone();
        let ArtifactSourceControl { source_control, handle } =
            create_artifact_source_control(&env, artifact_provider(move |_, _| MaybePromise::Promise(provider_result.promise.clone()))).await;
        let pending = env.scm.rpc_provide_artifacts(handle, "group", CancellationToken::None);
        let current = artifacts("current");
        let live_provider_artifacts = current.clone();
        source_control
            .set_artifact_provider(Some(artifact_provider(move |_, _| MaybePromise::Value(Some(live_provider_artifacts.clone())))))
            .expect("scmArtifactProvider");
        let live_result = env.scm.rpc_provide_artifacts(handle, "group", CancellationToken::None).await.expect("liveResult");
        result.complete(Some(artifacts("retired"))).await.expect("complete");
        assert!(pending.await.expect("pending").is_none());
        let command = live_result.as_ref().and_then(|live_result| live_result[0].command.clone()).expect("liveResult command");
        assert_same_command(&from_internal(&env, &command).expect("fromInternal"), current[0].command.as_ref().expect("command"));
    });
}

#[test]
fn does_not_cache_artifact_commands_after_request_cancellation() {
    run_test(true, |env| async move {
        let result = Rc::new(DeferredPromise::<Option<Vec<SourceControlArtifact>>>::new());
        let provider_result = result.clone();
        let ArtifactSourceControl { handle, .. } =
            create_artifact_source_control(&env, artifact_provider(move |_, _| MaybePromise::Promise(provider_result.promise.clone()))).await;
        let cts = CancellationTokenSource::new(None);
        env.guard.add_rc(Rc::new(cts.clone()));
        let pending = env.scm.rpc_provide_artifacts(handle, "group", cts.token());
        cts.cancel();
        result.complete(Some(artifacts("cancelled"))).await.expect("complete");
        assert!(pending.await.expect("pending").is_none());
        assert!(actual_command(&env, "test.artifact /1").is_none());
    });
}

#[test]
fn does_not_allocate_an_orphan_command_store_when_artifact_requests_reject() {
    run_test(true, |env| async move {
        let expected = Error::new("Expected artifact provider rejection");
        let rejection = expected.clone();
        let ArtifactSourceControl { handle, .. } =
            create_artifact_source_control(&env, artifact_provider(move |_, _| MaybePromise::Promise(Promise::rejected(rejection.clone())))).await;
        let error = env.scm.rpc_provide_artifacts(handle, "group", CancellationToken::None).await.expect_err("rejects");
        assert_eq!(error, expected);
    });
}

#[test]
fn releases_replaced_artifact_commands_and_retains_commands_from_other_groups() {
    run_test(true, |env| async move {
        let current = Rc::new(RefCell::new(artifacts("first")));
        let provider_current = current.clone();
        let ArtifactSourceControl { source_control, handle } =
            create_artifact_source_control(&env, artifact_provider(move |_, _| MaybePromise::Value(Some(provider_current.borrow().clone())))).await;
        let first = env.scm.rpc_provide_artifacts(handle, "first", CancellationToken::None).await.expect("first");
        let second = env.scm.rpc_provide_artifacts(handle, "second", CancellationToken::None).await.expect("second");
        let first_command = first.as_ref().and_then(|first| first[0].command.clone()).expect("first command");
        let second_command = second.as_ref().and_then(|second| second[0].command.clone()).expect("second command");
        *current.borrow_mut() = artifacts("replacement");
        let replacement = env.scm.rpc_provide_artifacts(handle, "first", CancellationToken::None).await.expect("replacement");
        let replacement_command = replacement.as_ref().and_then(|replacement| replacement[0].command.clone()).expect("replacement command");
        assert!(from_internal(&env, &first_command).is_none());
        assert!(from_internal(&env, &second_command).is_some());
        assert_same_command(&from_internal(&env, &replacement_command).expect("fromInternal"), current.borrow()[0].command.as_ref().expect("command"));
        source_control.dispose();
        assert!(from_internal(&env, &second_command).is_none());
        assert!(from_internal(&env, &replacement_command).is_none());
    });
}

#[test]
fn disposed_source_controls_are_removed_from_extension_bookkeeping() {
    run_test(false, |env| async move {
        let extension = git_extension();

        let source_control = env.scm.create_source_control(&extension, "git", "Git", Some(uri("/repo")), None, None, None);
        assert!(env.scm.get_last_input_box(&extension).is_some());

        source_control.dispose();

        assert!(env.scm.get_last_input_box(&extension).is_none());
    });
}

#[test]
fn active_repository_name_is_forwarded_to_the_main_thread() {
    run_test(true, |env| async move {
        let mut extension = git_extension();
        extension.manifest.enabled_api_proposals = Some(vec!["scmProviderOptions".to_owned()]);

        let source_control = env.scm.create_source_control(&extension, "worktree", "Git", Some(uri("/worktree")), None, None, None);
        source_control.set_active_repository_name(Some("repo".to_owned())).expect("scmProviderOptions");
        source_control.set_active_repository_name(Some("repo".to_owned())).expect("scmProviderOptions");
        source_control.set_active_repository_name(None).expect("scmProviderOptions");
        env.sync().await;

        let registrations = env.main_thread_scm.registrations.borrow().clone();
        let updates: Vec<(Option<usize>, Option<Option<String>>)> = env
            .main_thread_scm
            .updates
            .borrow()
            .iter()
            .map(|(handle, features)| (registrations.iter().position(|registration| registration == handle), features.active_repository_name.clone()))
            .collect();

        assert_eq!(source_control.active_repository_name().expect("scmProviderOptions"), None);
        assert_eq!(updates, vec![(Some(0), Some(Some("repo".to_owned()))), (Some(0), Some(None))]);

        source_control.dispose();
        env.sync().await;
    });
}
