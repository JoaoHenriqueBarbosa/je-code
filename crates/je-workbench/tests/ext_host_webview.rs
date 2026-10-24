//! Porte de `src/vs/workbench/api/test/browser/extHostWebview.test.ts` (suíte `ExtHostWebview`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `setup` do original
//!   (o `DisposableStore` e o `SingleProxyRPCProtocol`) roda antes do rastreador, então o dublê do `MainThreadWebviewManager`
//!   é criado ANTES do guarda, e o `disposables.add(...)` é o `guard.add_rc(...)` do guarda. Os casos `async` rodam em
//!   `run_with_faked_timers`.
//! - `SingleProxyRPCProtocol(shape)` devolve o mesmo objeto a qualquer proxy; o [`IMainContext`] do porte entrega o proxy
//!   pela forma pedida, então o [`MapMainContext`] registra o dublê nos dois identificadores que o `MainThreadWebviewManager`
//!   atende (`MainThreadWebviews` e `MainThreadWebviewPanels`), por [`MockMainThreadWebviewManager::register_in`].
//!   `createNoopMainThreadWebviews()` é [`MockMainThreadWebviewManager::noop`] (os quatro métodos vazios); o `mock` que só
//!   guarda as chamadas de `$setOptions` é [`MockMainThreadWebviewManager::capturing_set_options`].
//! - `{ extensionLocation } as IExtensionDescription` é a `nullExtensionDescription` com o `extension_location` trocado (o
//!   campo guarda a URI como texto), e `{} as IExtensionDescription` é a própria `nullExtensionDescription`.
//! - `NoopSerializer` guarda um nome (`A` ou `B`) no lugar de `this`, e `lastInvokedDeserializer` compara esse nome; o
//!   `assert.throws` sobre `registerWebviewPanelSerializer` é o `Err` do resultado.
//! - O `IExtHostWorkspace` mock (`getWorkspaceFolders`) é o [`FoldersWorkspace`], que implementa
//!   [`IExtHostWorkspaceFolders`]; `workspaceFolders ? new class ... : undefined` é `Option<Rc<...>>`.
//! - `0 as EditorGroupColumn` é `0.0`; `{}` (o estado e as opções) é um objeto vazio e as opções padrão.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/webview_support.rs"]
mod webview_support;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_base::js_string::JsString;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_protocol::{ExtHostWebviewPanelsShape, IMainContext, WebviewContentOptions, WebviewDeserializeInitData, WebviewPanelOptions};
use je_workbench::api::common::ext_host_webview::{ExtHostWebviews, IExtHostWorkspaceFolders};
use je_workbench::api::common::ext_host_webview_panels::{
    CreateWebviewPanelOptions, ExtHostWebviewPanel, ExtHostWebviewPanels, WebviewPanelSerializer, WebviewShowOptions,
};
use je_workbench::api::common::ext_host_workspace::WorkspaceFolder;
use je_workbench::contrib::webview::webview_common::{WEBVIEW_RESOURCE_BASE_HOST, WebviewRemoteInfo, decode_authority};
use je_workbench::services::extensions::extensions::null_extension_description;
use webview_support::{MapMainContext, MockMainThreadWebviewManager, NullApiDeprecationService};

/// `createNoopMainThreadWebviews()` registrado no contexto (`SingleProxyRPCProtocol(shape)`).
fn noop_rpc() -> Rc<MapMainContext> {
    let rpc = Rc::new(MapMainContext::default());
    MockMainThreadWebviewManager::noop().register_in(&*rpc);
    rpc
}

/// `new ExtHostWebviews(rpcProtocol, { authority, isRemote }, undefined, new NullLogService(), NullApiDeprecationService)`.
fn new_ext_host_webviews(rpc: &dyn IMainContext, remote_authority: Option<&str>, workspace: Option<Rc<dyn IExtHostWorkspaceFolders>>) -> Rc<ExtHostWebviews> {
    ExtHostWebviews::new(
        rpc,
        WebviewRemoteInfo { authority: remote_authority.map(str::to_owned), is_remote: remote_authority.is_some() },
        workspace,
        NullLogService::shared(),
        Rc::new(NullApiDeprecationService),
    )
}

