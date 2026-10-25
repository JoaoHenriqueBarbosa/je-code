//! Porte de `src/vs/workbench/services/extensionManagement/test/browser/extensionEnablementService.test.ts` (suíte
//! `ExtensionEnablementService Test`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do [`Fixture`], criado antes de qualquer serviço; o
//!   `disposableStore` do original é `fixture.guard`. Cada caso monta o seu [`Fixture`] (o `setup`) e o descarta ao fim.
//! - `TestInstantiationService` (`stub`, `get`, `createInstance`) é o [`Fixture`]: ele guarda os serviços que o `setup`
//!   registra e que os casos trocam no meio (`stub(IWorkbenchEnvironmentService, { ... })`, `stub(IWorkspaceTrustManagementService,
//!   { isWorkspaceTrusted() ... })`...). Como no original, `new TestExtensionEnablementService(instantiationService)` usa o
//!   que estiver registrado e, para o que falta, registra um serviço novo que os demais construtores passam a compartilhar
//!   (o armazenamento, a confiança no workspace, o ciclo de vida...). O `stub(IWorkspaceContextService, 'getWorkbenchState',
//!   EMPTY)` é o `TestContextService::stub_workbench_state`, aplicado ao serviço que o objeto de teste já tem.
//! - `ExtensionManagementService` (o serviço real do workbench, que o original constrói por `createInstance`) é o
//!   [`TestExtensionManagementService`]: junta os eventos e as listas dos servidores de gerenciamento, como o real, sem
//!   a instalação, que o `ExtensionEnablementService` não usa. `getInstalled: () => Promise.resolve(installed)` devolve
//!   a lista viva, que o gerenciador só copia quando continua depois do `await`: o servidor local do dublê lê
//!   `installed` nesse momento, e não quando a promessa é pedida.
//! - `aLocalExtension2` devolve `Object.create({ manifest, ...properties })`: aqui é uma [`IExtension`] compartilhada
//!   (`Rc`), que o original compara por identidade. O manifesto passa por `ExtensionManifest::from_value`, que descarta o
//!   que tem o tipo errado: `aContributes('views' | 'viewsContainers' | 'menus')` põe `{}` nesses pontos (que são
//!   dicionários) e `[]` nos demais, e os dois valem como "o ponto existe".
//! - `sinon.spy()` é o [`Spy`], que guarda os argumentos de cada chamada; `target.calledOnce` é `calls() == 1`.
//! - Os testes `async` rodam em `spawn_local` com as microtarefas esvaziadas ([`run`]); os timers não rodam (o laço da
//!   lista de maliciosas espera cinco minutos). `assert.ok(value)` de um `Promise<boolean[]>` confere que a promessa
//!   resolveu com a lista.
//! - `UserDataSyncAccountService` passado como "serviço" (`stub(IUserDataSyncAccountService, UserDataSyncAccountService)`,
//!   a classe, sem `account`) é a conta vazia.
//! - **Não portados** (dependem de módulos que o porte ainda não tem): `test built-in npm tasks are parsed in
//!   editor window` e `... in sessions window` (os dois casos do `for (const isSessionsWindow of [false, true])`), que usam o
//!   `ContextKeyService`, o `parse` de `taskConfiguration`, o `filterEnabledExtensions` do `abstractExtensionService` e o
//!   `ExtensionsRegistry.acceptUsers`.

mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::network::schemas;
use je_base::runtime;
use je_base::lifecycle::IDisposable;
use je_extensions::management::InstallOperation;
use je_extensions::manifest::{EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY, ExtensionIdentifierInfo, ExtensionType};
use je_platform::configuration::service::IConfigurationService;
use je_platform::environment::environment::DisableExtensions;
use je_platform::resources::uri_file;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::workspace::{WorkbenchState, new_test_workspace, to_workspace_folder};
use je_storage::StorageValue;
use je_workbench::services::extension_management::extension_management::{
    DidChangeProfileEvent, DidUninstallExtensionEvent, EnablementState, ExtensionRef, IWorkbenchExtensionEnablementService, InstallExtensionResult,
    InstallExtensionSource, WorkspaceTypeOverrides, local_extension_uri,
};
use je_workbench::services::extension_management::service_contracts::{ChatEntitlementContext, ChatEntitlementContextState};
use je_workbench::services::extensions::extension_manifest_properties_service::EXTENSIONS_SUPPORT_AGENTS_WINDOW;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, product_service};
use je_workbench::test::common::workbench_test_services::TestChatEntitlementService;
use serde_json::json;
use support::extension_enablement_support::{
    Fixture, Properties, TestExtensionEnablementService, a_contributes, a_local_extension, a_local_extension2, a_local_extension_with, dynamic_of,
    an_extension_management_server_service, identifier, uri_with_scheme,
};

use EnablementState as E;

/// `sinon.spy()` passado a `onEnablementChanged`: guarda os argumentos de cada chamada.
struct Spy {
    calls: Rc<RefCell<Vec<Vec<ExtensionRef>>>>,
}

impl Spy {
    /// `disposableStore.add(testObject.onEnablementChanged(target))`.
    fn on(fixture: &Fixture, test_object: &TestExtensionEnablementService) -> Spy {
        let calls: Rc<RefCell<Vec<Vec<ExtensionRef>>>> = Rc::new(RefCell::new(Vec::new()));
        let recorded = calls.clone();
        fixture.guard.add(test_object.on_enablement_changed().listen(move |extensions| recorded.borrow_mut().push(extensions.clone())));
        Spy { calls }
    }

    fn count(&self) -> usize {
        self.calls.borrow().len()
    }

    /// `target.args[call][0]`: as extensões da chamada.
    fn args(&self, call: usize) -> Vec<ExtensionRef> {
        self.calls.borrow()[call].clone()
    }

    /// `(<IExtension>target.args[0][0][0]).identifier`.
    fn first_identifier(&self) -> ExtensionIdentifierInfo {
        self.args(0)[0].identifier.clone()
    }
}

/// Roda o caso: o `setup` (com o `testObject` padrão), o corpo, e o `teardown` (descartar o `disposableStore`).
fn run<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<Fixture>, Rc<TestExtensionEnablementService>) -> F) {
    let fixture = Fixture::new();
    let test_object = fixture.new_test_object(None);
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    let future = body(fixture.clone(), test_object);
    runtime::spawn_local(async move {
        future.await;
        finished.set(true);
    });
    runtime::run_microtasks();
    assert!(done.get(), "o teste assíncrono não terminou");
}

/// `await testObject.setEnablement(extensions, state)`.
async fn set(test_object: &TestExtensionEnablementService, extensions: &[&ExtensionRef], state: EnablementState) -> Vec<bool> {
    test_object.set_enablement(extensions.iter().map(|extension| (*extension).clone()).collect(), state).await.expect("setEnablement resolve")
}

/// `await testObject.waitUntilInitialized()`.
async fn wait_until_initialized(test_object: &TestExtensionEnablementService) {
    test_object.wait_until_initialized().await.expect("waitUntilInitialized resolve");
}

/// `instantiationService.stub(IWorkspaceContextService, 'getWorkspace', { folders: [{ uri: URI.file('worskapceA').with({ scheme }) }] })`.
fn stub_workspace_folder(fixture: &Fixture, scheme: Option<&str>) {
    let uri = scheme.map_or_else(|| uri_file("worskapceA"), |scheme| uri_with_scheme("worskapceA", scheme));
    fixture.context.set_workspace(new_test_workspace("test-workspace", vec![to_workspace_folder(&uri, "worskapceA")], None, None));
}

// ---------------------------------------------------------------------------------------------------------------------
// Os casos da suíte `ExtensionEnablementService Test`.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn test_disable_an_extension_globally() {
    run(|_, t| async move {
        let extension = a_local_extension("pub.a");
        set(&t, &[&extension], E::DisabledGlobally).await;
        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_an_extension_globally_should_return_truthy_promise() {
    run(|_, t| async move {
        let value = set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert_eq!(value.len(), 1);
    });
}

#[test]
fn test_disable_an_extension_globally_triggers_the_change_event() {
    run(|f, t| async move {
        let target = Spy::on(&f, &t);
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert_eq!(target.count(), 1);
        assert_eq!(target.first_identifier(), identifier("pub.a"));
    });
}

#[test]
fn test_disable_an_extension_globally_again_should_return_a_falsy_promise() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        let value = set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert!(!value[0]);
    });
}

