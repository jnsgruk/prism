import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useState } from "react";

import type { SourceConfig } from "@ps/api/gen/canonical/prism/v1/config_pb";
import { useSetSecret } from "@ps/hooks/use-config";

import { SECRET_KEYS_BY_TYPE, baseSourceType } from "@/views/admin/lib/source-types";

const SECRET_LABELS: Record<string, string> = {
  api_token: "API Token",
  email: "Email",
  api_key: "API Key",
  api_username: "API Username",
  oauth_token: "OAuth Token",
  service_account_key: "Service Account Key",
};

const secretLabel = (key: string): string => SECRET_LABELS[key] ?? key;

/** Live secret form — sets secrets immediately via the API (requires existing source). */
export const SecretForm = ({ source }: { source: SourceConfig }): React.ReactElement => {
  const setSecret = useSetSecret();
  const secretKeys = SECRET_KEYS_BY_TYPE[baseSourceType(source.sourceType)] ?? ["api_token"];
  const [values, setValues] = useState<Record<string, string>>({});
  const [savingKey, setSavingKey] = useState<string | null>(null);

  const handleSave = (key: string): void => {
    const value = values[key] ?? "";
    if (!value.trim()) return;
    setSavingKey(key);
    setSecret.mutate(
      { sourceId: source.id, secretKey: key, secretValue: value },
      {
        onSuccess: () => {
          setValues((prev) => ({ ...prev, [key]: "" }));
          setSavingKey(null);
        },
        onSettled: () => setSavingKey(null),
      },
    );
  };

  return (
    <div className="space-y-3">
      {secretKeys.map((key) => (
        <div key={key} className="space-y-2">
          <Label>
            {secretLabel(key)}
            {source.secretStatus[key] && (
              <Badge variant="secondary" className="ml-2">
                set
              </Badge>
            )}
          </Label>
          <div className="flex gap-2">
            <Input
              type="password"
              value={values[key] ?? ""}
              onChange={(e) => setValues((prev) => ({ ...prev, [key]: e.target.value }))}
              placeholder={source.secretStatus[key] ? "Paste new value to update" : "Required"}
              className="font-mono"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => handleSave(key)}
              disabled={savingKey === key || !(values[key] ?? "").trim()}
            >
              {savingKey === key ? "Saving..." : "Save"}
            </Button>
          </div>
        </div>
      ))}

      {setSecret.isError && (
        <Alert variant="destructive">
          {setSecret.error instanceof Error ? setSecret.error.message : "Failed to set secret"}
        </Alert>
      )}
    </div>
  );
};

/** Buffered secret form — stores values in local state for later submission (no source ID needed). */
export const BufferedSecretForm = ({
  sourceType,
  secrets,
  onSecretsChange,
}: {
  sourceType: string;
  secrets: Record<string, string>;
  onSecretsChange: (secrets: Record<string, string>) => void;
}): React.ReactElement => {
  const secretKeys = SECRET_KEYS_BY_TYPE[baseSourceType(sourceType)] ?? ["api_token"];

  if (secretKeys.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">No credentials required for this source type.</p>
    );
  }

  return (
    <div className="space-y-3">
      {secretKeys.map((key) => (
        <div key={key} className="space-y-2">
          <Label>
            {secretLabel(key)}
            {secrets[key] && (
              <Badge variant="secondary" className="ml-2">
                filled
              </Badge>
            )}
          </Label>
          <Input
            type="password"
            value={secrets[key] ?? ""}
            onChange={(e) => onSecretsChange({ ...secrets, [key]: e.target.value })}
            placeholder={`Paste ${secretLabel(key).toLowerCase()}`}
            className="font-mono"
          />
        </div>
      ))}
    </div>
  );
};
