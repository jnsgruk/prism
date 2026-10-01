export const isActivePipeline = (status: string | undefined): boolean =>
  status !== undefined && ["pending", "accepted", "running", "cancelling", "cancel_requested"].includes(status);
