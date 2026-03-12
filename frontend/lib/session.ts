const SESSION_KEY = "prism_session_token";

export const getSessionToken = (): string | null => {
  if (typeof window === "undefined") return null;
  return localStorage.getItem(SESSION_KEY);
};

export const setSessionToken = (token: string): void => {
  localStorage.setItem(SESSION_KEY, token);
};

export const clearSessionToken = (): void => {
  localStorage.removeItem(SESSION_KEY);
};
