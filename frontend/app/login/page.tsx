"use client";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { useLogin, useSetupStatus } from "@ps/hooks/use-auth";

const LoginPage = (): React.ReactElement | null => {
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

  const handleLogin = (e: React.FormEvent): void => {
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
    <div className="space-y-6">
      <div className="text-center">
        <div className="mx-auto mb-3 flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
          <svg width="20" height="20" viewBox="0 0 16 16" fill="none">
            <path d="M8 1L14 4.5V11.5L8 15L2 11.5V4.5L8 1Z" fill="currentColor" fillOpacity="0.9" />
            <path d="M8 1L14 4.5L8 8L2 4.5L8 1Z" fill="currentColor" />
          </svg>
        </div>
        <p className="text-sm text-muted-foreground">Engineering Insights Platform</p>
      </div>

      <Card>
        <CardHeader className="text-center">
          <CardTitle>Sign in to Prism</CardTitle>
          <CardDescription>Enter your credentials to continue</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleLogin} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </div>

            {error && <Alert variant="destructive">{error}</Alert>}

            <Button type="submit" disabled={login.isPending} className="w-full">
              {login.isPending ? "Signing in..." : "Sign In"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
};

export default LoginPage;
