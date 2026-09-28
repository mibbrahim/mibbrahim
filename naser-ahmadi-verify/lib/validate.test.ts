import { test } from "node:test";
import assert from "node:assert/strict";
import { runChecks, statusFor, normalizePhone, isCaLicenseNumber } from "./validate.ts";
import { stateFromZip } from "./zip.ts";
import { parseAamva } from "./aamva.ts";
import { OTHER_PLAN, type IntakeForm } from "./types.ts";

const PLANS = ["Aetna PPO", "Cigna PPO"];
const TODAY = new Date("2026-09-28T12:00:00Z");

function form(over: Partial<IntakeForm> = {}): IntakeForm {
  return {
    phone: "(310) 555-0100",
    dob: "1985-04-12",
    license: {
      firstName: "Jane", lastName: "Doe", number: "D1234567", dob: "1985-04-12",
      expiration: "2029-04-12", street: "123 Main St", city: "Los Angeles", state: "CA",
      zip: "90012", scanned: false,
    },
    insurance: { plan: "Aetna PPO", otherPlanName: "", memberId: "W123456789", groupNumber: "" },
    cashPay: false,
    images: { licenseFront: "data:x", insuranceFront: "data:y" },
    ...over,
  };
}

test("valid California patient passes", () => {
  const checks = runChecks(form(), PLANS, TODAY);
  assert.deepEqual(checks.filter((c) => !c.ok), []);
  assert.equal(statusFor(checks, false), "verified");
});

test("DOB mismatch fails", () => {
  const c = runChecks(form({ dob: "1985-04-13" }), PLANS, TODAY).find((c) => c.id === "dob")!;
  assert.equal(c.ok, false);
});

test("out-of-state license and address fail", () => {
  const f = form();
  f.license = { ...f.license, state: "NV", zip: "89101" };
  const checks = runChecks(f, PLANS, TODAY);
  assert.equal(checks.find((c) => c.id === "ca_license")!.ok, false);
  assert.equal(checks.find((c) => c.id === "ca_address")!.detail, "ZIP 89101 is in NV");
});

test("expired license fails", () => {
  const f = form();
  f.license = { ...f.license, expiration: "2025-01-01" };
  assert.equal(runChecks(f, PLANS, TODAY).find((c) => c.id === "not_expired")!.ok, false);
});

test("plan not on list needs review, cash pay skips insurance checks", () => {
  const f = form({ insurance: { plan: OTHER_PLAN, otherPlanName: "Kaiser HMO", memberId: "1", groupNumber: "" } });
  assert.equal(statusFor(runChecks(f, PLANS, TODAY), false), "needs_review");
  const cash = { ...f, cashPay: true, images: { licenseFront: "data:x" } };
  const checks = runChecks(cash, PLANS, TODAY);
  assert.equal(checks.some((c) => c.id === "plan_accepted"), false);
  assert.equal(statusFor(checks, true), "cash_pay");
});

test("helpers", () => {
  assert.equal(normalizePhone("+1 (415) 555-0199"), "4155550199");
  assert.equal(normalizePhone("555-0199"), null);
  assert.equal(isCaLicenseNumber("d1234567"), true);
  assert.equal(isCaLicenseNumber("12345678"), false);
  assert.equal(stateFromZip("94103"), "CA");
  assert.equal(stateFromZip("96161"), "CA");
  assert.equal(stateFromZip("10001"), "NY");
  assert.equal(stateFromZip("abc"), null);
});

test("parses a California AAMVA barcode", () => {
  const raw =
    "@\n\x1e\rANSI 636014090002DL00410279ZC03200024DLDAQD1234567\nDCSDOE\nDDEN\nDACJANE\nDDFN\nDADMARIE\nDDGN\nDCAC\nDCBNONE\nDCDNONE\nDBD04122024\nDBB04121985\nDBA04122029\nDBC2\nDAU065 IN\nDAYBRO\nDAG123 MAIN ST\nDAILOS ANGELES\nDAJCA\nDAK900120000  \nDCF12345\nDCGUSA\n";
  assert.deepEqual(parseAamva(raw), {
    firstName: "Jane", lastName: "Doe", licenseNumber: "D1234567", dob: "1985-04-12",
    expiration: "2029-04-12", street: "123 Main St", city: "Los Angeles", state: "CA", zip: "90012",
  });
  assert.equal(parseAamva(raw.replace(/\n/g, "<LF>")).licenseNumber, "D1234567");
});
