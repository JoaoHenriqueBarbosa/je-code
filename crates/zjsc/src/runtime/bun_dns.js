// `Bun.dns` do bun 1.4.2 numa máquina sem rede: o que o `/etc/hosts` e os literais de IP respondem, o resto falha como o
// resolvedor que não alcança servidor nenhum (`ECONNREFUSED`, medido no oráculo sem rede). Programa escondido: a fábrica recebe
// `host` (`requireBuiltin`, `internal`).
(function (host) {
  "use strict";
  const fs = host.requireBuiltin("node:fs");
  const net = host.requireBuiltin("node:net");
  const { makeError, codes } = host.internal;

  const ADDRCONFIG = 32;
  const ALL = 16;
  const V4MAPPED = 8;

  function DNSException(code, syscall, hostname) {
    const message = hostname === undefined ? `${syscall} ${code}` : `${syscall} ${code} ${hostname}`;
    const error = new Error(message);
    Object.defineProperty(error, "name", { value: "DNSException", writable: true, configurable: true, enumerable: false });
    error.code = `DNS_${code}`;
    error.errno = ERRNO[code];
    error.syscall = syscall;
    if (hostname !== undefined) error.hostname = hostname;
    return error;
  }
  const ERRNO = { ECONNREFUSED: 11, ENOTIMP: 5 };

  const requireString = (value, name, label) => {
    if (typeof value !== "string") throw makeError("TypeError", "ERR_INVALID_ARG_TYPE", `Expected ${name} to be a string for '${label}'.`);
    if (value.length === 0) throw makeError("TypeError", "ERR_INVALID_ARG_TYPE", `Expected ${name} to be a non-empty string for '${label}'.`);
  };

  function hostsFile() {
    try {
      return fs.readFileSync("/etc/hosts", "utf8");
    } catch {
      return "";
    }
  }
  // Os nomes do `/etc/hosts` que apontam para loopback: o resolvedor do bun responde a eles com os dois endereços.
  function isLoopbackName(name) {
    const lower = name.toLowerCase();
    for (const line of hostsFile().split("\n")) {
      const fields = line.replace(/#.*/, "").trim().split(/\s+/);
      if (fields.length < 2) continue;
      if ((fields[0] === "127.0.0.1" || fields[0] === "::1") && fields.slice(1).some(alias => alias.toLowerCase() === lower)) return true;
    }
    return lower === "localhost";
  }

  const familyOf = value => {
    if (value === undefined || value === 0 || value === "0") return 0;
    if (value === 4 || value === "IPv4") return 4;
    if (value === 6 || value === "IPv6") return 6;
    throw new Error("Invalid options passed to lookup(): InvalidFamily");
  };

  async function lookup(hostname, options) {
    requireString(hostname, "hostname", "lookup");
    let family = 0;
    if (typeof options === "number") family = familyOf(options);
    else if (options !== undefined && options !== null && typeof options === "object") family = familyOf(options.family);
    const kind = net.isIP(hostname);
    if (kind === 4 && hostname.split(".").length === 4) return family === 6 ? [] : [{ address: hostname, family: 4, ttl: 0 }];
    if (kind === 6) return family === 4 ? [] : [{ address: hostname, family: 6, ttl: 0 }];
    if (isLoopbackName(hostname)) {
      const v6 = { address: "::1", family: 6, ttl: 0 };
      const v4 = { address: "127.0.0.1", family: 4, ttl: 0 };
      // Medido: com `family: 4` o resolvedor do bun repete o endereço IPv4.
      return family === 4 ? [v4, { ...v4 }] : family === 6 ? [v6] : [v6, v4];
    }
    throw DNSException("ECONNREFUSED", "getaddrinfo");
  }

  const recordTypes = ["A", "AAAA", "ANY", "CAA", "CNAME", "MX", "NAPTR", "NS", "PTR", "SOA", "SRV", "TXT"];
  const queryName = { A: "queryA", AAAA: "queryAaaa", ANY: "queryAny", CAA: "queryCaa", CNAME: "queryCname", MX: "queryMx", NAPTR: "queryNaptr", NS: "queryNs", PTR: "queryPtr", SOA: "querySoa", SRV: "querySrv", TXT: "queryTxt" };

  async function resolve(hostname, record) {
    if (arguments.length === 0) throw makeError("TypeError", "ERR_MISSING_ARGS", "Not enough arguments to 'resolve'. Expected 3, got 0.");
    requireString(hostname, "hostname", "resolve");
    let type = "A";
    if (record !== undefined && record !== null) {
      type = String(record);
      if (!recordTypes.includes(type)) {
        throw makeError("TypeError", "ERR_INVALID_ARG_VALUE", `The property "record" is invalid. Expected one of: ${recordTypes.join(", ")}, received type string ('${type}')`);
      }
    }
    throw DNSException("ECONNREFUSED", queryName[type], hostname);
  }
  const resolver = (label, type) => {
    const fn = {
      [label]: async function (hostname) {
        requireString(hostname, "hostname", label);
        throw DNSException("ECONNREFUSED", queryName[type], hostname);
      },
    }[label];
    Object.defineProperty(fn, "length", { value: 2 });
    return fn;
  };

  const servers = () => {
    try {
      return fs
        .readFileSync("/etc/resolv.conf", "utf8")
        .split("\n")
        .map(line => /^\s*nameserver\s+(\S+)/.exec(line))
        .filter(Boolean)
        .map(match => match[1]);
    } catch {
      return [];
    }
  };

  let cacheMisses = 0;
  const dns = {
    lookup,
    resolve,
    resolveSrv: resolver("resolveSrv", "SRV"),
    resolveTxt: resolver("resolveTxt", "TXT"),
    resolveSoa: resolver("resolveSoa", "SOA"),
    resolveNaptr: resolver("resolveNaptr", "NAPTR"),
    resolveMx: resolver("resolveMx", "MX"),
    resolveCaa: resolver("resolveCaa", "CAA"),
    resolveNs: resolver("resolveNs", "NS"),
    resolvePtr: resolver("resolvePtr", "PTR"),
    resolveCname: resolver("resolveCname", "CNAME"),
    resolveAny: resolver("resolveAny", "ANY"),
    getServers() {
      return servers();
    },
    setServers() {
      throw makeError("TypeError", "ERR_INVALID_ARG_TYPE", "Invalid address family");
    },
    async reverse(address) {
      requireString(address, "address", "reverse");
      if (address === "127.0.0.1") return ["ip6-localhost", "ip6-loopback"];
      throw DNSException("ENOTIMP", "getHostByAddr", address);
    },
    async lookupService(address, port) {
      if (net.isIP(String(address)) === 0) throw makeError("TypeError", "ERR_INVALID_ARG_VALUE", `The "address" argument is invalid. Received type string ('${address}')`);
      if (address === "127.0.0.1" && port === 80) return ["localhost", "http"];
      throw DNSException("ECONNREFUSED", "getnameinfo");
    },
    prefetch(hostname) {
      cacheMisses++;
    },
    getCacheStats() {
      return { cacheHitsCompleted: 0, cacheHitsInflight: 0, cacheMisses, size: cacheMisses, errors: 0, totalCount: cacheMisses };
    },
    ADDRCONFIG,
    ALL,
    V4MAPPED,
  };
  // Medido: os métodos dos pares nome-função-comprimento são todos de `length` 2 (`getCacheStats` 0).
  for (const name of Object.keys(dns)) {
    if (typeof dns[name] === "function" && name !== "getCacheStats") Object.defineProperty(dns[name], "length", { value: 2 });
  }
  return dns;
})
