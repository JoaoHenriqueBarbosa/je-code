//! Porte de `src/vs/platform/keybinding/common/keybindingResolver.ts`.
//!
//! Guarda os mapeamentos de atalhos para comandos e de comandos para atalhos. Dada uma sequência de acordes,
//! `resolve` diz a qual atalho ela corresponde. Os itens são compartilhados por `Rc` e comparados por identidade.

use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;

use super::resolved_keybinding_item::ResolvedKeybindingItem;
use crate::contextkey::{ContextKeyExprType, ContextKeyExpression, IContext, IContextKeyService, implies};

/// `ResultKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    /// Nenhum atalho tem essa sequência de acordes.
    NoMatchingKb,
    /// Há vários atalhos que têm a sequência como prefixo.
    MoreChordsNeeded,
    /// Um atalho único, a ser despachado e invocado.
    KbFound,
}

/// `ResolutionResult`.
#[derive(Clone, Debug, PartialEq)]
pub enum ResolutionResult {
    NoMatchingKb,
    MoreChordsNeeded,
    KbFound { command_id: Option<String>, command_args: Dynamic, is_bubble: bool },
}

impl ResolutionResult {
    pub fn kind(&self) -> ResultKind {
        match self {
            ResolutionResult::NoMatchingKb => ResultKind::NoMatchingKb,
            ResolutionResult::MoreChordsNeeded => ResultKind::MoreChordsNeeded,
            ResolutionResult::KbFound { .. } => ResultKind::KbFound,
        }
    }
}

/// `KeybindingResolver`.
pub struct KeybindingResolver {
    log: Rc<dyn Fn(&str)>,
    default_keybindings: Vec<Rc<ResolvedKeybindingItem>>,
    keybindings: Vec<Rc<ResolvedKeybindingItem>>,
    default_bound_commands: HashMap<String, bool>,
    /// Pelo primeiro acorde.
    map: HashMap<String, Vec<Rc<ResolvedKeybindingItem>>>,
    /// Pelo id do comando.
    lookup_map: HashMap<String, Vec<Rc<ResolvedKeybindingItem>>>,
}

impl KeybindingResolver {
    /// `default_keybindings`: os atalhos de fábrica e os de extensões. `overrides`: os do usuário.
    pub fn new(default_keybindings: Vec<Rc<ResolvedKeybindingItem>>, overrides: Vec<Rc<ResolvedKeybindingItem>>, log: Rc<dyn Fn(&str)>) -> Self {
        let mut default_bound_commands = HashMap::new();
        for default_keybinding in &default_keybindings {
            if let Some(command) = &default_keybinding.command
                && !command.starts_with('-')
            {
                default_bound_commands.insert(command.clone(), true);
            }
        }

        let all: Vec<Rc<ResolvedKeybindingItem>> = default_keybindings.iter().cloned().chain(overrides).collect();
        let keybindings = Self::handle_removals(all);
        let mut resolver = Self {
            log,
            default_keybindings,
            keybindings: Vec::new(),
            default_bound_commands,
            map: HashMap::new(),
            lookup_map: HashMap::new(),
        };
        for k in &keybindings {
            if k.chords.is_empty() {
                // unbound
                continue;
            }
            // substitute with constants that are registered after startup
            let when = k.when.as_ref().and_then(ContextKeyExpression::substitute_constants);
            if when.is_some_and(|w| w.expr_type() == ContextKeyExprType::False) {
                // when condition is false
                continue;
            }
            resolver.add_key_press(&k.chords[0], k.clone());
        }
        resolver.keybindings = keybindings;
        resolver
    }

    fn is_targeted_for_removal(default_kb: &ResolvedKeybindingItem, keypress: &[String], when: Option<&ContextKeyExpression>) -> bool {
        for (i, press) in keypress.iter().enumerate() {
            if Some(press) != default_kb.chords.get(i) {
                return false;
            }
        }
        // `true` means always, as does `undefined`, so we will treat `true` === `undefined`
        if let Some(when) = when
            && when.expr_type() != ContextKeyExprType::True
        {
            let Some(default_when) = &default_kb.when else {
                return false;
            };
            // Use implication instead of strict equality so that a removal still matches when the default
            // keybinding's when clause becomes more specific across updates.
            let default_when = default_when.substitute_constants();
            let removal_when = when.substitute_constants();
            if !Self::when_is_entirely_included(default_when.as_ref(), removal_when.as_ref()) {
                return false;
            }
        }
        true
    }

