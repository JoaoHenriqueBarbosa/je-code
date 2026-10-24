// `node:string_decoder` do bun 1.4.2. O bun implementa em C++; aqui o decodificador é JavaScript com o estado em campos
// privados, e a superfície (nenhuma chave própria; `lastChar`, `lastNeed`, `lastTotal` e `encoding` como acessores do
// protótipo, depois `write`, `end`, `text`) é a medida no bun.
(function (host) {
  "use strict";

  const { codes, makeError, determineSpecificType } = host.internal;
  const Buffer = host.requireBuiltin("node:buffer").Buffer;

  // Os nomes de codificação que o bun aceita, sem diferenciar maiúsculas, e o nome canônico de cada um.
  function normalizeEncoding(encoding) {
    if (encoding === undefined || encoding === null || encoding === "") return "utf8";
    switch (String(encoding).toLowerCase()) {
      case "utf8":
      case "utf-8":
        return "utf8";
      case "ucs2":
      case "ucs-2":
      case "utf16le":
      case "utf-16le":
        return "utf16le";
      case "latin1":
      case "binary":
        return "latin1";
      case "base64":
        return "base64";
      case "base64url":
        return "base64url";
      case "ascii":
        return "ascii";
      case "hex":
        return "hex";
      default:
        return undefined;
    }
  }

  // Quantos bytes do fim de `bytes` formam uma sequência incompleta, e o tamanho total dela: `[pendentes, total]`.
  function incompleteTail(encoding, bytes) {
    const length = bytes.length;
    if (encoding === "utf8") {
      for (let back = 1; back <= Math.min(3, length); back++) {
        const byte = bytes[length - back];
        if ((byte & 0xc0) === 0x80) continue;
        const total = byte >= 0xf0 && byte <= 0xf4 ? 4 : byte >= 0xe0 ? 3 : byte >= 0xc2 && byte < 0xe0 ? 2 : 1;
        return total > back ? [back, total] : [0, 0];
      }
      return [0, 0];
    }
    if (encoding === "utf16le") {
      let pending = length % 2;
      // Um substituto alto no fim espera o baixo.
      if (length - pending >= 2) {
        const high = bytes[length - pending - 1];
        if (high >= 0xd8 && high <= 0xdb) pending += 2;
      }
      return pending === 0 ? [0, 0] : [pending, pending % 2 === 1 ? 2 : 4];
    }
    if (encoding === "base64" || encoding === "base64url") {
      const pending = length % 3;
      return pending === 0 ? [0, 0] : [pending, 3];
    }
    return [0, 0];
  }

  class StringDecoder {
    #encoding;
    #pending = Buffer.alloc(0);
    #total = 0;

    // `length` 0, como o do bun (o parâmetro é lido de `args`).
    constructor(...args) {
      const encoding = args[0];
      const normalized = normalizeEncoding(encoding);
      if (normalized === undefined) throw codes.UNKNOWN_ENCODING(encoding);
      this.#encoding = normalized;
    }

    get lastChar() {
      const chars = Buffer.alloc(4);
      this.#pending.copy(chars);
      return chars;
    }

    get lastNeed() {
      return this.#pending.length === 0 ? 0 : this.#total - this.#pending.length;
    }

    get lastTotal() {
      return this.#total;
    }

    get encoding() {
      return this.#encoding;
    }

    write(buf) {
      if (typeof buf === "string") return buf;
      if (!ArrayBuffer.isView(buf)) {
        throw makeError(
          "TypeError",
          "ERR_INVALID_ARG_TYPE",
          `The "buf" argument must be of type Buffer, TypedArray, or DataView. Received ${determineSpecificType(buf)}`,
        );
      }
      let bytes = Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength);
      if (this.#pending.length > 0) bytes = Buffer.concat([this.#pending, bytes]);
      const [pending, total] = incompleteTail(this.#encoding, bytes);
      this.#pending = Buffer.from(bytes.subarray(bytes.length - pending));
      this.#total = total;
      return bytes.subarray(0, bytes.length - pending).toString(this.#encoding);
    }

    end(buf) {
      let result = buf === undefined ? "" : this.write(buf);
      if (this.#pending.length > 0) {
        const pending = this.#pending;
        this.#pending = Buffer.alloc(0);
        this.#total = 0;
        // Uma sequência incompleta vira um U+FFFD em utf8, e em utf16le sai o que dá para ler.
        result += this.#encoding === "utf8" ? "\u{FFFD}" : pending.toString(this.#encoding);
      }
      return result;
    }

    text(buf, offset) {
      this.#pending = Buffer.alloc(0);
      this.#total = 0;
      return this.write(buf.slice(offset));
    }
  }

  // No bun `constructor` é a última chave do protótipo.
  delete StringDecoder.prototype.constructor;
  Object.defineProperty(StringDecoder.prototype, "constructor", { value: StringDecoder, writable: true, configurable: true });

  return { StringDecoder };
})
