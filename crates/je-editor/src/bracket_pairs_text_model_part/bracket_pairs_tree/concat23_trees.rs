//! Porte de `.../bracketPairsTree/concat23Trees.ts`: concatena listas de nós `(2,3)` numa única árvore
//! `(2,3)`.

use je_base::errors::{self, Error};

use super::ast::{AstNodeKind, AstNodeRef, ListAstNode};

/// Concatena uma lista de `AstNode`s `(2,3)` num único `AstNode` `(2,3)`. Isto altera os itens do vetor de
/// entrada! Se todos os itens têm a mesma altura, o custo é O(items.len()). Senão, é
/// O(items.len() * max(log(items.len()), items.max(i => i.height))).
pub fn concat23_trees(items: &mut Vec<AstNodeRef>) -> Option<AstNodeRef> {
    if items.is_empty() {
        return None;
    }
    if items.len() == 1 {
        return Some(items[0].clone());
    }

    let mut i = 0;

    // Lê nós de mesma altura e os concatena num único nó.
    let mut read_node = |items: &mut Vec<AstNodeRef>| -> Option<AstNodeRef> {
        if i >= items.len() {
            return None;
        }
        let start = i;
        let height = items[start].list_height();
        i += 1;
        while i < items.len() && items[i].list_height() == height {
            i += 1;
        }

        if i - start >= 2 {
            if start == 0 && i == items.len() {
                concat23_trees_of_same_height(items, false)
            } else {
                concat23_trees_of_same_height(&mut items[start..i].to_vec(), false)
            }
        } else {
            Some(items[start].clone())
        }
    };

    // Os itens podem não ter a mesma altura. Juntamos todos com um operador binário de concatenação.
    let mut first = read_node(items).expect("há um primeiro item");
    let Some(mut second) = read_node(items) else {
        return Some(first);
    };

    while let Some(item) = read_node(items) {
        // Prefira concatenar as árvores menores, já que o custo da concatenação depende da altura da árvore.
        if height_diff(&first, &second) <= height_diff(&second, &item) {
            first = concat(&first, &second);
            second = item;
        } else {
            second = concat(&second, &item);
        }
    }

    Some(concat(&first, &second))
}

/// `concat23TreesOfSameHeight(items, createImmutableLists)`. Altera os itens do vetor.
pub fn concat23_trees_of_same_height(
    items: &mut [AstNodeRef],
    create_immutable_lists: bool,
) -> Option<AstNodeRef> {
    if items.is_empty() {
        return None;
    }
    if items.len() == 1 {
        return Some(items[0].clone());
    }

    let mut length = items.len();
    // Todas as árvores têm a mesma altura, basta criar os nós pai.
    while length > 3 {
        let new_length = length >> 1;
        for i in 0..new_length {
            let j = i << 1;
            items[i] = ListAstNode::create23(
                items[j].clone(),
                items[j + 1].clone(),
                if j + 3 == length {
                    Some(items[j + 2].clone())
                } else {
                    None
                },
                create_immutable_lists,
            );
        }
        length = new_length;
    }

    Some(ListAstNode::create23(
        items[0].clone(),
        items[1].clone(),
        if length >= 3 {
            Some(items[2].clone())
        } else {
            None
        },
        create_immutable_lists,
    ))
}

fn height_diff(node1: &AstNodeRef, node2: &AstNodeRef) -> u32 {
    node1.list_height().abs_diff(node2.list_height())
}

fn concat(node1: &AstNodeRef, node2: &AstNodeRef) -> AstNodeRef {
    if node1.list_height() == node2.list_height() {
        ListAstNode::create23(node1.clone(), node2.clone(), None, false)
    } else if node1.list_height() > node2.list_height() {
        // node1 é a árvore em que queremos inserir
        append(node1, node2)
    } else {
        prepend(node2, node1)
    }
}

fn unexpected() -> ! {
    errors::throw(Error::new("unexpected"))
}

fn as_list(node: &AstNodeRef) -> &ListAstNode {
    node.as_list().unwrap_or_else(|| unexpected())
}

