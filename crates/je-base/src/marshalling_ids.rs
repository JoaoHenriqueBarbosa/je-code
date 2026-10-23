//! Porte de `src/vs/base/common/marshallingIds.ts`: os valores do campo `$mid` que marcam um objeto
//! serializado ao atravessar o protocolo (URI, RegExp, itens de SCM, comentários, terminal, chat...).
//!
//! O `uri` tem a própria constante `MARSHALLED_ID_URI`, com o mesmo valor de [`MarshalledId::Uri`].

/// `MarshalledId`. O primeiro valor é 1, e os demais seguem a ordem do `const enum` do original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MarshalledId {
    Uri = 1,
    Regexp,
    ScmResource,
    ScmResourceGroup,
    ScmProvider,
    CommentController,
    CommentThread,
    CommentThreadInstance,
    CommentThreadReply,
    CommentNode,
    CommentThreadNode,
    TimelineActionContext,
    NotebookCellActionContext,
    NotebookActionContext,
    TerminalContext,
    TestItemContext,
    Date,
    TestMessageMenuArgs,
    ChatViewContext,
    LanguageModelToolResult,
    LanguageModelTextPart,
    LanguageModelThinkingPart,
    LanguageModelPromptTsxPart,
    LanguageModelDataPart,
    AgentSessionContext,
    ChatResponsePullRequestPart,
}

impl MarshalledId {
    const ALL: [MarshalledId; 26] = [
        Self::Uri,
        Self::Regexp,
        Self::ScmResource,
        Self::ScmResourceGroup,
        Self::ScmProvider,
        Self::CommentController,
        Self::CommentThread,
        Self::CommentThreadInstance,
        Self::CommentThreadReply,
        Self::CommentNode,
        Self::CommentThreadNode,
        Self::TimelineActionContext,
        Self::NotebookCellActionContext,
        Self::NotebookActionContext,
        Self::TerminalContext,
        Self::TestItemContext,
        Self::Date,
        Self::TestMessageMenuArgs,
        Self::ChatViewContext,
        Self::LanguageModelToolResult,
        Self::LanguageModelTextPart,
        Self::LanguageModelThinkingPart,
        Self::LanguageModelPromptTsxPart,
        Self::LanguageModelDataPart,
        Self::AgentSessionContext,
        Self::ChatResponsePullRequestPart,
    ];

    /// O id de um `$mid` lido, se for um dos conhecidos.
    pub fn from_i32(value: i32) -> Option<MarshalledId> {
        Self::ALL.into_iter().find(|id| *id as i32 == value)
    }
}
