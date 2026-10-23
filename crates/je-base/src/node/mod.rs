//! Porte de `src/vs/base/node`: a parte sem E/S de rede ou de processo, que o VS Code compartilha entre o
//! servidor remoto e o desktop.

pub mod crypto;
pub mod ps;
pub mod unc;

pub mod id;
pub mod mac_address;
pub mod network_interfaces;
