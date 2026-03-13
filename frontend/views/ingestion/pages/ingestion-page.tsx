import { PageHeader } from "@/components/page-header";
import { Activity, Loader2 } from "lucide-react";
import { useState } from "react";

import { SourceState } from "@ps/api/gen/prism/v1/ingestion_pb";

import { IngestionRunsTable } from "@/views/ingestion/components/ingestion-runs-table";
import { SourceStatusCard } from "@/views/ingestion/components/source-status-card";
import { useIngestionStatus, useListRuns } from "@/views/ingestion/hooks/use-ingestion";

const POLL_INTERVAL_ACTIVE = 5_000;
const POLL_INTERVAL_IDLE = 30_000;

const IngestionPage = (): React.ReactElement => {
  const [selectedSource, setSelectedSource] = useState<string | undefined>();

  const { data: sources, isLoading: sourcesLoading } = useIngestionStatus({
    refetchInterval: (query) => {
      const data = query.state.data?.sources;
      const hasActive = data?.some(
        (s) => s.state === SourceState.COLLECTING || s.state === SourceState.WAITING,
      );
      return hasActive ? POLL_INTERVAL_ACTIVE : POLL_INTERVAL_IDLE;
    },
  });

  const { data: runs, isLoading: runsLoading } = useListRuns(selectedSource);

  if (sourcesLoading) {
    return (
      <>
        <PageHeader title="Ingestion" description="Monitor data source ingestion runs" />
        <div className="flex flex-1 items-center justify-center p-6">
          <Loader2 className="size-6 animate-spin text-muted-foreground" />
        </div>
      </>
    );
  }

  if (!sources || sources.length === 0) {
    return (
      <>
        <PageHeader title="Ingestion" description="Monitor data source ingestion runs" />
        <div className="flex-1 p-6">
          <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
            <Activity className="mb-3 size-10 text-muted-foreground" />
            <p className="mb-1 font-medium">No sources configured</p>
            <p className="text-sm text-muted-foreground">
              Add a data source in the Admin page to start ingesting data.
            </p>
          </div>
        </div>
      </>
    );
  }

  return (
    <>
      <PageHeader title="Ingestion" description="Monitor data source ingestion runs" />
      <div className="flex-1 space-y-6 p-6">
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {sources.map((source) => (
            <SourceStatusCard key={source.name} source={source} />
          ))}
        </div>

        <div>
          <div className="mb-3 flex items-center gap-3">
            <h2 className="text-lg font-semibold">Run History</h2>
            <div className="flex gap-1">
              <button
                onClick={() => setSelectedSource(undefined)}
                className={`rounded-md px-2 py-1 text-xs ${
                  selectedSource === undefined
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:text-foreground"
                }`}
              >
                All
              </button>
              {sources.map((source) => (
                <button
                  key={source.name}
                  onClick={() => setSelectedSource(source.name)}
                  className={`rounded-md px-2 py-1 text-xs ${
                    selectedSource === source.name
                      ? "bg-primary text-primary-foreground"
                      : "bg-muted text-muted-foreground hover:text-foreground"
                  }`}
                >
                  {source.name}
                </button>
              ))}
            </div>
          </div>
          {runsLoading ? (
            <div className="flex justify-center py-8">
              <Loader2 className="size-5 animate-spin text-muted-foreground" />
            </div>
          ) : (
            <IngestionRunsTable runs={runs ?? []} />
          )}
        </div>
      </div>
    </>
  );
};

export default IngestionPage;
