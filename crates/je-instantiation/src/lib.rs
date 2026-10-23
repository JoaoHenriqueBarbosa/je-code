//! Porte de `src/vs/platform/instantiation/common` do VS Code: a injeção de dependência.
//!
//! # Desenho
//!
//! O VS Code identifica serviços por `createDecorator<IFoo>('fooService')` e injeta nos parâmetros
//! decorados de um construtor, lendo os decorators por reflexão. Rust não tem nenhum dos dois, então o
//! porte preserva a SEMÂNTICA observável (ordem de criação, criação preguiçosa, mensagem do erro de
//! ciclo, mensagens de serviço ausente, filhos que herdam e sobrescrevem, `dispose` na ordem do
//! original) trocando o mecanismo:
//!
//! - **Identificador**: [`ServiceId<T>`] com `T = dyn Trait`, o nome do `createDecorator` e (opcional)
//!   o proxy da instanciação atrasada. A identidade é nome mais `TypeId`, de modo que dois traits
//!   diferentes com o mesmo nome nunca se confundem (no original o `Map` só por nome os fundiria).
//!   [`service_id!`] declara o identificador e o liga ao trait pelo [`Service`].
//! - **Serviço**: `Rc<dyn Trait>`, de acordo com `docs/design.md` (grafo de objetos num thread). O
//!   registro guarda `Rc<T>` apagado de tipo e o devolve por `downcast`, sem `unsafe`.
//! - **Parâmetros decorados**: o trait [`Injectable`]. `Args` são os argumentos estáticos, `Deps` é
//!   uma tupla de `Rc<dyn Trait>` (ou `Option<Rc<...>>` para dependência opcional) na ordem dos
//!   parâmetros. A tupla é a única declaração e alimenta resolução, grafo e detecção de ciclo, então a
//!   lista de dependências nunca diverge do construtor. O descompasso entre argumentos estáticos e
//!   parâmetros (que o original corrige com `console.trace` e que o `StrictServiceCollection` rejeita)
//!   é erro de compilação.
//! - **Descritor**: [`SyncDescriptor<T>`] (`new::<Impl>(args)`, `.delayed()`), e
//!   [`register_singleton`] com [`InstantiationType`].
//! - **Instanciação atrasada**: o `Proxy` do JavaScript vira um tipo gerado por [`delayed_proxy!`]
//!   para cada trait atrasável. Ele cria o serviço no primeiro uso (ou no tempo ocioso, pelo
//!   `GlobalIdleValue`) e guarda as inscrições em eventos feitas antes disso, religando-as depois.
//! - **Exceções**: viram `je_base::errors::throw` (pânico com `Error`), como em todo o porte.
//!
//! Divergências conhecidas, todas por falta de equivalente em Rust: `get` do accessor lança quando o
//! serviço não existe (o original devolve `undefined` fora do modo estrito; `try_get` e
//! `Option<Rc<_>>` dão a forma original); o `IInstantiationService` é o tipo concreto
//! [`InstantiationService`] e não é gravado na coleção do usuário (evita um ciclo de `Rc`);
//! `dispose` solta da coleção as instâncias que o serviço criou, pelo mesmo motivo.

pub use je_base;

pub mod delayed;
pub mod descriptors;
pub mod extensions;
pub mod graph;
pub mod instantiation;
pub mod instantiation_service;
pub mod service_collection;
pub mod trace;

pub use delayed::DelayedHandle;
pub use descriptors::SyncDescriptor;
pub use extensions::{
    InstantiationType, SingletonRegistration, get_singleton_service_descriptors,
    register_singleton, register_singleton_descriptor,
};
pub use graph::{Graph, Node};
pub use instantiation::{
    Dependencies, Dependency, I_INSTANTIATION_SERVICE, Injectable, ProxyFn, Service, ServiceId,
    ServiceImpl, ServiceKey,
};
pub use instantiation_service::{InstantiationOptions, InstantiationService, ServicesAccessor};
pub use service_collection::{ServiceCollection, ServiceEntry};
pub use trace::Trace;
