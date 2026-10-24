//! Porte de `src/vs/workbench/contrib/issue/browser/baseIssueReporterService.ts`: a lógica do relator de problemas
//! que o teste `baseIssueReporterService.test.ts` exercita, a busca de problemas parecidos (GitHub e duplicados).
//!
//! Recorte e divergências:
//! - **Sem DOM.** O je-code não tem `Window` nem elementos. Os campos que a lógica lê e escreve são os modelos
//!   mínimos de [`IssueReporterElements`]: [`FormField`] (o `input#issue-title`, o `textarea#description` e o
//!   `select#issue-source`: um valor e a escuta do evento `input`) e [`SimilarIssuesView`] (o `div#similar-issues`: a
//!   lista de links que `displaySearchResults` desenha). `innerText = ''` é [`SimilarIssuesView::clear`].
//! - **Portado:** o construtor sem a parte visual (modelo com versão e SO, `fileOnMarketplace`/`fileOnProduct`, título e
//!   corpo iniciais), `searchVSCodeIssues`, `searchIssues`, `searchExtensionIssues`, `searchMarketplaceIssues`,
//!   `clearSearchResults`, `searchGitHub`, `searchDuplicates`, `displaySearchResults`, `parseGitHubUrl`,
//!   `getExtensionGitHubUrl`, o `dispose`, e do `setEventHandlers` as escutas de `description` e `issue-title` (`input`).
//! - **Fora do recorte (camada de janela e de widgets ainda não portada):** botões e menus (`Button`,
//!   `ButtonWithDropdown`, `Action`), a folha de estilo de ícones, a autenticação do GitHub, a lista de extensões, a
//!   criação e a prévia da issue, o rótulo de tamanho da URL da escuta de `issue-title` e as demais escutas de
//!   `setEventHandlers`.
//! - **`@debounce(300)`** é [`Debounce`] (um por método, como o decorador guarda um timer por método e instância).
//!   `AbortController` e `AbortSignal` são os de `je_base::cancellation`.
//! - **`IIssueFormService.searchGitHubIssues`** é [`GitHubIssueSearch`] (só o método que o relator usa) e o `fetch` é
//!   [`Fetcher`], com [`FetchInit`] e [`FetchResponse`] (`response.json()`).
//! - **`IProductConfiguration`** vira [`IssueReporterProduct`], só os campos que o relator lê, com a conversão a partir
//!   da configuração completa.
//! - `data.extensionId`, `issueTitle` e `issueBody` vazios valem como ausentes (a verdade de um texto em JavaScript).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::cancellation::{AbortController, AbortSignal};
use je_base::decorators::Debounce;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable, to_disposable};
use je_base::product::IProductConfiguration;
use je_base::promise::Promise;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};

use super::issue_common::{IssueReporterData, IssueReporterExtensionData, IssueSource, IssueType, SimilarIssue};
use super::issue_reporter_model::{IssueReporterData as IssueReporterModelData, IssueReporterModel, VersionInfo};
use super::issue_reporter_util::normalize_github_url;

/// O endereço do serviço de duplicados.
pub const DUPLICATE_CANDIDATES_URL: &str = "https://vscode-probot.westus.cloudapp.azure.com:7890/duplicate_candidates";

/// O atraso do `@debounce(300)` das buscas.
const SEARCH_DEBOUNCE_MS: u64 = 300;

/// O máximo de resultados que `displaySearchResults` mostra.
const MAX_RESULTS_DISPLAYED: usize = 5;

/// `{ type, arch, release }`: o sistema operacional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsInfo {
    pub r#type: String,
    pub arch: String,
    pub release: String,
}

/// Os campos do `IProductConfiguration` que o relator lê.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueReporterProduct {
    pub name_short: String,
    pub version: String,
    pub commit: Option<String>,
    pub date: Option<String>,
    pub darwin_universal_asset_id: Option<String>,
    pub report_marketplace_issue_url: Option<String>,
}

