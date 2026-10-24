//! Porte de `src/vs/workbench/contrib/onboarding/common/onboardingRegistry.ts`: o registro de cenários de
//! onboarding. As áreas de funcionalidade registram os cenários aqui (dados puros); o motor lê dele para decidir o
//! que rodar.
//!
//! Divergências:
//! - `IOnboardingScenarioDescriptor = Pick<IOnboardingScenario, 'id' | 'developerModeVariations' | 'tryout'>` é a
//!   struct [`OnboardingScenarioDescriptor`] com esses três campos. `getScenarioDescriptors` devolve, para um
//!   cenário ativo, o descritor dele (o original devolve o próprio objeto do cenário, que o tipo `Pick` esconde):
//!   [`OnboardingScenarioDescriptor::from`] copia o `id` e as duas referências (`Rc`, identidade preservada).
//! - `previous.developerModeVariations === descriptor.developerModeVariations && previous.tryout ===
//!   descriptor.tryout` (comparação por identidade) é `Rc::ptr_eq` nos dois campos (ausente com ausente é igual).
//! - O `Map` do original (ordem de inserção; trocar o valor de uma chave existente mantém a posição) é um `Vec` de
//!   pares com a mesma regra.
//! - `register` devolve o `{ dispose }` literal do original (um disposable sem rastreamento).
//! - O registro global (`onboardingScenarioRegistry`, criado na carga do módulo) é [`onboarding_scenario_registry`],
//!   uma instância por thread, criada no primeiro uso (Rust não tem efeito colateral na carga do módulo).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, plain_disposable};

use super::onboarding_scenario::OnboardingScenario;
use super::onboarding_tryout::OnboardingTryoutMetadata;

/// `IOnboardingScenarioDescriptor`: os metadados persistentes de um cenário cuja instância ativa pode ser criada
/// sob demanda.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OnboardingScenarioDescriptor {
    pub id: String,
    pub developer_mode_variations: Option<Rc<[String]>>,
    pub tryout: Option<Rc<OnboardingTryoutMetadata>>,
}

impl From<&OnboardingScenario> for OnboardingScenarioDescriptor {
    fn from(scenario: &OnboardingScenario) -> Self {
        Self {
            id: scenario.id.clone(),
            developer_mode_variations: scenario.developer_mode_variations.clone(),
            tryout: scenario.tryout.clone(),
        }
    }
}

/// `IOnboardingScenarioRegistry`.
pub trait IOnboardingScenarioRegistry {
    /// Registra um cenário. Lança se já existir um cenário com o mesmo id.
    fn register(&self, scenario: OnboardingScenario) -> DisposableRef;
    /// Registra metadados persistentes de um cenário cuja instância ativa pode ser criada sob demanda.
    fn register_descriptor(&self, descriptor: OnboardingScenarioDescriptor);
    /// Todos os cenários registrados agora.
    fn get_scenarios(&self) -> Vec<Rc<OnboardingScenario>>;
    /// Todos os descritores conhecidos, incluindo os cenários registrados agora.
    fn get_scenario_descriptors(&self) -> Vec<OnboardingScenarioDescriptor>;
    /// Busca um cenário pelo id.
    fn get_scenario(&self, id: &str) -> Option<Rc<OnboardingScenario>>;
    /// Dispara quando cenários entram ou saem, ou quando descritores entram ou são atualizados.
    fn on_did_change(&self) -> Event<()>;
}

/// Um `Map<string, V>` na ordem de inserção: trocar o valor de uma chave que já existe mantém a posição.
type OrderedMap<V> = Vec<(String, V)>;

fn ordered_set<V>(map: &mut OrderedMap<V>, key: &str, value: V) {
    match map.iter_mut().find(|(existing, _)| existing == key) {
        Some(entry) => entry.1 = value,
        None => map.push((key.to_string(), value)),
    }
}

