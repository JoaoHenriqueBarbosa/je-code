# Placar de conformidade

Gerado por `scripts/conformance.py`. Oráculo: testes mocha do VS Code 1.143.1; portado: `#[test]` nos arquivos que declaram a origem. Pastas fora de escopo (decisão do user) não entram.

| Camada | Arquivos de teste | Casos do oráculo | Arquivos portados | Casos portados | % |
| --- | --- | --- | --- | --- | --- |
| `base` | 150 | 2278 | 26 | 582 | 25.55 |
| `platform` | 723 | 15868 | 0 | 0 | 0.00 |
| `editor` | 182 | 3144 | 0 | 0 | 0.00 |
| `workbench/services` | 137 | 2491 | 0 | 0 | 0.00 |
| `workbench/api` | 94 | 965 | 0 | 0 | 0.00 |
| `workbench/contrib` | 402 | 5033 | 0 | 0 | 0.00 |
| `server` | 8 | 75 | 0 | 0 | 0.00 |
| `code` | 5 | 15 | 0 | 0 | 0.00 |
| **total** | | 29869 | | 582 | 1.95 |

## Arquivos portados

| Origem | Casos do oráculo | Porte | Casos portados |
| --- | --- | --- | --- |
| `src/vs/base/test/common/arrays.test.ts` | 29 | `crates/je-base/tests/arrays.rs` | 28 |
| `src/vs/base/test/common/arraysFind.test.ts` | 3 | `crates/je-base/tests/arrays_find.rs` | 3 |
| `src/vs/base/test/common/assert.test.ts` | 4 | `crates/je-base/tests/assert.rs` | 4 |
| `src/vs/base/test/common/cache.test.ts` | 3 | `crates/je-base/tests/cache.rs` | 3 |
| `src/vs/base/test/common/cancellation.test.ts` | 22 | `crates/je-base/tests/cancellation.rs` | 22 |
| `src/vs/base/test/common/collections.test.ts` | 12 | `crates/je-base/tests/collections.rs` | 12 |
| `src/vs/base/test/common/color.test.ts` | 29 | `crates/je-theme/tests/color.rs` | 29 |
| `src/vs/base/test/common/event.test.ts` | 93 | `crates/je-base/tests/event.rs` | 92 |
| `src/vs/base/test/common/extpath.test.ts` | 11 | `crates/je-base/tests/extpath.rs` | 17 |
| `src/vs/base/test/common/hash.test.ts` | 5 | `crates/je-base/tests/hash.rs` | 8 |
| `src/vs/base/test/common/iterator.test.ts` | 3 | `crates/je-base/tests/iterator.rs` | 3 |
| `src/vs/base/test/common/json.test.ts` | 17 | `crates/je-base/tests/json.rs` | 17 |
| `src/vs/base/test/common/lazy.test.ts` | 3 | `crates/je-base/tests/lazy.rs` | 3 |
| `src/vs/base/test/common/lifecycle.test.ts` | 30 | `crates/je-base/tests/lifecycle.rs` | 30 |
| `src/vs/base/test/common/linkedList.test.ts` | 7 | `crates/je-base/tests/linked_list.rs` | 7 |
| `src/vs/base/test/common/map.test.ts` | 44 | `crates/je-base/tests/map.rs` | 44 |
| `src/vs/base/test/common/network.test.ts` | 1 | `crates/je-base/tests/network.rs` | 6 |
| `src/vs/base/test/common/numbers.test.ts` | 6 | `crates/je-base/tests/numbers.rs` | 6 |
| `src/vs/base/test/common/objects.test.ts` | 10 | `crates/je-base/tests/objects.rs` | 11 |
| `src/vs/base/test/common/path.test.ts` | 9 | `crates/je-base/tests/path.rs` | 9 |
| `src/vs/base/test/common/resources.test.ts` | 16 | `crates/je-base/tests/resources.rs` | 31 |
| `src/vs/base/test/common/strings.test.ts` | 48 | `crates/je-base/tests/strings.rs` | 46 |
| `src/vs/base/test/common/ternarySearchtree.test.ts` | 32 | `crates/je-base/tests/ternary_search_tree.rs` | 31 |
| `src/vs/base/test/common/types.test.ts` | 66 | `crates/je-base/tests/types.rs` | 66 |
| `src/vs/base/test/common/uri.test.ts` | 46 | `crates/je-base/tests/uri.rs` | 51 |
| `src/vs/base/test/common/uuid.test.ts` | 3 | `crates/je-base/tests/uuid.rs` | 3 |
