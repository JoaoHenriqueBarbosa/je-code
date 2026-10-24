//! Porte de `src/vs/platform/terminal/common/terminalStrings.ts`: a formatação das mensagens do produto que se escrevem no
//! terminal.

/// `ITerminalFormatMessageOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ITerminalFormatMessageOptions {
    /// Whether to exclude the new line at the start of the message. Defaults to false.
    pub exclude_leading_new_line: bool,
    /// Whether to use "loud" formatting, this is for more important messages where the it's desirable to visually break
    /// the buffer up. Defaults to false.
    pub loud_formatting: bool,
}

/// Formats a message from the product to be written to the terminal.
pub fn format_message_for_terminal(message: &str, options: ITerminalFormatMessageOptions) -> String {
    let mut result = String::new();
    if !options.exclude_leading_new_line {
        result.push_str("\r\n");
    }
    result.push_str("\x1b[0m\x1b[7m * ");
    if options.loud_formatting {
        result.push_str("\x1b[0;104m");
    } else {
        result.push_str("\x1b[0m");
    }
    result.push_str(&format!(" {message} \x1b[0m\n\r"));
    result
}
