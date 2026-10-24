# Avisos de terceiros

O `zjsc` embute código derivado do Bun (`oven-sh/bun`, tag `bun-v1.4.2`, commit
`744846f844374847c902b5e7fd59b4342a51ef99`), licenciado sob a MIT. O `LICENSE.md` desta pasta é o do upstream, copiado
sem alteração. O upstream não traz linha de copyright própria; o crédito é dos autores do Bun (`oven-sh/bun`).

| Arquivo | Origem no upstream |
|---|---|
| `src/api/bun_wrap.js` | `src/runtime.js`, copiado sem alteração (o módulo interno `bun:wrap`) |
| `src/api/node_events.js` | `src/js/node/events.ts`, portado para JavaScript comum |
| `src/api/node_async_hooks.js` | `src/js/node/async_hooks.ts`, portado para JavaScript comum |
| `src/api/node_timers.js` | `src/js/node/timers.ts`, portado para JavaScript comum |
| `src/api/node_timers_promises.js` | `src/js/node/timers.promises.ts`, portado para JavaScript comum |
| `src/api/node_querystring.js` | `src/js/node/querystring.ts`, portado para JavaScript comum |
| `src/api/node_stream_modules.js` | gerado por `scripts/gen-node-stream.js` a partir dos fontes do `stream` do upstream |
| `src/api/node_fs_streams.js` | gerado por `scripts/gen-node-fs-streams.js` a partir de `src/js/internal/fs/streams.ts` |
| `src/runtime/node_fs.js` | escrito para o porte sobre o comportamento medido do `node:fs` do bun; o `FileHandle` segue `src/js/node/fs.promises.ts` |
| `src/api/node_fs_cp.js` | gerado por `scripts/gen-node-fs-cp.js` a partir de `src/js/internal/fs/cp-sync.ts` e `cp.ts`; o epílogo (`scripts/node-fs-cp-epilogue.js`) segue os despachantes de `src/js/node/fs.ts` e `fs.promises.ts` |
| `src/api/node_fs_glob.js` | gerado por `scripts/gen-node-fs-glob.js` a partir de `src/js/internal/fs/glob.ts` (que embute o minimatch do Node, Blue Oak Model License 1.0.0, sem alteração); o epílogo segue `fs.ts` e `fs.promises.ts` |
| `src/api/node_fs_watch.js` | gerado por `scripts/gen-node-fs-watch.js` a partir de `src/js/internal/fs/watch.ts` e `watchfile.ts`; o epílogo segue o `watch` de `src/js/node/fs.promises.ts`; o observador nativo (`scripts/node-fs-watch-prelude.js`) é escrito para o porte |
| `src/api/node_fs_utf8.js` | gerado por `scripts/gen-node-fs-utf8.js` a partir de `src/js/internal/streams/fast-utf8-stream.ts` (portado do Node, derivado do SonicBoom, MIT, Copyright (c) 2017 Matteo Collina) |
| `src/api/node_url.js` | gerado por `scripts/gen-node-url.js` a partir de `src/js/node/url.ts` e `src/js/internal/url.ts` |
| `src/api/node_util.js` | gerado por `scripts/gen-node-util.js` a partir de `src/js/node/util.ts` e dos internos que ele usa |
| `src/api/node_assert.js` | gerado por `scripts/gen-node-assert.js` a partir de `src/js/node/assert.ts` e dos internos que ele usa |
| `src/api/node_internal.js` | reescrita sobre o `internal/errors`, os validadores e o `internal/primordials` do upstream |

## JavaScriptCore (WebKit)

| Arquivo | Origem no upstream |
|---|---|
| `src/runtime/builtins_combined.js` | gerado por `scripts/gen-builtins.py` a partir de `derived/JavaScriptCore/JSCBuiltins.cpp`, a saída do gerador do próprio JavaScriptCore sobre `upstream/JavaScriptCore/builtins/*.js` (WebKit) |

Os fontes de `upstream/JavaScriptCore/builtins/*.js` trazem, cada um, o cabeçalho de licença BSD de duas cláusulas abaixo; o `builtins_combined.js` é a concatenação dos corpos das funções desses arquivos, e a licença continua valendo para ele. Os copyrights, como o upstream os traz:

