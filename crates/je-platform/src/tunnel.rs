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
