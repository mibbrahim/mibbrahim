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

test("DOB is optional but must match when given", () => {
  assert.equal(runChecks(form({ dob: "" }), PLANS, TODAY).find((c) => c.id === "dob")!.ok, true);
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

test("typed-in entries skip the photo checks", () => {
  const f = form({ manual: true, images: {} });
  const checks = runChecks(f, PLANS, TODAY);
  assert.equal(checks.some((c) => c.id.endsWith("_photo")), false);
  assert.equal(statusFor(checks, false), "verified");
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

import { parseInsuranceText } from "./insurance.ts";

test("reads member ID, group and payer from insurance card text", () => {
  const text = `aetna\nOpen Access Managed Choice\nMember: JANE DOE\nMember ID: W123 456 789\nGroup #: 0123456-01\nRx BIN 610502  PCN ADV\nPCP copay $25`;
  const g = parseInsuranceText(text, ["Aetna PPO", "Cigna PPO"]);
  assert.equal(g.memberId, "W123456789");
  assert.equal(g.groupNumber, "012345601");
  assert.equal(g.payer, "Aetna");
  assert.equal(g.plan, "Aetna PPO");
});

test("insurer not on the accepted list is reported but not matched", () => {
  const g = parseInsuranceText("KAISER PERMANENTE\nMRN 12345678\nID 000123456789", ["Aetna PPO"]);
  assert.equal(g.payer, "Kaiser Permanente");
  assert.equal(g.plan, undefined);
  assert.equal(g.memberId, "000123456789");
});

test("falls back to an ID-shaped token", () => {
  const g = parseInsuranceText("BlueShield of California\nJOHN SMITH\nXEH912345678\nGRP 5X001", ["Blue Shield of California PPO"]);
  assert.equal(g.memberId, "XEH912345678");
  assert.equal(g.groupNumber, "5X001");
  assert.equal(g.plan, "Blue Shield of California PPO");
});

import { parseLicenseText } from "./license.ts";

test("reads the front of a California license", () => {
  const text = `California USA DRIVER LICENSE\nDL D1234567\nEXP 04/12/2029\nLN DOE\nFN JANE MARIE\n123 MAIN ST\nLOS ANGELES, CA 90012\nDOB 04/12/1985\nSEX F HAIR BRN EYES BRO`;
  assert.deepEqual(parseLicenseText(text), {
    licenseNumber: "D1234567", expiration: "2029-04-12", dob: "1985-04-12", lastName: "Doe", firstName: "Jane",
    city: "Los Angeles", state: "CA", zip: "90012", street: "123 Main St",
  });
});

test("license front parsing tolerates missing fields", () => {
  const d = parseLicenseText("noise\nD7654321\nnothing else");
  assert.equal(d.licenseNumber, "D7654321");
  assert.equal(d.dob, undefined);
});
