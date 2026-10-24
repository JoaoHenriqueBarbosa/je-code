//! Porte de `src/vs/workbench/services/filesConfiguration/test/browser/filesConfigurationService.test.ts` (suíte
//! `FilesConfigurationService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso e o `setup` ([`setup`]) logo depois; o [`Fixture`] descarta o `disposables` do original no `Drop`, antes
//!   do guarda conferir os vazamentos (o `Drop` dele roda depois, por ter sido declarado primeiro).
//! - **`workbenchInstantiationService(undefined, disposables)`.** Só os serviços de que o `FilesConfigurationService`
//!   depende são criados, com os mesmos dublês do original: `MockContextKeyService` (aqui o `ContextKeyService` real,
//!   como no teste do `snippet_controller2`), `TestConfigurationService` com `files.participants.timeout`, o
//!   `TestContextService` com o `TestWorkspace`, o `TestEnvironmentService`, o `TestFileService`, o `TestMarkerService` e o
//!   `TestTextResourceConfigurationService`. O `UriIdentityService` real ainda não foi portado: o
//!   [`TestUriIdentityService`] devolve o `ExtUri` sem viés e `asCanonicalUri` identidade, que é o que o serviço usa dele.
//! - **Casos `async`** rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`new MarkdownString(value)`** é a [`markdown_string`], com os campos do `IMarkdownString` (o `MarkdownString` do
//!   original é passado como `IMarkdownString` por tipagem estrutural); `trustedReason.isTrusted = true` é o campo
//!   `is_trusted` ajustado.
//! - **`typeof readonly === 'object' ? readonly.value : undefined`** é [`reason_of`] e `!!readonly` é
//!   `Readonly::is_readonly`.
//! - **`updateReadonly(resource, 'reset' | true | false)`** é `update_readonly` com [`ReadonlyUpdate`]; com um array,
//!   `update_readonly_all`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::html_content::{IMarkdownString, IsTrusted, MarkdownString, TrustedOrOptions};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::resources::{host_environment, uri_file};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly, ReadonlyUpdate};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{
    TestContextService, TestFileService, TestFilesConfigurationService, TestMarkerService, TestTextResourceConfigurationService,
};

/// Um `IUriIdentityService` com o `ExtUri` sem viés.
struct TestUriIdentityService {
    ext_uri: ExtUri,
}

impl IUriIdentityService for TestUriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        self.ext_uri.clone()
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// O que o `setup` do original monta: o `disposables` e o `service`.
struct Fixture {
    disposables: Rc<DisposableStore>,
    service: Rc<TestFilesConfigurationService>,
}

