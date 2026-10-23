//! Porte de `src/vs/platform/instantiation/test/common/instantiationService.test.ts` (suíte `Instantiation Service`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Os decorators de parâmetro (`@IService1 service1: IService1`) viram a tupla `Deps` do `Injectable`,
//!   e os argumentos estáticos (`v: boolean`) viram `Args`.
//! - `instanceof` não existe entre `Rc<dyn Trait>`; onde o original confere a classe do argumento
//!   injetado, o porte confere a identidade do objeto registrado (`id_of`).
//! - `assert.throws` vira `errors::catch(...).is_err()` (exceção é pânico com `Error`).
//! - O `_serviceBrand` não tem equivalente e não é portado.

#![allow(dead_code)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{IDisposable, id_of};
use je_instantiation::{
    Injectable, InstantiationOptions, InstantiationService, ServiceCollection, ServiceEntry,
    ServicesAccessor, SyncDescriptor, delayed_proxy, service_id, service_impl,
};

// --- IService1 ---

trait IService1 {
    fn c(&self) -> i32;
}
service_id!(I_SERVICE1: dyn IService1 = "service1");

struct Service1;
impl IService1 for Service1 {
    fn c(&self) -> i32 {
        1
    }
}
impl Injectable for Service1 {
    type Args = ();
    type Deps = ();
    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}
service_impl!(Service1 => dyn IService1);

fn service1() -> Rc<dyn IService1> {
    Rc::new(Service1)
}

// --- IService2 ---

trait IService2 {
    fn d(&self) -> bool;
}
service_id!(I_SERVICE2: dyn IService2 = "service2");

struct Service2;
impl IService2 for Service2 {
    fn d(&self) -> bool {
        true
    }
}

fn service2() -> Rc<dyn IService2> {
    Rc::new(Service2)
}

// --- IService3 ---

trait IService3 {
    fn s(&self) -> String;
}
service_id!(I_SERVICE3: dyn IService3 = "service3");

struct Service3;
impl IService3 for Service3 {
    fn s(&self) -> String {
        "farboo".to_string()
    }
}

fn service3() -> Rc<dyn IService3> {
    Rc::new(Service3)
}

// --- IDependentService ---

trait IDependentService {
    fn name(&self) -> String;
}
service_id!(I_DEPENDENT_SERVICE: dyn IDependentService = "dependentService");

struct DependentService;
impl IDependentService for DependentService {
    fn name(&self) -> String {
        "farboo".to_string()
    }
}
impl Injectable for DependentService {
    type Args = ();
    type Deps = (Rc<dyn IService1>,);
    fn create(_: (), (service,): Self::Deps) -> Rc<Self> {
        assert_eq!(service.c(), 1);
        Rc::new(Self)
    }
}
service_impl!(DependentService => dyn IDependentService);

struct Service1Consumer;
impl Injectable for Service1Consumer {
    type Args = ();
    type Deps = (Rc<dyn IService1>,);
    fn create(_: (), (service1,): Self::Deps) -> Rc<Self> {
        assert_eq!(service1.c(), 1);
        Rc::new(Self)
    }
}

struct Target2Dep;
impl Injectable for Target2Dep {
    type Args = ();
    type Deps = (Rc<dyn IService1>, Rc<dyn IService2>);
    fn create(_: (), (service1, service2): Self::Deps) -> Rc<Self> {
        assert_eq!(service1.c(), 1);
        assert!(service2.d());
        Rc::new(Self)
    }
}

struct TargetWithStaticParam;
impl Injectable for TargetWithStaticParam {
    type Args = bool;
    type Deps = (Rc<dyn IService1>,);
    fn create(v: bool, (service1,): Self::Deps) -> Rc<Self> {
        assert!(v);
        assert_eq!(service1.c(), 1);
        Rc::new(Self)
    }
}

// --- IStrictStaticArgumentService ---

trait IStrictStaticArgumentService {
    fn value(&self) -> bool;
}
service_id!(I_STRICT_STATIC_ARGUMENT_SERVICE: dyn IStrictStaticArgumentService = "strictStaticArgumentService");

struct StrictStaticArgumentService {
    value: bool,
}
impl IStrictStaticArgumentService for StrictStaticArgumentService {
    fn value(&self) -> bool {
        self.value
    }
}
impl Injectable for StrictStaticArgumentService {
    type Args = bool;
    type Deps = (Rc<dyn IService1>,);
    fn create(value: bool, _: Self::Deps) -> Rc<Self> {
        Rc::new(Self { value })
    }
}
service_impl!(StrictStaticArgumentService => dyn IStrictStaticArgumentService);

