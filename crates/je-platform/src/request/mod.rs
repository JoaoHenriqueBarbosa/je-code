//! Porte de `src/vs/platform/request/common` do VS Code: o contrato do serviço de requisição, a base
//! (`AbstractRequestService`) com o log e o evento de fim de requisição, os auxiliares de resposta
//! (`isSuccess`, `asText`, `asJson`...) e o canal de IPC.
//!
//! Fora do porte (e por quê): `node/requestService.ts` e `electron-utility/requestService.ts` (o cliente HTTP
//! sobre `http`/`https`/proxy do Node e do Electron: o je-code ainda não escolheu um cliente HTTP, e TLS pede uma
//! pilha criptográfica sem C, o que a régua de `scripts/pure-rust-check.sh` restringe), `node/proxy.ts` e o
//! registro das configurações `http.*` (`registerProxyConfigurations`, `updateProxyConfigurationsScope`:
//! dependem do cliente HTTP e do registro de configuração por escopo).

#[allow(clippy::module_inception)] // `request.rs` é o porte de `request.ts`, como no original
pub mod request;
pub mod request_ipc;

pub use request::*;