/// `createWebview(rpcProtocol, remoteAuthority)`.
fn create_webview(guard: &common::LeakGuard, rpc: &dyn IMainContext, remote_authority: Option<&str>) -> Rc<ExtHostWebviewPanel> {
    let ext_host_webviews = guard.add_rc(new_ext_host_webviews(rpc, remote_authority, None));

    let ext_host_webview_panels = guard.add_rc(ExtHostWebviewPanels::new(rpc, ext_host_webviews, None));

    let mut extension = null_extension_description();
    extension.extension_location = URI::from(
        &UriComponents {
            scheme: Some(JsString::from(if remote_authority.is_some() { "vscode-remote" } else { "file" })),
            authority: remote_authority.map(JsString::from),
            path: Some(JsString::from("/ext/path")),
            ..UriComponents::default()
        },
        false,
    )
    .to_string(false)
    .to_string_lossy();

    guard.add_rc(ext_host_webview_panels.create_webview_panel(&extension, "type", "title", WebviewShowOptions::ViewColumn(1.0), CreateWebviewPanelOptions::default()))
}

/// `webview.webview.asWebviewUri(URI.parse(uri)).toString()`.
fn as_webview_uri(webview: &ExtHostWebviewPanel, uri: &str) -> String {
    webview.webview().unwrap().as_webview_uri(&URI::parse(uri, false)).to_string(false).to_string_lossy()
}

/// O `vscode.WebviewPanelSerializer` do caso: `deserializeWebviewPanel` guarda o nome em `lastInvokedDeserializer` e
/// registra o painel para o guarda descartar.
struct NoopSerializer {
    name: &'static str,
    last_invoked_deserializer: Rc<Cell<Option<&'static str>>>,
    store: Rc<DisposableStore>,
}

impl WebviewPanelSerializer for NoopSerializer {
    fn deserialize_webview_panel(&self, webview: Rc<ExtHostWebviewPanel>, _state: Dynamic) -> Promise<()> {
        self.last_invoked_deserializer.set(Some(self.name));
        self.store.add_rc(webview);
        Promise::resolved(())
    }
}

/// O `initData` do `$deserializeWebviewPanel` dos casos: `{ title, state: {}, panelOptions: {}, webviewOptions: {}, active: true }`.
fn deserialize_init_data() -> WebviewDeserializeInitData {
    WebviewDeserializeInitData {
        title: "title".to_owned(),
        state: Dynamic::object(Vec::<(&str, Dynamic)>::new()),
        panel_options: WebviewPanelOptions::default(),
        webview_options: WebviewContentOptions::default(),
        active: true,
    }
}

#[test]
fn cannot_register_multiple_serializers_for_the_same_view_type() {
    let rpc = noop_rpc();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let view_type = "view.type";

        let ext_host_webviews = store.add_rc(new_ext_host_webviews(&*rpc, None, None));

        let ext_host_webview_panels = store.add_rc(ExtHostWebviewPanels::new(&*rpc, ext_host_webviews, None));

        let last_invoked_deserializer: Rc<Cell<Option<&'static str>>> = Rc::new(Cell::new(None));

        let extension = null_extension_description();

        let serializer_a = Rc::new(NoopSerializer { name: "A", last_invoked_deserializer: last_invoked_deserializer.clone(), store: store.clone() });
        let serializer_b = Rc::new(NoopSerializer { name: "B", last_invoked_deserializer: last_invoked_deserializer.clone(), store: store.clone() });

        let serializer_a_registration = ext_host_webview_panels.register_webview_panel_serializer(&extension, view_type, serializer_a).unwrap();

        ext_host_webview_panels.rpc_deserialize_webview_panel("x", view_type, deserialize_init_data(), 0.0).await.unwrap();
        assert_eq!(last_invoked_deserializer.get(), Some("A"));

        // Should throw when registering two serializers for the same view
        assert!(ext_host_webview_panels.register_webview_panel_serializer(&extension, view_type, serializer_b.clone()).is_err());

        serializer_a_registration.dispose();

        store.add_rc(Rc::new(ext_host_webview_panels.register_webview_panel_serializer(&extension, view_type, serializer_b).unwrap()));

        ext_host_webview_panels.rpc_deserialize_webview_panel("x", view_type, deserialize_init_data(), 0.0).await.unwrap();
        assert_eq!(last_invoked_deserializer.get(), Some("B"));
    });
}

