import { createGrpcWebTransport } from "@connectrpc/connect-web";

import { getSessionToken } from "@ps/session";

export const transport = createGrpcWebTransport({
  baseUrl: typeof window !== "undefined" ? "/api" : (process.env.API_URL ?? "http://localhost:8080"),
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
