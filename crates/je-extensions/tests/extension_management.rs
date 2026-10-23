//! Porte de `src/vs/platform/extensionManagement/test/common/extensionManagement.test.ts` (suítes `Extension Identifier Pattern` e `Extension Blocked Message`), caso a caso e na mesma ordem.
//!
//! Divergências: o serviço de gerenciamento (ver `management_service`) é síncrono e a galeria e o serviço de
//! extensões permitidas são injetados por trait e configuração; o `Object.create(AbstractExtensionManagementService.prototype)`
//! com campos atribuídos vira o serviço montado por `common::setup`.

mod common;

use std::rc::Rc;

use common::{FakeGallery, a_gallery_extension, setup};
use je_extensions::management::{
    EXTENSION_IDENTIFIER_PATTERN, EXTENSION_PUBLISHER_IDENTIFIER_PATTERN, ExtensionBlockingInfo, ExtensionBlockingOrigin, ExtensionManagementErrorCode, InstallOptions,
    get_extension_blocked_message,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, TargetPlatform};
use je_extensions::util::ExtensionKey;
use zyarr::RegExp;

fn regex_test(regex: &RegExp, text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().collect();
    regex.exec(&units, 0).is_some()
}

#[test]
fn extension_identifier_pattern() {
    let reg_ex = RegExp::from_str(EXTENSION_IDENTIFIER_PATTERN, "").expect("padrão válido");
    assert!(regex_test(&reg_ex, "publisher.name"));
    assert!(regex_test(&reg_ex, "publiSher.name"));
    assert!(regex_test(&reg_ex, "publisher.Name"));
    assert!(regex_test(&reg_ex, "PUBLISHER.NAME"));
    assert!(regex_test(&reg_ex, "PUBLISHEr.NAMe"));
    assert!(regex_test(&reg_ex, "PUBLISHEr.N-AMe"));
    assert!(regex_test(&reg_ex, "PUB-LISHEr.NAMe"));
    assert!(regex_test(&reg_ex, "PUB-LISHEr.N-AMe"));
    assert!(regex_test(&reg_ex, "PUBLISH12Er90.N-A54Me123"));
    assert!(regex_test(&reg_ex, "111PUBLISH12Er90.N-1111A54Me123"));
    assert!(!regex_test(&reg_ex, "publishername"));
    assert!(!regex_test(&reg_ex, "-publisher.name"));
    assert!(!regex_test(&reg_ex, "publisher.-name"));
    assert!(!regex_test(&reg_ex, "-publisher.-name"));
    assert!(!regex_test(&reg_ex, "publ_isher.name"));
    assert!(!regex_test(&reg_ex, "publisher._name"));
}

#[test]
fn extension_identifier_pattern_and_publisher_identifier_pattern_are_mutually_exclusive_327194() {
    // Unanchored patterns let a compound key match both, failing array values against the publisher-only schema.
    let extension_id_reg_ex = RegExp::from_str(EXTENSION_IDENTIFIER_PATTERN, "").expect("padrão válido");
    let publisher_reg_ex = RegExp::from_str(EXTENSION_PUBLISHER_IDENTIFIER_PATTERN, "").expect("padrão válido");

    assert!(regex_test(&extension_id_reg_ex, "ms-vscode.cpptools"));
    assert!(!regex_test(&publisher_reg_ex, "ms-vscode.cpptools"));

    assert!(!regex_test(&extension_id_reg_ex, "ms-vscode"));
    assert!(regex_test(&publisher_reg_ex, "ms-vscode"));
}

#[test]
fn extension_key() {
    let id = || ExtensionIdentifierInfo::new("pub.extension-name");
    assert_eq!(ExtensionKey::new(id(), "1.0.1", TargetPlatform::Undefined).to_string(), "pub.extension-name-1.0.1");
    assert_eq!(ExtensionKey::new(id(), "1.0.1", TargetPlatform::Undefined).to_string(), "pub.extension-name-1.0.1");
    assert_eq!(ExtensionKey::new(id(), "1.0.1", TargetPlatform::Win32X64).to_string(), format!("pub.extension-name-1.0.1-{}", TargetPlatform::Win32X64));
}

#[test]
fn extension_key_parsing() {
    assert!(ExtensionKey::parse("pub.extension-name").is_none());
    assert!(ExtensionKey::parse("pub.extension-name@1.2.3").is_none());
    assert_eq!(ExtensionKey::parse("pub.extension-name-1.0.1").map(|k| k.to_string()).as_deref(), Some("pub.extension-name-1.0.1"));
    assert_eq!(ExtensionKey::parse("pub.extension-name-1.0.1-win32-x64").map(|k| k.to_string()).as_deref(), Some("pub.extension-name-1.0.1-win32-x64"));
}

#[test]
fn the_summary_names_the_organization_or_the_marketplace_per_origin() {
    let messages: Vec<String> = [ExtensionBlockingOrigin::Policy, ExtensionBlockingOrigin::Service]
        .into_iter()
        .map(|origin| get_extension_blocked_message(&ExtensionBlockingInfo { origin }))
        .collect();
    assert_eq!(
        messages,
        vec![
            "This extension is not permitted by your organization's policy.",
            "This extension has been blocked by the Extension Marketplace and must not be used."
        ]
    );
}

#[test]
fn can_install_returns_the_blocking_message() {
    let env = setup(Rc::new(FakeGallery::new(true)), None);

    let result = env.management.can_install(&a_gallery_extension(Some(ExtensionBlockingInfo { origin: ExtensionBlockingOrigin::Policy })));

    assert_eq!(result.expect_err("bloqueada"), "This extension is not permitted by your organization's policy.");
}

#[test]
fn install_from_gallery_reports_a_blocked_extension_as_not_allowed() {
    let env = setup(Rc::new(FakeGallery::new(true)), None);

    let error = env
        .management
        .install_from_gallery(&a_gallery_extension(Some(ExtensionBlockingInfo { origin: ExtensionBlockingOrigin::Policy })), &InstallOptions::default())
        .expect_err("bloqueada");

    assert_eq!(error.code, ExtensionManagementErrorCode::NotAllowed);
    assert_eq!(error.message, "This extension is not permitted by your organization's policy.");
}

#[test]
fn install_from_gallery_preserves_the_existing_plain_platform_error_for_other_can_install_failures() {
    // O `isAllowed` do original devolve um markdown com o link da lista; aqui a lista `extensions.allowed` com `*: false` faz o mesmo.
    let env = setup(Rc::new(FakeGallery::new(true)), Some(serde_json::json!({ "*": false })));

    let error = env.management.install_from_gallery(&a_gallery_extension(None), &InstallOptions::default()).expect_err("não permitida");

    assert_eq!(error.code, ExtensionManagementErrorCode::IncompatibleTargetPlatform);
    assert!(!error.message.contains("[allowed list]"));
}
