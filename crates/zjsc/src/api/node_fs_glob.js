// GERADO por `scripts/gen-node-fs-glob.js` a partir de `src/js/internal/fs/glob.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { codes, validators } = host.internal;
  const isPromise = value => value instanceof Promise;
  const emptyObject = Object.freeze({ __proto__: null });
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/shared") return { kEmptyObject: emptyObject };
    return host.requireBuiltin(name);
  };
  const { validateObject, validateString, validateBoolean, validateArray } = require("internal/validators");
  const { join, resolve, basename, dirname, isAbsolute } = require("node:path");
  const { kEmptyObject } = require("internal/shared");
  const isWindows = process.platform === "win32";
  const isMacOS = process.platform === "darwin";
  let _fs;
  function lazyFs() {
    return _fs ??= require("node:fs");
  }
  let _fsPromises;
  function lazyFsPromises() {
    return _fsPromises ??= require("node:fs/promises");
  }
  function compareDirentName(a, b) {
    return a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
  }
  function identity(v) {
    return v;
  }
  function nullOnReject() {
    return null;
  }
  function emptyArrayOnReject() {
    return [];
  }
  function excludeNothing(_path) {
    return false;
  }
  function makeMatchersExclude(matchers) {
    return function isExcludedByMatchers(value) {
      for (const matcher of matchers) {
        if (matcher.match(value))
          return true;
      }
      return false;
    };
  }
  function statForFileTypes(cache, root, path) {
    return cache.statSync(isAbsolute(path) ? path : join(root, path));
  }
  const kStats = Symbol("stats");
  let _DirentFromStats;
  function lazyDirentFromStats() {
    if (_DirentFromStats === undefined) {
      const { Dirent } = lazyFs();

      class DirentFromStats extends Dirent {
        constructor(name, stats, path) {
          super(name, null, path);
          this[kStats] = stats;
        }
      }
      for (const key of [
        "isBlockDevice",
        "isCharacterDevice",
        "isDirectory",
        "isFIFO",
        "isFile",
        "isSocket",
        "isSymbolicLink"
      ]) {
        DirentFromStats.prototype[key] = function() {
          return this[kStats][key]();
        };
      }
      _DirentFromStats = DirentFromStats;
    }
    return _DirentFromStats;
  }
  function toPathIfFileURL(url) {
    if (url == null || typeof url === "string") {
      return url;
    }
    if (url instanceof URL || typeof url === "object" && typeof url.href === "string" && typeof url.protocol === "string") {
      return Bun.fileURLToPath(url);
    }
    return url;
  }
  async function getDirent(path) {
    let stat;
    try {
      stat = await lazyFsPromises().lstat(path);
    } catch {
      return null;
    }
    const DirentFromStats = lazyDirentFromStats();
    return new DirentFromStats(basename(path), stat, dirname(path));
  }
  function sortDirents(dirents) {
    return dirents.sort(compareDirentName);
  }
  function getDirentSync(path) {
    let stat;
    try {
      stat = lazyFs().lstatSync(path);
    } catch {
      return null;
    }
    const DirentFromStats = lazyDirentFromStats();
    return new DirentFromStats(basename(path), stat, dirname(path));
  }
  function validateStringArrayOrFunction(value, name) {
    if (Array.isArray(value)) {
      for (let i = 0;i < value.length; ++i) {
        if (typeof value[i] !== "string") {
          throw codes.INVALID_ARG_TYPE(`${name}[${i}]`, "string", value[i]);
        }
      }
      return;
    }
    if (typeof value !== "function") {
      throw codes.INVALID_ARG_TYPE(name, ["string[]", "function"], value);
    }
  }
  function validateStringArray(value, name) {
    validateArray(value, name);
    for (let i = 0;i < value.length; ++i) {
      validateString(value[i], `${name}[${i}]`);
    }
  }
  function createMatcher(pattern, options = kEmptyObject) {
    const opts = {
      __proto__: null,
      nocase: isWindows || isMacOS,
      windowsPathsNoEscape: true,
      nonegate: true,
      nocomment: true,
      optimizationLevel: 2,
      platform: process.platform,
      nocaseMagicOnly: true,
      ...options
    };
    return new (lazyMinimatch()).Minimatch(pattern, opts);
  }
  function cloneSet(values) {
    const cloned = new Set;
    for (const value of values) {
      cloned.add(value);
    }
    return cloned;
  }

  class Cache {
    #cache = new Map;
    #statsCache = new Map;
    #followStatsCache = new Map;
    #readdirCache = new Map;
    #realpathCache = new Map;
    stat(path) {
      const cached = this.#statsCache.get(path);
      if (cached) {
        return cached;
      }
      const promise = getDirent(path);
      this.#statsCache.set(path, promise);
      return promise;
    }
    statSync(path) {
      const cached = this.#statsCache.get(path);
      if (cached && !(cached instanceof Promise)) {
        return cached;
      }
      const val = getDirentSync(path);
      this.#statsCache.set(path, val);
      return val;
    }
    followStat(path) {
      const cached = this.#followStatsCache.get(path);
      if (cached) {
        return cached;
      }
      const promise = lazyFsPromises().stat(path).then(identity, nullOnReject);
      this.#followStatsCache.set(path, promise);
      return promise;
    }
    followStatSync(path) {
      const cached = this.#followStatsCache.get(path);
      if (cached && !(cached instanceof Promise)) {
        return cached;
      }
      let val;
      try {
        val = lazyFs().statSync(path);
      } catch {
        val = null;
      }
      this.#followStatsCache.set(path, val);
      return val;
    }
    realpath(path) {
      const cached = this.#realpathCache.get(path);
      if (cached) {
        return cached;
      }
      const promise = lazyFsPromises().realpath(path).then(identity, nullOnReject);
      this.#realpathCache.set(path, promise);
      return promise;
    }
    realpathSync(path) {
      const cached = this.#realpathCache.get(path);
      if (cached && !(cached instanceof Promise)) {
        return cached;
      }
      let val;
      try {
        val = lazyFs().realpathSync(path);
      } catch {
        val = null;
      }
      this.#realpathCache.set(path, val);
      return val;
    }
    addToStatCache(path, val) {
      this.#statsCache.set(path, val);
    }
    async readdir(path) {
      const cached = this.#readdirCache.get(path);
      if (cached) {
        return cached;
      }
      const promise = lazyFsPromises().readdir(path, { __proto__: null, withFileTypes: true }).then(sortDirents, emptyArrayOnReject);
      this.#readdirCache.set(path, promise);
      return promise;
    }
    readdirSync(path) {
      const cached = this.#readdirCache.get(path);
      if (cached) {
        return cached;
      }
      let val;
      try {
        val = sortDirents(lazyFs().readdirSync(path, { __proto__: null, withFileTypes: true }));
      } catch {
        val = [];
      }
      this.#readdirCache.set(path, val);
      return val;
    }
    add(path, pattern) {
      let cache = this.#cache.get(path);
      if (!cache) {
        cache = new Set;
        this.#cache.set(path, cache);
      }
      const originalSize = cache.size;
      for (const index of pattern.indexes) {
        cache.add(pattern.cacheKey(index));
      }
      return cache.size !== originalSize + pattern.indexes.size;
    }
    seen(path, pattern, index) {
      return this.#cache.get(path)?.has(pattern.cacheKey(index));
    }
  }

  class Pattern {
    #pattern;
    #globStrings;
    indexes;
    symlinks;
    realpaths;
    last;
    constructor(pattern, globStrings, indexes, symlinks, realpaths = new Set) {
      this.#pattern = pattern;
      this.#globStrings = globStrings;
      this.indexes = indexes;
      this.symlinks = symlinks;
      this.realpaths = realpaths;
      this.last = pattern.length - 1;
    }
    isLast(isDirectory) {
      return this.indexes.has(this.last) || this.at(-1) === "" && isDirectory && this.indexes.has(this.last - 1) && this.at(-2) === lazyMinimatch().GLOBSTAR;
    }
    isFirst() {
      return this.indexes.has(0);
    }
    get hasSeenSymlinks() {
      for (const i of this.indexes) {
        if (!this.symlinks.has(i))
          return true;
      }
      return false;
    }
    at(index) {
      return this.#pattern.at(index);
    }
    child(indexes, symlinks = new Set, realpaths = this.realpaths) {
      return new Pattern(this.#pattern, this.#globStrings, indexes, symlinks, realpaths);
    }
    test(index, path) {
      if (index > this.#pattern.length) {
        return false;
      }
      const pattern = this.#pattern[index];
      if (pattern === lazyMinimatch().GLOBSTAR) {
        return true;
      }
      if (typeof pattern === "string") {
        return pattern === path;
      }
      if (typeof pattern?.test === "function") {
        return pattern.test(path);
      }
      return false;
    }
    cacheKey(index) {
      let key = "";
      for (let i = index;i < this.#globStrings.length; i++) {
        key += this.#globStrings[i];
        if (i !== this.#globStrings.length - 1) {
          key += "/";
        }
      }
      return key;
    }
  }

  class ResultSet extends Set {
    #root = ".";
    #isExcluded = excludeNothing;
    setup(root, isExcludedFn) {
      this.#root = root;
      this.#isExcluded = isExcludedFn;
    }
    add(value) {
      if (this.#isExcluded(resolve(this.#root, value))) {
        return false;
      }
      super.add(value);
      return true;
    }
  }

  class Glob {
    #root;
    #exclude;
    #cache = new Cache;
    #results = new ResultSet;
    #queue = [];
    #subpatterns = new Map;
    #patterns;
    #withFileTypes;
    #followSymlinks = false;
    #isExcluded = excludeNothing;
    matchers;
    constructor(pattern, options = kEmptyObject) {
      validateObject(options, "options");
      const { exclude, cwd, followSymlinks, withFileTypes } = options;
      this.#root = toPathIfFileURL(cwd) ?? process.cwd();
      if (followSymlinks != null) {
        validateBoolean(followSymlinks, "options.followSymlinks");
        this.#followSymlinks = followSymlinks;
      }
      this.#withFileTypes = !!withFileTypes;
      if (exclude != null) {
        validateStringArrayOrFunction(exclude, "options.exclude");
        if (Array.isArray(exclude)) {
          const matchers = [];
          for (const pat of exclude) {
            matchers.push(createMatcher(resolve(this.#root, pat)));
          }
          this.#isExcluded = makeMatchersExclude(matchers);
          this.#results.setup(this.#root, this.#isExcluded);
        } else {
          this.#exclude = exclude;
        }
      }
      let patterns;
      if (typeof pattern === "object") {
        validateStringArray(pattern, "patterns");
        patterns = pattern;
      } else {
        validateString(pattern, "patterns");
        patterns = [pattern];
      }
      this.matchers = [];
      this.#patterns = [];
      for (const pat of patterns) {
        const matcher = createMatcher(pat);
        this.matchers.push(matcher);
        for (let i = 0;i < matcher.set.length; i++) {
          this.#patterns.push(new Pattern(matcher.set[i], matcher.globParts[i], new Set().add(0), new Set));
        }
      }
    }
    globSync() {
      this.#queue.push({ __proto__: null, path: ".", patterns: this.#patterns });
      while (this.#queue.length > 0) {
        const item = this.#queue.pop();
        for (let i = 0;i < item.patterns.length; i++) {
          this.#addSubpatterns(item.path, item.patterns[i]);
        }
        for (const [path, patterns] of this.#subpatterns) {
          this.#queue.push({ __proto__: null, path, patterns });
        }
        this.#subpatterns.clear();
      }
      return Array.from(this.#results, this.#withFileTypes ? statForFileTypes.bind(null, this.#cache, this.#root) : undefined);
    }
    #isDirectorySync(path, stat, pattern) {
      if (stat?.isDirectory()) {
        return true;
      }
      if (!stat?.isSymbolicLink()) {
        return false;
      }
      if (this.#followSymlinks) {
        return !!this.#cache.followStatSync(path)?.isDirectory();
      }
      return pattern.hasSeenSymlinks;
    }
    async#isDirectory(path, stat, pattern) {
      if (stat?.isDirectory()) {
        return true;
      }
      if (!stat?.isSymbolicLink()) {
        return false;
      }
      if (this.#followSymlinks) {
        return !!(await this.#cache.followStat(path))?.isDirectory();
      }
      return pattern.hasSeenSymlinks;
    }
    #nextRealpathsSync(path, isDirectory, pattern) {
      if (!this.#followSymlinks || !isDirectory) {
        return pattern.realpaths;
      }
      const real = this.#cache.realpathSync(path);
      if (real === null) {
        return pattern.realpaths;
      }
      const realpaths = cloneSet(pattern.realpaths);
      realpaths.add(real);
      return realpaths;
    }
    async#nextRealpaths(path, isDirectory, pattern) {
      if (!this.#followSymlinks || !isDirectory) {
        return pattern.realpaths;
      }
      const real = await this.#cache.realpath(path);
      if (real === null) {
        return pattern.realpaths;
      }
      const realpaths = cloneSet(pattern.realpaths);
      realpaths.add(real);
      return realpaths;
    }
    async#isCyclic(path, isDirectory, pattern) {
      if (!this.#followSymlinks || !isDirectory) {
        return false;
      }
      const real = await this.#cache.realpath(path);
      return real !== null && pattern.realpaths.has(real);
    }
    #isCyclicSync(path, isDirectory, pattern) {
      if (!this.#followSymlinks || !isDirectory) {
        return false;
      }
      const real = this.#cache.realpathSync(path);
      return real !== null && pattern.realpaths.has(real);
    }
    #addSubpattern(path, pattern) {
      if (this.#isExcluded(path)) {
        return;
      }
      const fullpath = resolve(this.#root, path);
      if (this.#isExcluded(`${fullpath}/`) && this.#cache.statSync(fullpath).isDirectory()) {
        return;
      }
      if (this.#exclude) {
        if (this.#withFileTypes) {
          const stat = this.#cache.statSync(fullpath);
          if (stat !== null) {
            if (this.#exclude(stat)) {
              return;
            }
          }
        } else if (this.#exclude(path)) {
          return;
        }
      }
      if (!this.#subpatterns.has(path)) {
        this.#subpatterns.set(path, [pattern]);
      } else {
        this.#subpatterns.get(path).push(pattern);
      }
    }
    #addSubpatterns(path, pattern) {
      const seen = this.#cache.add(path, pattern);
      if (seen) {
        return;
      }
      const fullpath = resolve(this.#root, path);
      const stat = this.#cache.statSync(fullpath);
      const last = pattern.last;
      const isDirectory = this.#isDirectorySync(fullpath, stat, pattern);
      const isLast = pattern.isLast(isDirectory);
      const isFirst = pattern.isFirst();
      if (this.#isExcluded(fullpath)) {
        return;
      }
      if (isFirst && isWindows && typeof pattern.at(0) === "string" && pattern.at(0).endsWith(":")) {
        this.#addSubpattern(`${pattern.at(0)}\\`, pattern.child(new Set().add(1)));
        return;
      }
      if (isFirst && pattern.at(0) === "") {
        this.#addSubpattern("/", pattern.child(new Set().add(1)));
        return;
      }
      if (isFirst && pattern.at(0) === "..") {
        this.#addSubpattern("../", pattern.child(new Set().add(1)));
        return;
      }
      if (isFirst && pattern.at(0) === ".") {
        this.#addSubpattern(".", pattern.child(new Set().add(1)));
        return;
      }
      if (isLast && typeof pattern.at(-1) === "string") {
        const p = pattern.at(-1);
        const stat = this.#cache.statSync(join(fullpath, p));
        if (stat && (p || isDirectory)) {
          this.#results.add(join(path, p));
        }
        if (pattern.indexes.size === 1 && pattern.indexes.has(last)) {
          return;
        }
      } else if (isLast && pattern.at(-1) === lazyMinimatch().GLOBSTAR && (path !== "." || pattern.at(0) === "." || last === 0 && stat)) {
        this.#results.add(path);
      }
      if (!isDirectory || this.#isCyclicSync(fullpath, isDirectory, pattern)) {
        return;
      }
      const nextRealpaths = this.#nextRealpathsSync(fullpath, isDirectory, pattern);
      let children;
      const firstPattern = pattern.indexes.size === 1 && pattern.at(pattern.indexes.values().next().value);
      if (typeof firstPattern === "string") {
        const stat = this.#cache.statSync(join(fullpath, firstPattern));
        if (stat) {
          setDirentName(stat, firstPattern);
          children = [stat];
        } else {
          return;
        }
      } else {
        children = this.#cache.readdirSync(fullpath);
      }
      for (let i = 0;i < children.length; i++) {
        const entry = children[i];
        const entryPath = join(path, entry.name);
        const entryFullpath = join(fullpath, entry.name);
        this.#cache.addToStatCache(entryFullpath, entry);
        const entryIsDirectory = entry.isDirectory() || this.#followSymlinks && entry.isSymbolicLink() && !!this.#cache.followStatSync(entryFullpath)?.isDirectory();
        const subPatterns = new Set;
        const nSymlinks = new Set;
        for (const index of pattern.indexes) {
          if (this.#cache.seen(entryPath, pattern, index) || this.#cache.seen(entryPath, pattern, index + 1)) {
            return;
          }
          const current = pattern.at(index);
          const nextIndex = index + 1;
          const next = pattern.at(nextIndex);
          const fromSymlink = !this.#followSymlinks && pattern.symlinks.has(index);
          if (current === lazyMinimatch().GLOBSTAR) {
            const isDot = entry.name[0] === ".";
            const nextMatches = pattern.test(nextIndex, entry.name);
            let nextNonGlobIndex = nextIndex;
            while (pattern.at(nextNonGlobIndex) === lazyMinimatch().GLOBSTAR) {
              nextNonGlobIndex++;
            }
            const matchesDot = isDot && pattern.test(nextNonGlobIndex, entry.name);
            if (isDot && !matchesDot || this.#exclude && this.#exclude(this.#withFileTypes ? entry : entry.name)) {
              continue;
            }
            if (!fromSymlink && entryIsDirectory) {
              subPatterns.add(index);
            } else if (!fromSymlink && index === last) {
              this.#results.add(entryPath);
            }
            if (nextMatches && nextIndex === last && !isLast) {
              this.#results.add(entryPath);
            } else if (nextMatches && entryIsDirectory) {
              subPatterns.add(index + 2);
            }
            if ((nextMatches || pattern.at(0) === ".") && (entryIsDirectory || entry.isSymbolicLink()) && !fromSymlink) {
              subPatterns.add(nextIndex);
            }
            if (!this.#followSymlinks && entry.isSymbolicLink()) {
              nSymlinks.add(index);
            }
            if (next === ".." && entryIsDirectory) {
              const parent = join(path, "..");
              if (nextIndex < last) {
                if (!this.#subpatterns.has(path) && !this.#cache.seen(path, pattern, nextIndex + 1)) {
                  this.#subpatterns.set(path, [pattern.child(new Set().add(nextIndex + 1))]);
                }
                if (!this.#subpatterns.has(parent) && !this.#cache.seen(parent, pattern, nextIndex + 1)) {
                  this.#subpatterns.set(parent, [pattern.child(new Set().add(nextIndex + 1))]);
                }
              } else {
                if (!this.#cache.seen(path, pattern, nextIndex)) {
                  this.#cache.add(path, pattern.child(new Set().add(nextIndex)));
                  this.#results.add(path);
                }
                if (!this.#cache.seen(path, pattern, nextIndex) || !this.#cache.seen(parent, pattern, nextIndex)) {
                  this.#cache.add(parent, pattern.child(new Set().add(nextIndex)));
                  this.#results.add(parent);
                }
              }
            }
          }
          if (typeof current === "string") {
            if (pattern.test(index, entry.name) && index !== last) {
              subPatterns.add(nextIndex);
            } else if (current === "." && pattern.test(nextIndex, entry.name)) {
              if (nextIndex === last) {
                this.#results.add(entryPath);
              } else {
                subPatterns.add(nextIndex + 1);
              }
            }
          }
          if (typeof current === "object" && pattern.test(index, entry.name)) {
            if (index === last) {
              this.#results.add(entryPath);
            } else if (entryIsDirectory) {
              subPatterns.add(nextIndex);
            }
          }
        }
        if (subPatterns.size > 0) {
          this.#addSubpattern(entryPath, pattern.child(subPatterns, nSymlinks, nextRealpaths));
        }
      }
    }
    async* glob() {
      this.#queue.push({ __proto__: null, path: ".", patterns: this.#patterns });
      while (this.#queue.length > 0) {
        const item = this.#queue.pop();
        for (let i = 0;i < item.patterns.length; i++) {
          yield* this.#iterateSubpatterns(item.path, item.patterns[i]);
        }
        for (const [path, patterns] of this.#subpatterns) {
          this.#queue.push({ __proto__: null, path, patterns });
        }
        this.#subpatterns.clear();
      }
    }
    async* #iterateSubpatterns(path, pattern) {
      const seen = this.#cache.add(path, pattern);
      if (seen) {
        return;
      }
      const fullpath = resolve(this.#root, path);
      const stat = await this.#cache.stat(fullpath);
      const last = pattern.last;
      const isDirectory = await this.#isDirectory(fullpath, stat, pattern);
      const isLast = pattern.isLast(isDirectory);
      const isFirst = pattern.isFirst();
      if (this.#isExcluded(fullpath)) {
        return;
      }
      if (isFirst && isWindows && typeof pattern.at(0) === "string" && pattern.at(0).endsWith(":")) {
        this.#addSubpattern(`${pattern.at(0)}\\`, pattern.child(new Set().add(1)));
        return;
      }
      if (isFirst && pattern.at(0) === "") {
        this.#addSubpattern("/", pattern.child(new Set().add(1)));
        return;
      }
      if (isFirst && pattern.at(0) === "..") {
        this.#addSubpattern("../", pattern.child(new Set().add(1)));
        return;
      }
      if (isFirst && pattern.at(0) === ".") {
        this.#addSubpattern(".", pattern.child(new Set().add(1)));
        return;
      }
      if (isLast && typeof pattern.at(-1) === "string") {
        const p = pattern.at(-1);
        const stat = await this.#cache.stat(join(fullpath, p));
        if (stat && (p || isDirectory)) {
          const result = join(path, p);
          if (!this.#results.has(result)) {
            if (this.#results.add(result)) {
              yield this.#withFileTypes ? stat : result;
            }
          }
        }
        if (pattern.indexes.size === 1 && pattern.indexes.has(last)) {
          return;
        }
      } else if (isLast && pattern.at(-1) === lazyMinimatch().GLOBSTAR && (path !== "." || pattern.at(0) === "." || last === 0 && stat)) {
        if (!this.#results.has(path)) {
          if (this.#results.add(path)) {
            yield this.#withFileTypes ? stat : path;
          }
        }
      }
      if (!isDirectory || await this.#isCyclic(fullpath, isDirectory, pattern)) {
        return;
      }
      const nextRealpaths = await this.#nextRealpaths(fullpath, isDirectory, pattern);
      let children;
      const firstPattern = pattern.indexes.size === 1 && pattern.at(pattern.indexes.values().next().value);
      if (typeof firstPattern === "string") {
        const stat = await this.#cache.stat(join(fullpath, firstPattern));
        if (stat) {
          setDirentName(stat, firstPattern);
          children = [stat];
        } else {
          return;
        }
      } else {
        children = await this.#cache.readdir(fullpath);
      }
      for (let i = 0;i < children.length; i++) {
        const entry = children[i];
        const entryPath = join(path, entry.name);
        const entryFullpath = join(fullpath, entry.name);
        this.#cache.addToStatCache(entryFullpath, entry);
        const entryIsDirectory = entry.isDirectory() || this.#followSymlinks && entry.isSymbolicLink() && !!(await this.#cache.followStat(entryFullpath))?.isDirectory();
        const subPatterns = new Set;
        const nSymlinks = new Set;
        for (const index of pattern.indexes) {
          if (this.#cache.seen(entryPath, pattern, index) || this.#cache.seen(entryPath, pattern, index + 1)) {
            return;
          }
          const current = pattern.at(index);
          const nextIndex = index + 1;
          const next = pattern.at(nextIndex);
          const fromSymlink = !this.#followSymlinks && pattern.symlinks.has(index);
          if (current === lazyMinimatch().GLOBSTAR) {
            const isDot = entry.name[0] === ".";
            const nextMatches = pattern.test(nextIndex, entry.name);
            let nextNonGlobIndex = nextIndex;
            while (pattern.at(nextNonGlobIndex) === lazyMinimatch().GLOBSTAR) {
              nextNonGlobIndex++;
            }
            const matchesDot = isDot && pattern.test(nextNonGlobIndex, entry.name);
            if (isDot && !matchesDot || this.#exclude && this.#exclude(this.#withFileTypes ? entry : entry.name)) {
              continue;
            }
            if (!fromSymlink && entryIsDirectory) {
              subPatterns.add(index);
            } else if (!fromSymlink && index === last) {
              if (!this.#results.has(entryPath) && this.#results.add(entryPath)) {
                yield this.#withFileTypes ? entry : entryPath;
              }
            }
            if (nextMatches && nextIndex === last && !isLast) {
              if (!this.#results.has(entryPath) && this.#results.add(entryPath)) {
                yield this.#withFileTypes ? entry : entryPath;
              }
            } else if (nextMatches && entryIsDirectory) {
              subPatterns.add(index + 2);
            }
            if ((nextMatches || pattern.at(0) === ".") && (entryIsDirectory || entry.isSymbolicLink()) && !fromSymlink) {
              subPatterns.add(nextIndex);
            }
            if (!this.#followSymlinks && entry.isSymbolicLink()) {
              nSymlinks.add(index);
            }
            if (next === ".." && entryIsDirectory) {
              const parent = join(path, "..");
              if (nextIndex < last) {
                if (!this.#subpatterns.has(path) && !this.#cache.seen(path, pattern, nextIndex + 1)) {
                  this.#subpatterns.set(path, [pattern.child(new Set().add(nextIndex + 1))]);
                }
                if (!this.#subpatterns.has(parent) && !this.#cache.seen(parent, pattern, nextIndex + 1)) {
                  this.#subpatterns.set(parent, [pattern.child(new Set().add(nextIndex + 1))]);
                }
              } else {
                if (!this.#cache.seen(path, pattern, nextIndex)) {
                  this.#cache.add(path, pattern.child(new Set().add(nextIndex)));
                  if (!this.#results.has(path)) {
                    if (this.#results.add(path)) {
                      yield this.#withFileTypes ? this.#cache.statSync(fullpath) : path;
                    }
                  }
                }
                if (!this.#cache.seen(path, pattern, nextIndex) || !this.#cache.seen(parent, pattern, nextIndex)) {
                  this.#cache.add(parent, pattern.child(new Set().add(nextIndex)));
                  if (!this.#results.has(parent)) {
                    if (this.#results.add(parent)) {
                      yield this.#withFileTypes ? this.#cache.statSync(join(this.#root, parent)) : parent;
                    }
                  }
                }
              }
            }
          }
          if (typeof current === "string") {
            if (pattern.test(index, entry.name) && index !== last) {
              subPatterns.add(nextIndex);
            } else if (current === "." && pattern.test(nextIndex, entry.name)) {
              if (nextIndex === last) {
                if (!this.#results.has(entryPath)) {
                  if (this.#results.add(entryPath)) {
                    yield this.#withFileTypes ? entry : entryPath;
                  }
                }
              } else {
                subPatterns.add(nextIndex + 1);
              }
            }
          }
          if (typeof current === "object" && pattern.test(index, entry.name)) {
            if (index === last) {
              if (!this.#results.has(entryPath)) {
                if (this.#results.add(entryPath)) {
                  yield this.#withFileTypes ? entry : entryPath;
                }
              }
            } else if (entryIsDirectory) {
              subPatterns.add(nextIndex);
            }
          }
        }
        if (subPatterns.size > 0) {
          this.#addSubpattern(entryPath, pattern.child(subPatterns, nSymlinks, nextRealpaths));
        }
      }
    }
  }
  function setDirentName(dirent, name) {
    try {
      dirent.name = name;
    } catch {
      Object.defineProperty(dirent, "name", {
        value: name,
        writable: true,
        enumerable: true,
        configurable: true
      });
    }
  }
  function glob(pattern, options) {
    return new Glob(pattern, options).glob();
  }
  function globSync(pattern, options) {
    return new Glob(pattern, options).globSync();
  }
  const globModule = { glob, globSync, Glob };
  let _minimatch;
  function lazyMinimatch() {
    if (_minimatch)
      return _minimatch;
    const exports = {};
    const module = { exports };
    var __getOwnPropNames = Object.getOwnPropertyNames;
    var __commonJS = (cb, mod) => function __require() {
      return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
    };
    var require_commonjs = __commonJS({
      "node_modules/balanced-match/dist/commonjs/index.js"(exports2) {
        Object.defineProperty(exports2, "__esModule", { value: true });
        exports2.range = exports2.balanced = undefined;
        var balanced = (a, b, str) => {
          const ma = a instanceof RegExp ? maybeMatch(a, str) : a;
          const mb = b instanceof RegExp ? maybeMatch(b, str) : b;
          const r = ma !== null && mb != null && (0, exports2.range)(ma, mb, str);
          return r && {
            start: r[0],
            end: r[1],
            pre: str.slice(0, r[0]),
            body: str.slice(r[0] + ma.length, r[1]),
            post: str.slice(r[1] + mb.length)
          };
        };
        exports2.balanced = balanced;
        var maybeMatch = (reg, str) => {
          const m = str.match(reg);
          return m ? m[0] : null;
        };
        var range = (a, b, str) => {
          let begs, beg, left, right = undefined, result;
          let ai = str.indexOf(a);
          let bi = str.indexOf(b, ai + 1);
          let i = ai;
          if (ai >= 0 && bi > 0) {
            if (a === b) {
              return [ai, bi];
            }
            begs = [];
            left = str.length;
            while (i >= 0 && !result) {
              if (i === ai) {
                begs.push(i);
                ai = str.indexOf(a, i + 1);
              } else if (begs.length === 1) {
                const r = begs.pop();
                if (r !== undefined)
                  result = [r, bi];
              } else {
                beg = begs.pop();
                if (beg !== undefined && beg < left) {
                  left = beg;
                  right = bi;
                }
                bi = str.indexOf(b, i + 1);
              }
              i = ai < bi && ai >= 0 ? ai : bi;
            }
            if (begs.length && right !== undefined) {
              result = [left, right];
            }
          }
          return result;
        };
        exports2.range = range;
      }
    });
    var require_commonjs2 = __commonJS({
      "node_modules/brace-expansion/dist/commonjs/index.js"(exports2) {
        Object.defineProperty(exports2, "__esModule", { value: true });
        exports2.EXPANSION_MAX = undefined;
        exports2.expand = expand;
        var balanced_match_1 = require_commonjs();
        var escSlash = "\x00SLASH" + Math.random() + "\x00";
        var escOpen = "\x00OPEN" + Math.random() + "\x00";
        var escClose = "\x00CLOSE" + Math.random() + "\x00";
        var escComma = "\x00COMMA" + Math.random() + "\x00";
        var escPeriod = "\x00PERIOD" + Math.random() + "\x00";
        var escSlashPattern = new RegExp(escSlash, "g");
        var escOpenPattern = new RegExp(escOpen, "g");
        var escClosePattern = new RegExp(escClose, "g");
        var escCommaPattern = new RegExp(escComma, "g");
        var escPeriodPattern = new RegExp(escPeriod, "g");
        var slashPattern = /\\\\/g;
        var openPattern = /\\{/g;
        var closePattern = /\\}/g;
        var commaPattern = /\\,/g;
        var periodPattern = /\\\./g;
        exports2.EXPANSION_MAX = 1e5;
        function numeric(str) {
          return !isNaN(str) ? parseInt(str, 10) : str.charCodeAt(0);
        }
        function escapeBraces(str) {
          return str.replace(slashPattern, escSlash).replace(openPattern, escOpen).replace(closePattern, escClose).replace(commaPattern, escComma).replace(periodPattern, escPeriod);
        }
        function unescapeBraces(str) {
          return str.replace(escSlashPattern, "\\").replace(escOpenPattern, "{").replace(escClosePattern, "}").replace(escCommaPattern, ",").replace(escPeriodPattern, ".");
        }
        function parseCommaParts(str) {
          if (!str) {
            return [""];
          }
          const parts = [];
          const m = (0, balanced_match_1.balanced)("{", "}", str);
          if (!m) {
            return str.split(",");
          }
          const { pre, body, post } = m;
          const p = pre.split(",");
          p[p.length - 1] += "{" + body + "}";
          const postParts = parseCommaParts(post);
          if (post.length) {
            p[p.length - 1] += postParts.shift();
            p.push(...postParts);
          }
          parts.push(...p);
          return parts;
        }
        function expand(str, options = {}) {
          if (!str) {
            return [];
          }
          const { max = exports2.EXPANSION_MAX } = options;
          if (str.slice(0, 2) === "{}") {
            str = "\\{\\}" + str.slice(2);
          }
          return expand_(escapeBraces(str), max, true).map(unescapeBraces);
        }
        function embrace(str) {
          return "{" + str + "}";
        }
        function isPadded(el) {
          return /^-?0\d/.test(el);
        }
        function lte(i, y) {
          return i <= y;
        }
        function gte(i, y) {
          return i >= y;
        }
        function expand_(str, max, isTop) {
          const expansions = [];
          const m = (0, balanced_match_1.balanced)("{", "}", str);
          if (!m)
            return [str];
          const pre = m.pre;
          const post = m.post.length ? expand_(m.post, max, false) : [""];
          if (m.pre.endsWith("$")) {
            for (let k = 0;k < post.length && k < max; k++) {
              const expansion = pre + "{" + m.body + "}" + post[k];
              expansions.push(expansion);
            }
          } else {
            const isNumericSequence = /^-?\d+\.\.-?\d+(?:\.\.-?\d+)?$/.test(m.body);
            const isAlphaSequence = /^[a-zA-Z]\.\.[a-zA-Z](?:\.\.-?\d+)?$/.test(m.body);
            const isSequence = isNumericSequence || isAlphaSequence;
            const isOptions = m.body.indexOf(",") >= 0;
            if (!isSequence && !isOptions) {
              const mPost = m.post;
              if (mPost.match(/,(?!,).*\}/)) {
                str = m.pre + "{" + m.body + escClose + mPost;
                return expand_(str, max, true);
              }
              return [str];
            }
            let n;
            if (isSequence) {
              n = m.body.split(/\.\./);
            } else {
              n = parseCommaParts(m.body);
              const nLength = n.length;
              if (nLength === 1 && n[0] !== undefined) {
                n = expand_(n[0], max, false).map(embrace);
                if (n.length === 1) {
                  return post.map((p) => m.pre + n[0] + p);
                }
              }
            }
            let N;
            if (isSequence && n[0] !== undefined && n[1] !== undefined) {
              const x = numeric(n[0]);
              const y = numeric(n[1]);
              const width = Math.max(n[0].length, n[1].length);
              let incr = n.length === 3 && n[2] !== undefined ? Math.max(Math.abs(numeric(n[2])), 1) : 1;
              let test = lte;
              const reverse = y < x;
              if (reverse) {
                incr *= -1;
                test = gte;
              }
              const pad = n.some(isPadded);
              N = [];
              for (let i = x;test(i, y); i += incr) {
                let c;
                if (isAlphaSequence) {
                  c = String.fromCharCode(i);
                  if (c === "\\") {
                    c = "";
                  }
                } else {
                  c = String(i);
                  if (pad) {
                    const need = width - c.length;
                    if (need > 0) {
                      const z = new Array(need + 1).join("0");
                      if (i < 0) {
                        c = "-" + z + c.slice(1);
                      } else {
                        c = z + c;
                      }
                    }
                  }
                }
                N.push(c);
              }
            } else {
              N = [];
              for (let j = 0;j < n.length; j++) {
                N.push(...expand_(n[j], max, false));
              }
            }
            for (let j = 0;j < N.length; j++) {
              for (let k = 0;k < post.length && expansions.length < max; k++) {
                const expansion = pre + N[j] + post[k];
                if (!isTop || isSequence || expansion) {
                  expansions.push(expansion);
                }
              }
            }
          }
          return expansions;
        }
      }
    });
    var require_assert_valid_pattern = __commonJS({
      "dist/commonjs/assert-valid-pattern.js"(exports2) {
        Object.defineProperty(exports2, "__esModule", { value: true });
        exports2.assertValidPattern = undefined;
        var MAX_PATTERN_LENGTH = 1024 * 64;
        var assertValidPattern = (pattern) => {
          if (typeof pattern !== "string") {
            throw new TypeError("invalid pattern");
          }
          if (pattern.length > MAX_PATTERN_LENGTH) {
            throw new TypeError("pattern is too long");
          }
        };
        exports2.assertValidPattern = assertValidPattern;
      }
    });
    var require_brace_expressions = __commonJS({
      "dist/commonjs/brace-expressions.js"(exports2) {
        Object.defineProperty(exports2, "__esModule", { value: true });
        exports2.parseClass = undefined;
        var posixClasses = {
          "[:alnum:]": ["\\p{L}\\p{Nl}\\p{Nd}", true],
          "[:alpha:]": ["\\p{L}\\p{Nl}", true],
          "[:ascii:]": ["\\x00-\\x7f", false],
          "[:blank:]": ["\\p{Zs}\\t", true],
          "[:cntrl:]": ["\\p{Cc}", true],
          "[:digit:]": ["\\p{Nd}", true],
          "[:graph:]": ["\\p{Z}\\p{C}", true, true],
          "[:lower:]": ["\\p{Ll}", true],
          "[:print:]": ["\\p{C}", true],
          "[:punct:]": ["\\p{P}", true],
          "[:space:]": ["\\p{Z}\\t\\r\\n\\v\\f", true],
          "[:upper:]": ["\\p{Lu}", true],
          "[:word:]": ["\\p{L}\\p{Nl}\\p{Nd}\\p{Pc}", true],
          "[:xdigit:]": ["A-Fa-f0-9", false]
        };
        var braceEscape = (s) => s.replace(/[[\]\\-]/g, "\\$&");
        var regexpEscape = (s) => s.replace(/[-[\]{}()*+?.,\\^$|#\s]/g, "\\$&");
        var rangesToString = (ranges) => ranges.join("");
        var parseClass = (glob, position) => {
          const pos = position;
          if (glob.charAt(pos) !== "[") {
            throw new Error("not in a brace expression");
          }
          const ranges = [];
          const negs = [];
          let i = pos + 1;
          let sawStart = false;
          let uflag = false;
          let escaping = false;
          let negate = false;
          let endPos = pos;
          let rangeStart = "";
          WHILE:
            while (i < glob.length) {
              const c = glob.charAt(i);
              if ((c === "!" || c === "^") && i === pos + 1) {
                negate = true;
                i++;
                continue;
              }
              if (c === "]" && sawStart && !escaping) {
                endPos = i + 1;
                break;
              }
              sawStart = true;
              if (c === "\\") {
                if (!escaping) {
                  escaping = true;
                  i++;
                  continue;
                }
              }
              if (c === "[" && !escaping) {
                for (const [cls, [unip, u, neg]] of Object.entries(posixClasses)) {
                  if (glob.startsWith(cls, i)) {
                    if (rangeStart) {
                      return ["$.", false, glob.length - pos, true];
                    }
                    i += cls.length;
                    if (neg)
                      negs.push(unip);
                    else
                      ranges.push(unip);
                    uflag = uflag || u;
                    continue WHILE;
                  }
                }
              }
              escaping = false;
              if (rangeStart) {
                if (c > rangeStart) {
                  ranges.push(braceEscape(rangeStart) + "-" + braceEscape(c));
                } else if (c === rangeStart) {
                  ranges.push(braceEscape(c));
                }
                rangeStart = "";
                i++;
                continue;
              }
              if (glob.startsWith("-]", i + 1)) {
                ranges.push(braceEscape(c + "-"));
                i += 2;
                continue;
              }
              if (glob.startsWith("-", i + 1)) {
                rangeStart = c;
                i += 2;
                continue;
              }
              ranges.push(braceEscape(c));
              i++;
            }
          if (endPos < i) {
            return ["", false, 0, false];
          }
          if (!ranges.length && !negs.length) {
            return ["$.", false, glob.length - pos, true];
          }
          if (negs.length === 0 && ranges.length === 1 && /^\\?.$/.test(ranges[0]) && !negate) {
            const r = ranges[0].length === 2 ? ranges[0].slice(-1) : ranges[0];
            return [regexpEscape(r), false, endPos - pos, false];
          }
          const sranges = "[" + (negate ? "^" : "") + rangesToString(ranges) + "]";
          const snegs = "[" + (negate ? "" : "^") + rangesToString(negs) + "]";
          const comb = ranges.length && negs.length ? "(" + sranges + "|" + snegs + ")" : ranges.length ? sranges : snegs;
          return [comb, uflag, endPos - pos, true];
        };
        exports2.parseClass = parseClass;
      }
    });
    var require_unescape = __commonJS({
      "dist/commonjs/unescape.js"(exports2) {
        Object.defineProperty(exports2, "__esModule", { value: true });
        exports2.unescape = undefined;
        var unescape = (s, { windowsPathsNoEscape = false, magicalBraces = true } = {}) => {
          if (magicalBraces) {
            return windowsPathsNoEscape ? s.replace(/\[([^/\\])\]/g, "$1") : s.replace(/((?!\\).|^)\[([^/\\])\]/g, "$1$2").replace(/\\([^/])/g, "$1");
          }
          return windowsPathsNoEscape ? s.replace(/\[([^/\\{}])\]/g, "$1") : s.replace(/((?!\\).|^)\[([^/\\{}])\]/g, "$1$2").replace(/\\([^/{}])/g, "$1");
        };
        exports2.unescape = unescape;
      }
    });
    var require_ast = __commonJS({
      "dist/commonjs/ast.js"(exports2) {
        var _a;
        Object.defineProperty(exports2, "__esModule", { value: true });
        exports2.AST = undefined;
        var brace_expressions_js_1 = require_brace_expressions();
        var unescape_js_12 = require_unescape();
        var types = new Set(["!", "?", "+", "*", "@"]);
        var isExtglobType = (c) => types.has(c);
        var isExtglobAST = (c) => isExtglobType(c.type);
        var adoptionMap = new Map([
          ["!", ["@"]],
          ["?", ["?", "@"]],
          ["@", ["@"]],
          ["*", ["*", "+", "?", "@"]],
          ["+", ["+", "@"]]
        ]);
        var adoptionWithSpaceMap = new Map([
          ["!", ["?"]],
          ["@", ["?"]],
          ["+", ["?", "*"]]
        ]);
        var adoptionAnyMap = new Map([
          ["!", ["?", "@"]],
          ["?", ["?", "@"]],
          ["@", ["?", "@"]],
          ["*", ["*", "+", "?", "@"]],
          ["+", ["+", "@", "?", "*"]]
        ]);
        var usurpMap = new Map([
          ["!", new Map([["!", "@"]])],
          [
            "?",
            new Map([
              ["*", "*"],
              ["+", "*"]
            ])
          ],
          [
            "@",
            new Map([
              ["!", "!"],
              ["?", "?"],
              ["@", "@"],
              ["*", "*"],
              ["+", "+"]
            ])
          ],
          [
            "+",
            new Map([
              ["?", "*"],
              ["*", "*"]
            ])
          ]
        ]);
        var startNoTraversal = "(?!(?:^|/)\\.\\.?(?:$|/))";
        var startNoDot = "(?!\\.)";
        var addPatternStart = new Set(["[", "."]);
        var justDots = new Set(["..", "."]);
        var reSpecials = new Set("().*{}+?[]^$\\!");
        var regExpEscape2 = (s) => s.replace(/[-[\]{}()*+?.,\\^$|#\s]/g, "\\$&");
        var qmark2 = "[^/]";
        var star2 = qmark2 + "*?";
        var starNoEmpty = qmark2 + "+?";
        var ID = 0;
        var AST = class {
          type;
          #root;
          #hasMagic;
          #uflag = false;
          #parts = [];
          #parent;
          #parentIndex;
          #negs;
          #filledNegs = false;
          #options;
          #toString;
          #emptyExt = false;
          id = ++ID;
          get depth() {
            return (this.#parent?.depth ?? -1) + 1;
          }
          [Symbol.for("nodejs.util.inspect.custom")]() {
            return {
              "@@type": "AST",
              id: this.id,
              type: this.type,
              root: this.#root.id,
              parent: this.#parent?.id,
              depth: this.depth,
              partsLength: this.#parts.length,
              parts: this.#parts
            };
          }
          constructor(type, parent, options = {}) {
            this.type = type;
            if (type)
              this.#hasMagic = true;
            this.#parent = parent;
            this.#root = this.#parent ? this.#parent.#root : this;
            this.#options = this.#root === this ? options : this.#root.#options;
            this.#negs = this.#root === this ? [] : this.#root.#negs;
            if (type === "!" && !this.#root.#filledNegs)
              this.#negs.push(this);
            this.#parentIndex = this.#parent ? this.#parent.#parts.length : 0;
          }
          get hasMagic() {
            if (this.#hasMagic !== undefined)
              return this.#hasMagic;
            for (const p of this.#parts) {
              if (typeof p === "string")
                continue;
              if (p.type || p.hasMagic)
                return this.#hasMagic = true;
            }
            return this.#hasMagic;
          }
          toString() {
            return this.#toString !== undefined ? this.#toString : !this.type ? this.#toString = this.#parts.map((p) => String(p)).join("") : this.#toString = this.type + "(" + this.#parts.map((p) => String(p)).join("|") + ")";
          }
          #fillNegs() {
            if (this !== this.#root)
              throw new Error("should only call on root");
            if (this.#filledNegs)
              return this;
            this.toString();
            this.#filledNegs = true;
            let n;
            while (n = this.#negs.pop()) {
              if (n.type !== "!")
                continue;
              let p = n;
              let pp = p.#parent;
              while (pp) {
                for (let i = p.#parentIndex + 1;!pp.type && i < pp.#parts.length; i++) {
                  for (const part of n.#parts) {
                    if (typeof part === "string") {
                      throw new Error("string part in extglob AST??");
                    }
                    part.copyIn(pp.#parts[i]);
                  }
                }
                p = pp;
                pp = p.#parent;
              }
            }
            return this;
          }
          push(...parts) {
            for (const p of parts) {
              if (p === "")
                continue;
              if (typeof p !== "string" && !(p instanceof _a && p.#parent === this)) {
                throw new Error("invalid part: " + p);
              }
              this.#parts.push(p);
            }
          }
          toJSON() {
            const ret = this.type === null ? this.#parts.slice().map((p) => typeof p === "string" ? p : p.toJSON()) : [this.type, ...this.#parts.map((p) => p.toJSON())];
            if (this.isStart() && !this.type)
              ret.unshift([]);
            if (this.isEnd() && (this === this.#root || this.#root.#filledNegs && this.#parent?.type === "!")) {
              ret.push({});
            }
            return ret;
          }
          isStart() {
            if (this.#root === this)
              return true;
            if (!this.#parent?.isStart())
              return false;
            if (this.#parentIndex === 0)
              return true;
            const p = this.#parent;
            for (let i = 0;i < this.#parentIndex; i++) {
              const pp = p.#parts[i];
              if (!(pp instanceof _a && pp.type === "!")) {
                return false;
              }
            }
            return true;
          }
          isEnd() {
            if (this.#root === this)
              return true;
            if (this.#parent?.type === "!")
              return true;
            if (!this.#parent?.isEnd())
              return false;
            if (!this.type)
              return this.#parent?.isEnd();
            const pl = this.#parent ? this.#parent.#parts.length : 0;
            return this.#parentIndex === pl - 1;
          }
          copyIn(part) {
            if (typeof part === "string")
              this.push(part);
            else
              this.push(part.clone(this));
          }
          clone(parent) {
            const c = new _a(this.type, parent);
            for (const p of this.#parts) {
              c.copyIn(p);
            }
            return c;
          }
          static #parseAST(str, ast, pos, opt, extDepth) {
            const maxDepth = opt.maxExtglobRecursion ?? 2;
            let escaping = false;
            let inBrace = false;
            let braceStart = -1;
            let braceNeg = false;
            if (ast.type === null) {
              let i2 = pos;
              let acc2 = "";
              while (i2 < str.length) {
                const c = str.charAt(i2++);
                if (escaping || c === "\\") {
                  escaping = !escaping;
                  acc2 += c;
                  continue;
                }
                if (inBrace) {
                  if (i2 === braceStart + 1) {
                    if (c === "^" || c === "!") {
                      braceNeg = true;
                    }
                  } else if (c === "]" && !(i2 === braceStart + 2 && braceNeg)) {
                    inBrace = false;
                  }
                  acc2 += c;
                  continue;
                } else if (c === "[") {
                  inBrace = true;
                  braceStart = i2;
                  braceNeg = false;
                  acc2 += c;
                  continue;
                }
                const doRecurse = !opt.noext && isExtglobType(c) && str.charAt(i2) === "(" && extDepth <= maxDepth;
                if (doRecurse) {
                  ast.push(acc2);
                  acc2 = "";
                  const ext2 = new _a(c, ast);
                  i2 = _a.#parseAST(str, ext2, i2, opt, extDepth + 1);
                  ast.push(ext2);
                  continue;
                }
                acc2 += c;
              }
              ast.push(acc2);
              return i2;
            }
            let i = pos + 1;
            let part = new _a(null, ast);
            const parts = [];
            let acc = "";
            while (i < str.length) {
              const c = str.charAt(i++);
              if (escaping || c === "\\") {
                escaping = !escaping;
                acc += c;
                continue;
              }
              if (inBrace) {
                if (i === braceStart + 1) {
                  if (c === "^" || c === "!") {
                    braceNeg = true;
                  }
                } else if (c === "]" && !(i === braceStart + 2 && braceNeg)) {
                  inBrace = false;
                }
                acc += c;
                continue;
              } else if (c === "[") {
                inBrace = true;
                braceStart = i;
                braceNeg = false;
                acc += c;
                continue;
              }
              const doRecurse = !opt.noext && isExtglobType(c) && str.charAt(i) === "(" && (extDepth <= maxDepth || ast && ast.#canAdoptType(c));
              if (doRecurse) {
                const depthAdd = ast && ast.#canAdoptType(c) ? 0 : 1;
                part.push(acc);
                acc = "";
                const ext2 = new _a(c, part);
                part.push(ext2);
                i = _a.#parseAST(str, ext2, i, opt, extDepth + depthAdd);
                continue;
              }
              if (c === "|") {
                part.push(acc);
                acc = "";
                parts.push(part);
                part = new _a(null, ast);
                continue;
              }
              if (c === ")") {
                if (acc === "" && ast.#parts.length === 0) {
                  ast.#emptyExt = true;
                }
                part.push(acc);
                acc = "";
                ast.push(...parts, part);
                return i;
              }
              acc += c;
            }
            ast.type = null;
            ast.#hasMagic = undefined;
            ast.#parts = [str.substring(pos - 1)];
            return i;
          }
          #canAdoptWithSpace(child) {
            return this.#canAdopt(child, adoptionWithSpaceMap);
          }
          #canAdopt(child, map = adoptionMap) {
            if (!child || typeof child !== "object" || child.type !== null || child.#parts.length !== 1 || this.type === null) {
              return false;
            }
            const gc = child.#parts[0];
            if (!gc || typeof gc !== "object" || gc.type === null) {
              return false;
            }
            return this.#canAdoptType(gc.type, map);
          }
          #canAdoptType(c, map = adoptionAnyMap) {
            return !!map.get(this.type)?.includes(c);
          }
          #adoptWithSpace(child, index) {
            const gc = child.#parts[0];
            const blank = new _a(null, gc, this.options);
            blank.#parts.push("");
            gc.push(blank);
            this.#adopt(child, index);
          }
          #adopt(child, index) {
            const gc = child.#parts[0];
            this.#parts.splice(index, 1, ...gc.#parts);
            for (const p of gc.#parts) {
              if (typeof p === "object")
                p.#parent = this;
            }
            this.#toString = undefined;
          }
          #canUsurpType(c) {
            const m = usurpMap.get(this.type);
            return !!m?.has(c);
          }
          #canUsurp(child) {
            if (!child || typeof child !== "object" || child.type !== null || child.#parts.length !== 1 || this.type === null || this.#parts.length !== 1) {
              return false;
            }
            const gc = child.#parts[0];
            if (!gc || typeof gc !== "object" || gc.type === null) {
              return false;
            }
            return this.#canUsurpType(gc.type);
          }
          #usurp(child) {
            const m = usurpMap.get(this.type);
            const gc = child.#parts[0];
            const nt = m?.get(gc.type);
            if (!nt)
              return false;
            this.#parts = gc.#parts;
            for (const p of this.#parts) {
              if (typeof p === "object") {
                p.#parent = this;
              }
            }
            this.type = nt;
            this.#toString = undefined;
            this.#emptyExt = false;
          }
          static fromGlob(pattern, options = {}) {
            const ast = new _a(null, undefined, options);
            _a.#parseAST(pattern, ast, 0, options, 0);
            return ast;
          }
          toMMPattern() {
            if (this !== this.#root)
              return this.#root.toMMPattern();
            const glob = this.toString();
            const [re, body, hasMagic, uflag] = this.toRegExpSource();
            const anyMagic = hasMagic || this.#hasMagic || this.#options.nocase && !this.#options.nocaseMagicOnly && glob.toUpperCase() !== glob.toLowerCase();
            if (!anyMagic) {
              return body;
            }
            const flags = (this.#options.nocase ? "i" : "") + (uflag ? "u" : "");
            return Object.assign(new RegExp(`^${re}$`, flags), {
              _src: re
            });
          }
          get options() {
            return this.#options;
          }
          toRegExpSource(allowDot) {
            const dot = allowDot ?? !!this.#options.dot;
            if (this.#root === this) {
              this.#flatten();
              this.#fillNegs();
            }
            if (!isExtglobAST(this)) {
              const noEmpty = this.isStart() && this.isEnd() && !this.#parts.some((s) => typeof s !== "string");
              const src = this.#parts.map((p) => {
                const [re, _, hasMagic, uflag] = typeof p === "string" ? _a.#parseGlob(p, this.#hasMagic, noEmpty) : p.toRegExpSource(allowDot);
                this.#hasMagic = this.#hasMagic || hasMagic;
                this.#uflag = this.#uflag || uflag;
                return re;
              }).join("");
              let start2 = "";
              if (this.isStart()) {
                if (typeof this.#parts[0] === "string") {
                  const dotTravAllowed = this.#parts.length === 1 && justDots.has(this.#parts[0]);
                  if (!dotTravAllowed) {
                    const aps = addPatternStart;
                    const needNoTrav = dot && aps.has(src.charAt(0)) || src.startsWith("\\.") && aps.has(src.charAt(2)) || src.startsWith("\\.\\.") && aps.has(src.charAt(4));
                    const needNoDot = !dot && !allowDot && aps.has(src.charAt(0));
                    start2 = needNoTrav ? startNoTraversal : needNoDot ? startNoDot : "";
                  }
                }
              }
              let end = "";
              if (this.isEnd() && this.#root.#filledNegs && this.#parent?.type === "!") {
                end = "(?:$|\\/)";
              }
              const final2 = start2 + src + end;
              return [final2, (0, unescape_js_12.unescape)(src), this.#hasMagic = !!this.#hasMagic, this.#uflag];
            }
            const repeated = this.type === "*" || this.type === "+";
            const start = this.type === "!" ? "(?:(?!(?:" : "(?:";
            let body = this.#partsToRegExp(dot);
            if (this.isStart() && this.isEnd() && !body && this.type !== "!") {
              const s = this.toString();
              const me = this;
              me.#parts = [s];
              me.type = null;
              me.#hasMagic = undefined;
              return [s, (0, unescape_js_12.unescape)(this.toString()), false, false];
            }
            let bodyDotAllowed = !repeated || allowDot || dot || !startNoDot ? "" : this.#partsToRegExp(true);
            if (bodyDotAllowed === body) {
              bodyDotAllowed = "";
            }
            if (bodyDotAllowed) {
              body = `(?:${body})(?:${bodyDotAllowed})*?`;
            }
            let final = "";
            if (this.type === "!" && this.#emptyExt) {
              final = (this.isStart() && !dot ? startNoDot : "") + starNoEmpty;
            } else {
              const close = this.type === "!" ? "))" + (this.isStart() && !dot && !allowDot ? startNoDot : "") + star2 + ")" : this.type === "@" ? ")" : this.type === "?" ? ")?" : this.type === "+" && bodyDotAllowed ? ")" : this.type === "*" && bodyDotAllowed ? `)?` : `)${this.type}`;
              final = start + body + close;
            }
            return [final, (0, unescape_js_12.unescape)(body), this.#hasMagic = !!this.#hasMagic, this.#uflag];
          }
          #flatten() {
            if (!isExtglobAST(this)) {
              for (const p of this.#parts) {
                if (typeof p === "object") {
                  p.#flatten();
                }
              }
            } else {
              let iterations = 0;
              let done = false;
              do {
                done = true;
                for (let i = 0;i < this.#parts.length; i++) {
                  const c = this.#parts[i];
                  if (typeof c === "object") {
                    c.#flatten();
                    if (this.#canAdopt(c)) {
                      done = false;
                      this.#adopt(c, i);
                    } else if (this.#canAdoptWithSpace(c)) {
                      done = false;
                      this.#adoptWithSpace(c, i);
                    } else if (this.#canUsurp(c)) {
                      done = false;
                      this.#usurp(c);
                    }
                  }
                }
              } while (!done && ++iterations < 10);
            }
            this.#toString = undefined;
          }
          #partsToRegExp(dot) {
            return this.#parts.map((p) => {
              if (typeof p === "string") {
                throw new Error("string type in extglob ast??");
              }
              const [re, _, _hasMagic, uflag] = p.toRegExpSource(dot);
              this.#uflag = this.#uflag || uflag;
              return re;
            }).filter((p) => !(this.isStart() && this.isEnd()) || !!p).join("|");
          }
          static #parseGlob(glob, hasMagic, noEmpty = false) {
            let escaping = false;
            let re = "";
            let uflag = false;
            let inStar = false;
            for (let i = 0;i < glob.length; i++) {
              const c = glob.charAt(i);
              if (escaping) {
                escaping = false;
                re += (reSpecials.has(c) ? "\\" : "") + c;
                continue;
              }
              if (c === "*") {
                if (inStar)
                  continue;
                inStar = true;
                re += noEmpty && /^[*]+$/.test(glob) ? starNoEmpty : star2;
                hasMagic = true;
                continue;
              } else {
                inStar = false;
              }
              if (c === "\\") {
                if (i === glob.length - 1) {
                  re += "\\\\";
                } else {
                  escaping = true;
                }
                continue;
              }
              if (c === "[") {
                const [src, needUflag, consumed, magic] = (0, brace_expressions_js_1.parseClass)(glob, i);
                if (consumed) {
                  re += src;
                  uflag = uflag || needUflag;
                  i += consumed - 1;
                  hasMagic = hasMagic || magic;
                  continue;
                }
              }
              if (c === "?") {
                re += qmark2;
                hasMagic = true;
                continue;
              }
              re += regExpEscape2(c);
            }
            return [re, (0, unescape_js_12.unescape)(glob), !!hasMagic, uflag];
          }
        };
        exports2.AST = AST;
        _a = AST;
      }
    });
    var require_escape = __commonJS({
      "dist/commonjs/escape.js"(exports2) {
        Object.defineProperty(exports2, "__esModule", { value: true });
        exports2.escape = undefined;
        var escape = (s, { windowsPathsNoEscape = false, magicalBraces = false } = {}) => {
          if (magicalBraces) {
            return windowsPathsNoEscape ? s.replace(/[?*()[\]{}]/g, "[$&]") : s.replace(/[?*()[\]\\{}]/g, "\\$&");
          }
          return windowsPathsNoEscape ? s.replace(/[?*()[\]]/g, "[$&]") : s.replace(/[?*()[\]\\]/g, "\\$&");
        };
        exports2.escape = escape;
      }
    });
    Object.defineProperty(exports, "__esModule", { value: true });
    exports.unescape = exports.escape = exports.AST = exports.Minimatch = exports.match = exports.makeRe = exports.braceExpand = exports.defaults = exports.filter = exports.GLOBSTAR = exports.sep = exports.minimatch = undefined;
    var brace_expansion_1 = require_commonjs2();
    var assert_valid_pattern_js_1 = require_assert_valid_pattern();
    var ast_js_1 = require_ast();
    var escape_js_1 = require_escape();
    var unescape_js_1 = require_unescape();
    var minimatch = (p, pattern, options = {}) => {
      (0, assert_valid_pattern_js_1.assertValidPattern)(pattern);
      if (!options.nocomment && pattern.charAt(0) === "#") {
        return false;
      }
      return new Minimatch(pattern, options).match(p);
    };
    exports.minimatch = minimatch;
    var starDotExtRE = /^\*+([^+@!?*[(]*)$/;
    var starDotExtTest = (ext2) => (f) => !f.startsWith(".") && f.endsWith(ext2);
    var starDotExtTestDot = (ext2) => (f) => f.endsWith(ext2);
    var starDotExtTestNocase = (ext2) => {
      ext2 = ext2.toLowerCase();
      return (f) => !f.startsWith(".") && f.toLowerCase().endsWith(ext2);
    };
    var starDotExtTestNocaseDot = (ext2) => {
      ext2 = ext2.toLowerCase();
      return (f) => f.toLowerCase().endsWith(ext2);
    };
    var starDotStarRE = /^\*+\.\*+$/;
    var starDotStarTest = (f) => !f.startsWith(".") && f.includes(".");
    var starDotStarTestDot = (f) => f !== "." && f !== ".." && f.includes(".");
    var dotStarRE = /^\.\*+$/;
    var dotStarTest = (f) => f !== "." && f !== ".." && f.startsWith(".");
    var starRE = /^\*+$/;
    var starTest = (f) => f.length !== 0 && !f.startsWith(".");
    var starTestDot = (f) => f.length !== 0 && f !== "." && f !== "..";
    var qmarksRE = /^\?+([^+@!?*[(]*)?$/;
    var qmarksTestNocase = ([m0, ext2 = ""]) => {
      const noext = qmarksTestNoExt([m0]);
      if (!ext2)
        return noext;
      ext2 = ext2.toLowerCase();
      return (f) => noext(f) && f.toLowerCase().endsWith(ext2);
    };
    var qmarksTestNocaseDot = ([m0, ext2 = ""]) => {
      const noext = qmarksTestNoExtDot([m0]);
      if (!ext2)
        return noext;
      ext2 = ext2.toLowerCase();
      return (f) => noext(f) && f.toLowerCase().endsWith(ext2);
    };
    var qmarksTestDot = ([m0, ext2 = ""]) => {
      const noext = qmarksTestNoExtDot([m0]);
      return !ext2 ? noext : (f) => noext(f) && f.endsWith(ext2);
    };
    var qmarksTest = ([m0, ext2 = ""]) => {
      const noext = qmarksTestNoExt([m0]);
      return !ext2 ? noext : (f) => noext(f) && f.endsWith(ext2);
    };
    var qmarksTestNoExt = ([m0]) => {
      const len = m0.length;
      return (f) => f.length === len && !f.startsWith(".");
    };
    var qmarksTestNoExtDot = ([m0]) => {
      const len = m0.length;
      return (f) => f.length === len && f !== "." && f !== "..";
    };
    var defaultPlatform = typeof process === "object" && process ? typeof process.env === "object" && process.env && process.env.__MINIMATCH_TESTING_PLATFORM__ || process.platform : "posix";
    var path = {
      win32: { sep: "\\" },
      posix: { sep: "/" }
    };
    exports.sep = defaultPlatform === "win32" ? path.win32.sep : path.posix.sep;
    exports.minimatch.sep = exports.sep;
    exports.GLOBSTAR = Symbol("globstar **");
    exports.minimatch.GLOBSTAR = exports.GLOBSTAR;
    var qmark = "[^/]";
    var star = qmark + "*?";
    var twoStarDot = "(?:(?!(?:\\/|^)(?:\\.{1,2})($|\\/)).)*?";
    var twoStarNoDot = "(?:(?!(?:\\/|^)\\.).)*?";
    var filter = (pattern, options = {}) => (p) => (0, exports.minimatch)(p, pattern, options);
    exports.filter = filter;
    exports.minimatch.filter = exports.filter;
    var ext = (a, b = {}) => Object.assign({}, a, b);
    var defaults = (def) => {
      if (!def || typeof def !== "object" || !Object.keys(def).length) {
        return exports.minimatch;
      }
      const orig = exports.minimatch;
      const m = (p, pattern, options = {}) => orig(p, pattern, ext(def, options));
      return Object.assign(m, {
        Minimatch: class Minimatch extends orig.Minimatch {
          constructor(pattern, options = {}) {
            super(pattern, ext(def, options));
          }
          static defaults(options) {
            return orig.defaults(ext(def, options)).Minimatch;
          }
        },
        AST: class AST extends orig.AST {
          constructor(type, parent, options = {}) {
            super(type, parent, ext(def, options));
          }
          static fromGlob(pattern, options = {}) {
            return orig.AST.fromGlob(pattern, ext(def, options));
          }
        },
        unescape: (s, options = {}) => orig.unescape(s, ext(def, options)),
        escape: (s, options = {}) => orig.escape(s, ext(def, options)),
        filter: (pattern, options = {}) => orig.filter(pattern, ext(def, options)),
        defaults: (options) => orig.defaults(ext(def, options)),
        makeRe: (pattern, options = {}) => orig.makeRe(pattern, ext(def, options)),
        braceExpand: (pattern, options = {}) => orig.braceExpand(pattern, ext(def, options)),
        match: (list, pattern, options = {}) => orig.match(list, pattern, ext(def, options)),
        sep: orig.sep,
        GLOBSTAR: exports.GLOBSTAR
      });
    };
    exports.defaults = defaults;
    exports.minimatch.defaults = exports.defaults;
    var braceExpand = (pattern, options = {}) => {
      (0, assert_valid_pattern_js_1.assertValidPattern)(pattern);
      if (options.nobrace || !/\{(?:(?!\{).)*\}/.test(pattern)) {
        return [pattern];
      }
      return (0, brace_expansion_1.expand)(pattern, { max: options.braceExpandMax });
    };
    exports.braceExpand = braceExpand;
    exports.minimatch.braceExpand = exports.braceExpand;
    var makeRe = (pattern, options = {}) => new Minimatch(pattern, options).makeRe();
    exports.makeRe = makeRe;
    exports.minimatch.makeRe = exports.makeRe;
    var match = (list, pattern, options = {}) => {
      const mm = new Minimatch(pattern, options);
      list = list.filter((f) => mm.match(f));
      if (mm.options.nonull && !list.length) {
        list.push(pattern);
      }
      return list;
    };
    exports.match = match;
    exports.minimatch.match = exports.match;
    var globMagic = /[?*]|[+@!]\(.*?\)|\[|\]/;
    var regExpEscape = (s) => s.replace(/[-[\]{}()*+?.,\\^$|#\s]/g, "\\$&");
    var Minimatch = class {
      options;
      set;
      pattern;
      windowsPathsNoEscape;
      nonegate;
      negate;
      comment;
      empty;
      preserveMultipleSlashes;
      partial;
      globSet;
      globParts;
      nocase;
      isWindows;
      platform;
      windowsNoMagicRoot;
      maxGlobstarRecursion;
      regexp;
      constructor(pattern, options = {}) {
        (0, assert_valid_pattern_js_1.assertValidPattern)(pattern);
        options = options || {};
        this.options = options;
        this.maxGlobstarRecursion = options.maxGlobstarRecursion ?? 200;
        this.pattern = pattern;
        this.platform = options.platform || defaultPlatform;
        this.isWindows = this.platform === "win32";
        const awe = "allowWindowsEscape";
        this.windowsPathsNoEscape = !!options.windowsPathsNoEscape || options[awe] === false;
        if (this.windowsPathsNoEscape) {
          this.pattern = this.pattern.replace(/\\/g, "/");
        }
        this.preserveMultipleSlashes = !!options.preserveMultipleSlashes;
        this.regexp = null;
        this.negate = false;
        this.nonegate = !!options.nonegate;
        this.comment = false;
        this.empty = false;
        this.partial = !!options.partial;
        this.nocase = !!this.options.nocase;
        this.windowsNoMagicRoot = options.windowsNoMagicRoot !== undefined ? options.windowsNoMagicRoot : !!(this.isWindows && this.nocase);
        this.globSet = [];
        this.globParts = [];
        this.set = [];
        this.make();
      }
      hasMagic() {
        if (this.options.magicalBraces && this.set.length > 1) {
          return true;
        }
        for (const pattern of this.set) {
          for (const part of pattern) {
            if (typeof part !== "string")
              return true;
          }
        }
        return false;
      }
      debug(..._) {}
      make() {
        const pattern = this.pattern;
        const options = this.options;
        if (!options.nocomment && pattern.charAt(0) === "#") {
          this.comment = true;
          return;
        }
        if (!pattern) {
          this.empty = true;
          return;
        }
        this.parseNegate();
        this.globSet = [...new Set(this.braceExpand())];
        if (options.debug) {
          this.debug = (...args) => console.error(...args);
        }
        this.debug(this.pattern, this.globSet);
        const rawGlobParts = this.globSet.map((s) => this.slashSplit(s));
        this.globParts = this.preprocess(rawGlobParts);
        this.debug(this.pattern, this.globParts);
        let set = this.globParts.map((s, _, __) => {
          if (this.isWindows && this.windowsNoMagicRoot) {
            const isUNC = s[0] === "" && s[1] === "" && (s[2] === "?" || !globMagic.test(s[2])) && !globMagic.test(s[3]);
            const isDrive = /^[a-z]:/i.test(s[0]);
            if (isUNC) {
              return [...s.slice(0, 4), ...s.slice(4).map((ss) => this.parse(ss))];
            } else if (isDrive) {
              return [s[0], ...s.slice(1).map((ss) => this.parse(ss))];
            }
          }
          return s.map((ss) => this.parse(ss));
        });
        this.debug(this.pattern, set);
        this.set = set.filter((s) => s.indexOf(false) === -1);
        if (this.isWindows) {
          for (let i = 0;i < this.set.length; i++) {
            const p = this.set[i];
            if (p[0] === "" && p[1] === "" && this.globParts[i][2] === "?" && typeof p[3] === "string" && /^[a-z]:$/i.test(p[3])) {
              p[2] = "?";
            }
          }
        }
        this.debug(this.pattern, this.set);
      }
      preprocess(globParts) {
        if (this.options.noglobstar) {
          for (const partset of globParts) {
            for (let j = 0;j < partset.length; j++) {
              if (partset[j] === "**") {
                partset[j] = "*";
              }
            }
          }
        }
        const { optimizationLevel = 1 } = this.options;
        if (optimizationLevel >= 2) {
          globParts = this.firstPhasePreProcess(globParts);
          globParts = this.secondPhasePreProcess(globParts);
        } else if (optimizationLevel >= 1) {
          globParts = this.levelOneOptimize(globParts);
        } else {
          globParts = this.adjascentGlobstarOptimize(globParts);
        }
        return globParts;
      }
      adjascentGlobstarOptimize(globParts) {
        return globParts.map((parts) => {
          let gs = -1;
          while ((gs = parts.indexOf("**", gs + 1)) !== -1) {
            let i = gs;
            while (parts[i + 1] === "**") {
              i++;
            }
            if (i !== gs) {
              parts.splice(gs, i - gs);
            }
          }
          return parts;
        });
      }
      levelOneOptimize(globParts) {
        return globParts.map((parts) => {
          parts = parts.reduce((set, part) => {
            const prev = set[set.length - 1];
            if (part === "**" && prev === "**") {
              return set;
            }
            if (part === "..") {
              if (prev && prev !== ".." && prev !== "." && prev !== "**") {
                set.pop();
                return set;
              }
            }
            set.push(part);
            return set;
          }, []);
          return parts.length === 0 ? [""] : parts;
        });
      }
      levelTwoFileOptimize(parts) {
        if (!Array.isArray(parts)) {
          parts = this.slashSplit(parts);
        }
        let didSomething = false;
        do {
          didSomething = false;
          if (!this.preserveMultipleSlashes) {
            for (let i = 1;i < parts.length - 1; i++) {
              const p = parts[i];
              if (i === 1 && p === "" && parts[0] === "")
                continue;
              if (p === "." || p === "") {
                didSomething = true;
                parts.splice(i, 1);
                i--;
              }
            }
            if (parts[0] === "." && parts.length === 2 && (parts[1] === "." || parts[1] === "")) {
              didSomething = true;
              parts.pop();
            }
          }
          let dd = 0;
          while ((dd = parts.indexOf("..", dd + 1)) !== -1) {
            const p = parts[dd - 1];
            if (p && p !== "." && p !== ".." && p !== "**" && !(this.isWindows && /^[a-z]:$/i.test(p))) {
              didSomething = true;
              parts.splice(dd - 1, 2);
              dd -= 2;
            }
          }
        } while (didSomething);
        return parts.length === 0 ? [""] : parts;
      }
      firstPhasePreProcess(globParts) {
        let didSomething = false;
        do {
          didSomething = false;
          for (let parts of globParts) {
            let gs = -1;
            while ((gs = parts.indexOf("**", gs + 1)) !== -1) {
              let gss = gs;
              while (parts[gss + 1] === "**") {
                gss++;
              }
              if (gss > gs) {
                parts.splice(gs + 1, gss - gs);
              }
              let next = parts[gs + 1];
              const p = parts[gs + 2];
              const p2 = parts[gs + 3];
              if (next !== "..")
                continue;
              if (!p || p === "." || p === ".." || !p2 || p2 === "." || p2 === "..") {
                continue;
              }
              didSomething = true;
              parts.splice(gs, 1);
              const other = parts.slice(0);
              other[gs] = "**";
              globParts.push(other);
              gs--;
            }
            if (!this.preserveMultipleSlashes) {
              for (let i = 1;i < parts.length - 1; i++) {
                const p = parts[i];
                if (i === 1 && p === "" && parts[0] === "")
                  continue;
                if (p === "." || p === "") {
                  didSomething = true;
                  parts.splice(i, 1);
                  i--;
                }
              }
              if (parts[0] === "." && parts.length === 2 && (parts[1] === "." || parts[1] === "")) {
                didSomething = true;
                parts.pop();
              }
            }
            let dd = 0;
            while ((dd = parts.indexOf("..", dd + 1)) !== -1) {
              const p = parts[dd - 1];
              if (p && p !== "." && p !== ".." && p !== "**") {
                didSomething = true;
                const needDot = dd === 1 && parts[dd + 1] === "**";
                const splin = needDot ? ["."] : [];
                parts.splice(dd - 1, 2, ...splin);
                if (parts.length === 0)
                  parts.push("");
                dd -= 2;
              }
            }
          }
        } while (didSomething);
        return globParts;
      }
      secondPhasePreProcess(globParts) {
        for (let i = 0;i < globParts.length - 1; i++) {
          for (let j = i + 1;j < globParts.length; j++) {
            const matched = this.partsMatch(globParts[i], globParts[j], !this.preserveMultipleSlashes);
            if (matched) {
              globParts[i] = [];
              globParts[j] = matched;
              break;
            }
          }
        }
        return globParts.filter((gs) => gs.length);
      }
      partsMatch(a, b, emptyGSMatch = false) {
        let ai = 0;
        let bi = 0;
        let result = [];
        let which = "";
        while (ai < a.length && bi < b.length) {
          if (a[ai] === b[bi]) {
            result.push(which === "b" ? b[bi] : a[ai]);
            ai++;
            bi++;
          } else if (emptyGSMatch && a[ai] === "**" && b[bi] === a[ai + 1]) {
            result.push(a[ai]);
            ai++;
          } else if (emptyGSMatch && b[bi] === "**" && a[ai] === b[bi + 1]) {
            result.push(b[bi]);
            bi++;
          } else if (a[ai] === "*" && b[bi] && (this.options.dot || !b[bi].startsWith(".")) && b[bi] !== "**") {
            if (which === "b")
              return false;
            which = "a";
            result.push(a[ai]);
            ai++;
            bi++;
          } else if (b[bi] === "*" && a[ai] && (this.options.dot || !a[ai].startsWith(".")) && a[ai] !== "**") {
            if (which === "a")
              return false;
            which = "b";
            result.push(b[bi]);
            ai++;
            bi++;
          } else {
            return false;
          }
        }
        return a.length === b.length && result;
      }
      parseNegate() {
        if (this.nonegate)
          return;
        const pattern = this.pattern;
        let negate = false;
        let negateOffset = 0;
        for (let i = 0;i < pattern.length && pattern.charAt(i) === "!"; i++) {
          negate = !negate;
          negateOffset++;
        }
        if (negateOffset)
          this.pattern = pattern.slice(negateOffset);
        this.negate = negate;
      }
      matchOne(file, pattern, partial = false) {
        let fileStartIndex = 0;
        let patternStartIndex = 0;
        if (this.isWindows) {
          const fileDrive = typeof file[0] === "string" && /^[a-z]:$/i.test(file[0]);
          const fileUNC = !fileDrive && file[0] === "" && file[1] === "" && file[2] === "?" && /^[a-z]:$/i.test(file[3]);
          const patternDrive = typeof pattern[0] === "string" && /^[a-z]:$/i.test(pattern[0]);
          const patternUNC = !patternDrive && pattern[0] === "" && pattern[1] === "" && pattern[2] === "?" && typeof pattern[3] === "string" && /^[a-z]:$/i.test(pattern[3]);
          const fdi = fileUNC ? 3 : fileDrive ? 0 : undefined;
          const pdi = patternUNC ? 3 : patternDrive ? 0 : undefined;
          if (typeof fdi === "number" && typeof pdi === "number") {
            const [fd, pd] = [file[fdi], pattern[pdi]];
            if (fd.toLowerCase() === pd.toLowerCase()) {
              pattern[pdi] = fd;
              patternStartIndex = pdi;
              fileStartIndex = fdi;
            }
          }
        }
        const { optimizationLevel = 1 } = this.options;
        if (optimizationLevel >= 2) {
          file = this.levelTwoFileOptimize(file);
        }
        if (pattern.includes(exports.GLOBSTAR)) {
          return this.#matchGlobstar(file, pattern, partial, fileStartIndex, patternStartIndex);
        }
        return this.#matchOne(file, pattern, partial, fileStartIndex, patternStartIndex);
      }
      #matchGlobstar(file, pattern, partial, fileIndex, patternIndex) {
        const firstgs = pattern.indexOf(exports.GLOBSTAR, patternIndex);
        const lastgs = pattern.lastIndexOf(exports.GLOBSTAR);
        const [head, body, tail] = partial ? [pattern.slice(patternIndex, firstgs), pattern.slice(firstgs + 1), []] : [pattern.slice(patternIndex, firstgs), pattern.slice(firstgs + 1, lastgs), pattern.slice(lastgs + 1)];
        const headLength = head.length;
        if (headLength) {
          const fileHead = file.slice(fileIndex, fileIndex + headLength);
          if (!this.#matchOne(fileHead, head, partial, 0, 0)) {
            return false;
          }
          fileIndex += headLength;
          patternIndex += headLength;
        }
        let fileTailMatch = 0;
        const tailLength = tail.length;
        if (tailLength) {
          if (tailLength + fileIndex > file.length)
            return false;
          let tailStart = file.length - tailLength;
          if (this.#matchOne(file, tail, partial, tailStart, 0)) {
            fileTailMatch = tailLength;
          } else {
            if (file[file.length - 1] !== "" || fileIndex + tailLength === file.length) {
              return false;
            }
            tailStart--;
            if (!this.#matchOne(file, tail, partial, tailStart, 0)) {
              return false;
            }
            fileTailMatch = tailLength + 1;
          }
        }
        if (!body.length) {
          let sawSome = !!fileTailMatch;
          for (let i2 = fileIndex;i2 < file.length - fileTailMatch; i2++) {
            const f = String(file[i2]);
            sawSome = true;
            if (f === "." || f === ".." || !this.options.dot && f.startsWith(".")) {
              return false;
            }
          }
          return partial || sawSome;
        }
        const bodySegments = [[[], 0]];
        let currentBody = bodySegments[0];
        let nonGsParts = 0;
        const nonGsPartsSums = [0];
        for (const b of body) {
          if (b === exports.GLOBSTAR) {
            nonGsPartsSums.push(nonGsParts);
            currentBody = [[], 0];
            bodySegments.push(currentBody);
          } else {
            currentBody[0].push(b);
            nonGsParts++;
          }
        }
        let i = bodySegments.length - 1;
        const fileLength = file.length - fileTailMatch;
        for (const b of bodySegments) {
          b[1] = fileLength - (nonGsPartsSums[i--] + b[0].length);
        }
        return !!this.#matchGlobStarBodySections(file, bodySegments, fileIndex, 0, partial, 0, !!fileTailMatch);
      }
      #matchGlobStarBodySections(file, bodySegments, fileIndex, bodyIndex, partial, globStarDepth, sawTail) {
        const bs = bodySegments[bodyIndex];
        if (!bs) {
          for (let i = fileIndex;i < file.length; i++) {
            sawTail = true;
            const f = file[i];
            if (f === "." || f === ".." || !this.options.dot && f.startsWith(".")) {
              return false;
            }
          }
          return sawTail;
        }
        const [body, after] = bs;
        while (fileIndex <= after) {
          const m = this.#matchOne(file.slice(0, fileIndex + body.length), body, partial, fileIndex, 0);
          if (m && globStarDepth < this.maxGlobstarRecursion) {
            const sub = this.#matchGlobStarBodySections(file, bodySegments, fileIndex + body.length, bodyIndex + 1, partial, globStarDepth + 1, sawTail);
            if (sub !== false) {
              return sub;
            }
          }
          const f = file[fileIndex];
          if (f === "." || f === ".." || !this.options.dot && f.startsWith(".")) {
            return false;
          }
          fileIndex++;
        }
        return partial || null;
      }
      #matchOne(file, pattern, partial, fileIndex, patternIndex) {
        let fi;
        let pi;
        let pl;
        let fl;
        for (fi = fileIndex, pi = patternIndex, fl = file.length, pl = pattern.length;fi < fl && pi < pl; fi++, pi++) {
          this.debug("matchOne loop");
          let p = pattern[pi];
          let f = file[fi];
          this.debug(pattern, p, f);
          if (p === false || p === exports.GLOBSTAR) {
            return false;
          }
          let hit;
          if (typeof p === "string") {
            hit = f === p;
            this.debug("string match", p, f, hit);
          } else {
            hit = p.test(f);
            this.debug("pattern match", p, f, hit);
          }
          if (!hit)
            return false;
        }
        if (fi === fl && pi === pl) {
          return true;
        } else if (fi === fl) {
          return partial;
        } else if (pi === pl) {
          return fi === fl - 1 && file[fi] === "";
        } else {
          throw new Error("wtf?");
        }
      }
      braceExpand() {
        return (0, exports.braceExpand)(this.pattern, this.options);
      }
      parse(pattern) {
        (0, assert_valid_pattern_js_1.assertValidPattern)(pattern);
        const options = this.options;
        if (pattern === "**")
          return exports.GLOBSTAR;
        if (pattern === "")
          return "";
        let m;
        let fastTest = null;
        if (m = pattern.match(starRE)) {
          fastTest = options.dot ? starTestDot : starTest;
        } else if (m = pattern.match(starDotExtRE)) {
          fastTest = (options.nocase ? options.dot ? starDotExtTestNocaseDot : starDotExtTestNocase : options.dot ? starDotExtTestDot : starDotExtTest)(m[1]);
        } else if (m = pattern.match(qmarksRE)) {
          fastTest = (options.nocase ? options.dot ? qmarksTestNocaseDot : qmarksTestNocase : options.dot ? qmarksTestDot : qmarksTest)(m);
        } else if (m = pattern.match(starDotStarRE)) {
          fastTest = options.dot ? starDotStarTestDot : starDotStarTest;
        } else if (m = pattern.match(dotStarRE)) {
          fastTest = dotStarTest;
        }
        const re = ast_js_1.AST.fromGlob(pattern, this.options).toMMPattern();
        if (fastTest && typeof re === "object") {
          Reflect.defineProperty(re, "test", { value: fastTest });
        }
        return re;
      }
      makeRe() {
        const regexp = this.regexp;
        if (regexp || regexp === false)
          return regexp;
        const set = this.set;
        if (!set.length) {
          this.regexp = false;
          return this.regexp;
        }
        const options = this.options;
        const twoStar = options.noglobstar ? star : options.dot ? twoStarDot : twoStarNoDot;
        const flags = new Set(options.nocase ? ["i"] : []);
        let re = set.map((pattern) => {
          const pp = pattern.map((p) => {
            if (p instanceof RegExp) {
              for (const f of p.flags.split(""))
                flags.add(f);
            }
            return typeof p === "string" ? regExpEscape(p) : p === exports.GLOBSTAR ? exports.GLOBSTAR : p._src;
          });
          pp.forEach((p, i) => {
            const next = pp[i + 1];
            const prev = pp[i - 1];
            if (p !== exports.GLOBSTAR || prev === exports.GLOBSTAR) {
              return;
            }
            if (prev === undefined) {
              if (next !== undefined && next !== exports.GLOBSTAR) {
                pp[i + 1] = "(?:\\/|" + twoStar + "\\/)?" + next;
              } else {
                pp[i] = twoStar;
              }
            } else if (next === undefined) {
              pp[i - 1] = prev + "(?:\\/|\\/" + twoStar + ")?";
            } else {
              const GLOBSTAR = exports.GLOBSTAR;
              if (next !== GLOBSTAR) {
                pp[i - 1] = prev + "(?:\\/|\\/" + twoStar + "\\/)" + next;
                pp[i + 1] = GLOBSTAR;
              }
            }
          });
          const filtered = pp.filter((p) => p !== exports.GLOBSTAR);
          let filteredLength;
          if (this.partial && (filteredLength = filtered.length) >= 1) {
            const prefixes = [];
            for (let i = 1;i <= filteredLength; i++) {
              prefixes.push(filtered.slice(0, i).join("/"));
            }
            return "(?:" + prefixes.join("|") + ")";
          }
          return filtered.join("/");
        }).join("|");
        const [open, close] = set.length > 1 ? ["(?:", ")"] : ["", ""];
        re = "^" + open + re + close + "$";
        if (this.partial) {
          re = "^(?:\\/|" + open + re.slice(1, -1) + close + ")$";
        }
        if (this.negate)
          re = "^(?!" + re + ").+$";
        try {
          this.regexp = new RegExp(re, [...flags].join(""));
        } catch {
          this.regexp = false;
        }
        return this.regexp;
      }
      slashSplit(p) {
        if (this.preserveMultipleSlashes) {
          return p.split("/");
        } else if (this.isWindows && /^\/\/[^/]+/.test(p)) {
          return ["", ...p.split(/\/+/)];
        } else {
          return p.split(/\/+/);
        }
      }
      match(f, partial = this.partial) {
        this.debug("match", f, this.pattern);
        if (this.comment) {
          return false;
        }
        if (this.empty) {
          return f === "";
        }
        if (f === "/" && partial) {
          return true;
        }
        const options = this.options;
        if (this.isWindows) {
          f = f.split("\\").join("/");
        }
        const ff = this.slashSplit(f);
        this.debug(this.pattern, "split", ff);
        const set = this.set;
        this.debug(this.pattern, "set", set);
        let filename = ff[ff.length - 1];
        if (!filename) {
          for (let i = ff.length - 2;!filename && i >= 0; i--) {
            filename = ff[i];
          }
        }
        for (const pattern of set) {
          let file = ff;
          if (options.matchBase && pattern.length === 1) {
            file = [filename];
          }
          const hit = this.matchOne(file, pattern, partial);
          if (hit) {
            if (options.flipNegate) {
              return true;
            }
            return !this.negate;
          }
        }
        if (options.flipNegate) {
          return false;
        }
        return this.negate;
      }
      static defaults(def) {
        return exports.minimatch.defaults(def).Minimatch;
      }
    };
    exports.Minimatch = Minimatch;
    var ast_js_2 = require_ast();
    Object.defineProperty(exports, "AST", {
      enumerable: true,
      get: function() {
        return ast_js_2.AST;
      }
    });
    var escape_js_2 = require_escape();
    Object.defineProperty(exports, "escape", {
      enumerable: true,
      get: function() {
        return escape_js_2.escape;
      }
    });
    var unescape_js_2 = require_unescape();
    Object.defineProperty(exports, "unescape", {
      enumerable: true,
      get: function() {
        return unescape_js_2.unescape;
      }
    });
    exports.minimatch.AST = ast_js_1.AST;
    exports.minimatch.Minimatch = Minimatch;
    exports.minimatch.escape = escape_js_1.escape;
    exports.minimatch.unescape = unescape_js_1.unescape;
    return _minimatch = module.exports;
  }

  // Os despachantes de `fs.ts` e `fs.promises.ts` do bun.
  const { validateFunction } = validators;

  function arrayGlobSync(pattern, options) {
    return Array.from(globModule.globSync(pattern, options ?? kEmptyObject));
  }

  function callbackGlob(pattern, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    validateFunction(callback, "callback");
    // `process.nextTick` para que um `throw` do callback saia como exceção não capturada em vez de rejeitar a cadeia
    // interna (e nunca voltar ao `callback` como erro), como no Node.
    Array.fromAsync(globModule.glob(pattern, options ?? kEmptyObject)).then(
      result => process.nextTick(callback, null, result),
      error => process.nextTick(callback, error),
    );
  }

  async function* promisesGlob(pattern, options) {
    yield* new globModule.Glob(pattern, options).glob();
  }

  return { Glob: globModule.Glob, globSync: arrayGlobSync, glob: callbackGlob, promisesGlob, createMatcher };
})
