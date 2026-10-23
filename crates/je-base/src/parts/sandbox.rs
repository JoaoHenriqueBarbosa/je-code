//! Porte de `src/vs/base/parts/sandbox/common/sandboxTypes.ts`: a configuração que todo renderer em sandbox
//! precisa para funcionar.
//!
//! O `electronTypes.ts` (as definições de tipo da API do Electron: `ipcRenderer`, `webFrame`, `contextBridge`)
//! descreve uma API de JavaScript que o je-code não tem e não foi portado.

use crate::process::ProcessEnvironment;
use crate::product::IProductConfiguration;

record! {
    /// `ISandboxConfiguration.nls`.
    pub struct ISandboxNls {
        req {
            /// All NLS messages produced by `localize` and `localize2` calls under `src/vs`.
            messages: Vec<String>,
        }
        opt {
            /// The actual language of the NLS messages (e.g. 'en', de' or 'pt-br').
            language: String,
        }
    }
}

record! {
    /// `ISandboxConfiguration`: the common properties required for any sandboxed renderer to function.
    pub struct ISandboxConfiguration {
        req {
            /// Identifier of the sandboxed renderer.
            window_id: f64,
            /// Root path of the JavaScript sources. Note: This is NOT the installation root directory itself but
            /// contained in it at a level that is platform dependent.
            app_root: String,
            /// Per window process environment.
            user_env: ProcessEnvironment,
            /// Product configuration.
            product: IProductConfiguration,
            /// NLS support
            nls: ISandboxNls,
        }
        opt {
            /// Configured zoom level.
            zoom_level: f64,
            /// Location of V8 code cache.
            code_cache_path: String,
            /// DEV time only: All CSS-modules that we have.
            css_modules: Vec<String>,
        }
    }
}
