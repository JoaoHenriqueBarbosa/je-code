//! Porte de `src/vs/workbench/api/test/common/extHostExtensionActivator.test.ts` (suíte `ExtensionsActivator`), caso a caso e na mesma ordem.
//!
//! PORTADOS (9 de 9 `test(`): todos.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado antes do corpo (o `Drop` dele faz o `teardown`), e o
//!   corpo `async` roda no relógio virtual ([`run_with_faked_timers`]): `await timeout(0)` é um timer do runtime e as
//!   promessas são as do runtime, na mesma ordem de microtarefas do motor JS.
//! - `const disposables = new DisposableStore()` é o `DisposableStore` do `je-base`, descartado no fim de cada caso.
//! - `IExtensionDescription` é o `ExtensionDescription` de `je-extensions` (o `extensionLocation` é o texto
//!   `nothing://nowhere`), compartilhado como `Rc` entre o registro do host e o global, como as referências do JavaScript.
//!   `delete bExt.main` e `delete bExt.browser` são `None` no manifesto, e `bExt.api = 'none'` é `Some("none")`.
//! - `SimpleExtensionsActivatorHost` e `PromiseExtensionsActivatorHost` (que o estende) são os structs de mesmo nome; a
//!   herança é um `Deref` para o base. O `throw new Error('Unexpected!')` síncrono do dublê é [`errors::throw`].
//! - `assert.deepStrictEqual(host.activateCalls, [idA])` é `assert_eq!` entre listas de [`ExtensionIdentifier`] (a igualdade
//!   ignora a caixa, como o `ExtensionIdentifier` do original).
//! - `activator.activateById(...)` que lança de forma síncrona é o `Err` do resultado; a captura do `try` é um `match`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{Promise, Resolver};
use je_extensions::manifest::{Engines, ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_extension_activator::{
    ActivatedExtension, ExtensionActivationTimes, ExtensionsActivator, IExtensionsActivatorHost,
};
use je_workbench::services::extensions::extension_activation::{ExtensionActivationReason, MissingExtensionDependency};
use je_workbench::services::extensions::extension_description_registry::{ExtensionDescriptionRegistry, IActivationEventsReader};

fn id_a() -> ExtensionIdentifier {
    ExtensionIdentifier::new("a")
}

fn id_b() -> ExtensionIdentifier {
    ExtensionIdentifier::new("b")
}

fn id_c() -> ExtensionIdentifier {
    ExtensionIdentifier::new("c")
}

type ActivationError = (ExtensionIdentifier, Option<Error>, Option<MissingExtensionDependency>);

struct SimpleExtensionsActivatorHost {
    activate_calls: RefCell<Vec<ExtensionIdentifier>>,
    errors: RefCell<Vec<ActivationError>>,
}

impl SimpleExtensionsActivatorHost {
    fn new() -> Self {
        Self { activate_calls: RefCell::new(Vec::new()), errors: RefCell::new(Vec::new()) }
    }

    fn activate_calls(&self) -> Vec<ExtensionIdentifier> {
        self.activate_calls.borrow().clone()
    }

    fn errors(&self) -> Vec<ActivationError> {
        self.errors.borrow().clone()
    }

    fn record_error(&self, extension_id: &ExtensionIdentifier, error: Option<&Error>, missing_extension_dependency: Option<&MissingExtensionDependency>) {
        self.errors.borrow_mut().push((extension_id.clone(), error.cloned(), missing_extension_dependency.cloned()));
    }
}

impl IExtensionsActivatorHost for SimpleExtensionsActivatorHost {
    fn on_extension_activation_error(
        &self,
        extension_id: &ExtensionIdentifier,
        error: Option<&Error>,
        missing_extension_dependency: Option<&MissingExtensionDependency>,
    ) {
        self.record_error(extension_id, error, missing_extension_dependency);
    }

    fn actual_activate_extension(&self, extension_id: &ExtensionIdentifier, _reason: &ExtensionActivationReason) -> Promise<Rc<ActivatedExtension>> {
        self.activate_calls.borrow_mut().push(extension_id.clone());
        Promise::resolved(Rc::new(ActivatedExtension::empty(ExtensionActivationTimes::NONE)))
    }
}

struct PromiseExtensionsActivatorHost {
    base: SimpleExtensionsActivatorHost,
    promises: Vec<(ExtensionIdentifier, Rc<ExtensionActivationPromiseSource>)>,
}

impl PromiseExtensionsActivatorHost {
    fn new(promises: Vec<(ExtensionIdentifier, Rc<ExtensionActivationPromiseSource>)>) -> Self {
        Self { base: SimpleExtensionsActivatorHost::new(), promises }
    }
}

impl Deref for PromiseExtensionsActivatorHost {
    type Target = SimpleExtensionsActivatorHost;

    fn deref(&self) -> &SimpleExtensionsActivatorHost {
        &self.base
    }
}

impl IExtensionsActivatorHost for PromiseExtensionsActivatorHost {
    fn on_extension_activation_error(
        &self,
        extension_id: &ExtensionIdentifier,
        error: Option<&Error>,
        missing_extension_dependency: Option<&MissingExtensionDependency>,
    ) {
        self.base.record_error(extension_id, error, missing_extension_dependency);
    }

    fn actual_activate_extension(&self, extension_id: &ExtensionIdentifier, _reason: &ExtensionActivationReason) -> Promise<Rc<ActivatedExtension>> {
        self.base.activate_calls.borrow_mut().push(extension_id.clone());
        for (id, promise_source) in &self.promises {
            if id.value == extension_id.value {
                return promise_source.promise.clone();
            }
        }
        errors::throw(Error::new("Unexpected!"));
    }
}

struct ExtensionActivationPromiseSource {
    resolver: Resolver<Rc<ActivatedExtension>>,
    promise: Promise<Rc<ActivatedExtension>>,
}

impl ExtensionActivationPromiseSource {
    fn new() -> Rc<Self> {
        let (promise, resolver) = Promise::new();
        Rc::new(Self { resolver, promise })
    }

    fn resolve(&self) {
        self.resolver.resolve(Rc::new(ActivatedExtension::empty(ExtensionActivationTimes::NONE)));
    }

    fn reject(&self, err: Error) {
        self.resolver.reject(err);
    }
}

struct BasicActivationEventsReader;

impl IActivationEventsReader for BasicActivationEventsReader {
    fn read_activation_events(&self, extension_description: &ExtensionDescription) -> Vec<String> {
        extension_description.manifest.activation_events.clone().unwrap_or_default()
    }
}

fn create_activator(
    host: Rc<dyn IExtensionsActivatorHost>,
    extension_descriptions: Vec<ExtensionDescription>,
    other_host_extension_descriptions: Vec<ExtensionDescription>,
    disposables: &Rc<DisposableStore>,
) -> Rc<ExtensionsActivator> {
    let descriptions: Vec<Rc<ExtensionDescription>> = extension_descriptions.into_iter().map(Rc::new).collect();
    let others: Vec<Rc<ExtensionDescription>> = other_host_extension_descriptions.into_iter().map(Rc::new).collect();
    let registry = disposables.add_rc(ExtensionDescriptionRegistry::new(Rc::new(BasicActivationEventsReader), descriptions.clone()));
    let global_registry = disposables.add_rc(ExtensionDescriptionRegistry::new(
        Rc::new(BasicActivationEventsReader),
        descriptions.iter().cloned().chain(others).collect(),
    ));
    disposables.add_rc(ExtensionsActivator::new(registry, global_registry, host, NullLogService::shared()))
}

fn desc_with(id: &ExtensionIdentifier, deps: &[ExtensionIdentifier], activation_events: &[&str]) -> ExtensionDescription {
    ExtensionDescription {
        id: id.value.clone(),
        identifier: id.clone(),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: "nothing://nowhere".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: id.value.clone(),
            publisher: "test".to_owned(),
            version: "0.0.0".to_owned(),
            engines: Engines { vscode: Some("^1.0.0".to_owned()), ..Engines::default() },
            activation_events: Some(activation_events.iter().map(|event| (*event).to_owned()).collect()),
            main: Some("index.js".to_owned()),
            extension_dependencies: Some(deps.iter().map(|dep| dep.value.clone()).collect()),
            enabled_api_proposals: None,
            ..ExtensionManifest::default()
        },
    }
}

