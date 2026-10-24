//! Porte de `src/vs/platform/policy/common/multiplexPolicyService.ts`: um serviço de políticas que junta vários
//! outros (o valor de cada política vem do serviço que a define, e a fonte do valor acompanha).
//!
//! Decisões de tradução:
//! - **`extends AbstractPolicyService`** é uma [`AbstractPolicyService`] guardada por composição, como o
//!   [`FilePolicyService`](super::file_policy_service::FilePolicyService) faz; o serviço implementa
//!   [`PolicyService`] e delega o resto ao estado da base.
//! - O `updatePolicyDefinitions` da base é sobrescrito sem chamar o `super`, então o método abstrato
//!   `_updatePolicyDefinitions` da base nunca roda por ela: o [`PolicyUpdater`] que a base exige é vazio, e a
//!   lógica de `_updatePolicyDefinitions` fica no [`PolicyService::update_policy_definitions`] daqui (que é o seu
//!   único chamador).
//! - `this.policyDefinitions[name] = definitions[name]` é [`AbstractPolicyService::set_definition`].
//! - O `Event.any(...)` assina cada serviço, e a assinatura fica presa ao serviço (`_register`). O ouvinte segura
//!   o estado por `Weak`, para um serviço filho não manter o multiplexador vivo.

use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::{self, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;

use super::policy::{AbstractPolicyService, PolicyDefinitions, PolicyService, PolicyUpdater, PolicyValue, PolicyValueSource, PolicyValues, SerializedPolicy};
use crate::log::LogServiceRef;

/// O que o multiplexador e o seu ouvinte de eventos compartilham.
struct Sources {
    policy_services: Vec<Rc<dyn PolicyService>>,
    service: Rc<AbstractPolicyService>,
    log_service: LogServiceRef,
}

impl Sources {
    /// `updatePolicies()`.
    fn update_policies(&self) {
        self.service.clear_policy_values();
        let mut updated: Vec<String> = Vec::new();
        for policy_service in &self.policy_services {
            for (name, definition) in policy_service.policy_definitions() {
                let value = policy_service.get_policy_value(&name);
                self.service.set_definition(&name, definition);
                if let Some(value) = value {
                    let source = policy_service.get_policy_value_source(&name).unwrap_or(PolicyValueSource::Device);
                    self.service.update_policy_value(&name, Some(value), source);
                    updated.push(name);
                }
            }
        }

        // Check that no results have overlapping keys
        let mut changed: HashSet<String> = HashSet::new();
        for key in updated {
            if !changed.insert(key.clone()) {
                self.log_service.warn(&format!("MultiplexPolicyService#_updatePolicyDefinitions - Found overlapping keys in policy services: {key}"));
            }
        }
    }
}

/// `MultiplexPolicyService`.
pub struct MultiplexPolicyService {
    base: Rc<Disposable>,
    sources: Rc<Sources>,
}

impl MultiplexPolicyService {
    pub fn new(policy_services: Vec<Rc<dyn PolicyService>>, log_service: LogServiceRef) -> Rc<Self> {
        let updater: PolicyUpdater = Rc::new(|_| Promise::resolved(()));
        let sources = Rc::new(Sources { policy_services, service: AbstractPolicyService::new(updater), log_service });

        sources.update_policies();
        let base = Disposable::new();
        let weak: Weak<Sources> = Rc::downgrade(&sources);
        let on_did_change: Event<Vec<String>> = event::any(sources.policy_services.iter().map(|service| service.on_did_change()).collect());
        base.register(on_did_change.listen(move |names| {
            if let Some(sources) = weak.upgrade() {
                sources.update_policies();
                sources.service.fire_did_change(names.clone());
            }
        }));

        Rc::new(Self { base, sources })
    }
}

impl PolicyService for MultiplexPolicyService {
    fn on_did_change(&self) -> Event<Vec<String>> {
        self.sources.service.on_did_change()
    }

    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        let sources = self.sources.clone();
        async_fn(async move {
            // `_updatePolicyDefinitions`
            Promise::all(sources.policy_services.iter().map(|service| service.update_policy_definitions(policy_definitions.clone())).collect()).await?;
            sources.update_policies();
            Ok(sources.service.get_policy_values())
        })
    }

    fn get_policy_value(&self, name: &str) -> Option<PolicyValue> {
        self.sources.service.get_policy_value(name)
    }

    fn get_policy_value_source(&self, name: &str) -> Option<PolicyValueSource> {
        self.sources.service.get_policy_value_source(name)
    }

    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>> {
        self.sources.service.serialize()
    }

    fn policy_definitions(&self) -> PolicyDefinitions {
        PolicyService::policy_definitions(&self.sources.service)
    }
}

impl IDisposable for MultiplexPolicyService {
    fn dispose(&self) {
        self.base.dispose();
        self.sources.service.dispose();
    }
}
