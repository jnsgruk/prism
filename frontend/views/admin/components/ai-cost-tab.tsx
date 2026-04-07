import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Loader2 } from "lucide-react";

import { useUsageSummary } from "@/views/admin/hooks/use-ai-cost";

const USAGE_WINDOWS = [
  { key: "1w", label: "Last week", days: 7 },
  { key: "2w", label: "2 weeks", days: 14 },
  { key: "1m", label: "Month", days: 30 },
  { key: "1q", label: "Quarter", days: 90 },
  { key: "1y", label: "Year", days: 365 },
  { key: "all", label: "All time", days: 0 },
] as const;

const TASK_LABELS: Record<string, string> = {
  enrichment: "Enrichment",
  agentic: "Agentic",
  embeddings: "Embeddings",
  image_generation: "Image Generation",
};

/** Format large numbers with SI suffixes (4,200,000 → "4.2M"). */
const formatCompact = (n: number): string => {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
};

export const AiCostSection = (): React.ReactElement => {
  const [windowKey, setWindowKey] = useState("1m");
  const window = USAGE_WINDOWS.find((w) => w.key === windowKey) ?? USAGE_WINDOWS[2];
  const days = window.days || 3650;
  const { data, isLoading } = useUsageSummary(days);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const tasks = data?.taskBreakdown ?? [];
  const models = data?.modelBreakdown ?? [];

  const totalRequests = tasks.reduce((sum, t) => sum + Number(t.requestCount), 0);
  const totalInput = tasks.reduce((sum, t) => sum + Number(t.promptTokens), 0);
  const totalOutput = tasks.reduce((sum, t) => sum + Number(t.completionTokens), 0);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-medium">Usage</h3>
          <p className="text-xs text-muted-foreground">AI API usage across all tasks.</p>
        </div>
        <ToggleGroup
          className="h-7 rounded-lg bg-muted p-[3px] text-muted-foreground"
          value={[windowKey]}
          onValueChange={(values) => {
            const selected = values[0];
            if (selected) setWindowKey(selected);
          }}
        >
          {USAGE_WINDOWS.map((w) => (
            <ToggleGroupItem
              key={w.key}
              value={w.key}
              className="h-[calc(100%-1px)] rounded-md bg-transparent px-2 py-0.5 text-xs font-medium text-foreground/60 hover:bg-transparent hover:text-foreground aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-sm"
            >
              {w.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      {/* Stat cards */}
      <div className="grid grid-cols-3 gap-4">
        <StatCard label="Requests" value={totalRequests.toLocaleString()} />
        <StatCard label="Input tokens" value={formatCompact(totalInput)} />
        <StatCard label="Output tokens" value={formatCompact(totalOutput)} />
      </div>

      {/* By task */}
      {tasks.length > 0 && (
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">By task</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="overflow-x-auto rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Task</TableHead>
                    <TableHead className="text-right tabular-nums">Input tokens</TableHead>
                    <TableHead className="text-right tabular-nums">Output tokens</TableHead>
                    <TableHead className="text-right tabular-nums">Requests</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {tasks.map((row) => (
                    <TableRow key={row.taskType}>
                      <TableCell>
                        <Badge variant="secondary" className="text-[10px] uppercase">
                          {TASK_LABELS[row.taskType] ?? row.taskType}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {Number(row.promptTokens) > 0
                          ? Number(row.promptTokens).toLocaleString()
                          : "—"}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {Number(row.completionTokens) > 0
                          ? Number(row.completionTokens).toLocaleString()
                          : "—"}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {Number(row.requestCount).toLocaleString()}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </CardContent>
        </Card>
      )}

      {/* By model */}
      {models.length > 0 && (
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">By model</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="overflow-x-auto rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Model</TableHead>
                    <TableHead>Task</TableHead>
                    <TableHead className="text-right tabular-nums">Tokens</TableHead>
                    <TableHead className="text-right tabular-nums">Requests</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {models.map((row, i) => (
                    <TableRow key={`${row.model}-${row.taskType}-${i}`}>
                      <TableCell className="font-mono text-xs">{row.model}</TableCell>
                      <TableCell>
                        <Badge variant="secondary" className="text-[10px] uppercase">
                          {TASK_LABELS[row.taskType] ?? row.taskType}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {Number(row.promptTokens) + Number(row.completionTokens) > 0
                          ? (
                              Number(row.promptTokens) + Number(row.completionTokens)
                            ).toLocaleString()
                          : "—"}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {Number(row.requestCount).toLocaleString()}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Stat card
// ---------------------------------------------------------------------------

const StatCard = ({ label, value }: { label: string; value: string }): React.ReactElement => (
  <Card>
    <CardContent className="p-4">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="tabular-nums text-2xl font-semibold">{value}</p>
    </CardContent>
  </Card>
);
