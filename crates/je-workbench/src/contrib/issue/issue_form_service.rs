//! Porte de `src/vs/workbench/contrib/issue/browser/issueFormService.ts`: o serviço que abre o relator de problemas
//! numa janela auxiliar e busca issues parecidas no GitHub, no recorte que o teste `issueFormService.test.ts` exercita.
//!
//! Recorte e divergências:
//! - **Sem DOM.** O je-code não tem `Window`, `document` nem HTML. A janela da janela auxiliar é o
//!   [`IssueReporterWindow`] (só o que o serviço usa: o `beforeunload`, `focus` e `close`). Ficam de fora de
//!   `openAuxIssueReporter` o título do documento, as classes do `body`, a remoção do contêiner, a folha de estilo do
//!   `Menu` e o `safeSetInnerHtml(BaseHtml())`; sobra a espera de `whenStylesHaveLoaded`, a posição centralizada, o
//!   registro da janela e o tratamento do `beforeunload`.
//! - **Portado:** `searchGitHubIssues`, `openReporter`, `openAuxIssueReporterLegacy`, `openAuxIssueReporter`,
//!   `closeReporter`, `hasToReload` e os campos protegidos (`currentData`, `issueReporterWindow`,
//!   `extensionIdentifierSet`, `arch`, `release`, `type`).
//! - **Fora do recorte (serviços ainda não portados):** `submitIssue` e tudo que ele puxa (envio de anexos, URL da
//!   prévia, `extractIssueData`, `IGitHubUploadService`, `IFileService`, `IClipboardService`, `IOpenerService`),
//!   `sendReporterMenu` (`IMenuService` e `IContextKeyService`), `showConfirmCloseDialog` e `showClipboardDialog`
//!   (`IDialogService`) e `reloadWithExtensionsDisabled` (`IHostService`).
//! - **Camada do GitHub da plataforma ainda não portada:** [`GitHubAnonymousClient`] (só `search.issues`, o que o
//!   serviço usa), [`WorkbenchGitHubService`] (`acquireAnonymousApiClient`), [`ApiRequestOptions`] (`caller` e
//!   `deadline`) e os erros [`github_request_error`] (`GitHubRequestError`) e [`schema_error`] (`SchemaError`) são os
//!   mínimos. A página da busca devolve só [`SimilarIssue`], que é o que `searchGitHubIssues` expõe (`page.items`).
//! - **`IAuxiliaryWindowService`** é [`AuxiliaryWindowService`], com [`AuxiliaryWindowOptions`] e [`AuxiliaryWindow`]. O
//!   `open` do original devolve sempre uma janela, então o ramo `console.error('Failed to open auxiliary window')` não existe.
//! - **`IssueWebReporter`** (`issueReporterService.ts`, o relator legado com DOM) não está portado: o serviço o cria por
//!   [`IssueWebReporterFactory`], o que o `createInstance(IssueWebReporter, ...)` faz, e o usa como [`IssueWebReporter`]
//!   (`render` e `dispose`). O `product` global é o [`IssueReporterProduct`] das dependências.
//! - **`AbortSignal.any`** é [`abort_signal_any`], que também devolve o descarte que solta as escutas dos sinais de origem
//!   (no JavaScript o coletor de lixo faz isso). `abortOnDispose(this._store)` é `abort_on_dispose` do store do serviço.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::cancellation::{AbortController, AbortSignal, abort_on_dispose};
use je_base::errors::Error;
use je_base::event::{DomEventEmitter, Listener};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, none, plain_disposable};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_extensions::manifest::ExtensionIdentifierSet;
use je_platform::log::{LogArg, LogMessage, LogServiceRef};

use super::base_issue_reporter_service::{GitHubIssueSearch, IssueReporterProduct, OsInfo};
use super::issue_common::{IssueReporterData, SimilarIssue};

/// A largura da janela do relator.
const REPORTER_WIDTH: f64 = 700.0;

/// A altura da janela do relator.
const REPORTER_HEIGHT: f64 = 800.0;

