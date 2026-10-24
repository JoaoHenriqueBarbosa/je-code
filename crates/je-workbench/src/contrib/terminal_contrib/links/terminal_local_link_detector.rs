//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLocalLinkDetector.ts`: detecta, numa linha
//! (possivelmente quebrada) do buffer, os links para arquivos e pastas locais, validando cada candidato contra o
//! sistema de arquivos pelo [`ITerminalLinkResolver`].
//!
//! Decisões de tradução:
//! - `Terminal` do `@xterm/xterm` é o [`ITerminal`] (`cols` e `buffer.active`). As dependências injetadas
//!   (`ITerminalLogService`, `IUriIdentityService` e `IWorkspaceContextService`) são parâmetros do [`TerminalLocalLinkDetector::new`].
//!   O `ITerminalLogService` (um `ILogService` com uma marca de tipo) é o [`LogServiceRef`].
//! - `async detect(...)` é uma `async fn` interna embrulhada por [`async_fn`]; o detector é `Clone` (só `Rc` por dentro) para a
//!   promessa levar a sua própria cópia.
//! - O texto da linha é UTF-16 ([`JsString`]) e os índices contam unidades UTF-16, como o `string` do original; o texto do
//!   link e os candidatos são `String`.
//! - As expressões regulares dos `fallbackMatchers` rodam no `zyarr` (com grupos nomeados), e as demais
//!   (`/^(\.\.[\/\\])+/` e `/[\[\]"'\.]$/`) são varreduras à mão que casam o mesmo texto.
//! - `const enum Constants` são constantes do módulo. `trimRangeMap` (um `Map<string, number>`) é um `HashMap`.
//! - `OS` é a plataforma do hospedeiro (`Platform::detect().os`). O `PathModule` roda sobre o ambiente do processo.
//! - O `HACK: Adjust to 1-based for link API` do original mexe nos campos do intervalo candidato; aqui o intervalo é uma
//!   cópia (`IBufferRange` é `Copy`).

use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::js_string::{self, JsString};
use je_base::path::ProcessEnvironment;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::range::Range;
use je_platform::log::{LogArg, LogLevel, LogServiceRef};
use je_platform::terminal::capabilities::capabilities::{ITerminalCapabilityStore, TerminalCapability};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use zyarr::RegExp;

use super::links::{
    ITerminalLinkDetector, ITerminalLinkResolver, ITerminalSimpleLink, ResolvedLink, TerminalBuiltinLinkType, TerminalLinkProcessManager,
};
use super::terminal_link_helpers::{convert_link_range_to_buffer, get_xterm_line_content, get_xterm_ranges_by_attr, os_path_module, update_link_with_relative_cwd};
use super::terminal_link_parsing::detect_links;
use crate::contrib::terminal::xterm_buffer::{IBufferLine, IBufferRange, ITerminal};

/// `Constants.MaxLineLength`: the max line length to try extract word links from.
const MAX_LINE_LENGTH: usize = 2000;

/// `Constants.MaxResolvedLinksInLine`: the maximum number of links in a line to resolve against the file system. This limit is put
/// in place to avoid sending excessive data when remote connections are in place.
const MAX_RESOLVED_LINKS_IN_LINE: i32 = 10;

/// `Constants.MaxResolvedLinkLength`: the maximum length of a link to resolve against the file system. This limit is put in place
/// to avoid sending excessive data when remote connections are in place.
const MAX_RESOLVED_LINK_LENGTH: usize = 1024;

/// Os `fallbackMatchers`, na ordem do original.
const FALLBACK_MATCHERS: [&str; 6] = [
    // Python style error: File "<path>", line <line>
    r#"^ *File (?<link>"(?<path>.+)"(, line (?<line>\d+))?)"#,
    // Unknown tool #200166: FILE  <path>:<line>:<col>
    r"^ +FILE +(?<link>(?<path>.+)(?::(?<line>\d+)(?::(?<col>\d+))?)?)",
    // Some C++ compile error formats:
    // C:\foo\bar baz(339) : error ...
    // C:\foo\bar baz(339,12) : error ...
    // C:\foo\bar baz(339, 12) : error ...
    // C:\foo\bar baz(339): error ...       [#178584, Visual Studio CL/NVIDIA CUDA compiler]
    // C:\foo\bar baz(339,12): ...
    // C:\foo\bar baz(339, 12): ...
    r"^(?<link>(?<path>.+)\((?<line>\d+)(?:, ?(?<col>\d+))?\)) ?:",
    // C:\foo/bar baz:339 : error ...
    // C:\foo/bar baz:339:12 : error ...
    // C:\foo/bar baz:339: error ...
    // C:\foo/bar baz:339:12: error ...     [#178584, Clang]
    r"^(?<link>(?<path>.+):(?<line>\d+)(?::(?<col>\d+))?) ?:",
    // PowerShell and cmd prompt
    r"^(?:PS\s+)?(?<link>(?<path>[^>]+))>",
    // The whole line is the path
    r"^ *(?<link>(?<path>.+))",
];

