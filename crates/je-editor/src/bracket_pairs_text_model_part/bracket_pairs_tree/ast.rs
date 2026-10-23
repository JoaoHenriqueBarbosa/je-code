//! Porte de `.../bracketPairsTree/ast.ts`: os nós da árvore de colchetes (`TextAstNode`,
//! `BracketAstNode`, `PairAstNode`, `InvalidBracketAstNode` e as listas `(2,3)` do `ListAstNode`).
//!
//! Decisões de tradução:
//! - O `AstNode` do original é a união das cinco classes; aqui é um `enum` dentro de um `Rc`
//!   ([`AstNodeRef`]). A identidade de objeto que o original usa (`lastChild !== mutable`) é `Rc::ptr_eq`, e
//!   os nós folha e os colchetes são compartilhados entre árvores, como lá.
//! - A mutabilidade de lista é a do original: uma lista nasce imutável (`immutable = true`) ou mutável, e
//!   `toMutable` devolve uma cópia rasa mutável de uma lista imutável, ou a própria lista se ela já é
//!   mutável. Como no original, listas mutáveis são alteradas no lugar (por `concat23Trees`), inclusive as
//!   que a árvore anterior compartilha. O estado mutável fica num `RefCell`; mutar uma lista imutável é
//!   um erro, como o `throwIfImmutable`.
//! - As duas formas de armazenamento de filhos do original (`TwoThreeListAstNode`, a que vale, e
//!   `ArrayListAstNode`, usada em depuração e por `ListAstNode.create`) são as variantes de [`ListChildren`].
//! - `Number.MAX_SAFE_INTEGER` (a indentação mínima de um nó sem texto) é [`MAX_SAFE_INTEGER`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};

use super::length::{LENGTH_ZERO, Length, length_add, length_get_line_count, length_to_obj};
use super::small_immutable_set::SmallImmutableSet;
use crate::cursor_columns::visible_column_from_column;
use crate::language_brackets_configuration::BracketKind;
use crate::text_model::TextModel;

/// `Number.MAX_SAFE_INTEGER`.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// `OpeningBracketId`.
pub type OpeningBracketId = usize;

/// `AstNodeKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AstNodeKind {
    Text = 0,
    Bracket = 1,
    Pair = 2,
    UnexpectedClosingBracket = 3,
    List = 4,
}

/// Um nó compartilhado da árvore.
pub type AstNodeRef = Rc<AstNode>;

/// `AstNode`.
pub enum AstNode {
    Text(TextAstNode),
    Bracket(BracketAstNode),
    Pair(PairAstNode),
    UnexpectedClosingBracket(InvalidBracketAstNode),
    List(ListAstNode),
}

fn throw_error(message: &str) -> ! {
    errors::throw(Error::new(message))
}

impl AstNode {
    pub fn kind(&self) -> AstNodeKind {
        match self {
            AstNode::Text(_) => AstNodeKind::Text,
            AstNode::Bracket(_) => AstNodeKind::Bracket,
            AstNode::Pair(_) => AstNodeKind::Pair,
            AstNode::UnexpectedClosingBracket(_) => AstNodeKind::UnexpectedClosingBracket,
            AstNode::List(_) => AstNodeKind::List,
        }
    }

    /// O comprimento do nó inteiro, que deve ser a soma dos comprimentos dos filhos.
    pub fn length(&self) -> Length {
        match self {
            AstNode::Text(n) => n.length,
            AstNode::Bracket(n) => n.length,
            AstNode::Pair(n) => n.length,
            AstNode::UnexpectedClosingBracket(n) => n.length,
            AstNode::List(n) => n.length(),
        }
    }

    pub fn children_length(&self) -> usize {
        match self {
            AstNode::Text(_) | AstNode::Bracket(_) | AstNode::UnexpectedClosingBracket(_) => 0,
            AstNode::Pair(_) => 3,
            AstNode::List(n) => n.children_length(),
        }
    }

