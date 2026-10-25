//! Porte de `src/vs/workbench/api/test/browser/mainThreadWebviewPanels.test.ts` (suíte `MainThreadWebviewPanels`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (`store` é o `guard.add_rc`);
//!   os casos `async` rodam em `run_with_faked_timers`.
//! - Os `new class extends mock<T>()` são dublês de método ausente que falha (`panic!`), como o método ausente do objeto
//!   vazio lançaria `TypeError`: [`OverlayWebview`] (`IOverlayWebview`, com `extension`, `contentOptions` e `options` como
//!   campos e `onDidDispose = Event.None`), [`ExtHostPanels`] (`ExtHostWebviewPanelsShape`, só `$deserializeWebviewPanel`),
//!   [`MainThreadWebviewsMock`] (`addWebview() { }`), [`ExtensionServiceMock`] (`activateByEvent` e `getExtension`),
//!   [`WebviewWorkbenchServiceMock`] (`onDidChangeActiveWebviewEditor = Event.None` e `registerResolver`) e
//!   [`IgnorePathCaseUriIdentity`] (`extUri = extUriIgnorePathCase`).
//! - `IThemeService` não foi portado: o [`TestThemeService`] implementa o [`IWebviewThemeService`] mínimo da entrada.
//! - O dublê do host de extensões é registrado no [`MapMainContext`] (`SingleProxyRPCProtocol(...)` devolve o mesmo objeto a
//!   qualquer proxy; aqui cada proxy é registrado pela forma pedida). `Disposable.None` é [`none`].
//! - `assert.deepStrictEqual(deserializedOptions, webview.contentOptions)`: o `$deserializeWebviewPanel` leva o
//!   `IWebviewContentOptions` (`enableScripts`) e o webview guarda o `WebviewContentOptions` (`allowScripts`); o original passa
//!   o mesmo objeto, e o porte tipado o converte com `webview_content_options_to_dto`, então a conferência compara o que
//!   chegou ao host com a conversão das opções do webview.
//! - `assert.strictEqual(webview.contentOptions.localResourceRoots, roots)` (a mesma referência, "nenhuma troca") é a conferência
//!   de que o webview não recebeu novas opções (`set_content_options` não foi chamado) e de que as raízes continuam iguais.
//! - O `for (const roots of [undefined, []])` do original registra dois casos: `preserves_unspecified_roots` e
//!   `preserves_empty_roots`.
//! - `oldExtension = {...}` (parâmetro com valor padrão) é o `Option` explícito de cada chamada; `null` é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/webview_support.rs"]
mod webview_support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable, none};
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange};
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::Injectable;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::NullLogService;
use je_platform::resources::host_environment;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::api::browser::main_thread_webview_panels::{MainThreadWebviewPanels, MainThreadWebviewPanelsServices, webview_content_options_to_dto};
use je_workbench::api::browser::main_thread_webviews::{IMainThreadWebviews, MainThreadWebviewOptions};
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostWebviewPanelsShape, IMainContext, MainThreadWebviewPanelsShape, WebviewContentOptions as WebviewContentOptionsDto,
    WebviewDeserializeInitData, WebviewPanelViewStateData, WebviewSerializerOptions,
};
use je_workbench::contrib::webview::browser_webview::{
    IOverlayWebview, IWebview, WebviewContentOptions, WebviewExtensionDescription, WebviewMessageReceivedEvent, WebviewOptions,
};
use je_workbench::contrib::webview::resource_loading::get_resource_to_load;
use je_workbench::contrib::webview_panel::webview_editor_input::{IWebviewThemeService, WebviewIconPath, WebviewInput, WebviewInputInitInfo};
use je_workbench::contrib::webview_panel::webview_workbench_service::{
    IWebViewShowOptions, IWebviewWorkbenchService, OpenRevivedWebviewOptions, WebviewResolver,
};
use je_workbench::services::editor::editor_groups_service::GroupRef;
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestEditorService};
use je_theme::ThemeType;
use webview_support::MapMainContext;

const EXTENSION_ID: &str = "publisher.extension";

fn extension_id() -> ExtensionIdentifier {
    ExtensionIdentifier::new(EXTENSION_ID)
}

fn old_location() -> URI {
    URI::file("/extensions/publisher.extension-1.0.0", Platform::Posix)
}

fn new_location() -> URI {
    URI::file("/extensions/publisher.extension-2.0.0", Platform::Posix)
}

/// `URI.joinPath(uri, 'dist')`.
fn join_dist(uri: &URI) -> URI {
    URI::join_path(uri, &["dist"], Platform::Posix)
}

