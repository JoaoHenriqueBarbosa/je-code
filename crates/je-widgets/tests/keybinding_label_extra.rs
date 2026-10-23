//! Testes extras do `KeybindingLabel` (o original não tem teste dedicado): estrutura de nós, rótulos por sistema
//! operacional, realces, atalho ausente, `title` e as medidas de `keybindingLabel.css`.

mod hover_fixture;
mod keybinding_fixture;
mod support;

use std::rc::Rc;

use je_base::keybindings::ResolvedKeybinding;
use je_base::platform::OperatingSystem;
use je_render::Rgba;
use je_ui::{Content, NodeId, Tree, Ui};
use je_widgets::keybinding_label::{ChordMatches, IKeybindingLabelStyles, KeybindingLabel, KeybindingLabelOptions, Matches, unthemed_keybinding_label_options};
use je_widgets::text::{UI_FONT_SIZE, measure_text, ui_font};
use keybinding_fixture::{ChordSpec, FixtureKeybinding};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

fn keybinding(os: OperatingSystem, chords: &[ChordSpec]) -> Option<Rc<dyn ResolvedKeybinding>> {
    Some(Rc::new(FixtureKeybinding::new(os, chords)))
}

/// A primeira linha de texto sob o nó (o texto da tecla ou do separador).
fn text_under(tree: &Tree, node: NodeId) -> String {
    tree.subtree(node)
        .into_iter()
        .find_map(|n| match &tree.style(n).content {
            Content::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// `(classe, texto)` de cada filho do `.monaco-keybinding`.
fn children(ui: &Ui, label: &KeybindingLabel) -> Vec<(String, String)> {
    let tree = ui.tree();
    tree.children(label.element()).iter().map(|c| (tree.classes(*c).join(" "), text_under(&tree, *c))).collect()
}

fn key(text: &str) -> (String, String) {
    ("monaco-keybinding-key".to_owned(), text.to_owned())
}

fn separator(text: &str) -> (String, String) {
    ("monaco-keybinding-key-separator".to_owned(), text.to_owned())
}

#[test]
fn renders_ctrl_shift_p_on_linux_with_plus_separators() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    label.set(keybinding(OperatingSystem::Linux, &[(true, true, false, false, "P")]), None);
    assert!(ui.tree().has_class(label.element(), "monaco-keybinding"));
    assert_eq!(children(&ui, &label), [key("Ctrl"), separator("+"), key("Shift"), separator("+"), key("P")]);
    assert_eq!(label.key_elements().len(), 3);
}

#[test]
fn renders_mac_modifiers_as_symbols_without_separators() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Macintosh, None));
    label.set(keybinding(OperatingSystem::Macintosh, &[(true, true, true, true, "P")]), None);
    assert_eq!(children(&ui, &label), [key("\u{2303}"), key("\u{21E7}"), key("\u{2325}"), key("\u{2318}"), key("P")]);
}

#[test]
fn windows_and_linux_name_the_meta_key_differently() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let windows = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Windows, None));
    windows.set(keybinding(OperatingSystem::Windows, &[(false, false, false, true, "E")]), None);
    let linux = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    linux.set(keybinding(OperatingSystem::Linux, &[(false, false, false, true, "E")]), None);
    assert_eq!(children(&ui, &windows)[0].1, "Windows");
    assert_eq!(children(&ui, &linux)[0].1, "Super");
}

#[test]
fn two_chords_are_separated_by_a_six_pixel_gap() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    label.set(keybinding(OperatingSystem::Linux, &[(true, false, false, false, "K"), (true, false, false, false, "S")]), None);
    let kinds: Vec<String> = children(&ui, &label).into_iter().map(|(class, _)| class).collect();
    assert_eq!(
        kinds,
        ["monaco-keybinding-key", "monaco-keybinding-key-separator", "monaco-keybinding-key", "monaco-keybinding-key-chord-separator", "monaco-keybinding-key", "monaco-keybinding-key-separator", "monaco-keybinding-key"]
    );
    let tree = ui.tree();
    let chord_separator = tree.children_with_class(label.element(), "monaco-keybinding-key-chord-separator")[0];
    assert_eq!(tree.bounds(chord_separator).width(), 6.0);
}

