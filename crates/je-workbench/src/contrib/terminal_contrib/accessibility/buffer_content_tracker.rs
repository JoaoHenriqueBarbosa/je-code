//! Porte de `src/vs/workbench/contrib/terminalContrib/accessibility/browser/bufferContentTracker.ts`: o cache, em linhas de
//! editor, do conteúdo do buffer do terminal (o scrollback guardado e a área visível), que o buffer acessível e a ajuda
//! de acessibilidade leem.
//!
//! Decisões de tradução:
//! - `Pick<IXtermTerminal, 'getFont'> & { raw: Terminal }` é o [`IXtermRaw`] (`raw`): o `getFont` não é lido pela classe.
//!   `Terminal.rows`, `buffer.active.baseY`, `getLine` e `registerMarker` vêm do contrato [`IHeadlessTerminal`] do emulador.
//! - As dependências injetadas (`IConfigurationService` e `ITerminalLogService`) são parâmetros do [`BufferContentTracker::new`].
//!   As mensagens de `debug` do original, que levam os números como argumentos, saem já formatadas.
//! - O estado mutável (`_lastCachedMarker`, `_priorEditorViewportLineCount`, `_lines` e `bufferToEditorLineMapping`) fica num
//!   `RefCell`, e `lines` e `bufferToEditorLineMapping` são métodos que devolvem uma cópia.
//! - `this._register(this._xterm.raw.registerMarker())` registra o marcador no `Disposable` base quando o emulador o devolve
//!   (`registerMarker` pode devolver `undefined`, que o `_register` ignora).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::LogServiceRef;
use je_platform::terminal::capabilities::capabilities::IMarker;
use je_platform::terminal::terminal_settings::TerminalSettingId;
use je_platform::terminal::xterm_headless::{IBuffer, IHeadlessTerminal};

use crate::contrib::terminal::browser::xterm::xterm_terminal::IXtermRaw;

#[derive(Default)]
struct State {
    /// Marks the last part of the buffer that was cached.
    last_cached_marker: Option<Rc<dyn IMarker>>,
    /// The number of wrapped lines in the viewport when the last cached marker was set.
    prior_editor_viewport_line_count: i32,
    lines: Vec<String>,
    buffer_to_editor_line_mapping: HashMap<i32, i32>,
}

/// `BufferContentTracker`.
pub struct BufferContentTracker {
    base: Rc<Disposable>,
    xterm: Rc<dyn IXtermRaw>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: LogServiceRef,
    state: RefCell<State>,
}