- Copyright (C) 2014 Apple Inc. All rights reserved.
- Copyright (C) 2014-2017 Apple Inc. All rights reserved.
- Copyright (C) 2015 Apple Inc. All rights reserved.
- Copyright (C) 2015 Jordan Harband. All rights reserved.
- Copyright (C) 2015 Yusuke Suzuki <utatane.tea@gmail.com>.
- Copyright (C) 2015, 2016 Apple Inc. All rights reserved.
- Copyright (C) 2015-2016 Apple Inc. All rights reserved.
- Copyright (C) 2015-2016 Yusuke Suzuki <utatane.tea@gmail.com>.
- Copyright (C) 2015-2022 Apple Inc. All rights reserved.
- Copyright (C) 2016 Apple Inc. All rights reserved.
- Copyright (C) 2016 Oleksandr Skachkov <gskachkov@gmail.com>.
- Copyright (C) 2016 Yusuke Suzuki <utatane.tea@gmail.com>.
- Copyright (C) 2016-2017 Yusuke Suzuki <utatane.tea@gmail.com>.
- Copyright (C) 2016-2024 Apple Inc. All rights reserved.
- Copyright (C) 2018 Yusuke Suzuki <yusukesuzuki@slowstart.org>.
- Copyright (C) 2021 Igalia S.L.
- Copyright (C) 2022 Apple Inc. All rights reserved.
- Copyright (C) 2022 Jarred Sumner. All rights reserved.
- Copyright (C) 2023 Devin Rousso <webkit@devinrousso.com>.
- Copyright (C) 2024 Apple Inc. All rights reserved.
- Copyright (C) 2024 Sosuke Suzuki <aosukeke@gmail.com>.
- Copyright (C) 2025 Sosuke Suzuki <aosukeke@gmail.com>.

Condições, copiadas do cabeçalho de `upstream/JavaScriptCore/builtins/PromiseConstructor.js` sem alteração (um dos arquivos traz a variante "APPLE INC. AND ITS CONTRIBUTORS"):

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY APPLE INC. ``AS IS'' AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL APPLE INC. OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY
OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 
```

## minimatch (embutido em `src/api/node_fs_glob.js`)

O `glob.ts` do bun embute, sem alteração, o `deps/minimatch/index.js` do Node (minimatch 10.2.5, de Isaac Z. Schlueter, campo `license` do `package.json`: `BlueOak-1.0.0`; o comentário do `glob.ts` diz ISC, mas o repositório do Node diz Blue Oak). Origem: `nodejs/node`, commit `50c35fea9e64d50ab3bb5f359e8523de89d6c798`, arquivo `deps/minimatch/LICENSE.md`, copiado sem alteração (o arquivo não traz linha de copyright):

```
# Blue Oak Model License

Version 1.0.0

## Purpose

This license gives everyone as much permission to work with
this software as possible, while protecting contributors
from liability.

## Acceptance

In order to receive this license, you must agree to its
rules. The rules of this license are both obligations
under that agreement and conditions to your license.
You must not do anything with this software that triggers
a rule that you cannot or will not follow.

## Copyright

Each contributor licenses you to do everything with this
software that would otherwise infringe that contributor's
copyright in it.

## Notices

You must ensure that everyone who gets a copy of
any part of this software from you, with or without
changes, also gets the text of this license or a link to
<https://blueoakcouncil.org/license/1.0.0>.

## Excuse

If anyone notifies you in writing that you have not
complied with [Notices](#notices), you can keep your
license by taking all practical steps to comply within 30
days after the notice. If you do not do so, your license
ends immediately.

## Patent

Each contributor licenses you to do everything with this
software that would otherwise infringe any patent claims
they can license or become able to license.

## Reliability

No contributor can revoke this license.

## No Liability

**_As far as the law allows, this software comes as is,
without any warranty or condition, and no contributor
will be liable to anyone for any damages related to this
software or this license, under any kind of legal claim._**
```
