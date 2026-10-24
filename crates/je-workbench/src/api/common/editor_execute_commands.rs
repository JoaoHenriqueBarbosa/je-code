//! Porte dos comandos internos `_execute*` do editor que a API `vscode.execute*` chama (ver
//! [`ext_host_api_commands`](super::ext_host_api_commands)). No original cada um mora no arquivo do seu recurso e se
//! registra na carga do módulo (`CommandsRegistry.registerCommand`); aqui o registro é a chamada explícita de
//! [`register_editor_execute_commands`].
//!
//! Portados (arquivo de origem entre parênteses):
//! - `_execute{Definition,TypeDefinition,Declaration,Implementation}Provider` e as variantes `_recursive`, e
//!   `_executeReferenceProvider` com a `_recursive` (`gotoSymbol/browser/goToSymbol.ts`).
//! - `_executeDocumentHighlights` (`wordHighlighter/browser/wordHighlighter.ts`).
//! - `_executeDocumentSymbolProvider` (`documentSymbols/browser/documentSymbols.ts`).
//! - `_executeFormatDocumentProvider`, `_executeFormatRangeProvider` e `_executeFormatOnTypeProvider`
//!   (`format/browser/format.ts`).
//! - `_executeHoverProvider` e `_executeHoverProvider_recursive` (`hover/browser/getHover.ts`).
//! - `_executeLinkProvider` (`links/browser/getLinks.ts`).
//! - `_executeCodeLensProvider` (`codelens/browser/codelens.ts`).
//! - `_executeDocumentColorProvider` e `_executeColorPresentationProvider` (`colorPicker/browser/colorPickerContribution.ts`).
//! - `_executePrepareRename` (`rename/browser/rename.ts`).
//! - `_executeSelectionRangeProvider` (`smartSelect/browser/smartSelect.ts`).
//! - `_executeWorkspaceSymbolProvider` (`workbench/contrib/search/common/search.ts`).
//!
//! NÃO portados (ver a lista de `vscode.execute*` não portados em `ext_host_api_commands`): `_executeDocumentRenameProvider`,
//! `_executeCompletionItemProvider`, `_executeSignatureHelpProvider`,
//! `_executeCodeActionProvider`,
//! `_executeInlayHintProvider`, `_executeFoldingRangeProvider`, `_executeInlineValueProvider`, `_executePrepareCallHierarchy`
//! e as demais.
//!
//! Decisões de tradução:
//! - `ILanguageFeaturesService` é o [`LanguageFeaturesService`] concreto, com o identificador [`I_LANGUAGE_FEATURES_SERVICE`].
//! - O retorno `Promise<T>` do handler é a promessa embrulhada de [`promise_to_dynamic`], e o resultado é o [`Dynamic`]
//!   que o RPC entregaria (o JSON do DTO): `languages.LocationLink` é `{ uri, range, originSelectionRange?,
//!   targetSelectionRange? }`, `DocumentSymbol` é `{ name, detail, kind, tags, containerName?, range, selectionRange,
//!   children? }`, `TextEdit` é `{ range, text, eol? }` e `RenameLocation` é `{ range?, text? }`.
//! - `_sortedAndDeduped` cria um `ReferencesModel` só para ordenar e desduplicar: a ordenação (`extUri.compare` e depois
//!   `Range.compareRangesUsingStarts`), o agrupamento por recurso (`extUri.isEqual` sem o fragmento) e o descarte do vizinho
//!   igual são os do construtor dele, [`sorted_and_deduped`]. O resto da árvore de referências (`FileReferences`,
//!   `OneReference`) não é lido pelo comando e não tem porte.
//! - `assertType(URI.isUri(resource))` e as demais conferências são [`assert_type`], que lança no ponto da chamada (a
//!   promessa rejeitada do `ICommandService`).
//! - `IConfigurationService.getValue<boolean>('editor.occurrencesHighlightFromAllProviders', ...)` é verdadeiro só para o
//!   booleano `true`.
//! - `ensureFormattingOptions(options, reference)` lê as opções do modelo quando `options` não tem `tabSize` numérico e
//!   `insertSpaces` booleano (`isFormattingOptions`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime::set_timeout;
use je_base::types::assert_type;
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit as EditorTextEdit;
use je_editor::codelens::get_code_lens_model;
use je_editor::color::{ColorPresentationsCollector, DefaultColorDecoratorsEnablement, ExtColorDataCollector, find_color_data};
use je_editor::editor_extensions::{I_TEXT_MODEL_SERVICE, ServicesAccessor, position_from_dynamic, register_model_and_position_command};
use je_editor::editor_worker::{I_EDITOR_WORKER_SERVICE, IEditorWorkerService};
use je_editor::format::{
    get_document_formatting_edits_until_result, get_document_range_formatting_edits_until_result, get_on_type_formatting_edits,
};
use je_editor::get_links::get_links;
use je_editor::goto_symbol::{get_location_links_at_position, get_references_at_position};
use je_editor::hover::get_hover::get_hovers_promise;
use je_editor::language_features_service::{
    CodeLens, FormattingOptions, IColor, IColorInformation, IColorPresentation, LanguageFeaturesService, LocationLink, LocationLinkProvider, Registry,
    RenameLocation,
};
use je_editor::language_providers::{DocumentHighlight, DocumentSymbol};
use je_editor::outline_model::I_OUTLINE_MODEL_SERVICE;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::rename::prepare_rename;
use je_editor::services_model::I_MODEL_SERVICE;
use je_editor::smart_select::{SelectionRangesOptions, provide_selection_ranges};
use je_editor::text_model::TextModel;
use je_editor::word_highlighter::get_occurrences_at_position;
use je_instantiation::ServiceId;
use je_platform::commands::commands::{CommandHandler, commands_registry};
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::I_CONFIGURATION_SERVICE;
use je_platform::resources::ext_uri;

