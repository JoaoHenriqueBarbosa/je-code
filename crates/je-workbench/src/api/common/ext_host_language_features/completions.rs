//! `CompletionsAdapter` de `extHostLanguageFeatures.ts`.
//!
//! Divergências:
//! - O `StopWatch` é o de `je_base::stopwatch`; o `duration` é `sw.elapsed()`.
//! - O `x` de `ISuggestDataDto` e de `ISuggestResultDto` é `cache_id` (ver a nota da região no protocolo).
//! - `objects.equals(dto1.commandArguments, dto2.commandArguments)` compara as duas listas de argumentos elemento a
//!   elemento com o `equals` profundo de `je_base::objects`; ausente só é igual a ausente.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::objects::equals;
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;
use je_base::uri::URI;
use je_editor::languages::{
    CompletionContext as EditorCompletionContext, CompletionItemLabel as EditorCompletionItemLabel, CompletionItemRange, CompletionLabel,
    completion_item_insert_text_rule,
};
use je_editor::position::Position as EditorPosition;
use je_extensions::scanner::ExtensionDescription;

use crate::api::common::cache::Cache;
use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{
    ApiCompletionItemProvider, ApiCompletionList, ApiCompletionResult, CompletionItemRef, IExtHostApiDeprecationService,
};
use crate::api::common::ext_host_language_features_converters::{CompletionContext, CompletionItemKind, CompletionItemTag};
use crate::api::common::ext_host_protocol::{ChainedCacheId, ISuggestDataDto, ISuggestDefaultRangesDto, ISuggestResultDto};
use crate::api::common::ext_host_type_converters::{self as type_convert, ICommandsConverter, MarkdownString};
use crate::api::common::ext_host_types::{self as types, CompletionLabel as ApiCompletionLabel, CompletionRange, InsertText};

/// `CompletionsAdapter`.
pub struct CompletionsAdapter {
    cache: Cache<CompletionItemRef>,
    disposables: RefCell<HashMap<u32, Rc<DisposableStore>>>,
    documents: Rc<ExtHostDocuments>,
    commands: Rc<dyn ICommandsConverter>,
    provider: Rc<dyn ApiCompletionItemProvider>,
    api_deprecation: Rc<dyn IExtHostApiDeprecationService>,
    extension: Rc<ExtensionDescription>,
}

/// `objects.equals(a, b)` sobre `unknown[] | undefined`.
fn arguments_equal(a: &Option<Vec<Dynamic>>, b: &Option<Vec<Dynamic>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| equals(x, y)),
        _ => false,
    }
}

impl CompletionsAdapter {
    /// `CompletionsAdapter.supportsResolving(provider)`.
    pub fn supports_resolving(provider: &dyn ApiCompletionItemProvider) -> bool {
        provider.has_resolve_completion_item()
    }

    pub fn new(
        documents: Rc<ExtHostDocuments>,
        commands: Rc<dyn ICommandsConverter>,
        provider: Rc<dyn ApiCompletionItemProvider>,
        api_deprecation: Rc<dyn IExtHostApiDeprecationService>,
        extension: Rc<ExtensionDescription>,
    ) -> Rc<Self> {
        Rc::new(Self { cache: Cache::new("CompletionItem"), disposables: RefCell::new(HashMap::new()), documents, commands, provider, api_deprecation, extension })
    }

    pub fn provide_completion_items(
        self: &Rc<Self>,
        resource: URI,
        position: EditorPosition,
        context: EditorCompletionContext,
        token: CancellationToken,
    ) -> Promise<Option<ISuggestResultDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            // The default insert/replace ranges. It's important to compute them
            // before asynchronously asking the provider for its results. See
            // https://github.com/microsoft/vscode/issues/83400#issuecomment-546851421
            let replace_range = doc.get_word_range_at_position(&pos, None)?.unwrap_or_else(|| types::Range::new(pos, pos));
            let insert_range = replace_range.with(None, Some(pos));

