import { SourceState } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import type { SourceStatus } from "@ps/api/gen/canonical/prism/v1/handlers_pb";

/** Polling intervals used by ingestion views. */
export const POLL_INTERVAL_BURST = 1_000;
export const POLL_INTERVAL_ACTIVE = 2_000;
export const POLL_INTERVAL_IDLE = 30_000;

/** Whether a source is currently running (collecting or waiting). */
export const isActive = (s: SourceStatus): boolean =>
  s.state === SourceState.COLLECTING || s.state === SourceState.WAITING;