use crate::api::browser::main_thread_commands::promise_to_dynamic;
use crate::api::browser::main_thread_language_features::registries::{IWorkspaceSymbol, get_workspace_symbols};
use crate::api::common::ext_host_type_converters::{
    CodeLens as CodeLensConverter, DocumentLink as DocumentLinkConverter, Hover as HoverConverter, Location as LocationConverter, Range as RangeConverter,
};
use crate::api::common::ext_host_types::illegal_argument;
use crate::api::common::ext_host_values::{ApiValue, is_instance_of};

/// `ILanguageFeaturesService`.
pub const I_LANGUAGE_FEATURES_SERVICE: ServiceId<LanguageFeaturesService> = ServiceId::new("languageFeaturesService");

// ---- o resultado como valor ---------------------------------------------------------------------------------

/// `languages.LocationLink` como o DTO do RPC.
fn location_link_to_dynamic(link: &LocationLink) -> Dynamic {
    let mut entries = vec![("uri", link.uri.to_dynamic()), ("range", RangeConverter::to_dto(&link.range))];
    if let Some(range) = &link.origin_selection_range {
        entries.push(("originSelectionRange", RangeConverter::to_dto(range)));
    }
    if let Some(range) = &link.target_selection_range {
        entries.push(("targetSelectionRange", RangeConverter::to_dto(range)));
    }
    Dynamic::object(entries)
}

/// `languages.DocumentHighlight` como o DTO do RPC.
fn document_highlight_to_dynamic(highlight: &DocumentHighlight) -> Dynamic {
    let mut entries = vec![("range", RangeConverter::to_dto(&highlight.range))];
    if let Some(kind) = highlight.kind {
        entries.push(("kind", Dynamic::from(kind as i32)));
    }
    Dynamic::object(entries)
}

/// `languages.DocumentSymbol` como o DTO do RPC.
fn document_symbol_to_dynamic(symbol: &DocumentSymbol) -> Dynamic {
    let mut entries = vec![
        ("name", Dynamic::string(&symbol.name)),
        ("detail", Dynamic::string(&symbol.detail)),
        ("kind", Dynamic::from(symbol.kind as i32)),
        ("tags", Dynamic::array(symbol.tags.iter().map(|tag| Dynamic::from(*tag as i32)))),
        ("range", RangeConverter::to_dto(&symbol.range)),
        ("selectionRange", RangeConverter::to_dto(&symbol.selection_range)),
    ];
    if let Some(container_name) = &symbol.container_name {
        entries.push(("containerName", Dynamic::string(container_name)));
    }
    if let Some(children) = &symbol.children {
        entries.push(("children", Dynamic::array(children.iter().map(document_symbol_to_dynamic))));
    }
    Dynamic::object(entries)
}

