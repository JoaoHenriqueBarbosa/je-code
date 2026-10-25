//! Porte de `src/vs/platform/tunnel/common/tunnel.ts`: o contrato do serviço de túneis (`ITunnelService`,
//! `ISharedTunnelsService`, `RemoteTunnel`, `ITunnelProvider`...), o `AbstractTunnelService` com a contagem de referências
//! dos túneis abertos, o `DisposableTunnel` e as funções puras do arquivo (`extractLocalHostUriMetaDataForPortMapping`,
//! `extractQueryLocalHostUriMetaDataForPortMapping`, `isLocalhost`, `isAllInterfaces`, `isPortPrivileged`) com os enums
//! `TunnelProtocol` e `TunnelPrivacyId`.
//!
//! Decisões de tradução:
//! - O `port` do `{ address, port }` é `u32`: o `+match[2]` do original aceita qualquer número de dígitos, e um `u16` perderia
//!   (ou rejeitaria) `localhost:99999`. Mais de dez dígitos (fora do `u32`) devolvem `None`.
//! - **`ITunnelService` ficou em dois traits.** [`ITunnelService`] segue sendo o recorte `canTunnel` que o gerenciador de links
//!   do terminal e o detector de links do depurador leem (e que os dublês dos testes implementam). [`ITunnelServiceApi`] é o
//!   resto do contrato (`openTunnel`, `tunnels`, `closeTunnel`...), com o primeiro como supertrait; o
//!   [`AbstractTunnelService`] implementa os dois. Só há um identificador de serviço, [`I_TUNNEL_SERVICE`], do recorte.
//! - `RemoteTunnel` (interface) é o trait [`RemoteTunnel`]; `RemoteTunnel | string | undefined` é [`TunnelOpenResult`] (a
//!   `string` é a mensagem de erro do provedor de túneis). `Promise<...> | undefined` é `Option<Promise<...>>`.
//!   `IAddressProvider | ITunnelProvider` é [`AddressOrTunnelProvider`] (o `isTunnelProvider` é a escolha da variante).
//! - **Classe abstrata.** `AbstractTunnelService` recebe os membros abstratos (`isPortPrivileged`,
//!   `retainOrCreateTunnel`) como [`TunnelServiceHooks`], que a subclasse (`BaseTunnelService`) implementa; os membros
//!   `protected` que a subclasse chama (`getTunnelFromMap`, `addTunnelToMap`, `createWithProvider`, `defaultTunnelHost`) são
//!   públicos. `Disposable` é composição: os emissores nascem registrados num `Disposable` que o `dispose` descarta.
//! - **`dispose` assíncrono.** `override async dispose()` vira [`AbstractTunnelService::dispose_async`] (a promessa de que o
//!   original devolve); [`IDisposable::dispose`] a dispara e a ignora.
//! - `_tunnels` (`Map` de `Map`) é um [`IndexMap`] de [`IndexMap`] (a ordem de inserção que `getTunnels` percorre) e cada
//!   entrada `{ refcount, value }` é um [`TunnelEntry`] compartilhado, porque o original muta o objeto que o mapa guarda.
//! - `isRemoteTunnel(something: unknown)` (teste estrutural de um objeto qualquer) não existe: o tipo já garante a forma.
//! - `ITunnel` é só o trait: o `DisposableTunnel` do original tem `localAddress` com a união `{ port, host } | string` e por
//!   isso não o satisfaz, nem aqui.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::configuration::service::IConfigurationService;
use crate::log::{LogServiceRef, Logger};
use crate::remote::remote_agent_connection::IAddressProvider;
use crate::remote::remote_authority_resolver::TunnelPrivacy;

/// `ITunnelService`, o recorte `canTunnel` (ver o topo).
pub trait ITunnelService {
    /// `canTunnel(uri)`: se o endereço pode ser encaminhado por um túnel.
    fn can_tunnel(&self, uri: &URI) -> bool;
}

service_id!(pub I_TUNNEL_SERVICE: dyn ITunnelService = "tunnelService");

/// `{ port: number; host: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostPort {
    pub port: u32,
    pub host: String,
}

/// `{ port: number; host: string } | string`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalAddress {
    HostPort(HostPort),
    Text(String),
}

/// `RemoteTunnel`.
pub trait RemoteTunnel {
    fn tunnel_remote_port(&self) -> u32;
    fn tunnel_remote_host(&self) -> String;
    fn tunnel_local_port(&self) -> Option<u32>;
    fn local_address(&self) -> String;
    fn privacy(&self) -> String;
    fn protocol(&self) -> Option<String>;
    /// `dispose(silent?)`.
    fn dispose(&self, silent: bool) -> Promise<()>;
}

/// `RemoteTunnel | string | undefined`: o túnel, a mensagem de erro do provedor, ou nada.
#[derive(Clone)]
pub enum TunnelOpenResult {
    Tunnel(Rc<dyn RemoteTunnel>),
    Message(String),
    Undefined,
}

