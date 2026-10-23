//! Porte de `src/vs/editor/test/common/services/languagesRegistry.test.ts` (suíte `LanguagesRegistry`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `new LanguagesRegistry(false)` é `LanguagesRegistry::new(false, false)` (sem o `ModesRegistry` global e
//!   sem aviso de sobrescrita) e `_registerLanguages` é `register_languages`.
//! - Os campos opcionais do `ILanguageExtensionPoint` (`extensions`, `aliases`, `mimetypes`, ...) são
//!   `Option<Vec<String>>`: `[]` é `Some(vec![])` e a ausência do campo é `None`, como no original.
//! - `getLanguageName` devolve `Option<String>` no lugar do `string | null`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_base::path::Platform;
use je_base::uri::URI;
use je_editor::languages_registry::{LanguageNameIdPair, LanguagesRegistry};
use je_editor::modes_registry::LanguageExtensionPoint;

mod languages_registry {
    use super::*;

    fn strings(values: &[&str]) -> Option<Vec<String>> {
        Some(values.iter().map(|v| (*v).to_owned()).collect())
    }

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    fn pair(language_name: &str, language_id: &str) -> LanguageNameIdPair {
        LanguageNameIdPair { language_name: language_name.to_owned(), language_id: language_id.to_owned() }
    }

    fn file(path: &str) -> URI {
        URI::file(path, Platform::Posix)
    }

    /// `new LanguagesRegistry(false)`.
    fn new_registry() -> std::rc::Rc<LanguagesRegistry> {
        LanguagesRegistry::new(false, false)
    }

    #[test]
    fn output_language_does_not_have_a_name() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "outputLangId".to_owned(),
            extensions: strings(&[]),
            aliases: strings(&[]),
            mimetypes: strings(&["outputLanguageMimeType"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_sorted_registered_language_names(), Vec::<LanguageNameIdPair>::new());

        registry.dispose();
    }

    #[test]
    fn language_with_alias_does_have_a_name() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "langId".to_owned(),
            extensions: strings(&[]),
            aliases: strings(&["LangName"]),
            mimetypes: strings(&["bla"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("LangName", "langId")]);
        assert_eq!(registry.get_language_name("langId"), Some("LangName".to_owned()));

