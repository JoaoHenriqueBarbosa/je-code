//! Porte de `src/vs/platform/terminal/common/capabilities/bufferMarkCapability.ts`: gerencia as marcas
//! ("marks") do buffer, linhas acompanhadas quando outras linhas entram no buffer ou saem dele.
//!
//! Diferenças do original:
//! - Os dois `Map` (`_idToMarkerMap` e `_anonymousMarkers`, este com o `marker.id` numérico como chave) são
//!   [`IndexMap`] compartilhados (`Rc<RefCell<..>>`), porque o tratador de `onDispose` de cada marcador os
//!   remove. A chave numérica é o `id` do marcador como inteiro.
//! - `*markers()` (um gerador) devolve o retrato dos marcadores no momento da chamada: primeiro os com `id`, depois os
//!   anônimos.
//! - `properties?.marker || this._terminal.registerMarker()`: o `||` é o `or_else` do `Option`.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use super::capabilities::{IBufferMarkCapability, IMarkProperties, IMarker, TerminalCapability};
use crate::terminal::xterm_headless::IHeadlessTerminal;

/// `BufferMarkCapability`.
pub struct BufferMarkCapability {
    base: Rc<Disposable>,
    terminal: Rc<dyn IHeadlessTerminal>,
    id_to_marker_map: Rc<RefCell<IndexMap<String, Rc<dyn IMarker>>>>,
    anonymous_markers: Rc<RefCell<IndexMap<i64, Rc<dyn IMarker>>>>,
    on_mark_added: Rc<Emitter<IMarkProperties>>,
}

impl BufferMarkCapability {
    pub fn new(terminal: Rc<dyn IHeadlessTerminal>) -> Rc<Self> {
        let base = Disposable::new();
        let on_mark_added: Rc<Emitter<IMarkProperties>> = Emitter::new();
        base.register(on_mark_added.clone());
        Rc::new(Self { base, terminal, id_to_marker_map: Rc::default(), anonymous_markers: Rc::default(), on_mark_added })
    }
}

impl IDisposable for BufferMarkCapability {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IBufferMarkCapability for BufferMarkCapability {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::BufferMarkDetection
    }

    fn markers(&self) -> Box<dyn Iterator<Item = Rc<dyn IMarker>>> {
        let mut markers: Vec<Rc<dyn IMarker>> = self.id_to_marker_map.borrow().values().cloned().collect();
        markers.extend(self.anonymous_markers.borrow().values().cloned());
        Box::new(markers.into_iter())
    }

    fn on_mark_added(&self) -> Event<IMarkProperties> {
        self.on_mark_added.event()
    }

    fn add_mark(&self, properties: Option<IMarkProperties>) {
        let marker = properties.as_ref().and_then(|properties| properties.marker.clone()).or_else(|| self.terminal.register_marker(0));
        let id = properties.as_ref().and_then(|properties| properties.id.clone());
        let Some(marker) = marker else {
            return;
        };
        match id.as_deref().filter(|id| !id.is_empty()) {
            Some(id) => {
                self.id_to_marker_map.borrow_mut().insert(id.to_owned(), marker.clone());
                let map = self.id_to_marker_map.clone();
                let id = id.to_owned();
                marker.on_dispose().listen(move |_| {
                    map.borrow_mut().shift_remove(&id);
                });
            }
            None => {
                let marker_id = marker.id() as i64;
                self.anonymous_markers.borrow_mut().insert(marker_id, marker.clone());
                let map = self.anonymous_markers.clone();
                marker.on_dispose().listen(move |_| {
                    map.borrow_mut().shift_remove(&marker_id);
                });
            }
        }
        self.on_mark_added.fire(IMarkProperties {
            marker: Some(marker),
            id,
            hidden: properties.as_ref().and_then(|properties| properties.hidden),
            hover_message: properties.as_ref().and_then(|properties| properties.hover_message.clone()),
            disable_command_storage: None,
        });
    }

    fn get_mark(&self, id: &str) -> Option<Rc<dyn IMarker>> {
        self.id_to_marker_map.borrow().get(id).cloned()
    }
}
