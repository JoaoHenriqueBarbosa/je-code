//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLinkOpeners.ts`: os abridores dos links do terminal
//! (arquivo local, pasta dentro e fora do workspace, busca e URL).
//!
//! Decisões de tradução:
//! - Os serviços injetados (`@IEditorService` e companhia) são parâmetros dos construtores `new`. `async open(link)` é uma
//!   `async fn` interna embrulhada por [`async_fn`]; os abridores são `Clone` (só `Rc` por dentro) para a promessa levar a sua
//!   própria cópia. O erro que o original lança dentro de uma `async` (`throw new Error(...)`) rejeita a promessa devolvida.
//! - `@IQuickInputService` do `TerminalSearchLinkOpener` é só o `quickInputService.quickAccess.show(text)`. O `quickAccess`
//!   (`IQuickAccessController`) ainda não está portado no `IQuickInputService` de je-platform, então o abridor recebe o
//!   [`IQuickAccessController`] (só o `show`) no lugar do serviço.
//! - `this._fileQueryBuilder = instantiationService.createInstance(QueryBuilder)` e o `protected _fileQueryBuilder` que o teste
//!   troca: o abridor recebe um [`IFileQueryBuilder`] (o `QueryBuilder.file`, que o `QueryBuilder` implementa) e
//!   [`TerminalSearchLinkOpener::set_file_query_builder`] troca o campo. O `createInstance` do `QueryBuilder` (que lê seis
//!   serviços) fica a cargo de quem constrói o abridor.
//! - `commandService.executeCommand('revealInExplorer', link.uri)` leva o URI como o objeto `{ $mid, scheme, authority, path,
//!   query, fragment }` (a forma de `URI#toJSON`) num [`Dynamic`], porque os argumentos de comando do porte são `Dynamic`.
//! - `getOS: () => OperatingSystem` é um `Rc<dyn Fn() -> OperatingSystem>`; `IPath` é o [`PathModule`] sobre o ambiente do
//!   processo ([`ProcessEnvironment`]).
//! - As expressões regulares do original (`/^file:\/\/\/?/`, `/^(\.+[\\/])+/`, `/:[^\\/\d][^\d]*$/`, `/\.$/`, `/:\d+(:\d+)?$/` e a
//!   do ISO 8601) rodam no `zyarr`, sobre o texto em UTF-16, e a substituição troca só o primeiro casamento (as regex não
//!   têm a flag `g`).
//! - `text.substring(0, folder.name.length + 1)` conta unidades UTF-16.
//! - `openerService.open(...)` do `TerminalUrlLinkOpener` não é aguardado no original; o porte descarta a promessa.
//! - `try { ... } catch { return false }` de `_tryOpenExactLink` é o `Result` da promessa interna (qualquer erro vale `false`).
//! - `:${matchingParsedLink.suffix.row}` escreve o número como o JavaScript (`10` e não `10.0`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::js_string::{self, JsString};
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::LogServiceRef;
use je_platform::opener::editor_types::{EditorOptions, TextEditorSelection};
use je_platform::opener::opener::{AllowContributedOpeners, IOpenerService, OpenOptions, OpenResource};
use je_platform::resources::uri_file;
use je_platform::terminal::capabilities::capabilities::{ITerminalCapabilityStore, TerminalCapability};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use zyarr::RegExp;

use super::links::{ITerminalLinkOpener, ITerminalSimpleLink, TerminalBuiltinLinkType};
use super::terminal_link_helpers::{os_path_module, update_link_with_relative_cwd};
use super::terminal_link_parsing::{detect_links, get_link_suffix};
use super::terminal_local_link_detector::get_terminal_link_type;
use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
use crate::services::editor::editor_service::IEditorService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::host::host::{IHostService, OpenWindowOptions, WindowOpenable};
use crate::services::search::query_builder::{ICommonQueryBuilderOptions, IFileQueryBuilderOptions, QueryBuilder, WorkspaceFolderOrUri};
use crate::services::search::search::{IFileQuery, ISearchService};

/// `quickInputService.quickAccess`: só o `show(value?)`.
pub trait IQuickAccessController {
    /// Open the quick access picker with the optional value prefilled.
    fn show(&self, value: Option<&str>);
}

/// `QueryBuilder#file`: o que o `TerminalSearchLinkOpener` lê do `QueryBuilder`.
pub trait IFileQueryBuilder {
    fn file(&self, folders: &[WorkspaceFolderOrUri], options: &IFileQueryBuilderOptions) -> IFileQuery;
}

