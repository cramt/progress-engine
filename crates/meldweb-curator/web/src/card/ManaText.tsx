/**
 * Text with Scryfall's `{G}`-style symbols drawn as Scryfall draws them. The
 * file is the symbol without its braces and slashes, `{G/U/P}` is `GUP.svg`,
 * and svgs.scryfall.io answers CORS, so it loads under require-corp.
 */
export function ManaText({ text }: { text: string }) {
  return (
    <>
      {text.split(/(\{[^}]+\})/).map((part, i) => {
        const symbol = /^\{([^}]+)\}$/.exec(part)?.[1];
        if (symbol === undefined) return part;
        return (
          <img
            crossOrigin="anonymous"
            // The split's order is fixed for a given text.
            // biome-ignore lint/suspicious/noArrayIndexKey: see above
            key={i}
            className="mana"
            src={`https://svgs.scryfall.io/card-symbols/${symbol.replaceAll("/", "")}.svg`}
            alt={part}
            title={part}
          />
        );
      })}
    </>
  );
}