/// `languages.TextEdit` como o DTO do RPC.
fn text_edit_to_dynamic(edit: &EditorTextEdit) -> Dynamic {
    let mut entries = vec![("range", RangeConverter::to_dto(&edit.range)), ("text", Dynamic::string(&edit.text))];
    if let Some(eol) = edit.eol {
        entries.push(("eol", Dynamic::from(eol as i32)));
    }
    Dynamic::object(entries)
}

/// `TextEdit[] | undefined` como o DTO do RPC.
fn text_edits_to_dynamic(edits: Option<Vec<EditorTextEdit>>) -> Dynamic {
    match edits {
        Some(edits) => Dynamic::array(edits.iter().map(text_edit_to_dynamic)),
        None => Dynamic::Undefined,
    }
}

/// `languages.RenameLocation` como o DTO do RPC.
fn rename_location_to_dynamic(location: &RenameLocation) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(range) = &location.range {
        entries.push(("range", RangeConverter::to_dto(range)));
    }
    if let Some(text) = &location.text {
        entries.push(("text", Dynamic::string(text)));
    }
    Dynamic::object(entries)
}

// ---- goToSymbol.ts ------------------------------------------------------------------------------------------

/// `_sortedAndDeduped(callback)`: o resultado do `ReferencesModel` (ver as decisões de tradução do módulo).
fn sorted_and_deduped(mut links: Vec<LocationLink>) -> Vec<LocationLink> {
    let ext_uri = ext_uri();
    // `ReferencesModel._compareReferences(a, b)`
    let compare = |a: &LocationLink, b: &LocationLink| -> i32 {
        let by_uri = ext_uri.compare(&a.uri, &b.uri, false);
        if by_uri != 0 { by_uri } else { Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range)) }
    };
    links.sort_by(|a, b| compare(a, b).cmp(&0));

    let mut references: Vec<LocationLink> = Vec::new();
    let mut current_group: Option<URI> = None;
    let mut last_in_group: Option<LocationLink> = None;
    for link in links {
        if !current_group.as_ref().is_some_and(|uri| ext_uri.is_equal(Some(uri), Some(&link.uri), true)) {
            // new group
            current_group = Some(link.uri.clone());
            last_in_group = None;
        }
        // append, check for equality first!
        if last_in_group.as_ref().is_none_or(|last| compare(&link, last) != 0) {
            references.push(link.clone());
            last_in_group = Some(link);
        }
    }
    references
}

/// O handler de um `_execute*Provider` de localização: as ligações do `links`, ordenadas e desduplicadas.
fn register_location_links_command(id: &str, links: impl Fn(&LanguageFeaturesService, &Rc<TextModel>, Position) -> Promise<Vec<LocationLink>> + 'static) {
    register_model_and_position_command(
        id,
        Rc::new(move |accessor, model, position, _args| {
            let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
            let promise = links(&language_features_service, model, position);
            promise_to_dynamic(promise.then(|links| Dynamic::array(sorted_and_deduped(links).iter().map(location_link_to_dynamic))))
        }),
    );
}

/// `registerModelAndPositionCommand('_execute*Provider', ...)` de `goToSymbol.ts`.
fn register_go_to_symbol_commands() {
    type LinkRegistry = fn(&LanguageFeaturesService) -> Registry<dyn LocationLinkProvider>;
    let kinds: [(&str, LinkRegistry); 4] = [
        ("Definition", (|service: &LanguageFeaturesService| Rc::clone(&service.definition_provider)) as LinkRegistry),
        ("TypeDefinition", (|service: &LanguageFeaturesService| Rc::clone(&service.type_definition_provider)) as LinkRegistry),
        ("Declaration", (|service: &LanguageFeaturesService| Rc::clone(&service.declaration_provider)) as LinkRegistry),
        ("Implementation", (|service: &LanguageFeaturesService| Rc::clone(&service.implementation_provider)) as LinkRegistry),
    ];
    for (name, registry_of) in kinds {
        for recursive in [false, true] {
            let id = format!("_execute{name}Provider{}", if recursive { "_recursive" } else { "" });
            register_location_links_command(&id, move |service, model, position| {
                get_location_links_at_position(&registry_of(service), model, position, recursive, &CancellationToken::None)
            });
        }
    }
    for recursive in [false, true] {
        let id = format!("_executeReferenceProvider{}", if recursive { "_recursive" } else { "" });
        register_location_links_command(&id, move |service, model, position| {
            get_references_at_position(&service.reference_provider, model, position, false, recursive, &CancellationToken::None)
        });
    }
}

