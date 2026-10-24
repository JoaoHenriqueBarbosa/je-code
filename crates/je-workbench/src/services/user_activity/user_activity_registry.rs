//! Porte de `src/vs/workbench/services/userActivity/common/userActivityRegistry.ts`: o registro dos
//! rastreadores de atividade, que só são criados quando o serviço fica ocioso (`take`).
//!
//! Divergência: o construtor `{ new(s: IUserActivityService, ...args) }` do original é um [`Injectable`] cujo
//! argumento estático é o próprio serviço de atividade; o `createInstance(ctor, userActivityService)` vira
//! `create_instance::<C>(service)`. O registro é por thread, como o módulo do original é por processo.

use std::cell::RefCell;
use std::rc::Rc;

use je_instantiation::{Injectable, InstantiationService};

use super::user_activity_service::IUserActivityService;

/// Um construtor registrado: cria o rastreador `C` para o serviço dado.
type Creator = fn(Rc<dyn IUserActivityService>, &InstantiationService);

/// O serviço e o serviço de instanciação entregues a [`take`].
type Taken = (Rc<dyn IUserActivityService>, Rc<InstantiationService>);

#[derive(Default)]
struct UserActivityRegistry {
    todo: Vec<Creator>,
    /// Depois do `take`, o `add` do original passa a criar na hora.
    taken: Option<Taken>,
}

thread_local! {
    static REGISTRY: RefCell<UserActivityRegistry> = RefCell::new(UserActivityRegistry::default());
}

fn create<C: Injectable<Args = Rc<dyn IUserActivityService>>>(service: Rc<dyn IUserActivityService>, instantiation: &InstantiationService) {
    instantiation.create_instance::<C>(service);
}

/// `userActivityRegistry.add(ctor)`.
pub fn add<C: Injectable<Args = Rc<dyn IUserActivityService>>>() {
    let taken = REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let taken = registry.taken.clone();
        if taken.is_none() {
            registry.todo.push(create::<C>);
        }
        taken
    });
    if let Some((service, instantiation)) = taken {
        create::<C>(service, &instantiation);
    }
}

/// `userActivityRegistry.take(userActivityService, instantiation)`: cria os rastreadores pendentes e passa a
/// criar os próximos assim que forem registrados.
pub fn take(service: Rc<dyn IUserActivityService>, instantiation: Rc<InstantiationService>) {
    let todo = REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.taken = Some((service.clone(), instantiation.clone()));
        std::mem::take(&mut registry.todo)
    });
    for creator in todo {
        creator(service.clone(), &instantiation);
    }
}
