//! Porte de `src/vs/workbench/test/browser/contributions.test.ts` (suíte `Contributions`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` zera as contribuições `a` e `b` (`aCreated`, `aCreatedPromise`, `bCreated`,
//!   `bCreatedPromise`) e registra o painel de teste ([`setup_probes`] e [`run_case`]); o `teardown` é o `workbenchTeardown` do
//!   acessor (que fecha os editores e remove os grupos) seguido do `disposables.clear()`, antes de o `LeakGuard` conferir os
//!   vazamentos (`ensureNoDisposablesAreLeakedInTestSuite`). Os casos rodam em `run_with_faked_timers`, e `await` é
//!   `.await.unwrap()`.
//! - **`registerTestEditor(..., [new SyncDescriptor(TestFileEditorInput), new SyncDescriptor(TestForceRevealFileEditorInput)], ...)`.**
//!   O `TestForceRevealFileEditorInput` é a mesma classe com `capabilities` fixo (`TestFileEditorInput::new_force_reveal`), então o
//!   painel registra o `TypeId` único dela.
//! - **`TestContributionA`, `TestContributionB` e `TestContributionError`.** São estruturas sem estado cujo construtor (o fecho
//!   de [`Probe::ctor`]) marca `aCreated` e completa a promessa; o `TestContributionError` lança no construtor. O
//!   `instanceof` é a conferência do tipo concreto pelo `Any` da contribuição.
//! - **`workbenchInstantiationService(undefined, disposables)`, `createEditorPart` e `createEditorService`.** O
//!   [`EditorServiceAccessor`] já traz o serviço de instanciação, a parte de editores, o `EditorService` real e o
//!   `TestServiceAccessor` (com o `TestLifecycleService`); o `instantiationService.stub(IEditorGroupsService, part)` e o
//!   `stub(IEditorService, editorService)` do `createEditorService` já estão nele. O `ILifecycleService` do acessor é
//!   registrado na coleção, porque o registro o lê por injeção.
//! - **`registry.start(instantiationService)`.** O registro do porte recebe o serviço de instanciação, e não um
//!   `ServicesAccessor` (ver `common/contributions.rs`). O `instantiationService.createChild(new ServiceCollection([IEditorService,
//!   editorService]))` é `create_child` com a mesma coleção.
//! - **`getWorkbenchContribution`.** O `throw` dos `assert.throws` é o `Err` do resultado; o `assert.strictEqual(instanceA, ...)`
//!   compara a identidade do `Rc` (`id_of`).
//! - **`lifecycleService.phase = X`.** É `set_phase`. O `TestLifecycleService` do porte nasce na fase `Starting` (o original nasce
//!   sem fase), o que só muda o momento (síncrono ou numa microtarefa) em que a fase `Starting` é instanciada, que nenhum caso
//!   observa.
//! - **`(isCI ? test.skip : test)('lifecycle phase instantiation works for late phases', ...)`.** O original pula o caso no CI do
//!   Windows porque o `runWhenIdle` é instável lá; o porte roda sobre o relógio virtual, onde o `runWhenIdle` é determinístico,
//!   então o caso roda sempre.
//! - **`openEditor(input, { pinned: true }, SIDE_GROUP)`.** É `open_editor_typed` com `PreferredGroup::Side`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::{Any, TypeId};
use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableStore, id_of};
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::URI;
use je_instantiation::{ServiceCollection, ServicesAccessor};
use je_platform::opener::editor_types::EditorOptions;
use je_workbench::common::contributions::{IWorkbenchContribution, WorkbenchContributionCtor, WorkbenchContributionInstantiation, WorkbenchContributionsRegistry, WorkbenchPhase};
use je_workbench::common::editor::{EditorInputRef, TextEditorOptions};
use je_workbench::services::editor::editor_groups_service::PreferredGroup;
use je_workbench::services::editor::editor_service::{IEditorService, I_EDITOR_SERVICE};
use je_workbench::services::lifecycle::lifecycle::{ILifecycleService, I_LIFECYCLE_SERVICE, LifecyclePhase};
use je_workbench::test::browser::workbench_test_services::{TestFileEditorInput, register_test_editor};
use support::editor_service_support::EditorServiceAccessor;

