//! Porte de `src/vs/base/test/browser/ui/iconLabel/iconLabel.test.ts` (suíte `IconLabel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `for (const supportHighlights of [false, true])` gera dois `test(...)`: um `#[test]` para cada valor.
//! - `getComputedStyle(...)` (`fontStyle`, `textDecorationLine`, `fontWeight`) é [`computed_text_state`]: as
//!   regras `.bold`, `.italic` e `.strikethrough` de `iconlabel.css` aplicadas às classes do `.monaco-icon-label`
//!   antepassado (o `je-ui` não tem cascata). Que o texto é mesmo desenhado assim está em `icon_label_extra.rs`.
//! - `container.querySelector('.label-name')` é `support::query_all`.

mod support;

use je_base::lifecycle::to_disposable;
use je_render::Rgba;
use je_ui::{Role, Ui};
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabel, IconLabelStyles, TextState, computed_text_state};
use support::{ensure_no_disposables_are_leaked_in_test_suite, query_all};

fn styles() -> IconLabelStyles {
    IconLabelStyles {
        text: HighlightedLabelStyles {
            foreground: Rgba::opaque(0xcc, 0xcc, 0xcc),
            highlight_foreground: Some(Rgba::opaque(0x2a, 0xaa, 0xff)),
            highlight_background: None,
            highlight_weight: 400,
            font_size: 13.0,
            font_weight: 400,
            line_height: Some(22.0),
            italic: false,
            strikethrough: false,
            inherit_foreground: false,
        },
        disabled_foreground: None,
    }
}

fn name_suffix_and_description_share_state_styles(support_highlights: bool) {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        create_el(&mut tree, root, Role::Generic, "icon-label-test")
    };
    {
        let ui = ui.clone();
        disposables.add(to_disposable(move || ui.tree_mut().remove(container)));
    }
    let label = disposables.add_rc(IconLabel::new(
        &ui,
        container,
        Some(IIconLabelCreationOptions { support_highlights, support_description_highlights: support_highlights, ..Default::default() }),
        styles(),
    ));
    let mut states = Vec::new();
    let option_sets = [
        IIconLabelValueOptions { italic: true, ..Default::default() },
        IIconLabelValueOptions { strikethrough: true, ..Default::default() },
        IIconLabelValueOptions { bold: true, ..Default::default() },
        IIconLabelValueOptions::default(),
    ];
    for options in option_sets {
        label.set_label("filename".into(), Some("description"), Some(&IIconLabelValueOptions { suffix: Some(".ts".to_owned()), ..options }));
        let tree = ui.tree();
        states.push(["label-name", "label-suffix", "label-description"].map(|selector| computed_text_state(&tree, query_all(&tree, container, &[selector])[0])));
    }
    let state = |italic, strikethrough, bold| [TextState { italic, strikethrough, bold }; 3];
    assert_eq!(states, [state(true, false, false), state(false, true, false), state(false, false, true), state(false, false, false)]);
}

#[test]
fn name_suffix_and_description_share_state_styles_highlights_false() {
    name_suffix_and_description_share_state_styles(false);
}

#[test]
fn name_suffix_and_description_share_state_styles_highlights_true() {
    name_suffix_and_description_share_state_styles(true);
}
