//! Porte de `src/vs/workbench/contrib/notebook/browser/services/notebookRendererMessagingServiceImpl.ts`: o
//! `NotebookRendererMessagingService`, que ativa o renderizador (`onRenderer:<id>`) antes da primeira mensagem, guarda em fila o que
//! chega durante a ativação e entrega a mensagem do editor ao renderizador certo.
//!
//! Divergências:
//! - `Map<string, ...>` (ordem de inserção) é um `Vec` de pares para `scopedMessaging`, porque `receiveMessage` percorre os valores nessa
//!   ordem; `activations` (só busca por chave) é um `HashMap` de `Option<Vec<MessageToSend>>`, em que `None` é o `undefined` da ativação
//!   completa.
//! - O `for (const message of queue)` do `.then` de `prepare` vê as mensagens que entram na fila durante o laço (como o `for...of` de um
//!   array que cresce): o laço relê a fila por índice.
//! - O objeto literal de `getScoped` é o [`ScopedRendererMessaging`], com o `receiveMessageHandler` atribuível.
//! - O construtor devolve `Rc<Self>` (os fechos guardam um `Weak`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;

use crate::contrib::notebook::common::notebook_renderer_messaging_service::{
    INotebookRendererMessagingService, IScopedRendererMessaging, MessageToSend, ReceiveMessageHandler,
};
use crate::services::extensions::extensions::IExtensionService;

/// O objeto que `getScoped(editorId)` devolve.
pub struct ScopedRendererMessaging {
    editor_id: String,
    service: Weak<NotebookRendererMessagingService>,
    receive_message_handler: RefCell<Option<ReceiveMessageHandler>>,
}

impl IScopedRendererMessaging for ScopedRendererMessaging {
    fn receive_message_handler(&self) -> Option<ReceiveMessageHandler> {
        self.receive_message_handler.borrow().clone()
    }

    fn set_receive_message_handler(&self, handler: Option<ReceiveMessageHandler>) {
        *self.receive_message_handler.borrow_mut() = handler;
    }

    fn post_message(&self, renderer_id: &str, message: Dynamic) {
        if let Some(service) = self.service.upgrade() {
            service.post_message(&self.editor_id, renderer_id, message);
        }
    }
}

impl IDisposable for ScopedRendererMessaging {
    /// `dispose: () => this.scopedMessaging.delete(editorId)`.
    fn dispose(&self) {
        if let Some(service) = self.service.upgrade() {
            service.scoped_messaging.borrow_mut().retain(|(editor_id, _)| *editor_id != self.editor_id);
        }
    }
}

/// `NotebookRendererMessagingService`.
pub struct NotebookRendererMessagingService {
    this: Weak<NotebookRendererMessagingService>,
    base: Rc<Disposable>,
    extension_service: Rc<dyn IExtensionService>,
    /// Activation promises. Maps renderer IDs to a queue of messages that should be sent once activation finishes, or `None` if
    /// activation is complete.
    activations: RefCell<HashMap<String, Option<Vec<MessageToSend>>>>,
    scoped_messaging: RefCell<Vec<(String, Rc<ScopedRendererMessaging>)>>,
    post_message_emitter: Rc<Emitter<MessageToSend>>,
}

impl NotebookRendererMessagingService {
    pub fn new(extension_service: Rc<dyn IExtensionService>) -> Rc<Self> {
        let base = Disposable::new();
        let post_message_emitter = Emitter::<MessageToSend>::new();
        base.register(post_message_emitter.clone());
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            extension_service,
            activations: RefCell::new(HashMap::new()),
            scoped_messaging: RefCell::new(Vec::new()),
            post_message_emitter,
        })
    }

    fn post_message(&self, editor_id: &str, renderer_id: &str, message: Dynamic) {
        if !self.activations.borrow().contains_key(renderer_id) {
            self.prepare(renderer_id);
        }

        let to_send = MessageToSend { renderer_id: renderer_id.to_string(), editor_id: editor_id.to_string(), message };
        let ready_to_send = {
            let mut activations = self.activations.borrow_mut();
            match activations.get_mut(renderer_id).and_then(Option::as_mut) {
                Some(queue) => {
                    queue.push(to_send);
                    None
                }
                // activation === undefined
                None => Some(to_send),
            }
        };
        if let Some(to_send) = ready_to_send {
            self.post_message_emitter.fire(to_send);
        }
    }
}

impl INotebookRendererMessagingService for NotebookRendererMessagingService {
    fn on_should_post_message(&self) -> Event<MessageToSend> {
        self.post_message_emitter.event()
    }

    fn receive_message(&self, editor_id: Option<&str>, renderer_id: &str, message: Dynamic) -> Promise<bool> {
        let handle = |scoped: &Rc<ScopedRendererMessaging>| scoped.receive_message_handler().map(|handler| handler(renderer_id, message.clone()));
        match editor_id {
            None => {
                let sends: Vec<Promise<bool>> =
                    self.scoped_messaging.borrow().iter().map(|(_, scoped)| handle(scoped).unwrap_or_else(|| Promise::resolved(false))).collect();
                Promise::all(sends).then(|results| results.into_iter().any(|received| received))
            }
            Some(editor_id) => {
                let scoped = self.scoped_messaging.borrow().iter().find(|(id, _)| id == editor_id).map(|(_, scoped)| scoped.clone());
                scoped.and_then(|scoped| handle(&scoped)).unwrap_or_else(|| Promise::resolved(false))
            }
        }
    }

    fn prepare(&self, renderer_id: &str) {
        if self.activations.borrow().contains_key(renderer_id) {
            return;
        }

        self.activations.borrow_mut().insert(renderer_id.to_string(), Some(Vec::new()));

        let weak = self.this.clone();
        let renderer_id = renderer_id.to_string();
        let _ = self.extension_service.activate_by_event(&format!("onRenderer:{renderer_id}"), None).then(move |()| {
            let Some(this) = weak.upgrade() else { return };
            let mut index = 0;
            loop {
                let next = this.activations.borrow().get(&renderer_id).and_then(Option::as_ref).and_then(|queue| queue.get(index).cloned());
                let Some(message) = next else { break };
                this.post_message_emitter.fire(message);
                index += 1;
            }

            this.activations.borrow_mut().insert(renderer_id, None);
        });
    }

    fn get_scoped(&self, editor_id: &str) -> Rc<dyn IScopedRendererMessaging> {
        let existing = self.scoped_messaging.borrow().iter().find(|(id, _)| id == editor_id).map(|(_, scoped)| scoped.clone());
        if let Some(existing) = existing {
            return existing;
        }

        let messaging = Rc::new(ScopedRendererMessaging { editor_id: editor_id.to_string(), service: self.this.clone(), receive_message_handler: RefCell::new(None) });
        self.scoped_messaging.borrow_mut().push((editor_id.to_string(), messaging.clone()));
        messaging
    }
}

impl IDisposable for NotebookRendererMessagingService {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookRendererMessagingService"
    }
}
