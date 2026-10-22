import { type ReactNode, useId } from "react";
import suture from "./suture.svg?raw";

/**
 * The splash's suture at text size, for anything that waits. Each copy gets
 * its own ids: with the splash's shared ones, the needles and the blade's
 * gradient pointed at whichever copy came first, and drew nothing once it
 * unmounted.
 */
export function Spinner({ size }: { size?: string }) {
  const prefix = useId().replace(/:/g, "");
  return (
    <span
      className="spinner"
      style={size ? { fontSize: size } : undefined}
      // biome-ignore lint/security/noDangerouslySetInnerHtml: our own SVG, the one index.html inlines at build time
      dangerouslySetInnerHTML={{
        __html: suture.replaceAll("pe-", `${prefix}-`),
      }}
    />
  );
}

/** Words about something under way, said in bold beside the suture. */
export function Waiting({ children }: { children: ReactNode }) {
  return (
    <span className="waiting">
      <Spinner />
      {children}
    </span>
  );
}
