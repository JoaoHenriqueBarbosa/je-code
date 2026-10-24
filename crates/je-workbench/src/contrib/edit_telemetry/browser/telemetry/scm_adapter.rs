//! Porte parcial de `src/vs/workbench/contrib/editTelemetry/browser/telemetry/scmAdapter.ts`: o contrato
//! `IScmRepoAdapter` e o adaptador `ScmAdapter` que o `editSourceTrackingImpl.ts` usa para achar o repositório de um
//! documento.
//!
//! Representação: o `ScmAdapter` do original é uma classe criada por `createInstance` sobre o `ISCMService`. Aqui o
//! que o `EditSourceTrackingImpl` usa dele é o trait [`IScmAdapter`], registrado como o serviço [`I_SCM_ADAPTER`]
//! (o ponto em que os testes do original trocam a classe por `stubInstance(ScmAdapter, ...)`).
//!
//! Fica de fora: o `ScmAdapter` e o `ScmRepoAdapter` sobre o `ISCMService` (`onDidAddRepository`,
//! `onDidRemoveRepository`, `getRepository`, o `historyProvider` do repositório), porque o `contrib/scm` ainda não foi
//! portado. Eles entram como as implementações de [`IScmAdapter`] e [`IScmRepoAdapter`] quando o `ISCMService` existir.

use std::rc::Rc;

use je_base::observable::{IObservable, IReader};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `IScmRepoAdapter`.
pub trait IScmRepoAdapter {
    fn head_branch_name_obs(&self) -> IObservable<Option<String>>;

    fn head_commit_hash_obs(&self) -> IObservable<Option<String>>;

    fn is_ignored(&self, uri: &URI) -> Promise<bool>;
}

/// `ScmAdapter`: o repositório (se houver) de um documento.
pub trait IScmAdapter {
    /// `getRepo(uri, reader)`.
    fn get_repo(&self, uri: &URI, reader: Option<&dyn IReader>) -> Option<Rc<dyn IScmRepoAdapter>>;
}

service_id!(pub I_SCM_ADAPTER: dyn IScmAdapter = "scmAdapter");