struct DependentServiceTarget;
impl Injectable for DependentServiceTarget {
    type Args = ();
    type Deps = (Rc<dyn IDependentService>,);
    fn create(_: (), (d,): Self::Deps) -> Rc<Self> {
        assert_eq!(d.name(), "farboo");
        Rc::new(Self)
    }
}

struct DependentServiceTarget2;
impl Injectable for DependentServiceTarget2 {
    type Args = ();
    type Deps = (Rc<dyn IDependentService>, Rc<dyn IService1>);
    fn create(_: (), (d, s): Self::Deps) -> Rc<Self> {
        assert_eq!(d.name(), "farboo");
        assert_eq!(s.c(), 1);
        Rc::new(Self)
    }
}

// --- laço entre IService1 e IService2 ---

struct ServiceLoop1;
impl IService1 for ServiceLoop1 {
    fn c(&self) -> i32 {
        1
    }
}
impl Injectable for ServiceLoop1 {
    type Args = ();
    type Deps = (Rc<dyn IService2>,);
    fn create(_: (), _: Self::Deps) -> Rc<Self> {
        Rc::new(Self)
    }
}
service_impl!(ServiceLoop1 => dyn IService1);

struct ServiceLoop2;
impl IService2 for ServiceLoop2 {
    fn d(&self) -> bool {
        true
    }
}
impl Injectable for ServiceLoop2 {
    type Args = ();
    type Deps = (Rc<dyn IService1>,);
    fn create(_: (), _: Self::Deps) -> Rc<Self> {
        Rc::new(Self)
    }
}
service_impl!(ServiceLoop2 => dyn IService2);

fn strict() -> InstantiationOptions {
    InstantiationOptions {
        strict: true,
        ..InstantiationOptions::default()
    }
}

fn strict_with_tracing() -> InstantiationOptions {
    InstantiationOptions {
        strict: true,
        enable_tracing: true,
        ..InstantiationOptions::default()
    }
}

/// O erro lançado por `f`, se lançou.
fn thrown<R>(f: impl FnOnce() -> R) -> Option<Error> {
    errors::catch(f).err()
}

mod instantiation_service_suite {
    use super::*;

