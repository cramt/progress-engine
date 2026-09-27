import { type DragEvent, useEffect, useRef } from "react";

/** The drag type a search result carries, so the deck knows it from a card move. */
export const RESULT_TYPE = "application/x-meldweb-result";

/** Starts dragging the card called `name` out of the search results. */
export function startResultDrag(e: DragEvent, name: string): void {
  e.dataTransfer.effectAllowed = "copy";
  e.dataTransfer.setData(RESULT_TYPE, JSON.stringify({ name }));
  e.dataTransfer.setData("text/plain", name);
}

/** The name a result drag carries, or null if the data is not one. */
export function readResultDrag(data: string): string | null {
  try {
    const { name } = JSON.parse(data) as { name?: unknown };
    return typeof name === "string" && name !== "" ? name : null;
  } catch {
    return null;
  }
}

/**
 * Lets a search result be dropped on any stack with a category: an element
 * marked `data-category` (StacksView's stacks). The deck's own drag works in
 * React state; a result comes from outside it, so it is caught on the
 * document instead, and the stack under it is marked `result-over`.
 */
export function useResultDrops(
  onDrop: (name: string, category: string) => void,
): void {
  const latest = useRef(onDrop);
  latest.current = onDrop;
  useEffect(() => {
    let marked: Element | null = null;
    const mark = (el: Element | null) => {
      if (el === marked) return;
      marked?.classList.remove("result-over");
      el?.classList.add("result-over");
      marked = el;
    };
    const stackAt = (e: globalThis.DragEvent) => {
      if (!e.dataTransfer?.types.includes(RESULT_TYPE)) return null;
      const target = e.target instanceof Element ? e.target : null;
      return target?.closest("[data-category]") ?? null;
    };
    const over = (e: globalThis.DragEvent) => {
      const stack = stackAt(e);
      mark(stack);
      if (!stack || !e.dataTransfer) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    };
    const drop = (e: globalThis.DragEvent) => {
      const stack = stackAt(e);
      mark(null);
      const category = stack?.getAttribute("data-category");
      if (!category || !e.dataTransfer) return;
      e.preventDefault();
      const name = readResultDrag(e.dataTransfer.getData(RESULT_TYPE));
      if (name) latest.current(name, category);
    };
    const end = () => mark(null);
    document.addEventListener("dragover", over);
    document.addEventListener("drop", drop);
    document.addEventListener("dragend", end);
    return () => {
      document.removeEventListener("dragover", over);
      document.removeEventListener("drop", drop);
      document.removeEventListener("dragend", end);
      mark(null);
    };
  }, []);
}
