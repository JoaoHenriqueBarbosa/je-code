// `Bun.serve` e o `fetch` de `http:` em loopback do bun 1.4.2 (ver `bun_http.rs` e `net_loop.rs`). Programa escondido:
// recebe o `host` (as funções `net*` sobre o TCP do `Host`, `responseWire`, `markFetched`, `refused`) e devolve
// `{ serve, fetch }`. HTTP/1.1 escrito aqui: conexão não bloqueante, uma tarefa assíncrona por conexão, o laço de eventos
// acorda a tarefa quando o descritor fica pronto. Função de código embutido não tem `prototype` próprio, então tudo é
// `class` ou objeto literal.
(function (host) {
  "use strict";

  const EAGAIN = 11;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  const REASONS = {
    100: "Continue", 101: "Switching Protocols", 102: "Processing", 103: "Early Hints",
    200: "OK", 201: "Created", 202: "Accepted", 203: "Non-Authoritative Information", 204: "No Content", 205: "Reset Content",
    206: "Partial Content", 207: "Multi-Status", 208: "Already Reported", 226: "IM Used",
    300: "Multiple Choices", 301: "Moved Permanently", 302: "Found", 303: "See Other", 304: "Not Modified", 305: "Use Proxy",
    307: "Temporary Redirect", 308: "Permanent Redirect",
    400: "Bad Request", 401: "Unauthorized", 402: "Payment Required", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed",
    406: "Not Acceptable", 407: "Proxy Authentication Required", 408: "Request Timeout", 409: "Conflict", 410: "Gone",
    411: "Length Required", 412: "Precondition Failed", 413: "Payload Too Large", 414: "URI Too Long", 415: "Unsupported Media Type",
    416: "Range Not Satisfiable", 417: "Expectation Failed", 418: "I'm a Teapot", 421: "Misdirected Request",
    422: "Unprocessable Entity", 423: "Locked", 424: "Failed Dependency", 425: "Too Early", 426: "Upgrade Required",
    428: "Precondition Required", 429: "Too Many Requests", 431: "Request Header Fields Too Large", 451: "Unavailable For Legal Reasons",
    500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout",
    505: "HTTP Version Not Supported", 506: "Variant Also Negotiates", 507: "Insufficient Storage", 508: "Loop Detected",
    510: "Not Extended", 511: "Network Authentication Required",
  };

  function concat(first, second) {
    if (first.length === 0) return second;
    if (second.length === 0) return first;
    const out = new Uint8Array(first.length + second.length);
    out.set(first, 0);
    out.set(second, first.length);
    return out;
  }

  function concatAll(chunks) {
    let length = 0;
    for (const chunk of chunks) length += chunk.length;
    const out = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      out.set(chunk, offset);
      offset += chunk.length;
    }
    return out;
  }

  function headEnd(bytes) {
    for (let index = 0; index + 3 < bytes.length; index++) {
      if (bytes[index] === 13 && bytes[index + 1] === 10 && bytes[index + 2] === 13 && bytes[index + 3] === 10) return index;
    }
    return -1;
  }

  function errnoError(result) {
    const error = new Error("socket error");
    error.errno = -result;
    return error;
  }


  // Uma conexão TCP não bloqueante: leitura com buffer e escrita que espera o descritor ficar pronto.
  class Conn {
    constructor(fd) {
      this.fd = fd;
      this.pending = new Uint8Array(0);
      this.eof = false;
      this.closed = false;
      this.readWaiter = null;
      this.writeWaiter = null;
    }

    rewatch() {
      if (this.closed) return;
      if (this.readWaiter === null && this.writeWaiter === null) {
        host.netUnwatch(this.fd);
        return;
      }
      host.netWatch(this.fd, this.readWaiter !== null, this.writeWaiter !== null, (readable, writable, hangup, error) => {
        const reader = this.readWaiter;
        const writer = this.writeWaiter;
        if (reader !== null && (readable || hangup || error)) {
          this.readWaiter = null;
          reader();
        }
        if (writer !== null && (writable || hangup || error)) {
          this.writeWaiter = null;
          writer();
        }
        this.rewatch();
      });
    }

    waitReadable() {
      return new Promise(resolve => {
        this.readWaiter = resolve;
        this.rewatch();
      });
    }

    waitWritable() {
      return new Promise(resolve => {
        this.writeWaiter = resolve;
        this.rewatch();
      });
    }

    // O que o descritor tem agora; vazio é o fim da conexão.
    async recv(max) {
      for (;;) {
        if (this.closed) return new Uint8Array(0);
        const result = host.netRecv(this.fd, max);
        if (typeof result === "number") {
          if (result === -EAGAIN) {
            await this.waitReadable();
            continue;
          }
          if (result === -104 || result === -32) return new Uint8Array(0);
          throw errnoError(result);
        }
        return result;
      }
    }

    async send(bytes) {
      let offset = 0;
      while (offset < bytes.length) {
        if (this.closed) throw errnoError(-32);
        const sent = host.netSend(this.fd, offset === 0 ? bytes : bytes.subarray(offset));
        if (sent < 0) {
          if (sent === -EAGAIN) {
            await this.waitWritable();
            continue;
          }
          throw errnoError(sent);
        }
        offset += sent;
      }
    }

    async fill() {
      const chunk = await this.recv(65536);
      if (chunk.length === 0) this.eof = true;
      else this.pending = concat(this.pending, chunk);
    }

    // Os bytes antes de `\r\n\r\n` como texto (a linha inicial e os cabeçalhos), ou `null` se a conexão fechou antes.
    async readHead() {
      for (;;) {
        const end = headEnd(this.pending);
        if (end >= 0) {
          const head = decoder.decode(this.pending.subarray(0, end));
          this.pending = this.pending.subarray(end + 4);
          return head;
        }
        if (this.eof) return null;
        await this.fill();
      }
    }

    async readLine() {
      for (;;) {
        const end = this.pending.indexOf(10);
        if (end >= 0) {
          const line = decoder.decode(this.pending.subarray(0, end)).replace(/\r$/, "");
          this.pending = this.pending.subarray(end + 1);
          return line;
        }
        if (this.eof) throw new Error("unexpected end of connection");
        await this.fill();
      }
    }

    // Até `max` bytes: o que já está no buffer, senão o que chegar; vazio é o fim da conexão.
    async readSome(max) {
      if (this.pending.length === 0 && !this.eof) await this.fill();
      const out = this.pending.subarray(0, max);
      this.pending = this.pending.subarray(out.length);
      return out;
    }

    close() {
      if (this.closed) return;
      this.closed = true;
      host.netClose(this.fd);
      const reader = this.readWaiter;
      const writer = this.writeWaiter;
      this.readWaiter = null;
      this.writeWaiter = null;
      if (reader !== null) reader();
      if (writer !== null) writer();
    }
  }

  function parseHead(head) {
    const lines = head.split("\r\n");
    const headers = [];
    for (let index = 1; index < lines.length; index++) {
      const line = lines[index];
      const colon = line.indexOf(":");
      if (colon <= 0) continue;
      headers.push([line.slice(0, colon).trim(), line.slice(colon + 1).trim()]);
    }
    return { start: lines[0], headers };
  }

  function headerOf(headers, name) {
    const lower = name.toLowerCase();
    const values = [];
    for (const [key, value] of headers) if (key.toLowerCase() === lower) values.push(value);
    return values.length === 0 ? undefined : values.join(", ");
  }

  // O leitor do corpo de uma mensagem: devolve uma função assíncrona que entrega o próximo pedaço (`null` no fim), ou
  // `null` quando a mensagem não tem corpo. `untilClose` é o corpo sem tamanho de uma resposta (lido até fechar).
  function bodyReader(conn, headers, untilClose) {
    const encoding = headerOf(headers, "transfer-encoding");
    if (encoding !== undefined && /chunked/i.test(encoding)) {
      let remaining = 0;
      let finished = false;
      return async () => {
        if (finished) return null;
        if (remaining === 0) {
          const size = parseInt((await conn.readLine()).split(";")[0], 16);
          if (!(size > 0)) {
            for (;;) if ((await conn.readLine()) === "") break;
            finished = true;
            return null;
          }
          remaining = size;
        }
        const chunk = await conn.readSome(remaining);
        if (chunk.length === 0) throw new Error("unexpected end of connection");
        remaining -= chunk.length;
        if (remaining === 0) await conn.readLine();
        return chunk;
      };
    }
    const length = headerOf(headers, "content-length");
    if (length !== undefined) {
      let remaining = Number(length);
      if (!(remaining > 0)) return null;
      return async () => {
        if (remaining === 0) return null;
        const chunk = await conn.readSome(remaining);
        if (chunk.length === 0) throw new Error("unexpected end of connection");
        remaining -= chunk.length;
        return chunk;
      };
    }
    if (!untilClose) return null;
    return async () => {
      const chunk = await conn.readSome(65536);
      return chunk.length === 0 ? null : chunk;
    };
  }

  async function collect(next) {
    const chunks = [];
    for (;;) {
      const chunk = await next();
      if (chunk === null) return concatAll(chunks);
      chunks.push(chunk);
    }
  }

  function bytesOf(value) {
    if (typeof value === "string") return encoder.encode(value);
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }

  // ---- servidor ----

  function routeMatch(pattern, pathname) {
    if (pattern === pathname) return {};
    const wanted = pattern.split("/");
    const actual = pathname.split("/");
    const params = {};
    for (let index = 0; index < wanted.length; index++) {
      const part = wanted[index];
      if (part === "*" && index === wanted.length - 1) return params;
      if (index >= actual.length) return null;
      if (part.startsWith(":")) params[part.slice(1)] = decodeURIComponent(actual[index]);
      else if (part !== actual[index]) return null;
    }
    return wanted.length === actual.length ? params : null;
  }

  function routeHandler(routes, request, pathname) {
    let best = null;
    let bestScore = -1;
    for (const pattern of Object.keys(routes)) {
      const params = routeMatch(pattern, pathname);
      if (params === null) continue;
      const score = pattern === pathname ? 3 : pattern.includes(":") ? 2 : 1;
      if (score > bestScore) {
        best = { pattern, params };
        bestScore = score;
      }
    }
    if (best === null) return undefined;
    let route = routes[best.pattern];
    if (route === false) return undefined;
    if (route !== null && typeof route === "object" && !(route instanceof Response)) {
      route = route[request.method];
      if (route === undefined) return undefined;
    }
    request.params = best.params;
    return { route };
  }

  class Server {
    constructor(options, listener, port) {
      this.options = options;
      this.listener = listener;
      this.port = port;
      this.hostname = options.hostname === undefined ? "localhost" : String(options.hostname);
      this.development = options.development === undefined ? true : Boolean(options.development);
      this.id = options.id === undefined ? crypto.randomUUID() : String(options.id);
      this.pendingRequests = 0;
      this.pendingWebSockets = 0;
      this.stopped = false;
      this.connections = new Set();
      this.drained = [];
      this.fetchHandler = options.fetch;
      this.errorHandler = options.error;
      this.routes = options.routes;
    }

    get url() {
      return new URL(`http://${this.hostname === "0.0.0.0" ? "0.0.0.0" : this.hostname}:${this.port}/`);
    }

    async dispatch(request) {
      const pathname = new URL(request.url).pathname;
      if (this.routes !== undefined && this.routes !== null) {
        const found = routeHandler(this.routes, request, pathname);
        if (found !== undefined) {
          const route = found.route;
          if (route instanceof Response) return route.clone();
          if (typeof route === "function") return await route(request, this);
        }
      }
      if (typeof this.fetchHandler === "function") return await this.fetchHandler.call(this.options, request, this);
      return new Response("Not Found", { status: 404 });
    }

    async handle(request) {
      try {
        const response = await this.dispatch(request);
        if (!(response instanceof Response)) {
          // O bun responde com a página de boas-vindas e, em seguida, o erro mata o processo (medido no 1.4.2: o
          // cliente recebe a resposta e o stderr termina com `error: Expected a Response object...`, código 1).
          const text = `Expected a Response object, but received '${String(response)}'`;
          if (response === undefined) {
            setTimeout(() => host.fatal(text), 0);
            return new Response("Welcome to Bun! To get started, return a Response object.");
          }
          throw new TypeError(text);
        }
        return response;
      } catch (error) {
        if (typeof this.errorHandler === "function") {
          try {
            const response = await this.errorHandler.call(this.options, error);
            if (response instanceof Response) return response;
          } catch (inner) {
            host.reportError(inner, `${request.method} - ${new URL(request.url).pathname} failed`);
          }
        } else {
          host.reportError(error, `${request.method} - ${new URL(request.url).pathname} failed`);
        }
        if (this.development) {
          return new Response(`<!doctype html><html><head><title>Internal Server Error</title></head><body><h1>${String(error)}</h1></body></html>`, {
            status: 500,
            headers: { "content-type": "text/html;charset=utf-8" },
          });
        }
        return new Response("Internal Server Error", { status: 500, headers: { "content-type": "text/plain;charset=utf-8" } });
      }
    }

    async stop(force) {
      if (!this.stopped) {
        this.stopped = true;
        host.netClose(this.listener);
        for (const conn of this.connections) if (force || conn.idle) conn.close();
      }
      if (this.pendingRequests > 0 && !force) await new Promise(resolve => this.drained.push(resolve));
      return undefined;
    }

    reload(options) {
      if (options.fetch !== undefined) this.fetchHandler = options.fetch;
      if (options.error !== undefined) this.errorHandler = options.error;
      if (options.routes !== undefined) this.routes = options.routes;
      return this;
    }

    ref() {
      host.netRef(this.listener, true);
    }

    unref() {
      host.netRef(this.listener, false);
    }

    requestIP(request) {
      return request.__ip === undefined ? null : request.__ip;
    }

    timeout() {}

    async fetch(input, init) {
      const request = input instanceof Request ? input : new Request(input, init);
      return await this.handle(request);
    }

    requestDone() {
      this.pendingRequests--;
      if (this.pendingRequests === 0) for (const resolve of this.drained.splice(0)) resolve();
    }
  }

  function buildRequest(server, parsed, body) {
    const [method, target] = parsed.start.split(" ");
    const hostHeader = headerOf(parsed.headers, "host");
    const url = `http://${hostHeader === undefined ? `${server.hostname}:${server.port}` : hostHeader}${target}`;
    const init = { method, headers: parsed.headers };
    if (body !== null && method !== "GET" && method !== "HEAD") init.body = body;
    return new Request(url, init);
  }

  async function writeResponse(conn, request, response, keepAlive) {
    const wire = host.responseWire(response);
    const status = response.status;
    const headers = [];
    for (const [name, value] of response.headers) headers.push([name, value]);
    let bytes = null;
    let stream = null;
    let type = "";
    if (wire !== undefined) {
      if (wire[0] instanceof Uint8Array) bytes = wire[0];
      else stream = wire[0];
      type = wire[1];
    }
    const has = name => headers.some(([key]) => key === name);
    const noBody = status === 204 || status === 304 || (status >= 100 && status < 200);
    if (!has("content-type") && request.method !== "HEAD" && !noBody && type !== "") headers.push(["content-type", type]);
    // Corpo de stream (medido no bun 1.4.2): espera o primeiro pedaço (ou o fim) pelo tempo que for, deixa as
    // microtasks assentarem e, se o stream já fechou com menos de 256 no total (bytes de pedaço binário, unidades de
    // pedaço string), manda `Content-Length`; senão `Transfer-Encoding: chunked`.
    let chunks = null;
    let pump = null;
    let finished = true;
    let wake = null;
    const notify = () => {
      if (wake !== null) {
        const resolve = wake;
        wake = null;
        resolve();
      }
    };
    const waitMore = () => new Promise(resolve => {
      wake = resolve;
    });
    if (stream !== null) {
      const reader = stream.getReader();
      chunks = [];
      finished = false;
      let size = 0;
      pump = (async () => {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) {
            finished = true;
            notify();
            return;
          }
          size += typeof value === "string" ? value.length : value.byteLength;
          chunks.push(bytesOf(value));
          notify();
        }
      })();
      pump.catch(notify);
      while (chunks.length === 0 && !finished) await Promise.race([waitMore(), pump]);
      for (let turn = 0; turn < 1000 && !finished; turn++) await null;
      if (finished && size < 256) {
        await pump;
        bytes = concatAll(chunks);
        chunks = null;
      }
    }
    const chunked = chunks !== null;
    for (let index = headers.length - 1; index >= 0; index--) {
      const name = headers[index][0];
      if (name === (chunked ? "content-length" : "transfer-encoding")) headers.splice(index, 1);
    }
    if (!noBody && !has("content-length") && !has("transfer-encoding")) {
      if (chunked) headers.push(["transfer-encoding", "chunked"]);
      else headers.push(["content-length", String(bytes === null ? 0 : bytes.length)]);
    }
    if (!has("date")) headers.push(["date", new Date().toUTCString()]);
    if (!keepAlive) headers.push(["connection", "close"]);
    let head = `HTTP/1.1 ${status} ${REASONS[status] === undefined ? "Unknown" : REASONS[status]}\r\n`;
    for (const [name, value] of headers) head += `${name}: ${value}\r\n`;
    head += "\r\n";
    const sendBody = !noBody && request.method !== "HEAD";
    await conn.send(encoder.encode(head));
    if (!sendBody) {
      if (pump !== null) pump.catch(() => {});
      return;
    }
    if (!chunked) {
      if (bytes !== null && bytes.length > 0) await conn.send(bytes);
      return;
    }
    let sent = 0;
    for (;;) {
      while (sent < chunks.length) {
        const chunk = chunks[sent++];
        if (chunk.length > 0) await conn.send(concatAll([encoder.encode(`${chunk.length.toString(16)}\r\n`), chunk, encoder.encode("\r\n")]));
      }
      if (finished) break;
      await Promise.race([waitMore(), pump]);
    }
    await pump;
    await conn.send(encoder.encode("0\r\n\r\n"));
  }

  async function serveConnection(server, conn) {
    server.connections.add(conn);
    conn.idle = true;
    try {
      for (;;) {
        const head = await conn.readHead();
        if (head === null) break;
        conn.idle = false;
        server.pendingRequests++;
        try {
          const parsed = parseHead(head);
          const next = bodyReader(conn, parsed.headers, false);
          const body = next === null ? null : await collect(next);
          const request = buildRequest(server, parsed, body);
          const connection = headerOf(parsed.headers, "connection");
          const keepAlive = !(connection !== undefined && /close/i.test(connection)) && !server.stopped;
          const response = await server.handle(request);
          await writeResponse(conn, request, response, keepAlive);
          if (!keepAlive) break;
        } finally {
          server.requestDone();
        }
        conn.idle = true;
        if (server.stopped) break;
      }
    } catch (error) {
      // A conexão caiu no meio de um pedido: nada a responder.
    } finally {
      conn.close();
      server.connections.delete(conn);
    }
  }

  function portOf(options) {
    const raw = options.port !== undefined ? options.port : 3000;
    const port = Number(raw);
    if (!Number.isInteger(port) || port < 0 || port > 65535) {
      host.throwNodeError("TypeError", "ERR_INVALID_ARG_VALUE", `The property 'options.port' is invalid. Received ${String(raw)}`);
    }
    return port;
  }

  function serve(options) {
    if (options === null || typeof options !== "object") {
      host.throwNodeError("TypeError", "ERR_INVALID_ARG_TYPE", "Bun.serve expects an object");
    }
    if (typeof options.fetch !== "function" && (options.routes === undefined || options.routes === null)) {
      throw new TypeError("Bun.serve() needs either a 'routes' object or a 'fetch' handler");
    }
    const result = host.netListen(portOf(options), 512);
    if (typeof result === "number") {
      const error = new Error(`Failed to start server. Is port ${portOf(options)} in use?`);
      error.code = result === -98 ? "EADDRINUSE" : "ERR_SERVER_FAILED";
      throw error;
    }
    const [listener, port] = result;
    const server = new Server(options, listener, port);
    host.netWatch(listener, true, false, () => {
      for (;;) {
        const accepted = host.netAccept(listener);
        if (typeof accepted === "number") break;
        serveConnection(server, new Conn(accepted[0]));
      }
    });
    return server;
  }

  // ---- cliente ----

  const REDIRECTS = new Set([301, 302, 303, 307, 308]);

  function isLoopbackHost(hostname) {
    return hostname === "localhost" || /^127(\.\d{1,3}){3}$/.test(hostname);
  }

  function bodyStream(conn, next, signal) {
    let closed = false;
    const finish = () => {
      closed = true;
      conn.close();
    };
    if (signal !== undefined) {
      signal.addEventListener("abort", finish, { once: true });
    }
    return new ReadableStream({
      async pull(controller) {
        try {
          const chunk = closed ? null : await next();
          if (chunk === null) {
            finish();
            controller.close();
            return;
          }
          controller.enqueue(new Uint8Array(chunk));
        } catch (error) {
          finish();
          if (signal !== undefined && signal.aborted) controller.error(signal.reason);
          else controller.error(error);
        }
      },
      cancel() {
        finish();
      },
    });
  }

  function methodHasBody(method) {
    return method === "POST" || method === "PUT" || method === "PATCH";
  }

  async function exchange(url, method, requestHeaders, body, stream, signal) {
    const connected = host.netConnect(Number(url.port === "" ? 80 : url.port));
    if (typeof connected === "number") host.refused(url.href);
    const conn = new Conn(connected[0]);
    const onAbort = () => conn.close();
    if (signal !== undefined) signal.addEventListener("abort", onAbort, { once: true });
    try {
      const headers = [
        ["Host", url.host],
        ["User-Agent", `Bun/${Bun.version}`],
        ["Accept", "*/*"],
        ["Connection", "keep-alive"],
        ["Accept-Encoding", "gzip, deflate, br, zstd"],
      ];
      for (const [name, value] of requestHeaders) {
        const lower = name.toLowerCase();
        const existing = headers.findIndex(([key]) => key.toLowerCase() === lower);
        if (existing >= 0) headers[existing] = [name, value];
        else headers.push([name, value]);
      }
      if (stream !== null) headers.push(["Transfer-Encoding", "chunked"]);
      else if (body !== null) headers.push(["Content-Length", String(body.length)]);
      else if (methodHasBody(method)) headers.push(["Content-Length", "0"]);
      let head = `${method} ${url.pathname}${url.search} HTTP/1.1\r\n`;
      for (const [name, value] of headers) head += `${name}: ${value}\r\n`;
      await conn.send(encoder.encode(`${head}\r\n`));
      if (stream !== null) {
        const reader = stream.getReader();
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          const chunk = bytesOf(value);
          if (chunk.length > 0) await conn.send(concatAll([encoder.encode(`${chunk.length.toString(16)}\r\n`), chunk, encoder.encode("\r\n")]));
        }
        await conn.send(encoder.encode("0\r\n\r\n"));
      } else if (body !== null && body.length > 0) {
        await conn.send(body);
      }
      const responseHead = await conn.readHead();
      if (responseHead === null) throw new Error("The socket connection was closed unexpectedly. For more information, pass `verbose: true` in the second argument to fetch()");
      const parsed = parseHead(responseHead);
      const [, code, ...reason] = parsed.start.split(" ");
      return { conn, status: Number(code), statusText: reason.join(" "), headers: parsed.headers };
    } catch (error) {
      conn.close();
      if (signal !== undefined && signal.aborted) throw signal.reason;
      throw error;
    } finally {
      if (signal !== undefined) signal.removeEventListener("abort", onAbort);
    }
  }

  async function fetchHttp(input, init) {
    const request = new Request(input, init);
    const signal = request.signal;
    if (signal.aborted) throw signal.reason;
    const redirect = request.redirect;
    let method = request.method;
    let url = new URL(request.url);
    const requestHeaders = [...request.headers];
    const rawStream = init !== undefined && init !== null && init.body instanceof ReadableStream ? request.body : null;
    let body = null;
    if (rawStream === null && request.body !== null) body = new Uint8Array(await request.arrayBuffer());
    let redirected = false;
    for (let hops = 0; ; hops++) {
      const result = await exchange(url, method, requestHeaders, body, hops === 0 ? rawStream : null, signal);
      if (REDIRECTS.has(result.status) && headerOf(result.headers, "location") !== undefined && redirect !== "manual") {
        result.conn.close();
        if (redirect === "error") {
          const error = new TypeError(`UnexpectedRedirect fetching "${url.href}". For more information, pass \`verbose: true\` in the second argument to fetch()`);
          error.code = "UnexpectedRedirect";
          throw error;
        }
        if (hops >= 20) {
          const error = new TypeError(`TooManyRedirects fetching "${url.href}". For more information, pass \`verbose: true\` in the second argument to fetch()`);
          error.code = "TooManyRedirects";
          throw error;
        }
        url = new URL(headerOf(result.headers, "location"), url);
        if (result.status === 303 || ((result.status === 301 || result.status === 302) && method === "POST")) {
          if (method !== "HEAD") method = "GET";
          body = null;
        }
        redirected = true;
        continue;
      }
      const empty = method === "HEAD" || result.status === 204 || result.status === 304 || (result.status >= 100 && result.status < 200);
      const next = empty ? null : bodyReader(result.conn, result.headers, true);
      const headers = new Headers();
      for (const [name, value] of result.headers) headers.append(name, value);
      let payload = null;
      if (next === null) result.conn.close();
      else payload = bodyStream(result.conn, next, signal);
      const response = new Response(payload, { status: result.status, statusText: result.statusText, headers });
      host.markFetched(response, url.href, redirected);
      return response;
    }
  }

  return { serve, fetch: fetchHttp, isLoopbackHost };
});
