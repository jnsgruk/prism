"use client";

import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ArrowRight, Plug } from "lucide-react";
import Link from "next/link";

import { useListSources } from "@ps/hooks";

const DashboardPage = (): React.ReactElement => {
  const { data: sources } = useListSources();
  const hasSources = sources && sources.length > 0;

  return (
    <>
      <PageHeader title="Dashboard" />
      <div className="flex-1 p-6">
        {!hasSources ? (
          <Card className="mx-auto max-w-lg">
            <CardHeader className="text-center">
              <div className="mx-auto mb-2 flex size-12 items-center justify-center rounded-full bg-muted">
                <Plug className="size-6 text-muted-foreground" />
              </div>
              <CardTitle>Get started with Prism</CardTitle>
              <CardDescription>
                Connect your first data source to start gathering engineering insights across your
                team.
              </CardDescription>
            </CardHeader>
            <CardContent className="flex justify-center">
              <Button render={<Link href="/admin" />}>
                Configure Sources
                <ArrowRight className="size-4" />
              </Button>
            </CardContent>
          </Card>
        ) : (
          <Card className="mx-auto max-w-lg">
            <CardHeader className="text-center">
              <CardTitle>Welcome to Prism</CardTitle>
              <CardDescription>
                {sources.length} source{sources.length !== 1 ? "s" : ""} configured. Metrics and
                dashboards will appear here as data is ingested.
              </CardDescription>
            </CardHeader>
          </Card>
        )}
      </div>
    </>
  );
};

export default DashboardPage;
