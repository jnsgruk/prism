import { atom } from "nanostores";

export interface AuthUser {
  userId: string;
  username: string;
  displayName: string;
  role: string;
}

export const $currentUser = atom<AuthUser | null>(null);
export const $isAuthenticated = atom(false);
