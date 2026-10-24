//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalMultiLineLinkDetector.ts`: detecta, numa linha
//! (possivelmente quebrada) do buffer, os links cujo caminho está numa linha anterior: a saída com número de linha no começo
//! (ripgrep e eslint) e o cabeçalho `@@ ... @@` dos diffs do git.
//!
//! Decisões de tradução:
//! - `Terminal` do `@xterm/xterm` é o [`ITerminal`]. As dependências injetadas (`ITerminalLogService`, `IUriIdentityService` e
//!   `IWorkspaceContextService`) são parâmetros do [`TerminalMultiLineLinkDetector::new`], como no detector local.
//! - `async detect(...)` é uma `async fn` interna embrulhada por [`async_fn`]; o detector é `Clone` (só `Rc` por dentro).
//! - As expressões regulares (`lineNumberPrefixMatchers`, `gitDiffMatchers`, `/^\s*\d/` e `/\+\+\+ b\/(?<path>.+)/`) rodam no
//!   `zyarr`, com grupos nomeados. `parseInt` é o [`parse_int`] (um número `f64`, como o do original).
//! - O texto da linha é UTF-16 ([`JsString`]) e os índices contam unidades UTF-16, como o `string` do original.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::js_string::JsString;
use je_base::promise::Promise;
use je_editor::range::Range;
use je_platform::log::{LogArg, LogLevel, LogServiceRef};
use je_platform::opener::editor_types::TextEditorSelection;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use zyarr::RegExp;

use super::links::{ITerminalLinkDetector, ITerminalLinkResolver, ITerminalSimpleLink, TerminalLinkProcessManager};
use super::terminal_link_helpers::{convert_link_range_to_buffer, get_xterm_line_content};
use super::terminal_local_link_detector::get_terminal_link_type;
use crate::contrib::terminal::xterm_buffer::{IBufferLine, ITerminal};

/// `Constants.MaxLineLength`: the max line length to try extract word links from.
const MAX_LINE_LENGTH: usize = 2000;

/// `Constants.MaxResolvedLinkLength`: the maximum length of a link to resolve against the file system. This limit is put in place
/// to avoid sending excessive data when remote connections are in place.
const MAX_RESOLVED_LINK_LENGTH: usize = 1024;

/// `lineNumberPrefixMatchers`.
const LINE_NUMBER_PREFIX_MATCHERS: [&str; 1] = [
    // Ripgrep:
    //   /some/file
    //   16:searchresult
    //   16:    searchresult
    // Eslint:
    //   /some/file
    //     16:5  error ...
    r"^ *(?<link>(?<line>\d+):(?<col>\d+)?)",
];

/// `gitDiffMatchers`.
const GIT_DIFF_MATCHERS: [&str; 1] = [
    // --- a/some/file
    // +++ b/some/file
    // @@ -8,11 +8,11 @@ file content...
    r"^(?<link>@@ .+ \+(?<toFileLine>\d+),(?<toFileCount>\d+) @@)",
];

/// O texto de um grupo nomeado do casamento, ou `None` se ele não participou.
fn named_group(units: &[u16], found: &zyarr::Match, name: &str) -> Option<JsString> {
    found.group(name).map(|range| JsString::from(&units[range]))
}

/// `parseInt(text)` de um texto só de dígitos.
fn parse_int(text: &JsString) -> f64 {
    text.to_string_lossy().parse::<f64>().unwrap_or(f64::NAN)
}

/// `TerminalMultiLineLinkDetector`.
#[derive(Clone)]
pub struct TerminalMultiLineLinkDetector {
    xterm: Rc<dyn ITerminal>,
    process_manager: TerminalLinkProcessManager,
    link_resolver: Rc<dyn ITerminalLinkResolver>,
    log_service: LogServiceRef,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

impl TerminalMultiLineLinkDetector {
    pub const ID: &'static str = "multiline";

