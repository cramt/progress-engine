import "./sigil.css";

type Colour = "W" | "U" | "B" | "R" | "G";

/** WUBRG order, as Scryfall and every decklist write colour identity. */
const WUBRG: readonly Colour[] = ["W", "U", "B", "R", "G"];

const SVGS = import.meta.glob<string>("../../../../../assets/**/*.svg", {
  eager: true,
  query: "?url",
  import: "default",
});

/**
 * Every colour identity's sigil and the order its wedges run clockwise from
 * the top. The tile's look is settled in
 * assets/README.md, "The tile"; this is that table.
 */
const SIGILS: readonly { name: string; file: string; order: Colour[] }[] = [
  { name: "Colorless", file: "colorless/colorless", order: [] },
  { name: "White", file: "mono/white", order: ["W"] },
  { name: "Blue", file: "mono/blue", order: ["U"] },
  { name: "Black", file: "mono/black", order: ["B"] },
  { name: "Red", file: "mono/red", order: ["R"] },
  { name: "Green", file: "mono/green", order: ["G"] },
  { name: "Azorius", file: "guilds/azorius", order: ["W", "U"] },
  { name: "Dimir", file: "guilds/dimir", order: ["U", "B"] },
  { name: "Rakdos", file: "guilds/rakdos", order: ["B", "R"] },
  { name: "Gruul", file: "guilds/gruul", order: ["R", "G"] },
  { name: "Selesnya", file: "guilds/selesnya", order: ["G", "W"] },
  { name: "Orzhov", file: "guilds/orzhov", order: ["W", "B"] },
  { name: "Izzet", file: "guilds/izzet", order: ["U", "R"] },
  { name: "Golgari", file: "guilds/golgari", order: ["B", "G"] },
  { name: "Boros", file: "guilds/boros", order: ["R", "W"] },
  { name: "Simic", file: "guilds/simic", order: ["G", "U"] },
  { name: "Bant", file: "families/brokers", order: ["G", "W", "U"] },
  { name: "Esper", file: "families/obscura", order: ["W", "U", "B"] },
  { name: "Grixis", file: "families/maestros", order: ["U", "B", "R"] },
  { name: "Jund", file: "families/riveteers", order: ["B", "R", "G"] },
  { name: "Naya", file: "families/cabaretti", order: ["R", "G", "W"] },
  { name: "Abzan", file: "clans/abzan", order: ["W", "B", "G"] },
  { name: "Jeskai", file: "clans/jeskai", order: ["U", "R", "W"] },
  { name: "Sultai", file: "clans/sultai", order: ["B", "G", "U"] },
  { name: "Mardu", file: "clans/mardu", order: ["R", "W", "B"] },
  { name: "Temur", file: "clans/temur", order: ["G", "U", "R"] },
  {
    name: "Sans-white",
    file: "four-color/sans-white",
    order: ["U", "B", "R", "G"],
  },
  {
    name: "Sans-blue",
    file: "four-color/sans-blue",
    order: ["B", "R", "G", "W"],
  },
  {
    name: "Sans-black",
    file: "four-color/sans-black",
    order: ["R", "G", "W", "U"],
  },
  {
    name: "Sans-red",
    file: "four-color/sans-red",
    order: ["G", "W", "U", "B"],
  },
  {
    name: "Sans-green",
    file: "four-color/sans-green",
    order: ["W", "U", "B", "R"],
  },
  {
    name: "Five-color",
    file: "five-color/planeswalker",
    order: ["W", "U", "B", "R", "G"],
  },
];

const FILL: Record<Colour, string> = {
  W: "#f8f0b0",
  U: "#a9d8f2",
  B: "#888",
  R: "#f4a99a",
  G: "#9fd3a8",
};
const COLORLESS = "#cdc6b8";
/** Each seam blends over 22°, 11° either side. */
const BLEND = 11;

/**
 * Equal wedges clockwise from the top, each colour held to 11° short of its
 * seams; the end stops pad past 0° and 360° with the neighbour so the seam at
 * the top blends like the others.
 */
function background(order: Colour[]): string {
  const [first, ...rest] = order;
  if (first === undefined) return COLORLESS;
  const last = rest.at(-1);
  if (last === undefined) return FILL[first];
  const wedge = 360 / order.length;
  const stops = order.map(
    (c, i) =>
      `${FILL[c]} ${i * wedge + BLEND}deg ${(i + 1) * wedge - BLEND}deg`,
  );
  return `conic-gradient(${FILL[last]} -${BLEND}deg, ${stops.join(", ")}, ${FILL[first]} ${360 + BLEND}deg)`;
}

/** The sigil and tile background for a colour identity. */
export function sigilFor(identity: ReadonlySet<string>) {
  const colours = WUBRG.filter((c) => identity.has(c));
  // Every subset of WUBRG has an entry, so this always finds one
  const sigil = SIGILS.find(
    (s) =>
      s.order.length === colours.length &&
      colours.every((c) => s.order.includes(c)),
  );
  if (!sigil) return undefined;
  return {
    ...sigil,
    key: colours.join(""),
    src: SVGS[`../../../../../assets/${sigil.file}.svg`],
    background: background(sigil.order),
  };
}

/** A colour identity's sigil on its tile. */
export function Sigil({ identity }: { identity: ReadonlySet<string> }) {
  const sigil = sigilFor(identity);
  if (!sigil) return null;
  return (
    <span
      className="sigil"
      title={`${sigil.name}${sigil.key ? ` (${sigil.key})` : ""}`}
      style={{ background: sigil.background }}
    >
      <img crossOrigin="anonymous" src={sigil.src} alt={sigil.name} />
    </span>
  );
}
