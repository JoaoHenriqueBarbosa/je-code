//! Porte parcial de `src/vs/platform/native/common/native.ts`: só os tipos das propriedades do sistema operacional
//! (`ICPUProperties` e `IOSProperties`), que o diálogo "Sobre" (`dialogs/electron-browser/dialog.ts`) lê.
//!
//! O resto do arquivo é o `ICommonNativeHostService`, a interface das chamadas nativas da janela; ele entra junto com quem
//! o consumir.

/// `ICPUProperties`.
#[derive(Clone, Debug, PartialEq)]
pub struct CpuProperties {
    pub model: String,
    pub speed: f64,
}

/// `IOSProperties`.
#[derive(Clone, Debug, PartialEq)]
pub struct OsProperties {
    /// `type`.
    pub os_type: String,
    pub release: String,
    pub arch: String,
    pub platform: String,
    pub cpus: Vec<CpuProperties>,
}
