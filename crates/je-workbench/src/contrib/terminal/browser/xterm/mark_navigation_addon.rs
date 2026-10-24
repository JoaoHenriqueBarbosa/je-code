//! Porte de `src/vs/workbench/contrib/terminal/browser/xterm/markNavigationAddon.ts`: o addon do xterm que navega entre as
//! marcas do buffer (comandos, marcas de tarefas) e seleciona linhas, com as decorações temporárias do destino.
//!
//! Decisões de tradução:
//! - **`ITerminalAddon`.** O addon recebe o emulador em [`MarkNavigationAddon::activate`]; o emulador é o
//!   [`IMarkNavigationTerminal`]: o contrato sem tela da plataforma ([`IHeadlessTerminal`], com buffer, marcadores, `rows`,
//!   `cols` e `onData`) mais `registerDecoration`, `viewportY`, a rolagem (`scrollToTop`, `scrollToBottom`, `scrollToLine`) e
//!   `selectLines`.
//! - **Injeção.** Os parâmetros decorados do construtor (`IConfigurationService` e `IThemeService`) entram em
//!   [`MarkNavigationAddon::new`], depois de `capabilities`. O contêiner de injeção do original só os entregava.
//! - **`IMarker | Boundary`.** É [`MarkerOrBoundary`] (`Top` e `Bottom` são o enum `Boundary` do original); `IMarker | number`
//!   é [`MarkerOrLine`]. A identidade dos marcadores (`includes`, `indexOf`) é a identidade do `Rc`.
//! - **Erros.** Os `throw new Error(...)` (marcador que o emulador não consegue registrar) são o `Err` de [`Error`], e todo
//!   método que pode chegar neles devolve `Result<(), Error>`.
//! - **`ITerminalCommand | ICurrentPartialCommand`.** É [`TerminalCommandForLine`]. O `isFullTerminalCommand(command) ?
//!   command.marker : command.commandStartMarker` olha o comando completo (`marker`) ou o parcial (`commandStartMarker`), e o
//!   comando que o `onCommandExecuted` dispara como se fosse completo (o `HACK` da capacidade) é lido como parcial.
//! - **`IMarkTracker`.** É o trait [`IMarkTracker`] (de `terminal.ts`), definido aqui porque só este addon o implementa. O
//!   `revealCommand` dele aceita também uma `URI` no original; o addon nunca a trata, e a variante não entra no trait. Os
//!   métodos do trait que o original declara com menos parâmetros do que a classe (`scrollToNextMark()`) chamam o método da
//!   classe com os padrões.
//! - **Padrões dos parâmetros opcionais.** `scrollPosition = Middle`, `retainSelection = false` e `skipEmptyCommands = true`
//!   valem para o `None`.
//! - **`_getMarkers(...)[index]`.** O índice fora da lista (`undefined` no JavaScript, que depois lançaria um `TypeError`) faz
//!   o método retornar sem navegar.
//! - **`timeout(350).then(...)`.** É o `timeout` do `async`, e a promessa não é guardada, como no original.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::errors::Error;
use je_base::dynamic::Truthy;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable, dispose_all};
use je_editor::services::theme_service::IThemeService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::terminal::capabilities::capabilities::{
    BufferMarkDetection, CommandDetection, ICurrentPartialCommand, IMarker, ITerminalCapabilityStore, ITerminalCommand, PartialCommandDetection,
    TerminalCapability, TerminalCommandForLine,
};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;

use super::xterm_decoration::{IDecorationHost, IDecorationOptions, IDecorationOverviewRulerOptions, IXtermDecoration};
use crate::contrib::terminal::common::terminal_color_registry::TERMINAL_OVERVIEW_RULER_CURSOR_FOREGROUND_COLOR;
use crate::contrib::terminal::xterm_buffer::IBufferRange;

/// `TerminalContribSettingId.StickyScrollEnabled` (`TerminalStickyScrollSettingId.Enabled`).
const STICKY_SCROLL_ENABLED: &str = "terminal.integrated.stickyScroll.enabled";

/// `ScrollPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollPosition {
    Top,
    Middle,
}

/// `IMarker | Boundary`: o marcador atual da navegação, ou o topo ou o fim do buffer (`Boundary.Top` e `Boundary.Bottom`).
#[derive(Clone)]
pub enum MarkerOrBoundary {
    Marker(Rc<dyn IMarker>),
    Top,
    Bottom,
}

/// `IMarker | number`: um marcador ou o índice de uma linha.
#[derive(Clone)]
pub enum MarkerOrLine {
    Marker(Rc<dyn IMarker>),
    Line(i32),
}

impl From<Rc<dyn IMarker>> for MarkerOrLine {
    fn from(marker: Rc<dyn IMarker>) -> Self {
        Self::Marker(marker)
    }
}

impl From<i32> for MarkerOrLine {
    fn from(line: i32) -> Self {
        Self::Line(line)
    }
}

/// `IScrollToMarkerOptions`.
#[derive(Clone, Default)]
pub struct IScrollToMarkerOptions {
    pub hide_decoration: bool,
    /// Scroll even if the line is within the viewport
    pub force_scroll: bool,
    pub buffer_range: Option<IBufferRange>,
}

/// O `Terminal` do xterm no que o [`MarkNavigationAddon`] lê e escreve.
pub trait IMarkNavigationTerminal: IDecorationHost {
    /// `terminal.buffer.active.viewportY`: a linha absoluta do topo da área visível.
    fn viewport_y(&self) -> i32;

