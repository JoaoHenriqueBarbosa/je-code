//! Os links de um parágrafo como nós focáveis: papel, nome acessível, caixas, ordem de Tab, `Enter` e `Space`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;

use crate::{Content, EventKind, KeyData, Modifiers, NodeId, Paragraph, Role, Span, Ui};

const LINK_COLOR: Rgba = Rgba::opaque(0x37, 0x94, 0xff);
const TEXT_COLOR: Rgba = Rgba::opaque(0xcc, 0xcc, 0xcc);

fn style() -> TextStyle {
    TextStyle::new("sans-serif", 13.0, 400)
}

fn text(value: &str) -> Span {
    Span::text(value, style(), TEXT_COLOR)
}

fn link(value: &str, number: u32) -> Span {
    Span::text(value, style(), LINK_COLOR).with_link(Some(number))
}

/// "Veja a documentação e o guia." com dois links (0 e 1), num parágrafo de 300 px de largura.
fn paragraph_node(ui: &Ui, width: f64) -> NodeId {
    let spans = vec![text("Veja "), link("a documentação", 0), text(" e "), link(" o guia ", 1), text(".")];
    let node = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 400.0, 200.0));
        let node = tree.create(root, Role::Generic);
        tree.set_bounds(node, Rect::new(10.0, 10.0, 10.0 + width, 50.0));
        let mut paragraph = Paragraph::new(spans, 20.0);
        paragraph.focus_color = Some(Rgba::opaque(0x00, 0x7f, 0xd4));
        tree.style_mut(node).content = Content::Paragraph(paragraph);
        node
    };
    ui.sync_paragraph_links(node);
    node
}

fn key(code: KeyCode, shift: bool) -> KeyData {
    KeyData { key_code: code, modifiers: Modifiers { shift, ..Modifiers::default() } }
}

/// Os números dos links ativados por `Click`, na ordem; o ouvinte lê o link do alvo como o `renderMarkdown` faz.
fn record_activations(ui: &Ui, node: NodeId) -> (Rc<RefCell<Vec<u32>>>, je_base::lifecycle::DisposableRef) {
    let activated = Rc::new(RefCell::new(Vec::new()));
    let listener = ui.listen(node, EventKind::Click, false, {
        let (ui, activated) = (ui.clone(), activated.clone());
        move |e| {
            if let Some(index) = ui.tree().link_index(e.target) {
                activated.borrow_mut().push(index);
            }
        }
    });
    (activated, listener)
}

#[test]
fn link_becomes_node_with_role_name_and_no_box() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    let tree = ui.tree();
    let links = tree.paragraph_link_nodes(node);
    assert_eq!(links.iter().map(|(number, _)| *number).collect::<Vec<_>>(), [0, 1], "um nó por link, na ordem do texto");
    assert_eq!(tree.children(node).len(), 2, "nenhum nó por palavra");
    let names: Vec<_> = links.iter().map(|(_, id)| tree.label(*id).map(str::to_owned)).collect();
    assert_eq!(names, [Some("a documentação".to_owned()), Some("o guia".to_owned())], "nome acessível sem espaço nas pontas");
    for (_, id) in &links {
        assert_eq!(tree.role(*id), Role::Link);
        assert_eq!(tree.role(*id).aria_name(), Some("link"));
        let bounds = tree.bounds(*id);
        assert!(bounds.width() == 0.0 && bounds.height() == 0.0, "o link não tem caixa de layout própria");
    }
    assert_eq!(tree.text_content(node), "Veja a documentação e  o guia .", "o textContent não muda");
}

#[test]
fn link_boxes_come_from_paragraph_layout() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    let tree = ui.tree();
    let (_, first) = tree.paragraph_link_nodes(node)[0];
    let boxes = tree.link_boxes(first);
    assert_eq!(boxes.len(), 1, "o link cabe numa linha");
    let origin = tree.absolute_bounds(node);
    assert!(boxes[0].x0 > origin.x0 && boxes[0].width() > 0.0 && boxes[0].height() > 0.0, "caixa dentro do parágrafo, depois de \"Veja \"");
    assert!(boxes[0].y0 >= origin.y0 && boxes[0].y1 <= origin.y0 + 20.0, "a caixa fica na primeira linha");
    let linked_point = ((boxes[0].x0 + boxes[0].x1) / 2.0, (boxes[0].y0 + boxes[0].y1) / 2.0);
    assert_eq!(tree.paragraph_link_at(node, linked_point.0, linked_point.1), Some(0), "o clique continua servido pelo paragraph_link_at");
}

#[test]
fn wrapped_link_has_one_box_per_line() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 90.0);
    let tree = ui.tree();
    let (_, first) = tree.paragraph_link_nodes(node)[0];
    let boxes = tree.link_boxes(first);
    assert!(boxes.len() >= 2, "\"a documentação\" não cabe em 90 px: {boxes:?}");
    assert!(boxes[1].y0 > boxes[0].y0, "as caixas descem uma linha");
}