impl BufferContentTracker {
    pub fn new(xterm: Rc<dyn IXtermRaw>, configuration_service: Rc<dyn IConfigurationService>, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), xterm, configuration_service, log_service, state: RefCell::new(State::default()) })
    }

    /// `get lines()`.
    pub fn lines(&self) -> Vec<String> {
        self.state.borrow().lines.clone()
    }

    /// `bufferToEditorLineMapping`.
    pub fn buffer_to_editor_line_mapping(&self) -> HashMap<i32, i32> {
        self.state.borrow().buffer_to_editor_line_mapping.clone()
    }

    pub fn reset(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.lines = Vec::new();
            state.last_cached_marker = None;
        }
        self.update();
    }

    pub fn update(&self) {
        {
            let mut state = self.state.borrow_mut();
            if state.last_cached_marker.as_ref().is_some_and(|marker| marker.is_disposed()) {
                // the terminal was cleared, reset the cache
                state.lines = Vec::new();
                state.last_cached_marker = None;
            }
        }
        self.remove_viewport_content();
        self.update_cached_content();
        self.update_viewport_content();
        let marker = self.xterm.register_marker(0);
        if let Some(marker) = &marker {
            let disposable: Rc<dyn IDisposable> = marker.clone();
            self.base.register(disposable);
        }
        let mut state = self.state.borrow_mut();
        state.last_cached_marker = marker;
        self.log_service.debug(&format!("Buffer content tracker: set {} lines", state.lines.len()));
    }

    fn update_cached_content(&self) {
        let buffer = self.xterm.buffer_active();
        let rows = self.xterm.rows();
        let mut state = self.state.borrow_mut();
        let marker_line = state.last_cached_marker.as_ref().map_or(0.0, |marker| marker.line());
        let start = if marker_line != 0.0 && !marker_line.is_nan() { marker_line as i32 - rows + 1 } else { 0 };
        let end = buffer.base_y();
        if start < 0 || start > end {
            // in the viewport, no need to cache
            return;
        }

        // to keep the cache size down, remove any lines that are no longer in the scrollback
        let scrollback = match self.configuration_service.get_value(Some(TerminalSettingId::Scrollback.as_str()), None) {
            je_base::dynamic::Dynamic::Number(scrollback) => scrollback as i32,
            _ => 0,
        };
        let max_buffer_size = scrollback + rows - 1;
        let lines_to_add = end - start;
        if lines_to_add + state.lines.len() as i32 > max_buffer_size {
            let num_to_remove = lines_to_add + state.lines.len() as i32 - max_buffer_size;
            for _ in 0..num_to_remove {
                if !state.lines.is_empty() {
                    state.lines.remove(0);
                }
            }
            self.log_service.debug(&format!(
                "Buffer content tracker: removed {num_to_remove} lines from top of cached lines, now {} lines",
                state.lines.len()
            ));
        }

        // iterate through the buffer lines and add them to the editor line cache
        let mut cached_lines: Vec<String> = Vec::new();
        let mut current_line = String::new();
        for i in start..end {
            let Some(line) = buffer.get_line(i) else {
                continue;
            };
            let mapped_line = (state.lines.len() + cached_lines.len()) as i32;
            state.buffer_to_editor_line_mapping.insert(i, mapped_line);
            let is_wrapped = buffer.get_line(i + 1).is_some_and(|next| next.is_wrapped());
            current_line.push_str(&line.translate_to_string(Some(!is_wrapped), None, None).to_string_lossy());
            if (!current_line.is_empty() && !is_wrapped || i == buffer.base_y() + rows - 1) && line.length() != 0 {
                cached_lines.push(std::mem::take(&mut current_line));
            }
        }
        self.log_service.debug(&format!("Buffer content tracker: {} lines cached", cached_lines.len()));
        state.lines.extend(cached_lines);
    }

    fn remove_viewport_content(&self) {
        let mut state = self.state.borrow_mut();
        if state.lines.is_empty() {
            return;
        }
        // remove previous viewport content in case it has changed
        let mut lines_to_remove = state.prior_editor_viewport_line_count;
        let mut index = 1;
        while lines_to_remove != 0 {
            let removed_line = state.lines.len() as i32 - index;
            state.buffer_to_editor_line_mapping.retain(|_, value| *value != removed_line);
            state.lines.pop();
            index += 1;
            lines_to_remove -= 1;
        }
        self.log_service.debug(&format!("Buffer content tracker: removed lines from viewport, now {} lines cached", state.lines.len()));
    }

    fn update_viewport_content(&self) {
        let buffer = self.xterm.buffer_active();
        let rows = self.xterm.rows();
        let mut state = self.state.borrow_mut();
        state.prior_editor_viewport_line_count = 0;
        let mut current_line = String::new();
        for i in buffer.base_y()..buffer.base_y() + rows {
            let Some(line) = buffer.get_line(i) else {
                continue;
            };
            let mapped_line = state.lines.len() as i32;
            state.buffer_to_editor_line_mapping.insert(i, mapped_line);
            let is_wrapped = buffer.get_line(i + 1).is_some_and(|next| next.is_wrapped());
            current_line.push_str(&line.translate_to_string(Some(!is_wrapped), None, None).to_string_lossy());
            if (!current_line.is_empty() && !is_wrapped || i == buffer.base_y() + rows - 1) && !current_line.is_empty() {
                state.prior_editor_viewport_line_count += 1;
                state.lines.push(std::mem::take(&mut current_line));
            }
        }
        self.log_service.debug(&format!("Viewport content update complete, {} lines in the viewport", state.lines.len()));
    }
}

impl IDisposable for BufferContentTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}
