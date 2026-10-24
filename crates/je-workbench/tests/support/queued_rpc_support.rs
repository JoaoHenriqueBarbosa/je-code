//! O `RPCProtocol` real do `extHostSemanticTokens.test.ts` (`src/vs/workbench/services/extensions/common/rpcProtocol.ts`), reduzido ao
//! que os quatro casos `... over RPC` usam: dois lados ligados por duas filas de mensagens que o teste entrega à mão
//! (`deliver(messages, receiver)`), com o cancelamento viajando como mensagem. O arquivo não está em `support/mod.rs`: o
//! teste que o usa o inclui com `#[path]`, para que um erro aqui não quebre os demais testes que usam `mod support`.
//!
//! No original o teste cria dois `RPCProtocol` ligados por `Emitter<VSBuffer>`: `mainThreadRpc.getProxy(ExtHostLanguageFeatures)`
//! serializa cada chamada para `extHostMessages`, e o `extHostRpc` (com o `ExtHostLanguageFeatures` registrado) só a executa
//! quando o teste entrega a mensagem; a resposta volta por `mainThreadMessages`. Aqui o [`QueuedExtHostLanguageFeatures`] é o
//! proxy do lado da thread principal (um [`ExtHostLanguageFeaturesShape`] que encaminha as chamadas) e o `extHostRpc` que
//! executa as mensagens no `ExtHostLanguageFeatures` real.
//!
//! Notas de tradução:
//! - Em Rust não há reflexão para um proxy de qualquer `$método`, então o dublê implementa as 65 funções do contrato: só
//!   `rpc_provide_document_semantic_tokens` e `rpc_release_document_semantic_tokens` (as que os casos chamam) são encaminhadas;
//!   as demais falham com `panic!`, como o método ausente do `Proxy` do original (que lançaria `TypeError`).
//! - Os `sinon.spy(extHost, '$provideDocumentSemanticTokens')` e `sinon.spy(extHost, '$releaseDocumentSemanticTokens')` são o
//!   registro, no momento em que a mensagem é executada no `ExtHostLanguageFeatures`, do valor devolvido pelo primeiro
//!   (`provide.firstCall.returnValue`, [`QueuedExtHostLanguageFeatures::first_provide_return_value`]) e dos argumentos do
//!   segundo (`release.args`, [`QueuedExtHostLanguageFeatures::release_args`]). Chamadas diretas ao `extHost` (as do próprio
//!   teste) não passam pelo dublê, como as chamadas que o `sinon.spy` não veria se fossem feitas no objeto original antes do espião.
//! - O `RPCProtocol` envia o `CancellationToken` como uma mensagem de cancelamento (`MessageType.RequestCancel`) quando o
//!   token do chamador é cancelado, e o lado que recebe cancela o token que passou ao método. Um token já cancelado rejeita
//!   a chamada sem enviar nada (`canceled()`). A resposta de uma chamada sem retorno (`$releaseDocumentSemanticTokens`) é uma
//!   mensagem que ninguém lê, então não é modelada. A serialização das mensagens (`MarshalledId`, `VSBuffer`) não tem
//!   equivalente: os argumentos e os resultados do porte já são valores.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::{CodeActionContext, TextEdit as EditorTextEdit};
use je_editor::folding::syntax_range_provider::{FoldingContext, FoldingRange as EditorFoldingRange};
use je_editor::language_providers::{DocumentHighlight, DocumentSymbol, SelectionRange as EditorSelectionRange};
use je_editor::languages::CompletionContext;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_workbench::api::common::ext_host_protocol::*;
use je_workbench::services::inline_completions::inline_completions_unification::IInlineCompletionsUnificationState;

use crate::support::ext_host_language_features_support::timeout;

/// Uma mensagem do protocolo: o que o receptor faz ao recebê-la.
type Message = Box<dyn FnOnce()>;

/// `extHostMessages` ou `mainThreadMessages`: as mensagens enviadas e ainda não entregues.
#[derive(Clone, Default)]
pub struct MessageQueue(Rc<RefCell<VecDeque<Message>>>);

impl MessageQueue {
    /// `send: message => messages.push(message)`.
    fn send(&self, message: impl FnOnce() + 'static) {
        self.0.borrow_mut().push_back(Box::new(message));
    }

    /// `deliver(messages, receiver)`: `await timeout(0)` e depois `receiver.fire(messages.shift()!)` até esvaziar a fila (o
    /// que o receptor enfileira ao tratar uma mensagem também é entregue nesta rodada).
    pub fn deliver(&self) -> Promise<()> {
        let queue = self.clone();
        timeout(0).then(move |_| {
            loop {
                let next = queue.0.borrow_mut().pop_front();
                match next {
                    Some(message) => message(),
                    None => break,
                }
            }
        })
    }
}