/// `desc(id)`: sem dependências e com o evento `*`.
fn desc(id: &ExtensionIdentifier) -> ExtensionDescription {
    desc_with(id, &[], &["*"])
}

/// `desc(id, deps)`: com o evento `*`.
fn desc_deps(id: &ExtensionIdentifier, deps: &[ExtensionIdentifier]) -> ExtensionDescription {
    desc_with(id, deps, &["*"])
}

fn reason_of(extension_id: &ExtensionIdentifier, activation_event: &str) -> ExtensionActivationReason {
    ExtensionActivationReason { startup: false, extension_id: extension_id.clone(), activation_event: activation_event.to_owned() }
}

/// O corpo de um caso: o guarda de vazamentos e o relógio virtual.
fn run_test(body: impl std::future::Future<Output = ()> + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(body);
    drop(guard);
}

#[test]
fn calls_activate_only_once_with_sequential_activations() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let host = Rc::new(SimpleExtensionsActivatorHost::new());
        let activator = create_activator(host.clone(), vec![desc(&id_a())], vec![], &disposables);

        activator.activate_by_event("*", false).await.unwrap();
        assert_eq!(host.activate_calls(), vec![id_a()]);

        activator.activate_by_event("*", false).await.unwrap();
        assert_eq!(host.activate_calls(), vec![id_a()]);

        disposables.dispose();
    });
}

