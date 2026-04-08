import { useCallback, useRef } from "react";

type ResizeAxis = "horizontal" | "vertical";

/**
 * Hook that returns an onPointerDown handler for a resize drag handle.
 * While dragging, calls `onResize` with the new size in pixels.
 *
 * - `axis: "horizontal"` — drag left/right to resize width
 * - `axis: "vertical"` — drag up/down to resize height
 * - `reverse` — invert the drag direction (e.g. dragging left increases
 *   width for a right-anchored sidebar)
 */
export const useResize = ({
  axis,
  min,
  max,
  reverse = false,
  onResize,
}: {
  axis: ResizeAxis;
  min: number;
  max: number;
  reverse?: boolean;
  onResize: (size: number) => void;
}): { onPointerDown: React.PointerEventHandler } => {
  const startPos = useRef(0);
  const startSize = useRef(0);

  const onPointerDown = useCallback<React.PointerEventHandler>(
    (e) => {
      e.preventDefault();
      const el = e.currentTarget as HTMLElement;
      el.setPointerCapture(e.pointerId);

      startPos.current = axis === "horizontal" ? e.clientX : e.clientY;

      // Read the current size from the parent element.
      const parent = el.parentElement;
      if (parent) {
        startSize.current = axis === "horizontal" ? parent.getBoundingClientRect().width : 0;
      }
      // For vertical, read from the sibling (preview pane) rather than parent.
      // The caller passes the current size via CSS variable or we read from the
      // element. For simplicity, encode the current size as a data attribute.
      const encoded = el.dataset.currentSize;
      if (encoded) startSize.current = Number(encoded);

      const onPointerMove = (ev: PointerEvent): void => {
        const delta =
          axis === "horizontal" ? ev.clientX - startPos.current : ev.clientY - startPos.current;
        const rawSize = startSize.current + (reverse ? -delta : delta);
        onResize(Math.round(Math.min(max, Math.max(min, rawSize))));
      };

      const onPointerUp = (): void => {
        document.removeEventListener("pointermove", onPointerMove);
        document.removeEventListener("pointerup", onPointerUp);
      };

      document.addEventListener("pointermove", onPointerMove);
      document.addEventListener("pointerup", onPointerUp);
    },
    [axis, min, max, reverse, onResize],
  );

  return { onPointerDown };
};
