//! Porte de `src/vs/workbench/contrib/scm/test/browser/activity.test.ts` (suíte `SCMActiveRepositoryController`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso, e `disposables.add(...)` é o
//!   `add_rc` dele.
//! - Os `new class extends mock<I...>()` são structs que implementam os contratos estreitos do módulo de produção (ver as
//!   divergências dele); `IActivityService`, `IStatusbarService` e o `TestConfigurationService` não têm parâmetro no
//!   construtor do porte, porque o recorte não os lê.
//! - `observableValue(this, x)` é `observable_value`, e `.set(x, undefined)` é `.set(x, None)`.
//! - `contextKeyService.getContextKeyValue('scmActiveRepositoryName')` é o `get_context_key_value` do
//!   `MockContextKeyService`; o `deepStrictEqual` da lista é a comparação elemento a elemento.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::observable::base::{IObservable, Settable};
use je_base::observable::observable_value;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_workbench::contrib::scm::browser::activity::{
    ActiveRepository, ISCMActiveProvider, ISCMActiveRepository, ISCMActiveViewService, ITitleService, ITitleVariable,
    SCMActiveRepositoryController,
};

/// O provedor do caso: `name = 'worktree'`, `activeRepositoryName` e `historyProvider` indefinidos.
struct MockProvider {
    active_repository_name: Rc<je_base::observable::observables::observable_value::ObservableValue<Option<String>, ()>>,
    history_item_ref_name: IObservable<Option<String>>,
}

impl ISCMActiveProvider for MockProvider {
    fn name(&self) -> String {
        "worktree".to_string()
    }
    fn active_repository_name(&self) -> IObservable<Option<String>> {
        self.active_repository_name.clone()
    }
    fn history_item_ref_name(&self) -> IObservable<Option<String>> {
        self.history_item_ref_name.clone()
    }
}

/// O repositório do caso: só o `provider`.
struct MockRepository {
    provider: Rc<MockProvider>,
}

impl ISCMActiveRepository for MockRepository {
    fn provider(&self) -> Rc<dyn ISCMActiveProvider> {
        self.provider.clone()
    }
}

/// `ISCMViewService` do caso: o `activeRepository` com `pinned: false`.
struct MockViewService {
    active_repository: IObservable<Option<Rc<ActiveRepository>>>,
}

impl ISCMActiveViewService for MockViewService {
    fn active_repository(&self) -> IObservable<Option<Rc<ActiveRepository>>> {
        self.active_repository.clone()
    }
}

/// `ITitleService` do caso: `registerVariables(): void { }`.
struct MockTitleService;

impl ITitleService for MockTitleService {
    fn register_variables(&self, _variables: Vec<ITitleVariable>) {}
}

#[test]
fn uses_and_clears_the_active_repository_name_override() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();

    let active_repository_name = observable_value::<Option<String>, ()>("activeRepositoryName", None);
    let provider = Rc::new(MockProvider {
        active_repository_name: active_repository_name.clone(),
        history_item_ref_name: observable_value::<Option<String>, ()>("historyItemRefName", None),
    });
    let repository: Rc<dyn ISCMActiveRepository> = Rc::new(MockRepository { provider });
    let active_repository = observable_value::<Option<Rc<ActiveRepository>>, ()>(
        "activeRepository",
        Some(Rc::new(ActiveRepository { repository, pinned: false })),
    );
    let scm_view_service = Rc::new(MockViewService { active_repository });
    let context_key_service = disposables.add_rc(MockContextKeyService::new());
    let title_service = Rc::new(MockTitleService);

    let context_key_service_for_controller: Rc<dyn IContextKeyService> = context_key_service.clone();
    disposables.add_rc(SCMActiveRepositoryController::new(context_key_service_for_controller, scm_view_service, title_service));

    let name = || context_key_service.get_context_key_value("scmActiveRepositoryName");
    let mut names = vec![name()];
    active_repository_name.set(Some("repository".to_string()), None);
    names.push(name());
    active_repository_name.set(None, None);
    names.push(name());

    let expected = ["worktree", "repository", "worktree"];
    assert_eq!(names.len(), expected.len());
    for (actual, expected) in names.iter().zip(expected) {
        assert_deep_strict_equal(actual, &Dynamic::string(expected));
    }
}
