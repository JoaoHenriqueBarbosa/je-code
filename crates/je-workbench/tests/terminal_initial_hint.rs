//! Porte de `src/vs/workbench/contrib/terminalContrib/inlineHint/test/browser/terminalInitialHint.test.ts` (suíte `Terminal Initial Hint Addon`), caso a caso e na mesma ordem.
//!
//! Nenhum dos 4 casos foi portado, e o `InitialHintAddon` (`terminal.initialHint.contribution.ts`) também não existe na produção.
//! Casos de fora e o motivo exato:
//! - `Chat providers > hint is not shown when there are no chat providers`, `hint is not shown when there is just an editor
//!   agent`, `hint is shown when there is a terminal chat agent` e `hint is not shown again when another terminal chat agent is
//!   added if it has already shown`: o `InitialHintAddon` existe para mostrar a dica de chat do terminal e decide por
//!   `IChatAgent` (`ChatAgentLocation`, `ChatModeKind`, `IChatAgentService.onDidChangeAgents`), isto é, chat e agentes de IA,
//!   que estão fora do escopo do porte (recorte do user). O `onDidRequestCreateHint` só é disparado a partir dos agentes de chat.
