import type { Interceptor } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";

import { getSessionToken } from "@ps/session";

const authInterceptor: Interceptor = (next) => async (req) => {
  const token = getSessionToken();
  if (token) {
    req.header.set("Authorization", `Bearer ${token}`);
  }
  return next(req);
};

export const transport = createGrpcWebTransport({
  baseUrl:
    typeof window !== "undefined" ? "/api" : (process.env.API_URL ?? "http://localhost:8080"),
  interceptors: [authInterceptor],
});
