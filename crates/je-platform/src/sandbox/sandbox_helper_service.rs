//! Porte de `src/vs/platform/sandbox/common/sandboxHelperService.ts`: o identificador do serviço, o estado
//! das dependências do sandbox e a interface do serviço.

/// `ISandboxDependencyStatus`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SandboxDependencyStatus {
    pub bubblewrap_installed: bool,
    pub bubblewrap_usable: bool,
    pub socat_installed: bool,
    pub bubblewrap_error: Option<String>,
    pub dependency_install_command: Option<String>,
    pub apparmor_restricts_unprivileged_user_namespaces: Option<bool>,
}

/// `ISandboxHelperService`. O original devolve uma `Promise`; aqui a verificação é síncrona, porque o
/// processo filho e a leitura de arquivo são chamadas bloqueantes.
pub trait ISandboxHelperService {
    /// `checkSandboxDependencies`: `None` fora do Linux.
    fn check_sandbox_dependencies(&self) -> Option<SandboxDependencyStatus>;
}

je_instantiation::service_id!(pub I_SANDBOX_HELPER_SERVICE: dyn ISandboxHelperService = "sandboxHelperService");
