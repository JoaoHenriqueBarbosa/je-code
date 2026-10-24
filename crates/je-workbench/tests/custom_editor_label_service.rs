//! Porte de `src/vs/workbench/services/editor/test/browser/customEditorLabelService.test.ts` (suíte `Custom Editor Label
//! Service`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`), e o `disposables` do original é o store dele. O emissor do `TestConfigurationService` é
//!   entregue ao guarda, como o `network_filter_service` faz.
//! - **`workbenchInstantiationService(undefined, disposables)`.** Só o `IWorkspaceContextService` (o `TestContextService`
//!   com o `TestWorkspace`) é usado do original, e o `createInstance` é o `Injectable::create` com as dependências na
//!   ordem do construtor.
//! - **`updatePattern`** grava o valor e dispara a mudança de configuração da chave de padrões (o evento real, ver
//!   `configuration_event_support`).
//! - **`await`** de `setUserConfiguration` descarta o `Promise` já resolvido.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_instantiation::Injectable;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::resources::uri_file;
use je_workbench::services::editor::custom_editor_label_service::{CustomEditorLabelService, ICustomEditorLabelService};
use je_workbench::test::common::workbench_test_services::TestContextService;

/// `createCustomLabelService()`.
fn create_custom_label_service() -> (LeakGuard, Rc<CustomEditorLabelService>, Rc<TestConfigurationService>) {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();

    let config_service = TestConfigurationService::new(None);
    disposables.add_rc(config_service.on_did_change_configuration_emitter.clone());
    let _ = config_service.set_user_configuration(CustomEditorLabelService::SETTING_ID_ENABLED, Dynamic::Bool(true), None);

    let custom_label_service =
        disposables.add_rc(CustomEditorLabelService::create((), (config_service.clone(), TestContextService::new(None, None))));
    (disposables, custom_label_service, config_service)
}

/// `updatePattern(configService, value)`.
fn update_pattern(config_service: &TestConfigurationService, value: Dynamic) {
    let _ = config_service.set_user_configuration(CustomEditorLabelService::SETTING_ID_PATTERNS, value, None);
    fire_configuration_change(config_service, &[CustomEditorLabelService::SETTING_ID_PATTERNS]);
}

/// `{ [pattern]: template }` na ordem dada.
fn patterns(entries: &[(&str, &str)]) -> Dynamic {
    Dynamic::object(entries.iter().map(|(pattern, template)| (*pattern, Dynamic::string(template))))
}

/// `customLabelService.getName(URI.file(path))`.
fn name_of(service: &CustomEditorLabelService, path: &str) -> Option<String> {
    service.get_name(&uri_file(path))
}

#[test]
fn custom_labels_filename_extname() {
    let (_disposables, custom_label_service, config_service) = create_custom_label_service();

    update_pattern(&config_service, patterns(&[("**", "${filename}.${extname}")]));

    let filenames = ["file.txt", "file.txt1.tx2", ".file.txt"];

    for filename in filenames {
        let label = name_of(&custom_label_service, filename);
        assert_eq!(label.as_deref(), Some(filename));
    }

    let mut label = name_of(&custom_label_service, "file");
    assert_eq!(label.as_deref(), Some("file.${extname}"));

    label = name_of(&custom_label_service, ".file");
    assert_eq!(label.as_deref(), Some(".file.${extname}"));
}

#[test]
fn custom_labels_filename() {
    let (_disposables, custom_label_service, config_service) = create_custom_label_service();

    update_pattern(&config_service, patterns(&[("**", "${filename}")]));

    assert_eq!(name_of(&custom_label_service, "file").as_deref(), Some("file"));
    assert_eq!(name_of(&custom_label_service, "file.txt").as_deref(), Some("file"));
    assert_eq!(name_of(&custom_label_service, "file.txt1.txt2").as_deref(), Some("file"));
    assert_eq!(name_of(&custom_label_service, "folder/file.txt1.txt2").as_deref(), Some("file"));

    assert_eq!(name_of(&custom_label_service, ".file").as_deref(), Some(".file"));
    assert_eq!(name_of(&custom_label_service, ".file.txt").as_deref(), Some(".file"));
    assert_eq!(name_of(&custom_label_service, ".file.txt1.txt2").as_deref(), Some(".file"));
    assert_eq!(name_of(&custom_label_service, "folder/.file.txt1.txt2").as_deref(), Some(".file"));
}

/// `IExt`.
#[derive(Default)]
struct Ext {
    extname: Option<&'static str>,
    ext0: Option<&'static str>,
    ext1: Option<&'static str>,
    ext2: Option<&'static str>,
    ext_minus1: Option<&'static str>,
    ext_minus2: Option<&'static str>,
}