impl IFileQueryBuilder for QueryBuilder {
    fn file(&self, folders: &[WorkspaceFolderOrUri], options: &IFileQueryBuilderOptions) -> IFileQuery {
        QueryBuilder::file(self, folders, options)
    }
}

/// Compila uma expressão regular do original.
fn regex(pattern: &str) -> RegExp {
    RegExp::from_str(pattern, "").expect("a expressão regular do abridor de links é válida")
}

/// `text.replace(regex, '')`: troca só o primeiro casamento.
fn replace_first(text: &str, pattern: &RegExp) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    match pattern.exec(&units, 0) {
        Some(found) => {
            let mut result = units[..found.start()].to_vec();
            result.extend_from_slice(&units[found.end()..]);
            String::from_utf16_lossy(&result)
        }
        None => text.to_owned(),
    }
}

/// `text.match(regex)?.[0]`: o primeiro casamento.
fn match_first(text: &str, pattern: &RegExp) -> Option<String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    pattern.exec(&units, 0).map(|found| String::from_utf16_lossy(&units[found.start()..found.end()]))
}

/// O número como o JavaScript o escreve (`10`, e não `10.0`).
fn js_number(value: f64) -> String {
    value.to_string()
}

/// O `{ $mid, scheme, authority, path, query, fragment }` de `URI#toJSON`, como o argumento de um comando.
fn uri_to_dynamic(uri: &URI) -> Dynamic {
    Dynamic::object([
        ("$mid", Dynamic::Number(1.0)),
        ("scheme", Dynamic::String(uri.scheme().clone())),
        ("authority", Dynamic::String(uri.authority().clone())),
        ("path", Dynamic::String(uri.path().clone())),
        ("query", Dynamic::String(uri.query().clone())),
        ("fragment", Dynamic::String(uri.fragment().clone())),
    ])
}

/// `TerminalLocalFileLinkOpener`.
#[derive(Clone)]
pub struct TerminalLocalFileLinkOpener {
    editor_service: Rc<dyn IEditorService>,
}

impl TerminalLocalFileLinkOpener {
    pub fn new(editor_service: Rc<dyn IEditorService>) -> Self {
        Self { editor_service }
    }
}

impl ITerminalLinkOpener for TerminalLocalFileLinkOpener {
    fn open(&self, link: &ITerminalSimpleLink) -> Promise<()> {
        let Some(uri) = link.uri.clone() else {
            return Promise::rejected(Error::new("Tried to open file link without a resolved URI"));
        };
        let link_suffix = match &link.parsed_link {
            Some(parsed_link) => parsed_link.suffix.clone(),
            None => get_link_suffix(&link.text.encode_utf16().collect::<Vec<u16>>()),
        };
        let mut selection: Option<TextEditorSelection> = link.selection;
        if selection.is_none() {
            selection = link_suffix.as_ref().and_then(|suffix| {
                suffix.row.map(|row| TextEditorSelection {
                    start_line_number: row,
                    start_column: suffix.col.unwrap_or(1.0),
                    end_line_number: suffix.row_end,
                    end_column: suffix.col_end,
                })
            });
        }
        self.editor_service
            .open_editor(UntypedEditorInput {
                resource: Some(uri),
                options: Some(TextEditorOptions {
                    base: EditorOptions { pinned: Some(true), reveal_if_opened: Some(true), ..EditorOptions::default() },
                    selection,
                    ..TextEditorOptions::default()
                }),
                ..UntypedEditorInput::default()
            })
            .then(|_| ())
    }
}

/// `TerminalLocalFolderInWorkspaceLinkOpener`.
#[derive(Clone)]
pub struct TerminalLocalFolderInWorkspaceLinkOpener {
    command_service: Rc<dyn ICommandService>,
}

impl TerminalLocalFolderInWorkspaceLinkOpener {
    pub fn new(command_service: Rc<dyn ICommandService>) -> Self {
        Self { command_service }
    }
}

impl ITerminalLinkOpener for TerminalLocalFolderInWorkspaceLinkOpener {
    fn open(&self, link: &ITerminalSimpleLink) -> Promise<()> {
        let Some(uri) = &link.uri else {
            return Promise::rejected(Error::new("Tried to open folder in workspace link without a resolved URI"));
        };
        self.command_service.execute_command("revealInExplorer", vec![uri_to_dynamic(uri)]).then(|_| ())
    }
}

