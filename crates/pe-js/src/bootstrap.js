// The API a criteria file sees.
//
// `count` is the only way to look at what was drawn, and it deliberately returns
// a number rather than cards. That restriction is what lets the engine answer
// exactly instead of by simulation: criteria depending only on counts can be
// evaluated once per possible composition, not once per shuffled hand.
globalThis.__criteria = [];

globalThis.criterion = (name, fn, opts) => {
  if (typeof name !== "string" || name.length === 0) {
    throw new TypeError("criterion(name, fn, opts): name must be a non-empty string");
  }
  if (typeof fn !== "function") {
    throw new TypeError(`criterion(${JSON.stringify(name)}): second argument must be a function`);
  }
  const atLeast = opts && opts.atLeast !== undefined ? opts.atLeast : null;
  if (atLeast !== null && (typeof atLeast !== "number" || atLeast < 0 || atLeast > 1)) {
    throw new TypeError(
      `criterion(${JSON.stringify(name)}): atLeast must be a number between 0 and 1`,
    );
  }
  globalThis.__criteria.push({ name, fn, atLeast });
};

// `t(n)` is the state after turn n's draw; t(0) is the opening hand.
const turn = (n) => ({
  count: (query) => Deno.core.ops.op_pe_count(n, String(query)),
});

globalThis.__meta = () =>
  globalThis.__criteria.map((c) => ({ name: c.name, atLeast: c.atLeast }));

globalThis.__evaluate = () => globalThis.__criteria.map((c) => Boolean(c.fn(turn)));
