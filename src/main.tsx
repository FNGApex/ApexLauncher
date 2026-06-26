import React from "react";
import ReactDOM from "react-dom/client";
import { RouterProvider } from "react-router-dom";
import { QueryClientProvider } from "@tanstack/react-query";
import { attachConsole } from "@tauri-apps/plugin-log";
import { router } from "./router";
import { queryClient } from "@/lib/query";
import { prefetchStartupData } from "@/lib/prefetch";
import { applyTheme, subscribeSystem } from "@/lib/theme";
import "./styles.css";

// Forward Rust log output to the webview devtools console (fire-and-forget).
attachConsole();

// Apply the saved theme (idempotent with the index.html no-flash script) and
// keep "system" in sync with the OS scheme for the app's lifetime.
applyTheme();
subscribeSystem();

// Kick off startup prefetch as early as possible (fire-and-forget).
prefetchStartupData(queryClient);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </React.StrictMode>,
);