    /// Pode devolver `None` mesmo com `idx` menor que [`AstNode::children_length`].
    pub fn get_child(&self, idx: usize) -> Option<AstNodeRef> {
        match self {
            AstNode::Text(_) | AstNode::Bracket(_) | AstNode::UnexpectedClosingBracket(_) => None,
            AstNode::Pair(n) => match idx {
                0 => Some(n.opening_bracket.clone()),
                1 => n.child.clone(),
                2 => n.closing_bracket.clone(),
                _ => throw_error("Invalid child index"),
            },
            AstNode::List(n) => n.get_child(idx),
        }
    }

    /// Evite usar, a implementação pode alocar o vetor.
    pub fn children(&self) -> Vec<AstNodeRef> {
        match self {
            AstNode::Text(_) | AstNode::Bracket(_) | AstNode::UnexpectedClosingBracket(_) => {
                Vec::new()
            }
            AstNode::Pair(n) => {
                let mut result = vec![n.opening_bracket.clone()];
                if let Some(child) = &n.child {
                    result.push(child.clone());
                }
                if let Some(closing) = &n.closing_bracket {
                    result.push(closing.clone());
                }
                result
            }
            AstNode::List(n) => n.children(),
        }
    }

    /// O conjunto de todos os ids de abertura (potencialmente) faltando neste nó. Em `{ ] ) }` é {`[`, `(`}.
    pub fn missing_opening_bracket_ids(&self) -> SmallImmutableSet {
        match self {
            AstNode::Text(_) | AstNode::Bracket(_) => SmallImmutableSet::get_empty(),
            AstNode::Pair(n) => n.missing_opening_bracket_ids.clone(),
            AstNode::UnexpectedClosingBracket(n) => n.missing_opening_bracket_ids.clone(),
            AstNode::List(n) => n.missing_opening_bracket_ids(),
        }
    }

    /// Numa lista, a altura da árvore `(2,3)`.
    pub fn list_height(&self) -> u32 {
        match self {
            AstNode::List(n) => n.list_height,
            _ => 0,
        }
    }

    /// `openBracketIds` é o conjunto das aberturas ainda não fechadas.
    pub fn can_be_reused(&self, open_bracket_ids: &SmallImmutableSet) -> bool {
        match self {
            AstNode::Text(_) => true,
            // Estes nós poderiam ser reaproveitados, mas não de forma geral: o pai deles pode.
            AstNode::Bracket(_) => false,
            AstNode::Pair(n) => {
                if n.closing_bracket.is_none() {
                    // Um par sem fechamento termina no fim do documento ou quando um nó pai é fechado.
                    // Dá para melhorar: só retornar falso se algum token seguinte não for `undefined` nem um
                    // colchete que fecha um pai.
                    return false;
                }
                !open_bracket_ids.intersects(&n.missing_opening_bracket_ids)
            }
            AstNode::UnexpectedClosingBracket(n) => {
                !open_bracket_ids.intersects(&n.missing_opening_bracket_ids)
            }
            AstNode::List(n) => n.can_be_reused(open_bracket_ids),
        }
    }

    /// Achata todas as listas desta árvore. Só para depuração.
    pub fn flatten_lists(self: &Rc<Self>) -> AstNodeRef {
        match &**self {
            AstNode::Text(_) | AstNode::Bracket(_) | AstNode::UnexpectedClosingBracket(_) => {
                self.clone()
            }
            AstNode::Pair(n) => PairAstNode::create(
                n.opening_bracket.flatten_lists(),
                n.child.as_ref().map(|c| c.flatten_lists()),
                n.closing_bracket.as_ref().map(|c| c.flatten_lists()),
            ),
            AstNode::List(n) => {
                let mut items: Vec<AstNodeRef> = Vec::new();
                for c in n.children() {
                    let normalized = c.flatten_lists();
                    if normalized.kind() == AstNodeKind::List {
                        items.extend(normalized.children());
                    } else {
                        items.push(normalized);
                    }
                }
                ListAstNode::create(items, false)
            }
        }
    }

