//! Porte de `src/vs/workbench/api/test/browser/extHostNotebook.test.ts` (suíte `NotebookCell#Document`), caso a caso e na mesma ordem.
//!
//! PORTADOS (20 de 21 `test(`): todos, menos o caso abaixo.
//!
//! FORA, e por quê (não vira `#[test]` enquanto não puder ser fiel):
//! - `onDidChangeNotebook-event, froozen data`: confere `Object.isFrozen` do evento, de `cellChanges`, de `contentChanges` e
//!   do notebook, e que `metadata` NÃO é congelado. O evento do porte é um struct de Rust (sem a noção de objeto congelado):
//!   a imutabilidade é do tipo, e a asserção não tem correspondente.
//!
//! Notas de tradução:
//! - `new TestRPCProtocol()` é o [`TestMainContext`] de `support/ext_host_workspace_support.rs`, montado por
//!   `support/ext_host_notebook_support.rs` (ver as notas de lá). `ExtHostConsumerFileSystem` e `ExtHostSearch` não existem no
//!   porte: o controlador recebe o dublê de arquivo e não recebe a busca.
//! - `Event.toPromise(evento)` é `.to_promise(None)`. Nos casos em que o original registra o ouvinte e confere dentro dele
//!   (`onDidChangeNotebookDocument`), o porte cria a promessa antes e confere depois de aguardá-la (os valores são os mesmos).
//! - `isEqual(a, b)` entre as URIs dos documentos é a igualdade do texto da URI (todas têm caminho sensível a maiúsculas).
//! - `assert.strictEqual(a, b)` entre objetos da API (`event.notebook === notebook.apiNotebook`, `cellChange.cell === first`)
//!   é a identidade do `Rc` (`ptr_eq`).

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_editor::model::Eol;
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_notebook_document::NotebookCell;
use je_workbench::api::common::ext_host_notebook_editor::NotebookEditor;
use je_workbench::api::common::ext_host_protocol::{
    DocumentsAndEditorsDelta, ExtHostDocumentsAndEditorsShape, ExtHostNotebookDocumentsAndEditorsShape, ExtHostNotebookDocumentsShape,
    INotebookDocumentsAndEditorsDelta, INotebookEditorAddData, ModelAddedData, NotebookCellDto, NotebookCellsChangedEventDto, NotebookOutputItemDto,
    NotebookRawContentEventDto, SerializableObjectWithBuffers,
};
use je_workbench::api::common::ext_host_type_converters::CellRange;
use je_workbench::api::common::ext_host_types as types;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, NotebookCellTextModelSplice};
use support::ext_host_notebook_support::common::{assert_deep_strict_equal, run_with_faked_timers};
use support::ext_host_notebook_support::{NotebookEnv, cell_dto, lines_of, output_dto, output_item};

/// `new SerializableObjectWithBuffers({ versionId, rawEvents })`.
fn changed(version_id: i64, raw_events: Vec<NotebookRawContentEventDto>) -> SerializableObjectWithBuffers<NotebookCellsChangedEventDto> {
    SerializableObjectWithBuffers::new(NotebookCellsChangedEventDto { raw_events, version_id })
}

/// `{ kind: NotebookCellsChangeType.ModelChange, changes }`.
fn model_change(changes: Vec<NotebookCellTextModelSplice<NotebookCellDto>>) -> NotebookRawContentEventDto {
    NotebookRawContentEventDto::ModelChange { changes }
}

fn metadata_of(key: &str, value: f64) -> Dynamic {
    Dynamic::object([(key, Dynamic::Number(value))])
}

fn uri_text(document: &TextDocument) -> String {
    document.uri().to_string(false).to_string_lossy()
}

fn item_text(item: &types::NotebookCellOutputItem) -> String {
    String::from_utf8(item.data.clone()).expect("texto utf-8")
}

/// `assert.deepStrictEqual(a, b)` entre duas listas de saídas.
fn assert_outputs_equal(actual: &[types::NotebookCellOutput], expected: &[types::NotebookCellOutput]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.id, expected.id);
        assert_eq!(actual.items, expected.items);
        match (&actual.metadata, &expected.metadata) {
            (None, None) => {}
            (Some(actual), Some(expected)) => assert_deep_strict_equal(actual, expected),
            _ => panic!("os metadados das saídas diferem"),
        }
    }
}