/// `TerminalLocalFolderOutsideWorkspaceLinkOpener`.
#[derive(Clone)]
pub struct TerminalLocalFolderOutsideWorkspaceLinkOpener {
    host_service: Rc<dyn IHostService>,
}

impl TerminalLocalFolderOutsideWorkspaceLinkOpener {
    pub fn new(host_service: Rc<dyn IHostService>) -> Self {
        Self { host_service }
    }
}

impl ITerminalLinkOpener for TerminalLocalFolderOutsideWorkspaceLinkOpener {
    fn open(&self, link: &ITerminalSimpleLink) -> Promise<()> {
        let Some(uri) = &link.uri else {
            return Promise::rejected(Error::new("Tried to open folder in workspace link without a resolved URI"));
        };
        // `this._hostService.openWindow(...)` não é aguardado no original.
        drop(self.host_service.open_window(
            vec![WindowOpenable::Folder { folder_uri: uri.clone(), label: None }],
            Some(OpenWindowOptions { force_new_window: Some(true), ..OpenWindowOptions::default() }),
        ));
        Promise::resolved(())
    }
}

/// `IResourceMatch`.
struct ResourceMatch {
    uri: URI,
    is_directory: Option<bool>,
}

/// `TerminalSearchLinkOpener`.
#[derive(Clone)]
pub struct TerminalSearchLinkOpener {
    file_query_builder: Rc<RefCell<Rc<dyn IFileQueryBuilder>>>,
    capabilities: Rc<dyn ITerminalCapabilityStore>,
    initial_cwd: String,
    local_file_opener: Rc<TerminalLocalFileLinkOpener>,
    local_folder_in_workspace_opener: Rc<TerminalLocalFolderInWorkspaceLinkOpener>,
    get_os: Rc<dyn Fn() -> OperatingSystem>,
    file_service: Rc<dyn IFileService>,
    quick_access: Rc<dyn IQuickAccessController>,
    search_service: Rc<dyn ISearchService>,
    log_service: LogServiceRef,
    workbench_environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

impl TerminalSearchLinkOpener {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        capabilities: Rc<dyn ITerminalCapabilityStore>,
        initial_cwd: &str,
        local_file_opener: Rc<TerminalLocalFileLinkOpener>,
        local_folder_in_workspace_opener: Rc<TerminalLocalFolderInWorkspaceLinkOpener>,
        get_os: Rc<dyn Fn() -> OperatingSystem>,
        file_service: Rc<dyn IFileService>,
        file_query_builder: Rc<dyn IFileQueryBuilder>,
        quick_access: Rc<dyn IQuickAccessController>,
        search_service: Rc<dyn ISearchService>,
        log_service: LogServiceRef,
        workbench_environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        workspace_context_service: Rc<dyn IWorkspaceContextService>,
    ) -> Self {
        Self {
            file_query_builder: Rc::new(RefCell::new(file_query_builder)),
            capabilities,
            initial_cwd: initial_cwd.to_owned(),
            local_file_opener,
            local_folder_in_workspace_opener,
            get_os,
            file_service,
            quick_access,
            search_service,
            log_service,
            workbench_environment_service,
            workspace_context_service,
        }
    }

    /// O `protected _fileQueryBuilder` que a subclasse de teste troca (`setFileQueryBuilder`).
    pub fn set_file_query_builder(&self, value: Rc<dyn IFileQueryBuilder>) {
        *self.file_query_builder.borrow_mut() = value;
    }

