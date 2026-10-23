//! Testes próprios de `src/vs/base/common/idGenerator.ts` (o VS Code não tem `idGenerator.test.ts`).

use je_base::id_generator::{IdGenerator, default_generator_next_id};

#[test]
fn next_id_is_sequential_with_prefix() {
    let generator = IdGenerator::new("x-");
    assert_eq!(generator.next_id(), "x-1");
    assert_eq!(generator.next_id(), "x-2");
    assert_eq!(IdGenerator::new("x-").next_id(), "x-1");
}

#[test]
fn default_generator_uses_hash_prefix() {
    assert_eq!(default_generator_next_id(), "id#1");
    assert_eq!(default_generator_next_id(), "id#2");
}