/// O `mainThreadRpc.getProxy(ExtHostContext.ExtHostLanguageFeatures)` ligado ao `extHostRpc` (ver as notas do módulo).
pub struct QueuedExtHostLanguageFeatures {
    /// O objeto que `extHostRpc.set(ExtHostContext.ExtHostLanguageFeatures, extHost)` registrou.
    ext_host: Rc<dyn ExtHostLanguageFeaturesShape>,
    /// `extHostMessages`: o que a thread principal enviou ao host de extensões.
    to_ext_host: MessageQueue,
    /// `mainThreadMessages`: o que o host de extensões enviou à thread principal.
    to_main_thread: MessageQueue,
    /// Os tokens de cancelamento das chamadas em andamento no host de extensões, por número da requisição
    /// (`_cancelInvokedHandlers`).
    cancel_handlers: Rc<RefCell<HashMap<u32, CancellationTokenSource>>>,
    next_request_id: Cell<u32>,
    /// `provide.returnValues`.
    provide_return_values: Rc<RefCell<Vec<Promise<Option<VSBuffer>>>>>,
    /// `release.args`.
    release_args: Rc<RefCell<Vec<(u32, u32)>>>,
}

impl QueuedExtHostLanguageFeatures {
    pub fn new(ext_host: Rc<dyn ExtHostLanguageFeaturesShape>) -> Rc<Self> {
        Rc::new(Self {
            ext_host,
            to_ext_host: MessageQueue::default(),
            to_main_thread: MessageQueue::default(),
            cancel_handlers: Rc::default(),
            next_request_id: Cell::new(0),
            provide_return_values: Rc::default(),
            release_args: Rc::default(),
        })
    }

    /// `deliver(extHostMessages, toExtHost)`.
    pub fn deliver_to_ext_host(&self) -> Promise<()> {
        self.to_ext_host.deliver()
    }

    /// `deliver(mainThreadMessages, toMainThread)`.
    pub fn deliver_to_main_thread(&self) -> Promise<()> {
        self.to_main_thread.deliver()
    }

    /// `provide.firstCall.returnValue`.
    pub fn first_provide_return_value(&self) -> Promise<Option<VSBuffer>> {
        self.provide_return_values.borrow().first().cloned().expect("o provide nunca foi chamado")
    }

    /// `release.args`: os `[handle, semanticColoringResultId]` de cada chamada.
    pub fn release_args(&self) -> Vec<(u32, u32)> {
        self.release_args.borrow().clone()
    }
}

impl IDisposable for QueuedExtHostLanguageFeatures {
    fn dispose(&self) {}
}