const TEST_EDITOR_ID: &str = "MyTestEditorForContributions";
const TEST_EDITOR_INPUT_ID: &str = "testEditorInputForContributions";

//#region Infraestrutura da suíte

/// `class TestContributionA { constructor() { aCreated = true; aCreatedPromise.complete(); } }`.
#[derive(Default)]
struct TestContributionA;

impl IWorkbenchContribution for TestContributionA {}

/// `class TestContributionB`.
#[derive(Default)]
struct TestContributionB;

impl IWorkbenchContribution for TestContributionB {}

/// `aCreated` e `aCreatedPromise` (ou os de `b`).
struct Probe {
    created: Rc<Cell<bool>>,
    when_created: Rc<DeferredPromise<()>>,
}

impl Probe {
    fn new() -> Self {
        Self { created: Rc::new(Cell::new(false)), when_created: Rc::new(DeferredPromise::new()) }
    }

    /// O construtor da contribuição `T`: marca a criação e completa a promessa.
    fn ctor<T: IWorkbenchContribution + Default>(&self) -> WorkbenchContributionCtor {
        let (created, when_created) = (self.created.clone(), self.when_created.clone());
        Rc::new(move |_accessor: &ServicesAccessor| -> Rc<dyn IWorkbenchContribution> {
            created.set(true);
            let _ = when_created.complete(());

            Rc::new(T::default())
        })
    }

    /// `aCreated`.
    fn created(&self) -> bool {
        self.created.get()
    }

    /// `aCreatedPromise.p`.
    fn promise(&self) -> Promise<()> {
        self.when_created.promise.clone()
    }
}

/// `class TestContributionError { constructor() { throw new Error(); } }`.
fn error_ctor() -> WorkbenchContributionCtor {
    Rc::new(|_accessor: &ServicesAccessor| -> Rc<dyn IWorkbenchContribution> { errors::throw(Error::new("")) })
}

/// `instanceA instanceof TestContributionA`.
fn is_instance_of<T: 'static>(contribution: &Rc<dyn IWorkbenchContribution>) -> bool {
    let any: &dyn Any = &**contribution;

    any.is::<T>()
}

/// O que cada caso usa: o `disposables`, o acessor, o registro e as sondas das contribuições `a` e `b`.
struct Fixture {
    disposables: Rc<DisposableStore>,
    accessor: Rc<EditorServiceAccessor>,
    registry: Rc<WorkbenchContributionsRegistry>,
    a: Probe,
    b: Probe,
}

impl Fixture {
    fn lifecycle(&self) -> Rc<je_workbench::test::common::workbench_test_services::TestLifecycleService> {
        self.accessor.lifecycle_service.clone()
    }

    /// `createEditorService(instantiationService)`, para os casos que abrem editores: a coleção filha com o `IEditorService`,
    /// que é a do `registry.start(instantiationService.createChild(new ServiceCollection([IEditorService, editorService])))`.
    fn child_instantiation_service(&self) -> Rc<je_instantiation::InstantiationService> {
        let editor_service: Rc<dyn IEditorService> = self.accessor.editor_service.clone();
        self.accessor.instantiation_service.create_child(ServiceCollection::new().with(I_EDITOR_SERVICE, editor_service), Some(self.disposables.as_ref()))
    }

    /// `new TestFileEditorInput(URI.parse(resource), TEST_EDITOR_INPUT_ID)` já no `disposables`.
    fn editor_input(&self, resource: &str) -> EditorInputRef {
        let input = self.disposables.add_rc(TestFileEditorInput::new(URI::parse(resource, false), TEST_EDITOR_INPUT_ID));
        let editor: EditorInputRef = input;

        editor
    }
}

