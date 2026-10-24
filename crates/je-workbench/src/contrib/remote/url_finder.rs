//! Porte de `src/vs/workbench/contrib/remote/browser/urlFinder.ts`: o `UrlFinder`, que procura na saída dos terminais e
//! no REPL das sessões de depuração as URLs de servidores locais (`localhost`, `127.0.0.1`, `0.0.0.0` ou só `:porta`) e
//! avisa quando acha uma.
//!
//! Divergências:
//! - `ITerminalService`, `ITerminalInstance`, `IDebugService`, `IDebugSession` e `IReplElement` do porte ainda não têm
//!   `onData`, `onDidChangeReplElements`, `getReplElements`, `hasSeparateRepl` nem `parentSession` em forma de teste de
//!   presença. O que o `UrlFinder` lê deles entra como os contratos estreitos [`UrlFinderTerminalService`],
//!   [`UrlFinderTerminalInstance`], [`UrlFinderDebugService`], [`UrlFinderDebugSession`] e [`UrlFinderReplElement`].
//! - A chave do mapa de terminais é o `instanceId` (o original usa o próprio objeto) e a do REPL é o id da sessão, como no
//!   original. A identidade do "tail" do REPL (`element === tail`) é a identidade do ponteiro do `Rc`.
//! - A porta do caso do Python é `Number(...)` de uma sequência de dígitos; aqui é `u64` e uma sequência que não cabe vira
//!   `u64::MAX` (o original daria um número de ponto flutuante enorme, igualmente inválido como porta).
//! - `new URL(match)` é `url::Url::parse` (WHATWG, o mesmo conjunto de regras para esquemas especiais e opacos).
//! - O construtor devolve `Rc<Self>`, e os ouvintes seguram o `UrlFinder` por referência fraca.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::RunOnceWorker;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, IDisposable};
use je_base::strings::remove_ansi_escape_codes;
use url::Url;
use zyarr::RegExp;

/// `{ host: string; port: number }` do `onDidMatchLocalUrl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalUrl {
    pub host: String,
    pub port: u64,
}

/// O que o `UrlFinder` lê de um `ITerminalInstance`.
pub trait UrlFinderTerminalInstance {
    fn instance_id(&self) -> u32;
    fn title(&self) -> String;
    /// `onData`.
    fn on_data(&self) -> Event<String>;
}

/// O que o `UrlFinder` lê de um `ITerminalService`.
pub trait UrlFinderTerminalService {
    fn instances(&self) -> Vec<Rc<dyn UrlFinderTerminalInstance>>;
    fn on_did_create_instance(&self) -> Event<Rc<dyn UrlFinderTerminalInstance>>;
    fn on_did_dispose_instance(&self) -> Event<Rc<dyn UrlFinderTerminalInstance>>;
}

/// `IReplElement`, só o `toString()`.
pub trait UrlFinderReplElement {
    fn to_text(&self) -> String;
}

/// O que o `UrlFinder` lê de um `IDebugSession`.
pub trait UrlFinderDebugSession {
    fn get_id(&self) -> String;
    /// `!!parentSession`.
    fn has_parent_session(&self) -> bool;
    /// `hasSeparateRepl()`.
    fn has_separate_repl(&self) -> bool;
    /// `onDidChangeReplElements`.
    fn on_did_change_repl_elements(&self) -> Event<()>;
    /// `getReplElements()`.
    fn get_repl_elements(&self) -> Vec<Rc<dyn UrlFinderReplElement>>;
}

/// O que o `UrlFinder` lê de um `IDebugService`: `onDidEndSession` entrega a sessão do `{ session }` do original.
pub trait UrlFinderDebugService {
    fn on_did_new_session(&self) -> Event<Rc<dyn UrlFinderDebugSession>>;
    fn on_did_end_session(&self) -> Event<Rc<dyn UrlFinderDebugSession>>;
}

/// A chave de `listeners: Map<ITerminalInstance | string, IDisposable>`.
#[derive(Clone, PartialEq, Eq, Hash)]
enum ListenerKey {
    Terminal(u32),
    DebugSession(String),
}

/// `{ position: number; tail: IReplElement }` de `replPositions`.
#[derive(Clone)]
struct ReplPosition {
    /// `replElements.length - 1`: `-1` com o REPL vazio.
    position: i64,
    /// `replElements[replElements.length - 1]`: `undefined` com o REPL vazio.
    tail: Option<Rc<dyn UrlFinderReplElement>>,
}

/// `UrlFinder`.
pub struct UrlFinder {
    this: Weak<UrlFinder>,
    base: Rc<Disposable>,
    on_did_match_local_url: Rc<Emitter<LocalUrl>>,
    listeners: RefCell<HashMap<ListenerKey, DisposableRef>>,
    terminal_data_workers: Rc<DisposableMap<u32, RunOnceWorker<String>>>,
    repl_positions: RefCell<HashMap<String, ReplPosition>>,
    local_url_regex: RegExp,
    extract_port_regex: RegExp,
    local_python_server_regex: RegExp,
}

