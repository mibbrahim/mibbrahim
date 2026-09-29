import type { IntakeForm } from "./types.ts";

// PROTOTYPE ONLY. Sample values used to fill any field the scans couldn't read,
// so a demo always reaches a complete "Are these details correct?" screen.
// Turn off for real patients with DEMO_AUTOFILL=false.
export const DEMO_PHONE = "3105550147";

export const DEMO_LICENSE: Omit<IntakeForm["license"], "scanned"> = {
  firstName: "Maria",
  lastName: "Garcia",
  number: "D5823419",
  dob: "1988-06-14",
  expiration: "2030-06-14",
  street: "2450 Wilshire Blvd",
  city: "Los Angeles",
  state: "CA",
  zip: "90057",
};

export function demoInsurance(plans: string[]): Omit<IntakeForm["insurance"], "otherPlanName"> {
  return { plan: plans[0] ?? "", memberId: "W284719305", groupNumber: "0784512" };
}
