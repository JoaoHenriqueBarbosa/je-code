//! Porte parcial de `src/vs/workbench/contrib/terminal/browser/terminalActions.ts`: o par pasta e diretório de trabalho
//! ([`WorkspaceFolderCwdPair`]) e a função [`shrink_workspace_folder_cwd_pairs`], que o `terminalActions.test.ts` exercita.
//!
//! Fica de fora, porque depende de serviços de interface (quick input, comandos, serviço de ações, editor e
//! grupos de terminais) que o porte ainda não tem: `switchTerminalShowTabsTitle`, `sharedWhenClause`, `getCwdForSplit`,
//! `TerminalLaunchHelpAction`, `registerTerminalAction`, `registerContextualInstanceAction`, `registerActiveInstanceAction`,
//! `registerActiveXtermAction`, o registro das ações (`registerTerminalActions`), `pickTerminalCwd`,
//! `resolveWorkspaceFolderCwd`, `focusActiveTerminal`, `renameWithQuickPick`, `toOptionalUri` e `toOptionalString`.
//!
//! Divergências:
//! - **`WorkspaceFolderCwdPair`.** O `folder` é o [`WorkspaceFolder`] do porte (`uri`, `name`, `index`; o `toResource` do
//!   `IWorkspaceFolder` não existe lá). Os pares são compartilhados por `Rc`, porque o original compara os objetos por
//!   identidade (`selectedPairs.has(x)`).
//! - **Igualdade.** O `deepStrictEqual` do teste compara os campos; o [`PartialEq`] do par compara o `folder` (`uri`, `name` e
//!   `index`) e os demais campos, com os URIs pela forma textual (`toString()`).

use std::collections::HashMap;
use std::rc::Rc;

use je_base::uri::URI;
use je_platform::workspace::WorkspaceFolder;

/// `WorkspaceFolderCwdPair`.
#[derive(Clone, Debug)]
pub struct WorkspaceFolderCwdPair {
    pub folder: WorkspaceFolder,
    pub cwd: URI,
    pub is_absolute: bool,
    pub is_overridden: bool,
}

impl PartialEq for WorkspaceFolderCwdPair {
    fn eq(&self, other: &Self) -> bool {
        self.folder.uri.to_string(false).to_string_lossy() == other.folder.uri.to_string(false).to_string_lossy()
            && self.folder.name == other.folder.name
            && self.folder.index == other.folder.index
            && self.cwd.to_string(false).to_string_lossy() == other.cwd.to_string(false).to_string_lossy()
            && self.is_absolute == other.is_absolute
            && self.is_overridden == other.is_overridden
    }
}

/// Drops repeated CWDs, if any, by keeping the one which best matches the workspace folder. It also preserves the original order.
pub fn shrink_workspace_folder_cwd_pairs(pairs: &[Rc<WorkspaceFolderCwdPair>]) -> Vec<Rc<WorkspaceFolderCwdPair>> {
    // `map.set(key, pair)`: a chave guarda o índice do par escolhido (a identidade do objeto no original).
    let mut map: HashMap<String, usize> = HashMap::new();
    for (index, pair) in pairs.iter().enumerate() {
        let key = pair.cwd.to_string(false).to_string_lossy();
        if !map.contains_key(&key) || key == pair.folder.uri.to_string(false).to_string_lossy() {
            map.insert(key, index);
        }
    }
    // `new Set(map.values())` e `pairs.filter(x => selectedPairs.has(x))`: o par é selecionado se é o objeto escolhido
    // de alguma chave (o mesmo objeto repetido na entrada entra todas as vezes).
    pairs
        .iter()
        .filter(|candidate| map.values().any(|&selected| Rc::ptr_eq(&pairs[selected], candidate)))
        .cloned()
        .collect()
}
