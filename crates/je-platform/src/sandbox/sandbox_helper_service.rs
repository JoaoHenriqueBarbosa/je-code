//! Porte de `src/vs/platform/sandbox/common/sandboxHelperService.ts`: o identificador do serviço, o estado
//! das dependências do sandbox e a interface do serviço.

use je_base::node::pfs::LocalFuture;

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

/// `ISandboxHelperService`. O `Promise` do original é um futuro local.
pub trait ISandboxHelperService {
    /// `checkSandboxDependencies`: `None` fora do Linux.
    fn check_sandbox_dependencies(&self) -> LocalFuture<'_, Option<SandboxDependencyStatus>>;
}

je_instantiation::service_id!(pub I_SANDBOX_HELPER_SERVICE: dyn ISandboxHelperService = "sandboxHelperService");
