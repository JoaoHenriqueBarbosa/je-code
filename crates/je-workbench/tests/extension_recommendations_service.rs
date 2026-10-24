//! Porte de `src/vs/workbench/contrib/extensions/test/electron-browser/extensionRecommendationsService.test.ts` (suíte `ExtensionRecommendationsService Test`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `setup`, `setUpFolderWorkspace`, `aGalleryExtension`, `mockExtensionGallery`, `mockExtensionLocal` e os dublês de serviço
//!   estão em `support::extension_recommendations_support` (ver a nota do módulo: os serviços não registrados do original são
//!   `None`, o `TestExtensionTipsService` nasce com o serviço, o `TestNotificationService2` é o
//!   `PromptTrackingNotificationService`).
//! - `runWithFakedTimers({ useFakeTimers: true }, ...)` é `run`; o caso `Get file based recommendations from storage (new
//!   format)` não usa timers falsos (`Date.now()` do sistema) e roda em `run_real_clock`.
//! - `Event.toPromise(promptedEmitter.event)` é `to_promise(None)` do evento; `sinon.spy()` de `onDidChangeGlobalIgnoredRecommendation`
//!   é a lista de chamadas que o ouvinte acumula, e `calledWithMatch` confere os dois campos.
//! - `assert.ok(recommendations['x'])` é `contains_key`, e `Object.keys(...)` são as chaves na ordem de inserção.
//! - `galleryQuerySpy.notCalled` é `query_calls() == 0` da galeria de teste.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_workbench::contrib::extensions::extension_recommendations_service::ExtensionRecommendationsService;
use je_workbench::services::extension_recommendations::extension_recommendations::{
    IExtensionIgnoredRecommendationsService, IExtensionRecommendationsService, IgnoredRecommendationChangeNotification,
};
use support::extension_recommendations_support::{
    RECOMMENDED_EXTENSIONS, RecommendationsFixture, VALID_RECOMMENDED_EXTENSIONS, keys_of, mock_extension_gallery, mock_extension_local, run, run_real_clock,
};

const WORKSPACE_RECOMMENDATIONS_IGNORE: &str = "extensionsAssistant/workspaceRecommendationsIgnore";
const RECOMMENDATIONS: &str = "extensionsAssistant/recommendations";
const IGNORED_RECOMMENDATIONS: &str = "extensionsAssistant/ignored_recommendations";

fn store_workspace_flag(fixture: &RecommendationsFixture, key: &str) {
    fixture.base.storage.store(key, true.into(), StorageScope::Workspace, StorageTarget::Machine);
}

fn store_profile_text(fixture: &RecommendationsFixture, key: &str, value: &str) {
    fixture.base.storage.store(key, value.into(), StorageScope::Profile, StorageTarget::Machine);
}

/// `testNoPromptForValidRecommendations(recommendations)`.
async fn test_no_prompt_for_valid_recommendations(fixture: &Rc<RecommendationsFixture>, recommendations: &[&str]) {
    fixture.set_up_folder_workspace("myFolder", recommendations, &[]).await;
    let test_object = fixture.create_service();
    test_object.activation_promise().await.expect("activationPromise");
    assert_eq!(keys_of(&test_object.get_all_recommendations_with_reason()).len(), recommendations.len());
    assert!(!fixture.notification.prompted());
}

/// `testNoPromptOrRecommendationsForValidRecommendations(recommendations)` (o argumento do original é ignorado: a pasta usa
/// sempre `validRecommendedExtensions`).
async fn test_no_prompt_or_recommendations_for_valid_recommendations(fixture: &Rc<RecommendationsFixture>) {
    fixture.set_up_folder_workspace("myFolder", &VALID_RECOMMENDED_EXTENSIONS, &[]).await;
    let test_object = fixture.create_service();
    assert!(!fixture.notification.prompted());

    test_object.get_workspace_recommendations().await.expect("getWorkspaceRecommendations");
    assert_eq!(keys_of(&test_object.get_all_recommendations_with_reason()).len(), 0);
    assert!(!fixture.notification.prompted());
}

#[test]
fn no_prompt_for_valid_workspace_recommendations_when_gallery_service_is_absent() {
    run(|fixture| async move {
        fixture.gallery.set_enabled(false);

        test_no_prompt_or_recommendations_for_valid_recommendations(&fixture).await;
        assert_eq!(fixture.gallery.query_calls(), 0);
    });
}