#[test]
fn cell_document_is_vscode_text_document() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        assert_eq!(env.notebook.api_notebook().cell_count(), 2);

        let cells = env.notebook.api_notebook().get_cells(None);
        let (c1, c2) = (&cells[0], &cells[1]);
        let d1 = env.host.documents.get_document(c1.document().uri()).expect("document");

        assert_eq!(d1.language_id(), c1.document().language_id());
        assert_eq!(d1.version(), 1.0);

        let d2 = env.host.documents.get_document(c2.document().uri()).expect("document");
        assert_eq!(d2.language_id(), c2.document().language_id());
        assert_eq!(d2.version(), 1.0);
    });
}

#[test]
fn cell_document_goes_when_notebook_closes() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();
        let mut cell_uris: Vec<String> = Vec::new();
        for cell in env.notebook.api_notebook().get_cells(None) {
            assert!(env.host.documents.get_document(cell.document().uri()).is_ok());
            cell_uris.push(uri_text(&cell.document()));
        }

        let removed_cell_uris: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let reg = env.host.documents.on_did_remove_document().listen({
            let removed_cell_uris = removed_cell_uris.clone();
            move |doc: &TextDocument| removed_cell_uris.borrow_mut().push(uri_text(doc))
        });

        env.host.notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
            removed_documents: Some(vec![env.notebook.uri.clone()]),
            ..INotebookDocumentsAndEditorsDelta::default()
        }));
        reg.dispose();

        let mut removed = removed_cell_uris.borrow().clone();
        assert_eq!(removed.len(), 2);
        removed.sort();
        cell_uris.sort();
        assert_eq!(removed, cell_uris);
    });
}

#[test]
fn cell_document_is_vscode_text_document_after_changing_it() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        let p = env.notebook_documents.on_did_change_notebook_document().to_promise(None);

        env.notebook_documents.rpc_accept_model_changed(
            &env.notebook_uri,
            changed(
                env.notebook.api_notebook().version() + 1,
                vec![model_change(vec![(
                    0,
                    0,
                    vec![
                        cell_dto(&env.notebook_uri, 2, &["Hello", "World", "Hello World!"], "test", CellKind::Code),
                        cell_dto(&env.notebook_uri, 3, &["Hallo", "Welt", "Hallo Welt!"], "test", CellKind::Code),
                    ],
                )])],
            ),
            false,
            None,
        );

        let e = p.await.expect("onDidChangeNotebookDocument");
        assert_eq!(e.content_changes.len(), 1);
        assert_eq!(e.content_changes[0].added_cells.len(), 2);

        let (first, second) = (&e.content_changes[0].added_cells[0], &e.content_changes[0].added_cells[1]);

        let all_data = env.host.documents.get_all_document_data();
        let doc1 = all_data.iter().find(|data| uri_text(&data.document()) == uri_text(&first.document())).expect("doc1");
        assert!(TextDocument::ptr_eq(&doc1.document(), &first.document()));

        let doc2 = all_data.iter().find(|data| uri_text(&data.document()) == uri_text(&second.document())).expect("doc2");
        assert!(TextDocument::ptr_eq(&doc2.document(), &second.document()));
    });
}

#[test]
fn cell_document_stays_open_when_notebook_is_still_open() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        let mut docs: Vec<TextDocument> = Vec::new();
        let mut add_data: Vec<ModelAddedData> = Vec::new();
        for cell in env.notebook.api_notebook().get_cells(None) {
            let doc = env.host.documents.get_document(cell.document().uri()).expect("document");
            assert!(!env.host.documents.get_document(cell.document().uri()).expect("document").is_closed());
            add_data.push(ModelAddedData {
                eol: Eol::Lf,
                is_dirty: doc.is_dirty(),
                lines: lines_of(&doc.get_text(None).to_string_lossy()),
                language_id: doc.language_id(),
                uri: doc.uri().clone(),
                version_id: doc.version() as i64,
                encoding: "utf8".to_owned(),
            });
            docs.push(doc);
        }

        // this call happens when opening a document on the main side
        env.host.documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
            added_documents: Some(add_data),
            ..DocumentsAndEditorsDelta::default()
        });

        // this call happens when closing a document from the main side
        env.host.documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
            removed_documents: Some(docs.iter().map(|d| d.uri().clone()).collect()),
            ..DocumentsAndEditorsDelta::default()
        });

        // notebook is still open -> cell documents stay open
        for cell in env.notebook.api_notebook().get_cells(None) {
            assert!(env.host.documents.get_document(cell.document().uri()).is_ok());
            assert!(!env.host.documents.get_document(cell.document().uri()).expect("document").is_closed());
        }

        // close notebook -> docs are closed
        env.host.notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
            removed_documents: Some(vec![env.notebook.uri.clone()]),
            ..INotebookDocumentsAndEditorsDelta::default()
        }));
        for cell in env.notebook.api_notebook().get_cells(None) {
            assert!(env.host.documents.get_document(cell.document().uri()).is_err());
        }
        for doc in &docs {
            assert!(doc.is_closed());
        }
    });
}

