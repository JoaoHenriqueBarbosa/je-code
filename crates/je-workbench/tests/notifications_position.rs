//! Porte de `src/vs/workbench/test/browser/notificationsPosition.test.ts` (suíte `Notifications Position`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `await configurationService.setUserConfiguration(...)`: o [`TestConfigurationService`] devolve uma promessa
//!   já resolvida e grava o valor de forma síncrona, então o caso descarta a promessa e lê em seguida (o `await`
//!   do original só cede uma microtarefa).
//! - `getValue<NotificationsPosition>(...)` devolve o valor da configuração como [`Dynamic`]; o `?? BOTTOM_RIGHT`
//!   do primeiro caso troca o valor ausente pelo texto do padrão e a comparação é entre textos [`Dynamic`].
//! - `Codicon.chevronDown.id` e `Codicon.chevronUp.id` são as constantes de `codicon` do módulo das ações:
//!   o `Codicon` ainda não existe como tipo no `je-workbench` (ver `notifications_actions.rs`).
//! - Os ícones que o original devolve como `ThemeIcon` são [`RegisteredIcon`], e o `.id` é o campo `id`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_platform::configuration::service::IConfigurationService;
use je_platform::window::DEFAULT_CUSTOM_TITLEBAR_HEIGHT;
use je_workbench::browser::parts::notifications::notifications_actions::{
    HIDE_ICON, HIDE_UP_ICON, RegisteredIcon, codicon, get_notification_collapse_icon, get_notification_expand_icon,
};
use je_workbench::common::notifications::{NotificationsPosition, NotificationsSettings};
use je_platform::configuration::test_configuration_service::TestConfigurationService;

fn position_value(configuration_service: &TestConfigurationService) -> Dynamic {
    configuration_service.get_value(Some(NotificationsSettings::NotificationsPosition.as_str()), None)
}

// Configuration

#[test]
fn defaults_to_bottom_right_when_no_configuration_is_set() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service = TestConfigurationService::new(None);
    let value = position_value(&configuration_service);
    let position = if value.is_nullish() { Dynamic::from(NotificationsPosition::BottomRight.as_str()) } else { value };
    assert_eq!(position, Dynamic::from(NotificationsPosition::BottomRight.as_str()));
}

#[test]
fn returns_bottom_left_when_configured() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service = TestConfigurationService::new(None);
    drop(configuration_service.set_user_configuration(
        NotificationsSettings::NotificationsPosition.as_str(),
        Dynamic::from(NotificationsPosition::BottomLeft.as_str()),
        None,
    ));
    let position = position_value(&configuration_service);
    assert_eq!(position, Dynamic::from(NotificationsPosition::BottomLeft.as_str()));
}

#[test]
fn returns_top_right_when_configured() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service = TestConfigurationService::new(None);
    drop(configuration_service.set_user_configuration(
        NotificationsSettings::NotificationsPosition.as_str(),
        Dynamic::from(NotificationsPosition::TopRight.as_str()),
        None,
    ));
    let position = position_value(&configuration_service);
    assert_eq!(position, Dynamic::from(NotificationsPosition::TopRight.as_str()));
}

#[test]
fn returns_bottom_right_when_configured() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service = TestConfigurationService::new(None);
    drop(configuration_service.set_user_configuration(
        NotificationsSettings::NotificationsPosition.as_str(),
        Dynamic::from(NotificationsPosition::BottomRight.as_str()),
        None,
    ));
    let position = position_value(&configuration_service);
    assert_eq!(position, Dynamic::from(NotificationsPosition::BottomRight.as_str()));
}

// Status Bar Alignment

fn get_desired_alignment(position: NotificationsPosition) -> &'static str {
    match position {
        NotificationsPosition::BottomLeft => "left",
        NotificationsPosition::TopRight => "hidden", // bell is in titlebar instead
        NotificationsPosition::BottomRight => "right",
    }
}