#[test]
fn test_state_of_globally_disabled_extension() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::DisabledGlobally);
    });
}

#[test]
fn test_state_of_globally_enabled_extension() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        set(&t, &[&a_local_extension("pub.a")], E::EnabledGlobally).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::EnabledGlobally);
    });
}

#[test]
fn test_disable_an_extension_for_workspace() {
    run(|_, t| async move {
        let extension = a_local_extension("pub.a");
        set(&t, &[&extension], E::DisabledWorkspace).await;
        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledWorkspace);
    });
}

#[test]
fn test_disable_an_extension_for_workspace_returns_a_truthy_promise() {
    run(|_, t| async move {
        let value = set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert_eq!(value.len(), 1);
    });
}

#[test]
fn test_disable_an_extension_for_workspace_again_should_return_a_falsy_promise() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        let value = set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert!(!value[0]);
    });
}

#[test]
fn test_state_of_workspace_disabled_extension() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::DisabledWorkspace);
    });
}

#[test]
fn test_state_of_workspace_and_globally_disabled_extension() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::DisabledWorkspace);
    });
}

#[test]
fn test_state_of_workspace_enabled_extension() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::EnabledWorkspace).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::EnabledWorkspace);
    });
}

#[test]
fn test_state_of_globally_disabled_and_workspace_enabled_extension() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::EnabledWorkspace).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::EnabledWorkspace);
    });
}

#[test]
fn test_state_of_an_extension_when_disabled_for_workspace_from_workspace_enabled() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::EnabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::DisabledWorkspace);
    });
}

#[test]
fn test_state_of_an_extension_when_disabled_globally_from_workspace_enabled() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::EnabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::DisabledGlobally);
    });
}

#[test]
fn test_state_of_an_extension_when_disabled_globally_from_workspace_disabled() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::DisabledGlobally);
    });
}

#[test]
fn test_state_of_an_extension_when_enabled_globally_from_workspace_enabled() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::EnabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::EnabledGlobally).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::EnabledGlobally);
    });
}

#[test]
fn test_state_of_an_extension_when_enabled_globally_from_workspace_disabled() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.a")], E::EnabledGlobally).await;
        assert_eq!(t.get_enablement_state(&a_local_extension("pub.a")), E::EnabledGlobally);
    });
}

#[test]
fn test_disable_an_extension_for_workspace_and_then_globally() {
    run(|_, t| async move {
        let extension = a_local_extension("pub.a");
        set(&t, &[&extension], E::DisabledWorkspace).await;
        set(&t, &[&extension], E::DisabledGlobally).await;
        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_an_extension_for_workspace_and_then_globally_return_a_truthy_promise() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        let value = set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert_eq!(value.len(), 1);
    });
}

#[test]
fn test_disable_an_extension_for_workspace_and_then_globally_trigger_the_change_event() {
    run(|f, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        let target = Spy::on(&f, &t);
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert_eq!(target.count(), 1);
        assert_eq!(target.first_identifier(), identifier("pub.a"));
    });
}

#[test]
fn test_disable_an_extension_globally_and_then_for_workspace() {
    run(|_, t| async move {
        let extension = a_local_extension("pub.a");
        set(&t, &[&extension], E::DisabledGlobally).await;
        set(&t, &[&extension], E::DisabledWorkspace).await;
        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledWorkspace);
    });
}

#[test]
fn test_disable_an_extension_globally_and_then_for_workspace_return_a_truthy_promise() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        let value = set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert_eq!(value.len(), 1);
    });
}

#[test]
fn test_disable_an_extension_globally_and_then_for_workspace_triggers_the_change_event() {
    run(|f, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        let target = Spy::on(&f, &t);
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert_eq!(target.count(), 1);
        assert_eq!(target.first_identifier(), identifier("pub.a"));
    });
}

#[test]
fn test_disable_an_extension_for_workspace_when_there_is_no_workspace_throws_error() {
    run(|f, t| async move {
        f.context.stub_workbench_state(Some(WorkbenchState::Empty));
        let result = t.set_enablement(vec![a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert!(result.is_err(), "should throw an error");
    });
}

#[test]
fn test_enable_an_extension_globally() {
    run(|_, t| async move {
        let extension = a_local_extension("pub.a");
        set(&t, &[&extension], E::DisabledGlobally).await;
        set(&t, &[&extension], E::EnabledGlobally).await;
        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_enable_an_extension_globally_return_truthy_promise() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        let value = set(&t, &[&a_local_extension("pub.a")], E::EnabledGlobally).await;
        assert!(value[0]);
    });
}

#[test]
fn test_enable_an_extension_globally_triggers_change_event() {
    run(|f, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        let target = Spy::on(&f, &t);
        set(&t, &[&a_local_extension("pub.a")], E::EnabledGlobally).await;
        assert_eq!(target.count(), 1);
        assert_eq!(target.first_identifier(), identifier("pub.a"));
    });
}

#[test]
fn test_enable_an_extension_globally_when_already_enabled_return_falsy_promise() {
    run(|_, t| async move {
        let value = set(&t, &[&a_local_extension("pub.a")], E::EnabledGlobally).await;
        assert!(!value[0]);
    });
}

#[test]
fn test_enable_an_extension_for_workspace() {
    run(|_, t| async move {
        let extension = a_local_extension("pub.a");
        set(&t, &[&extension], E::DisabledWorkspace).await;
        set(&t, &[&extension], E::EnabledWorkspace).await;
        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledWorkspace);
    });
}

#[test]
fn test_enable_an_extension_for_workspace_return_truthy_promise() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        let value = set(&t, &[&a_local_extension("pub.a")], E::EnabledWorkspace).await;
        assert_eq!(value.len(), 1);
    });
}

#[test]
fn test_enable_an_extension_for_workspace_triggers_change_event() {
    run(|f, t| async move {
        set(&t, &[&a_local_extension("pub.b")], E::DisabledWorkspace).await;
        let target = Spy::on(&f, &t);
        set(&t, &[&a_local_extension("pub.b")], E::EnabledWorkspace).await;
        assert_eq!(target.count(), 1);
        assert_eq!(target.first_identifier(), identifier("pub.b"));
    });
}

#[test]
fn test_enable_an_extension_for_workspace_when_already_enabled_return_truthy_promise() {
    run(|_, t| async move {
        let value = set(&t, &[&a_local_extension("pub.a")], E::EnabledWorkspace).await;
        assert_eq!(value.len(), 1);
    });
}

#[test]
fn test_enable_an_extension_for_workspace_when_disabled_in_workspace_and_gloablly() {
    run(|_, t| async move {
        let extension = a_local_extension("pub.a");
        set(&t, &[&extension], E::DisabledWorkspace).await;
        set(&t, &[&extension], E::DisabledGlobally).await;
        set(&t, &[&extension], E::EnabledWorkspace).await;
        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledWorkspace);
    });
}