#[test]
fn as_webview_uri_for_local_file_paths() {
    let rpc = noop_rpc();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let webview = create_webview(&guard, &*rpc, None);

    // Unix basic
    assert_eq!(
        as_webview_uri(&webview, "file:///Users/codey/file.html"),
        format!("https://file%2B.vscode-resource.{WEBVIEW_RESOURCE_BASE_HOST}/Users/codey/file.html"),
    );

    // Unix should preserve fragment
    assert_eq!(
        as_webview_uri(&webview, "file:///Users/codey/file.html#frag"),
        format!("https://file%2B.vscode-resource.{WEBVIEW_RESOURCE_BASE_HOST}/Users/codey/file.html#frag"),
    );

    // Unix with encoding
    assert_eq!(
        as_webview_uri(&webview, "file:///Users/codey/f%20ile.html"),
        format!("https://file%2B.vscode-resource.{WEBVIEW_RESOURCE_BASE_HOST}/Users/codey/f%20ile.html"),
    );

    // Unix should preserve authority
    assert_eq!(
        as_webview_uri(&webview, "file://localhost/Users/codey/file.html"),
        format!("https://file%2Blocalhost.vscode-resource.{WEBVIEW_RESOURCE_BASE_HOST}/Users/codey/file.html"),
    );

    // Windows C drive
    assert_eq!(
        as_webview_uri(&webview, "file:///c:/codey/file.txt"),
        format!("https://file%2B.vscode-resource.{WEBVIEW_RESOURCE_BASE_HOST}/c%3A/codey/file.txt"),
    );
}

#[test]
fn as_webview_uri_for_remote_file_paths() {
    let rpc = noop_rpc();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let webview = create_webview(&guard, &*rpc, Some("remote"));

    // Unix basic
    assert_eq!(
        as_webview_uri(&webview, "file:///Users/codey/file.html"),
        format!("https://vscode-remote%2Bremote.vscode-resource.{WEBVIEW_RESOURCE_BASE_HOST}/Users/codey/file.html"),
    );
}

/// `URI.from({ scheme: 'vscode-remote', authority, path: '/Users/cody/x.png' })`.
fn remote_source_uri(authority: &str) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from("vscode-remote")),
            authority: Some(JsString::from(authority)),
            path: Some(JsString::from("/Users/cody/x.png")),
            ..UriComponents::default()
        },
        false,
    )
}

#[test]
fn as_webview_uri_for_remote_with_slash_and_plus_in_name() {
    let rpc = noop_rpc();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let webview = create_webview(&guard, &*rpc, Some("remote"));
    let authority = "ssh-remote+localhost=foo/bar";

    let source_uri = remote_source_uri(authority);

    let webview_uri = webview.webview().unwrap().as_webview_uri(&source_uri);
    // Check transform
    assert_eq!(
        webview_uri.to_string(false).to_string_lossy(),
        "https://vscode-remote%2Bssh-002dremote-002blocalhost-003dfoo-002fbar.vscode-resource.vscode-cdn.net/Users/cody/x.png",
    );

    // Check decoded authority
    assert_eq!(
        decode_authority(webview_uri.authority()).to_string_lossy(),
        format!("vscode-remote+{authority}.vscode-resource.vscode-cdn.net"),
    );
}

#[test]
fn as_webview_uri_for_remote_with_port_in_name() {
    let rpc = noop_rpc();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let webview = create_webview(&guard, &*rpc, Some("remote"));
    let authority = "localhost:8080";

    let source_uri = remote_source_uri(authority);

    let webview_uri = webview.webview().unwrap().as_webview_uri(&source_uri);
    // Check transform
    assert_eq!(
        webview_uri.to_string(false).to_string_lossy(),
        "https://vscode-remote%2Blocalhost-003a8080.vscode-resource.vscode-cdn.net/Users/cody/x.png",
    );

    // Check decoded authority
    assert_eq!(
        decode_authority(webview_uri.authority()).to_string_lossy(),
        format!("vscode-remote+{authority}.vscode-resource.vscode-cdn.net"),
    );
}

// ---- suite `ensureDefaultContentOptions`

/// O `IExtHostWorkspace` mock: `getWorkspaceFolders` devolve `f0`, `f1`, ... sobre as URIs dadas.
struct FoldersWorkspace {
    folders: Vec<Rc<WorkspaceFolder>>,
}

impl IExtHostWorkspaceFolders for FoldersWorkspace {
    fn get_workspace_folders(&self) -> Option<Vec<Rc<WorkspaceFolder>>> {
        Some(self.folders.clone())
    }
}

