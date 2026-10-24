//! `CodeActionAdapter` de `extHostLanguageFeatures.ts` (e o `CustomCodeAction`, que é o campo `is_synthetic` de
//! [`ICodeActionDto`]).
//!
//! Divergências:
//! - O `if (!candidate) continue` do laço não existe: o `Vec<CommandOrCodeAction>` não tem elementos ausentes.
//! - `Selection.isISelection(rangeOrSelection)` é o `match` no enum [`RangeOrSelection`].
//! - O `DisposableStore` de cada sessão é `Rc`, como no `CodeLensAdapter`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::CodeActionContext;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogServiceRef, Logger};

use crate::api::common::cache::Cache;
use crate::api::common::ext_host_diagnostics::ExtHostDiagnostics;
use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{
    ApiCodeActionContext, ApiCodeActionProvider, ApiRangeOrSelection, CommandOrCodeAction, IExtHostApiDeprecationService,
};
use crate::api::common::ext_host_language_features_converters::CodeActionTriggerKind;
use crate::api::common::ext_host_protocol::{ChainedCacheId, ICodeActionDto, ICodeActionListDto, RangeOrSelection, ResolvedCodeActionDto};
use crate::api::common::ext_host_type_converters::{self as type_convert, ICommandsConverter};
use crate::api::common::ext_host_types::CodeActionKind;
use crate::services::extensions::extensions::is_proposed_api_enabled;

/// `CodeActionAdapter`.
pub struct CodeActionAdapter {
    cache: Cache<CommandOrCodeAction>,
    disposables: RefCell<HashMap<u32, Rc<DisposableStore>>>,
    documents: Rc<ExtHostDocuments>,
    commands: Rc<dyn ICommandsConverter>,
    diagnostics: Rc<ExtHostDiagnostics>,
    provider: Rc<dyn ApiCodeActionProvider>,
    log_service: LogServiceRef,
    extension: Rc<ExtensionDescription>,
    api_deprecation: Rc<dyn IExtHostApiDeprecationService>,
}

