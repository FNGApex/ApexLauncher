import { createBrowserRouter, Navigate } from "react-router-dom";
import { AppShell } from "@/components/AppShell";
import { Home } from "@/routes/Home";
import { Browse } from "@/routes/Browse";
import { Settings } from "@/routes/Settings";
import { InstanceDetail } from "@/routes/InstanceDetail";
import { InfoTab } from "@/routes/instance-tabs/InfoTab";
import { ModlistTab } from "@/routes/instance-tabs/ModlistTab";
import { TechTab } from "@/routes/instance-tabs/TechTab";
import { JavaTab } from "@/routes/instance-tabs/JavaTab";

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
          { path: "java", element: <JavaTab /> },
        ],
      },
      { path: "browse", element: <Browse /> },
      { path: "settings", element: <Settings /> },
    ],
  },
]);