// ---- dublês

/// `new class extends mock<IOverlayWebview>()`.
struct OverlayWebview {
    extension: RefCell<Option<WebviewExtensionDescription>>,
    content_options: RefCell<WebviewContentOptions>,
    set_content_options_calls: Cell<usize>,
}

impl IDisposable for OverlayWebview {
    fn dispose(&self) {}
}

impl IWebview for OverlayWebview {
    fn provided_view_type(&self) -> Option<String> {
        panic!("providedViewType is not defined");
    }

    fn origin(&self) -> String {
        panic!("origin is not defined");
    }

    fn set_html(&self, _html: &str) {
        panic!("setHtml is not a function");
    }

    fn set_title(&self, _title: &str) {
        panic!("setTitle is not a function");
    }

    fn content_options(&self) -> WebviewContentOptions {
        self.content_options.borrow().clone()
    }

    fn set_content_options(&self, options: WebviewContentOptions) {
        self.set_content_options_calls.set(self.set_content_options_calls.get() + 1);
        *self.content_options.borrow_mut() = options;
    }

    fn extension(&self) -> Option<WebviewExtensionDescription> {
        self.extension.borrow().clone()
    }

    fn set_extension(&self, extension: Option<WebviewExtensionDescription>) {
        *self.extension.borrow_mut() = extension;
    }

    /// O `mock` não tem `state`: `undefined`.
    fn state(&self) -> Option<String> {
        None
    }

    fn set_state(&self, _state: Option<String>) {
        panic!("state is not writable");
    }

    fn on_did_dispose(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_click_link(&self) -> Event<String> {
        panic!("onDidClickLink is not defined");
    }

    fn on_missing_csp(&self) -> Event<ExtensionIdentifier> {
        panic!("onMissingCsp is not defined");
    }

    fn on_message(&self) -> Event<WebviewMessageReceivedEvent> {
        panic!("onMessage is not defined");
    }

    fn post_message(&self, _message: Dynamic, _transfer: Option<Vec<Vec<u8>>>) -> Promise<bool> {
        panic!("postMessage is not a function");
    }
}

impl IOverlayWebview for OverlayWebview {
    /// `override options = {}`.
    fn options(&self) -> WebviewOptions {
        WebviewOptions::default()
    }

    fn set_options(&self, _options: WebviewOptions) {
        panic!("options is not writable");
    }
}

/// `new TestThemeService()`.
struct TestThemeService;

impl IWebviewThemeService for TestThemeService {
    fn on_did_color_theme_change(&self) -> Event<()> {
        Event::none()
    }

    fn color_theme_type(&self) -> ThemeType {
        ThemeType::Dark
    }
}

/// O host de extensões: `$deserializeWebviewPanel` guarda o `webviewOptions`.
#[derive(Default)]
struct ExtHostPanels {
    deserialized_options: RefCell<Option<WebviewContentOptionsDto>>,
}

impl ExtHostWebviewPanelsShape for ExtHostPanels {
    fn rpc_on_did_change_webview_panel_view_states(&self, _new_state: WebviewPanelViewStateData) {
        panic!("$onDidChangeWebviewPanelViewStates is not a function");
    }

    fn rpc_on_did_dispose_webview_panel(&self, _handle: &str) -> Promise<()> {
        panic!("$onDidDisposeWebviewPanel is not a function");
    }

    fn rpc_deserialize_webview_panel(
        &self,
        _new_webview_handle: &str,
        _view_type: &str,
        init_data: WebviewDeserializeInitData,
        _position: f64,
    ) -> Promise<()> {
        *self.deserialized_options.borrow_mut() = Some(init_data.webview_options);
        Promise::resolved(())
    }
}

/// `new class extends mock<MainThreadWebviews>() { override addWebview() { } }`.
struct MainThreadWebviewsMock;

impl IMainThreadWebviews for MainThreadWebviewsMock {
    fn add_webview(&self, _handle: &str, _webview: Rc<dyn IWebview>, _options: MainThreadWebviewOptions) -> Result<(), je_base::errors::Error> {
        Ok(())
    }

    fn get_webview_resolved_failed_content(&self, _view_type: &str) -> String {
        panic!("getWebviewResolvedFailedContent is not a function");
    }
}

/// `new class extends mock<IExtensionService>()`: `activateByEvent` não faz nada e `getExtension` confere o id e devolve a
/// localização atual (ou `undefined`).
struct ExtensionServiceMock {
    current_location: Option<URI>,
}

impl IExtensionService for ExtensionServiceMock {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        Promise::resolved(())
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        panic!("activationEventIsDone is not a function");
    }

