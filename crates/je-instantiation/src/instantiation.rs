//! Porte de `instantiation.ts`: identificadores de serviço e o contrato de construção com injeção.
//!
//! O equivalente dos parâmetros decorados (`constructor(v: boolean, @IA a: IA, @IB b: IB)`) é um
//! [`Injectable`]: `Args` são os argumentos estáticos (os que vêm antes dos decorados) e `Deps` é uma
//! tupla de `Rc<dyn Trait>` na ordem dos parâmetros decorados. A tupla é a única declaração das
//! dependências: ela alimenta a resolução, o grafo de criação e a detecção de ciclo.

use std::any::{Any, TypeId};
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::DisposableRef;

use crate::delayed::DelayedHandle;
use crate::instantiation_service::InstantiationService;
use crate::trace::Trace;

/// Uma instância apagada de tipo: por baixo é um `Rc<T>` dentro de outro `Rc`.
pub type ErasedInstance = Rc<dyn Any>;

pub(crate) fn downcast<T: ?Sized + 'static>(instance: &ErasedInstance) -> Rc<T> {
    instance
        .downcast_ref::<Rc<T>>()
        .expect("o tipo do serviço bate com o do identificador")
        .clone()
}

/// O `constructor.name` do original: o último segmento do caminho do tipo, sem genéricos.
pub(crate) fn short_type_name<T: ?Sized>() -> String {
    let full = std::any::type_name::<T>();
    let base = full.split('<').next().unwrap_or(full);
    base.rsplit("::").next().unwrap_or(base).to_string()
}

/// A identidade de um serviço: o nome do `createDecorator` mais o tipo (o original só tem o nome).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ServiceKey {
    pub name: &'static str,
    type_id: TypeId,
}

impl fmt::Display for ServiceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// Como fabricar o proxy de instanciação atrasada de um serviço, ver [`crate::delayed_proxy`].
pub type ProxyFn<T> = fn(DelayedHandle<T>) -> Rc<T>;

/// `ServiceIdentifier<T>`: `T` é em geral `dyn Trait`. Barato de copiar e válido em `const`.
pub struct ServiceId<T: ?Sized + 'static> {
    name: &'static str,
    pub(crate) proxy: Option<ProxyFn<T>>,
    _marker: PhantomData<fn() -> Rc<T>>,
}

impl<T: ?Sized + 'static> Clone for ServiceId<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized + 'static> Copy for ServiceId<T> {}

impl<T: ?Sized + 'static> fmt::Display for ServiceId<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

impl<T: ?Sized + 'static> ServiceId<T> {
    /// `createDecorator<T>(name)`.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            proxy: None,
            _marker: PhantomData,
        }
    }

    /// Como [`ServiceId::new`], e declara o proxy que torna o serviço registrável como `Delayed`.
    pub const fn with_proxy(name: &'static str, proxy: ProxyFn<T>) -> Self {
        Self {
            name,
            proxy: Some(proxy),
            _marker: PhantomData,
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn key(&self) -> ServiceKey {
        ServiceKey {
            name: self.name,
            type_id: TypeId::of::<T>(),
        }
    }
}

/// Liga um tipo de serviço (`dyn Trait`) ao seu identificador, para as dependências serem declaradas
/// só pelo tipo.
pub trait Service: 'static {
    fn id() -> ServiceId<Self>;
}

/// Declara o identificador de um serviço e o liga ao trait.
///
/// ```ignore
/// service_id!(pub I_FOO: dyn IFoo = "fooService");
/// service_id!(pub I_BAR: dyn IBar = "barService", proxy BarProxy);
/// ```
#[macro_export]
macro_rules! service_id {
    ($vis:vis $name:ident : dyn $tr:path = $svc:literal) => {
        $vis const $name: $crate::ServiceId<dyn $tr> = $crate::ServiceId::new($svc);
        impl $crate::Service for dyn $tr {
            fn id() -> $crate::ServiceId<Self> {
                $name
            }
        }
    };
    ($vis:vis $name:ident : dyn $tr:path = $svc:literal, proxy $proxy:path) => {
        $vis const $name: $crate::ServiceId<dyn $tr> = $crate::ServiceId::with_proxy($svc, <$proxy>::wrap);
        impl $crate::Service for dyn $tr {
            fn id() -> $crate::ServiceId<Self> {
                $name
            }
        }
    };
}

/// `IInstantiationService` é o próprio [`InstantiationService`] (os métodos genéricos não cabem num
/// `dyn`). Quem o declara como dependência recebe o serviço que o está construindo.
pub const I_INSTANTIATION_SERVICE: ServiceId<InstantiationService> =
    ServiceId::new("instantiationService");

impl Service for InstantiationService {
    fn id() -> ServiceId<Self> {
        I_INSTANTIATION_SERVICE
    }
}