#[test]
fn cell_document_goes_when_cell_is_removed() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        assert_eq!(env.notebook.api_notebook().cell_count(), 2);
        let cells = env.notebook.api_notebook().get_cells(None);
        let (cell1, cell2) = (&cells[0], &cells[1]);

        env.notebook_documents.rpc_accept_model_changed(&env.notebook.uri, changed(2, vec![model_change(vec![(0, 1, vec![])])]), false, None);

        assert_eq!(env.notebook.api_notebook().cell_count(), 1);
        assert!(cell1.document().is_closed()); // ref still alive!
        assert!(!cell2.document().is_closed());

        assert!(env.host.documents.get_document(cell1.document().uri()).is_err());
    });
}

#[test]
fn cell_index() {
    let env = NotebookEnv::new();

    assert_eq!(env.notebook.api_notebook().cell_count(), 2);
    let cells = env.notebook.api_notebook().get_cells(None);
    let (first, second) = (&cells[0], &cells[1]);
    assert_eq!(first.index(), 0.0);
    assert_eq!(second.index(), 1.0);

    // remove first cell
    env.notebook_documents.rpc_accept_model_changed(
        &env.notebook.uri,
        changed(env.notebook.api_notebook().version() + 1, vec![model_change(vec![(0, 1, vec![])])]),
        false,
        None,
    );

    assert_eq!(env.notebook.api_notebook().cell_count(), 1);
    assert_eq!(second.index(), 0.0);

    env.notebook_documents.rpc_accept_model_changed(
        &env.notebook_uri,
        changed(
            env.notebook.api_notebook().version() + 1,
            vec![model_change(vec![(
                0,
                0,
                vec![
                    cell_dto(&env.notebook_uri, 2, &["Hello", "World", "Hello World!"], "test", CellKind::Code),
                    cell_dto(&env.notebook_uri, 3, &["Hallo", "Welt", "Hallo Welt!"], "test", CellKind::Code),
                ],
            )])],
        ),
        false,
        None,
    );

    assert_eq!(env.notebook.api_notebook().cell_count(), 3);
    assert_eq!(second.index(), 2.0);
}

#[test]
fn err_missing_ext_host_document_for_notebook_cell_116711() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        let p = env.notebook_documents.on_did_change_notebook_document().to_promise(None);

        // DON'T call this, make sure the cell-documents have not been created yet
        // assert.strictEqual(notebook.notebookDocument.cellCount, 2);

        env.notebook_documents.rpc_accept_model_changed(
            &env.notebook.uri,
            changed(
                100,
                vec![model_change(vec![(
                    0,
                    2,
                    vec![
                        cell_dto(&env.notebook_uri, 3, &["### Heading"], "markdown", CellKind::Markup),
                        cell_dto(&env.notebook_uri, 4, &["console.log(\"aaa\")", "console.log(\"bbb\")"], "javascript", CellKind::Code),
                    ],
                )])],
            ),
            false,
            None,
        );

        assert_eq!(env.notebook.api_notebook().cell_count(), 2);

        let event = p.await.expect("onDidChangeNotebookDocument");

        assert!(je_workbench::api::common::ext_host_notebook_document::NotebookDocument::ptr_eq(&event.notebook, &env.notebook.api_notebook()));
        assert_eq!(event.content_changes.len(), 1);
        assert_eq!(event.content_changes[0].range.end() - event.content_changes[0].range.start(), 2.0);
        assert!(event.content_changes[0].removed_cells[0].document().is_closed());
        assert!(event.content_changes[0].removed_cells[1].document().is_closed());
        assert_eq!(event.content_changes[0].added_cells.len(), 2);
        assert!(!event.content_changes[0].added_cells[0].document().is_closed());
        assert!(!event.content_changes[0].added_cells[1].document().is_closed());
    });
}

