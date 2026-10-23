//! Testes próprios de `product.rs`, `policy.rs`, `default_account.rs` e `managed_settings.rs`, que o
//! `vs/base/test` do VS Code não tem. Não é porte de teste do VS Code.
//!
//! O teste principal lê o `product.json` do oráculo (`ref/oracle/VSCode-linux-x64/resources/app/product.json`,
//! fora do git) como `IProductConfiguration`, escreve de volta e exige o mesmo JSON, o que prova a leitura sem
//! perda, inclusive das chaves que a interface não declara. Sem o oráculo na máquina o teste avisa e passa.

#![recursion_limit = "512"]

use je_base::managed_settings::{
    ExtraKnownMarketplace, ExtraKnownMarketplaceSource, IExtraKnownMarketplaceEntry, extra_known_marketplaces_to_config_dict,
};
use je_base::policy::{ExtensionConfigurationPolicy, PolicyCategory, PolicyValue, PolicyValueFn};
use je_base::default_account::IPolicyData;
use je_base::product::IProductConfiguration;
use serde_json::{Value, json};

const ORACLE_PRODUCT_JSON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/oracle/VSCode-linux-x64/resources/app/product.json");

/// O caminho da primeira diferença entre dois JSON (`None` se iguais), para a mensagem de falha caber numa linha.
fn first_difference(path: &str, a: &Value, b: &Value) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (key, value) in x {
                match y.get(key) {
                    None => return Some(format!("{path}/{key}: presente só na escrita")),
                    Some(other) => {
                        if let Some(found) = first_difference(&format!("{path}/{key}"), value, other) {
                            return Some(found);
                        }
                    }
                }
            }
            y.keys().find(|k| !x.contains_key(*k)).map(|k| format!("{path}/{k}: presente só no original"))
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x.iter().zip(y).enumerate().find_map(|(i, (p, q))| first_difference(&format!("{path}[{i}]"), p, q)),
        _ if a == b => None,
        _ => Some(format!("{path}: {a} contra {b}")),
    }
}

#[test]
fn reads_the_oracle_product_json_without_loss() {
    let Ok(text) = std::fs::read_to_string(ORACLE_PRODUCT_JSON) else {
        eprintln!("oráculo ausente em ref/oracle, teste pulado");
        return;
    };
    let original: Value = serde_json::from_str(&text).unwrap();
    let product = IProductConfiguration::from_json(&text).expect("o product.json do oráculo tem a forma de IProductConfiguration");

    assert_eq!(product.name_long, original["nameLong"].as_str().unwrap());
    assert_eq!(product.application_name, "code");
    // Chaves que a interface não declara são preservadas.
    for key in ["win32x64AppId", "darwinCredits", "auth", "tunnelServerQualities", "licenseName"] {
        assert!(product.extra.contains_key(key), "{key} devia estar em extra");
    }
    assert!(product.extension_configuration_policy.as_ref().is_some_and(|p| !p.is_empty()));
    assert!(product.configuration_sync_store.is_some());

    let written = product.to_json_value();
    assert_eq!(first_difference("", &written, &original), None);
    // A ordem das chaves dos dicionários (`IStringDictionary`) sobrevive; a dos campos de uma estrutura é a da
    // declaração, com as chaves desconhecidas depois.
    let keys = |v: &Value| v["extensionProperties"].as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&written), keys(&original));
}

