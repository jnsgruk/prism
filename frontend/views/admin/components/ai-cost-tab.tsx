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
import { AlertTriangle, Loader2 } from "lucide-react";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

import { useCostSummary } from "@/views/admin/hooks/use-ai-cost";

export const AiCostSection = (): React.ReactElement => {
  const { data, isLoading } = useCostSummary(7);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const todaySpend = data?.todaySpendUsd ?? 0;
  const budgetCap = data?.budgetCapUsd;
  const overBudget = budgetCap != null && todaySpend >= budgetCap;

  return (
    <div className="space-y-6 pt-4">
      <p className="text-sm text-muted-foreground">AI API usage and cost tracking.</p>

      {overBudget && (
        <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-4">
          <AlertTriangle className="size-4 text-destructive" />
          <span className="text-sm font-medium text-destructive">
            Daily budget exceeded — enrichment pipeline paused
          </span>
        </div>
      )}

      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <StatCard label="Today" value={`$${todaySpend.toFixed(2)}`} />
        <StatCard label="Budget cap" value={budgetCap != null ? `$${budgetCap.toFixed(2)}` : "—"} />
        <StatCard
          label="Utilisation"
          value={
            budgetCap != null && budgetCap > 0
              ? `${Math.round((todaySpend / budgetCap) * 100)}%`
              : "—"
          }
        />
        <StatCard
          label="7-day total"
          value={`$${(data?.dailySpend ?? []).reduce((sum, d) => sum + d.costUsd, 0).toFixed(2)}`}
        />
      </div>

      <DailySpendChart data={data?.dailySpend ?? []} />

      <TaskBreakdownTable data={data?.taskBreakdown ?? []} />

      <ModelBreakdownTable data={data?.modelBreakdown ?? []} />
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
}: {
  data: { date: string; costUsd: number; requestCount: bigint }[];
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <CardTitle className="text-base">Daily Spend</CardTitle>
      <CardDescription>Last 7 days</CardDescription>
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
}: {
  data: {
    taskType: string;
    costUsd: number;
    promptTokens: bigint;
    completionTokens: bigint;
    requestCount: bigint;
  }[];
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <CardTitle className="text-base">Today by Task</CardTitle>
    </CardHeader>
    <CardContent>
      {data.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">No usage today</p>
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
}: {
  data: {
    provider: string;
    model: string;
    taskType: string;
    costUsd: number;
    promptTokens: bigint;
    completionTokens: bigint;
    requestCount: bigint;
  }[];
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <CardTitle className="text-base">7-day Breakdown by Model</CardTitle>
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
                  <TableCell>{row.provider}</TableCell>
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
