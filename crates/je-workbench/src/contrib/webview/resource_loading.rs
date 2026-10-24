//! Porte de `src/vs/workbench/contrib/webview/browser/resourceLoading.ts`: a leitura dos recursos locais que um webview
//! pede (`vscode-resource`), com a conferência de que o recurso fica dentro de uma das raízes permitidas.
//!
//! Notas de tradução:
//! - **`namespace WebviewResourceResponse`.** É o módulo [`webview_resource_response`]: `Type` é o enum
//!   [`webview_resource_response::Type`], `StreamSuccess` e `NotModified` são structs, e a união `StreamResponse`
//!   (`StreamSuccess | Failed | AccessDenied | NotModified`) é o enum [`StreamResponse`], cujo discriminante lido por
//!   `response.type` é [`StreamResponse::response_type`]. As constantes `Failed` e `AccessDenied` são as variantes.
//! - **`loadLocalResource(accessor, ...)`.** O `accessor` é o [`ServicesAccessor`]; os três serviços são lidos antes do
//!   primeiro `await`, como no original, e a função `async` é um [`async_fn`] (o corpo roda já, até o primeiro `await`).
//! - **`err.options?.mtime`.** O `NotModified` lê `mtime` das opções do erro, que são as `IReadFileOptions` da leitura, e
//!   essas não têm `mtime`: o valor é sempre `undefined`, ou seja, `None`.
//! - **`isWindows`.** É a plataforma do hospedeiro ([`host_platform`]); `isUNC` e `fsPath` leem a mesma.
//! - **`WINDOWS_PARENT_TRAVERSAL_SEGMENT`.** A expressão `/(?:^|[\\/])\.\. +(?=$|[\\/])/` casa um segmento do caminho
//!   (entre `/` ou `\`) que é exatamente `..` seguido de um ou mais espaços; é a função
//!   [`is_windows_parent_traversal_segment`], sem expressão regular.

use je_base::async_::async_fn;
use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::extpath::is_unc;
use je_base::network::schemas;
use je_base::path::{Platform, PathModule};
use je_base::platform::OperatingSystem;
use je_base::resources::{ExtUri, original_fs_path};
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_base::JsString;
use je_instantiation::ServicesAccessor;
use je_platform::files::files::{FileOperationError, FileOperationResult, ReadFileOptions};
use je_platform::files::i_file_service::I_FILE_SERVICE;
use je_platform::log::{I_LOG_SERVICE, Logger};
use je_platform::resources::{host_environment, host_platform};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::webview::mime_types::get_webview_content_mime_type;

/// `namespace WebviewResourceResponse`.
pub mod webview_resource_response {
    use super::VSBufferReadableStream;

    /// `enum Type { Success, Failed, AccessDenied, NotModified }`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Type {
        Success,
        Failed,
        AccessDenied,
        NotModified,
    }

    /// `StreamSuccess`.
    #[derive(Clone)]
    pub struct StreamSuccess {
        pub stream: VSBufferReadableStream,
        pub etag: Option<String>,
        pub mtime: Option<i64>,
        pub mime_type: String,
        pub size: u64,
    }

    impl StreamSuccess {
        pub fn new(stream: VSBufferReadableStream, etag: Option<String>, mtime: Option<i64>, mime_type: String, size: u64) -> Self {
            Self { stream, etag, mtime, mime_type, size }
        }

        /// `readonly type = Type.Success`.
        pub fn response_type(&self) -> Type {
            Type::Success
        }
    }

    /// `NotModified`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct NotModified {
        pub mime_type: String,
        pub mtime: Option<i64>,
    }

    impl NotModified {
        pub fn new(mime_type: String, mtime: Option<i64>) -> Self {
            Self { mime_type, mtime }
        }

        /// `readonly type = Type.NotModified`.
        pub fn response_type(&self) -> Type {
            Type::NotModified
        }
    }
}

use webview_resource_response::{NotModified, StreamSuccess, Type};

/// `WebviewResourceResponse.StreamResponse`: `StreamSuccess | typeof Failed | typeof AccessDenied | NotModified`.
#[derive(Clone)]
pub enum StreamResponse {
    Success(StreamSuccess),
    /// `WebviewResourceResponse.Failed`.
    Failed,
    /// `WebviewResourceResponse.AccessDenied`.
    AccessDenied,
    NotModified(NotModified),
}

