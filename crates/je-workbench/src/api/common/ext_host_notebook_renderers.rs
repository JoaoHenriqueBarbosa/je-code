//! Porte de `src/vs/workbench/api/common/extHostNotebookRenderers.ts`: a troca de mensagens entre a extensão e os renderizadores de
//! notebook que ela contribui (`vscode.notebooks.createRendererMessaging`).
//!
//! Divergências:
//! - `ExtHostNotebookRenderers` é um `Rc` ([`ExtHostNotebookRenderers::new`]) que guarda o controlador por `Rc`; o `IMainContext` é
//!   o do porte (dá o `getProxy`).
//! - `vscode.NotebookRendererMessaging` é a struct [`NotebookRendererMessaging`], com `onDidReceiveMessage` e `postMessage`. O
//!   `{ editor, message }` do evento é a [`NotebookRendererMessageEvent`], e `message: unknown` é [`Dynamic`].
//! - **`postMessage(message, editorOrAlias)`.** O original troca os argumentos quando `message` é um editor de notebook (a
//!   compatibilidade com a ordem antiga). Com a assinatura tipada (`Dynamic`, `Option<&NotebookEditor>`) um editor nunca chega
//!   como `message`, então a troca não existe.
//! - **`createRendererMessaging`.** O `throw` é o `Err`. `manifest.contributes?.notebookRenderer?.some(...)` lê
//!   `manifest.contributes.notebook_renderer` (vazio quando não há `contributes`).
//! - **`getOrCreateEmitterFor`.** O `onDidRemoveLastListener` do emitter descarta o emitter e o tira do mapa; o gancho do porte
//!   recebe o próprio emitter. O `onDidReceiveMessage` só cria o emitter quando alguém se inscreve (o `event(listener)` do original
//!   chama `getOrCreateEmitterFor` no ato).
//! - `proxy.$postMessage(extHostEditor?.id, ...)` passa `Option<&str>`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::promise::Promise;
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_notebook::ExtHostNotebookController;
use super::ext_host_notebook_editor::NotebookEditor;
use super::ext_host_protocol::{ExtHostNotebookRenderersShape, IMainContext, MainContext, MainThreadNotebookRenderersShape};

/// O `{ editor: vscode.NotebookEditor; message: unknown }` do evento de mensagem do renderizador.
#[derive(Clone)]
pub struct NotebookRendererMessageEvent {
    pub editor: NotebookEditor,
    pub message: Dynamic,
}

type RendererMessageEmitters = Rc<RefCell<HashMap<String, Rc<Emitter<NotebookRendererMessageEvent>>>>>;

/// `ExtHostNotebookRenderers`.
pub struct ExtHostNotebookRenderers {
    renderer_message_emitters: RendererMessageEmitters,
    proxy: Rc<dyn MainThreadNotebookRenderersShape>,
    ext_host_notebook: Rc<ExtHostNotebookController>,
}

impl ExtHostNotebookRenderers {
    pub fn new(main_context: &dyn IMainContext, ext_host_notebook: Rc<ExtHostNotebookController>) -> Rc<ExtHostNotebookRenderers> {
        Rc::new(ExtHostNotebookRenderers {
            renderer_message_emitters: Rc::new(RefCell::new(HashMap::new())),
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_NOTEBOOK_RENDERERS),
            ext_host_notebook,
        })
    }

    /// `createRendererMessaging(manifest, rendererId)`.
    pub fn create_renderer_messaging(self: &Rc<Self>, manifest: &ExtensionDescription, renderer_id: &str) -> Result<NotebookRendererMessaging, Error> {
        let contributes_renderer =
            manifest.manifest.contributes.as_ref().is_some_and(|contributes| contributes.notebook_renderer.iter().any(|renderer| renderer.id == renderer_id));
        if !contributes_renderer {
            return Err(Error::new(format!("Extensions may only call createRendererMessaging() for renderers they contribute (got {renderer_id})")));
        }

        Ok(NotebookRendererMessaging { renderer_id: renderer_id.to_string(), host: self.clone() })
    }

    /// `getOrCreateEmitterFor(rendererId)`.
    fn get_or_create_emitter_for(&self, renderer_id: &str) -> Rc<Emitter<NotebookRendererMessageEvent>> {
        if let Some(emitter) = self.renderer_message_emitters.borrow().get(renderer_id) {
            return emitter.clone();
        }

        let (emitters, id) = (self.renderer_message_emitters.clone(), renderer_id.to_string());
        let emitter = Emitter::with_options(EmitterOptions {
            on_did_remove_last_listener: Some(Rc::new(move |emitter: &Emitter<NotebookRendererMessageEvent>| {
                let removed = emitters.borrow_mut().remove(&id);
                je_base::lifecycle::IDisposable::dispose(emitter);
                drop(removed);
            })),
            ..EmitterOptions::default()
        });

        self.renderer_message_emitters.borrow_mut().insert(renderer_id.to_string(), emitter.clone());
        emitter
    }
}

impl ExtHostNotebookRenderersShape for ExtHostNotebookRenderers {
    fn rpc_post_renderer_message(&self, editor_id: &str, renderer_id: &str, message: Dynamic) {
        let editor = self.ext_host_notebook.get_editor_by_id(editor_id);
        let emitter = self.renderer_message_emitters.borrow().get(renderer_id).cloned();
        if let Some(emitter) = emitter {
            emitter.fire(NotebookRendererMessageEvent { editor: editor.api_editor(), message });
        }
    }
}

/// `vscode.NotebookRendererMessaging`.
pub struct NotebookRendererMessaging {
    renderer_id: String,
    host: Rc<ExtHostNotebookRenderers>,
}

impl NotebookRendererMessaging {
    /// `onDidReceiveMessage`.
    pub fn on_did_receive_message(&self) -> Event<NotebookRendererMessageEvent> {
        let (host, renderer_id) = (self.host.clone(), self.renderer_id.clone());
        Event::new(move |listener| host.get_or_create_emitter_for(&renderer_id).event().listen_rc(listener))
    }

    /// `postMessage(message, editorOrAlias?)`.
    pub fn post_message(&self, message: Dynamic, editor_or_alias: Option<&NotebookEditor>) -> Promise<bool> {
        let ext_host_editor = editor_or_alias.map(|editor| editor.ext_host().clone());
        self.host.proxy.rpc_post_message(ext_host_editor.as_ref().map(|editor| editor.id.as_str()), &self.renderer_id, message)
    }
}
