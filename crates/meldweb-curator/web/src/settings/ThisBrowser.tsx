import { useRouter } from "@tanstack/react-router";
import { useRef, useState } from "react";
import { identityProblem } from "../deck";
import { forgetSession } from "../github/session";
import { DEFAULT_AUTHOR, savedAuthor, setAuthor } from "../repo/author";
import type { LocalRepo } from "../repo/local";

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * What this browser keeps outside the repo, and the repo itself as a file
 * (ADR-0041): the name commits are by, and the bare repo zipped, to take to
 * another computer or to `git clone` on a desktop.
 */
export function ThisBrowser({ repo }: { repo: LocalRepo }) {
  const router = useRouter();
  const saved = savedAuthor();
  const [name, setName] = useState(saved?.name ?? "");
  const [email, setEmail] = useState(saved?.email ?? "");
  const [kept, setKept] = useState(saved);
  const [notice, setNotice] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const picker = useRef<HTMLInputElement>(null);

  const trimmed = { name: name.trim(), email: email.trim() };
  const problem =
    trimmed.name === "" && trimmed.email === ""
      ? null
      : identityProblem(trimmed.name, trimmed.email);
  const unchanged =
    trimmed.name === (kept?.name ?? "") &&
    trimmed.email === (kept?.email ?? "");

  const keep = () => {
    const next = trimmed.name === "" && trimmed.email === "" ? null : trimmed;
    setAuthor(next);
    setKept(next);
    // The session names the user to a friend by this.
    forgetSession();
    setNotice(
      next
        ? `Commits from this browser are by ${next.name} <${next.email}> from now on.`
        : `Commits from this browser are by ${DEFAULT_AUTHOR.name} <${DEFAULT_AUTHOR.email}> again.`,
    );
  };

  const exportZip = async () => {
    setBusy(true);
    setRefusal(null);
    try {
      const bytes = await repo.exportZip();
      const url = URL.createObjectURL(
        new Blob([bytes as Uint8Array<ArrayBuffer>], {
          type: "application/zip",
        }),
      );
      const a = document.createElement("a");
      a.href = url;
      a.download = `mtg-${new Date().toISOString().slice(0, 10)}.zip`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 60_000);
    } catch (e) {
      setRefusal(`The repo could not be zipped: ${messageOf(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const importZip = async (file: File) => {
    const sure = window.confirm(
      `Replace the repo in this browser with the one in ${file.name}? Every deck, the collection and their history here are replaced. Export this one first to keep it.`,
    );
    if (!sure) return;
    setBusy(true);
    setRefusal(null);
    try {
      await repo.importZip(new Uint8Array(await file.arrayBuffer()));
      forgetSession();
      setNotice(`This browser's repo is now the one in ${file.name}.`);
      await router.invalidate();
    } catch (e) {
      setRefusal(`${file.name} was not imported: ${messageOf(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-rules" aria-labelledby="this-browser">
      <h2 id="this-browser">This browser</h2>
      <p className="settings-lede">
        Your decks and collection are a git repository kept in this browser.
        Every save is a commit, by the name below, which is kept in this browser
        and not in the repo.
      </p>
      <form
        className="settings-author"
        onSubmit={(e) => {
          e.preventDefault();
          if (!problem) keep();
        }}
      >
        <label>
          Name
          <input
            value={name}
            placeholder={DEFAULT_AUTHOR.name}
            autoComplete="name"
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <label>
          Email
          <input
            value={email}
            type="email"
            placeholder={DEFAULT_AUTHOR.email}
            autoComplete="email"
            onChange={(e) => setEmail(e.target.value)}
          />
        </label>
        <button type="submit" disabled={problem !== null || unchanged}>
          Use for commits
        </button>
      </form>
      {problem && (
        <p className="refusal" role="alert">
          {problem}
        </p>
      )}
      {!kept && (
        <p className="settings-source">
          Not set, so commits are by{" "}
          <code>
            {DEFAULT_AUTHOR.name} &lt;{DEFAULT_AUTHOR.email}&gt;
          </code>
          .
        </p>
      )}
      <p className="settings-lede">
        The repo travels as a zip of the bare repository: import it in another
        browser, or unzip it and <code>git clone</code> it on a computer.
      </p>
      <div className="rule-actions">
        <button type="button" disabled={busy} onClick={() => void exportZip()}>
          Export as zip
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => picker.current?.click()}
        >
          Import a zip…
        </button>
        <input
          ref={picker}
          type="file"
          accept=".zip,application/zip"
          hidden
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (file) void importZip(file);
          }}
        />
      </div>
      {notice && (
        <p className="settings-source" role="status">
          {notice}
        </p>
      )}
      {refusal && (
        <p className="refusal" role="alert">
          {refusal}
        </p>
      )}
    </section>
  );
}