    // This was chosen as a reasonable maximum line length given the tradeoff between performance
    // and how likely it is to encounter such a large line length. Some useful reference points:
    // - Window old max length: 260 ($MAX_PATH)
    // - Linux max length: 4096 ($PATH_MAX)
    pub const MAX_LINK_LENGTH: i32 = 500;

    pub fn new(
        xterm: Rc<dyn ITerminal>,
        process_manager: TerminalLinkProcessManager,
        link_resolver: Rc<dyn ITerminalLinkResolver>,
        log_service: LogServiceRef,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        workspace_context_service: Rc<dyn IWorkspaceContextService>,
    ) -> Self {
        Self { xterm, process_manager, link_resolver, log_service, uri_identity_service, workspace_context_service }
    }

    fn trace(&self, message: &str, detail: &dyn std::fmt::Debug) {
        if self.log_service.get_level() == LogLevel::Trace {
            self.log_service.trace_args(message, &[LogArg::from(format!("{detail:?}").as_str())]);
        }
    }

    /// O laço "Scan up looking for the first line that could be a path": sobe a partir da linha anterior ao início, ignorando
    /// as linhas que não começam uma linha quebrada, e devolve o texto da primeira em que `pick` achar o caminho.
    fn scan_up_for_path(&self, start_line: i32, pick: impl Fn(&JsString) -> Option<Option<JsString>>) -> Option<JsString> {
        let cols = self.xterm.cols();
        let buffer = self.xterm.buffer_active();
        let mut index = start_line - 1;
        while index >= 0 {
            // Ignore lines that aren't at the beginning of a wrapped line
            if buffer.get_line(index).expect("a linha existe").is_wrapped() {
                index -= 1;
                continue;
            }
            let text = get_xterm_line_content(buffer.as_ref(), index, index, cols);
            if let Some(path) = pick(&text) {
                return path;
            }
            index -= 1;
        }
        None
    }

