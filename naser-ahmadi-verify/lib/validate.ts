import { stateFromZip } from "./zip.ts";
import { OTHER_PLAN, type Check, type IntakeForm, type Submission } from "./types.ts";

export function digitsOnly(s: string): string {
  return s.replace(/\D/g, "");
}

// A US phone number, optionally with a leading 1.
export function normalizePhone(s: string): string | null {
  let d = digitsOnly(s);
  if (d.length === 11 && d.startsWith("1")) d = d.slice(1);
  return d.length === 10 ? d : null;
}

// California DL/ID numbers are one letter followed by seven digits.
export function isCaLicenseNumber(s: string): boolean {
  return /^[A-Z]\d{7}$/.test(s.trim().toUpperCase());
}

function isIsoDate(s: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s)) return false;
  const d = new Date(s + "T00:00:00Z");
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === s;
}

export function runChecks(form: IntakeForm, accepted: string[], today = new Date()): Check[] {
  const lic = form.license;
  const todayIso = today.toISOString().slice(0, 10);
  const zipState = stateFromZip(lic.zip);
  const plan = form.insurance.plan;
  const planName = plan === OTHER_PLAN ? form.insurance.otherPlanName.trim() : plan;

  const checks: Check[] = [
    {
      id: "phone",
      label: "Phone number",
      ok: normalizePhone(form.phone) !== null,
      detail: normalizePhone(form.phone) ? undefined : "Not a valid 10-digit US number",
    },
    {
      id: "dob",
      label: "Date of birth on license",
      ok: isIsoDate(lic.dob) && lic.dob < todayIso && lic.dob > "1900-01-01" && (!form.dob || form.dob === lic.dob),
      detail: !isIsoDate(lic.dob)
        ? "No date of birth read from the license"
        : form.dob && form.dob !== lic.dob
          ? `Entered ${form.dob}, license says ${lic.dob}`
          : lic.dob >= todayIso || lic.dob <= "1900-01-01"
            ? `Date of birth ${lic.dob} looks wrong`
            : undefined,
    },
    {
      id: "ca_license",
      label: "California driver's license / ID",
      ok: lic.state.toUpperCase() === "CA" && isCaLicenseNumber(lic.number),
      detail:
        lic.state.toUpperCase() !== "CA"
          ? `Issued in ${lic.state || "unknown state"}`
          : !isCaLicenseNumber(lic.number)
            ? "License number should be 1 letter + 7 digits"
            : undefined,
    },
    {
      id: "not_expired",
      label: "License not expired",
      ok: isIsoDate(lic.expiration) && lic.expiration >= todayIso,
      detail: !isIsoDate(lic.expiration)
        ? "No expiration date"
        : lic.expiration < todayIso
          ? `Expired ${lic.expiration}`
          : undefined,
    },
    {
      id: "ca_address",
      label: "California address",
      ok: zipState === "CA" && lic.street.trim() !== "" && lic.city.trim() !== "",
      detail:
        zipState !== "CA"
          ? zipState
            ? `ZIP ${lic.zip} is in ${zipState}`
            : "Missing or invalid ZIP code"
          : lic.street.trim() === "" || lic.city.trim() === ""
            ? "Street or city missing"
            : undefined,
    },
    {
      id: "license_photo",
      label: "License photo uploaded",
      ok: Boolean(form.images.licenseFront),
    },
  ];

  if (!form.cashPay) {
    checks.push(
      {
        id: "plan_accepted",
        label: "Insurance plan is accepted",
        ok: plan !== OTHER_PLAN && accepted.includes(plan),
        detail: plan === OTHER_PLAN || !accepted.includes(plan) ? `"${planName || "none"}" is not on the accepted list` : undefined,
      },
      {
        id: "member_id",
        label: "Insurance member ID",
        ok: form.insurance.memberId.trim().length >= 3,
      },
      {
        id: "insurance_photo",
        label: "Insurance card photo uploaded",
        ok: Boolean(form.images.insuranceFront),
      },
    );
  }

  return checks;
}

export function statusFor(checks: Check[], cashPay: boolean): Submission["status"] {
  if (!checks.every((c) => c.ok)) return "needs_review";
  return cashPay ? "cash_pay" : "verified";
}
