//! Porte de `src/vs/editor/contrib/codeAction/browser/codeActionKeybindingResolver.ts`: descobre o atalho que serve a
//! uma ação de código, a partir dos atalhos dos comandos de ação de código.
//!
//! Decisões de tradução:
//! - `getResolver()` devolve uma função (`(action) => ResolvedKeybinding | undefined`): é o [`ActionKeybindingResolver`],
//!   um `Rc<dyn Fn>`. O `Lazy` do original (a lista só é lida na primeira ação com `kind`) é o [`Lazy`] do `je-base`.
//! - `item.commandArgs` é um [`Dynamic`] e o `commandArgs` especial dos dois comandos embutidos é um objeto com só o
//!   `kind`.
//! - O `reduceRight` do original, que parte do último candidato e fica com o mais específico, é um laço de trás para
//!   frente.
//! - A identidade do atalho que o teste compara com `strictEqual` é o `Rc` ([`Rc::ptr_eq`]).

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::hierarchical_kind::HierarchicalKind;
use je_base::keybindings::ResolvedKeybinding;
use je_base::lazy::Lazy;
use je_platform::keybinding::IKeybindingService;

use super::code_action::{
    CODE_ACTION_COMMAND_ID, FIX_ALL_COMMAND_ID, ORGANIZE_IMPORTS_COMMAND_ID, REFACTOR_COMMAND_ID, SOURCE_ACTION_COMMAND_ID,
};
use super::languages::CodeAction;
use super::types::{CodeActionAutoApply, CodeActionCommandArgs, CodeActionCommandDefaults, CodeActionKind};

/// `(action: CodeAction) => ResolvedKeybinding | undefined`.
pub type ActionKeybindingResolver = Rc<dyn Fn(&CodeAction) -> Option<Rc<dyn ResolvedKeybinding>>>;

/// `ResolveCodeActionKeybinding`.
#[derive(Clone)]
struct ResolveCodeActionKeybinding {
    kind: HierarchicalKind,
    preferred: bool,
    resolved_keybinding: Rc<dyn ResolvedKeybinding>,
}

/// `CodeActionKeybindingResolver`.
pub struct CodeActionKeybindingResolver {
    keybinding_service: Rc<dyn IKeybindingService>,
}

impl CodeActionKeybindingResolver {
    const CODE_ACTION_COMMANDS: [&'static str; 5] = [
        REFACTOR_COMMAND_ID,
        CODE_ACTION_COMMAND_ID,
        SOURCE_ACTION_COMMAND_ID,
        ORGANIZE_IMPORTS_COMMAND_ID,
        FIX_ALL_COMMAND_ID,
    ];

    pub fn new(keybinding_service: Rc<dyn IKeybindingService>) -> Self {
        Self { keybinding_service }
    }

    pub fn get_resolver(&self) -> ActionKeybindingResolver {
        let keybinding_service = Rc::clone(&self.keybinding_service);
        // Lazy since we may not actually ever read the value
        let all_code_action_bindings: Lazy<Rc<Vec<ResolveCodeActionKeybinding>>> = Lazy::new(move || {
            Rc::new(
                keybinding_service
                    .get_keybindings()
                    .into_iter()
                    .filter(|item| {
                        item.command
                            .as_deref()
                            .is_some_and(|command| Self::CODE_ACTION_COMMANDS.iter().any(|candidate| *candidate == command))
                    })
                    .filter_map(|item| {
                        let resolved_keybinding = item.resolved_keybinding.clone()?;
                        // Special case these commands since they come built-in with VS Code and don't use 'commandArgs'
                        let command_args = match item.command.as_deref() {
                            Some(ORGANIZE_IMPORTS_COMMAND_ID) => Dynamic::object([(
                                "kind",
                                Dynamic::string(&CodeActionKind::source_organize_imports().value),
                            )]),
                            Some(FIX_ALL_COMMAND_ID) => {
                                Dynamic::object([("kind", Dynamic::string(&CodeActionKind::source_fix_all().value))])
                            }
                            _ => item.command_args.clone(),
                        };

                        let args = CodeActionCommandArgs::from_user(
                            &command_args,
                            CodeActionCommandDefaults { kind: HierarchicalKind::none(), apply: CodeActionAutoApply::Never },
                        );
                        Some(ResolveCodeActionKeybinding { resolved_keybinding, kind: args.kind, preferred: args.preferred })
                    })
                    .collect(),
            )
        });

        Rc::new(move |action: &CodeAction| {
            if action.kind.as_deref().is_some_and(|kind| !kind.is_empty()) {
                let binding = Self::best_keybinding_for_code_action(action, &all_code_action_bindings.value());
                return binding.map(|binding| binding.resolved_keybinding);
            }
            None
        })
    }

    fn best_keybinding_for_code_action(
        action: &CodeAction,
        candidates: &[ResolveCodeActionKeybinding],
    ) -> Option<ResolveCodeActionKeybinding> {
        let kind = HierarchicalKind::new(action.kind.as_deref().filter(|kind| !kind.is_empty())?);

        candidates
            .iter()
            .filter(|candidate| candidate.kind.contains(&kind))
            // If the candidate keybinding only applies to preferred actions, the this action must also be preferred
            .filter(|candidate| !candidate.preferred || action.is_preferred.unwrap_or(false))
            .rev()
            .fold(None, |current_best: Option<&ResolveCodeActionKeybinding>, candidate| match current_best {
                None => Some(candidate),
                // Select the more specific binding
                Some(best) if best.kind.contains(&candidate.kind) => Some(candidate),
                Some(best) => Some(best),
            })
            .cloned()
    }
}
