//! Porte de `src/vs/workbench/contrib/extensions/test/electron-browser/extensionsWorkbenchService.test.ts` (suíte `ExtensionsWorkbenchServiceTest`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `setup`, `stubConfiguration`, `aLocalExtension`, `aGalleryExtension`, `aPage`, `aMultiExtensionManagementServerService`
//!   e `createExtensionManagementService` do original, e os dublês de serviço, estão em
//!   `support::extensions_workbench_service_support` (ver a nota do módulo, que explica cada diferença: um serviço só para
//!   `IExtensionManagementService` e `IWorkbenchExtensionManagementService`, a mutação no lugar do `updateMetadata`, o
//!   `IProgressService` que executa a tarefa, o `uninstallExtensions` que resolve, os serviços que o `setup` não registra).
//! - `await`/`Promise` dos casos `async` rodam em `spawn_local` no laço `run`; `Event.toPromise(onChange)` é
//!   `on_change().to_promise(None)`.
//! - `sinon.spy()` é o `Spy` (conta as chamadas); um `target` que o original cria e nunca liga a nada
//!   (`assert.ok(!target.called)`) continua um `Spy` solto.
//! - `testObject.local[i]` é `test_object.local()[i]`; `extension.local?.pinned` é `extension.local().pinned` (o campo é
//!   `bool` no porte, então o `undefined` do original vale `false`).
//! - `await testObject.canInstall(x)` devolve `Ok(())` para o `true` do original e `Err(mensagem)` para o `IMarkdownString`.
//! - Os casos de desabilitar uma extensão com dependentes (`test disable extension fails if extension is a dependent of
//!   other`, `... if its dependencys dependency is itself` e `... with cyclic dependencies`) registram um
//!   `INotificationService.prompt` que chama `onCancel`; a produção pergunta pelo `IDialogService`, que o `setup` não
//!   registra, e é essa chamada que falha no original. Aqui o `StubDialogService` sem resposta rejeita do mesmo modo, e o
//!   `prompt` do `INotificationService` não tem quem o chame.
//! - Os casos `test updateAutoUpdateEnablementFor throws error ...` têm o `assert.fail` dentro do `try`, e o `catch` o
//!   captura: passam com qualquer resultado, e o porte conserva isso.
//! - **Não portados:**
//!   - `suite('installExtension waits for installed inventory')`, os seis casos (`newer pre-release`, `newer release`,
//!     `same release`, `older release`, `disabled pre-release` e `missing extension`): chamam
//!     `CommandsRegistry.getCommand('workbench.extensions.installExtension')` e `instantiationService.invokeFunction`, e o
//!     comando é registrado por `extensions.contribution.ts`, que o porte ainda não tem; sem ele não há o handler que o
//!     caso invoca, e reimplementar o handler no teste testaria outra coisa.
//!   - `test delayed auto update does not reschedule the marketplace update check`: usa `runWithFakedTimers({ useFakeTimers:
//!     true, startTime: 1000 * 60 * 60 * 3 })` e `timeout(...)` do relógio virtual; o `with_faked_timers` do porte começa o
//!     relógio em 0 e não aceita `startTime`, e o caso depende de `Date.now()` valer 3 horas (`lastUpdated: Date.now() - 1h`
//!     vira um instante antes da época).

mod support;

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::paging::IPager;
use je_base::runtime;
use je_extensions::management::{GalleryExtension, GalleryExtensionAssets, InstallOperation, QueryOptions};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionType};
use je_platform::product::ProductService;
use je_platform::resources::uri_file;
use je_workbench::contrib::extensions::extensions::{ExtensionRuntimeActionType, ExtensionState, IExtension};
use je_workbench::contrib::extensions::extensions_service::{AutoUpdateConfigurationValue, ExtensionOrPublisher};
use je_workbench::services::extension_management::extension_install_service::{InstallExtensionEvent, UninstallExtensionEvent};
use je_workbench::services::extension_management::extension_management::{
    DidUninstallExtensionEvent, EnablementState, ExtensionRef, InstallExtensionResult, InstallExtensionSource,
};
use je_workbench::test::common::workbench_test_services::TestProductService;
use serde_json::{Value, json};
use support::extension_enablement_support::uri_with_scheme;
use support::extensions_workbench_service_support::{
    GalleryProperties, LocalProperties, Spy, StubDialogService, WorkbenchFixture, a_gallery_extension, a_gallery_extension_props, a_gallery_extension_with,
    a_local_extension, a_local_extension_with, a_running_extension, asset, run,
};

use EnablementState as E;

const HOUR_MS: f64 = 1000.0 * 60.0 * 60.0;

/// `Date.now()`.
fn now() -> f64 {
    runtime::date_now() as f64
}

/// O profile que os eventos do original deixam `null!`.
fn profile() -> je_base::uri::URI {
    uri_file("profile")
}

/// `{ identifier, source: gallery, profileLocation: null! }` (`InstallExtensionEvent`).
fn install_event(gallery: &GalleryExtension) -> InstallExtensionEvent {
    InstallExtensionEvent {
        identifier: gallery.identifier.clone(),
        source: InstallExtensionSource::Gallery(gallery.clone()),
        profile_location: profile(),
        application_scoped: None,
        workspace_scoped: None,
    }
}

/// `{ identifier, profileLocation: null! }` (`UninstallExtensionEvent`).
fn uninstall_event(identifier: &ExtensionIdentifierInfo) -> UninstallExtensionEvent {
    UninstallExtensionEvent { identifier: identifier.clone(), profile_location: profile(), application_scoped: None, workspace_scoped: None }
}

/// `{ identifier, profileLocation: null! }` (`DidUninstallExtensionEvent`).
fn did_uninstall_event(identifier: &ExtensionIdentifierInfo) -> DidUninstallExtensionEvent {
    DidUninstallExtensionEvent { identifier: identifier.clone(), profile_location: None, error: None, workspace_scoped: None, server: None }
}

/// `{ identifier, source, operation, local, profileLocation: null! }` (`InstallExtensionResult`).
fn did_install_result(identifier: &ExtensionIdentifierInfo, source: Option<InstallExtensionSource>, operation: InstallOperation, local: ExtensionRef) -> InstallExtensionResult {
    InstallExtensionResult { identifier: identifier.clone(), operation, source, profile_location: Some(profile()), local: Some(local), workspace_scoped: None }
}

/// `aLocalExtension(gallery.name, gallery, { identifier })`: a extensão local com o identificador da galeria.
fn local_of(gallery: &GalleryExtension) -> ExtensionRef {
    a_local_extension_with(&gallery.name, json!({}), LocalProperties { identifier: Some(gallery.identifier.clone()), ..LocalProperties::default() })
}

/// `aLocalExtension(name, { extensionDependencies })`.
fn depending_on(name: &str, dependencies: &[&str]) -> ExtensionRef {
    a_local_extension_with(name, json!({ "extensionDependencies": dependencies }), LocalProperties::default())
}

/// `aLocalExtension(name, { extensionPack })`.
fn packing(name: &str, pack: &[&str]) -> ExtensionRef {
    a_local_extension_with(name, json!({ "extensionPack": pack }), LocalProperties::default())
}

/// `aLocalExtension(name, {}, { type })`.
fn of_type(name: &str, extension_type: ExtensionType) -> ExtensionRef {
    a_local_extension_with(name, json!({}), LocalProperties { extension_type: Some(extension_type), ..LocalProperties::default() })
}

