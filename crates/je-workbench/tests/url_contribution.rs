//! Porte de `src/vs/workbench/contrib/url/test/browser/url.contribution.test.ts` (suíte `Open URL action`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso; o `registerAction2` que o
//!   `import '../../browser/url.contribution.js'` faz ao carregar é `register_open_url_action`, registrado no guarda.
//! - `TestInstantiationService` com `stub(...)` é um `InstantiationService` sobre um `ServiceCollection` com os três
//!   serviços. `TestStorageService` é o `InMemoryStorageService`.
//! - `new class extends mock<IQuickInputService>() { override async input() }` é o `TestQuickInputService` do workbench com
//!   `stub_input_resolves`, que é o mesmo `input` fixo (os demais métodos lançam, como no `mock`).
//! - `new class extends mock<IOpenerService>() { override async open }` é [`RecordingOpenerService`], que guarda o recurso e
//!   as opções de cada `open` e lança nos outros métodos.
//! - `await instantiationService.invokeFunction(command.handler)`: o `Action2::run` do porte não devolve a promessa, então
//!   o caso espera as microtarefas pendentes com `timeout(0)` antes de conferir `opened`.
//! - `assert.deepStrictEqual(opened, [{ resource: input, options: { allowContributedOpeners: true } }])` confere o número de
//!   chamadas, o recurso (texto) e cada campo de [`OpenOptions`] (só `allow_contributed_openers` presente).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::commands::commands_registry;
use je_platform::opener::{
    AllowContributedOpeners, I_OPENER_SERVICE, IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource,
    ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_platform::quickinput::quick_input::{I_QUICK_INPUT_SERVICE, IQuickInputService};
use je_platform::storage::{I_STORAGE_SERVICE, StorageService};
use je_workbench::contrib::url::url_contribution::register_open_url_action;
use je_workbench::test::browser::workbench_test_services::TestQuickInputService;
use je_workbench::test::common::workbench_test_services::TestStorageService;

/// O `open` de `IOpenerService` que o caso grava: o recurso e as opções.
struct RecordingOpenerService {
    opened: RefCell<Vec<(OpenResource, Option<OpenOptions>)>>,
}

impl IOpenerService for RecordingOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        unreachable!("o mock só define open")
    }

    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        unreachable!("o mock só define open")
    }

    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        unreachable!("o mock só define open")
    }

    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        unreachable!("o mock só define open")
    }

    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        unreachable!("o mock só define open")
    }

    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        self.opened.borrow_mut().push((resource.clone(), options.cloned()));
        Promise::resolved(true)
    }

    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        unreachable!("o mock só define open")
    }
}

#[test]
fn allows_contributed_external_uri_openers_and_preserves_the_input() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    store.add(register_open_url_action());

    let input = "https://example.com/path%23fragment";
    let opened = Rc::new(RecordingOpenerService { opened: RefCell::new(Vec::new()) });

    let quick_input = TestQuickInputService::new();
    quick_input.stub_input_resolves(Some(input.to_string()));
    let quick_input_service: Rc<dyn IQuickInputService> = quick_input;
    let opener_service: Rc<dyn IOpenerService> = opened.clone();
    let storage_service = store.add_rc(TestStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());

    let instantiation_service = store.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_QUICK_INPUT_SERVICE, quick_input_service)
            .with(I_OPENER_SERVICE, opener_service)
            .with(I_STORAGE_SERVICE, storage),
    ));

    run_with_faked_timers(async move {
        let command = commands_registry().get_command("workbench.action.url.openUrl").expect("o comando precisa estar registrado");
        instantiation_service.invoke_function(|accessor| (command.handler)(accessor, &[]));
        timeout(0).await.unwrap();

        let opened = opened.opened.borrow();
        assert_eq!(opened.len(), 1);
        let (resource, options) = &opened[0];
        match resource {
            OpenResource::Text(text) => assert_eq!(text.to_string_lossy(), input),
            OpenResource::Uri(_) => panic!("o recurso deveria ser o texto da entrada"),
        }
        let options = options.as_ref().expect("as opções precisam ser passadas");
        assert_eq!(options.allow_contributed_openers, Some(AllowContributedOpeners::Bool(true)));
        assert!(options.open_to_side.is_none());
        assert!(options.editor_options.is_none());
        assert!(options.from_user_gesture.is_none());
        assert!(options.allow_commands.is_none());
        assert!(options.open_external.is_none());
        assert!(options.allow_tunneling.is_none());
        assert!(options.from_workspace.is_none());
        assert!(options.skip_validation.is_none());
    });
}
