//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/browser/terminalCompletionService.ts`: o
//! serviço que reúne as completions dos provedores registrados (os das extensões e os embutidos) e que
//! completa caminhos lendo o sistema de arquivos de verdade pelo `IFileService`.
//!
//! Diferenças do original:
//! - **Serviço.** `ITerminalCompletionService` é o trait [`ITerminalCompletionService`] (id
//!   [`I_TERMINAL_COMPLETION_SERVICE`]); `TerminalCompletionService` é [`Injectable`] com os mesmos quatro
//!   serviços do construtor (`IConfigurationService`, `IFileService`, `ILabelService` e o
//!   `ITerminalLogService`, que é o [`LogService`]). Os métodos `provideCompletions`,
//!   `registerTerminalCompletionProvider`, `providers` e `onDidChangeProviders` ficam no
//!   `impl` do trait; `resolveResources` e `_getEnabledProviders` são métodos próprios.
//! - **`_getEnabledProviders` protegido.** O original o chama numa subclasse de teste; aqui é
//!   [`TerminalCompletionService::get_enabled_providers`], público. `providerConfig` que não é um objeto
//!   (o `hasOwnProperty.call(undefined)` do original lançaria `TypeError`) não tem nenhum provedor
//!   desligado.
//! - **Provedores.** `provider.id` e `provider.triggerCharacters` são atribuídos pelo registro, então o
//!   trait [`ITerminalCompletionProvider`] tem os setters `set_id` e `set_trigger_characters`
//!   (mutabilidade interior no implementador). `TerminalShellType[]` é `Vec<ShellType>` (o `undefined` do
//!   array nunca casa com um `shellType` definido). O resultado de `provideCompletions`
//!   (`ITerminalCompletion[] | TerminalCompletionList | undefined`) é
//!   `Option<`[`TerminalCompletionProviderResult`]`>`.
//! - **Valores indefinidos.** `item.provider ??= provider.id` só atribui quando o `provider` do item
//!   é vazio (o `string` do Rust não tem `undefined`). O `labels.has(item.label)` do dedupe só compara
//!   rótulos de texto: um `CompletionItemLabel` (objeto) nunca é igual a uma string no `Set` do original.
//! - **Tipos de URI.** `cwd: UriComponents` revivido por `URI.revive` é o próprio [`URI`]. `globPattern:
//!   string | IRelativePattern` é [`GlobPattern`]. `processEnv` (o ambiente do processo, substituível em
//!   teste) é o ambiente de [`je_base::process`] até que [`TerminalCompletionService::set_process_env`]
//!   o troque.
//! - **Texto.** Os índices do original são unidades UTF-16: `cursorPosition - lastWord.length` usa o
//!   tamanho UTF-16 e `substring(0, cursorPosition)` corta nelas. As regexes do original (`/(?<!\\) /`,
//!   `/^[a-zA-Z_]+=(?<rhs>.+)$/`, `/^\.\.?[\\\/]/`, `/^~[\\\/]?/`, `/[\\\/]/`) são feitas à mão. O
//!   separador de caminho tem um único caractere.
//! - **Assíncrono.** Cada função `async` é [`async_fn`]; a espera é `promise.await` (que devolve o
//!   `Result`); o `try/catch` é o ramo de erro. Os filhos do diretório rodam como no `Promise.all(map(async
//!   ...))`: cada um corre sincronicamente até o primeiro `await` (o `realpath` dos links simbólicos), então
//!   os links entram na lista depois dos demais, como no original.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{async_fn, timeout};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::glob::{GlobOptions, RelativePattern, match_pattern};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::path::{PathModule, ProcessEnvironment as HostEnvironment};
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::{Injectable, service_id};
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileStat, FileSystemProviderCapabilities, ResolveFileOptions};
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::log::{LogArg, LogService};
use je_platform::resources::{ext_uri, host_platform};
use je_platform::terminal::capabilities::capabilities::{ITerminalCapabilityStore, ShellEnvDetection};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;
use je_platform::terminal::terminal::{GeneralShellType, ShellType, TerminalShellType, WindowsShellType};

use super::common::terminal_suggest_configuration::TerminalSuggestSettingId;
use super::terminal_completion_item::{ITerminalCompletion, TerminalCompletionItemKind};
use super::terminal_git_bash_helpers::{git_bash_to_windows_path, windows_to_git_bash_path};
use crate::services::suggest::simple_completion_item::SimpleCompletionLabel;
use je_base::dynamic::Dynamic;

service_id!(pub I_TERMINAL_COMPLETION_SERVICE: dyn ITerminalCompletionService = "terminalCompletionService");

/// `string | IRelativePattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobPattern {
    Text(String),
    Relative(RelativePattern),
}

/// `TerminalCompletionResourceOptions`.
#[derive(Clone, Debug)]
pub struct TerminalCompletionResourceOptions {
    pub show_files: Option<bool>,
    pub show_directories: Option<bool>,
    pub glob_pattern: Option<GlobPattern>,
    pub cwd: URI,
    pub path_separator: String,
}

impl TerminalCompletionResourceOptions {
    /// Só os campos obrigatórios (`cwd` e `pathSeparator`); os demais ficam `undefined`.
    pub fn new(cwd: URI, path_separator: &str) -> Self {
        Self { show_files: None, show_directories: None, glob_pattern: None, cwd, path_separator: path_separator.to_string() }
    }
}

/// `TerminalCompletionList`: uma coleção de completions a apresentar no terminal.
#[derive(Clone, Default)]
pub struct TerminalCompletionList {
    /// Os recursos devem aparecer na lista de completions.
    pub resource_options: Option<TerminalCompletionResourceOptions>,
    /// Os itens de completion.
    pub items: Option<Vec<ITerminalCompletion>>,
}

