//! O rastreamento de `instantiationService.ts` (a região `tracing`): `Trace`.
//!
//! Desligado (o padrão), todas as operações são no-ops sobre um valor sem alocação. Ligado, cada
//! criação ou invocação acumula a árvore de dependências e `stop()` publica o texto em [`Trace::all`]
//! quando levou mais de 2 ms ou criou algum serviço.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::runtime;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TraceType {
    Creation,
    Invocation,
    Branch,
}

struct TraceInner {
    kind: TraceType,
    name: String,
    start: u64,
    dep: RefCell<Vec<(String, bool, Trace)>>,
}

thread_local! {
    static ALL: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static TOTALS: Cell<f64> = const { Cell::new(0.0) };
}

/// `Trace`.
#[derive(Clone)]
pub struct Trace(Option<Rc<TraceInner>>);

impl Trace {
    fn new(kind: TraceType, name: &str) -> Self {
        Self(Some(Rc::new(TraceInner {
            kind,
            name: name.to_string(),
            start: runtime::date_now(),
            dep: RefCell::new(Vec::new()),
        })))
    }

    /// `Trace.all`: os textos publicados, sem repetição e em ordem de publicação.
    pub fn all() -> Vec<String> {
        ALL.with(|all| all.borrow().clone())
    }

    /// `Trace._None`.
    pub fn none() -> Self {
        Self(None)
    }

    pub fn trace_invocation(enable_tracing: bool, name: &str) -> Self {
        if enable_tracing {
            Self::new(TraceType::Invocation, name)
        } else {
            Self::none()
        }
    }

    pub fn trace_creation(enable_tracing: bool, name: &str) -> Self {
        if enable_tracing {
            Self::new(TraceType::Creation, name)
        } else {
            Self::none()
        }
    }

    /// `branch(id, first)`: `first` diz que esta dependência foi criada por causa deste rastro.
    pub fn branch(&self, id: &str, first: bool) -> Trace {
        let Some(inner) = &self.0 else {
            return self.clone();
        };
        let child = Self::new(TraceType::Branch, id);
        inner
            .dep
            .borrow_mut()
            .push((id.to_string(), first, child.clone()));
        child
    }

    pub fn stop(&self) {
        let Some(inner) = &self.0 else { return };
        let dur = runtime::date_now().saturating_sub(inner.start) as f64;
        let total = TOTALS.with(|t| {
            t.set(t.get() + dur);
            t.get()
        });

        let mut caused_creation = false;
        let children = print_child(1, inner, &mut caused_creation);
        let lines = [
            format!(
                "{} {}",
                if inner.kind == TraceType::Creation {
                    "CREATE"
                } else {
                    "CALL"
                },
                inner.name
            ),
            children,
            format!("DONE, took {dur:.2}ms (grand total {total:.2}ms)"),
        ];

        if dur > 2.0 || caused_creation {
            let text = lines.join("\n");
            ALL.with(|all| {
                if !all.borrow().contains(&text) {
                    all.borrow_mut().push(text);
                }
            });
        }
    }
}

fn print_child(n: usize, trace: &TraceInner, caused_creation: &mut bool) -> String {
    let prefix = "\t".repeat(n);
    let mut res: Vec<String> = Vec::new();
    for (id, first, child) in trace.dep.borrow().iter() {
        if *first {
            *caused_creation = true;
            res.push(format!("{prefix}CREATES -> {id}"));
            if let Some(child) = &child.0 {
                let nested = print_child(n + 1, child, caused_creation);
                if !nested.is_empty() {
                    res.push(nested);
                }
            }
        } else {
            res.push(format!("{prefix}uses -> {id}"));
        }
    }
    res.join("\n")
}
