# Placar de conformidade

Gerado por `scripts/conformance.py`. Oráculo: testes mocha do VS Code 1.143.1; portado: `#[test]` nos arquivos que declaram a origem. Pastas fora de escopo (decisão do user) não entram.

| Camada | Arquivos de teste | Casos do oráculo | Arquivos portados | Casos portados | % |
| --- | --- | --- | --- | --- | --- |
| `base` | 150 | 2278 | 3 | 72 | 3.16 |
| `platform` | 723 | 15868 | 0 | 0 | 0.00 |
| `editor` | 182 | 3144 | 0 | 0 | 0.00 |
| `workbench/services` | 137 | 2491 | 0 | 0 | 0.00 |
| `workbench/api` | 94 | 965 | 0 | 0 | 0.00 |
| `workbench/contrib` | 402 | 5033 | 0 | 0 | 0.00 |
| `server` | 8 | 75 | 0 | 0 | 0.00 |
| `code` | 5 | 15 | 0 | 0 | 0.00 |
| **total** | | 29869 | | 72 | 0.24 |

## Arquivos portados

| Origem | Casos do oráculo | Porte | Casos portados |
| --- | --- | --- | --- |
| `src/vs/base/test/common/json.test.ts` | 17 | `crates/je-base/tests/json.rs` | 17 |
| `src/vs/base/test/common/path.test.ts` | 9 | `crates/je-base/tests/path.rs` | 9 |
| `src/vs/base/test/common/strings.test.ts` | 48 | `crates/je-base/tests/strings.rs` | 46 |