/// Os métodos de [`ExtHostLanguageFeaturesShape`] que o dublê não encaminha: o `Proxy` de métodos `$` do original só existe
/// para o que o teste chama, e o resto lança `TypeError`.
macro_rules! unexpected_calls {
    ($($method:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(
            #[allow(unused_variables)]
            fn $method(&self, $($arg: $ty),*) $(-> $ret)? {
                panic!("{} is not a function", stringify!($method))
            }
        )*
    };
}

impl ExtHostLanguageFeaturesShape for QueuedExtHostLanguageFeatures {
    /// `$provideDocumentSemanticTokens(handle, resource, previousResultId, token)`: a requisição vira uma mensagem para o host
    /// de extensões, e a resposta volta por outra mensagem.
    fn rpc_provide_document_semantic_tokens(&self, handle: u32, resource: &URI, previous_result_id: u32, token: CancellationToken) -> Promise<Option<VSBuffer>> {
        if token.is_cancellation_requested() {
            // No need to do anything...
            return Promise::rejected(Error::cancellation());
        }

        let request_id = self.next_request_id.get();
        self.next_request_id.set(request_id + 1);
        let (reply, resolver) = Promise::new();

        // `cancellationToken.onCancellationRequested(() => this._protocol.send(cancel message))`
        let (cancel_queue, cancel_handlers) = (self.to_ext_host.clone(), Rc::clone(&self.cancel_handlers));
        let cancel_listener = token.on_cancellation_requested().listen(move |_| {
            let cancel_handlers = Rc::clone(&cancel_handlers);
            cancel_queue.send(move || {
                let source = cancel_handlers.borrow().get(&request_id).cloned();
                if let Some(source) = source {
                    source.cancel();
                }
            });
        });

        let (ext_host, to_main_thread) = (Rc::clone(&self.ext_host), self.to_main_thread.clone());
        let (cancel_handlers, provide_return_values) = (Rc::clone(&self.cancel_handlers), Rc::clone(&self.provide_return_values));
        let resource = resource.clone();
        self.to_ext_host.send(move || {
            let source = CancellationTokenSource::new(None);
            cancel_handlers.borrow_mut().insert(request_id, source.clone());

            let result = ext_host.rpc_provide_document_semantic_tokens(handle, &resource, previous_result_id, source.token());
            provide_return_values.borrow_mut().push(result.clone());

            // `.then(r => send(reply ok), err => send(reply error))`
            let _ = result.then_result(move |outcome| {
                cancel_handlers.borrow_mut().remove(&request_id);
                source.dispose_with(false);
                to_main_thread.send(move || {
                    cancel_listener.dispose();
                    match outcome {
                        Ok(value) => resolver.resolve(value),
                        Err(error) => resolver.reject(error),
                    }
                });
                Ok::<(), Error>(())
            });
        });

        reply
    }

    /// `$releaseDocumentSemanticTokens(handle, semanticColoringResultId)`.
    fn rpc_release_document_semantic_tokens(&self, handle: u32, semantic_coloring_result_id: u32) {
        let (ext_host, release_args) = (Rc::clone(&self.ext_host), Rc::clone(&self.release_args));
        self.to_ext_host.send(move || {
            release_args.borrow_mut().push((handle, semantic_coloring_result_id));
            ext_host.rpc_release_document_semantic_tokens(handle, semantic_coloring_result_id);
        });
    }

    unexpected_calls! {
        rpc_provide_document_symbols(handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<Vec<DocumentSymbol>>>;
        rpc_provide_code_lenses(handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<ICodeLensListDto>>;
        rpc_resolve_code_lens(handle: u32, symbol: ICodeLensDto, token: CancellationToken) -> Promise<Option<ICodeLensDto>>;
        rpc_release_code_lenses(handle: u32, id: u32);
        rpc_provide_definition(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        rpc_provide_declaration(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        rpc_provide_implementation(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        rpc_provide_type_definition(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        rpc_provide_hover(handle: u32, resource: &URI, position: EditorPosition, context: Option<HoverContextDto>, token: CancellationToken) -> Promise<Option<HoverWithId>>;
        rpc_release_hover(handle: u32, id: u32);
        rpc_provide_document_highlights(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<DocumentHighlight>>>;
        rpc_provide_multi_document_highlights(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            other_models: Vec<URI>,
            token: CancellationToken
        ) -> Promise<Option<Vec<MultiDocumentHighlightDto>>>;
        rpc_provide_references(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: ReferenceContext,
            token: CancellationToken
        ) -> Promise<Option<Vec<ILocationDto>>>;
        rpc_provide_evaluatable_expression(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<EvaluatableExpressionDto>>;
        rpc_provide_inline_values(
            handle: u32,
            resource: &URI,
            range: EditorRange,
            context: IInlineValueContextDto,
            token: CancellationToken
        ) -> Promise<Option<Vec<InlineValueDto>>>;
        rpc_provide_linked_editing_ranges(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<ILinkedEditingRangesDto>>;
        rpc_provide_code_actions(
            handle: u32,
            resource: &URI,
            range_or_selection: RangeOrSelection,
            context: CodeActionContext,
            token: CancellationToken
        ) -> Promise<Option<ICodeActionListDto>>;
        rpc_resolve_code_action(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedCodeActionDto>;
        rpc_release_code_actions(handle: u32, cache_id: u32);
        rpc_prepare_document_paste(
            handle: u32,
            uri: &URI,
            ranges: Vec<EditorRange>,
            data_transfer: DataTransferDto,
            token: CancellationToken
        ) -> Promise<Option<DataTransferDto>>;
        rpc_provide_paste_edits(
            handle: u32,
            request_id: u32,
            uri: &URI,
            ranges: Vec<EditorRange>,
            data_transfer: DataTransferDto,
            context: IDocumentPasteContextDto,
            token: CancellationToken
        ) -> Promise<Option<Vec<IPasteEditDto>>>;
        rpc_resolve_paste_edit(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedPasteEditDto>;
        rpc_release_paste_edits(handle: u32, cache_id: u32);
        rpc_provide_document_formatting_edits(handle: u32, resource: &URI, options: FormattingOptions, token: CancellationToken) -> Promise<Option<Vec<EditorTextEdit>>>;
        rpc_provide_document_range_formatting_edits(
            handle: u32,
            resource: &URI,
            range: EditorRange,
            options: FormattingOptions,
            token: CancellationToken
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        rpc_provide_document_ranges_formatting_edits(
            handle: u32,
            resource: &URI,
            ranges: Vec<EditorRange>,
            options: FormattingOptions,
            token: CancellationToken
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        rpc_provide_on_type_formatting_edits(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            ch: &str,
            options: FormattingOptions,
            token: CancellationToken
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        rpc_provide_workspace_symbols(handle: u32, search: &str, token: CancellationToken) -> Promise<IWorkspaceSymbolsDto>;
        rpc_resolve_workspace_symbol(handle: u32, symbol: IWorkspaceSymbolDto, token: CancellationToken) -> Promise<Option<IWorkspaceSymbolDto>>;
        rpc_release_workspace_symbols(handle: u32, id: u32);
        rpc_provide_rename_edits(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            new_name: &str,
            token: CancellationToken
        ) -> Promise<Option<RenameEditsDto>>;
        rpc_resolve_rename_location(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<RenameLocationDto>>;
        rpc_supports_automatic_new_symbol_names_trigger_kind(handle: u32) -> Promise<Option<bool>>;
        rpc_provide_new_symbol_names(
            handle: u32,
            resource: &URI,
            range: EditorRange,
            trigger_kind: LanguagesNewSymbolNameTriggerKind,
            token: CancellationToken
        ) -> Promise<Option<Vec<NewSymbolNameDto>>>;
        rpc_provide_document_range_semantic_tokens(handle: u32, resource: &URI, range: EditorRange, token: CancellationToken) -> Promise<Option<VSBuffer>>;
        rpc_provide_completion_items(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: CompletionContext,
            token: CancellationToken
        ) -> Promise<Option<ISuggestResultDto>>;
        rpc_resolve_completion_item(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ISuggestDataDto>>;
        rpc_release_completion_items(handle: u32, id: u32);
        rpc_provide_signature_help(
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: ISignatureHelpContextDto,
            token: CancellationToken
        ) -> Promise<Option<ISignatureHelpDto>>;
        rpc_release_signature_help(handle: u32, id: u32);
        rpc_provide_inlay_hints(handle: u32, resource: &URI, range: EditorRange, token: CancellationToken) -> Promise<Option<IInlayHintsDto>>;
        rpc_resolve_inlay_hint(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<IInlayHintDto>>;
        rpc_release_inlay_hints(handle: u32, id: u32);
        rpc_provide_document_links(handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<ILinksListDto>>;
        rpc_resolve_document_link(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ILinkDto>>;
        rpc_release_document_links(handle: u32, id: u32);
        rpc_provide_document_colors(handle: u32, resource: &URI, token: CancellationToken) -> Promise<Vec<IRawColorInfo>>;
        rpc_provide_color_presentations(
            handle: u32,
            resource: &URI,
            color_info: IRawColorInfo,
            token: CancellationToken
        ) -> Promise<Option<Vec<IColorPresentationDto>>>;
        rpc_provide_folding_ranges(handle: u32, resource: &URI, context: FoldingContext, token: CancellationToken) -> Promise<Option<Vec<EditorFoldingRange>>>;
        rpc_provide_selection_ranges(
            handle: u32,
            resource: &URI,
            positions: Vec<EditorPosition>,
            token: CancellationToken
        ) -> Promise<Vec<Vec<EditorSelectionRange>>>;
        rpc_prepare_call_hierarchy(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ICallHierarchyItemDto>>>;
        rpc_provide_call_hierarchy_incoming_calls(
            handle: u32,
            session_id: &str,
            item_id: &str,
            token: CancellationToken
        ) -> Promise<Option<Vec<IIncomingCallDto>>>;
        rpc_provide_call_hierarchy_outgoing_calls(
            handle: u32,
            session_id: &str,
            item_id: &str,
            token: CancellationToken
        ) -> Promise<Option<Vec<IOutgoingCallDto>>>;
        rpc_release_call_hierarchy(handle: u32, session_id: &str);
        rpc_prepare_type_hierarchy(handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        rpc_provide_type_hierarchy_supertypes(
            handle: u32,
            session_id: &str,
            item_id: &str,
            token: CancellationToken
        ) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        rpc_provide_type_hierarchy_subtypes(
            handle: u32,
            session_id: &str,
            item_id: &str,
            token: CancellationToken
        ) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        rpc_release_type_hierarchy(handle: u32, session_id: &str);
        rpc_provide_document_on_drop_edits(
            handle: u32,
            request_id: u32,
            resource: &URI,
            position: EditorPosition,
            data_transfer: DataTransferDto,
            token: CancellationToken
        ) -> Promise<Option<Vec<IDocumentDropEditDto>>>;
        rpc_resolve_drop_edit(handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedDropEditDto>;
        rpc_release_document_on_drop_edits(handle: u32, cache_id: u32);
        rpc_set_word_definitions(word_definitions: Vec<ILanguageWordDefinitionDto>);
        rpc_accept_inline_completions_unification_state(state: IInlineCompletionsUnificationState);
    }
}
