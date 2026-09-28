import { createRouter, RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { loadDeck } from "./deck";
import { LoadError } from "./home/LoadError";
import { routeTree } from "./routeTree.gen";
import "./styles.css";

// Every route's loader can meet a login that died or a worker that is down.
const router = createRouter({ routeTree, defaultErrorComponent: LoadError });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

await loadDeck();

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root");
createRoot(root).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
);
