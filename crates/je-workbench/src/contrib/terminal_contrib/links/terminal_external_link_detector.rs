//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalExternalLinkDetector.ts`: o detector dos links
//! que uma extensão fornece (o `ITerminalExternalLinkProvider`), no tipo `{ id }` e com o `activate` do provedor.
//!
//! Decisões de tradução:
//! - **`OmitFirstArg<ITerminalExternalLinkProvider['provideLinks']>`** é o [`ProvideExternalLinks`]: `(line) => Promise<links | undefined>`.
//! - O texto da linha é UTF-16 ([`JsString`]); `text.substring(startIndex, startIndex + length)` e os índices contam unidades UTF-16.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::js_string::{self, JsString};
use je_base::promise::Promise;
use je_editor::range::Range;

use super::links::{ITerminalExternalLinkType, ITerminalLinkDetector, ITerminalSimpleLink};
use super::terminal_link_helpers::{convert_link_range_to_buffer, get_xterm_line_content};
use crate::contrib::terminal::browser::terminal::ITerminalLink;
use crate::contrib::terminal::xterm_buffer::{IBufferLine, ITerminal};

/// `(line: string) => Promise<ITerminalLink[] | undefined>`.
pub type ProvideExternalLinks = Rc<dyn Fn(&str) -> Promise<Option<Vec<ITerminalLink>>>>;

/// `TerminalExternalLinkDetector`.
#[derive(Clone)]
pub struct TerminalExternalLinkDetector {
    pub id: String,
    xterm: Rc<dyn ITerminal>,
    provide_links: ProvideExternalLinks,
}

impl TerminalExternalLinkDetector {
    pub const MAX_LINK_LENGTH: i32 = 2000;

    pub fn new(id: &str, xterm: Rc<dyn ITerminal>, provide_links: ProvideExternalLinks) -> Self {
        Self { id: id.to_owned(), xterm, provide_links }
    }

    async fn detect_inner(&self, lines: Vec<Rc<dyn IBufferLine>>, start_line: i32, end_line: i32) -> Result<Vec<ITerminalSimpleLink>, Error> {
        // Get the text representation of the wrapped line
        let cols = self.xterm.cols();
        let text = get_xterm_line_content(self.xterm.buffer_active().as_ref(), start_line, end_line, cols);
        if text.is_empty() || text.len() > Self::MAX_LINK_LENGTH as usize {
            return Ok(Vec::new());
        }

        let Some(external_links) = (self.provide_links)(&text.to_string_lossy()).await? else {
            return Ok(Vec::new());
        };

        let result = external_links
            .into_iter()
            .map(|link| {
                let start_index = link.start_index as i32;
                let buffer_range = convert_link_range_to_buffer(
                    &lines,
                    cols,
                    &Range {
                        start_column: start_index + 1,
                        start_line_number: 1,
                        end_column: start_index + link.length as i32 + 1,
                        end_line_number: 1,
                    },
                    start_line,
                );
                let matching_text = JsString::from(js_string::substring(text.units(), link.start_index as i64, (link.start_index + link.length) as i64)).to_string_lossy();

                let activate = link.activate.clone();
                let mut simple_link = ITerminalSimpleLink::new(matching_text, buffer_range, super::links::TerminalLinkType::External(ITerminalExternalLinkType { id: self.id.clone() }));
                simple_link.label = link.label.clone();
                simple_link.activate = Some(Rc::new(move |_text: &str| activate()));
                simple_link
            })
            .collect();

        Ok(result)
    }
}

impl ITerminalLinkDetector for TerminalExternalLinkDetector {
    fn xterm(&self) -> Rc<dyn ITerminal> {
        self.xterm.clone()
    }

    fn max_link_length(&self) -> i32 {
        Self::MAX_LINK_LENGTH
    }

    fn detect(&self, lines: &[Rc<dyn IBufferLine>], start_line: i32, end_line: i32) -> Promise<Vec<ITerminalSimpleLink>> {
        let this = self.clone();
        let lines = lines.to_vec();
        async_fn(async move { this.detect_inner(lines, start_line, end_line).await })
    }
}