    /// `terminal.scrollToTop()`.
    fn scroll_to_top(&self);

    /// `terminal.scrollToBottom()`.
    fn scroll_to_bottom(&self);

    /// `terminal.scrollToLine(line)`.
    fn scroll_to_line(&self, line: i32);

    /// `terminal.selectLines(start, end)`.
    fn select_lines(&self, start: i32, end: i32);
}

/// `IMarkTracker` (`terminal.ts`).
pub trait IMarkTracker {
    fn scroll_to_previous_mark(&self, scroll_position: Option<ScrollPosition>, retain_selection: Option<bool>, skip_empty_commands: Option<bool>) -> Result<(), Error>;
    fn scroll_to_next_mark(&self) -> Result<(), Error>;
    fn select_to_previous_mark(&self) -> Result<(), Error>;
    fn select_to_next_mark(&self) -> Result<(), Error>;
    fn select_to_previous_line(&self) -> Result<(), Error>;
    fn select_to_next_line(&self) -> Result<(), Error>;
    fn clear(&self);
    fn scroll_to_closest_marker(&self, start_marker_id: &str, end_marker_id: Option<&str>, highlight: Option<bool>) -> Result<(), Error>;

    fn scroll_to_line(&self, line: i32, position: ScrollPosition);
    fn reveal_command(&self, command: &TerminalCommandForLine, position: Option<ScrollPosition>) -> Result<(), Error>;
    fn reveal_range(&self, range: &IBufferRange) -> Result<(), Error>;
    fn register_temporary_decoration(&self, marker: Rc<dyn IMarker>, end_marker: Option<Rc<dyn IMarker>>, show_outline: bool) -> Result<(), Error>;
    fn show_command_guide(&self, command: Option<Rc<dyn ITerminalCommand>>) -> Result<(), Error>;

    fn save_scroll_state(&self);
    fn restore_scroll_state(&self);
}

