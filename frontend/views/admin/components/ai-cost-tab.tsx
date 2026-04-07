import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

import type { AiProvider } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { aiProviderLabel } from "@/lib/proto-display";
import { useCostSummary } from "@/views/admin/hooks/use-ai-cost";

const COST_WINDOWS = [
  { key: "1w", label: "Last week", days: 7 },
  { key: "2w", label: "Last two weeks", days: 14 },
  { key: "1m", label: "Last month", days: 30 },
  { key: "1q", label: "Last quarter", days: 90 },
  { key: "1y", label: "Last year", days: 365 },
  { key: "all", label: "All time", days: 0 },
] as const;

export const AiCostSection = (): React.ReactElement => {
  const [windowKey, setWindowKey] = useState("1m");
  const window = COST_WINDOWS.find((w) => w.key === windowKey) ?? COST_WINDOWS[2];
  const days = window.days || 3650;
  const windowLabel = window.label;
  const { data, isLoading } = useCostSummary(days);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const todaySpend = data?.todaySpendUsd ?? 0;
  const totalSpend = (data?.dailySpend ?? []).reduce((sum, d) => sum + d.costUsd, 0);

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-sm font-medium">Usage & Cost</h3>
        <p className="text-xs text-muted-foreground">AI API usage and cost tracking.</p>
      </div>
      <ToggleGroup
        className="h-8 w-full rounded-lg bg-muted p-[3px] text-muted-foreground"
        value={[windowKey]}
        onValueChange={(values) => {
          const selected = values[0];
          if (selected) setWindowKey(selected);
        }}
      >
        {COST_WINDOWS.map((w) => (
          <ToggleGroupItem
            key={w.key}
            value={w.key}
            className="h-[calc(100%-1px)] flex-1 rounded-md bg-transparent px-3 py-0.5 text-sm font-medium text-foreground/60 hover:bg-transparent hover:text-foreground aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-sm"
          >
            {w.label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>

      <div className="grid grid-cols-2 gap-4 lg:grid-cols-3">
        <StatCard label="Today" value={`$${todaySpend.toFixed(2)}`} />
        <StatCard label={`${windowLabel} total`} value={`$${totalSpend.toFixed(2)}`} />
      </div>

      <DailySpendChart data={data?.dailySpend ?? []} windowLabel={windowLabel} />

      <TaskBreakdownTable data={data?.taskBreakdown ?? []} windowLabel={windowLabel} />

      <ModelBreakdownTable data={data?.modelBreakdown ?? []} windowLabel={windowLabel} />
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

// ---------------------------------------------------------------------------
// Daily spend chart
// ---------------------------------------------------------------------------

const DailySpendChart = ({
  data,
  windowLabel,
}: {
  data: { date: string; costUsd: number; requestCount: bigint }[];
  windowLabel: string;
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <CardTitle className="text-base">Daily Spend</CardTitle>
      <CardDescription>Last {windowLabel}</CardDescription>
    </CardHeader>
    <CardContent>
      {data.length === 0 ? (
        <p className="py-8 text-center text-sm text-muted-foreground">No usage data yet</p>
      ) : (
        <ResponsiveContainer width="100%" height={250}>
          <BarChart data={data}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis dataKey="date" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
            <YAxis
              tick={{ fontSize: 12 }}
              className="fill-muted-foreground"
              tickFormatter={(v: number) => `$${v.toFixed(2)}`}
            />
            <Tooltip
              formatter={(value) => [`$${Number(value).toFixed(4)}`, "Cost"]}
              contentStyle={{
                backgroundColor: "hsl(var(--popover))",
                border: "1px solid hsl(var(--border))",
                borderRadius: "var(--radius)",
              }}
            />
            <Bar dataKey="costUsd" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      )}
    </CardContent>
  </Card>
);

// ---------------------------------------------------------------------------
// Task breakdown table
// ---------------------------------------------------------------------------

const TaskBreakdownTable = ({
  data,
  windowLabel,
}: {
  data: {
    taskType: string;
    costUsd: number;
    promptTokens: bigint;
    completionTokens: bigint;
    requestCount: bigint;
  }[];
  windowLabel: string;
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <CardTitle className="text-base">By Task</CardTitle>
      <CardDescription>Last {windowLabel}</CardDescription>
    </CardHeader>
    <CardContent>
      {data.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">No usage data</p>
      ) : (
        <div className="overflow-x-auto rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Task</TableHead>
                <TableHead className="text-right tabular-nums">Cost</TableHead>
                <TableHead className="text-right tabular-nums">Prompt tokens</TableHead>
                <TableHead className="text-right tabular-nums">Completion tokens</TableHead>
                <TableHead className="text-right tabular-nums">Requests</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.map((row) => (
                <TableRow key={row.taskType}>
                  <TableCell>
                    <Badge variant="secondary" className="text-[10px] uppercase">
                      {row.taskType}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    ${row.costUsd.toFixed(4)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {row.promptTokens.toLocaleString()}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {row.completionTokens.toLocaleString()}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {row.requestCount.toLocaleString()}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </CardContent>
  </Card>
);

// ---------------------------------------------------------------------------
// Model breakdown table
// ---------------------------------------------------------------------------

const ModelBreakdownTable = ({
  data,
  windowLabel,
}: {
  data: {
    provider: AiProvider;
    model: string;
    taskType: string;
    costUsd: number;
    promptTokens: bigint;
    completionTokens: bigint;
    requestCount: bigint;
  }[];
  windowLabel: string;
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <CardTitle className="text-base">By Model</CardTitle>
      <CardDescription>Last {windowLabel}</CardDescription>
    </CardHeader>
    <CardContent>
      {data.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">No usage data</p>
      ) : (
        <div className="overflow-x-auto rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Provider</TableHead>
                <TableHead>Model</TableHead>
                <TableHead>Task</TableHead>
                <TableHead className="text-right tabular-nums">Cost</TableHead>
                <TableHead className="text-right tabular-nums">Tokens</TableHead>
                <TableHead className="text-right tabular-nums">Requests</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.map((row, i) => (
                <TableRow key={`${row.provider}-${row.model}-${row.taskType}-${i}`}>
                  <TableCell>{aiProviderLabel(row.provider)}</TableCell>
                  <TableCell className="font-mono text-xs">{row.model}</TableCell>
                  <TableCell>
                    <Badge variant="secondary" className="text-[10px] uppercase">
                      {row.taskType}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    ${row.costUsd.toFixed(4)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {(Number(row.promptTokens) + Number(row.completionTokens)).toLocaleString()}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {row.requestCount.toLocaleString()}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </CardContent>
  </Card>
);