#[test]
fn test_enable_an_extension_globally_when_disabled_in_workspace_and_gloablly() {
    run(|_, t| async move {
        let extension = a_local_extension("pub.a");
        set(&t, &[&extension], E::EnabledWorkspace).await;
        set(&t, &[&extension], E::DisabledWorkspace).await;
        set(&t, &[&extension], E::DisabledGlobally).await;
        set(&t, &[&extension], E::EnabledGlobally).await;
        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_enable_an_extension_also_enables_dependencies() {
    run(|f, t| async move {
        f.installed.borrow_mut().extend([a_local_extension2("pub.a", json!({ "extensionDependencies": ["pub.b"] }), Properties::default()), a_local_extension("pub.b")]);
        let target = f.installed.borrow()[0].clone();
        let dep = f.installed.borrow()[1].clone();
        wait_until_initialized(&t).await;
        set(&t, &[&dep, &target], E::DisabledGlobally).await;
        set(&t, &[&target], E::EnabledGlobally).await;
        assert!(t.is_enabled(&target));
        assert!(t.is_enabled(&dep));
        assert_eq!(t.get_enablement_state(&target), E::EnabledGlobally);
        assert_eq!(t.get_enablement_state(&dep), E::EnabledGlobally);
    });
}

#[test]
fn test_enable_an_extension_in_workspace_with_a_dependency_extension_that_has_auth_providers() {
    run(|f, t| async move {
        f.installed.borrow_mut().extend([
            a_local_extension2("pub.a", json!({ "extensionDependencies": ["pub.b"] }), Properties::default()),
            a_local_extension_with("pub.b", Some(json!({ "authentication": [{ "id": "a", "label": "a" }] })), None),
        ]);
        let target = f.installed.borrow()[0].clone();
        wait_until_initialized(&t).await;
        set(&t, &[&target], E::DisabledWorkspace).await;
        set(&t, &[&target], E::EnabledWorkspace).await;
        assert!(t.is_enabled(&target));
        assert_eq!(t.get_enablement_state(&target), E::EnabledWorkspace);
    });
}

#[test]
fn test_enable_an_extension_with_a_dependency_extension_that_cannot_be_enabled() {
    run(|f, _| async move {
        f.stub_server_service(an_extension_management_server_service(Some(f.an_extension_management_server("vscode-local")), Some(f.an_extension_management_server("vscode-remote")), None));
        let local_workspace_dep_extension = a_local_extension2("pub.b", json!({ "extensionKind": ["workspace"] }), Properties::at(uri_file("pub.b")));
        let remote_workspace_extension =
            a_local_extension2("pub.a", json!({ "extensionKind": ["workspace"], "extensionDependencies": ["pub.b"] }), Properties::at(uri_with_scheme("pub.a", "vscode-remote")));
        let remote_workspace_dep_extension = a_local_extension2("pub.b", json!({ "extensionKind": ["workspace"] }), Properties::at(uri_with_scheme("pub.b", "vscode-remote")));
        f.installed.borrow_mut().extend([local_workspace_dep_extension, remote_workspace_extension.clone(), remote_workspace_dep_extension]);

        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        set(&t, &[&remote_workspace_extension], E::DisabledGlobally).await;
        set(&t, &[&remote_workspace_extension], E::EnabledGlobally).await;
        assert!(t.is_enabled(&remote_workspace_extension));
        assert_eq!(t.get_enablement_state(&remote_workspace_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_enable_an_extension_also_enables_packed_extensions() {
    run(|f, t| async move {
        f.installed.borrow_mut().extend([a_local_extension2("pub.a", json!({ "extensionPack": ["pub.b"] }), Properties::default()), a_local_extension("pub.b")]);
        let target = f.installed.borrow()[0].clone();
        let dep = f.installed.borrow()[1].clone();
        set(&t, &[&dep, &target], E::DisabledGlobally).await;
        set(&t, &[&target], E::EnabledGlobally).await;
        assert!(t.is_enabled(&target));
        assert!(t.is_enabled(&dep));
        assert_eq!(t.get_enablement_state(&target), E::EnabledGlobally);
        assert_eq!(t.get_enablement_state(&dep), E::EnabledGlobally);
    });
}

#[test]
fn test_remove_an_extension_from_disablement_list_when_uninstalled() {
    run(|f, _| async move {
        let extension = a_local_extension("pub.a");
        f.installed.borrow_mut().push(extension.clone());
        let t = f.new_test_object(None);

        set(&t, &[&extension], E::DisabledWorkspace).await;
        set(&t, &[&extension], E::DisabledGlobally).await;
        f.did_uninstall_event.fire(DidUninstallExtensionEvent { identifier: identifier("pub.a"), profile_location: None, error: None, workspace_scoped: None, server: None });

        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_is_enabled_return_false_extension_is_disabled_globally() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledGlobally).await;
        assert!(!t.is_enabled(&a_local_extension("pub.a")));
    });
}

#[test]
fn test_is_enabled_return_false_extension_is_disabled_in_workspace() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        assert!(!t.is_enabled(&a_local_extension("pub.a")));
    });
}

#[test]
fn test_is_enabled_return_true_extension_is_not_disabled() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension("pub.a")], E::DisabledWorkspace).await;
        set(&t, &[&a_local_extension("pub.c")], E::DisabledGlobally).await;
        assert!(t.is_enabled(&a_local_extension("pub.b")));
    });
}

#[test]
fn test_can_change_enablement_return_false_for_language_packs() {
    run(|_, t| async move {
        let extension = a_local_extension_with("pub.a", Some(json!({ "localizations": [{ "languageId": "gr", "translations": [{ "id": "vscode", "path": "path" }] }] })), None);
        assert!(!t.can_change_enablement(&extension));
    });
}

#[test]
fn test_can_change_enablement_return_true_for_auth_extension() {
    run(|_, t| async move {
        let extension = a_local_extension_with("pub.a", Some(json!({ "authentication": [{ "id": "a", "label": "a" }] })), None);
        assert!(t.can_change_enablement(&extension));
    });
}

#[test]
fn test_can_change_enablement_return_true_for_auth_extension_when_user_data_sync_account_does_not_depends_on_it() {
    run(|f, _| async move {
        f.stub_user_data_sync_account("b");
        let t = f.new_test_object(None);
        let extension = a_local_extension_with("pub.a", Some(json!({ "authentication": [{ "id": "a", "label": "a" }] })), None);
        assert!(t.can_change_enablement(&extension));
    });
}

#[test]
fn test_can_change_enablement_return_true_for_auth_extension_when_user_data_sync_account_depends_on_it_but_auto_sync_is_off() {
    run(|f, _| async move {
        f.stub_user_data_sync_account("a");
        let t = f.new_test_object(None);
        let extension = a_local_extension_with("pub.a", Some(json!({ "authentication": [{ "id": "a", "label": "a" }] })), None);
        assert!(t.can_change_enablement(&extension));
    });
}

#[test]
fn test_can_change_enablement_return_false_for_auth_extension_and_user_data_sync_account_depends_on_it_and_auto_sync_is_on() {
    run(|f, _| async move {
        f.stub_user_data_sync_enabled(true);
        f.stub_user_data_sync_account("a");
        let t = f.new_test_object(None);
        let extension = a_local_extension_with("pub.a", Some(json!({ "authentication": [{ "id": "a", "label": "a" }] })), None);
        assert!(!t.can_change_enablement(&extension));
    });
}

#[test]
fn test_can_change_workspace_enablement_return_true() {
    run(|_, t| async move {
        assert!(t.can_change_workspace_enablement(&a_local_extension("pub.a")));
    });
}

#[test]
fn test_can_change_workspace_enablement_return_false_if_there_is_no_workspace() {
    run(|f, t| async move {
        f.context.stub_workbench_state(Some(WorkbenchState::Empty));
        assert!(!t.can_change_workspace_enablement(&a_local_extension("pub.a")));
    });
}

#[test]
fn test_can_change_workspace_enablement_return_true_for_auth_extension_when_it_is_not_used_by_settings_sync() {
    run(|_, t| async move {
        let extension = a_local_extension_with("pub.a", Some(json!({ "authentication": [{ "id": "a", "label": "a" }] })), None);
        assert!(t.can_change_workspace_enablement(&extension));
    });
}

#[test]
fn test_can_change_workspace_enablement_return_false_for_auth_extension_when_settings_sync_depends_on_it() {
    run(|f, _| async move {
        f.stub_user_data_sync_enabled(true);
        f.stub_user_data_sync_account("a");
        let t = f.new_test_object(None);
        let extension = a_local_extension_with("pub.a", Some(json!({ "authentication": [{ "id": "a", "label": "a" }] })), None);
        assert!(!t.can_change_workspace_enablement(&extension));
    });
}

