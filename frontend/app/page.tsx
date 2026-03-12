"use client";

import { useRouter } from "next/navigation";

import { useCurrentUser, useSetupStatus } from "@ps/hooks/use-auth";

const DashboardPage = () => {
  const router = useRouter();
  const { data: setupComplete, isLoading: statusLoading } = useSetupStatus();
  const { data: user, isLoading: userLoading, isError: userError } = useCurrentUser();

  if (statusLoading) return null;

  if (setupComplete === false) {
    router.replace("/setup");
    return null;
  }

  if (userLoading) return null;

  if (userError || !user) {
    router.replace("/login");
    return null;
  }

  return (
    <div className="flex min-h-screen items-center justify-center">
      <div className="text-center">
        <h1 className="text-4xl font-bold">Prism</h1>
        <p className="mt-2 text-muted-foreground">Welcome, {user.displayName}</p>
      </div>
    </div>
  );
};

export default DashboardPage;
