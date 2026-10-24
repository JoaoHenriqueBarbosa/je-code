//! Porte de `src/vs/editor/contrib/codeAction/common/types.ts`: os tipos de ação de código (`CodeActionKind`,
//! `CodeActionAutoApply`, `CodeActionTriggerSource`, `CodeActionFilter`, `CodeActionTrigger`, `CodeActionCommandArgs`,
//! `CodeActionItem`, `CodeActionSet`) e os filtros `mayIncludeActionsOfKind` e `filtersAction`.
//!
//! Decisões de tradução:
//! - `CodeActionKind` (um objeto com os `HierarchicalKind` fixos) é a estrutura [`CodeActionKind`] com uma função
//!   associada por tipo (`CodeActionKind.QuickFix` é `CodeActionKind::quick_fix()`); o valor é sempre novo e igual.
//! - `CodeActionTrigger.type` é `r#type`.
//! - `CodeActionCommandArgs.fromUser(arg: any, defaults)` lê o argumento como [`Dynamic`]: sem objeto (`null`,
//!   `undefined` ou outro tipo) vale o padrão, e cada campo só conta com o tipo certo (`kind` e `apply` texto,
//!   `preferred` booleano).
//! - O `CodeActionItem` é compartilhado por `Rc` (o `resolve` devolve `this`) e guarda a ação como [`CodeActionRef`]
//!   (ver `languages.rs`); `highlightRange` é um `Cell`, porque o consumidor o altera depois da criação.
//! - `CodeActionItem.resolve(token)` é uma função `async` ([`async_fn`]): `await` do `resolveCodeAction` e o
//!   `try`/`catch` que manda o erro para `onUnexpectedExternalError`.
//! - `CodeActionSet extends ActionSet<CodeActionItem>` é o trait [`CodeActionSet`] sobre o [`ActionSet`].

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::on_unexpected_external_error;
use je_base::hierarchical_kind::HierarchicalKind;
use je_base::promise::Promise;

use super::languages::{CodeAction, CodeActionProvider, CodeActionRef, CodeActionTriggerType, Command};
use super::platform::ActionSet;
use crate::position::Position;

/// `CodeActionKind`.
pub struct CodeActionKind;

impl CodeActionKind {
    pub fn quick_fix() -> HierarchicalKind {
        HierarchicalKind::new("quickfix")
    }

    pub fn refactor() -> HierarchicalKind {
        HierarchicalKind::new("refactor")
    }

    pub fn refactor_extract() -> HierarchicalKind {
        Self::refactor().append(&["extract"])
    }

    pub fn refactor_inline() -> HierarchicalKind {
        Self::refactor().append(&["inline"])
    }

    pub fn refactor_move() -> HierarchicalKind {
        Self::refactor().append(&["move"])
    }

    pub fn refactor_rewrite() -> HierarchicalKind {
        Self::refactor().append(&["rewrite"])
    }

    pub fn notebook() -> HierarchicalKind {
        HierarchicalKind::new("notebook")
    }

    pub fn source() -> HierarchicalKind {
        HierarchicalKind::new("source")
    }

    pub fn source_organize_imports() -> HierarchicalKind {
        Self::source().append(&["organizeImports"])
    }

    pub fn source_fix_all() -> HierarchicalKind {
        Self::source().append(&["fixAll"])
    }

    pub fn surround_with() -> HierarchicalKind {
        Self::refactor().append(&["surround"])
    }
}

/// `CodeActionAutoApply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeActionAutoApply {
    IfSingle,
    First,
    Never,
}

impl CodeActionAutoApply {
    /// O valor de texto do original (`'ifSingle'`, `'first'`, `'never'`).
    pub fn value(self) -> &'static str {
        match self {
            Self::IfSingle => "ifSingle",
            Self::First => "first",
            Self::Never => "never",
        }
    }
}

/// `CodeActionTriggerSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeActionTriggerSource {
    Refactor,
    RefactorPreview,
    Lightbulb,
    Default,
    SourceAction,
    QuickFix,
    FixAll,
    OrganizeImports,
    AutoFix,
    QuickFixHover,
    OnSave,
    ProblemsView,
}

