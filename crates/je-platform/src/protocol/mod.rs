//! Porte de `src/vs/platform/protocol/electron-main` do VS Code: os protocolos do aplicativo (`vscode-file:`, `file:`,
//! `vscode-remote-resource:` e `vscode-managed-remote-resource:`) e o `ProtocolMainService`.
//!
//! O `Request`, o `Response` e o `net.fetch` do Electron estão em [`fetch`]; o namespace `COI` de `base/common/network.ts`, em
//! [`coi`]. Só existe no desktop: o web não tem processo principal que sirva protocolos.

pub mod coi;
pub mod fetch;
pub mod managed_remote_resource_protocol;
#[allow(clippy::module_inception)] // `protocol.rs` é o porte de `protocol.ts`, como no original
pub mod protocol;
pub mod protocol_main_service;
pub mod remote_resource_protocol;