// ---- wordHighlighter.ts -------------------------------------------------------------------------------------

/// `registerModelAndPositionCommand('_executeDocumentHighlights', ...)`.
fn register_document_highlights_command() {
    register_model_and_position_command(
        "_executeDocumentHighlights",
        Rc::new(|accessor, model, position, _args| {
            let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
            let overrides = ConfigurationOverrides { override_identifier: Some(JsString::from(model.get_language_id())), resource: Some(model.uri()) };
            let merge_providers = matches!(
                accessor.get(I_CONFIGURATION_SERVICE).get_value(Some("editor.occurrencesHighlightFromAllProviders"), Some(&overrides)),
                Dynamic::Bool(true)
            );
            let uri = model.uri();
            let map = get_occurrences_at_position(&language_features_service.document_highlight_provider, model, position, &CancellationToken::None, merge_providers);
            promise_to_dynamic(map.then(move |map| match map.get(&uri) {
                Some(highlights) => Dynamic::array(highlights.iter().map(document_highlight_to_dynamic)),
                None => Dynamic::Undefined,
            }))
        }),
    );
}

// ---- getHover.ts --------------------------------------------------------------------------------------------

/// `registerModelAndPositionCommand('_executeHoverProvider' | '_executeHoverProvider_recursive', ...)`.
fn register_hover_commands() {
    for recursive in [false, true] {
        let id = format!("_executeHoverProvider{}", if recursive { "_recursive" } else { "" });
        register_model_and_position_command(
            &id,
            Rc::new(move |accessor, model, position, _args| {
                let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
                let hovers = get_hovers_promise(&language_features_service.hover_provider, model, position, &CancellationToken::None, recursive);
                promise_to_dynamic(hovers.then(|hovers| Dynamic::array(hovers.iter().map(HoverConverter::to_dto))))
            }),
        );
    }
}

// ---- getLinks.ts --------------------------------------------------------------------------------------------

/// `CommandsRegistry.registerCommand('_executeLinkProvider', ...)`.
fn register_link_provider_command() {
    let handler: CommandHandler = Rc::new(|accessor, args| {
        let resource = resource_argument(args);
        // `if (typeof resolveCount !== 'number') resolveCount = 0`
        let resolve_count = match args.get(1) {
            Some(Dynamic::Number(count)) => *count,
            _ => 0.0,
        };

        let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
        let Some(model) = accessor.get(I_MODEL_SERVICE).get_model(&resource) else {
            return promise_to_dynamic(Promise::resolved(Dynamic::array(Vec::<Dynamic>::new())));
        };
        promise_to_dynamic(async_fn(async move {
            let list = get_links(&language_features_service.link_provider, &model, &CancellationToken::None).await?;

            // resolve links
            let links = list.links();
            for link in links.iter().take(resolve_count.min(links.len() as f64).max(0.0).ceil() as usize) {
                link.resolve(&CancellationToken::None).await?;
            }

            let result = Dynamic::array(links.iter().map(|link| DocumentLinkConverter::to_dto(&link.to_json())));
            list.dispose();
            Ok(result)
        }))
    });
    commands_registry().register_command_handler("_executeLinkProvider", Some(handler));
}

// ---- codelens.ts --------------------------------------------------------------------------------------------

