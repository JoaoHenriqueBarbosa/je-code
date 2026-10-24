//! Porte de `src/vs/workbench/api/test/browser/extHostBulkEdits.test.ts` (suíte `ExtHostBulkEdits.applyWorkspaceEdit`), caso a caso e na mesma ordem.
//!
//! PORTADOS: os 2 casos do original. Nenhum fica de fora.
//!
//! Notas de tradução:
//! - `rpcProtocol.set(MainContext.MainThreadBulkEdits, new class extends mock<MainThreadBulkEditsShape>() { ... })` é o
//!   [`RecordingMainThreadBulkEdits`], que guarda o `value` do `SerializableObjectWithBuffers` recebido e resolve `true`.
//! - `SingleProxyRPCProtocol(null)` do `ExtHostDocumentsAndEditors` devolve um proxy nulo para qualquer identificador (nunca usado
//!   pelo caso). O porte registra o dublê de `MainThreadDocuments` que falha se for chamado, em outro [`TestMainContext`].
//! - `ensureNoDisposablesAreLeakedInTestSuite()` sem `store` é o guarda criado no começo do caso. `workspaceResourceEdits = null!`
//!   do `setup` é o `None` inicial.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model::Eol;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_bulk_edits::ExtHostBulkEdits;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    DocumentsAndEditorsDelta, ExtHostDocumentsAndEditorsShape, IWorkspaceEditDto, MainContext, MainThreadBulkEditsShape, MainThreadDocumentsShape, ModelAddedData,
    SerializableObjectWithBuffers, WorkspaceEditEntryDto,
};
use je_workbench::api::common::ext_host_types as types;
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_language_features_support::UnusedMainThreadDocuments;
use support::ext_host_workspace_support::TestMainContext;

/// `new class extends mock<MainThreadBulkEditsShape>() { override $tryApplyWorkspaceEdit(...) { workspaceResourceEdits = ...value; return Promise.resolve(true); } }`.
struct RecordingMainThreadBulkEdits {
    workspace_resource_edits: Rc<RefCell<Option<IWorkspaceEditDto>>>,
}

impl IDisposable for RecordingMainThreadBulkEdits {
    fn dispose(&self) {}
}

impl MainThreadBulkEditsShape for RecordingMainThreadBulkEdits {
    fn rpc_try_apply_workspace_edit(
        &self,
        workspace_edit_dto: SerializableObjectWithBuffers<IWorkspaceEditDto>,
        _undo_redo_group_id: Option<f64>,
        _is_refactoring: Option<bool>,
    ) -> Promise<bool> {
        *self.workspace_resource_edits.borrow_mut() = Some(workspace_edit_dto.value);
        Promise::resolved(true)
    }
}

/// `const resource = URI.parse('foo:bar')`.
fn resource() -> URI {
    URI::parse("foo:bar", false)
}

/// O `setup`: o `ExtHostBulkEdits` com o documento `foo:bar` (versão 1337) aberto e o que o `$tryApplyWorkspaceEdit` recebeu.
fn setup() -> (ExtHostBulkEdits, Rc<RefCell<Option<IWorkspaceEditDto>>>) {
    let workspace_resource_edits: Rc<RefCell<Option<IWorkspaceEditDto>>> = Rc::new(RefCell::new(None));

    let rpc_protocol = TestMainContext::new();
    rpc_protocol.set(
        &MainContext::MAIN_THREAD_BULK_EDITS,
        Rc::new(RecordingMainThreadBulkEdits { workspace_resource_edits: Rc::clone(&workspace_resource_edits) }) as Rc<dyn MainThreadBulkEditsShape>,
    );
    let documents_rpc_protocol = Rc::new(TestMainContext::new());
    documents_rpc_protocol.set(&MainContext::MAIN_THREAD_DOCUMENTS, Rc::new(UnusedMainThreadDocuments) as Rc<dyn MainThreadDocumentsShape>);
    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(documents_rpc_protocol, NullLogService::shared()));
    documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
        added_documents: Some(vec![ModelAddedData {
            is_dirty: false,
            language_id: "foo".to_owned(),
            uri: resource(),
            version_id: 1337,
            lines: vec![JsString::from("foo")],
            eol: Eol::Lf,
            encoding: "utf8".to_owned(),
        }]),
        ..DocumentsAndEditorsDelta::default()
    });
    (ExtHostBulkEdits::new(&rpc_protocol, documents_and_editors), workspace_resource_edits)
}

/// `edit.replace(uri, new Range(0, 0, 0, 0), 'hello')` e `await bulkEdits.applyWorkspaceEdit(edit, nullExtensionDescription, undefined)`.
async fn apply_replace(bulk_edits: &ExtHostBulkEdits, uri: &URI) {
    let mut edit = types::WorkspaceEdit::new();
    edit.replace(uri, types::Range::from_coordinates(0.0, 0.0, 0.0, 0.0).expect("Range"), "hello", None);
    bulk_edits.apply_workspace_edit(&edit, &null_extension_description(), None).await.expect("applyWorkspaceEdit");
}

#[test]
fn uses_version_id_if_document_available() {
    run_with_faked_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (bulk_edits, workspace_resource_edits) = setup();
        apply_replace(&bulk_edits, &resource()).await;
        let received = workspace_resource_edits.borrow();
        let edits = &received.as_ref().expect("$tryApplyWorkspaceEdit não foi chamado").edits;
        assert_eq!(edits.len(), 1);
        let WorkspaceEditEntryDto::Text(first) = &edits[0] else { panic!("a primeira edição deveria ser de texto") };
        assert_eq!(first.version_id, Some(1337));
    });
}

#[test]
fn does_not_use_version_id_if_document_is_not_available() {
    run_with_faked_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (bulk_edits, workspace_resource_edits) = setup();
        apply_replace(&bulk_edits, &URI::parse("foo:bar2", false)).await;
        let received = workspace_resource_edits.borrow();
        let edits = &received.as_ref().expect("$tryApplyWorkspaceEdit não foi chamado").edits;
        assert_eq!(edits.len(), 1);
        let WorkspaceEditEntryDto::Text(first) = &edits[0] else { panic!("a primeira edição deveria ser de texto") };
        assert!(first.version_id.is_none());
    });
}
