// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/_http_client.ts` do bun 1.4.2. Não editar à mão.
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
    const { kEmptyObject, once, ConnResetException, hasObserver, startPerf, stopPerf } = require("internal/shared");
    const kClientRequestStatistics = Symbol("ClientRequestStatistics");
    const {
      _checkIsHttpToken: checkIsHttpToken,
      freeParser,
      parsers,
      HTTPParser,
      calculateLenientFlags,
      prepareError,
      kSkipPendingData
    } = require("node:_http_common");
    const { kUniqueHeaders, parseUniqueHeadersOption, OutgoingMessage } = require("node:_http_outgoing");
    const Agent = require("node:_http_agent");
    const { urlToHttpOptions } = require("internal/url");
    const { kOutHeaders, kNeedDrain, kProxyConfig, checkShouldUseProxy } = require("internal/http");
    const { validateInteger, validateBoolean, validateString, validateOneOf } = require("internal/validators");
    const { getTimerDuration } = require("internal/timers");
    const { addAbortSignal } = require("internal/streams/add-abort-signal");
    const finished = require("internal/streams/end-of-stream");
    const dc = require("node:diagnostics_channel");
    const ObjectAssign = Object.assign;
    const ObjectDefineProperty = Object.defineProperty;
    const ObjectKeys = Object.keys;
    const NumberIsFinite = Number.isFinite;
    const ArrayIsArray = Array.isArray;
    const onClientRequestCreatedChannel = dc.channel("http.client.request.created");
    const onClientRequestStartChannel = dc.channel("http.client.request.start");
    const onClientRequestErrorChannel = dc.channel("http.client.request.error");
    const onClientResponseFinishChannel = dc.channel("http.client.response.finish");
    function emitErrorEvent(request, error) {
      if (onClientRequestErrorChannel.hasSubscribers) {
        onClientRequestErrorChannel.publish({
          request,
          error
        });
      }
      traceClientResponseEnd(request);
      request.emit("error", error);
    }
    function onCreateConnection(err, socket) {
      if (err) {
        process.nextTick(emitErrorEvent, this, err);
      } else {
        this.onSocket(socket);
      }
    }
    const INVALID_PATH_REGEX = /[^\u0021-\u00ff]/;
    const kError = Symbol("kError");
    const kPath = Symbol("kPath");
    const kPendingParserData = Symbol("kPendingParserData");
    function validateHost(host, name) {
      if (host !== null && host !== undefined && typeof host !== "string") {
        throw codes.INVALID_ARG_TYPE(`options.${name}`, "string or one of undefined or null", host);
      }
      return host;
    }

    class HTTPClientAsyncResource {
      type;
      req;
      constructor(type, req) {
        this.type = type;
        this.req = req;
      }
    }
    const kClientAsyncContext = Symbol("kClientAsyncContext");
    const runInFrame = require("internal/async_context_frame").run;
    function closeRequest(req) {
      if (req[kClientAsyncContext] !== undefined)
        req[kClientAsyncContext] = undefined;
      req._closed = true;
      req.emit("close");
    }
    function isURLInstance(input) {
      return input != null && typeof input === "object" && input instanceof URL;
    }
    function rewriteForProxiedHttp(req, reqOptions) {
      if (req._header) {
        debug("request._header is already sent, skipping rewriteForProxiedHttp", reqOptions);
        return false;
      }
      const agent = req.agent;
      if (!agent || !agent[kProxyConfig]) {
        return false;
      }
      if ((reqOptions.protocol || agent.protocol) !== "http:") {
        return false;
      }
      const shouldUseProxy = checkShouldUseProxy(agent[kProxyConfig], reqOptions);
      if (!shouldUseProxy) {
        return false;
      }
      const { auth } = agent[kProxyConfig];
      if (auth) {
        req.setHeader("proxy-authorization", auth);
      }
      if (req.shouldKeepAlive) {
        req.setHeader("proxy-connection", "keep-alive");
      } else {
        req.setHeader("proxy-connection", "close");
      }
      const requestHost = req.getHeader("host") || "localhost";
      const requestBase = `http://${requestHost}`;
      const requestURL = new URL(req.path, requestBase);
      const reqOptionsPort = reqOptions.port;
      if (reqOptionsPort) {
        requestURL.port = reqOptionsPort;
      }
      req.path = requestURL.href;
      return true;
    }
    const kHttpTraceCat = "node,node.http";
    const kTraceRequestActive = Symbol("kTraceRequestActive");
    let traceEvents = null;
    function traceClientResponseEnd(req) {
      if (req[kTraceRequestActive]) {
        req[kTraceRequestActive] = false;
        traceEvents.emitEvent("e", kHttpTraceCat, "http.client.request");
      }
    }
    function ClientRequest(input, options, cb) {
      if (!(this instanceof ClientRequest)) {
        return new ClientRequest(input, options, cb);
      }
      OutgoingMessage.call(this);
      if (typeof input === "string") {
        const urlStr = input;
        input = urlToHttpOptions(new URL(urlStr));
      } else if (isURLInstance(input)) {
        input = urlToHttpOptions(input);
      } else {
        cb = options;
        options = input;
        input = null;
      }
      if (typeof options === "function") {
        cb = options;
        options = input || kEmptyObject;
      } else {
        options = ObjectAssign({ __proto__: null }, input, options);
      }
      let agent = options.agent;
      const defaultAgent = options._defaultAgent || Agent.globalAgent;
      if (agent === false) {
        agent = new defaultAgent.constructor;
      } else if (agent === null || agent === undefined) {
        if (typeof options.createConnection !== "function") {
          agent = defaultAgent;
        }
      } else if (typeof agent.addRequest !== "function") {
        const err = codes.INVALID_ARG_TYPE("options.agent", ["Agent-like Object", "undefined", "false"], agent);
        err.message = err.message.replace(" must be of type ", " must be one of ");
        throw err;
      }
      this.agent = agent;
      const protocol = options.protocol || defaultAgent.protocol;
      let expectedProtocol = defaultAgent.protocol;
      const agentProtocol = this.agent?.protocol;
      if (agentProtocol)
        expectedProtocol = agentProtocol;
      const optionsPath = options.path;
      if (optionsPath) {
        const path = String(optionsPath);
        if (INVALID_PATH_REGEX.test(path)) {
          throw codes.UNESCAPED_CHARACTERS("Request path");
        }
      }
      if (protocol !== expectedProtocol) {
        throw codes.INVALID_PROTOCOL(protocol, expectedProtocol);
      }
      const defaultPort = options.defaultPort || this.agent?.defaultPort;
      const optsWithoutSignal = { __proto__: null, ...options };
      const port = optsWithoutSignal.port = options.port || defaultPort || 80;
      const host = optsWithoutSignal.host = validateHost(options.hostname, "hostname") || validateHost(options.host, "host") || "localhost";
      const setHost = options.setHost !== undefined ? Boolean(options.setHost) : options.setDefaultHeaders !== false;
      this._removedConnection = options.setDefaultHeaders === false;
      this._removedContLen = options.setDefaultHeaders === false;
      this._removedTE = options.setDefaultHeaders === false;
      this.socketPath = options.socketPath;
      const optionsTimeout = options.timeout;
      if (optionsTimeout !== undefined)
        this.timeout = getTimerDuration(optionsTimeout, "timeout");
      const signal = options.signal;
      if (signal) {
        addAbortSignal(signal, this);
        delete optsWithoutSignal.signal;
      }
      let method = options.method;
      if (method != null) {
        validateString(method, "options.method");
      }
      if (method) {
        if (!checkIsHttpToken(method)) {
          throw codes.INVALID_HTTP_TOKEN("Method", method);
        }
        method = this.method = method.toUpperCase();
      } else {
        method = this.method = "GET";
      }
      const maxHeaderSize = options.maxHeaderSize;
      if (maxHeaderSize !== undefined)
        validateInteger(maxHeaderSize, "maxHeaderSize", 0);
      this.maxHeaderSize = maxHeaderSize;
      const insecureHTTPParser = options.insecureHTTPParser;
      if (insecureHTTPParser !== undefined) {
        validateBoolean(insecureHTTPParser, "options.insecureHTTPParser");
      }
      this.insecureHTTPParser = insecureHTTPParser;
      const httpValidation = options.httpValidation;
      if (httpValidation !== undefined) {
        validateOneOf(httpValidation, "options.httpValidation", ["strict", "relaxed", "insecure"]);
        if (insecureHTTPParser !== undefined) {
          throw codes.INVALID_ARG_VALUE("options.httpValidation", httpValidation, "cannot be used together with options.insecureHTTPParser");
        }
      }
      this.httpValidation = httpValidation;
      const joinDuplicateHeaders = options.joinDuplicateHeaders;
      if (joinDuplicateHeaders !== undefined) {
        validateBoolean(joinDuplicateHeaders, "options.joinDuplicateHeaders");
      }
      this.joinDuplicateHeaders = joinDuplicateHeaders;
      this[kPath] = options.path || "/";
      if (cb) {
        this.once("response", cb);
      }
      if (method === "GET" || method === "HEAD" || method === "DELETE" || method === "OPTIONS" || method === "TRACE" || method === "CONNECT") {
        this.useChunkedEncodingByDefault = false;
      } else {
        this.useChunkedEncodingByDefault = true;
      }
      this._ended = false;
      this.res = null;
      this.aborted = false;
      this.timeoutCb = null;
      this.upgradeOrConnect = false;
      this.parser = null;
      this.maxHeadersCount = null;
      this.reusedSocket = false;
      this.host = host;
      this.protocol = protocol;
      const activeDomain = process.domain;
      if (activeDomain != null) {
        this.domain = activeDomain;
      }
      const thisAgent = this.agent;
      if (thisAgent) {
        if (!thisAgent.keepAlive && !NumberIsFinite(thisAgent.maxSockets)) {
          this._last = true;
          this.shouldKeepAlive = false;
        } else {
          this._last = false;
          this.shouldKeepAlive = true;
        }
      }
      const optionsHeaders = options.headers;
      const headersArray = ArrayIsArray(optionsHeaders);
      if (!headersArray) {
        if (optionsHeaders) {
          const keys = ObjectKeys(optionsHeaders);
          for (let i = 0;i < keys.length; i++) {
            const key = keys[i];
            this.setHeader(key, optionsHeaders[key]);
          }
        }
        if (host && !this.getHeader("host") && setHost) {
          let hostHeader = host;
          const posColon = hostHeader.indexOf(":");
          if (posColon !== -1 && hostHeader.includes(":", posColon + 1) && hostHeader.charCodeAt(0) !== 91) {
            hostHeader = `[${hostHeader}]`;
          }
          if (port && +port !== defaultPort) {
            hostHeader += ":" + port;
          }
          this.setHeader("Host", hostHeader);
        }
        const auth = options.auth;
        if (auth && !this.getHeader("Authorization")) {
          this.setHeader("Authorization", "Basic " + Buffer.from(auth).toString("base64"));
        }
        if (this.getHeader("expect")) {
          if (this._header) {
            throw codes.HTTP_HEADERS_SENT("render");
          }
          rewriteForProxiedHttp(this, optsWithoutSignal);
          this._storeHeader(this.method + " " + this.path + ` HTTP/1.1\r
    `, this[kOutHeaders]);
        } else {
          rewriteForProxiedHttp(this, optsWithoutSignal);
        }
      } else {
        rewriteForProxiedHttp(this, optsWithoutSignal);
        this._storeHeader(this.method + " " + this.path + ` HTTP/1.1\r
    `, optionsHeaders);
      }
      this[kUniqueHeaders] = parseUniqueHeadersOption(options.uniqueHeaders);
      if (thisAgent) {
        thisAgent.addRequest(this, optsWithoutSignal);
      } else {
        this._last = true;
        this.shouldKeepAlive = false;
        let opts = optsWithoutSignal;
        let socketPath;
        if (opts.path || (socketPath = opts.socketPath)) {
          opts = { ...optsWithoutSignal };
          socketPath ??= opts.socketPath;
          if (socketPath) {
            opts.path = socketPath;
          } else {
            opts.path &&= undefined;
          }
        }
        if (typeof opts.createConnection === "function") {
          const oncreate = once(onCreateConnection.bind(this));
          try {
            const newSocket = opts.createConnection(opts, oncreate);
            if (newSocket) {
              oncreate(null, newSocket);
            }
          } catch (err) {
            oncreate(err);
          }
        } else {
          debug("CLIENT use net.createConnection", opts);
          this.onSocket(require("node:net").createConnection(opts));
        }
      }
      if (onClientRequestCreatedChannel.hasSubscribers) {
        onClientRequestCreatedChannel.publish({
          request: this
        });
      }
      traceEvents ??= require("internal/trace_events");
      if (traceEvents.isCategoryGroupEnabled(kHttpTraceCat)) {
        traceEvents.emitEvent("b", kHttpTraceCat, "http.client.request");
        this[kTraceRequestActive] = true;
      }
    }
    toClass(ClientRequest, "ClientRequest", OutgoingMessage);
    ObjectDefineProperty(ClientRequest.prototype, "path", {
      __proto__: null,
      get() {
        return this[kPath];
      },
      set(value) {
        const path = String(value);
        if (INVALID_PATH_REGEX.test(path)) {
          throw codes.UNESCAPED_CHARACTERS("Request path");
        }
        this[kPath] = path;
      },
      configurable: true,
      enumerable: true
    });
    ClientRequest.prototype._finish = function _finish() {
      OutgoingMessage.prototype._finish.call(this);
      if (hasObserver("http")) {
        startPerf(this, kClientRequestStatistics, {
          type: "http",
          name: "HttpClient",
          detail: {
            req: {
              method: this.method,
              url: `${this.protocol}//${this.host}${this.path}`,
              headers: typeof this.getHeaders === "function" ? this.getHeaders() : {}
            }
          }
        });
      }
      if (onClientRequestStartChannel.hasSubscribers) {
        onClientRequestStartChannel.publish({
          request: this
        });
      }
    };
    ClientRequest.prototype._implicitHeader = function _implicitHeader() {
      if (this._header) {
        throw codes.HTTP_HEADERS_SENT("render");
      }
      this._storeHeader(this.method + " " + this.path + ` HTTP/1.1\r
    `, this[kOutHeaders]);
    };
    ClientRequest.prototype.abort = function abort() {
      if (this.aborted) {
        return;
      }
      this.aborted = true;
      process.nextTick(emitAbortNT, this);
      this.destroy();
    };
    ClientRequest.prototype.destroy = function destroy(err) {
      if (this.destroyed) {
        return this;
      }
      this.destroyed = true;
      traceClientResponseEnd(this);
      const res = this.res;
      if (res) {
        res._dump();
      }
      this[kError] = err;
      this.socket?.destroy(err);
      return this;
    };
    function emitAbortNT(req) {
      req.emit("abort");
    }
    function ondrain() {
      return runInFrame(this._httpMessage?.[kClientAsyncContext], ondrainInner, this);
    }
    function ondrainInner() {
      const msg = this._httpMessage;
      if (msg && !msg.finished && msg[kNeedDrain]) {
        msg[kNeedDrain] = false;
        msg.emit("drain");
      }
    }
    function socketCloseListener() {
      return runInFrame(this._httpMessage?.[kClientAsyncContext], socketCloseListenerInner, this);
    }
    function socketCloseListenerInner() {
      const socket = this;
      const req = socket._httpMessage;
      debug("HTTP socket close");
      const parser = socket.parser;
      const res = req.res;
      req.destroyed = true;
      traceClientResponseEnd(req);
      if (res) {
        if (!res.complete) {
          res.destroy(new ConnResetException("aborted"));
        }
        closeRequest(req);
        if (!res.aborted && res.readable) {
          res.push(null);
        }
      } else {
        if (!req.socket._hadError) {
          req.socket._hadError = true;
          emitErrorEvent(req, new ConnResetException("socket hang up"));
        }
        closeRequest(req);
      }
      const outputData = req.outputData;
      if (outputData)
        outputData.length = 0;
      if (parser) {
        parser.finish();
        freeParser(parser, req, socket);
      }
    }
    function socketErrorListener(err) {
      return runInFrame(this._httpMessage?.[kClientAsyncContext], socketErrorListenerInner, this, err);
    }
    function socketErrorListenerInner(err) {
      const socket = this;
      const req = socket._httpMessage;
      debug("SOCKET ERROR:", err);
      if (req) {
        socket._hadError = true;
        emitErrorEvent(req, err);
      }
      const parser = socket.parser;
      if (parser) {
        parser.finish();
        freeParser(parser, req, socket);
      }
      socket.removeListener("data", socketOnData);
      socket.removeListener("end", socketOnEnd);
      socket.destroy();
    }
    function socketOnEnd() {
      return runInFrame(this._httpMessage?.[kClientAsyncContext], socketOnEndInner, this);
    }
    function socketOnEndInner() {
      const socket = this;
      const req = this._httpMessage;
      const parser = this.parser;
      if (!req.res && !req.socket._hadError) {
        req.socket._hadError = true;
        emitErrorEvent(req, new ConnResetException("socket hang up"));
      }
      if (parser) {
        parser.finish();
        freeParser(parser, req, socket);
      }
      socket.destroy();
    }
    function socketOnData(d) {
      return runInFrame(this._httpMessage?.[kClientAsyncContext], socketOnDataInner, this, d);
    }
    function socketOnDataInner(d) {
      const socket = this;
      const pending = socket[kPendingParserData];
      if (pending !== undefined) {
        pending.push(d);
        return;
      }
      const queue = socket[kPendingParserData] = [d];
      try {
        while (queue.length !== 0) {
          const parser = socket.parser;
          if (!parser)
            break;
          processClientData(socket, queue.shift(), parser);
        }
      } finally {
        socket[kPendingParserData] = undefined;
      }
    }
    function processClientData(socket, d, parser) {
      const req = socket._httpMessage;
      assert(parser && parser.socket === socket);
      const ret = parser.execute(d);
      if (ret instanceof Error) {
        prepareError(ret, parser, d);
        debug("parse error", ret);
        freeParser(parser, req, socket);
        socket.removeListener("data", socketOnData);
        socket.removeListener("end", socketOnEnd);
        socket.destroy();
        req.socket._hadError = true;
        emitErrorEvent(req, ret);
      } else {
        const res = parser.incoming;
        if (res?.upgrade) {
          const bytesParsed = ret;
          req.res = res;
          socket.removeListener("data", socketOnData);
          socket.removeListener("end", socketOnEnd);
          socket.removeListener("drain", ondrain);
          const timeoutCb = req.timeoutCb;
          if (timeoutCb)
            socket.removeListener("timeout", timeoutCb);
          socket.removeListener("timeout", responseOnTimeout);
          parser.finish();
          freeParser(parser, req, socket);
          const bodyHead = d.slice(bytesParsed, d.length);
          const eventName = req.method === "CONNECT" ? "connect" : "upgrade";
          if (req.listenerCount(eventName) > 0) {
            req.upgradeOrConnect = true;
            socket.emit("agentRemove");
            socket.removeListener("close", socketCloseListener);
            socket.removeListener("error", socketErrorListener);
            socket._httpMessage = null;
            socket.readableFlowing = null;
            req[kClientAsyncContext] = undefined;
            req.emit(eventName, res, socket, bodyHead);
            req.destroyed = true;
            closeRequest(req);
          } else {
            socket.destroy();
          }
        } else if (res?.complete && !statusIsInformational(res.statusCode)) {
          socket.removeListener("data", socketOnData);
          socket.removeListener("end", socketOnEnd);
          socket.removeListener("drain", ondrain);
          freeParser(parser, req, socket);
        }
      }
    }
    function statusIsInformational(status) {
      return status < 200 && status >= 100 && status !== 101;
    }
    function parserOnIncomingClient(res, shouldKeepAlive) {
      const socket = this.socket;
      const req = socket._httpMessage;
      debug("AGENT incoming response!");
      const existingRes = req.res;
      if (existingRes) {
        socket.destroy();
        const parser = socket.parser;
        if (parser) {
          parser.incoming = existingRes;
          parser.incoming[kSkipPendingData] = true;
        }
        return 0;
      }
      req.res = res;
      if (res.upgrade)
        return 2;
      const method = req.method;
      if (method === "CONNECT") {
        res.upgrade = true;
        return 2;
      }
      const statusCode = res.statusCode;
      if (statusIsInformational(statusCode)) {
        req.res = null;
        if (statusCode === 100) {
          req.emit("continue");
        }
        req.emit("information", {
          statusCode,
          statusMessage: res.statusMessage,
          httpVersion: res.httpVersion,
          httpVersionMajor: res.httpVersionMajor,
          httpVersionMinor: res.httpVersionMinor,
          headers: res.headers,
          rawHeaders: res.rawHeaders
        });
        return 1;
      }
      if (req.shouldKeepAlive && !shouldKeepAlive && !req.upgradeOrConnect) {
        req.shouldKeepAlive = false;
      }
      if (req[kClientRequestStatistics] && hasObserver("http")) {
        stopPerf(req, kClientRequestStatistics, {
          detail: {
            res: {
              statusCode: res.statusCode,
              statusMessage: res.statusMessage,
              headers: res.headers
            }
          }
        });
      }
      if (onClientResponseFinishChannel.hasSubscribers) {
        onClientResponseFinishChannel.publish({
          request: req,
          response: res
        });
      }
      traceClientResponseEnd(req);
      req.res = res;
      res.req = req;
      const reqDomain = req.domain;
      if (reqDomain != null && typeof reqDomain.add === "function" && res.domain == null) {
        res.domain = reqDomain;
        reqDomain.add(res);
      }
      res.on("end", responseOnEnd);
      req.on("finish", requestOnFinish);
      socket.on("timeout", responseOnTimeout);
      if (req.aborted || !req.emit("response", res))
        res._dump();
      if (method === "HEAD")
        return 1;
      if (res.statusCode === 304) {
        res.complete = true;
        return 1;
      }
      return 0;
    }
    function responseKeepAlive(req) {
      const socket = req.socket;
      debug("AGENT socket keep-alive");
      if (req.timeoutCb) {
        socket.setTimeout(0, req.timeoutCb);
        req.timeoutCb = null;
      }
      socket.removeListener("close", socketCloseListener);
      socket.removeListener("error", socketErrorListener);
      socket.removeListener("data", socketOnData);
      socket.removeListener("end", socketOnEnd);
      process.nextTick(emitFreeNT, req);
      req.destroyed = true;
      const reqRes = req.res;
      if (reqRes) {
        reqRes.socket = null;
      }
    }
    function responseOnEnd() {
      const req = this.req;
      const socket = req.socket;
      if (socket) {
        if (req.timeoutCb)
          socket.removeListener("timeout", emitRequestTimeout);
        socket.removeListener("timeout", responseOnTimeout);
      }
      req._ended = true;
      if (!req.shouldKeepAlive) {
        if (socket.writable) {
          debug("AGENT socket.destroySoon()");
          if (typeof socket.destroySoon === "function")
            socket.destroySoon();
          else
            socket.end();
        }
        assert(!socket.writable);
      } else if (req.writableFinished && !this.aborted) {
        assert(req.finished);
        responseKeepAlive(req);
      }
    }
    function responseOnTimeout() {
      return runInFrame(this._httpMessage?.[kClientAsyncContext], responseOnTimeoutInner, this);
    }
    function responseOnTimeoutInner() {
      const req = this._httpMessage;
      if (!req)
        return;
      const res = req.res;
      if (!res)
        return;
      res.emit("timeout");
    }
    function requestOnFinish() {
      const req = this;
      if (req.shouldKeepAlive && req._ended && !req.destroyed)
        responseKeepAlive(req);
    }
    function emitFreeNT(req) {
      closeRequest(req);
      const socket = req.socket;
      if (socket) {
        socket.emit("free");
      }
    }
    function tickOnSocket(req, socket) {
      const parser = parsers.alloc();
      req.socket = socket;
      req[kClientAsyncContext] = host.getAsyncContext();
      const lenientFlags = calculateLenientFlags(req.httpValidation, req.insecureHTTPParser);
      parser.initialize(HTTPParser.RESPONSE, new HTTPClientAsyncResource("HTTPINCOMINGMESSAGE", req), req.maxHeaderSize || 0, lenientFlags);
      parser.socket = socket;
      parser.outgoing = req;
      req.parser = parser;
      socket.parser = parser;
      socket._httpMessage = req;
      const maxHeadersCount = req.maxHeadersCount;
      if (typeof maxHeadersCount === "number") {
        parser.maxHeaderPairs = maxHeadersCount << 1;
      }
      parser.joinDuplicateHeaders = req.joinDuplicateHeaders;
      parser.onIncoming = parserOnIncomingClient;
      socket.on("data", socketOnData);
      socket.on("end", socketOnEnd);
      socket.on("close", socketCloseListener);
      socket.on("drain", ondrain);
      if (req.timeout !== undefined || req.agent?.options?.timeout) {
        listenSocketTimeout(req);
      }
      req.emit("socket", socket);
    }
    function emitRequestTimeout() {
      return runInFrame(this._httpMessage?.[kClientAsyncContext], emitRequestTimeoutInner, this);
    }
    function emitRequestTimeoutInner() {
      const req = this._httpMessage;
      if (req) {
        req.emit("timeout");
      }
    }
    function onSocketListenTimeout(socket) {
      socket.once("timeout", emitRequestTimeout);
    }
    function listenSocketTimeout(req) {
      if (req.timeoutCb) {
        return;
      }
      req.timeoutCb = emitRequestTimeout;
      const reqSocket = req.socket;
      if (reqSocket) {
        reqSocket.once("timeout", emitRequestTimeout);
      } else {
        req.on("socket", onSocketListenTimeout);
      }
    }
    ClientRequest.prototype.onSocket = function onSocket(socket, err) {
      if (socket && !err) {
        socket._httpMessage = this;
        this[kClientAsyncContext] = host.getAsyncContext();
        socket.on("error", socketErrorListener);
      }
      process.nextTick(onSocketNT, this, socket, err);
    };
    function destroyRequestOnSocketNT(req, socket, err) {
      if (!req.aborted && !err) {
        err = new ConnResetException("socket hang up");
      }
      if (err && err.code !== "ERR_PROXY_TUNNEL" && !socket?._hadError) {
        emitErrorEvent(req, err);
      }
      traceClientResponseEnd(req);
      closeRequest(req);
    }
    function onSocketNT(req, socket, err) {
      if (req.destroyed || err) {
        req.destroyed = true;
        if (socket) {
          if (!err && req.agent && !socket.destroyed) {
            socket.emit("free");
            socket.removeListener("error", socketErrorListener);
          } else {
            finished(socket.destroy(err || req[kError]), onSocketFinishedDestroy.bind(undefined, req, socket, err));
            return;
          }
        }
        destroyRequestOnSocketNT(req, socket, err || req[kError]);
      } else {
        tickOnSocket(req, socket);
        req._flush();
      }
    }
    function onSocketFinishedDestroy(req, socket, err, er) {
      if (er?.code === "ERR_STREAM_PREMATURE_CLOSE") {
        er = null;
      }
      destroyRequestOnSocketNT(req, socket, er || err);
    }
    function callSocketMethod(method, arguments_) {
      if (method)
        this.socket[method].apply(this.socket, arguments_);
    }
    function deferToConnectOnSocket(method, arguments_) {
      if (this.socket.writable) {
        callSocketMethod.call(this, method, arguments_);
      } else {
        this.socket.once("connect", callSocketMethod.bind(this, method, arguments_));
      }
    }
    ClientRequest.prototype._deferToConnect = _deferToConnect;
    function _deferToConnect(method, arguments_) {
      if (!this.socket) {
        this.once("socket", deferToConnectOnSocket.bind(this, method, arguments_));
      } else {
        deferToConnectOnSocket.call(this, method, arguments_);
      }
    }
    ClientRequest.prototype.setTimeout = function setTimeout(msecs, callback) {
      if (this._ended) {
        return this;
      }
      listenSocketTimeout(this);
      msecs = getTimerDuration(msecs, "msecs");
      if (callback)
        this.once("timeout", callback);
      const socket = this.socket;
      if (socket) {
        setSocketTimeout(socket, msecs);
      } else {
        this.once("socket", onSocketSetTimeout.bind(undefined, msecs));
      }
      return this;
    };
    function onSocketSetTimeout(msecs, sock) {
      setSocketTimeout(sock, msecs);
    }
    function setSocketTimeout(sock, msecs) {
      if (sock.connecting) {
        sock.once("connect", function() {
          sock.setTimeout(msecs);
        });
      } else {
        sock.setTimeout(msecs);
      }
    }
    ClientRequest.prototype.setNoDelay = function setNoDelay(noDelay) {
      this._deferToConnect("setNoDelay", [noDelay]);
    };
    ClientRequest.prototype.setSocketKeepAlive = function setSocketKeepAlive(enable, initialDelay) {
      this._deferToConnect("setKeepAlive", [enable, initialDelay]);
    };
    ClientRequest.prototype.clearTimeout = function clearTimeout(cb) {
      this.setTimeout(0, cb);
    };
    return {
      ClientRequest
    };
  })();
})
