//! Porte de `src/vs/workbench/contrib/webviewPanel/browser/webviewWorkbenchService.ts`: o contrato do serviço que mostra e
//! gerencia os editores de webview ([`IWebviewWorkbenchService`]) e o [`WebviewResolver`] que o `MainThreadWebviewPanels`
//! registra nele.
//!
//! Divergências:
//! - Só entram o contrato (`IWebviewWorkbenchService`, `IWebViewShowOptions`, `WebviewResolver`) e o que os
//!   `MainThread*` leem dele. A implementação `WebviewEditorService`, com o `RevivalPool` e o
//!   `LazilyResolvedWebviewEditorInput`, depende do `IWebviewService` (que cria as sobreposições de webview, recorte da Fase 6)
//!   e de `IEditorService.openEditor` com uma entrada tipada e grupo preferido, que o [`IEditorService`] do porte ainda não
//!   expõe.
//! - `IEditorGroup | GroupIdentifier | ACTIVE_GROUP_TYPE | SIDE_GROUP_TYPE` é o [`GroupRef`] (o grupo ou o identificador, em
//!   que `ACTIVE_GROUP` e `SIDE_GROUP` são identificadores especiais).
//! - `instanceof WebviewInput` das entradas é a entrada concreta `Rc<WebviewInput>`.
//!
//! [`IEditorService`]: crate::services::editor::editor_service::IEditorService

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;

use super::webview_editor_input::{WebviewIconPath, WebviewInput};
use crate::contrib::webview::browser_webview::WebviewInitInfo;
use crate::services::editor::editor_groups_service::GroupRef;

/// `IWebViewShowOptions`.
#[derive(Clone, Default)]
pub struct IWebViewShowOptions {
    pub group: Option<GroupRef>,
    pub preserve_focus: Option<bool>,
}

/// O argumento de `openRevivedWebview(options)`.
pub struct OpenRevivedWebviewOptions {
    pub webview_init_info: WebviewInitInfo,
    pub view_type: String,
    pub title: String,
    pub icon_path: Option<WebviewIconPath>,
    pub state: Dynamic,
    pub group: Option<f64>,
}

/// Handles filling in the content of webview before it can be shown to the user (`WebviewResolver`).
pub trait WebviewResolver {
    /// Returns true if the resolver can resolve the given webview.
    fn can_resolve(&self, webview: &Rc<WebviewInput>) -> bool;

    /// Resolves the webview.
    fn resolve_webview(&self, webview: Rc<WebviewInput>, token: CancellationToken) -> Promise<()>;
}

/// Service responsible for showing and managing webview editors in the workbench (`IWebviewWorkbenchService`).
pub trait IWebviewWorkbenchService {
    /// Event fired when focus switches to a different webview editor.
    ///
    /// Fires `None` if focus switches to a non-webview editor.
    fn on_did_change_active_webview_editor(&self) -> Event<Option<Rc<WebviewInput>>>;

    /// Create a new webview editor and open it in the workbench.
    fn open_webview(
        &self,
        webview_init_info: WebviewInitInfo,
        view_type: &str,
        title: &str,
        icon_path: Option<WebviewIconPath>,
        show_options: IWebViewShowOptions,
    ) -> Rc<WebviewInput>;

    /// Open a webview that is being restored from serialization.
    fn open_revived_webview(&self, options: OpenRevivedWebviewOptions) -> Rc<WebviewInput>;

    /// Reveal an already opened webview editor in the workbench.
    fn reveal_webview(&self, webview: &Rc<WebviewInput>, group: GroupRef, preserve_focus: bool);

    /// Register a new [`WebviewResolver`].
    ///
    /// If there are any webviews awaiting revival that this resolver can handle, they will be resolved by it.
    fn register_resolver(&self, resolver: Rc<dyn WebviewResolver>) -> DisposableRef;

    /// Check if a webview should be serialized across window reloads.
    fn should_persist(&self, input: &Rc<WebviewInput>) -> bool;

    /// Try to resolve a webview. This will block until a resolver is registered for the webview.
    fn resolve_webview(&self, webview: Rc<WebviewInput>, token: CancellationToken) -> Promise<()>;
}
