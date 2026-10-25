//! Porte parcial de `src/vs/workbench/electron-browser/window.ts`: a resolução de URIs externas da `NativeWindow`
//! (`resolveExternalUri` e `openTunnel`), que o `setupOpenHandlers` registra no `IOpenerService`.
//!
//! O resto da `NativeWindow` (menus, barra de toque, ações de janela, fechamento, diálogos de autenticação...) depende de
//! serviços e de eventos de janela que ainda não existem no porte e entra com eles.
//!
//! Divergências:
//! - **`NativeWindowUriResolver`** é a fatia da `NativeWindow` que `resolveExternalUri` lê: os serviços dela vão por
//!   [`NativeWindowUriResolverServices`], e o `environmentService.remoteAuthority` é o campo `remote_authority` desses serviços.
//!   O `TestNativeWindow` do teste (que só anula `create`, `registerListeners` e `enableMultiWindowAwareTimeout`) não tem
//!   equivalente: a fatia não faz nada ao ser criada.
//! - **`ITunnelService`.** O contrato completo (`AbstractTunnelService`) depende de serviços ainda não portados; aqui é o
//!   recorte [`IExternalUriTunnelService`] com os dois membros que a janela chama (`getExistingTunnel`, `openTunnel`).
//!   `RemoteTunnel | string | undefined` é `Option<TunnelResult>` (a `string` é a mensagem de erro do túnel).
//!   `RemoteTunnel.dispose()` devolve uma promessa e é o campo [`RemoteTunnel::on_dispose`].
//! - **`IAddressProvider` e `IAddress`** (`remoteAgentConnection.ts`) são o trait [`IAddressProvider`] e o
//!   `ResolvedAuthority` do resolvedor de autoridade remota, que é o que o `getAddress` do original devolve.
//! - **`IFileService`.** O trait do porte não tem `canHandleResource`; o recorte [`IResourceHandlerFileService`] tem só ele.

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::lifecycle::to_disposable;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_platform::opener::opener::{OpenOptions, ResolvedExternalUri};
use je_platform::product::IProductService;
use je_platform::remote::remote_authority_resolver::{IRemoteAuthorityResolverService, ResolvedAuthority};
use je_platform::tunnel::{extract_local_host_uri_meta_data_for_port_mapping, extract_query_local_host_uri_meta_data_for_port_mapping};

/// `RemoteTunnel` (`tunnel.ts`).
#[derive(Clone)]
pub struct RemoteTunnel {
    pub tunnel_remote_host: String,
    pub tunnel_remote_port: u32,
    pub local_address: String,
    pub privacy: String,
    /// O `dispose()` do túnel.
    pub on_dispose: Rc<dyn Fn() -> Promise<()>>,
}

impl RemoteTunnel {
    /// `dispose()`.
    pub fn dispose(&self) -> Promise<()> {
        (self.on_dispose)()
    }
}

/// `RemoteTunnel | string`: o túnel, ou a mensagem de por que ele não abriu.
#[derive(Clone)]
pub enum TunnelResult {
    Tunnel(RemoteTunnel),
    Message(String),
}

/// `IAddressProvider` (`remoteAgentConnection.ts`).
pub trait IAddressProvider {
    /// `getAddress()`.
    fn get_address(&self) -> Promise<ResolvedAuthority>;
}

/// O recorte do `ITunnelService` que a janela usa.
pub trait IExternalUriTunnelService {
    /// `getExistingTunnel(remoteHost, remotePort)`.
    fn get_existing_tunnel(&self, remote_host: &str, remote_port: u32) -> Promise<Option<TunnelResult>>;

    /// `openTunnel(addressProvider, remoteHost, remotePort, ...)`: `None` é o `undefined` que o original pode devolver.
    fn open_tunnel(&self, address_provider: Option<Rc<dyn IAddressProvider>>, remote_host: Option<&str>, remote_port: u32) -> Option<Promise<Option<TunnelResult>>>;
}

/// O recorte do `IFileService` que a janela usa.
pub trait IResourceHandlerFileService {
    /// `canHandleResource(resource)`.
    fn can_handle_resource(&self, resource: &URI) -> Promise<bool>;
}

/// Os serviços de `NativeWindow.resolveExternalUri`.
#[derive(Clone)]
pub struct NativeWindowUriResolverServices {
    pub tunnel_service: Rc<dyn IExternalUriTunnelService>,
    pub file_service: Rc<dyn IResourceHandlerFileService>,
    pub product_service: Rc<dyn IProductService>,
    pub remote_authority_resolver_service: Rc<dyn IRemoteAuthorityResolverService>,
    /// `environmentService.remoteAuthority`.
    pub remote_authority: Option<String>,
}

