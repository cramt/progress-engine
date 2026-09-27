import { useRouter } from "@tanstack/react-router";
import { use, useEffect, useState } from "react";
import { connect } from "../github/connect";
import { newRepoUrl } from "../github/onboarding";
import { forgetSession, type Session } from "../github/session";

/** What stands between the user and their decks, for any session but `open`. */
export function SessionGate({
  session,
  returnPath,
}: {
  session: Exclude<Session, { kind: "open" }>;
  /** Where logging in comes back to. */
  returnPath: string;
}) {
  switch (session.kind) {
    case "logged-out":
      return <Login returnPath={returnPath} />;
    case "onboarding":
      return (
        <Onboarding login={session.login} repoExists={session.repoExists} />
      );
    case "refused":
      return (
        <main className="home">
          <h1>Meldweb Curator</h1>
          <p className="refusal" role="alert">
            {session.message}
          </p>
        </main>
      );
  }
}

function Login({ returnPath }: { returnPath: string }) {
  const { auth } = use(connect());
  return (
    <main className="home">
      <h1>Meldweb Curator</h1>
      <p>
        Your decks live in a GitHub repository named mtg, and Curator commits
        every change to it by itself.
      </p>
      <button
        type="button"
        className="primary"
        onClick={() => auth.login(returnPath)}
      >
        Log in with GitHub
      </button>
    </main>
  );
}

/**
 * The two trips to github.com: make `mtg`, then install the app on it alone.
 * Coming back to the tab (or pressing Check again) looks again.
 */
function Onboarding({
  login,
  repoExists,
}: {
  login: string;
  repoExists: boolean;
}) {
  const { onboarding } = use(connect());
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const recheck = () => {
    forgetSession();
    void router.invalidate();
  };
  useEffect(() => {
    const onFocus = () => {
      forgetSession();
      void router.invalidate();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [router]);

  return (
    <main className="home">
      <h1>Set up your Magic repo</h1>
      <p>
        Curator keeps your decks in <code>{login}/mtg</code> on GitHub, and can
        reach only that one repository.
      </p>
      <ol className="onboarding">
        <li>
          <a
            href={newRepoUrl()}
            target="_blank"
            rel="noopener"
            className="button primary"
            onClick={(e) => {
              e.preventDefault();
              onboarding.newRepo();
              recheck();
            }}
          >
            Create the mtg repository
          </a>{" "}
          {repoExists && <span className="done">✓ {login}/mtg exists</span>}
          <p className="hint">
            Opens GitHub's new-repository page with the name filled in. Public
            or private is up to you.
          </p>
        </li>
        <li>
          <button
            type="button"
            className="primary"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await onboarding.install();
                recheck();
              } finally {
                setBusy(false);
              }
            }}
          >
            Install Curator on mtg
          </button>
          <p className="hint">
            Choose <em>Only select repositories</em> and pick <code>mtg</code>.
          </p>
        </li>
      </ol>
      <button type="button" onClick={recheck}>
        Check again
      </button>
    </main>
  );
}
