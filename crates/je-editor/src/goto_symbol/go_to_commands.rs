//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/goToCommands.ts`: as ações de navegação por símbolo
//! (`DefinitionAction`, `DeclarationAction`, `TypeDefinitionAction`, `ImplementationAction`, `ReferencesAction` e a
//! `GenericGoToLocationAction`, todas sobre a `SymbolNavigationAction`), os comandos `editor.action.goToLocations`,
//! `editor.action.peekLocations`, `editor.action.findReferences` e o alias `editor.action.showReferences`, e o submenu
//! "Peek" do menu de contexto do editor.
//!
//! Representação:
//! - `abstract class SymbolNavigationAction extends EditorAction2` e as suas subclasses são a struct
//!   [`SymbolNavigationAction`] com o [`SymbolNavigationKind`] como dado: os quatro métodos abstratos
//!   (`_getLocationModel`, `_getNoResultFoundMessage`, `_getAlternativeCommand`, `_getGoToPreference`) diferem só no
//!   registro do provedor, nos textos e na opção `gotoLocation` que leem, então são um `match` sobre o tipo (o mesmo molde
//!   de `StickyScrollFocusedAction`). `DefinitionAction` é `Definition`, `DeclarationAction` é `Declaration`,
//!   `TypeDefinitionAction` é `TypeDefinition`, `ImplementationAction` é `Implementation`, `GoToReferencesAction` e
//!   `PeekReferencesAction` são `References { compact }` (o `true` e o `false` do `getReferencesAtPosition`) e
//!   `GenericGoToLocationAction` é `Generic`, com as localizações, o `multiple` e o `noResultsMessage` que o comando
//!   `editor.action.goToLocations` lhe dá (a subclasse anônima que sobrescreve `_getNoResultFoundMessage`).
//! - As classes registradas por `registerAction2(class ... extends DefinitionAction { ... })` são a descrição de cada
//!   ação (`register_spec`), registradas com o `EditorAction2Adapter` sobre o [`SymbolNavigationActionRegistration`].
//! - `static _allSymbolNavigationCommands` e `_activeAlternativeCommands` são estado por thread; o `Map` mantém a ordem de
//!   inserção, como o do original. `SymbolNavigationAction.all()` é [`SymbolNavigationAction::all`].
//! - `runEditorCommand(accessor, editor, arg?, range?)` é [`SymbolNavigationAction::run_editor_command`], com o
//!   `arg: SymbolNavigationAnchor | unknown` como `Option<SymbolNavigationAnchor>`: uma âncora carrega o modelo do texto, que
//!   um `Dynamic` não carrega, então ela só chega por quem chama a função tipada (`showGoToContextMenu` e
//!   `goToDefinitionWithLocation`); pelo comando registrado (`Action2.run(accessor, ...args)`) os argumentos são ignorados e a
//!   âncora é o modelo e a posição do editor, como no original sem argumento. `EditorAction2.run` é
//!   [`SymbolNavigationAction::run`].
//! - As funções `async` são [`async_fn`]; `raceCancellation` é [`race_cancellation`]; `progressService.showWhile(promise, 250)`
//!   não é aguardado.
//! - `createCancelablePromise(_ => Promise.resolve(model))` é [`create_cancelable_promise`].
//! - `isLocationLink(reference)` de `_openReference` só é verdadeiro para um `LocationLink` cru, e o único chamador passa a
//!   `OneReference` (`model.firstReference()`), que não tem `originSelectionRange` nem `targetSelectionRange`: a faixa é
//!   sempre a `range` da referência (`targetSelectionRange ?? range` do `OneReference.range`).
//! - `Position.isIPosition` e `URI.isUri` dos comandos leem o `Dynamic` pela forma de `toJSON` (ver `editor_extensions`); os
//!   elementos de `locations` são `{ uri, range }` com a faixa nos quatro números `startLineNumber`... `endColumn`.
//!   `multiple as GoToLocationValues` de um texto fora de `peek`, `gotoAndPeek` e `goto` vira "não informado" (a opção
//!   `gotoLocation.multipleReferences` do editor).
//!
//! Divergências:
//! - **`editor.invokeWithinContext`.** É o [`CodeEditorWidget::invoke_within_context`], com o serviço de instanciação do
//!   comando como reserva para o editor que não tem o próprio.
//! - **O peek e a mensagem.** O `ReferencesController` (`peek/referencesController.ts`) é o de
//!   [`super::peek::references_controller`], que implementa o contrato [`IReferencesController`] e registra o
//!   [`IReferencesControllerLookup`] ([`I_REFERENCES_CONTROLLER_LOOKUP`]). O `MessageController`
//!   (`message/browser/messageController.ts`) ainda não tem porte. Onde o original faz `ReferencesController.get(editor)` e
//!   `MessageController.get(editor)?.showMessage(...)` este módulo lê os contratos [`IReferencesControllerLookup`] e
//!   [`IMessageControllerLookup`] do `accessor` (`try_get`: sem serviço instalado é como o editor sem o controlador, o
//!   `get` que devolve `null`). A fatia da mensagem implementa e registra o [`I_MESSAGE_CONTROLLER_LOOKUP`].
//! - **Alertas.** O `alert` de `aria.js` é o `alert` do `IAccessibilityService`.
//! - `MenuId.EditorContext` ganha o submenu `EditorContextPeek` ("Peek") na carga do módulo; aqui isso é a chamada de
//!   [`register`], idempotente por thread como a dos demais serviços do porte, que registra também as ações e os comandos.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use je_base::async_::{async_fn, create_cancelable_promise, race_cancellation};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::{CancelablePromise, Promise};
use je_base::runtime::set_timeout;
use je_base::types::assert_type;
use je_instantiation::{I_INSTANTIATION_SERVICE, InstantiationService, ServicesAccessor, service_id};
use je_platform::accessibility::{IAccessibilityService, I_ACCESSIBILITY_SERVICE};
use je_platform::action::{CommandAction, Title};
use je_platform::actions::menu_ids::{editor_context, editor_context_peek, menubar_go_menu};
use je_platform::actions::{Action2Keybinding, Action2MenuItem, Action2Options, AnyMenuItem, SubmenuItem, menu_registry, register_action2};
use je_platform::commands::{
    Command as RegisteredCommand, CommandArgument, CommandDescription, CommandHandler, CommandMetadata, I_COMMAND_SERVICE, commands_registry,
};
use je_platform::contextkey::contextkeys::is_web_context;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, RawContextKey};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings, PlatformKeybindings};
use je_platform::notification::notification::I_NOTIFICATION_SERVICE;
use je_platform::notification::{INotificationService, NotificationMessage};

