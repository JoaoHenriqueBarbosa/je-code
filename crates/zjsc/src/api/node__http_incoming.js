// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/_http_incoming.ts` do bun 1.4.2. Não editar à mão.
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
    const Readable = require("internal/streams/readable");
    const finished = require("internal/streams/end-of-stream");
    const {
      abortedSymbol,
      eofInProgress,
      kHandle,
      noBodySymbol,
      typeSymbol,
      NodeHTTPIncomingRequestType,
      fakeSocketSymbol,
      emitErrorNextTickIfErrorListenerNT,
      NodeHTTPBodyReadState,
      emitEOFIncomingMessage,
      onDataIncomingMessage,
      kAbortController
    } = require("internal/http");
    const { FakeSocket } = require("internal/http/FakeSocket");
    const ObjectDefineProperty = Object.defineProperty;
    const ArrayPrototypeSlice = Array.prototype.slice;
    const kHeaders = Symbol("kHeaders");
    const kReqShouldKeepAlive = Symbol("kReqShouldKeepAlive");
    const kHeadersDistinct = Symbol("kHeadersDistinct");
    const kHeadersCount = Symbol("kHeadersCount");
    const kRawHeaders = Symbol("kRawHeaders");
    const kHeaderSource = Symbol("kHeaderSource");
    const kTrailers = Symbol("kTrailers");
    const kTrailersDistinct = Symbol("kTrailersDistinct");
    const kTrailersCount = Symbol("kTrailersCount");
    function readStart(socket) {
      if (socket && !socket._paused && socket.readable)
        socket.resume();
    }
    function readStop(socket) {
      if (socket)
        socket.pause();
    }
    function onIncomingMessagePauseNodeHTTPResponse() {
      const handle = this[kHandle];
      if (handle && !this.destroyed) {
        handle.pause();
      }
    }
    function onIncomingMessageResumeNodeHTTPResponse() {
      const handle = this[kHandle];
      if (handle && !this.destroyed) {
        const resumed = handle.resume();
        if (resumed && resumed !== true) {
          const bodyReadState = handle.hasBody;
          if ((bodyReadState & NodeHTTPBodyReadState.done) !== 0) {
            emitEOFIncomingMessage(this);
          }
          this.push(resumed);
        }
      }
    }
    function IncomingMessage(socket) {
      this[abortedSymbol] = false;
      this[eofInProgress] = false;
      this._consuming = false;
      this._dumped = false;
      this.complete = false;
      this._closed = false;
      this[kHeaders] = null;
      this[kReqShouldKeepAlive] = undefined;
      this[kRawHeaders] = null;
      this[kHeaderSource] = null;
      this[kHeadersCount] = 0;
      this[kTrailers] = null;
      this[kTrailersCount] = 0;
      this.rawTrailers = [];
      this[kAbortController] = null;
      if (socket === kHandle) {
        this[typeSymbol] = NodeHTTPIncomingRequestType.NodeHTTPResponse;
        this.url = arguments[1];
        this.method = arguments[2];
        this[kHeaderSource] = arguments[5];
        this[kHandle] = arguments[5];
        this[noBodySymbol] = !arguments[6];
        this[fakeSocketSymbol] = arguments[7];
        this.client = arguments[7];
        this.upgrade = null;
        Readable.call(this, arguments[7] ? { highWaterMark: arguments[7].readableHighWaterMark } : undefined);
        if (arguments[6]) {
          this.on("pause", onIncomingMessagePauseNodeHTTPResponse);
          this.on("resume", onIncomingMessageResumeNodeHTTPResponse);
        }
      } else {
        let streamOptions;
        if (socket) {
          streamOptions = {
            highWaterMark: socket.readableHighWaterMark
          };
        }
        Readable.call(this, streamOptions);
        this[fakeSocketSymbol] = socket;
        this.httpVersionMajor = null;
        this.httpVersionMinor = null;
        this.httpVersion = null;
        this.rawHeaders = [];
        this.joinDuplicateHeaders = false;
        this.upgrade = null;
        this.url = "";
        this.method = null;
        this.statusCode = null;
        this.statusMessage = null;
        this.client = socket;
        this[noBodySymbol] = false;
      }
      this._readableState.readingMore = true;
    }
    toClass(IncomingMessage, "IncomingMessage", Readable);
    IncomingMessage.prototype.httpVersion = "1.1";
    IncomingMessage.prototype.httpVersionMajor = 1;
    IncomingMessage.prototype.httpVersionMinor = 1;
    IncomingMessage.prototype.url = "";
    IncomingMessage.prototype.method = null;
    IncomingMessage.prototype.statusCode = null;
    IncomingMessage.prototype.statusMessage = null;
    IncomingMessage.prototype.upgrade = null;
    IncomingMessage.prototype.joinDuplicateHeaders = false;
    ObjectDefineProperty(IncomingMessage.prototype, "socket", {
      __proto__: null,
      get: function() {
        let socket = this[fakeSocketSymbol];
        if (socket === undefined && this[typeSymbol] === NodeHTTPIncomingRequestType.NodeHTTPResponse) {
          socket = this[fakeSocketSymbol] = new FakeSocket(this);
        }
        return socket;
      },
      set: function(val) {
        this[fakeSocketSymbol] = val;
      }
    });
    ObjectDefineProperty(IncomingMessage.prototype, "connection", {
      __proto__: null,
      get: function() {
        return this.socket;
      },
      set: function(val) {
        this.socket = val;
      }
    });
    ObjectDefineProperty(IncomingMessage.prototype, "aborted", {
      __proto__: null,
      get: function() {
        return this[abortedSymbol];
      },
      set: function(val) {
        this[abortedSymbol] = val;
      }
    });
    ObjectDefineProperty(IncomingMessage.prototype, "rawHeaders", {
      __proto__: null,
      enumerable: true,
      get: function() {
        let raw = this[kRawHeaders];
        if (raw === null) {
          const source = this[kHeaderSource];
          let built = source != null ? source.takeRawHeaders() : undefined;
          if (built === undefined)
            built = [];
          const maxHeadersCount = this[fakeSocketSymbol]?.server?.maxHeadersCount;
          if (typeof maxHeadersCount === "number" && maxHeadersCount > 0 && built.length > maxHeadersCount * 2) {
            built = ArrayPrototypeSlice.call(built, 0, maxHeadersCount * 2);
          }
          raw = this[kRawHeaders] = built;
          this[kHeadersCount] = built.length;
        }
        return raw;
      },
      set: function(value) {
        this[kRawHeaders] = value;
        this[kHeadersCount] = value ? value.length : 0;
      }
    });
    ObjectDefineProperty(IncomingMessage.prototype, "headers", {
      __proto__: null,
      get: function() {
        let dst = this[kHeaders];
        if (!dst) {
          dst = this[kHeaders] = {};
          const src = this.rawHeaders;
          const count = this[kHeadersCount];
          const addHeaderLine = this._addHeaderLine;
          for (let n = 0;n < count; n += 2) {
            addHeaderLine.call(this, src[n + 0], src[n + 1], dst);
          }
        }
        return dst;
      },
      set: function(val) {
        this[kHeaders] = val;
      }
    });
    ObjectDefineProperty(IncomingMessage.prototype, "headersDistinct", {
      __proto__: null,
      get: function() {
        if (!this[kHeadersDistinct]) {
          this[kHeadersDistinct] = { __proto__: null };
          const src = this.rawHeaders;
          const dst = this[kHeadersDistinct];
          for (let n = 0;n < this[kHeadersCount]; n += 2) {
            this._addHeaderLineDistinct(src[n + 0], src[n + 1], dst);
          }
        }
        return this[kHeadersDistinct];
      },
      set: function(val) {
        this[kHeadersDistinct] = val;
      }
    });
    ObjectDefineProperty(IncomingMessage.prototype, "trailers", {
      __proto__: null,
      get: function() {
        if (!this[kTrailers]) {
          this[kTrailers] = {};
          const src = this.rawTrailers;
          const dst = this[kTrailers];
          for (let n = 0;n < this[kTrailersCount]; n += 2) {
            this._addHeaderLine(src[n + 0], src[n + 1], dst);
          }
        }
        return this[kTrailers];
      },
      set: function(val) {
        this[kTrailers] = val;
      }
    });
    ObjectDefineProperty(IncomingMessage.prototype, "trailersDistinct", {
      __proto__: null,
      get: function() {
        if (!this[kTrailersDistinct]) {
          this[kTrailersDistinct] = { __proto__: null };
          const src = this.rawTrailers;
          const dst = this[kTrailersDistinct];
          for (let n = 0;n < this[kTrailersCount]; n += 2) {
            this._addHeaderLineDistinct(src[n + 0], src[n + 1], dst);
          }
        }
        return this[kTrailersDistinct];
      },
      set: function(val) {
        this[kTrailersDistinct] = val;
      }
    });
    ObjectDefineProperty(IncomingMessage.prototype, "signal", {
      __proto__: null,
      configurable: true,
      get: function() {
        if (this[kAbortController] === null) {
          const ac = new AbortController;
          this[kAbortController] = ac;
          if (this.destroyed) {
            ac.abort();
          } else {
            this.once("close", function() {
              ac.abort();
            });
          }
        }
        return this[kAbortController].signal;
      }
    });
    IncomingMessage.prototype.setTimeout = function setTimeout(msecs, callback) {
      if (callback)
        this.on("timeout", callback);
      this.socket?.setTimeout(msecs);
      return this;
    };
    IncomingMessage.prototype._read = function _read(_n) {
      if (!this._consuming) {
        this._readableState.readingMore = false;
        this._consuming = true;
      }
      const handle = this[kHandle];
      if (!handle) {
        const socket = this.socket;
        if (socket && socket.readable)
          readStart(socket);
        return;
      }
      const socket = this.socket;
      if (socket && socket.readable) {
        if (this.upgrade) {
          onIncomingMessageResumeNodeHTTPResponse.call(this);
        } else {
          socket.resume();
        }
      }
      if (this[eofInProgress]) {
        return;
      }
      if (this[noBodySymbol]) {
        emitEOFIncomingMessage(this);
        return;
      }
      const bodyReadState = handle.hasBody;
      if ((bodyReadState & NodeHTTPBodyReadState.done) !== 0 || bodyReadState === NodeHTTPBodyReadState.none || this._dumped) {
        emitEOFIncomingMessage(this);
      }
      if ((bodyReadState & NodeHTTPBodyReadState.hasBufferedDataDuringPause) !== 0) {
        const drained = handle.drainRequestBody();
        if (drained && !this._dumped) {
          this.push(drained);
        }
      }
      if (!handle.ondata) {
        handle.ondata = onDataIncomingMessage.bind(this);
        handle.hasCustomOnData = false;
      }
    };
    IncomingMessage.prototype._destroy = function _destroy(err, cb) {
      const shouldEmitAborted = !this.readableEnded || !this.complete;
      if (shouldEmitAborted) {
        this[abortedSymbol] = true;
        this.emit("aborted");
      }
      const handle = this[kHandle];
      if (handle) {
        this[kHandle] = undefined;
        handle.onabort = handle.ondata = undefined;
        if (!handle.finished && shouldEmitAborted && this.socket) {
          handle.abort();
        }
        const socket = this.socket;
        if (socket && !socket.destroyed && shouldEmitAborted) {
          socket.destroy(err);
        }
        if ((typeof cb === "function")) {
          emitErrorNextTickIfErrorListenerNT(this, err, cb);
        }
        return;
      }
      const socket = this.socket;
      if (socket && !socket.destroyed && this.aborted) {
        socket.destroy(err);
        const state = { cleanup: undefined };
        state.cleanup = finished(socket, onFinishedAfterDestroy.bind(this, state, err, cb));
      } else {
        process.nextTick(onError, this, err, cb);
      }
    };
    function onFinishedAfterDestroy(state, err, cb, e) {
      if (e?.code === "ERR_STREAM_PREMATURE_CLOSE") {
        e = null;
      }
      state.cleanup();
      process.nextTick(onError, this, e || err, cb);
    }
    IncomingMessage.prototype._addHeaderLines = _addHeaderLines;
    function _addHeaderLines(headers, n) {
      if (headers?.length) {
        let dest;
        if (this.complete) {
          this.rawTrailers = headers;
          this[kTrailersCount] = n;
          dest = this[kTrailers];
        } else {
          this.rawHeaders = headers;
          this[kHeadersCount] = n;
          dest = this[kHeaders];
        }
        if (dest) {
          for (let i = 0;i < n; i += 2) {
            this._addHeaderLine(headers[i], headers[i + 1], dest);
          }
        }
      }
    }
    const kFieldList = 0;
    const kFieldSetCookie = 1;
    const kFieldCookie = 2;
    const kFieldUnique = 3;
    let matchedFieldFlag = kFieldUnique;
    function matchKnownFields(field, lowercased) {
      switch (field.length) {
        case 3:
          if (field === "Age" || field === "age") {
            matchedFieldFlag = kFieldUnique;
            return "age";
          }
          break;
        case 4:
          if (field === "Host" || field === "host") {
            matchedFieldFlag = kFieldUnique;
            return "host";
          }
          if (field === "From" || field === "from") {
            matchedFieldFlag = kFieldUnique;
            return "from";
          }
          if (field === "ETag" || field === "etag") {
            matchedFieldFlag = kFieldUnique;
            return "etag";
          }
          if (field === "Date" || field === "date") {
            matchedFieldFlag = kFieldList;
            return "date";
          }
          if (field === "Vary" || field === "vary") {
            matchedFieldFlag = kFieldList;
            return "vary";
          }
          break;
        case 6:
          if (field === "Server" || field === "server") {
            matchedFieldFlag = kFieldUnique;
            return "server";
          }
          if (field === "Cookie" || field === "cookie") {
            matchedFieldFlag = kFieldCookie;
            return "cookie";
          }
          if (field === "Origin" || field === "origin") {
            matchedFieldFlag = kFieldList;
            return "origin";
          }
          if (field === "Expect" || field === "expect") {
            matchedFieldFlag = kFieldList;
            return "expect";
          }
          if (field === "Accept" || field === "accept") {
            matchedFieldFlag = kFieldList;
            return "accept";
          }
          break;
        case 7:
          if (field === "Referer" || field === "referer") {
            matchedFieldFlag = kFieldUnique;
            return "referer";
          }
          if (field === "Expires" || field === "expires") {
            matchedFieldFlag = kFieldUnique;
            return "expires";
          }
          if (field === "Upgrade" || field === "upgrade") {
            matchedFieldFlag = kFieldList;
            return "upgrade";
          }
          break;
        case 8:
          if (field === "Location" || field === "location") {
            matchedFieldFlag = kFieldUnique;
            return "location";
          }
          if (field === "If-Match" || field === "if-match") {
            matchedFieldFlag = kFieldList;
            return "if-match";
          }
          break;
        case 10:
          if (field === "User-Agent" || field === "user-agent") {
            matchedFieldFlag = kFieldUnique;
            return "user-agent";
          }
          if (field === "Set-Cookie" || field === "set-cookie") {
            matchedFieldFlag = kFieldSetCookie;
            return "set-cookie";
          }
          if (field === "Connection" || field === "connection") {
            matchedFieldFlag = kFieldList;
            return "connection";
          }
          break;
        case 11:
          if (field === "Retry-After" || field === "retry-after") {
            matchedFieldFlag = kFieldUnique;
            return "retry-after";
          }
          break;
        case 12:
          if (field === "Content-Type" || field === "content-type") {
            matchedFieldFlag = kFieldUnique;
            return "content-type";
          }
          if (field === "Max-Forwards" || field === "max-forwards") {
            matchedFieldFlag = kFieldUnique;
            return "max-forwards";
          }
          break;
        case 13:
          if (field === "Authorization" || field === "authorization") {
            matchedFieldFlag = kFieldUnique;
            return "authorization";
          }
          if (field === "Last-Modified" || field === "last-modified") {
            matchedFieldFlag = kFieldUnique;
            return "last-modified";
          }
          if (field === "Cache-Control" || field === "cache-control") {
            matchedFieldFlag = kFieldList;
            return "cache-control";
          }
          if (field === "If-None-Match" || field === "if-none-match") {
            matchedFieldFlag = kFieldList;
            return "if-none-match";
          }
          break;
        case 14:
          if (field === "Content-Length" || field === "content-length") {
            matchedFieldFlag = kFieldUnique;
            return "content-length";
          }
          break;
        case 15:
          if (field === "Accept-Encoding" || field === "accept-encoding") {
            matchedFieldFlag = kFieldList;
            return "accept-encoding";
          }
          if (field === "Accept-Language" || field === "accept-language") {
            matchedFieldFlag = kFieldList;
            return "accept-language";
          }
          if (field === "X-Forwarded-For" || field === "x-forwarded-for") {
            matchedFieldFlag = kFieldList;
            return "x-forwarded-for";
          }
          break;
        case 16:
          if (field === "Content-Encoding" || field === "content-encoding") {
            matchedFieldFlag = kFieldList;
            return "content-encoding";
          }
          if (field === "X-Forwarded-Host" || field === "x-forwarded-host") {
            matchedFieldFlag = kFieldList;
            return "x-forwarded-host";
          }
          break;
        case 17:
          if (field === "If-Modified-Since" || field === "if-modified-since") {
            matchedFieldFlag = kFieldUnique;
            return "if-modified-since";
          }
          if (field === "Transfer-Encoding" || field === "transfer-encoding") {
            matchedFieldFlag = kFieldList;
            return "transfer-encoding";
          }
          if (field === "X-Forwarded-Proto" || field === "x-forwarded-proto") {
            matchedFieldFlag = kFieldList;
            return "x-forwarded-proto";
          }
          break;
        case 19:
          if (field === "Proxy-Authorization" || field === "proxy-authorization") {
            matchedFieldFlag = kFieldUnique;
            return "proxy-authorization";
          }
          if (field === "If-Unmodified-Since" || field === "if-unmodified-since") {
            matchedFieldFlag = kFieldUnique;
            return "if-unmodified-since";
          }
          break;
      }
      if (lowercased) {
        matchedFieldFlag = kFieldList;
        return field;
      }
      return matchKnownFields(field.toLowerCase(), true);
    }
    IncomingMessage.prototype._addHeaderLine = _addHeaderLine;
    function _addHeaderLine(field, value, dest) {
      field = matchKnownFields(field);
      const flag = matchedFieldFlag;
      if (flag === kFieldList || flag === kFieldCookie) {
        if (typeof dest[field] === "string") {
          dest[field] += (flag === kFieldList ? ", " : "; ") + value;
        } else {
          dest[field] = value;
        }
      } else if (flag === kFieldSetCookie) {
        if (dest["set-cookie"] !== undefined) {
          dest["set-cookie"].push(value);
        } else {
          dest["set-cookie"] = [value];
        }
      } else if (this.joinDuplicateHeaders) {
        if (dest[field] === undefined) {
          dest[field] = value;
        } else {
          dest[field] += ", " + value;
        }
      } else if (dest[field] === undefined) {
        dest[field] = value;
      }
    }
    IncomingMessage.prototype._addHeaderLineDistinct = _addHeaderLineDistinct;
    function _addHeaderLineDistinct(field, value, dest) {
      field = field.toLowerCase();
      if (!dest[field]) {
        dest[field] = [value];
      } else {
        dest[field].push(value);
      }
    }
    IncomingMessage.prototype._dumpAndCloseReadable = function _dumpAndCloseReadable() {
      this._dumped = true;
      this._readableState.ended = true;
      this._readableState.endEmitted = true;
      this._readableState.destroyed = true;
      this._readableState.closed = true;
      this._readableState.closeEmitted = true;
    };
    IncomingMessage.prototype._dump = function _dump() {
      if (!this._dumped) {
        this._dumped = true;
        this.removeAllListeners("data");
        const handle = this[kHandle];
        if (handle) {
          handle.ondata = undefined;
        }
        this.resume();
      }
    };
    function onError(self, error, cb) {
      if (self.listenerCount("error") === 0) {
        cb();
      } else {
        cb(error);
      }
    }

    return { IncomingMessage, kReqShouldKeepAlive, readStart, readStop };
  })();
})
