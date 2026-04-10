import { useCallback, useRef } from "react";

type ResizeAxis = "horizontal" | "vertical";

/**
 * Hook that returns an onPointerDown handler for a resize drag handle.
 *
 * During the drag, the target element's parent is resized directly via
 * `style.width` / `style.height` (no React re-renders) so the resize
 * feels instant. `onResize` is called once on pointer-up with the final
 * size, committing it to React state and triggering a single reflow.
 *
 * The drag handle element must have `data-current-size` set to the
 * current pixel size so the hook knows the starting value.
 *
 * Pass a `targetRef` to resize a specific element instead of the handle's
 * parent.
 */
export const useResize = ({
  axis,
  min,
  max,
  reverse = false,
  onResize,
  targetRef,
}: {
  axis: ResizeAxis;
  min: number;
  max: number;
  reverse?: boolean;
  onResize: (size: number) => void;
  targetRef?: React.RefObject<HTMLElement | null>;
}): { onPointerDown: React.PointerEventHandler } => {
  const startPos = useRef(0);
  const startSize = useRef(0);

  const onPointerDown = useCallback<React.PointerEventHandler>(
    (e) => {
      e.preventDefault();
      const el = e.currentTarget as HTMLElement;
      el.setPointerCapture(e.pointerId);

      startPos.current = axis === "horizontal" ? e.clientX : e.clientY;

      const encoded = el.dataset.currentSize;
      if (encoded) startSize.current = Number(encoded);

      const target = targetRef?.current ?? el.parentElement;

      const clamp = (raw: number): number => Math.round(Math.min(max, Math.max(min, raw)));

      // Suppress CSS transitions during drag so they don't fight direct style writes.
      const prevTransition = target?.style.transition ?? "";
      if (target) target.style.transition = "none";

      const onPointerMove = (ev: PointerEvent): void => {
        const delta = axis === "horizontal" ? ev.clientX - startPos.current : ev.clientY - startPos.current;
        const size = clamp(startSize.current + (reverse ? -delta : delta));

        // Directly mutate DOM — avoids React re-render on every frame.
        if (target) {
          if (axis === "horizontal") {
            target.style.width = `${size}px`;
          } else {
            target.style.height = `${size}px`;
          }
        }
      };

      const onPointerUp = (ev: PointerEvent): void => {
        document.removeEventListener("pointermove", onPointerMove);
        document.removeEventListener("pointerup", onPointerUp);

        // Restore transitions before committing so open/close animations still work.
        if (target) target.style.transition = prevTransition;

        // Commit final size to React state (single reflow).
        const delta = axis === "horizontal" ? ev.clientX - startPos.current : ev.clientY - startPos.current;
        onResize(clamp(startSize.current + (reverse ? -delta : delta)));
      };

      document.addEventListener("pointermove", onPointerMove);
      document.addEventListener("pointerup", onPointerUp);
    },
    [axis, min, max, reverse, onResize, targetRef],
  );

  return { onPointerDown };
};
