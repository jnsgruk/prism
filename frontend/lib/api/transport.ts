import { createConnectTransport } from "@connectrpc/connect-web";

import { getSessionToken } from "@ps/session";

export const transport = createConnectTransport({
  baseUrl: typeof window !== "undefined" ? window.location.origin : (process.env.API_URL ?? "http://localhost:8080"),
  interceptors: [
    (next) => async (req) => {
      const token = getSessionToken();
      if (token) {
        req.header.set("Authorization", `Bearer ${token}`);
      }
      return next(req);
    },
  ],
});
