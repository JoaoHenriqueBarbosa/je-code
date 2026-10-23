//! Porte de `src/vs/base/browser/ui/findinput/findInputToggles.ts`: `navigateToggles` (as setas e o Escape
//! entre os botões de um campo de busca) e os três botões comuns do VS Code, `CaseSensitiveToggle`,
//! `WholeWordsToggle` e `RegexToggle`, mais o `PreserveCaseToggle` do `replaceInput.ts`.
//!
//! Como o TypeScript virou Rust:
//!
//! - Em TypeScript os quatro são subclasses de `Toggle` que só fixam o ícone e o título. Aqui são funções
//!   que constroem o [`Toggle`] com o ícone (`case-sensitive`, `whole-word`, `regex`, `preserve-case`) e o
//!   título (a descrição fixa mais o `appendTitle`, que o dono usa para acrescentar o atalho de teclado).
//!   O resultado é um `Rc<Toggle>` comum, sem tipo próprio.
//! - `IFindInputToggleOpts` é [`FindInputToggleOpts`]: as três cores `inputActiveOption*` do original vêm
//!   dentro de [`ToggleStyles`] (que ainda leva o hover, o foco e a cor herdada que o CSS lê por variável).
//! - `navigateToggles` recebe o [`Ui`] (o `document.activeElement` é o nó com foco da árvore) e o evento
//!   cru do `je-ui`, porque o `EventHelper.stop(event, true)` do original é `preventDefault` e
//!   `stopPropagation` sobre ele. Os botões são resolvidos por um fecho que só roda nas teclas que navegam.
//!
//! As descrições (`Match Case`, `Match Whole Word`, `Use Regular Expression`, `Preserve Case`) ficam em
//! inglês: a localização (`nls.localize`) é do produto, não dos widgets.

use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::themables::ThemeIcon;
use je_ui::{Event, NodeId, Ui};

use crate::dom::StandardKeyboardEvent;
use crate::hover_api::IHoverLifecycleOptions;
use crate::toggle::{Toggle, ToggleOpts, ToggleStyles, ToggleTitle};

/// `IFindInputToggleOpts`.
#[derive(Clone)]
pub struct FindInputToggleOpts {
    pub append_title: String,
    pub is_checked: bool,
    pub styles: ToggleStyles,
    pub hover_lifecycle_options: Option<IHoverLifecycleOptions>,
}

impl FindInputToggleOpts {
    pub fn new(styles: ToggleStyles) -> Self {
        Self { append_title: String::new(), is_checked: false, styles, hover_lifecycle_options: None }
    }
}

/// `navigateToggles`: as setas esquerda e direita levam o foco ao botão vizinho (dando a volta nas pontas)
/// e o Escape o devolve ao campo. Só age se o foco está num dos botões; nesse caso consome a tecla.
pub fn navigate_toggles(ui: &Ui, event: &mut Event, get_toggle_dom_nodes: impl FnOnce() -> Vec<NodeId>, focus_input: impl FnOnce()) {
    let Some(key) = StandardKeyboardEvent::from_event(event) else { return };
    let (left, right, escape) = (key.equals(KeyCode::LeftArrow.bits()), key.equals(KeyCode::RightArrow.bits()), key.equals(KeyCode::Escape.bits()));
    if !(left || right || escape) {
        return;
    }
    let indexes = get_toggle_dom_nodes();
    let Some(index) = ui.focused().and_then(|active| indexes.iter().position(|node| *node == active)) else { return };
    if escape {
        ui.blur();
        focus_input();
    } else if right {
        ui.focus(indexes[(index + 1) % indexes.len()]);
    } else {
        ui.focus(indexes[(index + indexes.len() - 1) % indexes.len()]);
    }
    event.stop(true);
}

const CASE_SENSITIVE_TOGGLE_LABEL: &str = "Match Case";
const WHOLE_WORD_TOGGLE_LABEL: &str = "Match Whole Word";
const REGEX_TOGGLE_LABEL: &str = "Use Regular Expression";
const PRESERVE_CASE_LABEL: &str = "Preserve Case";

/// O que as quatro subclasses de `Toggle` fazem igual: o ícone e o título (descrição mais `appendTitle`).
fn find_input_toggle(ui: &Ui, icon_id: &str, label: &str, opts: FindInputToggleOpts) -> Rc<Toggle> {
    let title = ToggleTitle::Text(format!("{label}{}", opts.append_title));
    Toggle::new(
        ui,
        ToggleOpts {
            icon: Some(ThemeIcon::from_id(icon_id)),
            hover_lifecycle_options: opts.hover_lifecycle_options,
            ..ToggleOpts::new(title, opts.is_checked, opts.styles)
        },
    )
}

/// `new CaseSensitiveToggle(opts)`.
pub fn case_sensitive_toggle(ui: &Ui, opts: FindInputToggleOpts) -> Rc<Toggle> {
    find_input_toggle(ui, "case-sensitive", CASE_SENSITIVE_TOGGLE_LABEL, opts)
}

/// `new WholeWordsToggle(opts)`.
pub fn whole_words_toggle(ui: &Ui, opts: FindInputToggleOpts) -> Rc<Toggle> {
    find_input_toggle(ui, "whole-word", WHOLE_WORD_TOGGLE_LABEL, opts)
}

/// `new RegexToggle(opts)`.
pub fn regex_toggle(ui: &Ui, opts: FindInputToggleOpts) -> Rc<Toggle> {
    find_input_toggle(ui, "regex", REGEX_TOGGLE_LABEL, opts)
}

/// `new PreserveCaseToggle(opts)` (do `replaceInput.ts`).
pub fn preserve_case_toggle(ui: &Ui, opts: FindInputToggleOpts) -> Rc<Toggle> {
    find_input_toggle(ui, "preserve-case", PRESERVE_CASE_LABEL, opts)
}
