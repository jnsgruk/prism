import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { z } from "zod";

import { PrismLogo } from "@/components/prism-logo";
import { useCompleteSetup, useSetupStatus } from "@ps/hooks/use-auth";

const setupSchema = z.object({
  username: z.string().min(1, "Username is required"),
  displayName: z.string().min(1, "Display name is required"),
  password: z.string().min(8, "Password must be at least 8 characters"),
});

const SetupPage = (): React.ReactElement | null => {
  const navigate = useNavigate();
  const { data: setupComplete, isLoading: statusLoading } = useSetupStatus();
  const completeSetup = useCompleteSetup();

  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    if (!statusLoading && setupComplete) {
      navigate("/login", { replace: true });
    }
  }, [statusLoading, setupComplete, navigate]);

  if (statusLoading || setupComplete) return null;

  const handleSetup = (e: React.FormEvent): void => {
    e.preventDefault();
    setError("");

    const result = setupSchema.safeParse({ username, displayName, password });
    if (!result.success) {
      setError(result.error.issues[0]?.message ?? "Invalid input");
      return;
    }

    completeSetup.mutate(result.data, {
      onSuccess: () => navigate("/", { replace: true }),
      onError: (err) => setError(err.message),
    });
  };

  return (
    <div className="space-y-6">
      <div className="text-center">
        <div className="mb-3 flex justify-center">
          <PrismLogo size={48} />
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