/// O texto de um grupo nomeado do casamento, ou `None` se ele não participou (ou ficou vazio, o `!link` do original).
fn named_group(units: &[u16], found: &zyarr::Match, name: &str) -> Option<JsString> {
    found.group(name).map(|range| JsString::from(&units[range]))
}

/// `/^(\.\.[\/\\])+/`: o que sobra depois dos `../` (ou `..\`) do começo, se houver ao menos um.
fn strip_leading_parent_dirs(text: &str) -> Option<&str> {
    let mut rest = text;
    let mut stripped = false;
    while let Some(next) = rest.strip_prefix("..").and_then(|after| after.strip_prefix(['/', '\\'])) {
        rest = next;
        stripped = true;
    }
    stripped.then_some(rest)
}

/// `/[\[\]"'\.]$/`: o texto sem o último caractere, se ele for um dos especiais.
fn remove_special_end_char(text: &str) -> &str {
    text.strip_suffix(['[', ']', '"', '\'', '.']).unwrap_or(text)
}

/// `TerminalLocalLinkDetector`.
#[derive(Clone)]
pub struct TerminalLocalLinkDetector {
    xterm: Rc<dyn ITerminal>,
    capabilities: Rc<dyn ITerminalCapabilityStore>,
    process_manager: TerminalLinkProcessManager,
    link_resolver: Rc<dyn ITerminalLinkResolver>,
    log_service: LogServiceRef,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

impl TerminalLocalLinkDetector {
    pub const ID: &'static str = "local";

    // This was chosen as a reasonable maximum line length given the tradeoff between performance
    // and how likely it is to encounter such a large line length. Some useful reference points:
    // - Window old max length: 260 ($MAX_PATH)
    // - Linux max length: 4096 ($PATH_MAX)
    pub const MAX_LINK_LENGTH: i32 = 500;

    pub fn new(
        xterm: Rc<dyn ITerminal>,
        capabilities: Rc<dyn ITerminalCapabilityStore>,
        process_manager: TerminalLinkProcessManager,
        link_resolver: Rc<dyn ITerminalLinkResolver>,
        log_service: LogServiceRef,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        workspace_context_service: Rc<dyn IWorkspaceContextService>,
    ) -> Self {
        Self { xterm, capabilities, process_manager, link_resolver, log_service, uri_identity_service, workspace_context_service }
    }