impl Drop for Fixture {
    /// O `teardown`: `disposables.clear()`.
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

fn setup() -> Fixture {
    let disposables = DisposableStore::new();

    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(Some(Dynamic::object([(
        "files",
        Dynamic::object([("participants", Dynamic::object([("timeout", Dynamic::from(60000))]))]),
    )])));
    let context_key_service = disposables.add_rc(ContextKeyService::new(configuration_service.clone()));
    let file_service = disposables.add_rc(TestFileService::new());

    let service = disposables.add_rc(TestFilesConfigurationService::create(
        (),
        (
            context_key_service,
            configuration_service.clone(),
            TestContextService::new(None, None),
            TestEnvironmentService::new(),
            Rc::new(TestUriIdentityService { ext_uri: ExtUri::unbiased(host_environment()) }),
            file_service,
            Rc::new(TestMarkerService),
            TestTextResourceConfigurationService::new(configuration_service),
        ),
    ));

    Fixture { disposables, service }
}

/// `new MarkdownString(value)`.
fn markdown_string(value: &str) -> IMarkdownString {
    let markdown = MarkdownString::new(Some(JsString::from(value)), TrustedOrOptions::Trusted(false));
    IMarkdownString {
        value: markdown.value,
        is_trusted: markdown.is_trusted,
        support_theme_icons: markdown.support_theme_icons,
        support_html: markdown.support_html,
        support_alert_syntax: markdown.support_alert_syntax,
        base_uri: markdown.base_uri,
        uris: markdown.uris,
    }
}

/// `typeof readonly === 'object' ? readonly.value : undefined`.
fn reason_of(readonly: &Readonly) -> Option<JsString> {
    match readonly {
        Readonly::Markdown(markdown) => Some(markdown.value.clone()),
        Readonly::Bool(_) => None,
    }
}

fn is_false(readonly: &Readonly) -> bool {
    matches!(readonly, Readonly::Bool(false))
}

#[test]
fn update_readonly_with_a_custom_reason_returns_that_reason() {
    run_with_faked_timers(async {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resource = uri_file("/test/file.txt");
        let reason = markdown_string("Custom read-only reason");

        service.update_readonly(&resource, ReadonlyUpdate::Markdown(reason.clone()).into()).await.unwrap();

        let readonly = service.is_readonly(&resource, None);
        assert_eq!(reason_of(&readonly), Some(reason.value));
    });
}

#[test]
fn update_readonly_with_true_returns_the_default_session_reason() {
    run_with_faked_timers(async {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resource = uri_file("/test/file.txt");
        let custom_reason = markdown_string("Custom read-only reason");

        service.update_readonly(&resource, ReadonlyUpdate::True.into()).await.unwrap();

        let readonly = service.is_readonly(&resource, None);
        assert!(matches!(&readonly, Readonly::Markdown(markdown) if !markdown.value.is_empty() && markdown.value != custom_reason.value));
    });
}

#[test]
fn update_readonly_reset_clears_a_custom_reason() {
    run_with_faked_timers(async {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resource = uri_file("/test/file.txt");

        service.update_readonly(&resource, ReadonlyUpdate::Markdown(markdown_string("Custom read-only reason")).into()).await.unwrap();
        service.update_readonly(&resource, ReadonlyUpdate::Reset.into()).await.unwrap();

        assert!(is_false(&service.is_readonly(&resource, None)));
    });
}

#[test]
fn update_readonly_with_an_array_applies_a_custom_reason_to_every_resource() {
    run_with_faked_timers(async {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resources = [uri_file("/test/file1.txt"), uri_file("/test/file2.txt")];
        let reason = markdown_string("Custom read-only reason");

        service.update_readonly_all(&resources, ReadonlyUpdate::Markdown(reason.clone())).await.unwrap();

        let reasons: Vec<Option<JsString>> = resources.iter().map(|resource| reason_of(&service.is_readonly(resource, None))).collect();
        assert_eq!(reasons, vec![Some(reason.value.clone()), Some(reason.value)]);
    });
}

#[test]
fn update_readonly_with_single_resource_fires_on_did_change_readonly_once() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resource = uri_file("/test/file.txt");
        let event_count = Rc::new(Cell::new(0));
        let counter = event_count.clone();
        disposables.add(service.on_did_change_readonly().listen(move |_| counter.set(counter.get() + 1)));

        service.update_readonly(&resource, ReadonlyUpdate::True.into()).await.unwrap();

        assert_eq!(event_count.get(), 1);
        assert!(service.is_readonly(&resource, None).is_readonly());
    });
}

#[test]
fn update_readonly_with_array_of_resources_fires_on_did_change_readonly_once() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resources = [uri_file("/test/file1.txt"), uri_file("/test/file2.txt"), uri_file("/test/file3.txt")];
        let event_count = Rc::new(Cell::new(0));
        let counter = event_count.clone();
        disposables.add(service.on_did_change_readonly().listen(move |_| counter.set(counter.get() + 1)));

        service.update_readonly_all(&resources, ReadonlyUpdate::True).await.unwrap();

        assert_eq!(event_count.get(), 1);
        for resource in &resources {
            assert!(service.is_readonly(resource, None).is_readonly());
        }
    });
}

#[test]
fn update_readonly_with_empty_array_does_not_fire_on_did_change_readonly() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let event_count = Rc::new(Cell::new(0));
        let counter = event_count.clone();
        disposables.add(service.on_did_change_readonly().listen(move |_| counter.set(counter.get() + 1)));

        service.update_readonly_all(&[], ReadonlyUpdate::True).await.unwrap();

        assert_eq!(event_count.get(), 0);
    });
}