#[test]
fn no_prompt_for_valid_workspace_recommendations_during_extension_development() {
    run(|fixture| async move {
        fixture.stub_extension_development(true);
        test_no_prompt_or_recommendations_for_valid_recommendations(&fixture).await;
    });
}

#[test]
fn no_workspace_recommendations_or_prompts_when_extensions_json_has_empty_array() {
    run(|fixture| async move {
        test_no_prompt_for_valid_recommendations(&fixture, &[]).await;
    });
}

#[test]
fn prompt_for_valid_workspace_recommendations() {
    run(|fixture| async move {
        fixture.set_up_folder_workspace("myFolder", &RECOMMENDED_EXTENSIONS, &[]).await;
        let test_object = fixture.create_service();

        fixture.prompted_emitter.event().to_promise(None).await.expect("promptedEmitter");
        let recommendations = keys_of(&test_object.get_all_recommendations_with_reason());
        let mut expected: Vec<String> = VALID_RECOMMENDED_EXTENSIONS.iter().map(|id| (*id).to_owned()).collect();
        expected.push("unknown.extension".to_owned());
        assert_eq!(recommendations.len(), expected.len());
        for id in &expected {
            assert!(recommendations.contains(&id.to_lowercase()));
        }
    });
}

#[test]
fn no_prompt_for_valid_workspace_recommendations_if_they_are_already_installed() {
    run(|fixture| async move {
        fixture.base.management.stub_installed(mock_extension_local(&mock_extension_gallery()));
        test_no_prompt_for_valid_recommendations(&fixture, &VALID_RECOMMENDED_EXTENSIONS).await;
    });
}

#[test]
fn no_prompt_for_valid_workspace_recommendations_with_casing_mismatch_if_they_are_already_installed() {
    run(|fixture| async move {
        fixture.base.management.stub_installed(mock_extension_local(&mock_extension_gallery()));
        let upper: Vec<String> = VALID_RECOMMENDED_EXTENSIONS.iter().map(|id| id.to_uppercase()).collect();
        let upper: Vec<&str> = upper.iter().map(String::as_str).collect();
        test_no_prompt_for_valid_recommendations(&fixture, &upper).await;
    });
}

#[test]
fn no_prompt_for_valid_workspace_recommendations_if_ignore_recommendations_is_set() {
    run(|fixture| async move {
        fixture.set_extensions_configuration(&[("ignoreRecommendations", true)]);
        test_no_prompt_for_valid_recommendations(&fixture, &VALID_RECOMMENDED_EXTENSIONS).await;
    });
}

#[test]
fn no_prompt_for_valid_workspace_recommendations_if_show_recommendations_only_on_demand_is_set() {
    run(|fixture| async move {
        fixture.set_extensions_configuration(&[("showRecommendationsOnlyOnDemand", true)]);
        fixture.set_up_folder_workspace("myFolder", &VALID_RECOMMENDED_EXTENSIONS, &[]).await;
        let test_object = fixture.create_service();
        test_object.activation_promise().await.expect("activationPromise");
        assert!(!fixture.notification.prompted());
    });
}

#[test]
fn no_prompt_for_valid_workspace_recommendations_if_ignore_recommendations_is_set_for_current_workspace() {
    run(|fixture| async move {
        store_workspace_flag(&fixture, WORKSPACE_RECOMMENDATIONS_IGNORE);
        test_no_prompt_for_valid_recommendations(&fixture, &VALID_RECOMMENDED_EXTENSIONS).await;
    });
}