#[test]
fn unknown_keys_survive_at_every_level() {
    let input = json!({
        "version": "1.0.0", "nameShort": "S", "nameLong": "L", "applicationName": "a", "urlProtocol": "u",
        "dataFolderName": ".d", "sharedDataFolderName": ".s", "serverApplicationName": "sa",
        "extensionProperties": { "pub.ext": { "hasPrereleaseVersion": true, "future": 1 } },
        "builtInExtensionsEnabledWithAutoUpdates": [],
        "defaultChatAgent": {
            "extensionId": "e", "chatExtensionId": "c", "chatExtensionOutputId": "o", "chatExtensionOutputExtensionStateCommand": "s",
            "documentationUrl": "d", "skusDocumentationUrl": "s", "optimizeUsageDocumentationUrl": "o", "publicCodeMatchesUrl": "p",
            "managePlanUrl": "m", "upgradePlanUrl": "u", "signUpUrl": "s", "termsStatementUrl": "t", "privacyStatementUrl": "p",
            "provider": {
                "default": {"id": "a", "name": "A"}, "enterprise": {"id": "b", "name": "B"}, "google": {"id": "c", "name": "C"},
                "apple": {"id": "d", "name": "D"}, "microsoft": {"id": "e", "name": "E"}
            },
            "providerExtensionId": "p", "providerUriSetting": "p", "providerScopes": [["a", "b"]], "entitlementUrl": "e",
            "entitlementSignupLimitedUrl": "e", "tokenEntitlementUrl": "t", "mcpRegistryDataUrl": "m", "managedSettingsUrl": "m",
            "chatQuotaExceededContext": "c", "completionsQuotaExceededContext": "c", "walkthroughCommand": "w", "completionsMenuCommand": "c",
            "chatRefreshTokenCommand": "c", "generateCommitMessageCommand": "g", "resolveMergeConflictsCommand": "r",
            "completionsAdvancedSetting": "c", "completionsEnablementSetting": "c", "nextEditSuggestionsSetting": "n",
            "brandNewField": {"x": [1, 2]}
        },
        "topLevelFuture": [true, null],
        "trustedExtensionAuthAccess": {"github": ["ms-vscode.x"]},
        "extensionConfigurationPolicy": {
            "a.b": {"policyReference": {"name": "Owner"}},
            "c.d": {
                "name": "P", "category": "Extensions", "minimumVersion": "1.2",
                "localization": {"description": {"key": "k", "value": "v"}}
            }
        }
    });
    let product: IProductConfiguration = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(product.extra["topLevelFuture"], json!([true, null]));
    assert_eq!(product.default_chat_agent.extra["brandNewField"], json!({"x": [1, 2]}));
    let policies = product.extension_configuration_policy.clone().unwrap();
    assert!(matches!(policies["a.b"], ExtensionConfigurationPolicy::Reference(_)));
    assert!(matches!(policies["c.d"], ExtensionConfigurationPolicy::Policy(_)));
    assert_eq!(product.to_json_value(), input);
}

#[test]
fn rejects_a_product_json_missing_required_keys() {
    assert!(IProductConfiguration::from_json(r#"{"nameShort": "x"}"#).is_err());
}

#[test]
fn policy_category_names_are_localized_with_stable_keys() {
    let name = PolicyCategory::Telemetry.name();
    assert_eq!((name.key.as_str(), name.value.as_str()), ("telemetryConfigurationTitle", "Telemetry"));
    assert_eq!(PolicyCategory::InteractiveSession.name().value, "Chat");
}

#[test]
fn policy_value_callback_is_outside_the_json() {
    let policy: ExtensionConfigurationPolicy = serde_json::from_value(json!({
        "name": "P", "category": "Update", "minimumVersion": "1.0",
        "localization": {"description": {"key": "k", "value": "v"}}, "restrictedValue": false
    }))
    .unwrap();
    let ExtensionConfigurationPolicy::Policy(mut policy) = policy else { panic!("devia ser uma política completa") };
    assert_eq!(policy.restricted_value, Some(PolicyValue::Boolean(false)));
    assert_eq!(policy.value.call(&serde_json::from_value::<IPolicyData>(json!({})).unwrap()), None);
    policy.value = PolicyValueFn::new(|data| data.mcp.map(PolicyValue::Boolean));
    let data: IPolicyData = serde_json::from_value(json!({"mcp": true})).unwrap();
    assert_eq!(policy.value.call(&data), Some(PolicyValue::Boolean(true)));
    assert!(serde_json::to_value(&policy).unwrap().get("value").is_none());
}

#[test]
fn extra_known_marketplaces_to_config_dict_matches_the_original_rules() {
    assert_eq!(extra_known_marketplaces_to_config_dict(None), None);
    assert_eq!(extra_known_marketplaces_to_config_dict(Some(&[])), None);
    let entries = vec![
        ExtraKnownMarketplace::Name("plain".into()),
        ExtraKnownMarketplace::Name("__proto__".into()),
        ExtraKnownMarketplace::Entry(IExtraKnownMarketplaceEntry {
            name: "gh".into(),
            auto_update: None,
            source: ExtraKnownMarketplaceSource::Github { repo: "o/r".into(), r#ref: Some("main".into()) },
        }),
        ExtraKnownMarketplace::Entry(IExtraKnownMarketplaceEntry {
            name: "git".into(),
            auto_update: Some(true),
            source: ExtraKnownMarketplaceSource::Git { url: "https://x/y.git".into(), r#ref: None },
        }),
        ExtraKnownMarketplace::Entry(IExtraKnownMarketplaceEntry {
            name: "constructor".into(),
            auto_update: None,
            source: ExtraKnownMarketplaceSource::Git { url: "u".into(), r#ref: None },
        }),
    ];
    let dict = extra_known_marketplaces_to_config_dict(Some(&entries)).unwrap();
    let pairs: Vec<(&str, &str)> = dict.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(pairs, vec![("plain", "plain"), ("gh", "o/r#main"), ("git", r#"{"source":"https://x/y.git","autoUpdate":true}"#)]);
}
