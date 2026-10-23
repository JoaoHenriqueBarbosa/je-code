//! Porte de `src/vs/platform/extensionManagement/test/common/extensionNls.test.ts` (suíte `Localize Manifest`), caso a caso e na mesma ordem.

use je_extensions::nls::{Translations, localize_manifest};
use je_platform::log::NullLogger;
use serde_json::{Value, json};

fn manifest() -> Value {
    json!({
        "name": "test",
        "publisher": "test",
        "version": "1.0.0",
        "engines": { "vscode": "*" },
        "contributes": {
            "commands": [
                { "command": "test.command", "title": "%test.command.title%", "category": "%test.command.category%" },
            ],
            "authentication": [
                { "id": "test.authentication", "label": "%test.authentication.label%" }
            ],
            "configuration": {
                // to ensure we test another "title" property
                "title": "%test.configuration.title%",
                "properties": {
                    "test.configuration": { "type": "string", "description": "not important" }
                }
            }
        }
    })
}

fn translations(value: Value) -> Translations {
    value.as_object().expect("objeto").clone()
}

#[test]
fn replaces_template_strings() {
    let logger = NullLogger::new();
    let mut localized = manifest();
    localize_manifest(
        &logger,
        &mut localized,
        &translations(json!({
            "test.command.title": "Test Command",
            "test.command.category": "Test Category",
            "test.authentication.label": "Test Authentication",
            "test.configuration.title": "Test Configuration",
        })),
        None,
    );

    assert_eq!(localized["contributes"]["commands"][0]["title"], "Test Command");
    assert_eq!(localized["contributes"]["commands"][0]["category"], "Test Category");
    assert_eq!(localized["contributes"]["authentication"][0]["label"], "Test Authentication");
    assert_eq!(localized["contributes"]["configuration"]["title"], "Test Configuration");
}

#[test]
fn replaces_template_strings_with_fallback_if_not_found_in_translations() {
    let logger = NullLogger::new();
    let mut localized = manifest();
    localize_manifest(
        &logger,
        &mut localized,
        &translations(json!({})),
        Some(&translations(json!({
            "test.command.title": "Test Command",
            "test.command.category": "Test Category",
            "test.authentication.label": "Test Authentication",
            "test.configuration.title": "Test Configuration",
        }))),
    );

    assert_eq!(localized["contributes"]["commands"][0]["title"], "Test Command");
    assert_eq!(localized["contributes"]["commands"][0]["category"], "Test Category");
    assert_eq!(localized["contributes"]["authentication"][0]["label"], "Test Authentication");
    assert_eq!(localized["contributes"]["configuration"]["title"], "Test Configuration");
}

#[test]
fn replaces_template_strings_command_title_and_categories_become_i_localized_string() {
    let logger = NullLogger::new();
    let mut localized = manifest();
    localize_manifest(
        &logger,
        &mut localized,
        &translations(json!({
            "test.command.title": "Befehl test",
            "test.command.category": "Testkategorie",
            "test.authentication.label": "Testauthentifizierung",
            "test.configuration.title": "Testkonfiguration",
        })),
        Some(&translations(json!({
            "test.command.title": "Test Command",
            "test.command.category": "Test Category",
            "test.authentication.label": "Test Authentication",
            "test.configuration.title": "Test Configuration",
        }))),
    );

    let title = &localized["contributes"]["commands"][0]["title"];
    let category = &localized["contributes"]["commands"][0]["category"];
    assert_eq!(title["value"], "Befehl test");
    assert_eq!(title["original"], "Test Command");
    assert_eq!(category["value"], "Testkategorie");
    assert_eq!(category["original"], "Test Category");

    // Everything else stays as a string.
    assert_eq!(localized["contributes"]["authentication"][0]["label"], "Testauthentifizierung");
    assert_eq!(localized["contributes"]["configuration"]["title"], "Testkonfiguration");
}

#[test]
fn replaces_template_strings_is_best_effort_164630() {
    let logger = NullLogger::new();
    let mut localized = json!({
        "name": "test",
        "publisher": "test",
        "version": "1.0.0",
        "engines": { "vscode": "*" },
        "contributes": {
            "authentication": [
                {
                    "id": "test.authentication",
                    // This not existing in the bundle shouldn't cause an error.
                    "label": "%doesnotexist%",
                }
            ],
            "commands": [
                { "command": "test.command", "title": "%test.command.title%", "category": "%test.command.category%" },
            ],
        }
    });

    localize_manifest(
        &logger,
        &mut localized,
        &translations(json!({ "test.command.title": "Test Command", "test.command.category": "Test Category" })),
        None,
    );

    assert_eq!(localized["contributes"]["commands"][0]["title"], "Test Command");
    assert_eq!(localized["contributes"]["commands"][0]["category"], "Test Category");
    assert_eq!(localized["contributes"]["authentication"][0]["label"], "%doesnotexist%");
}
