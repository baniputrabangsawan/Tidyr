// Shared app state: current screen, plan, settings. Kept tiny on purpose.

import { writable } from "svelte/store";
import type { ApplyResult, Plan, ScanOutcome, Settings } from "./api";

export type Screen =
  | "home"
  | "scan"
  | "plan"
  | "applying"
  | "done"
  | "history"
  | "settings";

export const screen = writable<Screen>("home");
export const scanOutcome = writable<ScanOutcome | null>(null);
export const plan = writable<Plan | null>(null);
export const applyResult = writable<ApplyResult | null>(null);
export const settings = writable<Settings | null>(null);
export const onboarded = writable<boolean>(
  typeof localStorage !== "undefined" &&
    localStorage.getItem("tidyr-onboarded") === "1",
);

export function markOnboarded() {
  try {
    localStorage.setItem("tidyr-onboarded", "1");
  } catch {
    /* private mode: onboarding just shows again next launch */
  }
  onboarded.set(true);
}
