//! Porte de `src/vs/platform/extensions/test/common/extensions.test.ts` (suítes `Parsing Enabled Api Proposals` e `Manifest Cache File Name`), caso a caso e na mesma ordem.

use je_extensions::manifest::{ExtensionType, get_manifest_cache_file_name, is_manifest_cache_file_name, parse_enabled_api_proposal_names};

#[test]
fn parsing_enabled_api_proposals() {
    let expected = vec!["activeComment", "commentsDraftState"];
    assert_eq!(expected, parse_enabled_api_proposal_names(&["activeComment", "commentsDraftState"]));
    assert_eq!(expected, parse_enabled_api_proposal_names(&["activeComment", "commentsDraftState@1"]));
    assert_eq!(expected, parse_enabled_api_proposal_names(&["activeComment", "commentsDraftState@"]));
    assert_eq!(expected, parse_enabled_api_proposal_names(&["activeComment", "commentsDraftState@randomstring"]));
    assert_eq!(expected, parse_enabled_api_proposal_names(&["activeComment", "commentsDraftState@1234"]));
    assert_eq!(expected, parse_enabled_api_proposal_names(&["activeComment", "commentsDraftState@1234_random"]));
}

#[test]
fn name_is_scoped_to_extension_type_and_language() {
    assert_eq!(
        vec![
            get_manifest_cache_file_name(ExtensionType::System, None),
            get_manifest_cache_file_name(ExtensionType::System, Some("en")),
            get_manifest_cache_file_name(ExtensionType::System, Some("zh-cn")),
            get_manifest_cache_file_name(ExtensionType::User, None),
            get_manifest_cache_file_name(ExtensionType::User, Some("en")),
        ],
        vec!["extensions.builtin.cache", "extensions.builtin.en.cache", "extensions.builtin.zh-cn.cache", "extensions.user.cache", "extensions.user.en.cache"]
    );
}

#[test]
fn a_language_is_matched_independently_of_its_casing() {
    assert_eq!(
        vec![get_manifest_cache_file_name(ExtensionType::System, Some("zh-CN")), get_manifest_cache_file_name(ExtensionType::System, Some("ZH-CN"))],
        vec!["extensions.builtin.zh-cn.cache", "extensions.builtin.zh-cn.cache"]
    );
}

#[test]
fn a_language_that_is_not_a_well_formed_locale_is_not_put_in_the_name() {
    let thousand_x = "x".repeat(1000);
    let languages = ["../evil", "a/b\\c", "zh_CN", thousand_x.as_str(), "", "-en"];
    let names: Vec<String> = languages.iter().map(|language| get_manifest_cache_file_name(ExtensionType::System, Some(language))).collect();
    assert_eq!(names, vec!["extensions.builtin.cache"; 6]);
}

#[test]
fn generated_names_are_recognized_and_only_for_their_own_extension_type() {
    let names: Vec<String> = [None, Some("en"), Some("zh-cn")].iter().map(|language| get_manifest_cache_file_name(ExtensionType::System, *language)).collect();

    assert_eq!(
        (
            names.iter().all(|name| is_manifest_cache_file_name(name, ExtensionType::System, false)),
            names.iter().any(|name| is_manifest_cache_file_name(name, ExtensionType::User, false)),
        ),
        (true, false)
    );
}

#[test]
fn a_differently_cased_name_is_only_recognized_when_path_casing_is_ignored() {
    let name = "Extensions.User.EN.Cache";

    assert_eq!(
        (is_manifest_cache_file_name(name, ExtensionType::User, true), is_manifest_cache_file_name(name, ExtensionType::User, false)),
        (true, false)
    );
}

#[test]
fn unrelated_file_names_are_not_recognized() {
    let names = ["extensions.json", "extensions.builtin.cache", "extensions.user.en.cache.bak", "my.extensions.user.en.cache"];
    let results: Vec<bool> = names.iter().map(|name| is_manifest_cache_file_name(name, ExtensionType::User, false)).collect();
    assert_eq!(results, vec![false, false, false, false]);
}
