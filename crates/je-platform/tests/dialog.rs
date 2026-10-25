//! Porte de `src/vs/platform/dialogs/test/electron-browser/dialog.test.ts` (suíte `Dialog`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `product` do original é o `product.json` do VS Code; aqui é o [`ProductService`] padrão, e o `copilotVersions` de cada
//!   caso é o que o `{ ...product, copilotVersions }` do original acrescenta.
//! - O `process.versions` do Node, que o diálogo "Sobre" também imprime, é o [`ProcessVersions`] padrão; o teste só lê as linhas
//!   `@github/copilot`.
//! - O `for (const { name, ... } of [...]) test(...)` vira um `#[test]` por elemento da lista, pelo macro `unstable_version_case`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::platform::Platform;
use je_base::product::ICopilotVersions;
use je_platform::dialogs::dialog::{ProcessVersions, create_native_about_dialog_details};
use je_platform::native::OsProperties;
use je_platform::product::ProductService;

fn os_properties() -> OsProperties {
    OsProperties { os_type: "Test OS".to_owned(), release: "1.0".to_owned(), arch: "test-arch".to_owned(), platform: "test".to_owned(), cpus: Vec::new() }
}

/// `{ details: string[]; detailsToCopy: string[] }`.
#[derive(Debug, PartialEq, Eq)]
struct CopilotVersionLines {
    details: Vec<String>,
    details_to_copy: Vec<String>,
}

fn get_copilot_version_lines(runtime: &str, sdk: &str) -> CopilotVersionLines {
    let product_service = ProductService {
        copilot_versions: Some(ICopilotVersions { runtime: runtime.to_owned(), sdk: sdk.to_owned(), extra: Default::default() }),
        ..ProductService::default()
    };
    let details = create_native_about_dialog_details(&product_service, &os_properties(), &ProcessVersions::default(), &Platform::detect());
    let select_copilot_version_lines = |value: &str| value.split('\n').filter(|line| line.starts_with("@github/copilot")).map(str::to_owned).collect::<Vec<_>>();

    CopilotVersionLines { details: select_copilot_version_lines(&details.details), details_to_copy: select_copilot_version_lines(&details.details_to_copy) }
}

/// O esperado de `details` e de `detailsToCopy`, que são iguais em todos os casos.
fn expected_lines(runtime: &str, sdk: &str) -> CopilotVersionLines {
    let lines = || vec![format!("@github/copilot: {runtime}"), format!("@github/copilot-sdk: {sdk}")];

    CopilotVersionLines { details: lines(), details_to_copy: lines() }
}

#[test]
fn formats_copilot_canary_versions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        get_copilot_version_lines("1.0.84-canary.70.gdb75d0d.unsigned", "0.1.23-canary.45.gabcdef.unsigned"),
        expected_lines("1.0.84.70.gdb75d0d", "0.1.23.45.gabcdef")
    );
}

#[test]
fn formats_copilot_unstable_versions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        get_copilot_version_lines("1.0.85-unstable.r35379093703.g3514c9a", "1.0.15-unstable.35393089353.gfc44743"),
        expected_lines("1.0.85.r35379093703.g3514c9a", "1.0.15.35393089353.gfc44743")
    );
}

macro_rules! unstable_version_case {
    ($test:ident, $runtime:expr, $sdk:expr, $expected_runtime:expr, $expected_sdk:expr) => {
        #[test]
        fn $test() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();

            assert_eq!(get_copilot_version_lines($runtime, $sdk), expected_lines($expected_runtime, $expected_sdk));
        }
    };
}

unstable_version_case!(
    formats_copilot_unstable_versions_runtime_only,
    "1.0.87-unstable.r35651157977.g1aacd25",
    "1.0.15",
    "1.0.87.r35651157977.g1aacd25",
    "1.0.15"
);

unstable_version_case!(
    formats_copilot_unstable_versions_sdk_only,
    "1.0.87",
    "1.0.15-unstable.35663726336.gcb2a8cc",
    "1.0.87",
    "1.0.15.35663726336.gcb2a8cc"
);

unstable_version_case!(formats_copilot_unstable_versions_bare_prerelease_markers, "1.0.87-unstable", "1.0.15-unstable", "1.0.87", "1.0.15");

unstable_version_case!(
    formats_copilot_unstable_versions_hyphenated_prerelease_identifiers,
    "1.0.87-unstable-r35651157977.g1aacd25.unsigned",
    "1.0.15-unstable-35663726336.gcb2a8cc.unsigned",
    "1.0.87-r35651157977.g1aacd25",
    "1.0.15-35663726336.gcb2a8cc"
);

unstable_version_case!(
    formats_copilot_unstable_versions_build_metadata,
    "1.0.87-unstable+r35651157977.g1aacd25",
    "1.0.15-unstable+35663726336.gcb2a8cc",
    "1.0.87+r35651157977.g1aacd25",
    "1.0.15+35663726336.gcb2a8cc"
);

unstable_version_case!(
    formats_copilot_unstable_versions_preserves_marker_like_build_metadata,
    "1.0.87+build-unstable.foo",
    "1.0.15+build-canary.foo",
    "1.0.87+build-unstable.foo",
    "1.0.15+build-canary.foo"
);

unstable_version_case!(
    formats_copilot_unstable_versions_removes_only_the_leading_prerelease_marker,
    "1.0.87-unstable+build-unstable.foo",
    "1.0.15-canary+build-canary.foo",
    "1.0.87+build-unstable.foo",
    "1.0.15+build-canary.foo"
);

unstable_version_case!(
    formats_copilot_unstable_versions_preserves_marker_like_text_inside_prerelease_identifiers,
    "1.0.87-preview-unstable.foo",
    "1.0.15-preview-canary.foo",
    "1.0.87-preview-unstable.foo",
    "1.0.15-preview-canary.foo"
);

#[test]
fn preserves_stable_copilot_versions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(get_copilot_version_lines("1.0.84", "0.1.23"), expected_lines("1.0.84", "0.1.23"));
}
