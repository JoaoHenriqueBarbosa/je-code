//! Porte parcial de `src/vs/workbench/contrib/debug/common/debug.ts`: só o que `debugSource` e `abstractDebugAdapter`
//! usam, o esquema `DEBUG_SCHEME` e o contrato `IDebugAdapter`.
//!
//! Notas de tradução:
//! - `IDebugAdapter` é o [`AbstractDebugAdapter`](super::abstract_debug_adapter::AbstractDebugAdapter): todo
//!   implementador do original (executável, socket, pipe nomeado, extension host, dublê de teste) estende a classe
//!   abstrata, que traz os métodos do contrato (`onError`, `onExit`, `onRequest`, `onEvent`, `sendResponse`,
//!   `sendRequest`) prontos; o trait é o contrato e a implementação padrão ao mesmo tempo.

pub use super::abstract_debug_adapter::AbstractDebugAdapter as IDebugAdapter;

/// `DEBUG_SCHEME`.
pub const DEBUG_SCHEME: &str = "debug";