    fn get_extension(&self, id: &str) -> Promise<Option<IExtensionDescription>> {
        assert_eq!(id, EXTENSION_ID);
        Promise::resolved(self.current_location.clone().map(|extension_location| IExtensionDescription { extension_location, display_name: None, name: None }))
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        panic!("onDidRegisterExtensions is not defined");
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        panic!("whenInstalledExtensionsRegistered is not a function");
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        panic!("extensions is not defined");
    }
}

/// `new class extends mock<IWebviewWorkbenchService>()`: `onDidChangeActiveWebviewEditor = Event.None` e `registerResolver`
/// (que guarda o resolvedor e devolve `Disposable.None`).
#[derive(Default)]
struct WebviewWorkbenchServiceMock {
    resolvers: RefCell<Vec<Rc<dyn WebviewResolver>>>,
}

impl IWebviewWorkbenchService for WebviewWorkbenchServiceMock {
    fn on_did_change_active_webview_editor(&self) -> Event<Option<Rc<WebviewInput>>> {
        Event::none()
    }

    fn open_webview(
        &self,
        _webview_init_info: je_workbench::contrib::webview::browser_webview::WebviewInitInfo,
        _view_type: &str,
        _title: &str,
        _icon_path: Option<WebviewIconPath>,
        _show_options: IWebViewShowOptions,
    ) -> Rc<WebviewInput> {
        panic!("openWebview is not a function");
    }

    fn open_revived_webview(&self, _options: OpenRevivedWebviewOptions) -> Rc<WebviewInput> {
        panic!("openRevivedWebview is not a function");
    }

    fn reveal_webview(&self, _webview: &Rc<WebviewInput>, _group: GroupRef, _preserve_focus: bool) {
        panic!("revealWebview is not a function");
    }

    fn register_resolver(&self, resolver: Rc<dyn WebviewResolver>) -> DisposableRef {
        self.resolvers.borrow_mut().push(resolver);
        none()
    }

    fn should_persist(&self, _input: &Rc<WebviewInput>) -> bool {
        panic!("shouldPersist is not a function");
    }

    fn resolve_webview(&self, _webview: Rc<WebviewInput>, _token: CancellationToken) -> Promise<()> {
        panic!("resolveWebview is not a function");
    }
}

/// `new class extends mock<IUriIdentityService>() { override extUri = extUriIgnorePathCase; }`.
struct IgnorePathCaseUriIdentity;

impl IUriIdentityService for IgnorePathCaseUriIdentity {
    fn ext_uri(&self) -> ExtUri {
        ExtUri::ignore_path_case(host_environment())
    }

    fn as_canonical_uri(&self, _uri: &URI) -> URI {
        panic!("asCanonicalUri is not a function");
    }
}

