import { useCallback, useEffect, useRef } from "react";

/**
 * Manages auto-scrolling for a scrollable container.
 *
 * - Scrolls to the bottom whenever `deps` change, but only if the user
 *   hasn't scrolled away.
 * - Detects "user scrolled up" via the scroll event and pauses auto-scroll.
 * - Resumes when the user scrolls back to within `threshold` px of the bottom.
 */
export const useAutoScroll = (
  containerRef: React.RefObject<HTMLElement | null>,
  deps: React.DependencyList,
  { threshold = 64 }: { threshold?: number } = {},
): void => {
  // Track whether we should auto-scroll. Starts true (viewport begins at top
  // which is also the bottom when there's no content).
  const shouldScroll = useRef(true);

  // Track whether the last scroll event was programmatic so we can ignore it
  // in the user-scroll detection handler.
  const programmaticScroll = useRef(false);

  // --- Scroll-position listener ---
  // Determines if the user manually scrolled away from the bottom.
  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;

    // Ignore scroll events we caused ourselves.
    if (programmaticScroll.current) {
      programmaticScroll.current = false;
      return;
    }

    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    shouldScroll.current = distanceFromBottom <= threshold;
  }, [containerRef, threshold]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return undefined;
    el.addEventListener("scroll", handleScroll, { passive: true });
    return (): void => el.removeEventListener("scroll", handleScroll);
  }, [containerRef, handleScroll]);

  // --- Auto-scroll effect ---
  // Fires when deps change (new tokens, new messages, status changes).
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !shouldScroll.current) return;

    // Use instant scroll (assign scrollTop) to avoid animation queue-up.
    // requestAnimationFrame ensures the DOM has rendered the new content
    // before we measure scrollHeight.
    requestAnimationFrame(() => {
      if (!shouldScroll.current) return;
      programmaticScroll.current = true;
      el.scrollTop = el.scrollHeight;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
};
