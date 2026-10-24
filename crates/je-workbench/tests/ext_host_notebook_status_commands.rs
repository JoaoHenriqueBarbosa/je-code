//! Porte de `src/vs/workbench/api/test/browser/extHostNotebookStatusCommands.test.ts` (suíte `Notebook cell status command lifetime`), caso a caso e na mesma ordem.
//!
//! Os 3 casos estão portados.
//!
//! Notas de tradução:
//! - O original troca `ExtHostCommands`, `ExtHostDocumentsAndEditors`, `ExtHostDocuments`, `IExtHostConsumerFileSystem` e `IExtHostSearch`
//!   por `mock<T>()` e entrega o documento da célula pelo `getDocument()` dublê. O porte usa os objetos reais montados por
//!   `support/ext_host_notebook_support.rs` (o `TestMainContext` tem o proxy de comandos e o de documentos): o documento da célula
//!   nasce do `addedDocuments` do próprio `$acceptDocumentAndEditorsDelta`, e o `CommandsConverter` é o do `ExtHostCommands` real
//!   (o `registerCommand` real chama o `$registerCommand` do dublê, que não faz nada). O que os casos conferem, o ciclo de vida
//!   dos argumentos do comando, não depende dessa troca.
//! - `assert.strictEqual(converter.fromInternal(dto), command)` confere o comando e os argumentos: o `Command` do porte é um valor
//!   (copiado no cache do conversor), então a identidade é conferida nos argumentos (`strict_equals` de objetos é a identidade).
//! - O id `-1` de `$releaseNotebookCellStatusBarItems(-1)` não existe em `u32`: é `u32::MAX`, igualmente nunca emitido (os ids
//!   do `Cache` começam em 1).
//! - `store.add(new Disposable(() => { }))` do `registerCommand` dublê não existe (o comando é o real).

mod support;

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::path::Platform;
use je_base::promise::MaybePromise;
use je_base::uri::URI;
use je_workbench::api::common::ext_host_notebook::{ExtHostNotebookController, NotebookCellStatusBarItemProvider, NotebookCellStatusBarItems};
use je_workbench::api::common::ext_host_notebook_document::NotebookCell;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostNotebookDocumentsAndEditorsShape, ExtHostNotebookShape, INotebookDocumentsAndEditorsDelta, INotebookModelAddedData, SerializableObjectWithBuffers,
};
use je_workbench::api::common::ext_host_type_converters::ICommandsConverter;
use je_workbench::api::common::ext_host_types as types;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_notebook_support::common::run_with_faked_timers;
use support::ext_host_notebook_support::{LeakGuard, NotebookHost, cell_dto, ensure_no_disposables_are_leaked_in_test_suite, new_notebook_host};

/// `{ provideCellStatusBarItems: () => ({ text: 'Inspect', alignment: Left, command }) }`.
struct InspectProvider {
    command: types::Command,
}

impl NotebookCellStatusBarItemProvider for InspectProvider {
    fn provide_cell_status_bar_items(&self, _cell: &NotebookCell, _token: CancellationToken) -> MaybePromise<Option<NotebookCellStatusBarItems>> {
        let mut item = types::NotebookCellStatusBarItem::new("Inspect", types::NotebookCellStatusBarAlignment::Left);
        item.command = Some(types::NotebookCellStatusBarCommand::Command(self.command.clone()));
        MaybePromise::Value(Some(NotebookCellStatusBarItems::One(item)))
    }
}

struct Setup {
    /// `store`: o guarda cria-se antes do `setup` e confere ao fim.
    store: LeakGuard,
    uri: URI,
    host: NotebookHost,
    service: Rc<ExtHostNotebookController>,
}