#[test]
fn update_readonly_only_notifies_when_the_session_override_changes() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resource = uri_file("/test/file.txt");
        let event_count = Rc::new(Cell::new(0));
        let mut event_counts: Vec<usize> = Vec::new();
        let counter = event_count.clone();
        disposables.add(service.on_did_change_readonly().listen(move |_| counter.set(counter.get() + 1)));

        for readonly in [
            ReadonlyUpdate::Reset,
            ReadonlyUpdate::True,
            ReadonlyUpdate::True,
            ReadonlyUpdate::False,
            ReadonlyUpdate::False,
            ReadonlyUpdate::Reset,
            ReadonlyUpdate::Reset,
        ] {
            service.update_readonly(&resource, readonly.into()).await.unwrap();
            event_counts.push(event_count.get());
        }

        assert_eq!(event_counts, vec![0, 1, 1, 2, 2, 3, 3]);
    });
}

#[test]
fn update_readonly_with_arrays_only_notifies_for_changed_overrides_and_applies_every_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let first = uri_file("/test/file1.txt");
        let second = uri_file("/test/file2.txt");
        let third = uri_file("/test/file3.txt");
        let event_count = Rc::new(Cell::new(0));
        let mut event_counts: Vec<usize> = Vec::new();
        let counter = event_count.clone();
        disposables.add(service.on_did_change_readonly().listen(move |_| counter.set(counter.get() + 1)));

        service.update_readonly(&first, ReadonlyUpdate::True.into()).await.unwrap();
        for readonly in [ReadonlyUpdate::True, ReadonlyUpdate::True, ReadonlyUpdate::False, ReadonlyUpdate::False, ReadonlyUpdate::Reset, ReadonlyUpdate::Reset] {
            service.update_readonly_all(&[first.clone(), second.clone(), third.clone(), first.clone()], readonly).await.unwrap();
            event_counts.push(event_count.get());
        }

        assert_eq!(event_counts, vec![2, 2, 3, 3, 4, 4]);
        for resource in [&first, &second, &third] {
            assert!(is_false(&service.is_readonly(resource, None)));
        }
    });
}

#[test]
fn update_readonly_only_notifies_for_changes_to_a_custom_reason() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resource = uri_file("/test/file.txt");
        let event_count = Rc::new(Cell::new(0));
        let mut event_counts: Vec<usize> = Vec::new();
        let counter = event_count.clone();
        disposables.add(service.on_did_change_readonly().listen(move |_| counter.set(counter.get() + 1)));
        let mut trusted_reason = markdown_string("Updated read-only reason");
        trusted_reason.is_trusted = Some(IsTrusted::Bool(true));

        for reason in [
            markdown_string("Custom read-only reason"),
            markdown_string("Custom read-only reason"),
            markdown_string("Updated read-only reason"),
            trusted_reason.clone(),
            trusted_reason,
        ] {
            service.update_readonly(&resource, ReadonlyUpdate::Markdown(reason).into()).await.unwrap();
            event_counts.push(event_count.get());
        }

        assert_eq!(event_counts, vec![1, 1, 2, 3, 3]);
    });
}

#[test]
fn update_readonly_with_array_supports_reset() {
    run_with_faked_timers(async {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resources = [uri_file("/test/file1.txt"), uri_file("/test/file2.txt")];

        service.update_readonly_all(&resources, ReadonlyUpdate::True).await.unwrap();
        for resource in &resources {
            assert!(service.is_readonly(resource, None).is_readonly());
        }

        service.update_readonly_all(&resources, ReadonlyUpdate::Reset).await.unwrap();
        for resource in &resources {
            assert!(is_false(&service.is_readonly(resource, None)));
        }
    });
}

#[test]
fn multiple_single_update_readonly_calls_fire_on_did_change_readonly_multiple_times() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let Fixture { service, .. } = &setup();

        let resources = [uri_file("/test/file1.txt"), uri_file("/test/file2.txt"), uri_file("/test/file3.txt")];
        let event_count = Rc::new(Cell::new(0));
        let counter = event_count.clone();
        disposables.add(service.on_did_change_readonly().listen(move |_| counter.set(counter.get() + 1)));

        for resource in &resources {
            service.update_readonly(resource, ReadonlyUpdate::True.into()).await.unwrap();
        }

        assert_eq!(event_count.get(), 3);
    });
}
