//! Porte de `src/vs/workbench/contrib/terminalContrib/clipboard/browser/terminal.clipboard.contribution.ts`: a contribuição de
//! área de transferência do terminal (copiar a seleção, colar, colar a seleção primária, o clique do meio e o clique secundário).
//!
//! Decisões de tradução:
//! - **`extends Disposable`** é o campo `base`; as dependências injetadas são parâmetros do [`TerminalClipboardContribution::new`], e
//!   o `instantiationService.invokeFunction(shouldPasteTerminalText, ...)` é a chamada direta com o `IConfigurationService` e o
//!   `IDialogService`.
//! - **`BrowserFeatures.clipboard.readText`** (e `writeText`) valem `true`: o je-code é um aplicativo nativo e a área de
//!   transferência não depende de permissão do navegador. Por isso o aviso "This browser doesn't support the clipboard.readText
//!   API" do clique secundário nunca é mostrado, e o `INotificationService` (que só esse aviso lê) não entra no construtor.
//! - **`MouseEvent`** é o [`TerminalMouseEvent`]; `handleMouseEvent` devolve `MaybePromise<Option<bool>>` (o `handled`).
//! - **`readText('selection')`** é o `IClipboardService::read_text_of_type`.
//! - **`setTimeout(() => clearSelection(), 0)`** (só no macOS) é o `set_timeout` do `runtime` do porte.
//! - **`overrideCopyOnSelection`** devolve um `Result` (o `throw` do original) com o `DisposableRef` do `toDisposable`.
//! - **`static get(instance)`** (`instance.getContribution`) não existe: o `ITerminalInstance` do porte não guarda contribuições.
//! - **Fora do porte: as ações** (`registerActiveInstanceAction` e `registerActiveXtermAction` de copiar o último comando e a
//!   saída, copiar a seleção e colar), que dependem do registro de ações do terminal (`terminalActions.ts`) e das chaves de
//!   contexto, sem contrato no porte.

use je_base::dynamic::Truthy;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::network::schemas;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::IDialogService;
use je_platform::resources::fs_path;
use je_platform::terminal::capabilities::capabilities::ITerminalCommand;
use je_platform::terminal::terminal_settings::TerminalSettingId;

use super::terminal_clipboard::{ShouldPasteTerminalText, should_paste_terminal_text};
use crate::contrib::terminal::browser::terminal::ITerminalConfigurationService;
use crate::contrib::terminal::browser::terminal_extensions::{ITerminalContribution, IXtermTerminal, TerminalContributionContext, register_terminal_contribution};
use crate::contrib::terminal::browser::terminal_input_events::TerminalMouseEvent;

/// `TerminalClipboardContribution`.
pub struct TerminalClipboardContribution {
    base: Rc<Disposable>,
    this: Weak<TerminalClipboardContribution>,
    ctx: TerminalContributionContext,
    clipboard_service: Rc<dyn IClipboardService>,
    configuration_service: Rc<dyn IConfigurationService>,
    dialog_service: Rc<dyn IDialogService>,
    terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,

    xterm: RefCell<Option<Rc<dyn IXtermTerminal>>>,
    override_copy_selection: Cell<Option<bool>>,
    on_will_paste: Rc<Emitter<String>>,
    on_did_paste: Rc<Emitter<String>>,
}

impl TerminalClipboardContribution {
    pub const ID: &'static str = "terminal.clipboard";

    pub fn new(
        ctx: TerminalContributionContext,
        clipboard_service: Rc<dyn IClipboardService>,
        configuration_service: Rc<dyn IConfigurationService>,
        dialog_service: Rc<dyn IDialogService>,
        terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_will_paste = Emitter::new();
        let on_did_paste = Emitter::new();
        base.register(on_will_paste.clone());
        base.register(on_did_paste.clone());
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            ctx,
            clipboard_service,
            configuration_service,
            dialog_service,
            terminal_configuration_service,
            xterm: RefCell::new(None),
            override_copy_selection: Cell::new(None),
            on_will_paste,
            on_did_paste,
        })
    }

    /// `onWillPaste`.
    pub fn on_will_paste(&self) -> Event<String> {
        self.on_will_paste.event()
    }

    /// `onDidPaste`.
    pub fn on_did_paste(&self) -> Event<String> {
        self.on_did_paste.event()
    }

    /// `copySelection(asHtml?, command?)`.
    pub fn copy_selection(&self, as_html: bool, command: Option<Rc<dyn ITerminalCommand>>) {
        // Nota do original: confirmar que não esperar a promessa do xterm é aceitável.
        let xterm = self.xterm.borrow().clone();
        if let Some(xterm) = xterm {
            xterm.copy_selection(as_html, command);
        }
    }

    /// Focuses and pastes the contents of the clipboard into the terminal instance.
    pub fn paste(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("a contribuição está viva");
        async_fn(async move {
            let mut text = this.clipboard_service.read_text().await?;

            if text.is_empty() {
                let resources = this.clipboard_service.read_resources().await?;
                if let Some(resource) = resources.first()
                    && *resource.scheme() == schemas::FILE
                {
                    text = fs_path(resource);
                }
            }

            this.paste_text(text).await
        })
    }

    /// Focuses and pastes the contents of the selection clipboard into the terminal instance.
    pub fn paste_selection(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("a contribuição está viva");
        async_fn(async move {
            let text = this.clipboard_service.read_text_of_type("selection").await?;
            this.paste_text(text).await
        })
    }

    fn paste_text(self: &Rc<Self>, value: String) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let Some(xterm) = this.xterm.borrow().clone() else {
                return Ok(());
            };

            let mut current_text = value;
            let should_paste_text = should_paste_terminal_text(
                this.configuration_service.clone(),
                this.dialog_service.clone(),
                &current_text,
                Some(xterm.raw().bracketed_paste_mode()),
            )
            .await?;
            match should_paste_text {
                ShouldPasteTerminalText::Paste(false) => return Ok(()),
                ShouldPasteTerminalText::Paste(true) => {}
                ShouldPasteTerminalText::ModifiedText(modified_text) => current_text = modified_text,
            }

            this.ctx.instance.focus();

            this.on_will_paste.fire(current_text.clone());
            xterm.raw().paste(&current_text);
            this.on_did_paste.fire(current_text);
            Ok(())
        })
    }

    /// Override the copy on selection feature with a custom value.
    pub fn override_copy_on_selection(&self, value: bool) -> Result<DisposableRef, Error> {
        if self.override_copy_selection.get().is_some() {
            return Err(Error::new("Cannot set a copy on selection override multiple times"));
        }
        self.override_copy_selection.set(Some(value));
        let this = self.this.clone();
        Ok(to_disposable(move || {
            if let Some(this) = this.upgrade() {
                this.override_copy_selection.set(None);
            }
        }))
    }
}