impl StreamResponse {
    /// `response.type`.
    pub fn response_type(&self) -> Type {
        match self {
            StreamResponse::Success(_) => Type::Success,
            StreamResponse::Failed => Type::Failed,
            StreamResponse::AccessDenied => Type::AccessDenied,
            StreamResponse::NotModified(_) => Type::NotModified,
        }
    }
}

/// `options.range`: `{ start: number; end?: number }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceRange {
    pub start: u64,
    pub end: Option<u64>,
}

/// O parâmetro `options` de `loadLocalResource`.
#[derive(Clone, Debug, Default)]
pub struct LoadLocalResourceOptions {
    pub if_none_match: Option<String>,
    pub roots: Vec<URI>,
    pub remote_os: Option<OperatingSystem>,
    pub range: Option<ResourceRange>,
}

/// `${uri}` num template literal: o `toString()` da URI, ou `undefined`.
fn display(resource: Option<&URI>) -> String {
    resource.map_or_else(|| "undefined".to_owned(), |uri| uri.to_string(false).to_string_lossy())
}

/// `loadLocalResource(accessor, requestUri, options, token)`.
pub fn load_local_resource(
    accessor: &ServicesAccessor,
    request_uri: &URI,
    options: &LoadLocalResourceOptions,
    token: &CancellationToken,
) -> je_base::promise::Promise<StreamResponse> {
    let uri_identity_service = accessor.get(I_URI_IDENTITY_SERVICE);
    let file_service = accessor.get(I_FILE_SERVICE);
    let log_service = accessor.get(I_LOG_SERVICE);

    let resource_to_load = get_resource_to_load(request_uri, &options.roots, &*uri_identity_service, options.remote_os);

    log_service.trace(&format!(
        "Webview.loadLocalResource - trying to load resource. requestUri={}, resourceToLoad={}",
        display(Some(request_uri)),
        display(resource_to_load.as_ref())
    ));

    let Some(resource_to_load) = resource_to_load else {
        log_service.trace(&format!(
            "Webview.loadLocalResource - access denied. requestUri={}, resourceToLoad={}",
            display(Some(request_uri)),
            display(None)
        ));
        return je_base::promise::Promise::resolved(StreamResponse::AccessDenied);
    };

    let mime = get_webview_content_mime_type(request_uri); // Use the original path for the mime

    let (request_uri, if_none_match, range, token) = (request_uri.clone(), options.if_none_match.clone(), options.range, token.clone());
    async_fn(async move {
        let mut read_options = ReadFileOptions { etag: if_none_match, ..ReadFileOptions::default() };
        if let Some(range) = range {
            read_options.stream.position = Some(range.start);
            if let Some(end) = range.end {
                if end < range.start {
                    return Ok(StreamResponse::Failed);
                }
                read_options.stream.length = Some(end - range.start + 1);
            }
        }
        match file_service.read_file_stream(&resource_to_load, Some(read_options), Some(token)).await {
            Ok(result) => {
                log_service.trace(&format!(
                    "Webview.loadLocalResource - Loaded. requestUri={}, resourceToLoad={}",
                    display(Some(&request_uri)),
                    display(Some(&resource_to_load))
                ));
                Ok(StreamResponse::Success(StreamSuccess::new(
                    result.value.clone(),
                    Some(result.etag().to_owned()),
                    Some(result.mtime()),
                    mime,
                    result.size(),
                )))
            }
            Err(err) => {
                if let Some(operation_error) = FileOperationError::from_error(&err) {
                    // NotModified status is expected and can be handled gracefully
                    if operation_error.file_operation_result == FileOperationResult::FileNotModifiedSince {
                        log_service.trace(&format!(
                            "Webview.loadLocalResource - not modified. requestUri={}, resourceToLoad={}",
                            display(Some(&request_uri)),
                            display(Some(&resource_to_load))
                        ));
                        return Ok(StreamResponse::NotModified(NotModified::new(mime, None)));
                    }
                }

                // Otherwise the error is unexpected.
                log_service.error(&format!(
                    "Webview.loadLocalResource - Error using fileReader. requestUri={}, resourceToLoad={}",
                    display(Some(&request_uri)),
                    display(Some(&resource_to_load))
                ));
                Ok(StreamResponse::Failed)
            }
        }
    })
}

