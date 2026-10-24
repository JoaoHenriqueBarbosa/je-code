# Extension host sobre o zjsc: o que falta, medido

Fonte de todos os números: **fork**, 2026-10-23, `testbench/exthost-probe` (hipóteses H15 e H16 do
`docs/bench-report.md`). O zjsc é o do pseudo-linus no commit `9e3d0767`. Oráculo: Node 24.21.0 do
Electron do VS Code em `ref/oracle`, rodado com `ELECTRON_RUN_AS_NODE=1`.

## O zjsc é separável

- `zjsc` depende de `ul-common` (relógios, `js_number`, `libm`, hash, `file_url`, fusos) e este do `sysabi`
  (tipos de errno, sinais, `Clock`). Os usos do `sysabi` no `ul-common` são só `try_current()`: sem sandbox
  instalado, `realtime_nanos`, `monotonic_nanos` e `sleep` caem em `std::time` e `std::thread::sleep`.
  O `sysabi` tem um `build.rs` que lê `testbench/golden/linux-facts/linux_facts.json` do próprio pseudo-linus
  (presente em `ref/`).
- Toda a E/S do motor passa pelo trait `zjsc::runtime::host::Host` (arquivos, ambiente, cwd, pid, entropia,
  `fs_*`, rede de loopback, `proc_spawn`), instalado por thread com `host::install`, mais `ModuleFs` e
  `ConsoleHost` para o carregador de módulos e o console. O `tests/host_boundary.rs` do zjsc garante que o
  `src/` não toca `std::fs`. O `Host` sobre o SO real existe só em teste (`tests/common/std_host.rs`, que
  chama `kill` e `renice` por processo externo); o da sonda (`testbench/exthost-probe/src/host.rs`) tem
  240 linhas, cobre `fs_*` com descritores virtuais (tabela de `std::fs::File`, sem `unsafe`) e não implementa
  rede, `proc_spawn`, `proc_wait`, `fs_utimes`, `fs_chown`, `fs_statfs` (devolvem `ENOSYS`).
- Compila no workspace do je-code por caminho, com duas mudanças locais, ambas fora do git porque `ref/` é
  ignorado: (1) `exclude = ["ref"]` no `Cargo.toml` raiz (versionado); (2) em
  `ref/pseudo-linus/crates/zjsc/Cargo.toml`, o `icu_collator` vendorizado aponta para o `vendor/icu_collator`
  do je-code (`../../../../vendor/icu_collator`), porque as duas cópias da mesma versão colidem no
  `Cargo.lock`. Esta segunda JÁ virou cópia versionada em `crates/zjsc` (2026-10-24, eu; lista de mudanças no README
  da crate), e a sonda aponta para ela.
- Dimensão: 600 736 linhas de Rust em `zjsc/src`; build de release em 2 min 57 s com `CARGO_BUILD_JOBS=4`
  (opt-level 3, `debug = 0` só para o zjsc); binário de 92 MB com `debug = 1` no resto.
- A árvore de dependências do zjsc tem 224 pacotes e nenhum crate de fonte C da lista do
  `scripts/pure-rust-check.sh` (`cc`, `*-sys`, `ring`, `openssl-sys` não aparecem; só `libc`). A varredura
  completa do script NÃO rodou: ele cobre `crates/*` e falhou antes, por erros de compilação em
  `je-base` (`json_rpc_protocol.rs`) que já estavam no trabalho em andamento.

## O que funciona (medido)

- Carregar um bundle de extensão CommonJS (esbuild), ativar, registrar comandos e `subscriptions`: 19 de 34
  extensões embutidas com `main` ativam, as mesmas 19 que no Node do oráculo.
- Avaliar o `extensionHostProcess.js` inteiro (2,3 MB, 61 imports) sem erro de sintaxe, com `import` reescrito
  para `require`.
- Carregar o `typescript.js` 6.0.3 e responder completion e diagnóstico semântico corretos (as mesmas 22 e 29
  entradas que o Node, com `map`, `filter` e `length`).

## O que falta no zjsc e na compat Node (lista medida)