    async fn open_inner(&self, link: ITerminalSimpleLink) -> Result<(), Error> {
        let os = (self.get_os)();
        let environment = ProcessEnvironment;
        let os_path: PathModule<'_> = os_path_module(os, &environment);
        let path_separator = os_path.sep();

        // Remove file:/// and any leading ./ or ../ since quick access doesn't understand that format
        let mut text = replace_first(&link.text, &regex(r"^file:\/\/\/?"));
        text = replace_first(&os_path.normalize(&text), &regex(r"^(\.+[\\/])+"));

        // Try extract any trailing line and column numbers by matching the text against parsed
        // links. This will give a search link `foo` on a line like `"foo", line 10` to open the
        // quick pick with `foo:10` as the contents.
        //
        // This also normalizes the path to remove suffixes like :10 or :5.0-4
        if let Some(context_line) = &link.context_line {
            // Skip suffix parsing if the text looks like it contains an ISO 8601 timestamp format
            let iso8601_pattern = regex(r":\d{2}:\d{2}[+-]\d{2}:\d{2}\.[a-z]+");
            if match_first(&link.text, &iso8601_pattern).is_none() {
                let context_units: Vec<u16> = context_line.encode_utf16().collect();
                let parsed_links = detect_links(&context_units, os);
                // Optimistically check that the link _starts with_ the parsed link text. If so,
                // continue to use the parsed link
                let matching_parsed_link = parsed_links.iter().find(|parsed_link| parsed_link.suffix.is_some() && link.text.starts_with(&parsed_link.path.text.to_string_lossy()));
                if let Some(matching_parsed_link) = matching_parsed_link
                    && let Some(suffix) = &matching_parsed_link.suffix
                    && let Some(row) = suffix.row
                {
                    // Normalize the path based on the parsed link
                    text = matching_parsed_link.path.text.to_string_lossy();
                    text.push_str(&format!(":{}", js_number(row)));
                    if let Some(col) = suffix.col {
                        text.push_str(&format!(":{}", js_number(col)));
                    }
                }
            }
        }

        // Remove `:<one or more non number characters>` from the end of the link.
        // Examples:
        // - Ruby stack traces: <link>:in ...
        // - Grep output: <link>:<result line>
        // This only happens when the colon is _not_ followed by a forward- or back-slash as that
        // would break absolute Windows paths (eg. `C:/Users/...`).
        text = replace_first(&text, &regex(r":[^\\/\d][^\d]*$"));

        // Remove any trailing periods after the line/column numbers, to prevent breaking the search feature, #200257
        // Examples:
        // "Check your code Test.tsx:12:45." -> Test.tsx:12:45
        // "Check your code Test.tsx:12." -> Test.tsx:12
        text = replace_first(&text, &regex(r"\.$"));

        // If any of the names of the folders in the workspace matches
        // a prefix of the link, remove that prefix and continue
        for folder in self.workspace_context_service.get_workspace().folders() {
            let units = JsString::from(text.as_str());
            let name_length = JsString::from(folder.name.as_str()).len();
            let prefix = JsString::from(js_string::substring(units.units(), 0, name_length as i64 + 1));
            if prefix.to_string_lossy() == format!("{}{path_separator}", folder.name) {
                text = JsString::from(js_string::substring(units.units(), name_length as i64 + 1, i64::MAX)).to_string_lossy();
            }
        }
        let mut cwd_resolved_text = text.clone();
        if self.capabilities.has(TerminalCapability::CommandDetection) {
            let resolved = update_link_with_relative_cwd(
                self.capabilities.as_ref(),
                f64::from(link.buffer_range.start.y),
                &text,
                &os_path,
                self.log_service.as_ref(),
            );
            cwd_resolved_text = resolved.and_then(|resolved| resolved.into_iter().next()).filter(|resolved| !resolved.is_empty()).unwrap_or_else(|| text.clone());
        }

        // Try open the cwd resolved link first
        if self.try_open_exact_link(&cwd_resolved_text, &link).await {
            return Ok(());
        }

        // If the cwd resolved text didn't match, try find the link without the cwd resolved, for
        // example when a command prints paths in a sub-directory of the current cwd
        if text != cwd_resolved_text && self.try_open_exact_link(&text, &link).await {
            return Ok(());
        }

        // Fallback to searching quick access
        self.quick_access.show(Some(&text));
        Ok(())
    }