use super::references_model::{OneReference, ReferencesModel};
use super::symbol_navigation::{ISymbolNavigationService, I_SYMBOL_NAVIGATION_SERVICE};
use super::{get_location_links_at_position, get_references_at_position, location_as_link};
use crate::code_action::platform::{I_EDITOR_PROGRESS_SERVICE, IProgressIndicator};
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::goto_location::{GOTO_LOCATION, GoToLocationOptions, GoToLocationValues};
use crate::core_commands::{editor_text_focus, text_editor_selection_source};
use crate::editor_common::ScrollType;
use crate::editor_context_keys::{
    has_declaration_provider, has_definition_provider, has_implementation_provider, has_reference_provider,
    has_type_definition_provider, is_in_embedded_editor,
};
use crate::editor_extensions::{
    EditorAction2, EditorAction2Adapter, action2_menu_item, editor_for_action2, localized_title, localized_title_with_mnemonic,
    position_from_dynamic, uri_from_dynamic,
};
use crate::editor_state::{EditorStateCancellationTokenSource, code_editor_state_flag};
use crate::embedded_code_editor_widget::is_embedded_code_editor;
use crate::keybinding_cancellation::I_EDITOR_CANCELLATION_TOKENS;
use crate::language_features_service::{I_LANGUAGE_FEATURES_SERVICE, LanguageFeaturesService, Location};
use crate::model::{ModelDecorationOptionsInit, ModelDeltaDecoration};
use crate::opener_service::{
    CodeEditorHandle, ICodeEditorService, I_CODE_EDITOR_SERVICE, TextEditorOptions, TextResourceEditorInput,
    range_to_text_editor_selection,
};
use crate::peek_view::not_in_peek_editor;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};
use crate::word_helper::WordAtPosition;
use je_platform::opener::editor_types::TextEditorSelectionRevealType;

thread_local! {
    /// `_allSymbolNavigationCommands`: `Map<string, SymbolNavigationAction>`, na ordem de inserção.
    static ALL_SYMBOL_NAVIGATION_COMMANDS: RefCell<Vec<(String, Rc<SymbolNavigationAction>)>> = const { RefCell::new(Vec::new()) };
    /// `_activeAlternativeCommands`.
    static ACTIVE_ALTERNATIVE_COMMANDS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

// #region Contratos do peek e da mensagem (fatias seguintes)

/// `ReferencesController` (`peek/referencesController.ts`): o que as ações pedem a ele.
pub trait IReferencesController {
    /// `toggleWidget(range, modelPromise, peekMode)`.
    fn toggle_widget(&self, range: Range, model: CancelablePromise<Rc<ReferencesModel>>, peek_mode: bool);
}

/// `ReferencesController.get(editor)`: o controlador do editor, ou `None` quando o editor não o tem.
pub trait IReferencesControllerLookup {
    fn get(&self, editor: &Rc<CodeEditorWidget>) -> Option<Rc<dyn IReferencesController>>;
}

service_id!(pub I_REFERENCES_CONTROLLER_LOOKUP: dyn IReferencesControllerLookup = "referencesControllerLookup");

/// `MessageController.get(editor)?.showMessage(message, position)`: nada acontece quando o editor não tem o controlador.
pub trait IMessageControllerLookup {
    fn show_message(&self, editor: &Rc<CodeEditorWidget>, message: &str, position: Position);
}

service_id!(pub I_MESSAGE_CONTROLLER_LOOKUP: dyn IMessageControllerLookup = "messageControllerLookup");

// #endregion

/// `SymbolNavigationActionConfig`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolNavigationActionConfig {
    pub open_to_side: bool,
    pub open_in_peek: bool,
    pub mute_message: bool,
}

/// `SymbolNavigationAnchor`: o modelo e a posição de onde a navegação parte.
#[derive(Clone)]
pub struct SymbolNavigationAnchor {
    pub model: Rc<TextModel>,
    pub position: Position,
}

/// As localizações, o `multiple` e o `noResultsMessage` da `GenericGoToLocationAction`.
#[derive(Clone)]
pub struct GenericLocations {
    pub references: Vec<Location>,
    pub multiple: Option<GoToLocationValues>,
    pub no_results_message: Option<String>,
}

/// Qual das subclasses de `SymbolNavigationAction` uma ação é (ver "Representação" no cabeçalho).
#[derive(Clone)]
pub enum SymbolNavigationKind {
    Definition,
    Declaration,
    TypeDefinition,
    Implementation,
    /// `compact` é o argumento de `getReferencesAtPosition`: verdadeiro em `GoToReferencesAction`.
    References { compact: bool },
    Generic(GenericLocations),
}

