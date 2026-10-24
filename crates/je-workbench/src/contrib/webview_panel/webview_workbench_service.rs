//! Porte de `src/vs/workbench/contrib/webviewPanel/browser/webviewWorkbenchService.ts`: o contrato do serviço que mostra e
//! gerencia os editores de webview ([`IWebviewWorkbenchService`]) e o [`WebviewResolver`] que o `MainThreadWebviewPanels`
//! registra nele.
//!
//! Divergências:
//! - Só entram o contrato (`IWebviewWorkbenchService`, `IWebViewShowOptions`, `WebviewResolver`), o que os `MainThread*` leem
//!   dele e o `LazilyResolvedWebviewEditorInput` (a base das entradas de editor personalizado). A implementação
//!   `WebviewEditorService`, com o `RevivalPool`, depende do `IWebviewService` (que cria as sobreposições de webview,
//!   recorte da Fase 6) e de `IEditorService.openEditor` com uma entrada tipada e grupo preferido, que o [`IEditorService`]
//!   do porte ainda não expõe.
//! - `LazilyResolvedWebviewEditorInput extends WebviewInput` é um [`WebviewInputKind`] (ver a nota de herança de
//!   [`WebviewInput`]).
//! - `IEditorGroup | GroupIdentifier | ACTIVE_GROUP_TYPE | SIDE_GROUP_TYPE` é o [`GroupRef`] (o grupo ou o identificador, em
//!   que `ACTIVE_GROUP` e `SIDE_GROUP` são identificadores especiais).
//! - `instanceof WebviewInput` das entradas é a entrada concreta `Rc<WebviewInput>`.
//!
//! [`IEditorService`]: crate::services::editor::editor_service::IEditorService

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::{async_fn, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::is_cancellation_error;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::{CancelablePromise, Promise};

use super::webview_editor_input::{IWebviewThemeService, WebviewIconPath, WebviewInput, WebviewInputInitInfo, WebviewInputKind};
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::editor_model::IEditorModel;
use crate::contrib::webview::browser_webview::{IOverlayWebview, WebviewInitInfo};
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

/// O estado de `LazilyResolvedWebviewEditorInput` que as continuações assíncronas compartilham.
struct LazilyResolvedState {
    resolved: Cell<bool>,
    resolve_promise: RefCell<Option<CancelablePromise<()>>>,
    webview_workbench_service: Rc<dyn IWebviewWorkbenchService>,
}

/// `LazilyResolvedWebviewEditorInput`: uma `WebviewInput` que se resolve ao ser aberta, pelo serviço (uma vez; a tentativa
/// que falha, fora a de uma entrada descartada, pode ser repetida). Como o resto da família, é a tabela virtual da entrada
/// ([`WebviewInputKind`]); as subclasses (`CustomEditorInput`...) a guardam e chamam [`Self::resolve`], [`Self::dispose`] e
/// [`Self::transfer`] no lugar do `super`.
pub struct LazilyResolvedWebviewEditorInput {
    state: Rc<LazilyResolvedState>,
}

impl LazilyResolvedWebviewEditorInput {
    /// `constructor(init, webview, themeService, webviewWorkbenchService)`, só a parte desta classe.
    pub fn new(webview_workbench_service: Rc<dyn IWebviewWorkbenchService>) -> Self {
        Self { state: Rc::new(LazilyResolvedState { resolved: Cell::new(false), resolve_promise: RefCell::new(None), webview_workbench_service }) }
    }

    /// `new LazilyResolvedWebviewEditorInput(init, webview, themeService, webviewWorkbenchService)`.
    pub fn create(
        init: WebviewInputInitInfo,
        webview: Rc<dyn IOverlayWebview>,
        theme_service: Rc<dyn IWebviewThemeService>,
        webview_workbench_service: Rc<dyn IWebviewWorkbenchService>,
    ) -> Rc<WebviewInput> {
        WebviewInput::with_kind(init, webview, theme_service, Rc::new(Self::new(webview_workbench_service)))
    }

    /// `_resolved`.
    pub fn is_resolved(&self) -> bool {
        self.state.resolved.get()
    }

    /// `_resolved = value`.
    pub fn set_resolved(&self, value: bool) {
        self.state.resolved.set(value);
    }

    /// O `resolve()` até o `await` da resolução do webview (o `super.resolve()` que fecha o método fica com quem chama).
    pub fn resolve(&self, input: &Rc<WebviewInput>) -> Promise<()> {
        let state = self.state.clone();
        if !state.resolved.get() {
            state.resolved.set(true);
            let service = state.webview_workbench_service.clone();
            let input = input.clone();
            *state.resolve_promise.borrow_mut() = Some(create_cancelable_promise(move |token| service.resolve_webview(input, token)));
        }

        let Some(resolve_promise) = state.resolve_promise.borrow().clone() else {
            return Promise::resolved(());
        };
        let input = input.clone();
        async_fn(async move {
            if let Err(error) = resolve_promise.clone().await {
                let disposed_while_resolving = is_cancellation_error(&error) && input.is_disposed();
                if !disposed_while_resolving {
                    // Allow resolving again, for example when the user retries opening the editor
                    let is_current = state.resolve_promise.borrow().as_ref().is_some_and(|current| current.ptr_eq(&resolve_promise));
                    if is_current {
                        state.resolved.set(false);
                        *state.resolve_promise.borrow_mut() = None;
                    }
                    return Err(error);
                }
            }
            Ok(())
        })
    }

    /// O que `dispose()` faz depois do `super.dispose()`: cancela a resolução em andamento.
    pub fn dispose(&self) {
        let resolve_promise = self.state.resolve_promise.borrow_mut().take();
        if let Some(resolve_promise) = resolve_promise {
            resolve_promise.cancel();
        }
    }

    /// O que `transfer(other)` leva para `other` (`other._resolved = this._resolved`).
    pub fn transfer(&self, other: &LazilyResolvedWebviewEditorInput) {
        other.set_resolved(self.is_resolved());
    }
}

impl WebviewInputKind for LazilyResolvedWebviewEditorInput {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn resolve(&self, input: &WebviewInput) -> Promise<Option<Rc<dyn IEditorModel>>> {
        LazilyResolvedWebviewEditorInput::resolve(self, &input.rc()).then(|()| None)
    }

    fn on_dispose(&self, _input: &WebviewInput) {
        self.dispose();
    }

    fn on_transfer(&self, _input: &WebviewInput, other: &Rc<WebviewInput>) {
        if let Some(other) = other.kind_as::<LazilyResolvedWebviewEditorInput>() {
            self.transfer(other);
        }
    }
}