impl TerminalCompletionList {
    /// `constructor(items?, resourceOptions?)`.
    pub fn new(items: Option<Vec<ITerminalCompletion>>, resource_options: Option<TerminalCompletionResourceOptions>) -> Self {
        Self { items, resource_options }
    }
}

/// `ITerminalCompletion[] | TerminalCompletionList`: o que um provedor devolve.
#[derive(Clone)]
pub enum TerminalCompletionProviderResult {
    Items(Vec<ITerminalCompletion>),
    List(TerminalCompletionList),
}

/// `ITerminalCompletionProvider`.
pub trait ITerminalCompletionProvider {
    /// `id`.
    fn id(&self) -> String;
    /// `provider.id = id`, feito pelo registro.
    fn set_id(&self, id: &str);
    /// `shellTypes`.
    fn shell_types(&self) -> Option<Vec<ShellType>> {
        None
    }
    fn provide_completions(&self, value: &str, cursor_position: i64, token: &CancellationToken) -> Promise<Option<TerminalCompletionProviderResult>>;
    /// `triggerCharacters`.
    fn trigger_characters(&self) -> Option<Vec<String>> {
        None
    }
    /// `provider.triggerCharacters = triggerCharacters`, feito pelo registro.
    fn set_trigger_characters(&self, trigger_characters: Vec<String>);
    /// `isBuiltin`.
    fn is_builtin(&self) -> bool {
        false
    }
}

/// `ITerminalCompletionService`.
pub trait ITerminalCompletionService {
    /// `providers`: os provedores na ordem de registro.
    fn providers(&self) -> Vec<Rc<dyn ITerminalCompletionProvider>>;

    fn on_did_change_providers(&self) -> Event<()>;

    fn register_terminal_completion_provider(
        &self,
        extension_identifier: &str,
        id: &str,
        provider: Rc<dyn ITerminalCompletionProvider>,
        trigger_characters: &[String],
    ) -> DisposableRef;

    #[allow(clippy::too_many_arguments)]
    fn provide_completions(
        &self,
        prompt_value: &str,
        cursor_position: i64,
        allow_fallback_completions: bool,
        shell_type: TerminalShellType,
        capabilities: Rc<dyn ITerminalCapabilityStore>,
        token: CancellationToken,
        trigger_character: Option<bool>,
        skip_extension_completions: Option<bool>,
        explicitly_invoked: Option<bool>,
    ) -> Promise<Option<Vec<ITerminalCompletion>>>;
}

type ProviderMap = IndexMap<String, IndexMap<String, Rc<dyn ITerminalCompletionProvider>>>;

/// `TerminalCompletionService`.
pub struct TerminalCompletionService {
    this: Weak<Self>,
    base: Rc<Disposable>,
    /// ext id -> provider id -> provider
    providers: Rc<RefCell<ProviderMap>>,
    on_did_change_providers: Rc<Emitter<()>>,
    /// `_processEnv`: `None` é o ambiente do processo.
    process_env: RefCell<Option<BTreeMap<String, String>>>,
    configuration_service: Rc<dyn IConfigurationService>,
    file_service: Rc<dyn IFileService>,
    label_service: Rc<dyn ILabelService>,
    log_service: Rc<dyn LogService>,
}

impl Injectable for TerminalCompletionService {
    type Args = ();
    type Deps = (Rc<dyn IConfigurationService>, Rc<dyn IFileService>, Rc<dyn ILabelService>, Rc<dyn LogService>);

    fn create(_: (), (configuration_service, file_service, label_service, log_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_providers: Rc<Emitter<()>> = Emitter::new();
        base.register(on_did_change_providers.clone());

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            providers: Rc::default(),
            on_did_change_providers,
            process_env: RefCell::new(None),
            configuration_service,
            file_service,
            label_service,
            log_service,
        })
    }
}

impl IDisposable for TerminalCompletionService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O que `resolveResources` resolveu para a pasta da última palavra: um recurso ou, quando só se sabe que
/// há uma pasta pessoal, o texto que a descreve.
enum FolderResource {
    Uri(URI),
    Text(&'static str),
}

/// Onde o caminho digitado começa.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathType {
    Tilde,
    Absolute,
    Relative,
}

/// O que cada filho de uma pasta lê para virar completion (o que o `map(async child => ...)` do original
/// captura).
struct ChildContext {
    file_service: Rc<dyn IFileService>,
    label_service: Rc<dyn ILabelService>,
    provider: String,
    path_separator: String,
    path_type: PathType,
    shell_type: TerminalShellType,
    glob_pattern: Option<GlobPattern>,
    last_word_folder: String,
    last_word_folder_has_dot_prefix: bool,
    show_directories: bool,
    show_files: bool,
    is_command_position: bool,
    use_windows_style_path: bool,
    replacement_range: [i64; 2],
}

impl TerminalCompletionService {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `set processEnv(env)`: substitui o ambiente do processo (para teste).
    pub fn set_process_env(&self, env: BTreeMap<String, String>) {
        *self.process_env.borrow_mut() = Some(env);
    }

