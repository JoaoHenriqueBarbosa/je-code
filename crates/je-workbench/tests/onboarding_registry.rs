//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/onboardingRegistry.test.ts` (suíte
//! `OnboardingScenarioRegistry`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `disposables.add`
//!   vira `guard.add`.
//! - **`getScenarioDescriptors()` de um cenário ativo.** No original devolve o próprio objeto do cenário, e o
//!   `deepStrictEqual` compara também `trigger` e `presentation`. No porte o descritor é a struct
//!   [`OnboardingScenarioDescriptor`] (`id`, `developer_mode_variations`, `tryout`), então o caso confere esses três
//!   campos no descritor e confere `trigger` e `presentation` no cenário que `get_scenario` devolve para o mesmo id.
//! - **`{ kind: 'auto' }`.** É [`OnboardingTrigger::Auto`]; `presentation: { kind: 'test', payload: undefined }` é uma
//!   [`OnboardingPresentationRef`] com `Dynamic::Undefined`.

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

fn variations(items: &[&str]) -> Option<Rc<[String]>> {
    Some(items.iter().map(|item| item.to_string()).collect::<Vec<_>>().into())
}

fn descriptor(id: &str, developer_mode_variations: Option<Rc<[String]>>) -> OnboardingScenarioDescriptor {
    OnboardingScenarioDescriptor { id: id.to_string(), developer_mode_variations, tryout: None }
}

fn scenario(id: &str, developer_mode_variations: Option<Rc<[String]>>) -> OnboardingScenario {
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
    registry.register_descriptor(descriptor("plain", None));
    registry.register_descriptor(descriptor("declared", variations(&["first"])));
    registry.register_descriptor(descriptor("declared", variations(&["second"])));
    let registration = guard.add(registry.register(scenario("active", None)));

    assert_eq!(
        registry.get_scenario_descriptors(),
        vec![descriptor("plain", None), descriptor("declared", variations(&["second"])), descriptor("active", None)]
    );
    let active = registry.get_scenario("active").expect("o cenário ativo está registrado");
    assert!(matches!(active.trigger, OnboardingTrigger::Auto));
    assert_eq!(active.presentation.kind, "test");
    assert!(matches!(active.presentation.payload, Dynamic::Undefined));

    registration.dispose();
    assert_eq!(
        registry.get_scenario_descriptors(),
        vec![descriptor("plain", None), descriptor("declared", variations(&["second"]))]
    );
}

#[test]
fn active_scenarios_override_their_persistent_descriptors() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = OnboardingScenarioRegistry::new();
    registry.register_descriptor(descriptor("dynamic", None));
    guard.add(registry.register(scenario("dynamic", variations(&["active"]))));

    assert_eq!(registry.get_scenario_descriptors(), vec![descriptor("dynamic", variations(&["active"]))]);
    let active = registry.get_scenario("dynamic").expect("o cenário ativo está registrado");
    assert!(matches!(active.trigger, OnboardingTrigger::Auto));
    assert_eq!(active.presentation.kind, "test");
    assert!(matches!(active.presentation.payload, Dynamic::Undefined));
}
