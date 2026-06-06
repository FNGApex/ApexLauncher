import { createBrowserRouter, Navigate } from "react-router-dom";
import { AppShell } from "@/components/AppShell";
import { Home } from "@/routes/Home";
import { Browse } from "@/routes/Browse";
import { Accounts } from "@/routes/Accounts";
import { Settings } from "@/routes/Settings";
import { InstanceDetail } from "@/routes/InstanceDetail";

export const router = createBrowserRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <Navigate to="/instances" replace /> },
      { path: "instances", element: <Home /> },
      { path: "instances/:slug", element: <InstanceDetail /> },
      { path: "browse", element: <Browse /> },
      { path: "accounts", element: <Accounts /> },
      { path: "settings", element: <Settings /> },
    ],
  },
]);
