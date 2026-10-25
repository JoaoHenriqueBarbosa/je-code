//! Porte parcial de `src/vs/workbench/services/remote/common/tunnelModel.ts`: o [`CandidatePort`], a porta que o extension host
//! encontra escutando e envia ao `TunnelModel`.
//!
//! O resto do arquivo (`TunnelModel`, `PortsAttributes`, `parseAddress`...) entra quando um consumidor pedir.

/// `CandidatePort`.
#[derive(Clone, Debug, PartialEq)]
pub struct CandidatePort {
    pub host: String,
    pub port: u32,
    pub detail: Option<String>,
    pub pid: Option<u32>,
}
