"use client";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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

  const handleSetup = (e: React.FormEvent) => {
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
          <CardTitle>Welcome to Prism</CardTitle>
          <CardDescription>Create your admin account to get started</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSetup} className="space-y-4">
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
              <Label htmlFor="displayName">Display Name</Label>
              <Input
                id="displayName"
                type="text"
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
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
                minLength={8}
              />
            </div>

            {error && <Alert variant="destructive">{error}</Alert>}

            <Button type="submit" disabled={completeSetup.isPending} className="w-full">
              {completeSetup.isPending ? "Creating..." : "Create Admin Account"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
};

export default SetupPage;