    async fn detect_inner(&self, lines: Vec<Rc<dyn IBufferLine>>, start_line: i32, end_line: i32) -> Result<Vec<ITerminalSimpleLink>, Error> {
        let mut links: Vec<ITerminalSimpleLink> = Vec::new();

        // Get the text representation of the wrapped line
        let cols = self.xterm.cols();
        let buffer = self.xterm.buffer_active();
        let text = get_xterm_line_content(buffer.as_ref(), start_line, end_line, cols);
        if text.is_empty() || text.len() > MAX_LINE_LENGTH {
            return Ok(Vec::new());
        }

        self.trace("terminalMultiLineLinkDetector#detect text", &text.to_string_lossy());

        // Match against the fallback matchers which are mainly designed to catch paths with spaces
        // that aren't possible using the regular mechanism.
        for matcher in LINE_NUMBER_PREFIX_MATCHERS {
            let regex = RegExp::from_str(matcher, "").expect("a expressão do prefixo é válida");
            let Some(found) = regex.exec(text.units(), 0) else {
                continue;
            };
            let link = named_group(text.units(), &found, "link").filter(|link| !link.is_empty());
            let line = named_group(text.units(), &found, "line");
            let col = named_group(text.units(), &found, "col").filter(|col| !col.is_empty());
            let (Some(link), Some(line)) = (link, line) else {
                continue;
            };

            // Don't try resolve any links of excessive length
            if link.len() > MAX_RESOLVED_LINK_LENGTH {
                continue;
            }

            self.trace("terminalMultiLineLinkDetector#detect candidate", &link.to_string_lossy());

            // Scan up looking for the first line that could be a path
            let starts_with_digit = RegExp::from_str(r"^\s*\d", "").expect("a expressão do dígito é válida");
            let possible_path = self.scan_up_for_path(start_line, |text| starts_with_digit.exec(text.units(), 0).is_none().then(|| Some(text.clone())));
            let Some(possible_path) = possible_path.filter(|path| !path.is_empty()) else {
                continue;
            };

            // Check if the first non-matching line is an absolute or relative link
            let link_stat = self.link_resolver.resolve_link(&self.process_manager, &possible_path.to_string_lossy(), None).await?;
            if let Some(link_stat) = link_stat {
                let type_ = get_terminal_link_type(&link_stat.uri, link_stat.is_directory, self.uri_identity_service.as_ref(), self.workspace_context_service.as_ref());

                // Convert the entire line's text string index into a wrapped buffer range
                let buffer_range = convert_link_range_to_buffer(
                    &lines,
                    cols,
                    &Range { start_column: 1, start_line_number: 1, end_column: 1 + text.len() as i32, end_line_number: 1 },
                    start_line,
                );

                let mut simple_link = ITerminalSimpleLink::new(link.to_string_lossy(), buffer_range, type_);
                simple_link.uri = Some(link_stat.uri);
                simple_link.selection = Some(TextEditorSelection {
                    start_line_number: parse_int(&line),
                    start_column: col.as_ref().map_or(1.0, parse_int),
                    end_line_number: None,
                    end_column: None,
                });
                simple_link.disable_trim_colon = Some(true);
                self.trace("terminalMultiLineLinkDetector#detect verified link", &simple_link.text);
                links.push(simple_link);

                // Break on the first match
                break;
            }
        }

        if links.is_empty() {
            for matcher in GIT_DIFF_MATCHERS {
                let regex = RegExp::from_str(matcher, "").expect("a expressão do diff é válida");
                let Some(found) = regex.exec(text.units(), 0) else {
                    continue;
                };
                let link = named_group(text.units(), &found, "link").filter(|link| !link.is_empty());
                let to_file_line = named_group(text.units(), &found, "toFileLine");
                let to_file_count = named_group(text.units(), &found, "toFileCount");
                let (Some(link), Some(to_file_line)) = (link, to_file_line) else {
                    continue;
                };

                // Don't try resolve any links of excessive length
                if link.len() > MAX_RESOLVED_LINK_LENGTH {
                    continue;
                }

                self.trace("terminalMultiLineLinkDetector#detect candidate", &link.to_string_lossy());

                // Scan up looking for the first line that could be a path
                let plus_path = RegExp::from_str(r"\+\+\+ b\/(?<path>.+)", "").expect("a expressão do caminho é válida");
                let possible_path = self.scan_up_for_path(start_line, |text| {
                    plus_path.exec(text.units(), 0).map(|found| named_group(text.units(), &found, "path"))
                });
                let Some(possible_path) = possible_path.filter(|path| !path.is_empty()) else {
                    continue;
                };

                // Check if the first non-matching line is an absolute or relative link
                let link_stat = self.link_resolver.resolve_link(&self.process_manager, &possible_path.to_string_lossy(), None).await?;
                if let Some(link_stat) = link_stat {
                    let type_ = get_terminal_link_type(&link_stat.uri, link_stat.is_directory, self.uri_identity_service.as_ref(), self.workspace_context_service.as_ref());

                    // Convert the link to the buffer range
                    let buffer_range = convert_link_range_to_buffer(
                        &lines,
                        cols,
                        &Range { start_column: 1, start_line_number: 1, end_column: 1 + link.len() as i32, end_line_number: 1 },
                        start_line,
                    );

                    let to_file_line = parse_int(&to_file_line);
                    let mut simple_link = ITerminalSimpleLink::new(link.to_string_lossy(), buffer_range, type_);
                    simple_link.uri = Some(link_stat.uri);
                    simple_link.selection = Some(TextEditorSelection {
                        start_line_number: to_file_line,
                        start_column: 1.0,
                        end_line_number: Some(to_file_line + to_file_count.as_ref().map_or(f64::NAN, parse_int)),
                        end_column: None,
                    });
                    self.trace("terminalMultiLineLinkDetector#detect verified link", &simple_link.text);
                    links.push(simple_link);

                    // Break on the first match
                    break;
                }
            }
        }

        Ok(links)
    }
}

impl ITerminalLinkDetector for TerminalMultiLineLinkDetector {
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