impl SymbolNavigationKind {
    /// `_getLocationModel(languageFeaturesService, model, position, token)`.
    fn get_location_model(
        &self,
        language_features_service: &LanguageFeaturesService,
        model: &Rc<TextModel>,
        position: Position,
        token: &CancellationToken,
    ) -> Promise<Option<Rc<ReferencesModel>>> {
        let (links, title) = match self {
            Self::Definition => (
                get_location_links_at_position(&language_features_service.definition_provider, model, position, false, token),
                localize("def.title", "Definitions", &[]),
            ),
            Self::Declaration => (
                get_location_links_at_position(&language_features_service.declaration_provider, model, position, false, token),
                localize("decl.title", "Declarations", &[]),
            ),
            Self::TypeDefinition => (
                get_location_links_at_position(&language_features_service.type_definition_provider, model, position, false, token),
                localize("typedef.title", "Type Definitions", &[]),
            ),
            Self::Implementation => (
                get_location_links_at_position(&language_features_service.implementation_provider, model, position, false, token),
                localize("impl.title", "Implementations", &[]),
            ),
            Self::References { compact } => (
                get_references_at_position(&language_features_service.reference_provider, model, position, *compact, false, token),
                localize("ref.title", "References", &[]),
            ),
            Self::Generic(generic) => (
                Promise::resolved(generic.references.iter().cloned().map(location_as_link).collect::<Vec<_>>()),
                localize("generic.title", "Locations", &[]),
            ),
        };
        links.then(move |links| Some(ReferencesModel::new(links, title)))
    }

    /// `_getNoResultFoundMessage(info)`.
    fn get_no_result_found_message(&self, info: Option<&WordAtPosition>) -> String {
        let word = info.map(|info| info.word.to_string_lossy());
        // `info && info.word ? localize(withWord) : localize(generic)`
        let with_word = |key: &str, message: &str, generic_key: &str, generic_message: &str| match word.as_deref() {
            Some(word) if !word.is_empty() => localize(key, message, &[word]),
            _ => localize(generic_key, generic_message, &[]),
        };
        match self {
            Self::Definition => with_word("noResultWord", "No definition found for '{0}'", "generic.noResults", "No definition found"),
            Self::Declaration => {
                with_word("decl.noResultWord", "No declaration found for '{0}'", "decl.generic.noResults", "No declaration found")
            }
            Self::TypeDefinition => with_word(
                "goToTypeDefinition.noResultWord",
                "No type definition found for '{0}'",
                "goToTypeDefinition.generic.noResults",
                "No type definition found",
            ),
            Self::Implementation => with_word(
                "goToImplementation.noResultWord",
                "No implementation found for '{0}'",
                "goToImplementation.generic.noResults",
                "No implementation found",
            ),
            // `info ? ... : ...`: o `info` basta, mesmo com a palavra vazia
            Self::References { .. } => match word.as_deref() {
                Some(word) => localize("references.no", "No references found for '{0}'", &[word]),
                None => localize("references.noGeneric", "No references found", &[]),
            },
            Self::Generic(generic) => match (&generic.no_results_message, word.as_deref()) {
                // `noResultsMessage || super._getNoResultFoundMessage(info)`
                (Some(message), _) if !message.is_empty() => message.clone(),
                // `info && localize(...) || ''`
                (_, Some(word)) => localize("generic.noResult", "No results for '{0}'", &[word]),
                _ => String::new(),
            },
        }
    }

    /// `_getAlternativeCommand(editor)`: `undefined` na ação genérica.
    fn get_alternative_command(&self, options: &GoToLocationOptions) -> Option<String> {
        match self {
            Self::Definition => Some(options.alternative_definition_command.clone()),
            Self::Declaration => Some(options.alternative_declaration_command.clone()),
            Self::TypeDefinition => Some(options.alternative_type_definition_command.clone()),
            Self::Implementation => Some(options.alternative_implementation_command.clone()),
            Self::References { .. } => Some(options.alternative_reference_command.clone()),
            Self::Generic(_) => None,
        }
    }

    /// `_getGoToPreference(editor)`.
    fn get_go_to_preference(&self, options: &GoToLocationOptions) -> GoToLocationValues {
        match self {
            Self::Definition => options.multiple_definitions,
            Self::Declaration => options.multiple_declarations,
            Self::TypeDefinition => options.multiple_type_definitions,
            Self::Implementation => options.multiple_implementations,
            Self::References { .. } => options.multiple_references,
            Self::Generic(generic) => generic.multiple.unwrap_or(options.multiple_references),
        }
    }
}

/// Os serviços que uma execução de `runEditorCommand` lê do `accessor` antes do primeiro `await` (o `accessor` só vale
/// durante a chamada).
struct Services {
    notification: Rc<dyn INotificationService>,
    editors: Rc<dyn ICodeEditorService>,
    symbol_navigation: Rc<dyn ISymbolNavigationService>,
    accessibility: Rc<dyn IAccessibilityService>,
    instantiation: Rc<InstantiationService>,
    references_controllers: Option<Rc<dyn IReferencesControllerLookup>>,
    message_controllers: Option<Rc<dyn IMessageControllerLookup>>,
}

/// `SymbolNavigationAction`.
pub struct SymbolNavigationAction {
    pub configuration: SymbolNavigationActionConfig,
    kind: SymbolNavigationKind,
    desc: Action2Options,
}

impl SymbolNavigationAction {
    /// `SymbolNavigationAction.all()`.
    pub fn all() -> Vec<Rc<SymbolNavigationAction>> {
        ALL_SYMBOL_NAVIGATION_COMMANDS.with(|all| all.borrow().iter().map(|(_, action)| Rc::clone(action)).collect())
    }

    /// `_patchConfig(opts)`: `f1: true` e a precondição da ação nos itens do menu de contexto.
    fn patch_config(opts: Action2Options) -> Action2Options {
        let precondition = opts.command.precondition.clone();
        let menu = opts
            .menu
            .into_iter()
            .map(|mut item| {
                if item.id == editor_context() || item.id == editor_context_peek() {
                    item.when = ContextKeyExpr::and([precondition.clone(), item.when.take()]);
                }
                item
            })
            .collect();
        Action2Options { menu, f1: true, ..opts }
    }

