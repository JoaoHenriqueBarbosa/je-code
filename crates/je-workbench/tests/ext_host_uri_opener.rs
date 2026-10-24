//! Porte de `src/vs/workbench/api/test/browser/extHostUriOpener.test.ts` (suíte `ExtHostUriOpeners`), caso a caso e na mesma ordem.
//!
//! PORTADOS: 3 dos 3 casos do original, contando cada passada do laço `for (const openToSide of [true, false, undefined])`.
//!
//! FORA: nenhum.
//!
//! Notas de tradução:
//! - O `setup` monta a cadeia `OpenerService` -> `ExternalUriOpenerService` -> `MainThreadUriOpeners` -> `ExtHostUriOpeners` sobre
//!   um [`TestRpcProtocol`] (`support/test_rpc_protocol_deferred.rs`). Os dois lados se pedem o proxy no construtor antes de o outro
//!   existir, então os proxies de [`MainThreadUriOpenersShape`] e [`ExtHostUriOpenersShape`] são os de [`MainThreadProxy`] e
//!   [`ExtHostProxy`], que encaminham cada método por `remote_call` e procuram o objeto local só quando a chamada roda (o
//!   `Proxy` do original, que faz isso para qualquer `$método`). `await rpc.sync()` é o `sync()` do protocolo de teste.
//! - `new class extends mock<IPreferencesService>() { }` e `new class extends mock<IQuickInputService>() { }` são o
//!   [`MockPreferencesService`] (nenhum método existe) e o `TestQuickInputService` do workbench (todo método falha com
//!   `panic!`, que é o `TypeError` do método ausente do `mock<T>()`); nenhum dos dois é chamado pelos casos.
//! - `TestConfigurationService` é o da plataforma sem configuração; `TestStorageService` é o `InMemoryStorageService`;
//!   `NullLogService` é o `NullLogService::shared()`; `NullCommandService` e `TestCodeEditorService` são os de
//!   `je-editor/tests/support`. O emissor de mudanças do `TestConfigurationService` entra no `store` (o original não o
//!   descarta, mas o rastreador de vazamentos do porte o conta).
//! - O opener da extensão é o [`TestOpener`]: `canOpenExternalUri: () => Preferred` e
//!   `openExternalUri: (uri, context) => { opened.push({ uri, context }); }`. O `openToSide` `undefined` é `None`, e
//!   `viewColumn: openToSide ? ViewColumn.Beside : undefined` é `Some(ViewColumn::BESIDE)` ou `None`.
//! - Os `async` do original são [`run_with_faked_timers`] e `.await` das promessas do porte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

#[path = "support/test_rpc_protocol_deferred.rs"]
mod test_rpc_protocol_deferred;
/// O suporte de editor do `je-editor/tests/support` que o `OpenerService` do teste precisa, no mesmo módulo pai (os arquivos se
/// referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod null_command_service;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
}

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use editor_support::null_command_service::NullCommandService;
use editor_support::test_code_editor_service::TestCodeEditorService;
use editor_support::test_color_theme_service::TestThemeService;
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::opener_service::OpenerService;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::notification::INotificationService;
use je_platform::opener::{AllowContributedOpeners, IExternalUriResolver, IOpenerService, OpenOptions, OpenResource, ResolvedExternalUri};
use je_platform::quickinput::quick_input::IQuickInputService;
use je_platform::storage::StorageService;
use je_workbench::api::browser::main_thread_uri_openers::MainThreadUriOpeners;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostUriOpenersShape, MainContext, MainThreadUriOpenersShape, OpenUriContextDto,
};
use je_workbench::api::common::ext_host_types::{ExternalUriOpenerPriority, ViewColumn};
use je_workbench::api::common::ext_host_uri_opener::{
    ApiExternalUriOpener, ApiExternalUriOpenerMetadata, ApiOpenExternalUriContext, ExtHostUriOpeners,
};
use je_workbench::contrib::external_uri_opener::common::external_uri_opener_service::{ExternalUriOpenerService, IExternalUriOpenerService};
use je_workbench::services::extensions::extensions::IExtensionService;
use je_workbench::services::preferences::preferences::{IOpenSettingsOptions, IPreferencesService};
use je_workbench::test::browser::workbench_test_services::{TestExtensionService, TestQuickInputService};
use je_workbench::test::common::workbench_test_services::{TestNotificationService, TestStorageService};
use test_rpc_protocol_deferred::TestRpcProtocol;