/// O provedor de endereço que o `openTunnel` monta quando há autoridade remota.
struct RemoteAuthorityAddressProvider {
    remote_authority: String,
    resolver: Rc<dyn IRemoteAuthorityResolverService>,
}

impl IAddressProvider for RemoteAuthorityAddressProvider {
    fn get_address(&self) -> Promise<ResolvedAuthority> {
        self.resolver.resolve_authority(&self.remote_authority).then(|result| result.authority)
    }
}

/// A fatia da `NativeWindow` que resolve URIs externas.
pub struct NativeWindowUriResolver {
    services: NativeWindowUriResolverServices,
}

impl NativeWindowUriResolver {
    pub fn new(services: NativeWindowUriResolverServices) -> Rc<Self> {
        Rc::new(Self { services })
    }

    /// `resolveExternalUri(uri, options)`.
    pub fn resolve_external_uri(self: &Rc<Self>, uri: &URI, options: Option<&OpenOptions>) -> Promise<Option<ResolvedExternalUri>> {
        let this = Rc::clone(self);
        let uri = uri.clone();
        let allow_tunneling = options.and_then(|options| options.allow_tunneling).unwrap_or(false);
        let open_external = options.and_then(|options| options.open_external).unwrap_or(false);
        async_fn(async move {
            let mut query_tunnel: Option<TunnelResult> = None;
            if allow_tunneling {
                let port_mapping_request = extract_local_host_uri_meta_data_for_port_mapping(&uri);
                let query_port_mapping = extract_query_local_host_uri_meta_data_for_port_mapping(&uri);
                if let Some(query_port_mapping) = query_port_mapping {
                    query_tunnel = this.open_tunnel(query_port_mapping.address.clone(), query_port_mapping.port).await?;
                    if let Some(TunnelResult::Tunnel(tunnel)) = query_tunnel.clone() {
                        // If the tunnel was mapped to a different port, dispose it, because some services
                        // validate the port number in the query string.
                        if tunnel.tunnel_remote_port != query_port_mapping.port {
                            let _ = tunnel.dispose();
                            query_tunnel = None;
                        } else if port_mapping_request.is_none() {
                            return Ok(Some(ResolvedExternalUri {
                                resolved: uri,
                                disposable: Some(to_disposable(move || {
                                    let _ = tunnel.dispose();
                                })),
                            }));
                        }
                    }
                }

                if let Some(port_mapping_request) = port_mapping_request
                    && let Some(TunnelResult::Tunnel(tunnel)) = this.open_tunnel(port_mapping_request.address, port_mapping_request.port).await?
                {
                    let address_as_uri = URI::parse(tunnel.local_address.as_str(), false).with(&UriChange { path: Field::Set(uri.path().clone()), ..UriChange::default() });
                    let resolved = if address_as_uri.scheme().to_string_lossy().starts_with(&uri.scheme().to_string_lossy()) {
                        address_as_uri
                    } else {
                        uri.with(&UriChange { authority: Field::Set(JsString::from(tunnel.local_address.as_str())), ..UriChange::default() })
                    };
                    return Ok(Some(ResolvedExternalUri {
                        resolved,
                        disposable: Some(to_disposable(move || {
                            let _ = tunnel.dispose();
                            if let Some(TunnelResult::Tunnel(query_tunnel)) = &query_tunnel {
                                let _ = query_tunnel.dispose();
                            }
                        })),
                    }));
                }
            }

            if !open_external && this.services.file_service.can_handle_resource(&uri).await? {
                return Ok(Some(ResolvedExternalUri {
                    resolved: URI::from(
                        &UriComponents {
                            scheme: Some(JsString::from(this.services.product_service.url_protocol())),
                            path: Some(JsString::from("workspace")),
                            query: Some(uri.to_string(false)),
                            ..UriComponents::default()
                        },
                        false,
                    ),
                    disposable: None,
                }));
            }

            Ok(None)
        })
    }

    /// `openTunnel(address, port)`.
    fn open_tunnel(&self, address: String, port: u32) -> Promise<Option<TunnelResult>> {
        let address_provider: Option<Rc<dyn IAddressProvider>> = self.services.remote_authority.clone().map(|remote_authority| {
            Rc::new(RemoteAuthorityAddressProvider { remote_authority, resolver: self.services.remote_authority_resolver_service.clone() }) as Rc<dyn IAddressProvider>
        });
        let tunnel_service = self.services.tunnel_service.clone();
        async_fn(async move {
            let existing: Option<TunnelResult> = tunnel_service.get_existing_tunnel(&address, port).await?;
            if let Some(TunnelResult::Tunnel(tunnel)) = existing {
                return Ok(Some(TunnelResult::Tunnel(tunnel)));
            }
            match tunnel_service.open_tunnel(address_provider, Some(&address), port) {
                Some(opened) => opened.await,
                None => Ok(None),
            }
        })
    }
}