/// O prazo absoluto da busca de issues, em milissegundos a partir do pedido.
const SEARCH_DEADLINE_MS: u64 = 10_000;

/// `IRectangle`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `Partial<IRectangle>`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PartialRectangle {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// `AuxiliaryWindowMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxiliaryWindowMode {
    Maximized,
    Normal,
    Fullscreen,
}

/// `IAuxiliaryWindowOpenOptions`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuxiliaryWindowOptions {
    pub bounds: Option<PartialRectangle>,
    pub mode: Option<AuxiliaryWindowMode>,
    pub native_titlebar: Option<bool>,
    pub disable_fullscreen: Option<bool>,
}

/// A janela (`CodeWindow`) do relator, no que o serviço usa dela.
pub trait IssueReporterWindow {
    /// `window.addEventListener('beforeunload', listener)`.
    fn add_before_unload_listener(&self, listener: Rc<dyn Fn()>);
    fn focus(&self);
    fn close(&self);
}

/// `IAuxiliaryWindow`: a janela e a espera pelos estilos.
pub trait AuxiliaryWindow: IDisposable {
    fn window(&self) -> Rc<dyn IssueReporterWindow>;
    /// `whenStylesHaveLoaded`.
    fn when_styles_have_loaded(&self) -> Promise<()>;
}

/// `IAuxiliaryWindowService`.
pub trait AuxiliaryWindowService {
    fn open(&self, options: AuxiliaryWindowOptions) -> Promise<Rc<dyn AuxiliaryWindow>>;
}

/// `ApiRequestOptions`: a origem do pedido e o prazo absoluto (milissegundos desde 1970).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApiRequestOptions {
    pub caller: Option<String>,
    pub deadline: Option<u64>,
}

/// A página de `search.issues`: só os itens que o serviço devolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchIssuesPage {
    pub total_count: u64,
    pub incomplete_results: bool,
    pub items: Vec<SimilarIssue>,
}

/// `IGitHubAnonymousClient`: um cliente descartável de dados públicos do GitHub; o serviço usa a busca de issues.
pub trait GitHubAnonymousClient: IDisposable {
    /// `search.issues(query, signal, options)`.
    fn search_issues(&self, query: &str, signal: AbortSignal, options: ApiRequestOptions) -> Promise<SearchIssuesPage>;
}

/// `IWorkbenchGitHubService`: o serviço entrega um cliente anônimo por chamada.
pub trait WorkbenchGitHubService {
    fn acquire_anonymous_api_client(&self, signal: AbortSignal) -> Promise<Rc<dyn GitHubAnonymousClient>>;
}

/// `new GitHubRequestError(message, kind, statusCode)`.
pub fn github_request_error(message: &str, kind: &str, status_code: Option<u16>) -> Error {
    let error = Error::named("GitHubRequestError", message).with_prop("kind", kind);
    match status_code {
        Some(status_code) => error.with_prop("statusCode", status_code),
        None => error,
    }
}

/// `new SchemaError(message)`.
pub fn schema_error(message: &str) -> Error {
    Error::named("SchemaError", message)
}

/// O relator legado (`IssueWebReporter`): desenha e descarta.
pub trait IssueWebReporter: IDisposable {
    fn render(&self);
}

/// `instantiationService.createInstance(IssueWebReporter, disableExtensions, data, os, product, window)`.
pub trait IssueWebReporterFactory {
    fn create(&self, disable_extensions: bool, data: &IssueReporterData, os: OsInfo, product: &IssueReporterProduct, window: Rc<dyn IssueReporterWindow>) -> Rc<dyn IssueWebReporter>;
}

/// O que os `@I...Service` do construtor entregam, no recorte portado.
pub struct IssueFormServiceDeps {
    pub auxiliary_window_service: Rc<dyn AuxiliaryWindowService>,
    pub git_hub_service: Rc<dyn WorkbenchGitHubService>,
    pub log_service: LogServiceRef,
    pub web_reporter_factory: Rc<dyn IssueWebReporterFactory>,
    pub product: IssueReporterProduct,
}