/// `CommandsRegistry.registerCommand('_executeCodeLensProvider', ...)`.
fn register_code_lens_command() {
    let handler: CommandHandler = Rc::new(|accessor, args| {
        let resource = resource_argument(args);
        let item_resolve_count = args.get(1).cloned().unwrap_or(Dynamic::Undefined);
        assert_type(&(matches!(item_resolve_count, Dynamic::Number(_)) || !item_resolve_count.is_truthy()), None);
        // `itemResolveCount === undefined || itemResolveCount === null` é `None`; o resto é o número (um falso que não é
        // número, como `false` ou `''`, vale `0` no `itemResolveCount-- > 0`)
        let mut item_resolve_count = match item_resolve_count {
            Dynamic::Undefined | Dynamic::Null => None,
            Dynamic::Number(count) => Some(count),
            _ => Some(0.0),
        };

        let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
        let Some(model) = accessor.get(I_MODEL_SERVICE).get_model(&resource) else {
            errors::throw(illegal_argument(None));
        };

        let result: Rc<RefCell<Vec<CodeLens>>> = Rc::default();
        let disposables = DisposableStore::new();
        let disposables_later = Rc::clone(&disposables);
        let promise = async_fn(async move {
            let value = get_code_lens_model(&language_features_service.code_lens_provider, &model, &CancellationToken::None).await?;

            disposables.add(Rc::clone(&value) as DisposableRef);
            let mut resolve: Vec<Promise<()>> = Vec::new();

            for item in &value.lenses {
                if item_resolve_count.is_none() || item.symbol.command.is_some() {
                    result.borrow_mut().push(item.symbol.clone());
                } else {
                    let remaining = item_resolve_count.unwrap_or_default();
                    item_resolve_count = Some(remaining - 1.0);
                    if remaining > 0.0 && item.provider.has_resolve_code_lens() {
                        let (result, symbol) = (Rc::clone(&result), item.symbol.clone());
                        resolve.push(
                            item.provider
                                .resolve_code_lens(&model, &item.symbol, &CancellationToken::None)
                                .then(move |resolved| result.borrow_mut().push(resolved.unwrap_or(symbol))),
                        );
                    }
                }
            }

            Promise::all(resolve).await?;
            let lenses = result.borrow();
            Ok(Dynamic::array(lenses.iter().map(CodeLensConverter::to_dto)))
        })
        .finally(move || {
            // make sure to return results, then (on next tick) dispose the results
            set_timeout(move || disposables_later.dispose(), 100);
        });
        promise_to_dynamic(promise)
    });
    commands_registry().register_command_handler("_executeCodeLensProvider", Some(handler));
}

// ---- colorPickerContribution.ts e color.ts ------------------------------------------------------------------

/// `_setupColorCommand(accessor, resource)`: o modelo, o registro de provedores e o `editor.defaultColorDecorators` do recurso.
fn setup_color_command(accessor: &ServicesAccessor, resource: &URI) -> (Rc<TextModel>, Rc<LanguageFeaturesService>, DefaultColorDecoratorsEnablement) {
    let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
    let Some(model) = accessor.get(I_MODEL_SERVICE).get_model(resource) else {
        errors::throw(illegal_argument(None));
    };
    let overrides = ConfigurationOverrides { override_identifier: None, resource: Some(resource.clone()) };
    // um valor ausente ou desconhecido não é `'always'` nem `'never'`: vale como `'auto'` nas duas comparações de `_findColorData`
    let enablement = match accessor.get(I_CONFIGURATION_SERVICE).get_value(Some("editor.defaultColorDecorators"), Some(&overrides)) {
        Dynamic::String(value) if value.to_string_lossy() == "always" => DefaultColorDecoratorsEnablement::Always,
        Dynamic::String(value) if value.to_string_lossy() == "never" => DefaultColorDecoratorsEnablement::Never,
        _ => DefaultColorDecoratorsEnablement::Auto,
    };
    (model, language_features_service, enablement)
}

/// `languages.IColorPresentation` como o DTO do RPC.
fn color_presentation_to_dynamic(presentation: &IColorPresentation) -> Dynamic {
    let entries = [
        ("label", Some(Dynamic::string(&presentation.label))),
        ("textEdit", presentation.text_edit.as_ref().map(text_edit_to_dynamic)),
        ("additionalTextEdits", presentation.additional_text_edits.as_ref().map(|edits| Dynamic::array(edits.iter().map(text_edit_to_dynamic)))),
    ];
    Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
}

