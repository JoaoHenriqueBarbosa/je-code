// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/_http_server.ts` do bun 1.4.2. Não editar à mão.
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
    const EventEmitter = require("node:events");
    const { Stream } = require("node:stream");
    const {
      _checkInvalidHeaderChar: checkInvalidHeaderChar,
      chunkExpression,
      validateHeaderName,
      validateHeaderValue,
      HTTPParser,
      calculateLenientFlags
    } = require("node:_http_common");
    const {
      validateObject,
      validateLinkHeaderValue,
      validateBoolean,
      validateInteger,
      validateFunction,
      validateOneOf
    } = require("internal/validators");
    const { ConnResetException, hasObserver, startPerf, stopPerf, kInternalSendOptions } = require("internal/shared");
    const kServerResponseStatistics = Symbol("ServerResponseStatistics");
    const { isPrimary } = require("internal/cluster/isPrimary");
    const {
      kInternalSocketData,
      serverSymbol,
      kHandle,
      kRealListen,
      tlsSymbol,
      optionsSymbol,
      headerStateSymbol,
      NodeHTTPHeaderState,
      kPendingCallbacks,
      kRequest,
      kCloseCallback,
      NodeHTTPResponseFlags,
      callCloseCallback,
      emitCloseNT,
      NodeHTTPResponseAbortEvent,
      STATUS_CODES,
      isTlsSymbol,
      hasServerResponseFinished,
      NodeHTTPBodyReadState,
      eofInProgress,
      drainMicrotasks,
      setServerCustomOptions,
      setServerAppFlags,
      getMaxHTTPHeaderSize,
      fakeSocketSymbol,
      noBodySymbol,
      kOutHeaders,
      onDataIncomingMessage,
      validateMsecs
    } = require("internal/http");
    const { FakeSocket } = require("internal/http/FakeSocket");
    const NumberIsNaN = Number.isNaN;
    const { IncomingMessage, kReqShouldKeepAlive } = require("node:_http_incoming");
    const {
      OutgoingMessage,
      kErrored,
      kHighWaterMark,
      kSocket,
      kRejectNonStandardBodyWrites,
      kUniqueHeaders,
      parseUniqueHeadersOption
    } = require("node:_http_outgoing");
    const OutgoingMessagePrototype = OutgoingMessage.prototype;
    const { kIncomingMessage } = require("node:_http_common");
    let http1Fallback;
    const kConnectionsCheckingInterval = Symbol("http.server.connectionsCheckingInterval");
    const kTrackedConnections = Symbol("http.server.trackedConnections");
    const kHttpAllowHalfOpen = Symbol("http.server.httpAllowHalfOpen");
    const kHttpTraceCat = "node,node.http";
    let traceEvents = null;
    function traceServerRequestStart(http_res) {
      traceEvents ??= require("internal/trace_events");
      if (!traceEvents.isCategoryGroupEnabled(kHttpTraceCat))
        return;
      traceEvents.emitEvent("b", kHttpTraceCat, "http.server.request");
      http_res.once("finish", traceServerRequestEnd);
    }
    function traceServerRequestEnd() {
      traceEvents.emitEvent("e", kHttpTraceCat, "http.server.request");
    }
    const getBunServerAllClosedPromise = notImplemented("http.Server close");
    const kServerResponse = Symbol("ServerResponse");
    const kChunkedEncoding = Symbol("kChunkedEncoding");
    const kShouldKeepAlive = Symbol("kShouldKeepAlive");
    const kOptimizeEmptyRequests = Symbol("kOptimizeEmptyRequests");
    const kEmptyBuffer = Buffer.alloc(0);
    const ObjectKeys = Object.keys;
    const MathMin = Math.min;
    const MathFloor = Math.floor;
    const DateNow = Date.now;
    let cluster;
    function emitCloseServer(self) {
      callCloseCallback(self);
      self.emit("close");
    }
    function emitCloseNTServer() {
      process.nextTick(emitCloseServer, this);
    }
    function setCloseCallback(self, callback) {
      if (callback === self[kCloseCallback]) {
        return;
      }
      if (self[kCloseCallback]) {
        throw new Error("Close callback already set");
      }
      self[kCloseCallback] = callback;
    }
    function assignSocketInternal(self, socket) {
      if (socket._httpMessage) {
        throw codes.HTTP_SOCKET_ASSIGNED("Socket already assigned");
      }
      socket._httpMessage = self;
      setCloseCallback(socket, onServerResponseClose);
      self.socket = socket;
      self.emit("socket", socket);
    }
    function onServerResponseClose() {
      const httpMessage = this._httpMessage;
      if (httpMessage) {
        emitCloseNT(httpMessage);
      }
    }
    function strictContentLength(response) {
      if (response.strictContentLength) {
        let contentLength = response._contentLength ?? response.getHeader("content-length");
        if (contentLength && response._hasBody && !response._removedContLen && !response.chunkedEncoding && !response.hasHeader("transfer-encoding")) {
          if (typeof contentLength === "number") {
            return contentLength;
          } else if (typeof contentLength === "string") {
            contentLength = parseInt(contentLength, 10);
            if (NumberIsNaN(contentLength)) {
              return;
            }
            return contentLength;
          }
        }
      }
    }
    const kParserOnTimeout = HTTPParser.kOnTimeout | 0;
    function serverParserShimFree() {}
    function serverParserShimClose() {}
    function serverParserShimOnTimeout() {}
    function createServerParserShim(socket) {
      return {
        incoming: null,
        outgoing: null,
        socket,
        free: serverParserShimFree,
        close: serverParserShimClose,
        [kParserOnTimeout]: serverParserShimOnTimeout
      };
    }
    function releaseServerParserShim(socket, req) {
      const parser = socket.parser;
      if (parser == null)
        return;
      socket.parser = null;
      parser.incoming = null;
      parser.socket = null;
      parser[kParserOnTimeout] = null;
      parser.free();
      if (req != null && req.parser != null)
        req.parser = null;
    }
    function onNodeHTTPServerSocketTimeout() {
      const req = this[kRequest];
      const reqTimeout = req && !req.complete && !req[noBodySymbol] && req.emit("timeout", this);
      const res = this._httpMessage;
      const resTimeout = res && res.emit("timeout", this);
      const serverTimeout = this.server.emit("timeout", this);
      if (!reqTimeout && !resTimeout && !serverTimeout)
        this.destroy();
    }
    function emitListeningNextTick(self, hostname, port) {
      if (!self[serverSymbol])
        return;
      self.emit("listening", null, hostname, port);
    }
    function emitListenErrorNextTick(self, err) {
      self.emit("error", err);
    }
    function normalizeServerTls(tls) {
      const requestCert = !!tls.requestCert;
      tls.requestCert = requestCert;
      tls.rejectUnauthorized = requestCert ? tls.rejectUnauthorized !== false : false;
      return tls;
    }
    function connectionListener(socket) {
      if (NodeHTTPServerSocket && socket instanceof NodeHTTPServerSocket)
        return;
      (http1Fallback ??= require("internal/http1_server_fallback")).connectionListenerHTTP1(this, socket, {
        http1Options: {
          IncomingMessage: this[kIncomingMessage],
          ServerResponse: this[kServerResponse]
        }
      });
    }
    function Server(options, callback) {
      if (!(this instanceof Server))
        return new Server(options, callback);
      EventEmitter.call(this);
      this.on("listening", setupConnectionsTracking);
      this.on("connection", connectionListener);
      this.prependListener("connection", (socket) => {
        if (socket != null && typeof socket === "object")
          socket.server = this;
      });
      this.listening = false;
      this._unref = false;
      this.timeout = 0;
      this.maxRequestsPerSocket = 0;
      this.maxHeadersCount = null;
      defineHttpAllowHalfOpen(this);
      this[kInternalSocketData] = undefined;
      this[kTrackedConnections] = new Set;
      this[tlsSymbol] = null;
      this.noDelay = true;
      if (typeof options === "function") {
        callback = options;
        options = {};
      } else if (options == null) {
        options = {};
      } else {
        validateObject(options, "options");
        options = { ...options };
        const tlsHelpers = options.pfx || options.cert || options.key || options.ca ? require("internal/tls") : undefined;
        let tlsOptions = options;
        if (options.pfx) {
          tlsOptions = tlsHelpers.processPfxOptions(options);
          this[isTlsSymbol] = true;
        }
        let cert = tlsOptions.cert;
        if (cert) {
          tlsHelpers.throwOnInvalidTLSArray("options.cert", cert);
          this[isTlsSymbol] = true;
        }
        let key = tlsOptions.key;
        if (key) {
          tlsHelpers.throwOnInvalidTLSArray("options.key", key);
          this[isTlsSymbol] = true;
        }
        let ca = tlsOptions.ca;
        const pfxExtraCAs = tlsOptions._pfxExtraCACerts;
        if (pfxExtraCAs?.length) {
          ca = ca == null ? pfxExtraCAs : Array.isArray(ca) ? [...ca, ...pfxExtraCAs] : [ca, ...pfxExtraCAs];
        }
        if (ca) {
          tlsHelpers.throwOnInvalidTLSArray("options.ca", ca);
          this[isTlsSymbol] = true;
        }
        let passphrase = options.passphrase;
        if (passphrase && typeof passphrase !== "string") {
          throw codes.INVALID_ARG_TYPE("options.passphrase", "string", passphrase);
        }
        let serverName = options.servername;
        if (serverName && typeof serverName !== "string") {
          throw codes.INVALID_ARG_TYPE("options.servername", "string", serverName);
        }
        let secureOptions = options.secureOptions || 0;
        if (secureOptions && typeof secureOptions !== "number") {
          throw codes.INVALID_ARG_TYPE("options.secureOptions", "number", secureOptions);
        }
        if (this[isTlsSymbol]) {
          const { validateSecureProtocol, secureProtocolToVersionRange, tlsStringToProtocolVersion } = tlsHelpers;
          validateSecureProtocol(options.secureProtocol);
          let minVersion, maxVersion;
          const range = secureProtocolToVersionRange(options.secureProtocol);
          if (range) {
            minVersion = range[0];
            maxVersion = range[1];
          } else {
            minVersion = tlsStringToProtocolVersion(options.minVersion);
            maxVersion = tlsStringToProtocolVersion(options.maxVersion);
          }
          this[tlsSymbol] = normalizeServerTls({
            serverName,
            key,
            cert,
            ca,
            passphrase,
            secureOptions,
            minVersion,
            maxVersion,
            ciphers: typeof options.ciphers === "string" && options.ciphers ? options.ciphers : undefined,
            requestCert: options.requestCert,
            rejectUnauthorized: options.rejectUnauthorized
          });
        } else {
          this[tlsSymbol] = null;
        }
      }
      this[optionsSymbol] = options;
      storeHTTPOptions.call(this, options);
      if (callback)
        this.on("request", callback);
      return this;
    }
    toClass(Server, "Server", EventEmitter);
    Server.prototype[kIncomingMessage] = undefined;
    Server.prototype[kServerResponse] = undefined;
    Server.prototype[kConnectionsCheckingInterval] = undefined;
    function rethrowUncaught(err) {
      throw err;
    }
    function setupConnectionsTracking() {
      if (this[kConnectionsCheckingInterval]) {
        clearInterval(this[kConnectionsCheckingInterval]);
      }
      const delay = this.connectionsCheckingInterval ?? 30000;
      this[kConnectionsCheckingInterval] = setInterval(checkConnections.bind(this), delay);
      this[kConnectionsCheckingInterval].unref();
    }
    function checkConnections() {
      const headersTimeout = this.headersTimeout;
      const requestTimeout = this.requestTimeout;
      if (headersTimeout === 0 && requestTimeout === 0) {
        return;
      }
      const connections = this[kTrackedConnections];
      if (!connections || connections.size === 0) {
        return;
      }
      for (const socket of connections) {
        const handle = socket[kHandle];
        if (handle?.isRequestTimedOut?.(headersTimeout, requestTimeout)) {
          onRequestTimeout(socket);
        }
      }
    }
    function onRequestTimeout(socket) {
      socketOnError.call(socket, codes.HTTP_REQUEST_TIMEOUT("Request timeout"));
    }
    Server.prototype.ref = function() {
      this._unref = false;
      this[serverSymbol]?.ref?.();
      return this;
    };
    Server.prototype.unref = function() {
      this._unref = true;
      this[serverSymbol]?.unref?.();
      return this;
    };
    Server.prototype.closeAllConnections = function() {
      http1Fallback?.closeAllHttp1Connections(this);
      const server = this[serverSymbol];
      if (!server) {
        return;
      }
      this[serverSymbol] = undefined;
      clearInterval(this[kConnectionsCheckingInterval]);
      this.listening = false;
      server.stop(true);
    };
    Server.prototype.getConnections = function(callback) {
      const count = this[kTrackedConnections]?.size ?? 0;
      if (typeof callback === "function") {
        process.nextTick(callback, null, count);
      }
      return this;
    };
    Server.prototype.closeIdleConnections = function() {
      http1Fallback?.closeIdleHttp1Connections(this);
      const server = this[serverSymbol];
      server?.closeIdleConnections();
    };
    Server.prototype.close = function(optionalCallback) {
      const server = this[serverSymbol];
      clearInterval(this[kConnectionsCheckingInterval]);
      http1Fallback?.closeIdleHttp1Connections(this);
      if (!server) {
        if (typeof optionalCallback === "function")
          process.nextTick(optionalCallback, codes.SERVER_NOT_RUNNING());
        return this;
      }
      this[serverSymbol] = undefined;
      if (typeof optionalCallback === "function")
        setCloseCallback(this, optionalCallback);
      this.listening = false;
      server.closeIdleConnections();
      server.stop();
      return this;
    };
    Server.prototype[EventEmitter.captureRejectionSymbol] = function(err, event, ...args) {
      switch (event) {
        case "request": {
          const { 1: res } = args;
          if (!res.headersSent && !res.writableEnded) {
            const names = res.getHeaderNames();
            for (let i = 0;i < names.length; i++) {
              res.removeHeader(names[i]);
            }
            res.statusCode = 500;
            res.end(STATUS_CODES[500]);
          } else {
            res.destroy();
          }
          break;
        }
        default:
          const { 1: res } = args;
          res?.socket?.destroy();
          break;
      }
    };
    Server.prototype[Symbol.asyncDispose] = function() {
      const { resolve, reject, promise } = Promise.withResolvers();
      this.close(function(err, ...args) {
        if (err) {
          reject(err);
        } else
          resolve(...args);
      });
      return promise;
    };
    Server.prototype.address = function() {
      if (!this[serverSymbol])
        return null;
      return this[serverSymbol].address;
    };
    Server.prototype.listen = function() {
      const server = this;
      let port, host;
      let socketPath;
      let tls = this[tlsSymbol];
      const argc = arguments.length;
      if (argc > 0) {
        const arg0 = arguments[0];
        if ((isObject(arg0) || (typeof arg0 === "function")) && arg0 !== null) {
          port = arg0.port;
          host = arg0.host;
          socketPath = arg0.path;
          const otherTLS = arg0.tls;
          if (otherTLS && isObject(otherTLS)) {
            tls = normalizeServerTls({ ...otherTLS });
          }
        } else if (typeof arg0 === "string" && !(Number(arg0) >= 0)) {
          socketPath = arg0;
        } else {
          port = arg0;
          if (argc > 1 && typeof arguments[1] === "string") {
            host = arguments[1];
          }
        }
      }
      if (port === undefined && !socketPath) {
        port = 0;
      }
      if (typeof port === "string") {
        const portNumber = parseInt(port);
        if (!Number.isNaN(portNumber)) {
          port = portNumber;
        }
      }
      const lastArg = arguments[argc - 1];
      if ((typeof lastArg === "function")) {
        this.once("listening", lastArg);
      }
      try {
        if (isPrimary) {
          server[kRealListen](tls, port, host, socketPath, false);
          return this;
        }
        if (cluster === undefined)
          cluster = require("node:cluster");
        server.once("listening", () => {
          if (!process.connected)
            return;
          cluster.worker.state = "listening";
          const address = server.address();
          const isObjectAddress = address !== null && typeof address === "object";
          const boundHost = host && isObjectAddress ? address : null;
          const message = {
            cmd: "NODE_CLUSTER",
            act: "listening",
            port: socketPath ? -1 : isObjectAddress && address.port || port,
            data: null,
            address: socketPath ?? (boundHost && boundHost.address) ?? null,
            addressType: socketPath ? -1 : boundHost && boundHost.family === "IPv6" ? 6 : 4
          };
          process.send(message, undefined, kInternalSendOptions);
        });
        server[kRealListen](tls, port, host, socketPath, true);
      } catch (err) {
        process.nextTick(emitListenErrorNextTick, server, err);
      }
      return this;
    };
    Server.prototype[kRealListen] = function(tls, port, host, socketPath, reusePort) {
      {
        const ResponseClass = this[optionsSymbol].ServerResponse || ServerResponse;
        const RequestClass = this[optionsSymbol].IncomingMessage || IncomingMessage;
        const canUseInternalAssignSocket = ResponseClass?.prototype.assignSocket === ServerResponse.prototype.assignSocket;
        let server = this;
        if (tls) {
          this.serverName = tls.serverName || host || "localhost";
        }
        this[serverSymbol] = Bun.serve({
          idleTimeout: 0,
          tls,
          port,
          hostname: host,
          unix: socketPath,
          reusePort,
          websocket: {
            open(ws) {
              ws.data.open(ws);
            },
            message(ws, message) {
              ws.data.message(ws, message);
            },
            close(ws, code, reason) {
              ws.data.close(ws, code, reason);
            },
            drain(ws) {
              ws.data.drain(ws);
            },
            ping(ws, data) {
              ws.data.ping(ws, data);
            },
            pong(ws, data) {
              ws.data.pong(ws, data);
            }
          },
          maxRequestBodySize: Number.MAX_SAFE_INTEGER,
          onNodeHTTPRequest(bunServer, url, method, dispatchBits, handle, hasBody, socketHandle, isSocketNew, socket, isAncientHTTP, connectHead, isPipelinedDispatch) {
            if (!socket) {
              socket = new (getNodeHTTPServerSocket())(server, socketHandle, !!tls);
            }
            if (socket[kKeepAliveTimeoutSet]) {
              socket[kKeepAliveTimeoutSet] = false;
              const serverTimeout = server.timeout;
              if (serverTimeout) {
                socket.setTimeout(serverTimeout);
              } else {
                socket.timeout = 0;
              }
            } else {
              socket._unrefTimer();
            }
            const http_req = new RequestClass(kHandle, url, method, undefined, undefined, handle, hasBody, socket);
            if (isAncientHTTP) {
              http_req.httpVersion = "1.0";
              http_req.httpVersionMajor = 1;
              http_req.httpVersionMinor = 0;
            }
            http_req[kReqShouldKeepAlive] = isAncientHTTP ? false : (dispatchBits & DISPATCH_CONN_CLOSE) === 0;
            if (server.joinDuplicateHeaders) {
              http_req.joinDuplicateHeaders = true;
            }
            let socketParser = socket.parser;
            if (socketParser == null) {
              socketParser = socket.parser = createServerParserShim(socket);
            }
            socketParser.incoming = http_req;
            socketParser[kParserOnTimeout] = serverParserShimOnTimeout;
            const isPipelined = !!isPipelinedDispatch;
            if (method === "CONNECT" && !isPipelined) {
              if (server.listenerCount("connect") > 0) {
                socket[kEnableStreaming](true);
                socketHandle.upgradeToTunnel();
                detachSocketListenersForHandoff(socket);
                const promise = newPromise();
                const head = connectHead ? connectHead : kEmptyBuffer;
                http_req.upgrade = true;
                releaseServerParserShim(socket, http_req);
                server.emit("connect", http_req, socket, head);
                socket.once("close", resolveHandoffPromise.bind(undefined, promise));
                return promise;
              } else {
                socketHandle.close();
              }
              return;
            }
            socket[kEnableStreaming](false);
            let http_res;
            if (ResponseClass === ServerResponse) {
              scratchResponseOptions[kHandle] = handle;
              scratchResponseOptions.highWaterMark = socket.writableHighWaterMark;
              scratchResponseOptions[kRejectNonStandardBodyWrites] = server.rejectNonStandardBodyWrites;
              http_res = new ResponseClass(http_req, scratchResponseOptions);
              scratchResponseOptions[kHandle] = undefined;
            } else {
              http_res = new ResponseClass(http_req, {
                [kHandle]: handle,
                highWaterMark: socket.writableHighWaterMark,
                [kRejectNonStandardBodyWrites]: server.rejectNonStandardBodyWrites
              });
            }
            http_res._keepAliveTimeout = server.keepAliveTimeout;
            const uniqueHeaders = server[kUniqueHeaders];
            if (uniqueHeaders != null)
              http_res[kUniqueHeaders] = uniqueHeaders;
            if (!http_req[kReqShouldKeepAlive]) {
              http_res[kMustCloseConnection] = true;
            }
            http_res.on("finish", emitResponseFinish);
            if (hasObserver("http")) {
              startPerf(http_res, kServerResponseStatistics, {
                type: "http",
                name: "HttpRequest",
                detail: {
                  req: {
                    method: http_req.method,
                    url: http_req.url,
                    headers: http_req.headers
                  }
                }
              });
              http_res.once("finish", stopServerResponsePerf);
            }
            handle.onabort = socket[kBoundOnAbort] ??= onServerRequestEvent.bind(socket);
            if (hasBody) {
              handle.ondata = onDataIncomingMessage.bind(http_req);
              handle.hasCustomOnData = false;
            }
            drainMicrotasks();
            let pendingPromise;
            let didFinish = false;
            const isRequestsLimitSet = typeof server.maxRequestsPerSocket === "number" && server.maxRequestsPerSocket > 0;
            let reachedRequestsLimit = false;
            if (isRequestsLimitSet) {
              const requestCount = (socket._requestCount || 0) + 1;
              socket._requestCount = requestCount;
              http_res._maxRequestsPerSocket = server.maxRequestsPerSocket;
              http_res.maxRequestsOnConnectionReached = server.maxRequestsPerSocket <= requestCount;
              if (server.maxRequestsPerSocket < requestCount) {
                reachedRequestsLimit = true;
              }
            }
            if (isSocketNew && !reachedRequestsLimit) {
              server.emit("connection", socket);
            }
            if (!isPipelined) {
              socket[kRequest] = http_req;
            }
            let is_upgrade = false;
            if (!isPipelined && (dispatchBits & DISPATCH_HAS_UPGRADE) !== 0 && (dispatchBits & DISPATCH_CONN_UPGRADE) !== 0) {
              is_upgrade = !!server.shouldUpgradeCallback(http_req);
            }
            http_req.upgrade = is_upgrade;
            if (isPipelined) {
              http_res[kPipelinedQueuedState] = {
                ops: [],
                bytes: 0,
                headerBytes: 0,
                needDrain: false,
                ended: false,
                isAncient: !!isAncientHTTP,
                socket
              };
              (socket[kPipelinedResponses] ??= []).push(http_res);
              if (socket._httpMessage == null && !socket[kPipelineKickScheduled]) {
                socket[kPipelineKickScheduled] = true;
                process.nextTick(advancePipelineIfIdleNT, server, socket);
              }
              if (!socket._paused && (socket[kOutgoingData] ?? 0) >= socket.writableHighWaterMark) {
                pausePipelineReads(socket);
              }
            } else if (!is_upgrade) {
              if (!socket[kStopParsingOnCloseListener]) {
                socket[kStopParsingOnCloseListener] = true;
                socket.on("close", onSocketCloseStopParsing);
              }
              if (canUseInternalAssignSocket) {
                assignSocketInternal(http_res, socket);
              } else {
                http_res.assignSocket(socket);
              }
            }
            function onClose() {
              didFinish = true;
              if (pendingPromise)
                resolvePromise(pendingPromise, undefined);
            }
            if (!isPipelined) {
              setCloseCallback(http_res, onClose);
            }
            if (!is_upgrade)
              traceServerRequestStart(http_res);
            if (server[kOptimizeEmptyRequests] && !is_upgrade && (dispatchBits & (DISPATCH_HAS_CONTENT_LENGTH | DISPATCH_HAS_TRANSFER_ENCODING)) === 0) {
              http_req._dumpAndCloseReadable();
            }
            if (reachedRequestsLimit) {
              server.emit("dropRequest", http_req, socket);
              http_res.writeHead(503);
              http_res.end();
              if (isPipelined) {
                http_res[kMustCloseConnection] = true;
              } else {
                socket.destroy();
              }
            } else if (is_upgrade) {
              socketHandle.upgradeToTunnel(hasBody);
              socket[kEnableStreaming](true);
              detachSocketListenersForHandoff(socket);
              releaseServerParserShim(socket, http_req);
              if (hasBody) {
                socket[kUpgradeIncoming] = http_req;
                http_req.once("end", clearUpgradeIncoming.bind(undefined, socket));
              }
              const upgradeHead = !hasBody && connectHead ? connectHead : kEmptyBuffer;
              let upgradeHandled;
              try {
                upgradeHandled = server.emit("upgrade", http_req, socket, upgradeHead);
              } catch (err) {
                process.nextTick(rethrowUncaught, err);
                upgradeHandled = true;
              }
              if (!upgradeHandled) {
                socket.destroy();
                return;
              }
              const upgradePromise = newPromise();
              socket.once("close", resolveHandoffPromise.bind(undefined, upgradePromise));
              return upgradePromise;
            } else if (server.requireHostHeader && (dispatchBits & DISPATCH_HAS_HOST) === 0 && http_req.httpVersionMajor === 1 && http_req.httpVersionMinor >= 1) {
              http_res.writeHead(400, { Connection: "close" });
              http_res.end();
            } else {
              if ((dispatchBits & DISPATCH_HAS_EXPECT) !== 0) {
                if ((dispatchBits & DISPATCH_EXPECT_CONTINUE) !== 0) {
                  if (server.listenerCount("checkContinue") > 0) {
                    server.emit("checkContinue", http_req, http_res);
                  } else {
                    http_res.writeContinue();
                    server.emit("request", http_req, http_res);
                  }
                } else if (server.listenerCount("checkExpectation") > 0) {
                  server.emit("checkExpectation", http_req, http_res);
                } else {
                  http_res.writeHead(417);
                  http_res.end();
                }
              } else {
                server.emit("request", http_req, http_res);
              }
            }
            socket.cork();
            if (isPipelined) {
              return;
            }
            if (handle.finished || didFinish) {
              handle = undefined;
              http_res[kCloseCallback] = undefined;
              http_res[kDispatcherDetached] = true;
              http_res.detachSocket(socket);
              if (socket[kPipelinedResponses] !== undefined) {
                advanceResponsePipeline(server, socket);
              }
              return;
            }
            pendingPromise = newPromise();
            return pendingPromise;
          }
        });
        this.listening = true;
        getBunServerAllClosedPromise(this[serverSymbol]).then(emitCloseNTServer.bind(this));
        applyServerCustomOptions(this);
        if (this?._unref) {
          this[serverSymbol]?.unref?.();
        }
        process.nextTick(emitListeningNextTick, this, this[serverSymbol]?.hostname, this[serverSymbol]?.port);
      }
    };
    function applyServerCustomOptions(server) {
      const handle = server[serverSymbol];
      if (!handle)
        return;
      setServerCustomOptions(handle, server.requireHostHeader, true, serverLenientFlags(server), typeof server.maxHeaderSize !== "undefined" ? server.maxHeaderSize : getMaxHTTPHeaderSize(), onServerClientError.bind(server), onServerConnection.bind(server), !!server.httpAllowHalfOpen);
    }
    function serverLenientFlags(server) {
      const lenient = calculateLenientFlags(server.httpValidation, server.insecureHTTPParser);
      if (lenient === HTTPParser.kLenientNone)
        return 0;
      return lenient === HTTPParser.kLenientAll ? 3 : 1;
    }
    function httpAllowHalfOpenGet() {
      return this[kHttpAllowHalfOpen];
    }
    function httpAllowHalfOpenSet(value) {
      const previous = !!this[kHttpAllowHalfOpen];
      this[kHttpAllowHalfOpen] = value;
      const next = !!value;
      if (previous === next)
        return;
      const handle = this[serverSymbol];
      if (handle)
        setServerAppFlags(handle, this.requireHostHeader, true, serverLenientFlags(this), next);
    }
    function defineHttpAllowHalfOpen(server) {
      server[kHttpAllowHalfOpen] = false;
      Object.defineProperty(server, "httpAllowHalfOpen", {
        configurable: true,
        enumerable: true,
        get: httpAllowHalfOpenGet,
        set: httpAllowHalfOpenSet
      });
    }
    function onServerConnection(socketHandle) {
      if (socketHandle.duplex) {
        return;
      }
      const isTLS = !!this[tlsSymbol];
      const socket = new (getNodeHTTPServerSocket())(this, socketHandle, isTLS);
      const maxConnections = this.maxConnections;
      const tracked = this[kTrackedConnections];
      if (maxConnections != null && (tracked?.size ?? 0) > maxConnections) {
        tracked?.delete(socket);
        const data = {
          localAddress: socket.localAddress,
          localPort: socket.localPort || socketHandle.localPort,
          localFamily: socket.localFamily,
          remoteAddress: socket.remoteAddress,
          remotePort: socket.remotePort,
          remoteFamily: socket.remoteFamily || "IPv4"
        };
        socket.destroy();
        this.emit("drop", data);
        return;
      }
      socket.parser = createServerParserShim(socket);
      this.emit("connection", socket);
      if (isTLS && socketHandle.secureEstablished) {
        this.emit("secureConnection", socket);
      }
    }
    Server.prototype.setTimeout = function(msecs, callback) {
      this.timeout = msecs;
      if (typeof callback === "function")
        this.on("timeout", callback);
      return this;
    };
    function onServerRequestEvent(event) {
      const socket = this;
      switch (event) {
        case NodeHTTPResponseAbortEvent.abort: {
          if (!socket.destroyed) {
            socket.destroy();
          }
          break;
        }
        case NodeHTTPResponseAbortEvent.timeout: {
          socket.emit("timeout");
          break;
        }
      }
    }
    var HttpParserError;
    ((HttpParserError) => {
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_NONE"] = 0] = "HTTP_PARSER_ERROR_NONE";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_INVALID_CHUNKED_ENCODING"] = 1] = "HTTP_PARSER_ERROR_INVALID_CHUNKED_ENCODING";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_INVALID_CONTENT_LENGTH"] = 2] = "HTTP_PARSER_ERROR_INVALID_CONTENT_LENGTH";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_INVALID_TRANSFER_ENCODING"] = 3] = "HTTP_PARSER_ERROR_INVALID_TRANSFER_ENCODING";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_MISSING_HOST_HEADER"] = 4] = "HTTP_PARSER_ERROR_MISSING_HOST_HEADER";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_INVALID_REQUEST"] = 5] = "HTTP_PARSER_ERROR_INVALID_REQUEST";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_REQUEST_HEADER_FIELDS_TOO_LARGE"] = 6] = "HTTP_PARSER_ERROR_REQUEST_HEADER_FIELDS_TOO_LARGE";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_INVALID_HTTP_VERSION"] = 7] = "HTTP_PARSER_ERROR_INVALID_HTTP_VERSION";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_INVALID_EOF"] = 8] = "HTTP_PARSER_ERROR_INVALID_EOF";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_INVALID_METHOD"] = 9] = "HTTP_PARSER_ERROR_INVALID_METHOD";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_INVALID_HEADER_TOKEN"] = 10] = "HTTP_PARSER_ERROR_INVALID_HEADER_TOKEN";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_LF_EXPECTED"] = 11] = "HTTP_PARSER_ERROR_LF_EXPECTED";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_CHUNK_EXTENSIONS_OVERFLOW"] = 12] = "HTTP_PARSER_ERROR_CHUNK_EXTENSIONS_OVERFLOW";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_PAUSED_H2_UPGRADE"] = 13] = "HTTP_PARSER_ERROR_PAUSED_H2_UPGRADE";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_CLOSED_CONNECTION"] = 14] = "HTTP_PARSER_ERROR_CLOSED_CONNECTION";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_TRAILER_FIELDS_TOO_LARGE"] = 15] = "HTTP_PARSER_ERROR_TRAILER_FIELDS_TOO_LARGE";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_CHUNK_TERMINATOR_EXPECTED"] = 16] = "HTTP_PARSER_ERROR_CHUNK_TERMINATOR_EXPECTED";
      HttpParserError[HttpParserError["HTTP_PARSER_ERROR_TRAILER_CONTENT_LENGTH"] = 17] = "HTTP_PARSER_ERROR_TRAILER_CONTENT_LENGTH";
    })(HttpParserError ||= {});
    function onServerClientError(ssl, socket, errorCode, rawPacket) {
      const self = this;
      const existingDuplex = socket.duplex;
      const nodeSocket = existingDuplex ?? new (getNodeHTTPServerSocket())(self, socket, ssl);
      if (!existingDuplex) {
        nodeSocket.parser = createServerParserShim(nodeSocket);
        self.emit("connection", nodeSocket);
      }
      if (errorCode === 4 /* HTTP_PARSER_ERROR_MISSING_HOST_HEADER */) {
        replyMissingHostHeader(nodeSocket);
        return;
      }
      let err;
      switch (errorCode) {
        case 1 /* HTTP_PARSER_ERROR_INVALID_CHUNKED_ENCODING */:
          err = codes.HPE_INVALID_CHUNK_SIZE("Parse Error: Invalid character in chunk size");
          break;
        case 2 /* HTTP_PARSER_ERROR_INVALID_CONTENT_LENGTH */:
          err = codes.HPE_UNEXPECTED_CONTENT_LENGTH("Parse Error");
          break;
        case 3 /* HTTP_PARSER_ERROR_INVALID_TRANSFER_ENCODING */:
          err = codes.HPE_INVALID_TRANSFER_ENCODING("Parse Error: Request has invalid `Transfer-Encoding`");
          break;
        case 17 /* HTTP_PARSER_ERROR_TRAILER_CONTENT_LENGTH */:
          err = codes.HPE_INVALID_CONTENT_LENGTH("Parse Error: Content-Length can't be present with Transfer-Encoding");
          break;
        case 5 /* HTTP_PARSER_ERROR_INVALID_REQUEST */:
          err = codes.HPE_INVALID_CONSTANT("Parse Error: Expected HTTP/");
          break;
        case 8 /* HTTP_PARSER_ERROR_INVALID_EOF */:
          err = codes.HPE_INVALID_EOF_STATE("Parse Error: Invalid EOF state");
          break;
        case 9 /* HTTP_PARSER_ERROR_INVALID_METHOD */:
          err = codes.HPE_INVALID_METHOD("Parse Error: Invalid method encountered");
          err.bytesParsed = 1;
          break;
        case 10 /* HTTP_PARSER_ERROR_INVALID_HEADER_TOKEN */:
          err = codes.HPE_INVALID_HEADER_TOKEN("Parse Error: Invalid header token encountered");
          break;
        case 6 /* HTTP_PARSER_ERROR_REQUEST_HEADER_FIELDS_TOO_LARGE */:
        case 15 /* HTTP_PARSER_ERROR_TRAILER_FIELDS_TOO_LARGE */:
          err = codes.HPE_HEADER_OVERFLOW("Parse Error: Header overflow");
          err.bytesParsed = rawPacket.byteLength;
          break;
        case 7 /* HTTP_PARSER_ERROR_INVALID_HTTP_VERSION */:
          err = codes.HPE_INVALID_VERSION("Parse Error: Invalid HTTP version");
          break;
        case 11 /* HTTP_PARSER_ERROR_LF_EXPECTED */:
          err = codes.HPE_LF_EXPECTED("Parse Error: Missing expected LF after header value");
          break;
        case 12 /* HTTP_PARSER_ERROR_CHUNK_EXTENSIONS_OVERFLOW */:
          err = codes.HPE_CHUNK_EXTENSIONS_OVERFLOW("Parse Error: Chunk extensions overflow");
          break;
        case 13 /* HTTP_PARSER_ERROR_PAUSED_H2_UPGRADE */:
          err = codes.HPE_PAUSED_H2_UPGRADE("Parse Error: Pause on PRI/Upgrade");
          err.bytesParsed = 24;
          break;
        case 14 /* HTTP_PARSER_ERROR_CLOSED_CONNECTION */:
          err = codes.HPE_CLOSED_CONNECTION("Parse Error: Data after `Connection: close`");
          break;
        case 16 /* HTTP_PARSER_ERROR_CHUNK_TERMINATOR_EXPECTED */:
          err = codes.HPE_STRICT("Parse Error: Expected LF after chunk data");
          break;
        default:
          err = codes.HPE_INTERNAL("Parse Error");
          break;
      }
      err.rawPacket = Buffer.from(rawPacket);
      socketOnError.call(nodeSocket, err);
    }
    function replyMissingHostHeader(socket) {
      if (!socket.writable)
        return;
      socket.end(`HTTP/1.1 400 Bad Request\r
    Connection: close\r
    Date: ${new Date().toUTCString()}\r
    Transfer-Encoding: chunked\r
    \r
    0\r
    \r
    `);
    }
    const kBytesWritten = Symbol("kBytesWritten");
    const kEnableStreaming = Symbol("kEnableStreaming");
    const kUpgradeIncoming = Symbol("kUpgradeIncoming");
    function onReadableStreamEnd() {}
    function clearUpgradeIncoming(socket) {
      socket[kUpgradeIncoming] = undefined;
    }
    function detachSocketListenersForHandoff(socket) {
      socket.removeListener("error", socketOnError);
      socket.removeListener("timeout", onNodeHTTPServerSocketTimeout);
      socket.on("end", onReadableStreamEnd);
    }
    function resolveHandoffPromise(promise) {
      resolvePromise(promise, undefined);
    }
    const kSocketTimeoutTimer = Symbol("socketTimeoutTimer");
    const kStreamingEnabled = Symbol("kStreamingEnabled");
    const scratchResponseOptions = {
      [kHandle]: undefined,
      highWaterMark: 0,
      [kRejectNonStandardBodyWrites]: false
    };
    const kBoundOnAbort = Symbol("kBoundOnAbort");
    const kKeepAliveTimeoutSet = Symbol("keepAliveTimeoutSet");
    const kKeepAliveIdleStart = Symbol("keepAliveIdleStart");
    const kPipelinedResponses = Symbol("kPipelinedResponses");
    const kPipelinedQueuedState = Symbol("kPipelinedQueuedState");
    const kOutgoingData = Symbol("kOutgoingData");
    const kReplayingPipelinedOps = Symbol("kReplayingPipelinedOps");
    const kStopParsingOnCloseListener = Symbol("kStopParsingOnCloseListener");
    const kDispatcherDetached = Symbol("kDispatcherDetached");
    const badRequestResponse = Buffer.from(`HTTP/1.1 400 Bad Request\r
    Connection: close\r
    \r
    `, "latin1");
    const requestTimeoutResponse = Buffer.from(`HTTP/1.1 408 Request Timeout\r
    Connection: close\r
    \r
    `, "latin1");
    const requestHeaderFieldsTooLargeResponse = Buffer.from(`HTTP/1.1 431 Request Header Fields Too Large\r
    Connection: close\r
    \r
    `, "latin1");
    const requestChunkExtensionsTooLargeResponse = Buffer.from(`HTTP/1.1 413 Payload Too Large\r
    Connection: close\r
    \r
    `, "latin1");
    function socketOnError(err) {
      this.removeListener("error", socketOnError);
      this.on("error", noopOnError);
      const server = this.server;
      if (!server || !server.emit("clientError", err, this)) {
        const message = this._httpMessage;
        if (this.writable && (!message || message[headerStateSymbol] !== NodeHTTPHeaderState.sent)) {
          let response;
          switch (err?.code) {
            case "HPE_HEADER_OVERFLOW":
              response = requestHeaderFieldsTooLargeResponse;
              break;
            case "HPE_CHUNK_EXTENSIONS_OVERFLOW":
              response = requestChunkExtensionsTooLargeResponse;
              break;
            case "ERR_HTTP_REQUEST_TIMEOUT":
              response = requestTimeoutResponse;
              break;
            default:
              response = badRequestResponse;
              break;
          }
          const handle = this[kHandle];
          if (handle && !handle.closed) {
            handle.write(response);
          } else {
            this.write(response);
          }
        }
        this.destroy(err);
      }
    }
    function noopOnError() {}
    function onSocketTimeoutTimerExpired(socket) {
      if (socket.timeout === 0 && !socket[kKeepAliveTimeoutSet]) {
        socket[kSocketTimeoutTimer]?.refresh();
        return;
      }
      const idleStart = socket[kKeepAliveIdleStart];
      if (idleStart !== undefined && socket[kKeepAliveTimeoutSet]) {
        socket[kKeepAliveIdleStart] = undefined;
        const remaining = socket.timeout - (DateNow() - idleStart);
        if (remaining > 0) {
          const existingTimer = socket[kSocketTimeoutTimer];
          if (existingTimer !== undefined)
            clearTimeout(existingTimer);
          const timer = setTimeout(onSocketTimeoutTimerExpired, remaining, socket);
          timer.unref();
          socket[kSocketTimeoutTimer] = timer;
          return;
        }
      }
      if (socket[kKeepAliveTimeoutSet]) {
        socket[kSocketTimeoutTimer] = undefined;
      }
      socket._onTimeout();
    }
    let NodeHTTPServerSocket;
    function getNodeHTTPServerSocket() {
      if (NodeHTTPServerSocket)
        return NodeHTTPServerSocket;
      const { Socket: NetSocket } = require("node:net");
      NodeHTTPServerSocket = class Socket extends NetSocket {
        bytesRead = 0;
        connecting = false;
        timeout = 0;
        parser = null;
        [kStreamingEnabled] = false;
        [kBoundOnAbort] = null;
        [kKeepAliveIdleStart] = undefined;
        [kBytesWritten] = 0;
        [kHandle];
        [kUpgradeIncoming] = undefined;
        server;
        _httpMessage;
        _secureEstablished = false;
        _paused = false;
        #pendingCallback = null;
        #pendingAbortMessage;
        constructor(server, handle, encrypted) {
          super(server[kHighWaterMark] !== undefined ? { highWaterMark: server[kHighWaterMark], allowHalfOpen: true } : { allowHalfOpen: true });
          this.removeAllListeners("end");
          this._writableState.emitClose = true;
          this._readableState.emitClose = true;
          this._writableState.decodeStrings = true;
          this.server = server;
          this[kHandle] = handle;
          this._secureEstablished = !!handle?.secureEstablished;
          handle.onclose = this.#onClose.bind(this);
          handle.duplex = this;
          this.encrypted = encrypted;
          this.on("timeout", onNodeHTTPServerSocketTimeout);
          this.on("error", socketOnError);
          server[kTrackedConnections]?.add(this);
          const serverTimeout = server.timeout;
          if (serverTimeout) {
            this.setTimeout(serverTimeout);
          }
        }
        get bytesWritten() {
          const handle = this[kHandle];
          return handle ? handle.response?.getBytesWritten?.() ?? handle.bytesWritten ?? this[kBytesWritten] ?? 0 : this[kBytesWritten] ?? 0;
        }
        set bytesWritten(value) {
          this[kBytesWritten] = value;
        }
        [kEnableStreaming](enable) {
          if (this[kStreamingEnabled] === enable) {
            return;
          }
          const handle = this[kHandle];
          if (handle) {
            this[kStreamingEnabled] = enable;
            if (enable) {
              handle.ondata = this.#onData.bind(this);
              handle.ondrain = this.#onDrain.bind(this);
            } else {
              handle.ondata = undefined;
              handle.ondrain = undefined;
            }
          }
        }
        #onDrain() {
          const handle = this[kHandle];
          this[kBytesWritten] = handle ? handle.response?.getBytesWritten?.() ?? handle.bytesWritten ?? 0 : 0;
          const callback = this.#pendingCallback;
          if (callback) {
            this.#pendingCallback = null;
            callback();
          }
          this.emit("drain");
        }
        #onData(chunk, last) {
          this._unrefTimer();
          if (chunk) {
            this.push(chunk);
          }
          if (last) {
            const handle = this[kHandle];
            if (handle) {
              handle.ondata = undefined;
              this[kStreamingEnabled] = false;
              handle.response?.unref();
            }
            this.push(null);
            if (this.readableLength === 0) {
              this.resume();
            }
          }
        }
        #closeHandle(handle, callback, err) {
          this[kHandle] = undefined;
          this.#pendingAbortMessage = this._httpMessage;
          handle.onclose = this.#onCloseForDestroy.bind(this, callback, err);
          handle.close();
        }
        #onClose() {
          releaseServerParserShim(this);
          this[kHandle] = null;
          this.server?.[kTrackedConnections]?.delete(this);
          const timer = this[kSocketTimeoutTimer];
          if (timer) {
            clearTimeout(timer);
            this[kSocketTimeoutTimer] = undefined;
          }
          const pending = this.#pendingAbortMessage;
          this.#pendingAbortMessage = undefined;
          const message = this._httpMessage ?? (pending?.destroyed ? pending : undefined);
          const req = message?.req;
          if (req && !req.destroyed && !req[kHandle]?.upgraded) {
            req[kHandle] = undefined;
            if (req.listenerCount("error") > 0) {
              req.destroy(new ConnResetException("aborted"));
            } else {
              req.destroy();
            }
          }
          if (message && !message._closed) {
            process.nextTick(emitCloseNT, message);
          }
          const pipelined = this[kPipelinedResponses];
          const pipelinedLength = pipelined ? pipelined.length : 0;
          if (pipelinedLength) {
            this[kPipelinedResponses] = undefined;
            for (let i = 0;i < pipelinedLength; i++) {
              const queuedRes = pipelined[i];
              const queuedReq = queuedRes.req;
              if (queuedReq && !queuedReq.destroyed) {
                queuedReq[kHandle] = undefined;
                if (queuedReq.listenerCount("error") > 0) {
                  queuedReq.destroy(new ConnResetException("aborted"));
                } else {
                  queuedReq.destroy();
                }
              }
              if (!queuedRes.destroyed) {
                queuedRes.destroy();
              } else if (!queuedRes._closed) {
                process.nextTick(emitCloseNT, queuedRes);
              }
            }
          }
          if (!this.destroyed) {
            this.destroy();
          }
        }
        #onCloseForDestroy(closeCallback, err) {
          this.#onClose();
          if ((typeof closeCallback === "function"))
            closeCallback(err);
        }
        _onTimeout() {
          const handle = this[kHandle];
          const response = handle?.response;
          if (response && response.writableLength > 0) {
            this._unrefTimer();
            return;
          }
          this.emit("timeout");
        }
        _unrefTimer() {
          this[kSocketTimeoutTimer]?.refresh();
        }
        address() {
          return this[kHandle]?.remoteAddress || null;
        }
        get bufferSize() {
          return this.writableLength;
        }
        connect(_port, _host, _connectListener) {
          return this;
        }
        _destroy(err, callback) {
          const handle = this[kHandle];
          if (!handle) {
            if ((typeof callback === "function"))
              callback(err);
            return;
          }
          handle.ondata = undefined;
          if (handle.closed) {
            const onclose = handle.onclose;
            handle.onclose = undefined;
            if ((typeof onclose === "function")) {
              onclose.call(handle);
            }
            if ((typeof callback === "function"))
              callback(err);
            return;
          }
          this.#closeHandle(handle, callback, err);
        }
        _final(callback) {
          const handle = this[kHandle];
          if (!handle) {
            callback();
            return;
          }
          handle.end();
          callback();
        }
        get localAddress() {
          return this[kHandle]?.localAddress?.address;
        }
        get localFamily() {
          return this[kHandle]?.localAddress?.family;
        }
        get localPort() {
          return this[kHandle]?.localAddress?.port;
        }
        get pending() {
          return this.connecting;
        }
        #resumeSocket() {
          const handle = this[kHandle];
          const response = handle?.response;
          const upgradeIncoming = this[kUpgradeIncoming];
          if (upgradeIncoming) {
            if (response) {
              const resumed = response.resume();
              if (resumed && resumed !== true) {
                upgradeIncoming.push(resumed);
              }
            }
            upgradeIncoming.resume();
            return;
          }
          if (response) {
            const resumed = response.resume();
            if (resumed && resumed !== true) {
              const bodyReadState = handle.hasBody;
              const message = this._httpMessage;
              const req = message?.req;
              if ((bodyReadState & NodeHTTPBodyReadState.done) !== 0) {
                emitServerSocketEOFNT(this, req);
              }
              if (req) {
                req.push(resumed);
              }
              this.push(resumed);
            }
          }
        }
        _read(_size) {
          this.#resumeSocket();
        }
        get readyState() {
          if (this.connecting)
            return "opening";
          if (this.readable) {
            return this.writable ? "open" : "readOnly";
          } else {
            return this.writable ? "writeOnly" : "closed";
          }
        }
        get servername() {
          if (!this.encrypted)
            return;
          const name = this[kHandle]?.servername;
          return typeof name === "string" && name.length > 0 ? name : false;
        }
        get authorized() {
          if (!this.encrypted)
            return;
          if (!this.server?.[tlsSymbol]?.requestCert)
            return false;
          return this[kHandle]?.peerCertVerified === true;
        }
        get authorizationError() {
          if (!this.encrypted)
            return;
          if (!this.server?.[tlsSymbol]?.requestCert)
            return null;
          return this[kHandle]?.authorizationError ?? null;
        }
        ref() {
          return this;
        }
        get remoteAddress() {
          return this.address()?.address;
        }
        set remoteAddress(val) {
          this.address().address = val;
        }
        get remotePort() {
          return this.address()?.port;
        }
        set remotePort(val) {
          this.address().port = val;
        }
        get remoteFamily() {
          return this.address()?.family;
        }
        set remoteFamily(val) {
          this.address().family = val;
        }
        resetAndDestroy() {}
        setKeepAlive(_enable = false, _initialDelay = 0) {}
        setNoDelay(_noDelay = true) {
          return this;
        }
        setTimeout(msecs, callback) {
          if (this.destroyed) {
            return this;
          }
          msecs = validateMsecs(msecs, "msecs");
          this.timeout = msecs;
          const existingTimer = this[kSocketTimeoutTimer];
          if (msecs === 0) {
            if (existingTimer) {
              clearTimeout(existingTimer);
              this[kSocketTimeoutTimer] = undefined;
            }
            if (callback !== undefined) {
              validateFunction(callback, "callback");
              this.removeListener("timeout", callback);
            }
          } else {
            if (existingTimer && existingTimer._idleTimeout === msecs) {
              existingTimer.refresh();
            } else {
              if (existingTimer)
                clearTimeout(existingTimer);
              const timer = setTimeout(onSocketTimeoutTimerExpired, msecs, this);
              timer.unref();
              this[kSocketTimeoutTimer] = timer;
            }
            if (callback !== undefined) {
              validateFunction(callback, "callback");
              this.once("timeout", callback);
            }
          }
          return this;
        }
        setEncoding(_encoding) {
          const err = new Error("Changing the socket encoding is not allowed per RFC7230 Section 3.");
          err.code = "ERR_HTTP_SOCKET_ENCODING";
          throw err;
        }
        unref() {
          return this;
        }
        _write(_chunk, _encoding, _callback) {
          const handle = this[kHandle];
          let err;
          this._unrefTimer();
          try {
            if (handle) {
              const flushed = handle.write(_chunk, _encoding);
              if (!flushed && handle.ondrain) {
                this.#pendingCallback = _callback;
                return false;
              }
            }
          } catch (e) {
            err = e;
          }
          if (err)
            _callback(err);
          else
            _callback();
        }
        pause() {
          const handle = this[kHandle];
          const response = handle?.response;
          if (response) {
            response.pause();
          }
          return super.pause();
        }
        resume() {
          this.#resumeSocket();
          return super.resume();
        }
        get [kInternalSocketData]() {
          return this[kHandle]?.response;
        }
      };
      Object.defineProperty(NodeHTTPServerSocket, "name", { value: "Socket" });
      return NodeHTTPServerSocket;
    }
    function willBeChunked(response) {
      const outHeaders = response[kOutHeaders];
      const te = outHeaders !== null ? outHeaders["transfer-encoding"] : undefined;
      if (te !== undefined) {
        return chunkExpression.test(String(te[1]));
      }
      if (outHeaders !== null && outHeaders["content-length"] !== undefined)
        return false;
      if (response._hasBody === false)
        return false;
      if (response._removedTE)
        return false;
      return response.useChunkedEncodingByDefault === true;
    }
    function hasInvalidTrailer(response) {
      if (willBeChunked(response))
        return false;
      if (response._trailer)
        return true;
      const outHeaders = response[kOutHeaders];
      return outHeaders !== null && outHeaders["trailer"] !== undefined;
    }
    function _writeHead(statusCode, reason, obj, response) {
      const originalStatusCode = statusCode;
      statusCode |= 0;
      if (statusCode < 100 || statusCode > 999) {
        throw codes.HTTP_INVALID_STATUS_CODE(require("internal/util/inspect").format("%s", originalStatusCode));
      }
      if (typeof reason === "string") {
        response.statusMessage = reason;
      } else {
        if (!response.statusMessage)
          response.statusMessage = STATUS_CODES[statusCode] || "unknown";
        obj ??= reason;
      }
      if (checkInvalidHeaderChar(response.statusMessage))
        throw codes.INVALID_CHAR("statusMessage");
      response.statusCode = statusCode;
      updateHasBody(response, statusCode);
      {
        let k;
        if (Array.isArray(obj)) {
          const length = obj.length;
          if (length && Array.isArray(obj[0])) {
            for (let i = 0;i < length; i++) {
              const k = obj[i];
              if (k)
                response.appendHeader(k[0], k[1]);
            }
          } else {
            if (length % 2 !== 0) {
              throw codes.INVALID_ARG_VALUE("headers", obj);
            }
            if (hasInvalidTrailer(response)) {
              throw codes.HTTP_TRAILER_INVALID("Trailers are invalid with this transfer encoding");
            }
            for (let n = 0;n < length; n += 2) {
              k = obj[n + 0];
              response.removeHeader(k);
            }
            for (let n = 0;n < length; n += 2) {
              k = obj[n];
              if (k)
                response.appendHeader(k, obj[n + 1]);
            }
          }
        } else if (obj) {
          const keys = Object.keys(obj);
          const length = keys.length;
          for (let i = 0;i < length; i++) {
            k = keys[i];
            if (k)
              response.setHeader(k, obj[k]);
          }
        }
        if (hasInvalidTrailer(response)) {
          response.removeHeader("trailer");
          throw codes.HTTP_TRAILER_INVALID("Trailers are invalid with this transfer encoding");
        }
      }
    }
    function ServerResponse(req, options) {
      if (!(this instanceof ServerResponse))
        return new ServerResponse(req, options);
      OutgoingMessage.call(this, options);
      this.useChunkedEncodingByDefault = true;
      this.req = req;
      this.sendDate = true;
      this._sent100 = false;
      this[headerStateSymbol] = NodeHTTPHeaderState.none;
      this[kPendingCallbacks] = [];
      this.finished = false;
      if (req.method === "HEAD")
        this._hasBody = false;
      if (req.httpVersionMajor < 1 || req.httpVersionMinor < 1) {
        this.useChunkedEncodingByDefault = chunkExpression.test(req.headers?.te ?? "");
        this.shouldKeepAlive = false;
      }
      if (options) {
        const handle = options[kHandle];
        if (handle) {
          this[kHandle] = handle;
        } else {
          this[kHandle] = req[kHandle];
        }
        const rejectNonStandardBodyWrites = options[kRejectNonStandardBodyWrites];
        if (rejectNonStandardBodyWrites !== undefined) {
          this[kRejectNonStandardBodyWrites] = rejectNonStandardBodyWrites;
        }
      } else {
        this[kHandle] = req[kHandle];
      }
      this.statusCode = 200;
      this.statusMessage = undefined;
      this.chunkedEncoding = false;
    }
    toClass(ServerResponse, "ServerResponse", OutgoingMessage);
    const scratchFlatHeaders = [];
    let scratchFlatHeadersBusy = false;
    function releaseRenderedHeaders(flat) {
      if (flat === scratchFlatHeaders)
        scratchFlatHeadersBusy = false;
    }
    const AUTO_HEADER_DATE = 1 << 0;
    const AUTO_HEADER_CONN_KEEP_ALIVE = 1 << 1;
    const AUTO_HEADER_CONN_CLOSE = 1 << 2;
    const AUTO_HEADER_KEEP_ALIVE_TIMEOUT = 1 << 3;
    const AUTO_HEADER_TRANSFER_ENCODING_CHUNKED = 1 << 4;
    let renderedAutoHeaders = 0;
    let renderedKeepAliveSecs = 0;
    function renderNativeHeaders(res) {
      let autoHeaders = 0;
      let keepAliveSecs = 0;
      const headersMap = res[kOutHeaders];
      let flat;
      if (scratchFlatHeadersBusy) {
        flat = [];
      } else {
        scratchFlatHeadersBusy = true;
        scratchFlatHeaders.length = 0;
        flat = scratchFlatHeaders;
      }
      let hasDate = false;
      let hasConnection = false;
      let hasKeepAlive = false;
      try {
        if (headersMap !== null && headersMap !== undefined) {
          for (const key in headersMap) {
            const entry = headersMap[key];
            const name = entry[0];
            const value = entry[1];
            if (key === "date")
              hasDate = true;
            else if (key === "connection") {
              hasConnection = true;
              if (RE_CONN_CLOSE.test(Array.isArray(value) ? value.join(", ") : String(value))) {
                res[kMustCloseConnection] = true;
              }
            } else if (key === "keep-alive")
              hasKeepAlive = true;
            if (Array.isArray(value)) {
              const valueLength = value.length;
              if (valueLength >= 2 && (key === "cookie" || res[kUniqueHeaders] != null && res[kUniqueHeaders].has(key))) {
                flat.push(name, value.join("; "));
              } else {
                for (let i = 0;i < valueLength; i++) {
                  flat.push(name, String(value[i]));
                }
              }
            } else {
              flat.push(name, String(value));
            }
          }
        }
        if (res.sendDate && !hasDate) {
          autoHeaders |= AUTO_HEADER_DATE;
        }
        const storedTransferEncoding = headersMap === null ? undefined : headersMap["transfer-encoding"];
        const storedContentLength = headersMap === null ? undefined : headersMap["content-length"];
        let defectiveNoBodyResponse = false;
        if (storedTransferEncoding !== undefined) {
          const statusCode = res[kSnapshotStatusCode] ?? res.statusCode;
          if (statusCode === 204 || statusCode === 304) {
            defectiveNoBodyResponse = true;
            res[kMustCloseConnection] = true;
          }
        }
        let closeDelimited = false;
        let forceChunked = false;
        if (storedContentLength === undefined && storedTransferEncoding === undefined) {
          if (res._hasBody === false) {} else if (res._removedTE) {
            closeDelimited = true;
            res[kMustCloseConnection] = true;
          } else if (res._removedContLen || res[kFramingFrozenChunked]) {
            const req = res.req;
            if (res.useChunkedEncodingByDefault && req.httpVersionMajor >= 1 && req.httpVersionMinor >= 1) {
              forceChunked = true;
            } else {
              closeDelimited = true;
              res[kMustCloseConnection] = true;
            }
          }
        }
        if (res._removedConnection) {
          if (res.shouldKeepAlive === false) {
            res[kMustCloseConnection] = true;
          }
        } else if (!hasConnection) {
          if (!defectiveNoBodyResponse && !closeDelimited && !res.maxRequestsOnConnectionReached && res.shouldKeepAlive !== false && requestShouldKeepAlive(res.req)) {
            const keepAliveTimeout = res._keepAliveTimeout;
            const maxRequestsPerSocket = res._maxRequestsPerSocket;
            if (keepAliveTimeout && !hasKeepAlive && ~~maxRequestsPerSocket > 0) {
              flat.push("Connection", "keep-alive");
              flat.push("Keep-Alive", `timeout=${MathFloor(keepAliveTimeout / 1000)}, max=${maxRequestsPerSocket}`);
            } else {
              autoHeaders |= AUTO_HEADER_CONN_KEEP_ALIVE;
              if (keepAliveTimeout && !hasKeepAlive) {
                autoHeaders |= AUTO_HEADER_KEEP_ALIVE_TIMEOUT;
                keepAliveSecs = MathFloor(keepAliveTimeout / 1000);
              }
            }
          } else {
            if (res.shouldKeepAlive === false) {
              res[kMustCloseConnection] = true;
            }
            autoHeaders |= AUTO_HEADER_CONN_CLOSE;
          }
        }
        if (res._hasBody === false) {
          flat.push("\x00", "2");
        }
        if (closeDelimited) {
          flat.push("\x00", "1");
        } else if (forceChunked) {
          autoHeaders |= AUTO_HEADER_TRANSFER_ENCODING_CHUNKED;
        }
      } catch (e) {
        if (flat === scratchFlatHeaders)
          scratchFlatHeadersBusy = false;
        throw e;
      }
      renderedAutoHeaders = autoHeaders;
      renderedKeepAliveSecs = keepAliveSecs;
      return flat;
    }
    const kMustCloseConnection = Symbol("kMustCloseConnection");
    function stopServerResponsePerf() {
      if (this[kServerResponseStatistics] && hasObserver("http")) {
        stopPerf(this, kServerResponseStatistics, {
          detail: {
            res: {
              statusCode: this.statusCode,
              statusMessage: this.statusMessage,
              headers: typeof this.getHeaders === "function" ? this.getHeaders() : {}
            }
          }
        });
      }
    }
    function emitResponseFinish() {
      const socket = this.req?.socket ?? this.socket;
      onResponseFinishHandleSocket(socket?.server, socket, this);
      if (this[kDispatcherDetached])
        return;
      if (socket != null)
        this.detachSocket(socket);
      advanceResponsePipeline(socket?.server, socket);
    }
    function onResponseFinishHandleSocket(server, socket, res) {
      if (res[kMustCloseConnection]) {
        socket?.end();
        return;
      }
      if (!socket || socket.destroyed || typeof socket.setTimeout !== "function") {
        return;
      }
      if (socket._httpMessage && socket._httpMessage !== res) {
        return;
      }
      if (socket[kPipelinedResponses]?.length) {
        return;
      }
      const rawKeepAliveTimeout = server.keepAliveTimeout;
      const keepAliveTimeout = Number.isFinite(rawKeepAliveTimeout) && rawKeepAliveTimeout >= 0 ? rawKeepAliveTimeout : 0;
      const rawKeepAliveBuffer = server.keepAliveTimeoutBuffer;
      const keepAliveTimeoutBuffer = Number.isFinite(rawKeepAliveBuffer) && rawKeepAliveBuffer >= 0 ? rawKeepAliveBuffer : 1000;
      if (keepAliveTimeout) {
        const total = keepAliveTimeout + keepAliveTimeoutBuffer;
        const timer = socket[kSocketTimeoutTimer];
        if (timer !== undefined && timer._idleTimeout === total) {
          socket.timeout = total;
        } else {
          socket.setTimeout(total);
        }
        socket[kKeepAliveIdleStart] = DateNow();
        socket[kKeepAliveTimeoutSet] = true;
      }
    }
    function onSocketCloseStopParsing() {
      this[kHandle]?.stopParsing?.();
    }
    function pausePipelineReads(socket) {
      const response = socket[kHandle]?.response;
      if (!response)
        return;
      socket._paused = true;
      response.pauseReads();
    }
    function addPipelineOutgoingData(queued, bytes) {
      const socket = queued.socket;
      socket[kOutgoingData] = (socket[kOutgoingData] ?? 0) + bytes;
    }
    function releasePipelineOutgoingData(socket, bytes) {
      const outgoing = (socket[kOutgoingData] ?? 0) - bytes;
      socket[kOutgoingData] = outgoing > 0 ? outgoing : 0;
      if (socket._paused && outgoing <= socket.writableHighWaterMark) {
        socket._paused = false;
        socket[kHandle]?.response?.resume();
      }
    }
    const kPipelineKickScheduled = Symbol("kPipelineKickScheduled");
    function advancePipelineIfIdleNT(server, socket) {
      socket[kPipelineKickScheduled] = false;
      if (socket._httpMessage == null && socket[kPipelinedResponses]?.length) {
        advanceResponsePipeline(server, socket);
      }
    }
    function advanceResponsePipeline(server, socket) {
      if (!socket || socket.writableEnded || socket.destroyed) {
        return;
      }
      const queue = socket[kPipelinedResponses];
      if (!queue || queue.length === 0) {
        return;
      }
      const res = queue.shift();
      const queued = res[kPipelinedQueuedState];
      res[kPipelinedQueuedState] = undefined;
      releasePipelineOutgoingData(socket, queued.bytes);
      const handle = res[kHandle];
      const socketHandle = socket[kHandle];
      if (res.destroyed || !handle) {
        if (!socket.destroyed) {
          socket.destroy();
        }
        return;
      }
      if (!socketHandle || socket.destroyed || !socketHandle.startPipelinedResponse(handle, !!queued.isAncient, !requestShouldKeepAlive(res.req))) {
        if (!res.destroyed) {
          res.destroy();
        }
        return;
      }
      if (res.assignSocket === ServerResponse.prototype.assignSocket) {
        assignSocketInternal(res, socket);
      } else {
        res.assignSocket(socket);
      }
      socket[kRequest] = res.req;
      res.outputSize = 0;
      const ops = queued.ops;
      const opsLength = ops.length;
      let lastWriteResult = true;
      if (opsLength) {
        if (queued.ended) {
          res.finished = false;
        }
        res[kReplayingPipelinedOps] = true;
        try {
          for (let i = 0;i < opsLength; i++) {
            const op = ops[i];
            const kind = op[0];
            if (kind === "raw") {
              handle.writeInformational(op[1], op[2]);
              if (typeof op[3] === "function")
                process.nextTick(op[3]);
            } else if (kind === "write") {
              lastWriteResult = res.write(op[1], op[2], op[3]);
            } else {
              res.end(op[1], op[2], op[3]);
            }
          }
        } finally {
          res[kReplayingPipelinedOps] = false;
        }
      }
      if (queued.needDrain && !queued.ended) {
        if (lastWriteResult !== false) {
          process.nextTick(emitPipelinedDrainNT, res);
        }
      }
    }
    function markResponseEndedNT(res) {
      res._ended = true;
    }
    function emitPipelinedDrainNT(res) {
      if (!res.destroyed && !res.finished) {
        res.emit("drain");
      }
    }
    function accountQueuedHeaderBytes(res, queued) {
      if (queued.headerBytes !== 0) {
        return;
      }
      let bytes = 15 + String(res.statusMessage ?? STATUS_CODES[res.statusCode] ?? "unknown").length;
      const outHeaders = res[kOutHeaders];
      if (outHeaders !== null && outHeaders !== undefined) {
        for (const key in outHeaders) {
          const entry = outHeaders[key];
          if (!entry)
            continue;
          const value = entry[1];
          if (Array.isArray(value)) {
            for (let i = 0;i < value.length; i++) {
              bytes += entry[0].length + 4 + String(value[i]).length;
            }
          } else {
            bytes += entry[0].length + 4 + String(value).length;
          }
        }
      }
      if (!res.hasHeader("date"))
        bytes += 37;
      if (!res.hasHeader("connection"))
        bytes += 24;
      if (!res.hasHeader("content-length") && !res.hasHeader("transfer-encoding"))
        bytes += 28;
      bytes += 2;
      queued.headerBytes = bytes;
      queued.bytes += bytes;
      res.outputSize += bytes;
      addPipelineOutgoingData(queued, bytes);
    }
    function bufferPipelinedWrite(res, queued, chunk, encoding, callback) {
      callWriteHeadIfObservable(res, res[headerStateSymbol]);
      if (res[headerStateSymbol] === NodeHTTPHeaderState.none) {
        updateHasBody(res, res.statusCode);
      }
      accountQueuedHeaderBytes(res, queued);
      if (chunk && !res._hasBody) {
        if (res[kRejectNonStandardBodyWrites]) {
          throw codes.HTTP_BODY_NOT_ALLOWED();
        }
        chunk = undefined;
      }
      queued.ops.push(["write", chunk, encoding, callback]);
      if (chunk) {
        const bytes = typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
        queued.bytes += bytes;
        res.outputSize += bytes;
        addPipelineOutgoingData(queued, bytes);
      }
      if (queued.bytes >= res.writableHighWaterMark) {
        queued.needDrain = true;
        return false;
      }
      return true;
    }
    function bufferPipelinedEnd(res, queued, chunk, encoding, callback) {
      callWriteHeadIfObservable(res, res[headerStateSymbol], true);
      if (res[headerStateSymbol] === NodeHTTPHeaderState.none) {
        updateHasBody(res, res.statusCode);
      }
      accountQueuedHeaderBytes(res, queued);
      if (chunk && !res._hasBody) {
        if (res[kRejectNonStandardBodyWrites]) {
          throw codes.HTTP_BODY_NOT_ALLOWED();
        }
        chunk = undefined;
      }
      queued.ops.push(["end", chunk, encoding, callback]);
      if (chunk) {
        const bytes = typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
        queued.bytes += bytes;
        res.outputSize += bytes;
        addPipelineOutgoingData(queued, bytes);
      }
      queued.ended = true;
      res.finished = true;
      return res;
    }
    const RE_CONN_CLOSE = /(?:^|\W)close(?:$|\W)/i;
    const DISPATCH_CONN_CLOSE = 1 << 0;
    const DISPATCH_CONN_UPGRADE = 1 << 1;
    const DISPATCH_HAS_UPGRADE = 1 << 2;
    const DISPATCH_HAS_HOST = 1 << 3;
    const DISPATCH_HAS_EXPECT = 1 << 4;
    const DISPATCH_EXPECT_CONTINUE = 1 << 5;
    const DISPATCH_HAS_CONTENT_LENGTH = 1 << 6;
    const DISPATCH_HAS_TRANSFER_ENCODING = 1 << 7;
    function shouldKeepAliveForConnection(req, connection) {
      if (!req)
        return true;
      if (req.httpVersionMajor === 1 && req.httpVersionMinor === 0) {
        return false;
      }
      return !(typeof connection === "string" && RE_CONN_CLOSE.test(connection));
    }
    function requestShouldKeepAlive(req) {
      if (!req)
        return true;
      const cached = req[kReqShouldKeepAlive];
      if (cached !== undefined)
        return cached;
      return req[kReqShouldKeepAlive] = shouldKeepAliveForConnection(req, req.headers.connection);
    }
    function isHTTPServerHeaderStateSentOrAssigned(state) {
      return state === NodeHTTPHeaderState.sent || state === NodeHTTPHeaderState.assigned;
    }
    function throwIfServerHeadersSent(self, action) {
      if (isHTTPServerHeaderStateSentOrAssigned(self[headerStateSymbol])) {
        throw codes.HTTP_HEADERS_SENT(action);
      }
    }
    ServerResponse.prototype.setHeader = function setHeader(name, value) {
      throwIfServerHeadersSent(this, "set");
      return OutgoingMessagePrototype.setHeader.call(this, name, value);
    };
    ServerResponse.prototype.appendHeader = function appendHeader(name, value) {
      throwIfServerHeadersSent(this, "append");
      return OutgoingMessagePrototype.appendHeader.call(this, name, value);
    };
    ServerResponse.prototype.setHeaders = function setHeaders(headers) {
      throwIfServerHeadersSent(this, "set");
      return OutgoingMessagePrototype.setHeaders.call(this, headers);
    };
    ServerResponse.prototype.removeHeader = function removeHeader(name) {
      throwIfServerHeadersSent(this, "remove");
      return OutgoingMessagePrototype.removeHeader.call(this, name);
    };
    Object.defineProperty(ServerResponse.prototype, "headers", {
      get() {
        return this.getHeaders();
      },
      set(value) {
        throwIfServerHeadersSent(this, "set");
        this[kOutHeaders] = null;
        if (!value)
          return;
        if (Array.isArray(value)) {
          for (const { 0: key, 1: val } of value) {
            this.appendHeader(key, val);
          }
        } else if (typeof value.entries === "function") {
          for (const { 0: key, 1: val } of value.entries()) {
            this.appendHeader(key, val);
          }
        } else {
          const keys = ObjectKeys(value);
          for (let i = 0;i < keys.length; i++) {
            this.setHeader(keys[i], value[keys[i]]);
          }
        }
      }
    });
    Object.defineProperty(ServerResponse.prototype, "socket", {
      get() {
        if (this[kPipelinedQueuedState] !== undefined) {
          return null;
        }
        return this[fakeSocketSymbol] ??= new FakeSocket(this);
      },
      set(value) {
        this[fakeSocketSymbol] = value;
        this[kSocket] = value;
      }
    });
    Object.defineProperty(ServerResponse.prototype, "connection", {
      get() {
        return this.socket;
      },
      set(value) {
        this.socket = value;
      }
    });
    Object.defineProperty(ServerResponse.prototype, "chunkedEncoding", {
      get() {
        return this[kChunkedEncoding] ?? false;
      },
      set(value) {
        this[kChunkedEncoding] = value;
      }
    });
    Object.defineProperty(ServerResponse.prototype, "writableCorked", {
      get() {
        return this.socket?.writableCorked ?? 0;
      },
      set(_value) {}
    });
    ServerResponse.prototype.cork = function cork() {
      this.socket?.cork();
    };
    ServerResponse.prototype.uncork = function uncork() {
      this.socket?.uncork();
    };
    ServerResponse.prototype.setTimeout = function setTimeout(msecs, callback) {
      if (callback)
        this.on("timeout", callback);
      if (!this[fakeSocketSymbol]) {
        this.once("socket", function socketSetTimeoutOnConnect(socket) {
          socket.setTimeout(msecs);
        });
      } else {
        this.socket.setTimeout(msecs);
      }
      return this;
    };
    ServerResponse.prototype._removedConnection = false;
    ServerResponse.prototype._removedContLen = false;
    ServerResponse.prototype._hasBody = true;
    ServerResponse.prototype._ended = false;
    ServerResponse.prototype[kRejectNonStandardBodyWrites] = undefined;
    Object.defineProperty(ServerResponse.prototype, "headersSent", {
      get() {
        return this[headerStateSymbol] === NodeHTTPHeaderState.sent || this[headerStateSymbol] === NodeHTTPHeaderState.assigned;
      },
      set(value) {
        this[headerStateSymbol] = value ? NodeHTTPHeaderState.sent : NodeHTTPHeaderState.none;
      }
    });
    ServerResponse.prototype._writeRaw = function(chunk, encoding, callback) {
      if (!this[kHandle]) {
        return OutgoingMessagePrototype._writeRaw.apply(this, arguments);
      }
      const queued = this[kPipelinedQueuedState];
      if (queued !== undefined) {
        queued.ops.push(["raw", chunk, encoding, callback]);
        const bytes = typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
        queued.bytes += bytes;
        this.outputSize += bytes;
        addPipelineOutgoingData(queued, bytes);
        return queued.bytes < this.writableHighWaterMark;
      }
      this[kHandle].writeInformational(chunk, encoding);
      if (typeof callback === "function")
        process.nextTick(callback);
      return true;
    };
    ServerResponse.prototype.writeEarlyHints = function(hints, cb) {
      validateObject(hints, "hints");
      if (hints.link === null || hints.link === undefined) {
        return;
      }
      const link = validateLinkHeaderValue(hints.link);
      if (link.length === 0) {
        return;
      }
      if (checkInvalidHeaderChar(link)) {
        throw codes.INVALID_CHAR("header content", "Link");
      }
      const headers = { __proto__: null, Link: link };
      const keys = ObjectKeys(hints);
      for (let i = 0;i < keys.length; i++) {
        const key = keys[i];
        if (key !== "link") {
          headers[key] = hints[key];
        }
      }
      this.writeInformation(103, headers, cb);
    };
    function processInformationHeader(name, value) {
      validateHeaderName(name);
      validateHeaderValue(name, value);
      return `${name}: ${value}\r
    `;
    }
    ServerResponse.prototype.writeInformation = function writeInformation(statusCode, headers, cb) {
      if (this.headersSent) {
        throw codes.HTTP_HEADERS_SENT("write");
      }
      validateInteger(statusCode, "statusCode", 100, 199);
      if (statusCode === 101) {
        throw codes.HTTP_INVALID_STATUS_CODE(statusCode);
      }
      const statusMessage = STATUS_CODES[statusCode] || "unknown";
      let head = `HTTP/1.1 ${statusCode} ${statusMessage}\r
    `;
      if (headers !== undefined && headers !== null) {
        if (Array.isArray(headers)) {
          const headersLength = headers.length;
          if (headersLength && Array.isArray(headers[0])) {
            for (let i = 0;i < headersLength; i++) {
              const entry = headers[i];
              head += processInformationHeader(entry[0], entry[1]);
            }
          } else {
            if (headersLength % 2 !== 0) {
              throw codes.INVALID_ARG_VALUE("headers", headers);
            }
            for (let i = 0;i < headersLength; i += 2) {
              head += processInformationHeader(headers[i], headers[i + 1]);
            }
          }
        } else {
          validateObject(headers, "headers");
          const keys = ObjectKeys(headers);
          for (let i = 0;i < keys.length; i++) {
            const key = keys[i];
            head += processInformationHeader(key, headers[key]);
          }
        }
      }
      head += `\r
    `;
      return this._writeRaw(head, "ascii", cb);
    };
    ServerResponse.prototype.writeProcessing = function(cb) {
      return this.writeInformation(102, null, cb);
    };
    ServerResponse.prototype.writeContinue = function(cb) {
      if (this.headersSent) {
        throw codes.HTTP_HEADERS_SENT("write");
      }
      if (!this[kHandle] || this[kPipelinedQueuedState] !== undefined) {
        this.writeInformation(100, null, cb);
        this._sent100 = true;
        return;
      }
      const native = this.socket?.[kHandle]?.response;
      if (native)
        native.writeContinue();
      else
        this[kHandle]?.writeContinue?.();
      this._sent100 = true;
      cb?.();
    };
    ServerResponse.prototype.end = function(chunk, encoding, callback) {
      const handle = this[kHandle];
      if (handle?.aborted) {
        return this;
      }
      if ((typeof chunk === "function")) {
        callback = chunk;
        chunk = undefined;
        encoding = undefined;
      } else if ((typeof encoding === "function")) {
        callback = encoding;
        encoding = undefined;
      } else if (!(typeof callback === "function")) {
        callback = undefined;
      }
      if (hasServerResponseFinished(this, chunk, callback)) {
        return this;
      }
      {
        const queuedState = this[kPipelinedQueuedState];
        if (queuedState !== undefined) {
          return bufferPipelinedEnd(this, queuedState, chunk, encoding, callback);
        }
      }
      if (!handle) {
        if (this[fakeSocketSymbol] || this.outputData?.length || !this._header) {
          return OutgoingMessagePrototype.end.call(this, chunk, encoding, callback);
        }
        if ((typeof callback === "function")) {
          process.nextTick(callback);
        }
        return this;
      }
      if (this[headerStateSymbol] === NodeHTTPHeaderState.none) {
        updateHasBody(this, this.statusCode);
      }
      if (chunk && !this._hasBody) {
        if (this[kRejectNonStandardBodyWrites]) {
          throw codes.HTTP_BODY_NOT_ALLOWED();
        } else {
          chunk = undefined;
        }
      }
      const trailer = this._trailer;
      if (trailer && this._hasBody && !this.hasHeader("content-length") && this.req?.httpVersionMajor === 1 && this.req?.httpVersionMinor >= 1) {
        this.socket?.[kHandle]?.setResponseTrailers(trailer);
      }
      const headerState = this[headerStateSymbol];
      callWriteHeadIfObservable(this, headerState, true);
      const flags = handle.flags;
      if (!!(flags & NodeHTTPResponseFlags.closed_or_completed)) {
        return true;
      }
      const sentState = NodeHTTPHeaderState.sent;
      if (headerState !== sentState) {
        {
          const renderedHeaders = renderNativeHeaders(this);
          try {
            this._contentLength = handle.writeHeadAndEnd(this[kSnapshotStatusCode] ?? this.statusCode, this[kSnapshotStatusMessage] ?? this.statusMessage, renderedHeaders, chunk, encoding, strictContentLength(this), renderedAutoHeaders, renderedKeepAliveSecs);
          } catch (e) {
            releaseRenderedHeaders(renderedHeaders);
            const code = e?.code;
            if (code !== "ERR_STREAM_ALREADY_FINISHED" && code !== "ERR_HTTP_HEADERS_SENT" && code !== "ERR_INVALID_CHAR" && !(e instanceof RangeError)) {
              this[headerStateSymbol] = sentState;
            }
            throw e;
          }
          releaseRenderedHeaders(renderedHeaders);
          this[headerStateSymbol] = sentState;
        }
      } else {
        if (!(!chunk && flags & NodeHTTPResponseFlags.ended) && !(flags & NodeHTTPResponseFlags.socket_closed)) {
          handle.end(chunk, encoding, undefined, strictContentLength(this));
        }
      }
      this._header = " ";
      const req = this.req;
      if (!req._consuming && !req?._readableState?.resumeScheduled) {
        req._dump();
      }
      this.finished = true;
      process.nextTick(markResponseEndedNT, this);
      this.emit("prefinish");
      this._callPendingCallbacks();
      if (callback) {
        process.nextTick(function(callback, self) {
          self.emit("finish");
          try {
            callback();
          } catch (err) {
            self.emit("error", err);
          }
          process.nextTick(emitCloseNT, self);
        }, callback, this);
      } else {
        process.nextTick(function(self) {
          self.emit("finish");
          process.nextTick(emitCloseNT, self);
        }, this);
      }
      return this;
    };
    Object.defineProperty(ServerResponse.prototype, "writable", {
      get() {
        return true;
      },
      set(_value) {}
    });
    ServerResponse.prototype.write = function(chunk, encoding, callback) {
      const handle = this[kHandle];
      if ((typeof chunk === "function")) {
        callback = chunk;
        chunk = undefined;
        encoding = undefined;
      } else if ((typeof encoding === "function")) {
        callback = encoding;
        encoding = undefined;
      } else if (!(typeof callback === "function")) {
        callback = undefined;
      }
      if (hasServerResponseFinished(this, chunk, callback)) {
        return false;
      }
      {
        const queuedState = this[kPipelinedQueuedState];
        if (queuedState !== undefined) {
          return bufferPipelinedWrite(this, queuedState, chunk, encoding, callback);
        }
      }
      let result = 0;
      const headerState = this[headerStateSymbol];
      callWriteHeadIfObservable(this, headerState);
      if (!handle) {
        return OutgoingMessagePrototype.write.call(this, chunk, encoding, callback);
      }
      if (this[headerStateSymbol] === NodeHTTPHeaderState.none) {
        updateHasBody(this, this.statusCode);
      }
      if (chunk && !this._hasBody) {
        if (this[kRejectNonStandardBodyWrites]) {
          throw codes.HTTP_BODY_NOT_ALLOWED();
        } else {
          chunk = undefined;
        }
      }
      const flags = handle.flags;
      if (!!(flags & NodeHTTPResponseFlags.closed_or_completed)) {
        return true;
      }
      if (this[headerStateSymbol] !== NodeHTTPHeaderState.sent) {
        handle.cork(() => {
          const renderedHeaders = renderNativeHeaders(this);
          try {
            handle.writeHead(this[kSnapshotStatusCode] ?? this.statusCode, this[kSnapshotStatusMessage] ?? this.statusMessage, renderedHeaders, renderedAutoHeaders, renderedKeepAliveSecs);
          } finally {
            releaseRenderedHeaders(renderedHeaders);
          }
          this[headerStateSymbol] = NodeHTTPHeaderState.sent;
          result = handle.write(chunk, encoding, allowWritesToContinue.bind(this), strictContentLength(this));
        });
      } else {
        result = handle.write(chunk, encoding, allowWritesToContinue.bind(this), strictContentLength(this));
      }
      if (result < 0) {
        if (callback) {
          this[kPendingCallbacks].push(callback);
        }
        return false;
      }
      this._callPendingCallbacks();
      if (callback) {
        process.nextTick(callback);
      }
      let written = 0;
      if (chunk) {
        written = typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
        if (written > 0 && this.getHeader("content-length") === undefined) {
          written += written.toString(16).length + 4;
        }
      }
      if (written > 0 && !this[kReplayingPipelinedOps]) {
        this[kBytesBuffered] = (this[kBytesBuffered] ?? 0) + written;
        scheduleWriteAccountingFlush(this);
        if (this[kBytesBuffered] >= this.writableHighWaterMark) {
          return false;
        }
      }
      return true;
    };
    const kBytesBuffered = Symbol("kBytesBuffered");
    const kAccountingFlushScheduled = Symbol("kAccountingFlushScheduled");
    function flushWriteAccountingNT(res) {
      res[kAccountingFlushScheduled] = false;
      const needsDrain = (res[kBytesBuffered] ?? 0) >= res.writableHighWaterMark;
      res[kBytesBuffered] = 0;
      if (needsDrain && !res.destroyed && !res.finished) {
        res.emit("drain");
      }
    }
    function scheduleWriteAccountingFlush(res) {
      if (res[kAccountingFlushScheduled])
        return;
      res[kAccountingFlushScheduled] = true;
      process.nextTick(flushWriteAccountingNT, res);
    }
    ServerResponse.prototype._callPendingCallbacks = function() {
      const originalLength = this[kPendingCallbacks].length;
      for (let i = 0;i < originalLength; ++i) {
        process.nextTick(this[kPendingCallbacks][i]);
      }
      if (this[kPendingCallbacks].length == originalLength) {
        this[kPendingCallbacks] = [];
      } else {
        this[kPendingCallbacks].splice(0, originalLength);
      }
    };
    ServerResponse.prototype._finish = function() {
      this.emit("prefinish");
    };
    ServerResponse.prototype.detachSocket = function(socket) {
      if (socket && socket._httpMessage === this) {
        if (socket[kCloseCallback])
          socket[kCloseCallback] = undefined;
        socket.removeListener("close", onServerResponseClose);
        socket._httpMessage = null;
        if (socket[kRequest] === this.req) {
          socket[kRequest] = undefined;
        }
      }
      this.socket = null;
    };
    ServerResponse.prototype._implicitHeader = function() {
      if (this.headersSent)
        return;
      this.writeHead(this.statusCode);
    };
    Object.defineProperty(ServerResponse.prototype, "writableNeedDrain", {
      get() {
        return !this.destroyed && !this.finished && ((this[kHandle]?.bufferedAmount ?? 0) !== 0 || (this[kBytesBuffered] ?? 0) >= this.writableHighWaterMark || (this[kPipelinedQueuedState]?.needDrain ?? false));
      }
    });
    Object.defineProperty(ServerResponse.prototype, "writableFinished", {
      get() {
        return !!(this.finished && (!this[kHandle] || this[kHandle].finished));
      }
    });
    Object.defineProperty(ServerResponse.prototype, "writableLength", {
      get() {
        if (this.writableFinished)
          return 0;
        return (this[kBytesBuffered] ?? 0) + (this[kHandle]?.bufferedAmount ?? 0) + (this[kPipelinedQueuedState]?.bytes ?? 0);
      }
    });
    Object.defineProperty(ServerResponse.prototype, "writableHighWaterMark", {
      get() {
        return this.socket?.writableHighWaterMark ?? this[kHighWaterMark] ?? 64 * 1024;
      }
    });
    Object.defineProperty(ServerResponse.prototype, "closed", {
      get() {
        return this._closed;
      }
    });
    ServerResponse.prototype._send = function(data, encoding, callback, _byteLength) {
      const handle = this[kHandle];
      if (!handle) {
        return OutgoingMessagePrototype._send.apply(this, arguments);
      }
      if (this[headerStateSymbol] !== NodeHTTPHeaderState.sent) {
        handle.cork(() => {
          const renderedHeaders = renderNativeHeaders(this);
          try {
            handle.writeHead(this[kSnapshotStatusCode] ?? this.statusCode, this[kSnapshotStatusMessage] ?? this.statusMessage, renderedHeaders, renderedAutoHeaders, renderedKeepAliveSecs);
          } finally {
            releaseRenderedHeaders(renderedHeaders);
          }
          this[headerStateSymbol] = NodeHTTPHeaderState.sent;
          handle.write(data, encoding, callback, strictContentLength(this));
        });
      } else {
        handle.write(data, encoding, callback, strictContentLength(this));
      }
    };
    const kSnapshotStatusCode = Symbol("kSnapshotStatusCode");
    const kSnapshotStatusMessage = Symbol("kSnapshotStatusMessage");
    const kFramingFrozenChunked = Symbol("kFramingFrozenChunked");
    const kImplicitHeaderFromEnd = Symbol("kImplicitHeaderFromEnd");
    ServerResponse.prototype.writeHead = function(statusCode, statusMessage, headers) {
      if (this.headersSent) {
        throw codes.HTTP_HEADERS_SENT("writeHead");
      }
      _writeHead(statusCode, statusMessage, headers, this);
      this[kSnapshotStatusCode] = this.statusCode;
      this[kSnapshotStatusMessage] = this.statusMessage;
      if (!this[kImplicitHeaderFromEnd] && !this.hasHeader("content-length") && !this.hasHeader("transfer-encoding")) {
        this[kFramingFrozenChunked] = true;
      }
      this[headerStateSymbol] = NodeHTTPHeaderState.assigned;
      if (!this[kHandle] && !this._header) {
        const statusLine = `HTTP/1.1 ${this.statusCode} ${this.statusMessage}\r
    `;
        this._storeHeader(statusLine, this[kOutHeaders]);
      }
      return this;
    };
    ServerResponse.prototype.assignSocket = function(socket) {
      if (socket._httpMessage) {
        throw codes.HTTP_SOCKET_ASSIGNED("Socket already assigned");
      }
      socket._httpMessage = this;
      socket.once("close", onServerResponseClose);
      this.socket = socket;
      this.emit("socket", socket);
      this._flush();
    };
    Object.defineProperty(ServerResponse.prototype, "shouldKeepAlive", {
      get() {
        return this[kShouldKeepAlive] ?? true;
      },
      set(value) {
        this[kShouldKeepAlive] = value;
      }
    });
    ServerResponse.prototype.destroy = function(err) {
      if (this.destroyed)
        return this;
      const handle = this[kHandle];
      this.destroyed = true;
      this[kErrored] = err;
      if (handle && this[kPipelinedQueuedState] === undefined) {
        handle.abort();
      }
      const socket = this[kSocket];
      if (socket) {
        socket.destroy(err);
      } else {
        process.nextTick(emitCloseNT, this);
      }
      return this;
    };
    ServerResponse.prototype.emit = function(event) {
      if (event === "close") {
        callCloseCallback(this);
      }
      return Stream.prototype.emit.apply(this, arguments);
    };
    ServerResponse.prototype.flushHeaders = function() {
      if (this[headerStateSymbol] === NodeHTTPHeaderState.sent)
        return;
      if (this[headerStateSymbol] !== NodeHTTPHeaderState.assigned)
        this._implicitHeader();
      if (this[kPipelinedQueuedState] !== undefined) {
        return;
      }
      const handle = this[kHandle];
      if (handle) {
        if (this[headerStateSymbol] === NodeHTTPHeaderState.assigned) {
          this[headerStateSymbol] = NodeHTTPHeaderState.sent;
          const renderedHeaders = renderNativeHeaders(this);
          try {
            handle.writeHead(this[kSnapshotStatusCode] ?? this.statusCode, this[kSnapshotStatusMessage] ?? this.statusMessage, renderedHeaders, renderedAutoHeaders, renderedKeepAliveSecs);
          } finally {
            releaseRenderedHeaders(renderedHeaders);
          }
        }
        handle.flushHeaders();
      } else {
        this._send("");
      }
    };
    function updateHasBody(response, statusCode) {
      if (statusCode === 204 || statusCode === 304 || statusCode >= 100 && statusCode <= 199) {
        response._hasBody = false;
      }
    }
    function emitServerSocketEOF(self, req) {
      self.push(null);
      if (req) {
        req.push(null);
        req.complete = true;
      }
    }
    function emitServerSocketEOFNT(self, req) {
      if (req) {
        req[eofInProgress] = true;
      }
      process.nextTick(emitServerSocketEOF, self);
    }
    let OriginalWriteHeadFn, OriginalImplicitHeadFn;
    function callWriteHeadIfObservable(self, headerState, fromEnd) {
      if (headerState === NodeHTTPHeaderState.none && !(self.writeHead === OriginalWriteHeadFn && self._implicitHeader === OriginalImplicitHeadFn)) {
        if (fromEnd)
          self[kImplicitHeaderFromEnd] = true;
        try {
          self.writeHead(self.statusCode, self.statusMessage);
        } finally {
          if (fromEnd)
            self[kImplicitHeaderFromEnd] = false;
        }
      }
    }
    function allowWritesToContinue() {
      this._callPendingCallbacks();
      this.emit("drain");
    }
    OriginalWriteHeadFn = ServerResponse.prototype.writeHead;
    OriginalImplicitHeadFn = ServerResponse.prototype._implicitHeader;
    function defaultShouldUpgradeCallback() {
      return this.listenerCount("upgrade") > 0;
    }
    function storeHTTPOptions(options) {
      this[kIncomingMessage] = options.IncomingMessage || IncomingMessage;
      this[kServerResponse] = options.ServerResponse || ServerResponse;
      const maxHeaderSize = options.maxHeaderSize;
      if (maxHeaderSize !== undefined)
        validateInteger(maxHeaderSize, "maxHeaderSize", 0);
      this.maxHeaderSize = maxHeaderSize;
      const insecureHTTPParser = options.insecureHTTPParser;
      if (insecureHTTPParser !== undefined)
        validateBoolean(insecureHTTPParser, "options.insecureHTTPParser");
      this.insecureHTTPParser = insecureHTTPParser;
      const httpValidation = options.httpValidation;
      if (httpValidation !== undefined) {
        validateOneOf(httpValidation, "options.httpValidation", ["strict", "relaxed", "insecure"]);
        if (insecureHTTPParser !== undefined) {
          throw codes.INVALID_ARG_VALUE("options.httpValidation", httpValidation, "cannot be used with options.insecureHTTPParser");
        }
        this.httpValidation = httpValidation;
      }
      this[kHighWaterMark] = options.highWaterMark;
      this[kUniqueHeaders] = parseUniqueHeadersOption(options.uniqueHeaders);
      const requestTimeout = options.requestTimeout;
      if (requestTimeout !== undefined) {
        validateInteger(requestTimeout, "requestTimeout", 0);
        this.requestTimeout = requestTimeout;
      } else {
        this.requestTimeout = 300000;
      }
      const headersTimeout = options.headersTimeout;
      if (headersTimeout !== undefined) {
        validateInteger(headersTimeout, "headersTimeout", 0);
        this.headersTimeout = headersTimeout;
      } else {
        this.headersTimeout = MathMin(60000, this.requestTimeout);
      }
      if (this.requestTimeout > 0 && this.headersTimeout > 0 && this.headersTimeout > this.requestTimeout) {
        throw codes.OUT_OF_RANGE("headersTimeout", "<= requestTimeout", headersTimeout);
      }
      const keepAliveTimeout = options.keepAliveTimeout;
      if (keepAliveTimeout !== undefined) {
        validateInteger(keepAliveTimeout, "keepAliveTimeout", 0);
        this.keepAliveTimeout = keepAliveTimeout;
      } else {
        this.keepAliveTimeout = 5000;
      }
      const keepAliveTimeoutBuffer = options.keepAliveTimeoutBuffer;
      if (keepAliveTimeoutBuffer !== undefined) {
        validateInteger(keepAliveTimeoutBuffer, "keepAliveTimeoutBuffer", 0);
        this.keepAliveTimeoutBuffer = keepAliveTimeoutBuffer;
      } else {
        this.keepAliveTimeoutBuffer = 1000;
      }
      const connectionsCheckingInterval = options.connectionsCheckingInterval;
      if (connectionsCheckingInterval !== undefined) {
        validateInteger(connectionsCheckingInterval, "connectionsCheckingInterval", 0);
        this.connectionsCheckingInterval = connectionsCheckingInterval;
      } else {
        this.connectionsCheckingInterval = 30000;
      }
      const requireHostHeader = options.requireHostHeader;
      if (requireHostHeader !== undefined) {
        validateBoolean(requireHostHeader, "options.requireHostHeader");
        this.requireHostHeader = requireHostHeader;
      } else {
        this.requireHostHeader = true;
      }
      const joinDuplicateHeaders = options.joinDuplicateHeaders;
      if (joinDuplicateHeaders !== undefined) {
        validateBoolean(joinDuplicateHeaders, "options.joinDuplicateHeaders");
      }
      this.joinDuplicateHeaders = joinDuplicateHeaders;
      const shouldUpgradeCallback = options.shouldUpgradeCallback;
      if (shouldUpgradeCallback !== undefined) {
        validateFunction(shouldUpgradeCallback, "options.shouldUpgradeCallback");
        this.shouldUpgradeCallback = shouldUpgradeCallback;
      } else {
        this.shouldUpgradeCallback = defaultShouldUpgradeCallback;
      }
      const rejectNonStandardBodyWrites = options.rejectNonStandardBodyWrites;
      if (rejectNonStandardBodyWrites !== undefined) {
        validateBoolean(rejectNonStandardBodyWrites, "options.rejectNonStandardBodyWrites");
        this.rejectNonStandardBodyWrites = rejectNonStandardBodyWrites;
      } else {
        this.rejectNonStandardBodyWrites = false;
      }
      const optimizeEmptyRequests = options.optimizeEmptyRequests;
      if (optimizeEmptyRequests !== undefined) {
        validateBoolean(optimizeEmptyRequests, "options.optimizeEmptyRequests");
      }
      this[kOptimizeEmptyRequests] = optimizeEmptyRequests || false;
    }
    return {
      Server,
      ServerResponse,
      kConnectionsCheckingInterval
    };
  })();
})
