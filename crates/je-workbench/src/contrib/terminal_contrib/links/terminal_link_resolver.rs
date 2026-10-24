//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLinkResolver.ts`: valida um link contra o
//! sistema de arquivos (resolve o caminho em relação ao cwd inicial e à casa do usuário, e confere se existe) e guarda o
//! resultado num cache por autoridade remota.
//!
//! Decisões de tradução:
//! - `async resolveLink(...)` é uma `async fn` interna embrulhada por [`async_fn`]; o `try`/`catch` em volta do `stat` é o
//!   `match` sobre o resultado da promessa (o `catch` do original engole qualquer erro e vale "não existe").
//! - `isWindows` é a plataforma do hospedeiro (`Platform::detect()`). `OS` também.
//! - `IPath` (`posix` e `win32`) é o [`PathModule`] sobre o ambiente do processo ([`ProcessEnvironment`]).
//! - `mainWindow.setTimeout` do cache é o [`runtime::set_timeout`].
//! - `Map<string, ResolvedLink>` com chave `string | URI` (a URI vira o texto de `toString()`) é um `HashMap<String, ..>`.
//! - `link.match(/^\/mnt\/[a-z]/i)` e `link.match(/^(?:\/\/|\\\\)wsl(?:\$|\.localhost)(\/|\\)/)` são testes de prefixo
//!   escritos à mão, que casam o mesmo texto.
//! - As expressões regulares restantes (`'^' + winDrivePrefix`) rodam no `zyarr`.
//! - `URI.from({ scheme, authority, path })` do original pode lançar para um caminho inválido (o `catch` externo vale
//!   "não existe"); o `URI::from` do porte não devolve erro, então só o `stat` falho cai nesse `catch`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::js_string::JsString;
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::uri_file;
use zyarr::RegExp;

use super::links::{IResolvedValidLink, ITerminalLinkResolver, ResolvedLink, TerminalLinkProcessManager, WslPathDirection};
use super::terminal_link_helpers::os_path_module;
use super::terminal_link_parsing::{WIN_DRIVE_PREFIX, remove_link_query_string, remove_link_suffix};

/// `LinkCacheConstants.TTL`: how long to cache links for in milliseconds, the TTL resets whenever a new value is set in
/// the cache.
const LINK_CACHE_TTL: u64 = 10_000;

/// A chave do cache: `string | URI`.
enum LinkCacheKey<'a> {
    Link(&'a str),
    Uri(&'a URI),
}

impl LinkCacheKey<'_> {
    /// `_getKey(link)`.
    fn key(&self) -> String {
        match self {
            Self::Link(link) => (*link).to_owned(),
            Self::Uri(uri) => uri.to_string(false).to_string_lossy(),
        }
    }
}

/// `LinkCache`.
struct LinkCache {
    cache: Rc<RefCell<HashMap<String, ResolvedLink>>>,
    cache_til_timeout: Cell<Option<TimerId>>,
}

impl LinkCache {
    fn new() -> Self {
        Self { cache: Rc::default(), cache_til_timeout: Cell::new(None) }
    }

    fn set(&self, link: LinkCacheKey<'_>, value: ResolvedLink) {
        // Reset cached link TTL on any set
        if let Some(timeout) = self.cache_til_timeout.take() {
            runtime::clear_timeout(timeout);
        }
        let cache = self.cache.clone();
        self.cache_til_timeout.set(Some(runtime::set_timeout(move || cache.borrow_mut().clear(), LINK_CACHE_TTL)));
        self.cache.borrow_mut().insert(link.key(), value);
    }

    /// `get(link)`: `None` é o `undefined` (não está no cache), e `Some(None)` é o `null` guardado.
    fn get(&self, link: LinkCacheKey<'_>) -> Option<ResolvedLink> {
        self.cache.borrow().get(&link.key()).cloned()
    }
}

/// `TerminalLinkResolver`.
#[derive(Clone)]
pub struct TerminalLinkResolver {
    file_service: Rc<dyn IFileService>,
    // Link cache could be shared across all terminals, but that could lead to weird results when
    // both local and remote terminals are present
    resolved_link_caches: Rc<RefCell<HashMap<String, Rc<LinkCache>>>>,
}

/// `link.match(/^\/mnt\/[a-z]/i)`.
fn is_wsl_mnt_link(link: &str) -> bool {
    link.strip_prefix("/mnt/").and_then(|rest| rest.chars().next()).is_some_and(|c| c.is_ascii_alphabetic())
}

/// `link.match(/^(?:\/\/|\\\\)wsl(?:\$|\.localhost)(\/|\\)/)`.
fn is_wsl_unc_link(link: &str) -> bool {
    let Some(rest) = link.strip_prefix("//").or_else(|| link.strip_prefix("\\\\")) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix("wsl") else {
        return false;
    };
    let Some(rest) = rest.strip_prefix('$').or_else(|| rest.strip_prefix(".localhost")) else {
        return false;
    };
    rest.starts_with(['/', '\\'])
}

impl TerminalLinkResolver {
    pub fn new(file_service: Rc<dyn IFileService>) -> Self {
        Self { file_service, resolved_link_caches: Rc::default() }
    }

    /// `stat` do arquivo e o resultado que vai para o cache (`{ uri, link, isDirectory }`), ou `null` se não existe.
    async fn stat_link(&self, uri: &URI, link: &str, cache: &LinkCache, cache_key: LinkCacheKey<'_>) -> ResolvedLink {
        match self.file_service.stat(uri).await {
            Ok(stat) => {
                let result = IResolvedValidLink { uri: uri.clone(), link: link.to_owned(), is_directory: stat.is_directory };
                cache.set(cache_key, Some(result.clone()));
                Some(result)
            }
            Err(_) => {
                // Does not exist
                cache.set(cache_key, None);
                None
            }
        }
    }