/// `CommandsRegistry.registerCommand('_executeDocumentColorProvider' | '_executeColorPresentationProvider', ...)`.
fn register_color_commands() {
    let document_color_handler: CommandHandler = Rc::new(|accessor, args| {
        // `if (!(resource instanceof URI)) throw illegalArgument()`
        let resource = args.first().filter(|resource| is_instance_of(resource, "Uri")).and_then(|resource| URI::from_dynamic(resource).ok());
        let Some(resource) = resource else {
            errors::throw(illegal_argument(None));
        };
        let (model, language_features_service, enablement) = setup_color_command(accessor, &resource);
        let colors = find_color_data(Rc::new(ExtColorDataCollector), &language_features_service.color_provider, &model, &CancellationToken::None, enablement);
        promise_to_dynamic(colors.then(|colors| {
            Dynamic::array(colors.iter().map(|data| {
                let [red, green, blue, alpha] = data.color;
                Dynamic::object([
                    ("range", RangeConverter::to_dto(&data.range)),
                    ("color", Dynamic::array([red, green, blue, alpha].map(Dynamic::Number))),
                ])
            }))
        }))
    });
    commands_registry().register_command_handler("_executeDocumentColorProvider", Some(document_color_handler));

    let color_presentation_handler: CommandHandler = Rc::new(|accessor, args| {
        let context = args.get(1).cloned().unwrap_or(Dynamic::Undefined);
        if !context.is_truthy() {
            return Dynamic::Undefined;
        }

        let (uri, range) = (context.get_str("uri"), context.get_str("range"));
        let color = match args.first() {
            Some(Dynamic::Array(color)) if color.len() == 4 => Some(color.to_vec()),
            _ => None,
        };
        let range = RangeConverter::from_dto(&range);
        let (Some(color), Some(range), true) = (color, range, is_instance_of(&uri, "Uri")) else {
            errors::throw(illegal_argument(None));
        };
        let uri = URI::from_dynamic(&uri).unwrap_or_else(|error| errors::throw(error));
        let (model, language_features_service, enablement) = setup_color_command(accessor, &uri);
        let channel = |value: &Dynamic| match value {
            Dynamic::Number(value) => *value,
            _ => f64::NAN,
        };
        let color_info = IColorInformation { range, color: IColor { red: channel(&color[0]), green: channel(&color[1]), blue: channel(&color[2]), alpha: channel(&color[3]) } };
        let presentations = find_color_data(
            Rc::new(ColorPresentationsCollector::new(color_info)),
            &language_features_service.color_provider,
            &model,
            &CancellationToken::None,
            enablement,
        );
        promise_to_dynamic(presentations.then(|presentations| Dynamic::array(presentations.iter().map(color_presentation_to_dynamic))))
    });
    commands_registry().register_command_handler("_executeColorPresentationProvider", Some(color_presentation_handler));
}

// ---- argumentos ---------------------------------------------------------------------------------------------

/// `const [resource] = args; assertType(URI.isUri(resource))`.
fn resource_argument(args: &[Dynamic]) -> URI {
    let resource = args.first().and_then(|resource| URI::from_dynamic(resource).ok());
    assert_type(&resource.is_some(), None);
    resource.expect("assert_type lança antes")
}

// ---- documentSymbols.ts -------------------------------------------------------------------------------------

/// `CommandsRegistry.registerCommand('_executeDocumentSymbolProvider', ...)`.
fn register_document_symbol_command() {
    let handler: CommandHandler = Rc::new(|accessor, args| {
        let resource = resource_argument(args);

        let outline_service = accessor.get(I_OUTLINE_MODEL_SERVICE);
        let model_service = accessor.get(I_TEXT_MODEL_SERVICE);

        promise_to_dynamic(async_fn(async move {
            let reference = model_service.create_model_reference(&resource).await?;
            let outline_model = outline_service.get_or_create(&reference.object, &CancellationToken::None).await;
            reference.dispose();
            Ok(Dynamic::array(outline_model?.get_top_level_symbols().iter().map(document_symbol_to_dynamic)))
        }))
    });
    commands_registry().register_command_handler("_executeDocumentSymbolProvider", Some(handler));
}

// ---- format.ts ----------------------------------------------------------------------------------------------

/// Os serviços que os três comandos de formatação leem (`ITextModelService` fica com quem registra).
struct FormatServices {
    worker_service: Rc<dyn IEditorWorkerService>,
    language_features_service: Rc<LanguageFeaturesService>,
}