/// A identidade de dois marcadores.
fn same_marker(a: &Rc<dyn IMarker>, b: &Rc<dyn IMarker>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// O endereço de uma decoração, para o `d !== decoration` dos filtros (sem guardar a decoração no ouvinte dela mesma).
fn decoration_address(decoration: &Rc<dyn IXtermDecoration>) -> usize {
    Rc::as_ptr(decoration).cast::<()>() as usize
}

pub struct MarkNavigationAddon {
    base: Rc<Disposable>,
    this: Weak<Self>,
    capabilities: Rc<dyn ITerminalCapabilityStore>,
    configuration_service: Rc<dyn IConfigurationService>,
    theme_service: Rc<dyn IThemeService>,

    current_marker: RefCell<MarkerOrBoundary>,
    selection_start: RefCell<Option<MarkerOrBoundary>>,
    is_disposable: Cell<bool>,
    terminal: RefCell<Option<Rc<dyn IMarkNavigationTerminal>>>,
    navigation_decorations: RefCell<Option<Vec<Rc<dyn IXtermDecoration>>>>,

    active_command_guide: RefCell<Option<Rc<dyn ITerminalCommand>>>,
    command_guide_decorations: Rc<MutableDisposable<DisposableStore>>,
    scroll_state: Cell<Option<i32>>,
}

impl MarkNavigationAddon {
    pub fn new(capabilities: Rc<dyn ITerminalCapabilityStore>, configuration_service: Rc<dyn IConfigurationService>, theme_service: Rc<dyn IThemeService>) -> Rc<Self> {
        let base = Disposable::new();
        let command_guide_decorations: Rc<MutableDisposable<DisposableStore>> = MutableDisposable::new();
        base.register(command_guide_decorations.clone());
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            capabilities,
            configuration_service,
            theme_service,
            current_marker: RefCell::new(MarkerOrBoundary::Bottom),
            selection_start: RefCell::new(None),
            is_disposable: Cell::new(false),
            terminal: RefCell::new(None),
            navigation_decorations: RefCell::new(None),
            active_command_guide: RefCell::new(None),
            command_guide_decorations,
            scroll_state: Cell::new(None),
        })
    }

    /// `activate(terminal)`.
    pub fn activate(&self, terminal: Rc<dyn IMarkNavigationTerminal>) {
        *self.terminal.borrow_mut() = Some(terminal.clone());
        let weak = self.this.clone();
        self.base.register(terminal.on_data().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                *this.current_marker.borrow_mut() = MarkerOrBoundary::Bottom;
            }
        }));
    }

    fn terminal(&self) -> Option<Rc<dyn IMarkNavigationTerminal>> {
        self.terminal.borrow().clone()
    }

    fn current_marker(&self) -> MarkerOrBoundary {
        self.current_marker.borrow().clone()
    }

    fn get_markers(&self, skip_empty_commands: bool) -> Vec<Rc<dyn IMarker>> {
        let command_capability = self.capabilities.get::<CommandDetection>();
        let partial_command_capability = self.capabilities.get::<PartialCommandDetection>();
        let mark_capability = self.capabilities.get::<BufferMarkDetection>();
        let mut markers: Vec<Rc<dyn IMarker>> = Vec::new();
        if let Some(command_capability) = command_capability {
            markers = command_capability
                .commands()
                .into_iter()
                .filter(|e| !skip_empty_commands || e.exit_code().is_some())
                .filter_map(|e| e.prompt_start_marker().or_else(|| e.marker()))
                .collect();
            // Allow navigating to the current command iff it has been executed, this ignores the
            // skipEmptyCommands flag intenionally as chances are it's not going to be empty if an
            // executed marker exists when this is requested.
            if let Some(current_command) = command_capability.current_command()
                && let Some(prompt_start_marker) = current_command.prompt_start_marker()
                && current_command.command_executed_marker().is_some()
            {
                markers.push(prompt_start_marker);
            }
        } else if let Some(partial_command_capability) = partial_command_capability {
            markers.extend(partial_command_capability.commands());
        }

        if let Some(mark_capability) = mark_capability
            && !skip_empty_commands
        {
            markers = mark_capability.markers().collect();
        }
        markers
    }

    fn find_command(&self, marker: &dyn IMarker) -> Option<TerminalCommandForLine> {
        let command_capability = self.capabilities.get::<CommandDetection>()?;
        let line = marker.line();
        let command = command_capability
            .commands()
            .into_iter()
            .find(|e| e.marker().is_some_and(|m| m.line() == line) || e.prompt_start_marker().is_some_and(|m| m.line() == line));
        if let Some(command) = command {
            return Some(TerminalCommandForLine::Command(command));
        }
        command_capability.current_command().map(TerminalCommandForLine::Partial)
    }

    pub fn clear(&self) {
        // Clear the current marker so successive focus/selection actions are performed from the
        // bottom of the buffer
        *self.current_marker.borrow_mut() = MarkerOrBoundary::Bottom;
        self.reset_navigation_decorations();
        *self.selection_start.borrow_mut() = None;
    }

    fn reset_navigation_decorations(&self) {
        let previous = self.navigation_decorations.borrow_mut().replace(Vec::new());
        if let Some(previous) = previous {
            dispose_all(previous.iter());
        }
    }

    fn is_empty_command(&self, marker: &MarkerOrBoundary) -> bool {
        match marker {
            MarkerOrBoundary::Bottom => true,
            MarkerOrBoundary::Top => !self.get_markers(true).iter().any(|e| e.line() == 0.0),
            MarkerOrBoundary::Marker(marker) => !self.get_markers(true).iter().any(|e| same_marker(e, marker)),
        }
    }

    /// `_getMarkers(skip)[index]`, com o índice possivelmente negativo ou além do fim.
    fn marker_at(&self, skip_empty_commands: bool, index: i64) -> Option<Rc<dyn IMarker>> {
        usize::try_from(index).ok().and_then(|index| self.get_markers(skip_empty_commands).get(index).cloned())
    }

    pub fn scroll_to_previous_mark(&self, scroll_position: Option<ScrollPosition>, retain_selection: Option<bool>, skip_empty_commands: Option<bool>) -> Result<(), Error> {
        let scroll_position = scroll_position.unwrap_or(ScrollPosition::Middle);
        let skip_empty_commands = skip_empty_commands.unwrap_or(true);
        let Some(terminal) = self.terminal() else { return Ok(()) };
        if !retain_selection.unwrap_or(false) {
            *self.selection_start.borrow_mut() = None;
        }

        let current_marker = self.current_marker();
        let current_line_y = match &current_marker {
            MarkerOrBoundary::Marker(marker) => self.get_target_scroll_line(marker.line() as i32, scroll_position),
            boundary => get_line(&*terminal, boundary).min(terminal.buffer_active().base_y()),
        };
        let viewport_y = terminal.viewport_y();
        let marker_index: i64;
        let scrolled = match &current_marker {
            MarkerOrBoundary::Marker(marker) => !self.is_marker_in_viewport(&*terminal, &MarkerOrLine::Marker(marker.clone())),
            _ => current_line_y != viewport_y,
        };
        if scrolled {
            // The user has scrolled, find the line based on the current scroll position. This only
            // works when not retaining selection
            let markers_below_viewport = self.get_markers(skip_empty_commands).iter().filter(|e| e.line() >= f64::from(viewport_y)).count() as i64;
            // -1 will scroll to the top
            marker_index = self.get_markers(skip_empty_commands).len() as i64 - markers_below_viewport - 1;
        } else if matches!(current_marker, MarkerOrBoundary::Bottom) {
            marker_index = self.get_markers(skip_empty_commands).len() as i64 - 1;
        } else if matches!(current_marker, MarkerOrBoundary::Top) {
            marker_index = -1;
        } else if self.is_disposable.get() {
            marker_index = self.find_previous_marker(skip_empty_commands);
            if let MarkerOrBoundary::Marker(marker) = &current_marker {
                marker.dispose();
            }
            self.is_disposable.set(false);
        } else if skip_empty_commands && self.is_empty_command(&current_marker) {
            marker_index = self.find_previous_marker(true);
        } else {
            let MarkerOrBoundary::Marker(marker) = &current_marker else { return Ok(()) };
            marker_index = self.get_markers(skip_empty_commands).iter().position(|e| same_marker(e, marker)).map_or(-1, |index| index as i64) - 1;
        }

        if marker_index < 0 {
            *self.current_marker.borrow_mut() = MarkerOrBoundary::Top;
            terminal.scroll_to_top();
            self.reset_navigation_decorations();
            return Ok(());
        }

        let Some(marker) = self.marker_at(skip_empty_commands, marker_index) else { return Ok(()) };
        *self.current_marker.borrow_mut() = MarkerOrBoundary::Marker(marker.clone());
        self.scroll_to_command(&marker, scroll_position)
    }

    pub fn scroll_to_next_mark(&self, scroll_position: Option<ScrollPosition>, retain_selection: Option<bool>, skip_empty_commands: Option<bool>) -> Result<(), Error> {
        let scroll_position = scroll_position.unwrap_or(ScrollPosition::Middle);
        let skip_empty_commands = skip_empty_commands.unwrap_or(true);
        let Some(terminal) = self.terminal() else { return Ok(()) };
        if !retain_selection.unwrap_or(false) {
            *self.selection_start.borrow_mut() = None;
        }

        let current_marker = self.current_marker();
        let current_line_y = match &current_marker {
            MarkerOrBoundary::Marker(marker) => self.get_target_scroll_line(marker.line() as i32, scroll_position),
            boundary => get_line(&*terminal, boundary).min(terminal.buffer_active().base_y()),
        };
        let viewport_y = terminal.viewport_y();
        let marker_index: i64;
        let scrolled = match &current_marker {
            MarkerOrBoundary::Marker(marker) => !self.is_marker_in_viewport(&*terminal, &MarkerOrLine::Marker(marker.clone())),
            _ => current_line_y != viewport_y,
        };
        if scrolled {
            // The user has scrolled, find the line based on the current scroll position. This only
            // works when not retaining selection
            let markers_above_viewport = self.get_markers(skip_empty_commands).iter().filter(|e| e.line() <= f64::from(viewport_y)).count() as i64;
            // markers.length will scroll to the bottom
            marker_index = markers_above_viewport;
        } else if matches!(current_marker, MarkerOrBoundary::Bottom) {
            marker_index = self.get_markers(skip_empty_commands).len() as i64;
        } else if matches!(current_marker, MarkerOrBoundary::Top) {
            marker_index = 0;
        } else if self.is_disposable.get() {
            marker_index = self.find_next_marker(skip_empty_commands);
            if let MarkerOrBoundary::Marker(marker) = &current_marker {
                marker.dispose();
            }
            self.is_disposable.set(false);
        } else if skip_empty_commands && self.is_empty_command(&current_marker) {
            marker_index = self.find_next_marker(true);
        } else {
            let MarkerOrBoundary::Marker(marker) = &current_marker else { return Ok(()) };
            marker_index = self.get_markers(skip_empty_commands).iter().position(|e| same_marker(e, marker)).map_or(-1, |index| index as i64) + 1;
        }

        if marker_index >= self.get_markers(skip_empty_commands).len() as i64 {
            *self.current_marker.borrow_mut() = MarkerOrBoundary::Bottom;
            terminal.scroll_to_bottom();
            self.reset_navigation_decorations();
            return Ok(());
        }

        let Some(marker) = self.marker_at(skip_empty_commands, marker_index) else { return Ok(()) };
        *self.current_marker.borrow_mut() = MarkerOrBoundary::Marker(marker.clone());
        self.scroll_to_command(&marker, scroll_position)
    }

    fn scroll_to_command(&self, marker: &Rc<dyn IMarker>, position: ScrollPosition) -> Result<(), Error> {
        match self.find_command(&**marker) {
            Some(command) => self.reveal_command(&command, Some(position)),
            None => self.scroll_to_marker(&MarkerOrLine::Marker(marker.clone()), position, None, None),
        }
    }

    fn scroll_to_marker(&self, start: &MarkerOrLine, position: ScrollPosition, end: Option<&MarkerOrLine>, options: Option<&IScrollToMarkerOptions>) -> Result<(), Error> {
        let Some(terminal) = self.terminal() else { return Ok(()) };
        let force_scroll = options.is_some_and(|options| options.force_scroll);
        if !self.is_marker_in_viewport(&*terminal, start) || force_scroll {
            let line = self.get_target_scroll_line(to_line_index(start), position);
            terminal.scroll_to_line(line);
        }
        if !options.is_some_and(|options| options.hide_decoration) {
            match options.and_then(|options| options.buffer_range) {
                Some(buffer_range) => self.highlight_buffer_range(&buffer_range)?,
                None => self.register_temporary_decoration_for(start, end, true)?,
            }
        }
        Ok(())
    }

    fn create_marker_for_offset(&self, marker: &MarkerOrLine, offset: i32) -> Result<Rc<dyn IMarker>, Error> {
        if offset == 0
            && let MarkerOrLine::Marker(marker) = marker
        {
            return Ok(marker.clone());
        }
        let terminal = self.terminal();
        let offset_marker = terminal.as_ref().and_then(|terminal| {
            let buffer = terminal.buffer_active();
            terminal.register_marker(-buffer.cursor_y() + to_line_index(marker) - buffer.base_y() + offset)
        });
        offset_marker.ok_or_else(|| Error::new(format!("Could not register marker with offset {}, {}", to_line_index(marker), offset)))
    }

    pub fn reveal_command(&self, command: &TerminalCommandForLine, position: Option<ScrollPosition>) -> Result<(), Error> {
        let position = position.unwrap_or(ScrollPosition::Middle);
        let (marker, prompt_row_count, command_row_count) = match command {
            TerminalCommandForLine::Command(command) => {
                let marker = if command.is_full_terminal_command() {
                    command.marker()
                } else {
                    command.as_partial_terminal_command().and_then(|partial| ICurrentPartialCommand::command_start_marker(partial))
                };
                (marker, command.get_prompt_row_count(), command.get_command_row_count())
            }
            TerminalCommandForLine::Partial(command) => (command.command_start_marker(), command.get_prompt_row_count(), command.get_command_row_count()),
        };
        let Some(marker) = marker else { return Ok(()) };
        if self.terminal().is_none() {
            return Ok(());
        }
        let line = marker.line() as i32;
        self.scroll_to_marker(
            &MarkerOrLine::Line(line - (prompt_row_count as i32 - 1)),
            position,
            Some(&MarkerOrLine::Line(line + (command_row_count as i32 - 1))),
            None,
        )
    }

    pub fn reveal_range(&self, range: &IBufferRange) -> Result<(), Error> {
        self.scroll_to_marker(
            &MarkerOrLine::Line(range.start.y - 1),
            ScrollPosition::Middle,
            Some(&MarkerOrLine::Line(range.end.y - 1)),
            Some(&IScrollToMarkerOptions {
                buffer_range: Some(*range),
                // Ensure scroll shows the line when sticky scroll is enabled
                force_scroll: self.configuration_service.get_value(Some(STICKY_SCROLL_ENABLED), None).is_truthy(),
                ..Default::default()
            }),
        )
    }

    pub fn show_command_guide(&self, command: Option<Rc<dyn ITerminalCommand>>) -> Result<(), Error> {
        let Some(terminal) = self.terminal() else { return Ok(()) };
        let Some(command) = command else {
            self.command_guide_decorations.clear();
            *self.active_command_guide.borrow_mut() = None;
            return Ok(());
        };
        if self.active_command_guide.borrow().as_ref().is_some_and(|active| std::ptr::addr_eq(Rc::as_ptr(active), Rc::as_ptr(&command))) {
            return Ok(());
        }
        if let Some(marker) = command.marker() {
            *self.active_command_guide.borrow_mut() = Some(command.clone());

            // Highlight output
            let store = DisposableStore::new();
            self.command_guide_decorations.set(Some(store.clone()));
            let (Some(_), Some(end_marker)) = (command.executed_marker(), command.end_marker()) else { return Ok(()) };
            let start_line = marker.line() as i32 - (command.get_prompt_row_count() as i32 - 1);
            let decoration_count = end_marker.line() as i32 - start_line;
            // Abort if the command is excessively long to avoid performance on hover/leave
            if decoration_count > 200 {
                return Ok(());
            }
            for i in 0..decoration_count {
                let decoration = terminal.register_decoration(IDecorationOptions {
                    marker: self.create_marker_for_offset(&MarkerOrLine::Line(start_line), i)?,
                    x: None,
                    width: None,
                    overview_ruler_options: None,
                });
                if let Some(decoration) = decoration {
                    store.add(decoration.clone());
                    let rendered_element = Cell::new(false);
                    store.add(decoration.on_render().listen(move |element| {
                        if !rendered_element.replace(true) {
                            element.class_list_add(&["terminal-command-guide"]);
                            if i == 0 {
                                element.class_list_add(&["top"]);
                            }
                            if i == decoration_count - 1 {
                                element.class_list_add(&["bottom"]);
                            }
                        }
                    }));
                }
            }
        }
        Ok(())
    }

    pub fn save_scroll_state(&self) {
        self.scroll_state.set(Some(self.terminal().map_or(0, |terminal| terminal.viewport_y())));
    }

    pub fn restore_scroll_state(&self) {
        if let (Some(viewport_y), Some(terminal)) = (self.scroll_state.get(), self.terminal()) {
            terminal.scroll_to_line(viewport_y);
            self.scroll_state.set(None);
        }
    }

    fn highlight_buffer_range(&self, range: &IBufferRange) -> Result<(), Error> {
        let Some(terminal) = self.terminal() else { return Ok(()) };

        self.reset_navigation_decorations();
        let start_line = range.start.y;
        let decoration_count = range.end.y - range.start.y + 1;
        for i in 0..decoration_count {
            let decoration = terminal.register_decoration(IDecorationOptions {
                marker: self.create_marker_for_offset(&MarkerOrLine::Line(start_line - 1), i)?,
                x: Some(range.start.x - 1),
                width: Some((range.end.x - 1) - (range.start.x - 1) + 1),
                overview_ruler_options: None,
            });
            if let Some(decoration) = decoration {
                self.push_navigation_decoration(&decoration);

                let rendered_element = Cell::new(false);
                // O retorno de `onRender` e de `onDispose` não é guardado no original.
                let _ = decoration.on_render().listen(move |element| {
                    if !rendered_element.replace(true) {
                        element.class_list_add(&["terminal-range-highlight"]);
                    }
                });
                self.remove_navigation_decoration_on_dispose(&decoration);
            }
        }
        Ok(())
    }

    fn push_navigation_decoration(&self, decoration: &Rc<dyn IXtermDecoration>) {
        if let Some(decorations) = self.navigation_decorations.borrow_mut().as_mut() {
            decorations.push(decoration.clone());
        }
    }

    /// `decoration.onDispose(() => { this._navigationDecorations = this._navigationDecorations?.filter(d => d !== decoration); })`.
    fn remove_navigation_decoration_on_dispose(&self, decoration: &Rc<dyn IXtermDecoration>) {
        let weak = self.this.clone();
        let address = decoration_address(decoration);
        let _ = decoration.on_dispose().listen(move |_| {
            if let Some(this) = weak.upgrade()
                && let Some(decorations) = this.navigation_decorations.borrow_mut().as_mut()
            {
                decorations.retain(|d| decoration_address(d) != address);
            }
        });
    }

    pub fn register_temporary_decoration(&self, marker: Rc<dyn IMarker>, end_marker: Option<Rc<dyn IMarker>>, show_outline: bool) -> Result<(), Error> {
        self.register_temporary_decoration_for(&MarkerOrLine::Marker(marker), end_marker.map(MarkerOrLine::Marker).as_ref(), show_outline)
    }

    /// `registerTemporaryDecoration(marker: IMarker | number, endMarker: IMarker | number | undefined, showOutline)`.
    pub fn register_temporary_decoration_for(&self, marker: &MarkerOrLine, end_marker: Option<&MarkerOrLine>, show_outline: bool) -> Result<(), Error> {
        let Some(terminal) = self.terminal() else { return Ok(()) };
        self.reset_navigation_decorations();
        let color = self.theme_service.get_color_theme().get_color(TERMINAL_OVERVIEW_RULER_CURSOR_FOREGROUND_COLOR, None);
        let start_line = to_line_index(marker);
        let decoration_count = end_marker.map_or(1, |end_marker| to_line_index(end_marker) - start_line + 1);
        for i in 0..decoration_count {
            let decoration = terminal.register_decoration(IDecorationOptions {
                marker: self.create_marker_for_offset(marker, i)?,
                x: None,
                width: Some(terminal.cols()),
                overview_ruler_options: (i == 0).then(|| IDecorationOverviewRulerOptions {
                    color: color.as_ref().map(ToString::to_string).filter(|color| !color.is_empty()).unwrap_or_else(|| "#a0a0a0cc".to_owned()),
                    position: None,
                }),
            });
            if let Some(decoration) = decoration {
                self.push_navigation_decoration(&decoration);

                let rendered_element: Rc<RefCell<Option<Rc<dyn super::xterm_decoration::IDecorationElement>>>> = Rc::default();
                let rendered = rendered_element.clone();
                let _ = decoration.on_render().listen(move |element| {
                    if rendered.borrow().is_none() {
                        *rendered.borrow_mut() = Some(element.clone());
                        element.class_list_add(&["terminal-scroll-highlight"]);
                        if show_outline {
                            element.class_list_add(&["terminal-scroll-highlight-outline"]);
                        }
                        if i == 0 {
                            element.class_list_add(&["top"]);
                        }
                        if i == decoration_count - 1 {
                            element.class_list_add(&["bottom"]);
                        }
                    } else {
                        element.class_list_add(&["terminal-scroll-highlight"]);
                    }
                });
                // Nota do original: isto não é eficiente para um `decorationCount` grande.
                self.remove_navigation_decoration_on_dispose(&decoration);
                // Number picked to align with symbol highlight in the editor
                if show_outline {
                    let _ = je_base::async_::timeout(350).then(move |()| {
                        if let Some(element) = rendered_element.borrow().as_ref() {
                            element.class_list_remove("terminal-scroll-highlight-outline");
                        }
                    });
                }
            }
        }
        Ok(())
    }

    pub fn scroll_to_line(&self, line: i32, position: ScrollPosition) {
        if let Some(terminal) = self.terminal() {
            terminal.scroll_to_line(self.get_target_scroll_line(line, position));
        }
    }

    pub fn get_target_scroll_line(&self, line: i32, position: ScrollPosition) -> i32 {
        // Middle is treated as 1/4 of the viewport's size because context below is almost always
        // more important than context above in the terminal.
        if let Some(terminal) = self.terminal()
            && position == ScrollPosition::Middle
        {
            return (line - terminal.rows().div_euclid(4)).max(0);
        }
        line
    }

    fn is_marker_in_viewport(&self, terminal: &dyn IMarkNavigationTerminal, marker: &MarkerOrLine) -> bool {
        let viewport_y = terminal.viewport_y();
        let line = to_line_index(marker);
        line >= viewport_y && line < viewport_y + terminal.rows()
    }

    pub fn scroll_to_closest_marker(&self, start_marker_id: &str, end_marker_id: Option<&str>, highlight: Option<bool>) -> Result<(), Error> {
        let Some(detection_capability) = self.capabilities.get::<BufferMarkDetection>() else { return Ok(()) };
        let Some(start_marker) = detection_capability.get_mark(start_marker_id) else { return Ok(()) };
        let end_marker = match end_marker_id.filter(|id| !id.is_empty()) {
            Some(end_marker_id) => detection_capability.get_mark(end_marker_id),
            None => Some(start_marker.clone()),
        };
        self.scroll_to_marker(
            &MarkerOrLine::Marker(start_marker),
            ScrollPosition::Top,
            end_marker.map(MarkerOrLine::Marker).as_ref(),
            Some(&IScrollToMarkerOptions { hide_decoration: !highlight.unwrap_or(false), ..Default::default() }),
        )
    }

    /// Guarda o ponto de partida da seleção (`this._selectionStart ??= currentMarker`, que no original é `=== null`).
    fn ensure_selection_start(&self) -> MarkerOrBoundary {
        let mut selection_start = self.selection_start.borrow_mut();
        selection_start.get_or_insert_with(|| self.current_marker()).clone()
    }

    pub fn select_to_previous_mark(&self) -> Result<(), Error> {
        let Some(terminal) = self.terminal() else { return Ok(()) };
        let selection_start = self.ensure_selection_start();
        if self.capabilities.has(TerminalCapability::CommandDetection) {
            self.scroll_to_previous_mark(Some(ScrollPosition::Middle), Some(true), Some(true))?;
        } else {
            self.scroll_to_previous_mark(Some(ScrollPosition::Middle), Some(true), Some(false))?;
        }
        select_lines(&*terminal, &self.current_marker(), Some(&selection_start));
        Ok(())
    }

    pub fn select_to_next_mark(&self) -> Result<(), Error> {
        let Some(terminal) = self.terminal() else { return Ok(()) };
        let selection_start = self.ensure_selection_start();
        if self.capabilities.has(TerminalCapability::CommandDetection) {
            self.scroll_to_next_mark(Some(ScrollPosition::Middle), Some(true), Some(true))?;
        } else {
            self.scroll_to_next_mark(Some(ScrollPosition::Middle), Some(true), Some(false))?;
        }
        select_lines(&*terminal, &self.current_marker(), Some(&selection_start));
        Ok(())
    }

    pub fn select_to_previous_line(&self) -> Result<(), Error> {
        let Some(terminal) = self.terminal() else { return Ok(()) };
        let selection_start = self.ensure_selection_start();
        self.scroll_to_previous_line(&*terminal, Some(ScrollPosition::Middle), Some(true))?;
        select_lines(&*terminal, &self.current_marker(), Some(&selection_start));
        Ok(())
    }

    pub fn select_to_next_line(&self) -> Result<(), Error> {
        let Some(terminal) = self.terminal() else { return Ok(()) };
        let selection_start = self.ensure_selection_start();
        self.scroll_to_next_line(&*terminal, Some(ScrollPosition::Middle), Some(true))?;
        select_lines(&*terminal, &self.current_marker(), Some(&selection_start));
        Ok(())
    }

    pub fn scroll_to_previous_line(&self, xterm: &dyn IMarkNavigationTerminal, scroll_position: Option<ScrollPosition>, retain_selection: Option<bool>) -> Result<(), Error> {
        let scroll_position = scroll_position.unwrap_or(ScrollPosition::Middle);
        if !retain_selection.unwrap_or(false) {
            *self.selection_start.borrow_mut() = None;
        }

        let current_marker = self.current_marker();
        match &current_marker {
            MarkerOrBoundary::Top => {
                xterm.scroll_to_top();
                return Ok(());
            }
            MarkerOrBoundary::Bottom => {
                let marker = register_marker_or_throw(xterm, self.get_offset(xterm) - 1)?;
                *self.current_marker.borrow_mut() = MarkerOrBoundary::Marker(marker);
            }
            MarkerOrBoundary::Marker(marker) => {
                let offset = self.get_offset(xterm);
                if self.is_disposable.get() {
                    marker.dispose();
                }
                let marker = register_marker_or_throw(xterm, offset - 1)?;
                *self.current_marker.borrow_mut() = MarkerOrBoundary::Marker(marker);
            }
        }
        self.is_disposable.set(true);
        let MarkerOrBoundary::Marker(marker) = self.current_marker() else { return Ok(()) };
        self.scroll_to_marker(&MarkerOrLine::Marker(marker), scroll_position, None, None)
    }

    pub fn scroll_to_next_line(&self, xterm: &dyn IMarkNavigationTerminal, scroll_position: Option<ScrollPosition>, retain_selection: Option<bool>) -> Result<(), Error> {
        let scroll_position = scroll_position.unwrap_or(ScrollPosition::Middle);
        if !retain_selection.unwrap_or(false) {
            *self.selection_start.borrow_mut() = None;
        }

        let current_marker = self.current_marker();
        match &current_marker {
            MarkerOrBoundary::Bottom => {
                xterm.scroll_to_bottom();
                return Ok(());
            }
            MarkerOrBoundary::Top => {
                let marker = register_marker_or_throw(xterm, self.get_offset(xterm) + 1)?;
                *self.current_marker.borrow_mut() = MarkerOrBoundary::Marker(marker);
            }
            MarkerOrBoundary::Marker(marker) => {
                let offset = self.get_offset(xterm);
                if self.is_disposable.get() {
                    marker.dispose();
                }
                let marker = register_marker_or_throw(xterm, offset + 1)?;
                *self.current_marker.borrow_mut() = MarkerOrBoundary::Marker(marker);
            }
        }
        self.is_disposable.set(true);
        let MarkerOrBoundary::Marker(marker) = self.current_marker() else { return Ok(()) };
        self.scroll_to_marker(&MarkerOrLine::Marker(marker), scroll_position, None, None)
    }

    fn get_offset(&self, xterm: &dyn IMarkNavigationTerminal) -> i32 {
        let current_marker = self.current_marker();
        let buffer = xterm.buffer_active();
        match &current_marker {
            MarkerOrBoundary::Bottom => 0,
            MarkerOrBoundary::Top => 0 - (buffer.base_y() + buffer.cursor_y()),
            MarkerOrBoundary::Marker(_) => get_line(xterm, &current_marker) - (buffer.base_y() + buffer.cursor_y()),
        }
    }

    fn find_previous_marker(&self, skip_empty_commands: bool) -> i64 {
        let current_marker = match self.current_marker() {
            MarkerOrBoundary::Top => return 0,
            MarkerOrBoundary::Bottom => return self.get_markers(skip_empty_commands).len() as i64 - 1,
            MarkerOrBoundary::Marker(marker) => marker,
        };

        let mut i = self.get_markers(skip_empty_commands).len() as i64 - 1;
        while i >= 0 {
            if self.get_markers(skip_empty_commands)[i as usize].line() < current_marker.line() {
                return i;
            }
            i -= 1;
        }

        -1
    }

    fn find_next_marker(&self, skip_empty_commands: bool) -> i64 {
        let current_marker = match self.current_marker() {
            MarkerOrBoundary::Top => return 0,
            MarkerOrBoundary::Bottom => return self.get_markers(skip_empty_commands).len() as i64 - 1,
            MarkerOrBoundary::Marker(marker) => marker,
        };

        let mut i = 0;
        while i < self.get_markers(skip_empty_commands).len() {
            if self.get_markers(skip_empty_commands)[i].line() > current_marker.line() {
                return i as i64;
            }
            i += 1;
        }

        self.get_markers(skip_empty_commands).len() as i64
    }
}

