import { describe, expect, it } from "vitest";

import { parseModelSelection } from "./model-selector";

describe("parseModelSelection", () => {
  it("returns isImageModel=false for undefined", () => {
    const result = parseModelSelection(undefined);
    expect(result.isImageModel).toBe(false);
    expect(result.imageModel).toBeUndefined();
    expect(result.chatModelOverride).toBeUndefined();
  });

  it("parses image model prefix", () => {
    const result = parseModelSelection("image:google/imagen-3");
    expect(result.isImageModel).toBe(true);
    expect(result.imageModel).toBe("google/imagen-3");
    expect(result.chatModelOverride).toBeUndefined();
  });

  it("returns chat model override for plain value", () => {
    const result = parseModelSelection("google/gemini-2.5-flash");
    expect(result.isImageModel).toBe(false);
    expect(result.chatModelOverride).toBe("google/gemini-2.5-flash");
    expect(result.imageModel).toBeUndefined();
  });

  it("handles image: prefix with nested model path", () => {
    const result = parseModelSelection("image:google/imagen-3.0-generate-002");
    expect(result.isImageModel).toBe(true);
    expect(result.imageModel).toBe("google/imagen-3.0-generate-002");
  });
});
