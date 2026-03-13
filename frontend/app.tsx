import { lazy, Suspense } from "react";
import { Route, Routes } from "react-router-dom";

import { AppShell } from "@/components/app-shell";

const DashboardPage = lazy(() => import("@/views/dashboard/pages/dashboard-page"));
const TeamsPage = lazy(() => import("@/views/teams/pages/teams-page"));
const AdminPage = lazy(() => import("@/views/admin/pages/admin-page"));
const IngestionPage = lazy(() => import("@/views/ingestion/pages/ingestion-page"));
const LoginPage = lazy(() => import("@/views/login/pages/login-page"));
const SetupPage = lazy(() => import("@/views/setup/pages/setup-page"));

export const App = (): React.ReactElement => (
  <AppShell>
    <Suspense>
      <Routes>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/teams" element={<TeamsPage />} />
        <Route path="/admin" element={<AdminPage />} />
        <Route path="/ingestion" element={<IngestionPage />} />
        <Route path="/login" element={<LoginPage />} />
        <Route path="/setup" element={<SetupPage />} />
      </Routes>
    </Suspense>
  </AppShell>
);