#[test]
fn test_can_change_enablement_return_false_when_extensions_are_disabled_in_environment() {
    run(|f, _| async move {
        f.stub_environment(EnvironmentOverrides { disable_extensions: Some(DisableExtensions::All(true)), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);
        assert!(!t.can_change_enablement(&a_local_extension("pub.a")));
    });
}

#[test]
fn test_can_change_enablement_return_false_when_the_extension_is_disabled_in_environment() {
    run(|f, _| async move {
        f.stub_environment(EnvironmentOverrides { disable_extensions: Some(DisableExtensions::Some(vec!["pub.a".to_owned()])), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);
        assert!(!t.can_change_enablement(&a_local_extension("pub.a")));
    });
}

#[test]
fn test_can_change_enablement_return_true_for_system_extensions_when_extensions_are_disabled_in_environment() {
    run(|f, _| async move {
        f.stub_environment(EnvironmentOverrides { disable_extensions: Some(DisableExtensions::All(true)), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);
        let extension = a_local_extension_with("pub.a", None, Some(ExtensionType::System));
        assert!(t.can_change_enablement(&extension));
    });
}

#[test]
fn test_can_change_enablement_return_false_for_system_extension_when_extension_is_disabled_in_environment() {
    run(|f, _| async move {
        f.stub_environment(EnvironmentOverrides { disable_extensions: Some(DisableExtensions::Some(vec!["pub.a".to_owned()])), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);
        let extension = a_local_extension_with("pub.a", None, Some(ExtensionType::System));
        assert!(!t.can_change_enablement(&extension));
    });
}

#[test]
fn test_extension_is_disabled_when_disabled_in_environment() {
    run(|f, _| async move {
        let extension = a_local_extension("pub.a");
        f.installed.borrow_mut().push(extension.clone());

        f.stub_environment(EnvironmentOverrides { disable_extensions: Some(DisableExtensions::Some(vec!["pub.a".to_owned()])), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);

        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledByEnvironment);
    });
}

#[test]
fn test_extension_is_enabled_globally_when_enabled_in_environment() {
    run(|f, _| async move {
        let extension = a_local_extension("pub.a");
        f.installed.borrow_mut().push(extension.clone());

        f.stub_environment(EnvironmentOverrides { enable_extensions: Some(vec!["pub.a".to_owned()]), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);

        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_is_enabled_workspace_when_enabled_in_environment() {
    run(|f, t| async move {
        let extension = a_local_extension("pub.a");
        f.installed.borrow_mut().push(extension.clone());

        set(&t, &[&extension], E::EnabledWorkspace).await;
        f.stub_environment(EnvironmentOverrides { enable_extensions: Some(vec!["pub.a".to_owned()]), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);

        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledWorkspace);
    });
}

#[test]
fn test_extension_is_enabled_by_environment_when_disabled_globally() {
    run(|f, t| async move {
        let extension = a_local_extension("pub.a");
        f.installed.borrow_mut().push(extension.clone());

        set(&t, &[&extension], E::DisabledGlobally).await;
        f.stub_environment(EnvironmentOverrides { enable_extensions: Some(vec!["pub.a".to_owned()]), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);

        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledByEnvironment);
    });
}

#[test]
fn test_extension_is_enabled_by_environment_when_disabled_workspace() {
    run(|f, t| async move {
        let extension = a_local_extension("pub.a");
        f.installed.borrow_mut().push(extension.clone());

        set(&t, &[&extension], E::DisabledWorkspace).await;
        f.stub_environment(EnvironmentOverrides { enable_extensions: Some(vec!["pub.a".to_owned()]), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);

        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledByEnvironment);
    });
}

#[test]
fn test_extension_is_disabled_by_environment_when_also_enabled_in_environment() {
    run(|f, t| async move {
        let extension = a_local_extension("pub.a");
        f.installed.borrow_mut().push(extension.clone());

        // Sem o `await`, como no original: o `setEnablement` fica pendente e o serviço novo já nasce sobre o que ele gravar.
        let _ = t.set_enablement(vec![extension.clone()], E::DisabledWorkspace);
        f.stub_environment(EnvironmentOverrides {
            disable_extensions: Some(DisableExtensions::All(true)),
            enable_extensions: Some(vec!["pub.a".to_owned()]),
            ..EnvironmentOverrides::default()
        });
        let t = f.new_test_object(None);

        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledByEnvironment);
    });
}

#[test]
fn test_can_change_enablement_return_false_when_the_extension_is_enabled_in_environment() {
    run(|f, _| async move {
        f.stub_environment(EnvironmentOverrides { enable_extensions: Some(vec!["pub.a".to_owned()]), ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);
        assert!(!t.can_change_enablement(&a_local_extension("pub.a")));
    });
}

#[test]
fn test_extension_does_not_support_vitrual_workspace_is_not_enabled_in_virtual_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "capabilities": { "virtualWorkspaces": false } }), Properties::default());
        stub_workspace_folder(&f, Some("virtual"));
        let t = f.new_test_object(None);
        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledByVirtualWorkspace);
    });
}

#[test]
fn test_web_extension_from_web_extension_management_server_and_does_not_support_vitrual_workspace_is_enabled_in_virtual_workspace() {
    run(|f, _| async move {
        f.stub_server_service(an_extension_management_server_service(None, Some(f.an_extension_management_server("vscode-remote")), Some(f.an_extension_management_server("web"))));
        let extension = a_local_extension2("pub.a", json!({ "capabilities": { "virtualWorkspaces": false }, "browser": "browser.js" }), Properties::at(uri_with_scheme("pub.a", "web")));
        stub_workspace_folder(&f, Some("virtual"));
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_web_extension_from_remote_extension_management_server_and_does_not_support_vitrual_workspace_is_disabled_in_virtual_workspace() {
    run(|f, _| async move {
        f.stub_server_service(an_extension_management_server_service(None, Some(f.an_extension_management_server("vscode-remote")), Some(f.an_extension_management_server("web"))));
        let extension =
            a_local_extension2("pub.a", json!({ "capabilities": { "virtualWorkspaces": false }, "browser": "browser.js" }), Properties::at(uri_with_scheme("pub.a", "vscode-remote")));
        stub_workspace_folder(&f, Some("virtual"));
        let t = f.new_test_object(None);
        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledByVirtualWorkspace);
    });
}

#[test]
fn test_enable_a_remote_workspace_extension_and_local_ui_extension_that_is_a_dependency_of_remote() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_ui_extension = a_local_extension2("pub.a", json!({ "main": "main.js", "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let remote_ui_extension = a_local_extension2("pub.a", json!({ "main": "main.js", "extensionKind": ["ui"] }), Properties::at(uri_with_scheme("pub.a", "vscode-remote")));
        let target = a_local_extension2("pub.b", json!({ "main": "main.js", "extensionDependencies": ["pub.a"] }), Properties::at(uri_with_scheme("pub.b", "vscode-remote")));
        let t = f.new_test_object(None);

        f.installed.borrow_mut().extend([local_ui_extension.clone(), remote_ui_extension, target.clone()]);
        set(&t, &[&target, &local_ui_extension], E::DisabledGlobally).await;
        set(&t, &[&target, &local_ui_extension], E::EnabledGlobally).await;
        assert!(t.is_enabled(&target));
        assert!(t.is_enabled(&local_ui_extension));
        assert_eq!(t.get_enablement_state(&target), E::EnabledGlobally);
        assert_eq!(t.get_enablement_state(&local_ui_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_enable_a_remote_workspace_extension_also_enables_its_dependency_in_local() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_ui_extension = a_local_extension2("pub.a", json!({ "main": "main.js", "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let remote_ui_extension = a_local_extension2("pub.a", json!({ "main": "main.js", "extensionKind": ["ui"] }), Properties::at(uri_with_scheme("pub.a", "vscode-remote")));
        let target = a_local_extension2("pub.b", json!({ "main": "main.js", "extensionDependencies": ["pub.a"] }), Properties::at(uri_with_scheme("pub.b", "vscode-remote")));
        let t = f.new_test_object(None);

        f.installed.borrow_mut().extend([local_ui_extension.clone(), remote_ui_extension, target.clone()]);
        set(&t, &[&target, &local_ui_extension], E::DisabledGlobally).await;
        set(&t, &[&target], E::EnabledGlobally).await;
        assert!(t.is_enabled(&target));
        assert!(t.is_enabled(&local_ui_extension));
        assert_eq!(t.get_enablement_state(&target), E::EnabledGlobally);
        assert_eq!(t.get_enablement_state(&local_ui_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_can_change_enablement_return_false_when_extension_is_disabled_in_virtual_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "capabilities": { "virtualWorkspaces": false } }), Properties::default());
        stub_workspace_folder(&f, Some("virtual"));
        let t = f.new_test_object(None);
        assert!(!t.can_change_enablement(&extension));
    });
}

#[test]
fn test_extension_does_not_support_vitrual_workspace_is_enabled_in_normal_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "capabilities": { "virtualWorkspaces": false } }), Properties::default());
        stub_workspace_folder(&f, None);
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_supports_virtual_workspace_is_enabled_in_virtual_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "capabilities": { "virtualWorkspaces": true } }), Properties::default());
        stub_workspace_folder(&f, Some("virtual"));
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_does_not_support_untrusted_workspaces_is_disabled_in_untrusted_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "hello" } } }), Properties::default());
        f.stub_workspace_trusted(false);
        let t = f.new_test_object(None);
        assert_eq!(t.get_enablement_state(&extension), E::DisabledByTrustRequirement);
    });
}