/// O proxy de `MainContext.MainThreadUriOpeners`: cada método vira uma chamada remota no objeto que `set` registrou.
struct MainThreadProxy {
    rpc: Rc<TestRpcProtocol>,
}

impl IDisposable for MainThreadProxy {
    fn dispose(&self) {}
}

impl MainThreadUriOpenersShape for MainThreadProxy {
    fn rpc_register_uri_opener(&self, id: &str, schemes: &[String], extension_id: &ExtensionIdentifier, label: &str) -> Promise<()> {
        let rpc = Rc::clone(&self.rpc);
        let (id, schemes, extension_id, label) = (id.to_owned(), schemes.to_vec(), extension_id.clone(), label.to_owned());
        self.rpc.remote_call(move || rpc.local(&MainContext::MAIN_THREAD_URI_OPENERS).rpc_register_uri_opener(&id, &schemes, &extension_id, &label))
    }

    fn rpc_unregister_uri_opener(&self, id: &str) -> Promise<()> {
        let rpc = Rc::clone(&self.rpc);
        let id = id.to_owned();
        self.rpc.remote_call(move || rpc.local(&MainContext::MAIN_THREAD_URI_OPENERS).rpc_unregister_uri_opener(&id))
    }
}

/// O proxy de `ExtHostContext.ExtHostUriOpeners`.
struct ExtHostProxy {
    rpc: Rc<TestRpcProtocol>,
}

impl ExtHostUriOpenersShape for ExtHostProxy {
    fn rpc_can_open_uri(&self, id: &str, uri: &URI, token: CancellationToken) -> Promise<je_editor::languages::ExternalUriOpenerPriority> {
        let rpc = Rc::clone(&self.rpc);
        let (id, uri) = (id.to_owned(), uri.clone());
        self.rpc.remote_call(move || rpc.local(&ExtHostContext::EXT_HOST_URI_OPENERS).rpc_can_open_uri(&id, &uri, token))
    }

    fn rpc_open_uri(&self, id: &str, context: OpenUriContextDto, token: CancellationToken) -> Promise<()> {
        let rpc = Rc::clone(&self.rpc);
        let id = id.to_owned();
        self.rpc.remote_call(move || rpc.local(&ExtHostContext::EXT_HOST_URI_OPENERS).rpc_open_uri(&id, context, token))
    }
}

/// `new class extends mock<IPreferencesService>() { }`.
struct MockPreferencesService;

impl IPreferencesService for MockPreferencesService {
    fn open_user_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openUserSettings is not a function")
    }
}

/// `{ canOpenExternalUri: () => Preferred, openExternalUri: (uri, context) => { opened.push({ uri, context }); } }`.
struct TestOpener {
    opened: Rc<RefCell<Vec<(URI, ApiOpenExternalUriContext)>>>,
}

impl ApiExternalUriOpener for TestOpener {
    fn can_open_external_uri(&self, _uri: &URI, _token: CancellationToken) -> MaybePromise<Option<ExternalUriOpenerPriority>> {
        MaybePromise::Value(Some(ExternalUriOpenerPriority::Preferred))
    }

    fn open_external_uri(&self, resolved_uri: &URI, ctx: &ApiOpenExternalUriContext, _token: CancellationToken) -> MaybePromise<()> {
        self.opened.borrow_mut().push((resolved_uri.clone(), ctx.clone()));
        MaybePromise::Value(())
    }
}

/// `{ async resolveExternalUri() { return { resolved: resolvedUri, dispose() { } }; } }`.
struct ResolvedUriResolver {
    resolved_uri: URI,
}

impl IExternalUriResolver for ResolvedUriResolver {
    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&OpenOptions>) -> Promise<Option<ResolvedExternalUri>> {
        Promise::resolved(Some(ResolvedExternalUri { resolved: self.resolved_uri.clone(), disposable: None }))
    }
}