/// `TunnelOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TunnelOptions {
    pub remote_address: Option<HostPort>,
    pub local_address_port: Option<u32>,
    pub label: Option<String>,
    pub public: Option<bool>,
    pub privacy: Option<String>,
    pub protocol: Option<String>,
}

/// `TunnelProtocol`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TunnelProtocol {
    Http,
    Https,
}

impl TunnelProtocol {
    /// O valor de texto do enum no original.
    pub fn as_str(self) -> &'static str {
        match self {
            TunnelProtocol::Http => "http",
            TunnelProtocol::Https => "https",
        }
    }
}

/// `TunnelPrivacyId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TunnelPrivacyId {
    /// Private, and changing is unsupported.
    ConstantPrivate,
    Private,
    Public,
}

impl TunnelPrivacyId {
    /// O valor de texto do enum no original.
    pub fn as_str(self) -> &'static str {
        match self {
            TunnelPrivacyId::ConstantPrivate => "constantPrivate",
            TunnelPrivacyId::Private => "private",
            TunnelPrivacyId::Public => "public",
        }
    }
}

/// `TunnelCreationOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TunnelCreationOptions {
    pub elevation_required: Option<bool>,
}

/// `TunnelProviderFeatures`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunnelProviderFeatures {
    pub elevation: bool,
    /// @deprecated
    pub public: Option<bool>,
    pub privacy_options: Vec<TunnelPrivacy>,
    pub protocol: bool,
}

/// `ITunnelProvider`.
pub trait ITunnelProvider {
    fn forward_port(&self, tunnel_options: &TunnelOptions, tunnel_creation_options: &TunnelCreationOptions) -> Option<Promise<TunnelOpenResult>>;
}

/// `IAddressProvider | ITunnelProvider`.
#[derive(Clone)]
pub enum AddressOrTunnelProvider {
    AddressProvider(Rc<dyn IAddressProvider>),
    TunnelProvider(Rc<dyn ITunnelProvider>),
}

/// `ProvidedOnAutoForward`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvidedOnAutoForward {
    Notify = 1,
    OpenBrowser = 2,
    OpenPreview = 3,
    Silent = 4,
    Ignore = 5,
    OpenBrowserOnce = 6,
}

/// `ProvidedPortAttributes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvidedPortAttributes {
    pub port: u32,
    pub auto_forward_action: ProvidedOnAutoForward,
}

/// `PortAttributesProvider`.
pub trait PortAttributesProvider {
    fn provide_port_attributes(&self, ports: &[u32], pid: Option<u32>, command_line: Option<&str>, token: &CancellationToken) -> Promise<Vec<ProvidedPortAttributes>>;
}

/// `ITunnel`.
pub trait ITunnel {
    fn remote_address(&self) -> HostPort;
    /// The complete local address (ex. localhost:1234).
    fn local_address(&self) -> String;
    /// @deprecated Use privacy instead.
    fn public(&self) -> Option<bool> {
        None
    }
    fn privacy(&self) -> Option<String> {
        None
    }
    fn protocol(&self) -> Option<String> {
        None
    }
    /// Implementers of Tunnel should fire onDidDispose when dispose is called.
    fn on_did_dispose(&self) -> Event<()>;
    fn dispose(&self) -> Promise<()>;
}

/// `ISharedTunnelsService`.
pub trait ISharedTunnelsService {
    #[allow(clippy::too_many_arguments)]
    fn open_tunnel(
        &self,
        authority: &str,
        address_provider: Option<Rc<dyn IAddressProvider>>,
        remote_host: Option<&str>,
        remote_port: u32,
        local_host: &str,
        local_port: Option<u32>,
        elevate_if_needed: Option<bool>,
        privacy: Option<&str>,
        protocol: Option<&str>,
    ) -> Option<Promise<TunnelOpenResult>>;
}

service_id!(pub I_SHARED_TUNNELS_SERVICE: dyn ISharedTunnelsService = "sharedTunnelsService");

/// O `{ host: string; port: number }` do `onTunnelClosed`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunnelClosedEvent {
    pub host: String,
    pub port: u32,
}

