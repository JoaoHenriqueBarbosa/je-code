// GERADO por `scripts/gen-node-url.js` a partir do `url.ts` e do `internal/url.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";

  const { codes } = host.internal;
  // `$isObject`: objeto ou função.
  const isObject = value => (typeof value === "object" && value !== null) || typeof value === "function";
  // `$putByIdDirect`: propriedade própria comum, sem passar por setter.
  const putByIdDirect = (object, key, value) =>
    Object.defineProperty(object, key, { value, writable: true, enumerable: true, configurable: true });
  // `$toLength` do JSC (ToLength da especificação).
  const toLength = value => {
    const length = Math.trunc(Number(value)) || 0;
    return length <= 0 ? 0 : Math.min(length, Number.MAX_SAFE_INTEGER);
  };
  const internalUrl = (() => {
    function urlToHttpOptions(url) {
      const options = {
        ...url,
        protocol: url.protocol,
        hostname: typeof url.hostname === "string" && url.hostname.startsWith("[") ? url.hostname.slice(1, -1) : url.hostname,
        hash: url.hash,
        search: url.search,
        pathname: url.pathname,
        path: `${url.pathname || ""}${url.search || ""}`,
        href: url.href
      };
      const port = url.port;
      if (port !== "") {
        options.port = Number(port);
      }
      const username = url.username;
      let password;
      if (username || (password = url.password)) {
        options.auth = `${decodeURIComponent(username)}:${decodeURIComponent(password ?? url.password)}`;
      }
      return options;
    }
    return {
      urlToHttpOptions
    };

  })();
  const { URL, URLSearchParams, URLPattern } = globalThis;
  const [domainToASCII, domainToUnicode, idnaToASCII] = [host.domainToAscii, host.domainToUnicode, host.idnaToASCII];
  const { urlToHttpOptions } = internalUrl;
  const { validateString, validateObject } = host.internal.validators;
  const ObjectSetPrototypeOf = Object.setPrototypeOf;
  function Url() {
    this.protocol = null;
    this.slashes = null;
    this.auth = null;
    this.host = null;
    this.port = null;
    this.hostname = null;
    this.hash = null;
    this.search = null;
    this.query = null;
    this.pathname = null;
    this.path = null;
    this.href = null;
  }
  Url.prototype = {};
  var protocolPattern = /^([a-z0-9.+-]+:)/i, portPattern = /:[0-9]*$/, simplePathPattern = /^(\/\/?(?!\/)[^?\s]*)(\?[^\s]*)?$/, delims = ["<", ">", '"', "`", " ", "\r", `
  `, "\t"], unwise = ["{", "}", "|", "\\", "^", "`"].concat(delims), autoEscape = ["'"].concat(unwise), forbiddenHostChars = /[\0\t\n\r #%/:<>?@[\\\]^|]/, forbiddenHostCharsIpv6 = /[\0\t\n\r #%/<>?@\\^|]/, hostnameMaxLen = 255, unsafeProtocol = {
    __proto__: null,
    javascript: true,
    "javascript:": true
  }, hostlessProtocol = {
    __proto__: null,
    javascript: true,
    "javascript:": true
  }, slashedProtocol = {
    __proto__: null,
    http: true,
    https: true,
    ftp: true,
    gopher: true,
    file: true,
    ws: true,
    wss: true,
    "http:": true,
    "https:": true,
    "ftp:": true,
    "gopher:": true,
    "file:": true,
    "ws:": true,
    "wss:": true
  };
  const { isInsideNodeModules } = host.internal;
  let urlParseWarned = false;
  function urlParse(url, parseQueryString, slashesDenoteHost) {
    if (!urlParseWarned && !isInsideNodeModules(4)) {
      urlParseWarned = true;
      process.emitWarning("`url.parse()` behavior is not standardized and prone to errors that have security implications. Use the WHATWG URL API instead. CVEs are not issued for `url.parse()` vulnerabilities.", "DeprecationWarning", "DEP0169");
    }
    if (isObject(url) && url instanceof Url)
      return url;
    var u = new Url;
    try {
      u.parse(url, parseQueryString, slashesDenoteHost);
    } catch (e) {
      putByIdDirect(e, "input", url);
      throw e;
    }
    return u;
  }
  Url.prototype.parse = function parse(url, parseQueryString, slashesDenoteHost) {
    validateString(url, "url");
    let hasHash = false;
    let hasAt = false;
    let start = -1;
    let end = -1;
    let rest = "";
    let lastPos = 0;
    for (let i = 0, inWs = false, split = false;i < url.length; ++i) {
      const code = url.charCodeAt(i);
      const isWs = code < 33 || code === 160 /* NO_BREAK_SPACE */ || code === 65279 /* ZERO_WIDTH_NOBREAK_SPACE */;
      if (start === -1) {
        if (isWs)
          continue;
        lastPos = start = i;
      } else if (inWs) {
        if (!isWs) {
          end = -1;
          inWs = false;
        }
      } else if (isWs) {
        end = i;
        inWs = true;
      }
      if (!split) {
        switch (code) {
          case 64 /* AT */:
            hasAt = true;
            break;
          case 35 /* HASH */:
            hasHash = true;
          case 63 /* QUESTION_MARK */:
            split = true;
            break;
          case 92 /* BACKWARD_SLASH */:
            if (i - lastPos > 0)
              rest += url.slice(lastPos, i);
            rest += "/";
            lastPos = i + 1;
            break;
        }
      } else if (!hasHash && code === 35 /* HASH */) {
        hasHash = true;
      }
    }
    if (start !== -1) {
      if (lastPos === start) {
        if (end === -1) {
          if (start === 0)
            rest = url;
          else
            rest = url.slice(start);
        } else {
          rest = url.slice(start, end);
        }
      } else if (end === -1 && lastPos < url.length) {
        rest += url.slice(lastPos);
      } else if (end !== -1 && lastPos < end) {
        rest += url.slice(lastPos, end);
      }
    }
    if (!slashesDenoteHost && !hasHash && !hasAt) {
      const simplePath = simplePathPattern.exec(rest);
      if (simplePath) {
        this.path = rest;
        this.href = rest;
        this.pathname = simplePath[1];
        if (simplePath[2]) {
          this.search = simplePath[2];
          if (parseQueryString) {
            this.query = ObjectSetPrototypeOf(new URLSearchParams(this.search.slice(1)).toJSON(), null);
          } else {
            this.query = this.search.slice(1);
          }
        } else if (parseQueryString) {
          this.search = null;
          this.query = Object.create(null);
        }
        return this;
      }
    }
    var proto = protocolPattern.exec(rest);
    if (proto) {
      proto = proto[0];
      var lowerProto = proto.toLowerCase();
      this.protocol = lowerProto;
      rest = rest.substring(proto.length);
    }
    let slashes;
    if (slashesDenoteHost || proto || rest.match(/^\/\/[^@/]+@[^@/]+/)) {
      slashes = rest.substring(0, 2) === "//";
      if (slashes && !(proto && hostlessProtocol[lowerProto])) {
        rest = rest.substring(2);
        this.slashes = true;
      }
    }
    if (!hostlessProtocol[lowerProto] && (slashes || lowerProto && !slashedProtocol[lowerProto])) {
      let hostEnd = -1;
      let atSign = -1;
      let nonHost = -1;
      for (let i = 0;i < rest.length; ++i) {
        switch (rest.charCodeAt(i)) {
          case 9 /* TAB */:
          case 10 /* LINE_FEED */:
          case 13 /* CARRIAGE_RETURN */:
            rest = rest.slice(0, i) + rest.slice(i + 1);
            i -= 1;
            break;
          case 32 /* SPACE */:
          case 34 /* DOUBLE_QUOTE */:
          case 37 /* PERCENT */:
          case 39 /* SINGLE_QUOTE */:
          case 59 /* SEMICOLON */:
          case 60 /* LEFT_ANGLE_BRACKET */:
          case 62 /* RIGHT_ANGLE_BRACKET */:
          case 92 /* BACKWARD_SLASH */:
          case 94 /* CIRCUMFLEX_ACCENT */:
          case 96 /* GRAVE_ACCENT */:
          case 123 /* LEFT_CURLY_BRACKET */:
          case 124 /* VERTICAL_LINE */:
          case 125 /* RIGHT_CURLY_BRACKET */:
            if (nonHost === -1)
              nonHost = i;
            break;
          case 35 /* HASH */:
          case 47 /* FORWARD_SLASH */:
          case 63 /* QUESTION_MARK */:
            if (nonHost === -1)
              nonHost = i;
            hostEnd = i;
            break;
          case 64 /* AT */:
            atSign = i;
            nonHost = -1;
            break;
        }
        if (hostEnd !== -1)
          break;
      }
      let authStart = 0;
      if (atSign !== -1) {
        this.auth = decodeURIComponent(rest.slice(0, atSign));
        authStart = atSign + 1;
      }
      if (nonHost === -1) {
        this.host = rest.slice(authStart);
        rest = "";
      } else {
        this.host = rest.slice(authStart, nonHost);
        rest = rest.slice(nonHost);
      }
      this.parseHost();
      if (typeof this.hostname !== "string") {
        this.hostname = "";
      }
      const hostname = this.hostname;
      var ipv6Hostname = isIpv6Hostname(this.hostname);
      if (!ipv6Hostname) {
        rest = getHostname(this, rest, hostname, url);
      }
      if (this.hostname.length > hostnameMaxLen) {
        this.hostname = "";
      } else {
        this.hostname = this.hostname.toLowerCase();
      }
      if (this.hostname !== "") {
        if (ipv6Hostname) {
          if (forbiddenHostCharsIpv6.test(this.hostname)) {
            throw codes.INVALID_URL(url);
          }
        } else {
          this.hostname = idnaToASCII(this.hostname);
          if (this.hostname === "" || forbiddenHostChars.test(this.hostname)) {
            throw codes.INVALID_URL(url);
          }
        }
      }
      var p = this.port ? ":" + this.port : "";
      var h = this.hostname || "";
      this.host = h + p;
      this.href += this.host;
      if (ipv6Hostname) {
        this.hostname = this.hostname.slice(1, -1);
        if (rest[0] !== "/") {
          rest = "/" + rest;
        }
      }
    }
    if (!unsafeProtocol[lowerProto]) {
      for (var i = 0, l = autoEscape.length;i < l; i++) {
        var ae = autoEscape[i];
        if (rest.indexOf(ae) === -1) {
          continue;
        }
        var esc = encodeURIComponent(ae);
        if (esc === ae) {
          esc = escape(ae);
        }
        rest = rest.split(ae).join(esc);
      }
    }
    var hash = rest.indexOf("#");
    if (hash !== -1) {
      this.hash = rest.substring(hash);
      rest = rest.slice(0, hash);
    }
    var qm = rest.indexOf("?");
    if (qm !== -1) {
      this.search = rest.substring(qm);
      this.query = rest.substring(qm + 1);
      if (parseQueryString) {
        const query = this.query;
        this.query = ObjectSetPrototypeOf(new URLSearchParams(query).toJSON(), null);
      }
      rest = rest.slice(0, qm);
    } else if (parseQueryString) {
      this.search = null;
      this.query = Object.create(null);
    }
    if (rest) {
      this.pathname = rest;
    }
    if (slashedProtocol[lowerProto] && this.hostname && !this.pathname) {
      this.pathname = "/";
    }
    const pathname = this.pathname;
    let search;
    if (pathname || (search = this.search)) {
      this.path = (pathname || "") + ((search ?? this.search) || "");
    }
    this.href = this.format();
    return this;
  };
  const noEscapeAuth = new Int8Array([
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    1,
    0,
    0,
    0,
    0,
    0,
    1,
    1,
    1,
    1,
    0,
    0,
    1,
    1,
    0,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    0,
    0,
    0,
    0,
    0,
    0,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    0,
    0,
    0,
    0,
    1,
    0,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    1,
    0,
    0,
    0,
    1,
    0
  ]);
  let hexTable;
  function getHexTable() {
    if (hexTable === undefined) {
      hexTable = new Array(256);
      for (let i = 0;i < 256; ++i)
        hexTable[i] = "%" + ((i < 16 ? "0" : "") + i.toString(16)).toUpperCase();
    }
    return hexTable;
  }
  function encodeStr(str, noEscapeTable, hexTable) {
    const len = str.length;
    if (len === 0)
      return "";
    let out = "";
    let lastPos = 0;
    let i = 0;
    outer:
      for (;i < len; i++) {
        let c = str.charCodeAt(i);
        while (c < 128) {
          if (noEscapeTable[c] !== 1) {
            if (lastPos < i)
              out += str.slice(lastPos, i);
            lastPos = i + 1;
            out += hexTable[c];
          }
          if (++i === len)
            break outer;
          c = str.charCodeAt(i);
        }
        if (lastPos < i)
          out += str.slice(lastPos, i);
        if (c < 2048) {
          lastPos = i + 1;
          out += hexTable[192 | c >> 6] + hexTable[128 | c & 63];
          continue;
        }
        if (c < 55296 || c >= 57344) {
          lastPos = i + 1;
          out += hexTable[224 | c >> 12] + hexTable[128 | c >> 6 & 63] + hexTable[128 | c & 63];
          continue;
        }
        ++i;
        if (i >= len)
          throw codes.INVALID_URI();
        const c2 = str.charCodeAt(i) & 1023;
        lastPos = i + 1;
        c = 65536 + ((c & 1023) << 10 | c2);
        out += hexTable[240 | c >> 18] + hexTable[128 | c >> 12 & 63] + hexTable[128 | c >> 6 & 63] + hexTable[128 | c & 63];
      }
    if (lastPos === 0)
      return str;
    if (lastPos < len)
      return out + str.slice(lastPos);
    return out;
  }
  function isIpv6Hostname(hostname) {
    return hostname.charCodeAt(0) === 91 /* LEFT_SQUARE_BRACKET */ && hostname.charCodeAt(toLength(hostname.length - 1)) === 93 /* RIGHT_SQUARE_BRACKET */;
  }
  let warnInvalidPort = true;
  function getHostname(self, rest, hostname, url) {
    for (let i = 0;i < hostname.length; ++i) {
      const code = hostname.charCodeAt(i);
      const isValid = code !== 47 /* FORWARD_SLASH */ && code !== 92 /* BACKWARD_SLASH */ && code !== 35 /* HASH */ && code !== 63 /* QUESTION_MARK */ && code !== 58 /* COLON */;
      if (!isValid) {
        if (warnInvalidPort && code === 58 /* COLON */) {
          const detail = `The URL ${url} is invalid. Future versions of Node.js will throw an error.`;
          process.emitWarning(detail, "DeprecationWarning", "DEP0170");
          warnInvalidPort = false;
        }
        self.hostname = hostname.slice(0, i);
        return `/${hostname.slice(i)}${rest}`;
      }
    }
    return rest;
  }
  function urlFormat(urlObject, options) {
    if (typeof urlObject === "string") {
      urlObject = urlParse(urlObject);
    } else if (typeof urlObject !== "object" || urlObject === null) {
      throw codes.INVALID_ARG_TYPE("urlObject", ["Object", "string"], urlObject);
    } else if (urlObject instanceof URL) {
      let fragment = true;
      let unicode = false;
      let search = true;
      let auth = true;
      if (options) {
        validateObject(options, "options");
        if (options.fragment != null) {
          fragment = Boolean(options.fragment);
        }
        if (options.unicode != null) {
          unicode = Boolean(options.unicode);
        }
        if (options.search != null) {
          search = Boolean(options.search);
        }
        if (options.auth != null) {
          auth = Boolean(options.auth);
        }
      }
      return formatWhatwgURL(urlObject, fragment, unicode, search, auth);
    }
    if (!(urlObject instanceof Url)) {
      return Url.prototype.format.call(urlObject);
    }
    return urlObject.format();
  }
  function formatWhatwgURL(url, fragment, unicode, search, auth) {
    const href = url.href;
    if (fragment && !unicode && search && auth)
      return href;
    let ret = url.protocol;
    if (href.startsWith(ret + "//")) {
      ret += "//";
      const hasUsername = url.username !== "";
      const hasPassword = url.password !== "";
      if (auth && (hasUsername || hasPassword)) {
        if (hasUsername)
          ret += url.username;
        if (hasPassword)
          ret += ":" + url.password;
        ret += "@";
      }
      ret += unicode ? domainToUnicode(url.hostname) || url.hostname : url.hostname;
      const { port } = url;
      if (port !== "")
        ret += ":" + port;
    } else if (url.pathname.startsWith("//")) {
      ret += "/.";
    }
    ret += url.pathname;
    const hashAt = href.indexOf("#");
    const queryEnd = hashAt === -1 ? href.length : hashAt;
    if (search)
      ret += url.search || (href[queryEnd - 1] === "?" ? "?" : "");
    if (fragment)
      ret += url.hash || (hashAt !== -1 ? "#" : "");
    return ret;
  }
  Url.prototype.format = function format() {
    var auth = this.auth || "";
    if (auth) {
      auth = encodeStr(auth, noEscapeAuth, getHexTable());
      auth += "@";
    }
    var protocol = this.protocol || "", pathname = this.pathname || "", hash = this.hash || "", host = "", query = "";
    if (protocol && protocol.substr(-1) !== ":") {
      protocol += ":";
    }
    const thisHost = this.host;
    let thisHostname;
    if (thisHost) {
      host = auth + thisHost;
    } else if (thisHostname = this.hostname) {
      host = auth + (thisHostname.includes(":") && !isIpv6Hostname(thisHostname) ? "[" + thisHostname + "]" : thisHostname);
      const thisPort = this.port;
      if (thisPort) {
        host += ":" + thisPort;
      }
    }
    const thisQuery = this.query;
    if (thisQuery && typeof thisQuery === "object" && Object.keys(thisQuery).length) {
      query = new URLSearchParams(thisQuery).toString();
    }
    var search = this.search || query && "?" + query || "";
    const { slashes } = this;
    if (slashes || slashedProtocol[protocol]) {
      if (slashes || host) {
        if (pathname && pathname.charAt(0) !== "/") {
          pathname = "/" + pathname;
        }
        host = "//" + host;
      } else if (protocol.startsWith("file")) {
        host = "//";
      }
    }
    if (hash && hash.charAt(0) !== "#") {
      hash = "#" + hash;
    }
    if (search && search.charAt(0) !== "?") {
      search = "?" + search;
    }
    pathname = pathname.replace(/[?#]/g, function(match) {
      return encodeURIComponent(match);
    });
    search = search.replace(/#/g, "%23");
    return protocol + host + pathname + search + hash;
  };
  function urlResolve(source, relative) {
    return urlParse(source, false, true).resolve(relative);
  }
  Url.prototype.resolve = function resolve(relative) {
    return this.resolveObject(urlParse(relative, false, true)).format();
  };
  function urlResolveObject(source, relative) {
    if (!source) {
      return relative;
    }
    return urlParse(source, false, true).resolveObject(relative);
  }
  Url.prototype.resolveObject = function resolveObject(relative) {
    if (typeof relative === "string") {
      var rel = new Url;
      rel.parse(relative, false, true);
      relative = rel;
    }
    var result = new Url;
    var tkeys = Object.keys(this);
    for (var tk = 0;tk < tkeys.length; tk++) {
      var tkey = tkeys[tk];
      result[tkey] = this[tkey];
    }
    result.hash = relative.hash;
    if (relative.href === "") {
      result.href = result.format();
      return result;
    }
    if (relative.slashes && !relative.protocol) {
      var rkeys = Object.keys(relative);
      for (var rk = 0;rk < rkeys.length; rk++) {
        var rkey = rkeys[rk];
        if (rkey !== "protocol") {
          result[rkey] = relative[rkey];
        }
      }
      if (slashedProtocol[result.protocol] && result.hostname && !result.pathname) {
        result.pathname = "/";
        result.path = result.pathname;
      }
      result.href = result.format();
      return result;
    }
    const relativeProtocol = relative.protocol;
    if (relativeProtocol && relativeProtocol !== result.protocol) {
      if (!slashedProtocol[relativeProtocol]) {
        var keys = Object.keys(relative);
        for (var v = 0;v < keys.length; v++) {
          var k = keys[v];
          result[k] = relative[k];
        }
        result.href = result.format();
        return result;
      }
      result.protocol = relativeProtocol;
      if (!relative.host && !(relativeProtocol === "file" || relativeProtocol === "file:") && !hostlessProtocol[relativeProtocol]) {
        let relPath = (relative.pathname || "").split("/");
        while (relPath.length && !(relative.host = relPath.shift())) {}
        relative.host ||= "";
        relative.hostname ||= "";
        if (relPath[0] !== "")
          relPath.unshift("");
        if (relPath.length < 2)
          relPath.unshift("");
        result.pathname = relPath.join("/");
      } else {
        result.pathname = relative.pathname;
      }
      result.search = relative.search;
      result.query = relative.query;
      result.host = relative.host || "";
      result.auth = relative.auth;
      result.hostname = relative.hostname || relative.host;
      result.port = relative.port;
      const resultPathname = result.pathname;
      let resultSearch;
      if (resultPathname || (resultSearch = result.search)) {
        result.path = (resultPathname || "") + ((resultSearch ?? result.search) || "");
      }
      result.slashes = result.slashes || relative.slashes;
      result.href = result.format();
      return result;
    }
    const isSourceAbs = result.pathname && result.pathname.charAt(0) === "/";
    const isRelAbs = relative.host || relative.pathname && relative.pathname.charAt(0) === "/";
    let mustEndAbs = isRelAbs || isSourceAbs || result.host && relative.pathname;
    const removeAllDots = mustEndAbs;
    let srcPath = result.pathname && result.pathname.split("/") || [];
    const relPath = relative.pathname && relative.pathname.split("/") || [];
    const psychotic = result.protocol && !slashedProtocol[result.protocol];
    if (psychotic) {
      result.hostname = "";
      result.port = null;
      const resultHost = result.host;
      if (resultHost) {
        if (srcPath[0] === "")
          srcPath[0] = resultHost;
        else
          srcPath.unshift(resultHost);
      }
      result.host = "";
      if (relative.protocol) {
        relative.hostname = null;
        relative.port = null;
        result.auth = null;
        const relativeHost = relative.host;
        if (relativeHost) {
          if (relPath[0] === "") {
            relPath[0] = relativeHost;
          } else {
            relPath.unshift(relativeHost);
          }
        }
        relative.host = null;
      }
      mustEndAbs &&= relPath[0] === "" || srcPath[0] === "";
    }
    if (isRelAbs) {
      const relativeHost = relative.host;
      if (relativeHost || relativeHost === "") {
        if (result.host !== relativeHost)
          result.auth = null;
        result.host = relativeHost;
        result.port = relative.port;
      }
      const relativeHostname = relative.hostname;
      if (relativeHostname || relativeHostname === "") {
        if (result.hostname !== relativeHostname)
          result.auth = null;
        result.hostname = relativeHostname;
      }
      result.search = relative.search;
      result.query = relative.query;
      srcPath = relPath;
    } else if (relPath.length) {
      srcPath ||= [];
      srcPath.pop();
      srcPath = srcPath.concat(relPath);
      result.search = relative.search;
      result.query = relative.query;
    } else {
      const relativeSearch = relative.search;
      if (relativeSearch != null && relativeSearch !== undefined) {
        if (psychotic) {
          result.hostname = result.host = srcPath.shift();
          var authInHost = result.host && result.host.indexOf("@") > 0 ? result.host.split("@") : false;
          if (authInHost) {
            result.auth = authInHost.shift();
            result.hostname = result.host = authInHost.shift();
          }
        }
        result.search = relativeSearch;
        result.query = relative.query;
        const resultPathname = result.pathname;
        const resultSearch = result.search;
        if (resultPathname !== null || resultSearch !== null) {
          result.path = (resultPathname ? resultPathname : "") + (resultSearch ? resultSearch : "");
        }
        result.href = result.format();
        return result;
      }
    }
    if (!srcPath.length) {
      result.pathname = null;
      const resultSearch = result.search;
      if (resultSearch) {
        result.path = "/" + resultSearch;
      } else {
        result.path = null;
      }
      result.href = result.format();
      return result;
    }
    var last = srcPath.slice(-1)[0];
    var hasTrailingSlash = (result.host || relative.host || srcPath.length > 1) && (last === "." || last === "..") || last === "";
    var up = 0;
    for (var i = srcPath.length;i >= 0; i--) {
      last = srcPath[i];
      if (last === ".") {
        srcPath.splice(i, 1);
      } else if (last === "..") {
        srcPath.splice(i, 1);
        up++;
      } else if (up) {
        srcPath.splice(i, 1);
        up--;
      }
    }
    if (!mustEndAbs && !removeAllDots) {
      for (;up--; up) {
        srcPath.unshift("..");
      }
    }
    if (mustEndAbs && srcPath[0] !== "" && (!srcPath[0] || srcPath[0].charAt(0) !== "/")) {
      srcPath.unshift("");
    }
    if (hasTrailingSlash && srcPath.join("/").substr(-1) !== "/") {
      srcPath.push("");
    }
    var isAbsolute = srcPath[0] === "" || srcPath[0] && srcPath[0].charAt(0) === "/";
    if (psychotic) {
      result.hostname = isAbsolute ? "" : srcPath.length ? srcPath.shift() : "";
      result.host = result.hostname;
      var authInHost = result.host && result.host.indexOf("@") > 0 ? result.host.split("@") : false;
      if (authInHost) {
        result.auth = authInHost.shift();
        result.hostname = result.host = authInHost.shift();
      }
    }
    mustEndAbs ||= result.host && srcPath.length;
    if (mustEndAbs && !isAbsolute) {
      srcPath.unshift("");
    }
    if (srcPath.length > 0) {
      result.pathname = srcPath.join("/");
    } else {
      result.pathname = null;
      result.path = null;
    }
    const resultPathname = result.pathname;
    let resultSearch;
    if (resultPathname !== null || (resultSearch = result.search) !== null) {
      resultSearch ??= result.search;
      result.path = (resultPathname ? resultPathname : "") + (resultSearch ? resultSearch : "");
    }
    result.auth = relative.auth || result.auth;
    result.slashes = result.slashes || relative.slashes;
    result.href = result.format();
    return result;
  };
  Url.prototype.parseHost = function parseHost() {
    var host = this.host;
    var port = portPattern.exec(host);
    if (port) {
      port = port[0];
      if (port !== ":") {
        this.port = port.slice(1);
      }
      host = host.slice(0, host.length - port.length);
    }
    if (host)
      this.hostname = host;
  };
  var Char;
  ((Char) => {
    Char[Char["AT"] = 64] = "AT";
    Char[Char["COLON"] = 58] = "COLON";
    Char[Char["BACKWARD_SLASH"] = 92] = "BACKWARD_SLASH";
    Char[Char["FORWARD_SLASH"] = 47] = "FORWARD_SLASH";
    Char[Char["HASH"] = 35] = "HASH";
    Char[Char["QUESTION_MARK"] = 63] = "QUESTION_MARK";
    Char[Char["PERCENT"] = 37] = "PERCENT";
    Char[Char["LEFT_SQUARE_BRACKET"] = 91] = "LEFT_SQUARE_BRACKET";
    Char[Char["RIGHT_SQUARE_BRACKET"] = 93] = "RIGHT_SQUARE_BRACKET";
    Char[Char["DOUBLE_QUOTE"] = 34] = "DOUBLE_QUOTE";
    Char[Char["SINGLE_QUOTE"] = 39] = "SINGLE_QUOTE";
    Char[Char["SEMICOLON"] = 59] = "SEMICOLON";
    Char[Char["LEFT_ANGLE_BRACKET"] = 60] = "LEFT_ANGLE_BRACKET";
    Char[Char["RIGHT_ANGLE_BRACKET"] = 62] = "RIGHT_ANGLE_BRACKET";
    Char[Char["CIRCUMFLEX_ACCENT"] = 94] = "CIRCUMFLEX_ACCENT";
    Char[Char["GRAVE_ACCENT"] = 96] = "GRAVE_ACCENT";
    Char[Char["LEFT_CURLY_BRACKET"] = 123] = "LEFT_CURLY_BRACKET";
    Char[Char["VERTICAL_LINE"] = 124] = "VERTICAL_LINE";
    Char[Char["RIGHT_CURLY_BRACKET"] = 125] = "RIGHT_CURLY_BRACKET";
    Char[Char["TAB"] = 9] = "TAB";
    Char[Char["LINE_FEED"] = 10] = "LINE_FEED";
    Char[Char["CARRIAGE_RETURN"] = 13] = "CARRIAGE_RETURN";
    Char[Char["SPACE"] = 32] = "SPACE";
    Char[Char["NO_BREAK_SPACE"] = 160] = "NO_BREAK_SPACE";
    Char[Char["ZERO_WIDTH_NOBREAK_SPACE"] = 65279] = "ZERO_WIDTH_NOBREAK_SPACE";
  })(Char ||= {});
  const isWindows = process.platform === "win32";
  const encodePathCharsRe = /[\0\t\n\r "#%?[\\\]^|~]/g;
  const encodePathCharsMap = {
    __proto__: null,
    "\x00": "%00",
    "\t": "%09",
    "\n": "%0A",
    "\r": "%0D",
    " ": "%20",
    '"': "%22",
    "#": "%23",
    "%": "%25",
    "?": "%3F",
    "[": "%5B",
    "\\": "%5C",
    "]": "%5D",
    "^": "%5E",
    "|": "%7C",
    "~": "%7E"
  };
  function encodeWindowsPathChar(ch) {
    return ch === "\\" ? "/" : encodePathCharsMap[ch];
  }
  function encodePosixPathChar(ch) {
    return encodePathCharsMap[ch];
  }
  function createFileURL(filepath, windows, hostname) {
    let outURL;
    try {
      outURL = new URL("file://" + filepath.replace(encodePathCharsRe, windows ? encodeWindowsPathChar : encodePosixPathChar));
    } catch {
      throw codes.INVALID_URL(filepath);
    }
    if (hostname !== undefined) {
      try {
        new URL("file://" + hostname + "/");
      } catch {
        throw codes.INVALID_URL(filepath, hostname);
      }
      outURL.hostname = hostname;
    }
    return outURL;
  }
  function pathToFileURL(filepath, options) {
    validateString(filepath, "path");
    const path = host.requireBuiltin("node:path");
    const windows = options?.windows ?? isWindows;
    const isUNC = windows && filepath.startsWith("\\\\");
    let resolved = isUNC ? filepath : windows ? path.win32.resolve(filepath) : path.posix.resolve(filepath);
    if (isUNC || windows && resolved.startsWith("\\\\")) {
      const isExtendedUNC = resolved.startsWith("\\\\?\\UNC\\");
      const prefixLength = isExtendedUNC ? 8 : 2;
      const hostnameEndIndex = resolved.indexOf("\\", prefixLength);
      if (hostnameEndIndex === -1) {
        throw codes.INVALID_ARG_VALUE("path", resolved, "Missing UNC resource path");
      }
      if (hostnameEndIndex === prefixLength) {
        throw codes.INVALID_ARG_VALUE("path", resolved, "Empty UNC servername");
      }
      const hostname = resolved.slice(prefixLength, hostnameEndIndex);
      return createFileURL(resolved.slice(hostnameEndIndex), true, hostname);
    }
    const filePathLast = filepath.charCodeAt(filepath.length - 1);
    if ((filePathLast === 47 /* FORWARD_SLASH */ || windows && filePathLast === 92 /* BACKWARD_SLASH */) && resolved[resolved.length - 1] !== path.sep) {
      resolved += "/";
    }
    return createFileURL(resolved, windows);
  }
  function getPathFromURLWin32(url) {
    const hostname = url.hostname;
    let pathname = url.pathname;
    for (let n = 0;n < pathname.length; n++) {
      if (pathname[n] === "%") {
        const third = pathname.codePointAt(n + 2) | 32;
        if (pathname[n + 1] === "2" && third === 102 || pathname[n + 1] === "5" && third === 99) {
          throw codes.INVALID_FILE_URL_PATH("File URL path must not include encoded \\ or / characters");
        }
      }
    }
    pathname = pathname.replace(/\//g, "\\");
    pathname = decodeURIComponent(pathname);
    if (hostname !== "") {
      return `\\\\${domainToUnicode(hostname)}${pathname}`;
    }
    const letter = pathname.codePointAt(1) | 32;
    const sep = pathname[2];
    if (letter < 97 || letter > 122 || sep !== ":") {
      throw codes.INVALID_FILE_URL_PATH("File URL path must be absolute");
    }
    return pathname.slice(1);
  }
  function getPathFromURLPosix(url) {
    if (url.hostname !== "") {
      throw codes.INVALID_FILE_URL_HOST(`File URL host must be "localhost" or empty on ${process.platform}`);
    }
    const pathname = url.pathname;
    for (let n = 0;n < pathname.length; n++) {
      if (pathname[n] === "%") {
        const third = pathname.codePointAt(n + 2) | 32;
        if (pathname[n + 1] === "2" && third === 102) {
          throw codes.INVALID_FILE_URL_PATH("File URL path must not include encoded / characters");
        }
      }
    }
    return decodeURIComponent(pathname);
  }
  function fileURLToPath(url, options) {
    const windows = options?.windows;
    if (windows !== undefined && windows !== null) {
      let parsed;
      if (typeof url === "string") {
        parsed = new URL(url);
      } else if (url instanceof URL) {
        parsed = url;
      } else {
        throw codes.INVALID_ARG_TYPE("path", ["string", "URL"], url);
      }
      if (parsed.protocol !== "file:") {
        throw codes.INVALID_URL_SCHEME("file");
      }
      return windows ? getPathFromURLWin32(parsed) : getPathFromURLPosix(parsed);
    }
    try {
      return Bun.fileURLToPath(url);
    } catch (err) {
      if (err?.code === "ERR_INVALID_FILE_URL_PATH") {
        err.input = typeof url === "string" ? new URL(url) : url;
      }
      throw err;
    }
  }
  function percentDecodeIntoBuffer(pathname) {
    const input = Buffer.from(pathname, "utf8");
    const length = input.length;
    const output = Buffer.allocUnsafe(length);
    let j = 0;
    for (let i = 0;i < length; ++i) {
      const byte = input[i];
      if (byte !== 37 /* PERCENT */) {
        output[j++] = byte;
        continue;
      }
      const hi = hexByteToNumber(input[i + 1]);
      const lo = hexByteToNumber(input[i + 2]);
      if (hi === -1 || lo === -1) {
        output[j++] = 37 /* PERCENT */;
        continue;
      }
      output[j++] = hi << 4 | lo;
      i += 2;
    }
    return output.subarray(0, j);
  }
  function hexByteToNumber(byte) {
    if (byte >= 48 && byte <= 57)
      return byte - 48;
    if (byte >= 65 && byte <= 70)
      return byte - 65 + 10;
    if (byte >= 97 && byte <= 102)
      return byte - 97 + 10;
    return -1;
  }
  function isURL(self) {
    return Boolean(self?.href && self.protocol && self.auth === undefined && self.path === undefined);
  }
  function fileURLToPathBuffer(path, options) {
    const windows = options?.windows;
    if (typeof path === "string") {
      path = new URL(path);
    } else if (!isURL(path)) {
      throw codes.INVALID_ARG_TYPE("path", ["string", "URL"], path);
    }
    const url = path;
    if (url.protocol !== "file:") {
      throw codes.INVALID_URL_SCHEME("The URL must be of scheme file");
    }
    if (windows ?? process.platform === "win32") {
      let pathname = url.pathname.replaceAll("/", "\\");
      const decoded = percentDecodeIntoBuffer(pathname);
      const hostname = url.hostname;
      if (hostname !== "") {
        return Buffer.concat([Buffer.from("\\\\", "ascii"), Buffer.from(domainToUnicode(hostname), "utf8"), decoded]);
      }
      const letter = decoded[1] | 32;
      const sep = decoded[2];
      if (letter < 97 || letter > 122 || sep !== 58 /* COLON */) {
        throw codes.INVALID_FILE_URL_PATH("File URL path must be absolute");
      }
      return decoded.subarray(1);
    }
    if (url.hostname !== "") {
      throw codes.INVALID_FILE_URL_HOST(`File URL host must be "localhost" or empty on ${process.platform}`);
    }
    return percentDecodeIntoBuffer(url.pathname);
  }
  return {
    parse: urlParse,
    resolve: urlResolve,
    resolveObject: urlResolveObject,
    format: urlFormat,
    Url,
    URLSearchParams,
    URL,
    URLPattern,
    pathToFileURL,
    fileURLToPath,
    fileURLToPathBuffer,
    urlToHttpOptions,
    domainToASCII,
    domainToUnicode
  };

})
