//! Porte de `src/vs/workbench/services/path/common/pathService.ts`: o `AbstractPathService`, a base dos serviços de
//! caminho do workbench (no navegador e no nativo), que resolve o sistema operacional e o home do usuário do
//! ambiente alvo (o remoto, quando há).
//!
//! Notas de tradução:
//! - **Reexportação.** O `export { IPathService }` e o `export type { IResourcePathProvider }` do original são os
//!   `pub use` abaixo (o contrato vive em `je_platform::path::path_service`).
//! - **Classe abstrata.** `AbstractPathService` é a struct concreta que as subclasses compõem; o construtor recebe o
//!   `localUserHome` e os três serviços injetados, e devolve a struct (não um `Rc`) para quem a embute.
//! - **`async`/`await`.** `resolveOS` e `resolveUserHome` são as promessas dos IIFE assíncronos do original, e as
//!   funções `async` encadeiam a promessa com `then`/`then_promise`. O `maybeUnresolvedUserHome` é um `Rc<RefCell>`
//!   porque o encadeamento o grava depois da construção.
//! - **Sobrecargas** de `userHome` e `hasValidBasename`: ver o contrato.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::extpath::is_valid_basename;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::network::schemas;
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_platform::resources::ext_uri;
use je_platform::virtual_workspace::get_virtual_workspace_scheme;
use je_platform::workspace::IWorkspaceContextService;

use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;

pub use je_platform::path::path_service::{I_PATH_SERVICE, IPathService, IResourcePathProvider, PathFlavor};

/// `AbstractPathService`.
pub struct AbstractPathService {
    local_user_home: URI,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    resolve_os: Promise<OperatingSystem>,
    resolve_user_home: Promise<URI>,
    maybe_unresolved_user_home: Rc<RefCell<Option<URI>>>,
    path_providers: Rc<RefCell<HashMap<String, Vec<Rc<dyn IResourcePathProvider>>>>>,
}

/// O `OS` do processo hospedeiro.
fn host_operating_system() -> OperatingSystem {
    Platform::detect().os
}

/// `doHasValidBasename`: o `isValidBasename` só vale para os esquemas padrão de arquivos em disco, locais ou
/// remotos.
fn do_has_valid_basename(resource: &URI, os: OperatingSystem, name: Option<&str>) -> bool {
    if resource.scheme() == schemas::FILE || resource.scheme() == schemas::VSCODE_REMOTE {
        let basename = ext_uri().basename(resource, None);
        return is_valid_basename(Some(name.unwrap_or(&basename)), os == OperatingSystem::Windows);
    }

    true
}

/// O trecho de `getOperatingSystem` depois dos provedores registrados.
fn fallback_operating_system(resource: &URI, resolve_os: &Promise<OperatingSystem>) -> Promise<Option<OperatingSystem>> {
    if resource.scheme() == schemas::FILE {
        return Promise::resolved(Some(host_operating_system()));
    }
    if resource.scheme() == schemas::VSCODE_REMOTE {
        return resolve_os.then(Some);
    }
    Promise::resolved(None)
}

/// O laço `for (let index = providers.length - 1; index >= 0; index--)`: pergunta ao provedor de `remaining - 1`, e
/// segue para o anterior enquanto a resposta for `undefined`.
fn ask_providers(providers: Vec<Rc<dyn IResourcePathProvider>>, remaining: usize, resource: URI) -> Promise<Option<OperatingSystem>> {
    if remaining == 0 {
        return Promise::resolved(None);
    }
    providers[remaining - 1].get_operating_system(&resource).then_promise(move |operating_system| {
        if operating_system.is_some() {
            return Promise::resolved(operating_system);
        }
        ask_providers(providers, remaining - 1, resource)
    })
}