/// `AbortSignal.any(signals)`: o sinal agregado e o descarte que solta as escutas dos sinais de origem. Um sinal já
/// abortado devolve um sinal abortado com o mesmo motivo.
pub fn abort_signal_any(signals: &[AbortSignal]) -> (AbortSignal, DisposableRef) {
    if let Some(aborted) = signals.iter().find(|signal| signal.aborted()) {
        return (AbortSignal::abort(aborted.reason()), none());
    }
    let controller = AbortController::new();
    let attached: Vec<(AbortSignal, Listener<()>)> = signals
        .iter()
        .map(|source| {
            let target = controller.clone();
            let origin = source.clone();
            let listener: Listener<()> = Rc::new(move |_: &()| target.abort(origin.reason()));
            source.add_event_listener("abort", listener.clone());
            (source.clone(), listener)
        })
        .collect();
    let release = plain_disposable(move || {
        for (source, listener) in &attached {
            source.remove_event_listener("abort", listener);
        }
    });
    (controller.signal(), release)
}

pub struct IssueFormService {
    store: Rc<DisposableStore>,
    me: Weak<Self>,
    auxiliary_window_service: Rc<dyn AuxiliaryWindowService>,
    git_hub_service: Rc<dyn WorkbenchGitHubService>,
    log_service: LogServiceRef,
    web_reporter_factory: Rc<dyn IssueWebReporterFactory>,
    product: IssueReporterProduct,
    dispose_signal: AbortSignal,
    pub current_data: RefCell<Option<IssueReporterData>>,
    pub issue_reporter_window: RefCell<Option<Rc<dyn IssueReporterWindow>>>,
    pub extension_identifier_set: RefCell<ExtensionIdentifierSet>,
    pub os: RefCell<OsInfo>,
}