#[test]
fn opening_a_notebook_results_in_vs_code_firing_the_event_on_did_change_active_notebook_editor_twice_118470() {
    let env = NotebookEnv::new();
    let count = Rc::new(Cell::new(0));
    let listener = env.host.notebooks.on_did_change_active_notebook_editor().listen({
        let count = count.clone();
        move |_| count.set(count.get() + 1)
    });
    env.guard.add(listener);

    env.host.notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
        added_editors: Some(vec![INotebookEditorAddData {
            document_uri: env.notebook_uri.clone(),
            id: "_notebook_editor_2".to_owned(),
            selections: vec![CellRange { start: 0.0, end: 1.0 }],
            visible_ranges: Vec::new(),
            view_column: None,
            view_type: "test".to_owned(),
        }]),
        ..INotebookDocumentsAndEditorsDelta::default()
    }));

    env.host.notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
        new_active_editor: Some(Some("_notebook_editor_2".to_owned())),
        ..INotebookDocumentsAndEditorsDelta::default()
    }));

    assert_eq!(count.get(), 1);
}

#[test]
fn unset_active_notebook_editor() {
    let env = NotebookEnv::new();

    let editor = env.host.notebooks.active_notebook_editor().expect("editor ativo");

    // `{ newActiveEditor: undefined }`: a propriedade ausente
    env.host.notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta::default()));
    assert!(NotebookEditor::ptr_eq(&env.host.notebooks.active_notebook_editor().expect("editor ativo"), &editor));

    env.host.notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta::default()));
    assert!(NotebookEditor::ptr_eq(&env.host.notebooks.active_notebook_editor().expect("editor ativo"), &editor));

    env.host.notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
        new_active_editor: Some(None),
        ..INotebookDocumentsAndEditorsDelta::default()
    }));
    assert!(env.host.notebooks.active_notebook_editor().is_none());
}

#[test]
fn change_cell_language_triggers_on_did_change_events() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        let first = env.notebook.api_notebook().cell_at(0.0).expect("cell");

        assert_eq!(first.document().language_id(), "markdown");

        let removed = env.host.documents.on_did_remove_document().to_promise(None);
        let added = env.host.documents.on_did_add_document().to_promise(None);

        env.notebook_documents.rpc_accept_model_changed(
            &env.notebook.uri,
            changed(12, vec![NotebookRawContentEventDto::ChangeCellLanguage { index: 0, language: "fooLang".to_owned() }]),
            false,
            None,
        );

        let removed_doc = removed.await.expect("onDidRemoveDocument");
        let added_doc = added.await.expect("onDidAddDocument");

        assert_eq!(first.document().language_id(), "fooLang");
        assert!(TextDocument::ptr_eq(&removed_doc, &added_doc));
    });
}

#[test]
fn on_did_change_notebook_event_cell_changes() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        let p = env.notebook_documents.on_did_change_notebook_document().to_promise(None);

        env.notebook_documents.rpc_accept_model_changed(
            &env.notebook.uri,
            changed(
                12,
                vec![
                    NotebookRawContentEventDto::ChangeCellMetadata { index: 0, metadata: metadata_of("foo", 1.0) },
                    NotebookRawContentEventDto::ChangeCellMetadata { index: 1, metadata: metadata_of("foo", 2.0) },
                    NotebookRawContentEventDto::Output {
                        index: 1,
                        outputs: vec![output_dto(
                            "1",
                            vec![NotebookOutputItemDto { value_bytes: VSBuffer::from_byte_array(&[0, 2, 3]), mime: "text/plain".to_owned() }],
                        )],
                    },
                ],
            ),
            false,
            None,
        );

        let event = p.await.expect("onDidChangeNotebookDocument");

        assert!(je_workbench::api::common::ext_host_notebook_document::NotebookDocument::ptr_eq(&event.notebook, &env.notebook.api_notebook()));
        assert_eq!(event.content_changes.len(), 0);
        assert_eq!(event.cell_changes.len(), 2);

        let (first, second) = (&event.cell_changes[0], &event.cell_changes[1]);
        assert_deep_strict_equal(first.metadata.as_ref().expect("metadata"), &first.cell.metadata());
        assert!(first.execution_summary.is_none());
        assert!(first.outputs.is_none());
        assert!(first.document.is_none());

        assert_outputs_equal(second.outputs.as_deref().expect("outputs"), &second.cell.outputs());
        assert_deep_strict_equal(second.metadata.as_ref().expect("metadata"), &second.cell.metadata());
        assert!(second.execution_summary.is_none());
        assert!(second.document.is_none());
    });
}

