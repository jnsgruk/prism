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
  background: "hsl(var(--border))",
  border: "1.5px solid hsl(var(--border))",
};

export const StageNode = ({ data }: { data: StageNodeData }): React.ReactElement => (
  <div>
    <Handle type="target" position={Position.Left} style={handleStyle} />
    <PipelineStage
      stageKey={data.stageKey}
      stage={data.stage}
      isCurrentStage={data.isCurrentStage}
      sourceStatuses={data.sourceStatuses}
    />
    <Handle type="source" position={Position.Right} style={handleStyle} />
  </div>
);
