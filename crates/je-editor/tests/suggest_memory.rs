//! Porte de `src/vs/editor/contrib/suggest/test/browser/suggestMemory.test.ts` (suíte `SuggestMemories`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; o `setup` e o
//!   `teardown` (`buffer.dispose()`) são [`setup`] e o `dispose` no fim de cada caso.
//! - `createSuggestItem` (de `completionModel.test.ts`) é `create_suggest_item` de `tests/support`.
//! - `item.completion.preselect = true` é `item.completion.borrow_mut().preselect = Some(true)`, e
//!   `item1.score = [10, 0, 0]` é a atribuição ao campo `score` do item dentro da lista.
//! - A subclasse anônima de `Memory` do primeiro caso é a struct local [`AbstractMemory`], cujos métodos não
//!   implementados lançam `Method not implemented.` como no original.
//! - No caso `NoMemory`, o `mem.memorize(buffer, pos, null!)` não existe em Rust: a referência ao item não
//!   pode ser nula, e o corpo do `NoMemory` não lê o item, então a segunda chamada não testaria nada novo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::json::Value;
use je_editor::position::Position;
use je_editor::suggest::suggest::CompletionItem;
use je_editor::suggest::suggest_memory::{LruMemory, MalformedMemory, MemMode, Memory, NoMemory, PrefixMemory};
use je_editor::text_model::TextModel;
use support::suggest_test_utils::create_suggest_item;
use support::test_text_model::create_text_model;

fn setup() -> (Position, Rc<TextModel>, Vec<CompletionItem>) {
    let pos = Position::new(1, 1);
    let buffer = create_text_model("This is some text.\nthis.\nfoo: ,");
    let items = vec![create_suggest_item("foo", 0), create_suggest_item("bar", 0)];
    (pos, buffer, items)
}

/// A subclasse anônima de `Memory` do caso `AbstractMemory, select`.
struct AbstractMemory;

impl Memory for AbstractMemory {
    fn name(&self) -> MemMode {
        MemMode::First
    }

    fn memorize(&mut self, _model: &TextModel, _pos: Position, _item: &CompletionItem) {
        panic!("Method not implemented.");
    }

    fn to_json(&self) -> Option<Value> {
        panic!("Method not implemented.");
    }

    fn from_json(&mut self, _data: &Value) -> Result<(), MalformedMemory> {
        panic!("Method not implemented.");
    }
}

#[test]
fn abstract_memory_select() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (pos, buffer, _items) = setup();

    let mem = AbstractMemory;

    let item1 = create_suggest_item("fazz", 0);
    let item2 = create_suggest_item("bazz", 0);
    let item3 = create_suggest_item("bazz", 0);
    let item4 = create_suggest_item("bazz", 0);
    item1.completion.borrow_mut().preselect = Some(false);
    item2.completion.borrow_mut().preselect = Some(true);
    item3.completion.borrow_mut().preselect = Some(true);

    assert_eq!(mem.select(&buffer, pos, &[item1, item2, item3, item4]), 1);
    buffer.dispose();
}

#[test]
fn no_prefix_lru_memory_honor_selection_boost() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (pos, buffer, _items) = setup();

    let item1 = create_suggest_item("fazz", 0);
    let item2 = create_suggest_item("bazz", 0);
    let item3 = create_suggest_item("bazz", 0);
    let item4 = create_suggest_item("bazz", 0);
    item1.completion.borrow_mut().preselect = Some(false);
    item2.completion.borrow_mut().preselect = Some(true);
    item3.completion.borrow_mut().preselect = Some(true);
    let items = [item1, item2, item3, item4];

    assert_eq!(NoMemory::new().select(&buffer, pos, &items), 1);
    assert_eq!(LruMemory::new().select(&buffer, pos, &items), 1);
    assert_eq!(PrefixMemory::new().select(&buffer, pos, &items), 1);
    buffer.dispose();
}