impl CodeActionAdapter {
    const MAX_CODE_ACTIONS_PER_FILE: usize = 1000;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        documents: Rc<ExtHostDocuments>,
        commands: Rc<dyn ICommandsConverter>,
        diagnostics: Rc<ExtHostDiagnostics>,
        provider: Rc<dyn ApiCodeActionProvider>,
        log_service: LogServiceRef,
        extension: Rc<ExtensionDescription>,
        api_deprecation: Rc<dyn IExtHostApiDeprecationService>,
    ) -> Rc<Self> {
        Rc::new(Self {
            cache: Cache::new("CodeAction"),
            disposables: RefCell::new(HashMap::new()),
            documents,
            commands,
            diagnostics,
            provider,
            log_service,
            extension,
            api_deprecation,
        })
    }

    pub fn provide_code_actions(
        self: &Rc<Self>,
        resource: URI,
        range_or_selection: RangeOrSelection,
        context: CodeActionContext,
        token: CancellationToken,
    ) -> Promise<Option<ICodeActionListDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let ran = match &range_or_selection {
                RangeOrSelection::Selection(selection) => ApiRangeOrSelection::Selection(type_convert::Selection::to(selection)?),
                RangeOrSelection::Range(range) => ApiRangeOrSelection::Range(type_convert::Range::to(range)?),
            };
            let mut all_diagnostics = Vec::new();

            for diagnostic in this.diagnostics.get_diagnostics(&resource) {
                if ran.range().intersection(&diagnostic.range).is_some() {
                    all_diagnostics.push(diagnostic);
                    if all_diagnostics.len() > Self::MAX_CODE_ACTIONS_PER_FILE {
                        break;
                    }
                }
            }

            let code_action_context = ApiCodeActionContext {
                diagnostics: all_diagnostics,
                only: context.only.as_ref().map(|only| CodeActionKind::new(only.as_str())),
                trigger_kind: CodeActionTriggerKind::to(context.trigger),
            };

            let commands_or_actions = await_maybe(this.provider.provide_code_actions(&doc, &ran, &code_action_context, token.clone())).await?;
            let Some(commands_or_actions) = commands_or_actions.filter(|items| !items.is_empty() && !token.is_cancellation_requested()) else {
                return Ok(None);
            };

            let cache_id = this.cache.add(commands_or_actions.clone());
            let disposables = DisposableStore::new();
            this.disposables.borrow_mut().insert(cache_id, disposables.clone());
            let mut actions: Vec<ICodeActionDto> = Vec::with_capacity(commands_or_actions.len());
            for (i, candidate) in commands_or_actions.iter().enumerate() {
                match candidate {
                    CommandOrCodeAction::Command(command) => {
                        // old school: synthetic code action
                        this.api_deprecation.report("CodeActionProvider.provideCodeActions - return commands", &this.extension, "Return 'CodeAction' instances instead.");

                        actions.push(ICodeActionDto {
                            is_synthetic: Some(true),
                            title: command.title.clone(),
                            command: this.commands.to_internal(Some(command), &disposables)?,
                            ..ICodeActionDto::default()
                        });
                    }
                    CommandOrCodeAction::CodeAction(to_convert) => {
                        let to_convert = to_convert.borrow();

                        // new school: convert code action
                        if let Some(only) = &code_action_context.only {
                            match &to_convert.kind {
                                None => this.log_service.warn(&format!(
                                    "{} - Code actions of kind '{}' requested but returned code action does not have a 'kind'. Code action will be dropped. Please set 'CodeAction.kind'.",
                                    this.extension.identifier.value, only.value
                                )),
                                Some(kind) if !only.contains(kind) => this.log_service.warn(&format!(
                                    "{} - Code actions of kind '{}' requested but returned code action is of kind '{}'. Code action will be dropped. Please check 'CodeActionContext.only' to only return requested code actions.",
                                    this.extension.identifier.value, only.value, kind.value
                                )),
                                Some(_) => {}
                            }
                        }

                        // Ensures that this is either a Range[] or an empty array so we don't get Array<Range | undefined>
                        let ranges = to_convert.ranges.clone().unwrap_or_default();

                        let command = match &to_convert.command {
                            Some(command) => this.commands.to_internal(Some(command), &disposables)?,
                            None => None,
                        };
                        let diagnostics = match &to_convert.diagnostics {
                            Some(diagnostics) => Some(diagnostics.iter().map(type_convert::Diagnostic::from).collect::<Result<Vec<_>, _>>()?),
                            None => None,
                        };

                        actions.push(ICodeActionDto {
                            is_synthetic: None,
                            cache_id: Some((cache_id, i as u32)),
                            title: to_convert.title.clone(),
                            command,
                            diagnostics,
                            edit: to_convert.edit.as_ref().map(|edit| type_convert::WorkspaceEdit::from(edit, None)),
                            kind: to_convert.kind.as_ref().map(|kind| kind.value.clone()),
                            is_preferred: to_convert.is_preferred,
                            is_ai: if is_proposed_api_enabled(&this.extension, "codeActionAI") { to_convert.is_ai } else { Some(false) },
                            ranges: if is_proposed_api_enabled(&this.extension, "codeActionRanges") {
                                Some(ranges.iter().map(type_convert::Range::from).collect())
                            } else {
                                None
                            },
                            disabled: to_convert.disabled.as_ref().map(|disabled| disabled.reason.clone()),
                        });
                    }
                }
            }
            Ok(Some(ICodeActionListDto { cache_id, actions }))
        })
    }

    pub fn resolve_code_action(self: &Rc<Self>, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedCodeActionDto> {
        let this = self.clone();
        async_fn(async move {
            let (session_id, item_id) = id;
            let Some(CommandOrCodeAction::CodeAction(item)) = this.cache.get(session_id, item_id as usize) else {
                return Ok(ResolvedCodeActionDto::default()); // code actions only!
            };
            if !this.provider.has_resolve_code_action() {
                return Ok(ResolvedCodeActionDto::default()); // this should not happen...
            }

            let resolved_item = await_maybe(this.provider.resolve_code_action(&item, token)).await?.unwrap_or(item);
            let resolved_item = resolved_item.borrow();

            let resolved_edit = resolved_item.edit.as_ref().map(|edit| type_convert::WorkspaceEdit::from(edit, None));

            let mut resolved_command = None;
            if let Some(command) = &resolved_item.command {
                let disposables = this.disposables.borrow().get(&session_id).cloned();
                if let Some(disposables) = disposables {
                    resolved_command = this.commands.to_internal(Some(command), &disposables)?;
                }
            }

            Ok(ResolvedCodeActionDto { edit: resolved_edit, command: resolved_command })
        })
    }

    pub fn release_code_actions(&self, cached_id: u32) {
        if let Some(disposables) = self.disposables.borrow_mut().remove(&cached_id) {
            disposables.dispose();
        }
        self.cache.delete(cached_id);
    }
}