    fn trace(&self, message: &str, detail: &dyn std::fmt::Debug) {
        if self.log_service.get_level() == LogLevel::Trace {
            self.log_service.trace_args(message, &[LogArg::from(format!("{detail:?}").as_str())]);
        }
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

        let mut resolved_link_count = 0;

        let os = self.process_manager.os.unwrap_or(Platform::detect().os);
        let parsed_links = detect_links(text.units(), os);
        self.trace("terminalLocalLinkDetector#detect text", &text.to_string_lossy());
        self.trace("terminalLocalLinkDetector#detect parsedLinks", &parsed_links);
        for parsed_link in &parsed_links {
            // Don't try resolve any links of excessive length
            if parsed_link.path.text.len() > MAX_RESOLVED_LINK_LENGTH {
                continue;
            }

            // Convert the link text's string index into a wrapped buffer range
            let start_index = parsed_link.prefix.as_ref().map_or(parsed_link.path.index, |prefix| prefix.index);
            let suffix_length = parsed_link.suffix.as_ref().map_or(0, |suffix| suffix.suffix.text.len());
            let buffer_range = convert_link_range_to_buffer(
                &lines,
                cols,
                &Range {
                    start_line_number: 1,
                    start_column: start_index as i32 + 1,
                    end_line_number: 1,
                    end_column: (parsed_link.path.index + parsed_link.path.text.len() + suffix_length) as i32 + 1,
                },
                start_line,
            );

            // Get a single link candidate if the cwd of the line is known
            let mut link_candidates: Vec<String> = Vec::new();
            let environment = ProcessEnvironment;
            let os_path = os_path_module(os, &environment);
            let path_text = parsed_link.path.text.to_string_lossy();
            let is_uri = path_text.starts_with("file://");
            if os_path.is_absolute(&path_text) || path_text.starts_with('~') || is_uri {
                link_candidates.push(path_text.clone());
            } else {
                if self.capabilities.has(TerminalCapability::CommandDetection) {
                    let absolute_path =
                        update_link_with_relative_cwd(self.capabilities.as_ref(), f64::from(buffer_range.start.y), &path_text, &os_path, self.log_service.as_ref());
                    // Only add a single exact link candidate if the cwd is available, this may cause
                    // the link to not be resolved but that should only occur when the actual file does
                    // not exist. Doing otherwise could cause unexpected results where handling via the
                    // word link detector is preferable.
                    if let Some(absolute_path) = absolute_path {
                        link_candidates.extend(absolute_path);
                    }
                }
                // Fallback to resolving against the initial cwd, removing any relative directory prefixes
                if link_candidates.is_empty() {
                    link_candidates.push(path_text.clone());
                    if let Some(rest) = strip_leading_parent_dirs(&path_text) {
                        link_candidates.push(rest.to_owned());
                    }
                }
            }

            // If any candidates end with special characters that are likely to not be part of the
            // link, add a candidate excluding them.
            let mut trim_range_map: HashMap<String, i32> = HashMap::new();
            let mut special_end_link_candidates: Vec<String> = Vec::new();
            for candidate in &link_candidates {
                let mut previous = candidate.clone();
                let mut removed = remove_special_end_char(&previous).to_owned();
                let mut trim_range = 0;
                while removed != previous {
                    // Only trim the link if there is no suffix, otherwise the underline would be incorrect
                    if parsed_link.suffix.is_none() {
                        trim_range += 1;
                    }
                    special_end_link_candidates.push(removed.clone());
                    trim_range_map.insert(removed.clone(), trim_range);
                    previous = removed;
                    removed = remove_special_end_char(&previous).to_owned();
                }
            }
            link_candidates.extend(special_end_link_candidates);
            self.trace("terminalLocalLinkDetector#detect linkCandidates", &link_candidates);

            // Validate the path and convert to the outgoing type
            let simple_link = self.validate_and_get_link(None, buffer_range, &link_candidates, Some(&trim_range_map)).await?;
            if let Some(mut simple_link) = simple_link {
                simple_link.parsed_link = Some(parsed_link.clone());
                let start = parsed_link.prefix.as_ref().map_or(parsed_link.path.index, |prefix| prefix.index);
                let end = match &parsed_link.suffix {
                    Some(suffix) => suffix.suffix.index + suffix.suffix.text.len(),
                    None => parsed_link.path.index + parsed_link.path.text.len(),
                };
                simple_link.text = JsString::from(js_string::substring(text.units(), start as i64, end as i64)).to_string_lossy();
                self.trace("terminalLocalLinkDetector#detect verified link", &simple_link.text);
                links.push(simple_link);
            }

            // Stop early if too many links exist in the line
            resolved_link_count += 1;
            if resolved_link_count >= MAX_RESOLVED_LINKS_IN_LINE {
                break;
            }
        }

        // Match against the fallback matchers which are mainly designed to catch paths with spaces
        // that aren't possible using the regular mechanism.
        if links.is_empty() {
            for matcher in FALLBACK_MATCHERS {
                let regex = RegExp::from_str(matcher, "").expect("a expressão do fallback é válida");
                let Some(found) = regex.exec(text.units(), 0) else {
                    continue;
                };
                let link = named_group(text.units(), &found, "link").filter(|link| !link.is_empty());
                let path = named_group(text.units(), &found, "path").filter(|path| !path.is_empty());
                let line = named_group(text.units(), &found, "line").filter(|line| !line.is_empty());
                let col = named_group(text.units(), &found, "col").filter(|col| !col.is_empty());
                let (Some(link), Some(path)) = (link, path) else {
                    continue;
                };

                // Don't try resolve any links of excessive length
                if link.len() > MAX_RESOLVED_LINK_LENGTH {
                    continue;
                }

                // Convert the link text's string index into a wrapped buffer range
                let string_index = js_string::index_of(text.units(), link.units(), 0).map_or(-1, |index| index as i32);
                let buffer_range = convert_link_range_to_buffer(
                    &lines,
                    cols,
                    &Range { start_line_number: 1, start_column: string_index + 1, end_line_number: 1, end_column: string_index + link.len() as i32 + 1 },
                    start_line,
                );

                // Validate and add link
                let suffix = match &line {
                    Some(line) => format!(":{}{}", line.to_string_lossy(), col.as_ref().map_or(String::new(), |col| format!(":{}", col.to_string_lossy()))),
                    None => String::new(),
                };
                let path = path.to_string_lossy();
                let simple_link = self.validate_and_get_link(Some(format!("{path}{suffix}")), buffer_range, std::slice::from_ref(&path), None).await?;
                if let Some(simple_link) = simple_link {
                    links.push(simple_link);
                }
            }
        }

        // Sometimes links are styled specially in the terminal like underlined or bolded, try split
        // the line by attributes and test whether it matches a path
        if links.is_empty() {
            let range_candidates = get_xterm_ranges_by_attr(buffer.as_ref(), start_line, end_line, cols);
            for mut range_candidate in range_candidates {
                let mut candidate_text = JsString::new();
                for y in range_candidate.start.y..=range_candidate.end.y {
                    let Some(line) = buffer.get_line(y) else {
                        break;
                    };
                    let line_start_x = if y == range_candidate.start.y { range_candidate.start.x } else { 0 };
                    let line_end_x = if y == range_candidate.end.y { range_candidate.end.x } else { cols - 1 };
                    candidate_text.push_units(line.translate_to_string(Some(false), Some(line_start_x), Some(line_end_x)).units());
                }

                // HACK: Adjust to 1-based for link API
                range_candidate.start.x += 1;
                range_candidate.start.y += 1;
                range_candidate.end.y += 1;

                // Validate and add link
                let candidate_text = candidate_text.to_string_lossy();
                let simple_link = self.validate_and_get_link(Some(candidate_text.clone()), range_candidate, std::slice::from_ref(&candidate_text), None).await?;
                if let Some(simple_link) = simple_link {
                    links.push(simple_link);
                }

                // Stop early if too many links exist in the line
                resolved_link_count += 1;
                if resolved_link_count >= MAX_RESOLVED_LINKS_IN_LINE {
                    break;
                }
            }
        }

        Ok(links)
    }

