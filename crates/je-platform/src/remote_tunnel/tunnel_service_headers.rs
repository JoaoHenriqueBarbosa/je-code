//! Porte de `src/vs/platform/remoteTunnel/common/tunnelServiceHeaders.ts`: a correlação entre uma operação do
//! serviço de túnel e as requisições de gerenciamento e de relay que a compõem.
//!
//! Divergências:
//! - `publicLog2<E, T>` (a classificação GDPR é só tipo do TypeScript) é `public_log2` com o dado como
//!   [`Dynamic`], na ordem das chaves do original.
//! - O `Record<string, string>` de `tunnelServiceHeaders` tem um acessor (`get 'X-Tunnels-VSCode-Client-Request-Id'`)
//!   que gera um UUID novo a cada leitura; aqui é o [`TunnelServiceHeaders`], cujo [`TunnelServiceHeaders::to_vec`]
//!   gera um UUID novo a cada chamada, e o nome de cada cabeçalho é uma constante.

use je_base::dynamic::Dynamic;
use je_base::uuid::generate_uuid;

use crate::telemetry::telemetry::ITelemetryService;

/// `ITunnelServiceCorrelation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITunnelServiceCorrelation {
    pub session_id: String,
    pub operation_id: String,
}

/// `TunnelServiceOperation`: `'list' | 'connect' | 'delete' | 'host'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelServiceOperation {
    List,
    Connect,
    Delete,
    Host,
}

impl TunnelServiceOperation {
    /// O valor de texto da operação.
    pub fn as_str(self) -> &'static str {
        match self {
            TunnelServiceOperation::List => "list",
            TunnelServiceOperation::Connect => "connect",
            TunnelServiceOperation::Delete => "delete",
            TunnelServiceOperation::Host => "host",
        }
    }
}

/// `createTunnelServiceCorrelation`: a sessão de telemetria e um id de operação novo; só registra o evento
/// `tunnelServiceOperation` quando há operação.
pub fn create_tunnel_service_correlation(telemetry_service: &dyn ITelemetryService, operation: Option<TunnelServiceOperation>) -> ITunnelServiceCorrelation {
    let correlation = ITunnelServiceCorrelation { session_id: telemetry_service.session_id(), operation_id: generate_uuid() };
    if let Some(operation) = operation {
        telemetry_service.public_log2(
            "tunnelServiceOperation",
            Some(Dynamic::object([
                ("tunnelSessionId", Dynamic::string(&correlation.session_id)),
                ("operationId", Dynamic::string(&correlation.operation_id)),
                ("operation", Dynamic::string(operation.as_str())),
            ])),
        );
    }
    correlation
}

/// `X-Tunnels-VSCode-Session-Id`.
pub const HEADER_SESSION_ID: &str = "X-Tunnels-VSCode-Session-Id";
/// `X-Tunnels-VSCode-Client-Operation-Id`.
pub const HEADER_CLIENT_OPERATION_ID: &str = "X-Tunnels-VSCode-Client-Operation-Id";
/// `X-Tunnels-VSCode-Client-Request-Id`.
pub const HEADER_CLIENT_REQUEST_ID: &str = "X-Tunnels-VSCode-Client-Request-Id";

/// O resultado de `tunnelServiceHeaders`: o SDK copia estes cabeçalhos separadamente para cada requisição de
/// gerenciamento e para cada handshake de relay do Node.js.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunnelServiceHeaders {
    session_id: String,
    operation_id: String,
}

impl TunnelServiceHeaders {
    /// Os cabeçalhos, com um `X-Tunnels-VSCode-Client-Request-Id` novo a cada chamada.
    pub fn to_vec(&self) -> Vec<(&'static str, String)> {
        vec![
            (HEADER_SESSION_ID, self.session_id.clone()),
            (HEADER_CLIENT_OPERATION_ID, self.operation_id.clone()),
            (HEADER_CLIENT_REQUEST_ID, generate_uuid()),
        ]
    }
}

/// `tunnelServiceHeaders`.
pub fn tunnel_service_headers(correlation: &ITunnelServiceCorrelation) -> TunnelServiceHeaders {
    TunnelServiceHeaders { session_id: correlation.session_id.clone(), operation_id: correlation.operation_id.clone() }
}
