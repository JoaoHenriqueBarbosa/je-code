//! Porte de `src/vs/platform/environment/test/electron-main/environmentMainService.test.ts` (suíte
//! `EnvironmentMainService`, 2 casos, na ordem do original).
//!
//! Divergências: o `process.env` do original é o ambiente do próprio serviço (`set_process_env_var` e
//! `process_env_var`), porque escrever no ambiente do processo é `unsafe` e o je-code o proíbe; assim os casos não
//! vazam variáveis um para o outro. `isLinux` é `cfg!(target_os = "linux")`. `process.env[x] === undefined` é
//! `process_env_var(x) == None`. O `new EnvironmentMainService({ '_': [] }, product)` é o serviço nativo sobre
//! argumentos vazios e o `ProductService` padrão.

use std::rc::Rc;

use je_base::path::{FixedEnvironment, Platform};
use je_platform::environment::environment_main_service::EnvironmentMainService;
use je_platform::environment::environment_service::{AbstractNativeEnvironmentService, NativeEnvironmentPaths};
use je_platform::environment::NativeParsedArgs;
use je_platform::product::ProductService;

fn service() -> EnvironmentMainService {
    let paths = NativeEnvironmentPaths { user_data_dir: "/user-data".to_owned(), home_dir: "/home".to_owned(), tmp_dir: "/tmp".to_owned(), app_root: "/app".to_owned() };
    let base = AbstractNativeEnvironmentService::new(NativeParsedArgs::default(), paths, ProductService::default(), Rc::new(FixedEnvironment::new(Platform::Posix, "/")));
    EnvironmentMainService::new(base, Vec::new())
}

#[track_caller]
fn assert_env(service: &EnvironmentMainService, expected: &[(&str, Option<&str>)]) {
    for (name, value) in expected {
        assert_eq!(service.process_env_var(name).as_deref(), *value, "variável {name}");
    }
}

/// O estado que o original confere depois do `unsetSnapExportedVariables`.
#[track_caller]
fn assert_unset(service: &EnvironmentMainService) {
    if cfg!(target_os = "linux") {
        assert_env(
            service,
            &[
                ("TEST_ARG1", Some("original")),
                ("TEST_ARG2", None),
                ("TEST_ARG1_VSCODE_SNAP_ORIG", Some("original")),
                ("TEST_ARG2_SNAP", Some("test_arg2")),
                ("TEST_ARG3_VSCODE_SNAP_ORIG", Some("")),
                ("TEST_ARG3", None),
            ],
        );
    } else {
        assert_env(
            service,
            &[
                ("TEST_ARG1", Some("modified")),
                ("TEST_ARG2", None),
                ("TEST_ARG1_VSCODE_SNAP_ORIG", Some("original")),
                ("TEST_ARG2_SNAP", Some("test_arg2")),
                ("TEST_ARG3_VSCODE_SNAP_ORIG", Some("")),
                ("TEST_ARG3", Some("test_arg3_non_empty")),
            ],
        );
    }
}

/// O estado que o original confere depois do `restoreSnapExportedVariables` (igual em todas as plataformas).
#[track_caller]
fn assert_restored(service: &EnvironmentMainService) {
    assert_env(
        service,
        &[
            ("TEST_ARG1", Some("modified")),
            ("TEST_ARG1_VSCODE_SNAP_ORIG", Some("original")),
            ("TEST_ARG2_SNAP", Some("test_arg2")),
            ("TEST_ARG2", None),
            ("TEST_ARG3_VSCODE_SNAP_ORIG", Some("")),
            ("TEST_ARG3", Some("test_arg3_non_empty")),
        ],
    );
}

fn set_snap_variables(service: &EnvironmentMainService) {
    service.set_process_env_var("TEST_ARG1_VSCODE_SNAP_ORIG", "original");
    service.set_process_env_var("TEST_ARG1", "modified");
    service.set_process_env_var("TEST_ARG2_SNAP", "test_arg2");
    service.set_process_env_var("TEST_ARG3_VSCODE_SNAP_ORIG", "");
    service.set_process_env_var("TEST_ARG3", "test_arg3_non_empty");
}

#[test]
fn can_unset_and_restore_snap_env_variables() {
    let service = service();

    set_snap_variables(&service);

    // Unset snap env variables
    service.unset_snap_exported_variables();
    assert_unset(&service);

    // Restore snap env variables
    service.restore_snap_exported_variables();
    assert_restored(&service);
}

#[test]
fn can_invoke_unset_snap_exported_variables_and_restore_snap_exported_variables_multiple_times() {
    let service = service();
    // Mock snap environment
    service.set_process_env_var("SNAP", "1");
    service.set_process_env_var("SNAP_REVISION", "test_revision");

    set_snap_variables(&service);

    // Unset snap env variables
    service.unset_snap_exported_variables();
    service.unset_snap_exported_variables();
    service.unset_snap_exported_variables();
    assert_unset(&service);

    // Restore snap env variables
    service.restore_snap_exported_variables();
    service.restore_snap_exported_variables();
    assert_restored(&service);

    // Unset snap env variables
    service.unset_snap_exported_variables();
    assert_unset(&service);
}
