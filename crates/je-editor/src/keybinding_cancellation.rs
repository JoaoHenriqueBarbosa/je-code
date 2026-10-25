//! Porte de `src/vs/editor/contrib/editorState/browser/keybindingCancellation.ts`: o serviço
//! `IEditorCancellationTokens` (as operações canceláveis de cada editor, com a chave de contexto
//! `cancellableOperation`) e o `EditorKeybindingCancellationTokenSource`, que se registra nele.
//!
//! Decisões de tradução:
//! - `editor.invokeWithinContext(accessor => accessor.get(IContextKeyService))` é o [`ContextKeyServiceFor`] que o
//!   serviço recebe: dado o editor, devolve o `IContextKeyService` do escopo dele.
//! - O `WeakMap<ICodeEditor, ...>` é um mapa pelo endereço do editor; a identidade do `CancellationTokenSource` na
//!   `LinkedList` é a identidade do token (`CancellationToken::is_same`).
//! - `LinkedList.push` devolve a função de remoção, que aqui é o [`DisposableRef`] devolvido por `add`; `LinkedList.pop`
//!   tira o último, como o `Vec::pop`.
//! - `EditorKeybindingCancellationTokenSource extends CancellationTokenSource` é o [`Deref`] para a fonte, e o `dispose`
//!   sobrescrito é o [`IDisposable`] desta estrutura.
//! - O comando `editor.cancelOperation` (`registerEditorCommand`) fica com quem monta o registro de comandos do editor:
//!   ele chama [`EditorCancellationTokens::cancel`] com o editor em foco.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};

use crate::code_editor_widget::CodeEditorWidget;

thread_local! {
    static CTX_CANCELLABLE_OPERATION: RawContextKey = RawContextKey::new(
        "cancellableOperation",
        false,
        ContextKeyMeta::Description("Whether the editor runs a cancellable operation, e.g. like 'Peek References'".to_owned()),
    );
}

/// `ctxCancellableOperation`.
pub fn ctx_cancellable_operation() -> RawContextKey {
    CTX_CANCELLABLE_OPERATION.with(Clone::clone)
}

/// O `editor.invokeWithinContext(accessor => accessor.get(IContextKeyService))`.
pub type ContextKeyServiceFor = Box<dyn Fn(&CodeEditorWidget) -> Rc<dyn IContextKeyService>>;

/// Os dados de um editor no serviço: a chave `cancellableOperation` e as fontes de cancelamento em curso.
struct EditorTokens {
    key: Rc<dyn IContextKey>,
    tokens: RefCell<Vec<CancellationTokenSource>>,
}

impl EditorTokens {
    fn refresh_key(&self) {
        self.key.set(Dynamic::Bool(!self.tokens.borrow().is_empty()));
    }
}

/// `IEditorCancellationTokens`.
pub struct EditorCancellationTokens {
    context_key_service_for: ContextKeyServiceFor,
    tokens: RefCell<HashMap<usize, Rc<EditorTokens>>>,
}

impl EditorCancellationTokens {
    pub fn new(context_key_service_for: ContextKeyServiceFor) -> Rc<Self> {
        Rc::new(Self { context_key_service_for, tokens: RefCell::new(HashMap::new()) })
    }

    fn editor_id(editor: &CodeEditorWidget) -> usize {
        std::ptr::from_ref(editor) as usize
    }

    /// `add(editor, cts)`: a remoção (sem cancelar) é o descarte do valor devolvido.
    pub fn add(&self, editor: &CodeEditorWidget, cts: &CancellationTokenSource) -> DisposableRef {
        let id = Self::editor_id(editor);
        let existing = self.tokens.borrow().get(&id).cloned();
        let data = existing.unwrap_or_else(|| {
            let service = (self.context_key_service_for)(editor);
            let data = Rc::new(EditorTokens { key: ctx_cancellable_operation().bind_to(&*service), tokens: RefCell::new(Vec::new()) });
            self.tokens.borrow_mut().insert(id, Rc::clone(&data));
            data
        });

        data.key.set(Dynamic::Bool(true));
        data.tokens.borrow_mut().push(cts.clone());

        let token = cts.token();
        to_disposable(move || {
            // remove w/o cancellation
            let mut tokens = data.tokens.borrow_mut();
            if let Some(index) = tokens.iter().position(|candidate| candidate.token().is_same(&token)) {
                tokens.remove(index);
                drop(tokens);
                data.refresh_key();
            }
        })
    }

    /// `cancel(editor)`.
    pub fn cancel(&self, editor: &CodeEditorWidget) {
        let Some(data) = self.tokens.borrow().get(&Self::editor_id(editor)).cloned() else {
            return;
        };
        // remove with cancellation
        let cts = data.tokens.borrow_mut().pop();
        if let Some(cts) = cts {
            cts.cancel();
            data.refresh_key();
        }
    }
}

/// `EditorKeybindingCancellationTokenSource`.
pub struct EditorKeybindingCancellationTokenSource {
    source: CancellationTokenSource,
    unregister: DisposableRef,
}

impl EditorKeybindingCancellationTokenSource {
    pub fn new(editor: &CodeEditorWidget, tokens: &EditorCancellationTokens, parent: Option<&CancellationToken>) -> Self {
        let source = CancellationTokenSource::new(parent);
        let unregister = tokens.add(editor, &source);
        Self { source, unregister }
    }
}

impl Deref for EditorKeybindingCancellationTokenSource {
    type Target = CancellationTokenSource;

    fn deref(&self) -> &CancellationTokenSource {
        &self.source
    }
}

impl IDisposable for EditorKeybindingCancellationTokenSource {
    fn dispose(&self) {
        self.unregister.dispose();
        self.source.dispose();
    }
}