#[test]
fn test_can_change_enablement_return_true_when_extension_is_disabled_by_workspace_trust() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "hello" } } }), Properties::default());
        f.stub_workspace_trusted(false);
        let t = f.new_test_object(None);
        assert!(t.can_change_enablement(&extension));
    });
}

#[test]
fn test_extension_supports_untrusted_workspaces_is_enabled_in_untrusted_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": true } } }), Properties::default());
        f.stub_workspace_trusted(false);
        let t = f.new_test_object(None);
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_does_not_support_untrusted_workspaces_is_enabled_in_trusted_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "" } } }), Properties::default());
        f.stub_workspace_trusted(true);
        let t = f.new_test_object(None);
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_supports_untrusted_workspaces_is_enabled_in_trusted_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": true } } }), Properties::default());
        f.stub_workspace_trusted(true);
        let t = f.new_test_object(None);
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_without_any_value_for_virtual_worksapce_is_enabled_in_virtual_workspace() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({}), Properties::default());
        stub_workspace_folder(&f, Some("virtual"));
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
    });
}

#[test]
fn test_local_workspace_extension_is_disabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["workspace"] }), Properties::at(uri_file("pub.a")));
        let t = f.new_test_object(None);
        assert!(!t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::DisabledByExtensionKind);
    });
}

#[test]
fn test_local_workspace_ui_extension_is_enabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["workspace", "ui"] }), Properties::at(uri_file("pub.a")));
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_local_ui_extension_is_not_disabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_can_change_enablement_return_true_when_the_local_workspace_extension_is_disabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["workspace"] }), Properties::at(uri_file("pub.a")));
        let t = f.new_test_object(None);
        assert!(!t.can_change_enablement(&local_workspace_extension));
    });
}

#[test]
fn test_can_change_enablement_return_true_for_local_ui_extension() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let t = f.new_test_object(None);
        assert!(t.can_change_enablement(&local_workspace_extension));
    });
}

#[test]
fn test_remote_ui_extension_is_disabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["ui"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        let t = f.new_test_object(None);
        assert!(!t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::DisabledByExtensionKind);
    });
}

#[test]
fn test_remote_ui_workspace_extension_is_disabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["ui", "workspace"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_remote_ui_extension_is_disabled_by_kind_when_there_is_no_local_server() {
    run(|f, _| async move {
        f.stub_server_service(an_extension_management_server_service(None, Some(f.an_extension_management_server("vscode-remote")), None));
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["ui"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        let t = f.new_test_object(None);
        assert!(!t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::DisabledByExtensionKind);
    });
}

#[test]
fn test_remote_workspace_extension_is_not_disabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["workspace"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_can_change_enablement_return_true_when_the_remote_ui_extension_is_disabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["ui"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        let t = f.new_test_object(None);
        assert!(!t.can_change_enablement(&local_workspace_extension));
    });
}

#[test]
fn test_can_change_enablement_return_true_for_remote_workspace_extension() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["workspace"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        let t = f.new_test_object(None);
        assert!(t.can_change_enablement(&local_workspace_extension));
    });
}

#[test]
fn test_web_extension_on_local_server_is_disabled_by_kind_when_web_worker_is_not_enabled() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "browser": "browser.js" }), Properties::at(uri_file("pub.a")));
        f.configuration.set_user_configuration("extensions", dynamic_of(&json!({ "webWorker": false })), None);
        let t = f.new_test_object(None);
        assert!(!t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::DisabledByExtensionKind);
    });
}

#[test]
fn test_web_extension_on_local_server_is_not_disabled_by_kind_when_web_worker_is_enabled() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "browser": "browser.js" }), Properties::at(uri_file("pub.a")));
        f.configuration.set_user_configuration("extensions", dynamic_of(&json!({ "webWorker": true })), None);
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_web_extension_on_remote_server_is_disabled_by_kind_when_web_worker_is_not_enabled() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "browser": "browser.js" }), Properties::at(uri_with_scheme("pub.a", "vscode-remote")));
        f.configuration.set_user_configuration("extensions", dynamic_of(&json!({ "webWorker": false })), None);
        let t = f.new_test_object(None);
        assert!(!t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::DisabledByExtensionKind);
    });
}

#[test]
fn test_web_extension_on_remote_server_is_disabled_by_kind_when_web_worker_is_enabled() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "browser": "browser.js" }), Properties::at(uri_with_scheme("pub.a", "vscode-remote")));
        f.configuration.set_user_configuration("extensions", dynamic_of(&json!({ "webWorker": true })), None);
        let t = f.new_test_object(None);
        assert!(!t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::DisabledByExtensionKind);
    });
}

#[test]
fn test_web_extension_on_remote_server_is_enabled_in_web() {
    run(|f, _| async move {
        f.stub_server_service(an_extension_management_server_service(
            Some(f.an_extension_management_server("vscode-local")),
            Some(f.an_extension_management_server("vscode-remote")),
            Some(f.an_extension_management_server("web")),
        ));
        let local_workspace_extension = a_local_extension2("pub.a", json!({ "browser": "browser.js" }), Properties::at(uri_with_scheme("pub.a", "vscode-remote")));
        f.configuration.set_user_configuration("extensions", dynamic_of(&json!({ "webWorker": false })), None);
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&local_workspace_extension));
        assert_eq!(t.get_enablement_state(&local_workspace_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_web_extension_on_web_server_is_not_disabled_by_kind() {
    run(|f, _| async move {
        f.stub_server_service(an_extension_management_server_service(
            Some(f.an_extension_management_server("vscode-local")),
            Some(f.an_extension_management_server("vscode-remote")),
            Some(f.an_extension_management_server("web")),
        ));
        let web_extension = a_local_extension2("pub.a", json!({ "browser": "browser.js" }), Properties::at(uri_with_scheme("pub.a", "web")));
        let t = f.new_test_object(None);
        assert!(t.is_enabled(&web_extension));
        assert_eq!(t.get_enablement_state(&web_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_state_of_multipe_extensions() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([a_local_extension("pub.a"), a_local_extension("pub.b"), a_local_extension("pub.c"), a_local_extension("pub.d"), a_local_extension("pub.e")]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        set(&t, &[&installed[0]], E::DisabledGlobally).await;
        set(&t, &[&installed[1]], E::DisabledWorkspace).await;
        set(&t, &[&installed[2]], E::EnabledWorkspace).await;
        set(&t, &[&installed[3]], E::EnabledGlobally).await;

        assert_eq!(
            t.get_enablement_states(&installed, WorkspaceTypeOverrides::default()),
            vec![E::DisabledGlobally, E::DisabledWorkspace, E::EnabledWorkspace, E::EnabledGlobally, E::EnabledGlobally]
        );
    });
}

#[test]
fn test_extension_is_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([a_local_extension2("pub.a", json!({}), Properties::default()), a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"] }), Properties::default())]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        set(&t, &[&installed[0]], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_state(&installed[1]), E::DisabledByExtensionDependency);
    });
}

#[test]
fn test_extension_is_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled_by_virtual_workspace() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([
            a_local_extension2("pub.a", json!({ "capabilities": { "virtualWorkspaces": false } }), Properties::default()),
            a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"], "capabilities": { "virtualWorkspaces": true } }), Properties::default()),
        ]);
        stub_workspace_folder(&f, Some("virtual"));
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        assert_eq!(t.get_enablement_state(&installed[0]), E::DisabledByVirtualWorkspace);
        assert_eq!(t.get_enablement_state(&installed[1]), E::DisabledByExtensionDependency);
    });
}

#[test]
fn test_can_change_enablement_return_false_when_extension_is_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled_by_virtual_workspace() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([
            a_local_extension2("pub.a", json!({ "capabilities": { "virtualWorkspaces": false } }), Properties::default()),
            a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"], "capabilities": { "virtualWorkspaces": true } }), Properties::default()),
        ]);
        stub_workspace_folder(&f, Some("virtual"));
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        assert!(!t.can_change_enablement(&installed[1]));
    });
}

