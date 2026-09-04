// The API a criteria file sees.
//
// `count` is the only way to look at what was drawn, and it deliberately returns
// a number rather than cards. That restriction is what lets the engine answer
// exactly instead of by simulation: criteria depending only on counts can be
// evaluated once per possible composition, not once per shuffled hand.
//
// Two kinds of question can be registered. `criterion` asks whether something
// held and is answered with a probability; `expect` asks how many and is
// answered with a mean and the distribution behind it. They are kept in separate
// lists and their return values are type-checked separately, because the one way
// to get this wrong is to write one and mean the other, and a silent coercion
// would turn that mistake into a plausible number instead of an error.
globalThis.__criteria = [];
globalThis.__expectations = [];

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

// No options argument, and no assertion. `atLeast` on a criterion is a threshold
// on a probability; on an expectation it would be a threshold in the units of
// whatever is being counted, and one keyword meaning two things is the unstated
// definition this tool exists to prevent. See the Expectation type in
// pe-criteria for what the assertion probably wants to be instead.
globalThis.expect = (name, fn) => {
  if (typeof name !== "string" || name.length === 0) {
    throw new TypeError("expect(name, fn): name must be a non-empty string");
  }
  if (typeof fn !== "function") {
    throw new TypeError(`expect(${JSON.stringify(name)}): second argument must be a function`);
  }
  globalThis.__expectations.push({ name, fn });
};

// `t(n)` is the state after turn n's draw; t(0) is the opening hand.
const turn = (n) => ({
  count: (query) => Deno.core.ops.op_pe_count(n, String(query)),
});

const describe = (v) => {
  if (v === null) return "null";
  if (Array.isArray(v)) return "an array";
  if (typeof v === "number") return `the number ${v}`;
  if (typeof v === "string") return `the string ${JSON.stringify(v)}`;
  return `a ${typeof v}`;
};

globalThis.__meta = () => ({
  criteria: globalThis.__criteria.map((c) => ({ name: c.name, atLeast: c.atLeast })),
  expectations: globalThis.__expectations.map((e) => ({ name: e.name })),
});

// Criteria first, then expectations, in registration order within each. Rust
// splits the array at the criterion count it read from __meta.
globalThis.__evaluate = () => {
  const out = [];
  for (const c of globalThis.__criteria) {
    const v = c.fn(turn);
    if (typeof v !== "boolean") {
      throw new TypeError(
        `criterion(${JSON.stringify(c.name)}) returned ${describe(v)}; a criterion must ` +
          `return true or false. To report how many rather than how often, register it ` +
          `with expect() instead.`,
      );
    }
    out.push(v);
  }
  for (const e of globalThis.__expectations) {
    const v = e.fn(turn);
    if (typeof v !== "number") {
      throw new TypeError(
        `expect(${JSON.stringify(e.name)}) returned ${describe(v)}; an expectation must ` +
          `return a number. To report how often rather than how many, register it with ` +
          `criterion() instead.`,
      );
    }
    out.push(v);
  }
  return out;
};
