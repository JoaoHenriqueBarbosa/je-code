//! Trecho de `src/vs/platform/browserView/common/browserView.ts` de que o `browserViewGroup.ts` depende:
//! quem controla uma visão do navegador ([`BrowserViewOwner`]) e a quem ela é concedida
//! ([`BrowserViewAudience`]), com as comparações.
//!
//! Divergências:
//! - O resto do `browserView.ts` (estado da visão, opções de criação, serviço de IPC, `IBrowserViewHost`,
//!   `IBrowserViewCreationContext` e seus tipos de sessão e telemetria) não foi portado: depende de
//!   `VSBuffer`, `Event` por canal de IPC, `ISandboxNetworkRestrictions` e do navegador em si, e nada disso
//!   é lógica pura.
//! - `IBrowserViewOwner` (`{ type: 'user' } | { type: 'agent'; sessionId }`) é o `enum`
//!   [`BrowserViewOwner`], e `IBrowserViewAgentAudience`/`IBrowserViewAudience` (único tipo hoje:
//!   `'agent'`, com `sessionId` opcional que vale como curinga) é o `enum` [`BrowserViewAudience`].
//!   O campo `type` das duas uniões é a escolha da variante, então `first.type === second.type` é o
//!   casamento das variantes.

/// `IBrowserViewOwner`: identifica quem controla uma visão do navegador.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserViewOwner {
    User,
    Agent { session_id: String },
}

/// `IBrowserViewAudience` (`IBrowserViewAgentAudience`): concede acesso aos agentes que casam. O
/// identificador omitido (`None`) casa com qualquer valor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserViewAudience {
    Agent { session_id: Option<String> },
}

/// `equalsBrowserViewAudience`.
pub fn equals_browser_view_audience(first: &BrowserViewAudience, second: &BrowserViewAudience) -> bool {
    first == second
}

/// `matchesBrowserViewAudience`: se uma audiência satisfaz um padrão cujos identificadores omitidos
/// são curingas.
pub fn matches_browser_view_audience(candidate: &BrowserViewAudience, pattern: &BrowserViewAudience) -> bool {
    let (BrowserViewAudience::Agent { session_id: candidate_session }, BrowserViewAudience::Agent { session_id: pattern_session }) = (candidate, pattern);
    pattern_session.is_none() || pattern_session == candidate_session
}
