//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalUriLinkDetector.ts`: detecta, numa linha
//! (possivelmente quebrada) do buffer, os links que são URIs (o `LinkComputer` do editor), validando os de `file://` contra
//! o sistema de arquivos pelo [`ITerminalLinkResolver`].
//!
//! Decisões de tradução:
//! - `Terminal` do `@xterm/xterm` é o [`ITerminal`]. As dependências injetadas (`ITerminalLogService`, `IUriIdentityService` e
//!   `IWorkspaceContextService`) são parâmetros do [`TerminalUriLinkDetector::new`], como no detector local.
//! - `async detect(...)` é uma `async fn` interna embrulhada por [`async_fn`]; o detector é `Clone` (só `Rc` por dentro).
//! - `computedLink.url` é `URI | string` ([`LinkUrl`]); o `toString()` de um `URI` é o `to_string(false)`.
//! - `/:\d+(:\d+)?$/` (o `_excludeLineAndColSuffix`) roda no `zyarr`.
//! - `new TerminalLinkAdapter(...)` é o [`TerminalLinkAdapter`], o [`ILinkComputerTarget`] de uma linha só.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::js_string::{self, JsString};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_editor::languages::LinkUrl;
use je_editor::link_computer::{ILinkComputerTarget, compute_links};
use je_platform::log::{LogArg, LogLevel, LogServiceRef};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use zyarr::RegExp;

use super::links::{ITerminalLinkDetector, ITerminalLinkResolver, ITerminalSimpleLink, TerminalBuiltinLinkType, TerminalLinkProcessManager};
use super::terminal_link_helpers::{convert_link_range_to_buffer, get_xterm_line_content};
use super::terminal_local_link_detector::get_terminal_link_type;
use crate::contrib::terminal::xterm_buffer::{IBufferLine, ITerminal};

/// `Constants.MaxResolvedLinksInLine`: the maximum number of links in a line to resolve against the file system. This limit is put
/// in place to avoid sending excessive data when remote connections are in place.
const MAX_RESOLVED_LINKS_IN_LINE: i32 = 10;

/// `TerminalUriLinkDetector`.
#[derive(Clone)]
pub struct TerminalUriLinkDetector {
    xterm: Rc<dyn ITerminal>,
    process_manager: TerminalLinkProcessManager,
    link_resolver: Rc<dyn ITerminalLinkResolver>,
    log_service: LogServiceRef,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

impl TerminalUriLinkDetector {
    pub const ID: &'static str = "uri";

    // 2048 is the maximum URL length
    pub const MAX_LINK_LENGTH: i32 = 2048;

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