    // O original grava `null` na coleção; `Rc` não é nulo, então o porte usa uma instância. O que o caso
    // verifica é que `set` devolve a entrada anterior (e nada na primeira vez).
    #[test]
    fn service_collection_cannot_overwrite() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let result = collection.set(I_SERVICE1, service1());
        assert!(result.is_none());
        let result = collection.set(I_SERVICE1, service1());
        assert!(matches!(result, Some(ServiceEntry::Instance(_))));
    }

    #[test]
    fn service_collection_add_has() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        collection.set(I_SERVICE1, service1());
        assert!(collection.has(I_SERVICE1));

        collection.set(I_SERVICE2, service2());
        assert!(collection.has(I_SERVICE1));
        assert!(collection.has(I_SERVICE2));
    }

    #[test]
    fn param_simple_class() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(I_SERVICE1, service1());
        collection.set(I_SERVICE2, service2());
        collection.set(I_SERVICE3, service3());

        service.create_instance::<Service1Consumer>(());
    }

    #[test]
    fn param_fixed_args() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(I_SERVICE1, service1());
        collection.set(I_SERVICE2, service2());
        collection.set(I_SERVICE3, service3());

        service.create_instance::<TargetWithStaticParam>(true);
    }

    #[test]
    fn service_collection_is_live() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        collection.set(I_SERVICE1, service1());

        let service = InstantiationService::new(collection.clone());
        service.create_instance::<Service1Consumer>(());

        collection.set(I_SERVICE2, service2());

        service.create_instance::<Target2Dep>(());
        service.invoke_function(|a| {
            assert_eq!(a.get(I_SERVICE1).c(), 1);
            assert!(a.get(I_SERVICE2).d());
        });
    }

    // O original barra, na coleção estrita, um descritor cujo número de argumentos estáticos difere do
    // número de parâmetros que antecedem os decorados ("must pass exactly 1 leading static arguments
    // (got 0)"). Em Rust o `Args` do `Injectable` tipa os argumentos estáticos, então os dois erros do
    // original (faltar e sobrar argumento) são erros de compilação (há um teste `compile_fail` em
    // `SyncDescriptor`). O que sobra a verificar é o caminho feliz: o descritor com o argumento certo
    // resolve e entrega o valor.
    #[test]
    fn strict_service_collection_validates_descriptor_static_arguments() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new().with(I_SERVICE1, service1());
        collection.set(
            I_STRICT_STATIC_ARGUMENT_SERVICE,
            SyncDescriptor::<dyn IStrictStaticArgumentService>::new::<StrictStaticArgumentService>(
                true,
            ),
        );
        let service = InstantiationService::new(collection)
            .invoke_function(|accessor| accessor.get(I_STRICT_STATIC_ARGUMENT_SERVICE));
        assert!(service.value());
    }

    #[test]
    fn sync_desc_no_dependencies() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(
            I_SERVICE1,
            SyncDescriptor::<dyn IService1>::new::<Service1>(()),
        );

        service.invoke_function(|accessor| {
            let service1 = accessor.get(I_SERVICE1);
            assert_eq!(service1.c(), 1);

            let service2 = accessor.get(I_SERVICE1);
            assert_eq!(id_of(&service1), id_of(&service2));
        });
    }

    #[test]
    fn sync_desc_service_with_service_dependency() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(
            I_SERVICE1,
            SyncDescriptor::<dyn IService1>::new::<Service1>(()),
        );
        collection.set(
            I_DEPENDENT_SERVICE,
            SyncDescriptor::<dyn IDependentService>::new::<DependentService>(()),
        );

        service.invoke_function(|accessor| {
            let d = accessor.get(I_DEPENDENT_SERVICE);
            assert_eq!(d.name(), "farboo");
        });
    }

    // `d instanceof DependentServiceTarget` é garantido pelo tipo de retorno; o que importa é que os
    // construtores rodaram (eles conferem os serviços injetados).
    #[test]
    fn sync_desc_target_depends_on_service_future() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(
            I_SERVICE1,
            SyncDescriptor::<dyn IService1>::new::<Service1>(()),
        );
        collection.set(
            I_DEPENDENT_SERVICE,
            SyncDescriptor::<dyn IDependentService>::new::<DependentService>(()),
        );

        let _d: Rc<DependentServiceTarget> = service.create_instance::<DependentServiceTarget>(());
        let _d2: Rc<DependentServiceTarget2> =
            service.create_instance::<DependentServiceTarget2>(());
    }

    #[test]
    fn sync_desc_explode_on_loop() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(
            I_SERVICE1,
            SyncDescriptor::<dyn IService1>::new::<ServiceLoop1>(()),
        );
        collection.set(
            I_SERVICE2,
            SyncDescriptor::<dyn IService2>::new::<ServiceLoop2>(()),
        );

        assert!(thrown(|| service.invoke_function(|accessor| accessor.get(I_SERVICE1))).is_some());
        assert!(thrown(|| service.invoke_function(|accessor| accessor.get(I_SERVICE2))).is_some());

        let err = thrown(|| service.invoke_function(|accessor| accessor.get(I_SERVICE1)))
            .expect("o laço lança");
        assert!(!err.name.is_empty());
        assert!(!err.message.is_empty());
        // A mensagem é a cadeia do ciclo (`CyclicDependencyError`).
        assert_eq!(err.message, "service1 -> service2 -> service1");
    }

    #[test]
    fn invoke_get_services() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(I_SERVICE1, service1());
        collection.set(I_SERVICE2, service2());

        let test = |accessor: &ServicesAccessor| {
            assert_eq!(accessor.get(I_SERVICE1).c(), 1);
            true
        };

        assert!(service.invoke_function(test));
    }

    #[test]
    fn invoke_get_service_optional() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new().with(I_SERVICE1, service1());
        let service = InstantiationService::with_options(collection, strict());

        let test = |accessor: &ServicesAccessor| {
            assert_eq!(accessor.get(I_SERVICE1).c(), 1);
            let err = thrown(|| accessor.try_get(I_SERVICE2)).expect("modo estrito lança");
            assert_eq!(err.message, "[invokeFunction] unknown service 'service2'");
            true
        };
        assert!(service.invoke_function(test));
    }

    #[test]
    fn invoke_keeping_accessor_not_allowed() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(I_SERVICE1, service1());
        collection.set(I_SERVICE2, service2());

        let cached: RefCell<Option<ServicesAccessor>> = RefCell::new(None);

        let test = |accessor: &ServicesAccessor| {
            assert_eq!(accessor.get(I_SERVICE1).c(), 1);
            *cached.borrow_mut() = Some(accessor.clone());
            true
        };

        assert!(service.invoke_function(test));

        let cached = cached.borrow().clone().expect("guardado");
        assert!(thrown(|| cached.get(I_SERVICE2)).is_some());
    }

    #[test]
    fn invoke_throw_error() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let collection = ServiceCollection::new();
        let service = InstantiationService::new(collection.clone());
        collection.set(I_SERVICE1, service1());
        collection.set(I_SERVICE2, service2());

        let test = |_accessor: &ServicesAccessor| -> () { errors::throw(Error::new("")) };

        assert!(thrown(|| service.invoke_function(test)).is_some());
    }

    #[test]
    fn create_child() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        // O `CtorCounter` do original conta construções numa variável do módulo; aqui o contador entra
        // como argumento estático do descritor.
        struct CtorCounter;
        impl IService1 for CtorCounter {
            fn c(&self) -> i32 {
                1
            }
        }
        impl Injectable for CtorCounter {
            type Args = Rc<Cell<u32>>;
            type Deps = ();
            fn create(count: Self::Args, _: ()) -> Rc<Self> {
                count.set(count.get() + 1);
                Rc::new(Self)
            }
        }
        service_impl!(CtorCounter => dyn IService1);

        let service_instance_count = Rc::new(Cell::new(0));
        let descriptor =
            || SyncDescriptor::<dyn IService1>::new::<CtorCounter>(service_instance_count.clone());

        // creating the service instance BEFORE the child service
        let mut service =
            InstantiationService::new(ServiceCollection::new().with(I_SERVICE1, descriptor()));
        service.create_instance::<Service1Consumer>(());

        // second instance must be earlier ONE
        let mut child =
            service.create_child(ServiceCollection::new().with(I_SERVICE2, service2()), None);
        child.create_instance::<Service1Consumer>(());

        assert_eq!(service_instance_count.get(), 1);

        // creating the service instance AFTER the child service
        service_instance_count.set(0);
        service =
            InstantiationService::new(ServiceCollection::new().with(I_SERVICE1, descriptor()));
        child = service.create_child(ServiceCollection::new().with(I_SERVICE2, service2()), None);

        // second instance must be earlier ONE
        service.create_instance::<Service1Consumer>(());
        child.create_instance::<Service1Consumer>(());

        assert_eq!(service_instance_count.get(), 1);
    }

    #[test]
    fn remote_window_integration_tests_is_broken_105562() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        trait IS1 {}
        service_id!(SERVICE1: dyn IS1 = "service1");
        trait IS2 {}
        service_id!(SERVICE2: dyn IS2 = "service2");
        trait IS21 {}
        service_id!(SERVICE21: dyn IS21 = "service21");

        struct Service1Impl;
        impl IS1 for Service1Impl {}
        impl Injectable for Service1Impl {
            type Args = ();
            type Deps = (Rc<InstantiationService>,);
            fn create(_: (), (insta,): Self::Deps) -> Rc<Self> {
                let c = insta.invoke_function(|accessor| accessor.get(SERVICE2)); // THIS is the recursive call
                let _ = c;
                Rc::new(Self)
            }
        }
        service_impl!(Service1Impl => dyn IS1);

        struct Service2Impl;
        impl IS2 for Service2Impl {}
        impl Injectable for Service2Impl {
            type Args = ();
            type Deps = ();
            fn create(_: (), _: ()) -> Rc<Self> {
                Rc::new(Self)
            }
        }
        service_impl!(Service2Impl => dyn IS2);

        // This service depends on Service1 and Service2 BUT creating Service1 creates Service2 (via recursive invocation)
        // and then Servce2 should not be created a second time
        struct Service21Impl;
        impl IS21 for Service21Impl {}
        impl Injectable for Service21Impl {
            type Args = ();
            type Deps = (Rc<dyn IS2>, Rc<dyn IS1>);
            fn create(_: (), _: Self::Deps) -> Rc<Self> {
                Rc::new(Self)
            }
        }
        service_impl!(Service21Impl => dyn IS21);

        let insta = InstantiationService::new(
            ServiceCollection::new()
                .with(SERVICE1, SyncDescriptor::<dyn IS1>::new::<Service1Impl>(()))
                .with(SERVICE2, SyncDescriptor::<dyn IS2>::new::<Service2Impl>(()))
                .with(
                    SERVICE21,
                    SyncDescriptor::<dyn IS21>::new::<Service21Impl>(()),
                ),
        );

        let _obj = insta.invoke_function(|accessor| accessor.get(SERVICE21));
    }

    #[test]
    fn sync_async_dependency_loop() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        trait A {
            fn do_it(&self);
        }
        trait B {
            fn b(&self) -> bool;
        }
        delayed_proxy!(AProxy for dyn A {
            methods { fn do_it(&self); }
            events {}
        });
        service_id!(A_ID: dyn A = "A", proxy AProxy);
        service_id!(B_ID: dyn B = "B");

        struct BConsumer {
            b: Rc<dyn B>,
        }
        impl BConsumer {
            fn do_it(&self) -> bool {
                self.b.b()
            }
        }
        impl Injectable for BConsumer {
            type Args = ();
            type Deps = (Rc<dyn B>,);
            fn create(_: (), (b,): Self::Deps) -> Rc<Self> {
                Rc::new(Self { b })
            }
        }

        struct AService {
            prop: Rc<BConsumer>,
        }
        impl A for AService {
            fn do_it(&self) {
                self.prop.do_it();
            }
        }
        impl Injectable for AService {
            type Args = ();
            type Deps = (Rc<InstantiationService>,);
            fn create(_: (), (insta,): Self::Deps) -> Rc<Self> {
                Rc::new(Self {
                    prop: insta.create_instance::<BConsumer>(()),
                })
            }
        }
        service_impl!(AService => dyn A);

        struct BService;
        impl B for BService {
            fn b(&self) -> bool {
                true
            }
        }
        impl Injectable for BService {
            type Args = ();
            type Deps = (Rc<dyn A>,);
            fn create(_: (), _a: Self::Deps) -> Rc<Self> {
                Rc::new(Self)
            }
        }
        service_impl!(BService => dyn B);

        // SYNC -> explodes AImpl -> [insta:BConsumer] -> BImpl -> AImpl
        {
            let insta1 = InstantiationService::with_options(
                ServiceCollection::new()
                    .with(A_ID, SyncDescriptor::<dyn A>::new::<AService>(()))
                    .with(B_ID, SyncDescriptor::<dyn B>::new::<BService>(())),
                strict_with_tracing(),
            );

            let error = thrown(|| insta1.invoke_function(|accessor| accessor.get(A_ID)))
                .expect("o laço síncrono explode");
            assert!(error.message.contains("RECURSIVELY"));
        }

        // ASYNC -> doesn't explode but cycle is tracked
        {
            let insta2 = InstantiationService::with_options(
                ServiceCollection::new()
                    .with(A_ID, SyncDescriptor::<dyn A>::new::<AService>(()).delayed())
                    .with(B_ID, SyncDescriptor::<dyn B>::new::<BService>(())),
                strict_with_tracing(),
            );

            let a = insta2.invoke_function(|accessor| accessor.get(A_ID));
            a.do_it();

            let cycle = insta2
                .global_graph()
                .and_then(|g| g.borrow().find_cycle_slow());
            assert_eq!(cycle.as_deref(), Some("A -> B -> A"));
        }
    }

    // As interfaces `A` dos quatro casos de eventos ficam locais a cada caso, como no original. O
    // `Event<this>` (que dispara o próprio objeto e o teste confere `instanceof AImpl`) vira
    // `Event<usize>` com a contagem de chamadas: o que o caso verifica é a entrega e a contagem.
    macro_rules! event_service {
        () => {
            trait A {
                fn on_did_do_it(&self) -> Event<usize>;
                fn do_it(&self);
                fn noop(&self);
            }
            delayed_proxy!(AProxy for dyn A {
                methods { fn do_it(&self); fn noop(&self); }
                events { fn on_did_do_it(&self) -> usize; }
            });
            service_id!(A_ID: dyn A = "A", proxy AProxy);

            struct AImpl {
                do_it_count: Cell<usize>,
                emitter: Rc<Emitter<usize>>,
            }
            impl A for AImpl {
                fn on_did_do_it(&self) -> Event<usize> {
                    self.emitter.event()
                }
                fn do_it(&self) {
                    self.do_it_count.set(self.do_it_count.get() + 1);
                    self.emitter.fire(self.do_it_count.get());
                }
                fn noop(&self) {}
            }
            impl Injectable for AImpl {
                type Args = Rc<Cell<bool>>;
                type Deps = ();
                fn create(created: Self::Args, _: ()) -> Rc<Self> {
                    created.set(true);
                    Rc::new(Self { do_it_count: Cell::new(0), emitter: Emitter::new() })
                }
            }
            service_impl!(AImpl => dyn A);

            struct Consumer {
                a: Rc<dyn A>,
            }
            impl Injectable for Consumer {
                type Args = ();
                type Deps = (Rc<dyn A>,);
                // eager subscribe -> NO service instance
                fn create(_: (), (a,): Self::Deps) -> Rc<Self> {
                    Rc::new(Self { a })
                }
            }
        };
    }

    #[test]
    fn delayed_and_events() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        event_service!();

        let created = Rc::new(Cell::new(false));
        let insta = InstantiationService::with_options(
            ServiceCollection::new().with(
                A_ID,
                SyncDescriptor::<dyn A>::new::<AImpl>(created.clone()).delayed(),
            ),
            strict_with_tracing(),
        );

        let c = insta.create_instance::<Consumer>(());
        let event_count = Rc::new(Cell::new(0));

        // subscribing to event doesn't trigger instantiation
        let listener = || {
            let event_count = event_count.clone();
            move |_: &usize| event_count.set(event_count.get() + 1)
        };
        let d1 = c.a.on_did_do_it().listen(listener());
        let d2 = c.a.on_did_do_it().listen(listener());
        assert!(!created.get());
        assert_eq!(event_count.get(), 0);
        d2.dispose();

        // instantiation happens on first call
        c.a.do_it();
        assert!(created.get());
        assert_eq!(event_count.get(), 1);

        let d3 = c.a.on_did_do_it().listen(listener());
        c.a.do_it();
        assert_eq!(event_count.get(), 3);

        d1.dispose();
        d3.dispose();
    }

    #[test]
    fn capture_event_before_init_use_after_init() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        event_service!();

        let created = Rc::new(Cell::new(false));
        let insta = InstantiationService::with_options(
            ServiceCollection::new().with(
                A_ID,
                SyncDescriptor::<dyn A>::new::<AImpl>(created.clone()).delayed(),
            ),
            strict_with_tracing(),
        );

        let c = insta.create_instance::<Consumer>(());
        let event_count = Rc::new(Cell::new(0));

        let listener = {
            let event_count = event_count.clone();
            move |_: &usize| event_count.set(event_count.get() + 1)
        };

        let event = c.a.on_did_do_it();

        assert!(!created.get());

        c.a.noop();
        assert!(created.get());

        let d1 = event.listen(listener);

        c.a.do_it();

        // instantiation happens on first call
        assert_eq!(event_count.get(), 1);

        d1.dispose();
    }

    #[test]
    fn dispose_early_event_listener() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        event_service!();

        let created = Rc::new(Cell::new(false));
        let insta = InstantiationService::with_options(
            ServiceCollection::new().with(
                A_ID,
                SyncDescriptor::<dyn A>::new::<AImpl>(created.clone()).delayed(),
            ),
            strict_with_tracing(),
        );

        let c = insta.create_instance::<Consumer>(());
        let event_count = Rc::new(Cell::new(0));

        // subscribing to event doesn't trigger instantiation
        let listener = {
            let event_count = event_count.clone();
            move |_: &usize| event_count.set(event_count.get() + 1)
        };

        let d1 = c.a.on_did_do_it().listen(listener);
        assert!(!created.get());
        assert_eq!(event_count.get(), 0);

        c.a.do_it();

        // instantiation happens on first call
        assert!(created.get());
        assert_eq!(event_count.get(), 1);

        d1.dispose();

        c.a.do_it();
        assert_eq!(event_count.get(), 1);
    }

    #[test]
    fn dispose_services_it_created() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        trait A {
            fn value(&self) -> i32;
        }
        trait B {
            fn value(&self) -> i32;
        }
        delayed_proxy!(AProxy for dyn A {
            methods { fn value(&self) -> i32; }
            events {}
        });
        service_id!(A_ID: dyn A = "A", proxy AProxy);
        service_id!(B_ID: dyn B = "B");

        struct AImpl {
            disposed: Rc<Cell<bool>>,
        }
        impl A for AImpl {
            fn value(&self) -> i32 {
                1
            }
        }
        impl IDisposable for AImpl {
            fn dispose(&self) {
                self.disposed.set(true);
            }
        }
        impl Injectable for AImpl {
            type Args = Rc<Cell<bool>>;
            type Deps = ();
            fn create(disposed: Self::Args, _: ()) -> Rc<Self> {
                Rc::new(Self { disposed })
            }
        }
        service_impl!(AImpl => dyn A, disposable);

        struct BImpl {
            disposed: Rc<Cell<bool>>,
        }
        impl B for BImpl {
            fn value(&self) -> i32 {
                1
            }
        }
        impl IDisposable for BImpl {
            fn dispose(&self) {
                self.disposed.set(true);
            }
        }

        struct Consumer;
        impl Injectable for Consumer {
            type Args = ();
            type Deps = (Rc<dyn A>, Rc<dyn B>);
            fn create(_: (), (a, b): Self::Deps) -> Rc<Self> {
                assert_eq!(a.value(), b.value());
                Rc::new(Self)
            }
        }

        let disposed_a = Rc::new(Cell::new(false));
        let disposed_b = Rc::new(Cell::new(false));
        let b: Rc<dyn B> = Rc::new(BImpl {
            disposed: disposed_b.clone(),
        });

        let insta = InstantiationService::with_options(
            ServiceCollection::new()
                .with(
                    A_ID,
                    SyncDescriptor::<dyn A>::new::<AImpl>(disposed_a.clone()).delayed(),
                )
                .with(B_ID, b),
            strict_with_tracing(),
        );

        let c = insta.create_instance::<Consumer>(());

        insta.dispose();
        drop(c);
        assert!(disposed_a.get());
        assert!(!disposed_b.get());
    }

    trait IB {
        fn value(&self) -> i32;
    }
    service_id!(I_B: dyn IB = "B");

    struct BImpl;
    impl IB for BImpl {
        fn value(&self) -> i32 {
            1
        }
    }

    struct BConsumer;
    impl Injectable for BConsumer {
        type Args = ();
        type Deps = (Rc<dyn IB>,);
        fn create(_: (), (b,): Self::Deps) -> Rc<Self> {
            assert_eq!(b.value(), 1);
            Rc::new(Self)
        }
    }

    #[test]
    fn disposed_service_cannot_be_used_anymore() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let b: Rc<dyn IB> = Rc::new(BImpl);
        let insta = InstantiationService::with_options(
            ServiceCollection::new().with(I_B, b),
            strict_with_tracing(),
        );

        let _c = insta.create_instance::<BConsumer>(());

        insta.dispose();

        assert!(thrown(|| insta.create_instance::<BConsumer>(())).is_some());
        assert!(thrown(|| insta.invoke_function(|_accessor| {})).is_some());
        assert!(thrown(|| insta.create_child(ServiceCollection::new(), None)).is_some());
    }

    #[test]
    fn child_does_not_dispose_parent() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let b: Rc<dyn IB> = Rc::new(BImpl);
        let insta1 = InstantiationService::with_options(
            ServiceCollection::new().with(I_B, b),
            strict_with_tracing(),
        );

        let insta2 = insta1.create_child(ServiceCollection::new(), None);

        insta1.create_instance::<BConsumer>(());
        insta2.create_instance::<BConsumer>(());

        insta2.dispose();

        insta1.create_instance::<BConsumer>(()); // parent NOT disposed by child
        assert!(thrown(|| insta2.create_instance::<BConsumer>(())).is_some());
    }

    #[test]
    fn parent_does_dispose_children() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let b: Rc<dyn IB> = Rc::new(BImpl);
        let insta1 = InstantiationService::with_options(
            ServiceCollection::new().with(I_B, b),
            strict_with_tracing(),
        );

        let insta2 = insta1.create_child(ServiceCollection::new(), None);

        insta1.create_instance::<BConsumer>(());
        insta2.create_instance::<BConsumer>(());

        insta1.dispose();

        assert!(thrown(|| insta2.create_instance::<BConsumer>(())).is_some()); // child is disposed by parent
        assert!(thrown(|| insta1.create_instance::<BConsumer>(())).is_some());
    }
}
