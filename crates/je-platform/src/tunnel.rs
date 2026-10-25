//! Porte parcial de `src/vs/platform/tunnel/common/tunnel.ts`: o `ITunnelService.canTunnel`, que o gerenciador de links do
//! terminal lê para escolher o texto do hover ("Follow link using forwarded port"), e as funções puras do arquivo
//! (`extractLocalHostUriMetaDataForPortMapping`, `extractQueryLocalHostUriMetaDataForPortMapping`, `isLocalhost`,
//! `isAllInterfaces`, `isPortPrivileged`) com os enums `TunnelProtocol` e `TunnelPrivacyId`.
//!
//! O resto do serviço (`AbstractTunnelService`: `openTunnel`, `tunnels`, `setTunnelProvider`, os eventos de túnel...) depende do
//! `ILogService`, do `IConfigurationService` e do `IAddressProvider` do `remoteAgentConnection`, que ainda não existem no porte;
//! entra com o porte do encaminhamento de portas.
//!
//! Decisões de tradução:
//! - O `port` do `{ address, port }` é `u32`: o `+match[2]` do original aceita qualquer número de dígitos, e um `u16` perderia
//!   (ou rejeitaria) `localhost:99999`. Mais de dez dígitos (fora do `u32`) devolvem `None`.

use je_base::platform::OperatingSystem;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `ITunnelService`, reduzido a `canTunnel`.
pub trait ITunnelService {
    /// `canTunnel(uri)`: se o endereço pode ser encaminhado por um túnel.
    fn can_tunnel(&self, uri: &URI) -> bool;
}

service_id!(pub I_TUNNEL_SERVICE: dyn ITunnelService = "tunnelService");

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

// `src/vs/platform/tunnel/common/tunnelProxy.ts`.
pub mod tunnel_proxy_info;

// `src/vs/platform/tunnel/node`: o proxy de túnel, com E/S de rede (`std::net`), fora do `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
mod http_wire;
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
