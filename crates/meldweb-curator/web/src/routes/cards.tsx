import { createFileRoute } from "@tanstack/react-router";
import { CardsPage } from "../cards/CardsPage";
import { listCards } from "../github/cards";
import { connect } from "../github/connect";
import { openSession } from "../github/session";
import { SessionGate } from "../home/SessionGate";

// Custom cards, Mirage Mockery's (ADR-0039).
export const Route = createFileRoute("/cards")({
  validateSearch: (s: Record<string, unknown>): { card?: string } =>
    typeof s.card === "string" ? { card: s.card } : {},
  loader: async () => {
    const session = await openSession();
    if (session.kind !== "open") return { kind: "session" as const, session };
    const { api } = await connect();
    const cards = await listCards(api, session.repo);
    return { kind: "cards" as const, cards, repo: session.repo, api };
  },
  // Never reopen from a cached read: its sha would be stale.
  gcTime: 0,
  component: Cards,
});

function Cards() {
  const data = Route.useLoaderData();
  const { card } = Route.useSearch();
  const navigate = Route.useNavigate();
  if (data.kind === "session") {
    return <SessionGate session={data.session} returnPath="/cards" />;
  }
  return (
    <CardsPage
      api={data.api}
      repo={data.repo}
      cards={data.cards}
      selected={card}
      onSelect={(path) =>
        navigate({ search: path ? { card: path } : {}, replace: true })
      }
    />
  );
}