    async fn detect_inner(&self, lines: Vec<Rc<dyn IBufferLine>>, start_line: i32, end_line: i32) -> Result<Vec<ITerminalSimpleLink>, Error> {
        let mut links: Vec<ITerminalSimpleLink> = Vec::new();

        let link_computer_target = TerminalLinkAdapter { xterm: self.xterm.clone(), line_start: start_line, line_end: end_line };
        let computed_links = compute_links(Some(&link_computer_target));

        let cols = self.xterm.cols();
        let mut resolved_link_count = 0;
        self.trace("terminalUriLinkDetector#detect computedLinks", &computed_links.iter().map(|link| (link.range.start_column, link.range.end_column)).collect::<Vec<_>>());
        for computed_link in &computed_links {
            let buffer_range = convert_link_range_to_buffer(&lines, cols, &computed_link.range, start_line);

            // Check if the link is within the mouse position
            let uri = match &computed_link.url {
                Some(LinkUrl::Text(url)) => Some(URI::parse(exclude_line_and_col_suffix(url), false)),
                Some(LinkUrl::Uri(uri)) => Some(uri.clone()),
                None => None,
            };
            let Some(uri) = uri else {
                continue;
            };

            let text = match &computed_link.url {
                Some(LinkUrl::Text(url)) => url.to_string_lossy(),
                Some(LinkUrl::Uri(uri)) => uri.to_string(false).to_string_lossy(),
                None => String::new(),
            };

            // Don't try resolve any links of excessive length
            if text.encode_utf16().count() > Self::MAX_LINK_LENGTH as usize {
                continue;
            }

            // Handle non-file scheme links
            if *uri.scheme() != schemas::FILE {
                links.push(link_with_uri(text, uri, buffer_range, TerminalBuiltinLinkType::Url));
                continue;
            }

            // Filter out URI with unrecognized authorities
            if uri.authority().len() != 2 && uri.authority().units().last() == Some(&(b':' as u16)) {
                continue;
            }

            // As a fallback URI, treat the authority as local to the workspace. This is required
            // for `ls --hyperlink` support for example which includes the hostname in the URI like
            // `file://Some-Hostname/mnt/c/foo/bar`.
            let mut uri_candidates: Vec<URI> = vec![uri.clone()];
            if !uri.authority().is_empty() {
                uri_candidates.push(URI::from(
                    &UriComponents {
                        scheme: Some(uri.scheme().clone()),
                        authority: None,
                        path: Some(uri.path().clone()),
                        query: Some(uri.query().clone()),
                        fragment: Some(uri.fragment().clone()),
                    },
                    false,
                ));
            }

            // Iterate over all candidates, pushing the candidate on the first that's verified
            self.trace("terminalUriLinkDetector#detect uriCandidates", &uri_candidates.iter().map(|candidate| candidate.to_string(false).to_string_lossy()).collect::<Vec<_>>());
            for uri_candidate in uri_candidates {
                let link_stat = self.link_resolver.resolve_link(&self.process_manager, &text, Some(uri_candidate.clone())).await?;

                // Create the link if validated
                if let Some(link_stat) = link_stat {
                    let type_ = get_terminal_link_type(&uri_candidate, link_stat.is_directory, self.uri_identity_service.as_ref(), self.workspace_context_service.as_ref());
                    // Use computedLink.url if it's a string to retain the line/col suffix
                    let link_text = match &computed_link.url {
                        Some(LinkUrl::Text(url)) => url.to_string_lossy(),
                        _ => link_stat.link.clone(),
                    };
                    let simple_link = link_with_uri(link_text, uri_candidate, buffer_range, type_);
                    self.trace("terminalUriLinkDetector#detect verified link", &simple_link.text);
                    links.push(simple_link);
                    resolved_link_count += 1;
                    break;
                }
            }

            // Stop early if too many links exist in the line
            resolved_link_count += 1;
            if resolved_link_count >= MAX_RESOLVED_LINKS_IN_LINE {
                break;
            }
        }

        Ok(links)
    }
}

/// Um link com o `uri` já resolvido.
fn link_with_uri(text: String, uri: URI, buffer_range: crate::contrib::terminal::xterm_buffer::IBufferRange, type_: TerminalBuiltinLinkType) -> ITerminalSimpleLink {
    let mut link = ITerminalSimpleLink::new(text, buffer_range, type_);
    link.uri = Some(uri);
    link
}

/// `_excludeLineAndColSuffix(path)`: `path.replace(/:\d+(:\d+)?$/, '')`.
fn exclude_line_and_col_suffix(path: &JsString) -> JsString {
    let regex = RegExp::from_str(r":\d+(:\d+)?$", "").expect("a expressão do sufixo é válida");
    match regex.exec(path.units(), 0) {
        Some(found) => JsString::from(js_string::substring(path.units(), 0, found.start() as i64)),
        None => path.clone(),
    }
}

impl ITerminalLinkDetector for TerminalUriLinkDetector {
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

/// `TerminalLinkAdapter`: a linha (quebrada) do buffer como o único documento do `LinkComputer`.
struct TerminalLinkAdapter {
    xterm: Rc<dyn ITerminal>,
    line_start: i32,
    line_end: i32,
}

impl ILinkComputerTarget for TerminalLinkAdapter {
    fn get_line_count(&self) -> i32 {
        1
    }

    fn get_line_content(&self, _line_number: i32) -> JsString {
        get_xterm_line_content(self.xterm.buffer_active().as_ref(), self.line_start, self.line_end, self.xterm.cols())
    }
}
