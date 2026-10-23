//! Porte de `src/vs/platform/keybinding/common/keybindingsRegistry.ts`: guarda todos os atalhos de fábrica e de
//! extensões (mas não os que o usuário define).
//!
//! `OS` é o sistema detectado na partida ([`je_base::platform::Platform::detect`]); `Registry.add(...)` do
//! original (o registro genérico de pontos de extensão) não existe aqui: o registro é o singleton por thread de
//! [`keybindings_registry`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::keybindings::{Keybinding, decode_keybinding};
use je_base::lifecycle::{DisposableRef, DisposableStore, to_disposable};
use je_base::linked_list::LinkedList;
use je_base::platform::{OperatingSystem, Platform};

use crate::commands::{Command, CommandHandler, CommandMetadata, commands_registry};
use crate::contextkey::ContextKeyExpression;

/// `IKeybindingItem`.
#[derive(Clone)]
pub struct KeybindingItem {
    pub keybinding: Option<Keybinding>,
    pub command: Option<String>,
    pub command_args: Dynamic,
    pub when: Option<ContextKeyExpression>,
    pub weight1: i64,
    pub weight2: i64,
    pub extension_id: Option<String>,
    pub is_builtin_extension: bool,
}

/// `{ primary?, secondary? }`: as formas binárias do atalho para uma plataforma.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlatformKeybindings {
    pub primary: Option<u32>,
    pub secondary: Option<Vec<u32>>,
}

/// `IKeybindings`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keybindings {
    pub primary: Option<u32>,
    pub secondary: Option<Vec<u32>>,
    pub win: Option<PlatformKeybindings>,
    pub linux: Option<PlatformKeybindings>,
    pub mac: Option<PlatformKeybindings>,
}

/// `IKeybindingRule`.
#[derive(Clone)]
pub struct KeybindingRule {
    pub keybindings: Keybindings,
    pub id: String,
    pub weight: i64,
    pub args: Dynamic,
    /// O atalho fica desativado se a expressão der falso.
    pub when: Option<ContextKeyExpression>,
}

/// `IExtensionKeybindingRule`.
#[derive(Clone)]
pub struct ExtensionKeybindingRule {
    pub keybinding: Option<Keybinding>,
    pub id: String,
    pub args: Dynamic,
    pub weight: i64,
    pub when: Option<ContextKeyExpression>,
    pub extension_id: Option<String>,
    pub is_builtin_extension: bool,
}

/// `KeybindingWeight`.
pub struct KeybindingWeight;

impl KeybindingWeight {
    pub const EDITOR_CORE: i64 = 0;
    pub const EDITOR_CONTRIB: i64 = 100;
    pub const WORKBENCH_CONTRIB: i64 = 200;
    pub const SESSIONS_CONTRIB: i64 = 250;
    pub const BUILTIN_EXTENSION: i64 = 300;
    pub const EXTERNAL_EXTENSION: i64 = 400;
}

/// `ICommandAndKeybindingRule`.
pub struct CommandAndKeybindingRule {
    pub rule: KeybindingRule,
    pub handler: CommandHandler,
    pub metadata: Option<CommandMetadata>,
}

/// `KeybindingsRegistryImpl`.
pub struct KeybindingsRegistry {
    core_keybindings: LinkedList<KeybindingItem>,
    core_keybinding_rules: LinkedList<KeybindingRule>,
    extension_keybindings: RefCell<Vec<KeybindingItem>>,
    cached_merged_keybindings: RefCell<Option<Vec<KeybindingItem>>>,
    os: OperatingSystem,
}

thread_local! {
    static KEYBINDINGS_REGISTRY: Rc<KeybindingsRegistry> = Rc::new(KeybindingsRegistry::new(Platform::detect().os));
}

/// `KeybindingsRegistry`.
pub fn keybindings_registry() -> Rc<KeybindingsRegistry> {
    KEYBINDINGS_REGISTRY.with(Rc::clone)
}

