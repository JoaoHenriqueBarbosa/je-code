//! Porte parcial de `src/vs/platform/tunnel/common/tunnel.ts`: só o `ITunnelService.canTunnel`, que o gerenciador de links do
//! terminal lê para escolher o texto do hover ("Follow link using forwarded port").
//!
//! O resto do serviço (`openTunnel`, `tunnels`, `setTunnelProvider`, os eventos de túnel...) entra com o porte do
//! encaminhamento de portas.

use je_base::uri::URI;
use je_instantiation::service_id;

/// `ITunnelService`, reduzido a `canTunnel`.
pub trait ITunnelService {
    /// `canTunnel(uri)`: se o endereço pode ser encaminhado por um túnel.
    fn can_tunnel(&self, uri: &URI) -> bool;
}

service_id!(pub I_TUNNEL_SERVICE: dyn ITunnelService = "tunnelService");

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
pub mod tunnel_socket;
