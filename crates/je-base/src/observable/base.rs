//! Porte de `observableInternal/base.ts`: as interfaces do sistema de observáveis.
//!
//! Decisões de tradução (valem para o módulo inteiro):
//! - Um observável de qualquer tipo é visto pelos observadores como [`AnyObservable`] (apagado); o valor
//!   tipado vive em [`Observable<T>`]. O `TChange` do TypeScript é só um tipo "fantasma" (`TChange` serve
//!   para tipar deltas), então aqui ele aparece só onde há código que produz ou consome deltas
//!   ([`Settable`], `IDerivedReader<C>`, `observable_signal`, `run_on_change`...). Mudança "indefinida"
//!   (`undefined`) é `()`.
//! - `T: Clone`: `get()` devolve o valor por cópia (um objeto do JS é um `Rc` aqui, e a igualdade padrão
//!   `===` é [`StrictEquals`](super::deps::StrictEquals)).
//! - Identidade de observável e de observador é o endereço do objeto dentro do `Rc`.
//! - Os `IReader` são objetos (`&dyn IReader`), então a leitura tipada passa por [`Observable::read`], que
//!   chama a leitura apagada [`IReader::read_observable_erased`] com um fecho que faz o `get()`.
//! - Logging e debug do original (`getLogger()`, `DebugLocation`, `@description`) ficam como no-op: o
//!   `getLogger()` do original é sempre `undefined` salvo quando alguém liga o `ConsoleObservableLogger`
//!   (constante `false`) ou o `DevToolsLogger` (variável de ambiente). Nome de debug vem só do nome
//!   explícito e do dono ([`DebugNameData`](super::debug_name::DebugNameData)); não há como ler o
//!   comentário `/** @description */` do código-fonte de um fecho.

use std::any::Any;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::lifecycle::DisposableStore;

pub type ObserverRef = Rc<dyn IObserver>;
pub type ObservableRef = Rc<dyn AnyObservable>;
/// `IObservable<T>`.
pub type IObservable<T> = Rc<dyn Observable<T>>;
/// `ISettableObservable<T, TChange>`.
pub type ISettableObservable<T, C = ()> = Rc<dyn Settable<T, C>>;

/// Uma referência para o grafo de debug: um observável ou um observador.
#[derive(Clone)]
pub enum DebugNode {
    Observable(ObservableRef),
    Observer(ObserverRef),
}

/// O que o `debugGetObservableGraph` lê de um nó (`Info` do original).
#[derive(Clone)]
pub struct DebugInfo {
    pub source_id: usize,
    pub name: String,
    pub kind: &'static str,
    pub value: String,
    pub state: String,
    pub dependencies: Vec<DebugNode>,
    pub observers: Vec<DebugNode>,
}

/// A parte de um `IObservable` que não depende do tipo do valor.
pub trait AnyObservable {
    /// A identidade do objeto (o `===`).
    fn id(&self) -> usize {
        (self as *const Self).cast::<()>() as usize
    }

    /// Força o observável a checar mudanças e reportá-las (por padrão, o mesmo que `get()`).
    fn report_changes(&self);

    /// Idempotente.
    fn add_observer(&self, observer: ObserverRef);

    /// Idempotente.
    fn remove_observer(&self, observer: &ObserverRef);

    fn debug_name(&self) -> String;

    /// O handle compartilhado deste objeto.
    fn to_any_observable(&self) -> ObservableRef;

    /// Informação para o grafo de debug; `None` se o tipo não participa.
    fn debug_info(&self) -> Option<DebugInfo> {
        None
    }
}

/// `IObservable<T>`.
pub trait Observable<T: Clone + 'static>: AnyObservable {
    /// O valor atual. Não pode ser chamado de dentro de `IObserver::handle_change`.
    fn get(&self) -> T;

    fn to_observable(&self) -> IObservable<T>;

    /// `read(reader)`: lê e inscreve o leitor.
    fn read(&self, reader: &dyn IReader) -> T {
        let mut out = None;
        reader.read_observable_erased(self.to_any_observable(), &mut || out = Some(self.get()));
        out.expect("o leitor chama `get` exatamente uma vez")
    }

    /// `read(undefined)` é `get()`.
    fn read_opt(&self, reader: Option<&dyn IReader>) -> T {
        match reader {
            Some(r) => self.read(r),
            None => self.get(),
        }
    }
}

