import { useEffect, useState } from "react";

/**
 * Returns a debounced version of the input value.
 * Updates the returned value only after `delay` ms of inactivity.
 */
export const useDebouncedValue = <T>(value: T, delay = 300): T => {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delay);
    return (): void => clearTimeout(timer);
  }, [value, delay]);

  return debounced;
};
