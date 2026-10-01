import { JiraAccountFields } from "@/views/admin/components/jira-account-fields";
import type { AccountDraft } from "@/views/admin/lib/person-form";
import { create } from "@bufbuild/protobuf";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vite-plus/test";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { SourceConfigSchema } from "@ps/api/gen/canonical/prism/v1/config_pb";
import { setupCleanup } from "@ps/test-utils";

vi.mock("@/views/admin/hooks/use-person-management", () => ({
  useLookupJiraAccounts: vi.fn<() => unknown>(() => ({
    isFetching: false,
    error: null,
    data: {
      accounts: [
        { accountId: "opaque-a", displayName: "Alex Smith", email: "alex@example.com", active: true },
        { accountId: "opaque-b", displayName: "Alex Smith", active: true },
      ],
    },
  })),
}));

const sources = ["First source", "Second source"].map((name) =>
  create(SourceConfigSchema, { id: name, name, sourceType: Platform.JIRA, enabled: true }),
);

const AccountForm = (): React.ReactElement => {
  const [account, setAccount] = useState<AccountDraft>({
    key: "jira",
    platform: Platform.JIRA,
    username: "",
    platformInstance: "",
    platformUserId: "",
  });

  return <JiraAccountFields account={account} sources={sources} onChange={setAccount} />;
};

const chooseSource = async (name: string): Promise<void> => {
  fireEvent.click(screen.getByRole("combobox", { name: "Jira lookup source" }));
  const option = await screen.findByRole("option", { name });
  fireEvent.pointerDown(option, { pointerType: "mouse" });
  fireEvent.click(option);
  await waitFor(() => expect(screen.getByRole("combobox", { name: "Jira lookup source" })).toHaveTextContent(name));
};

describe("Jira account selection", () => {
  setupCleanup();

  it.each(["search", "source"])("dismisses results after selection and reopens on a changed %s", async (change) => {
    render(<AccountForm />);
    await chooseSource("First source");
    fireEvent.change(screen.getByLabelText("Search Jira accounts"), { target: { value: "Alex" } });
    fireEvent.click(await screen.findByRole("button", { name: /Email hidden · opaque-b/ }));

    expect(screen.queryByRole("button", { name: /opaque-a/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /opaque-b/ })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Jira account ID (opaque ID, not email)")).toHaveValue("opaque-b");
    expect(screen.getByLabelText("Jira display name / username")).toHaveValue("Alex Smith");
    expect(screen.getByLabelText("Jira account ID (opaque ID, not email)")).toHaveFocus();

    if (change === "search")
      fireEvent.change(screen.getByLabelText("Search Jira accounts"), { target: { value: "Smith" } });
    else await chooseSource("Second source");

    fireEvent.click(await screen.findByRole("button", { name: /alex@example.com · opaque-a/ }));
    expect(screen.getByLabelText("Jira account ID (opaque ID, not email)")).toHaveValue("opaque-a");
    expect(screen.queryByRole("button", { name: /opaque-a/ })).not.toBeInTheDocument();
  });
});
