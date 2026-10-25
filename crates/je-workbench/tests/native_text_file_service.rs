//! Porte de `src/vs/workbench/services/textfile/test/electron-browser/nativeTextFileService.test.ts` (suíte `Files - NativeTextFileService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois. O `teardown`
//!   (`disposables.clear()`) é o descarte do store do guarda, que roda quando o guarda sai de escopo e antes de ele conferir
//!   os vazamentos. Os casos `async` rodam em `run_with_faked_timers`; `await` é `.await.unwrap()`.
//! - **`setup`.** `workbenchInstantiationService`, o `FileService` com o provedor em memória registrado para o esquema `file`, o
//!   `WorkingCopyFileService` (com `WorkingCopyService` e `UriIdentityService` próprios), o `createChild(collection)` e a
//!   `TestNativeTextFileServiceWithEncodingOverrides` são [`browser_io_child_services`] e
//!   [`TextFileTestAccessor::with_native_child_services`] (em `tests/support/text_file_support.rs`). O provedor é o
//!   `TestInMemoryFileSystemProvider` do suporte, que é o `InMemoryFileSystemProvider` do original com o `readFileStream` em
//!   trechos; a suíte não lê arquivos por ele. O `disposables.add(<TextFileEditorModelManager>service.files)` é o descarte do
//!   gerenciador no store do guarda.
//! - **`instantiationService.createInstance(TextFileEditorModel, resource, 'utf8', undefined)`** é
//!   [`TextFileTestAccessor::create_model`] (que, no acessor nativo, usa o `IFileService` do pai, como o original).
//! - **`instantiationService.createInstance(TestServiceAccessor)`** é o próprio acessor, que expõe o `lifecycle_service`.
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título do caso (a suíte
//!   mais o título do caso) é o argumento de [`to_resource`].
//! - **`model.save().then(() => pendingSaveAwaited = true)`** é a promessa de `save` encadeada num `Cell`, que a execução do
//!   caso mantém viva (o `then` do original também não é aguardado).
//! - **`Promise.all(accessor.lifecycleService.shutdownJoiners)`** é `Promise::all` sobre o `shutdown_joiners` do
//!   `TestLifecycleService`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_workbench::common::editor::editor_model::IEditorModel;
use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::resources::{host_environment, uri_file};
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use support::text_file_support::{TextFileTestAccessor, browser_io_child_services};

//#region Utilitários dos casos

/// O prefixo do `this.test.fullTitle()` do mocha: o título da suíte.
const SUITE_TITLE: &str = "Files - NativeTextFileService";

/// O `setup`: o filho com o `FileService` em memória e a `TestNativeTextFileServiceWithEncodingOverrides`, e o gerenciador de
/// modelos como descartável (`disposables.add(<TextFileEditorModelManager>service.files)`).
fn setup(disposables: &LeakGuard) -> TextFileTestAccessor {
    let (child, _file_provider) = browser_io_child_services(&disposables.store());
    let accessor = TextFileTestAccessor::with_native_child_services(&disposables.store(), child);
    disposables.add_rc(accessor.files());

    accessor
}

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();
    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

//#endregion

#[test]
fn shutdown_joins_on_pending_saves() {
    const TITLE: &str = "shutdown joins on pending saves";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        model.resolve().await.unwrap();

        let pending_save_awaited = Rc::new(Cell::new(false));
        let awaited = pending_save_awaited.clone();
        let _pending_save = model.save(None).then(move |_| awaited.set(true));

        accessor.lifecycle_service.fire_shutdown(None);

        let joiners = accessor.lifecycle_service.shutdown_joiners.borrow().clone();
        assert!(!joiners.is_empty());
        Promise::all(joiners).await.unwrap();

        assert!(pending_save_awaited.get());
    });
}
