//! Porte de `src/vs/workbench/services/telemetry/test/browser/commonProperties.test.ts` (suíte `Browser Telemetry - common properties`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` viram [`setup`] (o `InMemoryStorageService` entra no guarda, que o descarta no fim).
//! - `{ isSessionsWindow: false } as IWorkbenchEnvironmentService` é o `TestEnvironmentService`, cujo padrão
//!   também tem `isSessionsWindow` falso; `undefined!` do `productService` é `None`.
//! - `Object.defineProperties` com `get` enumerável é uma propriedade avaliada a cada chamada do
//!   `resolveCommonTelemetryProperties`: a closure devolve o valor corrente do contador.
//! - `hasKey(props, {...})` é `has_own` para cada chave.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_workbench::services::telemetry::browser::workbench_common_properties::resolve_workbench_common_properties;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;

fn setup(disposables: &LeakGuard) -> Rc<dyn StorageService> {
    let storage_service = disposables.add_rc(InMemoryStorageService::new());
    Rc::new(storage_service.clone())
}

#[test]
fn mixes_in_additional_properties() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_storage_service = setup(&guard);
    let environment_service = TestEnvironmentService::new();

    let resolve_common_telemetry_properties = || vec![("userId".to_string(), Dynamic::from("1"))];

    let props = resolve_workbench_common_properties(&*test_storage_service, None, &*environment_service, false, Some(&resolve_common_telemetry_properties));

    for key in [
        "commitHash",
        "sessionID",
        "timestamp",
        "common.platform",
        "common.timesincesessionstart",
        "common.sequence",
        "version",
        "common.firstSessionDate",
        "common.lastSessionDate",
        "common.isNewSession",
        "common.machineId",
    ] {
        assert!(props.has_own(key), "falta a chave {key}");
    }
    assert!(props.get("userId").strict_equals(&Dynamic::from("1")));
}

#[test]
fn mixes_in_additional_dyanmic_properties() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_storage_service = setup(&guard);
    let environment_service = TestEnvironmentService::new();

    let i = Cell::new(1);
    let resolve_common_telemetry_properties = || {
        let value = i.get();
        i.set(value + 1);
        vec![("userId".to_string(), Dynamic::from(value))]
    };

    let props = resolve_workbench_common_properties(&*test_storage_service, None, &*environment_service, false, Some(&resolve_common_telemetry_properties));
    assert!(props.get("userId").strict_equals(&Dynamic::from(1)));

    let props2 = resolve_workbench_common_properties(&*test_storage_service, None, &*environment_service, false, Some(&resolve_common_telemetry_properties));
    assert!(props2.get("userId").strict_equals(&Dynamic::from(2)));
}