/// O que `restore(...)` devolve: o webview e o serviço de identidade de URIs usado.
struct Restored {
    webview: Rc<OverlayWebview>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

/// O `UriIdentityService` real do valor padrão de `uriIdentityService` (sobre um `FileService` do `NullLogService`).
fn default_uri_identity_service(store: &LeakGuard) -> Rc<dyn IUriIdentityService> {
    let file_service = store.add_rc(FileService::new(NullLogService::shared()));
    let file_service: Rc<dyn IFileService> = file_service;
    let service = store.add_rc(UriIdentityService::create((), (file_service,)));
    service
}

/// `restore(localResourceRoots, oldExtension, currentLocation, uriIdentityService)`.
async fn restore(
    store: &LeakGuard,
    local_resource_roots: Option<Vec<URI>>,
    old_extension: Option<WebviewExtensionDescription>,
    current_location: Option<URI>,
    uri_identity_service: Option<Rc<dyn IUriIdentityService>>,
) -> Restored {
    let uri_identity_service = uri_identity_service.unwrap_or_else(|| default_uri_identity_service(store));
    let webview = Rc::new(OverlayWebview {
        extension: RefCell::new(old_extension),
        content_options: RefCell::new(WebviewContentOptions { allow_scripts: Some(true), local_resource_roots, ..WebviewContentOptions::default() }),
        set_content_options_calls: Cell::new(0),
    });
    let input = store.add_rc(WebviewInput::new(
        WebviewInputInitInfo { view_type: "mainThreadWebview-test".to_owned(), provided_id: Some("test".to_owned()), name: "Test".to_owned(), icon_path: None },
        webview.clone(),
        Rc::new(TestThemeService),
    ));
    let workbench_service = Rc::new(WebviewWorkbenchServiceMock::default());
    let ext_host = Rc::new(ExtHostPanels::default());
    let context: Rc<dyn IMainContext> = Rc::new(MapMainContext::default());
    let proxy: Rc<dyn ExtHostWebviewPanelsShape> = ext_host.clone();
    context.set(&ExtHostContext::EXT_HOST_WEBVIEW_PANELS, proxy);
    let storage_service = store.add_rc(InMemoryStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());
    let panels = store.add_rc(MainThreadWebviewPanels::new(
        &*context,
        Rc::new(MainThreadWebviewsMock),
        MainThreadWebviewPanelsServices {
            configuration_service: TestConfigurationService::new(None),
            editor_group_service: TestEditorGroupsService::new(None),
            editor_service: Rc::new(TestEditorService),
            extension_service: Rc::new(ExtensionServiceMock { current_location }),
            storage_service: storage,
            webview_workbench_service: workbench_service.clone(),
            uri_identity_service: uri_identity_service.clone(),
        },
    ));
    panels.rpc_register_serializer("test", WebviewSerializerOptions { serialize_buffers_for_post_message: false });
    let input_ref: Rc<WebviewInput> = input.clone();
    let resolver = workbench_service.resolvers.borrow().iter().find(|resolver| resolver.can_resolve(&input_ref)).cloned().expect("o serializador foi registrado");
    resolver.resolve_webview(input_ref, CancellationToken::None).await.unwrap();
    assert_eq!(*ext_host.deserialized_options.borrow(), Some(webview_content_options_to_dto(&webview.content_options())));
    Restored { webview, uri_identity_service }
}

fn default_old_extension() -> Option<WebviewExtensionDescription> {
    Some(WebviewExtensionDescription { id: extension_id(), location: Some(old_location()) })
}

/// `URI.toString()`.
fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `roots.map(root => root.toString())`.
fn texts(roots: Option<Vec<URI>>) -> Option<Vec<String>> {
    roots.map(|roots| roots.iter().map(text).collect())
}

// ---- casos

#[test]
fn restores_extension_relative_roots_before_deserializing_without_widening_access() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let restored = restore(&store, Some(vec![join_dist(&old_location())]), default_old_extension(), Some(new_location()), None).await;
        let webview = &restored.webview;
        let roots = webview.content_options().local_resource_roots.expect("raízes");
        let script = URI::join_path(&new_location(), &["dist", "webview.js"], Platform::Posix);
        let load = |uri: &URI| get_resource_to_load(uri, &roots, &*restored.uri_identity_service, None).map(|loaded| text(&loaded));

        assert_eq!(webview.extension(), Some(WebviewExtensionDescription { id: extension_id(), location: Some(new_location()) }));
        let options = webview.content_options();
        assert_eq!(options.allow_scripts, Some(true));
        assert_eq!(texts(options.local_resource_roots.clone()), Some(vec![text(&join_dist(&new_location()))]));
        assert_eq!(load(&script), Some(text(&script)));
        assert_eq!(load(&URI::join_path(&new_location(), &["private", "secret.txt"], Platform::Posix)), None);
        assert_eq!(load(&URI::join_path(&old_location(), &["dist", "webview.js"], Platform::Posix)), None);
    });
}

#[test]
fn preserves_unrelated_roots_and_relocates_the_extension_root_itself() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let old = old_location();
        let unrelated = vec![
            URI::file("/workspace", Platform::Posix),
            URI::file(format!("{}-other/dist", old.path().to_string_lossy()), Platform::Posix),
            old.with(&UriChange { scheme: Field::from("other"), ..UriChange::default() }),
            URI::parse("vscode-remote://other/extensions/publisher.extension-1.0.0/dist", false),
            old.with(&UriChange { path: Field::from(format!("{}/../outside", old.path().to_string_lossy()).as_str()), ..UriChange::default() }),
        ];
        let mut roots = vec![old.clone()];
        roots.extend(unrelated.clone());
        let restored = restore(&store, Some(roots), default_old_extension(), Some(new_location()), None).await;
        let mut expected = vec![new_location()];
        expected.extend(unrelated);
        assert_eq!(restored.webview.content_options(), WebviewContentOptions { allow_scripts: Some(true), local_resource_roots: Some(expected), ..WebviewContentOptions::default() });
    });
}