/// Uma dependência declarada: `Rc<T>` (obrigatória) ou `Option<Rc<T>>` (opcional, `None` quando o
/// serviço não existe e o serviço de instanciação não é estrito, como o `undefined` do original).
pub trait Dependency: Sized {
    fn key() -> ServiceKey;
    #[doc(hidden)]
    fn resolve(insta: &InstantiationService, ctor_name: &str, trace: &Trace) -> Self;
}

impl<T: ?Sized + Service> Dependency for Rc<T> {
    fn key() -> ServiceKey {
        T::id().key()
    }

    fn resolve(insta: &InstantiationService, ctor_name: &str, trace: &Trace) -> Self {
        let key = Self::key();
        match insta.resolve_service_arg(key, ctor_name, trace) {
            Some(service) => downcast(&service),
            // O original passa `undefined` quando não é estrito; um `Rc` não pode ser nulo.
            None => errors::throw(Error::new(format!(
                "[createInstance] {ctor_name} depends on UNKNOWN service {key}."
            ))),
        }
    }
}

impl<T: ?Sized + Service> Dependency for Option<Rc<T>> {
    fn key() -> ServiceKey {
        T::id().key()
    }

    fn resolve(insta: &InstantiationService, ctor_name: &str, trace: &Trace) -> Self {
        insta
            .resolve_service_arg(Self::key(), ctor_name, trace)
            .map(|service| downcast(&service))
    }
}

/// Uma tupla de [`Dependency`], na ordem dos parâmetros decorados.
pub trait Dependencies: Sized {
    fn keys() -> Vec<ServiceKey>;
    #[doc(hidden)]
    fn resolve(insta: &InstantiationService, ctor_name: &str, trace: &Trace) -> Self;
}

macro_rules! tuple_dependencies {
    ($($t:ident),*) => {
        impl<$($t: Dependency),*> Dependencies for ($($t,)*) {
            fn keys() -> Vec<ServiceKey> {
                vec![$($t::key()),*]
            }

            #[allow(unused_variables, clippy::unused_unit)]
            fn resolve(insta: &InstantiationService, ctor_name: &str, trace: &Trace) -> Self {
                ($($t::resolve(insta, ctor_name, trace),)*)
            }
        }
    };
}

tuple_dependencies!();
tuple_dependencies!(A);
tuple_dependencies!(A, B);
tuple_dependencies!(A, B, C);
tuple_dependencies!(A, B, C, D);
tuple_dependencies!(A, B, C, D, E);
tuple_dependencies!(A, B, C, D, E, F);
tuple_dependencies!(A, B, C, D, E, F, G);
tuple_dependencies!(A, B, C, D, E, F, G, H);
tuple_dependencies!(A, B, C, D, E, F, G, H, I);
tuple_dependencies!(A, B, C, D, E, F, G, H, I, J);
tuple_dependencies!(A, B, C, D, E, F, G, H, I, J, K);
tuple_dependencies!(A, B, C, D, E, F, G, H, I, J, K, L);

/// Um tipo construível por `createInstance`.
pub trait Injectable: Sized + 'static {
    /// Os argumentos estáticos (os parâmetros que antecedem os decorados).
    type Args: 'static;
    /// Os serviços injetados.
    type Deps: Dependencies;

    /// O construtor. Devolve `Rc` para permitir `Rc::new_cyclic` em quem guarda `Weak` de si.
    fn create(args: Self::Args, deps: Self::Deps) -> Rc<Self>;
}

/// Um [`Injectable`] que implementa um serviço `Service` (em geral `dyn Trait`).
pub trait ServiceImpl: Injectable {
    type Service: ?Sized + 'static;

    /// A conversão `Rc<Self>` para `Rc<dyn Trait>` (o corpo é só `this`).
    fn upcast(this: Rc<Self>) -> Rc<Self::Service>;

    /// O `isDisposable(candidate)` do `dispose` do serviço de instanciação: quem tem `dispose`
    /// devolve `Some`. O padrão é não ter.
    fn disposable(_this: &Rc<Self>) -> Option<DisposableRef> {
        None
    }
}

/// Implementa [`ServiceImpl`]. A forma `, disposable` registra o objeto para `dispose` junto com o
/// serviço de instanciação que o criou (o tipo precisa implementar `IDisposable`).
#[macro_export]
macro_rules! service_impl {
    ($ty:ty => dyn $tr:path) => {
        impl $crate::ServiceImpl for $ty {
            type Service = dyn $tr;
            fn upcast(this: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn $tr> {
                this
            }
        }
    };
    ($ty:ty => dyn $tr:path, disposable) => {
        impl $crate::ServiceImpl for $ty {
            type Service = dyn $tr;
            fn upcast(this: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn $tr> {
                this
            }
            fn disposable(
                this: &::std::rc::Rc<Self>,
            ) -> Option<$crate::je_base::lifecycle::DisposableRef> {
                let d: $crate::je_base::lifecycle::DisposableRef = this.clone();
                Some(d)
            }
        }
    };
}
