import { createBrowserRouter, Navigate } from "react-router-dom";
import { AppShell } from "@/components/AppShell";
import { Home } from "@/routes/Home";
import { BrowseProvider } from "@/routes/Browse";
import { BrowsePackInfo } from "@/routes/BrowsePackInfo";
import { Settings } from "@/routes/Settings";
import { InstanceDetail } from "@/routes/InstanceDetail";
import { InfoTab } from "@/routes/instance-tabs/InfoTab";
import { ModlistTab } from "@/routes/instance-tabs/ModlistTab";
import { TechTab } from "@/routes/instance-tabs/TechTab";
import { useUiStore } from "@/lib/store";

/** Reads the persisted last-used browse provider and redirects to /browse/<p>. */
function BrowseRedirect() {
  const p = useUiStore.getState().browseProvider;
  return <Navigate to={`/browse/${p}`} replace />;
}

export const router = createBrowserRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <Navigate to="/instances" replace /> },
      { path: "instances", element: <Home /> },
      {
        path: "instances/:slug",
        element: <InstanceDetail />,
        children: [
          { index: true, element: <Navigate to="info" replace /> },
          { path: "info", element: <InfoTab /> },
          { path: "modlist", element: <ModlistTab /> },
          { path: "tech", element: <TechTab /> },
        ],
      },
      { path: "browse", element: <BrowseRedirect /> },
      { path: "browse/:provider", element: <BrowseProvider /> },
      { path: "browse/:provider/:id", element: <BrowsePackInfo /> },
      { path: "settings", element: <Settings /> },
    ],
  },
]);
