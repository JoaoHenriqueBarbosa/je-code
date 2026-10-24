// `Bun.write` e o `FileSink` de `Bun.file(...).writer()` do bun 1.4.2 (ver `blob_file.rs`). Programa escondido: recebe o
// `host` (`requireBuiltin`, `internal`, `fileInfo`) e devolve `{ write, writer }`. O que é arquivo usa o `node:fs`, então os
// erros (`open`, `EISDIR`...) saem com a mesma forma dele. `options.mode` é ignorado, como no bun 1.4.2 (medido). A operação roda depois de um `setImmediate`, como o bun
// entrega o resultado de uma E/S.
(function (host) {
  "use strict";

  const fs = host.requireBuiltin("node:fs");
  const path = host.requireBuiltin("node:path");
  const { determineSpecificType } = host.internal;

  function codedError(Kind, message, code) {
    const error = new Kind(message);
    Object.defineProperty(error, "code", { value: code, writable: true, configurable: true, enumerable: false });
    return error;
  }

  const afterIo = () => new Promise(resolve => setImmediate(resolve));

  // O destino de `Bun.write`: caminho (texto), descritor (número), `URL` ou um Blob de arquivo.
  function targetOf(destination) {
    if (typeof destination === "string" || typeof destination === "number") return destination;
    if (destination !== null && typeof destination === "object") {
      const info = host.fileInfo(destination);
      if (info !== undefined) return info;
      if (typeof destination.href === "string" && typeof destination.protocol === "string") {
        return host.requireBuiltin("node:url").fileURLToPath(destination);
      }
    }
    throw codedError(
      TypeError,
      'The "destination" argument must be of type path, file descriptor, or Blob. Received ' + determineSpecificType(destination),
      "ERR_INVALID_ARG_TYPE",
    );
  }

  async function bytesOf(data) {
    if (data === undefined || data === null) {
      throw codedError(TypeError, "Bun.write(pathOrFdOrBlob, blob) expects a Blob-y thing to write", "ERR_INVALID_ARG_TYPE");
    }
    if (typeof data === "string") return Buffer.from(data, "utf8");
    if (data instanceof Response) return new Uint8Array(await data.arrayBuffer());
    if (data instanceof ReadableStream) {
      const chunks = [];
      const reader = data.getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(typeof value === "string" ? Buffer.from(value) : Buffer.from(value.buffer, value.byteOffset, value.byteLength));
      }
      return Buffer.concat(chunks);
    }
    if (data instanceof Blob) return data.bytes();
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    if (data instanceof ArrayBuffer || (typeof SharedArrayBuffer === "function" && data instanceof SharedArrayBuffer)) return new Uint8Array(data);
    if (Array.isArray(data)) return new Blob(data).bytes();
    return Buffer.from(String(data), "utf8");
  }

  function writeAll(fd, bytes) {
    let offset = 0;
    while (offset < bytes.length) offset += fs.writeSync(fd, bytes, offset, bytes.length - offset);
  }

  function writeTo(target, bytes, options) {
    if (typeof target === "number") {
      writeAll(target, bytes);
      return bytes.length;
    }
    if (!options || options.createPath !== false) fs.mkdirSync(path.dirname(path.resolve(target)), { recursive: true });
    const fd = fs.openSync(target, "w", 0o666);
    try {
      writeAll(fd, bytes);
    } finally {
      fs.closeSync(fd);
    }
    return bytes.length;
  }

  async function write(destination, data, options) {
    const target = targetOf(destination);
    const bytes = await bytesOf(data);
    await afterIo();
    return writeTo(target, bytes, options);
  }

  // `FileSink` (medido: `Object.keys` vazio, métodos `close, flush, end, start, write, ref, unref, _getFd, sinkId`).
  const sinks = new WeakMap();
  function sinkOf(sink) {
    const state = sinks.get(sink);
    if (state === undefined) throw new TypeError("Expected this to be instanceof FileSink");
    return state;
  }
  function FileSink() {
    throw new TypeError("FileSink is not constructable");
  }
  function flushPending(state) {
    if (state.fd === undefined) {
      if (typeof state.target === "number") state.fd = state.target;
      else {
        fs.mkdirSync(path.dirname(path.resolve(state.target)), { recursive: true });
        state.fd = fs.openSync(state.target, "w", 0o666);
        state.opened = true;
      }
    }
    const pending = Buffer.concat(state.chunks);
    state.chunks = [];
    writeAll(state.fd, pending);
    return pending.length;
  }
  function endSink(state) {
    const flushed = flushPending(state);
    if (state.opened) fs.closeSync(state.fd);
    state.fd = undefined;
    state.ended = true;
    return flushed;
  }
  const sinkMethods = {
    close() {
      return endSink(sinkOf(this));
    },
    flush() {
      return flushPending(sinkOf(this));
    },
    end() {
      return Promise.resolve(endSink(sinkOf(this)));
    },
    start() {},
    write(chunk) {
      const state = sinkOf(this);
      const bytes = typeof chunk === "string" ? Buffer.from(chunk, "utf8") : ArrayBuffer.isView(chunk) ? Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength) : Buffer.from(chunk);
      state.chunks.push(bytes);
      return bytes.length;
    },
    ref() {},
    unref() {},
    _getFd() {
      return sinkOf(this).fd === undefined ? -1 : sinkOf(this).fd;
    },
    sinkId() {
      return 0;
    },
  };
  const sinkPrototype = {};
  for (const name of Object.keys(sinkMethods)) {
    Object.defineProperty(sinkPrototype, name, { value: sinkMethods[name], writable: true, configurable: true, enumerable: true });
  }
  Object.defineProperty(sinkPrototype, "constructor", { value: FileSink, writable: true, configurable: true, enumerable: false });
  Object.defineProperty(sinkPrototype, Symbol.toStringTag, { value: "FileSink", configurable: true });
  FileSink.prototype = sinkPrototype;

  function writer(blob) {
    const target = targetOf(blob);
    const sink = Object.create(sinkPrototype);
    sinks.set(sink, { target, chunks: [], fd: undefined, opened: false, ended: false });
    return sink;
  }

  // `Bun.stdout.stat()` e afins: o `stat` de um descritor, depois de uma volta do laço como a E/S.
  async function fstat(fd) {
    await afterIo();
    return fs.fstatSync(fd);
  }

  return { write, writer, fstat };
})
