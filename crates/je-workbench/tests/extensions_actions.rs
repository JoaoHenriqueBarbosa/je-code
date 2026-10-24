//! Porte de `src/vs/workbench/contrib/extensions/test/electron-browser/extensionsActions.test.ts` (suítes `ExtensionsActions`, `ExtensionRuntimeStateAction`, `RemoteInstallAction`, `LocalInstallAction` e `EnableAIFeaturesInWorkspaceAction`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `setupTest`, `aMultiExtensionManagementServerService`, `aSingleRemoteExtensionManagementServerService`,
//!   `createExtensionManagementService` e os dublês de serviço estão em `support::extensions_actions_support` (e, por baixo,
//!   em `support::extensions_workbench_service_support`; ver a nota de cada módulo). `createInstance(Action, ...)` é
//!   `Action::new(..., &f.services())`, `disposables.add(x)` é `f.add(x)` e
//!   `createInstance(ExtensionContainers, [x])` é `f.containers(vec![c(&x)])`.
//! - `aLocalExtension(name, manifest, properties)` é o `a_local` daqui: o da `support` deixa `isBuiltin` falso, e o deste
//!   teste o liga quando `type` é `System`. `aGalleryExtension` é o `gallery` daqui (liga `hasReleaseVersion`). O
//!   `identifier` que o original passa como propriedade de `aGalleryExtension` é sobrescrito pelo próprio
//!   `aGalleryExtension` (`{ id, uuid: generateUuid() }`), então os casos não o repassam. `aLocalExtension('a', gallery,
//!   gallery)` é `local_from_gallery` (a versão e o identificador da galeria).
//! - `Event.toPromise(testObject.onDidChange)` é `test_object.on_did_change().to_promise(None)` (criado antes do gatilho,
//!   como o original); `new Promise(c => { disposables.add(testObject.onDidChange(() => { if (cond) c(); })); ... })` é o
//!   mesmo evento filtrado por `cond`.
//! - Os emissores `installEvent`, `didInstallEvent`, `uninstallEvent` e `didUninstallEvent` do módulo do original são os de
//!   `f.fixture.management`; os que um caso cria e liga a um serviço com `service.onXxx = emitter.event` são os emissores do
//!   próprio serviço (`service.install_event`...). Os emissores que o caso cria e nunca dispara (`onDidInstallEvent` dos
//!   casos de estado de execução em vários servidores) não existem aqui.
//! - `instantiationService.stub(IExtensionService, { extensions, onDidChangeExtensions, canAddExtension, ... })` é
//!   `stub_running`; o que o literal omite (`canAddExtension`, `canRemoveExtension`) vale `false`.
//! - `await testObject.update()` é `test_object.update()`: o `update` do original é síncrono (`void`).
//! - `instantiationService.stub(IExtensionsWorkbenchService, workbenchService, 'open', undefined)` (que silencia o
//!   `open` do serviço) não tem efeito aqui: o `open` do editor de extensão não existe no porte, e o `run` das ações que o
//!   chamam não o chama (ver os recortes de `extensions_actions`).
//! - `testObject.class` é `test_object.class()` (um `Option<String>`, sempre `Some`).
//! - O `updatedValue` do último caso é a última chamada de `updateValue` do `TestConfigurationService`.
//! - **Não portados:** nenhum.

mod support;

use std::rc::Rc;

use je_base::actions::IAction;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Emitter;
use je_base::paging::IPager;
use je_extensions::management::{GalleryExtension, InstallOperation, InstallOptions, QueryOptions};
use je_extensions::manifest::ExtensionType;
use je_platform::product::DefaultChatAgentExtensions;
use je_platform::resources::uri_file;
use je_workbench::contrib::extensions::extensions::{IExtension, IExtensionContainer};
use je_workbench::contrib::extensions::extensions_actions::{
    DisableForWorkspaceAction, DisableGloballyAction, EnableAIFeaturesInWorkspaceAction, EnableDropDownAction, EnableForWorkspaceAction, EnableGloballyAction,
    ExtensionRuntimeStateAction, InstallAction, InstallingLabelAction, LocalInstallAction, ManageExtensionAction, RemoteInstallAction, UninstallAction, UpdateAction,
};
use je_workbench::contrib::extensions::extensions_service::to_extension_description;
use je_workbench::contrib::extensions::extensions_workbench_service::Extension;
use je_workbench::services::extension_management::extension_enablement_service::CHAT_AI_DISABLED_SETTING_ID;
use je_workbench::services::extension_management::extension_management::{EnablementState, ExtensionRef, InstallExtensionSource};
use je_workbench::services::extensions::extensions::IExtensionsChangeEvent;
use je_workbench::test::common::workbench_test_services::TestProductService;
use je_extensions::scanner::ExtensionDescription;
use serde_json::{Value, json};
use support::extension_enablement_support::uri_with_scheme;
use support::extensions_actions_support::{
    ActionsFixture, ServerSlot, did_install_result, did_uninstall_event, install_event, install_event_for, run_actions, uninstall_event,
};
use support::extensions_workbench_service_support::{LocalProperties, GalleryProperties, StubExtensionService, a_gallery_extension, a_gallery_extension_props, a_local_extension_with};

use EnablementState as E;

// ---------------------------------------------------------------------------------------------------------------------
// Construtores e utilitários.
// ---------------------------------------------------------------------------------------------------------------------

/// `[testObject]` como `IExtensionContainer[]`.
fn c<T: IExtensionContainer + 'static>(container: &Rc<T>) -> Rc<dyn IExtensionContainer> {
    container.clone()
}

/// `{ installPreReleaseVersion }`.
fn options(install_pre_release_version: bool) -> InstallOptions {
    InstallOptions { install_pre_release_version, ..InstallOptions::default() }
}

/// `aLocalExtension(name, manifest, properties)`: com `isBuiltin` ligado quando o `type` é `System`.
fn a_local(name: &str, manifest: Value, properties: LocalProperties) -> ExtensionRef {
    let is_builtin = properties.extension_type == Some(ExtensionType::System);
    let local = a_local_extension_with(name, manifest, properties);
    let mut local = (*local).clone();
    local.is_builtin = is_builtin;
    Rc::new(local)
}

/// `aLocalExtension(name)`.
fn local_named(name: &str) -> ExtensionRef {
    a_local(name, json!({}), LocalProperties::default())
}

/// `aLocalExtension('a')`.
fn local_a() -> ExtensionRef {
    local_named("a")
}

/// `aLocalExtension('a', manifest)`.
fn local_manifest(manifest: Value) -> ExtensionRef {
    a_local("a", manifest, LocalProperties::default())
}

/// `aLocalExtension('a', {}, { type: ExtensionType.System })`.
fn local_system() -> ExtensionRef {
    a_local("a", json!({}), LocalProperties { extension_type: Some(ExtensionType::System), ..LocalProperties::default() })
}

/// `aLocalExtension('a', { extensionKind }, { location })`.
fn local_kinds(kinds: &[&str], location: je_base::uri::URI) -> ExtensionRef {
    a_local("a", json!({ "extensionKind": kinds }), LocalProperties { location: Some(location), ..LocalProperties::default() })
}

/// `aLocalExtension('a', manifest, { location })`.
fn local_at(manifest: Value, location: je_base::uri::URI) -> ExtensionRef {
    a_local("a", manifest, LocalProperties { location: Some(location), ..LocalProperties::default() })
}

/// `{ contributes: { localizations: [{ languageId: 'de', translations: [] }] } }`.
fn localizations() -> Value {
    json!({ "contributes": { "localizations": [{ "languageId": "de", "translations": [] }] } })
}

/// `URI.file('pub.a')`.
fn file_location() -> je_base::uri::URI {
    uri_file("pub.a")
}

/// `URI.file('pub.a').with({ scheme: Schemas.vscodeRemote })`.
fn remote_location() -> je_base::uri::URI {
    uri_with_scheme("pub.a", je_base::network::schemas::VSCODE_REMOTE)
}

