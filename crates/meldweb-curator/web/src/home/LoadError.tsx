import {
  type ErrorComponentProps,
  Link,
  useLocation,
  useRouter,
} from "@tanstack/react-router";
import { useEffect } from "react";
import { LoggedOutError } from "../github/api";
import { forgetSession } from "../github/session";
import { SessionGate } from "./SessionGate";

/**
 * What a route shows when its loader throws. The login dying mid-page is the
 * login gate, back to this page; anything else (the worker down, GitHub
 * refusing) says what happened and offers to try again.
 */
export function LoadError({ error }: ErrorComponentProps) {
  const router = useRouter();
  const here = useLocation().href;
  const loggedOut = error instanceof LoggedOutError;
  // The kept session is `open`, which is no longer true.
  useEffect(() => {
    if (loggedOut) forgetSession();
  }, [loggedOut]);

  if (loggedOut) {
    return <SessionGate session={{ kind: "logged-out" }} returnPath={here} />;
  }
  return (
    <main className="home">
      <h1>Meldweb Curator</h1>
      <p className="refusal" role="alert">
        {error instanceof Error ? error.message : String(error)}
      </p>
      <button
        type="button"
        onClick={() => {
          forgetSession();
          void router.invalidate();
        }}
      >
        Try again
      </button>{" "}
      <Link to="/">← Decks</Link>
    </main>
  );
}
