import type { ExportTarget } from "../deck";

/** What each Copy as button says and does, in the order they are offered. */
export const EXPORT_TARGETS: readonly {
  target: ExportTarget;
  label: string;
  title: string;
}[] = [
  {
    target: "archidekt",
    label: "Archidekt",
    title: "Archidekt text, with each card's categories, for its Import",
  },
  {
    target: "cockatrice",
    label: "Cockatrice",
    title:
      "For Cockatrice's Load deck from clipboard, with printings; the commander goes in the sideboard",
  },
  {
    target: "cardmarket",
    label: "Cardmarket",
    title:
      "For Add Deck List on a Cardmarket wants list: every card to buy, by name",
  },
];
