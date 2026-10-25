//! Porte de `src/vs/platform/git/common/localGitService.ts`: o contrato do serviço de baixo nível que executa comandos
//! git na máquina local.
//!
//! Decisões de tradução:
//! - **`ILocalGitService`** é o trait [`ILocalGitService`]; os métodos `async` devolvem [`Promise`]. O método `clone` se
//!   chama [`ILocalGitService::clone_repository`], porque `clone` é o nome do `Clone` do Rust e um objeto de serviço
//!   (`Rc<dyn ILocalGitService>`) confundiria as duas coisas.
//! - **`interface X extends Y`**: [`IGitPullOptions`] guarda o [`IGitNetworkOptions`] no campo `network`, em vez de repetir
//!   os campos. Campo opcional (`foo?: boolean`) é `Option`, e o `undefined` do original é `None`.
//! - **`ref?: string`** é `Option<&str>`; a string vazia é falsa no original e aqui também (ver o `clone`).

use je_base::promise::Promise;
use je_instantiation::service_id;

/// `IGitAuthentication`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IGitAuthentication {
    pub url: String,
    pub authorization_header: String,
}

/// `IGitNetworkOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IGitNetworkOptions {
    pub authentication: Option<IGitAuthentication>,
    pub log_errors: Option<bool>,
}

/// `IGitPullOptions` (estende `IGitNetworkOptions`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IGitPullOptions {
    pub network: IGitNetworkOptions,
    pub allow_hard_reset_on_divergence: Option<bool>,
    /// Leaves tag and commit checkouts unchanged instead of pulling without a branch.
    pub skip_detached_head: Option<bool>,
}

/// Low-level service for executing git commands on the local machine.
/// Used in the shared process where Node.js APIs are available.
/// All path arguments are native file-system paths.
pub trait ILocalGitService {
    /// `clone(operationId, cloneUrl, targetPath, ref?, options?)`.
    fn clone_repository(&self, operation_id: &str, clone_url: &str, target_path: &str, git_ref: Option<&str>, options: Option<&IGitNetworkOptions>) -> Promise<()>;
    fn pull(&self, operation_id: &str, repo_path: &str, options: Option<&IGitPullOptions>) -> Promise<bool>;
    fn checkout(&self, operation_id: &str, repo_path: &str, treeish: &str, detached: Option<bool>) -> Promise<()>;
    fn checkout_commit(&self, operation_id: &str, repo_path: &str, commit: &str) -> Promise<()>;
    fn rev_parse(&self, repo_path: &str, git_ref: &str) -> Promise<String>;
    fn get_remote_url(&self, operation_id: &str, repo_path: &str, options: Option<&IGitNetworkOptions>) -> Promise<String>;
    fn fetch(&self, operation_id: &str, repo_path: &str, options: Option<&IGitNetworkOptions>) -> Promise<()>;
    /// O número de commits em `from_ref..to_ref` (um `number` do JavaScript).
    fn rev_list_count(&self, repo_path: &str, from_ref: &str, to_ref: &str) -> Promise<f64>;
    fn cancel(&self, operation_id: &str) -> Promise<()>;
}

service_id!(pub I_LOCAL_GIT_SERVICE: dyn ILocalGitService = "localGitService");