/// `aLocalExtension('a', { extensionKind }, { location })`.
fn with_kinds(kinds: &[&str], location: je_base::uri::URI) -> ExtensionRef {
    a_local_extension_with("a", json!({ "extensionKind": kinds }), LocalProperties { location: Some(location), ..LocalProperties::default() })
}

/// `URI.file('pub.a')`.
fn local_location() -> je_base::uri::URI {
    uri_file("pub.a")
}

/// `URI.file('pub.a').with({ scheme: Schemas.vscodeRemote })`.
fn remote_location() -> je_base::uri::URI {
    uri_with_scheme("pub.a", je_base::network::schemas::VSCODE_REMOTE)
}

/// `instantiationService.get(IWorkbenchExtensionEnablementService).setEnablement(extensions, state)`.
async fn set_enablement(f: &WorkbenchFixture, extensions: &[&ExtensionRef], state: EnablementState) {
    f.enablement_service().set_enablement(extensions.iter().map(|extension| (*extension).clone()).collect(), state).await.expect("setEnablement resolve");
}

/// `instantiationService.stub(IExtensionManagementServerService, aMultiExtensionManagementServerService(...))` e
/// `instantiationService.stub(IWorkbenchExtensionEnablementService, disposableStore.add(new TestExtensionEnablementService(instantiationService)))`.
fn use_servers(f: &WorkbenchFixture, local: Vec<ExtensionRef>, remote: Vec<ExtensionRef>) {
    let service = f.a_multi_extension_management_server_service(local, remote);
    f.stub_server_service(service);
    f.restub_enablement_service();
}

/// Os `assets` de `test gallery extension` e de `test installed extensions get syncs with gallery`.
fn full_assets() -> GalleryExtensionAssets {
    GalleryExtensionAssets {
        manifest: Some(asset("uri:manifest", "fallback:manifest")),
        readme: Some(asset("uri:readme", "fallback:readme")),
        changelog: Some(asset("uri:changelog", "fallback:changlog")),
        license: Some(asset("uri:license", "fallback:license")),
        repository: Some(asset("uri:repository", "fallback:repository")),
        download: asset("uri:download", "fallback:download"),
        icon: Some(asset("uri:icon", "fallback:icon")),
        signature: Some(asset("uri:signature", "fallback:signature")),
        core_translations: Vec::new(),
    }
}

/// `queryGallery(CancellationToken.None)`.
async fn query_first_page(test_object: &je_workbench::contrib::extensions::extensions_service::ExtensionsWorkbenchService) -> Rc<dyn IPager<Rc<je_workbench::contrib::extensions::extensions_workbench_service::Extension>>> {
    test_object.query_gallery(QueryOptions::default(), CancellationToken::None).await.expect("queryGallery resolve")
}

#[test]
fn test_gallery_extension() {
    run(|f| async move {
        let expected = a_gallery_extension_with(
            "expectedName",
            GalleryProperties {
                display_name: Some("expectedDisplayName"),
                version: Some("1.5.0"),
                publisher_id: Some("expectedPublisherId"),
                publisher: Some("expectedPublisher"),
                publisher_display_name: Some("expectedPublisherDisplayName"),
                description: Some("expectedDescription"),
                install_count: Some(1000.0),
                rating: Some(4.0),
                rating_count: Some(100.0),
                ..GalleryProperties::default()
            },
            Some(&["pub.1", "pub.2"]),
            Some(full_assets()),
        );

        let test_object = f.a_workbench_service().await;
        f.gallery.stub_query(vec![expected]);

        let paged_response = query_first_page(&test_object).await;
        assert_eq!(1, paged_response.first_page().len());
        let actual = paged_response.first_page()[0].clone();

        assert_eq!(ExtensionType::User, actual.extension_type());
        assert_eq!("expectedName", actual.name());
        assert_eq!("expectedDisplayName", actual.display_name());
        assert_eq!("expectedpublisher.expectedname", actual.identifier().id);
        assert_eq!("expectedPublisher", actual.publisher());
        assert_eq!("expectedPublisherDisplayName", actual.publisher_display_name());
        assert_eq!("1.5.0", actual.version());
        assert_eq!("1.5.0", actual.latest_version());
        assert_eq!("expectedDescription", actual.description());
        assert_eq!(Some("uri:icon"), actual.icon_url().as_deref());
        assert_eq!(Some("fallback:icon"), actual.icon_url_fallback().as_deref());
        assert_eq!(Some("uri:license"), actual.license_url().as_deref());
        assert_eq!(ExtensionState::Uninstalled, actual.state());
        assert_eq!(Some(1000.0), actual.install_count());
        assert_eq!(Some(4.0), actual.rating());
        assert_eq!(Some(100.0), actual.rating_count());
        assert!(!actual.outdated());
        assert_eq!(vec!["pub.1".to_owned(), "pub.2".to_owned()], actual.dependencies());
    });
}

#[test]
fn test_for_empty_installed_extensions() {
    run(|f| async move {
        let test_object = f.a_workbench_service().await;

        assert!(test_object.local().is_empty());
    });
}

#[test]
fn test_for_installed_extensions() {
    run(|f| async move {
        let expected1 = a_local_extension_with(
            "local1",
            json!({
                "publisher": "localPublisher1",
                "version": "1.1.0",
                "displayName": "localDisplayName1",
                "description": "localDescription1",
                "icon": "localIcon1",
                "extensionDependencies": ["pub.1", "pub.2"],
            }),
            LocalProperties {
                extension_type: Some(ExtensionType::User),
                readme_url: Some("localReadmeUrl1"),
                changelog_url: Some("localChangelogUrl1"),
                location: Some(uri_file("localPath1")),
                ..LocalProperties::default()
            },
        );
        let expected2 = a_local_extension_with(
            "local2",
            json!({
                "publisher": "localPublisher2",
                "version": "1.2.0",
                "displayName": "localDisplayName2",
                "description": "localDescription2",
            }),
            LocalProperties {
                extension_type: Some(ExtensionType::System),
                readme_url: Some("localReadmeUrl2"),
                changelog_url: Some("localChangelogUrl2"),
                ..LocalProperties::default()
            },
        );
        f.management.stub_installed(vec![expected1, expected2]);
        let test_object = f.a_workbench_service().await;

        let actuals = test_object.local();
        assert_eq!(2, actuals.len());

        let actual = &actuals[0];
        assert_eq!(ExtensionType::User, actual.extension_type());
        assert_eq!("local1", actual.name());
        assert_eq!("localDisplayName1", actual.display_name());
        assert_eq!("localpublisher1.local1", actual.identifier().id);
        assert_eq!("localPublisher1", actual.publisher());
        assert_eq!("1.1.0", actual.version());
        assert_eq!("1.1.0", actual.latest_version());
        assert_eq!("localDescription1", actual.description());
        let icon_url = actual.icon_url().expect("a extensão tem ícone");
        assert!(icon_url == "file:///localPath1/localIcon1" || icon_url == "vscode-file://vscode-app/localPath1/localIcon1", "ícone inesperado: {icon_url}");
        assert!(actual.icon_url_fallback().is_none());
        assert_eq!(None, actual.license_url());
        assert_eq!(ExtensionState::Installed, actual.state());
        assert_eq!(None, actual.install_count());
        assert_eq!(None, actual.rating());
        assert_eq!(None, actual.rating_count());
        assert!(!actual.outdated());
        assert_eq!(vec!["pub.1".to_owned(), "pub.2".to_owned()], actual.dependencies());

        let actual = &actuals[1];
        assert_eq!(ExtensionType::System, actual.extension_type());
        assert_eq!("local2", actual.name());
        assert_eq!("localDisplayName2", actual.display_name());
        assert_eq!("localpublisher2.local2", actual.identifier().id);
        assert_eq!("localPublisher2", actual.publisher());
        assert_eq!("1.2.0", actual.version());
        assert_eq!("1.2.0", actual.latest_version());
        assert_eq!("localDescription2", actual.description());
        assert_eq!(None, actual.license_url());
        assert_eq!(ExtensionState::Installed, actual.state());
        assert_eq!(None, actual.install_count());
        assert_eq!(None, actual.rating());
        assert_eq!(None, actual.rating_count());
        assert!(!actual.outdated());
        assert!(actual.dependencies().is_empty());
    });
}

