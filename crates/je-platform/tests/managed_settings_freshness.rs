//! Porte de `src/vs/platform/policy/test/common/managedSettingsFreshness.test.ts` (suíte `Managed settings
//! freshness`), caso a caso e na mesma ordem.
//!
//! Divergências: o objeto de resultado do `deepStrictEqual` é uma tupla (na ordem dos campos do original),
//! e os literais de estado são as variantes de `IManagedSettingsFreshness`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::policy::copilot_managed_settings::ManagedSettingsChannel;
use je_platform::policy::managed_settings_freshness::{
    IManagedSettingsFreshness, IManagedSettingsFreshnessEffective, IManagedSettingsFreshnessScope, MANAGED_SETTINGS_FRESHNESS_NOT_REQUIRED, ManagedSettingsFreshnessFailure,
    is_managed_settings_freshness_blocking, is_managed_settings_freshness_satisfied_for,
};

fn scope() -> IManagedSettingsFreshnessScope {
    IManagedSettingsFreshnessScope { account_id: "account-1".to_owned(), authentication_provider_id: "github".to_owned(), endpoint_origin: "https://api.github.com".to_owned() }
}

fn satisfied() -> IManagedSettingsFreshness {
    IManagedSettingsFreshness::Satisfied { source: ManagedSettingsChannel::NativeMdm, scope: scope(), last_attempt_at: None, satisfied_at: 1 }
}

fn effective(source: ManagedSettingsChannel) -> IManagedSettingsFreshnessEffective {
    IManagedSettingsFreshnessEffective { source, scope: None, last_attempt_at: None }
}

#[test]
fn only_an_unresolved_or_failed_refresh_withholds_agent_functionality() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // (notRequired, pending, satisfied, blocked)
    assert_eq!(
        (
            is_managed_settings_freshness_blocking(&MANAGED_SETTINGS_FRESHNESS_NOT_REQUIRED),
            is_managed_settings_freshness_blocking(&IManagedSettingsFreshness::Pending(effective(ManagedSettingsChannel::Server))),
            is_managed_settings_freshness_blocking(&satisfied()),
            is_managed_settings_freshness_blocking(&IManagedSettingsFreshness::Blocked { effective: effective(ManagedSettingsChannel::Server), failure: ManagedSettingsFreshnessFailure::Network }),
        ),
        // Pending gates: an unresolved refresh must never read as permission to proceed.
        (false, true, false, true)
    );
}

#[test]
fn satisfaction_is_scoped_to_one_account_provider_and_endpoint() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // (sameScope, otherAccount, otherProvider, otherEndpoint, pendingNeverSatisfies)
    assert_eq!(
        (
            is_managed_settings_freshness_satisfied_for(&satisfied(), &scope()),
            is_managed_settings_freshness_satisfied_for(&satisfied(), &IManagedSettingsFreshnessScope { account_id: "account-2".to_owned(), ..scope() }),
            is_managed_settings_freshness_satisfied_for(&satisfied(), &IManagedSettingsFreshnessScope { authentication_provider_id: "github-enterprise".to_owned(), ..scope() }),
            is_managed_settings_freshness_satisfied_for(&satisfied(), &IManagedSettingsFreshnessScope { endpoint_origin: "https://ghe.example.com".to_owned(), ..scope() }),
            is_managed_settings_freshness_satisfied_for(&IManagedSettingsFreshness::Pending(effective(ManagedSettingsChannel::NativeMdm)), &scope()),
        ),
        (true, false, false, false, false)
    );
}
