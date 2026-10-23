# Reaproveitamento do pseudo-linus

Regra do user (2026-10-23): antes de portar qualquer coisa ou escolher dependência, conferir se o
pseudo-linus (`ref/pseudo-linus`, commit `9e3d0767` de 2026-10-12) já tem o porte, e reutilizar de lá.
O caminho padrão é a cópia versionada da crate para `crates/` com a origem (repositório, commit,
licença) no README dela, como o `zbun` faz com o bun; mudança local fica listada no README.

| Necessidade do je-code | Crate do pseudo-linus | Uso previsto | Estado |
| --- | --- | --- | --- |
| `RegExp` ECMAScript (glob, busca, find widget, `createRegExp`, snippets) | `zjsc/src/yarr` (YARR do JavaScriptCore, 25 915 linhas; depende de ~10 itens do `wtf` e de uma opção da `VM`) | extrair como `crates/zyarr` | em extração |
| Extension host: motor JS | `zjsc` (98,6% do test262, vigente 2026-10-12) | Fase 5 | a integrar |
| Transpilar TypeScript para JS (extensões e scripts) | `zbun` (transpilador do bun 1.4.2) | Fase 5 | a avaliar |
| Intl (collation do `comparers.ts`, `Intl.Segmenter`, datas) | `zjsc` usa `icu_collator`, `icu_segmenter`, `jiff` (crates Rust) | usar as mesmas crates ICU4X, nas mesmas versões | a medir |
| Rasterização e shaping idênticos ao Linux do oráculo (Chromium usa FreeType e HarfBuzz) | `zft` (FreeType 2.13.3), `zhb` (HarfBuzz 10.2.0) | comparar com parley/skrifa na H10 e H11 | a medir |
| Webviews de extensão (HTML de terceiros) | `ul-chromium` (DOM, Stylo, Blitz, zjsc) | Fase 6, só dentro da webview; a UI continua nativa | a avaliar |
| Armazenamento das webviews e do Chromium no user data (`Local Storage`, `IndexedDB`) | `ul-leveldb` (formato do leveldb do Chromium), `ul-chromium/src/idb` | retrocompatibilidade de user data | a avaliar |
| VSIX (zip), descompactação de extensões e de update | `ul-archive` (zip, gzip, tar, xz, zstd), `zdeflate` | Fase 5 | a extrair |
| Imagens no editor (preview, ícones de extensão) | `zjpeg`, `ul-chromium/src/webp` | Fase 6 | a extrair |
| Git embutido quando não há `git` no PATH (web, máquinas sem git) | `ul-git` (repositório, índice, refs, worktree) | Fase 6, fallback; com `git` presente, a extensão git usa o binário como no VS Code | a avaliar |
| Cliente HTTP/HTTPS com rustls | `ul-net` | marketplace, update, settings sync | a avaliar |
| Terminal no alvo web (sem pty no navegador) | `shell`, `kernel`, `vfs` e o userland | terminal web rodando o sandbox | ideia, não medida |
| Sistema de arquivos em memória no alvo web | `vfs` (tmpfs persistente com snapshot O(1)) | provider de arquivos da web | ideia, não medida |

## O que não se reaproveita, e por quê

- Módulos do Node no zjsc (`path`, `url`, `util`, `stream`): são o JS do bun rodando dentro do motor,
  não API Rust; servem ao extension host, não ao `vs/base` em Rust.
- `ul-sqlite`: usa `rusqlite` (C). O storage do je-code usa `turso_core` (H12).
- `regex-posix`: semântica POSIX; o VS Code usa ECMAScript (YARR) e Oniguruma (TextMate).
- `ul-textproc` (grep do GNU): a busca do VS Code é o ripgrep; a paridade vem das crates do ripgrep.
