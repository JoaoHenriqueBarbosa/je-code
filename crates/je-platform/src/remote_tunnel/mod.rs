//! Porte de `src/vs/platform/remoteTunnel` do VS Code: os tipos e o contrato do serviço de túnel remoto
//! ([`remote_tunnel`]), o parser das linhas de status que a CLI do túnel emite ([`tunnel_machine_status`]) e a
//! correlação das operações do serviço de túnel ([`tunnel_service_headers`]); no lado Node, a execução da CLI
//! ([`code_tunnel_cli_process`]), o coordenador do único processo de túnel ([`tunnel_process_coordinator`]) e o
//! [`remote_tunnel_service`].
//!
//! Não confundir com [`crate::remote`], que porta `src/vs/platform/remote`.

#[cfg(not(target_arch = "wasm32"))]
pub mod code_tunnel_cli_process;
#[allow(clippy::module_inception)] // `remote_tunnel.rs` é o porte de `remoteTunnel.ts`, como no original
pub mod remote_tunnel;
#[cfg(not(target_arch = "wasm32"))]
pub mod remote_tunnel_service;
pub mod tunnel_machine_status;
#[cfg(not(target_arch = "wasm32"))]
pub mod tunnel_process_coordinator;
pub mod tunnel_service_headers;