Módulos `node:*` que o host e as extensões pedem e o zjsc não entrega (`js/modules-check.js`, 33 nomes
testados): `v8`, `worker_threads`, `readline`, `perf_hooks`, `inspector`. Entregam: `assert`, `child_process`,
`crypto`, `events`, `fs`, `fs/promises`, `http`, `https`, `module`, `net`, `os`, `path`, `stream`,
`string_decoder`, `tls`, `url`, `util`, `vm`, `zlib`, `timers`, `tty`, `dns`, `buffer`, `querystring`,
`async_hooks`, `http2`, `diagnostics_channel`, `process`.

API faltando dentro de módulo que existe: `node:module.createRequire` e `node:module._nodeModulePaths`
(o `extensionHostProcess.js` chama `createRequire(import.meta.url)` 4 vezes e `_nodeModulePaths` no
carregador de extensões); `import.meta.url` e `import.meta.dirname` no ESM do host (a sonda os reescreve por
texto; não medi se o `import.meta` do zjsc os entrega).

Bloqueios por extensão atribuíveis ao zjsc (não ao `vscode` falso): `css-`, `html-` e
`json-language-features` (`readline`), `ms-vscode.vscode-js-profile-table` (`worker_threads`).

Pacotes que o host importa e o oráculo guarda em `node_modules.asar` (não existem em disco, precisam ser
lidos do asar ou vendorizados): `minimist`, `vscode-regexpp`, `@vscode/proxy-agent`, `@vscode/fs-copyfile`,
`esprima`, `electron`, `vsda`.

Host (`zjsc::runtime::host::Host`) sem implementação real: rede, `proc_spawn`, `proc_wait`, `fs_utimes`,
`fs_chown`, `fs_statfs`, `kill`, `set_priority`, TTY. O `tsserver` e o `git` precisam de `proc_spawn`; o
host de extensões do VS Code fala com o workbench por socket ou pipe nomeado (`net` ou `fs`), então rede de
loopback e pipe do SO não são opcionais.

## O que falta do lado do `vscode` (a API de verdade)

A sonda usa um `vscode` falso que registra o que a extensão toca. Das 34 extensões, 15 não ativam nem no
Node por causa dele; o que pediram e faltou (todos os nomes saem do `vscode.d.ts`, 202 de primeiro nível):
`env.createTelemetryLogger`, `languages.*` (provedores), `workspace.registerTextDocumentContentProvider`,
`workspace.onDidOpenTextDocument` e `onDidChangeTextDocument`, `window.createTextEditorDecorationType`,
`window.onDidChangeVisibleTextEditors`, `ThemeColor`, `OverviewRulerLane`, `Uri.joinPath`,
`ExtensionContext.secrets.onDidChange`, `workspace.notebookDocuments`, `extensions.allAcrossExtensionHosts`,
`TerminalCompletionItemKind`. O `vscode` real é o `extHost.api.impl.ts` do oráculo mais o protocolo RPC com o
workbench (`MainThreadXxx` e `ExtHostXxx`), que o `extensionHostProcess.js` já contém: reutilizá-lo no zjsc
evita reescrever a API, ao custo de fazer o resto do bundle (RPC, `net`, `readline`, `v8`) rodar.

## Desempenho (a razão do bloqueio)

| Operação | zjsc | Node do oráculo | Razão |
| --- | --- | --- | --- |
| Carregar `typescript.js` (9,1 MB, parse + eval) | 407 ms | 139 ms | 2,9 |
| Compilar `extensionHostProcess.js` (2,3 MB), 1a vez | 70 a 75 ms | 59 ms | 1,2 |
| Avaliar `extensionHostProcess.js` | 366 a 443 ms | 31 a 35 ms | cerca de 12 |
| Primeira completion, `lib` mínima | 3 925 ms | 120 ms | 33 |
| Primeira completion, `lib` padrão (com `lib.dom.d.ts`) | 38 471 ms | 340 ms | 113 |
| `createSourceFile` do `lib.dom.d.ts` (2,3 MB) | 30 763 ms | 251 ms | 123 |
| Completion com `lib` em cache, `lib` padrão | 91 a 96 ms | 9 a 14 ms | cerca de 10 |
| `fib(32)` | 4,4 a 4,5 s | 80 ms | 56 |
| `richards` | 12,2 a 14,2 s | 63 a 65 ms | cerca de 200 |
| Soma de avaliação das 19 extensões que ativam | 835 ms | 97 ms | 8,6 |
| Pico de memória, completion com `lib` mínima | 211 MB | 136 MB | 1,5 |