#[test]
fn on_did_change_notebook_event_notebook_metadata() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        let p = env.notebook_documents.on_did_change_notebook_document().to_promise(None);

        env.notebook_documents.rpc_accept_model_changed(&env.notebook.uri, changed(12, Vec::new()), false, Some(metadata_of("foo", 2.0)));

        let event = p.await.expect("onDidChangeNotebookDocument");

        assert!(je_workbench::api::common::ext_host_notebook_document::NotebookDocument::ptr_eq(&event.notebook, &env.notebook.api_notebook()));
        assert_eq!(event.content_changes.len(), 0);
        assert_eq!(event.cell_changes.len(), 0);
        assert_deep_strict_equal(event.metadata.as_ref().expect("metadata"), &metadata_of("foo", 2.0));
    });
}

#[test]
fn change_cell_language_and_on_did_change_notebook_document() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        let p = env.notebook_documents.on_did_change_notebook_document().to_promise(None);

        let first = env.notebook.api_notebook().cell_at(0.0).expect("cell");
        assert_eq!(first.document().language_id(), "markdown");

        env.notebook_documents.rpc_accept_model_changed(
            &env.notebook.uri,
            changed(12, vec![NotebookRawContentEventDto::ChangeCellLanguage { index: 0, language: "fooLang".to_owned() }]),
            false,
            None,
        );

        let event = p.await.expect("onDidChangeNotebookDocument");

        assert!(je_workbench::api::common::ext_host_notebook_document::NotebookDocument::ptr_eq(&event.notebook, &env.notebook.api_notebook()));
        assert_eq!(event.content_changes.len(), 0);
        assert_eq!(event.cell_changes.len(), 1);

        let cell_change = &event.cell_changes[0];

        assert!(NotebookCell::ptr_eq(&cell_change.cell, &first));
        assert!(TextDocument::ptr_eq(cell_change.document.as_ref().expect("document"), &first.document()));
        assert!(cell_change.execution_summary.is_none());
        assert!(cell_change.metadata.is_none());
        assert!(cell_change.outputs.is_none());
    });
}

#[test]
fn change_notebook_cell_document_and_on_did_change_notebook_document() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();

        let p = env.notebook_documents.on_did_change_notebook_document().to_promise(None);

        let first = env.notebook.api_notebook().cell_at(0.0).expect("cell");

        env.notebook_documents.rpc_accept_model_changed(
            &env.notebook.uri,
            changed(12, vec![NotebookRawContentEventDto::ChangeCellContent { index: 0 }]),
            false,
            None,
        );

        let event = p.await.expect("onDidChangeNotebookDocument");

        assert!(je_workbench::api::common::ext_host_notebook_document::NotebookDocument::ptr_eq(&event.notebook, &env.notebook.api_notebook()));
        assert_eq!(event.content_changes.len(), 0);
        assert_eq!(event.cell_changes.len(), 1);

        let cell_change = &event.cell_changes[0];

        assert!(NotebookCell::ptr_eq(&cell_change.cell, &first));
        assert!(TextDocument::ptr_eq(cell_change.document.as_ref().expect("document"), &first.document()));
        assert!(cell_change.execution_summary.is_none());
        assert!(cell_change.metadata.is_none());
        assert!(cell_change.outputs.is_none());
    });
}

/// `replaceOutputs(cellIndex, outputId, outputItems)`.
async fn replace_outputs(env: &NotebookEnv, cell_index: usize, output_id: &str, output_items: Vec<NotebookOutputItemDto>) {
    let change_event = env.notebook_documents.on_did_change_notebook_document().to_promise(None);
    env.notebook_documents.rpc_accept_model_changed(
        &env.notebook.uri,
        changed(
            env.notebook.api_notebook().version() + 1,
            vec![NotebookRawContentEventDto::Output { index: cell_index, outputs: vec![output_dto(output_id, output_items)] }],
        ),
        false,
        None,
    );
    change_event.await.expect("onDidChangeNotebookDocument");
}

/// `appendOutputItem(cellIndex, outputId, outputItems)`.
async fn append_output_item(env: &NotebookEnv, cell_index: usize, output_id: &str, output_items: Vec<NotebookOutputItemDto>) {
    let change_event = env.notebook_documents.on_did_change_notebook_document().to_promise(None);
    env.notebook_documents.rpc_accept_model_changed(
        &env.notebook.uri,
        changed(
            env.notebook.api_notebook().version() + 1,
            vec![NotebookRawContentEventDto::OutputItem { index: cell_index, append: true, output_id: output_id.to_owned(), output_items }],
        ),
        false,
        None,
    );
    change_event.await.expect("onDidChangeNotebookDocument");
}