#[test]
fn relocates_remote_extension_roots() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let old_remote = URI::parse("vscode-remote://ssh-remote+host/extensions/publisher.extension-1.0.0", false);
        let new_remote = old_remote.with(&UriChange { path: Field::from("/extensions/publisher.extension-2.0.0"), ..UriChange::default() });
        let other_host_root = join_dist(&old_remote.with(&UriChange { authority: Field::from("ssh-remote+other"), ..UriChange::default() }));
        let restored = restore(
            &store,
            Some(vec![join_dist(&old_remote), other_host_root.clone()]),
            Some(WebviewExtensionDescription { id: extension_id(), location: Some(old_remote) }),
            Some(new_remote.clone()),
            None,
        )
        .await;
        assert_eq!(restored.webview.content_options().local_resource_roots, Some(vec![join_dist(&new_remote), other_host_root]));
    });
}

#[test]
fn relocates_windows_extension_roots_with_spaces_and_trailing_separators() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let old_windows = URI::parse("file:///c:/Users/Code%20User/.vscode/extensions/publisher.extension-1.0.0/", false);
        let new_windows = URI::parse("file:///c:/Users/Code%20User/.vscode/extensions/publisher.extension-2.0.0", false);
        let restored = restore(
            &store,
            Some(vec![join_dist(&old_windows)]),
            Some(WebviewExtensionDescription { id: extension_id(), location: Some(old_windows) }),
            Some(new_windows.clone()),
            None,
        )
        .await;
        assert_eq!(texts(restored.webview.content_options().local_resource_roots), Some(vec![text(&join_dist(&new_windows))]));
    });
}

#[test]
fn preserves_subdirectories_on_case_insensitive_file_systems() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let uri_identity_service: Rc<dyn IUriIdentityService> = Rc::new(IgnorePathCaseUriIdentity);
        let old = old_location();
        let upper_dist = old.with(&UriChange {
            path: Field::Set(JsString::from(format!("{}/dist", old.path().to_string_lossy().to_uppercase()).as_str())),
            ..UriChange::default()
        });
        let restored = restore(
            &store,
            Some(vec![upper_dist]),
            Some(WebviewExtensionDescription { id: extension_id(), location: Some(old) }),
            Some(new_location()),
            Some(uri_identity_service),
        )
        .await;
        assert_eq!(texts(restored.webview.content_options().local_resource_roots), Some(vec![text(&join_dist(&new_location()))]));
    });
}

#[test]
fn preserves_unspecified_roots() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let restored = restore(&store, None, default_old_extension(), Some(new_location()), None).await;
        assert_eq!(restored.webview.content_options(), WebviewContentOptions { allow_scripts: Some(true), local_resource_roots: None, ..WebviewContentOptions::default() });
    });
}

#[test]
fn preserves_empty_roots() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let restored = restore(&store, Some(Vec::new()), default_old_extension(), Some(new_location()), None).await;
        assert_eq!(restored.webview.content_options(), WebviewContentOptions { allow_scripts: Some(true), local_resource_roots: Some(Vec::new()), ..WebviewContentOptions::default() });
    });
}

#[test]
fn leaves_roots_unchanged_when_the_extension_location_has_not_changed() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let roots = vec![join_dist(&old_location())];
        let restored = restore(&store, Some(roots.clone()), default_old_extension(), Some(old_location()), None).await;
        assert_eq!(restored.webview.set_content_options_calls.get(), 0);
        assert_eq!(restored.webview.content_options().local_resource_roots, Some(roots));
    });
}

#[test]
fn leaves_roots_unchanged_when_the_extension_is_unavailable() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let roots = vec![join_dist(&old_location())];
        let restored = restore(&store, Some(roots.clone()), default_old_extension(), None, None).await;
        assert_eq!(restored.webview.extension(), Some(WebviewExtensionDescription { id: extension_id(), location: Some(old_location()) }));
        assert_eq!(restored.webview.set_content_options_calls.get(), 0);
        assert_eq!(restored.webview.content_options().local_resource_roots, Some(roots));
    });
}

#[test]
fn leaves_roots_unchanged_when_the_saved_extension_is_unknown() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let roots = vec![join_dist(&old_location())];
        let restored = restore(&store, Some(roots.clone()), None, Some(new_location()), None).await;
        assert_eq!(restored.webview.set_content_options_calls.get(), 0);
        assert_eq!(restored.webview.content_options().local_resource_roots, Some(roots));
    });
}

#[test]
fn leaves_roots_unchanged_when_the_saved_extension_location_is_unknown() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let roots = vec![join_dist(&old_location())];
        let restored = restore(&store, Some(roots.clone()), Some(WebviewExtensionDescription { id: extension_id(), location: None }), Some(new_location()), None).await;
        assert_eq!(restored.webview.set_content_options_calls.get(), 0);
        assert_eq!(restored.webview.content_options().local_resource_roots, Some(roots));
    });
}
