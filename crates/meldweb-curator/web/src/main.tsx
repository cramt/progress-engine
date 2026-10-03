import { createRouter, RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { LoadError } from "./home/LoadError";
import { routeTree } from "./routeTree.gen";
import { Loading } from "./ui/Loading";
import "./styles.css";

// Every route's loader can meet a login that died or a worker that is down.
const router = createRouter({
  routeTree,
  defaultErrorComponent: LoadError,
  defaultPendingComponent: Loading,
  // A quick navigation keeps the page it is leaving rather than flashing the
  // splash; the first load has nothing to keep and shows it almost at once.
  defaultPendingMs: 200,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root");
createRoot(root).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
);