    /// O construtor: guarda a ação em `_allSymbolNavigationCommands` pelo id.
    pub fn new(configuration: SymbolNavigationActionConfig, kind: SymbolNavigationKind, opts: Action2Options) -> Rc<Self> {
        let desc = Self::patch_config(opts);
        let id = desc.command.id.clone();
        let action = Rc::new(Self { configuration, kind, desc });
        ALL_SYMBOL_NAVIGATION_COMMANDS.with(|all| {
            let mut all = all.borrow_mut();
            match all.iter_mut().find(|(key, _)| *key == id) {
                Some(entry) => entry.1 = Rc::clone(&action),
                None => all.push((id, Rc::clone(&action))),
            }
        });
        action
    }

    /// `desc.id`.
    pub fn id(&self) -> &str {
        &self.desc.command.id
    }

    /// `desc`.
    pub fn desc(&self) -> &Action2Options {
        &self.desc
    }

    /// `EditorAction2.run(accessor, ...args)` com os argumentos tipados: o editor com foco ou ativo, se a precondição vale.
    pub fn run(
        self: &Rc<Self>,
        accessor: &ServicesAccessor,
        anchor: Option<SymbolNavigationAnchor>,
        range: Option<Range>,
    ) -> Promise<()> {
        match editor_for_action2(accessor, self.desc.command.precondition.as_ref()) {
            Some(editor) => self.run_editor_command(accessor, &editor, anchor, range),
            None => Promise::resolved(()),
        }
    }

    /// `runEditorCommand(accessor, editor, arg?, range?)`.
    pub fn run_editor_command(
        self: &Rc<Self>,
        accessor: &ServicesAccessor,
        editor: &Rc<CodeEditorWidget>,
        anchor: Option<SymbolNavigationAnchor>,
        range: Option<Range>,
    ) -> Promise<()> {
        if !editor.has_model() {
            return Promise::resolved(());
        }
        let (Some(model), Some(position)) = (editor.get_model(), editor.get_position()) else {
            return Promise::resolved(());
        };
        let progress_service = accessor.get(I_EDITOR_PROGRESS_SERVICE);
        let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
        let editor_cancellation_tokens = accessor.get(I_EDITOR_CANCELLATION_TOKENS);
        let services = Rc::new(Services {
            notification: accessor.get(I_NOTIFICATION_SERVICE),
            editors: accessor.get(I_CODE_EDITOR_SERVICE),
            symbol_navigation: accessor.get(I_SYMBOL_NAVIGATION_SERVICE),
            accessibility: accessor.get(I_ACCESSIBILITY_SERVICE),
            instantiation: accessor.get(I_INSTANTIATION_SERVICE),
            references_controllers: accessor.try_get(I_REFERENCES_CONTROLLER_LOOKUP),
            message_controllers: accessor.try_get(I_MESSAGE_CONTROLLER_LOOKUP),
        });

        let anchor_arg = anchor.clone();
        let anchor = anchor.unwrap_or_else(|| SymbolNavigationAnchor { model: Rc::clone(&model), position });

        let cts = EditorStateCancellationTokenSource::new(
            editor,
            &editor_cancellation_tokens,
            code_editor_state_flag::VALUE | code_editor_state_flag::POSITION,
            None,
            None,
        );
        let token = cts.token();
        let location_model =
            race_cancellation(&self.kind.get_location_model(&language_features_service, &anchor.model, anchor.position, &token), &token);

        let (this, editor) = (Rc::clone(self), Rc::clone(editor));
        let promise = async_fn(async move {
            let result = match location_model.await {
                Ok(references) => this.handle_references(&services, &editor, &model, position, anchor_arg, range, &token, references.flatten()).await,
                Err(error) => {
                    // report an error
                    services.notification.error(vec![NotificationMessage::Error(error)]);
                    Ok(())
                }
            };
            cts.dispose();
            result
        });

        let _ = progress_service.show_while(&promise, Some(250));
        promise
    }

    /// O `async references => { ... }` do `then` de `runEditorCommand`.
    #[allow(clippy::too_many_arguments)]
    async fn handle_references(
        &self,
        services: &Services,
        editor: &Rc<CodeEditorWidget>,
        model: &Rc<TextModel>,
        position: Position,
        anchor: Option<SymbolNavigationAnchor>,
        range: Option<Range>,
        token: &CancellationToken,
        references: Option<Rc<ReferencesModel>>,
    ) -> Result<(), Error> {
        let Some(references) = references else {
            return Ok(());
        };
        if token.is_cancellation_requested() {
            return Ok(());
        }

        services.accessibility.alert(&references.aria_message());

        let mut alt_action: Option<Rc<SymbolNavigationAction>> = None;
        if references.reference_at(&model.uri(), &position).is_some()
            && let Some(alt_action_id) = self.kind.get_alternative_command(editor.get_option(GOTO_LOCATION))
        {
            let is_active = ACTIVE_ALTERNATIVE_COMMANDS.with(|active| active.borrow().contains(&alt_action_id));
            if !is_active {
                alt_action = ALL_SYMBOL_NAVIGATION_COMMANDS
                    .with(|all| all.borrow().iter().find(|(key, _)| *key == alt_action_id).map(|(_, action)| Rc::clone(action)));
            }
        }

        let reference_count = references.references().len();

        if reference_count == 0 {
            // no result -> show message
            if !self.configuration.mute_message {
                let info = model.get_word_at_position(position);
                if let Some(controllers) = &services.message_controllers {
                    controllers.show_message(editor, &self.kind.get_no_result_found_message(info.as_ref()), position);
                }
            }
            Ok(())
        } else if let (1, Some(alt_action)) = (reference_count, alt_action) {
            // already at the only result, run alternative
            let this_id = self.id().to_owned();
            ACTIVE_ALTERNATIVE_COMMANDS.with(|active| active.borrow_mut().insert(this_id.clone()));
            let _ = services
                .instantiation
                .invoke_function(|accessor| alt_action.run_editor_command(accessor, editor, anchor, range))
                .finally(move || {
                    ACTIVE_ALTERNATIVE_COMMANDS.with(|active| active.borrow_mut().remove(&this_id));
                });
            Ok(())
        } else {
            // normal results handling
            self.on_result(services, editor, references, range).await
        }
    }