/// `ITunnelService`, o restante do contrato (ver o topo).
pub trait ITunnelServiceApi: ITunnelService {
    fn tunnels(&self) -> Promise<Vec<Rc<dyn RemoteTunnel>>>;
    fn can_change_privacy(&self) -> bool;
    fn privacy_options(&self) -> Vec<TunnelPrivacy>;
    fn on_tunnel_opened(&self) -> Event<Rc<dyn RemoteTunnel>>;
    fn on_tunnel_closed(&self) -> Event<TunnelClosedEvent>;
    fn can_elevate(&self) -> bool;
    fn can_change_protocol(&self) -> bool;
    fn has_tunnel_provider(&self) -> bool;
    fn on_added_tunnel_provider(&self) -> Event<()>;
    #[allow(clippy::too_many_arguments)]
    fn open_tunnel(
        &self,
        address_provider: Option<Rc<dyn IAddressProvider>>,
        remote_host: Option<&str>,
        remote_port: u32,
        local_host: Option<&str>,
        local_port: Option<u32>,
        elevate_if_needed: bool,
        privacy: Option<&str>,
        protocol: Option<&str>,
    ) -> Option<Promise<TunnelOpenResult>>;
    fn get_existing_tunnel(&self, remote_host: &str, remote_port: u32) -> Promise<TunnelOpenResult>;
    fn set_environment_tunnel(&self, remote_host: &str, remote_port: u32, local_address: &str, privacy: &str, protocol: &str);
    fn close_tunnel(&self, remote_host: &str, remote_port: u32) -> Promise<()>;
    fn set_tunnel_provider(&self, provider: Option<Rc<dyn ITunnelProvider>>) -> DisposableRef;
    fn set_tunnel_features(&self, features: &TunnelProviderFeatures);
    fn is_port_privileged(&self, port: u32) -> bool;
}

/// O `{ address, port }` que `extractLocalHostUriMetaDataForPortMapping` devolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalHostUriMetaData {
    pub address: String,
    pub port: u32,
}

/// Os endereços que `extractLocalHostUriMetaDataForPortMapping` reconhece (a alternância `localhost|127\.0\.0\.1|0\.0\.0\.0`).
const PORT_MAPPING_ADDRESSES: [&str; 3] = ["localhost", "127.0.0.1", "0.0.0.0"];

/// `extractLocalHostUriMetaDataForPortMapping`: o endereço e a porta de um `http(s)://localhost:porta` (também
/// `127.0.0.1` e `0.0.0.0`), ou `None`.
pub fn extract_local_host_uri_meta_data_for_port_mapping(uri: &URI) -> Option<LocalHostUriMetaData> {
    let scheme = uri.scheme().to_string_lossy();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    // /^(localhost|127\.0\.0\.1|0\.0\.0\.0):(\d+)$/ sobre a autoridade (nenhum dos endereços tem `:`).
    let authority = uri.authority().to_string_lossy();
    let (address, port) = authority.split_once(':')?;
    if !PORT_MAPPING_ADDRESSES.contains(&address) || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(LocalHostUriMetaData { address: address.to_owned(), port: port.parse().ok()? })
}

/// `extractQueryLocalHostUriMetaDataForPortMapping`: o primeiro valor de parâmetro da query que é um endereço
/// `http(s)` local com porta.
pub fn extract_query_local_host_uri_meta_data_for_port_mapping(uri: &URI) -> Option<LocalHostUriMetaData> {
    let scheme = uri.scheme().to_string_lossy();
    if (scheme != "http" && scheme != "https") || uri.query().is_empty() {
        return None;
    }
    let query = uri.query().to_string_lossy();
    for keyvalue in query.split('&') {
        // `keyvalue.split('=')[1]`: sem `=`, o valor é `undefined`, que nunca casa com `/^https?:/`.
        let Some(value) = keyvalue.split('=').nth(1) else { continue };
        if value.starts_with("http:") || value.starts_with("https:") {
            if let Some(result) = extract_local_host_uri_meta_data_for_port_mapping(&URI::parse(value, false)) {
                return Some(result);
            }
        }
    }
    None
}

/// `LOCALHOST_ADDRESSES`.
pub const LOCALHOST_ADDRESSES: [&str; 4] = ["localhost", "127.0.0.1", "0:0:0:0:0:0:0:1", "::1"];

/// `isLocalhost`.
pub fn is_localhost(host: &str) -> bool {
    LOCALHOST_ADDRESSES.contains(&host)
}

/// `ALL_INTERFACES_ADDRESSES`.
pub const ALL_INTERFACES_ADDRESSES: [&str; 3] = ["0.0.0.0", "0:0:0:0:0:0:0:0", "::"];

/// `isAllInterfaces`.
pub fn is_all_interfaces(host: &str) -> bool {
    ALL_INTERFACES_ADDRESSES.contains(&host)
}

