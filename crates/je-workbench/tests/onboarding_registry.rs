//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/onboardingRegistry.test.ts` (suíte
//! `OnboardingScenarioRegistry`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `const disposables = ensureNoDisposablesAreLeakedInTestSuite()` e `disposables.add(...)` viram o guarda criado
//!   no começo de cada caso e `guard.add(...)`.
//! - `IOnboardingScenarioDescriptor` é `Pick<IOnboardingScenario, 'id' | 'developerModeVariations' | 'tryout'>`: o
//!   `deepStrictEqual` de `getScenarioDescriptors()` com o objeto do cenário ativo vira a comparação com o descritor
//!   dele (`OnboardingScenarioDescriptor::from`), que carrega os mesmos três campos.
//! - `presentation: { kind: 'test', payload: undefined }` é `Dynamic::Undefined`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_workbench::contrib::onboarding::onboarding_registry::{
    IOnboardingScenarioRegistry, OnboardingScenarioDescriptor, OnboardingScenarioRegistry,
};
use je_workbench::contrib::onboarding::onboarding_scenario::{
    OnboardingPresentationRef, OnboardingScenario, OnboardingTrigger,
};

fn variations(values: &[&str]) -> Option<Rc<[String]>> {
    Some(values.iter().map(ToString::to_string).collect())
}

fn auto_scenario(id: &str, developer_mode_variations: Option<Rc<[String]>>) -> OnboardingScenario {
    OnboardingScenario {
        id: id.to_string(),
        seen_key: None,
        when: None,
        trigger: OnboardingTrigger::Auto,
        priority: None,
        presentation: OnboardingPresentationRef { kind: "test".to_string(), payload: Dynamic::Undefined },
        tryout: None,
        experiment: None,
        developer_mode_variations,
        repeatable: None,
    }
}

#[test]
fn includes_declared_and_active_scenarios_in_the_descriptor_catalog() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = OnboardingScenarioRegistry::new();
    registry.register_descriptor(OnboardingScenarioDescriptor { id: "plain".to_string(), ..Default::default() });
    registry.register_descriptor(OnboardingScenarioDescriptor {
        id: "declared".to_string(),
        developer_mode_variations: variations(&["first"]),
        ..Default::default()
    });
    registry.register_descriptor(OnboardingScenarioDescriptor {
        id: "declared".to_string(),
        developer_mode_variations: variations(&["second"]),
        ..Default::default()
    });
    let active = auto_scenario("active", None);
    let active_descriptor = OnboardingScenarioDescriptor::from(&active);
    let registration = guard.add(registry.register(active));

    assert_eq!(
        registry.get_scenario_descriptors(),
        vec![
            OnboardingScenarioDescriptor { id: "plain".to_string(), ..Default::default() },
            OnboardingScenarioDescriptor {
                id: "declared".to_string(),
                developer_mode_variations: variations(&["second"]),
                ..Default::default()
            },
            active_descriptor,
        ]
    );

    registration.dispose();
    assert_eq!(
        registry.get_scenario_descriptors(),
        vec![
            OnboardingScenarioDescriptor { id: "plain".to_string(), ..Default::default() },
            OnboardingScenarioDescriptor {
                id: "declared".to_string(),
                developer_mode_variations: variations(&["second"]),
                ..Default::default()
            },
        ]
    );
}

#[test]
fn active_scenarios_override_their_persistent_descriptors() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = OnboardingScenarioRegistry::new();
    registry.register_descriptor(OnboardingScenarioDescriptor { id: "dynamic".to_string(), ..Default::default() });
    let scenario = auto_scenario("dynamic", variations(&["active"]));
    let scenario_descriptor = OnboardingScenarioDescriptor::from(&scenario);
    guard.add(registry.register(scenario));

    assert_eq!(registry.get_scenario_descriptors(), vec![scenario_descriptor]);
}