#[test]
fn test_extension_is_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled_by_workspace_trust() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([
            a_local_extension2("pub.a", json!({ "main": "hello.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "" } } }), Properties::default()),
            a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"], "capabilities": { "untrustedWorkspaces": { "supported": true } } }), Properties::default()),
        ]);
        f.stub_workspace_trusted(false);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        assert_eq!(t.get_enablement_state(&installed[0]), E::DisabledByTrustRequirement);
        assert_eq!(t.get_enablement_state(&installed[1]), E::DisabledByExtensionDependency);
    });
}

#[test]
fn test_extension_is_not_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled_by_extension_kind() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let local_ui_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let remote_ui_extension = a_local_extension2("pub.a", json!({ "extensionKind": ["ui"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        let remote_workspace_extension =
            a_local_extension2("pub.n", json!({ "extensionKind": ["workspace"], "extensionDependencies": ["pub.a"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        f.installed.borrow_mut().extend([local_ui_extension.clone(), remote_ui_extension.clone(), remote_workspace_extension.clone()]);

        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        assert_eq!(t.get_enablement_state(&local_ui_extension), E::EnabledGlobally);
        assert_eq!(t.get_enablement_state(&remote_ui_extension), E::DisabledByExtensionKind);
        assert_eq!(t.get_enablement_state(&remote_workspace_extension), E::EnabledGlobally);
    });
}

#[test]
fn test_can_change_enablement_return_false_when_extension_is_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled_by_workspace_trust() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([
            a_local_extension2("pub.a", json!({ "main": "hello.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "" } } }), Properties::default()),
            a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"], "capabilities": { "untrustedWorkspaces": { "supported": true } } }), Properties::default()),
        ]);
        f.stub_workspace_trusted(false);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        assert!(!t.can_change_enablement(&installed[1]));
    });
}

#[test]
fn test_can_change_enablement_return_false_when_extension_is_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled_globally() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([a_local_extension2("pub.a", json!({}), Properties::default()), a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"] }), Properties::default())]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        set(&t, &[&installed[0]], E::DisabledGlobally).await;

        assert!(!t.can_change_enablement(&installed[1]));
    });
}

#[test]
fn test_can_change_enablement_return_false_when_extension_is_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled_workspace() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([a_local_extension2("pub.a", json!({}), Properties::default()), a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"] }), Properties::default())]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        set(&t, &[&installed[0]], E::DisabledWorkspace).await;

        assert!(!t.can_change_enablement(&installed[1]));
    });
}

#[test]
fn test_extension_is_not_disabled_by_dependency_even_if_it_has_a_dependency_that_is_disabled_when_installed_extensions_are_not_set() {
    run(|_, t| async move {
        set(&t, &[&a_local_extension2("pub.a", json!({}), Properties::default())], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_state(&a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"] }), Properties::default())), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_is_disabled_by_dependency_if_it_has_a_dependency_that_is_disabled_when_all_extensions_are_passed() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([a_local_extension2("pub.a", json!({}), Properties::default()), a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"] }), Properties::default())]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        let installed = f.installed.borrow().clone();
        set(&t, &[&installed[0]], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_states(&installed, WorkspaceTypeOverrides::default()), vec![E::DisabledGlobally, E::DisabledByExtensionDependency]);
    });
}

#[test]
fn test_extension_is_not_disabled_when_it_has_a_missing_dependency() {
    run(|f, _| async move {
        let target = a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"] }), Properties::default());
        f.installed.borrow_mut().push(target.clone());
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        assert_eq!(t.get_enablement_state(&target), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_is_not_disabled_when_it_has_a_dependency_in_another_server() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let target = a_local_extension2("pub.a", json!({ "extensionDependencies": ["pub.b"], "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let depdency_on_another_server = a_local_extension2("pub.b", json!({}), Properties::at(uri_with_scheme("pub.b", schemas::VSCODE_REMOTE)));
        f.installed.borrow_mut().extend([target.clone(), depdency_on_another_server]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        assert_eq!(t.get_enablement_state(&target), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_is_enabled_when_it_has_a_dependency_in_another_server_which_is_disabled() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let target = a_local_extension2("pub.a", json!({ "extensionDependencies": ["pub.b"], "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let depdency_on_another_server = a_local_extension2("pub.b", json!({}), Properties::at(uri_with_scheme("pub.b", schemas::VSCODE_REMOTE)));
        f.installed.borrow_mut().extend([target.clone(), depdency_on_another_server.clone()]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;
        set(&t, &[&depdency_on_another_server], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_state(&target), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_is_enabled_when_it_has_a_dependency_in_another_server_which_is_disabled_and_with_no_exports_and_no_main_and_no_browser_entrypoints() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let target = a_local_extension2("pub.a", json!({ "extensionDependencies": ["pub.b"], "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let depdency_on_another_server = a_local_extension2("pub.b", json!({ "api": "none" }), Properties::at(uri_with_scheme("pub.b", schemas::VSCODE_REMOTE)));
        f.installed.borrow_mut().extend([target.clone(), depdency_on_another_server.clone()]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;
        set(&t, &[&depdency_on_another_server], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_state(&target), E::EnabledGlobally);
    });
}

#[test]
fn test_extension_is_disabled_by_dependency_when_it_has_a_dependency_in_another_server_which_is_disabled_and_with_no_exports_and_has_main_entry_point() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let target = a_local_extension2("pub.a", json!({ "extensionDependencies": ["pub.b"], "extensionKind": ["ui"] }), Properties::at(uri_file("pub.a")));
        let depdency_on_another_server = a_local_extension2("pub.b", json!({ "api": "none", "main": "main.js" }), Properties::at(uri_with_scheme("pub.b", schemas::VSCODE_REMOTE)));
        f.installed.borrow_mut().extend([target.clone(), depdency_on_another_server.clone()]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;
        set(&t, &[&depdency_on_another_server], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_state(&target), E::DisabledByExtensionDependency);
    });
}

#[test]
fn test_extension_is_disabled_by_dependency_when_it_has_a_dependency_in_another_server_which_is_disabled_and_with_no_exports_and_has_browser_entry_point() {
    run(|f, _| async move {
        f.stub_server_service(f.a_multi_extension_management_server_service());
        let target = a_local_extension2("pub.a", json!({ "extensionDependencies": ["pub.b"] }), Properties::at(uri_with_scheme("pub.a", schemas::VSCODE_REMOTE)));
        let depdency_on_another_server = a_local_extension2("pub.b", json!({ "api": "none", "browser": "browser.js", "extensionKind": "ui" }), Properties::at(uri_file("pub.b")));
        f.installed.borrow_mut().extend([target.clone(), depdency_on_another_server.clone()]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;
        set(&t, &[&depdency_on_another_server], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_state(&target), E::DisabledByExtensionDependency);
    });
}

#[test]
fn test_extension_is_disabled_by_invalidity() {
    run(|_, t| async move {
        let target = a_local_extension2("pub.b", json!({}), Properties::invalid());
        assert_eq!(t.get_enablement_state(&target), E::DisabledByInvalidExtension);
    });
}

#[test]
fn test_extension_is_disabled_by_dependency_when_it_has_a_dependency_that_is_invalid() {
    run(|f, _| async move {
        let target = a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"] }), Properties::default());
        f.installed.borrow_mut().extend([target.clone(), a_local_extension2("pub.a", json!({}), Properties::invalid())]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        assert_eq!(t.get_enablement_state(&target), E::DisabledByExtensionDependency);
    });
}

#[test]
fn test_extension_is_enabled_when_its_dependency_becomes_valid() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.b", json!({ "extensionDependencies": ["pub.a"] }), Properties::default());
        f.installed.borrow_mut().extend([extension.clone(), a_local_extension2("pub.a", json!({}), Properties::invalid())]);
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;

        assert_eq!(t.get_enablement_state(&extension), E::DisabledByExtensionDependency);

        let target = Spy::on(&f, &t);

        let valid_extension = a_local_extension2("pub.a", json!({}), Properties::default());
        let valid_location = local_extension_uri(&valid_extension);
        f.did_install_event.fire(vec![InstallExtensionResult {
            identifier: valid_extension.identifier.clone(),
            operation: InstallOperation::Install,
            source: Some(InstallExtensionSource::Location(valid_location.clone())),
            profile_location: Some(valid_location),
            local: Some(valid_extension.clone()),
            workspace_scoped: None,
            error: None,
        }]);

        assert_eq!(t.get_enablement_state(&extension), E::EnabledGlobally);
        assert_eq!(target.first_identifier(), identifier("pub.b"));
    });
}

#[test]
fn test_override_workspace_to_trusted_when_getting_extensions_enablements() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "hello" } } }), Properties::default());
        f.stub_workspace_trusted(false);
        let t = f.new_test_object(None);

        assert_eq!(t.get_enablement_states(&[extension], WorkspaceTypeOverrides { trusted: Some(true) })[0], E::EnabledGlobally);
    });
}

#[test]
fn test_override_workspace_to_not_trusted_when_getting_extensions_enablements() {
    run(|f, _| async move {
        let extension = a_local_extension2("pub.a", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "hello" } } }), Properties::default());
        f.stub_workspace_trusted(true);
        let t = f.new_test_object(None);

        assert_eq!(t.get_enablement_states(&[extension], WorkspaceTypeOverrides { trusted: Some(false) })[0], E::DisabledByTrustRequirement);
    });
}

