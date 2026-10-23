# Decisões de desenho do je-code

Decisões que valem para o porte inteiro. Cada uma diz o que o original faz, o que fazemos e por quê.

## Texto é UTF-16

O VS Code mede tudo em unidades UTF-16 (posição no editor, offset de settings, protocolo de extensão,
`String.length`). O porte usa `je_base::JsString` (`Vec<u16>`) e `&[u16]` onde índice importa. `&str` só
em API onde o resultado é o mesmo nas duas formas.

## Exceções viram pânico; erros esperados viram `Result`

No TypeScript, qualquer função pode lançar, e o VS Code tem dois tipos de erro: o inesperado (bug,
encaminhado ao `onUnexpectedError`) e o esperado (arquivo inexistente, cancelamento), que o chamador
trata. O porte separa os dois:

- **Inesperado**: `je_base::errors::throw(Error)` levanta um pânico com o valor `Error` dentro
  (`std::panic::panic_any`), e os pontos onde o VS Code tem `try/catch` em volta de código de terceiros
  (entrega de evento a listener, `dispose` em lote, tarefas do event loop) capturam com
  `catch_unwind` e entregam ao `onUnexpectedError`, como o original. O perfil do workspace mantém
  `panic = "unwind"`.
- **Esperado**: `Result<T, E>` com o tipo de erro do domínio (o `FileSystemProviderError` do original
  vira um `enum`). Onde o TypeScript usa `catch` para decidir fluxo, o porte usa `Result`.

## Grafo de objetos: `Rc`, `RefCell` e `Weak`, num único thread de UI

O workbench do VS Code é um grafo de objetos com coletor de lixo, num thread só (o renderer). O porte
mantém o thread único: serviços e widgets vivem em `Rc`, com mutabilidade interior (`Cell`/`RefCell`) e
`Weak` nas referências de volta (listener que aponta para o dono). Nada de `Arc<Mutex<_>>` no thread da
UI. Trabalho pesado (busca, tokenização, diff) vai para workers, como no VS Code (web workers e
processos), conversando por mensagens.

## Event loop com a ordem do JavaScript

O comportamento do VS Code depende da ordem do event loop: microtarefas (`Promise.then`,
`queueMicrotask`) rodam todas antes da próxima macrotarefa (`setTimeout`, I/O, input). O
`je_base::runtime` reproduz isso num thread: fila de microtarefas FIFO esvaziada depois de cada
macrotarefa, timers ordenados por (vencimento, ordem de criação), e um relógio injetável para os
testes que no original usam `runWithFakedTimers`. As promessas do porte (`je_base::async`) agendam as
continuações como microtarefas no momento em que se resolvem, como as do JavaScript.

## Injeção de dependência sem decorators (`je-instantiation`)

O VS Code monta tudo por `IInstantiationService`: serviços identificados por `createDecorator`, injetados
nos parâmetros decorados de um construtor, lidos por reflexão. Rust não tem nenhum dos dois. O porte
preserva a semântica observável (ordem de criação, criação preguiçosa, mensagem do erro de ciclo e do
serviço ausente, filhos que herdam e sobrescrevem, `dispose` na ordem do original) e troca o mecanismo:

- `ServiceId<dyn Trait>`: nome do `createDecorator` mais `TypeId`; serviço é `Rc<dyn Trait>`.
- Os parâmetros decorados viram o trait `Injectable`: `Args` (argumentos estáticos) e `Deps`, uma tupla de
  `Rc<dyn Trait>` na ordem dos parâmetros. A tupla é a única declaração das dependências e alimenta a
  resolução, o grafo de criação e a detecção de ciclo.
- Argumentos estáticos tipados: o descompasso que o original corrige com `console.trace` (e que o
  `StrictServiceCollection` rejeita) é erro de compilação.
- O `Proxy` da instanciação atrasada vira um tipo gerado por `delayed_proxy!` para cada trait atrasável,
  com as inscrições em eventos `onDid*`/`onWill*` guardadas até o serviço nascer.
- Serviço que guarda `IInstantiationService` aponta de volta para a coleção que o guarda, um ciclo de `Rc`
  que o coletor do JavaScript desfaz sozinho. `dispose` solta da coleção as instâncias que o serviço de
  instanciação criou, e filho precisa de `dispose`, como o original já pede.

Detalhes e divergências no topo de `crates/je-instantiation/src/lib.rs`.

## Opções do editor: chaves tipadas

`editorOptions.ts` (6 989 linhas, 53 classes de opção) guarda cada opção como um valor dinâmico indexado por
`EditorOption`. O porte troca o índice solto por uma **chave tipada**: `Opt<T>` carrega o id da opção e o tipo do
valor calculado (`pub const READ_ONLY: Opt<bool>`, `pub const MINIMAP: Opt<InternalMinimapOptions>`), e
`ComputedEditorOptions::get(Opt<T>) -> &T` devolve o valor já com o tipo, guardado como `Rc<dyn Any>`
(`downcast_ref`, sem `unsafe`). Cada opção é uma struct que implementa `EditorOptionImpl` (`id`, `name`,
`default_value`, `validate`, `compute`, `schema`) como as classes `SimpleEditorOption`, `BaseEditorOption` e
`ComputedEditorOption` do original; a tabela `EDITOR_OPTIONS_REGISTRY` mantém a ordem do enum `EditorOption`.
Os `IEditorOptions` (entrada, todos os campos opcionais) viram structs com `Option<T>` por campo. O esquema JSON
de configuração (`editorConfigurationSchema.ts`) sai da mesma tabela, para `settings.json` aceitar exatamente as
mesmas chaves e os mesmos valores do VS Code.
