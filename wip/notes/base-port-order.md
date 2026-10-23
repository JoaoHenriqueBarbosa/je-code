# Ordem de porte de `vs/base/common`

Gerado por `scripts/base_port_order.py`. [x] = portado em `crates/je-base`.

## Camada 0
- [x] `charCode` (450 linhas)
- [x] `collections` (176 linhas)
- [x] `decorators` (131 linhas)
- [x] `envfile` (100 linhas)
- [x] `errors` (357 linhas)
- [x] `functional` (32 linhas)
- [x] `hierarchicalKind` (31 linhas)
- [x] `idGenerator` (21 linhas)
- [x] `json` (1344 linhas)
- [x] `jsonSchema` (330 linhas)
- [x] `jsonc` (64 linhas)
- [x] `keyCodes` (838 linhas)
- [x] `lazy` (57 linhas)
- [x] `linkedList` (151 linhas)
- [ ] `managedSettings` (82 linhas)
- [x] `markdownLinks` (200 linhas)
- [x] `marshallingIds` (33 linhas)
- [x] `navigator` (50 linhas)
- [ ] `observable` (8 linhas)
- [x] `parsers` (79 linhas)
- [x] `platform` (284 linhas)
- [ ] `ports` (13 linhas)
- [x] `range` (60 linhas)
- [x] `sseParser` (245 linhas)
- [x] `stopwatch` (41 linhas)
- [x] `symbols` (9 linhas)
- [x] `uint` (59 linhas)
- [x] `uriTemplate` (305 linhas)
- [x] `uuid` (70 linhas)
- [x] `yaml` (1701 linhas)

## Camada 1
- [x] `assert` (89 linhas): errors
- [ ] `color` (942 linhas): charCode
- [x] `controlFlow` (69 linhas): errors
- [x] `date` (342 linhas): lazy, platform
- [x] `jsonErrorMessages` (26 linhas): json
- [x] `jsonFormatter` (261 linhas): json
- [x] `keybindings` (284 linhas): errors, keyCodes, platform
- [x] `layout` (166 linhas): range
- [x] `linkedText` (55 linhas): decorators
- [ ] `performance` (149 linhas): platform
- [ ] `process` (76 linhas): platform
- [ ] `processes` (151 linhas): platform

## Camada 2
- [ ] `desktopEnvironmentInfo` (101 linhas): process
- [x] `jsonEdit` (183 linhas): json, jsonFormatter
- [x] `keybindingLabels` (184 linhas): keybindings, platform
- [x] `keybindingParser` (102 linhas): keyCodes, keybindings
- [x] `numbers` (121 linhas): assert
- [x] `path` (1589 linhas): process
- [x] `types` (410 linhas): assert

## Camada 3
- [x] `comparers` (356 linhas): date, lazy, path
- [x] `iterator` (194 linhas): types
- [x] `mime` (139 linhas): path
- [x] `objects` (298 linhas): types
- [x] `uri` (765 linhas): charCode, marshallingIds, path, platform
- [ ] `verifier` (88 linhas): types

## Camada 4
- [ ] `console` (144 linhas): uri
- [x] `map` (1020 linhas): uri
- [x] `prefixTree` (259 linhas): iterator

## Camada 5
- [x] `normalization` (63 linhas): map

## Camada 6
- [x] `actions` (303 linhas): event, lifecycle, platform
- [x] `arrays` (949 linhas): arraysFind, cancellation, errors, sequence
- [x] `arraysFind` (226 linhas): arrays
- [ ] `async` (2752 linhas): cancellation, errors, event, lazy, lifecycle, platform, resources, symbols, uri
- [x] `buffer` (499 linhas): lazy, stream
- [x] `cache` (153 linhas): cancellation, lifecycle
- [x] `cancellation` (239 linhas): errors, event, lifecycle
- [ ] `codicons` (66 linhas): codiconsLibrary, codiconsUtil, themables
- [ ] `codiconsLibrary` (776 linhas): codiconsUtil
- [ ] `codiconsUtil` (28 linhas): themables, types
- [x] `dataTransfer` (198 linhas): arrays, iterator, uri, uuid
- [ ] `defaultAccount` (102 linhas): policy
- [ ] `editArcTracker` (365 linhas): errors, strings
- [x] `equals` (206 linhas): arrays
- [x] `errorMessage` (113 linhas): actions, arrays, types
- [x] `event` (1991 linhas): async, cancellation, collections, errors, functional, lifecycle, linkedList, observable, process, stopwatch, symbols
- [x] `extpath` (433 linhas): charCode, path, platform, strings, types
- [x] `filters` (981 linhas): charCode, map, normalization, strings
- [x] `fuzzyScorer` (953 linhas): charCode, comparers, filters, hash, path, platform, strings
- [x] `glob` (861 linhas): arrays, async, charCode, extpath, map, path, platform, strings
- [x] `hash` (361 linhas): assert, buffer, strings
- [x] `history` (298 linhas): collections, event, lifecycle, navigator
- [ ] `hotReload` (119 linhas): lifecycle
- [ ] `hotReloadHelpers` (52 linhas): hotReload, observable
- [x] `htmlContent` (287 linhas): errors, iconLabels, network, resources, strings, uri
- [x] `iconLabels` (116 linhas): filters, strings, themables
- [ ] `image` (150 linhas): buffer
- [ ] `ime` (36 linhas): event
- [x] `jsonRpcProtocol` (303 linhas): async, cancellation, errors, lifecycle, types
- [x] `labels` (484 linhas): extpath, path, platform, resources, strings, uri
- [x] `lifecycle` (974 linhas): arrays, collections, errors, functional, iterator, map, uri
- [x] `marshalling` (84 linhas): buffer, marshallingIds, uri
- [x] `network` (452 linhas): errors, path, platform, strings, uri
- [ ] `oauth` (1423 linhas): buffer
- [x] `paging` (392 linhas): arrays, cancellation, errors, event
- [ ] `policy` (161 linhas): defaultAccount
- [ ] `product` (478 linhas): collections, platform, policy
- [x] `resourceTree` (187 linhas): decorators, path, resources, ternarySearchTree, uri
- [x] `resources` (458 linhas): charCode, extpath, network, path, platform, strings, uri
- [x] `scrollable` (522 linhas): event, lifecycle
- [ ] `search` (48 linhas): strings
- [x] `sequence` (34 linhas): event
- [x] `severity` (56 linhas): strings
- [x] `stream` (790 linhas): cancellation, errors, lifecycle
- [x] `strings` (1424 linhas): cache, charCode, lazy, uint
- [x] `ternarySearchTree` (796 linhas): arrays, assert, charCode, strings, uri
- [x] `tfIdf` (240 linhas): cancellation
- [x] `themables` (117 linhas): codicons
- [ ] `uriIpc` (163 linhas): buffer, marshalling, marshallingIds, uri
- [ ] `uriTransformer` (51 linhas): uriIpc
- [x] `validation` (396 linhas): arrays, jsonSchema
- [ ] `zip` (91 linhas): buffer, lazy
