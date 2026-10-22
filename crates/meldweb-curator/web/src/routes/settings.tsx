import { createFileRoute } from "@tanstack/react-router";
import { connect } from "../github/connect";
import { openSession } from "../github/session";
import { loadSettingsFile } from "../github/settings";
import { SessionGate } from "../home/SessionGate";
import { SettingsEditor } from "../settings/SettingsEditor";

// Curator's own settings, `meldweb.toml` at the Magic repo's root (ADR-0026),
// and what this browser keeps beside the repo: its author and its zip.
export const Route = createFileRoute("/settings")({
  loader: async () => {
    const session = await openSession();
    if (session.kind !== "open") return { kind: "session" as const, session };
    const [file, connection] = await Promise.all([
      loadSettingsFile(session.repo),
      connect(),
    ]);
    return {
      kind: "settings" as const,
      file,
      repo: session.repo,
      local: connection.kind === "local" ? connection.repo : null,
      remote: connection.kind === "local" ? connection.remote : null,
    };
  },
  // Never reopen from a cached read: its sha would be stale.
  gcTime: 0,
  component: SettingsPage,
});

function SettingsPage() {
  const data = Route.useLoaderData();
  if (data.kind === "session") {
    return <SessionGate session={data.session} returnPath="/settings" />;
  }
  return (
    <SettingsEditor
      text={data.file?.text ?? null}
      sha={data.file?.sha ?? null}
      repo={data.repo}
      local={data.local}
      remote={data.remote}
    />
  );
}
