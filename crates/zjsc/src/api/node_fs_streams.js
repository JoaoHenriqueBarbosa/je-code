// GERADO por `scripts/gen-node-fs-streams.js` a partir de `src/js/internal/fs/streams.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { codes, validators, toClass } = host.internal;
  const assert = () => {};
  const isPromise = value => value instanceof Promise;
  const require = name => (name === "internal/validators" ? validators : name.startsWith("internal/") ? host.requireInternal(name) : host.requireBuiltin(name));
  const { Readable, Writable, finished } = require("node:stream");
  const fs = require("node:fs");
  const { read, write, fsync, writev } = fs;
  const { FileHandle, kRef, kUnref, kFd } = host.fsData();
  const { validateInteger, validateInt32, validateFunction } = require("internal/validators");
  const kIsPerformingIO = Symbol("kIsPerformingIO");
  const kIoDone = Symbol("kIoDone");
  const kWriteStreamFastPath = Symbol("kWriteStreamFastPath");
  const kFs = Symbol("kFs");
  const {
    read: fileHandlePrototypeRead,
    write: fileHandlePrototypeWrite,
    fsync: fileHandlePrototypeFsync,
    writev: fileHandlePrototypeWritev
  } = FileHandle.prototype;
  const fileHandleStreamFs = (fh) => ({
    read: fh.read === fileHandlePrototypeRead ? read : function(fd, buf, offset, length, pos, cb) {
      return fh.read(buf, offset, length, pos).then(({ bytesRead, buffer }) => cb(null, bytesRead, buffer), (err) => cb(err, 0, buf));
    },
    write: fh.write === fileHandlePrototypeWrite ? write : function(fd, buffer, offset, length, position, cb) {
      return fh.write(buffer, offset, length, position).then(({ bytesWritten, buffer }) => cb(null, bytesWritten, buffer), (err) => cb(err, 0, buffer));
    },
    writev: fh.writev === fileHandlePrototypeWritev ? writev : undefined,
    fsync: fh.sync === fileHandlePrototypeFsync ? fsync : function(fd, cb) {
      return fh.sync().then(() => cb(), cb);
    },
    close: streamFileHandleClose.bind(fh)
  });
  function streamFileHandleClose(fd, cb) {
    assert(this[kFd] == fd, "fd mismatch");
    this[kUnref]();
    this.close().then(() => cb(), cb);
  }
  function getValidatedPath(p) {
    if (p instanceof URL)
      return Bun.fileURLToPath(p);
    if (typeof p !== "string")
      throw codes.INVALID_ARG_TYPE("path", "string or URL", p);
    return require("node:path").resolve(p);
  }
  function copyObject(source) {
    const target = {};
    for (const key in source)
      target[key] = source[key];
    return target;
  }
  function getStreamOptions(options, defaultOptions = {}) {
    if (options == null || typeof options === "function") {
      return defaultOptions;
    }
    if (typeof options === "string") {
      if (options !== "buffer" && !Buffer.isEncoding(options)) {
        throw codes.INVALID_ARG_VALUE("encoding", options, "is invalid encoding");
      }
      return { encoding: options };
    } else if (typeof options !== "object") {
      throw codes.INVALID_ARG_TYPE("options", ["string", "Object"], options);
    }
    let { encoding, signal = true } = options;
    if (encoding && encoding !== "buffer" && !Buffer.isEncoding(encoding)) {
      throw codes.INVALID_ARG_VALUE("encoding", encoding, "is invalid encoding");
    }
    if (signal !== true && !signal) {
      throw codes.INVALID_ARG_TYPE("signal", "AbortSignal", signal);
    }
    return options;
  }
  function ReadStream(path, options) {
    if (!(this instanceof ReadStream)) {
      return new ReadStream(path, options);
    }
    options = copyObject(getStreamOptions(options));
    options.decodeStrings = true;
    let { fd, autoClose, fs: customFs, start, end = 1 / 0 } = options;
    if (fd == null) {
      this[kFs] = customFs || fs;
      this.fd = null;
      this.path = getValidatedPath(path);
      const { flags, mode } = options;
      this.flags = flags === undefined ? "r" : flags;
      this.mode = mode === undefined ? 438 : mode;
      if (customFs) {
        validateFunction(customFs.open, "options.fs.open");
      }
    } else if (typeof options.fd === "number") {
      if (Object.is(fd, -0)) {
        fd = 0;
      } else {
        validateInt32(fd, "fd", 0, 2147483647);
      }
      this.fd = fd;
      this[kFs] = customFs || fs;
    } else if (typeof fd === "object" && fd instanceof FileHandle) {
      if (options.fs) {
        throw codes.METHOD_NOT_IMPLEMENTED("fs.FileHandle with custom fs operations");
      }
      this[kFs] = fileHandleStreamFs(fd);
      this.fd = fd[kFd];
      fd[kRef]();
      fd.on("close", this.close.bind(this));
    } else {
      throw codes.INVALID_ARG_TYPE("options.fd", "number or FileHandle", fd);
    }
    if (customFs) {
      validateFunction(customFs.read, "options.fs.read");
    }
    assert(this[kFs], "fs implementation was not assigned");
    if ((options.autoDestroy = autoClose === undefined ? true : autoClose) && customFs) {
      validateFunction(customFs.close, "options.fs.close");
    }
    this.start = start;
    this.end = end;
    this.pos = undefined;
    this.bytesRead = 0;
    if (start !== undefined) {
      validateInteger(start, "start", 0);
      this.pos = start;
    }
    if (end === undefined) {
      end = 1 / 0;
    } else if (end !== 1 / 0) {
      validateInteger(end, "end", 0);
      if (start !== undefined && start > end) {
        throw codes.OUT_OF_RANGE("start", `<= "end" (here: ${end})`, start);
      }
    }
    Readable.call(this, options);
    return this;
  }
  toClass(ReadStream, "ReadStream", Readable);
  const readStreamPrototype = ReadStream.prototype;
  Object.defineProperty(readStreamPrototype, "autoClose", {
    get() {
      return this._readableState.autoDestroy;
    },
    set(val) {
      this._readableState.autoDestroy = val;
    }
  });
  const streamNoop = function open() {};
  function streamConstruct(callback) {
    const { fd } = this;
    if (typeof fd === "number") {
      callback();
      return;
    }
    const fastPath = this[kWriteStreamFastPath];
    if (this.open !== streamNoop) {
      const orgEmit = this.emit;
      this.emit = function(...args) {
        if (args[0] === "open") {
          this.emit = orgEmit;
          callback();
          orgEmit.apply(this, args);
        } else if (args[0] === "error") {
          this.emit = orgEmit;
          callback(args[1]);
        } else {
          orgEmit.apply(this, args);
        }
      };
      this.open();
    } else {
      if (fastPath) {
        callback();
        this.emit("open", this.fd);
        this.emit("ready");
        return;
      }
      this[kFs].open(this.path, this.flags, this.mode, (err, fd) => {
        if (err) {
          callback(err);
        } else {
          this.fd = fd;
          callback();
          this.emit("open", this.fd);
          this.emit("ready");
        }
      });
    }
  }
  readStreamPrototype.open = streamNoop;
  readStreamPrototype._construct = streamConstruct;
  readStreamPrototype._read = function(n) {
    n = this.pos !== undefined ? Math.min(this.end - this.pos + 1, n) : Math.min(this.end - this.bytesRead + 1, n);
    if (n <= 0) {
      this.push(null);
      return;
    }
    const buf = Buffer.allocUnsafeSlow(n);
    this[kIsPerformingIO] = true;
    this[kFs].read(this.fd, buf, 0, n, this.pos, (er, bytesRead, buf) => {
      this[kIsPerformingIO] = false;
      if (this.destroyed) {
        this.emit(kIoDone, er);
        return;
      }
      if (er) {
        require("internal/streams/destroy").errorOrDestroy(this, er);
      } else if (bytesRead > 0) {
        if (this.pos !== undefined) {
          this.pos += bytesRead;
        }
        this.bytesRead += bytesRead;
        if (bytesRead !== buf.length) {
          const dst = Buffer.allocUnsafeSlow(bytesRead);
          buf.copy(dst, 0, 0, bytesRead);
          buf = dst;
        }
        this.push(buf);
      } else {
        this.push(null);
      }
    });
  };
  readStreamPrototype._destroy = function(err, cb) {
    if (this[kIsPerformingIO]) {
      this.once(kIoDone, (er) => close(this, err || er, cb));
    } else {
      close(this, err, cb);
    }
  };
  readStreamPrototype.close = function(cb) {
    if (typeof cb === "function")
      finished(this, cb);
    this.destroy();
  };
  Object.defineProperty(readStreamPrototype, "pending", {
    get() {
      return this.fd == null;
    },
    configurable: true
  });
  function close(stream, err, cb) {
    const fastPath = stream[kWriteStreamFastPath];
    if (fastPath && fastPath !== true) {
      stream.fd = null;
      const maybePromise = fastPath.end(err);
      thenIfPromise(maybePromise, () => {
        cb(err);
      });
      return;
    }
    if (!stream.fd) {
      cb(err);
    } else if (stream.flush) {
      stream[kFs].fsync(stream.fd, (flushErr) => {
        closeAfterSync(stream, err || flushErr, cb);
      });
    } else {
      closeAfterSync(stream, err, cb);
    }
  }
  function closeAfterSync(stream, err, cb) {
    stream[kFs].close(stream.fd, (er) => {
      cb(er || err);
    });
    stream.fd = null;
  }
  function WriteStream(path, options) {
    if (!(this instanceof WriteStream)) {
      return new WriteStream(path, options);
    }
    let fastPath = options?.$fastPath;
    options = copyObject(getStreamOptions(options));
    options.decodeStrings = true;
    let { fd, autoClose, fs: customFs, start, flush } = options;
    if (fd == null) {
      this[kFs] = customFs || fs;
      this.fd = null;
      this.path = fastPath ? path : getValidatedPath(path);
      const { flags, mode } = options;
      this.flags = flags === undefined ? "w" : flags;
      this.mode = mode === undefined ? 438 : mode;
      if (customFs) {
        validateFunction(customFs.open, "options.fs.open");
      }
    } else if (typeof options.fd === "number") {
      if (Object.is(fd, -0)) {
        fd = 0;
      } else {
        validateInt32(fd, "fd", 0, 2147483647);
      }
      this.fd = fd;
      this[kFs] = customFs || fs;
    } else if (typeof fd === "object" && fd instanceof FileHandle) {
      if (options.fs) {
        throw codes.METHOD_NOT_IMPLEMENTED("fs.FileHandle with custom fs operations");
      }
      this[kFs] = customFs = fileHandleStreamFs(fd);
      fd[kRef]();
      fd.on("close", this.close.bind(this));
      this.fd = fd = fd[kFd];
    } else {
      throw codes.INVALID_ARG_TYPE("options.fd", "number or FileHandle", fd);
    }
    const autoDestroy = autoClose = options.autoDestroy = autoClose === undefined ? true : autoClose;
    if (customFs) {
      const { write, writev, close, fsync } = customFs;
      if (write)
        validateFunction(write, "options.fs.write");
      if (writev)
        validateFunction(writev, "options.fs.writev");
      if (autoDestroy)
        validateFunction(close, "options.fs.close");
      if (flush)
        validateFunction(fsync, "options.fs.fsync");
      if (!write && !writev) {
        throw codes.INVALID_ARG_TYPE("options.fs.write", "function", write);
      }
      if (!write)
        this._write = null;
      if (!writev)
        this._writev = null;
    } else {
      this._writev = undefined;
      assert(this[kFs].write, "assuming user does not delete fs.write!");
    }
    if (flush == null) {
      this.flush = false;
    } else {
      if (typeof flush !== "boolean")
        throw codes.INVALID_ARG_TYPE("options.flush", "boolean", flush);
      this.flush = flush;
    }
    this.start = start;
    this.pos = undefined;
    this.bytesWritten = 0;
    this[kIsPerformingIO] = false;
    if (start !== undefined) {
      validateInteger(start, "start", 0);
      this.pos = start;
    }
    let fastWriter;
    if (fastPath && fd != null) {
      try {
        fastWriter = Bun.file(fd).writer();
      } catch {
        fastPath = false;
      }
    }
    if (fastPath) {
      this[kWriteStreamFastPath] = fd != null ? fastWriter : true;
      this._write = underscoreWriteFast;
      this._writev = undefined;
      this.write = writeFast;
      if (fd != null) {
        this._construct = undefined;
      }
    }
    Writable.call(this, options);
    const encoding = options.encoding;
    if (encoding) {
      this.setDefaultEncoding(encoding);
    }
    return this;
  }
  toClass(WriteStream, "WriteStream", Writable);
  const writeStreamPrototype = WriteStream.prototype;
  writeStreamPrototype.open = streamNoop;
  writeStreamPrototype._construct = streamConstruct;
  function writeAll(data, size, pos, cb, retries = 0) {
    this[kFs].write(this.fd, data, 0, size, pos, (er, bytesWritten, buffer) => {
      if (er?.code === "EAGAIN") {
        er = null;
        bytesWritten = 0;
      }
      if (this.destroyed || er) {
        return cb(er || codes.STREAM_DESTROYED("write"));
      }
      this.bytesWritten += bytesWritten;
      retries = bytesWritten ? 0 : retries + 1;
      size -= bytesWritten;
      if (pos !== undefined)
        pos += bytesWritten;
      if (retries > 5) {
        cb(new Error("write failed"));
      } else if (size) {
        writeAll.call(this, buffer.slice(bytesWritten), size, pos, cb, retries);
      } else {
        cb();
      }
    });
  }
  function writevAll(chunks, size, pos, cb, retries = 0) {
    this[kFs].writev(this.fd, chunks, this.pos, (er, bytesWritten, buffers) => {
      if (er?.code === "EAGAIN") {
        er = null;
        bytesWritten = 0;
      }
      if (this.destroyed || er) {
        return cb(er || codes.STREAM_DESTROYED("writev"));
      }
      this.bytesWritten += bytesWritten;
      retries = bytesWritten ? 0 : retries + 1;
      size -= bytesWritten;
      pos += bytesWritten;
      if (retries > 5) {
        cb(new Error("writev failed"));
      } else if (size) {
        writevAll.call(this, [Buffer.concat(buffers).slice(bytesWritten)], size, pos, cb, retries);
      } else {
        cb();
      }
    });
  }
  function _write(data, encoding, cb) {
    const fileSink = this[kWriteStreamFastPath];
    if (fileSink && fileSink !== true) {
      const maybePromise = fileSink.write(data);
      if (isPromise(maybePromise)) {
        maybePromise.then(() => {
          this.emit("drain");
          cb(null);
        }).catch(cb);
        return false;
      } else {
        cb(null);
        return true;
      }
    } else {
      this[kIsPerformingIO] = true;
      writeAll.call(this, data, data.length, this.pos, (er) => {
        this[kIsPerformingIO] = false;
        if (this.destroyed) {
          cb(er);
          return this.emit(kIoDone, er);
        }
        cb(er);
      });
      if (this.pos !== undefined)
        this.pos += data.length;
    }
  }
  writeStreamPrototype._write = _write;
  function underscoreWriteFast(data, encoding, cb) {
    let fileSink = this[kWriteStreamFastPath];
    if (!fileSink) {
      this._write = _write;
      return this._write(data, encoding, cb);
    }
    try {
      if (fileSink === true) {
        fileSink = this[kWriteStreamFastPath] = Bun.file(this.path).writer();
        this.fd = fileSink._getFd();
      }
      const maybePromise = fileSink.write(data);
      if (isPromise(maybePromise)) {
        maybePromise.then(() => {
          if (cb)
            cb(null);
          this.emit("drain");
        }, (err) => {
          if (cb)
            cb(err);
          require("internal/streams/destroy").errorOrDestroy(this, err);
        });
        return false;
      } else {
        if (cb)
          process.nextTick(cb, null);
        return true;
      }
    } catch (e) {
      if (cb)
        process.nextTick(cb, e);
      require("internal/streams/destroy").errorOrDestroy(this, e, true);
      return false;
    }
  }
  const writablePrototypeWrite = Writable.prototype.write;
  const kWriteMonkeyPatchDefense = Symbol("!");
  function writeFast(data, encoding, cb) {
    if (this[kWriteMonkeyPatchDefense])
      return writablePrototypeWrite.call(this, data, encoding, cb);
    const state = this._writableState;
    if (state !== undefined && (state.ending || state.destroyed)) {
      return writablePrototypeWrite.call(this, data, encoding, cb);
    }
    if (typeof encoding === "function") {
      cb = encoding;
      encoding = undefined;
    }
    if (typeof cb !== "function") {
      cb = streamNoop;
    }
    const fileSink = this[kWriteStreamFastPath];
    if (fileSink && fileSink !== true) {
      const maybePromise = fileSink.write(data);
      if (isPromise(maybePromise)) {
        maybePromise.then(() => {
          this.emit("drain");
          cb(null);
        }, (err) => {
          cb(err);
          require("internal/streams/destroy").errorOrDestroy(this, err);
        });
        return false;
      } else {
        cb(null);
        return true;
      }
    } else {
      const result = this._write(data, encoding, cb);
      if (this.write === writeFast) {
        this.write = writablePrototypeWrite;
      } else {
        this[kWriteMonkeyPatchDefense] = true;
      }
      return result;
    }
  }
  writeStreamPrototype._writev = function(data, cb) {
    const len = data.length;
    const chunks = new Array(len);
    let size = 0;
    for (let i = 0;i < len; i++) {
      const chunk = data[i].chunk;
      chunks[i] = chunk;
      size += chunk.length;
    }
    const fileSink = this[kWriteStreamFastPath];
    if (fileSink && fileSink !== true) {
      const maybePromise = fileSink.write(Buffer.concat(chunks));
      if (isPromise(maybePromise)) {
        maybePromise.then(() => {
          this.emit("drain");
          cb(null);
        }).catch(cb);
        return false;
      } else {
        cb(null);
        return true;
      }
    } else {
      this[kIsPerformingIO] = true;
      writevAll.call(this, chunks, size, this.pos, (er) => {
        this[kIsPerformingIO] = false;
        if (this.destroyed) {
          cb(er);
          return this.emit(kIoDone, er);
        }
        cb(er);
      });
      if (this.pos !== undefined)
        this.pos += size;
    }
  };
  writeStreamPrototype._destroy = function(err, cb) {
    const sink = this[kWriteStreamFastPath];
    if (sink && sink !== true) {
      const end = sink.end(err);
      if (isPromise(end)) {
        end.then(() => cb(err), cb);
        return;
      }
    }
    if (this[kIsPerformingIO]) {
      this.once(kIoDone, (er) => close(this, err || er, cb));
    } else {
      close(this, err, cb);
    }
  };
  writeStreamPrototype.close = function(cb) {
    if (cb) {
      if (this.closed) {
        process.nextTick(cb);
        return;
      }
      this.on("close", cb);
    }
    if (!this.autoClose) {
      this.on("finish", this.destroy);
    }
    this.end();
  };
  writeStreamPrototype.destroySoon = writeStreamPrototype.end;
  Object.defineProperty(writeStreamPrototype, "autoClose", {
    get() {
      return this._writableState.autoDestroy;
    },
    set(val) {
      this._writableState.autoDestroy = val;
    }
  });
  Object.defineProperty(writeStreamPrototype, "pending", {
    get() {
      return this.fd === null;
    },
    configurable: true
  });
  function thenIfPromise(maybePromise, cb) {
    assert(typeof cb === "function", "cb is not a function");
    if (isPromise(maybePromise)) {
      maybePromise.then(() => cb(null), cb);
    } else {
      process.nextTick(cb, null);
    }
  }
  function writableFromFileSink(fileSink) {
    assert(typeof fileSink === "object", "fileSink is not an object");
    assert(typeof fileSink.write === "function", "fileSink.write is not a function");
    assert(typeof fileSink.end === "function", "fileSink.end is not a function");
    const w = new WriteStream("", { $fastPath: true });
    assert(w[kWriteStreamFastPath] === true, "fast path not enabled");
    w[kWriteStreamFastPath] = fileSink;
    w.path = undefined;
    return w;
  }
  return {
    ReadStream,
    WriteStream,
    kWriteStreamFastPath,
    writableFromFileSink
  };

})
