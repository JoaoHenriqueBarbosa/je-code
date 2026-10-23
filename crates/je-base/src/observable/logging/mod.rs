//! `observableInternal/logging`: o grafo de dependências de debug e a conexão RPC do depurador (`debugger/rpc.ts`).
//! O `ConsoleObservableLogger` e o `DevToolsLogger` (que falam com as ferramentas de desenvolvimento do navegador)
//! não foram portados.

pub mod debug_get_dependency_graph;
pub mod rpc;
