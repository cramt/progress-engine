import { useRouter } from "@tanstack/react-router";
import { use, useEffect, useState } from "react";
import { connect } from "../github/connect";
import { newRepoUrl } from "../github/onboarding";
import { forgetSession, type Session } from "../github/session";
import { BrandMark } from "../ui/icons";
import "./home.css";

/** The page before there are decks: the mark and one card in the middle. */
function Gate({ children }: { children: React.ReactNode }) {
  return (
    <main className="gate">
      <div className="gate-card">
        <span className="brand">
          <BrandMark />
          Meldweb Curator
        </span>
        {children}
      </div>
    </main>
  );
}

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
        <Gate>
          <h1>Can't reach your decks</h1>
          <p className="refusal" role="alert">
            {session.message}
          </p>
        </Gate>
      );
  }
}

function Login({ returnPath }: { returnPath: string }) {
  const { auth } = use(connect());
  return (
    <Gate>
      <h1>Your decks, in git</h1>
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
    </Gate>
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
    <Gate>
      <h1>Set up your Magic repo</h1>
      <p>
        Curator keeps your decks in <code>{login}/mtg</code> on GitHub, and can
        reach only that one repository.
      </p>
      <ol className="onboarding">
        <li className={repoExists ? "complete" : undefined}>
          <a
            href={newRepoUrl()}
            target="_blank"
            rel="noopener"
            className={repoExists ? "button" : "button primary"}
            onClick={(e) => {
              e.preventDefault();
              onboarding.newRepo();
              recheck();
            }}
          >
            Create the mtg repository
          </a>{" "}
          {repoExists && <span className="done">{login}/mtg exists</span>}
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
    </Gate>
  );
}