#[test]
fn custom_labels_extname_n() {
    let (_disposables, custom_label_service, config_service) = create_custom_label_service();

    update_pattern(
        &config_service,
        patterns(&[
            ("**/ext/**", "${extname}"),
            ("**/ext0/**", "${extname(0)}"),
            ("**/ext1/**", "${extname(1)}"),
            ("**/ext2/**", "${extname(2)}"),
            ("**/extMinus1/**", "${extname(-1)}"),
            ("**/extMinus2/**", "${extname(-2)}"),
        ]),
    );

    let assert_extname = |filename: &str, ext: Ext| {
        let check = |folder: &str, expected: Option<&str>, default: &str| {
            assert_eq!(name_of(&custom_label_service, &format!("test/{folder}/{filename}")).as_deref(), Some(expected.unwrap_or(default)), "{filename}");
        };
        check("ext", ext.extname, "${extname}");
        check("ext0", ext.ext0, "${extname(0)}");
        check("ext1", ext.ext1, "${extname(1)}");
        check("ext2", ext.ext2, "${extname(2)}");
        check("extMinus1", ext.ext_minus1, "${extname(-1)}");
        check("extMinus2", ext.ext_minus2, "${extname(-2)}");
    };

    assert_extname("file.txt", Ext { extname: Some("txt"), ext0: Some("txt"), ext_minus1: Some("txt"), ..Ext::default() });

    assert_extname(
        "file.txt1.txt2",
        Ext { extname: Some("txt1.txt2"), ext0: Some("txt2"), ext1: Some("txt1"), ext_minus1: Some("txt1"), ext_minus2: Some("txt2"), ..Ext::default() },
    );

    assert_extname(
        ".file.txt1.txt2",
        Ext { extname: Some("txt1.txt2"), ext0: Some("txt2"), ext1: Some("txt1"), ext_minus1: Some("txt1"), ext_minus2: Some("txt2"), ..Ext::default() },
    );

    assert_extname(
        ".file.txt1.txt2.txt3.txt4",
        Ext {
            extname: Some("txt1.txt2.txt3.txt4"),
            ext0: Some("txt4"),
            ext1: Some("txt3"),
            ext2: Some("txt2"),
            ext_minus1: Some("txt1"),
            ext_minus2: Some("txt2"),
            ..Ext::default()
        },
    );

    assert_extname("file", Ext::default());
    assert_extname(".file", Ext::default());
}

/// `IDir`.
#[derive(Default)]
struct Dir {
    dirname: Option<&'static str>,
    dir0: Option<&'static str>,
    dir1: Option<&'static str>,
    dir2: Option<&'static str>,
    dir_minus1: Option<&'static str>,
    dir_minus2: Option<&'static str>,
}

#[test]
fn custom_labels_dirname_n() {
    let (_disposables, custom_label_service, config_service) = create_custom_label_service();

    update_pattern(&config_service, patterns(&[("**", "${dirname},${dirname(0)},${dirname(1)},${dirname(2)},${dirname(-1)},${dirname(-2)}")]));

    let assert_dirname = |path: &str, dir: Dir| {
        let label = name_of(&custom_label_service, path);
        let part = |index: usize| label.as_deref().and_then(|label| label.split(',').nth(index));
        assert_eq!(part(0), Some(dir.dirname.unwrap_or("${dirname}")), "{path}");
        assert_eq!(part(1), Some(dir.dir0.unwrap_or("${dirname(0)}")), "{path}");
        assert_eq!(part(2), Some(dir.dir1.unwrap_or("${dirname(1)}")), "{path}");
        assert_eq!(part(3), Some(dir.dir2.unwrap_or("${dirname(2)}")), "{path}");
        assert_eq!(part(4), Some(dir.dir_minus1.unwrap_or("${dirname(-1)}")), "{path}");
        assert_eq!(part(5), Some(dir.dir_minus2.unwrap_or("${dirname(-2)}")), "{path}");
    };

    assert_dirname("folder/file.txt", Dir { dirname: Some("folder"), dir0: Some("folder"), dir_minus1: Some("folder"), ..Dir::default() });

    assert_dirname(
        "root/folder/file.txt",
        Dir { dirname: Some("folder"), dir0: Some("folder"), dir1: Some("root"), dir_minus1: Some("root"), dir_minus2: Some("folder"), ..Dir::default() },
    );

    assert_dirname(
        "root/.folder/file.txt",
        Dir { dirname: Some(".folder"), dir0: Some(".folder"), dir1: Some("root"), dir_minus1: Some("root"), dir_minus2: Some(".folder"), ..Dir::default() },
    );

    assert_dirname(
        "root/parent/folder/file.txt",
        Dir {
            dirname: Some("folder"),
            dir0: Some("folder"),
            dir1: Some("parent"),
            dir2: Some("root"),
            dir_minus1: Some("root"),
            dir_minus2: Some("parent"),
        },
    );

    assert_dirname("file.txt", Dir::default());
}

#[test]
fn custom_labels_no_pattern_match() {
    let (_disposables, custom_label_service, config_service) = create_custom_label_service();

    update_pattern(&config_service, patterns(&[("**/folder/**", "folder"), ("file", "file")]));

    assert_eq!(name_of(&custom_label_service, "file"), None);
    assert_eq!(name_of(&custom_label_service, "file.txt"), None);
    assert_eq!(name_of(&custom_label_service, "file.txt1.txt2"), None);
    assert_eq!(name_of(&custom_label_service, "folder1/file.txt1.txt2"), None);

    assert_eq!(name_of(&custom_label_service, ".file"), None);
    assert_eq!(name_of(&custom_label_service, ".file.txt"), None);
    assert_eq!(name_of(&custom_label_service, ".file.txt1.txt2"), None);
    assert_eq!(name_of(&custom_label_service, "folder1/file.txt1.txt2"), None);
}
