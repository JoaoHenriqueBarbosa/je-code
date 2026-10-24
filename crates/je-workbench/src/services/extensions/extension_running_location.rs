//! Porte de `src/vs/workbench/services/extensions/common/extensionRunningLocation.ts`: onde uma extensão roda
//! (`LocalProcessRunningLocation`, `LocalWebWorkerRunningLocation`, `RemoteRunningLocation`).
//!
//! Divergências: a união `ExtensionRunningLocation` é um `enum`; `kind` e `affinity` são métodos (`affinity` é sempre `0` no remoto).

use super::extension_host_kind::ExtensionHostKind;

/// `ExtensionRunningLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionRunningLocation {
    LocalProcess { affinity: u32 },
    LocalWebWorker { affinity: u32 },
    Remote,
}

impl ExtensionRunningLocation {
    pub fn kind(&self) -> ExtensionHostKind {
        match self {
            Self::LocalProcess { .. } => ExtensionHostKind::LocalProcess,
            Self::LocalWebWorker { .. } => ExtensionHostKind::LocalWebWorker,
            Self::Remote => ExtensionHostKind::Remote,
        }
    }

    pub fn affinity(&self) -> u32 {
        match self {
            Self::LocalProcess { affinity } | Self::LocalWebWorker { affinity } => *affinity,
            Self::Remote => 0,
        }
    }

    /// `equals(other)`: o remoto só compara o tipo (a afinidade dele é sempre `0`).
    pub fn equals(&self, other: &ExtensionRunningLocation) -> bool {
        match self {
            Self::Remote => self.kind() == other.kind(),
            _ => self.kind() == other.kind() && self.affinity() == other.affinity(),
        }
    }

    /// `asString()`.
    pub fn as_string(&self) -> String {
        let name = match self {
            Self::LocalProcess { .. } => "LocalProcess",
            Self::LocalWebWorker { .. } => "LocalWebWorker",
            Self::Remote => return "Remote".to_owned(),
        };
        match self.affinity() {
            0 => name.to_owned(),
            affinity => format!("{name}{affinity}"),
        }
    }
}
