import { useEffect } from "react";

import { AppSidebar } from "@/components/app-sidebar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { Skeleton } from "@/components/ui/skeleton";
import { useLocation, useNavigate } from "react-router-dom";

import { useCurrentUser, useSetupStatus } from "@ps/hooks/use-auth";

const PUBLIC_ROUTES = ["/login", "/setup"];

const LoadingSkeleton = (): React.ReactElement => (
  <div className="flex min-h-screen">
    <div className="w-64 border-r bg-sidebar p-4">
      <Skeleton className="mb-6 h-6 w-24" />
      <div className="space-y-2">
        <Skeleton className="h-8 w-full" />
        <Skeleton className="h-8 w-full" />
        <Skeleton className="h-8 w-full" />
      </div>
    </div>
    <div className="flex-1 p-8">
      <Skeleton className="mb-4 h-8 w-48" />
      <Skeleton className="h-64 w-full" />
    </div>
  </div>
);

export const AppShell = ({
  children,
}: {
  children: React.ReactNode;
}): React.ReactElement | null => {
  const { pathname } = useLocation();
  const navigate = useNavigate();
  const isPublicRoute = PUBLIC_ROUTES.some((route) => pathname.startsWith(route));

  const { data: setupComplete, isLoading: setupLoading } = useSetupStatus();
  const { data: user, isLoading: userLoading, isError: userError } = useCurrentUser();

  const needsSetup = !isPublicRoute && !setupLoading && !userLoading && setupComplete === false;
  const needsLogin = !isPublicRoute && !setupLoading && !userLoading && !needsSetup && (userError || !user);

  useEffect(() => {
    if (needsSetup) navigate("/setup", { replace: true });
  }, [needsSetup, navigate]);

  useEffect(() => {
    if (needsLogin) navigate("/login", { replace: true });
  }, [needsLogin, navigate]);

  // Public routes render without the shell
  if (isPublicRoute) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-muted/30">
        <div className="w-full max-w-md px-4">{children}</div>
      </div>
    );
  }

  // Loading state for authenticated routes
  if (setupLoading || userLoading) return <LoadingSkeleton />;

  // Waiting for redirect
  if (needsSetup || needsLogin) return null;

  return (
    <SidebarProvider>
      <AppSidebar user={user!} />
      <SidebarInset>{children}</SidebarInset>
    </SidebarProvider>
  );
};
