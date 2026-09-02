import { useSyncExternalStore } from "react";

const MOBILE_BREAKPOINT = 768;

const subscribe = (onChange: () => void): (() => void) => {
  const mql = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`);
  mql.addEventListener("change", onChange);
  return (): void => mql.removeEventListener("change", onChange);
};

const getSnapshot = (): boolean => window.innerWidth < MOBILE_BREAKPOINT;

export const useIsMobile = (): boolean => {
  const isMobile = useSyncExternalStore(subscribe, getSnapshot, () => false);

  return isMobile;
};