/// `ensureFormattingOptions(options, reference)`.
fn ensure_formatting_options(options: &Dynamic, model: &TextModel) -> FormattingOptions {
    if matches!(options, Dynamic::Object(_)) {
        if let (Dynamic::Number(tab_size), Dynamic::Bool(insert_spaces)) = (options.get_str("tabSize"), options.get_str("insertSpaces")) {
            return FormattingOptions { tab_size, insert_spaces };
        }
    }
    let model_options = model.get_options();
    FormattingOptions { tab_size: model_options.tab_size as f64, insert_spaces: model_options.insert_spaces }
}

/// `CommandsRegistry.registerCommand(id, async (accessor, ...args) => { ... })` dos três comandos de formatação:
/// `assert_arguments` é o `assertType` dos argumentos depois do recurso e `run` a chamada da consulta.
fn register_format_command(
    id: &str,
    assert_arguments: fn(&[Dynamic]),
    run: fn(&FormatServices, &Rc<TextModel>, &[Dynamic]) -> Promise<Option<Vec<EditorTextEdit>>>,
) {
    let handler: CommandHandler = Rc::new(move |accessor, args| {
        let resource = resource_argument(args);
        assert_arguments(args);

        let resolver_service = accessor.get(I_TEXT_MODEL_SERVICE);
        let services = FormatServices {
            worker_service: accessor.get(I_EDITOR_WORKER_SERVICE),
            language_features_service: accessor.get(I_LANGUAGE_FEATURES_SERVICE),
        };
        let args = args.to_vec();
        promise_to_dynamic(async_fn(async move {
            let reference = resolver_service.create_model_reference(&resource).await?;
            let edits = run(&services, &reference.object, &args).await;
            reference.dispose();
            Ok(text_edits_to_dynamic(edits?))
        }))
    });
    commands_registry().register_command_handler(id, Some(handler));
}

/// `Range.isIRange(range)`.
fn range_argument(value: Option<&Dynamic>) -> Range {
    let range = value.and_then(RangeConverter::from_dto);
    assert_type(&range.is_some(), None);
    range.expect("assert_type lança antes")
}

/// `Position.isIPosition(position)`.
fn position_argument(value: Option<&Dynamic>) -> Position {
    let position = value.and_then(position_from_dynamic);
    assert_type(&position.is_some(), None);
    position.expect("assert_type lança antes")
}

/// `registerCommand('_executeFormatRangeProvider' | '_executeFormatDocumentProvider' | '_executeFormatOnTypeProvider')`.
fn register_format_commands() {
    register_format_command(
        "_executeFormatRangeProvider",
        |args| {
            range_argument(args.get(1));
        },
        |services, model, args| {
            get_document_range_formatting_edits_until_result(
                &services.worker_service,
                &services.language_features_service,
                model,
                range_argument(args.get(1)),
                ensure_formatting_options(&args.get(2).cloned().unwrap_or(Dynamic::Undefined), model),
                &CancellationToken::None,
            )
        },
    );
    register_format_command(
        "_executeFormatDocumentProvider",
        |_args| {},
        |services, model, args| {
            get_document_formatting_edits_until_result(
                &services.worker_service,
                &services.language_features_service,
                model,
                ensure_formatting_options(&args.get(1).cloned().unwrap_or(Dynamic::Undefined), model),
                &CancellationToken::None,
            )
        },
    );
    register_format_command(
        "_executeFormatOnTypeProvider",
        |args| {
            position_argument(args.get(1));
            assert_type(&matches!(args.get(2), Some(Dynamic::String(_))), None);
        },
        |services, model, args| {
            let ch = match args.get(2) {
                Some(Dynamic::String(ch)) => ch.to_string_lossy(),
                _ => String::new(),
            };
            get_on_type_formatting_edits(
                &services.worker_service,
                &services.language_features_service,
                model,
                position_argument(args.get(1)),
                &ch,
                ensure_formatting_options(&args.get(3).cloned().unwrap_or(Dynamic::Undefined), model),
                &CancellationToken::None,
            )
        },
    );
}

// ---- rename.ts ----------------------------------------------------------------------------------------------

