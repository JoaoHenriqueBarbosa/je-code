//! Porte de `src/vs/workbench/contrib/terminal/browser/widgets/widgets.ts`: o contrato dos widgets do terminal.
//!
//! Divergência: `attach(container: HTMLElement)` recebe o [`ITerminalDocumentEvents`] do contêiner (o frontend nativo é o dono
//! do contêiner; o widget só precisa dos eventos de teclado do documento dele).

use std::rc::Rc;

use je_base::lifecycle::IDisposable;

use crate::contrib::terminal::browser::terminal_input_events::ITerminalDocumentEvents;

/// `ITerminalWidget`.
pub trait ITerminalWidget: IDisposable {
    /// Only one widget of each ID can be displayed at once.
    fn id(&self) -> String;

    fn attach(&self, container: Rc<dyn ITerminalDocumentEvents>);
}