const STDOUT: &str = "application/vnd.code.notebook.stdout";
const STDERR: &str = "application/vnd.code.notebook.stderr";

#[test]
fn append_multiple_text_plain_output_items() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();
        replace_outputs(&env, 1, "1", vec![output_item("text/plain", "foo")]).await;
        append_output_item(&env, 1, "1", vec![output_item("text/plain", "bar")]).await;
        append_output_item(&env, 1, "1", vec![output_item("text/plain", "baz")]).await;

        let outputs = env.notebook.api_notebook().cell_at(1.0).expect("cell").outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].items.len(), 3);
        assert_eq!(outputs[0].items[0].mime(), "text/plain");
        assert_eq!(item_text(&outputs[0].items[0]), "foo");
        assert_eq!(outputs[0].items[1].mime(), "text/plain");
        assert_eq!(item_text(&outputs[0].items[1]), "bar");
        assert_eq!(outputs[0].items[2].mime(), "text/plain");
        assert_eq!(item_text(&outputs[0].items[2]), "baz");
    });
}

#[test]
fn append_multiple_stdout_stream_output_items_to_an_output_with_another_mime() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();
        replace_outputs(&env, 1, "1", vec![output_item("text/plain", "foo")]).await;
        append_output_item(&env, 1, "1", vec![output_item(STDOUT, "bar")]).await;
        append_output_item(&env, 1, "1", vec![output_item(STDOUT, "baz")]).await;

        let outputs = env.notebook.api_notebook().cell_at(1.0).expect("cell").outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].items.len(), 3);
        assert_eq!(outputs[0].items[0].mime(), "text/plain");
        assert_eq!(outputs[0].items[1].mime(), STDOUT);
        assert_eq!(outputs[0].items[2].mime(), STDOUT);
    });
}

#[test]
fn compress_multiple_stdout_stream_output_items() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();
        replace_outputs(&env, 1, "1", vec![output_item(STDOUT, "foo")]).await;
        append_output_item(&env, 1, "1", vec![output_item(STDOUT, "bar")]).await;
        append_output_item(&env, 1, "1", vec![output_item(STDOUT, "baz")]).await;

        let outputs = env.notebook.api_notebook().cell_at(1.0).expect("cell").outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].items.len(), 1);
        assert_eq!(outputs[0].items[0].mime(), STDOUT);
        assert_eq!(item_text(&outputs[0].items[0]), "foobarbaz");
    });
}

#[test]
fn compress_multiple_stdout_stream_output_items_with_support_for_terminal_escape_code_move_cursor_up() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();
        replace_outputs(&env, 1, "1", vec![output_item(STDOUT, "\nfoo")]).await;
        append_output_item(&env, 1, "1", vec![output_item(STDOUT, &format!("{}[Abar", char::from(27u8)))]).await;

        let outputs = env.notebook.api_notebook().cell_at(1.0).expect("cell").outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].items.len(), 1);
        assert_eq!(outputs[0].items[0].mime(), STDOUT);
        assert_eq!(item_text(&outputs[0].items[0]), "bar");
    });
}

#[test]
fn compress_multiple_stdout_stream_output_items_with_support_for_terminal_escape_code_carriage_return() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();
        replace_outputs(&env, 1, "1", vec![output_item(STDOUT, "foo")]).await;
        append_output_item(&env, 1, "1", vec![output_item(STDOUT, "\rbar")]).await;

        let outputs = env.notebook.api_notebook().cell_at(1.0).expect("cell").outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].items.len(), 1);
        assert_eq!(outputs[0].items[0].mime(), STDOUT);
        assert_eq!(item_text(&outputs[0].items[0]), "bar");
    });
}

#[test]
fn compress_multiple_stderr_stream_output_items() {
    run_with_faked_timers(async move {
        let env = NotebookEnv::new();
        replace_outputs(&env, 1, "1", vec![output_item(STDERR, "foo")]).await;
        append_output_item(&env, 1, "1", vec![output_item(STDERR, "bar")]).await;
        append_output_item(&env, 1, "1", vec![output_item(STDERR, "baz")]).await;

        let outputs = env.notebook.api_notebook().cell_at(1.0).expect("cell").outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].items.len(), 1);
        assert_eq!(outputs[0].items[0].mime(), STDERR);
        assert_eq!(item_text(&outputs[0].items[0]), "foobarbaz");
    });
}