    async fn get_exact_match(&self, sanitized_link: &str) -> Result<Option<ResourceMatch>, Error> {
        // Make the link relative to the cwd if it isn't absolute
        let os = (self.get_os)();
        let environment = ProcessEnvironment;
        let path_module: PathModule<'_> = os_path_module(os, &environment);
        let is_absolute = path_module.is_absolute(sanitized_link);
        let mut absolute_path: Option<String> = if is_absolute { Some(sanitized_link.to_owned()) } else { None };
        if !is_absolute && !self.initial_cwd.is_empty() {
            absolute_path = Some(path_module.join(&[&self.initial_cwd, sanitized_link]));
        }

        // Try open as an absolute link
        let mut resource_match: Option<ResourceMatch> = None;
        if let Some(absolute_path) = absolute_path.filter(|path| !path.is_empty()) {
            let mut normalized_absolute_path = absolute_path.clone();
            if os == OperatingSystem::Windows {
                normalized_absolute_path = absolute_path.replace('\\', "/");
                if match_first(&normalized_absolute_path, &RegExp::from_str("[a-z]:", "i").expect("a expressão é válida")).is_some() {
                    normalized_absolute_path = format!("/{normalized_absolute_path}");
                }
            }
            let uri = match self.workbench_environment_service.remote_authority() {
                Some(remote_authority) => URI::from(
                    &UriComponents {
                        scheme: Some(schemas::VSCODE_REMOTE.into()),
                        authority: Some(remote_authority.as_str().into()),
                        path: Some(normalized_absolute_path.as_str().into()),
                        ..UriComponents::default()
                    },
                    false,
                ),
                None => uri_file(&normalized_absolute_path),
            };
            if let Ok(file_stat) = self.file_service.stat(&uri).await {
                resource_match = Some(ResourceMatch { uri, is_directory: Some(file_stat.is_directory) });
            }
            // File or dir doesn't exist, continue on
        }

        // Search the workspace if an exact match based on the absolute path was not found
        if resource_match.is_none() {
            let folders: Vec<WorkspaceFolderOrUri> = self.workspace_context_service.get_workspace().folders().into_iter().map(WorkspaceFolderOrUri::Folder).collect();
            let query = self.file_query_builder.borrow().file(
                &folders,
                &IFileQueryBuilderOptions {
                    file_pattern: Some(sanitized_link.to_owned()),
                    common: ICommonQueryBuilderOptions { max_results: Some(2), ..ICommonQueryBuilderOptions::default() },
                    ..IFileQueryBuilderOptions::default()
                },
            );
            let results = self.search_service.file_search(&query, None).await?;
            if !results.results.is_empty() {
                if results.results.len() == 1 {
                    // If there's exactly 1 search result, return it regardless of whether it's
                    // exact or partial.
                    resource_match = Some(ResourceMatch { uri: results.results[0].resource.clone(), is_directory: None });
                } else if !is_absolute {
                    // For non-absolute links, exact link matching is allowed only if there is a single an exact
                    // file match. For example searching for `foo.txt` when there is no cwd information
                    // available (ie. only the initial cwd) should open the file directly only if there is a
                    // single file names `foo.txt` anywhere within the folder. These same rules apply to
                    // relative paths with folders such as `src/foo.txt`.
                    let query = self.file_query_builder.borrow().file(
                        &folders,
                        &IFileQueryBuilderOptions { file_pattern: Some(format!("**/{sanitized_link}")), ..IFileQueryBuilderOptions::default() },
                    );
                    let results = self.search_service.file_search(&query, None).await?;
                    // Find an exact match if it exists
                    let exact_matches: Vec<_> = results.results.iter().filter(|e| e.resource.to_string(false).to_string_lossy().ends_with(sanitized_link)).collect();
                    if exact_matches.len() == 1 {
                        resource_match = Some(ResourceMatch { uri: exact_matches[0].resource.clone(), is_directory: None });
                    }
                }
            }
        }
        Ok(resource_match)
    }

    async fn try_open_exact_link(&self, text: &str, link: &ITerminalSimpleLink) -> bool {
        let sanitized_link = replace_first(text, &regex(r":\d+(:\d+)?$"));
        let attempt: Result<bool, Error> = async {
            let result = self.get_exact_match(&sanitized_link).await?;
            if let Some(ResourceMatch { uri, is_directory }) = result {
                let mut link_to_open = ITerminalSimpleLink::new(
                    // Use the absolute URI's path here so the optional line/col get detected
                    format!("{}{}", uri.path().to_string_lossy(), match_first(text, &regex(r":\d+(:\d+)?$")).unwrap_or_default()),
                    link.buffer_range,
                    link.type_.clone(),
                );
                link_to_open.uri = Some(uri);
                if is_directory.is_truthy() {
                    self.local_folder_in_workspace_opener.open(&link_to_open).await?;
                } else {
                    self.local_file_opener.open(&link_to_open).await?;
                }
                return Ok(true);
            }
            Ok(false)
        }
        .await;
        attempt.unwrap_or(false)
    }
}

impl ITerminalLinkOpener for TerminalSearchLinkOpener {
    fn open(&self, link: &ITerminalSimpleLink) -> Promise<()> {
        let this = self.clone();
        let link = link.clone();
        async_fn(async move { this.open_inner(link).await })
    }
}

