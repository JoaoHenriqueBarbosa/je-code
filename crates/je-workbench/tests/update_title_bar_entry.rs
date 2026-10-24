//! Porte de `src/vs/workbench/contrib/update/test/browser/updateTitleBarEntry.test.ts`, caso a caso e na mesma ordem
//! (suítes `UpdateGlobalActivityBadgeVisibleContext` e `UpdateTitleBarVisibleContexts`).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso.
//! - **`class TestContextKeyService extends MockContextKeyService`.** Herança vira composição: [`TestContextKeyService`]
//!   guarda o [`MockContextKeyService`], repassa `createKey` e implementa o `contextMatchesRules` sobrescrito, que é
//!   `rules.evaluate({ getValue: key => this.getContextKeyValue(key) })` ([`ServiceContext`] é esse objeto).
//! - **`X.bindTo(contextKeyService).set(v)`.** É `RawContextKey::bind_to` e `set` com o valor como [`Dynamic`];
//!   `contextKeyService.createKey(key, value)` é `create_key`.
//! - **`isMacintosh` e `isWeb`.** Saem de [`Platform::detect`], o mesmo que alimenta as chaves de plataforma.
//! - **`assert.deepStrictEqual(actual, expected)`.** São `assert_eq!` sobre os vetores de `(nome, visível)` e sobre a tupla
//!   `(additional, editor)`.
//!
//! Suítes que ficaram de fora (dependem de DOM, que o porte não tem, e estão no `updateTitleBarEntry.test.ts`):
//! - `UpdateTitleBarEntry` (`Show or Focus Hover focuses the tooltip while Tab remains unhandled`): renderiza o item no
//!   `mainWindow.document`, despacha um `KeyboardEvent` de `Tab` e confere o `defaultPrevented`.
//! - `UpdateTooltip` (`removes hidden actions from the tab order`): consulta o DOM do tooltip com
//!   `querySelectorAll('button, [tabindex]')` e lê `className`, `style.display` e `tabIndex`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::platform::Platform;
use je_platform::contextkey::contextkey::{ContextKeyExpression, IContext};
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_workbench::common::contextkeys::in_editor_zen_mode_context;
use je_workbench::contrib::update::browser::update_title_bar_entry::get_additional_update_title_bar_menu_when;
use je_workbench::contrib::update::common::update::{
    update_global_activity_badge_visible_context, update_title_bar_chat_in_progress_context, update_title_bar_context,
    update_title_bar_editor_visible_context,
};

/// O `{ getValue: key => this.getContextKeyValue(key) }` do `contextMatchesRules` sobrescrito.
struct ServiceContext(Rc<MockContextKeyService>);

impl IContext for ServiceContext {
    fn get_value(&self, key: &str) -> Dynamic {
        self.0.get_context_key_value(key)
    }
}

/// `TestContextKeyService`.
struct TestContextKeyService {
    inner: Rc<MockContextKeyService>,
}

impl TestContextKeyService {
    fn new() -> Self {
        Self { inner: MockContextKeyService::new() }
    }

    fn create_key(&self, key: &str, value: Dynamic) {
        self.inner.create_key(key, value);
    }

    /// `override contextMatchesRules(rules)`.
    fn context_matches_rules(&self, rules: &ContextKeyExpression) -> bool {
        rules.evaluate(&ServiceContext(self.inner.clone()))
    }
}

struct Scenario {
    name: &'static str,
    update_visible: bool,
    menu_bar_visibility: &'static str,
    activity_bar_location: &'static str,
    chat_in_progress: Option<bool>,
    expected: bool,
}

#[test]
fn update_global_activity_badge_visible_context_hides_the_badge_when_the_update_and_manage_actions_are_adjacent() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let platform = Platform::detect();
    let custom_menu_bar_can_be_hidden = !platform.is_macintosh || platform.is_web;
    let scenario = |name, update_visible, menu_bar_visibility, activity_bar_location, chat_in_progress, expected| Scenario {
        name,
        update_visible,
        menu_bar_visibility,
        activity_bar_location,
        chat_in_progress,
        expected,
    };
    let scenarios = [
        scenario("no update", false, "visible", "top", None, true),
        scenario("adjacent", true, "visible", "top", None, false),
        scenario("classic menu", true, "classic", "top", None, false),
        scenario("hidden menu", true, "hidden", "top", None, custom_menu_bar_can_be_hidden),
        scenario("toggle menu", true, "toggle", "top", None, custom_menu_bar_can_be_hidden),
        scenario("compact menu", true, "compact", "top", None, custom_menu_bar_can_be_hidden),
        scenario("bottom activity bar", true, "visible", "bottom", None, true),
        scenario("chat in progress", true, "visible", "top", Some(true), true),
    ];

    let actual: Vec<(&str, bool)> = scenarios
        .iter()
        .map(|scenario| {
            let context_key_service = TestContextKeyService::new();
            update_title_bar_context().bind_to(&*context_key_service.inner).set(Dynamic::from(scenario.update_visible));
            update_title_bar_chat_in_progress_context().bind_to(&*context_key_service.inner).set(Dynamic::from(scenario.chat_in_progress.unwrap_or(false)));
            in_editor_zen_mode_context().bind_to(&*context_key_service.inner);
            context_key_service.create_key("config.window.menuBarVisibility", Dynamic::from(scenario.menu_bar_visibility));
            context_key_service.create_key("config.workbench.activityBar.location", Dynamic::from(scenario.activity_bar_location));

            (scenario.name, context_key_service.context_matches_rules(&update_global_activity_badge_visible_context()))
        })
        .collect();

    let expected: Vec<(&str, bool)> = scenarios.iter().map(|scenario| (scenario.name, scenario.expected)).collect();
    assert_eq!(actual, expected);
}

#[test]
fn update_title_bar_visible_contexts_shows_an_additional_placement_during_an_active_chat_while_the_editor_hides_it() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_key_service = TestContextKeyService::new();
    update_title_bar_context().bind_to(&*context_key_service.inner).set(Dynamic::from(true));
    update_title_bar_chat_in_progress_context().bind_to(&*context_key_service.inner).set(Dynamic::from(true));
    in_editor_zen_mode_context().bind_to(&*context_key_service.inner).set(Dynamic::from(false));
    context_key_service.create_key("inDebugMode", Dynamic::from(false));

    assert_eq!(
        (
            context_key_service.context_matches_rules(&get_additional_update_title_bar_menu_when(None)),
            context_key_service.context_matches_rules(&update_title_bar_editor_visible_context()),
        ),
        (true, false),
    );
}