/// `{ pinned: true }`.
fn pinned() -> Option<TextEditorOptions> {
    Some(TextEditorOptions { base: EditorOptions { pinned: Some(true), ..EditorOptions::default() }, ..TextEditorOptions::default() })
}

/// O `setup` (as sondas e o painel de teste), o corpo no relógio virtual e o `teardown`.
fn run_case<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<Fixture>) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let (a, b) = setup_probes();
        disposables.add(register_test_editor(TEST_EDITOR_ID, vec![TypeId::of::<TestFileEditorInput>()], Some(TEST_EDITOR_INPUT_ID)));

        let accessor = Rc::new(EditorServiceAccessor::with_default_backup_service(&disposables));
        accessor.collection.set(I_LIFECYCLE_SERVICE, accessor.lifecycle_service.clone() as Rc<dyn ILifecycleService>);
        accessor.part.when_ready().await.unwrap();
        let registry = disposables.add_rc(WorkbenchContributionsRegistry::new());

        body(Rc::new(Fixture { disposables: disposables.clone(), accessor: accessor.clone(), registry, a, b })).await;

        accessor.teardown().await.unwrap();
        disposables.clear();
    });
}

/// `aCreated = false; aCreatedPromise = new DeferredPromise<void>(); bCreated = false; bCreatedPromise = new DeferredPromise<void>();`.
fn setup_probes() -> (Probe, Probe) {
    (Probe::new(), Probe::new())
}

//#endregion

#[test]
fn get_workbench_contribution_with_lazy_contributions() {
    run_case(|fixture| async move {
        let registry = &fixture.registry;

        assert!(registry.get_workbench_contribution("a").is_err());

        registry.register_workbench_contribution2(Some("a"), fixture.a.ctor::<TestContributionA>(), WorkbenchContributionInstantiation::Lazy);
        assert!(registry.get_workbench_contribution("a").is_err());

        registry.register_workbench_contribution2(Some("b"), fixture.b.ctor::<TestContributionB>(), WorkbenchContributionInstantiation::Lazy);
        registry.register_workbench_contribution2(Some("c"), error_ctor(), WorkbenchContributionInstantiation::Lazy);

        registry.start(&fixture.accessor.instantiation_service);

        let instance_a = registry.get_workbench_contribution("a").unwrap();
        assert!(is_instance_of::<TestContributionA>(&instance_a));
        assert!(fixture.a.created());
        assert_eq!(id_of(&instance_a), id_of(&registry.get_workbench_contribution("a").unwrap()));

        let instance_b = registry.get_workbench_contribution("b").unwrap();
        assert!(is_instance_of::<TestContributionB>(&instance_b));

        assert!(registry.get_workbench_contribution("c").is_err());
    });
}

#[test]
fn get_workbench_contribution_with_non_lazy_contributions() {
    run_case(|fixture| async move {
        let registry = &fixture.registry;

        fixture.lifecycle().use_phases.set(true);
        registry.start(&fixture.accessor.instantiation_service);

        assert!(registry.get_workbench_contribution("a").is_err());

        registry.register_workbench_contribution2(Some("a"), fixture.a.ctor::<TestContributionA>(), WorkbenchPhase::BlockRestore);

        let instance_a = registry.get_workbench_contribution("a").unwrap();
        assert!(is_instance_of::<TestContributionA>(&instance_a));
        assert!(fixture.a.created());

        fixture.lifecycle().set_phase(LifecyclePhase::Ready);
        fixture.a.promise().await.unwrap();

        assert_eq!(id_of(&instance_a), id_of(&registry.get_workbench_contribution("a").unwrap()));
    });
}

