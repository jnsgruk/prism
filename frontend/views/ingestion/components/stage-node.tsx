import { Handle, Position } from "@xyflow/react";

import type { StageData, StageKey } from "./pipeline-stage";
import { PipelineStage } from "./pipeline-stage";

export type StageNodeData = {
  stageKey: StageKey;
  stage: StageData | undefined;
  isCurrentStage: boolean;
  sourceStatuses?: Map<string, string>;
};

const handleStyle = {
  width: 7,
  height: 7,
  background: "var(--border)",
  border: "1.5px solid var(--border)",
};

/** Nodes with no incoming edges (first in a chain). */
const NO_TARGET: Set<StageKey> = new Set(["ingestion"]);
/** Nodes with no outgoing edges (last in a chain). */
const NO_SOURCE: Set<StageKey> = new Set(["insights", "identity_resolution"]);

export const StageNode = ({ data }: { data: StageNodeData }): React.ReactElement => (
  <div>
    {!NO_TARGET.has(data.stageKey) && <Handle type="target" position={Position.Left} style={handleStyle} />}
    <PipelineStage
      stageKey={data.stageKey}
      stage={data.stage}
      isCurrentStage={data.isCurrentStage}
      sourceStatuses={data.sourceStatuses}
    />
    {!NO_SOURCE.has(data.stageKey) && <Handle type="source" position={Position.Right} style={handleStyle} />}
  </div>
);