impl IDisposable for MarkNavigationAddon {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IMarkTracker for MarkNavigationAddon {
    fn scroll_to_previous_mark(&self, scroll_position: Option<ScrollPosition>, retain_selection: Option<bool>, skip_empty_commands: Option<bool>) -> Result<(), Error> {
        MarkNavigationAddon::scroll_to_previous_mark(self, scroll_position, retain_selection, skip_empty_commands)
    }

    fn scroll_to_next_mark(&self) -> Result<(), Error> {
        MarkNavigationAddon::scroll_to_next_mark(self, None, None, None)
    }

    fn select_to_previous_mark(&self) -> Result<(), Error> {
        MarkNavigationAddon::select_to_previous_mark(self)
    }

    fn select_to_next_mark(&self) -> Result<(), Error> {
        MarkNavigationAddon::select_to_next_mark(self)
    }

    fn select_to_previous_line(&self) -> Result<(), Error> {
        MarkNavigationAddon::select_to_previous_line(self)
    }

    fn select_to_next_line(&self) -> Result<(), Error> {
        MarkNavigationAddon::select_to_next_line(self)
    }

    fn clear(&self) {
        MarkNavigationAddon::clear(self);
    }

    fn scroll_to_closest_marker(&self, start_marker_id: &str, end_marker_id: Option<&str>, highlight: Option<bool>) -> Result<(), Error> {
        MarkNavigationAddon::scroll_to_closest_marker(self, start_marker_id, end_marker_id, highlight)
    }