/// `setup(() => { ... })`.
fn setup() -> Setup {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = URI::file("/status.notebook", if cfg!(windows) { Platform::Win32 } else { Platform::Posix });
    let host = new_notebook_host();
    // o `ExtHostDocuments` real guarda um `DisposableStore` rastreado (no original o dublê não o tem)
    store.add_rc(host.documents.clone());
    let service = host.notebooks.clone();
    let cell_uri_notebook = uri.clone();
    service.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
        added_documents: Some(vec![INotebookModelAddedData {
            uri: uri.clone(),
            view_type: "test".to_owned(),
            version_id: 0,
            cells: vec![cell_dto(&cell_uri_notebook, 0, &["sample"], "plaintext", CellKind::Code)],
            metadata: None,
        }]),
        ..INotebookDocumentsAndEditorsDelta::default()
    }));
    let cleanup: DisposableRef = {
        let service = service.clone();
        let uri = uri.clone();
        to_disposable(move || {
            service.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
                removed_documents: Some(vec![uri.clone()]),
                ..INotebookDocumentsAndEditorsDelta::default()
            }))
        })
    };
    store.add(cleanup);
    Setup { store, uri, host, service }
}

/// `register()`: devolve o comando e o handle que `$registerNotebookCellStatusBarItemProvider` recebeu.
fn register(setup: &Setup) -> (types::Command, u32) {
    let command = types::Command {
        command: "test.status".to_owned(),
        title: "Inspect Status".to_owned(),
        tooltip: None,
        arguments: Some(vec![Dynamic::object([("value", Dynamic::string("original"))])]),
    };
    let disposable = setup.service.register_notebook_cell_status_bar_item_provider(&null_extension_description(), "test", Rc::new(InspectProvider { command: command.clone() }));
    setup.store.add(Rc::new(disposable));
    let handle = *setup.host.main_thread_notebook.status_bar_handles.borrow().last().expect("$registerNotebookCellStatusBarItemProvider");
    (command, handle)
}

/// `assert.strictEqual(actual, expected)` entre o comando devolvido pelo conversor e o original.
fn assert_same_command(actual: Option<types::Command>, expected: &types::Command) {
    let actual = actual.expect("o comando deveria estar guardado");
    assert_eq!(actual.command, expected.command);
    assert_eq!(actual.title, expected.title);
    let (actual_args, expected_args) = (actual.arguments.expect("arguments"), expected.arguments.clone().expect("arguments"));
    assert_eq!(actual_args.len(), expected_args.len());
    for (actual, expected) in actual_args.iter().zip(&expected_args) {
        assert!(actual.strict_equals(expected), "o argumento deveria ser o mesmo objeto");
    }
}

#[test]
fn releases_command_arguments_with_the_cell_status_result() {
    run_with_faked_timers(async move {
        let setup = setup();
        let converter = setup.host.commands.converter();
        let (command, handle) = register(&setup);
        let result = setup
            .service
            .rpc_provide_notebook_cell_status_bar_items(handle, &setup.uri, 0, CancellationToken::None)
            .await
            .expect("$provideNotebookCellStatusBarItems")
            .expect("result");
        let dto = result.items[0].command.clone().expect("command");
        assert_same_command(converter.from_internal(&dto), &command);
        setup.service.rpc_release_notebook_cell_status_bar_items(result.cache_id);
        assert!(converter.from_internal(&dto).is_none());
        setup.service.rpc_release_notebook_cell_status_bar_items(result.cache_id);
    });
}

#[test]
fn releasing_one_result_preserves_another_live_result() {
    run_with_faked_timers(async move {
        let setup = setup();
        let converter = setup.host.commands.converter();
        let (command, handle) = register(&setup);
        let first = setup
            .service
            .rpc_provide_notebook_cell_status_bar_items(handle, &setup.uri, 0, CancellationToken::None)
            .await
            .expect("$provideNotebookCellStatusBarItems")
            .expect("first");
        let second = setup
            .service
            .rpc_provide_notebook_cell_status_bar_items(handle, &setup.uri, 0, CancellationToken::None)
            .await
            .expect("$provideNotebookCellStatusBarItems")
            .expect("second");
        let first_command = first.items[0].command.clone().expect("command");
        let second_command = second.items[0].command.clone().expect("command");
        setup.service.rpc_release_notebook_cell_status_bar_items(first.cache_id);
        assert!(converter.from_internal(&first_command).is_none());
        assert_same_command(converter.from_internal(&second_command), &command);
        setup.service.rpc_release_notebook_cell_status_bar_items(second.cache_id);
    });
}

#[test]
fn ignores_unknown_result_ids() {
    let setup = setup();
    setup.service.rpc_release_notebook_cell_status_bar_items(u32::MAX);
    setup.service.rpc_release_notebook_cell_status_bar_items(u32::MAX);
}