            let sw = StopWatch::new(None);
            let items_or_list = await_maybe(this.provider.provide_completion_items(&doc, &pos, token.clone(), &CompletionContext::to(&context))).await?;

            let Some(items_or_list) = items_or_list else {
                // undefined and null are valid results
                return Ok(None);
            };

            if token.is_cancellation_requested() {
                // cancelled -> return without further ado, esp no caching
                // of results as they will leak
                return Ok(None);
            }

            let list = match items_or_list {
                ApiCompletionResult::Items(items) => ApiCompletionList { is_incomplete: None, items },
                ApiCompletionResult::List(list) => list,
            };

            // keep result for providers that support resolving
            let pid = if Self::supports_resolving(&*this.provider) { this.cache.add(list.items.clone()) } else { this.cache.add(Vec::new()) };
            let disposables = DisposableStore::new();
            this.disposables.borrow_mut().insert(pid, disposables);

            let mut completions = Vec::with_capacity(list.items.len());
            let result_default_ranges =
                ISuggestDefaultRangesDto { replace: type_convert::Range::from(&replace_range), insert: type_convert::Range::from(&insert_range) };

            for (i, item) in list.items.iter().enumerate() {
                // check for bad completion item first
                let dto = this.convert_completion_item(&item.borrow(), (pid, i as u32), Some(&insert_range), Some(&replace_range))?;
                completions.push(dto);
            }

