import { describe, expect, it } from "vitest";
import { actionForKey, isTyping } from "./hotkeys";

describe("hover hotkeys", () => {
  it("are archidekt's keys, either case", () => {
    expect(actionForKey({ key: "+" })).toEqual({ kind: "increase" });
    expect(actionForKey({ key: "=" })).toEqual({ kind: "increase" });
    expect(actionForKey({ key: "-" })).toEqual({ kind: "decrease" });
    expect(actionForKey({ key: "r" })).toEqual({ kind: "remove" });
    expect(actionForKey({ key: "R" })).toEqual({ kind: "remove" });
    expect(actionForKey({ key: "a" })).toEqual({ kind: "automatic" });
    expect(actionForKey({ key: "m" })).toEqual({
      kind: "board",
      type: "maybeboard",
    });
    expect(actionForKey({ key: "S" })).toEqual({
      kind: "board",
      type: "sideboard",
    });
    expect(actionForKey({ key: "c" })).toEqual({ kind: "copy-name" });
    expect(actionForKey({ key: "p" })).toEqual({ kind: "printing" });
  });

  it("leave every other key and every chord alone, so ctrl+z and ctrl+c still undo and copy", () => {
    expect(actionForKey({ key: "x" })).toBeNull();
    expect(actionForKey({ key: "Enter" })).toBeNull();
    expect(actionForKey({ key: "z", ctrlKey: true })).toBeNull();
    expect(actionForKey({ key: "c", ctrlKey: true })).toBeNull();
    expect(actionForKey({ key: "c", metaKey: true })).toBeNull();
    expect(actionForKey({ key: "r", altKey: true })).toBeNull();
  });

  it("do not fire while typing in an input, a textarea, a select or contenteditable", () => {
    expect(isTyping({ tagName: "INPUT" })).toBe(true);
    expect(isTyping({ tagName: "textarea" })).toBe(true);
    expect(isTyping({ tagName: "SELECT" })).toBe(true);
    expect(isTyping({ tagName: "DIV", isContentEditable: true })).toBe(true);
    expect(isTyping({ tagName: "DIV", isContentEditable: false })).toBe(false);
    expect(isTyping({ tagName: "BODY" })).toBe(false);
    expect(isTyping(null)).toBe(false);
  });
});