/// `registerModelAndPositionCommand('_executePrepareRename', ...)`.
fn register_prepare_rename_command() {
    register_model_and_position_command(
        "_executePrepareRename",
        Rc::new(|accessor, model, position, _args| {
            let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
            let location = prepare_rename(&language_features_service.rename_provider, model, position, Some(&CancellationToken::None));
            promise_to_dynamic(async_fn(async move {
                let location = location.await?;
                if let Some(reject_reason) = location.as_ref().and_then(|location| location.reject_reason.as_deref()).filter(|reason| !reason.is_empty()) {
                    return Err(Error::new(reject_reason));
                }
                Ok(location.as_ref().map_or(Dynamic::Undefined, rename_location_to_dynamic))
            }))
        }),
    );
}

// ---- smartSelect.ts -----------------------------------------------------------------------------------------

/// `CommandsRegistry.registerCommand('_executeSelectionRangeProvider', ...)`.
fn register_selection_range_command() {
    let handler: CommandHandler = Rc::new(|accessor, args| {
        let resource = resource_argument(args);
        // `assertType(isArrayOf(positions, p => Position.isIPosition(p)))`
        let positions: Option<Vec<Position>> = match args.get(1) {
            Some(Dynamic::Array(items)) => items.to_vec().iter().map(position_from_dynamic).collect(),
            _ => None,
        };
        assert_type(&positions.is_some(), None);
        let positions = positions.expect("assert_type lança antes");

        let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
        let model_service = accessor.get(I_TEXT_MODEL_SERVICE);

        promise_to_dynamic(async_fn(async move {
            let reference = model_service.create_model_reference(&resource).await?;
            let ranges = provide_selection_ranges(
                &language_features_service.selection_range_provider,
                &reference.object,
                &positions,
                SelectionRangesOptions { select_leading_and_trailing_whitespace: true, select_subwords: true },
                &CancellationToken::None,
            )
            .await;
            reference.dispose();
            Ok(Dynamic::array(ranges?.iter().map(|row| Dynamic::array(row.iter().map(RangeConverter::to_dto)))))
        }))
    });
    commands_registry().register_command_handler("_executeSelectionRangeProvider", Some(handler));
}

// ---- search.ts ----------------------------------------------------------------------------------------------

/// `search.IWorkspaceSymbol` como o DTO do RPC (`location` com o `uri` e o `range` quando existem).
fn workspace_symbol_to_dynamic(symbol: &IWorkspaceSymbol) -> Dynamic {
    let mut entries = vec![
        ("name", Dynamic::string(&symbol.name)),
        ("kind", Dynamic::from(symbol.kind as i32)),
        ("location", LocationConverter::to_dto(&symbol.location)),
    ];
    if let Some(tags) = &symbol.tags {
        entries.push(("tags", Dynamic::array(tags.iter().map(|tag| Dynamic::from(*tag as i32)))));
    }
    if let Some(container_name) = &symbol.container_name {
        entries.push(("containerName", Dynamic::string(container_name)));
    }
    Dynamic::object(entries)
}

/// `CommandsRegistry.registerCommand('_executeWorkspaceSymbolProvider', ...)`.
fn register_workspace_symbol_command() {
    let handler: CommandHandler = Rc::new(|_accessor, args| {
        let query = match args.first() {
            Some(Dynamic::String(query)) => Some(query.to_string_lossy()),
            _ => None,
        };
        assert_type(&query.is_some(), None);
        let query = query.expect("assert_type lança antes");
        let result = get_workspace_symbols(&query, &CancellationToken::None);
        promise_to_dynamic(result.then(|items| Dynamic::array(items.iter().map(|item| workspace_symbol_to_dynamic(&item.symbol)))))
    });
    commands_registry().register_command_handler("_executeWorkspaceSymbolProvider", Some(handler));
}

// ---- o registro ---------------------------------------------------------------------------------------------

/// A carga dos módulos do editor que registram os comandos `_execute*` (os `import '...'` do teste do original).
pub fn register_editor_execute_commands() {
    register_go_to_symbol_commands();
    register_document_highlights_command();
    register_hover_commands();
    register_code_lens_command();
    register_color_commands();
    register_link_provider_command();
    register_document_symbol_command();
    register_format_commands();
    register_prepare_rename_command();
    register_selection_range_command();
    register_workspace_symbol_command();
}