#[test]
fn test_update_extensions_enablements_on_trust_change_triggers_change_events_for_extensions_depending_on_workspace_trust() {
    run(|f, _| async move {
        f.installed.borrow_mut().extend([
            a_local_extension2("pub.a", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "hello" } } }), Properties::default()),
            a_local_extension2("pub.b", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": true } } }), Properties::default()),
            a_local_extension2("pub.c", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": false, "description": "hello" } } }), Properties::default()),
            a_local_extension2("pub.d", json!({ "main": "main.js", "capabilities": { "untrustedWorkspaces": { "supported": true } } }), Properties::default()),
        ]);
        let t = f.new_test_object(None);
        let target = Spy::on(&f, &t);

        t.update_extensions_enablements_when_workspace_trust_changes().await.expect("updateExtensionsEnablementsWhenWorkspaceTrustChanges resolve");
        assert_eq!(target.args(0).len(), 2);
        assert_eq!(target.args(0)[0].identifier, identifier("pub.a"));
        assert_eq!(target.args(0)[1].identifier, identifier("pub.c"));
    });
}

#[test]
fn test_adding_an_extension_that_was_disabled() {
    run(|f, _| async move {
        let extension = a_local_extension("pub.a");
        f.installed.borrow_mut().push(extension.clone());
        let t = f.new_test_object(None);
        set(&t, &[&extension], E::DisabledGlobally).await;

        let target = Spy::on(&f, &t);
        f.did_change_profile_extensions_event.fire(DidChangeProfileEvent { added: vec![extension.clone()], removed: Vec::new(), server: None });

        assert!(!t.is_enabled(&extension));
        assert_eq!(t.get_enablement_state(&extension), E::DisabledGlobally);
        assert_eq!(target.args(0).len(), 1);
        assert_eq!(target.first_identifier(), identifier("pub.a"));
    });
}

#[test]
fn test_chat_extension_is_disabled_on_profile_switch_when_setup_is_not_completed() {
    run(|f, _| async move {
        let chat_extension_id = product_service().default_chat_agent.expect("o produto declara o agente de chat").chat_extension_id;
        let chat_extension = a_local_extension_with(&chat_extension_id, None, Some(ExtensionType::System));
        f.installed.borrow_mut().push(chat_extension.clone());

        // Clear migration flag set by the setup() instance so the migration runs fresh
        let storage_service = f.storage();
        storage_service.store("builtinChatExtensionEnablementMigration", StorageValue::Bool(false), StorageScope::Profile, StorageTarget::Machine);

        // Create a chat entitlement service with context where setup is not completed
        let chat_entitlement_service = TestChatEntitlementService::new();
        chat_entitlement_service.set_context(Some(ChatEntitlementContext { state: ChatEntitlementContextState { completed: false } }));

        let t = f.new_test_object(Some(chat_entitlement_service));
        wait_until_initialized(&t).await;

        // Chat extension should be disabled after initial setup
        assert_eq!(t.get_enablement_state(&chat_extension), E::DisabledGlobally);

        // Enable the chat extension to simulate it being enabled in a previous profile
        set(&t, &[&chat_extension], E::EnabledGlobally).await;
        assert_eq!(t.get_enablement_state(&chat_extension), E::EnabledGlobally);

        // Simulate switching to a fresh profile by clearing the migration flag
        let storage_service = f.storage();
        storage_service.store("builtinChatExtensionEnablementMigration", StorageValue::Bool(false), StorageScope::Profile, StorageTarget::Machine);

        // Chat extension should be disabled again after computing enablement state
        assert_eq!(t.get_enablement_state(&chat_extension), E::DisabledGlobally);
    });
}

#[test]
fn test_extension_is_disabled_by_allowed_list() {
    run(|_, t| async move {
        let target = a_local_extension2("unallowed.extension", json!({}), Properties::default());
        assert_eq!(t.get_enablement_state(&target), E::DisabledByAllowlist);
    });
}

#[test]
fn test_extension_is_disabled_by_malicious() {
    run(|f, _| async move {
        f.malicious.borrow_mut().push(identifier("malicious.extensionA"));
        let t = f.new_test_object(None);
        wait_until_initialized(&t).await;
        let target = a_local_extension2("malicious.extensionA", json!({}), Properties::default());
        assert_eq!(t.get_enablement_state(&target), E::DisabledByMalicious);
    });
}

#[test]
fn test_installed_malicious_extension_triggers_change_event() {
    run(|f, t| async move {
        t.dispose();
        f.malicious.borrow_mut().push(identifier("malicious.extensionB"));
        let local = a_local_extension2("malicious.extensionB", json!({}), Properties::default());
        f.installed.borrow_mut().push(local.clone());
        let t = f.new_test_object(None);
        assert_eq!(t.get_enablement_state(&local), E::EnabledGlobally);
        let promise = t.on_enablement_changed().to_promise(None);

        let result = promise.await.expect("onEnablementChanged dispara");
        assert!(Rc::ptr_eq(&result[0], &local));
        assert_eq!(t.get_enablement_state(&local), E::DisabledByMalicious);
    });
}

