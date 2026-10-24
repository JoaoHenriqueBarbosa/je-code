//! Porte de `src/vs/platform/terminal/common/terminalDataBuffering.ts`: o [`TerminalDataBufferer`], que junta os
//! eventos de dados de um terminal e os entrega em lote, para reduzir a quantidade de mensagens entre as threads.
//!
//! Divergências:
//! - `startBuffering(id, event: Event<string | IProcessDataEvent>)` recebe um `Event<T>` e o `to_data` que extrai o texto
//!   (`isString(e) ? e : e.data`); para o `Event<ProcessData>` do processo, `to_data` é `ProcessData::data`.
//! - O `_callback` é um `Rc<dyn Fn(u32, &str)>` e o `Timeout` do `setTimeout` é o [`TimerId`] do `je-base`.
//! - O `TerminalDataBuffer` (a interface com `data`, `timeoutId` e `dispose`) é um struct privado; o `dispose` do
//!   buffer limpa o temporizador, entrega o que está acumulado e remove o ouvinte do evento.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::runtime::{self, TimerId};

/// `TerminalDataBuffer`.
struct TerminalDataBuffer {
    data: RefCell<Vec<String>>,
    timeout_id: TimerId,
    /// O ouvinte do evento, que o `dispose` do buffer remove.
    disposable: DisposableRef,
}

struct Inner {
    terminal_buffer_map: RefCell<HashMap<u32, Rc<TerminalDataBuffer>>>,
    callback: Rc<dyn Fn(u32, &str)>,
}

impl Inner {
    /// `flushBuffer(id)`.
    fn flush_buffer(&self, id: u32) {
        let buffer = self.terminal_buffer_map.borrow_mut().remove(&id);
        if let Some(buffer) = buffer {
            let data = buffer.data.borrow().join("");
            (self.callback)(id, &data);
        }
    }

    /// O `dispose` do `TerminalDataBuffer`.
    fn dispose_buffer(&self, id: u32, buffer: &TerminalDataBuffer) {
        runtime::clear_timeout(buffer.timeout_id);
        self.flush_buffer(id);
        buffer.disposable.dispose();
    }
}

/// `TerminalDataBufferer`.
pub struct TerminalDataBufferer {
    inner: Rc<Inner>,
}

impl TerminalDataBufferer {
    pub fn new(callback: impl Fn(u32, &str) + 'static) -> Self {
        Self { inner: Rc::new(Inner { terminal_buffer_map: RefCell::new(HashMap::new()), callback: Rc::new(callback) }) }
    }

    /// `startBuffering(id, event, throttleBy = 5)`: devolve o ouvinte do evento.
    pub fn start_buffering<T: Clone + 'static>(&self, id: u32, event: &Event<T>, to_data: impl Fn(&T) -> String + 'static, throttle_by: Option<u64>) -> DisposableRef {
        let throttle_by = throttle_by.unwrap_or(5);
        // O ouvinte ainda não existe quando o fecho nasce: o `dispose` do buffer o lê daqui.
        let listener_slot: Rc<RefCell<Option<DisposableRef>>> = Rc::default();

        let inner = self.inner.clone();
        let slot = listener_slot.clone();
        let listener = event.listen(move |e| {
            let data = to_data(e);
            let existing = inner.terminal_buffer_map.borrow().get(&id).cloned();
            if let Some(buffer) = existing {
                buffer.data.borrow_mut().push(data);
                return;
            }

            let flush_inner = inner.clone();
            let timeout_id = runtime::set_timeout(move || flush_inner.flush_buffer(id), throttle_by);
            let disposable = slot.borrow().clone().expect("o ouvinte existe quando o primeiro dado chega");
            let buffer = Rc::new(TerminalDataBuffer { data: RefCell::new(vec![data]), timeout_id, disposable });
            inner.terminal_buffer_map.borrow_mut().insert(id, buffer);
        });
        // O ouvinte devolvido pelo `startBuffering` é o mesmo que o `dispose` do buffer remove.
        *listener_slot.borrow_mut() = Some(listener.clone());
        listener
    }

    /// `stopBuffering(id)`.
    pub fn stop_buffering(&self, id: u32) {
        let buffer = self.inner.terminal_buffer_map.borrow().get(&id).cloned();
        if let Some(buffer) = buffer {
            self.inner.dispose_buffer(id, &buffer);
        }
    }

    /// `flushBuffer(id)`.
    pub fn flush_buffer(&self, id: u32) {
        self.inner.flush_buffer(id);
    }
}

impl IDisposable for TerminalDataBufferer {
    fn dispose(&self) {
        let buffers: Vec<(u32, Rc<TerminalDataBuffer>)> = self.inner.terminal_buffer_map.borrow().iter().map(|(id, buffer)| (*id, buffer.clone())).collect();
        for (id, buffer) in buffers {
            self.inner.dispose_buffer(id, &buffer);
        }
    }
}
