# je-code: regras do projeto

Porte do VS Code inteiro para Rust. Compila para Windows, Linux (inclusive dentro do WSLg), macOS e web.
Paridade completa e retrocompatível com o VS Code (extensões, settings, keybindings, temas, perfis e user data).
60 fps na GPU quando houver, no mínimo 30 fps na CPU.

## Só Rust, sem unsafe

- Todo código nosso compila sob `unsafe_code = "forbid"` (lint do workspace). Nunca `unsafe impl`, nunca
  `#[allow(unsafe_code)]`.
- Nenhuma outra linguagem no produto: nada de C, C++, JS, TS, WGSL fora do que o wgpu já carrega, DSL
  própria. Dependência pode ter `unsafe` interno se a API usada for segura, mas dependência que compila C
  ou liga biblioteca C (`cc`, `cmake`, `*-sys` com fonte C, `ring`, `aws-lc`, `openssl`, `libsqlite3-sys`,
  `onig`) está proibida. `scripts/pure-rust-check.sh` confere o lockfile; falha dele é bloqueio.

## Reutilizar o pseudo-linus antes de portar

Antes de portar um módulo ou escolher uma dependência, conferir `docs/reuse-pseudo-linus.md` e
procurar em `ref/pseudo-linus/crates` (zjsc com o YARR, zbun, zft, zhb, ul-archive, ul-git, ul-net...).
Se já existe porte lá, a crate é copiada para `crates/` com a origem (repositório, commit, licença) no
README dela, e a mudança local fica listada ali. Escrever de novo o que já existe é retrabalho.

## Renderização nativa, nunca tecnologia web

A UI é uma árvore de widgets nativa em Rust desenhada por wgpu na GPU e por rasterizador em software na
CPU. Nada de DOM, CSS, HTML, stylo ou blitz no caminho de pintura. Os `.css` e `.html` do VS Code em
`ref/vscode` são ESPECIFICAÇÃO (medidas, cores, espaçamento, estados), lidos ao portar cada widget, nunca
código executado. Não repetir o erro do Electron.

## Medido, não assumido

Toda premissa de arquitetura vira hipótese numerada em `docs/bench-report.md`, com experimento em
`testbench/` e veredito (confirmada, refutada, parcial). A escolha segue a evidência, não a preferência.
Número sem fonte não entra em documento: cada um leva **eu** (medido aqui), **fork** (reportado) ou
**vigente** (herdado de arquivo versionado, com data). Critério nunca medido aparece como NÃO MEDIDO.

## Oráculo

Fidelidade é placar, não afirmação. O oráculo é o VS Code real: o binário estável em `ref/oracle/`, a
fonte em `ref/vscode` e a suíte de testes dele (cerca de 50 mil casos mocha), portada caso a caso. Um
comportamento só conta como pronto quando bate com o oráculo. Se o oráculo não cobre algo, melhora-se o
oráculo; nunca se exclui o caso.

## Roadmap vivo

`roadmap.md` tem as fases e os critérios de saída. `wip/notes/roadmap-status.md` tem o estado medido de
cada critério e é conferido e atualizado a cada integração. Cada hora de trabalho avança o critério de
saída da fase corrente; polimento fora da fase só quando um caso da fase tropeça.

## Repositório público

Nada da máquina de quem desenvolve entra em fixture, golden ou documento versionado: caminhos do home,
nome de usuário, chaves. `ref/` e `target/` ficam fora do git.

## DRY

Código parecido não se repete; função de repasse (corpo que só chama outra com os mesmos argumentos) não
existe. Quando a repetição aparecer, o caminho é dar nome ao conceito e extrair, fundir com a diferença
virando parâmetro, ou mudar o tipo para a repetição deixar de ser possível.
