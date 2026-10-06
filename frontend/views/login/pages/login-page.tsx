import { PrismLogo } from "@/components/prism-logo";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { useLogin, useSetupStatus } from "@ps/hooks/use-auth";

const LoginPage = (): React.ReactElement | null => {
  const navigate = useNavigate();
  const location = useLocation();
  const requestedDestination: unknown = location.state?.returnTo;
  const returnTo =
    typeof requestedDestination === "string" &&
    requestedDestination.startsWith("/") &&
    !requestedDestination.startsWith("//") &&
    !requestedDestination.includes("\\")
      ? requestedDestination
      : "/";
  const { data: setupComplete, isLoading: statusLoading } = useSetupStatus();
  const login = useLogin();

  const [error, setError] = useState("");

  useEffect(() => {
    if (!statusLoading && setupComplete === false) {
      navigate("/setup", { replace: true });
    }
  }, [statusLoading, setupComplete, navigate]);

  if (statusLoading || setupComplete === false) return null;

  const handleLogin = (e: React.FormEvent<HTMLFormElement>): void => {
    e.preventDefault();
    setError("");

    const formData = new FormData(e.currentTarget);
    const username = formData.get("username");
    const password = formData.get("password");
    if (typeof username !== "string" || typeof password !== "string") return;

    login.mutate(
      { username, password },
      {
        onSuccess: () => navigate(returnTo, { replace: true }),
        onError: (err) => setError(err.message),
      },
    );
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
          <CardTitle>Sign in to Prism</CardTitle>
          <CardDescription>Enter your credentials to continue</CardDescription>
        </CardHeader>
        <CardContent>
          <form id="login" method="post" onSubmit={handleLogin} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                name="username"
                type="text"
                autoComplete="username"
                autoCapitalize="none"
                spellCheck={false}
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="password">Password</Label>
              <Input id="password" name="password" type="password" autoComplete="current-password" required />
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