#[test]
fn test_installed_extensions_get_syncs_with_gallery() {
    run(|f| async move {
        let local1 = a_local_extension_with(
            "local1",
            json!({
                "publisher": "localPublisher1",
                "version": "1.1.0",
                "displayName": "localDisplayName1",
                "description": "localDescription1",
                "icon": "localIcon1",
                "extensionDependencies": ["pub.1", "pub.2"],
            }),
            LocalProperties {
                extension_type: Some(ExtensionType::User),
                readme_url: Some("localReadmeUrl1"),
                changelog_url: Some("localChangelogUrl1"),
                location: Some(uri_file("localPath1")),
                ..LocalProperties::default()
            },
        );
        let local2 = a_local_extension_with(
            "local2",
            json!({
                "publisher": "localPublisher2",
                "version": "1.2.0",
                "displayName": "localDisplayName2",
                "description": "localDescription2",
            }),
            LocalProperties {
                extension_type: Some(ExtensionType::System),
                readme_url: Some("localReadmeUrl2"),
                changelog_url: Some("localChangelogUrl2"),
                ..LocalProperties::default()
            },
        );
        let gallery1 = a_gallery_extension_with(
            &local1.manifest.name,
            GalleryProperties {
                display_name: Some("expectedDisplayName"),
                version: Some("1.5.0"),
                publisher_id: Some("expectedPublisherId"),
                publisher: Some(local1.manifest.publisher.as_str()),
                publisher_display_name: Some("expectedPublisherDisplayName"),
                description: Some("expectedDescription"),
                install_count: Some(1000.0),
                rating: Some(4.0),
                rating_count: Some(100.0),
                ..GalleryProperties::default()
            },
            Some(&["pub.1"]),
            Some(full_assets()),
        );
        f.management.stub_installed(vec![local1, local2]);
        f.gallery.stub_query(vec![gallery1.clone()]);
        f.gallery.stub_get_compatible_extension(gallery1.clone());
        f.gallery.stub_get_extensions(vec![gallery1]);
        let test_object = f.a_workbench_service().await;
        test_object.query_local(None).await.expect("queryLocal resolve");

        test_object.on_change().to_promise(None).await.expect("onChange dispara");
        let actuals = test_object.local();
        assert_eq!(2, actuals.len());

        let actual = &actuals[0];
        assert_eq!(ExtensionType::User, actual.extension_type());
        assert_eq!("local1", actual.name());
        assert_eq!("expectedDisplayName", actual.display_name());
        assert_eq!("localpublisher1.local1", actual.identifier().id);
        assert_eq!("localPublisher1", actual.publisher());
        assert_eq!("1.1.0", actual.version());
        assert_eq!("1.5.0", actual.latest_version());
        assert_eq!("expectedDescription", actual.description());
        assert_eq!(Some("uri:icon"), actual.icon_url().as_deref());
        assert_eq!(Some("fallback:icon"), actual.icon_url_fallback().as_deref());
        assert_eq!(ExtensionState::Installed, actual.state());
        assert_eq!(Some("uri:license"), actual.license_url().as_deref());
        assert_eq!(Some(1000.0), actual.install_count());
        assert_eq!(Some(4.0), actual.rating());
        assert_eq!(Some(100.0), actual.rating_count());
        assert!(actual.outdated());
        assert_eq!(vec!["pub.1".to_owned()], actual.dependencies());

        let actual = &actuals[1];
        assert_eq!(ExtensionType::System, actual.extension_type());
        assert_eq!("local2", actual.name());
        assert_eq!("localDisplayName2", actual.display_name());
        assert_eq!("localpublisher2.local2", actual.identifier().id);
        assert_eq!("localPublisher2", actual.publisher());
        assert_eq!("1.2.0", actual.version());
        assert_eq!("1.2.0", actual.latest_version());
        assert_eq!("localDescription2", actual.description());
        assert_eq!(None, actual.license_url());
        assert_eq!(ExtensionState::Installed, actual.state());
        assert_eq!(None, actual.install_count());
        assert_eq!(None, actual.rating());
        assert_eq!(None, actual.rating_count());
        assert!(!actual.outdated());
        assert!(actual.dependencies().is_empty());
    });
}

#[test]
fn test_extension_state_computation() {
    run(|f| async move {
        let gallery = a_gallery_extension("gallery1");
        let test_object = f.a_workbench_service().await;
        f.gallery.stub_query(vec![gallery.clone()]);

        let page = query_first_page(&test_object).await;
        let extension = page.first_page()[0].clone();
        assert_eq!(ExtensionState::Uninstalled, extension.state());

        let identifier = gallery.identifier.clone();

        // Installing
        f.management.install_event.fire(install_event(&gallery));
        let local = test_object.local();
        assert_eq!(1, local.len());
        let actual = local[0].clone();
        assert_eq!(format!("{}.{}", gallery.publisher, gallery.name), actual.identifier().id);
        assert_eq!(ExtensionState::Installing, actual.state());

        // Installed
        f.management.did_install_event.fire(vec![did_install_result(&identifier, Some(InstallExtensionSource::Gallery(gallery.clone())), InstallOperation::Install, local_of(&gallery))]);
        assert_eq!(ExtensionState::Installed, actual.state());
        assert_eq!(1, test_object.local().len());

        let _ = test_object.uninstall(&actual);

        // Uninstalling
        f.management.uninstall_event.fire(uninstall_event(&identifier));
        assert_eq!(ExtensionState::Uninstalling, actual.state());

        // Uninstalled
        f.management.did_uninstall_event.fire(did_uninstall_event(&identifier));
        assert_eq!(ExtensionState::Uninstalled, actual.state());

        assert_eq!(0, test_object.local().len());
    });
}

#[test]
fn test_extension_doesnot_show_outdated_for_system_extensions() {
    run(|f| async move {
        let local = a_local_extension_with("a", json!({ "version": "1.0.1" }), LocalProperties { extension_type: Some(ExtensionType::System), ..LocalProperties::default() });
        f.management.stub_installed(vec![local.clone()]);
        f.gallery.stub_query(vec![a_gallery_extension_props(&local.manifest.name, GalleryProperties { version: Some("1.0.2"), ..GalleryProperties::default() })]);
        let test_object = f.a_workbench_service().await;
        test_object.query_local(None).await.expect("queryLocal resolve");

        assert!(!test_object.local()[0].outdated());
    });
}

#[test]
fn test_is_auto_update_delayed_returns_true_for_a_recently_published_version() {
    run(|f| async move {
        let test_object = f.an_outdated_extension_workbench_service(Some(now() - HOUR_MS /* 1 hour ago */), true).await;

        assert!(test_object.local()[0].outdated());
        assert!(test_object.is_auto_update_delayed(&test_object.local()[0]));
    });
}

