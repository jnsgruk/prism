import { SidebarProvider } from "@/components/ui/sidebar";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, type RenderOptions, type RenderResult } from "@testing-library/react";
import { afterEach } from "vitest";

export const createTestQueryClient = (): QueryClient =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

export const TestWrapper = ({ children }: { children: React.ReactNode }): React.ReactElement => {
  const queryClient = createTestQueryClient();
  return (
    <QueryClientProvider client={queryClient}>
      <SidebarProvider>{children}</SidebarProvider>
    </QueryClientProvider>
  );
};

/**
 * Custom render that wraps components in the standard test provider stack
 * (QueryClient + SidebarProvider). Use this instead of importing render
 * directly from @testing-library/react.
 */
export const renderWithProviders = (
  ui: React.ReactElement,
  options?: Omit<RenderOptions, "wrapper">,
): RenderResult => render(ui, { wrapper: TestWrapper, ...options });

/** Call in describe() blocks to auto-cleanup after each test. */
export const setupCleanup = (): void => {
  afterEach(cleanup);
};