    /// `_onResult(editorService, symbolNavService, editor, model, range?)`.
    async fn on_result(
        &self,
        services: &Services,
        editor: &Rc<CodeEditorWidget>,
        model: Rc<ReferencesModel>,
        range: Option<Range>,
    ) -> Result<(), Error> {
        let goto_location = self.kind.get_go_to_preference(editor.get_option(GOTO_LOCATION));
        if !is_embedded_code_editor(editor)
            && (self.configuration.open_in_peek || (goto_location == GoToLocationValues::Peek && model.references().len() > 1))
        {
            self.open_in_peek(services, editor, &model, range);
        } else {
            let Some(next) = model.first_reference() else {
                return Ok(());
            };
            let peek = model.references().len() > 1 && goto_location == GoToLocationValues::GotoAndPeek;
            let target_editor = self.open_reference(services, editor, &next, self.configuration.open_to_side, !peek).await?;
            match target_editor {
                Some(target_editor) if peek => self.open_in_peek(services, &target_editor, &model, range),
                _ => model.dispose(),
            }

            // keep remaining locations around when using
            // 'goto'-mode
            if goto_location == GoToLocationValues::Goto {
                services.symbol_navigation.put(&next);
            }
        }
        Ok(())
    }

    /// `_openReference(editor, editorService, reference, sideBySide, highlight)`.
    async fn open_reference(
        &self,
        services: &Services,
        editor: &Rc<CodeEditorWidget>,
        reference: &Rc<OneReference>,
        side_by_side: bool,
        highlight: bool,
    ) -> Result<Option<CodeEditorHandle>, Error> {
        // range is the target-selection-range when we have one
        // and the fallback is the 'full' range
        let range = reference.range();

        let input = TextResourceEditorInput {
            resource: reference.uri().clone(),
            options: Some(TextEditorOptions {
                selection: Some(range_to_text_editor_selection(&range.collapse_to_start())),
                selection_reveal_type: Some(TextEditorSelectionRevealType::NearTopIfOutsideViewport),
                selection_source: Some(text_editor_selection_source::JUMP.to_owned()),
                ..TextEditorOptions::default()
            }),
        };
        let Some(target_editor) = services.editors.open_code_editor(input, Some(Rc::clone(editor)), Some(side_by_side)).await? else {
            return Ok(None);
        };

        if highlight {
            let model_now = target_editor.get_model();
            let decorations = target_editor.create_decorations_collection(Some(vec![ModelDeltaDecoration {
                range,
                options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                    description: "symbol-navigate-action-highlight".to_owned(),
                    class_name: Some("symbolHighlight".to_owned()),
                    ..ModelDecorationOptionsInit::default()
                }),
            }]));
            let editor_now = Rc::clone(&target_editor);
            let _ = set_timeout(
                move || {
                    let same_model = match (editor_now.get_model(), model_now) {
                        (Some(now), Some(before)) => Rc::ptr_eq(&now, &before),
                        (None, None) => true,
                        _ => false,
                    };
                    if same_model {
                        decorations.clear();
                    }
                },
                350,
            );
        }

        Ok(Some(target_editor))
    }

    /// `_openInPeek(target, model, range?)`.
    fn open_in_peek(&self, services: &Services, target: &Rc<CodeEditorWidget>, model: &Rc<ReferencesModel>, range: Option<Range>) {
        let controller = services.references_controllers.as_ref().and_then(|controllers| controllers.get(target));
        match controller {
            Some(controller) if target.has_model() => {
                let range = range.or_else(|| target.get_selection().map(|selection| *selection));
                if let Some(range) = range {
                    let model = Rc::clone(model);
                    let references = create_cancelable_promise(move |_| Promise::resolved(model));
                    controller.toggle_widget(range, references, self.configuration.open_in_peek);
                }
            }
            _ => model.dispose(),
        }
    }
}

/// O `EditorAction2` registrado de uma [`SymbolNavigationAction`] (ver "Representação" no cabeçalho).
pub struct SymbolNavigationActionRegistration(pub Rc<SymbolNavigationAction>);

impl EditorAction2 for SymbolNavigationActionRegistration {
    fn desc(&self) -> Action2Options {
        self.0.desc().clone()
    }

    fn run_editor_command(&self, accessor: &ServicesAccessor, editor: &Rc<CodeEditorWidget>, _args: &[Dynamic]) -> Promise<()> {
        self.0.run_editor_command(accessor, editor, None, None)
    }
}

// #region --- Registro das ações

/// `{ primary }` sem plataforma.
fn primary(primary: u32) -> Keybindings {
    Keybindings { primary: Some(primary), ..Keybindings::default() }
}

fn contrib_keybinding(when: Option<ContextKeyExpression>, keybindings: Keybindings) -> Action2Keybinding {
    Action2Keybinding { keybindings, weight: KeybindingWeight::EDITOR_CONTRIB, args: Dynamic::Undefined, when }
}

/// O item do menu "Ir" (`MenuId.MenubarGoMenu`), com `precondition: null`.
fn go_menu_item(order: f64) -> Action2MenuItem {
    Action2MenuItem { null_precondition: true, ..action2_menu_item(menubar_go_menu(), Some("4_symbol_nav"), Some(order)) }
}

