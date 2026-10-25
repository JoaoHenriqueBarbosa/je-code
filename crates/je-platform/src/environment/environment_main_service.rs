//! Porte de `src/vs/platform/environment/electron-main/environmentMainService.ts`: o `EnvironmentMainService`, o
//! serviço de ambiente do processo principal.
//!
//! Decisões de tradução:
//! - **Herança**: `EnvironmentMainService extends NativeEnvironmentService` é a composição com
//!   [`AbstractNativeEnvironmentService`] (campo `base`), exposta por `Deref`, então tudo o que o serviço nativo
//!   tem (inclusive `disable_updates`, do `INativeEnvironmentService`) vale aqui.
//! - **`process.env`**: escrever no ambiente do processo é `unsafe` em Rust (edição 2024) e o je-code proíbe
//!   `unsafe`. O serviço guarda a sua própria cópia do ambiente (`process_env`), lida na construção, e é ela que
//!   `unsetSnapExportedVariables`, `restoreSnapExportedVariables` e os getters leem e escrevem. Quem lança um
//!   processo filho passa [`EnvironmentMainService::process_env`] como o ambiente dele, que é o efeito que o
//!   `process.env` mutado tem sobre `child_process.spawn` no original.
//! - **`isLinux`** é `cfg!(target_os = "linux")`.
//! - **`@memoize`**: como em `environment_service.rs`, os getters são calculados a cada chamada.
//!
//! Fora do porte: `mainIPCHandle` (precisa do `IpcHandleEnvironment`, que o processo principal monta ao criar o
//! servidor de IPC; o cálculo é `create_static_ipc_handle`).

use std::cell::RefCell;
use std::ops::Deref;

use indexmap::IndexMap;
use je_base::path::{PathModule, ProcessEnvironment};

use super::environment::NativeEnvironmentServiceApi;
use super::environment_service::AbstractNativeEnvironmentService;

/// O sufixo das variáveis que o snap guarda com o valor de antes de entrar nele.
const SNAP_ORIG_SUFFIX: &str = "_VSCODE_SNAP_ORIG";

/// `EnvironmentMainService`.
pub struct EnvironmentMainService {
    base: AbstractNativeEnvironmentService,
    /// O `process.env` do processo principal.
    process_env: RefCell<IndexMap<String, String>>,
    /// `_snapEnv`.
    snap_env: RefCell<IndexMap<String, String>>,
}

impl Deref for EnvironmentMainService {
    type Target = AbstractNativeEnvironmentService;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl EnvironmentMainService {
    /// `process_env` é o ambiente do processo (por exemplo `std::env::vars()`).
    pub fn new(base: AbstractNativeEnvironmentService, process_env: impl IntoIterator<Item = (String, String)>) -> Self {
        Self { base, process_env: RefCell::new(process_env.into_iter().collect()), snap_env: RefCell::default() }
    }

    /// `process.env[name]`.
    pub fn process_env_var(&self, name: &str) -> Option<String> {
        self.process_env.borrow().get(name).cloned()
    }

    /// `process.env[name] = value`.
    pub fn set_process_env_var(&self, name: &str, value: &str) {
        self.process_env.borrow_mut().insert(name.to_owned(), value.to_owned());
    }

    /// `delete process.env[name]`.
    pub fn delete_process_env_var(&self, name: &str) {
        self.process_env.borrow_mut().shift_remove(name);
    }

    /// O ambiente que um processo filho herda.
    pub fn process_env(&self) -> Vec<(String, String)> {
        self.process_env.borrow().iter().map(|(key, value)| (key.clone(), value.clone())).collect()
    }

    /// `!!process.env[name]`, o valor não vazio de uma variável.
    fn non_empty_env_var(&self, name: &str) -> Option<String> {
        self.process_env_var(name).filter(|value| !value.is_empty())
    }

    /// `backupHome`: `join(this.userDataPath, 'Backups')`.
    pub fn backup_home(&self) -> String {
        crate::backup::backup_main_service::backup_home_of(&self.base)
    }

    /// `mainLockfile`: `join(this.userDataPath, 'code.lock')`.
    pub fn main_lockfile(&self) -> String {
        PathModule::native(&ProcessEnvironment).join(&[&self.base.user_data_path(), "code.lock"])
    }

    /// `isPortable`: `!!process.env['VSCODE_PORTABLE']`.
    pub fn is_portable(&self) -> bool {
        self.non_empty_env_var("VSCODE_PORTABLE").is_some()
    }

    /// `crossOriginIsolated`: `!!this.args['enable-coi']`.
    pub fn cross_origin_isolated(&self) -> bool {
        self.base.args().get_bool("enable-coi")
    }

    /// `enableRDPDisplayTracking`: `!!this.args['enable-rdp-display-tracking']`.
    pub fn enable_rdp_display_tracking(&self) -> bool {
        self.base.args().get_bool("enable-rdp-display-tracking")
    }

    /// `codeCachePath`: `process.env['VSCODE_CODE_CACHE_PATH'] || undefined`.
    pub fn code_cache_path(&self) -> Option<String> {
        self.non_empty_env_var("VSCODE_CODE_CACHE_PATH")
    }

    /// `useCodeCache`: `!!this.codeCachePath`.
    pub fn use_code_cache(&self) -> bool {
        self.code_cache_path().is_some()
    }

    /// `unsetSnapExportedVariables`: devolve a cada variável o valor de antes do snap (guardado em
    /// `<nome>_VSCODE_SNAP_ORIG`), e a remove se o valor de antes era vazio. Só no Linux.
    pub fn unset_snap_exported_variables(&self) {
        if !cfg!(target_os = "linux") {
            return;
        }
        let keys: Vec<String> = self.process_env.borrow().keys().filter(|key| key.ends_with(SNAP_ORIG_SUFFIX)).cloned().collect();
        for key in keys {
            let original_key = &key[..key.len() - SNAP_ORIG_SUFFIX.len()];
            if self.snap_env.borrow().get(original_key).is_some_and(|value| !value.is_empty()) {
                continue;
            }
            // Preserve the original value in case the snap env is re-entered
            if let Some(current) = self.non_empty_env_var(original_key) {
                self.snap_env.borrow_mut().insert(original_key.to_owned(), current);
            }
            // Copy the original value from before entering the snap env if available,
            // if not delete the env variable.
            match self.non_empty_env_var(&key) {
                Some(original) => self.set_process_env_var(original_key, &original),
                None => self.delete_process_env_var(original_key),
            }
        }
    }

    /// `restoreSnapExportedVariables`.
    pub fn restore_snap_exported_variables(&self) {
        if !cfg!(target_os = "linux") {
            return;
        }
        let saved: Vec<(String, String)> = self.snap_env.borrow_mut().drain(..).collect();
        for (key, value) in saved {
            self.set_process_env_var(&key, &value);
        }
    }
}