/// `URI.file('pub.a').with({ scheme: Schemas.vscodeUserData })`.
fn web_location() -> je_base::uri::URI {
    uri_with_scheme("pub.a", je_base::network::schemas::VSCODE_USER_DATA)
}

/// `aGalleryExtension(name)`.
fn gallery(name: &str) -> GalleryExtension {
    let mut gallery = a_gallery_extension(name);
    gallery.has_release_version = true;
    gallery
}

/// `aGalleryExtension('a', { version })`.
fn gallery_version(version: &str) -> GalleryExtension {
    let mut gallery = a_gallery_extension_props("a", GalleryProperties { version: Some(version), ..GalleryProperties::default() });
    gallery.has_release_version = true;
    gallery
}

/// `aLocalExtension('a', gallery, gallery)`: a versão e o identificador da galeria.
fn local_from_gallery(gallery: &GalleryExtension) -> ExtensionRef {
    a_local("a", json!({ "version": gallery.version }), LocalProperties { identifier: Some(gallery.identifier.clone()), ..LocalProperties::default() })
}

/// `aLocalExtension('a', { ...gallery, contributes: { localizations: [...] } }, gallery)`.
fn language_pack_from_gallery(gallery: &GalleryExtension) -> ExtensionRef {
    a_local(
        "a",
        json!({ "version": gallery.version, "contributes": { "localizations": [{ "languageId": "de", "translations": [] }] } }),
        LocalProperties { identifier: Some(gallery.identifier.clone()), ..LocalProperties::default() },
    )
}

/// `queryLocal()`.
async fn query_local(f: &ActionsFixture) -> Vec<Rc<Extension>> {
    f.workbench_service().query_local(None).await.expect("queryLocal resolve")
}

/// `queryGallery(CancellationToken.None)`: a primeira página.
async fn query_gallery(f: &ActionsFixture) -> Vec<Rc<Extension>> {
    f.workbench_service().query_gallery(QueryOptions::default(), CancellationToken::None).await.expect("queryGallery resolve").first_page()
}

/// `instantiationService.get(IWorkbenchExtensionEnablementService).setEnablement([local], state)`.
async fn set_enablement(f: &ActionsFixture, local: &ExtensionRef, state: EnablementState) {
    f.fixture.enablement_service().set_enablement(vec![local.clone()], state).await.expect("setEnablement resolve");
}

/// `toExtensionDescription(local)` de cada extensão.
fn running(extensions: &[&ExtensionRef]) -> Vec<ExtensionDescription> {
    extensions.iter().map(|local| to_extension_description(local)).collect()
}

/// `instantiationService.stub(IExtensionService, { extensions, onDidChangeExtensions, canAddExtension, canRemoveExtension, ... })`.
fn stub_running(f: &ActionsFixture, extensions: &[&ExtensionRef], can_add: bool, can_remove: bool) -> Rc<Emitter<IExtensionsChangeEvent>> {
    let emitter = f.add(Emitter::new());
    f.fixture.stub_extension_service_with(StubExtensionService::configured(running(extensions), can_add, can_remove, emitter.event()));
    emitter
}

fn runtime_action(f: &ActionsFixture) -> Rc<ExtensionRuntimeStateAction> {
    let test_object = f.add(ExtensionRuntimeStateAction::new(&f.services()));
    f.containers(vec![c(&test_object)]);
    test_object
}

/// `new InstallExtensionResult` com a extensão da galeria e `Install`.
fn did_install_from_gallery(f: &ActionsFixture, gallery: &GalleryExtension, operation: InstallOperation, local: ExtensionRef) {
    f.fixture.management.did_install_event.fire(vec![did_install_result(&gallery.identifier, Some(InstallExtensionSource::Gallery(gallery.clone())), operation, local)]);
}

// ---------------------------------------------------------------------------------------------------------------------
// Suíte `ExtensionsActions`.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn install_action_is_disabled_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(InstallAction::new(options(false), &f.services()));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_install_action_when_state_is_installed() {
    run_actions(|f| async move {
        let test_object = f.add(InstallAction::new(options(false), &f.services()));
        f.containers(vec![c(&test_object)]);
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);
        query_local(&f).await;
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));
        assert!(!test_object.enabled());
        assert_eq!("Install", test_object.label());
        assert_eq!(Some("extension-action label prominent install hide"), test_object.class().as_deref());
    });
}

#[test]
fn test_installinglabelaction_when_state_is_installing() {
    run_actions(|f| async move {
        let test_object = f.add(InstallingLabelAction::new());
        f.containers(vec![c(&test_object)]);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));
        f.fixture.management.install_event.fire(install_event(&gallery));

        assert!(!test_object.enabled());
        assert_eq!("Installing", test_object.label());
        assert_eq!(Some("extension-action label install installing"), test_object.class().as_deref());
    });
}

#[test]
fn test_install_action_when_state_is_uninstalled() {
    run_actions(|f| async move {
        let test_object = f.add(InstallAction::new(options(false), &f.services()));
        f.containers(vec![c(&test_object)]);
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let paged = query_gallery(&f).await;
        let promise = test_object.on_did_change().filter(|e| e.enabled == Some(true), None).to_promise(None);
        test_object.set_extension(Some(paged[0].clone()));
        promise.await.expect("onDidChange dispara");
        assert!(test_object.enabled());
        assert_eq!("Install", test_object.label());
    });
}

