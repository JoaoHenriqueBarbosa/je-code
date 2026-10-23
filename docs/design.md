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