/// O texto compara como o `<` do JavaScript: unidades UTF-16.
fn sorter(a: &KeybindingItem, b: &KeybindingItem) -> std::cmp::Ordering {
    if a.weight1 != b.weight1 {
        return a.weight1.cmp(&b.weight1);
    }
    if let (Some(a), Some(b)) = (&a.command, &b.command) {
        match a.encode_utf16().cmp(b.encode_utf16()) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    a.weight2.cmp(&b.weight2)
}

fn bind_to_platform(kb: &Keybindings, os: OperatingSystem) -> PlatformKeybindings {
    let chosen = match os {
        OperatingSystem::Windows => kb.win.as_ref(),
        OperatingSystem::Macintosh => kb.mac.as_ref(),
        OperatingSystem::Linux => kb.linux.as_ref(),
    };
    match chosen {
        Some(platform) => platform.clone(),
        None => PlatformKeybindings { primary: kb.primary, secondary: kb.secondary.clone() },
    }
}

impl KeybindingsRegistry {
    pub fn new(os: OperatingSystem) -> Self {
        Self {
            core_keybindings: LinkedList::new(),
            core_keybinding_rules: LinkedList::new(),
            extension_keybindings: RefCell::new(Vec::new()),
            cached_merged_keybindings: RefCell::new(None),
            os,
        }
    }

    /// `registerKeybindingRule`.
    pub fn register_keybinding_rule(self: &Rc<Self>, rule: KeybindingRule) -> DisposableRef {
        let actual_kb = bind_to_platform(&rule.keybindings, self.os);
        let result = DisposableStore::new();

        // `primary` 0 é falso no JavaScript: não registra.
        if let Some(primary) = actual_kb.primary.filter(|p| *p != 0)
            && let Ok(Some(kk)) = decode_keybinding(primary, self.os)
        {
            result.add(self.register_default_keybinding(kk, &rule.id, rule.args.clone(), rule.weight, 0, rule.when.clone()));
        }
        if let Some(secondary) = &actual_kb.secondary {
            for (i, k) in secondary.iter().enumerate() {
                if let Ok(Some(kk)) = decode_keybinding(*k, self.os) {
                    result.add(self.register_default_keybinding(kk, &rule.id, rule.args.clone(), rule.weight, -(i as i64) - 1, rule.when.clone()));
                }
            }
        }

        let remove_rule = self.core_keybinding_rules.push(rule);
        result.add(to_disposable(move || remove_rule.remove()));
        result
    }

    /// `setExtensionKeybindings`.
    pub fn set_extension_keybindings(&self, rules: Vec<ExtensionKeybindingRule>) {
        let result: Vec<KeybindingItem> = rules
            .into_iter()
            .filter_map(|rule| {
                rule.keybinding.map(|keybinding| KeybindingItem {
                    keybinding: Some(keybinding),
                    command: Some(rule.id),
                    command_args: rule.args,
                    when: rule.when,
                    weight1: rule.weight,
                    weight2: 0,
                    extension_id: rule.extension_id.filter(|id| !id.is_empty()),
                    is_builtin_extension: rule.is_builtin_extension,
                })
            })
            .collect();
        *self.extension_keybindings.borrow_mut() = result;
        *self.cached_merged_keybindings.borrow_mut() = None;
    }

    /// `registerCommandAndKeybindingRule`.
    pub fn register_command_and_keybinding_rule(self: &Rc<Self>, desc: CommandAndKeybindingRule) -> DisposableRef {
        let command = Command { id: desc.rule.id.clone(), handler: desc.handler, metadata: desc.metadata };
        je_base::lifecycle::combined_disposable(vec![self.register_keybinding_rule(desc.rule), commands_registry().register_command(command)])
    }

    fn register_default_keybinding(
        self: &Rc<Self>,
        keybinding: Keybinding,
        command_id: &str,
        command_args: Dynamic,
        weight1: i64,
        weight2: i64,
        when: Option<ContextKeyExpression>,
    ) -> DisposableRef {
        let remove = self.core_keybindings.push(KeybindingItem {
            keybinding: Some(keybinding),
            command: Some(command_id.to_string()),
            command_args,
            when,
            weight1,
            weight2,
            extension_id: None,
            is_builtin_extension: false,
        });
        *self.cached_merged_keybindings.borrow_mut() = None;
        let this = Rc::clone(self);
        to_disposable(move || {
            remove.remove();
            *this.cached_merged_keybindings.borrow_mut() = None;
        })
    }

    /// `getDefaultKeybindings`.
    pub fn get_default_keybindings(&self) -> Vec<KeybindingItem> {
        let mut cache = self.cached_merged_keybindings.borrow_mut();
        let merged = cache.get_or_insert_with(|| {
            let mut merged: Vec<KeybindingItem> = self.core_keybindings.iter().collect();
            merged.extend(self.extension_keybindings.borrow().iter().cloned());
            merged.sort_by(sorter);
            merged
        });
        merged.clone()
    }

    /// `getDefaultKeybindingsForOS`.
    pub fn get_default_keybindings_for_os(&self, os: OperatingSystem) -> Vec<KeybindingItem> {
        let mut result = Vec::new();
        for rule in self.core_keybinding_rules.iter() {
            let actual_kb = bind_to_platform(&rule.keybindings, os);
            let make = |kk: Keybinding, weight2: i64| KeybindingItem {
                keybinding: Some(kk),
                command: Some(rule.id.clone()),
                command_args: rule.args.clone(),
                when: rule.when.clone(),
                weight1: rule.weight,
                weight2,
                extension_id: None,
                is_builtin_extension: false,
            };
            if let Some(primary) = actual_kb.primary.filter(|p| *p != 0)
                && let Ok(Some(kk)) = decode_keybinding(primary, os)
            {
                result.push(make(kk, 0));
            }
            if let Some(secondary) = &actual_kb.secondary {
                for (i, k) in secondary.iter().enumerate() {
                    if let Ok(Some(kk)) = decode_keybinding(*k, os) {
                        result.push(make(kk, -(i as i64) - 1));
                    }
                }
            }
        }
        result.sort_by(sorter);
        result
    }
}
