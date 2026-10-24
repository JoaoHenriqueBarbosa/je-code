// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/_http_agent.ts` do bun 1.4.2. Não editar à mão.
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
    const { parseProxyConfigFromEnv, kProxyConfig, checkShouldUseProxy, kWaitForProxyTunnel } = require("internal/http");
    const { getLazy, kEmptyObject, once } = require("internal/shared");
    const { validateNumber, validateOneOf, validateString } = require("internal/validators");
    const { isIP } = require("internal/net/isIP");
    const kOnKeylog = Symbol("onkeylog");
    const kRequestOptions = Symbol("requestOptions");
    const kRequestAsyncResource = Symbol("requestAsyncResource");
    function freeSocketErrorListener(err) {
      const socket = this;
      debug("SOCKET ERROR on FREE socket:", err.message, err.stack);
      socket.destroy();
      socket.emit("agentRemove");
    }
    function Agent(options) {
      if (!(this instanceof Agent))
        return new Agent(options);
      EventEmitter.call(this);
      this.options = { __proto__: null, ...options };
      this.defaultPort = this.options.defaultPort || 80;
      this.protocol = this.options.protocol || "http:";
      if (this.options.noDelay === undefined)
        this.options.noDelay = true;
      this.options.path = null;
      this.requests = { __proto__: null };
      this.sockets = { __proto__: null };
      this.freeSockets = { __proto__: null };
      this.keepAliveMsecs = this.options.keepAliveMsecs || 1000;
      this.keepAlive = this.options.keepAlive || false;
      this.maxSockets = this.options.maxSockets || Agent.defaultMaxSockets;
      this.maxFreeSockets = this.options.maxFreeSockets || 256;
      this.scheduling = this.options.scheduling || "lifo";
      this.maxTotalSockets = this.options.maxTotalSockets;
      this.totalSocketCount = 0;
      this.agentKeepAliveTimeoutBuffer = typeof this.options.agentKeepAliveTimeoutBuffer === "number" && this.options.agentKeepAliveTimeoutBuffer >= 0 && Number.isFinite(this.options.agentKeepAliveTimeoutBuffer) ? this.options.agentKeepAliveTimeoutBuffer : 1000;
      const proxyEnv = this.options.proxyEnv;
      if (typeof proxyEnv === "object" && proxyEnv !== null) {
        this[kProxyConfig] = parseProxyConfigFromEnv(proxyEnv, this.protocol, this.keepAlive);
        debug(`new ${this.protocol} agent with proxy config`, this[kProxyConfig]);
      }
      validateOneOf(this.scheduling, "scheduling", ["fifo", "lifo"]);
      const maxTotalSockets = this.maxTotalSockets;
      if (maxTotalSockets !== undefined) {
        validateNumber(maxTotalSockets, "maxTotalSockets", 1);
      } else {
        this.maxTotalSockets = 1 / 0;
      }
      this.on("free", (socket, options) => {
        const name = this.getName(options);
        debug("agent.on(free)", name);
        if (!socket.writable) {
          socket.destroy();
          return;
        }
        const requests = this.requests[name];
        if (requests?.length) {
          const req = requests.shift();
          const reqAsyncRes = req[kRequestAsyncResource];
          if (reqAsyncRes) {
            reqAsyncRes.runInAsyncScope(setRequestSocket, undefined, this, req, socket);
            req[kRequestAsyncResource] = null;
          } else {
            setRequestSocket(this, req, socket);
          }
          if (requests.length === 0) {
            delete this.requests[name];
          }
          return;
        }
        const req = socket._httpMessage;
        if (!req || !req.shouldKeepAlive || !this.keepAlive) {
          socket.destroy();
          return;
        }
        const freeSockets = this.freeSockets[name] || [];
        const freeLen = freeSockets.length;
        let count = freeLen;
        const namedSockets = this.sockets[name];
        if (namedSockets)
          count += namedSockets.length;
        if (this.totalSocketCount > this.maxTotalSockets || count > this.maxSockets || freeLen >= this.maxFreeSockets || !this.keepSocketAlive(socket)) {
          socket.destroy();
          return;
        }
        this.freeSockets[name] = freeSockets;
        socket._httpMessage = null;
        this.removeSocket(socket, options);
        socket.once("error", freeSocketErrorListener);
        freeSockets.push(socket);
      });
      this.on("newListener", maybeEnableKeylog);
    }
    toClass(Agent, "Agent", EventEmitter);
    function maybeEnableKeylog(eventName) {
      if (eventName === "keylog") {
        this.removeListener("newListener", maybeEnableKeylog);
        const agent = this;
        this[kOnKeylog] = function onkeylog(keylog) {
          agent.emit("keylog", keylog, this);
        };
        for (const socket of Object.values(this.sockets).flat()) {
          socket.on("keylog", this[kOnKeylog]);
        }
      }
    }
    const tls = getLazy(() => require("node:tls"));
    const net = getLazy(() => require("node:net"));
    Agent.defaultMaxSockets = 1 / 0;
    Agent.prototype.createConnection = function createConnection(...args) {
      const normalized = net()._normalizeArgs(args);
      const options = normalized[0];
      const cb = normalized[1];
      const shouldUseProxy = checkShouldUseProxy(this[kProxyConfig], options);
      debug(`http createConnection should use proxy for ${options.host}:${options.port}:`, shouldUseProxy);
      if (!shouldUseProxy) {
        return net().createConnection(...args);
      }
      const connectOptions = {
        ...this[kProxyConfig].proxyConnectionOptions
      };
      const proxyProtocol = this[kProxyConfig].protocol;
      if (proxyProtocol === "http:") {
        return net().connect(connectOptions, cb);
      } else if (proxyProtocol === "https:") {
        return tls().connect(connectOptions, cb);
      }
      assert(false, `Unexpected proxy protocol ${proxyProtocol}`);
    };
    Agent.prototype.getName = function getName(options = kEmptyObject) {
      const { host, port, localAddress, family, socketPath } = options;
      let name = host || "localhost";
      name += ":";
      if (port)
        name += port;
      name += ":";
      if (localAddress)
        name += localAddress;
      if (family === 4 || family === 6)
        name += `:${family}`;
      if (socketPath)
        name += `:${socketPath}`;
      return name;
    };
    function handleSocketAfterProxy(err, req) {
      if (err.code === "ERR_PROXY_TUNNEL") {
        if (err.proxyTunnelTimeout) {
          req.emit("timeout");
        }
        req.emit("error", err);
      }
    }
    Agent.prototype.addRequest = function addRequest(req, options, port, localAddress) {
      if (typeof options === "string") {
        options = {
          __proto__: null,
          host: options,
          port,
          localAddress
        };
      }
      options = { __proto__: null, ...options, ...this.options };
      const socketPath = options.socketPath;
      if (socketPath)
        options.path = socketPath;
      normalizeServerName(options, req);
      const name = this.getName(options);
      this.sockets[name] ||= [];
      const freeSockets = this.freeSockets[name];
      let socket;
      if (freeSockets) {
        while (freeSockets.length && freeSockets[0].destroyed) {
          freeSockets.shift();
        }
        socket = this.scheduling === "fifo" ? freeSockets.shift() : freeSockets.pop();
        if (!freeSockets.length)
          delete this.freeSockets[name];
      }
      const freeLen = freeSockets ? freeSockets.length : 0;
      const sockLen = freeLen + this.sockets[name].length;
      if (socket) {
        this.reuseSocket(socket, req);
        setRequestSocket(this, req, socket);
        this.sockets[name].push(socket);
      } else if (sockLen < this.maxSockets && this.totalSocketCount < this.maxTotalSockets) {
        debug("call onSocket", sockLen, freeLen);
        this.createSocket(req, options, onSocketCreated.bind(this, req));
      } else {
        debug("wait for socket");
        this.requests[name] ||= [];
        req[kRequestOptions] = options;
        req[kRequestAsyncResource] = new (require("node:async_hooks")).AsyncResource("QueuedRequest");
        this.requests[name].push(req);
      }
    };
    Agent.prototype.createSocket = function createSocket(req, options, cb) {
      options = { __proto__: null, ...options, ...this.options };
      const socketPath = options.socketPath;
      if (socketPath)
        options.path = socketPath;
      normalizeServerName(options, req);
      const timeout = req.timeout || this.options.timeout || undefined;
      if (timeout) {
        options.timeout = timeout;
      }
      const name = this.getName(options);
      options._agentKey = name;
      debug("createConnection", name);
      options.encoding = null;
      const oncreate = once((err, s) => {
        const done = cb;
        cb = undefined;
        if (err)
          return done(err, s);
        this.sockets[name] ||= [];
        this.sockets[name].push(s);
        this.totalSocketCount++;
        debug("sockets", name, this.sockets[name].length, this.totalSocketCount);
        installListeners(this, s, options);
        done(null, s);
      });
      const keepAlive = this.keepAlive;
      if (keepAlive) {
        options.keepAlive = keepAlive;
        options.keepAliveInitialDelay = this.keepAliveMsecs;
      }
      const newSocket = this.createConnection(options, oncreate);
      if (newSocket && !newSocket[kWaitForProxyTunnel])
        oncreate(null, newSocket);
    };
    function normalizeServerName(options, req) {
      if (!options.servername && options.servername !== "")
        options.servername = calculateServerName(options, req);
    }
    function calculateServerName(options, req) {
      let servername = options.host;
      const hostHeader = req.getHeader("host");
      if (hostHeader) {
        validateString(hostHeader, "options.headers.host");
        if (hostHeader[0] === "[") {
          const index = hostHeader.indexOf("]");
          if (index === -1) {
            servername = hostHeader;
          } else {
            servername = hostHeader.substring(1, index);
          }
        } else {
          servername = hostHeader.split(":", 1)[0];
        }
      }
      if (isIP(servername))
        servername = "";
      return servername;
    }
    function installListeners(agent, s, options) {
      function onFree() {
        debug("CLIENT socket onFree");
        agent.emit("free", s, options);
      }
      s.on("free", onFree);
      function onClose() {
        debug("CLIENT socket onClose");
        agent.totalSocketCount--;
        agent.removeSocket(s, options);
      }
      s.on("close", onClose);
      function onTimeout() {
        debug("CLIENT socket onTimeout");
        const sockets = agent.freeSockets;
        if (Object.keys(sockets).some((name) => sockets[name].includes(s))) {
          return s.destroy();
        }
      }
      s.on("timeout", onTimeout);
      function onRemove() {
        debug("CLIENT socket onRemove");
        agent.totalSocketCount--;
        agent.removeSocket(s, options);
        s.removeListener("close", onClose);
        s.removeListener("free", onFree);
        s.removeListener("timeout", onTimeout);
        s.removeListener("agentRemove", onRemove);
      }
      s.on("agentRemove", onRemove);
      if (agent[kOnKeylog]) {
        s.on("keylog", agent[kOnKeylog]);
      }
    }
    Agent.prototype.removeSocket = function removeSocket(s, options) {
      const name = this.getName(options);
      debug("removeSocket", name, "writable:", s.writable);
      const sets = [this.sockets];
      if (!s.writable)
        sets.push(this.freeSockets);
      for (let sk = 0;sk < sets.length; sk++) {
        const sockets = sets[sk];
        const socket = sockets[name];
        if (socket) {
          const index = socket.indexOf(s);
          if (index !== -1) {
            socket.splice(index, 1);
            if (socket.length === 0)
              delete sockets[name];
          }
        }
      }
      let req;
      const requests = this.requests;
      if (requests[name]?.length) {
        debug("removeSocket, have a request, make a socket");
        req = requests[name][0];
      } else {
        const keys = Object.keys(requests);
        for (let i = 0;i < keys.length; i++) {
          const prop = keys[i];
          if (this.sockets[prop]?.length)
            break;
          debug("removeSocket, have a request with different origin, make a socket");
          req = this.requests[prop][0];
          options = req[kRequestOptions];
          break;
        }
      }
      if (req && options) {
        req[kRequestOptions] = undefined;
        this.createSocket(req, options, onSocketCreatedForPending.bind(undefined, req));
      }
    };
    function onSocketCreated(req, err, socket) {
      if (err) {
        handleSocketAfterProxy(err, req);
        req.onSocket(socket, err);
        return;
      }
      setRequestSocket(this, req, socket);
    }
    function onSocketCreatedForPending(req, err, socket) {
      if (err) {
        handleSocketAfterProxy(err, req);
        req.onSocket(socket ?? null, err);
        return;
      }
      socket.emit("free");
    }
    Agent.prototype.keepSocketAlive = function keepSocketAlive(socket) {
      socket.setKeepAlive(true, this.keepAliveMsecs);
      socket.unref();
      let agentTimeout = this.options.timeout || 0;
      let canKeepSocketAlive = true;
      const res = socket._httpMessage?.res;
      if (res) {
        const keepAliveHint = res.headers["keep-alive"];
        if (keepAliveHint) {
          const hint = /^timeout=(\d+)/.exec(keepAliveHint)?.[1];
          if (hint) {
            let serverHintTimeout = Number.parseInt(hint) * 1000 - this.agentKeepAliveTimeoutBuffer;
            serverHintTimeout = serverHintTimeout > 0 ? serverHintTimeout : 0;
            if (serverHintTimeout === 0) {
              canKeepSocketAlive = false;
            } else if (serverHintTimeout < agentTimeout) {
              agentTimeout = serverHintTimeout;
            }
          }
        }
      }
      if (socket.timeout !== agentTimeout) {
        socket.setTimeout(agentTimeout);
      }
      return canKeepSocketAlive;
    };
    Agent.prototype.reuseSocket = function reuseSocket(socket, req) {
      debug("have free socket");
      socket.removeListener("error", freeSocketErrorListener);
      req.reusedSocket = true;
      socket.ref();
    };
    Agent.prototype.destroy = function destroy() {
      const sets = [this.freeSockets, this.sockets];
      for (let s = 0;s < sets.length; s++) {
        const set = sets[s];
        const keys = Object.keys(set);
        for (let v = 0;v < keys.length; v++) {
          const setName = set[keys[v]];
          for (let n = 0;n < setName.length; n++) {
            setName[n].destroy();
          }
        }
      }
    };
    function setRequestSocket(agent, req, socket) {
      req.onSocket(socket);
      const agentTimeout = agent.options.timeout || 0;
      if (req.timeout === undefined || req.timeout === agentTimeout) {
        return;
      }
      socket.setTimeout(req.timeout);
    }
    function shouldUseEnvProxy() {
      const value = process.env.NODE_USE_ENV_PROXY;
      if (Boolean(value) === false || value === "0") {
        return false;
      }
      const httpProxy = process.env.http_proxy || process.env.HTTP_PROXY;
      const httpsProxy = process.env.https_proxy || process.env.HTTPS_PROXY;
      if (httpProxy && URL.canParse(httpProxy) === false) {
        throw codes.INVALID_URL(httpProxy);
      }
      if (httpsProxy && URL.canParse(httpsProxy) === false) {
        throw codes.INVALID_URL(httpsProxy);
      }
      return true;
    }
    return {
      Agent,
      globalAgent: new Agent({
        keepAlive: true,
        scheduling: "lifo",
        timeout: 5000,
        proxyEnv: shouldUseEnvProxy() ? process.env : undefined
      }),
      shouldUseEnvProxy
    };
  })();
})
