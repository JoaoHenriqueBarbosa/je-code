// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/zlib.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { validators, toClass, codes, makeAbortError } = host.internal;
  const debug = () => {};
  const assert = () => {};
  const isObject = value => value !== null && (typeof value === "object" || typeof value === "function");
  // `$newPromise()` e `$resolvePromise()` do JSC: uma promessa sem executor, resolvida de fora.
  const promiseResolvers = new WeakMap();
  const newPromise = () => {
    let resolve;
    const promise = new Promise(settle => {
      resolve = settle;
    });
    promiseResolvers.set(promise, resolve);
    return promise;
  };
  const resolvePromise = (promise, value) => promiseResolvers.get(promise)(value);
  const isBlob = value => typeof Blob === "function" && value instanceof Blob;
  // O que ainda não existe (TLS, `BlockList`...): lança no ponto de uso, nunca finge.
  const notImplemented = what => () => host.internal.throwNotImplemented(what);
  const require = name => (name.startsWith("internal/") || name.startsWith("native/") ? host.internalModule(name) : host.requireBuiltin(name));
  return (() => {
    const BufferModule = require("node:buffer");
    const crc32 = ((hashCrc32) => function crc32(data, ...rest) {
          let value = rest[0];
          if (typeof data === "string") data = new TextEncoder().encode(data);
          else if (data === undefined) throw host.internal.codes.INVALID_ARG_TYPE("data", ["string", "Buffer", "TypedArray", "DataView"], data);
          else if (!ArrayBuffer.isView(data) && !(data instanceof ArrayBuffer) && !(data instanceof SharedArrayBuffer)) {
            throw host.internal.makeError("TypeError", "ERR_INVALID_ARG_TYPE", `The "data" property must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received ${data === null ? "object" : typeof data}`);
          }
          if (rest.length === 0) value = 0;
          else if (typeof value !== "number") throw host.internal.codes.INVALID_ARG_TYPE("value", "number", value);
          else if (!Number.isInteger(value)) throw host.internal.codes.OUT_OF_RANGE("value", "an integer", value);
          else if (value < 0 || value > 4294967295) throw host.internal.codes.OUT_OF_RANGE("value", ">= 0 and <= 4294967295", value);
          return hashCrc32(data instanceof ArrayBuffer || data instanceof SharedArrayBuffer ? new Uint8Array(data) : data, value);
        })(Bun.hash.crc32);
    const NativeZlib = (native => (toClass(native, "NativeZlib"), native))(function NativeZlib() { host.internal.throwNotImplemented("zlib.NativeZlib"); });
    const NativeBrotli = (native => (toClass(native, "NativeBrotli"), native))(function NativeBrotli() { host.internal.throwNotImplemented("zlib.NativeBrotli"); });
    const NativeZstd = (native => (toClass(native, "NativeZstd"), native))(function NativeZstd() { host.internal.throwNotImplemented("zlib.NativeZstd"); });
    const ObjectKeys = Object.keys;
    const ArrayPrototypePush = Array.prototype.push;
    const ObjectDefineProperty = Object.defineProperty;
    const ObjectDefineProperties = Object.defineProperties;
    const ObjectFreeze = Object.freeze;
    const TypedArrayPrototypeFill = Uint8Array.prototype.fill;
    const ArrayPrototypeForEach = Array.prototype.forEach;
    const NumberIsNaN = Number.isNaN;
    const MathMax = Math.max;
    const isArrayBufferView = ArrayBuffer.isView;
    const isAnyArrayBuffer = (b) => b instanceof ArrayBuffer || b instanceof SharedArrayBuffer;
    const kMaxLength = BufferModule.kMaxLength;
    const { Transform, finished } = require("node:stream");
    const owner_symbol = Symbol("owner_symbol");
    const { checkRangesOrGetDefault, validateFunction, validateFiniteNumber } = require("internal/validators");
    const kFlushFlag = Symbol("kFlushFlag");
    const kError = Symbol("kError");
    const { zlib: constants } = ({ zlib: Object.freeze({"Z_NO_FLUSH":0,"Z_PARTIAL_FLUSH":1,"Z_SYNC_FLUSH":2,"Z_FULL_FLUSH":3,"Z_FINISH":4,"Z_BLOCK":5,"Z_OK":0,"Z_STREAM_END":1,"Z_NEED_DICT":2,"Z_ERRNO":-1,"Z_STREAM_ERROR":-2,"Z_DATA_ERROR":-3,"Z_MEM_ERROR":-4,"Z_BUF_ERROR":-5,"Z_VERSION_ERROR":-6,"Z_NO_COMPRESSION":0,"Z_BEST_SPEED":1,"Z_BEST_COMPRESSION":9,"Z_DEFAULT_COMPRESSION":-1,"Z_FILTERED":1,"Z_HUFFMAN_ONLY":2,"Z_RLE":3,"Z_FIXED":4,"Z_DEFAULT_STRATEGY":0,"ZLIB_VERNUM":4880,"DEFLATE":1,"INFLATE":2,"GZIP":3,"GUNZIP":4,"DEFLATERAW":5,"INFLATERAW":6,"UNZIP":7,"BROTLI_DECODE":8,"BROTLI_ENCODE":9,"ZSTD_COMPRESS":10,"ZSTD_DECOMPRESS":11,"Z_MIN_WINDOWBITS":8,"Z_MAX_WINDOWBITS":15,"Z_DEFAULT_WINDOWBITS":15,"Z_MIN_CHUNK":64,"Z_MAX_CHUNK":Infinity,"Z_DEFAULT_CHUNK":16384,"Z_MIN_MEMLEVEL":1,"Z_MAX_MEMLEVEL":9,"Z_DEFAULT_MEMLEVEL":8,"Z_MIN_LEVEL":-1,"Z_MAX_LEVEL":9,"Z_DEFAULT_LEVEL":-1,"BROTLI_OPERATION_PROCESS":0,"BROTLI_OPERATION_FLUSH":1,"BROTLI_OPERATION_FINISH":2,"BROTLI_OPERATION_EMIT_METADATA":3,"BROTLI_PARAM_MODE":0,"BROTLI_MODE_GENERIC":0,"BROTLI_MODE_TEXT":1,"BROTLI_MODE_FONT":2,"BROTLI_DEFAULT_MODE":0,"BROTLI_PARAM_QUALITY":1,"BROTLI_MIN_QUALITY":0,"BROTLI_MAX_QUALITY":11,"BROTLI_DEFAULT_QUALITY":11,"BROTLI_PARAM_LGWIN":2,"BROTLI_MIN_WINDOW_BITS":10,"BROTLI_MAX_WINDOW_BITS":24,"BROTLI_LARGE_MAX_WINDOW_BITS":30,"BROTLI_DEFAULT_WINDOW":22,"BROTLI_PARAM_LGBLOCK":3,"BROTLI_MIN_INPUT_BLOCK_BITS":16,"BROTLI_MAX_INPUT_BLOCK_BITS":24,"BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING":4,"BROTLI_PARAM_SIZE_HINT":5,"BROTLI_PARAM_LARGE_WINDOW":6,"BROTLI_PARAM_NPOSTFIX":7,"BROTLI_PARAM_NDIRECT":8,"BROTLI_DECODER_RESULT_ERROR":0,"BROTLI_DECODER_RESULT_SUCCESS":1,"BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT":2,"BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT":3,"BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION":0,"BROTLI_DECODER_PARAM_LARGE_WINDOW":1,"BROTLI_DECODER_NO_ERROR":0,"BROTLI_DECODER_SUCCESS":1,"BROTLI_DECODER_NEEDS_MORE_INPUT":2,"BROTLI_DECODER_NEEDS_MORE_OUTPUT":3,"BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_NIBBLE":-1,"BROTLI_DECODER_ERROR_FORMAT_RESERVED":-2,"BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_META_NIBBLE":-3,"BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_ALPHABET":-4,"BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_SAME":-5,"BROTLI_DECODER_ERROR_FORMAT_CL_SPACE":-6,"BROTLI_DECODER_ERROR_FORMAT_HUFFMAN_SPACE":-7,"BROTLI_DECODER_ERROR_FORMAT_CONTEXT_MAP_REPEAT":-8,"BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_1":-9,"BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_2":-10,"BROTLI_DECODER_ERROR_FORMAT_TRANSFORM":-11,"BROTLI_DECODER_ERROR_FORMAT_DICTIONARY":-12,"BROTLI_DECODER_ERROR_FORMAT_WINDOW_BITS":-13,"BROTLI_DECODER_ERROR_FORMAT_PADDING_1":-14,"BROTLI_DECODER_ERROR_FORMAT_PADDING_2":-15,"BROTLI_DECODER_ERROR_FORMAT_DISTANCE":-16,"BROTLI_DECODER_ERROR_DICTIONARY_NOT_SET":-19,"BROTLI_DECODER_ERROR_INVALID_ARGUMENTS":-20,"BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MODES":-21,"BROTLI_DECODER_ERROR_ALLOC_TREE_GROUPS":-22,"BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MAP":-25,"BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_1":-26,"BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_2":-27,"BROTLI_DECODER_ERROR_ALLOC_BLOCK_TYPE_TREES":-30,"BROTLI_DECODER_ERROR_UNREACHABLE":-31,"ZSTD_e_continue":0,"ZSTD_e_flush":1,"ZSTD_e_end":2,"ZSTD_fast":1,"ZSTD_dfast":2,"ZSTD_greedy":3,"ZSTD_lazy":4,"ZSTD_lazy2":5,"ZSTD_btlazy2":6,"ZSTD_btopt":7,"ZSTD_btultra":8,"ZSTD_btultra2":9,"ZSTD_c_compressionLevel":100,"ZSTD_c_windowLog":101,"ZSTD_c_hashLog":102,"ZSTD_c_chainLog":103,"ZSTD_c_searchLog":104,"ZSTD_c_minMatch":105,"ZSTD_c_targetLength":106,"ZSTD_c_strategy":107,"ZSTD_c_enableLongDistanceMatching":160,"ZSTD_c_ldmHashLog":161,"ZSTD_c_ldmMinMatch":162,"ZSTD_c_ldmBucketSizeLog":163,"ZSTD_c_ldmHashRateLog":164,"ZSTD_c_contentSizeFlag":200,"ZSTD_c_checksumFlag":201,"ZSTD_c_dictIDFlag":202,"ZSTD_c_nbWorkers":400,"ZSTD_c_jobSize":401,"ZSTD_c_overlapLog":402,"ZSTD_d_windowLogMax":100,"ZSTD_CLEVEL_DEFAULT":3,"ZSTD_error_no_error":0,"ZSTD_error_GENERIC":1,"ZSTD_error_prefix_unknown":10,"ZSTD_error_version_unsupported":12,"ZSTD_error_frameParameter_unsupported":14,"ZSTD_error_frameParameter_windowTooLarge":16,"ZSTD_error_corruption_detected":20,"ZSTD_error_checksum_wrong":22,"ZSTD_error_literals_headerWrong":24,"ZSTD_error_dictionary_corrupted":30,"ZSTD_error_dictionary_wrong":32,"ZSTD_error_dictionaryCreation_failed":34,"ZSTD_error_parameter_unsupported":40,"ZSTD_error_parameter_combination_unsupported":41,"ZSTD_error_parameter_outOfBound":42,"ZSTD_error_tableLog_tooLarge":44,"ZSTD_error_maxSymbolValue_tooLarge":46,"ZSTD_error_maxSymbolValue_tooSmall":48,"ZSTD_error_stabilityCondition_notRespected":50,"ZSTD_error_stage_wrong":60,"ZSTD_error_init_missing":62,"ZSTD_error_memory_allocation":64,"ZSTD_error_workSpace_tooSmall":66,"ZSTD_error_dstSize_tooSmall":70,"ZSTD_error_srcSize_wrong":72,"ZSTD_error_dstBuffer_null":74,"ZSTD_error_noForwardProgress_destFull":80,"ZSTD_error_noForwardProgress_inputEmpty":82}) });
    const {
      Z_NO_FLUSH,
      Z_BLOCK,
      Z_PARTIAL_FLUSH,
      Z_SYNC_FLUSH,
      Z_FULL_FLUSH,
      Z_FINISH,
      Z_MIN_CHUNK,
      Z_MIN_WINDOWBITS,
      Z_MAX_WINDOWBITS,
      Z_MIN_LEVEL,
      Z_MAX_LEVEL,
      Z_MIN_MEMLEVEL,
      Z_MAX_MEMLEVEL,
      Z_DEFAULT_CHUNK,
      Z_DEFAULT_COMPRESSION,
      Z_DEFAULT_STRATEGY,
      Z_DEFAULT_WINDOWBITS,
      Z_DEFAULT_MEMLEVEL,
      Z_FIXED,
      DEFLATE,
      DEFLATERAW,
      INFLATE,
      INFLATERAW,
      GZIP,
      GUNZIP,
      UNZIP,
      BROTLI_DECODE,
      BROTLI_ENCODE,
      ZSTD_COMPRESS,
      ZSTD_DECOMPRESS,
      BROTLI_OPERATION_PROCESS,
      BROTLI_OPERATION_FLUSH,
      BROTLI_OPERATION_FINISH,
      BROTLI_OPERATION_EMIT_METADATA,
      ZSTD_e_continue,
      ZSTD_e_flush,
      ZSTD_e_end
    } = constants;
    const codes = {
      Z_OK: constants.Z_OK,
      Z_STREAM_END: constants.Z_STREAM_END,
      Z_NEED_DICT: constants.Z_NEED_DICT,
      Z_ERRNO: constants.Z_ERRNO,
      Z_STREAM_ERROR: constants.Z_STREAM_ERROR,
      Z_DATA_ERROR: constants.Z_DATA_ERROR,
      Z_MEM_ERROR: constants.Z_MEM_ERROR,
      Z_BUF_ERROR: constants.Z_BUF_ERROR,
      Z_VERSION_ERROR: constants.Z_VERSION_ERROR
    };
    for (const ckey of ObjectKeys(codes)) {
      codes[codes[ckey]] = ckey;
    }
    function zlibBuffer(engine, buffer, callback) {
      validateFunction(callback, "callback");
      if (isArrayBufferView(buffer)) {
        buffer = Buffer.from(buffer.buffer, buffer.byteOffset, buffer.byteLength);
      } else if (isAnyArrayBuffer(buffer)) {
        buffer = Buffer.from(buffer);
      }
      engine.buffers = null;
      engine.nread = 0;
      engine.cb = callback;
      engine.on("data", zlibBufferOnData);
      engine.on("error", zlibBufferOnError);
      engine.on("end", zlibBufferOnEnd);
      engine.end(buffer);
    }
    function zlibBufferOnData(chunk) {
      if (!this.buffers)
        this.buffers = [chunk];
      else
        ArrayPrototypePush.call(this.buffers, chunk);
      this.nread += chunk.length;
      const maxOutputLength = this._maxOutputLength;
      if (this.nread > maxOutputLength) {
        this.close();
        this.removeAllListeners("end");
        this.cb(codes.BUFFER_TOO_LARGE(maxOutputLength));
      }
    }
    function zlibBufferOnError(err) {
      this.removeAllListeners("end");
      this.cb(err);
    }
    function zlibBufferOnEnd() {
      let buf;
      if (this.nread === 0) {
        buf = Buffer.alloc(0);
      } else {
        const bufs = this.buffers;
        buf = bufs.length === 1 ? bufs[0] : Buffer.concat(bufs, this.nread);
      }
      this.close();
      if (this._info)
        this.cb(null, { buffer: buf, engine: this });
      else
        this.cb(null, buf);
    }
    function zlibBufferSync(engine, buffer) {
      if (typeof buffer === "string") {
        buffer = Buffer.from(buffer);
      } else if (!isArrayBufferView(buffer)) {
        if (isAnyArrayBuffer(buffer)) {
          buffer = Buffer.from(buffer);
        } else {
          throw codes.INVALID_ARG_TYPE("buffer", "string, Buffer, TypedArray, DataView, or ArrayBuffer", buffer);
        }
      }
      buffer = processChunkSync(engine, buffer, engine._finishFlushFlag);
      if (engine._info)
        return { buffer, engine };
      return buffer;
    }
    function zlibOnError(message, errno, code) {
      const self = this[owner_symbol];
      const error = new Error(message);
      error.errno = errno;
      error.code = code;
      self.destroy(error);
      self[kError] = error;
    }
    const FLUSH_BOUND = [
      [Z_NO_FLUSH, Z_BLOCK],
      [BROTLI_OPERATION_PROCESS, BROTLI_OPERATION_EMIT_METADATA],
      [ZSTD_e_continue, ZSTD_e_end]
    ];
    const FLUSH_BOUND_IDX_NORMAL = 0;
    const FLUSH_BOUND_IDX_BROTLI = 1;
    const FLUSH_BOUND_IDX_ZSTD = 2;
    function ZlibBase(opts, mode, handle, { flush, finishFlush, fullFlush }) {
      let chunkSize = Z_DEFAULT_CHUNK;
      let maxOutputLength = kMaxLength;
      assert(typeof mode === "number");
      assert(mode >= DEFLATE && mode <= ZSTD_DECOMPRESS);
      let flushBoundIdx;
      if (mode === BROTLI_ENCODE || mode === BROTLI_DECODE) {
        flushBoundIdx = FLUSH_BOUND_IDX_BROTLI;
      } else if (mode === ZSTD_COMPRESS || mode === ZSTD_DECOMPRESS) {
        flushBoundIdx = FLUSH_BOUND_IDX_ZSTD;
      } else {
        flushBoundIdx = FLUSH_BOUND_IDX_NORMAL;
      }
      if (opts) {
        chunkSize = opts.chunkSize;
        if (!validateFiniteNumber(chunkSize, "options.chunkSize")) {
          chunkSize = Z_DEFAULT_CHUNK;
        } else if (chunkSize < Z_MIN_CHUNK) {
          throw codes.OUT_OF_RANGE("options.chunkSize", `>= ${Z_MIN_CHUNK}`, chunkSize);
        }
        flush = checkRangesOrGetDefault(opts.flush, "options.flush", FLUSH_BOUND[flushBoundIdx][0], FLUSH_BOUND[flushBoundIdx][1], flush);
        finishFlush = checkRangesOrGetDefault(opts.finishFlush, "options.finishFlush", FLUSH_BOUND[flushBoundIdx][0], FLUSH_BOUND[flushBoundIdx][1], finishFlush);
        maxOutputLength = checkRangesOrGetDefault(opts.maxOutputLength, "options.maxOutputLength", 1, kMaxLength, kMaxLength);
        if (opts.encoding || opts.objectMode || opts.writableObjectMode) {
          opts = { ...opts };
          opts.encoding = null;
          opts.objectMode = false;
          opts.writableObjectMode = false;
        }
      }
      Transform.apply(this, [{ autoDestroy: true, ...opts }]);
      this[kError] = null;
      this.bytesWritten = 0;
      this._handle = handle;
      handle[owner_symbol] = this;
      handle.onerror = zlibOnError;
      this._outBuffer = Buffer.allocUnsafe(chunkSize);
      this._outOffset = 0;
      this._chunkSize = chunkSize;
      this._defaultFlushFlag = flush;
      this._finishFlushFlag = finishFlush;
      this._defaultFullFlushFlag = fullFlush;
      this._info = opts && opts.info;
      this._maxOutputLength = maxOutputLength;
    }
    toClass(ZlibBase, "ZlibBase", Transform);
    ObjectDefineProperty(ZlibBase.prototype, "_closed", {
      configurable: true,
      enumerable: true,
      get() {
        return !this._handle;
      }
    });
    ObjectDefineProperty(ZlibBase.prototype, "bytesRead", {
      configurable: true,
      get: function() {
        return this.bytesWritten;
      },
      set: function(value) {
        this.bytesWritten = value;
      }
    });
    ZlibBase.prototype.reset = function() {
      assert(this._handle, "zlib binding closed");
      return this._handle.reset();
    };
    ZlibBase.prototype._flush = function(callback) {
      this._transform(Buffer.alloc(0), "", callback);
    };
    ZlibBase.prototype._final = function(callback) {
      callback();
    };
    const flushiness = [];
    const kFlushFlagList = [Z_NO_FLUSH, Z_BLOCK, Z_PARTIAL_FLUSH, Z_SYNC_FLUSH, Z_FULL_FLUSH, Z_FINISH];
    for (let i = 0;i < kFlushFlagList.length; i++) {
      flushiness[kFlushFlagList[i]] = i;
    }
    function maxFlush(a, b) {
      return flushiness[a] > flushiness[b] ? a : b;
    }
    const kFlushBuffers = [];
    {
      const dummyArrayBuffer = new ArrayBuffer;
      for (const flushFlag of kFlushFlagList) {
        kFlushBuffers[flushFlag] = Buffer.from(dummyArrayBuffer);
        kFlushBuffers[flushFlag][kFlushFlag] = flushFlag;
      }
    }
    ZlibBase.prototype.flush = function(kind, callback) {
      if (typeof kind === "function" || kind === undefined && !callback) {
        callback = kind;
        kind = this._defaultFullFlushFlag;
      }
      if (this.writableFinished) {
        if (callback)
          process.nextTick(callback);
      } else if (this.writableEnded) {
        if (callback)
          this.once("end", callback);
      } else {
        this.write(kFlushBuffers[kind], "", callback);
      }
    };
    ZlibBase.prototype.close = function(callback) {
      if (callback)
        finished(this, callback);
      this.destroy();
    };
    ZlibBase.prototype._destroy = function(err, callback) {
      _close(this);
      callback(err);
    };
    ZlibBase.prototype._transform = function(chunk, encoding, cb) {
      let flushFlag = this._defaultFlushFlag;
      if (typeof chunk[kFlushFlag] === "number") {
        flushFlag = chunk[kFlushFlag];
      }
      if (this.writableEnded && this.writableLength === chunk.byteLength) {
        flushFlag = maxFlush(flushFlag, this._finishFlushFlag);
      }
      processChunk(this, chunk, flushFlag, cb);
    };
    ZlibBase.prototype._processChunk = function(chunk, flushFlag, cb) {
      if (typeof cb === "function")
        processChunk(this, chunk, flushFlag, cb);
      else
        return processChunkSync(this, chunk, flushFlag);
    };
    function processChunkSync(self, chunk, flushFlag) {
      let availInBefore = chunk.byteLength;
      let availOutBefore = self._chunkSize - self._outOffset;
      let inOff = 0;
      let availOutAfter;
      let availInAfter;
      const buffers = [];
      let nread = 0;
      let inputRead = 0;
      const state = self._writeState;
      const handle = self._handle;
      let buffer = self._outBuffer;
      let offset = self._outOffset;
      const chunkSize = self._chunkSize;
      let error;
      self.on("error", function onError(er) {
        error = er;
      });
      while (true) {
        handle.writeSync(flushFlag, chunk, inOff, availInBefore, buffer, offset, availOutBefore);
        if (error) {
          if (typeof error === "string") {
            error = new Error(error);
          } else if (!Error.isError(error)) {
            error = new Error(String(error));
          }
          throw error;
        } else if (self[kError])
          throw self[kError];
        availOutAfter = state[0];
        availInAfter = state[1];
        const inDelta = availInBefore - availInAfter;
        inputRead += inDelta;
        const have = availOutBefore - availOutAfter;
        if (have > 0) {
          const out = buffer.slice(offset, offset + have);
          offset += have;
          ArrayPrototypePush.call(buffers, out);
          nread += out.byteLength;
          const maxOutputLength = self._maxOutputLength;
          if (nread > maxOutputLength) {
            _close(self);
            throw codes.BUFFER_TOO_LARGE(maxOutputLength);
          }
        } else {
          assert(have === 0, "have should not go down");
        }
        if (availOutAfter === 0 || offset >= chunkSize) {
          availOutBefore = chunkSize;
          offset = 0;
          buffer = Buffer.allocUnsafe(chunkSize);
        }
        if (availOutAfter === 0) {
          inOff += inDelta;
          availInBefore = availInAfter;
        } else {
          break;
        }
      }
      self.bytesWritten = inputRead;
      _close(self);
      if (nread === 0)
        return Buffer.alloc(0);
      return buffers.length === 1 ? buffers[0] : Buffer.concat(buffers, nread);
    }
    function processChunk(self, chunk, flushFlag, cb) {
      const handle = self._handle;
      if (!handle)
        return process.nextTick(cb);
      handle.buffer = chunk;
      handle.cb = cb;
      handle.availOutBefore = self._chunkSize - self._outOffset;
      handle.availInBefore = chunk.byteLength;
      handle.inOff = 0;
      handle.flushFlag = flushFlag;
      handle.write(flushFlag, chunk, 0, handle.availInBefore, self._outBuffer, self._outOffset, handle.availOutBefore);
    }
    function processCallback() {
      const handle = this;
      const self = this[owner_symbol];
      const state = self._writeState;
      if (self.destroyed) {
        this.buffer = null;
        this.cb();
        return;
      }
      const availOutAfter = state[0];
      const availInAfter = state[1];
      const inDelta = handle.availInBefore - availInAfter;
      self.bytesWritten += inDelta;
      const have = handle.availOutBefore - availOutAfter;
      let streamBufferIsFull = false;
      if (have > 0) {
        const out = self._outBuffer.slice(self._outOffset, self._outOffset + have);
        self._outOffset += have;
        streamBufferIsFull = !self.push(out);
      } else {
        assert(have === 0, "have should not go down");
      }
      if (self.destroyed) {
        this.cb();
        return;
      }
      let chunkSize;
      if (availOutAfter === 0 || self._outOffset >= (chunkSize = self._chunkSize)) {
        chunkSize ??= self._chunkSize;
        handle.availOutBefore = chunkSize;
        self._outOffset = 0;
        self._outBuffer = Buffer.allocUnsafe(chunkSize);
      }
      if (availOutAfter === 0) {
        handle.inOff += inDelta;
        handle.availInBefore = availInAfter;
        if (!streamBufferIsFull) {
          this.write(handle.flushFlag, this.buffer, handle.inOff, handle.availInBefore, self._outBuffer, self._outOffset, self._chunkSize);
        } else {
          const oldRead = self._read;
          self._read = (n) => {
            self._read = oldRead;
            this.write(handle.flushFlag, this.buffer, handle.inOff, handle.availInBefore, self._outBuffer, self._outOffset, self._chunkSize);
            self._read(n);
          };
        }
        return;
      }
      if (availInAfter > 0) {
        self.push(null);
      }
      this.buffer = null;
      this.cb();
    }
    function _close(engine) {
      engine._handle?.close();
      engine._handle = null;
    }
    const zlibDefaultOpts = {
      flush: Z_NO_FLUSH,
      finishFlush: Z_FINISH,
      fullFlush: Z_FULL_FLUSH
    };
    function Zlib(opts, mode) {
      let windowBits = Z_DEFAULT_WINDOWBITS;
      let level = Z_DEFAULT_COMPRESSION;
      let memLevel = Z_DEFAULT_MEMLEVEL;
      let strategy = Z_DEFAULT_STRATEGY;
      let dictionary;
      if (opts) {
        if ((opts.windowBits == null || opts.windowBits === 0) && (mode === INFLATE || mode === GUNZIP || mode === UNZIP)) {
          windowBits = 0;
        } else {
          const min = Z_MIN_WINDOWBITS + (mode === GZIP ? 1 : 0);
          windowBits = checkRangesOrGetDefault(opts.windowBits, "options.windowBits", min, Z_MAX_WINDOWBITS, Z_DEFAULT_WINDOWBITS);
        }
        level = checkRangesOrGetDefault(opts.level, "options.level", Z_MIN_LEVEL, Z_MAX_LEVEL, Z_DEFAULT_COMPRESSION);
        memLevel = checkRangesOrGetDefault(opts.memLevel, "options.memLevel", Z_MIN_MEMLEVEL, Z_MAX_MEMLEVEL, Z_DEFAULT_MEMLEVEL);
        strategy = checkRangesOrGetDefault(opts.strategy, "options.strategy", Z_DEFAULT_STRATEGY, Z_FIXED, Z_DEFAULT_STRATEGY);
        dictionary = opts.dictionary;
        if (dictionary !== undefined && !isArrayBufferView(dictionary)) {
          if (isAnyArrayBuffer(dictionary)) {
            dictionary = Buffer.from(dictionary);
          } else {
            throw codes.INVALID_ARG_TYPE("options.dictionary", ["Buffer", "TypedArray", "DataView", "ArrayBuffer"], dictionary);
          }
        }
      }
      const handle = new NativeZlib(mode);
      this._writeState = new Uint32Array(2);
      handle.init(windowBits, level, memLevel, strategy, this._writeState, processCallback, dictionary);
      ZlibBase.apply(this, [opts, mode, handle, zlibDefaultOpts]);
      this._level = level;
      this._strategy = strategy;
    }
    toClass(Zlib, "Zlib", ZlibBase);
    function paramsAfterFlushCallback(level, strategy, callback) {
      assert(this._handle, "zlib binding closed");
      this._handle.params(level, strategy);
      if (!this.destroyed) {
        this._level = level;
        this._strategy = strategy;
        if (callback)
          callback();
      }
    }
    Zlib.prototype.params = function params(level, strategy, callback) {
      checkRangesOrGetDefault(level, "level", Z_MIN_LEVEL, Z_MAX_LEVEL);
      checkRangesOrGetDefault(strategy, "strategy", Z_DEFAULT_STRATEGY, Z_FIXED);
      if (this._level !== level || this._strategy !== strategy) {
        this.flush(Z_SYNC_FLUSH, paramsAfterFlushCallback.bind(this, level, strategy, callback));
      } else {
        process.nextTick(callback);
      }
    };
    function Deflate(opts) {
      if (!(this instanceof Deflate))
        return new Deflate(opts);
      Zlib.apply(this, [opts, DEFLATE]);
    }
    toClass(Deflate, "Deflate", Zlib);
    function Inflate(opts) {
      if (!(this instanceof Inflate))
        return new Inflate(opts);
      Zlib.apply(this, [opts, INFLATE]);
    }
    toClass(Inflate, "Inflate", Zlib);
    function Gzip(opts) {
      if (!(this instanceof Gzip))
        return new Gzip(opts);
      Zlib.apply(this, [opts, GZIP]);
    }
    toClass(Gzip, "Gzip", Zlib);
    function Gunzip(opts) {
      if (!(this instanceof Gunzip))
        return new Gunzip(opts);
      Zlib.apply(this, [opts, GUNZIP]);
    }
    toClass(Gunzip, "Gunzip", Zlib);
    function DeflateRaw(opts) {
      if (opts && opts.windowBits === 8)
        opts.windowBits = 9;
      if (!(this instanceof DeflateRaw))
        return new DeflateRaw(opts);
      Zlib.apply(this, [opts, DEFLATERAW]);
    }
    toClass(DeflateRaw, "DeflateRaw", Zlib);
    function InflateRaw(opts) {
      if (!(this instanceof InflateRaw))
        return new InflateRaw(opts);
      Zlib.apply(this, [opts, INFLATERAW]);
    }
    toClass(InflateRaw, "InflateRaw", Zlib);
    function Unzip(opts) {
      if (!(this instanceof Unzip))
        return new Unzip(opts);
      Zlib.apply(this, [opts, UNZIP]);
    }
    toClass(Unzip, "Unzip", Zlib);
    function createConvenienceMethod(ctor, sync, methodName, isZstd) {
      if (sync) {
        const fn = function(buffer, opts) {
          return zlibBufferSync(new ctor(opts), buffer);
        };
        ObjectDefineProperty(fn, "name", { value: methodName });
        return fn;
      } else {
        const fn = function(buffer, opts, callback) {
          if (typeof opts === "function") {
            callback = opts;
            opts = {};
          }
          if (isZstd) {
            let bufferSize;
            if (typeof buffer === "string") {
              bufferSize = Buffer.byteLength(buffer);
            } else if (isArrayBufferView(buffer)) {
              bufferSize = buffer.byteLength;
            } else if (isAnyArrayBuffer(buffer)) {
              bufferSize = buffer.byteLength;
            } else {
              bufferSize = 0;
            }
            if (!opts?.pledgedSrcSize && bufferSize > 0) {
              opts = { ...opts, pledgedSrcSize: bufferSize };
            }
          }
          return zlibBuffer(new ctor(opts), buffer, callback);
        };
        ObjectDefineProperty(fn, "name", { value: methodName });
        return fn;
      }
    }
    const kMaxBrotliParam = 9;
    const brotliInitParamsArray = new Uint32Array(kMaxBrotliParam + 1);
    const brotliDefaultOpts = {
      flush: BROTLI_OPERATION_PROCESS,
      finishFlush: BROTLI_OPERATION_FINISH,
      fullFlush: BROTLI_OPERATION_FLUSH
    };
    function Brotli(opts, mode) {
      assert(mode === BROTLI_DECODE || mode === BROTLI_ENCODE);
      TypedArrayPrototypeFill.call(brotliInitParamsArray, -1);
      if (opts?.params) {
        ArrayPrototypeForEach.call(ObjectKeys(opts.params), (origKey) => {
          const key = +origKey;
          if (NumberIsNaN(key) || key < 0 || key > kMaxBrotliParam || (brotliInitParamsArray[key] | 0) !== -1) {
            throw codes.BROTLI_INVALID_PARAM(origKey);
          }
          const value = opts.params[origKey];
          if (typeof value !== "number" && typeof value !== "boolean") {
            throw codes.INVALID_ARG_TYPE("options.params[key]", "number", opts.params[origKey]);
          }
          brotliInitParamsArray[key] = value;
        });
      }
      let dictionary = opts?.dictionary;
      if (dictionary !== undefined && !isArrayBufferView(dictionary)) {
        if (isAnyArrayBuffer(dictionary)) {
          dictionary = Buffer.from(dictionary);
        } else {
          throw codes.INVALID_ARG_TYPE("options.dictionary", ["Buffer", "TypedArray", "DataView", "ArrayBuffer"], dictionary);
        }
      }
      const handle = new NativeBrotli(mode);
      this._writeState = new Uint32Array(2);
      if (!handle.init(brotliInitParamsArray, this._writeState, processCallback, dictionary)) {
        throw codes.ZLIB_INITIALIZATION_FAILED();
      }
      ZlibBase.apply(this, [opts, mode, handle, brotliDefaultOpts]);
    }
    toClass(Brotli, "Brotli", Zlib);
    function BrotliCompress(opts) {
      if (!(this instanceof BrotliCompress))
        return new BrotliCompress(opts);
      Brotli.apply(this, [opts, BROTLI_ENCODE]);
    }
    toClass(BrotliCompress, "BrotliCompress", Brotli);
    function BrotliDecompress(opts) {
      if (!(this instanceof BrotliDecompress))
        return new BrotliDecompress(opts);
      Brotli.apply(this, [opts, BROTLI_DECODE]);
    }
    toClass(BrotliDecompress, "BrotliDecompress", Brotli);
    const zstdDefaultOpts = {
      flush: ZSTD_e_continue,
      finishFlush: ZSTD_e_end,
      fullFlush: ZSTD_e_flush
    };

    class Zstd extends ZlibBase {
      constructor(opts, mode, initParamsArray, maxParam) {
        assert(mode === ZSTD_COMPRESS || mode === ZSTD_DECOMPRESS);
        initParamsArray.fill(-1);
        if (opts?.params) {
          ObjectKeys(opts.params).forEach((origKey) => {
            const key = +origKey;
            if (NumberIsNaN(key) || key < 0 || key > maxParam || (initParamsArray[key] | 0) !== -1) {
              throw codes.ZSTD_INVALID_PARAM(origKey);
            }
            const value = opts.params[origKey];
            if (typeof value !== "number" && typeof value !== "boolean") {
              throw codes.INVALID_ARG_TYPE("options.params[key]", "number", opts.params[origKey]);
            }
            initParamsArray[key] = value;
          });
        }
        const handle = new NativeZstd(mode);
        const pledgedSrcSize = opts?.pledgedSrcSize ?? undefined;
        const writeState = new Uint32Array(2);
        handle.init(initParamsArray, pledgedSrcSize, writeState, processCallback, opts?.dictionary && isArrayBufferView(opts.dictionary) ? opts.dictionary : undefined);
        super(opts, mode, handle, zstdDefaultOpts);
        this._writeState = writeState;
      }
    }
    const kMaxZstdCParam = MathMax(...ObjectKeys(constants).map((key) => key.startsWith("ZSTD_c_") ? constants[key] : 0));
    const zstdInitCParamsArray = new Uint32Array(kMaxZstdCParam + 1);

    class ZstdCompress extends Zstd {
      constructor(opts) {
        super(opts, ZSTD_COMPRESS, zstdInitCParamsArray, kMaxZstdCParam);
      }
    }
    const kMaxZstdDParam = MathMax(...ObjectKeys(constants).map((key) => key.startsWith("ZSTD_d_") ? constants[key] : 0));
    const zstdInitDParamsArray = new Uint32Array(kMaxZstdDParam + 1);

    class ZstdDecompress extends Zstd {
      constructor(opts) {
        super(opts, ZSTD_DECOMPRESS, zstdInitDParamsArray, kMaxZstdDParam);
      }
    }
    ObjectDefineProperty(NativeZlib.prototype, "jsref", {
      __proto__: null,
      get() {
        return this[owner_symbol];
      },
      set(v) {
        return this[owner_symbol] = v;
      }
    });
    const zlib = {
      crc32,
      Deflate,
      Inflate,
      Gzip,
      Gunzip,
      DeflateRaw,
      InflateRaw,
      Unzip,
      BrotliCompress,
      BrotliDecompress,
      ZstdCompress,
      ZstdDecompress,
      deflate: createConvenienceMethod(Deflate, false, "deflate"),
      deflateSync: createConvenienceMethod(Deflate, true, "deflateSync"),
      gzip: createConvenienceMethod(Gzip, false, "gzip"),
      gzipSync: createConvenienceMethod(Gzip, true, "gzipSync"),
      deflateRaw: createConvenienceMethod(DeflateRaw, false, "deflateRaw"),
      deflateRawSync: createConvenienceMethod(DeflateRaw, true, "deflateRawSync"),
      unzip: createConvenienceMethod(Unzip, false, "unzip"),
      unzipSync: createConvenienceMethod(Unzip, true, "unzipSync"),
      inflate: createConvenienceMethod(Inflate, false, "inflate"),
      inflateSync: createConvenienceMethod(Inflate, true, "inflateSync"),
      gunzip: createConvenienceMethod(Gunzip, false, "gunzip"),
      gunzipSync: createConvenienceMethod(Gunzip, true, "gunzipSync"),
      inflateRaw: createConvenienceMethod(InflateRaw, false, "inflateRaw"),
      inflateRawSync: createConvenienceMethod(InflateRaw, true, "inflateRawSync"),
      brotliCompress: createConvenienceMethod(BrotliCompress, false, "brotliCompress"),
      brotliCompressSync: createConvenienceMethod(BrotliCompress, true, "brotliCompressSync"),
      brotliDecompress: createConvenienceMethod(BrotliDecompress, false, "brotliDecompress"),
      brotliDecompressSync: createConvenienceMethod(BrotliDecompress, true, "brotliDecompressSync"),
      zstdCompress: createConvenienceMethod(ZstdCompress, false, "zstdCompress", true),
      zstdCompressSync: createConvenienceMethod(ZstdCompress, true, "zstdCompressSync"),
      zstdDecompress: createConvenienceMethod(ZstdDecompress, false, "zstdDecompress"),
      zstdDecompressSync: createConvenienceMethod(ZstdDecompress, true, "zstdDecompressSync"),
      createDeflate: function(options) {
        return new Deflate(options);
      },
      createInflate: function(options) {
        return new Inflate(options);
      },
      createDeflateRaw: function(options) {
        return new DeflateRaw(options);
      },
      createInflateRaw: function(options) {
        return new InflateRaw(options);
      },
      createGzip: function(options) {
        return new Gzip(options);
      },
      createGunzip: function(options) {
        return new Gunzip(options);
      },
      createUnzip: function(options) {
        return new Unzip(options);
      },
      createBrotliCompress: function(options) {
        return new BrotliCompress(options);
      },
      createBrotliDecompress: function(options) {
        return new BrotliDecompress(options);
      },
      createZstdCompress: function(options) {
        return new ZstdCompress(options);
      },
      createZstdDecompress: function(options) {
        return new ZstdDecompress(options);
      }
    };
    ObjectDefineProperties(zlib, {
      constants: {
        enumerable: true,
        value: ObjectFreeze(constants)
      },
      codes: {
        enumerable: true,
        value: ObjectFreeze(codes)
      }
    });
    {
      const { Z_OK, Z_STREAM_END, Z_NEED_DICT, Z_ERRNO, Z_STREAM_ERROR, Z_DATA_ERROR, Z_MEM_ERROR, Z_BUF_ERROR, Z_VERSION_ERROR, Z_NO_COMPRESSION, Z_BEST_SPEED, Z_BEST_COMPRESSION, Z_DEFAULT_COMPRESSION, Z_FILTERED, Z_HUFFMAN_ONLY, Z_RLE, ZLIB_VERNUM, Z_MAX_CHUNK, Z_DEFAULT_LEVEL } = constants;
      ObjectDefineProperty(zlib, "Z_NO_FLUSH", { value: Z_NO_FLUSH });
      ObjectDefineProperty(zlib, "Z_PARTIAL_FLUSH", { value: Z_PARTIAL_FLUSH });
      ObjectDefineProperty(zlib, "Z_SYNC_FLUSH", { value: Z_SYNC_FLUSH });
      ObjectDefineProperty(zlib, "Z_FULL_FLUSH", { value: Z_FULL_FLUSH });
      ObjectDefineProperty(zlib, "Z_FINISH", { value: Z_FINISH });
      ObjectDefineProperty(zlib, "Z_BLOCK", { value: Z_BLOCK });
      ObjectDefineProperty(zlib, "Z_OK", { value: Z_OK });
      ObjectDefineProperty(zlib, "Z_STREAM_END", { value: Z_STREAM_END });
      ObjectDefineProperty(zlib, "Z_NEED_DICT", { value: Z_NEED_DICT });
      ObjectDefineProperty(zlib, "Z_ERRNO", { value: Z_ERRNO });
      ObjectDefineProperty(zlib, "Z_STREAM_ERROR", { value: Z_STREAM_ERROR });
      ObjectDefineProperty(zlib, "Z_DATA_ERROR", { value: Z_DATA_ERROR });
      ObjectDefineProperty(zlib, "Z_MEM_ERROR", { value: Z_MEM_ERROR });
      ObjectDefineProperty(zlib, "Z_BUF_ERROR", { value: Z_BUF_ERROR });
      ObjectDefineProperty(zlib, "Z_VERSION_ERROR", { value: Z_VERSION_ERROR });
      ObjectDefineProperty(zlib, "Z_NO_COMPRESSION", { value: Z_NO_COMPRESSION });
      ObjectDefineProperty(zlib, "Z_BEST_SPEED", { value: Z_BEST_SPEED });
      ObjectDefineProperty(zlib, "Z_BEST_COMPRESSION", { value: Z_BEST_COMPRESSION });
      ObjectDefineProperty(zlib, "Z_DEFAULT_COMPRESSION", { value: Z_DEFAULT_COMPRESSION });
      ObjectDefineProperty(zlib, "Z_FILTERED", { value: Z_FILTERED });
      ObjectDefineProperty(zlib, "Z_HUFFMAN_ONLY", { value: Z_HUFFMAN_ONLY });
      ObjectDefineProperty(zlib, "Z_RLE", { value: Z_RLE });
      ObjectDefineProperty(zlib, "Z_FIXED", { value: Z_FIXED });
      ObjectDefineProperty(zlib, "Z_DEFAULT_STRATEGY", { value: Z_DEFAULT_STRATEGY });
      ObjectDefineProperty(zlib, "ZLIB_VERNUM", { value: ZLIB_VERNUM });
      ObjectDefineProperty(zlib, "DEFLATE", { value: DEFLATE });
      ObjectDefineProperty(zlib, "INFLATE", { value: INFLATE });
      ObjectDefineProperty(zlib, "GZIP", { value: GZIP });
      ObjectDefineProperty(zlib, "GUNZIP", { value: GUNZIP });
      ObjectDefineProperty(zlib, "DEFLATERAW", { value: DEFLATERAW });
      ObjectDefineProperty(zlib, "INFLATERAW", { value: INFLATERAW });
      ObjectDefineProperty(zlib, "UNZIP", { value: UNZIP });
      ObjectDefineProperty(zlib, "Z_MIN_WINDOWBITS", { value: Z_MIN_WINDOWBITS });
      ObjectDefineProperty(zlib, "Z_MAX_WINDOWBITS", { value: Z_MAX_WINDOWBITS });
      ObjectDefineProperty(zlib, "Z_DEFAULT_WINDOWBITS", { value: Z_DEFAULT_WINDOWBITS });
      ObjectDefineProperty(zlib, "Z_MIN_CHUNK", { value: Z_MIN_CHUNK });
      ObjectDefineProperty(zlib, "Z_MAX_CHUNK", { value: Z_MAX_CHUNK });
      ObjectDefineProperty(zlib, "Z_DEFAULT_CHUNK", { value: Z_DEFAULT_CHUNK });
      ObjectDefineProperty(zlib, "Z_MIN_MEMLEVEL", { value: Z_MIN_MEMLEVEL });
      ObjectDefineProperty(zlib, "Z_MAX_MEMLEVEL", { value: Z_MAX_MEMLEVEL });
      ObjectDefineProperty(zlib, "Z_DEFAULT_MEMLEVEL", { value: Z_DEFAULT_MEMLEVEL });
      ObjectDefineProperty(zlib, "Z_MIN_LEVEL", { value: Z_MIN_LEVEL });
      ObjectDefineProperty(zlib, "Z_MAX_LEVEL", { value: Z_MAX_LEVEL });
      ObjectDefineProperty(zlib, "Z_DEFAULT_LEVEL", { value: Z_DEFAULT_LEVEL });
    }
    return zlib;
  })();
})
