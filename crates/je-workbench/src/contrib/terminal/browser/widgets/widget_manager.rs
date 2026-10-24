//! Porte de `src/vs/workbench/contrib/terminal/browser/widgets/widgetManager.ts`: o gerenciador dos widgets do terminal
//! (no máximo um por id).
//!
//! Decisões de tradução:
//! - **`attachToElement(terminalWrapper: HTMLElement)`** cria um `div.terminal-widget-container` no invólucro do terminal. O
//!   contêiner é o [`ITerminalDocumentEvents`] que o frontend entrega a [`TerminalWidgetManager::attach_to_element`], e o
//!   `container.remove()` do `dispose` é largar essa referência.
//! - **`attachWidget(widget): IDisposable | undefined`** é [`ITerminalWidgetManager::attach_widget`] (`None` sem contêiner).
//!   O trait existe porque o `TerminalLinkManager` fala com o gerenciador por contrato, e o teste dele troca o gerenciador
//!   por um dublê; o [`TerminalWidgetManager`] é a implementação do original.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable, plain_disposable};

use super::widgets::ITerminalWidget;
use crate::contrib::terminal::browser::terminal_input_events::ITerminalDocumentEvents;

/// O que quem posiciona widgets lê do gerenciador.
pub trait ITerminalWidgetManager {
    /// `attachWidget(widget)`.
    fn attach_widget(&self, widget: Rc<dyn ITerminalWidget>) -> Option<DisposableRef>;
}

/// `TerminalWidgetManager`.
#[derive(Default)]
pub struct TerminalWidgetManager {
    container: RefCell<Option<Rc<dyn ITerminalDocumentEvents>>>,
    attached: Rc<RefCell<HashMap<String, Rc<dyn ITerminalWidget>>>>,
}

impl TerminalWidgetManager {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn attach_to_element(&self, terminal_wrapper: Rc<dyn ITerminalDocumentEvents>) {
        let mut container = self.container.borrow_mut();
        if container.is_none() {
            *container = Some(terminal_wrapper);
        }
    }
}

impl IDisposable for TerminalWidgetManager {
    fn dispose(&self) {
        *self.container.borrow_mut() = None;
        let widgets: Vec<Rc<dyn ITerminalWidget>> = self.attached.borrow().values().cloned().collect();
        for widget in widgets {
            widget.dispose();
        }
        self.attached.borrow_mut().clear();
    }
}

impl ITerminalWidgetManager for TerminalWidgetManager {
    fn attach_widget(&self, widget: Rc<dyn ITerminalWidget>) -> Option<DisposableRef> {
        let container = self.container.borrow().clone()?;
        let previous = self.attached.borrow().get(&widget.id()).cloned();
        if let Some(previous) = previous {
            previous.dispose();
        }
        widget.attach(container);
        self.attached.borrow_mut().insert(widget.id(), widget.clone());
        let attached = self.attached.clone();
        Some(plain_disposable(move || {
            let current = attached.borrow().get(&widget.id()).cloned();
            if current.is_some_and(|current| Rc::ptr_eq(&current, &widget)) {
                attached.borrow_mut().remove(&widget.id());
                widget.dispose();
            }
        }))
    }
}