    fn scroll_to_line(&self, line: i32, position: ScrollPosition) {
        MarkNavigationAddon::scroll_to_line(self, line, position);
    }

    fn reveal_command(&self, command: &TerminalCommandForLine, position: Option<ScrollPosition>) -> Result<(), Error> {
        MarkNavigationAddon::reveal_command(self, command, position)
    }

    fn reveal_range(&self, range: &IBufferRange) -> Result<(), Error> {
        MarkNavigationAddon::reveal_range(self, range)
    }

    fn register_temporary_decoration(&self, marker: Rc<dyn IMarker>, end_marker: Option<Rc<dyn IMarker>>, show_outline: bool) -> Result<(), Error> {
        MarkNavigationAddon::register_temporary_decoration(self, marker, end_marker, show_outline)
    }

    fn show_command_guide(&self, command: Option<Rc<dyn ITerminalCommand>>) -> Result<(), Error> {
        MarkNavigationAddon::show_command_guide(self, command)
    }

    fn save_scroll_state(&self) {
        MarkNavigationAddon::save_scroll_state(self);
    }

    fn restore_scroll_state(&self) {
        MarkNavigationAddon::restore_scroll_state(self);
    }
}

fn register_marker_or_throw(xterm: &dyn IMarkNavigationTerminal, cursor_y_offset: i32) -> Result<Rc<dyn IMarker>, Error> {
    xterm.register_marker(cursor_y_offset).ok_or_else(|| Error::new(format!("Could not create marker for {cursor_y_offset}")))
}

pub fn get_line(xterm: &dyn IMarkNavigationTerminal, marker: &MarkerOrBoundary) -> i32 {
    match marker {
        // Use the _second last_ row as the last row is likely the prompt
        MarkerOrBoundary::Bottom => xterm.buffer_active().base_y() + xterm.rows() - 1,
        MarkerOrBoundary::Top => 0,
        MarkerOrBoundary::Marker(marker) => marker.line() as i32,
    }
}

pub fn select_lines(xterm: &dyn IMarkNavigationTerminal, start: &MarkerOrBoundary, end: Option<&MarkerOrBoundary>) {
    let end = end.cloned().unwrap_or(MarkerOrBoundary::Bottom);

    let mut start_line = get_line(xterm, start);
    let mut end_line = get_line(xterm, &end);

    if start_line > end_line {
        std::mem::swap(&mut start_line, &mut end_line);
    }

    // Subtract a line as the marker is on the line the command run, we do not want the next
    // command in the selection for the current command
    end_line -= 1;

    xterm.select_lines(start_line, end_line);
}

fn to_line_index(line: &MarkerOrLine) -> i32 {
    match line {
        MarkerOrLine::Marker(marker) => marker.line() as i32,
        MarkerOrLine::Line(line) => *line,
    }
}
