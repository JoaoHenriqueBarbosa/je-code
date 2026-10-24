//! O miolo do `EventEmitter` do node que o `process` usa: a tabela de ouvintes por nome de evento e a emissão.
//!
//! É o mínimo fiel que as fatias de `exit` e `beforeExit` exigem (`on`, `once`, `off`, `emit`, ordem de registro,
//! `once` removido antes da chamada, instantâneo da lista durante a emissão) e, da fatia 5, `prependListener`,
//! `eventNames`, `listeners`, `removeAllListeners`. O evento `newListener`/`removeListener` e o limite de ouvintes ficam
//! em `process_exit.rs`. Falta: nomes que são símbolos e o `MaxListenersExceededWarning` de emissores comuns.
//!
//! DIVERGÊNCIA: o nome do evento é uma chave de texto; símbolo como nome de evento ainda não é suportado.

use std::cell::RefCell;
use std::thread::LocalKey;

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_promise_host::Thrown as CallThrown;
use crate::runtime::js_value::JSValue;

/// Um ouvinte registrado.
struct Listener {
    event: String,
    function: JSValue,
    once: bool,
}

/// Os ouvintes de um emissor, na ordem de registro.
#[derive(Default)]
pub(crate) struct ListenerTable {
    entries: Vec<Listener>,
}

impl ListenerTable {
    /// Os ouvintes registrados são raízes: o emissor os segura enquanto existirem.
    pub(crate) fn visit_gc_roots(&self, marker: &mut crate::runtime::gc::Marker) {
        self.entries.iter().for_each(|listener| marker.value(listener.function));
    }

    /// `emitter.on(event, function)` (ou `once`): entra no fim da lista do evento.
    pub(crate) fn add(&mut self, event: &str, function: JSValue, once: bool) {
        self.entries.push(Listener { event: event.to_owned(), function, once });
    }

    /// `emitter.prependListener(event, function)` (ou `prependOnceListener`): entra no começo da lista do evento.
    pub(crate) fn add_front(&mut self, event: &str, function: JSValue, once: bool) {
        self.entries.insert(0, Listener { event: event.to_owned(), function, once });
    }

    /// `emitter.off(event, function)`: sai a ocorrência mais recente (a última registrada). `true` quando saiu algo.
    pub(crate) fn remove(&mut self, event: &str, function: JSValue) -> bool {
        match self.entries.iter().rposition(|entry| entry.event == event && entry.function == function) {
            Some(position) => {
                self.entries.remove(position);
                true
            }
            None => false,
        }
    }

    /// `emitter.removeAllListeners([event])`: devolve o que saiu, na ordem de registro.
    pub(crate) fn remove_all(&mut self, event: Option<&str>) -> Vec<(String, JSValue)> {
        let (removed, kept): (Vec<Listener>, Vec<Listener>) =
            std::mem::take(&mut self.entries).into_iter().partition(|entry| event.is_none_or(|name| entry.event == name));
        self.entries = kept;
        removed.into_iter().map(|entry| (entry.event, entry.function)).collect()
    }

    /// `emitter.listeners(event)` e `rawListeners(event)`: as funções registradas, na ordem.
    pub(crate) fn functions(&self, event: &str) -> Vec<JSValue> {
        self.entries.iter().filter(|entry| entry.event == event).map(|entry| entry.function).collect()
    }

    /// `emitter.eventNames()`: os nomes com ouvinte, na ordem da primeira aparição.
    pub(crate) fn event_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for entry in &self.entries {
            if !names.contains(&entry.event) {
                names.push(entry.event.clone());
            }
        }
        names
    }

    /// `emitter.listenerCount(event)`.
    pub(crate) fn count(&self, event: &str) -> usize {
        self.entries.iter().filter(|entry| entry.event == event).count()
    }

    /// Os ouvintes de `event` na ordem, cada um com o marcador de `once` (o que `_events` do node expõe).
    pub(crate) fn entries_of(&self, event: &str) -> Vec<(JSValue, bool)> {
        self.entries.iter().filter(|entry| entry.event == event).map(|entry| (entry.function, entry.once)).collect()
    }

    /// O instantâneo dos ouvintes de `event` para uma emissão; os `once` já saem da tabela.
    pub(crate) fn take_for_emit(&mut self, event: &str) -> Vec<JSValue> {
        let functions: Vec<JSValue> = self.entries.iter().filter(|entry| entry.event == event).map(|entry| entry.function).collect();
        self.entries.retain(|entry| !(entry.event == event && entry.once));
        functions
    }
}

/// `emitter.emit(event, ...args)` sobre a tabela `table`: chama cada ouvinte com `this` igual a `this_value`, na
/// ordem; a primeira exceção interrompe a emissão e sobe. `Ok(true)` quando havia ouvinte.
pub(crate) fn emit(
    global_object: &JSGlobalObject,
    table: &'static LocalKey<RefCell<ListenerTable>>,
    this_value: JSValue,
    event: &str,
    args: &[JSValue],
) -> Result<bool, CallThrown> {
    let functions = table.with(|table| table.borrow_mut().take_for_emit(event));
    call_listeners(global_object, &functions, this_value, args)
}

/// Chama cada função de `functions` (já tirada da tabela por `take_for_emit`) com `this_value` e `args`; a primeira
/// exceção interrompe. `Ok(true)` quando havia ouvinte. É o miolo de [`emit`] para quem guarda a tabela fora de um
/// `LocalKey` (o `process_stdio` tem uma tabela por objeto).
pub(crate) fn call_listeners(
    global_object: &JSGlobalObject,
    functions: &[JSValue],
    this_value: JSValue,
    args: &[JSValue],
) -> Result<bool, CallThrown> {
    for function in functions {
        call_microtask(global_object, *function, this_value, args, "callback is not a function")?;
    }
    Ok(!functions.is_empty())
}
