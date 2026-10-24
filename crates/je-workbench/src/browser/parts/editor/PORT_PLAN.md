# Plano de porte: EditorPart, EditorGroupView e o grid

Levantamento do que `src/vs/workbench/services/editor/test/browser/editorGroupsService.test.ts` (69 casos, suíte
`EditorGroupsService`) exige e ainda não existe em Rust. Estado conferido em `crates/je-workbench/src` e
`crates/je-widgets/src`.

## O que já existe

- **Grid**: `GridView`, `Grid` e `SerializableGrid` já estão portados em `crates/je-widgets/src/{gridview,grid,splitview}.rs`,
  com os testes do oráculo (`tests/grid.rs`, 38 casos, e `tests/gridview.rs`, 9 casos, os mesmos números de
  `docs/conformance.md`). Não há o que portar em `je-base`: duplicar seria retrabalho. O `EditorPart` usa
  `je_widgets::grid::SerializableGrid<T>`.
- **Modelo de grupo**: `common/editor/editor_group_model.rs` (`EditorGroupModel`, eventos, serialização, MRU, seleção,
  transiente, aderente) e `filtered_editor_group_model.rs`.
- **Serviços**: `services/editor/{editor_group_column,editor_group_finder,editor_groups_service,editor_service}.rs`. O
  `IEditorGroupsService` de lá é reduzido (só `active_group`, `groups`, `get_groups`, `get_group`, `find_group`,
  `add_group`, `main_part`, partes auxiliar e modal). O `IEditorGroup` é reduzido a `id`, `window_id`, `active_editor`,
  `editors`, `is_locked`, `is_active`, `close_editor`, `move_editor`.
- **Parte modal**: `browser/parts/editor/modal_editor_part.rs` tem só o estado e a aritmética de layout; o próprio
  arquivo registra que `ModalEditorPartImpl` estende o `EditorPart`, que ainda não existe.
- **Teste**: `test/browser/workbench_test_services.rs` tem `TestFileEditorInput` e um `TestEditorGroupsService`
  de mentira (lista fixa de grupos). Não há `TestEditorPart`, `TestEditorParts` nem `createEditorParts`.

## O que falta, por grupo de casos do teste

### A. Contrato de `common/editorGroupsService.ts` (todos os casos dependem)

Faltam os tipos `MergeGroupMode`, `GroupOrientation`, `GroupsArrangement`, `GroupActivationReason`,
`IEditorGroupActivationEvent`, `EditorGroupLayout` e `GroupLayoutArgument`, `IMergeGroupOptions`,
`IEditorGroupContextKeyProvider`, `isEditorGroup`, e o `IEditorGroup` completo (`label`, `index`, `count`... aqui `index`,
`label`, `ariaLabel`, `editors`, `activeEditor`, `previewEditor`, `selectedEditors`, `stickyCount`, `isPinned`,
`isSticky`, `isSelected`, `isTransient`, `openEditor(s)`, `closeEditor(s)`, `closeAllEditors`, `moveEditor(s)`,
`copyEditor(s)`, `replaceEditors`, `pinEditor`, `stickEditor`, `setSelection`, `lock`, `findEditors`, `contains`,
`getEditorByIndex`, `getIndexOfEditor`, `onDidModelChange`, `onWillOpenEditor`, `onWillMoveEditor`...). Os eventos e o
`IEditorGroupsContainer` (`onDidAddGroup`, `onDidRemoveGroup`, `onDidMoveGroup`, `onDidChangeActiveGroup`,
`onDidChangeGroupIndex`, `onDidChangeGroupLabel`, `onDidChangeGroupLocked`, `onDidChangeGroupMaximized`,
`onDidActivateGroup`, `whenReady`, `whenRestored`, `partOptions`, `onDidChangeEditorPartOptions`) também.
Cobre: `groups basics`, `groups index / labels`, `groups label`, `options`, `whenReady / whenRestored`,
`onDidActivateGroup carries activation reason`.

### B. `EditorPart` (editorPart.ts, 1679 linhas) sem DOM

Lógica a portar: criação do grid (`doCreateGridControl*`, `doSetGridWidget`), `getGroups(order)` e `fillGridNodes`,
`findGroup` por direção e por localização, `activateGroup`, `addGroup` (com cópia e `getSplitSizingStyle`),
`removeGroup` (com e sem editores), `moveGroup`, `copyGroup`, `mergeGroup`, `mergeAllGroups`, `arrangeGroups`,
`toggleMaximizeGroup`, `toggleExpandGroup`, `setGroupOrientation`, `applyLayout`, `getLayout`, `centerLayout`,
`notifyGroupsLabelChange`, `enforcePartOptions`, `createState`/`applyState`/`doApplyState`, `toJSON`, persistência
(`loadState`, `saveState`, memento). Também os context keys por grupo (`handleContextKeys`) e o provider.
Cobre: `groups basics`, `sideGroup`, `save & restore state`, `copy/merge groups`, `merge all groups`,
`find neighbour group (left/right, up/down)`, `find group by location`, `applyLayout (2x2)`, `getLayout`,
`centeredLayout`, `maximize editor group`, `working sets (3 casos)`, `context key provider (4 casos)`,
`locked groups (4 casos)`, `onDidActivateGroup carries activation reason`.
O que é pintura (`updateStyles`, drag e drop, `setBoundarySashes`, `createEditorDropTarget`, `layout` em pixels) fica para
a árvore de widgets; o `layout` numérico entra porque o grid decide tamanhos.

