// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/http2.ts` do bun 1.4.2. Não editar à mão.
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
    const { isTypedArray } = require("node:util/types");
    const { hideFromStack, hasObserver, enqueueNodeEntry, PerformanceNodeEntry } = require("internal/shared");
    const { STATUS_CODES } = require("internal/http");
    const { kTimeout, getTimerDuration } = require("internal/timers");
    const tls = require("node:tls");
    const net = require("node:net");
    const fs = require("node:fs");
    const FileHandle = require("node:fs/promises").FileHandle ?? class FileHandle {};
    const bunSocketServerOptions = Symbol.for("::bunnetserveroptions::");
    const kInfoHeaders = Symbol("sent-info-headers");
    const kStrictSingleValueFields = Symbol("strictSingleValueFields");
    const kDeferWriteCallback = Symbol("deferWriteCallback");
    const kProxySocket = Symbol("proxySocket");
    const kSessions = Symbol("sessions");
    const kOptions = Symbol("options");
    const kQuotedString = /^[\x09\x20-\x5b\x5d-\x7e\x80-\xff]*$/;
    const MAX_ADDITIONAL_SETTINGS = 10;
    const Stream = require("node:stream");
    const dc = require("node:diagnostics_channel");
    const onClientStreamCreatedChannel = dc.channel("http2.client.stream.created");
    const onClientStreamStartChannel = dc.channel("http2.client.stream.start");
    const onClientStreamErrorChannel = dc.channel("http2.client.stream.error");
    const onClientStreamBodyChunkSentChannel = dc.channel("http2.client.stream.bodyChunkSent");
    const onClientStreamBodySentChannel = dc.channel("http2.client.stream.bodySent");
    const onClientStreamFinishChannel = dc.channel("http2.client.stream.finish");
    const onClientStreamCloseChannel = dc.channel("http2.client.stream.close");
    const onServerStreamCreatedChannel = dc.channel("http2.server.stream.created");
    const onServerStreamStartChannel = dc.channel("http2.server.stream.start");
    const onServerStreamErrorChannel = dc.channel("http2.server.stream.error");
    const onServerStreamFinishChannel = dc.channel("http2.server.stream.finish");
    const onServerStreamCloseChannel = dc.channel("http2.server.stream.close");
    const { Readable } = Stream;
    const TLSSocket = tls.TLSSocket;
    const EventEmitter = require("node:events");
    const { Duplex } = Stream;
    const { SafeArrayIterator, SafeSet } = require("internal/primordials");
    const { promisify } = require("internal/promisify");
    const ObjectAssign = Object.assign;
    const ArrayIsArray = Array.isArray;
    const ObjectKeys = Object.keys;
    const FunctionPrototypeBind = Function.prototype.bind;
    const StringPrototypeTrim = String.prototype.trim;
    const ArrayPrototypePush = Array.prototype.push;
    const StringPrototypeToLowerCase = String.prototype.toLowerCase;
    const StringPrototypeIncludes = String.prototype.includes;
    const StringPrototypeStartsWith = String.prototype.startsWith;
    const ObjectPrototypeHasOwnProperty = Object.prototype.hasOwnProperty;
    const H2FrameParser = (native => (toClass(native, "H2FrameParser"), native))(function H2FrameParser() { host.internal.throwNotImplemented("http2 frame parser"); });
    const { upgradeRawSocketToH2 } = require("node:_http2_upgrade");
    const kSettingIds = {
      1: "headerTableSize",
      2: "enablePush",
      3: "maxConcurrentStreams",
      4: "initialWindowSize",
      5: "maxFrameSize",
      6: "maxHeaderListSize",
      8: "enableConnectProtocol"
    };
    const kDefaultSettings = {
      headerTableSize: 4096,
      enablePush: true,
      maxConcurrentStreams: 2 ** 32 - 1,
      initialWindowSize: 65535,
      maxFrameSize: 16384,
      maxHeaderListSize: 65535,
      maxHeaderSize: 65535,
      enableConnectProtocol: false
    };
    function initialLocalSettings(submitted) {
      const settings = { ...kDefaultSettings };
      const custom = submitted?.customSettings;
      if (custom != null && typeof custom === "object") {
        settings.customSettings = { ...custom };
      }
      return settings;
    }
    function throwSettingRangeError(name, value) {
      const err = new RangeError(`Invalid value for setting "${name}": ${value}`);
      err.code = "ERR_HTTP2_INVALID_SETTING_VALUE";
      throw err;
    }
    function throwSettingTypeError(name, value) {
      const err = new TypeError(`Invalid value for setting "${name}": ${value}`);
      err.code = "ERR_HTTP2_INVALID_SETTING_VALUE";
      throw err;
    }
    function validateSettings(settings) {
      if (typeof settings !== "object" || settings === null || Array.isArray(settings)) {
        throw codes.INVALID_ARG_TYPE("settings", "object", settings);
      }
      if (settings.headerTableSize !== undefined) {
        const v = settings.headerTableSize;
        if (typeof v !== "number" || v < 0 || v > kMaxInt || Number.isNaN(v)) {
          throwSettingRangeError("headerTableSize", v);
        }
      }
      if (settings.enablePush !== undefined) {
        const v = settings.enablePush;
        if (typeof v !== "boolean") {
          throwSettingTypeError("enablePush", v);
        }
      }
      if (settings.initialWindowSize !== undefined) {
        const v = settings.initialWindowSize;
        if (typeof v !== "number" || v < 0 || v > 2147483647 || Number.isNaN(v)) {
          throwSettingRangeError("initialWindowSize", v);
        }
      }
      if (settings.maxFrameSize !== undefined) {
        const v = settings.maxFrameSize;
        if (typeof v !== "number" || v < 16384 || v > 16777215 || Number.isNaN(v)) {
          throwSettingRangeError("maxFrameSize", v);
        }
      }
      if (settings.maxConcurrentStreams !== undefined) {
        const v = settings.maxConcurrentStreams;
        if (typeof v !== "number" || v < 0 || v > kMaxInt || Number.isNaN(v)) {
          throwSettingRangeError("maxConcurrentStreams", v);
        }
      }
      if (settings.maxHeaderListSize !== undefined) {
        const v = settings.maxHeaderListSize;
        if (typeof v !== "number" || v < 0 || v > kMaxInt || Number.isNaN(v)) {
          throwSettingRangeError("maxHeaderListSize", v);
        }
      }
      if (settings.maxHeaderSize !== undefined) {
        const v = settings.maxHeaderSize;
        if (typeof v !== "number" || v < 0 || v > kMaxInt || Number.isNaN(v)) {
          throwSettingRangeError("maxHeaderSize", v);
        }
      }
      if (settings.enableConnectProtocol !== undefined) {
        const v = settings.enableConnectProtocol;
        if (typeof v !== "boolean") {
          throwSettingTypeError("enableConnectProtocol", v);
        }
      }
      if (settings.customSettings !== undefined) {
        const cs = settings.customSettings;
        if (typeof cs !== "object" || cs === null) {
          throwSettingRangeError("customSettings", cs);
        }
        const keys = ObjectKeys(cs);
        if (keys.length > MAX_ADDITIONAL_SETTINGS) {
          const err = new Error("Number of custom settings exceeds MAX_ADDITIONAL_SETTINGS");
          err.code = "ERR_HTTP2_TOO_MANY_CUSTOM_SETTINGS";
          throw err;
        }
        for (const key of keys) {
          const id = Number(key);
          if (!Number.isInteger(id) || id < 0 || id > 65535) {
            throwSettingRangeError(key, cs[key]);
          }
          const val = cs[key];
          if (typeof val !== "number" || val < 0 || val > kMaxInt || !Number.isFinite(val)) {
            throwSettingRangeError(key, val);
          }
        }
      }
    }
    function assertSettings(settings) {
      validateSettings(settings);
    }
    function getPackedSettings(settings) {
      if (settings === undefined)
        return Buffer.alloc(0);
      validateSettings(settings);
      const entries = [];
      if (settings.headerTableSize !== undefined) {
        entries.push([1, settings.headerTableSize]);
      }
      if (settings.enablePush !== undefined) {
        entries.push([2, settings.enablePush ? 1 : 0]);
      }
      if (settings.maxConcurrentStreams !== undefined) {
        entries.push([3, settings.maxConcurrentStreams]);
      }
      if (settings.initialWindowSize !== undefined) {
        entries.push([4, settings.initialWindowSize]);
      }
      if (settings.maxFrameSize !== undefined) {
        entries.push([5, settings.maxFrameSize]);
      }
      if (settings.maxHeaderListSize !== undefined) {
        entries.push([6, settings.maxHeaderListSize]);
      } else if (settings.maxHeaderSize !== undefined) {
        entries.push([6, settings.maxHeaderSize]);
      }
      if (settings.enableConnectProtocol !== undefined) {
        entries.push([8, settings.enableConnectProtocol ? 1 : 0]);
      }
      if (settings.customSettings) {
        const cs = settings.customSettings;
        const keys = ObjectKeys(cs);
        keys.sort((a, b) => Number(a) - Number(b));
        for (const key of keys) {
          entries.push([Number(key), cs[key]]);
        }
      }
      const buf = Buffer.alloc(entries.length * 6);
      for (let i = 0;i < entries.length; i++) {
        const offset = i * 6;
        buf.writeUInt16BE(entries[i][0], offset);
        buf.writeUInt32BE(entries[i][1], offset + 2);
      }
      return buf;
    }
    function getUnpackedSettings(buf, options) {
      if (buf === undefined) {
        return { ...kDefaultSettings };
      }
      if (!Buffer.isBuffer(buf) && !isTypedArray(buf)) {
        {
          const err = new TypeError(`The "buf" argument must be an instance of Buffer or TypedArray. Received ` + receivedValueLabel(buf));
          err.code = "ERR_INVALID_ARG_TYPE";
          throw err;
        }
      }
      if (buf.length % 6 !== 0) {
        const err = new RangeError("Packed settings length must be a multiple of six");
        err.code = "ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH";
        throw err;
      }
      const settings = {};
      const customSettings = {};
      let hasCustom = false;
      for (let i = 0;i < buf.length; i += 6) {
        const type = buf[i] * 256 + buf[i + 1];
        const value = (buf[i + 2] << 24 | buf[i + 3] << 16 | buf[i + 4] << 8 | buf[i + 5]) >>> 0;
        const name = kSettingIds[type];
        if (name) {
          if (name === "enablePush") {
            settings[name] = value !== 0;
          } else if (name === "enableConnectProtocol") {
            settings[name] = value !== 0;
          } else {
            settings[name] = value;
          }
          if (name === "maxHeaderListSize") {
            settings.maxHeaderSize = value;
          }
        } else {
          customSettings[String(type)] = value;
          hasCustom = true;
        }
      }
      if (hasCustom) {
        settings.customSettings = customSettings;
      }
      if (options && options.validate) {
        validateSettings(settings);
      }
      return settings;
    }
    const sensitiveHeaders = Symbol.for("nodejs.http2.sensitiveHeaders");
    const bunHTTP2Native = Symbol.for("::bunhttp2native::");
    const bunHTTP2Socket = Symbol.for("::bunhttp2socket::");
    const bunHTTP2OriginSet = Symbol("::bunhttp2originset::");
    const bunHTTP2StreamFinal = Symbol.for("::bunHTTP2StreamFinal::");
    const bunHTTP2WaitForTrailers = Symbol("::bunhttp2waitfortrailers::");
    const bunHTTP2StreamStatus = Symbol.for("::bunhttp2StreamStatus::");
    const bunHTTP2Session = Symbol.for("::bunhttp2session::");
    const bunHTTP2Headers = Symbol.for("::bunhttp2headers::");
    const bunHTTP2AsyncContextFrame = Symbol("::bunhttp2asynccontextframe::");
    const bunHTTP2SessionTeardownFrame = Symbol("::bunhttp2sessionteardownframe::");
    const kNoSessionTeardown = Symbol("::bunhttp2noteardown::");
    const runInFrame = require("internal/async_context_frame").run;
    const ReflectGetPrototypeOf = Reflect.getPrototypeOf;
    const kBeginSend = Symbol("begin-send");
    const kServer = Symbol("server");
    const kState = Symbol("state");
    const kStream = Symbol("stream");
    const kResponse = Symbol("response");
    const kHeaders = Symbol("headers");
    const kRawHeaders = Symbol("rawHeaders");
    const kTrailers = Symbol("trailers");
    const kRawTrailers = Symbol("rawTrailers");
    const kSetHeader = Symbol("setHeader");
    const kAppendHeader = Symbol("appendHeader");
    const kAborted = Symbol("aborted");
    const kSetStreamId = Symbol("setStreamId");
    const kRequest = Symbol("request");
    const kHeadRequest = Symbol("headRequest");
    const kSessionDestroyError = Symbol("sessionDestroyError");
    const kOwnsFd = Symbol("ownsFd");
    const kRequestHeaders = Symbol("requestHeaders");
    const kSendingTrailers = Symbol("sendingTrailers");
    const kSettingsAckGraceTimer = Symbol("settingsAckGraceTimer");
    const kBoundSession = Symbol("boundSession");
    const kInspect = Symbol.for("nodejs.util.inspect.custom");
    let util;
    let addAbortListener;
    let priorityDeprecationWarned = false;
    let priorityWeightDeprecationWarned = false;
    const kPush = Symbol("pushStream");
    const kNeverAnnounced = Symbol("neverAnnounced");
    const kReceivedGoaway = Symbol("receivedGoaway");
    const kGoawayCode = Symbol("goawayCode");
    const kGoawayLastStreamID = Symbol("goawayLastStreamID");
    const kReleaseUnannouncedStream = Symbol("releaseUnannouncedStream");
    const kGoawaySent = Symbol("goawaySent");
    function isEconnresetAfterGoaway(session, error) {
      return error?.code === "ECONNRESET" && session[kGoawayCode] !== undefined;
    }
    const kMaxStreams = 2 ** 32 - 1;
    const kMaxUint32 = 4294967295;
    const kMaxInt = 4294967295;
    const kMaxWindowSize = 2 ** 31 - 1;
    const {
      validateInteger,
      validateString,
      validateObject,
      validateFunction,
      checkIsHttpToken,
      validateLinkHeaderValue,
      validateUint32,
      validateBuffer,
      validateNumber,
      validateBoolean,
      validateAbortSignal
    } = require("internal/validators");
    let utcCache;
    function utcDate() {
      if (!utcCache)
        cache();
      return utcCache;
    }
    function emitEventNT(self, event, ...args) {
      if (self.listenerCount(event) > 0) {
        self.emit(event, ...args);
      }
    }
    function emitSessionCloseNT(self, frame) {
      if (self.listenerCount("close") > 0) {
        runInFrame(frame, self.emit, self, "close");
      }
    }
    function emitErrorNT(self, error, destroy) {
      if (destroy) {
        if (self.listenerCount("error") > 0) {
          self.destroy(error);
        } else {
          self.destroy();
        }
      } else if (self.listenerCount("error") > 0) {
        self.emit("error", error);
      }
    }
    function emitOutofStreamErrorNT(self) {
      self.destroy(codes.HTTP2_OUT_OF_STREAMS());
    }
    function cache() {
      const d = new Date;
      utcCache = d.toUTCString();
      setTimeout(resetCache, 1000 - d.getMilliseconds()).unref();
    }
    function resetCache() {
      utcCache = undefined;
    }
    function getAuthority(headers) {
      if (headers[HTTP2_HEADER_AUTHORITY] !== undefined)
        return headers[HTTP2_HEADER_AUTHORITY];
      if (headers[HTTP2_HEADER_HOST] !== undefined)
        return headers[HTTP2_HEADER_HOST];
    }
    function onStreamData(chunk) {
      const request = this[kRequest];
      if (request !== undefined && !request.push(chunk))
        this.pause();
    }
    function onStreamTrailers(trailers, flags, rawTrailers) {
      const request = this[kRequest];
      if (request !== undefined) {
        ObjectAssign(request[kTrailers], trailers);
        ArrayPrototypePush.call(request[kRawTrailers], ...new SafeArrayIterator(rawTrailers));
      }
    }
    function onStreamEnd() {
      const request = this[kRequest];
      if (request !== undefined)
        this[kRequest].push(null);
    }
    function onStreamError(error) {}
    function onRequestPause() {
      this[kStream].pause();
    }
    function onRequestResume() {
      this[kStream].resume();
    }
    function onStreamDrain() {
      const response = this[kResponse];
      if (response !== undefined)
        response.emit("drain");
    }
    function onStreamAbortedResponse() {}
    function onStreamAbortedRequest() {
      const request = this[kRequest];
      if (request !== undefined && request[kState].closed === false) {
        request[kAborted] = true;
        request.emit("aborted");
      }
    }
    function resumeStream(stream) {
      stream.resume();
    }
    function onStreamTrailersReady() {
      this.sendTrailers(this[kResponse][kTrailers]);
    }
    function onStreamCloseResponse() {
      const res = this[kResponse];
      if (res === undefined)
        return;
      const state = res[kState];
      if (this.headRequest !== state.headRequest)
        return;
      state.closed = true;
      this.removeListener("wantTrailers", onStreamTrailersReady);
      this[kResponse] = undefined;
      res.emit("finish");
      res.emit("close");
    }
    function onStreamCloseRequest() {
      const req = this[kRequest];
      if (req === undefined)
        return;
      const state = req[kState];
      state.closed = true;
      req.push(null);
      if (!state.didRead && !req._readableState.resumeScheduled)
        req.resume();
      this[kRequest] = undefined;
      req.emit("close");
    }
    function onStreamTimeout() {
      this.emit("timeout");
    }
    function isPseudoHeader(name) {
      switch (name) {
        case HTTP2_HEADER_STATUS:
        case HTTP2_HEADER_METHOD:
        case HTTP2_HEADER_PATH:
        case HTTP2_HEADER_AUTHORITY:
        case HTTP2_HEADER_SCHEME:
          return true;
        default:
          return false;
      }
    }
    function isConnectionHeaderAllowed(name, value) {
      return name !== HTTP2_HEADER_CONNECTION || value === "trailers";
    }
    let statusConnectionHeaderWarned = false;
    let statusMessageWarned = false;
    function statusMessageWarn() {
      if (statusMessageWarned === false) {
        process.emitWarning("Status message is not supported by HTTP/2 (RFC7540 8.1.2.4)", "UnsupportedWarning");
        statusMessageWarned = true;
      }
    }
    function connectionHeaderMessageWarn() {
      if (statusConnectionHeaderWarned === false) {
        process.emitWarning("The provided connection header is not valid, " + "the value will be dropped from the header and " + "will never be in use.", "UnsupportedWarning");
        statusConnectionHeaderWarned = true;
      }
    }
    function assertValidHeader(name, value) {
      if (name === "" || typeof name !== "string" || StringPrototypeIncludes.call(name, " ")) {
        throw codes.INVALID_HTTP_TOKEN("Header name", name);
      }
      if (isPseudoHeader(name)) {
        throw codes.HTTP2_PSEUDOHEADER_NOT_ALLOWED();
      }
      if (value === undefined || value === null) {
        throw codes.HTTP2_INVALID_HEADER_VALUE(value, name);
      }
      if (!isConnectionHeaderAllowed(name, value)) {
        connectionHeaderMessageWarn();
      }
    }
    function assertIsObject(value, name, types) {
      if (value !== undefined && (!isObject(value) || Array.isArray(value))) {
        throw codes.INVALID_ARG_TYPE(name, Array.isArray(types) ? types : [types || "Object"], value);
      }
    }
    function assertIsArray(value, name, types) {
      if (value !== undefined && !Array.isArray(value)) {
        throw codes.INVALID_ARG_TYPE(name, Array.isArray(types) ? types : [types || "Array"], value);
      }
    }
    hideFromStack(assertIsObject);
    hideFromStack(assertIsArray);
    hideFromStack(assertValidHeader);

    class Http2ServerRequest extends Readable {
      [kState];
      [kHeaders];
      [kRawHeaders];
      [kTrailers];
      [kRawTrailers];
      [kStream];
      [kAborted];
      constructor(stream, headers, options, rawHeaders) {
        super({ autoDestroy: false, ...options });
        this[kState] = {
          closed: false,
          didRead: false
        };
        this[kHeaders] = headers;
        this[kRawHeaders] = rawHeaders;
        this[kTrailers] = {};
        this[kRawTrailers] = [];
        this[kStream] = stream;
        this[kAborted] = false;
        stream[kRequest] = this;
        stream.on("trailers", onStreamTrailers);
        stream.on("end", onStreamEnd);
        stream.on("error", onStreamError);
        stream.on("aborted", onStreamAbortedRequest);
        stream.on("close", onStreamCloseRequest);
        stream.on("timeout", onStreamTimeout.bind(this));
        this.on("pause", onRequestPause);
        this.on("resume", onRequestResume);
      }
      get aborted() {
        return this[kAborted];
      }
      get complete() {
        return this[kAborted] || this.readableEnded || this[kState].closed || this[kStream].destroyed;
      }
      get stream() {
        return this[kStream];
      }
      get headers() {
        return this[kHeaders];
      }
      get rawHeaders() {
        return this[kRawHeaders];
      }
      get trailers() {
        return this[kTrailers];
      }
      get rawTrailers() {
        return this[kRawTrailers];
      }
      get httpVersionMajor() {
        return 2;
      }
      get httpVersionMinor() {
        return 0;
      }
      get httpVersion() {
        return "2.0";
      }
      get socket() {
        const stream = this[kStream];
        const proxySocket = stream[kProxySocket];
        if (proxySocket == null)
          return stream[kProxySocket] = new Proxy(stream, proxyCompatSocketHandler);
        return proxySocket;
      }
      get connection() {
        return this.socket;
      }
      _read(nread) {
        const state = this[kState];
        if (!state.didRead) {
          state.didRead = true;
          this[kStream].on("data", onStreamData);
        } else {
          process.nextTick(resumeStream, this[kStream]);
        }
      }
      get method() {
        return this[kHeaders][HTTP2_HEADER_METHOD];
      }
      set method(method) {
        validateString(method, "method");
        if (StringPrototypeTrim.call(method) === "")
          throw codes.INVALID_ARG_VALUE("method", method);
        this[kHeaders][HTTP2_HEADER_METHOD] = method;
      }
      get authority() {
        return getAuthority(this[kHeaders]);
      }
      get scheme() {
        return this[kHeaders][HTTP2_HEADER_SCHEME];
      }
      get url() {
        return this[kHeaders][HTTP2_HEADER_PATH];
      }
      set url(url) {
        this[kHeaders][HTTP2_HEADER_PATH] = url;
      }
      setTimeout(msecs, callback) {
        if (!this[kState].closed)
          this[kStream].setTimeout(msecs, callback);
        return this;
      }
    }

    class Http2ServerResponse extends Stream {
      [kState];
      [kHeaders];
      [kTrailers];
      [kStream];
      constructor(stream, options) {
        super(options);
        this[kState] = {
          closed: false,
          ending: false,
          destroyed: false,
          headRequest: false,
          sendDate: true,
          statusCode: HTTP_STATUS_OK
        };
        this[kHeaders] = Object.create(null);
        this[kTrailers] = Object.create(null);
        this[kStream] = stream;
        stream[kResponse] = this;
        this.writable = true;
        this.req = stream[kRequest];
        stream.on("drain", onStreamDrain);
        stream.on("aborted", onStreamAbortedResponse);
        stream.on("close", onStreamCloseResponse);
        stream.on("wantTrailers", onStreamTrailersReady);
        stream.on("timeout", onStreamTimeout.bind(this));
      }
      get _header() {
        return this.headersSent;
      }
      get writableEnded() {
        const state = this[kState];
        return state.ending;
      }
      get finished() {
        const state = this[kState];
        return state.ending;
      }
      get socket() {
        if (this[kState].closed)
          return;
        const stream = this[kStream];
        const proxySocket = stream[kProxySocket];
        if (proxySocket == null)
          return stream[kProxySocket] = new Proxy(stream, proxyCompatSocketHandler);
        return proxySocket;
      }
      get connection() {
        return this.socket;
      }
      get stream() {
        return this[kStream];
      }
      get headersSent() {
        return this[kStream].headersSent;
      }
      get sendDate() {
        return this[kState].sendDate;
      }
      set sendDate(bool) {
        this[kState].sendDate = Boolean(bool);
      }
      get statusCode() {
        return this[kState].statusCode;
      }
      get writableCorked() {
        return this[kStream].writableCorked;
      }
      get writableHighWaterMark() {
        return this[kStream].writableHighWaterMark;
      }
      get writableObjectMode() {
        return this[kStream].writableObjectMode;
      }
      get writableNeedDrain() {
        return this[kStream].writableNeedDrain;
      }
      get writableFinished() {
        return this[kStream].writableFinished;
      }
      get writableLength() {
        return this[kStream].writableLength;
      }
      set statusCode(code) {
        code |= 0;
        if (code >= 100 && code < 200)
          throw codes.HTTP2_INFO_STATUS_NOT_ALLOWED();
        if (code < 100 || code > 599)
          throw codes.HTTP2_STATUS_INVALID(code);
        this[kState].statusCode = code;
      }
      setTrailer(name, value) {
        validateString(name, "name");
        name = StringPrototypeToLowerCase.call(StringPrototypeTrim.call(name));
        assertValidHeader(name, value);
        this[kTrailers][name] = value;
      }
      addTrailers(headers) {
        const keys = ObjectKeys(headers);
        let key = "";
        for (let i = 0;i < keys.length; i++) {
          key = keys[i];
          this.setTrailer(key, headers[key]);
        }
      }
      getHeader(name) {
        validateString(name, "name");
        name = StringPrototypeToLowerCase.call(StringPrototypeTrim.call(name));
        return this[kHeaders][name];
      }
      getHeaderNames() {
        return ObjectKeys(this[kHeaders]);
      }
      getHeaders() {
        const headers = Object.create(null);
        return ObjectAssign(headers, this[kHeaders]);
      }
      hasHeader(name) {
        validateString(name, "name");
        name = StringPrototypeToLowerCase.call(StringPrototypeTrim.call(name));
        return ObjectPrototypeHasOwnProperty.call(this[kHeaders], name);
      }
      removeHeader(name) {
        validateString(name, "name");
        if (this[kStream].headersSent)
          throw codes.HTTP2_HEADERS_SENT();
        name = StringPrototypeToLowerCase.call(StringPrototypeTrim.call(name));
        if (name === "date") {
          this[kState].sendDate = false;
          return;
        }
        delete this[kHeaders][name];
      }
      setHeader(name, value) {
        validateString(name, "name");
        if (this[kStream].headersSent)
          throw codes.HTTP2_HEADERS_SENT();
        this[kSetHeader](name, value);
      }
      [kSetHeader](name, value) {
        name = StringPrototypeToLowerCase.call(StringPrototypeTrim.call(name));
        assertValidHeader(name, value);
        if (!isConnectionHeaderAllowed(name, value)) {
          return;
        }
        if (name[0] === ":")
          assertValidPseudoHeader(name);
        else if (!checkIsHttpToken(name))
          this.destroy(codes.INVALID_HTTP_TOKEN("Header name", name));
        this[kHeaders][name] = value;
      }
      appendHeader(name, value) {
        validateString(name, "name");
        if (this[kStream].headersSent)
          throw codes.HTTP2_HEADERS_SENT();
        this[kAppendHeader](name, value);
      }
      [kAppendHeader](name, value) {
        name = StringPrototypeToLowerCase.call(StringPrototypeTrim.call(name));
        assertValidHeader(name, value);
        if (!isConnectionHeaderAllowed(name, value)) {
          return;
        }
        if (name[0] === ":")
          assertValidPseudoHeader(name);
        else if (!checkIsHttpToken(name))
          this.destroy(codes.INVALID_HTTP_TOKEN("Header name", name));
        const headers = this[kHeaders];
        if (headers === null || !headers[name]) {
          return this.setHeader(name, value);
        }
        if (!ArrayIsArray(headers[name])) {
          headers[name] = [headers[name]];
        }
        const existingValues = headers[name];
        if (ArrayIsArray(value)) {
          for (let i = 0, length = value.length;i < length; i++) {
            existingValues.push(value[i]);
          }
        } else {
          existingValues.push(value);
        }
      }
      get statusMessage() {
        statusMessageWarn();
        return "";
      }
      set statusMessage(msg) {
        statusMessageWarn();
      }
      flushHeaders() {
        const state = this[kState];
        if (!state.closed && !this[kStream].headersSent)
          this.writeHead(state.statusCode);
      }
      writeHead(statusCode, statusMessage, headers) {
        const state = this[kState];
        if (state.closed || this.stream.destroyed || this.stream.closed)
          return this;
        if (this[kStream].headersSent)
          throw codes.HTTP2_HEADERS_SENT();
        if (typeof statusMessage === "string")
          statusMessageWarn();
        if (headers === undefined && typeof statusMessage === "object")
          headers = statusMessage;
        let i;
        if (ArrayIsArray(headers)) {
          if (this[kHeaders]) {
            if (headers.length && ArrayIsArray(headers[0])) {
              for (let n = 0;n < headers.length; n += 1) {
                const key = headers[n + 0][0];
                this.removeHeader(key);
              }
            } else {
              for (let n = 0;n < headers.length; n += 2) {
                const key = headers[n + 0];
                this.removeHeader(key);
              }
            }
          }
          if (headers.length && ArrayIsArray(headers[0])) {
            for (i = 0;i < headers.length; i++) {
              const header = headers[i];
              this[kAppendHeader](header[0], header[1]);
            }
          } else {
            if (headers.length % 2 !== 0) {
              throw codes.INVALID_ARG_VALUE("headers", headers);
            }
            for (i = 0;i < headers.length; i += 2) {
              this[kAppendHeader](headers[i], headers[i + 1]);
            }
          }
        } else if (typeof headers === "object") {
          const keys = ObjectKeys(headers);
          let key = "";
          for (i = 0;i < keys.length; i++) {
            key = keys[i];
            this[kSetHeader](key, headers[key]);
          }
        }
        state.statusCode = statusCode;
        this[kBeginSend]();
        return this;
      }
      cork() {
        this[kStream].cork();
      }
      uncork() {
        this[kStream].uncork();
      }
      write(chunk, encoding, cb) {
        const state = this[kState];
        if (typeof encoding === "function") {
          cb = encoding;
          encoding = "utf8";
        }
        let err;
        if (state.ending) {
          err = codes.STREAM_WRITE_AFTER_END();
        } else if (state.closed) {
          err = codes.HTTP2_INVALID_STREAM();
        } else if (state.destroyed) {
          return false;
        }
        if (err) {
          if (typeof cb === "function")
            process.nextTick(cb, err);
          this.destroy(err);
          return false;
        }
        const stream = this[kStream];
        if (!stream.headersSent)
          this.writeHead(state.statusCode);
        return stream.write(chunk, encoding, cb);
      }
      end(chunk, encoding, cb) {
        const stream = this[kStream];
        const state = this[kState];
        if (typeof chunk === "function") {
          cb = chunk;
          chunk = null;
        } else if (typeof encoding === "function") {
          cb = encoding;
          encoding = "utf8";
        }
        if ((state.closed || state.ending) && state.headRequest === stream.headRequest) {
          if (typeof cb === "function") {
            process.nextTick(cb);
          }
          return this;
        }
        if (chunk !== null && chunk !== undefined)
          this.write(chunk, encoding);
        state.headRequest = stream.headRequest;
        state.ending = true;
        if (typeof cb === "function") {
          if (stream.writableEnded)
            this.once("finish", cb);
          else
            stream.once("finish", cb);
        }
        if (!stream.headersSent)
          this.writeHead(this[kState].statusCode);
        if (this[kState].closed || stream.destroyed)
          onStreamCloseResponse.call(stream);
        else
          stream.end();
        return this;
      }
      destroy(err) {
        if (this[kState].destroyed)
          return;
        this[kState].destroyed = true;
        this[kStream].destroy(err);
      }
      setTimeout(msecs, callback) {
        if (this[kState].closed)
          return;
        this[kStream].setTimeout(msecs, callback);
      }
      createPushResponse(headers, callback) {
        validateFunction(callback, "callback");
        if (this[kState].closed) {
          const error = codes.HTTP2_INVALID_STREAM();
          process.nextTick(callback, error);
          return;
        }
        this[kStream].pushStream(headers, {}, (err, stream, headers, options) => {
          if (err) {
            callback(err);
            return;
          }
          callback(null, new Http2ServerResponse(stream));
        });
      }
      [kBeginSend]() {
        const state = this[kState];
        const headers = this[kHeaders];
        headers[HTTP2_HEADER_STATUS] = state.statusCode;
        const options = {
          endStream: state.ending,
          waitForTrailers: true,
          sendDate: state.sendDate
        };
        this[kStream].respond(headers, options);
      }
      writeContinue() {
        const stream = this[kStream];
        if (stream.headersSent || this[kState].closed)
          return false;
        stream.additionalHeaders({
          [HTTP2_HEADER_STATUS]: HTTP_STATUS_CONTINUE
        });
        return true;
      }
      writeInformation(statusCode, headers) {
        if (typeof statusCode !== "number" || (statusCode | 0) !== statusCode || statusCode < 100 || statusCode > 199 || statusCode === 101) {
          throw codes.HTTP2_STATUS_INVALID(statusCode);
        }
        const stream = this[kStream];
        if (stream.headersSent || this[kState].closed)
          return false;
        stream.additionalHeaders({
          ...headers || {},
          [HTTP2_HEADER_STATUS]: statusCode
        });
        return true;
      }
      writeEarlyHints(hints) {
        validateObject(hints, "hints");
        const headers = Object.create(null);
        const linkHeaderValue = validateLinkHeaderValue(hints.link);
        for (const key of ObjectKeys(hints)) {
          if (key !== "link") {
            headers[key] = hints[key];
          }
        }
        if (linkHeaderValue.length === 0) {
          return false;
        }
        const stream = this[kStream];
        if (stream.headersSent || this[kState].closed)
          return false;
        stream.additionalHeaders({
          ...headers,
          [HTTP2_HEADER_STATUS]: HTTP_STATUS_EARLY_HINTS,
          Link: linkHeaderValue
        });
        return true;
      }
    }
    function onServerStream(Http2ServerRequest, Http2ServerResponse, stream, headers, flags, rawHeaders) {
      const server = this;
      const request = new Http2ServerRequest(stream, headers, undefined, rawHeaders);
      const response = new Http2ServerResponse(stream);
      const method = headers[HTTP2_HEADER_METHOD];
      if (method === "CONNECT") {
        if (!server.emit("connect", request, response)) {
          response.statusCode = HTTP_STATUS_METHOD_NOT_ALLOWED;
          response.end();
        }
        return;
      }
      if (headers.expect !== undefined) {
        if (headers.expect === "100-continue") {
          if (server.listenerCount("checkContinue")) {
            server.emit("checkContinue", request, response);
          } else {
            response.writeContinue();
            server.emit("request", request, response);
          }
        } else if (server.listenerCount("checkExpectation")) {
          server.emit("checkExpectation", request, response);
        } else {
          response.statusCode = HTTP_STATUS_EXPECTATION_FAILED;
          response.end();
        }
        return;
      }
      server.emit("request", request, response);
    }
    function socketBytesWritten(socket) {
      const native = socket._handle?.bytesWritten || 0;
      const js = socket.bytesWritten || 0;
      return native > js ? native : js;
    }
    const proxyCompatSocketHandler = {
      has(stream, prop) {
        const ref = stream.session !== undefined ? stream.session[bunHTTP2Socket] : stream;
        return prop in stream || prop in ref;
      },
      get(stream, prop) {
        switch (prop) {
          case "on":
          case "once":
          case "end":
          case "emit":
          case "destroy":
            return stream[prop].bind(stream);
          case "writable":
          case "destroyed":
            return stream[prop];
          case "readable": {
            if (stream.destroyed)
              return false;
            const request = stream[kRequest];
            return request ? request.readable : stream.readable;
          }
          case "setTimeout": {
            const session = stream.session;
            if (session !== undefined)
              return session.setTimeout.bind(session);
            return stream.setTimeout.bind(stream);
          }
          case "write":
          case "read":
          case "pause":
          case "resume":
            throw codes.HTTP2_NO_SOCKET_MANIPULATION();
          case "bytesWritten": {
            const ref = stream.session !== undefined ? stream.session[bunHTTP2Socket] : stream;
            return socketBytesWritten(ref);
          }
          default: {
            const ref = stream.session !== undefined ? stream.session[bunHTTP2Socket] : stream;
            const value = ref[prop];
            return typeof value === "function" ? value.bind(ref) : value;
          }
        }
      },
      getPrototypeOf(stream) {
        if (stream.session !== undefined)
          return ReflectGetPrototypeOf(stream.session[bunHTTP2Socket]);
        return ReflectGetPrototypeOf(stream);
      },
      set(stream, prop, value) {
        switch (prop) {
          case "writable":
          case "readable":
          case "destroyed":
          case "on":
          case "once":
          case "end":
          case "emit":
          case "destroy":
            stream[prop] = value;
            return true;
          case "setTimeout": {
            const session = stream.session;
            if (session !== undefined)
              session.setTimeout = value;
            else
              stream.setTimeout = value;
            return true;
          }
          case "write":
          case "read":
          case "pause":
          case "resume":
            throw codes.HTTP2_NO_SOCKET_MANIPULATION();
          default: {
            const ref = stream.session !== undefined ? stream.session[bunHTTP2Socket] : stream;
            ref[prop] = value;
            return true;
          }
        }
      }
    };
    const proxySocketHandler = {
      get(session, prop) {
        switch (prop) {
          case "setTimeout":
          case "ref":
          case "unref":
            return FunctionPrototypeBind.call(session[prop], session);
          case "destroy":
          case "emit":
          case "end":
          case "pause":
          case "read":
          case "resume":
          case "write":
          case "setEncoding":
          case "setKeepAlive":
          case "setNoDelay":
            throw codes.HTTP2_NO_SOCKET_MANIPULATION();
          case "bytesWritten": {
            const socket = session[bunHTTP2Socket];
            if (!socket) {
              throw codes.HTTP2_SOCKET_UNBOUND();
            }
            return socketBytesWritten(socket);
          }
          default: {
            const socket = session[bunHTTP2Socket];
            if (!socket) {
              throw codes.HTTP2_SOCKET_UNBOUND();
            }
            const value = socket[prop];
            return typeof value === "function" ? FunctionPrototypeBind.call(value, socket) : value;
          }
        }
      },
      getPrototypeOf(session) {
        const socket = session[bunHTTP2Socket];
        if (!socket) {
          throw codes.HTTP2_SOCKET_UNBOUND();
        }
        return ReflectGetPrototypeOf(socket);
      },
      set(session, prop, value) {
        switch (prop) {
          case "setTimeout":
          case "ref":
          case "unref":
            session[prop] = value;
            return true;
          case "destroy":
          case "emit":
          case "end":
          case "pause":
          case "read":
          case "resume":
          case "write":
          case "setEncoding":
          case "setKeepAlive":
          case "setNoDelay":
            throw codes.HTTP2_NO_SOCKET_MANIPULATION();
          default: {
            const socket = session[bunHTTP2Socket];
            if (!socket) {
              throw codes.HTTP2_SOCKET_UNBOUND();
            }
            socket[prop] = value;
            return true;
          }
        }
      }
    };
    const nameForErrorCode = [
      "NGHTTP2_NO_ERROR",
      "NGHTTP2_PROTOCOL_ERROR",
      "NGHTTP2_INTERNAL_ERROR",
      "NGHTTP2_FLOW_CONTROL_ERROR",
      "NGHTTP2_SETTINGS_TIMEOUT",
      "NGHTTP2_STREAM_CLOSED",
      "NGHTTP2_FRAME_SIZE_ERROR",
      "NGHTTP2_REFUSED_STREAM",
      "NGHTTP2_CANCEL",
      "NGHTTP2_COMPRESSION_ERROR",
      "NGHTTP2_CONNECT_ERROR",
      "NGHTTP2_ENHANCE_YOUR_CALM",
      "NGHTTP2_INADEQUATE_SECURITY",
      "NGHTTP2_HTTP_1_1_REQUIRED"
    ];
    const constants = {
      NGHTTP2_ERR_FRAME_SIZE_ERROR: -522,
      NGHTTP2_SESSION_SERVER: 0,
      NGHTTP2_SESSION_CLIENT: 1,
      NGHTTP2_STREAM_STATE_IDLE: 1,
      NGHTTP2_STREAM_STATE_OPEN: 2,
      NGHTTP2_STREAM_STATE_RESERVED_LOCAL: 3,
      NGHTTP2_STREAM_STATE_RESERVED_REMOTE: 4,
      NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL: 5,
      NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE: 6,
      NGHTTP2_STREAM_STATE_CLOSED: 7,
      NGHTTP2_FLAG_NONE: 0,
      NGHTTP2_FLAG_END_STREAM: 1,
      NGHTTP2_FLAG_END_HEADERS: 4,
      NGHTTP2_FLAG_ACK: 1,
      NGHTTP2_FLAG_PADDED: 8,
      NGHTTP2_FLAG_PRIORITY: 32,
      DEFAULT_SETTINGS_HEADER_TABLE_SIZE: 4096,
      DEFAULT_SETTINGS_ENABLE_PUSH: 1,
      DEFAULT_SETTINGS_MAX_CONCURRENT_STREAMS: 4294967295,
      DEFAULT_SETTINGS_INITIAL_WINDOW_SIZE: 65535,
      DEFAULT_SETTINGS_MAX_FRAME_SIZE: 16384,
      DEFAULT_SETTINGS_MAX_HEADER_LIST_SIZE: 65535,
      DEFAULT_SETTINGS_ENABLE_CONNECT_PROTOCOL: 0,
      MAX_MAX_FRAME_SIZE: 16777215,
      MIN_MAX_FRAME_SIZE: 16384,
      MAX_INITIAL_WINDOW_SIZE: 2147483647,
      NGHTTP2_SETTINGS_HEADER_TABLE_SIZE: 1,
      NGHTTP2_SETTINGS_ENABLE_PUSH: 2,
      NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS: 3,
      NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE: 4,
      NGHTTP2_SETTINGS_MAX_FRAME_SIZE: 5,
      NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE: 6,
      NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL: 8,
      PADDING_STRATEGY_NONE: 0,
      PADDING_STRATEGY_ALIGNED: 1,
      PADDING_STRATEGY_MAX: 2,
      PADDING_STRATEGY_CALLBACK: 1,
      NGHTTP2_NO_ERROR: 0,
      NGHTTP2_PROTOCOL_ERROR: 1,
      NGHTTP2_INTERNAL_ERROR: 2,
      NGHTTP2_FLOW_CONTROL_ERROR: 3,
      NGHTTP2_SETTINGS_TIMEOUT: 4,
      NGHTTP2_STREAM_CLOSED: 5,
      NGHTTP2_FRAME_SIZE_ERROR: 6,
      NGHTTP2_REFUSED_STREAM: 7,
      NGHTTP2_CANCEL: 8,
      NGHTTP2_COMPRESSION_ERROR: 9,
      NGHTTP2_CONNECT_ERROR: 10,
      NGHTTP2_ENHANCE_YOUR_CALM: 11,
      NGHTTP2_INADEQUATE_SECURITY: 12,
      NGHTTP2_HTTP_1_1_REQUIRED: 13,
      NGHTTP2_DEFAULT_WEIGHT: 16,
      HTTP2_HEADER_STATUS: ":status",
      HTTP2_HEADER_METHOD: ":method",
      HTTP2_HEADER_AUTHORITY: ":authority",
      HTTP2_HEADER_SCHEME: ":scheme",
      HTTP2_HEADER_PATH: ":path",
      HTTP2_HEADER_PROTOCOL: ":protocol",
      HTTP2_HEADER_ACCEPT_ENCODING: "accept-encoding",
      HTTP2_HEADER_ACCEPT_LANGUAGE: "accept-language",
      HTTP2_HEADER_ACCEPT_RANGES: "accept-ranges",
      HTTP2_HEADER_ACCEPT: "accept",
      HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS: "access-control-allow-credentials",
      HTTP2_HEADER_ACCESS_CONTROL_ALLOW_HEADERS: "access-control-allow-headers",
      HTTP2_HEADER_ACCESS_CONTROL_ALLOW_METHODS: "access-control-allow-methods",
      HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN: "access-control-allow-origin",
      HTTP2_HEADER_ACCESS_CONTROL_EXPOSE_HEADERS: "access-control-expose-headers",
      HTTP2_HEADER_ACCESS_CONTROL_REQUEST_HEADERS: "access-control-request-headers",
      HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD: "access-control-request-method",
      HTTP2_HEADER_AGE: "age",
      HTTP2_HEADER_AUTHORIZATION: "authorization",
      HTTP2_HEADER_CACHE_CONTROL: "cache-control",
      HTTP2_HEADER_CONNECTION: "connection",
      HTTP2_HEADER_CONTENT_DISPOSITION: "content-disposition",
      HTTP2_HEADER_CONTENT_ENCODING: "content-encoding",
      HTTP2_HEADER_CONTENT_LENGTH: "content-length",
      HTTP2_HEADER_CONTENT_TYPE: "content-type",
      HTTP2_HEADER_COOKIE: "cookie",
      HTTP2_HEADER_DATE: "date",
      HTTP2_HEADER_ETAG: "etag",
      HTTP2_HEADER_FORWARDED: "forwarded",
      HTTP2_HEADER_HOST: "host",
      HTTP2_HEADER_IF_MODIFIED_SINCE: "if-modified-since",
      HTTP2_HEADER_IF_NONE_MATCH: "if-none-match",
      HTTP2_HEADER_IF_RANGE: "if-range",
      HTTP2_HEADER_LAST_MODIFIED: "last-modified",
      HTTP2_HEADER_LINK: "link",
      HTTP2_HEADER_LOCATION: "location",
      HTTP2_HEADER_RANGE: "range",
      HTTP2_HEADER_REFERER: "referer",
      HTTP2_HEADER_SERVER: "server",
      HTTP2_HEADER_SET_COOKIE: "set-cookie",
      HTTP2_HEADER_STRICT_TRANSPORT_SECURITY: "strict-transport-security",
      HTTP2_HEADER_TRANSFER_ENCODING: "transfer-encoding",
      HTTP2_HEADER_TE: "te",
      HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS: "upgrade-insecure-requests",
      HTTP2_HEADER_UPGRADE: "upgrade",
      HTTP2_HEADER_USER_AGENT: "user-agent",
      HTTP2_HEADER_VARY: "vary",
      HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS: "x-content-type-options",
      HTTP2_HEADER_X_FRAME_OPTIONS: "x-frame-options",
      HTTP2_HEADER_KEEP_ALIVE: "keep-alive",
      HTTP2_HEADER_PROXY_CONNECTION: "proxy-connection",
      HTTP2_HEADER_X_XSS_PROTECTION: "x-xss-protection",
      HTTP2_HEADER_ALT_SVC: "alt-svc",
      HTTP2_HEADER_CONTENT_SECURITY_POLICY: "content-security-policy",
      HTTP2_HEADER_EARLY_DATA: "early-data",
      HTTP2_HEADER_EXPECT_CT: "expect-ct",
      HTTP2_HEADER_ORIGIN: "origin",
      HTTP2_HEADER_PURPOSE: "purpose",
      HTTP2_HEADER_TIMING_ALLOW_ORIGIN: "timing-allow-origin",
      HTTP2_HEADER_X_FORWARDED_FOR: "x-forwarded-for",
      HTTP2_HEADER_PRIORITY: "priority",
      HTTP2_HEADER_ACCEPT_CHARSET: "accept-charset",
      HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE: "access-control-max-age",
      HTTP2_HEADER_ALLOW: "allow",
      HTTP2_HEADER_CONTENT_LANGUAGE: "content-language",
      HTTP2_HEADER_CONTENT_LOCATION: "content-location",
      HTTP2_HEADER_CONTENT_MD5: "content-md5",
      HTTP2_HEADER_CONTENT_RANGE: "content-range",
      HTTP2_HEADER_DNT: "dnt",
      HTTP2_HEADER_EXPECT: "expect",
      HTTP2_HEADER_EXPIRES: "expires",
      HTTP2_HEADER_FROM: "from",
      HTTP2_HEADER_IF_MATCH: "if-match",
      HTTP2_HEADER_IF_UNMODIFIED_SINCE: "if-unmodified-since",
      HTTP2_HEADER_MAX_FORWARDS: "max-forwards",
      HTTP2_HEADER_PREFER: "prefer",
      HTTP2_HEADER_PROXY_AUTHENTICATE: "proxy-authenticate",
      HTTP2_HEADER_PROXY_AUTHORIZATION: "proxy-authorization",
      HTTP2_HEADER_REFRESH: "refresh",
      HTTP2_HEADER_RETRY_AFTER: "retry-after",
      HTTP2_HEADER_TRAILER: "trailer",
      HTTP2_HEADER_TK: "tk",
      HTTP2_HEADER_VIA: "via",
      HTTP2_HEADER_WARNING: "warning",
      HTTP2_HEADER_WWW_AUTHENTICATE: "www-authenticate",
      HTTP2_HEADER_HTTP2_SETTINGS: "http2-settings",
      HTTP2_METHOD_ACL: "ACL",
      HTTP2_METHOD_BASELINE_CONTROL: "BASELINE-CONTROL",
      HTTP2_METHOD_BIND: "BIND",
      HTTP2_METHOD_CHECKIN: "CHECKIN",
      HTTP2_METHOD_CHECKOUT: "CHECKOUT",
      HTTP2_METHOD_CONNECT: "CONNECT",
      HTTP2_METHOD_COPY: "COPY",
      HTTP2_METHOD_DELETE: "DELETE",
      HTTP2_METHOD_GET: "GET",
      HTTP2_METHOD_HEAD: "HEAD",
      HTTP2_METHOD_LABEL: "LABEL",
      HTTP2_METHOD_LINK: "LINK",
      HTTP2_METHOD_LOCK: "LOCK",
      HTTP2_METHOD_MERGE: "MERGE",
      HTTP2_METHOD_MKACTIVITY: "MKACTIVITY",
      HTTP2_METHOD_MKCALENDAR: "MKCALENDAR",
      HTTP2_METHOD_MKCOL: "MKCOL",
      HTTP2_METHOD_MKREDIRECTREF: "MKREDIRECTREF",
      HTTP2_METHOD_MKWORKSPACE: "MKWORKSPACE",
      HTTP2_METHOD_MOVE: "MOVE",
      HTTP2_METHOD_OPTIONS: "OPTIONS",
      HTTP2_METHOD_ORDERPATCH: "ORDERPATCH",
      HTTP2_METHOD_PATCH: "PATCH",
      HTTP2_METHOD_POST: "POST",
      HTTP2_METHOD_PRI: "PRI",
      HTTP2_METHOD_PROPFIND: "PROPFIND",
      HTTP2_METHOD_PROPPATCH: "PROPPATCH",
      HTTP2_METHOD_PUT: "PUT",
      HTTP2_METHOD_REBIND: "REBIND",
      HTTP2_METHOD_REPORT: "REPORT",
      HTTP2_METHOD_SEARCH: "SEARCH",
      HTTP2_METHOD_TRACE: "TRACE",
      HTTP2_METHOD_UNBIND: "UNBIND",
      HTTP2_METHOD_UNCHECKOUT: "UNCHECKOUT",
      HTTP2_METHOD_UNLINK: "UNLINK",
      HTTP2_METHOD_UNLOCK: "UNLOCK",
      HTTP2_METHOD_UPDATE: "UPDATE",
      HTTP2_METHOD_UPDATEREDIRECTREF: "UPDATEREDIRECTREF",
      HTTP2_METHOD_VERSION_CONTROL: "VERSION-CONTROL",
      HTTP_STATUS_CONTINUE: 100,
      HTTP_STATUS_SWITCHING_PROTOCOLS: 101,
      HTTP_STATUS_PROCESSING: 102,
      HTTP_STATUS_EARLY_HINTS: 103,
      HTTP_STATUS_OK: 200,
      HTTP_STATUS_CREATED: 201,
      HTTP_STATUS_ACCEPTED: 202,
      HTTP_STATUS_NON_AUTHORITATIVE_INFORMATION: 203,
      HTTP_STATUS_NO_CONTENT: 204,
      HTTP_STATUS_RESET_CONTENT: 205,
      HTTP_STATUS_PARTIAL_CONTENT: 206,
      HTTP_STATUS_MULTI_STATUS: 207,
      HTTP_STATUS_ALREADY_REPORTED: 208,
      HTTP_STATUS_IM_USED: 226,
      HTTP_STATUS_MULTIPLE_CHOICES: 300,
      HTTP_STATUS_MOVED_PERMANENTLY: 301,
      HTTP_STATUS_FOUND: 302,
      HTTP_STATUS_SEE_OTHER: 303,
      HTTP_STATUS_NOT_MODIFIED: 304,
      HTTP_STATUS_USE_PROXY: 305,
      HTTP_STATUS_TEMPORARY_REDIRECT: 307,
      HTTP_STATUS_PERMANENT_REDIRECT: 308,
      HTTP_STATUS_BAD_REQUEST: 400,
      HTTP_STATUS_UNAUTHORIZED: 401,
      HTTP_STATUS_PAYMENT_REQUIRED: 402,
      HTTP_STATUS_FORBIDDEN: 403,
      HTTP_STATUS_NOT_FOUND: 404,
      HTTP_STATUS_METHOD_NOT_ALLOWED: 405,
      HTTP_STATUS_NOT_ACCEPTABLE: 406,
      HTTP_STATUS_PROXY_AUTHENTICATION_REQUIRED: 407,
      HTTP_STATUS_REQUEST_TIMEOUT: 408,
      HTTP_STATUS_CONFLICT: 409,
      HTTP_STATUS_GONE: 410,
      HTTP_STATUS_LENGTH_REQUIRED: 411,
      HTTP_STATUS_PRECONDITION_FAILED: 412,
      HTTP_STATUS_PAYLOAD_TOO_LARGE: 413,
      HTTP_STATUS_URI_TOO_LONG: 414,
      HTTP_STATUS_UNSUPPORTED_MEDIA_TYPE: 415,
      HTTP_STATUS_RANGE_NOT_SATISFIABLE: 416,
      HTTP_STATUS_EXPECTATION_FAILED: 417,
      HTTP_STATUS_TEAPOT: 418,
      HTTP_STATUS_MISDIRECTED_REQUEST: 421,
      HTTP_STATUS_UNPROCESSABLE_ENTITY: 422,
      HTTP_STATUS_LOCKED: 423,
      HTTP_STATUS_FAILED_DEPENDENCY: 424,
      HTTP_STATUS_TOO_EARLY: 425,
      HTTP_STATUS_UPGRADE_REQUIRED: 426,
      HTTP_STATUS_PRECONDITION_REQUIRED: 428,
      HTTP_STATUS_TOO_MANY_REQUESTS: 429,
      HTTP_STATUS_REQUEST_HEADER_FIELDS_TOO_LARGE: 431,
      HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS: 451,
      HTTP_STATUS_INTERNAL_SERVER_ERROR: 500,
      HTTP_STATUS_NOT_IMPLEMENTED: 501,
      HTTP_STATUS_BAD_GATEWAY: 502,
      HTTP_STATUS_SERVICE_UNAVAILABLE: 503,
      HTTP_STATUS_GATEWAY_TIMEOUT: 504,
      HTTP_STATUS_HTTP_VERSION_NOT_SUPPORTED: 505,
      HTTP_STATUS_VARIANT_ALSO_NEGOTIATES: 506,
      HTTP_STATUS_INSUFFICIENT_STORAGE: 507,
      HTTP_STATUS_LOOP_DETECTED: 508,
      HTTP_STATUS_BANDWIDTH_LIMIT_EXCEEDED: 509,
      HTTP_STATUS_NOT_EXTENDED: 510,
      HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED: 511
    };
    const {
      NGHTTP2_SESSION_SERVER,
      NGHTTP2_SESSION_CLIENT,
      NGHTTP2_NO_ERROR,
      NGHTTP2_INTERNAL_ERROR,
      NGHTTP2_CANCEL,
      HTTP2_HEADER_STATUS,
      HTTP2_HEADER_METHOD,
      HTTP2_HEADER_AUTHORITY,
      HTTP2_HEADER_SCHEME,
      HTTP2_HEADER_PATH,
      HTTP2_HEADER_PROTOCOL,
      HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS,
      HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD,
      HTTP2_HEADER_AGE,
      HTTP2_HEADER_AUTHORIZATION,
      HTTP2_HEADER_CONNECTION,
      HTTP2_HEADER_CONTENT_ENCODING,
      HTTP2_HEADER_CONTENT_LENGTH,
      HTTP2_HEADER_CONTENT_TYPE,
      HTTP2_HEADER_COOKIE,
      HTTP2_HEADER_DATE,
      HTTP2_HEADER_ETAG,
      HTTP2_HEADER_HOST,
      HTTP2_HEADER_IF_MODIFIED_SINCE,
      HTTP2_HEADER_IF_NONE_MATCH,
      HTTP2_HEADER_IF_RANGE,
      HTTP2_HEADER_LAST_MODIFIED,
      HTTP2_HEADER_LOCATION,
      HTTP2_HEADER_RANGE,
      HTTP2_HEADER_REFERER,
      HTTP2_HEADER_SET_COOKIE,
      HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS,
      HTTP2_HEADER_USER_AGENT,
      HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS,
      HTTP2_HEADER_ORIGIN,
      HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE,
      HTTP2_HEADER_CONTENT_LANGUAGE,
      HTTP2_HEADER_CONTENT_LOCATION,
      HTTP2_HEADER_CONTENT_MD5,
      HTTP2_HEADER_CONTENT_RANGE,
      HTTP2_HEADER_DNT,
      HTTP2_HEADER_EXPIRES,
      HTTP2_HEADER_FROM,
      HTTP2_HEADER_IF_MATCH,
      HTTP2_HEADER_IF_UNMODIFIED_SINCE,
      HTTP2_HEADER_MAX_FORWARDS,
      HTTP2_HEADER_PROXY_AUTHORIZATION,
      HTTP2_HEADER_RETRY_AFTER,
      HTTP2_HEADER_TK,
      HTTP2_METHOD_CONNECT,
      HTTP2_METHOD_DELETE,
      HTTP2_METHOD_GET,
      HTTP2_METHOD_HEAD,
      HTTP_STATUS_CONTINUE,
      HTTP_STATUS_SWITCHING_PROTOCOLS,
      HTTP_STATUS_EARLY_HINTS,
      HTTP_STATUS_OK,
      HTTP_STATUS_NO_CONTENT,
      HTTP_STATUS_RESET_CONTENT,
      HTTP_STATUS_NOT_MODIFIED,
      HTTP_STATUS_METHOD_NOT_ALLOWED,
      HTTP_STATUS_EXPECTATION_FAILED
    } = constants;
    const kValidPseudoHeaders = new SafeSet([
      HTTP2_HEADER_STATUS,
      HTTP2_HEADER_METHOD,
      HTTP2_HEADER_AUTHORITY,
      HTTP2_HEADER_SCHEME,
      HTTP2_HEADER_PATH,
      HTTP2_HEADER_PROTOCOL
    ]);
    const kSingleValueHeaders = new SafeSet([
      HTTP2_HEADER_STATUS,
      HTTP2_HEADER_METHOD,
      HTTP2_HEADER_AUTHORITY,
      HTTP2_HEADER_SCHEME,
      HTTP2_HEADER_PATH,
      HTTP2_HEADER_PROTOCOL,
      HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS,
      HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE,
      HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD,
      HTTP2_HEADER_AGE,
      HTTP2_HEADER_AUTHORIZATION,
      HTTP2_HEADER_CONTENT_ENCODING,
      HTTP2_HEADER_CONTENT_LANGUAGE,
      HTTP2_HEADER_CONTENT_LENGTH,
      HTTP2_HEADER_CONTENT_LOCATION,
      HTTP2_HEADER_CONTENT_MD5,
      HTTP2_HEADER_CONTENT_RANGE,
      HTTP2_HEADER_CONTENT_TYPE,
      HTTP2_HEADER_DATE,
      HTTP2_HEADER_DNT,
      HTTP2_HEADER_ETAG,
      HTTP2_HEADER_EXPIRES,
      HTTP2_HEADER_FROM,
      HTTP2_HEADER_HOST,
      HTTP2_HEADER_IF_MATCH,
      HTTP2_HEADER_IF_MODIFIED_SINCE,
      HTTP2_HEADER_IF_NONE_MATCH,
      HTTP2_HEADER_IF_RANGE,
      HTTP2_HEADER_IF_UNMODIFIED_SINCE,
      HTTP2_HEADER_LAST_MODIFIED,
      HTTP2_HEADER_LOCATION,
      HTTP2_HEADER_MAX_FORWARDS,
      HTTP2_HEADER_PROXY_AUTHORIZATION,
      HTTP2_HEADER_RANGE,
      HTTP2_HEADER_REFERER,
      HTTP2_HEADER_RETRY_AFTER,
      HTTP2_HEADER_TK,
      HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS,
      HTTP2_HEADER_USER_AGENT,
      HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS
    ]);
    function assertValidPseudoHeader(key) {
      if (!kValidPseudoHeaders.has(key)) {
        throw codes.HTTP2_INVALID_PSEUDOHEADER(key);
      }
    }
    hideFromStack(assertValidPseudoHeader);
    function sessionName(type) {
      switch (type) {
        case NGHTTP2_SESSION_SERVER:
          return "server";
        case NGHTTP2_SESSION_CLIENT:
          return "client";
        default:
          return "<invalid>";
      }
    }
    function assertWithinRange(name, value, min = 0, max = 1 / 0) {
      if (value !== undefined && (value < min || value > max || !Number.isFinite(value))) {
        throwSettingRangeError(name, value);
      }
    }
    const kNghttp2ErrorMessages = {
      0: "Success",
      [-501]: "Invalid argument",
      [-502]: "Out of buffer space",
      [-505]: "Protocol error",
      [-510]: "Stream was already closed or invalid",
      [-522]: "The length of the frame is invalid",
      [-523]: "Header compression/decompression error",
      [-524]: "Flow control error",
      [-901]: "Out of memory",
      [-903]: "Received bad client magic byte string",
      [-904]: "Flooding was detected in this HTTP/2 session, and it must be closed"
    };

    class NghttpError extends Error {
      code;
      errno;
      constructor(integerCode, customErrorCode) {
        super(customErrorCode ? customErrorCode : kNghttp2ErrorMessages[integerCode] || "Unknown error code");
        this.code = customErrorCode || "ERR_HTTP2_ERROR";
        this.errno = integerCode;
      }
      toString() {
        return `${this.name} [${this.code}]: ${this.message}`;
      }
    }
    const NoPayloadMethods = new Set([HTTP2_METHOD_DELETE, HTTP2_METHOD_GET, HTTP2_METHOD_HEAD]);

    class Http2Session extends EventEmitter {
      [bunHTTP2SessionTeardownFrame] = kNoSessionTeardown;
      [bunHTTP2Socket];
      [bunHTTP2OriginSet] = undefined;
      [bunHTTP2AsyncContextFrame] = host.getAsyncContext();
      [kDeferWriteCallback] = setImmediate;
      get goawayCode() {
        return this[kGoawayCode] || NGHTTP2_NO_ERROR;
      }
      get goawayLastStreamID() {
        return this[kGoawayLastStreamID] || 0;
      }
      [EventEmitter.captureRejectionSymbol](err, event, ...args) {
        switch (event) {
          case "stream": {
            const stream = args[0];
            stream.destroy(err);
            break;
          }
          default:
            this.destroy(err);
        }
      }
    }
    function streamErrorFromCode(code) {
      if (code === 14) {
        return codes.HTTP2_MAX_PENDING_SETTINGS_ACK();
      }
      return codes.HTTP2_STREAM_ERROR(nameForErrorCode[code] || code);
    }
    hideFromStack(streamErrorFromCode);
    const { AsyncResource } = require("node:async_hooks");
    const kEmptyPingPayload = Buffer.alloc(8);
    function makeHttp2Ping(callback) {
      const resource = new AsyncResource("HTTP2PING");
      return function done(err, duration, payload) {
        if (err) {
          resource.runInAsyncScope(callback, null, err);
        } else {
          resource.runInAsyncScope(callback, null, null, duration, payload);
        }
        resource.emitDestroy();
      };
    }
    function cancelPendingPings(callbacks) {
      if (!callbacks)
        return;
      const err = codes.HTTP2_PING_CANCEL();
      for (let i = 0;i < callbacks.length; i++) {
        process.nextTick(callbacks[i][0], err);
      }
    }
    function createPendingStreamCancelError(cause) {
      let message = "The pending stream has been canceled";
      const cancelError = new Error(message);
      if (cause != null && typeof cause === "object") {
        cancelError.cause = cause;
        if (typeof cause.message === "string") {
          cancelError.message = `${message} (caused by: ${cause.message})`;
        }
      }
      cancelError.code = "ERR_HTTP2_STREAM_CANCEL";
      return cancelError;
    }
    function serverNativeSettings(options) {
      const merged = { ...options, ...options?.settings };
      if (merged.enablePush !== undefined)
        merged.enablePush = false;
      return merged;
    }
    function sessionErrorFromCode(code) {
      if (code === 14) {
        return codes.HTTP2_MAX_PENDING_SETTINGS_ACK();
      }
      return codes.HTTP2_SESSION_ERROR(code);
    }
    hideFromStack(sessionErrorFromCode);
    function assertSession(session) {
      if (!session) {
        throw codes.HTTP2_INVALID_SESSION();
      }
    }
    hideFromStack(assertSession);
    function validateWindowSize(windowSize) {
      if (typeof windowSize !== "number") {
        throw codes.INVALID_ARG_TYPE("windowSize", "number", windowSize);
      }
      if (!Number.isInteger(windowSize)) {
        throw codes.OUT_OF_RANGE("windowSize", "an integer", windowSize);
      }
      if (windowSize < 0 || windowSize > kMaxWindowSize) {
        throw codes.OUT_OF_RANGE("windowSize", `>= 0 && <= ${kMaxWindowSize}`, windowSize);
      }
    }
    hideFromStack(validateWindowSize);
    function pushToStream(stream, data) {
      if (data && stream[bunHTTP2StreamStatus] & 8 /* Closed */) {
        if (!stream._readableState.ended) {
          stream.resume();
          stream.push(null);
        }
        return;
      }
      const perf = stream[kPerfState];
      if (perf !== undefined && data !== null) {
        if (perf.firstByte === 0)
          perf.firstByte = performance.now() - perf.start;
        perf.bytesRead += data.length;
      }
      if (!stream.push(data) && data !== null) {
        streamOnPause.call(stream);
      }
    }
    var StreamState;
    ((StreamState) => {
      StreamState[StreamState["EndedCalled"] = 1] = "EndedCalled";
      StreamState[StreamState["WantTrailer"] = 2] = "WantTrailer";
      StreamState[StreamState["FinalCalled"] = 4] = "FinalCalled";
      StreamState[StreamState["Closed"] = 8] = "Closed";
      StreamState[StreamState["StreamResponded"] = 16] = "StreamResponded";
      StreamState[StreamState["WritableClosed"] = 32] = "WritableClosed";
      StreamState[StreamState["NativeClosed"] = 64] = "NativeClosed";
      StreamState[StreamState["EndStreamSent"] = 128] = "EndStreamSent";
      StreamState[StreamState["Delivered"] = 256] = "Delivered";
    })(StreamState ||= {});
    const kWriteFlushedWithoutCallback = 16;
    function deferWriteCallbackForSocket(nativeSocket) {
      return nativeSocket ? process.nextTick : setImmediate;
    }
    function isFinalWrite(stream, pendingLength) {
      return (stream._writableState.ending || stream[kEndingWithChunk] === true) && stream.writableLength === pendingLength && !stream[bunHTTP2WaitForTrailers];
    }
    function onEndStreamSettled(stream) {
      markWritableDone(stream);
      if ((stream.id & 1) === 0 && stream[bunHTTP2Session]?.type === constants.NGHTTP2_SESSION_SERVER) {
        if (!stream.rstCode)
          stream.rstCode = 0;
        markStreamClosed(stream);
      }
    }
    function markWritableDone(stream) {
      const _final = stream[bunHTTP2StreamFinal];
      if (typeof _final === "function") {
        stream[bunHTTP2StreamFinal] = null;
        _final();
        stream[bunHTTP2StreamStatus] |= 32 /* WritableClosed */ | 4 /* FinalCalled */;
        return;
      }
      stream[bunHTTP2StreamStatus] |= 32 /* WritableClosed */;
    }
    const kCloseChannelPublished = Symbol("closeChannelPublished");
    function publishStreamCloseChannel(stream) {
      if (stream[kCloseChannelPublished])
        return;
      stream[kCloseChannelPublished] = true;
      if (stream instanceof ClientHttp2Stream) {
        if (onClientStreamCloseChannel.hasSubscribers)
          onClientStreamCloseChannel.publish({ stream });
      } else if (onServerStreamCloseChannel.hasSubscribers) {
        onServerStreamCloseChannel.publish({ stream });
      }
    }
    const kEndingWithChunk = Symbol("http2EndingWithChunk");
    const kPerfStats = Symbol("http2PerfStats");
    const kPerfState = Symbol("http2PerfState");
    function initHttp2SessionPerf(session, type) {
      if (!hasObserver("http2"))
        return;
      session[kPerfStats] = {
        type,
        start: performance.now(),
        streamCount: 0,
        streamDurationSum: 0,
        closedStreams: 0,
        live: 0,
        maxConcurrentStreams: 0,
        pingRTT: 0,
        emitted: false
      };
    }
    function trackHttp2StreamStart(stream, session) {
      const stats = session?.[kPerfStats];
      if (stats === undefined)
        return;
      stats.streamCount++;
      stats.live++;
      if (stats.live > stats.maxConcurrentStreams)
        stats.maxConcurrentStreams = stats.live;
      stream[kPerfState] = {
        start: performance.now(),
        bytesRead: 0,
        bytesWritten: 0,
        firstHeader: 0,
        firstByte: 0,
        firstByteSent: 0
      };
    }
    function emitHttp2StreamPerf(stream) {
      const state = stream[kPerfState];
      if (state === undefined)
        return;
      stream[kPerfState] = undefined;
      const now = performance.now();
      const stats = stream[bunHTTP2Session]?.[kPerfStats];
      if (stats !== undefined) {
        stats.live--;
        stats.closedStreams++;
        stats.streamDurationSum += now - state.start;
      }
      enqueueNodeEntry(new PerformanceNodeEntry("Http2Stream", "http2", state.start, now - state.start, {
        id: typeof stream.id === "number" ? stream.id : 0,
        timeToFirstByte: state.firstByte,
        timeToFirstByteSent: state.firstByteSent,
        timeToFirstHeader: state.firstHeader,
        bytesWritten: state.bytesWritten,
        bytesRead: state.bytesRead
      }));
    }
    function captureHttp2PerfFrameSnapshot(session, parser) {
      const stats = session[kPerfStats];
      if (stats !== undefined && stats.frameSnapshot === undefined && parser) {
        stats.frameSnapshot = parser.getFrameCounters();
      }
    }
    function emitHttp2SessionPerf(session, parser, socket) {
      const stats = session[kPerfStats];
      if (stats === undefined || stats.emitted)
        return;
      stats.emitted = true;
      const now = performance.now();
      const counters = stats.frameSnapshot ?? parser?.getFrameCounters() ?? { framesReceived: 0, framesSent: 0 };
      enqueueNodeEntry(new PerformanceNodeEntry("Http2Session", "http2", stats.start, now - stats.start, {
        bytesWritten: socket?.bytesWritten ?? 0,
        bytesRead: socket?.bytesRead ?? 0,
        framesReceived: counters.framesReceived,
        framesSent: counters.framesSent,
        maxConcurrentStreams: stats.maxConcurrentStreams,
        pingRTT: stats.pingRTT,
        streamAverageDuration: stats.closedStreams > 0 ? stats.streamDurationSum / stats.closedStreams : 0,
        streamCount: stats.streamCount,
        type: stats.type
      }));
    }
    function markStreamClosed(stream) {
      const status = stream[bunHTTP2StreamStatus];
      if ((status & 8 /* Closed */) === 0) {
        stream[bunHTTP2StreamStatus] = status | 8 /* Closed */;
        publishStreamCloseChannel(stream);
        markWritableDone(stream);
      }
    }
    function rstNextTick(id, rstCode) {
      const session = this;
      session[bunHTTP2Native]?.rstStream(id, rstCode);
    }
    function streamOnPause() {
      const session = this[bunHTTP2Session];
      const id = this.id;
      if (session && id)
        session[bunHTTP2Native]?.setStreamReading(id, false);
    }
    function streamOnResume() {
      const session = this[bunHTTP2Session];
      const id = this.id;
      if (session && id)
        session[bunHTTP2Native]?.setStreamReading(id, true);
    }
    function sendRstOnReady(session, code) {
      setImmediate(rstNextTick.bind(session, this.id, code));
    }
    function uncorkNT(stream) {
      stream.uncork();
    }
    function abortRequestStream(signal) {
      this.destroy(makeAbortError(undefined, { cause: signal.reason }));
    }
    function setupRequestEndAndSignal(req, options, signal) {
      if (options?.endStream)
        req.end();
      if (signal) {
        addAbortListener ??= require("internal/abort_listener").addAbortListener;
        const disposable = addAbortListener(signal, abortRequestStream.bind(req, signal));
        req.once("close", disposable[Symbol.dispose]);
      }
    }
    function destroyStreamForSessionDestroy(error, rstCode, stream) {
      if (stream.destroyed)
        return;
      if (rstCode && !stream.rstCode)
        stream.rstCode = rstCode;
      if (error === undefined && (stream[bunHTTP2StreamStatus] & 64 /* NativeClosed */) !== 0 && stream.readable && stream.readableFlowing !== null) {
        stream.push(null);
        stream.once("end", destroySelfOnEnd);
        return;
      }
      stream.destroy(error !== undefined && stream.listenerCount("error") > 0 ? error : undefined);
    }

    class Http2Stream extends Duplex {
      #id;
      [bunHTTP2Session] = null;
      [bunHTTP2StreamFinal] = null;
      [bunHTTP2StreamStatus] = 0;
      [bunHTTP2AsyncContextFrame] = host.getAsyncContext();
      rstCode = undefined;
      [bunHTTP2Headers];
      [kInfoHeaders];
      #sentTrailers;
      [kSendingTrailers] = false;
      [kAborted] = false;
      [kHeadRequest] = false;
      constructor(streamId, session, headers) {
        super({
          decodeStrings: false,
          autoDestroy: false
        });
        this.#id = streamId;
        this[bunHTTP2Session] = session;
        this[bunHTTP2Headers] = headers;
        trackHttp2StreamStart(this, session);
        this.on("pause", streamOnPause);
        this.on("resume", streamOnResume);
      }
      get scheme() {
        const headers = this[bunHTTP2Headers];
        if (headers)
          return headers[":scheme"] || "https";
        return "https";
      }
      get id() {
        return this.#id;
      }
      get pending() {
        return !this.#id;
      }
      [kSetStreamId](id) {
        this.#id = id;
      }
      [kInspect](depth, opts) {
        if (typeof depth === "number" && depth < 0)
          return this;
        const obj = {
          id: this.#id || "<pending>",
          closed: this.closed,
          destroyed: this.destroyed,
          state: this.state,
          readableState: this._readableState,
          writableState: this._writableState
        };
        util ??= require("node:util");
        return `Http2Stream ${util.inspect(obj, opts)}`;
      }
      get bufferSize() {
        return this.writableLength;
      }
      get sentHeaders() {
        return this[bunHTTP2Headers];
      }
      get sentInfoHeaders() {
        return this[kInfoHeaders] || [];
      }
      get sentTrailers() {
        return this.#sentTrailers;
      }
      get headRequest() {
        return !!this[kHeadRequest];
      }
      sendTrailers(headers) {
        const session = this[bunHTTP2Session];
        if (this.destroyed || this.closed) {
          throw codes.HTTP2_INVALID_STREAM();
        }
        if (this.#sentTrailers) {
          throw codes.HTTP2_TRAILERS_ALREADY_SENT();
        }
        assertSession(session);
        if ((this[bunHTTP2StreamStatus] & 2 /* WantTrailer */) === 0) {
          throw codes.HTTP2_TRAILERS_NOT_READY();
        }
        if (headers == undefined) {
          headers = {};
        } else if (!isObject(headers) || Array.isArray(headers)) {
          throw codes.INVALID_ARG_TYPE("headers", "object", headers);
        } else {
          headers = { ...headers };
        }
        const sensitives = headers[sensitiveHeaders];
        if (sensitives !== undefined && !Array.isArray(sensitives)) {
          throw codes.INVALID_ARG_VALUE("headers[http2.neverIndex]", sensitives);
        }
        const sensitiveNames = buildSensitiveNames(headers, sensitives);
        if (sensitives !== undefined)
          headers[sensitiveHeaders] = sensitives;
        this.#sentTrailers = headers;
        this[kSendingTrailers] = true;
        try {
          if (ObjectKeys(headers).length === 0) {
            session[bunHTTP2Native]?.noTrailers(this.#id);
          } else {
            session[bunHTTP2Native]?.sendTrailers(this.#id, headers, sensitiveNames);
          }
        } catch (error) {
          this.#sentTrailers = undefined;
          throw error;
        } finally {
          this[kSendingTrailers] = false;
        }
      }
      setTimeout(timeout, callback) {
        const session = this[bunHTTP2Session];
        if (!session)
          return;
        session.setTimeout(timeout, callback);
      }
      get closed() {
        return (this[bunHTTP2StreamStatus] & 8 /* Closed */) !== 0;
      }
      get destroyed() {
        return this[bunHTTP2Session] === null || this._readableState !== undefined && this._writableState !== undefined && this._readableState.destroyed === true && this._writableState.destroyed === true;
      }
      set destroyed(value) {
        if (this._readableState !== undefined && this._writableState !== undefined) {
          this._readableState.destroyed = value;
          this._writableState.destroyed = value;
        }
      }
      get state() {
        const session = this[bunHTTP2Session];
        if (session && !session.destroyed && typeof this.#id === "number") {
          return session[bunHTTP2Native]?.getStreamState(this.#id) ?? {};
        }
        return {};
      }
      priority(_options) {
        if (!priorityDeprecationWarned) {
          priorityDeprecationWarned = true;
          process.emitWarning("http2Stream.priority is longer supported after priority signalling was deprecated in RFC 9113", "DeprecationWarning", "DEP0194");
        }
        if (this.destroyed || this.session === undefined) {
          throw codes.HTTP2_INVALID_STREAM();
        }
        return true;
      }
      get endAfterHeaders() {
        const session = this[bunHTTP2Session];
        if (session) {
          return session[bunHTTP2Native]?.getEndAfterHeaders(this.#id) || false;
        }
        return false;
      }
      get aborted() {
        return this[kAborted] || false;
      }
      get session() {
        const session = this[bunHTTP2Session];
        if (session == null || session.destroyed)
          return;
        return session;
      }
      get pushAllowed() {
        const session = this[bunHTTP2Session];
        return session != null && session.type === 0 && !!session.remoteSettings?.enablePush && !this.destroyed && !this.closed;
      }
      close(code, callback) {
        if ((this[bunHTTP2StreamStatus] & 8 /* Closed */) === 0) {
          const session = this[bunHTTP2Session];
          assertSession(session);
          if (code === undefined)
            code = 0;
          validateInteger(code, "code");
          if (code < 0 || code > kMaxInt) {
            throw codes.OUT_OF_RANGE("code", `>= 0 && <= ${kMaxInt}`, code);
          }
          if (typeof callback !== "undefined") {
            validateFunction(callback, "callback");
            this.once("close", callback);
          }
          this.push(null);
          const { ending } = this._writableState;
          if (!ending) {
            if (!this.aborted) {
              this[kAborted] = true;
              this.emit("aborted");
            }
            this.end();
          }
          this.rstCode = code;
          markStreamClosed(this);
          if (this.pending) {
            this.once("ready", sendRstOnReady.bind(this, session, code));
          } else if (this.writableFinished || code) {
            setImmediate(rstNextTick.bind(session, this.#id, code));
          } else {
            this.once("finish", rstNextTick.bind(session, this.#id, code));
          }
          if (this.writableFinished) {
            scheduleDestroyIfNotDestroyed(this);
          } else {
            this.once("finish", scheduleDestroyIfNotDestroyed.bind(null, this));
          }
        }
      }
      _destroy(err, callback) {
        this[bunHTTP2AsyncContextFrame] = undefined;
        const { ending } = this._writableState;
        this.push(null);
        if (!ending && !this[kPush]) {
          if (!this.aborted) {
            this[kAborted] = true;
            this.emit("aborted");
          }
          this._writableState.destroyed = false;
          this.end();
          this._writableState.destroyed = true;
        }
        const session = this[bunHTTP2Session];
        assertSession(session);
        let rstCode = this.rstCode;
        if (!rstCode) {
          if (err != null) {
            if (err.code === "ABORT_ERR") {
              rstCode = NGHTTP2_CANCEL;
            } else {
              rstCode = NGHTTP2_INTERNAL_ERROR;
            }
          } else {
            rstCode = this.rstCode = 0;
          }
        }
        this.rstCode = rstCode;
        emitHttp2StreamPerf(this);
        markStreamClosed(this);
        if (err == null && rstCode !== NGHTTP2_NO_ERROR && rstCode !== NGHTTP2_CANCEL) {
          err = session?.[kSessionDestroyError] ?? codes.HTTP2_STREAM_ERROR(nameForErrorCode[rstCode] || rstCode);
        }
        if (err != null && this instanceof ServerHttp2Stream && (this[bunHTTP2StreamStatus] & 256 /* Delivered */) === 0) {
          err = null;
        }
        this[bunHTTP2Session] = null;
        if (session && typeof this.#id === "number" && !this[kNeverAnnounced] && (rstCode !== 0 || (this[bunHTTP2StreamStatus] & 64 /* NativeClosed */) === 0)) {
          setImmediate(rstNextTick.bind(session, this.#id, rstCode));
        }
        if (err != null) {
          if (this instanceof ClientHttp2Stream) {
            if (onClientStreamErrorChannel.hasSubscribers)
              onClientStreamErrorChannel.publish({ stream: this, error: err });
          } else if (onServerStreamErrorChannel.hasSubscribers) {
            onServerStreamErrorChannel.publish({ stream: this, error: err });
          }
        }
        callback(err);
      }
      _final(callback) {
        if (this.pending) {
          this.once("ready", this._final.bind(this, callback));
          return;
        }
        const status = this[bunHTTP2StreamStatus];
        if (onClientStreamBodySentChannel.hasSubscribers && this instanceof ClientHttp2Stream) {
          onClientStreamBodySentChannel.publish({ stream: this });
        }
        if ((status & 128 /* EndStreamSent */) !== 0) {
          this[bunHTTP2StreamStatus] = status | 4 /* FinalCalled */;
          if ((status & (32 /* WritableClosed */ | 8 /* Closed */)) !== 0) {
            callback();
          } else {
            this[bunHTTP2StreamFinal] = callback;
          }
          return;
        }
        const session = this[bunHTTP2Session];
        if (session) {
          const native = session[bunHTTP2Native];
          if (native) {
            if (this instanceof ServerHttp2Stream && !this.headersSent && (this.id & 1) === 0) {
              this[bunHTTP2StreamFinal] = callback;
              return;
            }
            this[bunHTTP2StreamStatus] |= 4 /* FinalCalled */;
            if (this[bunHTTP2WaitForTrailers]) {
              this[bunHTTP2WaitForTrailers] = false;
              if ((this[bunHTTP2StreamStatus] & 2 /* WantTrailer */) === 0) {
                this[bunHTTP2StreamStatus] |= 2 /* WantTrailer */;
                this[bunHTTP2StreamFinal] = callback;
                if ((this[bunHTTP2StreamStatus] & 8 /* Closed */) !== 0 || this.listenerCount("wantTrailers") === 0) {
                  native.noTrailers(this.#id);
                  this.#sentTrailers = {};
                } else {
                  this.emit("wantTrailers");
                }
                native.flush();
                if (this[bunHTTP2StreamFinal] !== callback) {
                  return;
                }
                this[bunHTTP2StreamFinal] = null;
              }
              callback();
              return;
            }
            const settled = native.writeStream(this.#id, "", "ascii", true, callback);
            native.flush();
            if (settled === 5) {
              markWritableDone(this);
              if ((this.#id & 1) === 0 && this[bunHTTP2Session]?.type === constants.NGHTTP2_SESSION_SERVER) {
                if (!this.rstCode)
                  this.rstCode = 0;
                markStreamClosed(this);
              }
            }
            return;
          }
        }
        if ((status & 32 /* WritableClosed */) !== 0 || (status & 8 /* Closed */) !== 0) {
          callback();
          this[bunHTTP2StreamStatus] |= 4 /* FinalCalled */;
        } else {
          this[bunHTTP2StreamFinal] = callback;
        }
      }
      _read(_size) {
        if (this.destroyed) {
          this.push(null);
          return;
        }
        if (!this.pending) {
          streamOnResume.call(this);
        } else {
          this.once("ready", streamOnResume);
        }
      }
      end(chunk, encoding, callback) {
        const status = this[bunHTTP2StreamStatus];
        if (typeof callback === "undefined") {
          if (typeof chunk === "function") {
            callback = chunk;
            chunk = undefined;
          } else if (typeof encoding === "function") {
            callback = encoding;
            encoding = undefined;
          }
        }
        if ((status & 1 /* EndedCalled */) !== 0) {
          typeof callback == "function" && callback();
          return this;
        }
        this[bunHTTP2StreamStatus] = status | 1 /* EndedCalled */;
        const hasChunk = chunk !== undefined && chunk !== null && chunk.length > 0;
        if (hasChunk)
          this[kEndingWithChunk] = true;
        try {
          return super.end(chunk, encoding, callback);
        } finally {
          if (hasChunk)
            this[kEndingWithChunk] = false;
        }
      }
      _writev(data, callback) {
        if (this.pending) {
          this.once("ready", this._writev.bind(this, data, callback));
          return;
        }
        const writevPerf = this[kPerfState];
        if (writevPerf !== undefined) {
          if (writevPerf.firstByteSent === 0)
            writevPerf.firstByteSent = performance.now() - writevPerf.start;
          for (let i = 0;i < data.length; i++) {
            const { chunk, encoding } = data[i];
            writevPerf.bytesWritten += typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
          }
        }
        const session = this[bunHTTP2Session];
        if (session) {
          const native = session[bunHTTP2Native];
          if (native) {
            let batchLength = 0;
            for (let i = 0;i < data.length; i++) {
              batchLength += data[i].chunk.length;
            }
            const allBuffers = data.allBuffers;
            let chunks;
            if (allBuffers) {
              chunks = data;
              for (let i = 0;i < data.length; i++) {
                data[i] = data[i].chunk;
              }
            } else {
              chunks = new Array(data.length);
              for (let i = 0;i < data.length; i++) {
                const { chunk, encoding } = data[i];
                if (typeof chunk === "string") {
                  chunks[i] = Buffer.from(chunk, encoding);
                } else {
                  chunks[i] = chunk;
                }
              }
            }
            const chunk = Buffer.concat(chunks || []);
            if (session[kTimeout])
              session[kTimeout].refresh();
            const endStream = isFinalWrite(this, batchLength);
            const status = native.writeStream(this.#id, chunk, undefined, endStream, callback, true);
            if (status & kWriteFlushedWithoutCallback)
              session[kDeferWriteCallback](callback);
            if (endStream) {
              this[bunHTTP2StreamStatus] |= 128 /* EndStreamSent */;
              if ((status & ~kWriteFlushedWithoutCallback) === 5)
                onEndStreamSettled(this);
            }
            if (onClientStreamBodyChunkSentChannel.hasSubscribers && this instanceof ClientHttp2Stream) {
              onClientStreamBodyChunkSentChannel.publish({ stream: this, writev: true, data, encoding: "" });
            }
            return;
          }
        }
        if (typeof callback == "function") {
          callback();
        }
      }
      _write(chunk, encoding, callback) {
        if (this.pending) {
          this.once("ready", this._write.bind(this, chunk, encoding, callback));
          return;
        }
        const writePerf = this[kPerfState];
        if (writePerf !== undefined) {
          if (writePerf.firstByteSent === 0)
            writePerf.firstByteSent = performance.now() - writePerf.start;
          writePerf.bytesWritten += typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
        }
        const session = this[bunHTTP2Session];
        if (session) {
          const native = session[bunHTTP2Native];
          if (native) {
            let wireChunk = chunk;
            let wireEncoding = encoding;
            if (typeof chunk === "string" && (encoding === "utf-16le" || encoding === "utf16le" || encoding === "ucs-2")) {
              wireChunk = Buffer.from(chunk, encoding);
              wireEncoding = undefined;
            }
            if (session[kTimeout])
              session[kTimeout].refresh();
            const endStream = isFinalWrite(this, chunk.length);
            const status = native.writeStream(this.#id, wireChunk, wireEncoding, endStream, callback, true);
            if (status & kWriteFlushedWithoutCallback)
              session[kDeferWriteCallback](callback);
            if (endStream) {
              this[bunHTTP2StreamStatus] |= 128 /* EndStreamSent */;
              if ((status & ~kWriteFlushedWithoutCallback) === 5)
                onEndStreamSettled(this);
            }
            if (onClientStreamBodyChunkSentChannel.hasSubscribers && this instanceof ClientHttp2Stream) {
              onClientStreamBodyChunkSentChannel.publish({ stream: this, writev: false, data: chunk, encoding });
            }
            return;
          }
        }
        if (typeof callback == "function") {
          callback();
        }
      }
      [EventEmitter.captureRejectionSymbol](err, event, ...args) {
        switch (event) {
          case "stream": {
            const stream = args[0];
            stream.destroy(err);
            break;
          }
          default:
            this.destroy(err);
        }
      }
    }

    class ClientHttp2Stream extends Http2Stream {
    }
    function withStreamFrame(handler) {
      return function(self, stream, a, b, c) {
        if (typeof stream !== "object" || stream === null)
          return handler(self, stream, a, b, c);
        const teardownFrame = self != null ? self[bunHTTP2SessionTeardownFrame] : kNoSessionTeardown;
        const frame = teardownFrame !== kNoSessionTeardown ? teardownFrame : stream[bunHTTP2AsyncContextFrame];
        return runInFrame(frame, handler, undefined, self, stream, a, b, c);
      };
    }
    function tryClose(fd) {
      try {
        fs.close(fd);
      } catch {}
    }
    function doSendFileFD(options, fd, headers, err, stat) {
      const onError = options.onError;
      const ownsFd = this[kOwnsFd] === true;
      if (err) {
        if (ownsFd && err.code !== "EBADF") {
          tryClose(fd);
        }
        if (onError)
          onError(err);
        else {
          this.respond(headers, options);
          this.destroy(streamErrorFromCode(NGHTTP2_INTERNAL_ERROR));
        }
        return;
      }
      if (!stat.isFile()) {
        const isDirectory = stat.isDirectory();
        if (options.offset !== undefined || options.offset > 0 || options.length !== undefined || options.length >= 0 || isDirectory) {
          const err = isDirectory ? codes.HTTP2_SEND_FILE() : codes.HTTP2_SEND_FILE_NOSEEK();
          if (ownsFd)
            tryClose(fd);
          if (onError)
            onError(err);
          else {
            this.respond(headers, options);
            this.destroy(err);
          }
          return;
        }
        options.offset = 0;
        options.length = -1;
      }
      if (this.destroyed || this.closed) {
        if (ownsFd)
          tryClose(fd);
        this.destroy(codes.HTTP2_INVALID_STREAM());
        return;
      }
      const statOptions = {
        offset: options.offset !== undefined ? options.offset : 0,
        length: options.length !== undefined ? options.length : -1
      };
      if (statOptions.offset <= 0) {
        statOptions.offset = 0;
      }
      if (statOptions.length <= 0) {
        if (stat.isFile()) {
          statOptions.length = stat.size;
        } else {
          statOptions.length = undefined;
        }
      }
      if (typeof options.statCheck === "function" && options.statCheck.call(this, stat, headers, options) === false || this.headersSent) {
        if (this[kOwnsFd] === true)
          tryClose(fd);
        return;
      }
      if (stat.isFile()) {
        statOptions.length = statOptions.length < 0 ? stat.size - +statOptions.offset : Math.min(stat.size - +statOptions.offset, statOptions.length);
        for (let i in headers) {
          if (i?.toLowerCase() === HTTP2_HEADER_CONTENT_LENGTH) {
            delete headers[i];
          }
        }
        headers[HTTP2_HEADER_CONTENT_LENGTH] = statOptions.length;
      }
      try {
        this.respond(headers, options);
      } catch (err) {
        if (this[kOwnsFd] === true)
          tryClose(fd);
        if (typeof onError === "function") {
          onError(err);
        } else {
          this.destroy(err);
        }
        return;
      }
      const finishNativeStream = closeWritableForFileResponse(this);
      const stream = this;
      const fileStream = fs.createReadStream(null, {
        fd,
        autoClose: this[kOwnsFd] === true,
        start: statOptions.offset ? statOptions.offset : undefined,
        end: typeof statOptions.length === "number" ? statOptions.length + (statOptions.offset || 0) - 1 : undefined,
        emitClose: false
      });
      const sink = new Stream.Writable({
        write(chunk, _encoding, cb) {
          const native = stream[bunHTTP2Session]?.[bunHTTP2Native];
          if (!native || stream.destroyed || stream.closed) {
            cb();
            return;
          }
          const status = native.writeStream(stream.id, chunk, undefined, false, cb, true);
          if (status & kWriteFlushedWithoutCallback)
            process.nextTick(cb);
        },
        final(cb) {
          if (stream.destroyed || stream.closed) {
            cb();
            return;
          }
          finishNativeStream(cb);
        }
      });
      fileStream.on("error", onFileStreamError.bind(this));
      this.once("close", () => {
        fileStream.unpipe(sink);
        if (ownsFd)
          fileStream.destroy();
        else
          fileStream.pause();
      });
      fileStream.pipe(sink);
    }
    function onFileStreamError() {
      if (!this.destroyed && !this.closed)
        this.close(NGHTTP2_INTERNAL_ERROR);
    }
    const kFileResponseFinal = Symbol("fileResponseFinal");
    function closeWritableForFileResponse(stream) {
      let final = stream[kFileResponseFinal];
      if (final === undefined) {
        final = stream._final.bind(stream);
        stream[kFileResponseFinal] = final;
        stream._final = null;
        stream.end();
      }
      return final;
    }
    function afterOpen(options, headers, err, fd) {
      const onError = options.onError;
      if (err) {
        tryClose(fd);
        if (onError)
          onError(err);
        else
          this.destroy(err);
        return;
      }
      if (this.destroyed || this.closed) {
        tryClose(fd);
        return;
      }
      fs.fstat(fd, doSendFileFD.bind(this, options, fd, headers));
    }
    function maybeCloseUnreadServerStream(stream) {
      if (stream.destroyed || stream.closed)
        return;
      if (!stream.writableFinished || !stream.headersSent)
        return;
      if (stream[kFileResponseFinal] !== undefined)
        return;
      if (stream[kSendingTrailers])
        return;
      if ((stream[bunHTTP2StreamStatus] & 2 /* WantTrailer */) !== 0 && stream.sentTrailers === undefined)
        return;
      if (!stream.readableDidRead && stream.readableFlowing === null) {
        setImmediate(callStreamClose, stream);
      }
    }
    function serverStreamOnFinish() {
      if (this.readableLength > 0)
        maybeCloseUnreadServerStream(this);
    }
    function callStreamClose(stream) {
      if (!stream.destroyed && !stream.closed)
        stream.close();
    }

    class ServerHttp2Stream extends Http2Stream {
      headersSent = false;
      constructor(streamId, session, headers) {
        super(streamId, session, headers);
      }
      _final(callback) {
        this.once("finish", serverStreamOnFinish);
        super._final(callback);
      }
      _write(chunk, encoding, callback) {
        if (!this.headersSent && !this.destroyed && !this.closed && this.session !== undefined) {
          this.respond();
        }
        super._write(chunk, encoding, callback);
      }
      _writev(data, callback) {
        if (!this.headersSent && !this.destroyed && !this.closed && this.session !== undefined) {
          this.respond();
        }
        super._writev(data, callback);
      }
      pushStream(headers, options, callback) {
        if (typeof options === "function") {
          callback = options;
          options = undefined;
        }
        {
          const session = this[bunHTTP2Session];
          if (session == null || session.destroyed) {
            throw codes.HTTP2_PUSH_DISABLED();
          }
        }
        if (typeof callback !== "function") {
          throw codes.INVALID_ARG_TYPE("callback", "function", callback);
        }
        if ((this.id & 1) === 0) {
          const err = new Error("A push stream cannot initiate another push stream.");
          err.code = "ERR_HTTP2_NESTED_PUSH";
          throw err;
        }
        if (!this.pushAllowed) {
          throw codes.HTTP2_PUSH_DISABLED();
        }
        const session = this[bunHTTP2Session];
        const parser = session?.[bunHTTP2Native];
        if (!parser) {
          throw codes.HTTP2_INVALID_STREAM();
        }
        headers = { ...headers };
        assertNoConnectionHeaders(headers);
        if (session[kStrictSingleValueFields] !== false)
          assertSingleValueHeaders(headers);
        const sensitives = headers[sensitiveHeaders];
        if (sensitives !== undefined && !Array.isArray(sensitives)) {
          throw codes.INVALID_ARG_VALUE("headers[http2.neverIndex]", sensitives);
        }
        const sensitiveNames = buildSensitiveNames(headers, sensitives);
        const parentRequestHeaders = this[kRequestHeaders] || this[bunHTTP2Headers];
        if (headers[HTTP2_HEADER_METHOD] === undefined)
          headers[HTTP2_HEADER_METHOD] = "GET";
        if (headers[HTTP2_HEADER_PATH] === undefined)
          headers[HTTP2_HEADER_PATH] = "/";
        if (headers[HTTP2_HEADER_SCHEME] === undefined) {
          headers[HTTP2_HEADER_SCHEME] = parentRequestHeaders?.[HTTP2_HEADER_SCHEME] || this.scheme;
        }
        if (headers[HTTP2_HEADER_AUTHORITY] === undefined && parentRequestHeaders) {
          headers[HTTP2_HEADER_AUTHORITY] = parentRequestHeaders[HTTP2_HEADER_AUTHORITY];
        }
        const pushId = parser.getNextStream();
        if (pushId === -1) {
          throw codes.HTTP2_OUT_OF_STREAMS();
        }
        const pushedStream = parser.getStreamContext(pushId);
        if (pushedStream && pushedStream[bunHTTP2Headers] == null) {
          pushedStream[bunHTTP2Headers] = headers;
        }
        if (onServerStreamCreatedChannel.hasSubscribers) {
          onServerStreamCreatedChannel.publish({ stream: pushedStream, headers });
        }
        if (onServerStreamStartChannel.hasSubscribers) {
          onServerStreamStartChannel.publish({ stream: pushedStream, headers });
        }
        let pushResult;
        try {
          pushResult = parser.pushPromise(this.id, pushId, headers, sensitiveNames);
        } catch (err) {
          if (pushedStream && !pushedStream.destroyed) {
            pushedStream[kNeverAnnounced] = true;
            session[kReleaseUnannouncedStream](pushId);
            pushedStream.destroy();
          }
          process.nextTick(callback, err);
          return;
        }
        if (pushResult === -1) {
          if (pushedStream) {
            pushedStream[kNeverAnnounced] = true;
            pushedStream[bunHTTP2StreamStatus] |= 256 /* Delivered */;
          }
          process.nextTick(callback, null, pushedStream, headers);
          return;
        }
        if (pushedStream) {
          pushedStream[bunHTTP2StreamStatus] |= 256 /* Delivered */;
          if (headers[HTTP2_HEADER_METHOD] === HTTP2_METHOD_HEAD) {
            pushedStream[kHeadRequest] = true;
            pushedStream.end();
          } else if (options?.endStream) {
            pushedStream.end();
          }
        }
        process.nextTick(callback, null, pushedStream, headers);
      }
      respondWithFile(path, headers, options) {
        if (this.destroyed) {
          throw codes.HTTP2_INVALID_STREAM();
        }
        if (this.headersSent)
          throw codes.HTTP2_HEADERS_SENT();
        if (Array.isArray(headers)) {
          throw codes.INVALID_ARG_TYPE("headers", ["Array", "Object"], headers);
        }
        if (headers == undefined) {
          headers = {};
        } else if (!isObject(headers)) {
          throw codes.INVALID_ARG_TYPE("headers", "object", headers);
        } else {
          headers = { ...headers };
        }
        if (headers[HTTP2_HEADER_STATUS] === undefined) {
          headers[HTTP2_HEADER_STATUS] = 200;
        }
        const statusCode = headers[HTTP2_HEADER_STATUS];
        options = { ...options };
        if (statusCode === HTTP_STATUS_NO_CONTENT || statusCode === HTTP_STATUS_RESET_CONTENT || statusCode === HTTP_STATUS_NOT_MODIFIED || this.headRequest) {
          throw codes.HTTP2_PAYLOAD_FORBIDDEN(statusCode);
        }
        if (options.offset !== undefined && typeof options.offset !== "number") {
          throw codes.INVALID_ARG_VALUE("options.offset", options.offset);
        }
        if (options.length !== undefined && typeof options.length !== "number") {
          throw codes.INVALID_ARG_VALUE("options.length", options.length);
        }
        if (options.statCheck !== undefined && typeof options.statCheck !== "function") {
          throw codes.INVALID_ARG_VALUE("options.statCheck", options.statCheck);
        }
        this[kOwnsFd] = true;
        fs.open(path, "r", afterOpen.bind(this, options || {}, headers));
      }
      respondWithFD(fd, headers, options) {
        if (typeof fd !== "number") {
          if (fd !== null && typeof fd === "object" && typeof fd.fd === "number") {
            fd = fd.fd;
          } else {
            const err = new TypeError(`The "fd" argument must be of type number or an instance of FileHandle.` + ` Received ${receivedValueLabel(fd)}`);
            err.code = "ERR_INVALID_ARG_TYPE";
            throw err;
          }
        }
        if (this.destroyed) {
          throw codes.HTTP2_INVALID_STREAM();
        }
        if (this.headersSent)
          throw codes.HTTP2_HEADERS_SENT();
        if (Array.isArray(headers)) {
          throw codes.INVALID_ARG_TYPE("headers", ["Array", "Object"], headers);
        }
        if (headers == undefined) {
          headers = {};
        } else if (!isObject(headers)) {
          throw codes.INVALID_ARG_TYPE("headers", "object", headers);
        } else {
          headers = { ...headers };
        }
        if (headers[HTTP2_HEADER_STATUS] === undefined) {
          headers[HTTP2_HEADER_STATUS] = 200;
        }
        const statusCode = headers[HTTP2_HEADER_STATUS];
        if (statusCode === HTTP_STATUS_NO_CONTENT || statusCode === HTTP_STATUS_RESET_CONTENT || statusCode === HTTP_STATUS_NOT_MODIFIED || this.headRequest) {
          throw codes.HTTP2_PAYLOAD_FORBIDDEN(statusCode);
        }
        options = { ...options };
        if (options.offset !== undefined && typeof options.offset !== "number") {
          throw codes.INVALID_ARG_VALUE("options.offset", options.offset);
        }
        if (options.length !== undefined && typeof options.length !== "number") {
          throw codes.INVALID_ARG_VALUE("options.length", options.length);
        }
        if (options.statCheck !== undefined && typeof options.statCheck !== "function") {
          throw codes.INVALID_ARG_VALUE("options.statCheck", options.statCheck);
        }
        this[kOwnsFd] = false;
        if (options.statCheck === undefined) {
          closeWritableForFileResponse(this);
        }
        if (fd instanceof FileHandle) {
          fs.fstat(fd.fd, doSendFileFD.bind(this, options, fd, headers));
        } else {
          fs.fstat(fd, doSendFileFD.bind(this, options, fd, headers));
        }
      }
      additionalHeaders(headers) {
        if (this.destroyed || this.closed || this.session === undefined) {
          throw codes.HTTP2_INVALID_STREAM();
        }
        if (this.sentTrailers) {
          throw codes.HTTP2_TRAILERS_ALREADY_SENT();
        }
        if (this.headersSent) {
          throw codes.HTTP2_HEADERS_AFTER_RESPOND();
        }
        if (headers == undefined) {
          headers = {};
        } else if (!isObject(headers) || Array.isArray(headers)) {
          throw codes.INVALID_ARG_TYPE("headers", "object", headers);
        } else {
          headers = { ...headers };
        }
        for (const name in headers) {
          if (name.startsWith(":") && name !== HTTP2_HEADER_STATUS) {
            throw codes.HTTP2_INVALID_PSEUDOHEADER(name);
          }
        }
        const sensitives = headers[sensitiveHeaders];
        if (sensitives !== undefined && !Array.isArray(sensitives)) {
          throw codes.INVALID_ARG_VALUE("headers[http2.neverIndex]", sensitives);
        }
        const sensitiveNames = buildSensitiveNames(headers, sensitives);
        if (this[bunHTTP2Session]?.[kStrictSingleValueFields] !== false)
          assertSingleValueHeaders(headers);
        let hasStatus = true;
        if (headers[HTTP2_HEADER_STATUS] === undefined) {
          headers[HTTP2_HEADER_STATUS] = 200;
          hasStatus = false;
        }
        const statusCode = headers[HTTP2_HEADER_STATUS];
        if (hasStatus) {
          if (statusCode === HTTP_STATUS_SWITCHING_PROTOCOLS)
            throw codes.HTTP2_STATUS_101();
          if (statusCode < 100 || statusCode >= 200) {
            throw codes.HTTP2_INVALID_INFO_STATUS(statusCode);
          }
          if (statusCode === HTTP_STATUS_NO_CONTENT || statusCode === HTTP_STATUS_RESET_CONTENT || statusCode === HTTP_STATUS_NOT_MODIFIED || this.headRequest) {
            throw codes.HTTP2_PAYLOAD_FORBIDDEN(statusCode);
          }
        }
        const session = this[bunHTTP2Session];
        assertSession(session);
        if (!this[kInfoHeaders]) {
          this[kInfoHeaders] = [headers];
        } else {
          ArrayPrototypePush.call(this[kInfoHeaders], headers);
        }
        session[bunHTTP2Native]?.request(this.id, undefined, headers, sensitiveNames);
      }
      respond(headers, options) {
        if (this.destroyed || this.session === undefined) {
          throw codes.HTTP2_INVALID_STREAM();
        }
        const session = this[bunHTTP2Session];
        assertSession(session);
        if (this.headersSent)
          throw codes.HTTP2_HEADERS_SENT();
        if (this.sentTrailers) {
          throw codes.HTTP2_TRAILERS_ALREADY_SENT();
        }
        let rawHeadersList = null;
        let statusCode;
        if (headers == undefined) {
          headers = {};
        } else if (Array.isArray(headers)) {
          statusCode = 0;
          let statusFound = false;
          let isDateSet = false;
          const sensitiveNamesForCopy = headers[sensitiveHeaders];
          headers = headers.slice();
          if (sensitiveNamesForCopy !== undefined)
            headers[sensitiveHeaders] = sensitiveNamesForCopy;
          for (let i = 0;i < headers.length; i += 2) {
            const key = headers[i];
            if (typeof key !== "string")
              continue;
            const lowered = key.toLowerCase();
            if (lowered === HTTP2_HEADER_STATUS) {
              statusFound = true;
              statusCode = headers[i + 1] | 0;
            } else if (lowered === HTTP2_HEADER_DATE)
              isDateSet = true;
          }
          if (!statusFound) {
            statusCode = 200;
            headers.unshift(HTTP2_HEADER_STATUS, statusCode);
          }
          const sendDateOption = options?.sendDate;
          if (!isDateSet && (sendDateOption == null || sendDateOption)) {
            headers.push(HTTP2_HEADER_DATE, utcDate());
          }
          rawHeadersList = headers;
          const headersObject = { __proto__: null };
          for (let i = 0;i < rawHeadersList.length; i += 2) {
            const key = rawHeadersList[i];
            const value = rawHeadersList[i + 1];
            const existing = headersObject[key];
            if (existing === undefined)
              headersObject[key] = value;
            else if (Array.isArray(existing))
              existing.push(value);
            else
              headersObject[key] = [existing, value];
          }
          if (rawHeadersList[sensitiveHeaders] !== undefined) {
            headersObject[sensitiveHeaders] = rawHeadersList[sensitiveHeaders];
          }
          headers = headersObject;
        } else if (!isObject(headers)) {
          throw codes.INVALID_ARG_TYPE("headers", "object", headers);
        } else {
          headers = { ...headers };
        }
        const sensitives = headers[sensitiveHeaders];
        if (sensitives !== undefined && !Array.isArray(sensitives)) {
          throw codes.INVALID_ARG_VALUE("headers[http2.neverIndex]", sensitives);
        }
        const sensitiveNames = buildSensitiveNames(headers, sensitives);
        if (session[kStrictSingleValueFields] !== false)
          assertSingleValueHeaders(headers);
        if (sensitives !== undefined)
          headers[sensitiveHeaders] = sensitives;
        if (rawHeadersList === null) {
          for (const name in headers) {
            if (headerValueIsUnsendable(headers[name])) {
              delete headers[name];
            }
          }
          if (headers[HTTP2_HEADER_STATUS] === undefined) {
            headers[HTTP2_HEADER_STATUS] = 200;
          }
          statusCode = headers[HTTP2_HEADER_STATUS] |= 0;
        }
        if (statusCode === 101) {
          throw codes.HTTP2_STATUS_101();
        }
        if (statusCode < 100 || statusCode > 599) {
          throw codes.HTTP2_STATUS_INVALID(statusCode);
        }
        let endStream = !!options?.endStream;
        if (endStream || statusCode === HTTP_STATUS_NO_CONTENT || statusCode === HTTP_STATUS_RESET_CONTENT || statusCode === HTTP_STATUS_NOT_MODIFIED || this.headRequest === true) {
          options = { ...options, endStream: true, waitForTrailers: false };
          endStream = true;
        }
        const sendDate = options?.sendDate;
        if (rawHeadersList === null && (sendDate == null || sendDate)) {
          const current_date = headers["date"];
          if (current_date == null) {
            headers["date"] = utcDate();
          }
        }
        const wireHeaders = rawHeadersList !== null ? rawHeadersList : headers;
        if (typeof options === "undefined") {
          session[bunHTTP2Native]?.request(this.id, undefined, wireHeaders, sensitiveNames);
        } else {
          session[bunHTTP2Native]?.request(this.id, undefined, wireHeaders, sensitiveNames, options);
          if (options?.waitForTrailers && !endStream) {
            this[bunHTTP2WaitForTrailers] = true;
          }
        }
        this.headersSent = true;
        if (onServerStreamFinishChannel.hasSubscribers) {
          onServerStreamFinishChannel.publish({ stream: this, headers, flags: 0 });
        }
        this[bunHTTP2Headers] = headers;
        if (endStream) {
          this.end();
        }
        return;
      }
    }
    function connectWithProtocol(protocol, options, listener) {
      if (protocol === "http:") {
        return net.connect(options, listener);
      }
      return tls.connect(options, listener);
    }
    function emitConnectNT(self, socket) {
      self.emit("connect", self, socket);
    }
    function destroyWithInvalidSessionNT(stream) {
      if (!stream.destroyed)
        stream.destroy(codes.HTTP2_INVALID_SESSION());
    }
    function scheduleSettingsAckGraceNT(session) {
      if (session[kSettingsAckGraceTimer] !== undefined)
        return;
      const timer = setTimeout(destroyIfNotDestroyedNT, 250, session);
      timer.unref?.();
      session[kSettingsAckGraceTimer] = timer;
    }
    function destroyIfNotDestroyedNT(target) {
      if (!target.destroyed)
        target.destroy();
    }
    function scheduleDestroyIfNotDestroyed(target) {
      if (!target.destroyed) {
        setImmediate(destroyIfNotDestroyedNT, target);
      }
    }
    function rejectNoPayloadContentLengthNT(req) {
      req.rstCode = constants.NGHTTP2_PROTOCOL_ERROR;
      req.destroy(streamErrorFromCode(constants.NGHTTP2_PROTOCOL_ERROR));
    }
    function emitStreamErrorNT(self, stream, error, destroy, destroy_self) {
      if (stream) {
        if (stream.destroyed && stream.listenerCount("error") === 0) {
          if (destroy_self)
            self.destroy();
          return;
        }
        if (typeof error === "number" && self != null && self[kSessionDestroyError] != null) {
          if (error !== 0 && !stream.rstCode)
            stream.rstCode = error;
          error = self[kSessionDestroyError];
        }
        let error_instance = undefined;
        if (stream.listenerCount("error") > 0) {
          if (typeof error === "number") {
            stream.rstCode = error;
            if (error != 0) {
              error_instance = streamErrorFromCode(error);
            }
          } else {
            error_instance = error;
          }
        }
        if (stream.readable) {
          stream.resume();
          pushToStream(stream, null);
        }
        if (destroy) {
          stream.destroy(error_instance, stream.rstCode);
        } else {
          markStreamClosed(stream);
          if (error_instance) {
            stream.emit("error", error_instance);
          }
        }
        if (destroy_self)
          self.destroy();
      }
    }
    const kForbiddenConnectionHeaders = new SafeSet([
      "connection",
      "upgrade",
      "http2-settings",
      "keep-alive",
      "proxy-connection",
      "transfer-encoding"
    ]);
    function assertNoConnectionHeaders(headers) {
      for (const name in headers) {
        const lower = name.toLowerCase();
        if (kForbiddenConnectionHeaders.has(lower) || lower === "te" && headers[name] !== "trailers") {
          const err = new TypeError(`HTTP/1 Connection specific headers are forbidden: "${lower}"`);
          err.code = "ERR_HTTP2_INVALID_CONNECTION_HEADERS";
          throw err;
        }
      }
    }
    function headerValueIsUnsendable(value) {
      if (Array.isArray(value)) {
        for (let i = 0;i < value.length; i++) {
          if (headerValueIsUnsendable(value[i]))
            return true;
        }
        return false;
      }
      if (typeof value !== "string")
        return false;
      for (let i = 0;i < value.length; i++) {
        const c = value.charCodeAt(i);
        if (c > 255 || c === 13 || c === 10 || c === 0)
          return true;
      }
      return false;
    }
    function stripInvalidWhitespaceFields(rawheaders) {
      let filtered = null;
      for (let i = 0;i < rawheaders.length; i += 2) {
        const value = rawheaders[i + 1];
        let bad = false;
        if (typeof value === "string" && value.length > 0) {
          const first = value.charCodeAt(0);
          const last = value.charCodeAt(value.length - 1);
          bad = first === 32 || first === 9 || last === 32 || last === 9;
        }
        if (bad) {
          if (filtered === null)
            filtered = rawheaders.slice(0, i);
        } else if (filtered !== null) {
          filtered.push(rawheaders[i], value);
        }
      }
      return filtered === null ? rawheaders : filtered;
    }
    function assertSingleValueHeaders(headers) {
      let seen = null;
      const keys = Object.keys(headers);
      for (let i = 0;i < keys.length; i++) {
        const lower = keys[i].toLowerCase();
        if (!kSingleValueHeaders.has(lower))
          continue;
        const value = headers[keys[i]];
        if (Array.isArray(value) && value.length > 1 || seen !== null && seen.has(lower)) {
          throw codes.HTTP2_HEADER_SINGLE_VALUE(`Header field "${lower}" must only have a single value`);
        }
        if (seen === null)
          seen = new SafeSet;
        seen.add(lower);
      }
    }
    function receivedValueLabel(value) {
      if (value === null)
        return "null";
      if (typeof value === "object")
        return "an instance of " + (value.constructor?.name || "Object");
      if (typeof value === "function")
        return `function ${value.name}`;
      if (typeof value === "string")
        return `type string ('${value}')`;
      if (typeof value === "symbol")
        return `type symbol (${String(value)})`;
      if (typeof value === "number")
        return `type number (${String(value)})`;
      return `type ${typeof value} (${JSON.stringify(value)})`;
    }
    function buildSensitiveNames(headers, sensitives) {
      const map = {};
      if (sensitives) {
        for (let i = 0;i < sensitives.length; i++) {
          map[String(sensitives[i]).toLowerCase()] = true;
        }
      }
      if (headers != null && typeof headers === "object") {
        const keys = Object.keys(headers);
        for (let i = 0;i < keys.length; i++) {
          const lower = keys[i].toLowerCase();
          if (lower === "authorization") {
            map["authorization"] = true;
          } else if (lower === "cookie") {
            const cookie = headers[keys[i]];
            if (typeof cookie === "string" && cookie.length < 20)
              map["cookie"] = true;
          }
        }
      }
      return map;
    }
    function toHeaderObject(headers, sensitiveHeadersValue) {
      const obj = { __proto__: null, [sensitiveHeaders]: sensitiveHeadersValue };
      for (let n = 0;n < headers.length; n += 2) {
        const name = headers[n];
        let value = headers[n + 1] || "";
        if (name === HTTP2_HEADER_STATUS)
          value |= 0;
        const existing = obj[name];
        if (existing === undefined) {
          obj[name] = name === HTTP2_HEADER_SET_COOKIE ? [value] : value;
        } else if (!kSingleValueHeaders.has(name)) {
          switch (name) {
            case HTTP2_HEADER_COOKIE:
              obj[name] = `${existing}; ${value}`;
              break;
            case HTTP2_HEADER_SET_COOKIE:
              ArrayPrototypePush.call(existing, value);
              break;
            default:
              obj[name] = `${existing}, ${value}`;
              break;
          }
        }
      }
      return obj;
    }
    function getOrigin(origin, isAltSvc) {
      if (typeof origin === "string") {
        try {
          origin = new URL(origin).origin;
        } catch (e) {
          if (isAltSvc) {
            throw codes.HTTP2_ALTSVC_INVALID_ORIGIN();
          } else {
            throw codes.INVALID_URL(origin);
          }
        }
      } else if (origin != null && typeof origin === "object") {
        origin = origin.origin;
      }
      validateString(origin, "origin");
      if (!origin || origin === "null") {
        if (isAltSvc) {
          throw codes.HTTP2_ALTSVC_INVALID_ORIGIN();
        } else {
          throw codes.HTTP2_INVALID_ORIGIN();
        }
      }
      return origin;
    }
    function initOriginSet(session) {
      let originSet = session[bunHTTP2OriginSet];
      if (originSet === undefined) {
        const socket = session[bunHTTP2Socket];
        session[bunHTTP2OriginSet] = originSet = new Set;
        let hostName = socket.servername;
        if (!hostName) {
          if (socket.remoteFamily === "IPv6") {
            hostName = `[${socket.remoteAddress}]`;
          } else {
            hostName = socket.remoteAddress;
          }
        }
        let originString = `https://${hostName}`;
        if (socket.remotePort != null)
          originString += `:${socket.remotePort}`;
        originSet.add(originString);
      }
      return originSet;
    }
    function removeOriginFromSet(session, stream) {
      const originSet = session[bunHTTP2OriginSet];
      const origin = `https://${stream.authority}`;
      if (originSet && origin) {
        originSet.delete(origin);
      }
    }

    class ServerHttp2Session extends Http2Session {
      [kServer] = null;
      #closed = false;
      #destroying = false;
      #closeCalled = false;
      #connected = false;
      #connections = 0;
      #socket_proxy;
      #parser;
      #alpnProtocol = undefined;
      #localSettings = null;
      #encrypted = false;
      #pendingSettingsAck = true;
      #pendingSettingsAckCount = 1;
      #maxOutstandingSettings = 10;
      #pendingSettingsCallbacks = [null];
      #remoteSettings = null;
      #pingCallbacks = null;
      #strictFieldWhitespaceValidation = true;
      #advertisedMaxConcurrentStreams = 1 / 0;
      #peerInitiatedStreams = 0;
      static #Handlers = {
        binaryType: "buffer",
        streamStart(self, stream_id) {
          if (!self)
            return;
          if (stream_id % 2 === 1 && self.#peerInitiatedStreams >= self.#advertisedMaxConcurrentStreams) {
            self.#parser?.rstStream(stream_id, constants.NGHTTP2_REFUSED_STREAM);
            return;
          }
          self.#connections++;
          if (stream_id % 2 === 1)
            self.#peerInitiatedStreams++;
          const stream = new ServerHttp2Stream(stream_id, self, null);
          return stream;
        },
        frameError(self, stream, frameType, errorCode) {
          if (!self || typeof stream !== "object")
            return;
          process.nextTick(emitFrameErrorEventNT, stream, frameType, errorCode);
        },
        aborted(self, stream, error, old_state) {
          if (!self || typeof stream !== "object")
            return;
          stream.rstCode = constants.NGHTTP2_CANCEL;
          if (old_state != 5 && old_state != 7) {
            stream[kAborted] = true;
            stream.emit("aborted");
          }
          self.#connections--;
          if (stream.id % 2 === 1)
            self.#peerInitiatedStreams--;
          process.nextTick(emitStreamErrorNT, self, stream, error, true, self.#connections === 0 && self.#closed);
        },
        streamError(self, stream, error) {
          if (!self || typeof stream !== "object")
            return;
          self.#connections--;
          if (stream.id % 2 === 1)
            self.#peerInitiatedStreams--;
          process.nextTick(emitStreamErrorNT, self, stream, error, true, self.#connections === 0 && self.#closed);
        },
        streamEnd(self, stream, state) {
          if (!self || typeof stream !== "object")
            return;
          if (state === 7 && stream[kSendingTrailers]) {
            process.nextTick(ServerHttp2Session.#Handlers.streamEnd, self, stream, state);
            return;
          }
          if (state == 6 || state == 7) {
            if (stream.readable) {
              if (!stream.rstCode) {
                stream.rstCode = 0;
              }
              pushToStream(stream, null);
              if ((state == 7 || stream.readableLength === 0) && stream.readableFlowing === null) {
                stream.resume();
              }
            }
          }
          if (state === 7) {
            stream[bunHTTP2StreamStatus] |= 64 /* NativeClosed */;
            markStreamClosed(stream);
            self.#connections--;
            if (stream.id % 2 === 1)
              self.#peerInitiatedStreams--;
            if (stream.readable && !stream.rstCode) {
              stream.once("end", destroySelfOnEnd);
            } else if ((stream.writableEnded || stream[kEndingWithChunk]) && !stream.writableFinished && !stream.destroyed) {
              stream.once("finish", destroySelfOnEnd);
            } else {
              stream.destroy();
            }
            if (self.#connections === 0 && self.#closed) {
              if (self.#pendingSettingsAckCount > 0 || self.#pingCallbacks !== null && self.#pingCallbacks.length > 0) {
                scheduleSettingsAckGraceNT(self);
              } else {
                captureHttp2PerfFrameSnapshot(self, self[bunHTTP2Native]);
                setImmediate(destroyIfNotDestroyedNT, self);
              }
            }
          } else if (state === 5) {
            markWritableDone(stream);
          }
        },
        streamData(self, stream, data) {
          if (!self || typeof stream !== "object" || !data)
            return;
          pushToStream(stream, data);
          maybeCloseUnreadServerStream(stream);
        },
        streamHeaders(self, stream, headersTuple, flags) {
          if (!self || typeof stream !== "object" || self.closed || stream.closed)
            return;
          const requestPerf = stream[kPerfState];
          if (requestPerf !== undefined && requestPerf.firstHeader === 0) {
            requestPerf.firstHeader = performance.now() - requestPerf.start;
          }
          let rawheaders = headersTuple[0];
          let headers = headersTuple[1];
          if (self.#strictFieldWhitespaceValidation) {
            const filtered = stripInvalidWhitespaceFields(rawheaders);
            if (filtered !== rawheaders) {
              rawheaders = filtered;
              headers = toHeaderObject(filtered, headersTuple[2] || []);
            }
          }
          if (stream[kRequestHeaders] === undefined) {
            stream[kRequestHeaders] = headers;
          }
          if (stream[bunHTTP2Headers] == null) {
            stream[bunHTTP2Headers] = headers;
          }
          if (headers[HTTP2_HEADER_METHOD] === HTTP2_METHOD_HEAD) {
            stream[kHeadRequest] = true;
          }
          const status = stream[bunHTTP2StreamStatus];
          if ((status & 16 /* StreamResponded */) !== 0) {
            stream.emit("trailers", headers, flags, rawheaders);
          } else {
            stream[bunHTTP2StreamStatus] |= 16 /* StreamResponded */ | 256 /* Delivered */;
            if (onServerStreamCreatedChannel.hasSubscribers) {
              onServerStreamCreatedChannel.publish({ stream, headers });
            }
            if (onServerStreamStartChannel.hasSubscribers) {
              onServerStreamStartChannel.publish({ stream, headers });
            }
            self[kServer]?.emit("stream", stream, headers, flags, rawheaders);
            self.emit("stream", stream, headers, flags, rawheaders);
          }
        },
        localSettings(self, settings) {
          if (!self)
            return;
          self.#localSettings = settings;
          self.#pendingSettingsAck = false;
          if (self.#pendingSettingsAckCount > 0)
            self.#pendingSettingsAckCount--;
          const pingsDrained = self.#pingCallbacks === null || self.#pingCallbacks.length === 0;
          if (self.#pendingSettingsAckCount === 0 && pingsDrained && self[kSettingsAckGraceTimer] !== undefined) {
            clearTimeout(self[kSettingsAckGraceTimer]);
            self[kSettingsAckGraceTimer] = undefined;
          }
          if (self.#closed && self.#connections === 0 && self.#pendingSettingsAckCount === 0 && pingsDrained && !self.destroyed) {
            captureHttp2PerfFrameSnapshot(self, self[bunHTTP2Native]);
            setImmediate(destroyIfNotDestroyedNT, self);
          }
          const queued = self.#pendingSettingsCallbacks.shift();
          if (queued != null)
            queued[0](null, settings, Date.now() - queued[1]);
          self.emit("localSettings", settings);
        },
        remoteSettings(self, settings) {
          if (!self)
            return;
          self.#remoteSettings = settings;
          self.emit("remoteSettings", settings);
        },
        ping(self, payload, isACK) {
          if (!self)
            return;
          if (!isACK) {
            self.emit("ping", payload);
          }
          if (isACK) {
            const callbacks = self.#pingCallbacks;
            if (callbacks) {
              const callbackInfo = callbacks.shift();
              if (callbackInfo) {
                const [callback, start] = callbackInfo;
                const rtt = Date.now() - start;
                const stats = self[kPerfStats];
                if (stats !== undefined)
                  stats.pingRTT = rtt;
                callback(null, rtt, payload);
                if (callbacks.length === 0 && self.#pendingSettingsAckCount === 0) {
                  if (self[kSettingsAckGraceTimer] !== undefined) {
                    clearTimeout(self[kSettingsAckGraceTimer]);
                    self[kSettingsAckGraceTimer] = undefined;
                  }
                  if (self.#closed && self.#connections === 0 && !self.destroyed) {
                    captureHttp2PerfFrameSnapshot(self, self[bunHTTP2Native]);
                    setImmediate(destroyIfNotDestroyedNT, self);
                  }
                }
              }
            }
          }
        },
        error(self, errorCode, lastStreamId, opaqueData) {
          if (!self)
            return;
          if (errorCode === "ERR_HTTP2_TOO_MANY_INVALID_FRAMES") {
            self.destroy(codes.HTTP2_TOO_MANY_INVALID_FRAMES());
            return;
          }
          if (typeof errorCode === "number" && errorCode < 0) {
            self.destroy(new NghttpError(errorCode));
            return;
          }
          self.destroy(errorCode);
        },
        wantTrailers(self, stream) {
          if (!self || typeof stream !== "object")
            return;
          const status = stream[bunHTTP2StreamStatus];
          if ((status & 2 /* WantTrailer */) !== 0)
            return;
          stream[bunHTTP2StreamStatus] = status | 2 /* WantTrailer */;
          if (stream.listenerCount("wantTrailers") === 0) {
            self[bunHTTP2Native]?.noTrailers(stream.id);
          } else {
            stream.emit("wantTrailers");
          }
        },
        goaway(self, errorCode, lastStreamId, opaqueData) {
          if (!self)
            return;
          if (self.destroyed)
            return;
          self[kGoawayCode] = errorCode;
          self[kGoawayLastStreamID] = lastStreamId;
          self.emit("goaway", errorCode, lastStreamId, opaqueData || Buffer.allocUnsafe(0));
          if (errorCode === constants.NGHTTP2_NO_ERROR) {
            self.close();
          } else if (self.closed && self.#connections === 0) {
            self.destroy();
          } else {
            self.#parser?.emitErrorToAllStreams(errorCode);
            self.destroy(sessionErrorFromCode(errorCode), constants.NGHTTP2_NO_ERROR);
          }
        },
        end(self, errorCode, lastStreamId, opaqueData) {
          if (!self)
            return;
          self.destroy();
        },
        write(self, buffer) {
          if (!self)
            return -1;
          const socket = self[bunHTTP2Socket];
          if (socket && !socket.writableEnded && self.#connected) {
            return socket.write(buffer) ? 1 : 0;
          }
          return -1;
        }
      };
      #onRead(data) {
        if (this[kTimeout])
          this[kTimeout].refresh();
        this.#parser?.read(data);
      }
      #onClose() {
        const parser = this.#parser;
        if (parser) {
          parser.emitAbortToAllStreams();
          parser.forEachStream(streamSocketClosed);
          parser.detach();
          this.#parser = null;
        }
        this.close();
        this.destroy();
      }
      #onError(error) {
        if (this.destroyed) {
          return;
        }
        if (isEconnresetAfterGoaway(this, error)) {
          this.destroy();
          return;
        }
        if (this.listenerCount("error") === 0 && error?.code === "ECONNRESET") {
          this.destroy();
          return;
        }
        this.destroy(error);
      }
      #onTimeout() {
        const parser = this.#parser;
        if (parser) {
          parser.forEachStream(emitTimeout);
        }
        this.emit("timeout");
      }
      #onDrain() {
        const parser = this.#parser;
        if (parser) {
          parser.flush();
        }
      }
      altsvc(alt, originOrStream) {
        const MAX_LENGTH = 16382;
        const parser = this.#parser;
        if (this.destroyed || !parser)
          throw codes.HTTP2_INVALID_SESSION();
        let stream = 0;
        let origin;
        if (typeof originOrStream === "string") {
          origin = getOrigin(originOrStream, true);
        } else if (typeof originOrStream === "number") {
          if (originOrStream >>> 0 !== originOrStream || originOrStream === 0) {
            throw codes.OUT_OF_RANGE("originOrStream", `> 0 && < ${2 ** 32}`, originOrStream);
          }
          stream = originOrStream;
        } else if (originOrStream !== undefined) {
          if (originOrStream !== null && typeof originOrStream === "object")
            origin = originOrStream.origin;
          if (typeof origin !== "string") {
            throw codes.INVALID_ARG_TYPE("originOrStream", ["string", "number", "URL", "object"], originOrStream);
          } else if (origin === "null" || origin === "") {
            throw codes.HTTP2_ALTSVC_INVALID_ORIGIN();
          }
        }
        validateString(alt, "alt");
        if (!kQuotedString.test(alt)) {
          throw codes.INVALID_CHAR("alt");
        }
        origin = origin || "";
        if (Buffer.byteLength(origin) + Buffer.byteLength(alt) > MAX_LENGTH) {
          throw codes.HTTP2_ALTSVC_LENGTH();
        }
        parser.altsvc(origin, alt, stream);
      }
      origin(...origins) {
        const parser = this.#parser;
        if (this.destroyed || !parser)
          throw codes.HTTP2_INVALID_SESSION();
        let length = origins.length;
        if (length === 0) {
          return;
        }
        if (length === 1) {
          return parser.origin(getOrigin(origins[0], false));
        }
        let validOrigins = new Array(length);
        for (let i = 0;i < length; i++) {
          validOrigins[i] = getOrigin(origins[i], false);
        }
        parser.origin(validOrigins);
      }
      [kReleaseUnannouncedStream](streamId) {
        this.#connections--;
        this.#parser?.setStreamContext(streamId, undefined);
      }
      constructor(socket, options, server) {
        super();
        if (socket[kBoundSession] !== undefined) {
          const err = new Error("The socket is already bound to an Http2Session");
          err.code = "ERR_HTTP2_SOCKET_BOUND";
          throw err;
        }
        socket[kBoundSession] = this;
        this[kServer] = server;
        if (options?.strictFieldWhitespaceValidation === false) {
          this.#strictFieldWhitespaceValidation = false;
        }
        this[kStrictSingleValueFields] = options?.strictSingleValueFields !== false;
        if (server) {
          server[kSessions].add(this);
        }
        this.#connected = true;
        if (socket instanceof TLSSocket) {
          this.#alpnProtocol = socket.alpnProtocol || "h2";
        } else {
          this.#alpnProtocol = "h2c";
        }
        this[bunHTTP2Socket] = socket;
        const nativeSocket = socket._handle;
        this[kDeferWriteCallback] = deferWriteCallbackForSocket(nativeSocket);
        this.#encrypted = socket instanceof TLSSocket;
        if (typeof options?.maxOutstandingSettings === "number" && options.maxOutstandingSettings >= 1) {
          this.#maxOutstandingSettings = options.maxOutstandingSettings;
        }
        const advertisedMaxConcurrentStreams = options?.settings?.maxConcurrentStreams ?? options?.maxConcurrentStreams;
        if (typeof advertisedMaxConcurrentStreams === "number") {
          this.#advertisedMaxConcurrentStreams = advertisedMaxConcurrentStreams;
        }
        if (options?.settings !== undefined) {
          validateSettings(options.settings);
        }
        const nativeSettings = serverNativeSettings(options);
        this.#localSettings = initialLocalSettings(nativeSettings);
        this.#parser = new H2FrameParser({
          native: nativeSocket,
          context: this,
          settings: nativeSettings,
          type: 0,
          handlers: ServerHttp2Session.#Handlers
        });
        socket.on("close", this.#onClose.bind(this));
        socket.on("error", this.#onError.bind(this));
        socket.on("timeout", this.#onTimeout.bind(this));
        initHttp2SessionPerf(this, "server");
        socket.on("data", this.#onRead.bind(this));
        socket.on("drain", this.#onDrain.bind(this));
        process.nextTick(emitConnectNT, this, socket);
      }
      get server() {
        return this[kServer];
      }
      get originSet() {
        if (this.encrypted) {
          return Array.from(initOriginSet(this));
        }
      }
      get alpnProtocol() {
        return this.#alpnProtocol;
      }
      get connecting() {
        const socket = this[bunHTTP2Socket];
        if (!socket) {
          return false;
        }
        return socket.connecting || false;
      }
      get connected() {
        return this[bunHTTP2Socket]?.connecting === false;
      }
      get destroyed() {
        return this[bunHTTP2Socket] === null;
      }
      get encrypted() {
        return this.#encrypted;
      }
      get closed() {
        return this.#closeCalled;
      }
      get remoteSettings() {
        if (this.destroyed || this.connecting)
          return {};
        return this.#remoteSettings ??= getDefaultSettings();
      }
      get localSettings() {
        if (this.destroyed || this.connecting)
          return {};
        return this.#localSettings ??= getDefaultSettings();
      }
      get pendingSettingsAck() {
        return this.#pendingSettingsAck;
      }
      get type() {
        return 0;
      }
      get socket() {
        const socket = this[bunHTTP2Socket];
        if (!socket)
          return;
        if (this.#socket_proxy)
          return this.#socket_proxy;
        this.#socket_proxy = new Proxy(this, proxySocketHandler);
        return this.#socket_proxy;
      }
      get state() {
        return this.#parser?.getCurrentState();
      }
      get [bunHTTP2Native]() {
        return this.#parser;
      }
      unref() {
        const socket = this[bunHTTP2Socket];
        if (typeof socket?.unref === "function")
          return socket.unref();
      }
      ref() {
        const socket = this[bunHTTP2Socket];
        if (typeof socket?.ref === "function")
          return socket.ref();
      }
      setTimeout(msecs, callback) {
        return setSessionTimeout.call(this, msecs, callback);
      }
      ping(payload, callback) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        if (typeof payload === "function") {
          callback = payload;
          payload = undefined;
        }
        if (payload) {
          validateBuffer(payload, "payload");
          if (payload.byteLength !== 8)
            throw codes.HTTP2_PING_LENGTH();
        }
        validateFunction(callback, "callback");
        const cb = makeHttp2Ping(callback);
        const parser = this.#parser;
        if (!parser || !this[bunHTTP2Socket]) {
          process.nextTick(cb, codes.HTTP2_PING_CANCEL(), 0, payload);
          return false;
        }
        if (parser.ping(payload || kEmptyPingPayload) === false) {
          process.nextTick(cb, codes.HTTP2_PING_CANCEL(), 0, payload);
          return false;
        }
        if (this.#pingCallbacks) {
          this.#pingCallbacks.push([cb, Date.now()]);
        } else {
          this.#pingCallbacks = [[cb, Date.now()]];
        }
        return true;
      }
      goaway(code = NGHTTP2_NO_ERROR, lastStreamID = 0, opaqueData) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        if (opaqueData !== undefined) {
          validateBuffer(opaqueData, "opaqueData");
        }
        validateInteger(code, "code", 0, kMaxUint32);
        validateNumber(lastStreamID, "lastStreamID");
        return this.#parser?.goaway(code, lastStreamID, opaqueData);
      }
      setLocalWindowSize(windowSize) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        validateWindowSize(windowSize);
        return this.#parser?.setLocalWindowSize?.(windowSize);
      }
      settings(settings, callback) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        if (callback !== undefined && typeof callback !== "function") {
          throw codes.INVALID_ARG_TYPE("callback", "function", callback);
        }
        if (settings === undefined)
          settings = {};
        validateSettings(settings);
        settings = { ...settings, enablePush: false };
        if (typeof settings.maxConcurrentStreams === "number") {
          this.#advertisedMaxConcurrentStreams = settings.maxConcurrentStreams;
        }
        this.#pendingSettingsAckCount++;
        if (this.#pendingSettingsAckCount > this.#maxOutstandingSettings) {
          this.destroy(codes.HTTP2_MAX_PENDING_SETTINGS_ACK(), constants.NGHTTP2_INTERNAL_ERROR);
          return;
        }
        this.#pendingSettingsAck = true;
        this.#parser?.settings(settings);
        this.#parser?.flush?.();
        this.#pendingSettingsCallbacks.push(typeof callback === "function" ? [callback, Date.now()] : null);
      }
      close(callback) {
        if (this.#closed || this.destroyed)
          return;
        this.#closed = true;
        this.#closeCalled = true;
        if (typeof callback === "function") {
          this.once("close", callback);
        }
        this.goaway(constants.NGHTTP2_NO_ERROR, 0, Buffer.alloc(0));
        this[kGoawaySent] = true;
        this.#parser?.flush?.();
        if (this.#connections === 0) {
          if (this.#pendingSettingsAckCount > 0 || this.#pingCallbacks !== null && this.#pingCallbacks.length > 0) {
            scheduleSettingsAckGraceNT(this);
          } else {
            captureHttp2PerfFrameSnapshot(this, this.#parser);
            setImmediate(destroyIfNotDestroyedNT, this);
          }
        }
      }
      destroy(error = NGHTTP2_NO_ERROR, code) {
        if (this.#destroying) {
          return;
        }
        this.#destroying = true;
        emitHttp2SessionPerf(this, this.#parser, this[bunHTTP2Socket]);
        try {
          const server = this[kServer];
          if (server) {
            server[kSessions].delete(this);
          }
          if (this[kTimeout]) {
            clearTimeout(this[kTimeout]);
            this[kTimeout] = null;
          }
          cancelPendingPings(this.#pingCallbacks);
          this.#pingCallbacks = null;
          if (typeof error === "number") {
            code = error;
            error = code !== NGHTTP2_NO_ERROR ? codes.HTTP2_SESSION_ERROR(code) : undefined;
          }
          if (code === undefined && error != null) {
            code = constants.NGHTTP2_INTERNAL_ERROR;
          }
          if (error) {
            this[kSessionDestroyError] = error;
          }
          const socket = this[bunHTTP2Socket];
          if (!this.#connected)
            return;
          this.#closed = true;
          this.#connected = false;
          if (socket) {
            if (!this[kGoawaySent] || code) {
              this.goaway(code || constants.NGHTTP2_NO_ERROR, 0, Buffer.alloc(0));
            }
            if (error) {
              endThenDestroySessionSocket(socket, error);
            } else {
              socket.resume();
              socket.end();
            }
          }
          const parser = this.#parser;
          if (parser) {
            const streamRstCode = this[kGoawayCode] || code || constants.NGHTTP2_NO_ERROR;
            parser.forEachStream(FunctionPrototypeBind.call(destroyStreamForSessionDestroy, undefined, error, streamRstCode));
            parser.emitErrorToAllStreams(streamRstCode);
            parser.detach();
            this.#parser = null;
          }
        } catch (e) {
          this.#destroying = false;
          throw e;
        }
        this[bunHTTP2Socket] = null;
        const asyncFrame = this[bunHTTP2AsyncContextFrame];
        this[bunHTTP2AsyncContextFrame] = undefined;
        if (error) {
          runInFrame(asyncFrame, this.emit, this, "error", error);
        }
        process.nextTick(emitSessionCloseNT, this, asyncFrame);
      }
    }
    function emitTimeout(session) {
      session.emit("timeout");
    }
    const kTimeoutBytesSnapshot = Symbol("timeoutBytesSnapshot");
    const kTimeoutWrittenSnapshot = Symbol("timeoutWrittenSnapshot");
    let sessionHasPendingWrite = false;
    function checkStreamWritePending(stream) {
      if (stream.writableLength > 0)
        sessionHasPendingWrite = true;
    }
    function setSessionTimeout(msecs, callback) {
      if (this.destroyed)
        return this;
      this.timeout = msecs;
      msecs = getTimerDuration(msecs, "msecs");
      clearTimeout(this[kTimeout]);
      this[kTimeout] = null;
      if (msecs === 0) {
        if (callback !== undefined) {
          validateFunction(callback, "callback");
          this.removeListener("timeout", callback);
        }
      } else {
        this[kTimeoutWrittenSnapshot] = this[bunHTTP2Socket]?._handle?.bytesWritten ?? this[bunHTTP2Socket]?.bytesWritten ?? 0;
        this[kTimeout] = setTimeout(sessionTimerExpired, msecs, this).unref();
        if (callback !== undefined) {
          validateFunction(callback, "callback");
          this.once("timeout", callback);
        }
      }
      return this;
    }
    function destroySessionSocketDelayedNT(socket, error) {
      if (!socket.destroyed) {
        socket.destroy(error);
      }
    }
    function endThenDestroySessionSocket(socket, error) {
      socket.end(() => setImmediate(destroySessionSocketDelayedNT, socket, error));
    }
    function sessionTimerExpired(session) {
      if (session.destroyed)
        return;
      const parser = session[bunHTTP2Native];
      if (parser) {
        const sessionSocket = session[bunHTTP2Socket];
        const bytesWritten = sessionSocket?._handle?.bytesWritten ?? sessionSocket?.bytesWritten ?? 0;
        if (bytesWritten !== (session[kTimeoutWrittenSnapshot] ?? 0)) {
          session[kTimeoutWrittenSnapshot] = bytesWritten;
          session[kTimeout]?.refresh();
          return;
        }
        sessionHasPendingWrite = false;
        parser.forEachStream(checkStreamWritePending);
        const nativeBuffered = parser.bufferSize();
        const previousBuffered = session[kTimeoutBytesSnapshot] ?? 0;
        if (sessionHasPendingWrite || nativeBuffered > 0 || previousBuffered > 0) {
          if (nativeBuffered !== previousBuffered) {
            session[kTimeoutBytesSnapshot] = nativeBuffered;
            session[kTimeout]?.refresh();
            return;
          }
        }
        parser.forEachStream(emitTimeout);
      }
      session.emit("timeout");
    }
    function destroySelfOnEnd() {
      this.destroy();
    }
    function streamCancel(stream) {
      stream.close(NGHTTP2_CANCEL);
    }
    function streamSocketClosed(stream) {
      if (!stream.destroyed) {
        stream.destroy();
      }
    }
    function rejectStreamAboveGoawayLastId(lastStreamId, stream) {
      if (typeof stream?.id === "number" && stream.id > lastStreamId) {
        streamRejectedByGoawaySession(stream);
      }
    }
    function streamRejectedByGoawaySession(stream) {
      if (!stream.destroyed) {
        const err = new Error("New streams cannot be created after receiving a GOAWAY");
        err.code = "ERR_HTTP2_GOAWAY_SESSION";
        stream.rstCode = constants.NGHTTP2_REFUSED_STREAM;
        stream.destroy(err);
      }
    }

    class ClientHttp2Session extends Http2Session {
      #closed = false;
      #destroying = false;
      #closeCalled = false;
      #connected = false;
      #connections = 0;
      #socket_proxy;
      #parser;
      #url;
      #authority;
      #alpnProtocol = undefined;
      #localSettings = null;
      #encrypted = false;
      #pendingSettingsAck = true;
      #pendingSettingsAckCount = 1;
      #maxOutstandingSettings = 10;
      #pendingSettingsCallbacks = [null];
      #remoteSettings = null;
      #pingCallbacks = null;
      #maxReservedRemoteStreams = 200;
      #reservedStreamsCount = 0;
      #strictFieldWhitespaceValidation = true;
      #activeRequestCount = 0;
      #pendingRequests = null;
      static #Handlers = {
        binaryType: "buffer",
        streamStart(self, stream_id) {
          if (!self)
            return;
          self.#connections++;
          if (stream_id % 2 === 0) {
            const stream = new ClientHttp2Stream(stream_id, self, null);
            return stream;
          }
        },
        streamPush(self, pushId, headersTuple, flags) {
          if (!self)
            return;
          if (self.#reservedStreamsCount >= self.#maxReservedRemoteStreams) {
            self.#parser?.rstStream(pushId, constants.NGHTTP2_CANCEL);
            return;
          }
          self.#reservedStreamsCount++;
          let rawheaders = headersTuple[0];
          let headers = headersTuple[1];
          if (self.#strictFieldWhitespaceValidation) {
            const filtered = stripInvalidWhitespaceFields(rawheaders);
            if (filtered !== rawheaders) {
              rawheaders = filtered;
              headers = toHeaderObject(filtered, headersTuple[2] || []);
            }
          }
          const pushedStream = new ClientHttp2Stream(pushId, self, headers);
          pushedStream[kPush] = true;
          pushedStream.once("close", () => {
            self.#reservedStreamsCount--;
          });
          self.#connections++;
          self.#parser?.setStreamContext(pushId, pushedStream);
          if (onClientStreamCreatedChannel.hasSubscribers) {
            onClientStreamCreatedChannel.publish({ stream: pushedStream, headers });
          }
          if (onClientStreamStartChannel.hasSubscribers) {
            onClientStreamStartChannel.publish({ stream: pushedStream, headers });
          }
          self.emit("stream", pushedStream, headers, flags, rawheaders);
        },
        frameError: withStreamFrame((self, stream, frameType, errorCode) => {
          if (!self || typeof stream !== "object")
            return;
          process.nextTick(emitFrameErrorEventNT, stream, frameType, errorCode);
        }),
        aborted: withStreamFrame((self, stream, error, old_state) => {
          if (!self || typeof stream !== "object")
            return;
          stream.rstCode = constants.NGHTTP2_CANCEL;
          if (old_state != 5 && old_state != 7) {
            stream[kAborted] = true;
            stream.emit("aborted");
          }
          self.#connections--;
          process.nextTick(emitStreamErrorNT, self, stream, error, true, self.#connections === 0 && self.#closed);
        }),
        streamError: withStreamFrame((self, stream, error) => {
          if (!self || typeof stream !== "object")
            return;
          self.#connections--;
          process.nextTick(emitStreamErrorNT, self, stream, error, true, self.#connections === 0 && self.#closed);
        }),
        streamEnd: withStreamFrame((self, stream, state) => {
          if (!self || typeof stream !== "object")
            return;
          if (state === 7 && stream[kSendingTrailers]) {
            process.nextTick(ClientHttp2Session.#Handlers.streamEnd, self, stream, state);
            return;
          }
          if (state == 6 || state == 7) {
            if (stream.readable) {
              if (!stream.rstCode) {
                stream.rstCode = 0;
              }
              pushToStream(stream, null);
              stream.read(0);
            }
          }
          if (state === 7) {
            stream[bunHTTP2StreamStatus] |= 64 /* NativeClosed */;
            markStreamClosed(stream);
            self.#connections--;
            if (stream.readable && !stream.rstCode) {
              stream.once("end", destroySelfOnEnd);
            } else if ((stream.writableEnded || stream[kEndingWithChunk]) && !stream.writableFinished && !stream.destroyed) {
              stream.once("finish", destroySelfOnEnd);
            } else {
              stream.destroy();
            }
            if (self.#connections === 0 && self.#closed) {
              if (self.#pendingSettingsAckCount > 0 || self.#pingCallbacks !== null && self.#pingCallbacks.length > 0) {
                scheduleSettingsAckGraceNT(self);
              } else {
                captureHttp2PerfFrameSnapshot(self, self[bunHTTP2Native]);
                setImmediate(destroyIfNotDestroyedNT, self);
              }
            }
          } else if (state === 5) {
            markWritableDone(stream);
          }
        }),
        streamData: withStreamFrame((self, stream, data) => {
          if (!self || typeof stream !== "object" || !data)
            return;
          pushToStream(stream, data);
        }),
        streamHeaders: withStreamFrame((self, stream, headersTuple, flags) => {
          if (!self || typeof stream !== "object" || stream.rstCode)
            return;
          let rawheaders = headersTuple[0];
          let headers = headersTuple[1];
          if (self.#strictFieldWhitespaceValidation) {
            const filtered = stripInvalidWhitespaceFields(rawheaders);
            if (filtered !== rawheaders) {
              rawheaders = filtered;
              headers = toHeaderObject(filtered, headersTuple[2] || []);
            }
          }
          const status = stream[bunHTTP2StreamStatus];
          const header_status = headers[HTTP2_HEADER_STATUS];
          if (header_status === HTTP_STATUS_CONTINUE) {
            stream.emit("continue");
          }
          if ((status & 16 /* StreamResponded */) !== 0) {
            stream.emit("trailers", headers, flags, rawheaders);
          } else {
            if (header_status >= 100 && header_status < 200) {
              stream.emit("headers", headers, flags, rawheaders);
            } else {
              stream[bunHTTP2StreamStatus] |= 16 /* StreamResponded */;
              if (header_status === 421) {
                removeOriginFromSet(self, stream);
              }
              if (onClientStreamFinishChannel.hasSubscribers) {
                onClientStreamFinishChannel.publish({ stream, headers, flags });
              }
              if (stream[kPush]) {
                stream.emit("push", headers, flags, rawheaders);
              } else {
                const responsePerf = stream[kPerfState];
                if (responsePerf !== undefined && responsePerf.firstHeader === 0) {
                  responsePerf.firstHeader = performance.now() - responsePerf.start;
                }
                stream.emit("response", headers, flags, rawheaders);
              }
            }
          }
        }),
        localSettings(self, settings) {
          if (!self)
            return;
          self.#localSettings = settings;
          self.#pendingSettingsAck = false;
          if (self.#pendingSettingsAckCount > 0)
            self.#pendingSettingsAckCount--;
          const pingsDrained = self.#pingCallbacks === null || self.#pingCallbacks.length === 0;
          if (self.#pendingSettingsAckCount === 0 && pingsDrained && self[kSettingsAckGraceTimer] !== undefined) {
            clearTimeout(self[kSettingsAckGraceTimer]);
            self[kSettingsAckGraceTimer] = undefined;
          }
          if (self.#closed && self.#connections === 0 && self.#pendingSettingsAckCount === 0 && pingsDrained && !self.destroyed) {
            captureHttp2PerfFrameSnapshot(self, self[bunHTTP2Native]);
            setImmediate(destroyIfNotDestroyedNT, self);
          }
          const queued = self.#pendingSettingsCallbacks.shift();
          if (queued != null)
            queued[0](null, settings, Date.now() - queued[1]);
          self.emit("localSettings", settings);
        },
        remoteSettings(self, settings) {
          if (!self)
            return;
          self.#remoteSettings = settings;
          self.emit("remoteSettings", settings);
          self.#flushPendingRequests();
        },
        ping(self, payload, isACK) {
          if (!self)
            return;
          if (!isACK) {
            self.emit("ping", payload);
          }
          if (isACK) {
            const callbacks = self.#pingCallbacks;
            if (callbacks) {
              const callbackInfo = callbacks.shift();
              if (callbackInfo) {
                const [callback, start] = callbackInfo;
                const rtt = Date.now() - start;
                const stats = self[kPerfStats];
                if (stats !== undefined)
                  stats.pingRTT = rtt;
                callback(null, rtt, payload);
                if (callbacks.length === 0 && self.#pendingSettingsAckCount === 0) {
                  if (self[kSettingsAckGraceTimer] !== undefined) {
                    clearTimeout(self[kSettingsAckGraceTimer]);
                    self[kSettingsAckGraceTimer] = undefined;
                  }
                  if (self.#closed && self.#connections === 0 && (self.#pendingRequests === null || self.#pendingRequests.length === 0) && !self.destroyed) {
                    captureHttp2PerfFrameSnapshot(self, self[bunHTTP2Native]);
                    setImmediate(destroyIfNotDestroyedNT, self);
                  }
                }
              }
            }
          }
        },
        error(self, errorCode, lastStreamId, opaqueData) {
          if (!self)
            return;
          const error_instance = errorCode === "ERR_HTTP2_TOO_MANY_INVALID_FRAMES" ? codes.HTTP2_TOO_MANY_INVALID_FRAMES() : typeof errorCode === "number" && errorCode < 0 ? new NghttpError(errorCode) : sessionErrorFromCode(errorCode);
          self.destroy(error_instance);
        },
        wantTrailers: withStreamFrame((self, stream) => {
          if (!self || typeof stream !== "object")
            return;
          const status = stream[bunHTTP2StreamStatus];
          if ((status & 2 /* WantTrailer */) !== 0)
            return;
          stream[bunHTTP2StreamStatus] = status | 2 /* WantTrailer */;
          if (stream.listenerCount("wantTrailers") === 0) {
            self[bunHTTP2Native]?.noTrailers(stream.id);
          } else {
            stream.emit("wantTrailers");
          }
        }),
        goaway(self, errorCode, lastStreamId, opaqueData) {
          if (!self)
            return;
          if (self.destroyed)
            return;
          self[kGoawayCode] = errorCode;
          self[kGoawayLastStreamID] = lastStreamId;
          self[kReceivedGoaway] = true;
          self.emit("goaway", errorCode, lastStreamId, opaqueData || Buffer.allocUnsafe(0));
          self.#parser?.forEachStream(rejectStreamAboveGoawayLastId.bind(null, lastStreamId));
          const pendingRequests = self.#pendingRequests;
          self.#pendingRequests = null;
          if (pendingRequests !== null) {
            for (let i = 0;i < pendingRequests.length; i++) {
              streamRejectedByGoawaySession(pendingRequests[i].req);
            }
          }
          if (errorCode === constants.NGHTTP2_NO_ERROR) {
            self.close();
          } else if (self.closed && self.#connections === 0) {
            self.destroy();
          } else {
            self.destroy(sessionErrorFromCode(errorCode), constants.NGHTTP2_NO_ERROR);
          }
        },
        end(self, errorCode, lastStreamId, opaqueData) {
          if (!self)
            return;
          self.destroy();
        },
        altsvc(self, origin, value, streamId) {
          if (!self)
            return;
          self.emit("altsvc", value, origin, streamId);
        },
        origin(self, origin) {
          if (!self)
            return;
          if (self.encrypted) {
            const originSet = initOriginSet(self);
            if (Array.isArray(origin)) {
              for (const item of origin) {
                originSet.add(item);
              }
              self.emit("origin", origin);
            } else if (origin) {
              originSet.add(origin);
              self.emit("origin", [origin]);
            }
          }
        },
        write(self, buffer) {
          if (!self)
            return -1;
          const socket = self[bunHTTP2Socket];
          if (socket && !socket.writableEnded && self.#connected) {
            return socket.write(buffer) ? 1 : 0;
          }
          return -1;
        }
      };
      #onRead(data) {
        if (this[kTimeout])
          this[kTimeout].refresh();
        this.#parser?.read(data);
      }
      get originSet() {
        if (this.encrypted) {
          return Array.from(initOriginSet(this));
        }
      }
      get alpnProtocol() {
        return this.#alpnProtocol;
      }
      #onConnect() {
        const socket = this[bunHTTP2Socket];
        if (!socket)
          return;
        this.#connected = true;
        if (socket instanceof TLSSocket) {
          if (socket.alpnProtocol !== "h2") {
            socket.end();
            const error = codes.HTTP2_ERROR("h2 is not supported");
            this.emit("error", error);
          }
          this.#alpnProtocol = "h2";
        } else {
          this.#alpnProtocol = "h2c";
        }
        const nativeSocket = socket._handle;
        if (nativeSocket) {
          this.#parser.setNativeSocket(nativeSocket);
          this[kDeferWriteCallback] = deferWriteCallbackForSocket(nativeSocket);
        }
        process.nextTick(emitConnectNT, this, socket);
        this.#parser.flush();
        if (this.#closed) {
          const pendingRequests = this.#pendingRequests;
          this.#pendingRequests = null;
          if (pendingRequests !== null) {
            for (let i = 0;i < pendingRequests.length; i++) {
              streamRejectedByGoawaySession(pendingRequests[i].req);
            }
          }
          this.#parser?.forEachStream(streamRejectedByGoawaySession);
          this.destroy();
          return;
        }
        this.#flushPendingRequests();
      }
      #onClose() {
        const parser = this.#parser;
        const err = this.connecting ? codes.SOCKET_CLOSED() : null;
        if (parser) {
          parser.forEachStream(streamCancel);
          parser.forEachStream(streamSocketClosed);
          parser.detach();
          this.#parser = null;
        }
        this.destroy(err, NGHTTP2_NO_ERROR);
        this[bunHTTP2Socket] = null;
      }
      #onError(error) {
        if (this.destroyed) {
          return;
        }
        this[bunHTTP2Socket] = null;
        if (this.#closed) {
          this.destroy();
          return;
        }
        if (isEconnresetAfterGoaway(this, error)) {
          this.destroy();
          return;
        }
        if (this.listenerCount("error") === 0 && error?.code === "ECONNRESET") {
          this.destroy();
          return;
        }
        this.destroy(error);
      }
      #onTimeout() {
        const parser = this.#parser;
        if (parser) {
          parser.forEachStream(emitTimeout);
        }
        this.emit("timeout");
      }
      #onDrain() {
        const parser = this.#parser;
        if (parser) {
          parser.flush();
        }
      }
      get connecting() {
        const socket = this[bunHTTP2Socket];
        if (!socket) {
          return false;
        }
        return socket.connecting || false;
      }
      get connected() {
        return this[bunHTTP2Socket]?.connecting === false;
      }
      get destroyed() {
        return this[bunHTTP2Socket] === null;
      }
      get encrypted() {
        return this.#encrypted;
      }
      get closed() {
        return this.#closeCalled;
      }
      get remoteSettings() {
        if (this.destroyed || this.connecting)
          return {};
        return this.#remoteSettings ??= getDefaultSettings();
      }
      get localSettings() {
        if (this.destroyed || this.connecting)
          return {};
        return this.#localSettings ??= getDefaultSettings();
      }
      get pendingSettingsAck() {
        return this.#pendingSettingsAck;
      }
      get type() {
        return 1;
      }
      unref() {
        const socket = this[bunHTTP2Socket];
        if (typeof socket?.unref === "function")
          return socket.unref();
      }
      ref() {
        const socket = this[bunHTTP2Socket];
        if (typeof socket?.ref === "function")
          return socket.ref();
      }
      setNextStreamID(id) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        validateNumber(id, "id");
        if (id <= 0 || id > kMaxStreams)
          throw codes.OUT_OF_RANGE("id", `> 0 and <= ${kMaxStreams}`, id);
        this.#parser?.setNextStreamID(id);
      }
      setTimeout(msecs, callback) {
        return setSessionTimeout.call(this, msecs, callback);
      }
      ping(payload, callback) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        if (typeof payload === "function") {
          callback = payload;
          payload = undefined;
        }
        if (payload) {
          validateBuffer(payload, "payload");
          if (payload.byteLength !== 8)
            throw codes.HTTP2_PING_LENGTH();
        }
        validateFunction(callback, "callback");
        const cb = makeHttp2Ping(callback);
        const parser = this.#parser;
        if (!parser || !this[bunHTTP2Socket]) {
          process.nextTick(cb, codes.HTTP2_PING_CANCEL(), 0, payload);
          return false;
        }
        if (parser.ping(payload || kEmptyPingPayload) === false) {
          process.nextTick(cb, codes.HTTP2_PING_CANCEL(), 0, payload);
          return false;
        }
        if (this.#pingCallbacks) {
          this.#pingCallbacks.push([cb, Date.now()]);
        } else {
          this.#pingCallbacks = [[cb, Date.now()]];
        }
        return true;
      }
      goaway(errorCode = constants.NGHTTP2_NO_ERROR, lastStreamId = 0, opaqueData) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        return this.#parser?.goaway(errorCode, lastStreamId, opaqueData);
      }
      setLocalWindowSize(windowSize) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        validateWindowSize(windowSize);
        return this.#parser?.setLocalWindowSize?.(windowSize);
      }
      get socket() {
        const socket = this[bunHTTP2Socket];
        if (!socket)
          return;
        if (this.#socket_proxy)
          return this.#socket_proxy;
        this.#socket_proxy = new Proxy(this, proxySocketHandler);
        return this.#socket_proxy;
      }
      get state() {
        return this.#parser?.getCurrentState();
      }
      settings(settings, callback) {
        if (this.destroyed)
          throw codes.HTTP2_INVALID_SESSION();
        if (callback !== undefined && typeof callback !== "function") {
          throw codes.INVALID_ARG_TYPE("callback", "function", callback);
        }
        if (settings === undefined)
          settings = {};
        validateSettings(settings);
        this.#pendingSettingsAckCount++;
        if (this.#pendingSettingsAckCount > this.#maxOutstandingSettings) {
          this.destroy(codes.HTTP2_MAX_PENDING_SETTINGS_ACK(), constants.NGHTTP2_INTERNAL_ERROR);
          return;
        }
        this.#pendingSettingsAck = true;
        this.#parser?.settings(settings);
        this.#parser?.flush?.();
        this.#pendingSettingsCallbacks.push(typeof callback === "function" ? [callback, Date.now()] : null);
      }
      constructor(url, options, listener) {
        super();
        if (typeof options === "function") {
          listener = options;
          options = undefined;
        }
        assertIsObject(options, "options");
        options = { ...options };
        assertIsArray(options.remoteCustomSettings, "options.remoteCustomSettings");
        if (options.remoteCustomSettings) {
          options.remoteCustomSettings = [...options.remoteCustomSettings];
          if (options.remoteCustomSettings.length > MAX_ADDITIONAL_SETTINGS)
            throw codes.HTTP2_TOO_MANY_CUSTOM_SETTINGS();
        }
        if (typeof url === "string")
          url = new URL(url);
        assertIsObject(url, "authority", ["string", "Object", "URL"]);
        if (options.maxReservedRemoteStreams !== undefined) {
          this.#maxReservedRemoteStreams = options.maxReservedRemoteStreams;
        }
        if (options.strictFieldWhitespaceValidation === false) {
          this.#strictFieldWhitespaceValidation = false;
        }
        this[kStrictSingleValueFields] = options.strictSingleValueFields !== false;
        this.#url = url;
        const protocol = url.protocol || options?.protocol || "https:";
        switch (protocol) {
          case "http:":
          case "https:":
            break;
          default:
            throw codes.HTTP2_UNSUPPORTED_PROTOCOL(protocol);
        }
        const port = url.port ? parseInt(url.port, 10) : protocol === "http:" ? 80 : 443;
        let host = "localhost";
        if (url.hostname) {
          host = url.hostname;
          if (host[0] === "[")
            host = host.slice(1, -1);
        } else if (url.host) {
          host = url.host;
        }
        const authorityHost = typeof options?.servername === "string" && options.servername !== "" ? options.servername : host;
        {
          const needsBrackets = StringPrototypeIncludes.call(authorityHost, ":") && !StringPrototypeStartsWith.call(authorityHost, "[");
          this.#authority = needsBrackets ? `[${authorityHost}]:${port}` : `${authorityHost}:${port}`;
        }
        function onConnect() {
          if (this.#parser === undefined) {
            process.nextTick(onConnect.bind(this));
            return;
          }
          try {
            this.#onConnect(arguments);
            listener?.call(this, this);
          } catch (e) {
            this.destroy(e);
          }
        }
        let socket;
        if (typeof options?.maxOutstandingSettings === "number" && options.maxOutstandingSettings >= 1) {
          this.#maxOutstandingSettings = options.maxOutstandingSettings;
        }
        let connectOnNextTick = false;
        if (typeof options?.createConnection === "function") {
          socket = options.createConnection(url, options);
          this[bunHTTP2Socket] = socket;
          if (socket.connecting || socket.secureConnecting) {
            const connectEvent = socket instanceof tls.TLSSocket ? "secureConnect" : "connect";
            socket.once(connectEvent, onConnect.bind(this));
          } else {
            connectOnNextTick = true;
          }
        } else {
          socket = connectWithProtocol(protocol, options ? {
            host,
            port: String(port),
            ALPNProtocols: ["h2"],
            ...options
          } : {
            host,
            port: String(port),
            ALPNProtocols: ["h2"]
          }, onConnect.bind(this));
          this[bunHTTP2Socket] = socket;
        }
        this.#encrypted = socket instanceof TLSSocket;
        const nativeSocket = socket._handle;
        this[kDeferWriteCallback] = deferWriteCallbackForSocket(nativeSocket);
        if (options?.settings !== undefined) {
          validateSettings(options.settings);
        }
        const nativeSettings = { ...options, ...options?.settings };
        this.#localSettings = initialLocalSettings(nativeSettings);
        this.#parser = new H2FrameParser({
          native: nativeSocket,
          context: this,
          settings: nativeSettings,
          handlers: ClientHttp2Session.#Handlers
        });
        socket.on("data", this.#onRead.bind(this));
        socket.on("drain", this.#onDrain.bind(this));
        socket.on("close", this.#onClose.bind(this));
        socket.on("error", this.#onError.bind(this));
        socket.on("timeout", this.#onTimeout.bind(this));
        initHttp2SessionPerf(this, "client");
        if (connectOnNextTick) {
          process.nextTick(onConnect.bind(this));
        }
      }
      close(callback) {
        if (this.#closed || this.destroyed)
          return;
        this.#closed = true;
        this.#closeCalled = true;
        if (typeof callback === "function") {
          this.once("close", callback);
        }
        this.goaway(constants.NGHTTP2_NO_ERROR, 0, Buffer.alloc(0));
        this[kGoawaySent] = true;
        this.#parser?.flush?.();
        if (this.#connections === 0 && (this.#pendingRequests === null || this.#pendingRequests.length === 0)) {
          if (this.#pendingSettingsAckCount > 0 || this.#pingCallbacks !== null && this.#pingCallbacks.length > 0) {
            scheduleSettingsAckGraceNT(this);
          } else {
            captureHttp2PerfFrameSnapshot(this, this.#parser);
            setImmediate(destroyIfNotDestroyedNT, this);
          }
        }
      }
      destroy(error, code) {
        if (this.#destroying) {
          return;
        }
        const socket = this[bunHTTP2Socket];
        if (this.#closed && !this.#connected && !this.#parser) {
          return;
        }
        this.#destroying = true;
        emitHttp2SessionPerf(this, this.#parser, socket);
        try {
          if (this[kTimeout]) {
            clearTimeout(this[kTimeout]);
            this[kTimeout] = null;
          }
          cancelPendingPings(this.#pingCallbacks);
          this.#pingCallbacks = null;
          if (error === undefined) {
            code = undefined;
          } else if (typeof error === "number") {
            code = error;
            error = code !== constants.NGHTTP2_NO_ERROR ? codes.HTTP2_SESSION_ERROR(code) : undefined;
          }
          if (code === undefined && error != null) {
            code = constants.NGHTTP2_INTERNAL_ERROR;
          }
          if (error) {
            this[kSessionDestroyError] = error;
          }
          this.#closed = true;
          this.#connected = false;
          {
            const pendingRequests = this.#pendingRequests;
            this.#pendingRequests = null;
            if (pendingRequests !== null && pendingRequests.length > 0) {
              const cancelError = createPendingStreamCancelError(error);
              for (let i = 0;i < pendingRequests.length; i++) {
                const req = pendingRequests[i].req;
                if (!req.destroyed) {
                  req.rstCode = code !== undefined ? code : constants.NGHTTP2_CANCEL;
                  req.destroy(cancelError);
                }
              }
            }
          }
          if (socket) {
            if (!this[kGoawaySent] || code) {
              this.goaway(code || constants.NGHTTP2_NO_ERROR, 0, Buffer.alloc(0));
            }
            if (error) {
              endThenDestroySessionSocket(socket, error);
            } else {
              socket.resume();
              socket.end();
            }
          }
          const parser = this.#parser;
          if (parser) {
            if (this[kSessionDestroyError] == null && error == null) {
              this[kSessionDestroyError] = createPendingStreamCancelError();
            }
            this[bunHTTP2SessionTeardownFrame] = host.getAsyncContext();
            try {
              parser.emitErrorToAllStreams(this[kGoawayCode] || (code !== undefined ? code : constants.NGHTTP2_CANCEL));
            } finally {
              this[bunHTTP2SessionTeardownFrame] = kNoSessionTeardown;
            }
            parser.detach();
          }
        } catch (e) {
          this.#destroying = false;
          throw e;
        }
        this.#parser = null;
        this[bunHTTP2Socket] = null;
        const asyncFrame = this[bunHTTP2AsyncContextFrame];
        this[bunHTTP2AsyncContextFrame] = undefined;
        if (error) {
          runInFrame(asyncFrame, this.emit, this, "error", error);
        }
        process.nextTick(emitSessionCloseNT, this, asyncFrame);
      }
      request(headers, options) {
        let connectionsCounted = false;
        try {
          if (options !== undefined) {
            if (options.endStream !== undefined)
              validateBoolean(options.endStream, "options.endStream");
            if (options.parent !== undefined)
              validateNumber(options.parent, "options.parent");
            if (options.exclusive !== undefined)
              validateBoolean(options.exclusive, "options.exclusive");
            if (options.silent !== undefined)
              validateBoolean(options.silent, "options.silent");
          }
          if (this.destroyed) {
            const req = new ClientHttp2Stream(undefined, this, headers);
            process.nextTick(destroyWithInvalidSessionNT, req);
            return req;
          }
          if (this[kReceivedGoaway]) {
            const err = new Error("New streams cannot be created after receiving a GOAWAY");
            err.code = "ERR_HTTP2_GOAWAY_SESSION";
            throw err;
          }
          if (this.closed) {
            const err = new Error("New streams cannot be created after receiving a GOAWAY");
            err.code = "ERR_HTTP2_GOAWAY_SESSION";
            throw err;
          }
          if (this.sentTrailers) {
            throw codes.HTTP2_TRAILERS_ALREADY_SENT();
          }
          let rawHeadersList = null;
          if (headers == undefined) {
            headers = {};
          } else if (Array.isArray(headers)) {
            const raw = headers;
            let method, scheme, authority, path, protocol;
            for (let i = 0;i < raw.length; i += 2) {
              const key = raw[i];
              if (typeof key !== "string" || key.charCodeAt(0) !== 58)
                continue;
              const lowered = key.toLowerCase();
              const value = raw[i + 1];
              if (lowered === HTTP2_HEADER_METHOD)
                method = value;
              else if (lowered === HTTP2_HEADER_SCHEME)
                scheme = value;
              else if (lowered === HTTP2_HEADER_AUTHORITY)
                authority = value;
              else if (lowered === HTTP2_HEADER_PATH)
                path = value;
              else if (lowered === HTTP2_HEADER_PROTOCOL)
                protocol = value;
            }
            const additionalPseudoHeaders = [];
            if (method === undefined) {
              method = HTTP2_METHOD_GET;
              additionalPseudoHeaders.push(HTTP2_HEADER_METHOD, method);
            }
            if (method !== HTTP2_METHOD_CONNECT || protocol !== undefined) {
              let rawHasHost = false;
              for (let i = 0;i < raw.length; i += 2) {
                if (typeof raw[i] === "string" && raw[i].toLowerCase() === HTTP2_HEADER_HOST) {
                  rawHasHost = true;
                  break;
                }
              }
              if (authority === undefined && !rawHasHost) {
                authority = this.#authority;
                additionalPseudoHeaders.push(HTTP2_HEADER_AUTHORITY, authority);
              }
              if (scheme === undefined) {
                const urlProtocol = this.#url?.protocol || options?.protocol || "https:";
                scheme = urlProtocol === "http:" ? "http" : urlProtocol === "https:" ? "https" : urlProtocol;
                additionalPseudoHeaders.push(HTTP2_HEADER_SCHEME, scheme);
              }
              if (path === undefined) {
                additionalPseudoHeaders.push(HTTP2_HEADER_PATH, "/");
              }
            } else {
              if (authority === undefined)
                throw codes.HTTP2_CONNECT_AUTHORITY();
              if (scheme !== undefined)
                throw codes.HTTP2_CONNECT_SCHEME();
              if (path !== undefined)
                throw codes.HTTP2_CONNECT_PATH();
            }
            rawHeadersList = additionalPseudoHeaders.length ? additionalPseudoHeaders.concat(raw) : raw;
            const headersObject = { __proto__: null };
            for (let i = 0;i < rawHeadersList.length; i += 2) {
              const key = rawHeadersList[i];
              const value = rawHeadersList[i + 1];
              const existing = headersObject[key];
              if (existing === undefined)
                headersObject[key] = value;
              else if (Array.isArray(existing))
                existing.push(value);
              else
                headersObject[key] = [existing, value];
            }
            if (raw[sensitiveHeaders] !== undefined) {
              headersObject[sensitiveHeaders] = raw[sensitiveHeaders];
            }
            headers = headersObject;
          } else if (!isObject(headers)) {
            throw codes.INVALID_ARG_TYPE("headers", "object", headers);
          } else {
            headers = { ...headers };
          }
          if (isObject(options)) {
            options = { ...options };
          }
          if (isObject(options) && "weight" in options) {
            if (!priorityWeightDeprecationWarned) {
              priorityWeightDeprecationWarned = true;
              process.emitWarning("Priority signaling has been deprecated as of RFC 9113.", "DeprecationWarning", "DEP0194");
            }
            delete options.weight;
          }
          const sensitives = headers[sensitiveHeaders];
          if (sensitives !== undefined && !Array.isArray(sensitives)) {
            throw codes.INVALID_ARG_VALUE("headers[http2.neverIndex]", sensitives);
          }
          const sensitiveNames = buildSensitiveNames(headers, sensitives);
          {
            const headerNames = ObjectKeys(headers);
            for (let i = 0;i < headerNames.length; i++) {
              const name = headerNames[i];
              if (name === "")
                continue;
              if (name.charCodeAt(0) === 58) {
                assertValidPseudoHeader(name);
                continue;
              }
              if (!checkIsHttpToken(name)) {
                throw codes.INVALID_HTTP_TOKEN("Header name", name);
              }
            }
          }
          if (this[kStrictSingleValueFields] !== false)
            assertSingleValueHeaders(headers);
          if (sensitives !== undefined)
            headers[sensitiveHeaders] = sensitives;
          const url = this.#url;
          if (headers[":method"] === HTTP2_METHOD_CONNECT && headers[":protocol"] === undefined) {
            if (!headers[":authority"]) {
              throw codes.HTTP2_CONNECT_AUTHORITY();
            }
            if (headers[":scheme"] !== undefined) {
              throw codes.HTTP2_CONNECT_SCHEME();
            }
            if (headers[":path"] !== undefined) {
              throw codes.HTTP2_CONNECT_PATH();
            }
          }
          let method = headers[":method"];
          if (!method) {
            method = "GET";
            headers[":method"] = method;
          }
          let authority = headers[":authority"];
          if (!authority) {
            authority = this.#authority;
            if (!headers["host"]) {
              headers[":authority"] = authority;
            }
          }
          if (method !== HTTP2_METHOD_CONNECT || headers[":protocol"] !== undefined) {
            let scheme = headers[":scheme"];
            if (!scheme) {
              let protocol = url.protocol || options?.protocol || "https:";
              switch (protocol) {
                case "https:":
                  scheme = "https";
                  break;
                case "http:":
                  scheme = "http";
                  break;
                default:
                  scheme = protocol;
              }
              headers[":scheme"] = scheme;
            }
            if (headers[":path"] == undefined) {
              headers[":path"] = "/";
            }
          }
          let rejectContentLengthOnNoPayload = false;
          if (NoPayloadMethods.has(method.toUpperCase())) {
            if (!options || !isObject(options)) {
              options = { endStream: true };
            } else if (options.endStream === undefined) {
              options = { ...options, endStream: true };
            }
            if (options.endStream) {
              for (const key of Object.keys(headers)) {
                if (key.toLowerCase() === "content-length") {
                  rejectContentLengthOnNoPayload = true;
                  break;
                }
              }
            }
          }
          {
            const path = headers[":path"];
            if (typeof path === "string") {
              for (let i = 0;i < path.length; i++) {
                const c = path.charCodeAt(i);
                if (c <= 32 || c === 127) {
                  const req = new ClientHttp2Stream(undefined, this, headers);
                  req.authority = authority;
                  req[kHeadRequest] = method === HTTP2_METHOD_HEAD;
                  req.rstCode = constants.NGHTTP2_PROTOCOL_ERROR;
                  process.nextTick(emitStreamErrorNT, this, req, constants.NGHTTP2_PROTOCOL_ERROR, true, false);
                  process.nextTick(emitEventNT, req, "ready");
                  return req;
                }
              }
            }
          }
          if (rejectContentLengthOnNoPayload) {
            const req = new ClientHttp2Stream(undefined, this, headers);
            req.authority = authority;
            req[kHeadRequest] = method === HTTP2_METHOD_HEAD;
            process.nextTick(rejectNoPayloadContentLengthNT, req);
            process.nextTick(emitEventNT, req, "ready");
            return req;
          }
          let signal;
          if (isObject(options) && options.signal) {
            signal = options.signal;
            validateAbortSignal(signal, "options.signal");
            if (signal.aborted) {
              const req = new ClientHttp2Stream(undefined, this, headers);
              if (options.endStream)
                req.end();
              req.destroy(makeAbortError(undefined, { cause: signal.reason }));
              return req;
            }
          }
          const maxConcurrentStreams = this.#remoteSettings?.maxConcurrentStreams;
          if (!this.#connected || this.#pendingRequests !== null && this.#pendingRequests.length > 0 || typeof maxConcurrentStreams === "number" && this.#activeRequestCount >= maxConcurrentStreams) {
            const req = new ClientHttp2Stream(undefined, this, headers);
            req.authority = authority;
            req[kHeadRequest] = method === HTTP2_METHOD_HEAD;
            if (onClientStreamCreatedChannel.hasSubscribers) {
              onClientStreamCreatedChannel.publish({ stream: req, headers });
            }
            if (this.#pendingRequests === null) {
              this.#pendingRequests = [];
            }
            this.#pendingRequests.push({
              req,
              headers,
              wireHeaders: rawHeadersList !== null ? rawHeadersList : headers,
              sensitiveNames,
              options
            });
            req.cork();
            process.nextTick(uncorkNT, req);
            setupRequestEndAndSignal(req, options, signal);
            return req;
          }
          connectionsCounted = true;
          let stream_id = this.#parser.getNextStream();
          if (stream_id < 0) {
            const req = new ClientHttp2Stream(undefined, this, headers);
            process.nextTick(emitOutofStreamErrorNT, req);
            return req;
          }
          const req = new ClientHttp2Stream(stream_id, this, headers);
          req.authority = authority;
          req[kHeadRequest] = method === HTTP2_METHOD_HEAD;
          if (onClientStreamCreatedChannel.hasSubscribers) {
            onClientStreamCreatedChannel.publish({ stream: req, headers });
          }
          const wireHeaders = rawHeadersList !== null ? rawHeadersList : headers;
          if (typeof options === "undefined") {
            this.#parser.request(stream_id, req, wireHeaders, sensitiveNames);
          } else {
            this.#parser.request(stream_id, req, wireHeaders, sensitiveNames, options);
          }
          if (onClientStreamStartChannel.hasSubscribers) {
            onClientStreamStartChannel.publish({ stream: req, headers });
          }
          this.#trackActiveRequest(req);
          req.cork();
          process.nextTick(uncorkNT, req);
          setupRequestEndAndSignal(req, options, signal);
          process.nextTick(emitEventNT, req, "ready");
          return req;
        } catch (e) {
          if (connectionsCounted) {
            this.#connections--;
            process.nextTick(emitErrorNT, this, e, this.#connections === 0 && this.#closed);
          }
          throw e;
        }
      }
      #trackActiveRequest(req) {
        this.#activeRequestCount++;
        req.once("close", () => {
          this.#activeRequestCount--;
          this.#flushPendingRequests();
        });
      }
      #flushPendingRequests() {
        const queue = this.#pendingRequests;
        if (queue === null || queue.length === 0)
          return;
        while (queue.length > 0) {
          const parser = this.#parser;
          if (this.destroyed || !parser) {
            const { req } = queue.shift();
            if (!req.destroyed) {
              req.rstCode = constants.NGHTTP2_CANCEL;
              req.destroy();
            }
            continue;
          }
          const maxConcurrentStreams = this.#remoteSettings?.maxConcurrentStreams;
          if (typeof maxConcurrentStreams === "number" && this.#activeRequestCount >= maxConcurrentStreams) {
            break;
          }
          const { req, headers, wireHeaders, sensitiveNames, options } = queue.shift();
          if (req.destroyed)
            continue;
          const stream_id = parser.getNextStream();
          if (stream_id < 0) {
            process.nextTick(emitOutofStreamErrorNT, req);
            continue;
          }
          req[kSetStreamId](stream_id);
          try {
            if (typeof options === "undefined") {
              parser.request(stream_id, req, wireHeaders, sensitiveNames);
            } else {
              parser.request(stream_id, req, wireHeaders, sensitiveNames, options);
            }
          } catch (err) {
            this.#connections--;
            req[kNeverAnnounced] = true;
            if (!req.destroyed)
              req.destroy(err);
            continue;
          }
          if (onClientStreamStartChannel.hasSubscribers) {
            onClientStreamStartChannel.publish({ stream: req, headers });
          }
          this.#trackActiveRequest(req);
          process.nextTick(emitEventNT, req, "ready");
        }
      }
      static connect(url, options, listener) {
        return new ClientHttp2Session(url, options, listener);
      }
      get [bunHTTP2Native]() {
        return this.#parser;
      }
    }
    function connect(url, options, listener) {
      return ClientHttp2Session.connect(url, options, listener);
    }
    function setupCompat(ev) {
      if (ev === "request") {
        this.removeListener("newListener", setupCompat);
        const options = this[bunSocketServerOptions];
        const ServerRequest = options?.Http2ServerRequest || Http2ServerRequest;
        const ServerResponse = options?.Http2ServerResponse || Http2ServerResponse;
        this.on("stream", FunctionPrototypeBind.call(onServerStream, this, ServerRequest, ServerResponse));
      }
    }
    function sessionOnError(error) {
      this[kServer]?.emit("sessionError", error, this);
    }
    function sessionOnTimeout() {
      if (this.destroyed || this.closed)
        return;
      const server = this[kServer];
      if (!server.emit("timeout", this)) {
        this.destroy();
      }
    }
    function closeAllSessions(server) {
      const sessions = server[kSessions];
      if (sessions.size > 0) {
        for (const session of sessions) {
          session.close();
        }
      }
    }
    const {
      connectionListenerHTTP1,
      closeIdleHttp1Connections,
      kHttp1Connections
    } = require("internal/http1_server_fallback");
    function connectionListener(socket) {
      const options = this[bunSocketServerOptions] || {};
      if (socket.alpnProtocol === false || socket.alpnProtocol === "http/1.1") {
        if (options.allowHTTP1 === true) {
          return connectionListenerHTTP1(this, socket, options);
        }
        if (!this.emit("unknownProtocol", socket)) {
          const timer = setTimeout(() => {
            if (!socket.destroyed) {
              socket.destroy();
            }
          }, options.unknownProtocolTimeout);
          timer.unref();
          socket.once("close", () => clearTimeout(timer));
          socket.end(`HTTP/1.0 403 Forbidden\r
    ` + `Content-Type: text/plain\r
    \r
    ` + "Missing ALPN Protocol, expected `h2` to be available.\n" + "If this is a HTTP request: The server was not " + "configured with the `allowHTTP1` option or a " + "listener for the `unknownProtocol` event.\n");
        }
        return;
      }
      const session = new ServerHttp2Session(socket, options, this);
      session.on("error", sessionOnError);
      const timeout = this.timeout;
      if (timeout)
        session.setTimeout(timeout, sessionOnTimeout);
      this.emit("session", session);
      if (options.origins && Array.isArray(options.origins)) {
        try {
          session.origin(...options.origins);
        } catch (e) {
          session.emit("frameError", HTTP2_HEADER_ORIGIN, e, 0);
        }
      }
    }
    function initializeOptions(options) {
      assertIsObject(options, "options");
      options = { ...options };
      assertIsObject(options.settings, "options.settings");
      options.settings = { ...options.settings };
      assertIsArray(options.remoteCustomSettings, "options.remoteCustomSettings");
      if (options.remoteCustomSettings) {
        options.remoteCustomSettings = [...options.remoteCustomSettings];
        if (options.remoteCustomSettings.length > MAX_ADDITIONAL_SETTINGS)
          throw codes.HTTP2_TOO_MANY_CUSTOM_SETTINGS();
      }
      if (options.maxSessionInvalidFrames !== undefined)
        validateUint32(options.maxSessionInvalidFrames, "options.maxSessionInvalidFrames");
      if (options.maxSessionRejectedStreams !== undefined) {
        validateUint32(options.maxSessionRejectedStreams, "options.maxSessionRejectedStreams");
      }
      if (options.unknownProtocolTimeout !== undefined)
        validateUint32(options.unknownProtocolTimeout, "options.unknownProtocolTimeout");
      else
        options.unknownProtocolTimeout = 1e4;
      options.http1Options = { ...options.http1Options };
      if (options.Http1IncomingMessage !== undefined) {
        options.http1Options.IncomingMessage ??= options.Http1IncomingMessage;
      }
      if (options.Http1ServerResponse !== undefined) {
        options.http1Options.ServerResponse ??= options.Http1ServerResponse;
      }
      options.Http2ServerRequest ||= Http2ServerRequest;
      options.Http2ServerResponse ||= Http2ServerResponse;
      return options;
    }

    class Http2Server extends net.Server {
      timeout = 0;
      [kSessions] = new SafeSet;
      constructor(options, onRequestHandler) {
        if (typeof options === "function") {
          onRequestHandler = options;
          options = {};
        }
        options = initializeOptions(options);
        super(options);
        this[kSessions] = new SafeSet;
        this[kOptions] = { settings: options.settings || {} };
        this.setMaxListeners(0);
        this.on("connection", connectionListener);
        this.on("newListener", setupCompat);
        if (typeof onRequestHandler === "function") {
          this.on("request", onRequestHandler);
        }
      }
      setTimeout(ms, callback) {
        if (callback !== undefined && typeof callback !== "function") {
          throw codes.INVALID_ARG_TYPE("callback", "function", callback);
        }
        this.timeout = ms;
        if (typeof callback === "function") {
          this.on("timeout", callback);
        }
        return this;
      }
      updateSettings(settings) {
        assertSettings(settings);
        const options = this[bunSocketServerOptions];
        if (options) {
          options.settings = { ...options.settings, ...settings };
        }
        this[kOptions].settings = { ...this[kOptions].settings, ...settings };
      }
      close(callback) {
        super.close(callback);
        closeAllSessions(this);
      }
    }
    Http2Server.prototype[EventEmitter.captureRejectionSymbol] = function(err, event, ...args) {
      switch (event) {
        case "stream": {
          const { 0: stream } = args;
          if (stream.headersSent) {
            stream.destroy(err);
          } else {
            stream.respond({ [HTTP2_HEADER_STATUS]: 500 });
            stream.end();
          }
          break;
        }
        case "request": {
          const { 1: res } = args;
          if (!res.headersSent && !res.finished) {
            for (const name of res.getHeaderNames()) {
              res.removeHeader(name);
            }
            res.statusCode = 500;
            res.end(STATUS_CODES[500]);
          } else {
            res.destroy();
          }
          break;
        }
        default:
          break;
      }
    };
    function onErrorSecureServerSession(err, socket) {
      if (!this.emit("clientError", err, socket)) {
        if (!socket.destroyed)
          socket.destroy();
      }
    }
    function emitFrameErrorEventNT(stream, frameType, errorCode) {
      stream.emit("frameError", frameType, errorCode);
    }

    class Http2SecureServer extends tls.Server {
      timeout = 0;
      [kSessions] = new SafeSet;
      constructor(options, onRequestHandler) {
        if (typeof options !== "undefined") {
          if (options && typeof options === "object") {
            options = { ...options };
          } else {
            throw codes.INVALID_ARG_TYPE("options", "object", options);
          }
        } else {
          options = {};
        }
        const settings = options.settings;
        if (typeof settings !== "undefined") {
          validateObject(settings, "options.settings");
        }
        if (options.maxSessionInvalidFrames !== undefined)
          validateUint32(options.maxSessionInvalidFrames, "options.maxSessionInvalidFrames");
        if (options.maxSessionRejectedStreams !== undefined) {
          validateUint32(options.maxSessionRejectedStreams, "options.maxSessionRejectedStreams");
        }
        options = initializeOptions(options);
        if (!options.ALPNCallback) {
          options.ALPNProtocols = ["h2"];
          if (options.allowHTTP1 === true)
            options.ALPNProtocols.push("http/1.1");
        }
        super(options, connectionListener);
        this[kSessions] = new SafeSet;
        this[kOptions] = { settings: settings || {} };
        this.setMaxListeners(0);
        this.on("newListener", setupCompat);
        if (options.allowHTTP1 === true) {
          this[kHttp1Connections] = new SafeSet;
          const http1Options = { ...options, ...options.http1Options };
          this.keepAliveTimeout = http1Options.keepAliveTimeout ?? 5000;
          this.headersTimeout = http1Options.headersTimeout ?? 60000;
          this.requestTimeout = http1Options.requestTimeout ?? 300000;
          this.maxHeadersCount = http1Options.maxHeadersCount ?? null;
          this.maxRequestsPerSocket = http1Options.maxRequestsPerSocket ?? 0;
          this.maxHeaderSize = http1Options.maxHeaderSize;
          this.insecureHTTPParser = http1Options.insecureHTTPParser;
          this.httpValidation = http1Options.httpValidation;
        }
        if (typeof onRequestHandler === "function") {
          this.on("request", onRequestHandler);
        }
        this.on("tlsClientError", onErrorSecureServerSession);
      }
      emit(event, ...args) {
        if (event === "connection") {
          const socket = args[0];
          if (socket && !(socket instanceof TLSSocket)) {
            return upgradeRawSocketToH2(connectionListener, this, socket);
          }
        }
        return super.emit(event, ...args);
      }
      setTimeout(ms, callback) {
        if (callback !== undefined && typeof callback !== "function") {
          throw codes.INVALID_ARG_TYPE("callback", "function", callback);
        }
        this.timeout = ms;
        if (typeof callback === "function") {
          this.on("timeout", callback);
        }
        return this;
      }
      updateSettings(settings) {
        assertSettings(settings);
        const options = this[bunSocketServerOptions];
        if (options) {
          options.settings = { ...options.settings, ...settings };
        }
        this[kOptions].settings = { ...this[kOptions].settings, ...settings };
      }
      close(callback) {
        super.close(callback);
        if (this[bunSocketServerOptions]?.allowHTTP1 === true)
          this.closeIdleConnections();
        closeAllSessions(this);
      }
      closeIdleConnections() {
        if (this[bunSocketServerOptions]?.allowHTTP1 === true)
          closeIdleHttp1Connections(this);
      }
    }
    function createServer(options, onRequestHandler) {
      return new Http2Server(options, onRequestHandler);
    }
    function createSecureServer(options, onRequestHandler) {
      return new Http2SecureServer(options, onRequestHandler);
    }
    function performServerHandshake(socket, options = {}) {
      options = initializeOptions(options);
      return new ServerHttp2Session(socket, options, undefined);
    }
    function getDefaultSettings() {
      return getUnpackedSettings();
    }
    Object.defineProperty(connect, promisify.custom, {
      __proto__: null,
      value: function(authority, options) {
        const { promise, resolve, reject } = Promise.withResolvers();
        const server = connect(authority, options, () => {
          server.removeListener("error", reject);
          return resolve(server);
        });
        server.once("error", reject);
        return promise;
      }
    });
    return {
      constants,
      createServer,
      createSecureServer,
      getDefaultSettings,
      getPackedSettings,
      getUnpackedSettings,
      performServerHandshake,
      sensitiveHeaders,
      Http2ServerRequest,
      Http2ServerResponse,
      connect,
      ClientHttp2Session,
      [Symbol.for("::bunhttp2internals::")]: {
        core: {
          Http2Session,
          ServerHttp2Session,
          ClientHttp2Session,
          Http2Stream,
          ServerHttp2Stream,
          ClientHttp2Stream
        },
        util: {
          assertIsObject,
          assertIsArray,
          assertWithinRange,
          assertValidPseudoHeader,
          sessionName,
          NghttpError
        }
      }
    };
    hideFromStack([
      Http2ServerRequest,
      Http2ServerResponse,
      connect,
      createServer,
      createSecureServer,
      getDefaultSettings,
      getPackedSettings,
      getUnpackedSettings,
      ClientHttp2Session,
      ClientHttp2Stream
    ]);

  })();
})
