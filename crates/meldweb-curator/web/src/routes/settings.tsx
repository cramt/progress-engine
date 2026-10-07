import { createFileRoute } from "@tanstack/react-router";
import { connect } from "../github/connect";
import { openSession } from "../github/session";
import { loadSettingsFile } from "../github/settings";
import { SessionGate } from "../home/SessionGate";
import { SettingsEditor } from "../settings/SettingsEditor";

// Curator's own settings, `meldweb.toml` at the Magic repo's root (ADR-0026).
export const Route = createFileRoute("/settings")({
  loader: async () => {
    const session = await openSession();
    if (session.kind !== "open") return { kind: "session" as const, session };
    const { api } = await connect();
    const file = await loadSettingsFile(api, session.repo);
    return { kind: "settings" as const, file, repo: session.repo, api };
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
      api={data.api}
    />
  );
}
