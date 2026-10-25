//! Porte de `src/vs/workbench/contrib/terminal/browser/xterm`.

pub mod xterm_terminal;

// Os addons do xterm e o contrato de decorações que eles leem.
pub mod decoration_addon;
pub mod decoration_styles;
pub mod line_data_event_addon;
pub mod mark_navigation_addon;
pub mod terminal_font_rendering;
pub mod xterm_decoration;