impl CodeActionTriggerSource {
    /// O valor de texto do original.
    pub fn value(self) -> &'static str {
        match self {
            Self::Refactor => "refactor",
            Self::RefactorPreview => "refactor preview",
            Self::Lightbulb => "lightbulb",
            Self::Default => "other (default)",
            Self::SourceAction => "source action",
            Self::QuickFix => "quick fix action",
            Self::FixAll => "fix all",
            Self::OrganizeImports => "organize imports",
            Self::AutoFix => "auto fix",
            Self::QuickFixHover => "quick fix hover window",
            Self::OnSave => "save participants",
            Self::ProblemsView => "problems view",
        }
    }
}

/// `CodeActionFilter`.
#[derive(Clone, Debug, Default)]
pub struct CodeActionFilter {
    pub include: Option<HierarchicalKind>,
    pub excludes: Option<Vec<HierarchicalKind>>,
    pub include_source_actions: Option<bool>,
    pub only_include_preferred_actions: Option<bool>,
}

/// `mayIncludeActionsOfKind(filter, providedKind)`.
pub fn may_include_actions_of_kind(filter: &CodeActionFilter, provided_kind: &HierarchicalKind) -> bool {
    // A provided kind may be a subset or superset of our filtered kind.
    if let Some(include) = &filter.include
        && !include.intersects(provided_kind)
    {
        return false;
    }

    if let Some(excludes) = &filter.excludes
        && excludes.iter().any(|exclude| excludes_action(provided_kind, exclude, filter.include.as_ref()))
    {
        return false;
    }

    // Don't return source actions unless they are explicitly requested
    if !filter.include_source_actions.unwrap_or(false) && CodeActionKind::source().contains(provided_kind) {
        return false;
    }

    true
}

/// `filtersAction(filter, action)`.
pub fn filters_action(filter: &CodeActionFilter, action: &CodeAction) -> bool {
    let action_kind = action.kind.as_deref().filter(|kind| !kind.is_empty()).map(HierarchicalKind::new);

    // Filter out actions by kind
    if let Some(include) = &filter.include {
        match &action_kind {
            Some(action_kind) if include.contains(action_kind) => {}
            _ => return false,
        }
    }

    if let (Some(excludes), Some(action_kind)) = (&filter.excludes, &action_kind)
        && excludes.iter().any(|exclude| excludes_action(action_kind, exclude, filter.include.as_ref()))
    {
        return false;
    }

    // Don't return source actions unless they are explicitly requested
    if !filter.include_source_actions.unwrap_or(false)
        && let Some(action_kind) = &action_kind
        && CodeActionKind::source().contains(action_kind)
    {
        return false;
    }

    if filter.only_include_preferred_actions.unwrap_or(false) && !action.is_preferred.unwrap_or(false) {
        return false;
    }

    true
}

fn excludes_action(provided_kind: &HierarchicalKind, exclude: &HierarchicalKind, include: Option<&HierarchicalKind>) -> bool {
    if !exclude.contains(provided_kind) {
        return false;
    }
    if let Some(include) = include
        && exclude.contains(include)
    {
        // The include is more specific, don't filter out
        return false;
    }
    true
}

/// O `context` de `CodeActionTrigger`.
#[derive(Clone, Debug)]
pub struct CodeActionTriggerContext {
    pub not_available_message: String,
    pub position: Position,
}

/// `CodeActionTrigger`.
#[derive(Clone, Debug)]
pub struct CodeActionTrigger {
    pub r#type: CodeActionTriggerType,
    pub trigger_action: CodeActionTriggerSource,
    pub filter: Option<CodeActionFilter>,
    pub auto_apply: Option<CodeActionAutoApply>,
    pub context: Option<CodeActionTriggerContext>,
}

