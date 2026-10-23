//! A tradução dos eventos do `notify` para os do `@parcel/watcher` (`create`, `update`, `delete`), e o
//! `ignore` do `@parcel/watcher` (que o `notify` não tem).
//!
//! Onde o `notify` obriga a divergir do `@parcel/watcher`:
//! - O `notify` entrega eventos soltos, sem o agrupamento de 50 ms do Parcel; o `RunOnceWorker` de 75 ms do
//!   `ParcelWatcher` (e a `coalesce_events`) faz o resto, como no original.
//! - Um rename vira `delete` do nome antigo e `create` do novo (o Parcel faz o mesmo no inotify). O
//!   `Modify(Name(Both))` repete o par `From`/`To` e é descartado. No macOS (FSEvents) o `notify` só diz
//!   "renomeou" (`Name(Any)`): quem decide é o `stat` (existe, `create`; não existe, `delete`).
//! - O inotify (Linux) só avisa de arquivos criados dentro de uma pasta nova depois de o `notify` registrar
//!   o watch dela, então o que foi criado antes (`cp -r`, `mv` de pasta) chegaria sem aviso. O Parcel varre
//!   a pasta nova; aqui [`scan_new_folder`] faz o mesmo (só no Linux, onde o problema existe; Windows e
//!   macOS entregam tudo). Duplicatas com os eventos reais são inofensivas: a `coalesce_events` as funde.
//! - O `ignore` do Parcel poda a árvore (não registra watch nas pastas ignoradas, o que no inotify poupa o
//!   limite de watches). O `notify` registra a árvore inteira, então aqui o `ignore` só filtra eventos
//!   ([`IgnoreMatcher`]): os arquivos ignorados continuam consumindo watches, mas nada deles sai.

use std::path::Path;

use je_base::glob::{PatternItem, ParsedPattern};
use notify::EventKind;
use notify::event::{ModifyKind, RenameMode};

use crate::files::watcher::{is_linux, parse_watcher_patterns};

use super::super::fs_util::{is_absolute, is_equal, join};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParcelEventType {
    Create,
    Update,
    Delete,
}

/// `parcelWatcher.Event`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParcelEvent {
    pub path: String,
    pub event_type: ParcelEventType,
}

fn is_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// Os eventos do Parcel para um evento do `notify`. `polling` descarta os `update` de pasta (o
/// `getEventsSince` do Parcel não os produz); `ignore` poda a varredura de pastas novas.
pub fn parcel_events(event: &notify::Event, polling: bool, ignore: &IgnoreMatcher) -> Vec<ParcelEvent> {
    if event.need_rescan() {
        return Vec::new(); // o `notify` perdeu eventos e não diz quais; o Parcel também só registra o erro
    }
    let mut events = Vec::new();
    let created = |path: &Path, events: &mut Vec<ParcelEvent>| {
        events.push(ParcelEvent { path: path.to_string_lossy().into_owned(), event_type: ParcelEventType::Create });
        if is_linux() && is_directory(path) {
            scan_new_folder(path, ignore, events);
        }
    };
    let deleted = |path: &Path, events: &mut Vec<ParcelEvent>| {
        events.push(ParcelEvent { path: path.to_string_lossy().into_owned(), event_type: ParcelEventType::Delete });
    };
    match event.kind {
        EventKind::Access(_) => {}
        EventKind::Create(_) => event.paths.iter().for_each(|p| created(p, &mut events)),
        EventKind::Remove(_) => event.paths.iter().for_each(|p| deleted(p, &mut events)),
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => event.paths.iter().for_each(|p| deleted(p, &mut events)),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => event.paths.iter().for_each(|p| created(p, &mut events)),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {} // repete o par From/To
        EventKind::Modify(ModifyKind::Name(_)) => {
            for path in &event.paths {
                if std::fs::symlink_metadata(path).is_ok() { created(path, &mut events) } else { deleted(path, &mut events) }
            }
        }
        EventKind::Modify(_) => {
            for path in &event.paths {
                if polling && is_directory(path) {
                    continue;
                }
                events.push(ParcelEvent { path: path.to_string_lossy().into_owned(), event_type: ParcelEventType::Update });
            }
        }
        EventKind::Any | EventKind::Other => {
            // Sem tipo preciso (alguns eventos do FSEvents): o `stat` decide.
            for path in &event.paths {
                if std::fs::symlink_metadata(path).is_ok() {
                    events.push(ParcelEvent { path: path.to_string_lossy().into_owned(), event_type: ParcelEventType::Update });
                } else {
                    deleted(path, &mut events);
                }
            }
        }
    }
    events
}

/// `create` para tudo o que já existe dentro de uma pasta que acabou de aparecer.
fn scan_new_folder(folder: &Path, ignore: &IgnoreMatcher, events: &mut Vec<ParcelEvent>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let text = path.to_string_lossy().into_owned();
        if ignore.is_ignored(&text) {
            continue;
        }
        events.push(ParcelEvent { path: text, event_type: ParcelEventType::Create });
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            scan_new_folder(&path, ignore, events);
        }
    }
}

/// Um padrão do `ignore` do Parcel é glob (casado com o caminho relativo à raiz) ou caminho (a pasta e
/// tudo abaixo dela). O `is-glob` do original é aproximado: `*`, `?`, ou um par `[]`, `{}`.
fn is_glob(value: &str) -> bool {
    value.contains(['*', '?']) || (value.contains('[') && value.contains(']')) || (value.contains('{') && value.contains('}'))
}

/// O `ignore` do `@parcel/watcher` para uma raiz.
pub struct IgnoreMatcher {
    root: String,
    paths: Vec<String>,
    globs: Vec<ParsedPattern>,
}

impl IgnoreMatcher {
    pub fn new(root: &str, ignore: &[String]) -> Self {
        let mut paths = Vec::new();
        let mut glob_patterns = Vec::new();
        for value in ignore {
            if is_glob(value) {
                glob_patterns.push(PatternItem::Str(value.clone()));
            } else if is_absolute(value) {
                paths.push(value.clone());
            } else {
                paths.push(join(&[root, value]));
            }
        }
        Self { root: root.to_string(), paths, globs: parse_watcher_patterns(root, &glob_patterns, !is_linux()) }
    }

    /// Se o caminho (ou algum ancestral dele abaixo da raiz) é ignorado.
    pub fn is_ignored(&self, path: &str) -> bool {
        if self.paths.iter().any(|ignored| is_equal_or_inside(path, ignored)) {
            return true;
        }
        if self.globs.is_empty() || !is_equal_or_inside(path, &self.root) {
            return false;
        }
        let mut current = path;
        loop {
            if is_equal(current, &self.root) {
                return false;
            }
            if self.globs.iter().any(|glob| glob.matches(current, None)) {
                return true;
            }
            match current.rfind(['/', '\\']) {
                Some(index) if index > 0 => current = &current[..index],
                _ => return false,
            }
        }
    }
}

/// `path` é `base` ou está dentro dele.
fn is_equal_or_inside(path: &str, base: &str) -> bool {
    super::super::fs_util::is_equal_or_parent(path, base)
}
