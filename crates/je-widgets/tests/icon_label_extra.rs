//! Testes extras do `IconLabel` e do `SimpleIconLabel` (o original só testa o estado de estilo, em
//! `icon_label.rs`): estrutura de nós e classes, medidas do fluxo, estados desenhados, vários nomes com
//! `matches`, ícone, corte com reticências e hover.

mod hover_fixture;
mod support;

use je_base::filters::Match;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_ui::{Content, NodeId, Tree, Ui};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabel, IconLabelStyles, IconLabelText, IconLabelTitle, IconPath};
use je_widgets::simple_icon_label::SimpleIconLabel;
use je_widgets::text::{measure_text, ui_font};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, query_all};

const TEXT: Rgba = Rgba::opaque(0xcc, 0xcc, 0xcc);
const DISABLED: Rgba = Rgba::opaque(0x55, 0x55, 0x55);
const HIGHLIGHT: Rgba = Rgba::opaque(0x2a, 0xaa, 0xff);

fn text_styles() -> HighlightedLabelStyles {
    HighlightedLabelStyles { foreground: TEXT, highlight_foreground: Some(HIGHLIGHT), highlight_weight: 400, font_size: 13.0, font_weight: 400, line_height: Some(22.0), italic: false, strikethrough: false }
}

fn styles() -> IconLabelStyles {
    IconLabelStyles { text: text_styles(), disabled_foreground: Some(DISABLED) }
}

fn make(ui: &Ui, store: &support::LeakGuard, highlights: bool) -> (NodeId, std::rc::Rc<IconLabel>) {
    let container = create_container(ui, 500.0, 40.0);
    let options = IIconLabelCreationOptions { support_highlights: highlights, support_description_highlights: highlights, ..Default::default() };
    (container, store.add_rc(IconLabel::new(ui, container, Some(options), styles())))
}

fn find(ui: &Ui, container: NodeId, class: &str) -> NodeId {
    query_all(&ui.tree(), container, &[class])[0]
}

