import { describe, expect, it } from "vitest";
import {
  addSymbol,
  costSymbols,
  dropSymbol,
  genericOf,
  insertAt,
  toggleColor,
  toggleType,
  withGeneric,
} from "./cardForm";

describe("the form's mana cost buttons", () => {
  it("read a cost as its symbols", () => {
    expect(costSymbols("{2}{U}{U/P}")).toEqual(["2", "U", "U/P"]);
    expect(costSymbols("")).toEqual([]);
  });

  it("keep the generic part one number, after X and before colours", () => {
    expect(genericOf("{X}{2}{U}")).toBe(2);
    expect(withGeneric("{X}{U}", 3)).toBe("{X}{3}{U}");
    expect(withGeneric("{2}{U}", 0)).toBe("{U}");
    expect(withGeneric("", 1)).toBe("{1}");
  });

  it("add a colour at the end and X at the front, and drop by place", () => {
    expect(addSymbol("{1}{U}", "U")).toBe("{1}{U}{U}");
    expect(addSymbol("{1}{U}", "X")).toBe("{X}{1}{U}");
    expect(dropSymbol("{U}{U}{B}", 1)).toBe("{U}{B}");
  });
});

describe("the form's type buttons", () => {
  it("put a word where Oracle text puts it, and take it out again", () => {
    expect(toggleType("Creature — Elf", "Legendary")).toBe(
      "Legendary Creature — Elf",
    );
    expect(toggleType("Creature — Elf", "Artifact")).toBe(
      "Artifact Creature — Elf",
    );
    expect(toggleType("Artifact Creature — Elf", "Artifact")).toBe(
      "Creature — Elf",
    );
    expect(toggleType("", "Instant")).toBe("Instant");
  });
});

describe("the rules text's symbol buttons", () => {
  it("put the symbol over the selection and the caret after it", () => {
    expect(insertAt("Pay : draw", "{T}", 4, 4)).toEqual({
      text: "Pay {T}: draw",
      caret: 7,
    });
  });
});

describe("the form's colour indicator buttons", () => {
  it("toggle a colour, keeping WUBRG order", () => {
    expect(toggleColor("", "R")).toBe("R");
    expect(toggleColor("R", "W")).toBe("WR");
    expect(toggleColor("WR", "R")).toBe("W");
  });
});
