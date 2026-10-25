//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/goToSymbol.ts`: as consultas aos provedores de definição,
//! declaração, implementação, definição de tipo e referências.
//!
//! Decisões de tradução:
//! - `getDefinitionsAtPosition`, `getDeclarationsAtPosition`, `getImplementationsAtPosition` e
//!   `getTypeDefinitionsAtPosition` têm o mesmo corpo (só mudam o registro e o método do provedor, que no porte é um
//!   trait só, [`LocationLinkProvider`]): são a função [`get_location_links_at_position`], chamada com o registro de
//!   cada recurso (`definition_provider`, `declaration_provider`, `implementation_provider`,
//!   `type_definition_provider`). Quatro funções de repasse seriam o código repetido que o projeto não tem.
//! - `getLocationLinks<T>` é [`get_location_links`], genérica no tipo do provedor. O `provide` devolve uma
//!   `ProviderResult<LocationLink | LocationLink[]>`: o trait do porte já devolve a lista (um único `LocationLink` é uma
//!   lista de um), e o `coalesce(values.flat())` descarta só os `None` de cada provedor.
//! - `getReferencesAtPosition` recebe `Location[]` do provedor de referências; cada `Location` é lida como o
//!   `LocationLink` que o original devolve (uma `Location` tem `uri` e `range`, e os campos de origem e alvo
//!   ausentes).
//! - `Promise.resolve(provide(...)).then(undefined, err => { onUnexpectedExternalError(err); return undefined; })` é o
//!   `then_result` que transforma a rejeição em `None`.
//! - Os comandos `_execute*Provider` (`registerModelAndPositionCommand` com `_sortedAndDeduped`) dependem de
//!   `ServicesAccessor` e `IModelService`: ficam de fora.
//!
//! O resto do módulo `src/vs/editor/contrib/gotoSymbol/browser` mora na pasta `goto_symbol/`: `referencesModel.ts`
//! ([`references_model`], a árvore de referências agrupada por arquivo), `symbolNavigation.ts`
//! ([`symbol_navigation`], a navegação pelo teclado entre os resultados e a chave de contexto `hasSymbols`) e
//! `goToCommands.ts` ([`go_to_commands`], `DefinitionAction` e companhia). O `goToDefinitionWithLocation` que o sticky scroll
//! chama não é de `goToCommands.ts`: é de `inlayHints/browser/inlayHintsLocations.ts` ([`crate::inlay_hints_locations`]).

pub mod go_to_commands;
pub mod peek;
pub mod references_model;
pub mod symbol_navigation;

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_external_error;
use je_base::network::{UriOrString, matches_some_scheme, schemas};
use je_base::promise::Promise;
use je_base::uri::URI;

use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{Location, LocationLink, LocationLinkProvider, ReferenceContext, ReferenceProvider};
use crate::position::Position;
use crate::text_model::TextModel;

/// `shouldIncludeLocationLink(sourceModel, loc)`, sobre o `uri` da localização.
fn should_include_uri(source_model: &TextModel, uri: &URI) -> bool {
    // Always allow the location if the request comes from a document with the same scheme.
    if uri.scheme().units() == source_model.uri().scheme().units() {
        return true;
    }

    // Otherwise filter out locations from internal schemes
    !matches_some_scheme(
        UriOrString::Uri(uri),
        &[schemas::WALK_THROUGH_SNIPPET, schemas::VSCODE_CHAT_CODE_BLOCK, schemas::VSCODE_CHAT_CODE_COMPARE_BLOCK],
    )
}

/// `Location` lida como `LocationLink` (ver a nota do módulo).
pub fn location_as_link(location: Location) -> LocationLink {
    LocationLink { origin_selection_range: None, uri: location.uri, range: location.range, target_selection_range: None }
}

/// `getLocationLinks(model, position, registry, recursive, provide)`.
fn get_location_links<T: ?Sized + 'static>(
    model: &Rc<TextModel>,
    position: Position,
    registry: &LanguageFeatureRegistry<T>,
    recursive: bool,
    provide: impl Fn(&Rc<T>, &Rc<TextModel>, Position) -> Promise<Option<Vec<LocationLink>>>,
) -> Promise<Vec<LocationLink>> {
    let providers = registry.ordered(model, recursive);

    // get results
    let promises: Vec<Promise<Option<Vec<LocationLink>>>> = providers
        .iter()
        .map(|provider| {
            provide(provider, model, position).then_result(|result| match result {
                Ok(links) => Ok(links),
                Err(err) => {
                    on_unexpected_external_error(&err);
                    Ok(None)
                }
            })
        })
        .collect();

    let model = Rc::clone(model);
    Promise::all(promises).then(move |values| values.into_iter().flatten().flatten().filter(|loc| should_include_uri(&model, &loc.uri)).collect())
}

/// `getDefinitionsAtPosition`, `getDeclarationsAtPosition`, `getImplementationsAtPosition` e
/// `getTypeDefinitionsAtPosition(registry, model, position, recursive, token)` (ver a nota do módulo).
pub fn get_location_links_at_position(
    registry: &LanguageFeatureRegistry<dyn LocationLinkProvider>,
    model: &Rc<TextModel>,
    position: Position,
    recursive: bool,
    token: &CancellationToken,
) -> Promise<Vec<LocationLink>> {
    get_location_links(model, position, registry, recursive, |provider, model, position| provider.provide_location_links(model, position, token))
}

/// O `await provider.provideReferences(...)` seguido de `?.filter(ref => shouldIncludeLocationLink(model, ref))`.
fn provide_filtered_references(
    provider: &Rc<dyn ReferenceProvider>,
    model: &Rc<TextModel>,
    position: Position,
    include_declaration: bool,
    token: &CancellationToken,
) -> Promise<Option<Vec<LocationLink>>> {
    let model_for_filter = Rc::clone(model);
    provider.provide_references(model, position, &ReferenceContext { include_declaration }, token).then(move |result| {
        result.map(|locations| locations.into_iter().filter(|location| should_include_uri(&model_for_filter, &location.uri)).map(location_as_link).collect())
    })
}

/// `getReferencesAtPosition(registry, model, position, compact, recursive, token)`.
pub fn get_references_at_position(
    registry: &LanguageFeatureRegistry<dyn ReferenceProvider>,
    model: &Rc<TextModel>,
    position: Position,
    compact: bool,
    recursive: bool,
    token: &CancellationToken,
) -> Promise<Vec<LocationLink>> {
    get_location_links(model, position, registry, recursive, |provider, model, position| {
        let (provider, model, token) = (Rc::clone(provider), Rc::clone(model), token.clone());
        async_fn(async move {
            let Some(result) = provide_filtered_references(&provider, &model, position, true, &token).await? else {
                return Ok(None);
            };
            if !compact || result.len() != 2 {
                return Ok(Some(result));
            }
            let result_without_declaration = provide_filtered_references(&provider, &model, position, false, &token).await?;
            if let Some(without_declaration) = result_without_declaration.filter(|links| links.len() == 1) {
                return Ok(Some(without_declaration));
            }
            Ok(Some(result))
        })
    })
}
