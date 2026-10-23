//! Porte de `src/vs/platform/keybinding/common/keybinding.ts`: o contrato do serviço de atalhos.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::key_codes::KeyCode;
use je_base::keybindings::{Keybinding, ResolvedKeybinding};
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_instantiation::service_id;

use super::keybinding_resolver::ResolutionResult;
use super::resolved_keybinding_item::ResolvedKeybindingItem;
use crate::contextkey::{IContextKeyService, IContextKeyServiceTarget};

/// `IUserFriendlyKeybinding`.
#[derive(Clone, Debug)]
pub struct UserFriendlyKeybinding {
    pub key: String,
    pub command: String,
    pub args: Dynamic,
    pub when: Option<String>,
    /// Quando `true`, o atalho é registrado como global do sistema (dispara sem o foco do app). Só desktop, e só
    /// para entradas do `keybindings.json` do usuário (não para atalhos de extensão).
    pub system_wide: Option<bool>,
}

/// `IKeyboardEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardEvent {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub alt_graph_key: bool,
    pub key_code: KeyCode,
    pub code: Option<String>,
}

/// `KeybindingsSchemaContribution`.
pub trait KeybindingsSchemaContribution {
    fn on_did_change(&self) -> Option<Event<()>> {
        None
    }
    /// `IJSONSchema[]`, como valores dinâmicos.
    fn get_schema_additions(&self) -> Vec<Dynamic>;
}

/// `IKeybindingService`.
pub trait IKeybindingService {
    fn in_chord_mode(&self) -> bool;
    fn on_did_update_keybindings(&self) -> Event<()>;

    /// Devolve nenhum, um ou vários (conforme o layout do teclado).
    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>>;
    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding>;
    fn resolve_user_binding(&self, user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>>;

    /// Resolve e despacha o evento, e invoca o comando.
    fn dispatch_event(&self, e: &KeyboardEvent, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool;
    /// Resolve e despacha o evento sem invocar o comando nem mudar o estado interno.
    fn soft_dispatch(&self, keyboard_event: &KeyboardEvent, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> ResolutionResult;

    /// Liga o modo de segurar para o comando: só se ele estiver sendo despachado (depois do keydown e antes do keyup).
    /// Devolve a promessa que resolve quando o segurar termina, ou `None` se não foi possível.
    fn enable_keybinding_hold_mode(&self, command_id: &str) -> Option<Promise<()>>;

    fn dispatch_by_user_settings_label(&self, user_settings_label: &str, target: Option<Rc<dyn IContextKeyServiceTarget>>);

    /// Os atalhos de um comando. Use `lookup_keybinding` para o preferido.
    fn lookup_keybindings(&self, command_id: &str) -> Vec<Rc<dyn ResolvedKeybinding>>;
    /// O atalho preferido (o último definido) de um comando.
    fn lookup_keybinding(&self, command_id: &str, context: Option<&dyn IContextKeyService>, enforce_context_check: bool) -> Option<Rc<dyn ResolvedKeybinding>>;

    fn get_default_keybindings_content(&self) -> String;
    fn get_default_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>>;
    fn get_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>>;
    fn custom_keybindings_count(&self) -> usize;

    /// Se a tecla produz um caractere visível, por exemplo numa caixa de texto. O resultado pode estar errado.
    fn might_produce_printable_character(&self, event: &KeyboardEvent) -> bool;

    fn register_schema_contribution(&self, contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef;

    fn toggle_logging(&self) -> bool;

    /// Dado o rótulo de um elemento de interface e o id do comando, acrescenta o rótulo do atalho: `rótulo (atalho)`.
    fn append_keybinding(&self, label: &str, command_id: Option<&str>, context: Option<&dyn IContextKeyService>, enforce_context_check: bool) -> String;

    fn dump_debug_info(&self) -> String;
    fn dump_debug_info_json(&self) -> String;
}

service_id!(pub I_KEYBINDING_SERVICE: dyn IKeybindingService = "keybindingService");
