"use client";

import { PageHeader } from "@/components/page-header";
import { Activity } from "lucide-react";

const IngestionPage = (): React.ReactElement => {
  return (
    <>
      <PageHeader title="Ingestion" description="Monitor data source ingestion runs" />
      <div className="flex-1 p-6">
        <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
          <Activity className="mb-3 size-10 text-muted-foreground" />
          <p className="mb-1 font-medium">Ingestion Monitoring</p>
          <p className="text-sm text-muted-foreground">
            Source ingestion monitoring will be implemented in a future workstream.
          </p>
        </div>
      </div>
    </>
  );
};

export default IngestionPage;