impl CodeActionTrigger {
    /// O literal `{ type, triggerAction }`, sem os campos opcionais.
    pub fn new(r#type: CodeActionTriggerType, trigger_action: CodeActionTriggerSource) -> Self {
        Self { r#type, trigger_action, filter: None, auto_apply: None, context: None }
    }
}

/// Os valores padrão que `CodeActionCommandArgs.fromUser` recebe.
pub struct CodeActionCommandDefaults {
    pub kind: HierarchicalKind,
    pub apply: CodeActionAutoApply,
}

/// `CodeActionCommandArgs`.
#[derive(Clone, Debug)]
pub struct CodeActionCommandArgs {
    pub kind: HierarchicalKind,
    pub apply: CodeActionAutoApply,
    pub preferred: bool,
}

impl CodeActionCommandArgs {
    /// `CodeActionCommandArgs.fromUser(arg, defaults)`.
    pub fn from_user(arg: &Dynamic, defaults: CodeActionCommandDefaults) -> Self {
        if arg.is_nullish() || arg.type_of() != "object" {
            return Self { kind: defaults.kind, apply: defaults.apply, preferred: false };
        }
        Self {
            kind: Self::get_kind_from_user(arg, defaults.kind),
            apply: Self::get_apply_from_user(arg, defaults.apply),
            preferred: Self::get_preferred_user(arg),
        }
    }

    fn get_apply_from_user(arg: &Dynamic, default_auto_apply: CodeActionAutoApply) -> CodeActionAutoApply {
        let apply = match arg.get_str("apply") {
            Dynamic::String(apply) => apply.to_string_lossy().to_lowercase(),
            _ => String::new(),
        };
        match apply.as_str() {
            "first" => CodeActionAutoApply::First,
            "never" => CodeActionAutoApply::Never,
            "ifsingle" => CodeActionAutoApply::IfSingle,
            _ => default_auto_apply,
        }
    }

    fn get_kind_from_user(arg: &Dynamic, default_kind: HierarchicalKind) -> HierarchicalKind {
        match arg.get_str("kind") {
            Dynamic::String(kind) => HierarchicalKind::new(kind.to_string_lossy()),
            _ => default_kind,
        }
    }

    fn get_preferred_user(arg: &Dynamic) -> bool {
        match arg.get_str("preferred") {
            Dynamic::Bool(preferred) => preferred,
            _ => false,
        }
    }
}

/// `CodeActionItem`.
pub struct CodeActionItem {
    pub action: CodeActionRef,
    pub provider: Option<Rc<dyn CodeActionProvider>>,
    pub highlight_range: Cell<Option<bool>>,
}

impl CodeActionItem {
    pub fn new(action: CodeActionRef, provider: Option<Rc<dyn CodeActionProvider>>, highlight_range: Option<bool>) -> Rc<Self> {
        Rc::new(Self { action, provider, highlight_range: Cell::new(highlight_range) })
    }

    /// `resolve(token)`: preenche o `edit` da ação quando o provedor sabe resolvê-lo e ele ainda falta.
    pub fn resolve(self: &Rc<Self>, token: &CancellationToken) -> Promise<Rc<CodeActionItem>> {
        let this = Rc::clone(self);
        let token = token.clone();
        async_fn(async move {
            let needs_edit = this.action.borrow().edit.is_none();
            if needs_edit
                && let Some(provider) = this.provider.clone().filter(|provider| provider.has_resolve_code_action())
            {
                match provider.resolve_code_action(&this.action, &token).await {
                    Ok(Some(action)) => {
                        let edit = action.borrow().edit.clone();
                        this.action.borrow_mut().edit = edit;
                    }
                    Ok(None) => {}
                    Err(err) => on_unexpected_external_error(&err),
                }
            }
            Ok(this)
        })
    }
}

/// `CodeActionSet`: o conjunto de ações de um pedido (`ActionSet<CodeActionItem>` mais a documentação).
pub trait CodeActionSet: ActionSet<Rc<CodeActionItem>> {
    /// `documentation`.
    fn documentation(&self) -> Vec<Command>;
}
