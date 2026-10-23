const { C } = require('./cases.js');
const out = C.map(c => {
  const input = eval(c.input);
  let res;
  try {
    const re = new RegExp(c.pattern, c.flags);
    if (c.flags.includes('g') || c.flags.includes('y')) re.lastIndex = c.start;
    const m = re.exec(input);
    if (!m) res = null;
    else {
      const ind = new RegExp(c.pattern, c.flags.includes('d') ? c.flags : c.flags + 'd');
      if (c.flags.includes('g') || c.flags.includes('y')) ind.lastIndex = c.start;
      const mi = ind.exec(input);
      res = { caps: Array.from(mi.indices, r => r ? [r[0], r[1]] : null),
              groups: mi.indices.groups ? Object.entries(mi.indices.groups).map(([k, r]) => [k, r ? [r[0], r[1]] : null]) : [],
              lastIndex: re.lastIndex };
    }
  } catch (e) { res = { error: String(e.message) }; }
  return { ...c, units: Array.from({ length: input.length }, (_, i) => input.charCodeAt(i)), res };
});
console.log(JSON.stringify(out));