### C. `EditorGroupView` (editorGroupView.ts, 2301 linhas) sem DOM

É onde estão todos os casos de editor: `openEditor(s)`, `closeEditor(s)` com tratamento de sujo (`handleCloseConfirmation`,
`ConfirmResult`) e de `CannotClose`, `closeAllEditors`, `moveEditor(s)` dentro do grupo e entre grupos, `copyEditor(s)`,
`replaceEditors` (com `forceReplaceDirty`, índices e lado a lado), `findEditors`, aderentes e fixados (`sticky: true`
vence o índice), transientes, seleção (`setSelection`, `isSelected`, `selectedEditors`), `moveEditor with context`,
`copyEditor with context`, `moveEditor disabled`, `onWillOpenEditor`, bloqueio (`lock`, bloqueio automático por
configuração), `closeAllGroups action` (`CloseAllEditorGroupsAction`, em `editorActions.ts`, também a portar).
Cobre: `editor basics`, `openEditors / closeEditors`, os 20 casos `closeEditor(s)`/`closeAllEditors`, `moveEditor`,
`moveEditors`, `copyEditor(s)`, `replaceEditors` (6 casos), `find editors` (2), `sticky editors` (2), `selection`,
`moveEditor with context`, `moveEditor disabled`, `onWillOpenEditor`, `copyEditor with context`, `transient editors` (3),
`locked groups`.
Dependências novas: `EditorPanes` (editorPanes.ts, só a escolha e abertura de painel, sem DOM), o título
(`EditorTitleControl`, só as decisões: aderentes e prévia), `EditorGroupModel` já existe.

### D. `EditorParts` e o `TestEditorParts`

`editorParts.ts` (1042 linhas) agrega a parte principal e as auxiliares; o teste usa `createEditorParts`,
`testMainPart` e `TestEditorPart` (`testSaveState`, `clearState`) de `workbenchTestServices`. Falta portar esse
helper em `test/browser/workbench_test_services.rs`, com a `EditorPart` de teste.

## Ordem proposta das fatias (cada uma compilável sozinha)

1. **FEITA.** Tipos do contrato (A): `common/editorGroupsService` completo em `services/editor/editor_groups_service.rs`, com os
   métodos novos do `IEditorGroup`/`IEditorGroupsService` como métodos com implementação padrão (sem quebrar os
   implementadores atuais). Divergências de assinatura existentes, mantidas e anotadas (o original completo é um método
   irmão com padrão): `closeEditor`/`moveEditor` sem `options` (`close_editor_with_options`, `move_editor_with_options`),
   `findGroup` sem `source`/`wrap` (`find_group_from`), `IModalEditorPart.close()` sem `options` e sem promessa
   (`close_with_options`), `createAuxiliaryEditorPart` devolve `IEditorPart` e não `IAuxiliaryEditorPart`, `addGroup` com
   `Option<GroupRef>`, e `IEditorGroupsContainer` não é trait (macro `editor_groups_container_members` nos dois traits).
   Esperado para a fatia 2: sobrescrever todos os padrões que lançam `Method not implemented.`.
2. **FEITA (escrita sem compilar, o integrador confere).** `EditorPart` lógico (B) em `browser/parts/editor/editor_part.rs` sobre
   `SerializableGrid`, com o `EditorGroupView` mínimo em `editor_group_view.rs` (modelo, identidade, índice, rótulo, vista do
   grid, contexto de chaves escopado) e `editor.rs` (`DEFAULT_EDITOR_PART_OPTIONS`, `getEditorPartOptions`,
   `impactsEditorPartOptions`, tamanhos mínimo e máximo do editor). Cobre: `getGroups` nas três ordens, `findGroup` por direção e
   por localização, `activateGroup` (e o motivo de ativação), `addGroup` (com cópia), `removeGroup`, `moveGroup`, `copyGroup`,
   `mergeGroup`, `mergeAllGroups`, `arrangeGroups` (maximizar, expandir, igualar), `toggleMaximizeGroup`, `toggleExpandGroup`,
   `getSize`, `setSize`, `setGroupOrientation`, `applyLayout`, `getLayout`, `centerLayout`, `enforcePartOptions`, as opções da
   parte, os eventos (`onDidAddGroup`, `onDidRemoveGroup`, `onDidMoveGroup`, `onDidActivateGroup`, `onDidChangeActiveGroup`,
   `onDidChangeGroupIndex`, `onDidChangeGroupLabel`, `onDidChangeGroupLocked`, `onDidChangeGroupMaximized`, `onDidLayout`),
   `createState`, `saveState`, o restauro do estado do memento (`create`) e `notifyGroupsLabelChange`. `close_editor` e
   `move_editor` do `IEditorGroup` passaram a ter padrão que lança (eram obrigatórios), porque o grupo mínimo ainda não os tem.
   Placar: 13 casos em `tests/editor_groups_service.rs` (`groups basics`, `groups index / labels`, `groups label`,
   `whenReady / whenRestored`, `options`, `find neighbour group (left/right)`, `(up/down)`, `find group by location`,
   `applyLayout (2x2)`, `getLayout`, `centeredLayout`, `maximize editor group`, `onDidActivateGroup carries activation reason`).
   Ficaram de fora: `sideGroup` (precisa de `openEditor` e do `findGroup` do serviço), `save & restore state` (abre editores e
   precisa do `TestEditorPart`), `copy/merge groups` e `merge all groups` (abrem editores). Pendentes na parte: `applyState`
   (conjuntos de trabalho, precisa de `closeAllEditors` e `openEditors`), o provider de context key, `handleContextKeys`
   (`common/contextkeys.ts`), o ouvinte de mudança externa do memento e o movimento entre partes (fatia 4).

