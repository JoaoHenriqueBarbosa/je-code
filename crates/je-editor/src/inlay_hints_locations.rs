//! Porte de `src/vs/editor/contrib/inlayHints/browser/inlayHintsLocations.ts`, a parte que o sticky scroll chama: o
//! `goToDefinitionWithLocation`, que abre a definição de uma localização (no editor, ao lado ou no peek, conforme o evento do
//! clique e as opções do editor) por uma `DefinitionAction`.
//!
//! Pendente: o `showGoToContextMenu`, que lê o `RenderedInlayHintLabelPart` (do `inlayHintsController.ts`, sem porte) e o
//! `IContextMenuService` para listar `SymbolNavigationAction.all()` no menu de uma dica embutida.
//!
//! Divergências:
//! - **`invokeWithinContext`.** É o [`CodeEditorWidget::invoke_within_context`], com o serviço de instanciação de quem chama
//!   como reserva para o editor que não tem o próprio.
//! - `event.hasSideBySideModifier` é lido na chamada, antes do primeiro `await`: o evento do porte é um valor, e o original o
//!   lê dentro do `invokeWithinContext` sem que ninguém o mude.
//! - A ação sem id (`id: ''`) entra em `SymbolNavigationAction.all()` sob a chave vazia, como no original, onde o construtor
//!   guarda toda instância.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Truthy;
use je_base::promise::Promise;
use je_instantiation::{I_INSTANTIATION_SERVICE, ServicesAccessor};
use je_platform::action::{CommandAction, CommandActionTitle, Title};
use je_platform::actions::Action2Options;
use je_platform::contextkey::I_CONTEXT_KEY_SERVICE;

use crate::click_link_gesture::ClickLinkMouseEvent;
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::registry::DEFINITION_LINK_OPENS_IN_PEEK;
use crate::editor_extensions::I_TEXT_MODEL_SERVICE;
use crate::goto_symbol::go_to_commands::{
    SymbolNavigationAction, SymbolNavigationActionConfig, SymbolNavigationAnchor, SymbolNavigationKind,
};
use crate::language_features_service::Location;
use crate::peek_view::in_peek_editor;

/// `goToDefinitionWithLocation(accessor, event, editor, location)`.
pub fn go_to_definition_with_location(
    accessor: &ServicesAccessor,
    event: &ClickLinkMouseEvent,
    editor: &Rc<CodeEditorWidget>,
    location: &Location,
) -> Promise<()> {
    let resolver_service = accessor.get(I_TEXT_MODEL_SERVICE);
    let instantiation = accessor.get(I_INSTANTIATION_SERVICE);
    let open_to_side = event.has_side_by_side_modifier;
    let (editor, location) = (Rc::clone(editor), location.clone());

    async_fn(async move {
        let reference = resolver_service.create_model_reference(&location.uri).await?;

        editor
            .invoke_within_context(&instantiation, |accessor| {
                let context_key_service = accessor.get(I_CONTEXT_KEY_SERVICE);

                let is_in_peek = in_peek_editor().get_value(&*context_key_service).is_truthy();
                let can_peek = !open_to_side && *editor.get_option(DEFINITION_LINK_OPENS_IN_PEEK) && !is_in_peek;

                let action = SymbolNavigationAction::new(
                    SymbolNavigationActionConfig { open_to_side, open_in_peek: can_peek, mute_message: true },
                    SymbolNavigationKind::Definition,
                    Action2Options {
                        command: CommandAction::new(
                            "",
                            Title::Localized(CommandActionTitle { value: String::new(), original: String::new(), mnemonic_title: None }),
                        ),
                        menu: Vec::new(),
                        keybinding: Vec::new(),
                        f1: false,
                    },
                );
                action.run(
                    accessor,
                    Some(SymbolNavigationAnchor { model: Rc::clone(&reference.object), position: location.range.get_start_position() }),
                    Some(location.range),
                )
            })
            .await?;

        reference.dispose();
        Ok(())
    })
}
