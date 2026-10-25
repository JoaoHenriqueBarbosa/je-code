//! Porte de `src/vs/platform/protocol/electron-main/protocolMainService.ts`: o `ProtocolMainService`, que serve os arquivos do
//! aplicativo pelo protocolo `vscode-file:` (só de raízes e extensões permitidas), recusa qualquer `file:`, registra o
//! protocolo dos recursos remotos gerenciados e publica objetos para os renderizadores pelo IPC.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único. Decisões de tradução:
//! - **`session.defaultSession.protocol`** é o trait [`ProtocolSession`] (`handle` e `unhandle` de um esquema), que a pilha
//!   nativa de rede implementa.
//! - **`validatedIpcMain`** é o trait [`ValidatedIpcMain`] (`handle` e `removeHandler`), que o IPC nativo implementa.
//! - **`net.fetch(pathToFileURL(path), { method, headers, bypassCustomProtocolHandlers })`** do arquivo é a leitura direta do
//!   arquivo, com o `Content-Type` pela extensão, o `Content-Length` e o `Range` de uma faixa só (o que o carregador de
//!   `file:` do Chromium atende e o `<video>` usa para avançar). Um arquivo que não se lê (inexistente, diretório) é o
//!   `catch` do original: loga e devolve `Response.error()`.
//! - **O ciclo `this` dentro dos tratadores** não existe: eles levam só o [`ResourceLoader`] (as raízes e o ambiente), que não
//!   guarda o serviço; o serviço guarda as inscrições para desfazer ao ser descartado.
//! - **`FileAccess.uriToFileUri`** é a de um [`FileAccess`] nativo, criado aqui (o original usa o singleton global).
//! - **`createIPCObjectUrl<T>`** carrega [`Dynamic`] (ver [`super::protocol`]).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::{self, Disposable, DisposableRef, IDisposable};
use je_base::mime::get_media_or_text_mime;
use je_base::network::{CACHE_CONTROL_HEADERS, DOCUMENT_POLICY_HEADERS, FileAccess, FileAccessContext, RemoteAuthorities, schemas};
use je_base::node::pfs;
use je_base::path::{Environment, PathModule, ProcessEnvironment};
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::{Field, UriChange, UriComponents, URI};
use je_base::uuid::generate_uuid;

use super::coi::{COOP_AND_COEP, get_headers_from_query};
use super::fetch::{GlobalHeaders, GlobalRequest, GlobalResponse, ProtocolHandler, ResponseBody};
use super::managed_remote_resource_protocol::create_managed_remote_resource_request_handler;
use super::protocol::{IIPCObjectUrl, IProtocolMainService, RequestRemoteResource};
use crate::environment::environment::NativeEnvironmentServiceApi;
use crate::log::{LogArg, LogMessage, LogServiceRef, Logger};
use crate::user_data_profile::user_data_profile::IUserDataProfilesService;

/// `session.defaultSession.protocol`: o registro dos tratadores de protocolo.
pub trait ProtocolSession {
    /// `protocol.handle(scheme, handler)`.
    fn handle(&self, scheme: &str, handler: ProtocolHandler);
    /// `protocol.unhandle(scheme)`.
    fn unhandle(&self, scheme: &str);
}

/// `validatedIpcMain`: o registro dos tratadores de `ipcRenderer.invoke(channel)`.
pub trait ValidatedIpcMain {
    /// `ipcMain.handle(channel, handler)`.
    fn handle(&self, channel: &str, handler: Rc<dyn Fn() -> Promise<Dynamic>>);
    /// `ipcMain.removeHandler(channel)`.
    fn remove_handler(&self, channel: &str);
}

/// O `path` nativo do processo.
fn native_path() -> PathModule<'static> {
    PathModule::native(&ProcessEnvironment)
}

/// `{ ...headers, ...extra }`.
fn merge_headers(headers: Option<GlobalHeaders>, extra: &[(&str, &str)]) -> GlobalHeaders {
    let mut headers = headers.unwrap_or_default();
    for (name, value) in extra {
        headers.set(name, value);
    }
    headers
}

/// O `Range` de um pedido, já resolvido contra o tamanho do arquivo.
enum ByteRange {
    Whole,
    Partial { start: usize, end: usize },
    Unsatisfiable,
}

