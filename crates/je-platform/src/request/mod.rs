//! Porte de `src/vs/platform/request/common` do VS Code: o contrato do serviço de requisição, a base
//! (`AbstractRequestService`) com o log e o evento de fim de requisição, os auxiliares de resposta
//! (`isSuccess`, `asText`, `asJson`...) e o canal de IPC. Fora do alvo `wasm32`, também o lado Node:
//! `node/proxy.ts` ([`node_proxy`]) e `node/requestService.ts` ([`node_request_service`]).
//!
//! Fora do porte (e por quê): `electron-utility/requestService.ts` (a requisição sobre o `net` do Electron, que não
//! existe aqui) e o registro das configurações `http.*` (`registerProxyConfigurations`,
//! `updateProxyConfigurationsScope`: dependem do registro de configuração por escopo).

#[allow(clippy::module_inception)] // `request.rs` é o porte de `request.ts`, como no original
pub mod request;
pub mod request_ipc;
#[cfg(not(target_arch = "wasm32"))]
pub mod node_proxy;
#[cfg(not(target_arch = "wasm32"))]
pub mod node_request_service;

pub use request::*;