impl From<&IProductConfiguration> for IssueReporterProduct {
    fn from(product: &IProductConfiguration) -> Self {
        Self {
            name_short: product.name_short.clone(),
            version: product.version.clone(),
            commit: product.commit.clone(),
            date: product.date.clone(),
            darwin_universal_asset_id: product.darwin_universal_asset_id.clone(),
            report_marketplace_issue_url: product.report_marketplace_issue_url.clone(),
        }
    }
}

/// O `IIssueFormService.searchGitHubIssues`: busca issues parecidas num repositório, cancelável pelo sinal.
pub trait GitHubIssueSearch {
    fn search_github_issues(&self, repo: &str, title: &str, signal: AbortSignal) -> Promise<Vec<SimilarIssue>>;
}

/// O `init` que o relator passa ao `fetch`.
#[derive(Clone)]
pub struct FetchInit {
    pub method: String,
    pub body: String,
    pub headers: Vec<(String, String)>,
    pub signal: AbortSignal,
}

/// O `{ candidates?: readonly ISimilarIssue[] }` que o serviço de duplicados responde.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuplicateCandidates {
    pub candidates: Option<Vec<SimilarIssue>>,
}

/// O `Response` do `fetch`: o relator só lê `json()` (`null` é `None`).
pub trait FetchResponse {
    fn json(&self) -> Promise<Option<DuplicateCandidates>>;
}

/// `typeof fetch`.
pub type Fetcher = Rc<dyn Fn(&str, FetchInit) -> Promise<Rc<dyn FetchResponse>>>;

/// Um campo de formulário: o valor e a escuta do evento `input`.
#[derive(Default)]
pub struct FormField {
    value: RefCell<String>,
    input_listeners: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl FormField {
    pub fn new(value: &str) -> Rc<Self> {
        Rc::new(Self { value: RefCell::new(value.to_string()), input_listeners: RefCell::new(Vec::new()) })
    }

    /// `element.value`.
    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    pub fn set_value(&self, value: &str) {
        *self.value.borrow_mut() = value.to_string();
    }

    /// `element.addEventListener('input', listener)`.
    pub fn add_input_listener(&self, listener: Rc<dyn Fn()>) {
        self.input_listeners.borrow_mut().push(listener);
    }

    /// `element.dispatchEvent(new Event('input'))`.
    pub fn dispatch_input(&self) {
        let listeners = self.input_listeners.borrow().clone();
        for listener in listeners {
            listener();
        }
    }
}

/// Um item de `div.issue`: o link e o estado (`open` ou `closed`) quando o resultado traz um.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimilarIssueItem {
    pub title: String,
    pub href: String,
    pub state: Option<String>,
}

/// O `div#similar-issues`: o título da lista e os itens.
#[derive(Default)]
pub struct SimilarIssuesView {
    items: RefCell<Vec<SimilarIssueItem>>,
}

impl SimilarIssuesView {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// `innerText = ''`.
    pub fn clear(&self) {
        self.items.borrow_mut().clear();
    }

    pub fn items(&self) -> Vec<SimilarIssueItem> {
        self.items.borrow().clone()
    }

    /// O `textContent` de cada `a` (`querySelectorAll('a')`).
    pub fn link_titles(&self) -> Vec<String> {
        self.items.borrow().iter().map(|item| item.title.clone()).collect()
    }

    /// O `textContent` do contêiner: o título da lista, e por item o rótulo do estado e o título do link.
    pub fn text_content(&self) -> String {
        let items = self.items.borrow();
        if items.is_empty() {
            return String::new();
        }
        let mut text = String::from("Similar issues");
        for item in items.iter() {
            match item.state.as_deref() {
                Some("open") => text.push_str("Open"),
                Some(_) => text.push_str("Closed"),
                None => {}
            }
            text.push_str(&item.title);
        }
        text
    }

    fn show(&self, items: Vec<SimilarIssueItem>) {
        self.items.borrow_mut().extend(items);
    }
}

/// Os elementos que a lógica do relator lê e escreve.
#[derive(Clone)]
pub struct IssueReporterElements {
    pub issue_title: Rc<FormField>,
    pub description: Rc<FormField>,
    pub issue_source: Rc<FormField>,
    pub similar_issues: Rc<SimilarIssuesView>,
}