/// Lê o cabeçalho `Range: bytes=...` (uma faixa só; várias faixas, ou um valor que não se entende, servem o arquivo inteiro).
fn parse_range(header: Option<&str>, total: usize) -> ByteRange {
    let Some(spec) = header.and_then(|header| header.trim().strip_prefix("bytes=")) else {
        return ByteRange::Whole;
    };
    let Some((first, last)) = spec.split_once('-').filter(|_| !spec.contains(',')) else {
        return ByteRange::Whole;
    };
    let (first, last) = (first.trim(), last.trim());

    match (first.parse::<usize>(), last.parse::<usize>()) {
        (Ok(start), Ok(end)) if start <= end => {
            if start >= total {
                ByteRange::Unsatisfiable
            } else {
                ByteRange::Partial { start, end: end.min(total - 1) }
            }
        }
        (Ok(start), Err(_)) if last.is_empty() => {
            if start >= total {
                ByteRange::Unsatisfiable
            } else {
                ByteRange::Partial { start, end: total - 1 }
            }
        }
        (Err(_), Ok(suffix)) if first.is_empty() && suffix > 0 => {
            if total == 0 {
                ByteRange::Unsatisfiable
            } else {
                ByteRange::Partial { start: total.saturating_sub(suffix), end: total - 1 }
            }
        }
        _ => ByteRange::Whole,
    }
}

/// A resposta do carregador de `file:` para o arquivo (o que o `net.fetch` do original devolve).
async fn read_file_response(request: &GlobalRequest, path: &str) -> Result<GlobalResponse, Error> {
    let bytes = pfs::read_file(path).await?;
    let total = bytes.len();

    let mut headers = GlobalHeaders::new();
    if let Some(mime_type) = get_media_or_text_mime(path, &ProcessEnvironment) {
        headers.set("Content-Type", mime_type);
    }
    headers.set("Accept-Ranges", "bytes");

    let body_of = |bytes: Vec<u8>| if request.method() == "HEAD" { ResponseBody::Null } else { ResponseBody::Bytes(bytes) };
    match parse_range(request.headers().get("range"), total) {
        ByteRange::Unsatisfiable => {
            headers.set("Content-Range", &format!("bytes */{total}"));
            GlobalResponse::new(ResponseBody::Null, 416, Some(headers))
        }
        ByteRange::Partial { start, end } => {
            headers.set("Content-Range", &format!("bytes {start}-{end}/{total}"));
            headers.set("Content-Length", &(end - start + 1).to_string());
            GlobalResponse::new(body_of(bytes[start..=end].to_vec()), 206, Some(headers))
        }
        ByteRange::Whole => {
            headers.set("Content-Length", &total.to_string());
            GlobalResponse::new(body_of(bytes), 200, Some(headers))
        }
    }
}

/// O que os tratadores de `vscode-file:` e de `file:` leem: as raízes permitidas, as extensões permitidas e o ambiente.
struct ResourceLoader {
    environment_service: Rc<dyn NativeEnvironmentServiceApi>,
    log_service: LogServiceRef,
    valid_roots: Rc<RefCell<TernarySearchTree<String, bool>>>,
    valid_extensions: HashSet<&'static str>,
    file_access: FileAccess,
}

impl ResourceLoader {
    //#region file://

    fn handle_file_request(&self, request: &GlobalRequest) -> GlobalResponse {
        let uri = URI::parse(request.url(), false);

        self.log_service.error(&format!(
            "Refused to load resource {} from {}: protocol (original URL: {})",
            uri.fs_path(ProcessEnvironment.platform()).to_string_lossy(),
            schemas::FILE,
            request.url()
        ));

        GlobalResponse::error()
    }

    //#endregion

    //#region vscode-file://