#[test]
fn test_extensions_are_disabled_in_sessions_window_unless_they_only_contribute_themes() {
    run(|f, _| async move {
        f.stub_environment(EnvironmentOverrides { is_sessions_window: true, ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);

        let theme_only = a_local_extension2("pub.themeOnly", json!({ "contributes": a_contributes(&["themes"]) }), Properties::default());
        let icon_theme = a_local_extension2("pub.iconTheme", json!({ "contributes": a_contributes(&["iconThemes"]) }), Properties::default());
        let product_icon_theme = a_local_extension2("pub.productIconTheme", json!({ "contributes": a_contributes(&["productIconThemes"]) }), Properties::default());
        let grammar = a_local_extension2("pub.grammar", json!({ "contributes": a_contributes(&["grammars"]) }), Properties::default());
        let with_main = a_local_extension2("pub.withMain", json!({ "main": "main.js", "contributes": a_contributes(&["themes"]) }), Properties::default());
        let with_browser = a_local_extension2("pub.withBrowser", json!({ "browser": "main.browser.js", "contributes": a_contributes(&["themes"]) }), Properties::default());
        let non_theme_contrib = a_local_extension2("pub.nonThemeContrib", json!({ "contributes": a_contributes(&["commands"]) }), Properties::default());
        let builtin_with_main = a_local_extension2("pub.builtinWithMain", json!({ "main": "main.js" }), Properties::of_type(ExtensionType::System));
        let builtin_with_views = a_local_extension_with("pub.builtinWithViews", Some(a_contributes(&["views"])), Some(ExtensionType::System));
        let builtin_with_view_containers = a_local_extension_with("pub.builtinWithViewContainers", Some(a_contributes(&["viewsContainers"])), Some(ExtensionType::System));
        let builtin_with_debuggers = a_local_extension_with("pub.builtinWithDebuggers", Some(a_contributes(&["debuggers"])), Some(ExtensionType::System));
        let builtin_with_walkthroughs = a_local_extension_with("pub.builtinWithWalkthroughs", Some(a_contributes(&["walkthroughs"])), Some(ExtensionType::System));
        let user_npm = a_local_extension2("vscode.npm", json!({ "main": "main.js", "contributes": a_contributes(&["views"]) }), Properties::default());

        let states: Vec<EnablementState> = [
            theme_only,
            icon_theme,
            product_icon_theme,
            grammar,
            with_main,
            with_browser,
            non_theme_contrib,
            builtin_with_main,
            builtin_with_views,
            builtin_with_view_containers,
            builtin_with_debuggers,
            builtin_with_walkthroughs,
            user_npm,
        ]
        .iter()
        .map(|ext| t.get_enablement_state(ext))
        .collect();
        assert_eq!(
            states,
            vec![
                E::EnabledGlobally,
                E::EnabledGlobally,
                E::EnabledGlobally,
                E::EnabledGlobally,
                E::DisabledByEnvironment,
                E::DisabledByEnvironment,
                E::DisabledByEnvironment,
                E::EnabledGlobally,
                E::DisabledByEnvironment,
                E::DisabledByEnvironment,
                E::DisabledByEnvironment,
                E::DisabledByEnvironment,
                E::DisabledByEnvironment,
            ]
        );
    });
}

#[test]
fn test_built_in_npm_can_be_disabled_globally_in_sessions_window() {
    run(|f, _| async move {
        f.stub_environment(EnvironmentOverrides { is_sessions_window: true, ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);
        let npm = a_local_extension2(
            "vscode.npm",
            json!({ "main": "./out/npmMain", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": a_contributes(&["views"]) }),
            Properties::of_type(ExtensionType::System),
        );

        set(&t, &[&npm], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_state(&npm), E::DisabledGlobally);
    });
}

#[test]
fn test_built_in_npm_can_be_disabled_for_workspace_in_sessions_window() {
    run(|f, _| async move {
        f.stub_environment(EnvironmentOverrides { is_sessions_window: true, ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);
        let npm = a_local_extension2(
            "vscode.npm",
            json!({ "main": "./out/npmMain", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": a_contributes(&["views"]) }),
            Properties::of_type(ExtensionType::System),
        );

        set(&t, &[&npm], E::DisabledWorkspace).await;

        assert_eq!(t.get_enablement_state(&npm), E::DisabledWorkspace);
    });
}

/// Os casos de `for (const capabilityEnabled of [false, true])`.
async fn agents_window_declarations_without_the_proposal(f: Rc<Fixture>, capability_enabled: bool) {
    f.configuration.set_user_configuration(EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY, Dynamic::Bool(capability_enabled), None).await.expect("setUserConfiguration resolve");
    f.stub_environment(EnvironmentOverrides { is_sessions_window: true, ..EnvironmentOverrides::default() });
    let t = f.new_test_object(None);

    let builtin_supported = a_local_extension2(
        "pub.builtinSupported",
        json!({ "main": "main.js", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": a_contributes(&["views"]) }),
        Properties::of_type(ExtensionType::System),
    );
    let builtin_unsupported = a_local_extension2(
        "pub.builtinUnsupported",
        json!({ "capabilities": { "agentsWindow": { "supported": false } }, "contributes": a_contributes(&["themes"]) }),
        Properties::of_type(ExtensionType::System),
    );
    let user_supported = a_local_extension2(
        "pub.userSupported",
        json!({ "main": "main.js", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": a_contributes(&["views"]) }),
        Properties::default(),
    );
    let user_unsupported =
        a_local_extension2("pub.userUnsupported", json!({ "capabilities": { "agentsWindow": { "supported": false } }, "contributes": a_contributes(&["themes"]) }), Properties::default());
    let user_without_capability = a_local_extension2("pub.userWithoutCapability", json!({ "main": "main.js", "contributes": a_contributes(&["views"]) }), Properties::default());

    let states: Vec<EnablementState> =
        [builtin_supported, builtin_unsupported, user_supported, user_unsupported, user_without_capability].iter().map(|ext| t.get_enablement_state(ext)).collect();
    assert_eq!(
        states,
        vec![
            E::EnabledGlobally,
            E::DisabledByEnvironment,
            if capability_enabled { E::EnabledGlobally } else { E::DisabledByEnvironment },
            if capability_enabled { E::DisabledByEnvironment } else { E::EnabledGlobally },
            E::DisabledByEnvironment,
        ]
    );
}

#[test]
fn test_agents_window_declarations_without_the_proposal_with_capability_setting_disabled() {
    run(|f, _| agents_window_declarations_without_the_proposal(f, false));
}

#[test]
fn test_agents_window_declarations_without_the_proposal_with_capability_setting_enabled() {
    run(|f, _| agents_window_declarations_without_the_proposal(f, true));
}

#[test]
fn test_configured_extensions_are_enabled_in_sessions_window() {
    run(|f, _| async move {
        f.configuration
            .set_user_configuration(EXTENSIONS_SUPPORT_AGENTS_WINDOW, dynamic_of(&json!({ "pub.withMain": true, "pub.nonThemeContrib": true })), None)
            .await
            .expect("setUserConfiguration resolve");
        f.stub_environment(EnvironmentOverrides { is_sessions_window: true, ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);

        let with_main = a_local_extension2("pub.withMain", json!({ "main": "main.js", "contributes": a_contributes(&["themes"]) }), Properties::default());
        let non_theme_contrib = a_local_extension2("pub.nonThemeContrib", json!({ "contributes": a_contributes(&["commands"]) }), Properties::default());
        let with_browser = a_local_extension2("pub.withBrowser", json!({ "browser": "main.browser.js", "contributes": a_contributes(&["themes"]) }), Properties::default());

        let states: Vec<EnablementState> = [with_main, non_theme_contrib, with_browser].iter().map(|ext| t.get_enablement_state(ext)).collect();
        assert_eq!(states, vec![E::EnabledGlobally, E::EnabledGlobally, E::DisabledByEnvironment]);
    });
}

#[test]
fn test_extensions_declaring_agents_window_support_can_still_be_disabled_by_the_user() {
    run(|f, _| async move {
        f.configuration.set_user_configuration(EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY, Dynamic::Bool(true), None).await.expect("setUserConfiguration resolve");
        f.stub_environment(EnvironmentOverrides { is_sessions_window: true, ..EnvironmentOverrides::default() });
        let t = f.new_test_object(None);

        let supported = a_local_extension2("pub.supported", json!({ "main": "main.js", "capabilities": { "agentsWindow": { "supported": true } } }), Properties::default());
        set(&t, &[&supported], E::DisabledGlobally).await;

        assert_eq!(t.get_enablement_state(&supported), E::DisabledGlobally);
    });
}

#[test]
fn test_extensions_are_not_disabled_in_non_sessions_window() {
    run(|_, t| async move {
        let with_main = a_local_extension2("pub.withMain", json!({ "main": "main.js" }), Properties::default());
        let with_browser = a_local_extension2("pub.withBrowser", json!({ "browser": "main.browser.js" }), Properties::default());
        let command_contrib = a_local_extension2("pub.commands", json!({ "contributes": a_contributes(&["commands"]) }), Properties::default());

        let states: Vec<EnablementState> = [with_main, with_browser, command_contrib].iter().map(|ext| t.get_enablement_state(ext)).collect();
        assert_eq!(states, vec![E::EnabledGlobally, E::EnabledGlobally, E::EnabledGlobally]);
    });
}