3. **FEITA (escrita sem compilar, o integrador confere).** `EditorGroupView` completo e lógico (C) em
   `browser/parts/editor/editor_group_view.rs`, com o painel de editor substituído por um registro lógico do editor visível
   (`editor_panes.rs`: `EditorPanes`, `EditorPane`, `EditorPaneRegistry`; o painel guarda a entrada atribuída, as opções e o
   id registrado, sem DOM). Famílias, nesta ordem:
   - (a) abrir e ativar: `openEditor`, `openEditors`, `activeEditor`, `activeEditorPane`, `isActive`, `previewEditor`,
     `isPinned`, `onDidModelChange`, `onWillOpenEditor`, `onDidActiveEditorChange`, `onDidOpenEditorFail`, `whenRestored`,
     restauração do editor ativo ao copiar ou restaurar, auto bloqueio por `workbench.editor.autoLockGroups`;
   - (b) fechar: `closeEditor`, `closeEditors` (lista e filtro: `except`, `direction`, `savedOnly`, `excludeSticky`),
     `closeAllEditors`, editor sujo com `handleCloseConfirmation` (auto salvar, `closeHandler`, `ConfirmResult`), `CannotClose`
     e `force`, `onWillCloseEditor`, `onDidCloseEditor`, descarte de editor compartilhado só quando nenhum grupo o tem;
   - (c) mover e copiar entre grupos: `moveEditor`, `moveEditors`, `copyEditor`, `copyEditors` (com `canMove`, `onWillMoveEditor`
     e o estado de visão), e, no `EditorPart`, `copyGroup`, `mergeGroup`, `mergeAllGroups` (já existiam, agora com grupos que
     têm editores) e `sideGroup` (`findGroup` com `SIDE_GROUP` sobre o `IEditorGroupsService` do `InstantiationService`);
   - (d) `replaceEditors` (com `forceReplaceDirty` e índices), `findEditors`, `stickEditor` e `unstickEditor`, transientes
     (o foco limpa o transiente), seleção (`setSelection`, `isSelected`, `selectedEditors`) e `lock`.
   O `EditorGroupView` recebe a parte pelo trait `EditorGroupsView` (o `IEditorGroupsView` e o `IEditorPartsView` do original).
   `IEditorGroup` ganhou `onWillOpenEditor` e `IEditorPane` ganhou `getId`, ambos com padrão. Ficaram fora (documentado nos
   cabeçalhos): o título (`EditorTitleControl`), o menu, o telemetria, os context keys do grupo (`common/contextkeys.ts`),
   `hostService.focus`, o diálogo modal de erro ao abrir, o painel de "confiança necessária" e a busca de painel por
   `instanceof`. Placar: 62 dos 71 casos de `tests/editor_groups_service.rs` (os 13 da fatia 2 e 49 novos: `sideGroup`,
   `copy/merge groups`, `merge all groups`, `editor basics`, `openEditors / closeEditors`, `closeEditor` (3), `closeEditors` (10),
   `closeAllEditors` (5), `moveEditor` (3), `moveEditors`, `copyEditor`, `copyEditors`, `replaceEditors` (6), `find editors` (2),
   `sticky editors` (2), `selection`, `moveEditor with context`, `moveEditor disabled`, `onWillOpenEditor`,
   `copyEditor with context`, `locked groups` (3) e `transient editors` (3)). Fora: `save & restore state` (`TestEditorPart` e
   o serializador do editor de teste, fatia 4), `closeAllGroups action` (`CloseAllEditorGroupsAction` de `editorActions.ts`),
   `working sets` (3) e `context key provider` (4) (`applyState`, `EditorParts` e `common/contextkeys.ts`).
4. `EditorParts` e `TestEditorParts` (D), e o `ModalEditorPart` passa a estender a `EditorPart`.
5. Placar: ligar o teste em `crates/je-workbench/tests/editor_groups_service.rs` e acrescentar a linha em
   `docs/conformance.md`.

## Decisão sobre a primeira fatia

O grid pedido já existe e está com paridade de testes, então esta fatia não escreve código novo de grid. O próximo passo
útil é a fatia 1 acima.
