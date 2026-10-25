//! Porte de `src/vs/workbench/contrib/files/test/browser/fileOnDiskProvider.test.ts` (suíte
//! `Files - FileOnDiskContentProvider`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo do
//!   caso e o `setup` vem logo depois: `workbenchInstantiationService(undefined, disposables)` e `TestServiceAccessor` são o
//!   [`TextFileTestAccessor`], e o gerenciador de modelos é registrado para descarte como em `save_participant.rs`. O
//!   `teardown` (`disposables.clear()`) é o descarte do guarda ao sair de escopo.
//! - **`instantiationService.createInstance(TextFileContentProvider)`.** É [`TextFileContentProvider::new`] com os quatro
//!   serviços que o construtor injeta (`ITextFileService`, `IFileService`, `ILanguageService`, `IModelService`).
//! - **`await provider.provideTextContent(...)`.** O contrato devolve um `MaybePromise`; o caso espera a promessa.
//! - **`content.createSnapshot()`.** É `create_snapshot(false)` (o `preserveBOM` padrão do original).
//! - **`URI.parse('testFileOnDiskContentProvider://foo')`, `uri.with(...)` e `JSON.stringify({ scheme })`.** São
//!   `URI::parse`, `URI::with` e `serde_json::json!`, que serializa `{"scheme":"..."}` como o original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::lifecycle::IDisposable;
use je_base::promise::MaybePromise;
use je_base::uri::{URI, UriChange};
use je_workbench::contrib::files::common_files::TextFileContentProvider;
use je_workbench::services::textfile::textfiles::snapshot_to_string;
use je_workbench::services::textmodel_resolver::resolver_service::ITextModelContentProvider;
use support::text_file_support::TextFileTestAccessor;

#[test]
fn provide_text_content() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = TextFileTestAccessor::new(&guard.store());
        guard.add_rc(accessor.files());

        let provider = guard.add_rc(TextFileContentProvider::new(
            accessor.text_file_service.clone(),
            accessor.file_service.clone(),
            accessor.language_service.clone(),
            accessor.model_service.clone(),
        ));
        let uri = URI::parse("testFileOnDiskContentProvider://foo", false);

        let query = serde_json::json!({ "scheme": uri.scheme().to_string_lossy() }).to_string();
        let resource = uri.with(&UriChange { scheme: "conflictResolution".into(), query: query.as_str().into(), ..UriChange::default() });
        let content = match provider.provide_text_content(&resource) {
            MaybePromise::Value(content) => content,
            MaybePromise::Promise(promise) => promise.await.expect("provideTextContent"),
        };

        let content = content.expect("o provedor devolve o modelo");
        assert_eq!(snapshot_to_string(&mut *content.create_snapshot(false)), "Hello Html");
        let last_read = accessor.file_service.get_last_read_file_uri().expect("o arquivo foi lido");
        assert_eq!(last_read.scheme().to_string_lossy(), uri.scheme().to_string_lossy());
        assert_eq!(last_read.path().to_string_lossy(), uri.path().to_string_lossy());

        content.dispose();
    });
}