#[test]
fn no_recommendations_of_globally_ignored_recommendations() {
    run(|fixture| async move {
        store_workspace_flag(&fixture, WORKSPACE_RECOMMENDATIONS_IGNORE);
        store_profile_text(&fixture, RECOMMENDATIONS, r#"["ms-dotnettools.csharp", "ms-python.python", "ms-vscode.vscode-typescript-tslint-plugin"]"#);
        store_profile_text(&fixture, IGNORED_RECOMMENDATIONS, r#"["ms-dotnettools.csharp", "mockpublisher2.mockextension2"]"#);

        fixture.set_up_folder_workspace("myFolder", &VALID_RECOMMENDED_EXTENSIONS, &[]).await;
        let test_object = fixture.create_service();
        test_object.activation_promise().await.expect("activationPromise");
        let recommendations = test_object.get_all_recommendations_with_reason();
        assert!(!recommendations.contains_key("ms-dotnettools.csharp")); // stored recommendation that has been globally ignored
        assert!(recommendations.contains_key("ms-python.python")); // stored recommendation
        assert!(recommendations.contains_key("mockpublisher1.mockextension1")); // workspace recommendation
        assert!(!recommendations.contains_key("mockpublisher2.mockextension2")); // workspace recommendation that has been globally ignored
    });
}

#[test]
fn no_recommendations_of_workspace_ignored_recommendations() {
    run(|fixture| async move {
        let ignored_recommendations = ["ms-dotnettools.csharp", "mockpublisher2.mockextension2"]; // ignore a stored recommendation and a workspace recommendation.
        let stored_recommendations = r#"["ms-dotnettools.csharp", "ms-python.python"]"#;
        store_workspace_flag(&fixture, WORKSPACE_RECOMMENDATIONS_IGNORE);
        store_profile_text(&fixture, RECOMMENDATIONS, stored_recommendations);

        fixture.set_up_folder_workspace("myFolder", &VALID_RECOMMENDED_EXTENSIONS, &ignored_recommendations).await;
        let test_object = fixture.create_service();
        test_object.activation_promise().await.expect("activationPromise");
        let recommendations = test_object.get_all_recommendations_with_reason();
        assert!(!recommendations.contains_key("ms-dotnettools.csharp")); // stored recommendation that has been workspace ignored
        assert!(recommendations.contains_key("ms-python.python")); // stored recommendation
        assert!(recommendations.contains_key("mockpublisher1.mockextension1")); // workspace recommendation
        assert!(!recommendations.contains_key("mockpublisher2.mockextension2")); // workspace recommendation that has been workspace ignored
    });
}

#[test]
fn able_to_retrieve_collection_of_all_ignored_recommendations() {
    run(|fixture| async move {
        let workspace_ignored_recommendations = ["ms-dotnettools.csharp"]; // ignore a stored recommendation and a workspace recommendation.
        let stored_recommendations = r#"["ms-dotnettools.csharp", "ms-python.python"]"#;
        let globally_ignored_recommendations = r#"["mockpublisher2.mockextension2"]"#; // ignore a workspace recommendation.
        store_workspace_flag(&fixture, WORKSPACE_RECOMMENDATIONS_IGNORE);
        store_profile_text(&fixture, RECOMMENDATIONS, stored_recommendations);
        store_profile_text(&fixture, IGNORED_RECOMMENDATIONS, globally_ignored_recommendations);

        fixture.set_up_folder_workspace("myFolder", &VALID_RECOMMENDED_EXTENSIONS, &workspace_ignored_recommendations).await;
        let test_object = fixture.create_service();
        test_object.activation_promise().await.expect("activationPromise");

        let recommendations = test_object.get_all_recommendations_with_reason();
        assert_eq!(keys_of(&recommendations), vec!["ms-python.python".to_owned(), "mockpublisher1.mockextension1".to_owned()]);
    });
}

#[test]
fn able_to_dynamically_ignore_unignore_global_recommendations() {
    run(|fixture| async move {
        let stored_recommendations = r#"["ms-dotnettools.csharp", "ms-python.python"]"#;
        let globally_ignored_recommendations = r#"["mockpublisher2.mockextension2"]"#; // ignore a workspace recommendation.
        store_workspace_flag(&fixture, WORKSPACE_RECOMMENDATIONS_IGNORE);
        store_profile_text(&fixture, RECOMMENDATIONS, stored_recommendations);
        store_profile_text(&fixture, IGNORED_RECOMMENDATIONS, globally_ignored_recommendations);

        fixture.set_up_folder_workspace("myFolder", &VALID_RECOMMENDED_EXTENSIONS, &[]).await;
        let extension_ignored_recommendations_service = fixture.ignored_recommendations_service.borrow().clone().expect("IExtensionIgnoredRecommendationsService");
        let test_object = fixture.create_service();
        test_object.activation_promise().await.expect("activationPromise");

        let recommendations = test_object.get_all_recommendations_with_reason();
        assert!(recommendations.contains_key("ms-python.python"));
        assert!(recommendations.contains_key("mockpublisher1.mockextension1"));
        assert!(!recommendations.contains_key("mockpublisher2.mockextension2"));

        extension_ignored_recommendations_service.toggle_global_ignored_recommendation("mockpublisher1.mockextension1", true);

        let recommendations = test_object.get_all_recommendations_with_reason();
        assert!(recommendations.contains_key("ms-python.python"));
        assert!(!recommendations.contains_key("mockpublisher1.mockextension1"));
        assert!(!recommendations.contains_key("mockpublisher2.mockextension2"));

        extension_ignored_recommendations_service.toggle_global_ignored_recommendation("mockpublisher1.mockextension1", false);

        let recommendations = test_object.get_all_recommendations_with_reason();
        assert!(recommendations.contains_key("ms-python.python"));
        assert!(recommendations.contains_key("mockpublisher1.mockextension1"));
        assert!(!recommendations.contains_key("mockpublisher2.mockextension2"));
    });
}

#[test]
fn global_extensions_are_modified_and_recommendation_change_event_is_fired_when_an_extension_is_ignored() {
    run(|fixture| async move {
        let change_handler_target: Rc<RefCell<Vec<IgnoredRecommendationChangeNotification>>> = Rc::default();
        let ignored_extension_id = "Some.Extension";

        store_workspace_flag(&fixture, WORKSPACE_RECOMMENDATIONS_IGNORE);
        store_profile_text(&fixture, IGNORED_RECOMMENDATIONS, r#"["ms-vscode.vscode"]"#);

        fixture.set_up_folder_workspace("myFolder", &[], &[]).await;
        let test_object = fixture.create_service();
        let extension_ignored_recommendations_service = fixture.ignored_recommendations_service.borrow().clone().expect("IExtensionIgnoredRecommendationsService");
        let calls = change_handler_target.clone();
        fixture.base.guard.add(extension_ignored_recommendations_service.on_did_change_global_ignored_recommendation().listen(move |event| calls.borrow_mut().push(event.clone())));
        extension_ignored_recommendations_service.toggle_global_ignored_recommendation(ignored_extension_id, true);
        test_object.activation_promise().await.expect("activationPromise");

        let calls = change_handler_target.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], IgnoredRecommendationChangeNotification { extension_id: ignored_extension_id.to_lowercase(), is_recommended: false });
    });
}

#[test]
fn get_file_based_recommendations_from_storage_old_format() {
    run(|fixture| async move {
        let stored_recommendations = r#"["ms-dotnettools.csharp", "ms-python.python", "ms-vscode.vscode-typescript-tslint-plugin"]"#;
        store_profile_text(&fixture, RECOMMENDATIONS, stored_recommendations);

        fixture.set_up_folder_workspace("myFolder", &[], &[]).await;
        let test_object = fixture.create_service();
        test_object.activation_promise().await.expect("activationPromise");
        let recommendations = test_object.get_file_based_recommendations();
        assert_eq!(recommendations.len(), 2);
        assert!(recommendations.iter().any(|extension_id| extension_id == "ms-dotnettools.csharp")); // stored recommendation that exists in product.extensionTips
        assert!(recommendations.iter().any(|extension_id| extension_id == "ms-python.python")); // stored recommendation that exists in product.extensionImportantTips
        assert!(recommendations.iter().all(|extension_id| extension_id != "ms-vscode.vscode-typescript-tslint-plugin")); // stored recommendation that is no longer in neither product.extensionTips nor product.extensionImportantTips
    });
}

#[test]
fn get_file_based_recommendations_from_storage_new_format() {
    run_real_clock(|fixture| async move {
        let milli_seconds_in_a_day = 1000.0 * 60.0 * 60.0 * 24.0;
        let now = je_base::runtime::date_now() as f64;
        let ten_days_old = 10.0 * milli_seconds_in_a_day;
        let stored_recommendations = format!(
            r#"{{"ms-dotnettools.csharp": {now}, "ms-python.python": {now}, "ms-vscode.vscode-typescript-tslint-plugin": {now}, "lukehoban.Go": {ten_days_old}}}"#
        );
        store_profile_text(&fixture, RECOMMENDATIONS, &stored_recommendations);

        fixture.set_up_folder_workspace("myFolder", &[], &[]).await;
        let test_object: Rc<ExtensionRecommendationsService> = fixture.create_service();
        test_object.activation_promise().await.expect("activationPromise");

        let recommendations = test_object.get_file_based_recommendations();
        assert_eq!(recommendations.len(), 2);
        assert!(recommendations.iter().any(|extension_id| extension_id == "ms-dotnettools.csharp")); // stored recommendation that exists in product.extensionTips
        assert!(recommendations.iter().any(|extension_id| extension_id == "ms-python.python")); // stored recommendation that exists in product.extensionImportantTips
        assert!(recommendations.iter().all(|extension_id| extension_id != "ms-vscode.vscode-typescript-tslint-plugin")); // stored recommendation that is no longer in neither product.extensionTips nor product.extensionImportantTips
        assert!(recommendations.iter().all(|extension_id| extension_id != "lukehoban.Go")); //stored recommendation that is older than a week
    });
}