Onde o tempo vai (do `perf-baseline.md` do pseudo-linus, vigente 2026-10-11, `callgrind`): laço do
interpretador 21 a 28%, registro de células (`RefCell<Registry>`) 6 a 22%, decodificação de operandos 10 a
14%, SipHash em escopo e propriedades 4 a 7%, resolução de escopo 9%. Não reperfilei aqui.

## Lacunas de medida (NÃO MEDIDO)

- O `tsserver` real por IPC (`_tsserver.js` pára em `Cannot find module 'readline'`).
- Um projeto médio de verdade (a medida foi um arquivo virtual de 3 linhas contra a `lib` do TypeScript).
- Extensões ESM (`github`) e as instaladas pelo user; as demais embutidas (97 diretórios em `extensions`, 34 com `main` de Node) são tema, gramática e snippet ou ESM.
- O startup completo do `extensionHostProcess.js` (a sonda corta a chamada final de inicialização).
- Ganho de um cache de AST e bytecode do `lib` entre sessões, e de otimizações no zjsc (registro de células,
  decodificação de operandos).

## Restrição que muda as saídas da H16 (eu, 2026-10-23)

A H16 previa "um JIT de linha de base em Rust safe" como saída. Isso não existe sob `unsafe_code = "forbid"`:
executar código de máquina gerado exige `unsafe` (mapear página executável e saltar para ela), e uma dependência
que fizesse isso por nós quebraria a regra de só Rust sem unsafe no caminho de execução do produto. As saídas que
sobram, em ordem de custo:

1. Cache de AST e bytecode do `lib.*.d.ts` entre sessões (o parse do `lib.dom.d.ts` é 30,8 s do zjsc contra 251 ms
   do Node; com a lib em cache a completion cai a 91 ms).
2. Otimizar o interpretador do zjsc (o perfil do pseudo-linus: registro de células 22%, operandos 10 a 14%, sem
   cache inline 13 a 17%, SipHash 4 a 7%; ganho estimado 2 a 3 vezes pelo fork, não medido por mim).
3. Servidor de TypeScript nativo em Rust: não existe um maduro (o `tsgo` da Microsoft é Go).
4. Rodar o `tsserver` fora do produto (processo do Node do sistema, quando existir), perdendo a regra de só Rust
   nesse processo; decisão do user, não minha.

## Decisão de arquitetura (integrador, 2026-10-24): o extension host é Rust, a API `vscode` é exposta ao zjsc

O `extensionHostProcess.js` (o bundle do oráculo) NÃO entra no produto: é JavaScript da Microsoft e a regra do
projeto é só Rust. O host de extensões é portado para Rust (`crates/je-workbench/src/api/`: `ext_host_*.rs`, o porte
dos `extHost*.ts` e dos conversores) e o zjsc roda apenas o código das EXTENSÕES, que enxerga um objeto `vscode`
construído em Rust. Consequências já decididas para o porte:

- **Valores que cruzam a fronteira** (argumentos de comando, resultados de provedores, opções) são `Dynamic`. As
  classes da API (`Position`, `Range`, `Selection`, `Location`, `Uri`, `TextEdit`, `ThemeColor`...) cruzam como
  `Dynamic::Object` com a propriedade reservada `$$class` com o nome da classe, que é o equivalente de `instanceof`
  (`class_of(&Dynamic) == Some("Position")`). `ext_host_values.rs` guarda os auxiliares (`tag`, `class_of`,
  `is_instance_of`) e cada tipo de `ext_host_types` ganha `to_dynamic`/`from_dynamic`. A ponte com o zjsc, quando
  existir, cria o objeto JS com o protótipo do registro de classes e preserva `$$class`.
- **`cloneAndChange`/`revive`/`MarshalledId`** do original operam sobre esse `Dynamic` (não sobre structs), como o
  original opera sobre `any`.
- **RPC**: o protocolo `MainThreadXxx`/`ExtHostXxx` é o trait por proxy de `ext_host_protocol.rs`; o transporte (socket
  ou pipe, `net`/`fs` do `Host` do zjsc) vem na Fase 5, depois dos testes do núcleo.
- **Ponte JS**: cada membro do `vscode.d.ts` (202 de primeiro nível) é um binding do zjsc para a função Rust
  correspondente; a lista de nomes que as extensões embutidas pediram e faltaram (seção acima) é a fila.