    /// `_getEnabledProviders(providers)`.
    pub fn get_enabled_providers(&self, providers: Vec<Rc<dyn ITerminalCompletionProvider>>) -> Vec<Rc<dyn ITerminalCompletionProvider>> {
        let provider_config = self.configuration_service.get_value(Some(TerminalSuggestSettingId::Providers.as_str()), None);
        let configured_keys = provider_config.own_keys();
        providers
            .into_iter()
            .filter(|provider| {
                let provider_id = provider.id();
                if provider_id.is_empty() {
                    return false;
                }
                let key = provider_id.as_str();
                !configured_keys.iter().any(|configured| *configured == key) || !matches!(provider_config.get_str(key), Dynamic::Bool(false))
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_completions(
        &self,
        providers: Vec<Rc<dyn ITerminalCompletionProvider>>,
        shell_type: TerminalShellType,
        prompt_value: &str,
        cursor_position: i64,
        _allow_fallback_completions: bool,
        capabilities: Rc<dyn ITerminalCapabilityStore>,
        token: CancellationToken,
        explicitly_invoked: Option<bool>,
    ) -> Promise<Option<Vec<ITerminalCompletion>>> {
        self.log_service.trace("TerminalCompletionService#_collectCompletions");
        let completion_promises: Vec<Promise<Option<Vec<ITerminalCompletion>>>> = providers
            .into_iter()
            .map(|provider| {
                let this = self.rc();
                let prompt_value = prompt_value.to_string();
                let capabilities = capabilities.clone();
                let token = token.clone();
                async_fn(async move {
                    if let (Some(shell_types), Some(shell_type)) = (provider.shell_types(), shell_type)
                        && !shell_types.contains(&shell_type)
                    {
                        return Ok(None);
                    }
                    let timeout_ms: u64 = if explicitly_invoked == Some(true) { 30000 } else { 5000 };
                    let timed_out = Rc::new(Cell::new(false));
                    let provider_id = provider.id();

                    let finished = {
                        let (this, provider_id) = (this.clone(), provider_id.clone());
                        provider.provide_completions(&prompt_value, cursor_position, &token).then(move |result| {
                            this.log_service.trace(&format!("TerminalCompletionService#_collectCompletions provider {provider_id} finished"));
                            result
                        })
                    };
                    let timer = {
                        let timed_out = timed_out.clone();
                        timeout(timeout_ms).then(move |()| -> Option<TerminalCompletionProviderResult> {
                            timed_out.set(true);
                            None
                        })
                    };
                    let completions = match Promise::race(vec![finished, timer]).await {
                        Ok(completions) => completions,
                        Err(error) => {
                            this.log_service.trace_args(&format!("[TerminalCompletionService] Exception from provider '{provider_id}':"), &[LogArg::from(&error)]);
                            return Ok(None);
                        }
                    };
                    if timed_out.get() {
                        this.log_service.trace(&format!(
                            "[TerminalCompletionService] Provider '{provider_id}' timed out after {timeout_ms}ms. promptValue='{prompt_value}', cursorPosition={cursor_position}, explicitlyInvoked={}",
                            explicitly_invoked.map_or("undefined".to_string(), |invoked| invoked.to_string())
                        ));
                        return Ok(None);
                    }
                    let Some(completions) = completions else {
                        return Ok(None);
                    };
                    let (mut completion_items, is_array, resource_options) = match completions {
                        TerminalCompletionProviderResult::Items(items) => (items, true, None),
                        TerminalCompletionProviderResult::List(list) => (list.items.unwrap_or_default(), false, list.resource_options),
                    };
                    this.log_service.trace(&format!("TerminalCompletionService#_collectCompletions amend {} completion items", completion_items.len()));
                    if shell_type == Some(ShellType::General(GeneralShellType::PowerShell)) {
                        for completion in &mut completion_items {
                            let start = completion.replacement_range.map_or(0, |range| range[0]);
                            completion.is_file_override.get_or_insert(completion.kind == Some(TerminalCompletionItemKind::Method) && start == 0);
                        }
                    }
                    if provider.is_builtin() {
                        // TODO do original: why is this needed?
                        for item in &mut completion_items {
                            if item.provider.is_empty() {
                                item.provider = provider_id.clone();
                            }
                        }
                    }
                    if is_array {
                        return Ok(Some(completion_items));
                    }
                    if let Some(resource_options) = resource_options {
                        let resource_completions = this
                            .resolve_resources(&resource_options, &prompt_value, cursor_position, &format!("core:path:ext:{provider_id}"), capabilities, shell_type)
                            .await?;
                        this.log_service.trace("TerminalCompletionService#_collectCompletions dedupe");
                        if let Some(resource_completions) = resource_completions {
                            let labels: Vec<String> = completion_items
                                .iter()
                                .filter_map(|item| match &item.label {
                                    SimpleCompletionLabel::Text(text) => Some(text.clone()),
                                    _ => None,
                                })
                                .collect();
                            for item in resource_completions {
                                // Ensure no duplicates such as .
                                if !labels.iter().any(|label| label == item.label.text()) {
                                    completion_items.push(item);
                                }
                            }
                        }
                        this.log_service.trace("TerminalCompletionService#_collectCompletions dedupe done");
                    }
                    Ok(Some(completion_items))
                })
            })
            .collect();

        let log_service = self.log_service.clone();
        Promise::all(completion_promises).then(move |results| {
            log_service.trace("TerminalCompletionService#_collectCompletions done");
            Some(results.into_iter().flatten().flatten().collect())
        })
    }

    /// `resolveResources(resourceOptions, promptValue, cursorPosition, provider, capabilities, shellType?)`.
    pub fn resolve_resources(
        &self,
        resource_options: &TerminalCompletionResourceOptions,
        prompt_value: &str,
        cursor_position: i64,
        provider: &str,
        capabilities: Rc<dyn ITerminalCapabilityStore>,
        shell_type: TerminalShellType,
    ) -> Promise<Option<Vec<ITerminalCompletion>>> {
        self.log_service.trace("TerminalCompletionService#resolveResources");
        let this = self.rc();
        let (resource_options, prompt_value, provider) = (resource_options.clone(), prompt_value.to_string(), provider.to_string());

        async_fn(async move {
            let path_separator = resource_options.path_separator.clone();
            let use_windows_style_path = path_separator == "\\";
            let mut prompt_value = prompt_value;
            if use_windows_style_path {
                // for tests, make sure the right path separator is used
                prompt_value = prompt_value.replace(['\\', '/'], &path_separator);
            }

            // Files requested implies folders requested since the file could be in any folder. We could
            // provide diagnostics when a folder is provided where a file is expected.
            let show_directories = resource_options.show_directories.unwrap_or(false) || resource_options.show_files.unwrap_or(false);
            let show_files = resource_options.show_files.unwrap_or(false);
            let glob_pattern = resource_options.glob_pattern.clone();

            if !show_directories && !show_files {
                return Ok(None);
            }

            let cursor_prefix = substring_to(&prompt_value, cursor_position);

            // Determine if we're completing the command (first word) vs an argument
            // We're in command position if there are no unescaped spaces before cursor
            let words_before_cursor = split_unescaped_spaces(&cursor_prefix);
            let is_command_position = words_before_cursor.len() <= 1 && !cursor_prefix.ends_with(' ');

            // TODO do original: leverage Fig's tokens array here?
            // The last word (or argument). When the cursor is following a space it will be the empty
            // string
            let mut last_word = if cursor_prefix.ends_with(' ') { String::new() } else { words_before_cursor.last().cloned().unwrap_or_default() };

            // Ignore prefixes in the word that look like setting an environment variable
            if let Some(rhs) = env_var_assignment_value(&last_word) {
                last_word = rhs;
            }

            // Get the nearest folder path from the prefix. This ignores everything after the `/` as
            // they are what triggers changes in the directory.
            let last_word_chars: Vec<char> = last_word.chars().collect();
            let last_slash_index: Option<usize> = if use_windows_style_path {
                // TODO do original: flesh out escaped path logic, it currently only partially works
                let last_backslash_index = (0..last_word_chars.len())
                    .rev()
                    .find(|&i| last_word_chars[i] == '\\' && (i == last_word_chars.len() - 1 || last_word_chars[i + 1] != ' '));
                let last_forward_slash_index = last_word_chars.iter().rposition(|&c| c == '/');
                last_backslash_index.max(last_forward_slash_index)
            } else {
                let separator = path_separator.chars().next();
                last_word_chars.iter().rposition(|&c| Some(c) == separator)
            };

            // The _complete_ folder of the last word. For example if the last word is `./src/file`,
            // this will be `./src/`. This also always ends in the path separator if it is not the empty
            // string and path separators are normalized on Windows.
            let mut last_word_folder: String = last_slash_index.map_or_else(String::new, |index| last_word_chars[..=index].iter().collect());
            if use_windows_style_path {
                last_word_folder = last_word_folder.replace('/', "\\");
            }

            // Determine the current folder being shown
            let last_word_folder_has_dot_prefix = has_dot_prefix(&last_word_folder);
            let last_word_folder_has_tilde_prefix = last_word_folder.starts_with('~');
            let is_absolute_path = get_is_absolute_path(shell_type, &path_separator, &last_word_folder, use_windows_style_path);
            let path_type = if last_word_folder_has_tilde_prefix {
                PathType::Tilde
            } else if is_absolute_path {
                PathType::Absolute
            } else {
                PathType::Relative
            };
            let cwd = resource_options.cwd.clone();
            let mut last_word_folder_resource: Option<FolderResource> = None;
            if path_type == PathType::Relative && !last_word_folder.is_empty() {
                // If the typed folder matches the tail of cwd (common when the extension already
                // resolved the path, such as `./src/vs/`), reuse cwd to avoid duplicating segments.
                let normalized_folder = (if use_windows_style_path { last_word_folder.replace('\\', "/") } else { last_word_folder.clone() }).replace("\\ ", " ");
                if let Some(without_dot) = normalized_folder.strip_prefix("./") {
                    let stripped = without_dot.trim_start_matches('/').trim_end_matches('/');
                    let reuse_cwd = if stripped.is_empty() {
                        // `./` by itself means the current directory, use cwd directly to avoid
                        // trailing slash issues with URI.joinPath on some remote file systems.
                        true
                    } else {
                        let cwd_path = cwd.path().to_string_lossy();
                        let cwd_parts: Vec<&str> = cwd_path.trim_end_matches('/').split('/').collect();
                        let stripped_parts: Vec<&str> = stripped.split('/').collect();
                        stripped_parts.len() <= cwd_parts.len()
                            && stripped_parts.iter().enumerate().all(|(index, part)| cwd_parts[cwd_parts.len() - stripped_parts.len() + index] == *part)
                    };
                    if reuse_cwd {
                        if !stat_succeeds(&this.file_service, &cwd).await {
                            return Ok(None);
                        }
                        last_word_folder_resource = Some(FolderResource::Uri(cwd.clone()));
                    }
                }

                // Otherwise resolve the folder relative to cwd.
                if last_word_folder_resource.is_none() {
                    let folder_to_resolve = URI::join_path(&cwd, &[normalized_folder.as_str()], host_platform());
                    if !stat_succeeds(&this.file_service, &folder_to_resolve).await {
                        return Ok(None);
                    }
                    last_word_folder_resource = Some(FolderResource::Uri(folder_to_resolve));
                }
            } else if path_type == PathType::Relative {
                last_word_folder_resource = Some(FolderResource::Uri(cwd.clone()));
            }
            if path_type == PathType::Relative && last_word_folder_resource.is_none() {
                if !stat_succeeds(&this.file_service, &cwd).await {
                    return Ok(None);
                }
                last_word_folder_resource = Some(FolderResource::Uri(cwd.clone()));
            }

            match path_type {
                PathType::Tilde => {
                    if let Some(home) = this.get_home_dir(use_windows_style_path, capabilities.as_ref()) {
                        last_word_folder_resource = Some(FolderResource::Uri(URI::join_path(
                            &create_uri_from_local_path(&cwd, &home),
                            &[last_word_folder.chars().skip(1).collect::<String>().replace("\\ ", " ").as_str()],
                            host_platform(),
                        )));
                    }
                    if last_word_folder_resource.is_none() {
                        // Use less strong wording here as it's not as strong of a concept on Windows
                        // and could be misleading
                        if is_tilde_folder(&last_word) {
                            last_word_folder_resource = Some(FolderResource::Text(if use_windows_style_path { "Home directory" } else { "$HOME" }));
                        }
                    }
                }
                PathType::Absolute => {
                    let absolute_path = if shell_type == Some(ShellType::Windows(WindowsShellType::GitBash)) {
                        git_bash_to_windows_path(&last_word_folder, this.get_process_env("SystemDrive").as_deref())
                    } else {
                        last_word_folder.replace("\\ ", " ")
                    };
                    last_word_folder_resource = Some(FolderResource::Uri(create_uri_from_local_path(&cwd, &absolute_path)));
                }
                PathType::Relative => {
                    if last_word_folder_resource.is_none() {
                        last_word_folder_resource = Some(FolderResource::Uri(cwd.clone()));
                    }
                }
            }

            // Assemble completions based on the resource of lastWordFolder. Note that on Windows the
            // path separators are normalized to `\`.
            let Some(last_word_folder_resource) = last_word_folder_resource else {
                return Ok(None);
            };

            let replacement_range = [cursor_position - utf16_len(&last_word), cursor_position];

            // Early exit with basic completion if we don't know the resource
            let last_word_folder_resource = match last_word_folder_resource {
                FolderResource::Text(text) => {
                    return Ok(Some(vec![resource_completion(last_word_folder.clone(), &provider, TerminalCompletionItemKind::Folder, text.to_string(), replacement_range)]));
                }
                FolderResource::Uri(uri) => uri,
            };

            let stat = this
                .file_service
                .resolve_with_metadata(&last_word_folder_resource, Some(ResolveFileOptions { resolve_metadata: true, resolve_single_child_descendants: true, ..ResolveFileOptions::default() }))
                .await?
                .into_stat();
            let Some(children) = stat.children else {
                return Ok(None);
            };
            let resource_completions: Rc<RefCell<Vec<ITerminalCompletion>>> = Rc::default();

            // Add current directory. This should be shown at the top because it will be an exact
            // match and therefore highlight the detail, plus it improves the experience when
            // runOnEnter is used.
            //
            // - (relative) `|`       -> `.`
            //   this does not have the trailing `/` intentionally as it's common to complete the
            //   current working directory and we do not want to complete `./` when `runOnEnter` is
            //   used.
            // - (relative) `./src/|` -> `./src/`
            // - (absolute) `/src/|`  -> `/src/`
            // - (tilde)    `~/|`     -> `~/`
            // - (tilde)    `~/src/|` -> `~/src/`
            this.log_service.trace("TerminalCompletionService#resolveResources cwd");
            if show_directories {
                let label = match path_type {
                    PathType::Tilde | PathType::Absolute => last_word_folder.clone(),
                    PathType::Relative => {
                        if last_word_folder.is_empty() {
                            ".".to_string()
                        } else {
                            add_path_relative_prefix(&last_word_folder, &path_separator, last_word_folder_has_dot_prefix)
                        }
                    }
                };
                resource_completions.borrow_mut().push(resource_completion(
                    label,
                    &provider,
                    TerminalCompletionItemKind::Folder,
                    get_friendly_path(this.label_service.as_ref(), &last_word_folder_resource, &path_separator, TerminalCompletionItemKind::Folder, shell_type),
                    replacement_range,
                ));
            }

            // Add all direct children files or folders
            //
            // - (relative) `cd ./src/`  -> `cd ./src/folder1/`, ...
            // - (absolute) `cd c:/src/` -> `cd c:/src/folder1/`, ...
            // - (tilde)    `cd ~/src/`  -> `cd ~/src/folder1/`, ...
            this.log_service.trace("TerminalCompletionService#resolveResources direct children");
            let context = Rc::new(ChildContext {
                file_service: this.file_service.clone(),
                label_service: this.label_service.clone(),
                provider: provider.clone(),
                path_separator: path_separator.clone(),
                path_type,
                shell_type,
                glob_pattern,
                last_word_folder: last_word_folder.clone(),
                last_word_folder_has_dot_prefix,
                show_directories,
                show_files,
                is_command_position,
                use_windows_style_path,
                replacement_range,
            });
            let child_promises: Vec<Promise<()>> = children
                .into_iter()
                .map(|child| {
                    let (context, resource_completions) = (context.clone(), resource_completions.clone());
                    async_fn(async move { resolve_child(&context, child, &resource_completions).await })
                })
                .collect();
            Promise::all(child_promises).await?;

            // Support $CDPATH specially for the `cd` command only
            //
            // - (relative) `|` -> `/foo/vscode` (CDPATH has /foo which contains vscode folder)
            this.log_service.trace("TerminalCompletionService#resolveResources CDPATH");
            if path_type == PathType::Relative && show_directories && prompt_value.starts_with("cd ") {
                let config = this.configuration_service.get_value(Some(TerminalSuggestSettingId::CdPath.as_str()), None);
                let config = match &config {
                    Dynamic::String(value) => value.to_string_lossy(),
                    _ => String::new(),
                };
                if config == "absolute" || config == "relative" {
                    if let Some(cd_path) = this.get_env_var("CDPATH", capabilities.as_ref()).filter(|cd_path| !cd_path.is_empty()) {
                        for cd_path_entry in cd_path.split(if use_windows_style_path { ';' } else { ':' }) {
                            // `catch { /* ignore */ }`
                            let Ok(file_stat) = this
                                .file_service
                                .resolve(&create_uri_from_local_path(&cwd, cd_path_entry), Some(ResolveFileOptions { resolve_single_child_descendants: true, ..ResolveFileOptions::default() }))
                                .await
                            else {
                                continue;
                            };
                            for child in file_stat.children.into_iter().flatten() {
                                if !child.is_directory {
                                    continue;
                                }
                                let use_relative = config == "relative";
                                let kind = TerminalCompletionItemKind::Folder;
                                let friendly_path = get_friendly_path(this.label_service.as_ref(), &child.resource, &path_separator, kind, shell_type);
                                let label = if use_relative {
                                    let environment = HostEnvironment;
                                    PathModule::native(&environment).basename(&je_platform::resources::fs_path(&child.resource), None)
                                } else if shell_type == Some(ShellType::Windows(WindowsShellType::GitBash)) {
                                    windows_to_git_bash_path(&je_platform::resources::fs_path(&child.resource))
                                } else {
                                    friendly_path.clone()
                                };
                                let detail = if use_relative { format!("CDPATH {friendly_path}") } else { "CDPATH".to_string() };
                                resource_completions.borrow_mut().push(resource_completion(label, &provider, kind, detail, replacement_range));
                            }
                        }
                    }
                }
            }

            // Add parent directory to the bottom of the list because it's not as useful as other suggestions
            //
            // - (relative) `|` -> `../`
            // - (relative) `./src/|` -> `./src/../`
            this.log_service.trace("TerminalCompletionService#resolveResources parent dir");
            if path_type == PathType::Relative && show_directories {
                let mut label = format!("..{path_separator}");
                if !last_word_folder.is_empty() {
                    label = add_path_relative_prefix(&format!("{last_word_folder}{label}"), &path_separator, last_word_folder_has_dot_prefix);
                }
                let parent_dir = URI::join_path(&last_word_folder_resource, &[format!("..{path_separator}").as_str()], host_platform());
                resource_completions.borrow_mut().push(resource_completion(
                    label,
                    &provider,
                    TerminalCompletionItemKind::Folder,
                    get_friendly_path(this.label_service.as_ref(), &parent_dir, &path_separator, TerminalCompletionItemKind::Folder, shell_type),
                    replacement_range,
                ));
            }

            // Add tilde for home directory for relative paths when there is no path separator in the
            // input.
            //
            // - (relative) `|` -> `~`
            this.log_service.trace("TerminalCompletionService#resolveResources tilde");
            if path_type == PathType::Relative && !last_word_folder.contains(['\\', '/']) {
                let home_detail = match this.get_home_dir(use_windows_style_path, capabilities.as_ref()) {
                    Some(home) => get_friendly_path(this.label_service.as_ref(), &create_uri_from_local_path(&cwd, &home), &path_separator, TerminalCompletionItemKind::Folder, shell_type),
                    // Use less strong wording here as it's not as strong of a concept on Windows
                    // and could be misleading
                    None => (if use_windows_style_path { "Home directory" } else { "$HOME" }).to_string(),
                };
                resource_completions.borrow_mut().push(resource_completion("~".to_string(), &provider, TerminalCompletionItemKind::Folder, home_detail, replacement_range));
            }

            this.log_service.trace("TerminalCompletionService#resolveResources done");
            let completions = resource_completions.borrow().clone();
            Ok(Some(completions))
        })
    }

    /// `_getEnvVar(key)` de `processEnv`.
    fn get_process_env(&self, key: &str) -> Option<String> {
        match &*self.process_env.borrow() {
            Some(env) => env.get(key).cloned(),
            None => je_base::process::env_var(key),
        }
    }

    fn get_env_var(&self, key: &str, capabilities: &dyn ITerminalCapabilityStore) -> Option<String> {
        let env = capabilities.get::<ShellEnvDetection>().and_then(|capability| capability.env().value);
        match env {
            Some(env) => env.get(key).cloned().flatten(),
            None => self.get_process_env(key),
        }
    }

    fn get_home_dir(&self, use_windows_style_path: bool, capabilities: &dyn ITerminalCapabilityStore) -> Option<String> {
        self.get_env_var(if use_windows_style_path { "USERPROFILE" } else { "HOME" }, capabilities)
    }
}

impl ITerminalCompletionService for TerminalCompletionService {
    fn providers(&self) -> Vec<Rc<dyn ITerminalCompletionProvider>> {
        self.providers.borrow().values().flat_map(|provider_map| provider_map.values().cloned()).collect()
    }

    fn on_did_change_providers(&self) -> Event<()> {
        self.on_did_change_providers.event()
    }

    fn register_terminal_completion_provider(
        &self,
        extension_identifier: &str,
        id: &str,
        provider: Rc<dyn ITerminalCompletionProvider>,
        trigger_characters: &[String],
    ) -> DisposableRef {
        provider.set_trigger_characters(trigger_characters.to_vec());
        provider.set_id(id);
        self.providers.borrow_mut().entry(extension_identifier.to_string()).or_default().insert(id.to_string(), provider);
        self.on_did_change_providers.fire(());

        let (providers, on_did_change_providers) = (self.providers.clone(), self.on_did_change_providers.clone());
        let (extension_identifier, id) = (extension_identifier.to_string(), id.to_string());
        to_disposable(move || {
            let mut providers = providers.borrow_mut();
            if let Some(ext_map) = providers.get_mut(&extension_identifier) {
                ext_map.shift_remove(&id);
                if ext_map.is_empty() {
                    providers.shift_remove(&extension_identifier);
                }
            }
            drop(providers);
            on_did_change_providers.fire(());
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn provide_completions(
        &self,
        prompt_value: &str,
        cursor_position: i64,
        allow_fallback_completions: bool,
        shell_type: TerminalShellType,
        capabilities: Rc<dyn ITerminalCapabilityStore>,
        token: CancellationToken,
        trigger_character: Option<bool>,
        skip_extension_completions: Option<bool>,
        explicitly_invoked: Option<bool>,
    ) -> Promise<Option<Vec<ITerminalCompletion>>> {
        self.log_service.trace("TerminalCompletionService#provideCompletions");
        if cursor_position < 0 {
            return Promise::resolved(None);
        }

        let mut providers: Vec<Rc<dyn ITerminalCompletionProvider>> = if trigger_character == Some(true) {
            let prefix = substring_to(prompt_value, cursor_position);
            self.providers()
                .into_iter()
                .filter(|provider| provider.trigger_characters().is_some_and(|characters| characters.iter().any(|character| prefix.ends_with(character.as_str()))))
                .collect()
        } else {
            self.providers()
        };

        if skip_extension_completions == Some(true) {
            providers.retain(|provider| provider.is_builtin());
            return self.collect_completions(providers, shell_type, prompt_value, cursor_position, allow_fallback_completions, capabilities, token, explicitly_invoked);
        }

        let providers = self.get_enabled_providers(providers);

        if providers.is_empty() {
            return Promise::resolved(None);
        }

        self.collect_completions(providers, shell_type, prompt_value, cursor_position, allow_fallback_completions, capabilities, token, explicitly_invoked)
    }
}

/// `try { await fileService.stat(resource) } catch { return undefined }`: diz se o `stat` deu certo.
async fn stat_succeeds(file_service: &Rc<dyn IFileService>, resource: &URI) -> bool {
    file_service.stat(resource).await.is_ok()
}

/// O corpo do `async child => { ... }` do `Promise.all` dos filhos.
async fn resolve_child(context: &ChildContext, child: FileStat, resource_completions: &RefCell<Vec<ITerminalCompletion>>) -> Result<(), Error> {
    let mut detail: Option<String> = None;
    let kind = if context.show_directories && child.is_directory {
        if child.is_symbolic_link { TerminalCompletionItemKind::SymbolicLinkFolder } else { TerminalCompletionItemKind::Folder }
    } else if context.show_files && child.is_file {
        // When completing the command (first word) on Unix, only show executable files
        if context.is_command_position && !context.use_windows_style_path && !child.executable.unwrap_or(false) {
            return Ok(());
        }
        if child.is_symbolic_link { TerminalCompletionItemKind::SymbolicLinkFile } else { TerminalCompletionItemKind::File }
    } else {
        return Ok(());
    };

    let mut label = context.last_word_folder.clone();
    if !label.is_empty() && !label.ends_with(context.path_separator.as_str()) {
        label.push_str(&context.path_separator);
    }
    label.push_str(&child.name);
    if context.path_type == PathType::Relative {
        label = add_path_relative_prefix(&label, &context.path_separator, context.last_word_folder_has_dot_prefix);
    }
    if child.is_directory && !label.ends_with(context.path_separator.as_str()) {
        label.push_str(&context.path_separator);
    }

    let label = escape_terminal_completion_label(&label, context.shell_type, &context.path_separator);

    if let Some(glob_pattern) = context.glob_pattern.as_ref().filter(|_| child.is_file) {
        let file_path = je_platform::resources::fs_path(&child.resource);
        let ignore_case = !context.file_service.has_capability(&child.resource, FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
        let options = GlobOptions { ignore_case: Some(ignore_case), ..GlobOptions::default() };
        let matches = match glob_pattern {
            GlobPattern::Text(pattern) => match_pattern(pattern, &file_path, &options),
            GlobPattern::Relative(pattern) => match_pattern(pattern, &file_path, &options),
        };
        if !matches {
            return Ok(());
        }
    }

    // Try to resolve symlink target for symbolic links
    if child.is_symbolic_link {
        // Ignore errors resolving symlink targets - they may be dangling links
        if let Ok(Some(realpath)) = context.file_service.realpath(&child.resource).await
            && !ext_uri().is_equal(Some(&child.resource), Some(&realpath), false)
        {
            detail = Some(format!(
                "{} -> {}",
                get_friendly_path(context.label_service.as_ref(), &child.resource, &context.path_separator, kind, context.shell_type),
                get_friendly_path(context.label_service.as_ref(), &realpath, &context.path_separator, kind, context.shell_type)
            ));
        }
    }

    let detail = detail.unwrap_or_else(|| get_friendly_path(context.label_service.as_ref(), &child.resource, &context.path_separator, kind, context.shell_type));
    resource_completions.borrow_mut().push(resource_completion(label, &context.provider, kind, detail, context.replacement_range));
    Ok(())
}

/// O `{ label, provider, kind, detail, replacementRange }` que `resolveResources` cria.
fn resource_completion(label: String, provider: &str, kind: TerminalCompletionItemKind, detail: String, replacement_range: [i64; 2]) -> ITerminalCompletion {
    let completion = ITerminalCompletion {
        label: label.as_str().into(),
        provider: provider.to_string(),
        kind: Some(kind),
        replacement_range: Some(replacement_range),
        ..ITerminalCompletion::default()
    };
    completion.set_detail(Some(detail));
    completion
}

fn get_friendly_path(label_service: &dyn ILabelService, uri: &URI, path_separator: &str, kind: TerminalCompletionItemKind, shell_type: TerminalShellType) -> String {
    let mut path = label_service.get_uri_label(uri, &UriLabelOptions { no_prefix: Some(true), ..UriLabelOptions::default() });
    // Normalize line endings for folders
    let separator = if shell_type == Some(ShellType::Windows(WindowsShellType::GitBash)) { "\\" } else { path_separator };
    if kind == TerminalCompletionItemKind::Folder && !path.ends_with(separator) {
        path.push_str(separator);
    }
    path
}

/// Normalize suggestion to add a ./ prefix to the start of the path if there isn't one already. We
/// may want to change this behavior in the future to go with whatever format the user has.
fn add_path_relative_prefix(text: &str, path_separator: &str, last_word_folder_has_dot_prefix: bool) -> String {
    if !last_word_folder_has_dot_prefix {
        if text.starts_with(path_separator) {
            return format!(".{text}");
        }
        return format!(".{path_separator}{text}");
    }
    text.to_string()
}

/// Escapes special characters in a file/folder label for shell completion.
/// This ensures that characters like [, ], etc. are properly escaped.
pub fn escape_terminal_completion_label(label: &str, shell_type: TerminalShellType, _path_separator: &str) -> String {
    // Only escape for bash/zsh/fish; PowerShell and cmd have different rules
    if shell_type.is_none() || shell_type == Some(ShellType::General(GeneralShellType::PowerShell)) || shell_type == Some(ShellType::Windows(WindowsShellType::CommandPrompt)) {
        return label.to_string();
    }
    let mut escaped = String::with_capacity(label.len());
    for character in label.chars() {
        if matches!(character, '[' | ']' | '(' | ')' | '\'' | '"' | '\\' | '`' | '*' | '?' | ';' | '|' | '&' | '<' | '>') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn get_is_absolute_path(shell_type: TerminalShellType, path_separator: &str, last_word: &str, use_windows_style_path: bool) -> bool {
    let starts_with_drive = |separators: &[char]| {
        let mut chars = last_word.chars();
        matches!((chars.next(), chars.next(), chars.next()), (Some(drive), Some(':'), Some(separator)) if drive.is_ascii_alphabetic() && separators.contains(&separator))
    };
    if shell_type == Some(ShellType::Windows(WindowsShellType::GitBash)) {
        return last_word.starts_with(path_separator) || starts_with_drive(&['/']);
    }
    if use_windows_style_path { starts_with_drive(&['\\', '/']) } else { last_word.starts_with(path_separator) }
}

/// Creates a URI from an absolute path, preserving the scheme and authority from the cwd.
/// For local file:// URIs, uses URI.file() which handles Windows path normalization.
/// For remote URIs (e.g., vscode-remote://wsl+Ubuntu), preserves the remote context.
fn create_uri_from_local_path(cwd: &URI, absolute_path: &str) -> URI {
    if cwd.scheme() == "file" {
        return URI::file(absolute_path, host_platform());
    }
    cwd.with(&UriChange { path: Field::from(absolute_path), ..UriChange::default() })
}

/// `str.length` do JavaScript: unidades UTF-16.
fn utf16_len(text: &str) -> i64 {
    text.encode_utf16().count() as i64
}

/// `str.substring(0, end)`: `end` fora do texto é limitado a ele.
fn substring_to(text: &str, end: i64) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = end.clamp(0, units.len() as i64) as usize;
    String::from_utf16_lossy(&units[..end])
}

/// `text.split(/(?<!\\) /)`: separa nos espaços que não vêm depois de uma barra invertida.
fn split_unescaped_spaces(text: &str) -> Vec<String> {
    let mut words = vec![String::new()];
    let mut previous: Option<char> = None;
    for character in text.chars() {
        if character == ' ' && previous != Some('\\') {
            words.push(String::new());
        } else {
            words.last_mut().expect("sempre há uma palavra").push(character);
        }
        previous = Some(character);
    }
    words
}

/// `text.match(/^[a-zA-Z_]+=(?<rhs>.+)$/)?.groups.rhs`: o lado direito de uma atribuição de variável de
/// ambiente (o `.` não casa terminadores de linha).
fn env_var_assignment_value(text: &str) -> Option<String> {
    let name_length = text.find(|character: char| !(character.is_ascii_alphabetic() || character == '_')).unwrap_or(text.len());
    if name_length == 0 {
        return None;
    }
    let rhs = text[name_length..].strip_prefix('=')?;
    if rhs.is_empty() || rhs.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    Some(rhs.to_string())
}

/// `/^\.\.?[\\\/]/.test(text)`.
fn has_dot_prefix(text: &str) -> bool {
    let mut chars = text.chars();
    if chars.next() != Some('.') {
        return false;
    }
    match chars.next() {
        Some('.') => matches!(chars.next(), Some('\\' | '/')),
        Some('\\' | '/') => true,
        _ => false,
    }
}

/// `/^~[\\\/]$/.test(text)`.
fn is_tilde_folder(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next() == Some('~') && matches!(chars.next(), Some('\\' | '/')) && chars.next().is_none()
}
