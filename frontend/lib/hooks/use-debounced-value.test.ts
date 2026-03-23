import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useDebouncedValue } from "./use-debounced-value";

describe("useDebouncedValue", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns initial value immediately", () => {
    const { result } = renderHook(() => useDebouncedValue("hello"));
    expect(result.current).toBe("hello");
  });

  it("updates after the default delay (300ms)", () => {
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value), {
      initialProps: { value: "initial" },
    });

    rerender({ value: "updated" });
    expect(result.current).toBe("initial");

    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(result.current).toBe("updated");
  });

  it("updates after a custom delay", () => {
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value, 500), {
      initialProps: { value: "initial" },
    });

    rerender({ value: "updated" });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(result.current).toBe("initial");

    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(result.current).toBe("updated");
  });

  it("resets timer on rapid changes, only applies last value", () => {
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value), {
      initialProps: { value: "a" },
    });

    rerender({ value: "b" });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    rerender({ value: "c" });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    rerender({ value: "d" });

    // None of the intermediate values should have taken effect
    expect(result.current).toBe("a");

    act(() => {
      vi.advanceTimersByTime(300);
    });
    // Only the last value should apply
    expect(result.current).toBe("d");
  });

  it("works with non-string types", () => {
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value), {
      initialProps: { value: 42 },
    });

    rerender({ value: 99 });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(result.current).toBe(99);
  });
});
