//! Porte de `src/vs/workbench/contrib/notebook/common/notebookRendererMessagingService.ts`: o contrato do serviço de mensagens entre o
//! host de extensões e os renderizadores de saída (`INotebookRendererMessagingService`) e o objeto por editor
//! (`IScopedRendererMessaging`).
//!
//! Divergências:
//! - `unknown` (o corpo da mensagem) é o [`Dynamic`].
//! - `receiveMessageHandler?: (rendererId, message) => Promise<boolean>`, uma propriedade atribuível, é o par
//!   [`IScopedRendererMessaging::receive_message_handler`] e [`IScopedRendererMessaging::set_receive_message_handler`].
//! - `Promise<boolean>` é a [`Promise`] do porte, e `postMessage` devolve `void`.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_instantiation::service_id;

/// `{ editorId: string; rendererId: string; message: unknown }`.
#[derive(Clone)]
pub struct MessageToSend {
    pub editor_id: String,
    pub renderer_id: String,
    pub message: Dynamic,
}

/// `(rendererId, message) => Promise<boolean>`.
pub type ReceiveMessageHandler = Rc<dyn Fn(&str, Dynamic) -> Promise<bool>>;

/// `INotebookRendererMessagingService`.
pub trait INotebookRendererMessagingService {
    /// Event that fires when a message should be posted to extension hosts.
    fn on_should_post_message(&self) -> Event<MessageToSend>;

    /// Prepares messaging for the given renderer ID.
    fn prepare(&self, renderer_id: &str);

    /// Gets messaging scoped for a specific editor.
    fn get_scoped(&self, editor_id: &str) -> Rc<dyn IScopedRendererMessaging>;

    /// Called when the main thread gets a message for a renderer.
    fn receive_message(&self, editor_id: Option<&str>, renderer_id: &str, message: Dynamic) -> Promise<bool>;
}

service_id!(pub I_NOTEBOOK_RENDERER_MESSAGING_SERVICE: dyn INotebookRendererMessagingService = "INotebookRendererMessagingService");

/// `IScopedRendererMessaging`.
pub trait IScopedRendererMessaging: IDisposable {
    /// Method called when a message is received. Should return a boolean indicating whether a renderer received it.
    fn receive_message_handler(&self) -> Option<ReceiveMessageHandler>;

    /// Atribui o `receiveMessageHandler`.
    fn set_receive_message_handler(&self, handler: Option<ReceiveMessageHandler>);

    /// Sends a message to an extension from a renderer.
    fn post_message(&self, renderer_id: &str, message: Dynamic);
}