#[test]
fn calls_activate_only_once_with_parallel_activations() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let ext_activation = ExtensionActivationPromiseSource::new();
        let host = Rc::new(PromiseExtensionsActivatorHost::new(vec![(id_a(), ext_activation.clone())]));
        let activator = create_activator(host.clone(), vec![desc_with(&id_a(), &[], &["evt1", "evt2"])], vec![], &disposables);

        let activate1 = activator.activate_by_event("evt1", false);
        let activate2 = activator.activate_by_event("evt2", false);

        ext_activation.resolve();

        activate1.await.unwrap();
        activate2.await.unwrap();

        assert_eq!(host.activate_calls(), vec![id_a()]);

        disposables.dispose();
    });
}

#[test]
fn activates_dependencies_first() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let ext_activation_a = ExtensionActivationPromiseSource::new();
        let ext_activation_b = ExtensionActivationPromiseSource::new();
        let host = Rc::new(PromiseExtensionsActivatorHost::new(vec![(id_a(), ext_activation_a.clone()), (id_b(), ext_activation_b.clone())]));
        let activator = create_activator(
            host.clone(),
            vec![desc_with(&id_a(), &[id_b()], &["evt1"]), desc_with(&id_b(), &[], &["evt1"])],
            vec![],
            &disposables,
        );

        let activate = activator.activate_by_event("evt1", false);

        timeout(0).await.unwrap();
        assert_eq!(host.activate_calls(), vec![id_b()]);
        ext_activation_b.resolve();

        timeout(0).await.unwrap();
        assert_eq!(host.activate_calls(), vec![id_b(), id_a()]);
        ext_activation_a.resolve();

        timeout(0).await.unwrap();
        activate.await.unwrap();

        assert_eq!(host.activate_calls(), vec![id_b(), id_a()]);

        disposables.dispose();
    });
}

#[test]
fn supports_having_resolved_extensions() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let host = Rc::new(SimpleExtensionsActivatorHost::new());
        let mut b_ext = desc(&id_b());
        b_ext.manifest.main = None;
        b_ext.manifest.browser = None;
        let activator = create_activator(host.clone(), vec![desc_deps(&id_a(), &[id_b()])], vec![b_ext], &disposables);

        activator.activate_by_event("*", false).await.unwrap();
        assert_eq!(host.activate_calls(), vec![id_a()]);

        disposables.dispose();
    });
}