/// O texto dos nós de texto sob `node`, na ordem do DOM.
fn text(tree: &Tree, node: NodeId) -> String {
    tree.subtree(node)
        .into_iter()
        .filter_map(|n| match &tree.style(n).content {
            Content::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Os pesos de fonte dos nós de texto sob `node`.
fn weights(tree: &Tree, node: NodeId) -> Vec<u16> {
    tree.subtree(node)
        .into_iter()
        .filter_map(|n| match &tree.style(n).content {
            Content::Text { style, .. } => Some(style.weight),
            _ => None,
        })
        .collect()
}

#[test]
fn builds_the_structure_of_the_original() {
    for highlights in [false, true] {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let ui = Ui::new();
        let (container, label) = make(&ui, &store, highlights);
        label.set_label("filename".into(), Some("description"), Some(&IIconLabelValueOptions { suffix: Some(".ts".to_owned()), ..Default::default() }));
        let tree = ui.tree();
        let root = label.element();
        assert!(tree.has_class(root, "monaco-icon-label"));
        assert_eq!(tree.children(container), [root]);
        let label_container = tree.children_with_class(root, "monaco-icon-label-container")[0];
        let order: Vec<&str> = tree.children(label_container).iter().map(|c| tree.classes(*c)[0].as_str()).collect();
        assert_eq!(order, ["monaco-icon-name-container", "monaco-icon-suffix-container", "monaco-icon-description-container"], "o sufixo vem logo depois do nome");
        assert_eq!(text(&tree, find(&ui, container, "label-name")), "filename");
        assert_eq!(text(&tree, find(&ui, container, "label-suffix")), ".ts");
        assert_eq!(text(&tree, find(&ui, container, "label-description")), "description");
    }
}

#[test]
fn the_state_options_become_classes_and_the_title_the_aria_label() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (_, label) = make(&ui, &store, false);
    let options = IIconLabelValueOptions {
        extra_classes: vec!["file-icon".to_owned(), "ts-name-file-icon".to_owned()],
        bold: true,
        italic: true,
        strikethrough: true,
        disabled_command: true,
        title: Some(IconLabelTitle::Text("a dica".to_owned())),
        ..Default::default()
    };
    label.set_label("x".into(), None, Some(&options));
    let tree = ui.tree();
    assert_eq!(tree.classes(label.element()), ["monaco-icon-label", "file-icon", "ts-name-file-icon", "bold", "italic", "strikethrough"]);
    assert_eq!(tree.label(label.element()), Some("a dica"));
    let container = tree.children_with_class(label.element(), "monaco-icon-label-container")[0];
    assert!(tree.has_class(container, "disabled"));
    drop(tree);
    label.set_label("x".into(), None, None);
    let tree = ui.tree();
    assert_eq!(tree.classes(label.element()), ["monaco-icon-label"], "um `setLabel` sem opções limpa o estado");
    assert_eq!(tree.label(label.element()), Some(""));
}

#[test]
fn bold_is_drawn_in_weight_700_and_the_others_in_400() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (_, label) = make(&ui, &store, true);
    label.set_label("filename".into(), Some("description"), Some(&IIconLabelValueOptions { bold: true, suffix: Some(".ts".to_owned()), ..Default::default() }));
    assert!(weights(&ui.tree(), label.element()).iter().all(|w| *w == 700));
    label.set_label("filename".into(), Some("description"), Some(&IIconLabelValueOptions { suffix: Some(".ts".to_owned()), ..Default::default() }));
    assert!(weights(&ui.tree(), label.element()).iter().all(|w| *w == 400));
}

#[test]
fn strikethrough_draws_a_one_pixel_line_over_each_text() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, false);
    label.set_label("filename".into(), Some("description"), Some(&IIconLabelValueOptions { strikethrough: true, ..Default::default() }));
    let tree = ui.tree();
    for class in ["label-name", "label-description"] {
        let holder = find(&ui, container, class);
        let run = tree.children(holder)[0];
        let line = tree.children(run)[0];
        assert_eq!(tree.bounds(line).height(), 1.0);
        assert_eq!(tree.bounds(line).width(), tree.bounds(run).width());
        assert_eq!(tree.style(line).background, Some(TEXT));
    }
    drop(tree);
    label.set_label("filename".into(), Some("description"), None);
    let tree = ui.tree();
    let run = tree.children(find(&ui, container, "label-name"))[0];
    assert!(tree.children(run).is_empty());
}

#[test]
fn the_description_follows_the_name_with_half_an_em_of_margin_and_is_smaller() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, false);
    label.set_label("filename".into(), Some("description"), None);
    let tree = ui.tree();
    let name = measure_text("filename", &ui_font(13.0, 400));
    let small = ui_font(13.0 * 0.9, 400);
    let description = find(&ui, container, "label-description");
    let description_container = tree.parent(description).expect("contêiner");
    assert_eq!(tree.bounds(tree.parent(find(&ui, container, "label-name")).expect("contêiner do nome")).width(), name);
    assert_eq!(tree.bounds(description_container).x0, name);
    let margin = f64::from(13.0_f32 * 0.9) * 0.5;
    assert_eq!(tree.bounds(description).x0, margin);
    assert_eq!(tree.bounds(description).width(), measure_text("description", &small));
    assert_eq!(tree.style(description).opacity, 0.7);
    assert_eq!(label.content_width(), name + margin + measure_text("description", &small));
}

#[test]
fn an_empty_description_has_no_margin_and_the_suffix_sits_between() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, false);
    label.set_label("name".into(), Some("d"), Some(&IIconLabelValueOptions { suffix: Some(".x".to_owned()), ..Default::default() }));
    label.set_label("name".into(), None, Some(&IIconLabelValueOptions { suffix: Some(".x".to_owned()), ..Default::default() }));
    let tree = ui.tree();
    assert_eq!(tree.bounds(find(&ui, container, "label-description")).x0, 0.0, "o `FastLabelNode.empty` zera a margem");
    let (name, suffix) = (measure_text("name", &ui_font(13.0, 400)), measure_text(".x", &ui_font(13.0, 400)));
    assert_eq!(label.content_width(), name + suffix);
    assert_eq!(tree.style(find(&ui, container, "label-suffix")).opacity, 0.7);
}