#[test]
fn test_is_auto_update_delayed_returns_true_for_a_recently_published_signed_version() {
    run(|f| async move {
        let test_object = f.an_outdated_extension_workbench_service(Some(now() - HOUR_MS /* 1 hour ago */), true).await;

        assert!(test_object.local()[0].outdated());
        assert!(test_object.is_auto_update_delayed(&test_object.local()[0]));
    });
}

#[test]
fn test_is_auto_update_delayed_returns_false_for_a_version_published_more_than_2_hours_ago() {
    run(|f| async move {
        let test_object = f.an_outdated_extension_workbench_service(Some(now() - HOUR_MS * 3.0 /* 3 hours ago */), true).await;

        assert!(test_object.local()[0].outdated());
        assert!(!test_object.is_auto_update_delayed(&test_object.local()[0]));
    });
}

#[test]
fn test_is_auto_update_delayed_returns_false_for_a_version_with_a_future_published_timestamp() {
    run(|f| async move {
        let test_object = f.an_outdated_extension_workbench_service(Some(now() + HOUR_MS /* 1 hour in the future */), true).await;

        assert!(test_object.local()[0].outdated());
        assert!(!test_object.is_auto_update_delayed(&test_object.local()[0]));
    });
}

#[test]
fn test_is_auto_update_delayed_returns_false_when_auto_update_is_disabled() {
    run(|f| async move {
        f.stub_configuration(Some(json!(false)));
        let test_object = f.an_outdated_extension_workbench_service(Some(now() - HOUR_MS /* 1 hour ago */), true).await;

        assert!(test_object.local()[0].outdated());
        assert!(!test_object.is_auto_update_delayed(&test_object.local()[0]));
    });
}

#[test]
fn test_get_auto_update_delay_remaining_returns_remaining_time_within_the_delay_window() {
    run(|f| async move {
        let test_object = f.an_outdated_extension_workbench_service(Some(now() - HOUR_MS /* 1 hour ago */), true).await;

        let remaining = test_object.get_auto_update_delay_remaining(&test_object.local()[0]);
        // Published 1 hour ago, so ~1 hour of the 2 hour window remains.
        assert!(remaining > (1000.0 * 60.0 * 30.0) && remaining <= (1000.0 * 60.0 * 60.0), "unexpected remaining time {remaining}");
    });
}

#[test]
fn test_get_auto_update_delay_remaining_returns_remaining_time_regardless_of_signing() {
    run(|f| async move {
        let test_object = f.an_outdated_extension_workbench_service(Some(now() - HOUR_MS /* 1 hour ago */), false).await;

        let remaining = test_object.get_auto_update_delay_remaining(&test_object.local()[0]);
        assert!(remaining > (1000.0 * 60.0 * 30.0) && remaining <= (1000.0 * 60.0 * 60.0), "unexpected remaining time {remaining}");
    });
}

#[test]
fn test_get_auto_update_delay_remaining_returns_0_when_the_published_timestamp_is_missing() {
    run(|f| async move {
        let test_object = f.an_outdated_extension_workbench_service(None, true).await;

        assert_eq!(test_object.get_auto_update_delay_remaining(&test_object.local()[0]), 0.0);
    });
}

#[test]
fn test_get_auto_update_delay_remaining_returns_0_for_a_trusted_publisher() {
    run(|f| async move {
        f.stub_product_service(ProductService { trusted_extension_publishers: vec!["pub".to_owned()], ..TestProductService::default() });
        let test_object = f.an_outdated_extension_workbench_service(Some(now() - HOUR_MS /* 1 hour ago */), true).await;

        assert_eq!(test_object.get_auto_update_delay_remaining(&test_object.local()[0]), 0.0);
        assert!(!test_object.is_auto_update_delayed(&test_object.local()[0]));
    });
}

#[test]
fn test_get_auto_update_value_normalizes_legacy_and_migrated_values() {
    run(|f| async move {
        let expected: Vec<(Value, AutoUpdateConfigurationValue)> = vec![
            (json!("on"), AutoUpdateConfigurationValue::On),
            (json!("off"), AutoUpdateConfigurationValue::Off),
            (json!("delayed"), AutoUpdateConfigurationValue::On),
            (json!("onlySelectedExtensions"), AutoUpdateConfigurationValue::Off),
            (json!(true), AutoUpdateConfigurationValue::On),
            (json!(false), AutoUpdateConfigurationValue::Off),
            (json!("onlyEnabledExtensions"), AutoUpdateConfigurationValue::On),
        ];
        for (configured, normalized) in expected {
            f.stub_configuration(Some(configured.clone()));
            let test_object = f.a_workbench_service().await;
            assert_eq!(test_object.get_auto_update_value(), normalized, "unexpected value for {configured}");
        }
    });
}

#[test]
fn test_can_install_returns_false_for_extensions_with_out_gallery() {
    run(|f| async move {
        let local = a_local_extension_with("a", json!({ "version": "1.0.1" }), LocalProperties { extension_type: Some(ExtensionType::System), ..LocalProperties::default() });
        f.management.stub_installed(vec![local.clone()]);
        let test_object = f.a_workbench_service().await;
        let target = test_object.local()[0].clone();
        let _ = test_object.uninstall(&target);
        f.management.uninstall_event.fire(uninstall_event(&local.identifier));
        f.management.did_uninstall_event.fire(did_uninstall_event(&local.identifier));

        assert!(test_object.can_install(&target).await.expect("canInstall resolve").is_err());
    });
}

#[test]
fn test_can_install_returns_false_for_a_system_extension() {
    run(|f| async move {
        let local = a_local_extension_with("a", json!({ "version": "1.0.1" }), LocalProperties { extension_type: Some(ExtensionType::System), ..LocalProperties::default() });
        f.management.stub_installed(vec![local.clone()]);
        f.gallery.stub_query(vec![a_gallery_extension_props(&local.manifest.name, GalleryProperties::default())]);
        let test_object = f.a_workbench_service().await;
        let target = test_object.local()[0].clone();

        assert!(test_object.can_install(&target).await.expect("canInstall resolve").is_err());
    });
}

#[test]
fn test_can_install_returns_true_for_extensions_with_gallery() {
    run(|f| async move {
        let local = a_local_extension_with("a", json!({ "version": "1.0.1" }), LocalProperties { extension_type: Some(ExtensionType::User), ..LocalProperties::default() });
        f.management.stub_installed(vec![local.clone()]);
        let gallery = a_gallery_extension_props(&local.manifest.name, GalleryProperties::default());
        f.gallery.stub_query(vec![gallery.clone()]);
        f.gallery.stub_get_compatible_extension(gallery.clone());
        f.gallery.stub_get_extensions(vec![gallery]);
        let test_object = f.a_workbench_service().await;
        let target = test_object.local()[0].clone();

        test_object.on_change().filter(|e| e.as_ref().is_some_and(|e| e.gallery().is_some()), None).to_promise(None).await.expect("onChange dispara");
        assert!(test_object.can_install(&target).await.expect("canInstall resolve").is_ok());
    });
}

#[test]
fn test_onchange_event_is_triggered_while_installing() {
    run(|f| async move {
        let gallery = a_gallery_extension("gallery1");
        let test_object = f.a_workbench_service().await;
        f.gallery.stub_query(vec![gallery.clone()]);

        let page = query_first_page(&test_object).await;
        let extension = page.first_page()[0].clone();
        assert_eq!(ExtensionState::Uninstalled, extension.state());

        f.management.install_event.fire(install_event(&gallery));
        let promise = test_object.on_change().to_promise(None);

        // Installed
        f.management.did_install_event.fire(vec![did_install_result(&gallery.identifier, Some(InstallExtensionSource::Gallery(gallery.clone())), InstallOperation::Install, local_of(&gallery))]);

        promise.await.expect("onChange dispara");
    });
}