impl AbstractPathService {
    pub fn new(
        local_user_home: URI,
        remote_agent_service: Rc<dyn IRemoteAgentService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        context_service: Rc<dyn IWorkspaceContextService>,
    ) -> Self {
        // OS
        let resolve_os = remote_agent_service.get_environment().then(|env| env.map_or_else(host_operating_system, |env| env.os));

        // User Home
        let maybe_unresolved_user_home = Rc::new(RefCell::new(None));
        let unresolved = maybe_unresolved_user_home.clone();
        let fallback_home = local_user_home.clone();
        let resolve_user_home = remote_agent_service.get_environment().then(move |env| {
            let user_home = env.map_or(fallback_home, |env| env.user_home);
            *unresolved.borrow_mut() = Some(user_home.clone());
            user_home
        });

        Self {
            local_user_home,
            environment_service,
            context_service,
            resolve_os,
            resolve_user_home,
            maybe_unresolved_user_home,
            path_providers: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn find_default_uri_scheme(environment_service: &dyn IWorkbenchEnvironmentService, context_service: &dyn IWorkspaceContextService) -> String {
        if environment_service.remote_authority().is_some_and(|authority| !authority.is_empty()) {
            return schemas::VSCODE_REMOTE.to_string();
        }

        let workspace = context_service.get_workspace();
        if let Some(virtual_workspace) = get_virtual_workspace_scheme(workspace.as_ref()) {
            return virtual_workspace;
        }

        if let Some(first_folder) = workspace.folders().first() {
            return first_folder.uri.scheme().to_string_lossy();
        }

        if let Some(configuration) = workspace.configuration() {
            return configuration.scheme().to_string_lossy();
        }

        schemas::FILE.to_string()
    }
}

impl IPathService for AbstractPathService {
    fn path(&self) -> Promise<PathFlavor> {
        self.resolve_os.then(OperatingSystem::path_flavor)
    }

    fn default_uri_scheme(&self) -> String {
        Self::find_default_uri_scheme(self.environment_service.as_ref(), self.context_service.as_ref())
    }

    fn file_uri(&self, path: &str) -> Promise<URI> {
        let path = path.to_string();
        self.resolve_os.then(move |os| {
            let mut authority = String::new();

            // normalize to fwd-slashes on windows,
            // on other systems bwd-slashes are valid
            // filename character, eg /f\oo/ba\r.txt
            let mut path = if os == OperatingSystem::Windows { path.replace('\\', "/") } else { path };

            // check for authority as used in UNC shares
            // or use the path as given
            if path.starts_with("//") {
                match path[2..].find('/') {
                    None => {
                        authority = path[2..].to_string();
                        path = "/".to_string();
                    }
                    Some(offset) => {
                        // `_path.substring(idx) || '/'`: a partir de uma barra o resto nunca é vazio
                        let idx = offset + 2;
                        authority = path[2..idx].to_string();
                        path = path[idx..].to_string();
                    }
                }
            }

            URI::from(
                &UriComponents {
                    scheme: Some(JsString::from(schemas::FILE)),
                    authority: Some(JsString::from(authority)),
                    path: Some(JsString::from(path)),
                    query: Some(JsString::new()),
                    fragment: Some(JsString::new()),
                },
                false,
            )
        })
    }

    fn user_home(&self) -> Promise<URI> {
        self.resolve_user_home.clone()
    }

    fn local_user_home(&self) -> URI {
        self.local_user_home.clone()
    }

    fn has_valid_basename(&self, resource: &URI, basename: Option<&str>) -> Promise<bool> {
        let resource = resource.clone();
        let basename = basename.map(str::to_string);
        self.resolve_os.then(move |os| do_has_valid_basename(&resource, os, basename.as_deref()))
    }

    fn has_valid_basename_for_os(&self, resource: &URI, os: OperatingSystem, basename: Option<&str>) -> bool {
        do_has_valid_basename(resource, os, basename)
    }

    fn get_operating_system(&self, resource: &URI) -> Promise<Option<OperatingSystem>> {
        let key = resource.scheme().to_lower_case().to_string_lossy();
        let providers = self.path_providers.borrow().get(&key).cloned();
        let resolve_os = self.resolve_os.clone();
        let resource = resource.clone();
        match providers {
            Some(providers) => {
                let count = providers.len();
                ask_providers(providers, count, resource.clone()).then_promise(move |operating_system| {
                    if operating_system.is_some() {
                        return Promise::resolved(operating_system);
                    }
                    fallback_operating_system(&resource, &resolve_os)
                })
            }
            None => fallback_operating_system(&resource, &resolve_os),
        }
    }

    fn get_path(&self, resource: &URI) -> Promise<Option<PathFlavor>> {
        self.get_operating_system(resource).then(|operating_system| operating_system.map(OperatingSystem::path_flavor))
    }

    fn register_path_provider(&self, scheme: &str, provider: Rc<dyn IResourcePathProvider>) -> DisposableRef {
        let key = JsString::from(scheme).to_lower_case().to_string_lossy();
        self.path_providers.borrow_mut().entry(key.clone()).or_default().push(provider.clone());

        let path_providers = self.path_providers.clone();
        to_disposable(move || {
            let mut map = path_providers.borrow_mut();
            let Some(providers) = map.get_mut(&key) else { return };
            if let Some(index) = providers.iter().position(|candidate| std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&provider))) {
                providers.remove(index);
                if providers.is_empty() {
                    map.remove(&key);
                }
            }
        })
    }

    fn resolved_user_home(&self) -> Option<URI> {
        self.maybe_unresolved_user_home.borrow().clone()
    }
}
