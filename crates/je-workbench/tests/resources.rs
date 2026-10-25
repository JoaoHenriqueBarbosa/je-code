//! Porte de `src/vs/workbench/test/common/resources.test.ts` (suíte `ResourceGlobMatcher`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` cria o `TestContextService` (com o workspace padrão do teste) e o
//!   `TestConfigurationService` com as expressões `**/*.md` e `**/*.txt`; o `teardown` é o descarte do
//!   [`LeakGuard`](common::LeakGuard), que reprova se algo ficou sem descartar (`ensureNoDisposablesAreLeakedInTestSuite`).
//! - **`getExpression`.** `() => configurationService.getValue(SETTING)` devolve `undefined` quando o valor sumiu, que é o
//!   `None` do porte.
//! - **`onDidChangeConfigurationEmitter.fire({ affectsConfiguration: key => key === SETTING } as any)`.** O literal é o
//!   [`ConfigurationChangeEvent`] real com a chave alterada (`configuration_event_support`), que responde `true` a `SETTING`.
//! - **`URI.file`.** É `uri_file` do ambiente do processo; `await` é `.await.unwrap()` num caso do relógio virtual.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod configuration_event_support;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_base::uri::URI;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::resources::uri_file;
use je_platform::workspace::IWorkspaceContextService;
use je_workbench::common::resources::{GetExpression, ResourceGlobMatcher, ShouldUpdate};
use je_workbench::test::common::workbench_test_services::TestContextService;

const SETTING: &str = "test.matcher";

/// `{ '**/*.md': true, '**/*.txt': false }`.
fn initial_expression() -> Dynamic {
    Dynamic::object([(SETTING, Dynamic::object([("**/*.md", Dynamic::Bool(true)), ("**/*.txt", Dynamic::Bool(false))]))])
}

#[test]
fn basics() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(None, None);
        let configuration_service = TestConfigurationService::new(Some(initial_expression()));

        let get_expression: GetExpression = {
            let configuration_service = configuration_service.clone();
            Rc::new(move |_folder: Option<&URI>| {
                let value = configuration_service.get_value(Some(SETTING), None);
                if value.is_undefined() { None } else { Some(value) }
            })
        };
        let should_update: ShouldUpdate = Rc::new(|event: &ConfigurationChangeEvent| event.affects_configuration(SETTING, None));
        let matcher = disposables.add_rc(ResourceGlobMatcher::new(get_expression, should_update, context_service, configuration_service.clone()));

        // Matching
        assert!(!matcher.matches(&uri_file("/foo/bar"), None));
        assert!(matcher.matches(&uri_file("/foo/bar.md"), None));
        assert!(!matcher.matches(&uri_file("/foo/bar.txt"), None));

        // Events
        let event_counter = Rc::new(Cell::new(0));
        disposables.add(matcher.on_expression_change().listen({
            let event_counter = event_counter.clone();
            move |_| event_counter.set(event_counter.get() + 1)
        }));

        configuration_service.set_user_configuration(SETTING, Dynamic::object([("**/*.foo", Dynamic::Bool(true))]), None).await.unwrap();
        fire_configuration_change(&configuration_service, &[SETTING]);
        assert_eq!(event_counter.get(), 1);

        assert!(!matcher.matches(&uri_file("/foo/bar.md"), None));
        assert!(matcher.matches(&uri_file("/foo/bar.foo"), None));

        configuration_service.set_user_configuration(SETTING, Dynamic::Undefined, None).await.unwrap();
        fire_configuration_change(&configuration_service, &[SETTING]);
        assert_eq!(event_counter.get(), 2);

        assert!(!matcher.matches(&uri_file("/foo/bar.md"), None));
        assert!(!matcher.matches(&uri_file("/foo/bar.foo"), None));

        configuration_service
            .set_user_configuration(
                SETTING,
                Dynamic::object([
                    ("**/*.md", Dynamic::Bool(true)),
                    ("**/*.txt", Dynamic::Bool(false)),
                    ("C:/bar/**", Dynamic::Bool(true)),
                    ("/bar/**", Dynamic::Bool(true)),
                ]),
                None,
            )
            .await
            .unwrap();
        fire_configuration_change(&configuration_service, &[SETTING]);

        assert!(matcher.matches(&uri_file("/bar/foo.1"), None));
        assert!(matcher.matches(&uri_file("C:/bar/foo.1"), None));
    });
}