#[test]
fn a_key_is_the_text_plus_padding_border_and_the_minimum_width() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    label.set(keybinding(OperatingSystem::Linux, &[(false, false, false, false, "P"), (false, false, false, false, "Backspace")]), None);
    let keys = label.key_elements();
    let tree = ui.tree();
    let font = ui_font(11.0, 400);
    // `min-width: 12px` no conteúdo: a letra cabe em 12 px; a palavra longa passa.
    assert_eq!(tree.bounds(keys[0]).width(), 12.0 + 10.0 + 2.0);
    assert_eq!(tree.bounds(keys[1]).width(), measure_text("Backspace", &font) + 10.0 + 2.0);
    // `line-height: 10px` + `padding: 3px` + `border: 1px`.
    assert_eq!(tree.bounds(keys[0]).height(), 18.0);
    assert_eq!(tree.style(keys[0]).radius, 3.0);
}

#[test]
fn first_and_last_keys_have_no_outer_margin_and_the_others_two_pixels() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Macintosh, None));
    label.set(keybinding(OperatingSystem::Macintosh, &[(false, true, false, true, "P")]), None);
    let keys = label.key_elements();
    let tree = ui.tree();
    let (a, b, c) = (tree.bounds(keys[0]), tree.bounds(keys[1]), tree.bounds(keys[2]));
    assert_eq!(a.x0, 0.0, "`:first-child` sem margem à esquerda");
    assert_eq!(b.x0, a.x1 + 4.0, "margem de 2 px de cada lado entre teclas");
    assert_eq!(c.x0, b.x1 + 4.0);
    assert_eq!(tree.bounds(label.element()).width(), c.x1, "`:last-child` sem margem à direita");
    assert_eq!(tree.bounds(label.element()).height(), 18.0);
}

#[test]
fn separators_are_centered_vertically_in_the_row() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    label.set(keybinding(OperatingSystem::Linux, &[(true, false, false, false, "P")]), None);
    let tree = ui.tree();
    let plus = tree.children_with_class(label.element(), "monaco-keybinding-key-separator")[0];
    let r = tree.bounds(plus);
    assert_eq!((r.y0, r.height()), (4.0, 10.0), "`align-items: center` de uma linha de 10 px numa de 18");
    assert_eq!(r.width(), measure_text("+", &ui_font(UI_FONT_SIZE, 400)));
}

#[test]
fn highlights_the_matched_parts_of_each_chord() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    let matches = Matches { first_part: ChordMatches { ctrl_key: true, ..Default::default() }, chord_part: ChordMatches { key_code: true, ..Default::default() } };
    label.set(keybinding(OperatingSystem::Linux, &[(true, false, false, false, "K"), (true, false, false, false, "S")]), Some(matches));
    let tree = ui.tree();
    let highlighted: Vec<String> = label.key_elements().into_iter().filter(|k| tree.has_class(*k, "highlight")).map(|k| text_under(&tree, k)).collect();
    assert_eq!(highlighted, ["Ctrl", "S"]);
}

#[test]
fn unbound_shows_a_key_only_when_asked() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let plain = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    plain.set(None, None);
    assert!(children(&ui, &plain).is_empty());
    assert_eq!(ui.tree().bounds(plain.element()).width(), 0.0);

    let options = KeybindingLabelOptions { render_unbound_keybindings: true, ..unthemed_keybinding_label_options() };
    let unbound = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, Some(options)));
    unbound.set(None, None);
    assert_eq!(children(&ui, &unbound), [key("Unbound")]);
}