#[test]
fn tab_walks_links_in_order_and_wraps() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    let links: Vec<NodeId> = ui.tree().paragraph_link_nodes(node).into_iter().map(|(_, id)| id).collect();
    assert_eq!(ui.tree().tab_order(), links, "a ordem de Tab é a ordem dos links no texto");
    assert_eq!(ui.focused(), None);

    assert!(!ui.key_input(true, key(KeyCode::Tab, false)), "o Tab moveu o foco: tecla consumida");
    assert_eq!(ui.focused(), Some(links[0]));
    assert!(!ui.key_input(true, key(KeyCode::Tab, false)));
    assert_eq!(ui.focused(), Some(links[1]));
    ui.key_input(true, key(KeyCode::Tab, false));
    assert_eq!(ui.focused(), Some(links[0]), "no fim da ordem volta ao primeiro");
    ui.key_input(true, key(KeyCode::Tab, true));
    assert_eq!(ui.focused(), Some(links[1]), "Shift+Tab volta um");
    ui.key_input(true, key(KeyCode::Tab, true));
    assert_eq!(ui.focused(), Some(links[0]));
}

#[test]
fn tab_with_prevent_default_keeps_focus() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    let _guard = ui.listen(ui.root(), EventKind::KeyDown, false, |e| e.prevent_default());
    assert!(!ui.key_input(true, key(KeyCode::Tab, false)));
    assert_eq!(ui.focused(), None, "o ouvinte consumiu o Tab");
    assert!(!ui.tree().paragraph_link_nodes(node).is_empty());
}

#[test]
fn tab_skips_hidden_link_and_negative_tabindex() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    let links: Vec<NodeId> = ui.tree().paragraph_link_nodes(node).into_iter().map(|(_, id)| id).collect();
    ui.tree_mut().set_attribute(links[0], "tabindex", "-1");
    assert_eq!(ui.tree().tab_order(), vec![links[1]], "tabindex negativo sai da ordem");
    ui.tree_mut().style_mut(node).visible = false;
    assert!(ui.tree().tab_order().is_empty(), "display: none tira a subárvore da ordem");
    assert!(!ui.focus_by_tab(false), "sem nó focável o Tab não faz nada");
}

#[test]
fn enter_activates_focused_link_and_space_does_not() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    let (activated, _listener) = record_activations(&ui, node);
    let links: Vec<NodeId> = ui.tree().paragraph_link_nodes(node).into_iter().map(|(_, id)| id).collect();

    ui.focus(links[1]);
    assert!(!ui.key_input(true, key(KeyCode::Enter, false)), "o Enter ativou o link: tecla consumida");
    assert_eq!(*activated.borrow(), [1], "o Click chega ao parágrafo com o link por alvo");

    assert!(ui.key_input(true, key(KeyCode::Space, false)), "o Space não é consumido por um link");
    assert!(ui.key_input(false, key(KeyCode::Enter, false)), "o keyup não ativa");
    assert_eq!(*activated.borrow(), [1], "nem Space nem keyup ativam");

    ui.focus(links[0]);
    ui.key_input(true, key(KeyCode::Enter, false));
    assert_eq!(*activated.borrow(), [1, 0]);
}

#[test]
fn enter_prevented_or_unfocused_does_not_activate() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    let (activated, _listener) = record_activations(&ui, node);
    let links: Vec<NodeId> = ui.tree().paragraph_link_nodes(node).into_iter().map(|(_, id)| id).collect();

    ui.key_input(true, key(KeyCode::Enter, false));
    assert!(activated.borrow().is_empty(), "sem foco num link o Enter não ativa nada");

    ui.focus(links[0]);
    let _guard = ui.listen(links[0], EventKind::KeyDown, false, |e| e.prevent_default());
    assert!(!ui.key_input(true, key(KeyCode::Enter, false)));
    assert!(activated.borrow().is_empty(), "o ouvinte do keydown cancelou a ação padrão");
}

#[test]
fn sync_keeps_identity_and_removes_vanished_link() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    let before: Vec<(u32, NodeId)> = ui.tree().paragraph_link_nodes(node);
    ui.focus(before[0].1);

    // O texto do primeiro link muda; o segundo link some.
    {
        let mut tree = ui.tree_mut();
        let Content::Paragraph(paragraph) = &mut tree.style_mut(node).content else { panic!("o nó é um parágrafo") };
        paragraph.spans = vec![text("Leia "), link("o manual", 0), text(".")];
    }
    ui.sync_paragraph_links(node);

    let after = ui.tree().paragraph_link_nodes(node);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0], before[0], "o link que continua mantém o nó");
    assert_eq!(ui.tree().label(after[0].1), Some("o manual"));
    assert_eq!(ui.focused(), Some(before[0].1), "e mantém o foco");
    assert!(!ui.tree().is_alive(before[1].1), "o link que sumiu foi removido");

    ui.focus(after[0].1);
    ui.remove(after[0].1);
    assert_eq!(ui.focused(), None, "remover o link com foco limpa o foco");
}

#[test]
fn new_link_in_the_middle_follows_text_order() {
    let ui = Ui::new();
    let node = paragraph_node(&ui, 300.0);
    {
        let mut tree = ui.tree_mut();
        let Content::Paragraph(paragraph) = &mut tree.style_mut(node).content else { panic!("o nó é um parágrafo") };
        paragraph.spans = vec![link("um", 0), text(" "), link("dois", 5), text(" "), link("tres", 1)];
    }
    ui.sync_paragraph_links(node);
    let tree = ui.tree();
    let order: Vec<_> = tree.tab_order().into_iter().map(|id| tree.link_index(id)).collect();
    assert_eq!(order, [Some(0), Some(5), Some(1)], "a ordem de Tab segue o texto, não o número do link");
}
