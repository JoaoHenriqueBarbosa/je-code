//! Um serviço de hover de teste: grava o que os widgets pedem ao `getBaseLayerHoverDelegate()` (o serviço de
//! verdade é da camada de plataforma).
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable, none, to_disposable};
use je_ui::NodeId;
use je_widgets::hover_api::{
    HoverContent, HoverTarget, IDelayedHoverOptions, IHoverDelegate, IHoverDelegate2, IHoverLifecycleOptions, IHoverOptions, IHoverWidget, IManagedHover, IManagedHoverOptions, ManagedHoverContent,
    ManagedHoverContentOrFactory, set_base_layer_hover_delegate,
};

/// O texto de um conteúdo gerenciado.
pub fn content_text(content: &ManagedHoverContentOrFactory) -> Option<String> {
    let content = match content {
        ManagedHoverContentOrFactory::Content(content) => content.clone(),
        ManagedHoverContentOrFactory::Factory(f) => f(),
    };
    match content {
        ManagedHoverContent::Text(text) => Some(text),
        _ => None,
    }
}

/// O último `setup_delayed_hover_at_mouse`: o nó e a função de opções.
pub type DelayedHover = (NodeId, Rc<dyn Fn() -> IDelayedHoverOptions>);

#[derive(Default)]
pub struct HoverLog {
    /// `setup_managed_hover`, `update` e `dispose`, na ordem: `managed:<nó>:<texto>`, `update:<texto>`, `dispose`.
    pub events: RefCell<Vec<String>>,
    /// O último `setup_delayed_hover_at_mouse`: o nó e a função de opções.
    pub delayed: RefCell<Option<DelayedHover>>,
    pub delayed_disposed: RefCell<u32>,
}

struct RecordingManagedHover(Rc<HoverLog>);

impl IDisposable for RecordingManagedHover {
    fn dispose(&self) {
        self.0.events.borrow_mut().push("dispose".to_owned());
    }
}

impl IManagedHover for RecordingManagedHover {
    fn show(&self, _focus: bool) {}

    fn hide(&self) {}

    fn update(&self, tooltip: ManagedHoverContentOrFactory, _options: Option<IManagedHoverOptions>) {
        self.0.events.borrow_mut().push(format!("update:{}", content_text(&tooltip).unwrap_or_else(|| "<none>".to_owned())));
    }
}

struct RecordingHover(Rc<HoverLog>);

impl IHoverDelegate2 for RecordingHover {
    fn show_delayed_hover(&self, _: IDelayedHoverOptions, _: HoverTarget, _: IHoverLifecycleOptions) -> Option<Rc<dyn IHoverWidget>> {
        None
    }

    fn setup_delayed_hover(&self, _: NodeId, _: Rc<dyn Fn() -> IDelayedHoverOptions>, _: Option<IHoverLifecycleOptions>) -> DisposableRef {
        none()
    }

    fn setup_delayed_hover_at_mouse(&self, target: NodeId, options: Rc<dyn Fn() -> IDelayedHoverOptions>, _: Option<IHoverLifecycleOptions>) -> DisposableRef {
        *self.0.delayed.borrow_mut() = Some((target, options));
        let log = self.0.clone();
        to_disposable(move || *log.delayed_disposed.borrow_mut() += 1)
    }

    fn show_instant_hover(&self, _: IHoverOptions, _: bool) -> Option<Rc<dyn IHoverWidget>> {
        None
    }

    fn hide_hover(&self, _: bool) {}

    fn show_and_focus_last_hover(&self) {}

    fn setup_managed_hover(&self, _delegate: Rc<dyn IHoverDelegate>, target: NodeId, content: ManagedHoverContentOrFactory, _: Option<IManagedHoverOptions>) -> Rc<dyn IManagedHover> {
        self.0.events.borrow_mut().push(format!("managed:{}:{}", node_key(target), content_text(&content).unwrap_or_else(|| "<none>".to_owned())));
        Rc::new(RecordingManagedHover(self.0.clone()))
    }

    fn show_managed_hover(&self, _: NodeId) {}
}

/// Um identificador estável do nó para as mensagens do log (o `NodeId` não tem `Display`).
pub fn node_key(node: NodeId) -> String {
    format!("{node:?}")
}

/// Instala o serviço de hover de teste neste thread e devolve o log.
pub fn install() -> Rc<HoverLog> {
    let log = Rc::new(HoverLog::default());
    set_base_layer_hover_delegate(Rc::new(RecordingHover(log.clone())));
    log
}

/// O texto de um `HoverContent`.
pub fn hover_text(content: &HoverContent) -> String {
    match content {
        HoverContent::Text(text) => text.clone(),
        other => format!("{other:?}"),
    }
}
