//! Porte de `src/vs/base/common/network.ts`: os esquemas conhecidos (`Schemas`), `matchesScheme`,
//! `RemoteAuthorities` e `FileAccess`.
//!
//! O original usa singletons (`RemoteAuthorities`, `FileAccess`) que leem `platform.isNative`,
//! `platform.isWeb`, `platform.webWorkerOrigin` e `globalThis._VSCODE_FILE_ROOT`. Aqui são structs com
//! esse contexto injetado, para o teste fixar o ambiente (o `FileAccess` é dono do `RemoteAuthorities`).
//!
//! Não portado: o namespace `COI` (cabeçalhos de isolamento de origem, que depende de `URL` e
//! `URLSearchParams` da web), que nenhum teste da base usa.

use std::collections::HashMap;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::path::{Environment, FixedEnvironment, PathModule, Platform};
use crate::strings::{equals_ignore_case, starts_with_ignore_case};
use crate::uri::{encode_uri_component, Field, UriChange, UriComponents, URI};
use crate::JsString;

/// O namespace `Schemas` do original.
pub mod schemas {
    /// A schema that is used for models that exist in memory only and that have no correspondence on a server or such.
    pub const IN_MEMORY: &str = "inmemory";
    /// A schema that is used for setting files
    pub const VSCODE: &str = "vscode";
    /// A schema that is used for internal private files
    pub const INTERNAL: &str = "private";
    /// A walk-through document.
    pub const WALK_THROUGH: &str = "walkThrough";
    /// An embedded code snippet.
    pub const WALK_THROUGH_SNIPPET: &str = "walkThroughSnippet";
    pub const VSCODE_ONBOARDING_SAMPLE: &str = "vscode-onboarding-sample";
    pub const HTTP: &str = "http";
    pub const HTTPS: &str = "https";
    pub const FILE: &str = "file";
    pub const MAILTO: &str = "mailto";
    pub const UNTITLED: &str = "untitled";
    pub const DATA: &str = "data";
    pub const COMMAND: &str = "command";
    pub const VSCODE_REMOTE: &str = "vscode-remote";
    pub const VSCODE_REMOTE_RESOURCE: &str = "vscode-remote-resource";
    pub const VSCODE_MANAGED_REMOTE_RESOURCE: &str = "vscode-managed-remote-resource";
    pub const VSCODE_USER_DATA: &str = "vscode-userdata";
    pub const VSCODE_CUSTOM_EDITOR: &str = "vscode-custom-editor";
    pub const VSCODE_NOTEBOOK_CELL: &str = "vscode-notebook-cell";
    pub const VSCODE_NOTEBOOK_CELL_METADATA: &str = "vscode-notebook-cell-metadata";
    pub const VSCODE_NOTEBOOK_CELL_METADATA_DIFF: &str = "vscode-notebook-cell-metadata-diff";
    pub const VSCODE_NOTEBOOK_CELL_OUTPUT: &str = "vscode-notebook-cell-output";
    pub const VSCODE_NOTEBOOK_CELL_OUTPUT_DIFF: &str = "vscode-notebook-cell-output-diff";
    pub const VSCODE_NOTEBOOK_METADATA: &str = "vscode-notebook-metadata";
    pub const VSCODE_INTERACTIVE_INPUT: &str = "vscode-interactive-input";
    pub const VSCODE_SETTINGS: &str = "vscode-settings";
    pub const VSCODE_WORKSPACE_TRUST: &str = "vscode-workspace-trust";
    pub const VSCODE_TERMINAL: &str = "vscode-terminal";
    /// Scheme used for the image carousel editor.
    pub const VSCODE_IMAGE_CAROUSEL: &str = "vscode-image-carousel";
    /// Scheme used for code blocks in chat.
    pub const VSCODE_CHAT_CODE_BLOCK: &str = "vscode-chat-code-block";
    /// Scheme used for LHS of code compare (aka diff) blocks in chat.
    pub const VSCODE_CHAT_CODE_COMPARE_BLOCK: &str = "vscode-chat-code-compare-block";
    /// Scheme used for the chat input editor.
    pub const VSCODE_CHAT_EDITOR: &str = "vscode-chat-editor";
    /// Scheme used for the chat input part
    pub const VSCODE_CHAT_INPUT: &str = "chatSessionInput";
    /// Scheme used for the Agents window new-session composer input
    pub const SESSIONS_CHAT_INPUT: &str = "sessions-chat";
    /// Scheme used for local chat session content
    pub const VSCODE_LOCAL_CHAT_SESSION: &str = "vscode-chat-session";
    /// Scheme used for read-only resources owned by a chat response or attachment
    pub const VSCODE_CHAT_RESPONSE_RESOURCE: &str = "vscode-chat-response-resource";
    /// Scheme used for Agent Host terminal channels.
    pub const AGENT_HOST_TERMINAL: &str = "agenthost-terminal";
    /// Scheme used for subscription-backed Agent Host terminal output editors.
    pub const VSCODE_CHAT_TERMINAL_OUTPUT: &str = "vscode-chat-terminal-output";
    /// Scheme used internally for webviews that aren't linked to a resource (i.e. not custom editors)
    pub const WEBVIEW_PANEL: &str = "webview-panel";
    /// Scheme used for loading the wrapper html and script in webviews.
    pub const VSCODE_WEBVIEW: &str = "vscode-webview";
    /// Scheme used for integrated browser tabs using WebContentsView.
    pub const VSCODE_BROWSER: &str = "vscode-browser";
    /// Scheme used for extension pages
    pub const EXTENSION: &str = "extension";
    /// Scheme used as a replacement of `file` scheme to load files with our custom protocol handler (desktop only).
    pub const VSCODE_FILE_RESOURCE: &str = "vscode-file";
    /// Scheme used for temporary resources
    pub const TMP: &str = "tmp";
    /// Scheme used vs live share
    pub const VSLS: &str = "vsls";
    /// Scheme used for the Source Control commit input's text document
    pub const VSCODE_SOURCE_CONTROL: &str = "vscode-scm";
    /// Scheme used for input box for creating comments.
    pub const COMMENTS_INPUT: &str = "comment";
    /// Scheme used for special rendering of settings in the release notes
    pub const CODE_SETTING: &str = "code-setting";
    /// Scheme used for output panel resources
    pub const OUTPUT_CHANNEL: &str = "output";
    /// Scheme used for the accessible view
    pub const ACCESSIBLE_VIEW: &str = "accessible-view";
    /// Used for snapshots of chat edits
    pub const CHAT_EDITING_SNAPSHOT_SCHEME: &str = "chat-editing-snapshot-text-model";
    pub const CHAT_EDITING_MODEL: &str = "chat-editing-text-model";
    /// Used for rendering multidiffs in copilot agent sessions
    pub const COPILOT_PR: &str = "copilot-pr";
}