/// O que o `@IIssueFormService`, o `@ILogService` e o `fetcher` do construtor entregam, mais os elementos.
pub struct IssueReporterDeps {
    pub issue_form_service: Rc<dyn GitHubIssueSearch>,
    pub log_service: LogServiceRef,
    pub fetcher: Fetcher,
    pub elements: IssueReporterElements,
}

type SearchGitHubArgs = (String, String, u64);
type SearchDuplicatesArgs = (String, Option<String>, u64);

/// `isLinuxSnap`.
fn is_linux_snap() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("SNAP").is_some() && std::env::var_os("SNAP_REVISION").is_some()
}

/// A verdade de um texto opcional em JavaScript: nem ausente, nem vazio.
fn non_empty(value: Option<&String>) -> Option<&String> {
    value.filter(|text| !text.is_empty())
}

/// O que sobra de `https://github.com/` ou `http://github.com/` no começo da URL.
fn github_path(url: &str) -> Option<&str> {
    url.strip_prefix("https://github.com/").or_else(|| url.strip_prefix("http://github.com/"))
}

/// A parte de `repo` depois do primeiro `/` (inclusive ele), ou vazia, de `owner/repo...`; `None` se faltar o `/` do dono.
fn github_repo_tail(url: &str) -> Option<&str> {
    let (_owner, after) = github_path(url)?.split_once('/')?;
    Some(after.find('/').map_or("", |index| &after[index..]))
}

/// `/^https?:\/\/github\.com\/([^\/]*)\/([^\/]*)\/?(\/issues)?$/`.
fn matches_github_bugs_url(url: &str) -> bool {
    matches!(github_repo_tail(url), Some("" | "/" | "/issues" | "//issues"))
}

/// `/^https?:\/\/github\.com\/([^\/]*)\/([^\/]*)$/`.
fn matches_github_repository_url(url: &str) -> bool {
    github_repo_tail(url) == Some("")
}

/// `/^https?:\/\/github\.com\/(.*)/` e o grupo 1: o `.` não passa de uma quebra de linha.
fn github_repository_path(url: &str) -> Option<&str> {
    let rest = github_path(url)?;
    let end = rest.find(['\n', '\r', '\u{2028}', '\u{2029}']).unwrap_or(rest.len());
    Some(&rest[..end])
}

pub struct BaseIssueReporterService {
    base: Rc<Disposable>,
    me: Weak<Self>,
    pub issue_reporter_model: Rc<IssueReporterModel>,
    pub number_of_search_results_displayed: Cell<usize>,
    pub non_github_issue_url: Cell<bool>,
    pub product: IssueReporterProduct,
    pub os: OsInfo,
    pub is_web: bool,
    elements: IssueReporterElements,
    issue_form_service: Rc<dyn GitHubIssueSearch>,
    log_service: LogServiceRef,
    fetcher: Fetcher,
    similar_issue_search: Rc<MutableDisposable<dyn IDisposable>>,
    similar_issue_search_generation: Cell<u64>,
    disposed: Cell<bool>,
    search_github_debounce: Debounce<SearchGitHubArgs, SearchGitHubArgs>,
    search_duplicates_debounce: Debounce<SearchDuplicatesArgs, SearchDuplicatesArgs>,
}