    /// Procura as regras com `-commandId` e as remove.
    pub fn handle_removals(rules: Vec<Rc<ResolvedKeybindingItem>>) -> Vec<Rc<ResolvedKeybindingItem>> {
        // Do a first pass and construct a hash-map for removals
        let mut removals: HashMap<String, Vec<Rc<ResolvedKeybindingItem>>> = HashMap::new();
        for rule in &rules {
            if let Some(command) = &rule.command
                && let Some(stripped) = command.strip_prefix('-')
            {
                removals.entry(stripped.to_string()).or_default().push(rule.clone());
            }
        }
        if removals.is_empty() {
            // There are no removals
            return rules;
        }

        // Do a second pass and keep only non-removed keybindings
        let mut result = Vec::new();
        for rule in rules {
            let command = match &rule.command {
                Some(command) if !command.is_empty() => command.clone(),
                _ => {
                    result.push(rule);
                    continue;
                }
            };
            if command.starts_with('-') {
                continue;
            }
            let Some(command_removals) = removals.get(&command).filter(|_| rule.is_default) else {
                result.push(rule);
                continue;
            };
            let is_removed = command_removals.iter().any(|removal| Self::is_targeted_for_removal(&rule, &removal.chords, removal.when.as_ref()));
            if !is_removed {
                result.push(rule);
            }
        }
        result
    }

    fn add_key_press(&mut self, keypress: &str, item: Rc<ResolvedKeybindingItem>) {
        let Some(conflicts) = self.map.get(keypress).cloned() else {
            // There is no conflict so far
            self.map.insert(keypress.to_string(), vec![item.clone()]);
            self.add_to_lookup_map(&item);
            return;
        };

        for conflict in conflicts.iter().rev() {
            if conflict.command == item.command {
                continue;
            }
            // Test if the shorter keybinding is a prefix of the longer one. If the shorter keybinding is a prefix,
            // it effectively will shadow the longer one and is considered a conflict.
            let mut is_shorter_kb_prefix = true;
            let mut i = 1;
            while i < conflict.chords.len() && i < item.chords.len() {
                if conflict.chords[i] != item.chords[i] {
                    // The ith step does not conflict
                    is_shorter_kb_prefix = false;
                    break;
                }
                i += 1;
            }
            if !is_shorter_kb_prefix {
                continue;
            }
            if Self::when_is_entirely_included(conflict.when.as_ref(), item.when.as_ref()) {
                // `item` completely overwrites `conflict`: remove conflict from the lookupMap
                self.remove_from_lookup_map(conflict);
            }
        }

        self.map.get_mut(keypress).expect("a lista de conflitos existe").push(item.clone());
        self.add_to_lookup_map(&item);
    }

    fn add_to_lookup_map(&mut self, item: &Rc<ResolvedKeybindingItem>) {
        let Some(command) = &item.command else {
            return;
        };
        self.lookup_map.entry(command.clone()).or_default().push(item.clone());
    }

    fn remove_from_lookup_map(&mut self, item: &Rc<ResolvedKeybindingItem>) {
        let Some(command) = &item.command else {
            return;
        };
        let Some(arr) = self.lookup_map.get_mut(command) else {
            return;
        };
        if let Some(pos) = arr.iter().position(|candidate| Rc::ptr_eq(candidate, item)) {
            arr.remove(pos);
        }
    }

    /// Verdadeiro se for demonstrável que `a` implica `b`.
    pub fn when_is_entirely_included(a: Option<&ContextKeyExpression>, b: Option<&ContextKeyExpression>) -> bool {
        let Some(b) = b.filter(|b| b.expr_type() != ContextKeyExprType::True) else {
            return true;
        };
        let Some(a) = a.filter(|a| a.expr_type() != ContextKeyExprType::True) else {
            return false;
        };
        implies(a, b)
    }

    pub fn get_default_bound_commands(&self) -> &HashMap<String, bool> {
        &self.default_bound_commands
    }

    pub fn get_default_keybindings(&self) -> &[Rc<ResolvedKeybindingItem>] {
        &self.default_keybindings
    }

    pub fn get_keybindings(&self) -> &[Rc<ResolvedKeybindingItem>] {
        &self.keybindings
    }

    /// Do mais específico (o último definido) ao menos.
    pub fn lookup_keybindings(&self, command_id: &str) -> Vec<Rc<ResolvedKeybindingItem>> {
        match self.lookup_map.get(command_id) {
            Some(items) if !items.is_empty() => items.iter().rev().cloned().collect(),
            _ => Vec::new(),
        }
    }

