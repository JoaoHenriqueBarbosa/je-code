// A semântica do `node:fs` do bun 1.4.2 (ver `node_fs.rs`). Programa escondido: recebe o `binding` (uma camada fina
// sobre o `Host`, cujas funções lançam o `-errno` como número) e devolve a tabela de exports na ordem do bun; os
// dados vão embrulhados em `{ v }`.
(function (b) {
  "use strict";

  const O = { RDONLY: 0, WRONLY: 1, RDWR: 2, CREAT: 64, EXCL: 128, NOCTTY: 256, TRUNC: 512, APPEND: 1024, DIRECTORY: 65536, NOFOLLOW: 131072, NOATIME: 262144, SYNC: 1052672, DSYNC: 4096, DIRECT: 16384, NONBLOCK: 2048 };
  const S_IFMT = 61440, S_IFREG = 32768, S_IFDIR = 16384, S_IFCHR = 8192, S_IFBLK = 24576, S_IFIFO = 4096, S_IFLNK = 40960, S_IFSOCK = 49152;

  // `uv_strerror`
  const ERRORS = {
    1: ["EPERM", "operation not permitted"], 2: ["ENOENT", "no such file or directory"], 3: ["ESRCH", "no such process"],
    4: ["EINTR", "interrupted system call"], 5: ["EIO", "i/o error"], 6: ["ENXIO", "no such device or address"],
    7: ["E2BIG", "argument list too long"], 9: ["EBADF", "bad file descriptor"], 11: ["EAGAIN", "resource temporarily unavailable"],
    12: ["ENOMEM", "not enough memory"], 13: ["EACCES", "permission denied"], 14: ["EFAULT", "bad address in system call argument"],
    16: ["EBUSY", "resource busy or locked"], 17: ["EEXIST", "file already exists"], 18: ["EXDEV", "cross-device link not permitted"],
    19: ["ENODEV", "no such device"], 20: ["ENOTDIR", "not a directory"], 21: ["EISDIR", "illegal operation on a directory"],
    22: ["EINVAL", "invalid argument"], 23: ["ENFILE", "file table overflow"], 24: ["EMFILE", "too many open files"],
    25: ["ENOTTY", "inappropriate ioctl for device"], 26: ["ETXTBSY", "text file is busy"], 27: ["EFBIG", "file too large"],
    28: ["ENOSPC", "no space left on device"], 29: ["ESPIPE", "invalid seek"], 30: ["EROFS", "read-only file system"],
    31: ["EMLINK", "too many links"], 32: ["EPIPE", "broken pipe"], 34: ["ERANGE", "result too large"],
    36: ["ENAMETOOLONG", "name too long"], 38: ["ENOSYS", "function not implemented"], 39: ["ENOTEMPTY", "directory not empty"],
    40: ["ELOOP", "too many symbolic links encountered"], 95: ["ENOTSUP", "operation not supported on socket"],
  };

  function uvError(negative, syscall, path, dest, fd) {
    const entry = ERRORS[-negative] || ["UNKNOWN", "unknown error"];
    let message = entry[0] + ": " + entry[1] + ", " + syscall;
    if (path !== undefined) {
      message += " '" + path + "'";
      if (dest !== undefined) message += " -> '" + dest + "'";
    }
    const error = new Error(message);
    error.code = entry[0];
    if (path !== undefined) error.path = path;
    if (fd !== undefined) error.fd = fd;
    if (dest !== undefined) error.dest = dest;
    error.syscall = syscall;
    error.errno = negative;
    return error;
  }

  // Chama o `binding` e converte o `-errno` lançado no erro do Node.
  function sys(syscall, path, dest, fn, ...args) {
    try {
      return fn(...args);
    } catch (thrown) {
      if (typeof thrown === "number") throw uvError(thrown, syscall, path, dest);
      throw thrown;
    }
  }

  function sysFd(syscall, fd, fn, ...args) {
    try {
      return fn(...args);
    } catch (thrown) {
      if (typeof thrown === "number") throw uvError(thrown, syscall, undefined, undefined, fd);
      throw thrown;
    }
  }

  function codedError(Kind, message, code) {
    const error = new Kind(message);
    Object.defineProperty(error, "code", { value: code, writable: true, configurable: true, enumerable: false });
    return error;
  }

  const typeError = (message) => codedError(TypeError, message, "ERR_INVALID_ARG_TYPE");
  const valueError = (message) => codedError(TypeError, message, "ERR_INVALID_ARG_VALUE");

  function toPath(value, message) {
    if (typeof value === "string") return value;
    if (value instanceof Uint8Array) return Buffer.from(value.buffer, value.byteOffset, value.byteLength).toString();
    if (value !== null && typeof value === "object" && typeof value.href === "string" && typeof value.protocol === "string") {
      if (value.protocol !== "file:") throw codedError(TypeError, "The URL must be of scheme file", "ERR_INVALID_URL_SCHEME");
      return decodeURIComponent(value.pathname);
    }
    throw typeError(message || "path must be a string or TypedArray");
  }

  function parseFlags(flags, fallback) {
    if (flags === undefined || flags === null) return fallback;
    if (typeof flags === "number") return flags;
    switch (flags) {
      case "r": return O.RDONLY;
      case "rs": case "sr": return O.RDONLY | O.SYNC;
      case "r+": return O.RDWR;
      case "rs+": case "sr+": return O.RDWR | O.SYNC;
      case "w": return O.WRONLY | O.CREAT | O.TRUNC;
      case "wx": case "xw": return O.WRONLY | O.CREAT | O.TRUNC | O.EXCL;
      case "w+": return O.RDWR | O.CREAT | O.TRUNC;
      case "wx+": case "xw+": return O.RDWR | O.CREAT | O.TRUNC | O.EXCL;
      case "a": return O.WRONLY | O.APPEND | O.CREAT;
      case "ax": case "xa": return O.WRONLY | O.APPEND | O.CREAT | O.EXCL;
      case "as": case "sa": return O.WRONLY | O.APPEND | O.CREAT | O.SYNC;
      case "a+": return O.RDWR | O.APPEND | O.CREAT;
      case "ax+": case "xa+": return O.RDWR | O.APPEND | O.CREAT | O.EXCL;
      case "as+": case "sa+": return O.RDWR | O.APPEND | O.CREAT | O.SYNC;
    }
    throw valueError("The argument 'flags' is invalid. Received " + (typeof flags === "string" ? "'" + flags + "'" : String(flags)));
  }

  function parseEncoding(encoding, fallback) {
    if (encoding === undefined || encoding === null) return fallback;
    if (encoding === "buffer" || (typeof encoding === "string" && Buffer.isEncoding(encoding))) return encoding;
    throw valueError("encoding '" + String(encoding) + "' is an invalid encoding");
  }

  // `options` como o Node o lê: texto é a codificação, objeto traz `encoding`.
  function readOptions(options, defaults) {
    if (typeof options === "string") return { ...defaults, encoding: parseEncoding(options, defaults.encoding) };
    if (options !== null && typeof options === "object") {
      const merged = { ...defaults, ...options };
      merged.encoding = parseEncoding(options.encoding, defaults.encoding);
      return merged;
    }
    return { ...defaults };
  }

  function parseMode(mode, fallback) {
    if (mode === undefined || mode === null) return fallback;
    if (typeof mode === "string") return parseInt(mode, 8);
    return mode;
  }

  function checkFd(fd) {
    if (typeof fd !== "number" || !Number.isInteger(fd)) throw typeError('The "fd" argument must be of type number. Received ' + describe(fd));
    return fd;
  }

  function describe(value) {
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    if (typeof value === "function") return "function " + value.name;
    if (typeof value === "object") return value.constructor && value.constructor.name ? "an instance of " + value.constructor.name : "[Object: null prototype]";
    let shown = typeof value === "string" ? "'" + value + "'" : String(value);
    return "type " + typeof value + " (" + shown + ")";
  }

  // ---- Stats e Dirent ----
  // `atime`, `mtime`, `ctime` e `birthtime` nascem na primeira leitura como propriedade própria (medido no bun).
  function ownDate(object, key, value) {
    Object.defineProperty(object, key, { value, writable: true, enumerable: true, configurable: true });
    return value;
  }

  function Stats() {
    this.dev = arguments[0]; this.ino = arguments[7]; this.mode = arguments[1]; this.nlink = arguments[2]; this.uid = arguments[3];
    this.gid = arguments[4]; this.rdev = arguments[5]; this.size = arguments[8]; this.blksize = arguments[6]; this.blocks = arguments[9];
    this.atimeMs = arguments[10]; this.mtimeMs = arguments[11]; this.ctimeMs = arguments[12]; this.birthtimeMs = arguments[13];
  }
  function BigIntStats() {
    this.dev = arguments[0]; this.ino = arguments[7]; this.mode = arguments[1]; this.nlink = arguments[2]; this.uid = arguments[3];
    this.gid = arguments[4]; this.rdev = arguments[5]; this.size = arguments[8]; this.blksize = arguments[6]; this.blocks = arguments[9];
    this.atimeMs = arguments[10]; this.mtimeMs = arguments[11]; this.ctimeMs = arguments[12]; this.birthtimeMs = arguments[13];
    this.atimeNs = arguments[14]; this.mtimeNs = arguments[15]; this.ctimeNs = arguments[16]; this.birthtimeNs = arguments[17];
  }

  const typeOf = (mode) => Number(mode) & S_IFMT;
  const kinds = {
    isBlockDevice: S_IFBLK, isCharacterDevice: S_IFCHR, isDirectory: S_IFDIR, isFIFO: S_IFIFO, isFile: S_IFREG, isSocket: S_IFSOCK, isSymbolicLink: S_IFLNK,
  };
  for (const Class of [Stats, BigIntStats]) {
    for (const name of Object.keys(kinds)) {
      const bits = kinds[name];
      const method = { [name]() { return typeOf(this.mode) === bits; } }[name];
      Object.defineProperty(Class.prototype, name, { value: method, writable: true, enumerable: true, configurable: true });
    }
    for (const key of ["atime", "mtime", "ctime", "birthtime"]) {
      const getter = Object.getOwnPropertyDescriptor({ get [key]() { return ownDate(this, key, new Date(Number(this[key + "Ms"]))); } }, key).get;
      const setter = Object.getOwnPropertyDescriptor({ set [key](value) { ownDate(this, key, value); } }, key).set;
      Object.defineProperty(Class.prototype, key, { get: getter, set: setter, enumerable: true, configurable: true });
    }
  }

  function makeStats(array, bigint) {
    if (bigint) {
      const ms = (seconds, nanos) => BigInt(seconds) * 1000n + BigInt(nanos) / 1000000n;
      const ns = (seconds, nanos) => BigInt(seconds) * 1000000000n + BigInt(nanos);
      return new BigIntStats(BigInt(array[0]), BigInt(array[2]), BigInt(array[3]), BigInt(array[4]), BigInt(array[5]), BigInt(array[6]), BigInt(array[8]), BigInt(array[1]), BigInt(array[7]), BigInt(array[9]),
        ms(array[10], array[11]), ms(array[12], array[13]), ms(array[14], array[15]), ms(array[16], array[17]),
        ns(array[10], array[11]), ns(array[12], array[13]), ns(array[14], array[15]), ns(array[16], array[17]));
    }
    const ms = (seconds, nanos) => seconds * 1000 + nanos / 1000000;
    return new Stats(array[0], array[2], array[3], array[4], array[5], array[6], array[8], array[1], array[7], array[9],
      ms(array[10], array[11]), ms(array[12], array[13]), ms(array[14], array[15]), ms(array[16], array[17]));
  }

  function Dirent(name, type, path) {
    this.name = name;
    this.path = path;
    this.parentPath = path;
    Object.defineProperty(this, "type", { value: type, writable: true, enumerable: false, configurable: true });
  }
  for (const name of Object.keys(kinds)) {
    const bits = kinds[name];
    const method = { [name]() { return this.type === bits; } }[name];
    Object.defineProperty(Dirent.prototype, name, { value: method, writable: true, enumerable: true, configurable: true });
  }

  // ---- caminhos ----
  const cwd = () => process.cwd();
  function resolvePath(path) {
    if (path.startsWith("/")) return normalize(path);
    const base = cwd();
    return normalize(base + (base.endsWith("/") ? "" : "/") + path);
  }
  function normalize(path) {
    const parts = [];
    for (const part of path.split("/")) {
      if (part === "" || part === ".") continue;
      if (part === "..") parts.pop();
      else parts.push(part);
    }
    return "/" + parts.join("/");
  }
  function parentOf(path) {
    const index = path.lastIndexOf("/");
    return index <= 0 ? "/" : path.slice(0, index);
  }
  const join = (directory, name) => (directory.endsWith("/") ? directory + name : directory + "/" + name);

  // ---- arquivos ----
  function readAll(fd, size) {
    const chunks = [];
    let total = 0;
    let capacity = size > 0 ? size : 8192;
    for (;;) {
      const chunk = Buffer.allocUnsafe(capacity);
      const read = sys("read", undefined, undefined, b.read, fd, chunk, 0, capacity, null);
      if (read === 0) break;
      chunks.push(read === capacity ? chunk : chunk.subarray(0, read));
      total += read;
      capacity = size > total ? size - total : 65536;
    }
    return chunks.length === 1 ? chunks[0] : Buffer.concat(chunks, total);
  }

  function writeAll(fd, buffer) {
    let offset = 0;
    while (offset < buffer.length) {
      offset += sys("write", undefined, undefined, b.write, fd, buffer.subarray(offset), null);
    }
  }

  function readFileSync(path, options) {
    const opts = readOptions(options, { encoding: null, flag: "r" });
    let fd;
    let owned = false;
    if (typeof path === "number") {
      fd = path;
    } else {
      if (typeof path !== "string" && !(path instanceof Uint8Array) && !(path !== null && typeof path === "object" && typeof path.href === "string")) {
        throw typeError("path must be a string or a file descriptor");
      }
      const text = toPath(path);
      fd = sys("open", text, undefined, b.open, text, parseFlags(opts.flag, O.RDONLY), 0o666);
      owned = true;
    }
    let result;
    try {
      const stat = sys("fstat", undefined, undefined, b.fstat, fd);
      result = readAll(fd, (stat[2] & S_IFMT) === S_IFREG ? stat[7] : 0);
    } finally {
      if (owned) b.close(fd);
    }
    return opts.encoding && opts.encoding !== "buffer" ? result.toString(opts.encoding) : result;
  }

  function toBytes(data, encoding) {
    if (typeof data === "string") return Buffer.from(data, encoding);
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    throw typeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
  }

  function writeFile(path, data, options, defaultFlag) {
    const opts = readOptions(options, { encoding: "utf8", mode: 0o666, flag: defaultFlag });
    const bytes = toBytes(data, opts.encoding);
    let fd;
    let owned = false;
    if (typeof path === "number") {
      fd = path;
    } else {
      const text = toPath(path);
      fd = sys("open", text, undefined, b.open, text, parseFlags(opts.flag, O.WRONLY | O.CREAT | O.TRUNC), parseMode(opts.mode, 0o666));
      owned = true;
    }
    try {
      writeAll(fd, bytes);
    } finally {
      if (owned) b.close(fd);
    }
  }
  const writeFileSync = function writeFileSync(path, data, options) { writeFile(path, data, options, "w"); };
  const appendFileSync = function appendFileSync(path, data, options) { writeFile(path, data, options, "a"); };

  function existsSync(path) {
    try {
      b.access(toPath(path), 0);
      return true;
    } catch {
      return false;
    }
  }

  function statWith(syscall, call, options, path) {
    const opts = options !== null && typeof options === "object" ? options : {};
    try {
      return makeStats(call(), opts.bigint === true);
    } catch (thrown) {
      if (typeof thrown === "number") {
        if (opts.throwIfNoEntry === false && (thrown === -2 || thrown === -20)) return undefined;
        throw uvError(thrown, syscall, path);
      }
      throw thrown;
    }
  }
  function statSync(path, options) {
    const text = toPath(path);
    return statWith("stat", () => b.stat(text, true), options, text);
  }
  function lstatSync(path, options) {
    const text = toPath(path);
    return statWith("lstat", () => b.stat(text, false), options, text);
  }
  function fstatSync(fd, options) {
    checkFd(fd);
    const opts = options !== null && typeof options === "object" ? options : {};
    return makeStats(sysFd("fstat", fd, b.fstat, fd), opts.bigint === true);
  }

  function readdirSync(path, options) {
    const opts = readOptions(options, { encoding: "utf8", withFileTypes: false, recursive: false });
    const text = toPath(path);
    const name = (entry) => (opts.encoding === "buffer" ? Buffer.from(entry) : entry);
    const result = [];
    const queue = [""];
    for (let index = 0; index < queue.length; index++) {
      const relative = queue[index];
      const directory = relative ? join(text, relative) : text;
      const flat = sys("scandir", text, undefined, b.readdir, directory);
      for (let i = 0; i < flat.length; i += 2) {
        const entry = flat[i];
        const shown = relative ? relative + "/" + entry : entry;
        result.push(opts.withFileTypes ? new Dirent(name(entry), flat[i + 1], directory) : name(shown));
        if (opts.recursive && flat[i + 1] === S_IFDIR) queue.push(shown);
      }
      if (!opts.recursive) break;
    }
    return result;
  }

  function mkdirSync(path, options) {
    const text = toPath(path);
    let recursive = false;
    let mode = 0o777;
    if (options !== null && typeof options === "object") {
      recursive = options.recursive === true;
      mode = parseMode(options.mode, 0o777);
    } else if (options !== undefined) {
      mode = parseMode(options, 0o777);
    }
    if (!recursive) {
      sys("mkdir", text, undefined, b.mkdir, text, mode);
      return undefined;
    }
    const absolute = resolvePath(text);
    const pending = [];
    let current = absolute;
    for (;;) {
      try {
        const stat = b.stat(current, true);
        if ((stat[2] & S_IFMT) !== S_IFDIR) throw uvError(-(pending.length === 0 ? 17 : 20), "mkdir", text);
        break;
      } catch (thrown) {
        if (thrown !== -2) {
          if (typeof thrown === "number") throw uvError(thrown, "mkdir", text);
          throw thrown;
        }
        pending.push(current);
        const parent = parentOf(current);
        if (parent === current) break;
        current = parent;
      }
    }
    for (let i = pending.length - 1; i >= 0; i--) {
      try {
        b.mkdir(pending[i], mode);
      } catch (thrown) {
        if (thrown === -17) continue;
        if (typeof thrown === "number") throw uvError(thrown, "mkdir", text);
        throw thrown;
      }
    }
    return pending.length ? pending[pending.length - 1] : undefined;
  }

  function removeTree(path) {
    const flat = b.readdir(path);
    for (let i = 0; i < flat.length; i += 2) {
      const child = join(path, flat[i]);
      if (flat[i + 1] === S_IFDIR) removeTree(child);
      else b.unlink(child);
    }
    b.rmdir(path);
  }

  function rmSync(path, options) {
    const text = toPath(path);
    const opts = options !== null && typeof options === "object" ? options : {};
    let stat;
    try {
      stat = b.stat(text, false);
    } catch (thrown) {
      if (typeof thrown === "number") {
        if (opts.force === true && (thrown === -2 || thrown === -20)) return;
        throw uvError(thrown, "lstat", text);
      }
      throw thrown;
    }
    if ((stat[2] & S_IFMT) === S_IFDIR) {
      if (opts.recursive !== true) {
        const error = new Error("Path is a directory: rm returned EISDIR (is a directory) " + text);
        error.name = "SystemError";
        error.info = { code: "EISDIR", message: "is a directory", path: text, syscall: "rm", errno: 21 };
        error.errno = 21;
        error.syscall = "rm";
        error.path = text;
        Object.defineProperty(error, "code", { value: "ERR_FS_EISDIR", writable: true, configurable: true, enumerable: false });
        throw error;
      }
      try {
        removeTree(text);
      } catch (thrown) {
        if (typeof thrown === "number") {
          if (opts.force === true && thrown === -2) return;
          throw uvError(thrown, "rm", text);
        }
        throw thrown;
      }
      return;
    }
    sys("unlink", text, undefined, b.unlink, text);
  }

  function rmdirSync(path, options) {
    const text = toPath(path);
    if (options !== null && typeof options === "object" && options.recursive === true) return rmSync(text, { recursive: true });
    sys("rmdir", text, undefined, b.rmdir, text);
  }

  function unlinkSync(path) {
    const text = toPath(path);
    sys("unlink", text, undefined, b.unlink, text);
  }

  function renameSync(from, to) {
    const source = toPath(from);
    const target = toPath(to);
    sys("rename", source, target, b.rename, source, target);
  }

  function copyFileSync(from, to, mode) {
    const source = toPath(from);
    const target = toPath(to);
    const exclusive = ((mode === undefined ? 0 : mode) & 1) !== 0;
    const run = () => {
      const input = b.open(source, O.RDONLY, 0);
      try {
        const stat = b.fstat(input);
        if ((stat[2] & S_IFMT) === S_IFDIR) throw -21;
        const output = b.open(target, O.WRONLY | O.CREAT | (exclusive ? O.EXCL : O.TRUNC), stat[2] & 0o7777);
        try {
          for (;;) {
            const chunk = Buffer.allocUnsafe(65536);
            const read = b.read(input, chunk, 0, 65536, null);
            if (read === 0) break;
            let offset = 0;
            while (offset < read) offset += b.write(output, chunk.subarray(offset, read), null);
          }
        } finally {
          b.close(output);
        }
      } finally {
        b.close(input);
      }
    };
    sys("copyfile", source, target, run);
  }

  function linkSync(existing, created) {
    const source = toPath(existing);
    const target = toPath(created);
    sys("link", source, target, b.link, source, target);
  }

  function symlinkSync(target, path) {
    const to = toPath(target);
    const at = toPath(path);
    sys("symlink", to, at, b.symlink, to, at);
  }

  function readlinkSync(path, options) {
    const opts = readOptions(options, { encoding: "utf8" });
    const text = toPath(path);
    const target = sys("readlink", text, undefined, b.readlink, text);
    return opts.encoding === "buffer" ? Buffer.from(target) : target;
  }

  function realpathSync(path, options) {
    const opts = readOptions(options, { encoding: "utf8" });
    const text = toPath(path);
    const resolved = sys("lstat", text, undefined, b.realpath, text);
    return opts.encoding === "buffer" ? Buffer.from(resolved) : resolved;
  }

  function accessSync(path, mode) {
    const text = toPath(path);
    sys("access", text, undefined, b.access, text, mode === undefined ? 0 : mode);
  }

  function chmodSync(path, mode) {
    const text = toPath(path);
    sys("chmod", text, undefined, b.chmod, text, parseMode(mode, 0));
  }
  function fchmodSync(fd, mode) {
    checkFd(fd);
    sysFd("fchmod", fd, b.fchmod, fd, parseMode(mode, 0));
  }

  function openSync(path, flags, mode) {
    const text = toPath(path);
    return sys("open", text, undefined, b.open, text, parseFlags(flags, O.RDONLY), parseMode(mode, 0o666));
  }
  function closeSync(fd) {
    checkFd(fd);
    sysFd("close", fd, b.close, fd);
  }

  function truncateSync(path, length) {
    if (typeof path === "number") return ftruncateSync(path, length);
    const text = toPath(path);
    const fd = sys("open", text, undefined, b.open, text, O.RDWR, 0o666);
    try {
      b.ftruncate(fd, length === undefined ? 0 : length);
    } catch (thrown) {
      if (typeof thrown === "number") throw uvError(thrown, "ftruncate", undefined, undefined, fd);
      throw thrown;
    } finally {
      b.close(fd);
    }
  }
  function ftruncateSync(fd, length) {
    checkFd(fd);
    sysFd("ftruncate", fd, b.ftruncate, fd, length === undefined ? 0 : length);
  }
  function fsyncSync(fd) {
    checkFd(fd);
    sysFd("fsync", fd, b.fsync, fd);
  }

  function toUnixTimestamp(time) {
    if (typeof time === "string" && +time == time) return +time;
    if (Number.isFinite(time)) return time < 0 ? Date.now() / 1000 : time;
    if (time instanceof Date) return time.getTime() / 1000;
    throw typeError('The "time" argument must be of type number or an instance of Date. Received ' + describe(time));
  }
  function utimesSync(path, atime, mtime) {
    const text = toPath(path);
    sys("utime", text, undefined, b.utimes, text, toUnixTimestamp(atime), toUnixTimestamp(mtime), true);
  }
  function lutimesSync(path, atime, mtime) {
    const text = toPath(path);
    sys("lutime", text, undefined, b.utimes, text, toUnixTimestamp(atime), toUnixTimestamp(mtime), false);
  }
  function futimesSync(fd, atime, mtime) {
    checkFd(fd);
    sysFd("futime", fd, b.futimes, fd, toUnixTimestamp(atime), toUnixTimestamp(mtime));
  }

  function readSync(fd, buffer, offsetOrOptions, length, position) {
    checkFd(fd);
    if (!ArrayBuffer.isView(buffer)) throw typeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView. Received ' + describe(buffer));
    let offset = offsetOrOptions;
    if (offsetOrOptions !== null && typeof offsetOrOptions === "object") {
      ({ offset = 0, length, position = null } = offsetOrOptions);
    }
    if (offset === undefined || offset === null) offset = 0;
    if (length === undefined || length === null) length = buffer.byteLength - offset;
    if (typeof position === "bigint") position = Number(position);
    if (length === 0) return 0;
    return sysFd("read", fd, b.read, fd, buffer, offset, length, position === undefined ? null : position);
  }

  function writeSync(fd, data, offsetOrPosition, lengthOrEncoding, position) {
    checkFd(fd);
    let bytes;
    let at;
    if (typeof data === "string") {
      bytes = Buffer.from(data, typeof lengthOrEncoding === "string" ? lengthOrEncoding : "utf8");
      at = offsetOrPosition === undefined ? null : offsetOrPosition;
      return sysFd("write", fd, b.write, fd, bytes, at);
    }
    if (!ArrayBuffer.isView(data)) throw typeError('The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ' + describe(data));
    let offset = offsetOrPosition;
    let length = lengthOrEncoding;
    if (offset !== null && typeof offset === "object") ({ offset = 0, length, position = null } = offset);
    if (offset === undefined || offset === null) offset = 0;
    if (length === undefined || length === null) length = data.byteLength - offset;
    const view = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    return sysFd("write", fd, b.write, fd, view.subarray(offset, offset + length), position === undefined ? null : position);
  }

  const ALPHABET = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
  function mkdtempSync(prefix, options) {
    const opts = readOptions(options, { encoding: "utf8" });
    const text = toPath(prefix);
    for (;;) {
      const random = b.random(6);
      let suffix = "";
      for (let i = 0; i < 6; i++) suffix += ALPHABET[random[i] % ALPHABET.length];
      try {
        b.mkdir(text + suffix, 0o700);
        return opts.encoding === "buffer" ? Buffer.from(text + suffix) : text + suffix;
      } catch (thrown) {
        if (thrown === -17) continue;
        if (typeof thrown === "number") throw uvError(thrown, "mkdtemp", text + "XXXXXX");
        throw thrown;
      }
    }
  }

  const constants = {
    UV_FS_SYMLINK_DIR: 1, UV_FS_SYMLINK_JUNCTION: 2, O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, UV_DIRENT_UNKNOWN: 0, UV_DIRENT_FILE: 1,
    UV_DIRENT_DIR: 2, UV_DIRENT_LINK: 3, UV_DIRENT_FIFO: 4, UV_DIRENT_SOCKET: 5, UV_DIRENT_CHAR: 6, UV_DIRENT_BLOCK: 7, S_IFMT: 61440,
    S_IFREG: 32768, S_IFDIR: 16384, S_IFCHR: 8192, S_IFBLK: 24576, S_IFIFO: 4096, S_IFLNK: 40960, S_IFSOCK: 49152, O_CREAT: 64, O_EXCL: 128,
    UV_FS_O_FILEMAP: 0, O_NOCTTY: 256, O_TRUNC: 512, O_APPEND: 1024, O_DIRECTORY: 65536, O_NOATIME: 262144, O_NOFOLLOW: 131072,
    O_SYNC: 1052672, O_DSYNC: 4096, O_DIRECT: 16384, O_NONBLOCK: 2048, S_IRWXU: 448, S_IRUSR: 256, S_IWUSR: 128, S_IXUSR: 64,
    S_IRWXG: 56, S_IRGRP: 32, S_IWGRP: 16, S_IXGRP: 8, S_IRWXO: 7, S_IROTH: 4, S_IWOTH: 2, S_IXOTH: 1, F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1,
    UV_FS_COPYFILE_EXCL: 1, COPYFILE_EXCL: 1, UV_FS_COPYFILE_FICLONE: 2, COPYFILE_FICLONE: 2, UV_FS_COPYFILE_FICLONE_FORCE: 4,
    COPYFILE_FICLONE_FORCE: 4, EXTENSIONLESS_FORMAT_JAVASCRIPT: 0, EXTENSIONLESS_FORMAT_WASM: 1,
  };

  // `[callback, promise]` de cada operação assíncrona.
  const ASYNC_LENGTHS = {
    access: [3, 3], appendFile: [4, 1], chmod: [3, 3], close: [2, 1], copyFile: [4, 4], exists: [2, 0], fchmod: [3, 3], fdatasync: [2, 2],
    fstat: [3, 1], fsync: [2, 2], ftruncate: [1, 1], futimes: [4, 4], link: [3, 3], lstat: [3, 1], lutimes: [4, 4], mkdir: [3, 3],
    mkdtemp: [3, 3], open: [4, 1], read: [6, 6], readFile: [3, 1], readdir: [3, 3], readlink: [3, 3], realpath: [3, 3], rename: [3, 3],
    rm: [3, 2], rmdir: [3, 2], stat: [3, 1], symlink: [4, 4], truncate: [3, 3], unlink: [2, 2], utimes: [4, 4], write: [6, 6], writeFile: [4, 1],
    chown: [4, 4], fchown: [4, 4], lchown: [4, 4], statfs: [3, 2], opendir: [3, 2], writev: [4, 3], readv: [4, 3], mkdtempDisposable: [0, 2], cp: [4, 3], glob: [3, 2],
  };
  // ---- chown, statfs, writev, readv, mkdtempDisposable, Dir, openAsBlob ----
  const id32 = (value) => (value === -1 ? 4294967295 : value);
  function chownArguments(uid, gid) {
    if (uid === undefined) throw typeError("uid is required");
    if (gid === undefined) throw typeError("gid is required");
    if (typeof uid !== "number") throw typeError('The "uid" argument must be of type number. Received ' + describe(uid));
    if (typeof gid !== "number") throw typeError('The "gid" argument must be of type number. Received ' + describe(gid));
    return [id32(uid), id32(gid)];
  }
  function chownSync(path, uid, gid) {
    const text = toPath(path);
    sys("chown", text, undefined, b.chown, text, ...chownArguments(uid, gid), true);
  }
  function lchownSync(path, uid, gid) {
    const text = toPath(path);
    sys("lchown", text, undefined, b.chown, text, ...chownArguments(uid, gid), false);
  }
  function fchownSync(fd, uid, gid) {
    checkFd(fd);
    sysFd("fchown", fd, b.fchown, fd, ...chownArguments(uid, gid));
  }

  function statfsSync(path, options) {
    const text = toPath(path);
    const stat = sys("statfs", text, undefined, b.statfs, text);
    const names = ["type", "bsize", "frsize", "blocks", "bfree", "bavail", "files", "ffree"];
    const bigint = options !== null && typeof options === "object" && options.bigint === true;
    const result = {};
    names.forEach((name, index) => { result[name] = bigint ? BigInt(stat[index]) : stat[index]; });
    return result;
  }

  function buffersOf(buffers) {
    if (!Array.isArray(buffers) || !buffers.every((buffer) => ArrayBuffer.isView(buffer))) throw typeError("Expected ArrayBufferView[]");
    return buffers;
  }
  function writevSync(fd, buffers, position) {
    checkFd(fd);
    let total = 0;
    for (const buffer of buffersOf(buffers)) {
      const view = new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
      const at = typeof position === "number" ? position + total : null;
      total += sysFd("write", fd, b.write, fd, view, at);
    }
    return total;
  }
  function readvSync(fd, buffers, position) {
    checkFd(fd);
    let total = 0;
    for (const buffer of buffersOf(buffers)) {
      const view = new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
      const at = typeof position === "number" ? position + total : null;
      const read = sysFd("read", fd, b.read, fd, view, 0, view.length, at);
      total += read;
      if (read < view.length) break;
    }
    return total;
  }

  function mkdtempDisposableSync(prefix, options) {
    const path = mkdtempSync(prefix, options);
    const disposable = {
      path,
      remove() { rmSync(path, { recursive: true, force: true }); },
    };
    Object.defineProperty(disposable, Symbol.dispose, { value: function () { this.remove(); }, writable: true, configurable: true });
    return disposable;
  }

  // `Dir`: lê as entradas de uma vez (como o bun) e as entrega uma a uma.
  const dirState = new WeakMap();
  function Dir(path) {
    dirState.set(this, { path, entries: null, closed: false });
  }
  function dirEntries(dir) {
    const state = dirState.get(dir);
    if (state.closed) throw codedError(Error, "Directory handle was closed", "ERR_DIR_CLOSED");
    if (state.entries === null) state.entries = readdirSync(state.path, { withFileTypes: true });
    return state;
  }
  Object.defineProperty(Dir.prototype, "readSync", { value: function readSync() { const state = dirEntries(this); return state.entries.shift() ?? null; }, writable: true, configurable: true });
  Object.defineProperty(Dir.prototype, "read", {
    value: function read(callback) {
      if (typeof callback === "function") {
        let entry = null;
        let failure = null;
        try { entry = this.readSync(); } catch (error) { failure = error; }
        setImmediate(() => (failure ? callback(failure) : callback(null, entry)));
        return undefined;
      }
      return promiseFor(() => this.readSync())();
    },
    writable: true, configurable: true,
  });
  Object.defineProperty(Dir.prototype, "close", {
    value: function close(callback) {
      if (typeof callback === "function") {
        let failure = null;
        try { this.closeSync(); } catch (error) { failure = error; }
        setImmediate(() => callback(failure));
        return undefined;
      }
      return promiseFor(() => this.closeSync())();
    },
    writable: true, configurable: true,
  });
  Object.defineProperty(Dir.prototype, "closeSync", { value: function closeSync() { const state = dirState.get(this); if (state.closed) throw codedError(Error, "Directory handle was closed", "ERR_DIR_CLOSED"); state.closed = true; }, writable: true, configurable: true });
  Object.defineProperty(Dir.prototype, "path", { get: function () { return dirState.get(this).path; }, enumerable: false, configurable: true });
  Object.defineProperty(Dir.prototype, Symbol.asyncIterator, {
    value: async function* () {
      try {
        for (let entry = this.readSync(); entry; entry = this.readSync()) yield entry;
      } finally {
        if (!dirState.get(this).closed) this.closeSync();
      }
    },
    writable: true, configurable: true,
  });
  function opendirSync(path) {
    const text = toPath(path);
    const stat = sys("opendir", text, undefined, b.stat, text, true);
    if ((stat[2] & S_IFMT) !== S_IFDIR) {
      // Medido no bun: o erro de `opendir` num arquivo traz as chaves em outra ordem (code, errno, syscall, path).
      const error = new Error("ENOTDIR: not a directory, opendir '" + text + "'");
      error.code = "ENOTDIR";
      error.errno = -20;
      error.syscall = "opendir";
      error.path = text;
      throw error;
    }
    return new Dir(text);
  }

  function openAsBlob(path, options) {
    const text = toPath(path);
    return promiseFor(() => new Blob([readFileSync(text)], { type: options && options.type ? String(options.type) : "" }))();
  }

  // ---- callbacks e promises ----
  // A operação roda na hora da chamada (a ordem dos efeitos é a das chamadas) e o resultado é entregue num
  // `setImmediate`, depois dos `nextTick` e das microtasks, como o bun entrega o de uma operação que acabou.
  const isFsFailure = (error) => error !== null && typeof error === "object" && (error.syscall !== undefined || error.name === "SystemError");

  // `arity` é o `length` do callback do bun: o argumento `arity - 1` (ou o anterior, se as opções foram omitidas) é o `cb`.
  function callbackFor(sync, adapt, arity) {
    return function (...args) {
      const callback = args.length ? args[args.length - 1] : undefined;
      if (typeof callback !== "function") {
        const given = args[arity - 1] !== undefined ? args[arity - 1] : args[arity - 2];
        throw typeError('The "cb" argument must be of type function. Received ' + describe(given));
      }
      args.pop();
      let result;
      let failure = null;
      try {
        result = sync(...args);
      } catch (error) {
        if (!isFsFailure(error)) throw error;
        failure = error;
      }
      setImmediate(() => {
        if (adapt) adapt(callback, failure, result, args);
        else if (failure) callback(failure);
        else callback(null, result);
      });
    };
  }

  function promiseFor(sync, adapt) {
    return function (...args) {
      let result;
      let failure;
      let failed = false;
      try {
        result = sync(...args.map((argument, index) => (index === 0 ? handleFd(argument) : argument)));
        if (adapt) result = adapt(result, args);
      } catch (error) {
        failed = true;
        failure = error;
        if (isFsFailure(error)) delete error.stack;
      }
      return new Promise((resolve, reject) => setImmediate(() => (failed ? reject(failure) : resolve(result))));
    };
  }

  function readArguments(args) {
    // `read(fd, [options], cb)` e `read(fd, cb)`: o buffer sai das opções.
    const [fd, second] = args;
    if (ArrayBuffer.isView(second)) return args;
    const options = second !== null && typeof second === "object" ? second : {};
    const buffer = options.buffer || Buffer.alloc(16384);
    return [fd, buffer, options.offset || 0, options.length === undefined ? buffer.byteLength - (options.offset || 0) : options.length, options.position === undefined ? null : options.position];
  }
  const readSyncLoose = (...args) => readSync(...readArguments(args));
  const withBuffer = (callback, failure, result, args) => (failure ? callback(failure) : callback(null, result, readArguments(args)[1]));
  const withWrittenBuffer = (callback, failure, result, args) => (failure ? callback(failure) : callback(null, result, args[1]));
  const existsCallback = (callback, failure, result) => callback(result);

  // O `FileHandle` do bun (`fs.promises.ts`): um `EventEmitter` com contagem de referências, que adia o fechamento
  // enquanto há operação em curso.
  const EventEmitter = b.builtin("node:events");
  const kFd = Symbol("kFd");
  const kRefs = Symbol("kRefs");
  const kClosePromise = Symbol("kClosePromise");
  const kCloseResolve = Symbol("kCloseResolve");
  const kCloseReject = Symbol("kCloseReject");
  const kRef = Symbol("kRef");
  const kUnref = Symbol("kUnref");
  const kFlag = Symbol("kFlag");

  function throwEBADFIfNecessary(syscall, fd) {
    if (fd === -1) {
      const error = new Error("Bad file descriptor");
      error.code = "EBADF";
      error.name = "SystemError";
      error.syscall = syscall;
      throw error;
    }
  }

  // Corpo comum dos métodos do `FileHandle`: confere o fd, segura uma referência e chama a versão de promise.
  async function withHandle(handle, syscall, run) {
    throwEBADFIfNecessary(syscall, handle[kFd]);
    try {
      handle[kRef]();
      return await run(handle[kFd]);
    } finally {
      handle[kUnref]();
    }
  }

  class FileHandle extends EventEmitter {
    constructor(fd, flag) {
      super();
      this[kFd] = fd ? fd : -1;
      this[kRefs] = 1;
      this[kClosePromise] = null;
      this[kFlag] = flag;
    }

    get fd() {
      return this[kFd];
    }

    async appendFile(data, options) {
      let encoding = "utf8";
      let flush = false;
      if (options == null || typeof options === "function") {
      } else if (typeof options === "string") {
        encoding = options;
      } else {
        encoding = options?.encoding ?? encoding;
        flush = options?.flush ?? flush;
      }
      return withHandle(this, "writeFile", (fd) => promises.writeFile(fd, data, { encoding, flush, flag: this[kFlag] }));
    }

    async chmod(mode) {
      return withHandle(this, "fchmod", (fd) => promises.fchmod(fd, mode));
    }

    async chown(uid, gid) {
      return withHandle(this, "fchown", (fd) => promises.fchown(fd, uid, gid));
    }

    async readv(buffers, position) {
      return withHandle(this, "readv", (fd) => promises.readv(fd, buffers, position));
    }

    async writev(buffers, position) {
      return withHandle(this, "writev", (fd) => promises.writev(fd, buffers, position));
    }

    async datasync() {
      return withHandle(this, "fdatasync", (fd) => promises.fdatasync(fd));
    }

    async sync() {
      return withHandle(this, "fsync", (fd) => promises.fsync(fd));
    }

    async read(bufferOrParams, offset, length, position) {
      throwEBADFIfNecessary("read", this[kFd]);
      let buffer = bufferOrParams;
      if (!ArrayBuffer.isView(buffer)) {
        if (bufferOrParams !== undefined && (typeof bufferOrParams !== "object" || Array.isArray(bufferOrParams))) {
          throw typeError('The "options" property must be of type object, got ' + (bufferOrParams === null ? "null" : typeof bufferOrParams));
        }
        ({ buffer = Buffer.alloc(16384), offset = 0, length = buffer.byteLength - offset, position = null } = bufferOrParams ?? {});
      }
      if (offset !== null && typeof offset === "object") {
        ({ offset = 0, length = buffer?.byteLength - offset, position = null } = offset);
      }
      if (offset == null) offset = 0;
      length ??= buffer?.byteLength - offset;
      return withHandle(this, "read", async (fd) => {
        const { bytesRead } = await promises.read(fd, buffer, offset, length, position);
        return { buffer, bytesRead };
      });
    }

    async readFile(options) {
      return withHandle(this, "readFile", (fd) => promises.readFile(fd, options));
    }

    async stat(options) {
      return withHandle(this, "fstat", (fd) => promises.fstat(fd, options));
    }

    async truncate(length = 0) {
      return withHandle(this, "ftruncate", (fd) => promises.ftruncate(fd, length));
    }

    async utimes(atime, mtime) {
      return withHandle(this, "futimes", (fd) => promises.futimes(fd, atime, mtime));
    }

    async write(buffer, offset, length, position) {
      throwEBADFIfNecessary("write", this[kFd]);
      if (buffer?.byteLength === 0) return { __proto__: null, bytesWritten: 0, buffer };
      if (ArrayBuffer.isView(buffer)) {
        if (typeof offset === "object") ({ offset = 0, length = buffer.byteLength - offset, position = null } = offset ?? {});
        if (offset == null) offset = 0;
        if (typeof length !== "number") length = buffer.byteLength - offset;
        if (typeof position !== "number") position = null;
      } else if (typeof buffer !== "string") {
        throw typeError('The "buffer" property must be of type string, Buffer, TypedArray, or DataView, got ' + typeof buffer);
      }
      return withHandle(this, "write", async (fd) => ({ buffer, bytesWritten: (await promises.write(fd, buffer, offset, length, position)).bytesWritten }));
    }

    async writeFile(data, options = "utf8") {
      let encoding = "utf8";
      if (options == null || typeof options === "function") {
      } else if (typeof options === "string") {
        encoding = options;
      } else {
        encoding = options?.encoding ?? encoding;
      }
      return withHandle(this, "writeFile", (fd) => promises.writeFile(fd, data, { encoding, flag: this[kFlag] }));
    }

    async close() {
      const fd = this[kFd];
      if (fd === -1) return Promise.resolve();
      if (this[kClosePromise]) return this[kClosePromise];
      if (--this[kRefs] === 0) {
        this[kFd] = -1;
        this[kClosePromise] = promises.close(fd).finally(() => {
          this[kClosePromise] = undefined;
        });
      } else {
        this[kClosePromise] = new Promise((resolve, reject) => {
          this[kCloseResolve] = resolve;
          this[kCloseReject] = reject;
        }).finally(() => {
          this[kClosePromise] = undefined;
          this[kCloseReject] = undefined;
          this[kCloseResolve] = undefined;
        });
      }
      this.emit("close");
      return this[kClosePromise];
    }

    async [Symbol.asyncDispose]() {
      return this.close();
    }

    createReadStream(options = {}) {
      throwEBADFIfNecessary("createReadStream", this[kFd]);
      return new (b.streams().ReadStream)(undefined, { highWaterMark: 64 * 1024, ...options, fd: this });
    }

    createWriteStream(options = {}) {
      throwEBADFIfNecessary("createWriteStream", this[kFd]);
      return new (b.streams().WriteStream)(undefined, { highWaterMark: 64 * 1024, ...options, fd: this });
    }

    [kRef]() {
      this[kRefs]++;
    }

    [kUnref]() {
      if (--this[kRefs] === 0) {
        const fd = this[kFd];
        this[kFd] = -1;
        (fd !== -1 ? promises.close(fd) : Promise.resolve()).then(this[kCloseResolve], this[kCloseReject]);
      }
    }
  }
  // `promiseFor` aceita o FileHandle no lugar do fd (o `fileHandleOrFdOrPath?.[kFd]` do bun).
  const handleFd = (argument) => (argument instanceof FileHandle ? argument[kFd] : argument);

  // Os pares (callback, promise) de cada operação do bun, pela versão síncrona.
  const operations = {
    access: accessSync, appendFile: appendFileSync, chmod: chmodSync, close: closeSync, copyFile: copyFileSync, fchmod: fchmodSync,
    fdatasync: fsyncSync, fstat: fstatSync, fsync: fsyncSync, ftruncate: ftruncateSync, futimes: futimesSync, link: linkSync, lstat: lstatSync,
    lutimes: lutimesSync, mkdir: mkdirSync, mkdtemp: mkdtempSync, open: openSync, readFile: readFileSync, readdir: readdirSync,
    readlink: readlinkSync, realpath: realpathSync, rename: renameSync, rm: rmSync, rmdir: rmdirSync, stat: statSync, symlink: symlinkSync,
    truncate: truncateSync, unlink: unlinkSync, utimes: utimesSync, writeFile: writeFileSync, chown: chownSync, fchown: fchownSync, lchown: lchownSync,
    statfs: statfsSync, opendir: opendirSync,
  };
  const callbacks = {};
  const promises = {};
  for (const name of Object.keys(operations)) {
    callbacks[name] = callbackFor(operations[name], undefined, ASYNC_LENGTHS[name][0]);
    promises[name] = promiseFor(operations[name]);
  }
  callbacks.writev = callbackFor(writevSync, (callback, failure, result, args) => (failure ? callback(failure) : callback(null, result, args[1])), 4);
  callbacks.readv = callbackFor(readvSync, (callback, failure, result, args) => (failure ? callback(failure) : callback(null, result, args[1])), 4);
  promises.mkdtempDisposable = promiseFor(mkdtempDisposableSync, (disposable) => {
    const remove = disposable.remove;
    disposable.remove = () => promiseFor(remove)();
    Object.defineProperty(disposable, Symbol.asyncDispose, { value: function () { return this.remove(); }, writable: true, configurable: true });
    return disposable;
  });
  promises.writev = promiseFor(writevSync, (bytesWritten, args) => ({ bytesWritten, buffers: args[1] }));
  promises.readv = promiseFor(readvSync, (bytesRead, args) => ({ bytesRead, buffers: args[1] }));
  callbacks.exists = callbackFor(existsSync, existsCallback, 2);
  callbacks.close = callbackFor(closeSync, (callback, failure) => (failure ? callback(failure) : callback(null)), 2);
  callbacks.read = callbackFor(readSyncLoose, withBuffer, 6);
  callbacks.write = callbackFor(writeSync, withWrittenBuffer, 6);
  promises.exists = promiseFor(existsSync);
  promises.read = promiseFor(readSyncLoose, (bytesRead, args) => ({ bytesRead, buffer: readArguments(args)[1] }));
  promises.write = promiseFor(writeSync, (bytesWritten, args) => ({ bytesWritten, buffer: args[1] }));
  promises.open = promiseFor(openSync, (fd, args) => new FileHandle(fd, args[1] === undefined ? "r" : args[1]));

  // `glob`: o `internal/fs/glob` do bun (com o minimatch do Node), gerado por `scripts/gen-node-fs-glob.js`.
  const globSync = (pattern, options) => b.asset("glob").globSync(pattern, options);
  callbacks.glob = (pattern, options, callback) => b.asset("glob").glob(pattern, options, callback);
  promises.glob = (pattern, options) => b.asset("glob").promisesGlob(pattern, options);

  // `watch`, `watchFile` e `unwatchFile`: o `internal/fs/watch` e `watchfile` do bun, gerados por
  // `scripts/gen-node-fs-watch.js`, sobre um observador por comparação de `stat` (o sysabi não tem inotify).
  const watch = (path, options, listener) => b.asset("watch").watch(path, options, listener);
  const watchFile = (path, options, listener) => b.asset("watch").watchFile(path, options, listener);
  const unwatchFile = (path, listener) => b.asset("watch").unwatchFile(path, listener);
  promises.watch = (filename, options) => b.asset("watch").promisesWatch(filename, options);

  // `cp`: o `internal/fs/cp` do bun, gerado (`scripts/gen-node-fs-cp.js`) e montado na primeira chamada.
  const cpSync = (src, dest, options) => b.asset("cp").cpSync(src, dest, options);
  promises.cp = (src, dest, options) => b.asset("cp").cp(src, dest, options);
  // A versão com callback valida na chamada (lança na hora) e entrega o resultado da promise; um `throw` dentro do
  // callback sai por `queueMicrotask`, que o leva a `uncaughtException` em vez de rejeitar a cadeia interna.
  callbacks.cp = function cp(src, dest, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== "function") throw typeError('The "cb" argument must be of type function. Received ' + describe(callback));
    const cpModule = b.asset("cp");
    options = cpModule.validateCpOptions(options);
    src = cpModule.getValidatedFsPath(src, "src");
    dest = cpModule.getValidatedFsPath(dest, "dest");
    promises.cp(src, dest, options).then(() => queueMicrotask(() => callback(null)), (error) => queueMicrotask(() => callback(error)));
  };

  const createReadStream = function createReadStream(path, options) {
    return new (b.fsExports().ReadStream)(path, options);
  };
  const createWriteStream = function createWriteStream(path, options) {
    return new (b.fsExports().WriteStream)(path, options);
  };
  const implemented = {
    ...callbacks,
    createReadStream, createWriteStream,
    appendFileSync, accessSync, chmodSync, closeSync, copyFileSync, existsSync, fchmodSync, fstatSync, fsyncSync, ftruncateSync, futimesSync,
    linkSync, lstatSync, lutimesSync, mkdirSync, mkdtempSync, openSync, readFileSync, readSync, readdirSync, readlinkSync, realpathSync,
    renameSync, rmSync, rmdirSync, statSync, symlinkSync, truncateSync, unlinkSync, utimesSync, writeFileSync, writeSync,
    fdatasyncSync: fsyncSync, _toUnixTimestamp: toUnixTimestamp, mkdtempDisposableSync, openAsBlob, opendirSync, statfsSync, writevSync, readvSync,
    chownSync, fchownSync, lchownSync, cpSync, globSync, watch, watchFile, unwatchFile,
  };
  // O `length` de cada função do bun (medido).
  const LENGTHS = {
    appendFileSync: 3, accessSync: 2, chmodSync: 2, closeSync: 1, copyFileSync: 3, existsSync: 1, fchmodSync: 2, fstatSync: 1, fsyncSync: 1,
    ftruncateSync: 1, futimesSync: 3, linkSync: 2, lstatSync: 1, lutimesSync: 3, mkdirSync: 2, mkdtempSync: 2, openSync: 3, readFileSync: 2,
    readSync: 5, readdirSync: 2, readlinkSync: 2, realpathSync: 2, renameSync: 2, rmSync: 2, rmdirSync: 2, statSync: 1, symlinkSync: 3,
    truncateSync: 2, unlinkSync: 1, utimesSync: 3, writeFileSync: 3, writeSync: 5, fdatasyncSync: 1, _toUnixTimestamp: 1, createReadStream: 2, createWriteStream: 2, chownSync: 3, fchownSync: 3, lchownSync: 3, statfsSync: 2, opendirSync: 2, writevSync: 3, readvSync: 3,
    mkdtempDisposableSync: 2, openAsBlob: 2, cpSync: 3, globSync: 2, watch: 3, watchFile: 3, unwatchFile: 2,
  };
  const setLength = (fn, value) => Object.defineProperty(fn, "length", { value, configurable: true });
  for (const name of Object.keys(LENGTHS)) setLength(implemented[name], LENGTHS[name]);
  for (const name of Object.keys(ASYNC_LENGTHS)) {
    if (callbacks[name]) setLength(callbacks[name], ASYNC_LENGTHS[name][0]);
    setLength(promises[name], ASYNC_LENGTHS[name][1]);
  }
  setLength(promises.watch, 1);
  // `fs.promises.lchmod` existe no bun e rejeita sempre (o `lchmod` só vale no macOS); `fs.lchmod` é `undefined`.
  promises.lchmod = async function lchmod(_path, _mode) {
    throw codedError(Error, "The lchmod() method is not implemented", "ERR_METHOD_NOT_IMPLEMENTED");
  };
  const streamData = { FileHandle, kRef, kUnref, kFd };
  const data = { Dir, lchmod: undefined, lchmodSync: undefined, Dirent, F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1, constants, Stats, ReadStream: undefined, WriteStream: undefined, FileReadStream: undefined, FileWriteStream: undefined, Utf8Stream: undefined };

  // A ordem das chaves de `Object.keys(require("fs"))` do bun 1.4.2.
  const ORDER = [
    "appendFile", "appendFileSync", "access", "accessSync", "chown", "chownSync", "chmod", "chmodSync", "close", "closeSync", "copyFile",
    "copyFileSync", "cp", "cpSync", "createReadStream", "createWriteStream", "exists", "existsSync", "fchown", "fchownSync", "fchmod",
    "fchmodSync", "fdatasync", "fdatasyncSync", "fstat", "fstatSync", "fsync", "fsyncSync", "ftruncate", "ftruncateSync", "futimes",
    "futimesSync", "glob", "globSync", "lchown", "lchownSync", "lchmod", "lchmodSync", "link", "linkSync", "lstat", "lstatSync", "lutimes",
    "lutimesSync", "mkdir", "mkdirSync", "mkdtemp", "mkdtempSync", "mkdtempDisposableSync", "open", "openSync", "read", "readFile",
    "readFileSync", "readSync", "readdir", "readdirSync", "readlink", "readlinkSync", "readv", "readvSync", "realpath", "realpathSync", "rename", "renameSync",
    "rm", "rmSync", "rmdir", "rmdirSync", "stat", "statfs", "statSync", "statfsSync", "symlink", "symlinkSync", "truncate", "truncateSync",
    "unlink", "unlinkSync", "unwatchFile", "utimes", "utimesSync", "watch", "watchFile", "write", "writeFile", "writeFileSync", "writeSync",
    "writev", "writevSync", "_toUnixTimestamp", "openAsBlob", "Dirent", "opendir", "opendirSync", "F_OK", "R_OK", "W_OK", "X_OK",
    "constants", "Dir", "Stats", "ReadStream", "WriteStream", "FileReadStream", "Utf8Stream", "FileWriteStream", "promises",
  ];

  const PROMISES_ORDER = [
    "access", "appendFile", "close", "copyFile", "cp", "exists", "chown", "chmod", "fchmod", "fchown", "fstat", "fsync", "fdatasync", "ftruncate",
    "futimes", "glob", "lchmod", "lchown", "link", "lstat", "mkdir", "mkdtemp", "mkdtempDisposable", "statfs", "open", "read", "write", "readdir",
    "readFile", "writeFile", "readlink", "realpath", "rename", "stat", "symlink", "truncate", "unlink", "utimes", "lutimes", "rm", "rmdir",
    "writev", "readv", "constants", "watch", "opendir",
  ];

  // Os exports na ordem do bun; `promises.X` são os do objeto `fs.promises` (o Rust os monta e põe em `fs.promises`).
  const table = { $data: { v: streamData } };
  for (const name of ORDER) {
    if (name === "promises") {
      table.promises = { v: undefined };
      for (const key of PROMISES_ORDER) {
        if (key === "constants") table["promises.constants"] = { v: constants };
        else if (key in promises) table["promises." + key] = promises[key];
      }
    } else if (name in implemented) {
      table[name] = implemented[name];
    } else if (name in data) {
      table[name] = { v: data[name] };
    }
  }
  return table;
})