/// `ContextKeyExpr.and(provider, [PeekContext.notInPeekEditor], [isInEmbeddedEditor.toNegated()])`.
fn precondition(provider: &RawContextKey, not_in_peek: bool, not_embedded: bool) -> Option<ContextKeyExpression> {
    ContextKeyExpr::and([
        Some(provider.expr()),
        not_in_peek.then(not_in_peek_editor),
        not_embedded.then(|| is_in_embedded_editor().to_negated()),
    ])
}

/// A `registerAction2(class ... extends XAction { ... })`: tudo que muda de uma ação para outra.
struct ActionSpec {
    id: &'static str,
    title: Title,
    precondition: Option<ContextKeyExpression>,
    keybinding: Vec<Action2Keybinding>,
    menu: Vec<Action2MenuItem>,
    configuration: SymbolNavigationActionConfig,
    kind: SymbolNavigationKind,
    /// Os `CommandsRegistry.registerCommandAlias(alias, id)` do construtor.
    aliases: &'static [&'static str],
}

fn register_spec(spec: ActionSpec) {
    let ActionSpec { id, title, precondition, keybinding, menu, configuration, kind, aliases } = spec;
    let action = SymbolNavigationAction::new(
        configuration,
        kind,
        Action2Options { command: CommandAction { precondition, ..CommandAction::new(id, title) }, menu, keybinding, f1: false },
    );
    register_action2(Rc::new(EditorAction2Adapter(SymbolNavigationActionRegistration(action))));
    for alias in aliases {
        commands_registry().register_command_alias(alias, id);
    }
}

fn config(open_to_side: bool, open_in_peek: bool) -> SymbolNavigationActionConfig {
    SymbolNavigationActionConfig { open_to_side, open_in_peek, mute_message: false }
}

/// As onze `registerAction2` do arquivo.
fn register_actions() {
    let text_focus = || Some(editor_text_focus());
    let text_focus_on_web = || ContextKeyExpr::and([Some(editor_text_focus()), Some(is_web_context().expr())]);
    let peek_menu = |order: f64| vec![action2_menu_item(editor_context_peek(), Some("peek"), Some(order))];
    let navigation_menu = |order: f64, go_order: f64| vec![action2_menu_item(editor_context(), Some("navigation"), Some(order)), go_menu_item(go_order)];

    // #region --- DEFINITION
    register_spec(ActionSpec {
        id: "editor.action.revealDefinition",
        title: localized_title_with_mnemonic("actions.goToDecl.label", "Go to Definition", "miGotoDefinition", "Go to &&Definition"),
        precondition: precondition(&has_definition_provider(), false, false),
        keybinding: vec![
            contrib_keybinding(text_focus(), primary(KeyCode::F12.bits())),
            contrib_keybinding(text_focus_on_web(), primary(KeyMod::CTRL_CMD | KeyCode::F12.bits())),
        ],
        menu: navigation_menu(1.1, 2.0),
        configuration: config(false, false),
        kind: SymbolNavigationKind::Definition,
        aliases: &["editor.action.goToDeclaration"],
    });
    register_spec(ActionSpec {
        id: "editor.action.revealDefinitionAside",
        title: localized_title("actions.goToDeclToSide.label", "Open Definition to the Side"),
        precondition: precondition(&has_definition_provider(), false, true),
        keybinding: vec![
            contrib_keybinding(text_focus(), primary(key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyCode::F12.bits()))),
            contrib_keybinding(
                text_focus_on_web(),
                primary(key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::F12.bits())),
            ),
        ],
        menu: Vec::new(),
        configuration: config(true, false),
        kind: SymbolNavigationKind::Definition,
        aliases: &["editor.action.openDeclarationToTheSide"],
    });
    register_spec(ActionSpec {
        id: "editor.action.peekDefinition",
        title: localized_title("actions.previewDecl.label", "Peek Definition"),
        precondition: precondition(&has_definition_provider(), true, true),
        keybinding: vec![contrib_keybinding(
            text_focus(),
            Keybindings {
                primary: Some(KeyMod::ALT | KeyCode::F12.bits()),
                linux: Some(PlatformKeybindings {
                    primary: Some(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::F10.bits()),
                    secondary: None,
                }),
                ..Keybindings::default()
            },
        )],
        menu: peek_menu(2.0),
        configuration: config(false, true),
        kind: SymbolNavigationKind::Definition,
        aliases: &["editor.action.previewDeclaration"],
    });
    // #endregion

    // #region --- DECLARATION
    register_spec(ActionSpec {
        id: "editor.action.revealDeclaration",
        title: localized_title_with_mnemonic(
            "actions.goToDeclaration.label",
            "Go to Declaration",
            "miGotoDeclaration",
            "Go to &&Declaration",
        ),
        precondition: precondition(&has_declaration_provider(), false, true),
        keybinding: Vec::new(),
        menu: navigation_menu(1.3, 3.0),
        configuration: config(false, false),
        kind: SymbolNavigationKind::Declaration,
        aliases: &[],
    });
    register_spec(ActionSpec {
        id: "editor.action.peekDeclaration",
        title: localized_title("actions.peekDecl.label", "Peek Declaration"),
        precondition: precondition(&has_declaration_provider(), true, true),
        keybinding: Vec::new(),
        menu: peek_menu(3.0),
        configuration: config(false, true),
        kind: SymbolNavigationKind::Declaration,
        aliases: &[],
    });
    // #endregion

    // #region --- TYPE DEFINITION
    register_spec(ActionSpec {
        id: "editor.action.goToTypeDefinition",
        title: localized_title_with_mnemonic(
            "actions.goToTypeDefinition.label",
            "Go to Type Definition",
            "miGotoTypeDefinition",
            "Go to &&Type Definition",
        ),
        precondition: precondition(&has_type_definition_provider(), false, false),
        keybinding: vec![contrib_keybinding(text_focus(), primary(0))],
        menu: navigation_menu(1.4, 3.0),
        configuration: config(false, false),
        kind: SymbolNavigationKind::TypeDefinition,
        aliases: &[],
    });
    register_spec(ActionSpec {
        id: "editor.action.peekTypeDefinition",
        title: localized_title("actions.peekTypeDefinition.label", "Peek Type Definition"),
        precondition: precondition(&has_type_definition_provider(), true, true),
        keybinding: Vec::new(),
        menu: peek_menu(4.0),
        configuration: config(false, true),
        kind: SymbolNavigationKind::TypeDefinition,
        aliases: &[],
    });
    // #endregion

    // #region --- IMPLEMENTATION
    register_spec(ActionSpec {
        id: "editor.action.goToImplementation",
        title: localized_title_with_mnemonic(
            "actions.goToImplementation.label",
            "Go to Implementations",
            "miGotoImplementation",
            "Go to &&Implementations",
        ),
        precondition: precondition(&has_implementation_provider(), false, false),
        keybinding: vec![contrib_keybinding(text_focus(), primary(KeyMod::CTRL_CMD | KeyCode::F12.bits()))],
        menu: navigation_menu(1.45, 4.0),
        configuration: config(false, false),
        kind: SymbolNavigationKind::Implementation,
        aliases: &[],
    });
    register_spec(ActionSpec {
        id: "editor.action.peekImplementation",
        title: localized_title("actions.peekImplementation.label", "Peek Implementations"),
        precondition: precondition(&has_implementation_provider(), true, true),
        keybinding: vec![contrib_keybinding(text_focus(), primary(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::F12.bits()))],
        menu: peek_menu(5.0),
        configuration: config(false, true),
        kind: SymbolNavigationKind::Implementation,
        aliases: &[],
    });
    // #endregion

    // #region --- REFERENCES
    register_spec(ActionSpec {
        id: "editor.action.goToReferences",
        title: localized_title_with_mnemonic("goToReferences.label", "Go to References", "miGotoReference", "Go to &&References"),
        precondition: precondition(&has_reference_provider(), true, true),
        keybinding: vec![contrib_keybinding(text_focus(), primary(KeyMod::SHIFT | KeyCode::F12.bits()))],
        menu: navigation_menu(1.45, 5.0),
        configuration: config(false, false),
        kind: SymbolNavigationKind::References { compact: true },
        aliases: &[],
    });
    register_spec(ActionSpec {
        id: "editor.action.referenceSearch.trigger",
        title: localized_title("references.action.label", "Peek References"),
        precondition: precondition(&has_reference_provider(), true, true),
        keybinding: Vec::new(),
        menu: peek_menu(6.0),
        configuration: config(false, true),
        kind: SymbolNavigationKind::References { compact: false },
        aliases: &[],
    });
    // #endregion
}

