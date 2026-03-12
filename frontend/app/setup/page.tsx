"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

import { useCompleteSetup, useSetupStatus } from "@ps/hooks/use-auth";

const SetupPage = () => {
  const router = useRouter();
  const { data: setupComplete, isLoading: statusLoading } = useSetupStatus();
  const completeSetup = useCompleteSetup();

  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");

  if (statusLoading) return null;

  if (setupComplete) {
    router.replace("/login");
    return null;
  }

  const handleSetup = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");

    completeSetup.mutate(
      { username, displayName, password },
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
          <h1 className="text-2xl font-bold">Welcome to Prism</h1>
          <p className="mt-1 text-sm text-muted-foreground">Create your admin account to get started</p>
        </div>

        <form onSubmit={handleSetup} className="space-y-4">
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
            <label htmlFor="displayName" className="text-sm font-medium">
              Display Name
            </label>
            <input
              id="displayName"
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
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
              minLength={8}
            />
          </div>

          {error && <p className="text-sm text-red-600">{error}</p>}

          <button
            type="submit"
            disabled={completeSetup.isPending}
            className="w-full rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50"
          >
            {completeSetup.isPending ? "Creating..." : "Create Admin Account"}
          </button>
        </form>
      </div>
    </div>
  );
};

export default SetupPage;