impl IssueFormService {
    pub fn new(deps: IssueFormServiceDeps) -> Rc<Self> {
        let store = DisposableStore::new();
        let dispose_signal = abort_on_dispose(&store);
        Rc::new_cyclic(|me| Self {
            store,
            me: me.clone(),
            auxiliary_window_service: deps.auxiliary_window_service,
            git_hub_service: deps.git_hub_service,
            log_service: deps.log_service,
            web_reporter_factory: deps.web_reporter_factory,
            product: deps.product,
            dispose_signal,
            current_data: RefCell::new(None),
            issue_reporter_window: RefCell::new(None),
            extension_identifier_set: RefCell::new(ExtensionIdentifierSet::default()),
            os: RefCell::new(OsInfo { r#type: String::new(), arch: String::new(), release: String::new() }),
        })
    }

    pub fn open_reporter(&self, data: IssueReporterData) -> Promise<()> {
        if self.has_to_reload(&data) {
            return Promise::resolved(());
        }

        // A web só abre o relator legado. O painel do assistente é registrado só pela contribuição nativa, e as
        // dependências dele (captura de tela, gravação, envio ao GitHub, diagnóstico nativo) não existem na web.
        self.open_aux_issue_reporter_legacy(data)
    }

    /// Abre o relator clássico, sem o assistente, numa janela auxiliar.
    pub fn open_aux_issue_reporter_legacy(&self, data: IssueReporterData) -> Promise<()> {
        let target = self.me.clone();
        self.open_aux_issue_reporter(data.clone(), None).then(move |disposables| {
            let Some(service) = target.upgrade() else { return };
            let window = service.issue_reporter_window.borrow().clone();
            if let Some(window) = window {
                let os = service.os.borrow().clone();
                let issue_reporter = service.web_reporter_factory.create(false, &data, os, &service.product, window);
                disposables.add(Rc::new(issue_reporter.clone()));
                issue_reporter.render();
            }
        })
    }

    pub fn open_aux_issue_reporter(&self, _data: IssueReporterData, bounds: Option<Rectangle>) -> Promise<Rc<DisposableStore>> {
        let mut issue_reporter_bounds = PartialRectangle { width: Some(REPORTER_WIDTH), height: Some(REPORTER_HEIGHT), ..PartialRectangle::default() };

        // Centraliza a janela do relator pelos limites que o serviço nativo do host entrega. O `typeof` do original
        // (e não a verdade do número) deixa uma janela ativa em x:0 / y:0 também ser centralizada.
        if let Some(bounds) = bounds {
            let center_x = bounds.x + bounds.width / 2.0;
            let center_y = bounds.y + bounds.height / 2.0;
            issue_reporter_bounds.x = Some(center_x - REPORTER_WIDTH / 2.0);
            issue_reporter_bounds.y = Some(center_y - REPORTER_HEIGHT / 2.0);
        }

        let disposables = DisposableStore::new();
        let target = self.me.clone();
        let options = AuxiliaryWindowOptions {
            bounds: Some(issue_reporter_bounds),
            mode: Some(AuxiliaryWindowMode::Normal),
            native_titlebar: Some(true),
            disable_fullscreen: Some(true),
        };

        self.auxiliary_window_service.open(options).then_promise(move |auxiliary_window| {
            disposables.add(Rc::new(auxiliary_window.clone()));
            auxiliary_window.when_styles_have_loaded().then(move |()| {
                if let Some(service) = target.upgrade() {
                    *service.issue_reporter_window.borrow_mut() = Some(auxiliary_window.window());

                    // trata o fechamento do relator
                    let window = service.issue_reporter_window.borrow().clone();
                    if let Some(window) = window {
                        let closing = target.clone();
                        let store = disposables.clone();
                        window.add_before_unload_listener(Rc::new(move || {
                            auxiliary_window.window().close();
                            store.dispose();
                            if let Some(service) = closing.upgrade() {
                                *service.issue_reporter_window.borrow_mut() = None;
                            }
                        }));
                    }
                }
                disposables
            })
        })
    }

    pub fn close_reporter(&self) {
        let window = self.issue_reporter_window.borrow().clone();
        if let Some(window) = window {
            window.close();
        }
    }

    pub fn has_to_reload(&self, data: &IssueReporterData) -> bool {
        if let Some(extension_id) = data.extension_id.as_ref().filter(|id| !id.is_empty()) {
            if self.extension_identifier_set.borrow().has(extension_id) {
                *self.current_data.borrow_mut() = Some(data.clone());
                self.focus_window();
                return true;
            }
        }

        if self.issue_reporter_window.borrow().is_some() {
            self.focus_window();
            return true;
        }

        false
    }

    fn focus_window(&self) {
        let window = self.issue_reporter_window.borrow().clone();
        if let Some(window) = window {
            window.focus();
        }
    }
}

impl GitHubIssueSearch for IssueFormService {
    fn search_github_issues(&self, repo: &str, title: &str, signal: AbortSignal) -> Promise<Vec<SimilarIssue>> {
        let (signal, release_signals) = abort_signal_any(&[signal, self.dispose_signal.clone()]);
        if signal.aborted() {
            release_signals.dispose();
            return Promise::rejected(signal.reason().unwrap_or_else(Error::cancellation));
        }
        let lifetime = DisposableStore::new();
        let query = format!("repo:{repo} {title}");
        let log_service = self.log_service.clone();
        let search_signal = signal.clone();
        let failed_signal = signal.clone();
        let search_lifetime = lifetime.clone();

        self.git_hub_service
            .acquire_anonymous_api_client(signal)
            .then_promise(move |client| {
                search_lifetime.add(Rc::new(client.clone()));
                let options = ApiRequestOptions { caller: Some("github.query".to_string()), deadline: Some(date_now() + SEARCH_DEADLINE_MS) };
                client.search_issues(&query, search_signal, options).then(|page| page.items)
            })
            .then_result(move |result| {
                if let Err(error) = &result {
                    if !failed_signal.aborted() {
                        log_service.warn_args(&LogMessage::from("[IssueFormService] GitHub issue search failed"), &[LogArg::from(error)]);
                    }
                }
                result
            })
            .finally(move || {
                lifetime.dispose();
                release_signals.dispose();
            })
    }
}

impl IDisposable for IssueFormService {
    fn dispose(&self) {
        self.store.dispose();
    }
}
