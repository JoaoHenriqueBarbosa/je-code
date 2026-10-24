# jschardet

Porte para Rust seguro (`unsafe_code = "forbid"`) do `jschardet` 3.1.4, o detector de codificação que o
VS Code usa em `guessEncodingByBuffer` (`src/vs/workbench/services/textfile/common/encoding.ts`).

## Origem

- Pacote npm `jschardet` 3.1.4 (autor António Afonso), extraído em `ref/jschardet-src/package`.
- Repositório do original: <https://github.com/aadsm/jschardet>. O `jschardet` é um porte do `chardet` 2.0.1
  do Python (Mark Pilgrim), que por sua vez porta o detector universal de codificação da Mozilla (Shy Shalom).
- Licença: LGPL-2.1 ou posterior (`LGPL-2.1+` no `package.json`). O cabeçalho de cada fonte original fala
  de "GNU Lesser General Public License, version 2.1 or (at your option) any later version".

## Estrutura

Um arquivo `.rs` por arquivo `.js` de `src/`, com os mesmos nomes (`universaldetector.rs`,
`mbcsgroupprober.rs`, `big5freq.rs`, `mbcssm/utf8.rs`, ...). O `index.js` é o `lib.rs`.

## API

```rust
pub fn detect(bytes: &[u8], options: Option<&DetectOptions>) -> Option<Detected>;
pub fn detect_all(bytes: &[u8], options: Option<&DetectOptions>) -> Vec<Detected>;
pub fn try_detect(bytes: &[u8], options: Option<&DetectOptions>) -> Result<Option<Detected>, Error>;
pub fn try_detect_all(bytes: &[u8], options: Option<&DetectOptions>) -> Result<Vec<Detected>, Error>;
```

`DetectOptions { detect_encodings, min_confidence }` é o `{ detectEncodings, minimumThreshold }` do original.

## Mudanças locais

- A entrada é `&[u8]` (um byte por posição), no lugar da "binary string" (um caractere por byte) do original.
- `detect` devolve `None` quando o `encoding` do original seria `null`. O original lança uma exceção nos
  casos abaixo; `try_detect` e `try_detect_all` devolvem esses casos como [`Error`], e `detect` e
  `detect_all` os tratam como "nada detectado":
  - codificação fora da lista suportada em `detectEncodings` (`Error::UnsupportedEncoding`);
  - o `TypeError` do original em `jpcntx.js` quando a ordem de um hiragana Shift_JIS passa de 82 e a tabela
    `jp2CharContext` não tem a linha (`Error::HiraganaOrderOutOfRange`).
- O `logger.js` (`enableDebug`, `setLogger`) não existe: o log padrão do original é um no-op e nenhum
  resultado depende dele.
- Os probers que se referenciam em ciclo (`HebrewProber` e os dois `SingleByteCharSetProber` do hebraico)
  se ligam por `Rc<RefCell<..>>` e `Weak`; o resto do desenho (um objeto por prober, `feed`, `reset`,
  `getConfidence`) é o do original. Os campos que o original guarda mas nunca lê
  (`_mCurrentBytePos`, `_mStart`, `_mDone` do analisador de distribuição, `_mTableSize` fora do `getOrder`)
  não existem.
- Nas tabelas de frequência de ordem `big5freq`, `euckrfreq`, `gb2312freq` e `jisfreq` o array do original
  é mais comprido do que `*_TABLE_SIZE` (por exemplo, 13 973 entradas no Big5 contra 5 376), mas todo acesso
  é guardado por `order < _mTableSize`: a cauda é inalcançável e foi omitida. O prefixo até `*_TABLE_SIZE`
  está inteiro, na mesma ordem. O `euctwfreq` é o contrário: o array tem 6 016 entradas e o
  `EUCTW_TABLE_SIZE` é 8 102, então o original lê `undefined` entre as duas medidas e `512 > undefined` é
  falso; o porte guarda as 6 016 entradas inteiras e lê com `get`, que devolve `None` no mesmo caso.
- `jschardet.detect` com `Buffer` converte para `binary`; aqui não há essa conversão porque a entrada já são bytes.
