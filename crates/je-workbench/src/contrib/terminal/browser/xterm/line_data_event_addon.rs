//! Porte de `src/vs/workbench/contrib/terminal/browser/xterm/lineDataEventAddon.ts`: o addon do xterm que dispara `onLineData`
//! com o texto de cada linha terminada por avanço de linha (e, no Windows, a cada movimento do cursor).
//!
//! Decisões de tradução:
//! - **`ITerminalAddon` e `loadAddon`.** O addon recebe o emulador em [`LineDataEventAddon::activate`], que é o `activate(xterm)`
//!   do original. O emulador é o [`ILineFeedTerminal`]: o contrato sem tela da plataforma ([`IHeadlessTerminal`], que já traz o
//!   buffer e o `parser`) com o `onLineFeed`. O `terminal.dispose()` do xterm descarta os addons carregados; aqui quem criou o
//!   addon o descarta.
//! - **`activate` assíncrono.** O `await this._initializationPromise` de uma promessa ausente só cede um microtask, e sem ela os
//!   ouvintes entram na hora; com ela, entram quando a promessa resolve.
//! - **`const buffer = xterm.buffer`.** O original instancia o namespace do buffer antes de o terminal ser descartado e depois lê
//!   `buffer.active`; aqui o `Rc<dyn IBuffer>` de `buffer_active()` é pego no `activate`, na mesma linha, e é o que os
//!   ouvintes leem.
//! - **`registerCsiHandler({ final: 'H' }, ...)`.** É `register_csi_handler('H', ...)`; o tratador devolve `false` e o xterm
//!   continua com o movimento do cursor. O terminal que ele captura é fraco, porque o `parser` do terminal é quem guarda o
//!   tratador.
//! - **`string`.** O texto das linhas vira `String` (a conversão perde apenas substitutos soltos de UTF-16).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_platform::terminal::xterm_headless::{CsiHandler, IBuffer, IHeadlessTerminal};

/// O `Terminal` do xterm no que o [`LineDataEventAddon`] lê: o emulador sem tela e o `onLineFeed`.
pub trait ILineFeedTerminal: IHeadlessTerminal {
    /// `terminal.onLineFeed`: dispara depois de cada avanço de linha (`LF`) interpretado.
    fn on_line_feed(&self) -> Event<()>;
}

/// Provides extensions to the xterm object in a modular, testable way.
pub struct LineDataEventAddon {
    base: Rc<Disposable>,
    xterm: RefCell<Option<Rc<dyn ILineFeedTerminal>>>,
    is_os_set: Cell<bool>,
    on_line_data: Rc<Emitter<String>>,
    initialization_promise: Option<Promise<()>>,
}

impl LineDataEventAddon {
    pub fn new(initialization_promise: Option<Promise<()>>) -> Rc<Self> {
        let base = Disposable::new();
        let on_line_data: Rc<Emitter<String>> = Emitter::new();
        base.register(on_line_data.clone());
        Rc::new(Self { base, xterm: RefCell::new(None), is_os_set: Cell::new(false), on_line_data, initialization_promise })
    }

    /// `onLineData`.
    pub fn on_line_data(&self) -> Event<String> {
        self.on_line_data.event()
    }

    /// `activate(xterm)`.
    pub fn activate(self: &Rc<Self>, xterm: Rc<dyn ILineFeedTerminal>) {
        *self.xterm.borrow_mut() = Some(xterm.clone());

        // IMPORTANT: Instantiate the buffer namespace object here before it's disposed.
        let buffer = xterm.buffer_active();

        // If there is an initialization promise, wait for it before registering the event
        match self.initialization_promise.clone() {
            Some(promise) => {
                let weak = Rc::downgrade(self);
                let _ = promise.then(move |()| {
                    if let Some(this) = weak.upgrade() {
                        this.register_line_listeners(&*xterm, buffer);
                    }
                });
            }
            None => self.register_line_listeners(&*xterm, buffer),
        }
    }

    /// O que o `activate` registra depois da promessa de inicialização.
    fn register_line_listeners(self: &Rc<Self>, xterm: &dyn ILineFeedTerminal, buffer: Rc<dyn IBuffer>) {
        // Fire onLineData when a line feed occurs, taking into account wrapped lines
        let weak = Rc::downgrade(self);
        let line_feed_buffer = buffer.clone();
        self.base.register(xterm.on_line_feed().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let line_index = line_feed_buffer.base_y() + line_feed_buffer.cursor_y();
            let new_line = line_feed_buffer.get_line(line_index);
            if new_line.is_some_and(|new_line| !new_line.is_wrapped()) {
                this.send_line_data(&*line_feed_buffer, line_index - 1);
            }
        }));

        // Fire onLineData when disposing object to flush last line
        let weak = Rc::downgrade(self);
        self.base.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.send_line_data(&*buffer, buffer.base_y() + buffer.cursor_y());
            }
        }));
    }

    /// `setOperatingSystem(os)`.
    pub fn set_operating_system(self: &Rc<Self>, os: OperatingSystem) {
        let Some(xterm) = self.xterm.borrow().clone() else { return };
        if self.is_os_set.get() {
            return;
        }
        self.is_os_set.set(true);

        // Force line data to be sent when the cursor is moved, the main purpose for
        // this is because ConPTY will often not do a line feed but instead move the
        // cursor, in which case we still want to send the current line's data to tasks.
        if os == OperatingSystem::Windows {
            let weak_this = Rc::downgrade(self);
            let weak_xterm: Weak<dyn ILineFeedTerminal> = Rc::downgrade(&xterm);
            let handler: CsiHandler = Rc::new(move |_| {
                if let (Some(this), Some(xterm)) = (weak_this.upgrade(), weak_xterm.upgrade()) {
                    let buffer = xterm.buffer_active();
                    this.send_line_data(&*buffer, buffer.base_y() + buffer.cursor_y());
                }
                false
            });
            self.base.register(xterm.parser().register_csi_handler('H', handler));
        }
    }

    fn send_line_data(&self, buffer: &dyn IBuffer, line_index: i32) {
        let mut line_index = line_index;
        let Some(mut line) = buffer.get_line(line_index) else { return };
        let mut line_data = line.translate_to_string(Some(true), None, None).to_string_lossy();
        while line_index > 0 && line.is_wrapped() {
            line_index -= 1;
            let Some(previous) = buffer.get_line(line_index) else { break };
            line = previous;
            line_data = line.translate_to_string(Some(false), None, None).to_string_lossy() + &line_data;
        }
        self.on_line_data.fire(line_data);
    }
}

impl IDisposable for LineDataEventAddon {
    fn dispose(&self) {
        self.base.dispose();
    }
}