impl BaseIssueReporterService {
    /// O construtor sem a parte visual (ver o cabeçalho do módulo).
    pub fn new(disable_extensions: bool, data: &IssueReporterData, os: OsInfo, product: IssueReporterProduct, is_web: bool, deps: IssueReporterDeps) -> Rc<Self> {
        let target_extension: Option<IssueReporterExtensionData> = non_empty(data.extension_id.as_ref()).and_then(|extension_id| {
            data.enabled_extensions.iter().find(|extension| extension.id.to_lowercase() == extension_id.to_lowercase()).cloned()
        });
        let base = Disposable::new();
        let universal = non_empty(product.darwin_universal_asset_id.as_ref()).is_some();
        let product_version = if universal { format!("{} (Universal)", product.version) } else { product.version.clone() };
        let version_info = VersionInfo {
            vscode_version: format!(
                "{} {} ({}, {})",
                product.name_short,
                product_version,
                non_empty(product.commit.as_ref()).map_or("Commit unknown", String::as_str),
                non_empty(product.date.as_ref()).map_or("Date unknown", String::as_str),
            ),
            os: format!("{} {} {}{}", os.r#type, os.arch, os.release, if is_linux_snap() { " snap" } else { "" }),
        };
        let file_on_extension = non_empty(data.extension_id.as_ref()).map(|_| !target_extension.as_ref().is_some_and(|extension| extension.is_builtin));
        let model = IssueReporterModel::new(Some(IssueReporterModelData {
            issue_type: data.issue_type.unwrap_or(IssueType::Bug),
            issue_source: data.issue_source,
            issue_title: data.issue_title.clone(),
            version_info: Some(version_info),
            extensions_disabled: Some(disable_extensions),
            file_on_extension,
            selected_extension: target_extension,
            restricted_mode: Some(data.restricted_mode),
            is_installation_pure: Some(data.is_installation_pure),
            is_sessions_window: Some(data.is_sessions_window),
            ..IssueReporterModelData::default()
        }));
        base.register(model.clone());
        let similar_issue_search = MutableDisposable::<dyn IDisposable>::new();
        base.register(similar_issue_search.clone());

        let service = Rc::new_cyclic(|me: &Weak<Self>| {
            let github_target = me.clone();
            let duplicates_target = me.clone();
            Self {
                base,
                me: me.clone(),
                issue_reporter_model: model,
                number_of_search_results_displayed: Cell::new(0),
                non_github_issue_url: Cell::new(false),
                product,
                os,
                is_web,
                elements: deps.elements,
                issue_form_service: deps.issue_form_service,
                log_service: deps.log_service,
                fetcher: deps.fetcher,
                similar_issue_search,
                similar_issue_search_generation: Cell::new(0),
                disposed: Cell::new(false),
                search_github_debounce: Debounce::without_reducer(SEARCH_DEBOUNCE_MS, move |(repo, title, generation): SearchGitHubArgs| {
                    if let Some(service) = github_target.upgrade() {
                        service.search_github(&repo, &title, generation);
                    }
                }),
                search_duplicates_debounce: Debounce::without_reducer(SEARCH_DEBOUNCE_MS, move |(title, body, generation): SearchDuplicatesArgs| {
                    if let Some(service) = duplicates_target.upgrade() {
                        service.search_duplicates(&title, body.as_deref(), generation);
                    }
                }),
            }
        });

        let file_on_marketplace = data.issue_source == Some(IssueSource::Marketplace);
        let file_on_product = data.issue_source == Some(IssueSource::VSCode);
        service.issue_reporter_model.update(|model_data| {
            model_data.file_on_marketplace = Some(file_on_marketplace);
            model_data.file_on_product = Some(file_on_product);
        });

        if let Some(issue_title) = non_empty(data.issue_title.as_ref()) {
            service.elements.issue_title.set_value(issue_title);
        }
        if let Some(issue_body) = non_empty(data.issue_body.as_ref()) {
            service.elements.description.set_value(issue_body);
            service.issue_reporter_model.update(|model_data| model_data.issue_description = Some(issue_body.clone()));
        }
        service
    }

    /// `setEventHandlers`, as escutas de `description` e `issue-title`.
    pub fn set_event_handlers(&self) {
        let description = self.elements.description.clone();
        let target = self.me.clone();
        self.elements.description.add_input_listener(Rc::new(move || {
            let Some(service) = target.upgrade() else { return };
            let issue_description = description.value();
            service.issue_reporter_model.update(|data| data.issue_description = Some(issue_description.clone()));

            let file_on_marketplace = service.issue_reporter_model.get_data().file_on_marketplace.unwrap_or(false);
            if service.issue_reporter_model.file_on_extension() == Some(false) && !file_on_marketplace {
                let title = service.elements.issue_title.value();
                service.search_vscode_issues(&title, Some(&issue_description));
            }
        }));

        let issue_title = self.elements.issue_title.clone();
        let target = self.me.clone();
        self.elements.issue_title.add_input_listener(Rc::new(move || {
            let Some(service) = target.upgrade() else { return };
            let title = issue_title.value();
            service.issue_reporter_model.update(|data| data.issue_title = Some(title));
        }));

        let issue_title = self.elements.issue_title.clone();
        let target = self.me.clone();
        self.elements.issue_title.add_input_listener(Rc::new(move || {
            let Some(service) = target.upgrade() else { return };
            let title = issue_title.value();
            if service.elements.issue_source.value().is_empty() {
                return;
            }
            let (file_on_extension, file_on_marketplace) = {
                let data = service.issue_reporter_model.get_data();
                (data.file_on_extension, data.file_on_marketplace)
            };
            service.search_issues(&title, file_on_extension, file_on_marketplace);
        }));
    }

    /// `getExtensionBugsUrl`.
    pub fn get_extension_bugs_url(&self) -> Option<String> {
        self.issue_reporter_model.get_data().selected_extension.as_ref().and_then(|extension| extension.bugs_url.clone())
    }

    fn get_extension_repository_url(&self) -> Option<String> {
        self.issue_reporter_model.get_data().selected_extension.as_ref().and_then(|extension| extension.repository_url.clone())
    }

    fn get_extension_github_url(&self) -> String {
        let bugs_url = self.get_extension_bugs_url();
        let extension_url = self.get_extension_repository_url();
        let bugs_url = non_empty(bugs_url.as_ref());
        let extension_url = non_empty(extension_url.as_ref());
        if let Some(bugs_url) = bugs_url.filter(|url| matches_github_bugs_url(url)) {
            // casa exatamente com: https://github.com/owner/repo/issues
            normalize_github_url(bugs_url)
        } else if let Some(extension_url) = extension_url.filter(|url| matches_github_repository_url(url)) {
            // casa exatamente com: https://github.com/owner/repo
            normalize_github_url(extension_url)
        } else {
            self.non_github_issue_url.set(true);
            bugs_url.or(extension_url).cloned().unwrap_or_default()
        }
    }

    /// `parseGitHubUrl`: `(owner, repositoryName)` de uma URL de repositório do GitHub.
    pub fn parse_github_url(&self, url: &str) -> Option<(String, String)> {
        let parsed = github_path(url).and_then(|rest| rest.split_once('/')).map(|(owner, after)| {
            let repository_name = after.split('/').next().unwrap_or_default();
            (owner.to_string(), repository_name.to_string())
        });
        if parsed.is_none() {
            eprintln!("No GitHub issues match");
        }
        parsed
    }

    pub fn search_vscode_issues(&self, title: &str, issue_description: Option<&str>) {
        self.clear_search_results();
        if !title.is_empty() {
            self.search_duplicates_debounce.call((title.to_string(), issue_description.map(str::to_string), self.similar_issue_search_generation.get()));
        }
    }

    pub fn search_issues(&self, title: &str, file_on_extension: Option<bool>, file_on_marketplace: Option<bool>) {
        self.clear_search_results();
        if title.is_empty() {
            return;
        }
        if file_on_extension == Some(true) {
            return self.search_extension_issues(title);
        }

        if file_on_marketplace == Some(true) {
            return self.search_marketplace_issues(title);
        }

        let description = self.issue_reporter_model.get_data().issue_description.clone();
        self.search_duplicates_debounce.call((title.to_string(), description, self.similar_issue_search_generation.get()));
    }

    fn search_extension_issues(&self, title: &str) {
        let url = self.get_extension_github_url();
        if !title.is_empty() {
            if let Some(repo) = github_repository_path(&url) {
                return self.search_github_debounce.call((repo.to_string(), title.to_string(), self.similar_issue_search_generation.get()));
            }

            // Se a extensão não tem repositório, mostra o resultado vazio
            if self.issue_reporter_model.get_data().selected_extension.is_some() {
                self.clear_search_results();
                return self.display_search_results(&[]);
            }
        }

        self.clear_search_results();
    }

    fn search_marketplace_issues(&self, title: &str) {
        if !title.is_empty() {
            let url = self.product.report_marketplace_issue_url.as_deref().unwrap_or("undefined");
            if let Some((owner, repository_name)) = self.parse_github_url(url) {
                self.search_github_debounce.call((format!("{owner}/{repository_name}"), title.to_string(), self.similar_issue_search_generation.get()));
            }
        }
    }

    pub fn clear_search_results(&self) {
        self.similar_issue_search_generation.set(self.similar_issue_search_generation.get() + 1);
        self.similar_issue_search.clear();
        self.elements.similar_issues.clear();
        self.number_of_search_results_displayed.set(0);
    }

    /// Registra a busca ativa: o `controller` aborta quando ela é substituída, limpa ou o relator é descartado.
    fn track_active_search(&self) -> AbortController {
        let controller = AbortController::new();
        let aborter = controller.clone();
        self.similar_issue_search.set(Some(to_disposable(move || aborter.abort(None))));
        controller
    }

    fn is_stale(&self, generation: u64) -> bool {
        self.disposed.get() || generation != self.similar_issue_search_generation.get()
    }

    fn warn_unless_aborted(&self, signal: &AbortSignal, message: &str, error: &Error) {
        if !signal.aborted() {
            self.log_service.warn_args(&LogMessage::from(message), &[LogArg::from(error)]);
        }
    }

    fn search_github(&self, repo: &str, title: &str, generation: u64) {
        if self.is_stale(generation) {
            return;
        }
        let controller = self.track_active_search();
        let signal = controller.signal();
        let similar_issues = self.elements.similar_issues.clone();
        let target = self.me.clone();

        self.issue_form_service.search_github_issues(repo, title, signal.clone()).then_result(move |result| {
            let Some(service) = target.upgrade() else { return Ok(()) };
            match result {
                Ok(results) => {
                    if !signal.aborted() {
                        similar_issues.clear();
                        service.display_search_results(&results);
                    }
                }
                Err(error) => service.warn_unless_aborted(&signal, "[IssueReporter] Error fetching GitHub issues", &error),
            }
            Ok(())
        });
    }

    fn search_duplicates(&self, title: &str, body: Option<&str>, generation: u64) {
        if self.is_stale(generation) {
            return;
        }
        let controller = self.track_active_search();
        let signal = controller.signal();
        let payload = Dynamic::object([
            ("title", Dynamic::string(title)),
            ("body", body.map_or(Dynamic::Undefined, Dynamic::string)),
        ]);
        let init = FetchInit {
            method: "POST".to_string(),
            body: json_stringify(&payload).unwrap_or_default(),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            signal: signal.clone(),
        };
        let target = self.me.clone();

        (self.fetcher)(DUPLICATE_CANDIDATES_URL, init).then_promise(|response| response.json()).then_result(move |result| {
            let Some(service) = target.upgrade() else { return Ok(()) };
            let outcome = result.and_then(|parsed| {
                if signal.aborted() {
                    return Ok(None);
                }
                match parsed.and_then(|duplicates| duplicates.candidates) {
                    Some(candidates) => Ok(Some(candidates)),
                    None => Err(Error::new("Unexpected response, no candidates property")),
                }
            });
            match outcome {
                Ok(Some(candidates)) => service.display_search_results(&candidates),
                Ok(None) => {}
                Err(error) => service.warn_unless_aborted(&signal, "[IssueReporter] Error fetching duplicate issues", &error),
            }
            Ok(())
        });
    }

    fn display_search_results(&self, results: &[SimilarIssue]) {
        if results.is_empty() {
            return;
        }
        let displayed = results.len().min(MAX_RESULTS_DISPLAYED);
        self.number_of_search_results_displayed.set(displayed);
        let items = results[..displayed]
            .iter()
            .map(|issue| SimilarIssueItem { title: issue.title.clone(), href: issue.html_url.clone(), state: issue.state.clone() })
            .collect();
        self.elements.similar_issues.show(items);
    }
}

impl IDisposable for BaseIssueReporterService {
    fn dispose(&self) {
        self.disposed.set(true);
        self.base.dispose();
    }
}