#[test]
fn lifecycle_phase_instantiation_works_when_phase_changes() {
    run_case(|fixture| async move {
        let registry = &fixture.registry;

        registry.start(&fixture.accessor.instantiation_service);

        registry.register_workbench_contribution2(Some("a"), fixture.a.ctor::<TestContributionA>(), WorkbenchPhase::BlockRestore);
        assert!(!fixture.a.created());

        fixture.lifecycle().set_phase(LifecyclePhase::Ready);
        fixture.a.promise().await.unwrap();
        assert!(fixture.a.created());
    });
}

#[test]
fn lifecycle_phase_instantiation_works_when_phase_was_already_met() {
    run_case(|fixture| async move {
        let registry = &fixture.registry;

        fixture.lifecycle().use_phases.set(true);
        fixture.lifecycle().set_phase(LifecyclePhase::Restored);

        registry.register_workbench_contribution2(Some("a"), fixture.a.ctor::<TestContributionA>(), WorkbenchPhase::BlockRestore);
        registry.start(&fixture.accessor.instantiation_service);

        fixture.a.promise().await.unwrap();
        assert!(fixture.a.created());
    });
}

#[test]
fn lifecycle_phase_instantiation_works_for_late_phases() {
    run_case(|fixture| async move {
        let registry = &fixture.registry;

        fixture.lifecycle().use_phases.set(true);
        registry.start(&fixture.accessor.instantiation_service);

        registry.register_workbench_contribution2(Some("a"), fixture.a.ctor::<TestContributionA>(), WorkbenchPhase::AfterRestored);
        registry.register_workbench_contribution2(Some("b"), fixture.b.ctor::<TestContributionB>(), WorkbenchPhase::Eventually);
        assert!(!fixture.a.created());
        assert!(!fixture.b.created());

        fixture.lifecycle().set_phase(LifecyclePhase::Starting);
        fixture.lifecycle().set_phase(LifecyclePhase::Ready);
        fixture.lifecycle().set_phase(LifecyclePhase::Restored);
        fixture.a.promise().await.unwrap();
        assert!(fixture.a.created());

        fixture.lifecycle().set_phase(LifecyclePhase::Eventually);
        fixture.b.promise().await.unwrap();
        assert!(fixture.b.created());
    });
}

#[test]
fn contribution_on_editor_editor_exists_before_start() {
    run_case(|fixture| async move {
        let registry = &fixture.registry;
        let editor_service = &fixture.accessor.editor_service;

        let input = fixture.editor_input("my://resource-basics");
        editor_service.open_editor_typed(&input, pinned(), None).await.unwrap();

        registry.register_workbench_contribution2(Some("a"), fixture.a.ctor::<TestContributionA>(), WorkbenchContributionInstantiation::OnEditor { editor_type_id: TEST_EDITOR_ID.to_owned() });
        registry.start(&fixture.child_instantiation_service());

        fixture.a.promise().await.unwrap();
        assert!(fixture.a.created());

        registry.register_workbench_contribution2(Some("b"), fixture.b.ctor::<TestContributionB>(), WorkbenchContributionInstantiation::OnEditor { editor_type_id: TEST_EDITOR_ID.to_owned() });

        let input2 = fixture.editor_input("my://resource-basics2");
        editor_service.open_editor_typed(&input2, pinned(), Some(PreferredGroup::Side)).await.unwrap();

        fixture.b.promise().await.unwrap();
        assert!(fixture.b.created());
    });
}

#[test]
fn contribution_on_editor_editor_does_not_exist_before_start() {
    run_case(|fixture| async move {
        let registry = &fixture.registry;
        let editor_service = &fixture.accessor.editor_service;

        let input = fixture.editor_input("my://resource-basics");

        registry.register_workbench_contribution2(Some("a"), fixture.a.ctor::<TestContributionA>(), WorkbenchContributionInstantiation::OnEditor { editor_type_id: TEST_EDITOR_ID.to_owned() });
        registry.start(&fixture.child_instantiation_service());

        editor_service.open_editor_typed(&input, pinned(), None).await.unwrap();

        fixture.a.promise().await.unwrap();
        assert!(fixture.a.created());
    });
}
