//! Porte de `src/vs/platform/remoteTunnel/common` do VS Code: os tipos e o contrato do serviço de túnel
//! remoto ([`remote_tunnel`]) e o parser das linhas de status que a CLI do túnel emite
//! ([`tunnel_machine_status`]).
//!
//! Não confundir com [`crate::remote`], que porta `src/vs/platform/remote`.

#[allow(clippy::module_inception)] // `remote_tunnel.rs` é o porte de `remoteTunnel.ts`, como no original
pub mod remote_tunnel;
pub mod tunnel_machine_status;