/// O primeiro `(\d+)\.(\d+)\.(\d+)` de `os_release` e o seu primeiro número (o `parseInt(osVersion[1])`).
fn first_dotted_version_major(os_release: &str) -> Option<u64> {
    let bytes = os_release.as_bytes();
    let digits_end = |from: usize| from + bytes[from..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    (0..bytes.len()).find_map(|start| {
        let first_end = digits_end(start);
        if first_end == start || bytes.get(first_end) != Some(&b'.') {
            return None;
        }
        let second_start = first_end + 1;
        let second_end = digits_end(second_start);
        if second_end == second_start || bytes.get(second_end) != Some(&b'.') {
            return None;
        }
        let third_start = second_end + 1;
        if digits_end(third_start) == third_start {
            return None;
        }
        Some(os_release[start..first_end].parse::<u64>().unwrap_or(u64::MAX))
    })
}

/// `isPortPrivileged`.
pub fn is_port_privileged(port: u32, host: &str, os: OperatingSystem, os_release: &str) -> bool {
    match os {
        OperatingSystem::Windows => false,
        // Since macOS Mojave (darwin version 18.0.0) all interfaces can bind low ports.
        OperatingSystem::Macintosh if is_all_interfaces(host) && first_dotted_version_major(os_release).is_some_and(|major| major >= 18) => false,
        _ => port < 1024,
    }
}

/// `DisposableTunnel`.
pub struct DisposableTunnel {
    on_dispose: Rc<Emitter<()>>,
    pub remote_address: HostPort,
    pub local_address: LocalAddress,
    dispose_fn: Rc<dyn Fn() -> Promise<()>>,
}

impl DisposableTunnel {
    pub fn new(remote_address: HostPort, local_address: LocalAddress, dispose: impl Fn() -> Promise<()> + 'static) -> Self {
        Self { on_dispose: Emitter::new(), remote_address, local_address, dispose_fn: Rc::new(dispose) }
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }

    pub fn dispose(&self) -> Promise<()> {
        self.on_dispose.fire(());
        self.on_dispose.dispose();
        (self.dispose_fn)()
    }
}

/// `{ refcount: number; readonly value: Promise<RemoteTunnel | string | undefined> }`, a entrada de `_tunnels`.
pub struct TunnelEntry {
    pub refcount: Cell<i64>,
    pub value: Promise<TunnelOpenResult>,
}

type PortMap = Rc<RefCell<IndexMap<u32, Rc<TunnelEntry>>>>;

/// Os membros abstratos de `AbstractTunnelService` (ver o topo).
pub trait TunnelServiceHooks {
    /// `isPortPrivileged(port)`.
    fn is_port_privileged(&self, service: &AbstractTunnelService, port: u32) -> bool;

    /// `retainOrCreateTunnel(...)`.
    #[allow(clippy::too_many_arguments)]
    fn retain_or_create_tunnel(
        &self,
        service: &AbstractTunnelService,
        address_or_tunnel_provider: AddressOrTunnelProvider,
        remote_host: &str,
        remote_port: u32,
        local_host: &str,
        local_port: Option<u32>,
        elevate_if_needed: bool,
        privacy: Option<&str>,
        protocol: Option<&str>,
    ) -> Option<Promise<TunnelOpenResult>>;
}

/// O `RemoteTunnel` de `setEnvironmentTunnel`: já existe, e descartá-lo não faz nada.
struct EnvironmentTunnel {
    remote_host: String,
    remote_port: u32,
    local_address: String,
    privacy: String,
    protocol: String,
}

impl RemoteTunnel for EnvironmentTunnel {
    fn tunnel_remote_port(&self) -> u32 {
        self.remote_port
    }
    fn tunnel_remote_host(&self) -> String {
        self.remote_host.clone()
    }
    fn tunnel_local_port(&self) -> Option<u32> {
        None
    }
    fn local_address(&self) -> String {
        self.local_address.clone()
    }
    fn privacy(&self) -> String {
        self.privacy.clone()
    }
    fn protocol(&self) -> Option<String> {
        Some(self.protocol.clone())
    }
    fn dispose(&self, _silent: bool) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O objeto que `makeTunnel` devolve: o mesmo túnel, com o `dispose` que solta uma referência.
struct MadeTunnel {
    service: Rc<AbstractTunnelService>,
    tunnel_remote_port: u32,
    tunnel_remote_host: String,
    tunnel_local_port: Option<u32>,
    local_address: String,
    privacy: String,
    protocol: Option<String>,
}

impl RemoteTunnel for MadeTunnel {
    fn tunnel_remote_port(&self) -> u32 {
        self.tunnel_remote_port
    }
    fn tunnel_remote_host(&self) -> String {
        self.tunnel_remote_host.clone()
    }
    fn tunnel_local_port(&self) -> Option<u32> {
        self.tunnel_local_port
    }
    fn local_address(&self) -> String {
        self.local_address.clone()
    }
    fn privacy(&self) -> String {
        self.privacy.clone()
    }
    fn protocol(&self) -> Option<String> {
        self.protocol.clone()
    }
    fn dispose(&self, _silent: bool) -> Promise<()> {
        let service = self.service.clone();
        let (remote_host, remote_port) = (self.tunnel_remote_host.clone(), self.tunnel_remote_port);
        async_fn(async move {
            service.log_service.trace(&format!("ForwardedPorts: (TunnelService) dispose request for {remote_host}:{remote_port} "));
            let existing_host = service.tunnels.borrow().get(&remote_host).cloned();
            if let Some(existing_host) = existing_host {
                let existing = existing_host.borrow().get(&remote_port).cloned();
                if let Some(existing) = existing {
                    existing.refcount.set(existing.refcount.get() - 1);
                    service.try_dispose_tunnel(&remote_host, remote_port, existing).await?;
                }
            }
            Ok(())
        })
    }
}

/// `AbstractTunnelService`.
pub struct AbstractTunnelService {
    me: Weak<AbstractTunnelService>,
    base: Rc<Disposable>,
    log_service: LogServiceRef,
    configuration_service: Rc<dyn IConfigurationService>,
    hooks: Rc<dyn TunnelServiceHooks>,
    on_tunnel_opened: Rc<Emitter<Rc<dyn RemoteTunnel>>>,
    on_tunnel_closed: Rc<Emitter<TunnelClosedEvent>>,
    on_added_tunnel_provider: Rc<Emitter<()>>,
    tunnels: RefCell<IndexMap<String, PortMap>>,
    tunnel_provider: RefCell<Option<Rc<dyn ITunnelProvider>>>,
    can_elevate: Cell<bool>,
    can_change_protocol: Cell<bool>,
    privacy_options: RefCell<Vec<TunnelPrivacy>>,
    factory_in_progress: RefCell<HashSet<u32>>,
}

impl AbstractTunnelService {
    /// O construtor da classe abstrata, com os membros abstratos em `hooks`.
    pub fn with_hooks(log_service: LogServiceRef, configuration_service: Rc<dyn IConfigurationService>, hooks: Rc<dyn TunnelServiceHooks>) -> Rc<Self> {
        let base = Disposable::new();
        let on_tunnel_opened: Rc<Emitter<Rc<dyn RemoteTunnel>>> = Emitter::new();
        let on_tunnel_closed: Rc<Emitter<TunnelClosedEvent>> = Emitter::new();
        let on_added_tunnel_provider: Rc<Emitter<()>> = Emitter::new();
        base.register(on_tunnel_opened.clone());
        base.register(on_tunnel_closed.clone());
        base.register(on_added_tunnel_provider.clone());
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            log_service,
            configuration_service,
            hooks,
            on_tunnel_opened,
            on_tunnel_closed,
            on_added_tunnel_provider,
            tunnels: RefCell::new(IndexMap::new()),
            tunnel_provider: RefCell::new(None),
            can_elevate: Cell::new(false),
            can_change_protocol: Cell::new(true),
            privacy_options: RefCell::new(Vec::new()),
            factory_in_progress: RefCell::new(HashSet::new()),
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço de túneis está vivo enquanto um método dele roda")
    }

    /// `logService` (`protected`).
    pub fn log_service(&self) -> &LogServiceRef {
        &self.log_service
    }

    /// `defaultTunnelHost` (`protected`).
    pub fn default_tunnel_host(&self) -> String {
        let setting_value = self.configuration_service.get_value(Some("remote.localPortHost"), None);
        if !setting_value.is_truthy() || setting_value.strict_equals(&Dynamic::string("localhost")) { "127.0.0.1".to_owned() } else { "0.0.0.0".to_owned() }
    }

    /// `override async dispose()`.
    pub fn dispose_async(&self) -> Promise<()> {
        self.base.dispose();
        let this = self.rc();
        async_fn(async move {
            let port_maps: Vec<PortMap> = this.tunnels.borrow().values().cloned().collect();
            for port_map in port_maps {
                let entries: Vec<Rc<TunnelEntry>> = port_map.borrow().values().cloned().collect();
                for entry in entries {
                    entry
                        .value
                        .then_promise(|tunnel| match tunnel {
                            TunnelOpenResult::Tunnel(tunnel) => tunnel.dispose(false),
                            _ => Promise::resolved(()),
                        })
                        .await?;
                }
                port_map.borrow_mut().clear();
            }
            this.tunnels.borrow_mut().clear();
            Ok(())
        })
    }

    fn make_tunnel(&self, tunnel: &Rc<dyn RemoteTunnel>) -> Rc<dyn RemoteTunnel> {
        Rc::new(MadeTunnel {
            service: self.rc(),
            tunnel_remote_port: tunnel.tunnel_remote_port(),
            tunnel_remote_host: tunnel.tunnel_remote_host(),
            tunnel_local_port: tunnel.tunnel_local_port(),
            local_address: tunnel.local_address(),
            privacy: tunnel.privacy(),
            protocol: tunnel.protocol(),
        })
    }

    fn try_dispose_tunnel(&self, remote_host: &str, remote_port: u32, tunnel: Rc<TunnelEntry>) -> Promise<()> {
        let this = self.rc();
        let remote_host = remote_host.to_owned();
        async_fn(async move {
            if tunnel.refcount.get() <= 0 {
                this.log_service.trace(&format!("ForwardedPorts: (TunnelService) Tunnel is being disposed {remote_host}:{remote_port}."));
                let closing = this.clone();
                let dispose_promise: Promise<()> = tunnel.value.then_promise(move |value| match value {
                    TunnelOpenResult::Tunnel(tunnel) => {
                        let closed = TunnelClosedEvent { host: tunnel.tunnel_remote_host(), port: tunnel.tunnel_remote_port() };
                        tunnel.dispose(true).then(move |()| closing.on_tunnel_closed.fire(closed))
                    }
                    _ => Promise::resolved(()),
                });
                let port_map = this.tunnels.borrow().get(&remote_host).cloned();
                if let Some(port_map) = port_map {
                    port_map.borrow_mut().shift_remove(&remote_port);
                }
                return dispose_promise.await;
            }
            Ok(())
        })
    }

    /// `addTunnelToMap` (`protected`).
    pub fn add_tunnel_to_map(&self, remote_host: &str, remote_port: u32, tunnel: Promise<TunnelOpenResult>) {
        let port_map = self.tunnels.borrow_mut().entry(remote_host.to_owned()).or_default().clone();
        port_map.borrow_mut().insert(remote_port, Rc::new(TunnelEntry { refcount: Cell::new(1), value: tunnel }));
    }

    fn remove_empty_or_error_tunnel_from_map(&self, remote_host: &str, remote_port: u32) -> Promise<()> {
        let this = self.rc();
        let remote_host = remote_host.to_owned();
        async_fn(async move {
            let host_map = this.tunnels.borrow().get(&remote_host).cloned();
            if let Some(host_map) = host_map {
                let tunnel = host_map.borrow().get(&remote_port).cloned();
                let tunnel_result = match tunnel {
                    Some(tunnel) => Some(tunnel.value.clone().await?),
                    None => None,
                };
                if matches!(tunnel_result, None | Some(TunnelOpenResult::Undefined) | Some(TunnelOpenResult::Message(_))) {
                    host_map.borrow_mut().shift_remove(&remote_port);
                }
                let is_empty = host_map.borrow().is_empty();
                if is_empty {
                    this.tunnels.borrow_mut().shift_remove(&remote_host);
                }
            }
            Ok(())
        })
    }

    /// `getTunnelFromMap` (`protected`).
    pub fn get_tunnel_from_map(&self, remote_host: &str, remote_port: u32) -> Option<Rc<TunnelEntry>> {
        let mut hosts: Vec<&str> = vec![remote_host];
        // Order matters. We want the original host to be first.
        if is_localhost(remote_host) {
            hosts.extend(LOCALHOST_ADDRESSES);
            // For localhost, we add the all interfaces hosts because if the tunnel is already available at all interfaces,
            // then of course it is available at localhost.
            hosts.extend(ALL_INTERFACES_ADDRESSES);
        } else if is_all_interfaces(remote_host) {
            hosts.extend(ALL_INTERFACES_ADDRESSES);
        }

        let tunnels = self.tunnels.borrow();
        let existing_port_maps: Vec<Option<PortMap>> = hosts.iter().map(|host| tunnels.get(*host).cloned()).collect();
        existing_port_maps.into_iter().flatten().find_map(|port_map| port_map.borrow().get(&remote_port).cloned())
    }

    /// `createWithProvider` (`protected`).
    pub fn create_with_provider(
        &self,
        tunnel_provider: &Rc<dyn ITunnelProvider>,
        remote_host: &str,
        remote_port: u32,
        local_port: Option<u32>,
        elevate_if_needed: bool,
        privacy: Option<&str>,
        protocol: Option<&str>,
    ) -> Option<Promise<TunnelOpenResult>> {
        self.log_service.trace(&format!(
            "ForwardedPorts: (TunnelService) Creating tunnel with provider {remote_host}:{remote_port} on local port {}.",
            local_port.map_or_else(|| "undefined".to_owned(), |port| port.to_string())
        ));
        let key = remote_port;
        self.factory_in_progress.borrow_mut().insert(key);
        let preferred_local_port = local_port.unwrap_or(remote_port);
        let creation_info = TunnelCreationOptions { elevation_required: Some(if elevate_if_needed { self.is_port_privileged(preferred_local_port) } else { false }) };
        let privacy = privacy.filter(|privacy| !privacy.is_empty());
        let tunnel_options = TunnelOptions {
            remote_address: Some(HostPort { host: remote_host.to_owned(), port: remote_port }),
            local_address_port: local_port,
            label: None,
            public: privacy.map(|privacy| privacy != TunnelPrivacyId::Private.as_str()),
            privacy: privacy.map(str::to_owned),
            protocol: protocol.map(str::to_owned),
        };
        match tunnel_provider.forward_port(&tunnel_options, &creation_info) {
            Some(tunnel) => {
                self.add_tunnel_to_map(remote_host, remote_port, tunnel.clone());
                let this = self.rc();
                tunnel.finally(move || {
                    this.log_service.trace("ForwardedPorts: (TunnelService) Tunnel created by provider.");
                    this.factory_in_progress.borrow_mut().remove(&key);
                });
                Some(tunnel)
            }
            None => {
                self.factory_in_progress.borrow_mut().remove(&key);
                None
            }
        }
    }
}

impl IDisposable for AbstractTunnelService {
    fn dispose(&self) {
        let _ = self.dispose_async();
    }
}

impl ITunnelService for AbstractTunnelService {
    fn can_tunnel(&self, uri: &URI) -> bool {
        extract_local_host_uri_meta_data_for_port_mapping(uri).is_some()
    }
}

impl ITunnelServiceApi for AbstractTunnelService {
    fn tunnels(&self) -> Promise<Vec<Rc<dyn RemoteTunnel>>> {
        let this = self.rc();
        async_fn(async move {
            let mut tunnels: Vec<Rc<dyn RemoteTunnel>> = Vec::new();
            let port_maps: Vec<PortMap> = this.tunnels.borrow().values().cloned().collect();
            for port_map in port_maps {
                let entries: Vec<Rc<TunnelEntry>> = port_map.borrow().values().cloned().collect();
                for entry in entries {
                    if let TunnelOpenResult::Tunnel(tunnel) = entry.value.clone().await? {
                        tunnels.push(tunnel);
                    }
                }
            }
            Ok(tunnels)
        })
    }

    fn can_change_privacy(&self) -> bool {
        !self.privacy_options.borrow().is_empty()
    }

    fn privacy_options(&self) -> Vec<TunnelPrivacy> {
        self.privacy_options.borrow().clone()
    }

    fn on_tunnel_opened(&self) -> Event<Rc<dyn RemoteTunnel>> {
        self.on_tunnel_opened.event()
    }

    fn on_tunnel_closed(&self) -> Event<TunnelClosedEvent> {
        self.on_tunnel_closed.event()
    }

    fn can_elevate(&self) -> bool {
        self.can_elevate.get()
    }

    fn can_change_protocol(&self) -> bool {
        self.can_change_protocol.get()
    }

    fn has_tunnel_provider(&self) -> bool {
        self.tunnel_provider.borrow().is_some()
    }

    fn on_added_tunnel_provider(&self) -> Event<()> {
        self.on_added_tunnel_provider.event()
    }

    fn open_tunnel(
        &self,
        address_provider: Option<Rc<dyn IAddressProvider>>,
        remote_host: Option<&str>,
        remote_port: u32,
        local_host: Option<&str>,
        local_port: Option<u32>,
        elevate_if_needed: bool,
        privacy: Option<&str>,
        protocol: Option<&str>,
    ) -> Option<Promise<TunnelOpenResult>> {
        self.log_service.trace(&format!(
            "ForwardedPorts: (TunnelService) openTunnel request for {}:{remote_port} on local port {}.",
            remote_host.unwrap_or("undefined"),
            local_port.map_or_else(|| "undefined".to_owned(), |port| port.to_string())
        ));
        let tunnel_provider = self.tunnel_provider.borrow().clone();
        let address_or_tunnel_provider = match (tunnel_provider, address_provider) {
            (Some(provider), _) => AddressOrTunnelProvider::TunnelProvider(provider),
            (None, Some(address_provider)) => AddressOrTunnelProvider::AddressProvider(address_provider),
            (None, None) => return None,
        };

        let remote_host = remote_host.filter(|host| !host.is_empty()).unwrap_or("localhost").to_owned();
        let local_host = local_host.filter(|host| !host.is_empty()).map_or_else(|| self.default_tunnel_host(), str::to_owned);

        // Prevent tunnel factories from calling openTunnel from within the factory
        if self.tunnel_provider.borrow().is_some() && self.factory_in_progress.borrow().contains(&remote_port) {
            self.log_service.debug("ForwardedPorts: (TunnelService) Another call to create a tunnel with the same address has occurred before the last one completed. This call will be ignored.");
            return None;
        }

        let resolved_tunnel = self.hooks.retain_or_create_tunnel(self, address_or_tunnel_provider, &remote_host, remote_port, &local_host, local_port, elevate_if_needed, privacy, protocol);
        let Some(resolved_tunnel) = resolved_tunnel else {
            self.log_service.trace("ForwardedPorts: (TunnelService) Tunnel was not created.");
            return None;
        };

        let this = self.rc();
        let privacy = privacy.filter(|privacy| !privacy.is_empty()).map(str::to_owned);
        Some(resolved_tunnel.then(move |tunnel| match tunnel {
            TunnelOpenResult::Undefined => {
                this.log_service.trace("ForwardedPorts: (TunnelService) New tunnel is undefined.");
                let _ = this.remove_empty_or_error_tunnel_from_map(&remote_host, remote_port);
                TunnelOpenResult::Undefined
            }
            TunnelOpenResult::Message(message) => {
                this.log_service.trace("ForwardedPorts: (TunnelService) The tunnel provider returned an error when creating the tunnel.");
                let _ = this.remove_empty_or_error_tunnel_from_map(&remote_host, remote_port);
                TunnelOpenResult::Message(message)
            }
            TunnelOpenResult::Tunnel(tunnel) => {
                this.log_service.trace("ForwardedPorts: (TunnelService) New tunnel established.");
                let new_tunnel = this.make_tunnel(&tunnel);
                if tunnel.tunnel_remote_host() != remote_host || tunnel.tunnel_remote_port() != remote_port {
                    this.log_service.warn("ForwardedPorts: (TunnelService) Created tunnel does not match requirements of requested tunnel. Host or port mismatch.");
                }
                if privacy.as_deref().is_some_and(|privacy| tunnel.privacy() != privacy) {
                    this.log_service.warn("ForwardedPorts: (TunnelService) Created tunnel does not match requirements of requested tunnel. Privacy mismatch.");
                }
                this.on_tunnel_opened.fire(new_tunnel.clone());
                TunnelOpenResult::Tunnel(new_tunnel)
            }
        }))
    }

    fn get_existing_tunnel(&self, remote_host: &str, remote_port: u32) -> Promise<TunnelOpenResult> {
        let this = self.rc();
        let mut remote_host = remote_host.to_owned();
        async_fn(async move {
            if is_all_interfaces(&remote_host) || is_localhost(&remote_host) {
                remote_host = LOCALHOST_ADDRESSES[0].to_owned();
            }

            let existing = this.get_tunnel_from_map(&remote_host, remote_port);
            if let Some(existing) = existing {
                existing.refcount.set(existing.refcount.get() + 1);
                return existing.value.clone().await;
            }
            Ok(TunnelOpenResult::Undefined)
        })
    }

    fn set_environment_tunnel(&self, remote_host: &str, remote_port: u32, local_address: &str, privacy: &str, protocol: &str) {
        let tunnel: Rc<dyn RemoteTunnel> =
            Rc::new(EnvironmentTunnel { remote_host: remote_host.to_owned(), remote_port, local_address: local_address.to_owned(), privacy: privacy.to_owned(), protocol: protocol.to_owned() });
        self.add_tunnel_to_map(remote_host, remote_port, Promise::resolved(TunnelOpenResult::Tunnel(tunnel)));
    }

    fn close_tunnel(&self, remote_host: &str, remote_port: u32) -> Promise<()> {
        let this = self.rc();
        let remote_host = remote_host.to_owned();
        async_fn(async move {
            this.log_service.trace(&format!("ForwardedPorts: (TunnelService) close request for {remote_host}:{remote_port} "));
            let port_map = this.tunnels.borrow().get(&remote_host).cloned();
            if let Some(port_map) = port_map {
                let value = port_map.borrow().get(&remote_port).cloned();
                if let Some(value) = value {
                    value.refcount.set(0);
                    this.try_dispose_tunnel(&remote_host, remote_port, value).await?;
                }
            }
            Ok(())
        })
    }

    fn set_tunnel_provider(&self, provider: Option<Rc<dyn ITunnelProvider>>) -> DisposableRef {
        *self.tunnel_provider.borrow_mut() = provider.clone();
        if provider.is_none() {
            // clear features
            self.can_elevate.set(false);
            self.privacy_options.borrow_mut().clear();
            self.on_added_tunnel_provider.fire(());
            return to_disposable(|| {});
        }

        self.on_added_tunnel_provider.fire(());
        let this = self.rc();
        to_disposable(move || {
            *this.tunnel_provider.borrow_mut() = None;
            this.can_elevate.set(false);
            this.privacy_options.borrow_mut().clear();
        })
    }

    fn set_tunnel_features(&self, features: &TunnelProviderFeatures) {
        self.can_elevate.set(features.elevation);
        *self.privacy_options.borrow_mut() = features.privacy_options.clone();
        self.can_change_protocol.set(features.protocol);
    }

    fn is_port_privileged(&self, port: u32) -> bool {
        self.hooks.is_port_privileged(self, port)
    }
}

// `src/vs/platform/tunnel/common/tunnelProxy.ts`.
pub mod tunnel_proxy_info;

// `src/vs/platform/tunnel/node`: o proxy de túnel, com E/S de rede (`std::net`), fora do `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
mod http_wire;
#[cfg(not(target_arch = "wasm32"))]
mod loop_bridge;
#[cfg(not(target_arch = "wasm32"))]
pub mod self_signed_cert;
#[cfg(not(target_arch = "wasm32"))]
mod tls_stream;
#[cfg(not(target_arch = "wasm32"))]
pub mod tunnel_proxy;
#[cfg(not(target_arch = "wasm32"))]
pub mod tunnel_service;
#[cfg(not(target_arch = "wasm32"))]
pub mod tunnel_socket;
