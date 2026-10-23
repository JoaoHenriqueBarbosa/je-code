//! A ponte entre os threads do `notify` e o loop de thread único do porte.
//!
//! O `notify` entrega eventos num thread próprio (o laço do inotify, do `ReadDirectoryChangesW`, do
//! FSEvents ou do `PollWatcher`), e o porte inteiro (`Rc`, `RefCell`, `Promise`) só vive no thread do
//! loop de `je_base::runtime` (ver `docs/design.md`). A decisão:
//!
//! - o handler do `notify` só põe `(id do watch, evento)` num canal `std::sync::mpsc`, sem tocar em nada
//!   do porte (o canal é a única coisa que atravessa threads);
//! - um timer do loop, o "pump", drena o canal e entrega cada evento ao handler do watch, já no thread do
//!   loop, onde ele pode usar `Rc` à vontade;
//! - o pump só existe enquanto houver ao menos um watch vivo: o primeiro [`FsWatch::start`] o arma e o
//!   `dispose` do último o cancela. Assim `run_until_idle` termina quando não há watcher, e um app sem
//!   nada observado não acorda o loop à toa.
//!
//! O pump roda a cada [`PUMP_INTERVAL_MS`] ms. O loop de `runtime` não tem como ser acordado de outro
//! thread (o `Waker` só serve a tarefas `async`), então sondar o canal é o jeito de entregar sem mexer no
//! `je-base`; o custo (um timer de 10 ms) é o preço de haver algo observado, e a latência somada (até
//! 10 ms) é pequena frente aos 75 ms de agrupamento que o original já aplica aos eventos.
//!
//! Este módulo é o único que usa o `notify` diretamente. Fica fora do alvo web (não há sistema de
//! arquivos); os clientes de lá são os de `web.rs`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use je_base::lifecycle::IDisposable;
use je_base::runtime::{self, TimerId};
use notify::{Event, RecursiveMode};

/// O intervalo do pump, em ms.
const PUMP_INTERVAL_MS: u64 = 10;

/// O que o `notify` (ou o polling) entrega: um evento, ou um erro já em texto (com o `ENOSPC` reconhecível, que o
/// watcher recursivo trata à parte).
#[derive(Debug)]
pub enum FsEvent {
    Event(Event),
    Error(String),
}

type Handler = Rc<dyn Fn(FsEvent)>;
type Message = (u64, notify::Result<Event>);

struct Bridge {
    tx: Sender<Message>,
    rx: Receiver<Message>,
    handlers: HashMap<u64, Handler>,
    next_id: u64,
    timer: Option<TimerId>,
}

thread_local! {
    static BRIDGE: RefCell<Bridge> = RefCell::new({
        let (tx, rx) = mpsc::channel();
        Bridge { tx, rx, handlers: HashMap::new(), next_id: 0, timer: None }
    });
}

fn error_text(error: &notify::Error) -> String {
    match error.kind {
        // O texto do `ENOSPC` que o `parcelWatcher.ts` procura.
        notify::ErrorKind::MaxFilesWatch => "No space left on device (ENOSPC): the system limit on file watchers was reached".to_string(),
        _ => error.to_string(),
    }
}

/// Um turno do pump: arma o próximo (se ainda há watch) e entrega o que chegou.
fn pump() {
    BRIDGE.with(|bridge| {
        let mut bridge = bridge.borrow_mut();
        bridge.timer = (!bridge.handlers.is_empty()).then(|| runtime::set_timeout(pump, PUMP_INTERVAL_MS));
    });
    loop {
        // O handler pode chamar `start`/`dispose`, então nada fica emprestado durante a chamada.
        let next = BRIDGE.with(|bridge| {
            let bridge = bridge.borrow();
            let (id, result) = bridge.rx.try_recv().ok()?;
            Some((bridge.handlers.get(&id).cloned(), result))
        });
        let Some((handler, result)) = next else { break };
        if let Some(handler) = handler {
            handler(match result {
                Ok(event) => FsEvent::Event(event),
                Err(error) => FsEvent::Error(error_text(&error)),
            });
        }
    }
}

/// Um watch do disco: um `notify::Watcher` com o handler que recebe seus eventos no thread do loop.
pub struct FsWatch {
    id: u64,
    /// O que mantém o watch vivo: o `notify::Watcher` ou o `poll::Stop`.
    watcher: RefCell<Option<Box<dyn std::any::Any>>>,
}