#[test]
fn test_onchange_event_is_triggered_when_installation_is_finished() {
    run(|f| async move {
        let gallery = a_gallery_extension("gallery1");
        let test_object = f.a_workbench_service().await;
        f.gallery.stub_query(vec![gallery.clone()]);
        let target = Spy::new();

        let page = query_first_page(&test_object).await;
        let extension = page.first_page()[0].clone();
        assert_eq!(ExtensionState::Uninstalled, extension.state());

        target.attach(&f.guard, test_object.on_change());

        // Installing
        f.management.install_event.fire(install_event(&gallery));

        assert!(target.called_once());
    });
}

#[test]
fn test_onchange_event_is_triggered_while_uninstalling() {
    run(|f| async move {
        let local = of_type("a", ExtensionType::System);
        f.management.stub_installed(vec![local.clone()]);
        let test_object = f.a_workbench_service().await;
        let target = Spy::new();

        let _ = test_object.uninstall(&test_object.local()[0]);
        target.attach(&f.guard, test_object.on_change());
        f.management.uninstall_event.fire(uninstall_event(&local.identifier));

        assert!(target.called_once());
    });
}

#[test]
fn test_onchange_event_is_triggered_when_uninstalling_is_finished() {
    run(|f| async move {
        let local = of_type("a", ExtensionType::System);
        f.management.stub_installed(vec![local.clone()]);
        let test_object = f.a_workbench_service().await;
        let target = Spy::new();

        let _ = test_object.uninstall(&test_object.local()[0]);
        f.management.uninstall_event.fire(uninstall_event(&local.identifier));
        target.attach(&f.guard, test_object.on_change());
        f.management.did_uninstall_event.fire(did_uninstall_event(&local.identifier));

        assert!(target.called_once());
    });
}