/// `URI | string`, o tipo do alvo de `matchesScheme`.
#[derive(Clone, Copy, Debug)]
pub enum UriOrString<'a> {
    Uri(&'a URI),
    Str(&'a str),
}

pub fn matches_scheme(target: UriOrString<'_>, scheme: &str) -> bool {
    match target {
        UriOrString::Uri(uri) => equals_ignore_case(uri.scheme().units(), &scheme.encode_utf16().collect::<Vec<_>>()),
        UriOrString::Str(text) => starts_with_ignore_case(
            &text.encode_utf16().collect::<Vec<_>>(),
            &format!("{scheme}:").encode_utf16().collect::<Vec<_>>(),
        ),
    }
}

pub fn matches_some_scheme(target: UriOrString<'_>, schemes: &[&str]) -> bool {
    schemes.iter().any(|scheme| matches_scheme(target, scheme))
}

pub const CONNECTION_TOKEN_COOKIE_NAME: &str = "vscode-tkn";
pub const CONNECTION_TOKEN_QUERY_NAME: &str = "tkn";

/// O `{ quality?: string; commit?: string }` de `getServerProductSegment`.
#[derive(Clone, Debug, Default)]
pub struct ServerProduct {
    pub quality: Option<String>,
    pub commit: Option<String>,
}

pub fn get_server_product_segment(product: &ServerProduct) -> String {
    format!("{}-{}", product.quality.as_deref().unwrap_or("oss"), product.commit.as_deref().unwrap_or("dev"))
}

/// O `'http' | 'https'` de `setPreferredWebSchema`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSchema {
    Http,
    Https,
}

impl WebSchema {
    fn as_str(self) -> &'static str {
        match self {
            WebSchema::Http => schemas::HTTP,
            WebSchema::Https => schemas::HTTPS,
        }
    }
}

type RewriteDelegate = Box<dyn Fn(&URI) -> URI>;

