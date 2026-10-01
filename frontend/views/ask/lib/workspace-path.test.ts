import { classifyWorkspaceReference, decodeWorkspacePath, workspaceDownloadHref } from "@/views/ask/lib/workspace-path";
import workspacePaths from "@fixtures/workspace-paths.json";
import { describe, expect, it } from "vite-plus/test";

describe("workspace path contract", () => {
  it.each(workspacePaths)("matches the shared contract for $encoded", ({ encoded, path }) => {
    expect(decodeWorkspacePath(encoded)).toBe(path);
  });
  it.each([
    "/workspace/nested/report%20one.pdf",
    "workspace/nested/report%20one.pdf",
    "nested/report%20one.pdf",
    "./nested/report%20one.pdf",
  ])("classifies %s", (href) => {
    expect(classifyWorkspaceReference(href)).toEqual({ path: "nested/report one.pdf" });
  });
  it("recognises same-origin workspace URLs and excludes remote origins and ordinary routes", () => {
    expect(classifyWorkspaceReference(`${window.location.origin}/workspace/report.pdf`)).toEqual({
      path: "report.pdf",
    });
    for (const href of [
      "https://remote.example/workspace/report.pdf",
      "/teams/person.pdf",
      "settings",
      "https://remote.example/file.pdf",
      "//remote.example/file.pdf",
    ])
      expect(classifyWorkspaceReference(href)).toBeNull();
  });
  it.each([
    "../file.pdf",
    "folder/%2e%2e/file.pdf",
    "folder//file.pdf",
    "folder/%2fetc.pdf",
    "%5cfile.pdf",
    "%00file.pdf",
    "%zz.pdf",
  ])("rejects %s", (path) => expect(decodeWorkspacePath(path)).toBeNull());
  it("round-trips literal escapes, unicode, spaces and nested filenames exactly once", () => {
    const path = "nested/100% #é.pdf";
    const href = workspaceDownloadHref("conversation", path);
    expect(decodeWorkspacePath(href.split("/files/")[1]!)).toBe(path);
    expect(decodeWorkspacePath("%252e%252e/report.pdf")).toBe("%2e%2e/report.pdf");
  });
  it("makes malformed generated references unavailable without crashing or enabling navigation", () => {
    expect(classifyWorkspaceReference("https://%bad")).toBeNull();
    expect(classifyWorkspaceReference("../report.pdf")).toEqual({ path: null });
    expect(classifyWorkspaceReference("%2E%2E/report.pdf")).toEqual({ path: null });
    expect(classifyWorkspaceReference("report%2Epdf")).toEqual({ path: "report.pdf" });
    expect(classifyWorkspaceReference("../guide")).toBeNull();
    expect(decodeWorkspacePath("%C2%80/report.pdf")).toBeNull();
    expect(decodeWorkspacePath("a".repeat(4097))).toBeNull();
  });
});