#[test]
fn setting_the_same_keybinding_and_matches_does_not_render_again() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    let binding = keybinding(OperatingSystem::Linux, &[(true, false, false, false, "P")]);
    label.set(binding.clone(), None);
    let before = label.key_elements();
    label.set(binding.clone(), None);
    assert_eq!(label.key_elements(), before, "mesmo atalho e mesmos realces: nada é refeito");
    label.set(binding.clone(), Some(Matches::default()));
    assert_ne!(label.key_elements(), before, "realces diferentes refazem");
}

#[test]
fn paints_the_themed_key_colors() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let (bg, fg, border, bottom, shadow) = (Rgba::opaque(1, 1, 1), Rgba::opaque(2, 2, 2), Rgba::opaque(3, 3, 3), Rgba::opaque(4, 4, 4), Rgba::opaque(5, 5, 5));
    let options = KeybindingLabelOptions {
        styles: IKeybindingLabelStyles {
            keybinding_label_background: Some(bg),
            keybinding_label_foreground: Some(fg),
            keybinding_label_border: Some(border),
            keybinding_label_bottom_border: Some(bottom),
            keybinding_label_shadow: Some(shadow),
        },
        ..unthemed_keybinding_label_options()
    };
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, Some(options)));
    label.set(keybinding(OperatingSystem::Linux, &[(false, false, false, false, "P")]), None);
    let tree = ui.tree();
    let key = label.key_elements()[0];
    assert_eq!(tree.style(key).background, Some(bg));
    let parts: Vec<NodeId> = tree.children(key).to_vec();
    // contorno, sombra interna, borda de baixo e texto
    assert_eq!(tree.style(parts[0]).border.map(|b| (b.width, b.color)), Some((1.0, border)));
    assert_eq!(tree.style(parts[1]).background, Some(shadow));
    assert_eq!(tree.style(parts[2]).background, Some(bottom));
    let Content::Text { color, .. } = &tree.style(parts[3]).content else { panic!("sem texto") };
    assert_eq!(*color, fg);
    let bottom_rect = tree.bounds(parts[2]);
    assert_eq!((bottom_rect.y0, bottom_rect.height()), (17.0, 1.0), "a borda de baixo é a última linha da tecla");
    assert_eq!(tree.bounds(parts[1]).y0, 16.0, "a sombra interna fica logo acima");
}

#[test]
fn without_a_border_color_the_keys_use_the_text_color() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let options = KeybindingLabelOptions { foreground: Rgba::opaque(9, 9, 9), ..unthemed_keybinding_label_options() };
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, Some(options)));
    label.set(keybinding(OperatingSystem::Linux, &[(false, false, false, false, "P")]), None);
    let tree = ui.tree();
    let frame = tree.children(label.key_elements()[0])[0];
    assert_eq!(tree.style(frame).border.map(|b| b.color), Some(Rgba::opaque(9, 9, 9)), "`currentColor`");
}

#[test]
fn the_title_is_the_aria_label_unless_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let log = hover_fixture::install();
    let ui = Ui::new();
    let container = create_container(&ui, 400.0, 40.0);
    let label = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, None));
    label.set(keybinding(OperatingSystem::Linux, &[(true, true, false, false, "P")]), None);
    assert_eq!(ui.tree().label(label.element()), Some("Control+Shift+P"));
    assert_eq!(log.events.borrow().last().map(String::as_str), Some("update:Control+Shift+P"));

    let silent = store.add_rc(KeybindingLabel::new(&ui, container, OperatingSystem::Linux, Some(KeybindingLabelOptions { disable_title: true, ..unthemed_keybinding_label_options() })));
    silent.set(keybinding(OperatingSystem::Linux, &[(true, false, false, false, "P")]), None);
    assert_eq!(ui.tree().label(silent.element()), Some(""));
    assert_eq!(log.events.borrow().last().map(String::as_str), Some("update:<none>"));
}