    fn handle_resource_request(&self, request: GlobalRequest) -> Promise<GlobalResponse> {
        let path = self.request_to_normalized_file_path(&request);
        let path_basename = native_path().basename(&path, None);
        let is_workbench_html = path_basename == "workbench.html" || path_basename == "workbench-dev.html";

        let mut headers: Option<GlobalHeaders> = None;
        if self.environment_service.cross_origin_isolated() {
            let coi_headers = if is_workbench_html { Some(COOP_AND_COEP) } else { get_headers_from_query(request.url()) };
            headers = coi_headers.map(|coi_headers| GlobalHeaders::from_pairs(coi_headers.iter().copied()));
        }

        // In OSS, evict resources from the memory cache in the renderer process
        // Refs https://github.com/microsoft/vscode/issues/148541#issuecomment-2670891511
        if !self.environment_service.is_built() {
            headers = Some(merge_headers(headers, &CACHE_CONTROL_HEADERS));
        }

        // Document-policy header is needed for collecting
        // JavaScript callstacks via https://www.electronjs.org/docs/latest/api/web-frame-main#framecollectjavascriptcallstack-experimental
        // until https://github.com/electron/electron/issues/45356 is resolved.
        if is_workbench_html {
            headers = Some(merge_headers(headers, &DOCUMENT_POLICY_HEADERS));
        }

        // first check by validRoots
        if self.valid_roots.borrow().find_substr(&path).is_some_and(|valid| *valid) {
            return self.create_file_response(request, path, headers);
        }

        // then check by validExtensions
        if self.valid_extensions.contains(native_path().extname(&path).to_lowercase().as_str()) {
            return self.create_file_response(request, path, headers);
        }

        // finally block to load the resource
        self.log_service.error(&format!(
            "{}: Refused to load resource {} from {}: protocol (original URL: {})",
            schemas::VSCODE_FILE_RESOURCE,
            path,
            schemas::VSCODE_FILE_RESOURCE,
            request.url()
        ));

        Promise::resolved(GlobalResponse::error())
    }

    fn create_file_response(&self, request: GlobalRequest, path: String, headers: Option<GlobalHeaders>) -> Promise<GlobalResponse> {
        let log_service = self.log_service.clone();
        async_fn(async move {
            match read_file_response(&request, &path).await {
                Ok(mut response) => {
                    if let Some(headers) = headers {
                        for (name, value) in headers.iter() {
                            response.set_header(name, value);
                        }
                    }

                    Ok(response)
                }
                Err(error) => {
                    log_service.error_args(&LogMessage::Text(format!("{}: Failed to load resource {path}", schemas::VSCODE_FILE_RESOURCE)), &[LogArg::from(&error)]);

                    Ok(GlobalResponse::error())
                }
            }
        })
    }

    fn request_to_normalized_file_path(&self, request: &GlobalRequest) -> String {
        // 1.) Use `URI.parse()` util from us to convert the raw
        //     URL into our URI.
        let request_uri = URI::parse(request.url(), false);

        // 2.) Use `FileAccess.asFileUri` to convert back from a
        //     `vscode-file:` URI to a `file:` URI.
        let unnormalized_file_uri = self.file_access.uri_to_file_uri(&request_uri);

        // 3.) Strip anything from the URI that could result in
        //     relative paths (such as "..") by using `normalize`
        native_path().normalize(&unnormalized_file_uri.fs_path(ProcessEnvironment.platform()).to_string_lossy())
    }

    //#endregion
}

/// `createIPCObjectUrl<T>()`: o objeto publicado, o canal e o IPC que o serve.
struct IpcObjectUrl {
    resource: URI,
    channel: String,
    obj: Rc<RefCell<Dynamic>>,
    ipc_main: Rc<dyn ValidatedIpcMain>,
    log_service: LogServiceRef,
}

impl IIPCObjectUrl for IpcObjectUrl {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn update(&self, obj: Dynamic) {
        *self.obj.borrow_mut() = obj;
    }
}

impl IDisposable for IpcObjectUrl {
    fn dispose(&self) {
        self.log_service.trace(&format!("IPC Object URL: Removed channel {}.", self.channel));

        self.ipc_main.remove_handler(&self.channel);
    }
}

pub struct ProtocolMainService {
    base: Rc<Disposable>,
    log_service: LogServiceRef,
    session: Rc<dyn ProtocolSession>,
    ipc_main: Rc<dyn ValidatedIpcMain>,
    valid_roots: Rc<RefCell<TernarySearchTree<String, bool>>>,
}

