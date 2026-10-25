//! Porte parcial de `src/vs/platform/remote/common/electronRemoteResources.ts`: os nomes do método e do canal do IPC dos recursos
//! remotos e a resposta que o lado remoto devolve ([`NodeRemoteResourceResponse`]).
//!
//! Fora do porte: o `NodeRemoteResourceRouter` (um `IClientRouter` que escolhe a conexão pelo `authority` do URI). Ele só existe
//! para ligar o canal `remoteResourceHandler` ao IPC do Electron, que no processo único não há; entra junto com o primeiro
//! consumidor do canal.

/// `NODE_REMOTE_RESOURCE_IPC_METHOD_NAME`.
pub const NODE_REMOTE_RESOURCE_IPC_METHOD_NAME: &str = "request";

/// `NODE_REMOTE_RESOURCE_CHANNEL_NAME`.
pub const NODE_REMOTE_RESOURCE_CHANNEL_NAME: &str = "remoteResourceHandler";

/// `NodeRemoteResourceResponse`: `{ body: string (base64); mimeType?: string; statusCode: number }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRemoteResourceResponse {
    /// O corpo, em base64.
    pub body: String,
    pub mime_type: Option<String>,
    pub status_code: u16,
}