impl UrlFinder {
    /// Debounce time in ms before processing accumulated terminal data.
    const DATA_DEBOUNCE_TIMEOUT: u64 = 500;

    /// Maximum amount of data to accumulate before skipping URL detection.
    /// When data exceeds this threshold, it indicates high-throughput scenarios
    /// (like games or animations) where URL detection is unlikely to find useful results.
    const MAX_DATA_LENGTH: usize = 10000;

    /// Local server url pattern matching following urls:
    /// http://localhost:3000/ - commonly used across multiple frameworks
    /// https://127.0.0.1:5001/ - ASP.NET
    /// http://:8080 - Beego Golang
    /// http://0.0.0.0:4000 - Elixir Phoenix
    const LOCAL_URL_REGEX: &'static str = r"\b\w{0,20}(?::\/\/)?(?:localhost|127\.0\.0\.1|0\.0\.0\.0|:\d{2,5})[\w\-\.\~:\/\?\#[\]\@!\$&\(\)\*\+\,\;\=]*";
    const EXTRACT_PORT_REGEX: &'static str = r"(localhost|127\.0\.0\.1|0\.0\.0\.0):(\d{1,5})";
    /// https://github.com/microsoft/vscode-remote-release/issues/3949
    const LOCAL_PYTHON_SERVER_REGEX: &'static str = r"HTTP\son\s(127\.0\.0\.1|0\.0\.0\.0)\sport\s(\d+)";

    const EXCLUDE_TERMINALS: [&'static str; 1] = ["Dev Containers"];

    pub fn new(terminal_service: &Rc<dyn UrlFinderTerminalService>, debug_service: &Rc<dyn UrlFinderDebugService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_match_local_url = Emitter::<LocalUrl>::new();
        base.register(on_did_match_local_url.clone());
        let terminal_data_workers = DisposableMap::<u32, RunOnceWorker<String>>::new();
        base.register(terminal_data_workers.clone());

        let this = Rc::new_cyclic(|this: &Weak<UrlFinder>| Self {
            this: this.clone(),
            base,
            on_did_match_local_url,
            listeners: RefCell::new(HashMap::new()),
            terminal_data_workers,
            repl_positions: RefCell::new(HashMap::new()),
            local_url_regex: RegExp::from_str(Self::LOCAL_URL_REGEX, "gim").expect("a expressão das URLs locais é válida"),
            extract_port_regex: RegExp::from_str(Self::EXTRACT_PORT_REGEX, "").expect("a expressão da porta é válida"),
            local_python_server_regex: RegExp::from_str(Self::LOCAL_PYTHON_SERVER_REGEX, "").expect("a expressão do servidor Python é válida"),
        });

        // Terminal
        for instance in terminal_service.instances() {
            this.register_terminal_instance(instance);
        }
        this.listen(terminal_service.on_did_create_instance(), |this, instance| this.register_terminal_instance(instance.clone()));
        this.listen(terminal_service.on_did_dispose_instance(), |this, instance| {
            let id = instance.instance_id();
            let listener = this.listeners.borrow_mut().remove(&ListenerKey::Terminal(id));
            if let Some(listener) = listener {
                listener.dispose();
            }
            this.terminal_data_workers.delete_and_dispose(&id);
        });

        // Debug
        this.listen(debug_service.on_did_new_session(), |this, session| {
            if !session.has_parent_session() || session.has_separate_repl() {
                let weak = this.this.clone();
                let session_for_listener = session.clone();
                let listener = session.on_did_change_repl_elements().listen(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.process_new_repl_elements(&session_for_listener);
                    }
                });
                this.listeners.borrow_mut().insert(ListenerKey::DebugSession(session.get_id()), listener);
            }
        });
        this.listen(debug_service.on_did_end_session(), |this, session| {
            let listener = this.listeners.borrow_mut().remove(&ListenerKey::DebugSession(session.get_id()));
            if let Some(listener) = listener {
                listener.dispose();
            }
        });

