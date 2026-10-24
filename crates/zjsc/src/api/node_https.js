// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/https.ts` do bun 1.4.2. Não editar à mão.
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
    const http = require("node:http");
    const { isIP } = require("internal/net/isIP");
    const { urlToHttpOptions } = require("internal/url");
    const { kEmptyObject, once } = require("internal/shared");
    const { validateObject } = require("internal/validators");
    const { kProxyConfig, checkShouldUseProxy, kWaitForProxyTunnel } = require("internal/http");
    const { validateHeaderValue } = require("node:_http_common");
    const ArrayPrototypeShift = Array.prototype.shift;
    const ObjectAssign = Object.assign;
    const ArrayPrototypeUnshift = Array.prototype.unshift;
    const JSONStringify = JSON.stringify;
    function request(...args) {
      let options = {};
      if (typeof args[0] === "string") {
        const urlStr = ArrayPrototypeShift.call(args);
        options = urlToHttpOptions(new URL(urlStr));
      } else if (args[0] instanceof URL) {
        options = urlToHttpOptions(ArrayPrototypeShift.call(args));
      }
      if (args[0] && typeof args[0] !== "function") {
        ObjectAssign.call(null, options, ArrayPrototypeShift.call(args));
      }
      options._defaultAgent = https.globalAgent;
      ArrayPrototypeUnshift.call(args, options);
      return new http.ClientRequest(...args);
    }
    function get(input, options, cb) {
      const req = request(input, options, cb);
      req.end();
      return req;
    }
    function getTunnelConfigForProxiedHttps(agent, reqOptions) {
      if (agent[kProxyConfig] === undefined || agent[kProxyConfig] === null) {
        return null;
      }
      if ((reqOptions.protocol || agent.protocol) !== "https:") {
        return null;
      }
      const shouldUseProxy = checkShouldUseProxy(agent[kProxyConfig], reqOptions);
      debug(`getTunnelConfigForProxiedHttps should use proxy for ${reqOptions.host}:${reqOptions.port}:`, shouldUseProxy);
      if (shouldUseProxy === false || shouldUseProxy === null || shouldUseProxy === undefined) {
        return null;
      }
      const { auth } = agent[kProxyConfig];
      const ipType = isIP(reqOptions.host);
      const requestHost = ipType === 6 ? `[${reqOptions.host}]` : reqOptions.host;
      const requestPort = reqOptions.port || agent.defaultPort;
      const endpoint = `${requestHost}:${requestPort}`;
      validateHeaderValue("host", endpoint);
      let payload = `CONNECT ${endpoint} HTTP/1.1\r
    `;
      if (auth) {
        payload += `proxy-authorization: ${auth}\r
    `;
      }
      if (agent.keepAlive || agent.maxSockets !== 1 / 0) {
        payload += `proxy-connection: keep-alive\r
    `;
      }
      payload += `host: ${endpoint}`;
      payload += `\r
    \r
    `;
      const result = {
        __proto__: null,
        proxyTunnelPayload: payload,
        requestOptions: {
          __proto__: null,
          servername: reqOptions.servername || (ipType ? undefined : reqOptions.host),
          ...reqOptions
        }
      };
      return result;
    }
    function establishTunnel(agent, socket, options, tunnelConfig, afterSocket) {
      const { proxyTunnelPayload } = tunnelConfig;
      let tunnelDone = false;
      function read() {
        let chunk;
        while (tunnelDone === false && (chunk = socket.read()) !== null) {
          if (onProxyData(chunk) !== -1) {
            break;
          }
        }
        if (tunnelDone === false) {
          socket.once("readable", read);
        }
      }
      function cleanup() {
        tunnelDone = true;
        socket.removeListener("end", onProxyEnd);
        socket.removeListener("error", onProxyError);
        socket.removeListener("readable", read);
        socket.setTimeout(0);
      }
      function onProxyError(err) {
        debug("onProxyError", err);
        cleanup();
        afterSocket(err, socket);
      }
      let buffer = "";
      function onProxyData(chunk) {
        const str = chunk.toString();
        debug("onProxyData", str);
        buffer += str;
        const headerEndIndex = buffer.indexOf(`\r
    \r
    `);
        if (headerEndIndex === -1)
          return headerEndIndex;
        const statusLine = buffer.substring(0, buffer.indexOf(`\r
    `));
        const statusCode = statusLine.split(" ")[1];
        if (statusCode !== "200") {
          debug(`onProxyData receives ${statusCode}, cleaning up`);
          cleanup();
          const targetHost = proxyTunnelPayload.split("\r")[0].split(" ")[1];
          const message = `Failed to establish tunnel to ${targetHost} via ${agent[kProxyConfig].href}: ${statusLine}`;
          const err = codes.PROXY_TUNNEL(message);
          err.statusCode = parseInt(statusCode);
          afterSocket(err, socket);
        } else {
          let onTLSHandshakeError = function(err) {
            debug("Propagate error event from tunneled socket to tunnel socket");
            afterSocket(err, tunneledSocket);
          }, onTLSHandshakeSuccess = function() {
            debug("TLS handshake over tunnel succeeded");
            tunneledSocket.removeListener("error", onTLSHandshakeError);
            afterSocket(null, tunneledSocket);
          }, onTunneledSocketFree = function() {
            debug("Propagate free event from tunneled socket to tunnel socket");
            socket.emit("free");
          };
          debug("onProxyData receives 200, establishing tunnel");
          cleanup();
          const { requestOptions } = tunnelConfig;
          tunnelConfig.requestOptions = null;
          requestOptions.socket = socket;
          let tunneledSocket;
          tunneledSocket = require("node:tls").connect(requestOptions, onTLSHandshakeSuccess);
          tunneledSocket.on("free", onTunneledSocketFree);
          tunneledSocket.on("error", onTLSHandshakeError);
          const agentKey = requestOptions._agentKey;
          if (agentKey) {
            tunneledSocket.on("session", onSocketSession.bind(agent, agentKey));
            tunneledSocket.once("close", onSocketClose.bind(agent, agentKey));
          }
        }
        return headerEndIndex;
      }
      function onProxyEnd() {
        cleanup();
        const err = codes.PROXY_TUNNEL("Connection to establish proxy tunnel ended unexpectedly");
        afterSocket(err, socket);
      }
      const proxyTunnelTimeout = tunnelConfig.requestOptions.timeout;
      function onProxyTimeout() {
        debug("onProxyTimeout", proxyTunnelTimeout);
        cleanup();
        const err = codes.PROXY_TUNNEL(`Connection to establish proxy tunnel timed out after ${proxyTunnelTimeout}ms`);
        err.proxyTunnelTimeout = proxyTunnelTimeout;
        afterSocket(err, socket);
      }
      if (proxyTunnelTimeout && proxyTunnelTimeout > 0) {
        debug("proxy tunnel setTimeout", proxyTunnelTimeout);
        socket.setTimeout(proxyTunnelTimeout, onProxyTimeout);
      }
      socket.on("error", onProxyError);
      socket.on("end", onProxyEnd);
      socket.write(proxyTunnelPayload);
      read();
    }
    function createConnection(...args) {
      let options;
      if (args[0] !== null && typeof args[0] === "object") {
        options = args[0];
      } else if (args[1] !== null && typeof args[1] === "object") {
        options = { ...args[1] };
      } else if (args[2] === null || typeof args[2] !== "object") {
        options = {};
      } else {
        options = { ...args[2] };
      }
      if (typeof args[0] === "number") {
        options.port = args[0];
      }
      if (typeof args[1] === "string") {
        options.host = args[1];
      }
      let cb;
      const lastArg = args[args.length - 1];
      if (typeof lastArg === "function") {
        cb = lastArg;
      }
      debug("https createConnection", options);
      const agentKey = options._agentKey;
      if (agentKey) {
        const session = this._getSession(agentKey);
        if (session) {
          debug("reuse session for %j", agentKey);
          options = {
            session,
            ...options
          };
        }
      }
      let socket;
      const tunnelConfig = getTunnelConfigForProxiedHttps(this, options);
      if (tunnelConfig === null) {
        socket = require("node:tls").connect(options);
      } else {
        let onError = function(err) {
          cleanupAndPropagate(err, socket);
        }, onTimeout = function() {
          const err = codes.PROXY_TUNNEL(`Connection to establish proxy tunnel timed out after ${proxyTunnelTimeout}ms`);
          err.proxyTunnelTimeout = proxyTunnelTimeout;
          cleanupAndPropagate(err, socket);
        }, onProxyConnection = function() {
          socket.removeListener("error", onError);
          establishTunnel(agent, socket, options, tunnelConfig, cleanupAndPropagate);
        };
        const connectOptions = {
          ...this[kProxyConfig].proxyConnectionOptions
        };
        debug("Create proxy socket", connectOptions);
        const agent = this;
        const proxyTunnelTimeout = tunnelConfig.requestOptions.timeout;
        const cleanupAndPropagate = once(function cleanupAndPropagateImpl(err, currentSocket) {
          debug("cleanupAndPropagate", err);
          socket.removeListener("error", onError);
          socket.removeListener("timeout", onTimeout);
          if (err && err.code === "ERR_PROXY_TUNNEL" && err.statusCode === undefined) {
            socket.destroy();
          }
          if (cb) {
            cb(err, currentSocket);
          }
        });
        if (this[kProxyConfig].protocol === "http:") {
          socket = require("node:net").connect(connectOptions, onProxyConnection);
        } else {
          socket = require("node:tls").connect(connectOptions, onProxyConnection);
        }
        socket.on("error", onError);
        if (proxyTunnelTimeout) {
          socket.setTimeout(proxyTunnelTimeout, onTimeout);
        }
        socket[kWaitForProxyTunnel] = true;
      }
      if (agentKey && tunnelConfig === null) {
        socket.on("session", onSocketSession.bind(this, agentKey));
        socket.once("close", onSocketClose.bind(this, agentKey));
      }
      return socket;
    }
    function onSocketSession(agentKey, session) {
      this._cacheSession(agentKey, session);
    }
    function onSocketClose(agentKey, err) {
      if (err)
        this._evictSession(agentKey);
    }
    function Agent(options) {
      if (!(this instanceof Agent))
        return new Agent(options);
      options = { __proto__: null, ...options };
      options.defaultPort ??= 443;
      options.protocol ??= "https:";
      http.Agent.call(this, options);
      this.maxCachedSessions = this.options.maxCachedSessions;
      if (this.maxCachedSessions === undefined)
        this.maxCachedSessions = 100;
      this._sessionCache = {
        map: {},
        list: []
      };
    }
    toClass(Agent, "Agent", http.Agent);
    Agent.prototype.createConnection = createConnection;
    Agent.prototype.getName = function getName(options = kEmptyObject) {
      let name = http.Agent.prototype.getName.call(this, options);
      const {
        ca,
        cert,
        clientCertEngine,
        ciphers,
        key,
        pfx,
        rejectUnauthorized,
        servername,
        host,
        minVersion,
        maxVersion,
        secureProtocol,
        crl,
        honorCipherOrder,
        ecdhCurve,
        dhparam,
        secureOptions,
        sessionIdContext,
        sigalgs,
        privateKeyIdentifier,
        privateKeyEngine
      } = options;
      name += ":";
      if (ca)
        name += ca;
      name += ":";
      if (cert)
        name += cert;
      name += ":";
      if (clientCertEngine)
        name += clientCertEngine;
      name += ":";
      if (ciphers)
        name += ciphers;
      name += ":";
      if (key)
        name += key;
      name += ":";
      if (pfx)
        name += pfx;
      name += ":";
      if (rejectUnauthorized !== undefined)
        name += rejectUnauthorized;
      name += ":";
      if (servername && servername !== host)
        name += servername;
      name += ":";
      if (minVersion)
        name += minVersion;
      name += ":";
      if (maxVersion)
        name += maxVersion;
      name += ":";
      if (secureProtocol)
        name += secureProtocol;
      name += ":";
      if (crl)
        name += crl;
      name += ":";
      if (honorCipherOrder !== undefined)
        name += honorCipherOrder;
      name += ":";
      if (ecdhCurve)
        name += ecdhCurve;
      name += ":";
      if (dhparam)
        name += dhparam;
      name += ":";
      if (secureOptions !== undefined)
        name += secureOptions;
      name += ":";
      if (sessionIdContext)
        name += sessionIdContext;
      name += ":";
      if (sigalgs)
        name += JSONStringify(sigalgs);
      name += ":";
      if (privateKeyIdentifier)
        name += privateKeyIdentifier;
      name += ":";
      if (privateKeyEngine)
        name += privateKeyEngine;
      return name;
    };
    Agent.prototype._getSession = function _getSession(key) {
      return this._sessionCache.map[key];
    };
    Agent.prototype._cacheSession = function _cacheSession(key, session) {
      if (this.maxCachedSessions === 0)
        return;
      const sessionMap = this._sessionCache.map;
      if (sessionMap[key]) {
        sessionMap[key] = session;
        return;
      }
      if (this._sessionCache.list.length >= this.maxCachedSessions) {
        const oldKey = this._sessionCache.list.shift();
        debug("evicting %j", oldKey);
        delete this._sessionCache.map[oldKey];
      }
      this._sessionCache.list.push(key);
      this._sessionCache.map[key] = session;
    };
    Agent.prototype._evictSession = function _evictSession(key) {
      const index = this._sessionCache.list.indexOf(key);
      if (index === -1)
        return;
      this._sessionCache.list.splice(index, 1);
      delete this._sessionCache.map[key];
    };
    const { shouldUseEnvProxy } = require("node:_http_agent");
    function createServer(options, requestListener) {
      if (typeof options === "function") {
        requestListener = options;
        options = {};
      } else if (options == null) {
        options = {};
      } else {
        validateObject(options, "options");
        options = { ...options };
      }
      if (!options.ALPNProtocols && !options.ALPNCallback) {
        options.ALPNProtocols = ["http/1.1"];
      }
      const server = http.createServer(options, requestListener);
      const optionsALPNProtocols = options.ALPNProtocols;
      if (optionsALPNProtocols) {
        require("node:tls").convertALPNProtocols(optionsALPNProtocols, server);
      }
      server.ALPNCallback = options.ALPNCallback;
      return server;
    }
    var https = {
      Agent,
      globalAgent: new Agent({
        keepAlive: true,
        scheduling: "lifo",
        timeout: 5000,
        proxyEnv: shouldUseEnvProxy() ? process.env : undefined
      }),
      Server: http.Server,
      createServer,
      get,
      request
    };
    return https;
  })();
})