/// `ISettableObservable<T, TChange>`.
pub trait Settable<T: Clone + 'static, C: 'static>: Observable<T> {
    /// `set(value, transaction, change)`. Com uma transação, os observadores reagem quando ela termina.
    fn set_with_change(&self, value: T, tx: Option<&dyn ITransaction>, change: C);

    fn to_settable(&self) -> ISettableObservable<T, C>;
}

/// `set(value, tx)` para os observáveis sem tipo de mudança (`TChange = void`).
pub trait SettableExt<T: Clone + 'static> {
    fn set(&self, value: T, tx: Option<&dyn ITransaction>);
}

impl<T: Clone + 'static, S: Settable<T, ()> + ?Sized> SettableExt<T> for S {
    fn set(&self, value: T, tx: Option<&dyn ITransaction>) {
        self.set_with_change(value, tx, ());
    }
}

/// `IObserver`.
pub trait IObserver {
    /// O observável pode ter mudado e uma transação que o modifica começou. Todo `begin_update` tem um
    /// `end_update` correspondente.
    fn begin_update(&self, observable: &dyn AnyObservable);

    fn end_update(&self, observable: &dyn AnyObservable);

    fn handle_possible_change(&self, observable: &dyn AnyObservable);

    /// `change` é o delta (`&()` quando indefinido).
    fn handle_change(&self, observable: &dyn AnyObservable, change: &dyn Any);

    fn debug_info(&self) -> Option<DebugInfo> {
        None
    }
}

/// `IReader`.
pub trait IReader {
    /// Lê (`add_observer`, `get`, registra a dependência). `get` guarda o valor tipado fora.
    fn read_observable_erased(&self, observable: ObservableRef, get: &mut dyn FnMut());
}

impl dyn IReader + '_ {
    /// `reader.readObservable(observable)`.
    pub fn read_observable<T: Clone + 'static>(&self, observable: &IObservable<T>) -> T {
        observable.read(self)
    }
}

/// `IReaderWithStore`.
pub trait IReaderWithStore: IReader {
    /// Descartado logo antes de o observável recomputar/rerrodar ou de ficar sem observadores.
    fn store(&self) -> Rc<DisposableStore>;

    /// Descartado logo depois de recomputar/rerrodar.
    fn delayed_store(&self) -> Rc<DisposableStore>;
}

/// `ITransaction`.
pub trait ITransaction {
    /// Chama `begin_update` já e `end_update` quando a transação termina.
    fn update_observer(&self, observer: ObserverRef, observable: ObservableRef);
}

/// `handleBugIndicatingErrorRecovery`: o chamador se recuperou de um erro que indica bug.
pub fn handle_bug_indicating_error_recovery(message: &str) {
    let err = Error::new(format!("BugIndicatingErrorRecovery: {message}"));
    errors::on_unexpected_error(&err);
    eprintln!("recovered from an error that indicates a bug: {err}");
}

/// A identidade de um observador.
pub fn observer_id(observer: &ObserverRef) -> usize {
    Rc::as_ptr(observer).cast::<()>() as usize
}

/// A identidade de um observável (handle).
pub fn observable_id(observable: &ObservableRef) -> usize {
    Rc::as_ptr(observable).cast::<()>() as usize
}

/// `change === undefined`. Um observável cujo tipo de mudança é `Rc<dyn Any>` (o `TChange | undefined` do original, com
/// tipos de evento diferentes) representa a mudança indefinida como um `Rc<dyn Any>` que guarda `()`.
pub fn is_undefined_change(change: &dyn Any) -> bool {
    change.is::<()>() || change.downcast_ref::<Rc<dyn Any>>().is_some_and(|inner| (**inner).is::<()>())
}