// #endregion

// #region --- GENERIC goto symbols command

/// `new GenericGoToLocationAction(config, references, gotoMultipleBehaviour)`, com o `noResultsMessage` da subclasse anônima
/// de `editor.action.goToLocations`.
fn generic_go_to_location_action(configuration: SymbolNavigationActionConfig, generic: GenericLocations) -> Rc<SymbolNavigationAction> {
    SymbolNavigationAction::new(
        configuration,
        SymbolNavigationKind::Generic(generic),
        Action2Options {
            command: CommandAction {
                precondition: ContextKeyExpr::and([Some(not_in_peek_editor()), Some(is_in_embedded_editor().to_negated())]),
                ..CommandAction::new("editor.action.goToLocation", localized_title("label.generic", "Go to Any Symbol"))
            },
            menu: Vec::new(),
            keybinding: Vec::new(),
            f1: false,
        },
    )
}

/// `Range.isIRange` e a leitura da faixa pelos quatro números.
fn range_from_dynamic(value: &Dynamic) -> Option<Range> {
    if value.is_nullish() {
        return None;
    }
    let number = |key: &str| match value.get_str(key) {
        Dynamic::Number(number) => Some(number as i32),
        _ => None,
    };
    Some(Range::new(number("startLineNumber")?, number("startColumn")?, number("endLineNumber")?, number("endColumn")?))
}

/// `references: Location[]` do comando: cada elemento é `{ uri, range }`.
fn locations_from_dynamic(references: &Dynamic) -> Vec<Location> {
    references
        .own_entries()
        .into_iter()
        .map(|(_, item)| {
            let (Some(uri), Some(range)) =
                (uri_from_dynamic(&item.optional_get_str("uri")), range_from_dynamic(&item.optional_get_str("range")))
            else {
                errors::throw(Error::new("Illegal argument: locations must be an array of { uri, range }"));
            };
            Location { uri, range }
        })
        .collect()
}

