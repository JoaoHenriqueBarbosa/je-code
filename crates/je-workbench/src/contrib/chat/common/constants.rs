//! Porte parcial de `src/vs/workbench/contrib/chat/common/constants.ts`: os dois enums que descrevem onde e em que modo um
//! agente de chat atua (`ChatAgentLocation` e `ChatModeKind`). O resto do arquivo (configurações, permissões e afins) é do chat
//! em si, que está fora do escopo do porte.

/// `ChatAgentLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatAgentLocation {
    /// This is chat, whether it's in the sidebar, a chat editor, or quick chat.
    /// Leaving the values alone as they are in stored data so we don't have to normalize them.
    Chat,
    Terminal,
    Notebook,
    /// EditorInline means inline chat in a text editor.
    EditorInline,
}

impl ChatAgentLocation {
    /// O valor de texto do enum no original (`'panel'`, `'terminal'`, `'notebook'`, `'editor'`).
    pub fn as_str(self) -> &'static str {
        match self {
            ChatAgentLocation::Chat => "panel",
            ChatAgentLocation::Terminal => "terminal",
            ChatAgentLocation::Notebook => "notebook",
            ChatAgentLocation::EditorInline => "editor",
        }
    }
}

/// `ChatModeKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatModeKind {
    Ask,
    Edit,
    Agent,
}

impl ChatModeKind {
    /// O valor de texto do enum no original (`'ask'`, `'edit'`, `'agent'`).
    pub fn as_str(self) -> &'static str {
        match self {
            ChatModeKind::Ask => "ask",
            ChatModeKind::Edit => "edit",
            ChatModeKind::Agent => "agent",
        }
    }
}