impl ProtocolMainService {
    pub fn new(
        environment_service: Rc<dyn NativeEnvironmentServiceApi>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        log_service: LogServiceRef,
        session: Rc<dyn ProtocolSession>,
        ipc_main: Rc<dyn ValidatedIpcMain>,
    ) -> Rc<Self> {
        let valid_roots = Rc::new(RefCell::new(TernarySearchTree::for_paths(!Platform::detect().is_linux)));
        let file_access = FileAccess::new(Rc::new(ProcessEnvironment), FileAccessContext { is_native: true, ..FileAccessContext::default() }, RemoteAuthorities::new(false));
        let loader = Rc::new(ResourceLoader {
            environment_service: environment_service.clone(),
            log_service: log_service.clone(),
            valid_roots: valid_roots.clone(),
            valid_extensions: HashSet::from([".svg", ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".mp4", ".otf", ".ttf"]), // https://github.com/microsoft/vscode/issues/119384
            file_access,
        });
        let service = Rc::new(Self { base: Disposable::new(), log_service, session, ipc_main, valid_roots });

        // Define an initial set of roots we allow loading from
        // - appRoot	: all files installed as part of the app
        // - extensions : all files shipped from extensions
        // - storage    : all files in global and workspace storage (https://github.com/microsoft/vscode/issues/116735)
        let host = ProcessEnvironment.platform();
        let as_file_path = |uri: &URI| uri.with(&UriChange { scheme: Field::from(schemas::FILE), ..UriChange::default() }).fs_path(host).to_string_lossy();
        service.add_valid_file_root(&environment_service.app_root());
        service.add_valid_file_root(&environment_service.extensions_path());
        service.add_valid_file_root(&as_file_path(&user_data_profiles_service.default_profile().global_storage_home));
        service.add_valid_file_root(&as_file_path(&environment_service.workspace_storage_home()));

        // Handle protocols
        service.handle_protocols(loader);

        service
    }

    fn handle_protocols(&self, loader: Rc<ResourceLoader>) {
        // Register vscode-file:// handler
        let resource_loader = loader.clone();
        self.session.handle(schemas::VSCODE_FILE_RESOURCE, Rc::new(move |request: GlobalRequest| resource_loader.handle_resource_request(request)));

        // Block any file:// access
        self.session.handle(schemas::FILE, Rc::new(move |request: GlobalRequest| Promise::resolved(loader.handle_file_request(&request))));

        // Cleanup
        let session = self.session.clone();
        self.base.register(lifecycle::to_disposable(move || {
            session.unhandle(schemas::VSCODE_FILE_RESOURCE);
            session.unhandle(schemas::FILE);
        }));
    }
}

impl IProtocolMainService for ProtocolMainService {
    fn create_ipc_object_url(&self) -> Rc<dyn IIPCObjectUrl> {
        let obj = Rc::new(RefCell::new(Dynamic::Undefined));

        // Create unique URI
        let resource = URI::from(
            &UriComponents {
                scheme: Some("vscode".into()), // used for all our IPC communication (vscode:<channel>)
                path: Some(generate_uuid().into()),
                ..UriComponents::default()
            },
            false,
        );

        // Install IPC handler
        let channel = resource.to_string(false).to_string_lossy();
        let handler_obj = obj.clone();
        self.ipc_main.handle(&channel, Rc::new(move || Promise::resolved(handler_obj.borrow().clone())));

        self.log_service.trace(&format!("IPC Object URL: Registered new channel {channel}."));

        Rc::new(IpcObjectUrl { resource, channel, obj, ipc_main: self.ipc_main.clone(), log_service: self.log_service.clone() })
    }

    fn add_valid_file_root(&self, root: &str) -> DisposableRef {
        // Pass to `normalize` because we later also do the
        // same for all paths to check against.
        let normalized_root = native_path().normalize(root);

        if !self.valid_roots.borrow().get(&normalized_root).is_some_and(|valid| *valid) {
            self.valid_roots.borrow_mut().set(normalized_root.clone(), true);

            let valid_roots = self.valid_roots.clone();
            return lifecycle::to_disposable(move || valid_roots.borrow_mut().delete(&normalized_root));
        }

        lifecycle::none()
    }

    fn register_managed_remote_resource_protocol(&self, request_remote_resource: RequestRemoteResource) {
        self.session.handle(schemas::VSCODE_MANAGED_REMOTE_RESOURCE, create_managed_remote_resource_request_handler(request_remote_resource, self.log_service.clone()));

        let session = self.session.clone();
        self.base.register(lifecycle::to_disposable(move || session.unhandle(schemas::VSCODE_MANAGED_REMOTE_RESOURCE)));
    }
}

impl IDisposable for ProtocolMainService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
