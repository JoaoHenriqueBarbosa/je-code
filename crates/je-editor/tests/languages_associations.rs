//! Porte de `src/vs/editor/test/common/services/languagesAssociations.test.ts` (suíte `LanguagesAssociations`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - As associações são estado global por thread (`thread_local!`), e cada `#[test]` roda no próprio thread,
//!   então os casos não herdam o registro uns dos outros (no original o módulo é compartilhado entre os
//!   casos; nenhum deles depende do que o anterior registrou).
//! - `URI.file(path)` vira `URI::file(path, Platform::Posix)` e `getMimeTypes(uri, firstLine)` vira
//!   `get_mime_types(Some(&uri), first_line)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::path::Platform;
use je_base::uri::URI;
use je_editor::language_associations::{
    LanguageAssociation, get_mime_types, register_configured_language_association, register_platform_language_association,
};
use support::re;

mod languages_associations {
    use super::*;

    fn file(path: &str) -> URI {
        URI::file(path, Platform::Posix)
    }

    /// `getMimeTypes(resource, firstLine)`.
    fn guess(resource: &URI, first_line: Option<&str>) -> Vec<String> {
        let first_line = first_line.map(JsString::from);
        get_mime_types(Some(resource), first_line.as_ref())
    }

    /// `registerPlatformLanguageAssociation(association)`.
    fn register_platform(association: LanguageAssociation) {
        register_platform_language_association(association, false);
    }

    fn assoc(id: &str, mime: &str) -> LanguageAssociation {
        LanguageAssociation { id: id.to_owned(), mime: mime.to_owned(), ..LanguageAssociation::default() }
    }

    fn with_extension(id: &str, extension: &str, mime: &str) -> LanguageAssociation {
        LanguageAssociation { extension: Some(extension.to_owned()), ..assoc(id, mime) }
    }

    fn with_filename(id: &str, filename: &str, mime: &str) -> LanguageAssociation {
        LanguageAssociation { filename: Some(filename.to_owned()), ..assoc(id, mime) }
    }

    fn with_filepattern(id: &str, filepattern: &str, mime: &str) -> LanguageAssociation {
        LanguageAssociation { filepattern: Some(filepattern.to_owned()), ..assoc(id, mime) }
    }

    fn with_firstline(id: &str, mime: &str, pattern: &str) -> LanguageAssociation {
        LanguageAssociation { firstline: Some(re(pattern, "")), ..assoc(id, mime) }
    }

    fn mimes(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn dynamically_register_text_mime() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut guess_ = guess(&file("foo.monaco"), None);
        assert_eq!(guess_, mimes(&["application/unknown"]));

        register_platform(with_extension("monaco", ".monaco", "text/monaco"));
        guess_ = guess(&file("foo.monaco"), None);
        assert_eq!(guess_, mimes(&["text/monaco", "text/plain"]));

        guess_ = guess(&file(".monaco"), None);
        assert_eq!(guess_, mimes(&["text/monaco", "text/plain"]));

        register_platform(with_filename("codefile", "Codefile", "text/code"));
        guess_ = guess(&file("Codefile"), None);
        assert_eq!(guess_, mimes(&["text/code", "text/plain"]));

        guess_ = guess(&file("foo.Codefile"), None);
        assert_eq!(guess_, mimes(&["application/unknown"]));

        register_platform(with_filepattern("docker", "Docker*", "text/docker"));
        guess_ = guess(&file("Docker-debug"), None);
        assert_eq!(guess_, mimes(&["text/docker", "text/plain"]));

        guess_ = guess(&file("docker-PROD"), None);
        assert_eq!(guess_, mimes(&["text/docker", "text/plain"]));

        register_platform(with_firstline("niceregex", "text/nice-regex", "RegexesAreNice"));
        guess_ = guess(&file("Randomfile.noregistration"), Some("RegexesAreNice"));
        assert_eq!(guess_, mimes(&["text/nice-regex", "text/plain"]));

        guess_ = guess(&file("Randomfile.noregistration"), Some("RegexesAreNotNice"));
        assert_eq!(guess_, mimes(&["application/unknown"]));

        guess_ = guess(&file("Codefile"), Some("RegexesAreNice"));
        assert_eq!(guess_, mimes(&["text/code", "text/plain"]));
    }

    #[test]
    fn mimes_priority() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_platform(with_extension("monaco", ".monaco", "text/monaco"));
        register_platform(with_firstline("foobar", "text/foobar", "foobar"));

        let mut guess_ = guess(&file("foo.monaco"), None);
        assert_eq!(guess_, mimes(&["text/monaco", "text/plain"]));

        guess_ = guess(&file("foo.monaco"), Some("foobar"));
        assert_eq!(guess_, mimes(&["text/monaco", "text/plain"]));

        register_platform(with_filename("docker", "dockerfile", "text/winner"));
        register_platform(with_filepattern("docker", "dockerfile*", "text/looser"));
        guess_ = guess(&file("dockerfile"), None);
        assert_eq!(guess_, mimes(&["text/winner", "text/plain"]));