    /// Cria uma cópia profunda.
    pub fn deep_clone(self: &Rc<Self>) -> AstNodeRef {
        match &**self {
            AstNode::Text(_) | AstNode::Bracket(_) | AstNode::UnexpectedClosingBracket(_) => {
                self.clone()
            }
            AstNode::Pair(n) => Rc::new(AstNode::Pair(PairAstNode {
                length: n.length,
                opening_bracket: n.opening_bracket.deep_clone(),
                child: n.child.as_ref().map(|c| c.deep_clone()),
                closing_bracket: n.closing_bracket.as_ref().map(|c| c.deep_clone()),
                missing_opening_bracket_ids: n.missing_opening_bracket_ids.clone(),
            })),
            AstNode::List(n) => n.deep_clone(),
        }
    }

    /// A indentação mínima (em colunas visíveis) do texto contido no nó.
    pub fn compute_min_indentation(&self, offset: Length, text_model: &TextModel) -> i64 {
        match self {
            AstNode::Text(n) => n.compute_min_indentation(offset, text_model),
            AstNode::Bracket(_) | AstNode::UnexpectedClosingBracket(_) => MAX_SAFE_INTEGER,
            AstNode::Pair(n) => match &n.child {
                Some(child) => child.compute_min_indentation(
                    length_add(offset, n.opening_bracket.length()),
                    text_model,
                ),
                None => MAX_SAFE_INTEGER,
            },
            AstNode::List(n) => n.compute_min_indentation(offset, text_model),
        }
    }