fn ordered_get<'a, V>(map: &'a OrderedMap<V>, key: &str) -> Option<&'a V> {
    map.iter().find(|(existing, _)| existing == key).map(|(_, value)| value)
}

/// `undefined === undefined` ou a mesma referência.
fn same_reference<T: ?Sized>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

struct RegistryState {
    scenarios: RefCell<OrderedMap<Rc<OnboardingScenario>>>,
    descriptors: RefCell<OrderedMap<OnboardingScenarioDescriptor>>,
    on_did_change: Rc<Emitter<()>>,
}

/// `OnboardingScenarioRegistry`.
pub struct OnboardingScenarioRegistry {
    state: Rc<RegistryState>,
}

impl Default for OnboardingScenarioRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingScenarioRegistry {
    pub fn new() -> Self {
        Self {
            state: Rc::new(RegistryState {
                scenarios: RefCell::new(Vec::new()),
                descriptors: RefCell::new(Vec::new()),
                on_did_change: Emitter::new(),
            }),
        }
    }
}

impl IOnboardingScenarioRegistry for OnboardingScenarioRegistry {
    fn register(&self, scenario: OnboardingScenario) -> DisposableRef {
        let id = scenario.id.clone();
        if ordered_get(&self.state.scenarios.borrow(), &id).is_some() {
            errors::throw(Error::new(&format!("An onboarding scenario with id '{id}' is already registered.")));
        }
        let scenario = Rc::new(scenario);
        ordered_set(&mut self.state.scenarios.borrow_mut(), &id, scenario.clone());
        self.state.on_did_change.fire(());
        let state = self.state.clone();
        plain_disposable(move || {
            let is_current = ordered_get(&state.scenarios.borrow(), &id).is_some_and(|current| Rc::ptr_eq(current, &scenario));
            if is_current {
                state.scenarios.borrow_mut().retain(|(existing, _)| *existing != id);
                state.on_did_change.fire(());
            }
        })
    }

    fn register_descriptor(&self, descriptor: OnboardingScenarioDescriptor) {
        let unchanged = ordered_get(&self.state.descriptors.borrow(), &descriptor.id).is_some_and(|previous| {
            same_reference(&previous.developer_mode_variations, &descriptor.developer_mode_variations)
                && same_reference(&previous.tryout, &descriptor.tryout)
        });
        if unchanged {
            return;
        }
        let id = descriptor.id.clone();
        ordered_set(&mut self.state.descriptors.borrow_mut(), &id, descriptor);
        self.state.on_did_change.fire(());
    }

    fn get_scenarios(&self) -> Vec<Rc<OnboardingScenario>> {
        self.state.scenarios.borrow().iter().map(|(_, scenario)| scenario.clone()).collect()
    }

    fn get_scenario_descriptors(&self) -> Vec<OnboardingScenarioDescriptor> {
        let mut descriptors = self.state.descriptors.borrow().clone();
        for (_, scenario) in self.state.scenarios.borrow().iter() {
            ordered_set(&mut descriptors, &scenario.id, OnboardingScenarioDescriptor::from(scenario.as_ref()));
        }
        descriptors.into_iter().map(|(_, descriptor)| descriptor).collect()
    }

    fn get_scenario(&self, id: &str) -> Option<Rc<OnboardingScenario>> {
        ordered_get(&self.state.scenarios.borrow(), id).cloned()
    }

    fn on_did_change(&self) -> Event<()> {
        self.state.on_did_change.event()
    }
}

thread_local! {
    static ONBOARDING_SCENARIO_REGISTRY: Rc<OnboardingScenarioRegistry> = Rc::new(OnboardingScenarioRegistry::new());
}

/// `onboardingScenarioRegistry`: o registro global de cenários. Os arquivos de dados dos cenários se registram
/// aqui.
pub fn onboarding_scenario_registry() -> Rc<dyn IOnboardingScenarioRegistry> {
    ONBOARDING_SCENARIO_REGISTRY.with(Rc::clone)
}
