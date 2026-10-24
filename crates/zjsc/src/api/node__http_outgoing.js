// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/_http_outgoing.ts` do bun 1.4.2. Não editar à mão.
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
    const { Stream } = require("internal/stream");
    const { isUint8Array, validateString } = require("internal/validators");
    const { deprecate } = require("internal/util/deprecate");
    const { getDefaultHighWaterMark } = require("internal/streams/state");
    const { kOutHeaders, kNeedDrain, utcDate } = require("internal/http");
    const {
      validateHeaderName,
      validateHeaderValue,
      _checkInvalidHeaderChar: checkInvalidHeaderChar,
      chunkExpression: RE_TE_CHUNKED,
      isLenient
    } = require("node:_http_common");
    const { FakeSocket } = require("internal/http/FakeSocket");
    const EE = require("node:events");
    const ObjectDefineProperty = Object.defineProperty;
    const ObjectKeys = Object.keys;
    const ObjectValues = Object.values;
    const ObjectHasOwn = Object.hasOwn;
    const ArrayIsArray = Array.isArray;
    const MathFloor = Math.floor;
    const commonLowercasedHeaders = { __proto__: null };
    for (const name of [
      "accept",
      "accept-encoding",
      "accept-language",
      "accept-ranges",
      "access-control-allow-origin",
      "age",
      "cache-control",
      "connection",
      "content-disposition",
      "content-encoding",
      "content-language",
      "content-length",
      "content-range",
      "content-type",
      "cookie",
      "date",
      "etag",
      "expires",
      "host",
      "last-modified",
      "location",
      "pragma",
      "server",
      "set-cookie",
      "transfer-encoding",
      "user-agent",
      "vary",
      "x-content-type-options",
      "x-forwarded-for",
      "x-powered-by",
      "x-requested-with"
    ]) {
      commonLowercasedHeaders[name] = name;
      let titled = "";
      for (const part of name.split("-"))
        titled += (titled ? "-" : "") + part.charAt(0).toUpperCase() + part.slice(1);
      commonLowercasedHeaders[titled] = name;
    }
    commonLowercasedHeaders["ETag"] = "etag";
    commonLowercasedHeaders["etag"] = "etag";
    function lowercaseHeaderName(name) {
      const known = commonLowercasedHeaders[name];
      return known !== undefined ? known : name.toLowerCase();
    }
    const kCorked = Symbol("corked");
    const kSocket = Symbol("kSocket");
    const kChunkedBuffer = Symbol("kChunkedBuffer");
    const kChunkedLength = Symbol("kChunkedLength");
    const kUniqueHeaders = Symbol("kUniqueHeaders");
    const kBytesWritten = Symbol("kBytesWritten");
    const kErrored = Symbol("errored");
    const kHighWaterMark = Symbol("kHighWaterMark");
    const kRejectNonStandardBodyWrites = Symbol("kRejectNonStandardBodyWrites");
    const kLenientValidation = Symbol("kLenientValidation");
    const nop = () => {};
    const RE_CONN_CLOSE = /(?:^|\W)close(?:$|\W)/i;
    function isCookieField(s) {
      return s.length === 6 && s.toLowerCase() === "cookie";
    }
    function isContentDispositionField(s) {
      return s.length === 19 && s.toLowerCase() === "content-disposition";
    }
    function OutgoingMessage(options) {
      Stream.call(this, options);
      this.outputData = [];
      this.outputSize = 0;
      this.writable = true;
      this.destroyed = false;
      this._last = false;
      this.chunkedEncoding = false;
      this.shouldKeepAlive = true;
      this.maxRequestsOnConnectionReached = false;
      this._defaultKeepAlive = true;
      this.useChunkedEncodingByDefault = true;
      this.sendDate = false;
      this._removedConnection = false;
      this._removedContLen = false;
      this._removedTE = false;
      this.strictContentLength = false;
      this[kBytesWritten] = 0;
      this._contentLength = null;
      this._hasBody = true;
      this._trailer = "";
      this[kNeedDrain] = false;
      this.finished = false;
      this._headerSent = false;
      this[kCorked] = 0;
      this[kChunkedBuffer] = [];
      this[kChunkedLength] = 0;
      this._closed = false;
      this[kSocket] = null;
      this._header = null;
      this[kOutHeaders] = null;
      this._keepAliveTimeout = 0;
      this._onPendingData = nop;
      this[kErrored] = null;
      this[kHighWaterMark] = options?.highWaterMark ?? getDefaultHighWaterMark();
      this[kRejectNonStandardBodyWrites] = options?.rejectNonStandardBodyWrites ?? false;
      this[kLenientValidation] = undefined;
    }
    toClass(OutgoingMessage, "OutgoingMessage", Stream);
    OutgoingMessage.prototype._isLenientHeaderValidation = function() {
      const cached = this[kLenientValidation];
      if (cached !== undefined)
        return cached;
      let result;
      const httpValidation = this.httpValidation;
      if (httpValidation !== undefined) {
        result = httpValidation !== "strict";
      } else {
        const server = this.req?.socket?.server;
        const serverHttpValidation = server?.httpValidation;
        if (serverHttpValidation !== undefined) {
          result = serverHttpValidation !== "strict";
        } else {
          const insecureHTTPParser = this.insecureHTTPParser;
          if (typeof insecureHTTPParser === "boolean") {
            result = insecureHTTPParser;
          } else {
            const serverOption = server?.insecureHTTPParser;
            result = typeof serverOption === "boolean" ? serverOption : isLenient();
          }
        }
      }
      return this[kLenientValidation] = result;
    };
    ObjectDefineProperty(OutgoingMessage.prototype, "errored", {
      __proto__: null,
      get() {
        return this[kErrored];
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "closed", {
      __proto__: null,
      get() {
        return this._closed;
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "writableFinished", {
      __proto__: null,
      get() {
        return this.finished && this.outputSize === 0 && (!this[kSocket] || this[kSocket].writableLength === 0);
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "writableObjectMode", {
      __proto__: null,
      get() {
        return false;
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "writableLength", {
      __proto__: null,
      get() {
        return this.outputSize + this[kChunkedLength] + (this[kSocket] ? this[kSocket].writableLength : 0);
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "writableHighWaterMark", {
      __proto__: null,
      get() {
        return this[kSocket] ? this[kSocket].writableHighWaterMark : this[kHighWaterMark];
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "writableCorked", {
      __proto__: null,
      get() {
        return this[kCorked];
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "connection", {
      __proto__: null,
      get: function() {
        return this[kSocket];
      },
      set: function(val) {
        this.socket = val;
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "socket", {
      __proto__: null,
      get: function() {
        return this[kSocket];
      },
      set: function(val) {
        for (let n = 0;n < this[kCorked]; n++) {
          val?.cork();
          this[kSocket]?.uncork();
        }
        this[kSocket] = val;
      }
    });
    OutgoingMessage.prototype._renderHeaders = function _renderHeaders() {
      if (this._header) {
        throw codes.HTTP_HEADERS_SENT("render");
      }
      const headersMap = this[kOutHeaders];
      const headers = {};
      if (headersMap !== null) {
        const keys = ObjectKeys(headersMap);
        for (let i = 0, l = keys.length;i < l; i++) {
          const key = keys[i];
          headers[headersMap[key][0]] = headersMap[key][1];
        }
      }
      return headers;
    };
    OutgoingMessage.prototype.cork = function cork() {
      this[kCorked]++;
      if (this[kSocket]) {
        this[kSocket].cork();
      }
    };
    OutgoingMessage.prototype.uncork = function uncork() {
      this[kCorked]--;
      if (this[kSocket]) {
        this[kSocket].uncork();
      }
      if (this[kCorked] || this[kChunkedBuffer].length === 0) {
        return;
      }
      const len = this[kChunkedLength];
      const buf = this[kChunkedBuffer];
      assert(this.chunkedEncoding);
      let callbacks;
      this._send(len.toString(16), "latin1", null);
      this._send(crlf_buf, null, null);
      for (let n = 0;n < buf.length; n += 3) {
        this._send(buf[n + 0], buf[n + 1], null);
        if (buf[n + 2]) {
          callbacks ??= [];
          callbacks.push(buf[n + 2]);
        }
      }
      this._send(crlf_buf, null, callbacks?.length ? runChunkCallbacks.bind(undefined, callbacks) : null);
      this[kChunkedBuffer].length = 0;
      this[kChunkedLength] = 0;
    };
    function runChunkCallbacks(callbacks, err) {
      for (const callback of callbacks) {
        callback(err);
      }
    }
    OutgoingMessage.prototype.setTimeout = function setTimeout(msecs, callback) {
      if (callback) {
        this.on("timeout", callback);
      }
      if (!this[kSocket]) {
        this.once("socket", function socketSetTimeoutOnConnect(socket) {
          socket.setTimeout(msecs);
        });
      } else {
        this[kSocket].setTimeout(msecs);
      }
      return this;
    };
    OutgoingMessage.prototype.destroy = function destroy(error) {
      if (this.destroyed) {
        return this;
      }
      this.destroyed = true;
      this[kErrored] = error;
      if (this[kSocket]) {
        this[kSocket].destroy(error);
      } else {
        process.nextTick(emitDestroyNT, this);
      }
      return this;
    };
    function emitDestroyNT(self) {
      if (!self._closed) {
        self._closed = true;
        self.emit("close");
      }
    }
    OutgoingMessage.prototype._send = function _send(data, encoding, callback, byteLength) {
      let header;
      if (!this._headerSent && (header = this._header) !== null) {
        if (typeof data === "string" && (encoding === "utf8" || encoding === "latin1" || !encoding)) {
          data = header + data;
        } else {
          this.outputData.unshift({
            data: header,
            encoding: "latin1",
            callback: null
          });
          this.outputSize += header.length;
          this._onPendingData(header.length);
        }
        this._headerSent = true;
      }
      return this._writeRaw(data, encoding, callback, byteLength);
    };
    OutgoingMessage.prototype._writeRaw = _writeRaw;
    function _writeRaw(data, encoding, callback, _size) {
      const conn = this[kSocket];
      if (conn?.destroyed) {
        return false;
      }
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = null;
      }
      if (conn && conn._httpMessage === this && conn.writable) {
        if (this.outputData.length) {
          this._flushOutput(conn);
        }
        return conn.write(data, encoding, callback);
      }
      this.outputData.push({ data, encoding, callback });
      this.outputSize += data.length;
      this._onPendingData(data.length);
      return this.outputSize < this[kHighWaterMark];
    }
    OutgoingMessage.prototype._storeHeader = _storeHeader;
    function _storeHeader(firstLine, headers) {
      const state = {
        connection: false,
        contLen: false,
        te: false,
        date: false,
        expect: false,
        trailer: false,
        header: firstLine
      };
      const lenient = this._isLenientHeaderValidation();
      if (headers) {
        if (headers === this[kOutHeaders]) {
          for (const key in headers) {
            const entry = headers[key];
            processHeader(this, state, entry[0], entry[1], false, lenient);
          }
        } else if (ArrayIsArray(headers)) {
          const headersLength = headers.length;
          if (headersLength && ArrayIsArray(headers[0])) {
            for (let i = 0;i < headersLength; i++) {
              const entry = headers[i];
              processHeader(this, state, entry[0], entry[1], true, lenient);
            }
          } else {
            if (headersLength % 2 !== 0) {
              throw codes.INVALID_ARG_VALUE("headers", headers);
            }
            for (let n = 0;n < headersLength; n += 2) {
              processHeader(this, state, headers[n + 0], headers[n + 1], true, lenient);
            }
          }
        } else {
          for (const key in headers) {
            if (ObjectHasOwn(headers, key)) {
              processHeader(this, state, key, headers[key], true, lenient);
            }
          }
        }
      }
      let { header } = state;
      if (this.sendDate && !state.date) {
        header += "Date: " + utcDate() + `\r
    `;
      }
      if (this.chunkedEncoding && (this.statusCode === 204 || this.statusCode === 304)) {
        this.chunkedEncoding = false;
        this.shouldKeepAlive = false;
      }
      if (this._removedConnection) {
        this._last = !this.shouldKeepAlive;
      } else if (!state.connection) {
        const shouldSendKeepAlive = this.shouldKeepAlive && (state.contLen || this.useChunkedEncodingByDefault || this.agent);
        if (shouldSendKeepAlive && this.maxRequestsOnConnectionReached) {
          header += `Connection: close\r
    `;
        } else if (shouldSendKeepAlive) {
          header += `Connection: keep-alive\r
    `;
          const keepAliveTimeout = this._keepAliveTimeout;
          if (keepAliveTimeout && this._defaultKeepAlive) {
            const timeoutSeconds = MathFloor(keepAliveTimeout / 1000);
            let max = "";
            const maxRequestsPerSocket = this._maxRequestsPerSocket;
            if (~~maxRequestsPerSocket > 0) {
              max = `, max=${maxRequestsPerSocket}`;
            }
            header += `Keep-Alive: timeout=${timeoutSeconds}${max}\r
    `;
          }
        } else {
          this._last = true;
          header += `Connection: close\r
    `;
        }
      }
      if (!state.contLen && !state.te) {
        if (!this._hasBody) {
          this.chunkedEncoding = false;
        } else if (!this.useChunkedEncodingByDefault) {
          this._last = true;
        } else {
          let contentLength;
          if (!state.trailer && !this._removedContLen && typeof (contentLength = this._contentLength) === "number") {
            header += "Content-Length: " + contentLength + `\r
    `;
          } else if (!this._removedTE) {
            header += `Transfer-Encoding: chunked\r
    `;
            this.chunkedEncoding = true;
          } else {
            this._last = true;
          }
        }
      }
      if (this.chunkedEncoding !== true && state.trailer) {
        throw codes.HTTP_TRAILER_INVALID();
      }
      this._header = header + `\r
    `;
      this._headerSent = false;
      if (state.expect)
        this._send("");
    }
    function processHeader(self, state, key, value, validate, lenient) {
      if (validate)
        validateHeaderName(key);
      if (isContentDispositionField(key) && self._contentLength) {
        if (ArrayIsArray(value)) {
          for (let i = 0;i < value.length; i++) {
            value[i] = Buffer.from(value[i], "latin1");
          }
        } else {
          value = Buffer.from(value, "latin1");
        }
      }
      if (ArrayIsArray(value)) {
        const valueLength = value.length;
        if ((valueLength < 2 || !isCookieField(key)) && (!self[kUniqueHeaders] || !self[kUniqueHeaders].has(key.toLowerCase()))) {
          for (let i = 0;i < valueLength; i++)
            storeHeader(self, state, key, value[i], validate, lenient);
          return;
        }
        value = value.join("; ");
      }
      storeHeader(self, state, key, value, validate, lenient);
    }
    function validateHeaderValueLenient(name, value, lenient) {
      if (value === undefined) {
        throw codes.HTTP_INVALID_HEADER_VALUE(value, name);
      }
      if (checkInvalidHeaderChar(value, lenient)) {
        throw codes.INVALID_CHAR("header content", name);
      }
    }
    function storeHeader(self, state, key, value, validate, lenient) {
      if (validate)
        validateHeaderValueLenient(key, value, lenient);
      state.header += key + ": " + value + `\r
    `;
      matchHeader(self, state, key, value);
    }
    function matchHeader(self, state, field, value) {
      if (field.length < 4 || field.length > 17)
        return;
      field = field.toLowerCase();
      switch (field) {
        case "connection":
          state.connection = true;
          self._removedConnection = false;
          if (RE_CONN_CLOSE.test(value))
            self._last = true;
          else
            self.shouldKeepAlive = true;
          break;
        case "transfer-encoding":
          state.te = true;
          self._removedTE = false;
          if (RE_TE_CHUNKED.test(value))
            self.chunkedEncoding = true;
          break;
        case "content-length":
          state.contLen = true;
          self._contentLength = +value;
          self._removedContLen = false;
          break;
        case "date":
        case "expect":
        case "trailer":
          state[field] = true;
          break;
        case "keep-alive":
          self._defaultKeepAlive = false;
          break;
      }
    }
    function parseUniqueHeadersOption(headers) {
      if (!ArrayIsArray(headers)) {
        return null;
      }
      const unique = new Set;
      const l = headers.length;
      for (let i = 0;i < l; i++) {
        unique.add(headers[i].toLowerCase());
      }
      return unique;
    }
    OutgoingMessage.prototype.setHeader = function setHeader(name, value) {
      if (this._header) {
        throw codes.HTTP_HEADERS_SENT("set");
      }
      validateHeaderName(name);
      validateHeaderValueLenient(name, value, this._isLenientHeaderValidation());
      let headers = this[kOutHeaders];
      if (headers === null)
        this[kOutHeaders] = headers = { __proto__: null };
      headers[lowercaseHeaderName(name)] = [name, value];
      return this;
    };
    OutgoingMessage.prototype.setHeaders = function setHeaders(headers) {
      if (this._header) {
        throw codes.HTTP_HEADERS_SENT("set");
      }
      if (!headers || ArrayIsArray(headers) || typeof headers.keys !== "function" || typeof headers.get !== "function") {
        throw codes.INVALID_ARG_TYPE("headers", ["Headers", "Map"], headers);
      }
      let cookies = null;
      for (const { 0: key, 1: value } of headers) {
        if (key === "set-cookie") {
          if (ArrayIsArray(value)) {
            cookies ??= [];
            cookies.push(...value);
          } else {
            cookies ??= [];
            cookies.push(value);
          }
          continue;
        }
        this.setHeader(key, value);
      }
      if (cookies != null) {
        this.setHeader("set-cookie", cookies);
      }
      return this;
    };
    OutgoingMessage.prototype.appendHeader = function appendHeader(name, value) {
      if (this._header) {
        throw codes.HTTP_HEADERS_SENT("append");
      }
      validateHeaderName(name);
      validateHeaderValueLenient(name, value, this._isLenientHeaderValidation());
      const field = lowercaseHeaderName(name);
      const headers = this[kOutHeaders];
      if (headers === null || !headers[field]) {
        return this.setHeader(name, value);
      }
      if (!ArrayIsArray(headers[field][1])) {
        headers[field][1] = [headers[field][1]];
      }
      const existingValues = headers[field][1];
      if (ArrayIsArray(value)) {
        for (let i = 0, length = value.length;i < length; i++) {
          existingValues.push(value[i]);
        }
      } else {
        existingValues.push(value);
      }
      return this;
    };
    OutgoingMessage.prototype.getHeader = function getHeader(name) {
      validateString(name, "name");
      const headers = this[kOutHeaders];
      if (headers === null)
        return;
      const entry = headers[lowercaseHeaderName(name)];
      return entry?.[1];
    };
    OutgoingMessage.prototype.getHeaderNames = function getHeaderNames() {
      return this[kOutHeaders] !== null ? ObjectKeys(this[kOutHeaders]) : [];
    };
    OutgoingMessage.prototype.getRawHeaderNames = function getRawHeaderNames() {
      const headersMap = this[kOutHeaders];
      if (headersMap === null)
        return [];
      const values = ObjectValues(headersMap);
      const headers = Array(values.length);
      for (let i = 0, l = values.length;i < l; i++) {
        headers[i] = values[i][0];
      }
      return headers;
    };
    OutgoingMessage.prototype.getHeaders = function getHeaders() {
      const headers = this[kOutHeaders];
      const ret = { __proto__: null };
      if (headers) {
        const keys = ObjectKeys(headers);
        for (let i = 0;i < keys.length; ++i) {
          const key = keys[i];
          const val = headers[key][1];
          ret[key] = val;
        }
      }
      return ret;
    };
    OutgoingMessage.prototype.hasHeader = function hasHeader(name) {
      validateString(name, "name");
      return this[kOutHeaders] !== null && !!this[kOutHeaders][lowercaseHeaderName(name)];
    };
    OutgoingMessage.prototype.removeHeader = function removeHeader(name) {
      validateString(name, "name");
      if (this._header) {
        throw codes.HTTP_HEADERS_SENT("remove");
      }
      const key = lowercaseHeaderName(name);
      switch (key) {
        case "connection":
          this._removedConnection = true;
          break;
        case "content-length":
          this._removedContLen = true;
          break;
        case "transfer-encoding":
          this._removedTE = true;
          break;
        case "date":
          this.sendDate = false;
          break;
      }
      if (this[kOutHeaders] !== null) {
        delete this[kOutHeaders][key];
      }
    };
    OutgoingMessage.prototype._implicitHeader = function _implicitHeader() {
      throw codes.METHOD_NOT_IMPLEMENTED("_implicitHeader()");
    };
    ObjectDefineProperty(OutgoingMessage.prototype, "headersSent", {
      __proto__: null,
      configurable: true,
      enumerable: true,
      get: function() {
        return !!this._header;
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "writableEnded", {
      __proto__: null,
      get: function() {
        return this.finished;
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "writableNeedDrain", {
      __proto__: null,
      get: function() {
        return !this.destroyed && !this.finished && this[kNeedDrain];
      }
    });
    const crlf_buf = Buffer.from(`\r
    `);
    OutgoingMessage.prototype.write = function write(chunk, encoding, callback) {
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = null;
      }
      const ret = write_(this, chunk, encoding, callback, false);
      if (!ret)
        this[kNeedDrain] = true;
      return ret;
    };
    function onError(msg, err, callback) {
      if (msg.destroyed) {
        return;
      }
      process.nextTick(emitErrorNt, msg, err, callback);
    }
    function emitErrorNt(msg, err, callback) {
      callback(err);
      if (typeof msg.emit === "function" && !msg.destroyed) {
        msg.emit("error", err);
      }
    }
    function strictContentLength(msg) {
      return msg.strictContentLength && msg._contentLength != null && msg._hasBody && !msg._removedContLen && !msg.chunkedEncoding && !msg.hasHeader("transfer-encoding");
    }
    function write_(msg, chunk, encoding, callback, fromEnd) {
      if (typeof callback !== "function")
        callback = nop;
      if (chunk === null) {
        throw codes.STREAM_NULL_VALUES();
      } else if (typeof chunk !== "string" && !isUint8Array(chunk)) {
        throw codes.INVALID_ARG_TYPE("chunk", ["string", "Buffer", "Uint8Array"], chunk);
      }
      let err;
      if (msg.finished) {
        err = codes.STREAM_WRITE_AFTER_END();
      } else if (msg.destroyed) {
        err = codes.STREAM_DESTROYED("write");
      }
      if (err) {
        if (!msg.destroyed) {
          onError(msg, err, callback);
        } else {
          process.nextTick(callback, err);
        }
        return false;
      }
      let len;
      if (msg.strictContentLength) {
        len ??= typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.byteLength;
        const contentLength = msg._contentLength;
        if (strictContentLength(msg) && (fromEnd ? msg[kBytesWritten] + len !== contentLength : msg[kBytesWritten] + len > contentLength)) {
          throw codes.HTTP_CONTENT_LENGTH_MISMATCH(len + msg[kBytesWritten], contentLength);
        }
        msg[kBytesWritten] += len;
      }
      if (!msg._header) {
        if (fromEnd) {
          len ??= typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.byteLength;
          msg._contentLength = len;
        }
        msg._implicitHeader();
      }
      if (!msg._hasBody) {
        if (msg[kRejectNonStandardBodyWrites]) {
          throw codes.HTTP_BODY_NOT_ALLOWED();
        } else {
          process.nextTick(callback);
          return true;
        }
      }
      let msgSocket;
      if (!fromEnd && (msgSocket = msg.socket) && !msgSocket.writableCorked) {
        msgSocket.cork();
        process.nextTick(connectionCorkNT, msgSocket);
      }
      let ret;
      if (msg.chunkedEncoding && chunk.length !== 0) {
        len ??= typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.byteLength;
        if (msg[kCorked] && msg._headerSent) {
          msg[kChunkedBuffer].push(chunk, encoding, callback);
          msg[kChunkedLength] += len;
          ret = msg[kChunkedLength] < msg[kHighWaterMark];
        } else {
          msg._send(len.toString(16), "latin1", null);
          msg._send(crlf_buf, null, null);
          msg._send(chunk, encoding, null, len);
          ret = msg._send(crlf_buf, null, callback);
        }
      } else {
        ret = msg._send(chunk, encoding, callback, len);
      }
      return ret;
    }
    function connectionCorkNT(conn) {
      conn.uncork();
    }
    OutgoingMessage.prototype.addTrailers = function addTrailers(headers) {
      this._trailer = "";
      const keys = ObjectKeys(headers);
      const isArray = ArrayIsArray(headers);
      const lenient = this._isLenientHeaderValidation();
      for (let i = 0, l = keys.length;i < l; i++) {
        let field, value;
        const key = keys[i];
        if (isArray) {
          field = headers[key][0];
          value = headers[key][1];
        } else {
          field = key;
          value = headers[key];
        }
        validateHeaderName(field, "Trailer name");
        const isArrayValue = ArrayIsArray(value);
        const valueLength = isArrayValue ? value.length : 0;
        if (isArrayValue && valueLength > 1 && (!this[kUniqueHeaders] || !this[kUniqueHeaders].has(field.toLowerCase()))) {
          for (let j = 0, l = valueLength;j < l; j++) {
            if (checkInvalidHeaderChar(value[j], lenient)) {
              throw codes.INVALID_CHAR("trailer content", field);
            }
            this._trailer += field + ": " + value[j] + `\r
    `;
          }
        } else {
          if (isArrayValue) {
            value = value.join("; ");
          }
          if (checkInvalidHeaderChar(value, lenient)) {
            throw codes.INVALID_CHAR("trailer content", field);
          }
          this._trailer += field + ": " + value + `\r
    `;
        }
      }
    };
    function onFinish(outmsg) {
      if (outmsg?.socket?._hadError)
        return;
      outmsg.emit("finish");
    }
    OutgoingMessage.prototype.end = function end(chunk, encoding, callback) {
      if (typeof chunk === "function") {
        callback = chunk;
        chunk = null;
        encoding = null;
      } else if (typeof encoding === "function") {
        callback = encoding;
        encoding = null;
      }
      if (chunk) {
        if (this.finished) {
          onError(this, codes.STREAM_WRITE_AFTER_END(), typeof callback !== "function" ? nop : callback);
          return this;
        }
        if (this[kSocket]) {
          this[kSocket].cork();
        }
        write_(this, chunk, encoding, null, true);
      } else if (this.finished) {
        if (typeof callback === "function") {
          if (!this.writableFinished) {
            this.on("finish", callback);
          } else {
            callback(codes.STREAM_ALREADY_FINISHED("end"));
          }
        }
        return this;
      } else if (!this._header) {
        if (this[kSocket]) {
          this[kSocket].cork();
        }
        this._contentLength = 0;
        this._implicitHeader();
      }
      if (typeof callback === "function")
        this.once("finish", callback);
      let contentLength;
      if (strictContentLength(this) && this[kBytesWritten] !== (contentLength = this._contentLength)) {
        throw codes.HTTP_CONTENT_LENGTH_MISMATCH(this[kBytesWritten], contentLength);
      }
      const finish = onFinish.bind(undefined, this);
      if (this._hasBody && this.chunkedEncoding) {
        this._send(`0\r
    ` + this._trailer + `\r
    `, "latin1", finish);
      } else if (!this._headerSent || this.writableLength || chunk) {
        this._send("", "latin1", finish);
      } else {
        process.nextTick(finish);
      }
      if (this[kSocket]) {
        this[kSocket]._writableState.corked = 1;
        this[kSocket].uncork();
      }
      this[kCorked] = 1;
      this.uncork();
      this.finished = true;
      if (this.outputData.length === 0 && this[kSocket] && this[kSocket]._httpMessage === this) {
        this._finish();
      }
      return this;
    };
    OutgoingMessage.prototype._finish = function _finish() {
      assert(this[kSocket]);
      this.emit("prefinish");
    };
    OutgoingMessage.prototype._flush = function _flush() {
      const socket = this[kSocket];
      if (socket?.writable) {
        this._flushOutput(socket);
        if (this.finished) {
          this._finish();
        } else if (this[kNeedDrain] && this.writableLength === 0) {
          this[kNeedDrain] = false;
          this.emit("drain");
        }
      }
    };
    OutgoingMessage.prototype._flushOutput = function _flushOutput(socket) {
      const outputLength = this.outputData.length;
      if (outputLength <= 0)
        return;
      const outputData = this.outputData;
      socket.cork();
      let ret;
      for (let i = 0;i < outputLength; i++) {
        const { data, encoding, callback } = outputData[i];
        outputData[i].data = null;
        ret = socket.write(data, encoding, callback);
      }
      socket.uncork();
      this.outputData = [];
      this._onPendingData(-this.outputSize);
      this.outputSize = 0;
      return ret;
    };
    OutgoingMessage.prototype.flushHeaders = function flushHeaders() {
      if (!this._header) {
        this._implicitHeader();
      }
      this._send("");
    };
    OutgoingMessage.prototype.pipe = function pipe() {
      this.emit("error", codes.STREAM_CANNOT_PIPE());
    };
    OutgoingMessage.prototype[EE.captureRejectionSymbol] = function(err, _event) {
      this.destroy(err);
    };
    ObjectDefineProperty(OutgoingMessage.prototype, "_headers", {
      __proto__: null,
      get: deprecate(function() {
        return this.getHeaders();
      }, "OutgoingMessage.prototype._headers is deprecated", "DEP0066"),
      set: deprecate(function(val) {
        if (val == null) {
          this[kOutHeaders] = null;
        } else if (typeof val === "object") {
          const headers = this[kOutHeaders] = { __proto__: null };
          const keys = ObjectKeys(val);
          for (let i = 0;i < keys.length; ++i) {
            const name = keys[i];
            headers[lowercaseHeaderName(name)] = [name, val[name]];
          }
        }
      }, "OutgoingMessage.prototype._headers is deprecated", "DEP0066")
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "headers", {
      __proto__: null,
      get() {
        return this.getHeaders();
      },
      set(val) {
        if (val == null) {
          this[kOutHeaders] = null;
        } else if (typeof val === "object") {
          this[kOutHeaders] = { __proto__: null };
          const keys = ObjectKeys(val);
          for (let i = 0;i < keys.length; ++i) {
            const name = keys[i];
            this.setHeader(name, val[name]);
          }
        }
      }
    });
    ObjectDefineProperty(OutgoingMessage.prototype, "_headerNames", {
      __proto__: null,
      get: deprecate(function() {
        const headers = this[kOutHeaders];
        if (headers !== null) {
          const out = { __proto__: null };
          const keys = ObjectKeys(headers);
          for (let i = 0;i < keys.length; ++i) {
            const key = keys[i];
            const val = headers[key][0];
            out[key] = val;
          }
          return out;
        }
        return null;
      }, "OutgoingMessage.prototype._headerNames is deprecated", "DEP0066"),
      set: deprecate(function(val) {
        if (typeof val === "object" && val !== null) {
          const headers = this[kOutHeaders];
          if (!headers)
            return;
          const keys = ObjectKeys(val);
          for (let i = 0;i < keys.length; ++i) {
            const header = headers[keys[i]];
            if (header)
              header[0] = val[keys[i]];
          }
        }
      }, "OutgoingMessage.prototype._headerNames is deprecated", "DEP0066")
    });
    return {
      kHighWaterMark,
      kUniqueHeaders,
      kOutHeaders,
      kErrored,
      kSocket,
      kRejectNonStandardBodyWrites,
      parseUniqueHeadersOption,
      validateHeaderName,
      validateHeaderValue,
      OutgoingMessage,
      FakeSocket
    };
  })();
})