impl IDisposable for TerminalClipboardContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITerminalContribution for TerminalClipboardContribution {
    fn xterm_ready(&self, xterm: Rc<dyn IXtermTerminal>) {
        *self.xterm.borrow_mut() = Some(xterm.clone());
        // Nota do original: deveria ser outro evento do xterm; copiar como HTML não deveria dividir o evento de pedir para executar um comando.
        let this = self.this.clone();
        self.base.register(xterm.on_did_request_copy_as_html().listen(move |command| {
            if let Some(this) = this.upgrade() {
                this.copy_selection(true, Some(command.clone()));
            }
        }));
        let this = self.this.clone();
        self.base.register(xterm.raw().on_selection_change().listen(move |_| {
            let Some(this) = this.upgrade() else { return };
            if this.configuration_service.get_value(Some(TerminalSettingId::CopyOnSelection.as_str()), None).is_truthy() {
                if this.override_copy_selection.get() == Some(false) {
                    return;
                }
                if this.ctx.instance.has_selection() {
                    this.copy_selection(false, None);
                }
            }
        }));
    }

    fn handle_mouse_event(&self, event: &TerminalMouseEvent) -> MaybePromise<Option<bool>> {
        let this = self.this.upgrade().expect("a contribuição está viva");
        match event.button {
            1 => {
                // Middle click
                if this.terminal_configuration_service.config().get_str("middleClickBehavior").strict_equals(&je_base::dynamic::Dynamic::string("paste")) {
                    let _ = this.paste();
                    return MaybePromise::Value(Some(true));
                }
            }
            2 => {
                // Right click
                // Ignore shift click as it forces the context menu
                if event.shift_key {
                    return MaybePromise::Value(None);
                }
                let right_click_behavior = this.terminal_configuration_service.config().get_str("rightClickBehavior");
                let copy_paste = right_click_behavior.strict_equals(&je_base::dynamic::Dynamic::string("copyPaste"));
                if !copy_paste && !right_click_behavior.strict_equals(&je_base::dynamic::Dynamic::string("paste")) {
                    return MaybePromise::Value(None);
                }
                if copy_paste && this.ctx.instance.has_selection() {
                    this.copy_selection(false, None);
                    this.ctx.instance.clear_selection();
                } else {
                    // `BrowserFeatures.clipboard.readText` vale `true` (ver o cabeçalho do módulo).
                    let _ = this.paste();
                }
                // Clear selection after all click event bubbling is finished on Mac to prevent
                // right-click selecting a word which is seemed cannot be disabled. There is a
                // flicker when pasting but this appears to give the best experience if the
                // setting is enabled.
                if Platform::detect().is_macintosh {
                    let instance = this.ctx.instance.clone();
                    je_base::runtime::set_timeout(move || instance.clear_selection(), 0);
                }
                return MaybePromise::Value(Some(true));
            }
            _ => {}
        }
        MaybePromise::Value(None)
    }
}

/// `registerTerminalContribution(TerminalClipboardContribution.ID, TerminalClipboardContribution, false)`: quem monta os
/// serviços chama esta função com o que o construtor lê.
pub fn register_terminal_clipboard_contribution(
    clipboard_service: Rc<dyn IClipboardService>,
    configuration_service: Rc<dyn IConfigurationService>,
    dialog_service: Rc<dyn IDialogService>,
    terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
) {
    register_terminal_contribution(
        TerminalClipboardContribution::ID,
        Rc::new(move |ctx| {
            TerminalClipboardContribution::new(
                ctx,
                clipboard_service.clone(),
                configuration_service.clone(),
                dialog_service.clone(),
                terminal_configuration_service.clone(),
            ) as Rc<dyn ITerminalContribution>
        }),
        false,
    );
}