#[test]
fn test_install_action_when_extension_is_system_action() {
    run_actions(|f| async move {
        let test_object = f.add(InstallAction::new(options(false), &f.services()));
        f.containers(vec![c(&test_object)]);
        let local = local_system();
        f.fixture.management.stub_installed(vec![local.clone()]);

        let extensions = query_local(&f).await;
        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        f.fixture.management.did_uninstall_event.fire(did_uninstall_event(&local.identifier));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_install_action_when_extension_doesnot_has_gallery() {
    run_actions(|f| async move {
        let test_object = f.add(InstallAction::new(options(false), &f.services()));
        f.containers(vec![c(&test_object)]);
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);

        let extensions = query_local(&f).await;
        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        f.fixture.management.did_uninstall_event.fire(did_uninstall_event(&local.identifier));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn uninstall_action_is_disabled_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(UninstallAction::new(&f.services()));
        f.containers(vec![c(&test_object)]);

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_uninstall_action_when_state_is_uninstalling() {
    run_actions(|f| async move {
        let test_object = f.add(UninstallAction::new(&f.services()));
        f.containers(vec![c(&test_object)]);
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        assert!(!test_object.enabled());
        assert_eq!("Uninstalling", test_object.label());
        assert_eq!(Some("extension-action label uninstall uninstalling"), test_object.class().as_deref());
    });
}

#[test]
fn test_uninstall_action_when_state_is_installed_and_is_user_extension() {
    run_actions(|f| async move {
        let test_object = f.add(UninstallAction::new(&f.services()));
        f.containers(vec![c(&test_object)]);
        f.fixture.management.stub_installed(vec![local_a()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Uninstall", test_object.label());
        assert_eq!(Some("extension-action label uninstall"), test_object.class().as_deref());
    });
}

#[test]
fn test_uninstall_action_when_state_is_installed_and_is_system_extension() {
    run_actions(|f| async move {
        let test_object = f.add(UninstallAction::new(&f.services()));
        f.containers(vec![c(&test_object)]);
        f.fixture.management.stub_installed(vec![local_system()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
        assert_eq!("Uninstall", test_object.label());
        assert_eq!(Some("extension-action label uninstall"), test_object.class().as_deref());
    });
}

#[test]
fn test_uninstall_action_when_state_is_installing_and_is_user_extension() {
    run_actions(|f| async move {
        let test_object = f.add(UninstallAction::new(&f.services()));
        f.containers(vec![c(&test_object)]);
        f.fixture.management.stub_installed(vec![local_a()]);

        let extensions = query_local(&f).await;
        let gallery = gallery("a");
        let extension = extensions[0].clone();
        extension.set_gallery(Some(Rc::new(gallery.clone())));
        f.fixture.management.install_event.fire(install_event(&gallery));
        test_object.set_extension(Some(extension));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_uninstall_action_after_extension_is_installed() {
    run_actions(|f| async move {
        let test_object = f.add(UninstallAction::new(&f.services()));
        f.containers(vec![c(&test_object)]);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);

        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));

        f.fixture.management.install_event.fire(install_event(&gallery));
        let promise = test_object.on_did_change().to_promise(None);
        did_install_from_gallery(&f, &gallery, InstallOperation::Install, local_from_gallery(&gallery));

        promise.await.expect("onDidChange dispara");
        assert!(test_object.enabled());
        assert_eq!("Uninstall", test_object.label());
        assert_eq!(Some("extension-action label uninstall"), test_object.class().as_deref());
    });
}

#[test]
fn test_updateaction_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(UpdateAction::new(false, &f.services()));
        f.containers(vec![c(&test_object)]);

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_updateaction_when_extension_is_uninstalled() {
    run_actions(|f| async move {
        let test_object = f.add(UpdateAction::new(false, &f.services()));
        f.containers(vec![c(&test_object)]);
        f.fixture.gallery.stub_query(vec![gallery_version("1.0.0")]);

        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_updateaction_when_extension_is_installed_and_not_outdated() {
    run_actions(|f| async move {
        let test_object = f.add(UpdateAction::new(false, &f.services()));
        f.containers(vec![c(&test_object)]);
        let local = local_manifest(json!({ "version": "1.0.0" }));
        f.fixture.management.stub_installed(vec![local.clone()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        f.fixture.gallery.stub_query(vec![gallery_version(&local.manifest.version)]);
        query_gallery(&f).await;
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_updateaction_when_extension_is_installed_outdated_and_system_extension() {
    run_actions(|f| async move {
        let test_object = f.add(UpdateAction::new(false, &f.services()));
        f.containers(vec![c(&test_object)]);
        let local = a_local("a", json!({ "version": "1.0.0" }), LocalProperties { extension_type: Some(ExtensionType::System), ..LocalProperties::default() });
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        f.fixture.gallery.stub_query(vec![gallery_version("1.0.1")]);
        query_gallery(&f).await;
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_updateaction_when_extension_is_installed_outdated_and_user_extension() {
    run_actions(|f| async move {
        let test_object = f.add(UpdateAction::new(false, &f.services()));
        f.containers(vec![c(&test_object)]);
        let local = local_manifest(json!({ "version": "1.0.0" }));
        f.fixture.management.stub_installed(vec![local]);

        let workbench_service = f.workbench_service();
        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        let gallery = gallery_version("1.0.1");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        f.fixture.gallery.stub_get_compatible_extension(gallery.clone());
        f.fixture.gallery.stub_get_extensions(vec![gallery]);
        assert!(!test_object.enabled());
        let observed = test_object.clone();
        let enabled = test_object.on_did_change().filter(move |_| observed.enabled(), None).to_promise(None);
        let _ = workbench_service.query_gallery(QueryOptions::default(), CancellationToken::None);
        enabled.await.expect("onDidChange dispara");
    });
}

#[test]
fn test_updateaction_when_extension_is_installing_and_outdated_and_user_extension() {
    run_actions(|f| async move {
        let test_object = f.add(UpdateAction::new(false, &f.services()));
        f.containers(vec![c(&test_object)]);
        let local = local_manifest(json!({ "version": "1.0.0" }));
        f.fixture.management.stub_installed(vec![local.clone()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        let gallery = gallery_version("1.0.1");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        f.fixture.gallery.stub_get_compatible_extension(gallery.clone());
        f.fixture.gallery.stub_get_extensions(vec![gallery.clone()]);
        let observed = test_object.clone();
        let enabled = test_object.on_did_change().filter(move |_| observed.enabled(), None).to_promise(None);
        let _ = f.workbench_service().query_gallery(QueryOptions::default(), CancellationToken::None);
        enabled.await.expect("onDidChange dispara");
        let observed = test_object.clone();
        let disabled = test_object.on_did_change().filter(move |_| !observed.enabled(), None).to_promise(None);
        f.fixture.management.install_event.fire(install_event_for(&local.identifier, &gallery));
        disabled.await.expect("onDidChange dispara");
    });
}

#[test]
fn test_manageextensionaction_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(ManageExtensionAction::new());
        f.containers(vec![c(&test_object)]);

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_manageextensionaction_when_extension_is_installed() {
    run_actions(|f| async move {
        let test_object = f.add(ManageExtensionAction::new());
        f.containers(vec![c(&test_object)]);
        f.fixture.management.stub_installed(vec![local_a()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!(Some("extension-action icon manage codicon codicon-extensions-manage"), test_object.class().as_deref());
        assert_eq!("Manage", test_object.tooltip());
    });
}

#[test]
fn test_manageextensionaction_when_extension_is_uninstalled() {
    run_actions(|f| async move {
        let test_object = f.add(ManageExtensionAction::new());
        f.containers(vec![c(&test_object)]);
        f.fixture.gallery.stub_query(vec![gallery("a")]);

        let page = query_gallery(&f).await;
        test_object.set_extension(Some(page[0].clone()));
        assert!(!test_object.enabled());
        assert_eq!(Some("extension-action icon manage codicon codicon-extensions-manage hide"), test_object.class().as_deref());
        assert_eq!("Manage", test_object.tooltip());
    });
}

#[test]
fn test_manageextensionaction_when_extension_is_installing() {
    run_actions(|f| async move {
        let test_object = f.add(ManageExtensionAction::new());
        f.containers(vec![c(&test_object)]);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);

        let page = query_gallery(&f).await;
        test_object.set_extension(Some(page[0].clone()));

        f.fixture.management.install_event.fire(install_event(&gallery));
        assert!(!test_object.enabled());
        assert_eq!(Some("extension-action icon manage codicon codicon-extensions-manage hide"), test_object.class().as_deref());
        assert_eq!("Manage", test_object.tooltip());
    });
}

#[test]
fn test_manageextensionaction_when_extension_is_queried_from_gallery_and_installed() {
    run_actions(|f| async move {
        let test_object = f.add(ManageExtensionAction::new());
        f.containers(vec![c(&test_object)]);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);

        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));
        f.fixture.management.install_event.fire(install_event(&gallery));
        let promise = test_object.on_did_change().to_promise(None);
        did_install_from_gallery(&f, &gallery, InstallOperation::Install, local_from_gallery(&gallery));

        promise.await.expect("onDidChange dispara");
        assert!(test_object.enabled());
        assert_eq!(Some("extension-action icon manage codicon codicon-extensions-manage"), test_object.class().as_deref());
        assert_eq!("Manage", test_object.tooltip());
    });
}

#[test]
fn test_manageextensionaction_when_extension_is_system_extension() {
    run_actions(|f| async move {
        let test_object = f.add(ManageExtensionAction::new());
        f.containers(vec![c(&test_object)]);
        f.fixture.management.stub_installed(vec![local_system()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!(Some("extension-action icon manage codicon codicon-extensions-manage"), test_object.class().as_deref());
        assert_eq!("Manage", test_object.tooltip());
    });
}

#[test]
fn test_manageextensionaction_when_extension_is_uninstalling() {
    run_actions(|f| async move {
        let test_object = f.add(ManageExtensionAction::new());
        f.containers(vec![c(&test_object)]);
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));

        assert!(!test_object.enabled());
        assert_eq!(Some("extension-action icon manage codicon codicon-extensions-manage"), test_object.class().as_deref());
        assert_eq!("Manage", test_object.tooltip());
    });
}

#[test]
fn test_enableforworkspaceaction_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(EnableForWorkspaceAction::new(&f.services()));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enableforworkspaceaction_when_there_extension_is_not_disabled() {
    run_actions(|f| async move {
        f.fixture.management.stub_installed(vec![local_a()]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableForWorkspaceAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enableforworkspaceaction_when_the_extension_is_disabled_globally() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableForWorkspaceAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_enableforworkspaceaction_when_extension_is_disabled_for_workspace() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledWorkspace).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableForWorkspaceAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_enableforworkspaceaction_when_the_extension_is_disabled_globally_and_workspace() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        set_enablement(&f, &local, E::DisabledWorkspace).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableForWorkspaceAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_enablegloballyaction_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(EnableGloballyAction::new(&f.services()));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enablegloballyaction_when_the_extension_is_not_disabled() {
    run_actions(|f| async move {
        f.fixture.management.stub_installed(vec![local_a()]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enablegloballyaction_when_the_extension_is_disabled_for_workspace() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledWorkspace).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enablegloballyaction_when_the_extension_is_disabled_globally() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_enablegloballyaction_when_the_extension_is_disabled_in_both() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        set_enablement(&f, &local, E::DisabledWorkspace).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_enableaction_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(EnableDropDownAction::new(&f.services()));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enabledropdownaction_when_extension_is_installed_and_enabled() {
    run_actions(|f| async move {
        f.fixture.management.stub_installed(vec![local_a()]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableDropDownAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enabledropdownaction_when_extension_is_installed_and_disabled_globally() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableDropDownAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_enabledropdownaction_when_extension_is_installed_and_disabled_for_workspace() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledWorkspace).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableDropDownAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_enabledropdownaction_when_extension_is_uninstalled() {
    run_actions(|f| async move {
        f.fixture.gallery.stub_query(vec![gallery("a")]);

        let page = query_gallery(&f).await;
        let test_object = f.add(EnableDropDownAction::new(&f.services()));
        test_object.set_extension(Some(page[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enabledropdownaction_when_extension_is_installing() {
    run_actions(|f| async move {
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);

        let page = query_gallery(&f).await;
        let test_object = f.add(EnableDropDownAction::new(&f.services()));
        test_object.set_extension(Some(page[0].clone()));
        f.containers(vec![c(&test_object)]);

        f.fixture.management.install_event.fire(install_event(&gallery));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_enabledropdownaction_when_extension_is_uninstalling() {
    run_actions(|f| async move {
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);

        let extensions = query_local(&f).await;
        let test_object = f.add(EnableDropDownAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disableforworkspaceaction_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(DisableForWorkspaceAction::new(&f.services()));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disableforworkspaceaction_when_the_extension_is_disabled_globally() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableForWorkspaceAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disableforworkspaceaction_when_the_extension_is_disabled_workspace() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableForWorkspaceAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disableforworkspaceaction_when_extension_is_enabled() {
    run_actions(|f| async move {
        let local = local_a();
        stub_running(&f, &[&local], false, false);

        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableForWorkspaceAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = f.add(DisableGloballyAction::new(&f.services()));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_the_extension_is_disabled_globally() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_the_extension_is_disabled_for_workspace() {
    run_actions(|f| async move {
        let local = local_a();
        set_enablement(&f, &local, E::DisabledWorkspace).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_the_extension_is_enabled() {
    run_actions(|f| async move {
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);
        stub_running(&f, &[&local], false, false);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_extension_is_installed_and_enabled() {
    run_actions(|f| async move {
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);
        stub_running(&f, &[&local], false, false);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_extension_is_installed_and_disabled_globally() {
    run_actions(|f| async move {
        let local = local_a();
        stub_running(&f, &[&local], false, false);

        set_enablement(&f, &local, E::DisabledGlobally).await;
        f.fixture.management.stub_installed(vec![local]);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_extension_is_uninstalled() {
    run_actions(|f| async move {
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        stub_running(&f, &[&local_a()], false, false);

        let page = query_gallery(&f).await;
        let test_object = f.add(DisableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(page[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_extension_is_installing() {
    run_actions(|f| async move {
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        stub_running(&f, &[&local_a()], false, false);

        let page = query_gallery(&f).await;
        let test_object = f.add(DisableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(page[0].clone()));
        f.containers(vec![c(&test_object)]);
        f.fixture.management.install_event.fire(install_event(&gallery));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_disablegloballyaction_when_extension_is_uninstalling() {
    run_actions(|f| async move {
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);
        stub_running(&f, &[&local], false, false);

        let extensions = query_local(&f).await;
        let test_object = f.add(DisableGloballyAction::new(&f.services()));
        test_object.set_extension(Some(extensions[0].clone()));
        f.containers(vec![c(&test_object)]);
        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        assert!(!test_object.enabled());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Suíte `ExtensionRuntimeStateAction`.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn test_runtime_state_when_there_is_no_extension() {
    run_actions(|f| async move {
        let test_object = runtime_action(&f);

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_state_is_installing() {
    run_actions(|f| async move {
        let test_object = runtime_action(&f);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));
        f.fixture.management.install_event.fire(install_event(&gallery));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_state_is_uninstalling() {
    run_actions(|f| async move {
        let test_object = runtime_action(&f);
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);

        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_newly_installed() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_named("b")], false, false);
        let test_object = runtime_action(&f);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);

        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));
        assert!(!test_object.enabled());

        f.fixture.management.install_event.fire(install_event(&gallery));
        let promise = test_object.on_did_change().to_promise(None);
        did_install_from_gallery(&f, &gallery, InstallOperation::Install, local_from_gallery(&gallery));
        promise.await.expect("onDidChange dispara");
        assert!(test_object.enabled());
        assert_eq!(test_object.tooltip(), "Please restart extensions to enable this extension.");
    });
}

#[test]
fn test_runtime_state_when_extension_is_newly_installed_and_ext_host_restart_is_not_required() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_named("b")], true, false);
        f.recreate_workbench_service();
        let test_object = runtime_action(&f);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);

        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));
        assert!(!test_object.enabled());

        f.fixture.management.install_event.fire(install_event(&gallery));
        did_install_from_gallery(&f, &gallery, InstallOperation::Install, local_from_gallery(&gallery));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_installed_and_uninstalled() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_named("b")], false, false);
        let test_object = runtime_action(&f);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        let paged = query_gallery(&f).await;

        test_object.set_extension(Some(paged[0].clone()));
        let identifier = gallery.identifier.clone();
        f.fixture.management.install_event.fire(install_event(&gallery));
        f.fixture.management.did_install_event.fire(vec![did_install_result(
            &identifier,
            Some(InstallExtensionSource::Gallery(gallery.clone())),
            InstallOperation::Install,
            a_local("a", json!({}), LocalProperties { identifier: Some(identifier.clone()), ..LocalProperties::default() }),
        )]);
        f.fixture.management.uninstall_event.fire(uninstall_event(&identifier));
        f.fixture.management.did_uninstall_event.fire(did_uninstall_event(&identifier));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_uninstalled() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_manifest(json!({ "version": "1.0.0" }))], false, false);
        f.recreate_workbench_service();
        let test_object = runtime_action(&f);
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);
        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));

        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        f.fixture.management.did_uninstall_event.fire(did_uninstall_event(&local.identifier));
        assert!(test_object.enabled());
        assert_eq!(test_object.tooltip(), "Please restart extensions to complete the uninstallation of this extension.");
    });
}

#[test]
fn test_runtime_state_when_extension_is_uninstalled_and_can_be_removed() {
    run_actions(|f| async move {
        let local = local_a();
        stub_running(&f, &[&local], true, true);
        let test_object = runtime_action(&f);
        f.fixture.management.stub_installed(vec![local.clone()]);
        let extensions = query_local(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));

        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        f.fixture.management.did_uninstall_event.fire(did_uninstall_event(&local.identifier));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_uninstalled_and_installed() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_manifest(json!({ "version": "1.0.0" }))], false, false);
        f.recreate_workbench_service();
        let test_object = runtime_action(&f);
        let local = local_a();
        f.fixture.management.stub_installed(vec![local.clone()]);
        let extensions = query_local(&f).await;

        test_object.set_extension(Some(extensions[0].clone()));
        f.fixture.management.uninstall_event.fire(uninstall_event(&local.identifier));
        f.fixture.management.did_uninstall_event.fire(did_uninstall_event(&local.identifier));

        let gallery = gallery("a");
        let identifier = gallery.identifier.clone();
        f.fixture.management.install_event.fire(install_event(&gallery));
        f.fixture.management.did_install_event.fire(vec![did_install_result(&identifier, Some(InstallExtensionSource::Gallery(gallery.clone())), InstallOperation::Install, local)]);

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_updated_while_running() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_manifest(json!({ "version": "1.0.1" }))], false, true);
        f.recreate_workbench_service();
        let test_object = runtime_action(&f);
        let local = local_manifest(json!({ "version": "1.0.1" }));
        let workbench_service = f.workbench_service();
        f.fixture.management.stub_installed(vec![local.clone()]);
        let extensions = workbench_service.query_local(None).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));

        let observed = test_object.clone();
        let updated = test_object
            .on_did_change()
            .filter(move |_| observed.enabled() && observed.tooltip() == "Please restart extensions to enable the updated extension.", None)
            .to_promise(None);
        let gallery = gallery_version("1.0.2");
        f.fixture.management.install_event.fire(install_event(&gallery));
        did_install_from_gallery(&f, &gallery, InstallOperation::Install, local_from_gallery(&gallery));
        updated.await.expect("onDidChange dispara");
    });
}

#[test]
fn test_runtime_state_when_extension_is_updated_when_not_running() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_named("b")], false, false);
        let local = local_manifest(json!({ "version": "1.0.1" }));
        set_enablement(&f, &local, E::DisabledGlobally).await;
        let test_object = runtime_action(&f);
        let workbench_service = f.workbench_service();
        f.fixture.management.stub_installed(vec![local.clone()]);
        let extensions = workbench_service.query_local(None).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));

        let gallery = gallery_version("1.0.2");
        f.fixture.management.install_event.fire(install_event(&gallery));
        did_install_from_gallery(&f, &gallery, InstallOperation::Update, local_from_gallery(&gallery));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_disabled_when_running() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_a()], false, false);
        f.recreate_workbench_service();
        let test_object = runtime_action(&f);
        let local = local_a();
        let workbench_service = f.workbench_service();
        f.fixture.management.stub_installed(vec![local]);
        let extensions = workbench_service.query_local(None).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        workbench_service.set_enablement(vec![extensions[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        test_object.update();

        assert!(test_object.enabled());
        assert_eq!("Please restart extensions to disable this extension.", test_object.tooltip());
    });
}

#[test]
fn test_runtime_state_when_extension_enablement_is_toggled_when_running() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_manifest(json!({ "version": "1.0.0" }))], false, false);
        f.recreate_workbench_service();
        let test_object = runtime_action(&f);
        let local = local_a();
        let workbench_service = f.workbench_service();
        f.fixture.management.stub_installed(vec![local]);
        let extensions = workbench_service.query_local(None).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        workbench_service.set_enablement(vec![extensions[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        workbench_service.set_enablement(vec![extensions[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_enabled_when_not_running() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_named("b")], false, false);
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        let test_object = runtime_action(&f);
        let workbench_service = f.workbench_service();
        f.fixture.management.stub_installed(vec![local]);
        let extensions = workbench_service.query_local(None).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        workbench_service.set_enablement(vec![extensions[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        test_object.update();
        assert!(test_object.enabled());
        assert_eq!("Please restart extensions to enable this extension.", test_object.tooltip());
    });
}

#[test]
fn test_runtime_state_when_extension_enablement_is_toggled_when_not_running() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_named("b")], false, false);
        let local = local_a();
        set_enablement(&f, &local, E::DisabledGlobally).await;
        let test_object = runtime_action(&f);
        let workbench_service = f.workbench_service();
        f.fixture.management.stub_installed(vec![local]);
        let extensions = workbench_service.query_local(None).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        workbench_service.set_enablement(vec![extensions[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        workbench_service.set_enablement(vec![extensions[0].clone()], E::DisabledGlobally).await.expect("setEnablement resolve");
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_updated_when_not_running_and_enabled() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_a()], false, false);
        let local = local_manifest(json!({ "version": "1.0.1" }));
        set_enablement(&f, &local, E::DisabledGlobally).await;
        let test_object = runtime_action(&f);
        let workbench_service = f.workbench_service();
        f.fixture.management.stub_installed(vec![local.clone()]);
        let extensions = workbench_service.query_local(None).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));

        let gallery = gallery_version("1.0.2");
        f.fixture.management.install_event.fire(install_event(&gallery));
        did_install_from_gallery(&f, &gallery, InstallOperation::Install, local_from_gallery(&gallery));
        workbench_service.set_enablement(vec![extensions[0].clone()], E::EnabledGlobally).await.expect("setEnablement resolve");
        test_object.update();
        assert!(test_object.enabled());
        assert_eq!("Please restart extensions to enable this extension.", test_object.tooltip());
    });
}

#[test]
fn test_runtime_state_when_a_localization_extension_is_newly_installed() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_named("b")], false, false);
        let test_object = runtime_action(&f);
        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);

        let paged = query_gallery(&f).await;
        test_object.set_extension(Some(paged[0].clone()));
        assert!(!test_object.enabled());

        f.fixture.management.install_event.fire(install_event(&gallery));
        did_install_from_gallery(&f, &gallery, InstallOperation::Install, language_pack_from_gallery(&gallery));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_a_localization_extension_is_updated_while_running() {
    run_actions(|f| async move {
        stub_running(&f, &[&local_manifest(json!({ "version": "1.0.1" }))], false, false);
        let test_object = runtime_action(&f);
        let local = local_manifest(json!({ "version": "1.0.1", "contributes": { "localizations": [{ "languageId": "de", "translations": [] }] } }));
        let workbench_service = f.workbench_service();
        f.fixture.management.stub_installed(vec![local]);
        let extensions = workbench_service.query_local(None).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));

        let gallery = gallery_version("1.0.2");
        f.fixture.management.install_event.fire(install_event(&gallery));
        did_install_from_gallery(&f, &gallery, InstallOperation::Install, language_pack_from_gallery(&gallery));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_not_installed_but_extension_from_different_server_is_installed_and_running() {
    run_actions(|f| async move {
        // multi server setup
        let gallery = gallery("a");
        let local_extension = local_kinds(&["workspace"], file_location());
        let remote_extension = local_kinds(&["workspace"], remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_extension])),
            ServerSlot::With(f.create_extension_management_service(vec![remote_extension.clone()])),
            None,
        );
        f.use_servers(servers.clone());
        stub_running(&f, &[&remote_extension], false, false);
        let workbench_service = f.recreate_workbench_service();

        let test_object = runtime_action(&f);
        f.fixture.gallery.stub_query(vec![gallery]);

        query_gallery(&f).await;
        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_extension_is_uninstalled_but_extension_from_different_server_is_installed_and_running() {
    run_actions(|f| async move {
        // multi server setup
        let gallery = gallery("a");
        let local_extension = local_kinds(&["workspace"], file_location());
        let remote_extension = local_kinds(&["workspace"], remote_location());
        let local_management_service = f.create_extension_management_service(vec![local_extension.clone()]);
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(local_management_service.clone()),
            ServerSlot::With(f.create_extension_management_service(vec![remote_extension.clone()])),
            None,
        );
        f.use_servers(servers.clone());
        stub_running(&f, &[&remote_extension], false, false);
        let workbench_service = f.recreate_workbench_service();

        let test_object = runtime_action(&f);
        f.fixture.gallery.stub_query(vec![gallery]);

        query_gallery(&f).await;
        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());

        local_management_service.uninstall_event.fire(uninstall_event(&local_extension.identifier));
        f.fixture.management.did_uninstall_event.fire(did_uninstall_event(&local_extension.identifier));

        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_workspace_extension_is_disabled_on_local_server_and_installed_in_remote_server() {
    run_actions(|f| async move {
        // multi server setup
        let gallery = gallery("a");
        let remote_management_service = f.create_extension_management_service(Vec::new());
        let local_extension = local_kinds(&["workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_extension])),
            ServerSlot::With(remote_management_service.clone()),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        stub_running(&f, &[], false, false);
        let test_object = runtime_action(&f);
        f.fixture.gallery.stub_query(vec![gallery]);

        query_gallery(&f).await;
        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());

        let remote_extension = local_kinds(&["workspace"], remote_location());
        let promise = test_object.on_did_change().to_promise(None);
        remote_management_service.did_install_event.fire(vec![did_install_result(&remote_extension.identifier, None, InstallOperation::Install, remote_extension.clone())]);

        promise.await.expect("onDidChange dispara");
        assert!(test_object.enabled());
        assert_eq!(test_object.tooltip(), "Please reload window to enable this extension.");
    });
}

#[test]
fn test_runtime_state_when_ui_extension_is_disabled_on_remote_server_and_installed_in_local_server() {
    run_actions(|f| async move {
        // multi server setup
        let gallery = gallery("a");
        let local_management_service = f.create_extension_management_service(Vec::new());
        let remote_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(local_management_service.clone()),
            ServerSlot::With(f.create_extension_management_service(vec![remote_extension])),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        stub_running(&f, &[], false, false);
        let test_object = runtime_action(&f);
        f.fixture.gallery.stub_query(vec![gallery]);

        query_gallery(&f).await;
        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());

        let local_extension = local_kinds(&["ui"], file_location());
        let observed = test_object.clone();
        let promise = test_object.on_did_change().filter(move |_| observed.enabled(), None).to_promise(None);
        local_management_service.did_install_event.fire(vec![did_install_result(&local_extension.identifier, None, InstallOperation::Install, local_extension.clone())]);

        promise.await.expect("onDidChange dispara");
        assert!(test_object.enabled());
        assert_eq!(test_object.tooltip(), "Please reload window to enable this extension.");
    });
}

/// Os casos `Test Runtime State for ... extension is ... when it is installed ...`: o cenário de vários servidores, a
/// extensão consultada no servidor dado e a expectativa de `enabled`. `stub_first` é a ordem do original: o
/// `IExtensionService` é registrado antes (verdadeiro) ou depois (falso) de criar o `ExtensionsWorkbenchService`, que só
/// vê o serviço que já estava registrado.
async fn runtime_state_in_servers(
    f: &ActionsFixture,
    local: Option<ExtensionRef>,
    remote: Option<ExtensionRef>,
    running_extension: &ExtensionRef,
    query_remote: bool,
    stub_first: bool,
) -> bool {
    let gallery = gallery("a");
    let servers = f.a_multi_extension_management_server_service(
        ServerSlot::With(f.create_extension_management_service(local.into_iter().collect())),
        ServerSlot::With(f.create_extension_management_service(remote.into_iter().collect())),
        None,
    );
    f.use_servers(servers.clone());
    if stub_first {
        stub_running(f, &[running_extension], false, false);
    }
    let workbench_service = f.recreate_workbench_service();
    if !stub_first {
        stub_running(f, &[running_extension], false, false);
    }
    let test_object = runtime_action(f);
    f.fixture.gallery.stub_query(vec![gallery]);

    query_gallery(f).await;
    let server = if query_remote { servers.remote_extension_management_server() } else { servers.local_extension_management_server() };
    let extensions = workbench_service.query_local(server).await.expect("queryLocal resolve");
    test_object.set_extension(Some(extensions[0].clone()));
    assert!(test_object.extension().is_some());
    test_object.enabled()
}

#[test]
fn test_runtime_state_for_remote_ui_extension_is_disabled_when_it_is_installed_and_enabled_in_local_server() {
    run_actions(|f| async move {
        let local_extension = local_kinds(&["ui"], file_location());
        let remote_extension = local_kinds(&["ui"], remote_location());
        let enabled = runtime_state_in_servers(&f, Some(local_extension.clone()), Some(remote_extension), &local_extension, true, true).await;
        assert!(!enabled);
    });
}

#[test]
fn test_runtime_state_for_remote_workspace_ui_extension_is_enabled_when_it_is_installed_and_enabled_in_local_server() {
    run_actions(|f| async move {
        let local_extension = local_kinds(&["workspace", "ui"], file_location());
        let remote_extension = local_kinds(&["workspace", "ui"], remote_location());
        let enabled = runtime_state_in_servers(&f, Some(local_extension.clone()), Some(remote_extension), &local_extension, true, false).await;
        assert!(enabled);
    });
}

#[test]
fn test_runtime_state_for_local_ui_workspace_extension_is_enabled_when_it_is_installed_and_enabled_in_remote_server() {
    run_actions(|f| async move {
        let local_extension = local_kinds(&["ui", "workspace"], file_location());
        let remote_extension = local_kinds(&["ui", "workspace"], remote_location());
        let enabled = runtime_state_in_servers(&f, Some(local_extension), Some(remote_extension.clone()), &remote_extension, false, false).await;
        assert!(enabled);
    });
}

#[test]
fn test_runtime_state_for_local_workspace_ui_extension_is_enabled_when_it_is_installed_in_both_servers_but_running_in_local_server() {
    run_actions(|f| async move {
        let local_extension = local_kinds(&["workspace", "ui"], file_location());
        let remote_extension = local_kinds(&["workspace", "ui"], remote_location());
        let enabled = runtime_state_in_servers(&f, Some(local_extension.clone()), Some(remote_extension), &local_extension, false, false).await;
        assert!(enabled);
    });
}

#[test]
fn test_runtime_state_for_remote_ui_workspace_extension_is_enabled_when_it_is_installed_on_both_servers_but_running_in_remote_server() {
    run_actions(|f| async move {
        let local_extension = local_kinds(&["ui", "workspace"], file_location());
        let remote_extension = local_kinds(&["ui", "workspace"], remote_location());
        let enabled = runtime_state_in_servers(&f, Some(local_extension), Some(remote_extension.clone()), &remote_extension, true, false).await;
        assert!(enabled);
    });
}

#[test]
fn test_runtime_state_when_ui_workspace_web_extension_is_installed_in_web_and_remote_and_running_in_remote() {
    run_actions(|f| async move {
        // multi server setup
        let gallery = gallery("a");
        let web_extension = local_at(json!({ "extensionKind": ["ui", "workspace"], "browser": "browser.js" }), web_location());
        let remote_extension = local_at(json!({ "extensionKind": ["ui", "workspace"], "browser": "browser.js" }), remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::Absent,
            ServerSlot::With(f.create_extension_management_service(vec![remote_extension.clone()])),
            Some(f.create_extension_management_service(vec![web_extension])),
        );
        f.use_servers(servers.clone());
        stub_running(&f, &[&remote_extension], false, false);
        let workbench_service = f.recreate_workbench_service();

        let test_object = runtime_action(&f);
        f.fixture.gallery.stub_query(vec![gallery]);

        query_gallery(&f).await;
        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_runtime_state_when_workspace_ui_web_extension_is_installed_in_web_and_local_and_running_in_local() {
    run_actions(|f| async move {
        // multi server setup
        let gallery = gallery("a");
        let web_extension = local_at(json!({ "extensionKind": ["workspace", "ui"], "browser": "browser.js" }), web_location());
        let local_extension = local_at(json!({ "extensionKind": ["workspace", "ui"], "browser": "browser.js" }), file_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_extension.clone()])),
            ServerSlot::Absent,
            Some(f.create_extension_management_service(vec![web_extension])),
        );
        f.use_servers(servers.clone());
        stub_running(&f, &[&local_extension], false, false);
        let workbench_service = f.recreate_workbench_service();

        let test_object = runtime_action(&f);
        f.fixture.gallery.stub_query(vec![gallery]);

        query_gallery(&f).await;
        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Suíte `RemoteInstallAction`.
// ---------------------------------------------------------------------------------------------------------------------

const REMOTE_LABEL_CLASS: &str = "extension-action label prominent install-other-server";
const INSTALLING_OTHER_SERVER_CLASS: &str = "extension-action label install-other-server installing";

/// `instantiationService.createInstance(RemoteInstallAction, canInstallAnyWhere)` e o `ExtensionContainers`.
fn remote_install_action(f: &ActionsFixture, can_install_anywhere: bool) -> Rc<je_workbench::contrib::extensions::extensions_actions::InstallInOtherServerAction> {
    let test_object = f.add(RemoteInstallAction::new(can_install_anywhere, &f.services()));
    f.containers(vec![c(&test_object)]);
    test_object
}

/// `instantiationService.createInstance(LocalInstallAction)` e o `ExtensionContainers`.
fn local_install_action(f: &ActionsFixture) -> Rc<je_workbench::contrib::extensions::extensions_actions::InstallInOtherServerAction> {
    let test_object = f.add(LocalInstallAction::new(&f.services()));
    f.containers(vec![c(&test_object)]);
    test_object
}

#[test]
fn test_remote_install_action_is_enabled_for_local_workspace_extension() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_remote_install_action_when_installing_local_workspace_extension() {
    run_actions(|f| async move {
        // multi server setup
        let remote_management_service = f.create_extension_management_service(Vec::new());
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension.clone()])),
            ServerSlot::With(remote_management_service.clone()),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());

        remote_management_service.install_event.fire(install_event_for(&local_workspace_extension.identifier, &gallery));
        assert!(test_object.enabled());
        assert_eq!("Installing", test_object.label());
        assert_eq!(Some(INSTALLING_OTHER_SERVER_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_remote_install_action_when_installing_local_workspace_extension_is_finished() {
    run_actions(|f| async move {
        // multi server setup
        let remote_management_service = f.create_extension_management_service(Vec::new());
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension.clone()])),
            ServerSlot::With(remote_management_service.clone()),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());

        remote_management_service.install_event.fire(install_event_for(&local_workspace_extension.identifier, &gallery));
        assert!(test_object.enabled());
        assert_eq!("Installing", test_object.label());
        assert_eq!(Some(INSTALLING_OTHER_SERVER_CLASS), test_object.class().as_deref());

        let installed_extension = local_kinds(&["workspace"], remote_location());
        let promise = test_object.on_did_change().to_promise(None);
        remote_management_service.did_install_event.fire(vec![did_install_result(&installed_extension.identifier, None, InstallOperation::Install, installed_extension.clone())]);
        promise.await.expect("onDidChange dispara");
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_enabled_for_disabled_local_workspace_extension() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        let remote_workspace_extension = local_kinds(&["workspace"], remote_location());
        set_enablement(&f, &remote_workspace_extension, E::DisabledGlobally).await;
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_remote_install_action_is_enabled_local_workspace_ui_extension() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["workspace", "ui"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension.clone()])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        set_enablement(&f, &local_workspace_extension, E::DisabledGlobally).await;
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_remote_install_action_is_enabled_for_local_ui_workapace_extension_if_can_install_is_true() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["ui", "workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension.clone()])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        set_enablement(&f, &local_workspace_extension, E::DisabledGlobally).await;
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, true);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_remote_install_action_is_disabled_for_local_ui_workapace_extension_if_can_install_is_false() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["ui", "workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension.clone()])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        set_enablement(&f, &local_workspace_extension, E::DisabledGlobally).await;
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_when_extension_is_not_set() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension])), ServerSlot::Default, None);
        f.fixture.restub_enablement_service();
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_for_extension_which_is_not_installed() {
    run_actions(|f| async move {
        // multi server setup
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::Default, None);
        f.use_servers(servers);
        f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let pager = query_gallery(&f).await;
        test_object.set_extension(Some(pager[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_for_local_workspace_extension_which_is_disabled_in_env() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension])), ServerSlot::Default, None);
        f.fixture.stub_environment(je_workbench::test::browser::workbench_test_services::EnvironmentOverrides {
            disable_extensions: Some(je_platform::environment::environment::DisableExtensions::All(true)),
            ..Default::default()
        });
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_when_remote_server_is_not_available() {
    run_actions(|f| async move {
        // single server setup
        let workbench_service = f.workbench_service();
        let servers = f.fixture.server_service();
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        f.fixture.management.stub_installed(vec![local_workspace_extension]);
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_for_local_workspace_extension_if_it_is_uninstalled_locally() {
    run_actions(|f| async move {
        // multi server setup
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.fixture.management.clone()), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        f.fixture.management.stub_installed(vec![local_workspace_extension.clone()]);
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());

        f.fixture.management.uninstall_event.fire(uninstall_event(&local_workspace_extension.identifier));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_for_local_workspace_extension_if_it_is_installed_in_remote() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        let remote_workspace_extension = local_kinds(&["workspace"], remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension])),
            ServerSlot::With(f.create_extension_management_service(vec![remote_workspace_extension])),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_enabled_for_local_workspace_extension_if_it_has_not_gallery() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["workspace"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_for_local_workspace_system_extension() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_system_extension =
            a_local("a", json!({ "extensionKind": ["workspace"] }), LocalProperties { location: Some(file_location()), extension_type: Some(ExtensionType::System), ..LocalProperties::default() });
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_workspace_system_extension])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_for_local_ui_extension_if_it_is_not_installed_in_remote() {
    run_actions(|f| async move {
        // multi server setup
        let local_ui_extension = local_kinds(&["ui"], file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![local_ui_extension])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_disabled_for_local_ui_extension_if_it_is_also_installed_in_remote() {
    run_actions(|f| async move {
        // multi server setup
        let local_ui_extension = local_kinds(&["ui"], file_location());
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_ui_extension])),
            ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension])),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_remote_install_action_is_enabled_for_locally_installed_language_pack_extension() {
    run_actions(|f| async move {
        // multi server setup
        let language_pack_extension = local_at(localizations(), file_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.create_extension_management_service(vec![language_pack_extension])), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_remote_install_action_is_disabled_if_local_language_pack_extension_is_uninstalled() {
    run_actions(|f| async move {
        // multi server setup
        let servers = f.a_multi_extension_management_server_service(ServerSlot::With(f.fixture.management.clone()), ServerSlot::Default, None);
        f.use_servers(servers.clone());
        let language_pack_extension = local_at(localizations(), file_location());
        f.fixture.management.stub_installed(vec![language_pack_extension.clone()]);
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = remote_install_action(&f, false);

        let extensions = workbench_service.query_local(servers.local_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install in remote", test_object.label());

        f.fixture.management.uninstall_event.fire(uninstall_event(&language_pack_extension.identifier));
        assert!(!test_object.enabled());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Suíte `LocalInstallAction`.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn test_local_install_action_is_enabled_for_remote_ui_extension() {
    run_actions(|f| async move {
        // multi server setup
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension])), None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install Locally", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_local_install_action_is_enabled_for_remote_ui_workspace_extension() {
    run_actions(|f| async move {
        // multi server setup
        let remote_ui_extension = local_kinds(&["ui", "workspace"], remote_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension])), None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install Locally", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_local_install_action_when_installing_remote_ui_extension() {
    run_actions(|f| async move {
        // multi server setup
        let local_management_service = f.create_extension_management_service(Vec::new());
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(local_management_service.clone()),
            ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension.clone()])),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install Locally", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());

        local_management_service.install_event.fire(install_event_for(&remote_ui_extension.identifier, &gallery));
        assert!(test_object.enabled());
        assert_eq!("Installing", test_object.label());
        assert_eq!(Some(INSTALLING_OTHER_SERVER_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_local_install_action_when_installing_remote_ui_extension_is_finished() {
    run_actions(|f| async move {
        // multi server setup
        let local_management_service = f.create_extension_management_service(Vec::new());
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(local_management_service.clone()),
            ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension.clone()])),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        let gallery = gallery("a");
        f.fixture.gallery.stub_query(vec![gallery.clone()]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install Locally", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());

        local_management_service.install_event.fire(install_event_for(&remote_ui_extension.identifier, &gallery));
        assert!(test_object.enabled());
        assert_eq!("Installing", test_object.label());
        assert_eq!(Some(INSTALLING_OTHER_SERVER_CLASS), test_object.class().as_deref());

        let installed_extension = local_kinds(&["ui"], file_location());
        let promise = test_object.on_did_change().to_promise(None);
        local_management_service.did_install_event.fire(vec![did_install_result(&installed_extension.identifier, None, InstallOperation::Install, installed_extension.clone())]);
        promise.await.expect("onDidChange dispara");
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_enabled_for_disabled_remote_ui_extension() {
    run_actions(|f| async move {
        // multi server setup
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension])), None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        let local_ui_extension = local_kinds(&["ui"], file_location());
        set_enablement(&f, &local_ui_extension, E::DisabledGlobally).await;
        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install Locally", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_local_install_action_is_disabled_when_extension_is_not_set() {
    run_actions(|f| async move {
        // multi server setup
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension])), None);
        f.fixture.restub_enablement_service();
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_disabled_for_extension_which_is_not_installed() {
    run_actions(|f| async move {
        // multi server setup
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::Default, None);
        f.use_servers(servers);
        f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let pager = query_gallery(&f).await;
        test_object.set_extension(Some(pager[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_disabled_for_remote_ui_extension_which_is_disabled_in_env() {
    run_actions(|f| async move {
        // multi server setup
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        f.fixture.stub_environment(je_workbench::test::browser::workbench_test_services::EnvironmentOverrides {
            disable_extensions: Some(je_platform::environment::environment::DisableExtensions::All(true)),
            ..Default::default()
        });
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension])), None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_disabled_when_local_server_is_not_available() {
    run_actions(|f| async move {
        // single server setup
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_single_remote_extension_management_server_service(f.create_extension_management_service(vec![remote_ui_extension]));
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_disabled_for_remote_ui_extension_if_it_is_installed_in_local() {
    run_actions(|f| async move {
        // multi server setup
        let local_ui_extension = local_kinds(&["ui"], file_location());
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_ui_extension])),
            ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension])),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_disabled_for_remoteui_extension_if_it_is_uninstalled_locally() {
    run_actions(|f| async move {
        // multi server setup
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.fixture.management.clone()), None);
        f.use_servers(servers.clone());
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        f.fixture.management.stub_installed(vec![remote_ui_extension.clone()]);
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install Locally", test_object.label());

        f.fixture.management.uninstall_event.fire(uninstall_event(&remote_ui_extension.identifier));
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_enabled_for_remote_ui_extension_if_it_has_gallery() {
    run_actions(|f| async move {
        // multi server setup
        let remote_ui_extension = local_kinds(&["ui"], remote_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![remote_ui_extension])), None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_disabled_for_remote_ui_system_extension() {
    run_actions(|f| async move {
        // multi server setup
        let remote_ui_system_extension =
            a_local("a", json!({ "extensionKind": ["ui"] }), LocalProperties { location: Some(remote_location()), extension_type: Some(ExtensionType::System), ..LocalProperties::default() });
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![remote_ui_system_extension])), None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_disabled_for_remote_workspace_extension_if_it_is_not_installed_in_local() {
    run_actions(|f| async move {
        // multi server setup
        let remote_workspace_extension = local_kinds(&["workspace"], remote_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![remote_workspace_extension])), None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_disabled_for_remote_workspace_extension_if_it_is_also_installed_in_local() {
    run_actions(|f| async move {
        // multi server setup
        let local_workspace_extension = local_kinds(&["workspae"], file_location());
        let remote_workspace_extension = local_kinds(&["workspace"], remote_location());
        let servers = f.a_multi_extension_management_server_service(
            ServerSlot::With(f.create_extension_management_service(vec![local_workspace_extension])),
            ServerSlot::With(f.create_extension_management_service(vec![remote_workspace_extension])),
            None,
        );
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.extension().is_some());
        assert!(!test_object.enabled());
    });
}

#[test]
fn test_local_install_action_is_enabled_for_remotely_installed_language_pack_extension() {
    run_actions(|f| async move {
        // multi server setup
        let language_pack_extension = local_at(localizations(), remote_location());
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.create_extension_management_service(vec![language_pack_extension])), None);
        f.use_servers(servers.clone());
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install Locally", test_object.label());
        assert_eq!(Some(REMOTE_LABEL_CLASS), test_object.class().as_deref());
    });
}

#[test]
fn test_local_install_action_is_disabled_if_remote_language_pack_extension_is_uninstalled() {
    run_actions(|f| async move {
        // multi server setup
        let servers = f.a_multi_extension_management_server_service(ServerSlot::Default, ServerSlot::With(f.fixture.management.clone()), None);
        f.use_servers(servers.clone());
        let language_pack_extension = local_at(localizations(), remote_location());
        f.fixture.management.stub_installed(vec![language_pack_extension.clone()]);
        let workbench_service = f.recreate_workbench_service();

        f.fixture.gallery.stub_query(vec![gallery("a")]);
        let test_object = local_install_action(&f);

        let extensions = workbench_service.query_local(servers.remote_extension_management_server()).await.expect("queryLocal resolve");
        query_gallery(&f).await;
        test_object.set_extension(Some(extensions[0].clone()));
        assert!(test_object.enabled());
        assert_eq!("Install Locally", test_object.label());

        f.fixture.management.uninstall_event.fire(uninstall_event(&language_pack_extension.identifier));
        assert!(!test_object.enabled());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Suíte `EnableAIFeaturesInWorkspaceAction`.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn test_enable_ai_in_workspace_updates_workspace_setting_when_ai_is_disabled_globally() {
    run_actions(|f| async move {
        let mut product = TestProductService::default();
        product.default_chat_agent = Some(DefaultChatAgentExtensions { extension_id: String::new(), chat_extension_id: "GitHub.copilot-chat".to_owned() });
        f.fixture.stub_product_service(product);

        let _ = f.configuration.set_user_configuration(CHAT_AI_DISABLED_SETTING_ID, Dynamic::Bool(true), None);

        let chat_extension = a_local("copilot-chat", json!({ "publisher": "GitHub" }), LocalProperties { extension_type: Some(ExtensionType::System), ..LocalProperties::default() });
        f.fixture.management.stub_installed(vec![chat_extension]);

        let workbench_service = f.workbench_service();
        workbench_service.query_local(None).await.expect("queryLocal resolve");

        let extensions = workbench_service.local();
        let copilot_chat = extensions.iter().find(|e| e.identifier().id == "github.copilot-chat").cloned().expect("a extensão de chat está instalada");

        let test_object = f.add(EnableAIFeaturesInWorkspaceAction::new(&f.services()));
        f.containers(vec![c(&test_object)]);
        test_object.set_extension(Some(copilot_chat));
        assert!(test_object.enabled());

        test_object.run(&[]).await.expect("run resolve");

        let calls = f.configuration.update_value_calls();
        let updated_value = calls.last().expect("updateValue should have been called");
        assert_eq!(updated_value.key, CHAT_AI_DISABLED_SETTING_ID);
        assert_eq!(updated_value.value, Dynamic::Bool(false), "workspace setting should be set to false");
    });
}
