"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

import { useLogin, useSetupStatus } from "@ps/hooks/use-auth";

const LoginPage = () => {
  const router = useRouter();
  const { data: setupComplete, isLoading: statusLoading } = useSetupStatus();
  const login = useLogin();

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");

  if (statusLoading) return null;

  if (setupComplete === false) {
    router.replace("/setup");
    return null;
  }

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");

    login.mutate(
      { username, password },
      {
        onSuccess: () => router.replace("/"),
        onError: (err) => setError(err.message),
      },
    );
  };

  return (
    <div className="flex min-h-screen items-center justify-center">
      <div className="w-full max-w-md space-y-6 rounded-lg border p-8">
        <div className="text-center">
          <h1 className="text-2xl font-bold">Sign in to Prism</h1>
        </div>

        <form onSubmit={handleLogin} className="space-y-4">
          <div>
            <label htmlFor="username" className="text-sm font-medium">
              Username
            </label>
            <input
              id="username"
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              className="mt-1 block w-full rounded border px-3 py-2"
              required
            />
          </div>

          <div>
            <label htmlFor="password" className="text-sm font-medium">
              Password
            </label>
            <input
              id="password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="mt-1 block w-full rounded border px-3 py-2"
              required
            />
          </div>

          {error && <p className="text-sm text-red-600">{error}</p>}

          <button
            type="submit"
            disabled={login.isPending}
            className="w-full rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50"
          >
            {login.isPending ? "Signing in..." : "Sign In"}
          </button>
        </form>
      </div>
    </div>
  );
};

export default LoginPage;