#[test]
fn test_uninstalled_extensions_are_always_enabled() {
    run(|f| async move {
        set_enablement(&f, &[&a_local_extension("b")], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("c")], E::DisabledWorkspace).await;
        let test_object = f.a_workbench_service().await;
        f.gallery.stub_query(vec![a_gallery_extension("a")]);
        let paged_response = query_first_page(&test_object).await;
        let actual = paged_response.first_page()[0].clone();
        assert_eq!(actual.enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_enablement_state_installed_enabled_extension() {
    run(|f| async move {
        set_enablement(&f, &[&a_local_extension("b")], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("c")], E::DisabledWorkspace).await;
        f.management.stub_installed(vec![a_local_extension("a")]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.local()[0].clone();

        assert_eq!(actual.enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_workspace_disabled_extension() {
    run(|f| async move {
        let extension_a = a_local_extension("a");
        set_enablement(&f, &[&a_local_extension("b")], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("d")], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_a], E::DisabledWorkspace).await;
        set_enablement(&f, &[&a_local_extension("e")], E::DisabledWorkspace).await;
        f.management.stub_installed(vec![extension_a]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.local()[0].clone();

        assert_eq!(actual.enablement_state(), E::DisabledWorkspace);
    });
}

#[test]
fn test_globally_disabled_extension() {
    run(|f| async move {
        let local_extension = a_local_extension("a");
        set_enablement(&f, &[&local_extension], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("d")], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("c")], E::DisabledWorkspace).await;
        f.management.stub_installed(vec![local_extension]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.local()[0].clone();

        assert_eq!(actual.enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_enablement_state_is_updated_for_user_extensions() {
    run(|f| async move {
        set_enablement(&f, &[&a_local_extension("c")], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("b")], E::DisabledWorkspace).await;
        f.management.stub_installed(vec![a_local_extension("a")]);
        let test_object = f.a_workbench_service().await;
        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledWorkspace).await.expect("setEnablement resolve");
        let actual = test_object.local()[0].clone();
        assert_eq!(actual.enablement_state(), E::DisabledWorkspace);
    });
}

#[test]
fn test_enable_extension_globally_when_extension_is_disabled_for_workspace() {
    run(|f| async move {
        let local_extension = a_local_extension("a");
        set_enablement(&f, &[&local_extension], E::DisabledWorkspace).await;
        f.management.stub_installed(vec![local_extension]);
        let test_object = f.a_workbench_service().await;
        test_object.set_enablement(vec![test_object.local()[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        let actual = test_object.local()[0].clone();
        assert_eq!(actual.enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_disable_extension_globally() {
    run(|f| async move {
        f.management.stub_installed(vec![a_local_extension("a")]);
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        let actual = test_object.local()[0].clone();
        assert_eq!(actual.enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_system_extensions_can_be_disabled() {
    run(|f| async move {
        f.management.stub_installed(vec![of_type("a", ExtensionType::System)]);
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        let actual = test_object.local()[0].clone();
        assert_eq!(actual.enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_enablement_state_is_updated_on_change_from_outside() {
    run(|f| async move {
        let local_extension = a_local_extension("a");
        set_enablement(&f, &[&a_local_extension("c")], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("b")], E::DisabledWorkspace).await;
        f.management.stub_installed(vec![local_extension.clone()]);
        let test_object = f.a_workbench_service().await;

        set_enablement(&f, &[&local_extension], E::DisabledGlobally).await;
        let actual = test_object.local()[0].clone();
        assert_eq!(actual.enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_extension_with_dependencies_disable_only_itself() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::DisabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_disable_extension_pack_disables_the_pack() {
    run(|f| async move {
        let extension_a = packing("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::DisabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_extension_pack_disable_all() {
    run(|f| async move {
        let extension_a = packing("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::DisabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_extension_fails_if_extension_is_a_dependent_of_other() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        // `instantiationService.stub(INotificationService, { prompt(...) { options.onCancel(); } })`: ver a nota do módulo.
        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;
        let result = test_object.set_enablement(vec![test_object.local()[1].clone()], E::DisabledGlobally).await;
        assert!(result.is_err(), "Should fail");
    });
}

#[test]
fn test_disable_extension_disables_all_dependents_when_chosen_to_disable_all() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        f.stub_dialog_service(StubDialogService::prompting(Dynamic::Bool(true)));
        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;
        test_object.set_enablement(vec![test_object.local()[1].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::DisabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_extension_when_extension_is_part_of_a_pack() {
    run(|f| async move {
        let extension_a = packing("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;
        test_object.set_enablement(vec![test_object.local()[1].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[1].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_both_dependency_and_dependent_do_not_promot_and_do_not_fail() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let target = Spy::new();
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[1].clone(), test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert!(!target.called());
        assert_eq!(test_object.local()[0].enablement_state(), E::DisabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_enable_both_dependency_and_dependent_do_not_promot_and_do_not_fail() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::DisabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let target = Spy::new();
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[1].clone(), test_object.local()[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        assert!(!target.called());
        assert_eq!(test_object.local()[0].enablement_state(), E::EnabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_disable_extension_does_not_fail_if_its_dependency_is_a_dependent_of_other_but_chosen_to_disable_only_itself() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = depending_on("c", &["pub.b"]);

        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_extension_if_its_dependency_is_a_dependent_of_other_disabled_extension() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = depending_on("c", &["pub.b"]);

        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::DisabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_extension_if_its_dependencys_dependency_is_itself() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = depending_on("b", &["pub.a"]);
        let extension_c = a_local_extension("c");

        // `instantiationService.stub(INotificationService, { prompt(...) { options.onCancel(); } })`: ver a nota do módulo.
        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;

        let result = test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await;
        assert!(result.is_err(), "An extension with dependent should not be disabled");
    });
}

#[test]
fn test_disable_extension_if_its_dependency_is_dependent_and_is_disabled() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = depending_on("c", &["pub.b"]);

        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);

        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_disable_extension_with_cyclic_dependencies() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = depending_on("b", &["pub.c"]);
        let extension_c = depending_on("c", &["pub.a"]);

        // `instantiationService.stub(INotificationService, { prompt(...) { options.onCancel(); } })`: ver a nota do módulo.
        set_enablement(&f, &[&extension_a], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::EnabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;
        let result = test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await;
        assert!(result.is_err(), "An extension with dependent should not be disabled");
    });
}

#[test]
fn test_reload_prompt_when_disabling_running_extension() {
    run(|f| async move {
        let local_extension = a_local_extension_with(
            "my_extension",
            json!({}),
            LocalProperties { identifier: Some(ExtensionIdentifierInfo::new("my_publisher.my_extension")), ..LocalProperties::default() },
        );
        f.management.stub_installed(vec![local_extension.clone()]);

        // Mock a running extension (not under development)
        f.stub_extension_service(vec![a_running_extension(&local_extension, false)]);

        let test_object = f.a_workbench_service().await;
        let extension = test_object.local()[0].clone();

        // Extension should require reload action when disabling
        test_object.set_enablement(vec![extension.clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(extension.runtime_state().map(|state| state.action), Some(ExtensionRuntimeActionType::RestartExtensions));
    });
}

#[test]
fn test_no_reload_prompt_when_disabling_extension_under_development() {
    run(|f| async move {
        let local_extension = a_local_extension_with(
            "my_extension",
            json!({}),
            LocalProperties { identifier: Some(ExtensionIdentifierInfo::new("my_publisher.my_extension")), ..LocalProperties::default() },
        );
        f.management.stub_installed(vec![local_extension.clone()]);

        // Mock a running extension under development
        f.stub_extension_service(vec![a_running_extension(&local_extension, true)]);

        let test_object = f.a_workbench_service().await;
        let extension = test_object.local()[0].clone();

        // Extension should have no reload action required when disabling
        test_object.set_enablement(vec![extension.clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(extension.runtime_state(), None);
    });
}

#[test]
fn test_enable_extension_with_dependencies_enable_all() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::DisabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::EnabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_enable_extension_with_dependencies_does_not_prompt_if_dependency_is_enabled_already() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::EnabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::DisabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let target = Spy::new();
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        assert!(!target.called());
        assert_eq!(test_object.local()[0].enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_enable_extension_with_dependency_does_not_prompt_if_both_are_enabled() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = a_local_extension("b");
        let extension_c = a_local_extension("c");

        set_enablement(&f, &[&extension_a], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::DisabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);
        let target = Spy::new();
        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[1].clone(), test_object.local()[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        assert!(!target.called());
        assert_eq!(test_object.local()[0].enablement_state(), E::EnabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_enable_extension_with_cyclic_dependencies() {
    run(|f| async move {
        let extension_a = depending_on("a", &["pub.b"]);
        let extension_b = depending_on("b", &["pub.c"]);
        let extension_c = depending_on("c", &["pub.a"]);

        set_enablement(&f, &[&extension_a], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_b], E::DisabledGlobally).await;
        set_enablement(&f, &[&extension_c], E::DisabledGlobally).await;
        f.management.stub_installed(vec![extension_a, extension_b, extension_c]);

        let test_object = f.a_workbench_service().await;

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        assert_eq!(test_object.local()[0].enablement_state(), E::EnabledGlobally);
        assert_eq!(test_object.local()[1].enablement_state(), E::EnabledGlobally);
        assert_eq!(test_object.local()[2].enablement_state(), E::EnabledGlobally);
    });
}

#[test]
fn test_change_event_is_fired_when_disablement_flags_are_changed() {
    run(|f| async move {
        set_enablement(&f, &[&a_local_extension("c")], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("b")], E::DisabledWorkspace).await;
        f.management.stub_installed(vec![a_local_extension("a")]);
        let test_object = f.a_workbench_service().await;
        let target = Spy::new();
        target.attach(&f.guard, test_object.on_change());

        test_object.set_enablement(vec![test_object.local()[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert!(target.called_once());
    });
}

#[test]
fn test_change_event_is_fired_when_disablement_flags_are_changed_from_outside() {
    run(|f| async move {
        let local_extension = a_local_extension("a");
        set_enablement(&f, &[&a_local_extension("c")], E::DisabledGlobally).await;
        set_enablement(&f, &[&a_local_extension("b")], E::DisabledWorkspace).await;
        f.management.stub_installed(vec![local_extension.clone()]);
        let test_object = f.a_workbench_service().await;
        let target = Spy::new();
        target.attach(&f.guard, test_object.on_change());

        set_enablement(&f, &[&local_extension], E::DisabledGlobally).await;
        assert!(target.called_once());
    });
}

#[test]
fn test_updating_an_extension_does_not_re_eanbles_it_when_disabled_globally() {
    run(|f| async move {
        let test_object = f.a_workbench_service().await;
        let local = a_local_extension("pub.a");
        set_enablement(&f, &[&local], E::DisabledGlobally).await;
        f.management.did_install_event.fire(vec![did_install_result(&local.identifier, None, InstallOperation::Update, local.clone())]);
        f.management.stub_installed(vec![local.clone()]);
        let actual = test_object.query_local(None).await.expect("queryLocal resolve");
        assert_eq!(actual[0].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_updating_an_extension_does_not_re_eanbles_it_when_workspace_disabled() {
    run(|f| async move {
        let test_object = f.a_workbench_service().await;
        let local = a_local_extension("pub.a");
        set_enablement(&f, &[&local], E::DisabledWorkspace).await;
        f.management.did_install_event.fire(vec![did_install_result(&local.identifier, None, InstallOperation::Update, local.clone())]);
        f.management.stub_installed(vec![local.clone()]);
        let actual = test_object.query_local(None).await.expect("queryLocal resolve");
        assert_eq!(actual[0].enablement_state(), E::DisabledWorkspace);
    });
}

#[test]
fn test_user_extension_is_preferred_when_the_same_extension_exists_as_system_and_user_extension() {
    run(|f| async move {
        let test_object = f.a_workbench_service().await;
        let user_extension = a_local_extension("pub.a");
        let system_extension = of_type("pub.a", ExtensionType::System);
        f.management.stub_installed(vec![system_extension, user_extension.clone()]);

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &user_extension));
    });
}

#[test]
fn test_user_extension_is_disabled_when_the_same_extension_exists_as_system_and_user_extension_and_system_extension_is_disabled() {
    run(|f| async move {
        let test_object = f.a_workbench_service().await;
        let system_extension = of_type("pub.a", ExtensionType::System);
        set_enablement(&f, &[&system_extension], E::DisabledGlobally).await;
        let user_extension = a_local_extension("pub.a");
        f.management.stub_installed(vec![system_extension, user_extension.clone()]);

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &user_extension));
        assert_eq!(actual[0].enablement_state(), E::DisabledGlobally);
    });
}

/// O corpo comum dos casos `Test local ... extension is chosen if it exists only in local server`.
async fn local_only_is_chosen(f: &WorkbenchFixture, kinds: &[&str]) {
    // multi server setup
    let local_extension = with_kinds(kinds, local_location());

    use_servers(f, vec![local_extension.clone()], Vec::new());
    let test_object = f.a_workbench_service().await;

    let actual = test_object.query_local(None).await.expect("queryLocal resolve");

    assert_eq!(actual.len(), 1);
    assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &local_extension));
}

#[test]
fn test_local_ui_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["ui"]).await });
}

#[test]
fn test_local_workspace_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["workspace"]).await });
}

#[test]
fn test_local_web_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["web"]).await });
}

#[test]
fn test_local_ui_workspace_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["ui", "workspace"]).await });
}

#[test]
fn test_local_workspace_ui_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["workspace", "ui"]).await });
}

#[test]
fn test_local_ui_workspace_web_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["ui", "workspace", "web"]).await });
}

#[test]
fn test_local_ui_web_workspace_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["ui", "web", "workspace"]).await });
}

#[test]
fn test_local_web_ui_workspace_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["web", "ui", "workspace"]).await });
}

#[test]
fn test_local_web_workspace_ui_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["web", "workspace", "ui"]).await });
}

#[test]
fn test_local_workspace_web_ui_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["workspace", "web", "ui"]).await });
}

