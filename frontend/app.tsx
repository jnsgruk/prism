import { lazy, Suspense } from "react";
import { Route, Routes } from "react-router-dom";

import { AppShell } from "@/components/app-shell";

const DashboardPage = lazy(() => import("@/views/dashboard/pages/dashboard-page"));
const TeamsPage = lazy(() => import("@/views/teams/pages/teams-page"));
const AdminPage = lazy(() => import("@/views/admin/pages/admin-page"));
const IngestionPage = lazy(() => import("@/views/ingestion/pages/ingestion-page"));
const LoginPage = lazy(() => import("@/views/login/pages/login-page"));
const SetupPage = lazy(() => import("@/views/setup/pages/setup-page"));
const PeopleListPage = lazy(() => import("@/views/people/pages/people-list-page"));
const PersonProfilePage = lazy(() => import("@/views/people/pages/person-profile-page"));
const ContributionDetailPage = lazy(
  () => import("@/views/contributions/pages/contribution-detail-page"),
);
const AskPage = lazy(() => import("@/views/ask/pages/ask-page"));
const ChatHistoryPage = lazy(() => import("@/views/ask/pages/chat-history-page"));
const NotFoundPage = lazy(() => import("@/views/not-found/pages/not-found-page"));

export const App = (): React.ReactElement => (
  <AppShell>
    <Suspense fallback={null}>
      <Routes>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/teams" element={<TeamsPage />} />
        <Route path="/teams/:teamId" element={<TeamsPage />} />
        <Route path="/people" element={<PeopleListPage />} />
        <Route path="/people/:personId" element={<PersonProfilePage />} />
        <Route path="/admin" element={<AdminPage />} />
        <Route path="/contributions/:contributionId" element={<ContributionDetailPage />} />
        <Route path="/ingestion" element={<IngestionPage />} />
        <Route path="/ask" element={<AskPage />} />
        <Route path="/ask/history" element={<ChatHistoryPage />} />
        <Route path="/ask/:conversationId" element={<AskPage />} />
        <Route path="/login" element={<LoginPage />} />
        <Route path="/setup" element={<SetupPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Routes>
    </Suspense>
  </AppShell>
);