    async fn validate_link_candidates(&self, link_candidates: &[String]) -> Result<ResolvedLink, Error> {
        for link in link_candidates {
            let uri = link.starts_with("file://").then(|| URI::parse(link.as_str(), false));
            let result = self.link_resolver.resolve_link(&self.process_manager, link, uri).await?;
            if result.is_some() {
                return Ok(result);
            }
        }
        Ok(None)
    }

    /// Validates a set of link candidates and returns a link if validated.
    ///
    /// `link_text` is the link text, this should be `None` to use the link stat value. `trim_range_map` is a map of link
    /// candidates to the amount of buffer range they need trimmed.
    async fn validate_and_get_link(
        &self,
        link_text: Option<String>,
        buffer_range: IBufferRange,
        link_candidates: &[String],
        trim_range_map: Option<&HashMap<String, i32>>,
    ) -> Result<Option<ITerminalSimpleLink>, Error> {
        let link_stat = self.validate_link_candidates(link_candidates).await?;
        let Some(link_stat) = link_stat else {
            return Ok(None);
        };
        let type_ = get_terminal_link_type(&link_stat.uri, link_stat.is_directory, self.uri_identity_service.as_ref(), self.workspace_context_service.as_ref());

        // Offset the buffer range if the link range was trimmed
        let mut buffer_range = buffer_range;
        let trim_range = trim_range_map.and_then(|map| map.get(&link_stat.link)).copied().unwrap_or(0);
        if trim_range != 0 {
            buffer_range.end.x -= trim_range;
            if buffer_range.end.x < 0 {
                buffer_range.end.y -= 1;
                buffer_range.end.x += self.xterm.cols();
            }
        }

        let mut link = ITerminalSimpleLink::new(link_text.unwrap_or_else(|| link_stat.link.clone()), buffer_range, type_);
        link.uri = Some(link_stat.uri);
        Ok(Some(link))
    }
}

impl ITerminalLinkDetector for TerminalLocalLinkDetector {
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

/// `getTerminalLinkType(uri, isDirectory, uriIdentityService, workspaceContextService)`.
pub fn get_terminal_link_type(
    uri: &URI,
    is_directory: bool,
    uri_identity_service: &dyn IUriIdentityService,
    workspace_context_service: &dyn IWorkspaceContextService,
) -> TerminalBuiltinLinkType {
    if is_directory {
        // Check if directory is inside workspace
        let folders = workspace_context_service.get_workspace().folders();
        for folder in &folders {
            if uri_identity_service.ext_uri().is_equal_or_parent(uri, &folder.uri, false, false) {
                return TerminalBuiltinLinkType::LocalFolderInWorkspace;
            }
        }
        TerminalBuiltinLinkType::LocalFolderOutsideWorkspace
    } else {
        TerminalBuiltinLinkType::LocalFile
    }
}