impl FsWatch {
    /// Começa a observar `path`. Com `polling_interval` usa o [`poll`] (o `getEventsSince` do
    /// `@parcel/watcher`); sem ele, o mecanismo do sistema. `ignore` poda a varredura do polling (o
    /// mecanismo do sistema não tem poda). `Err` leva o texto do erro.
    pub fn start(path: &Path, recursive: bool, polling_interval: Option<Duration>, ignore: Vec<String>, handler: Handler) -> Result<Rc<Self>, String> {
        let (id, tx) = BRIDGE.with(|bridge| {
            let mut bridge = bridge.borrow_mut();
            bridge.next_id += 1;
            let id = bridge.next_id;
            bridge.handlers.insert(id, handler);
            if bridge.timer.is_none() {
                bridge.timer = Some(runtime::set_timeout(pump, PUMP_INTERVAL_MS));
            }
            (id, bridge.tx.clone())
        });
        let watch = Rc::new(Self { id, watcher: RefCell::new(None) });
        let send = move |result: notify::Result<Event>| {
            let _ = tx.send((id, result));
        };
        let mode = if recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
        if let Some(interval) = polling_interval {
            return match poll::start(path, interval, ignore, move |event| send(Ok(event))) {
                Ok(stop) => {
                    *watch.watcher.borrow_mut() = Some(Box::new(stop));
                    Ok(watch)
                }
                Err(error) => {
                    watch.dispose();
                    Err(error)
                }
            };
        }
        let created = notify::recommended_watcher(send);
        let result = created.and_then(|mut watcher| {
            notify::Watcher::watch(&mut watcher, path, mode)?;
            Ok(watcher)
        });
        match result {
            Ok(watcher) => {
                *watch.watcher.borrow_mut() = Some(Box::new(watcher));
                Ok(watch)
            }
            Err(error) => {
                watch.dispose();
                Err(error_text(&error))
            }
        }
    }
}

impl IDisposable for FsWatch {
    fn dispose(&self) {
        // Soltar o `notify::Watcher` encerra o thread dele; eventos já no canal são descartados porque
        // o handler sai do mapa.
        drop(self.watcher.borrow_mut().take());
        let _ = BRIDGE.try_with(|bridge| {
            let mut bridge = bridge.borrow_mut();
            bridge.handlers.remove(&self.id);
            if bridge.handlers.is_empty()
                && let Some(timer) = bridge.timer.take()
            {
                runtime::clear_timeout(timer);
            }
        });
    }
}

/// O polling do `@parcel/watcher` (`writeSnapshot` + `getEventsSince`): um thread que varre a árvore a
/// cada intervalo, guarda `(mtime em ns, tamanho, é pasta)` por caminho e emite o que mudou entre duas
/// varreduras. Não usa o `PollWatcher` do `notify`: ele compara `mtime` em segundos inteiros (sem
/// `with_compare_contents`, que lê todos os arquivos a cada volta), então perderia uma escrita no mesmo
/// segundo da anterior, e o `getEventsSince` do Parcel compara o `mtime` com precisão de nanossegundos.
mod poll {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, RecvTimeoutError, Sender};
    use std::thread;
    use std::time::{Duration, SystemTime};

    use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind};
    use notify::{Event, EventKind};

    use super::super::parcel::events::IgnoreMatcher;

    type Snapshot = HashMap<PathBuf, (Option<SystemTime>, u64, bool)>;

    /// Soltar isto desconecta o canal e encerra o thread de varredura (o `unlink(snapshotFile)` do
    /// original). O campo só existe para ser solto.
    #[allow(dead_code)]
    pub struct Stop(Sender<()>);

    fn scan(root: &Path, ignore: &IgnoreMatcher, snapshot: &mut Snapshot) {
        let Ok(entries) = std::fs::read_dir(root) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if ignore.is_ignored(&path.to_string_lossy()) {
                continue;
            }
            let Ok(metadata) = std::fs::symlink_metadata(&path) else { continue };
            let is_dir = metadata.is_dir();
            snapshot.insert(path.clone(), (metadata.modified().ok(), metadata.len(), is_dir));
            if is_dir {
                scan(&path, ignore, snapshot);
            }
        }
    }

    fn take_snapshot(root: &Path, ignore: &IgnoreMatcher) -> Snapshot {
        let mut snapshot = Snapshot::new();
        scan(root, ignore, &mut snapshot);
        snapshot
    }

    pub fn start(root: &Path, interval: Duration, ignore: Vec<String>, emit: impl Fn(Event) + Send + 'static) -> Result<Stop, String> {
        std::fs::metadata(root).map_err(|e| e.to_string())?;
        let root = root.to_path_buf();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let ignore_root = root.to_string_lossy().into_owned();
        // The first snapshot is written before the watcher is ready (the `counter === 1` of the original
        // `startPolling`), so a file created right after `start` returns is always seen as a change.
        let mut previous = take_snapshot(&root, &IgnoreMatcher::new(&ignore_root, &ignore));
        thread::Builder::new().name("je poll watcher".to_string()).spawn(move || {
            let ignore = IgnoreMatcher::new(&ignore_root, &ignore);
            loop {
                match stop_rx.recv_timeout(interval) {
                    Err(RecvTimeoutError::Timeout) => {}
                    _ => return,
                }
                let current = take_snapshot(&root, &ignore);
                for (path, (modified, len, is_dir)) in &current {
                    let kind = match previous.get(path) {
                        None => Some(EventKind::Create(if *is_dir { CreateKind::Folder } else { CreateKind::File })),
                        Some((old_modified, old_len, old_is_dir)) if old_modified != modified || old_len != len || old_is_dir != is_dir => {
                            Some(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
                        }
                        Some(_) => None,
                    };
                    if let Some(kind) = kind {
                        emit(Event::new(kind).add_path(path.clone()));
                    }
                }
                for (path, (_, _, is_dir)) in &previous {
                    if !current.contains_key(path) {
                        emit(Event::new(EventKind::Remove(if *is_dir { RemoveKind::Folder } else { RemoveKind::File })).add_path(path.clone()));
                    }
                }
                previous = current;
            }
        }).map_err(|e| e.to_string())?;
        Ok(Stop(stop_tx))
    }
}