#[test]
fn the_name_highlights_use_the_matches() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, true);
    label.set_label("filename".into(), Some("src/file"), Some(&IIconLabelValueOptions { matches: Some(vec![Match { start: 0, end: 4 }]), description_matches: Some(vec![Match { start: 4, end: 8 }]), ..Default::default() }));
    let tree = ui.tree();
    let highlighted = |class: &str| -> Vec<String> { query_all(&tree, find(&ui, container, class), &["highlight"]).into_iter().map(|n| text(&tree, n)).collect() };
    assert_eq!(highlighted("label-name"), ["file"]);
    assert_eq!(highlighted("label-description"), ["file"]);
}

#[test]
fn without_highlight_support_the_matches_are_ignored() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, false);
    label.set_label("filename".into(), None, Some(&IIconLabelValueOptions { matches: Some(vec![Match { start: 0, end: 4 }]), ..Default::default() }));
    assert!(query_all(&ui.tree(), container, &["highlight"]).is_empty());
}

#[test]
fn supports_icons_in_the_name_and_in_the_description() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 500.0, 40.0);
    let options = IIconLabelCreationOptions { support_icons: true, support_description_highlights: true, ..Default::default() };
    let label = store.add_rc(IconLabel::new(&ui, container, Some(options), styles()));
    label.set_label("$(sync) Sincronizar".into(), Some("$(check) ok"), None);
    let tree = ui.tree();
    let icons: Vec<String> = tree
        .subtree(container)
        .into_iter()
        .filter_map(|n| match &tree.style(n).content {
            Content::Codicon { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(icons, ["sync", "check"]);
    // O `supportIcons` do valor vence o da criação.
    drop(tree);
    label.set_label("$(sync) x".into(), None, Some(&IIconLabelValueOptions { support_icons: Some(false), ..Default::default() }));
    assert_eq!(text(&ui.tree(), find(&ui, container, "label-name")), "$(sync) x");
}

#[test]
fn several_names_are_separated_and_the_matches_are_split_between_them() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, true);
    let names = IconLabelText::from(vec!["src".to_owned(), "base".to_owned(), "ui".to_owned()]);
    label.set_label(names, None, Some(&IIconLabelValueOptions { matches: Some(vec![Match { start: 2, end: 6 }]), ..Default::default() }));
    let tree = ui.tree();
    let name_container = find(&ui, container, "monaco-icon-name-container");
    assert!(tree.has_class(name_container, "multiple"));
    let anchors = tree.children_with_class(name_container, "label-name");
    assert_eq!(anchors.len(), 3);
    assert_eq!(tree.role(anchors[0]), je_ui::Role::TreeItem);
    let separators: Vec<String> = query_all(&tree, name_container, &["label-separator"]).into_iter().map(|n| text(&tree, n)).collect();
    assert_eq!(separators, ["/", "/"]);
    let highlighted = |anchor: NodeId| -> Vec<String> { query_all(&tree, anchor, &["highlight"]).into_iter().map(|n| text(&tree, n)).collect() };
    // `[2, 6)` do texto inteiro `src/base/ui`: "c" no primeiro nome e "ba" no segundo.
    assert_eq!((highlighted(anchors[0]), highlighted(anchors[1]), highlighted(anchors[2])), (vec!["c".to_owned()], vec!["ba".to_owned()], vec![]));
    for separator in query_all(&tree, name_container, &["label-separator"]) {
        assert_eq!(tree.style(separator).opacity, 0.5);
    }
}

#[test]
fn a_custom_separator_replaces_the_slash() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, false);
    label.set_label(vec!["a".to_owned(), "b".to_owned()].into(), None, Some(&IIconLabelValueOptions { separator: Some(" > ".to_owned()), ..Default::default() }));
    let tree = ui.tree();
    let separators: Vec<String> = query_all(&tree, container, &["label-separator"]).into_iter().map(|n| text(&tree, n)).collect();
    assert_eq!(separators, [" > "]);
    let name_container = find(&ui, container, "monaco-icon-name-container");
    let (a, sep, b) = (measure_text("a", &ui_font(13.0, 400)), measure_text(" > ", &ui_font(13.0, 400)), measure_text("b", &ui_font(13.0, 400)));
    assert_eq!(tree.bounds(name_container).width(), a + sep + 4.0 + b, "`margin: 0 2px` no separador");
}

