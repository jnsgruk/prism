import { create } from "@bufbuild/protobuf";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { Period } from "@ps/api/gen/canonical/prism/v1/metrics_pb";
import { PeriodSchema, PeriodType } from "@ps/api/gen/canonical/prism/v1/metrics_pb";

const fmt = (d: Date): string => d.toISOString().slice(0, 10);

type PeriodPreset = {
  key: string;
  label: string;
  build: () => Period;
};

const presets: PeriodPreset[] = [
  {
    key: "1w",
    label: "Last week",
    build: () => {
      const now = new Date();
      const start = new Date(now);
      start.setDate(now.getDate() - 7);
      return create(PeriodSchema, { type: PeriodType.WEEK, start: fmt(start), end: fmt(now) });
    },
  },
  {
    key: "2w",
    label: "Last two weeks",
    build: () => {
      const now = new Date();
      const start = new Date(now);
      start.setDate(now.getDate() - 14);
      return create(PeriodSchema, { type: PeriodType.WEEK, start: fmt(start), end: fmt(now) });
    },
  },
  {
    key: "1m",
    label: "Last month",
    build: () => {
      const now = new Date();
      const start = new Date(now);
      start.setMonth(now.getMonth() - 1);
      return create(PeriodSchema, { type: PeriodType.MONTH, start: fmt(start), end: fmt(now) });
    },
  },
  {
    key: "1q",
    label: "Last quarter",
    build: () => {
      const now = new Date();
      const start = new Date(now);
      start.setMonth(now.getMonth() - 3);
      return create(PeriodSchema, {
        type: PeriodType.QUARTER,
        start: fmt(start),
        end: fmt(now),
      });
    },
  },
  {
    key: "1y",
    label: "Last year",
    build: () => {
      const now = new Date();
      const start = new Date(now);
      start.setFullYear(now.getFullYear() - 1);
      return create(PeriodSchema, {
        type: PeriodType.QUARTER,
        start: fmt(start),
        end: fmt(now),
      });
    },
  },
  {
    key: "all",
    label: "All time",
    build: () =>
      create(PeriodSchema, {
        type: PeriodType.QUARTER,
        start: "2000-01-01",
        end: fmt(new Date()),
      }),
  },
];

export const defaultPeriodKey = "1m";

export const buildPeriod = (key: string): Period => {
  const preset = presets.find((p) => p.key === key);
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- presets[2] is "1m", always present
  return preset ? preset.build() : presets[2]!.build();
};

export const PeriodSelector = ({
  value,
  onChange,
}: {
  value: string;
  onChange: (key: string) => void;
}): React.ReactElement => (
  <ToggleGroup
    className="h-8 w-full rounded-lg bg-muted p-[3px] text-muted-foreground"
    value={[value]}
    onValueChange={(values) => {
      const selected = values[0];
      if (selected) onChange(selected);
    }}
  >
    {presets.map((p) => (
      <ToggleGroupItem
        key={p.key}
        value={p.key}
        className="h-[calc(100%-1px)] flex-1 rounded-md bg-transparent px-3 py-0.5 text-sm font-medium text-foreground/60 hover:bg-transparent hover:text-foreground aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-sm"
      >
        {p.label}
      </ToggleGroupItem>
    ))}
  </ToggleGroup>
);