        register_platform(with_firstline("azure-looser", "text/azure-looser", "azure"));
        register_platform(with_firstline("azure-winner", "text/azure-winner", "azure"));
        guess_ = guess(&file("azure"), Some("azure"));
        assert_eq!(guess_, mimes(&["text/azure-winner", "text/plain"]));
    }

    #[test]
    fn specificity_priority_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_platform(with_extension("monaco2", ".monaco2", "text/monaco2"));
        register_platform(with_filename("monaco2", "specific.monaco2", "text/specific-monaco2"));

        assert_eq!(guess(&file("specific.monaco2"), None), mimes(&["text/specific-monaco2", "text/plain"]));
        assert_eq!(guess(&file("foo.monaco2"), None), mimes(&["text/monaco2", "text/plain"]));
    }

    #[test]
    fn specificity_priority_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_platform(with_filename("monaco3", "specific.monaco3", "text/specific-monaco3"));
        register_platform(with_extension("monaco3", ".monaco3", "text/monaco3"));

        assert_eq!(guess(&file("specific.monaco3"), None), mimes(&["text/specific-monaco3", "text/plain"]));
        assert_eq!(guess(&file("foo.monaco3"), None), mimes(&["text/monaco3", "text/plain"]));
    }

    #[test]
    fn mimes_priority_longest_extension_wins() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_platform(with_extension("monaco", ".monaco", "text/monaco"));
        register_platform(with_extension("monaco", ".monaco.xml", "text/monaco-xml"));
        register_platform(with_extension("monaco", ".monaco.xml.build", "text/monaco-xml-build"));

        let mut guess_ = guess(&file("foo.monaco"), None);
        assert_eq!(guess_, mimes(&["text/monaco", "text/plain"]));

        guess_ = guess(&file("foo.monaco.xml"), None);
        assert_eq!(guess_, mimes(&["text/monaco-xml", "text/plain"]));

        guess_ = guess(&file("foo.monaco.xml.build"), None);
        assert_eq!(guess_, mimes(&["text/monaco-xml-build", "text/plain"]));
    }

    #[test]
    fn mimes_priority_user_configured_wins() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_configured_language_association(with_extension("monaco", ".monaco.xnl", "text/monaco"));
        register_platform(with_extension("monaco", ".monaco.xml", "text/monaco-xml"));

        let guess_ = guess(&file("foo.monaco.xnl"), None);
        assert_eq!(guess_, mimes(&["text/monaco", "text/plain"]));
    }

    #[test]
    fn mimes_priority_pattern_matches_on_path_if_specified() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_platform(with_filepattern("monaco", "**/dot.monaco.xml", "text/monaco"));
        register_platform(with_filepattern("other", "*ot.other.xml", "text/other"));

        let guess_ = guess(&file("/some/path/dot.monaco.xml"), None);
        assert_eq!(guess_, mimes(&["text/monaco", "text/plain"]));
    }

    #[test]
    fn mimes_priority_last_registered_mime_wins() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_platform(with_filepattern("monaco", "**/dot.monaco.xml", "text/monaco"));
        register_platform(with_filepattern("other", "**/dot.monaco.xml", "text/other"));

        let guess_ = guess(&file("/some/path/dot.monaco.xml"), None);
        assert_eq!(guess_, mimes(&["text/other", "text/plain"]));
    }

    #[test]
    fn data_uris() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_platform(with_extension("data", ".data", "text/data"));

        let uri = URI::parse("data:;label:something.data;description:data,", false);
        assert_eq!(guess(&uri, None), mimes(&["text/data", "text/plain"]));
    }

    #[test]
    fn shebang_detection_for_typescript_runtimes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_platform(with_firstline("typescript", "text/typescript", r"^#!.*\b(deno|bun|ts-node)\b"));
        let script = file("script");
        let typescript = mimes(&["text/typescript", "text/plain"]);
        let unknown = mimes(&["application/unknown"]);

        // Deno shebangs
        assert_eq!(guess(&script, Some("#!/usr/bin/env deno")), typescript);
        assert_eq!(guess(&script, Some("#!/usr/bin/env -S deno -A")), typescript);
        assert_eq!(guess(&script, Some("#!/usr/bin/deno")), typescript);

        // Bun shebangs
        assert_eq!(guess(&script, Some("#!/usr/bin/env bun")), typescript);
        assert_eq!(guess(&script, Some("#!/usr/bin/env -S bun run")), typescript);

        // ts-node shebangs
        assert_eq!(guess(&script, Some("#!/usr/bin/env ts-node")), typescript);
        assert_eq!(guess(&script, Some("#!/usr/bin/env -S ts-node --esm")), typescript);

        // Should NOT match other shebangs
        assert_eq!(guess(&script, Some("#!/usr/bin/env node")), unknown);
        assert_eq!(guess(&script, Some("#!/usr/bin/env python")), unknown);
        assert_eq!(guess(&script, Some("#!/bin/bash")), unknown);
    }
}
