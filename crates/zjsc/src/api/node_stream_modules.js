// GERADO por `scripts/gen-node-stream.js` a partir dos fontes do stream do bun 1.4.2. Não editar à mão.
// O começo de `node_stream_modules.js` (gerado por `gen-node-stream.js`): a fábrica do módulo, com o que os intrínsecos
// do JSC (`$debug`, `$toClass`, `$ERR_*`, ...) viram aqui. `host.internal` é o `node_internal.js`.
(function (host) {
  "use strict";

  const {
    codes,
    makeAbortError,
    validators,
    aggregateTwoErrors,
    SafeSet,
    SafeArrayIterator,
    kEmptyObject,
    resistStopPropagation,
    once,
    addAbortListener,
    toClass,
  } = host.internal;
  const kAutoDestroyed = Symbol("kAutoDestroyed");
  const kWeakHandler = Symbol("kWeak");

  const debug = () => {};
  const assert = () => {};
  const isCallable = value => typeof value === "function";

  let BufferConstructor;
  const getBuffer = () => (BufferConstructor ??= host.requireBuiltin("node:buffer").Buffer);

  // `$inheritsReadableStream` e companhia: o teste de classe dos web streams.
  const inheritsReadableStream = value => typeof ReadableStream === "function" && value instanceof ReadableStream;
  const inheritsWritableStream = value => typeof WritableStream === "function" && value instanceof WritableStream;
  const inheritsTransformStream = value => typeof TransformStream === "function" && value instanceof TransformStream;
  const inheritsBlob = value => typeof Blob === "function" && value instanceof Blob;

  // DIVERGÊNCIA: os dois ganchos de web stream do bun (`$webStreamControllerError`, `$webStreamClosedPromise`) falam com
  // o interior do `ReadableStream` e ainda não têm equivalente aqui.
  const webStreamControllerError = () => {
    throw new Error("node:stream com web streams ainda não é suportado");
  };
  const webStreamClosedPromise = () => {
    throw new Error("node:stream com web streams ainda não é suportado");
  };

  const definitions = {
    "node:stream": function (module, _unusedExports, require) {
      "use strict";
      const exports = require("internal/stream");
      debug("node:stream loaded");
      exports.eos = require("internal/streams/end-of-stream");
      module.exports = exports;

    },
    "internal/stream": function (module, _unusedExports, require) {
      "use strict";
      const ObjectKeys = Object.keys;
      const ObjectDefineProperty = Object.defineProperty;
      const customPromisify = Symbol.for("nodejs.util.promisify.custom");
      const { streamReturningOperators, promiseReturningOperators } = require("internal/streams/operators");
      const compose = require("internal/streams/compose");
      const { setDefaultHighWaterMark, getDefaultHighWaterMark } = require("internal/streams/state");
      const { pipeline } = require("internal/streams/pipeline");
      const { destroyer } = require("internal/streams/destroy");
      const eos = require("internal/streams/end-of-stream");
      const utils = require("internal/streams/utils");
      const { isArrayBufferView, isUint8Array } = require("node:util/types");
      const Stream = require("internal/streams/legacy").Stream;
      Stream.isDestroyed = utils.isDestroyed;
      Stream.isDisturbed = utils.isDisturbed;
      Stream.isErrored = utils.isErrored;
      Stream.isReadable = utils.isReadable;
      Stream.isWritable = utils.isWritable;
      Stream.Readable = require("internal/streams/readable");
      const streamKeys = ObjectKeys(streamReturningOperators);
      for (let i = 0;i < streamKeys.length; i++) {
        let fn = function(...args) {
          if (new.target) {
            throw codes.ILLEGAL_CONSTRUCTOR();
          }
          return Stream.Readable.from(op.apply(this, args));
        };
        const key = streamKeys[i];
        const op = streamReturningOperators[key];
        ObjectDefineProperty(fn, "name", { __proto__: null, value: op.name });
        ObjectDefineProperty(fn, "length", { __proto__: null, value: op.length });
        ObjectDefineProperty(Stream.Readable.prototype, key, {
          __proto__: null,
          value: fn,
          enumerable: false,
          configurable: true,
          writable: true
        });
      }
      const promiseKeys = ObjectKeys(promiseReturningOperators);
      for (let i = 0;i < promiseKeys.length; i++) {
        let fn = function(...args) {
          if (new.target) {
            throw codes.ILLEGAL_CONSTRUCTOR();
          }
          return Promise.resolve().then(() => op.apply(this, args));
        };
        const key = promiseKeys[i];
        const op = promiseReturningOperators[key];
        ObjectDefineProperty(fn, "name", { __proto__: null, value: op.name });
        ObjectDefineProperty(fn, "length", { __proto__: null, value: op.length });
        ObjectDefineProperty(Stream.Readable.prototype, key, {
          __proto__: null,
          value: fn,
          enumerable: false,
          configurable: true,
          writable: true
        });
      }
      Stream.Writable = require("internal/streams/writable");
      Stream.Duplex = require("internal/streams/duplex");
      Stream.Transform = require("internal/streams/transform");
      Stream.PassThrough = require("internal/streams/passthrough");
      Stream.duplexPair = require("internal/streams/duplexpair");
      Stream.pipeline = pipeline;
      const { addAbortSignal } = require("internal/streams/add-abort-signal");
      Stream.addAbortSignal = addAbortSignal;
      Stream.finished = eos;
      Stream.destroy = destroyer;
      Stream.compose = compose;
      Stream.setDefaultHighWaterMark = setDefaultHighWaterMark;
      Stream.getDefaultHighWaterMark = getDefaultHighWaterMark;
      ObjectDefineProperty(Stream, "promises", {
        __proto__: null,
        configurable: true,
        enumerable: true,
        get() {
          return require("internal/stream.promises");
        }
      });
      ObjectDefineProperty(pipeline, customPromisify, {
        __proto__: null,
        enumerable: true,
        get() {
          return require("internal/stream.promises").pipeline;
        }
      });
      ObjectDefineProperty(eos, customPromisify, {
        __proto__: null,
        enumerable: true,
        get() {
          return require("internal/stream.promises").finished;
        }
      });
      Stream.Stream = Stream;
      Stream._isArrayBufferView = isArrayBufferView;
      Stream._isUint8Array = isUint8Array;
      Stream._uint8ArrayToBuffer = function _uint8ArrayToBuffer(chunk) {
        return new (getBuffer())(chunk.buffer, chunk.byteOffset, chunk.byteLength);
      };
      module.exports = Stream;

    },
    "internal/stream.promises": function (module, _unusedExports, require) {
      "use strict";
      const ArrayPrototypePop = Array.prototype.pop;
      const { isIterable, isNodeStream, isWebStream } = require("internal/streams/utils");
      const { pipelineImpl: pl } = require("internal/streams/pipeline");
      const { finished } = require("internal/streams/end-of-stream");
      function pipeline(...streams) {
        return new Promise((resolve, reject) => {
          let signal;
          let end;
          const lastArg = streams[streams.length - 1];
          if (lastArg && typeof lastArg === "object" && !isNodeStream(lastArg) && !isIterable(lastArg) && !isWebStream(lastArg)) {
            const options = ArrayPrototypePop.call(streams);
            signal = options.signal;
            end = options.end;
          }
          pl(streams, (err, value) => {
            if (err) {
              reject(err);
            } else {
              resolve(value);
            }
          }, { signal, end });
        });
      }
      module.exports = {
        finished,
        pipeline
      };

    },
    "internal/streams/add-abort-signal": function (module, _unusedExports, require) {
      "use strict";
      const { isNodeStream, isWebStream, kControllerErrorFunction } = require("internal/streams/utils");
      const eos = require("internal/streams/end-of-stream");
      const SymbolDispose = Symbol.dispose;
      let addAbortListener;
      const validateAbortSignal = (signal, name) => {
        if (typeof signal !== "object" || !("aborted" in signal)) {
          throw codes.INVALID_ARG_TYPE(name, "AbortSignal", signal);
        }
      };
      function addAbortSignal(signal, stream) {
        validateAbortSignal(signal, "signal");
        if (!isNodeStream(stream) && !isWebStream(stream)) {
          throw codes.INVALID_ARG_TYPE("stream", ["ReadableStream", "WritableStream", "Stream"], stream);
        }
        return addAbortSignalNoValidate(signal, stream);
      }
      function addAbortSignalNoValidate(signal, stream) {
        if (typeof signal !== "object" || !("aborted" in signal)) {
          return stream;
        }
        const onAbort = isNodeStream(stream) ? () => {
          stream.destroy(makeAbortError(undefined, { cause: signal.reason }));
        } : () => {
          const error = makeAbortError(undefined, { cause: signal.reason });
          if (stream[kControllerErrorFunction] !== undefined) {
            stream[kControllerErrorFunction](error);
          } else {
            webStreamControllerError(stream, error);
          }
        };
        if (signal.aborted) {
          onAbort();
        } else {
          addAbortListener ??= require("internal/abort_listener").addAbortListener;
          const disposable = addAbortListener(signal, onAbort);
          eos(stream, disposable[SymbolDispose]);
        }
        return stream;
      }
      module.exports = {
        addAbortSignal,
        addAbortSignalNoValidate
      };

    },
    "internal/streams/compose": function (module, _unusedExports, require) {
      "use strict";
      const { pipeline } = require("internal/streams/pipeline");
      const Duplex = require("internal/streams/duplex");
      const { destroyer } = require("internal/streams/destroy");
      const {
        isNodeStream,
        isReadable,
        isWritable,
        isWebStream,
        isTransformStream,
        isWritableStream,
        isReadableStream
      } = require("internal/streams/utils");
      const eos = require("internal/streams/end-of-stream");
      const ArrayPrototypeSlice = Array.prototype.slice;
      module.exports = function compose(...streams) {
        if (streams.length === 0) {
          throw codes.MISSING_ARGS("streams");
        }
        if (streams.length === 1) {
          return Duplex.from(streams[0]);
        }
        const orgStreams = ArrayPrototypeSlice.call(streams);
        if (typeof streams[0] === "function") {
          streams[0] = Duplex.from(streams[0]);
        }
        const lastIdx = streams.length - 1;
        if (typeof streams[lastIdx] === "function") {
          streams[lastIdx] = Duplex.from(streams[lastIdx]);
        }
        for (let n = 0;n < streams.length; ++n) {
          if (!isNodeStream(streams[n]) && !isWebStream(streams[n])) {
            continue;
          }
          if (n < streams.length - 1 && !(isReadable(streams[n]) || isReadableStream(streams[n]) || isTransformStream(streams[n]))) {
            throw codes.INVALID_ARG_VALUE(`streams[${n}]`, orgStreams[n], "must be readable");
          }
          if (n > 0 && !(isWritable(streams[n]) || isWritableStream(streams[n]) || isTransformStream(streams[n]))) {
            throw codes.INVALID_ARG_VALUE(`streams[${n}]`, orgStreams[n], "must be writable");
          }
        }
        let ondrain;
        let onfinish;
        let onreadable;
        let onclose;
        let d;
        function onfinished(err) {
          const cb = onclose;
          onclose = null;
          if (cb) {
            cb(err);
          } else if (err) {
            d.destroy(err);
          } else if (!readable && !writable) {
            d.destroy();
          }
        }
        const head = streams[0];
        const tail = pipeline(streams, onfinished);
        const writable = !!(isWritable(head) || isWritableStream(head) || isTransformStream(head));
        const readable = !!(isReadable(tail) || isReadableStream(tail) || isTransformStream(tail));
        d = new Duplex({
          writableObjectMode: !!head?.writableObjectMode,
          readableObjectMode: !!tail?.readableObjectMode,
          writable,
          readable
        });
        if (writable) {
          if (isNodeStream(head)) {
            d._write = function(chunk, encoding, callback) {
              if (head.write(chunk, encoding)) {
                callback();
              } else {
                ondrain = callback;
              }
            };
            d._final = function(callback) {
              head.end();
              onfinish = callback;
            };
            head.on("drain", function() {
              if (ondrain) {
                const cb = ondrain;
                ondrain = null;
                cb();
              }
            });
          } else if (isWebStream(head)) {
            const writable = isTransformStream(head) ? head.writable : head;
            const writer = writable.getWriter();
            d._write = async function(chunk, encoding, callback) {
              try {
                await writer.ready;
                writer.write(chunk).catch(() => {});
                callback();
              } catch (err) {
                callback(err);
              }
            };
            d._final = async function(callback) {
              try {
                await writer.ready;
                writer.close().catch(() => {});
                onfinish = callback;
              } catch (err) {
                callback(err);
              }
            };
          }
          const toRead = isTransformStream(tail) ? tail.readable : tail;
          eos(toRead, () => {
            if (onfinish) {
              const cb = onfinish;
              onfinish = null;
              cb();
            }
          });
        }
        if (readable) {
          if (isNodeStream(tail)) {
            tail.on("readable", function() {
              if (onreadable) {
                const cb = onreadable;
                onreadable = null;
                cb();
              }
            });
            tail.on("end", function() {
              d.push(null);
            });
            d._read = function() {
              while (true) {
                const buf = tail.read();
                if (buf === null) {
                  onreadable = d._read;
                  return;
                }
                if (!d.push(buf)) {
                  return;
                }
              }
            };
          } else if (isWebStream(tail)) {
            const readable = isTransformStream(tail) ? tail.readable : tail;
            const reader = readable.getReader();
            d._read = async function() {
              while (true) {
                try {
                  const { value, done } = await reader.read();
                  if (!d.push(value)) {
                    return;
                  }
                  if (done) {
                    d.push(null);
                    return;
                  }
                } catch {
                  return;
                }
              }
            };
          }
        }
        d._destroy = function(err, callback) {
          if (!err && onclose !== null) {
            err = makeAbortError();
          }
          onreadable = null;
          ondrain = null;
          onfinish = null;
          if (isNodeStream(tail)) {
            destroyer(tail, err);
          }
          if (onclose === null) {
            callback(err);
          } else {
            onclose = callback;
          }
        };
        return d;
      }

    },
    "internal/streams/destroy": function (module, _unusedExports, require) {
      "use strict";
      const { aggregateTwoErrors } = require("internal/errors");
      const {
        kIsDestroyed,
        isDestroyed,
        isFinished,
        isServerRequest,
        kState,
        kErrorEmitted,
        kEmitClose,
        kClosed,
        kCloseEmitted,
        kConstructed,
        kDestroyed,
        kAutoDestroy,
        kErrored
      } = require("internal/streams/utils");
      const ProcessNextTick = process.nextTick;
      const kDestroy = Symbol("kDestroy");
      const kConstruct = Symbol("kConstruct");
      function checkError(err, w, r) {
        if (err) {
          err.stack;
          if (w && !w.errored) {
            w.errored = err;
          }
          if (r && !r.errored) {
            r.errored = err;
          }
        }
      }
      function destroy(err, cb) {
        const r = this._readableState;
        const w = this._writableState;
        const s = w || r;
        if (w && (w[kState] & kDestroyed) !== 0 || r && (r[kState] & kDestroyed) !== 0) {
          if (typeof cb === "function") {
            cb();
          }
          return this;
        }
        checkError(err, w, r);
        if (w) {
          w[kState] |= kDestroyed;
        }
        if (r) {
          r[kState] |= kDestroyed;
        }
        if ((s[kState] & kConstructed) === 0) {
          this.once(kDestroy, function(er) {
            _destroy(this, aggregateTwoErrors(er, err), cb);
          });
        } else {
          _destroy(this, err, cb);
        }
        return this;
      }
      function _destroy(self, err, cb) {
        let called = false;
        function onDestroy(err) {
          if (called) {
            return;
          }
          called = true;
          const r = self._readableState;
          const w = self._writableState;
          checkError(err, w, r);
          if (w) {
            w[kState] |= kClosed;
          }
          if (r) {
            r[kState] |= kClosed;
          }
          if (typeof cb === "function") {
            cb(err);
          }
          if (err) {
            ProcessNextTick(emitErrorCloseNT, self, err);
          } else {
            ProcessNextTick(emitCloseNT, self);
          }
        }
        try {
          self._destroy(err || null, onDestroy);
        } catch (err) {
          onDestroy(err);
        }
      }
      function emitErrorCloseNT(self, err) {
        emitErrorNT(self, err);
        emitCloseNT(self);
      }
      function emitCloseNT(self) {
        const r = self._readableState;
        const w = self._writableState;
        if (w) {
          w[kState] |= kCloseEmitted;
        }
        if (r) {
          r[kState] |= kCloseEmitted;
        }
        if (w && (w[kState] & kEmitClose) !== 0 || r && (r[kState] & kEmitClose) !== 0) {
          self.emit("close");
        }
      }
      function emitErrorNT(self, err) {
        const r = self._readableState;
        const w = self._writableState;
        if (w && (w[kState] & kErrorEmitted) !== 0 || r && (r[kState] & kErrorEmitted) !== 0) {
          return;
        }
        if (w) {
          w[kState] |= kErrorEmitted;
        }
        if (r) {
          r[kState] |= kErrorEmitted;
        }
        self.emit("error", err);
      }
      function undestroy() {
        const r = this._readableState;
        const w = this._writableState;
        if (r) {
          r.constructed = true;
          r.closed = false;
          r.closeEmitted = false;
          r.destroyed = false;
          r.errored = null;
          r.errorEmitted = false;
          r.reading = false;
          r.ended = r.readable === false;
          r.endEmitted = r.readable === false;
        }
        if (w) {
          w.constructed = true;
          w.destroyed = false;
          w.closed = false;
          w.closeEmitted = false;
          w.errored = null;
          w.errorEmitted = false;
          w.finalCalled = false;
          w.prefinished = false;
          w.ended = w.writable === false;
          w.ending = w.writable === false;
          w.finished = w.writable === false;
        }
      }
      function errorOrDestroy(stream, err, sync) {
        const r = stream._readableState;
        const w = stream._writableState;
        if (w && (w[kState] ? (w[kState] & kDestroyed) !== 0 : w.destroyed) || r && (r[kState] ? (r[kState] & kDestroyed) !== 0 : r.destroyed)) {
          return this;
        }
        if (r && (r[kState] & kAutoDestroy) !== 0 || w && (w[kState] & kAutoDestroy) !== 0) {
          stream.destroy(err);
        } else if (err) {
          err.stack;
          if (w && (w[kState] & kErrored) === 0) {
            w.errored = err;
          }
          if (r && (r[kState] & kErrored) === 0) {
            r.errored = err;
          }
          if (sync) {
            ProcessNextTick(emitErrorNT, stream, err);
          } else {
            emitErrorNT(stream, err);
          }
        }
      }
      function construct(stream, cb) {
        if (typeof stream._construct !== "function") {
          return;
        }
        const r = stream._readableState;
        const w = stream._writableState;
        if (r) {
          r[kState] &= ~kConstructed;
        }
        if (w) {
          w[kState] &= ~kConstructed;
        }
        stream.once(kConstruct, cb);
        if (stream.listenerCount(kConstruct) > 1) {
          return;
        }
        ProcessNextTick(constructNT, stream);
      }
      function constructNT(stream) {
        let called = false;
        function onConstruct(err) {
          if (called) {
            errorOrDestroy(stream, err ?? codes.MULTIPLE_CALLBACK());
            return;
          }
          called = true;
          const r = stream._readableState;
          const w = stream._writableState;
          const s = w || r;
          if (r) {
            r[kState] |= kConstructed;
          }
          if (w) {
            w[kState] |= kConstructed;
          }
          if (s.destroyed) {
            stream.emit(kDestroy, err);
          } else if (err) {
            errorOrDestroy(stream, err, true);
          } else {
            stream.emit(kConstruct);
          }
        }
        try {
          stream._construct((err) => {
            ProcessNextTick(onConstruct, err);
          });
        } catch (err) {
          ProcessNextTick(onConstruct, err);
        }
      }
      function isRequest(stream) {
        return stream?.setHeader && typeof stream.abort === "function";
      }
      function emitCloseLegacy(stream) {
        stream.emit("close");
      }
      function emitErrorCloseLegacy(stream, err) {
        stream.emit("error", err);
        ProcessNextTick(emitCloseLegacy, stream);
      }
      function destroyer(stream, err) {
        if (!stream || isDestroyed(stream)) {
          return;
        }
        if (!err && !isFinished(stream)) {
          err = makeAbortError();
        }
        if (isServerRequest(stream)) {
          stream.socket = null;
          stream.destroy(err);
        } else if (isRequest(stream)) {
          stream.abort();
        } else {
          const req = stream.req;
          if (isRequest(req)) {
            req.abort();
          } else if (typeof stream.destroy === "function") {
            stream.destroy(err);
          } else if (typeof stream.close === "function") {
            stream.close();
          } else if (err) {
            ProcessNextTick(emitErrorCloseLegacy, stream, err);
          } else {
            ProcessNextTick(emitCloseLegacy, stream);
          }
        }
        if (!stream.destroyed) {
          stream[kIsDestroyed] = true;
        }
      }
      module.exports = {
        construct,
        destroyer,
        destroy,
        undestroy,
        errorOrDestroy
      };

    },
    "internal/streams/duplex": function (module, _unusedExports, require) {
      "use strict";
      const Stream = require("internal/streams/legacy").Stream;
      const Readable = require("internal/streams/readable");
      const Writable = require("internal/streams/writable");
      const { addAbortSignal } = require("internal/streams/add-abort-signal");
      const destroyImpl = require("internal/streams/destroy");
      const { kOnConstructed } = require("internal/streams/utils");
      const ObjectKeys = Object.keys;
      const ObjectDefineProperties = Object.defineProperties;
      const ObjectGetOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
      function Duplex(options) {
        if (!(this instanceof Duplex))
          return new Duplex(options);
        this._events ??= {
          close: undefined,
          error: undefined,
          prefinish: undefined,
          finish: undefined,
          drain: undefined,
          data: undefined,
          end: undefined,
          readable: undefined
        };
        this._readableState = new Readable.ReadableState(options, this, true);
        this._writableState = new Writable.WritableState(options, this, true);
        if (options) {
          this.allowHalfOpen = options.allowHalfOpen !== false;
          if (options.readable === false) {
            this._readableState.readable = false;
            this._readableState.ended = true;
            this._readableState.endEmitted = true;
          }
          if (options.writable === false) {
            this._writableState.writable = false;
            this._writableState.ending = true;
            this._writableState.ended = true;
            this._writableState.finished = true;
          }
          const { read, write, writev, destroy, final, construct, signal } = options;
          if (typeof read === "function")
            this._read = read;
          if (typeof write === "function")
            this._write = write;
          if (typeof writev === "function")
            this._writev = writev;
          if (typeof destroy === "function")
            this._destroy = destroy;
          if (typeof final === "function")
            this._final = final;
          if (typeof construct === "function")
            this._construct = construct;
          if (signal)
            addAbortSignal(signal, this);
        } else {
          this.allowHalfOpen = true;
        }
        Stream.call(this, options);
        if (this._construct != null) {
          destroyImpl.construct(this, () => {
            this._readableState[kOnConstructed](this);
            this._writableState[kOnConstructed](this);
          });
        }
      }
      toClass(Duplex, "Duplex", Readable);
      Duplex.prototype.destroy = Writable.prototype.destroy;
      {
        const keys = ObjectKeys(Writable.prototype);
        for (let i = 0;i < keys.length; i++) {
          const method = keys[i];
          Duplex.prototype[method] ||= Writable.prototype[method];
        }
      }
      ObjectDefineProperties(Duplex.prototype, {
        writable: { __proto__: null, ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writable") },
        writableHighWaterMark: {
          __proto__: null,
          ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writableHighWaterMark")
        },
        writableObjectMode: { __proto__: null, ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writableObjectMode") },
        writableBuffer: { __proto__: null, ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writableBuffer") },
        writableLength: { __proto__: null, ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writableLength") },
        writableFinished: { __proto__: null, ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writableFinished") },
        writableCorked: { __proto__: null, ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writableCorked") },
        writableEnded: { __proto__: null, ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writableEnded") },
        writableNeedDrain: { __proto__: null, ...ObjectGetOwnPropertyDescriptor(Writable.prototype, "writableNeedDrain") },
        destroyed: {
          __proto__: null,
          get() {
            if (this._readableState === undefined || this._writableState === undefined) {
              return false;
            }
            return this._readableState.destroyed && this._writableState.destroyed;
          },
          set(value) {
            const readableState = this._readableState;
            let writableState;
            if (readableState && (writableState = this._writableState)) {
              readableState.destroyed = value;
              writableState.destroyed = value;
            }
          }
        }
      });
      let webStreamsAdapters;
      function lazyWebStreams() {
        if (webStreamsAdapters === undefined)
          webStreamsAdapters = require("internal/webstreams_adapters");
        return webStreamsAdapters;
      }
      Duplex.fromWeb = function(pair, options) {
        return lazyWebStreams().newStreamDuplexFromReadableWritablePair(pair, options);
      };
      Duplex.toWeb = function(duplex, options) {
        return lazyWebStreams().newReadableWritablePairFromDuplex(duplex, options);
      };
      let duplexify;
      Duplex.from = function(body) {
        duplexify ??= require("internal/streams/duplexify");
        return duplexify(body, "body");
      };
      module.exports = Duplex;

    },
    "internal/streams/duplexify": function (module, _unusedExports, require) {
      "use strict";
      const {
        isReadable,
        isWritable,
        isIterable,
        isNodeStream,
        isReadableNodeStream,
        isWritableNodeStream,
        isDuplexNodeStream,
        isReadableStream,
        isWritableStream
      } = require("internal/streams/utils");
      const eos = require("internal/streams/end-of-stream");
      const { destroyer } = require("internal/streams/destroy");
      const Duplex = require("internal/streams/duplex");
      const Readable = require("internal/streams/readable");
      const Writable = require("internal/streams/writable");
      const from = require("internal/streams/from");
      const PromiseWithResolvers = Promise.withResolvers.bind(Promise);

      class Duplexify extends Duplex {
        constructor(options) {
          super(options);
          if (options?.readable === false) {
            this._readableState.readable = false;
            this._readableState.ended = true;
            this._readableState.endEmitted = true;
          }
          if (options?.writable === false) {
            this._writableState.writable = false;
            this._writableState.ending = true;
            this._writableState.ended = true;
            this._writableState.finished = true;
          }
        }
      }
      function duplexify(body, name) {
        if (isDuplexNodeStream(body)) {
          return body;
        }
        if (isReadableNodeStream(body)) {
          return _duplexify({ readable: body });
        }
        if (isWritableNodeStream(body)) {
          return _duplexify({ writable: body });
        }
        if (isNodeStream(body)) {
          return _duplexify({ writable: false, readable: false });
        }
        if (isReadableStream(body)) {
          return _duplexify({ readable: Readable.fromWeb(body) });
        }
        if (isWritableStream(body)) {
          return _duplexify({ writable: Writable.fromWeb(body) });
        }
        if (typeof body === "function") {
          const { value, write, final, destroy } = fromAsyncGen(body);
          if (isDuplexNodeStream(value)) {
            return value;
          }
          if (isIterable(value)) {
            return from(Duplexify, value, {
              objectMode: true,
              write,
              final,
              destroy
            });
          }
          const then = value?.then;
          if (typeof then === "function") {
            let d;
            const promise = then.call(value, (val) => {
              if (val != null) {
                throw codes.INVALID_RETURN_VALUE("nully", "body", val);
              }
            }, (err) => {
              destroyer(d, err);
            });
            return d = new Duplexify({
              objectMode: true,
              readable: false,
              write,
              final(cb) {
                final(async () => {
                  try {
                    await promise;
                    process.nextTick(cb, null);
                  } catch (err) {
                    process.nextTick(cb, err);
                  }
                });
              },
              destroy
            });
          }
          throw codes.INVALID_RETURN_VALUE("Iterable, AsyncIterable or AsyncFunction", name, value);
        }
        if (inheritsBlob(body)) {
          return duplexify(body.arrayBuffer());
        }
        if (isIterable(body)) {
          return from(Duplexify, body, {
            objectMode: true,
            writable: false
          });
        }
        if (isReadableStream(body?.readable) && isWritableStream(body?.writable)) {
          return Duplexify.fromWeb(body);
        }
        if (typeof body?.writable === "object" || typeof body?.readable === "object") {
          const readable = body?.readable ? isReadableNodeStream(body?.readable) ? body?.readable : duplexify(body.readable) : undefined;
          const writable = body?.writable ? isWritableNodeStream(body?.writable) ? body?.writable : duplexify(body.writable) : undefined;
          return _duplexify({ readable, writable });
        }
        const then = body?.then;
        if (typeof then === "function") {
          let d;
          then.call(body, (val) => {
            if (val != null) {
              d.push(val);
            }
            d.push(null);
          }, (err) => {
            destroyer(d, err);
          });
          return d = new Duplexify({
            objectMode: true,
            writable: false,
            read() {}
          });
        }
        throw codes.INVALID_ARG_TYPE(name, [
          "Blob",
          "ReadableStream",
          "WritableStream",
          "Stream",
          "Iterable",
          "AsyncIterable",
          "Function",
          "{ readable, writable } pair",
          "Promise"
        ], body);
      }
      function fromAsyncGen(fn) {
        let { promise, resolve } = PromiseWithResolvers();
        const ac = new AbortController;
        const signal = ac.signal;
        const value = fn(async function* () {
          while (true) {
            const _promise = promise;
            promise = null;
            const { chunk, done, cb } = await _promise;
            process.nextTick(cb);
            if (done)
              return;
            if (signal.aborted)
              throw makeAbortError(undefined, { cause: signal.reason });
            ({ promise, resolve } = PromiseWithResolvers());
            yield chunk;
          }
        }(), { signal });
        return {
          value,
          write(chunk, encoding, cb) {
            const _resolve = resolve;
            resolve = null;
            _resolve({ chunk, done: false, cb });
          },
          final(cb) {
            const _resolve = resolve;
            resolve = null;
            _resolve({ done: true, cb });
          },
          destroy(err, cb) {
            ac.abort(err);
            if (resolve !== null) {
              const _resolve = resolve;
              resolve = null;
              _resolve({ __proto__: null, done: true, cb() {} });
            }
            cb(err);
          }
        };
      }
      function _duplexify(pair) {
        const r = pair.readable && typeof pair.readable.read !== "function" ? Readable.wrap(pair.readable) : pair.readable;
        const w = pair.writable;
        let readable = !!isReadable(r);
        let writable = !!isWritable(w);
        let ondrain;
        let onfinish;
        let onreadable;
        let onclose;
        let d;
        function onfinished(err) {
          const cb = onclose;
          onclose = null;
          if (cb) {
            cb(err);
          } else if (err) {
            d.destroy(err);
          }
        }
        d = new Duplexify({
          readableObjectMode: !!r?.readableObjectMode,
          writableObjectMode: !!w?.writableObjectMode,
          readable,
          writable
        });
        if (writable) {
          eos(w, (err) => {
            writable = false;
            if (err) {
              destroyer(r, err);
            }
            onfinished(err);
          });
          d._write = function(chunk, encoding, callback) {
            if (w.write(chunk, encoding)) {
              callback();
            } else {
              ondrain = callback;
            }
          };
          d._final = function(callback) {
            w.end();
            onfinish = callback;
          };
          w.on("drain", function() {
            if (ondrain) {
              const cb = ondrain;
              ondrain = null;
              cb();
            }
          });
          w.on("finish", function() {
            if (onfinish) {
              const cb = onfinish;
              onfinish = null;
              cb();
            }
          });
        }
        if (readable) {
          eos(r, (err) => {
            readable = false;
            if (err) {
              destroyer(w, err);
            }
            onfinished(err);
          });
          r.on("readable", function() {
            if (onreadable) {
              const cb = onreadable;
              onreadable = null;
              cb();
            }
          });
          r.on("end", function() {
            d.push(null);
          });
          d._read = function() {
            while (true) {
              const buf = r.read();
              if (buf === null) {
                onreadable = d._read;
                return;
              }
              if (!d.push(buf)) {
                return;
              }
            }
          };
        }
        d._destroy = function(err, callback) {
          if (!err && onclose !== null) {
            err = makeAbortError();
          }
          onreadable = null;
          ondrain = null;
          onfinish = null;
          if (onclose === null) {
            callback(err);
          } else {
            onclose = callback;
            destroyer(w, err);
            destroyer(r, err);
          }
        };
        return d;
      }
      module.exports = duplexify;

    },
    "internal/streams/duplexpair": function (module, _unusedExports, require) {
      "use strict";
      const Duplex = require("internal/streams/duplex");
      const kCallback = Symbol("Callback");
      const kInitOtherSide = Symbol("InitOtherSide");

      class DuplexSide extends Duplex {
        #otherSide = null;
        [kCallback] = null;
        constructor(options) {
          super(options);
          this.#otherSide = null;
        }
        [kInitOtherSide](otherSide) {
          if (this.#otherSide === null) {
            this.#otherSide = otherSide;
          } else {
            assert(this.#otherSide === null);
          }
        }
        _read() {
          const callback = this[kCallback];
          if (callback) {
            this[kCallback] = null;
            callback();
          }
        }
        _write(chunk, encoding, callback) {
          assert(this.#otherSide !== null);
          assert(this.#otherSide[kCallback] === null);
          if (chunk.length === 0) {
            process.nextTick(callback);
          } else {
            this.#otherSide.push(chunk);
            this.#otherSide[kCallback] = callback;
          }
        }
        _final(callback) {
          this.#otherSide.on("end", callback);
          this.#otherSide.push(null);
        }
        _destroy(err, callback) {
          const otherSide = this.#otherSide;
          if (otherSide !== null && !otherSide.destroyed) {
            process.nextTick(() => {
              if (otherSide.destroyed)
                return;
              if (err)
                otherSide.destroy();
              else
                otherSide.push(null);
            });
          }
          callback(err);
        }
      }
      function duplexPair(options) {
        const side0 = new DuplexSide(options);
        const side1 = new DuplexSide(options);
        side0[kInitOtherSide](side1);
        side1[kInitOtherSide](side0);
        return [side0, side1];
      }
      module.exports = duplexPair;

    },
    "internal/streams/end-of-stream": function (module, _unusedExports, require) {
      "use strict";
      const { kEmptyObject, once } = require("internal/shared");
      const { validateAbortSignal, validateFunction, validateObject, validateBoolean } = require("internal/validators");
      const {
        isClosed,
        isReadable,
        isReadableNodeStream,
        isReadableStream,
        isReadableFinished,
        isReadableErrored,
        isWritable,
        isWritableNodeStream,
        isWritableStream,
        isWritableFinished,
        isWritableErrored,
        isNodeStream,
        willEmitClose: _willEmitClose,
        kIsClosedPromise
      } = require("internal/streams/utils");
      const SymbolDispose = Symbol.dispose;
      const PromisePrototypeThen = Promise.prototype.then;
      let addAbortListener;
      let AsyncResource;
      let enabledHooksExist;
      function isRequest(stream) {
        return stream.setHeader && typeof stream.abort === "function";
      }
      const nop = () => {};
      function bindAsyncResource(fn, type) {
        AsyncResource ??= require("node:async_hooks").AsyncResource;
        const resource = new AsyncResource(type);
        return function(...args) {
          return resource.runInAsyncScope(fn, this, ...args);
        };
      }
      function hasAsyncContext() {
        if (undefined !== undefined)
          return true;
        enabledHooksExist ??= require("internal/async_hooks").enabledHooksExist;
        return enabledHooksExist();
      }
      function getEosErrored(stream) {
        const errored = isWritableErrored(stream) || isReadableErrored(stream);
        return typeof errored !== "boolean" && errored || null;
      }
      function getEosOnCloseError(stream, readable, readableFinished, writable, writableFinished) {
        const errored = getEosErrored(stream);
        if (errored) {
          return errored;
        }
        if (readable && !readableFinished && isReadableNodeStream(stream, true)) {
          if (!isReadableFinished(stream, false)) {
            return codes.STREAM_PREMATURE_CLOSE();
          }
        }
        if (writable && !writableFinished) {
          if (!isWritableFinished(stream, false)) {
            return codes.STREAM_PREMATURE_CLOSE();
          }
        }
        return null;
      }
      const kEosNodeSynchronousCallback = Symbol("kEosNodeSynchronousCallback");
      function eos(stream, options, callback) {
        if (arguments.length === 2) {
          callback = options;
          options = kEmptyObject;
        } else if (options == null) {
          options = kEmptyObject;
        } else {
          validateObject(options, "options");
        }
        validateFunction(callback, "callback");
        validateAbortSignal(options.signal, "options.signal");
        if (isReadableStream(stream) || isWritableStream(stream)) {
          return eosWeb(stream, options, callback);
        }
        if (!isNodeStream(stream)) {
          throw codes.INVALID_ARG_TYPE("stream", ["ReadableStream", "WritableStream", "Stream"], stream);
        }
        const readable = options.readable ?? isReadableNodeStream(stream);
        const writable = options.writable ?? isWritableNodeStream(stream);
        let willEmitClose = _willEmitClose(stream) && isReadableNodeStream(stream) === readable && isWritableNodeStream(stream) === writable;
        let writableFinished = isWritableFinished(stream, false);
        let readableFinished = isReadableFinished(stream, false);
        const wState = stream._writableState;
        const rState = stream._readableState;
        let immediateResult;
        if (isClosed(stream)) {
          immediateResult = getEosOnCloseError(stream, readable, readableFinished, writable, writableFinished);
        } else if (wState?.errorEmitted || rState?.errorEmitted) {
          if (!willEmitClose) {
            immediateResult = getEosErrored(stream);
          }
        } else if (!readable && (!willEmitClose || isReadable(stream)) && (writableFinished || isWritable(stream) === false) && (wState == null || wState.pendingcb === undefined || wState.pendingcb === 0)) {
          immediateResult = getEosErrored(stream);
        } else if (!writable && (!willEmitClose || isWritable(stream)) && (readableFinished || isReadable(stream) === false)) {
          immediateResult = getEosErrored(stream);
        } else if (rState && stream.req && stream.aborted) {
          immediateResult = getEosErrored(stream);
        }
        let cleanup = () => {
          callback = nop;
        };
        if (immediateResult !== undefined) {
          if (options.error !== false) {
            stream.on("error", nop);
            cleanup = () => {
              callback = nop;
              stream.removeListener("error", nop);
            };
          }
        } else {
          const signal = options.signal;
          if (signal?.aborted) {
            immediateResult = makeAbortError(undefined, { cause: signal.reason });
          }
        }
        const invokeImmediate = () => {
          if (immediateResult === null) {
            callback.call(stream);
          } else {
            callback.call(stream, immediateResult);
          }
        };
        if (immediateResult !== undefined && options[kEosNodeSynchronousCallback]) {
          invokeImmediate();
          return cleanup;
        }
        if (hasAsyncContext()) {
          callback = bindAsyncResource(callback, "STREAM_END_OF_STREAM");
        }
        if (immediateResult !== undefined) {
          process.nextTick(invokeImmediate);
          return cleanup;
        }
        callback = once(callback);
        const onlegacyfinish = () => {
          if (!stream.writable) {
            onfinish();
          }
        };
        const onfinish = () => {
          writableFinished = true;
          if (stream.destroyed) {
            willEmitClose = false;
          }
          if (willEmitClose && (!stream.readable || readable)) {
            return;
          }
          if (!readable || readableFinished) {
            callback.call(stream);
          }
        };
        const onend = () => {
          readableFinished = true;
          if (stream.destroyed) {
            willEmitClose = false;
          }
          if (willEmitClose && (!stream.writable || writable)) {
            return;
          }
          if (!writable || writableFinished) {
            callback.call(stream);
          }
        };
        const onerror = (err) => {
          callback.call(stream, err);
        };
        const onclose = () => {
          const error = getEosOnCloseError(stream, readable, readableFinished, writable, writableFinished);
          if (error === null) {
            callback.call(stream);
          } else {
            callback.call(stream, error);
          }
        };
        const onrequest = () => {
          stream.req.on("finish", onfinish);
        };
        if (isRequest(stream)) {
          stream.on("complete", onfinish);
          if (!willEmitClose) {
            stream.on("abort", onclose);
          }
          if (stream.req) {
            onrequest();
          } else {
            stream.on("request", onrequest);
          }
        } else if (writable && !wState) {
          stream.on("end", onlegacyfinish);
          stream.on("close", onlegacyfinish);
        }
        if (!willEmitClose && typeof stream.aborted === "boolean") {
          stream.on("aborted", onclose);
        }
        stream.on("end", onend);
        stream.on("finish", onfinish);
        if (options.error !== false) {
          stream.on("error", onerror);
        }
        stream.on("close", onclose);
        cleanup = () => {
          callback = nop;
          stream.removeListener("aborted", onclose);
          stream.removeListener("complete", onfinish);
          stream.removeListener("abort", onclose);
          stream.removeListener("request", onrequest);
          const streamReq = stream.req;
          if (streamReq)
            streamReq.removeListener("finish", onfinish);
          stream.removeListener("end", onlegacyfinish);
          stream.removeListener("close", onlegacyfinish);
          stream.removeListener("finish", onfinish);
          stream.removeListener("end", onend);
          stream.removeListener("error", onerror);
          stream.removeListener("close", onclose);
        };
        const signal = options.signal;
        if (signal) {
          const abort = () => {
            const endCallback = callback;
            cleanup();
            endCallback.call(stream, makeAbortError(undefined, { cause: signal.reason }));
          };
          addAbortListener ??= require("internal/abort_listener").addAbortListener;
          const disposable = addAbortListener(signal, abort);
          const originalCallback = callback;
          callback = once((...args) => {
            disposable[SymbolDispose]();
            originalCallback.apply(stream, args);
          });
        }
        return cleanup;
      }
      function eosWeb(stream, options, callback) {
        if (hasAsyncContext()) {
          callback = once(bindAsyncResource(callback, "STREAM_END_OF_STREAM"));
        } else {
          callback = once(callback);
        }
        let isAborted = false;
        let abort = nop;
        const signal = options.signal;
        if (signal) {
          abort = () => {
            isAborted = true;
            callback.call(stream, makeAbortError(undefined, { cause: signal.reason }));
          };
          if (signal.aborted) {
            process.nextTick(abort);
          } else {
            addAbortListener ??= require("internal/abort_listener").addAbortListener;
            const disposable = addAbortListener(signal, abort);
            const originalCallback = callback;
            callback = once((...args) => {
              disposable[SymbolDispose]();
              originalCallback.apply(stream, args);
            });
          }
        }
        const resolverFn = (...args) => {
          if (!isAborted) {
            process.nextTick(() => callback.apply(stream, args));
          }
        };
        const closedPromise = stream[kIsClosedPromise]?.promise ?? webStreamClosedPromise(stream);
        PromisePrototypeThen.call(closedPromise, resolverFn, resolverFn);
        return nop;
      }
      function finished(stream, opts) {
        let autoCleanup = false;
        if (opts === null) {
          opts = kEmptyObject;
        }
        if (opts?.cleanup) {
          validateBoolean(opts.cleanup, "cleanup");
          autoCleanup = opts.cleanup;
        }
        return new Promise((resolve, reject) => {
          const cleanup = eos(stream, opts, (err) => {
            if (autoCleanup) {
              cleanup();
            }
            if (err) {
              reject(err);
            } else {
              resolve();
            }
          });
        });
      }
      eos.finished = finished;
      eos.kEosNodeSynchronousCallback = kEosNodeSynchronousCallback;
      module.exports = eos;

    },
    "internal/streams/from": function (module, _unusedExports, require) {
      "use strict";
      const SymbolIterator = Symbol.iterator;
      const SymbolAsyncIterator = Symbol.asyncIterator;
      const PromisePrototypeThen = Promise.prototype.then;
      const { aggregateTwoErrors } = require("internal/errors");
      function from(Readable, iterable, opts) {
        let iterator;
        if (typeof iterable === "string" || iterable instanceof Buffer) {
          return new Readable({
            objectMode: true,
            ...opts,
            read() {
              this.push(iterable);
              this.push(null);
            }
          });
        }
        let isAsync;
        if (iterable?.[SymbolAsyncIterator]) {
          isAsync = true;
          iterator = iterable[SymbolAsyncIterator]();
        } else if (iterable?.[SymbolIterator]) {
          isAsync = false;
          iterator = iterable[SymbolIterator]();
        } else {
          throw codes.INVALID_ARG_TYPE("iterable", ["Iterable"], iterable);
        }
        const readable = new Readable({
          objectMode: true,
          highWaterMark: 1,
          ...opts
        });
        let reading = false;
        let isAsyncValues = false;
        readable._read = function() {
          if (!reading) {
            reading = true;
            if (isAsync) {
              nextAsync();
            } else if (isAsyncValues) {
              nextSyncWithAsyncValues();
            } else {
              nextSyncWithSyncValues();
            }
          }
        };
        const originalDestroy = readable._destroy;
        readable._destroy = function(error, cb) {
          originalDestroy.call(this, error, (destroyError) => {
            const combinedError = destroyError || error;
            PromisePrototypeThen.call(close(combinedError), isCallable(cb) ? () => process.nextTick(cb, combinedError) : () => {}, isCallable(cb) ? (closeError) => process.nextTick(cb, aggregateTwoErrors(combinedError, closeError)) : () => {});
          });
        };
        async function close(error) {
          const hadError = error !== undefined && error !== null;
          const hasThrow = typeof iterator.throw === "function";
          if (hadError && hasThrow) {
            const { value, done } = await iterator.throw(error);
            await value;
            if (done) {
              return;
            }
          }
          if (typeof iterator.return === "function") {
            const { value } = await iterator.return();
            await value;
          }
        }
        function nextSyncWithSyncValues() {
          for (;; ) {
            try {
              const { value, done } = iterator.next();
              if (done) {
                readable.push(null);
                return;
              }
              if (value && typeof value.then === "function") {
                return changeToAsyncValues(value);
              }
              if (value === null) {
                reading = false;
                throw codes.STREAM_NULL_VALUES();
              }
              if (readable.push(value)) {
                continue;
              }
              reading = false;
            } catch (err) {
              readable.destroy(err);
            }
            break;
          }
        }
        async function changeToAsyncValues(value) {
          isAsyncValues = true;
          try {
            const res = await value;
            if (res === null) {
              reading = false;
              throw codes.STREAM_NULL_VALUES();
            }
            if (readable.push(res)) {
              nextSyncWithAsyncValues();
              return;
            }
            reading = false;
          } catch (err) {
            readable.destroy(err);
          }
        }
        async function nextSyncWithAsyncValues() {
          for (;; ) {
            try {
              const { value, done } = iterator.next();
              if (done) {
                readable.push(null);
                return;
              }
              const res = value && typeof value.then === "function" ? await value : value;
              if (res === null) {
                reading = false;
                throw codes.STREAM_NULL_VALUES();
              }
              if (readable.push(res)) {
                continue;
              }
              reading = false;
            } catch (err) {
              readable.destroy(err);
            }
            break;
          }
        }
        async function nextAsync() {
          for (;; ) {
            try {
              const { value, done } = await iterator.next();
              if (done) {
                readable.push(null);
                return;
              }
              if (value === null) {
                reading = false;
                throw codes.STREAM_NULL_VALUES();
              }
              if (readable.push(value)) {
                continue;
              }
              reading = false;
            } catch (err) {
              readable.destroy(err);
            }
            break;
          }
        }
        return readable;
      }
      module.exports = from;

    },
    "internal/streams/lazy_transform": function (module, _unusedExports, require) {
      "use strict";
      const Transform = require("internal/streams/transform");
      const ObjectDefineProperty = Object.defineProperty;
      const ObjectDefineProperties = Object.defineProperties;
      function LazyTransform(options) {
        this._options = options;
      }
      toClass(LazyTransform, "LazyTransform", Transform);
      function makeGetter(name) {
        return function() {
          Transform.call(this, this._options);
          this._writableState.decodeStrings = false;
          return this[name];
        };
      }
      function makeSetter(name) {
        return function(val) {
          ObjectDefineProperty(this, name, {
            __proto__: null,
            value: val,
            enumerable: true,
            configurable: true,
            writable: true
          });
        };
      }
      ObjectDefineProperties(LazyTransform.prototype, {
        _readableState: {
          __proto__: null,
          get: makeGetter("_readableState"),
          set: makeSetter("_readableState"),
          configurable: true,
          enumerable: true
        },
        _writableState: {
          __proto__: null,
          get: makeGetter("_writableState"),
          set: makeSetter("_writableState"),
          configurable: true,
          enumerable: true
        }
      });
      module.exports = LazyTransform;

    },
    "internal/streams/legacy": function (module, _unusedExports, require) {
      "use strict";
      const EE = require("node:events");
      const { isArrayBufferView, isUint8Array } = require("node:util/types");
      const ReflectOwnKeys = Reflect.ownKeys;
      const ArrayIsArray = Array.isArray;
      function Stream(opts) {
        EE.call(this, opts);
      }
      toClass(Stream, "Stream", EE);
      Stream.prototype.pipe = function(dest, options) {
        const source = this;
        function ondata(chunk) {
          if (dest.writable && dest.write(chunk) === false && source.pause) {
            source.pause();
          }
        }
        source.on("data", ondata);
        function ondrain() {
          if (source.readable && source.resume) {
            source.resume();
          }
        }
        dest.on("drain", ondrain);
        if (!dest._isStdio && (!options || options.end !== false)) {
          source.on("end", onend);
          source.on("close", onclose);
        }
        let didOnEnd = false;
        function onend() {
          if (didOnEnd)
            return;
          didOnEnd = true;
          dest.end();
        }
        function onclose() {
          if (didOnEnd)
            return;
          didOnEnd = true;
          if (typeof dest.destroy === "function")
            dest.destroy();
        }
        function onerror(er) {
          cleanup();
          if (this.listenerCount?.("error") === 0) {
            this.emit("error", er);
          }
        }
        prependListener(source, "error", onerror);
        prependListener(dest, "error", onerror);
        function cleanup() {
          source.removeListener("data", ondata);
          dest.removeListener("drain", ondrain);
          source.removeListener("end", onend);
          source.removeListener("close", onclose);
          source.removeListener("error", onerror);
          dest.removeListener("error", onerror);
          source.removeListener("end", cleanup);
          source.removeListener("close", cleanup);
          dest.removeListener("close", cleanup);
        }
        source.on("end", cleanup);
        source.on("close", cleanup);
        dest.on("close", cleanup);
        dest.emit("pipe", source);
        return dest;
      };
      Stream.prototype.eventNames = function eventNames() {
        const names = [];
        for (const key of ReflectOwnKeys(this._events)) {
          if (typeof this._events[key] === "function" || ArrayIsArray(this._events[key]) && this._events[key].length > 0) {
            names.push(key);
          }
        }
        return names;
      };
      function prependListener(emitter, event, fn) {
        if (typeof emitter.prependListener === "function")
          return emitter.prependListener(event, fn);
        let events, existing;
        if (!(events = emitter._events) || !(existing = events[event]))
          emitter.on(event, fn);
        else if (ArrayIsArray(existing))
          events[event] = [fn, ...existing];
        else
          events[event] = [fn, existing];
      }
      Stream._isArrayBufferView = isArrayBufferView;
      Stream._isUint8Array = isUint8Array;
      Stream._uint8ArrayToBuffer = function _uint8ArrayToBuffer(chunk) {
        return new (getBuffer())(chunk.buffer, chunk.byteOffset, chunk.byteLength);
      };
      module.exports = { Stream, prependListener };

    },
    "internal/streams/operators": function (module, _unusedExports, require) {
      "use strict";
      const { validateAbortSignal, validateFunction, validateInteger, validateObject } = require("internal/validators");
      const { kWeakHandler, resistStopPropagation } = require("internal/shared");
      const { finished } = require("internal/streams/end-of-stream");
      const MathFloor = Math.floor;
      const PromiseResolve = Promise.resolve.bind(Promise);
      const PromiseReject = Promise.reject.bind(Promise);
      const PromisePrototypeThen = Promise.prototype.then;
      const ArrayPrototypePush = Array.prototype.push;
      const NumberIsNaN = Number.isNaN;
      const ObjectDefineProperty = Object.defineProperty;
      const kEmpty = Symbol("kEmpty");
      const kEof = Symbol("kEof");
      function map(fn, options) {
        validateFunction(fn, "fn");
        if (options != null) {
          validateObject(options, "options");
        }
        if (options?.signal != null) {
          validateAbortSignal(options.signal, "options.signal");
        }
        let concurrency = 1;
        if (options?.concurrency != null) {
          concurrency = MathFloor(options.concurrency);
        }
        let highWaterMark = concurrency - 1;
        if (options?.highWaterMark != null) {
          highWaterMark = MathFloor(options.highWaterMark);
        }
        validateInteger(concurrency, "options.concurrency", 1);
        validateInteger(highWaterMark, "options.highWaterMark", 0);
        highWaterMark += concurrency;
        return async function* map() {
          const signal = AbortSignal.any([options?.signal].filter(Boolean));
          const stream = this;
          const queue = [];
          const signalOpt = { signal };
          let next;
          let resume;
          let done = false;
          let cnt = 0;
          function onCatch() {
            done = true;
            afterItemProcessed();
          }
          function afterItemProcessed() {
            cnt -= 1;
            maybeResume();
          }
          function maybeResume() {
            if (resume && !done && cnt < concurrency && queue.length < highWaterMark) {
              resume();
              resume = null;
            }
          }
          async function pump() {
            try {
              for await (let val of stream) {
                if (done) {
                  return;
                }
                if (signal.aborted) {
                  throw makeAbortError();
                }
                try {
                  val = fn(val, signalOpt);
                  if (val === kEmpty) {
                    continue;
                  }
                  val = PromiseResolve(val);
                } catch (err) {
                  val = PromiseReject(err);
                }
                cnt += 1;
                PromisePrototypeThen.call(val, afterItemProcessed, onCatch);
                queue.push(val);
                if (next) {
                  next();
                  next = null;
                }
                if (!done && (queue.length >= highWaterMark || cnt >= concurrency)) {
                  await new Promise((resolve) => {
                    resume = resolve;
                  });
                }
              }
              queue.push(kEof);
            } catch (err) {
              const val = PromiseReject(err);
              PromisePrototypeThen.call(val, afterItemProcessed, onCatch);
              queue.push(val);
            } finally {
              done = true;
              if (next) {
                next();
                next = null;
              }
            }
          }
          pump();
          try {
            while (true) {
              while (queue.length > 0) {
                const val = await queue[0];
                if (val === kEof) {
                  return;
                }
                if (signal.aborted) {
                  throw makeAbortError();
                }
                if (val !== kEmpty) {
                  yield val;
                }
                queue.shift();
                maybeResume();
              }
              await new Promise((resolve) => {
                next = resolve;
              });
            }
          } finally {
            done = true;
            if (resume) {
              resume();
              resume = null;
            }
          }
        }.call(this);
      }
      async function some(fn, options = undefined) {
        for await (const unused of filter.call(this, fn, options)) {
          return true;
        }
        return false;
      }
      async function every(fn, options = undefined) {
        validateFunction(fn, "fn");
        return !await some.call(this, async (...args) => {
          return !await fn(...args);
        }, options);
      }
      async function find(fn, options) {
        for await (const result of filter.call(this, fn, options)) {
          return result;
        }
        return;
      }
      async function forEach(fn, options) {
        validateFunction(fn, "fn");
        async function forEachFn(value, options) {
          await fn(value, options);
          return kEmpty;
        }
        for await (const unused of map.call(this, forEachFn, options))
          ;
      }
      function filter(fn, options) {
        validateFunction(fn, "fn");
        async function filterFn(value, options) {
          if (await fn(value, options)) {
            return value;
          }
          return kEmpty;
        }
        return map.call(this, filterFn, options);
      }

      class ReduceAwareErrMissingArgs extends TypeError {
        constructor() {
          super("reduce");
          this.code = "ERR_MISSING_ARGS";
          this.message = "Reduce of an empty stream requires an initial value";
        }
      }
      async function reduce(reducer, initialValue, options) {
        validateFunction(reducer, "reducer");
        if (options != null) {
          validateObject(options, "options");
        }
        if (options?.signal != null) {
          validateAbortSignal(options.signal, "options.signal");
        }
        let hasInitialValue = arguments.length > 1;
        if (options?.signal?.aborted) {
          const err = makeAbortError(undefined, { cause: options.signal.reason });
          this.once("error", () => {});
          await finished(this.destroy(err));
          throw err;
        }
        const ac = new AbortController;
        const signal = ac.signal;
        if (options?.signal) {
          const opts = resistStopPropagation({ once: true, [kWeakHandler]: this });
          options.signal.addEventListener("abort", () => ac.abort(), opts);
        }
        let gotAnyItemFromStream = false;
        try {
          for await (const value of this) {
            gotAnyItemFromStream = true;
            if (options?.signal?.aborted) {
              throw makeAbortError();
            }
            if (!hasInitialValue) {
              initialValue = value;
              hasInitialValue = true;
            } else {
              initialValue = await reducer(initialValue, value, { signal });
            }
          }
          if (!gotAnyItemFromStream && !hasInitialValue) {
            throw new ReduceAwareErrMissingArgs;
          }
        } finally {
          ac.abort();
        }
        return initialValue;
      }
      async function toArray(options) {
        if (options != null) {
          validateObject(options, "options");
        }
        if (options?.signal != null) {
          validateAbortSignal(options.signal, "options.signal");
        }
        const result = [];
        for await (const val of this) {
          if (options?.signal?.aborted) {
            throw makeAbortError(undefined, { cause: options.signal.reason });
          }
          ArrayPrototypePush.call(result, val);
        }
        return result;
      }
      function flatMap(fn, options) {
        const values = map.call(this, fn, options);
        async function* flatMapInner() {
          for await (const val of values) {
            yield* val;
          }
        }
        return flatMapInner.call(this);
      }
      function toIntegerOrInfinity(number) {
        number = Number(number);
        if (NumberIsNaN(number)) {
          return 0;
        }
        if (number < 0) {
          throw codes.OUT_OF_RANGE("number", ">= 0", number);
        }
        return number;
      }
      function drop(number, options) {
        if (options != null) {
          validateObject(options, "options");
        }
        if (options?.signal != null) {
          validateAbortSignal(options.signal, "options.signal");
        }
        number = toIntegerOrInfinity(number);
        return async function* drop() {
          if (options?.signal?.aborted) {
            throw makeAbortError();
          }
          for await (const val of this) {
            if (options?.signal?.aborted) {
              throw makeAbortError();
            }
            if (number-- <= 0) {
              yield val;
            }
          }
        }.call(this);
      }
      ObjectDefineProperty(drop, "length", { value: 1 });
      function take(number, options) {
        if (options != null) {
          validateObject(options, "options");
        }
        if (options?.signal != null) {
          validateAbortSignal(options.signal, "options.signal");
        }
        number = toIntegerOrInfinity(number);
        return async function* take() {
          if (options?.signal?.aborted) {
            throw makeAbortError();
          }
          for await (const val of this) {
            if (options?.signal?.aborted) {
              throw makeAbortError();
            }
            if (number-- > 0) {
              yield val;
            }
            if (number <= 0) {
              return;
            }
          }
        }.call(this);
      }
      ObjectDefineProperty(take, "length", { value: 1 });
      module.exports = {
        streamReturningOperators: {
          drop,
          filter,
          flatMap,
          map,
          take
        },
        promiseReturningOperators: {
          every,
          forEach,
          reduce,
          toArray,
          some,
          find
        }
      };

    },
    "internal/streams/passthrough": function (module, _unusedExports, require) {
      "use strict";
      const Transform = require("internal/streams/transform");
      function PassThrough(options) {
        if (!(this instanceof PassThrough))
          return new PassThrough(options);
        Transform.call(this, options);
      }
      toClass(PassThrough, "PassThrough", Transform);
      PassThrough.prototype._transform = function(chunk, encoding, cb) {
        cb(null, chunk);
      };
      module.exports = PassThrough;

    },
    "internal/streams/pipeline": function (module, _unusedExports, require) {
      "use strict";
      const eos = require("internal/streams/end-of-stream");
      const { once } = require("internal/shared");
      const destroyImpl = require("internal/streams/destroy");
      const Duplex = require("internal/streams/duplex");
      const { aggregateTwoErrors } = require("internal/errors");
      const { validateFunction, validateAbortSignal } = require("internal/validators");
      const {
        isIterable,
        isReadable,
        isReadableNodeStream,
        isNodeStream,
        isTransformStream,
        isWebStream,
        isReadableStream,
        isReadableFinished
      } = require("internal/streams/utils");
      const SymbolAsyncIterator = Symbol.asyncIterator;
      const ArrayIsArray = Array.isArray;
      const SymbolDispose = Symbol.dispose;
      let PassThrough;
      let Readable;
      let addAbortListener;
      function destroyer(stream, reading, writing) {
        let finished = false;
        stream.on("close", () => {
          finished = true;
        });
        const cleanup = eos(stream, { readable: reading, writable: writing }, (err) => {
          finished = !err;
        });
        return {
          destroy: (err) => {
            if (finished)
              return;
            finished = true;
            destroyImpl.destroyer(stream, err || codes.STREAM_DESTROYED("pipe"));
          },
          cleanup
        };
      }
      function popCallback(streams) {
        validateFunction(streams[streams.length - 1], "streams[stream.length - 1]");
        return streams.pop();
      }
      function makeAsyncIterable(val) {
        if (isIterable(val)) {
          return val;
        } else if (isReadableNodeStream(val)) {
          return fromReadable(val);
        }
        throw codes.INVALID_ARG_TYPE("val", ["Readable", "Iterable", "AsyncIterable"], val);
      }
      async function* fromReadable(val) {
        Readable ??= require("internal/streams/readable");
        yield* Readable.prototype[SymbolAsyncIterator].call(val);
      }
      async function pumpToNode(iterable, writable, finish, { end }) {
        let error;
        let onresolve = null;
        const resume = (err) => {
          if (err) {
            error = err;
          }
          if (onresolve) {
            const callback = onresolve;
            onresolve = null;
            callback();
          }
        };
        const wait = () => new Promise((resolve, reject) => {
          if (error) {
            reject(error);
          } else {
            onresolve = () => {
              if (error) {
                reject(error);
              } else {
                resolve();
              }
            };
          }
        });
        writable.on("drain", resume);
        const cleanup = eos(writable, { readable: false }, resume);
        try {
          if (writable.writableNeedDrain) {
            await wait();
          }
          for await (const chunk of iterable) {
            if (!writable.write(chunk)) {
              await wait();
            }
          }
          if (end) {
            writable.end();
            await wait();
          }
          finish();
        } catch (err) {
          finish(error !== err ? aggregateTwoErrors(error, err) : err);
        } finally {
          cleanup();
          writable.off("drain", resume);
        }
      }
      async function pumpToWeb(readable, writable, finish, { end }) {
        if (isTransformStream(writable)) {
          writable = writable.writable;
        }
        const writer = writable.getWriter();
        try {
          for await (const chunk of readable) {
            await writer.ready;
            writer.write(chunk).catch(() => {});
          }
          await writer.ready;
          if (end) {
            await writer.close();
          }
          finish();
        } catch (err) {
          try {
            await writer.abort(err);
            finish(err);
          } catch (err) {
            finish(err);
          }
        }
      }
      function pipeline(...streams) {
        return pipelineImpl(streams, once(popCallback(streams)));
      }
      function pipelineImpl(streams, callback, opts) {
        if (streams.length === 1 && ArrayIsArray(streams[0])) {
          streams = streams[0];
        }
        if (streams.length < 2) {
          throw codes.MISSING_ARGS("streams");
        }
        const ac = new AbortController;
        const signal = ac.signal;
        const outerSignal = opts?.signal;
        const lastStreamCleanup = [];
        validateAbortSignal(outerSignal, "options.signal");
        function abort() {
          finishImpl(makeAbortError(undefined, { cause: outerSignal?.reason }));
        }
        addAbortListener ??= require("internal/abort_listener").addAbortListener;
        let disposable;
        if (outerSignal) {
          disposable = addAbortListener(outerSignal, abort);
        }
        let error;
        let value;
        const destroys = [];
        let finishCount = 0;
        function finish(err) {
          finishImpl(err, --finishCount === 0);
        }
        function finishOnlyHandleError(err) {
          finishImpl(err, false);
        }
        function finishImpl(err, final) {
          if (err && (!error || error.code === "ERR_STREAM_PREMATURE_CLOSE" || error.name === "AbortError")) {
            error = err;
          }
          if (!error && !final) {
            return;
          }
          while (destroys.length) {
            destroys.shift()?.(error);
          }
          disposable?.[SymbolDispose]();
          ac.abort();
          if (final) {
            if (!error) {
              lastStreamCleanup.forEach((fn) => fn());
            }
            process.nextTick(callback, error, value);
          }
        }
        let ret;
        for (let i = 0;i < streams.length; i++) {
          const stream = streams[i];
          const reading = i < streams.length - 1;
          const writing = i > 0;
          const next = i + 1 < streams.length ? streams[i + 1] : null;
          const end = reading || opts?.end !== false;
          const isLastStream = i === streams.length - 1;
          if (isNodeStream(stream)) {
            let onError = function(err) {
              if (err && err.name !== "AbortError" && err.code !== "ERR_STREAM_PREMATURE_CLOSE") {
                finishOnlyHandleError(err);
              }
            };
            if (next !== null && (next?.closed || next?.destroyed)) {
              throw codes.STREAM_UNABLE_TO_PIPE();
            }
            if (end) {
              const { destroy, cleanup } = destroyer(stream, reading, writing);
              destroys.push(destroy);
              if (isReadable(stream) && isLastStream) {
                lastStreamCleanup.push(cleanup);
              }
            }
            stream.on("error", onError);
            if (isReadable(stream) && isLastStream) {
              lastStreamCleanup.push(() => {
                stream.removeListener("error", onError);
              });
            }
          }
          if (i === 0) {
            if (typeof stream === "function") {
              ret = stream({ signal });
              if (!isIterable(ret)) {
                throw codes.INVALID_RETURN_VALUE("Iterable, AsyncIterable or Stream", "source", ret);
              }
            } else if (isIterable(stream) || isReadableNodeStream(stream) || isTransformStream(stream)) {
              ret = stream;
            } else {
              ret = Duplex.from(stream);
            }
          } else if (typeof stream === "function") {
            if (isTransformStream(ret)) {
              ret = makeAsyncIterable(ret?.readable);
            } else {
              ret = makeAsyncIterable(ret);
            }
            ret = stream(ret, { signal });
            if (reading) {
              if (!isIterable(ret, true)) {
                throw codes.INVALID_RETURN_VALUE("AsyncIterable", `transform[${i - 1}]`, ret);
              }
            } else {
              PassThrough ??= require("internal/streams/passthrough");
              const pt = new PassThrough({
                objectMode: true
              });
              const then = ret?.then;
              if (typeof then === "function") {
                finishCount++;
                then.call(ret, (val) => {
                  value = val;
                  if (val != null) {
                    pt.write(val);
                  }
                  if (end) {
                    pt.end();
                  }
                  process.nextTick(finish);
                }, (err) => {
                  pt.destroy(err);
                  process.nextTick(finish, err);
                });
              } else if (isIterable(ret, true)) {
                finishCount++;
                pumpToNode(ret, pt, finish, { end });
              } else if (isReadableStream(ret) || isTransformStream(ret)) {
                const toRead = ret.readable || ret;
                finishCount++;
                pumpToNode(toRead, pt, finish, { end });
              } else {
                throw codes.INVALID_RETURN_VALUE("AsyncIterable or Promise", "destination", ret);
              }
              ret = pt;
              const { destroy, cleanup } = destroyer(ret, false, true);
              destroys.push(destroy);
              if (isLastStream) {
                lastStreamCleanup.push(cleanup);
              }
            }
          } else if (isNodeStream(stream)) {
            if (isReadableNodeStream(ret)) {
              finishCount += 2;
              const cleanup = pipe(ret, stream, finish, finishOnlyHandleError, { end });
              if (isReadable(stream) && isLastStream) {
                lastStreamCleanup.push(cleanup);
              }
            } else if (isTransformStream(ret) || isReadableStream(ret)) {
              const toRead = ret.readable || ret;
              finishCount++;
              pumpToNode(toRead, stream, finish, { end });
            } else if (isIterable(ret)) {
              finishCount++;
              pumpToNode(ret, stream, finish, { end });
            } else {
              throw codes.INVALID_ARG_TYPE("val", ["Readable", "Iterable", "AsyncIterable", "ReadableStream", "TransformStream"], ret);
            }
            ret = stream;
          } else if (isWebStream(stream)) {
            if (isReadableNodeStream(ret)) {
              finishCount++;
              pumpToWeb(makeAsyncIterable(ret), stream, finish, { end });
            } else if (isReadableStream(ret) || isIterable(ret)) {
              finishCount++;
              pumpToWeb(ret, stream, finish, { end });
            } else if (isTransformStream(ret)) {
              finishCount++;
              pumpToWeb(ret.readable, stream, finish, { end });
            } else {
              throw codes.INVALID_ARG_TYPE("val", ["Readable", "Iterable", "AsyncIterable", "ReadableStream", "TransformStream"], ret);
            }
            ret = stream;
          } else {
            ret = Duplex.from(stream);
          }
        }
        if (signal?.aborted || outerSignal?.aborted) {
          process.nextTick(abort);
        }
        return ret;
      }
      function pipe(src, dst, finish, finishOnlyHandleError, { end }) {
        let ended = false;
        dst.on("close", () => {
          if (!ended) {
            finishOnlyHandleError(codes.STREAM_PREMATURE_CLOSE());
          }
        });
        src.pipe(dst, { end: false });
        if (end) {
          let endFn = function() {
            ended = true;
            dst.end();
          };
          if (isReadableFinished(src)) {
            process.nextTick(endFn);
          } else {
            src.once("end", endFn);
          }
        } else {
          finish();
        }
        eos(src, { readable: true, writable: false }, (err) => {
          const rState = src._readableState;
          if (err && err.code === "ERR_STREAM_PREMATURE_CLOSE" && rState?.ended && !rState.errored && !rState.errorEmitted) {
            src.once("end", finish).once("error", finish);
          } else {
            finish(err);
          }
        });
        return eos(dst, { readable: false, writable: true }, finish);
      }
      module.exports = { pipelineImpl, pipeline };

    },
    "internal/streams/readable": function (module, _unusedExports, require) {
      "use strict";
      const EE = require("node:events");
      const { Stream, prependListener } = require("internal/streams/legacy");
      const { addAbortSignal, addAbortSignalNoValidate } = require("internal/streams/add-abort-signal");
      const eos = require("internal/streams/end-of-stream");
      const destroyImpl = require("internal/streams/destroy");
      const { getHighWaterMark, getDefaultHighWaterMark } = require("internal/streams/state");
      const {
        kOnConstructed,
        kState,
        kObjectMode,
        kErrorEmitted,
        kAutoDestroy,
        kEmitClose,
        kDestroyed,
        kClosed,
        kCloseEmitted,
        kErrored,
        kConstructed
      } = require("internal/streams/utils");
      const { aggregateTwoErrors } = require("internal/errors");
      const { validateAbortSignal, validateObject } = require("internal/validators");
      const { StringDecoder } = require("node:string_decoder");
      const from = require("internal/streams/from");
      const { SafeSet } = require("internal/primordials");
      const { kAutoDestroyed } = require("internal/shared");
      const ObjectDefineProperties = Object.defineProperties;
      const SymbolAsyncDispose = Symbol.asyncDispose;
      const NumberIsNaN = Number.isNaN;
      const NumberIsInteger = Number.isInteger;
      const NumberParseInt = Number.parseInt;
      const ArrayPrototypeIndexOf = Array.prototype.indexOf;
      const ObjectKeys = Object.keys;
      const SymbolAsyncIterator = Symbol.asyncIterator;
      const TypedArrayPrototypeSet = Uint8Array.prototype.set;
      const { errorOrDestroy } = destroyImpl;
      const nop = () => {};
      const kErroredValue = Symbol("kErroredValue");
      const kDefaultEncodingValue = Symbol("kDefaultEncodingValue");
      const kDecoderValue = Symbol("kDecoderValue");
      const kEncodingValue = Symbol("kEncodingValue");
      const kEnded = 1 << 9;
      const kEndEmitted = 1 << 10;
      const kReading = 1 << 11;
      const kSync = 1 << 12;
      const kNeedReadable = 1 << 13;
      const kEmittedReadable = 1 << 14;
      const kReadableListening = 1 << 15;
      const kResumeScheduled = 1 << 16;
      const kMultiAwaitDrain = 1 << 17;
      const kReadingMore = 1 << 18;
      const kDataEmitted = 1 << 19;
      const kDefaultUTF8Encoding = 1 << 20;
      const kDecoder = 1 << 21;
      const kEncoding = 1 << 22;
      const kHasFlowing = 1 << 23;
      const kFlowing = 1 << 24;
      const kHasPaused = 1 << 25;
      const kPaused = 1 << 26;
      const kDataListening = 1 << 27;
      function makeBitMapDescriptor(bit) {
        return {
          enumerable: false,
          get() {
            return (this[kState] & bit) !== 0;
          },
          set(value) {
            if (value)
              this[kState] |= bit;
            else
              this[kState] &= ~bit;
          }
        };
      }
      function ReadableState(options, stream, isDuplex) {
        this[kState] = kEmitClose | kAutoDestroy | kConstructed | kSync;
        if (options?.objectMode)
          this[kState] |= kObjectMode;
        if (isDuplex && options?.readableObjectMode)
          this[kState] |= kObjectMode;
        this.highWaterMark = options ? getHighWaterMark(this, options, "readableHighWaterMark", isDuplex) : getDefaultHighWaterMark(false);
        this.buffer = [];
        this.bufferIndex = 0;
        this.length = 0;
        this.pipes = [];
        if (options && options.emitClose === false)
          this[kState] &= ~kEmitClose;
        if (options && options.autoDestroy === false)
          this[kState] &= ~kAutoDestroy;
        const defaultEncoding = options?.defaultEncoding;
        if (defaultEncoding == null || defaultEncoding === "utf8" || defaultEncoding === "utf-8") {
          this[kState] |= kDefaultUTF8Encoding;
        } else if (Buffer.isEncoding(defaultEncoding)) {
          this.defaultEncoding = defaultEncoding;
        } else {
          throw codes.UNKNOWN_ENCODING(defaultEncoding);
        }
        this.awaitDrainWriters = null;
        if (options?.encoding) {
          this.decoder = new StringDecoder(options.encoding);
          this.encoding = options.encoding;
        }
      }
      ReadableState.prototype = {};
      ObjectDefineProperties(ReadableState.prototype, {
        objectMode: makeBitMapDescriptor(kObjectMode),
        ended: makeBitMapDescriptor(kEnded),
        endEmitted: makeBitMapDescriptor(kEndEmitted),
        reading: makeBitMapDescriptor(kReading),
        constructed: makeBitMapDescriptor(kConstructed),
        sync: makeBitMapDescriptor(kSync),
        needReadable: makeBitMapDescriptor(kNeedReadable),
        emittedReadable: makeBitMapDescriptor(kEmittedReadable),
        readableListening: makeBitMapDescriptor(kReadableListening),
        resumeScheduled: makeBitMapDescriptor(kResumeScheduled),
        errorEmitted: makeBitMapDescriptor(kErrorEmitted),
        emitClose: makeBitMapDescriptor(kEmitClose),
        autoDestroy: makeBitMapDescriptor(kAutoDestroy),
        destroyed: makeBitMapDescriptor(kDestroyed),
        closed: makeBitMapDescriptor(kClosed),
        closeEmitted: makeBitMapDescriptor(kCloseEmitted),
        multiAwaitDrain: makeBitMapDescriptor(kMultiAwaitDrain),
        readingMore: makeBitMapDescriptor(kReadingMore),
        dataEmitted: makeBitMapDescriptor(kDataEmitted),
        errored: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kErrored) !== 0 ? this[kErroredValue] : null;
          },
          set(value) {
            if (value) {
              this[kErroredValue] = value;
              this[kState] |= kErrored;
            } else {
              this[kState] &= ~kErrored;
            }
          }
        },
        defaultEncoding: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kDefaultUTF8Encoding) !== 0 ? "utf8" : this[kDefaultEncodingValue];
          },
          set(value) {
            if (value === "utf8" || value === "utf-8") {
              this[kState] |= kDefaultUTF8Encoding;
            } else {
              this[kState] &= ~kDefaultUTF8Encoding;
              this[kDefaultEncodingValue] = value;
            }
          }
        },
        decoder: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kDecoder) !== 0 ? this[kDecoderValue] : null;
          },
          set(value) {
            if (value) {
              this[kDecoderValue] = value;
              this[kState] |= kDecoder;
            } else {
              this[kState] &= ~kDecoder;
            }
          }
        },
        encoding: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kEncoding) !== 0 ? this[kEncodingValue] : null;
          },
          set(value) {
            if (value) {
              this[kEncodingValue] = value;
              this[kState] |= kEncoding;
            } else {
              this[kState] &= ~kEncoding;
            }
          }
        },
        flowing: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kHasFlowing) !== 0 ? (this[kState] & kFlowing) !== 0 : null;
          },
          set(value) {
            if (value == null) {
              this[kState] &= ~(kHasFlowing | kFlowing);
            } else if (value) {
              this[kState] |= kHasFlowing | kFlowing;
            } else {
              this[kState] |= kHasFlowing;
              this[kState] &= ~kFlowing;
            }
          }
        }
      });
      ReadableState.prototype[kOnConstructed] = function onConstructed(stream) {
        if ((this[kState] & kNeedReadable) !== 0) {
          maybeReadMore(stream, this);
        }
      };
      function Readable(options) {
        if (!(this instanceof Readable))
          return new Readable(options);
        this._events ??= {
          close: undefined,
          error: undefined,
          data: undefined,
          end: undefined,
          readable: undefined
        };
        this._readableState = new ReadableState(options, this, false);
        if (options) {
          const { read, destroy, construct, signal } = options;
          if (typeof read === "function")
            this._read = read;
          if (typeof destroy === "function")
            this._destroy = destroy;
          if (typeof construct === "function")
            this._construct = construct;
          if (signal)
            addAbortSignal(signal, this);
        }
        Stream.call(this, options);
        if (this._construct != null) {
          destroyImpl.construct(this, () => {
            this._readableState[kOnConstructed](this);
          });
        }
      }
      toClass(Readable, "Readable", Stream);
      Readable.ReadableState = ReadableState;
      Readable.prototype.destroy = destroyImpl.destroy;
      Readable.prototype._undestroy = destroyImpl.undestroy;
      Readable.prototype._destroy = function(err, cb) {
        cb(err);
      };
      Readable.prototype[EE.captureRejectionSymbol] = function(err) {
        this.destroy(err);
      };
      Readable.prototype[SymbolAsyncDispose] = function() {
        let error;
        if (!this.destroyed) {
          error = this.readableEnded ? null : makeAbortError();
          this.destroy(error);
        }
        return new Promise((resolve, reject) => eos(this, (err) => err && err !== error ? reject(err) : resolve(null)));
      };
      Readable.prototype.push = function(chunk, encoding) {
        debug("push", chunk);
        const state = this._readableState;
        return (state[kState] & kObjectMode) === 0 ? readableAddChunkPushByteMode(this, state, chunk, encoding) : readableAddChunkPushObjectMode(this, state, chunk, encoding);
      };
      Readable.prototype.unshift = function(chunk, encoding) {
        debug("unshift", chunk);
        const state = this._readableState;
        return (state[kState] & kObjectMode) === 0 ? readableAddChunkUnshiftByteMode(this, state, chunk, encoding) : readableAddChunkUnshiftObjectMode(this, state, chunk);
      };
      function readableAddChunkUnshiftByteMode(stream, state, chunk, encoding) {
        if (chunk === null) {
          state[kState] &= ~kReading;
          onEofChunk(stream, state);
          return false;
        }
        if (typeof chunk === "string") {
          encoding ||= state.defaultEncoding;
          const stateEncoding = state.encoding;
          if (stateEncoding !== encoding) {
            if (stateEncoding) {
              chunk = Buffer.from(chunk, encoding).toString(stateEncoding);
            } else {
              chunk = Buffer.from(chunk, encoding);
            }
          }
        } else if (Stream._isArrayBufferView(chunk)) {
          chunk = Stream._uint8ArrayToBuffer(chunk);
        } else if (chunk !== undefined && !(chunk instanceof Buffer)) {
          errorOrDestroy(stream, codes.INVALID_ARG_TYPE("chunk", ["string", "Buffer", "TypedArray", "DataView"], chunk));
          return false;
        }
        if (!(chunk && chunk.length > 0)) {
          return canPushMore(state);
        }
        return readableAddChunkUnshiftValue(stream, state, chunk);
      }
      function readableAddChunkUnshiftObjectMode(stream, state, chunk) {
        if (chunk === null) {
          state[kState] &= ~kReading;
          onEofChunk(stream, state);
          return false;
        }
        return readableAddChunkUnshiftValue(stream, state, chunk);
      }
      function readableAddChunkUnshiftValue(stream, state, chunk) {
        if ((state[kState] & kEndEmitted) !== 0)
          errorOrDestroy(stream, codes.STREAM_UNSHIFT_AFTER_END_EVENT());
        else if ((state[kState] & (kDestroyed | kErrored)) !== 0)
          return false;
        else
          addChunk(stream, state, chunk, true);
        return canPushMore(state);
      }
      function readableAddChunkPushByteMode(stream, state, chunk, encoding) {
        if (chunk === null) {
          state[kState] &= ~kReading;
          onEofChunk(stream, state);
          return false;
        }
        if (typeof chunk === "string") {
          encoding ||= state.defaultEncoding;
          if (state.encoding !== encoding) {
            chunk = Buffer.from(chunk, encoding);
            encoding = "";
          }
        } else if (chunk instanceof Buffer) {
          encoding = "";
        } else if (Stream._isArrayBufferView(chunk)) {
          chunk = Stream._uint8ArrayToBuffer(chunk);
          encoding = "";
        } else if (chunk !== undefined) {
          errorOrDestroy(stream, codes.INVALID_ARG_TYPE("chunk", ["string", "Buffer", "TypedArray", "DataView"], chunk));
          return false;
        }
        if (!chunk || chunk.length <= 0) {
          state[kState] &= ~kReading;
          maybeReadMore(stream, state);
          return canPushMore(state);
        }
        if ((state[kState] & kEnded) !== 0) {
          errorOrDestroy(stream, codes.STREAM_PUSH_AFTER_EOF());
          return false;
        }
        if ((state[kState] & (kDestroyed | kErrored)) !== 0) {
          return false;
        }
        state[kState] &= ~kReading;
        if ((state[kState] & kDecoder) !== 0 && !encoding) {
          chunk = state[kDecoderValue].write(chunk);
          if (chunk.length === 0) {
            maybeReadMore(stream, state);
            return canPushMore(state);
          }
        }
        addChunk(stream, state, chunk, false);
        return canPushMore(state);
      }
      function readableAddChunkPushObjectMode(stream, state, chunk, encoding) {
        if (chunk === null) {
          state[kState] &= ~kReading;
          onEofChunk(stream, state);
          return false;
        }
        if ((state[kState] & kEnded) !== 0) {
          errorOrDestroy(stream, codes.STREAM_PUSH_AFTER_EOF());
          return false;
        }
        if ((state[kState] & (kDestroyed | kErrored)) !== 0) {
          return false;
        }
        state[kState] &= ~kReading;
        if ((state[kState] & kDecoder) !== 0 && !encoding) {
          chunk = state[kDecoderValue].write(chunk);
        }
        addChunk(stream, state, chunk, false);
        return canPushMore(state);
      }
      function canPushMore(state) {
        return (state[kState] & kEnded) === 0 && (state.length < state.highWaterMark || state.length === 0);
      }
      function addChunk(stream, state, chunk, addToFront) {
        if ((state[kState] & (kFlowing | kSync | kDataListening)) === (kFlowing | kDataListening) && state.length === 0) {
          if ((state[kState] & kMultiAwaitDrain) !== 0) {
            state.awaitDrainWriters.clear();
          } else {
            state.awaitDrainWriters = null;
          }
          state[kState] |= kDataEmitted;
          stream.emit("data", chunk);
        } else {
          state.length += (state[kState] & kObjectMode) !== 0 ? 1 : chunk.length;
          if (addToFront) {
            if (state.bufferIndex > 0) {
              state.buffer[--state.bufferIndex] = chunk;
            } else {
              state.buffer.unshift(chunk);
            }
          } else {
            state.buffer.push(chunk);
          }
          if ((state[kState] & kNeedReadable) !== 0)
            emitReadable(stream);
        }
        maybeReadMore(stream, state);
      }
      Readable.prototype.isPaused = function() {
        const state = this._readableState;
        return (state[kState] & kPaused) !== 0 || (state[kState] & (kHasFlowing | kFlowing)) === kHasFlowing;
      };
      Readable.prototype.setEncoding = function(enc) {
        const state = this._readableState;
        const decoder = new StringDecoder(enc);
        state.decoder = decoder;
        state.encoding = state.decoder.encoding;
        let content = "";
        for (const data of state.buffer.slice(state.bufferIndex)) {
          content += decoder.write(data);
        }
        state.buffer.length = 0;
        state.bufferIndex = 0;
        if (content !== "")
          state.buffer.push(content);
        state.length = content.length;
        return this;
      };
      const MAX_HWM = 1073741824;
      function computeNewHighWaterMark(n) {
        if (n > MAX_HWM) {
          throw codes.OUT_OF_RANGE("size", "<= 1GiB", n);
        } else {
          n--;
          n |= n >>> 1;
          n |= n >>> 2;
          n |= n >>> 4;
          n |= n >>> 8;
          n |= n >>> 16;
          n++;
        }
        return n;
      }
      function howMuchToRead(n, state) {
        if (n <= 0 || state.length === 0 && (state[kState] & kEnded) !== 0)
          return 0;
        if ((state[kState] & kObjectMode) !== 0)
          return 1;
        if (NumberIsNaN(n)) {
          if ((state[kState] & kDecoder) === 0 && state.length)
            return state.buffer[state.bufferIndex].length;
          if ((state[kState] & kFlowing) !== 0 && state.length)
            return state.buffer[state.bufferIndex].length;
          return state.length;
        }
        if (n <= state.length)
          return n;
        return (state[kState] & kEnded) !== 0 ? state.length : 0;
      }
      Readable.prototype.read = function(n) {
        debug("read", n);
        if (n === undefined) {
          n = NaN;
        } else if (!NumberIsInteger(n)) {
          n = NumberParseInt(n, 10);
        }
        const state = this._readableState;
        const nOrig = n;
        if (n > state.highWaterMark)
          state.highWaterMark = computeNewHighWaterMark(n);
        if (n !== 0)
          state[kState] &= ~kEmittedReadable;
        const stateLength = state.length;
        if (n === 0 && (state[kState] & kNeedReadable) !== 0 && ((state.highWaterMark !== 0 ? stateLength >= state.highWaterMark : stateLength > 0) || (state[kState] & kEnded) !== 0)) {
          debug("read: emitReadable");
          if (stateLength === 0 && (state[kState] & kEnded) !== 0)
            endReadable(this);
          else
            emitReadable(this);
          return null;
        }
        n = howMuchToRead(n, state);
        if (n === 0 && (state[kState] & kEnded) !== 0) {
          if (state.length === 0)
            endReadable(this);
          return null;
        }
        let doRead = (state[kState] & kNeedReadable) !== 0;
        debug("need readable", doRead);
        if (state.length === 0 || state.length - n < state.highWaterMark) {
          doRead = true;
          debug("length less than watermark", doRead);
        }
        if ((state[kState] & (kReading | kEnded | kDestroyed | kErrored | kConstructed)) !== kConstructed) {
          doRead = false;
          debug("reading, ended or constructing", doRead);
        } else if (doRead) {
          debug("do read");
          state[kState] |= kReading | kSync;
          if (state.length === 0)
            state[kState] |= kNeedReadable;
          try {
            this._read(state.highWaterMark);
          } catch (err) {
            errorOrDestroy(this, err);
          }
          state[kState] &= ~kSync;
          if ((state[kState] & kReading) === 0)
            n = howMuchToRead(nOrig, state);
        }
        let ret;
        if (n > 0)
          ret = fromList(n, state);
        else
          ret = null;
        if (ret === null) {
          state[kState] |= state.length <= state.highWaterMark ? kNeedReadable : 0;
          n = 0;
        } else {
          state.length -= n;
          if ((state[kState] & kMultiAwaitDrain) !== 0) {
            state.awaitDrainWriters.clear();
          } else {
            state.awaitDrainWriters = null;
          }
        }
        if (state.length === 0) {
          if ((state[kState] & kEnded) === 0)
            state[kState] |= kNeedReadable;
          if (nOrig !== n && (state[kState] & kEnded) !== 0)
            endReadable(this);
        }
        if (ret !== null && (state[kState] & (kErrorEmitted | kCloseEmitted)) === 0) {
          state[kState] |= kDataEmitted;
          this.emit("data", ret);
        }
        return ret;
      };
      function onEofChunk(stream, state) {
        debug("onEofChunk");
        if ((state[kState] & kEnded) !== 0)
          return;
        const decoder = (state[kState] & kDecoder) !== 0 ? state[kDecoderValue] : null;
        if (decoder) {
          const chunk = decoder.end();
          if (chunk?.length) {
            state.buffer.push(chunk);
            state.length += (state[kState] & kObjectMode) !== 0 ? 1 : chunk.length;
          }
        }
        state[kState] |= kEnded;
        if ((state[kState] & kSync) !== 0) {
          emitReadable(stream);
        } else {
          state[kState] &= ~kNeedReadable;
          state[kState] |= kEmittedReadable;
          emitReadable_(stream);
        }
      }
      function emitReadable(stream) {
        const state = stream._readableState;
        debug("emitReadable");
        state[kState] &= ~kNeedReadable;
        if ((state[kState] & kEmittedReadable) === 0) {
          debug("emitReadable", (state[kState] & kFlowing) !== 0);
          state[kState] |= kEmittedReadable;
          process.nextTick(emitReadable_, stream);
        }
      }
      function emitReadable_(stream) {
        const state = stream._readableState;
        debug("emitReadable_");
        if ((state[kState] & (kDestroyed | kErrored)) === 0 && (state.length || (state[kState] & kEnded) !== 0)) {
          stream.emit("readable");
          state[kState] &= ~kEmittedReadable;
        }
        state[kState] |= (state[kState] & (kFlowing | kEnded)) === 0 && state.length <= state.highWaterMark ? kNeedReadable : 0;
        flow(stream);
      }
      function maybeReadMore(stream, state) {
        if ((state[kState] & (kReadingMore | kReading | kConstructed)) === kConstructed) {
          state[kState] |= kReadingMore;
          process.nextTick(maybeReadMore_, stream, state);
        }
      }
      function maybeReadMore_(stream, state) {
        while ((state[kState] & (kReading | kEnded)) === 0 && (state.length < state.highWaterMark || (state[kState] & kFlowing) !== 0 && state.length === 0)) {
          const len = state.length;
          debug("maybeReadMore read 0");
          stream.read(0);
          if (len === state.length)
            break;
        }
        state[kState] &= ~kReadingMore;
      }
      Readable.prototype._read = function(_n) {
        throw codes.METHOD_NOT_IMPLEMENTED("_read()");
      };
      Readable.prototype.pipe = function(dest, pipeOpts) {
        const src = this;
        const state = this._readableState;
        if (state.pipes.length === 1) {
          if ((state[kState] & kMultiAwaitDrain) === 0) {
            state[kState] |= kMultiAwaitDrain;
            state.awaitDrainWriters = new SafeSet(state.awaitDrainWriters ? [state.awaitDrainWriters] : []);
          }
        }
        state.pipes.push(dest);
        debug("pipe count=%d opts=%j", state.pipes.length, pipeOpts);
        const doEnd = (!pipeOpts || pipeOpts.end !== false) && dest !== process.stdout && dest !== process.stderr;
        const endFn = doEnd ? onend : unpipe;
        if ((state[kState] & kEndEmitted) !== 0)
          process.nextTick(endFn);
        else
          src.once("end", endFn);
        dest.on("unpipe", onunpipe);
        function onunpipe(readable, unpipeInfo) {
          debug("onunpipe");
          if (readable === src) {
            if (unpipeInfo && unpipeInfo.hasUnpiped === false) {
              unpipeInfo.hasUnpiped = true;
              cleanup();
            }
          }
        }
        function onend() {
          debug("onend");
          dest.end();
        }
        let ondrain;
        let cleanedUp = false;
        function cleanup() {
          debug("cleanup");
          dest.removeListener("close", onclose);
          dest.removeListener("finish", onfinish);
          if (ondrain) {
            dest.removeListener("drain", ondrain);
          }
          dest.removeListener("error", onerror);
          dest.removeListener("unpipe", onunpipe);
          src.removeListener("end", onend);
          src.removeListener("end", unpipe);
          src.removeListener("data", ondata);
          cleanedUp = true;
          if (ondrain && state.awaitDrainWriters && (!dest._writableState || dest._writableState.needDrain))
            ondrain();
        }
        function pause() {
          if (!cleanedUp) {
            if (state.pipes.length === 1 && state.pipes[0] === dest) {
              debug("false write response, pause", 0);
              state.awaitDrainWriters = dest;
              state[kState] &= ~kMultiAwaitDrain;
            } else if (state.pipes.length > 1 && state.pipes.includes(dest)) {
              debug("false write response, pause", state.awaitDrainWriters.size);
              state.awaitDrainWriters.add(dest);
            }
            src.pause();
          }
          if (!ondrain) {
            ondrain = pipeOnDrain(src, dest);
            dest.on("drain", ondrain);
          }
        }
        src.on("data", ondata);
        function ondata(chunk) {
          debug("ondata");
          try {
            const ret = dest.write(chunk);
            debug("dest.write", ret);
            if (ret === false) {
              pause();
            }
          } catch (err) {
            dest.destroy(err);
          }
        }
        function onerror(er) {
          debug("onerror", er);
          unpipe();
          dest.removeListener("error", onerror);
          if (dest.listenerCount("error") === 0) {
            const s = dest._writableState || dest._readableState;
            if (s && !s.errorEmitted) {
              errorOrDestroy(dest, er);
            } else {
              dest.emit("error", er);
            }
          }
        }
        prependListener(dest, "error", onerror);
        function onclose() {
          dest.removeListener("finish", onfinish);
          unpipe();
        }
        dest.once("close", onclose);
        function onfinish() {
          debug("onfinish");
          dest.removeListener("close", onclose);
          unpipe();
        }
        dest.once("finish", onfinish);
        function unpipe() {
          debug("unpipe");
          src.unpipe(dest);
        }
        dest.emit("pipe", src);
        if (dest.writableNeedDrain === true) {
          pause();
        } else if ((state[kState] & kFlowing) === 0) {
          debug("pipe resume");
          src.resume();
        }
        return dest;
      };
      function pipeOnDrain(src, dest) {
        return function pipeOnDrainFunctionResult() {
          const state = src._readableState;
          if (state.awaitDrainWriters === dest) {
            debug("pipeOnDrain", 1);
            state.awaitDrainWriters = null;
          } else if ((state[kState] & kMultiAwaitDrain) !== 0) {
            debug("pipeOnDrain", state.awaitDrainWriters.size);
            state.awaitDrainWriters.delete(dest);
          }
          if ((!state.awaitDrainWriters || state.awaitDrainWriters.size === 0) && (state[kState] & kDataListening) !== 0) {
            src.resume();
          }
        };
      }
      Readable.prototype.unpipe = function(dest) {
        const state = this._readableState;
        const unpipeInfo = { hasUnpiped: false };
        if (state.pipes.length === 0)
          return this;
        if (!dest) {
          const dests = state.pipes;
          state.pipes = [];
          this.pause();
          for (let i = 0;i < dests.length; i++)
            dests[i].emit("unpipe", this, { hasUnpiped: false });
          return this;
        }
        const index = ArrayPrototypeIndexOf.call(state.pipes, dest);
        if (index === -1)
          return this;
        state.pipes.splice(index, 1);
        if (state.pipes.length === 0)
          this.pause();
        dest.emit("unpipe", this, unpipeInfo);
        return this;
      };
      Readable.prototype.on = function(ev, fn) {
        const res = Stream.prototype.on.call(this, ev, fn);
        const state = this._readableState;
        if (ev === "data") {
          state[kState] |= kDataListening;
          state[kState] |= this.listenerCount("readable") > 0 ? kReadableListening : 0;
          if ((state[kState] & (kHasFlowing | kFlowing)) !== kHasFlowing) {
            this.resume();
          }
        } else if (ev === "readable") {
          if ((state[kState] & (kEndEmitted | kReadableListening)) === 0) {
            state[kState] |= kReadableListening | kNeedReadable | kHasFlowing;
            state[kState] &= ~(kFlowing | kEmittedReadable);
            debug("on readable");
            if (state.length) {
              emitReadable(this);
            } else if ((state[kState] & kReading) === 0) {
              process.nextTick(nReadingNextTick, this);
            }
          }
        }
        return res;
      };
      Readable.prototype.addListener = Readable.prototype.on;
      Readable.prototype.removeListener = function(ev, fn) {
        const state = this._readableState;
        const res = Stream.prototype.removeListener.call(this, ev, fn);
        if (ev === "readable") {
          process.nextTick(updateReadableListening, this);
        } else if (ev === "data" && this.listenerCount("data") === 0) {
          state[kState] &= ~kDataListening;
        }
        return res;
      };
      Readable.prototype.off = Readable.prototype.removeListener;
      Readable.prototype.removeAllListeners = function(ev) {
        const res = Stream.prototype.removeAllListeners.apply(this, arguments);
        if (ev === "readable" || ev === undefined) {
          process.nextTick(updateReadableListening, this);
        }
        return res;
      };
      function updateReadableListening(self) {
        const state = self._readableState;
        if (self.listenerCount("readable") > 0) {
          state[kState] |= kReadableListening;
        } else {
          state[kState] &= ~kReadableListening;
        }
        if ((state[kState] & (kHasPaused | kPaused | kResumeScheduled)) === (kHasPaused | kResumeScheduled)) {
          state[kState] |= kHasFlowing | kFlowing;
        } else if ((state[kState] & kDataListening) !== 0) {
          self.resume();
        } else if ((state[kState] & kReadableListening) === 0) {
          state[kState] &= ~(kHasFlowing | kFlowing);
        }
      }
      function nReadingNextTick(self) {
        debug("readable nexttick read 0");
        self.read(0);
      }
      Readable.prototype.resume = function() {
        const state = this._readableState;
        if ((state[kState] & (kDestroyed | kEndEmitted)) === (kDestroyed | kEndEmitted)) {
          return this;
        }
        if ((state[kState] & kFlowing) === 0) {
          debug("resume");
          state[kState] |= kHasFlowing;
          if ((state[kState] & kReadableListening) === 0) {
            state[kState] |= kFlowing;
          } else {
            state[kState] &= ~kFlowing;
          }
          resume(this, state);
        }
        state[kState] |= kHasPaused;
        state[kState] &= ~kPaused;
        return this;
      };
      function resume(stream, state) {
        if ((state[kState] & kResumeScheduled) === 0) {
          state[kState] |= kResumeScheduled;
          process.nextTick(resume_, stream, state);
        }
      }
      function resume_(stream, state) {
        debug("resume", (state[kState] & kReading) !== 0);
        if ((state[kState] & kReading) === 0) {
          stream.read(0);
        }
        state[kState] &= ~kResumeScheduled;
        stream.emit("resume");
        flow(stream);
        if ((state[kState] & (kFlowing | kReading)) === kFlowing)
          stream.read(0);
      }
      Readable.prototype.pause = function() {
        const state = this._readableState;
        if ((state[kState] & kDestroyed) !== 0) {
          return this;
        }
        debug("call pause");
        if ((state[kState] & (kHasFlowing | kFlowing)) !== kHasFlowing) {
          debug("pause");
          state[kState] |= kHasFlowing;
          state[kState] &= ~kFlowing;
          this.emit("pause");
        }
        state[kState] |= kHasPaused | kPaused;
        return this;
      };
      function flow(stream) {
        const state = stream._readableState;
        debug("flow");
        while ((state[kState] & kFlowing) !== 0 && stream.read() !== null)
          ;
      }
      Readable.prototype.wrap = function(stream) {
        let paused = false;
        stream.on("data", (chunk) => {
          if (!this.push(chunk) && stream.pause) {
            paused = true;
            stream.pause();
          }
        });
        stream.on("end", () => {
          this.push(null);
        });
        stream.on("error", (err) => {
          errorOrDestroy(this, err);
        });
        stream.on("close", () => {
          this.destroy();
        });
        stream.on("destroy", () => {
          this.destroy();
        });
        this._read = () => {
          if (paused && stream.resume) {
            paused = false;
            stream.resume();
          }
        };
        const streamKeys = ObjectKeys(stream);
        for (let j = 1;j < streamKeys.length; j++) {
          const i = streamKeys[j];
          if (this[i] === undefined && typeof stream[i] === "function") {
            this[i] = stream[i].bind(stream);
          }
        }
        return this;
      };
      Readable.prototype[SymbolAsyncIterator] = function() {
        return streamToAsyncIterator(this);
      };
      Readable.prototype.iterator = function(options) {
        if (options !== undefined) {
          validateObject(options, "options");
        }
        return streamToAsyncIterator(this, options);
      };
      {
        const toAsyncStreamable = Symbol.for("Stream.toAsyncStreamable");
        let createBatchedAsyncIterator;
        let normalizeBatch;
        let kValidatedSource;
        Readable.prototype[toAsyncStreamable] = function() {
          if (createBatchedAsyncIterator === undefined) {
            if (!false) {
              throw codes.STREAM_ITER_MISSING_FLAG();
            }
            ({ createBatchedAsyncIterator, normalizeBatch } = require("internal/streams/iter/classic"));
            ({ kValidatedSource } = require("internal/streams/iter/types"));
          }
          const state = this._readableState;
          const normalize = state.objectMode || state.encoding ? normalizeBatch : null;
          const iter = createBatchedAsyncIterator(this, normalize);
          iter[kValidatedSource] = true;
          iter.stream = this;
          return iter;
        };
      }
      let composeImpl;
      Readable.prototype.compose = function compose(stream, options) {
        if (options != null) {
          validateObject(options, "options");
        }
        if (options?.signal != null) {
          validateAbortSignal(options.signal, "options.signal");
        }
        composeImpl ??= require("internal/streams/compose");
        const composedStream = composeImpl(this, stream);
        if (options?.signal) {
          addAbortSignalNoValidate(options.signal, composedStream);
        }
        return composedStream;
      };
      function streamToAsyncIterator(stream, options) {
        if (typeof stream.read !== "function") {
          stream = Readable.wrap(stream, { objectMode: true });
        }
        const iter = createAsyncIterator(stream, options);
        iter.stream = stream;
        return iter;
      }
      async function* createAsyncIterator(stream, options) {
        let callback = nop;
        function next(resolve) {
          if (this === stream) {
            callback();
            callback = nop;
          } else {
            callback = resolve;
          }
        }
        stream.on("readable", next);
        let error;
        const cleanup = eos(stream, { writable: false }, (err) => {
          error = err ? aggregateTwoErrors(error, err) : null;
          callback();
          callback = nop;
        });
        try {
          while (true) {
            const chunk = stream.destroyed ? null : stream.read();
            if (chunk !== null) {
              yield chunk;
            } else if (error) {
              throw error;
            } else if (error === null) {
              return;
            } else {
              await new Promise(next);
            }
          }
        } catch (err) {
          error = aggregateTwoErrors(error, err);
          throw error;
        } finally {
          if ((error || options?.destroyOnReturn !== false) && (error === undefined || stream._readableState.autoDestroy)) {
            destroyImpl.destroyer(stream, null);
          } else {
            stream.off("readable", next);
            cleanup();
          }
        }
      }
      ObjectDefineProperties(Readable.prototype, {
        readable: {
          __proto__: null,
          get() {
            const r = this._readableState;
            return !!r && r.readable !== false && !r.destroyed && !r.errorEmitted && !r.endEmitted;
          },
          set(val) {
            const state = this._readableState;
            if (state) {
              state.readable = !!val;
            }
          }
        },
        readableDidRead: {
          __proto__: null,
          enumerable: false,
          get: function() {
            return this._readableState.dataEmitted;
          }
        },
        readableAborted: {
          __proto__: null,
          enumerable: false,
          get: function() {
            return !!(this._readableState.readable !== false && (this._readableState.destroyed || this._readableState.errored) && !this._readableState.endEmitted);
          }
        },
        readableHighWaterMark: {
          __proto__: null,
          enumerable: false,
          get: function() {
            return this._readableState.highWaterMark;
          }
        },
        readableBuffer: {
          __proto__: null,
          enumerable: false,
          get: function() {
            return this._readableState?.buffer;
          }
        },
        readableFlowing: {
          __proto__: null,
          enumerable: false,
          get: function() {
            return this._readableState.flowing;
          },
          set: function(state) {
            const readableState = this._readableState;
            if (readableState) {
              readableState.flowing = state;
            }
          }
        },
        readableLength: {
          __proto__: null,
          enumerable: false,
          get() {
            return this._readableState.length;
          }
        },
        readableObjectMode: {
          __proto__: null,
          enumerable: false,
          get() {
            return this._readableState ? this._readableState.objectMode : false;
          }
        },
        readableEncoding: {
          __proto__: null,
          enumerable: false,
          get() {
            return this._readableState ? this._readableState.encoding : null;
          }
        },
        errored: {
          __proto__: null,
          enumerable: false,
          get() {
            return this._readableState ? this._readableState.errored : null;
          }
        },
        closed: {
          __proto__: null,
          get() {
            return this._readableState ? this._readableState.closed : false;
          }
        },
        destroyed: {
          __proto__: null,
          enumerable: false,
          get() {
            return this._readableState ? this._readableState.destroyed : false;
          },
          set(value) {
            if (!this._readableState) {
              return;
            }
            this._readableState.destroyed = value;
          }
        },
        readableEnded: {
          __proto__: null,
          enumerable: false,
          get() {
            return this._readableState ? this._readableState.endEmitted : false;
          }
        }
      });
      ObjectDefineProperties(ReadableState.prototype, {
        pipesCount: {
          __proto__: null,
          get() {
            return this.pipes.length;
          }
        },
        paused: {
          __proto__: null,
          get() {
            return (this[kState] & kPaused) !== 0;
          },
          set(value) {
            this[kState] |= kHasPaused;
            if (value) {
              this[kState] |= kPaused;
            } else {
              this[kState] &= ~kPaused;
            }
          }
        }
      });
      Readable._fromList = fromList;
      function fromList(n, state) {
        if (state.length === 0)
          return null;
        let idx = state.bufferIndex;
        let ret;
        const buf = state.buffer;
        const len = buf.length;
        let stateLength;
        if ((state[kState] & kObjectMode) !== 0) {
          ret = buf[idx];
          buf[idx++] = null;
        } else if (!n || n >= (stateLength = state.length)) {
          if ((state[kState] & kDecoder) !== 0) {
            ret = "";
            while (idx < len) {
              ret += buf[idx];
              buf[idx++] = null;
            }
          } else if (len - idx === 0) {
            ret = Buffer.alloc(0);
          } else if (len - idx === 1) {
            ret = buf[idx];
            buf[idx++] = null;
          } else {
            ret = Buffer.allocUnsafe(stateLength ?? state.length);
            let i = 0;
            while (idx < len) {
              TypedArrayPrototypeSet.call(ret, buf[idx], i);
              i += buf[idx].length;
              buf[idx++] = null;
            }
          }
        } else if (n < buf[idx].length) {
          ret = buf[idx].slice(0, n);
          buf[idx] = buf[idx].slice(n);
        } else if (n === buf[idx].length) {
          ret = buf[idx];
          buf[idx++] = null;
        } else if ((state[kState] & kDecoder) !== 0) {
          ret = "";
          while (idx < len) {
            const str = buf[idx];
            const strLength = str.length;
            if (n > strLength) {
              ret += str;
              n -= strLength;
              buf[idx++] = null;
            } else {
              if (n === strLength) {
                ret += str;
                buf[idx++] = null;
              } else {
                ret += str.slice(0, n);
                buf[idx] = str.slice(n);
              }
              break;
            }
          }
        } else {
          ret = Buffer.allocUnsafe(n);
          const retLen = n;
          while (idx < len) {
            const data = buf[idx];
            const dataLength = data.length;
            if (n > dataLength) {
              TypedArrayPrototypeSet.call(ret, data, retLen - n);
              n -= dataLength;
              buf[idx++] = null;
            } else {
              if (n === dataLength) {
                TypedArrayPrototypeSet.call(ret, data, retLen - n);
                buf[idx++] = null;
              } else {
                TypedArrayPrototypeSet.call(ret, new (getBuffer())(data.buffer, data.byteOffset, n), retLen - n);
                buf[idx] = new (getBuffer())(data.buffer, data.byteOffset + n, dataLength - n);
              }
              break;
            }
          }
        }
        if (idx === len) {
          state.buffer.length = 0;
          state.bufferIndex = 0;
        } else if (idx > 1024) {
          state.buffer.splice(0, idx);
          state.bufferIndex = 0;
        } else {
          state.bufferIndex = idx;
        }
        return ret;
      }
      function endReadable(stream) {
        const state = stream._readableState;
        debug("endReadable");
        if ((state[kState] & kEndEmitted) === 0) {
          state[kState] |= kEnded;
          process.nextTick(endReadableNT, state, stream);
        }
      }
      function endReadableNT(state, stream) {
        debug("endReadableNT");
        if ((state[kState] & (kErrored | kCloseEmitted | kEndEmitted)) === 0 && state.length === 0) {
          state[kState] |= kEndEmitted;
          stream.emit("end");
          if (stream.writable && stream.allowHalfOpen === false) {
            process.nextTick(endWritableNT, stream);
          } else if (state.autoDestroy) {
            const wState = stream._writableState;
            const autoDestroy = !wState || wState.autoDestroy && (wState.finished || wState.writable === false);
            if (autoDestroy) {
              stream[kAutoDestroyed] = true;
              stream.destroy();
            }
          }
        }
      }
      function endWritableNT(stream) {
        const writable = stream.writable && !stream.writableEnded && !stream.destroyed;
        if (writable) {
          stream.end();
        }
      }
      Readable.from = function(iterable, opts) {
        return from(Readable, iterable, opts);
      };
      let webStreamsAdapters;
      function lazyWebStreams() {
        if (webStreamsAdapters === undefined)
          webStreamsAdapters = require("internal/webstreams_adapters");
        return webStreamsAdapters;
      }
      Readable.fromWeb = function(readableStream, options) {
        return lazyWebStreams().newStreamReadableFromReadableStream(readableStream, options);
      };
      Readable.toWeb = function(streamReadable, options) {
        return lazyWebStreams().newReadableStreamFromStreamReadable(streamReadable, options);
      };
      Readable.wrap = function(src, options) {
        return new Readable({
          objectMode: src.readableObjectMode ?? src.objectMode ?? true,
          ...options,
          destroy(err, callback) {
            destroyImpl.destroyer(src, err);
            callback(err);
          }
        }).wrap(src);
      };
      module.exports = Readable;

    },
    "internal/streams/state": function (module, _unusedExports, require) {
      "use strict";
      const { validateInteger } = require("internal/validators");
      const NumberIsInteger = Number.isInteger;
      const MathFloor = Math.floor;
      let defaultHighWaterMarkBytes = process.platform === "win32" ? 16 * 1024 : 64 * 1024;
      let defaultHighWaterMarkObjectMode = 16;
      function highWaterMarkFrom(options, isDuplex, duplexKey) {
        return options.highWaterMark != null ? options.highWaterMark : isDuplex ? options[duplexKey] : null;
      }
      function getDefaultHighWaterMark(objectMode = false) {
        return objectMode ? defaultHighWaterMarkObjectMode : defaultHighWaterMarkBytes;
      }
      function setDefaultHighWaterMark(objectMode, value) {
        validateInteger(value, "value", 0);
        if (objectMode) {
          defaultHighWaterMarkObjectMode = value;
        } else {
          defaultHighWaterMarkBytes = value;
        }
      }
      function getHighWaterMark(state, options, duplexKey, isDuplex) {
        const hwm = highWaterMarkFrom(options, isDuplex, duplexKey);
        if (hwm != null) {
          if (!NumberIsInteger(hwm) || hwm < 0) {
            const name = isDuplex ? `options.${duplexKey}` : "options.highWaterMark";
            throw codes.INVALID_ARG_VALUE(name, hwm);
          }
          return MathFloor(hwm);
        }
        return getDefaultHighWaterMark(state.objectMode);
      }
      module.exports = {
        getHighWaterMark,
        getDefaultHighWaterMark,
        setDefaultHighWaterMark
      };

    },
    "internal/streams/transform": function (module, _unusedExports, require) {
      "use strict";
      const Duplex = require("internal/streams/duplex");
      const { getHighWaterMark } = require("internal/streams/state");
      const kCallback = Symbol("kCallback");
      function Transform(options) {
        if (!(this instanceof Transform))
          return new Transform(options);
        const readableHighWaterMark = options ? getHighWaterMark(this, options, "readableHighWaterMark", true) : null;
        if (readableHighWaterMark === 0) {
          options = {
            ...options,
            highWaterMark: null,
            readableHighWaterMark,
            writableHighWaterMark: options.writableHighWaterMark || 0
          };
        }
        Duplex.call(this, options);
        this._readableState.sync = false;
        this[kCallback] = null;
        if (options) {
          const { transform, flush } = options;
          if (typeof transform === "function")
            this._transform = transform;
          if (typeof flush === "function")
            this._flush = flush;
        }
        this.on("prefinish", prefinish);
      }
      toClass(Transform, "Transform", Duplex);
      function final(cb) {
        if (typeof this._flush === "function" && !this.destroyed) {
          this._flush((er, data) => {
            if (er) {
              if (cb) {
                cb(er);
              } else {
                this.destroy(er);
              }
              return;
            }
            if (data != null) {
              this.push(data);
            }
            this.push(null);
            if (cb) {
              cb();
            }
          });
        } else {
          this.push(null);
          if (cb) {
            cb();
          }
        }
      }
      function prefinish() {
        if (this._final !== final) {
          final.call(this);
        }
      }
      Transform.prototype._final = final;
      Transform.prototype._transform = function(_chunk, _encoding, _callback) {
        throw codes.METHOD_NOT_IMPLEMENTED("_transform()");
      };
      Transform.prototype._write = function(chunk, encoding, callback) {
        const rState = this._readableState;
        const wState = this._writableState;
        const length = rState.length;
        this._transform(chunk, encoding, (err, val) => {
          if (err) {
            callback(err);
            return;
          }
          if (val != null) {
            this.push(val);
          }
          if (rState.ended) {
            process.nextTick(callback);
          } else if (wState.ended || length === rState.length || rState.length < rState.highWaterMark) {
            callback();
          } else {
            this[kCallback] = callback;
          }
        });
      };
      Transform.prototype._read = function() {
        if (this[kCallback]) {
          const callback = this[kCallback];
          this[kCallback] = null;
          callback();
        }
      };
      module.exports = Transform;

    },
    "internal/streams/utils": function (module, _unusedExports, require) {
      "use strict";
      const SymbolFor = Symbol.for;
      const SymbolIterator = Symbol.iterator;
      const SymbolAsyncIterator = Symbol.asyncIterator;
      const kIsDestroyed = SymbolFor("nodejs.stream.destroyed");
      const kIsErrored = SymbolFor("nodejs.stream.errored");
      const kIsReadable = SymbolFor("nodejs.stream.readable");
      const kIsWritable = SymbolFor("nodejs.stream.writable");
      const kIsDisturbed = SymbolFor("nodejs.stream.disturbed");
      const kOnConstructed = Symbol("kOnConstructed");
      const kIsClosedPromise = SymbolFor("nodejs.webstream.isClosedPromise");
      const kControllerErrorFunction = SymbolFor("nodejs.webstream.controllerErrorFunction");
      const kState = Symbol("kState");
      const kObjectMode = 1 << 0;
      const kErrorEmitted = 1 << 1;
      const kAutoDestroy = 1 << 2;
      const kEmitClose = 1 << 3;
      const kDestroyed = 1 << 4;
      const kClosed = 1 << 5;
      const kCloseEmitted = 1 << 6;
      const kErrored = 1 << 7;
      const kConstructed = 1 << 8;
      function isReadableNodeStream(obj, strict = false) {
        return !!(obj && typeof obj.pipe === "function" && typeof obj.on === "function" && (!strict || typeof obj.pause === "function" && typeof obj.resume === "function") && (!obj._writableState || obj._readableState?.readable !== false) && (!obj._writableState || obj._readableState));
      }
      function isWritableNodeStream(obj) {
        return !!(obj && typeof obj.write === "function" && typeof obj.on === "function" && (!obj._readableState || obj._writableState?.writable !== false));
      }
      function isDuplexNodeStream(obj) {
        return !!(obj && typeof obj.pipe === "function" && obj._readableState && typeof obj.on === "function" && typeof obj.write === "function");
      }
      function isNodeStream(obj) {
        return obj && (obj._readableState || obj._writableState || typeof obj.write === "function" && typeof obj.on === "function" || typeof obj.pipe === "function" && typeof obj.on === "function");
      }
      function isReadableStream(obj) {
        return inheritsReadableStream(obj);
      }
      function isWritableStream(obj) {
        return inheritsWritableStream(obj);
      }
      function isTransformStream(obj) {
        return inheritsTransformStream(obj);
      }
      function isWebStream(obj) {
        return isReadableStream(obj) || isWritableStream(obj) || isTransformStream(obj);
      }
      function isIterable(obj, isAsync) {
        if (obj == null)
          return false;
        if (isAsync === true)
          return typeof obj[SymbolAsyncIterator] === "function";
        if (isAsync === false)
          return typeof obj[SymbolIterator] === "function";
        return typeof obj[SymbolAsyncIterator] === "function" || typeof obj[SymbolIterator] === "function";
      }
      function isDestroyed(stream) {
        if (!isNodeStream(stream))
          return null;
        const wState = stream._writableState;
        const rState = stream._readableState;
        const state = wState || rState;
        return !!(stream.destroyed || stream[kIsDestroyed] || state?.destroyed);
      }
      function isWritableEnded(stream) {
        if (!isWritableNodeStream(stream))
          return null;
        if (stream.writableEnded === true)
          return true;
        const wState = stream._writableState;
        if (wState?.errored)
          return false;
        if (typeof wState?.ended !== "boolean")
          return null;
        return wState.ended;
      }
      function isWritableFinished(stream, strict) {
        if (!isWritableNodeStream(stream))
          return null;
        if (stream.writableFinished === true)
          return true;
        const wState = stream._writableState;
        if (wState?.errored)
          return false;
        if (typeof wState?.finished !== "boolean")
          return null;
        return !!(wState.finished || strict === false && wState.ended === true && wState.length === 0);
      }
      function isReadableFinished(stream, strict) {
        if (!isReadableNodeStream(stream))
          return null;
        const rState = stream._readableState;
        if (rState?.errored)
          return false;
        if (typeof rState?.endEmitted !== "boolean")
          return null;
        return !!(rState.endEmitted || strict === false && rState.ended === true && rState.length === 0);
      }
      function isReadable(stream) {
        if (stream && stream[kIsReadable] != null)
          return stream[kIsReadable];
        if (typeof stream?.readable !== "boolean")
          return null;
        if (isDestroyed(stream))
          return false;
        return isReadableNodeStream(stream) && stream.readable && !isReadableFinished(stream);
      }
      function isWritable(stream) {
        if (stream && stream[kIsWritable] != null)
          return stream[kIsWritable];
        if (typeof stream?.writable !== "boolean")
          return null;
        if (isDestroyed(stream))
          return false;
        return isWritableNodeStream(stream) && stream.writable && !isWritableEnded(stream);
      }
      function isFinished(stream, opts) {
        if (!isNodeStream(stream)) {
          return null;
        }
        if (isDestroyed(stream)) {
          return true;
        }
        if (opts?.readable !== false && isReadable(stream)) {
          return false;
        }
        if (opts?.writable !== false && isWritable(stream)) {
          return false;
        }
        return true;
      }
      function isWritableErrored(stream) {
        if (!isNodeStream(stream)) {
          return null;
        }
        const writableErrored = stream.writableErrored;
        if (writableErrored) {
          return writableErrored;
        }
        return stream._writableState?.errored ?? null;
      }
      function isReadableErrored(stream) {
        if (!isNodeStream(stream)) {
          return null;
        }
        const readableErrored = stream.readableErrored;
        if (readableErrored) {
          return readableErrored;
        }
        return stream._readableState?.errored ?? null;
      }
      function isClosed(stream) {
        if (!isNodeStream(stream)) {
          return null;
        }
        const closed = stream.closed;
        if (typeof closed === "boolean") {
          return closed;
        }
        const wState = stream._writableState;
        const rState = stream._readableState;
        if (typeof wState?.closed === "boolean" || typeof rState?.closed === "boolean") {
          return wState?.closed || rState?.closed;
        }
        const _closed = stream._closed;
        if (typeof _closed === "boolean" && isOutgoingMessage(stream)) {
          return _closed;
        }
        return null;
      }
      function isOutgoingMessage(stream) {
        return typeof stream._closed === "boolean" && typeof stream._defaultKeepAlive === "boolean" && typeof stream._removedConnection === "boolean" && typeof stream._removedContLen === "boolean";
      }
      function isServerResponse(stream) {
        return typeof stream._sent100 === "boolean" && isOutgoingMessage(stream);
      }
      function isServerRequest(stream) {
        return typeof stream._consuming === "boolean" && typeof stream._dumped === "boolean" && stream.req?.upgradeOrConnect === undefined;
      }
      function willEmitClose(stream) {
        if (!isNodeStream(stream))
          return null;
        const wState = stream._writableState;
        const rState = stream._readableState;
        const state = wState || rState;
        return !state && isServerResponse(stream) || !!(state?.autoDestroy && state.emitClose && state.closed === false);
      }
      function isDisturbed(stream) {
        return !!(stream && (stream[kIsDisturbed] ?? (stream.readableDidRead || stream.readableAborted)));
      }
      function isErrored(stream) {
        return !!(stream && (stream[kIsErrored] ?? stream.readableErrored ?? stream.writableErrored ?? stream._readableState?.errorEmitted ?? stream._writableState?.errorEmitted ?? stream._readableState?.errored ?? stream._writableState?.errored));
      }
      module.exports = {
        kOnConstructed,
        isDestroyed,
        kIsDestroyed,
        isDisturbed,
        isErrored,
        isReadable,
        kIsClosedPromise,
        kControllerErrorFunction,
        isClosed,
        isDuplexNodeStream,
        isFinished,
        isIterable,
        isReadableNodeStream,
        isReadableStream,
        isReadableFinished,
        isReadableErrored,
        isNodeStream,
        isWebStream,
        isWritable,
        isWritableNodeStream,
        isWritableStream,
        isWritableEnded,
        isWritableFinished,
        isWritableErrored,
        isServerRequest,
        willEmitClose,
        isTransformStream,
        kState,
        kObjectMode,
        kErrorEmitted,
        kAutoDestroy,
        kEmitClose,
        kDestroyed,
        kClosed,
        kCloseEmitted,
        kErrored,
        kConstructed
      };

    },
    "internal/streams/writable": function (module, _unusedExports, require) {
      "use strict";
      const EE = require("node:events");
      const { Stream } = require("internal/streams/legacy");
      const destroyImpl = require("internal/streams/destroy");
      const eos = require("internal/streams/end-of-stream");
      const { addAbortSignal } = require("internal/streams/add-abort-signal");
      const { getHighWaterMark, getDefaultHighWaterMark } = require("internal/streams/state");
      const {
        kOnConstructed,
        kState,
        kObjectMode,
        kErrorEmitted,
        kAutoDestroy,
        kEmitClose,
        kDestroyed,
        kClosed,
        kCloseEmitted,
        kErrored,
        kConstructed
      } = require("internal/streams/utils");
      const ObjectDefineProperties = Object.defineProperties;
      const ArrayPrototypeSlice = Array.prototype.slice;
      const ObjectDefineProperty = Object.defineProperty;
      const SymbolHasInstance = Symbol.hasInstance;
      const FunctionPrototypeSymbolHasInstance = Function.prototype[Symbol.hasInstance];
      const StringPrototypeToLowerCase = String.prototype.toLowerCase;
      const SymbolAsyncDispose = Symbol.asyncDispose;
      const { errorOrDestroy } = destroyImpl;
      function nop() {}
      const kOnFinishedValue = Symbol("kOnFinishedValue");
      const kErroredValue = Symbol("kErroredValue");
      const kDefaultEncodingValue = Symbol("kDefaultEncodingValue");
      const kWriteCbValue = Symbol("kWriteCbValue");
      const kAfterWriteTickInfoValue = Symbol("kAfterWriteTickInfoValue");
      const kBufferedValue = Symbol("kBufferedValue");
      const kSync = 1 << 9;
      const kFinalCalled = 1 << 10;
      const kNeedDrain = 1 << 11;
      const kEnding = 1 << 12;
      const kFinished = 1 << 13;
      const kDecodeStrings = 1 << 14;
      const kWriting = 1 << 15;
      const kBufferProcessing = 1 << 16;
      const kPrefinished = 1 << 17;
      const kAllBuffers = 1 << 18;
      const kAllNoop = 1 << 19;
      const kOnFinished = 1 << 20;
      const kHasWritable = 1 << 21;
      const kWritable = 1 << 22;
      const kCorked = 1 << 23;
      const kDefaultUTF8Encoding = 1 << 24;
      const kWriteCb = 1 << 25;
      const kExpectWriteCb = 1 << 26;
      const kAfterWriteTickInfo = 1 << 27;
      const kAfterWritePending = 1 << 28;
      const kBuffered = 1 << 29;
      const kEnded = 1 << 30;
      function makeBitMapDescriptor(bit) {
        return {
          enumerable: false,
          get() {
            return (this[kState] & bit) !== 0;
          },
          set(value) {
            if (value)
              this[kState] |= bit;
            else
              this[kState] &= ~bit;
          }
        };
      }
      WritableState.prototype = {};
      ObjectDefineProperties(WritableState.prototype, {
        objectMode: makeBitMapDescriptor(kObjectMode),
        finalCalled: makeBitMapDescriptor(kFinalCalled),
        needDrain: makeBitMapDescriptor(kNeedDrain),
        ending: makeBitMapDescriptor(kEnding),
        ended: makeBitMapDescriptor(kEnded),
        finished: makeBitMapDescriptor(kFinished),
        destroyed: makeBitMapDescriptor(kDestroyed),
        decodeStrings: makeBitMapDescriptor(kDecodeStrings),
        writing: makeBitMapDescriptor(kWriting),
        sync: makeBitMapDescriptor(kSync),
        bufferProcessing: makeBitMapDescriptor(kBufferProcessing),
        constructed: makeBitMapDescriptor(kConstructed),
        prefinished: makeBitMapDescriptor(kPrefinished),
        errorEmitted: makeBitMapDescriptor(kErrorEmitted),
        emitClose: makeBitMapDescriptor(kEmitClose),
        autoDestroy: makeBitMapDescriptor(kAutoDestroy),
        closed: makeBitMapDescriptor(kClosed),
        closeEmitted: makeBitMapDescriptor(kCloseEmitted),
        allBuffers: makeBitMapDescriptor(kAllBuffers),
        allNoop: makeBitMapDescriptor(kAllNoop),
        errored: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kErrored) !== 0 ? this[kErroredValue] : null;
          },
          set(value) {
            if (value) {
              this[kErroredValue] = value;
              this[kState] |= kErrored;
            } else {
              this[kState] &= ~kErrored;
            }
          }
        },
        writable: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kHasWritable) !== 0 ? (this[kState] & kWritable) !== 0 : undefined;
          },
          set(value) {
            if (value == null) {
              this[kState] &= ~(kHasWritable | kWritable);
            } else if (value) {
              this[kState] |= kHasWritable | kWritable;
            } else {
              this[kState] |= kHasWritable;
              this[kState] &= ~kWritable;
            }
          }
        },
        defaultEncoding: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kDefaultUTF8Encoding) !== 0 ? "utf8" : this[kDefaultEncodingValue];
          },
          set(value) {
            if (value === "utf8" || value === "utf-8") {
              this[kState] |= kDefaultUTF8Encoding;
            } else {
              this[kState] &= ~kDefaultUTF8Encoding;
              this[kDefaultEncodingValue] = value;
            }
          }
        },
        writecb: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kWriteCb) !== 0 ? this[kWriteCbValue] : nop;
          },
          set(value) {
            this[kWriteCbValue] = value;
            if (value) {
              this[kState] |= kWriteCb;
            } else {
              this[kState] &= ~kWriteCb;
            }
          }
        },
        afterWriteTickInfo: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kAfterWriteTickInfo) !== 0 ? this[kAfterWriteTickInfoValue] : null;
          },
          set(value) {
            this[kAfterWriteTickInfoValue] = value;
            if (value) {
              this[kState] |= kAfterWriteTickInfo;
            } else {
              this[kState] &= ~kAfterWriteTickInfo;
            }
          }
        },
        buffered: {
          __proto__: null,
          enumerable: false,
          get() {
            return (this[kState] & kBuffered) !== 0 ? this[kBufferedValue] : [];
          },
          set(value) {
            this[kBufferedValue] = value;
            if (value) {
              this[kState] |= kBuffered;
            } else {
              this[kState] &= ~kBuffered;
            }
          }
        }
      });
      function WritableState(options, stream, isDuplex) {
        this[kState] = kSync | kConstructed | kEmitClose | kAutoDestroy;
        if (options?.objectMode)
          this[kState] |= kObjectMode;
        if (isDuplex && options?.writableObjectMode)
          this[kState] |= kObjectMode;
        this.highWaterMark = options ? getHighWaterMark(this, options, "writableHighWaterMark", isDuplex) : getDefaultHighWaterMark(false);
        if (!options || options.decodeStrings !== false)
          this[kState] |= kDecodeStrings;
        if (options && options.emitClose === false)
          this[kState] &= ~kEmitClose;
        if (options && options.autoDestroy === false)
          this[kState] &= ~kAutoDestroy;
        const defaultEncoding = options ? options.defaultEncoding : null;
        if (defaultEncoding == null || defaultEncoding === "utf8" || defaultEncoding === "utf-8") {
          this[kState] |= kDefaultUTF8Encoding;
        } else if (Buffer.isEncoding(defaultEncoding)) {
          this[kState] &= ~kDefaultUTF8Encoding;
          this[kDefaultEncodingValue] = defaultEncoding;
        } else {
          throw codes.UNKNOWN_ENCODING(defaultEncoding);
        }
        this.length = 0;
        this.corked = 0;
        this.onwrite = onwrite.bind(undefined, stream);
        this.writelen = 0;
        resetBuffer(this);
        this.pendingcb = 0;
      }
      function resetBuffer(state) {
        state[kBufferedValue] = null;
        state.bufferedIndex = 0;
        state[kState] |= kAllBuffers | kAllNoop;
        state[kState] &= ~kBuffered;
      }
      WritableState.prototype.getBuffer = function getBuffer() {
        return (this[kState] & kBuffered) === 0 ? [] : ArrayPrototypeSlice.call(this.buffered, this.bufferedIndex);
      };
      ObjectDefineProperty(WritableState.prototype, "bufferedRequestCount", {
        __proto__: null,
        get() {
          return (this[kState] & kBuffered) === 0 ? 0 : this[kBufferedValue].length - this.bufferedIndex;
        }
      });
      WritableState.prototype[kOnConstructed] = function onConstructed(stream) {
        if ((this[kState] & kWriting) === 0) {
          clearBuffer(stream, this);
        }
        if ((this[kState] & kEnding) !== 0) {
          finishMaybe(stream, this);
        }
      };
      function Writable(options) {
        if (!(this instanceof Writable))
          return new Writable(options);
        this._events ??= {
          close: undefined,
          error: undefined,
          prefinish: undefined,
          finish: undefined,
          drain: undefined
        };
        this._writableState = new WritableState(options, this, false);
        if (options) {
          const { write, writev, destroy, final, construct, signal } = options;
          if (typeof write === "function")
            this._write = write;
          if (typeof writev === "function")
            this._writev = writev;
          if (typeof destroy === "function")
            this._destroy = destroy;
          if (typeof final === "function")
            this._final = final;
          if (typeof construct === "function")
            this._construct = construct;
          if (signal)
            addAbortSignal(signal, this);
        }
        Stream.call(this, options);
        if (this._construct != null) {
          destroyImpl.construct(this, () => {
            this._writableState[kOnConstructed](this);
          });
        }
      }
      toClass(Writable, "Writable", Stream);
      Writable.WritableState = WritableState;
      ObjectDefineProperty(Writable, SymbolHasInstance, {
        __proto__: null,
        value: function(object) {
          if (FunctionPrototypeSymbolHasInstance.call(this, object))
            return true;
          if (this !== Writable)
            return false;
          return object && object._writableState instanceof WritableState;
        }
      });
      Writable.prototype.pipe = function() {
        errorOrDestroy(this, codes.STREAM_CANNOT_PIPE());
      };
      function _write(stream, chunk, encoding, cb) {
        const state = stream._writableState;
        if (cb == null || typeof cb !== "function") {
          cb = nop;
        }
        if (chunk === null) {
          throw codes.STREAM_NULL_VALUES();
        }
        if ((state[kState] & kObjectMode) === 0) {
          if (!encoding) {
            encoding = (state[kState] & kDefaultUTF8Encoding) !== 0 ? "utf8" : state.defaultEncoding;
          } else if (encoding !== "buffer" && !Buffer.isEncoding(encoding)) {
            throw codes.UNKNOWN_ENCODING(encoding);
          }
          if (typeof chunk === "string") {
            if (encoding === "buffer") {
              throw codes.UNKNOWN_ENCODING(encoding);
            }
            if ((state[kState] & kDecodeStrings) !== 0) {
              chunk = Buffer.from(chunk, encoding);
              encoding = "buffer";
            }
          } else if (chunk instanceof Buffer) {
            encoding = "buffer";
          } else if (Stream._isArrayBufferView(chunk)) {
            chunk = Stream._uint8ArrayToBuffer(chunk);
            encoding = "buffer";
          } else {
            throw codes.INVALID_ARG_TYPE("chunk", ["string", "Buffer", "TypedArray", "DataView"], chunk);
          }
        }
        let err;
        if ((state[kState] & kEnding) !== 0) {
          err = codes.STREAM_WRITE_AFTER_END();
        } else if ((state[kState] & kDestroyed) !== 0) {
          err = codes.STREAM_DESTROYED("write");
        }
        if (err) {
          process.nextTick(cb, err);
          errorOrDestroy(stream, err, true);
          return err;
        }
        state.pendingcb++;
        return writeOrBuffer(stream, state, chunk, encoding, cb);
      }
      Writable.prototype.write = function(chunk, encoding, cb) {
        if (encoding != null && typeof encoding === "function") {
          cb = encoding;
          encoding = null;
        }
        return _write(this, chunk, encoding, cb) === true;
      };
      Writable.prototype.cork = function() {
        const state = this._writableState;
        state[kState] |= kCorked;
        state.corked++;
      };
      Writable.prototype.uncork = function() {
        const state = this._writableState;
        if (state.corked) {
          state.corked--;
          if (!state.corked) {
            state[kState] &= ~kCorked;
          }
          if ((state[kState] & kWriting) === 0)
            clearBuffer(this, state);
        }
      };
      Writable.prototype.setDefaultEncoding = function setDefaultEncoding(encoding) {
        if (typeof encoding === "string")
          encoding = StringPrototypeToLowerCase.call(encoding);
        if (!Buffer.isEncoding(encoding))
          throw codes.UNKNOWN_ENCODING(encoding);
        this._writableState.defaultEncoding = encoding;
        return this;
      };
      function writeOrBuffer(stream, state, chunk, encoding, callback) {
        const len = (state[kState] & kObjectMode) !== 0 ? 1 : chunk.length;
        state.length += len;
        if ((state[kState] & (kWriting | kErrored | kCorked | kConstructed)) !== kConstructed) {
          if ((state[kState] & kBuffered) === 0) {
            state[kState] |= kBuffered;
            state[kBufferedValue] = [];
          }
          state[kBufferedValue].push({ chunk, encoding, callback });
          if ((state[kState] & kAllBuffers) !== 0 && encoding !== "buffer") {
            state[kState] &= ~kAllBuffers;
          }
          if ((state[kState] & kAllNoop) !== 0 && callback !== nop) {
            state[kState] &= ~kAllNoop;
          }
        } else {
          state.writelen = len;
          if (callback !== nop) {
            state.writecb = callback;
          }
          state[kState] |= kWriting | kSync | kExpectWriteCb;
          stream._write(chunk, encoding, state.onwrite);
          state[kState] &= ~kSync;
        }
        const ret = state.length < state.highWaterMark || state.length === 0;
        if (!ret) {
          state[kState] |= kNeedDrain;
        }
        return ret && (state[kState] & (kDestroyed | kErrored)) === 0;
      }
      function doWrite(stream, state, writev, len, chunk, encoding, cb) {
        state.writelen = len;
        if (cb !== nop) {
          state.writecb = cb;
        }
        state[kState] |= kWriting | kSync | kExpectWriteCb;
        if ((state[kState] & kDestroyed) !== 0)
          state.onwrite(codes.STREAM_DESTROYED("write"));
        else if (writev)
          stream._writev(chunk, state.onwrite);
        else
          stream._write(chunk, encoding, state.onwrite);
        state[kState] &= ~kSync;
      }
      function onwriteError(stream, state, er, cb) {
        --state.pendingcb;
        cb(er);
        errorBuffer(state);
        errorOrDestroy(stream, er);
      }
      function onwrite(stream, er) {
        const state = stream._writableState;
        if ((state[kState] & kExpectWriteCb) === 0) {
          errorOrDestroy(stream, codes.MULTIPLE_CALLBACK());
          return;
        }
        const sync = (state[kState] & kSync) !== 0;
        const cb = (state[kState] & kWriteCb) !== 0 ? state[kWriteCbValue] : nop;
        state.writecb = null;
        state[kState] &= ~(kWriting | kExpectWriteCb);
        state.length -= state.writelen;
        state.writelen = 0;
        if (er) {
          er.stack;
          if ((state[kState] & kErrored) === 0) {
            state[kErroredValue] = er;
            state[kState] |= kErrored;
          }
          const readableState = stream._readableState;
          if (readableState && !readableState.errored) {
            readableState.errored = er;
          }
          if (sync) {
            process.nextTick(onwriteError, stream, state, er, cb);
          } else {
            onwriteError(stream, state, er, cb);
          }
        } else {
          if ((state[kState] & kBuffered) !== 0) {
            clearBuffer(stream, state);
          }
          if (sync) {
            const needDrain = (state[kState] & kNeedDrain) !== 0 && state.length === 0;
            const needTick = needDrain || state[kState] & Number(kDestroyed !== 0) || cb !== nop;
            if (cb === nop) {
              if ((state[kState] & kAfterWritePending) === 0 && needTick) {
                process.nextTick(afterWrite, stream, state, 1, cb);
                state[kState] |= kAfterWritePending;
              } else {
                state.pendingcb--;
                if ((state[kState] & kEnding) !== 0) {
                  finishMaybe(stream, state, true);
                }
              }
            } else if ((state[kState] & kAfterWriteTickInfo) !== 0 && state[kAfterWriteTickInfoValue].cb === cb) {
              state[kAfterWriteTickInfoValue].count++;
            } else if (needTick) {
              state[kAfterWriteTickInfoValue] = { count: 1, cb, stream, state };
              process.nextTick(afterWriteTick, state[kAfterWriteTickInfoValue]);
              state[kState] |= kAfterWritePending | kAfterWriteTickInfo;
            } else {
              state.pendingcb--;
              if ((state[kState] & kEnding) !== 0) {
                finishMaybe(stream, state, true);
              }
            }
          } else {
            afterWrite(stream, state, 1, cb);
          }
        }
      }
      function afterWriteTick({ stream, state, count, cb }) {
        state[kState] &= ~kAfterWriteTickInfo;
        state[kAfterWriteTickInfoValue] = null;
        return afterWrite(stream, state, count, cb);
      }
      function afterWrite(stream, state, count, cb) {
        state[kState] &= ~kAfterWritePending;
        const needDrain = (state[kState] & (kEnding | kNeedDrain | kDestroyed)) === kNeedDrain && state.length === 0;
        if (needDrain) {
          state[kState] &= ~kNeedDrain;
          stream.emit("drain");
        }
        while (count-- > 0) {
          state.pendingcb--;
          cb(null);
        }
        if ((state[kState] & kDestroyed) !== 0) {
          errorBuffer(state);
        }
        if ((state[kState] & kEnding) !== 0) {
          finishMaybe(stream, state, true);
        }
      }
      function errorBuffer(state) {
        if ((state[kState] & kWriting) !== 0) {
          return;
        }
        if ((state[kState] & kBuffered) !== 0) {
          for (let n = state.bufferedIndex;n < state.buffered.length; ++n) {
            const { chunk, callback } = state[kBufferedValue][n];
            const len = (state[kState] & kObjectMode) !== 0 ? 1 : chunk.length;
            state.length -= len;
            callback(state.errored ?? codes.STREAM_DESTROYED("write"));
          }
        }
        callFinishedCallbacks(state, state.errored ?? codes.STREAM_DESTROYED("end"));
        resetBuffer(state);
      }
      function clearBuffer(stream, state) {
        if ((state[kState] & (kDestroyed | kBufferProcessing | kCorked | kBuffered | kConstructed)) !== (kBuffered | kConstructed)) {
          return;
        }
        const objectMode = (state[kState] & kObjectMode) !== 0;
        const { [kBufferedValue]: buffered, bufferedIndex } = state;
        const bufferedLength = buffered.length - bufferedIndex;
        if (!bufferedLength) {
          return;
        }
        let i = bufferedIndex;
        state[kState] |= kBufferProcessing;
        if (bufferedLength > 1 && stream._writev) {
          state.pendingcb -= bufferedLength - 1;
          const callback = (state[kState] & kAllNoop) !== 0 ? nop : (err) => {
            for (let n = i;n < buffered.length; ++n) {
              buffered[n].callback(err);
            }
          };
          const chunks = (state[kState] & kAllNoop) !== 0 && i === 0 ? buffered : ArrayPrototypeSlice.call(buffered, i);
          chunks.allBuffers = (state[kState] & kAllBuffers) !== 0;
          doWrite(stream, state, true, state.length, chunks, "", callback);
          resetBuffer(state);
        } else {
          do {
            const { chunk, encoding, callback } = buffered[i];
            buffered[i++] = null;
            const len = objectMode ? 1 : chunk.length;
            doWrite(stream, state, false, len, chunk, encoding, callback);
          } while (i < buffered.length && (state[kState] & kWriting) === 0);
          if (i === buffered.length) {
            resetBuffer(state);
          } else if (i > 256) {
            buffered.splice(0, i);
            state.bufferedIndex = 0;
          } else {
            state.bufferedIndex = i;
          }
        }
        state[kState] &= ~kBufferProcessing;
      }
      Writable.prototype._write = function(chunk, encoding, cb) {
        if (this._writev) {
          this._writev([{ chunk, encoding }], cb);
        } else {
          throw codes.METHOD_NOT_IMPLEMENTED("_write()");
        }
      };
      Writable.prototype._writev = null;
      Writable.prototype.end = function(chunk, encoding, cb) {
        const state = this._writableState;
        if (typeof chunk === "function") {
          cb = chunk;
          chunk = null;
          encoding = null;
        } else if (typeof encoding === "function") {
          cb = encoding;
          encoding = null;
        }
        let err;
        if (chunk != null) {
          const ret = _write(this, chunk, encoding);
          if (Error.isError(ret)) {
            err = ret;
          }
        }
        if ((state[kState] & kCorked) !== 0) {
          state.corked = 1;
          this.uncork();
        }
        if (err) {} else if ((state[kState] & (kEnding | kErrored)) === 0) {
          state[kState] |= kEnding;
          finishMaybe(this, state, true);
          state[kState] |= kEnded;
        } else if ((state[kState] & kFinished) !== 0) {
          err = codes.STREAM_ALREADY_FINISHED("end");
        } else if ((state[kState] & kDestroyed) !== 0) {
          err = codes.STREAM_DESTROYED("end");
        }
        if (typeof cb === "function") {
          if (err) {
            process.nextTick(cb, err);
          } else if ((state[kState] & kErrored) !== 0) {
            process.nextTick(cb, state[kErroredValue]);
          } else if ((state[kState] & kFinished) !== 0) {
            process.nextTick(cb, null);
          } else {
            state[kState] |= kOnFinished;
            state[kOnFinishedValue] ??= [];
            state[kOnFinishedValue].push(cb);
          }
        }
        return this;
      };
      function needFinish(state) {
        return (state[kState] & (kEnding | kDestroyed | kConstructed | kFinished | kWriting | kErrorEmitted | kCloseEmitted | kErrored | kBuffered)) === (kEnding | kConstructed) && state.length === 0;
      }
      function onFinish(stream, state, err) {
        if ((state[kState] & kPrefinished) !== 0) {
          errorOrDestroy(stream, err ?? codes.MULTIPLE_CALLBACK());
          return;
        }
        state.pendingcb--;
        if (err) {
          callFinishedCallbacks(state, err);
          errorOrDestroy(stream, err, (state[kState] & kSync) !== 0);
        } else if (needFinish(state)) {
          state[kState] |= kPrefinished;
          stream.emit("prefinish");
          state.pendingcb++;
          process.nextTick(finish, stream, state);
        }
      }
      function prefinish(stream, state) {
        if ((state[kState] & (kPrefinished | kFinalCalled)) !== 0) {
          return;
        }
        if (typeof stream._final === "function" && (state[kState] & kDestroyed) === 0) {
          state[kState] |= kFinalCalled | kSync;
          state.pendingcb++;
          try {
            stream._final((err) => onFinish(stream, state, err));
          } catch (err) {
            onFinish(stream, state, err);
          }
          state[kState] &= ~kSync;
        } else {
          state[kState] |= kFinalCalled | kPrefinished;
          stream.emit("prefinish");
        }
      }
      function finishMaybe(stream, state, sync) {
        if (needFinish(state)) {
          prefinish(stream, state);
          if (state.pendingcb === 0) {
            if (sync) {
              state.pendingcb++;
              process.nextTick((stream, state) => {
                if (needFinish(state)) {
                  finish(stream, state);
                } else {
                  state.pendingcb--;
                }
              }, stream, state);
            } else if (needFinish(state)) {
              state.pendingcb++;
              finish(stream, state);
            }
          }
        }
      }
      function finish(stream, state) {
        state.pendingcb--;
        state[kState] |= kFinished;
        callFinishedCallbacks(state, null);
        stream.emit("finish");
        if ((state[kState] & kAutoDestroy) !== 0) {
          const rState = stream._readableState;
          const autoDestroy = !rState || rState.autoDestroy && (rState.endEmitted || rState.readable === false);
          if (autoDestroy) {
            stream.destroy();
          }
        }
      }
      function callFinishedCallbacks(state, err) {
        if ((state[kState] & kOnFinished) === 0) {
          return;
        }
        const onfinishCallbacks = state[kOnFinishedValue];
        state[kOnFinishedValue] = null;
        state[kState] &= ~kOnFinished;
        for (let i = 0;i < onfinishCallbacks.length; i++) {
          onfinishCallbacks[i](err);
        }
      }
      ObjectDefineProperties(Writable.prototype, {
        closed: {
          __proto__: null,
          get() {
            return this._writableState ? (this._writableState[kState] & kClosed) !== 0 : false;
          }
        },
        destroyed: {
          __proto__: null,
          get() {
            return this._writableState ? (this._writableState[kState] & kDestroyed) !== 0 : false;
          },
          set(value) {
            if (!this._writableState)
              return;
            if (value)
              this._writableState[kState] |= kDestroyed;
            else
              this._writableState[kState] &= ~kDestroyed;
          }
        },
        writable: {
          __proto__: null,
          get() {
            const w = this._writableState;
            return !!w && w.writable !== false && (w[kState] & (kEnding | kEnded | kDestroyed | kErrored)) === 0;
          },
          set(val) {
            const state = this._writableState;
            if (state) {
              state.writable = !!val;
            }
          }
        },
        writableFinished: {
          __proto__: null,
          get() {
            const state = this._writableState;
            return state ? (state[kState] & kFinished) !== 0 : false;
          }
        },
        writableObjectMode: {
          __proto__: null,
          get() {
            const state = this._writableState;
            return state ? (state[kState] & kObjectMode) !== 0 : false;
          }
        },
        writableBuffer: {
          __proto__: null,
          get() {
            const state = this._writableState;
            return state && state.getBuffer();
          }
        },
        writableEnded: {
          __proto__: null,
          get() {
            const state = this._writableState;
            return state ? (state[kState] & kEnding) !== 0 : false;
          }
        },
        writableNeedDrain: {
          __proto__: null,
          get() {
            const state = this._writableState;
            return state ? (state[kState] & (kDestroyed | kEnding | kNeedDrain)) === kNeedDrain : false;
          }
        },
        writableHighWaterMark: {
          __proto__: null,
          get() {
            const state = this._writableState;
            return state?.highWaterMark;
          }
        },
        writableCorked: {
          __proto__: null,
          get() {
            const state = this._writableState;
            return state ? state.corked : 0;
          }
        },
        writableLength: {
          __proto__: null,
          get() {
            const state = this._writableState;
            return state?.length;
          }
        },
        errored: {
          __proto__: null,
          enumerable: false,
          get() {
            const state = this._writableState;
            return state ? state.errored : null;
          }
        },
        writableAborted: {
          __proto__: null,
          get: function() {
            const state = this._writableState;
            return (state[kState] & (kHasWritable | kWritable)) !== kHasWritable && (state[kState] & (kDestroyed | kErrored)) !== 0 && (state[kState] & kFinished) === 0;
          }
        }
      });
      const destroy = destroyImpl.destroy;
      Writable.prototype.destroy = function(err, cb) {
        const state = this._writableState;
        if ((state[kState] & (kBuffered | kOnFinished)) !== 0 && (state[kState] & kDestroyed) === 0) {
          process.nextTick(errorBuffer, state);
        }
        destroy.call(this, err, cb);
        return this;
      };
      Writable.prototype._undestroy = destroyImpl.undestroy;
      Writable.prototype._destroy = function(err, cb) {
        cb(err);
      };
      Writable.prototype[EE.captureRejectionSymbol] = function(err) {
        this.destroy(err);
      };
      let webStreamsAdapters;
      function lazyWebStreams() {
        if (webStreamsAdapters === undefined)
          webStreamsAdapters = require("internal/webstreams_adapters");
        return webStreamsAdapters;
      }
      Writable.fromWeb = function(writableStream, options) {
        return lazyWebStreams().newStreamWritableFromWritableStream(writableStream, options);
      };
      Writable.toWeb = function(streamWritable) {
        return lazyWebStreams().newWritableStreamFromStreamWritable(streamWritable);
      };
      Writable.prototype[SymbolAsyncDispose] = function() {
        let error;
        if (!this.destroyed) {
          error = this.writableFinished ? null : makeAbortError();
          this.destroy(error);
        }
        return new Promise((resolve, reject) => eos(this, (err) => err && err.name !== "AbortError" ? reject(err) : resolve(null)));
      };
      module.exports = Writable;

    },
  };

  // Os módulos que o bun resolve por dentro (`internal/*`) e que aqui vêm do `node_internal.js`.
  const provided = {
    "internal/validators": validators,
    "internal/errors": { aggregateTwoErrors },
    "internal/shared": { once, kEmptyObject, kWeakHandler, kAutoDestroyed, resistStopPropagation },
    "internal/abort_listener": { addAbortListener },
    "internal/primordials": { SafeSet, SafeArrayIterator },
    // Nenhum hook de `async_hooks` está ligado (o módulo ainda não existe), então o `eos` não embrulha o callback.
    "internal/async_hooks": { enabledHooksExist: () => false },
    "node:util/types": host.requireBuiltin("node:util/types"),
  };

  const loaded = { __proto__: null };
  function require(name) {
    if (name in provided) return provided[name];
    if (name in loaded) return loaded[name].exports;
    const define = definitions[name];
    if (define === undefined) return host.requireBuiltin(name);
    const module = { exports: {} };
    loaded[name] = module;
    define(module, undefined, require);
    return module.exports;
  }

  // O `require` interno fica guardado no hospedeiro para o `node:util`, que lê `internal/streams/utils` (o `isReadableStream`
  // e afins) sem que o objeto exportado de `stream` carregue nada a mais.
  host.registerRequire(require);
  return require("node:stream");
})