#[test]
fn going_back_to_a_single_name_drops_the_multiple_layout() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, true);
    label.set_label(vec!["a".to_owned(), "b".to_owned()].into(), None, None);
    label.set_label("single".into(), None, None);
    let tree = ui.tree();
    let name_container = find(&ui, container, "monaco-icon-name-container");
    assert!(!tree.has_class(name_container, "multiple"));
    assert_eq!(tree.children_with_class(name_container, "label-name").len(), 1);
    assert_eq!(text(&tree, name_container), "single");
}

#[test]
fn the_icon_path_is_prepended_and_removed() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (_, label) = make(&ui, &store, false);
    let with_icon = IIconLabelValueOptions { icon_path: Some(IconPath::ThemeIcon(ThemeIcon::from_id("file"))), ..Default::default() };
    label.set_label("a".into(), None, Some(&with_icon));
    let tree = ui.tree();
    let first = tree.children(label.element())[0];
    assert_eq!(tree.classes(first), ["monaco-icon-label-iconpath", "codicon", "codicon-file"]);
    assert!(matches!(&tree.style(first).content, Content::Codicon { id, .. } if id == "file"));
    let r = tree.bounds(first);
    assert_eq!((r.x0, r.width(), r.height()), (0.0, 16.0, 22.0));
    let container = tree.children_with_class(label.element(), "monaco-icon-label-container")[0];
    assert_eq!(tree.bounds(container).x0, 22.0, "16 px do ícone e 6 de margem");
    drop(tree);

    label.set_label("a".into(), None, Some(&with_icon));
    assert_eq!(ui.tree().children_with_class(label.element(), "monaco-icon-label-iconpath").len(), 1, "o nó do ícone é reusado");
    label.set_label("a".into(), None, None);
    assert!(ui.tree().children_with_class(label.element(), "monaco-icon-label-iconpath").is_empty());
}

#[test]
fn a_disabled_command_uses_the_disabled_foreground() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, true);
    label.set_label("name".into(), Some("d"), Some(&IIconLabelValueOptions { disabled_command: true, ..Default::default() }));
    let color_of = |class: &str| -> Option<Rgba> {
        let tree = ui.tree();
        tree.subtree(find(&ui, container, class)).into_iter().find_map(|n| match &tree.style(n).content {
            Content::Text { color, .. } => Some(*color),
            _ => None,
        })
    };
    assert_eq!(color_of("label-name"), Some(DISABLED));
    assert_eq!(color_of("label-description"), Some(DISABLED));
    label.set_label("name".into(), Some("d"), None);
    assert_eq!(color_of("label-name"), Some(TEXT));
}

#[test]
fn layout_cuts_the_description_first_with_an_ellipsis() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, false);
    label.set_label("filename".into(), Some("a very long description here"), None);
    let name = measure_text("filename", &ui_font(13.0, 400));
    label.layout(Some(name + 60.0));
    let tree = ui.tree();
    assert_eq!(text(&tree, find(&ui, container, "label-name")), "filename");
    let description = text(&tree, find(&ui, container, "label-description"));
    assert!(description.ends_with('\u{2026}') && description.len() < "a very long description here".len(), "{description}");
    assert_eq!(tree.bounds(label.element()).width(), name + 60.0);
    let description_node = find(&ui, container, "label-description");
    assert!(tree.bounds(tree.parent(description_node).expect("contêiner")).x1 <= name + 60.0 + 0.01);
    drop(tree);
    label.layout(None);
    assert_eq!(text(&ui.tree(), find(&ui, container, "label-description")), "a very long description here");
}

