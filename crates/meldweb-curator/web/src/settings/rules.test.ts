import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { loadDeckSync } from "../deck";
import {
  draftRule,
  fileFor,
  missingDefaults,
  moveRule,
  previewOf,
  problemOf,
  readSettings,
  TERMS,
  writeSettings,
} from "./rules";

beforeAll(() => {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
});

const defaults = () => readSettings(null).defaults;

describe("the settings page's rules", () => {
  it("reads a repo without meldweb.toml as the default rules", () => {
    const read = readSettings(null);
    expect(read.kind).toBe("read");
    if (read.kind !== "read") return;
    expect(read.declared).toBe(false);
    expect(read.rules).toEqual(read.defaults);
  });

  it("keeps a rule that does not parse, so it can be fixed", () => {
    const read = readSettings('[printings]\nrank = [{ avoid = "lang:jp" }]\n');
    expect(read).toMatchObject({
      kind: "read",
      rules: [{ verb: "avoid", query: "lang:jp" }],
    });
    expect(problemOf("lang:jp")).toMatch(/language/);
    expect(problemOf("lang:ja")).toBeNull();
    expect(problemOf("")).not.toBeNull();
  });

  it("refuses a file the format does not allow, defaults beside it", () => {
    const read = readSettings("[printings]\nsort = 1\n");
    expect(read.kind).toBe("refused");
    expect(read.defaults.length).toBeGreaterThan(0);
  });

  it("saves the loaded file untouched until the rules change", () => {
    const text = '# mine\n[printings]\nrank = [{ avoid = "is:ub" }]\n';
    const loaded = {
      text,
      rules: [{ verb: "avoid" as const, query: "is:ub" }],
    };
    expect(fileFor(loaded.rules.map(draftRule), loaded)).toBe(text);
    const flipped = fileFor([{ verb: "prefer", query: "is:ub" }], loaded);
    expect(flipped).toContain('{ prefer = "is:ub" }');
    expect(readSettings(flipped)).toMatchObject({
      rules: [{ verb: "prefer", query: "is:ub" }],
    });
  });

  it("holds the save back while a rule does not parse", () => {
    const loaded = { text: "", rules: defaults() };
    expect(fileFor([{ verb: "avoid", query: "lang:" }], loaded)).toBeNull();
    // A repo without the file, at the default rules, writes nothing.
    expect(fileFor(defaults(), loaded)).toBe("");
  });

  it("previews only the rules that parse, and says where each one is", () => {
    const { text, at } = previewOf([
      { verb: "avoid", query: "is:ub" },
      { verb: "avoid", query: "lang:jp" },
      { verb: "prefer", query: "is:fullart" },
    ]);
    expect(at).toEqual([0, 2]);
    expect(readSettings(text)).toMatchObject({
      rules: [
        { verb: "avoid", query: "is:ub" },
        { verb: "prefer", query: "is:fullart" },
      ],
    });
  });

  it("moves a rule to where it is dropped", () => {
    expect(moveRule(["a", "b", "c", "d"], 0, 2)).toEqual(["b", "c", "a", "d"]);
    expect(moveRule(["a", "b", "c", "d"], 3, 0)).toEqual(["d", "a", "b", "c"]);
    expect(moveRule(["a", "b"], 1, 9)).toEqual(["a", "b"]);
  });

  it("offers back the default rules a draft lacks", () => {
    const all = defaults();
    expect(missingDefaults(all, all)).toEqual([]);
    expect(missingDefaults(all.slice(1), all)).toEqual([all[0]]);
  });

  it("names only printing terms the query reader knows", () => {
    for (const [term] of TERMS)
      expect([term, problemOf(term)]).toEqual([term, null]);
  });

  it("writes queries with quotes in them so they read back", () => {
    const rules = [{ verb: "prefer" as const, query: 'name:"Lim-Dûl"' }];
    expect(readSettings(writeSettings(rules))).toMatchObject({ rules });
  });
});
