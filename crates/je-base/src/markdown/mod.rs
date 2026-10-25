//! Porte de `src/vs/base/browser/markdownRenderer.ts` (parte 1) e de `src/vs/base/browser/domSanitize.ts`.
//!
//! Origem do parser: o `marked` v14 já está portado em [`crate::marked`] (lexer, tokenizer, gramáticas, `Parser` e
//! `Renderer`, com as mesmas regexes no motor `zyarr`). Esta fatia não reescreve o parser: só abriu nele o
//! `Parser::with_renderer`, os ganchos de bloco do [`crate::marked::InlineRenderer`] (`code`, `blockquote`,
//! `html` de bloco, `paragraph`, o que o original troca em `renderer.*`) e o `Lexer::with_links`.
//!
//! Árvore nativa: o Markdown vira HTML só como texto intermediário (é o formato que o `marked`, o gancho de
//! ícones de tema e o sanitizador do original trocam entre si); ele é lido pelo `html_parser` em uma árvore de
//! [`Element`](crate::browser::html_parser::Element) e [`Node`](crate::browser::html_parser::Node), sanitizada e
//! reescrita. Nada é pintado como HTML nem passa por DOM, CSS ou stylo: a árvore é o dado que o je-widgets
//! mapeia para widgets nativos.
//!
//! Módulos:
//! - [`sanitize`]: `domSanitize.ts` (tags e atributos permitidos, protocolos de link e de mídia, `replaceWithPlaintext`);
//! - [`incomplete`]: `fillInIncompleteTokens`;
//! - [`render`]: `renderMarkdown`, o tratamento de links e imagens (`transformUri`, `baseUri`, `uris`, `isTrusted`,
//!   `actionHandler`), `supportHtml`, `supportThemeIcons`, `supportAlertSyntax`, `codeBlockRendererSync`.
//!
//! Fatia 2: `codeBlockRenderer` assíncrono, `asyncRenderCallback`, `target` (`render_markdown_into`) e os 127 casos de
//! `base/test/browser/markdownRenderer.test.ts` (em `tests/markdown_renderer.rs`). Ficam de fora de propósito:
//! - os ouvintes de clique e teclado de `renderMarkdown`: existe [`render::RenderedMarkdown::activate_link`], que o widget
//!   chama com o `data-href` do `a` clicado (Space e Enter, botão esquerdo ou do meio), e o ouvinte de `load` das imagens;
//! - `markedExtensions` (o `marked` portado não tem extensões);
//! - o mapeamento da árvore para os widgets do je-ui e do je-widgets, com os codicons.

pub mod dom;
pub mod incomplete;
pub mod render;
pub mod sanitize;

pub(crate) mod re;

pub use incomplete::fill_in_incomplete_tokens;
pub use render::{
    MarkdownActionHandler, MarkdownEnvironment, MarkdownLinkKind, MarkdownRenderOptions, MarkdownRendererMarkedOptions, MarkdownSanitizerConfig,
    RenderedMarkdown, allowed_markdown_html_attributes, allowed_markdown_html_tags, render_markdown, render_markdown_into,
};