/// O `handler` de `editor.action.goToLocations`.
fn go_to_locations_handler(accessor: &ServicesAccessor, args: &[Dynamic]) -> Dynamic {
    let arg = |index: usize| args.get(index).cloned().unwrap_or(Dynamic::Undefined);

    let resource = uri_from_dynamic(&arg(0));
    assert_type(&resource.is_some(), None);
    let position = position_from_dynamic(&arg(1));
    assert_type(&position.is_some(), None);
    assert_type(&arg(2).is_array(), None);
    let multiple = arg(3);
    assert_type(&(multiple.is_undefined() || matches!(multiple, Dynamic::String(_))), None);
    let open_in_peek = arg(5);
    assert_type(&(open_in_peek.is_undefined() || matches!(open_in_peek, Dynamic::Bool(_))), None);
    let (Some(resource), Some(position)) = (resource, position) else {
        return Dynamic::Undefined;
    };

    let generic = GenericLocations {
        references: locations_from_dynamic(&arg(2)),
        multiple: match &multiple {
            Dynamic::String(text) => GoToLocationValues::from_text(&text.to_string_lossy()),
            _ => None,
        },
        no_results_message: match arg(4) {
            Dynamic::String(message) => Some(message.to_string_lossy()),
            _ => None,
        },
    };
    let configuration = SymbolNavigationActionConfig {
        mute_message: generic.no_results_message.as_deref().is_none_or(str::is_empty),
        open_in_peek: matches!(open_in_peek, Dynamic::Bool(true)),
        open_to_side: false,
    };

    let editor_service = accessor.get(I_CODE_EDITOR_SERVICE);
    let instantiation = accessor.get(I_INSTANTIATION_SERVICE);
    let opened = editor_service.open_code_editor(
        TextResourceEditorInput { resource, options: None },
        editor_service.get_focused_code_editor(),
        None,
    );
    let _ = opened
        .then_result_promise(move |result| match result {
            Err(error) => Promise::rejected(error),
            // `isCodeEditor(editor)`
            Ok(Some(editor)) => {
                editor.set_position(position, None);
                editor.reveal_position_in_center_if_outside_viewport(position, Some(ScrollType::Smooth));

                editor.invoke_within_context(&instantiation, |accessor| {
                    let command = generic_go_to_location_action(configuration, generic);
                    command.run(accessor, None, None)
                })
            }
            Ok(None) => Promise::resolved(()),
        })
        .catch(|error| errors::on_unexpected_error(&error));
    Dynamic::Undefined
}

/// O `handler` de `editor.action.peekLocations`.
fn peek_locations_handler(accessor: &ServicesAccessor, args: &[Dynamic]) -> Dynamic {
    let arg = |index: usize| args.get(index).cloned().unwrap_or(Dynamic::Undefined);
    let _ = accessor
        .get(I_COMMAND_SERVICE)
        .execute_command("editor.action.goToLocations", vec![arg(0), arg(1), arg(2), arg(3), Dynamic::Undefined, Dynamic::Bool(true)])
        .catch(|error| {
            errors::on_unexpected_error(&error);
            Dynamic::Undefined
        });
    Dynamic::Undefined
}

/// O `handler` de `editor.action.findReferences`.
fn find_references_handler(accessor: &ServicesAccessor, args: &[Dynamic]) -> Dynamic {
    let arg = |index: usize| args.get(index).cloned().unwrap_or(Dynamic::Undefined);
    let resource = uri_from_dynamic(&arg(0));
    assert_type(&resource.is_some(), None);
    let position = position_from_dynamic(&arg(1));
    assert_type(&position.is_some(), None);
    let (Some(resource), Some(position)) = (resource, position) else {
        return Dynamic::Undefined;
    };

    let language_features_service = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
    let code_editor_service = accessor.get(I_CODE_EDITOR_SERVICE);
    let references_controllers = accessor.try_get(I_REFERENCES_CONTROLLER_LOOKUP);
    let _ = code_editor_service
        .open_code_editor(TextResourceEditorInput { resource, options: None }, code_editor_service.get_focused_code_editor(), None)
        .then(move |control| {
            let control = control?;
            let model = control.get_model()?;
            let controller = references_controllers?.get(&control)?;

            let references = create_cancelable_promise(move |token| {
                get_references_at_position(&language_features_service.reference_provider, &model, position, false, false, &token)
                    .then(|links| ReferencesModel::new(links, localize("ref.title", "References", &[])))
            });
            let range = Range::from_positions(position, position);
            controller.toggle_widget(range, references, false);
            Some(())
        })
        .catch(|error| {
            errors::on_unexpected_error(&error);
            None
        });
    Dynamic::Undefined
}

/// Um argumento da descrição de um comando.
fn command_argument(name: &str, description: &str) -> CommandArgument {
    CommandArgument { description: Some(description.to_owned()), ..CommandArgument::new(name) }
}

fn register_command(id: &str, description: &str, args: Vec<CommandArgument>, handler: CommandHandler) {
    commands_registry().register_command(RegisteredCommand {
        id: id.to_owned(),
        handler,
        metadata: Some(CommandMetadata { description: CommandDescription::Plain(description.to_owned()), args: Some(args), returns: None }),
    });
}

// #endregion

/// O carregamento do módulo (`MenuRegistry.appendMenuItem`, os `registerAction2` e os `CommandsRegistry.registerCommand`),
/// idempotente por thread (ver "Divergências").
pub fn register() {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }

    menu_registry().append_menu_item(
        &editor_context(),
        AnyMenuItem::Submenu(SubmenuItem {
            title: Title::Plain(localize("peek.submenu", "Peek", &[])),
            submenu: editor_context_peek(),
            icon: None,
            when: None,
            group: Some("navigation".to_owned()),
            order: Some(100.0),
            is_selection: None,
            is_split_button: None,
        }),
    );

    register_actions();

    let uri = command_argument("uri", "The text document in which to start");
    let position = command_argument("position", "The position at which to start");
    let locations = command_argument("locations", "An array of locations.");
    let multiple = command_argument("multiple", "Define what to do when having multiple results, either `peek`, `gotoAndPeek`, or `goto`");

    register_command(
        "editor.action.goToLocations",
        "Go to locations from a position in a file",
        vec![
            uri.clone(),
            position.clone(),
            locations.clone(),
            multiple.clone(),
            command_argument("noResultsMessage", "Human readable message that shows when locations is empty."),
        ],
        Rc::new(go_to_locations_handler),
    );
    register_command(
        "editor.action.peekLocations",
        "Peek locations from a position in a file",
        vec![uri, position, locations, multiple],
        Rc::new(peek_locations_handler),
    );

    commands_registry().register_command(RegisteredCommand {
        id: "editor.action.findReferences".to_owned(),
        handler: Rc::new(find_references_handler),
        metadata: None,
    });

    // use NEW command
    commands_registry().register_command_alias("editor.action.showReferences", "editor.action.peekLocations");
}