            Ok(Some(ISuggestResultDto {
                cache_id: Some(pid),
                default_ranges: result_default_ranges,
                completions,
                is_incomplete: if list.is_incomplete == Some(true) { Some(true) } else { None },
                duration: sw.elapsed(),
            }))
        })
    }

    pub fn resolve_completion_item(self: &Rc<Self>, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ISuggestDataDto>> {
        let this = self.clone();
        async_fn(async move {
            if !this.provider.has_resolve_completion_item() {
                return Ok(None);
            }

            let Some(item) = this.cache.get(id.0, id.1 as usize) else {
                return Ok(None);
            };

            let dto1 = this.convert_completion_item(&item.borrow(), id, None, None)?;

            let resolved_item = await_maybe(this.provider.resolve_completion_item(&item, token)).await?;

            let Some(resolved_item) = resolved_item else {
                return Ok(None);
            };

            let dto2 = this.convert_completion_item(&resolved_item.borrow(), id, None, None)?;

            if dto1.insert_text != dto2.insert_text || dto1.insert_text_rules != dto2.insert_text_rules {
                this.api_deprecation.report("CompletionItem.insertText", &this.extension, "extension MAY NOT change 'insertText' of a CompletionItem during resolve");
            }

            if dto1.command_ident != dto2.command_ident || dto1.command_id != dto2.command_id || !arguments_equal(&dto1.command_arguments, &dto2.command_arguments) {
                this.api_deprecation.report("CompletionItem.command", &this.extension, "extension MAY NOT change 'command' of a CompletionItem during resolve");
            }

            Ok(Some(ISuggestDataDto {
                documentation: dto2.documentation,
                detail: dto2.detail,
                additional_text_edits: dto2.additional_text_edits,

                // (fishy) async insertText
                insert_text: dto2.insert_text,
                insert_text_rules: dto2.insert_text_rules,

                // (fishy) async command
                command_ident: dto2.command_ident,
                command_id: dto2.command_id,
                command_arguments: dto2.command_arguments,
                ..dto1
            }))
        })
    }

    pub fn release_completion_items(&self, id: u32) {
        if let Some(disposables) = self.disposables.borrow_mut().remove(&id) {
            disposables.dispose();
        }
        self.cache.delete(id);
    }

    fn convert_completion_item(
        &self,
        item: &types::CompletionItem,
        id: ChainedCacheId,
        default_insert_range: Option<&types::Range>,
        default_replace_range: Option<&types::Range>,
    ) -> Result<ISuggestDataDto, Error> {
        let disposables = self.disposables.borrow().get(&id.0).cloned();
        let Some(disposables) = disposables else {
            return Err(Error::new("DisposableStore is missing..."));
        };

        let command = self.commands.to_internal(item.command.as_ref(), &disposables)?;
        // `item.sortText !== item.label`: um rótulo com detalhes nunca é igual a um texto.
        let label_text = match &item.label {
            ApiCompletionLabel::Text(text) => Some(text.as_str()),
            ApiCompletionLabel::Detailed(_) => None,
        };
        let mut rules = if item.keep_whitespace == Some(true) { completion_item_insert_text_rule::KEEP_WHITESPACE } else { completion_item_insert_text_rule::NONE };
        let command_has_ident = command.as_ref().and_then(|command| command.ident.as_deref()).is_some_and(|ident| !ident.is_empty());
        let mut result = ISuggestDataDto {
            cache_id: Some(id),
            label: match &item.label {
                ApiCompletionLabel::Text(text) => CompletionLabel::Text(text.clone()),
                ApiCompletionLabel::Detailed(label) => CompletionLabel::Item(EditorCompletionItemLabel {
                    label: label.label.clone(),
                    detail: label.detail.clone(),
                    description: label.description.clone(),
                }),
            },
            kind: item.kind.map(CompletionItemKind::from),
            kind_modifier: item.tags.as_ref().map(|tags| tags.iter().copied().map(CompletionItemTag::from).collect()),
            detail: item.detail.clone(),
            documentation: MarkdownString::from_strict(item.documentation.as_ref()),
            sort_text: item.sort_text.clone().filter(|sort_text| Some(sort_text.as_str()) != label_text),
            filter_text: item.filter_text.clone().filter(|filter_text| Some(filter_text.as_str()) != label_text),
            preselect: item.preselect.filter(|preselect| *preselect),
            insert_text: None,
            insert_text_rules: None,
            range: None,
            commit_characters: item.commit_characters.as_ref().map(|characters| characters.join("")),
            additional_text_edits: item.additional_text_edits.as_ref().map(|edits| edits.iter().map(type_convert::TextEdit::from).collect()),
            command_ident: command.as_ref().and_then(|command| command.ident.clone()),
            command_id: command.as_ref().map(|command| command.id.clone()),
            // filled in on main side from $ident
            command_arguments: if command_has_ident { None } else { command.as_ref().and_then(|command| command.arguments.clone()) },
        };

        // 'insertText'-logic
        if let Some(text_edit) = &item.text_edit {
            self.api_deprecation.report("CompletionItem.textEdit", &self.extension, "Use 'CompletionItem.insertText' and 'CompletionItem.range' instead.");
            result.insert_text = Some(text_edit.new_text().to_owned());
        } else {
            match &item.insert_text {
                Some(InsertText::Text(text)) => result.insert_text = Some(text.clone()),
                Some(InsertText::Snippet(snippet)) => {
                    result.insert_text = Some(snippet.value.clone());
                    rules |= completion_item_insert_text_rule::INSERT_AS_SNIPPET;
                }
                None => {}
            }
        }
        result.insert_text_rules = Some(rules);

        // 'overwrite[Before|After]'-logic
        let range = match &item.text_edit {
            Some(text_edit) => Some(CompletionRange::Range(text_edit.range())),
            None => item.range,
        };

        match range {
            Some(CompletionRange::Range(range)) => {
                // "old" range
                result.range = Some(CompletionItemRange::Single(type_convert::Range::from(&range)));
            }
            Some(CompletionRange::InsertReplace { inserting, replacing }) => {
                // ONLY send range when it's different from the default ranges (safe bandwidth)
                let insert_differs = default_insert_range.is_none_or(|default| !default.is_equal(&inserting));
                let replace_differs = default_replace_range.is_none_or(|default| !default.is_equal(&replacing));
                if insert_differs || replace_differs {
                    result.range = Some(CompletionItemRange::InsertReplace {
                        insert: type_convert::Range::from(&inserting),
                        replace: type_convert::Range::from(&replacing),
                    });
                }
            }
            None => {}
        }

        Ok(result)
    }
}
