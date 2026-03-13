import { create } from "@bufbuild/protobuf";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { Period } from "@ps/api/gen/prism/v1/metrics_pb";
import { PeriodSchema, PeriodType } from "@ps/api/gen/prism/v1/metrics_pb";

const PERIOD_TYPE_LABELS: Record<number, string> = {
  [PeriodType.WEEK]: "Week",
  [PeriodType.MONTH]: "Month",
  [PeriodType.QUARTER]: "Quarter",
};

/** Build default periods: current + previous for each period type. */
const buildDefaultPeriods = (): Period[] => {
  const now = new Date();
  const periods: Period[] = [];

  // Weeks (current + last 3)
  for (let i = 0; i < 4; i++) {
    const d = new Date(now);
    d.setDate(d.getDate() - i * 7);
    const day = d.getDay();
    const monday = new Date(d);
    monday.setDate(d.getDate() - ((day + 6) % 7));
    const sunday = new Date(monday);
    sunday.setDate(monday.getDate() + 6);
    periods.push(
      create(PeriodSchema, {
        type: PeriodType.WEEK,
        start: fmt(monday),
        end: fmt(sunday),
      }),
    );
  }

  // Months (current + last 3)
  for (let i = 0; i < 4; i++) {
    const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
    const end = new Date(d.getFullYear(), d.getMonth() + 1, 0);
    periods.push(
      create(PeriodSchema, {
        type: PeriodType.MONTH,
        start: fmt(d),
        end: fmt(end),
      }),
    );
  }

  // Quarters (current + last 1)
  for (let i = 0; i < 2; i++) {
    const qMonth = Math.floor(now.getMonth() / 3) * 3 - i * 3;
    const d = new Date(now.getFullYear(), qMonth, 1);
    const end = new Date(d.getFullYear(), d.getMonth() + 3, 0);
    periods.push(
      create(PeriodSchema, {
        type: PeriodType.QUARTER,
        start: fmt(d),
        end: fmt(end),
      }),
    );
  }

  return periods;
};

const fmt = (d: Date): string => d.toISOString().slice(0, 10);

const formatPeriodLabel = (p: Period): string => {
  const typeLabel = PERIOD_TYPE_LABELS[p.type] ?? "Period";
  return `${typeLabel}: ${p.start} — ${p.end}`;
};

const periodToKey = (p: Period): string => `${p.type}-${p.start}`;

export const PeriodSelector = ({
  value,
  onChange,
}: {
  value: Period;
  onChange: (period: Period) => void;
}): React.ReactElement => {
  const periods = buildDefaultPeriods();
  const currentKey = periodToKey(value);

  return (
    <Select
      value={currentKey}
      onValueChange={(key) => {
        const selected = periods.find((p) => periodToKey(p) === key);
        if (selected) onChange(selected);
      }}
    >
      <SelectTrigger className="w-[280px]">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {periods.map((p) => (
          <SelectItem key={periodToKey(p)} value={periodToKey(p)}>
            {formatPeriodLabel(p)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
};