/// Acrescenta o nó dado ao fim desta árvore `(2,3)`. Devolve a nova raiz.
fn append(list: &AstNodeRef, node_to_append: &AstNodeRef) -> AstNodeRef {
    let list = list.to_mutable();
    let mut cur_node: AstNodeRef = list.clone();
    let mut parents: Vec<AstNodeRef> = Vec::new();

    let mut node_to_append_of_correct_height: Option<AstNodeRef>;
    loop {
        // assert nodeToInsert.listHeight <= curNode.listHeight
        if node_to_append.list_height() == cur_node.list_height() {
            node_to_append_of_correct_height = Some(node_to_append.clone());
            break;
        }
        // assert 0 <= nodeToInsert.listHeight < curNode.listHeight
        if cur_node.kind() != AstNodeKind::List {
            unexpected();
        }
        parents.push(cur_node.clone());
        // assert 2 <= curNode.childrenLength <= 3
        let next = as_list(&cur_node)
            .make_last_element_mutable()
            .expect("a lista tem filhos");
        cur_node = next;
    }
    // assert nodeToAppendOfCorrectHeight!.listHeight === curNode.listHeight

    for parent_node in parents.iter().rev() {
        let parent = as_list(parent_node);
        if let Some(node) = node_to_append_of_correct_height.take() {
            // Podemos aceitar o elemento?
            if parent.children_length() >= 3 {
                // assert parent.childrenLength === 3 && parent.listHeight === nodeToAppendOfCorrectHeight.listHeight + 1

                // precisamos dividir para manter a propriedade da árvore (2,3).
                // Manda o terceiro elemento mais o novo elemento para o pai.
                node_to_append_of_correct_height = Some(ListAstNode::create23(
                    parent.unappend_child().expect("há um terceiro filho"),
                    node,
                    None,
                    false,
                ));
            } else {
                parent.append_child_of_same_height(node);
            }
        } else {
            parent.handle_children_changed();
        }
    }

    match node_to_append_of_correct_height {
        Some(node) => ListAstNode::create23(list, node, None, false),
        None => list,
    }
}

/// Acrescenta o nó dado ao início desta árvore `(2,3)`. Devolve a nova raiz.
fn prepend(list: &AstNodeRef, node_to_append: &AstNodeRef) -> AstNodeRef {
    let list = list.to_mutable();
    let mut cur_node: AstNodeRef = list.clone();
    let mut parents: Vec<AstNodeRef> = Vec::new();

    // assert nodeToInsert.listHeight <= curNode.listHeight
    while node_to_append.list_height() != cur_node.list_height() {
        // assert 0 <= nodeToInsert.listHeight < curNode.listHeight
        if cur_node.kind() != AstNodeKind::List {
            unexpected();
        }
        parents.push(cur_node.clone());
        // assert 2 <= curNode.childrenFast.length <= 3
        let next = as_list(&cur_node)
            .make_first_element_mutable()
            .expect("a lista tem filhos");
        cur_node = next;
    }

    let mut node_to_prepend_of_correct_height: Option<AstNodeRef> = Some(node_to_append.clone());
    // assert nodeToAppendOfCorrectHeight!.listHeight === curNode.listHeight

    for parent_node in parents.iter().rev() {
        let parent = as_list(parent_node);
        if let Some(node) = node_to_prepend_of_correct_height.take() {
            // Podemos aceitar o elemento?
            if parent.children_length() >= 3 {
                // assert parent.childrenLength === 3 && parent.listHeight === nodeToAppendOfCorrectHeight.listHeight + 1

                // precisamos dividir para manter a propriedade da árvore (2,3).
                // Manda o terceiro elemento mais o novo elemento para o pai.
                node_to_prepend_of_correct_height = Some(ListAstNode::create23(
                    node,
                    parent.unprepend_child().expect("há um terceiro filho"),
                    None,
                    false,
                ));
            } else {
                parent.prepend_child_of_same_height(node);
            }
        } else {
            parent.handle_children_changed();
        }
    }

    match node_to_prepend_of_correct_height {
        Some(node) => ListAstNode::create23(node, list, None, false),
        None => list,
    }
}
