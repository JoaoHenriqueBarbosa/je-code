//! Porte de `src/vs/platform/environment/node/environmentService.ts`: o `NativeEnvironmentService`, que
//! junta o home, o tmp e o diretório de dados do usuário do sistema real, e as funções que leem as portas de
//! depuração dos processos auxiliares.

use std::rc::Rc;

use je_base::errors::Error;
use je_base::path::ProcessEnvironment;

use super::environment_service::{AbstractNativeEnvironmentService, NativeEnvironmentPaths, parse_debug_params};
use super::environment::ExtensionHostDebugParams;
use super::native_parsed_args::NativeParsedArgs;
use super::user_data_path::{get_user_data_path, home_dir, tmp_dir};
use crate::product::ProductService;

/// `NativeEnvironmentService`.
pub type NativeEnvironmentService = AbstractNativeEnvironmentService;

impl NativeEnvironmentService {
    /// `new NativeEnvironmentService(args, productService)`: o home e o tmp do sistema e o diretório de dados
    /// do usuário do produto. `app_root` é onde a aplicação está instalada.
    pub fn native(args: NativeParsedArgs, product_service: ProductService, app_root: &str) -> Result<Self, Error> {
        let user_data_dir = get_user_data_path(&args, &product_service.name_short)?;
        let paths = NativeEnvironmentPaths { user_data_dir, home_dir: home_dir(), tmp_dir: tmp_dir(), app_root: app_root.to_owned() };
        Ok(Self::new(args, paths, product_service, Rc::new(ProcessEnvironment)))
    }
}

// O original passa `args.extensionEnvironment` no lugar do `debugId` nas três funções abaixo; a
// semântica é mantida, e o resultado é o mesmo tipo do depurador de extensões.

/// `parsePtyHostDebugPort(args, isBuilt)`.
pub fn parse_pty_host_debug_port(args: &NativeParsedArgs, is_built: bool) -> ExtensionHostDebugParams {
    parse_debug_params(
        args.get_string("inspect-ptyhost").as_deref(),
        args.get_string("inspect-brk-ptyhost").as_deref(),
        5877,
        is_built,
        args.get_string("extensionEnvironment"),
        None,
    )
}

/// `parseAgentHostDebugPort(args, isBuilt)`.
pub fn parse_agent_host_debug_port(args: &NativeParsedArgs, is_built: bool) -> ExtensionHostDebugParams {
    parse_debug_params(
        args.get_string("inspect-agenthost").as_deref(),
        args.get_string("inspect-brk-agenthost").as_deref(),
        5878,
        is_built,
        args.get_string("extensionEnvironment"),
        None,
    )
}

/// `parseSharedProcessDebugPort(args, isBuilt)`.
pub fn parse_shared_process_debug_port(args: &NativeParsedArgs, is_built: bool) -> ExtensionHostDebugParams {
    parse_debug_params(
        args.get_string("inspect-sharedprocess").as_deref(),
        args.get_string("inspect-brk-sharedprocess").as_deref(),
        5879,
        is_built,
        args.get_string("extensionEnvironment"),
        None,
    )
}