#[test]
fn test_local_workspace_ui_web_extension_is_chosen_if_it_exists_only_in_local_server() {
    run(|f| async move { local_only_is_chosen(&f, &["workspace", "ui", "web"]).await });
}

#[test]
fn test_local_ui_extension_is_chosen_if_it_exists_in_both_servers() {
    run(|f| async move {
        // multi server setup
        let local_extension = with_kinds(&["ui"], local_location());
        let remote_extension = with_kinds(&["ui"], remote_location());

        use_servers(&f, vec![local_extension.clone()], vec![remote_extension]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &local_extension));
    });
}

#[test]
fn test_local_ui_workspace_extension_is_chosen_if_it_exists_in_both_servers() {
    run(|f| async move {
        // multi server setup
        let local_extension = with_kinds(&["ui", "workspace"], local_location());
        let remote_extension = with_kinds(&["ui", "workspace"], remote_location());

        use_servers(&f, vec![local_extension.clone()], vec![remote_extension]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &local_extension));
    });
}

#[test]
fn test_remote_workspace_extension_is_chosen_if_it_exists_in_remote_server() {
    run(|f| async move {
        // multi server setup
        let remote_extension = with_kinds(&["workspace"], remote_location());

        use_servers(&f, Vec::new(), vec![remote_extension.clone()]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &remote_extension));
    });
}

#[test]
fn test_remote_workspace_extension_is_chosen_if_it_exists_in_both_servers() {
    run(|f| async move {
        // multi server setup
        let local_extension = with_kinds(&["workspace"], local_location());
        let remote_extension = with_kinds(&["workspace"], remote_location());

        use_servers(&f, vec![local_extension], vec![remote_extension.clone()]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &remote_extension));
    });
}

#[test]
fn test_remote_workspace_extension_is_chosen_if_it_exists_in_both_servers_and_local_is_disabled() {
    run(|f| async move {
        // multi server setup
        let local_extension = with_kinds(&["workspace"], local_location());
        let remote_extension = with_kinds(&["workspace"], remote_location());

        use_servers(&f, vec![local_extension], vec![remote_extension.clone()]);
        set_enablement(&f, &[&remote_extension], E::DisabledGlobally).await;
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &remote_extension));
        assert_eq!(actual[0].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_remote_workspace_extension_is_chosen_if_it_exists_in_both_servers_and_remote_is_disabled_in_workspace() {
    run(|f| async move {
        // multi server setup
        let local_extension = with_kinds(&["workspace"], local_location());
        let remote_extension = with_kinds(&["workspace"], remote_location());

        use_servers(&f, vec![local_extension], vec![remote_extension.clone()]);
        set_enablement(&f, &[&remote_extension], E::DisabledWorkspace).await;
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &remote_extension));
        assert_eq!(actual[0].enablement_state(), E::DisabledWorkspace);
    });
}

#[test]
fn test_local_ui_workspace_extension_is_chosen_if_it_exists_in_both_servers_and_local_is_disabled() {
    run(|f| async move {
        // multi server setup
        let local_extension = with_kinds(&["ui", "workspace"], local_location());
        let remote_extension = with_kinds(&["ui", "workspace"], remote_location());

        use_servers(&f, vec![local_extension.clone()], vec![remote_extension]);
        set_enablement(&f, &[&local_extension], E::DisabledGlobally).await;
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &local_extension));
        assert_eq!(actual[0].enablement_state(), E::DisabledGlobally);
    });
}

#[test]
fn test_local_ui_workspace_extension_is_chosen_if_it_exists_in_both_servers_and_local_is_disabled_in_workspace() {
    run(|f| async move {
        // multi server setup
        let local_extension = with_kinds(&["ui", "workspace"], local_location());
        let remote_extension = with_kinds(&["ui", "workspace"], remote_location());

        use_servers(&f, vec![local_extension.clone()], vec![remote_extension]);
        set_enablement(&f, &[&local_extension], E::DisabledWorkspace).await;
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &local_extension));
        assert_eq!(actual[0].enablement_state(), E::DisabledWorkspace);
    });
}

#[test]
fn test_local_web_extension_is_chosen_if_it_exists_in_both_servers() {
    run(|f| async move {
        // multi server setup
        let local_extension = with_kinds(&["web"], local_location());
        let remote_extension = with_kinds(&["web"], remote_location());

        use_servers(&f, vec![local_extension.clone()], vec![remote_extension]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &local_extension));
    });
}

#[test]
fn test_remote_web_extension_is_chosen_if_it_exists_only_in_remote() {
    run(|f| async move {
        // multi server setup
        let remote_extension = with_kinds(&["web"], remote_location());

        use_servers(&f, Vec::new(), vec![remote_extension.clone()]);
        let test_object = f.a_workbench_service().await;

        let actual = test_object.query_local(None).await.expect("queryLocal resolve");

        assert_eq!(actual.len(), 1);
        assert!(Rc::ptr_eq(&actual[0].local().expect("a extensão tem local"), &remote_extension));
    });
}

/// `(local: Mutable<ILocalExtension>, metadata: Partial<Metadata>) => { local.pinned = !!metadata.pinned; return local; }`.
fn stub_pinned_update(f: &WorkbenchFixture) {
    f.management.stub_update_metadata(|local, metadata| {
        let mut updated = local.clone();
        updated.pinned = metadata.pinned.unwrap_or(false);
        updated
    });
}

#[test]
fn test_disable_autoupdate_for_extension_when_auto_update_is_enabled_for_all() {
    run(|f| async move {
        let extension1 = a_local_extension("a");
        let extension2 = a_local_extension("b");
        f.management.stub_installed(vec![extension1, extension2]);
        stub_pinned_update(&f);
        let test_object = f.a_workbench_service().await;

        assert!(!test_object.local()[0].local().expect("local").pinned);
        assert!(!test_object.local()[1].local().expect("local").pinned);

        test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), false).await.expect("updateAutoUpdateEnablementFor resolve");

        assert!(!test_object.local()[0].local().expect("local").pinned);
        assert!(!test_object.local()[1].local().expect("local").pinned);

        assert_eq!(test_object.get_enabled_auto_update_extensions(), Vec::<String>::new());
        assert_eq!(test_object.get_disabled_auto_update_extensions(), vec!["pub.a".to_owned()]);
    });
}