/// `RemoteAuthoritiesImpl`.
pub struct RemoteAuthorities {
    is_web: bool,
    hosts: HashMap<String, String>,
    ports: HashMap<String, u32>,
    connection_tokens: HashMap<String, String>,
    preferred_web_schema: WebSchema,
    delegate: Option<RewriteDelegate>,
    server_root_path: String,
}

fn posix_env() -> FixedEnvironment {
    FixedEnvironment::new(Platform::Posix, "/")
}

impl RemoteAuthorities {
    /// `is_web` é o `platform.isWeb` do original.
    pub fn new(is_web: bool) -> Self {
        Self {
            is_web,
            hosts: HashMap::new(),
            ports: HashMap::new(),
            connection_tokens: HashMap::new(),
            preferred_web_schema: WebSchema::Http,
            delegate: None,
            server_root_path: "/".to_string(),
        }
    }

    pub fn set_preferred_web_schema(&mut self, schema: WebSchema) {
        self.preferred_web_schema = schema;
    }

    pub fn set_delegate(&mut self, delegate: impl Fn(&URI) -> URI + 'static) {
        self.delegate = Some(Box::new(delegate));
    }

    pub fn set_server_root_path(&mut self, product: &ServerProduct, server_base_path: Option<&str>) {
        let env = posix_env();
        self.server_root_path =
            PathModule::posix(&env).join(&[server_base_path.unwrap_or("/"), &get_server_product_segment(product)]);
    }

    pub fn get_server_root_path(&self) -> &str {
        &self.server_root_path
    }

    fn remote_resources_path(&self) -> String {
        let env = posix_env();
        PathModule::posix(&env).join(&[&self.server_root_path, schemas::VSCODE_REMOTE_RESOURCE])
    }

    pub fn set(&mut self, authority: &str, host: &str, port: u32) {
        self.hosts.insert(authority.to_string(), host.to_string());
        self.ports.insert(authority.to_string(), port);
    }

    pub fn set_connection_token(&mut self, authority: &str, connection_token: &str) {
        self.connection_tokens.insert(authority.to_string(), connection_token.to_string());
    }

    pub fn get_preferred_web_schema(&self) -> WebSchema {
        self.preferred_web_schema
    }

    pub fn rewrite(&self, uri: &URI) -> URI {
        if let Some(delegate) = &self.delegate {
            return match errors::catch(|| delegate(uri)) {
                Ok(rewritten) => rewritten,
                Err(err) => {
                    errors::on_unexpected_external_error(&err);
                    uri.clone()
                }
            };
        }
        let authority = uri.authority().to_string_lossy();
        // `${host}:${port}` imprime "undefined" quando a autoridade não foi registrada.
        let mut host = self.hosts.get(&authority).cloned();
        if let Some(h) = &host
            && h.contains(':')
            && !h.contains('[')
        {
            host = Some(format!("[{h}]"));
        }
        let host_text = host.unwrap_or_else(|| "undefined".to_string());
        let port_text = self.ports.get(&authority).map_or_else(|| "undefined".to_string(), ToString::to_string);

        let mut query = JsString::from("path=");
        query.push_units(&encode_uri_component(uri.path().units()));
        if let Some(connection_token) = self.connection_tokens.get(&authority) {
            query.push_units(&format!("&{CONNECTION_TOKEN_QUERY_NAME}=").encode_utf16().collect::<Vec<_>>());
            query.push_units(&encode_uri_component(&connection_token.encode_utf16().collect::<Vec<_>>()));
        }
        let scheme = if self.is_web { self.preferred_web_schema.as_str() } else { schemas::VSCODE_REMOTE_RESOURCE };
        URI::from(
            &UriComponents {
                scheme: Some(scheme.into()),
                authority: Some(format!("{host_text}:{port_text}").into()),
                path: Some(self.remote_resources_path().into()),
                query: Some(query),
                fragment: None,
            },
            false,
        )
    }
}

/// A string pointing to a path inside the app. It should not begin with ./ or ../
pub type AppResourcePath = str;

pub const BUILTIN_EXTENSIONS_PATH: &str = "vs/../../extensions";
pub const NODE_MODULES_PATH: &str = "vs/../../node_modules";
pub const NODE_MODULES_ASAR_PATH: &str = "vs/../../node_modules.asar";
pub const NODE_MODULES_ASAR_UNPACKED_PATH: &str = "vs/../../node_modules.asar.unpacked";

pub const AGENTS_AUTHORITY: &str = "agents";
pub const VSCODE_AUTHORITY: &str = "vscode-app";

