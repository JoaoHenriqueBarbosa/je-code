// GERADO por `scripts/gen-node-fs-cp.js` a partir de `src/js/internal/fs/cp-sync.ts` e `cp.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { codes, validators } = host.internal;
  const isPromise = value => value instanceof Promise;
  let cpSyncModule;
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/fs/cp-sync") return cpSyncModule;
    return host.requireBuiltin(name);
  };
  cpSyncModule = (function () {
    const { validateObject, validateBoolean, validateFunction, validateInteger } = require("internal/validators");
    const {
      chmodSync,
      copyFileSync,
      existsSync,
      lstatSync,
      mkdirSync,
      readdirSync,
      readlinkSync,
      statSync,
      symlinkSync,
      unlinkSync,
      utimesSync
    } = require("node:fs");
    const { dirname, isAbsolute, join, parse, resolve, sep } = require("node:path");
    const { EEXIST, EISDIR, EINVAL, ENOTDIR } = host.requireBuiltin("node:os").constants.errno;
    const ArrayPrototypeEvery = Array.prototype.every;
    const ArrayPrototypeFilter = Array.prototype.filter;
    const StringPrototypeSplit = String.prototype.split;
    const kMaxCopyMode = 7;
    const defaultCpOptions = {
      dereference: false,
      errorOnExist: false,
      filter: undefined,
      force: true,
      preserveTimestamps: false,
      recursive: false,
      verbatimSymlinks: false,
      mode: 0
    };
    function decorateSystemError(err, prefix, context) {
      const { syscall, code, message: ctxMessage, path, dest, errno } = context;
      let message = `${prefix}: ${syscall} returned ${code} (${ctxMessage})`;
      if (path !== undefined)
        message += ` ${path}`;
      if (dest !== undefined)
        message += ` => ${dest}`;
      err.message = message;
      err.name = "SystemError";
      err.info = context;
      err.errno = errno;
      err.syscall = syscall;
      if (path !== undefined)
        err.path = path;
      if (dest !== undefined)
        err.dest = dest;
      return err;
    }
    function fsCpDirToNonDirError(context) {
      return decorateSystemError(codes.FS_CP_DIR_TO_NON_DIR(context.message), "Cannot overwrite non-directory with directory", context);
    }
    function fsCpEExistError(context) {
      return decorateSystemError(codes.FS_CP_EEXIST(context.message), "Target already exists", context);
    }
    function fsCpEinvalError(context) {
      return decorateSystemError(codes.FS_CP_EINVAL(context.message), "Invalid src or dest", context);
    }
    function fsCpFifoPipeError(context) {
      return decorateSystemError(codes.FS_CP_FIFO_PIPE(context.message), "Cannot copy a FIFO pipe", context);
    }
    function fsCpNonDirToDirError(context) {
      return decorateSystemError(codes.FS_CP_NON_DIR_TO_DIR(context.message), "Cannot overwrite directory with non-directory", context);
    }
    function fsCpSocketError(context) {
      return decorateSystemError(codes.FS_CP_SOCKET(context.message), "Cannot copy a socket file", context);
    }
    function fsCpSymlinkToSubdirectoryError(context) {
      return decorateSystemError(codes.FS_CP_SYMLINK_TO_SUBDIRECTORY(context.message), "Cannot overwrite symlink in subdirectory of self", context);
    }
    function fsCpUnknownError(context) {
      return decorateSystemError(codes.FS_CP_UNKNOWN(context.message), "Cannot copy an unknown file type", context);
    }
    function fsEisdirError(context) {
      return decorateSystemError(codes.FS_EISDIR(context.message), "Path is a directory", context);
    }
    function getValidMode(mode) {
      if (mode == null) {
        return 0;
      }
      validateInteger(mode, "mode", 0, kMaxCopyMode);
      return mode;
    }
    const kValidatedCpOptions = Symbol("kValidatedCpOptions");
    function validateCpOptions(options) {
      if (options?.[kValidatedCpOptions])
        return options;
      if (options === undefined) {
        options = { ...defaultCpOptions };
        options[kValidatedCpOptions] = true;
        return options;
      }
      validateObject(options, "options");
      options = { ...defaultCpOptions, ...options };
      validateBoolean(options.dereference, "options.dereference");
      validateBoolean(options.errorOnExist, "options.errorOnExist");
      validateBoolean(options.force, "options.force");
      validateBoolean(options.preserveTimestamps, "options.preserveTimestamps");
      validateBoolean(options.recursive, "options.recursive");
      validateBoolean(options.verbatimSymlinks, "options.verbatimSymlinks");
      options.mode = getValidMode(options.mode);
      if (options.dereference === true && options.verbatimSymlinks === true) {
        throw codes.INCOMPATIBLE_OPTION_PAIR('Option "dereference" cannot be used in combination with option "verbatimSymlinks"');
      }
      const { filter } = options;
      if (filter !== undefined) {
        validateFunction(filter, "options.filter");
      }
      options[kValidatedCpOptions] = true;
      return options;
    }
    function areIdentical(srcStat, destStat) {
      return destStat.ino && destStat.dev && destStat.ino === srcStat.ino && destStat.dev === srcStat.dev;
    }
    const normalizePathToArray = (path) => ArrayPrototypeFilter.call(StringPrototypeSplit.call(resolve(path), sep), Boolean);
    function isSrcSubdir(src, dest) {
      const srcArr = normalizePathToArray(src);
      const destArr = normalizePathToArray(dest);
      return ArrayPrototypeEvery.call(srcArr, (cur, i) => destArr[i] === cur);
    }
    function checkPathsSync(src, dest, opts) {
      if (opts.filter) {
        const shouldCopy = opts.filter(src, dest);
        if (isPromise(shouldCopy)) {
          throw codes.INVALID_RETURN_VALUE("boolean", "filter", shouldCopy);
        }
        if (!shouldCopy)
          return { __proto__: null, skipped: true };
      }
      const { srcStat, destStat } = getStatsSync(src, dest, opts);
      if (destStat) {
        if (areIdentical(srcStat, destStat)) {
          throw fsCpEinvalError({
            message: "src and dest cannot be the same",
            path: dest,
            syscall: "cp",
            errno: EINVAL,
            code: "EINVAL"
          });
        }
        if (srcStat.isDirectory() && !destStat.isDirectory()) {
          throw fsCpDirToNonDirError({
            message: `cannot overwrite non-directory ${dest} with directory ${src}`,
            path: dest,
            syscall: "cp",
            errno: EISDIR,
            code: "EISDIR"
          });
        }
        if (!srcStat.isDirectory() && destStat.isDirectory()) {
          throw fsCpNonDirToDirError({
            message: `cannot overwrite directory ${dest} with non-directory ${src}`,
            path: dest,
            syscall: "cp",
            errno: ENOTDIR,
            code: "ENOTDIR"
          });
        }
      }
      if (srcStat.isDirectory() && isSrcSubdir(src, dest)) {
        throw fsCpEinvalError({
          message: `cannot copy ${src} to a subdirectory of self ${dest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      return { __proto__: null, srcStat, destStat, skipped: false };
    }
    function getStatsSync(src, dest, opts) {
      let destStat;
      const statFunc = opts.dereference ? (file) => statSync(file, { bigint: true }) : (file) => lstatSync(file, { bigint: true });
      const srcStat = statFunc(src);
      try {
        destStat = statFunc(dest);
      } catch (err) {
        if (err.code === "ENOENT")
          return { srcStat, destStat: null };
        throw err;
      }
      return { srcStat, destStat };
    }
    function checkParentPathsSync(src, srcStat, dest) {
      const srcParent = resolve(dirname(src));
      const destParent = resolve(dirname(dest));
      if (destParent === srcParent || destParent === parse(destParent).root)
        return;
      let destStat;
      try {
        destStat = statSync(destParent, { bigint: true });
      } catch (err) {
        if (err.code === "ENOENT")
          return;
        throw err;
      }
      if (areIdentical(srcStat, destStat)) {
        throw fsCpEinvalError({
          message: `cannot copy ${src} to a subdirectory of self ${dest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      return checkParentPathsSync(src, srcStat, destParent);
    }
    function treeContainsOnlyFilesAndDirsSync(root) {
      const stack = [root];
      while (stack.length) {
        const dir = stack.pop();
        let entries;
        try {
          entries = readdirSync(dir, { withFileTypes: true });
        } catch {
          return false;
        }
        for (let i = 0;i < entries.length; i++) {
          const entry = entries[i];
          if (entry.isDirectory()) {
            stack.push(join(dir, entry.name));
          } else if (!entry.isFile()) {
            return false;
          }
        }
      }
      return true;
    }
    function tryNativeFastPathSync(src, dest, opts) {
      const checked = checkPathsSync(src, dest, opts);
      const { srcStat, destStat } = checked;
      checkParentPathsSync(src, srcStat, dest);
      if (srcStat.isDirectory() && !opts.recursive) {
        throw fsEisdirError({
          message: `${src} is a directory (not copied)`,
          path: src,
          syscall: "cp",
          errno: EISDIR,
          code: "EISDIR"
        });
      }
      if (srcStat.isDirectory()) {
        return {
          ok: process.platform === "darwin" && !destStat && treeContainsOnlyFilesAndDirsSync(src),
          checked
        };
      }
      return { ok: srcStat.isFile() && (!destStat || destStat.isFile()), checked };
    }
    function cpSyncFn(src, dest, opts, checked) {
      const { srcStat, destStat, skipped } = checked ?? checkPathsSync(src, dest, opts);
      if (skipped)
        return;
      if (checked === undefined)
        checkParentPathsSync(src, srcStat, dest);
      return checkParentDir(destStat, src, dest, opts);
    }
    function checkParentDir(destStat, src, dest, opts) {
      const destParent = dirname(dest);
      if (!existsSync(destParent))
        mkdirSync(destParent, { recursive: true });
      return getStats(destStat, src, dest, opts);
    }
    function getStats(destStat, src, dest, opts) {
      const statSyncFn = opts.dereference ? statSync : lstatSync;
      const srcStat = statSyncFn(src);
      if (srcStat.isDirectory() && opts.recursive) {
        return onDir(srcStat, destStat, src, dest, opts);
      } else if (srcStat.isDirectory()) {
        throw fsEisdirError({
          message: `${src} is a directory (not copied)`,
          path: src,
          syscall: "cp",
          errno: EISDIR,
          code: "EISDIR"
        });
      } else if (srcStat.isFile() || srcStat.isCharacterDevice() || srcStat.isBlockDevice()) {
        return onFile(srcStat, destStat, src, dest, opts);
      } else if (srcStat.isSymbolicLink()) {
        return onLink(destStat, src, dest, opts);
      } else if (srcStat.isSocket()) {
        throw fsCpSocketError({
          message: `cannot copy a socket file: ${dest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      } else if (srcStat.isFIFO()) {
        throw fsCpFifoPipeError({
          message: `cannot copy a FIFO pipe: ${dest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      throw fsCpUnknownError({
        message: `cannot copy an unknown file type: ${dest}`,
        path: dest,
        syscall: "cp",
        errno: EINVAL,
        code: "EINVAL"
      });
    }
    function onFile(srcStat, destStat, src, dest, opts) {
      if (!destStat)
        return copyFile(srcStat, src, dest, opts);
      return mayCopyFile(srcStat, src, dest, opts);
    }
    function mayCopyFile(srcStat, src, dest, opts) {
      if (opts.force) {
        unlinkSync(dest);
        return copyFile(srcStat, src, dest, opts);
      } else if (opts.errorOnExist) {
        throw fsCpEExistError({
          message: `${dest} already exists`,
          path: dest,
          syscall: "cp",
          errno: EEXIST,
          code: "EEXIST"
        });
      }
    }
    function copyFile(srcStat, src, dest, opts) {
      copyFileSync(src, dest, opts.mode);
      if (opts.preserveTimestamps)
        handleTimestamps(srcStat.mode, src, dest);
      return setDestMode(dest, srcStat.mode);
    }
    function handleTimestamps(srcMode, src, dest) {
      if (fileIsNotWritable(srcMode))
        makeFileWritable(dest, srcMode);
      return setDestTimestamps(src, dest);
    }
    function fileIsNotWritable(srcMode) {
      return (srcMode & 128) === 0;
    }
    function makeFileWritable(dest, srcMode) {
      return setDestMode(dest, srcMode | 128);
    }
    function setDestMode(dest, srcMode) {
      return chmodSync(dest, srcMode);
    }
    function setDestTimestamps(src, dest) {
      const updatedSrcStat = statSync(src);
      return utimesSync(dest, updatedSrcStat.atime, updatedSrcStat.mtime);
    }
    function onDir(srcStat, destStat, src, dest, opts) {
      if (!destStat)
        return mkDirAndCopy(srcStat.mode, src, dest, opts);
      if (opts.errorOnExist && !opts.force) {
        throw fsCpEExistError({
          message: `${dest} already exists`,
          path: dest,
          syscall: "cp",
          errno: EEXIST,
          code: "EEXIST"
        });
      }
      return copyDir(src, dest, opts);
    }
    function mkDirAndCopy(srcMode, src, dest, opts) {
      mkdirSync(dest);
      copyDir(src, dest, opts);
      return setDestMode(dest, srcMode);
    }
    function copyDir(src, dest, opts) {
      for (const dirent of readdirSync(src, { withFileTypes: true })) {
        const { name } = dirent;
        const srcItem = join(src, name);
        const destItem = join(dest, name);
        const { destStat, skipped } = checkPathsSync(srcItem, destItem, opts);
        if (!skipped)
          getStats(destStat, srcItem, destItem, opts);
      }
    }
    function onLink(destStat, src, dest, opts) {
      let resolvedSrc = readlinkSync(src);
      if (!opts.verbatimSymlinks && !isAbsolute(resolvedSrc)) {
        resolvedSrc = resolve(dirname(src), resolvedSrc);
      }
      if (!destStat) {
        return symlinkSync(resolvedSrc, dest);
      }
      let resolvedDest;
      try {
        resolvedDest = readlinkSync(dest);
      } catch (err) {
        if (err.code === "EINVAL" || err.code === "UNKNOWN") {
          return symlinkSync(resolvedSrc, dest);
        }
        throw err;
      }
      if (!isAbsolute(resolvedDest)) {
        resolvedDest = resolve(dirname(dest), resolvedDest);
      }
      let srcIsDir = false;
      try {
        srcIsDir = statSync(src).isDirectory();
      } catch {}
      if (srcIsDir && isSrcSubdir(resolvedSrc, resolvedDest)) {
        throw fsCpEinvalError({
          message: `cannot copy ${resolvedSrc} to a subdirectory of self ${resolvedDest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      if (statSync(dest).isDirectory() && isSrcSubdir(resolvedDest, resolvedSrc)) {
        throw fsCpSymlinkToSubdirectoryError({
          message: `cannot overwrite ${resolvedDest} with ${resolvedSrc}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      return copyLink(resolvedSrc, dest);
    }
    function copyLink(resolvedSrc, dest) {
      unlinkSync(dest);
      return symlinkSync(resolvedSrc, dest);
    }
    return {
      cpSyncFn,
      validateCpOptions,
      tryNativeFastPathSync,
      errno: { EEXIST, EISDIR, EINVAL, ENOTDIR },
      fsCpDirToNonDirError,
      fsCpEExistError,
      fsCpEinvalError,
      fsCpFifoPipeError,
      fsCpNonDirToDirError,
      fsCpSocketError,
      fsCpSymlinkToSubdirectoryError,
      fsCpUnknownError,
      fsEisdirError,
      areIdentical,
      isSrcSubdir
    };

  })();
  const cpModule = (function () {
    const {
      errno: { EEXIST, EINVAL, EISDIR, ENOTDIR },
      fsCpDirToNonDirError,
      fsCpEExistError,
      fsCpEinvalError,
      fsCpFifoPipeError,
      fsCpNonDirToDirError,
      fsCpSocketError,
      fsCpSymlinkToSubdirectoryError,
      fsCpUnknownError,
      fsEisdirError,
      areIdentical,
      isSrcSubdir
    } = require("internal/fs/cp-sync");
    const {
      chmod,
      copyFile,
      lstat,
      mkdir,
      opendir,
      readdir,
      readlink,
      stat,
      symlink,
      unlink,
      utimes
    } = require("node:fs/promises");
    const { dirname, isAbsolute, join, parse, resolve } = require("node:path");
    const PromisePrototypeThen = Promise.prototype.then;
    const PromiseReject = Promise.reject;
    async function checkPaths(src, dest, opts) {
      if (opts.filter && !await opts.filter(src, dest)) {
        return { __proto__: null, skipped: true };
      }
      const { 0: srcStat, 1: destStat } = await getStats(src, dest, opts);
      if (destStat) {
        if (areIdentical(srcStat, destStat)) {
          throw fsCpEinvalError({
            message: "src and dest cannot be the same",
            path: dest,
            syscall: "cp",
            errno: EINVAL,
            code: "EINVAL"
          });
        }
        if (srcStat.isDirectory() && !destStat.isDirectory()) {
          throw fsCpDirToNonDirError({
            message: `cannot overwrite non-directory ${dest} with directory ${src}`,
            path: dest,
            syscall: "cp",
            errno: EISDIR,
            code: "EISDIR"
          });
        }
        if (!srcStat.isDirectory() && destStat.isDirectory()) {
          throw fsCpNonDirToDirError({
            message: `cannot overwrite directory ${dest} with non-directory ${src}`,
            path: dest,
            syscall: "cp",
            errno: ENOTDIR,
            code: "ENOTDIR"
          });
        }
      }
      if (srcStat.isDirectory() && isSrcSubdir(src, dest)) {
        throw fsCpEinvalError({
          message: `cannot copy ${src} to a subdirectory of self ${dest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      return { __proto__: null, srcStat, destStat, skipped: false };
    }
    function getStats(src, dest, opts) {
      const statFunc = opts.dereference ? (file) => stat(file, { bigint: true }) : (file) => lstat(file, { bigint: true });
      return Promise.all([
        statFunc(src),
        PromisePrototypeThen.call(statFunc(dest), undefined, (err) => {
          if (err.code === "ENOENT")
            return null;
          throw err;
        })
      ]);
    }
    async function checkParentPaths(src, srcStat, dest) {
      const srcParent = resolve(dirname(src));
      const destParent = resolve(dirname(dest));
      if (destParent === srcParent || destParent === parse(destParent).root) {
        return;
      }
      let destStat;
      try {
        destStat = await stat(destParent, { bigint: true });
      } catch (err) {
        if (err.code === "ENOENT")
          return;
        throw err;
      }
      if (areIdentical(srcStat, destStat)) {
        throw fsCpEinvalError({
          message: `cannot copy ${src} to a subdirectory of self ${dest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      return checkParentPaths(src, srcStat, destParent);
    }
    async function treeContainsOnlyFilesAndDirs(root) {
      const stack = [root];
      while (stack.length) {
        const dir = stack.pop();
        let entries;
        try {
          entries = await readdir(dir, { withFileTypes: true });
        } catch {
          return false;
        }
        for (let i = 0;i < entries.length; i++) {
          const entry = entries[i];
          if (entry.isDirectory()) {
            stack.push(join(dir, entry.name));
          } else if (!entry.isFile()) {
            return false;
          }
        }
      }
      return true;
    }
    async function tryNativeFastPath(src, dest, opts) {
      const checked = await checkPaths(src, dest, opts);
      const { srcStat, destStat } = checked;
      await checkParentPaths(src, srcStat, dest);
      if (srcStat.isDirectory() && !opts.recursive) {
        throw fsEisdirError({
          message: `${src} is a directory (not copied)`,
          path: src,
          syscall: "cp",
          errno: EISDIR,
          code: "EISDIR"
        });
      }
      if (srcStat.isDirectory()) {
        return {
          ok: process.platform === "darwin" && !destStat && await treeContainsOnlyFilesAndDirs(src),
          checked
        };
      }
      return { ok: srcStat.isFile() && (!destStat || destStat.isFile()), checked };
    }
    async function cpFn(src, dest, opts, checked) {
      const { srcStat, destStat, skipped } = checked ?? await checkPaths(src, dest, opts);
      if (skipped)
        return;
      if (checked === undefined)
        await checkParentPaths(src, srcStat, dest);
      return checkParentDir(destStat, src, dest, opts);
    }
    async function checkParentDir(destStat, src, dest, opts) {
      const destParent = dirname(dest);
      const dirExists = await pathExists(destParent);
      if (dirExists)
        return getStatsForCopy(destStat, src, dest, opts);
      await mkdir(destParent, { recursive: true });
      return getStatsForCopy(destStat, src, dest, opts);
    }
    function pathExistsFulfilled() {
      return true;
    }
    function pathExistsRejected(err) {
      return err.code === "ENOENT" ? false : PromiseReject(err);
    }
    function pathExists(dest) {
      return PromisePrototypeThen.call(stat(dest), pathExistsFulfilled, pathExistsRejected);
    }
    async function getStatsForCopy(destStat, src, dest, opts) {
      const statFn = opts.dereference ? stat : lstat;
      const srcStat = await statFn(src);
      if (srcStat.isDirectory() && opts.recursive) {
        return onDir(srcStat, destStat, src, dest, opts);
      } else if (srcStat.isDirectory()) {
        throw fsEisdirError({
          message: `${src} is a directory (not copied)`,
          path: src,
          syscall: "cp",
          errno: EISDIR,
          code: "EISDIR"
        });
      } else if (srcStat.isFile() || srcStat.isCharacterDevice() || srcStat.isBlockDevice()) {
        return onFile(srcStat, destStat, src, dest, opts);
      } else if (srcStat.isSymbolicLink()) {
        return onLink(destStat, src, dest, opts);
      } else if (srcStat.isSocket()) {
        throw fsCpSocketError({
          message: `cannot copy a socket file: ${dest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      } else if (srcStat.isFIFO()) {
        throw fsCpFifoPipeError({
          message: `cannot copy a FIFO pipe: ${dest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      throw fsCpUnknownError({
        message: `cannot copy an unknown file type: ${dest}`,
        path: dest,
        syscall: "cp",
        errno: EINVAL,
        code: "EINVAL"
      });
    }
    function onFile(srcStat, destStat, src, dest, opts) {
      if (!destStat)
        return _copyFile(srcStat, src, dest, opts);
      return mayCopyFile(srcStat, src, dest, opts);
    }
    async function mayCopyFile(srcStat, src, dest, opts) {
      if (opts.force) {
        await unlink(dest);
        return _copyFile(srcStat, src, dest, opts);
      } else if (opts.errorOnExist) {
        throw fsCpEExistError({
          message: `${dest} already exists`,
          path: dest,
          syscall: "cp",
          errno: EEXIST,
          code: "EEXIST"
        });
      }
    }
    async function _copyFile(srcStat, src, dest, opts) {
      await copyFile(src, dest, opts.mode);
      if (opts.preserveTimestamps) {
        return handleTimestampsAndMode(srcStat.mode, src, dest);
      }
      return setDestMode(dest, srcStat.mode);
    }
    async function handleTimestampsAndMode(srcMode, src, dest) {
      if (fileIsNotWritable(srcMode)) {
        await makeFileWritable(dest, srcMode);
        return setDestTimestampsAndMode(srcMode, src, dest);
      }
      return setDestTimestampsAndMode(srcMode, src, dest);
    }
    function fileIsNotWritable(srcMode) {
      return (srcMode & 128) === 0;
    }
    function makeFileWritable(dest, srcMode) {
      return setDestMode(dest, srcMode | 128);
    }
    async function setDestTimestampsAndMode(srcMode, src, dest) {
      await setDestTimestamps(src, dest);
      return setDestMode(dest, srcMode);
    }
    function setDestMode(dest, srcMode) {
      return chmod(dest, srcMode);
    }
    async function setDestTimestamps(src, dest) {
      const updatedSrcStat = await stat(src);
      return utimes(dest, updatedSrcStat.atime, updatedSrcStat.mtime);
    }
    function onDir(srcStat, destStat, src, dest, opts) {
      if (!destStat)
        return mkDirAndCopy(srcStat.mode, src, dest, opts);
      if (opts.errorOnExist && !opts.force) {
        throw fsCpEExistError({
          message: `${dest} already exists`,
          path: dest,
          syscall: "cp",
          errno: EEXIST,
          code: "EEXIST"
        });
      }
      return copyDir(src, dest, opts);
    }
    async function mkDirAndCopy(srcMode, src, dest, opts) {
      await mkdir(dest);
      await copyDir(src, dest, opts);
      return setDestMode(dest, srcMode);
    }
    async function copyDir(src, dest, opts) {
      const dir = await opendir(src);
      for await (const { name } of dir) {
        const srcItem = join(src, name);
        const destItem = join(dest, name);
        const { destStat, skipped } = await checkPaths(srcItem, destItem, opts);
        if (!skipped)
          await getStatsForCopy(destStat, srcItem, destItem, opts);
      }
    }
    async function onLink(destStat, src, dest, opts) {
      let resolvedSrc = await readlink(src);
      if (!opts.verbatimSymlinks && !isAbsolute(resolvedSrc)) {
        resolvedSrc = resolve(dirname(src), resolvedSrc);
      }
      if (!destStat) {
        return symlink(resolvedSrc, dest);
      }
      let resolvedDest;
      try {
        resolvedDest = await readlink(dest);
      } catch (err) {
        if (err.code === "EINVAL" || err.code === "UNKNOWN") {
          return symlink(resolvedSrc, dest);
        }
        throw err;
      }
      if (!isAbsolute(resolvedDest)) {
        resolvedDest = resolve(dirname(dest), resolvedDest);
      }
      const srcStat = await stat(src);
      const srcIsDir = srcStat.isDirectory();
      if (srcIsDir && isSrcSubdir(resolvedSrc, resolvedDest)) {
        throw fsCpEinvalError({
          message: `cannot copy ${resolvedSrc} to a subdirectory of self ${resolvedDest}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      if (srcIsDir && isSrcSubdir(resolvedDest, resolvedSrc)) {
        throw fsCpSymlinkToSubdirectoryError({
          message: `cannot overwrite ${resolvedDest} with ${resolvedSrc}`,
          path: dest,
          syscall: "cp",
          errno: EINVAL,
          code: "EINVAL"
        });
      }
      return copyLink(resolvedSrc, dest);
    }
    async function copyLink(resolvedSrc, dest) {
      await unlink(dest);
      return symlink(resolvedSrc, dest);
    }
    return { cpFn, tryNativeFastPath };

  })();
  // Os despachantes de `fs.cpSync` (`fs.ts`) e `fs.promises.cp` (`fs.promises.ts`) do bun, sem o caminho nativo: o
  // bun só o usa quando `ok` e o resultado é o mesmo do caminho em JavaScript, que aqui roda sempre (com o `checked`
  // da validação, como no bun, para a ordem dos erros ser a mesma).
  function getValidatedFsPath(value, name = "path") {
    if (value instanceof URL) value = host.requireBuiltin("node:url").fileURLToPath(value);
    if (typeof value === "string") {
      if (value.indexOf("\u0000") !== -1) {
        throw codes.INVALID_ARG_VALUE(name, value, "must be a string, Uint8Array, or URL without null bytes");
      }
      return value;
    }
    if (value instanceof Uint8Array) {
      if (value.indexOf(0) !== -1) {
        throw codes.INVALID_ARG_VALUE(name, value, "must be a string, Uint8Array, or URL without null bytes");
      }
      return value;
    }
    throw codes.INVALID_ARG_TYPE(name, ["string", "Buffer", "URL"], value);
  }

  const { validateCpOptions, cpSyncFn, tryNativeFastPathSync } = cpSyncModule;
  const { cpFn, tryNativeFastPath } = cpModule;

  const isPlainCopy = options =>
    !options.filter && !options.dereference && !options.preserveTimestamps && !options.verbatimSymlinks && !options.mode && !options.errorOnExist && options.force;

  function cpSync(src, dest, options) {
    options = validateCpOptions(options);
    src = getValidatedFsPath(src, "src");
    dest = getValidatedFsPath(dest, "dest");
    if (isPlainCopy(options)) return cpSyncFn(src, dest, options, tryNativeFastPathSync(src, dest, options).checked);
    return cpSyncFn(src, dest, options);
  }

  async function cp(src, dest, options) {
    options = validateCpOptions(options);
    src = getValidatedFsPath(src, "src");
    dest = getValidatedFsPath(dest, "dest");
    if (isPlainCopy(options)) return cpFn(src, dest, options, (await tryNativeFastPath(src, dest, options)).checked);
    return cpFn(src, dest, options);
  }

  return { cpSync, cp, validateCpOptions, getValidatedFsPath };
})