#[test]
fn test_disable_autoupdate_for_extension_when_auto_update_is_enabled_for_enabled_extensions() {
    run(|f| async move {
        f.stub_configuration(Some(json!("onlyEnabledExtensions")));

        let extension1 = a_local_extension("a");
        let extension2 = a_local_extension("b");
        f.management.stub_installed(vec![extension1, extension2]);
        stub_pinned_update(&f);
        let test_object = f.a_workbench_service().await;

        assert!(!test_object.local()[0].local().expect("local").pinned);
        assert!(!test_object.local()[1].local().expect("local").pinned);

        test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), false).await.expect("updateAutoUpdateEnablementFor resolve");

        assert!(!test_object.local()[0].local().expect("local").pinned);
        assert!(!test_object.local()[1].local().expect("local").pinned);

        assert_eq!(test_object.get_enabled_auto_update_extensions(), Vec::<String>::new());
        assert_eq!(test_object.get_disabled_auto_update_extensions(), vec!["pub.a".to_owned()]);
    });
}

#[test]
fn test_enable_autoupdate_for_extension_when_auto_update_is_enabled_for_all() {
    run(|f| async move {
        let extension1 = a_local_extension("a");
        let extension2 = a_local_extension("b");
        f.management.stub_installed(vec![extension1, extension2]);
        stub_pinned_update(&f);
        let test_object = f.a_workbench_service().await;

        assert!(!test_object.local()[0].local().expect("local").pinned);
        assert!(!test_object.local()[1].local().expect("local").pinned);

        test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), false).await.expect("updateAutoUpdateEnablementFor resolve");
        test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), true).await.expect("updateAutoUpdateEnablementFor resolve");

        assert!(!test_object.local()[0].local().expect("local").pinned);
        assert!(!test_object.local()[1].local().expect("local").pinned);

        assert_eq!(test_object.get_enabled_auto_update_extensions(), Vec::<String>::new());
        assert_eq!(test_object.get_disabled_auto_update_extensions(), Vec::<String>::new());
    });
}

#[test]
fn test_enable_autoupdate_for_pinned_extension_when_auto_update_is_enabled() {
    run(|f| async move {
        let extension1 = a_local_extension_with("a", json!({}), LocalProperties { pinned: true, ..LocalProperties::default() });
        let extension2 = a_local_extension("b");
        f.management.stub_installed(vec![extension1, extension2]);
        stub_pinned_update(&f);
        let test_object = f.a_workbench_service().await;

        assert!(test_object.local()[0].local().expect("local").pinned);
        assert!(!test_object.local()[1].local().expect("local").pinned);

        test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), true).await.expect("updateAutoUpdateEnablementFor resolve");

        assert!(!test_object.local()[0].local().expect("local").pinned);
        assert!(!test_object.local()[1].local().expect("local").pinned);

        assert_eq!(test_object.get_enabled_auto_update_extensions(), Vec::<String>::new());
        assert_eq!(test_object.get_disabled_auto_update_extensions(), Vec::<String>::new());
    });
}

#[test]
fn test_update_auto_update_enablement_for_throws_error_when_auto_update_is_disabled() {
    run(|f| async move {
        f.stub_configuration(Some(json!(false)));

        let extension1 = a_local_extension("a");
        let extension2 = a_local_extension("b");
        f.management.stub_installed(vec![extension1, extension2]);
        let test_object = f.a_workbench_service().await;

        // O `assert.fail('error expected')` do original está dentro do `try` e o `catch` o captura: o caso passa com
        // qualquer resultado.
        let _ = test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), true).await;
    });
}

#[test]
fn test_update_auto_update_enablement_for_throws_error_for_publisher_when_auto_update_is_enabled() {
    run(|f| async move {
        let extension1 = a_local_extension("a");
        let extension2 = a_local_extension("b");
        f.management.stub_installed(vec![extension1, extension2]);
        let test_object = f.a_workbench_service().await;

        // O `assert.fail('error expected')` do original está dentro do `try` e o `catch` o captura: o caso passa com
        // qualquer resultado.
        let _ = test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Publisher(test_object.local()[0].publisher()), true).await;
    });
}

#[test]
fn test_enable_autoupdate_for_extension_when_auto_update_is_disabled() {
    run(|f| async move {
        f.stub_configuration(Some(json!(false)));

        let extension1 = a_local_extension_with("a", json!({}), LocalProperties { pinned: true, ..LocalProperties::default() });
        let extension2 = a_local_extension_with("b", json!({}), LocalProperties { pinned: true, ..LocalProperties::default() });
        f.management.stub_installed(vec![extension1, extension2]);
        stub_pinned_update(&f);
        let test_object = f.a_workbench_service().await;

        assert!(test_object.local()[0].local().expect("local").pinned);
        assert!(test_object.local()[1].local().expect("local").pinned);

        test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), true).await.expect("updateAutoUpdateEnablementFor resolve");

        assert!(test_object.local()[0].local().expect("local").pinned);
        assert!(test_object.local()[1].local().expect("local").pinned);

        assert_eq!(test_object.get_enabled_auto_update_extensions(), vec!["pub.a".to_owned()]);
        assert_eq!(test_object.get_disabled_auto_update_extensions(), Vec::<String>::new());
    });
}

#[test]
fn test_reset_autoupdate_extensions_state_when_auto_update_is_disabled() {
    run(|f| async move {
        f.stub_dialog_service(StubDialogService::confirming(true));

        let extension1 = a_local_extension_with("a", json!({}), LocalProperties { pinned: true, ..LocalProperties::default() });
        let extension2 = a_local_extension_with("b", json!({}), LocalProperties { pinned: true, ..LocalProperties::default() });
        f.management.stub_installed(vec![extension1, extension2]);
        stub_pinned_update(&f);
        let test_object = f.a_workbench_service().await;

        test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), false).await.expect("updateAutoUpdateEnablementFor resolve");

        assert_eq!(test_object.get_enabled_auto_update_extensions(), Vec::<String>::new());
        assert_eq!(test_object.get_disabled_auto_update_extensions(), vec!["pub.a".to_owned()]);

        test_object.update_auto_update_for_all_extensions(false).await.expect("updateAutoUpdateForAllExtensions resolve");

        assert_eq!(test_object.get_enabled_auto_update_extensions(), Vec::<String>::new());
        assert_eq!(test_object.get_disabled_auto_update_extensions(), Vec::<String>::new());
    });
}

#[test]
fn test_reset_autoupdate_extensions_state_when_auto_update_is_enabled() {
    run(|f| async move {
        f.stub_configuration(Some(json!(false)));
        f.stub_dialog_service(StubDialogService::confirming(true));

        let extension1 = a_local_extension_with("a", json!({}), LocalProperties { pinned: true, ..LocalProperties::default() });
        let extension2 = a_local_extension_with("b", json!({}), LocalProperties { pinned: true, ..LocalProperties::default() });
        f.management.stub_installed(vec![extension1, extension2]);
        stub_pinned_update(&f);
        let test_object = f.a_workbench_service().await;

        test_object.update_auto_update_enablement_for(ExtensionOrPublisher::Extension(test_object.local()[0].clone()), true).await.expect("updateAutoUpdateEnablementFor resolve");

        assert_eq!(test_object.get_enabled_auto_update_extensions(), vec!["pub.a".to_owned()]);
        assert_eq!(test_object.get_disabled_auto_update_extensions(), Vec::<String>::new());

        test_object.update_auto_update_for_all_extensions(true).await.expect("updateAutoUpdateForAllExtensions resolve");

        assert_eq!(test_object.get_enabled_auto_update_extensions(), Vec::<String>::new());
        assert_eq!(test_object.get_disabled_auto_update_extensions(), Vec::<String>::new());
    });
}
