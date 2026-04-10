import { Button } from "@/components/ui/button";
import { ReactFlow, ReactFlowProvider, useReactFlow, type Edge, type Node, type NodeTypes } from "@xyflow/react";
import { Scan } from "lucide-react";
import { useMemo } from "react";

import type { PipelineInfo } from "@ps/api/gen/canonical/prism/v1/handlers_pb";

import type { StageData, StageKey } from "./pipeline-stage";
import { StageNode, type StageNodeData } from "./stage-node";

type StagesMap = Record<string, StageData>;

const isStagesMap = (v: unknown): v is StagesMap => typeof v === "object" && v !== null;

const parseStages = (pipeline: PipelineInfo | undefined): StagesMap => {
  if (!pipeline?.stagesJson) return {};
  try {
    const parsed: unknown = JSON.parse(pipeline.stagesJson);
    return isStagesMap(parsed) ? parsed : {};
  } catch {
    return {};
  }
};

const nodeTypes: NodeTypes = { stage: StageNode };

// Static node positions — ingestion at top-left, branches centered to its right.
const NODE_POSITIONS: Record<StageKey, { x: number; y: number }> = {
  ingestion: { x: 0, y: 0 },
  metrics: { x: 210, y: 40 },
  enrichment: { x: 410, y: 40 },
  embedding: { x: 610, y: 40 },
  insights: { x: 810, y: 40 },
  identity_resolution: { x: 210, y: 120 },
};

const edgeDefaults = {
  type: "smoothstep" as const,
  style: { stroke: "var(--border)", strokeWidth: 1.5 },
};

const EDGES: Edge[] = [
  { id: "ingestion-metrics", source: "ingestion", target: "metrics", ...edgeDefaults },
  { id: "metrics-enrichment", source: "metrics", target: "enrichment", ...edgeDefaults },
  { id: "enrichment-embedding", source: "enrichment", target: "embedding", ...edgeDefaults },
  { id: "embedding-insights", source: "embedding", target: "insights", ...edgeDefaults },
  {
    id: "ingestion-identity",
    source: "ingestion",
    target: "identity_resolution",
    ...edgeDefaults,
  },
];

const STAGE_KEYS: StageKey[] = ["ingestion", "metrics", "enrichment", "embedding", "insights", "identity_resolution"];

const FitViewButton = (): React.ReactElement => {
  const { fitView } = useReactFlow();
  return (
    <Button
      variant="outline"
      size="icon"
      className="absolute right-2 top-2 z-10 size-7"
      onClick={() => fitView({ padding: 0.05, duration: 200 })}
    >
      <Scan className="size-3.5" />
    </Button>
  );
};

const FlowInner = ({
  pipeline,
  sourceStatuses,
}: {
  pipeline: PipelineInfo;
  sourceStatuses?: Map<string, string>;
}): React.ReactElement => {
  const stages = parseStages(pipeline);
  const currentStage = pipeline.currentStage;

  const nodes: Node<StageNodeData>[] = useMemo(
    () =>
      STAGE_KEYS.filter((key) => stages[key] !== undefined || key !== "identity_resolution").map((key) => ({
        id: key,
        type: "stage",
        position: NODE_POSITIONS[key],
        draggable: false,
        selectable: false,
        connectable: false,
        data: {
          stageKey: key,
          stage: stages[key],
          isCurrentStage: currentStage === key,
          sourceStatuses: key === "ingestion" ? sourceStatuses : undefined,
        },
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [pipeline.stagesJson, currentStage, sourceStatuses],
  );

  const edges = useMemo(() => EDGES.filter((e) => nodes.some((n) => n.id === e.target)), [nodes]);

  return (
    <div className="relative h-[200px]">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.05 }}
        panOnDrag
        zoomOnScroll
        minZoom={0.5}
        maxZoom={1.5}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        proOptions={{ hideAttribution: true }}
      />
      <FitViewButton />
    </div>
  );
};

export const PipelineDAGFlow = ({
  pipeline,
  sourceStatuses,
}: {
  pipeline: PipelineInfo;
  sourceStatuses?: Map<string, string>;
}): React.ReactElement => (
  <ReactFlowProvider>
    <FlowInner pipeline={pipeline} sourceStatuses={sourceStatuses} />
  </ReactFlowProvider>
);
