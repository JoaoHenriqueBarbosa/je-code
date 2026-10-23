//! Porte de `descriptors.ts`: `SyncDescriptor`, a criação preguiçosa com argumentos estáticos.

use std::rc::Rc;

use je_base::lifecycle::DisposableRef;

use crate::instantiation::{
    Dependencies, ErasedInstance, ServiceId, ServiceImpl, ServiceKey, short_type_name,
};
use crate::instantiation_service::InstantiationService;
use crate::trace::Trace;

/// O que a construção devolve: o serviço e, se tem `dispose`, o handle para o `dispose` do dono.
pub(crate) struct Built<T: ?Sized> {
    pub service: Rc<T>,
    pub disposable: Option<DisposableRef>,
}

pub(crate) type BuildFn<T> = Rc<dyn Fn(&InstantiationService, &Trace) -> Built<T>>;

pub(crate) type InstantiateFn = Box<dyn Fn(&InstantiationService, &Trace) -> ErasedInstance>;

/// O descritor sem o tipo do serviço, o que o serviço de instanciação guarda.
pub(crate) struct ErasedDescriptor {
    pub ctor_name: String,
    pub dependencies: Vec<ServiceKey>,
    pub instantiate: InstantiateFn,
}

/// `SyncDescriptor<T>`. Os argumentos estáticos são tipados por `C::Args`, então o descompasso entre
/// argumentos e parâmetros (que o original corrige com um `console.trace`) não compila.
///
/// ```compile_fail
/// use je_instantiation::*;
/// use std::rc::Rc;
/// trait IFoo {}
/// struct Foo(bool);
/// impl IFoo for Foo {}
/// impl Injectable for Foo {
///     type Args = bool;
///     type Deps = ();
///     fn create(args: bool, _: ()) -> Rc<Self> { Rc::new(Foo(args)) }
/// }
/// service_impl!(Foo => dyn IFoo);
/// // falta o argumento estático: erro de compilação
/// let _ = SyncDescriptor::<dyn IFoo>::new::<Foo>(());
/// ```
pub struct SyncDescriptor<T: ?Sized + 'static> {
    ctor_name: String,
    dependencies: Vec<ServiceKey>,
    supports_delayed_instantiation: bool,
    build: BuildFn<T>,
}

impl<T: ?Sized + 'static> Clone for SyncDescriptor<T> {
    fn clone(&self) -> Self {
        Self {
            ctor_name: self.ctor_name.clone(),
            dependencies: self.dependencies.clone(),
            supports_delayed_instantiation: self.supports_delayed_instantiation,
            build: self.build.clone(),
        }
    }
}

impl<T: ?Sized + 'static> SyncDescriptor<T> {
    /// `new SyncDescriptor(ctor, staticArguments)`.
    pub fn new<C: ServiceImpl<Service = T>>(args: C::Args) -> Self
    where
        C::Args: Clone,
    {
        let ctor_name = short_type_name::<C>();
        let name = ctor_name.clone();
        let build: BuildFn<T> = Rc::new(move |insta, trace| {
            let deps = <C::Deps as Dependencies>::resolve(insta, &name, trace);
            let instance = C::create(args.clone(), deps);
            let disposable = C::disposable(&instance);
            Built {
                service: C::upcast(instance),
                disposable,
            }
        });
        Self {
            ctor_name,
            dependencies: <C::Deps as Dependencies>::keys(),
            supports_delayed_instantiation: false,
            build,
        }
    }

    /// `supportsDelayedInstantiation`: o serviço só é criado no primeiro uso.
    #[must_use]
    pub fn delayed(mut self) -> Self {
        self.supports_delayed_instantiation = true;
        self
    }

    pub fn supports_delayed_instantiation(&self) -> bool {
        self.supports_delayed_instantiation
    }

    pub(crate) fn build(&self) -> BuildFn<T> {
        self.build.clone()
    }

    pub(crate) fn ctor_name(&self) -> &str {
        &self.ctor_name
    }

    pub(crate) fn erase(&self, id: ServiceId<T>) -> ErasedDescriptor {
        let build = self.build.clone();
        let delayed = self.supports_delayed_instantiation;
        let proxy = id.proxy;
        let name = id.name();
        ErasedDescriptor {
            ctor_name: self.ctor_name.clone(),
            dependencies: self.dependencies.clone(),
            instantiate: Box::new(move |owner, trace| {
                let service: Rc<T> = if delayed {
                    owner.create_delayed_service(
                        name,
                        proxy.expect("proxy conferido no registro"),
                        build.clone(),
                        trace,
                    )
                } else {
                    let built = build(owner, trace);
                    owner.add_to_dispose_bucket(built.disposable);
                    built.service
                };
                Rc::new(service)
            }),
        }
    }
}
