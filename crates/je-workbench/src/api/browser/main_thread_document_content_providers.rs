//! Porte de `src/vs/workbench/api/browser/mainThreadDocumentContentProviders.ts`: os provedores de conteúdo de texto
//! (`workspace.registerTextDocumentContentProvider`) que as extensões registram no host e o workbench consulta.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadDocumentContentProviders)` e o `IExtHostContext` do construtor: o
//!   registro de clientes não existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de
//!   `ExtHostDocumentContentProviders`.
//! - `UriComponents` e `URI.revive` são a [`URI`] direto. O `handle` é `u64` (como no protocolo).
//! - O `string` de `$onVirtualDocumentChange` é [`JsString`]; o `TextEdit.text` do editor é `String`, então o texto vai
//!   para a edição pelo `to_string_lossy` (um substituto isolado do host vira U+FFFD, o que o tipo da edição não
//!   consegue guardar).
//! - `value.substr(0, 1 + value.search(/\r?\n/))` é feito sobre as unidades UTF-16: o índice do primeiro `\n` (ou do `\r`
//!   que o precede) mais um, ou zero quando não há quebra de linha (`search` devolve `-1`).
//! - O `async`/`await` de `$onVirtualDocumentChange` é uma cadeia sobre a promessa de `computeMoreMinimalEdits`, e o
//!   `try/catch` em volta do `await` é o ramo `Err`: o erro vai para `onUnexpectedError` e o token pendente fica no mapa,
//!   como no original (a remoção vem depois do `await`).
//! - `Range.lift(edit.range)` é a própria `Range` da edição.
//! - O mapa `_pendingUpdate` fica num `Rc<RefCell<HashMap>>`, porque a continuação da promessa o captura.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationTokenSource;
use je_base::errors::on_unexpected_error;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::lifecycle::DisposableMap;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit;
use je_editor::edit_operation::EditOperation;
use je_editor::editor_worker::IEditorWorkerService;
use je_editor::language_service::ILanguageService;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};

use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostDocumentContentProvidersShape, IMainContext, MainThreadDocumentContentProvidersShape,
};
use crate::services::textmodel_resolver::resolver_service::{ITextModelContentProvider, ITextModelService};

type PendingUpdates = Rc<RefCell<HashMap<String, CancellationTokenSource>>>;

/// O texto da primeira linha de `value` com a quebra, para o `createByFilepathOrFirstLine`:
/// `value.substr(0, 1 + value.search(/\r?\n/))`.
fn first_line_text(value: &JsString) -> JsString {
    const CR: u16 = b'\r' as u16;
    const LF: u16 = b'\n' as u16;
    let units = value.units();
    let index = (0..units.len()).find(|&i| units[i] == LF || (units[i] == CR && units.get(i + 1) == Some(&LF)));
    value.slice(0, index.map_or(0, |index| index + 1))
}

/// O provedor que `$registerTextContentProvider` entrega ao `ITextModelService`.
struct ExtHostContentProvider {
    handle: u64,
    proxy: Rc<dyn ExtHostDocumentContentProvidersShape>,
    language_service: Rc<dyn ILanguageService>,
    model_service: Rc<dyn IModelService>,
}

impl ITextModelContentProvider for ExtHostContentProvider {
    fn provide_text_content(&self, uri: &URI) -> MaybePromise<Option<Rc<TextModel>>> {
        let language_service = self.language_service.clone();
        let model_service = self.model_service.clone();
        let uri = uri.clone();
        MaybePromise::Promise(self.proxy.rpc_provide_text_document_content(self.handle, &uri).then(move |value| {
            let value = value?;
            let first_line = first_line_text(&value);
            let language_selection = language_service.create_by_filepath_or_first_line(Some(&uri), Some(&first_line));
            Some(model_service.create_model(TextSource::Text(value), Some(&*language_selection), Some(&uri), false))
        }))
    }
}

/// `MainThreadDocumentContentProviders`.
pub struct MainThreadDocumentContentProviders {
    resource_content_provider: Rc<DisposableMap<u64, dyn IDisposable>>,
    pending_update: PendingUpdates,
    proxy: Rc<dyn ExtHostDocumentContentProvidersShape>,
    text_model_resolver_service: Rc<dyn ITextModelService>,
    language_service: Rc<dyn ILanguageService>,
    model_service: Rc<dyn IModelService>,
    editor_worker_service: Rc<dyn IEditorWorkerService>,
}

impl MainThreadDocumentContentProviders {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        text_model_resolver_service: Rc<dyn ITextModelService>,
        language_service: Rc<dyn ILanguageService>,
        model_service: Rc<dyn IModelService>,
        editor_worker_service: Rc<dyn IEditorWorkerService>,
    ) -> Rc<Self> {
        Rc::new(Self {
            resource_content_provider: DisposableMap::new(),
            pending_update: Rc::default(),
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_DOCUMENT_CONTENT_PROVIDERS),
            text_model_resolver_service,
            language_service,
            model_service,
            editor_worker_service,
        })
    }
}

impl IDisposable for MainThreadDocumentContentProviders {
    fn dispose(&self) {
        self.resource_content_provider.dispose();
        let pending: Vec<CancellationTokenSource> = self.pending_update.borrow().values().cloned().collect();
        for token_source in pending {
            token_source.dispose();
        }
    }
}

impl MainThreadDocumentContentProvidersShape for MainThreadDocumentContentProviders {
    fn rpc_register_text_content_provider(&self, handle: u64, scheme: &str) {
        let registration: DisposableRef = self.text_model_resolver_service.register_text_model_content_provider(
            scheme,
            Rc::new(ExtHostContentProvider {
                handle,
                proxy: self.proxy.clone(),
                language_service: self.language_service.clone(),
                model_service: self.model_service.clone(),
            }),
        );
        self.resource_content_provider.set(handle, registration, false);
    }

    fn rpc_unregister_text_content_provider(&self, handle: u64) {
        self.resource_content_provider.delete_and_dispose(&handle);
    }

    fn rpc_on_virtual_document_change(&self, uri: &URI, value: &JsString) -> Promise<()> {
        let Some(model) = self.model_service.get_model(uri) else {
            return Promise::resolved(());
        };

        // cancel and dispose an existing update
        let pending = self.pending_update.borrow().get(model.id()).cloned();
        if let Some(pending) = pending {
            pending.cancel();
        }

        // create and keep update token
        let my_token = CancellationTokenSource::new(None);
        self.pending_update.borrow_mut().insert(model.id().to_owned(), my_token.clone());

        let edits = self
            .editor_worker_service
            .compute_more_minimal_edits(&model.uri(), Some(vec![TextEdit { range: model.get_full_model_range(), text: value.to_string_lossy(), eol: None, insert_as_snippet: None, keep_whitespace: None }]));
        let pending_update = self.pending_update.clone();
        edits.then_result(move |result| {
            match result {
                Ok(edits) => {
                    // remove token
                    pending_update.borrow_mut().remove(model.id());

                    if my_token.token().is_cancellation_requested() {
                        // ignore this
                        return Ok(());
                    }

                    if model.is_disposed() {
                        // model was disposed during the async operation
                        return Ok(());
                    }
                    if let Some(edits) = edits.filter(|edits| !edits.is_empty()) {
                        // use the evil-edit as these models show in readonly-editor only
                        model.apply_edits(&edits.into_iter().map(|edit| EditOperation::replace(edit.range, Some(JsString::from(edit.text)))).collect::<Vec<_>>());
                    }
                }
                Err(error) => on_unexpected_error(&error),
            }
            Ok(())
        })
    }
}