        registry.dispose();
    }

    #[test]
    fn language_without_alias_gets_a_name() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "langId".to_owned(),
            extensions: strings(&[]),
            mimetypes: strings(&["bla"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("langId", "langId")]);
        assert_eq!(registry.get_language_name("langId"), Some("langId".to_owned()));

        registry.dispose();
    }

    #[test]
    fn bug_4360_f_sharp_not_shown_in_status_bar() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "langId".to_owned(),
            extensions: strings(&[".ext1"]),
            aliases: strings(&["LangName"]),
            mimetypes: strings(&["bla"]),
            ..LanguageExtensionPoint::default()
        }]);

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "langId".to_owned(),
            extensions: strings(&[".ext2"]),
            aliases: strings(&[]),
            mimetypes: strings(&["bla"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("LangName", "langId")]);
        assert_eq!(registry.get_language_name("langId"), Some("LangName".to_owned()));

        registry.dispose();
    }

    #[test]
    fn issue_5278_extension_cannot_override_language_name_anymore() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "langId".to_owned(),
            extensions: strings(&[".ext1"]),
            aliases: strings(&["LangName"]),
            mimetypes: strings(&["bla"]),
            ..LanguageExtensionPoint::default()
        }]);

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "langId".to_owned(),
            extensions: strings(&[".ext2"]),
            aliases: strings(&["BetterLanguageName"]),
            mimetypes: strings(&["bla"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("BetterLanguageName", "langId")]);
        assert_eq!(registry.get_language_name("langId"), Some("BetterLanguageName".to_owned()));

        registry.dispose();
    }

    #[test]
    fn mimetypes_are_generated_if_necessary() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint::new("langId")]);

        assert_eq!(registry.get_mime_type("langId"), Some("text/x-langId".to_owned()));

        registry.dispose();
    }

    #[test]
    fn first_mimetype_wins() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "langId".to_owned(),
            mimetypes: strings(&["text/langId", "text/langId2"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_mime_type("langId"), Some("text/langId".to_owned()));

        registry.dispose();
    }

    #[test]
    fn first_mimetype_wins_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint::new("langId")]);

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "langId".to_owned(),
            mimetypes: strings(&["text/langId"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_mime_type("langId"), Some("text/x-langId".to_owned()));

        registry.dispose();
    }

    #[test]
    fn aliases() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint::new("a")]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("a", "a")]);
        assert_eq!(registry.get_language_id_by_language_name("a"), Some("a".to_owned()));
        assert_eq!(registry.get_language_name("a"), Some("a".to_owned()));

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            aliases: strings(&["A1", "A2"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("A1", "a")]);
        assert_eq!(registry.get_language_id_by_language_name("a"), Some("a".to_owned()));
        assert_eq!(registry.get_language_id_by_language_name("a1"), Some("a".to_owned()));
        assert_eq!(registry.get_language_id_by_language_name("a2"), Some("a".to_owned()));
        assert_eq!(registry.get_language_name("a"), Some("A1".to_owned()));

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            aliases: strings(&["A3", "A4"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("A3", "a")]);
        assert_eq!(registry.get_language_id_by_language_name("a"), Some("a".to_owned()));
        assert_eq!(registry.get_language_id_by_language_name("a1"), Some("a".to_owned()));
        assert_eq!(registry.get_language_id_by_language_name("a2"), Some("a".to_owned()));
        assert_eq!(registry.get_language_id_by_language_name("a3"), Some("a".to_owned()));
        assert_eq!(registry.get_language_id_by_language_name("a4"), Some("a".to_owned()));
        assert_eq!(registry.get_language_name("a"), Some("A3".to_owned()));

        registry.dispose();
    }

    #[test]
    fn empty_aliases_array_means_no_alias() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint::new("a")]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("a", "a")]);
        assert_eq!(registry.get_language_id_by_language_name("a"), Some("a".to_owned()));
        assert_eq!(registry.get_language_name("a"), Some("a".to_owned()));

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "b".to_owned(),
            aliases: strings(&[]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_sorted_registered_language_names(), vec![pair("a", "a")]);
        assert_eq!(registry.get_language_id_by_language_name("a"), Some("a".to_owned()));
        assert_eq!(registry.get_language_id_by_language_name("b"), Some("b".to_owned()));
        assert_eq!(registry.get_language_name("a"), Some("a".to_owned()));
        assert_eq!(registry.get_language_name("b"), None);

        registry.dispose();
    }

    #[test]
    fn extensions() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            aliases: strings(&["aName"]),
            extensions: strings(&["aExt"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_extensions("a"), owned(&["aExt"]));

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            extensions: strings(&["aExt2"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_extensions("a"), owned(&["aExt", "aExt2"]));

        registry.dispose();
    }

    #[test]
    fn extensions_of_primary_language_registration_come_first() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            extensions: strings(&["aExt3"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_extensions("a")[0], "aExt3");

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            configuration: Some(file("conf.json")),
            extensions: strings(&["aExt"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_extensions("a")[0], "aExt");

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            extensions: strings(&["aExt2"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_extensions("a")[0], "aExt");

        registry.dispose();
    }

    #[test]
    fn filenames() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            aliases: strings(&["aName"]),
            filenames: strings(&["aFilename"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_filenames("a"), owned(&["aFilename"]));

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            filenames: strings(&["aFilename2"]),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_filenames("a"), owned(&["aFilename", "aFilename2"]));

        registry.dispose();
    }

    #[test]
    fn configuration() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let registry = new_registry();

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            aliases: strings(&["aName"]),
            configuration: Some(file("/path/to/aFilename")),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_configuration_files("a"), vec![file("/path/to/aFilename")]);
        assert_eq!(registry.get_configuration_files("aname"), Vec::<URI>::new());
        assert_eq!(registry.get_configuration_files("aName"), Vec::<URI>::new());

        registry.register_languages(vec![LanguageExtensionPoint {
            id: "a".to_owned(),
            configuration: Some(file("/path/to/aFilename2")),
            ..LanguageExtensionPoint::default()
        }]);

        assert_eq!(registry.get_configuration_files("a"), vec![file("/path/to/aFilename"), file("/path/to/aFilename2")]);
        assert_eq!(registry.get_configuration_files("aname"), Vec::<URI>::new());
        assert_eq!(registry.get_configuration_files("aName"), Vec::<URI>::new());

        registry.dispose();
    }
}