#[test]
fn layout_cuts_the_name_when_it_alone_does_not_fit_and_hides_the_rest() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let (container, label) = make(&ui, &store, true);
    label.set_label("a-very-long-file-name".into(), Some("description"), None);
    label.layout(Some(50.0));
    let tree = ui.tree();
    let name = text(&tree, find(&ui, container, "label-name"));
    assert!(name.ends_with('\u{2026}'), "{name}");
    let run = tree.subtree(find(&ui, container, "label-description")).into_iter().find(|n| matches!(tree.style(*n).content, Content::Text { .. })).expect("texto da descrição");
    assert!(!tree.style(run).visible);
}

#[test]
fn the_title_sets_up_a_hover_on_the_element_or_on_the_label_container() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let log = hover_fixture::install();
    let ui = Ui::new();
    let (_, label) = make(&ui, &store, false);
    label.set_label("a".into(), None, Some(&IIconLabelValueOptions { title: Some("dica".into()), ..Default::default() }));
    assert_eq!(*log.events.borrow(), [format!("managed:{}:dica", hover_fixture::node_key(label.element()))]);

    log.events.borrow_mut().clear();
    label.set_label("a".into(), Some("d"), Some(&IIconLabelValueOptions { title: Some("dica".into()), description_title: Some("sobre d".into()), ..Default::default() }));
    let container = ui.tree().children_with_class(label.element(), "monaco-icon-label-container")[0];
    let events = log.events.borrow().clone();
    assert!(events.contains(&format!("managed:{}:dica", hover_fixture::node_key(container))), "{events:?}");
    assert!(events.iter().any(|e| e.ends_with(":sobre d")), "{events:?}");
    // Como no original, o hover guardado para o elemento só é trocado quando o `setupHover` volta a ele.
    assert!(!events.contains(&"dispose".to_owned()), "{events:?}");
}

#[test]
fn a_hover_target_override_must_be_an_ancestor() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    hover_fixture::install();
    let ui = Ui::new();
    let container = create_container(&ui, 500.0, 40.0);
    let outside = create_container(&ui, 10.0, 10.0);
    let options = IIconLabelCreationOptions { hover_target_override: Some(outside), ..Default::default() };
    let label = store.add_rc(IconLabel::new(&ui, container, Some(options), styles()));
    support::assert_throws(|| label.set_label("a".into(), None, Some(&IIconLabelValueOptions { title: Some("dica".into()), ..Default::default() })));

    let ok = IIconLabelCreationOptions { hover_target_override: Some(container), ..Default::default() };
    let label = store.add_rc(IconLabel::new(&ui, container, Some(ok), styles()));
    label.set_label("a".into(), None, Some(&IIconLabelValueOptions { title: Some("dica".into()), ..Default::default() }));
}

#[test]
fn dispose_disposes_the_hovers() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let log = hover_fixture::install();
    let ui = Ui::new();
    let container = create_container(&ui, 500.0, 40.0);
    let label = IconLabel::new(&ui, container, None, styles());
    label.set_label("a".into(), None, Some(&IIconLabelValueOptions { title: Some("dica".into()), ..Default::default() }));
    je_base::lifecycle::IDisposable::dispose(&*label);
    assert_eq!(log.events.borrow().last().map(String::as_str), Some("dispose"));
}

#[test]
fn simple_icon_label_renders_text_with_icons_and_a_hover_title() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let log = hover_fixture::install();
    let ui = Ui::new();
    let container = create_container(&ui, 300.0, 22.0);
    let label = store.add_rc(std::rc::Rc::new(SimpleIconLabel::new(&ui, container, text_styles())));
    label.set_text("$(alert) Unresponsive");
    {
        let tree = ui.tree();
        let children = tree.children(container).to_vec();
        assert_eq!(children.len(), 2);
        assert!(tree.has_class(children[0], "codicon-alert"));
        assert_eq!(text(&tree, container), " Unresponsive");
        assert_eq!(tree.bounds(children[1]).x0, 16.0);
    }
    label.set_text("plain");
    assert_eq!(ui.tree().children(container).len(), 1, "o contêiner é refeito (`reset`)");

    label.set_title("");
    assert!(log.events.borrow().is_empty(), "título vazio não cria hover");
    label.set_title("dica");
    label.set_title("nova");
    assert_eq!(log.events.borrow().len(), 2);
    assert_eq!(log.events.borrow()[1], "update:nova");
}
