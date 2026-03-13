import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useState } from "react";

import type { SourceConfig } from "@ps/api/gen/prism/v1/config_pb";
import { useSetSecret } from "@ps/hooks/use-config";

import { SECRET_KEYS_BY_TYPE } from "@/views/admin/lib/source-types";

/** Live secret form — sets secrets immediately via the API (requires existing source). */
export const SecretForm = ({ source }: { source: SourceConfig }): React.ReactElement => {
  const setSecret = useSetSecret();
  const secretKeys = SECRET_KEYS_BY_TYPE[source.sourceType] ?? ["api_token"];
  const [selectedKey, setSelectedKey] = useState(secretKeys[0] ?? "api_token");
  const [secretValue, setSecretValue] = useState("");

  const handleSave = (): void => {
    setSecret.mutate(
      { sourceId: source.id, secretKey: selectedKey, secretValue },
      {
        onSuccess: () => {
          setSecretValue("");
        },
      },
    );
  };

  return (
    <div className="space-y-3">
      {secretKeys.length > 1 && (
        <div className="space-y-2">
          <Label>Secret key</Label>
          <Select value={selectedKey} onValueChange={(v) => v !== null && setSelectedKey(v)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {secretKeys.map((k) => (
                <SelectItem key={k} value={k}>
                  {k}
                  {source.secretStatus[k] ? " (set)" : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      <div className="space-y-2">
        <Label>
          {secretKeys.length <= 1 ? selectedKey : "Value"}
          {source.secretStatus[selectedKey] && (
            <Badge variant="secondary" className="ml-2">
              set
            </Badge>
          )}
        </Label>
        <div className="flex gap-2">
          <Input
            type="password"
            value={secretValue}
            onChange={(e) => setSecretValue(e.target.value)}
            placeholder="Paste new value to update"
            className="font-mono"
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleSave}
            disabled={setSecret.isPending || !secretValue.trim()}
          >
            {setSecret.isPending ? "Saving..." : "Save"}
          </Button>
        </div>
      </div>

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
  const secretKeys = SECRET_KEYS_BY_TYPE[sourceType] ?? ["api_token"];
  const [selectedKey, setSelectedKey] = useState(secretKeys[0] ?? "api_token");

  if (secretKeys.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">No credentials required for this source type.</p>
    );
  }

  const currentValue = secrets[selectedKey] ?? "";

  const updateSecret = (key: string, value: string): void => {
    onSecretsChange({ ...secrets, [key]: value });
  };

  return (
    <div className="space-y-3">
      {secretKeys.length > 1 && (
        <div className="space-y-2">
          <Label>Secret key</Label>
          <Select value={selectedKey} onValueChange={(v) => v !== null && setSelectedKey(v)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {secretKeys.map((k) => (
                <SelectItem key={k} value={k}>
                  {k}
                  {secrets[k] ? " (filled)" : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      <div className="space-y-2">
        <Label>
          {secretKeys.length <= 1 ? selectedKey : "Value"}
          {currentValue && (
            <Badge variant="secondary" className="ml-2">
              filled
            </Badge>
          )}
        </Label>
        <Input
          type="password"
          value={currentValue}
          onChange={(e) => updateSecret(selectedKey, e.target.value)}
          placeholder="Paste secret value"
          className="font-mono"
        />
      </div>
    </div>
  );
};