    pub fn lookup_primary_keybinding(&self, command_id: &str, context: &dyn IContextKeyService, enforce_context_check: bool) -> Option<Rc<ResolvedKeybindingItem>> {
        let items = self.lookup_map.get(command_id).filter(|items| !items.is_empty())?;
        if items.len() == 1 && !enforce_context_check {
            return Some(items[0].clone());
        }
        for item in items.iter().rev() {
            if context.context_matches_rules(item.when.as_ref()) {
                return Some(item.clone());
            }
        }
        if enforce_context_check {
            return None;
        }
        items.last().cloned()
    }

    /// Procura o atalho disparado ao pressionar a sequência `[...current_chords, keypress]`.
    ///
    /// Exemplo: três acordes pressionados em sequência, `cmd+k cmd+p cmd+i`: `current_chords = ['cmd+k', 'cmd+p']`
    /// e `keypress = 'cmd+i'`, o último acorde pressionado.
    pub fn resolve(&self, context: &dyn IContext, current_chords: &[String], keypress: &str) -> ResolutionResult {
        let mut pressed_chords: Vec<String> = current_chords.to_vec();
        pressed_chords.push(keypress.to_string());
        (self.log)(&format!("| Resolving {}", pressed_chords.join(",")));
        let Some(kb_candidates) = self.map.get(&pressed_chords[0]) else {
            // No bindings with such 0-th chord
            (self.log)("\\ No keybinding entries.");
            return ResolutionResult::NoMatchingKb;
        };

        let lookup_map: Vec<Rc<ResolvedKeybindingItem>> = if pressed_chords.len() < 2 {
            kb_candidates.clone()
        } else {
            // Fetch all chord bindings for `currentChords`
            let mut lookup_map = Vec::new();
            for candidate in kb_candidates {
                if pressed_chords.len() > candidate.chords.len() {
                    // # of pressed chords can't be less than # of chords in a keybinding to invoke
                    continue;
                }
                let prefix_matches = (1..pressed_chords.len()).all(|i| candidate.chords[i] == pressed_chords[i]);
                if prefix_matches {
                    lookup_map.push(candidate.clone());
                }
            }
            lookup_map
        };

        // check there's a keybinding with a matching when clause
        let Some(result) = Self::find_command(context, &lookup_map) else {
            (self.log)(&format!("\\ From {} keybinding entries, no when clauses matched the context.", lookup_map.len()));
            return ResolutionResult::NoMatchingKb;
        };

        // check we got all chords necessary to be sure a particular keybinding needs to be invoked
        if pressed_chords.len() < result.chords.len() {
            // The chord sequence is not complete
            (self.log)(&format!(
                "\\ From {} keybinding entries, awaiting {} more chord(s), when: {}, source: {}.",
                lookup_map.len(),
                result.chords.len() - pressed_chords.len(),
                print_when_explanation(result.when.as_ref()),
                print_source_explanation(&result)
            ));
            return ResolutionResult::MoreChordsNeeded;
        }

        (self.log)(&format!(
            "\\ From {} keybinding entries, matched {}, when: {}, source: {}.",
            lookup_map.len(),
            result.command.as_deref().unwrap_or("null"),
            print_when_explanation(result.when.as_ref()),
            print_source_explanation(&result)
        ));
        ResolutionResult::KbFound { command_id: result.command.clone(), command_args: result.command_args.clone(), is_bubble: result.bubble }
    }

    fn find_command(context: &dyn IContext, matches: &[Rc<ResolvedKeybindingItem>]) -> Option<Rc<ResolvedKeybindingItem>> {
        matches.iter().rev().find(|k| Self::context_matches_rules(context, k.when.as_ref())).cloned()
    }

    fn context_matches_rules(context: &dyn IContext, rules: Option<&ContextKeyExpression>) -> bool {
        match rules {
            None => true,
            Some(rules) => rules.evaluate(context),
        }
    }
}

fn print_when_explanation(when: Option<&ContextKeyExpression>) -> String {
    match when {
        None => "no when condition".to_string(),
        Some(when) => when.serialize(),
    }
}

fn print_source_explanation(kb: &ResolvedKeybindingItem) -> String {
    match &kb.extension_id {
        Some(id) => {
            if kb.is_builtin_extension {
                format!("built-in extension {id}")
            } else {
                format!("user extension {id}")
            }
        }
        None => (if kb.is_default { "built-in" } else { "user" }).to_string(),
    }
}
