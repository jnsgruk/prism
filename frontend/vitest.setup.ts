import "@testing-library/jest-dom/vitest";
import { vi } from "vite-plus/test";

// Stub global fetch to prevent happy-dom from making real HTTP requests during
// tests. All RPC calls are mocked at the transport layer, so a real fetch
// reaching the network means something is misconfigured. Returning a 418
// makes accidental leaks easy to spot in test output.
vi.stubGlobal(
  "fetch",
  vi.fn(() => Promise.resolve(new Response(null, { status: 418, statusText: "Teapot" }))),
);