/// O contexto global que o `FileAccessImpl` lê no original.
#[derive(Clone, Debug, Default)]
pub struct FileAccessContext {
    /// `platform.isNative`.
    pub is_native: bool,
    /// `platform.webWorkerOrigin`.
    pub web_worker_origin: Option<String>,
    /// `globalThis._VSCODE_FILE_ROOT`: URL (com esquema) ou caminho da raiz dos arquivos do app.
    pub file_root: Option<String>,
}

pub struct FileAccess {
    env: Rc<dyn Environment>,
    context: FileAccessContext,
    remote_authorities: RemoteAuthorities,
}

/// `/^\w[\w\d+.-]*:\/\//`.
fn has_url_scheme(text: &str) -> bool {
    let Some((scheme, _)) = text.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '.' | '-'))
}

impl FileAccess {
    pub fn new(env: Rc<dyn Environment>, context: FileAccessContext, remote_authorities: RemoteAuthorities) -> Self {
        Self { env, context, remote_authorities }
    }

    pub fn remote_authorities(&mut self) -> &mut RemoteAuthorities {
        &mut self.remote_authorities
    }

    /// Returns a URI to use in contexts where the browser is responsible for loading (e.g. fetch())
    /// or when used within the DOM.
    pub fn as_browser_uri(&self, resource_path: &AppResourcePath) -> URI {
        let uri = self.to_uri(resource_path);
        self.uri_to_browser_uri(&uri)
    }

    pub fn uri_to_browser_uri(&self, uri: &URI) -> URI {
        // Handle remote URIs via `RemoteAuthorities`
        if uri.scheme() == schemas::VSCODE_REMOTE {
            return self.remote_authorities.rewrite(uri);
        }

        // Convert to `vscode-file` resource..
        if
        // ...only ever for `file` resources
        uri.scheme() == schemas::FILE
            && (
                // ...and we run in native environments
                self.context.is_native
                // ...or web worker extensions on desktop
                || self.context.web_worker_origin.as_deref()
                    == Some(&format!("{}://{}", schemas::VSCODE_FILE_RESOURCE, VSCODE_AUTHORITY))
            )
        {
            return uri.with(&UriChange {
                scheme: schemas::VSCODE_FILE_RESOURCE.into(),
                // We need to provide an authority here so that it can serve as origin for network and
                // loading matters in chromium. If the URI is not coming with an authority already, we add our own
                authority: if uri.authority().is_empty() { VSCODE_AUTHORITY.into() } else { Field::Set(uri.authority().clone()) },
                path: Field::Keep,
                query: Field::Clear,
                fragment: Field::Clear,
            });
        }

        uri.clone()
    }

    /// Returns the `file` URI to use in contexts where node.js is responsible for loading.
    pub fn as_file_uri(&self, resource_path: &AppResourcePath) -> URI {
        let uri = self.to_uri(resource_path);
        self.uri_to_file_uri(&uri)
    }

    pub fn uri_to_file_uri(&self, uri: &URI) -> URI {
        // Only convert the URI if it is `vscode-file:` scheme
        if uri.scheme() == schemas::VSCODE_FILE_RESOURCE {
            return uri.with(&UriChange {
                scheme: schemas::FILE.into(),
                // Only preserve the `authority` if it is different from our fallback authority. This
                // ensures we properly preserve Windows UNC paths that come with their own authority.
                authority: if uri.authority() != VSCODE_AUTHORITY { Field::Set(uri.authority().clone()) } else { Field::Clear },
                path: Field::Keep,
                query: Field::Clear,
                fragment: Field::Clear,
            });
        }

        uri.clone()
    }

    fn to_uri(&self, module: &str) -> URI {
        let Some(root) = self.context.file_root.as_ref().filter(|root| !root.is_empty()) else {
            errors::throw(Error::new("Cannot determine URI for module id!"));
        };

        // File URL (with scheme)
        if has_url_scheme(root) {
            return URI::join_path(&URI::parse(root.as_str(), true), &[module], self.env.platform());
        }

        // File Path (no scheme)
        let module_path = PathModule::native(self.env.as_ref()).join(&[root, module]);
        URI::file(module_path, self.env.platform())
    }
}

pub const CACHE_CONTROL_HEADERS: [(&str, &str); 1] = [("Cache-Control", "no-cache, no-store")];

pub const DOCUMENT_POLICY_HEADERS: [(&str, &str); 1] = [("Document-Policy", "include-js-call-stacks-in-crash-reports")];