    async fn resolve_link_inner(&self, process_manager: TerminalLinkProcessManager, link: String, uri: Option<URI>) -> Result<ResolvedLink, Error> {
        let platform = Platform::detect();
        // Correct scheme and authority for remote terminals
        let mut uri = uri;
        if let (Some(file_uri), Some(remote_authority)) = (&uri, &process_manager.remote_authority)
            && *file_uri.scheme() == schemas::FILE
        {
            uri = Some(file_uri.with(&UriChange {
                scheme: Field::Set(JsString::from(schemas::VSCODE_REMOTE)),
                authority: Field::Set(JsString::from(remote_authority.as_str())),
                ..UriChange::default()
            }));
        }

        // Get the link cache
        let cache_name = process_manager.remote_authority.clone().unwrap_or_default();
        let cache = {
            let mut caches = self.resolved_link_caches.borrow_mut();
            caches.entry(cache_name).or_insert_with(|| Rc::new(LinkCache::new())).clone()
        };

        // Check resolved link cache first
        let cached = match &uri {
            Some(uri) => cache.get(LinkCacheKey::Uri(uri)),
            None => cache.get(LinkCacheKey::Link(&link)),
        };
        if let Some(cached) = cached {
            return Ok(cached);
        }

        if let Some(uri) = &uri {
            return Ok(self.stat_link(uri, &link, &cache, LinkCacheKey::Uri(uri)).await);
        }

        // Remove any line/col suffix
        let link_units: Vec<u16> = link.encode_utf16().collect();
        let link_url = remove_link_suffix(&link_units);

        // Remove any query string
        let link_url = remove_link_query_string(link_url.units());

        // Exit early if the link is determines as not valid already
        if link_url.is_empty() {
            cache.set(LinkCacheKey::Link(&link), None);
            return Ok(None);
        }
        let mut link_url = link_url.to_string_lossy();

        // If the link looks like a /mnt/ WSL path and this is a Windows frontend, use the backend
        // to get the resolved path from the wslpath util.
        if platform.is_windows && is_wsl_mnt_link(&link) && process_manager.backend.is_some() {
            if let Some(backend) = &process_manager.backend {
                link_url = backend.get_wsl_path(&link_url, WslPathDirection::UnixToWin).await?;
            }
        }
        // Skip preprocessing if it looks like a special Windows -> WSL link
        else if platform.is_windows && is_wsl_unc_link(&link) {
            // No-op, it's already the right format
        }
        // Handle all non-WSL links
        else {
            let preprocessed_link = self.preprocess_path(&link_url, &process_manager.initial_cwd, process_manager.os, process_manager.user_home.as_deref());
            let Some(preprocessed_link) = preprocessed_link else {
                cache.set(LinkCacheKey::Link(&link), None);
                return Ok(None);
            };
            link_url = preprocessed_link;
        }

        let uri = match &process_manager.remote_authority {
            Some(remote_authority) => URI::from(
                &UriComponents {
                    scheme: Some(schemas::VSCODE_REMOTE.into()),
                    authority: Some(remote_authority.as_str().into()),
                    path: Some(link_url.as_str().into()),
                    ..UriComponents::default()
                },
                false,
            ),
            None => uri_file(&link_url),
        };

        Ok(self.stat_link(&uri, &link, &cache, LinkCacheKey::Link(&link)).await)
    }

    /// `_preprocessPath(link, initialCwd, os, userHome)`.
    pub fn preprocess_path(&self, link: &str, initial_cwd: &str, os: Option<OperatingSystem>, user_home: Option<&str>) -> Option<String> {
        let environment = ProcessEnvironment;
        let os = os.unwrap_or(Platform::detect().os);
        let os_path: PathModule<'_> = os_path_module(os, &environment);
        let mut link = link.to_owned();
        let first = link.chars().next();
        if first == Some('~') {
            // Resolve ~ -> userHome
            let user_home = user_home.filter(|user_home| !user_home.is_empty())?;
            link = os_path.join(&[user_home, &link[1..]]);
        } else if first != Some('/') && first != Some('~') {
            // Resolve workspace path . | .. | <relative_path> -> <path>/. | <path>/.. | <path>/<relative_path>
            if os == OperatingSystem::Windows {
                let drive_prefix = RegExp::from_str(&format!("^{WIN_DRIVE_PREFIX}"), "").expect("a expressão do prefixo de unidade é válida");
                let link_units: Vec<u16> = link.encode_utf16().collect();
                let has_drive_prefix = drive_prefix.exec(&link_units, 0).is_some();
                if !has_drive_prefix && !link.starts_with("\\\\?\\") {
                    if initial_cwd.is_empty() {
                        // Abort if no workspace is open
                        return None;
                    }
                    link = os_path.join(&[initial_cwd, &link]);
                } else {
                    // Remove \\?\ from paths so that they share the same underlying
                    // uri and don't open multiple tabs for the same file
                    if let Some(rest) = link.strip_prefix("\\\\?\\") {
                        link = rest.to_owned();
                    }
                }
            } else {
                if initial_cwd.is_empty() {
                    // Abort if no workspace is open
                    return None;
                }
                link = os_path.join(&[initial_cwd, &link]);
            }
        }
        Some(os_path.normalize(&link))
    }
}

impl ITerminalLinkResolver for TerminalLinkResolver {
    fn resolve_link(&self, process_manager: &TerminalLinkProcessManager, link: &str, uri: Option<URI>) -> Promise<ResolvedLink> {
        let this = self.clone();
        let process_manager = process_manager.clone();
        let link = link.to_owned();
        async_fn(async move { this.resolve_link_inner(process_manager, link, uri).await })
    }
}