#[test]
fn bottom_right_position_aligns_bell_to_right() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_desired_alignment(NotificationsPosition::BottomRight), "right");
}

#[test]
fn bottom_left_position_aligns_bell_to_left() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_desired_alignment(NotificationsPosition::BottomLeft), "left");
}

#[test]
fn top_right_position_hides_status_bar_bell() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_desired_alignment(NotificationsPosition::TopRight), "hidden");
}

// Top Offset for Top-Right

fn compute_top_offset(position: NotificationsPosition, title_bar_visible: bool) -> Option<u32> {
    if position != NotificationsPosition::TopRight {
        return None;
    }
    let mut top_offset = 7;
    if title_bar_visible {
        top_offset += DEFAULT_CUSTOM_TITLEBAR_HEIGHT;
    }
    Some(top_offset)
}

#[test]
fn bottom_right_has_no_top_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(compute_top_offset(NotificationsPosition::BottomRight, true), None);
}

#[test]
fn bottom_left_has_no_top_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(compute_top_offset(NotificationsPosition::BottomLeft, true), None);
}

#[test]
fn top_right_without_titlebar_has_7px_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(compute_top_offset(NotificationsPosition::TopRight, false), Some(7));
}

#[test]
fn top_right_with_titlebar_has_42px_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(compute_top_offset(NotificationsPosition::TopRight, true), Some(42));
}

// NotificationsPosition Enum Values

#[test]
fn enum_values_match_expected_strings() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(NotificationsPosition::BottomRight.as_str(), "bottom-right");
    assert_eq!(NotificationsPosition::BottomLeft.as_str(), "bottom-left");
    assert_eq!(NotificationsPosition::TopRight.as_str(), "top-right");
}

#[test]
fn setting_key_is_correct() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(NotificationsSettings::NotificationsPosition.as_str(), "workbench.notifications.position");
}

#[test]
fn button_setting_key_is_correct() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(NotificationsSettings::NotificationsButton.as_str(), "workbench.notifications.showInTitleBar");
}

// Hide Notifications Icon

fn get_hide_icon(position: NotificationsPosition) -> RegisteredIcon {
    if position == NotificationsPosition::TopRight { HIDE_UP_ICON } else { HIDE_ICON }
}

#[test]
fn bottom_right_uses_chevron_down_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_hide_icon(NotificationsPosition::BottomRight).id, HIDE_ICON.id);
}

#[test]
fn bottom_left_uses_chevron_down_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_hide_icon(NotificationsPosition::BottomLeft).id, HIDE_ICON.id);
}

#[test]
fn top_right_uses_chevron_up_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_hide_icon(NotificationsPosition::TopRight).id, HIDE_UP_ICON.id);
}

#[test]
fn hide_icon_defaults_use_correct_codicons() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(codicon::CHEVRON_DOWN, "chevron-down");
    assert_eq!(codicon::CHEVRON_UP, "chevron-up");
}

// Expand/Collapse Notification Icons

#[test]
fn bottom_right_expand_uses_notifications_expand_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_notification_expand_icon(NotificationsPosition::BottomRight).id, "notifications-expand");
}

#[test]
fn bottom_left_expand_uses_notifications_expand_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_notification_expand_icon(NotificationsPosition::BottomLeft).id, "notifications-expand");
}

#[test]
fn top_right_expand_uses_notifications_expand_down_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_notification_expand_icon(NotificationsPosition::TopRight).id, "notifications-expand-down");
}

#[test]
fn bottom_right_collapse_uses_notifications_collapse_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_notification_collapse_icon(NotificationsPosition::BottomRight).id, "notifications-collapse");
}

#[test]
fn bottom_left_collapse_uses_notifications_collapse_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_notification_collapse_icon(NotificationsPosition::BottomLeft).id, "notifications-collapse");
}

#[test]
fn top_right_collapse_uses_notifications_collapse_up_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_notification_collapse_icon(NotificationsPosition::TopRight).id, "notifications-collapse-up");
}