#[test]
fn no_memory() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (pos, buffer, items) = setup();

    let mut mem = NoMemory::new();

    assert_eq!(mem.select(&buffer, pos, &items), 0);
    assert_eq!(mem.select(&buffer, pos, &[]), 0);

    mem.memorize(&buffer, pos, &items[0]);
    buffer.dispose();
}

#[test]
fn lru_memory() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_pos, buffer, items) = setup();

    let pos = Position::new(2, 6);

    let mut mem = LruMemory::new();
    mem.memorize(&buffer, pos, &items[1]);

    assert_eq!(mem.select(&buffer, pos, &items), 1);
    assert_eq!(mem.select(&buffer, Position::new(1, 3), &items), 0);

    mem.memorize(&buffer, pos, &items[0]);
    assert_eq!(mem.select(&buffer, pos, &items), 0);

    assert_eq!(mem.select(&buffer, pos, &[create_suggest_item("new", 0), create_suggest_item("bar", 0)]), 1);

    assert_eq!(mem.select(&buffer, pos, &[create_suggest_item("new1", 0), create_suggest_item("new2", 0)]), 0);
    buffer.dispose();
}

#[test]
fn editor_suggest_selection_recently_used_should_be_a_little_more_sticky_78571() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_pos, buffer, _items) = setup();

    let item1 = create_suggest_item("gamma", 0);
    let item2 = create_suggest_item("game", 0);
    let items = vec![item1, item2];

    let mut mem = LruMemory::new();
    buffer.set_value("    foo.".into());
    mem.memorize(&buffer, Position::new(1, 1), &items[1]);

    assert_eq!(mem.select(&buffer, Position::new(1, 2), &items), 0); // leading whitespace -> ignore recent items

    mem.memorize(&buffer, Position::new(1, 9), &items[1]);
    assert_eq!(mem.select(&buffer, Position::new(1, 9), &items), 1); // foo.

    buffer.set_value("    foo.g".into());
    assert_eq!(mem.select(&buffer, Position::new(1, 10), &items), 1); // foo.g, 'gamma' and 'game' have the same score

    *items[0].score.borrow_mut() = vec![10, 0, 0];
    assert_eq!(mem.select(&buffer, Position::new(1, 10), &items), 0); // foo.g, 'gamma' has higher score
    buffer.dispose();
}

#[test]
fn intellisense_is_not_showing_top_options_first_43429() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_pos, buffer, items) = setup();

    // ensure we don't memorize for whitespace prefixes

    let pos = Position::new(2, 6);
    let mut mem = LruMemory::new();

    mem.memorize(&buffer, pos, &items[1]);
    assert_eq!(mem.select(&buffer, pos, &items), 1);

    assert_eq!(mem.select(&buffer, Position::new(3, 5), &items), 0); // foo: |,
    assert_eq!(mem.select(&buffer, Position::new(3, 6), &items), 1); // foo: ,|
    buffer.dispose();
}

#[test]
fn prefix_memory() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (_pos, buffer, _items) = setup();

    let mut mem = PrefixMemory::new();
    buffer.set_value("constructor".into());
    let item0 = create_suggest_item("console", 0);
    let item1 = create_suggest_item("const", 0);
    let item2 = create_suggest_item("constructor", 0);
    let item3 = create_suggest_item("constant", 0);
    let items = [item0, item1, item2, item3];

    mem.memorize(&buffer, Position::new(1, 2), &items[1]); // c -> const
    mem.memorize(&buffer, Position::new(1, 3), &items[0]); // co -> console
    mem.memorize(&buffer, Position::new(1, 4), &items[2]); // con -> constructor

    assert_eq!(mem.select(&buffer, Position::new(1, 1), &items), 0);
    assert_eq!(mem.select(&buffer, Position::new(1, 2), &items), 1);
    assert_eq!(mem.select(&buffer, Position::new(1, 3), &items), 0);
    assert_eq!(mem.select(&buffer, Position::new(1, 4), &items), 2);
    assert_eq!(mem.select(&buffer, Position::new(1, 7), &items), 2); // find substr
    buffer.dispose();
}