#[test]
fn supports_having_external_extensions() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let ext_activation_a = ExtensionActivationPromiseSource::new();
        let ext_activation_b = ExtensionActivationPromiseSource::new();
        let host = Rc::new(PromiseExtensionsActivatorHost::new(vec![(id_a(), ext_activation_a.clone()), (id_b(), ext_activation_b.clone())]));
        let mut b_ext = desc(&id_b());
        b_ext.manifest.api = Some("none".to_owned());
        let activator = create_activator(host.clone(), vec![desc_deps(&id_a(), &[id_b()])], vec![b_ext], &disposables);

        let activate = activator.activate_by_event("*", false);

        timeout(0).await.unwrap();
        assert_eq!(host.activate_calls(), vec![id_b()]);
        ext_activation_b.resolve();

        timeout(0).await.unwrap();
        assert_eq!(host.activate_calls(), vec![id_b(), id_a()]);
        ext_activation_a.resolve();

        activate.await.unwrap();
        assert_eq!(host.activate_calls(), vec![id_b(), id_a()]);

        disposables.dispose();
    });
}

#[test]
fn error_activate_by_id_with_missing_extension() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let host = Rc::new(SimpleExtensionsActivatorHost::new());
        let activator = create_activator(host.clone(), vec![desc(&id_a()), desc(&id_b())], vec![], &disposables);

        let error: Option<Error> = match activator.activate_by_id(&id_c(), reason_of(&id_c(), "none")) {
            Ok(activation) => activation.await.err(),
            Err(err) => Some(err),
        };

        assert!(error.is_some());

        disposables.dispose();
    });
}

#[test]
fn error_dependency_missing() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let host = Rc::new(SimpleExtensionsActivatorHost::new());
        let activator = create_activator(host.clone(), vec![desc_deps(&id_a(), &[id_b()])], vec![], &disposables);

        activator.activate_by_event("*", false).await.unwrap();

        assert_eq!(host.errors().len(), 1);
        assert_eq!(host.errors()[0].0, id_a());

        disposables.dispose();
    });
}

#[test]
fn error_dependency_activation_failed() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let ext_activation_a = ExtensionActivationPromiseSource::new();
        let ext_activation_b = ExtensionActivationPromiseSource::new();
        let host = Rc::new(PromiseExtensionsActivatorHost::new(vec![(id_a(), ext_activation_a.clone()), (id_b(), ext_activation_b.clone())]));
        let activator = create_activator(host.clone(), vec![desc_deps(&id_a(), &[id_b()]), desc(&id_b())], vec![], &disposables);

        let activate = activator.activate_by_event("*", false);
        ext_activation_b.reject(Error::new("b fails!"));

        activate.await.unwrap();
        assert_eq!(host.errors().len(), 2);
        assert_eq!(host.errors()[0].0, id_b());
        assert_eq!(host.errors()[1].0, id_a());

        disposables.dispose();
    });
}

#[test]
fn issue_144518_problem_with_git_extension_and_vscode_icons() {
    run_test(async move {
        let disposables = DisposableStore::new();
        let ext_activation_a = ExtensionActivationPromiseSource::new();
        let ext_activation_b = ExtensionActivationPromiseSource::new();
        let ext_activation_c = ExtensionActivationPromiseSource::new();
        let host = Rc::new(PromiseExtensionsActivatorHost::new(vec![
            (id_a(), ext_activation_a.clone()),
            (id_b(), ext_activation_b.clone()),
            (id_c(), ext_activation_c.clone()),
        ]));
        let activator = create_activator(host.clone(), vec![desc_deps(&id_a(), &[id_b()]), desc(&id_b()), desc(&id_c())], vec![], &disposables);

        drop(activator.activate_by_event("*", false));
        assert_eq!(host.activate_calls(), vec![id_b(), id_c()]);

        ext_activation_b.resolve();
        timeout(0).await.unwrap();

        assert_eq!(host.activate_calls(), vec![id_b(), id_c(), id_a()]);
        ext_activation_a.resolve();

        disposables.dispose();
    });
}