/// `getResourceToLoad(requestUri, roots, uriIdentityService, remoteOS?)`.
pub fn get_resource_to_load(
    request_uri: &URI,
    roots: &[URI],
    uri_identity_service: &dyn IUriIdentityService,
    remote_os: Option<OperatingSystem>,
) -> Option<URI> {
    let mut request_uri = request_uri.clone();
    let mut roots: Vec<URI> = roots.to_vec();
    if request_uri.scheme() == schemas::VSCODE_REMOTE {
        let remote_os = remote_os?;
        request_uri = normalize_remote_resource(&request_uri, remote_os);
        roots = roots
            .iter()
            .map(|root| if root.scheme() == schemas::VSCODE_REMOTE { normalize_remote_resource(root, remote_os) } else { root.clone() })
            .collect();
    }
    let request_uri_no_query_string = request_uri.with(&UriChange { query: Field::Clear, ..UriChange::default() });
    if has_windows_parent_traversal_segment(&request_uri_no_query_string) {
        return None;
    }

    let force_posix_semantics =
        request_uri.scheme() == schemas::VSCODE_REMOTE && matches!(remote_os, Some(OperatingSystem::Linux | OperatingSystem::Macintosh));
    let ext_uri = uri_identity_service.ext_uri();
    for root in &roots {
        if contains_resource(root, &request_uri_no_query_string, &ext_uri, force_posix_semantics) {
            return Some(normalize_resource_path(&request_uri));
        }
    }

    None
}

fn contains_resource(root: &URI, resource: &URI, ext_uri: &ExtUri, force_posix_semantics: bool) -> bool {
    if ext_uri.is_equal(Some(root), Some(resource), /* ignoreFragment */ true) {
        return false;
    }

    // Compare unc paths case-insensitively
    let env = host_environment();
    let platform = host_platform();
    let is_unc_file = |uri: &URI| uri.scheme() == schemas::FILE && is_unc(&uri.fs_path(platform).to_string_lossy(), env.as_ref());
    if is_unc_file(root) {
        if is_unc_file(resource) {
            let lowered = |uri: &URI| {
                uri.with(&UriChange {
                    path: Field::Set(uri.path().to_lower_case()),
                    authority: Field::Set(uri.authority().to_lower_case()),
                    ..UriChange::default()
                })
            };
            return ext_uri.is_equal_or_parent(&lowered(resource), &lowered(root), /* ignoreFragment */ true, false);
        }
        return false;
    }

    ext_uri.is_equal_or_parent(resource, root, /* ignoreFragment */ true, force_posix_semantics)
}

/// `WINDOWS_PARENT_TRAVERSAL_SEGMENT.test(path)`, para um segmento: `..` seguido só de espaços (ao menos um).
fn is_windows_parent_traversal_segment(segment: &str) -> bool {
    segment.strip_prefix("..").is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c == ' '))
}

fn has_windows_parent_traversal_segment(resource: &URI) -> bool {
    matches!(host_platform(), Platform::Win32)
        && resource.scheme() == schemas::FILE
        && resource.path().to_string_lossy().split(['\\', '/']).any(is_windows_parent_traversal_segment)
}

fn normalize_remote_resource(resource: &URI, remote_os: OperatingSystem) -> URI {
    let env = host_environment();
    let path = JsString::from(if remote_os == OperatingSystem::Windows {
        let file_resource = URI::from(
            &UriComponents { scheme: Some(JsString::from(schemas::FILE)), path: Some(resource.path().clone()), ..UriComponents::default() },
            false,
        );
        let path = original_fs_path(&file_resource, host_platform()).to_string_lossy();
        let normalized_path = PathModule::win32(env.as_ref()).normalize(&path).replace('\\', "/");
        if normalized_path.starts_with('/') { normalized_path } else { format!("/{normalized_path}") }
    } else {
        PathModule::posix(env.as_ref()).normalize(&resource.path().to_string_lossy())
    });
    resource.with(&UriChange { path: Field::Set(path), ..UriChange::default() })
}

fn normalize_resource_path(resource: &URI) -> URI {
    // Rewrite remote uris to a path that the remote file system can understand
    if resource.scheme() == schemas::VSCODE_REMOTE {
        let query = json_stringify(&Dynamic::object([("requestResourcePath", Dynamic::string(&resource.path().to_string_lossy()))]))
            .unwrap_or_default();
        return URI::from(
            &UriComponents {
                scheme: Some(JsString::from(schemas::VSCODE_REMOTE)),
                authority: Some(resource.authority().clone()),
                path: Some(JsString::from("/vscode-resource")),
                query: Some(JsString::from(query)),
                ..UriComponents::default()
            },
            false,
        );
    }
    resource.clone()
}