/// `createExtHostWebviewsWithCapture(workspaceFolders)`.
fn create_ext_host_webviews_with_capture(
    guard: &common::LeakGuard,
    workspace_folders: Option<Vec<URI>>,
) -> (Rc<ExtHostWebviews>, Rc<MockMainThreadWebviewManager>) {
    let shape = MockMainThreadWebviewManager::capturing_set_options();
    let capture_rpc = MapMainContext::default();
    shape.register_in(&capture_rpc);

    let workspace: Option<Rc<dyn IExtHostWorkspaceFolders>> = workspace_folders.map(|folders| {
        let folders = folders.into_iter().enumerate().map(|(index, uri)| WorkspaceFolder::new(uri, format!("f{index}"), index)).collect();
        Rc::new(FoldersWorkspace { folders }) as Rc<dyn IExtHostWorkspaceFolders>
    });

    let ext_host_webviews = guard.add_rc(new_ext_host_webviews(&capture_rpc, None, workspace));

    (ext_host_webviews, shape)
}

/// `{ extensionLocation: URI.file('/ext/install/path') } as IExtensionDescription`.
fn install_path_extension() -> (ExtensionDescription, String) {
    let location = URI::file("/ext/install/path", Platform::Posix).to_string(false).to_string_lossy();
    let mut extension = null_extension_description();
    extension.extension_location = location.clone();
    (extension, location)
}

#[test]
fn fills_default_local_resource_roots_from_workspace_folders_and_extension_location_when_caller_did_not_supply_them() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (extension, extension_location) = install_path_extension();

    let folder_a = URI::file("/workspace/a", Platform::Posix);
    let folder_b = URI::file("/workspace/b", Platform::Posix);
    let (ext_host_webviews, shape) = create_ext_host_webviews_with_capture(&guard, Some(vec![folder_a.clone(), folder_b.clone()]));

    let options = WebviewContentOptions { enable_scripts: Some(true), ..WebviewContentOptions::default() };
    guard.add_rc(ext_host_webviews.create_new_webview("handle-1", &options, &extension));
    ext_host_webviews.ensure_default_content_options("handle-1", &options, &extension);

    let set_options_calls = shape.set_options_calls.borrow();
    assert_eq!(set_options_calls.len(), 1, "expected $setOptions to be called once");
    let call = &set_options_calls[0];
    assert_eq!(call.0, "handle-1");
    assert_eq!(call.1.enable_scripts, Some(true));
    let roots = call.1.local_resource_roots.as_ref().expect("expected localResourceRoots to be set");
    let root_strings: Vec<String> = roots.iter().map(|root| root.to_string(false).to_string_lossy()).collect();
    assert_eq!(
        root_strings,
        vec![folder_a.to_string(false).to_string_lossy(), folder_b.to_string(false).to_string_lossy(), extension_location],
    );
}

#[test]
fn does_nothing_when_caller_already_supplied_local_resource_roots() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (extension, _) = install_path_extension();

    let (ext_host_webviews, shape) = create_ext_host_webviews_with_capture(&guard, Some(vec![URI::file("/workspace/a", Platform::Posix)]));
    let explicit = vec![URI::file("/explicit/root", Platform::Posix)];

    let options = WebviewContentOptions { local_resource_roots: Some(explicit), ..WebviewContentOptions::default() };
    guard.add_rc(ext_host_webviews.create_new_webview("handle-2", &options, &extension));
    ext_host_webviews.ensure_default_content_options("handle-2", &options, &extension);

    assert_eq!(shape.set_options_calls.borrow().len(), 0, "expected $setOptions not to be called");
}

#[test]
fn falls_back_to_just_the_extension_location_when_there_are_no_workspace_folders() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (extension, extension_location) = install_path_extension();

    let (ext_host_webviews, shape) = create_ext_host_webviews_with_capture(&guard, None);

    let options = WebviewContentOptions::default();
    guard.add_rc(ext_host_webviews.create_new_webview("handle-3", &options, &extension));
    ext_host_webviews.ensure_default_content_options("handle-3", &options, &extension);

    let set_options_calls = shape.set_options_calls.borrow();
    assert_eq!(set_options_calls.len(), 1);
    let roots = set_options_calls[0].1.local_resource_roots.as_ref().unwrap();
    let root_strings: Vec<String> = roots.iter().map(|root| root.to_string(false).to_string_lossy()).collect();
    assert_eq!(root_strings, vec![extension_location]);
}
