//! Porte parcial de `src/vs/workbench/contrib/scm/browser/activity.ts`: o `SCMActiveRepositoryController` com as variáveis
//! de título e as chaves de contexto do repositório ativo (`scmActiveRepositoryName` e `scmActiveRepositoryBranchName`).
//!
//! Divergências:
//! - **O que não entra.** O selo de contagem na barra de atividade (`scm.countBadge`, `IActivityService`), as entradas da
//!   barra de status (`IStatusbarService`, `getStatusBarCommandGenericName`, `getSCMRepositoryIcon`) e o
//!   `SCMActiveResourceContextKeyController` dependem de `util.ts`, do `IStatusbarService` e do `IEditorGroupsService`
//!   (provedores de chave de contexto por grupo), que não estão portados com os membros que eles leem
//!   (`ISCMProvider.count`, `statusBarCommands`, `label`, `onDidChangeResources`). Entram quando esses contratos entrarem; o
//!   construtor ganha então os parâmetros `activityService`, `configurationService` e `statusbarService` do original.
//! - Os serviços são contratos estreitos deste módulo: [`ITitleService`] (só `registerVariables`) e
//!   [`ISCMActiveViewService`] (só `activeRepository`); o provedor é [`ISCMActiveProvider`] (`name`, `activeRepositoryName` e
//!   o nome do `historyItemRef` do `historyProvider`, que o original lê encadeando os dois observáveis).
//! - `activeRepository: IObservable<{ repository, pinned } | undefined>` é `IObservable<Option<Rc<ActiveRepository>>>` (a
//!   igualdade do observável é a identidade do `Rc`, como o `===` do original).

use std::rc::Rc;

use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::observable::base::IObservable;
use je_base::observable::{ObservableExt, autorun, derived};
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};

/// `ISCMProvider`, só o que o controlador lê.
pub trait ISCMActiveProvider {
    /// `name`.
    fn name(&self) -> String;
    /// `activeRepositoryName`.
    fn active_repository_name(&self) -> IObservable<Option<String>>;
    /// `historyProvider.read(reader)?.historyItemRef.read(reader)?.name`.
    fn history_item_ref_name(&self) -> IObservable<Option<String>>;
}

/// `ISCMRepository`, só o `provider`.
pub trait ISCMActiveRepository {
    fn provider(&self) -> Rc<dyn ISCMActiveProvider>;
}

/// `{ repository: ISCMRepository; pinned: boolean }`.
pub struct ActiveRepository {
    pub repository: Rc<dyn ISCMActiveRepository>,
    pub pinned: bool,
}

/// `ISCMViewService`, só `activeRepository`.
pub trait ISCMActiveViewService {
    fn active_repository(&self) -> IObservable<Option<Rc<ActiveRepository>>>;
}

/// `ITitleVariable` (o argumento de `registerVariables`): o nome da variável e a chave de contexto que a alimenta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITitleVariable {
    pub name: String,
    pub context_key: String,
}

/// `ITitleService`, só `registerVariables`.
pub trait ITitleService {
    fn register_variables(&self, variables: Vec<ITitleVariable>);
}

/// `ActiveRepositoryContextKeys.ActiveRepositoryName`.
pub const ACTIVE_REPOSITORY_NAME_KEY: &str = "scmActiveRepositoryName";
/// `ActiveRepositoryContextKeys.ActiveRepositoryBranchName`.
pub const ACTIVE_REPOSITORY_BRANCH_NAME_KEY: &str = "scmActiveRepositoryBranchName";

/// `SCMActiveRepositoryController` (o recorte descrito no cabeçalho).
pub struct SCMActiveRepositoryController {
    store: Rc<DisposableStore>,
}

impl SCMActiveRepositoryController {
    /// `new SCMActiveRepositoryController(..., contextKeyService, ..., scmViewService, ..., titleService)`.
    pub fn new(
        context_key_service: Rc<dyn IContextKeyService>,
        scm_view_service: Rc<dyn ISCMActiveViewService>,
        title_service: Rc<dyn ITitleService>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();

        let active_repository_name_key = RawContextKey::new(ACTIVE_REPOSITORY_NAME_KEY, "", ContextKeyMeta::None).bind_to(&*context_key_service);
        let active_repository_branch_name_key =
            RawContextKey::new(ACTIVE_REPOSITORY_BRANCH_NAME_KEY, "", ContextKeyMeta::None).bind_to(&*context_key_service);

        title_service.register_variables(vec![
            ITitleVariable { name: "activeRepositoryName".to_string(), context_key: ACTIVE_REPOSITORY_NAME_KEY.to_string() },
            ITitleVariable { name: "activeRepositoryBranchName".to_string(), context_key: ACTIVE_REPOSITORY_BRANCH_NAME_KEY.to_string() },
        ]);

        let active_repository = scm_view_service.active_repository();

        let history_source = active_repository.clone();
        let active_repository_history_item_ref_name: IObservable<Option<String>> = derived(move |reader| {
            let active_repository = history_source.read(reader);
            active_repository.and_then(|active| active.repository.provider().history_item_ref_name().read(reader))
        });

        store.add(autorun(move |reader| {
            let active_repository = active_repository.read(reader);
            let history_item_ref_name = active_repository_history_item_ref_name.read(reader);
            let provider = active_repository.map(|active| active.repository.provider());
            let repository_name = provider.as_ref().and_then(|provider| provider.active_repository_name().read(reader).or_else(|| Some(provider.name())));

            Self::update_active_repository_context_keys(
                &*active_repository_name_key,
                &*active_repository_branch_name_key,
                repository_name,
                history_item_ref_name,
            );
        }));

        Rc::new(Self { store })
    }

    /// `_updateActiveRepositoryContextKeys(repositoryName, branchName)`.
    fn update_active_repository_context_keys(
        name_key: &dyn IContextKey,
        branch_name_key: &dyn IContextKey,
        repository_name: Option<String>,
        branch_name: Option<String>,
    ) {
        name_key.set(repository_name.unwrap_or_default().into());
        branch_name_key.set(branch_name.unwrap_or_default().into());
    }
}

impl IDisposable for SCMActiveRepositoryController {
    fn dispose(&self) {
        self.store.dispose();
    }
}
