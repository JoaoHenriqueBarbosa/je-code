# Placar de conformidade

Gerado por `scripts/conformance.py`. Oráculo: testes mocha do VS Code 1.143.1; portado: `#[test]` nos arquivos que declaram a origem. Pastas fora de escopo (decisão do user) não entram.

| Camada | Arquivos de teste | Casos do oráculo | Arquivos portados | Casos portados | % |
| --- | --- | --- | --- | --- | --- |
| `base` | 150 | 2278 | 53 | 1147 | 50.35 |
| `platform` | 723 | 15868 | 13 | 231 | 1.46 |
| `editor` | 182 | 3144 | 17 | 534 | 16.98 |
| `workbench/services` | 137 | 2491 | 0 | 0 | 0.00 |
| `workbench/api` | 94 | 965 | 0 | 0 | 0.00 |
| `workbench/contrib` | 402 | 5033 | 0 | 0 | 0.00 |
| `server` | 8 | 75 | 0 | 0 | 0.00 |
| `code` | 5 | 15 | 0 | 0 | 0.00 |
| **total** | | 29869 | | 1912 | 6.40 |

## Arquivos portados

| Origem | Casos do oráculo | Porte | Casos portados |
| --- | --- | --- | --- |
| `src/vs/base/test/browser/comparers.test.ts` | 10 | `crates/je-base/tests/comparers.rs` | 10 |
| `src/vs/base/test/common/arrays.test.ts` | 29 | `crates/je-base/tests/arrays.rs` | 28 |
| `src/vs/base/test/common/arraysFind.test.ts` | 3 | `crates/je-base/tests/arrays_find.rs` | 3 |
| `src/vs/base/test/common/assert.test.ts` | 4 | `crates/je-base/tests/assert.rs` | 4 |
| `src/vs/base/test/common/async.test.ts` | 145 | `crates/je-base/tests/async_.rs` | 149 |
| `src/vs/base/test/common/buffer.test.ts` | 31 | `crates/je-base/tests/buffer.rs` | 31 |
| `src/vs/base/test/common/cache.test.ts` | 3 | `crates/je-base/tests/cache.rs` | 3 |
| `src/vs/base/test/common/cancellation.test.ts` | 22 | `crates/je-base/tests/cancellation.rs` | 22 |
| `src/vs/base/test/common/collections.test.ts` | 12 | `crates/je-base/tests/collections.rs` | 12 |
| `src/vs/base/test/common/color.test.ts` | 29 | `crates/je-theme/tests/color.rs` | 29 |
| `src/vs/base/test/common/date.test.ts` | 13 | `crates/je-base/tests/date.rs` | 13 |
| `src/vs/base/test/common/decorators.test.ts` | 5 | `crates/je-base/tests/decorators.rs` | 3 |
| `src/vs/base/test/common/diff/diff.test.ts` | 2 | `crates/je-base/tests/diff.rs` | 2 |
| `src/vs/base/test/common/envfile.test.ts` | 1 | `crates/je-base/tests/envfile.rs` | 1 |
| `src/vs/base/test/common/event.test.ts` | 93 | `crates/je-base/tests/event.rs` | 93 |
| `src/vs/base/test/common/extpath.test.ts` | 11 | `crates/je-base/tests/extpath.rs` | 17 |
| `src/vs/base/test/common/filters.test.ts` | 40 | `crates/je-base/tests/filters.rs` | 41 |
| `src/vs/base/test/common/fuzzyScorer.test.ts` | 68 | `crates/je-base/tests/fuzzy_scorer.rs` | 68 |
| `src/vs/base/test/common/glob.test.ts` | 51 | `crates/je-base/tests/glob.rs` | 60 |
| `src/vs/base/test/common/hash.test.ts` | 5 | `crates/je-base/tests/hash.rs` | 8 |
| `src/vs/base/test/common/history.test.ts` | 27 | `crates/je-base/tests/history.rs` | 27 |
| `src/vs/base/test/common/iconLabels.test.ts` | 5 | `crates/je-base/tests/icon_labels.rs` | 5 |
| `src/vs/base/test/common/iterator.test.ts` | 3 | `crates/je-base/tests/iterator.rs` | 3 |
| `src/vs/base/test/common/json.test.ts` | 17 | `crates/je-base/tests/json.rs` | 17 |
| `src/vs/base/test/common/keyCodes.test.ts` | 42 | `crates/je-base/tests/key_codes.rs` | 4 |
| `src/vs/base/test/common/keybindings.test.ts` | 1 | `crates/je-base/tests/keybindings.rs` | 1 |
| `src/vs/base/test/common/labels.test.ts` | 3 | `crates/je-base/tests/labels.rs` | 5 |
| `src/vs/base/test/common/lazy.test.ts` | 3 | `crates/je-base/tests/lazy.rs` | 3 |
| `src/vs/base/test/common/lifecycle.test.ts` | 30 | `crates/je-base/tests/lifecycle.rs` | 30 |
| `src/vs/base/test/common/linkedList.test.ts` | 7 | `crates/je-base/tests/linked_list.rs` | 7 |
| `src/vs/base/test/common/map.test.ts` | 44 | `crates/je-base/tests/map.rs` | 44 |
| `src/vs/base/test/common/markdownString.test.ts` | 14 | `crates/je-base/tests/markdown_string.rs` | 14 |
| `src/vs/base/test/common/marshalling.test.ts` | 3 | `crates/je-base/tests/marshalling.rs` | 3 |
| `src/vs/base/test/common/mime.test.ts` | 3 | `crates/je-base/tests/mime.rs` | 3 |
| `src/vs/base/test/common/naturalLanguage/korean.test.ts` | 5 | `crates/je-base/tests/korean.rs` | 5 |
| `src/vs/base/test/common/network.test.ts` | 1 | `crates/je-base/tests/network.rs` | 6 |
| `src/vs/base/test/common/normalization.test.ts` | 1 | `crates/je-base/tests/normalization.rs` | 1 |
| `src/vs/base/test/common/numbers.test.ts` | 6 | `crates/je-base/tests/numbers.rs` | 6 |
| `src/vs/base/test/common/objects.test.ts` | 10 | `crates/je-base/tests/objects.rs` | 11 |
| `src/vs/base/test/common/observables/debug.test.ts` | 1 | `crates/je-base/tests/observable_debug.rs` | 1 |
| `src/vs/base/test/common/observables/observable.test.ts` | 43 | `crates/je-base/tests/observable.rs` | 44 |
| `src/vs/base/test/common/observables/throttledObservable.test.ts` | 6 | `crates/je-base/tests/observable_throttled.rs` | 6 |
| `src/vs/base/test/common/paging.test.ts` | 7 | `crates/je-base/tests/paging.rs` | 7 |
| `src/vs/base/test/common/path.test.ts` | 9 | `crates/je-base/tests/path.rs` | 9 |
| `src/vs/base/test/common/prefixTree.test.ts` | 13 | `crates/je-base/tests/prefix_tree.rs` | 13 |
| `src/vs/base/test/common/resources.test.ts` | 16 | `crates/je-base/tests/resources.rs` | 31 |
| `src/vs/base/test/common/stream.test.ts` | 24 | `crates/je-base/tests/stream.rs` | 21 |
| `src/vs/base/test/common/strings.test.ts` | 48 | `crates/je-base/tests/strings.rs` | 46 |
| `src/vs/base/test/common/ternarySearchtree.test.ts` | 32 | `crates/je-base/tests/ternary_search_tree.rs` | 31 |
| `src/vs/base/test/common/types.test.ts` | 66 | `crates/je-base/tests/types.rs` | 66 |
| `src/vs/base/test/common/uri.test.ts` | 46 | `crates/je-base/tests/uri.rs` | 51 |
| `src/vs/base/test/common/uriTemplate.test.ts` | 26 | `crates/je-base/tests/uri_template.rs` | 26 |
| `src/vs/base/test/common/uuid.test.ts` | 3 | `crates/je-base/tests/uuid.rs` | 3 |
| `src/vs/platform/configuration/test/common/configuration.test.ts` | 21 | `crates/je-platform/tests/configuration.rs` | 21 |
| `src/vs/platform/configuration/test/common/configurationModels.test.ts` | 94 | `crates/je-platform/tests/configuration_models.rs` | 94 |
| `src/vs/platform/configuration/test/common/configurationRegistry.test.ts` | 22 | `crates/je-platform/tests/configuration_registry.rs` | 22 |
| `src/vs/platform/configuration/test/common/configurationService.test.ts` | 17 | `crates/je-platform/tests/configuration_service.rs` | 0 |
| `src/vs/platform/configuration/test/common/configurationVariables.test.ts` | 7 | `crates/je-platform/tests/configuration_variables.rs` | 8 |
| `src/vs/platform/configuration/test/common/configurations.test.ts` | 15 | `crates/je-platform/tests/configurations.rs` | 15 |
| `src/vs/platform/configuration/test/common/policyConfiguration.test.ts` | 24 | `crates/je-platform/tests/policy_configuration.rs` | 0 |
| `src/vs/platform/files/test/browser/inmemoryFileService.test.ts` | 20 | `crates/je-platform/tests/in_memory_file_service.rs` | 20 |
| `src/vs/platform/files/test/common/files.test.ts` | 6 | `crates/je-platform/tests/files.rs` | 6 |
| `src/vs/platform/files/test/common/watcher.test.ts` | 8 | `crates/je-platform/tests/files_watcher.rs` | 12 |
| `src/vs/platform/instantiation/test/common/graph.test.ts` | 5 | `crates/je-instantiation/tests/graph.rs` | 5 |
| `src/vs/platform/instantiation/test/common/instantiationService.test.ts` | 24 | `crates/je-instantiation/tests/instantiation_service.rs` | 24 |
| `src/vs/platform/undoRedo/test/common/undoRedoService.test.ts` | 4 | `crates/je-editor/tests/undo_redo_service.rs` | 4 |
| `src/vs/editor/test/common/core/characterClassifier.test.ts` | 1 | `crates/je-editor/tests/character_classifier.rs` | 1 |
| `src/vs/editor/test/common/core/cursorColumns.test.ts` | 5 | `crates/je-editor/tests/cursor_columns.rs` | 7 |
| `src/vs/editor/test/common/core/range.test.ts` | 9 | `crates/je-editor/tests/range.rs` | 9 |
| `src/vs/editor/test/common/core/stringBuilder.test.ts` | 4 | `crates/je-editor/tests/string_builder.rs` | 4 |
| `src/vs/editor/test/common/model/editStack.test.ts` | 1 | `crates/je-editor/tests/edit_stack.rs` | 1 |
| `src/vs/editor/test/common/model/editableTextModel.test.ts` | 62 | `crates/je-editor/tests/editable_text_model.rs` | 62 |
| `src/vs/editor/test/common/model/editableTextModelAuto.test.ts` | 4 | `crates/je-editor/tests/editable_text_model_auto.rs` | 4 |
| `src/vs/editor/test/common/model/intervalTree.test.ts` | 25 | `crates/je-editor/tests/interval_tree.rs` | 25 |
| `src/vs/editor/test/common/model/model.line.test.ts` | 53 | `crates/je-editor/tests/model_line.rs` | 53 |
| `src/vs/editor/test/common/model/model.test.ts` | 32 | `crates/je-editor/tests/model.rs` | 32 |
| `src/vs/editor/test/common/model/modelDecorations.test.ts` | 101 | `crates/je-editor/tests/model_decorations.rs` | 101 |
| `src/vs/editor/test/common/model/modelEditOperation.test.ts` | 8 | `crates/je-editor/tests/model_edit_operation.rs` | 8 |
| `src/vs/editor/test/common/model/pieceTreeTextBuffer/pieceTreeTextBuffer.test.ts` | 87 | `crates/je-editor/tests/piece_tree_text_buffer.rs` | 87 |
| `src/vs/editor/test/common/model/textChange.test.ts` | 10 | `crates/je-editor/tests/text_change.rs` | 10 |
| `src/vs/editor/test/common/model/textModel.test.ts` | 40 | `crates/je-editor/tests/text_model.rs` | 40 |
| `src/vs/editor/test/common/model/textModelSearch.test.ts` | 42 | `crates/je-editor/tests/text_model_search.rs` | 42 |
| `src/vs/editor/test/common/viewModel/prefixSumComputer.test.ts` | 48 | `crates/je-editor/tests/prefix_sum_computer.rs` | 48 |
