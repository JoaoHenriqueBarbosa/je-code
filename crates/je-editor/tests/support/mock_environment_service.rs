//! `mock<IEnvironmentService>()` com `isBuilt` e `isExtensionDevelopment` (o `mock` de `base/test/common/mock.ts`): o que o
//! caso não lê é `unimplemented!`, como um `mock` sem a propriedade.

use std::rc::Rc;

use je_base::uri::URI;
use je_platform::environment::environment::{DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, SyncMode};

pub struct MockEnvironmentService {
    is_built: bool,
    is_extension_development: bool,
}

impl MockEnvironmentService {
    pub fn new(is_built: bool, is_extension_development: bool) -> Rc<Self> {
        Rc::new(Self { is_built, is_extension_development })
    }
}

impl EnvironmentService for MockEnvironmentService {
    fn state_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn user_roaming_data_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn keyboard_layout_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn argv_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn untitled_workspaces_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn workspace_storage_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn local_history_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn cache_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn app_shared_data_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn user_data_sync_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn sync(&self) -> Option<SyncMode> {
        unimplemented!("mock")
    }
    fn continue_on(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn edit_session_id(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn debug_extension_host(&self) -> ExtensionHostDebugParams {
        unimplemented!("mock")
    }
    fn is_extension_development(&self) -> bool {
        self.is_extension_development
    }
    fn disable_extensions(&self) -> DisableExtensions {
        unimplemented!("mock")
    }
    fn skip_builtin_extensions(&self) -> Vec<String> {
        unimplemented!("mock")
    }
    fn extension_development_location_uri(&self) -> Option<Vec<URI>> {
        unimplemented!("mock")
    }
    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>> {
        unimplemented!("mock")
    }
    fn extension_tests_location_uri(&self) -> Option<URI> {
        unimplemented!("mock")
    }
    fn logs_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn log_level(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn extension_log_level(&self) -> Option<Vec<(String, String)>> {
        unimplemented!("mock")
    }
    fn verbose(&self) -> bool {
        unimplemented!("mock")
    }
    fn is_built(&self) -> bool {
        self.is_built
    }
    fn disable_telemetry(&self) -> bool {
        unimplemented!("mock")
    }
    fn disable_experiments(&self) -> bool {
        unimplemented!("mock")
    }
    fn service_machine_id_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn agent_sessions_workspace(&self) -> URI {
        unimplemented!("mock")
    }
    fn policy_file(&self) -> Option<URI> {
        unimplemented!("mock")
    }
}
