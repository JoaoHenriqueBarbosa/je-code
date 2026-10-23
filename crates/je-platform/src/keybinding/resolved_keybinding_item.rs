//! Porte de `src/vs/platform/keybinding/common/resolvedKeybindingItem.ts`.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::keybindings::ResolvedKeybinding;

use crate::contextkey::ContextKeyExpression;

/// `ResolvedKeybindingItem`. É compartilhado por `Rc`: o resolvedor compara os itens por identidade.
pub struct ResolvedKeybindingItem {
    pub resolved_keybinding: Option<Rc<dyn ResolvedKeybinding>>,
    pub chords: Vec<String>,
    pub bubble: bool,
    pub command: Option<String>,
    pub command_args: Dynamic,
    pub when: Option<ContextKeyExpression>,
    pub is_default: bool,
    pub extension_id: Option<String>,
    pub is_builtin_extension: bool,
    /// Se o atalho foi declarado como global do sistema pelo `keybindings.json`: só vale para atalhos do usuário.
    pub system_wide: bool,
}

impl ResolvedKeybindingItem {
    #[allow(clippy::too_many_arguments)] // os oito parâmetros do construtor original
    pub fn new(
        resolved_keybinding: Option<Rc<dyn ResolvedKeybinding>>,
        command: Option<&str>,
        command_args: Dynamic,
        when: Option<ContextKeyExpression>,
        is_default: bool,
        extension_id: Option<&str>,
        is_builtin_extension: bool,
        system_wide: bool,
    ) -> Rc<Self> {
        let mut chords = resolved_keybinding
            .as_ref()
            .map(|kb| to_empty_array_if_contains_none(kb.get_dispatch_chords()))
            .unwrap_or_default();
        if let Some(kb) = &resolved_keybinding
            && chords.is_empty()
        {
            // handle possible single modifier chord keybindings
            chords = to_empty_array_if_contains_none(kb.get_single_modifier_dispatch_chords())
                .into_iter()
                .map(|m| single_modifier_name(m).to_string())
                .collect();
        }
        let bubble = command.is_some_and(|c| c.starts_with('^'));
        let command = match command {
            Some(c) if bubble => Some(c[1..].to_string()),
            Some(c) => Some(c.to_string()),
            None => None,
        };
        Rc::new(Self {
            resolved_keybinding,
            chords,
            bubble,
            command,
            command_args,
            when,
            is_default,
            extension_id: extension_id.map(str::to_string),
            is_builtin_extension,
            system_wide,
        })
    }
}

/// O texto de um `SingleModifierChord` (`'ctrl' | 'shift' | 'alt' | 'meta'`).
pub fn single_modifier_name(modifier: je_base::keybindings::SingleModifierChord) -> &'static str {
    use je_base::keybindings::SingleModifierChord;
    match modifier {
        SingleModifierChord::Ctrl => "ctrl",
        SingleModifierChord::Shift => "shift",
        SingleModifierChord::Alt => "alt",
        SingleModifierChord::Meta => "meta",
    }
}

/// `toEmptyArrayIfContainsNull`: a lista vazia se algum elemento for nulo. (Os acordes de texto vazio, que o
/// JavaScript também trata como falsos, não ocorrem: o despacho nunca devolve `""`.)
pub fn to_empty_array_if_contains_none<T>(arr: Vec<Option<T>>) -> Vec<T> {
    let mut result = Vec::with_capacity(arr.len());
    for element in arr {
        match element {
            Some(element) => result.push(element),
            None => return Vec::new(),
        }
    }
    result
}