        this
    }

    /// `this._register(event(listener))`, com o `this` seguro por `Weak` para o ouvinte não manter a classe viva.
    fn listen<T: 'static>(&self, event: Event<T>, handler: impl Fn(&UrlFinder, &T) + 'static) {
        let weak = self.this.clone();
        self.base.register(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    /// `onDidMatchLocalUrl`.
    pub fn on_did_match_local_url(&self) -> Event<LocalUrl> {
        self.on_did_match_local_url.event()
    }

    fn register_terminal_instance(&self, instance: Rc<dyn UrlFinderTerminalInstance>) {
        if !Self::EXCLUDE_TERMINALS.contains(&instance.title().as_str()) {
            let weak = self.this.clone();
            let id = instance.instance_id();
            let listener = instance.on_data().listen(move |data| {
                if let Some(this) = weak.upgrade() {
                    this.get_or_create_worker(id).work(data.clone());
                }
            });
            self.listeners.borrow_mut().insert(ListenerKey::Terminal(id), listener);
        }
    }

    fn get_or_create_worker(&self, id: u32) -> Rc<RunOnceWorker<String>> {
        if let Some(worker) = self.terminal_data_workers.get(&id) {
            return worker;
        }
        let weak = self.this.clone();
        let worker = RunOnceWorker::new(
            move |chunks: Vec<String>| {
                if let Some(this) = weak.upgrade() {
                    this.process_terminal_data(&chunks);
                }
            },
            Self::DATA_DEBOUNCE_TIMEOUT,
        );
        self.terminal_data_workers.set(id, worker.clone(), false);
        worker
    }

    fn process_terminal_data(&self, chunks: &[String]) {
        // Skip processing if data exceeds threshold (high-throughput scenario like games)
        let total_length: usize = chunks.iter().map(|chunk| chunk.encode_utf16().count()).sum();
        if total_length > Self::MAX_DATA_LENGTH {
            return;
        }
        self.process_data(&chunks.concat());
    }

    fn process_new_repl_elements(&self, session: &Rc<dyn UrlFinderDebugSession>) {
        let id = session.get_id();
        let old_repl_position = self.repl_positions.borrow().get(&id).cloned();
        let repl_elements = session.get_repl_elements();
        self.repl_positions.borrow_mut().insert(
            id,
            ReplPosition { position: repl_elements.len() as i64 - 1, tail: repl_elements.last().cloned() },
        );

        match old_repl_position {
            None if !repl_elements.is_empty() => {
                for element in &repl_elements {
                    self.process_data(&element.to_text());
                }
            }
            Some(old) if repl_elements.len() as i64 - 1 != old.position => {
                // Process lines until we reach the old "tail"
                for element in repl_elements.iter().rev() {
                    if old.tail.as_ref().is_some_and(|tail| std::ptr::addr_eq(Rc::as_ptr(tail), Rc::as_ptr(element))) {
                        break;
                    }
                    self.process_data(&element.to_text());
                }
            }
            _ => {}
        }
    }

    /// `data.match(localUrlRegex)` com a flag `g`: cada casamento, em ordem.
    fn find_local_url_matches(&self, data: &JsString) -> Vec<JsString> {
        let units = data.units();
        let mut matches = Vec::new();
        let mut start = 0;
        while let Some(found) = self.local_url_regex.exec(units, start) {
            let Some(range) = found.captures[0].clone() else {
                break;
            };
            start = if range.end == range.start { range.end + 1 } else { range.end };
            matches.push(data.slice(range.start, range.end));
            if start > units.len() {
                break;
            }
        }
        matches
    }

    fn process_data(&self, data: &str) {
        // strip ANSI terminal codes
        let data = remove_ansi_escape_codes(JsString::from(data).units());
        let url_matches = self.find_local_url_matches(&data);
        if !url_matches.is_empty() {
            let debugger_listening = data.to_string_lossy().starts_with("Debugger listening on");
            for matched in &url_matches {
                // check if valid url
                let Ok(server_url) = Url::parse(&matched.to_string_lossy()) else {
                    // Not a valid URL
                    continue;
                };
                // check if the port is a valid integer value
                let port = match server_url.port() {
                    Some(port) => Some(u32::from(port)),
                    None => self
                        .extract_port_regex
                        .exec(matched.units(), 0)
                        .and_then(|found| found.captures.get(2).cloned().flatten())
                        .and_then(|range| matched.slice(range.start, range.end).to_string_lossy().parse::<u32>().ok()),
                };
                let Some(port) = port.filter(|port| *port > 0 && *port <= 65535) else {
                    continue;
                };
                // normalize the host name
                let mut host = server_url.host_str().unwrap_or("");
                if host != "0.0.0.0" && host != "127.0.0.1" {
                    host = "localhost";
                }
                // Exclude node inspect, except when using default port
                if port != 9229 && debugger_listening {
                    continue;
                }
                self.on_did_match_local_url.fire(LocalUrl { host: host.to_string(), port: u64::from(port) });
            }
        } else {
            // Try special python case
            let units = data.units();
            if let Some(found) = self.local_python_server_regex.exec(units, 0) {
                let group = |index: usize| found.captures.get(index).cloned().flatten().map(|range| data.slice(range.start, range.end).to_string_lossy());
                if let (Some(host), Some(port)) = (group(1), group(2)) {
                    self.on_did_match_local_url.fire(LocalUrl { host, port: port.parse::<u64>().unwrap_or(u64::MAX) });
                }
            }
        }
    }
}

impl IDisposable for UrlFinder {
    fn dispose(&self) {
        self.base.dispose();
        let listeners: Vec<DisposableRef> = self.listeners.borrow_mut().drain().map(|(_, listener)| listener).collect();
        for listener in listeners {
            listener.dispose();
        }
    }
}
