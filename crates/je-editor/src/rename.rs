//! Porte de `src/vs/editor/contrib/rename/browser/rename.ts`, a parte que decide a renomeação: o `RenameSkeleton` e as
//! funções de módulo `hasProvider`, `prepareRename`, `rawRename` e `rename`.
//!
//! Decisões de tradução:
//! - `RenameLocation & Rejection` é o [`RenameLocation`] (de `range`, `text` e `reject_reason` opcionais) e
//!   `WorkspaceEdit & Rejection` é o [`RenameEdits`] (`workspace_edit` e `reject_reason` opcionais); `{ edits: [],
//!   rejectReason }` é um `RenameEdits` com a edição vazia.
//! - `if (res.rejectReason)` e `if (result.rejectReason)` testam uma `string | undefined`: a cadeia vazia também é falsa.
//! - `RenameSkeleton` é um `Rc` (as `async` do original capturam `this`); o `_providerRenameIdx`, que o laço `for` do
//!   original usa como contador e que `provideRenameEdits` lê depois, é um `Cell`. `_provideRenameEdits`, recursiva em
//!   `i + 1` com `rejects.concat(...)`, é o laço que acumula as razões.
//! - `nls.localize('no result', "No result.")` é o texto original.
//! - `Range.fromPositions(position)` com um só argumento é o intervalo vazio na posição.
//! - A `RenameController`, a `RenameWidget` e as ações (`registerEditorAction`, `registerEditorContribution`,
//!   `registerModelAndPositionCommand('_executeDocumentRenameProvider')`) dependem do editor com DOM e de
//!   `ServicesAccessor`: ficam de fora.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;

use crate::code_action::languages::WorkspaceEdit;
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{RenameEdits, RenameLocation, RenameProvider};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `if (value.rejectReason)`: presente e não vazia.
fn has_reject_reason(reject_reason: &Option<String>) -> bool {
    reject_reason.as_deref().is_some_and(|reason| !reason.is_empty())
}

/// `{ edits: [], rejectReason }`.
fn rejection(reject_reason: Option<String>) -> RenameEdits {
    RenameEdits { workspace_edit: Some(WorkspaceEdit { edits: Vec::new() }), reject_reason }
}

/// `RenameSkeleton`.
struct RenameSkeleton {
    model: Rc<TextModel>,
    position: Position,
    providers: Vec<Rc<dyn RenameProvider>>,
    provider_rename_idx: Cell<usize>,
}

impl RenameSkeleton {
    fn new(model: &Rc<TextModel>, position: Position, registry: &LanguageFeatureRegistry<dyn RenameProvider>) -> Rc<Self> {
        Rc::new(Self { model: Rc::clone(model), position, providers: registry.ordered(model, false), provider_rename_idx: Cell::new(0) })
    }

    fn resolve_rename_location(self: &Rc<Self>, token: &CancellationToken) -> Promise<Option<RenameLocation>> {
        let (this, token) = (Rc::clone(self), token.clone());
        async_fn(async move {
            let mut rejects: Vec<String> = Vec::new();

            for (index, provider) in this.providers.iter().enumerate() {
                this.provider_rename_idx.set(index);
                if !provider.has_resolve_rename_location() {
                    break;
                }
                let Some(res) = provider.resolve_rename_location(&this.model, this.position, &token).await? else {
                    continue;
                };
                if has_reject_reason(&res.reject_reason) {
                    rejects.extend(res.reject_reason);
                    continue;
                }
                return Ok(Some(res));
            }

            // we are here when no provider prepared a location which means we can
            // just rely on the word under cursor and start with the first provider
            this.provider_rename_idx.set(0);

            let reject_reason = if rejects.is_empty() { None } else { Some(rejects.join("\n")) };
            let Some(word) = this.model.get_word_at_position(this.position) else {
                return Ok(Some(RenameLocation { range: Some(Range::from_positions(this.position, this.position)), text: Some(String::new()), reject_reason }));
            };
            Ok(Some(RenameLocation {
                range: Some(Range::new(this.position.line_number, word.start_column, this.position.line_number, word.end_column)),
                text: Some(word.word.to_string_lossy()),
                reject_reason,
            }))
        })
    }

    fn provide_rename_edits(self: &Rc<Self>, new_name: &str, token: &CancellationToken) -> Promise<RenameEdits> {
        let (this, new_name, token) = (Rc::clone(self), new_name.to_owned(), token.clone());
        async_fn(async move {
            let mut rejects: Vec<String> = Vec::new();
            let mut index = this.provider_rename_idx.get();
            loop {
                let Some(provider) = this.providers.get(index) else {
                    return Ok(rejection(Some(rejects.join("\n"))));
                };
                match provider.provide_rename_edits(&this.model, this.position, &new_name, &token).await? {
                    None => rejects.push("No result.".to_owned()),
                    Some(result) if has_reject_reason(&result.reject_reason) => rejects.extend(result.reject_reason),
                    Some(result) => return Ok(result),
                }
                index += 1;
            }
        })
    }
}

/// `hasProvider(registry, model)`.
pub fn has_provider(registry: &LanguageFeatureRegistry<dyn RenameProvider>, model: &Rc<TextModel>) -> bool {
    !registry.ordered(model, false).is_empty()
}

/// `prepareRename(registry, model, position, cancellationToken?)`.
pub fn prepare_rename(
    registry: &LanguageFeatureRegistry<dyn RenameProvider>,
    model: &Rc<TextModel>,
    position: Position,
    cancellation_token: Option<&CancellationToken>,
) -> Promise<Option<RenameLocation>> {
    RenameSkeleton::new(model, position, registry).resolve_rename_location(cancellation_token.unwrap_or(&CancellationToken::None))
}

/// `rawRename(registry, model, position, newName, cancellationToken?)`.
pub fn raw_rename(
    registry: &LanguageFeatureRegistry<dyn RenameProvider>,
    model: &Rc<TextModel>,
    position: Position,
    new_name: &str,
    cancellation_token: Option<&CancellationToken>,
) -> Promise<RenameEdits> {
    RenameSkeleton::new(model, position, registry).provide_rename_edits(new_name, cancellation_token.unwrap_or(&CancellationToken::None))
}

/// `rename(registry, model, position, newName)`.
pub fn rename(registry: &LanguageFeatureRegistry<dyn RenameProvider>, model: &Rc<TextModel>, position: Position, new_name: &str) -> Promise<RenameEdits> {
    let skeleton = RenameSkeleton::new(model, position, registry);
    let new_name = new_name.to_owned();
    async_fn(async move {
        let loc = skeleton.resolve_rename_location(&CancellationToken::None).await?;
        if let Some(loc) = loc.filter(|loc| has_reject_reason(&loc.reject_reason)) {
            return Ok(rejection(loc.reject_reason));
        }
        skeleton.provide_rename_edits(&new_name, &CancellationToken::None).await
    })
}