    /// O nó como lista, se for uma.
    pub fn as_list(&self) -> Option<&ListAstNode> {
        match self {
            AstNode::List(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_pair(&self) -> Option<&PairAstNode> {
        match self {
            AstNode::Pair(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_bracket(&self) -> Option<&BracketAstNode> {
        match self {
            AstNode::Bracket(n) => Some(n),
            _ => None,
        }
    }

    /// `toMutable()` de uma lista: uma cópia rasa mutável, ou a própria lista se já é mutável. Os demais nós
    /// não são listas.
    pub fn to_mutable(self: &Rc<Self>) -> AstNodeRef {
        match &**self {
            AstNode::List(n) => {
                if n.immutable {
                    Rc::new(AstNode::List(n.shallow_mutable_copy()))
                } else {
                    self.clone()
                }
            }
            _ => throw_error("unexpected"),
        }
    }
}

/// `TextAstNode`.
pub struct TextAstNode {
    pub length: Length,
}

impl TextAstNode {
    /// `new TextAstNode(length)`.
    pub fn create(length: Length) -> AstNodeRef {
        Rc::new(AstNode::Text(TextAstNode { length }))
    }

    fn compute_min_indentation(&self, offset: Length, text_model: &TextModel) -> i64 {
        let start = length_to_obj(offset);

        // Os nós de texto não têm indentação parcial (o tokenizador garante). Logo, se este nó não começa na
        // coluna 0, a primeira linha não tem indentação nenhuma.
        let start_line_number = (if start.column_count == 0 {
            start.line_count
        } else {
            start.line_count + 1
        }) + 1;
        let end_line_number = length_get_line_count(length_add(offset, self.length)) + 1;

        let mut result = MAX_SAFE_INTEGER;

        let tab_size = text_model.get_options().tab_size;
        for line_number in start_line_number..=end_line_number {
            let line_number = line_number as i32;
            let first_non_ws_column = text_model.get_line_first_non_whitespace_column(line_number);
            let line_content = text_model.get_line_content(line_number);
            if first_non_ws_column == 0 {
                continue;
            }
            let visible_column =
                visible_column_from_column(line_content.units(), first_non_ws_column, tab_size);
            result = result.min(i64::from(visible_column));
        }

        result
    }
}

/// `BracketAstNode`.
pub struct BracketAstNode {
    pub length: Length,
    pub bracket_info: Rc<BracketKind>,
    /// Numa abertura, o id dela. Num fechamento, os ids de todas as aberturas que ele pode fechar.
    pub bracket_ids: SmallImmutableSet,
}

impl BracketAstNode {
    pub fn create(
        length: Length,
        bracket_info: Rc<BracketKind>,
        bracket_ids: SmallImmutableSet,
    ) -> AstNodeRef {
        Rc::new(AstNode::Bracket(BracketAstNode {
            length,
            bracket_info,
            bracket_ids,
        }))
    }

    pub fn text(&self) -> &je_base::JsString {
        self.bracket_info.bracket_text()
    }

    pub fn language_id(&self) -> &str {
        self.bracket_info.language_id()
    }
}

/// `InvalidBracketAstNode`: um fechamento sem abertura.
pub struct InvalidBracketAstNode {
    pub length: Length,
    pub missing_opening_bracket_ids: SmallImmutableSet,
}

impl InvalidBracketAstNode {
    pub fn create(closing_brackets: SmallImmutableSet, length: Length) -> AstNodeRef {
        Rc::new(AstNode::UnexpectedClosingBracket(InvalidBracketAstNode {
            length,
            missing_opening_bracket_ids: closing_brackets,
        }))
    }
}

/// `PairAstNode`: um par de colchetes com o filho (`{ ... }`). Pode estar sem fechamento. Imutável, se todos
/// os filhos forem imutáveis.
pub struct PairAstNode {
    pub length: Length,
    pub opening_bracket: AstNodeRef,
    pub child: Option<AstNodeRef>,
    pub closing_bracket: Option<AstNodeRef>,
    pub missing_opening_bracket_ids: SmallImmutableSet,
}

impl PairAstNode {
    /// `PairAstNode.create(openingBracket, child, closingBracket)`: `opening_bracket` e `closing_bracket` são
    /// nós `Bracket`.
    pub fn create(
        opening_bracket: AstNodeRef,
        child: Option<AstNodeRef>,
        closing_bracket: Option<AstNodeRef>,
    ) -> AstNodeRef {
        let mut length = opening_bracket.length();
        if let Some(child) = &child {
            length = length_add(length, child.length());
        }
        if let Some(closing) = &closing_bracket {
            length = length_add(length, closing.length());
        }
        let missing = child
            .as_ref()
            .map_or_else(SmallImmutableSet::get_empty, |c| {
                c.missing_opening_bracket_ids()
            });
        Rc::new(AstNode::Pair(PairAstNode {
            length,
            opening_bracket,
            child,
            closing_bracket,
            missing_opening_bracket_ids: missing,
        }))
    }

    /// O colchete de abertura como [`BracketAstNode`].
    pub fn opening(&self) -> &BracketAstNode {
        self.opening_bracket
            .as_bracket()
            .expect("a abertura de um par é um colchete")
    }

    /// O colchete de fechamento como [`BracketAstNode`].
    pub fn closing(&self) -> Option<&BracketAstNode> {
        self.closing_bracket.as_ref().map(|c| {
            c.as_bracket()
                .expect("o fechamento de um par é um colchete")
        })
    }
}

enum ListChildren {
    /// `TwoThreeListAstNode`.
    TwoThree {
        item1: AstNodeRef,
        item2: AstNodeRef,
        item3: Option<AstNodeRef>,
    },
    /// `ArrayListAstNode`.
    Array(Vec<AstNodeRef>),
}

impl ListChildren {
    fn len(&self) -> usize {
        match self {
            ListChildren::TwoThree { item3, .. } => {
                if item3.is_some() {
                    3
                } else {
                    2
                }
            }
            ListChildren::Array(children) => children.len(),
        }
    }

    fn get(&self, idx: usize) -> Option<AstNodeRef> {
        match self {
            ListChildren::TwoThree {
                item1,
                item2,
                item3,
            } => match idx {
                0 => Some(item1.clone()),
                1 => Some(item2.clone()),
                2 => item3.clone(),
                _ => throw_error("Invalid child index"),
            },
            ListChildren::Array(children) => children.get(idx).cloned(),
        }
    }

    fn set(&mut self, idx: usize, node: AstNodeRef) {
        match self {
            ListChildren::TwoThree {
                item1,
                item2,
                item3,
            } => match idx {
                0 => *item1 = node,
                1 => *item2 = node,
                2 => *item3 = Some(node),
                _ => throw_error("Invalid child index"),
            },
            ListChildren::Array(children) => children[idx] = node,
        }
    }

    fn to_vec(&self) -> Vec<AstNodeRef> {
        match self {
            ListChildren::TwoThree {
                item1,
                item2,
                item3,
            } => {
                let mut result = vec![item1.clone(), item2.clone()];
                if let Some(item3) = item3 {
                    result.push(item3.clone());
                }
                result
            }
            ListChildren::Array(children) => children.clone(),
        }
    }

    fn shallow_clone(&self) -> ListChildren {
        match self {
            ListChildren::TwoThree {
                item1,
                item2,
                item3,
            } => ListChildren::TwoThree {
                item1: item1.clone(),
                item2: item2.clone(),
                item3: item3.clone(),
            },
            ListChildren::Array(children) => ListChildren::Array(children.clone()),
        }
    }
}

struct ListInner {
    length: Length,
    missing_opening_bracket_ids: SmallImmutableSet,
    children: ListChildren,
    cached_min_indentation: i64,
}

/// `ListAstNode`.
pub struct ListAstNode {
    pub list_height: u32,
    immutable: bool,
    inner: RefCell<ListInner>,
}

impl ListAstNode {
    fn new(
        length: Length,
        list_height: u32,
        immutable: bool,
        children: ListChildren,
        missing: SmallImmutableSet,
    ) -> Self {
        Self {
            list_height,
            immutable,
            inner: RefCell::new(ListInner {
                length,
                missing_opening_bracket_ids: missing,
                children,
                cached_min_indentation: -1,
            }),
        }
    }

    /// Usa nós de lista que guardam só 2 ou 3 filhos, mais econômicos em memória.
    pub fn create23(
        item1: AstNodeRef,
        item2: AstNodeRef,
        item3: Option<AstNodeRef>,
        immutable: bool,
    ) -> AstNodeRef {
        let mut length = item1.length();
        let mut missing_bracket_ids = item1.missing_opening_bracket_ids();

        if item1.list_height() != item2.list_height() {
            throw_error("Invalid list heights");
        }

        length = length_add(length, item2.length());
        missing_bracket_ids = missing_bracket_ids.merge(&item2.missing_opening_bracket_ids());

        if let Some(item3) = &item3 {
            if item1.list_height() != item3.list_height() {
                throw_error("Invalid list heights");
            }
            length = length_add(length, item3.length());
            missing_bracket_ids = missing_bracket_ids.merge(&item3.missing_opening_bracket_ids());
        }

        let height = item1.list_height() + 1;
        Rc::new(AstNode::List(ListAstNode::new(
            length,
            height,
            immutable,
            ListChildren::TwoThree {
                item1,
                item2,
                item3,
            },
            missing_bracket_ids,
        )))
    }

    /// `ListAstNode.create(items, immutable)`.
    pub fn create(items: Vec<AstNodeRef>, immutable: bool) -> AstNodeRef {
        if items.is_empty() {
            return Self::get_empty();
        }
        let mut length = items[0].length();
        let mut unopened_brackets = items[0].missing_opening_bracket_ids();
        for item in &items[1..] {
            length = length_add(length, item.length());
            unopened_brackets = unopened_brackets.merge(&item.missing_opening_bracket_ids());
        }
        let height = items[0].list_height() + 1;
        Rc::new(AstNode::List(ListAstNode::new(
            length,
            height,
            immutable,
            ListChildren::Array(items),
            unopened_brackets,
        )))
    }

    /// `ListAstNode.getEmpty()`.
    pub fn get_empty() -> AstNodeRef {
        Rc::new(AstNode::List(ListAstNode::new(
            LENGTH_ZERO,
            0,
            true,
            ListChildren::Array(Vec::new()),
            SmallImmutableSet::get_empty(),
        )))
    }

    pub fn length(&self) -> Length {
        self.inner.borrow().length
    }

    pub fn missing_opening_bracket_ids(&self) -> SmallImmutableSet {
        self.inner.borrow().missing_opening_bracket_ids.clone()
    }

    pub fn children_length(&self) -> usize {
        self.inner.borrow().children.len()
    }

    pub fn get_child(&self, idx: usize) -> Option<AstNodeRef> {
        self.inner.borrow().children.get(idx)
    }

    pub fn children(&self) -> Vec<AstNodeRef> {
        self.inner.borrow().children.to_vec()
    }

    fn throw_if_immutable(&self) {
        if self.immutable {
            throw_error("this instance is immutable");
        }
    }

    fn set_child(&self, idx: usize, child: AstNodeRef) {
        self.inner.borrow_mut().children.set(idx, child);
    }

    fn shallow_mutable_copy(&self) -> ListAstNode {
        let inner = self.inner.borrow();
        ListAstNode::new(
            inner.length,
            self.list_height,
            false,
            inner.children.shallow_clone(),
            inner.missing_opening_bracket_ids.clone(),
        )
    }

    pub fn make_last_element_mutable(&self) -> Option<AstNodeRef> {
        self.throw_if_immutable();
        let child_count = self.children_length();
        if child_count == 0 {
            return None;
        }
        let last_child = self
            .get_child(child_count - 1)
            .expect("o último filho existe");
        let mutable = if last_child.kind() == AstNodeKind::List {
            last_child.to_mutable()
        } else {
            last_child.clone()
        };
        if !Rc::ptr_eq(&last_child, &mutable) {
            self.set_child(child_count - 1, mutable.clone());
        }
        Some(mutable)
    }

    pub fn make_first_element_mutable(&self) -> Option<AstNodeRef> {
        self.throw_if_immutable();
        let child_count = self.children_length();
        if child_count == 0 {
            return None;
        }
        let first_child = self.get_child(0).expect("o primeiro filho existe");
        let mutable = if first_child.kind() == AstNodeKind::List {
            first_child.to_mutable()
        } else {
            first_child.clone()
        };
        if !Rc::ptr_eq(&first_child, &mutable) {
            self.set_child(0, mutable.clone());
        }
        Some(mutable)
    }

    fn can_be_reused(&self, open_bracket_ids: &SmallImmutableSet) -> bool {
        if open_bracket_ids.intersects(&self.missing_opening_bracket_ids()) {
            return false;
        }

        if self.children_length() == 0 {
            // Não reaproveite listas vazias.
            return false;
        }

        let mut last_child: AstNodeRef = match self.get_child(self.children_length() - 1) {
            Some(child) => child,
            None => return false,
        };
        while let AstNode::List(list) = &*last_child.clone() {
            let last_length = list.children_length();
            if last_length == 0 {
                // Listas vazias nunca devem estar dentro de outras listas.
                errors::throw(Error::bug_indicating(None));
            }
            last_child = list
                .get_child(last_length - 1)
                .expect("o último filho existe");
        }
        last_child.can_be_reused(open_bracket_ids)
    }

    pub fn handle_children_changed(&self) {
        self.throw_if_immutable();
        let mut inner = self.inner.borrow_mut();
        let count = inner.children.len();
        let first = inner
            .children
            .get(0)
            .expect("uma lista não é vazia ao mudar os filhos");
        let mut length = first.length();
        let mut unopened_brackets = first.missing_opening_bracket_ids();

        for i in 1..count {
            let child = inner.children.get(i).expect("o filho existe");
            length = length_add(length, child.length());
            unopened_brackets = unopened_brackets.merge(&child.missing_opening_bracket_ids());
        }
        inner.length = length;
        inner.missing_opening_bracket_ids = unopened_brackets;
        inner.cached_min_indentation = -1;
    }

    fn deep_clone(&self) -> AstNodeRef {
        let inner = self.inner.borrow();
        let children = match &inner.children {
            ListChildren::TwoThree {
                item1,
                item2,
                item3,
            } => ListChildren::TwoThree {
                item1: item1.deep_clone(),
                item2: item2.deep_clone(),
                item3: item3.as_ref().map(|i| i.deep_clone()),
            },
            ListChildren::Array(children) => {
                ListChildren::Array(children.iter().map(|c| c.deep_clone()).collect())
            }
        };
        // O clone profundo da lista imutável do original é mutável (`new TwoThreeListAstNode`/`ArrayListAstNode`).
        Rc::new(AstNode::List(ListAstNode::new(
            inner.length,
            self.list_height,
            false,
            children,
            inner.missing_opening_bracket_ids.clone(),
        )))
    }

    fn compute_min_indentation(&self, offset: Length, text_model: &TextModel) -> i64 {
        let cached = self.inner.borrow().cached_min_indentation;
        if cached != -1 {
            return cached;
        }

        let mut min_indentation = MAX_SAFE_INTEGER;
        let mut child_offset = offset;
        for i in 0..self.children_length() {
            if let Some(child) = self.get_child(i) {
                min_indentation =
                    min_indentation.min(child.compute_min_indentation(child_offset, text_model));
                child_offset = length_add(child_offset, child.length());
            }
        }

        self.inner.borrow_mut().cached_min_indentation = min_indentation;
        min_indentation
    }

    pub fn append_child_of_same_height(&self, node: AstNodeRef) {
        self.throw_if_immutable();
        {
            let mut inner = self.inner.borrow_mut();
            match &mut inner.children {
                ListChildren::TwoThree { item3, .. } => {
                    if item3.is_some() {
                        throw_error("Cannot append to a full (2,3) tree node");
                    }
                    *item3 = Some(node);
                }
                ListChildren::Array(children) => children.push(node),
            }
        }
        self.handle_children_changed();
    }

    pub fn unappend_child(&self) -> Option<AstNodeRef> {
        self.throw_if_immutable();
        let result = {
            let mut inner = self.inner.borrow_mut();
            match &mut inner.children {
                ListChildren::TwoThree { item3, .. } => match item3.take() {
                    Some(item) => Some(item),
                    None => throw_error("Cannot remove from a non-full (2,3) tree node"),
                },
                ListChildren::Array(children) => children.pop(),
            }
        };
        self.handle_children_changed();
        result
    }

    pub fn prepend_child_of_same_height(&self, node: AstNodeRef) {
        self.throw_if_immutable();
        {
            let mut inner = self.inner.borrow_mut();
            match &mut inner.children {
                ListChildren::TwoThree {
                    item1,
                    item2,
                    item3,
                } => {
                    if item3.is_some() {
                        throw_error("Cannot prepend to a full (2,3) tree node");
                    }
                    *item3 = Some(item2.clone());
                    *item2 = item1.clone();
                    *item1 = node;
                }
                ListChildren::Array(children) => children.insert(0, node),
            }
        }
        self.handle_children_changed();
    }

    pub fn unprepend_child(&self) -> Option<AstNodeRef> {
        self.throw_if_immutable();
        let result = {
            let mut inner = self.inner.borrow_mut();
            match &mut inner.children {
                ListChildren::TwoThree {
                    item1,
                    item2,
                    item3,
                } => match item3.take() {
                    Some(third) => {
                        let first = std::mem::replace(item1, item2.clone());
                        *item2 = third;
                        Some(first)
                    }
                    None => throw_error("Cannot remove from a non-full (2,3) tree node"),
                },
                ListChildren::Array(children) => {
                    if children.is_empty() {
                        None
                    } else {
                        Some(children.remove(0))
                    }
                }
            }
        };
        self.handle_children_changed();
        result
    }

    /// Se esta lista é imutável.
    pub fn is_immutable(&self) -> bool {
        self.immutable
    }
}