/// `TerminalUrlLinkOpener`.
#[derive(Clone)]
pub struct TerminalUrlLinkOpener {
    is_remote: bool,
    local_file_opener: Rc<TerminalLocalFileLinkOpener>,
    local_folder_in_workspace_opener: Rc<TerminalLocalFolderInWorkspaceLinkOpener>,
    local_folder_outside_workspace_opener: Rc<TerminalLocalFolderOutsideWorkspaceLinkOpener>,
    opener_service: Rc<dyn IOpenerService>,
    configuration_service: Rc<dyn IConfigurationService>,
    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    log_service: LogServiceRef,
}

impl TerminalUrlLinkOpener {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        is_remote: bool,
        local_file_opener: Rc<TerminalLocalFileLinkOpener>,
        local_folder_in_workspace_opener: Rc<TerminalLocalFolderInWorkspaceLinkOpener>,
        local_folder_outside_workspace_opener: Rc<TerminalLocalFolderOutsideWorkspaceLinkOpener>,
        opener_service: Rc<dyn IOpenerService>,
        configuration_service: Rc<dyn IConfigurationService>,
        file_service: Rc<dyn IFileService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        workspace_context_service: Rc<dyn IWorkspaceContextService>,
        log_service: LogServiceRef,
    ) -> Self {
        Self {
            is_remote,
            local_file_opener,
            local_folder_in_workspace_opener,
            local_folder_outside_workspace_opener,
            opener_service,
            configuration_service,
            file_service,
            uri_identity_service,
            workspace_context_service,
            log_service,
        }
    }

    /// `this._openerService.open(link.text, { allowTunneling, allowContributedOpeners: true, openExternal: true })`.
    fn open_with_opener_service(&self, link: &ITerminalSimpleLink) {
        let allow_tunneling = self.is_remote && self.configuration_service.get_value(Some("remote.forwardOnOpen"), None).is_truthy();
        // `openerService.open(...)` não é aguardado no original.
        drop(self.opener_service.open(
            &OpenResource::Text(JsString::from(link.text.as_str())),
            Some(&OpenOptions {
                allow_tunneling: Some(allow_tunneling),
                allow_contributed_openers: Some(AllowContributedOpeners::Bool(true)),
                open_external: Some(true),
                ..OpenOptions::default()
            }),
        ));
    }

    async fn open_file_scheme_link(&self, link: &ITerminalSimpleLink) -> Result<(), Error> {
        let Some(uri) = &link.uri else {
            return Ok(());
        };

        // Os erros dos abridores delegados também caem no `catch` do original.
        let delegated: Result<Option<()>, Error> = match self.file_service.stat(uri).await {
            Ok(stat) => {
                let is_directory = stat.is_directory;
                let link_type = get_terminal_link_type(uri, is_directory, self.uri_identity_service.as_ref(), self.workspace_context_service.as_ref());

                // Delegate to appropriate opener based on link type
                match link_type {
                    TerminalBuiltinLinkType::LocalFile => self.local_file_opener.open(link).await.map(Some),
                    TerminalBuiltinLinkType::LocalFolderInWorkspace => self.local_folder_in_workspace_opener.open(link).await.map(Some),
                    TerminalBuiltinLinkType::LocalFolderOutsideWorkspace => self.local_folder_outside_workspace_opener.open(link).await.map(Some),
                    TerminalBuiltinLinkType::Url => self.open(link).await.map(Some),
                    TerminalBuiltinLinkType::Search => Ok(None),
                }
            }
            Err(error) => Err(error),
        };
        match delegated {
            Ok(Some(())) => return Ok(()),
            Ok(None) => {}
            Err(_) => self.log_service.warn("Open file via native file explorer"),
        }
        self.open_with_opener_service(link);
        Ok(())
    }
}

impl ITerminalLinkOpener for TerminalUrlLinkOpener {
    fn open(&self, link: &ITerminalSimpleLink) -> Promise<()> {
        let Some(uri) = &link.uri else {
            return Promise::rejected(Error::new("Tried to open a url without a resolved URI"));
        };
        // Handle file:// URIs by delegating to appropriate file/folder openers
        if *uri.scheme() == schemas::FILE {
            let this = self.clone();
            let link = link.clone();
            return async_fn(async move { this.open_file_scheme_link(&link).await });
        }
        // It's important to use the raw string value here to avoid converting pre-encoded values
        // from the URL like `%2B` -> `+`.
        self.open_with_opener_service(link);
        Promise::resolved(())
    }
}
