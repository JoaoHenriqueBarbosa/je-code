//! O pedaço de `INativeEnvironmentService` (`src/vs/platform/environment/common/environment.ts`) que o
//! coordenador do processo de túnel e o `RemoteTunnelService` leem: `appRoot`, `isBuilt`, `userDataPath` e
//! `logsHome`.
//!
//! Divergência: o `INativeEnvironmentService` do original é o objeto inteiro; o trait
//! [`NativeEnvironmentServiceApi`] do porte tem dezenas de métodos que estes dois serviços não leem, então eles
//! recebem este trait reduzido (como o `supports_telemetry` recebe só o que lê). [`NativeRemoteTunnelEnvironment`]
//! o implementa sobre o ambiente nativo de verdade.

use std::rc::Rc;

use je_base::uri::URI;

use crate::environment::environment::{EnvironmentService, NativeEnvironmentServiceApi};

/// O que os serviços do túnel remoto leem do `INativeEnvironmentService`.
pub trait RemoteTunnelEnvironment {
    /// `appRoot`.
    fn app_root(&self) -> String;
    /// `isBuilt`.
    fn is_built(&self) -> bool;
    /// `userDataPath`.
    fn user_data_path(&self) -> String;
    /// `logsHome`.
    fn logs_home(&self) -> URI;
}

/// O [`RemoteTunnelEnvironment`] do ambiente nativo.
pub struct NativeRemoteTunnelEnvironment(pub Rc<dyn NativeEnvironmentServiceApi>);

impl RemoteTunnelEnvironment for NativeRemoteTunnelEnvironment {
    fn app_root(&self) -> String {
        self.0.app_root()
    }

    fn is_built(&self) -> bool {
        self.0.is_built()
    }

    fn user_data_path(&self) -> String {
        self.0.user_data_path()
    }

    fn logs_home(&self) -> URI {
        self.0.logs_home()
    }
}