/// O corpo de `maps internal openToSide=${openToSide} to viewColumn`, com o `setup` antes.
fn maps_internal_open_to_side_to_view_column(open_to_side: Option<bool>) {
    run_with_faked_timers(async move {
        let source_uri = URI::parse("https://source.example.com", false);
        let resolved_uri = URI::parse("https://resolved.example.com", false);

        // setup(async () => { ... })
        let store: LeakGuard = ensure_no_disposables_are_leaked_in_test_suite();
        let opened: Rc<RefCell<Vec<(URI, ApiOpenExternalUriContext)>>> = Rc::default();
        let rpc = TestRpcProtocol::new();
        rpc.register_proxy(&MainContext::MAIN_THREAD_URI_OPENERS, Rc::new(MainThreadProxy { rpc: Rc::clone(&rpc) }) as Rc<dyn MainThreadUriOpenersShape>);
        rpc.register_proxy(&ExtHostContext::EXT_HOST_URI_OPENERS, Rc::new(ExtHostProxy { rpc: Rc::clone(&rpc) }) as Rc<dyn ExtHostUriOpenersShape>);

        let code_editor_service = store.add_rc(Rc::new(TestCodeEditorService::new(TestThemeService::new_default())));
        let command_service: Rc<dyn ICommandService> = Rc::new(NullCommandService);
        let opener_service = store.add_rc(Rc::new(OpenerService::new(code_editor_service, command_service)));
        let opener_service_ref: Rc<dyn IOpenerService> = opener_service.clone();

        let configuration_service = TestConfigurationService::new(None);
        store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let configuration: Rc<dyn IConfigurationService> = configuration_service;
        let preferences_service: Rc<dyn IPreferencesService> = Rc::new(MockPreferencesService);
        let quick_input_service: Rc<dyn IQuickInputService> = TestQuickInputService::new();
        let external_uri_opener_service =
            store.add_rc(ExternalUriOpenerService::new(opener_service_ref.clone(), configuration, NullLogService::shared(), preferences_service, quick_input_service));

        let ext_host_uri_openers = ExtHostUriOpeners::new(rpc.as_ref());
        rpc.set(&ExtHostContext::EXT_HOST_URI_OPENERS, ext_host_uri_openers.clone() as Rc<dyn ExtHostUriOpenersShape>);
        let storage_service: Rc<dyn StorageService> = Rc::new(store.add_rc(TestStorageService::new()));
        let extension_service: Rc<dyn IExtensionService> = TestExtensionService::new();
        let notification_service: Rc<dyn INotificationService> = Rc::new(TestNotificationService);
        let main_thread_uri_openers = store.add_rc(MainThreadUriOpeners::new(
            rpc.as_ref(),
            storage_service,
            external_uri_opener_service as Rc<dyn IExternalUriOpenerService>,
            extension_service,
            opener_service_ref.clone(),
            notification_service,
        ));
        rpc.set(&MainContext::MAIN_THREAD_URI_OPENERS, main_thread_uri_openers as Rc<dyn MainThreadUriOpenersShape>);
        store.add(opener_service.register_external_uri_resolver(Rc::new(ResolvedUriResolver { resolved_uri: resolved_uri.clone() })));
        store.add(
            ext_host_uri_openers
                .register_external_uri_opener(
                    &ExtensionIdentifier::new("test.opener"),
                    "test-opener",
                    Rc::new(TestOpener { opened: Rc::clone(&opened) }),
                    &ApiExternalUriOpenerMetadata { schemes: vec!["https".to_owned()], label: "Test Opener".to_owned() },
                )
                .expect("registerExternalUriOpener"),
        );
        rpc.sync().await.expect("sync");

        // test(`maps internal openToSide=${openToSide} to viewColumn`, async () => { ... })
        let options = OpenOptions {
            allow_contributed_openers: Some(AllowContributedOpeners::Bool(true)),
            open_to_side,
            ..OpenOptions::default()
        };
        opener_service.open(&OpenResource::Uri(source_uri.clone()), Some(&options)).await.expect("open");

        let expected_view_column = (open_to_side == Some(true)).then_some(ViewColumn::BESIDE);
        assert_eq!(*opened.borrow(), [(resolved_uri, ApiOpenExternalUriContext { source_uri, view_column: expected_view_column })]);
    });
}

#[test]
fn maps_internal_open_to_side_true_to_view_column() {
    maps_internal_open_to_side_to_view_column(Some(true));
}

#[test]
fn maps_internal_open_to_side_false_to_view_column() {
    maps_internal_open_to_side_to_view_column(Some(false));
}

#[test]
fn maps_internal_open_to_side_undefined_to_view_column() {
    maps_internal_open_to_side_to_view_column(None);
}
